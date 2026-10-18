// src/handlers/physics_handler.rs
//! Physics API Handlers
//!
//! HTTP handlers for physics simulation endpoints using PhysicsService.

use crate::settings::auth_extractor::AuthenticatedUser;
use crate::AppState;
use crate::{error_json, ok_json};
use actix_web::{web, HttpResponse, Result as ActixResult};
use log::warn;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use actix::Addr;

use crate::application::physics_service::{
    LayoutOptimizationRequest, PhysicsService, SimulationParams,
};
use crate::models::simulation_params::SettleMode;
use visionclaw_domain::ports::gpu_physics_adapter::PhysicsParameters;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// Controls simulation convergence behavior. If omitted, defaults to FastSettle.
    pub settle_mode: Option<SettleMode>,
}

#[derive(Debug, Serialize)]
pub struct StartSimulationResponse {
    pub simulation_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationStatusResponse {
    pub simulation_id: Option<String>,
    pub running: bool,
    pub gpu_status: Option<GpuStatusInfo>,
    pub statistics: Option<StatisticsInfo>,
    pub settle_mode: SettleMode,
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

/// Most iterations one caller may request; the routes are power-user gated,
/// but an unbounded `u32` still let one request hold the GPU (2026-10-18
/// security review).
pub const MAX_PHYSICS_ITERATIONS: u32 = 10_000;

pub async fn start_simulation(
    user: AuthenticatedUser,
    physics_service: web::Data<Arc<PhysicsService>>,
    graph_service: web::Data<Addr<GraphServiceSupervisor>>,
    req: web::Json<StartSimulationRequest>,
) -> ActixResult<HttpResponse> {
    user.require_power_user()?;
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };

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
        params.max_iterations = v.min(MAX_PHYSICS_ITERATIONS);
    }

    let sim_params = SimulationParams {
        profile_name: req
            .profile_name
            .clone()
            .unwrap_or_else(|| "default".to_string()),
        physics_params: params,
        auto_stop_on_convergence: req.auto_stop_on_convergence.unwrap_or(true),
        settle_mode: req.settle_mode.clone().unwrap_or_default(),
    };

    match physics_service.start_simulation(graph, sim_params).await {
        Ok(simulation_id) => ok_json!(StartSimulationResponse {
            simulation_id,
            status: "started".to_string(),
        }),
        Err(e) => error_json!("Failed to start simulation: {}", e),
    }
}

pub async fn stop_simulation(
    user: AuthenticatedUser,
    physics_service: web::Data<Arc<PhysicsService>>,
) -> ActixResult<HttpResponse> {
    user.require_power_user()?;
    match physics_service.stop_simulation().await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "stopped"
        })),
        Err(e) => error_json!("Failed to stop simulation: {}", e),
    }
}

pub async fn get_status(app_state: web::Data<AppState>) -> ActixResult<HttpResponse> {
    // Query GPU compute actor availability from AppState
    let gpu_available = app_state.get_gpu_compute_addr().await.is_some();

    let gpu_status = if gpu_available {
        // GPU is available but detailed status requires PhysicsService adapter
        // which may not be registered. Report availability.
        Some(GpuStatusInfo {
            device_name: "GPU compute actor active".to_string(),
            compute_capability: "N/A".to_string(),
            total_memory_mb: 0,
            free_memory_mb: 0,
        })
    } else {
        warn!("GPU compute actor not available for physics status query");
        None
    };

    ok_json!(SimulationStatusResponse {
        simulation_id: None,
        running: gpu_available,
        gpu_status,
        statistics: None,
        settle_mode: SettleMode::default(),
    })
}

pub async fn optimize_layout(
    physics_service: web::Data<Arc<PhysicsService>>,
    graph_service: web::Data<Addr<GraphServiceSupervisor>>,
    req: web::Json<OptimizeLayoutRequest>,
) -> ActixResult<HttpResponse> {
    let graph = match crate::handlers::graph_access::current_graph(&graph_service).await {
        Ok(g) => g,
        Err(e) => return error_json!("Failed to read the graph: {}", e),
    };

    let optimization_req = LayoutOptimizationRequest {
        algorithm: req.algorithm.clone(),
        max_iterations: req
            .max_iterations
            .unwrap_or(1000)
            .min(MAX_PHYSICS_ITERATIONS),
        target_energy: req.target_energy.unwrap_or(0.01),
    };

    match physics_service
        .optimize_layout(graph, optimization_req)
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
    user: AuthenticatedUser,
    physics_service: web::Data<Arc<PhysicsService>>,
    req: web::Json<ApplyForcesRequest>,
) -> ActixResult<HttpResponse> {
    user.require_power_user()?;
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
    user: AuthenticatedUser,
    physics_service: web::Data<Arc<PhysicsService>>,
    req: web::Json<PinNodesRequest>,
) -> ActixResult<HttpResponse> {
    user.require_power_user()?;
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
    user: AuthenticatedUser,
    physics_service: web::Data<Arc<PhysicsService>>,
    req: web::Json<UpdateParametersRequest>,
) -> ActixResult<HttpResponse> {
    user.require_power_user()?;
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
    user: AuthenticatedUser,
    physics_service: web::Data<Arc<PhysicsService>>,
) -> ActixResult<HttpResponse> {
    user.require_power_user()?;
    match physics_service.reset().await {
        Ok(_) => ok_json!(serde_json::json!({
            "status": "reset"
        })),
        Err(e) => error_json!("Failed to reset simulation: {}", e),
    }
}

/// Request body for setting the settle mode.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSettleModeRequest {
    pub settle_mode: SettleMode,
}

/// Response for settle mode queries.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleModeResponse {
    pub settle_mode: SettleMode,
}

/// GET /physics/settle-mode -- return the default settle mode configuration.
/// In a full integration this would read from the running simulation state;
/// for now it returns the default so clients can discover the schema.
pub async fn get_settle_mode() -> ActixResult<HttpResponse> {
    ok_json!(SettleModeResponse {
        settle_mode: SettleMode::default(),
    })
}

/// POST /physics/settle-mode -- validate and echo back the requested mode.
/// The actual mode is set when starting a simulation via `/physics/start`.
pub async fn set_settle_mode(req: web::Json<SetSettleModeRequest>) -> ActixResult<HttpResponse> {
    // Validate FastSettle parameters if applicable.
    if let SettleMode::FastSettle {
        damping_override,
        max_settle_iterations,
        energy_threshold,
    } = &req.settle_mode
    {
        if *damping_override <= 0.0 || *damping_override > 1.0 {
            return error_json!(
                "damping_override must be in (0.0, 1.0], got {}",
                damping_override
            );
        }
        if *max_settle_iterations == 0 {
            return error_json!("max_settle_iterations must be > 0");
        }
        if *energy_threshold <= 0.0 {
            return error_json!("energy_threshold must be > 0.0, got {}", energy_threshold);
        }
    }

    ok_json!(SettleModeResponse {
        settle_mode: req.settle_mode.clone(),
    })
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
            .route("/reset", web::post().to(reset_simulation))
            .route("/settle-mode", web::get().to(get_settle_mode))
            .route("/settle-mode", web::post().to(set_settle_mode)),
    );
}

#[cfg(test)]
mod tests {
    use super::MAX_PHYSICS_ITERATIONS;

    #[test]
    fn iteration_cap_is_bounded_and_above_the_default() {
        assert!(
            MAX_PHYSICS_ITERATIONS >= 1000,
            "the default of 1000 must fit"
        );
        assert_eq!(u32::MAX.min(MAX_PHYSICS_ITERATIONS), MAX_PHYSICS_ITERATIONS);
    }
}
