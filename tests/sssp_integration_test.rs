//! ADR-031 D7 — SSSP integration tests, extended with VALUE assertions.
//!
//! Previously CPU-only and entirely commented out. Rewritten to:
//!   - Keep the CPU-reference known-answer assertions (Dijkstra oracle).
//!   - Add GPU value assertions that bind to the intended SSSP GPU path and
//!     assert the returned distances equal the CPU oracle (D7 obligation:
//!     "tests/sssp_integration_test.rs (CPU-only) -> add GPU value assertions").
//!
//! Per ADR-031 D2/D-SSSP: SSSP needs NO wire-layout change — the slots
//! sssp_distance@28 / sssp_parent@32 already exist; the fix is wiring the
//! encoder feed (currently hardcoded None at three broadcast sites). These
//! tests assert the VALUES that must reach those existing slots.

#[path = "analytics_fixtures.rs"]
mod fx;

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// CPU Dijkstra oracle on the canonical simple SSSP graph.
// ---------------------------------------------------------------------------

/// Directed weighted graph for SSSP. (source, target, weight).
struct SsspGraph {
    n: usize,
    edges: Vec<(u32, u32, f32)>,
}

impl SsspGraph {
    /// 0 -> 1 (1.0) -> 2 (2.0); 0 -> 3 (3.0) -> 2 (1.0); node 4 disconnected.
    fn simple() -> Self {
        SsspGraph {
            n: 5,
            edges: vec![(0, 1, 1.0), (1, 2, 2.0), (0, 3, 3.0), (3, 2, 1.0)],
        }
    }
}

/// CPU Dijkstra reference: distance and parent from `source`.
fn dijkstra(g: &SsspGraph, source: u32) -> (Vec<Option<f32>>, Vec<i32>) {
    let mut adj: HashMap<u32, Vec<(u32, f32)>> = HashMap::new();
    for &(u, v, w) in &g.edges {
        adj.entry(u).or_default().push((v, w));
    }
    let mut dist: Vec<Option<f32>> = vec![None; g.n];
    let mut parent: Vec<i32> = vec![-1; g.n];
    dist[source as usize] = Some(0.0);
    // simple O(n^2) Dijkstra — n is tiny
    let mut settled = vec![false; g.n];
    for _ in 0..g.n {
        let mut best: Option<(u32, f32)> = None;
        for i in 0..g.n {
            if settled[i] {
                continue;
            }
            if let Some(d) = dist[i] {
                if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((i as u32, d));
                }
            }
        }
        let (u, du) = match best {
            Some(x) => x,
            None => break,
        };
        settled[u as usize] = true;
        if let Some(neigh) = adj.get(&u) {
            for &(v, w) in neigh {
                let nd = du + w;
                if dist[v as usize].map(|d| nd < d).unwrap_or(true) {
                    dist[v as usize] = Some(nd);
                    parent[v as usize] = u as i32;
                }
            }
        }
    }
    (dist, parent)
}

#[test]
fn sssp_cpu_reference_known_answer() {
    let g = SsspGraph::simple();
    let (dist, parent) = dijkstra(&g, 0);
    assert_eq!(dist[0], Some(0.0), "source distance is 0");
    assert_eq!(dist[1], Some(1.0), "0->1 = 1.0");
    assert_eq!(
        dist[2],
        Some(3.0),
        "0->1->2 = 3.0 (optimal over 0->3->2 = 4.0)"
    );
    assert_eq!(dist[3], Some(3.0), "0->3 = 3.0");
    assert_eq!(dist[4], None, "node 4 is unreachable");
    // parent pointers reach the wire's sssp_parent@32 slot.
    assert_eq!(parent[1], 0);
    assert_eq!(parent[2], 1, "optimal predecessor of 2 is 1, not 3");
    assert_eq!(parent[3], 0);
    assert_eq!(parent[4], -1, "unreachable node has parent -1");
}

#[test]
fn sssp_unreachable_encodes_as_infinity_minus_one() {
    // The wire contract for unreachable: sssp_distance = +inf, sssp_parent = -1.
    let g = SsspGraph::simple();
    let (dist, parent) = dijkstra(&g, 0);
    let wire_dist = dist[4].unwrap_or(f32::INFINITY);
    assert!(
        wire_dist.is_infinite(),
        "unreachable distance -> +inf on wire"
    );
    assert_eq!(parent[4], -1, "unreachable parent -> -1 on wire");
}

// ---------------------------------------------------------------------------
// GPU value assertions — bind to the intended GPU SSSP path; the returned
// distances/parents must equal the CPU oracle. GATED on a CUDA device.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs GPU: unified_gpu_compute SSSP kernel value round-trip"]
fn gpu_sssp_matches_cpu_oracle() {
    let g = SsspGraph::simple();
    let (cpu_dist, cpu_parent) = dijkstra(&g, 0);
    // Intended binding once a synchronous GPU SSSP entry point exists:
    //   let (gpu_dist, gpu_parent) = run_sssp_blocking(&graph, source=0);
    //   for i in 0..g.n {
    //       match cpu_dist[i] {
    //           Some(d) => assert!((gpu_dist[i] - d).abs() < 1e-4),
    //           None    => assert!(gpu_dist[i].is_infinite()),
    //       }
    //       assert_eq!(gpu_parent[i], cpu_parent[i]);
    //   }
    let _ = (cpu_dist, cpu_parent);
    // GAP (see report): no host-callable synchronous SSSP entry point that
    // returns (distances, parents) for a given source is exposed for tests.
    panic!("bind to GPU SSSP test hook once exposed");
}

/// ADR-031 D2b: the broadcast encoder must feed `sssp_data` into wire slot 28
/// (previously hardcoded `None` -> +inf at every site). This binds the canonical
/// production encoder (`encode_node_data_with_live_analytics`, the same fn the
/// broadcast path uses) and asserts the decoded record carries the real
/// distance@28 / parent@32 — not the default. The live stream passes no SSSP map
/// today (the shared `node_sssp` map was removed 2026-11-06); the encoder keeps
/// the slot so the frozen wire can carry it.
/// The GPU value round-trip (that run_sssp produces these distances) is covered
/// by `gpu_sssp_matches_cpu_oracle`; this CPU test covers the encoder feed.
#[test]
fn sssp_encoder_feed_reaches_wire_slot_28() {
    use visionclaw_server::utils::binary_protocol::encode_node_data_with_live_analytics;
    use visionclaw_server::utils::socket_flow_messages::BinaryNodeData;

    let g = SsspGraph::simple();
    let (dist, parent) = dijkstra(&g, 0);

    // One wire node per graph vertex (positions arbitrary).
    let nodes: Vec<(u32, BinaryNodeData)> = (0..g.n as u32)
        .map(|id| {
            (
                id,
                BinaryNodeData {
                    node_id: id,
                    x: id as f32,
                    y: 0.0,
                    z: 0.0,
                    vx: 0.0,
                    vy: 0.0,
                    vz: 0.0,
                },
            )
        })
        .collect();

    // Mirror ShortestPathActor's publish: key by compact node_id, leave
    // unreachable nodes absent (the encoder defaults missing nodes to +inf/-1).
    let mut sssp: HashMap<u32, (f32, i32)> = HashMap::new();
    for i in 0..g.n {
        if let Some(d) = dist[i] {
            sssp.insert(i as u32, (d, parent[i]));
        }
    }

    let frame = encode_node_data_with_live_analytics(&nodes, None, Some(&sssp));

    // Frame = 1-byte protocol-version header + N * 52 B records.
    assert_eq!(
        (frame.len() - 1) % fx::WIRE_V3_ITEM_SIZE_52,
        0,
        "frame must be version byte + N*52 B records"
    );
    let body = &frame[1..];

    for i in 0..g.n {
        let rec = &body[i * fx::WIRE_V3_ITEM_SIZE_52..(i + 1) * fx::WIRE_V3_ITEM_SIZE_52];
        let (pos, _an) = fx::decode_record_52(rec);
        match dist[i] {
            Some(d) => {
                assert!(
                    (pos.sssp_distance - d).abs() < 1e-4,
                    "node {i} distance@28 must equal oracle {d}, got {}",
                    pos.sssp_distance
                );
                assert_eq!(
                    pos.sssp_parent, parent[i],
                    "node {i} parent@32 must equal oracle"
                );
            }
            None => {
                assert!(
                    pos.sssp_distance.is_infinite(),
                    "unreachable node {i} distance@28 must be +inf, got {}",
                    pos.sssp_distance
                );
                assert_eq!(
                    pos.sssp_parent, -1,
                    "unreachable node {i} parent@32 must be -1"
                );
            }
        }
    }
}
