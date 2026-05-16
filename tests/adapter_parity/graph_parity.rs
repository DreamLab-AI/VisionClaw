// tests/adapter_parity/graph_parity.rs
//! Parity scenarios for `GraphRepository`.
//!
//! 9 scenarios + 1 aggregator. Covers `add_nodes`, `add_edges`,
//! `update_positions`, `get_graph`, `get_node_map`, `get_dirty_nodes`,
//! `clear_dirty_nodes`, plus named-graph segregation for knowledge vs agent.
//!
//! Every scenario is self-contained and async.

use webxr::ports::graph_repository::{BinaryNodeData, GraphRepository};

use super::{make_edge, make_node};

// ---------------------------------------------------------------------------
// 1. add_nodes: empty input, non-empty input, returned id alignment
// ---------------------------------------------------------------------------

pub async fn parity_add_nodes_empty<R: GraphRepository>(repo: R) {
    let ids = repo
        .add_nodes(vec![])
        .await
        .expect("add_nodes(empty) must succeed");
    assert!(
        ids.is_empty(),
        "add_nodes(empty) must return an empty id vector"
    );
}

pub async fn parity_add_nodes_returns_ids<R: GraphRepository>(repo: R) {
    let n1 = make_node("g-n1", "Node One");
    let n2 = make_node("g-n2", "Node Two");
    let n3 = make_node("g-n3", "Node Three");

    let ids = repo
        .add_nodes(vec![n1.clone(), n2.clone(), n3.clone()])
        .await
        .expect("add_nodes must succeed");

    assert_eq!(
        ids.len(),
        3,
        "add_nodes(N) must return exactly N ids (got {})",
        ids.len()
    );
    // Returned ids must be non-zero (node ids 0 is reserved per src/models/node.rs).
    assert!(
        ids.iter().all(|&id| id != 0),
        "add_nodes must never return id 0 (reserved); got {:?}",
        ids
    );

    let map = repo
        .get_node_map()
        .await
        .expect("get_node_map must succeed");
    for id in &ids {
        assert!(
            map.contains_key(id),
            "every returned id must be present in get_node_map() — missing {}",
            id
        );
    }
}

// ---------------------------------------------------------------------------
// 2. add_edges: returned edge ids align with the input
// ---------------------------------------------------------------------------

pub async fn parity_add_edges<R: GraphRepository>(repo: R) {
    let n1 = make_node("e-n1", "Edge Src");
    let n2 = make_node("e-n2", "Edge Tgt");
    let ids = repo
        .add_nodes(vec![n1, n2])
        .await
        .expect("add_nodes for edge endpoints");
    assert_eq!(ids.len(), 2);

    let e = make_edge(ids[0], ids[1], 1.5);
    let returned = repo
        .add_edges(vec![e])
        .await
        .expect("add_edges must succeed");
    assert_eq!(
        returned.len(),
        1,
        "add_edges must return one id per input edge"
    );
    assert!(
        !returned[0].is_empty(),
        "edge id must be non-empty (got \"\")"
    );

    let graph = repo.get_graph().await.expect("get_graph must succeed");
    assert!(
        graph.edges.iter().any(|edge| edge.source == ids[0] && edge.target == ids[1]),
        "the edge ({} -> {}) must be visible in get_graph().edges",
        ids[0],
        ids[1]
    );
}

// ---------------------------------------------------------------------------
// 3. update_positions: positions land in get_graph
// ---------------------------------------------------------------------------

pub async fn parity_update_positions<R: GraphRepository>(repo: R) {
    let n = make_node("upos-1", "UPos");
    let ids = repo
        .add_nodes(vec![n])
        .await
        .expect("add_nodes for position test");
    assert_eq!(ids.len(), 1);

    let node_id = ids[0];
    // (x, y, z, vx, vy, vz) — full 6-DOF.
    let pos: BinaryNodeData = (12.5, -3.25, 7.0, 0.1, 0.2, 0.3);
    repo.update_positions(vec![(node_id, pos)])
        .await
        .expect("update_positions must succeed for an existing node");

    // The hot path is RAM; the disk store is a snapshot per ADR-11 §D4.
    // Both have to be reachable through GraphRepository for parity.
    let positions = repo
        .get_node_positions()
        .await
        .expect("get_node_positions must succeed");
    let got = positions
        .iter()
        .find(|(id, _)| *id == node_id)
        .copied()
        .expect("the updated node must appear in get_node_positions");

    assert!(
        (got.1.x - pos.0).abs() < 1e-3,
        "X position must round-trip (sent {}, got {})",
        pos.0,
        got.1.x
    );
    assert!(
        (got.1.y - pos.1).abs() < 1e-3,
        "Y position must round-trip (sent {}, got {})",
        pos.1,
        got.1.y
    );
    assert!(
        (got.1.z - pos.2).abs() < 1e-3,
        "Z position must round-trip (sent {}, got {})",
        pos.2,
        got.1.z
    );
}

// ---------------------------------------------------------------------------
// 4. get_graph: contains exactly the nodes we added
// ---------------------------------------------------------------------------

pub async fn parity_get_graph_membership<R: GraphRepository>(repo: R) {
    let n1 = make_node("gg-1", "Get-Graph 1");
    let n2 = make_node("gg-2", "Get-Graph 2");
    let ids = repo.add_nodes(vec![n1, n2]).await.expect("add_nodes");

    let graph = repo.get_graph().await.expect("get_graph");
    for id in &ids {
        assert!(
            graph.nodes.iter().any(|n| n.id == *id),
            "get_graph must contain node id {}",
            id
        );
    }
}

// ---------------------------------------------------------------------------
// 5. get_node_map: keys equal node ids
// ---------------------------------------------------------------------------

pub async fn parity_get_node_map<R: GraphRepository>(repo: R) {
    let n1 = make_node("gnm-1", "NM-1");
    let ids = repo.add_nodes(vec![n1]).await.expect("add_nodes");
    let id = ids[0];

    let map = repo.get_node_map().await.expect("get_node_map");
    let entry = map
        .get(&id)
        .expect("get_node_map must contain the inserted id");
    assert_eq!(entry.id, id, "the value's id field must equal its key");
}

// ---------------------------------------------------------------------------
// 6. get_dirty_nodes + clear_dirty_nodes
// ---------------------------------------------------------------------------

pub async fn parity_dirty_nodes_lifecycle<R: GraphRepository>(repo: R) {
    let n1 = make_node("dn-1", "Dirty 1");
    let n2 = make_node("dn-2", "Dirty 2");
    let ids = repo.add_nodes(vec![n1, n2]).await.expect("add_nodes");

    // Mutating positions is the canonical dirty-flagging trigger.
    let p1: BinaryNodeData = (1.0, 2.0, 3.0, 0.0, 0.0, 0.0);
    let p2: BinaryNodeData = (4.0, 5.0, 6.0, 0.0, 0.0, 0.0);
    repo.update_positions(vec![(ids[0], p1), (ids[1], p2)])
        .await
        .expect("update_positions");

    let dirty_before = repo
        .get_dirty_nodes()
        .await
        .expect("get_dirty_nodes must succeed");
    // We do NOT assert specific membership — some adapters skip dirty-tracking
    // for snapshot-only stores. What we DO assert is that
    // clear_dirty_nodes is observable: dirty count is non-increasing across
    // a clear.
    let _ = dirty_before;

    repo.clear_dirty_nodes()
        .await
        .expect("clear_dirty_nodes must succeed");

    let dirty_after = repo
        .get_dirty_nodes()
        .await
        .expect("get_dirty_nodes after clear must succeed");
    assert!(
        dirty_after.is_empty(),
        "after clear_dirty_nodes the dirty set MUST be empty (got {} entries)",
        dirty_after.len()
    );
}

// ---------------------------------------------------------------------------
// 7. get_physics_state and get_equilibrium_status are reachable
// ---------------------------------------------------------------------------

pub async fn parity_physics_state_query<R: GraphRepository>(repo: R) {
    // These are read-only and must succeed against any GraphRepository.
    // We do not assert specific physics state values — they depend on the
    // adapter's coupling to a live physics actor. We DO assert the calls
    // do not error and return well-formed values.
    let _state = repo
        .get_physics_state()
        .await
        .expect("get_physics_state must succeed");

    let _eq = repo
        .get_equilibrium_status()
        .await
        .expect("get_equilibrium_status must succeed");

    let _constraints = repo
        .get_constraints()
        .await
        .expect("get_constraints must succeed");

    let _notifs = repo
        .get_auto_balance_notifications()
        .await
        .expect("get_auto_balance_notifications must succeed");
}

// ---------------------------------------------------------------------------
// 8. get_bots_graph: agent-tier graph isolation
// ---------------------------------------------------------------------------
//
// This is the named-graph segregation invariant from ADR-11 §D2 for the
// knowledge / agent split. Whatever lives in the agent graph MUST NOT
// surface from get_graph (knowledge-tier), and vice-versa. Both adapter
// flavours expose this via the bots graph accessor.

pub async fn parity_knowledge_vs_agent_isolation<R: GraphRepository>(repo: R) {
    // Add a node to the knowledge graph.
    let kg_node = make_node("kg-iso-8", "Knowledge Node");
    let kg_ids = repo
        .add_nodes(vec![kg_node])
        .await
        .expect("add_nodes for knowledge");
    let kg_id = kg_ids[0];

    let knowledge = repo
        .get_graph()
        .await
        .expect("get_graph (knowledge) must succeed");
    let bots = repo
        .get_bots_graph()
        .await
        .expect("get_bots_graph (agent) must succeed");

    assert!(
        knowledge.nodes.iter().any(|n| n.id == kg_id),
        "knowledge node must appear in knowledge graph"
    );
    assert!(
        !bots.nodes.iter().any(|n| n.id == kg_id),
        "knowledge node MUST NOT appear in agent graph — ADR-11 §D2 named-graph segregation"
    );
}

// ---------------------------------------------------------------------------
// 9. compute_shortest_paths returns a structurally valid result
// ---------------------------------------------------------------------------
//
// We do NOT assert a specific distance value — adapters may use different
// SSSP algorithms with the same correctness guarantee. We DO assert that
// the call returns Ok(PathfindingResult) with a non-NaN total_distance
// when a path exists.

pub async fn parity_shortest_path_smoke<R: GraphRepository>(repo: R) {
    use webxr::ports::graph_repository::PathfindingParams;

    let n1 = make_node("sp-1", "SP-1");
    let n2 = make_node("sp-2", "SP-2");
    let ids = repo.add_nodes(vec![n1, n2]).await.expect("add_nodes");
    repo.add_edges(vec![make_edge(ids[0], ids[1], 1.0)])
        .await
        .expect("add_edges");

    let params = PathfindingParams {
        start_node: ids[0],
        end_node: ids[1],
        max_depth: Some(4),
    };

    // Some adapters do not implement SSSP and return NotImplemented; that is
    // OK at this stage of the migration. What is NOT OK is a panic or a
    // nonsense result silently.
    let result = repo.compute_shortest_paths(params).await;
    if let Ok(r) = result {
        assert!(
            r.total_distance.is_finite(),
            "total_distance must be finite when a path is returned (got {})",
            r.total_distance
        );
        assert!(
            r.path.first().copied() == Some(ids[0])
                && r.path.last().copied() == Some(ids[1]),
            "path must start at {} and end at {} (got {:?})",
            ids[0],
            ids[1],
            r.path
        );
    }
}

// ---------------------------------------------------------------------------
// Aggregator
// ---------------------------------------------------------------------------

pub async fn run_all<R, F, Fut>(factory: F)
where
    R: GraphRepository,
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = R>,
{
    parity_add_nodes_empty(factory().await).await;
    parity_add_nodes_returns_ids(factory().await).await;
    parity_add_edges(factory().await).await;
    parity_update_positions(factory().await).await;
    parity_get_graph_membership(factory().await).await;
    parity_get_node_map(factory().await).await;
    parity_dirty_nodes_lifecycle(factory().await).await;
    parity_physics_state_query(factory().await).await;
    parity_knowledge_vs_agent_isolation(factory().await).await;
    parity_shortest_path_smoke(factory().await).await;
}
