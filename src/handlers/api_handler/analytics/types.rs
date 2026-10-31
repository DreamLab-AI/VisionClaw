use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gpu::visual_analytics::{PerformanceMetrics, VisualAnalyticsParams};
use visionclaw_domain::models::constraints::ConstraintSet;

// GPUPhysicsStats - connecting to real GPU compute actors for live performance data

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GPUPhysicsStats {
    pub iteration_count: u32,
    pub nodes_count: u32,
    pub edges_count: u32,
    pub kinetic_energy: f32,
    pub total_forces: f32,
    pub gpu_enabled: bool,

    pub compute_mode: String,
    pub kernel_mode: String,
    pub num_nodes: u32,
    pub num_edges: u32,
    pub num_constraints: u32,
    pub num_isolation_layers: u32,
    pub stress_majorization_interval: u32,
    pub last_stress_majorization: u32,
    pub gpu_failure_count: u32,
    pub has_advanced_features: bool,
    pub has_dual_graph_features: bool,
    pub has_visual_analytics_features: bool,
    pub stress_safety_stats: StressMajorizationStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressMajorizationStats {
    pub total_runs: u32,
    pub successful_runs: u32,
    pub failed_runs: u32,
    pub consecutive_failures: u32,
    pub emergency_stopped: bool,
    pub last_error: String,
    pub average_computation_time_ms: u64,
    pub success_rate: f32,
    pub is_emergency_stopped: bool,
    pub emergency_stop_reason: String,
    pub avg_computation_time_ms: u64,
    pub avg_stress: f32,
    pub avg_displacement: f32,
    pub is_converging: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsParamsResponse {
    pub success: bool,
    pub params: Option<VisualAnalyticsParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintsResponse {
    pub success: bool,
    pub constraints: Option<ConstraintSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConstraintsRequest {
    pub constraint_set: Option<ConstraintSet>,
    pub constraint_data: Option<Value>,
    pub group_name: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetFocusRequest {
    pub node_id: Option<i32>,
    pub region: Option<FocusRegion>,
    pub radius: Option<f32>,
    pub intensity: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusRegion {
    pub center_x: f32,
    pub center_y: f32,
    pub center_z: f32,
    pub radius: f32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusResponse {
    pub success: bool,
    pub focus_node: Option<i32>,
    pub focus_region: Option<FocusRegion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsResponse {
    pub success: bool,
    pub physics_stats: Option<GPUPhysicsStats>,
    pub visual_analytics_metrics: Option<PerformanceMetrics>,
    pub system_metrics: Option<SystemMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMetrics {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub gpu_utilization: f32,
    pub memory_usage_mb: f32,
    pub active_nodes: u32,
    pub active_edges: u32,
    pub render_time_ms: f32,
    pub network_cost_per_mb: f32,
    pub total_network_cost: f32,
    pub bandwidth_usage_mbps: f32,
    pub data_transfer_mb: f32,
    pub network_latency_ms: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusteringRequest {
    #[serde(alias = "algorithm")]
    pub method: String,
    pub params: ClusteringParams,
}

// ADR-090 A6.4: ClusteringParams and Cluster moved to visionclaw-domain
// (they were the only types making 6 actor files depend on handlers).
// Re-exported here so HTTP handlers keep their existing `super::types::*`
// imports working unchanged.
pub use visionclaw_domain::analytics::{Cluster, ClusteringParams};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusteringResponse {
    pub success: bool,
    pub clusters: Option<Vec<Cluster>>,
    pub method: Option<String>,
    pub execution_time_ms: Option<u64>,
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusteringStatusResponse {
    pub success: bool,
    pub task_id: Option<String>,
    pub status: String,
    pub progress: f32,
    pub method: Option<String>,
    pub started_at: Option<String>,
    pub estimated_completion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterFocusRequest {
    pub cluster_id: String,
    pub zoom_level: Option<f32>,
    pub highlight: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyDetectionConfig {
    pub enabled: bool,
    pub method: String,
    pub sensitivity: f32,
    pub window_size: u32,
    pub update_interval: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Anomaly {
    pub id: String,
    pub node_id: String,
    pub r#type: String,
    pub severity: String,
    pub score: f32,
    pub description: String,
    pub timestamp: u64,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyStats {
    pub total: u32,
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub last_updated: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyResponse {
    pub success: bool,
    pub anomalies: Option<Vec<Anomaly>>,
    pub stats: Option<AnomalyStats>,
    pub enabled: Option<bool>,
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsResponse {
    pub success: bool,
    pub insights: Option<Vec<String>>,
    pub patterns: Option<Vec<GraphPattern>>,
    pub recommendations: Option<Vec<String>>,
    pub analysis_timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPattern {
    pub id: String,
    pub r#type: String,
    pub description: String,
    pub confidence: f32,
    pub nodes: Vec<u32>,
    pub significance: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SSSPRequest {
    pub source_node_id: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SSSPToggleRequest {
    pub enabled: bool,
    pub alpha: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SSSPResponse {
    pub success: bool,
    pub distances: Option<std::collections::HashMap<u32, Option<f32>>>,
    pub unreachable_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SSSPToggleResponse {
    pub success: bool,
    pub enabled: bool,
    pub alpha: Option<f32>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClusteringTask {
    pub task_id: String,
    pub method: String,
    pub status: String,
    pub progress: f32,
    pub started_at: u64,
    pub clusters: Option<Vec<Cluster>>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AnomalyState {
    pub enabled: bool,
    pub method: String,
    pub sensitivity: f32,
    pub window_size: u32,
    pub update_interval: u32,
    pub anomalies: Vec<Anomaly>,
    pub stats: AnomalyStats,
}

// Seven of the original nine flags (`gpu_clustering`, `gpu_anomaly_detection`,
// `real_time_insights`, `advanced_visualizations`, `performance_monitoring`,
// `stress_majorization`, `semantic_constraints`) were removed 2026-09-05:
// `FEATURE_FLAGS` (see `state.rs`) was verified to be read only in
// `sssp_handlers.rs` (`sssp_integration`) and `ontology/mod.rs`
// (`ontology_validation`) — every other field gated nothing anywhere in the
// codebase. `ontology_validation` remains a real gate on the reasoning
// endpoints. `sssp_integration` stays on the wire but gates nothing and is no
// longer held here: the feature-flag handlers derive it from the stored
// `physics.ssspEnabled` (ADR-2140), and a body that sets it is ignored. Its
// `FEATURE_FLAGS` value is a placeholder.
#[derive(Debug, Serialize, Deserialize)]
pub struct FeatureFlags {
    pub ontology_validation: bool,
    #[serde(default = "default_sssp_integration")]
    pub sssp_integration: bool,
}

fn default_sssp_integration() -> bool {
    true
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            sssp_integration: true,
            // Reasoning endpoints (/api/ontology/{validate,inferred,inference,
            // metrics,hierarchy}) are gated on this flag. The Whelk classifier
            // runs at boot regardless; default-on EXPOSES the already-materialised
            // inferred graph via the API so the bridge/agents (and the About page's
            // "live reasoned graph") reflect it persistently, not just until restart.
            ontology_validation: true,
        }
    }
}

#[cfg(test)]
mod feature_flags_tests {
    use super::*;

    #[test]
    fn a_flags_body_without_sssp_integration_still_parses() {
        let flags: FeatureFlags =
            serde_json::from_value(serde_json::json!({ "ontology_validation": false })).unwrap();
        assert!(!flags.ontology_validation);
        assert!(flags.sssp_integration);
    }
}
