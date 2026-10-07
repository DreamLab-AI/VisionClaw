//! Client Coordinator Actor - WebSocket Communication Management
//!
//! This actor coordinates all client-related WebSocket communications, handling:
//! - Real-time position updates broadcasting
//! - Client connection state management
//! - Force broadcasts for new clients
//! - Initial client synchronization
//! - Adaptive broadcasting based on graph state
//!
//! ## Key Features
//! - **Time-based Broadcasting**: Prevents spam during stable periods
//! - **Force Broadcast Support**: Immediate updates for new clients
//! - **Efficient Binary Protocol**: Optimized WebSocket data transmission
//! - **Telemetry Integration**: Comprehensive logging and monitoring
//! - **Connection Tracking**: Manages client lifecycle and state
//!
//! ## Broadcasting Strategy
//! - **Active Periods**: 20Hz (50ms intervals) during graph changes
//! - **Stable Periods**: 1Hz (1s intervals) during settled states
//! - **New Client**: Immediate broadcast regardless of graph state
//! - **Binary Protocol**: 28-byte optimized node data for network efficiency

use actix::prelude::*;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

// Import required types and messages
use crate::actors::messages::*;
use crate::telemetry::agent_telemetry::{get_telemetry_logger, CorrelationId, Position3D};
use crate::utils::socket_flow_messages::BinaryNodeDataClient;

#[derive(Debug, Clone)]
pub struct ClientState {
    pub client_id: usize,
    /// ADR-090 A6-S4: erased recipients replace `Addr<SocketFlowServer>` so
    /// this actor (inner ring) no longer depends on the handler (outer ring).
    pub addr: ClientRecipients,
    pub connected_at: Instant,
    pub last_update: Instant,
    pub position_sent: bool,
    pub initial_sync_completed: bool,
    pub pubkey: Option<String>,
    pub is_power_user: bool,
    pub filter: ClientFilter,
    /// Per-user settings override. When set, the client has customised
    /// settings that differ from the global defaults. For the MVP this is
    /// stored and returned to the client on reconnect; actual per-user
    /// GPU computation is a follow-up.
    pub settings_override: Option<crate::config::AppFullSettings>,
    /// Whether this client authenticated with an ephemeral (dev-mode) identity
    pub ephemeral_session: bool,
}

/// Per-client filter settings for graph visibility
#[derive(Debug, Clone)]
pub struct ClientFilter {
    pub enabled: bool,
    pub quality_threshold: f64,
    pub authority_threshold: f64,
    pub filter_by_quality: bool,
    pub filter_by_authority: bool,
    pub filter_mode: FilterMode,
    pub max_nodes: Option<usize>,
    pub filtered_node_ids: std::collections::HashSet<u32>,
    pub include_linked_pages: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterMode {
    And,
    Or,
}

impl Default for ClientFilter {
    fn default() -> Self {
        Self {
            // DEFAULT FILTER ENABLED: Fresh clients get quality-filtered sparse dataset
            enabled: true,
            quality_threshold: 0.9,
            authority_threshold: 0.5,
            filter_by_quality: true,
            filter_by_authority: false,
            filter_mode: FilterMode::Or,
            max_nodes: None,
            filtered_node_ids: std::collections::HashSet::new(),
            include_linked_pages: false,
        }
    }
}

impl std::str::FromStr for FilterMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "and" => Ok(FilterMode::And),
            "or" => Ok(FilterMode::Or),
            _ => Err(format!("Invalid filter mode: {}", s)),
        }
    }
}

// ---------------------------------------------------------------------------
// ADR-031 gap 3b: Per-client reconnect message queue.
// Buffers messages for recently disconnected clients so they can be replayed
// on reconnect, closing the gap between disconnect and reconnect.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct DisconnectedClientQueue {
    /// Maps client_id to buffered binary messages since disconnect.
    buffers: HashMap<usize, VecDeque<Vec<u8>>>,
    /// When the client disconnected (for TTL eviction).
    disconnected_at: HashMap<usize, Instant>,
    /// Maximum number of messages buffered per client.
    max_buffer_per_client: usize,
    /// How long to keep buffers before eviction.
    ttl: Duration,
}

impl DisconnectedClientQueue {
    fn new(max_buffer_per_client: usize, ttl: Duration) -> Self {
        Self {
            buffers: HashMap::new(),
            disconnected_at: HashMap::new(),
            max_buffer_per_client,
            ttl,
        }
    }

    /// Start buffering for a newly disconnected client.
    fn track_disconnect(&mut self, client_id: usize) {
        self.buffers.entry(client_id).or_default();
        self.disconnected_at.insert(client_id, Instant::now());
    }

    /// Buffer a message for a disconnected client. Drops oldest if at capacity.
    fn push_message(&mut self, client_id: usize, data: Vec<u8>) {
        if let Some(buf) = self.buffers.get_mut(&client_id) {
            if buf.len() >= self.max_buffer_per_client {
                buf.pop_front(); // drop oldest
            }
            buf.push_back(data);
        }
    }

    /// Drain buffered messages for a reconnecting client, if any exist and TTL hasn't expired.
    /// Called by `replay_buffered_messages` when a client reconnects with its old ID.
    fn drain_for_reconnect(&mut self, client_id: usize) -> Vec<Vec<u8>> {
        // Check TTL
        if let Some(&disconnected) = self.disconnected_at.get(&client_id) {
            if disconnected.elapsed() > self.ttl {
                // Expired — discard.
                self.buffers.remove(&client_id);
                self.disconnected_at.remove(&client_id);
                return Vec::new();
            }
        }

        self.disconnected_at.remove(&client_id);
        self.buffers
            .remove(&client_id)
            .map(|q| q.into_iter().collect())
            .unwrap_or_default()
    }

    /// Evict stale entries past TTL. Called periodically.
    fn evict_stale(&mut self) {
        let now = Instant::now();
        let stale_ids: Vec<usize> = self
            .disconnected_at
            .iter()
            .filter(|(_, &t)| now.duration_since(t) > self.ttl)
            .map(|(&id, _)| id)
            .collect();

        for id in &stale_ids {
            self.buffers.remove(id);
            self.disconnected_at.remove(id);
        }

        if !stale_ids.is_empty() {
            debug!(
                "[DisconnectedClientQueue] Evicted {} stale client buffers",
                stale_ids.len()
            );
        }
    }

    /// Returns the set of currently tracked disconnected client IDs.
    fn tracked_client_ids(&self) -> Vec<usize> {
        self.buffers.keys().copied().collect()
    }
}

/// ADR-031 item 5: Result returned by broadcast methods.
///
/// `sent` is the number of clients that accepted the message.
///
/// `congested_clients` are clients whose socket-actor mailbox was full, so
/// this one frame was skipped for them. They stay registered: position frames
/// are full latest-wins snapshots, so the next frame that fits supersedes the
/// skipped one. A peer that never drains is reaped by the socket heartbeat
/// timeout, whose `stopped()` sends `UnregisterClient`.
///
/// `closed_clients` are clients whose socket actor has stopped. Callers MUST
/// evict these under a write lock AFTER releasing any read lock held during
/// the broadcast (see `ClientCoordinatorActor::settle_broadcast_result`).
#[derive(Debug, Default)]
pub struct BroadcastResult {
    pub sent: usize,
    pub congested_clients: Vec<usize>,
    pub closed_clients: Vec<usize>,
}

impl BroadcastResult {
    /// Try to hand `data` to one client and record the outcome.
    fn deliver_binary(&mut self, client_id: usize, client_state: &ClientState, data: Vec<u8>) {
        match client_state.addr.binary.try_send(SendToClientBinary(data)) {
            Ok(()) => self.sent += 1,
            // Full = the socket actor has not drained its mailbox yet (the peer
            // is momentarily not reading). Skip this frame for it; evicting it
            // here would silence an open socket for good.
            Err(actix::prelude::SendError::Full(_)) => {
                self.congested_clients.push(client_id);
            }
            Err(actix::prelude::SendError::Closed(_)) => {
                self.closed_clients.push(client_id);
            }
        }
    }
}

pub struct ClientManager {
    pub clients: HashMap<usize, ClientState>,
    pub next_id: usize,
    pub total_connections: usize,
    pub active_connections: usize,
}

/// Helper to convert RwLock poison errors to ActorError
fn handle_rwlock_error<T>(
    result: Result<T, std::sync::PoisonError<T>>,
) -> Result<T, crate::errors::ActorError> {
    result.map_err(|_| crate::errors::ActorError::RuntimeFailure {
        actor_name: "ClientCoordinatorActor".to_string(),
        reason: "RwLock poisoned - a thread panicked while holding the lock".to_string(),
    })
}

impl ClientManager {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            next_id: 1,
            total_connections: 0,
            active_connections: 0,
        }
    }

    pub fn register_client(&mut self, addr: ClientRecipients) -> usize {
        let client_id = self.next_id;
        self.next_id += 1;

        let now = Instant::now();
        let client_state = ClientState {
            client_id,
            addr,
            connected_at: now,
            last_update: now,
            position_sent: false,
            initial_sync_completed: false,
            pubkey: None,
            is_power_user: false,
            filter: ClientFilter::default(),
            settings_override: None,
            ephemeral_session: false,
        };

        self.clients.insert(client_id, client_state);
        self.total_connections += 1;
        self.active_connections = self.clients.len();

        debug!(
            "Client {} registered. Total active: {}",
            client_id, self.active_connections
        );
        client_id
    }

    pub fn get_client_mut(&mut self, client_id: usize) -> Option<&mut ClientState> {
        self.clients.get_mut(&client_id)
    }

    pub fn get_client(&self, client_id: usize) -> Option<&ClientState> {
        self.clients.get(&client_id)
    }

    pub fn unregister_client(&mut self, client_id: usize) -> bool {
        if self.clients.remove(&client_id).is_some() {
            self.active_connections = self.clients.len();
            debug!(
                "Client {} unregistered. Total active: {}",
                client_id, self.active_connections
            );
            true
        } else {
            warn!("Attempted to unregister non-existent client {}", client_id);
            false
        }
    }

    pub fn mark_client_synced(&mut self, client_id: usize) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.initial_sync_completed = true;
            client.last_update = Instant::now();
        }
    }

    pub fn update_client_timestamp(&mut self, client_id: usize) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.last_update = Instant::now();
        }
    }

    /// Broadcast raw bytes to every connected client.
    ///
    /// Uses `try_send` (ADR-031 item 5) to detect full or closed mailboxes.
    /// Returns a `BroadcastResult`; the caller evicts its `closed_clients`
    /// under a write lock after releasing any read lock.
    pub fn broadcast_to_all(&self, data: Vec<u8>) -> BroadcastResult {
        let mut result = BroadcastResult::default();
        for (&client_id, client_state) in &self.clients {
            result.deliver_binary(client_id, client_state, data.clone());
        }
        result
    }

    /// Broadcast with per-client filtering, including node type flags in binary protocol
    ///
    /// Pre-serializes the unfiltered payload once so that clients without active filters
    /// receive a cheap `Vec<u8>` clone instead of re-encoding per client.
    /// Complexity: O(N + F*N_f) where F = filtered-client count, N_f = per-filter node count.
    /// Broadcast position frames with per-client filtering.
    ///
    /// Pre-serialises the unfiltered payload once so clients without active
    /// filters get a cheap `Vec<u8>` clone instead of re-encoding per client.
    /// Uses `try_send` (ADR-031 item 5) to detect backpressure.
    /// Complexity: O(N + F×N_f) where F = filtered-client count, N_f = per-filter node count.
    pub fn broadcast_with_filter(
        &self,
        positions: &[BinaryNodeDataClient],
        node_type_arrays: &crate::actors::messages::NodeTypeArrays,
        broadcast_sequence: u64,
        analytics_data: Option<
            &std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
        >,
    ) -> BroadcastResult {
        if positions.is_empty() || self.clients.is_empty() {
            return BroadcastResult::default();
        }

        // Pre-serialize the full unfiltered payload ONCE
        let unfiltered_binary = self.serialize_positions(
            positions,
            node_type_arrays,
            broadcast_sequence,
            analytics_data,
        );

        let mut result = BroadcastResult::default();
        for (&client_id, client_state) in &self.clients {
            let payload = if !client_state.filter.enabled {
                // Send pre-serialized payload — no re-encoding needed
                Some(unfiltered_binary.clone())
            } else {
                // Only re-serialize for clients with active filters
                let filtered_positions: Vec<_> = positions
                    .iter()
                    .filter(|pos| client_state.filter.filtered_node_ids.contains(&pos.node_id))
                    .copied()
                    .collect();
                if filtered_positions.is_empty() {
                    None
                } else {
                    Some(self.serialize_positions(
                        &filtered_positions,
                        node_type_arrays,
                        broadcast_sequence,
                        analytics_data,
                    ))
                }
            };

            if let Some(data) = payload {
                result.deliver_binary(client_id, client_state, data);
            }
        }
        result
    }

    /// Serialize positions into V5 binary frame format.
    ///
    /// V5 wire format: `[1 byte: version=5][8 bytes: broadcast_sequence LE][V3 node data without version byte]`
    /// This embeds the authoritative server broadcast sequence so clients can echo it
    /// back in acks, enabling true end-to-end backpressure correlation.
    fn serialize_positions(
        &self,
        positions: &[BinaryNodeDataClient],
        nta: &crate::actors::messages::NodeTypeArrays,
        broadcast_sequence: u64,
        analytics_data: Option<
            &std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
        >,
    ) -> Vec<u8> {
        use crate::utils::binary_protocol::{encode_node_data_extended_with_sssp, NodeClassIds};
        use crate::utils::socket_flow_messages::BinaryNodeData;
        // Convert to (u32, BinaryNodeData) format for V3 protocol encoding
        let nodes: Vec<(u32, BinaryNodeData)> =
            positions.iter().map(|pos| (pos.node_id, *pos)).collect();
        let encoded = encode_node_data_extended_with_sssp(
            &nodes,
            NodeClassIds {
                agent: &nta.agent_ids,
                knowledge: &nta.knowledge_ids,
                ontology_class: &nta.ontology_class_ids,
                ontology_individual: &nta.ontology_individual_ids,
                ontology_property: &nta.ontology_property_ids,
            },
            None,
            analytics_data,
        );
        // Build V5 frame: [version=5][8-byte sequence LE][V3 node data without version byte]
        let mut result = Vec::with_capacity(1 + 8 + encoded.len().saturating_sub(1));
        result.push(5u8); // Protocol V5 = V3 nodes + embedded broadcast sequence
        result.extend_from_slice(&broadcast_sequence.to_le_bytes());
        if encoded.len() > 1 {
            result.extend_from_slice(&encoded[1..]); // node data without V3 version byte
        }
        result
    }

    pub fn broadcast_message(&self, message: String) -> usize {
        let mut broadcast_count = 0;
        for client_state in self.clients.values() {
            client_state
                .addr
                .text
                .do_send(SendToClientText(message.clone()));
            broadcast_count += 1;
        }
        broadcast_count
    }

    pub fn get_client_count(&self) -> usize {
        self.clients.len()
    }

    /// ADR-2134 same-user relay: hand `message` to every session authenticated
    /// as exactly `pubkey`, except `exclude`. Sessions with no pubkey or a
    /// different one never receive it. Returns the delivery count.
    pub fn relay_text_to_pubkey(
        &self,
        pubkey: &str,
        exclude: Option<usize>,
        message: &str,
    ) -> usize {
        if pubkey.is_empty() {
            return 0;
        }
        let mut n = 0;
        for (&id, client) in &self.clients {
            if Some(id) == exclude || client.pubkey.as_deref() != Some(pubkey) {
                continue;
            }
            if client
                .addr
                .text
                .try_send(SendToClientText(message.to_owned()))
                .is_ok()
            {
                n += 1;
            }
        }
        n
    }

    pub fn get_unsynced_clients(&self) -> Vec<usize> {
        self.clients
            .values()
            .filter(|client| !client.initial_sync_completed)
            .map(|client| client.client_id)
            .collect()
    }
}

impl Default for ClientManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ClientCoordinatorActor {
    client_manager: Arc<RwLock<ClientManager>>,

    last_broadcast: Instant,

    broadcast_interval: Duration,

    active_broadcast_interval: Duration,

    stable_broadcast_interval: Duration,

    initial_positions_sent: bool,

    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,

    /// GPU compute actor address for backpressure acknowledgements
    gpu_compute_addr: Option<Addr<crate::actors::gpu::force_compute_actor::ForceComputeActor>>,

    /// Broadcast sequence counter for acknowledgement correlation
    broadcast_sequence: u64,

    // Settings repository for loading/saving user filters (Oxigraph migration: ADR-11)
    settings_repository: Option<Arc<crate::adapters::SqliteSettingsRepository>>,

    position_cache: HashMap<u32, BinaryNodeDataClient>,

    broadcast_count: u64,
    bytes_sent: u64,

    force_broadcast_requests: u32,

    connection_stats: ConnectionStats,

    bandwidth_limit_bytes_per_sec: usize,
    bytes_sent_this_second: usize,
    last_bandwidth_check: Instant,

    pending_voice_data: Vec<Vec<u8>>,
    voice_data_queued_bytes: usize,

    /// Cached node type arrays from GraphStateActor for binary protocol flags
    node_type_arrays: crate::actors::messages::NodeTypeArrays,

    /// Shared node analytics data (NodeAnalytics) per node
    node_analytics: Arc<
        std::sync::RwLock<
            std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
        >,
    >,

    /// ADR-031 gap 3b: Per-client reconnect message queue.
    disconnected_queue: DisconnectedClientQueue,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionStats {
    pub total_registrations: usize,
    pub total_unregistrations: usize,
    pub current_clients: usize,
    pub peak_clients: usize,
    pub average_session_duration: Duration,
}

impl ClientCoordinatorActor {
    /// ADR-031 item 5: act on a broadcast's outcome. Closed clients are
    /// evicted; congested clients only had this frame skipped and stay
    /// registered (see `BroadcastResult`). `site` names the broadcast path
    /// in the log line.
    fn settle_broadcast_result(&self, result: &BroadcastResult, site: &str) {
        for id in &result.congested_clients {
            debug!(
                "[ClientCoordinator] Client {} mailbox full — skipped one frame ({})",
                id, site
            );
        }
        if result.closed_clients.is_empty() {
            return;
        }
        if let Ok(mut manager) = self.client_manager.write() {
            for id in &result.closed_clients {
                warn!(
                    "[ClientCoordinator] Evicting closed client {} ({})",
                    id, site
                );
                manager.unregister_client(*id);
            }
        }
    }

    pub fn new() -> Self {
        Self {
            client_manager: Arc::new(RwLock::new(ClientManager::new())),
            last_broadcast: Instant::now(),
            broadcast_interval: Duration::from_millis(50),
            active_broadcast_interval: Duration::from_millis(50),
            stable_broadcast_interval: Duration::from_millis(1000),
            initial_positions_sent: false,
            graph_service_addr: None,
            gpu_compute_addr: None,
            broadcast_sequence: 0,
            settings_repository: None,
            position_cache: HashMap::new(),
            broadcast_count: 0,
            bytes_sent: 0,
            force_broadcast_requests: 0,
            connection_stats: ConnectionStats::default(),
            bandwidth_limit_bytes_per_sec: 1_000_000,
            bytes_sent_this_second: 0,
            last_bandwidth_check: Instant::now(),
            pending_voice_data: Vec::new(),
            voice_data_queued_bytes: 0,
            node_type_arrays: crate::actors::messages::NodeTypeArrays::default(),
            node_analytics: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            disconnected_queue: DisconnectedClientQueue::new(
                64,                      // max 64 messages buffered per client
                Duration::from_secs(30), // 30-second TTL
            ),
        }
    }

    /// ADR-031 gap 3b: Buffer a binary message to all currently tracked disconnected clients.
    fn buffer_to_disconnected(&mut self, data: &[u8]) {
        let ids = self.disconnected_queue.tracked_client_ids();
        for id in ids {
            self.disconnected_queue.push_message(id, data.to_vec());
        }
    }

    /// ADR-031 gap 3b: Replay buffered messages to a specific client address.
    /// Called during RegisterClient when the new client_id matches a tracked
    /// disconnected client, draining any buffered binary messages.
    fn replay_buffered_messages(&mut self, old_client_id: usize, addr: &ClientRecipients) {
        let messages = self.disconnected_queue.drain_for_reconnect(old_client_id);
        if !messages.is_empty() {
            info!(
                "Replayed {} buffered messages to reconnected client {}",
                messages.len(),
                old_client_id
            );
            for msg in messages {
                addr.binary.do_send(SendToClientBinary(msg));
            }
        }
    }

    /// Set the shared node analytics map (NodeAnalytics)
    pub fn set_node_analytics(
        &mut self,
        analytics: Arc<
            std::sync::RwLock<
                std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
            >,
        >,
    ) {
        self.node_analytics = analytics;
        info!("Node analytics configured for ClientCoordinatorActor");
    }

    /// Set the GPU compute actor address for backpressure acknowledgements
    pub fn set_gpu_compute_addr(
        &mut self,
        addr: Addr<crate::actors::gpu::force_compute_actor::ForceComputeActor>,
    ) {
        self.gpu_compute_addr = Some(addr);
        info!("GPU compute address configured for ClientCoordinatorActor backpressure acks");
    }

    pub fn set_bandwidth_limit(&mut self, bytes_per_sec: usize) {
        self.bandwidth_limit_bytes_per_sec = bytes_per_sec;
        info!("Bandwidth limit set to {} bytes/sec", bytes_per_sec);
    }

    /// Set the SQLite settings repository for loading user filters (ADR-11)
    pub fn set_settings_repository(
        &mut self,
        repo: Arc<crate::adapters::SqliteSettingsRepository>,
    ) {
        self.settings_repository = Some(repo);
        info!("SQLite settings repository configured for ClientCoordinatorActor");
    }

    fn check_bandwidth_available(&mut self, bytes_needed: usize) -> bool {
        if self.bandwidth_limit_bytes_per_sec == 0 {
            return true;
        }

        if self.last_bandwidth_check.elapsed() >= Duration::from_secs(1) {
            self.bytes_sent_this_second = 0;
            self.last_bandwidth_check = Instant::now();
        }

        self.bytes_sent_this_second + bytes_needed <= self.bandwidth_limit_bytes_per_sec
    }

    fn record_bytes_sent(&mut self, bytes: usize) {
        self.bytes_sent_this_second += bytes;
        self.bytes_sent += bytes as u64;
    }

    pub fn queue_voice_data(&mut self, audio: Vec<u8>) {
        let audio_len = audio.len();
        self.voice_data_queued_bytes += audio_len;
        self.pending_voice_data.push(audio);
        debug!(
            "Queued voice data: {} bytes, total queued: {} bytes",
            audio_len, self.voice_data_queued_bytes
        );
    }

    fn send_prioritized_broadcasts(&mut self) -> Result<usize, String> {
        use crate::utils::binary_protocol::BinaryProtocol;

        let mut total_sent = 0;

        while !self.pending_voice_data.is_empty() {
            let voice_data_len = self.pending_voice_data[0].len();
            let encoded = BinaryProtocol::encode_voice_data(&self.pending_voice_data[0]);

            if !self.check_bandwidth_available(encoded.len()) {
                debug!(
                    "Bandwidth limit reached, deferring {} voice messages",
                    self.pending_voice_data.len()
                );
                break;
            }

            let broadcast_result = {
                let manager = match handle_rwlock_error(self.client_manager.read()) {
                    Ok(manager) => manager,
                    Err(e) => {
                        error!("RwLock error: {}", e);
                        return Err(format!("Failed to acquire client manager lock: {}", e));
                    }
                };
                manager.broadcast_to_all(encoded.clone())
            };
            self.settle_broadcast_result(&broadcast_result, "voice broadcast");
            self.record_bytes_sent(encoded.len());
            total_sent += broadcast_result.sent;

            self.voice_data_queued_bytes -= voice_data_len;
            self.pending_voice_data.remove(0);

            debug!(
                "Sent voice data: {} bytes to {} clients",
                encoded.len(),
                broadcast_result.sent
            );
        }

        if !self.position_cache.is_empty() && self.should_broadcast() {
            let mut position_data = Vec::new();
            for node_data in self.position_cache.values() {
                position_data.push(*node_data);
            }

            let binary_data = self.serialize_positions(&position_data);

            if self.check_bandwidth_available(binary_data.len()) {
                let broadcast_result = {
                    let manager = match handle_rwlock_error(self.client_manager.read()) {
                        Ok(manager) => manager,
                        Err(e) => {
                            error!("RwLock error: {}", e);
                            return Err(format!("Failed to acquire client manager lock: {}", e));
                        }
                    };
                    manager.broadcast_to_all(binary_data.clone())
                };
                self.settle_broadcast_result(&broadcast_result, "position cache");
                self.record_bytes_sent(binary_data.len());
                self.broadcast_count += 1;
                self.last_broadcast = Instant::now();
                total_sent += broadcast_result.sent;

                debug!(
                    "Sent graph update: {} nodes, {} bytes to {} clients",
                    position_data.len(),
                    binary_data.len(),
                    broadcast_result.sent
                );
            } else {
                debug!("Bandwidth limit reached, deferring graph update");
            }
        }

        Ok(total_sent)
    }

    pub fn set_graph_service_addr(&mut self, addr: Addr<crate::actors::GraphServiceSupervisor>) {
        self.graph_service_addr = Some(addr);
        debug!("Graph service address set in client coordinator");
    }

    pub fn update_broadcast_interval(&mut self, is_stable: bool) {
        let new_interval = if is_stable {
            self.stable_broadcast_interval
        } else {
            self.active_broadcast_interval
        };

        if new_interval != self.broadcast_interval {
            self.broadcast_interval = new_interval;
            debug!(
                "Broadcast interval updated: {}ms (stable: {})",
                new_interval.as_millis(),
                is_stable
            );
        }
    }

    pub fn should_broadcast(&self) -> bool {
        self.last_broadcast.elapsed() >= self.broadcast_interval
    }

    pub fn force_broadcast(&mut self, reason: &str) -> bool {
        info!("Force broadcasting positions: {}", reason);
        self.force_broadcast_requests += 1;

        let client_count = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return false;
                }
            };
            manager.get_client_count()
        };

        if client_count == 0 {
            debug!("No clients connected for force broadcast");
            return false;
        }

        let mut position_data = Vec::new();
        for node_data in self.position_cache.values() {
            position_data.push(*node_data);
        }

        if position_data.is_empty() {
            warn!(
                "Force broadcast requested but no position data available (reason: {})",
                reason
            );
            return false;
        }

        // Increment sequence BEFORE broadcast so it's embedded in the wire frame
        self.broadcast_sequence += 1;
        let current_sequence = self.broadcast_sequence;

        // Read analytics data for embedding in binary protocol
        let analytics_guard = self.node_analytics.read().ok();
        let analytics_ref = analytics_guard.as_deref();

        // Use per-client filtered broadcast for consistency with BroadcastPositions
        let result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return false;
                }
            };
            manager.broadcast_with_filter(
                &position_data,
                &self.node_type_arrays,
                current_sequence,
                analytics_ref,
            )
        };
        let broadcast_count = result.sent;
        self.settle_broadcast_result(&result, "force broadcast");

        // Approximate byte size (V5 protocol: 48 bytes per node + 1 header + 8 sequence)
        let approx_bytes = 1 + 8 + position_data.len() * 48;

        self.broadcast_count += 1;
        self.bytes_sent += approx_bytes as u64;
        self.last_broadcast = Instant::now();
        self.initial_positions_sent = true;

        // NOTE: No immediate PositionBroadcastAck here — that was a false ack
        // (queue-enqueue, not client receipt). Real acks come from ClientBroadcastAck
        // handler when clients confirm receipt, providing true end-to-end backpressure.

        if let Some(logger) = get_telemetry_logger() {
            let correlation_id = CorrelationId::new();
            logger.log_event(
                crate::telemetry::agent_telemetry::TelemetryEvent::new(
                    correlation_id,
                    crate::telemetry::agent_telemetry::LogLevel::INFO,
                    "client_coordinator",
                    "force_broadcast",
                    &format!(
                        "Force broadcast: {} nodes to {} clients (reason: {})",
                        position_data.len(),
                        broadcast_count,
                        reason
                    ),
                    "client_coordinator_actor",
                )
                .with_metadata("bytes_sent", serde_json::json!(approx_bytes))
                .with_metadata("client_count", serde_json::json!(broadcast_count))
                .with_metadata("reason", serde_json::json!(reason)),
            );
        }

        info!(
            "Force broadcast complete: {} nodes sent to {} clients (reason: {})",
            position_data.len(),
            broadcast_count,
            reason
        );
        true
    }

    fn serialize_positions(&self, positions: &[BinaryNodeDataClient]) -> Vec<u8> {
        use crate::utils::binary_protocol::encode_node_data_extended;
        use crate::utils::socket_flow_messages::BinaryNodeData;
        // Convert to (u32, BinaryNodeData) format for V3 protocol encoding
        let nodes: Vec<(u32, BinaryNodeData)> =
            positions.iter().map(|pos| (pos.node_id, *pos)).collect();
        let nta = &self.node_type_arrays;
        encode_node_data_extended(
            &nodes,
            &nta.agent_ids,
            &nta.knowledge_ids,
            &nta.ontology_class_ids,
            &nta.ontology_individual_ids,
            &nta.ontology_property_ids,
        )
    }

    /// Update cached node type arrays from GraphStateActor
    pub fn update_node_type_arrays(&mut self, arrays: crate::actors::messages::NodeTypeArrays) {
        info!(
            "Node type arrays updated: knowledge={}, agent={}, owl_class={}, owl_individual={}, owl_property={}",
            arrays.knowledge_ids.len(), arrays.agent_ids.len(), arrays.ontology_class_ids.len(),
            arrays.ontology_individual_ids.len(), arrays.ontology_property_ids.len()
        );
        self.node_type_arrays = arrays;
    }

    pub fn update_position_cache(&mut self, positions: Vec<(u32, BinaryNodeDataClient)>) {
        for (node_id, node_data) in positions {
            self.position_cache.insert(node_id, node_data);
        }
        debug!(
            "Position cache updated with {} nodes",
            self.position_cache.len()
        );
    }

    pub fn broadcast_positions(&mut self, is_stable: bool) -> Result<usize, String> {
        self.update_broadcast_interval(is_stable);

        let client_count = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.get_client_count()
        };

        if client_count == 0 {
            return Ok(0);
        }

        let force_broadcast = !self.initial_positions_sent;

        if !force_broadcast && !self.should_broadcast() {
            return Ok(0);
        }

        let mut position_data = Vec::new();
        for node_data in self.position_cache.values() {
            position_data.push(*node_data);
        }

        if position_data.is_empty() {
            return Err("No position data available for broadcast".to_string());
        }

        // Increment sequence BEFORE broadcast so it's embedded in the wire frame
        self.broadcast_sequence += 1;
        let current_sequence = self.broadcast_sequence;

        // Read analytics data for embedding in binary protocol
        let analytics_guard = self.node_analytics.read().ok();
        let analytics_ref = analytics_guard.as_deref();

        // Use per-client filtered broadcast for consistency with BroadcastPositions
        let result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.broadcast_with_filter(
                &position_data,
                &self.node_type_arrays,
                current_sequence,
                analytics_ref,
            )
        };
        let broadcast_count = result.sent;
        self.settle_broadcast_result(&result, "position broadcast");

        // Approximate byte size (V5 protocol: 48 bytes per node + 1 header + 8 sequence)
        let approx_bytes = 1 + 8 + position_data.len() * 48;

        self.broadcast_count += 1;
        self.bytes_sent += approx_bytes as u64;
        self.last_broadcast = Instant::now();

        // NOTE: No immediate PositionBroadcastAck here — that was a false ack
        // (queue-enqueue, not client receipt). Real acks come from ClientBroadcastAck
        // handler when clients confirm receipt, providing true end-to-end backpressure.

        if force_broadcast {
            self.initial_positions_sent = true;
            info!(
                "Sent initial positions to clients ({} nodes to {} clients)",
                position_data.len(),
                broadcast_count
            );
        }

        if crate::utils::logging::is_debug_enabled() && !force_broadcast {
            debug!(
                "Broadcast positions: {} nodes to {} clients, stable: {}",
                position_data.len(),
                broadcast_count,
                is_stable
            );
        }

        if force_broadcast || position_data.len() > 100 {
            if let Some(logger) = get_telemetry_logger() {
                let correlation_id = CorrelationId::new();
                logger.log_event(
                    crate::telemetry::agent_telemetry::TelemetryEvent::new(
                        correlation_id,
                        crate::telemetry::agent_telemetry::LogLevel::DEBUG,
                        "client_coordinator",
                        "position_broadcast",
                        &format!(
                            "Broadcast: {} nodes to {} clients",
                            position_data.len(),
                            broadcast_count
                        ),
                        "client_coordinator_actor",
                    )
                    .with_metadata("bytes_sent", serde_json::json!(approx_bytes))
                    .with_metadata("client_count", serde_json::json!(broadcast_count))
                    .with_metadata("is_initial", serde_json::json!(force_broadcast))
                    .with_metadata("is_stable", serde_json::json!(is_stable)),
                );
            }
        }

        Ok(broadcast_count)
    }

    fn generate_initial_position(&self, client_id: usize) -> Position3D {
        use rand::prelude::*;

        let mut rng = thread_rng();

        let radius = rng.gen_range(50.0..200.0);
        let theta = rng.gen_range(0.0..std::f32::consts::PI * 2.0);
        let phi = rng.gen_range(0.0..std::f32::consts::PI);

        let x = radius * phi.sin() * theta.cos();
        let y = radius * phi.sin() * theta.sin();
        let z = radius * phi.cos();

        let position = Position3D::new(x, y, z);

        info!(
            "Generated position for client {}: ({:.2}, {:.2}, {:.2}), magnitude: {:.2}",
            client_id, position.x, position.y, position.z, position.magnitude
        );

        if position.is_origin() {
            warn!(
                "ORIGIN POSITION BUG DETECTED: Client {} generated at origin despite parameters",
                client_id
            );
        }

        position
    }

    fn update_connection_stats(&mut self) {
        let manager = match handle_rwlock_error(self.client_manager.read()) {
            Ok(manager) => manager,
            Err(e) => {
                error!("RwLock error: {}", e);
                return;
            }
        };
        self.connection_stats.current_clients = manager.get_client_count();

        if self.connection_stats.current_clients > self.connection_stats.peak_clients {
            self.connection_stats.peak_clients = self.connection_stats.current_clients;
        }
    }

    pub fn get_stats(&self) -> ClientCoordinatorStats {
        let manager = match handle_rwlock_error(self.client_manager.read()) {
            Ok(manager) => manager,
            Err(e) => {
                error!("RwLock error: {}", e);
                return ClientCoordinatorStats {
                    active_clients: 0,
                    total_broadcasts: self.broadcast_count,
                    bytes_sent: self.bytes_sent,
                    force_broadcasts: self.force_broadcast_requests,
                    position_cache_size: self.position_cache.len(),
                    initial_positions_sent: self.initial_positions_sent,
                    current_broadcast_interval: self.broadcast_interval,
                    connection_stats: self.connection_stats.clone(),
                };
            }
        };
        ClientCoordinatorStats {
            active_clients: manager.get_client_count(),
            total_broadcasts: self.broadcast_count,
            bytes_sent: self.bytes_sent,
            force_broadcasts: self.force_broadcast_requests,
            position_cache_size: self.position_cache.len(),
            initial_positions_sent: self.initial_positions_sent,
            current_broadcast_interval: self.broadcast_interval,
            connection_stats: self.connection_stats.clone(),
        }
    }
}

impl Default for ClientCoordinatorActor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCoordinatorStats {
    pub active_clients: usize,
    pub total_broadcasts: u64,
    pub bytes_sent: u64,
    pub force_broadcasts: u32,
    pub position_cache_size: usize,
    pub initial_positions_sent: bool,
    pub current_broadcast_interval: Duration,
    pub connection_stats: ConnectionStats,
}

impl Actor for ClientCoordinatorActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("ClientCoordinatorActor started - WebSocket communication manager ready");

        // ADR-031 gap 3b: Periodic cleanup of stale disconnected client buffers (every 60s).
        ctx.run_interval(Duration::from_secs(60), |act, _ctx| {
            act.disconnected_queue.evict_stale();
        });

        if let Some(logger) = get_telemetry_logger() {
            let correlation_id = CorrelationId::new();
            logger.log_event(
                crate::telemetry::agent_telemetry::TelemetryEvent::new(
                    correlation_id,
                    crate::telemetry::agent_telemetry::LogLevel::INFO,
                    "actor_lifecycle",
                    "client_coordinator_start",
                    "Client Coordinator Actor started successfully",
                    "client_coordinator_actor",
                )
                .with_metadata(
                    "broadcast_interval_ms",
                    serde_json::json!(self.broadcast_interval.as_millis()),
                )
                .with_metadata(
                    "stable_interval_ms",
                    serde_json::json!(self.stable_broadcast_interval.as_millis()),
                )
                .with_metadata(
                    "active_interval_ms",
                    serde_json::json!(self.active_broadcast_interval.as_millis()),
                ),
            );
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        let stats = self.get_stats();
        info!(
            "ClientCoordinatorActor stopped - {} clients, {} broadcasts, {} bytes sent",
            stats.active_clients, stats.total_broadcasts, stats.bytes_sent
        );

        if let Some(logger) = get_telemetry_logger() {
            let correlation_id = CorrelationId::new();
            logger.log_event(
                crate::telemetry::agent_telemetry::TelemetryEvent::new(
                    correlation_id,
                    crate::telemetry::agent_telemetry::LogLevel::INFO,
                    "actor_lifecycle",
                    "client_coordinator_stop",
                    &format!(
                        "Client Coordinator Actor stopped - processed {} clients",
                        stats.active_clients
                    ),
                    "client_coordinator_actor",
                )
                .with_metadata(
                    "final_stats",
                    serde_json::to_value(&stats).unwrap_or_default(),
                ),
            );
        }
    }
}

// ===== MESSAGE HANDLERS =====

impl Handler<RegisterClient> for ClientCoordinatorActor {
    type Result = Result<usize, String>;

    fn handle(&mut self, msg: RegisterClient, _ctx: &mut Self::Context) -> Self::Result {
        let client_id = {
            let mut manager = match handle_rwlock_error(self.client_manager.write()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.register_client(msg.recipients)
        };

        let initial_position = self.generate_initial_position(client_id);

        self.connection_stats.total_registrations += 1;
        self.update_connection_stats();

        if let Some(logger) = get_telemetry_logger() {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert("client_id".to_string(), serde_json::json!(client_id));
            metadata.insert(
                "total_clients".to_string(),
                serde_json::json!(self.connection_stats.current_clients),
            );
            metadata.insert(
                "position_generation_method".to_string(),
                serde_json::json!("random_spherical"),
            );

            logger.log_agent_spawn(
                &format!("client_{}", client_id),
                None,
                initial_position,
                metadata,
            );
        }

        if !self.position_cache.is_empty() {
            self.force_broadcast(&format!("new_client_{}", client_id));
        } else {
            debug!("No position data available for new client {} - broadcast will occur when data is available", client_id);
        }

        // ADR-031 gap 3b: Replay buffered messages to reconnected client.
        // If this client_id was previously tracked in the disconnected queue
        // (e.g. from a rapid disconnect/reconnect cycle that reuses the same
        // slot, or from pubkey-matched reconnection), drain and replay.
        if self
            .disconnected_queue
            .tracked_client_ids()
            .contains(&client_id)
        {
            // Extract the addr under a short-lived read lock, then replay
            // outside the lock so the &mut self borrow doesn't conflict.
            let reconnected_addr = {
                match handle_rwlock_error(self.client_manager.read()) {
                    Ok(manager) => manager.clients.get(&client_id).map(|c| c.addr.clone()),
                    Err(_) => None,
                }
            };
            if let Some(addr) = reconnected_addr {
                self.replay_buffered_messages(client_id, &addr);
            }
        }

        // Recompute filter immediately so binary broadcasts respect include_linked_pages
        if let Some(graph_addr) = self.graph_service_addr.clone() {
            let manager_arc = self.client_manager.clone();
            actix::spawn(async move {
                use crate::actors::messages::GetGraphData;
                if let Ok(Ok(graph_data)) = graph_addr.send(GetGraphData).await {
                    if let Ok(mut manager) = manager_arc.write() {
                        if let Some(client) = manager.get_client_mut(client_id) {
                            crate::actors::client_filter::recompute_filtered_nodes(
                                &mut client.filter,
                                &graph_data,
                            );
                            info!("Initial filter for client {}: {} nodes visible (include_linked_pages={})",
                                  client_id, client.filter.filtered_node_ids.len(), client.filter.include_linked_pages);
                        }
                    }
                }
            });
        }

        info!(
            "Client {} registered successfully. Total clients: {}",
            client_id, self.connection_stats.current_clients
        );
        Ok(client_id)
    }
}

impl Handler<UnregisterClient> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UnregisterClient, _ctx: &mut Self::Context) -> Self::Result {
        let success = {
            let mut manager = match handle_rwlock_error(self.client_manager.write()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.unregister_client(msg.client_id)
        };

        if success {
            // ADR-031 gap 3b: Start buffering for the disconnected client so
            // messages can be replayed if they reconnect within the TTL window.
            self.disconnected_queue.track_disconnect(msg.client_id);
            debug!(
                "[ClientCoordinator] Client {} moved to disconnected queue (TTL: {:?})",
                msg.client_id, self.disconnected_queue.ttl
            );

            self.connection_stats.total_unregistrations += 1;
            self.update_connection_stats();

            if let Some(logger) = get_telemetry_logger() {
                let correlation_id =
                    CorrelationId::from_agent_id(&format!("client_{}", msg.client_id));
                logger.log_event(
                    crate::telemetry::agent_telemetry::TelemetryEvent::new(
                        correlation_id,
                        crate::telemetry::agent_telemetry::LogLevel::INFO,
                        "client_management",
                        "client_disconnect",
                        &format!("Client {} disconnected", msg.client_id),
                        "client_coordinator_actor",
                    )
                    .with_agent_id(&format!("client_{}", msg.client_id))
                    .with_metadata(
                        "remaining_clients",
                        serde_json::json!(self.connection_stats.current_clients),
                    ),
                );
            }

            info!(
                "Client {} unregistered successfully. Total clients: {}",
                msg.client_id, self.connection_stats.current_clients
            );
            Ok(())
        } else {
            let error_msg = format!("Failed to unregister client {}: not found", msg.client_id);
            error!("{}", error_msg);
            Err(error_msg)
        }
    }
}

impl Handler<BroadcastNodePositions> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: BroadcastNodePositions, _ctx: &mut Self::Context) -> Self::Result {
        // ADR-031 gap 3b: buffer to disconnected clients for reconnect replay.
        self.buffer_to_disconnected(&msg.positions);

        let broadcast_result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.broadcast_to_all(msg.positions.clone())
        };
        self.settle_broadcast_result(&broadcast_result, "node positions");
        let client_count = broadcast_result.sent;

        if client_count > 0 {
            self.broadcast_count += 1;
            self.bytes_sent += msg.positions.len() as u64;
            self.last_broadcast = Instant::now();

            debug!(
                "Broadcasted {} bytes to {} clients",
                msg.positions.len(),
                client_count
            );

            if msg.positions.len() > 1000 || client_count > 10 {
                info!(
                    "Large broadcast: {} bytes to {} clients",
                    msg.positions.len(),
                    client_count
                );

                if let Some(logger) = get_telemetry_logger() {
                    let correlation_id = CorrelationId::new();
                    logger.log_event(
                        crate::telemetry::agent_telemetry::TelemetryEvent::new(
                            correlation_id,
                            crate::telemetry::agent_telemetry::LogLevel::INFO,
                            "client_coordinator",
                            "large_broadcast",
                            &format!(
                                "Large broadcast: {} bytes to {} clients",
                                msg.positions.len(),
                                client_count
                            ),
                            "client_coordinator_actor",
                        )
                        .with_metadata("bytes_sent", serde_json::json!(msg.positions.len()))
                        .with_metadata("client_count", serde_json::json!(client_count)),
                    );
                }
            }
        }

        Ok(())
    }
}

/// Handler for `BroadcastAgentActionFrame` (ADR-059 §4, Phase 2b).
///
/// Fans out a pre-encoded `0x23 AGENT_ACTION` wire frame to every connected
/// client, reusing the exact same per-client `send_binary` dispatch loop as
/// `BroadcastNodePositions` (`ClientManager::broadcast_to_all` → each client's
/// `addr.binary.try_send(SendToClientBinary(..))`). No new client registry, no
/// duplicated dispatch: the agent-beam frame rides the established binary path.
impl Handler<BroadcastAgentActionFrame> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: BroadcastAgentActionFrame, _ctx: &mut Self::Context) -> Self::Result {
        let frame = msg.0;
        let frame_len = frame.len();

        let broadcast_result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error in BroadcastAgentActionFrame: {}", e);
                    return;
                }
            };
            manager.broadcast_to_all(frame)
        };

        self.settle_broadcast_result(&broadcast_result, "agent-action frame");

        let client_count = broadcast_result.sent;
        if client_count > 0 {
            self.broadcast_count += 1;
            self.bytes_sent += frame_len as u64;
            self.last_broadcast = Instant::now();
            debug!(
                "Broadcasted agent-action frame ({} bytes) to {} clients",
                frame_len, client_count
            );
        }
    }
}

/// Handler for BroadcastPositions - modern position broadcasting with backpressure ack
impl Handler<BroadcastPositions> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: BroadcastPositions, _ctx: &mut Self::Context) -> Self::Result {
        // Increment sequence BEFORE broadcast so it's embedded in the wire frame
        self.broadcast_sequence += 1;
        let current_sequence = self.broadcast_sequence;

        // Read analytics data for embedding in binary protocol
        let analytics_guard = self.node_analytics.read().ok();
        let analytics_ref = analytics_guard.as_deref();

        let result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error in BroadcastPositions: {}", e);
                    return;
                }
            };
            manager.broadcast_with_filter(
                &msg.positions,
                &self.node_type_arrays,
                current_sequence,
                analytics_ref,
            )
        };
        let client_count = result.sent;
        self.settle_broadcast_result(&result, "BroadcastPositions");

        if client_count > 0 {
            self.broadcast_count += 1;
            // Approximate byte size (V5 protocol: 48 bytes per node + 1 header + 8 sequence)
            let approx_bytes = 1 + 8 + msg.positions.len() * 48;
            self.bytes_sent += approx_bytes as u64;
            self.last_broadcast = Instant::now();

            // NOTE: No immediate PositionBroadcastAck here — that was a false ack
            // (queue-enqueue, not client receipt). Real acks come from ClientBroadcastAck
            // handler when clients confirm receipt, providing true end-to-end backpressure.

            debug!(
                "Broadcasted {} node positions to {} clients (~{} bytes), seq: {}",
                msg.positions.len(),
                client_count,
                approx_bytes,
                self.broadcast_sequence
            );
        }
    }
}

impl Handler<BroadcastMessage> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: BroadcastMessage, _ctx: &mut Self::Context) -> Self::Result {
        // ADR-031 gap 3b: buffer text messages for disconnected clients.
        self.buffer_to_disconnected(msg.message.as_bytes());

        let client_count = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.broadcast_message(msg.message.clone())
        };

        if client_count > 0 {
            debug!(
                "Broadcasted message to {} clients: {}",
                client_count,
                if msg.message.len() > 100 {
                    format!("{}...", &msg.message[..100])
                } else {
                    msg.message.clone()
                }
            );
        }

        Ok(())
    }
}

impl Handler<GetClientCount> for ClientCoordinatorActor {
    type Result = Result<usize, String>;

    fn handle(&mut self, _msg: GetClientCount, _ctx: &mut Self::Context) -> Self::Result {
        let count = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.get_client_count()
        };
        Ok(count)
    }
}

impl Handler<ForcePositionBroadcast> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ForcePositionBroadcast, _ctx: &mut Self::Context) -> Self::Result {
        if self.force_broadcast(&msg.reason) {
            Ok(())
        } else {
            let error_msg = format!("Force broadcast failed: {}", msg.reason);
            warn!("{}", error_msg);
            Err(error_msg)
        }
    }
}

impl Handler<InitialClientSync> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitialClientSync, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Initial client sync requested by {} from {}",
            msg.client_identifier, msg.trigger_source
        );

        let broadcast_reason = format!(
            "initial_sync_{}_{}",
            msg.client_identifier, msg.trigger_source
        );

        if self.force_broadcast(&broadcast_reason) {
            if let Ok(client_id) = msg.client_identifier.parse::<usize>() {
                let mut manager = match handle_rwlock_error(self.client_manager.write()) {
                    Ok(manager) => manager,
                    Err(e) => {
                        error!("RwLock error: {}", e);
                        return Err(format!("Failed to acquire client manager lock: {}", e));
                    }
                };
                manager.mark_client_synced(client_id);
            }

            info!(
                "Initial sync broadcast complete for client {} from {}",
                msg.client_identifier, msg.trigger_source
            );
            Ok(())
        } else {
            let error_msg = format!(
                "Initial sync failed for client {} - no position data available",
                msg.client_identifier
            );
            warn!("{}", error_msg);
            Err(error_msg)
        }
    }
}

impl Handler<UpdateNodePositions> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateNodePositions, _ctx: &mut Self::Context) -> Self::Result {
        let mut client_positions = Vec::new();
        for (node_id, node_data) in msg.positions {
            let client_data = BinaryNodeDataClient {
                node_id: node_data.node_id,
                x: node_data.x,
                y: node_data.y,
                z: node_data.z,
                vx: node_data.vx,
                vy: node_data.vy,
                vz: node_data.vz,
            };
            client_positions.push((node_id, client_data));
        }

        self.update_position_cache(client_positions);

        let client_count = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.get_client_count()
        };

        if client_count > 0 {
            let unsynced_clients = {
                let manager = match handle_rwlock_error(self.client_manager.read()) {
                    Ok(manager) => manager,
                    Err(e) => {
                        error!("RwLock error: {}", e);
                        return Err(format!("Failed to acquire client manager lock: {}", e));
                    }
                };
                manager.get_unsynced_clients()
            };

            let force_broadcast = !unsynced_clients.is_empty() || !self.initial_positions_sent;

            if force_broadcast {
                self.force_broadcast("position_update_with_unsynced_clients");
            } else {
                self.broadcast_positions(false)?;
            }
        }

        debug!(
            "Updated position cache with {} nodes for {} clients",
            self.position_cache.len(),
            client_count
        );
        Ok(())
    }
}

impl Handler<SetGraphServiceAddress> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: SetGraphServiceAddress, _ctx: &mut Self::Context) -> Self::Result {
        debug!("Setting graph service address in client coordinator");
        self.set_graph_service_addr(msg.addr);
    }
}

/// Handler for UpdateNodeTypeArrays - caches node type classification for binary protocol flags
impl Handler<UpdateNodeTypeArrays> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateNodeTypeArrays, _ctx: &mut Self::Context) -> Self::Result {
        self.update_node_type_arrays(msg.arrays);
    }
}

/// Handler for SetGpuComputeAddress - enables backpressure acknowledgements
impl Handler<SetGpuComputeAddress> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: SetGpuComputeAddress, _ctx: &mut Self::Context) -> Self::Result {
        self.set_gpu_compute_addr(msg.addr);
    }
}

/// Handler for ClientBroadcastAck - true end-to-end backpressure flow control
/// Forwards client ACKs to GPU actor to replenish tokens based on actual client receipt
/// This replaces queue-only ACKs with application-level acknowledgements
impl Handler<ClientBroadcastAck> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: ClientBroadcastAck, _ctx: &mut Self::Context) -> Self::Result {
        // Forward the acknowledgement to GPU actor for backpressure token restoration
        if let Some(ref gpu_addr) = self.gpu_compute_addr {
            gpu_addr.do_send(PositionBroadcastAck {
                correlation_id: msg.sequence_id,
                clients_delivered: 1, // Each client ACK counts as 1 delivery
            });

            // Log at trace level to avoid spam (every 100th ACK at debug)
            if msg.sequence_id.is_multiple_of(100) {
                debug!(
                    "ClientBroadcastAck: seq={}, nodes={}, client_timestamp={}ms, client_id={:?}",
                    msg.sequence_id, msg.nodes_received, msg.timestamp, msg.client_id
                );
            }
        } else {
            // GPU address not set, log warning once per 1000 ACKs
            if msg.sequence_id.is_multiple_of(1000) {
                warn!("ClientBroadcastAck: GPU compute address not set, cannot forward ACK");
            }
        }
    }
}

#[derive(Message)]
#[rtype(result = "Result<ClientCoordinatorStats, String>")]
pub struct GetClientCoordinatorStats;

impl Handler<GetClientCoordinatorStats> for ClientCoordinatorActor {
    type Result = Result<ClientCoordinatorStats, String>;

    fn handle(
        &mut self,
        _msg: GetClientCoordinatorStats,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(self.get_stats())
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct QueueVoiceData {
    pub audio: Vec<u8>,
}

impl Handler<QueueVoiceData> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: QueueVoiceData, _ctx: &mut Self::Context) -> Self::Result {
        self.queue_voice_data(msg.audio);

        match self.send_prioritized_broadcasts() {
            Ok(count) => {
                debug!("Voice data queued and {} broadcasts sent", count);
                Ok(())
            }
            Err(e) => {
                warn!(
                    "Failed to send prioritized broadcasts after queuing voice: {}",
                    e
                );
                Ok(())
            }
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SetBandwidthLimit {
    pub bytes_per_sec: usize,
}

impl Handler<SetBandwidthLimit> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: SetBandwidthLimit, _ctx: &mut Self::Context) -> Self::Result {
        self.set_bandwidth_limit(msg.bytes_per_sec);
    }
}

impl Handler<RelayToUserSessions> for ClientCoordinatorActor {
    type Result = usize;

    fn handle(&mut self, msg: RelayToUserSessions, _ctx: &mut Self::Context) -> Self::Result {
        match handle_rwlock_error(self.client_manager.read()) {
            Ok(manager) => {
                manager.relay_text_to_pubkey(&msg.pubkey, msg.exclude_client_id, &msg.message)
            }
            Err(e) => {
                error!("RwLock error: {}", e);
                0
            }
        }
    }
}

/// Handle client authentication
impl Handler<AuthenticateClient> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: AuthenticateClient, ctx: &mut Self::Context) -> Self::Result {
        let mut manager = match handle_rwlock_error(self.client_manager.write()) {
            Ok(manager) => manager,
            Err(e) => {
                error!("RwLock error: {}", e);
                return Err(format!("Failed to acquire client manager lock: {}", e));
            }
        };

        if let Some(client) = manager.get_client_mut(msg.client_id) {
            client.pubkey = Some(msg.pubkey.clone());
            client.is_power_user = msg.is_power_user;
            client.ephemeral_session = msg.ephemeral;
            info!(
                "Client {} authenticated as pubkey {} (power_user: {}, ephemeral: {})",
                msg.client_id, msg.pubkey, msg.is_power_user, msg.ephemeral
            );

            // Load saved filter and settings from SQLite repository if available.
            // Phase 2: get_user_filter / get_user_settings are not yet on SqliteSettingsRepository.
            // todo!("Phase 2: migrate user filter/settings load from Cypher to SPARQL/SQLite path")
            if let Some(_settings_repo) = self.settings_repository.clone() {
                // Per-user filter persistence is queued for Phase 2 (ADR-11 §D5).
                // For now, all clients use the global default filter on auth.
                info!(
                    "User filter persistence pending Phase 2 SQLite migration — using defaults for pubkey {}",
                    msg.pubkey
                );
            } else {
                warn!(
                    "Settings repository not configured, using default filter for client {}",
                    msg.client_id
                );
            }

            // Recompute if filter is enabled (applies regardless of repository availability)
            if client.filter.enabled && self.settings_repository.is_none() {
                if let Some(graph_addr) = self.graph_service_addr.clone() {
                    let client_id = msg.client_id;
                    let manager_arc = self.client_manager.clone();

                    ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                        use crate::actors::messages::GetGraphData;
                        match graph_addr.send(GetGraphData).await {
                            Ok(Ok(graph_data)) => {
                                if let Ok(mut manager) = manager_arc.write() {
                                    if let Some(client) = manager.get_client_mut(client_id) {
                                        crate::actors::client_filter::recompute_filtered_nodes(
                                            &mut client.filter,
                                            &graph_data
                                        );
                                        info!("Recomputed filter for authenticated client {}: {} nodes visible",
                                              client_id, client.filter.filtered_node_ids.len());
                                    }
                                }
                            }
                            Err(e) => warn!("Failed to fetch graph data for filter recomputation: {}", e),
                            Ok(Err(e)) => warn!("Graph data fetch error: {}", e),
                        }
                    }).map(|_, _, _| ()));
                }
            }

            Ok(())
        } else {
            Err(format!("Client {} not found", msg.client_id))
        }
    }
}

/// Handle filter updates from client
impl Handler<UpdateClientFilter> for ClientCoordinatorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateClientFilter, ctx: &mut Self::Context) -> Self::Result {
        let mut manager = match handle_rwlock_error(self.client_manager.write()) {
            Ok(manager) => manager,
            Err(e) => {
                error!("RwLock error: {}", e);
                return Err(format!("Failed to acquire client manager lock: {}", e));
            }
        };

        if let Some(client) = manager.get_client_mut(msg.client_id) {
            let filter_mode = msg
                .filter_mode
                .parse::<FilterMode>()
                .map_err(|e| format!("Invalid filter mode: {}", e))?;

            client.filter.enabled = msg.enabled;
            client.filter.quality_threshold = msg.quality_threshold;
            client.filter.authority_threshold = msg.authority_threshold;
            client.filter.filter_by_quality = msg.filter_by_quality;
            client.filter.filter_by_authority = msg.filter_by_authority;
            client.filter.filter_mode = filter_mode;
            client.filter.include_linked_pages = msg.include_linked_pages;

            info!(
                "Updated filter for client {}: enabled={}, quality_threshold={}",
                msg.client_id, msg.enabled, msg.quality_threshold
            );

            // Recompute filtered nodes with updated criteria and send filtered graph to client
            if let Some(graph_addr) = self.graph_service_addr.clone() {
                let client_id = msg.client_id;
                let manager_arc = self.client_manager.clone();

                ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                    use crate::actors::messages::GetGraphData;
                    use crate::utils::socket_flow_messages::{InitialNodeData, InitialEdgeData};

                    match graph_addr.send(GetGraphData).await {
                        Ok(Ok(graph_data)) => {
                            if let Ok(mut manager) = manager_arc.write() {
                                if let Some(client) = manager.get_client_mut(client_id) {
                                    // Recompute which nodes pass the filter
                                    crate::actors::client_filter::recompute_filtered_nodes(
                                        &mut client.filter,
                                        &graph_data
                                    );

                                    let filtered_count = client.filter.filtered_node_ids.len();
                                    info!("Recomputed filter for client {}: {} nodes visible",
                                          client_id, filtered_count);

                                    // Build filtered node data
                                    let filtered_nodes: Vec<InitialNodeData> = graph_data
                                        .nodes
                                        .iter()
                                        .filter(|n| client.filter.filtered_node_ids.contains(&n.id))
                                        .map(|node| InitialNodeData {
                                            id: node.id,
                                            metadata_id: node.metadata_id.clone(),
                                            label: node.label.clone(),
                                            x: node.data.x,
                                            y: node.data.y,
                                            z: node.data.z,
                                            vx: node.data.vx,
                                            vy: node.data.vy,
                                            vz: node.data.vz,
                                            owl_class_iri: node.owl_class_iri.clone(),
                                            node_type: node.node_type.clone(),
                                            did_nostr: node.metadata.get("did_nostr").cloned(),
                                            metadata: node.metadata.clone(),
                                        })
                                        .collect();

                                    // Build filtered edge data (only edges where both endpoints pass filter)
                                    let filtered_edges: Vec<InitialEdgeData> = graph_data
                                        .edges
                                        .iter()
                                        .filter(|e| {
                                            client.filter.filtered_node_ids.contains(&e.source) &&
                                            client.filter.filtered_node_ids.contains(&e.target)
                                        })
                                        .map(|edge| InitialEdgeData {
                                            id: edge.id.clone(),
                                            source_id: edge.source,
                                            target_id: edge.target,
                                            weight: Some(edge.weight),
                                            edge_type: edge.edge_type.clone(),
                                            inferred: crate::services::inferred_edge_materialiser::edge_is_inferred(edge),
                                        })
                                        .collect();

                                    info!("Sending filtered graph to client {}: {} nodes, {} edges",
                                          client_id, filtered_nodes.len(), filtered_edges.len());

                                    // Send filtered graph data to this specific client
                                    client.addr.initial_load.do_send(SendInitialGraphLoad {
                                        nodes: filtered_nodes,
                                        edges: filtered_edges,
                                    });
                                }
                            }
                        }
                        Err(e) => warn!("Failed to fetch graph data for filter recomputation: {}", e),
                        Ok(Err(e)) => warn!("Graph data fetch error: {}", e),
                    }
                }).map(|_, _, _| ()));
            } else {
                // Clear filtered_node_ids if we can't fetch graph data
                client.filter.filtered_node_ids.clear();
            }

            Ok(())
        } else {
            Err(format!("Client {} not found", msg.client_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every text frame it is handed (stands in for a SocketFlowServer).
    struct Probe(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    impl Actor for Probe {
        type Context = Context<Self>;
    }
    impl Handler<SendToClientText> for Probe {
        type Result = ();
        fn handle(&mut self, m: SendToClientText, _: &mut Self::Context) {
            self.0.lock().unwrap().push(m.0);
        }
    }
    impl Handler<SendToClientBinary> for Probe {
        type Result = ();
        fn handle(&mut self, _: SendToClientBinary, _: &mut Self::Context) {}
    }
    impl Handler<crate::actors::messages::SendInitialGraphLoad> for Probe {
        type Result = ();
        fn handle(
            &mut self,
            _: crate::actors::messages::SendInitialGraphLoad,
            _: &mut Self::Context,
        ) {
        }
    }

    fn probe() -> (
        ClientRecipients,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let addr = Probe(seen.clone()).start();
        let r = ClientRecipients {
            binary: addr.clone().recipient(),
            text: addr.clone().recipient(),
            initial_load: addr.recipient(),
        };
        (r, seen)
    }

    #[actix::test]
    async fn relay_reaches_only_the_same_pubkeys_other_sessions() {
        let mut m = ClientManager::new();
        let (r_desk, desk) = probe();
        let (r_xr, xr) = probe();
        let (r_other, other) = probe();
        let (r_anon, anon) = probe();
        let desk_id = m.register_client(r_desk);
        let xr_id = m.register_client(r_xr);
        let other_id = m.register_client(r_other);
        let _anon_id = m.register_client(r_anon);
        m.get_client_mut(desk_id).unwrap().pubkey = Some("alice".into());
        m.get_client_mut(xr_id).unwrap().pubkey = Some("alice".into());
        m.get_client_mut(other_id).unwrap().pubkey = Some("bob".into());

        let n = m.relay_text_to_pubkey("alice", Some(desk_id), "{\"type\":\"beatClock\"}");
        assert_eq!(n, 1, "only alice's other session");
        assert_eq!(
            m.relay_text_to_pubkey("", None, "x"),
            0,
            "empty pubkey reaches nobody"
        );
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(xr.lock().unwrap().as_slice(), ["{\"type\":\"beatClock\"}"]);
        assert!(
            desk.lock().unwrap().is_empty(),
            "never echoed to the sender"
        );
        assert!(other.lock().unwrap().is_empty(), "never cross-user");
        assert!(
            anon.lock().unwrap().is_empty(),
            "never to an unauthenticated session"
        );
    }

    /// Counts binary frames it is handed (stands in for a SocketFlowServer
    /// whose socket write side is momentarily not being polled).
    struct BinaryCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Actor for BinaryCounter {
        type Context = Context<Self>;
    }
    impl Handler<SendToClientBinary> for BinaryCounter {
        type Result = ();
        fn handle(&mut self, _: SendToClientBinary, _: &mut Self::Context) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    impl Handler<SendToClientText> for BinaryCounter {
        type Result = ();
        fn handle(&mut self, _: SendToClientText, _: &mut Self::Context) {}
    }
    impl Handler<crate::actors::messages::SendInitialGraphLoad> for BinaryCounter {
        type Result = ();
        fn handle(
            &mut self,
            _: crate::actors::messages::SendInitialGraphLoad,
            _: &mut Self::Context,
        ) {
        }
    }

    fn counter() -> (
        ClientRecipients,
        Addr<BinaryCounter>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let addr = BinaryCounter(n.clone()).start();
        let r = ClientRecipients {
            binary: addr.clone().recipient(),
            text: addr.clone().recipient(),
            initial_load: addr.clone().recipient(),
        };
        (r, addr, n)
    }

    /// Regression: a client whose mailbox is momentarily full (the browser's
    /// main thread was busy, so its socket was not drained for a few seconds)
    /// used to be unregistered from the broadcast registry while its socket
    /// stayed open. It then received no position frame ever again. A full
    /// snapshot is latest-wins, so the right response to `Full` is to skip
    /// that one frame for that client and keep it registered.
    #[actix::test]
    async fn full_mailbox_skips_the_frame_and_keeps_the_client() {
        let mut m = ClientManager::new();
        let (r, _addr, frames) = counter();
        let id = m.register_client(r);
        m.get_client_mut(id).unwrap().filter.enabled = false;
        let positions = vec![BinaryNodeDataClient {
            node_id: 1,
            x: 1.0,
            y: 2.0,
            z: 3.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        }];
        let nta = crate::actors::messages::NodeTypeArrays::default();

        // The receiver is not polled until we yield, so this fills its mailbox.
        let mut congested_seen = false;
        for seq in 0..64u64 {
            let to_all = m.broadcast_to_all(vec![0u8; 8]);
            let filtered = m.broadcast_with_filter(&positions, &nta, seq, None);
            for res in [&to_all, &filtered] {
                assert!(
                    res.closed_clients.is_empty(),
                    "a congested client must not be marked for eviction"
                );
                if res.congested_clients.contains(&id) {
                    congested_seen = true;
                }
            }
        }
        assert!(
            congested_seen,
            "the mailbox should have filled at least once"
        );
        assert!(m.get_client_mut(id).is_some(), "client stays registered");

        // Once the receiver drains, broadcasts reach it again.
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        let before = frames.load(std::sync::atomic::Ordering::SeqCst);
        let res = m.broadcast_with_filter(&positions, &nta, 99, None);
        assert_eq!(res.sent, 1, "the recovered client receives the next frame");
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(frames.load(std::sync::atomic::Ordering::SeqCst), before + 1);
    }

    /// A client whose socket actor has stopped is still reported for eviction.
    #[actix::test]
    async fn closed_mailbox_is_reported_for_eviction() {
        struct StopsAtOnce;
        impl Actor for StopsAtOnce {
            type Context = Context<Self>;
            fn started(&mut self, ctx: &mut Self::Context) {
                ctx.stop();
            }
        }
        impl Handler<SendToClientBinary> for StopsAtOnce {
            type Result = ();
            fn handle(&mut self, _: SendToClientBinary, _: &mut Self::Context) {}
        }
        impl Handler<SendToClientText> for StopsAtOnce {
            type Result = ();
            fn handle(&mut self, _: SendToClientText, _: &mut Self::Context) {}
        }
        impl Handler<crate::actors::messages::SendInitialGraphLoad> for StopsAtOnce {
            type Result = ();
            fn handle(
                &mut self,
                _: crate::actors::messages::SendInitialGraphLoad,
                _: &mut Self::Context,
            ) {
            }
        }
        let addr = StopsAtOnce.start();
        let r = ClientRecipients {
            binary: addr.clone().recipient(),
            text: addr.clone().recipient(),
            initial_load: addr.recipient(),
        };
        let mut m = ClientManager::new();
        let id = m.register_client(r);
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        let res = m.broadcast_to_all(vec![0u8; 8]);
        assert_eq!(res.closed_clients, vec![id]);
        assert!(res.congested_clients.is_empty());
    }

    #[test]
    fn test_client_manager_registration() {
        let manager = ClientManager::new();
        assert_eq!(manager.get_client_count(), 0);
    }

    #[test]
    fn test_position_serialization() {
        let actor = ClientCoordinatorActor::new();
        let positions = vec![BinaryNodeDataClient {
            node_id: 1,
            x: 1.0,
            y: 2.0,
            z: 3.0,
            vx: 0.1,
            vy: 0.2,
            vz: 0.3,
        }];

        let serialized = actor.serialize_positions(&positions);
        // V3 protocol (ADR-031 analytics extension): 1 header byte + 52 bytes per node
        assert_eq!(serialized.len(), 1 + 52);
    }

    #[test]
    fn test_broadcast_timing() {
        let mut actor = ClientCoordinatorActor::new();

        // Immediately after creation, last_broadcast is set to now, so should NOT broadcast
        assert!(!actor.should_broadcast());

        // Set last_broadcast to the past (beyond broadcast_interval of 50ms)
        actor.last_broadcast = Instant::now() - Duration::from_millis(100);

        // Now should broadcast since elapsed > broadcast_interval
        assert!(actor.should_broadcast());
    }
}
