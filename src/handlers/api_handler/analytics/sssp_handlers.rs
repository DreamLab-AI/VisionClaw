use actix_web::{web, HttpResponse, Result};
use log::{debug, error, info, warn};

use crate::AppState;
use crate::{error_json, ok_json};

use super::state::FEATURE_FLAGS;
use super::types::{SSSPToggleRequest, SSSPToggleResponse};

use std::sync::Arc;

use crate::adapters::SqliteSettingsRepository;
use crate::settings::api::settings_routes::{apply_physics_patch, PhysicsPatchError};

/// The physics patch an SSSP toggle writes: only the two SSSP fields, so the deep merge leaves
/// every other stored value alone. `alpha` is omitted when the request did not name one.
pub(crate) fn sssp_toggle_patch(enabled: bool, alpha: Option<f32>) -> serde_json::Value {
    let mut patch = serde_json::json!({ "ssspEnabled": enabled });
    if let Some(alpha) = alpha {
        patch["ssspAlpha"] = serde_json::json!(alpha);
    }
    patch
}

#[cfg(test)]
mod sssp_patch_tests {
    use super::*;
    use crate::config::PhysicsSettings;
    use crate::models::simulation_params::SimulationParams;
    use crate::settings::api::settings_routes::merge_physics_patch;

    fn stored() -> PhysicsSettings {
        let mut p = PhysicsSettings::default();
        p.repel_k = 321.0;
        p.rest_length = 77.0;
        p.damping = 0.42;
        p.sssp_alpha = 0.3;
        p
    }

    fn apply(enabled: bool, alpha: Option<f32>) -> PhysicsSettings {
        let current = serde_json::to_value(stored()).unwrap();
        merge_physics_patch(current, sssp_toggle_patch(enabled, alpha))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn the_toggle_patch_names_only_the_sssp_fields() {
        assert_eq!(
            sssp_toggle_patch(false, None),
            serde_json::json!({"ssspEnabled": false})
        );
        let with_alpha = sssp_toggle_patch(true, Some(0.4));
        assert_eq!(with_alpha["ssspEnabled"], true);
        assert!((with_alpha["ssspAlpha"].as_f64().unwrap() - 0.4).abs() < 1e-6);
        assert_eq!(with_alpha.as_object().unwrap().len(), 2);
    }

    #[test]
    fn toggling_through_the_store_changes_only_the_sssp_fields() {
        let out = apply(false, Some(0.4));
        assert!(!out.sssp_enabled);
        assert!((out.sssp_alpha - 0.4).abs() < 1e-6);
        assert_eq!(
            (out.repel_k, out.rest_length, out.damping),
            (321.0, 77.0, 0.42),
            "other stored physics must survive the toggle"
        );
    }

    #[test]
    fn omitted_alpha_keeps_the_stored_alpha() {
        let out = apply(true, None);
        assert!(out.sssp_enabled);
        assert!((out.sssp_alpha - 0.3).abs() < 1e-6);
    }

    #[test]
    fn the_stored_toggle_is_what_the_gpu_receives() {
        use crate::models::simulation_params::{FeatureFlags, SimParams};
        for enabled in [true, false] {
            let stored = apply(enabled, None);
            let live: SimulationParams = (&stored).into();
            assert_eq!(live.use_sssp_distances, enabled);
            let gpu: SimParams = (&stored).into();
            assert_eq!(
                gpu.feature_flags & FeatureFlags::ENABLE_SSSP_SPRING_ADJUST != 0,
                enabled
            );
        }
    }

    #[test]
    fn a_stored_toggle_changes_the_gpu_image_the_actor_gate_compares() {
        // `ForceComputeActor::UpdateSimulationParams` skips an update whose GPU image
        // (`SimParams` bytes) equals the current one, so a toggle must change those bytes.
        use crate::models::simulation_params::SimParams;
        let image = |enabled: bool| {
            let live: SimulationParams = (&apply(enabled, None)).into();
            bytemuck::bytes_of(&SimParams::from(&live)).to_vec()
        };
        assert_ne!(image(true), image(false));
    }

    #[test]
    fn an_out_of_range_alpha_is_refused_by_the_central_validator() {
        let bad = apply(true, Some(9.0));
        assert!(crate::settings::api::settings_routes::validate_physics_settings(&bad).is_err());
    }
}

pub async fn toggle_sssp(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    request: web::Json<SSSPToggleRequest>,
) -> Result<HttpResponse> {
    info!(
        "Toggling SSSP spring adjustment: enabled={}, alpha={:?}",
        request.enabled, request.alpha
    );

    // The one write path (ADR-2140 decision 3): the toggle is a physics patch, validated by the
    // central validator and applied to the store, the GPU actors and SQLite together.
    let patch = sssp_toggle_patch(request.enabled, request.alpha);
    match apply_physics_patch(&app_state, settings_repo.get_ref(), patch, &auth.pubkey).await {
        Ok(stored) => {
            FEATURE_FLAGS.lock().await.sssp_integration = stored.sssp_enabled;
            let message = if stored.sssp_enabled {
                format!(
                    "SSSP spring adjustment enabled with alpha={:.2}",
                    stored.sssp_alpha
                )
            } else {
                "SSSP spring adjustment disabled".to_string()
            };
            info!("Successfully toggled SSSP: {}", message);
            ok_json!(SSSPToggleResponse {
                success: true,
                enabled: stored.sssp_enabled,
                alpha: Some(stored.sssp_alpha),
                message,
                error: None,
            })
        }
        Err(PhysicsPatchError::BadRequest(e)) => {
            warn!("SSSP toggle refused: {}", e);
            Ok(HttpResponse::BadRequest().json(SSSPToggleResponse {
                success: false,
                enabled: false,
                alpha: None,
                message: "Invalid SSSP parameters".to_string(),
                error: Some(e),
            }))
        }
        Err(PhysicsPatchError::Internal(e)) => {
            error!("Failed to store SSSP toggle: {}", e);
            Ok(
                HttpResponse::InternalServerError().json(SSSPToggleResponse {
                    success: false,
                    enabled: false,
                    alpha: None,
                    message: "Failed to update physics settings".to_string(),
                    error: Some(e),
                }),
            )
        }
    }
}

pub async fn get_sssp_status(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    // Read the store, not the display flag: the stored physics document is the one definition.
    let enabled = match app_state
        .settings_addr
        .send(crate::actors::messages::GetSettings)
        .await
    {
        Ok(Ok(settings)) => settings.visualisation.graphs.knowledge.physics.sssp_enabled,
        _ => FEATURE_FLAGS.lock().await.sssp_integration,
    };

    ok_json!(serde_json::json!({
        "success": true,
        "enabled": enabled,
        "description": "Single-Source Shortest Path spring adjustment for improved edge length uniformity",
        "feature_flag": "FeatureFlags::ENABLE_SSSP_SPRING_ADJUST"
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

pub async fn get_sssp_params(
    _app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
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
