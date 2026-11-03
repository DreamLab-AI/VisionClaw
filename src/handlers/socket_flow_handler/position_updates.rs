use actix::prelude::*;
use log::{debug, info, warn};
use std::time::Instant;

use super::types::SocketFlowServer;

/// Whether the ADR-060 private-node drop-set filter is enabled (default ON, fail closed). One
/// definition for every path that applies it, including the position stream:
/// `visionclaw_domain::utils::visibility_filter::filter_enabled`.
pub(crate) fn pubkey_visibility_filter_enabled() -> bool {
    visionclaw_domain::utils::visibility_filter::filter_enabled()
}
/// Maximum number of nodes a single connection may hold at once: one per hand.
const MAX_DRAGGED_NODES_PER_CLIENT: usize = 2;

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

// ---------------------------------------------------------------------------
// Server-side drag handling
// ---------------------------------------------------------------------------
//
// A drag is a HOLD, not a pin. The connection takes hold of a node (`HoldNodes`), the GPU keeps
// it at the hand and the rest of the graph springs around it, and when the hand lets go
// (`nodeDragEnd`, a timeout or a disconnect) the node REJOINS the simulation. A lasting anchor is
// a separate, explicit act: `nodePin` / `nodeUnpin`. Two clients, or two hands, may hold at once.

/// True when this connection may start holding `node`: it already holds it, or has a free hand.
pub(crate) fn may_hold(dragged: &std::collections::HashSet<u32>, node: u32) -> bool {
    dragged.contains(&node) || dragged.len() < MAX_DRAGGED_NODES_PER_CLIENT
}

/// Read `data.nodeId` (must fit a u32; VULN-03).
fn parse_node_id(data: &serde_json::Value) -> Option<u32> {
    match data.get("nodeId").and_then(|v| v.as_u64()) {
        Some(id) if id <= u32::MAX as u64 => Some(id as u32),
        _ => {
            warn!("[Drag] Invalid or missing nodeId");
            None
        }
    }
}

/// Read `data.position`, rejecting NaN, infinity and out-of-range values (VULN-05).
fn parse_position(data: &serde_json::Value, what: &str) -> Option<[f32; 3]> {
    let axis = |k: &str| {
        data.get("position")
            .and_then(|p| p.get(k))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32
    };
    let (x, y, z) = (axis("x"), axis("y"), axis("z"));
    match sanitize_position(x, y, z) {
        Some((x, y, z)) => Some([x, y, z]),
        None => {
            warn!("[Drag] {what}: rejecting invalid position [{x}, {y}, {z}]");
            None
        }
    }
}

/// Tell the physics actor this connection holds `holds`, and wake the orchestrator for the nodes
/// that are newly held (`wake`).
fn send_holds(
    app_state: &std::sync::Arc<crate::app_state::AppState>,
    holder: u64,
    holds: Vec<(u32, [f32; 3])>,
    wake: bool,
) -> impl std::future::Future<Output = ()> {
    let app_state = app_state.clone();
    async move {
        use crate::actors::messages::{HoldNodes, NodeInteractionMessage, NodeInteractionType};
        if wake {
            for (node_id, pos) in &holds {
                app_state
                    .graph_service_addr
                    .do_send(NodeInteractionMessage {
                        node_id: *node_id,
                        holder,
                        interaction_type: NodeInteractionType::Dragged,
                        position: Some(*pos),
                    });
            }
        }
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            gpu_addr.do_send(HoldNodes { holder, holds });
        }
    }
}

/// Release `nodes` held by `holder`: the physics actor frees the ones this holder still owns and
/// opens the settle window, and the orchestrator is told so it stops treating the graph as held.
/// `disconnected` releases everything the holder owns, including nodes missing from `nodes`.
pub(crate) fn release_holds(
    app_state: &std::sync::Arc<crate::app_state::AppState>,
    holder: u64,
    nodes: Vec<u32>,
    disconnected: bool,
) {
    let app_state = app_state.clone();
    actix::spawn(async move {
        use crate::actors::messages::{
            NodeInteractionMessage, NodeInteractionType, ReleaseHolder, ReleaseNodes,
        };
        for node_id in &nodes {
            app_state
                .graph_service_addr
                .do_send(NodeInteractionMessage {
                    node_id: *node_id,
                    holder,
                    interaction_type: NodeInteractionType::Released,
                    position: None,
                });
        }
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            if disconnected {
                gpu_addr.do_send(ReleaseHolder { holder });
            } else {
                gpu_addr.do_send(ReleaseNodes { holder, nodes });
            }
        }
    });
}

/// Handle `nodeDragStart` from client.
///
/// The connection takes hold of the node at the reported position. The GPU keeps it there and
/// the neighbours spring around it; every connected client sees the result in the position
/// stream. Nothing here pins the node past the drag.
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
    let Some(data) = msg.get("data") else {
        warn!("[Drag] nodeDragStart missing 'data' field");
        return;
    };
    let Some(node_id) = parse_node_id(data) else {
        return;
    };
    let Some(pos) = parse_position(data, "nodeDragStart") else {
        return;
    };

    // VULN-10: one node per hand
    if !may_hold(&act.dragged_nodes, node_id) {
        warn!(
            "[Drag] Client exceeded max simultaneous drags ({})",
            MAX_DRAGGED_NODES_PER_CLIENT
        );
        return;
    }

    info!(
        "[Drag] nodeDragStart: node_id={}, holder={}, pos=[{:.2}, {:.2}, {:.2}]",
        node_id, act.hold_id, pos[0], pos[1], pos[2]
    );

    act.dragged_nodes.insert(node_id);
    act.drag_last_update.insert(node_id, Instant::now());
    act.drag_pending.remove(&node_id);

    let fut = send_holds(&act.app_state, act.hold_id, vec![(node_id, pos)], true);
    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _, _| ()));

    let ack = serde_json::json!({
        "type": "nodeDragStartAck",
        "data": { "nodeId": node_id },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&ack) {
        ctx.text(msg_str);
    }

    let timeout_ms = act.drag_timeout_ms;
    ctx.run_later(
        std::time::Duration::from_millis(timeout_ms + 100),
        move |act, ctx| {
            check_drag_timeout(act, node_id, ctx);
        },
    );
}

/// Handle `nodeDragUpdate` from client during an active hold.
///
/// Moves the held node on the GPU. An update that arrives inside the rate-limit interval is not
/// discarded: the newest one is kept and applied by a trailing timer (or at `nodeDragEnd`), so the
/// hand's final position always lands.
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
    let Some(data) = msg.get("data") else {
        return;
    };
    let Some(node_id) = parse_node_id(data) else {
        return;
    };
    let Some(pos) = parse_position(data, "nodeDragUpdate") else {
        return;
    };

    // An update for a node we have no start for is an implicit start (a lost nodeDragStart).
    let implicit_start = !act.dragged_nodes.contains(&node_id);
    if implicit_start {
        debug!(
            "[Drag] dragUpdate for non-held node {}, treating as implicit drag start",
            node_id
        );
        if !may_hold(&act.dragged_nodes, node_id) {
            warn!(
                "[Drag] Client exceeded max simultaneous drags ({})",
                MAX_DRAGGED_NODES_PER_CLIENT
            );
            return;
        }
        act.dragged_nodes.insert(node_id);
        let timeout_ms = act.drag_timeout_ms;
        ctx.run_later(
            std::time::Duration::from_millis(timeout_ms + 100),
            move |act, ctx| {
                check_drag_timeout(act, node_id, ctx);
            },
        );
    }

    // VULN-02: server-side rate limit (~60 Hz per node), by coalescing rather than dropping.
    if !implicit_start {
        if let Some(last) = act.drag_last_update.get(&node_id) {
            if last.elapsed() < std::time::Duration::from_millis(MIN_DRAG_INTERVAL_MS) {
                let first_pending = act.drag_pending.insert(node_id, pos).is_none();
                if first_pending {
                    ctx.run_later(
                        std::time::Duration::from_millis(MIN_DRAG_INTERVAL_MS),
                        move |act, ctx| flush_pending_drag(act, node_id, ctx),
                    );
                }
                return;
            }
        }
    }

    act.drag_pending.remove(&node_id);
    act.drag_last_update.insert(node_id, Instant::now());

    // A drag update does exactly one thing: move the node's hold on the GPU. What every other
    // client sees is the GPU's own entitled snapshot stream. This handler used to ALSO run CPU
    // `SimulationStep`s and broadcast the whole graph as bare V3 to every session: a second,
    // un-entitled physics fighting the real one.
    let fut = send_holds(
        &act.app_state,
        act.hold_id,
        vec![(node_id, pos)],
        implicit_start,
    );
    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));
}

/// Apply the newest rate-limited position for `node_id`, if one is still waiting.
fn flush_pending_drag(
    act: &mut SocketFlowServer,
    node_id: u32,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    if !act.dragged_nodes.contains(&node_id) {
        act.drag_pending.remove(&node_id);
        return;
    }
    if let Some(pos) = act.drag_pending.remove(&node_id) {
        act.drag_last_update.insert(node_id, Instant::now());
        let fut = send_holds(&act.app_state, act.hold_id, vec![(node_id, pos)], false);
        ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));
    }
}

/// Handle `nodeDragEnd` from client.
///
/// Lets go of the node. Unless it also carries an explicit `nodePin`, it REJOINS the simulation
/// and the graph settles around it within about two seconds; every client sees that settle in the
/// position stream. The newest rate-limited position is applied first so the node is released
/// where the hand was.
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
    let Some(data) = msg.get("data") else {
        warn!("[Drag] nodeDragEnd missing 'data' field");
        return;
    };
    let Some(node_id) = parse_node_id(data) else {
        return;
    };

    info!(
        "[Drag] nodeDragEnd: node_id={}, holder={}",
        node_id, act.hold_id
    );

    let last = act.drag_pending.remove(&node_id);
    act.dragged_nodes.remove(&node_id);
    act.drag_last_update.remove(&node_id);

    // Land the hand's last position, then release. Both go to the same actor in order.
    let holder = act.hold_id;
    let app_state = act.app_state.clone();
    let fut = async move {
        if let Some(pos) = last {
            send_holds(&app_state, holder, vec![(node_id, pos)], false).await;
        }
        release_holds(&app_state, holder, vec![node_id], false);
    };
    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));

    let ack = serde_json::json!({
        "type": "nodeDragEndAck",
        "data": { "nodeId": node_id },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&ack) {
        ctx.text(msg_str);
    }
}

/// Handle `nodePin`: anchor a node in place until an explicit `nodeUnpin`. This is the lasting
/// pin; a drag no longer leaves one behind. A pin made before or during a drag survives the
/// drag's release.
///
/// Expected message shape:
/// ```json
/// { "type": "nodePin", "data": { "nodeId": 42, "position": { "x": 1.0, "y": 2.0, "z": 3.0 } } }
/// ```
pub(crate) fn handle_node_pin(
    act: &mut SocketFlowServer,
    msg: &serde_json::Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    // VULN-01: Reject unauthenticated clients
    if act.pubkey.is_none() {
        warn!("[Pin] Rejecting pin from unauthenticated client");
        return;
    }
    let Some(data) = msg.get("data") else {
        warn!("[Pin] nodePin missing 'data' field");
        return;
    };
    let Some(node_id) = parse_node_id(data) else {
        return;
    };
    let Some(pos) = parse_position(data, "nodePin") else {
        return;
    };
    info!("[Pin] nodePin: node_id={}", node_id);

    let app_state = act.app_state.clone();
    let fut = async move {
        use crate::actors::messages::PinNodePositions;
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            gpu_addr.do_send(PinNodePositions {
                pins: vec![(node_id, pos)],
                unpin: Vec::new(),
                reheat: false,
            });
        }
    };
    ctx.spawn(actix::fut::wrap_future::<_, SocketFlowServer>(fut).map(|_, _act, _ctx| {}));

    let ack = serde_json::json!({
        "type": "nodePinAck",
        "data": { "nodeId": node_id },
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&ack) {
        ctx.text(msg_str);
    }
}

/// Handle `nodeUnpin` from client: clear the explicit pin on a node. A node still held by a hand
/// stays held until the hand lets go; otherwise it rejoins the simulation and settles.
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
    let Some(data) = msg.get("data") else {
        warn!("[Unpin] nodeUnpin missing 'data' field");
        return;
    };
    let Some(node_id) = parse_node_id(data) else {
        return;
    };

    info!("[Unpin] nodeUnpin: node_id={}", node_id);

    let app_state = act.app_state.clone();
    let fut = async move {
        use crate::actors::messages::PinNodePositions;
        if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
            gpu_addr.do_send(PinNodePositions {
                pins: Vec::new(),
                unpin: vec![node_id],
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

/// Periodic timeout checker: if no drag update has been received for a node within
/// `drag_timeout_ms`, release it (safety net for a client that vanished without `nodeDragEnd`).
/// The node rejoins the simulation; an explicit pin on it, if any, is untouched.
fn check_drag_timeout(
    act: &mut SocketFlowServer,
    node_id: u32,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
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
            "[Drag] Timeout: releasing node {} (no update for >{}ms)",
            node_id, act.drag_timeout_ms
        );
        act.dragged_nodes.remove(&node_id);
        act.drag_last_update.remove(&node_id);
        act.drag_pending.remove(&node_id);
        release_holds(&act.app_state, act.hold_id, vec![node_id], false);
    } else {
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
mod drag_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn a_connection_may_hold_two_nodes_and_not_a_third() {
        let mut held = HashSet::new();
        assert!(may_hold(&held, 1));
        held.insert(1);
        assert!(may_hold(&held, 2), "the second hand");
        held.insert(2);
        assert!(!may_hold(&held, 3), "no third hand");
        assert!(
            may_hold(&held, 2),
            "moving a node already held is always allowed"
        );
    }

    #[test]
    fn positions_outside_the_world_or_non_finite_are_rejected() {
        let ok = serde_json::json!({"position": {"x": 1.0, "y": 2.0, "z": 3.0}});
        assert_eq!(parse_position(&ok, "t"), Some([1.0, 2.0, 3.0]));
        let far = serde_json::json!({"position": {"x": 1.0e9, "y": 0.0, "z": 0.0}});
        assert_eq!(parse_position(&far, "t"), None);
    }

    #[test]
    fn a_node_id_larger_than_u32_is_rejected() {
        let big = serde_json::json!({"nodeId": u64::from(u32::MAX) + 1});
        assert_eq!(parse_node_id(&big), None);
        assert_eq!(parse_node_id(&serde_json::json!({"nodeId": 42})), Some(42));
    }
}
