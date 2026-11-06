use actix_web::web;
use log::debug;

use crate::middleware::{RateLimit, RequireAuth};
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
mod anomaly_handlers;
mod clustering_handlers;
mod cycles_handlers;
mod feature_flags_handlers;
mod insights_handlers;
mod params_handlers;
mod performance_handlers;
mod sssp_handlers;
pub mod state;
mod stress_handlers;
pub mod types;
mod visible_analytics;
mod visible_results;

// Clustering handler (separate file, already existed)
pub mod anomaly;
pub mod clustering;

// Re-export all public types for backwards compatibility
pub use types::*;

// Re-export global state for backwards compatibility (used by websocket_integration and ontology)
pub use state::{ANOMALY_STATE, CLUSTERING_TASKS, FEATURE_FLAGS};

// Re-export handler functions for backwards compatibility
pub use anomaly_handlers::{get_anomaly_config, get_current_anomalies, toggle_anomaly_detection};
pub use clustering_handlers::{
    cancel_clustering, focus_cluster, get_clustering_status, run_clustering, run_dbscan_clustering,
};
pub use feature_flags_handlers::{get_feature_flags, update_feature_flags};
pub use insights_handlers::{
    get_ai_insights, get_dashboard_status, get_health_check, get_realtime_insights,
};
pub use params_handlers::{
    get_analytics_params, get_constraints, set_focus, set_kernel_mode, update_analytics_params,
    update_constraints,
};
pub use performance_handlers::{
    get_gpu_features, get_gpu_metrics, get_gpu_status, get_performance_stats,
};
pub use sssp_handlers::{compute_sssp, get_sssp_status, toggle_sssp};
pub use stress_handlers::{
    configure_stress_majorization, get_stress_majorization_config, get_stress_majorization_stats,
    reset_stress_majorization_safety, trigger_stress_majorization,
    update_stress_majorization_params,
};

pub async fn run_community_detection(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<crate::AppState>,
    request: web::Json<community::CommunityDetectionRequest>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    debug!("Community detection request: {:?}", request);

    let gate = crate::handlers::graph_access::caller_gate(
        &app_state.graph_service_addr,
        Some(auth.pubkey.as_str()),
    )
    .await;
    let result = match gate {
        Ok(gate) => community::run_gpu_community_detection(&app_state, &request)
            .await
            .map(|mut response| {
                if let Some((_, visible)) = gate {
                    response.communities =
                        visible_results::gate_communities(response.communities, &visible);
                    response.total_communities = response.communities.len();
                }
                response
            }),
        Err(e) => Err(e),
    };
    match result {
        Ok(response) => ok_json!(response),
        Err(e) => {
            log::error!("Community detection failed: {}", e);
            Ok(
                actix_web::HttpResponse::InternalServerError().json(serde_json::json!({
                    "success": false,
                    "error": e,
                    "communities": [],
                    "total_communities": 0,
                    "modularity": 0.0
                })),
            )
        }
    }
}

pub async fn run_anomaly_detection(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<crate::AppState>,
    request: web::Json<anomaly::AnomalyDetectRequest>,
) -> Result<actix_web::HttpResponse, actix_web::Error> {
    debug!("Structural anomaly detection request: {:?}", request);

    let gate = crate::handlers::graph_access::caller_gate(
        &app_state.graph_service_addr,
        Some(auth.pubkey.as_str()),
    )
    .await;
    let result = match gate {
        Ok(gate) => anomaly::run_gpu_anomaly_detection(
            &app_state,
            &request.method,
            request.k_neighbors,
            request.radius,
            request.feature_data.clone(),
            request.threshold,
        )
        .await
        .map(|anomalies| match gate {
            Some((_, visible)) => visible_results::gate_anomalies(anomalies, &visible),
            None => anomalies,
        }),
        Err(e) => Err(e),
    };
    match result {
        Ok(anomalies) => {
            let total = anomalies.len();
            ok_json!(serde_json::json!({
                "success": true,
                "anomalies": anomalies,
                "total": total,
                "method": request.method,
            }))
        }
        Err(e) => {
            log::error!("Structural anomaly detection failed: {}", e);
            Ok(
                actix_web::HttpResponse::InternalServerError().json(serde_json::json!({
                    "success": false,
                    "error": e,
                    "anomalies": [],
                    "total": 0
                })),
            )
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
    // S2 (re-enabling disabled auth): the previously fully-public `/analytics`
    // scope mixes harmless read-only metrics (GET) with sensitive control/mutation
    // operations (POST) that reconfigure GPU/physics state and trigger expensive
    // compute jobs. A blanket `authenticated()` wrap would lock out the public read
    // metrics — the client interceptor only attaches a token once the user is
    // logged in, so an anonymous browser sends no Authorization header at all and
    // those GETs would start 401-ing. We therefore split the scope:
    //   * Public, rate-limited GET metrics (status/stats/config readbacks) stay
    //     open: they expose no secrets, only operational telemetry the read-only
    //     dashboard already consumes anonymously.
    //   * Every state-mutating / compute-triggering POST now requires
    //     authentication. Routine analytics tuning is left at `authenticated()`
    //     (not power_user) so legitimate logged-in operator flows keep working.
    // Single `/analytics` scope: actix-web does NOT fall through duplicate-prefix
    // scopes. `mutations_only()` gates every state-mutating / compute-triggering
    // POST at `authenticated()` while leaving the public read-only metric GETs
    // open (the anonymous read-only dashboard consumes them with no token). The
    // /ws upgrade is a GET, so it carries its own full `authenticated()` wrap to
    // stay gated despite the scope's mutations_only mode.
    cfg.service(
        web::scope("/analytics")
            .wrap(RequireAuth::authenticated().mutations_only())
            // --- Authenticated write / control / compute operations (POST) ---
            .route("/params", web::post().to(update_analytics_params))
            .route("/constraints", web::post().to(update_constraints))
            .route("/focus", web::post().to(set_focus))
            .route("/kernel-mode", web::post().to(set_kernel_mode))
            .route("/clustering/run", web::post().to(run_clustering))
            .route("/clustering/focus", web::post().to(focus_cluster))
            .route("/clustering/cancel", web::post().to(cancel_clustering))
            .route("/clustering/dbscan", web::post().to(run_dbscan_clustering))
            .route("/community/detect", web::post().to(run_community_detection))
            // Graph-structural anomaly (GPU LOF / Z-score) -> node_analytics.anomaly
            .route("/anomaly/detect", web::post().to(run_anomaly_detection))
            // Agent-health heuristic (MCP telemetry) -> ANOMALY_STATE, NOT node_analytics
            .route("/anomaly/toggle", web::post().to(toggle_anomaly_detection))
            .route("/sssp/compute", web::post().to(compute_sssp))
            // Each toggle is a full physics write (store, GPU, SQLite, broadcast), so it carries
            // the same limit as `/api/settings/*`.
            .service(
                web::resource("/sssp/toggle")
                    .wrap(RateLimit::per_minute(60))
                    .route(web::post().to(toggle_sssp)),
            )
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
            .route(
                "/pathfinding/sssp",
                web::post().to(pathfinding::compute_sssp),
            )
            .route(
                "/pathfinding/apsp",
                web::post().to(pathfinding::compute_apsp),
            )
            .route(
                "/pathfinding/path",
                web::post().to(pathfinding::compute_point_to_point),
            )
            .route(
                "/pathfinding/connected-components",
                web::post().to(pathfinding::compute_connected_components),
            )
            // --- Public read-only metrics (GET, harmless operational telemetry) ---
            .route(
                "/pathfinding/stats/sssp",
                web::get().to(pathfinding::get_shortest_path_stats),
            )
            .route(
                "/pathfinding/stats/components",
                web::get().to(pathfinding::get_connected_components_stats),
            )
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
            .route("/sssp/status", web::get().to(get_sssp_status))
            .route("/cycles", web::get().to(cycles_handlers::get_cycles))
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
            )
            .service(
                web::resource("/ws")
                    .wrap(RequireAuth::authenticated())
                    .route(web::get().to(websocket_integration::gpu_analytics_websocket)),
            ),
    );
}

#[cfg(test)]
mod sssp_params_removed_tests {
    use actix_web::{http::StatusCode, test, web, App, HttpMessage};

    use crate::middleware::auth::AuthenticatedUser;
    use crate::services::nostr_service::NostrService;

    /// ADR-2140: `/analytics/sssp/params` answered from hard-coded values (GET, never routed) and
    /// ignored its body (POST). Nothing called it, so both are gone; the stored physics document
    /// is read through `/sssp/status` and written through `/sssp/toggle`.
    ///
    /// The status is the discriminator: 404 for a path with no route. An auth refusal would hide a
    /// missing route, so the assertions are `== 404` (a refused probe fails), and the control test
    /// shows the probe is not simply 404-ing everything.
    async fn status_of(method: actix_web::http::Method, uri: &str) -> StatusCode {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(NostrService::new()))
                .wrap_fn(|req, srv| {
                    req.extensions_mut().insert(AuthenticatedUser {
                        pubkey: "a".repeat(64),
                    });
                    actix_web::dev::Service::call(srv, req)
                })
                .configure(super::config),
        )
        .await;
        let req = test::TestRequest::default()
            .method(method)
            .uri(uri)
            .insert_header(("Authorization", "Nostr x"))
            .set_json(serde_json::json!({"useSsspDistances": true, "ssspAlpha": 0.9}))
            .to_request();
        test::call_service(&app, req).await.status()
    }

    #[actix_web::test]
    async fn the_sssp_params_path_is_not_routed() {
        use actix_web::http::Method;
        assert_eq!(
            status_of(Method::POST, "/analytics/sssp/params").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_of(Method::GET, "/analytics/sssp/params").await,
            StatusCode::NOT_FOUND
        );
    }

    #[actix_web::test]
    async fn the_probe_reaches_a_route_that_exists() {
        // Guards the test above: the same probe must find a live path, so a 404 means "no
        // route", not "the probe is broken". Any other status (the handler's own refusal, here)
        // proves the path resolved.
        use actix_web::http::Method;
        assert_ne!(
            status_of(Method::POST, "/analytics/sssp/compute").await,
            StatusCode::NOT_FOUND
        );
    }
}
