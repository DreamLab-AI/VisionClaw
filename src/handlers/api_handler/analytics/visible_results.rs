//! Per-caller filters for the analytics routes whose GPU actors answer for the whole graph and
//! keep no per-caller state (ADR-060, ADR-2184 D1).
//!
//! Community detection, anomaly detection and PageRank answer in positions of `graph.nodes`.
//! Each filter below takes the caller's visible positions
//! ([`crate::handlers::graph_access::visible_positions`]) and rebuilds every field that was
//! derived from the members it removes, so a count, a top-k list or a "centre" node never
//! betrays a hidden node.

use std::collections::HashSet;

use visionclaw_domain::models::graph::GraphData;

use super::community::Community;
use super::Anomaly;
use crate::actors::gpu::pagerank_actor::{pagerank_statistics, PageRankNode, PageRankResult};

/// Communities restricted to visible members. A community with no visible member disappears,
/// `size` is recounted and `center_node` becomes the first visible member.
pub(crate) fn gate_communities(
    communities: Vec<Community>,
    visible: &HashSet<u32>,
) -> Vec<Community> {
    let mut out: Vec<Community> = communities
        .into_iter()
        .filter_map(|mut c| {
            c.nodes.retain(|n| visible.contains(n));
            if c.nodes.is_empty() {
                return None;
            }
            c.size = c.nodes.len() as u32;
            c.center_node = c.nodes.first().copied();
            Some(c)
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.size));
    out
}

/// PageRank restricted to visible positions. A hidden position's value becomes `NaN`, which
/// serialises as JSON `null`; the summary statistics and the top-k list are recomputed over the
/// visible values, keeping the actor's k.
pub(crate) fn gate_pagerank(mut result: PageRankResult, visible: &HashSet<u32>) -> PageRankResult {
    let k = result.top_nodes.len();
    for (i, v) in result.pagerank_values.iter_mut().enumerate() {
        if !visible.contains(&(i as u32)) {
            *v = f32::NAN;
        }
    }
    let mut shown: Vec<(u32, f32)> = result
        .pagerank_values
        .iter()
        .enumerate()
        .filter(|(i, _)| visible.contains(&(*i as u32)))
        .map(|(i, &v)| (i as u32, v))
        .collect();
    let values: Vec<f32> = shown.iter().map(|&(_, v)| v).collect();
    let stats = pagerank_statistics(
        &values,
        result.stats.iterations,
        result.stats.converged,
        result.stats.computation_time_ms,
    );
    shown.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result.top_nodes = shown
        .into_iter()
        .take(k)
        .enumerate()
        .map(|(rank, (node_id, pagerank))| PageRankNode {
            node_id,
            pagerank,
            rank: rank + 1,
        })
        .collect();
    result.stats = stats;
    result
}

/// Structural anomalies restricted to visible positions. The converter names a node
/// `node_<position>`; an id that does not parse that way is dropped (fail closed).
pub(crate) fn gate_anomalies(anomalies: Vec<Anomaly>, visible: &HashSet<u32>) -> Vec<Anomaly> {
    anomalies
        .into_iter()
        .filter(|a| {
            a.node_id
                .strip_prefix("node_")
                .and_then(|i| i.parse::<u32>().ok())
                .is_some_and(|i| visible.contains(&i))
        })
        .collect()
}

/// Whether `/sssp/compute` may start from `source`. The graph actor keys its search by node id,
/// so this checks the id against the caller's view; a hidden source must read exactly as a
/// missing one.
pub(crate) fn sssp_source_visible(view: &GraphData, source: u32) -> bool {
    view.nodes.iter().any(|n| n.id == source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::gpu::pagerank_actor::PageRankStats;
    use visionclaw_domain::models::node::Node;

    fn visible() -> HashSet<u32> {
        [0, 2, 3].into_iter().collect()
    }

    fn community(nodes: Vec<u32>) -> Community {
        Community {
            id: "c".into(),
            label: "Community 0".into(),
            size: nodes.len() as u32,
            center_node: nodes.first().copied(),
            nodes,
            modularity_contribution: 0.1,
            color: "#000000".into(),
        }
    }

    fn anomaly(node_id: &str) -> Anomaly {
        Anomaly {
            id: "a".into(),
            node_id: node_id.into(),
            r#type: "local_outlier".into(),
            severity: "high".into(),
            score: 2.0,
            description: String::new(),
            timestamp: 0,
            metadata: None,
        }
    }

    #[test]
    fn communities_lose_hidden_members_and_recount() {
        let out = gate_communities(vec![community(vec![1, 2]), community(vec![1])], &visible());
        assert_eq!(out.len(), 1, "a hidden-only community must disappear");
        assert_eq!(out[0].nodes, vec![2]);
        assert_eq!(out[0].size, 1);
        assert_eq!(
            out[0].center_node,
            Some(2),
            "the centre must not be a hidden node"
        );
    }

    #[test]
    fn pagerank_hides_values_and_recomputes_top_k_and_stats() {
        let values = vec![0.1, 0.5, 0.3, 0.1];
        let result = PageRankResult {
            pagerank_values: values.clone(),
            iterations: 7,
            converged: true,
            convergence_value: 0.0,
            top_nodes: vec![
                PageRankNode {
                    node_id: 1,
                    pagerank: 0.5,
                    rank: 1,
                },
                PageRankNode {
                    node_id: 2,
                    pagerank: 0.3,
                    rank: 2,
                },
            ],
            stats: pagerank_statistics(&values, 7, true, 3),
        };
        let out = gate_pagerank(result, &visible());
        assert!(
            out.pagerank_values[1].is_nan(),
            "a hidden value must not be sent"
        );
        assert_eq!(
            serde_json::to_value(&out.pagerank_values).unwrap(),
            serde_json::json!([0.1f32, null, 0.3f32, 0.1f32])
        );
        let top: Vec<(u32, usize)> = out.top_nodes.iter().map(|n| (n.node_id, n.rank)).collect();
        assert_eq!(
            top,
            vec![(2, 1), (0, 2)],
            "top-k refilled from visible nodes, ranks from 1"
        );
        assert_eq!(out.stats.total_nodes, 3);
        assert!((out.stats.max_pagerank - 0.3).abs() < 1e-6);
        let _: &PageRankStats = &out.stats;
    }

    #[test]
    fn anomalies_on_hidden_or_unparsable_nodes_are_dropped() {
        let out = gate_anomalies(
            vec![anomaly("node_1"), anomaly("node_2"), anomaly("agent-x")],
            &visible(),
        );
        let ids: Vec<&str> = out.iter().map(|a| a.node_id.as_str()).collect();
        assert_eq!(ids, vec!["node_2"]);
    }

    #[test]
    fn a_hidden_sssp_source_is_not_startable() {
        let mut view = GraphData::new();
        for id in [0, 2] {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            view.nodes.push(n);
        }
        assert!(sssp_source_visible(&view, 2));
        assert!(!sssp_source_visible(&view, 1));
    }
}
