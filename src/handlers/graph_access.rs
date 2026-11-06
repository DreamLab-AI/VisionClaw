//! The current graph for request handlers, read through the registered
//! [`GraphServiceSupervisor`] address.
//!
//! Several handlers used to extract `web::Data<Addr<GraphStateActor>>` or
//! `web::Data<Arc<RwLock<GraphData>>>`, neither of which `main.rs` registers,
//! so Actix refused every request on those routes with a 500 before the
//! handler ran (estate register ER-25). The supervisor is registered and
//! forwards `GetGraphData` to the graph state actor it owns.

use actix::Addr;
use std::collections::HashSet;
use std::sync::Arc;

use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use crate::actors::messages::GetGraphData;
use visionclaw_domain::models::graph::GraphData;

/// The part of `graph` the caller may see: the ADR-060 policy the `/wss` `initialGraphLoad`
/// applies (`visionclaw_domain::utils::visibility_filter::visible_subgraph`), under the same
/// `PUBKEY_VISIBILITY_FILTER` flag. Every handler that returns node identities passes its graph
/// through this first. `OptionalAuth` yields `None` for a bad or absent credential, so such a
/// caller reads as anonymous (fail closed).
pub fn caller_view(
    graph: &Arc<GraphData>,
    caller: &crate::settings::OptionalAuth,
) -> Arc<GraphData> {
    caller_view_for(graph, caller_pubkey(caller))
}

/// The caller's pubkey, `None` for an anonymous (or badly credentialed) caller.
pub fn caller_pubkey(caller: &crate::settings::OptionalAuth) -> Option<&str> {
    caller.0.as_ref().map(|u| u.pubkey.as_str())
}

/// [`caller_view`] for a caller known only by pubkey (`None` is anonymous).
pub fn caller_view_for(graph: &Arc<GraphData>, pubkey: Option<&str>) -> Arc<GraphData> {
    visionclaw_domain::utils::visibility_filter::visible_subgraph(
        graph,
        pubkey,
        visionclaw_domain::utils::visibility_filter::filter_enabled(),
    )
}

/// Node ids (`Node::id`) the caller may see. Results keyed by node id (DBSCAN clusters, the
/// `/api/graph` reads, every CPU-over-view answer) are filtered with this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleIds(HashSet<u32>);

/// Positions in `graph.nodes` (the GPU's index space) the caller may see. Results the GPU
/// analysers return per position are filtered with this, never with [`VisibleIds`]: the two
/// agree only while the graph is compact, and mixing them is a compile error by design.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleSlots(HashSet<u32>);

impl VisibleIds {
    /// Whether node `id` is visible.
    pub fn contains(&self, id: u32) -> bool {
        self.0.contains(&id)
    }
    /// The ids as a set.
    pub fn as_set(&self) -> &HashSet<u32> {
        &self.0
    }
}

impl VisibleSlots {
    /// Whether position `slot` holds a visible node under its own id.
    pub fn contains(&self, slot: u32) -> bool {
        self.0.contains(&slot)
    }
    /// The positions as a set.
    pub fn as_set(&self) -> &HashSet<u32> {
        &self.0
    }
}

/// What one caller may see of one graph snapshot, for a caller whose view hides something.
/// The one ADR-060 "is anything hidden from this caller" decision: [`gate_for_graph`] (and its
/// async wrapper [`caller_gate`]) is the only constructor, and every gated analytics route,
/// whatever its key, takes its id or position set from here.
#[derive(Debug, Clone)]
pub struct CallerGate {
    graph: Arc<GraphData>,
    view: Arc<GraphData>,
    ids: VisibleIds,
    slots: VisibleSlots,
}

impl CallerGate {
    /// The whole graph the decision was taken over.
    pub fn graph(&self) -> &Arc<GraphData> {
        &self.graph
    }
    /// The caller's view of it ([`caller_view`]).
    pub fn view(&self) -> &Arc<GraphData> {
        &self.view
    }
    /// Visible node ids.
    pub fn ids(&self) -> &VisibleIds {
        &self.ids
    }
    /// Visible GPU positions. A position counts only when its node is visible AND its id equals
    /// its position, so any drift fails closed rather than letting a hidden node through under
    /// a visible node's number.
    pub fn slots(&self) -> &VisibleSlots {
        &self.slots
    }
    /// The node id at GPU position `slot`, when that position is visible (fail closed).
    pub fn id_at_slot(&self, slot: usize) -> Option<u32> {
        u32::try_from(slot)
            .ok()
            .filter(|s| self.slots.contains(*s))
            .and_then(|s| self.graph.nodes.get(s as usize).map(|n| n.id))
    }
}

/// The caller's gate over `graph`, under the `PUBKEY_VISIBILITY_FILTER` flag. `None` means
/// nothing is hidden from this caller (filter off, or every node visible): the whole-graph
/// result passes through untouched.
pub fn gate_for_graph(graph: &Arc<GraphData>, pubkey: Option<&str>) -> Option<CallerGate> {
    gate_for_graph_with(
        graph,
        pubkey,
        visionclaw_domain::utils::visibility_filter::filter_enabled(),
    )
}

/// [`gate_for_graph`] with the visibility flag passed in, for tests.
pub fn gate_for_graph_with(
    graph: &Arc<GraphData>,
    pubkey: Option<&str>,
    filter_on: bool,
) -> Option<CallerGate> {
    let view =
        visionclaw_domain::utils::visibility_filter::visible_subgraph(graph, pubkey, filter_on);
    if Arc::ptr_eq(&view, graph) {
        return None;
    }
    let ids: HashSet<u32> = view.nodes.iter().map(|n| n.id).collect();
    let slots = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| n.id as usize == *i && ids.contains(&n.id))
        .map(|(i, _)| i as u32)
        .collect();
    Some(CallerGate {
        graph: Arc::clone(graph),
        view,
        ids: VisibleIds(ids),
        slots: VisibleSlots(slots),
    })
}

/// [`gate_for_graph`] over the current graph. `Ok(None)` means nothing is hidden from this
/// caller, and the graph is not fetched at all when the filter is off. `Err` means the graph
/// could not be read; the route must then answer with an error rather than an unfiltered
/// result (fail closed).
pub async fn caller_gate(
    supervisor: &Addr<GraphServiceSupervisor>,
    pubkey: Option<&str>,
) -> Result<Option<CallerGate>, String> {
    if !visionclaw_domain::utils::visibility_filter::filter_enabled() {
        return Ok(None);
    }
    let graph = current_graph(supervisor).await?;
    Ok(gate_for_graph(&graph, pubkey))
}

/// The ontology class IRIs this caller may not see: the `owl_class_iri` of every node in the
/// whole graph that is not in the caller's view. The Oxigraph store still holds a formal class
/// whose page is private, so every ontology read surface drops these IRIs (and every edge and
/// axiom that names one) to make a hidden class read exactly as a class that was never stored.
pub fn hidden_class_iris(gate: &CallerGate) -> HashSet<String> {
    gate.graph
        .nodes
        .iter()
        .filter(|n| !gate.ids.contains(n.id))
        .filter_map(|n| n.owl_class_iri.clone())
        .collect()
}

/// [`hidden_class_iris`] for a caller over the current graph. Empty when nothing is hidden
/// from them (filter off, or full access). `Err` means the graph could not be read: the route
/// must then refuse rather than serve the unfiltered store (fail closed).
pub async fn caller_hidden_class_iris(
    supervisor: &Addr<GraphServiceSupervisor>,
    pubkey: Option<&str>,
) -> Result<HashSet<String>, String> {
    Ok(caller_gate(supervisor, pubkey)
        .await?
        .map(|gate| hidden_class_iris(&gate))
        .unwrap_or_default())
}

/// [`caller_hidden_class_iris`] for an ontology route: the [`HiddenClasses`] its reads pass
/// through. An unreadable graph answers 503 rather than the unfiltered store (fail closed).
///
/// [`HiddenClasses`]: crate::services::ontology_visibility::HiddenClasses
pub async fn caller_hidden_classes(
    supervisor: &Addr<GraphServiceSupervisor>,
    caller: &crate::settings::OptionalAuth,
) -> Result<crate::services::ontology_visibility::HiddenClasses, actix_web::Error> {
    caller_hidden_class_iris(supervisor, caller_pubkey(caller))
        .await
        .map(Into::into)
        .map_err(actix_web::error::ErrorServiceUnavailable)
}

/// Node, edge and metadata-row counts of what a caller may see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct VisibleCounts {
    /// Visible nodes.
    pub nodes: usize,
    /// Edges with both endpoints visible.
    pub edges: usize,
    /// Metadata rows of visible nodes.
    pub metadata: usize,
}

/// The counts a reply may state for a caller: those of their view, so a hidden node, its edges
/// and its metadata row count exactly as if they did not exist (ADR-060). Used by
/// `/api/files/refresh_graph` and the `/wss` `state_sync`.
pub fn visible_counts(
    graph: &Arc<GraphData>,
    pubkey: Option<&str>,
    filter_on: bool,
) -> VisibleCounts {
    let view =
        visionclaw_domain::utils::visibility_filter::visible_subgraph(graph, pubkey, filter_on);
    VisibleCounts {
        nodes: view.nodes.len(),
        edges: view.edges.len(),
        metadata: view.metadata.len(),
    }
}

/// Fetch the current graph, or a message saying why it is unavailable.
pub async fn current_graph(
    supervisor: &Addr<GraphServiceSupervisor>,
) -> Result<Arc<GraphData>, String> {
    match supervisor.send(GetGraphData).await {
        Ok(reply) => reply,
        Err(e) => Err(format!("graph service unavailable: {e}")),
    }
}

#[cfg(test)]
mod gate_keying_tests {
    use super::*;
    use visionclaw_domain::models::node::Node;

    /// A non-compact graph, as after a `remove_node`: ids 0, 2, 3 at positions 0, 1, 2.
    /// Node 3 is private to nobody we know, so it is hidden from an anonymous caller.
    fn non_compact() -> Arc<GraphData> {
        let node = |id: u32, public: bool| {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            n.metadata.insert("public".into(), public.to_string());
            n
        };
        Arc::new(GraphData {
            nodes: vec![node(0, true), node(2, true), node(3, false)],
            ..GraphData::default()
        })
    }

    #[test]
    fn ids_and_positions_are_distinct_sets_on_a_non_compact_graph() {
        let gate = gate_for_graph_with(&non_compact(), None, true).expect("node 3 is hidden");
        // By id: both visible nodes, wherever they sit.
        assert!(gate.ids().contains(0));
        assert!(gate.ids().contains(2));
        assert!(
            !gate.ids().contains(3),
            "the hidden node is never visible by id"
        );
        // By position: only position 0, whose id equals its position. Position 1 holds id 2,
        // so a GPU answer at position 1 cannot be trusted to name node 2 (fail closed), and
        // position 2 holds the hidden node 3.
        assert!(gate.slots().contains(0));
        assert!(!gate.slots().contains(1), "id/position drift fails closed");
        assert!(
            !gate.slots().contains(2),
            "the hidden node's position is never visible"
        );
        assert_eq!(gate.id_at_slot(0), Some(0));
        assert_eq!(gate.id_at_slot(1), None);
        assert_eq!(gate.id_at_slot(2), None);
        assert_eq!(gate.id_at_slot(usize::MAX), None);
    }

    #[test]
    fn nothing_hidden_or_filter_off_means_no_gate() {
        let g = non_compact();
        assert!(gate_for_graph_with(&g, None, false).is_none());
        let all_public = Arc::new(GraphData {
            nodes: g
                .nodes
                .iter()
                .cloned()
                .map(|mut n| {
                    n.metadata.insert("public".into(), "true".into());
                    n
                })
                .collect(),
            ..GraphData::default()
        });
        assert!(gate_for_graph_with(&all_public, None, true).is_none());
    }
}

#[cfg(test)]
mod visible_counts_tests {
    use super::*;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::metadata::Metadata;
    use visionclaw_domain::models::node::Node;

    /// Public 0 and 1 joined by an edge; when `with_hidden`, a hidden 2 with an edge to 0 and a
    /// metadata row of its own.
    fn graph(with_hidden: bool) -> Arc<GraphData> {
        let mut g = GraphData::default();
        let n = with_hidden.then_some(3).unwrap_or(2);
        for id in 0..n {
            let mut node = Node::new_with_id(format!("p{id}.md"), None);
            node.id = id;
            if id < 2 {
                node.metadata.insert("public".into(), "true".into());
            }
            g.metadata.insert(format!("p{id}.md"), Metadata::default());
            g.nodes.push(node);
        }
        g.edges.push(Edge::new(0, 1, 1.0));
        if with_hidden {
            g.edges.push(Edge::new(0, 2, 1.0));
        }
        Arc::new(g)
    }

    #[test]
    fn a_hidden_node_counts_exactly_as_one_that_does_not_exist() {
        assert_eq!(
            visible_counts(&graph(true), None, true),
            visible_counts(&graph(false), None, true)
        );
        assert_eq!(
            visible_counts(&graph(true), None, false),
            VisibleCounts {
                nodes: 3,
                edges: 2,
                metadata: 3
            },
            "the filter off counts everything"
        );
    }
}

#[cfg(test)]
mod hidden_class_tests {
    use super::*;
    use visionclaw_domain::models::node::Node;

    fn graph() -> Arc<GraphData> {
        let node = |id: u32, public: bool, iri: Option<&str>| {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            n.owl_class_iri = iri.map(str::to_string);
            n.metadata.insert("public".into(), public.to_string());
            n
        };
        Arc::new(GraphData {
            nodes: vec![
                node(0, true, Some("urn:c:Visible")),
                node(1, false, Some("urn:c:Hidden")),
                node(2, false, None),
            ],
            ..GraphData::default()
        })
    }

    #[test]
    fn the_deny_list_is_the_class_iris_of_nodes_outside_the_view() {
        let gate = gate_for_graph_with(&graph(), None, true).expect("two nodes are hidden");
        let hidden = hidden_class_iris(&gate);
        assert_eq!(
            hidden,
            HashSet::from(["urn:c:Hidden".to_string()]),
            "only hidden nodes' class IRIs; a hidden node without one adds nothing"
        );
    }

    #[test]
    fn full_access_has_no_gate_so_no_deny_list() {
        assert!(gate_for_graph_with(&graph(), None, false).is_none());
    }
}
