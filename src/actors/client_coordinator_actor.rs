//! Client Coordinator Actor - WebSocket Communication Management
//!
//! Coordinates all client-related WebSocket communication:
//! - Client connection state and lifecycle
//! - **Per-client acknowledged position streaming** (ADR-2141): each client has its own stream
//!   state ([`ClientStream`]); a chunk of the nodes that changed since that client's last
//!   acknowledged positions is built and sent when the previous chunk is acknowledged (or times
//!   out). There is no global broadcast rate, bandwidth cap or token bucket: delivery follows
//!   each client's own round trip, and stops when every client holds the latest positions.
//! - Per-session entitlement (private nodes are never in a session's candidate set, and nothing
//!   is sent until the visibility policy has loaded)
//! - Telemetry, voice relay and reconnect buffering for text messages

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
use crate::actors::position_stream::{AnalyticsKey, ClientStream, StreamSettings};
use crate::utils::socket_flow_messages::BinaryNodeDataClient;

#[derive(Debug, Clone)]
pub struct ClientState {
    pub client_id: usize,
    /// ADR-090 A6-S4: erased recipients replace `Addr<SocketFlowServer>` so
    /// this actor (inner ring) no longer depends on the handler (outer ring).
    pub addr: ClientRecipients,
    pub connected_at: Instant,
    pub last_update: Instant,
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
    /// When the client's mailbox was first found full in the current run of
    /// congested broadcasts; `None` once a broadcast reaches it again. A
    /// client congested for longer than `addr.stall_timeout` is evicted.
    pub congested_since: Option<Instant>,
    /// What this client has acknowledged and the chunk in flight (ADR-2141).
    pub stream: ClientStream,
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
/// skipped one. A peer that has gone away stops answering pings and is closed
/// after `system.websocket.heartbeatTimeout`
/// (`socket_flow_handler/heartbeat.rs`); its `stopped()` sends
/// `UnregisterClient`. A peer that stays connected but never reads is
/// evicted once it has been congested for `stall_timeout` (its heartbeat
/// timeout): `ClientManager::evict_stalled` closes its transport, because the
/// session actor does not run while the dispatcher is blocked on its writes.
///
/// `closed_clients` are clients whose socket actor has stopped. Callers MUST
/// evict these under a write lock AFTER releasing any read lock held during
/// the broadcast (see `ClientCoordinatorActor::settle_broadcast_result`).
#[derive(Debug, Default)]
pub struct BroadcastResult {
    pub sent: usize,
    /// Bytes handed to clients' mailboxes.
    pub bytes: usize,
    pub congested_clients: Vec<usize>,
    pub closed_clients: Vec<usize>,
}

impl BroadcastResult {
    /// Try to hand `data` to one client and record the outcome.
    fn deliver_binary(&mut self, client_id: usize, client_state: &ClientState, data: Vec<u8>) {
        let len = data.len();
        match client_state.addr.binary.try_send(SendToClientBinary(data)) {
            Ok(()) => {
                self.sent += 1;
                self.bytes += len;
            }
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

/// Everything a pump needs, borrowed from the coordinator for the duration of one call.
pub struct PumpContext<'a> {
    pub cache: &'a HashMap<u32, BinaryNodeDataClient>,
    pub nta: &'a crate::actors::messages::NodeTypeArrays,
    pub settings: &'a StreamSettings,
    pub analytics:
        Option<&'a std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>>,
    pub now: Instant,
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
            pubkey: None,
            is_power_user: false,
            filter: ClientFilter::default(),
            settings_override: None,
            ephemeral_session: false,
            congested_since: None,
            stream: ClientStream::new(),
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

    /// Track congestion from one broadcast: start the clock for clients whose
    /// mailbox was full, stop it for every other registered client.
    pub fn record_congestion(&mut self, result: &BroadcastResult, now: Instant) {
        for (id, client) in self.clients.iter_mut() {
            if result.congested_clients.contains(id) {
                client.congested_since.get_or_insert(now);
            } else {
                client.congested_since = None;
            }
        }
    }

    /// Evict every client that has stayed congested for its stall timeout:
    /// unregister it, close its transport (the session actor cannot run while
    /// its peer is not reading) and queue a close for the session. Returns the
    /// evicted ids.
    pub fn evict_stalled(&mut self, now: Instant) -> Vec<usize> {
        let stalled: Vec<usize> = self
            .clients
            .iter()
            .filter(|(_, c)| {
                c.congested_since.is_some_and(|since| {
                    now.saturating_duration_since(since) >= c.addr.stall_timeout
                })
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &stalled {
            if let Some(client) = self.clients.remove(id) {
                warn!(
                    "[ClientCoordinator] Client {} congested for {:?} (stall timeout) — evicting and closing",
                    id, client.addr.stall_timeout
                );
                client.addr.close.do_send(CloseClientSession {
                    reason: "not reading (stall timeout)".to_string(),
                });
                if let Some(transport) = &client.addr.transport {
                    transport.close();
                }
            }
        }
        self.active_connections = self.clients.len();
        stalled
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

    /// Send the next chunk to every client that is due one (ADR-2141). Called when new positions
    /// arrive and from the safety timer.
    pub fn pump_all(&mut self, cx: &PumpContext<'_>, seq: &mut u64) -> BroadcastResult {
        let mut result = BroadcastResult::default();
        if cx.cache.is_empty() || self.clients.is_empty() {
            return result;
        }
        // Fail closed: with the visibility filter on, "no classification has arrived yet" must not
        // read as "no node is private". Until a real snapshot is in hand nothing is sent.
        let policy_on = visionclaw_domain::utils::visibility_filter::filter_enabled();
        if policy_on && !cx.nta.loaded {
            log::warn!("positions withheld: node classification (visibility policy) has not arrived yet");
            return result;
        }
        let mut drop_sets: HashMap<Option<String>, std::collections::HashSet<u32>> = HashMap::new();
        let ids: Vec<usize> = self.clients.keys().copied().collect();
        for id in ids {
            self.pump_one(id, cx, seq, policy_on, &mut drop_sets, &mut result);
        }
        result
    }

    /// Send the next chunk to one client if it is due one (after its acknowledgement).
    pub fn pump_client(&mut self, client_id: usize, cx: &PumpContext<'_>, seq: &mut u64) -> BroadcastResult {
        let mut result = BroadcastResult::default();
        if cx.cache.is_empty() {
            return result;
        }
        let policy_on = visionclaw_domain::utils::visibility_filter::filter_enabled();
        if policy_on && !cx.nta.loaded {
            return result;
        }
        let mut drop_sets = HashMap::new();
        self.pump_one(client_id, cx, seq, policy_on, &mut drop_sets, &mut result);
        result
    }

    fn pump_one(
        &mut self,
        client_id: usize,
        cx: &PumpContext<'_>,
        seq: &mut u64,
        policy_on: bool,
        drop_sets: &mut HashMap<Option<String>, std::collections::HashSet<u32>>,
        result: &mut BroadcastResult,
    ) {
        let Some(client) = self.clients.get_mut(&client_id) else {
            return;
        };
        if client.stream.is_waiting(cx.now, cx.settings.ack_timeout) {
            return;
        }
        // Entitlement (ADR-060): the private-node drop-set for this session's identity, computed
        // once per distinct identity per pump. A candidate set that never contains a private node
        // means a frame can never carry one.
        let entitlement_on = policy_on && !cx.nta.private_nodes.is_empty();
        let hidden = if entitlement_on {
            let key = client.pubkey.clone();
            Some(&*drop_sets.entry(key).or_insert_with(|| {
                visionclaw_domain::utils::visibility_filter::compute_private_opaque_ids(
                    &cx.nta.private_nodes,
                    client.pubkey.as_deref(),
                )
            }))
        } else {
            None
        };
        let filter_on = client.filter.enabled;
        let allowed = &client.filter.filtered_node_ids;
        let visible = |id: u32| (!filter_on || allowed.contains(&id)) && hidden.is_none_or(|h| !h.contains(&id));

        // The analytics a record carries are part of what a client holds: a re-clustered node is
        // news even if it did not move.
        let analytics_key = |id: u32| -> AnalyticsKey {
            cx.analytics.and_then(|m| m.get(&id)).map_or([0; 4], |a| {
                [a.cluster_id, a.community_id, a.anomaly.to_bits(), a.centrality.to_bits()]
            })
        };

        let ids = client.stream.plan(cx.cache, &visible, &analytics_key, cx.settings);
        if ids.is_empty() {
            return;
        }
        let chunk: Vec<BinaryNodeDataClient> = ids.iter().filter_map(|i| cx.cache.get(i).copied()).collect();
        *seq += 1;
        let frame = Self::serialize_positions(&chunk, cx.nta, *seq, cx.analytics);
        let before = result.sent;
        result.deliver_binary(client_id, client, frame);
        if result.sent > before {
            client.stream.record_sent(*seq, cx.now, &chunk, &analytics_key);
        } else {
            // Not delivered (mailbox full or closed): the sequence number is simply skipped.
            // Nothing is in flight, so the same nodes are planned again next time.
        }
    }

    /// The client acknowledged chunk `seq`. Returns the number of nodes acknowledged.
    pub fn acknowledge(&mut self, client_id: usize, seq: u64) -> Option<usize> {
        self.clients.get_mut(&client_id)?.stream.acknowledge(seq)
    }

    /// Serialize positions into a V5 binary frame.
    ///
    /// V5 wire format: `[1 byte: version=5][8 bytes: sequence LE][V3 node data without version byte]`.
    /// The sequence names the chunk: clients echo it in their acknowledgement, and the XR client's
    /// freshness gate requires it to increase on a connection.
    fn serialize_positions(
        positions: &[BinaryNodeDataClient],
        nta: &crate::actors::messages::NodeTypeArrays,
        broadcast_sequence: u64,
        analytics_data: Option<
            &std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
        >,
    ) -> Vec<u8> {
        use crate::utils::binary_protocol::{encode_node_data_extended_with_sssp, NodeClassIds};
        use crate::utils::socket_flow_messages::BinaryNodeData;
        let nodes: Vec<(u32, BinaryNodeData)> = positions.iter().map(|pos| (pos.node_id, *pos)).collect();
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
        let mut result = Vec::with_capacity(1 + 8 + encoded.len().saturating_sub(1));
        result.push(5u8);
        result.extend_from_slice(&broadcast_sequence.to_le_bytes());
        if encoded.len() > 1 {
            result.extend_from_slice(&encoded[1..]);
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
}

impl Default for ClientManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ClientCoordinatorActor {
    client_manager: Arc<RwLock<ClientManager>>,

    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,

    /// Chunk sequence counter: each chunk sent to any client takes the next number, so a client
    /// sees strictly increasing sequences and echoes the one it received in its acknowledgement.
    broadcast_sequence: u64,

    /// Stored stream settings (`physics.streamChunkNodes` / `streamMoveThreshold` /
    /// `streamAckTimeoutMs`), applied live by `ConfigureStream`.
    stream_settings: StreamSettings,

    // Settings repository for loading/saving user filters (Oxigraph migration: ADR-11)
    settings_repository: Option<Arc<crate::adapters::SqliteSettingsRepository>>,

    position_cache: HashMap<u32, BinaryNodeDataClient>,

    /// Chunks delivered to clients' mailboxes, and their bytes.
    broadcast_count: u64,
    bytes_sent: u64,

    connection_stats: ConnectionStats,

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
        if let Ok(mut manager) = self.client_manager.write() {
            let now = Instant::now();
            manager.record_congestion(result, now);
            manager.evict_stalled(now);
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
        use visionclaw_domain::types::physics_config::{
            DEFAULT_STREAM_ACK_TIMEOUT_MS, DEFAULT_STREAM_CHUNK_NODES, DEFAULT_STREAM_MOVE_THRESHOLD,
        };
        Self {
            client_manager: Arc::new(RwLock::new(ClientManager::new())),
            graph_service_addr: None,
            broadcast_sequence: 0,
            stream_settings: StreamSettings {
                chunk_nodes: DEFAULT_STREAM_CHUNK_NODES as usize,
                move_threshold: DEFAULT_STREAM_MOVE_THRESHOLD,
                ack_timeout: Duration::from_millis(u64::from(DEFAULT_STREAM_ACK_TIMEOUT_MS)),
            },
            settings_repository: None,
            position_cache: HashMap::new(),
            broadcast_count: 0,
            bytes_sent: 0,
            connection_stats: ConnectionStats::default(),
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
    pub fn set_settings_repository(
        &mut self,
        repo: Arc<crate::adapters::SqliteSettingsRepository>,
    ) {
        self.settings_repository = Some(repo);
        info!("SQLite settings repository configured for ClientCoordinatorActor");
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

    /// Relay queued voice data to every client.
    fn flush_voice(&mut self) -> Result<usize, String> {
        use crate::utils::binary_protocol::BinaryProtocol;

        let mut total_sent = 0;
        while !self.pending_voice_data.is_empty() {
            let voice_data_len = self.pending_voice_data[0].len();
            let encoded = BinaryProtocol::encode_voice_data(&self.pending_voice_data[0]);

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
            self.bytes_sent += encoded.len() as u64;
            total_sent += broadcast_result.sent;

            self.voice_data_queued_bytes -= voice_data_len;
            self.pending_voice_data.remove(0);

            debug!("Sent voice data: {} bytes to {} clients", encoded.len(), broadcast_result.sent);
        }
        Ok(total_sent)
    }

    pub fn set_graph_service_addr(&mut self, addr: Addr<crate::actors::GraphServiceSupervisor>) {
        self.graph_service_addr = Some(addr);
        debug!("Graph service address set in client coordinator");
    }

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

    /// Send each client its next chunk where one is due (ADR-2141): called when new positions
    /// arrive, after an acknowledgement (for that client) and from the safety timer.
    fn pump(&mut self, only: Option<usize>) {
        if self.position_cache.is_empty() {
            return;
        }
        let analytics_guard = self.node_analytics.read().ok();
        let analytics_ref = analytics_guard.as_deref();
        let cx = PumpContext {
            cache: &self.position_cache,
            nta: &self.node_type_arrays,
            settings: &self.stream_settings,
            analytics: analytics_ref,
            now: Instant::now(),
        };
        let mut seq = self.broadcast_sequence;
        let result = {
            let mut manager = match handle_rwlock_error(self.client_manager.write()) {
                Ok(manager) => manager,
                Err(e) => {
                    error!("RwLock error while pumping positions: {}", e);
                    return;
                }
            };
            match only {
                Some(id) => manager.pump_client(id, &cx, &mut seq),
                None => manager.pump_all(&cx, &mut seq),
            }
        };
        drop(analytics_guard);
        self.broadcast_sequence = seq;
        self.broadcast_count += result.sent as u64;
        self.bytes_sent += result.bytes as u64;
        self.settle_broadcast_result(&result, "position stream");
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
        let (active_clients, clients_in_flight) = match handle_rwlock_error(self.client_manager.read()) {
            Ok(m) => (m.get_client_count(), m.clients.values().filter(|c| c.stream.has_in_flight()).count()),
            Err(e) => {
                error!("RwLock error: {}", e);
                (0, 0)
            }
        };
        ClientCoordinatorStats {
            active_clients,
            chunks_sent: self.broadcast_count,
            bytes_sent: self.bytes_sent,
            position_cache_size: self.position_cache.len(),
            clients_in_flight,
            stream_chunk_nodes: self.stream_settings.chunk_nodes,
            stream_ack_timeout_ms: self.stream_settings.ack_timeout.as_millis() as u64,
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
    /// Position chunks delivered to client mailboxes.
    pub chunks_sent: u64,
    pub bytes_sent: u64,
    pub position_cache_size: usize,
    /// Clients with a chunk sent and not yet acknowledged.
    pub clients_in_flight: usize,
    pub stream_chunk_nodes: usize,
    pub stream_ack_timeout_ms: u64,
    pub connection_stats: ConnectionStats,
}

impl Actor for ClientCoordinatorActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("ClientCoordinatorActor started - WebSocket communication manager ready");

        // Stall sweep: a client congested past its stall timeout is evicted and
        // closed even when no broadcast runs to notice it.
        ctx.run_interval(Duration::from_secs(1), |act, _ctx| {
            if let Ok(mut manager) = act.client_manager.write() {
                manager.evict_stalled(Instant::now());
            }
        });

        // The stored stream settings (the saved physics row) apply from the first client on; the
        // physics PUT keeps them current afterwards.
        if let Some(repo) = self.settings_repository.clone() {
            let me = ctx.address();
            actix::spawn(async move {
                if let Some(p) = crate::actors::gpu::physics_restore::load_saved_physics(repo.as_ref()).await {
                    me.do_send(ConfigureStream {
                        chunk_nodes: p.stream_chunk_nodes,
                        move_threshold: p.stream_move_threshold,
                        ack_timeout_ms: p.stream_ack_timeout_ms,
                    });
                }
            });
        }

        // Position stream safety net (ADR-2141). New positions and acknowledgements drive the
        // stream; this tick covers what neither does: a chunk that timed out unacknowledged, and
        // a client whose visible set just changed (filter recomputed, authenticated).
        ctx.run_interval(Duration::from_millis(50), |act, _ctx| {
            act.pump(None);
        });

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
                    "stream_chunk_nodes",
                    serde_json::json!(self.stream_settings.chunk_nodes),
                )
                .with_metadata(
                    "stream_ack_timeout_ms",
                    serde_json::json!(self.stream_settings.ack_timeout.as_millis()),
                ),
            );
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        let stats = self.get_stats();
        info!(
            "ClientCoordinatorActor stopped - {} clients, {} chunks, {} bytes sent",
            stats.active_clients, stats.chunks_sent, stats.bytes_sent
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

        // A new client has acknowledged nothing, so every node it may see is a candidate: the
        // stream delivers its initial sync, in chunks, through the same path as everything else.
        self.pump(Some(client_id));

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
            debug!(
                "Broadcasted agent-action frame ({} bytes) to {} clients",
                frame_len, client_count
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

/// New display positions from the physics (one full set per step, every entitled node). They
/// replace the cache; each client then gets its own next chunk where one is due (ADR-2141).
impl Handler<PublishPositions> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: PublishPositions, _ctx: &mut Self::Context) -> Self::Result {
        self.update_position_cache(msg.positions.into_iter().map(|p| (p.node_id, p)).collect());
        self.pump(None);
    }
}

/// Stored stream settings changed (`physics.streamChunkNodes` / `streamMoveThreshold` /
/// `streamAckTimeoutMs`): applied to every client's next chunk.
impl Handler<ConfigureStream> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: ConfigureStream, _ctx: &mut Self::Context) -> Self::Result {
        self.stream_settings = StreamSettings {
            chunk_nodes: msg.chunk_nodes.max(1) as usize,
            move_threshold: msg.move_threshold,
            ack_timeout: Duration::from_millis(u64::from(msg.ack_timeout_ms)),
        };
        info!(
            "ClientCoordinator: stream settings chunk={} nodes, threshold={}, ack timeout={} ms",
            self.stream_settings.chunk_nodes, self.stream_settings.move_threshold, msg.ack_timeout_ms
        );
        self.pump(None);
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

/// A client acknowledged position chunk `sequence_id`: what that chunk carried is now what the
/// client holds, and its next chunk (if anything moved) is built and sent at once.
impl Handler<ClientBroadcastAck> for ClientCoordinatorActor {
    type Result = ();

    fn handle(&mut self, msg: ClientBroadcastAck, _ctx: &mut Self::Context) -> Self::Result {
        let Some(client_id) = msg.client_id else {
            debug!("ClientBroadcastAck without a client id (seq {}): ignored", msg.sequence_id);
            return;
        };
        let acknowledged = match handle_rwlock_error(self.client_manager.write()) {
            Ok(mut m) => m.acknowledge(client_id, msg.sequence_id),
            Err(e) => {
                error!("RwLock error: {}", e);
                return;
            }
        };
        match acknowledged {
            Some(n) => {
                if msg.sequence_id.is_multiple_of(200) {
                    debug!("ack seq={} client={} nodes={} (client says {})", msg.sequence_id, client_id, n, msg.nodes_received);
                }
                self.pump(Some(client_id));
            }
            None => trace_stale_ack(client_id, msg.sequence_id),
        }
    }
}

fn trace_stale_ack(client_id: usize, seq: u64) {
    log::trace!("ack seq={} from client {} matches no chunk in flight (late or duplicate)", seq, client_id);
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

        match self.flush_voice() {
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
    impl Handler<CloseClientSession> for Probe {
        type Result = ();
        fn handle(&mut self, _: CloseClientSession, _: &mut Self::Context) {}
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
        let r = ClientRecipients::new(
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
        );
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

    /// Captures every binary frame it is handed, so a test can decode what a session received.
    struct BinaryCapture(std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>);
    impl Actor for BinaryCapture {
        type Context = Context<Self>;
    }
    impl Handler<CloseClientSession> for BinaryCapture {
        type Result = ();
        fn handle(&mut self, _: CloseClientSession, _: &mut Self::Context) {}
    }
    impl Handler<SendToClientBinary> for BinaryCapture {
        type Result = ();
        fn handle(&mut self, m: SendToClientBinary, _: &mut Self::Context) {
            self.0.lock().unwrap().push(m.0);
        }
    }
    impl Handler<SendToClientText> for BinaryCapture {
        type Result = ();
        fn handle(&mut self, _: SendToClientText, _: &mut Self::Context) {}
    }
    impl Handler<crate::actors::messages::SendInitialGraphLoad> for BinaryCapture {
        type Result = ();
        fn handle(&mut self, _: crate::actors::messages::SendInitialGraphLoad, _: &mut Self::Context) {}
    }

    fn capture_client() -> (ClientRecipients, std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
        let frames = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let addr = BinaryCapture(frames.clone()).start();
        let r = ClientRecipients::new(
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
        );
        (r, frames)
    }

    /// Node ids carried by a V5 frame: `[0x05][seq:8]` then 52-byte records, id in the low 26 bits.
    fn frame_ids(frame: &[u8]) -> Vec<u32> {
        assert_eq!(frame[0], 5, "live broadcasts are V5");
        frame[9..].chunks_exact(52).map(|r| u32::from_le_bytes([r[0], r[1], r[2], r[3]]) & 0x03FF_FFFF).collect()
    }

    fn node(id: u32) -> BinaryNodeDataClient {
        BinaryNodeDataClient { node_id: id, x: id as f32, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0 }
    }

    /// A cache of `n` nodes (ids 1..=n) and a pump context over it.
    fn cache_of(n: u32) -> HashMap<u32, BinaryNodeDataClient> {
        (1..=n).map(|i| (i, node(i))).collect()
    }

    fn stream(chunk: usize, thr: f32, timeout_ms: u64) -> StreamSettings {
        StreamSettings { chunk_nodes: chunk, move_threshold: thr, ack_timeout: Duration::from_millis(timeout_ms) }
    }

    fn loaded() -> crate::actors::messages::NodeTypeArrays {
        crate::actors::messages::NodeTypeArrays { loaded: true, ..Default::default() }
    }

    fn pump_all_now(
        m: &mut ClientManager,
        cache: &HashMap<u32, BinaryNodeDataClient>,
        nta: &crate::actors::messages::NodeTypeArrays,
        set: &StreamSettings,
        seq: &mut u64,
    ) -> BroadcastResult {
        m.pump_all(&PumpContext { cache, nta, settings: set, analytics: None, now: Instant::now() }, seq)
    }

    fn ids_of(f: &std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>, frame: usize) -> Vec<u32> {
        frame_ids(&f.lock().unwrap()[frame])
    }

    /// The sequence number a V5 frame carries.
    fn frame_seq(frame: &[u8]) -> u64 {
        u64::from_le_bytes(frame[1..9].try_into().unwrap())
    }

    /// The connect-time sync withholds private nodes from sessions that may not see them, but
    /// the live stream used to send every node to every session (measured: an anonymous
    /// connection received all 9478 nodes in V5 frames after being sent 9470 on connect).
    /// Each session must receive exactly the nodes it is entitled to, in every frame.
    #[actix::test]
    async fn the_stream_withholds_private_nodes_from_sessions_that_may_not_see_them() {
        use visionclaw_domain::utils::visibility_filter::NodeVisibility;
        let mut m = ClientManager::new();
        let (anon_r, anon_frames) = capture_client();
        let (owner_r, owner_frames) = capture_client();
        let (other_r, other_frames) = capture_client();
        let (anon, owner, other) = (m.register_client(anon_r), m.register_client(owner_r), m.register_client(other_r));
        for id in [anon, owner, other] {
            m.get_client_mut(id).unwrap().filter.enabled = false;
        }
        m.get_client_mut(owner).unwrap().pubkey = Some("owner-key".into());
        m.get_client_mut(other).unwrap().pubkey = Some("someone-else".into());

        let nta = crate::actors::messages::NodeTypeArrays {
            loaded: true,
            private_nodes: vec![NodeVisibility::private_owned_by(5, "owner-key"), NodeVisibility::private_unowned(6)],
            ..Default::default()
        };
        let cache = cache_of(8);
        let mut seq = 0;
        pump_all_now(&mut m, &cache, &nta, &stream(100, 0.25, 500), &mut seq);
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;

        let ids = |f: &std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>| {
            let frames = f.lock().unwrap();
            assert_eq!(frames.len(), 1, "exactly one chunk per session");
            frame_ids(&frames[0])
        };
        assert_eq!(ids(&anon_frames), vec![1, 2, 3, 4, 7, 8], "anonymous: public only (fail closed)");
        assert_eq!(ids(&owner_frames), vec![1, 2, 3, 4, 5, 7, 8], "the owner also sees their own private node 5, never the unowned 6");
        assert_eq!(ids(&other_frames), vec![1, 2, 3, 4, 7, 8], "a different authenticated user sees public only");
    }

    /// Fail closed: before any classification snapshot has arrived, the stream must not read
    /// "nothing is private" and send everything to everyone.
    #[actix::test]
    async fn nothing_is_streamed_until_the_visibility_policy_has_arrived() {
        let mut m = ClientManager::new();
        let (r, frames) = capture_client();
        let id = m.register_client(r);
        m.get_client_mut(id).unwrap().filter.enabled = false;
        let cache = cache_of(4);
        let set = stream(100, 0.25, 500);
        let mut seq = 0;

        let before = pump_all_now(&mut m, &cache, &crate::actors::messages::NodeTypeArrays::default(), &set, &mut seq);
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(before.sent, 0);
        assert!(frames.lock().unwrap().is_empty(), "no chunk before the policy is known");

        pump_all_now(&mut m, &cache, &loaded(), &set, &mut seq);
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(frame_ids(&frames.lock().unwrap()[0]), vec![1, 2, 3, 4], "a loaded, empty policy means nothing is private");
    }

    /// Nothing private in the graph: every session gets every node (when the chunk is large enough).
    #[actix::test]
    async fn with_no_private_nodes_every_session_gets_every_node() {
        let mut m = ClientManager::new();
        let (r1, f1) = capture_client();
        let (r2, f2) = capture_client();
        let (a, b) = (m.register_client(r1), m.register_client(r2));
        for id in [a, b] {
            m.get_client_mut(id).unwrap().filter.enabled = false;
        }
        let cache = cache_of(5);
        let mut seq = 0;
        pump_all_now(&mut m, &cache, &loaded(), &stream(100, 0.25, 500), &mut seq);
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        for f in [&f1, &f2] {
            assert_eq!(frame_ids(&f.lock().unwrap()[0]), vec![1, 2, 3, 4, 5]);
        }
    }

    /// Counts binary frames it is handed (stands in for a SocketFlowServer
    /// whose socket write side is momentarily not being polled).
    struct BinaryCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Actor for BinaryCounter {
        type Context = Context<Self>;
    }
    impl Handler<CloseClientSession> for BinaryCounter {
        type Result = ();
        fn handle(&mut self, _: CloseClientSession, _: &mut Self::Context) {}
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
        let r = ClientRecipients::new(
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
        );
        (r, addr, n)
    }

    /// Regression: a client whose mailbox is momentarily full (the browser's
    /// main thread was busy, so its socket was not drained for a few seconds)
    /// used to be unregistered from the broadcast registry while its socket
    /// stayed open. It then received no position frame ever again. A full
    /// snapshot is latest-wins, so the right response to `Full` is to skip
    /// that one frame for that client and keep it registered.
    #[actix::test]
    async fn full_mailbox_skips_the_chunk_and_keeps_the_client() {
        let mut m = ClientManager::new();
        let (r, _addr, frames) = counter();
        let id = m.register_client(r);
        m.get_client_mut(id).unwrap().filter.enabled = false;
        let cache = cache_of(1);
        let nta = loaded();
        let set = stream(100, 0.25, 500);

        // The receiver is not polled until we yield, so this fills its mailbox.
        let mut congested_seen = false;
        let mut seq = 0;
        for _ in 0..64u64 {
            let to_all = m.broadcast_to_all(vec![0u8; 8]);
            // A chunk that could not be delivered is not "in flight": it is planned again.
            let streamed = pump_all_now(&mut m, &cache, &nta, &set, &mut seq);
            for res in [&to_all, &streamed] {
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

        // Once the receiver drains, the stream reaches it again. (One chunk was delivered before
        // the mailbox filled and was never acknowledged, so the client is still waiting on it: it
        // is rebuilt after the ack timeout, here reached by advancing the clock.)
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        let before = frames.load(std::sync::atomic::Ordering::SeqCst);
        let later = Instant::now() + Duration::from_secs(1);
        let res = m.pump_all(
            &PumpContext { cache: &cache, nta: &nta, settings: &set, analytics: None, now: later },
            &mut seq,
        );
        assert_eq!(res.sent, 1, "the recovered client receives its chunk");
        actix::clock::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(frames.load(std::sync::atomic::Ordering::SeqCst), before + 1);
    }

    /// Records close requests (stands in for a SocketFlowServer).
    struct CloseProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Actor for CloseProbe {
        type Context = Context<Self>;
    }
    impl Handler<SendToClientBinary> for CloseProbe {
        type Result = ();
        fn handle(&mut self, _: SendToClientBinary, _: &mut Self::Context) {}
    }
    impl Handler<SendToClientText> for CloseProbe {
        type Result = ();
        fn handle(&mut self, _: SendToClientText, _: &mut Self::Context) {}
    }
    impl Handler<crate::actors::messages::SendInitialGraphLoad> for CloseProbe {
        type Result = ();
        fn handle(
            &mut self,
            _: crate::actors::messages::SendInitialGraphLoad,
            _: &mut Self::Context,
        ) {
        }
    }
    impl Handler<CloseClientSession> for CloseProbe {
        type Result = ();
        fn handle(&mut self, _: CloseClientSession, ctx: &mut Self::Context) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ctx.stop();
        }
    }

    struct Stalled {
        recipients: ClientRecipients,
        transport_closes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        session_closes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    fn stalled_client(stall_timeout: Duration) -> Stalled {
        let session_closes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let transport_closes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let addr = CloseProbe(session_closes.clone()).start();
        let t = transport_closes.clone();
        let recipients = ClientRecipients::new(
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.recipient(),
        )
        .with_transport(TransportCloser::new(move || {
            t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }))
        .with_stall_timeout(stall_timeout);
        Stalled {
            recipients,
            transport_closes,
            session_closes,
        }
    }

    /// A client that stays connected but never reads used to keep its
    /// registration forever, skipping every frame. Once it has stayed
    /// congested for its stall timeout (the heartbeat timeout) it is evicted,
    /// its transport is closed from outside the session actor (whose mailbox
    /// it cannot drain) and the session is told to close.
    #[actix::test]
    async fn a_client_congested_past_its_stall_timeout_is_evicted_and_closed() {
        let mut m = ClientManager::new();
        let c = stalled_client(Duration::from_millis(100));
        let id = m.register_client(c.recipients);

        // Fill the mailbox without yielding, so the probe never drains it.
        let t0 = Instant::now();
        for _ in 0..40 {
            let r = m.broadcast_to_all(vec![0u8; 8]);
            m.record_congestion(&r, t0);
        }
        assert!(
            m.evict_stalled(t0 + Duration::from_millis(99)).is_empty(),
            "not yet"
        );
        assert_eq!(m.evict_stalled(t0 + Duration::from_millis(100)), vec![id]);
        assert!(m.get_client_mut(id).is_none(), "unregistered");
        assert_eq!(
            c.transport_closes.load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        // The session close was queued past the full mailbox (do_send) and is
        // handled once the session runs again.
        actix::clock::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            c.session_closes.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    /// A client that drains again before its stall timeout is not evicted.
    #[actix::test]
    async fn a_client_that_recovers_before_the_stall_timeout_is_kept() {
        let mut m = ClientManager::new();
        let c = stalled_client(Duration::from_millis(100));
        let id = m.register_client(c.recipients);
        let t0 = Instant::now();
        for _ in 0..40 {
            let r = m.broadcast_to_all(vec![0u8; 8]);
            m.record_congestion(&r, t0);
        }
        actix::clock::sleep(Duration::from_millis(20)).await; // the probe drains
        let r = m.broadcast_to_all(vec![0u8; 8]);
        assert_eq!(r.sent, 1);
        m.record_congestion(&r, t0 + Duration::from_millis(50));
        assert!(m.evict_stalled(t0 + Duration::from_millis(500)).is_empty());
        assert!(m.get_client_mut(id).is_some());
        assert_eq!(
            c.transport_closes.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
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
        impl Handler<CloseClientSession> for StopsAtOnce {
            type Result = ();
            fn handle(&mut self, _: CloseClientSession, _: &mut Self::Context) {}
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
        let r = ClientRecipients::new(
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
            addr.clone().recipient(),
        );
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

    /// The whole protocol on the manager: a client with a small chunk size is brought up to date in
    /// chunks, each sent only after the previous one is acknowledged, every node exactly once;
    /// then the stream is quiet until something moves.
    #[actix::test]
    async fn chunks_follow_acknowledgements_and_the_stream_goes_quiet_when_the_client_is_current() {
        let mut m = ClientManager::new();
        let (r, frames) = capture_client();
        let id = m.register_client(r);
        m.get_client_mut(id).unwrap().filter.enabled = false;
        let mut cache = cache_of(8);
        let (nta, set, mut seq) = (loaded(), stream(3, 0.25, 5000), 0u64);

        let mut delivered = Vec::new();
        for round in 0..3 {
            pump_all_now(&mut m, &cache, &nta, &set, &mut seq);
            // Nothing more is sent while the chunk is unacknowledged.
            let again = pump_all_now(&mut m, &cache, &nta, &set, &mut seq);
            assert_eq!(again.sent, 0, "round {round}: one chunk in flight at a time");
            actix::clock::sleep(Duration::from_millis(10)).await;
            let f = frames.lock().unwrap().last().unwrap().clone();
            assert!(frame_ids(&f).len() <= 3);
            delivered.extend(frame_ids(&f));
            assert!(m.acknowledge(id, frame_seq(&f)).is_some());
        }
        delivered.sort_unstable();
        assert_eq!(delivered, (1..=8).collect::<Vec<_>>(), "all eight, once each, in chunks of at most three");

        let quiet = pump_all_now(&mut m, &cache, &nta, &set, &mut seq);
        assert_eq!(quiet.sent, 0, "current client: nothing to send");

        cache.insert(5, BinaryNodeDataClient { x: 5.0 + 3.0, ..node(5) });
        assert_eq!(pump_all_now(&mut m, &cache, &nta, &set, &mut seq).sent, 1, "a real move is sent");
        actix::clock::sleep(Duration::from_millis(10)).await;
        let n = frames.lock().unwrap().len();
        assert_eq!(ids_of(&frames, n - 1), vec![5], "only the node that moved");
    }

    /// A client that never acknowledges (or a lost frame) is not abandoned: after the stored timeout
    /// its chunk is rebuilt and sent again with a new sequence number.
    #[actix::test]
    async fn an_unacknowledged_chunk_is_rebuilt_and_resent_after_the_timeout() {
        let mut m = ClientManager::new();
        let (r, frames) = capture_client();
        let id = m.register_client(r);
        m.get_client_mut(id).unwrap().filter.enabled = false;
        let cache = cache_of(4);
        let (nta, set, mut seq) = (loaded(), stream(100, 0.25, 500), 0u64);
        let t0 = Instant::now();

        let cx = |now| PumpContext { cache: &cache, nta: &nta, settings: &set, analytics: None, now };
        assert_eq!(m.pump_all(&cx(t0), &mut seq).sent, 1);
        assert_eq!(m.pump_all(&cx(t0 + Duration::from_millis(100)), &mut seq).sent, 0, "still waiting");
        assert_eq!(m.pump_all(&cx(t0 + Duration::from_millis(501)), &mut seq).sent, 1, "timed out: resent");
        actix::clock::sleep(Duration::from_millis(10)).await;
        let f = frames.lock().unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(frame_ids(&f[1]), frame_ids(&f[0]), "the same nodes");
        assert!(frame_seq(&f[1]) > frame_seq(&f[0]), "a fresh, larger sequence number");
        drop(f);
        assert!(m.acknowledge(id, 1).is_none(), "the late ack for the superseded chunk changes nothing");
        assert_eq!(m.acknowledge(id, 2), Some(4));
    }

    /// One client's acknowledgement never settles another client's chunk.
    #[actix::test]
    async fn acknowledgements_are_per_client() {
        let mut m = ClientManager::new();
        let ((r1, f1), (r2, f2)) = (capture_client(), capture_client());
        let (a, b) = (m.register_client(r1), m.register_client(r2));
        for id in [a, b] {
            m.get_client_mut(id).unwrap().filter.enabled = false;
        }
        let cache = cache_of(3);
        let (nta, set, mut seq) = (loaded(), stream(100, 0.25, 5000), 0u64);
        pump_all_now(&mut m, &cache, &nta, &set, &mut seq);
        actix::clock::sleep(Duration::from_millis(10)).await;
        let (sa, sb) = (frame_seq(&f1.lock().unwrap()[0]), frame_seq(&f2.lock().unwrap()[0]));
        assert_ne!(sa, sb, "each chunk has its own sequence");
        assert!(m.acknowledge(a, sb).is_none(), "client a cannot acknowledge client b's chunk");
        assert_eq!(m.acknowledge(a, sa), Some(3));
        assert_eq!(m.acknowledge(b, sb), Some(3));
    }
}
