//! Ontology-Physics Integration API Handlers
//!
//! Provides REST API endpoints for managing ontology-driven physics forces,
//! enabling semantic relationships to influence graph layout through GPU-accelerated
//! constraint forces.
//!
//! ## Architecture
//!
//! This module bridges the ontology validation system with the GPU physics pipeline:
//! - OntologyActor -> OntologyConstraintActor -> ForceComputeActor
//! - Constraints are uploaded to GPU and applied during physics simulation

use crate::{error_json, ok_json, service_unavailable};
use actix_web::{web, HttpResponse, Responder};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::actors::messages::GetOntologyConstraintStats;
use crate::adapters::SqliteSettingsRepository;
use crate::settings::api::settings_routes::apply_physics_patch;
use crate::AppState;
use std::sync::Arc;
use visionclaw_adapters::provenance_emitter;

// ============================================================================
// REQUEST/RESPONSE DTOs
// ============================================================================

/// Body of `POST /api/ontology-physics/enable`. Every field is optional; older clients still
/// send `ontologyId` and `mergeMode`, which are accepted and ignored: the constraints come from
/// the last sync's reasoning, not from a named ontology (ADR-2192).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableOntologyPhysicsRequest {
    /// Also store this strength (0.0 to 1.0); absent keeps the stored strength.
    pub strength: Option<f32>,
}

/// Constraint list response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintListResponse {
    pub active_constraints: u32,
    pub total_constraints: u32,
    pub constraint_types: Vec<String>,
    pub ontology_id: Option<String>,
}

/// Body of `PUT /api/ontology-physics/weights`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustWeightsRequest {
    /// The strength to store (0.0 to 1.0). Required.
    pub global_strength: Option<f32>,
}

/// The physics patch `POST /enable` applies: constraints on, and the strength if one was given.
pub(crate) fn enable_patch(strength: Option<f32>) -> serde_json::Value {
    let mut patch = json!({ "ontologyConstraintsEnabled": true });
    if let Some(s) = strength {
        patch["ontologyConstraintStrength"] = json!(s);
    }
    patch
}

/// The physics patch `POST /disable` applies.
pub(crate) fn disable_patch() -> serde_json::Value {
    json!({ "ontologyConstraintsEnabled": false })
}

/// The physics patch `PUT /weights` applies, or why there is none. Out-of-range values are left
/// to the central physics validator, which answers 400 rather than clamping (ADR-2140).
pub(crate) fn weights_patch(global_strength: Option<f32>) -> Result<serde_json::Value, String> {
    global_strength
        .map(|s| json!({ "ontologyConstraintStrength": s }))
        .ok_or_else(|| "globalStrength is required".to_string())
}

/// Write `patch` through the one physics write path and answer with the stored policy.
async fn store_policy(
    state: &web::Data<AppState>,
    settings_repo: &Arc<SqliteSettingsRepository>,
    patch: serde_json::Value,
    pubkey: &str,
) -> HttpResponse {
    match apply_physics_patch(state, settings_repo, patch, pubkey).await {
        Ok(stored) => HttpResponse::Ok().json(json!({
            "success": true,
            "enabled": stored.ontology_constraints_enabled,
            "appliedStrength": stored.ontology_constraint_strength,
        })),
        Err(e) => e.into_response(),
    }
}

// ============================================================================
// FEATURE CHECK
// ============================================================================

/// Check if ontology feature is enabled (always enabled)
async fn check_ontology_feature() -> Result<(), actix_web::Error> {
    Ok(())
}

// ============================================================================
// REST ENDPOINTS
// ============================================================================

/// POST /api/ontology-physics/enable - switch the reasoner-derived constraints on.
///
/// Stores `physics.ontologyConstraintsEnabled = true` (and `ontologyConstraintStrength` when
/// `strength` is given) through the physics write path, which re-applies the set the last sync
/// produced (ADR-2192). The body is optional.
pub async fn enable_ontology_physics(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    req: Option<web::Json<EnableOntologyPhysicsRequest>>,
) -> impl Responder {
    let strength = req.and_then(|r| r.strength);
    info!(
        "POST /api/ontology-physics/enable (strength {:?})",
        strength
    );
    store_policy(
        &state,
        settings_repo.get_ref(),
        enable_patch(strength),
        &auth.pubkey,
    )
    .await
}

/// GET /api/ontology-physics/constraints - List active ontology constraints
/// Returns statistics about currently active ontology-derived physics constraints.
/// ## Response
/// ```json
/// {
///   "activeConstraints": 42,
///   "totalConstraints": 50,
///   "constraintTypes": ["SubClassOf", "DisjointWith", "EquivalentClasses"],
///   "ontologyId": "university-ontology"
/// }
/// ```
pub async fn get_constraints(state: web::Data<AppState>) -> impl Responder {
    info!("GET /api/ontology-physics/constraints");

    check_ontology_feature().await?;

    let Some(ref gpu_manager_addr) = state.gpu_manager_addr else {
        return service_unavailable!(json!({
            "error": "GPU manager not available"
        }));
    };

    // Get ontology constraint stats
    match gpu_manager_addr.send(GetOntologyConstraintStats).await {
        Ok(Ok(stats)) => {
            ok_json!(json!({
                "activeConstraints": stats.active_ontology_constraints,
                // Constraints currently driving GPU forces. Post-dispatch every
                // uploaded constraint is active, so total == active here; the
                // distinct axiom input count is surfaced as `axiomsProcessed`.
                "totalConstraints": stats.active_ontology_constraints,
                "axiomsProcessed": stats.total_axioms_processed,
                "constraintEvaluationCount": stats.constraint_evaluation_count,
                "lastUpdateTimeMs": stats.last_update_time_ms,
                "gpuFailureCount": stats.gpu_failure_count,
                "cpuFallbackCount": stats.cpu_fallback_count
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to get constraint stats: {}", e);
            error_json!(json!({
                "error": "Stats retrieval failed",
                "details": e
            }))
        }
        Err(e) => {
            error!("GPU Manager mailbox error: {}", e);
            error_json!(json!({
                "error": "Actor communication failed",
                "details": e.to_string()
            }))
        }
    }
}

/// PUT /api/ontology-physics/weights - Adjust constraint strengths
/// Modifies the strength of ontology-derived forces without reloading constraints.
/// ## Request Body
/// ```json
/// {
///   "globalStrength": 0.5,
///   "constraintWeights": {
///     "SubClassOf": 0.8,
///     "DisjointWith": 1.0
///   }
/// }
/// ```
/// ## Response
/// ```json
/// {
///   "success": true,
///   "appliedStrength": 0.5,
///   "activeConstraints": 42
/// }
/// ```
pub async fn adjust_weights(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
    req: web::Json<AdjustWeightsRequest>,
) -> impl Responder {
    info!("PUT /api/ontology-physics/weights");
    match weights_patch(req.global_strength) {
        Ok(patch) => store_policy(&state, settings_repo.get_ref(), patch, &auth.pubkey).await,
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e })),
    }
}

/// POST /api/ontology-physics/disable - switch the reasoner-derived constraints off.
///
/// Stores `physics.ontologyConstraintsEnabled = false` through the physics write path, which
/// removes the constraints from the GPU (ADR-2192). The reasoned set is kept, so `/enable`
/// re-applies it without a sync.
pub async fn disable_ontology_physics(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    settings_repo: web::Data<Arc<SqliteSettingsRepository>>,
) -> impl Responder {
    info!("POST /api/ontology-physics/disable");
    store_policy(
        &state,
        settings_repo.get_ref(),
        disable_patch(),
        &auth.pubkey,
    )
    .await
}

// ============================================================================
// ROUTE CONFIGURATION
// ============================================================================

/// GET /api/ontology-physics/trust-status — PRD-022 WS-5 liveness canary.
///
/// Reports the semantic trust layer health: shapes loaded, provenance
/// triples stored, gate modes. Enables operators and CI to verify that
/// WS-0/WS-1/WS-2 are wired live, not silently inert.
pub async fn get_trust_status(state: web::Data<AppState>) -> impl Responder {
    info!("GET /api/ontology-physics/trust-status");

    let store = state.ontology_repository.store();

    let shapes_loaded = provenance_emitter::count_shapes_loaded(store).unwrap_or(0);
    let provenance_triples = provenance_emitter::count_provenance_triples(store).unwrap_or(0);

    // PRD-022 WS-1: report the REAL enforcement posture. The shape-driven SHACL
    // validator now consumes the loaded NodeShapes, and the ingest gate rejects
    // `sh:Violation` findings on the write path when the process-wide mode
    // (seeded from `ontology_agent.shacl_mode`) is enforcing.
    let gate_mode = visionclaw_ontology::services::jsonld_ingest::shacl_gate::global_gate_mode();
    let mode_str = gate_mode.as_str();

    let shapes_healthy = shapes_loaded > 0;
    let provenance_healthy = true;

    let status = if shapes_healthy {
        "healthy"
    } else {
        "degraded"
    };

    ok_json!(json!({
        "status": status,
        "shacl": {
            "shapesLoaded": shapes_loaded,
            // The ingest gate is the shape-driven validator: it parses the five
            // `.shacl.ttl` NodeShapes and validates every ingested JSON-LD entry
            // against them. On the write path a `sh:Violation` finding rejects
            // the ingest when the gate mode is enforcing (`sh:Warning` stays
            // advisory in every mode). This report reflects the LIVE mode.
            "engine": "shape-driven",
            "gateModes": {
                // Write (ingest) path honours the configured mode; read/query
                // paths never gate (advisory).
                "writePaths": mode_str,
                "readPaths": "advisory"
            },
            // The loaded NodeShapes are now consumed by the running validator;
            // enforcement is active whenever writePaths == "enforcing".
            "w3cEnforcement": mode_str,
            "healthy": shapes_healthy
        },
        "provenance": {
            "triplesStored": provenance_triples,
            "graphName": "urn:ngm:graph:provenance",
            "appendOnly": true,
            "healthy": provenance_healthy
        },
        "federation": {
            "status": "deferred",
            "reason": "Relay-mediated SPARQL federation deferred to Phase 2 (PRD-022 §WS-3)"
        }
    }))
}

/// Configure ontology-physics routes
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/ontology-physics")
            .route("/enable", web::post().to(enable_ontology_physics))
            .route("/disable", web::post().to(disable_ontology_physics))
            .route("/constraints", web::get().to(get_constraints))
            .route("/weights", web::put().to(adjust_weights))
            .route("/trust-status", web::get().to(get_trust_status)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-2192: the three legacy routes are thin writers of the two stored keys.
    #[test]
    fn the_routes_patch_only_the_stored_policy_keys() {
        assert_eq!(
            enable_patch(None),
            json!({ "ontologyConstraintsEnabled": true })
        );
        assert_eq!(
            enable_patch(Some(0.3)),
            json!({ "ontologyConstraintsEnabled": true, "ontologyConstraintStrength": 0.3_f32 })
        );
        assert_eq!(
            disable_patch(),
            json!({ "ontologyConstraintsEnabled": false })
        );
        assert_eq!(
            weights_patch(Some(0.8)).unwrap(),
            json!({ "ontologyConstraintStrength": 0.8_f32 })
        );
        assert!(weights_patch(None).is_err());
    }

    /// The patches merge onto a stored document and pass the central validator; an
    /// out-of-range strength is refused there, not clamped.
    #[test]
    fn the_patches_merge_and_validate_and_out_of_range_is_refused() {
        use crate::settings::api::settings_routes::{
            merge_physics_patch, validate_physics_settings,
        };
        let base = serde_json::to_value(crate::config::PhysicsSettings::default()).unwrap();
        let off = merge_physics_patch(base.clone(), disable_patch())
            .unwrap()
            .unwrap();
        assert!(!off.ontology_constraints_enabled);
        assert!(validate_physics_settings(&off).is_ok());
        let on = merge_physics_patch(serde_json::to_value(&off).unwrap(), enable_patch(Some(0.4)))
            .unwrap()
            .unwrap();
        assert!(on.ontology_constraints_enabled);
        assert_eq!(on.ontology_constraint_strength, 0.4);
        let too_strong = merge_physics_patch(base, weights_patch(Some(1.5)).unwrap())
            .unwrap()
            .unwrap();
        assert!(validate_physics_settings(&too_strong).is_err());
    }
}
