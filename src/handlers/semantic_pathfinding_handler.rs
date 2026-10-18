//! Semantic Pathfinding Handler - API endpoints for intelligent graph traversal

use actix_web::{web, Responder};
use log::{debug, info};
use serde::Deserialize;
use std::sync::Arc;

use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use crate::services::semantic_pathfinding_service::SemanticPathfindingService;
use crate::{error_json, ok_json};
use actix::Addr;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindPathRequest {
    pub start_id: u32,
    pub end_id: u32,
    pub query: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraversalRequest {
    pub start_id: u32,
    pub query: Option<String>,
    pub max_nodes: Option<usize>,
}

pub async fn find_semantic_path(
    pathfinding_service: web::Data<Arc<SemanticPathfindingService>>,
    graph_state_actor: web::Data<Addr<GraphServiceSupervisor>>,
    request: web::Json<FindPathRequest>,
) -> impl Responder {
    info!(
        "Finding semantic path from {} to {}",
        request.start_id, request.end_id
    );

    let graph_result = graph_state_actor
        .send(crate::actors::messages::GetGraphData)
        .await;

    match graph_result {
        Ok(Ok(graph_data)) => {
            match pathfinding_service.find_semantic_path(
                &graph_data,
                request.start_id,
                request.end_id,
                request.query.as_deref(),
            ) {
                Some(path) => ok_json!(path),
                None => error_json!("No path found", "Could not find path between nodes"),
            }
        }
        Ok(Err(e)) => error_json!("Graph error", e),
        Err(e) => error_json!("Actor error", e.to_string()),
    }
}

pub async fn query_traversal(
    pathfinding_service: web::Data<Arc<SemanticPathfindingService>>,
    graph_state_actor: web::Data<Addr<GraphServiceSupervisor>>,
    request: web::Json<TraversalRequest>,
) -> impl Responder {
    info!("Query traversal from {}", request.start_id);

    let graph_result = graph_state_actor
        .send(crate::actors::messages::GetGraphData)
        .await;

    match graph_result {
        Ok(Ok(graph_data)) => {
            if let Some(ref query) = request.query {
                let results = pathfinding_service.query_traversal(
                    &graph_data,
                    request.start_id,
                    query,
                    traversal_budget(request.max_nodes),
                );
                ok_json!(serde_json::json!({ "results": results }))
            } else {
                error_json!("Missing query", "Query parameter required")
            }
        }
        Ok(Err(e)) => error_json!("Graph error", e),
        Err(e) => error_json!("Actor error", e.to_string()),
    }
}

pub async fn chunk_traversal(
    pathfinding_service: web::Data<Arc<SemanticPathfindingService>>,
    graph_state_actor: web::Data<Addr<GraphServiceSupervisor>>,
    request: web::Json<TraversalRequest>,
) -> impl Responder {
    debug!("Chunk traversal from {}", request.start_id);

    let graph_result = graph_state_actor
        .send(crate::actors::messages::GetGraphData)
        .await;

    match graph_result {
        Ok(Ok(graph_data)) => {
            let results = pathfinding_service.chunk_traversal(
                &graph_data,
                request.start_id,
                traversal_budget(request.max_nodes),
            );
            ok_json!(serde_json::json!({ "results": results }))
        }
        Ok(Err(e)) => error_json!("Graph error", e),
        Err(e) => error_json!("Actor error", e.to_string()),
    }
}

/// Most nodes one traversal may visit; whole-graph work for one caller is
/// capped (2026-10-18 security review).
pub const MAX_TRAVERSAL_NODES: usize = 1_000;
/// Requests per minute per client across `/pathfinding`.
pub const PATHFINDING_REQUESTS_PER_MINUTE: usize = 30;

fn traversal_budget(requested: Option<usize>) -> usize {
    requested.unwrap_or(50).min(MAX_TRAVERSAL_NODES)
}

pub fn configure_pathfinding_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/pathfinding")
            .wrap(crate::middleware::RateLimit::per_minute(
                PATHFINDING_REQUESTS_PER_MINUTE,
            ))
            .route("/semantic-path", web::post().to(find_semantic_path))
            .route("/query-traversal", web::post().to(query_traversal))
            .route("/chunk-traversal", web::post().to(chunk_traversal)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_budget_defaults_and_caps() {
        assert_eq!(traversal_budget(None), 50);
        assert_eq!(traversal_budget(Some(10)), 10);
        assert_eq!(traversal_budget(Some(usize::MAX)), MAX_TRAVERSAL_NODES);
    }
}
