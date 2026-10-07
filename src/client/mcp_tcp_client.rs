use crate::utils::json::{from_json, to_json};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Debug, Clone)]
pub struct McpTelemetryClient {
    host: String,
    port: u16,
    request_timeout: Duration,
    next_request_id: u64,
}

impl McpTelemetryClient {
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            request_timeout: Duration::from_secs(10),
            next_request_id: 1,
        }
    }

    pub fn for_multi_agent_container() -> Self {
        Self::new("multi-agent-container".to_string(), 9500)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    async fn connect(&self) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", self.host, self.port);
        debug!("Connecting to MCP TCP server at {}", addr);

        let stream =
            tokio::time::timeout(self.request_timeout, TcpStream::connect(&addr)).await??;

        info!("Connected to MCP TCP server at {}", addr);
        Ok(stream)
    }

    async fn send_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut stream = self.connect().await?;

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });

        let request_str = to_json(&request)?;
        debug!("Sending MCP request: {}", request_str);

        stream.write_all(request_str.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();

        tokio::time::timeout(self.request_timeout, reader.read_line(&mut response_line)).await??;

        debug!("Received MCP response: {}", response_line);

        let response: Value = from_json(&response_line)?;

        if let Some(error) = response.get("error") {
            return Err(format!("MCP error: {}", error).into());
        }

        Ok(response["result"].clone())
    }

    pub async fn list_tools(
        &mut self,
    ) -> Result<Vec<McpTool>, Box<dyn std::error::Error + Send + Sync>> {
        let result = self.send_request("tools/list", json!({})).await?;

        let tools: Vec<McpTool> =
            serde_json::from_value(result.get("tools").cloned().unwrap_or(json!([])))?;

        Ok(tools)
    }

    pub async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let params = json!({
            "name": tool_name,
            "arguments": arguments
        });

        self.send_request("tools/call", params).await
    }

    pub async fn query_session_status(
        &mut self,
        session_uuid: &str,
    ) -> Result<SessionStatus, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying session status for UUID: {}", session_uuid);

        let result = self
            .call_tool("session_status", json!({ "session_id": session_uuid }))
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    pub async fn query_session_agents(
        &mut self,
        session_uuid: &str,
    ) -> Result<Vec<AgentInfo>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying session agents for UUID: {}", session_uuid);

        let result = self
            .call_tool("session_agents", json!({ "session_id": session_uuid }))
            .await?;

        let agents: Vec<AgentInfo> =
            serde_json::from_value(result.get("agents").cloned().unwrap_or(json!([])))?;

        Ok(agents)
    }

    pub async fn query_session_metrics(
        &mut self,
        session_uuid: &str,
    ) -> Result<SessionMetrics, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying session metrics for UUID: {}", session_uuid);

        let result = self
            .call_tool("session_metrics", json!({ "session_id": session_uuid }))
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    pub async fn query_swarm_list(
        &mut self,
    ) -> Result<Vec<SwarmInfo>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying all swarms");

        let result = self.call_tool("swarm_list", json!({})).await?;

        let swarms: Vec<SwarmInfo> =
            serde_json::from_value(result.get("swarms").cloned().unwrap_or(json!([])))?;

        Ok(swarms)
    }

    pub async fn query_swarm_metrics(
        &mut self,
        swarm_id: &str,
    ) -> Result<SwarmMetrics, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying swarm metrics for: {}", swarm_id);

        let result = self
            .call_tool("swarm_monitor", json!({ "swarm_id": swarm_id }))
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    pub async fn query_agent_metrics(
        &mut self,
        agent_id: &str,
    ) -> Result<AgentMetrics, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying agent metrics for: {}", agent_id);

        let result = self
            .call_tool("agent_metrics", json!({ "agent_id": agent_id }))
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    pub async fn query_performance_summary(
        &mut self,
    ) -> Result<PerformanceSummary, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying performance summary");

        let result = self.call_tool("performance_summary", json!({})).await?;

        Ok(serde_json::from_value(result)?)
    }

    pub async fn ping(&mut self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        match self.send_request("ping", json!({})).await {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("MCP ping failed: {}", e);
                Ok(false)
            }
        }
    }
}

// Data structures for MCP responses

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub status: String,
    pub swarm_id: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub role: String,
    pub status: String,
    #[serde(default)]
    pub position: Position3D,
    #[serde(default)]
    pub connections: Vec<String>,
    pub current_task: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Position3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub swarm_id: String,
    pub session_id: String,
    #[serde(default)]
    pub agents: Vec<AgentMetrics>,
    pub tasks: TaskMetrics,
    pub performance: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub agent_id: String,
    pub performance: AgentPerformance,
    #[serde(default)]
    pub resources: ResourceUsage,
    pub neural_state: Option<NeuralState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPerformance {
    pub tasks_per_hour: f32,
    pub success_rate: f32,
    pub average_task_duration_ms: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceUsage {
    pub cpu_usage: f32,
    pub memory_mb: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralState {
    pub activation_pattern: Vec<f32>,
    pub decision_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMetrics {
    pub total_tasks: u32,
    pub completed_tasks: u32,
    pub active_tasks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_tasks: u32,
    pub completed_tasks: u32,
    pub active_agents: u32,
    pub memory_usage_mb: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmInfo {
    pub swarm_id: String,
    pub session_id: String,
    pub status: String,
    pub agents: u32,
    pub tasks: TaskMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmMetrics {
    pub swarm_id: String,
    pub status: String,
    pub agents: Vec<AgentInfo>,
    pub metrics: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    pub total_swarms: u32,
    pub total_agents: u32,
    pub total_tasks: u32,
    pub completed_tasks: u32,
    pub system_cpu_usage: f32,
    pub system_memory_mb: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mcp_client_creation() {
        let client = McpTelemetryClient::for_multi_agent_container();
        assert_eq!(client.host, "multi-agent-container");
        assert_eq!(client.port, 9500);
    }

    #[tokio::test]
    async fn test_mcp_client_with_timeout() {
        let client =
            McpTelemetryClient::for_multi_agent_container().with_timeout(Duration::from_secs(5));
        assert_eq!(client.request_timeout, Duration::from_secs(5));
    }
}
