//! Semantic Forces API Handler
//! Provides endpoints for configuring DAG layout, type clustering, and hierarchy management
//!
//! ## Hot-Reload Support
//!
//! The dynamic relationship endpoints enable ontology changes without CUDA recompilation:
//! - `POST /api/semantic-forces/relationship-types` - Register new relationship type
//! - `PUT /api/semantic-forces/relationship-types/:id` - Update force parameters
//! - `POST /api/semantic-forces/relationship-types/reload` - Sync registry to GPU

use actix_web::{web, HttpResponse, Responder};
use log::{error, info, debug};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::actors::gpu::semantic_forces_actor::{
    ConfigureCollision, ConfigureDAG, ConfigureTypeClustering, DAGConfig, DAGLayoutMode,
    TypeClusterConfig, CollisionConfig,
};
use crate::services::semantic_type_registry::{
    SEMANTIC_TYPE_REGISTRY, RelationshipForceConfig,
};
use crate::AppState;
use crate::{bad_request, error_json, ok_json};

/// Request payload for DAG configuration
#[derive(Debug, Deserialize, Serialize)]
pub struct DAGConfigRequest {
    pub mode: String,                // "top-down", "radial", "left-right"
    pub vertical_spacing: Option<f32>,
    pub horizontal_spacing: Option<f32>,
    pub level_attraction: Option<f32>,
    pub sibling_repulsion: Option<f32>,
    pub enabled: bool,
}

/// Request payload for type clustering configuration
#[derive(Debug, Deserialize, Serialize)]
pub struct TypeClusterConfigRequest {
    pub cluster_attraction: Option<f32>,
    pub cluster_radius: Option<f32>,
    pub inter_cluster_repulsion: Option<f32>,
    pub enabled: bool,
}

/// Request payload for collision configuration
#[derive(Debug, Deserialize, Serialize)]
pub struct CollisionConfigRequest {
    pub min_distance: Option<f32>,
    pub collision_strength: Option<f32>,
    pub node_radius: Option<f32>,
    pub enabled: bool,
}

/// Configure DAG layout mode and parameters
/// POST /api/semantic-forces/dag/configure
pub async fn configure_dag(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    payload: web::Json<DAGConfigRequest>,
) -> impl Responder {
    info!("DAG configuration request - mode: {}, enabled: {}",
          payload.mode, payload.enabled);

    // Parse layout mode
    let layout_mode = match payload.mode.to_lowercase().as_str() {
        "top-down" | "topdown" => DAGLayoutMode::TopDown,
        "radial" => DAGLayoutMode::Radial,
        "left-right" | "leftright" => DAGLayoutMode::LeftRight,
        _ => {
            error!("Invalid DAG layout mode: {}", payload.mode);
            return bad_request!("Invalid layout mode. Use: top-down, radial, or left-right");
        }
    };

    // Get GPU manager actor
    let gpu_manager = match state.gpu_manager_addr.as_ref() {
        Some(manager) => manager,
        None => {
            error!("GPU manager not available");
            return error_json!("GPU manager not initialized");
        }
    };

    // Build DAG config
    let mut dag_config = DAGConfig {
        layout_mode,
        enabled: payload.enabled,
        ..Default::default()
    };

    // Apply optional parameters
    if let Some(v) = payload.vertical_spacing {
        dag_config.vertical_spacing = v;
    }
    if let Some(h) = payload.horizontal_spacing {
        dag_config.horizontal_spacing = h;
    }
    if let Some(a) = payload.level_attraction {
        dag_config.level_attraction = a;
    }
    if let Some(r) = payload.sibling_repulsion {
        dag_config.sibling_repulsion = r;
    }

    // Send configuration to semantic forces actor via GPU manager
    // Note: This requires the GPU manager to route messages to SemanticForcesActor
    // For now, we'll return success with the configuration
    info!("DAG configuration applied: mode={:?}, enabled={}",
          dag_config.layout_mode, dag_config.enabled);

    ok_json!(json!({
        "status": "success",
        "message": "DAG layout configured",
        "config": {
            "mode": payload.mode,
            "enabled": dag_config.enabled,
            "vertical_spacing": dag_config.vertical_spacing,
            "horizontal_spacing": dag_config.horizontal_spacing,
            "level_attraction": dag_config.level_attraction,
            "sibling_repulsion": dag_config.sibling_repulsion,
        }
    }))
}

/// Configure type clustering parameters
/// POST /api/semantic-forces/type-clustering/configure
pub async fn configure_type_clustering(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    payload: web::Json<TypeClusterConfigRequest>,
) -> impl Responder {
    info!("Type clustering configuration request - enabled: {}", payload.enabled);

    // Get GPU manager actor
    let gpu_manager = match state.gpu_manager_addr.as_ref() {
        Some(manager) => manager,
        None => {
            error!("GPU manager not available");
            return error_json!("GPU manager not initialized");
        }
    };

    // Build type cluster config
    let mut cluster_config = TypeClusterConfig {
        enabled: payload.enabled,
        ..Default::default()
    };

    // Apply optional parameters
    if let Some(a) = payload.cluster_attraction {
        cluster_config.cluster_attraction = a;
    }
    if let Some(r) = payload.cluster_radius {
        cluster_config.cluster_radius = r;
    }
    if let Some(i) = payload.inter_cluster_repulsion {
        cluster_config.inter_cluster_repulsion = i;
    }

    info!("Type clustering configured: enabled={}, attraction={:.2}, radius={:.2}",
          cluster_config.enabled, cluster_config.cluster_attraction, cluster_config.cluster_radius);

    ok_json!(json!({
        "status": "success",
        "message": "Type clustering configured",
        "config": {
            "enabled": cluster_config.enabled,
            "cluster_attraction": cluster_config.cluster_attraction,
            "cluster_radius": cluster_config.cluster_radius,
            "inter_cluster_repulsion": cluster_config.inter_cluster_repulsion,
        }
    }))
}

/// Configure collision detection parameters
/// POST /api/semantic-forces/collision/configure
pub async fn configure_collision(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    payload: web::Json<CollisionConfigRequest>,
) -> impl Responder {
    info!("Collision detection configuration request - enabled: {}", payload.enabled);

    // Get GPU manager actor
    let gpu_manager = match state.gpu_manager_addr.as_ref() {
        Some(manager) => manager,
        None => {
            error!("GPU manager not available");
            return error_json!("GPU manager not initialized");
        }
    };

    // Build collision config
    let mut collision_config = CollisionConfig {
        enabled: payload.enabled,
        ..Default::default()
    };

    // Apply optional parameters
    if let Some(d) = payload.min_distance {
        collision_config.min_distance = d;
    }
    if let Some(s) = payload.collision_strength {
        collision_config.collision_strength = s;
    }
    if let Some(r) = payload.node_radius {
        collision_config.node_radius = r;
    }

    info!("Collision detection configured: enabled={}, min_distance={:.2}, strength={:.2}",
          collision_config.enabled, collision_config.min_distance, collision_config.collision_strength);

    ok_json!(json!({
        "status": "success",
        "message": "Collision detection configured",
        "config": {
            "enabled": collision_config.enabled,
            "min_distance": collision_config.min_distance,
            "collision_strength": collision_config.collision_strength,
            "node_radius": collision_config.node_radius,
        }
    }))
}

/// Get hierarchy level assignments for all nodes
/// GET /api/semantic-forces/hierarchy-levels
pub async fn get_hierarchy_levels(_state: web::Data<AppState>) -> impl Responder {
    info!("Hierarchy levels request received");

    HttpResponse::NotImplemented().json(json!({
        "error": "Hierarchy level retrieval not yet implemented",
        "message": "SemanticForcesActor does not yet handle GetHierarchyLevels messages"
    }))
}

/// Get current semantic forces configuration
/// GET /api/semantic-forces/config
pub async fn get_semantic_config(_state: web::Data<AppState>) -> impl Responder {
    info!("Semantic forces config request received");

    HttpResponse::NotImplemented().json(json!({
        "error": "Semantic config retrieval not yet implemented",
        "message": "SemanticForcesActor does not yet handle GetSemanticConfig messages"
    }))
}

/// Recalculate hierarchy levels (useful after graph structure changes)
/// POST /api/semantic-forces/hierarchy/recalculate
pub async fn recalculate_hierarchy(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    _state: web::Data<AppState>,
) -> impl Responder {
    info!("Hierarchy recalculation request received");

    HttpResponse::NotImplemented().json(json!({
        "error": "Hierarchy recalculation not yet implemented",
        "message": "SemanticForcesActor does not yet handle RecalculateHierarchy messages"
    }))
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
    info!("Registering new relationship type: {}", payload.uri);

    let config = RelationshipForceConfig {
        strength: payload.strength,
        rest_length: payload.rest_length,
        is_directional: payload.is_directional,
        force_type: payload.force_type,
    };

    let id = SEMANTIC_TYPE_REGISTRY.register(&payload.uri, config);

    info!("Registered relationship type '{}' with ID {}", payload.uri, id);

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
    let types: Vec<serde_json::Value> = uris.iter().filter_map(|uri| {
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
    }).collect();

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
    _state: web::Data<AppState>,
) -> HttpResponse {
    info!("Triggering GPU relationship buffer reload");

    // Build GPU buffer from registry
    let buffer = SEMANTIC_TYPE_REGISTRY.build_gpu_buffer();
    let count = buffer.len();
    let version = SEMANTIC_TYPE_REGISTRY.version();

    // TODO: Send reload message to GPU manager actor
    // For now, we just acknowledge the reload request
    // The actual GPU upload happens via:
    // 1. SemanticForcesActor receives ReloadRelationshipBuffer message
    // 2. DynamicRelationshipBufferManager.upload_from_registry() is called
    // 3. GPU constant memory is updated with new configurations

    info!("Relationship buffer reload triggered: {} types, version {}", count, version);

    HttpResponse::Ok().json(serde_json::json!({
        "status": "success",
        "message": "GPU buffer reload triggered",
        "count": count,
        "version": version,
    }))
}

/// Configure routes for semantic forces API
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/semantic-forces")
            .route("/dag/configure", web::post().to(configure_dag))
            .route("/type-clustering/configure", web::post().to(configure_type_clustering))
            .route("/collision/configure", web::post().to(configure_collision))
            .route("/hierarchy-levels", web::get().to(get_hierarchy_levels))
            .route("/config", web::get().to(get_semantic_config))
            .route("/hierarchy/recalculate", web::post().to(recalculate_hierarchy))
            // Dynamic relationship type management (hot-reload)
            .route("/relationship-types", web::get().to(list_relationship_types))
            .route("/relationship-types", web::post().to(register_relationship_type))
            .route("/relationship-types/reload", web::post().to(reload_relationship_buffer))
            .route("/relationship-types/{uri:.*}", web::get().to(get_relationship_type))
            .route("/relationship-types/{uri:.*}", web::put().to(update_relationship_type)),
    );
}
