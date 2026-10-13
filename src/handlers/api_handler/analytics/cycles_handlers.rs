//! Directed-cycle analysis endpoint.
//!
//! `GET /api/analytics/cycles[?edgeType=<type>|all]` reports the nodes that sit
//! on a directed cycle, using the same adjacency builder and Tarjan pass as the
//! CPU hierarchical layout engine (`layout::scc`), over the same subsumption
//! edges the GPU DAG ranker uses. It is computed on the CPU from the
//! authoritative graph, so the desktop client, the XR client and agents all see
//! one answer. Read-only and as public as `/api/graph/data`, which already
//! exposes the same topology.

use actix_web::{web, HttpResponse, Result};
use log::error;
use serde::{Deserialize, Serialize};

use crate::actors::messages::GetGraphData;
use crate::layout::scc::cycle_report;
use crate::models::edge::Edge;
use crate::{error_json, ok_json, AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CyclesQuery {
    /// Which edges to analyse. Absent: edges that assert class subsumption
    /// (`Edge::asserts_subsumption`, the same predicate the GPU DAG ranker feeds
    /// on), because a cycle only means something on a directed relation. The
    /// collapsed `hierarchical` label also carries equivalence and `sameAs`,
    /// and mutual links and `co_citation` edges are symmetric, so each would
    /// read as a two-node cycle. `all`: every edge. Anything else: that exact
    /// `edge_type`.
    pub edge_type: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CyclesResponse {
    pub node_count: usize,
    /// Edges that took part in the analysis (after the `edgeType` filter).
    pub edge_count: usize,
    /// The filter applied: `subsumption` (default), `all`, or the requested type.
    pub edge_filter: String,
    /// Node ids on a directed cycle, ascending.
    pub cyclic_node_ids: Vec<u32>,
    /// Multi-node strongly connected components, largest first.
    pub components: Vec<Vec<u32>>,
}

/// Edges `(source, target)` that match the `edgeType` selector.
fn select_edges(edges: &[Edge], wanted: Option<&str>) -> Vec<(u32, u32)> {
    edges
        .iter()
        .filter(|e| match wanted {
            None => e.asserts_subsumption(),
            Some("all") => true,
            Some(t) => e.edge_type.as_deref() == Some(t),
        })
        .map(|e| (e.source, e.target))
        .collect()
}

pub async fn get_cycles(
    app_state: web::Data<AppState>,
    query: web::Query<CyclesQuery>,
) -> Result<HttpResponse> {
    let graph = match app_state.graph_service_addr.send(GetGraphData).await {
        Ok(Ok(g)) => g,
        Ok(Err(e)) => {
            error!("cycles: graph unavailable: {}", e);
            return error_json!(format!("Graph unavailable: {}", e));
        }
        Err(e) => {
            error!("cycles: graph actor mailbox error: {}", e);
            return error_json!(format!("Graph actor unavailable: {}", e));
        }
    };

    let node_ids: Vec<u32> = graph.nodes.iter().map(|n| n.id).collect();
    let wanted = query.edge_type.as_deref();
    let edge_filter = wanted.unwrap_or("subsumption").to_string();
    let edges = select_edges(&graph.edges, wanted);

    let report = cycle_report(&node_ids, edges.iter().copied());
    ok_json!(CyclesResponse {
        node_count: node_ids.len(),
        edge_count: edges.len(),
        edge_filter,
        cyclic_node_ids: report.cyclic_node_ids,
        components: report.components,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: u32, t: u32, ty: &str) -> Edge {
        Edge::new(s, t, 1.0).with_edge_type(ty.to_string())
    }

    #[test]
    fn default_selector_keeps_only_edges_that_assert_subsumption() {
        let sub_iri = visionclaw_domain::models::edge::RDFS_SUBCLASS_OF_IRI.to_string();
        let edges = [
            typed(1, 2, "is_subclass_of"),
            typed(2, 1, "co_citation"),
            // `hierarchical` also carries equivalence / sameAs: only the
            // subClassOf-provenance one counts.
            typed(2, 3, "hierarchical").with_owl_property_iri(sub_iri),
            typed(3, 2, "hierarchical"),
            Edge::new(3, 1, 1.0),
        ];
        assert_eq!(select_edges(&edges, None), vec![(1, 2), (2, 3)]);
    }

    #[test]
    fn all_keeps_every_edge_and_a_named_type_is_exact() {
        let edges = [typed(1, 2, "explicit_link"), typed(2, 1, "co_citation")];
        assert_eq!(select_edges(&edges, Some("all")).len(), 2);
        assert_eq!(select_edges(&edges, Some("co_citation")), vec![(2, 1)]);
    }
}
