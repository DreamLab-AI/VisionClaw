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
    caller_view_for(graph, caller.0.as_ref().map(|u| u.pubkey.as_str()))
}

/// [`caller_view`] for a caller known only by pubkey (`None` is anonymous).
pub fn caller_view_for(graph: &Arc<GraphData>, pubkey: Option<&str>) -> Arc<GraphData> {
    visionclaw_domain::utils::visibility_filter::visible_subgraph(
        graph,
        pubkey,
        visionclaw_domain::utils::visibility_filter::filter_enabled(),
    )
}

/// GPU analysers answer in positions of `graph.nodes`, which equal node ids only while the graph
/// is compact (a `remove_node` leaves a gap until the next reload). A position counts as visible
/// only when its node is in the caller's `view` AND its id is its position, so any drift fails
/// closed rather than letting a hidden node through under a visible node's number.
pub fn visible_positions(graph: &GraphData, view: &GraphData) -> HashSet<u32> {
    let seen: HashSet<u32> = view.nodes.iter().map(|n| n.id).collect();
    graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| n.id as usize == *i && seen.contains(&n.id))
        .map(|(i, _)| i as u32)
        .collect()
}

/// Whether an analytics run over `graph` may be written to a stream every viewer reads
/// (the shared `node_sssp` map behind wire slot 28). Only when the anonymous view hides
/// nothing: then the run was computed over exactly what every viewer may see. Otherwise the
/// distances would carry hidden shortcuts and sources to other viewers. Fails closed.
pub fn stream_safe(graph: &Arc<GraphData>) -> bool {
    stream_safe_for(
        graph,
        visionclaw_domain::utils::visibility_filter::filter_enabled(),
    )
}

/// [`stream_safe`] with the visibility flag passed in, for tests.
pub fn stream_safe_for(graph: &Arc<GraphData>, filter_on: bool) -> bool {
    let anon =
        visionclaw_domain::utils::visibility_filter::visible_subgraph(graph, None, filter_on);
    Arc::ptr_eq(&anon, graph)
}

/// The caller's view and visible positions, for a route whose result comes from a whole-graph
/// actor. `Ok(None)` means nothing is hidden from this caller (filter off, or every node
/// visible), so the result passes through untouched and the graph is not fetched when the filter
/// is off. `Err` means the graph could not be read; the route must then answer with an error
/// rather than an unfiltered result (fail closed).
pub async fn caller_gate(
    supervisor: &Addr<GraphServiceSupervisor>,
    pubkey: Option<&str>,
) -> Result<Option<(Arc<GraphData>, HashSet<u32>)>, String> {
    if !visionclaw_domain::utils::visibility_filter::filter_enabled() {
        return Ok(None);
    }
    let graph = current_graph(supervisor).await?;
    let view = caller_view_for(&graph, pubkey);
    if Arc::ptr_eq(&graph, &view) {
        return Ok(None);
    }
    let visible = visible_positions(&graph, &view);
    Ok(Some((view, visible)))
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
mod stream_safe_tests {
    use super::*;
    use visionclaw_domain::models::node::Node;

    fn graph(private_one: bool) -> Arc<GraphData> {
        let node = |id: u32, public: bool| {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            n.metadata.insert("public".into(), public.to_string());
            n
        };
        Arc::new(GraphData {
            nodes: vec![node(0, true), node(1, !private_one)],
            ..GraphData::default()
        })
    }

    #[test]
    fn a_run_over_a_graph_with_a_hidden_node_is_never_streamed() {
        assert!(
            !stream_safe_for(&graph(true), true),
            "a graph with a node hidden from anonymous viewers must not reach node_sssp"
        );
    }

    #[test]
    fn an_all_public_graph_or_the_filter_off_may_stream() {
        assert!(stream_safe_for(&graph(false), true));
        assert!(stream_safe_for(&graph(true), false));
    }
}
