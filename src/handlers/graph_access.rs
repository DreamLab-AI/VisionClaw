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

/// Fetch the current graph, or a message saying why it is unavailable.
pub async fn current_graph(
    supervisor: &Addr<GraphServiceSupervisor>,
) -> Result<Arc<GraphData>, String> {
    match supervisor.send(GetGraphData).await {
        Ok(reply) => reply,
        Err(e) => Err(format!("graph service unavailable: {e}")),
    }
}
