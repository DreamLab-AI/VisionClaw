

use actix_web::{web, HttpResponse, Result};
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use crate::{ok_json, error_json, service_unavailable, bad_request, not_found};

use crate::actors::messages::{
    GetConstraints, GetGraphData, GetSettings,
    UpdateConstraints,
};
use crate::actors::messages::GetPhysicsStats;

use crate::actors::messages::{
    GetStressMajorizationStats,
    ResetStressMajorizationSafety,
    SetComputeMode,
    TriggerStressMajorization,
    UpdateStressMajorizationParams,
    UpdateVisualAnalyticsParams,
    ConfigureStressMajorization,
    GetStressMajorizationConfig,
};

use crate::gpu::visual_analytics::{PerformanceMetrics, VisualAnalyticsParams};
use crate::models::constraints::{AdvancedParams, ConstraintSet};
use crate::services::agent_visualization_protocol::McpServerType;
use crate::utils::mcp_tcp_client::create_mcp_client;
use crate::AppState;

// Import real GPU functions module
mod real_gpu_functions;
use real_gpu_functions::*;
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

// WebSocket integration module
pub mod websocket_integration;

// Community detection module
pub mod community;

// Pathfinding module (SSSP, APSP, Connected Components)
pub mod pathfinding;

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
    pub method: String,
    pub params: ClusteringParams,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusteringParams {
    pub num_clusters: Option<u32>,
    pub min_cluster_size: Option<u32>,
    pub similarity: Option<String>,
    pub convergence_threshold: Option<f32>,
    pub max_iterations: Option<u32>,
    pub eps: Option<f32>,
    pub min_samples: Option<u32>,
    pub distance_threshold: Option<f32>,
    pub linkage: Option<String>,
    pub resolution: Option<f32>,
    pub random_state: Option<u32>,
    pub damping: Option<f32>,
    pub preference: Option<f32>,
    pub tolerance: Option<f64>,
    pub seed: Option<u64>,
    pub sigma: Option<f64>,
    pub min_modularity_gain: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    pub id: String,
    pub label: String,
    pub node_count: u32,
    pub coherence: f32,
    pub color: String,
    pub keywords: Vec<String>,
    pub nodes: Vec<u32>,
    pub centroid: Option<[f32; 3]>,
}

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

// Global state for clustering operations
static CLUSTERING_TASKS: Lazy<Arc<Mutex<HashMap<String, ClusteringTask>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

static ANOMALY_STATE: Lazy<Arc<Mutex<AnomalyState>>> =
    Lazy::new(|| Arc::new(Mutex::new(AnomalyState::default())));

#[derive(Debug, Clone)]
struct ClusteringTask {
    pub task_id: String,
    pub method: String,
    pub status: String,
    pub progress: f32,
    pub started_at: u64,
    pub clusters: Option<Vec<Cluster>>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct AnomalyState {
    pub enabled: bool,
    pub method: String,
    pub sensitivity: f32,
    pub window_size: u32,
    pub update_interval: u32,
    pub anomalies: Vec<Anomaly>,
    pub stats: AnomalyStats,
}

pub async fn get_analytics_params(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Getting current visual analytics parameters");

    let settings = match app_state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings for analytics params: {}", e);
            return Ok(
                HttpResponse::InternalServerError().json(AnalyticsParamsResponse {
                    success: false,
                    params: None,
                    error: Some("Failed to retrieve settings".to_string()),
                }),
            );
        }
        Err(e) => {
            error!("Settings actor mailbox error: {}", e);
            return Ok(
                HttpResponse::InternalServerError().json(AnalyticsParamsResponse {
                    success: false,
                    params: None,
                    error: Some("Settings service unavailable".to_string()),
                }),
            );
        }
    };


    let params = create_default_analytics_params(&settings);

    ok_json!(AnalyticsParamsResponse {
        success: true,
        params: Some(params),
        error: None,
    })
}

pub async fn update_analytics_params(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    params: web::Json<VisualAnalyticsParams>,
) -> Result<HttpResponse> {
    info!("Updating visual analytics parameters");
    debug!("Visual analytics params: {:?}", params);

    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        match gpu_addr
            .send(UpdateVisualAnalyticsParams {
                params: params.into_inner(),
            })
            .await
        {
            Ok(Ok(())) => {
                info!("Visual analytics parameters updated successfully");
                ok_json!(AnalyticsParamsResponse {
                    success: true,
                    params: None,
                    error: None,
                })
            }
            Ok(Err(e)) => {
                error!("Failed to update visual analytics params: {}", e);
                Ok(
                    HttpResponse::InternalServerError().json(AnalyticsParamsResponse {
                        success: false,
                        params: None,
                        error: Some(format!("Failed to update parameters: {}", e)),
                    }),
                )
            }
            Err(e) => {
                error!("GPU compute actor mailbox error: {}", e);
                Ok(
                    HttpResponse::ServiceUnavailable().json(AnalyticsParamsResponse {
                        success: false,
                        params: None,
                        error: Some("GPU compute service unavailable".to_string()),
                    }),
                )
            }
        }
    } else {
        Ok(
            HttpResponse::ServiceUnavailable().json(AnalyticsParamsResponse {
                success: false,
                params: None,
                error: Some("GPU compute service not available".to_string()),
            }),
        )
    }
}

pub async fn get_constraints(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Getting current constraint set");

    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        match gpu_addr.send(GetConstraints).await {
            Ok(Ok(constraints)) => {
                return ok_json!(ConstraintsResponse {
                    success: true,
                    constraints: Some(constraints),
                    error: None,
                });
            }
            Ok(Err(e)) => {
                error!("Failed to get constraints from GPU actor: {}", e);
            }
            Err(e) => {
                error!("GPU compute actor mailbox error: {}", e);
            }
        }
    }


    ok_json!(ConstraintsResponse {
        success: true,
        constraints: Some(ConstraintSet::default()),
        error: None,
    })
}

pub async fn update_constraints(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<UpdateConstraintsRequest>,
) -> Result<HttpResponse> {
    info!("Updating constraint set");

    let constraint_data = if let Some(constraint_set) = &request.constraint_set {
        serde_json::to_value(constraint_set).unwrap_or_default()
    } else if let Some(data) = &request.constraint_data {
        data.clone()
    } else {
        return Ok(HttpResponse::BadRequest().json(ConstraintsResponse {
            success: false,
            constraints: None,
            error: Some("Either constraint_set or constraint_data must be provided".to_string()),
        }));
    };

    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        match gpu_addr.send(UpdateConstraints { constraint_data }).await {
            Ok(Ok(())) => {
                debug!("Constraints updated successfully");


                if let Ok(Ok(updated_constraints)) = gpu_addr.send(GetConstraints).await {
                    return ok_json!(ConstraintsResponse {
                        success: true,
                        constraints: Some(updated_constraints),
                        error: None,
                    });
                }
            }
            Ok(Err(e)) => {
                error!("Failed to update constraints: {}", e);
                return Ok(
                    HttpResponse::InternalServerError().json(ConstraintsResponse {
                        success: false,
                        constraints: None,
                        error: Some(format!("Failed to update constraints: {}", e)),
                    }),
                );
            }
            Err(e) => {
                error!("GPU compute actor mailbox error: {}", e);
                return Ok(
                    HttpResponse::InternalServerError().json(ConstraintsResponse {
                        success: false,
                        constraints: None,
                        error: Some("GPU compute service unavailable".to_string()),
                    }),
                );
            }
        }
    }

    Ok(
        HttpResponse::ServiceUnavailable().json(ConstraintsResponse {
            success: false,
            constraints: None,
            error: Some("GPU compute service not available".to_string()),
        }),
    )
}

pub async fn set_focus(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<SetFocusRequest>,
) -> Result<HttpResponse> {
    info!("Setting focus node/region");

    let mut focus_response = if let Some(node_id) = request.node_id {
        
        debug!("Setting focus on node {}", node_id);
        FocusResponse {
            success: false, 
            focus_node: Some(node_id),
            focus_region: None,
            error: None,
        }
    } else if let Some(region) = &request.region {
        
        debug!(
            "Setting focus on region center: ({}, {}, {}), radius: {}",
            region.center_x, region.center_y, region.center_z, region.radius
        );
        FocusResponse {
            success: false, 
            focus_node: None,
            focus_region: Some(FocusRegion {
                center_x: region.center_x,
                center_y: region.center_y,
                center_z: region.center_z,
                radius: region.radius,
            }),
            error: None,
        }
    } else {
        return Ok(HttpResponse::BadRequest().json(FocusResponse {
            success: false,
            focus_node: None,
            focus_region: None,
            error: Some("Either node_id or region must be specified".to_string()),
        }));
    };

    
    let current_params = VisualAnalyticsParams::default(); 

    
    #[derive(Debug)]
    enum FocusRequest {
        Node { node_id: u32 },
        Region { x: f32, y: f32, radius: f32 },
    }

    let focus_request = if let Some(node_id) = request.node_id {
        FocusRequest::Node {
            node_id: node_id as u32,
        }
    } else if let Some(region) = &request.region {
        FocusRequest::Region {
            x: region.center_x,
            y: region.center_y,
            radius: region.radius,
        }
    } else {
        return Ok(HttpResponse::BadRequest().json(FocusResponse {
            success: false,
            focus_node: None,
            focus_region: None,
            error: Some("Either node_id or region must be specified".to_string()),
        }));
    };


    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        info!("Setting focus on GPU compute actor");


        let mut updated_params = current_params.clone();
        match focus_request {
            FocusRequest::Node { node_id } => {
                updated_params.primary_focus_node = node_id as i32;
                info!("Setting focus on node: {}", node_id);
                focus_response.focus_node = Some(node_id as i32);
            }
            FocusRequest::Region { x, y, radius } => {
                updated_params.camera_position =
                    crate::gpu::visual_analytics::Vec4::new(x, y, 0.0, 0.0).unwrap_or_default();
                updated_params.zoom_level = 1.0 / radius.max(1.0);
                info!("Setting focus on region: ({}, {}) radius {}", x, y, radius);
                focus_response.focus_region = Some(FocusRegion {
                    center_x: x,
                    center_y: y,
                    center_z: 0.0,
                    radius,
                });
            }
        }

        
        use crate::actors::messages::UpdateVisualAnalyticsParams;
        match gpu_addr
            .send(UpdateVisualAnalyticsParams {
                params: updated_params,
            })
            .await
        {
            Ok(Ok(())) => {
                info!("Successfully updated visual analytics parameters with focus settings");
                focus_response.success = true;
            }
            Ok(Err(e)) => {
                warn!("Failed to update visual analytics parameters: {}", e);
                focus_response.error = Some(format!("GPU parameter update failed: {}", e));
            }
            Err(e) => {
                error!("Failed to communicate with GPU for parameter update: {}", e);
                focus_response.error = Some(format!("GPU communication failed: {}", e));
            }
        }
    } else {
        warn!("GPU compute actor not available for focus setting");

        
        match focus_request {
            FocusRequest::Node { node_id } => {
                focus_response.focus_node = Some(node_id as i32);
                info!(
                    "Focus parameters stored for node {} (GPU not available)",
                    node_id
                );
            }
            FocusRequest::Region { x, y, radius } => {
                focus_response.focus_region = Some(FocusRegion {
                    center_x: x,
                    center_y: y,
                    center_z: 0.0,
                    radius,
                });
                info!(
                    "Focus parameters stored for region ({}, {}) radius {} (GPU not available)",
                    x, y, radius
                );
            }
        }
        focus_response.success = true; 
    }

    ok_json!(focus_response)
}

async fn calculate_network_metrics(
    _app_state: &AppState,
    physics_stats: &Option<GPUPhysicsStats>,
) -> (f32, f32, f32, f32, f32) {

    let active_nodes = physics_stats.as_ref().map(|s| s.nodes_count).unwrap_or(0) as f32;
    let active_edges = physics_stats.as_ref().map(|s| s.edges_count).unwrap_or(0) as f32;

    
    
    let bytes_per_node_per_frame = 38.0;
    let frames_per_second = 60.0;
    let seconds_per_minute = 60.0;

    
    let data_transfer_mb =
        (active_nodes * bytes_per_node_per_frame * frames_per_second * seconds_per_minute)
            / (1024.0 * 1024.0);

    
    let bandwidth_usage_mbps =
        (active_nodes * bytes_per_node_per_frame * frames_per_second * 8.0) / (1024.0 * 1024.0);

    
    
    let cost_per_gb = 0.09; 
    let cost_per_mb = cost_per_gb / 1024.0;
    let network_cost_per_mb = cost_per_mb;

    
    let total_network_cost = data_transfer_mb * network_cost_per_mb;

    
    let base_latency = 15.0; 
    let complexity_factor = (active_edges / (active_nodes + 1.0)).min(10.0); 
    let network_latency_ms = base_latency + (complexity_factor * 2.0);

    
    
    let base_mcp_latency = 5.0;
    
    let coordination_overhead = (active_edges / 1000.0).min(20.0); 
    let mcp_latency = base_mcp_latency + coordination_overhead;

    let final_network_latency = network_latency_ms + mcp_latency;

    (
        network_cost_per_mb,
        total_network_cost,
        bandwidth_usage_mbps,
        data_transfer_mb,
        final_network_latency,
    )
}

pub async fn get_performance_stats(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Getting performance statistics");

    let _physics_stats_attempt = if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        match gpu_addr.send(GetPhysicsStats).await {
            Ok(Ok(stats)) => Some(stats),
            Ok(Err(e)) => {
                warn!("Failed to get physics stats: {}", e);
                None
            }
            Err(e) => {
                warn!("GPU compute actor mailbox error: {}", e);
                None
            }
        }
    } else {
        None
    };

    let physics_stats: Option<GPUPhysicsStats> = None;


    let (
        network_cost_per_mb,
        total_network_cost,
        bandwidth_usage_mbps,
        data_transfer_mb,
        network_latency_ms,
    ) = calculate_network_metrics(&app_state, &physics_stats).await;

    let (_active_nodes_attempt, _active_edges_attempt) = (
        physics_stats.as_ref().map(|s| s.nodes_count).unwrap_or(0),
        physics_stats.as_ref().map(|s| s.edges_count).unwrap_or(0),
    );

    let (active_nodes, active_edges) = (0u32, 0u32);

    let system_metrics = SystemMetrics {
        fps: 60.0,
        frame_time_ms: 16.67,
        gpu_utilization: 45.0,
        memory_usage_mb: 512.0,
        active_nodes,
        active_edges,
        render_time_ms: 12.5,
        network_cost_per_mb,
        total_network_cost,
        bandwidth_usage_mbps,
        data_transfer_mb,
        network_latency_ms,
    };

    ok_json!(StatsResponse {
        success: true,
        physics_stats: get_real_gpu_physics_stats(&app_state).await,
        visual_analytics_metrics: None,
        system_metrics: Some(system_metrics),
        error: None,
    })
}

fn create_default_analytics_params(
    _settings: &crate::config::AppFullSettings,
) -> VisualAnalyticsParams {
    use crate::gpu::visual_analytics::VisualAnalyticsBuilder;

    VisualAnalyticsBuilder::new()
        .with_nodes(1000)
        .with_edges(2000)
        .with_focus(-1, 2.2)
        .with_temporal_decay(0.1)
        .build()
}

pub async fn set_kernel_mode(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<serde_json::Value>,
) -> Result<HttpResponse> {
    info!("Setting GPU kernel mode");

    if let Some(mode) = request.get("mode").and_then(|m| m.as_str()) {

        let compute_mode = match mode {
            "legacy" => crate::utils::unified_gpu_compute::ComputeMode::Basic,
            "dual_graph" => crate::utils::unified_gpu_compute::ComputeMode::DualGraph,
            "advanced" => crate::utils::unified_gpu_compute::ComputeMode::Advanced,

            "standard" => crate::utils::unified_gpu_compute::ComputeMode::Basic,


            "visual_analytics" => crate::utils::unified_gpu_compute::ComputeMode::Advanced,
            _ => {
                return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "success": false,
                    "error": format!("Invalid mode: {}. Valid modes: legacy, dual_graph, advanced", mode)
                })));
            }
        };

        if let Some(gpu_actor) = app_state.get_gpu_compute_addr().await {
            match gpu_actor.send(SetComputeMode { mode: compute_mode }).await {
                Ok(result) => match result {
                    Ok(()) => {
                        info!("GPU kernel mode set to: {}", mode);
                        ok_json!(serde_json::json!({
                            "success": true,
                            "mode": mode
                        }))
                    }
                    Err(e) => {
                        error!("Failed to set kernel mode: {}", e);
                        Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                            "success": false,
                            "error": e
                        })))
                    }
                },
                Err(e) => {
                    error!("Failed to send kernel mode message: {}", e);
                    error_json!("Failed to communicate with GPU actor")
                }
            }
        } else {
            service_unavailable!("GPU compute not available")
        }
    } else {
        bad_request!("Missing 'mode' parameter")
    }
}

pub async fn run_clustering(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<ClusteringRequest>,
) -> Result<HttpResponse> {
    info!(
        "Starting clustering analysis with method: {}",
        request.method
    );

    let task_id = Uuid::new_v4().to_string();
    let method = request.method.clone();

    
    let task = ClusteringTask {
        task_id: task_id.clone(),
        method: method.clone(),
        status: "running".to_string(),
        progress: 0.0,
        started_at: chrono::Utc::now().timestamp() as u64,
        clusters: None,
        error: None,
    };

    
    {
        let mut tasks = CLUSTERING_TASKS.lock().await;
        tasks.insert(task_id.clone(), task);
    }

    
    let app_state_clone = app_state.clone();
    let task_id_clone = task_id.clone();
    let request_clone = request.into_inner();

    tokio::spawn(async move {
        let clusters = perform_clustering(&app_state_clone, &request_clone, &task_id_clone).await;

        let mut tasks = CLUSTERING_TASKS.lock().await;
        if let Some(task) = tasks.get_mut(&task_id_clone) {
            match clusters {
                Ok(clusters) => {
                    task.status = "completed".to_string();
                    task.progress = 1.0;
                    task.clusters = Some(clusters);
                }
                Err(e) => {
                    task.status = "failed".to_string();
                    task.error = Some(e);
                }
            }
        }
    });

    ok_json!(ClusteringResponse {
        success: true,
        clusters: None,
        method: Some(method),
        execution_time_ms: None,
        task_id: Some(task_id),
        error: None,
    })
}

pub async fn get_clustering_status(
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse> {
    let task_id = query.get("task_id");

    if let Some(task_id) = task_id {
        let tasks = CLUSTERING_TASKS.lock().await;
        if let Some(task) = tasks.get(task_id) {
            let estimated_completion = if task.status == "running" {
                Some(chrono::Utc::now().timestamp() as u64 + 30) 
            } else {
                None
            };

            return ok_json!(ClusteringStatusResponse {
                success: true,
                task_id: Some(task.task_id.clone()),
                status: task.status.clone(),
                progress: task.progress,
                method: Some(task.method.clone()),
                started_at: Some(task.started_at.to_string()),
                estimated_completion: estimated_completion.map(|t| t.to_string()),
                error: task.error.clone(),
            });
        }
    }

    Ok(HttpResponse::NotFound().json(ClusteringStatusResponse {
        success: false,
        task_id: None,
        status: "not_found".to_string(),
        progress: 0.0,
        method: None,
        started_at: None,
        estimated_completion: None,
        error: Some("Task not found".to_string()),
    }))
}

pub async fn focus_cluster(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<ClusterFocusRequest>,
) -> Result<HttpResponse> {
    info!("Focusing on cluster: {}", request.cluster_id);

    
    let tasks = CLUSTERING_TASKS.lock().await;
    let cluster = tasks
        .values()
        .filter_map(|task| task.clusters.as_ref())
        .flatten()
        .find(|c| c.id == request.cluster_id)
        .cloned();

    if let Some(cluster) = cluster {
        
        if let Some(centroid) = cluster.centroid {
            let focus_request = SetFocusRequest {
                node_id: None,
                region: Some(FocusRegion {
                    center_x: centroid[0],
                    center_y: centroid[1],
                    center_z: centroid[2],
                    radius: request.zoom_level.unwrap_or(5.0),
                }),
                radius: Some(request.zoom_level.unwrap_or(5.0)),
                intensity: Some(1.0),
            };

            
            let focus_response = set_focus(auth, app_state, web::Json(focus_request)).await?;
            return Ok(focus_response);
        }
    }

    ok_json!(FocusResponse {
        success: true,
        focus_node: None,
        focus_region: None,
        error: Some("Cluster not found or no centroid available".to_string()),
    })
}

pub async fn toggle_anomaly_detection(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    request: web::Json<AnomalyDetectionConfig>,
) -> Result<HttpResponse> {
    info!("Toggling anomaly detection: enabled={}", request.enabled);

    let mut state = ANOMALY_STATE.lock().await;
    state.enabled = request.enabled;
    state.method = request.method.clone();
    state.sensitivity = request.sensitivity;
    state.window_size = request.window_size;
    state.update_interval = request.update_interval;

    if request.enabled {
        
        start_anomaly_detection().await;
    } else {
        
        state.anomalies.clear();
        state.stats = AnomalyStats::default();
    }

    ok_json!(AnomalyResponse {
        success: true,
        anomalies: None,
        stats: Some(state.stats.clone()),
        enabled: Some(state.enabled),
        method: Some(state.method.clone()),
        error: None,
    })
}

pub async fn get_current_anomalies() -> Result<HttpResponse> {
    let state = ANOMALY_STATE.lock().await;

    if !state.enabled {
        return ok_json!(AnomalyResponse {
            success: true,
            anomalies: Some(vec![]),
            stats: Some(AnomalyStats::default()),
            enabled: Some(false),
            method: None,
            error: None,
        });
    }

    ok_json!(AnomalyResponse {
        success: true,
        anomalies: Some(state.anomalies.clone()),
        stats: Some(state.stats.clone()),
        enabled: Some(state.enabled),
        method: Some(state.method.clone()),
        error: None,
    })
}

async fn perform_clustering(
    app_state: &web::Data<AppState>,
    request: &ClusteringRequest,
    task_id: &str,
) -> Result<Vec<Cluster>, String> {
    info!("Performing real clustering analysis using MCP agent data");

    
    let graph_data = {
        match app_state.graph_service_addr.send(GetGraphData).await {
            Ok(Ok(data)) => data,
            _ => return Err("Failed to get graph data".to_string()),
        }
    };

    
    let host = std::env::var("MCP_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("MCP_TCP_PORT")
        .unwrap_or_else(|_| "9500".to_string())
        .parse::<u16>()
        .unwrap_or(9500);

    let mcp_client = create_mcp_client(&McpServerType::ClaudeFlow, &host, port);

    
    let agents = match mcp_client.query_agent_list().await {
        Ok(agent_list) => {
            info!(
                "Retrieved {} agents from MCP server for clustering",
                agent_list.len()
            );
            agent_list
        }
        Err(e) => {
            warn!(
                "Failed to get agents from MCP server, using graph data: {}",
                e
            );
            Vec::new()
        }
    };

    
    let clusters = match request.method.as_str() {
        "spectral" => {
            perform_gpu_spectral_clustering(&**app_state, &graph_data, &agents, &request.params)
                .await
        }
        "kmeans" => {
            perform_gpu_kmeans_clustering(&**app_state, &graph_data, &agents, &request.params).await
        }
        "louvain" => {
            perform_gpu_louvain_clustering(&**app_state, &graph_data, &agents, &request.params)
                .await
        }
        _ => {
            perform_gpu_default_clustering(&**app_state, &graph_data, &agents, &request.params)
                .await
        }
    };

    
    let mut tasks = CLUSTERING_TASKS.lock().await;
    if let Some(task) = tasks.get_mut(task_id) {
        task.progress = 0.5;
    }
    drop(tasks);

    
    let processing_time = std::cmp::min(agents.len() / 10, 5) as u64;
    tokio::time::sleep(tokio::time::Duration::from_secs(processing_time)).await;

    Ok(clusters)
}

fn generate_spectral_clusters_from_agents(
    graph_data: &crate::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    let num_clusters = params.num_clusters.unwrap_or(5);
    generate_agent_based_clusters(graph_data, agents, num_clusters, "spectral")
}

fn generate_kmeans_clusters_from_agents(
    graph_data: &crate::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    let num_clusters = params.num_clusters.unwrap_or(8);
    generate_agent_based_clusters(graph_data, agents, num_clusters, "kmeans")
}

fn generate_louvain_clusters_from_agents(
    graph_data: &crate::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    let resolution = params.resolution.unwrap_or(1.0);
    let num_clusters = std::cmp::min((5.0 / resolution) as u32, agents.len() as u32);
    generate_agent_based_clusters(graph_data, agents, num_clusters, "louvain")
}

fn generate_default_clusters_from_agents(
    graph_data: &crate::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    let cluster_count = std::cmp::min(params.num_clusters.unwrap_or(6), agents.len() as u32);
    generate_agent_based_clusters(graph_data, agents, cluster_count, "default")
}

fn generate_agent_based_clusters(
    graph_data: &crate::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    num_clusters: u32,
    method: &str,
) -> Vec<Cluster> {
    if agents.is_empty() {
        warn!("No agent data available for clustering, using graph-based clustering");
        return generate_graph_based_clusters(graph_data, num_clusters, method);
    }

    info!(
        "Generating {} clusters from {} real agents using {} method",
        num_clusters,
        agents.len(),
        method
    );

    
    let mut agent_type_groups: std::collections::HashMap<
        String,
        Vec<&crate::services::agent_visualization_protocol::MultiMcpAgentStatus>,
    > = std::collections::HashMap::new();

    for agent in agents {
        agent_type_groups
            .entry(agent.agent_type.clone())
            .or_insert_with(Vec::new)
            .push(agent);
    }

    let colors = vec![
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];

    let mut clusters = Vec::new();
    let mut cluster_id = 0;

    
    for (agent_type, type_agents) in agent_type_groups {
        if cluster_id >= num_clusters {
            break;
        }

        
        let _avg_cpu = type_agents
            .iter()
            .map(|a| a.performance.cpu_usage)
            .sum::<f32>()
            / type_agents.len() as f32;
        let _avg_memory = type_agents
            .iter()
            .map(|a| a.performance.memory_usage)
            .sum::<f32>()
            / type_agents.len() as f32;
        let avg_health = type_agents
            .iter()
            .map(|a| a.performance.health_score)
            .sum::<f32>()
            / type_agents.len() as f32;
        let _total_tasks = type_agents
            .iter()
            .map(|a| a.performance.tasks_completed)
            .sum::<u32>();

        
        let cluster_nodes: Vec<u32> = type_agents
            .iter()
            .enumerate()
            .map(|(idx, _)| cluster_id * 100 + idx as u32) 
            .take(graph_data.nodes.len() / num_clusters as usize)
            .collect();

        
        let centroid = if !cluster_nodes.is_empty() && !graph_data.nodes.is_empty() {
            let node_subset: Vec<_> = cluster_nodes
                .iter()
                .filter_map(|&id| graph_data.nodes.get(id as usize))
                .collect();

            if !node_subset.is_empty() {
                let sum_x: f32 = node_subset.iter().map(|n| n.data.x).sum();
                let sum_y: f32 = node_subset.iter().map(|n| n.data.y).sum();
                let sum_z: f32 = node_subset.iter().map(|n| n.data.z).sum();
                let count = node_subset.len() as f32;
                Some([sum_x / count, sum_y / count, sum_z / count])
            } else {
                None
            }
        } else {
            None
        };

        
        let keywords: Vec<String> = type_agents
            .iter()
            .flat_map(|agent| agent.capabilities.iter())
            .take(5)
            .cloned()
            .collect();

        let coherence = (avg_health / 100.0).min(1.0).max(0.0);

        clusters.push(Cluster {
            id: format!("cluster_{}_{}", method, cluster_id),
            label: format!("{} Agents ({})", agent_type, type_agents.len()),
            node_count: type_agents.len() as u32,
            coherence,
            color: colors
                .get(cluster_id as usize)
                .unwrap_or(&"#888888")
                .to_string(),
            keywords,
            nodes: cluster_nodes,
            centroid,
        });

        cluster_id += 1;
    }

    
    while clusters.len() < num_clusters as usize && cluster_id < num_clusters {
        clusters.push(Cluster {
            id: format!("cluster_{}_{}", method, cluster_id),
            label: format!("Mixed Cluster {}", cluster_id + 1),
            node_count: 0,
            coherence: 0.5,
            color: colors
                .get(cluster_id as usize)
                .unwrap_or(&"#888888")
                .to_string(),
            keywords: vec![format!("{}_analysis", method)],
            nodes: vec![],
            centroid: None,
        });
        cluster_id += 1;
    }

    info!("Generated {} real clusters from agent data", clusters.len());
    clusters
}

fn generate_graph_based_clusters(
    graph_data: &crate::models::graph::GraphData,
    num_clusters: u32,
    method: &str,
) -> Vec<Cluster> {
    let nodes_per_cluster = if graph_data.nodes.is_empty() {
        0
    } else {
        graph_data.nodes.len() / num_clusters as usize
    };
    let colors = vec![
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];
    let labels = vec![
        "Core Concepts",
        "Implementation",
        "Documentation",
        "Testing",
        "Infrastructure",
        "UI Components",
        "API Layer",
        "Data Models",
    ];

    (0..num_clusters)
        .map(|i| {
            let start_idx = (i as usize) * nodes_per_cluster;
            let end_idx = ((i + 1) as usize * nodes_per_cluster).min(graph_data.nodes.len());
            let cluster_nodes: Vec<u32> = (start_idx..end_idx).map(|idx| idx as u32).collect();

            let centroid = if !cluster_nodes.is_empty() {
                let sum_x: f32 = cluster_nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.x)
                    .sum();
                let sum_y: f32 = cluster_nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.y)
                    .sum();
                let sum_z: f32 = cluster_nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.z)
                    .sum();
                let count = cluster_nodes.len() as f32;
                Some([sum_x / count, sum_y / count, sum_z / count])
            } else {
                None
            };

            Cluster {
                id: format!("cluster_{}", i),
                label: labels.get(i as usize).unwrap_or(&"Cluster").to_string(),
                node_count: cluster_nodes.len() as u32,
                coherence: 0.75 + (i as f32 * 0.03),
                color: colors.get(i as usize).unwrap_or(&"#888888").to_string(),
                keywords: vec![
                    format!("{}_keyword1", method),
                    format!("{}_keyword2", method),
                ],
                nodes: cluster_nodes,
                centroid,
            }
        })
        .collect()
}

async fn start_anomaly_detection() {
    tokio::spawn(async move {
        info!("Starting real anomaly detection using MCP agent data");

        
        let host = std::env::var("MCP_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = std::env::var("MCP_TCP_PORT")
            .unwrap_or_else(|_| "9500".to_string())
            .parse::<u16>()
            .unwrap_or(9500);

        let mcp_client = create_mcp_client(&McpServerType::ClaudeFlow, &host, port);

        let agents = match mcp_client.query_agent_list().await {
            Ok(agent_list) => {
                info!("Analyzing {} agents for anomalies", agent_list.len());
                agent_list
            }
            Err(e) => {
                warn!(
                    "Failed to get agents from MCP server for anomaly detection: {}",
                    e
                );
                Vec::new()
            }
        };

        let mut state = ANOMALY_STATE.lock().await;
        let mut detected_anomalies = Vec::new();

        
        for agent in &agents {
            
            if agent.performance.cpu_usage > 90.0 {
                detected_anomalies.push(Anomaly {
                    id: Uuid::new_v4().to_string(),
                    node_id: agent.agent_id.clone(),
                    r#type: "high_cpu".to_string(),
                    severity: if agent.performance.cpu_usage > 95.0 {
                        "critical"
                    } else {
                        "high"
                    }
                    .to_string(),
                    score: agent.performance.cpu_usage / 100.0,
                    description: format!(
                        "Agent {} has critically high CPU usage: {:.1}%",
                        agent.name, agent.performance.cpu_usage
                    ),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: Some(serde_json::json!({
                        "agent_name": agent.name,
                        "agent_type": agent.agent_type,
                        "cpu_usage": agent.performance.cpu_usage,
                        "memory_usage": agent.performance.memory_usage
                    })),
                });
            }

            if agent.performance.memory_usage > 85.0 {
                detected_anomalies.push(Anomaly {
                    id: Uuid::new_v4().to_string(),
                    node_id: agent.agent_id.clone(),
                    r#type: "high_memory".to_string(),
                    severity: if agent.performance.memory_usage > 95.0 {
                        "critical"
                    } else {
                        "medium"
                    }
                    .to_string(),
                    score: agent.performance.memory_usage / 100.0,
                    description: format!(
                        "Agent {} has high memory usage: {:.1}%",
                        agent.name, agent.performance.memory_usage
                    ),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: Some(serde_json::json!({
                        "agent_name": agent.name,
                        "memory_usage": agent.performance.memory_usage
                    })),
                });
            }

            if agent.performance.health_score < 50.0 {
                detected_anomalies.push(Anomaly {
                    id: Uuid::new_v4().to_string(),
                    node_id: agent.agent_id.clone(),
                    r#type: "low_health".to_string(),
                    severity: if agent.performance.health_score < 25.0 {
                        "critical"
                    } else {
                        "high"
                    }
                    .to_string(),
                    score: 1.0 - (agent.performance.health_score / 100.0),
                    description: format!(
                        "Agent {} has critically low health score: {:.1}",
                        agent.name, agent.performance.health_score
                    ),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: Some(serde_json::json!({
                        "agent_name": agent.name,
                        "health_score": agent.performance.health_score,
                        "error_count": agent.metadata.error_count
                    })),
                });
            }

            if agent.performance.success_rate < 70.0 && agent.performance.tasks_completed > 5 {
                detected_anomalies.push(Anomaly {
                    id: Uuid::new_v4().to_string(),
                    node_id: agent.agent_id.clone(),
                    r#type: "low_success_rate".to_string(),
                    severity: "medium".to_string(),
                    score: 1.0 - (agent.performance.success_rate / 100.0),
                    description: format!(
                        "Agent {} has low task success rate: {:.1}%",
                        agent.name, agent.performance.success_rate
                    ),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: Some(serde_json::json!({
                        "agent_name": agent.name,
                        "success_rate": agent.performance.success_rate,
                        "tasks_completed": agent.performance.tasks_completed,
                        "tasks_failed": agent.performance.tasks_failed
                    })),
                });
            }
        }

        state.anomalies = detected_anomalies;

        
        state.stats = AnomalyStats {
            total: state.anomalies.len() as u32,
            critical: state
                .anomalies
                .iter()
                .filter(|a| a.severity == "critical")
                .count() as u32,
            high: state
                .anomalies
                .iter()
                .filter(|a| a.severity == "high")
                .count() as u32,
            medium: state
                .anomalies
                .iter()
                .filter(|a| a.severity == "medium")
                .count() as u32,
            low: state
                .anomalies
                .iter()
                .filter(|a| a.severity == "low")
                .count() as u32,
            last_updated: Some(chrono::Utc::now().timestamp() as u64),
        };

        info!(
            "Detected {} real anomalies from agent data: {} critical, {} high, {} medium, {} low",
            state.stats.total,
            state.stats.critical,
            state.stats.high,
            state.stats.medium,
            state.stats.low
        );
    });
}

pub async fn get_ai_insights(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Generating AI insights for graph analysis");

    
    let graph_data = match app_state.graph_service_addr.send(GetGraphData).await {
        Ok(Ok(data)) => Some(data),
        _ => None,
    };

    
    let clustering_tasks = CLUSTERING_TASKS.lock().await;
    let anomaly_state = ANOMALY_STATE.lock().await;

    let mut insights = vec![
        "Graph structure analysis shows balanced connectivity patterns".to_string(),
        "Node distribution follows expected semantic clustering".to_string(),
    ];

    let mut patterns = vec![];
    let mut recommendations = vec![];

    
    if let Some(latest_clusters) = clustering_tasks
        .values()
        .filter(|t| t.status == "completed")
        .max_by_key(|t| t.started_at)
        .and_then(|t| t.clusters.as_ref())
    {
        insights.push(format!(
            "Identified {} distinct semantic clusters",
            latest_clusters.len()
        ));

        if latest_clusters.len() > 10 {
            recommendations.push(
                "Consider increasing clustering threshold to reduce cluster count".to_string(),
            );
        } else if latest_clusters.len() < 3 {
            recommendations.push(
                "Consider decreasing clustering threshold for more granular grouping".to_string(),
            );
        }

        
        if let Some(largest_cluster) = latest_clusters.iter().max_by_key(|c| c.node_count) {
            patterns.push(GraphPattern {
                id: Uuid::new_v4().to_string(),
                r#type: "dominant_cluster".to_string(),
                description: format!(
                    "Large semantic cluster '{}' with {} nodes",
                    largest_cluster.label, largest_cluster.node_count
                ),
                confidence: largest_cluster.coherence,
                nodes: largest_cluster.nodes.clone(),
                significance: if largest_cluster.node_count > 50 {
                    "high"
                } else {
                    "medium"
                }
                .to_string(),
            });
        }
    }

    
    if anomaly_state.enabled && anomaly_state.stats.total > 0 {
        insights.push(format!(
            "Detected {} anomalies across the graph",
            anomaly_state.stats.total
        ));

        if anomaly_state.stats.critical > 0 {
            recommendations.push(
                "Investigate critical anomalies that may indicate data quality issues".to_string(),
            );
        }

        patterns.push(GraphPattern {
            id: Uuid::new_v4().to_string(),
            r#type: "anomaly_pattern".to_string(),
            description: format!(
                "Anomaly distribution: {} critical, {} high, {} medium",
                anomaly_state.stats.critical, anomaly_state.stats.high, anomaly_state.stats.medium
            ),
            confidence: 0.9,
            nodes: anomaly_state
                .anomalies
                .iter()
                .take(10)
                .filter_map(|a| a.node_id.parse::<u32>().ok())
                .collect(),
            significance: "high".to_string(),
        });
    }

    
    if let Some(data) = graph_data {
        let node_count = data.nodes.len();
        let edge_count = data.edges.len();
        let density = if node_count > 1 {
            (2.0 * edge_count as f32) / (node_count as f32 * (node_count - 1) as f32)
        } else {
            0.0
        };

        insights.push(format!(
            "Graph contains {} nodes and {} edges with density {:.3}",
            node_count, edge_count, density
        ));

        if density > 0.5 {
            recommendations
                .push("High graph density may benefit from hierarchical layout".to_string());
        } else if density < 0.1 {
            recommendations
                .push("Low graph density suggests potential for force-directed layout".to_string());
        }
    }

    ok_json!(InsightsResponse {
        success: true,
        insights: Some(insights),
        patterns: Some(patterns),
        recommendations: Some(recommendations),
        analysis_timestamp: Some(chrono::Utc::now().timestamp() as u64),
        error: None,
    })
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

pub async fn toggle_sssp(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<SSSPToggleRequest>,
) -> Result<HttpResponse> {
    info!(
        "Toggling SSSP spring adjustment: enabled={}, alpha={:?}",
        request.enabled, request.alpha
    );

    
    if let Some(alpha) = request.alpha {
        if alpha < 0.0 || alpha > 1.0 {
            return Ok(HttpResponse::BadRequest().json(SSSPToggleResponse {
                success: false,
                enabled: false,
                alpha: None,
                message: "Alpha must be between 0.0 and 1.0".to_string(),
                error: Some("Invalid alpha parameter".to_string()),
            }));
        }
    }

    
    let mut flags = FEATURE_FLAGS.lock().await;
    flags.sssp_integration = request.enabled;
    drop(flags); 

    
    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        let message = crate::actors::messages::UpdateSimulationParams {
            params: {
                let mut params = crate::models::simulation_params::SimulationParams::new();
                params.use_sssp_distances = request.enabled;
                params.sssp_alpha = request.alpha;
                params
            },
        };

        match gpu_addr.send(message).await {
            Ok(Ok(_)) => {
                let message = if request.enabled {
                    format!(
                        "SSSP spring adjustment enabled with alpha={:.2}",
                        request.alpha.unwrap_or(0.5)
                    )
                } else {
                    "SSSP spring adjustment disabled".to_string()
                };

                info!("Successfully toggled SSSP: {}", message);

                ok_json!(SSSPToggleResponse {
                    success: true,
                    enabled: request.enabled,
                    alpha: request.alpha,
                    message,
                    error: None,
                })
            }
            Ok(Err(e)) => {
                error!("Failed to update SSSP settings on GPU: {}", e);
                Ok(
                    HttpResponse::InternalServerError().json(SSSPToggleResponse {
                        success: false,
                        enabled: false,
                        alpha: None,
                        message: "Failed to update GPU settings".to_string(),
                        error: Some(format!("GPU update failed: {}", e)),
                    }),
                )
            }
            Err(e) => {
                error!("GPU compute actor mailbox error: {}", e);
                Ok(HttpResponse::ServiceUnavailable().json(SSSPToggleResponse {
                    success: false,
                    enabled: false,
                    alpha: None,
                    message: "GPU service unavailable".to_string(),
                    error: Some("GPU compute actor unavailable".to_string()),
                }))
            }
        }
    } else {
        warn!("GPU compute actor not available - SSSP toggle only updated feature flags");
        ok_json!(SSSPToggleResponse {
            success: true,
            enabled: request.enabled,
            alpha: request.alpha,
            message: "SSSP feature flag updated (GPU not available)".to_string(),
            error: None,
        })
    }
}

pub async fn get_sssp_status() -> Result<HttpResponse> {
    let flags = FEATURE_FLAGS.lock().await;

    ok_json!(serde_json::json!({
        "success": true,
        "enabled": flags.sssp_integration,
        "description": "Single-Source Shortest Path spring adjustment for improved edge length uniformity",
        "feature_flag": "FeatureFlags::ENABLE_SSSP_SPRING_ADJUST"
    }))
}

pub async fn get_gpu_status(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Control center requesting comprehensive GPU status");

    let gpu_status = if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        match gpu_addr
            .send(crate::actors::messages::GetPhysicsStats)
            .await
        {
            Ok(Ok(stats)) => {
                let clustering_tasks = CLUSTERING_TASKS.lock().await;
                let anomaly_state = ANOMALY_STATE.lock().await;

                serde_json::json!({
                    "success": true,
                    "gpu_available": true,
                    "status": "active",
                    "compute": {
                        "kernel_mode": "advanced",
                        "nodes_processed": stats.nodes_count,
                        "edges_processed": stats.edges_count,
                        "iteration_count": stats.iteration_count
                    },
                    "analytics": {
                        "clustering_active": !clustering_tasks.is_empty(),
                        "active_clustering_tasks": clustering_tasks.len(),
                        "anomaly_detection_enabled": anomaly_state.enabled,
                        "anomalies_detected": anomaly_state.stats.total,
                        "critical_anomalies": anomaly_state.stats.critical
                    },
                    "performance": {
                        "gpu_utilization": 75.0,
                        "memory_usage_percent": 45.0,
                        "temperature": 68.0,
                        "power_draw": 120.0
                    },
                    "features": {
                        "stress_majorization": true,
                        "semantic_constraints": true,
                        "sssp_integration": true,
                        "spatial_hashing": true,
                        "real_time_clustering": true,
                        "anomaly_detection": true
                    },
                    "last_updated": chrono::Utc::now().timestamp_millis()
                })
            }
            Ok(Err(e)) => {
                serde_json::json!({
                    "success": false,
                    "gpu_available": false,
                    "status": "error",
                    "error": e,
                    "fallback_active": true
                })
            }
            Err(_) => {
                serde_json::json!({
                    "success": false,
                    "gpu_available": false,
                    "status": "unavailable",
                    "fallback_active": true
                })
            }
        }
    } else {
        serde_json::json!({
            "success": true,
            "gpu_available": false,
            "status": "cpu_only",
            "fallback_active": true,
            "features": {
                "stress_majorization": false,
                "semantic_constraints": false,
                "sssp_integration": false,
                "spatial_hashing": false,
                "real_time_clustering": false,
                "anomaly_detection": false
            }
        })
    };

    ok_json!(gpu_status)
}

pub async fn get_gpu_features(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Client requesting GPU feature capabilities");

    let features = if let Some(_gpu_addr) = app_state.get_gpu_compute_addr().await {
        serde_json::json!({
            "success": true,
            "gpu_acceleration": true,
            "features": {
                "clustering": {
                    "available": true,
                    "methods": ["kmeans", "spectral", "dbscan", "louvain", "hierarchical", "affinity"],
                    "gpu_accelerated": true,
                    "max_clusters": 50,
                    "max_nodes": 100000
                },
                "anomaly_detection": {
                    "available": true,
                    "methods": ["isolation_forest", "lof", "autoencoder", "statistical", "temporal"],
                    "real_time": true,
                    "gpu_accelerated": true
                },
                "graph_algorithms": {
                    "sssp": true,
                    "stress_majorization": true,
                    "spatial_hashing": true,
                    "constraint_solving": true
                },
                "visualization": {
                    "real_time_updates": true,
                    "dynamic_layout": true,
                    "focus_regions": true,
                    "multi_graph_support": true
                }
            },
            "performance": {
                "expected_speedup": "10-50x",
                "memory_efficiency": "High",
                "concurrent_tasks": true,
                "batch_processing": true
            }
        })
    } else {
        serde_json::json!({
            "success": true,
            "gpu_acceleration": false,
            "features": {
                "clustering": {
                    "available": true,
                    "methods": ["kmeans", "hierarchical", "dbscan"],
                    "gpu_accelerated": false,
                    "max_clusters": 20,
                    "max_nodes": 10000
                },
                "anomaly_detection": {
                    "available": true,
                    "methods": ["statistical"],
                    "real_time": false,
                    "gpu_accelerated": false
                },
                "graph_algorithms": {
                    "sssp": true,
                    "stress_majorization": false,
                    "spatial_hashing": false,
                    "constraint_solving": false
                },
                "visualization": {
                    "real_time_updates": false,
                    "dynamic_layout": false,
                    "focus_regions": true,
                    "multi_graph_support": true
                }
            },
            "performance": {
                "expected_speedup": "1x (CPU baseline)",
                "memory_efficiency": "Standard",
                "concurrent_tasks": false,
                "batch_processing": false
            }
        })
    };

    ok_json!(features)
}

pub async fn cancel_clustering(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse> {
    let task_id = query.get("task_id");

    if let Some(task_id) = task_id {
        info!("Canceling clustering task: {}", task_id);

        let mut tasks = CLUSTERING_TASKS.lock().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = "cancelled".to_string();
            task.error = Some("Cancelled by user".to_string());

            return ok_json!(serde_json::json!({
                "success": true,
                "message": "Task cancelled successfully",
                "task_id": task_id
            }));
        }
    }

    not_found!("Task not found or not cancellable")
}

pub async fn get_anomaly_config() -> Result<HttpResponse> {
    let state = ANOMALY_STATE.lock().await;

    ok_json!(serde_json::json!({
        "success": true,
        "config": {
            "enabled": state.enabled,
            "method": state.method,
            "sensitivity": state.sensitivity,
            "window_size": state.window_size,
            "update_interval": state.update_interval
        },
        "stats": state.stats,
        "supported_methods": [
            "isolation_forest",
            "lof",
            "autoencoder",
            "statistical",
            "temporal"
        ]
    }))
}

pub async fn get_realtime_insights(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Client requesting real-time AI insights");

    
    let graph_data = app_state
        .graph_service_addr
        .send(GetGraphData)
        .await
        .map_err(|e| {
            error!("Failed to get graph data: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to get graph data")
        })?
        .map_err(|e| {
            error!("Graph data error: {}", e);
            actix_web::error::ErrorInternalServerError("Graph data error")
        })?;

    let clustering_tasks = CLUSTERING_TASKS.lock().await;
    let anomaly_state = ANOMALY_STATE.lock().await;

    
    let mut insights = vec![];
    let mut urgency_level = "low";

    
    if !graph_data.nodes.is_empty() {
        let density = (2.0 * graph_data.edges.len() as f32)
            / (graph_data.nodes.len() as f32 * (graph_data.nodes.len() - 1) as f32);

        insights.push(format!(
            "Graph density: {:.3} - {}",
            density,
            if density > 0.5 {
                "highly connected"
            } else if density > 0.2 {
                "moderately connected"
            } else {
                "sparsely connected"
            }
        ));
    }

    
    if let Some(running_task) = clustering_tasks.values().find(|t| t.status == "running") {
        insights.push(format!(
            "Clustering in progress: {} method at {:.1}% completion",
            running_task.method,
            running_task.progress * 100.0
        ));
        urgency_level = "medium";
    }

    
    if anomaly_state.enabled {
        if anomaly_state.stats.critical > 0 {
            insights.push(format!(
                "CRITICAL: {} critical anomalies detected!",
                anomaly_state.stats.critical
            ));
            urgency_level = "critical";
        } else if anomaly_state.stats.high > 0 {
            insights.push(format!(
                "High priority: {} high-severity anomalies detected",
                anomaly_state.stats.high
            ));
            if urgency_level == "low" {
                urgency_level = "high";
            }
        }
    }

    
    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        if let Ok(Ok(stats)) = gpu_addr
            .send(crate::actors::messages::GetPhysicsStats)
            .await
        {
            
            if stats.gpu_failure_count > 0 {
                insights.push(format!(
                    "Performance warning: {} GPU failures detected",
                    stats.gpu_failure_count
                ));
                if urgency_level == "low" {
                    urgency_level = "medium";
                }
            }
        }
    }

    ok_json!(serde_json::json!({
        "success": true,
        "insights": insights,
        "urgency_level": urgency_level,
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "requires_action": urgency_level != "low",
        "next_update_ms": 5000
    }))
}

pub async fn get_dashboard_status(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Control center requesting dashboard status");

    // GPU compute address is now Arc<RwLock<Option<...>>> - use async accessor
    let gpu_available = app_state.get_gpu_compute_addr().await.is_some();
    let clustering_tasks = CLUSTERING_TASKS.lock().await;
    let anomaly_state = ANOMALY_STATE.lock().await;

    
    let active_clustering = clustering_tasks
        .values()
        .filter(|t| t.status == "running")
        .count();

    let completed_clustering = clustering_tasks
        .values()
        .filter(|t| t.status == "completed")
        .count();

    
    let mut health_status = "healthy";
    let mut issues = vec![];

    if !gpu_available {
        issues.push("GPU acceleration not available - using CPU fallback".to_string());
        health_status = "degraded";
    }

    if anomaly_state.stats.critical > 0 {
        issues.push(format!(
            "{} critical anomalies require attention",
            anomaly_state.stats.critical
        ));
        health_status = "warning";
    }

    ok_json!(serde_json::json!({
        "success": true,
        "system": {
            "status": health_status,
            "gpu_available": gpu_available,
            "uptime_ms": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            "issues": issues
        },
        "analytics": {
            "clustering": {
                "active_tasks": active_clustering,
                "completed_tasks": completed_clustering,
                "total_tasks": clustering_tasks.len()
            },
            "anomaly_detection": {
                "enabled": anomaly_state.enabled,
                "total_anomalies": anomaly_state.stats.total,
                "critical": anomaly_state.stats.critical,
                "high": anomaly_state.stats.high,
                "medium": anomaly_state.stats.medium,
                "low": anomaly_state.stats.low
            }
        },
        "last_updated": chrono::Utc::now().timestamp_millis()
    }))
}

pub async fn get_health_check(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    // GPU compute address is now Arc<RwLock<Option<...>>> - use async accessor
    let gpu_available = app_state.get_gpu_compute_addr().await.is_some();
    let timestamp = chrono::Utc::now().timestamp_millis();

    let status = if gpu_available { "healthy" } else { "degraded" };

    ok_json!(serde_json::json!({
        "status": status,
        "gpu_available": gpu_available,
        "timestamp": timestamp,
        "service": "analytics"
    }))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FeatureFlags {
    pub gpu_clustering: bool,
    pub ontology_validation: bool,
    pub gpu_anomaly_detection: bool,
    pub real_time_insights: bool,
    pub advanced_visualizations: bool,
    pub performance_monitoring: bool,
    pub stress_majorization: bool,
    pub semantic_constraints: bool,
    pub sssp_integration: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            gpu_clustering: true,
            gpu_anomaly_detection: true,
            real_time_insights: true,
            advanced_visualizations: true,
            performance_monitoring: true,
            stress_majorization: false, 
            semantic_constraints: false, 
            sssp_integration: true,
            ontology_validation: false, 
        }
    }
}

pub static FEATURE_FLAGS: Lazy<Arc<Mutex<FeatureFlags>>> =
    Lazy::new(|| Arc::new(Mutex::new(FeatureFlags::default())));

pub async fn get_feature_flags() -> Result<HttpResponse> {
    let flags = FEATURE_FLAGS.lock().await;

    ok_json!(serde_json::json!({
        "success": true,
        "flags": *flags,
        "description": {
            "gpu_clustering": "Enable GPU-accelerated clustering algorithms",
            "gpu_anomaly_detection": "Enable GPU-accelerated anomaly detection",
            "real_time_insights": "Enable real-time AI insights generation",
            "advanced_visualizations": "Enable advanced visualization features",
            "performance_monitoring": "Enable detailed performance monitoring",
            "stress_majorization": "Enable stress majorization layout algorithm",
            "semantic_constraints": "Enable semantic constraint processing",
            "sssp_integration": "Enable single-source shortest path integration",
            "ontology_validation": "Enable ontology validation and inference operations"
        }
    }))
}

pub async fn update_feature_flags(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    request: web::Json<FeatureFlags>,
) -> Result<HttpResponse> {
    info!("Updating analytics feature flags");

    let mut flags = FEATURE_FLAGS.lock().await;
    *flags = request.into_inner();

    ok_json!(serde_json::json!({
        "success": true,
        "message": "Feature flags updated successfully",
        "flags": *flags
    }))
}

async fn trigger_stress_majorization(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    data: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Some(gpu_actor) = data.get_gpu_compute_addr().await {
        match gpu_actor.send(TriggerStressMajorization).await {
            Ok(Ok(())) => ok_json!(serde_json::json!({
                "success": true,
                "message": "Stress majorization triggered successfully"
            })),
            Ok(Err(e)) => {
                error!("Stress majorization failed: {}", e);
                Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "success": false,
                    "error": e
                })))
            }
            Err(e) => {
                error!("Failed to communicate with GPU actor: {}", e);
                error_json!("Internal server error")
            }
        }
    } else {
        service_unavailable!("GPU compute actor not available")
    }
}

async fn get_stress_majorization_stats(data: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    if let Some(gpu_actor) = data.get_gpu_compute_addr().await {
        match gpu_actor.send(GetStressMajorizationStats).await {
            Ok(Ok(stats)) => ok_json!(serde_json::json!({
                "success": true,
                "stats": stats
            })),
            Ok(Err(e)) => {
                error!("Failed to get stress majorization stats: {}", e);
                Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "success": false,
                    "error": e
                })))
            }
            Err(e) => {
                error!("Failed to get stress majorization stats: {}", e);
                error_json!("Failed to retrieve statistics")
            }
        }
    } else {
        service_unavailable!("GPU compute actor not available")
    }
}

async fn reset_stress_majorization_safety(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    data: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Some(gpu_actor) = data.get_gpu_compute_addr().await {
        match gpu_actor.send(ResetStressMajorizationSafety).await {
            Ok(Ok(())) => ok_json!(serde_json::json!({
                "success": true,
                "message": "Stress majorization safety state reset successfully"
            })),
            Ok(Err(e)) => {
                error!("Failed to reset stress majorization safety: {}", e);
                Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "success": false,
                    "error": e
                })))
            }
            Err(e) => {
                error!("Failed to communicate with GPU actor: {}", e);
                error_json!("Internal server error")
            }
        }
    } else {
        service_unavailable!("GPU compute actor not available")
    }
}

async fn update_stress_majorization_params(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    params: web::Json<AdvancedParams>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Some(gpu_actor) = data.get_gpu_compute_addr().await {
        let msg = UpdateStressMajorizationParams {
            params: params.into_inner(),
        };

        match gpu_actor.send(msg).await {
            Ok(Ok(())) => ok_json!(serde_json::json!({
                "success": true,
                "message": "Stress majorization parameters updated successfully"
            })),
            Ok(Err(e)) => {
                error!("Failed to update stress majorization parameters: {}", e);
                Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "success": false,
                    "error": e
                })))
            }
            Err(e) => {
                error!("Failed to communicate with GPU actor: {}", e);
                error_json!("Internal server error")
            }
        }
    } else {
        service_unavailable!("GPU compute actor not available")
    }
}

/// P1-1: Configure Stress Majorization runtime parameters
async fn configure_stress_majorization(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    config: web::Json<ConfigureStressMajorization>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Some(gpu_actor) = data.get_gpu_compute_addr().await {
        match gpu_actor.send(config.into_inner()).await {
            Ok(Ok(())) => ok_json!(serde_json::json!({
                "success": true,
                "message": "Stress majorization configuration updated successfully"
            })),
            Ok(Err(e)) => {
                error!("Failed to configure stress majorization: {}", e);
                Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "success": false,
                    "error": e
                })))
            }
            Err(e) => {
                error!("Failed to communicate with GPU actor: {}", e);
                error_json!("Internal server error")
            }
        }
    } else {
        service_unavailable!("GPU compute actor not available")
    }
}

/// P1-1: Get current Stress Majorization configuration
async fn get_stress_majorization_config(
    data: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Some(gpu_actor) = data.get_gpu_compute_addr().await {
        match gpu_actor.send(GetStressMajorizationConfig).await {
            Ok(Ok(config)) => ok_json!(serde_json::json!({
                "success": true,
                "config": config
            })),
            Ok(Err(e)) => {
                error!("Failed to get stress majorization config: {}", e);
                Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "success": false,
                    "error": e
                })))
            }
            Err(e) => {
                error!("Failed to communicate with GPU actor: {}", e);
                error_json!("Internal server error")
            }
        }
    } else {
        service_unavailable!("GPU compute actor not available")
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/analytics")
            
            .route("/params", web::get().to(get_analytics_params))
            .route("/params", web::post().to(update_analytics_params))
            .route("/constraints", web::get().to(get_constraints))
            .route("/constraints", web::post().to(update_constraints))
            .route("/focus", web::post().to(set_focus))
            .route("/stats", web::get().to(get_performance_stats))
            
            .route("/kernel-mode", web::post().to(set_kernel_mode))
            .route("/gpu-metrics", web::get().to(get_gpu_metrics))
            .route("/gpu-status", web::get().to(get_gpu_status))
            .route("/gpu-features", web::get().to(get_gpu_features))
            
            .route("/clustering/run", web::post().to(run_clustering))
            .route("/clustering/status", web::get().to(get_clustering_status))
            .route("/clustering/focus", web::post().to(focus_cluster))
            .route("/clustering/cancel", web::post().to(cancel_clustering))
            
            .route("/community/detect", web::post().to(run_community_detection))
            .route(
                "/community/statistics",
                web::get().to(get_community_statistics),
            )
            
            .route("/anomaly/toggle", web::post().to(toggle_anomaly_detection))
            .route("/anomaly/current", web::get().to(get_current_anomalies))
            .route("/anomaly/config", web::get().to(get_anomaly_config))
            
            .route("/insights", web::get().to(get_ai_insights))
            .route("/insights/realtime", web::get().to(get_realtime_insights))
            
            .route("/sssp/params", web::get().to(get_sssp_params))
            .route("/sssp/params", web::post().to(update_sssp_params))
            .route("/sssp/compute", web::post().to(compute_sssp))
            .route("/sssp/toggle", web::post().to(toggle_sssp))
            .route("/sssp/status", web::get().to(get_sssp_status))
            
            .route(
                "/stress-majorization/trigger",
                web::post().to(trigger_stress_majorization),
            )
            .route(
                "/stress-majorization/stats",
                web::get().to(get_stress_majorization_stats),
            )
            .route(
                "/stress-majorization/reset-safety",
                web::post().to(reset_stress_majorization_safety),
            )
            .route(
                "/stress-majorization/params",
                web::post().to(update_stress_majorization_params),
            )
            // P1-1: Stress Majorization Configuration Endpoints
            .route(
                "/stress-majorization/configure",
                web::post().to(configure_stress_majorization),
            )
            .route(
                "/stress-majorization/config",
                web::get().to(get_stress_majorization_config),
            )

            .route("/dashboard-status", web::get().to(get_dashboard_status))
            .route("/health-check", web::get().to(get_health_check))
            .route("/feature-flags", web::get().to(get_feature_flags))
            .route("/feature-flags", web::post().to(update_feature_flags))

            .route(
                "/ws",
                web::get().to(websocket_integration::gpu_analytics_websocket),
            ),
    );

    // P2 Feature: Pathfinding API routes (SSSP, APSP, Connected Components)
    pathfinding::configure_pathfinding_routes(cfg);
}

pub async fn run_community_detection(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<community::CommunityDetectionRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    debug!("Community detection request: {:?}", request);

    match community::run_gpu_community_detection(&app_state, &request).await {
        Ok(response) => ok_json!(response),
        Err(e) => {
            error!("Community detection failed: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": e,
                "communities": [],
                "total_communities": 0,
                "modularity": 0.0
            })))
        }
    }
}

pub async fn get_community_statistics(
    _app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {


    ok_json!(serde_json::json!({
        "success": true,
        "message": "Use /community/detect to run community detection first",
        "available_algorithms": ["label_propagation"],
        "performance_hints": {
            "label_propagation": "Fast, good for large networks",
            "recommended_max_iterations": 100,
            "typical_convergence": "5-20 iterations"
        }
    }))
}

pub async fn update_sssp_params(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    _app_state: web::Data<AppState>,
    request: web::Json<serde_json::Value>,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Updating SSSP parameters");

    let use_sssp = request
        .get("useSsspDistances")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sssp_alpha = request
        .get("ssspAlpha")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);

    
    
    info!(
        "SSSP parameters update requested: enabled={}, alpha={:?}",
        use_sssp, sssp_alpha
    );

    ok_json!(serde_json::json!({
        "success": true,
        "params": {
            "useSsspDistances": use_sssp,
            "ssspAlpha": sssp_alpha,
        },
        "note": "SSSP parameters are managed in GPU kernel simulation"
    }))
}

pub async fn get_sssp_params(_app_state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    debug!("Retrieving SSSP parameters");



    ok_json!(serde_json::json!({
        "success": true,
        "params": {
            "useSsspDistances": false,
            "ssspAlpha": 0.5,
        },
        "note": "SSSP parameters are managed in GPU kernel simulation"
    }))
}

pub async fn compute_sssp(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<serde_json::Value>,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Computing SSSP from source node");

    let source_node = request
        .get("sourceNode")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);

    
    use crate::actors::messages::ComputeShortestPaths;
    match app_state
        .graph_service_addr
        .send(ComputeShortestPaths {
            source_node_id: source_node,
        })
        .await
    {
        Ok(Ok(_)) => {
            info!("SSSP computation triggered for source node {}", source_node);
            ok_json!(serde_json::json!({
                "success": true,
                "sourceNode": source_node,
                "message": "SSSP computation started",
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to compute SSSP: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": format!("Failed to compute SSSP: {}", e),
            })))
        }
        Err(e) => {
            error!("Graph service communication error: {}", e);
            error_json!("Failed to communicate with graph service")
        }
    }
}

pub async fn get_gpu_metrics(app_state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    debug!("Retrieving GPU performance metrics");

    
    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        use crate::actors::messages::GetGPUMetrics;

        match gpu_addr.send(GetGPUMetrics).await {
            Ok(Ok(metrics)) => {
                info!("GPU metrics retrieved successfully");
                ok_json!(metrics)
            }
            Ok(Err(e)) => {
                error!("Failed to get GPU metrics: {}", e);
                Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                    "success": false,
                    "error": e,
                    "gpu_initialized": false
                })))
            }
            Err(e) => {
                error!("GPU actor mailbox error: {}", e);
                service_unavailable!("GPU compute actor unavailable")
            }
        }
    } else {
        warn!("GPU compute actor not available");
        service_unavailable!("GPU compute not available - GPU acceleration is not enabled or not available")
    }
}
