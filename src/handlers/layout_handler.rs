use crate::adapters::SqliteSettingsRepository;
use crate::layout::engines::compute_layout;
use crate::layout::types::*;
use crate::ok_json;
use crate::settings::api::settings_routes::{
    apply_physics_patch, stored_physics, PhysicsPatchError,
};
use crate::settings::auth_extractor::AuthenticatedUser;
use crate::AppState;
use actix_web::{web, HttpResponse, Result};
use std::sync::Arc;

// The layout mode is one stored field, `physics.layoutMode` (ADR-2140), and
// `POST /api/layout/mode` writes it through `apply_physics_patch`, the same path as
// `PUT /api/settings/physics`. What each mode does on the device is decided by the
// stored-to-device conversion (`SimulationParams::resolve_mode_defaults`): `radial` primes
// the `dag_radial_bias` shell, `hierarchical` and `dagLeftRight` the Sugiyama layer spring,
// `typeClustering` the cluster strength; `spectral` and `temporal` are CPU one-shot
// placements computed by `compute_layout` below, so the stored value says "place once" and
// the device keeps its force-directed baseline. `clustered` rides the stored
// `clusterStrength` knob and is otherwise the force-directed baseline, so it is not
// advertised, but it is accepted and stored.
const LIVE_LAYOUT_MODES: [&str; 7] = [
    "forceDirected",
    "hierarchical",
    "radial",
    "dagLeftRight",
    "typeClustering",
    "spectral",
    "temporal",
];

pub async fn get_layout_modes(data: web::Data<AppState>) -> Result<HttpResponse> {
    let current = stored_physics(&data)
        .await
        .map(|p| p.layout_mode)
        .unwrap_or_default();
    ok_json!(serde_json::json!({
        "current": current,
        "available": LIVE_LAYOUT_MODES,
        "transitioning": false
    }))
}

pub async fn set_layout_mode(
    auth: AuthenticatedUser,
    data: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse> {
    let mode_str = body
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("forceDirected");
    let transition_ms = body
        .get("transitionMs")
        .and_then(|t| t.as_u64())
        .unwrap_or(500);

    // Reject an unrecognised mode instead of silently coercing to
    // ForceDirected — a typo'd or stale client-side mode name must not read
    // as "applied" when it was actually ignored. Either vocabulary is read: the
    // camelCase name or a quality-gate name such as `dag-topdown`.
    let mode: LayoutMode = match LayoutMode::parse(mode_str) {
        Some(m) => m,
        None => {
            return Err(actix_web::error::ErrorBadRequest(format!(
                "unknown layout mode '{}' (expected one of: forceDirected, hierarchical, \
                 radial, spectral, temporal, clustered, dagLeftRight, typeClustering)",
                mode_str
            )));
        }
    };

    // The one write path: merge, validate, store, push to the GPU actors, persist, broadcast.
    // A request for the mode that is already stored writes nothing: the client repeats this
    // call on every page load, and a repeat must not rewrite the store, broadcast to other
    // sessions or resume a settled simulation. CPU one-shot placement below still runs.
    let mut persisted: std::result::Result<(), String> = Ok(());
    if stored_physics(&data).await.map(|p| p.layout_mode) == Some(mode) {
        log::debug!(
            "set_layout_mode: {} is already stored; nothing to write",
            mode_str
        );
    } else {
        match apply_physics_patch(
            &data,
            settings_repo.get_ref(),
            serde_json::json!({ "layoutMode": mode }),
            &auth.pubkey,
        )
        .await
        {
            Ok(_) => {}
            Err(e @ PhysicsPatchError::BadRequest(_)) => return Ok(e.into_response()),
            Err(PhysicsPatchError::Internal(e)) => {
                log::warn!("set_layout_mode: could not apply mode {}: {}", mode_str, e);
                persisted = Err(e);
            }
        }
    }

    // GPU-resident modes settle continuously on the GPU; no CPU one-shot positions.
    // Their entire effect IS the persisted mode, so a persistence failure must be
    // reported as a failure rather than a hollow success with no positions.
    if mode.is_gpu_resident() {
        return match persisted {
            Ok(()) => ok_json!(serde_json::json!({
                "success": true,
                "mode": mode_str,
                "transitionMs": transition_ms,
                "positions": []
            })),
            Err(e) => ok_json!(serde_json::json!({
                "success": false,
                "mode": mode_str,
                "error": format!("Failed to apply layout mode: {}", e)
            })),
        };
    }

    // CPU one-shot modes (Spectral/Temporal): compute placement below. Every other mode is
    // GPU-resident and returns above. A failed write is reported, not placed.
    if let Err(e) = persisted {
        return ok_json!(serde_json::json!({
            "success": false,
            "mode": mode_str,
            "error": format!("Failed to apply layout mode: {}", e)
        }));
    }
    // Fetch current graph data
    use crate::actors::messages::GetGraphData;
    let graph_data = match data.graph_service_addr.send(GetGraphData).await {
        Ok(Ok(gd)) => gd,
        Ok(Err(e)) => {
            log::error!("set_layout_mode: failed to get graph data: {}", e);
            return ok_json!(serde_json::json!({
                "success": false,
                "error": "Failed to retrieve graph data",
                "mode": mode_str
            }));
        }
        Err(e) => {
            log::error!("set_layout_mode: actor mailbox error: {}", e);
            return ok_json!(serde_json::json!({
                "success": false,
                "error": "Graph service unavailable",
                "mode": mode_str
            }));
        }
    };

    // Convert graph data to the flat slices expected by compute_layout
    let nodes: Vec<(u32, String)> = graph_data
        .nodes
        .iter()
        .map(|n| (n.id, n.label.clone()))
        .collect();

    let edges: Vec<(u32, u32, f32)> = graph_data
        .edges
        .iter()
        .map(|e| (e.source, e.target, e.weight))
        .collect();

    let config = LayoutModeConfig {
        mode,
        ..LayoutModeConfig::default()
    };

    let raw_positions = compute_layout(&mode, &nodes, &edges, &config);

    // Build JSON position array [{id, x, y, z}, ...]
    let positions: Vec<serde_json::Value> = nodes
        .iter()
        .zip(raw_positions.iter())
        .map(|((id, _label), &(x, y, z))| serde_json::json!({ "id": id, "x": x, "y": y, "z": z }))
        .collect();

    ok_json!(serde_json::json!({
        "success": true,
        "mode": mode_str,
        "transitionMs": transition_ms,
        "positions": positions
    }))
}

/// ADR-141 P3: re-key the radial shells of the `dag_radial_bias` term. Body:
/// `{ "mode": "dagRank"|"typeTier"|"ego", "focusNode": <u32|null>, "transitionMs": <u64> }`.
/// Mirrors `set_layout_mode`'s persistence pattern: the mode is applied on the
/// actor via `SetRadialLayout`; a failure returns `success:false`.
pub async fn set_radial_layout(
    data: web::Data<AppState>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse> {
    let mode_str = body
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("dagRank");
    let focus_node = body
        .get("focusNode")
        .and_then(|f| f.as_u64())
        .map(|v| v as u32);
    let transition_ms = body
        .get("transitionMs")
        .and_then(|t| t.as_u64())
        .unwrap_or(500);

    // Reject an unrecognised mode instead of silently running the default and
    // returning success — a typo'd mode string must not read as applied.
    let mode: RadialMode = match serde_json::from_value(serde_json::Value::String(
        mode_str.to_string(),
    )) {
        Ok(m) => m,
        Err(_) => {
            return ok_json!(serde_json::json!({
                "success": false,
                "mode": mode_str,
                "error": format!("unknown radial mode '{}' (expected dagRank|typeTier|ego)", mode_str)
            }));
        }
    };

    if let Some(addr) = data.get_gpu_compute_addr().await {
        use crate::actors::messages::SetRadialLayout;
        match addr.send(SetRadialLayout { mode, focus_node }).await {
            Ok(Ok(())) => ok_json!(serde_json::json!({
                "success": true,
                "mode": mode_str,
                "focusNode": focus_node,
                "transitionMs": transition_ms
            })),
            Ok(Err(e)) => {
                log::warn!("set_radial_layout: actor rejected mode {}: {}", mode_str, e);
                ok_json!(serde_json::json!({
                    "success": false,
                    "mode": mode_str,
                    "error": format!("Failed to apply radial layout: {}", e)
                }))
            }
            Err(e) => {
                log::warn!("set_radial_layout: actor mailbox error: {}", e);
                ok_json!(serde_json::json!({
                    "success": false,
                    "mode": mode_str,
                    "error": format!("actor mailbox error: {}", e)
                }))
            }
        }
    } else {
        log::warn!("set_radial_layout: GPU compute actor unavailable — radial layout not applied");
        ok_json!(serde_json::json!({
            "success": false,
            "mode": mode_str,
            "error": "GPU compute actor unavailable"
        }))
    }
}

pub async fn get_layout_status(data: web::Data<AppState>) -> Result<HttpResponse> {
    // See `LIVE_LAYOUT_MODES` above for why `Clustered` is not advertised.
    let current_mode = stored_physics(&data)
        .await
        .map(|p| p.layout_mode)
        .unwrap_or_default();
    ok_json!(LayoutStatus {
        current_mode,
        transitioning: false,
        transition_progress: 1.0,
        iterations: 0,
        converged: false,
        kinetic_energy: 0.0,
        available_modes: vec![
            LayoutMode::ForceDirected,
            LayoutMode::Hierarchical,
            LayoutMode::Radial,
            LayoutMode::DagLeftRight,
            LayoutMode::TypeClustering,
            LayoutMode::Spectral,
            LayoutMode::Temporal,
        ],
    })
}

pub async fn set_zones(
    _data: web::Data<AppState>,
    body: web::Json<Vec<ConstraintZone>>,
) -> Result<HttpResponse> {
    // TODO: Forward zones to ForceComputeActor
    ok_json!(serde_json::json!({
        "success": true,
        "zones": body.into_inner().len()
    }))
}

pub async fn get_zones(_data: web::Data<AppState>) -> Result<HttpResponse> {
    ok_json!(serde_json::json!({
        "zones": []
    }))
}

pub async fn reset_layout(data: web::Data<AppState>) -> Result<HttpResponse> {
    use crate::actors::messages::ResetPositions;

    if let Some(addr) = data.get_gpu_compute_addr().await {
        match addr.send(ResetPositions).await {
            Ok(Ok(_)) => {
                ok_json!(serde_json::json!({
                    "success": true,
                    "message": "Layout reset triggered — positions randomized and reheat applied"
                }))
            }
            Ok(Err(e)) => {
                ok_json!(serde_json::json!({
                    "success": false,
                    "message": format!("Reset failed: {}", e)
                }))
            }
            Err(e) => {
                ok_json!(serde_json::json!({
                    "success": false,
                    "message": format!("ForceComputeActor mailbox error: {}", e)
                }))
            }
        }
    } else {
        ok_json!(serde_json::json!({
            "success": false,
            "message": "GPU compute actor not available — layout reset skipped"
        }))
    }
}

pub fn configure_layout_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/layout")
            .route("/modes", web::get().to(get_layout_modes))
            // `POST /layout/mode` is a full physics write (ADR-2140), so it is limited like
            // `/api/settings/*` and `/analytics/sssp/toggle`.
            .service(
                web::resource("/mode")
                    .wrap(crate::middleware::rate_limit::RateLimit::per_minute(60))
                    .route(web::post().to(set_layout_mode)),
            )
            .route("/radial", web::post().to(set_radial_layout))
            .route("/status", web::get().to(get_layout_status))
            .route("/zones", web::post().to(set_zones))
            .route("/zones", web::get().to(get_zones))
            .route("/reset", web::post().to(reset_layout)),
    );
}
