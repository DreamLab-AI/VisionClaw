// src/handlers/semantic_handler.rs
//! Semantic Analysis API Handlers
//!
//! HTTP handlers for semantic analysis endpoints using SemanticService.

use crate::{error_json, ok_json};
use actix_web::{web, HttpResponse, Result as ActixResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use actix::Addr;

use crate::application::semantic_service::{
    CentralityRequest, CommunityDetectionRequest, SemanticService, ShortestPathRequest,
};
use visionclaw_domain::ports::gpu_semantic_analyzer::{
    ClusteringAlgorithm, ImportanceAlgorithm, SemanticConstraintConfig,
};

#[derive(Debug, Deserialize)]
pub struct DetectCommunitiesRequest {
    pub algorithm: String,
    pub min_cluster_size: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct CommunitiesResponse {
    pub clusters: HashMap<u32, usize>,
    pub cluster_sizes: HashMap<usize, usize>,
    pub modularity: f32,
    pub computation_time_ms: f32,
}

#[derive(Debug, Deserialize)]
pub struct CentralityAnalysisRequest {
    pub algorithm: String,
    pub damping: Option<f32>,
    pub max_iterations: Option<usize>,
    pub top_k: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct CentralityResponse {
    pub scores: HashMap<u32, f32>,
    pub algorithm: String,
    pub top_nodes: Vec<(u32, f32)>,
}

#[derive(Debug, Deserialize)]
pub struct ShortestPathAnalysisRequest {
    pub source_node_id: u32,
    pub target_node_id: Option<u32>,
    pub include_path: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ShortestPathResponse {
    pub source_node: u32,
    pub distances: HashMap<u32, f32>,
    pub paths: HashMap<u32, Vec<u32>>,
    pub computation_time_ms: f32,
}

#[derive(Debug, Deserialize)]
pub struct GenerateConstraintsRequest {
    pub similarity_threshold: Option<f32>,
    pub enable_clustering: Option<bool>,
    pub enable_importance: Option<bool>,
    pub enable_topic: Option<bool>,
    pub max_constraints: Option<usize>,
}

pub async fn detect_communities(
    semantic_service: web::Data<Arc<SemanticService>>,
    graph_service: web::Data<Addr<GraphServiceSupervisor>>,
    req: web::Json<DetectCommunitiesRequest>,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };
    if let Err(e) = semantic_service.initialize(graph).await {
        return error_json!("Failed to initialize: {}", e);
    }

    let algorithm = match req.algorithm.as_str() {
        "louvain" => ClusteringAlgorithm::Louvain,
        "label_propagation" => ClusteringAlgorithm::LabelPropagation,
        "connected_components" => ClusteringAlgorithm::ConnectedComponents,
        "hierarchical" => ClusteringAlgorithm::HierarchicalClustering {
            min_cluster_size: req.min_cluster_size.unwrap_or(5),
        },
        _ => ClusteringAlgorithm::Louvain,
    };

    let request = CommunityDetectionRequest {
        algorithm,
        min_cluster_size: req.min_cluster_size,
    };

    match semantic_service.detect_communities(request).await {
        Ok(result) => ok_json!(CommunitiesResponse {
            clusters: result.clusters,
            cluster_sizes: result.cluster_sizes,
            modularity: result.modularity,
            computation_time_ms: result.computation_time_ms,
        }),
        Err(e) => error_json!("Failed to detect communities: {}", e),
    }
}

pub async fn compute_centrality(
    semantic_service: web::Data<Arc<SemanticService>>,
    graph_service: web::Data<Addr<GraphServiceSupervisor>>,
    req: web::Json<CentralityAnalysisRequest>,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };
    if let Err(e) = semantic_service.initialize(graph).await {
        return error_json!("Failed to initialize: {}", e);
    }

    let algorithm = match req.algorithm.as_str() {
        "pagerank" => ImportanceAlgorithm::PageRank {
            damping: req.damping.unwrap_or(0.85).clamp(0.0, 1.0),
            max_iterations: bounded(req.max_iterations, 100, MAX_PAGERANK_ITERATIONS),
        },
        "betweenness" => ImportanceAlgorithm::Betweenness,
        "closeness" => ImportanceAlgorithm::Closeness,
        "eigenvector" => ImportanceAlgorithm::Eigenvector,
        "degree" => ImportanceAlgorithm::Degree,
        _ => ImportanceAlgorithm::PageRank {
            damping: 0.85,
            max_iterations: 100,
        },
    };

    let request = CentralityRequest {
        algorithm: algorithm.clone(),
        top_k: req.top_k.map(|k| k.min(MAX_TOP_K)),
    };

    match semantic_service.compute_centrality(request).await {
        Ok(scores) => {
            let mut top_nodes: Vec<_> = scores.iter().map(|(&id, &score)| (id, score)).collect();
            top_nodes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            if let Some(k) = req.top_k {
                top_nodes.truncate(k);
            } else {
                top_nodes.truncate(10);
            }

            ok_json!(CentralityResponse {
                scores,
                algorithm: req.algorithm.clone(),
                top_nodes,
            })
        }
        Err(e) => error_json!("Failed to compute centrality: {}", e),
    }
}

pub async fn compute_shortest_path(
    semantic_service: web::Data<Arc<SemanticService>>,
    graph_service: web::Data<Addr<GraphServiceSupervisor>>,
    req: web::Json<ShortestPathAnalysisRequest>,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };
    if let Err(e) = semantic_service.initialize(graph).await {
        return error_json!("Failed to initialize: {}", e);
    }

    let request = ShortestPathRequest {
        source_node_id: req.source_node_id,
        target_node_id: req.target_node_id,
        include_path: req.include_path.unwrap_or(true),
    };

    match semantic_service.compute_shortest_paths(request).await {
        Ok(result) => ok_json!(ShortestPathResponse {
            source_node: result.source_node,
            distances: result.distances,
            paths: result.paths,
            computation_time_ms: result.computation_time_ms,
        }),
        Err(e) => error_json!("Failed to compute shortest paths: {}", e),
    }
}

pub async fn generate_constraints(
    semantic_service: web::Data<Arc<SemanticService>>,
    graph_service: web::Data<Addr<GraphServiceSupervisor>>,
    req: web::Json<GenerateConstraintsRequest>,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };
    if let Err(e) = semantic_service.initialize(graph).await {
        return error_json!("Failed to initialize: {}", e);
    }

    let config = SemanticConstraintConfig {
        similarity_threshold: req.similarity_threshold.unwrap_or(0.7),
        enable_clustering_constraints: req.enable_clustering.unwrap_or(true),
        enable_importance_constraints: req.enable_importance.unwrap_or(true),
        enable_topic_constraints: req.enable_topic.unwrap_or(false),
        max_constraints: bounded(req.max_constraints, 1000, MAX_CONSTRAINTS),
    };

    match semantic_service.generate_semantic_constraints(config).await {
        Ok(constraints) => ok_json!(serde_json::json!({
            "constraint_count": constraints.constraints.len(),
            "status": "generated"
        })),
        Err(e) => error_json!("Failed to generate constraints: {}", e),
    }
}

pub async fn get_statistics(
    semantic_service: web::Data<Arc<SemanticService>>,
) -> ActixResult<HttpResponse> {
    match semantic_service.get_statistics().await {
        Ok(stats) => ok_json!(serde_json::json!({
            "total_analyses": stats.total_analyses,
            "average_clustering_time_ms": stats.average_clustering_time_ms,
            "average_pathfinding_time_ms": stats.average_pathfinding_time_ms,
            "cache_hit_rate": stats.cache_hit_rate,
            "gpu_memory_used_mb": stats.gpu_memory_used_mb,
        })),
        Err(e) => error_json!("Failed to get statistics: {}", e),
    }
}

pub async fn invalidate_cache(
    semantic_service: web::Data<Arc<SemanticService>>,
) -> ActixResult<HttpResponse> {
    match semantic_service.invalidate_cache().await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "invalidated"
        })),
        Err(e) => error_json!("Failed to invalidate cache: {}", e),
    }
}

/// Upper bounds on caller-supplied work. These routes run whole-graph
/// algorithms under the semantic adapter's write lock, so an unbounded value
/// lets one request stall every other caller (2026-10-18 security review).
pub const MAX_PAGERANK_ITERATIONS: usize = 1_000;
/// Most ranked nodes one centrality response returns.
pub const MAX_TOP_K: usize = 1_000;
/// Most constraints one generation request may produce.
pub const MAX_CONSTRAINTS: usize = 10_000;
/// Requests per minute per client across `/semantic`.
pub const SEMANTIC_REQUESTS_PER_MINUTE: usize = 30;

/// One limiter for every worker: `configure` runs once per worker, and a
/// fresh limiter there would give each worker its own budget. Keyed on the
/// verified pubkey `RbacGate` places on the request, since the forwarding
/// headers the IP fallback reads are client-controlled; an unauthenticated
/// read falls back to the IP.
fn semantic_limiter() -> crate::middleware::RateLimit {
    static LIMITER: std::sync::OnceLock<crate::middleware::RateLimit> = std::sync::OnceLock::new();
    LIMITER
        .get_or_init(|| {
            crate::middleware::RateLimit::new(
                crate::middleware::RateLimitConfig::new(
                    SEMANTIC_REQUESTS_PER_MINUTE,
                    std::time::Duration::from_secs(60),
                )
                .with_user_id(),
            )
        })
        .clone()
}

/// `value` or `default`, never above `max`.
pub fn bounded(value: Option<usize>, default: usize, max: usize) -> usize {
    value.unwrap_or(default).min(max)
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/semantic")
            .wrap(semantic_limiter())
            .route("/communities", web::post().to(detect_communities))
            .route("/centrality", web::post().to(compute_centrality))
            .route("/shortest-path", web::post().to(compute_shortest_path))
            .route(
                "/constraints/generate",
                web::post().to(generate_constraints),
            )
            .route("/statistics", web::get().to(get_statistics))
            .route("/cache/invalidate", web::post().to(invalidate_cache)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_supplied_work_is_capped() {
        assert_eq!(bounded(None, 100, MAX_PAGERANK_ITERATIONS), 100);
        assert_eq!(bounded(Some(5), 100, MAX_PAGERANK_ITERATIONS), 5);
        assert_eq!(
            bounded(Some(usize::MAX), 100, MAX_PAGERANK_ITERATIONS),
            MAX_PAGERANK_ITERATIONS
        );
        assert_eq!(
            bounded(Some(usize::MAX), 1000, MAX_CONSTRAINTS),
            MAX_CONSTRAINTS
        );
    }
}
