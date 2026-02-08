// src/handlers/physics_handler.rs
//! Physics API Handlers
//!
//! HTTP handlers for physics simulation endpoints using PhysicsService.

use actix_web::{web, HttpResponse, Result as ActixResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{ok_json, error_json};

use crate::application::physics_service::{
    LayoutOptimizationRequest, PhysicsService, SimulationParams,
};
use crate::models::graph::GraphData;
use crate::ports::gpu_physics_adapter::PhysicsParameters;

#[derive(Debug, Deserialize)]
pub struct StartSimulationRequest {
    pub profile_name: Option<String>,
    pub time_step: Option<f32>,
    pub damping: Option<f32>,
    pub spring_constant: Option<f32>,
    pub repulsion_strength: Option<f32>,
    pub attraction_strength: Option<f32>,
    pub max_velocity: Option<f32>,
    pub convergence_threshold: Option<f32>,
    pub max_iterations: Option<u32>,
    pub auto_stop_on_convergence: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct StartSimulationResponse {
    pub simulation_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct SimulationStatusResponse {
    pub simulation_id: Option<String>,
    pub running: bool,
    pub gpu_status: Option<GpuStatusInfo>,
    pub statistics: Option<StatisticsInfo>,
}

#[derive(Debug, Serialize)]
pub struct GpuStatusInfo {
    pub device_name: String,
    pub compute_capability: String,
    pub total_memory_mb: usize,
    pub free_memory_mb: usize,
}

#[derive(Debug, Serialize)]
pub struct StatisticsInfo {
    pub total_steps: u64,
    pub average_step_time_ms: f32,
    pub average_energy: f32,
    pub gpu_memory_used_mb: f32,
}

#[derive(Debug, Deserialize)]
pub struct OptimizeLayoutRequest {
    pub algorithm: String,
    pub max_iterations: Option<u32>,
    pub target_energy: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct OptimizeLayoutResponse {
    pub nodes_updated: usize,
    pub optimization_score: f64,
}

#[derive(Debug, Deserialize)]
pub struct ApplyForcesRequest {
    pub forces: Vec<NodeForceInput>,
}

#[derive(Debug, Deserialize)]
pub struct NodeForceInput {
    pub node_id: u32,
    pub force_x: f32,
    pub force_y: f32,
    pub force_z: f32,
}

#[derive(Debug, Deserialize)]
pub struct PinNodesRequest {
    pub nodes: Vec<NodePositionInput>,
}

#[derive(Debug, Deserialize)]
pub struct NodePositionInput {
    pub node_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Deserialize)]
pub struct UpdateParametersRequest {
    pub time_step: Option<f32>,
    pub damping: Option<f32>,
    pub spring_constant: Option<f32>,
    pub repulsion_strength: Option<f32>,
    pub attraction_strength: Option<f32>,
    pub max_velocity: Option<f32>,
}

pub async fn start_simulation(
    physics_service: web::Data<Arc<PhysicsService>>,
    graph_data: web::Data<Arc<RwLock<GraphData>>>,
    req: web::Json<StartSimulationRequest>,
) -> ActixResult<HttpResponse> {
    let graph = graph_data.read().await.clone();

    
    let mut params = PhysicsParameters::default();
    if let Some(v) = req.time_step {
        params.time_step = v;
    }
    if let Some(v) = req.damping {
        params.damping = v;
    }
    if let Some(v) = req.spring_constant {
        params.spring_constant = v;
    }
    if let Some(v) = req.repulsion_strength {
        params.repulsion_strength = v;
    }
    if let Some(v) = req.attraction_strength {
        params.attraction_strength = v;
    }
    if let Some(v) = req.max_velocity {
        params.max_velocity = v;
    }
    if let Some(v) = req.convergence_threshold {
        params.convergence_threshold = v;
    }
    if let Some(v) = req.max_iterations {
        params.max_iterations = v;
    }

    let sim_params = SimulationParams {
        profile_name: req
            .profile_name
            .clone()
            .unwrap_or_else(|| "default".to_string()),
        physics_params: params,
        auto_stop_on_convergence: req.auto_stop_on_convergence.unwrap_or(true),
    };

    match physics_service
        .start_simulation(Arc::new(graph), sim_params)
        .await
    {
        Ok(simulation_id) => ok_json!(StartSimulationResponse {
            simulation_id,
            status: "started".to_string(),
        }),
        Err(e) => error_json!("Failed to start simulation: {}", e),
    }
}

pub async fn stop_simulation(
    physics_service: web::Data<Arc<PhysicsService>>,
) -> ActixResult<HttpResponse> {
    match physics_service.stop_simulation().await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "stopped"
        })),
        Err(e) => error_json!("Failed to stop simulation: {}", e),
    }
}

pub async fn get_status(
    physics_service: web::Data<Arc<PhysicsService>>,
) -> ActixResult<HttpResponse> {
    let running = physics_service.is_running().await;
    let simulation_id = physics_service.get_simulation_id().await;

    let gpu_status = physics_service
        .get_gpu_status()
        .await
        .ok()
        .map(|s| GpuStatusInfo {
            device_name: s.device_name,
            compute_capability: format!("{}.{}", s.compute_capability.0, s.compute_capability.1),
            total_memory_mb: s.total_memory_mb,
            free_memory_mb: s.free_memory_mb,
        });

    let statistics = physics_service
        .get_statistics()
        .await
        .ok()
        .map(|s| StatisticsInfo {
            total_steps: s.total_steps,
            average_step_time_ms: s.average_step_time_ms,
            average_energy: s.average_energy,
            gpu_memory_used_mb: s.gpu_memory_used_mb,
        });

    ok_json!(SimulationStatusResponse {
        simulation_id,
        running,
        gpu_status,
        statistics,
    })
}

pub async fn optimize_layout(
    physics_service: web::Data<Arc<PhysicsService>>,
    graph_data: web::Data<Arc<RwLock<GraphData>>>,
    req: web::Json<OptimizeLayoutRequest>,
) -> ActixResult<HttpResponse> {
    let graph = graph_data.read().await.clone();

    let optimization_req = LayoutOptimizationRequest {
        algorithm: req.algorithm.clone(),
        max_iterations: req.max_iterations.unwrap_or(1000),
        target_energy: req.target_energy.unwrap_or(0.01),
    };

    match physics_service
        .optimize_layout(Arc::new(graph), optimization_req)
        .await
    {
        Ok(nodes) => ok_json!(OptimizeLayoutResponse {
            nodes_updated: nodes.len(),
            optimization_score: 0.0,
        }),
        Err(e) => error_json!("Failed to optimize layout: {}", e),
    }
}

pub async fn perform_step(
    physics_service: web::Data<Arc<PhysicsService>>,
) -> ActixResult<HttpResponse> {
    match physics_service.step().await {
        Ok(result) => ok_json!(serde_json::json!({
            "nodes_updated": result.nodes_updated,
            "total_energy": result.total_energy,
            "max_displacement": result.max_displacement,
            "converged": result.converged,
            "computation_time_ms": result.computation_time_ms,
        })),
        Err(e) => error_json!("Failed to perform step: {}", e),
    }
}

pub async fn apply_forces(
    physics_service: web::Data<Arc<PhysicsService>>,
    req: web::Json<ApplyForcesRequest>,
) -> ActixResult<HttpResponse> {
    let forces: Vec<_> = req
        .forces
        .iter()
        .map(|f| (f.node_id, f.force_x, f.force_y, f.force_z))
        .collect();

    match physics_service.apply_external_forces(forces).await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "applied"
        })),
        Err(e) => error_json!("Failed to apply forces: {}", e),
    }
}

pub async fn pin_nodes(
    physics_service: web::Data<Arc<PhysicsService>>,
    req: web::Json<PinNodesRequest>,
) -> ActixResult<HttpResponse> {
    let nodes: Vec<_> = req
        .nodes
        .iter()
        .map(|n| (n.node_id, n.x, n.y, n.z))
        .collect();

    match physics_service.pin_nodes(nodes).await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "pinned"
        })),
        Err(e) => error_json!("Failed to pin nodes: {}", e),
    }
}

pub async fn unpin_nodes(
    physics_service: web::Data<Arc<PhysicsService>>,
    req: web::Json<Vec<u32>>,
) -> ActixResult<HttpResponse> {
    match physics_service.unpin_nodes(req.into_inner()).await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "unpinned"
        })),
        Err(e) => error_json!("Failed to unpin nodes: {}", e),
    }
}

pub async fn update_parameters(
    physics_service: web::Data<Arc<PhysicsService>>,
    req: web::Json<UpdateParametersRequest>,
) -> ActixResult<HttpResponse> {
    let mut params = PhysicsParameters::default();

    if let Some(v) = req.time_step {
        params.time_step = v;
    }
    if let Some(v) = req.damping {
        params.damping = v;
    }
    if let Some(v) = req.spring_constant {
        params.spring_constant = v;
    }
    if let Some(v) = req.repulsion_strength {
        params.repulsion_strength = v;
    }
    if let Some(v) = req.attraction_strength {
        params.attraction_strength = v;
    }
    if let Some(v) = req.max_velocity {
        params.max_velocity = v;
    }

    match physics_service.update_parameters(params).await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "updated"
        })),
        Err(e) => error_json!("Failed to update parameters: {}", e),
    }
}

pub async fn reset_simulation(
    physics_service: web::Data<Arc<PhysicsService>>,
) -> ActixResult<HttpResponse> {
    match physics_service.reset().await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "reset"
        })),
        Err(e) => error_json!("Failed to reset simulation: {}", e),
    }
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/physics")
            .route("/start", web::post().to(start_simulation))
            .route("/stop", web::post().to(stop_simulation))
            .route("/status", web::get().to(get_status))
            .route("/optimize", web::post().to(optimize_layout))
            .route("/step", web::post().to(perform_step))
            .route("/forces/apply", web::post().to(apply_forces))
            .route("/nodes/pin", web::post().to(pin_nodes))
            .route("/nodes/unpin", web::post().to(unpin_nodes))
            .route("/parameters", web::post().to(update_parameters))
            .route("/reset", web::post().to(reset_simulation)),
    );
}
