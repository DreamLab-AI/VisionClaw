use crate::utils::network::{
    CircuitBreaker, HealthCheckConfig, HealthCheckManager, ServiceEndpoint, TimeoutConfig,
};
use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, StreamHandler};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use log::{debug, error, info, warn};
use serde_json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};

#[derive(Message)]
#[rtype(result = "()")]
struct OrchestratorText(String);

#[derive(Message)]
#[rtype(result = "()")]
struct OrchestratorBinary(Vec<u8>);

pub struct MCPRelayActor {
    client_id: String,
    orchestrator_tx: Option<
        Arc<
            Mutex<
                SplitSink<
                    tokio_tungstenite::WebSocketStream<
                        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                    >,
                    TungsteniteMessage,
                >,
            >,
        >,
    >,
    self_addr: Option<Addr<Self>>,
    
    circuit_breaker: Arc<CircuitBreaker>,
    health_manager: Arc<HealthCheckManager>,
    timeout_config: TimeoutConfig,
    connection_attempts: u32,
    last_health_check: Instant,
    is_orchestrator_healthy: bool,
}

impl MCPRelayActor {
    fn new() -> Self {
        let client_id = uuid::Uuid::new_v4().to_string();
        let circuit_breaker = Arc::new(CircuitBreaker::mcp_operations());
        let health_manager = Arc::new(HealthCheckManager::new());
        let timeout_config = TimeoutConfig::mcp_operations();

        

        info!(
            "[MCP Relay] Creating new actor with resilience features: {}",
            client_id
        );

        Self {
            client_id,
            orchestrator_tx: None,
            self_addr: None,
            circuit_breaker,
            health_manager,
            timeout_config,
            connection_attempts: 0,
            last_health_check: Instant::now(),
            is_orchestrator_healthy: true,
        }
    }

    fn connect_to_orchestrator(&mut self, ctx: &mut <Self as Actor>::Context) {
        let orchestrator_url = std::env::var("ORCHESTRATOR_WS_URL")
            .unwrap_or_else(|_| "ws://multi-agent-container:3002/ws".to_string());

        self.connection_attempts += 1;
        info!(
            "[MCP Relay] Connecting to orchestrator at: {} (attempt {})",
            orchestrator_url, self.connection_attempts
        );

        let addr = ctx.address();
        self.self_addr = Some(addr.clone());
        let circuit_breaker = self.circuit_breaker.clone();
        let health_manager = self.health_manager.clone();
        let timeout_config = self.timeout_config.clone();
        let connection_attempts = self.connection_attempts;

        actix::spawn(async move {
            
            let connection_result = circuit_breaker
                .execute(async {
                    let conn_timeout = timeout_config.connect_timeout;
                    match tokio::time::timeout(
                        conn_timeout,
                        connect_async(orchestrator_url.as_str()),
                    )
                    .await
                    {
                        Ok(Ok(stream)) => Ok(stream),
                        Ok(Err(e)) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
                        Err(_) => Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "Connection timeout",
                        ))
                            as Box<dyn std::error::Error + Send + Sync>),
                    }
                })
                .await;

            match connection_result {
                Ok((ws_stream, _)) => {
                    info!(
                        "[MCP Relay] Connected to orchestrator on attempt {}",
                        connection_attempts
                    );
                    let (tx, mut rx) = ws_stream.split();
                    let tx = Arc::new(Mutex::new(tx));

                    
                    let _health_check_result =
                        health_manager.check_service_now("orchestrator").await;
                    debug!("[MCP Relay] Health check performed for orchestrator");

                    
                    addr.do_send(OrchestratorText("connected".to_string()));

                    
                    while let Some(msg) = rx.next().await {
                        match msg {
                            Ok(TungsteniteMessage::Text(text)) => {
                                addr.do_send(OrchestratorText(text));
                            }
                            Ok(TungsteniteMessage::Binary(bin)) => {
                                addr.do_send(OrchestratorBinary(bin));
                            }
                            Ok(TungsteniteMessage::Close(_)) => {
                                info!("[MCP Relay] Orchestrator connection closed");
                                break;
                            }
                            Ok(TungsteniteMessage::Ping(data)) => {
                                let tx_clone = tx.clone();
                                let health_manager_clone = health_manager.clone();
                                actix::spawn(async move {
                                    
                                    let mut tx_guard = tx_clone.lock().await;
                                    match tx_guard.send(TungsteniteMessage::Pong(data)).await {
                                        Err(e) => {
                                            error!("[MCP Relay] Failed to send pong: {}", e);
                                            let _ = health_manager_clone
                                                .check_service_now("orchestrator")
                                                .await;
                                        }
                                        _ => {
                                            let _ = health_manager_clone
                                                .check_service_now("orchestrator")
                                                .await;
                                        }
                                    }
                                });
                            }
                            Ok(TungsteniteMessage::Pong(_)) => {
                                
                            }
                            Ok(_) => {}
                            Err(e) => {
                                error!("[MCP Relay] Error receiving from orchestrator: {}", e);
                                
                                let _ = health_manager.check_service_now("orchestrator").await;
                                break;
                            }
                        }
                    }

                    info!("[MCP Relay] Orchestrator connection handler ended");
                }
                Err(e) => {
                    error!(
                        "[MCP Relay] Failed to connect to orchestrator on attempt {}: {:?}",
                        connection_attempts, e
                    );

                    
                    let _ = health_manager.check_service_now("orchestrator").await;

                    
                    let retry_delay = std::cmp::min(
                        Duration::from_secs(5) * 2_u32.pow(connection_attempts.saturating_sub(1)),
                        Duration::from_secs(60),
                    );

                    info!("[MCP Relay] Retrying connection in {:?}", retry_delay);
                    tokio::time::sleep(retry_delay).await;
                    addr.do_send(OrchestratorText("retry".to_string()));
                }
            }
        });
    }
}

impl Actor for MCPRelayActor {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(
            "[MCP Relay] Actor started for client: {} with resilience features",
            self.client_id
        );

        
        let health_manager = self.health_manager.clone();
        actix::spawn(async move {
            let endpoint = ServiceEndpoint {
                name: "orchestrator".to_string(),
                host: "localhost".to_string(),
                port: 8080, 
                config: HealthCheckConfig::default(),
                additional_endpoints: vec![],
            };
            health_manager.register_service(endpoint).await;
        });

        
        ctx.run_interval(Duration::from_secs(30), |act, ctx| {
            ctx.ping(b"");

            
            let health_manager = act.health_manager.clone();
            actix::spawn(async move {
                let health_result = health_manager.check_service_now("orchestrator").await;

                if health_result.is_none() || !health_result.map_or(false, |r| r.status.is_usable())
                {
                    warn!("[MCP Relay] Orchestrator health check failed");
                }
            });
        });

        
        ctx.run_interval(Duration::from_secs(60), |act, _ctx| {
            act.last_health_check = Instant::now();
            
            let health_manager = act.health_manager.clone();
            actix::spawn(async move {
                let _health = health_manager.get_service_health("orchestrator").await;
            });

            let circuit_breaker = act.circuit_breaker.clone();
            actix::spawn(async move {
                let stats = circuit_breaker.stats().await;
                debug!(
                    "[MCP Relay] Circuit breaker stats - State: {:?}, Failures: {}, Successes: {}",
                    stats.state, stats.failed_requests, stats.successful_requests
                );
            });
        });

        
        self.connect_to_orchestrator(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("[MCP Relay] Actor stopped for client: {}", self.client_id);
    }
}

// Handle messages from orchestrator
impl Handler<OrchestratorText> for MCPRelayActor {
    type Result = ();

    fn handle(&mut self, msg: OrchestratorText, ctx: &mut Self::Context) {
        match msg.0.as_str() {
            "connected" => {
                
                ctx.text(
                    serde_json::json!({
                        "type": "orchestrator_connected",
                        "timestamp": chrono::Utc::now().timestamp_millis()
                    })
                    .to_string(),
                );
            }
            "retry" => {
                
                self.connect_to_orchestrator(ctx);
            }
            _ => {
                
                ctx.text(msg.0);
            }
        }
    }
}

impl Handler<OrchestratorBinary> for MCPRelayActor {
    type Result = ();

    fn handle(&mut self, msg: OrchestratorBinary, ctx: &mut Self::Context) {
        
        ctx.binary(msg.0);
    }
}

// WebSocket stream handler for client messages
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MCPRelayActor {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                
            }
            Ok(ws::Message::Text(text)) => {
                debug!("[MCP Relay] Received text from client: {}", text);

                
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                    
                    if let Some(msg_type) = msg.get("type").and_then(|t| t.as_str()) {
                        match msg_type {
                            "ping" => {
                                ctx.text(
                                    serde_json::json!({
                                        "type": "pong",
                                        "timestamp": chrono::Utc::now().timestamp_millis()
                                    })
                                    .to_string(),
                                );
                                return;
                            }
                            _ => {}
                        }
                    }
                }

                
                if let Some(tx) = &self.orchestrator_tx {
                    if !self.is_orchestrator_healthy {
                        warn!("[MCP Relay] Orchestrator unhealthy, dropping message");
                        ctx.text(
                            serde_json::json!({
                                "type": "error",
                                "message": "Orchestrator unhealthy",
                                "timestamp": chrono::Utc::now().timestamp_millis()
                            })
                            .to_string(),
                        );
                        return;
                    }

                    let tx = tx.clone();
                    let text_clone = text.to_string();
                    let health_manager = self.health_manager.clone();

                    actix::spawn(async move {
                        let mut tx_guard = tx.lock().await;
                        match tokio::time::timeout(
                            Duration::from_secs(5),
                            tx_guard.send(TungsteniteMessage::Text(text_clone)),
                        )
                        .await
                        {
                            Ok(Ok(_)) => {
                                
                                let _ = health_manager.check_service_now("orchestrator").await;
                            }
                            Ok(Err(e)) => {
                                error!("[MCP Relay] Failed to send to orchestrator: {}", e);
                                let _ = health_manager.check_service_now("orchestrator").await;
                            }
                            Err(_) => {
                                error!("[MCP Relay] Timeout sending to orchestrator");
                                let _ = health_manager.check_service_now("orchestrator").await;
                            }
                        }
                    });
                } else {
                    warn!("[MCP Relay] Client message received but orchestrator not connected");
                    ctx.text(
                        serde_json::json!({
                            "type": "error",
                            "message": "Orchestrator not connected",
                            "timestamp": chrono::Utc::now().timestamp_millis()
                        })
                        .to_string(),
                    );
                }
            }
            Ok(ws::Message::Binary(bin)) => {
                debug!(
                    "[MCP Relay] Received binary from client: {} bytes",
                    bin.len()
                );

                
                if let Some(tx) = &self.orchestrator_tx {
                    if !self.is_orchestrator_healthy {
                        warn!("[MCP Relay] Orchestrator unhealthy, dropping binary message");
                        return;
                    }

                    let tx = tx.clone();
                    let bin_vec = bin.to_vec();
                    let health_manager = self.health_manager.clone();

                    actix::spawn(async move {
                        let mut tx_guard = tx.lock().await;
                        match tokio::time::timeout(
                            Duration::from_secs(5),
                            tx_guard.send(TungsteniteMessage::Binary(bin_vec)),
                        )
                        .await
                        {
                            Ok(Ok(_)) => {
                                let _ = health_manager.check_service_now("orchestrator").await;
                            }
                            Ok(Err(e)) => {
                                error!("[MCP Relay] Failed to send binary to orchestrator: {}", e);
                                let _ = health_manager.check_service_now("orchestrator").await;
                            }
                            Err(_) => {
                                error!("[MCP Relay] Timeout sending binary to orchestrator");
                                let _ = health_manager.check_service_now("orchestrator").await;
                            }
                        }
                    });
                }
            }
            Ok(ws::Message::Close(reason)) => {
                info!("[MCP Relay] Client closed connection: {:?}", reason);
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) => {
                ctx.stop();
            }
            Ok(ws::Message::Nop) => {}
            Err(e) => {
                error!("[MCP Relay] WebSocket error: {}", e);
                ctx.stop();
            }
        }
    }
}

pub async fn mcp_relay_handler(
    req: HttpRequest,
    stream: web::Payload,
) -> Result<HttpResponse, actix_web::Error> {
    info!("[MCP Relay] New WebSocket connection request");
    ws::start(MCPRelayActor::new(), &req, stream)
}
