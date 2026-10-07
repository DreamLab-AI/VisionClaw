//! Domain-safe analytics message types.
//!
//! Moved here: pure parameter structs/enums and messages whose rtype
//! results use only std/domain types.
//!
//! Blocked in webxr (stay in src/actors/messages/analytics_messages.rs):
//!   - `KMeansResult`             — refs `handlers::api_handler::analytics::Cluster`,
//!     `actors::gpu::clustering_actor::ClusteringStats`
//!   - `RunKMeans`                — rtype refs KMeansResult (webxr-internal)
//!   - `AnomalyResult`            — refs `actors::gpu::anomaly_detection_actor::AnomalyNode`
//!   - `RunAnomalyDetection`      — rtype refs AnomalyResult (webxr-internal)
//!   - `CommunityDetectionResult` — refs `actors::gpu::clustering_actor::*`
//!   - `RunCommunityDetection`    — rtype refs CommunityDetectionResult
//!   - `DBSCANResult`             — refs `handlers::api_handler::analytics::Cluster`
//!   - `RunDBSCAN`                — rtype refs DBSCANResult
//!   - `PerformGPUClustering`     — refs `handlers::api_handler::analytics::*`
//!   - `ComputePageRank`          — refs `actors::gpu::pagerank_actor::PageRankParams`
//!   - `GetPageRankResult`        — refs `actors::gpu::pagerank_actor::PageRankResult`
//!   - `ComputeShortestPaths`     — rtype refs `ports::gpu_semantic_analyzer::PathfindingResult`

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// K-means clustering (params only — KMeansResult stays in webxr)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KMeansParams {
    pub num_clusters: usize,
    pub max_iterations: Option<u32>,
    pub tolerance: Option<f32>,
    pub seed: Option<u32>,
}

// ---------------------------------------------------------------------------
// Anomaly detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyDetectionStats {
    pub total_nodes_analyzed: u32,
    pub anomalies_found: usize,
    pub detection_threshold: f32,
    pub computation_time_ms: u64,
    pub method: AnomalyDetectionMethod,
    pub average_anomaly_score: f32,
    pub max_anomaly_score: f32,
    pub min_anomaly_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalyMethod {
    LocalOutlierFactor,
    ZScore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalyDetectionMethod {
    LOF,
    ZScore,
    IsolationForest,
    DBSCAN,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyParams {
    pub method: AnomalyMethod,
    pub k_neighbors: i32,
    pub radius: f32,
    pub feature_data: Option<Vec<f32>>,
    pub threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyDetectionParams {
    pub method: AnomalyDetectionMethod,
    pub threshold: Option<f32>,
    pub k_neighbors: Option<i32>,
    pub window_size: Option<usize>,
    pub feature_data: Option<Vec<f32>>,
}

// ---------------------------------------------------------------------------
// Community detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommunityDetectionAlgorithm {
    LabelPropagation,
    Louvain,
    Leiden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityDetectionParams {
    pub algorithm: CommunityDetectionAlgorithm,
    pub max_iterations: Option<u32>,
    pub convergence_tolerance: Option<f32>,
    pub synchronous: Option<bool>,
    pub seed: Option<u32>,
    /// Modularity resolution (Louvain / Leiden). Higher → more, smaller communities.
    pub resolution: Option<f32>,
}

// ---------------------------------------------------------------------------
// DBSCAN clustering
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DBSCANStats {
    pub total_nodes: usize,
    pub num_clusters: usize,
    pub num_noise_points: usize,
    pub largest_cluster_size: usize,
    pub smallest_cluster_size: usize,
    pub average_cluster_size: f32,
    pub computation_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DBSCANParams {
    pub epsilon: f32,
    pub min_points: u32,
}

// ---------------------------------------------------------------------------
// GPU Clustering (orchestration messages — pure param/result types)
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<String, String>")]
pub struct StartGPUClustering {
    pub algorithm: String,
    pub cluster_count: u32,
    pub task_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<serde_json::Value, String>")]
pub struct GetClusteringStatus;

#[derive(Message)]
#[rtype(result = "Result<serde_json::Value, String>")]
pub struct GetClusteringResults;

#[derive(Message)]
#[rtype(result = "Result<String, String>")]
pub struct ExportClusterAssignments {
    pub format: String,
}

// ---------------------------------------------------------------------------
// SSSP (Single-Source Shortest Path)
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ComputeSSSP {
    pub source_node: u32,
}

#[derive(Message)]
#[rtype(result = "Result<HashMap<(u32, u32), Vec<u32>>, String>")]
pub struct ComputeAllPairsShortestPaths {
    pub num_landmarks: Option<usize>,
}

// ---------------------------------------------------------------------------
// PageRank Centrality
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "()")]
pub struct ClearPageRankCache;

// ---------------------------------------------------------------------------
// Connected Components
// ---------------------------------------------------------------------------

// REMOVED (ADR-2054, vc-gpu-wire; struct side by vc-knowledge): `UpdateComponentEdges`.
// The message had zero senders anywhere in the tree, and it was the sole writer of
// `ConnectedComponentsActor::cached_edges`, so the CPU fallback that read that field
// could only ever have produced all-singleton components. vc-gpu-wire removed the
// handler, the field and that fallback; this is the struct definition it left behind.
// The GPU-failure branch now propagates the error instead of fabricating a
// wrong-but-plausible result.

// ---------------------------------------------------------------------------
// Node Analytics (ADR-014 Phase 2 — DL4 fix)
// ---------------------------------------------------------------------------

/// Inject the shared node_analytics map into an analytics actor so it can
/// populate cluster_id / anomaly_score / community_id after computation.
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SetNodeAnalytics {
    pub node_analytics: std::sync::Arc<
        std::sync::RwLock<
            std::collections::HashMap<u32, visionclaw_domain::analytics::NodeAnalytics>,
        >,
    >,
}

/// Inject the shared node_sssp map into the ShortestPathActor so it can publish
/// per-node shortest-path distances after a ComputeSSP run. The map is keyed by
/// compact node_id -> (distance, parent_id). `f32::INFINITY` marks unreachable;
/// `parent_id == -1` marks no predecessor (run_sssp returns no predecessor array).
/// Read by the binary broadcast path to fill V3 wire slot 28 (sssp_distance@28).
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct SetNodeSSSP {
    pub node_sssp: SharedNodeSssp,
}

/// The shared per-node SSSP map carried by [`SetNodeSSSP`]: compact node_id ->
/// (distance, parent_id).
pub type SharedNodeSssp =
    std::sync::Arc<std::sync::RwLock<std::collections::HashMap<u32, (f32, i32)>>>;
