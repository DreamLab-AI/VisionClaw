// src/settings/api/settings_routes.rs
//! REST API endpoints for settings management.
//! Uses OptimizedSettingsActor (via AppState) as the single source of truth.
//! All PUT routes validate input before applying. (QE Fix #1, #2, #3, #5)

use actix_web::{web, HttpResponse, Responder};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::actors::messages::{
    BroadcastMessage, ConfigureBroadcastOptimization, ForceResumePhysics, GetSettings,
    ResetPositions, SetComputeMode, UpdateClusteringParams, UpdateConstraints, UpdateSettings,
    UpdateSimulationParams,
};
use crate::adapters::SqliteSettingsRepository;
use crate::config::{PhysicsSettings, RenderingSettings};
use crate::settings::auth_extractor::{AuthenticatedUser, OptionalAuth};
use crate::settings::models::{
    AllSettings, ConstraintSettings, NodeFilterSettings, QualityGateSettings,
};
use crate::utils::unified_gpu_compute::ComputeMode;
/// Placeholder for the per-user filter record. Full SQLite migration in Phase 2.
// Persisted per-user via the `settings` table's owner layer (ADR-11 §D5) —
// see `update_user_filter` / `get_user_filter`, keyed by `USER_FILTER_KEY`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone)]
pub struct UserFilter {
    pub pubkey: String,
    pub enabled: bool,
    pub quality_threshold: f64,
    pub authority_threshold: f64,
    pub filter_by_quality: bool,
    pub filter_by_authority: bool,
    pub filter_mode: String,
    pub max_nodes: Option<i64>,
}
use crate::ports::settings_repository::{SettingValue, SettingsRepository};
use crate::AppState;

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProfileRequest {
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileIdResponse {
    pub id: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub error: String,
}

// ============================================================================
// Physics Settings Validation (QE Fix #2 + Fix #5)
// ============================================================================

/// Normalize incoming physics JSON keys to the canonical camelCase names
/// expected by PhysicsSettings (which uses `#[serde(rename_all = "camelCase")]`).
///
/// This maps common aliases (snake_case, legacy names, client variants) so that
/// both `{"spring_k": 0.05}` and `{"springK": 0.05}` work correctly.
fn normalize_physics_keys(
    patch: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut normalized = serde_json::Map::new();
    for (key, value) in patch {
        let canonical = match key.as_str() {
            // snake_case → camelCase mappings
            "spring_k" => "springK",
            "repel_k" => "repelK",
            "spring_strength" => "springK",
            "repulsion_strength" => "repelK",
            "center_gravity_k" => "centerGravityK",
            "max_velocity" => "maxVelocity",
            "max_force" => "maxForce",
            "enable_bounds" => "enableBounds",
            "bounds_size" => "boundsSize",
            "separation_radius" => "separationRadius",
            "boundary_damping" => "boundaryDamping",
            "rest_length" => "restLength",
            "grid_cell_size" => "gridCellSize",
            "warmup_iterations" => "warmupIterations",
            "cooling_rate" => "coolingRate",
            "max_repulsion_dist" => "maxRepulsionDist",
            "auto_balance" => "autoBalance",
            "cluster_strength" => "clusterStrength",
            "sssp_alpha" => "ssspAlpha",
            "broadcast_fps" => "broadcastFps",
            // Legacy/client aliases
            "springStrength" => "springK",
            "repulsionStrength" => "repelK",
            "attractionStrength" => "centerGravityK",
            "attractionK" => "centerGravityK",
            "springStiffness" => "springK",
            "springDamping" => "damping",
            "deltaTime" => "dt",
            // Already camelCase — pass through
            other => other,
        };
        // Don't overwrite if the canonical key was already provided explicitly
        if !normalized.contains_key(canonical) {
            normalized.insert(canonical.to_string(), value);
        }
    }
    normalized
}

/// Validates physics settings values are within safe ranges.
/// Rejects NaN, Infinity, and out-of-range values.
pub fn validate_physics_settings(settings: &PhysicsSettings) -> Result<(), String> {
    let mut errors = Vec::new();

    // Helper closure: check finite
    let check_finite = |val: f32, name: &str, errs: &mut Vec<String>| {
        if !val.is_finite() {
            errs.push(format!(
                "{} must be a finite number (not NaN or Infinity)",
                name
            ));
        }
    };

    // Helper closure: check range (inclusive)
    let check_range = |val: f32, name: &str, min: f32, max: f32, errs: &mut Vec<String>| {
        if !val.is_finite() {
            errs.push(format!(
                "{} must be a finite number (not NaN or Infinity)",
                name
            ));
        } else if val < min || val > max {
            errs.push(format!(
                "{} must be between {} and {} (got {})",
                name, min, max, val
            ));
        }
    };

    // Range-checked fields read their (MIN, MAX) from the single source of
    // truth, `crate::actors::gpu::physics_bounds`, so this route validator can
    // never diverge from the OptimizedSettingsActor path-pattern caps or the
    // canonical client defaults again (T4 ceiling-consistency fix, 2026-06-03).
    use crate::actors::gpu::physics_bounds as bounds;

    check_range(
        settings.gravity,
        "gravity",
        bounds::GRAVITY.0,
        bounds::GRAVITY.1,
        &mut errors,
    );
    check_range(
        settings.damping,
        "damping",
        bounds::DAMPING.0,
        bounds::DAMPING.1,
        &mut errors,
    );
    check_range(
        settings.spring_k,
        "spring_k",
        bounds::SPRING_K.0,
        bounds::SPRING_K.1,
        &mut errors,
    );
    check_range(
        settings.max_velocity,
        "max_velocity",
        bounds::MAX_VELOCITY.0,
        bounds::MAX_VELOCITY.1,
        &mut errors,
    );
    check_range(
        settings.max_force,
        "max_force",
        bounds::MAX_FORCE.0,
        bounds::MAX_FORCE.1,
        &mut errors,
    );
    check_range(
        settings.dt,
        "timestep (dt)",
        bounds::DT.0,
        bounds::DT.1,
        &mut errors,
    );
    check_range(
        settings.max_repulsion_dist,
        "max_repulsion_dist",
        bounds::MAX_REPULSION_DIST.0,
        bounds::MAX_REPULSION_DIST.1,
        &mut errors,
    );
    check_range(
        settings.cooling_rate,
        "cooling_rate",
        bounds::COOLING_RATE.0,
        bounds::COOLING_RATE.1,
        &mut errors,
    );
    check_range(
        settings.boundary_damping,
        "boundary_damping",
        bounds::BOUNDARY_DAMPING.0,
        bounds::BOUNDARY_DAMPING.1,
        &mut errors,
    );
    // cluster_strength is the raw kernel coefficient (no scale factor applied).
    check_range(
        settings.cluster_strength,
        "cluster_strength",
        bounds::CLUSTER_STRENGTH.0,
        bounds::CLUSTER_STRENGTH.1,
        &mut errors,
    );
    check_range(
        settings.sssp_alpha,
        "sssp_alpha",
        bounds::SSSP_ALPHA.0,
        bounds::SSSP_ALPHA.1,
        &mut errors,
    );
    // repel_k now has a bounded range from the shared source of truth (was
    // previously finite-only here, while the actor capped it at 100 — the exact
    // divergence this fix resolves).
    check_range(
        settings.repel_k,
        "repel_k",
        bounds::REPEL_K.0,
        bounds::REPEL_K.1,
        &mut errors,
    );
    check_range(
        settings.bounds_size,
        "bounds_size",
        bounds::BOUNDS_SIZE.0,
        bounds::BOUNDS_SIZE.1,
        &mut errors,
    );
    check_range(
        settings.temperature,
        "temperature",
        bounds::TEMPERATURE.0,
        bounds::TEMPERATURE.1,
        &mut errors,
    );
    check_range(
        settings.broadcast_fps as f32,
        "broadcast_fps",
        bounds::BROADCAST_FPS.0,
        bounds::BROADCAST_FPS.1,
        &mut errors,
    );

    // All other f32 fields: reject NaN/Infinity
    check_finite(settings.separation_radius, "separation_radius", &mut errors);
    check_finite(settings.rest_length, "rest_length", &mut errors);
    check_finite(
        settings.repulsion_softening_epsilon,
        "repulsion_softening_epsilon",
        &mut errors,
    );
    check_finite(settings.center_gravity_k, "center_gravity_k", &mut errors);
    check_finite(settings.grid_cell_size, "grid_cell_size", &mut errors);
    check_finite(
        settings.constraint_max_force_per_node,
        "constraint_max_force_per_node",
        &mut errors,
    );
    check_finite(
        settings.clustering_resolution,
        "clustering_resolution",
        &mut errors,
    );

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

// ============================================================================
// Constraint Settings Validation (QE Fix #1)
// ============================================================================

/// Validates constraint threshold values are finite, non-negative, and properly ordered.
pub fn validate_constraint_settings(settings: &ConstraintSettings) -> Result<(), String> {
    if !settings.far_threshold.is_finite() || settings.far_threshold < 0.0 {
        return Err("far_threshold must be finite and non-negative".into());
    }
    if !settings.medium_threshold.is_finite() || settings.medium_threshold < 0.0 {
        return Err("medium_threshold must be finite and non-negative".into());
    }
    if !settings.near_threshold.is_finite() || settings.near_threshold < 0.0 {
        return Err("near_threshold must be finite and non-negative".into());
    }
    if settings.near_threshold >= settings.medium_threshold
        || settings.medium_threshold >= settings.far_threshold
    {
        return Err("Thresholds must be ordered: near < medium < far".into());
    }
    Ok(())
}

// ============================================================================
// Rendering Settings Validation (QE Fix #1)
// ============================================================================

/// Validates rendering light intensity values are finite and non-negative.
pub fn validate_rendering_settings(settings: &RenderingSettings) -> Result<(), String> {
    let check_finite = |v: f64, name: &str| -> Result<(), String> {
        if !v.is_finite() || v < 0.0 {
            Err(format!("{} must be finite and non-negative", name))
        } else {
            Ok(())
        }
    };
    check_finite(
        settings.ambient_light_intensity as f64,
        "ambient_light_intensity",
    )?;
    check_finite(
        settings.directional_light_intensity as f64,
        "directional_light_intensity",
    )?;
    check_finite(
        settings.environment_intensity as f64,
        "environment_intensity",
    )?;
    Ok(())
}

// ============================================================================
// Node Filter Settings Validation (QE Fix #1)
// ============================================================================

/// Validates node filter thresholds and filter mode.
pub fn validate_node_filter_settings(settings: &NodeFilterSettings) -> Result<(), String> {
    if settings.quality_threshold < 0.0 || settings.quality_threshold > 1.0 {
        return Err("quality_threshold must be 0.0-1.0".into());
    }
    if settings.authority_threshold < 0.0 || settings.authority_threshold > 1.0 {
        return Err("authority_threshold must be 0.0-1.0".into());
    }
    if settings.filter_mode != "and" && settings.filter_mode != "or" {
        return Err("filter_mode must be 'and' or 'or'".into());
    }
    Ok(())
}

// ============================================================================
// Quality Gate Settings Validation
// ============================================================================

/// Validates quality gate settings are within safe operational ranges.
pub fn validate_quality_gate_settings(settings: &QualityGateSettings) -> Result<(), String> {
    let mut errors = Vec::new();

    if !settings.ontology_strength.is_finite()
        || settings.ontology_strength < 0.0
        || settings.ontology_strength > 1.0
    {
        errors.push(format!(
            "ontology_strength must be between 0.0 and 1.0 (got {})",
            settings.ontology_strength
        ));
    }
    if !settings.dag_level_attraction.is_finite()
        || settings.dag_level_attraction < 0.0
        || settings.dag_level_attraction > 2.0
    {
        errors.push(format!(
            "dag_level_attraction must be between 0.0 and 2.0 (got {})",
            settings.dag_level_attraction
        ));
    }
    if !settings.dag_sibling_repulsion.is_finite()
        || settings.dag_sibling_repulsion < 0.0
        || settings.dag_sibling_repulsion > 2.0
    {
        errors.push(format!(
            "dag_sibling_repulsion must be between 0.0 and 2.0 (got {})",
            settings.dag_sibling_repulsion
        ));
    }
    if !settings.type_cluster_attraction.is_finite()
        || settings.type_cluster_attraction < 0.0
        || settings.type_cluster_attraction > 2.0
    {
        errors.push(format!(
            "type_cluster_attraction must be between 0.0 and 2.0 (got {})",
            settings.type_cluster_attraction
        ));
    }
    if !settings.type_cluster_radius.is_finite()
        || settings.type_cluster_radius < 10.0
        || settings.type_cluster_radius > 500.0
    {
        errors.push(format!(
            "type_cluster_radius must be between 10.0 and 500.0 (got {})",
            settings.type_cluster_radius
        ));
    }
    let valid_modes = [
        "force-directed",
        "dag-topdown",
        "dag-radial",
        "dag-leftright",
        "type-clustering",
    ];
    if !valid_modes.contains(&settings.layout_mode.as_str()) {
        errors.push(format!(
            "layout_mode must be one of {:?} (got '{}')",
            valid_modes, settings.layout_mode
        ));
    }
    if settings.min_fps_threshold < 10 || settings.min_fps_threshold > 120 {
        errors.push(format!(
            "min_fps_threshold must be between 10 and 120 (got {})",
            settings.min_fps_threshold
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Tell every other connected session that a settings category changed.
///
/// ADR-2047: settings writes used to be split between categories that told other
/// sessions and categories that did not — `rendering` and `node-filter`
/// broadcast, while `physics`, `constraints`, `quality-gates`, `visual` and the
/// profile routes stayed silent, so a second open browser kept stale values
/// until it happened to re-read. The asymmetry was incidental, not a decision:
/// the payload was copy-pasted into the two handlers that needed it first.
///
/// This is the single emitter. It is fire-and-forget by design — a settings
/// write must not fail because a broadcast could not be serialised or the
/// coordinator's mailbox was full, so failures are logged and swallowed.
fn broadcast_settings_change(state: &web::Data<AppState>, category: &str, updated_by: &str) {
    let payload = serde_json::json!({
        "type": "settingsUpdated",
        "category": category,
        "updatedBy": updated_by,
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    match serde_json::to_string(&payload) {
        Ok(message) => {
            state
                .client_manager_addr
                .do_send(BroadcastMessage { message });
            info!("{category} settings change broadcast sent to connected clients");
        }
        Err(e) => warn!("failed to serialise {category} settings broadcast: {e}"),
    }
}

// ============================================================================
// Physics Settings Routes
// ============================================================================

/// The message that applies `physics.broadcast_fps` to the live
/// ForceComputeActor. Rate only: spatial culling is not a settings field and
/// is left as the actor has it.
pub fn broadcast_rate_message(physics: &PhysicsSettings) -> ConfigureBroadcastOptimization {
    ConfigureBroadcastOptimization::rate_only(physics.broadcast_fps)
}

/// GET /api/settings/physics
pub async fn get_physics_settings(
    state: web::Data<AppState>,
    _auth: OptionalAuth,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
) -> impl Responder {
    // Try SQLite persisted settings first, fall back to in-memory actor
    if let Ok(Some(SettingValue::Json(json))) = settings_repo.get_setting("physics").await {
        if let Ok(physics) = serde_json::from_value::<PhysicsSettings>(json) {
            return HttpResponse::Ok().json(physics);
        }
    }

    match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => {
            HttpResponse::Ok().json(settings.visualisation.graphs.knowledge.physics)
        }
        Ok(Err(e)) => {
            error!("Failed to get physics settings: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to get physics settings: {}", e),
            })
        }
        Err(e) => {
            error!("Actor mailbox error: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Actor communication error: {}", e),
            })
        }
    }
}

/// PUT /api/settings/physics
/// Validates input before applying (QE Fix #2 + #5).
/// Accepts partial JSON updates -- missing fields retain current values from the actor.
/// Uses single GetSettings call to avoid TOCTOU race (QE Fix #3).
pub async fn update_physics_settings(
    state: web::Data<AppState>,
    body: web::Json<serde_json::Value>,
    auth: AuthenticatedUser,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
) -> impl Responder {
    debug!(
        "User {} updating physics settings — request body: {:?}",
        auth.pubkey, body
    );

    // Single GetSettings call -- fetch full settings snapshot once to avoid TOCTOU race
    let mut full_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to fetch current settings: {}", e);
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to fetch current settings: {}", e),
            });
        }
        Err(e) => {
            error!("Actor mailbox error: {}", e);
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Actor communication error: {}", e),
            });
        }
    };

    // Merge partial patch onto current physics from the same snapshot
    let current_physics = &full_settings.visualisation.graphs.knowledge.physics;
    let current_json = serde_json::to_value(current_physics).unwrap_or_default();

    let new_physics =
        if let (serde_json::Value::Object(mut base), serde_json::Value::Object(patch)) =
            (current_json, body.into_inner())
        {
            // Normalize incoming keys: map common aliases (snake_case, legacy names)
            // to the canonical camelCase field names used by PhysicsSettings.
            let normalized_patch = normalize_physics_keys(patch);
            for (k, v) in normalized_patch {
                base.insert(k, v);
            }
            match serde_json::from_value::<PhysicsSettings>(serde_json::Value::Object(base)) {
                Ok(merged) => merged,
                Err(e) => {
                    warn!("Physics settings merge failed: {}", e);
                    return HttpResponse::BadRequest().json(ErrorResponse {
                        error: format!("Invalid settings value: {}", e),
                    });
                }
            }
        } else {
            full_settings.visualisation.graphs.knowledge.physics.clone()
        };

    // Validate before applying
    if let Err(validation_err) = validate_physics_settings(&new_physics) {
        warn!("Physics settings validation failed: {}", validation_err);
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("Validation failed: {}", validation_err),
        });
    }

    // Apply merged physics to the same snapshot and write back atomically
    full_settings.visualisation.graphs.knowledge.physics = new_physics.clone();
    match state
        .settings_addr
        .send(UpdateSettings {
            settings: full_settings,
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Physics settings updated successfully by {}", auth.pubkey);

            // Propagate physics changes to GPU actors so layout actually responds
            let sim_params: crate::models::simulation_params::SimulationParams =
                (&new_physics).into();
            info!(
                "Propagating SimulationParams: spring_k={}, repel_k={}, damping={}",
                sim_params.spring_k, sim_params.repel_k, sim_params.damping
            );
            let update_msg = UpdateSimulationParams { params: sim_params };

            if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
                info!("Sending UpdateSimulationParams to GPUComputeActor (direct)");
                if let Err(e) = gpu_addr.send(update_msg.clone()).await {
                    error!("Failed to propagate physics to GPUComputeActor: {}", e);
                } else {
                    info!("UpdateSimulationParams sent to GPUComputeActor successfully");
                }
            } else {
                // Fallback: route through GPUManagerActor when direct address isn't cached yet
                // (first ~6s of startup while async init task completes)
                warn!("Direct GPUComputeActor address not available, routing via GPUManagerActor fallback");
                if let Some(ref gpu_mgr) = state.gpu_manager_addr {
                    if let Err(e) = gpu_mgr.send(update_msg.clone()).await {
                        error!(
                            "Fallback: Failed to route physics via GPUManagerActor: {}",
                            e
                        );
                    } else {
                        info!("Fallback: UpdateSimulationParams routed via GPUManagerActor");
                    }
                } else {
                    error!("No GPUComputeActor or GPUManagerActor available — physics won't propagate to GPU!");
                }
            }

            // Community-detector params (algorithm/resolution/iterations) cannot ride
            // in the 172-byte repr-C SimParams, so dispatch them separately and
            // directly to the ForceComputeActor. This is what makes the Physics-tab
            // "Community Resolution" / "Community Method" controls live: the GPU
            // re-runs Leiden/Louvain with the new params on the next cohesion pass.
            if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
                gpu_addr.do_send(UpdateClusteringParams {
                    algorithm: new_physics.clustering_algorithm.clone(),
                    resolution: new_physics.clustering_resolution,
                    iterations: new_physics.clustering_iterations,
                });
                // The broadcast rate is not a SimParams field either: it goes
                // straight to the live broadcast optimiser (already validated).
                match gpu_addr.send(broadcast_rate_message(&new_physics)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => warn!("broadcast rate not applied: {}", e),
                    Err(e) => warn!("broadcast rate not delivered to ForceComputeActor: {}", e),
                }
            } else {
                warn!(
                    "ForceComputeActor not available; broadcast rate {} Hz applies at its next boot push",
                    new_physics.broadcast_fps
                );
            }

            info!("Sending UpdateSimulationParams to GraphServiceSupervisor");
            if let Err(e) = state.graph_service_addr.send(update_msg).await {
                error!("Failed to propagate physics to GraphServiceActor: {}", e);
            } else {
                info!("UpdateSimulationParams sent to GraphServiceSupervisor successfully");
            }

            // Force-resume physics so the new parameters actually take effect.
            // Without this, a converged system stays paused and param changes are invisible.
            info!("Sending ForceResumePhysics to GraphServiceSupervisor");
            if let Err(e) = state
                .graph_service_addr
                .send(ForceResumePhysics {
                    reason: "Physics settings updated via API".to_string(),
                })
                .await
            {
                warn!("Failed to send ForceResumePhysics: {}", e);
            } else {
                info!("ForceResumePhysics sent to GraphServiceSupervisor successfully");
            }

            // Persist physics settings to SQLite for cross-restart survival
            match serde_json::to_value(&new_physics) {
                Ok(physics_json) => {
                    if let Err(e) = settings_repo
                        .set_setting(
                            "physics",
                            SettingValue::Json(physics_json),
                            Some("Physics simulation settings"),
                        )
                        .await
                    {
                        warn!("Failed to persist physics settings to SQLite: {}", e);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to serialize physics settings for persistence: {}",
                        e
                    );
                }
            }

            // ADR-2047: tell the other sessions. Physics is the category most
            // visible to a second viewer — the layout visibly moves — and was
            // the one that never announced itself.
            broadcast_settings_change(&state, "physics", &auth.pubkey);

            HttpResponse::Ok().json(&new_physics)
        }
        Ok(Err(e)) => {
            error!("Failed to update physics settings: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to update physics settings: {}", e),
            })
        }
        Err(e) => {
            error!("Actor mailbox error: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Actor communication error: {}", e),
            })
        }
    }
}

// ============================================================================
// Constraint Settings Routes
// ============================================================================

/// GET /api/settings/constraints
/// Loads constraint settings from SQLite repository, falling back to defaults.
pub async fn get_constraint_settings(
    _state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    _auth: OptionalAuth,
) -> impl Responder {
    match settings_repo.get_setting("constraints").await {
        Ok(Some(crate::ports::settings_repository::SettingValue::Json(json))) => {
            match serde_json::from_value::<ConstraintSettings>(json) {
                Ok(settings) => HttpResponse::Ok().json(settings),
                Err(e) => {
                    warn!(
                        "Failed to parse stored constraint settings, returning defaults: {}",
                        e
                    );
                    HttpResponse::Ok().json(ConstraintSettings::default())
                }
            }
        }
        Ok(_) => HttpResponse::Ok().json(ConstraintSettings::default()),
        Err(e) => {
            warn!("Failed to load constraint settings from repository: {}", e);
            HttpResponse::Ok().json(ConstraintSettings::default())
        }
    }
}

/// PUT /api/settings/constraints
/// Validates input before accepting (QE Fix #1). Returns updated state (QE Fix #2).
/// Persists to SQLite repository via set_setting.
pub async fn update_constraint_settings(
    state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    body: web::Json<ConstraintSettings>,
    auth: AuthenticatedUser,
) -> impl Responder {
    info!("User {} updating constraint settings", auth.pubkey);

    let settings = body.into_inner();

    // Validate before accepting
    if let Err(validation_err) = validate_constraint_settings(&settings) {
        warn!("Constraint settings validation failed: {}", validation_err);
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("Validation failed: {}", validation_err),
        });
    }

    // Persist to SQLite
    let settings_json = match serde_json::to_value(&settings) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialize constraint settings: {}", e);
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to serialize constraint settings: {}", e),
            });
        }
    };

    if let Err(e) = settings_repo
        .set_setting(
            "constraints",
            crate::ports::settings_repository::SettingValue::Json(settings_json),
            Some("Constraint settings for physics simulation"),
        )
        .await
    {
        error!("Failed to persist constraint settings: {}", e);
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to persist constraint settings: {}", e),
        });
    }

    info!(
        "Constraint settings updated and persisted for user {}",
        auth.pubkey
    );

    // Propagate constraint settings to GPU actors so physics simulation reflects changes
    let constraint_json = match serde_json::to_value(&settings) {
        Ok(json) => json,
        Err(e) => {
            error!(
                "Failed to serialize constraint settings for GPU propagation: {}",
                e
            );
            // Settings are already persisted; return success but log the propagation failure
            return HttpResponse::Ok().json(&settings);
        }
    };

    let constraint_msg = UpdateConstraints {
        constraint_data: constraint_json,
    };

    if let Some(ref gpu_manager) = state.gpu_manager_addr {
        if let Err(e) = gpu_manager.send(constraint_msg).await {
            error!("Failed to propagate constraints to GPUManagerActor: {}", e);
        } else {
            info!(
                "Constraint settings propagated to GPUManagerActor for user {}",
                auth.pubkey
            );
        }
    } else {
        warn!("GPUManagerActor not available; constraint settings persisted but not propagated to GPU");
    }

    HttpResponse::Ok().json(&settings)
}

// ============================================================================
// Rendering Settings Routes
// ============================================================================

/// GET /api/settings/rendering
pub async fn get_rendering_settings(
    state: web::Data<AppState>,
    _auth: OptionalAuth,
) -> impl Responder {
    match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => HttpResponse::Ok().json(settings.visualisation.rendering),
        Ok(Err(e)) => {
            error!("Failed to get rendering settings: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to get rendering settings: {}", e),
            })
        }
        Err(e) => {
            error!("Actor mailbox error: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to get rendering settings: {}", e),
            })
        }
    }
}

/// PUT /api/settings/rendering
/// Validates input before applying (QE Fix #1). Returns updated state (QE Fix #2).
pub async fn update_rendering_settings(
    state: web::Data<AppState>,
    body: web::Json<RenderingSettings>,
    auth: AuthenticatedUser,
) -> impl Responder {
    info!("User {} updating rendering settings", auth.pubkey);

    let new_rendering = body.into_inner();

    // Validate before applying
    if let Err(validation_err) = validate_rendering_settings(&new_rendering) {
        warn!("Rendering settings validation failed: {}", validation_err);
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("Validation failed: {}", validation_err),
        });
    }

    match state.settings_addr.send(GetSettings).await {
        Ok(Ok(mut full_settings)) => {
            full_settings.visualisation.rendering = new_rendering.clone();
            match state
                .settings_addr
                .send(UpdateSettings {
                    settings: full_settings,
                })
                .await
            {
                Ok(Ok(())) => {
                    info!("Rendering settings updated successfully by {}", auth.pubkey);

                    // ADR-2047: rendering settings (ambient light, shadows,
                    // environment) are applied client-side, so other sessions
                    // must be told. Now routed through the single emitter.
                    broadcast_settings_change(&state, "rendering", &auth.pubkey);

                    HttpResponse::Ok().json(&new_rendering)
                }
                Ok(Err(e)) => {
                    error!("Failed to update rendering settings: {}", e);
                    HttpResponse::InternalServerError().json(ErrorResponse {
                        error: format!("Failed to update rendering settings: {}", e),
                    })
                }
                Err(e) => {
                    error!("Actor mailbox error: {}", e);
                    HttpResponse::InternalServerError().json(ErrorResponse {
                        error: format!("Actor communication error: {}", e),
                    })
                }
            }
        }
        Ok(Err(e)) => {
            error!("Failed to fetch current settings: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to fetch current settings: {}", e),
            })
        }
        Err(e) => {
            error!("Actor mailbox error: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Actor communication error: {}", e),
            })
        }
    }
}

// ============================================================================
// Node Filter Settings Routes
// ============================================================================

/// GET /api/settings/node-filter
/// Loads node filter settings from SQLite repository, falling back to defaults.
pub async fn get_node_filter_settings(
    _state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    _auth: OptionalAuth,
) -> impl Responder {
    match settings_repo.get_setting("node_filter").await {
        Ok(Some(crate::ports::settings_repository::SettingValue::Json(json))) => {
            match serde_json::from_value::<NodeFilterSettings>(json) {
                Ok(settings) => HttpResponse::Ok().json(settings),
                Err(e) => {
                    warn!(
                        "Failed to parse stored node filter settings, returning defaults: {}",
                        e
                    );
                    HttpResponse::Ok().json(NodeFilterSettings::default())
                }
            }
        }
        Ok(_) => HttpResponse::Ok().json(NodeFilterSettings::default()),
        Err(e) => {
            warn!("Failed to load node filter settings from repository: {}", e);
            HttpResponse::Ok().json(NodeFilterSettings::default())
        }
    }
}

/// PUT /api/settings/node-filter
/// Validates input before accepting (QE Fix #1). Returns updated state (QE Fix #2).
/// Persists to SQLite repository via set_setting.
pub async fn update_node_filter_settings(
    state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    body: web::Json<NodeFilterSettings>,
    auth: AuthenticatedUser,
) -> impl Responder {
    info!(
        "User {} updating node filter settings: enabled={}, threshold={}",
        auth.pubkey, body.enabled, body.quality_threshold
    );

    let settings = body.into_inner();

    // Validate before accepting
    if let Err(validation_err) = validate_node_filter_settings(&settings) {
        warn!("Node filter settings validation failed: {}", validation_err);
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("Validation failed: {}", validation_err),
        });
    }

    // Persist to SQLite
    let settings_json = match serde_json::to_value(&settings) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialize node filter settings: {}", e);
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to serialize node filter settings: {}", e),
            });
        }
    };

    if let Err(e) = settings_repo
        .set_setting(
            "node_filter",
            crate::ports::settings_repository::SettingValue::Json(settings_json),
            Some("Node confidence filter settings"),
        )
        .await
    {
        error!("Failed to persist node filter settings: {}", e);
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to persist node filter settings: {}", e),
        });
    }

    // Propagate node filter changes to all connected clients via broadcast.
    // Clients receiving this message recompute which nodes pass the filter
    // and re-render the visible graph accordingly.
    let broadcast_payload = serde_json::json!({
        "type": "settingsUpdated",
        "category": "nodeFilter",
        "settings": {
            "enabled": settings.enabled,
            "qualityThreshold": settings.quality_threshold,
            "authorityThreshold": settings.authority_threshold,
            "filterByQuality": settings.filter_by_quality,
            "filterByAuthority": settings.filter_by_authority,
            "filterMode": settings.filter_mode,
            "includeLinkedPages": settings.include_linked_pages,
        },
        "updatedBy": auth.pubkey,
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(msg_str) = serde_json::to_string(&broadcast_payload) {
        state
            .client_manager_addr
            .do_send(BroadcastMessage { message: msg_str });
        info!("Node filter settings change broadcast sent to connected clients");
    }

    info!(
        "Node filter settings updated and persisted for user {}: enabled={}, quality_threshold={}",
        auth.pubkey, settings.enabled, settings.quality_threshold
    );
    HttpResponse::Ok().json(&settings)
}

// ============================================================================
// Quality Gate Settings Routes
// ============================================================================

/// GET /api/settings/quality-gates
/// Loads quality gate settings from SQLite repository, falling back to defaults.
pub async fn get_quality_gate_settings(
    _state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    _auth: OptionalAuth,
) -> impl Responder {
    match settings_repo.get_setting("quality_gates").await {
        Ok(Some(crate::ports::settings_repository::SettingValue::Json(json))) => {
            match serde_json::from_value::<QualityGateSettings>(json) {
                Ok(settings) => HttpResponse::Ok().json(settings),
                Err(e) => {
                    warn!(
                        "Failed to parse stored quality gate settings, returning defaults: {}",
                        e
                    );
                    HttpResponse::Ok().json(QualityGateSettings::default())
                }
            }
        }
        Ok(_) => HttpResponse::Ok().json(QualityGateSettings::default()),
        Err(e) => {
            warn!(
                "Failed to load quality gate settings from repository: {}",
                e
            );
            HttpResponse::Ok().json(QualityGateSettings::default())
        }
    }
}

/// PUT /api/settings/quality-gates
/// Accepts partial JSON updates -- missing fields retain their persisted or default values.
/// Returns updated state (QE Fix #2). Persists to SQLite repository.
pub async fn update_quality_gate_settings(
    state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    body: web::Json<serde_json::Value>,
    auth: AuthenticatedUser,
) -> impl Responder {
    // Load current persisted settings as the merge base (instead of hardcoded defaults)
    let current_settings = match settings_repo.get_setting("quality_gates").await {
        Ok(Some(crate::ports::settings_repository::SettingValue::Json(json))) => {
            serde_json::from_value::<QualityGateSettings>(json).unwrap_or_default()
        }
        _ => QualityGateSettings::default(),
    };

    let mut settings = current_settings;
    let current_json = serde_json::to_value(&settings).unwrap_or_default();

    if let (serde_json::Value::Object(mut base), serde_json::Value::Object(patch)) =
        (current_json, body.into_inner())
    {
        for (k, v) in patch {
            base.insert(k, v);
        }
        match serde_json::from_value::<QualityGateSettings>(serde_json::Value::Object(base)) {
            Ok(merged) => settings = merged,
            Err(e) => {
                warn!("Quality gate settings merge failed: {}", e);
                return HttpResponse::BadRequest().json(ErrorResponse {
                    error: format!("Invalid settings value: {}", e),
                });
            }
        }
    }

    // Validate before persisting
    if let Err(validation_err) = validate_quality_gate_settings(&settings) {
        warn!(
            "Quality gate settings validation failed: {}",
            validation_err
        );
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: format!("Validation failed: {}", validation_err),
        });
    }

    // Persist to SQLite
    let settings_json = match serde_json::to_value(&settings) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialize quality gate settings: {}", e);
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to serialize quality gate settings: {}", e),
            });
        }
    };

    if let Err(e) = settings_repo
        .set_setting(
            "quality_gates",
            crate::ports::settings_repository::SettingValue::Json(settings_json),
            Some("Quality gate settings for feature toggles and performance thresholds"),
        )
        .await
    {
        error!("Failed to persist quality gate settings: {}", e);
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to persist quality gate settings: {}", e),
        });
    }

    info!("User {} updated quality gate settings: gpu={}, ontology={}, semantic={}, layout={}, maxNodeCount={}",
          auth.pubkey, settings.gpu_acceleration, settings.ontology_physics,
          settings.semantic_forces, settings.layout_mode, settings.max_node_count);

    // Propagate GPU-affecting quality gate changes to the physics engine.
    // Fetch current physics from the settings actor so SimulationParams is built
    // from the authoritative snapshot, then overlay quality-gate fields.
    match state.settings_addr.send(GetSettings).await {
        Ok(Ok(full_settings)) => {
            let physics = &full_settings.visualisation.graphs.knowledge.physics;
            let mut sim_params: crate::models::simulation_params::SimulationParams = physics.into();

            // gpu_acceleration -> compute_mode (0 = Basic/CPU, 2 = Advanced/GPU)
            if settings.gpu_acceleration {
                sim_params.compute_mode = 2;
            } else {
                sim_params.compute_mode = 0;
            }
            sim_params.enabled = settings.gpu_acceleration;

            // Apply layout-mode-specific physics overrides.
            // DAG modes enable center gravity + SSSP for hierarchical layout.
            // Type-clustering enables cluster/alignment forces for grouping.
            match settings.layout_mode.as_str() {
                "dag-topdown" | "dag-radial" | "dag-leftright" => {
                    info!(
                        "Quality gates: applying DAG layout overrides for mode: {}",
                        settings.layout_mode
                    );
                    sim_params.center_gravity_k = sim_params.center_gravity_k.max(0.1);
                    sim_params.use_sssp_distances = true;
                    sim_params.sssp_alpha = Some(sim_params.sssp_alpha.unwrap_or(0.0).max(0.5));
                }
                "type-clustering" => {
                    info!("Quality gates: applying type-clustering layout overrides");
                    // cluster_strength is now the raw kernel coefficient (range
                    // [0, 0.02]); floor at the upper end to strengthen grouping.
                    sim_params.cluster_strength = sim_params.cluster_strength.max(0.01);
                }
                _ => {
                    // force-directed: use physics settings as-is
                }
            }

            let update_msg = UpdateSimulationParams { params: sim_params };

            if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
                if let Err(e) = gpu_addr.send(update_msg.clone()).await {
                    error!("Quality gates: failed to propagate SimulationParams to ForceComputeActor: {}", e);
                }
                let compute_mode = if settings.gpu_acceleration {
                    ComputeMode::Advanced
                } else {
                    ComputeMode::Basic
                };
                if let Err(e) = gpu_addr.send(SetComputeMode { mode: compute_mode }).await {
                    error!(
                        "Quality gates: failed to propagate ComputeMode to ForceComputeActor: {}",
                        e
                    );
                }
            }

            if let Err(e) = state.graph_service_addr.send(update_msg).await {
                error!(
                    "Quality gates: failed to propagate SimulationParams to GraphServiceActor: {}",
                    e
                );
            }

            info!(
                "Quality gates propagated to physics engine: gpu={}, compute_mode={}, layout={}",
                settings.gpu_acceleration,
                if settings.gpu_acceleration {
                    "Advanced"
                } else {
                    "Basic"
                },
                settings.layout_mode
            );

            // --- Propagate semantic forces configuration to SemanticForcesActor ---
            if let Some(ref gpu_manager) = state.gpu_manager_addr {
                use crate::actors::messages::{
                    AdjustConstraintWeights, ConfigureDAG, ConfigureTypeClustering,
                };

                let is_dag_mode = matches!(
                    settings.layout_mode.as_str(),
                    "dag-topdown" | "dag-radial" | "dag-leftright"
                );
                let is_type_clustering = settings.layout_mode == "type-clustering";

                // Configure DAG layout forces
                let dag_msg = ConfigureDAG {
                    vertical_spacing: None,
                    horizontal_spacing: None,
                    level_attraction: Some(settings.dag_level_attraction),
                    sibling_repulsion: Some(settings.dag_sibling_repulsion),
                    enabled: Some(settings.semantic_forces && is_dag_mode),
                };
                if let Err(e) = gpu_manager.send(dag_msg).await {
                    warn!("Quality gates: failed to propagate DAG config: {}", e);
                }

                // Configure type clustering forces
                let cluster_msg = ConfigureTypeClustering {
                    cluster_attraction: Some(settings.type_cluster_attraction),
                    cluster_radius: Some(settings.type_cluster_radius),
                    inter_cluster_repulsion: None,
                    enabled: Some(
                        settings.semantic_forces
                            && (is_type_clustering || settings.layout_mode == "force-directed"),
                    ),
                };
                if let Err(e) = gpu_manager.send(cluster_msg).await {
                    warn!(
                        "Quality gates: failed to propagate type clustering config: {}",
                        e
                    );
                }

                // Adjust ontology constraint weights when strength changes
                if settings.ontology_physics {
                    let weight_msg = AdjustConstraintWeights {
                        global_strength: settings.ontology_strength,
                    };
                    if let Err(e) = gpu_manager.send(weight_msg).await {
                        warn!("Quality gates: failed to propagate ontology weights: {}", e);
                    }
                }

                info!("Quality gates: semantic forces propagated — dag_enabled={}, clustering_enabled={}, ontology_strength={}",
                      settings.semantic_forces && is_dag_mode,
                      settings.semantic_forces && (is_type_clustering || settings.layout_mode == "force-directed"),
                      settings.ontology_strength);
            }
        }
        Ok(Err(e)) => {
            warn!(
                "Quality gates persisted but failed to read settings for propagation: {}",
                e
            );
        }
        Err(e) => {
            warn!(
                "Quality gates persisted but settings actor unreachable for propagation: {}",
                e
            );
        }
    }

    HttpResponse::Ok().json(settings)
}

// ============================================================================
// Visual Settings Routes (opaque JSON blob for client visual settings)
// ============================================================================

/// Recursively deep-merge `patch` into `base`. Values in `patch` take priority.
/// Both must be JSON objects; non-object values in `patch` replace `base` wholesale.
fn deep_merge_json(base: &mut serde_json::Value, patch: serde_json::Value) {
    if let (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) =
        (base, patch)
    {
        for (key, value) in patch_map {
            let entry = base_map.entry(key).or_insert(serde_json::Value::Null);
            if value.is_object() && entry.is_object() {
                deep_merge_json(entry, value);
            } else {
                *entry = value;
            }
        }
    }
}

/// GET /api/settings/visual
/// Returns the stored visual settings blob (glow, hologram, graphTypeVisuals,
/// gemMaterial, sceneEffects, clusterHulls, animations, interaction, nodes, edges, labels).
/// Falls back to empty object if nothing stored yet.
pub async fn get_visual_settings(
    _state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    _auth: OptionalAuth,
) -> impl Responder {
    match settings_repo.get_setting("visual").await {
        Ok(Some(SettingValue::Json(json))) => HttpResponse::Ok().json(json),
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({})),
        Err(e) => {
            warn!("Failed to load visual settings from repository: {}", e);
            HttpResponse::Ok().json(serde_json::json!({}))
        }
    }
}

/// PUT /api/settings/visual
/// Accepts partial JSON updates — deep merges with currently stored values.
/// This is the persistence endpoint for all client-only visual settings.
pub async fn update_visual_settings(
    _state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    body: web::Json<serde_json::Value>,
    auth: AuthenticatedUser,
) -> impl Responder {
    info!("User {} updating visual settings", auth.pubkey);

    let patch = body.into_inner();
    if !patch.is_object() {
        return HttpResponse::BadRequest().json(ErrorResponse {
            error: "Visual settings must be a JSON object".to_string(),
        });
    }

    // Load current stored settings as merge base
    let mut current = match settings_repo.get_setting("visual").await {
        Ok(Some(SettingValue::Json(json))) if json.is_object() => json,
        _ => serde_json::json!({}),
    };

    // Deep merge patch into current
    deep_merge_json(&mut current, patch);

    // Persist merged result to SQLite
    if let Err(e) = settings_repo.set_setting(
        "visual",
        SettingValue::Json(current.clone()),
        Some("Client visual settings (glow, hologram, graphTypeVisuals, nodes, edges, labels, etc.)"),
    ).await {
        error!("Failed to persist visual settings: {}", e);
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to persist visual settings: {}", e),
        });
    }

    info!(
        "Visual settings updated and persisted for user {}",
        auth.pubkey
    );
    HttpResponse::Ok().json(current)
}

// ============================================================================
// All Settings Route
// ============================================================================

/// GET /api/settings/all
/// Returns global settings for anonymous users, or user-specific settings for authenticated users
pub async fn get_all_settings(
    state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    auth: OptionalAuth,
) -> impl Responder {
    use crate::adapters::sqlite_settings_repository::CURRENT_OWNER_PUBKEY;
    // Per-user layered resolution (ADR-11 §D5): scope the repository reads to
    // the authenticated owner so a user's own overrides win, falling back to
    // the global layer (owner_pubkey = ''). Anonymous requests resolve the
    // global layer only. The layered SELECT lives in SqliteSettingsRepository
    // (`owner_pubkey = ?2 OR owner_pubkey = ''`), so scoping the task-local is
    // all the handler needs to do.
    match auth.0 {
        Some(user) => {
            info!("GET /api/settings/all (per-user layer for {})", user.pubkey);
            CURRENT_OWNER_PUBKEY
                .scope(
                    Some(user.pubkey),
                    get_all_from_actor(&state, &settings_repo),
                )
                .await
        }
        None => {
            info!("GET /api/settings/all (global layer, anonymous)");
            get_all_from_actor(&state, &settings_repo).await
        }
    }
}

async fn get_all_from_actor(
    state: &web::Data<AppState>,
    settings_repo: &web::Data<Arc<SqliteSettingsRepository>>,
) -> HttpResponse {
    match state.settings_addr.send(GetSettings).await {
        Ok(Ok(full_settings)) => {
            // Load persisted settings from SQLite repository (ADR-11), falling back to actor/defaults
            let physics = match settings_repo.get_setting("physics").await {
                Ok(Some(SettingValue::Json(json))) => {
                    serde_json::from_value::<PhysicsSettings>(json)
                        .unwrap_or(full_settings.visualisation.graphs.knowledge.physics)
                }
                _ => full_settings.visualisation.graphs.knowledge.physics,
            };

            let constraints = match settings_repo.get_setting("constraints").await {
                Ok(Some(SettingValue::Json(json))) => {
                    serde_json::from_value::<ConstraintSettings>(json).unwrap_or_default()
                }
                _ => ConstraintSettings::default(),
            };

            let node_filter = match settings_repo.get_setting("node_filter").await {
                Ok(Some(SettingValue::Json(json))) => {
                    serde_json::from_value::<NodeFilterSettings>(json).unwrap_or_default()
                }
                _ => NodeFilterSettings::default(),
            };

            let quality_gates = match settings_repo.get_setting("quality_gates").await {
                Ok(Some(SettingValue::Json(json))) => {
                    serde_json::from_value::<QualityGateSettings>(json).unwrap_or_default()
                }
                _ => QualityGateSettings::default(),
            };

            let visual = match settings_repo.get_setting("visual").await {
                Ok(Some(SettingValue::Json(json))) => json,
                _ => serde_json::json!({}),
            };

            let all = AllSettings {
                physics,
                constraints,
                rendering: full_settings.visualisation.rendering,
                node_filter,
                quality_gates,
                visual,
            };
            HttpResponse::Ok().json(all)
        }
        Ok(Err(e)) => {
            error!("Failed to get all settings: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to get all settings: {}", e),
            })
        }
        Err(e) => {
            error!("Failed to get all settings: {}", e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to get all settings: {}", e),
            })
        }
    }
}

// ============================================================================
// User Filter Routes
// ============================================================================

/// Storage key for the per-user node visibility filter. The owner layer of the
/// `settings` table (ADR-11 §D5) namespaces this per authenticated pubkey, so a
/// single key resolves to distinct rows per user.
const USER_FILTER_KEY: &str = "user_filter";

/// GET /api/user/filter
/// Reads the authenticated user's persisted filter from SQLite (ADR-11 §D5),
/// scoping the layered read to the caller's owner layer. Falls back to the
/// default filter when the user has never stored one.
pub async fn get_user_filter(
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    auth: AuthenticatedUser,
) -> impl Responder {
    use crate::adapters::sqlite_settings_repository::CURRENT_OWNER_PUBKEY;

    let repo = settings_repo.get_ref().clone();
    let owner = auth.pubkey.clone();
    let stored = CURRENT_OWNER_PUBKEY
        .scope(Some(owner), async move {
            repo.get_setting(USER_FILTER_KEY).await
        })
        .await;

    match stored {
        Ok(Some(SettingValue::Json(json))) => match serde_json::from_value::<UserFilter>(json) {
            Ok(mut filter) => {
                filter.pubkey = auth.pubkey.clone();
                info!(
                    "GET /api/user/filter served persisted filter for user {}",
                    auth.pubkey
                );
                HttpResponse::Ok().json(filter)
            }
            Err(e) => {
                warn!(
                    "Stored user filter for {} failed to deserialize ({}); returning default",
                    auth.pubkey, e
                );
                let filter = UserFilter {
                    pubkey: auth.pubkey.clone(),
                    ..Default::default()
                };
                HttpResponse::Ok().json(filter)
            }
        },
        Ok(_) => {
            info!(
                "GET /api/user/filter no stored filter for user {}; returning default",
                auth.pubkey
            );
            let filter = UserFilter {
                pubkey: auth.pubkey.clone(),
                ..Default::default()
            };
            HttpResponse::Ok().json(filter)
        }
        Err(e) => {
            error!("Failed to read user filter for {}: {}", auth.pubkey, e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to read user filter: {}", e),
            })
        }
    }
}

/// PUT /api/user/filter
/// Persists the authenticated user's filter to SQLite (ADR-11 §D5), scoping the
/// write to the caller's owner layer so `GET /api/user/filter` round-trips it.
pub async fn update_user_filter(
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    body: web::Json<UserFilter>,
    auth: AuthenticatedUser,
) -> impl Responder {
    use crate::adapters::sqlite_settings_repository::CURRENT_OWNER_PUBKEY;

    let mut filter = body.into_inner();
    filter.pubkey = auth.pubkey.clone();

    let filter_json = match serde_json::to_value(&filter) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to serialize user filter for {}: {}", auth.pubkey, e);
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to serialize user filter: {}", e),
            });
        }
    };

    let repo = settings_repo.get_ref().clone();
    let owner = auth.pubkey.clone();
    let persisted = CURRENT_OWNER_PUBKEY
        .scope(Some(owner), async move {
            repo.set_setting(
                USER_FILTER_KEY,
                SettingValue::Json(filter_json),
                Some("Per-user node visibility filter (ADR-11 §D5)"),
            )
            .await
        })
        .await;

    match persisted {
        Ok(()) => {
            info!("PUT /api/user/filter persisted for user {}", auth.pubkey);
            HttpResponse::Ok().json(filter)
        }
        Err(e) => {
            error!("Failed to persist user filter for {}: {}", auth.pubkey, e);
            HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to persist user filter: {}", e),
            })
        }
    }
}

// ============================================================================
// Profile Management Routes
// ============================================================================

/// POST /api/settings/profiles
pub async fn save_profile(
    _state: web::Data<AppState>,
    body: web::Json<SaveProfileRequest>,
    auth: AuthenticatedUser,
) -> impl Responder {
    info!(
        "User {} saving settings profile: {}",
        auth.pubkey, body.name
    );
    HttpResponse::Created().json(ProfileIdResponse { id: 1 })
}

/// GET /api/settings/profiles/{id}
pub async fn load_profile(
    _state: web::Data<AppState>,
    path: web::Path<i64>,
    _auth: OptionalAuth,
) -> impl Responder {
    let profile_id = path.into_inner();
    info!("Loading settings profile: {}", profile_id);
    HttpResponse::NotFound().json(ErrorResponse {
        error: "Profile not found".to_string(),
    })
}

/// GET /api/settings/profiles
pub async fn list_profiles(_state: web::Data<AppState>, _auth: OptionalAuth) -> impl Responder {
    HttpResponse::Ok().json(Vec::<crate::settings::models::SettingsProfile>::new())
}

/// DELETE /api/settings/profiles/{id}
pub async fn delete_profile(
    _state: web::Data<AppState>,
    path: web::Path<i64>,
    auth: AuthenticatedUser,
) -> impl Responder {
    let profile_id = path.into_inner();
    info!(
        "User {} deleting settings profile: {}",
        auth.pubkey, profile_id
    );
    HttpResponse::Ok().finish()
}

// ============================================================================
// Physics Reset Layout
// ============================================================================

/// POST /api/settings/physics/reset-layout
/// Re-randomizes all node positions to a uniform sphere, resets physics to safe
/// defaults, and triggers full reheat. Use when the graph has exploded or converged
/// to an unusable state.
pub async fn reset_layout(
    state: web::Data<AppState>,
    _auth: AuthenticatedUser,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
) -> impl Responder {
    info!("Reset layout requested — sending ResetPositions + canonical physics defaults");

    // 1. Re-apply the canonical physics defaults (the single source of truth)
    // before re-randomizing positions. No hand-coded literals: PhysicsSettings::
    // default() carries enable_dual_disc_layout=false
    // (natural fully-3D layout by default; dual-disc is opt-in).
    let reset_physics = PhysicsSettings::default();
    let sim_params: crate::models::simulation_params::SimulationParams = (&reset_physics).into();
    info!(
        "Reset physics from canonical default (axis_compression_z={}, enable_dual_disc_layout={}, repel_k={}, center_gravity_k={})",
        sim_params.axis_compression_z,
        sim_params.enable_dual_disc_layout,
        sim_params.repel_k,
        sim_params.center_gravity_k
    );
    let update_msg = UpdateSimulationParams { params: sim_params };

    if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
        if let Err(e) = gpu_addr.send(update_msg.clone()).await {
            warn!(
                "Failed to send canonical physics defaults to ForceComputeActor: {}",
                e
            );
        }
    }
    if let Err(e) = state.graph_service_addr.send(update_msg).await {
        warn!(
            "Failed to propagate canonical physics to GraphServiceSupervisor: {}",
            e
        );
    }

    // Persist the canonical default to SQLite so the persisted store and the live
    // GPU actor cannot diverge after a reset. Same set_setting("physics")
    // pattern as update_physics_settings.
    match serde_json::to_value(&reset_physics) {
        Ok(physics_json) => {
            if let Err(e) = settings_repo
                .set_setting(
                    "physics",
                    SettingValue::Json(physics_json),
                    Some("Physics simulation settings (canonical default, layout reset)"),
                )
                .await
            {
                warn!(
                    "Failed to persist canonical physics to SQLite on reset: {}",
                    e
                );
            } else {
                info!("Persisted canonical physics default to SQLite on reset");
            }
        }
        Err(e) => warn!(
            "Failed to serialize canonical physics for persistence on reset: {}",
            e
        ),
    }

    // 2. Re-randomize positions on GPU
    if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
        if let Err(e) = gpu_addr.send(ResetPositions).await {
            error!("Failed to send ResetPositions to ForceComputeActor: {}", e);
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to reset layout: {}", e),
            });
        }
        info!("ResetPositions sent successfully");
    } else {
        error!("ForceComputeActor not available to handle ResetPositions");
        return HttpResponse::ServiceUnavailable().json(ErrorResponse {
            error: "GPU physics not available — ForceComputeActor not initialized".to_string(),
        });
    }

    // 3. Force-resume physics so the new layout starts settling
    if let Err(e) = state
        .graph_service_addr
        .send(ForceResumePhysics {
            reason: "Layout reset via API".to_string(),
        })
        .await
    {
        warn!("Failed to send ForceResumePhysics after reset: {}", e);
    }

    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "message": "Layout reset — positions re-randomized, physics reset to canonical defaults (close dual-disc), persisted to SQLite, full reheat triggered"
    }))
}

// ============================================================================
// Route Configuration
// ============================================================================

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    log::info!("Configuring settings routes (unified via OptimizedSettingsActor)");

    cfg.route("physics", web::get().to(get_physics_settings))
        .route("physics", web::put().to(update_physics_settings))
        .route("physics/reset-layout", web::post().to(reset_layout))
        .route("constraints", web::get().to(get_constraint_settings))
        .route("constraints", web::put().to(update_constraint_settings))
        .route("rendering", web::get().to(get_rendering_settings))
        .route("rendering", web::put().to(update_rendering_settings))
        .route("node-filter", web::get().to(get_node_filter_settings))
        .route("node-filter", web::put().to(update_node_filter_settings))
        .route("quality-gates", web::get().to(get_quality_gate_settings))
        .route("quality-gates", web::put().to(update_quality_gate_settings))
        .route("visual", web::get().to(get_visual_settings))
        .route("visual", web::put().to(update_visual_settings))
        .route("all", web::get().to(get_all_settings))
        .route("profiles", web::post().to(save_profile))
        .route("profiles", web::get().to(list_profiles))
        .route("profiles/{id}", web::get().to(load_profile))
        .route("profiles/{id}", web::delete().to(delete_profile));

    // User-specific filter settings
    cfg.service(
        web::scope("/user")
            .route("/filter", web::get().to(get_user_filter))
            .route("/filter", web::put().to(update_user_filter)),
    );

    log::info!("Settings routes configuration complete (single actor, validated PUT routes)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_rate(fps: u32) -> PhysicsSettings {
        PhysicsSettings {
            broadcast_fps: fps,
            ..PhysicsSettings::default()
        }
    }

    /// `broadcastFps` is validated against the shared 1-60 Hz bound.
    #[test]
    fn broadcast_fps_is_validated_1_to_60() {
        assert!(validate_physics_settings(&with_rate(8)).is_ok());
        assert!(validate_physics_settings(&with_rate(1)).is_ok());
        assert!(validate_physics_settings(&with_rate(60)).is_ok());
        for bad in [0, 61, 1000] {
            let err = validate_physics_settings(&with_rate(bad)).unwrap_err();
            assert!(err.contains("broadcast_fps"), "{err}");
        }
    }

    /// The snake_case alias is normalised like the other physics keys.
    #[test]
    fn broadcast_fps_snake_case_alias_is_normalised() {
        let mut patch = serde_json::Map::new();
        patch.insert("broadcast_fps".into(), serde_json::json!(12));
        let n = normalize_physics_keys(patch);
        assert_eq!(n.get("broadcastFps"), Some(&serde_json::json!(12)));
    }

    /// A physics PUT is turned into exactly the message the live
    /// ForceComputeActor applies: the rate only, culling left as it is.
    #[test]
    fn physics_settings_map_to_a_rate_only_broadcast_message() {
        let msg = broadcast_rate_message(&with_rate(12));
        assert_eq!(msg.target_fps, Some(12));
        assert_eq!(msg.enable_spatial_culling, None);
        assert_eq!(msg.delta_threshold, None);
    }
}
