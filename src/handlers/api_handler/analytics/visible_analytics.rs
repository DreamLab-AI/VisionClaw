//! Per-node analytics for a caller whose view hides nodes (ADR-060).
//!
//! The GPU analysers run over the whole graph, so a gated caller's request never reaches them.
//! A GPU SSSP run may also be written to the shared `node_sssp` map behind wire slot 28; that
//! happens only when the request sets `ComputeSSP::publish`, which callers set only when the
//! graph hides nothing from anyone (`graph_access::stream_safe`).
//!
//! Every answer here is computed on the CPU over the caller's view
//! ([`CallerGate::view`]) and keyed by node id, so a hidden node is indistinguishable from an
//! absent one: no distance, component, community, modularity or rank is shaped by it, and no
//! reply is a whole-graph-length array that would name hidden ids by their slot. Shortest paths
//! come from the one CPU Dijkstra, [`crate::services::pathfinding::single_source`].

use std::collections::{BTreeMap, HashMap};

use visionclaw_domain::models::graph::GraphData;

use crate::actors::gpu::analytics_telemetry::ExecutionPath;
use crate::actors::gpu::pagerank_actor::{pagerank_statistics, PageRankNode, PageRankStats};
use crate::handlers::api_handler::analytics::community::Community;
use crate::handlers::api_handler::analytics::Cluster;
use crate::handlers::graph_access::CallerGate;
use crate::services::pathfinding::single_source;

/// The undirected weighted adjacency of `view`, keyed by node id: every edge in both directions,
/// a self-loop once, an edge with an endpoint outside `view` skipped (as the GPU upload does).
fn adjacency(view: &GraphData) -> BTreeMap<u32, Vec<(u32, f32)>> {
    let mut adj: BTreeMap<u32, Vec<(u32, f32)>> =
        view.nodes.iter().map(|n| (n.id, Vec::new())).collect();
    for e in &view.edges {
        if !adj.contains_key(&e.source) || !adj.contains_key(&e.target) {
            continue;
        }
        adj.get_mut(&e.source).unwrap().push((e.target, e.weight));
        if e.source != e.target {
            adj.get_mut(&e.target).unwrap().push((e.source, e.weight));
        }
    }
    adj
}

/// The SSSP reply for a caller whose view hides nodes: only visible nodes reachable within
/// the cap, keyed by node id. A per-index array would name every hidden id by its slot.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VisibleSsspResult {
    /// Distance from the source to each reachable visible node, keyed by node id. Unreachable
    /// nodes and nodes beyond `max_distance` are absent.
    pub distances: BTreeMap<u32, f32>,
    /// The source node id.
    pub source_idx: usize,
    /// Visible nodes reached (before the cap, as the actor counts).
    pub nodes_reached: usize,
    /// Largest distance found (before the cap, as the actor reports).
    pub max_distance: f32,
    /// Computation time in milliseconds.
    pub computation_time_ms: u64,
}

/// Single-source shortest paths from GPU position `source_slot` over the caller's view. `None`
/// when that position is not visible to the caller (hidden, drifted or past the end), which the
/// route answers exactly as an out-of-range source. `nodes_reached` and `max_distance` are taken
/// before the `max_distance` cap, as the actor does.
pub(crate) fn visible_sssp(
    gate: &CallerGate,
    source_slot: usize,
    max_distance: Option<f32>,
) -> Option<VisibleSsspResult> {
    let start = std::time::Instant::now();
    let source = gate.id_at_slot(source_slot)?;
    let sp = single_source(gate.view(), source)?;
    let nodes_reached = sp.distances.len();
    let max_seen = sp.distances.values().copied().fold(0.0f32, f32::max);
    Some(VisibleSsspResult {
        distances: sp
            .distances
            .into_iter()
            .filter(|&(_, d)| max_distance.is_none_or(|cap| d <= cap))
            .collect(),
        source_idx: source as usize,
        nodes_reached,
        max_distance: max_seen,
        computation_time_ms: start.elapsed().as_millis() as u64,
    })
}

/// The components reply for a caller whose view hides nodes: a label per visible node id only,
/// labels numbered densely from 0 in order of each component's smallest member.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VisibleComponentsResult {
    /// Component label of each visible node, keyed by node id.
    pub labels: BTreeMap<u32, u32>,
    /// Number of visible components.
    pub num_components: usize,
    /// Size of each visible component, largest first.
    pub component_sizes: Vec<usize>,
    /// Largest visible component size.
    pub largest_component_size: usize,
    /// Whether the visible subgraph is connected.
    pub is_connected: bool,
    /// Iterations (0: union-find).
    pub iterations: u32,
    /// Computation time in milliseconds.
    pub computation_time_ms: u64,
    /// The path that ran (the CPU visible-subgraph path, not a GPU fallback).
    pub execution_path: ExecutionPath,
}

/// Renumber an id → raw-label map densely from 0, in order of each label's smallest member
/// (ids iterate in ascending order), so a numbering gap never stands for a hidden-only group.
fn dense_labels<L: std::hash::Hash + Eq + Copy>(raw: &BTreeMap<u32, L>) -> BTreeMap<u32, u32> {
    let mut dense: HashMap<L, u32> = HashMap::new();
    raw.iter()
        .map(|(&id, &l)| {
            let next = dense.len() as u32;
            (id, *dense.entry(l).or_insert(next))
        })
        .collect()
}

/// Connected components of the caller's view; hidden nodes are absent and count towards
/// nothing.
pub(crate) fn visible_components(gate: &CallerGate) -> VisibleComponentsResult {
    let start = std::time::Instant::now();
    let adj = adjacency(gate.view());
    // Flood fill from each unlabelled node in ascending id order: the root is the smallest member.
    let mut root: BTreeMap<u32, u32> = BTreeMap::new();
    for &s in adj.keys() {
        if root.contains_key(&s) {
            continue;
        }
        let mut stack = vec![s];
        root.insert(s, s);
        while let Some(u) = stack.pop() {
            for &(v, _) in &adj[&u] {
                if let std::collections::btree_map::Entry::Vacant(e) = root.entry(v) {
                    e.insert(s);
                    stack.push(v);
                }
            }
        }
    }
    let labels = dense_labels(&root);
    let mut component_sizes =
        vec![0usize; labels.values().map(|&l| l as usize + 1).max().unwrap_or(0)];
    for &l in labels.values() {
        component_sizes[l as usize] += 1;
    }
    component_sizes.sort_unstable_by(|a, b| b.cmp(a));
    VisibleComponentsResult {
        labels,
        num_components: component_sizes.len(),
        largest_component_size: component_sizes.first().copied().unwrap_or(0),
        is_connected: component_sizes.len() == 1,
        component_sizes,
        iterations: 0,
        computation_time_ms: start.elapsed().as_millis() as u64,
        execution_path: ExecutionPath::CpuVisibleSubgraph,
    }
}

/// The DBSCAN keyword rule (`ClusteringActor::generate_cluster_keywords`), re-applied to the
/// visible members so neither keyword is derived from a hidden one.
fn cluster_keywords(nodes: &[u32]) -> Vec<String> {
    let Some(first) = nodes.first() else {
        return vec!["empty".to_string()];
    };
    let size = match nodes.len() {
        1 => "singleton",
        2..=5 => "small",
        6..=20 => "medium",
        _ => "large",
    };
    vec![size.to_string(), format!("cluster-{}", first % 10)]
}

/// Clusters as the caller may see them. Members are graph node ids (DBSCAN translates GPU
/// indices), so they are filtered by [`CallerGate::ids`]; hidden members are removed, a cluster
/// left empty disappears, and a cluster that lost members has its count, keywords and centroid
/// recomputed from the visible ones (the centroid from their snapshot positions).
pub(crate) fn gate_clusters(clusters: &[Cluster], gate: &CallerGate) -> Vec<Cluster> {
    let position: HashMap<u32, [f32; 3]> = gate
        .graph()
        .nodes
        .iter()
        .map(|n| (n.id, [n.data.x, n.data.y, n.data.z]))
        .collect();
    clusters
        .iter()
        .filter_map(|c| {
            let nodes: Vec<u32> = c
                .nodes
                .iter()
                .copied()
                .filter(|id| gate.ids().contains(*id))
                .collect();
            if nodes.is_empty() {
                return None;
            }
            if nodes.len() == c.nodes.len() {
                return Some(c.clone());
            }
            let pts: Vec<[f32; 3]> = nodes
                .iter()
                .filter_map(|id| position.get(id))
                .copied()
                .collect();
            let centroid = (!pts.is_empty()).then(|| {
                let k = pts.len() as f32;
                let sum = pts
                    .iter()
                    .fold([0.0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
                [sum[0] / k, sum[1] / k, sum[2] / k]
            });
            Some(Cluster {
                node_count: nodes.len() as u32,
                keywords: cluster_keywords(&nodes),
                centroid,
                nodes,
                ..c.clone()
            })
        })
        .collect()
}

/// Point-to-point SSSP for a caller whose view hides nodes, computed over the view.
/// `source_id`/`target_id` are node ids. A hidden or unknown source gets the same error; a
/// hidden or unknown target reads as unreachable. Returns `(distance, exists, nodes_reached)`,
/// with `f32::MAX` for unreachable as the GPU arm does.
pub(crate) fn visible_point_to_point(
    gate: &CallerGate,
    source_id: u32,
    target_id: u32,
) -> Result<(f32, bool, usize), String> {
    let sp = single_source(gate.view(), source_id)
        .ok_or_else(|| format!("Source node {} not found in graph", source_id))?;
    Ok(match sp.distances.get(&target_id) {
        Some(&d) => (d, true, sp.distances.len()),
        None => (f32::MAX, false, sp.distances.len()),
    })
}

/// Modularity of `assignment` (node id → community) over `view`, edges undirected and weighted
/// (Newman's Q). Nodes of `view` missing from `assignment` count as singletons. 0.0 for a view
/// with no edge weight.
pub(crate) fn modularity(view: &GraphData, assignment: &BTreeMap<u32, u32>) -> f32 {
    let adj = adjacency(view);
    let degree: BTreeMap<u32, f64> = adj
        .iter()
        .map(|(&u, es)| (u, es.iter().map(|&(_, w)| w as f64).sum()))
        .collect();
    let two_m: f64 = degree.values().sum();
    if two_m <= 0.0 {
        return 0.0;
    }
    let comm = |u: u32| {
        assignment
            .get(&u)
            .map(|&c| c as i64)
            .unwrap_or(-(u as i64) - 1)
    };
    let mut inside = 0.0f64;
    let mut tot: HashMap<i64, f64> = HashMap::new();
    for (&u, es) in &adj {
        *tot.entry(comm(u)).or_default() += degree[&u];
        for &(v, w) in es {
            if comm(u) == comm(v) {
                inside += w as f64;
            }
        }
    }
    let expected: f64 = tot.values().map(|t| (t / two_m).powi(2)).sum();
    (inside / two_m - expected) as f32
}

/// Communities of `view` by greedy modularity local moving (the first phase of Louvain):
/// each node, in ascending id order, moves to the neighbouring community with the largest
/// positive modularity gain, until a full sweep moves nothing. Deterministic; returns node id →
/// dense community label, the sweeps run, and whether it settled within `max_iterations`.
pub(crate) fn local_moving(
    view: &GraphData,
    max_iterations: u32,
) -> (BTreeMap<u32, u32>, u32, bool) {
    let adj = adjacency(view);
    let degree: BTreeMap<u32, f64> = adj
        .iter()
        .map(|(&u, es)| (u, es.iter().map(|&(_, w)| w as f64).sum()))
        .collect();
    let two_m: f64 = degree.values().sum();
    let mut comm: BTreeMap<u32, u32> = adj.keys().map(|&u| (u, u)).collect();
    let mut tot: HashMap<u32, f64> = degree.iter().map(|(&u, &k)| (u, k)).collect();
    let mut iterations = 0;
    let mut converged = two_m <= 0.0;
    while !converged && iterations < max_iterations.max(1) {
        iterations += 1;
        let mut moved = false;
        for (&u, es) in &adj {
            let k = degree[&u];
            let own = comm[&u];
            *tot.get_mut(&own).unwrap() -= k;
            let mut links: BTreeMap<u32, f64> = BTreeMap::new();
            for &(v, w) in es {
                if v != u {
                    *links.entry(comm[&v]).or_default() += w as f64;
                }
            }
            let gain = |c: u32, l: f64| l - tot.get(&c).copied().unwrap_or(0.0) * k / two_m;
            let mut best = (own, gain(own, links.get(&own).copied().unwrap_or(0.0)));
            for (&c, &l) in &links {
                let g = gain(c, l);
                if g > best.1 + 1e-12 {
                    best = (c, g);
                }
            }
            *tot.entry(best.0).or_default() += k;
            if best.0 != own {
                comm.insert(u, best.0);
                moved = true;
            }
        }
        converged = !moved;
    }
    (dense_labels(&comm), iterations, converged)
}

const COMMUNITY_COLOURS: [&str; 20] = [
    "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FECA57", "#FF9FF3", "#54A0FF", "#5F27CD",
    "#00D2D3", "#FF9F43", "#C44569", "#40407A", "#706FD3", "#F97F51", "#833471", "#A55EEA",
    "#26D0CE", "#FD79A8", "#FDCB6E", "#6C5CE7",
];

/// Communities of the caller's view: modularity local moving over the view, numbered densely,
/// modularity and each community's contribution computed over the view. Returns
/// `(communities largest first, modularity, iterations, converged)`.
pub(crate) fn visible_communities(
    gate: &CallerGate,
    max_iterations: u32,
) -> (Vec<Community>, f32, u32, bool) {
    let view = gate.view();
    let (labels, iterations, converged) = local_moving(view, max_iterations);
    let q = modularity(view, &labels);
    let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (&id, &l) in &labels {
        members.entry(l).or_default().push(id);
    }
    // A community's share of Q, in one pass over the view: its own Newman term
    // in_c/2m - (tot_c/2m)^2 less the singleton terms of its members, so the shares sum to
    // Q minus the all-singletons baseline.
    let adj = adjacency(view);
    let degree = |u: u32| -> f64 { adj[&u].iter().map(|&(_, w)| w as f64).sum() };
    let two_m: f64 = adj.keys().map(|&u| degree(u)).sum();
    let share = |nodes: &[u32]| -> f32 {
        if two_m <= 0.0 {
            return 0.0;
        }
        let (mut inside, mut tot, mut baseline) = (0.0f64, 0.0f64, 0.0f64);
        for &u in nodes {
            let k = degree(u);
            tot += k;
            let own: f64 = adj[&u]
                .iter()
                .filter(|&&(v, _)| v == u)
                .map(|&(_, w)| w as f64)
                .sum();
            baseline += own / two_m - (k / two_m).powi(2);
            inside += adj[&u]
                .iter()
                .filter(|&&(v, _)| labels.get(&v) == labels.get(&u))
                .map(|&(_, w)| w as f64)
                .sum::<f64>();
        }
        (inside / two_m - (tot / two_m).powi(2) - baseline) as f32
    };
    let mut communities: Vec<Community> = members
        .into_iter()
        .map(|(l, nodes)| Community {
            id: format!("community-{l}"),
            label: format!("Community {l}"),
            size: nodes.len() as u32,
            center_node: nodes.first().copied(),
            modularity_contribution: share(&nodes),
            color: COMMUNITY_COLOURS[l as usize % COMMUNITY_COLOURS.len()].to_string(),
            nodes,
        })
        .collect();
    communities.sort_by_key(|c| std::cmp::Reverse(c.size));
    (communities, q, iterations, converged)
}

/// PageRank for a caller whose view hides nodes: values for visible nodes only, keyed by node
/// id, computed over the view.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VisiblePageRankResult {
    /// PageRank of each visible node, keyed by node id.
    pub pagerank_values: BTreeMap<u32, f32>,
    /// Iterations performed.
    pub iterations: u32,
    /// Whether the run converged.
    pub converged: bool,
    /// Final L1 change between iterations.
    pub convergence_value: f32,
    /// Top nodes by PageRank.
    pub top_nodes: Vec<PageRankNode>,
    /// Summary statistics over the visible values.
    pub stats: PageRankStats,
    /// The path that ran.
    pub execution_path: ExecutionPath,
}

/// PageRank over the caller's view (power iteration, undirected edges as two arcs weighted
/// by edge weight, dangling mass spread evenly). `top_k` nodes are listed.
pub(crate) fn visible_pagerank(
    gate: &CallerGate,
    damping: f32,
    max_iterations: u32,
    epsilon: f32,
    top_k: usize,
) -> VisiblePageRankResult {
    let start = std::time::Instant::now();
    let adj = adjacency(gate.view());
    let ids: Vec<u32> = adj.keys().copied().collect();
    let index: HashMap<u32, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let n = ids.len();
    let out_w: Vec<f32> = ids
        .iter()
        .map(|id| adj[id].iter().map(|&(_, w)| w).sum())
        .collect();
    let d = damping.clamp(0.0, 1.0);
    let mut rank = vec![if n > 0 { 1.0 / n as f32 } else { 0.0 }; n];
    let (mut iterations, mut converged, mut delta) = (0u32, false, 0.0f32);
    while n > 0 && iterations < max_iterations.max(1) {
        iterations += 1;
        let dangling: f32 = (0..n).filter(|&i| out_w[i] <= 0.0).map(|i| rank[i]).sum();
        let base = (1.0 - d) / n as f32 + d * dangling / n as f32;
        let mut next = vec![base; n];
        for (i, id) in ids.iter().enumerate() {
            if out_w[i] <= 0.0 {
                continue;
            }
            for &(v, w) in &adj[id] {
                next[index[&v]] += d * rank[i] * w / out_w[i];
            }
        }
        delta = next.iter().zip(&rank).map(|(a, b)| (a - b).abs()).sum();
        rank = next;
        if delta < epsilon {
            converged = true;
            break;
        }
    }
    let elapsed = start.elapsed().as_millis() as u64;
    let stats = pagerank_statistics(&rank, iterations, converged, elapsed);
    let mut ranked: Vec<(u32, f32)> = ids.iter().copied().zip(rank.iter().copied()).collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    VisiblePageRankResult {
        top_nodes: ranked
            .iter()
            .take(top_k)
            .enumerate()
            .map(|(r, &(node_id, pagerank))| PageRankNode {
                node_id,
                pagerank,
                rank: r + 1,
            })
            .collect(),
        pagerank_values: ranked.into_iter().collect(),
        iterations,
        converged,
        convergence_value: delta,
        stats,
        execution_path: ExecutionPath::CpuVisibleSubgraph,
    }
}

/// Landmark distances for a caller whose view hides nodes. Columns are the visible node ids in
/// `node_ids` order; row `l` holds the exact distance over the view from landmark `landmarks[l]`
/// to each of them (`null` when unreachable). A pair `(i, j)` is bounded by
/// `min over l of row_l[i] + row_l[j]`, which the client computes on demand: the rows are
/// `k x n`, never the `n x n` matrix, so a gated call stays linear in the view.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VisibleApspResult {
    /// The visible node ids, the column order of every row.
    pub node_ids: Vec<u32>,
    /// Landmark node ids, the row order.
    pub landmarks: Vec<u32>,
    /// Row-major `[landmarks.len() x node_ids.len()]` distances.
    pub landmark_distances: Vec<f32>,
    /// `node_ids.len()`.
    pub num_nodes: usize,
    /// `landmarks.len()`.
    pub num_landmarks: usize,
    /// Computation time in milliseconds.
    pub computation_time_ms: u64,
    /// The path that ran.
    pub execution_path: ExecutionPath,
}

/// Most landmarks a gated call may ask for (each is one Dijkstra over the view).
pub(crate) const MAX_VISIBLE_LANDMARKS: usize = 256;

/// [`VisibleApspResult`] with `num_landmarks` landmarks (0: ⌈√n⌉, capped at
/// [`MAX_VISIBLE_LANDMARKS`]) chosen evenly over the visible ids in ascending order.
pub(crate) fn visible_apsp(gate: &CallerGate, num_landmarks: usize) -> VisibleApspResult {
    let start = std::time::Instant::now();
    let view = gate.view();
    let node_ids: Vec<u32> = {
        let mut v: Vec<u32> = view.nodes.iter().map(|n| n.id).collect();
        v.sort_unstable();
        v
    };
    let n = node_ids.len();
    let k = if num_landmarks == 0 {
        (n as f64).sqrt().ceil() as usize
    } else {
        num_landmarks
    }
    .min(MAX_VISIBLE_LANDMARKS)
    .min(n);
    let landmarks: Vec<u32> = (0..k).map(|i| node_ids[i * n / k.max(1)]).collect();
    let mut landmark_distances = Vec::with_capacity(k * n);
    for &l in &landmarks {
        let row = single_source(view, l)
            .map(|sp| sp.distances)
            .unwrap_or_default();
        landmark_distances.extend(
            node_ids
                .iter()
                .map(|id| row.get(id).copied().unwrap_or(f32::INFINITY)),
        );
    }
    VisibleApspResult {
        num_nodes: n,
        num_landmarks: landmarks.len(),
        node_ids,
        landmarks,
        landmark_distances,
        computation_time_ms: start.elapsed().as_millis() as u64,
        execution_path: ExecutionPath::CpuVisibleSubgraph,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::graph_access::gate_for_graph_with;
    use std::sync::Arc;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::node::Node;

    /// Path 0–1–2 (weight 1 each) with a long 0–2 bypass (weight 5), node 1 hidden (no
    /// `public` flag), and an isolated visible node 3.
    fn graph() -> GraphData {
        let node = |id: u32| {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            // `new_with_id` places nodes at random; pin the position.
            (n.data.x, n.data.y, n.data.z) = (id as f32, 0.0, 0.0);
            if id != 1 {
                n.metadata.insert("public".into(), "true".into());
            }
            n
        };
        GraphData {
            nodes: (0..4).map(node).collect(),
            edges: vec![
                Edge::new(0, 1, 1.0),
                Edge::new(1, 2, 1.0),
                Edge::new(0, 2, 5.0),
            ],
            ..GraphData::default()
        }
    }

    fn gate_of(g: GraphData) -> CallerGate {
        gate_for_graph_with(&Arc::new(g), None, true).expect("node 1 is hidden")
    }

    #[test]
    fn sssp_never_routes_through_or_reaches_a_hidden_node() {
        let g = gate_of(graph());
        let r = visible_sssp(&g, 0, None).unwrap();
        assert_eq!(r.distances.get(&0), Some(&0.0));
        assert_eq!(r.distances.get(&1), None, "a hidden node is never named");
        assert_eq!(
            r.distances.get(&2),
            Some(&5.0),
            "the short way runs through hidden 1"
        );
        assert_eq!(r.distances.get(&3), None, "unreachable is absent");
        assert_eq!((r.nodes_reached, r.max_distance), (2, 5.0));
    }

    #[test]
    fn sssp_cap_matches_the_gpu_shape() {
        let g = gate_of(graph());
        let r = visible_sssp(&g, 0, Some(2.0)).unwrap();
        assert_eq!(r.distances.get(&2), None, "beyond the cap is omitted");
        assert_eq!(
            r.max_distance, 5.0,
            "taken before the cap, as the actor does"
        );
    }

    #[test]
    fn components_split_where_only_a_hidden_node_joined_them() {
        let mut g = graph();
        g.edges.retain(|e| !(e.source == 0 && e.target == 2));
        let r = visible_components(&gate_of(g));
        assert_eq!(r.labels.get(&1), None, "a hidden node gets no label");
        assert_ne!(
            r.labels[&0], r.labels[&2],
            "0 and 2 met only through hidden 1"
        );
        assert_eq!(r.num_components, 3);
        assert_eq!(r.component_sizes, vec![1, 1, 1]);
        assert!(!r.is_connected);
        assert!(!r.execution_path.is_fallback());
    }

    /// Review item 5: a gated reply must not be a whole-graph-length array, which spells out the
    /// exact hidden ids (and their count) as the INFINITY / u32::MAX slots.
    #[test]
    fn gated_sssp_json_names_only_visible_reachable_ids() {
        let g = gate_of(graph());
        let json = serde_json::to_value(visible_sssp(&g, 0, None).unwrap()).unwrap();
        let d = json["distances"]
            .as_object()
            .expect("distances keyed by node id, not a per-index array");
        let keys: Vec<&str> = d.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["0", "2"],
            "hidden 1 and unreachable 3 are absent"
        );
        assert_eq!(d["2"], 5.0);
        assert_eq!(json["nodes_reached"], 2);
        let capped = serde_json::to_value(visible_sssp(&g, 0, Some(2.0)).unwrap()).unwrap();
        let keys: Vec<&String> = capped["distances"].as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["0"], "beyond the cap is omitted, not f32::MAX");
    }

    #[test]
    fn gated_components_json_labels_only_visible_ids_densely() {
        let mut g = graph();
        g.edges.retain(|e| !(e.source == 0 && e.target == 2));
        let json = serde_json::to_value(visible_components(&gate_of(g))).unwrap();
        let labels = json["labels"]
            .as_object()
            .expect("labels keyed by node id, not a per-index array");
        let keys: Vec<&str> = labels.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["0", "2", "3"], "no slot for hidden 1");
        let mut got: Vec<u64> = labels.values().map(|l| l.as_u64().unwrap()).collect();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![0, 1, 2],
            "component labels are dense, not positions"
        );
        assert_eq!(json["num_components"], 3);
    }

    #[test]
    fn clusters_lose_hidden_members_and_recompute_what_they_derived() {
        let g = gate_of(graph());
        let c = |id: &str, nodes: Vec<u32>| Cluster {
            id: id.into(),
            label: format!("Cluster {id}"),
            node_count: nodes.len() as u32,
            coherence: 0.5,
            color: "#000000".into(),
            keywords: vec!["x".into()],
            centroid: Some([9.0, 9.0, 9.0]),
            nodes,
        };
        let out = gate_clusters(&[c("a", vec![1, 0, 2]), c("b", vec![1])], &g);
        assert_eq!(out.len(), 1, "a cluster of hidden nodes only disappears");
        assert_eq!(out[0].nodes, vec![0, 2]);
        assert_eq!(out[0].node_count, 2);
        assert_eq!(
            out[0].keywords,
            vec!["small".to_string(), "cluster-0".to_string()]
        );
        assert_eq!(out[0].centroid, Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn point_to_point_never_takes_the_hidden_shortcut() {
        let g = gate_of(graph());
        let (d, exists, _) = visible_point_to_point(&g, 0, 2).unwrap();
        assert!(exists);
        assert_eq!(
            d, 5.0,
            "0->2 is 5 over the view; 2 is the hidden route via 1"
        );
    }

    #[test]
    fn point_to_point_starts_from_the_requested_node() {
        let g = gate_of(graph());
        // Node 2 sits at view position 1, which is hidden node 1 in the full graph.
        let (d, _, _) = visible_point_to_point(&g, 2, 0).unwrap();
        assert_eq!(
            d, 5.0,
            "the run must start at node 2, not at the hidden node at slot 1"
        );
    }

    /// Verdict test (the old arm already behaved this way): a hidden source gets exactly the
    /// error an unknown id gets, so the reply is no existence oracle.
    #[test]
    fn point_to_point_hidden_source_reads_as_unknown() {
        let g = gate_of(graph());
        assert_eq!(
            visible_point_to_point(&g, 1, 0).unwrap_err(),
            "Source node 1 not found in graph"
        );
    }

    #[test]
    fn a_hidden_or_past_the_end_sssp_source_is_refused_alike() {
        let g = gate_of(graph());
        assert!(
            visible_sssp(&g, 1, None).is_none(),
            "slot 1 is hidden node 1"
        );
        assert!(visible_sssp(&g, 4, None).is_none(), "slot 4 does not exist");
    }

    /// Two triangles {0,1,2} and {3,4,5} joined by the bridge 2–3: Newman's Q for the
    /// two-triangle split is 2 * (3/7 - (7/14)^2) = 5/14.
    #[test]
    fn modularity_is_newmans_q() {
        let mut g = GraphData {
            nodes: (0..6)
                .map(|id| {
                    let mut n = Node::new_with_id(format!("n{id}"), None);
                    n.id = id;
                    n
                })
                .collect(),
            ..GraphData::default()
        };
        for (a, b) in [(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5), (2, 3)] {
            g.edges.push(Edge::new(a, b, 1.0));
        }
        let split: BTreeMap<u32, u32> = (0..6).map(|i| (i, (i / 3) as u32)).collect();
        assert!((modularity(&g, &split) - 5.0 / 14.0).abs() < 1e-6);
        let (labels, _, converged) = local_moving(&g, 50);
        assert!(converged);
        assert_eq!(
            labels, split,
            "local moving finds the two triangles, numbered densely"
        );
    }

    /// Hidden 1 is the only link between 0 and 2 apart from the weak bypass; with the bypass
    /// removed, 0 and 2 must land in different communities and modularity is that of the view.
    #[test]
    fn communities_and_modularity_are_computed_over_the_view() {
        let mut g = graph();
        g.edges.retain(|e| !(e.source == 0 && e.target == 2));
        g.edges.push(Edge::new(2, 3, 1.0));
        let gate = gate_of(g);
        let (communities, q, _, _) = visible_communities(&gate, 50);
        assert!(
            communities.iter().all(|c| !c.nodes.contains(&1)),
            "a hidden node is never a member"
        );
        let of = |id: u32| communities.iter().position(|c| c.nodes.contains(&id));
        assert_ne!(of(0), of(2), "0 and 2 met only through hidden 1");
        assert_eq!(of(2), of(3));
        let mut labels: Vec<&str> = communities.iter().map(|c| c.label.as_str()).collect();
        labels.sort_unstable();
        assert_eq!(
            labels,
            vec!["Community 0", "Community 1"],
            "dense numbering"
        );
        let assignment: BTreeMap<u32, u32> = communities
            .iter()
            .enumerate()
            .flat_map(|(k, c)| c.nodes.iter().map(move |&id| (id, k as u32)))
            .collect();
        assert!((q - modularity(gate.view(), &assignment)).abs() < 1e-6);
        let total: f32 = communities.iter().map(|c| c.modularity_contribution).sum();
        assert!((total - (q - modularity(gate.view(), &BTreeMap::new()))).abs() < 1e-5);
    }

    /// Hidden 1 is a hub joined to 0 and 2. Over the view, 0, 2 and 3 have no edges between
    /// them but 0–2 (the bypass): 3 is isolated. The hub's rank never shows, and never lifts
    /// its neighbours above what the view alone gives them.
    #[test]
    fn pagerank_is_computed_over_the_view_and_keyed_by_visible_id() {
        let gate = gate_of(graph());
        let r = visible_pagerank(&gate, 0.85, 200, 1e-7, 10);
        let keys: Vec<u32> = r.pagerank_values.keys().copied().collect();
        assert_eq!(keys, vec![0, 2, 3], "no value, not even null, for hidden 1");
        assert!((r.pagerank_values[&0] - r.pagerank_values[&2]).abs() < 1e-6);
        let sum: f32 = r.pagerank_values.values().sum();
        assert!((sum - 1.0).abs() < 1e-4, "a distribution over the view");
        assert!(r.top_nodes.iter().all(|n| n.node_id != 1));
        assert_eq!(r.stats.total_nodes, 3);
        let json = serde_json::to_value(&r).unwrap();
        assert!(json["pagerank_values"]
            .as_object()
            .unwrap()
            .get("1")
            .is_none());
    }

    #[test]
    fn apsp_rows_are_visible_ids_and_never_route_through_a_hidden_node() {
        let gate = gate_of(graph());
        let r = visible_apsp(&gate, 0);
        assert_eq!(r.node_ids, vec![0, 2, 3]);
        assert_eq!(
            r.landmarks,
            vec![0, 2],
            "⌈√3⌉ = 2 landmarks, never hidden 1"
        );
        assert_eq!(r.landmark_distances.len(), 2 * 3, "k x n rows, not n x n");
        let row0 = &r.landmark_distances[0..3];
        assert_eq!(row0[0], 0.0);
        assert_eq!(row0[1], 5.0, "0->2 is 5 over the view, not 2 via hidden 1");
        assert!(row0[2].is_infinite(), "3 is isolated in the view");
        let json = serde_json::to_value(&r).unwrap();
        assert!(json.get("distances").is_none());
    }
}
