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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::services::agent_visualization_protocol::McpServerType;
use crate::AppState;
// DEPRECATED: HybridHealthManager removed
use crate::utils::network::{
    retry_with_backoff, CircuitBreaker, HealthCheckConfig, HealthCheckManager, RetryConfig,
    RetryableError, ServiceEndpoint, TimeoutConfig,
};

/// MCP services whose health gates discovery and performance responses.
const MONITORED_SERVICES: &[&str] = &["claude-flow", "ruv-swarm", "flow-nexus"];

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

// Define a simple retryable error type for MCP operations
#[derive(Debug, Clone)]
struct McpError(String);

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MCP Error: {}", self.0)
    }
}

impl std::error::Error for McpError {}

impl RetryableError for McpError {
    fn is_retryable(&self) -> bool {
        true
    }
}

pub struct MultiMcpVisualizationWs {
    #[allow(dead_code)]
    app_state: web::Data<AppState>,
    _hybrid_manager: Option<()>,
    client_id: String,

    last_heartbeat: Instant,
    last_discovery_request: Instant,
    subscription_filters: SubscriptionFilters,
    performance_mode: PerformanceMode,

    timeout_config: TimeoutConfig,
    circuit_breaker: Option<std::sync::Arc<CircuitBreaker>>,
    health_manager: Option<std::sync::Arc<HealthCheckManager>>,
    /// Cached "at least one MCP service is usable" verdict, published by the
    /// single health-monitor task started at connection init (ADR-2094).
    /// Optimistic until the first probe lands.
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
    pub fn new(app_state: web::Data<AppState>, _hybrid_manager: Option<()>) -> Self {
        let client_id = Uuid::new_v4().to_string();
        info!(
            "Creating new Multi-MCP WebSocket client with resilience and hybrid integration: {}",
            client_id
        );

        let circuit_breaker = std::sync::Arc::new(CircuitBreaker::mcp_operations());

        let health_manager_network = std::sync::Arc::new(HealthCheckManager::new());

        Self {
            app_state,
            _hybrid_manager: None,
            client_id,

            last_heartbeat: Instant::now(),
            last_discovery_request: Instant::now(),
            subscription_filters: SubscriptionFilters::default(),
            performance_mode: PerformanceMode::default(),
            timeout_config: TimeoutConfig::websocket(),
            circuit_breaker: Some(circuit_breaker),
            health_manager: Some(health_manager_network),
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
    fn start_health_monitor(&self, health_manager: std::sync::Arc<HealthCheckManager>) {
        let cache = std::sync::Arc::downgrade(&self.healthy_services);
        let client_id = self.client_id.clone();

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

                let mut results = Vec::with_capacity(MONITORED_SERVICES.len());
                for service in MONITORED_SERVICES {
                    let is_healthy = health_manager
                        .check_service_now(service)
                        .await
                        .is_some_and(|result| result.status.is_usable());
                    results.push((*service, is_healthy));
                }

                let (any_usable, unhealthy) = fold_service_health(&results);
                for service in &unhealthy {
                    warn!(
                        "[Multi-MCP] Service {} unhealthy for client {}",
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

    fn send_discovery_data(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        let client_id = self.client_id.clone();
        let circuit_breaker = self.circuit_breaker.clone();
        let _timeout_config = self.timeout_config.clone();

        let _app_state = ctx.address();

        if !self.has_healthy_services() {
            warn!(
                "[Multi-MCP] No healthy services available for discovery, client {}",
                client_id
            );
            ctx.text(
                serde_json::json!({
                    "type": "error",
                    "message": "No healthy MCP services available",
                    "timestamp": chrono::Utc::now().timestamp_millis()
                })
                .to_string(),
            );
            return;
        }

        if let Some(cb) = circuit_breaker {
            let addr = ctx.address();
            let retry_config = self.retry_config.clone();
            let failures = self.connection_failures;

            actix::spawn(async move {
                let result = retry_with_backoff(retry_config, || {
                    let cb_clone = cb.clone();
                    Box::pin(async move {
                        cb_clone
                            .execute(async {
                                if fastrand::f32() < 0.2 && failures > 0 {
                                    return Err(Box::new(std::io::Error::new(
                                        std::io::ErrorKind::ConnectionRefused,
                                        "Discovery service temporarily unavailable",
                                    ))
                                        as Box<dyn std::error::Error + Send + Sync>);
                                }

                                tokio::time::sleep(Duration::from_millis(100)).await;
                                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                            })
                            .await
                            .map_err(|e| McpError(format!("{:?}", e)))
                    })
                })
                .await;

                match result {
                    Ok(_) => {
                        debug!("Discovery operation successful for client: {}", client_id);
                        addr.do_send(DiscoverySuccess);
                        addr.do_send(RequestDiscoveryData);
                    }
                    Err(e) => {
                        error!(
                            "Discovery operation failed for client {} after retries: {:?}",
                            client_id, e
                        );
                        addr.do_send(DiscoveryFailure(format!("{:?}", e)));
                    }
                }
            });
        } else {
            let addr = ctx.address();
            let retry_config = self.retry_config.clone();

            actix::spawn(async move {
                let result = retry_with_backoff(retry_config, || {
                    Box::pin(async {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        if fastrand::f32() < 0.1 {
                            Err::<(), McpError>(McpError("Random failure".to_string()))
                        } else {
                            Ok::<(), McpError>(())
                        }
                    })
                })
                .await;

                match result {
                    Ok(_) => addr.do_send(RequestDiscoveryData),
                    Err(e) => {
                        error!(
                            "Discovery fallback failed for client {}: {:?}",
                            client_id, e
                        );
                        addr.do_send(DiscoveryFailure(format!("{:?}", e)));
                    }
                }
            });
        }
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

    #[allow(dead_code)]
    fn should_send_message(
        &self,
        message_type: &str,
        _message_content: &serde_json::Value,
    ) -> bool {
        match message_type {
            "discovery" => true,
            "multi_agent_update" => true,
            "topology_update" => self.subscription_filters.include_topology,
            "neural_update" => self.subscription_filters.include_neural,
            "performance_analysis" => self.subscription_filters.include_performance,
            _ => true,
        }
    }

    #[allow(dead_code)]
    fn filter_agent_data(&self, data: &mut serde_json::Value) {
        if let Some(agents_array) = data.get_mut("agents").and_then(|a| a.as_array_mut()) {
            agents_array.retain(|agent| {
                if let Some(server_source) = agent.get("server_source") {
                    if let Ok(server_type) =
                        serde_json::from_value::<McpServerType>(server_source.clone())
                    {
                        return self
                            .subscription_filters
                            .server_types
                            .contains(&server_type);
                    }
                }
                false
            });
        }

        if !self.subscription_filters.agent_types.is_empty() {
            if let Some(agents_array) = data.get_mut("agents").and_then(|a| a.as_array_mut()) {
                agents_array.retain(|agent| {
                    if let Some(agent_type) = agent.get("agent_type").and_then(|t| t.as_str()) {
                        return self
                            .subscription_filters
                            .agent_types
                            .contains(&agent_type.to_string());
                    }
                    false
                });
            }
        }

        if !self.subscription_filters.swarm_ids.is_empty() {
            if let Some(agents_array) = data.get_mut("agents").and_then(|a| a.as_array_mut()) {
                agents_array.retain(|agent| {
                    if let Some(swarm_id) = agent.get("swarm_id").and_then(|s| s.as_str()) {
                        return self
                            .subscription_filters
                            .swarm_ids
                            .contains(&swarm_id.to_string());
                    }
                    false
                });
            }
        }
    }
}

impl Actor for MultiMcpVisualizationWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Multi-MCP WebSocket client {} connected", self.client_id);

        self.start_heartbeat(ctx);

        if let Some(health_manager) = &self.health_manager {
            let health_manager = health_manager.clone();
            let registrations = health_manager.clone();
            actix::spawn(async move {
                for (i, service) in MONITORED_SERVICES.iter().enumerate() {
                    let endpoint = ServiceEndpoint {
                        name: service.to_string(),
                        host: "localhost".to_string(),
                        port: 8080 + i as u16,
                        config: HealthCheckConfig::default(),
                        additional_endpoints: vec![],
                    };
                    registrations.register_service(endpoint).await;
                }
            });

            // ADR-2094: exactly one health task per connection, replacing both
            // the 30s `run_interval` spawn and the per-call spawn that
            // `has_healthy_services` used to leak.
            self.start_health_monitor(health_manager);
        }

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

        self.send_discovery_data(ctx);
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("Multi-MCP WebSocket client {} disconnected", self.client_id);
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

    fn handle(&mut self, _: RequestDiscoveryData, _ctx: &mut Self::Context) {
        debug!("Requesting discovery data for client {}", self.client_id);
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
    _hybrid_manager: Option<()>,
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

    ws::start(MultiMcpVisualizationWs::new(app_state, None), &req, stream)
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
}
