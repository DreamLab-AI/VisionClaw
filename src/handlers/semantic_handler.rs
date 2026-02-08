// src/handlers/semantic_handler.rs
//! Semantic Analysis API Handlers
//!
//! HTTP handlers for semantic analysis endpoints using SemanticService.

use actix_web::{web, HttpResponse, Result as ActixResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{ok_json, error_json};

use crate::application::semantic_service::{
    CentralityRequest, CommunityDetectionRequest, SemanticService, ShortestPathRequest,
};
use crate::models::graph::GraphData;
use crate::ports::gpu_semantic_analyzer::{
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
    graph_data: web::Data<Arc<RwLock<GraphData>>>,
    req: web::Json<DetectCommunitiesRequest>,
) -> ActixResult<HttpResponse> {
    
    let graph = graph_data.read().await.clone();
    if let Err(e) = semantic_service.initialize(Arc::new(graph)).await {
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
    graph_data: web::Data<Arc<RwLock<GraphData>>>,
    req: web::Json<CentralityAnalysisRequest>,
) -> ActixResult<HttpResponse> {
    
    let graph = graph_data.read().await.clone();
    if let Err(e) = semantic_service.initialize(Arc::new(graph)).await {
        return error_json!("Failed to initialize: {}", e);
    }


    let algorithm = match req.algorithm.as_str() {
        "pagerank" => ImportanceAlgorithm::PageRank {
            damping: req.damping.unwrap_or(0.85),
            max_iterations: req.max_iterations.unwrap_or(100),
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
        top_k: req.top_k,
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
    graph_data: web::Data<Arc<RwLock<GraphData>>>,
    req: web::Json<ShortestPathAnalysisRequest>,
) -> ActixResult<HttpResponse> {
    
    let graph = graph_data.read().await.clone();
    if let Err(e) = semantic_service.initialize(Arc::new(graph)).await {
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
    graph_data: web::Data<Arc<RwLock<GraphData>>>,
    req: web::Json<GenerateConstraintsRequest>,
) -> ActixResult<HttpResponse> {
    
    let graph = graph_data.read().await.clone();
    if let Err(e) = semantic_service.initialize(Arc::new(graph)).await {
        return error_json!("Failed to initialize: {}", e);
    }

    let config = SemanticConstraintConfig {
        similarity_threshold: req.similarity_threshold.unwrap_or(0.7),
        enable_clustering_constraints: req.enable_clustering.unwrap_or(true),
        enable_importance_constraints: req.enable_importance.unwrap_or(true),
        enable_topic_constraints: req.enable_topic.unwrap_or(false),
        max_constraints: req.max_constraints.unwrap_or(1000),
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

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/semantic")
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
