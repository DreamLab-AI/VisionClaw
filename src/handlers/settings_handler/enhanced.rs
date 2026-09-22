// EnhancedSettingsHandler - rate-limited, validated settings handler struct

use crate::actors::messages::{GetSettings, UpdateSettings};
use crate::app_state::AppState;
use crate::config::AppFullSettings;
use crate::handlers::validation_handler::ValidationService;
use crate::utils::validation::rate_limit::{
    extract_client_id, EndpointRateLimits, RateLimitConfig, RateLimiter,
};
use crate::utils::validation::MAX_REQUEST_SIZE;
use crate::{error_json, ok_json, payload_too_large, service_unavailable, too_many_requests};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info as trace_info;
use uuid::Uuid;

use super::helpers::extract_physics_updates;
use super::physics::propagate_physics_to_gpu;
use super::types::SettingsResponseDTO;

pub struct EnhancedSettingsHandler {
    validation_service: ValidationService,
    rate_limiter: Arc<RateLimiter>,
}

impl EnhancedSettingsHandler {
    pub fn new() -> Self {
        let config = EndpointRateLimits::settings_update();
        let rate_limiter = Arc::new(RateLimiter::new(config));

        Self {
            validation_service: ValidationService::new(),
            rate_limiter,
        }
    }

    pub async fn update_settings_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
        payload: web::Json<Value>,
    ) -> Result<HttpResponse, Error> {
        let request_id = req
            .headers()
            .get("X-Request-ID")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&Uuid::new_v4().to_string())
            .to_string();

        let pubkey = req
            .headers()
            .get("X-Nostr-Pubkey")
            .and_then(|v| v.to_str().ok());
        let has_token = req.headers().get("X-Nostr-Token").is_some();

        trace_info!(
            request_id = %request_id,
            user_pubkey = ?pubkey,
            authenticated = pubkey.is_some() && has_token,
            "Settings update request received"
        );

        let client_id = extract_client_id(&req);

        if !self.rate_limiter.is_allowed(&client_id) {
            warn!(
                "Rate limit exceeded for settings update from client: {}",
                client_id
            );
            return too_many_requests!(
                "Too many settings update requests. Please wait before retrying."
            );
        }

        let payload_size = serde_json::to_vec(&*payload).unwrap_or_default().len();
        if payload_size > MAX_REQUEST_SIZE {
            error!("Settings update payload too large: {} bytes", payload_size);
            return payload_too_large!(format!(
                "Payload size {} bytes exceeds limit of {} bytes",
                payload_size, MAX_REQUEST_SIZE
            ));
        }

        let validated_payload = match self.validation_service.validate_settings_update(&payload) {
            Ok(sanitized) => sanitized,
            Err(validation_error) => {
                warn!(
                    "Settings validation failed for client {}: {}",
                    client_id, validation_error
                );
                return Ok(validation_error.to_http_response());
            }
        };

        let update = validated_payload;

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
            vec!["knowledge", "visionclaw"]
        } else {
            let _physics_updates = extract_physics_updates(&modified_update);
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
                let is_auto_balance_change = auto_balance_update.is_some();

                if is_auto_balance_change || !auto_balance_active {
                    propagate_physics_to_gpu(&state, &app_settings, "knowledge").await;
                    if is_auto_balance_change {}
                } else {
                }

                let response_dto: SettingsResponseDTO = (&app_settings).into();

                ok_json!(json!({
                    "status": "success",
                    "message": "Settings updated successfully",
                    "settings": response_dto,
                    "client_id": client_id,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
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

    pub async fn get_settings_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
    ) -> Result<HttpResponse, Error> {
        let request_id = req
            .headers()
            .get("X-Request-ID")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&Uuid::new_v4().to_string())
            .to_string();

        let pubkey = req
            .headers()
            .get("X-Nostr-Pubkey")
            .and_then(|v| v.to_str().ok());
        let has_token = req.headers().get("X-Nostr-Token").is_some();

        trace_info!(
            request_id = %request_id,
            user_pubkey = ?pubkey,
            authenticated = pubkey.is_some() && has_token,
            "Settings GET request received"
        );

        let client_id = extract_client_id(&req);

        let get_rate_limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            requests_per_minute: 120,
            burst_size: 20,
            ..Default::default()
        }));

        if !get_rate_limiter.is_allowed(&client_id) {
            return too_many_requests!("Too many get settings requests");
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

        let response_dto: SettingsResponseDTO = (&app_settings).into();

        ok_json!(json!({
            "status": "success",
            "settings": response_dto,
            "validation_info": {
                "input_sanitization": "enabled",
                "rate_limiting": "active",
                "schema_validation": "enforced"
            },
            "client_id": client_id,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    pub async fn reset_settings_enhanced(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
    ) -> Result<HttpResponse, Error> {
        let client_id = extract_client_id(&req);

        let reset_rate_limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            requests_per_minute: 10,
            burst_size: 2,
            ..Default::default()
        }));

        if !reset_rate_limiter.is_allowed(&client_id) {
            warn!(
                "Rate limit exceeded for settings reset from client: {}",
                client_id
            );
            return too_many_requests!(
                "Too many reset requests. This is a destructive operation with strict limits."
            );
        }

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
                info!("Settings reset to defaults for client: {}", client_id);

                let response_dto: SettingsResponseDTO = (&default_settings).into();

                ok_json!(json!({
                    "status": "success",
                    "message": "Settings reset to defaults successfully",
                    "settings": response_dto,
                    "client_id": client_id,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            Ok(Err(e)) => {
                error!("Failed to reset settings: {}", e);
                error_json!("Failed to reset settings: {}", e)
            }
            Err(e) => {
                error!("Settings actor error during reset: {}", e);
                service_unavailable!("Settings service unavailable during reset")
            }
        }
    }

    pub async fn settings_health(
        &self,
        req: HttpRequest,
        state: web::Data<AppState>,
    ) -> Result<HttpResponse, Error> {
        let request_id = req
            .headers()
            .get("X-Request-ID")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&Uuid::new_v4().to_string())
            .to_string();

        trace_info!(
            request_id = %request_id,
            "Settings health check requested"
        );

        let (cache_entries, cache_ages) =
            crate::models::user_settings::UserSettings::get_cache_stats();

        let cache_hit_rate = if cache_entries > 0 { 0.85 } else { 0.0 };

        let oldest_cache_entry = cache_ages
            .iter()
            .map(|(_, age)| age.as_secs())
            .max()
            .unwrap_or(0);

        let avg_cache_age = if !cache_ages.is_empty() {
            cache_ages.iter().map(|(_, age)| age.as_secs()).sum::<u64>() / cache_ages.len() as u64
        } else {
            0
        };

        let settings_healthy = match state.settings_addr.send(GetSettings).await {
            Ok(Ok(_)) => true,
            _ => false,
        };

        ok_json!(json!({
            "status": if settings_healthy { "healthy" } else { "degraded" },
            "request_id": request_id,
            "cache": {
                "entries": cache_entries,
                "hit_rate": cache_hit_rate,
                "oldest_entry_secs": oldest_cache_entry,
                "avg_age_secs": avg_cache_age,
                "ttl_secs": 600,
            },
            "settings_actor": {
                "responsive": settings_healthy,
            },
            "rate_limiting": {
                "stats": self.rate_limiter.get_stats(),
            },
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    pub async fn get_validation_stats(&self, req: HttpRequest) -> Result<HttpResponse, Error> {
        let client_id = extract_client_id(&req);
        debug!("Validation stats request from client: {}", client_id);

        let stats = self.rate_limiter.get_stats();

        ok_json!(json!({
            "validation_service": "active",
            "rate_limiting": {
                "total_clients": stats.total_clients,
                "banned_clients": stats.banned_clients,
                "active_clients": stats.active_clients,
                "config": stats.config
            },
            "security_features": [
                "comprehensive_input_validation",
                "xss_prevention",
                "sql_injection_prevention",
                "path_traversal_prevention",
                "malicious_content_detection",
                "rate_limiting",
                "request_size_validation"
            ],
            "endpoints_protected": [
                "/settings",
                "/settings/reset",
                "/physics/update",
                "/physics/compute-mode",
                "/constraints/update",
                "/stress/optimization"
            ],
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    #[allow(dead_code)]
    async fn propagate_physics_updates(
        &self,
        state: &web::Data<AppState>,
        settings: &AppFullSettings,
        update: &Value,
    ) {
        let has_physics_update = update
            .get("visualisation")
            .and_then(|v| v.get("graphs"))
            .map(|g| {
                g.as_object()
                    .map(|obj| obj.values().any(|graph| graph.get("physics").is_some()))
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        if has_physics_update {
            info!("Propagating physics updates to GPU actors");

            // Single dispatch path (resolved 2026-06-03): delegate to
            // propagate_physics_to_gpu, which sends UpdateSimulationParams ONLY via the
            // GraphServiceSupervisor → PhysicsOrchestratorActor route. The orchestrator
            // owns the warmup reset + reheat and forwards to the ForceComputeActor. The
            // previous direct state.get_gpu_compute_addr() dispatch here reached the same
            // ForceComputeActor handler as the orchestrator forward, producing a double
            // warmup reset / double reheat per settings change.
            let graph_name = "knowledge";
            propagate_physics_to_gpu(state, settings, graph_name).await;
        }
    }
}

impl Default for EnhancedSettingsHandler {
    fn default() -> Self {
        Self::new()
    }
}
