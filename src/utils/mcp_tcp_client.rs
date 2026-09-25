//! MCP TCP Client Implementation
//!
//! This module provides a TCP client for connecting to MCP (Model Context Protocol) servers
//! and executing agent discovery queries. It replaces mock data with real TCP connections.

use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::services::agent_visualization_protocol::{
    AgentExtendedMetadata, AgentPerformanceData, McpServerInfo, McpServerType, MultiMcpAgentStatus,
    SwarmTopologyData, TopologyPosition,
};
use crate::utils::json::{from_json, to_json};
use crate::utils::time;

#[derive(Debug, Clone)]
pub struct McpTcpClient {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
}

#[derive(Debug)]
pub struct McpConnectionPool {
    servers: Arc<Mutex<HashMap<String, McpTcpClient>>>,
    max_connections_per_server: usize,
}

static CONNECTION_POOL: Lazy<McpConnectionPool> = Lazy::new(|| McpConnectionPool::new(5));

#[derive(Debug, serde::Serialize)]
struct McpRequest {
    jsonrpc: String,
    method: String,
    params: Value,
    id: u64,
}

#[derive(Debug, serde::Deserialize)]
struct McpResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<McpError>,
    #[allow(dead_code)]
    id: u64,
}

#[derive(Debug, serde::Deserialize)]
struct McpError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl McpTcpClient {
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            timeout: Duration::from_secs(10),
            max_retries: 3,
            retry_delay: Duration::from_millis(500),
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

    async fn connect(&self) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
        let addr_str = format!("{}:{}", self.host, self.port);
        debug!(
            "Connecting to MCP server at {} with {} retries",
            addr_str, self.max_retries
        );

        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            match tokio::time::timeout(self.timeout, TcpStream::connect(&addr_str)).await {
                Ok(Ok(stream)) => {
                    if let Err(e) = stream.set_nodelay(true) {
                        warn!("Failed to set TCP_NODELAY: {}", e);
                    }

                    if attempt > 0 {
                        info!(
                            "Successfully connected to MCP server at {}:{} on attempt {}",
                            self.host,
                            self.port,
                            attempt + 1
                        );
                    } else {
                        debug!(
                            "Successfully connected to MCP server at {}:{}",
                            self.host, self.port
                        );
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

            if attempt < self.max_retries {
                warn!(
                    "Connection attempt {} failed, retrying in {:?}: {}",
                    attempt + 1,
                    self.retry_delay,
                    last_error.as_ref().expect("Expected value to be present")
                );
                tokio::time::sleep(self.retry_delay).await;
            }
        }

        Err(format!(
            "Failed to connect after {} attempts: {}",
            self.max_retries + 1,
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )
        .into())
    }

    async fn send_request(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            match self.try_send_request(method, params.clone()).await {
                Ok(result) => {
                    if attempt > 0 {
                        info!("Request succeeded on attempt {}", attempt + 1);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.max_retries {
                        warn!(
                            "Request attempt {} failed, retrying: {}",
                            attempt + 1,
                            last_error.as_ref().expect("Expected value to be present")
                        );
                        tokio::time::sleep(self.retry_delay).await;
                    }
                }
            }
        }

        Err(format!(
            "Request failed after {} attempts: {}",
            self.max_retries + 1,
            last_error.unwrap_or_else(|| "Unknown error".into())
        )
        .into())
    }

    async fn send_tool_call(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let wrapped_params = json!({
            "name": tool_name,
            "arguments": arguments
        });

        debug!(
            "Sending tool call '{}' with arguments: {}",
            tool_name, arguments
        );

        let response = self.send_request("tools/call", wrapped_params).await?;

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

    async fn try_send_request(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut stream = self.connect().await?;

        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: time::timestamp_millis() as u64,
        };

        let request_json =
            to_json(&request).map_err(|e| format!("Failed to serialize request: {}", e))?;
        debug!("Sending MCP request: {}", request_json);

        let request_data = format!("{}\n", request_json);
        stream
            .write_all(request_data.as_bytes())
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;
        stream
            .flush()
            .await
            .map_err(|e| format!("Failed to flush stream: {}", e))?;

        let response_str = self.read_response(&mut stream).await?;
        debug!("Received MCP response: {}", response_str.trim());

        let response: McpResponse = from_json(response_str.trim())
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(error) = response.error {
            return Err(format!(
                "MCP Error {}: {} (data: {:?})",
                error.code, error.message, error.data
            )
            .into());
        }

        response
            .result
            .ok_or_else(|| "No result in MCP response".into())
    }

    async fn read_response(
        &self,
        stream: &mut TcpStream,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();

        match tokio::time::timeout(self.timeout, reader.read_line(&mut response_line)).await {
            Ok(Ok(bytes_read)) => {
                if bytes_read == 0 {
                    return Err("Connection closed without response".into());
                }
                debug!("Read {} bytes from MCP server", bytes_read);
                Ok(response_line)
            }
            Ok(Err(e)) => Err(format!("Read error: {}", e).into()),
            Err(_) => Err("Read timeout".into()),
        }
    }

    pub async fn query_agent_list(
        &self,
    ) -> Result<Vec<MultiMcpAgentStatus>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying agent list from MCP server");

        let params = json!({
            "filter": "all",
            "include_metadata": true
        });

        let result = self.send_tool_call("agent_list", params).await?;
        debug!("Agent list response: {}", result);

        let agents = self.parse_agent_list_response(&result)?;
        info!(
            "Successfully retrieved {} agents from MCP server",
            agents.len()
        );

        Ok(agents)
    }

    pub async fn query_swarm_status(
        &self,
    ) -> Result<SwarmTopologyData, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying swarm status from MCP server");

        let params = json!({
            "include_topology": true,
            "include_performance": true
        });

        let result = self.send_tool_call("swarm_status", params).await?;
        debug!("Swarm status response: {}", result);

        let topology = self.parse_swarm_topology_response(&result)?;
        info!("Successfully retrieved swarm topology from MCP server");

        Ok(topology)
    }

    pub async fn query_server_info(
        &self,
    ) -> Result<McpServerInfo, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying server info from MCP server");

        let params = json!({});

        let result = self.send_tool_call("server_info", params).await?;
        debug!("Server info response: {}", result);

        let server_info = self.parse_server_info_response(&result)?;
        info!("Successfully retrieved server info from MCP server");

        Ok(server_info)
    }

    /// Store a value into RuVector via the claude-flow `memory_store` MCP tool
    /// (ADR-114 seed-leg substrate: xinference `bge-small-en-v1.5` 384-dim HNSW).
    /// This is the mandated write path — the embedding pipeline runs behind
    /// `memory_store`; a raw SQL INSERT would bypass it (agentbox CLAUDE.md).
    /// Returns the raw tool result on success.
    pub async fn memory_store(
        &self,
        namespace: &str,
        key: &str,
        value: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let params = json!({
            "namespace": namespace,
            "key": key,
            "value": value,
        });
        debug!(
            "memory_store ns='{}' key='{}' ({} bytes)",
            namespace,
            key,
            value.len()
        );
        self.send_tool_call("memory_store", params).await
    }

    /// Semantic-search a RuVector namespace via the claude-flow `memory_search`
    /// MCP tool. Used by the ADR-119 liveness canary to prove the seed index is
    /// live and readable. Returns the raw tool result (caller extracts hits).
    pub async fn memory_search(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        // min_score 0: the canary proves the index is live and populated, not
        // that the probe query is a good match; the server's relevance floor
        // (default 0.55) would otherwise report a healthy index as EMPTY.
        let params = json!({
            "namespace": namespace,
            "query": query,
            "limit": limit,
            "min_score": 0.0,
        });
        debug!(
            "memory_search ns='{}' query='{}' limit={}",
            namespace, query, limit
        );
        self.send_tool_call("memory_search", params).await
    }

    fn parse_agent_list_response(
        &self,
        response: &Value,
    ) -> Result<Vec<MultiMcpAgentStatus>, Box<dyn std::error::Error + Send + Sync>> {
        let agents_array = response
            .get("agents")
            .and_then(|a| a.as_array())
            .ok_or("No 'agents' array in response")?;

        let mut agents = Vec::new();

        for agent_data in agents_array {
            match self.parse_single_agent(agent_data) {
                Ok(agent) => agents.push(agent),
                Err(e) => {
                    warn!("Failed to parse agent data: {}", e);
                    continue;
                }
            }
        }

        Ok(agents)
    }

    fn parse_single_agent(
        &self,
        agent_data: &Value,
    ) -> Result<MultiMcpAgentStatus, Box<dyn std::error::Error + Send + Sync>> {
        let agent_id = agent_data
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("Missing agent ID")?
            .to_string();

        let name = agent_data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&agent_id)
            .to_string();

        let agent_type = agent_data
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let status = agent_data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let swarm_id = agent_data
            .get("swarm_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();

        let capabilities = agent_data
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(Vec::new);

        let performance = self.parse_performance_data(agent_data.get("performance"))?;

        let metadata = self.parse_agent_metadata(agent_data.get("metadata"))?;

        let neural_info = agent_data
            .get("neural")
            .map(|neural_data| self.parse_neural_data(neural_data))
            .transpose()?;

        // COM-14: consume the agentbox-minted did:nostr from the agent record if
        // present, else None. Accept both the snake_case wire key and a camelCase
        // fallback; validation to the trusted form happens at the Agent carry
        // boundary (Agent::from), not here.
        let did_nostr = agent_data
            .get("did_nostr")
            .or_else(|| agent_data.get("didNostr"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let now = time::now();

        Ok(MultiMcpAgentStatus {
            agent_id,
            swarm_id,
            server_source: McpServerType::ClaudeFlow,
            name,
            agent_type,
            status,
            capabilities,
            metadata,
            performance,
            neural_info,
            created_at: now.timestamp(),
            last_active: now.timestamp(),
            did_nostr,
        })
    }

    fn parse_performance_data(
        &self,
        perf_data: Option<&Value>,
    ) -> Result<AgentPerformanceData, Box<dyn std::error::Error + Send + Sync>> {
        let default_json = json!({});
        let perf = perf_data.unwrap_or(&default_json);

        Ok(AgentPerformanceData {
            cpu_usage: perf
                .get("cpu_usage")
                .and_then(|v| v.as_f64())
                .unwrap_or(25.0) as f32,
            memory_usage: perf
                .get("memory_usage")
                .and_then(|v| v.as_f64())
                .unwrap_or(35.0) as f32,
            health_score: perf
                .get("health_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(100.0) as f32,
            activity_level: perf
                .get("activity_level")
                .and_then(|v| v.as_f64())
                .unwrap_or(50.0) as f32,
            tasks_active: perf
                .get("tasks_active")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            tasks_completed: perf
                .get("tasks_completed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            tasks_failed: perf
                .get("tasks_failed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            success_rate: perf
                .get("success_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or(100.0) as f32,
            token_usage: perf
                .get("token_usage")
                .and_then(|v| v.as_u64())
                .unwrap_or(1000),
            token_rate: perf
                .get("token_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32,
            response_time_ms: perf
                .get("response_time_ms")
                .and_then(|v| v.as_f64())
                .unwrap_or(150.0) as f32,
            throughput: perf
                .get("throughput")
                .and_then(|v| v.as_f64())
                .unwrap_or(10.0) as f32,
        })
    }

    fn parse_agent_metadata(
        &self,
        meta_data: Option<&Value>,
    ) -> Result<AgentExtendedMetadata, Box<dyn std::error::Error + Send + Sync>> {
        let default_json = json!({});
        let meta = meta_data.unwrap_or(&default_json);

        let topology_position = meta.get("topology_position").map(|pos| TopologyPosition {
            layer: pos.get("layer").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            index_in_layer: pos
                .get("index_in_layer")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            connections: pos
                .get("connections")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_else(Vec::new),
            is_coordinator: pos
                .get("is_coordinator")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            coordination_level: pos
                .get("coordination_level")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        });

        let tags = meta
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(Vec::new);

        Ok(AgentExtendedMetadata {
            session_id: meta
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            parent_id: meta
                .get("parent_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            topology_position,
            coordination_role: meta
                .get("coordination_role")
                .and_then(|v| v.as_str())
                .map(String::from),
            task_queue_size: meta
                .get("task_queue_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            error_count: meta
                .get("error_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            warning_count: meta
                .get("warning_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            tags,
        })
    }

    fn parse_neural_data(
        &self,
        neural_data: &Value,
    ) -> Result<
        crate::services::agent_visualization_protocol::NeuralAgentData,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Ok(
            crate::services::agent_visualization_protocol::NeuralAgentData {
                model_type: neural_data
                    .get("model_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                model_size: neural_data
                    .get("model_size")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium")
                    .to_string(),
                training_status: neural_data
                    .get("training_status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("idle")
                    .to_string(),
                cognitive_pattern: neural_data
                    .get("cognitive_pattern")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string(),
                learning_rate: neural_data
                    .get("learning_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.01) as f32,
                adaptation_score: neural_data
                    .get("adaptation_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5) as f32,
                memory_capacity: neural_data
                    .get("memory_capacity")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1024000),
                knowledge_domains: neural_data
                    .get("knowledge_domains")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_else(Vec::new),
            },
        )
    }

    fn parse_swarm_topology_response(
        &self,
        response: &Value,
    ) -> Result<SwarmTopologyData, Box<dyn std::error::Error + Send + Sync>> {
        let topology_type = response
            .get("topology_type")
            .and_then(|v| v.as_str())
            .unwrap_or("mesh")
            .to_string();

        let total_agents = response
            .get("total_agents")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let coordination_layers = response
            .get("coordination_layers")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        let efficiency_score = response
            .get("efficiency_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.85) as f32;

        Ok(SwarmTopologyData {
            topology_type,
            total_agents,
            coordination_layers,
            efficiency_score,
            load_distribution: vec![],
            critical_paths: vec![],
            bottlenecks: vec![],
        })
    }

    fn parse_server_info_response(
        &self,
        response: &Value,
    ) -> Result<McpServerInfo, Box<dyn std::error::Error + Send + Sync>> {
        let server_id = response
            .get("server_id")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-flow")
            .to_string();

        let server_type = match response.get("server_type").and_then(|v| v.as_str()) {
            Some("claude-flow") => McpServerType::ClaudeFlow,
            Some("ruv-swarm") => McpServerType::RuvSwarm,
            Some("daa") => McpServerType::Daa,
            Some(custom) => McpServerType::Custom(custom.to_string()),
            None => McpServerType::ClaudeFlow,
        };

        let supported_tools = response
            .get("supported_tools")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| {
                vec![
                    "agent_list".to_string(),
                    "swarm_status".to_string(),
                    "server_info".to_string(),
                ]
            });

        let agent_count = response
            .get("agent_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        Ok(McpServerInfo {
            server_id,
            server_type,
            host: self.host.clone(),
            port: self.port,
            is_connected: true,
            last_heartbeat: time::timestamp_seconds(),
            supported_tools,
            agent_count,
        })
    }

    pub async fn test_connection(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        match self.connect().await {
            Ok(_) => {
                info!("MCP TCP connection test successful");
                Ok(true)
            }
            Err(e) => {
                warn!("MCP TCP connection test failed: {}", e);
                Ok(false)
            }
        }
    }

    pub async fn initialize_session(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!("Initializing MCP session");

        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "roots": {
                    "listChanged": true
                },
                "sampling": {}
            },
            "clientInfo": {
                "name": "visionclaw-mcp-client",
                "version": "1.0.0"
            }
        });

        let result = self.send_request("initialize", params).await?;
        debug!("MCP session initialized: {}", result);

        Ok(())
    }
}

impl McpConnectionPool {
    pub fn new(max_connections_per_server: usize) -> Self {
        Self {
            servers: Arc::new(Mutex::new(HashMap::new())),
            max_connections_per_server,
        }
    }

    pub async fn get_client(&self, server_id: &str, host: &str, port: u16) -> McpTcpClient {
        let mut servers = self.servers.lock().await;

        if let Some(client) = servers.get(server_id) {
            debug!("Reusing existing MCP client for {}", server_id);
            client.clone()
        } else {
            debug!(
                "Creating new MCP client for {} at {}:{}",
                server_id, host, port
            );
            let client = McpTcpClient::new(host.to_string(), port)
                .with_timeout(Duration::from_secs(10))
                .with_retry_config(3, Duration::from_millis(500));

            servers.insert(server_id.to_string(), client.clone());
            client
        }
    }

    pub async fn remove_client(&self, server_id: &str) {
        let mut servers = self.servers.lock().await;
        if servers.remove(server_id).is_some() {
            info!("Removed MCP client for {} from pool", server_id);
        }
    }

    pub async fn get_stats(&self) -> HashMap<String, usize> {
        let servers = self.servers.lock().await;
        let mut stats = HashMap::new();
        stats.insert("total_clients".to_string(), servers.len());
        stats.insert(
            "max_per_server".to_string(),
            self.max_connections_per_server,
        );
        stats
    }
}

pub fn create_mcp_client(server_type: &McpServerType, host: &str, port: u16) -> McpTcpClient {
    info!(
        "Creating MCP TCP client for {:?} at {}:{}",
        server_type, host, port
    );
    McpTcpClient::new(host.to_string(), port)
        .with_timeout(Duration::from_secs(15))
        .with_retry_config(5, Duration::from_millis(1000))
}

pub async fn get_pooled_mcp_client(
    server_type: &McpServerType,
    host: &str,
    port: u16,
) -> McpTcpClient {
    let server_id = format!("{:?}_{}_{})", server_type, host, port);
    CONNECTION_POOL.get_client(&server_id, host, port).await
}

pub async fn test_mcp_connectivity(
    servers: &HashMap<String, (String, u16)>,
) -> HashMap<String, bool> {
    let mut results = HashMap::new();

    for (server_id, (host, port)) in servers {
        let client = McpTcpClient::new(host.clone(), *port);
        match client.test_connection().await {
            Ok(connected) => {
                results.insert(server_id.clone(), connected);
                if connected {
                    info!("MCP server {} is reachable at {}:{}", server_id, host, port);
                } else {
                    warn!(
                        "✗ MCP server {} is not reachable at {}:{}",
                        server_id, host, port
                    );
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
