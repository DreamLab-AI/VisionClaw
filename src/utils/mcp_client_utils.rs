//! Unified MCP Client Utilities
//!
//! This module provides consolidated MCP (Model Context Protocol) client functionality
//! with connection pooling, retry logic, timeout handling, and protocol negotiation.
//!
//! Consolidates duplicate patterns from:
//! - src/utils/mcp_tcp_client.rs (898 lines)
//! - src/client/mcp_tcp_client.rs (370 lines)
//! - src/utils/mcp_connection.rs (449 lines)
//! - src/services/mcp_relay_manager.rs (311 lines)

use log::{debug, error, info, warn};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;


// ============================================================================
// Core MCP Types
// ============================================================================

/// MCP JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
    pub id: u64,
}

/// MCP JSON-RPC 2.0 Response
#[derive(Debug, Clone, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
    pub id: u64,
}

/// MCP Error Structure
#[derive(Debug, Clone, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MCP Error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for McpError {}

// ============================================================================
// Connection Configuration
// ============================================================================

/// Configuration for MCP client connections
#[derive(Debug, Clone)]
pub struct McpConnectionConfig {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub protocol_version: String,
    pub client_name: String,
    pub client_version: String,
}

impl Default for McpConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 9500,
            timeout: Duration::from_secs(10),
            max_retries: 3,
            retry_delay: Duration::from_millis(500),
            protocol_version: "2024-11-05".to_string(),
            client_name: "visionflow-mcp-client".to_string(),
            client_version: "1.0.0".to_string(),
        }
    }
}

impl McpConnectionConfig {
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            ..Default::default()
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_retry_config(mut self, max_retries: u32, retry_delay: Duration) -> Self {
        self.max_retries = max_retries;
        self.retry_delay = retry_delay;
        self
    }

    pub fn with_client_info(mut self, name: String, version: String) -> Self {
        self.client_name = name;
        self.client_version = version;
        self
    }
}

// ============================================================================
// Connection Management
// ============================================================================

/// Manages a persistent MCP TCP connection with automatic initialization
pub struct McpConnection {
    stream: Arc<Mutex<TcpStream>>,
    session_id: String,
    config: McpConnectionConfig,
    initialized: bool,
}

impl McpConnection {
    /// Create a new MCP connection and initialize the session
    pub async fn new(config: McpConnectionConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", config.host, config.port);
        info!("Establishing MCP connection to {}", addr);

        let stream = Self::connect_with_retry(&addr, config.timeout, config.max_retries, config.retry_delay).await?;
        let session_id = Uuid::new_v4().to_string();

        let mut connection = Self {
            stream: Arc::new(Mutex::new(stream)),
            session_id: session_id.clone(),
            config,
            initialized: false,
        };

        connection.initialize_session().await?;
        connection.initialized = true;

        Ok(connection)
    }

    /// Connect to MCP server with retry logic
    async fn connect_with_retry(
        addr: &str,
        timeout: Duration,
        max_retries: u32,
        retry_delay: Duration,
    ) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
        let mut last_error = None;

        for attempt in 0..=max_retries {
            match tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => {
                    // Enable TCP_NODELAY for lower latency
                    if let Err(e) = stream.set_nodelay(true) {
                        warn!("Failed to set TCP_NODELAY: {}", e);
                    }

                    if attempt > 0 {
                        info!("Connected to MCP server on attempt {}", attempt + 1);
                    } else {
                        debug!("Connected to MCP server at {}", addr);
                    }
                    return Ok(stream);
                }
                Ok(Err(e)) => {
                    last_error = Some(format!("Connection error: {}", e));
                }
                Err(_) => {
                    last_error = Some("Connection timeout".to_string());
                }
            }

            if attempt < max_retries {
                warn!(
                    "Connection attempt {} failed, retrying in {:?}: {}",
                    attempt + 1,
                    retry_delay,
                    last_error.as_ref().expect("Expected value to be present")
                );
                tokio::time::sleep(retry_delay).await;
            }
        }

        Err(format!(
            "Failed to connect after {} attempts: {}",
            max_retries + 1,
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )
        .into())
    }

    /// Initialize MCP session with protocol negotiation
    async fn initialize_session(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!("Initializing MCP session {}", self.session_id);

        let init_request = json!({
            "jsonrpc": "2.0",
            "id": self.session_id.clone(),
            "method": "initialize",
            "params": {
                "protocolVersion": self.config.protocol_version,
                "clientInfo": {
                    "name": self.config.client_name,
                    "version": self.config.client_version
                },
                "capabilities": {
                    "tools": {
                        "listChanged": true
                    },
                    "roots": {
                        "listChanged": true
                    },
                    "sampling": {}
                }
            }
        });

        let msg = format!("{}\n", init_request.to_string());
        debug!("Sending MCP init: {}", msg.trim());

        let mut stream = self.stream.lock().await;
        stream.write_all(msg.as_bytes()).await?;
        stream.flush().await?;

        // Read initialization response (may receive notifications first)
        loop {
            let response_line = Self::read_line(&mut stream, self.config.timeout).await?;
            debug!("MCP init response: {}", response_line.trim());

            // Skip server.initialized notifications
            if response_line.contains("server.initialized") {
                continue;
            }

            // Parse response
            if let Ok(response) = serde_json::from_str::<Value>(&response_line) {
                if response.get("id").and_then(|id| id.as_str()) == Some(&self.session_id) {
                    if response.get("result").is_some() {
                        info!("MCP session initialized: {}", self.session_id);
                        return Ok(());
                    } else if let Some(error) = response.get("error") {
                        error!("MCP init error: {:?}", error);
                        return Err(format!("MCP initialization failed: {:?}", error).into());
                    }
                }
            }
        }
    }

    /// Read a single line from the stream with timeout
    async fn read_line(
        stream: &mut TcpStream,
        timeout: Duration,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = Vec::new();
        let mut byte = [0u8; 1];

        loop {
            match tokio::time::timeout(timeout, stream.read_exact(&mut byte)).await {
                Ok(Ok(_)) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    buffer.push(byte[0]);
                }
                Ok(Err(e)) => {
                    error!("Error reading from stream: {}", e);
                    return Err(Box::new(e));
                }
                Err(_) => {
                    error!("Timeout reading from MCP server");
                    return Err("Read timeout".into());
                }
            }
        }

        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    /// Execute a command on the MCP server
    pub async fn execute_command(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        if !self.initialized {
            return Err("Connection not initialized".into());
        }

        let request_id = Uuid::new_v4().to_string();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });

        let msg = format!("{}\n", request.to_string());
        debug!("Sending MCP command: {}", msg.trim());

        let mut stream = self.stream.lock().await;
        stream.write_all(msg.as_bytes()).await?;
        stream.flush().await?;

        // Read response (skip notifications)
        loop {
            let response_line = Self::read_line(&mut *stream, self.config.timeout).await?;
            let trimmed = response_line.trim();

            if trimmed.is_empty() {
                continue;
            }

            debug!("MCP response: {}", trimmed);

            // Skip notifications
            if trimmed.contains("server.initialized") {
                continue;
            }

            // Parse response
            if let Ok(response) = serde_json::from_str::<Value>(trimmed) {
                if response.get("id").and_then(|id| id.as_str()) == Some(&request_id) {
                    if let Some(result) = response.get("result") {
                        info!("MCP command '{}' executed successfully", method);
                        return Ok(result.clone());
                    } else if let Some(error) = response.get("error") {
                        error!("MCP command error: {:?}", error);
                        return Err(format!("MCP error: {:?}", error).into());
                    }
                } else if response.get("method").is_some() {
                    // Skip notifications with method field
                    continue;
                }
            }
        }
    }
}

// ============================================================================
// Connection Pool
// ============================================================================

/// Pool of persistent MCP connections
#[derive(Clone)]
pub struct McpConnectionPool {
    connections: Arc<RwLock<HashMap<String, Arc<McpConnection>>>>,
    default_config: McpConnectionConfig,
}

impl McpConnectionPool {
    pub fn new(config: McpConnectionConfig) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            default_config: config,
        }
    }

    /// Get or create a connection for a specific purpose
    pub async fn get_connection(
        &self,
        purpose: &str,
    ) -> Result<Arc<McpConnection>, Box<dyn std::error::Error + Send + Sync>> {
        // Try to reuse existing connection
        {
            let connections = self.connections.read().await;
            if let Some(conn) = connections.get(purpose) {
                debug!("Reusing existing MCP connection for {}", purpose);
                return Ok(Arc::clone(conn));
            }
        }

        // Create new connection
        info!("Creating new MCP connection for {}", purpose);
        let conn = Arc::new(McpConnection::new(self.default_config.clone()).await?);

        // Store in pool
        let mut connections = self.connections.write().await;
        connections.insert(purpose.to_string(), Arc::clone(&conn));

        Ok(conn)
    }

    /// Execute a command using a pooled connection
    pub async fn execute_command(
        &self,
        purpose: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let conn = self.get_connection(purpose).await?;
        conn.execute_command(method, params).await
    }

    /// Remove a connection from the pool
    pub async fn remove_connection(&self, purpose: &str) {
        let mut connections = self.connections.write().await;
        if connections.remove(purpose).is_some() {
            info!("Removed MCP connection for {}", purpose);
        }
    }

    /// Get pool statistics
    pub async fn get_stats(&self) -> HashMap<String, usize> {
        let connections = self.connections.read().await;
        let mut stats = HashMap::new();
        stats.insert("total_connections".to_string(), connections.len());
        stats
    }
}

// ============================================================================
// High-Level MCP Client
// ============================================================================

/// High-level MCP client with request/response handling and retry logic
#[derive(Clone)]
pub struct McpClient {
    config: McpConnectionConfig,
    pool: Arc<McpConnectionPool>,
}

impl McpClient {
    /// Create a new MCP client
    pub fn new(host: String, port: u16) -> Self {
        let config = McpConnectionConfig::new(host, port);
        let pool = Arc::new(McpConnectionPool::new(config.clone()));
        Self { config, pool }
    }

    /// Create client with custom configuration
    pub fn with_config(config: McpConnectionConfig) -> Self {
        let pool = Arc::new(McpConnectionPool::new(config.clone()));
        Self { config, pool }
    }

    /// Send a typed request and receive a typed response
    pub async fn send_request<T, R>(
        &self,
        method: &str,
        params: T,
    ) -> Result<R, Box<dyn std::error::Error + Send + Sync>>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let params_json = serde_json::to_value(&params)
            .map_err(|e| format!("Failed to serialize params: {}", e))?;

        let result = self.with_retry(|| {
            let pool = self.pool.clone();
            let method = method.to_string();
            let params = params_json.clone();
            Box::pin(async move {
                pool.execute_command("default", &method, params).await
            })
        })
        .await?;

        serde_json::from_value(result)
            .map_err(|e| format!("Failed to deserialize response: {}", e).into())
    }

    /// Send a tool call request
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let wrapped_params = json!({
            "name": tool_name,
            "arguments": arguments
        });

        debug!("Sending tool call '{}' with arguments: {}", tool_name, arguments);

        let response = self.with_retry(|| {
            let pool = self.pool.clone();
            let params = wrapped_params.clone();
            Box::pin(async move {
                pool.execute_command("default", "tools/call", params).await
            })
        })
        .await?;

        // Extract result from tool call response
        if let Some(content) = response.get("content") {
            if let Some(content_array) = content.as_array() {
                if let Some(first_content) = content_array.first() {
                    if let Some(text) = first_content.get("text").and_then(|t| t.as_str()) {
                        match serde_json::from_str::<Value>(text) {
                            Ok(parsed) => {
                                debug!("Parsed tool response: {}", parsed);
                                return Ok(parsed);
                            }
                            Err(e) => {
                                warn!("Failed to parse tool response JSON: {}", e);
                                return Err(format!("Failed to parse tool response: {}", e).into());
                            }
                        }
                    }
                }
            }
        }

        Err("Invalid tool call response format".into())
    }

    /// Execute an operation with retry logic
    pub async fn with_retry<F, Fut>(
        &self,
        operation: F,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn() -> Pin<Box<Fut>>,
        Fut: std::future::Future<Output = Result<Value, Box<dyn std::error::Error + Send + Sync>>>,
    {
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        info!("Request succeeded on attempt {}", attempt + 1);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries {
                        warn!("Request attempt {} failed, retrying", attempt + 1);
                        tokio::time::sleep(self.config.retry_delay).await;
                    }
                }
            }
        }

        Err(format!(
            "Request failed after {} attempts: {}",
            self.config.max_retries + 1,
            last_error.unwrap_or_else(|| "Unknown error".into())
        )
        .into())
    }

    /// Test connection to MCP server
    pub async fn test_connection(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        match self.pool.get_connection("connection_test").await {
            Ok(_) => {
                info!("MCP connection test successful");
                Ok(true)
            }
            Err(e) => {
                warn!("MCP connection test failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Get connection pool statistics
    pub async fn get_pool_stats(&self) -> HashMap<String, usize> {
        self.pool.get_stats().await
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Test connectivity to multiple MCP servers
pub async fn test_mcp_connectivity(
    servers: &HashMap<String, (String, u16)>,
) -> HashMap<String, bool> {
    let mut results = HashMap::new();

    for (server_id, (host, port)) in servers {
        let client = McpClient::new(host.clone(), *port);
        match client.test_connection().await {
            Ok(connected) => {
                results.insert(server_id.clone(), connected);
                if connected {
                    info!("✓ MCP server {} is reachable at {}:{}", server_id, host, port);
                } else {
                    warn!("✗ MCP server {} is not reachable at {}:{}", server_id, host, port);
                }
            }
            Err(e) => {
                results.insert(server_id.clone(), false);
                error!("✗ Failed to test MCP server {}: {}", server_id, e);
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = McpConnectionConfig::new("localhost".to_string(), 9500)
            .with_timeout(Duration::from_secs(5))
            .with_retry_config(5, Duration::from_secs(1));

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 9500);
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.max_retries, 5);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let config = McpConnectionConfig::default();
        let pool = McpConnectionPool::new(config);
        let stats = pool.get_stats().await;
        assert_eq!(stats.get("total_connections"), Some(&0));
    }
}
