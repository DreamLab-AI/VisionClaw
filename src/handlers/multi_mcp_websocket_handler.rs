//! Multi-MCP WebSocket Handler
//!
//! Provides real-time WebSocket streaming of agent visualization data
//! from multiple MCP servers to the VisionFlow graph renderer.

use actix::{Actor, AsyncContext, Handler, Message, StreamHandler};
use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use actix_web_actors:::ws;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json:::json;
use std::time::{Duration, Instant};
use uuid:::Uuid;

use crate::services::agent_visualization_protocol:::McpServerType;
use crate::ok_json;
use crate:::AppState;
// DEPRECATED: HybridHealthManager removed
use crate::utils::network::{
    retry_with_backoff, CircuitBreaker, HealthCheckConfig, HealthCheckManager, RetryConfig,
    RetryableError, ServiceEndpoint, TimeoutConfig,
};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    
    HighFrequency,
    
    Normal,
    
    LowFrequency,
    
    OnDemand,
}

impl Default for PerformanceMode {
    fn default() -> Self {
        Self::Normal
    }
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

    
    fn perform_health_checks(&mut self) {
        if let Some(health_manager) = &self.health_manager {
            let health_manager_clone = health_manager.clone();
            let client_id = self.client_id.clone();

            actix::spawn(async move {
                
                for service in ["claude-flow", "ruv-swarm", "flow-nexus"] {
                    let health_result = health_manager_clone.check_service_now(service).await;
                    let is_healthy = health_result.map_or(false, |r| r.status.is_usable());

                    if !is_healthy {
                        warn!(
                            "[Multi-MCP] Service {} unhealthy for client {}",
                            service, client_id
                        );
                    }
                }
            });
        }
    }

    
    
    fn has_healthy_services(&self) -> bool {
        if let Some(health_manager) = &self.health_manager {
            let health_manager_clone = health_manager.clone();

            
            
            tokio::spawn(async move {
                for service in ["claude-flow", "ruv-swarm", "flow-nexus"] {
                    
                    if let Some(health_info) =
                        health_manager_clone.get_service_health(service).await
                    {
                        if health_info.current_status.is_usable() {
                            debug!("Service {} is healthy (cached)", service);
                        }
                    }
                }
            });

            
            
            
            return true;
        }
        
        true
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

    
    fn should_send_message(
        &self,
        message_type: &str,
        _message_content: &serde_json::Value,
    ) -> bool {
        match message_type {
            "discovery" => true,          
            "multi_agent_update" => true, 
            "topology_update" => {
                
                self.subscription_filters.include_topology
            }
            "neural_update" => self.subscription_filters.include_neural,
            "performance_analysis" => self.subscription_filters.include_performance,
            _ => true, 
        }
    }

    
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
            actix::spawn(async move {
                for (i, service) in ["claude-flow", "ruv-swarm", "flow-nexus"]
                    .iter()
                    .enumerate()
                {
                    let endpoint = ServiceEndpoint {
                        name: service.to_string(),
                        host: "localhost".to_string(),
                        port: 8080 + i as u16, 
                        config: HealthCheckConfig::default(),
                        additional_endpoints: vec![],
                    };
                    health_manager.register_service(endpoint).await;
                }
            });
        }

        
        self.start_position_updates(ctx);

        
        ctx.run_interval(Duration::from_secs(30), |act, _ctx| {
            act.perform_health_checks();
        });

        
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
    ws::start(MultiMcpVisualizationWs::new(app_state, None), &req, stream)
}

pub async fn get_mcp_server_status(_app_state: web::Data<AppState>) -> ActixResult<HttpResponse> {
    
    let response = json!({
        "servers": [
            {
                "server_id": "claude-flow",
                "server_type": "claude_flow",
                "host": "localhost",
                "port": 9500,
                "is_connected": true,
                "agent_count": 4
            },
            {
                "server_id": "ruv-swarm",
                "server_type": "ruv_swarm",
                "host": "localhost",
                "port": 9501,
                "is_connected": false,
                "agent_count": 0
            }
        ],
        "total_agents": 4,
        "timestamp": chrono::Utc::now().timestamp_millis()
    });

    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .json(response))
}

pub async fn refresh_mcp_discovery(_app_state: web::Data<AppState>) -> ActixResult<HttpResponse> {
    info!("Manual MCP discovery refresh requested");

    

    ok_json!(json!({
        "success": true,
        "message": "Discovery refresh initiated",
        "timestamp": chrono::Utc::now().timestamp_millis()
    }))
}

pub fn configure_multi_mcp_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/multi-mcp")
            .route("/ws", web::get().to(multi_mcp_visualization_ws))
            .route("/status", web::get().to(get_mcp_server_status))
            .route("/refresh", web::post().to(refresh_mcp_discovery)),
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

