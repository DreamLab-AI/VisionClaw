use actix_web::{web, HttpResponse, Result};
use log::{debug, error, info, warn};

use crate::AppState;
use crate::{error_json, ok_json};

use super::state::FEATURE_FLAGS;
use super::types::{SSSPToggleRequest, SSSPToggleResponse};

use crate::models::simulation_params::SimulationParams;

/// The params to send for an SSSP toggle: the current ones with ONLY the SSSP fields
/// changed. This used to start from `SimulationParams::new()`, so toggling SSSP silently
/// reset every other physics setting to its default on the GPU (and clients were never told).
pub(crate) fn sssp_patched(
    mut base: SimulationParams,
    enabled: bool,
    alpha: Option<f32>,
) -> SimulationParams {
    base.use_sssp_distances = enabled;
    if alpha.is_some() {
        base.sssp_alpha = alpha;
    }
    base
}

#[cfg(test)]
mod sssp_patch_tests {
    use super::*;

    #[test]
    fn toggling_sssp_changes_only_the_sssp_fields() {
        let mut live = SimulationParams::default();
        live.repel_k = 321.0;
        live.rest_length = 77.0;
        live.damping = 0.42;
        let out = sssp_patched(live.clone(), true, Some(0.4));
        assert!(out.use_sssp_distances);
        assert_eq!(out.sssp_alpha, Some(0.4));
        assert_eq!(
            (out.repel_k, out.rest_length, out.damping),
            (321.0, 77.0, 0.42),
            "other physics must survive the toggle"
        );
    }

    #[test]
    fn omitted_alpha_keeps_the_current_alpha() {
        let mut live = SimulationParams::default();
        live.sssp_alpha = Some(0.3);
        assert_eq!(sssp_patched(live, false, None).sssp_alpha, Some(0.3));
    }
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

    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        let message = crate::actors::messages::UpdateSimulationParams {
            params: sssp_patched(
                live_params(&app_state).await,
                request.enabled,
                request.alpha,
            ),
        };

        match gpu_addr.send(message).await {
            Ok(Ok(_)) => {
                // Update feature flag only after GPU succeeds
                let mut flags = FEATURE_FLAGS.lock().await;
                flags.sssp_integration = request.enabled;
                drop(flags);

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
        // No GPU available — safe to update flag directly
        let mut flags = FEATURE_FLAGS.lock().await;
        flags.sssp_integration = request.enabled;
        drop(flags);

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

/// Current physics as the GPU should hold them: the persisted settings (what the control
/// panel shows), falling back to defaults only when nothing is stored.
async fn live_params(app_state: &web::Data<AppState>) -> SimulationParams {
    use crate::config::PhysicsSettings;
    use crate::ports::settings_repository::{SettingValue, SettingsRepository};
    if let Ok(Some(SettingValue::Json(json))) = app_state
        .sqlite_settings_repository
        .get_setting("physics")
        .await
    {
        if let Ok(ps) = serde_json::from_value::<PhysicsSettings>(json) {
            return (&ps).into();
        }
    }
    SimulationParams::default()
}
