use actix_web::{web, HttpResponse, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::ok_json;
use crate::utils::network::CircuitBreakerStats;
use crate::AppState;

/// Wrapper around `Instant` so it can be registered as Actix app data.
#[derive(Clone)]
pub struct ProcessStartTime(pub Instant);

#[derive(Serialize)]
pub struct MetricsResponse {
    pub uptime_secs: u64,
    pub active_connections: usize,
    pub circuit_breakers: HashMap<String, CircuitBreakerStats>,
}

/// GET /api/metrics
///
/// Returns JSON with process uptime, active WebSocket connections,
/// and circuit breaker states.
pub async fn get_metrics(
    app_state: web::Data<AppState>,
    start_time: web::Data<ProcessStartTime>,
) -> Result<HttpResponse> {
    let uptime_secs = start_time.0.elapsed().as_secs();
    let active_connections = app_state.active_connections.load(Ordering::Relaxed);

    // Circuit breaker stats are not currently registered as shared app data.
    // Return an empty map; when a global registry is wired into AppState this
    // will automatically populate.
    let circuit_breakers: HashMap<String, CircuitBreakerStats> = HashMap::new();

    let response = MetricsResponse {
        uptime_secs,
        active_connections,
        circuit_breakers,
    };

    ok_json!(response)
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/metrics", web::get().to(get_metrics));
}
