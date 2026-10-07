//! Analytics-domain messages: clustering, anomaly detection, community detection,
//! SSSP, PageRank, and related parameter/result types.
//!
//! Domain-safe parameter structs/enums and pure messages have been moved to
//! `visionclaw_actors::messages::analytics_messages`.
//! This file re-exports them and defines the webxr-internal types that cannot move.

// ---------------------------------------------------------------------------
// Re-export domain-safe types from the domain crate
// ---------------------------------------------------------------------------

pub use visionclaw_actors::messages::analytics_messages::{
    AnomalyDetectionMethod, AnomalyDetectionParams, AnomalyDetectionStats, AnomalyMethod,
    AnomalyParams, ClearPageRankCache, CommunityDetectionAlgorithm, CommunityDetectionParams,
    ComputeAllPairsShortestPaths, ComputeSSSP, DBSCANParams, DBSCANStats, ExportClusterAssignments,
    GetClusteringResults, GetClusteringStatus, KMeansParams, SetNodeAnalytics, SetNodeSSSP,
    SharedNodeSssp, StartGPUClustering,
};

// ---------------------------------------------------------------------------
// Webxr-internal types (cannot move to domain crate)
// ---------------------------------------------------------------------------

use actix::prelude::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// K-means clustering (result type refs webxr-internal gpu crate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KMeansResult {
    pub cluster_assignments: Vec<i32>,
    pub centroids: Vec<(f32, f32, f32)>,
    pub inertia: f32,
    pub iterations: u32,
    pub clusters: Vec<crate::handlers::api_handler::analytics::Cluster>,
    pub stats: crate::actors::gpu::clustering_actor::ClusteringStats,
    pub converged: bool,
    pub final_iteration: u32,
}

#[derive(Message)]
#[rtype(result = "Result<KMeansResult, String>")]
pub struct RunKMeans {
    pub params: KMeansParams,
}

// ---------------------------------------------------------------------------
// Anomaly detection (result type refs webxr-internal gpu crate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyResult {
    pub lof_scores: Option<Vec<f32>>,
    pub local_densities: Option<Vec<f32>>,
    pub zscore_values: Option<Vec<f32>>,
    pub anomaly_threshold: f32,
    pub num_anomalies: usize,
    pub anomalies: Vec<crate::actors::gpu::anomaly_detection_actor::AnomalyNode>,
    pub stats: AnomalyDetectionStats,
    pub method: AnomalyDetectionMethod,
    pub threshold: f32,
}

#[derive(Message)]
#[rtype(result = "Result<AnomalyResult, String>")]
pub struct RunAnomalyDetection {
    pub params: AnomalyParams,
}

// ---------------------------------------------------------------------------
// Community detection (result type refs webxr-internal gpu crate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityDetectionResult {
    pub node_labels: Vec<i32>,
    pub num_communities: usize,
    pub modularity: f32,
    pub iterations: u32,
    pub community_sizes: Vec<i32>,
    pub converged: bool,
    pub communities: Vec<crate::actors::gpu::clustering_actor::Community>,
    pub stats: crate::actors::gpu::clustering_actor::CommunityDetectionStats,
    pub algorithm: CommunityDetectionAlgorithm,
}

#[derive(Message)]
#[rtype(result = "Result<CommunityDetectionResult, String>")]
pub struct RunCommunityDetection {
    pub params: CommunityDetectionParams,
}

// ---------------------------------------------------------------------------
// DBSCAN clustering (result type refs webxr-internal handler crate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DBSCANResult {
    pub labels: Vec<i32>,
    pub num_clusters: usize,
    pub num_noise_points: usize,
    pub clusters: Vec<crate::handlers::api_handler::analytics::Cluster>,
    pub stats: DBSCANStats,
}

#[derive(Message)]
#[rtype(result = "Result<DBSCANResult, String>")]
pub struct RunDBSCAN {
    pub params: DBSCANParams,
}

// ---------------------------------------------------------------------------
// GPU Clustering (refs webxr-internal handler analytics types)
// ---------------------------------------------------------------------------

#[derive(Message, Clone)]
#[rtype(result = "Result<Vec<crate::handlers::api_handler::analytics::Cluster>, String>")]
pub struct PerformGPUClustering {
    pub method: String,
    pub params: crate::handlers::api_handler::analytics::ClusteringParams,
    pub task_id: String,
}

/// Persist a finished clustering run's per-node assignments into the shared
/// `node_analytics` store through the single writer (ADR-031 D3): ClusteringActor.
///
/// The spawn task in `clustering_handlers::run_clustering` obtains a final
/// `Vec<Cluster>` (whose `nodes` carry graph node ids) regardless of whether the
/// GPU kernels or the CPU label-propagation fallback produced it, then routes the
/// assignment back through this message so `node_analytics.cluster_id` is written
/// by the same masked-key / 1-based / stale-reset writer the GPU path uses. No
/// second writer is introduced — ClusteringActor delegates to the shared
/// `write_cluster_id_from_assignments` method.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct WriteClusterAnalytics {
    pub clusters: Vec<crate::handlers::api_handler::analytics::Cluster>,
}

// ---------------------------------------------------------------------------
// SSSP (ComputeShortestPaths refs ports::gpu_semantic_analyzer::PathfindingResult)
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<super::PathfindingResult, String>")]
pub struct ComputeShortestPaths {
    pub source_node_id: u32,
}

// ---------------------------------------------------------------------------
// PageRank Centrality (refs webxr-internal gpu actor types)
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<crate::actors::gpu::pagerank_actor::PageRankResult, String>")]
pub struct ComputePageRank {
    pub params: Option<crate::actors::gpu::pagerank_actor::PageRankParams>,
}

#[derive(Message)]
#[rtype(result = "Option<crate::actors::gpu::pagerank_actor::PageRankResult>")]
pub struct GetPageRankResult;
