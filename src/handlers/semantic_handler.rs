// src/handlers/semantic_handler.rs
//! Semantic Analysis API Handlers
//!
//! HTTP handlers for semantic analysis endpoints using SemanticService.

use crate::{error_json, ok_json};
use actix_web::{web, HttpResponse, Result as ActixResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use visionclaw_domain::models::graph::GraphData;

use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use actix::Addr;

use crate::application::semantic_service::{
    CentralityRequest, CommunityDetectionRequest, SemanticService, ShortestPathRequest,
};
use visionclaw_domain::ports::gpu_semantic_analyzer::{
    ClusteringAlgorithm, GpuSemanticAnalyzerError, ImportanceAlgorithm, SemanticConstraintConfig,
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
    caller: crate::settings::OptionalAuth,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };
    // ADR-060: a caller whose view hides nodes gets communities, numbering and modularity
    // computed over that view (the one CPU path the analytics routes share), never a filtered
    // whole-graph run whose modularity and grouping hidden nodes shaped.
    if let Some(gate) = caller_gate(&graph, &caller) {
        return ok_json!(visible_communities_response(&gate));
    }
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
    caller: crate::settings::OptionalAuth,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };
    let gate = caller_gate(&graph, &caller);
    let visible = gate.as_ref().map(|g| g.slots().as_set());
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

    let top_k = req.top_k.map(|k| k.min(MAX_TOP_K));
    // Under the gate the service must not truncate first: a top-k cut before the filter would
    // come back short and show how many hidden nodes ranked above the visible ones.
    let request = CentralityRequest {
        algorithm: algorithm.clone(),
        top_k: if visible.is_some() { None } else { top_k },
    };

    match semantic_service.compute_centrality(request).await {
        Ok(scores) => {
            let (scores, top_nodes) = gate_centrality(scores, visible, req.top_k);

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
    caller: crate::settings::OptionalAuth,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };
    let include_path = req.include_path.unwrap_or(true);
    // ADR-060: a caller whose view hides nodes is answered over that view by the one CPU
    // Dijkstra, so a visible target reachable over visible edges keeps its true (visible)
    // distance instead of vanishing because the whole-graph run happened to route it through a
    // hidden node. A source the caller cannot see answers exactly as a node id that does not
    // exist.
    if let Some(gate) = caller_gate(&graph, &caller) {
        return match visible_shortest_path(&gate, req.source_node_id, include_path) {
            Some(response) => ok_json!(response),
            None => {
                let e = source_out_of_range(req.source_node_id);
                error_json!("Failed to compute shortest paths: {}", e)
            }
        };
    }
    if let Err(e) = semantic_service.initialize(graph).await {
        return error_json!("Failed to initialize: {}", e);
    }
    let request = ShortestPathRequest {
        source_node_id: req.source_node_id,
        target_node_id: req.target_node_id,
        include_path,
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

/// The caller's gate over `graph` (`graph_access`), or `None` when nothing is hidden from them,
/// in which case responses pass through unchanged.
fn caller_gate(
    graph: &Arc<GraphData>,
    caller: &crate::settings::OptionalAuth,
) -> Option<crate::handlers::graph_access::CallerGate> {
    crate::handlers::graph_access::gate_for_graph(
        graph,
        crate::handlers::graph_access::caller_pubkey(caller),
    )
}

/// The error the analyser gives for a source id past the end of the graph.
fn source_out_of_range(id: u32) -> GpuSemanticAnalyzerError {
    GpuSemanticAnalyzerError::InvalidGraph(format!("Source node {} out of range", id))
}

/// Communities of the caller's view: node id → dense community number, sizes and modularity
/// all computed over the view.
fn visible_communities_response(
    gate: &crate::handlers::graph_access::CallerGate,
) -> CommunitiesResponse {
    let start = std::time::Instant::now();
    let (communities, modularity, _, _) =
        crate::handlers::api_handler::analytics::visible_analytics::visible_communities(gate, 100);
    let mut clusters = HashMap::new();
    let mut cluster_sizes = HashMap::new();
    for (k, c) in communities.iter().enumerate() {
        cluster_sizes.insert(k, c.nodes.len());
        for &id in &c.nodes {
            clusters.insert(id, k);
        }
    }
    CommunitiesResponse {
        clusters,
        cluster_sizes,
        modularity,
        computation_time_ms: start.elapsed().as_secs_f32() * 1000.0,
    }
}

/// Centrality scores for the visible positions, cut to `top_k` after the filter, and the
/// ranked `top_nodes` list (`top_k`, default 10). The analyser answers per GPU position, so the
/// filter is by position ([`crate::handlers::graph_access::VisibleSlots`]).
fn gate_centrality(
    scores: HashMap<u32, f32>,
    visible: Option<&HashSet<u32>>,
    top_k: Option<usize>,
) -> (HashMap<u32, f32>, Vec<(u32, f32)>) {
    let mut ranked: Vec<(u32, f32)> = scores
        .into_iter()
        .filter(|(id, _)| visible.is_none_or(|v| v.contains(id)))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(k) = top_k {
        ranked.truncate(k.min(MAX_TOP_K));
    }
    let scores: HashMap<u32, f32> = ranked.iter().copied().collect();
    ranked.truncate(top_k.unwrap_or(10));
    (scores, ranked)
}

/// Shortest paths from GPU position `source` over the caller's view, keyed by node id. `None`
/// when that position is not visible to the caller. Paths are included when asked for.
fn visible_shortest_path(
    gate: &crate::handlers::graph_access::CallerGate,
    source: u32,
    include_path: bool,
) -> Option<ShortestPathResponse> {
    let start = std::time::Instant::now();
    let id = gate.id_at_slot(source as usize)?;
    let sp = crate::services::pathfinding::single_source(gate.view(), id)?;
    let paths = if include_path {
        sp.distances
            .keys()
            .filter(|&&t| t != id)
            .map(|&t| (t, sp.path_to(t)))
            .collect()
    } else {
        HashMap::new()
    };
    Some(ShortestPathResponse {
        source_node: id,
        distances: sp.distances.iter().map(|(&k, &v)| (k, v)).collect(),
        paths,
        computation_time_ms: start.elapsed().as_secs_f32() * 1000.0,
    })
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

/// Response gating for the semantic routes: a gated caller is answered over their view.
#[cfg(test)]
mod response_gate_tests {
    use super::*;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::node::Node;

    /// Public 0, 1, 2, 4; node 3 has no `public` flag, so it is hidden. Edges: 0–1 (1),
    /// 1–2 (1), 0–3 (1), 3–4 (1), and a long visible way to 4, 2–4 (5).
    fn gate() -> crate::handlers::graph_access::CallerGate {
        let nodes = (0..5)
            .map(|id| {
                let mut n = Node::new_with_id(format!("n{id}"), None);
                n.id = id;
                if id != 3 {
                    n.metadata.insert("public".into(), "true".into());
                }
                n
            })
            .collect();
        let g = Arc::new(GraphData {
            nodes,
            edges: vec![
                Edge::new(0, 1, 1.0),
                Edge::new(1, 2, 1.0),
                Edge::new(0, 3, 1.0),
                Edge::new(3, 4, 1.0),
                Edge::new(2, 4, 5.0),
            ],
            ..GraphData::default()
        });
        crate::handlers::graph_access::gate_for_graph_with(&g, None, true).unwrap()
    }

    #[test]
    fn a_shortest_path_never_names_or_routes_through_a_hidden_node() {
        let out = visible_shortest_path(&gate(), 0, true).unwrap();
        let mut targets: Vec<u32> = out.distances.keys().copied().collect();
        targets.sort_unstable();
        assert_eq!(targets, vec![0, 1, 2, 4]);
        assert!(out.paths.values().flatten().all(|id| *id != 3));
    }

    /// Review M2: the old filter dropped 4 because the whole-graph run reached it through hidden
    /// 3 (distance 2), which told the caller a hidden bridge exists. Over the view, 4 is
    /// reachable the long visible way and keeps that distance.
    #[test]
    fn a_visible_target_behind_a_hidden_shortcut_keeps_its_visible_distance() {
        let out = visible_shortest_path(&gate(), 0, true).unwrap();
        assert_eq!(out.distances[&4], 7.0);
        assert_eq!(out.paths[&4], vec![0, 1, 2, 4]);
    }

    #[test]
    fn distances_only_omits_paths() {
        let out = visible_shortest_path(&gate(), 0, false).unwrap();
        assert_eq!(out.distances.len(), 4);
        assert!(out.paths.is_empty(), "paths were not asked for");
    }

    /// A hidden source and a source past the end both answer `None`, which the route turns
    /// into the one out-of-range error.
    #[test]
    fn a_hidden_source_reads_as_a_missing_node() {
        assert!(visible_shortest_path(&gate(), 3, true).is_none());
        assert!(visible_shortest_path(&gate(), 99, true).is_none());
        assert_eq!(
            source_out_of_range(3).to_string(),
            GpuSemanticAnalyzerError::InvalidGraph("Source node 3 out of range".into()).to_string()
        );
    }

    /// Review item (VC-25 Open): modularity used to pass through from the whole-graph run. A
    /// gated caller's modularity is that of their view's partition, and the hidden node is in
    /// no community.
    #[test]
    fn gated_communities_and_modularity_are_the_views() {
        let g = gate();
        let out = visible_communities_response(&g);
        assert!(!out.clusters.contains_key(&3));
        assert_eq!(out.clusters.len(), 4);
        let total: usize = out.cluster_sizes.values().sum();
        assert_eq!(total, 4, "sizes count visible members only");
        let assignment: std::collections::BTreeMap<u32, u32> = out
            .clusters
            .iter()
            .map(|(&id, &c)| (id, c as u32))
            .collect();
        let q = crate::handlers::api_handler::analytics::visible_analytics::modularity(
            g.view(),
            &assignment,
        );
        assert!((out.modularity - q).abs() < 1e-6);
        let mut numbers: Vec<usize> = out.cluster_sizes.keys().copied().collect();
        numbers.sort_unstable();
        assert_eq!(
            numbers,
            (0..numbers.len()).collect::<Vec<_>>(),
            "dense numbering"
        );
    }

    #[test]
    fn centrality_filters_before_the_top_k_cut() {
        let scores: HashMap<u32, f32> = [(3, 0.9), (0, 0.5), (1, 0.4), (2, 0.3), (4, 0.2)]
            .into_iter()
            .collect();
        let g = gate();
        let (s, top) = gate_centrality(scores, Some(g.slots().as_set()), Some(2));
        assert!(!s.contains_key(&3));
        assert_eq!(
            s.len(),
            2,
            "the cut is taken over visible nodes, so it is not short"
        );
        assert_eq!(
            top.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }
}
