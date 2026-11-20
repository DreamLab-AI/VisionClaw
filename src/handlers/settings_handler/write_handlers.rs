// Write/mutation route handlers: update_settings, save_settings, batch operations

use crate::actors::messages::{GetSettings, UpdateSettings};
use crate::app_state::AppState;
use crate::config::path_access::JsonPathAccessible;
use crate::config::AppFullSettings;
use crate::{bad_request, error_json, ok_json, service_unavailable};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use log::{debug, error, info, warn};
use serde_json::{json, Value};

use crate::handlers::settings_validation_fix::convert_to_snake_case_recursive;
use crate::settings::auth_extractor::AuthenticatedUser;

use super::helpers::extract_physics_updates;
use super::physics::propagate_physics_to_gpu;
use super::types::{value_type_name, SettingsResponseDTO};
use super::validation::validate_settings_update;

pub async fn update_settings(
    _req: HttpRequest,
    _user: AuthenticatedUser,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let mut update = payload.into_inner();

    convert_to_snake_case_recursive(&mut update);

    debug!("Settings update received: {:?}", update);

    if let Err(e) = validate_settings_update(&update) {
        error!("Settings validation failed: {}", e);
        error!(
            "Failed update payload: {}",
            serde_json::to_string_pretty(&update).unwrap_or_default()
        );
        return bad_request!("Invalid settings: {}", e);
    }

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

    if crate::utils::logging::is_debug_enabled() {
        debug!(
            "Settings update payload (before merge): {}",
            serde_json::to_string_pretty(&update)
                .unwrap_or_else(|_| "Could not serialize".to_string())
        );
    }

    let mut modified_update = update.clone();
    let auto_balance_update = update
        .get("visualisation")
        .and_then(|v| v.get("graphs"))
        .and_then(|g| {
            if let Some(knowledge) = crate::config::knowledge_graph_value(g) {
                if let Some(physics) = knowledge.get("physics") {
                    if let Some(auto_balance) = physics.get("autoBalance") {
                        return Some(auto_balance.clone());
                    }
                }
            }

            if let Some(visionclaw) = g.get("visionclaw") {
                if let Some(physics) = visionclaw.get("physics") {
                    if let Some(auto_balance) = physics.get("autoBalance") {
                        return Some(auto_balance.clone());
                    }
                }
            }
            None
        });

    if let Some(ref auto_balance_value) = auto_balance_update {
        info!(
            "Synchronizing auto_balance setting across both graphs: {}",
            auto_balance_value
        );

        let vis_obj = modified_update
            .as_object_mut()
            .and_then(|o| {
                o.entry("visualisation")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
            })
            .and_then(|v| {
                v.entry("graphs")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
            });

        if let Some(graphs) = vis_obj {
            let knowledge_physics = graphs
                .entry("knowledge")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .and_then(|l| {
                    l.entry("physics")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                });
            if let Some(physics) = knowledge_physics {
                physics.insert("autoBalance".to_string(), auto_balance_value.clone());
            }

            let visionclaw_physics = graphs
                .entry("visionclaw")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .and_then(|v| {
                    v.entry("physics")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                });
            if let Some(physics) = visionclaw_physics {
                physics.insert("autoBalance".to_string(), auto_balance_value.clone());
            }
        }
    }

    if let Err(e) = app_settings.merge_update(modified_update.clone()) {
        error!("Failed to merge settings: {}", e);
        if crate::utils::logging::is_debug_enabled() {
            error!(
                "Update payload that caused error: {}",
                serde_json::to_string_pretty(&modified_update)
                    .unwrap_or_else(|_| "Could not serialize".to_string())
            );
        }
        return error_json!("Failed to merge settings: {}", e);
    }

    let _updated_graphs = if auto_balance_update.is_some() {
        let _physics_updates = extract_physics_updates(&update);
        vec!["knowledge", "visionclaw"]
    } else {
        modified_update
            .get("visualisation")
            .and_then(|v| v.get("graphs"))
            .and_then(|g| g.as_object())
            .map(|graphs| {
                let mut updated = Vec::new();
                if crate::config::graphs_map_has_knowledge(graphs) {
                    updated.push("knowledge");
                }
                if graphs.contains_key("visionclaw") {
                    updated.push("visionclaw");
                }
                updated
            })
            .unwrap_or_default()
    };

    let auto_balance_active = app_settings
        .visualisation
        .graphs
        .knowledge
        .physics
        .auto_balance
        || app_settings
            .visualisation
            .graphs
            .visionclaw
            .physics
            .auto_balance;

    match state
        .settings_addr
        .send(UpdateSettings {
            settings: app_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Settings updated successfully");

            let is_auto_balance_change = auto_balance_update.is_some();

            if is_auto_balance_change || !auto_balance_active {
                propagate_physics_to_gpu(&state, &app_settings, "knowledge").await;
                if is_auto_balance_change {
                    info!("[AUTO-BALANCE] Propagating auto_balance setting change to GPU (knowledge graph only)");
                }
            } else {
                info!("[AUTO-BALANCE] Skipping physics propagation to GPU - auto-balance is active and not changing");
            }

            let response_dto: SettingsResponseDTO = (&app_settings).into();

            ok_json!(response_dto)
        }
        Ok(Err(e)) => {
            error!("Failed to save settings: {}", e);
            error_json!("Failed to save settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

pub async fn reset_settings(
    _req: HttpRequest,
    _user: AuthenticatedUser,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let default_settings = match AppFullSettings::new() {
        Ok(settings) => settings,
        Err(e) => {
            error!("Failed to load default settings: {}", e);
            return error_json!("Failed to load default settings");
        }
    };

    match state
        .settings_addr
        .send(UpdateSettings {
            settings: default_settings.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!("Settings reset to defaults");

            let response_dto: SettingsResponseDTO = (&default_settings).into();

            ok_json!(response_dto)
        }
        Ok(Err(e)) => {
            error!("Failed to reset settings: {}", e);
            error_json!("Failed to reset settings: {}", e)
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            service_unavailable!("Settings service unavailable")
        }
    }
}

pub async fn save_settings(
    _req: HttpRequest,
    _user: AuthenticatedUser,
    state: web::Data<AppState>,
    payload: Option<web::Json<Value>>,
) -> Result<HttpResponse, Error> {
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

    if let Some(update) = payload {
        let update_value = update.into_inner();

        if let Err(e) = validate_settings_update(&update_value) {
            error!("Settings validation failed: {}", e);
            return bad_request!("Invalid settings: {}", e);
        }

        if let Err(e) = app_settings.merge_update(update_value) {
            error!("Failed to merge settings update: {}", e);
            return bad_request!("Failed to merge settings: {}", e);
        }
    }

    if !app_settings.system.persist_settings {
        return bad_request!(
            "Settings persistence is disabled. Enable 'system.persist_settings' to save settings."
        );
    }

    match app_settings.save() {
        Ok(()) => {
            info!("Settings successfully saved to file");

            match state
                .settings_addr
                .send(UpdateSettings {
                    settings: app_settings.clone(),
                })
                .await
            {
                Ok(Ok(())) => {
                    let response_dto: SettingsResponseDTO = (&app_settings).into();
                    ok_json!(json!({
                        "message": "Settings saved successfully",
                        "settings": response_dto
                    }))
                }
                Ok(Err(e)) => {
                    error!("Failed to update settings in actor after save: {}", e);
                    error_json!("Settings saved to file but failed to update in memory")
                }
                Err(e) => {
                    error!("Settings actor communication error: {}", e);
                    service_unavailable!("Settings saved to file but service is unavailable")
                }
            }
        }
        Err(e) => {
            error!("Failed to save settings to file: {}", e);
            error_json!("Failed to save settings to file")
        }
    }
}

pub async fn batch_get_settings(
    _auth: AuthenticatedUser,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    let paths = payload
        .get("paths")
        .and_then(|p| p.as_array())
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing 'paths' array"))?
        .iter()
        .map(|p| p.as_str().unwrap_or("").to_string())
        .collect::<Vec<String>>();

    if paths.is_empty() {
        return bad_request!("Paths array cannot be empty");
    }

    let app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    let results: Vec<Value> = paths
        .iter()
        .map(|path| match app_settings.get_json_by_path(path) {
            Ok(value_json) => {
                json!({
                    "path": path,
                    "value": value_json,
                    "success": true
                })
            }
            Err(e) => {
                warn!("Path not found '{}': {}", path, e);
                json!({
                    "path": path,
                    "success": false,
                    "error": "Path not found",
                    "message": e
                })
            }
        })
        .collect();

    ok_json!(json!({
        "success": true,
        "message": format!("Successfully processed {} paths", results.len()),
        "values": results
    }))
}

pub async fn batch_update_settings(
    _user: AuthenticatedUser,
    state: web::Data<AppState>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, Error> {
    info!("Batch update request received: {:?}", payload);

    let updates = payload
        .get("updates")
        .and_then(|u| u.as_array())
        .ok_or_else(|| {
            error!(
                "Batch update failed: Missing 'updates' array in payload: {:?}",
                payload
            );
            actix_web::error::ErrorBadRequest("Missing 'updates' array")
        })?;

    if updates.is_empty() {
        error!("Batch update failed: Empty updates array");
        return bad_request!("Updates array cannot be empty");
    }

    info!("Processing {} batch updates", updates.len());

    let mut app_settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(settings)) => settings,
        Ok(Err(e)) => {
            error!("Failed to get settings: {}", e);
            return error_json!("Failed to retrieve settings");
        }
        Err(e) => {
            error!("Settings actor error: {}", e);
            return service_unavailable!("Settings service unavailable");
        }
    };

    let mut results = Vec::new();
    let mut success_count = 0;

    for update in updates {
        let path = update.get("path").and_then(|p| p.as_str()).unwrap_or("");
        let value = update.get("value").unwrap_or(&Value::Null).clone();

        info!(
            "Processing batch update: path='{}', value={:?}",
            path, value
        );

        let previous_value = app_settings.get_json_by_path(path).ok();

        match app_settings.set_json_by_path(path, value.clone()) {
            Ok(()) => {
                success_count += 1;
                info!(
                    "Successfully updated path '{}' with value {:?}",
                    path, value
                );
                results.push(json!({
                    "path": path,
                    "success": true,
                    "value": update.get("value").cloned().unwrap_or(Value::Null),
                    "previousValue": previous_value
                }));
            }
            Err(e) => {
                error!(
                    "Failed to update path '{}' with value {:?}: {}",
                    path, value, e
                );

                let error_detail = if e.contains("does not exist") {
                    format!("Path '{}' does not exist in settings structure", path)
                } else if e.contains("Type mismatch") {
                    format!("Type mismatch: {}", e)
                } else if e.contains("not found") {
                    format!("Field not found: {}", e)
                } else {
                    e.clone()
                };

                results.push(json!({
                    "path": path,
                    "success": false,
                    "error": error_detail,
                    "message": e,
                    "providedValue": value,
                    "expectedType": previous_value.as_ref().map(|v| value_type_name(v))
                }));
            }
        }
    }

    if success_count > 0 {
        match state
            .settings_addr
            .send(UpdateSettings {
                settings: app_settings.clone(),
            })
            .await
        {
            Ok(Ok(())) => {
                info!("Batch updated {} settings successfully", success_count);

                let mut physics_updated = false;
                for update in updates {
                    let path = update.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    if path.contains(".physics.")
                        || crate::config::path_targets_knowledge_graph(path)
                        || path.contains(".graphs.visionclaw.")
                    {
                        physics_updated = true;
                        break;
                    }
                }

                if physics_updated {
                    info!("Physics settings changed in batch update, propagating to GPU actors");

                    propagate_physics_to_gpu(&state, &app_settings, "knowledge").await;
                }
            }
            Ok(Err(e)) => {
                error!("Failed to save batch settings: {}", e);
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "error": format!("Failed to save settings: {}", e),
                    "results": results
                })));
            }
            Err(e) => {
                error!("Settings actor error: {}", e);
                return service_unavailable!("Settings service unavailable");
            }
        }
    }

    ok_json!(json!({
        "success": true,
        "message": format!("Successfully updated {} out of {} settings", success_count, updates.len()),
        "results": results
    }))
}
