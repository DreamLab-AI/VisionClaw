use actix::prelude::*;
use log::{debug, info, warn};
use std::time::Instant;

use crate::utils::binary_protocol;
use crate::utils::socket_flow_messages::BinaryNodeData;

use super::types::SocketFlowServer;

/// Whether the ADR-060 private-node drop-set filter is enabled (default ON, fail closed). One
/// definition for every path that applies it, including the position stream:
/// `visionclaw_domain::utils::visibility_filter::filter_enabled`.
pub(crate) fn pubkey_visibility_filter_enabled() -> bool {
    visionclaw_domain::utils::visibility_filter::filter_enabled()
}
/// Maximum time budget (ms) for settle iterations per drag update.

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

pub(crate) fn handle_request_swarm_telemetry(
    act: &mut SocketFlowServer,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    debug!("Client requested enhanced swarm telemetry");

    let app_state = act.app_state.clone();

    ctx.spawn(
        actix::fut::wrap_future::<_, SocketFlowServer>(async move {
            match crate::handlers::bots_handler::fetch_hive_mind_agents(&app_state).await {
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

        // 2. Grab→spring: pin the node ON THE GPU so the force kernel holds it at
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
    // The GPU is the only physics. A drag update does exactly one thing: move the node's GPU pin
    // (grab -> spring: the force kernel holds it at the hand position and its neighbours spring
    // around it). What other users see is the GPU's own full, entitled snapshot stream, broadcast
    // at the interaction rate while pins are being updated. This handler used to ALSO run CPU
    // `SimulationStep`s on the graph-state copy and broadcast the whole of it as bare V3 to every
    // session: a second, un-entitled physics fighting the real one, leaking private nodes, and
    // flooding ~190 x 490 KB frames in 4 s.
    let fut = async move {
        use crate::actors::messages::PinNodePositions;
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            gpu_addr.do_send(PinNodePositions {
                pins: vec![(node_id, [pos_x, pos_y, pos_z])],
                unpin: Vec::new(),
                reheat: false,
            });
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

    let fut = async move {
        // Notify physics that the drag interaction ended (node released). The node STAYS pinned
        // on the GPU at the drop position until an explicit `nodeUnpin` (persistent grab-and-place
        // semantics); the GPU keeps relaxing the neighbours and streams the result like any other
        // frame. No CPU settle, no side-channel broadcast.
        use crate::actors::messages::{NodeInteractionMessage, NodeInteractionType};
        app_state
            .graph_service_addr
            .do_send(NodeInteractionMessage {
                node_id,
                interaction_type: NodeInteractionType::Released,
                position: None,
            });
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
