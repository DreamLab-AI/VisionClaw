use actix::prelude::*;
use actix_web_actors::ws;
use log::{debug, info, trace, warn};
use std::sync::OnceLock;
use std::time::Instant;

use crate::utils::binary_protocol;
use crate::utils::socket_flow_messages::{BinaryNodeData, BinaryNodeDataClient};
use crate::utils::validation::rate_limit::EndpointRateLimits;

use super::types::SocketFlowServer;

pub(crate) use visionclaw_domain::utils::visibility_filter::{
    apply_drop_set, compute_private_opaque_ids, NodeVisibility,
};

/// Env flag gating the ADR-060 pubkey-visibility drop-set filter.
///
/// Default ON: when unset (absent env) the filter is active, so private nodes
/// not owned by the session pubkey are dropped from the wire frame entirely
/// (ADR-059 §Phase-4) on top of the ADR-050 bit-29 opacification layer.
/// Fail-closed: anonymous + default ⇒ public-only. The posture is
/// operator-overridable via an explicit opt-out ("0"/"false"/"off"/"no").
/// Note: public nodes are unaffected by the filter, so default-on is
/// behaviour-neutral for all-public deployments.
pub(crate) const PUBKEY_VISIBILITY_FILTER_ENV: &str = "PUBKEY_VISIBILITY_FILTER";

/// Pure truth table for the visibility flag, independent of the environment.
///
/// Secure by default: returns `true` (filter ON) for an absent value and for any
/// unrecognised token; only an explicit falsy token ("0"/"false"/"off"/"no",
/// case-insensitive, surrounding whitespace ignored) turns the filter OFF. Kept
/// pure so it is exhaustively unit-testable without touching process env.
pub(crate) fn parse_visibility_flag(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        None => true,
    }
}

/// Cached result of the first env read (genuine parse-once).
static VISIBILITY_FILTER_ENABLED: OnceLock<bool> = OnceLock::new();

/// Whether the ADR-060 drop-set filter is enabled for this process.
///
/// Shared by every site that gates on the filter (here and [`super::types`]).
/// The `PUBKEY_VISIBILITY_FILTER` env var is read **once** on first call and the
/// result cached for the process lifetime via [`VISIBILITY_FILTER_ENABLED`];
/// runtime env mutations after that first read are intentionally ignored (the
/// posture is fixed on first use — the `OnceLock` captures the environment at
/// the first call, which may be later than process start). Truth table lives in
/// [`parse_visibility_flag`].
pub(crate) fn pubkey_visibility_filter_enabled() -> bool {
    *VISIBILITY_FILTER_ENABLED.get_or_init(|| {
        parse_visibility_flag(std::env::var(PUBKEY_VISIBILITY_FILTER_ENV).ok().as_deref())
    })
}

/// Project a graph node's ADR-050 visibility primitives onto its flagged wire id.
///
/// ADR-050 stores `visibility` / `owner_pubkey` on the node. Until that storage
/// lands, we derive them from the node metadata the parser already populates:
/// a node is public iff `metadata["public"] == "true"` (the vault frontmatter
/// `public: true` key), and `owner_pubkey` is read from `metadata["owner_pubkey"]` when
/// present. Absent an owner, a private node fails closed (dropped for everyone
/// but a future owner match).
fn node_visibility(wire_id: u32, node: &visionclaw_domain::models::Node) -> NodeVisibility {
    let is_public = node
        .metadata
        .get("public")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let owner_pubkey = node.metadata.get("owner_pubkey").cloned();
    NodeVisibility {
        wire_id,
        is_public,
        owner_pubkey,
    }
}

/// Maximum time budget (ms) for settle iterations per drag update.
const DRAG_SETTLE_BUDGET_MS: u64 = 50;

/// Maximum number of nodes a single client may drag simultaneously.
const MAX_DRAGGED_NODES_PER_CLIENT: usize = 5;

/// Minimum interval between drag position updates (~60 Hz cap).
const MIN_DRAG_INTERVAL_MS: u64 = 16;

/// Validate that a position is finite and within sane world-space bounds.
/// Returns `None` for NaN, Infinity, or out-of-range values (VULN-05).
fn sanitize_position(x: f32, y: f32, z: f32) -> Option<(f32, f32, f32)> {
    const MAX_BOUND: f32 = 10000.0;
    if x.is_finite()
        && y.is_finite()
        && z.is_finite()
        && x.abs() <= MAX_BOUND
        && y.abs() <= MAX_BOUND
        && z.abs() <= MAX_BOUND
    {
        Some((x, y, z))
    } else {
        None
    }
}

/// Fetch nodes from the graph service for streaming position updates.
///
/// Pre-flags all node IDs with their type (agent, knowledge, ontology) so that
/// downstream delta/binary encoding emits correct type bits on the wire.
pub(crate) async fn fetch_nodes(
    app_state: std::sync::Arc<crate::app_state::AppState>,
    _settings_addr: actix::Addr<crate::actors::optimized_settings_actor::OptimizedSettingsActor>,
) -> Option<(Vec<(u32, BinaryNodeData)>, bool, Vec<NodeVisibility>)> {
    use crate::actors::messages::{GetGraphData, GetNodeTypeArrays};
    use log::error;
    use std::collections::HashSet;

    let graph_data = match app_state.graph_service_addr.send(GetGraphData).await {
        Ok(Ok(data)) => data,
        Ok(Err(e)) => {
            error!("[WebSocket] Failed to get graph data: {}", e);
            return None;
        }
        Err(e) => {
            error!(
                "[WebSocket] Failed to send message to GraphServiceActor: {}",
                e
            );
            return None;
        }
    };

    if graph_data.nodes.is_empty() {
        // hot-path: trace only (fires every update cycle when graph is empty)
        trace!("[WebSocket] No nodes to send! Empty graph data.");
        return None;
    }

    // Fetch node type classification arrays for binary protocol flags (already remapped to compact wire IDs)
    let nta = match app_state.graph_service_addr.send(GetNodeTypeArrays).await {
        Ok(arrays) => arrays,
        Err(_) => crate::actors::messages::NodeTypeArrays::default(),
    };
    let agent_set: HashSet<u32> = nta.agent_ids.iter().copied().collect();
    let knowledge_set: HashSet<u32> = nta.knowledge_ids.iter().copied().collect();
    let ontology_class_set: HashSet<u32> = nta.ontology_class_ids.iter().copied().collect();
    let ontology_individual_set: HashSet<u32> =
        nta.ontology_individual_ids.iter().copied().collect();
    let ontology_property_set: HashSet<u32> = nta.ontology_property_ids.iter().copied().collect();

    let debug_enabled = crate::utils::logging::is_debug_enabled();
    let debug_websocket = debug_enabled;
    let detailed_debug = debug_enabled && debug_websocket;

    // hot-path: trace only (fires every update cycle, formats node data)
    if detailed_debug {
        trace!(
            "Raw nodes count: {}, showing first 5 nodes IDs:",
            graph_data.nodes.len()
        );
        for (i, node) in graph_data.nodes.iter().take(5).enumerate() {
            trace!(
                "  Node {}: id={} (compact), metadata_id={} (filename)",
                i,
                node.id,
                node.metadata_id
            );
        }
    }

    // ADR-060: only pay for the per-node visibility projection when the
    // drop-set filter is actually enabled; otherwise leave the vec empty so the
    // hot path is untouched.
    let filter_on = pubkey_visibility_filter_enabled();

    let mut nodes = Vec::with_capacity(graph_data.nodes.len());
    let mut visibility = if filter_on {
        Vec::with_capacity(graph_data.nodes.len())
    } else {
        Vec::new()
    };
    for node in &graph_data.nodes {
        // Node IDs are already compact (0..N-1) from GraphStateActor source remapping
        let compact_id = node.id;
        // Apply node type flags so the client can distinguish agent/knowledge/ontology nodes
        let flagged_id = if agent_set.contains(&compact_id) {
            binary_protocol::set_agent_flag(compact_id)
        } else if knowledge_set.contains(&compact_id) {
            binary_protocol::set_knowledge_flag(compact_id)
        } else if ontology_class_set.contains(&compact_id) {
            binary_protocol::set_ontology_class_flag(compact_id)
        } else if ontology_individual_set.contains(&compact_id) {
            binary_protocol::set_ontology_individual_flag(compact_id)
        } else if ontology_property_set.contains(&compact_id) {
            binary_protocol::set_ontology_property_flag(compact_id)
        } else {
            compact_id
        };
        if filter_on {
            visibility.push(node_visibility(flagged_id, node));
        }
        let node_data = BinaryNodeDataClient::new(
            flagged_id,
            node.data.position().into(),
            node.data.velocity().into(),
        );
        nodes.push((flagged_id, node_data));
    }

    if nodes.is_empty() {
        return None;
    }

    Some((nodes, detailed_debug, visibility))
}

pub(crate) fn handle_request_full_snapshot(
    _act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    debug!("Client requested full position snapshot");

    let graphs = msg.get("graphs").and_then(|g| g.as_array());
    let include_knowledge = graphs.map_or(true, |arr| {
        arr.iter().any(|v| v.as_str() == Some("knowledge"))
    });
    let include_agent = graphs.map_or(true, |arr| arr.iter().any(|v| v.as_str() == Some("agent")));

    let app_state = _act.app_state.clone();
    // ADR-060: the drop-set filter must also cover the snapshot path, not just
    // the live subscribe broadcast — a snapshot request would otherwise leak
    // private node positions with the filter enabled.
    let caller_pubkey = _act.pubkey.clone();
    let fut = async move {
        use crate::actors::messages::{GetBotsGraphData, GetGraphData, GetNodeTypeArrays};
        use std::collections::HashSet;

        // hot-path: trace only (fires per snapshot request)
        trace!(
            "RequestPositionSnapshot: include_knowledge={}, include_agent={}",
            include_knowledge,
            include_agent
        );

        let mut knowledge_nodes = Vec::new();
        let mut agent_nodes = Vec::new();
        let filter_on = pubkey_visibility_filter_enabled();
        let mut visibility: Vec<NodeVisibility> = Vec::new();

        // Fetch node type arrays (already compact IDs from source remapping)
        let nta = app_state
            .graph_service_addr
            .send(GetNodeTypeArrays)
            .await
            .unwrap_or_default();
        let agent_set: HashSet<u32> = nta.agent_ids.iter().copied().collect();
        let ontology_class_set: HashSet<u32> = nta.ontology_class_ids.iter().copied().collect();
        let ontology_individual_set: HashSet<u32> =
            nta.ontology_individual_ids.iter().copied().collect();
        let ontology_property_set: HashSet<u32> =
            nta.ontology_property_ids.iter().copied().collect();

        if include_knowledge {
            if let Ok(Ok(graph_data)) = app_state.graph_service_addr.send(GetGraphData).await {
                for node in &graph_data.nodes {
                    let compact_id = node.id;
                    // Apply type flags for correct client-side classification
                    let flagged_id = if agent_set.contains(&compact_id) {
                        binary_protocol::set_agent_flag(compact_id)
                    } else if ontology_class_set.contains(&compact_id) {
                        binary_protocol::set_ontology_class_flag(compact_id)
                    } else if ontology_individual_set.contains(&compact_id) {
                        binary_protocol::set_ontology_individual_flag(compact_id)
                    } else if ontology_property_set.contains(&compact_id) {
                        binary_protocol::set_ontology_property_flag(compact_id)
                    } else {
                        binary_protocol::set_knowledge_flag(compact_id)
                    };
                    let node_data = BinaryNodeData {
                        node_id: flagged_id,
                        x: node.data.x,
                        y: node.data.y,
                        z: node.data.z,
                        vx: node.data.vx,
                        vy: node.data.vy,
                        vz: node.data.vz,
                    };
                    if filter_on {
                        visibility.push(node_visibility(flagged_id, node));
                    }
                    if agent_set.contains(&compact_id) {
                        agent_nodes.push((flagged_id, node_data));
                    } else {
                        knowledge_nodes.push((flagged_id, node_data));
                    }
                }
            }
        }

        // ADR-060 / ADR-059 §Phase-4: drop private nodes before the snapshot
        // leaves the server. Default OFF (`visibility` stays empty).
        if !visibility.is_empty() {
            let drop_set = compute_private_opaque_ids(&visibility, caller_pubkey.as_deref());
            if !drop_set.is_empty() {
                let before = knowledge_nodes.len() + agent_nodes.len();
                knowledge_nodes.retain(|(id, _)| !drop_set.contains(id));
                agent_nodes.retain(|(id, _)| !drop_set.contains(id));
                debug!(
                    "ADR-060 snapshot filter dropped {} private nodes",
                    before - (knowledge_nodes.len() + agent_nodes.len())
                );
            }
        }

        if include_agent {
            if let Ok(Ok(bots_data)) = app_state.graph_service_addr.send(GetBotsGraphData).await {
                for node in &bots_data.nodes {
                    // Bots graph IDs — use as-is (bots have their own ID scheme)
                    let compact_id = node.id;
                    let node_data = BinaryNodeData {
                        node_id: compact_id,
                        x: node.data.x,
                        y: node.data.y,
                        z: node.data.z,
                        vx: node.data.vx,
                        vy: node.data.vy,
                        vz: node.data.vz,
                    };
                    agent_nodes.push((compact_id, node_data));
                }
            }
        }

        crate::actors::messages::PositionSnapshot {
            knowledge_nodes,
            agent_nodes,
            timestamp: std::time::Instant::now(),
        }
    };

    let fut = actix::fut::wrap_future::<_, SocketFlowServer>(fut);
    ctx.spawn(fut.map(move |snapshot, _act, ctx| {
        let mut all_nodes = Vec::new();

        // IDs were already stamped with the correct type flag when the snapshot
        // was built (knowledge bucket also carries ontology-class/individual/
        // property flags). Re-applying set_knowledge_flag here would mask off
        // those ontology bits and mis-classify every ontology node as knowledge.
        for (id, data) in snapshot.knowledge_nodes {
            all_nodes.push((id, data));
        }

        for (id, data) in snapshot.agent_nodes {
            all_nodes.push((id, data));
        }

        if !all_nodes.is_empty() {
            let analytics = _act.app_state.node_analytics.read().ok();
            let analytics_ref = analytics.as_deref();
            let sssp = _act.app_state.node_sssp.read().ok();
            let sssp_ref = sssp.as_deref();
            let binary_data = binary_protocol::encode_node_data_with_live_analytics(
                &all_nodes,
                analytics_ref,
                sssp_ref,
            );
            ctx.binary(binary_data);
            debug!("Sent position snapshot with {} nodes", all_nodes.len());
        }
    }));
}

/// Minimum gap between `requestInitialData` full-graph pushes on one
/// connection. A legitimate client requests initial data once per connection
/// (a reconnect-in-place after >30s is still served); anything faster is a
/// client bug or an abuse attempt and is dropped.
const INITIAL_DATA_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn handle_request_initial_data(
    act: &mut SocketFlowServer,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // #2 (codex): `send_full_state_sync` triggers a full-graph fetch + quality
    // sort + binary encode. Without a guard, a client can spam
    // `requestInitialData` to force that work unbounded (a cheap DoS). Serve at
    // most once per INITIAL_DATA_COOLDOWN per connection; drop + log repeats.
    let now = std::time::Instant::now();
    if let Some(last) = act.last_initial_data_request {
        if now.duration_since(last) < INITIAL_DATA_COOLDOWN {
            warn!(
                "Dropping repeated requestInitialData from {} (<{}s since last; DoS guard)",
                act.client_ip,
                INITIAL_DATA_COOLDOWN.as_secs()
            );
            return;
        }
    }
    act.last_initial_data_request = Some(now);

    // #4: previously this only replied with an `initialDataInfo` stub telling
    // the client to call a REST endpoint first. The anonymous Godot client has
    // no REST bootstrap step, so it never received nodes/edges. Push the real
    // `initialGraphLoad` (plus state_sync + binary positions) directly, using
    // the same builder as the connect/Authenticate path
    // (`send_full_state_sync`), which respects the settings-driven
    // `initialNodeLimit` (default 3000, top-N by quality, raisable via
    // /api/settings) and — via the added visibility gate — the
    // PUBKEY_VISIBILITY_FILTER drop-set when enabled.
    info!("Client requested initial data - pushing initialGraphLoad directly");
    act.last_activity = now;
    act.send_full_state_sync(ctx);
}

pub(crate) fn handle_enable_randomization(msg: &serde_json::Value) {
    let enabled = msg
        .get("enabled")
        .and_then(|e| e.as_bool())
        .unwrap_or(false);
    info!(
        "Client requested to {} node position randomization (server-side removed, client-side used instead)",
        if enabled { "enable" } else { "disable" }
    );
}

pub(crate) fn handle_request_bots_graph(
    act: &mut SocketFlowServer,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    info!("Client requested bots graph - returning optimized position data only");

    let graph_addr = act.app_state.graph_service_addr.clone();

    ctx.spawn(
        actix::fut::wrap_future::<_, SocketFlowServer>(async move {
            use crate::actors::messages::GetBotsGraphData;
            match graph_addr.send(GetBotsGraphData).await {
                Ok(Ok(graph_data)) => Some(graph_data),
                _ => None,
            }
        })
        .map(|graph_data_opt, _act, ctx| {
            if let Some(graph_data) = graph_data_opt {
                let minimal_nodes: Vec<serde_json::Value> = graph_data
                    .nodes
                    .iter()
                    .map(|node| {
                        serde_json::json!({
                            "id": node.id,
                            "metadata_id": node.metadata_id,
                            "x": node.data.x,
                            "y": node.data.y,
                            "z": node.data.z,
                            "vx": node.data.vx,
                            "vy": node.data.vy,
                            "vz": node.data.vz
                        })
                    })
                    .collect();

                let minimal_edges: Vec<serde_json::Value> = graph_data
                    .edges
                    .iter()
                    .map(|edge| {
                        serde_json::json!({
                            "id": edge.id,
                            "source": edge.source,
                            "target": edge.target,
                            "weight": edge.weight
                        })
                    })
                    .collect();

                let response = serde_json::json!({
                    "type": "botsGraphUpdate",
                    "data": {
                        "nodes": minimal_nodes,
                        "edges": minimal_edges,
                    },
                    "meta": {
                        "optimized": true,
                        "message": "This response contains only position data. For full agent details:",
                        "api_endpoints": {
                            "full_agent_data": "/api/bots/data",
                            "agent_status": "/api/bots/status",
                            "individual_agent": "/api/agents/{id}"
                        }
                    },
                    "timestamp": chrono::Utc::now().timestamp_millis()
                });

                if let Ok(msg_str) = serde_json::to_string(&response) {
                    let original_size = graph_data.nodes.len() * 500;
                    let optimized_size = msg_str.len();
                    debug!(
                        "Sending optimized bots graph: {} nodes, {} edges ({} bytes, est. {}% reduction)",
                        minimal_nodes.len(),
                        minimal_edges.len(),
                        optimized_size,
                        if original_size > 0 {
                            100 - (optimized_size * 100 / original_size)
                        } else {
                            0
                        }
                    );
                    ctx.text(msg_str);
                }
            } else {
                warn!("No bots graph data available");
                let response = serde_json::json!({
                    "type": "botsGraphUpdate",
                    "error": "No data available",
                    "meta": {
                        "api_endpoints": {
                            "full_agent_data": "/api/bots/data",
                            "agent_status": "/api/bots/status"
                        }
                    },
                    "timestamp": chrono::Utc::now().timestamp_millis()
                });
                if let Ok(msg_str) = serde_json::to_string(&response) {
                    ctx.text(msg_str);
                }
            }
        }),
    );
}

pub(crate) fn handle_request_bots_positions(
    act: &mut SocketFlowServer,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    debug!("Client requested bots position updates");

    let app_state = act.app_state.clone();

    ctx.spawn(
        actix::fut::wrap_future::<_, SocketFlowServer>(async move {
            let bots_nodes =
                crate::handlers::bots_handler::get_bots_positions(&app_state.bots_client).await;

            if bots_nodes.is_empty() {
                return vec![];
            }

            let mut nodes_data = Vec::new();
            for node in bots_nodes {
                // Node IDs are already compact from source remapping
                let compact_id = node.id;
                // Flag bots/agent nodes so the client renders them in AgentNodesLayer
                let flagged_id = binary_protocol::set_agent_flag(compact_id);
                let node_data = BinaryNodeData {
                    node_id: flagged_id,
                    x: node.data.x,
                    y: node.data.y,
                    z: node.data.z,
                    vx: node.data.vx,
                    vy: node.data.vy,
                    vz: node.data.vz,
                };
                nodes_data.push((flagged_id, node_data));
            }

            nodes_data
        })
        .map(|nodes_data, _act, ctx| {
            if !nodes_data.is_empty() {
                let analytics = _act.app_state.node_analytics.read().ok();
                let analytics_ref = analytics.as_deref();
                let sssp = _act.app_state.node_sssp.read().ok();
                let sssp_ref = sssp.as_deref();
                let binary_data = binary_protocol::encode_node_data_with_live_analytics(
                    &nodes_data,
                    analytics_ref,
                    sssp_ref,
                );

                // hot-path: trace only (fires per bots position update cycle)
                trace!(
                    "Sending bots positions: {} nodes, {} bytes",
                    nodes_data.len(),
                    binary_data.len()
                );

                ctx.binary(binary_data);
            }
        }),
    );

    let response = serde_json::json!({
        "type": "botsUpdatesStarted",
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&response) {
        ctx.text(msg_str);
    }
}

pub(crate) fn handle_subscribe_position_updates(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // hot-path: trace only (re-fires every interval via run_later re-subscription loop)
    trace!("Client requested position update subscription");

    // Per-session rate limit: ignore subscribes arriving within 2s of the last
    // accepted one. A client stuck in a reconnect/subscribe loop (observed at
    // ~400ms cadence) otherwise restarts the broadcast loop and re-snapshots
    // continuously, degrading the pipeline for every connected client. The
    // duplicate-loop generation collapse below handles correctness; this guard
    // handles the resource churn. Legitimate clients subscribe once per
    // connection (plus ~30s watchdog probes), far below this limit.
    if let Some(last) = act.last_position_subscribe {
        if last.elapsed() < std::time::Duration::from_secs(2) {
            trace!("subscribe_position_updates rate-limited (per-session, <2s since last)");
            return;
        }
    }
    act.last_position_subscribe = Some(std::time::Instant::now());

    // Collapse duplicate subscriptions to a single broadcast loop. A client that
    // subscribes more than once (AppInitializer fires on both connection-status-change
    // and connection_established) would otherwise spawn one independent run_later
    // self-loop per subscribe, doubling the binary broadcast rate. Bumping the
    // generation orphans every older loop: each tick below stops once it sees a newer
    // generation, so exactly one loop survives regardless of subscribe count.
    act.position_sub_generation = act.position_sub_generation.wrapping_add(1);
    let my_generation = act.position_sub_generation;

    let interval = msg
        .get("data")
        .and_then(|data| data.get("interval"))
        .and_then(|interval| interval.as_u64())
        .unwrap_or(60);

    let binary = msg
        .get("data")
        .and_then(|data| data.get("binary"))
        .and_then(|binary| binary.as_bool())
        .unwrap_or(true);

    // FIX 6: Parse optional nodeTypes filter from subscription message.
    // Example: { "type": "subscribe_position_updates", "data": { "nodeTypes": ["knowledge", "agent"] } }
    // When specified, only nodes matching these types are included in binary broadcasts.
    let node_types: std::collections::HashSet<String> = msg
        .get("data")
        .and_then(|data| data.get("nodeTypes"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect()
        })
        .unwrap_or_default();

    if !node_types.is_empty() {
        info!("Client subscribed with node type filter: {:?}", node_types);
    }
    act.subscribed_node_types = node_types;

    let min_allowed_interval =
        1000 / (EndpointRateLimits::socket_flow_updates().requests_per_minute / 60);
    let actual_interval = interval.max(min_allowed_interval as u64);

    // hot-path: trace only (fires every re-subscription cycle)
    if actual_interval != interval {
        trace!(
            "Adjusted position update interval from {}ms to {}ms to comply with rate limits",
            interval,
            actual_interval
        );
    }

    // hot-path: trace only (fires every re-subscription cycle)
    trace!(
        "Starting position updates with interval: {}ms, binary: {}",
        actual_interval,
        binary
    );

    let update_interval = std::time::Duration::from_millis(actual_interval);
    let app_state = act.app_state.clone();
    let settings_addr = act.app_state.settings_addr.clone();

    let response = serde_json::json!({
        "type": "subscription_confirmed",
        "subscription": "position_updates",
        "interval": actual_interval,
        "binary": binary,
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "rate_limit": {
            "requests_per_minute": EndpointRateLimits::socket_flow_updates().requests_per_minute,
            "min_interval_ms": min_allowed_interval
        }
    });
    if let Ok(msg_str) = serde_json::to_string(&response) {
        ctx.text(msg_str);
    }

    ctx.run_later(update_interval, move |_act, ctx| {
        let fut = fetch_nodes(app_state.clone(), settings_addr.clone());
        let fut = actix::fut::wrap_future::<_, SocketFlowServer>(fut);

        ctx.spawn(fut.map(move |result, act, ctx| {
            // A newer subscribe superseded this loop while the fetch was in flight;
            // drop the result so only the latest loop broadcasts (no duplicate frames).
            if act.position_sub_generation != my_generation {
                return;
            }
            if let Some((mut nodes, detailed_debug, visibility)) = result {
                // FIX 6: Apply per-client node type filter to reduce bandwidth.
                // When the client subscribes with nodeTypes, skip nodes that
                // don't match. Type is determined from the flag bits in the node ID.
                if !act.subscribed_node_types.is_empty() {
                    let type_filter = &act.subscribed_node_types;
                    nodes.retain(|(flagged_id, _)| {
                        let node_type = binary_protocol::get_node_type(*flagged_id);
                        let type_str = match node_type {
                            binary_protocol::NodeType::Agent => "agent",
                            binary_protocol::NodeType::Knowledge => "knowledge",
                            binary_protocol::NodeType::OntologyClass => "ontology",
                            binary_protocol::NodeType::OntologyIndividual => "ontology",
                            binary_protocol::NodeType::OntologyProperty => "ontology",
                            binary_protocol::NodeType::Unknown => "unknown",
                        };
                        type_filter.contains(type_str)
                    });
                }

                // ADR-060 / ADR-059 §Phase-4: pubkey-visibility drop-set filter.
                // Default OFF (fetch_nodes returns an empty `visibility` vec when
                // the flag is unset, so this branch is skipped and the frame is
                // untouched). When enabled, drop every (id, data) pair for a node
                // that is private and not owned by this session's pubkey, before
                // it reaches the encoder. Fail-closed: an unauthenticated session
                // (`act.pubkey == None`) drops all private nodes → public-only.
                if !visibility.is_empty() {
                    let drop_set =
                        compute_private_opaque_ids(&visibility, act.pubkey.as_deref());
                    let dropped = apply_drop_set(&mut nodes, &drop_set);
                    if dropped > 0 {
                        debug!(
                            "Sent position frame with {} nodes ({} dropped by {})",
                            nodes.len(),
                            dropped,
                            PUBKEY_VISIBILITY_FILTER_ENV
                        );
                    }
                }

                // Single full-state frame per tick. No delta encoding, no
                // per-client previous-state, no version dispatch. Physics is
                // whole-graph (all nodes settle together) and the client lerps
                // toward the latest received positions — deltas add cost
                // without bandwidth savings under that workload.
                let analytics = act.app_state.node_analytics.read().ok();
                let analytics_ref = analytics.as_deref();
                let binary_data = binary_protocol::encode_node_data_extended_with_sssp(
                    &nodes,
                    &[], // agent_node_ids — fetch_nodes() already flagged IDs
                    &[], // knowledge_node_ids — fetch_nodes() already flagged IDs
                    &[], // ontology_class_ids
                    &[], // ontology_individual_ids
                    &[], // ontology_property_ids
                    None, // sssp_data
                    analytics_ref,
                );

                act.total_node_count = nodes.len();
                let moving_nodes = nodes
                    .iter()
                    .filter(|(_, node_data)| {
                        let vel = node_data.velocity();
                        vel.x.abs() > 0.001 || vel.y.abs() > 0.001 || vel.z.abs() > 0.001
                    })
                    .count();
                act.nodes_in_motion = moving_nodes;

                act.last_transfer_size = binary_data.len();
                act.total_bytes_sent += binary_data.len();
                act.update_count += 1;
                act.nodes_sent_count += nodes.len();

                if detailed_debug {
                    debug!(
                        "[Position Updates] Broadcast: {} nodes, {} moving, {} bytes",
                        nodes.len(),
                        moving_nodes,
                        binary_data.len()
                    );
                }

                ctx.binary(binary_data);

                let next_interval = std::time::Duration::from_millis(actual_interval);
                ctx.run_later(next_interval, move |act, ctx| {
                    // Stop re-injecting once a newer subscribe takes over this client,
                    // otherwise duplicate loops would persist indefinitely.
                    if act.position_sub_generation != my_generation {
                        return;
                    }
                    let subscription_msg = format!(
                        "{{\"type\":\"subscribe_position_updates\",\"data\":{{\"interval\":{},\"binary\":{}}}}}",
                        actual_interval, binary
                    );
                    <SocketFlowServer as StreamHandler<
                        Result<ws::Message, ws::ProtocolError>,
                    >>::handle(
                        act,
                        Ok(ws::Message::Text(subscription_msg.into())),
                        ctx,
                    );
                });
            } else {
                // fetch_nodes returned None (transient: graph empty mid-rebuild,
                // or GraphServiceActor momentarily unavailable). DO NOT let the
                // self-perpetuating loop die — a single dropped tick would
                // silently stop position streaming for the rest of the client's
                // session. Reschedule the re-subscribe so the loop self-heals
                // once the graph repopulates.
                let retry_interval = std::time::Duration::from_millis(actual_interval);
                ctx.run_later(retry_interval, move |act, ctx| {
                    if act.position_sub_generation != my_generation {
                        return;
                    }
                    let subscription_msg = format!(
                        "{{\"type\":\"subscribe_position_updates\",\"data\":{{\"interval\":{},\"binary\":{}}}}}",
                        actual_interval, binary
                    );
                    <SocketFlowServer as StreamHandler<
                        Result<ws::Message, ws::ProtocolError>,
                    >>::handle(
                        act,
                        Ok(ws::Message::Text(subscription_msg.into())),
                        ctx,
                    );
                });
            }
        }));
    });
}

pub(crate) fn handle_request_swarm_telemetry(
    act: &mut SocketFlowServer,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    debug!("Client requested enhanced swarm telemetry");

    let app_state = act.app_state.clone();

    ctx.spawn(
        actix::fut::wrap_future::<_, SocketFlowServer>(async move {
            match crate::handlers::bots_handler::fetch_hive_mind_agents(&app_state, None).await {
                Ok(agents) => {
                    let mut nodes_data = Vec::new();
                    let mut swarm_metrics = serde_json::json!({
                        "total_agents": agents.len(),
                        "active_agents": 0,
                        "avg_health": 0.0,
                        "avg_cpu": 0.0,
                        "avg_workload": 0.0,
                        "total_tokens": 0,
                        "swarm_ids": std::collections::HashSet::<String>::new(),
                    });

                    let (mut active_count, mut total_health, mut total_cpu, mut total_workload) =
                        (0u32, 0.0f32, 0.0f32, 0.0f32);

                    for (idx, agent) in agents.iter().enumerate() {
                        if agent.status == "active" {
                            active_count += 1;
                        }
                        total_health += agent.health;
                        total_cpu += agent.cpu_usage;
                        total_workload += agent.workload;

                        let id = (1000 + idx) as u32;
                        // Flag swarm telemetry nodes as agents for client-side rendering
                        let flagged_id = binary_protocol::set_agent_flag(id);
                        let node_data = BinaryNodeData {
                            node_id: flagged_id,
                            x: (idx as f32 * 100.0).sin() * 500.0,
                            y: (idx as f32 * 100.0).cos() * 500.0,
                            z: 0.0,
                            vx: 0.0,
                            vy: 0.0,
                            vz: 0.0,
                        };
                        nodes_data.push((flagged_id, node_data));
                    }

                    let n = agents.len() as f32;
                    if n > 0.0 {
                        swarm_metrics["active_agents"] = serde_json::json!(active_count);
                        swarm_metrics["avg_health"] = serde_json::json!(total_health / n);
                        swarm_metrics["avg_cpu"] = serde_json::json!(total_cpu / n);
                        swarm_metrics["avg_workload"] = serde_json::json!(total_workload / n);
                        swarm_metrics["total_tokens"] = serde_json::json!(0);
                        swarm_metrics["swarm_count"] = serde_json::json!(0);
                    }

                    (nodes_data, swarm_metrics)
                }
                Err(_) => (vec![], serde_json::json!({})),
            }
        })
        .map(|(nodes_data, swarm_metrics), _act, ctx| {
            if !nodes_data.is_empty() {
                let analytics = _act.app_state.node_analytics.read().ok();
                let analytics_ref = analytics.as_deref();
                let sssp = _act.app_state.node_sssp.read().ok();
                let sssp_ref = sssp.as_deref();
                let binary_data = binary_protocol::encode_node_data_with_live_analytics(
                    &nodes_data,
                    analytics_ref,
                    sssp_ref,
                );
                ctx.binary(binary_data);
            }

            let telemetry_response = serde_json::json!({
                "type": "swarmTelemetry",
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "data_source": "live",
                "metrics": swarm_metrics,
                "node_count": nodes_data.len()
            });

            if let Ok(msg_str) = serde_json::to_string(&telemetry_response) {
                ctx.text(msg_str);
            }
        }),
    );
}

// ---------------------------------------------------------------------------
// Server-side drag handling
// ---------------------------------------------------------------------------

/// Handle `nodeDragStart` from client.
///
/// Pins the node at its current (or client-reported) position and notifies
/// the physics orchestrator so the simulation can resume if auto-paused.
///
/// Expected message shape:
/// ```json
/// { "type": "nodeDragStart", "data": { "nodeId": 42, "position": { "x": 1.0, "y": 2.0, "z": 3.0 } } }
/// ```
pub(crate) fn handle_node_drag_start(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // VULN-01: Reject unauthenticated clients
    if act.pubkey.is_none() {
        warn!("[Drag] Rejecting drag from unauthenticated client");
        return;
    }

    let data = match msg.get("data") {
        Some(d) => d,
        None => {
            warn!("[Drag] nodeDragStart missing 'data' field");
            return;
        }
    };

    // VULN-03: Validate nodeId fits in u32 (prevent silent truncation)
    let node_id = match data.get("nodeId").and_then(|v| v.as_u64()) {
        Some(id) if id <= u32::MAX as u64 => id as u32,
        _ => {
            warn!("[Drag] Invalid or missing nodeId");
            return;
        }
    };

    let pos_x = data
        .get("position")
        .and_then(|p| p.get("x"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let pos_y = data
        .get("position")
        .and_then(|p| p.get("y"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let pos_z = data
        .get("position")
        .and_then(|p| p.get("z"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;

    // VULN-05: Reject NaN / Infinity / out-of-bounds positions
    let (pos_x, pos_y, pos_z) = match sanitize_position(pos_x, pos_y, pos_z) {
        Some(p) => p,
        None => {
            warn!(
                "[Drag] nodeDragStart: rejecting invalid position [{}, {}, {}]",
                pos_x, pos_y, pos_z
            );
            return;
        }
    };

    // VULN-10: Cap simultaneous drags per client
    if act.dragged_nodes.len() >= MAX_DRAGGED_NODES_PER_CLIENT
        && !act.dragged_nodes.contains(&node_id)
    {
        warn!(
            "[Drag] Client exceeded max simultaneous drags ({})",
            MAX_DRAGGED_NODES_PER_CLIENT
        );
        return;
    }

    info!(
        "[Drag] nodeDragStart: node_id={}, pos=[{:.2}, {:.2}, {:.2}]",
        node_id, pos_x, pos_y, pos_z
    );

    // Track drag state on this connection
    act.dragged_nodes.insert(node_id);
    act.drag_last_update.insert(node_id, Instant::now());

    // Pin the node at client position + notify physics to resume if paused
    let app_state = act.app_state.clone();

    let fut = async move {
        // 1. Send NodeInteractionMessage to resume physics if auto-paused
        use crate::actors::messages::{NodeInteractionMessage, NodeInteractionType};
        app_state
            .graph_service_addr
            .do_send(NodeInteractionMessage {
                node_id,
                interaction_type: NodeInteractionType::Dragged,
                position: Some([pos_x, pos_y, pos_z]),
            });

        // 2. Update the node position in graph state (velocity zeroed -- pinned)
        use crate::actors::messages::UpdateNodePositions;
        let pinned_data = BinaryNodeData {
            node_id,
            x: pos_x,
            y: pos_y,
            z: pos_z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        };
        app_state.graph_service_addr.do_send(UpdateNodePositions {
            positions: vec![(node_id, pinned_data)],
            correlation_id: None,
        });

        // 3. Grab→spring: pin the node ON THE GPU so the force kernel holds it at
        //    the hand position and springs its neighbours around it. Reheat on
        //    drag-start so a settled graph has energy to visibly react.
        use crate::actors::messages::PinNodePositions;
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            gpu_addr.do_send(PinNodePositions {
                pins: vec![(node_id, [pos_x, pos_y, pos_z])],
                unpin: Vec::new(),
                reheat: true,
            });
        }
    };

    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _, _| ()));

    // Acknowledge to the client
    let ack = serde_json::json!({
        "type": "nodeDragStartAck",
        "data": { "nodeId": node_id },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&ack) {
        ctx.text(msg_str);
    }

    // Start drag timeout checker for this node
    let timeout_ms = act.drag_timeout_ms;
    let drag_node_id = node_id;
    ctx.run_later(
        std::time::Duration::from_millis(timeout_ms + 100),
        move |act, ctx| {
            check_drag_timeout(act, drag_node_id, ctx);
        },
    );
}

/// Handle `nodeDragUpdate` from client during an active drag.
///
/// Updates the pinned node's position and runs a time-budgeted settle
/// cycle for the rest of the graph, then broadcasts results to all clients.
///
/// Expected message shape:
/// ```json
/// { "type": "nodeDragUpdate", "data": { "nodeId": 42, "position": { "x": 1.0, "y": 2.0, "z": 3.0 }, "timestamp": 1234567890 } }
/// ```
pub(crate) fn handle_node_drag_update(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // VULN-01: Reject unauthenticated clients
    if act.pubkey.is_none() {
        warn!("[Drag] Rejecting drag from unauthenticated client");
        return;
    }

    let data = match msg.get("data") {
        Some(d) => d,
        None => return,
    };

    // VULN-03: Validate nodeId fits in u32 (prevent silent truncation)
    let node_id = match data.get("nodeId").and_then(|v| v.as_u64()) {
        Some(id) if id <= u32::MAX as u64 => id as u32,
        _ => {
            warn!("[Drag] Invalid or missing nodeId");
            return;
        }
    };

    // VULN-02: Server-side rate limit on drag updates (~60 Hz max)
    if let Some(last) = act.drag_last_update.get(&node_id) {
        if last.elapsed() < std::time::Duration::from_millis(MIN_DRAG_INTERVAL_MS) {
            return; // Drop excess updates silently
        }
    }

    // Ignore updates for nodes we haven't received a drag start for
    if !act.dragged_nodes.contains(&node_id) {
        debug!(
            "[Drag] Received dragUpdate for non-dragged node {}, treating as implicit drag start",
            node_id
        );
        // VULN-10: Cap simultaneous drags per client (implicit drag start path)
        if act.dragged_nodes.len() >= MAX_DRAGGED_NODES_PER_CLIENT
            && !act.dragged_nodes.contains(&node_id)
        {
            warn!(
                "[Drag] Client exceeded max simultaneous drags ({})",
                MAX_DRAGGED_NODES_PER_CLIENT
            );
            return;
        }
        // Implicit drag start -- pin and track
        act.dragged_nodes.insert(node_id);
    }

    let pos_x = data
        .get("position")
        .and_then(|p| p.get("x"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let pos_y = data
        .get("position")
        .and_then(|p| p.get("y"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let pos_z = data
        .get("position")
        .and_then(|p| p.get("z"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;

    // VULN-05: Reject NaN / Infinity / out-of-bounds positions
    let (pos_x, pos_y, pos_z) = match sanitize_position(pos_x, pos_y, pos_z) {
        Some(p) => p,
        None => {
            warn!(
                "[Drag] nodeDragUpdate: rejecting invalid position [{}, {}, {}]",
                pos_x, pos_y, pos_z
            );
            return;
        }
    };

    // Multi-client conflict: last-write-wins via timestamp
    let client_ts = data.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
    _ = client_ts; // Timestamp available for future last-write-wins comparisons

    // Update the drag timeout tracker
    act.drag_last_update.insert(node_id, Instant::now());

    let app_state = act.app_state.clone();
    let client_manager_addr = act.client_manager_addr.clone();

    let fut = async move {
        let settle_start = Instant::now();

        // 1. Move the pinned node to the new client-reported position (velocity zeroed)
        use crate::actors::messages::UpdateNodePositions;
        let pinned_data = BinaryNodeData {
            node_id,
            x: pos_x,
            y: pos_y,
            z: pos_z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        };
        app_state.graph_service_addr.do_send(UpdateNodePositions {
            positions: vec![(node_id, pinned_data)],
            correlation_id: None,
        });

        // 1b. Grab→spring: update the GPU pin so the force kernel holds the node at
        //     the new hand position and its neighbours spring around it. No reheat on
        //     per-frame updates (only on drag-start).
        use crate::actors::messages::PinNodePositions;
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            gpu_addr.do_send(PinNodePositions {
                pins: vec![(node_id, [pos_x, pos_y, pos_z])],
                unpin: Vec::new(),
                reheat: false,
            });
        }

        // 2. Run time-budgeted settle iterations for neighbor relaxation
        //    We run multiple SimulationSteps within our time budget.
        use crate::actors::messages::SimulationStep;
        let budget = std::time::Duration::from_millis(DRAG_SETTLE_BUDGET_MS);

        let mut iterations = 0u32;
        while settle_start.elapsed() < budget && iterations < 10 {
            // We use send() to await completion of each step before the next
            match app_state.graph_service_addr.send(SimulationStep).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    debug!("[Drag] Settle step {} failed: {}", iterations, e);
                    break;
                }
                Err(e) => {
                    debug!("[Drag] Settle step {} mailbox error: {}", iterations, e);
                    break;
                }
            }
            iterations += 1;
        }

        debug!(
            "[Drag] Ran {} settle iterations for node {} in {:.1}ms",
            iterations,
            node_id,
            settle_start.elapsed().as_secs_f64() * 1000.0
        );

        // 3. Fetch updated positions and broadcast to all clients (IDs already compact)
        use crate::actors::messages::GetGraphData;

        if let Ok(Ok(graph_data)) = app_state.graph_service_addr.send(GetGraphData).await {
            let node_data: Vec<(u32, BinaryNodeData)> = graph_data
                .nodes
                .iter()
                .map(|node| {
                    // Node IDs are already compact (0..N-1) from source remapping
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

            if !node_data.is_empty() {
                use crate::actors::messages::BroadcastNodePositions;
                let analytics = app_state.node_analytics.read().ok();
                let analytics_ref = analytics.as_deref();
                let sssp = app_state.node_sssp.read().ok();
                let sssp_ref = sssp.as_deref();
                let binary_data = binary_protocol::encode_node_data_with_live_analytics(
                    &node_data,
                    analytics_ref,
                    sssp_ref,
                );
                client_manager_addr.do_send(BroadcastNodePositions {
                    positions: binary_data,
                });
            }
        }
    };

    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));
}

/// Handle `nodeDragEnd` from client.
///
/// Leaves the node PINNED in place at the drop position (persistent grab-and-place),
/// runs one final settle cycle so neighbours relax around the held node, and
/// broadcasts the resulting positions. The node stays anchored until an explicit
/// `nodeUnpin` message (see [`handle_node_unpin`]).
///
/// Expected message shape:
/// ```json
/// { "type": "nodeDragEnd", "data": { "nodeId": 42 } }
/// ```
pub(crate) fn handle_node_drag_end(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // VULN-01: Reject unauthenticated clients
    if act.pubkey.is_none() {
        warn!("[Drag] Rejecting drag from unauthenticated client");
        return;
    }

    let data = match msg.get("data") {
        Some(d) => d,
        None => {
            warn!("[Drag] nodeDragEnd missing 'data' field");
            return;
        }
    };

    // VULN-03: Validate nodeId fits in u32 (prevent silent truncation)
    let node_id = match data.get("nodeId").and_then(|v| v.as_u64()) {
        Some(id) if id <= u32::MAX as u64 => id as u32,
        _ => {
            warn!("[Drag] Invalid or missing nodeId");
            return;
        }
    };

    info!("[Drag] nodeDragEnd: node_id={}", node_id);

    // Remove from drag tracking
    act.dragged_nodes.remove(&node_id);
    act.drag_last_update.remove(&node_id);

    let app_state = act.app_state.clone();
    let client_manager_addr = act.client_manager_addr.clone();

    let fut = async move {
        // 1. Notify physics that the drag interaction ended (node released)
        use crate::actors::messages::{NodeInteractionMessage, NodeInteractionType};
        app_state
            .graph_service_addr
            .do_send(NodeInteractionMessage {
                node_id,
                interaction_type: NodeInteractionType::Released,
                position: None,
            });

        // 1b. Grab→pin-in-place: on drag-END the node STAYS pinned at the position
        //     where it was dropped. It is NOT released here — it remains anchored
        //     on the GPU (integration skipped, still exerting forces) until an
        //     explicit `nodeUnpin` message (handle_node_unpin) clears it. This is
        //     the persistent VR grab-and-place semantics (see task PHASE 1). The
        //     node was already pinned at its final position by the last drag update,
        //     so no further pin message is required here.

        // 2. Run one final settle cycle so neighbours relax around the held node
        use crate::actors::messages::SimulationStep;
        let budget = std::time::Duration::from_millis(DRAG_SETTLE_BUDGET_MS);
        let settle_start = Instant::now();
        let mut iterations = 0u32;

        while settle_start.elapsed() < budget && iterations < 10 {
            match app_state.graph_service_addr.send(SimulationStep).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    debug!("[Drag] Final settle step {} failed: {}", iterations, e);
                    break;
                }
                Err(e) => {
                    debug!(
                        "[Drag] Final settle step {} mailbox error: {}",
                        iterations, e
                    );
                    break;
                }
            }
            iterations += 1;
        }

        debug!(
            "[Drag] Final settle: {} iterations for node {} in {:.1}ms",
            iterations,
            node_id,
            settle_start.elapsed().as_secs_f64() * 1000.0
        );

        // 3. Broadcast final positions to all clients (IDs already compact)
        use crate::actors::messages::GetGraphData;

        if let Ok(Ok(graph_data)) = app_state.graph_service_addr.send(GetGraphData).await {
            let node_data: Vec<(u32, BinaryNodeData)> = graph_data
                .nodes
                .iter()
                .map(|node| {
                    // Node IDs are already compact (0..N-1) from source remapping
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

            if !node_data.is_empty() {
                use crate::actors::messages::BroadcastNodePositions;
                let analytics = app_state.node_analytics.read().ok();
                let analytics_ref = analytics.as_deref();
                let sssp = app_state.node_sssp.read().ok();
                let sssp_ref = sssp.as_deref();
                let binary_data = binary_protocol::encode_node_data_with_live_analytics(
                    &node_data,
                    analytics_ref,
                    sssp_ref,
                );
                client_manager_addr.do_send(BroadcastNodePositions {
                    positions: binary_data,
                });
            }
        }
    };

    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));

    // Acknowledge drag end
    let ack = serde_json::json!({
        "type": "nodeDragEndAck",
        "data": { "nodeId": node_id },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&ack) {
        ctx.text(msg_str);
    }
}

/// Handle `nodeUnpin` from client — explicitly release a node that was pinned in
/// place (e.g. by a prior grab-and-place). Clears the GPU pin so the node resumes
/// free integration and the surrounding springs settle it into a new resting
/// place. This is the counterpart to the persistent-pin `nodeDragEnd` semantics.
///
/// Expected message shape:
/// ```json
/// { "type": "nodeUnpin", "data": { "nodeId": 42 } }
/// ```
pub(crate) fn handle_node_unpin(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // VULN-01: Reject unauthenticated clients
    if act.pubkey.is_none() {
        warn!("[Unpin] Rejecting unpin from unauthenticated client");
        return;
    }

    let data = match msg.get("data") {
        Some(d) => d,
        None => {
            warn!("[Unpin] nodeUnpin missing 'data' field");
            return;
        }
    };

    // VULN-03: Validate nodeId fits in u32 (prevent silent truncation)
    let node_id = match data.get("nodeId").and_then(|v| v.as_u64()) {
        Some(id) if id <= u32::MAX as u64 => id as u32,
        _ => {
            warn!("[Unpin] Invalid or missing nodeId");
            return;
        }
    };

    info!("[Unpin] nodeUnpin: node_id={}", node_id);

    // Clear any residual drag tracking (a client may unpin without a drag cycle).
    act.dragged_nodes.remove(&node_id);
    act.drag_last_update.remove(&node_id);

    let app_state = act.app_state.clone();
    let fut = async move {
        use crate::actors::messages::PinNodePositions;
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            gpu_addr.do_send(PinNodePositions {
                pins: Vec::new(),
                unpin: vec![node_id],
                // Mild reheat so the freed node visibly settles from its anchor.
                reheat: true,
            });
        }
    };
    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));

    let ack = serde_json::json!({
        "type": "nodeUnpinAck",
        "data": { "nodeId": node_id },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&ack) {
        ctx.text(msg_str);
    }
}

/// Periodic timeout checker: if no drag update has been received for a node
/// within `drag_timeout_ms`, automatically unpin it (safety net for dropped
/// connections or missed dragEnd messages).
fn check_drag_timeout(
    act: &mut SocketFlowServer,
    node_id: u32,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // If the node is no longer being dragged, nothing to do
    if !act.dragged_nodes.contains(&node_id) {
        return;
    }

    let timeout = std::time::Duration::from_millis(act.drag_timeout_ms);
    let timed_out = act
        .drag_last_update
        .get(&node_id)
        .map(|last| last.elapsed() > timeout)
        .unwrap_or(true);

    if timed_out {
        info!(
            "[Drag] Timeout: auto-unpin node {} (no update for >{}ms)",
            node_id, act.drag_timeout_ms
        );

        // A timeout means the drag stalled or the connection dropped — this is a
        // FAILURE path, not a deliberate drop, so the node must be RELEASED, not
        // left permanently pinned. We must NOT route through handle_node_drag_end
        // (which now pins-in-place): that would strand a stale anchor forever.
        // Clear drag tracking and send an explicit unpin so the node integrates
        // freely again.
        act.dragged_nodes.remove(&node_id);
        act.drag_last_update.remove(&node_id);
        let app_state = act.app_state.clone();
        let fut = async move {
            use crate::actors::messages::PinNodePositions;
            if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
                gpu_addr.do_send(PinNodePositions {
                    pins: Vec::new(),
                    unpin: vec![node_id],
                    reheat: false,
                });
            }
        };
        ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));
    } else {
        // Re-schedule check
        let timeout_ms = act.drag_timeout_ms;
        ctx.run_later(
            std::time::Duration::from_millis(timeout_ms + 100),
            move |act, ctx| {
                check_drag_timeout(act, node_id, ctx);
            },
        );
    }
}

#[cfg(test)]
mod visibility_flag_tests {
    use super::parse_visibility_flag;

    /// Exhaustively exercise the secure-by-default truth table. Pure: no process
    /// env is touched, so this is race-free and can run in parallel with anything.
    #[test]
    fn parse_visibility_flag_defaults_on_and_honours_opt_out() {
        // Absent value ⇒ filter ON (secure by default).
        assert!(
            parse_visibility_flag(None),
            "absent value must default the filter ON"
        );

        // Explicit truthy / unrecognised tokens keep it ON (fail-safe).
        for on in ["1", "true", "on", "yes", "TRUE", "  1  ", "banana", ""] {
            assert!(
                parse_visibility_flag(Some(on)),
                "{on:?} must leave the filter ON"
            );
        }

        // Explicit falsy tokens are the only way to opt out.
        for off in ["0", "false", "off", "no", "FALSE", "  off  ", "No"] {
            assert!(
                !parse_visibility_flag(Some(off)),
                "{off:?} must turn the filter OFF"
            );
        }
    }
}
