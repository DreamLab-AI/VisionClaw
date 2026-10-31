use actix_web::{web, HttpResponse, Result};
use log::info;

use crate::ok_json;

use super::sssp_handlers::stored_sssp_enabled;
use super::state::FEATURE_FLAGS;
use super::types::FeatureFlags;
use crate::AppState;

/// The flags as reported: `sssp_integration` is read-only and derived from the stored
/// `physics.ssspEnabled` (ADR-2140), never from `FEATURE_FLAGS`.
async fn reported_flags(app_state: &AppState) -> FeatureFlags {
    FeatureFlags {
        ontology_validation: FEATURE_FLAGS.lock().await.ontology_validation,
        sssp_integration: stored_sssp_enabled(app_state).await,
    }
}

pub async fn get_feature_flags(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    let flags = reported_flags(&app_state).await;

    ok_json!(serde_json::json!({
        "success": true,
        "flags": flags,
        "description": {
            "sssp_integration": "Read-only: mirrors the stored physics.ssspEnabled and gates no behaviour; change it with /sssp/toggle or PUT /api/settings/physics",
            "ontology_validation": "Enable ontology validation and inference operations"
        }
    }))
}

pub async fn update_feature_flags(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<FeatureFlags>,
) -> Result<HttpResponse> {
    info!("Updating analytics feature flags");

    // `sssp_integration` in the body is accepted for compatibility and ignored: the store owns it.
    FEATURE_FLAGS.lock().await.ontology_validation = request.ontology_validation;
    let flags = reported_flags(&app_state).await;

    ok_json!(serde_json::json!({
        "success": true,
        "message": "Feature flags updated successfully",
        "flags": flags
    }))
}
