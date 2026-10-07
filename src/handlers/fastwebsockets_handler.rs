//! High-Performance WebSocket Handler using fastwebsockets
//!
//! This module provides a WebSocket fallback for clients that don't support QUIC/WebTransport.
//! Using Deno's fastwebsockets library which is 2.4x faster than tungstenite.
//!
//! Features:
//! - Zero-copy message handling
//! - Efficient frame parsing
//! - Compatible with existing binary protocol
//! - Automatic upgrade from HTTP

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use fastwebsockets::{FragmentCollector, Frame, OpCode, Payload, WebSocket, WebSocketError};
use http_body_util::Empty;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use log::{debug, error, info, trace, warn};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::app_state::AppState;
use crate::utils::binary_protocol;
use crate::utils::socket_flow_messages::BinaryNodeData;

use super::quic_transport_handler::{PostcardBatchUpdate, PostcardNodeUpdate};

// ============================================================================
// FASTWEBSOCKET SERVER
// ============================================================================

/// FastWebSockets server configuration
pub struct FastWebSocketConfig {
    pub bind_addr: SocketAddr,
    pub max_connections: usize,
    pub max_message_size: usize,
    pub ping_interval_ms: u64,
    pub pong_timeout_ms: u64,
}

impl Default for FastWebSocketConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 9001)),
            max_connections: 1000,
            max_message_size: 16 * 1024 * 1024, // 16 MB
            ping_interval_ms: 5000,
            pong_timeout_ms: 30000,
        }
    }
}

/// Client session for fastwebsockets
pub struct FastWsClientSession {
    pub session_id: String,
    pub remote_addr: SocketAddr,
    pub connected_at: Instant,
    pub last_activity: Instant,
    pub subscriptions: Vec<String>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    /// Use postcard for binary serialization (12 GB/s vs 2 GB/s JSON)
    pub use_postcard: bool,
}

/// High-performance WebSocket server using fastwebsockets
pub struct FastWebSocketServer {
    config: FastWebSocketConfig,
    app_state: Arc<AppState>,
    sessions: Arc<RwLock<HashMap<String, Arc<RwLock<FastWsClientSession>>>>>,
    /// Broadcast channel for position updates
    position_broadcast: broadcast::Sender<PostcardBatchUpdate>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl FastWebSocketServer {
    /// Create a new FastWebSocket server
    pub fn new(app_state: Arc<AppState>, config: FastWebSocketConfig) -> Self {
        let (position_broadcast, _) = broadcast::channel(100);

        Self {
            config,
            app_state,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            position_broadcast,
            shutdown_tx: None,
        }
    }

    /// Start the FastWebSocket server
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(self.config.bind_addr).await?;
        info!(
            "FastWebSocket server listening on {}",
            self.config.bind_addr
        );

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        let sessions = self.sessions.clone();
        let app_state = self.app_state.clone();
        let position_broadcast = self.position_broadcast.clone();
        let max_message_size = self.config.max_message_size;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok((stream, addr)) = listener.accept() => {
                        let sessions = sessions.clone();
                        let app_state = app_state.clone();
                        let position_broadcast = position_broadcast.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                addr,
                                sessions,
                                app_state,
                                position_broadcast,
                                max_message_size,
                            ).await {
                                error!("FastWebSocket connection error from {}: {}", addr, e);
                            }
                        });
                    }
                    _ = shutdown_rx.recv() => {
                        info!("FastWebSocket server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle incoming TCP connection and upgrade to WebSocket
    async fn handle_connection(
        stream: tokio::net::TcpStream,
        _addr: SocketAddr,
        _sessions: Arc<RwLock<HashMap<String, Arc<RwLock<FastWsClientSession>>>>>,
        _app_state: Arc<AppState>,
        position_broadcast: broadcast::Sender<PostcardBatchUpdate>,
        _max_message_size: usize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let broadcast_tx = Arc::new(position_broadcast);
        let broadcast_for_service = broadcast_tx.clone();

        // Hyper service for HTTP upgrade
        let service = service_fn(move |req: Request<Incoming>| {
            let broadcast_tx = broadcast_for_service.clone();
            async move { Self::upgrade_handler(req, broadcast_tx).await }
        });

        let io = TokioIo::new(stream);
        let conn = http1::Builder::new()
            .serve_connection(io, service)
            .with_upgrades();

        // Handle the connection
        if let Err(e) = conn.await {
            error!("HTTP connection error: {}", e);
        }

        Ok(())
    }

    /// Handle WebSocket upgrade
    async fn upgrade_handler(
        mut req: Request<Incoming>,
        position_broadcast: Arc<broadcast::Sender<PostcardBatchUpdate>>,
    ) -> Result<Response<Empty<Bytes>>, WebSocketError> {
        // SECURITY: Validate Origin header to prevent cross-site WebSocket hijacking
        if let Some(origin) = req.headers().get("origin") {
            let origin_str = origin.to_str().unwrap_or("");
            let allowed_origins = std::env::var("ALLOWED_WS_ORIGINS").unwrap_or_else(|_| {
                "http://localhost,https://localhost,http://127.0.0.1,https://127.0.0.1".to_string()
            });
            let is_allowed = allowed_origins
                .split(',')
                .any(|allowed| origin_str.starts_with(allowed.trim()));

            // Also allow same-host origins (proxied through nginx in Docker)
            // Nginx $host strips port, so compare hostnames only
            let is_same_host = if !is_allowed {
                if let Some(host) = req
                    .headers()
                    .get("host")
                    .or_else(|| req.headers().get("x-forwarded-host"))
                {
                    let host_str = host.to_str().unwrap_or("");
                    let origin_host = origin_str
                        .strip_prefix("http://")
                        .or_else(|| origin_str.strip_prefix("https://"))
                        .unwrap_or("");
                    let host_no_port = host_str.split(':').next().unwrap_or("");
                    let origin_no_port = origin_host.split(':').next().unwrap_or("");
                    !host_no_port.is_empty()
                        && !origin_no_port.is_empty()
                        && origin_no_port == host_no_port
                } else {
                    false
                }
            } else {
                false
            };

            if !is_allowed && !is_same_host {
                warn!(
                    "SECURITY: Rejected WebSocket upgrade from disallowed origin: {}",
                    origin_str
                );
                let mut resp = Response::new(Empty::new());
                *resp.status_mut() = hyper::StatusCode::FORBIDDEN;
                return Ok(resp);
            }
        }

        // SECURITY: Require authentication token before WebSocket upgrade.
        // ADR-2058: the Authorization header is the only accepted carrier in a
        // release build. The `?token=` query fallback leaks the bearer token into
        // access logs, proxy logs and Referer headers. It is compiled out of release
        // and survives only behind the dev-auth gate. Clients that cannot set headers
        // on an upgrade authenticate post-connect with the NIP-98 `authenticate`
        // envelope (kind 27235) — the path the XR client already uses
        // (`xr-client/rust/src/signer.rs:110-114`).
        let header_token = req
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        #[cfg(any(debug_assertions, feature = "dev-auth"))]
        let token = header_token.or_else(|| {
            let found = req.uri().query().and_then(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .find(|(k, _)| k == "token")
                    .map(|(_, v)| v.to_string())
            });
            if found.is_some() {
                warn!(
                    "SECURITY: fastwebsockets upgrade authenticated via ?token= query string — \
                     dev-only path (ADR-2058). Use the Authorization header."
                );
            }
            found
        });

        #[cfg(not(any(debug_assertions, feature = "dev-auth")))]
        let token = {
            if req.uri().query().is_some_and(|q| q.contains("token=")) {
                warn!(
                    "SECURITY: Rejecting ?token= query-string auth on the fastwebsockets \
                     endpoint — ADR-2058 requires the Authorization header in release builds"
                );
            }
            header_token
        };

        if token.as_deref().unwrap_or("").is_empty() {
            warn!(
                "SECURITY: Rejected unauthenticated WebSocket upgrade on fastwebsockets endpoint"
            );
            let mut resp = Response::new(Empty::new());
            *resp.status_mut() = hyper::StatusCode::UNAUTHORIZED;
            return Ok(resp);
        }

        // Check if it's a WebSocket upgrade request and perform upgrade
        let (response, fut) = fastwebsockets::upgrade::upgrade(&mut req)?;

        // Spawn the WebSocket handler with broadcast channel
        let mut broadcast_rx = position_broadcast.subscribe();
        tokio::spawn(async move {
            match fut.await {
                Ok(ws) => {
                    if let Err(e) = Self::handle_websocket(ws, &mut broadcast_rx).await {
                        error!("WebSocket handler error: {}", e);
                    }
                }
                Err(e) => {
                    error!("WebSocket upgrade failed: {}", e);
                }
            }
        });

        Ok(response)
    }

    /// Handle WebSocket connection after upgrade
    async fn handle_websocket<S>(
        ws: WebSocket<S>,
        broadcast_rx: &mut broadcast::Receiver<PostcardBatchUpdate>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let mut ws = FragmentCollector::new(ws);

        loop {
            tokio::select! {
                frame_result = ws.read_frame() => {
                    match frame_result {
                        Ok(frame) => {
                            match frame.opcode {
                                OpCode::Text => {
                                    // Handle text messages (JSON control frames)
                                    let payload = String::from_utf8_lossy(&frame.payload);
                                    debug!("Received text message: {}", payload);

                                    // Parse JSON and route through control handling
                                    match serde_json::from_str::<serde_json::Value>(&payload) {
                                        Ok(json) => {
                                            // Route based on message type
                                            let msg_type = json.get("type")
                                                .and_then(|t| t.as_str())
                                                .unwrap_or("unknown");
                                            debug!("Received control message type: {}", msg_type);
                                            // Acknowledge receipt
                                            let ack = serde_json::json!({"type": "ack", "status": "ok"});
                                            let ack_bytes = serde_json::to_vec(&ack).unwrap_or_default();
                                            let _ = ws.write_frame(Frame::text(Payload::Owned(ack_bytes))).await;
                                        }
                                        Err(_) => {
                                            warn!("Received non-JSON text message, ignoring");
                                        }
                                    }
                                }
                                OpCode::Binary => {
                                    // Handle binary messages (position updates from client)
                                    trace!("Received client binary: {} bytes", frame.payload.len());

                                    // Try postcard first, fall back to legacy protocol
                                    if let Ok(batch) = postcard::from_bytes::<PostcardBatchUpdate>(&frame.payload) {
                                        trace!("Decoded postcard batch: {} nodes", batch.nodes.len());
                                    } else if let Ok(nodes) = binary_protocol::decode_node_data(&frame.payload) {
                                        trace!("Decoded legacy binary: {} nodes", nodes.len());
                                    } else {
                                        warn!("Failed to decode client binary message ({} bytes)", frame.payload.len());
                                    }
                                }
                                OpCode::Close => {
                                    info!("WebSocket close received");
                                    return Ok(());
                                }
                                OpCode::Ping => {
                                    ws.write_frame(Frame::pong(frame.payload)).await?;
                                }
                                OpCode::Pong => {
                                    trace!("Pong received");
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            warn!("WebSocket read error: {}", e);
                            return Ok(());
                        }
                    }
                }
                Ok(batch) = broadcast_rx.recv() => {
                    // Broadcast position updates to this client
                    match postcard::to_stdvec(&batch) {
                        Ok(data) => {
                            if let Err(e) = ws.write_frame(Frame::binary(Payload::Owned(data))).await {
                                warn!("Failed to send broadcast frame: {}", e);
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            warn!("Failed to serialize broadcast: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Broadcast position update to all connected clients
    pub async fn broadcast_positions(&self, nodes: &[(u32, BinaryNodeData)]) {
        let updates: Vec<PostcardNodeUpdate> = nodes
            .iter()
            .map(|(_, data)| PostcardNodeUpdate::from(data))
            .collect();

        let now_ms = current_timestamp_ms();
        let batch = PostcardBatchUpdate {
            frame_id: now_ms,
            timestamp_ms: now_ms,
            nodes: updates,
        };

        if let Err(e) = self.position_broadcast.send(batch) {
            trace!("No subscribers for position broadcast: {}", e);
        }
    }

    /// Get active session count
    pub async fn active_sessions(&self) -> usize {
        self.sessions.read().await.len()
    }
}

/// Get current timestamp in milliseconds since UNIX epoch (safe, no panics)
#[inline]
fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// STANDALONE WEBSOCKET HANDLER (for actix integration)
// ============================================================================

/// Standalone WebSocket handler using fastwebsockets
/// Can be integrated with actix-web via raw socket handling
pub struct StandaloneFastWsHandler {
    app_state: Arc<AppState>,
    use_postcard: bool,
    last_activity: Instant,
    bytes_sent: u64,
    bytes_received: u64,
}

impl StandaloneFastWsHandler {
    pub fn new(app_state: Arc<AppState>, use_postcard: bool) -> Self {
        Self {
            app_state,
            use_postcard,
            last_activity: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
        }
    }

    /// Process incoming binary message
    pub fn process_binary(&mut self, data: &[u8]) -> Result<Vec<u8>, String> {
        self.last_activity = Instant::now();
        self.bytes_received += data.len() as u64;

        // Try postcard first for better performance
        if self.use_postcard {
            if let Ok(batch) = postcard::from_bytes::<PostcardBatchUpdate>(data) {
                debug!("Processed postcard batch: {} nodes", batch.nodes.len());
                return Ok(vec![]); // Acknowledge
            }
        }

        // Fall back to legacy binary protocol
        match binary_protocol::decode_node_data(data) {
            Ok(nodes) => {
                debug!("Processed legacy binary: {} nodes", nodes.len());
                Ok(vec![])
            }
            Err(e) => Err(format!("Failed to decode binary: {}", e)),
        }
    }

    /// Encode nodes for transmission
    pub fn encode_nodes(&mut self, nodes: &[(u32, BinaryNodeData)]) -> Vec<u8> {
        let data = if self.use_postcard {
            let updates: Vec<PostcardNodeUpdate> = nodes
                .iter()
                .map(|(_, data)| PostcardNodeUpdate::from(data))
                .collect();

            let batch = PostcardBatchUpdate {
                frame_id: 0,
                timestamp_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                nodes: updates,
            };

            postcard::to_stdvec(&batch).unwrap_or_else(|_| {
                // Fall back to legacy if postcard fails
                let analytics = self.app_state.node_analytics.read().ok();
                let analytics_ref = analytics.as_deref();
                let sssp = self.app_state.node_sssp.read().ok();
                let sssp_ref = sssp.as_deref();
                binary_protocol::encode_node_data_with_live_analytics(
                    nodes,
                    analytics_ref,
                    sssp_ref,
                )
            })
        } else {
            let analytics = self.app_state.node_analytics.read().ok();
            let analytics_ref = analytics.as_deref();
            let sssp = self.app_state.node_sssp.read().ok();
            let sssp_ref = sssp.as_deref();
            binary_protocol::encode_node_data_with_live_analytics(nodes, analytics_ref, sssp_ref)
        };

        self.bytes_sent += data.len() as u64;
        data
    }

    /// Get session statistics
    pub fn stats(&self) -> (u64, u64, Duration) {
        (
            self.bytes_sent,
            self.bytes_received,
            self.last_activity.elapsed(),
        )
    }
}

// ============================================================================
// PROTOCOL NEGOTIATION
// ============================================================================

/// Supported transport protocols
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportProtocol {
    /// QUIC with WebTransport (preferred)
    QuicWebTransport,
    /// fastwebsockets with postcard (fast fallback)
    FastWebSocketPostcard,
    /// Legacy WebSocket with binary protocol (compatibility)
    LegacyWebSocket,
}

/// Protocol negotiation result
pub struct NegotiatedProtocol {
    pub protocol: TransportProtocol,
    pub serialization: SerializationFormat,
    pub supports_datagrams: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SerializationFormat {
    Postcard,
    LegacyBinary,
    Json,
}

/// Negotiate best protocol based on client capabilities
pub fn negotiate_protocol(client_capabilities: &[String]) -> NegotiatedProtocol {
    // Check for QUIC/WebTransport support
    if client_capabilities.contains(&"webtransport".to_string()) {
        return NegotiatedProtocol {
            protocol: TransportProtocol::QuicWebTransport,
            serialization: SerializationFormat::Postcard,
            supports_datagrams: true,
        };
    }

    // Check for postcard support
    if client_capabilities.contains(&"postcard".to_string()) {
        return NegotiatedProtocol {
            protocol: TransportProtocol::FastWebSocketPostcard,
            serialization: SerializationFormat::Postcard,
            supports_datagrams: false,
        };
    }

    // Fall back to legacy
    NegotiatedProtocol {
        protocol: TransportProtocol::LegacyWebSocket,
        serialization: SerializationFormat::LegacyBinary,
        supports_datagrams: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_negotiation_quic() {
        let caps = vec!["webtransport".to_string(), "postcard".to_string()];
        let result = negotiate_protocol(&caps);
        assert_eq!(result.protocol, TransportProtocol::QuicWebTransport);
        assert!(result.supports_datagrams);
    }

    #[test]
    fn test_protocol_negotiation_postcard() {
        let caps = vec!["postcard".to_string()];
        let result = negotiate_protocol(&caps);
        assert_eq!(result.protocol, TransportProtocol::FastWebSocketPostcard);
        assert!(!result.supports_datagrams);
    }

    #[test]
    fn test_protocol_negotiation_legacy() {
        let caps = vec![];
        let result = negotiate_protocol(&caps);
        assert_eq!(result.protocol, TransportProtocol::LegacyWebSocket);
    }

    // Test disabled - AppState::default_mock() no longer exists
    // #[test]
    // fn test_standalone_handler_encoding() {
    //     let app_state = Arc::new(AppState::default_mock());
    //     let mut handler = StandaloneFastWsHandler::new(app_state, true);
    //
    //     let nodes = vec![(
    //         1u32,
    //         BinaryNodeData {
    //             node_id: 1,
    //             x: 1.0,
    //             y: 2.0,
    //             z: 3.0,
    //             vx: 0.1,
    //             vy: 0.2,
    //             vz: 0.3,
    //         },
    //     )];
    //
    //     let encoded = handler.encode_nodes(&nodes);
    //     assert!(!encoded.is_empty());
    //
    //     let (sent, received, _) = handler.stats();
    //     assert!(sent > 0);
    //     assert_eq!(received, 0);
    // }
}
