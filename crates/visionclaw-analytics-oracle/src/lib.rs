//! Shared analytics test fixtures + CPU reference implementations (ADR-031 D7).
//!
//! This crate is the single source of truth for the correctness-gating analytics
//! test suite. It is deliberately **CUDA-free** (pure `std`, zero dependencies)
//! so the ubuntu `CPU_CRATES` CI job can build and run the known-answer gate.
//! Previously this lived in `tests/analytics_fixtures.rs` inside the CUDA-linking
//! root `visionclaw-server` crate, which the CPU CI job excludes — so the gate
//! never ran. Hoisting it into its own crate turns the suite into a real gate.
//!
//! Consumers:
//!   - `crates/visionclaw-analytics-oracle/tests/analytics_correctness.rs` —
//!     the CPU-reference known-answer + property + NFR + invariant tests
//!     (the actual CI gate, runs under `cargo test`).
//!   - The root `visionclaw-server` crate's GPU-gated tests
//!     (`tests/analytics_correctness_test.rs`, `analytics_endpoints_test.rs`,
//!     `analytics_wire_snapshot_test.rs`, `sssp_integration_test.rs`) pull it in
//!     via the thin `tests/analytics_fixtures.rs` re-export shim for the
//!     `#[ignore]` GPU oracle harness (run with `--ignored` on the CUDA host).
//!
//! Contents:
//!   - Named graph fixtures: two-clique, triangle, star, linear chain, and a
//!     deterministic synthetic ~10,676-node graph matching the live dataset.
//!   - A pure-Rust CPU reference for each metric (Louvain modularity, PageRank,
//!     DBSCAN, LOF). These are the oracle the GPU kernels are asserted against;
//!     a single GPU implementation cannot self-detect the sigma_tot race or the
//!     per-block dangling-mass bug — a second, independent implementation can.
//!   - The canonical `NodeAnalytics` wire offsets (52 B record, centrality@48).
//!
//! NOTE ON `NodeAnalytics`: ADR-031 D2 lands a typed struct
//! `struct NodeAnalytics { cluster_id: u32, community_id: u32, anomaly: f32, centrality: f32 }`
//! into `src/app_state.rs`, replacing the 3-slot tuple. As of writing the lead
//! has not landed it; the production type is still the tuple
//! `(cluster_id, anomaly_score, community_id)` and the wire is still 48 B.
//! The fixtures below encode against the **intended** 52 B layout so the suite
//! fails CI the moment the layout drifts from the contract.

use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Canonical wire layout (ADR-031 D2). The NodeAnalytics struct is the single
// source of truth for offsets; these constants mirror it for the host tests.
// ---------------------------------------------------------------------------

/// Per-node V3 wire record size AFTER the centrality append (48 B -> 52 B).
pub const WIRE_V3_ITEM_SIZE_52: usize = 52;
/// Legacy size before centrality was appended. Tracked so the snapshot test can
/// assert the migration actually happened (52 != 48).
pub const WIRE_V3_ITEM_SIZE_48: usize = 48;

pub const OFF_ID: usize = 0; // u32
pub const OFF_POSITION: usize = 4; // 3 x f32 (12 B)
pub const OFF_VELOCITY: usize = 16; // 3 x f32 (12 B)
pub const OFF_SSSP_DISTANCE: usize = 28; // f32
pub const OFF_SSSP_PARENT: usize = 32; // i32
pub const OFF_CLUSTER_ID: usize = 36; // u32 (1-based, 0 = unclustered)
pub const OFF_ANOMALY: usize = 40; // f32 (real LOF ratio)
pub const OFF_COMMUNITY: usize = 44; // u32 (Louvain label; DISTINCT from cluster_id)
pub const OFF_CENTRALITY: usize = 48; // f32 (PageRank, normalised) — the new slot

/// Mirror of the contract struct so tests can build/round-trip records without
/// depending on the production type (which may not have landed yet).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeAnalyticsFx {
    pub cluster_id: u32,
    pub community_id: u32,
    pub anomaly: f32,
    pub centrality: f32,
}

/// Position component of a wire record (the analytics struct does not own
/// position; it is sourced from the graph node).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WirePosFx {
    pub id: u32,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub sssp_distance: f32,
    pub sssp_parent: i32,
}

/// Encode a single node to the canonical 52 B little-endian wire record.
/// This is the host-side mirror of the production encoder; the golden-snapshot
/// test asserts the production encoder produces byte-identical output.
pub fn encode_record_52(p: &WirePosFx, a: &NodeAnalyticsFx) -> Vec<u8> {
    let mut b = Vec::with_capacity(WIRE_V3_ITEM_SIZE_52);
    b.extend_from_slice(&p.id.to_le_bytes()); // @0
    for c in p.pos {
        b.extend_from_slice(&c.to_le_bytes());
    } // @4
    for c in p.vel {
        b.extend_from_slice(&c.to_le_bytes());
    } // @16
    b.extend_from_slice(&p.sssp_distance.to_le_bytes()); // @28
    b.extend_from_slice(&p.sssp_parent.to_le_bytes()); // @32
    b.extend_from_slice(&a.cluster_id.to_le_bytes()); // @36
    b.extend_from_slice(&a.anomaly.to_le_bytes()); // @40
    b.extend_from_slice(&a.community_id.to_le_bytes()); // @44
    b.extend_from_slice(&a.centrality.to_le_bytes()); // @48
    debug_assert_eq!(b.len(), WIRE_V3_ITEM_SIZE_52);
    b
}

/// Decode a 52 B record back to its components (round-trip oracle).
pub fn decode_record_52(b: &[u8]) -> (WirePosFx, NodeAnalyticsFx) {
    assert_eq!(b.len(), WIRE_V3_ITEM_SIZE_52, "record must be exactly 52 B");
    let r_u32 = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let r_i32 = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let r_f32 = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let pos = WirePosFx {
        id: r_u32(OFF_ID),
        pos: [
            r_f32(OFF_POSITION),
            r_f32(OFF_POSITION + 4),
            r_f32(OFF_POSITION + 8),
        ],
        vel: [
            r_f32(OFF_VELOCITY),
            r_f32(OFF_VELOCITY + 4),
            r_f32(OFF_VELOCITY + 8),
        ],
        sssp_distance: r_f32(OFF_SSSP_DISTANCE),
        sssp_parent: r_i32(OFF_SSSP_PARENT),
    };
    let an = NodeAnalyticsFx {
        cluster_id: r_u32(OFF_CLUSTER_ID),
        anomaly: r_f32(OFF_ANOMALY),
        community_id: r_u32(OFF_COMMUNITY),
        centrality: r_f32(OFF_CENTRALITY),
    };
    (pos, an)
}

// ---------------------------------------------------------------------------
// Graph fixture type. Undirected simple graph; edges stored once (u < v) plus
// an adjacency view for the references.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GraphFixture {
    pub name: &'static str,
    pub n: usize,
    /// Undirected edges, each stored once as (min, max).
    pub edges: Vec<(u32, u32)>,
}

impl GraphFixture {
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Undirected adjacency: node -> set of neighbours.
    pub fn adjacency(&self) -> HashMap<u32, HashSet<u32>> {
        let mut adj: HashMap<u32, HashSet<u32>> = HashMap::new();
        for i in 0..self.n as u32 {
            adj.entry(i).or_default();
        }
        for &(u, v) in &self.edges {
            adj.entry(u).or_default().insert(v);
            adj.entry(v).or_default().insert(u);
        }
        adj
    }

    pub fn degrees(&self) -> Vec<u32> {
        let mut d = vec![0u32; self.n];
        for &(u, v) in &self.edges {
            d[u as usize] += 1;
            d[v as usize] += 1;
        }
        d
    }
}

// ---- Named known-answer fixtures ------------------------------------------

/// Complete graph K_n on the contiguous range [base, base+k).
fn clique(base: u32, k: u32) -> Vec<(u32, u32)> {
    let mut e = Vec::new();
    for a in 0..k {
        for b in (a + 1)..k {
            e.push((base + a, base + b));
        }
    }
    e
}

/// Two K_n cliques joined by a single bridge edge. With two K_5 joined by one
/// edge, the natural 2-community partition yields modularity > 0.3.
/// Returns a 10-node graph (two K_5).
pub fn two_clique() -> GraphFixture {
    let k = 5u32;
    let mut edges = clique(0, k);
    edges.extend(clique(k, k));
    // bridge: last node of clique A to first node of clique B
    edges.push((k - 1, k));
    GraphFixture {
        name: "two_clique",
        n: (2 * k) as usize,
        edges,
    }
}

/// Triangle K_3.
pub fn triangle() -> GraphFixture {
    GraphFixture {
        name: "triangle",
        n: 3,
        edges: vec![(0, 1), (1, 2), (0, 2)],
    }
}

/// Star graph: node 0 is the hub, leaves 1..=n-1. PageRank centralises on 0.
pub fn star(leaves: usize) -> GraphFixture {
    let edges = (1..=leaves as u32).map(|i| (0u32, i)).collect();
    GraphFixture {
        name: "star",
        n: leaves + 1,
        edges,
    }
}

/// Linear chain 0-1-2-...-(n-1).
pub fn linear_chain(n: usize) -> GraphFixture {
    let edges = (0..n as u32 - 1).map(|i| (i, i + 1)).collect();
    GraphFixture {
        name: "linear_chain",
        n,
        edges,
    }
}

/// Canonical ~10,676-node fixture matching the live dataset scale.
///
/// No real fixture file ships in `tests/` (confirmed: only ontology fixtures
/// exist), so this is a **deterministic synthetic graph** generated from a
/// fixed seed. It is built as 16 dense communities (planted partition) linked
/// by sparse inter-community edges, so it has genuine community structure for
/// Louvain to find while staying O(n) in memory (NFR-7: no O(n^2) allocation).
///
/// Deterministic: identical bytes on every run, no RNG crate, no I/O.
pub const CANONICAL_N: usize = 10_676;
pub const CANONICAL_COMMUNITIES: usize = 16;

pub fn canonical_live_scale() -> GraphFixture {
    let n = CANONICAL_N;
    let comms = CANONICAL_COMMUNITIES;
    let per = n / comms; // ~667 nodes per community
    let mut edges: Vec<(u32, u32)> = Vec::with_capacity(n * 6);
    let mut seen: HashSet<u64> = HashSet::new();
    let push = |edges: &mut Vec<(u32, u32)>, seen: &mut HashSet<u64>, a: u32, b: u32| {
        if a == b {
            return;
        }
        let (u, v) = if a < b { (a, b) } else { (b, a) };
        let key = ((u as u64) << 32) | v as u64;
        if seen.insert(key) {
            edges.push((u, v));
        }
    };
    // Splitmix64-style deterministic hash for reproducible edge selection.
    let h = |mut x: u64| -> u64 {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    // Intra-community: each node connects to ~5 others in its block.
    for i in 0..n as u32 {
        let comm = (i as usize / per).min(comms - 1);
        let lo = (comm * per) as u32;
        let hi = (((comm + 1) * per).min(n)) as u32;
        let span = (hi - lo).max(1);
        for k in 0..5u32 {
            let off = (h(i as u64 * 131 + k as u64) % span as u64) as u32;
            push(&mut edges, &mut seen, i, lo + off);
        }
    }
    // Inter-community: sparse bridges (~1 per 8 nodes) to keep modularity high
    // but the graph connected.
    for i in (0..n as u32).step_by(8) {
        let target = (h(i as u64 * 977 + 7) % n as u64) as u32;
        push(&mut edges, &mut seen, i, target);
    }
    GraphFixture {
        name: "canonical_live_scale",
        n,
        edges,
    }
}

// ---------------------------------------------------------------------------
// CPU reference implementations (the GPU<->CPU oracle).
// ---------------------------------------------------------------------------

/// Modularity Q of a given partition on an undirected, unweighted graph.
/// Q = (1/2m) * sum_ij [A_ij - k_i k_j / 2m] * delta(c_i, c_j).
/// Range is [-0.5, 1]. This is the oracle the GPU Louvain modularity is gated
/// against (the CPU shadow at clustering_actor.rs:796 is being DELETED per D8;
/// this reference replaces it for test purposes only).
pub fn modularity(g: &GraphFixture, community: &[u32]) -> f64 {
    assert_eq!(community.len(), g.n, "partition must cover every node");
    let m = g.edge_count() as f64;
    if m == 0.0 {
        return 0.0;
    }
    let two_m = 2.0 * m;
    let deg = g.degrees();
    // sum over communities of (intra_edges/m) - (sum_deg/2m)^2
    let mut comm_internal: HashMap<u32, f64> = HashMap::new();
    let mut comm_degree: HashMap<u32, f64> = HashMap::new();
    for &(u, v) in &g.edges {
        let cu = community[u as usize];
        let cv = community[v as usize];
        if cu == cv {
            *comm_internal.entry(cu).or_insert(0.0) += 1.0; // each undirected edge once
        }
    }
    for i in 0..g.n {
        *comm_degree.entry(community[i]).or_insert(0.0) += deg[i] as f64;
    }
    let mut q = 0.0;
    for (c, internal) in &comm_internal {
        let dc = comm_degree.get(c).copied().unwrap_or(0.0);
        q += (internal / m) - (dc / two_m).powi(2);
    }
    // communities with no internal edges still contribute the -degree term
    for (c, dc) in &comm_degree {
        if !comm_internal.contains_key(c) {
            q -= (dc / two_m).powi(2);
        }
    }
    q
}

/// PageRank reference (power iteration with correct GLOBAL dangling-mass
/// redistribution). This is the oracle for ADR-031 D8's FFI switch from the
/// buggy per-block dangling kernel to the correct global-dangling one.
/// Returns a vector that sums to 1.0.
pub fn pagerank(g: &GraphFixture, damping: f64, iters: usize) -> Vec<f64> {
    let n = g.n;
    if n == 0 {
        return vec![];
    }
    let adj = g.adjacency();
    let out_deg: Vec<f64> = (0..n as u32)
        .map(|i| adj.get(&i).map(|s| s.len() as f64).unwrap_or(0.0))
        .collect();
    let mut rank = vec![1.0 / n as f64; n];
    for _ in 0..iters {
        let mut next = vec![(1.0 - damping) / n as f64; n];
        // Global dangling mass: nodes with no out-edges spill uniformly.
        let dangling: f64 = (0..n).filter(|&i| out_deg[i] == 0.0).map(|i| rank[i]).sum();
        let dangling_share = damping * dangling / n as f64;
        for slot in next.iter_mut() {
            *slot += dangling_share;
        }
        for i in 0..n as u32 {
            if out_deg[i as usize] == 0.0 {
                continue;
            }
            let share = damping * rank[i as usize] / out_deg[i as usize];
            for &j in adj.get(&i).unwrap() {
                next[j as usize] += share;
            }
        }
        rank = next;
    }
    // Renormalise to defeat float drift; the contract is "sums to 1.0".
    let s: f64 = rank.iter().sum();
    if s > 0.0 {
        for r in &mut rank {
            *r /= s;
        }
    }
    rank
}

/// Point set for DBSCAN / LOF references.
pub type Pt = [f64; 2];

fn euclid(a: Pt, b: Pt) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// DBSCAN reference. Returns a label per point: `Some(cluster_id)` (0-based) or
/// `None` for noise. The contract under test (D7): a BORDER point (density-
/// reachable from a core but not itself a core) is assigned to the core's
/// cluster, NOT labelled noise.
pub fn dbscan(points: &[Pt], eps: f64, min_pts: usize) -> Vec<Option<usize>> {
    let n = points.len();
    let mut labels: Vec<Option<usize>> = vec![None; n];
    let mut visited = vec![false; n];
    let neighbours = |i: usize| -> Vec<usize> {
        (0..n)
            .filter(|&j| j != i && euclid(points[i], points[j]) <= eps)
            .collect()
    };
    let mut cluster = 0usize;
    for i in 0..n {
        if visited[i] {
            continue;
        }
        visited[i] = true;
        let mut nbrs = neighbours(i);
        if nbrs.len() + 1 < min_pts {
            // not a core point yet — leave as noise unless later claimed as border
            continue;
        }
        // expand cluster
        labels[i] = Some(cluster);
        let mut k = 0;
        while k < nbrs.len() {
            let q = nbrs[k];
            if !visited[q] {
                visited[q] = true;
                let qn = neighbours(q);
                if qn.len() + 1 >= min_pts {
                    for &x in &qn {
                        if !nbrs.contains(&x) {
                            nbrs.push(x);
                        }
                    }
                }
            }
            // border-point assignment: any point reached is added to the cluster
            if labels[q].is_none() {
                labels[q] = Some(cluster);
            }
            k += 1;
        }
        cluster += 1;
    }
    labels
}

/// Local Outlier Factor reference (Breunig et al.). LOF is approximately 1 for
/// inliers and much greater than 1 for outliers. This is the oracle the GPU LOF
/// ratio kernel (gpu_clustering_kernels.cu:489-493) is asserted against
/// (ADR-031 D4 / D7).
pub fn lof(points: &[Pt], k: usize) -> Vec<f64> {
    let n = points.len();
    if n <= k {
        return vec![1.0; n];
    }
    // k-distance and k-neighbourhoods
    let mut knn: Vec<Vec<usize>> = Vec::with_capacity(n);
    let mut kdist: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        let mut d: Vec<(f64, usize)> = (0..n)
            .filter(|&j| j != i)
            .map(|j| (euclid(points[i], points[j]), j))
            .collect();
        d.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let kth = d[k - 1].0;
        let nbrs: Vec<usize> = d
            .iter()
            .filter(|&&(dd, _)| dd <= kth)
            .map(|&(_, j)| j)
            .collect();
        kdist.push(kth);
        knn.push(nbrs);
    }
    // reachability distance: max(k-distance(o), d(p,o))
    let reach = |p: usize, o: usize| euclid(points[p], points[o]).max(kdist[o]);
    // local reachability density
    let lrd: Vec<f64> = (0..n)
        .map(|p| {
            let nbrs = &knn[p];
            let sum: f64 = nbrs.iter().map(|&o| reach(p, o)).sum();
            if sum == 0.0 {
                f64::INFINITY
            } else {
                nbrs.len() as f64 / sum
            }
        })
        .collect();
    // LOF
    (0..n)
        .map(|p| {
            let nbrs = &knn[p];
            if nbrs.is_empty() || lrd[p] == 0.0 {
                return 1.0;
            }
            let sum: f64 = nbrs.iter().map(|&o| lrd[o] / lrd[p]).sum();
            sum / nbrs.len() as f64
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Greedy CPU community detection (modularity-maximising single-pass agglomerate)
// — used only to produce a *good* partition for the two-clique oracle so the
// modularity test does not depend on the GPU. NOT a Louvain reimplementation;
// just enough to demonstrate the two-clique 2-community optimum.
// ---------------------------------------------------------------------------

/// Returns the natural 2-community partition for the two-clique fixture
/// (community 0 = first clique, 1 = second), which is the modularity optimum.
pub fn two_clique_optimal_partition(g: &GraphFixture) -> Vec<u32> {
    let half = (g.n / 2) as u32;
    (0..g.n as u32)
        .map(|i| if i < half { 0 } else { 1 })
        .collect()
}

/// Count distinct community labels actually used in a partition.
pub fn distinct_communities(partition: &[u32]) -> usize {
    partition.iter().copied().collect::<HashSet<u32>>().len()
}
