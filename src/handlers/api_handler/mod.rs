pub mod analytics;
pub mod bots;
pub mod files;
pub mod graph;
pub mod ontology;
pub mod ontology_physics;
pub mod quest3;
// pub mod sessions;
pub mod semantic_forces;
pub mod visualisation;

// Re-export specific types and functions
// Re-export specific types and functions
pub use files::{fetch_and_process_files, get_file_content};

pub use graph::{
    get_graph_data, get_graph_positions, get_paginated_graph_data, refresh_graph, update_graph,
};

pub use visualisation::get_visualisation_settings;

use crate::handlers::utils::execute_in_thread;
use crate::{error_json, ok_json};
use actix_web::{web, HttpResponse, Responder};
use log::{error, info};
use serde_json::json;

async fn health_check() -> Result<HttpResponse, actix_web::Error> {
    info!("Health check requested");
    ok_json!(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn get_app_config(state: web::Data<crate::AppState>) -> impl Responder {
    info!("App config requested via CQRS");

    use crate::application::settings::{LoadAllSettings, LoadAllSettingsHandler};
    use hexser::QueryHandler;

    let handler = LoadAllSettingsHandler::new(state.settings_repository.clone());

    let result = execute_in_thread(move || handler.handle(LoadAllSettings)).await;

    match result {
        Ok(Ok(Some(settings))) => ok_json!(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "features": {
                "ragflow": settings.ragflow.is_some(),
                "perplexity": settings.perplexity.is_some(),
                "openai": settings.openai.is_some(),
                "kokoro": settings.kokoro.is_some(),
                "whisper": settings.whisper.is_some(),
            },
            "websocket": {
                "minUpdateRate": settings.system.websocket.min_update_rate,
                "maxUpdateRate": settings.system.websocket.max_update_rate,
                "motionThreshold": settings.system.websocket.motion_threshold,
                "motionDamping": settings.system.websocket.motion_damping,
            },
            "rendering": {
                "ambientLightIntensity": settings.visualisation.rendering.ambient_light_intensity,
                "enableAmbientOcclusion": settings.visualisation.rendering.enable_ambient_occlusion,
                "backgroundColor": settings.visualisation.rendering.background_color,
            },
            "xr": {
                "enabled": settings.xr.enabled.unwrap_or(false),
                "roomScale": settings.xr.room_scale,
                "spaceType": settings.xr.space_type,
            }
        })),
        Ok(Ok(None)) => {
            log::warn!("No settings found, using defaults");
            use crate::config::AppFullSettings;
            use crate::ok_json;

            let settings = AppFullSettings::default();
            ok_json!(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "features": {
                    "ragflow": settings.ragflow.is_some(),
                    "perplexity": settings.perplexity.is_some(),
                    "openai": settings.openai.is_some(),
                    "kokoro": settings.kokoro.is_some(),
                    "whisper": settings.whisper.is_some(),
                },
                "websocket": {
                    "minUpdateRate": settings.system.websocket.min_update_rate,
                    "maxUpdateRate": settings.system.websocket.max_update_rate,
                    "motionThreshold": settings.system.websocket.motion_threshold,
                    "motionDamping": settings.system.websocket.motion_damping,
                },
                "rendering": {
                    "ambientLightIntensity": settings.visualisation.rendering.ambient_light_intensity,
                    "enableAmbientOcclusion": settings.visualisation.rendering.enable_ambient_occlusion,
                    "backgroundColor": settings.visualisation.rendering.background_color,
                },
                "xr": {
                    "enabled": settings.xr.enabled.unwrap_or(false),
                    "roomScale": settings.xr.room_scale,
                    "spaceType": settings.xr.space_type,
                }
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to load settings via CQRS: {}", e);
            error_json!("Failed to retrieve configuration")
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

// Configure all API routes
// NOTE: Do NOT wrap in web::scope("") — it acts as a catch-all that shadows
// every .configure() registered after api_handler::config on the parent scope.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health_check))
        .route("/config", web::get().to(get_app_config))
        .configure(files::config)
        .configure(graph::config)
        .configure(crate::handlers::graph_state_handler::config)
        .configure(crate::handlers::ontology_handler::config)
        .configure(bots::config)
        .configure(analytics::config)
        .configure(quest3::config)
        .configure(crate::handlers::nostr_handler::config)
        // OLD settings_handler disabled - using new SettingsActor routes from webxr::settings::api
        // .configure(crate::handlers::settings_handler::config)
        .configure(crate::handlers::ragflow_handler::config)
        .configure(crate::handlers::clustering_handler::config)
        .configure(crate::handlers::constraints_handler::config)
        // Ontology routes (previously orphaned outside scope)
        .configure(ontology::config)
        // Ontology-physics integration routes (P0-2)
        .configure(ontology_physics::configure_routes)
        // Semantic forces routes (GPU feature)
        .configure(semantic_forces::config)
        ;
}
