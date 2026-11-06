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
    ClusteringAlgorithm, GpuSemanticAnalyzerError, ImportanceAlgorithm, PathfindingResult,
    SemanticConstraintConfig,
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
    let visible = visible_ids(&graph, &caller);
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
        Ok(result) => {
            let (clusters, cluster_sizes) =
                gate_communities(result.clusters, result.cluster_sizes, visible.as_ref());
            ok_json!(CommunitiesResponse {
                clusters,
                cluster_sizes,
                modularity: result.modularity,
                computation_time_ms: result.computation_time_ms,
            })
        }
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
    let visible = visible_ids(&graph, &caller);
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
            let (scores, top_nodes) = gate_centrality(scores, visible.as_ref(), req.top_k);

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
    let visible = visible_ids(&graph, &caller);
    if let Err(e) = semantic_service.initialize(graph).await {
        return error_json!("Failed to initialize: {}", e);
    }

    let include_path = req.include_path.unwrap_or(true);
    // A source the caller cannot see answers exactly as a node id that does not exist.
    if visible
        .as_ref()
        .is_some_and(|v| !v.contains(&req.source_node_id))
    {
        let e = source_out_of_range(req.source_node_id);
        return error_json!("Failed to compute shortest paths: {}", e);
    }
    let request = ShortestPathRequest {
        source_node_id: req.source_node_id,
        target_node_id: req.target_node_id,
        // Under the gate the paths are always built: they are what proves a distance was reached
        // through visible nodes only. They are stripped again below when not asked for.
        include_path: include_path || visible.is_some(),
    };

    match semantic_service.compute_shortest_paths(request).await {
        Ok(result) => ok_json!(gate_shortest_path(result, visible.as_ref(), include_path)),
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

/// The node ids the caller may see, or `None` when the gate removes nothing (filter off, or no
/// private node hidden from this caller), in which case responses pass through unchanged. The set
/// comes from the one visibility policy (`graph_access::caller_view`).
fn visible_ids(
    graph: &Arc<GraphData>,
    caller: &crate::settings::OptionalAuth,
) -> Option<HashSet<u32>> {
    let view = crate::handlers::graph_access::caller_view(graph, caller);
    if Arc::ptr_eq(&view, graph) {
        None
    } else {
        Some(visible_positions(graph, &view))
    }
}

/// The analyser answers in positions of `graph.nodes`, which equal node ids only while the graph
/// is compact (a `remove_node` leaves a gap until the next reload). A position counts as visible
/// only when its node is in the caller's view AND its id is its position, so any drift fails
/// closed rather than letting a hidden node through under a visible node's number.
fn visible_positions(graph: &GraphData, view: &GraphData) -> HashSet<u32> {
    let seen: HashSet<u32> = view.nodes.iter().map(|n| n.id).collect();
    graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| n.id as usize == *i && seen.contains(&n.id))
        .map(|(i, _)| i as u32)
        .collect()
}

/// The error the analyser gives for a source id past the end of the graph.
fn source_out_of_range(id: u32) -> GpuSemanticAnalyzerError {
    GpuSemanticAnalyzerError::InvalidGraph(format!("Source node {} out of range", id))
}

/// Community membership for the visible nodes only; sizes are recounted from what is left, so a
/// size never counts a hidden member and a community with no visible member disappears.
fn gate_communities(
    clusters: HashMap<u32, usize>,
    cluster_sizes: HashMap<usize, usize>,
    visible: Option<&HashSet<u32>>,
) -> (HashMap<u32, usize>, HashMap<usize, usize>) {
    let Some(visible) = visible else {
        return (clusters, cluster_sizes);
    };
    let clusters: HashMap<u32, usize> = clusters
        .into_iter()
        .filter(|(id, _)| visible.contains(id))
        .collect();
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for cluster in clusters.values() {
        *sizes.entry(*cluster).or_default() += 1;
    }
    (clusters, sizes)
}

/// Centrality scores for the visible nodes, cut to `top_k` after the filter, and the ranked
/// `top_nodes` list (`top_k`, default 10).
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

/// The shortest-path response for the caller. A target is kept only when it is visible and its
/// whole path is: a distance reached through a hidden node would reveal that node. The source
/// itself (distance 0, no path) is kept. Paths are dropped when `include_path` is false.
fn gate_shortest_path(
    result: PathfindingResult,
    visible: Option<&HashSet<u32>>,
    include_path: bool,
) -> ShortestPathResponse {
    let PathfindingResult {
        source_node,
        mut distances,
        mut paths,
        computation_time_ms,
    } = result;
    if let Some(visible) = visible {
        // A reconstruction that failed part-way returns a path that does not reach the source;
        // the distance still counts the missing hops, so such a target is dropped too.
        paths.retain(|target, path| {
            path.first() == Some(&source_node)
                && path.last() == Some(target)
                && path.iter().all(|n| visible.contains(n))
        });
        distances.retain(|target, _| *target == source_node || paths.contains_key(target));
    }
    if !include_path {
        paths.clear();
    }
    ShortestPathResponse {
        source_node,
        distances,
        paths,
        computation_time_ms,
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

/// Response gating for the semantic routes: no private node id leaves, even though the shared
/// analyser keeps the full graph.
#[cfg(test)]
mod response_gate_tests {
    use super::*;

    /// Public nodes 0, 1, 2, 4; node 3 is private to someone else.
    fn visible() -> HashSet<u32> {
        [0, 1, 2, 4].into_iter().collect()
    }

    /// From source 0: 1 directly, 2 via 1, 4 only through the hidden 3, 3 itself.
    fn sssp() -> PathfindingResult {
        PathfindingResult {
            source_node: 0,
            distances: [(0, 0.0), (1, 1.0), (2, 2.0), (3, 1.0), (4, 2.0)]
                .into_iter()
                .collect(),
            paths: [
                (1, vec![0, 1]),
                (2, vec![0, 1, 2]),
                (3, vec![0, 3]),
                (4, vec![0, 3, 4]),
            ]
            .into_iter()
            .collect(),
            computation_time_ms: 1.0,
        }
    }

    #[test]
    fn a_shortest_path_never_names_or_routes_through_a_hidden_node() {
        let out = gate_shortest_path(sssp(), Some(&visible()), true);
        let mut targets: Vec<u32> = out.distances.keys().copied().collect();
        targets.sort_unstable();
        // 3 is hidden; 4 is visible but reached only through 3, so its distance would reveal 3.
        assert_eq!(targets, vec![0, 1, 2]);
        let mut with_paths: Vec<u32> = out.paths.keys().copied().collect();
        with_paths.sort_unstable();
        assert_eq!(with_paths, vec![1, 2]);
        assert!(out.paths.values().flatten().all(|id| *id != 3));
    }

    #[test]
    fn distances_only_are_still_checked_against_the_path() {
        let out = gate_shortest_path(sssp(), Some(&visible()), false);
        let mut targets: Vec<u32> = out.distances.keys().copied().collect();
        targets.sort_unstable();
        assert_eq!(targets, vec![0, 1, 2]);
        assert!(out.paths.is_empty(), "paths were not asked for");
    }

    #[test]
    fn a_partial_path_does_not_vouch_for_its_distance() {
        let mut r = sssp();
        // Reconstruction stopped at 2: the visible tail hides how 4 was really reached.
        r.paths.insert(4, vec![2, 4]);
        let out = gate_shortest_path(r, Some(&visible()), true);
        assert!(!out.distances.contains_key(&4));
        assert!(!out.paths.contains_key(&4));
    }

    #[test]
    fn a_position_whose_node_id_drifted_counts_as_hidden() {
        use visionclaw_domain::models::node::Node;
        // `new_with_id` treats id 0 as "allocate one", so the id is set explicitly.
        let node = |id: u32| {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            n
        };
        // Position 1 holds node id 2 (node 1 was removed); position 2 holds node id 3.
        let graph = GraphData {
            nodes: vec![node(0), node(2), node(3)],
            ..GraphData::default()
        };
        let view = GraphData {
            nodes: vec![node(0), node(2)],
            ..GraphData::default()
        };
        let v = visible_positions(&graph, &view);
        assert_eq!(v, [0].into_iter().collect::<HashSet<u32>>());
    }

    #[test]
    fn with_nothing_hidden_the_shortest_path_passes_through() {
        let out = gate_shortest_path(sssp(), None, true);
        assert_eq!(out.distances.len(), 5);
        assert_eq!(out.paths.len(), 4);
    }

    #[test]
    fn a_hidden_source_reads_as_a_missing_node() {
        assert_eq!(
            source_out_of_range(3).to_string(),
            GpuSemanticAnalyzerError::InvalidGraph("Source node 3 out of range".into()).to_string()
        );
    }

    #[test]
    fn communities_drop_hidden_members_and_recount() {
        let clusters: HashMap<u32, usize> = [(0, 7), (1, 7), (3, 7), (2, 8), (4, 9)]
            .into_iter()
            .collect();
        let sizes: HashMap<usize, usize> = [(7, 3), (8, 1), (9, 1)].into_iter().collect();
        let mut only_hidden = clusters.clone();
        only_hidden.insert(3, 5);
        let mut only_hidden_sizes = sizes.clone();
        *only_hidden_sizes.get_mut(&7).unwrap() -= 1;
        only_hidden_sizes.insert(5, 1);

        let (c, s) = gate_communities(clusters, sizes, Some(&visible()));
        assert!(!c.contains_key(&3));
        assert_eq!(s.get(&7), Some(&2), "the hidden member is not counted");

        let (c, s) = gate_communities(only_hidden, only_hidden_sizes, Some(&visible()));
        assert!(!c.contains_key(&3));
        assert!(
            !s.contains_key(&5),
            "a community of hidden nodes only disappears"
        );
    }

    #[test]
    fn centrality_filters_before_the_top_k_cut() {
        let scores: HashMap<u32, f32> = [(3, 0.9), (0, 0.5), (1, 0.4), (2, 0.3), (4, 0.2)]
            .into_iter()
            .collect();
        let (s, top) = gate_centrality(scores, Some(&visible()), Some(2));
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
