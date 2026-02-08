//! Types for claude flow integration via TCP
//! These replace the local claude_flow module types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::{timeout, Duration};

use crate::utils::time;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    
    #[serde(rename = "id")]
    pub agent_id: String,
    pub profile: AgentProfile,
    pub status: String,

    
    pub active_tasks_count: u32,
    pub completed_tasks_count: u32,
    pub failed_tasks_count: u32,
    pub success_rate: f32,
    pub timestamp: DateTime<Utc>,
    pub current_task: Option<TaskReference>,

    
    #[serde(rename = "type")]
    pub agent_type: String,

    #[serde(rename = "currentTask")]
    pub current_task_description: Option<String>,

    pub capabilities: Vec<String>,

    
    pub position: Option<Vec3>,

    
    #[serde(rename = "cpuUsage")]
    pub cpu_usage: f32,

    #[serde(rename = "memoryUsage")]
    pub memory_usage: f32,

    pub health: f32,
    pub activity: f32,

    #[serde(rename = "tasksActive")]
    pub tasks_active: u32,

    #[serde(rename = "tasksCompleted")]
    pub tasks_completed: u32,

    #[serde(rename = "successRate")]
    pub success_rate_normalized: f32,

    pub tokens: u64,

    #[serde(rename = "tokenRate")]
    pub token_rate: f32,

    
    pub performance_metrics: PerformanceMetrics,
    pub token_usage: TokenUsage,

    #[serde(rename = "swarmId")]
    pub swarm_id: Option<String>,

    #[serde(rename = "agentMode")]
    pub agent_mode: Option<String>,

    #[serde(rename = "parentQueenId")]
    pub parent_queen_id: Option<String>,

    #[serde(rename = "processingLogs")]
    pub processing_logs: Option<Vec<String>>,

    #[serde(rename = "createdAt")]
    pub created_at: String,

    pub age: u64,
    pub workload: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub tasks_completed: u32,
    pub success_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub total: u64,
    pub token_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,
    pub agent_type: AgentType,
    pub capabilities: Vec<String>,
    pub description: Option<String>,
    pub version: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    Coordinator,
    Researcher,
    Coder,
    Analyst,
    Architect,
    Tester,
    Reviewer,
    Optimizer,
    Documenter,
    Generic,
}

impl ToString for AgentType {
    fn to_string(&self) -> String {
        match self {
            AgentType::Coordinator => "coordinator".to_string(),
            AgentType::Researcher => "researcher".to_string(),
            AgentType::Coder => "coder".to_string(),
            AgentType::Analyst => "analyst".to_string(),
            AgentType::Architect => "architect".to_string(),
            AgentType::Tester => "tester".to_string(),
            AgentType::Reviewer => "reviewer".to_string(),
            AgentType::Optimizer => "optimizer".to_string(),
            AgentType::Documenter => "documenter".to_string(),
            AgentType::Generic => "generic".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReference {
    pub task_id: String,
    pub description: String,
    pub priority: TaskPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

// TCP Client for communicating with external claude flow service
#[derive(Clone)]
pub struct ClaudeFlowClient {
    host: String,
    port: u16,
    
}

impl ClaudeFlowClient {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }

    pub async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::net::TcpStream;
        use tokio::time::{timeout, Duration};

        log::info!("Connecting to Claude Flow at {}:{}", self.host, self.port);

        let addr = format!("{}:{}", self.host, self.port);
        let stream = timeout(Duration::from_secs(10), TcpStream::connect(&addr)).await??;

        log::info!("Successfully connected to Claude Flow at {}", addr);

        
        drop(stream); 
        Ok(())
    }

    pub async fn get_agent_statuses(
        &self,
    ) -> Result<Vec<AgentStatus>, Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
        use tokio::time::{timeout, Duration};

        let addr = format!("{}:{}", self.host, self.port);
        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await??;

        
        let request = json!({
            "jsonrpc": "2.0",
            "method": "list_agents",
            "id": 1
        });

        let request_str = format!("{}\n", request.to_string());
        stream.write_all(request_str.as_bytes()).await?;

        
        let mut buffer = vec![0; 8192];
        let bytes_read = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await??;

        if bytes_read == 0 {
            return Ok(vec![]); 
        }

        let response_str = String::from_utf8_lossy(&buffer[..bytes_read]);

        
        if let Ok(response_json) = serde_json::from_str::<serde_json::Value>(&response_str) {
            if let Some(agents_array) = response_json.get("result").and_then(|r| r.as_array()) {
                let mut statuses = Vec::new();
                for agent_data in agents_array {
                    if let Ok(status) = self.parse_agent_status(agent_data) {
                        statuses.push(status);
                    }
                }
                return Ok(statuses);
            }
        }

        log::debug!("No valid agents found in TCP response");
        Ok(vec![])
    }

    fn parse_agent_status(
        &self,
        agent_data: &serde_json::Value,
    ) -> Result<AgentStatus, Box<dyn std::error::Error + Send + Sync>> {
        let agent_id = agent_data
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let agent_type = agent_data
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("generic")
            .to_string();
        let status = agent_data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("idle")
            .to_string();

        
        let position = if let (Some(x), Some(y), Some(z)) = (
            agent_data.get("x").and_then(|v| v.as_f64()),
            agent_data.get("y").and_then(|v| v.as_f64()),
            agent_data.get("z").and_then(|v| v.as_f64()),
        ) {
            Some(Vec3 {
                x: x as f32,
                y: y as f32,
                z: z as f32,
            })
        } else if let Some(pos) = agent_data.get("position") {
            Some(Vec3 {
                x: pos.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                y: pos.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                z: pos.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            })
        } else {
            None
        };

        
        let current_task_description = agent_data
            .get("current_task")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                agent_data
                    .get("currentTask")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            });

        
        let capabilities = agent_data
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_else(|| vec![agent_type.clone()]);

        
        let success_rate_raw = agent_data
            .get("success_rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.95) as f32;
        let success_rate_normalized = if success_rate_raw > 1.0 {
            success_rate_raw / 100.0
        } else {
            success_rate_raw
        };

        
        let cpu_usage_raw = agent_data
            .get("cpu_usage")
            .and_then(|v| v.as_f64())
            .unwrap_or(25.0) as f32;
        let cpu_usage = if cpu_usage_raw > 1.0 {
            cpu_usage_raw / 100.0
        } else {
            cpu_usage_raw
        };

        let memory_usage_raw = agent_data
            .get("memory_usage")
            .and_then(|v| v.as_f64())
            .unwrap_or(128.0) as f32;
        let memory_usage = if memory_usage_raw > 1.0 {
            memory_usage_raw / 100.0
        } else {
            memory_usage_raw
        };

        let now = time::now();
        let created_at = now.to_rfc3339();

        Ok(AgentStatus {
            agent_id: agent_id.clone(),
            profile: AgentProfile {
                name: agent_id.clone(),
                agent_type: match agent_type.as_str() {
                    "coordinator" => AgentType::Coordinator,
                    "researcher" => AgentType::Researcher,
                    "coder" => AgentType::Coder,
                    "analyst" => AgentType::Analyst,
                    "architect" => AgentType::Architect,
                    "tester" => AgentType::Tester,
                    "reviewer" => AgentType::Reviewer,
                    "optimizer" => AgentType::Optimizer,
                    "documenter" => AgentType::Documenter,
                    _ => AgentType::Generic,
                },
                capabilities: capabilities.clone(),
                description: Some(format!("{} agent", agent_type)),
                version: "1.0.0".to_string(),
                tags: vec!["general".to_string()],
            },
            status: status.clone(),

            
            active_tasks_count: agent_data
                .get("active_tasks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            completed_tasks_count: agent_data
                .get("completed_tasks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            failed_tasks_count: agent_data
                .get("failed_tasks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            success_rate: success_rate_raw,
            timestamp: now,
            current_task: current_task_description
                .as_ref()
                .map(|task_desc| TaskReference {
                    task_id: format!("task_{}", uuid::Uuid::new_v4()),
                    description: task_desc.clone(),
                    priority: TaskPriority::Medium,
                }),

            
            agent_type: agent_type.clone(),
            current_task_description,
            capabilities,
            position,
            cpu_usage,
            memory_usage,
            health: agent_data
                .get("health")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.9) as f32,
            activity: agent_data
                .get("activity")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5) as f32,
            tasks_active: agent_data
                .get("tasks_active")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32,
            tasks_completed: agent_data
                .get("completed_tasks")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            success_rate_normalized,
            tokens: agent_data
                .get("total_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(1500),
            token_rate: agent_data
                .get("token_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.1) as f32,

            
            performance_metrics: PerformanceMetrics {
                tasks_completed: agent_data
                    .get("completed_tasks")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                success_rate: success_rate_normalized,
            },
            token_usage: TokenUsage {
                total: agent_data
                    .get("total_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1500),
                token_rate: agent_data
                    .get("token_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.1) as f32,
            },
            swarm_id: agent_data
                .get("swarm_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            agent_mode: agent_data
                .get("agent_mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            parent_queen_id: agent_data
                .get("parent_queen_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            processing_logs: Some(vec![]),
            created_at,
            age: 0, 
            workload: agent_data
                .get("workload")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
        })
    }

    
    pub async fn send_mcp_request(
        &self,
        request: &McpRequest,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
use crate::utils::time;

        let addr = format!("{}:{}", self.host, self.port);
        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await??;

        
        let json_request = json!({
            "jsonrpc": "2.0",
            "method": request.method,
            "params": request.params,
            "id": uuid::Uuid::new_v4().to_string()
        });

        let request_str = format!("{}\n", json_request.to_string());
        stream.write_all(request_str.as_bytes()).await?;

        
        let mut buffer = vec![0; 16384];
        let bytes_read = timeout(Duration::from_secs(10), stream.read(&mut buffer)).await??;

        if bytes_read == 0 {
            return Err("No response received from TCP server".into());
        }

        let response_str = String::from_utf8_lossy(&buffer[..bytes_read]);

        
        match serde_json::from_str::<serde_json::Value>(&response_str) {
            Ok(json_response) => {
                if let Some(error) = json_response.get("error") {
                    return Err(format!("MCP request failed: {}", error).into());
                }
                Ok(json_response
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null))
            }
            Err(e) => {
                log::error!(
                    "Failed to parse TCP response: {} (raw: {})",
                    e,
                    response_str
                );
                Err(format!("Invalid JSON response: {}", e).into())
            }
        }
    }
}

// Error types
#[derive(Debug)]
pub enum ConnectorError {
    NotConnected,
    NetworkError(String),
    ParseError(String),
}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectorError::NotConnected => write!(f, "Not connected to Claude Flow service"),
            ConnectorError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            ConnectorError::ParseError(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for ConnectorError {}
