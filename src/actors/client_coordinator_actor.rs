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
//! - **Binary Protocol**: 28-byte node data per ADR-061

use actix::prelude::*;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

// Import required types and messages
use crate::actors::messages::*;
use crate::handlers::socket_flow_handler::SocketFlowServer;
use crate::telemetry::agent_telemetry::{get_telemetry_logger, CorrelationId, Position3D};
use crate::utils::socket_flow_messages::BinaryNodeDataClient;

#[derive(Debug)]
pub struct ClientState {
    pub client_id: usize,
    pub addr: Addr<SocketFlowServer>,
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
    pub settings_override: Option<crate::config::app_settings::AppFullSettings>,
    /// Whether this client authenticated with an ephemeral (dev-mode) identity
    pub ephemeral_session: bool,
    /// Number of consecutive `SendError::Full` events on the client's mailbox.
    /// Reset to 0 on a successful binary push. Evicted when this exceeds
    /// `MAILBOX_FULL_EVICTION_THRESHOLD` — preventing a permanently-stuck
    /// client from holding 2048 queued frames forever, while still tolerating
    /// the initial-burst window (browser GPU shader compile + 25k-node mesh build)
    /// that motivated drop-on-Full in Fix D.
    ///
    /// `AtomicU32` enables in-place mutation through the read-locked broadcast
    /// hot path without escalating to a write-lock or restructuring the API.
    pub consecutive_full_frames: std::sync::atomic::AtomicU32,
}

/// Number of consecutive frame-drop events after which a client is considered
/// genuinely stuck (rather than experiencing a transient initial-burst stall)
/// and should be evicted. At ~60 Hz broadcast cadence, 60 frames = ~1 second
/// of sustained backpressure.
pub const MAILBOX_FULL_EVICTION_THRESHOLD: u32 = 60;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterMode {
    And,
    Or,
}

impl Default for ClientFilter {
    fn default() -> Self {
        Self {
            // Filter DISABLED by default. With `enabled: true` and an empty
            // `filtered_node_ids` set, `broadcast_with_filter` produces no payload
            // for fresh clients — so newly-connected subscribers silently receive
            // zero position frames until they proactively push a `filter_update`
            // with `enabled: false` (which the UI does, but headless/test clients
            // don't). Default to wide-open: clients that want filtering opt in.
            enabled: false,
            quality_threshold: 0.7,
            authority_threshold: 0.5,
            filter_by_quality: true,
            filter_by_authority: false,
            filter_mode: FilterMode::Or,
            max_nodes: Some(10000),
            filtered_node_ids: std::collections::HashSet::new(),
        }
    }
}

/// ADR-061 (supersedes ADR-050 §H2): build the `private_hidden_ids` set for a
/// given caller.
///
/// For every private node in `nta.private_node_owners` whose `owner_pubkey`
/// does NOT match the caller, insert its node_id into the returned set.
///
/// Per ADR-061 §D3, the wire no longer carries an opacification flag bit;
/// privacy is enforced by DROPPING the node from the position frame entirely
/// for non-owners. Callers receive this set and use it to filter the
/// positions vec before encoding.
///
/// Anonymous callers (`caller_pubkey == None`) see no private nodes.
/// Owners see their own private nodes with full fidelity.
pub fn compute_private_opaque_ids(
    nta: &crate::actors::messages::NodeTypeArrays,
    caller_pubkey: Option<&str>,
) -> std::collections::HashSet<u32> {
    let mut set = std::collections::HashSet::new();
    for (node_id, owner) in &nta.private_node_owners {
        match caller_pubkey {
            Some(caller) if !caller.is_empty() && caller == owner.as_str() => {
                // caller is the owner — full fidelity, do NOT hide.
            }
            _ => {
                set.insert(*node_id);
            }
        }
    }
    set
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
        self.buffers.entry(client_id).or_insert_with(VecDeque::new);
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
/// `slow_clients` contains IDs of clients whose actor mailbox was full or
/// closed (backpressure detected). Callers MUST evict these clients under a
/// write lock AFTER releasing any read lock held during the broadcast.
#[derive(Debug, Default)]
pub struct BroadcastResult {
    pub sent: usize,
    pub slow_clients: Vec<usize>,
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

    pub fn register_client(&mut self, addr: Addr<SocketFlowServer>) -> usize {
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
            consecutive_full_frames: std::sync::atomic::AtomicU32::new(0),
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
    /// Returns a `BroadcastResult` whose `slow_clients` list the caller
    /// should evict under a write lock after releasing any read lock.
    pub fn broadcast_to_all(&self, data: Vec<u8>) -> BroadcastResult {
        use std::sync::atomic::Ordering;
        let mut sent = 0;
        let mut slow_clients = Vec::new();
        for (&client_id, client_state) in &self.clients {
            match client_state.addr.try_send(SendToClientBinary(data.clone())) {
                Ok(()) => {
                    sent += 1;
                    // Successful push — clear the sustained-Full counter so a
                    // client that briefly stalls but recovers isn't penalised.
                    client_state
                        .consecutive_full_frames
                        .store(0, Ordering::Relaxed);
                }
                Err(actix::prelude::SendError::Full(_)) => {
                    // Drop frame on transient backpressure but track sustained pressure.
                    // Eviction on a single Full strands the client on JSON-only fallback
                    // (the original Fix D bug). Eviction never strands the client when
                    // pressure is transient (initial-burst stall), but a permanently
                    // stuck client is removed after ~1s of continuous Fulls.
                    let n = client_state
                        .consecutive_full_frames
                        .fetch_add(1, Ordering::Relaxed)
                        + 1;
                    if n >= MAILBOX_FULL_EVICTION_THRESHOLD {
                        warn!(
                            "[ClientCoordinator] Client {} mailbox full for {} consecutive frames — evicting",
                            client_id, n
                        );
                        slow_clients.push(client_id);
                    } else {
                        debug!(
                            "[ClientCoordinator] Client {} mailbox full — dropping frame ({} consecutive)",
                            client_id, n
                        );
                    }
                }
                Err(actix::prelude::SendError::Closed(_)) => {
                    slow_clients.push(client_id);
                }
            }
        }
        BroadcastResult { sent, slow_clients }
    }

    /// Broadcast position frames with per-client visibility and filter.
    ///
    /// ADR-061 §D3: privacy is enforced here by DROPPING positions for nodes
    /// the caller may not see (via `compute_private_opaque_ids`) before
    /// encoding. The encoder no longer knows about visibility — this function
    /// is the choke point.
    ///
    /// Pre-serialises the unfiltered payload once per visibility-set so
    /// clients with the same caller identity share a `Vec<u8>` clone instead
    /// of re-encoding. Uses `try_send` (ADR-031 item 5) to detect
    /// backpressure.
    ///
    /// `node_type_arrays` is consulted for the private-node owner table; per
    /// ADR-061 it is no longer used to flag wire ids.
    pub fn broadcast_with_filter(
        &self,
        positions: &[BinaryNodeDataClient],
        node_type_arrays: &crate::actors::messages::NodeTypeArrays,
        broadcast_sequence: u64,
    ) -> BroadcastResult {
        if positions.is_empty() || self.clients.is_empty() {
            return BroadcastResult::default();
        }

        let mut sent = 0;
        let mut slow_clients = Vec::new();
        // Cache encoded buffers keyed by the sorted set of hidden ids — every
        // client with the same caller identity will produce the same set, so
        // we serialise once per identity and clone the bytes per send.
        let mut unfiltered_cache: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
        for (&client_id, client_state) in &self.clients {
            let private_hidden_ids =
                compute_private_opaque_ids(node_type_arrays, client_state.pubkey.as_deref());

            let payload = if !client_state.filter.enabled {
                let mut key_vec: Vec<u32> = private_hidden_ids.iter().copied().collect();
                key_vec.sort_unstable();
                let mut key_bytes = Vec::with_capacity(key_vec.len() * 4);
                for id in &key_vec {
                    key_bytes.extend_from_slice(&id.to_le_bytes());
                }
                let entry = unfiltered_cache.entry(key_bytes).or_insert_with(|| {
                    let visible: Vec<BinaryNodeDataClient> = positions
                        .iter()
                        .filter(|pos| !private_hidden_ids.contains(&pos.node_id))
                        .copied()
                        .collect();
                    self.serialize_positions(&visible, broadcast_sequence)
                });
                Some(entry.clone())
            } else {
                // Per-client filter: subset positions to those allowed by the
                // filter AND drop privately-owned nodes the caller can't see.
                let filtered_positions: Vec<_> = positions
                    .iter()
                    .filter(|pos| {
                        client_state.filter.filtered_node_ids.contains(&pos.node_id)
                            && !private_hidden_ids.contains(&pos.node_id)
                    })
                    .copied()
                    .collect();
                if filtered_positions.is_empty() {
                    None
                } else {
                    Some(self.serialize_positions(&filtered_positions, broadcast_sequence))
                }
            };

            if let Some(data) = payload {
                use std::sync::atomic::Ordering;
                match client_state.addr.try_send(SendToClientBinary(data)) {
                    Ok(()) => {
                        sent += 1;
                        client_state
                            .consecutive_full_frames
                            .store(0, Ordering::Relaxed);
                    }
                    Err(actix::prelude::SendError::Full(_)) => {
                        // Drop the frame; track sustained pressure so a permanently
                        // stuck client doesn't accumulate a 2048-deep queue indefinitely.
                        // Transient initial-burst stalls (browser GPU shader compile,
                        // 25k-node mesh build, React StrictMode double-mount) recover
                        // well below the threshold; only genuine zombies get evicted.
                        let n = client_state
                            .consecutive_full_frames
                            .fetch_add(1, Ordering::Relaxed)
                            + 1;
                        if n >= MAILBOX_FULL_EVICTION_THRESHOLD {
                            warn!(
                                "[ClientCoordinator] Client {} mailbox full for {} consecutive frames — evicting",
                                client_id, n
                            );
                            slow_clients.push(client_id);
                        } else {
                            debug!(
                                "[ClientCoordinator] Client {} mailbox full — dropping frame ({} consecutive)",
                                client_id, n
                            );
                        }
                    }
                    Err(actix::prelude::SendError::Closed(_)) => {
                        // Real disconnect: client actor is gone. Evict.
                        warn!(
                            "[ClientCoordinator] Client {} mailbox closed — marking for eviction",
                            client_id
                        );
                        slow_clients.push(client_id);
                    }
                }
            } else {
                // Logged at debug so we can trace filter-caused zero-sends without
                // spamming the log in healthy operation.
                debug!(
                    "[ClientCoordinator] Client {} received no payload (filter produced empty set)",
                    client_id
                );
            }
        }
        BroadcastResult { sent, slow_clients }
    }

    /// Serialise positions into a binary protocol frame.
    ///
    /// Per ADR-061: single wire format, 28 bytes/node, no analytics, no
    /// flags. Visibility is enforced upstream — by the time this is called,
    /// `positions` contains only nodes the recipient may see.
    fn serialize_positions(
        &self,
        positions: &[BinaryNodeDataClient],
        broadcast_sequence: u64,
    ) -> Vec<u8> {
        use crate::utils::binary_protocol::encode_position_frame;
        use crate::utils::socket_flow_messages::BinaryNodeData;
        let nodes: Vec<(u32, BinaryNodeData)> =
            positions.iter().map(|pos| (pos.node_id, *pos)).collect();
        encode_position_frame(&nodes, broadcast_sequence)
    }

    pub fn broadcast_message(&self, message: String) -> usize {
        let mut broadcast_count = 0;
        for (_, client_state) in &self.clients {
            client_state.addr.do_send(SendToClientText(message.clone()));
            broadcast_count += 1;
        }
        broadcast_count
    }

    pub fn get_client_count(&self) -> usize {
        self.clients.len()
    }

    pub fn get_unsynced_clients(&self) -> Vec<usize> {
        self.clients
            .values()
            .filter(|client| !client.initial_sync_completed)
            .map(|client| client.client_id)
            .collect()
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

    // Neo4j settings repository for loading/saving user filters
    neo4j_settings_repository:
        Option<Arc<crate::adapters::neo4j_settings_repository::Neo4jSettingsRepository>>,

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

    /// Shared node analytics data (cluster_id, anomaly_score, community_id) per node
    node_analytics: Arc<std::sync::RwLock<std::collections::HashMap<u32, (u32, f32, u32)>>>,

    /// ADR-031 gap 3b: Per-client reconnect message queue.
    disconnected_queue: DisconnectedClientQueue,

    /// ADR-061 §D2: per-source rate cap on `analytics_update` emissions.
    /// Defence-in-depth — producers should already be bounded, but this
    /// throttles fast-arriving emits so a misconfigured producer cannot
    /// flood clients. Max 1 emit per second per source.
    last_analytics_emit: HashMap<crate::actors::messages::AnalyticsSource, Instant>,

    /// ADR-061 §D2: when an emit is rate-capped, stash the LATEST message
    /// per source so it can be flushed when the window opens. Last-wins by
    /// generation — only the freshest update is retained.
    pending_analytics_emit: HashMap<
        crate::actors::messages::AnalyticsSource,
        crate::actors::messages::BroadcastAnalyticsUpdate,
    >,
    /// Tracks whether a flush is already scheduled for a source so we don't
    /// pile up `run_later` callbacks under sustained pressure.
    analytics_flush_scheduled: std::collections::HashSet<crate::actors::messages::AnalyticsSource>,
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
    pub fn new() -> Self {
        Self {
            client_manager: Arc::new(RwLock::new(ClientManager::new())),
            last_broadcast: Instant::now(),
            // OMNIBUS-FIX-8: client is authoritative for layout (runs its own
            // physics in graph.worker). Server broadcasts are a coordination
            // signal, not a real-time feed. Down from 50ms (20 Hz) to 2000ms
            // (0.5 Hz). User directive: "we don't want deltas and we don't
            // want continuous, we want to send the nodes once the gpus settle,
            // or whatever rate the backpressure can sustain".
            broadcast_interval: Duration::from_millis(2000),
            active_broadcast_interval: Duration::from_millis(2000),
            stable_broadcast_interval: Duration::from_millis(5000),
            initial_positions_sent: false,
            graph_service_addr: None,
            gpu_compute_addr: None,
            broadcast_sequence: 0,
            neo4j_settings_repository: None,
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
            last_analytics_emit: HashMap::new(),
            pending_analytics_emit: HashMap::new(),
            analytics_flush_scheduled: std::collections::HashSet::new(),
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
    fn replay_buffered_messages(&mut self, old_client_id: usize, addr: &Addr<SocketFlowServer>) {
        let messages = self.disconnected_queue.drain_for_reconnect(old_client_id);
        if !messages.is_empty() {
            info!(
                "Replayed {} buffered messages to reconnected client {}",
                messages.len(),
                old_client_id
            );
            for msg in messages {
                addr.do_send(SendToClientBinary(msg));
            }
        }
    }

    /// Set the shared node analytics map (cluster_id, anomaly_score, community_id)
    pub fn set_node_analytics(
        &mut self,
        analytics: Arc<std::sync::RwLock<std::collections::HashMap<u32, (u32, f32, u32)>>>,
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

    /// Set the Neo4j settings repository for loading user filters
    pub fn set_neo4j_repository(
        &mut self,
        repo: Arc<crate::adapters::neo4j_settings_repository::Neo4jSettingsRepository>,
    ) {
        self.neo4j_settings_repository = Some(repo);
        info!("Neo4j settings repository configured for ClientCoordinatorActor");
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
            // ADR-031 item 5: evict slow clients detected during broadcast.
            if !broadcast_result.slow_clients.is_empty() {
                if let Ok(mut manager) = self.client_manager.write() {
                    for id in &broadcast_result.slow_clients {
                        warn!(
                            "[ClientCoordinator] Evicting slow client {} (voice broadcast)",
                            id
                        );
                        manager.unregister_client(*id);
                    }
                }
            }
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
            for (_, node_data) in &self.position_cache {
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
                // ADR-031 item 5: evict slow clients.
                if !broadcast_result.slow_clients.is_empty() {
                    if let Ok(mut manager) = self.client_manager.write() {
                        for id in &broadcast_result.slow_clients {
                            warn!(
                                "[ClientCoordinator] Evicting slow client {} (position cache)",
                                id
                            );
                            manager.unregister_client(*id);
                        }
                    }
                }
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
        for (_, node_data) in &self.position_cache {
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

        // Use per-client filtered broadcast for consistency with BroadcastPositions
        let result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return false;
                }
            };
            manager.broadcast_with_filter(&position_data, &self.node_type_arrays, current_sequence)
        };
        let broadcast_count = result.sent;
        // ADR-031 item 5: evict slow clients detected during force broadcast.
        if !result.slow_clients.is_empty() {
            if let Ok(mut manager) = self.client_manager.write() {
                for id in &result.slow_clients {
                    warn!(
                        "[ClientCoordinator] Evicting slow client {} (force broadcast)",
                        id
                    );
                    manager.unregister_client(*id);
                }
            }
        }

        // Approximate byte size: 9-byte frame header + 28 bytes per node.
        let approx_bytes = crate::utils::binary_protocol::FRAME_HEADER_SIZE
            + position_data.len() * crate::utils::binary_protocol::NODE_ENTRY_SIZE;

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

    /// Serialise positions for the broadcast-to-all path (voice relay,
    /// position cache flush). No per-client identity available here, so
    /// every private node is filtered out of the frame.
    fn serialize_positions(&self, positions: &[BinaryNodeDataClient]) -> Vec<u8> {
        use crate::utils::binary_protocol::encode_position_frame;
        use crate::utils::socket_flow_messages::BinaryNodeData;
        let nta = &self.node_type_arrays;
        let private_hidden_ids = compute_private_opaque_ids(nta, None);
        let nodes: Vec<(u32, BinaryNodeData)> = positions
            .iter()
            .filter(|pos| !private_hidden_ids.contains(&pos.node_id))
            .map(|pos| (pos.node_id, *pos))
            .collect();
        // Sequence 0 — this path is for broadcast-to-all where the sequence
        // is not consumed by ack correlation (force_broadcast handles its
        // own sequence). For send_prioritized_broadcasts, the broadcast
        // cadence and ack mechanism are decoupled (ADR-031 §queue-only ack).
        encode_position_frame(&nodes, 0)
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
        for (_, node_data) in &self.position_cache {
            position_data.push(*node_data);
        }

        if position_data.is_empty() {
            return Err("No position data available for broadcast".to_string());
        }

        // Increment sequence BEFORE broadcast so it's embedded in the wire frame
        self.broadcast_sequence += 1;
        let current_sequence = self.broadcast_sequence;

        // Use per-client filtered broadcast for consistency with BroadcastPositions
        let result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error: {}", e);
                    return Err(format!("Failed to acquire client manager lock: {}", e));
                }
            };
            manager.broadcast_with_filter(&position_data, &self.node_type_arrays, current_sequence)
        };
        let broadcast_count = result.sent;
        // ADR-031 item 5: evict slow clients detected during position broadcast.
        if !result.slow_clients.is_empty() {
            if let Ok(mut manager) = self.client_manager.write() {
                for id in &result.slow_clients {
                    warn!(
                        "[ClientCoordinator] Evicting slow client {} (position broadcast)",
                        id
                    );
                    manager.unregister_client(*id);
                }
            }
        }

        // Approximate byte size: 9-byte frame header + 28 bytes per node.
        let approx_bytes = crate::utils::binary_protocol::FRAME_HEADER_SIZE
            + position_data.len() * crate::utils::binary_protocol::NODE_ENTRY_SIZE;

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

        // REGRESSION FIX 2026-04-29: refresh node_type_arrays from GraphStateActor.
        //
        // Symptom: every node in the binary frame had idFlagged with all flag
        // bits zero — the encoder couldn't distinguish KG from agent from
        // ontology, so the renderer fell back to a uniform appearance and
        // user setting changes had no visible effect ("we don't see live
        // updates"). Root cause: ClientCoordinator caches the arrays in
        // `self.node_type_arrays` but nobody ever sent `UpdateNodeTypeArrays`
        // (zero senders in the codebase). The cache stayed at default-empty
        // forever. The encoder at `binary_protocol.rs:442-451` walks each
        // array — empty arrays mean no flag is set on any node ID.
        //
        // Fix: pull arrays from GraphStateActor every 5 s. First refresh
        // fires ~immediately so the very first broadcast that reaches the
        // client carries proper flags. 5 s cadence keeps stale-after-sync
        // windows short without flooding GraphStateActor.
        ctx.run_interval(Duration::from_secs(5), |act, ctx| {
            let Some(graph_addr) = act.graph_service_addr.clone() else {
                return;
            };
            let self_addr = ctx.address();
            actix::spawn(async move {
                use crate::actors::messages::{GetNodeTypeArrays, UpdateNodeTypeArrays};
                match graph_addr.send(GetNodeTypeArrays).await {
                    Ok(arrays) => {
                        self_addr.do_send(UpdateNodeTypeArrays { arrays });
                    }
                    Err(e) => warn!("[ClientCoordinator] node_type_arrays mailbox error: {}", e),
                }
            });
        });
        // Fire one refresh ~immediately at boot (don't wait for the 5-s tick).
        ctx.run_later(Duration::from_secs(1), |act, ctx| {
            let Some(graph_addr) = act.graph_service_addr.clone() else {
                return;
            };
            let self_addr = ctx.address();
            actix::spawn(async move {
                use crate::actors::messages::{GetNodeTypeArrays, UpdateNodeTypeArrays};
                if let Ok(arrays) = graph_addr.send(GetNodeTypeArrays).await {
                    self_addr.do_send(UpdateNodeTypeArrays { arrays });
                }
            });
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
                    return Err(format!("Failed to acquire client manager lock: {}", e).into());
                }
            };
            manager.register_client(msg.addr)
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
        // ADR-031 item 5: evict slow clients.
        if !broadcast_result.slow_clients.is_empty() {
            if let Ok(mut manager) = self.client_manager.write() {
                for id in &broadcast_result.slow_clients {
                    warn!(
                        "[ClientCoordinator] Evicting slow client {} (node positions)",
                        id
                    );
                    manager.unregister_client(*id);
                }
            }
        }
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

/// Handler for BroadcastPositions — the per-physics-tick push path.
///
/// Per ADR-061: encoder is `binary_protocol::encode_position_frame`, no
/// analytics columns, no flag bits. Privacy is enforced inside
/// `broadcast_with_filter` by dropping invisible nodes from the frame.
/// Backpressure ack remains via the broadcast-sequence header.
impl Handler<BroadcastPositions> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: BroadcastPositions, _ctx: &mut Self::Context) -> Self::Result {
        // Increment sequence BEFORE broadcast so it's embedded in the wire frame
        self.broadcast_sequence += 1;
        let current_sequence = self.broadcast_sequence;

        let result = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error in BroadcastPositions: {}", e);
                    return;
                }
            };
            manager.broadcast_with_filter(&msg.positions, &self.node_type_arrays, current_sequence)
        };
        let client_count = result.sent;
        // ADR-031 item 5: evict slow clients detected during BroadcastPositions.
        if !result.slow_clients.is_empty() {
            if let Ok(mut manager) = self.client_manager.write() {
                for id in &result.slow_clients {
                    warn!(
                        "[ClientCoordinator] Evicting slow client {} (BroadcastPositions)",
                        id
                    );
                    manager.unregister_client(*id);
                }
            }
        }

        if client_count > 0 {
            self.broadcast_count += 1;
            // Approximate byte size: 9-byte frame header + 28 bytes per node.
            let approx_bytes = crate::utils::binary_protocol::FRAME_HEADER_SIZE
                + msg.positions.len() * crate::utils::binary_protocol::NODE_ENTRY_SIZE;
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

/// Handler for `BroadcastAnalyticsUpdate` (PRD-007 / ADR-061 §D2).
///
/// Sticky GPU outputs (cluster, community, anomaly, sssp) ride this side
/// channel at recompute cadence rather than the per-frame position stream.
/// Wire format is a JSON text message:
/// ```jsonc
/// {"type":"analytics_update","source":"clustering",
///  "generation":<u64>,"entries":[...]}
/// ```
///
/// Server-side rate cap: max 1 emit per source per second. Producers should
/// already be bounded by recompute cadence; this is defence-in-depth so a
/// runaway producer cannot flood clients.
/// ADR-061 §D2 rate-cap window per source.
const ANALYTICS_RATE_CAP: Duration = Duration::from_secs(1);

impl ClientCoordinatorActor {
    /// Emit a single `analytics_update` envelope to all subscribed clients.
    /// Updates `last_analytics_emit` so the rate-cap window restarts.
    fn emit_analytics_update(&mut self, msg: crate::actors::messages::BroadcastAnalyticsUpdate) {
        self.last_analytics_emit.insert(msg.source, Instant::now());
        let envelope = serde_json::json!({
            "type": "analytics_update",
            "source": msg.source,
            "generation": msg.generation,
            "entries": msg.entries,
        });
        let serialised = match serde_json::to_string(&envelope) {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "[ClientCoordinator] failed to serialise analytics_update: {}",
                    e
                );
                return;
            }
        };
        let count = {
            let manager = match handle_rwlock_error(self.client_manager.read()) {
                Ok(m) => m,
                Err(e) => {
                    error!("[ClientCoordinator] analytics_update lock error: {}", e);
                    return;
                }
            };
            manager.broadcast_message(serialised)
        };
        debug!(
            "[ClientCoordinator] analytics_update source={:?} gen={} entries={} fanned to {} clients",
            msg.source, msg.generation, msg.entries.len(), count
        );
    }
}

impl Handler<crate::actors::messages::BroadcastAnalyticsUpdate> for ClientCoordinatorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::BroadcastAnalyticsUpdate,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        let now = Instant::now();
        let elapsed = self
            .last_analytics_emit
            .get(&msg.source)
            .map(|prev| now.duration_since(*prev));

        // Within the rate window — coalesce: keep the latest message per source
        // (last-wins by generation; the merge resolves it client-side too) and
        // schedule a flush at the window boundary if one isn't already pending.
        if let Some(elapsed) = elapsed {
            if elapsed < ANALYTICS_RATE_CAP {
                let source = msg.source;
                let prior_gen = self
                    .pending_analytics_emit
                    .get(&source)
                    .map(|m| m.generation);
                let keep = match prior_gen {
                    Some(g) => msg.generation > g,
                    None => true,
                };
                if keep {
                    debug!(
                        "[ClientCoordinator] coalescing analytics_update source={:?} gen={} (rate cap, keeping for flush)",
                        source, msg.generation
                    );
                    self.pending_analytics_emit.insert(source, msg);
                } else {
                    debug!(
                        "[ClientCoordinator] dropping analytics_update source={:?} gen={} (older than pending)",
                        source, msg.generation
                    );
                }

                if !self.analytics_flush_scheduled.contains(&source) {
                    self.analytics_flush_scheduled.insert(source);
                    let remaining = ANALYTICS_RATE_CAP.saturating_sub(elapsed);
                    ctx.run_later(remaining, move |actor, _ctx| {
                        actor.analytics_flush_scheduled.remove(&source);
                        if let Some(pending) = actor.pending_analytics_emit.remove(&source) {
                            actor.emit_analytics_update(pending);
                        }
                    });
                }
                return;
            }
        }

        // Window open — emit immediately.
        self.emit_analytics_update(msg);
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
            if msg.sequence_id % 100 == 0 {
                debug!(
                    "ClientBroadcastAck: seq={}, nodes={}, client_timestamp={}ms, client_id={:?}",
                    msg.sequence_id, msg.nodes_received, msg.timestamp, msg.client_id
                );
            }
        } else {
            // GPU address not set, log warning once per 1000 ACKs
            if msg.sequence_id % 1000 == 0 {
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

            // Load saved filter and settings from Neo4j if repository is available
            if let Some(neo4j_repo) = self.neo4j_settings_repository.clone() {
                let pubkey_clone = msg.pubkey.clone();
                let client_id = msg.client_id;
                let manager_arc = self.client_manager.clone();
                let graph_addr = self.graph_service_addr.clone();
                let neo4j_repo_for_filter = neo4j_repo.clone();

                ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                    match neo4j_repo_for_filter.get_user_filter(&pubkey_clone).await {
                        Ok(Some(user_filter)) => {
                            info!("✅ Loaded filter from Neo4j for pubkey {}: enabled={}, quality_threshold={}",
                                  pubkey_clone, user_filter.enabled, user_filter.quality_threshold);

                            // Update client filter with loaded settings
                            if let Ok(mut manager) = manager_arc.write() {
                                if let Some(client) = manager.get_client_mut(client_id) {
                                    client.filter.enabled = user_filter.enabled;
                                    client.filter.quality_threshold = user_filter.quality_threshold;
                                    client.filter.authority_threshold = user_filter.authority_threshold;
                                    client.filter.filter_by_quality = user_filter.filter_by_quality;
                                    client.filter.filter_by_authority = user_filter.filter_by_authority;
                                    client.filter.filter_mode = match user_filter.filter_mode.as_str() {
                                        "and" => FilterMode::And,
                                        _ => FilterMode::Or,
                                    };
                                    client.filter.max_nodes = user_filter.max_nodes.map(|n| n as usize);

                                    // Recompute filtered nodes with loaded settings
                                    if client.filter.enabled {
                                        if let Some(graph_addr) = graph_addr {
                                            use crate::actors::messages::GetGraphData;
                                            match graph_addr.send(GetGraphData).await {
                                                Ok(Ok(graph_data)) => {
                                                    crate::actors::client_filter::recompute_filtered_nodes(
                                                        &mut client.filter,
                                                        &graph_data
                                                    );
                                                    info!("Recomputed filter for authenticated client {}: {} nodes visible",
                                                          client_id, client.filter.filtered_node_ids.len());
                                                }
                                                _ => warn!("Failed to fetch graph data for filter recomputation"),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            info!("No saved filter found for pubkey {}, using defaults", pubkey_clone);
                        }
                        Err(e) => {
                            error!("Failed to load filter from Neo4j: {}", e);
                        }
                    }
                }).map(|_, _, _| ()));

                // Also load saved user settings from Neo4j for per-user physics isolation
                let neo4j_repo2 = neo4j_repo;
                let pubkey_clone2 = msg.pubkey.clone();
                let client_id2 = msg.client_id;
                let manager_arc2 = self.client_manager.clone();

                ctx.spawn(actix::fut::wrap_future::<_, Self>(async move {
                    match neo4j_repo2.get_user_settings(&pubkey_clone2).await {
                        Ok(Some(user_settings)) => {
                            info!("Loaded user settings from Neo4j for pubkey {}", pubkey_clone2);
                            if let Ok(mut manager) = manager_arc2.write() {
                                if let Some(client) = manager.get_client_mut(client_id2) {
                                    client.settings_override = Some(user_settings);
                                    info!(
                                        "Applied per-user settings_override for client {} (pubkey {})",
                                        client_id2, pubkey_clone2
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            info!("No saved user settings for pubkey {}, using global defaults", pubkey_clone2);
                        }
                        Err(e) => {
                            error!("Failed to load user settings from Neo4j: {}", e);
                        }
                    }
                }).map(|_, _, _| ()));
            } else {
                // Fallback: No Neo4j repo configured, use default filter behavior
                warn!(
                    "Neo4j repository not configured, using default filter for client {}",
                    msg.client_id
                );
            }

            // Original behavior: recompute if filter is enabled (kept for non-Neo4j fallback)
            if client.filter.enabled && self.neo4j_settings_repository.is_none() {
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
            client.filter.max_nodes = msg.max_nodes.map(|n| n as usize);

            info!(
                "Updated filter for client {}: enabled={}, quality_threshold={}, max_nodes={:?}",
                msg.client_id, msg.enabled, msg.quality_threshold, msg.max_nodes
            );

            // Recompute which nodes pass the filter so the binary position
            // stream only sends updates for visible nodes. Graph data itself
            // is served via REST /api/graph/data — never over WebSocket.
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

                                    let filtered_count = client.filter.filtered_node_ids.len();
                                    info!("Recomputed filter for client {}: {} nodes visible (binary stream only)",
                                          client_id, filtered_count);
                                }
                            }
                        }
                        Err(e) => warn!("Failed to fetch graph data for filter recomputation: {}", e),
                        Ok(Err(e)) => warn!("Graph data fetch error: {}", e),
                    }
                }).map(|_, _, _| ()));
            } else {
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

    #[test]
    fn test_client_manager_registration() {
        let mut manager = ClientManager::new();
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
        // V3 protocol: 1 header byte + 48 bytes per node
        assert_eq!(serialized.len(), 1 + 48);
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
