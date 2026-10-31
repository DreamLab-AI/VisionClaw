// Physics-related handlers and GPU propagation logic

use crate::actors::messages::{
    ForceResumePhysics, GetSettings, UpdateClusteringParams, UpdateSettings, UpdateSimulationParams,
};
use crate::app_state::AppState;
use crate::config::AppFullSettings;
use crate::{bad_request, error_json, ok_json, service_unavailable};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use log::{debug, error, info, warn};
use serde_json::{json, Value};

use super::helpers::create_physics_settings_update;
use super::validation::validate_constraints;

/// Propagate a graph's stored physics to the GPU actors. The device values are the stored
/// document resolved by `From<&PhysicsSettings> for SimulationParams` (ADR-2140); this function
/// applies no layout-mode override of its own.
pub async fn propagate_physics_to_gpu(
    state: &web::Data<AppState>,
    settings: &AppFullSettings,
    graph: &str,
) {
    let physics = settings.get_physics(graph);

    info!("[PHYSICS UPDATE] Propagating {} physics to actors:", graph);
    info!(
        "  - repulsion_k: {:.3} (affects node spreading)",
        physics.repel_k
    );
    info!(
        "  - spring_k: {:.3} (affects edge tension)",
        physics.spring_k
    );
    info!("  - spring_k: {:.3} (affects clustering)", physics.spring_k);
    info!(
        "  - damping: {:.3} (affects settling, 1.0 = no movement)",
        physics.damping
    );
    info!("  - time_step: {:.3} (simulation speed)", physics.dt);
    info!(
        "  - max_velocity: {:.3} (prevents explosions)",
        physics.max_velocity
    );
    info!(
        "  - temperature: {:.3} (random motion)",
        physics.temperature
    );
    info!("  - gravity: {:.3} (directional force)", physics.gravity);

    if crate::utils::logging::is_debug_enabled() {
        debug!("  - bounds_size: {:.1}", physics.bounds_size);
        debug!("  - separation_radius: {:.3}", physics.separation_radius);
        debug!("  - boundary_damping: {:.3}", physics.boundary_damping);
        debug!("  - iterations: {}", physics.iterations);
        debug!("  - enabled: {}", physics.enabled);

        debug!("  - sssp_alpha: {:.3}", physics.sssp_alpha);
        debug!("  - max_repulsion_dist: {:.1}", physics.max_repulsion_dist);
        debug!("  - warmup_iterations: {}", physics.warmup_iterations);
        debug!("  - cooling_rate: {:.6}", physics.cooling_rate);
        debug!("  - clustering_algorithm: {}", physics.clustering_algorithm);
        debug!("  - cluster_count: {}", physics.cluster_count);
        debug!(
            "  - clustering_resolution: {:.3}",
            physics.clustering_resolution
        );
        debug!(
            "  - clustering_iterations: {}",
            physics.clustering_iterations
        );
        debug!("[GPU Parameters] All new parameters available for GPU processing");
    }

    let sim_params: crate::models::simulation_params::SimulationParams = physics.into();

    info!(
        "[PHYSICS UPDATE] Converted to SimulationParams - repulsion: {}, damping: {:.3}, time_step: {:.3}, layout: {}",
        sim_params.repel_k, sim_params.damping, sim_params.dt, sim_params.layout_mode
    );

    let update_msg = UpdateSimulationParams { params: sim_params };

    // Single dispatch path (resolved 2026-06-03): send UpdateSimulationParams ONLY
    // via the GraphServiceSupervisor → PhysicsOrchestratorActor route. The orchestrator
    // owns the warmup reset (stability_warmup_remaining = 1800) and reheat
    // (force_compute_actor.rs ~2188/2195) and forwards the message to the
    // ForceComputeActor itself (physics_orchestrator_actor.rs:1478-1479). The previous
    // direct dispatch to state.get_gpu_compute_addr() reached the SAME ForceComputeActor
    // handler, so every settings change triggered a DOUBLE warmup reset / double reheat,
    // fighting the "settle then back off" model. The orchestrator is the reliable
    // delivery path (it always holds the GPU address from StoreGPUComputeAddress).
    info!("[PHYSICS UPDATE] Sending to GraphServiceActor (orchestrator dispatch)...");
    if let Err(e) = state.graph_service_addr.send(update_msg).await {
        error!("[PHYSICS UPDATE] FAILED to update GraphServiceActor: {}", e);
    } else {
        info!("[PHYSICS UPDATE] GraphServiceActor updated successfully");
    }

    // Force-resume physics so updated parameters take effect even if simulation
    // auto-paused at equilibrium.
    info!("[PHYSICS UPDATE] Sending ForceResumePhysics...");
    if let Err(e) = state
        .graph_service_addr
        .send(ForceResumePhysics {
            reason: format!("Physics propagated for graph '{}'", graph),
        })
        .await
    {
        warn!("[PHYSICS UPDATE] Failed to send ForceResumePhysics: {}", e);
    } else {
        info!("[PHYSICS UPDATE] ForceResumePhysics sent successfully");
    }

    // Propagate the live community-cohesion detector params to the GPU engine.
    // These drive refresh_community_cohesion_labels (Leiden/Louvain partition)
    // for the Community Cohesion force — independent of analytics clustering.
    if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
        gpu_addr.do_send(UpdateClusteringParams {
            algorithm: physics.clustering_algorithm.clone(),
            resolution: physics.clustering_resolution,
            iterations: physics.clustering_iterations,
        });
    }
}

pub async fn update_compute_mode(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();

    info!("Compute mode update request received");
    debug!(
        "Compute mode payload: {}",
        serde_json::to_string_pretty(&update).unwrap_or_default()
    );

    let compute_mode = update
        .get("computeMode")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            actix_web::error::ErrorBadRequest("computeMode must be an integer between 0 and 3")
        })?;

    if compute_mode > 3 {
        return bad_request!("computeMode must be between 0 and 3");
    }

    let physics_update = json!({
        "computeMode": compute_mode
    });

    let settings_update = create_physics_settings_update(physics_update);

    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    if let Err(e) = app_settings.merge_update(settings_update) {
        error!("Failed to merge compute mode settings: {}", e);
        return error_json!("Failed to update compute mode: {}", e);
    }

    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Compute mode updated successfully to: {}", compute_mode);

            propagate_physics_to_gpu(&state, &app_settings, "knowledge").await;
            propagate_physics_to_gpu(&state, &app_settings, "visionclaw").await;

            ok_json!(json!({
                "status": "Compute mode updated successfully",
                "computeMode": compute_mode
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to save compute mode settings: {}", e);
            error_json!("Failed to save compute mode settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

pub async fn update_constraints(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();

    info!("Constraints update request received");
    debug!(
        "Constraints payload: {}",
        serde_json::to_string_pretty(&update).unwrap_or_default()
    );

    if let Err(e) = validate_constraints(&update) {
        return bad_request!("Invalid constraints: {}", e);
    }

    let settings_update = json!({
        "visualisation": {
            "graphs": {
                "knowledge": {
                    "physics": {
                        "computeMode": 2
                    }
                },
                "visionclaw": {
                    "physics": {
                        "computeMode": 2
                    }
                }
            }
        }
    });

    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    if let Err(e) = app_settings.merge_update(settings_update) {
        error!("Failed to merge constraints settings: {}", e);
        return error_json!("Failed to update constraints: {}", e);
    }

    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Constraints updated successfully");

            propagate_physics_to_gpu(&state, &app_settings, "knowledge").await;
            propagate_physics_to_gpu(&state, &app_settings, "visionclaw").await;

            ok_json!(json!({
                "status": "Constraints updated successfully"
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to save constraints settings: {}", e);
            error_json!("Failed to save constraints settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

pub async fn update_stress_optimization(
    _req: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let update = payload.into_inner();

    info!("Stress optimization update request received");
    debug!(
        "Stress optimization payload: {}",
        serde_json::to_string_pretty(&update).unwrap_or_default()
    );

    let stress_weight = update
        .get("stressWeight")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1) as f32;

    let stress_alpha = update
        .get("stressAlpha")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.1) as f32;

    if !(0.0..=1.0).contains(&stress_weight) || !(0.0..=1.0).contains(&stress_alpha) {
        return bad_request!("stressWeight and stressAlpha must be between 0.0 and 1.0");
    }

    let physics_update = json!({
        "stressWeight": stress_weight,
        "stressAlpha": stress_alpha
    });

    let settings_update = create_physics_settings_update(physics_update);

    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!("Failed to get current settings: {}", e);
            return error_json!("Failed to get current settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    if let Err(e) = app_settings.merge_update(settings_update) {
        error!("Failed to merge stress optimization settings: {}", e);
        return error_json!("Failed to update stress optimization: {}", e);
    }

    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Stress optimization updated successfully");

            propagate_physics_to_gpu(&state, &app_settings, "knowledge").await;
            propagate_physics_to_gpu(&state, &app_settings, "visionclaw").await;

            ok_json!(json!({
                "status": "Stress optimization updated successfully",
                "stressWeight": stress_weight,
                "stressAlpha": stress_alpha
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to save stress optimization settings: {}", e);
            error_json!("Failed to save stress optimization settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}
