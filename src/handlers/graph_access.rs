//! The current graph for request handlers, read through the registered
//! [`GraphServiceSupervisor`] address.
//!
//! Several handlers used to extract `web::Data<Addr<GraphStateActor>>` or
//! `web::Data<Arc<RwLock<GraphData>>>`, neither of which `main.rs` registers,
//! so Actix refused every request on those routes with a 500 before the
//! handler ran (estate register ER-25). The supervisor is registered and
//! forwards `GetGraphData` to the graph state actor it owns.

use actix::Addr;
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
    visionclaw_domain::utils::visibility_filter::visible_subgraph(
        graph,
        caller.0.as_ref().map(|u| u.pubkey.as_str()),
        visionclaw_domain::utils::visibility_filter::filter_enabled(),
    )
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
