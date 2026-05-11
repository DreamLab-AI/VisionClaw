use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use actix::prelude::*;
use actix_web_actors::ws;
use log::{debug, error, info, trace, warn};

use crate::app_state::AppState;
use crate::types::vec3::Vec3Data;
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::utils::validation::rate_limit::{EndpointRateLimits, RateLimiter};
use crate::utils::websocket_heartbeat::HeartbeatDirective;

// Constants for throttling debug logs
pub(crate) const DEBUG_LOG_SAMPLE_RATE: usize = 10;

// Default values for deadbands if not provided in settings
pub(crate) const DEFAULT_POSITION_DEADBAND: f32 = 0.01;
pub(crate) const DEFAULT_VELOCITY_DEADBAND: f32 = 0.005;

#[allow(dead_code)]
pub(crate) const BATCH_UPDATE_WINDOW_MS: u64 = 200;

// Create a global rate limiter for WebSocket position updates
lazy_static::lazy_static! {
    pub(crate) static ref WEBSOCKET_RATE_LIMITER: Arc<RateLimiter> = {
        Arc::new(RateLimiter::new(EndpointRateLimits::socket_flow_updates()))
    };
}

#[derive(Clone, Debug)]
pub struct PreReadSocketSettings {
    pub min_update_rate: u32,
    pub max_update_rate: u32,
    pub motion_threshold: f32,
    pub motion_damping: f32,
    pub heartbeat_interval_ms: u64,
    pub heartbeat_timeout_ms: u64,
}

#[allow(dead_code)]
pub struct SocketFlowServer {
    pub(crate) app_state: Arc<AppState>,
    pub(crate) client_id: Option<usize>,
    pub(crate) client_manager_addr:
        actix::Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>,
    pub(crate) last_ping: Option<u64>,
    pub(crate) update_counter: usize,
    pub(crate) last_activity: std::time::Instant,
    pub(crate) heartbeat_timer_set: bool,

    pub(crate) _node_position_cache: HashMap<String, BinaryNodeData>,
    pub(crate) last_sent_positions: HashMap<String, Vec3Data>,
    pub(crate) last_sent_velocities: HashMap<String, Vec3Data>,
    pub(crate) position_deadband: f32,
    pub(crate) velocity_deadband: f32,

    pub(crate) last_transfer_size: usize,
    pub(crate) last_transfer_time: Instant,
    pub(crate) total_bytes_sent: usize,
    pub(crate) update_count: usize,
    pub(crate) nodes_sent_count: usize,

    pub(crate) last_batch_time: Instant,
    pub(crate) current_update_rate: u32,

    pub(crate) min_update_rate: u32,
    pub(crate) max_update_rate: u32,
    pub(crate) motion_threshold: f32,
    pub(crate) motion_damping: f32,

    pub(crate) nodes_in_motion: usize,
    pub(crate) total_node_count: usize,
    pub(crate) last_motion_check: Instant,

    pub(crate) client_ip: String,
    pub(crate) is_reconnection: bool,
    pub(crate) state_synced: bool,

    // Authentication state
    pub(crate) pubkey: Option<String>,
    pub(crate) is_power_user: bool,
    // HTTP-equivalent URL of the WebSocket connection (for NIP-98 validation)
    pub(crate) connection_url: String,

    // Server-side drag handling state
    /// Set of node IDs currently being dragged by this client.
    /// Nodes are pinned on the server when added and unpinned when removed.
    pub(crate) dragged_nodes: HashSet<u32>,
    /// Last time a drag position update was received per node (for timeout-based unpin).
    pub(crate) drag_last_update: HashMap<u32, Instant>,
    /// Maximum time (ms) with no position update before auto-unpin. Default 500ms.
    pub(crate) drag_timeout_ms: u64,

    // FIX 6: Per-client node type filter for binary position stream.
    // When non-empty, only nodes whose type matches one of these strings
    // (e.g. "knowledge", "agent", "ontology") are included in position broadcasts.
    // Set via subscribe_position_updates { data: { nodeTypes: ["knowledge", "agent"] } }.
    pub(crate) subscribed_node_types: HashSet<String>,

    // Delta encoding state (per-connection)
    /// Frame counter for delta encoding (0..59, wraps at DELTA_RESYNC_INTERVAL)
    pub(crate) delta_frame_counter: u64,
    /// Previous frame's node positions, used to compute deltas
    pub(crate) delta_previous_nodes: HashMap<u32, BinaryNodeData>,
    /// Position epsilon squared for delta detection (configurable, default 0.001^2)
    pub(crate) delta_epsilon_sq: f32,

    /// ADR-031 item 4: Pending server-to-client directives embedded in pong frames.
    /// Drained on each `send_pong` call via the `WebSocketHeartbeat` trait override.
    pub(crate) pending_directives: Vec<HeartbeatDirective>,
}

impl SocketFlowServer {
    pub fn new(
        app_state: Arc<AppState>,
        pre_read_settings: PreReadSocketSettings,
        client_manager_addr: actix::Addr<
            crate::actors::client_coordinator_actor::ClientCoordinatorActor,
        >,
        client_ip: String,
    ) -> Self {
        let min_update_rate = pre_read_settings.min_update_rate;
        let max_update_rate = pre_read_settings.max_update_rate;
        let motion_threshold = pre_read_settings.motion_threshold;
        let motion_damping = pre_read_settings.motion_damping;

        let position_deadband = DEFAULT_POSITION_DEADBAND;
        let velocity_deadband = DEFAULT_VELOCITY_DEADBAND;

        let current_update_rate = max_update_rate;

        Self {
            app_state,
            client_id: None,
            client_manager_addr,
            last_ping: None,
            update_counter: 0,
            last_activity: std::time::Instant::now(),
            heartbeat_timer_set: false,
            _node_position_cache: HashMap::new(),
            last_sent_positions: HashMap::new(),
            last_sent_velocities: HashMap::new(),
            position_deadband,
            velocity_deadband,
            last_transfer_size: 0,
            last_transfer_time: Instant::now(),
            total_bytes_sent: 0,
            last_batch_time: Instant::now(),
            update_count: 0,
            nodes_sent_count: 0,
            current_update_rate,
            min_update_rate,
            max_update_rate,
            motion_threshold,
            motion_damping,
            nodes_in_motion: 0,
            total_node_count: 0,
            last_motion_check: Instant::now(),
            client_ip,
            is_reconnection: false,
            state_synced: false,
            pubkey: None,
            is_power_user: false,
            connection_url: String::new(),
            dragged_nodes: HashSet::new(),
            drag_last_update: HashMap::new(),
            drag_timeout_ms: 500,
            subscribed_node_types: HashSet::new(),
            delta_frame_counter: 0,
            delta_previous_nodes: HashMap::new(),
            delta_epsilon_sq: 0.001 * 0.001, // epsilon = 0.001, stored as squared
            pending_directives: Vec::new(),
        }
    }

    /// ADR-031 item 4: Queue a directive to be sent to this client in the next pong.
    pub fn queue_directive(&mut self, directive: HeartbeatDirective) {
        self.pending_directives.push(directive);
    }

    pub(crate) fn handle_ping(
        &mut self,
        msg: crate::utils::socket_flow_messages::PingMessage,
    ) -> crate::utils::socket_flow_messages::PongMessage {
        self.last_ping = Some(msg.timestamp);
        crate::utils::socket_flow_messages::PongMessage {
            type_: "pong".to_string(),
            timestamp: msg.timestamp,
        }
    }

    pub(crate) fn should_log_update(&mut self) -> bool {
        self.update_counter = (self.update_counter + 1) % DEBUG_LOG_SAMPLE_RATE;
        self.update_counter == 0
    }

    pub(crate) fn has_node_changed_significantly(
        &mut self,
        node_id: &str,
        new_position: Vec3Data,
        new_velocity: Vec3Data,
    ) -> bool {
        let position_changed = if let Some(last_position) = self.last_sent_positions.get(node_id) {
            let dx = new_position.x - last_position.x;
            let dy = new_position.y - last_position.y;
            let dz = new_position.z - last_position.z;
            let distance_squared = dx * dx + dy * dy + dz * dz;
            distance_squared > self.position_deadband * self.position_deadband
        } else {
            true
        };

        let velocity_changed = if let Some(last_velocity) = self.last_sent_velocities.get(node_id) {
            let dvx = new_velocity.x - last_velocity.x;
            let dvy = new_velocity.y - last_velocity.y;
            let dvz = new_velocity.z - last_velocity.z;
            let velocity_change_squared = dvx * dvx + dvy * dvy + dvz * dvz;
            velocity_change_squared > self.velocity_deadband * self.velocity_deadband
        } else {
            true
        };

        if position_changed || velocity_changed {
            self.last_sent_positions
                .insert(node_id.to_string(), new_position);
            self.last_sent_velocities
                .insert(node_id.to_string(), new_velocity);
            return true;
        }

        false
    }

    #[allow(dead_code)]
    pub(crate) fn get_current_update_interval(&self) -> std::time::Duration {
        let millis = (1000.0 / self.current_update_rate as f64) as u64;
        std::time::Duration::from_millis(millis)
    }

    #[allow(dead_code)]
    pub(crate) fn calculate_motion_percentage(&self) -> f32 {
        if self.total_node_count == 0 {
            return 0.0;
        }
        (self.nodes_in_motion as f32) / (self.total_node_count as f32)
    }

    #[allow(dead_code)]
    pub(crate) fn update_dynamic_rate(&mut self) {
        let now = Instant::now();
        let batch_window = std::time::Duration::from_millis(BATCH_UPDATE_WINDOW_MS);
        let elapsed = now.duration_since(self.last_batch_time);

        if elapsed >= batch_window {
            let motion_pct = self.calculate_motion_percentage();

            if motion_pct > self.motion_threshold {
                self.current_update_rate = ((self.current_update_rate as f32) * self.motion_damping
                    + (self.max_update_rate as f32) * (1.0 - self.motion_damping))
                    as u32;
            } else {
                self.current_update_rate = ((self.current_update_rate as f32) * self.motion_damping
                    + (self.min_update_rate as f32) * (1.0 - self.motion_damping))
                    as u32;
            }

            self.current_update_rate = self
                .current_update_rate
                .clamp(self.min_update_rate, self.max_update_rate);

            self.last_motion_check = now;
        }
    }

    /// Send full state sync to a newly connected client (graph data + initial load).
    pub(crate) fn send_full_state_sync(&self, ctx: &mut <Self as Actor>::Context) {
        let app_state = self.app_state.clone();
        let addr = ctx.address();

        actix::spawn(async move {
            if let Ok(Ok(graph_data)) = app_state
                .graph_service_addr
                .send(crate::actors::messages::GetGraphData)
                .await
            {
                if let Ok(Ok(settings)) = app_state
                    .settings_addr
                    .send(crate::actors::messages::GetSettings)
                    .await
                {
                    let state_sync = serde_json::json!({
                        "type": "state_sync",
                        "data": {
                            "graph": {
                                "nodes_count": graph_data.nodes.len(),
                                "edges_count": graph_data.edges.len(),
                                "metadata_count": graph_data.metadata.len(),
                            },
                            "settings": {
                                "version": settings.version,
                            },
                            "timestamp": std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                        }
                    });

                    if let Ok(msg_str) = serde_json::to_string(&state_sync) {
                        addr.do_send(crate::actors::messages::SendToClientText(msg_str));
                        info!(
                            "Sent state sync: {} nodes, {} edges, version: {}",
                            graph_data.nodes.len(),
                            graph_data.edges.len(),
                            settings.version
                        );
                    }

                    // Graph data is served via REST /api/graph/data — NOT over WebSocket.
                    // WebSocket carries only binary position/velocity frames and
                    // lightweight JSON control messages (state_sync, filter_update_success,
                    // analytics_update). Sending the full graph as JSON text over WS
                    // caused ~2.3MB main-thread JSON parsing that locked the browser.

                    // Send an initial binary position snapshot so the client has
                    // positions immediately after connecting (before the next GPU tick).
                    if !graph_data.nodes.is_empty() {
                        let node_data: Vec<(u32, BinaryNodeData)> = graph_data
                            .nodes
                            .iter()
                            .map(|node| {
                                (
                                    node.id,
                                    BinaryNodeData {
                                        node_id: node.id,
                                        x: node.data.x,
                                        y: node.data.y,
                                        z: node.data.z,
                                        vx: node.data.vx,
                                        vy: node.data.vy,
                                        vz: node.data.vz,
                                    },
                                )
                            })
                            .collect();

                        addr.do_send(super::actor_messages::BroadcastPositionUpdate(node_data.clone()));
                        info!("Sent initial binary position snapshot for {} nodes", node_data.len());
                    }
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// ADR-031 item 4: WebSocketHeartbeat trait implementation.
// Overrides `get_pending_directives` to drain and return queued directives.
// ---------------------------------------------------------------------------
impl crate::utils::websocket_heartbeat::WebSocketHeartbeat for SocketFlowServer {
    fn get_client_id(&self) -> &str {
        // client_id is usize; return a static fallback when not yet assigned.
        // The heartbeat trait needs a &str — we leak a tiny string for the
        // lifetime of the session. In practice this is fine because sessions
        // are short-lived and the string is tiny.
        // A better approach is to store a String client_id, but that would
        // change more code than necessary for this gap fix.
        static UNKNOWN: &str = "unknown";
        // We can't return &str from usize, so this trait method is unused
        // by the SocketFlowServer's own heartbeat (it uses its own timer).
        // Provide a best-effort impl for completeness.
        if self.client_id.is_some() {
            UNKNOWN // The numeric ID is used elsewhere; this trait path is informational only.
        } else {
            UNKNOWN
        }
    }

    fn get_last_heartbeat(&self) -> Instant {
        self.last_activity
    }

    fn update_last_heartbeat(&mut self) {
        self.last_activity = Instant::now();
    }

    fn get_pending_directives(&mut self) -> Vec<HeartbeatDirective> {
        std::mem::take(&mut self.pending_directives)
    }
}

impl Actor for SocketFlowServer {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Default actix mailbox capacity is 16. With ~1.2 MB binary frames at
        // 60 Hz, that buffers <300 ms — far less than the browser's initial-burst
        // window (React StrictMode double-mount + GPU shader compile + 25 k-node
        // mesh build can stall WS draining for several seconds). When the mailbox
        // overflows, ClientCoordinatorActor was previously evicting the client,
        // silently stranding it on JSON-only updates so settings changes never
        // reflected on the visual graph. Bump to 2048 to survive the burst.
        ctx.set_mailbox_capacity(2048);

        let client_ip = self.client_ip.clone();
        let cm_addr = self.client_manager_addr.clone();
        let addr = ctx.address();
        let is_reconnection = self.is_reconnection;
        let addr_clone = addr.clone();

        actix::spawn(async move {
            use crate::actors::messages::RegisterClient;
            match cm_addr.send(RegisterClient { addr: addr_clone }).await {
                Ok(Ok(id)) => {
                    addr.do_send(super::actor_messages::SetClientId(id));
                }
                Ok(Err(e)) => {
                    error!("ClientManagerActor failed to register client: {}", e);
                }
                Err(e) => {
                    error!(
                        "Failed to send RegisterClient message to ClientManagerActor: {}",
                        e
                    );
                }
            }
        });

        info!(
            "[WebSocket] {} client connected from {}",
            if is_reconnection {
                "Reconnecting"
            } else {
                "New"
            },
            client_ip
        );
        self.last_activity = std::time::Instant::now();

        if !self.heartbeat_timer_set {
            ctx.run_interval(std::time::Duration::from_secs(5), |act, ctx| {
                trace!("[WebSocket] Sending server heartbeat ping");
                ctx.ping(b"");
                act.last_activity = std::time::Instant::now();
            });
            self.heartbeat_timer_set = true;
        }

        self.send_full_state_sync(ctx);
        self.state_synced = true;

        let response = serde_json::json!({
            "type": "connection_established",
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "is_reconnection": is_reconnection,
            "state_sync_sent": true,
            "protocol": {
                // ADR-037: V3 is the canonical node payload, V5 is the broadcast
                // envelope. V4 delta frames were retired on 2026-04-20.
                "supported": [3, 5],
                "preferred": 5,
                "delta_encoding": false
            }
        });

        if let Ok(msg_str) = serde_json::to_string(&response) {
            ctx.text(msg_str);
            self.last_activity = std::time::Instant::now();
        }

        let loading_msg = serde_json::json!({
            "type": "loading",
            "message": if is_reconnection { "Restoring state..." } else { "Calculating initial layout..." }
        });
        ctx.text(serde_json::to_string(&loading_msg).unwrap_or_default());
        self.last_activity = std::time::Instant::now();
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        // Clean up orphaned drags: unpin any nodes this client was still dragging
        // when the WebSocket connection dropped (missed dragEnd messages).
        if !self.dragged_nodes.is_empty() {
            let node_ids: Vec<u32> = self.dragged_nodes.drain().collect();
            let graph_addr = self.app_state.graph_service_addr.clone();
            let count = node_ids.len();
            actix::spawn(async move {
                use crate::actors::messages::{NodeInteractionMessage, NodeInteractionType};
                for node_id in &node_ids {
                    graph_addr.do_send(NodeInteractionMessage {
                        node_id: *node_id,
                        interaction_type: NodeInteractionType::Released,
                        position: None,
                    });
                }
                debug!("[Drag] Cleaned up {} orphaned drags on disconnect", count);
            });
            warn!(
                "[Drag] Client disconnected with {} nodes still dragged, sending release",
                count
            );
        }
        self.drag_last_update.clear();

        if let Some(client_id) = self.client_id {
            let cm_addr = self.client_manager_addr.clone();
            actix::spawn(async move {
                use crate::actors::messages::UnregisterClient;
                if let Err(e) = cm_addr.send(UnregisterClient { client_id }).await {
                    error!("Failed to unregister client from ClientManagerActor: {}", e);
                }
            });
            info!("[WebSocket] Client {} disconnected", client_id);
        }
    }
}
