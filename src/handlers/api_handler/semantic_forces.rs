//! Semantic Forces API Handler
//!
//! Force PARAMETERS are not configured here: they are the central physics settings' `semantic`
//! group (`PUT /api/settings/physics`), validated and applied to the GPU by that one path. This
//! module serves the derived hierarchy, a read-only view of the stored parameters, and the
//! ontology relationship type registry.
//!
//! ## Hot-Reload Support
//!
//! The dynamic relationship endpoints enable ontology changes without CUDA recompilation:
//! - `POST /api/semantic-forces/relationship-types` - Register new relationship type
//! - `PUT /api/semantic-forces/relationship-types/:id` - Update force parameters
//! - `POST /api/semantic-forces/relationship-types/reload` - Sync registry to GPU

use actix_web::{web, HttpResponse, Responder};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::actors::messages::{GetHierarchyLevels, RecalculateHierarchy, SetRelationshipTable};
use crate::services::semantic_type_registry::{RelationshipForceConfig, SEMANTIC_TYPE_REGISTRY};
use crate::AppState;
use crate::{bad_request, error_json, ok_json};

/// Get hierarchy level assignments for all nodes
/// GET /api/semantic-forces/hierarchy-levels
pub async fn get_hierarchy_levels(state: web::Data<AppState>) -> impl Responder {
    info!("Hierarchy levels request received");

    let gpu_manager = match state.gpu_manager_addr.as_ref() {
        Some(manager) => manager,
        None => {
            error!("GPU manager not available");
            return error_json!("GPU manager not initialized");
        }
    };

    match gpu_manager.send(GetHierarchyLevels).await {
        Ok(Ok(levels)) => {
            ok_json!(json!({
                "status": "success",
                "maxLevel": levels.max_level,
                "nodeLevels": levels.node_levels,
                "levelCounts": levels.level_counts
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to get hierarchy levels: {}", e);
            error_json!(json!({
                "error": "Hierarchy level retrieval failed",
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

/// Get the stored semantic forces configuration (read-only view of `physics.semantic`).
/// GET /api/semantic-forces/config
pub async fn get_semantic_config(state: web::Data<AppState>) -> impl Responder {
    use crate::config::PhysicsSettings;
    use crate::ports::settings_repository::{SettingValue, SettingsRepository};
    let stored = state
        .sqlite_settings_repository
        .get_setting("physics")
        .await;
    let semantic = match stored {
        Ok(Some(SettingValue::Json(json))) => serde_json::from_value::<PhysicsSettings>(json)
            .map(|p| p.semantic)
            .unwrap_or_default(),
        _ => PhysicsSettings::default().semantic,
    };
    ok_json!(json!({ "status": "success", "config": semantic }))
}

/// Recalculate hierarchy levels (useful after graph structure changes)
/// POST /api/semantic-forces/hierarchy/recalculate
pub async fn recalculate_hierarchy(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
) -> impl Responder {
    info!("Hierarchy recalculation request received");

    let gpu_manager = match state.gpu_manager_addr.as_ref() {
        Some(manager) => manager,
        None => {
            error!("GPU manager not available");
            return error_json!("GPU manager not initialized");
        }
    };

    match gpu_manager.send(RecalculateHierarchy).await {
        Ok(Ok(())) => {
            ok_json!(json!({
                "status": "success",
                "message": "Hierarchy levels recalculated"
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to recalculate hierarchy: {}", e);
            error_json!(json!({
                "error": "Hierarchy recalculation failed",
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

// =============================================================================
// Dynamic Relationship Type Management (Hot-Reload)
// =============================================================================

/// Request payload for registering a new relationship type
#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterRelationshipTypeRequest {
    pub uri: String,
    pub strength: f32,
    pub rest_length: f32,
    pub is_directional: bool,
    #[serde(default)]
    pub force_type: u32, // 0=spring, 1=orbit, 2=cross-domain, 3=repulsion
}

/// Request payload for updating a relationship type's force parameters
#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateRelationshipForceRequest {
    pub strength: Option<f32>,
    pub rest_length: Option<f32>,
    pub is_directional: Option<bool>,
    pub force_type: Option<u32>,
}

/// Response for relationship type operations
#[derive(Debug, Serialize)]
pub struct RelationshipTypeResponse {
    pub id: u32,
    pub uri: String,
    pub strength: f32,
    pub rest_length: f32,
    pub is_directional: bool,
    pub force_type: u32,
}

/// Register a new relationship type (hot-reload without CUDA recompilation)
/// POST /api/semantic-forces/relationship-types
pub async fn register_relationship_type(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    payload: web::Json<RegisterRelationshipTypeRequest>,
) -> HttpResponse {
    if !payload.strength.is_finite() || payload.strength < 0.0 || payload.strength > 10.0 {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "status": "error",
            "message": "strength must be a finite number in range 0.0..=10.0"
        }));
    }
    if !payload.rest_length.is_finite() || payload.rest_length < 0.0 || payload.rest_length > 1000.0
    {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "status": "error",
            "message": "rest_length must be a finite number in range 0.0..=1000.0"
        }));
    }

    info!("Registering new relationship type: {}", payload.uri);

    let config = RelationshipForceConfig {
        strength: payload.strength,
        rest_length: payload.rest_length,
        is_directional: payload.is_directional,
        force_type: payload.force_type,
    };

    let id = SEMANTIC_TYPE_REGISTRY.register(&payload.uri, config);

    info!(
        "Registered relationship type '{}' with ID {}",
        payload.uri, id
    );

    HttpResponse::Ok().json(serde_json::json!({
        "status": "success",
        "message": "Relationship type registered",
        "type": {
            "id": id,
            "uri": payload.uri,
            "strength": config.strength,
            "rest_length": config.rest_length,
            "is_directional": config.is_directional,
            "force_type": config.force_type,
        }
    }))
}

/// Update force parameters for an existing relationship type
/// PUT /api/semantic-forces/relationship-types/{uri}
pub async fn update_relationship_type(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    path: web::Path<String>,
    payload: web::Json<UpdateRelationshipForceRequest>,
) -> HttpResponse {
    let uri = path.into_inner();
    info!("Updating relationship type: {}", uri);

    // Get existing config
    let id = match SEMANTIC_TYPE_REGISTRY.get_id(&uri) {
        Some(id) => id,
        None => {
            error!("Relationship type not found: {}", uri);
            return HttpResponse::BadRequest().json(serde_json::json!({
                "success": false,
                "error": format!("Relationship type '{}' not found", uri)
            }));
        }
    };

    let existing = match SEMANTIC_TYPE_REGISTRY.get_config(id) {
        Some(config) => config,
        None => {
            error!("Configuration not found for type: {}", uri);
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": "Configuration not found"
            }));
        }
    };

    // Merge updates
    let updated = RelationshipForceConfig {
        strength: payload.strength.unwrap_or(existing.strength),
        rest_length: payload.rest_length.unwrap_or(existing.rest_length),
        is_directional: payload.is_directional.unwrap_or(existing.is_directional),
        force_type: payload.force_type.unwrap_or(existing.force_type),
    };

    if SEMANTIC_TYPE_REGISTRY.update_config(&uri, updated) {
        info!("Updated relationship type '{}' (ID {})", uri, id);
        HttpResponse::Ok().json(serde_json::json!({
            "status": "success",
            "message": "Relationship type updated",
            "type": {
                "id": id,
                "uri": uri,
                "strength": updated.strength,
                "rest_length": updated.rest_length,
                "is_directional": updated.is_directional,
                "force_type": updated.force_type,
            }
        }))
    } else {
        HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "error": "Failed to update relationship type"
        }))
    }
}

/// Get all registered relationship types
/// GET /api/semantic-forces/relationship-types
pub async fn list_relationship_types() -> HttpResponse {
    debug!("Listing all relationship types");

    let uris = SEMANTIC_TYPE_REGISTRY.registered_uris();
    let types: Vec<serde_json::Value> = uris
        .iter()
        .filter_map(|uri| {
            let id = SEMANTIC_TYPE_REGISTRY.get_id(uri)?;
            let config = SEMANTIC_TYPE_REGISTRY.get_config(id)?;
            Some(json!({
                "id": id,
                "uri": uri,
                "strength": config.strength,
                "rest_length": config.rest_length,
                "is_directional": config.is_directional,
                "force_type": config.force_type,
            }))
        })
        .collect();

    HttpResponse::Ok().json(serde_json::json!({
        "status": "success",
        "count": types.len(),
        "types": types,
    }))
}

/// Get a specific relationship type by URI
/// GET /api/semantic-forces/relationship-types/{uri}
pub async fn get_relationship_type(path: web::Path<String>) -> HttpResponse {
    let uri = path.into_inner();
    debug!("Getting relationship type: {}", uri);

    let id = match SEMANTIC_TYPE_REGISTRY.get_id(&uri) {
        Some(id) => id,
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "success": false,
                "error": format!("Relationship type '{}' not found", uri)
            }));
        }
    };

    let config = match SEMANTIC_TYPE_REGISTRY.get_config(id) {
        Some(config) => config,
        None => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": "Configuration not found"
            }));
        }
    };

    HttpResponse::Ok().json(serde_json::json!({
        "status": "success",
        "type": {
            "id": id,
            "uri": uri,
            "strength": config.strength,
            "rest_length": config.rest_length,
            "is_directional": config.is_directional,
            "force_type": config.force_type,
        }
    }))
}

/// Trigger GPU buffer reload from registry (hot-reload)
/// POST /api/semantic-forces/relationship-types/reload
pub async fn reload_relationship_buffer(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
) -> HttpResponse {
    info!("Triggering GPU relationship table reload");

    let version = SEMANTIC_TYPE_REGISTRY.version();
    let configs = SEMANTIC_TYPE_REGISTRY.build_gpu_buffer();
    let count = configs.len();

    let Some(gpu_manager) = state.gpu_manager_addr.as_ref() else {
        return HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "error",
            "message": "GPU manager not initialized",
            "version": version,
        }));
    };
    match gpu_manager
        .send(SetRelationshipTable {
            configs,
            version: version as i32,
        })
        .await
    {
        Ok(Ok(())) => {
            info!(
                "Relationship table reloaded to GPU: {} types, version {}",
                count, version
            );
            HttpResponse::Ok().json(serde_json::json!({
                "status": "success",
                "message": "GPU relationship table reloaded",
                "count": count,
                "version": version,
            }))
        }
        Ok(Err(e)) => {
            error!("GPU relationship table upload failed: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "status": "error",
                "message": format!("GPU relationship table upload failed: {}", e),
                "version": version,
            }))
        }
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "status": "error",
            "message": format!("GPU manager unreachable: {}", e),
            "version": version,
        })),
    }
}

/// Configure routes for semantic forces API
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/semantic-forces")
            .route("/hierarchy-levels", web::get().to(get_hierarchy_levels))
            .route("/config", web::get().to(get_semantic_config))
            .route(
                "/hierarchy/recalculate",
                web::post().to(recalculate_hierarchy),
            )
            // Dynamic relationship type management (hot-reload)
            .route(
                "/relationship-types",
                web::get().to(list_relationship_types),
            )
            .route(
                "/relationship-types",
                web::post().to(register_relationship_type),
            )
            .route(
                "/relationship-types/reload",
                web::post().to(reload_relationship_buffer),
            )
            .route(
                "/relationship-types/{uri:.*}",
                web::get().to(get_relationship_type),
            )
            .route(
                "/relationship-types/{uri:.*}",
                web::put().to(update_relationship_type),
            ),
    );
}
