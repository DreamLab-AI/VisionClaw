//! Per-caller filter for structural anomaly detection, the one analytics route whose GPU answer
//! is still filtered on the way out rather than recomputed over the caller's view (ADR-060).
//!
//! LOF and Z-score score node *positions*, and the positions come from a layout every node,
//! hidden ones included, took part in; recomputing over the view would not remove that
//! influence, so the route only drops hidden members. Communities, PageRank, SSSP, components
//! and APSP are computed over the view instead (`visible_analytics`).

use super::Anomaly;
use crate::handlers::graph_access::VisibleSlots;

/// Structural anomalies restricted to visible positions. The converter names a node
/// `node_<position>`; an id that does not parse that way is dropped (fail closed).
pub(crate) fn gate_anomalies(anomalies: Vec<Anomaly>, visible: &VisibleSlots) -> Vec<Anomaly> {
    anomalies
        .into_iter()
        .filter(|a| {
            a.node_id
                .strip_prefix("node_")
                .and_then(|i| i.parse::<u32>().ok())
                .is_some_and(|i| visible.contains(i))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::graph_access::gate_for_graph_with;
    use std::sync::Arc;
    use visionclaw_domain::models::graph::GraphData;
    use visionclaw_domain::models::node::Node;

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
    fn anomalies_on_hidden_or_unparsable_nodes_are_dropped() {
        let nodes = (0..4)
            .map(|id| {
                let mut n = Node::new_with_id(format!("n{id}"), None);
                n.id = id;
                if id != 1 {
                    n.metadata.insert("public".into(), "true".into());
                }
                n
            })
            .collect();
        let g = Arc::new(GraphData {
            nodes,
            ..GraphData::default()
        });
        let gate = gate_for_graph_with(&g, None, true).unwrap();
        let out = gate_anomalies(
            vec![anomaly("node_1"), anomaly("node_2"), anomaly("agent-x")],
            gate.slots(),
        );
        let ids: Vec<&str> = out.iter().map(|a| a.node_id.as_str()).collect();
        assert_eq!(ids, vec!["node_2"]);
    }
}
