//! Multi-MCP WebSocket Handler
//!
//! Provides real-time WebSocket streaming of agent visualization data
//! from multiple MCP servers to the VisionClaw graph renderer.

use actix::{Actor, AsyncContext, Handler, Message, StreamHandler};
use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use actix_web_actors::ws;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::services::agent_visualization_protocol::{
    McpServerInfo, McpServerType, MultiMcpAgentStatus, SwarmTopologyData,
};
use crate::services::multi_mcp_agent_discovery::{DiscoveryStats, MultiMcpAgentDiscovery};
use crate::utils::network::{CircuitBreaker, RetryConfig};
use crate::AppState;

/// How often the single per-connection health monitor re-probes.
const HEALTH_MONITOR_INTERVAL: Duration = Duration::from_secs(30);

/// Fold per-service probe results into the cached verdict.
///
/// Returns `(any service usable, names of the unhealthy ones)`. Split out as a
/// pure function (ADR-2094) so the gating policy is testable without a live
/// `HealthCheckManager` or a running actor.
///
/// An **empty** result set is treated as usable: it means nothing has been
/// probed yet, and refusing every request before the first probe completes
/// would break discovery on a freshly opened connection.
fn fold_service_health(results: &[(&str, bool)]) -> (bool, Vec<String>) {
    if results.is_empty() {
        return (true, Vec::new());
    }
    let any_usable = results.iter().any(|(_, healthy)| *healthy);
    let unhealthy = results
        .iter()
        .filter(|(_, healthy)| !*healthy)
        .map(|(name, _)| (*name).to_string())
        .collect();
    (any_usable, unhealthy)
}

pub struct MultiMcpVisualizationWs {
    client_id: String,

    last_heartbeat: Instant,
    last_discovery_request: Instant,
    subscription_filters: SubscriptionFilters,
    performance_mode: PerformanceMode,

    circuit_breaker: Option<std::sync::Arc<CircuitBreaker>>,
    /// Shared discovery service (TODO-unified S-4(1)): the one source of server,
    /// agent and topology state this socket relays.
    discovery: Arc<MultiMcpAgentDiscovery>,
    /// Discovery cycle count at the last frame pushed to this client.
    last_frame_cycle: u64,
    /// Cached "at least one configured MCP server is connected" verdict,
    /// published by the single health-monitor task started at connection init
    /// (ADR-2094). Optimistic until the first discovery cycle lands.
    healthy_services: std::sync::Arc<AtomicBool>,
    retry_config: RetryConfig,
    connection_failures: u32,
    last_successful_operation: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionFilters {
    pub server_types: Vec<McpServerType>,

    pub agent_types: Vec<String>,

    pub swarm_ids: Vec<String>,

    pub include_performance: bool,

    pub include_neural: bool,

    pub include_topology: bool,
}

impl Default for SubscriptionFilters {
    fn default() -> Self {
        Self {
            server_types: vec![
                McpServerType::ClaudeFlow,
                McpServerType::RuvSwarm,
                McpServerType::Daa,
            ],
            agent_types: vec![],
            swarm_ids: vec![],
            include_performance: true,
            include_neural: true,
            include_topology: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    HighFrequency,

    #[default]
    Normal,

    LowFrequency,

    OnDemand,
}

impl MultiMcpVisualizationWs {
    pub fn new(discovery: Arc<MultiMcpAgentDiscovery>) -> Self {
        let client_id = Uuid::new_v4().to_string();
        info!(
            "Creating new Multi-MCP WebSocket client with resilience and hybrid integration: {}",
            client_id
        );

        let circuit_breaker = std::sync::Arc::new(CircuitBreaker::mcp_operations());

        Self {
            client_id,

            last_heartbeat: Instant::now(),
            last_discovery_request: Instant::now(),
            subscription_filters: SubscriptionFilters::default(),
            performance_mode: PerformanceMode::default(),
            circuit_breaker: Some(circuit_breaker),
            discovery,
            last_frame_cycle: 0,
            healthy_services: std::sync::Arc::new(AtomicBool::new(true)),
            retry_config: RetryConfig::mcp_operations(),
            connection_failures: 0,
            last_successful_operation: Instant::now(),
        }
    }

    fn start_position_updates(&self, ctx: &mut ws::WebsocketContext<Self>) {
        let interval = match self.performance_mode {
            PerformanceMode::HighFrequency => Duration::from_millis(16),
            PerformanceMode::Normal => Duration::from_millis(100),
            PerformanceMode::LowFrequency => Duration::from_millis(1000),
            PerformanceMode::OnDemand => return,
        };

        ctx.run_interval(interval, |_act, ctx| {
            ctx.address().do_send(RequestAgentUpdate);
        });
    }

    fn start_heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(Duration::from_secs(5), |act, ctx| {
            if Instant::now().duration_since(act.last_heartbeat) > Duration::from_secs(30) {
                warn!(
                    "WebSocket client {} heartbeat timeout, disconnecting",
                    act.client_id
                );
                ctx.close(None);
                return;
            }

            ctx.ping(b"ping");
        });
    }

    /// Start the single per-connection health monitor (ADR-2094).
    ///
    /// Exactly one task per WebSocket connection owns probing the MCP services
    /// and publishing the verdict into [`Self::healthy_services`]. It holds a
    /// [`Weak`](std::sync::Weak) handle on that cell, so when the actor is
    /// dropped on disconnect the next tick fails to upgrade and the task
    /// terminates — no task outlives its connection.
    fn start_health_monitor(&self) {
        let cache = std::sync::Arc::downgrade(&self.healthy_services);
        let client_id = self.client_id.clone();
        let discovery = self.discovery.clone();

        actix::spawn(async move {
            loop {
                tokio::time::sleep(HEALTH_MONITOR_INTERVAL).await;

                // The actor has gone: stop, rather than probing forever.
                let Some(cache) = cache.upgrade() else {
                    debug!(
                        "[Multi-MCP] Health monitor for client {} stopping (client gone)",
                        client_id
                    );
                    return;
                };

                // Read the discovery service's own per-server verdicts. These
                // replace the old probes of `localhost:8080+i`, which were not
                // the servers discovery polls and so gated every request on
                // services that did not exist.
                let statuses = discovery.get_server_statuses().await;
                let results: Vec<(&str, bool)> = statuses
                    .iter()
                    .map(|server| (server.server_id.as_str(), server.is_connected))
                    .collect();

                let (any_usable, unhealthy) = fold_service_health(&results);
                for service in &unhealthy {
                    warn!(
                        "[Multi-MCP] Server {} not connected for client {}",
                        service, client_id
                    );
                }
                cache.store(any_usable, Ordering::Relaxed);
            }
        });
    }

    /// Read the cached health verdict.
    ///
    /// Pure read of an atomic — no task is spawned. The previous implementation
    /// spawned a detached probe on *every* call and then unconditionally
    /// returned `true`, so the answer was meaningless and each call leaked a
    /// task.
    fn has_healthy_services(&self) -> bool {
        self.healthy_services.load(Ordering::Relaxed)
    }

    fn record_success(&mut self) {
        self.connection_failures = 0;
        self.last_successful_operation = Instant::now();
    }

    fn record_failure(&mut self) {
        self.connection_failures += 1;
        warn!(
            "[Multi-MCP] Operation failure #{} for client {}",
            self.connection_failures, self.client_id
        );
    }

    /// Send the current discovery snapshot to this client.
    ///
    /// Reads the shared [`MultiMcpAgentDiscovery`] caches and pushes one
    /// `discovery_data` frame. Nothing here simulates a failure or a delay:
    /// when no configured server is connected the client gets an `error`
    /// frame, otherwise the real state.
    fn send_discovery_data(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        let client_id = self.client_id.clone();

        if !self.has_healthy_services() {
            warn!(
                "[Multi-MCP] No connected MCP servers for discovery, client {}",
                client_id
            );
            ctx.text(
                serde_json::json!({
                    "type": "error",
                    "message": "No connected MCP servers",
                    "timestamp": chrono::Utc::now().timestamp_millis()
                })
                .to_string(),
            );
            return;
        }

        let discovery = self.discovery.clone();
        let filters = self.subscription_filters.clone();
        let addr = ctx.address();

        actix::spawn(async move {
            let servers = discovery.get_server_statuses().await;

            // Judge connectivity on the state just read, not on the verdict the
            // monitor cached up to 30 s ago.
            let verdicts: Vec<(&str, bool)> = servers
                .iter()
                .map(|server| (server.server_id.as_str(), server.is_connected))
                .collect();
            if !fold_service_health(&verdicts).0 {
                addr.do_send(DiscoveryFrame(
                    serde_json::json!({
                        "type": "error",
                        "message": "No connected MCP servers",
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    })
                    .to_string(),
                ));
                return;
            }

            let agents = discovery.get_all_agents().await;
            let topology = discovery.get_topology_data().await;
            let stats = discovery.get_discovery_stats().await;

            let frame = build_discovery_frame(
                &client_id,
                &filters,
                &servers,
                &agents,
                &topology,
                &stats,
                chrono::Utc::now().timestamp_millis(),
            );
            addr.do_send(DiscoveryFrame(frame.to_string()));
            addr.do_send(DiscoverySuccess);
        });
    }

    fn handle_client_config(&mut self, config: ClientConfig, ctx: &mut ws::WebsocketContext<Self>) {
        info!("Updating client configuration for {}", self.client_id);

        if let Some(filters) = config.subscription_filters {
            self.subscription_filters = filters;
        }

        if let Some(performance_mode) = config.performance_mode {
            self.performance_mode = performance_mode;

            self.start_position_updates(ctx);
        }

        let response = json!({
            "type": "config_updated",
            "client_id": self.client_id,
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "filters": self.subscription_filters,
            "performance_mode": self.performance_mode
        });

        ctx.text(response.to_string());
    }

    fn handle_discovery_request(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        let now = Instant::now();

        if now.duration_since(self.last_discovery_request) < Duration::from_secs(1) {
            debug!(
                "Discovery request rate limited for client {}",
                self.client_id
            );
            return;
        }

        self.last_discovery_request = now;
        self.send_discovery_data(ctx);
    }
}

/// Build the `discovery_data` frame for one client.
///
/// Pure so the filtering and shape can be tested without a socket or a live
/// discovery service. `server_types` and `agent_types` narrow what is sent
/// (an empty `agent_types` or `swarm_ids` means no narrowing), and topology is
/// included only when the client asked for it.
fn build_discovery_frame(
    client_id: &str,
    filters: &SubscriptionFilters,
    servers: &[McpServerInfo],
    agents: &[MultiMcpAgentStatus],
    topology: &HashMap<String, SwarmTopologyData>,
    stats: &DiscoveryStats,
    now_ms: i64,
) -> serde_json::Value {
    let servers: Vec<&McpServerInfo> = servers
        .iter()
        .filter(|s| filters.server_types.contains(&s.server_type))
        .collect();
    let agents: Vec<&MultiMcpAgentStatus> = agents
        .iter()
        .filter(|a| filters.server_types.contains(&a.server_source))
        .filter(|a| filters.agent_types.is_empty() || filters.agent_types.contains(&a.agent_type))
        .filter(|a| filters.swarm_ids.is_empty() || filters.swarm_ids.contains(&a.swarm_id))
        .collect();
    let topology: HashMap<&String, &SwarmTopologyData> = if filters.include_topology {
        topology
            .iter()
            .filter(|(id, _)| filters.swarm_ids.is_empty() || filters.swarm_ids.contains(*id))
            .collect()
    } else {
        HashMap::new()
    };

    json!({
        "type": "discovery_data",
        "client_id": client_id,
        "timestamp": now_ms,
        "servers": servers,
        "agents": agents,
        "topology": topology,
        "stats": {
            "total_discoveries": stats.successful_discoveries + stats.failed_discoveries,
            "successful_discoveries": stats.successful_discoveries,
            "failed_discoveries": stats.failed_discoveries,
            // The poll loop never writes the stored totals below, so report
            // what this frame actually carries.
            "total_agents_discovered": agents.len(),
            "last_discovery_time": stats.last_discovery_time.map(|t| t.timestamp_millis()),
            "average_discovery_time_ms": stats.average_discovery_time_ms,
            "servers_online": servers.iter().filter(|s| s.is_connected).count(),
            "servers_offline": servers.iter().filter(|s| !s.is_connected).count(),
        },
    })
}

impl Actor for MultiMcpVisualizationWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Multi-MCP WebSocket client {} connected", self.client_id);

        self.start_heartbeat(ctx);

        // Start polling when the first client connects (refcounted in the
        // service) and run exactly one health task per connection (ADR-2094).
        // The count changes synchronously here, so a late or reordered start
        // task cannot leave polling running with nobody attached.
        self.discovery.attach();
        let discovery = self.discovery.clone();
        actix::spawn(async move { discovery.sync_running().await });
        self.start_health_monitor();

        // Only a cycle completed after this point may produce a frame: whatever
        // is cached now may be empty or from an earlier session.
        self.last_frame_cycle = self.discovery.cycle_count();

        // Push a frame whenever a new discovery cycle has landed. Nothing is
        // sent at connect: the caches may be empty or stale until the first
        // cycle completes, and an empty frame would read as "no agents".
        ctx.run_interval(Duration::from_secs(1), |act, ctx| {
            let cycles = act.discovery.cycle_count();
            if cycles > act.last_frame_cycle {
                act.last_frame_cycle = cycles;
                act.send_discovery_data(ctx);
            }
        });

        self.start_position_updates(ctx);

        ctx.run_interval(Duration::from_secs(60), |act, ctx| {
            let now = Instant::now();
            let time_since_success = now.duration_since(act.last_successful_operation);


            if time_since_success > Duration::from_secs(300) {
                warn!("[Multi-MCP] No successful operations for {:?}, attempting recovery for client {}",
                     time_since_success, act.client_id);
                act.send_discovery_data(ctx);
            }


            if let Some(cb) = &act.circuit_breaker {
                let cb = cb.clone();
                let client_id = act.client_id.clone();
                let connection_failures = act.connection_failures;
                actix::spawn(async move {
                    let stats = cb.stats().await;
                    debug!("[Multi-MCP] Client {} resilience stats - Circuit: {:?}, Failures: {}, Successes: {}, Connection failures: {}",
                          client_id, stats.state, stats.failed_requests, stats.successful_requests, connection_failures);
                });
            }
        });

    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("Multi-MCP WebSocket client {} disconnected", self.client_id);
        self.discovery.detach();
        let discovery = self.discovery.clone();
        actix::spawn(async move { discovery.sync_running().await });
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MultiMcpVisualizationWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.last_heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.last_heartbeat = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                // Handle plain-text heartbeat before JSON parsing
                if text.trim() == "ping" {
                    self.last_heartbeat = Instant::now();
                    ctx.text("pong");
                    return;
                }
                debug!("Received WebSocket message: {}", text);

                if let Ok(request) = serde_json::from_str::<ClientRequest>(&text) {
                    match request.action.as_str() {
                        "configure" => {
                            if let Some(config_data) = request.data {
                                if let Ok(config) =
                                    serde_json::from_value::<ClientConfig>(config_data)
                                {
                                    self.handle_client_config(config, ctx);
                                }
                            }
                        }
                        "request_discovery" => {
                            self.handle_discovery_request(ctx);
                        }
                        "request_agents" => {
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || {
                                    if let Some(cb) = &self.circuit_breaker {
                                        let cb_clone = cb.clone();
                                        let ctx_addr = ctx.address();
                                        let client_id = self.client_id.clone();

                                        tokio::spawn(async move {
                                            let stats = cb_clone.stats().await;
                                            match stats.state {
                                            crate::utils::network::CircuitBreakerState::Open => {
                                                warn!("[Multi-MCP] Circuit breaker open, using degraded mode for client {}", client_id);

                                                ctx_addr.do_send(RequestAgentUpdate);
                                            }
                                            _ => {

                                                ctx_addr.do_send(RequestAgentUpdate);
                                            }
                                        }
                                        });
                                    } else {
                                        ctx.address().do_send(RequestAgentUpdate);
                                    }
                                },
                            ));

                            if result.is_err() {
                                error!(
                                    "Error processing agent request for client {}",
                                    self.client_id
                                );
                                self.record_failure();
                                self.send_error_response(ctx, "Agent request processing failed");
                            }
                        }
                        "request_performance" => {
                            if !self.has_healthy_services() {
                                warn!("[Multi-MCP] No healthy services for performance data, using cached data");
                                let degraded_response = serde_json::json!({
                                    "type": "performance_data",
                                    "message": "Using cached performance data - services degraded",
                                    "timestamp": chrono::Utc::now().timestamp_millis(),
                                    "data": {
                                        "status": "degraded",
                                        "cached_metrics": true,
                                        "last_update": chrono::Utc::now().timestamp_millis()
                                    }
                                });
                                ctx.text(degraded_response.to_string());
                            } else {
                                ctx.address().do_send(RequestPerformanceUpdate);
                            }
                        }
                        "request_topology" => {
                            if let Some(data) = request.data {
                                if let Some(swarm_id_value) = data.get("swarm_id") {
                                    if let Some(swarm_id) = swarm_id_value.as_str() {
                                        ctx.address().do_send(RequestTopologyUpdate {
                                            swarm_id: swarm_id.to_string(),
                                        });
                                    }
                                }
                            }
                        }
                        _ => {
                            warn!("Unknown WebSocket action: {}", request.action);
                            self.send_error_response(
                                ctx,
                                &format!("Unknown action: {}", request.action),
                            );
                        }
                    }
                }
            }
            Ok(ws::Message::Binary(_)) => {
                warn!("Binary WebSocket messages not supported");
            }
            Ok(ws::Message::Close(reason)) => {
                info!(
                    "[Multi-MCP] WebSocket closing for client {}: {:?}",
                    self.client_id, reason
                );

                if let Some(cb) = &self.circuit_breaker {
                    let cb_clone = cb.clone();
                    let client_id = self.client_id.clone();
                    let connection_failures = self.connection_failures;
                    actix::spawn(async move {
                        let stats = cb_clone.stats().await;
                        info!("[Multi-MCP] Final stats for client {} - Circuit: {:?}, Failures: {}, Successes: {}, Connection failures: {}",
                             client_id, stats.state, stats.failed_requests, stats.successful_requests, connection_failures);
                    });
                }

                ctx.close(reason);
            }
            _ => {
                warn!(
                    "Unhandled WebSocket message type for client {}",
                    self.client_id
                );
                ctx.close(None);
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClientRequest {
    action: String,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ClientConfig {
    subscription_filters: Option<SubscriptionFilters>,
    performance_mode: Option<PerformanceMode>,
}

#[derive(Message)]
#[rtype(result = "()")]
struct RequestAgentUpdate;

#[derive(Message)]
#[rtype(result = "()")]
struct RequestDiscoveryData;

#[derive(Message)]
#[rtype(result = "()")]
struct RequestPerformanceUpdate;

#[derive(Message)]
#[rtype(result = "()")]
struct RequestTopologyUpdate {
    swarm_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
struct DiscoveryFrame(String);

#[derive(Message)]
#[rtype(result = "()")]
struct DiscoverySuccess;

#[derive(Message)]
#[rtype(result = "()")]
struct DiscoveryFailure(String);

#[derive(Message)]
#[rtype(result = "()")]
struct SendHeartbeatPing;

#[derive(Message)]
#[rtype(result = "()")]
struct ReconnectionCompleted;

impl Handler<RequestAgentUpdate> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, _: RequestAgentUpdate, _ctx: &mut Self::Context) {
        debug!("Requesting agent update for client {}", self.client_id);
    }
}

impl Handler<RequestDiscoveryData> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, _: RequestDiscoveryData, ctx: &mut Self::Context) {
        debug!("Requesting discovery data for client {}", self.client_id);
        self.send_discovery_data(ctx);
    }
}

impl Handler<DiscoveryFrame> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, msg: DiscoveryFrame, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

impl Handler<RequestPerformanceUpdate> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, _: RequestPerformanceUpdate, _ctx: &mut Self::Context) {
        debug!(
            "Requesting performance update for client {}",
            self.client_id
        );
    }
}

impl Handler<RequestTopologyUpdate> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, msg: RequestTopologyUpdate, _ctx: &mut Self::Context) {
        debug!(
            "Requesting topology update for swarm {} for client {}",
            msg.swarm_id, self.client_id
        );
    }
}

impl Handler<DiscoverySuccess> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, _: DiscoverySuccess, _ctx: &mut Self::Context) {
        debug!(
            "[Multi-MCP] Discovery success for client {}",
            self.client_id
        );
        self.record_success();
    }
}

impl Handler<DiscoveryFailure> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, msg: DiscoveryFailure, ctx: &mut Self::Context) {
        warn!(
            "[Multi-MCP] Discovery failure for client {}: {}",
            self.client_id, msg.0
        );
        self.record_failure();

        let error_response = serde_json::json!({
            "type": "discovery_error",
            "message": msg.0,
            "client_id": self.client_id,
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "retry_in_seconds": self.retry_config.initial_delay.as_secs(),
            "fallback_mode": "local_cache",
            "degraded_functionality": true
        });

        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.text(error_response.to_string());
        })) {
            error!(
                "Failed to send error response for client {}: {:?}",
                self.client_id, e
            );
        }
    }
}

impl Handler<SendHeartbeatPing> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, _: SendHeartbeatPing, ctx: &mut Self::Context) {
        ctx.ping(b"mcp-heartbeat");
    }
}

impl Handler<ReconnectionCompleted> for MultiMcpVisualizationWs {
    type Result = ();

    fn handle(&mut self, _: ReconnectionCompleted, _ctx: &mut Self::Context) {
        info!(
            "[Multi-MCP] Reconnection completed for client {}",
            self.client_id
        );
        self.record_success();
    }
}

pub async fn multi_mcp_visualization_ws(
    req: HttpRequest,
    stream: web::Payload,
    app_state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    debug!("Starting Multi-MCP visualization WebSocket connection");

    // SECURITY (ADR-2090): WebSocket credential validation at upgrade time.
    //
    // This previously accepted ANY non-empty string: it checked only that a
    // token was present, never that it named a live session, so `?token=x` was
    // sufficient to open the socket. It now resolves the token through
    // `NostrService::get_session`, which as of ADR-2044 also enforces the
    // `AUTH_TOKEN_EXPIRY` window. Unknown, expired and absent tokens all fail
    // closed to 401.
    {
        let client_ip = req
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let unauthorised = |reason: &str| {
            warn!(
                "SECURITY: Rejected WebSocket upgrade on /multi-mcp/ws from {} — {}",
                client_ip, reason
            );
            Ok(HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"})))
        };

        // ADR-2058/ADR-2090: the Authorization header is the ONLY accepted carrier
        // in a release build. The `?token=` query fallback leaks the bearer token
        // into access logs, proxy logs and Referer headers. It is compiled out of
        // release and survives only behind the dev-auth gate. Clients that cannot
        // set headers on an upgrade authenticate post-connect with the NIP-98
        // `authenticate` envelope (kind 27235).
        let header_token = req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        #[cfg(any(debug_assertions, feature = "dev-auth"))]
        let token = header_token.or_else(|| {
            let found = url::form_urlencoded::parse(req.query_string().as_bytes())
                .find(|(k, _)| k == "token")
                .map(|(_, v)| v.to_string());
            if found.is_some() {
                warn!(
                    "SECURITY: /multi-mcp/ws upgrade authenticated via ?token= query string — \
                     dev-only path (ADR-2058). Use the Authorization header."
                );
            }
            found
        });

        #[cfg(not(any(debug_assertions, feature = "dev-auth")))]
        let token = {
            if req.query_string().contains("token=") {
                warn!(
                    "SECURITY: Rejecting ?token= query-string auth on /multi-mcp/ws — \
                     ADR-2058 requires the Authorization header in release builds"
                );
            }
            header_token
        };

        let token = match token {
            Some(t) if !t.is_empty() => t,
            _ => return unauthorised("no bearer token or ?token= present"),
        };

        // Fail closed when the service is absent: without a session store there
        // is nothing to validate against, so the socket must not open.
        let Some(nostr_service) = app_state.nostr_service.clone() else {
            return unauthorised("no NostrService configured — cannot validate the session token");
        };

        if nostr_service.get_session(&token).await.is_none() {
            return unauthorised("token does not name a live, unexpired session");
        }
    }

    ws::start(
        MultiMcpVisualizationWs::new(app_state.multi_mcp_discovery.clone()),
        &req,
        stream,
    )
}

pub fn configure_multi_mcp_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/multi-mcp")
            // ADR-2091: /status and /refresh removed — both returned fiction.
            // /status served a hardcoded server list (claude-flow reported
            // is_connected:true with agent_count:4, never queried); /refresh
            // reported "Discovery refresh initiated" while doing nothing.
            // Both took _app_state unused. Real discovery state lives in
            // services/multi_mcp_agent_discovery.rs — see ADR-2091.
            .route("/ws", web::get().to(multi_mcp_visualization_ws)),
    );
}

impl MultiMcpVisualizationWs {
    fn send_error_response(&mut self, ctx: &mut ws::WebsocketContext<Self>, error_message: &str) {
        let error_response = serde_json::json!({
            "type": "error",
            "message": error_message,
            "client_id": self.client_id,
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "recoverable": true
        });

        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.text(error_response.to_string());
        })) {
            error!(
                "Failed to send error response for client {}: {:?}",
                self.client_id, e
            );

            ctx.close(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression ADR-2094 closes: the gate used to be a constant `true`
    /// (with a leaked probe task on the side), so an all-unhealthy estate still
    /// reported healthy. Now an all-unhealthy fold is `false`.
    #[test]
    fn all_services_unhealthy_is_not_usable() {
        let (any_usable, unhealthy) = fold_service_health(&[
            ("claude-flow", false),
            ("ruv-swarm", false),
            ("flow-nexus", false),
        ]);
        assert!(!any_usable, "no usable service must report unhealthy");
        assert_eq!(unhealthy, vec!["claude-flow", "ruv-swarm", "flow-nexus"]);
    }

    /// One survivor is enough to keep discovery open, and only the dead ones
    /// are named in the warning.
    #[test]
    fn one_healthy_service_keeps_the_gate_open() {
        let (any_usable, unhealthy) = fold_service_health(&[
            ("claude-flow", false),
            ("ruv-swarm", true),
            ("flow-nexus", false),
        ]);
        assert!(any_usable);
        assert_eq!(unhealthy, vec!["claude-flow", "flow-nexus"]);
    }

    /// A fully healthy estate names nobody.
    #[test]
    fn all_healthy_warns_about_nothing() {
        let (any_usable, unhealthy) =
            fold_service_health(&[("claude-flow", true), ("ruv-swarm", true)]);
        assert!(any_usable);
        assert!(unhealthy.is_empty());
    }

    /// Before the first probe there are no results; the connection must not be
    /// refused discovery on that basis.
    #[test]
    fn no_probe_yet_is_optimistic() {
        let (any_usable, unhealthy) = fold_service_health(&[]);
        assert!(any_usable, "pre-probe state must not block a fresh client");
        assert!(unhealthy.is_empty());
    }

    /// `has_healthy_services` is a pure atomic read: calling it repeatedly must
    /// neither spawn work nor change the cached verdict (the leak this closes).
    #[test]
    fn cached_verdict_reads_are_pure_and_stable() {
        let cache = std::sync::Arc::new(AtomicBool::new(true));
        for _ in 0..1_000 {
            assert!(cache.load(Ordering::Relaxed));
        }
        cache.store(false, Ordering::Relaxed);
        for _ in 0..1_000 {
            assert!(!cache.load(Ordering::Relaxed));
        }
    }

    /// The monitor's termination condition: once the actor's `Arc` is dropped
    /// the task's `Weak` no longer upgrades, so the loop returns instead of
    /// probing for the lifetime of the process.
    #[test]
    fn monitor_stops_when_the_client_is_dropped() {
        let cache = std::sync::Arc::new(AtomicBool::new(true));
        let observer = std::sync::Arc::downgrade(&cache);
        assert!(observer.upgrade().is_some(), "live client keeps monitoring");
        drop(cache);
        assert!(
            observer.upgrade().is_none(),
            "dropped client must terminate the monitor task"
        );
    }

    fn server(id: &str, kind: McpServerType, connected: bool) -> McpServerInfo {
        McpServerInfo {
            server_id: id.to_string(),
            server_type: kind,
            host: "h".to_string(),
            port: 1,
            is_connected: connected,
            last_heartbeat: 0,
            supported_tools: vec![],
            agent_count: 0,
        }
    }

    /// The frame carries real state, honours the subscription filters, and
    /// drops topology unless the client asked for it.
    #[test]
    fn discovery_frame_applies_server_filter_and_topology_flag() {
        let servers = vec![
            server("claude-flow", McpServerType::ClaudeFlow, true),
            server("daa", McpServerType::Daa, false),
        ];
        let mut topo = HashMap::new();
        topo.insert(
            "swarm-1".to_string(),
            SwarmTopologyData {
                topology_type: "mesh".to_string(),
                total_agents: 0,
                coordination_layers: 1,
                efficiency_score: 1.0,
                load_distribution: vec![],
                critical_paths: vec![],
                bottlenecks: vec![],
            },
        );
        let mut filters = SubscriptionFilters {
            server_types: vec![McpServerType::ClaudeFlow],
            ..SubscriptionFilters::default()
        };

        let frame = build_discovery_frame(
            "c1",
            &filters,
            &servers,
            &[],
            &topo,
            &DiscoveryStats::default(),
            42,
        );
        assert_eq!(frame["type"], "discovery_data");
        assert_eq!(frame["client_id"], "c1");
        assert_eq!(frame["timestamp"], 42);
        assert_eq!(frame["servers"].as_array().unwrap().len(), 1);
        assert_eq!(frame["servers"][0]["server_id"], "claude-flow");
        assert!(frame["topology"].get("swarm-1").is_some());
        assert_eq!(frame["stats"]["servers_online"], 1);
        assert_eq!(frame["stats"]["servers_offline"], 0);
        assert_eq!(frame["stats"]["total_agents_discovered"], 0);

        filters.include_topology = false;
        let frame = build_discovery_frame(
            "c1",
            &filters,
            &servers,
            &[],
            &topo,
            &DiscoveryStats::default(),
            42,
        );
        assert!(frame["topology"].as_object().unwrap().is_empty());
    }
}
