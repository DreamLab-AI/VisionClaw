use crate::actors::messages::{GetGPUStatus, GetGraphData, GetMetadata, GetSettings};
use crate::ok_json;
use crate::services::mcp_relay_manager::McpRelayManager;
use crate::AppState;
use actix_web::{web, HttpResponse, Result};
use chrono::Utc;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::process::Command;
use sysinfo::System;
use tokio::time::Duration;

/// Timeout duration for individual subsystem health checks
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// Threshold percentage for high CPU usage warning (f32 to match cpu_usage type)
const HIGH_CPU_THRESHOLD: f32 = 90.0;
/// Threshold percentage for high memory usage warning
const HIGH_MEMORY_THRESHOLD: f64 = 90.0;
/// Threshold percentage for high disk usage warning
const HIGH_DISK_THRESHOLD: f64 = 90.0;

#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: String,
    pub issues: Vec<String>,
    pub system: SystemMetrics,
    pub services: ServiceMetrics,
    pub mcp: Option<McpMetrics>,
}

#[derive(Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub gpu_status: String,
}

#[derive(Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub metadata_count: usize,
    pub nodes_count: usize,
    pub edges_count: usize,
    pub mcp_status: String,
}

#[derive(Serialize, Deserialize)]
pub struct McpMetrics {
    pub container_running: bool,
    pub mcp_relay_running: bool,
    pub last_logs: Option<String>,
    pub message: String,
}

#[derive(Serialize, Deserialize)]
pub struct PhysicsSimulationStatus {
    status: String,
    details: String,
    timestamp: String,
}

/// The node, edge and metadata counts a health reply states, for the caller identified by
/// `pubkey`. `store_metadata` is the metadata store's row count.
pub(crate) fn caller_graph_counts(
    graph: &std::sync::Arc<visionclaw_domain::models::graph::GraphData>,
    store_metadata: usize,
    pubkey: Option<&str>,
    filter_on: bool,
) -> (usize, usize, usize) {
    // ADR-060: a caller whose view hides nodes is told the counts of that view, so a hidden
    // node, its edges and its metadata row count as if they did not exist. A full-access
    // caller keeps the whole-graph numbers and the metadata store's own row count.
    if crate::handlers::graph_access::gate_for_graph_with(graph, pubkey, filter_on).is_none() {
        return (graph.nodes.len(), graph.edges.len(), store_metadata);
    }
    let c = crate::handlers::graph_access::visible_counts(graph, pubkey, filter_on);
    (c.nodes, c.edges, c.metadata)
}

pub async fn unified_health_check(
    app_state: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
) -> Result<HttpResponse> {
    let pubkey = crate::handlers::graph_access::caller_pubkey(&caller);
    let mut health_status = "healthy".to_string();
    let mut issues = Vec::new();

    // Check application-level degraded state (e.g. Oxigraph store init failure)
    if let Some(reason) = app_state.get_degraded_reason() {
        health_status = "degraded".to_string();
        issues.push(reason);
    }

    let system_metrics = check_system_metrics(&mut health_status, &mut issues);

    // D5: compute the real MCP state ONCE, before building ServiceMetrics, so the
    // `services.mcp_status` field derives from the same check as the `mcp` block
    // below. /api/health then reports one MCP truth, not two disagreeing fields.
    let mcp_metrics = check_mcp_metrics().await;

    let service_metrics = check_service_metrics(
        pubkey,
        &app_state,
        mcp_status_label(&mcp_metrics),
        &mut health_status,
        &mut issues,
    )
    .await;

    if health_status == "healthy" && !issues.is_empty() {
        health_status = "degraded".to_string();
    }

    let response = HealthResponse {
        status: health_status,
        timestamp: Utc::now().to_rfc3339(),
        issues,
        system: system_metrics,
        services: service_metrics,
        mcp: Some(mcp_metrics),
    };

    ok_json!(response)
}

fn check_system_metrics(health_status: &mut String, issues: &mut Vec<String>) -> SystemMetrics {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_usage = sys.global_cpu_usage();
    let memory_usage = sys.used_memory() as f64 / sys.total_memory() as f64 * 100.0;
    let disk_usage = check_disk_usage();
    let gpu_status = check_gpu_status();

    if cpu_usage > HIGH_CPU_THRESHOLD {
        *health_status = "degraded".to_string();
        issues.push("High CPU usage".to_string());
    }
    if memory_usage > HIGH_MEMORY_THRESHOLD {
        *health_status = "degraded".to_string();
        issues.push("High memory usage".to_string());
    }
    if disk_usage > HIGH_DISK_THRESHOLD {
        *health_status = "degraded".to_string();
        issues.push("High disk usage".to_string());
    }

    SystemMetrics {
        cpu_usage: cpu_usage as f64,
        memory_usage,
        disk_usage,
        gpu_status,
    }
}

/// Derive the single-word `services.mcp_status` label from the real MCP check,
/// so it never disagrees with the `mcp` block on the same /api/health response
/// (D5). Kept in lockstep with [`check_mcp_metrics`]'s message states.
fn mcp_status_label(metrics: &McpMetrics) -> &'static str {
    match (metrics.container_running, metrics.mcp_relay_running) {
        (false, _) => "not_running",
        (true, false) => "relay_down",
        (true, true) => "connected",
    }
}

async fn check_service_metrics(
    pubkey: Option<&str>,
    app_state: &web::Data<AppState>,
    mcp_status: &str,
    health_status: &mut String,
    issues: &mut Vec<String>,
) -> ServiceMetrics {
    let filter_on = visionclaw_domain::utils::visibility_filter::filter_enabled();
    let mut metadata_count = match tokio::time::timeout(
        HEALTH_CHECK_TIMEOUT,
        app_state.metadata_addr.send(GetMetadata),
    )
    .await
    {
        Ok(Ok(Ok(metadata_store))) => metadata_store.len(),
        Ok(Ok(Err(_))) => {
            *health_status = "degraded".to_string();
            issues.push("Metadata store error".to_string());
            0
        }
        Ok(Err(_)) => {
            *health_status = "degraded".to_string();
            issues.push("Metadata actor not responding".to_string());
            0
        }
        Err(_) => {
            *health_status = "unhealthy".to_string();
            issues.push("Metadata actor timeout".to_string());
            0
        }
    };

    let (nodes_count, edges_count) = match tokio::time::timeout(
        HEALTH_CHECK_TIMEOUT,
        app_state.graph_service_addr.send(GetGraphData),
    )
    .await
    {
        Ok(Ok(Ok(graph_data))) => {
            let c = caller_graph_counts(&graph_data, metadata_count, pubkey, filter_on);
            metadata_count = c.2;
            (c.0, c.1)
        }
        Ok(Ok(Err(_))) => {
            *health_status = "degraded".to_string();
            issues.push("Graph service error".to_string());
            (0, 0)
        }
        Ok(Err(_)) => {
            *health_status = "degraded".to_string();
            issues.push("Graph service actor not responding".to_string());
            (0, 0)
        }
        Err(_) => {
            *health_status = "unhealthy".to_string();
            issues.push("Graph service actor timeout".to_string());
            (0, 0)
        }
    };

    ServiceMetrics {
        metadata_count,
        nodes_count,
        edges_count,
        mcp_status: mcp_status.to_string(),
    }
}

async fn check_mcp_metrics() -> McpMetrics {
    let container_running = McpRelayManager::check_mcp_container();
    let mcp_relay_running = if container_running {
        let manager = McpRelayManager::new();
        manager.check_relay_status().await.unwrap_or(false)
    } else {
        false
    };

    let last_logs = if container_running {
        McpRelayManager::get_relay_logs(20).ok()
    } else {
        None
    };

    let message = match (container_running, mcp_relay_running) {
        (false, _) => "Multi-agent container is not running".to_string(),
        (true, false) => "Container is running but MCP relay is not active".to_string(),
        (true, true) => "MCP relay is healthy and running".to_string(),
    };

    McpMetrics {
        container_running,
        mcp_relay_running,
        last_logs,
        message,
    }
}

fn check_disk_usage() -> f64 {
    match Command::new("df").args(["."]).output() {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = output_str.lines().nth(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    if let Ok(usage) = parts[4].trim_end_matches('%').parse::<f64>() {
                        return usage;
                    }
                }
            }
        }
        Err(e) => warn!("Failed to check disk usage: {}", e),
    }
    0.0
}

fn check_gpu_status() -> String {
    match Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                if let Some(usage_str) = output_str.lines().next() {
                    if let Ok(usage) = usage_str.trim().parse::<f64>() {
                        return format!("available ({}% usage)", usage);
                    }
                }
                "available".to_string()
            } else {
                "unavailable".to_string()
            }
        }
        Err(_) => match Command::new("nvcc").args(["--version"]).output() {
            Ok(output) if output.status.success() => "cuda_only".to_string(),
            _ => "unavailable".to_string(),
        },
    }
}

pub async fn check_physics_simulation(
    app_state: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
) -> Result<HttpResponse> {
    let pubkey = crate::handlers::graph_access::caller_pubkey(&caller);
    let current_time = Utc::now();

    let (status, diagnostics) = match get_physics_diagnostics(&app_state, pubkey).await {
        Ok((s, d)) => (s, d),
        Err(e) => {
            error!("Failed to get physics diagnostics: {}", e);
            ("error".to_string(), format!("Diagnostics failed: {}", e))
        }
    };

    info!(
        "Physics simulation diagnostic check at {}: {}",
        current_time, diagnostics
    );

    ok_json!(PhysicsSimulationStatus {
        status,
        details: diagnostics,
        timestamp: current_time.to_rfc3339(),
    })
}

async fn get_physics_diagnostics(
    app_state: &web::Data<AppState>,
    pubkey: Option<&str>,
) -> Result<(String, String), String> {
    let mut diagnostics = Vec::new();
    let mut status = "healthy".to_string();
    let filter_on = visionclaw_domain::utils::visibility_filter::filter_enabled();

    match tokio::time::timeout(
        Duration::from_secs(3),
        app_state.graph_service_addr.send(GetGraphData),
    )
    .await
    {
        Ok(Ok(Ok(graph_data))) => {
            let (nodes, edges, _) = caller_graph_counts(&graph_data, 0, pubkey, filter_on);
            diagnostics.push(format!("Graph: {} nodes, {} edges", nodes, edges));
            if nodes == 0 {
                status = "warning".to_string();
                diagnostics.push("No nodes in graph".to_string());
            }
        }
        Ok(Ok(Err(e))) => {
            status = "error".to_string();
            diagnostics.push(format!("Graph service error: {}", e));
        }
        Ok(Err(_)) => {
            status = "error".to_string();
            diagnostics.push("Graph service actor not responding".to_string());
        }
        Err(_) => {
            status = "error".to_string();
            diagnostics.push("Graph service timeout".to_string());
        }
    }

    #[cfg(feature = "gpu")]
    if let Some(gpu_compute_addr) = app_state.get_gpu_compute_addr().await {
        match tokio::time::timeout(Duration::from_secs(2), gpu_compute_addr.send(GetGPUStatus))
            .await
        {
            Ok(Ok(gpu_status)) => {
                diagnostics.push(format!("GPU compute: available, status: {:?}", gpu_status));
            }
            Ok(Err(e)) => {
                diagnostics.push(format!("GPU compute error: {}", e));
                if status == "healthy" {
                    status = "degraded".to_string();
                }
            }
            _ => {
                diagnostics.push("GPU compute not responding".to_string());
                if status == "healthy" {
                    status = "degraded".to_string();
                }
            }
        }
    } else {
        diagnostics.push("GPU compute not available - using CPU fallback".to_string());
    }

    let physics_info = check_physics_parameters(app_state).await;
    diagnostics.push(physics_info);

    let full_diagnostics = diagnostics.join("; ");
    Ok((status, full_diagnostics))
}

async fn check_physics_parameters(app_state: &web::Data<AppState>) -> String {
    // Try to read actual physics settings from OptimizedSettingsActor
    match tokio::time::timeout(
        Duration::from_secs(2),
        app_state.settings_addr.send(GetSettings),
    )
    .await
    {
        Ok(Ok(Ok(settings))) => {
            let physics = &settings.visualisation.graphs.knowledge.physics;
            let gravity = physics.gravity;
            let damping = physics.damping;
            let spring_k = physics.spring_k;

            if gravity < 0.0 || gravity > 1.0 {
                return format!("Invalid gravity value: {}", gravity);
            }
            if damping < 0.0 || damping > 1.0 {
                return format!("Invalid damping value: {}", damping);
            }
            if spring_k < 0.0 || spring_k > 1.0 {
                return format!("Invalid spring strength: {}", spring_k);
            }

            format!(
                "Physics params OK (gravity: {}, damping: {}, spring: {})",
                gravity, damping, spring_k
            )
        }
        _ => {
            // Fallback to hardcoded defaults when settings actor is unavailable
            let gravity = 0.08;
            let damping = 0.92;
            let spring_k = 0.3;

            if gravity < 0.0 || gravity > 1.0 {
                return "Invalid gravity value [fallback]".to_string();
            }
            if damping < 0.0 || damping > 1.0 {
                return "Invalid damping value [fallback]".to_string();
            }
            if spring_k < 0.0 || spring_k > 1.0 {
                return "Invalid spring strength [fallback]".to_string();
            }

            format!(
                "Physics params OK (gravity: {}, damping: {}, spring: {} [fallback])",
                gravity, damping, spring_k
            )
        }
    }
}

pub async fn start_mcp_relay() -> Result<HttpResponse> {
    let manager = McpRelayManager::new();
    match manager.ensure_relay_running().await {
        Ok(_) => ok_json!(serde_json::json!({
            "success": true,
            "message": "MCP relay started successfully"
        })),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
    }
}

#[derive(Deserialize)]
pub struct LogQuery {
    lines: Option<usize>,
}

pub async fn get_mcp_logs(query: web::Query<LogQuery>) -> Result<HttpResponse> {
    let lines = query.lines.unwrap_or(50);

    match McpRelayManager::get_relay_logs(lines) {
        Ok(logs) => ok_json!(serde_json::json!({
            "success": true,
            "logs": logs
        })),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
    }
}

/// Liveness probe — returns 200 immediately with no system checks.
/// Mount at `/healthz` (outside `/api` scope) for Kubernetes/Docker probes.
pub async fn liveness_probe() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({"status": "alive"}))
}

/// Readiness probe — returns 200 if the application is ready to serve traffic,
/// 503 if critical subsystems (e.g. Oxigraph store) are unavailable.
pub async fn readiness_probe(app_state: web::Data<AppState>) -> HttpResponse {
    if let Some(reason) = app_state.get_degraded_reason() {
        HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "not_ready",
            "reason": reason,
        }))
    } else {
        HttpResponse::Ok().json(serde_json::json!({"status": "ready"}))
    }
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/health")
            .route("", web::get().to(unified_health_check))
            .route("/physics", web::get().to(check_physics_simulation))
            .service(
                web::scope("/mcp")
                    .route("/start", web::post().to(start_mcp_relay))
                    .route("/logs", web::get().to(get_mcp_logs)),
            ),
    )
    // Lightweight probe endpoints under /api for back-compat. The canonical
    // root-level /healthz and /readyz (k8s/Docker convention) are registered
    // directly on the App in main.rs.
    .route("/healthz", web::get().to(liveness_probe))
    .route("/readyz", web::get().to(readiness_probe));
}

#[cfg(test)]
mod caller_count_tests {
    use super::caller_graph_counts;
    use std::sync::Arc;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::graph::GraphData;
    use visionclaw_domain::models::metadata::Metadata;
    use visionclaw_domain::models::node::Node;

    /// Public 0 and 1 joined by an edge; when `with_hidden`, a hidden 2 with an edge to 0 and a
    /// metadata row of its own.
    fn graph(with_hidden: bool) -> Arc<GraphData> {
        let mut g = GraphData::default();
        for id in 0..if with_hidden { 3 } else { 2 } {
            let mut node = Node::new_with_id(format!("p{id}.md"), None);
            node.id = id;
            if id < 2 {
                node.metadata.insert("public".into(), "true".into());
            }
            g.metadata.insert(format!("p{id}.md"), Metadata::default());
            g.nodes.push(node);
        }
        g.edges.push(Edge::new(0, 1, 1.0));
        if with_hidden {
            g.edges.push(Edge::new(0, 2, 1.0));
        }
        Arc::new(g)
    }

    #[test]
    fn health_counts_a_hidden_node_exactly_as_one_that_does_not_exist() {
        let store_rows = |g: &GraphData| g.metadata.len();
        let with = graph(true);
        let without = graph(false);
        assert_eq!(
            caller_graph_counts(&with, store_rows(&with), None, true),
            caller_graph_counts(&without, store_rows(&without), None, true),
        );
    }

    #[test]
    fn a_full_access_caller_still_gets_whole_graph_health_counts() {
        let g = graph(true);
        assert_eq!(caller_graph_counts(&g, 3, None, false), (3, 2, 3));
    }
}
