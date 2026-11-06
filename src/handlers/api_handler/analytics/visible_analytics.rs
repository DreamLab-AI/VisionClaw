//! Per-node analytics for a caller whose view hides nodes (ADR-060).
//!
//! The GPU analysers run over the whole graph and some publish into shared state (SSSP fills
//! the `node_sssp` stream slot every client receives), so a gated caller never reaches them.
//! Their answer is computed here on the CPU over the caller's visible subgraph, in the GPU's
//! index space (positions of `graph.nodes`) and shape, so a hidden node is indistinguishable
//! from an unreachable or absent one and no distance or component runs through it.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use visionclaw_domain::models::graph::GraphData;

use crate::actors::gpu::analytics_telemetry::ExecutionPath;
use crate::actors::gpu::connected_components_actor::ConnectedComponentsResult;
use crate::actors::gpu::shortest_path_actor::SSSPResult;
use crate::handlers::api_handler::analytics::Cluster;

/// The undirected weighted adjacency the GPU uploads (`ForceComputeActor`): every edge in both
/// directions, a self-loop once, an edge with an unknown endpoint skipped. Only positions in
/// `visible` take part.
fn visible_adjacency(graph: &GraphData, visible: &HashSet<u32>) -> Vec<Vec<(usize, f32)>> {
    let index: HashMap<u32, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();
    let mut adj = vec![Vec::new(); graph.nodes.len()];
    for e in &graph.edges {
        let (Some(&s), Some(&t)) = (index.get(&e.source), index.get(&e.target)) else {
            continue;
        };
        if !visible.contains(&(s as u32)) || !visible.contains(&(t as u32)) {
            continue;
        }
        adj[s].push((t, e.weight));
        if s != t {
            adj[t].push((s, e.weight));
        }
    }
    adj
}

#[derive(PartialEq)]
struct Frontier(f32, usize);
impl Eq for Frontier {}
impl Ord for Frontier {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for Frontier {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Single-source shortest paths from `source` (a visible position) over the visible subgraph,
/// shaped as the GPU actor's result: unreachable and hidden positions are `INFINITY`,
/// `nodes_reached`/`max_distance` are taken before the `max_distance` cap, and the cap sets
/// longer distances to `f32::MAX`.
pub(crate) fn visible_sssp(
    graph: &GraphData,
    visible: &HashSet<u32>,
    source: usize,
    max_distance: Option<f32>,
) -> SSSPResult {
    let start = std::time::Instant::now();
    let adj = visible_adjacency(graph, visible);
    let mut dist = vec![f32::INFINITY; graph.nodes.len()];
    dist[source] = 0.0;
    let mut heap = BinaryHeap::from([Frontier(0.0, source)]);
    while let Some(Frontier(d, u)) = heap.pop() {
        if d > dist[u] {
            continue;
        }
        for &(v, w) in &adj[u] {
            let nd = d + w;
            if nd < dist[v] {
                dist[v] = nd;
                heap.push(Frontier(nd, v));
            }
        }
    }
    let reached: Vec<f32> = dist.iter().copied().filter(|d| *d < f32::MAX).collect();
    let nodes_reached = reached.len();
    let max_seen = reached.iter().copied().fold(0.0f32, f32::max);
    if let Some(cap) = max_distance {
        for d in &mut dist {
            if *d > cap {
                *d = f32::MAX;
            }
        }
    }
    SSSPResult {
        distances: dist,
        source_idx: source,
        nodes_reached,
        max_distance: max_seen,
        computation_time_ms: start.elapsed().as_millis() as u64,
    }
}

/// Label of a position hidden from the caller: never a real component label.
pub(crate) const HIDDEN_LABEL: u32 = u32::MAX;

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = x;
    while parent[c] != r {
        let n = parent[c];
        parent[c] = r;
        c = n;
    }
    r
}

/// Connected components of the visible subgraph. A component is labelled by its smallest
/// position; hidden positions get [`HIDDEN_LABEL`] and count towards nothing.
pub(crate) fn visible_components(
    graph: &GraphData,
    visible: &HashSet<u32>,
) -> ConnectedComponentsResult {
    let start = std::time::Instant::now();
    let n = graph.nodes.len();
    let adj = visible_adjacency(graph, visible);
    let mut parent: Vec<usize> = (0..n).collect();
    for (u, edges) in adj.iter().enumerate() {
        for &(v, _) in edges {
            let (a, b) = (find(&mut parent, u), find(&mut parent, v));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut labels = vec![HIDDEN_LABEL; n];
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for (i, label) in labels.iter_mut().enumerate() {
        if visible.contains(&(i as u32)) {
            let root = find(&mut parent, i) as u32;
            *label = root;
            *sizes.entry(root).or_default() += 1;
        }
    }
    let mut component_sizes: Vec<usize> = sizes.values().copied().collect();
    component_sizes.sort_unstable_by(|a, b| b.cmp(a));
    ConnectedComponentsResult {
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
/// indices); hidden members are removed, a cluster left empty disappears, and a cluster that
/// lost members has its count, keywords and centroid recomputed from the visible ones (the
/// centroid from their snapshot positions).
pub(crate) fn gate_clusters(
    clusters: &[Cluster],
    graph: &GraphData,
    visible_ids: &HashSet<u32>,
) -> Vec<Cluster> {
    let position: HashMap<u32, [f32; 3]> = graph
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
                .filter(|id| visible_ids.contains(id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::node::Node;

    /// Path 0–1–2 (weight 1 each) with a long 0–2 bypass (weight 5), node 1 hidden, and an
    /// isolated visible node 3.
    fn fixture() -> (GraphData, HashSet<u32>) {
        let node = |id: u32| {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            // `new_with_id` places nodes at random; pin the position.
            (n.data.x, n.data.y, n.data.z) = (id as f32, 0.0, 0.0);
            n
        };
        let g = GraphData {
            nodes: (0..4).map(node).collect(),
            edges: vec![
                Edge::new(0, 1, 1.0),
                Edge::new(1, 2, 1.0),
                Edge::new(0, 2, 5.0),
            ],
            ..GraphData::default()
        };
        (g, [0, 2, 3].into_iter().collect())
    }

    #[test]
    fn sssp_never_routes_through_or_reaches_a_hidden_node() {
        let (g, v) = fixture();
        let r = visible_sssp(&g, &v, 0, None);
        assert_eq!(r.distances[0], 0.0);
        assert!(r.distances[1].is_infinite(), "hidden reads as unreachable");
        assert_eq!(r.distances[2], 5.0, "the short way runs through hidden 1");
        assert!(r.distances[3].is_infinite());
        assert_eq!((r.nodes_reached, r.max_distance), (2, 5.0));
    }

    #[test]
    fn sssp_cap_matches_the_gpu_shape() {
        let (g, v) = fixture();
        let r = visible_sssp(&g, &v, 0, Some(2.0));
        assert_eq!(r.distances[2], f32::MAX);
        assert_eq!(
            r.max_distance, 5.0,
            "taken before the cap, as the actor does"
        );
    }

    #[test]
    fn components_split_where_only_a_hidden_node_joined_them() {
        let (mut g, v) = fixture();
        g.edges.retain(|e| !(e.source == 0 && e.target == 2));
        let r = visible_components(&g, &v);
        assert_eq!(r.labels[1], HIDDEN_LABEL);
        assert_ne!(
            r.labels[0], r.labels[2],
            "0 and 2 met only through hidden 1"
        );
        assert_eq!(r.num_components, 3);
        assert_eq!(r.component_sizes, vec![1, 1, 1]);
        assert!(!r.is_connected);
        assert!(!r.execution_path.is_fallback());
    }

    #[test]
    fn clusters_lose_hidden_members_and_recompute_what_they_derived() {
        let (g, _) = fixture();
        let visible_ids: HashSet<u32> = [0, 2, 3].into_iter().collect();
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
        let out = gate_clusters(&[c("a", vec![1, 0, 2]), c("b", vec![1])], &g, &visible_ids);
        assert_eq!(out.len(), 1, "a cluster of hidden nodes only disappears");
        assert_eq!(out[0].nodes, vec![0, 2]);
        assert_eq!(out[0].node_count, 2);
        assert_eq!(
            out[0].keywords,
            vec!["small".to_string(), "cluster-0".to_string()]
        );
        assert_eq!(out[0].centroid, Some([1.0, 0.0, 0.0]));
    }
}
