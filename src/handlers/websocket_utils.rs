//! WebSocket Utilities Module
//!
//! Provides common utilities for WebSocket handlers to eliminate duplicate code
//! across multiple WebSocket implementations.

use actix::prelude::*;
use actix_web_actors::ws;
use log::{debug, error, info, warn};
use serde::{de::DeserializeOwned, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Standard WebSocket message wrapper with common fields
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebSocketMessage<T> {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub data: T,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl<T> WebSocketMessage<T> {
    pub fn new(msg_type: String, data: T) -> Self {
        Self {
            msg_type,
            data,
            timestamp: current_timestamp(),
            client_id: None,
            session_id: None,
        }
    }

    pub fn with_client_id(mut self, client_id: String) -> Self {
        self.client_id = Some(client_id);
        self
    }

    pub fn with_session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

/// WebSocket connection metrics for tracking performance
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebSocketMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub errors_count: u64,
    pub connection_time: u64,
}

impl WebSocketMetrics {
    pub fn new() -> Self {
        Self {
            connection_time: current_timestamp(),
            ..Default::default()
        }
    }

    pub fn record_sent(&mut self, bytes: usize) {
        self.messages_sent += 1;
        self.bytes_sent += bytes as u64;
    }

    pub fn record_received(&mut self, bytes: usize) {
        self.messages_received += 1;
        self.bytes_received += bytes as u64;
    }

    pub fn record_error(&mut self) {
        self.errors_count += 1;
    }

    pub fn uptime_seconds(&self) -> u64 {
        current_timestamp().saturating_sub(self.connection_time) / 1000
    }
}

/// WebSocket connection wrapper for standard operations
pub struct WebSocketConnection {
    client_id: String,
    session_id: String,
    heartbeat: Instant,
    metrics: WebSocketMetrics,
}

impl WebSocketConnection {
    pub fn new() -> Self {
        Self {
            client_id: Uuid::new_v4().to_string(),
            session_id: Uuid::new_v4().to_string(),
            heartbeat: Instant::now(),
            metrics: WebSocketMetrics::new(),
        }
    }

    pub fn with_client_id(client_id: String) -> Self {
        Self {
            client_id: client_id.clone(),
            session_id: Uuid::new_v4().to_string(),
            heartbeat: Instant::now(),
            metrics: WebSocketMetrics::new(),
        }
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn metrics(&self) -> &WebSocketMetrics {
        &self.metrics
    }

    pub fn update_heartbeat(&mut self) {
        self.heartbeat = Instant::now();
    }

    pub fn time_since_heartbeat(&self) -> Duration {
        Instant::now().duration_since(self.heartbeat)
    }

    pub fn is_heartbeat_timeout(&self, timeout: Duration) -> bool {
        self.time_since_heartbeat() > timeout
    }

    /// Send JSON message with automatic serialization and metrics tracking
    pub fn send_json<T, A>(
        &mut self,
        ctx: &mut ws::WebsocketContext<A>,
        message: &WebSocketMessage<T>,
    ) where
        T: Serialize,
        A: Actor<Context = ws::WebsocketContext<A>>,
    {
        match serde_json::to_string(message) {
            Ok(json_str) => {
                let bytes = json_str.len();
                ctx.text(json_str);
                self.metrics.record_sent(bytes);

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "[WebSocket] Sent message type '{}' to client {} ({} bytes)",
                        message.msg_type, self.client_id, bytes
                    );
                }
            }
            Err(e) => {
                error!(
                    "[WebSocket] Failed to serialize message for client {}: {}",
                    self.client_id, e
                );
                self.metrics.record_error();
            }
        }
    }

    /// Send binary data with metrics tracking
    pub fn send_binary<A>(&mut self, ctx: &mut ws::WebsocketContext<A>, data: Vec<u8>)
    where
        A: Actor<Context = ws::WebsocketContext<A>>,
    {
        let bytes = data.len();
        ctx.binary(data);
        self.metrics.record_sent(bytes);

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "[WebSocket] Sent binary data to client {} ({} bytes)",
                self.client_id, bytes
            );
        }
    }

    /// Send error response with standard format
    pub fn send_error<A>(&mut self, ctx: &mut ws::WebsocketContext<A>, error_message: &str)
    where
        A: Actor<Context = ws::WebsocketContext<A>>,
    {
        let error_response = serde_json::json!({
            "type": "error",
            "message": error_message,
            "client_id": self.client_id,
            "timestamp": current_timestamp(),
            "recoverable": true
        });

        match serde_json::to_string(&error_response) {
            Ok(json_str) => {
                let bytes = json_str.len();
                ctx.text(json_str);
                self.metrics.record_sent(bytes);
                self.metrics.record_error();

                warn!(
                    "[WebSocket] Sent error to client {}: {}",
                    self.client_id, error_message
                );
            }
            Err(e) => {
                error!(
                    "[WebSocket] Failed to send error message to client {}: {}",
                    self.client_id, e
                );
            }
        }
    }

    /// Send welcome/connected message
    pub fn send_welcome<A>(&mut self, ctx: &mut ws::WebsocketContext<A>, features: Vec<&str>)
    where
        A: Actor<Context = ws::WebsocketContext<A>>,
    {
        let welcome = serde_json::json!({
            "type": "connection_established",
            "client_id": self.client_id,
            "session_id": self.session_id,
            "server_time": current_timestamp(),
            "features": features
        });

        match serde_json::to_string(&welcome) {
            Ok(json_str) => {
                let bytes = json_str.len();
                ctx.text(json_str);
                self.metrics.record_sent(bytes);

                info!(
                    "[WebSocket] Client {} connected with session {}",
                    self.client_id, self.session_id
                );
            }
            Err(e) => {
                error!(
                    "[WebSocket] Failed to send welcome message to client {}: {}",
                    self.client_id, e
                );
            }
        }
    }

    /// Handle standard ping message
    pub fn handle_ping<A>(&mut self, ctx: &mut ws::WebsocketContext<A>, msg: &[u8])
    where
        A: Actor<Context = ws::WebsocketContext<A>>,
    {
        self.update_heartbeat();
        ctx.pong(msg);

        if log::log_enabled!(log::Level::Trace) {
            debug!("[WebSocket] Pong sent to client {}", self.client_id);
        }
    }

    /// Handle standard pong message
    pub fn handle_pong(&mut self) {
        self.update_heartbeat();

        if log::log_enabled!(log::Level::Trace) {
            debug!("[WebSocket] Pong received from client {}", self.client_id);
        }
    }

    /// Record received text message
    pub fn record_text_received(&mut self, text: &str) {
        self.metrics.record_received(text.len());
        self.update_heartbeat();
    }

    /// Record received binary message
    pub fn record_binary_received(&mut self, data: &[u8]) {
        self.metrics.record_received(data.len());
        self.update_heartbeat();
    }
}

impl Default for WebSocketConnection {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse JSON message from WebSocket text
pub fn parse_message<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| format!("Failed to parse WebSocket message: {}", e))
}

/// Parse typed WebSocket message
pub fn parse_typed_message<T: DeserializeOwned>(text: &str) -> Result<WebSocketMessage<T>, String> {
    serde_json::from_str(text).map_err(|e| format!("Failed to parse typed message: {}", e))
}

/// Get current timestamp in milliseconds
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Close WebSocket with error message
pub fn close_with_error<A>(ctx: &mut ws::WebsocketContext<A>, error_message: &str, client_id: &str)
where
    A: Actor<Context = ws::WebsocketContext<A>>,
{
    error!(
        "[WebSocket] Closing connection for client {} with error: {}",
        client_id, error_message
    );

    let close_reason = ws::CloseReason {
        code: ws::CloseCode::Error,
        description: Some(error_message.to_string()),
    };

    ctx.close(Some(close_reason));
    ctx.stop();
}

/// Handle WebSocket protocol error
pub fn handle_protocol_error<A>(
    ctx: &mut ws::WebsocketContext<A>,
    error: &ws::ProtocolError,
    client_id: &str,
) where
    A: Actor<Context = ws::WebsocketContext<A>>,
{
    error!(
        "[WebSocket] Protocol error for client {}: {}",
        client_id, error
    );

    // Send error message before closing
    let error_msg = serde_json::json!({
        "type": "error",
        "message": format!("WebSocket protocol error: {}", error),
        "recoverable": false
    });

    if let Ok(msg_str) = serde_json::to_string(&error_msg) {
        ctx.text(msg_str);
    }

    ctx.stop();
}

/// Setup standard heartbeat interval
pub fn setup_heartbeat<A, F>(ctx: &mut ws::WebsocketContext<A>, interval: Duration, mut check_fn: F)
where
    A: Actor<Context = ws::WebsocketContext<A>>,
    F: FnMut(&mut A, &mut ws::WebsocketContext<A>) + 'static,
{
    ctx.run_interval(interval, move |act, ctx| {
        check_fn(act, ctx);
    });
}

/// Setup standard ping interval
pub fn setup_ping_interval<A>(ctx: &mut ws::WebsocketContext<A>, interval: Duration)
where
    A: Actor<Context = ws::WebsocketContext<A>>,
{
    ctx.run_interval(interval, |_act, ctx| {
        ctx.ping(b"");
    });
}

/// Standard heartbeat timeout duration (120 seconds)
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(120);

/// Standard heartbeat check interval (30 seconds)
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Standard ping interval (5 seconds)
pub const PING_INTERVAL: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_connection_creation() {
        let conn = WebSocketConnection::new();
        assert!(!conn.client_id().is_empty());
        assert!(!conn.session_id().is_empty());
    }

    #[test]
    fn test_websocket_connection_with_client_id() {
        let client_id = "test-client-123".to_string();
        let conn = WebSocketConnection::with_client_id(client_id.clone());
        assert_eq!(conn.client_id(), client_id);
    }

    #[test]
    fn test_heartbeat_tracking() {
        let mut conn = WebSocketConnection::new();
        std::thread::sleep(Duration::from_millis(100));

        assert!(conn.time_since_heartbeat() >= Duration::from_millis(100));

        conn.update_heartbeat();
        assert!(conn.time_since_heartbeat() < Duration::from_millis(50));
    }

    #[test]
    fn test_heartbeat_timeout() {
        let conn = WebSocketConnection::new();
        assert!(!conn.is_heartbeat_timeout(Duration::from_secs(1)));
    }

    #[test]
    fn test_metrics_tracking() {
        let mut metrics = WebSocketMetrics::new();

        metrics.record_sent(100);
        assert_eq!(metrics.messages_sent, 1);
        assert_eq!(metrics.bytes_sent, 100);

        metrics.record_received(200);
        assert_eq!(metrics.messages_received, 1);
        assert_eq!(metrics.bytes_received, 200);

        metrics.record_error();
        assert_eq!(metrics.errors_count, 1);
    }

    #[test]
    fn test_websocket_message_creation() {
        let msg = WebSocketMessage::new("test".to_string(), "data".to_string())
            .with_client_id("client-123".to_string())
            .with_session_id("session-456".to_string());

        assert_eq!(msg.msg_type, "test");
        assert_eq!(msg.data, "data");
        assert_eq!(msg.client_id, Some("client-123".to_string()));
        assert_eq!(msg.session_id, Some("session-456".to_string()));
    }

    #[test]
    fn test_parse_message() {
        #[derive(serde::Deserialize)]
        struct TestData {
            value: String,
        }

        let json = r#"{"value": "test"}"#;
        let result: Result<TestData, _> = parse_message(json);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().value, "test");
    }

    #[test]
    fn test_parse_message_invalid() {
        #[derive(serde::Deserialize)]
        struct TestData {
            #[serde(rename = "value")]
            _value: String,
        }

        let json = r#"{"invalid": "data"}"#;
        let result: Result<TestData, _> = parse_message(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_current_timestamp() {
        let timestamp = current_timestamp();
        assert!(timestamp > 0);

        // Timestamp should be reasonable (after 2020)
        assert!(timestamp > 1577836800000); // Jan 1, 2020 in milliseconds
    }
}
