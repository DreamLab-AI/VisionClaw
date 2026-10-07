use crate::layout::engines::compute_layout;
use crate::layout::types::*;
use crate::ok_json;
use crate::AppState;
use actix_web::{web, HttpResponse, Result};

// The modes the live `SetLayoutMode` path on `ForceComputeActor`
// (src/actors/gpu/force_compute_actor.rs) actually differentiates:
// `ForceDirected` is the GPU-resident default baseline; `Radial` and
// `Hierarchical` prime a real GPU force term (`dag_bias_k` / `layer_bias_k`
// respectively — both read by the CUDA kernel); `Spectral` and `Temporal`
// are CPU one-shot placements computed by `compute_layout` below.
// `LayoutMode::Clustered` is deliberately excluded: it is classified
// `is_gpu_resident() == true` so the early return below skips CPU placement,
// but the actor's `SetLayoutMode` handler has no dedicated match arm for it —
// it falls into the same catch-all as `ForceDirected` and clears both bias
// terms. The only related term, `cluster_strength` (driving
// `cluster_cohesion_kernel`), is an independent `/api/settings/physics` knob
// applied unconditionally whenever it is > 0, regardless of the selected
// layout mode. Selecting `Clustered` is therefore currently indistinguishable
// from `ForceDirected` — it is not advertised until that gap is wired up.
const LIVE_LAYOUT_MODES: [&str; 5] = [
    "forceDirected",
    "hierarchical",
    "radial",
    "spectral",
    "temporal",
];

pub async fn get_layout_modes(_data: web::Data<AppState>) -> Result<HttpResponse> {
    ok_json!(serde_json::json!({
        "current": "forceDirected",
        "available": LIVE_LAYOUT_MODES,
        "transitioning": false
    }))
}

pub async fn set_layout_mode(
    data: web::Data<AppState>,
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
    // as "applied" when it was actually ignored.
    let mode: LayoutMode =
        match serde_json::from_value(serde_json::Value::String(mode_str.to_string())) {
            Ok(m) => m,
            Err(_) => {
                return Err(actix_web::error::ErrorBadRequest(format!(
                    "unknown layout mode '{}' (expected one of: forceDirected, hierarchical, \
                     radial, spectral, temporal, clustered)",
                    mode_str
                )));
            }
        };

    // ADR-141 P1: persist the active mode into the GPU-visible SimParams.layout_mode
    // so both clients (XR + desktop) share one authoritative layout mode. For
    // GPU-resident modes (ForceDirected/Radial/Clustered) the SetLayoutMode handler
    // also primes the relevant force-term scalars, and the GPU keeps streaming
    // positions — so we return no one-shot positions and let the settling engine run.
    let persisted = if let Some(addr) = data.get_gpu_compute_addr().await {
        use crate::actors::messages::SetLayoutMode;
        match addr.send(SetLayoutMode { mode }).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                log::warn!("set_layout_mode: actor rejected mode {}: {}", mode_str, e);
                Err(e)
            }
            Err(e) => {
                log::warn!("set_layout_mode: actor mailbox error: {}", e);
                Err(format!("actor mailbox error: {}", e))
            }
        }
    } else {
        log::warn!("set_layout_mode: GPU compute actor unavailable — mode not persisted GPU-side");
        Err("GPU compute actor unavailable".to_string())
    };

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

    // CPU one-shot modes (Spectral/Temporal): compute placement below. Hierarchical
    // is now GPU-resident (ADR-141 P4 Sugiyama layer spring) and returns above.
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

pub async fn get_layout_status(_data: web::Data<AppState>) -> Result<HttpResponse> {
    // See `LIVE_LAYOUT_MODES` above for why `Clustered` is excluded.
    ok_json!(LayoutStatus {
        current_mode: LayoutMode::ForceDirected,
        transitioning: false,
        transition_progress: 1.0,
        iterations: 0,
        converged: false,
        kinetic_energy: 0.0,
        available_modes: vec![
            LayoutMode::ForceDirected,
            LayoutMode::Hierarchical,
            LayoutMode::Radial,
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
            .route("/mode", web::post().to(set_layout_mode))
            .route("/radial", web::post().to(set_radial_layout))
            .route("/status", web::get().to(get_layout_status))
            .route("/zones", web::post().to(set_zones))
            .route("/zones", web::get().to(get_zones))
            .route("/reset", web::post().to(reset_layout)),
    );
}
