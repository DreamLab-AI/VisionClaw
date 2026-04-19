use actix_web::web;
use log::debug;

use crate::middleware::RequireAuth;
use crate::ok_json;

// Existing submodules
mod real_gpu_functions;

// WebSocket integration module
pub mod websocket_integration;

// Community detection module
pub mod community;

// Pathfinding module (SSSP, APSP, Connected Components)
pub mod pathfinding;

// PageRank centrality module
pub mod pagerank_handlers;

// New submodules from split
pub mod types;
pub mod state;
mod params_handlers;
mod performance_handlers;
mod clustering_handlers;
mod anomaly_handlers;
mod insights_handlers;
mod sssp_handlers;
mod stress_handlers;
mod feature_flags_handlers;

// Clustering handler (separate file, already existed)
pub mod clustering;
pub mod anomaly;

// Re-export all public types for backwards compatibility
pub use types::*;

// Re-export global state for backwards compatibility (used by websocket_integration and ontology)
pub use state::{CLUSTERING_TASKS, ANOMALY_STATE, FEATURE_FLAGS};

// Re-export handler functions for backwards compatibility
pub use params_handlers::{
    get_analytics_params, update_analytics_params, get_constraints, update_constraints,
    set_focus, set_kernel_mode,
};
pub use performance_handlers::{
    get_performance_stats, get_gpu_metrics, get_gpu_status, get_gpu_features,
};
pub use clustering_handlers::{
    run_clustering, get_clustering_status, focus_cluster, cancel_clustering,
    run_dbscan_clustering,
};
pub use anomaly_handlers::{
    toggle_anomaly_detection, get_current_anomalies, get_anomaly_config,
};
pub use insights_handlers::{
    get_ai_insights, get_realtime_insights, get_dashboard_status, get_health_check,
};
pub use sssp_handlers::{
    toggle_sssp, get_sssp_status, update_sssp_params, get_sssp_params, compute_sssp,
};
pub use stress_handlers::{
    trigger_stress_majorization, get_stress_majorization_stats,
    reset_stress_majorization_safety, update_stress_majorization_params,
    configure_stress_majorization, get_stress_majorization_config,
};
pub use feature_flags_handlers::{get_feature_flags, update_feature_flags};

pub async fn run_community_detection(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<crate::AppState>,
    request: web::Json<community::CommunityDetectionRequest>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    debug!("Community detection request: {:?}", request);

    match community::run_gpu_community_detection(&app_state, &request).await {
        Ok(response) => ok_json!(response),
        Err(e) => {
            log::error!("Community detection failed: {}", e);
            Ok(actix_web::HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": e,
                "communities": [],
                "total_communities": 0,
                "modularity": 0.0
            })))
        }
    }
}

pub async fn get_community_statistics(
    _app_state: web::Data<crate::AppState>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {

    ok_json!(serde_json::json!({
        "success": true,
        "message": "Use /community/detect to run community detection first",
        "available_algorithms": ["label_propagation"],
        "performance_hints": {
            "label_propagation": "Fast, good for large networks",
            "recommended_max_iterations": 100,
            "typical_convergence": "5-20 iterations"
        }
    }))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    // Analytics splits into two auth bands to re-enable the protections that
    // the "auth temporarily disabled for testing" comment had removed:
    //
    //   - Reads (GETs): `RequireAuth::optional()` so anonymous callers see
    //     public-scoped results while signed callers receive the full view.
    //     Handlers that traverse ownership-scoped data must inspect
    //     `get_authenticated_user` to tailor output.
    //   - Writes (POSTs that mutate analytics config, kernel mode, anomaly
    //     toggles, clustering runs, feature flags, etc.) +
    //     idempotent-but-expensive POSTs (sssp/compute, pagerank/compute,
    //     pathfinding/*): `RequireAuth::authenticated()`.
    //   - WebSocket: `authenticated()` as before.
    //
    // actix-web dispatches to the first matching scope; reads scope is
    // declared first with a disjoint route list to avoid overlap.
    cfg.service(
        web::scope("/analytics")
            .wrap(RequireAuth::optional())
            .route("/params", web::get().to(get_analytics_params))
            .route("/constraints", web::get().to(get_constraints))
            .route("/stats", web::get().to(get_performance_stats))
            .route("/gpu-metrics", web::get().to(get_gpu_metrics))
            .route("/gpu-status", web::get().to(get_gpu_status))
            .route("/gpu-features", web::get().to(get_gpu_features))
            .route("/clustering/status", web::get().to(get_clustering_status))
            .route(
                "/community/statistics",
                web::get().to(get_community_statistics),
            )
            .route("/anomaly/current", web::get().to(get_current_anomalies))
            .route("/anomaly/config", web::get().to(get_anomaly_config))
            .route("/insights", web::get().to(get_ai_insights))
            .route("/insights/realtime", web::get().to(get_realtime_insights))
            .route("/sssp/params", web::get().to(get_sssp_params))
            .route("/sssp/status", web::get().to(get_sssp_status))
            .route(
                "/stress-majorization/stats",
                web::get().to(get_stress_majorization_stats),
            )
            .route(
                "/stress-majorization/config",
                web::get().to(get_stress_majorization_config),
            )
            .route("/dashboard-status", web::get().to(get_dashboard_status))
            .route("/health-check", web::get().to(get_health_check))
            .route("/feature-flags", web::get().to(get_feature_flags))
            .route(
                "/pagerank/result",
                web::get().to(pagerank_handlers::get_pagerank_result),
            ),
    )
    .service(
        web::scope("/analytics")
            .wrap(RequireAuth::authenticated())
            .route("/params", web::post().to(update_analytics_params))
            .route("/constraints", web::post().to(update_constraints))
            .route("/focus", web::post().to(set_focus))
            .route("/kernel-mode", web::post().to(set_kernel_mode))
            .route("/clustering/run", web::post().to(run_clustering))
            .route("/clustering/focus", web::post().to(focus_cluster))
            .route("/clustering/cancel", web::post().to(cancel_clustering))
            .route("/clustering/dbscan", web::post().to(run_dbscan_clustering))
            .route("/community/detect", web::post().to(run_community_detection))
            .route("/anomaly/toggle", web::post().to(toggle_anomaly_detection))
            .route("/sssp/params", web::post().to(update_sssp_params))
            .route("/sssp/compute", web::post().to(compute_sssp))
            .route("/sssp/toggle", web::post().to(toggle_sssp))
            .route(
                "/stress-majorization/trigger",
                web::post().to(trigger_stress_majorization),
            )
            .route(
                "/stress-majorization/reset-safety",
                web::post().to(reset_stress_majorization_safety),
            )
            .route(
                "/stress-majorization/params",
                web::post().to(update_stress_majorization_params),
            )
            .route(
                "/stress-majorization/configure",
                web::post().to(configure_stress_majorization),
            )
            .route("/feature-flags", web::post().to(update_feature_flags))
            .route(
                "/pagerank/compute",
                web::post().to(pagerank_handlers::compute_pagerank),
            )
            .route(
                "/pagerank/clear",
                web::post().to(pagerank_handlers::clear_pagerank_cache),
            )
            .route("/pathfinding/sssp", web::post().to(pathfinding::compute_sssp))
            .route("/pathfinding/apsp", web::post().to(pathfinding::compute_apsp))
            .route(
                "/pathfinding/connected-components",
                web::post().to(pathfinding::compute_connected_components),
            )
            .service(
                web::resource("/ws")
                    .route(web::get().to(websocket_integration::gpu_analytics_websocket)),
            ),
    );
}
