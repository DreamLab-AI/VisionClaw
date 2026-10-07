use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use actix::prelude::*;
use actix_web_actors::ws;
use log::{debug, error, info, trace, warn};

use crate::app_state::AppState;
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::utils::validation::rate_limit::{EndpointRateLimits, RateLimiter};

// Constants for throttling debug logs
pub(crate) const DEBUG_LOG_SAMPLE_RATE: usize = 10;

// Default values for deadbands if not provided in settings

// Create a global rate limiter for WebSocket position updates
lazy_static::lazy_static! {
    pub(crate) static ref WEBSOCKET_RATE_LIMITER: Arc<RateLimiter> = {
        Arc::new(RateLimiter::new(EndpointRateLimits::socket_flow_updates()))
    };
}

pub struct SocketFlowServer {
    pub(crate) app_state: Arc<AppState>,
    pub(crate) client_id: Option<usize>,
    pub(crate) client_manager_addr:
        actix::Addr<crate::actors::client_coordinator_actor::ClientCoordinatorActor>,
    pub(crate) last_ping: Option<u64>,
    pub(crate) update_counter: usize,
    pub(crate) last_activity: std::time::Instant,
    pub(crate) heartbeat_timer_set: bool,
    /// Inbound-liveness tracker driven by `system.websocket.heartbeat*`
    /// (see `heartbeat.rs`). Replaced at upgrade with the configured timings.
    pub(crate) heartbeat: super::heartbeat::Heartbeat,
    /// Closes this connection's socket from outside the actor (set at upgrade
    /// from the connection data; `None` on non-TCP transports).
    pub(crate) transport: Option<crate::actors::messages::TransportCloser>,

    pub(crate) _node_position_cache: HashMap<String, BinaryNodeData>,

    pub(crate) last_transfer_size: usize,
    pub(crate) total_bytes_sent: usize,
    pub(crate) update_count: usize,
    pub(crate) nodes_sent_count: usize,

    pub(crate) nodes_in_motion: usize,
    pub(crate) total_node_count: usize,

    pub(crate) client_ip: String,
    pub(crate) is_reconnection: bool,
    pub(crate) state_synced: bool,

    // Authentication state
    pub(crate) pubkey: Option<String>,
    pub(crate) is_power_user: bool,
    /// Whether the dev-session-token bypass is permitted for THIS connection —
    /// computed once at the WS handshake from the peer address + env opt-in via
    /// `crate::utils::auth::dev_bypass_permitted_for_addr`, so the WS auth path
    /// routes through the same single gate as the REST paths. Always `false` in
    /// release builds. (ADR-142 hardening, Codex round-2.)
    #[cfg(any(debug_assertions, feature = "dev-auth"))]
    pub(crate) dev_bypass_ok: bool,
    // HTTP-equivalent URL of the WebSocket connection (for NIP-98 validation)
    pub(crate) connection_url: String,
    /// ADR-2134 same-user relay throttles (beatClock, memoryRoute), ≤ 4 Hz each.
    pub(crate) relay_throttles:
        HashMap<super::session_relay::RelayKind, super::session_relay::RelayThrottle>,
    /// One `error` frame per session for relay attempts before authentication.
    pub(crate) relay_unauth_reported: bool,

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

    /// Generation counter for the position-update re-subscription loop.
    /// A client that subscribes more than once (e.g. AppInitializer fires both
    /// on connection-status-change and on connection_established) would otherwise
    /// spawn one independent `run_later` self-loop per subscribe, doubling the
    /// binary broadcast rate. Each subscribe bumps this counter; every loop tick
    /// captures the generation it was started under and stops re-injecting once a
    /// newer subscribe supersedes it, leaving exactly one active loop.
    pub(crate) position_sub_generation: u64,

    /// Rate limit for `subscribe_position_updates`. A buggy client in a
    /// reconnect loop (observed 2026-06-12: a zombie tab resubscribing every
    /// ~400ms for hours) constantly restarts the broadcast loop and
    /// re-snapshots, degrading the position pipeline for EVERY connected
    /// client. Per-session state: one client's storm never throttles another
    /// client's subscription.
    pub(crate) last_position_subscribe: Option<Instant>,

    /// Last time this connection served a `requestInitialData` full-graph push.
    /// Guards against a client spamming `requestInitialData` to force unbounded
    /// full-graph fetch/sort/encode cycles (a cheap DoS). `None` until the first
    /// request; subsequent requests within `INITIAL_DATA_COOLDOWN` are dropped
    /// (a >30s gap still serves a legitimate reconnect-in-place).
    pub(crate) last_initial_data_request: Option<Instant>,

    /// Monotonic generation for in-flight full-state syncs (codex round-2 HIGH).
    /// `send_full_state_sync` runs asynchronously and its result is delivered via
    /// `do_send`, so two concurrent syncs can complete out of order — a stale
    /// (e.g. pre-auth, public-only) sync could otherwise land AFTER a fresh
    /// post-auth sync and overwrite the authenticated view, or a prior identity's
    /// private nodes could arrive after a re-auth as a different user. Every sync
    /// initiation and every auth/pubkey change bumps this counter; each spawned
    /// sync task captures the value at start and discards its result if the
    /// session generation has since moved on. `Arc<AtomicU64>` so the detached
    /// task can read the latest value the actor thread has written.
    pub(crate) sync_generation: Arc<std::sync::atomic::AtomicU64>,
}

impl SocketFlowServer {
    pub fn new(
        app_state: Arc<AppState>,
        client_manager_addr: actix::Addr<
            crate::actors::client_coordinator_actor::ClientCoordinatorActor,
        >,
        client_ip: String,
    ) -> Self {
        Self {
            app_state,
            client_id: None,
            client_manager_addr,
            last_ping: None,
            update_counter: 0,
            last_activity: std::time::Instant::now(),
            heartbeat_timer_set: false,
            heartbeat: super::heartbeat::Heartbeat::new(
                super::heartbeat::HeartbeatConfig::default(),
                Instant::now(),
            ),
            transport: None,
            _node_position_cache: HashMap::new(),
            last_transfer_size: 0,
            total_bytes_sent: 0,
            update_count: 0,
            nodes_sent_count: 0,
            nodes_in_motion: 0,
            total_node_count: 0,
            client_ip,
            is_reconnection: false,
            state_synced: false,
            pubkey: None,
            is_power_user: false,
            #[cfg(any(debug_assertions, feature = "dev-auth"))]
            dev_bypass_ok: false,
            connection_url: String::new(),
            relay_throttles: HashMap::new(),
            relay_unauth_reported: false,
            dragged_nodes: HashSet::new(),
            drag_last_update: HashMap::new(),
            drag_timeout_ms: 500,
            subscribed_node_types: HashSet::new(),
            position_sub_generation: 0,
            last_position_subscribe: None,
            last_initial_data_request: None,
            sync_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Bump the sync generation and return the new value (codex round-2 HIGH).
    /// Call whenever a full-state sync is initiated or the session's auth/pubkey
    /// state changes, so any older in-flight sync task detects it is stale and
    /// discards its result.
    pub(crate) fn bump_sync_generation(&self) -> u64 {
        self.sync_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    // REMOVED (ADR-2054): queue_directive — only caller was the removed
    // Handler<PushDirective>, which had zero senders tree-wide.

    pub(crate) fn handle_ping(
        &mut self,
        msg: crate::utils::socket_flow_messages::PingMessage,
    ) -> crate::utils::socket_flow_messages::PongMessage {
        self.last_ping = Some(msg.timestamp);
        crate::utils::socket_flow_messages::PongMessage {
            type_: "pong".to_string(),
            timestamp: msg.timestamp,
            // ADR-2134: the server clock at reply, so a client can estimate its
            // offset to the server from the round trip (XR beat clock sync).
            server_time: Some(chrono::Utc::now().timestamp_millis().max(0) as u64),
        }
    }

    pub(crate) fn should_log_update(&mut self) -> bool {
        self.update_counter = (self.update_counter + 1) % DEBUG_LOG_SAMPLE_RATE;
        self.update_counter == 0
    }

    /// Send full state sync to a newly connected client (graph data + initial load).
    pub(crate) fn send_full_state_sync(&self, ctx: &mut <Self as Actor>::Context) {
        let app_state = self.app_state.clone();
        let addr = ctx.address();
        // #4/ADR-060: honour the pubkey-visibility drop-set on the initial graph
        // load too. `caller_pubkey == None` (anon, e.g. the Godot client) with the
        // filter enabled fails closed to public-only nodes.
        let caller_pubkey = self.pubkey.clone();
        // Shared parse-once helper: default ON, explicit falsy tokens opt out.
        let visibility_filter_on = super::position_updates::pubkey_visibility_filter_enabled();

        // codex round-2 HIGH: initiating a sync bumps the generation, invalidating
        // any older in-flight sync. This task captures `my_generation`; if the
        // session generation has advanced by the time results are ready (a newer
        // sync — e.g. post-auth or a re-auth as a different identity — started),
        // it delivers nothing, so a stale/public-only view cannot overwrite a
        // fresher one and a prior identity's nodes cannot leak post-re-auth.
        let sync_generation = self.sync_generation.clone();
        let my_generation = self.bump_sync_generation();

        actix::spawn(async move {
            let is_current = || sync_generation_is_current(&sync_generation, my_generation);

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

                    // Discard if a newer sync superseded this one during the
                    // GetGraphData/GetSettings awaits above. Guards the state_sync
                    // and the InitialGraphLoad below (no await between them); the
                    // binary broadcast past the next await is re-checked.
                    if !is_current() {
                        debug!("Discarding stale full-state sync (generation superseded)");
                        return;
                    }

                    if let Ok(msg_str) = serde_json::to_string(&state_sync) {
                        // Generation-gated: re-validated at the socket write so a
                        // sync superseded after this enqueue is dropped, not sent.
                        addr.do_send(super::actor_messages::GenGatedText {
                            text: msg_str,
                            generation: my_generation,
                        });
                        info!(
                            "Sent state sync: {} nodes, {} edges, version: {}",
                            graph_data.nodes.len(),
                            graph_data.edges.len(),
                            settings.version
                        );
                    }

                    // Initial-load node cap, now settings-driven (ungated via the
                    // /api/settings interface). `initialNodeLimit` lives on the
                    // knowledge graph's physics settings; 0/absent = the built-in
                    // default (3000), and any value is clamped to a sanity ceiling
                    // (100_000) so a client can raise it up to the full graph.
                    let initial_node_limit = resolve_initial_node_limit(
                        settings
                            .visualisation
                            .graphs
                            .knowledge
                            .physics
                            .initial_node_limit,
                    );

                    // Send new InitialGraphLoad message with LIMITED node set for fast initial render
                    if !graph_data.nodes.is_empty() || !graph_data.edges.is_empty() {
                        use crate::services::inferred_edge_materialiser::edge_is_inferred;
                        use crate::utils::socket_flow_messages::{
                            InitialEdgeData, InitialNodeData,
                        };
                        use std::collections::HashSet;

                        // ADR-060 §Phase-4: drop private nodes not owned by this
                        // session before they reach the wire. Default OFF; when
                        // enabled an anonymous caller sees public-only (fail-closed).
                        let is_visible = |node: &visionclaw_domain::models::node::Node| -> bool {
                            if !visibility_filter_on {
                                return true;
                            }
                            let is_public = node
                                .metadata
                                .get("public")
                                .map(|v| v.eq_ignore_ascii_case("true"))
                                .unwrap_or(false);
                            if is_public {
                                return true;
                            }
                            match (caller_pubkey.as_deref(), node.metadata.get("owner_pubkey")) {
                                (Some(pk), Some(owner)) => pk == owner,
                                _ => false,
                            }
                        };

                        let mut sorted_nodes: Vec<&visionclaw_domain::models::node::Node> =
                            graph_data.nodes.iter().filter(|n| is_visible(n)).collect();

                        // Sort by quality_score descending
                        sorted_nodes.sort_by(|a, b| {
                            let quality_a = graph_data
                                .metadata
                                .get(&a.metadata_id)
                                .and_then(|m| m.quality_score)
                                .unwrap_or(0.0);
                            let quality_b = graph_data
                                .metadata
                                .get(&b.metadata_id)
                                .and_then(|m| m.quality_score)
                                .unwrap_or(0.0);
                            quality_b
                                .partial_cmp(&quality_a)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });

                        let filtered_nodes: Vec<&visionclaw_domain::models::node::Node> =
                            sorted_nodes.into_iter().take(initial_node_limit).collect();

                        let filtered_node_ids: HashSet<u32> =
                            filtered_nodes.iter().map(|n| n.id).collect();

                        let nodes: Vec<InitialNodeData> = filtered_nodes
                            .iter()
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

                        // Only include edges where BOTH source and target are in filtered nodes
                        let edges: Vec<InitialEdgeData> = graph_data
                            .edges
                            .iter()
                            .filter(|edge| {
                                filtered_node_ids.contains(&edge.source)
                                    && filtered_node_ids.contains(&edge.target)
                            })
                            .map(|edge| InitialEdgeData {
                                id: edge.id.clone(),
                                source_id: edge.source,
                                target_id: edge.target,
                                weight: Some(edge.weight),
                                edge_type: edge.edge_type.clone(),
                                inferred: edge_is_inferred(edge),
                            })
                            .collect();

                        addr.do_send(super::actor_messages::GenGatedInitialGraphLoad {
                            nodes: nodes.clone(),
                            edges: edges.clone(),
                            generation: my_generation,
                        });
                        info!("Sent InitialGraphLoad: {} nodes (sparse from {} total), {} edges [limit: {}]",
                              nodes.len(), graph_data.nodes.len(),
                              edges.len(), initial_node_limit);

                        // Fetch node type arrays for binary protocol flags
                        let nta = app_state
                            .graph_service_addr
                            .send(crate::actors::messages::GetNodeTypeArrays)
                            .await
                            .unwrap_or_default();
                        let agent_set: std::collections::HashSet<u32> =
                            nta.agent_ids.iter().copied().collect();
                        let knowledge_set: std::collections::HashSet<u32> =
                            nta.knowledge_ids.iter().copied().collect();

                        // Also send binary position data for SAME limited nodes only,
                        // with node type flags applied for client-side rendering
                        let node_data: Vec<(u32, BinaryNodeData)> = graph_data
                            .nodes
                            .iter()
                            .filter(|node| filtered_node_ids.contains(&node.id))
                            .map(|node| {
                                let flagged_id = if agent_set.contains(&node.id) {
                                    crate::utils::binary_protocol::set_agent_flag(node.id)
                                } else if knowledge_set.contains(&node.id) {
                                    crate::utils::binary_protocol::set_knowledge_flag(node.id)
                                } else {
                                    node.id
                                };
                                (
                                    flagged_id,
                                    BinaryNodeData {
                                        node_id: flagged_id,
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

                        // Re-check: a newer sync may have superseded this one
                        // during the GetNodeTypeArrays await above. Skip the
                        // binary frame so it cannot overwrite a fresher sync's
                        // positions.
                        if !is_current() {
                            debug!(
                                "Discarding stale full-state sync binary frame (generation superseded)"
                            );
                            return;
                        }

                        addr.do_send(super::actor_messages::BroadcastPositionUpdate(
                            node_data.clone(),
                            Some(my_generation),
                        ));
                        debug!(
                            "Sent initial node positions for {} limited nodes (binary)",
                            node_data.len()
                        );
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
        // client_id is a usize and the heartbeat trait wants a &str, so this
        // informational path always reports "unknown". SocketFlowServer's own
        // heartbeat runs on its own timer and does not use it.
        "unknown"
    }

    fn get_last_heartbeat(&self) -> Instant {
        self.last_activity
    }

    fn update_last_heartbeat(&mut self) {
        self.last_activity = Instant::now();
    }

    // REMOVED (ADR-2054): get_pending_directives override and the `pending_directives`
    // field it drained. `send_pong` (which calls this) is never invoked on the live
    // ping/pong path (mod.rs answers pings with `ctx.pong(&msg)` directly), so the
    // trait's default (`Vec::new()`) is behaviourally identical to the always-empty
    // queue this override used to drain.
}

impl Actor for SocketFlowServer {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let client_ip = self.client_ip.clone();
        let cm_addr = self.client_manager_addr.clone();
        let addr = ctx.address();
        let is_reconnection = self.is_reconnection;
        let addr_clone = addr.clone();
        // A client congested for a full heartbeat timeout is not reading.
        let stall_timeout = self.heartbeat.config().timeout;
        let transport = self.transport.clone();

        actix::spawn(async move {
            use crate::actors::messages::{ClientRecipients, RegisterClient};
            // ADR-090 A6-S4: build type-erased recipients from the concrete Addr
            // so the coordinator actor (inner ring) never sees SocketFlowServer.
            let mut recipients = ClientRecipients::new(
                addr_clone.clone().recipient(),
                addr_clone.clone().recipient(),
                addr_clone.clone().recipient(),
                addr_clone.clone().recipient(),
            )
            .with_stall_timeout(stall_timeout);
            if let Some(transport) = transport {
                recipients = recipients.with_transport(transport);
            }
            match cm_addr.send(RegisterClient { recipients }).await {
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
            // Ping every `heartbeatInterval`; close after `heartbeatTimeout` of
            // inbound silence. Sending a ping does not count as liveness.
            let config = self.heartbeat.config();
            self.heartbeat.inbound(Instant::now());
            ctx.run_interval(config.interval, move |act, ctx| {
                use super::heartbeat::HeartbeatAction;
                match act.heartbeat.tick(Instant::now()) {
                    HeartbeatAction::Ping => {
                        trace!("[WebSocket] Sending server heartbeat ping");
                        ctx.ping(b"");
                    }
                    HeartbeatAction::Close => {
                        warn!(
                            "[WebSocket] Client {:?} silent for {:?} — closing (heartbeat timeout)",
                            act.client_id, config.timeout
                        );
                        ctx.close(Some(ws::CloseReason {
                            code: ws::CloseCode::Away,
                            description: Some("heartbeat timeout".into()),
                        }));
                        ctx.stop();
                    }
                }
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
                "supported": [3, 5],
                "preferred": 3
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
            let app_state = self.app_state.clone();
            let count = node_ids.len();
            actix::spawn(async move {
                use crate::actors::messages::{
                    NodeInteractionMessage, NodeInteractionType, PinNodePositions,
                };
                for node_id in &node_ids {
                    graph_addr.do_send(NodeInteractionMessage {
                        node_id: *node_id,
                        interaction_type: NodeInteractionType::Released,
                        position: None,
                    });
                }
                // Release the GPU pins so orphaned nodes resume normal integration.
                if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
                    gpu_addr.do_send(PinNodePositions {
                        pins: Vec::new(),
                        unpin: node_ids.clone(),
                        reheat: false,
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

/// True iff the sync generation captured when a full-state sync task started
/// still matches the session's current generation (codex round-2 HIGH). A
/// detached sync task calls this before delivering its result and discards it
/// when a newer sync has since been initiated (or auth state changed), so
/// out-of-order completion cannot overwrite a fresher view.
pub(crate) fn sync_generation_is_current(
    current: &std::sync::atomic::AtomicU64,
    captured: u64,
) -> bool {
    current.load(std::sync::atomic::Ordering::SeqCst) == captured
}

/// Built-in fallback initial-load node cap when `initialNodeLimit` is unset (0).
pub(crate) const INITIAL_NODE_LIMIT_DEFAULT: usize = 3000;
/// Sanity ceiling for a client-configured initial-load node cap.
pub(crate) const INITIAL_NODE_LIMIT_CEILING: usize = 100_000;

/// Resolve the settings-driven `initialNodeLimit` to the concrete top-N cap used
/// by the initial graph load. `0` (or absent → serde default 0) means "use the
/// built-in default (3000)"; any other value is clamped to
/// [`INITIAL_NODE_LIMIT_CEILING`] so a client can raise the cap up to the full
/// graph without allowing an unbounded request.
pub(crate) fn resolve_initial_node_limit(configured: u32) -> usize {
    let configured = configured as usize;
    if configured == 0 {
        INITIAL_NODE_LIMIT_DEFAULT
    } else {
        configured.min(INITIAL_NODE_LIMIT_CEILING)
    }
}

#[cfg(test)]
mod initial_node_limit_tests {
    use super::{
        resolve_initial_node_limit, INITIAL_NODE_LIMIT_CEILING, INITIAL_NODE_LIMIT_DEFAULT,
    };

    #[test]
    fn zero_uses_builtin_default() {
        assert_eq!(resolve_initial_node_limit(0), INITIAL_NODE_LIMIT_DEFAULT);
    }

    #[test]
    fn explicit_value_is_honoured() {
        assert_eq!(resolve_initial_node_limit(500), 500);
        assert_eq!(resolve_initial_node_limit(20_000), 20_000);
    }

    #[test]
    fn value_above_ceiling_is_clamped() {
        assert_eq!(
            resolve_initial_node_limit(u32::MAX),
            INITIAL_NODE_LIMIT_CEILING
        );
    }
}

#[cfg(test)]
mod sync_generation_tests {
    use super::sync_generation_is_current;
    use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

    #[test]
    fn fresh_sync_is_current() {
        let gen = AtomicU64::new(0);
        // Initiate a sync: bump then capture (mirrors bump_sync_generation()).
        let my = gen.fetch_add(1, SeqCst) + 1;
        assert!(sync_generation_is_current(&gen, my));
    }

    #[test]
    fn superseded_sync_is_discarded() {
        let gen = AtomicU64::new(0);
        // Sync A starts.
        let a = gen.fetch_add(1, SeqCst) + 1;
        // Sync B (e.g. post-auth) starts while A is still in flight.
        let b = gen.fetch_add(1, SeqCst) + 1;
        // A must now detect it is stale; B is still current.
        assert!(
            !sync_generation_is_current(&gen, a),
            "stale sync A must be discarded"
        );
        assert!(
            sync_generation_is_current(&gen, b),
            "fresh sync B must deliver"
        );
    }

    #[test]
    fn reauth_bump_discards_prior_identity_sync() {
        let gen = AtomicU64::new(0);
        // Pre-auth sync starts.
        let anon = gen.fetch_add(1, SeqCst) + 1;
        // Auth state change bumps the generation (bump_sync_generation()).
        gen.fetch_add(1, SeqCst);
        // The prior-identity sync must not deliver after the identity changed.
        assert!(!sync_generation_is_current(&gen, anon));
    }

    #[test]
    fn delivery_time_check_catches_toctou_after_presend_passed() {
        // codex round-3: a sync stamps its frames with generation `g` and passes
        // the pre-send early-out (gen == g). If auth bumps the generation while
        // the frame sits queued in the mailbox, the delivery-time check (run in
        // the message handler, against the SAME predicate) must now drop it.
        let gen = AtomicU64::new(0);
        let g = gen.fetch_add(1, SeqCst) + 1;

        // Pre-send early-out: still current.
        assert!(sync_generation_is_current(&gen, g), "pre-send check passes");

        // ... message enqueued ... auth bumps the generation before the handler
        // runs (the TOCTOU window the pre-send check alone cannot cover).
        gen.fetch_add(1, SeqCst);

        // Delivery-time check in the handler: same generation stamp `g`, now stale.
        assert!(
            !sync_generation_is_current(&gen, g),
            "delivery-time check drops the stale frame"
        );
    }
}
