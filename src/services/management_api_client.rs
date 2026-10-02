//! Management API Client for Multi-Agent Docker Container
//!
//! This client provides HTTP access to the Management API (port 9090) running in the
//! agentic-workstation container. It handles all task creation, monitoring, and control
//! operations, replacing the legacy DockerHiveMind system.
//!
//! ## Architecture
//!
//! VisionClaw Container → ManagementApiClient (HTTP) → agentic-workstation:9090 → Management API
//!
//! ## Features
//!
//! - Task creation via POST /v1/tasks
//! - Task status polling via GET /v1/tasks/:taskId
//! - Task cancellation via DELETE /v1/tasks/:taskId
//! - System status via GET /v1/status
//! - Automatic retry with exponential backoff
//! - Bearer token authentication

use crate::types::user_context::UserContext;
use log::{debug, info};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct ManagementApiClient {
    base_url: String,
    api_key: String,
    client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResponse {
    pub task_id: String,
    pub status: String,
    pub message: String,
    pub task_dir: Option<String>,
    pub log_file: Option<String>,
    pub start_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub task_id: String,
    pub agent: String,
    pub task: String,
    pub provider: String,
    pub status: TaskState,
    pub start_time: u64,
    pub exit_time: Option<u64>,
    pub exit_code: Option<i32>,
    pub duration: u64,
    pub task_dir: String,
    pub log_file: String,
    pub log_tail: Option<String>,
    /// D2 (PRD-023 WP-3): the claude-flow swarm `agent_id` this task was created
    /// for, when the creator knew one — echoed back by the Management API. `agent`
    /// carries a role label ("coder"/"researcher"/…) and can NEVER be a claude-flow
    /// id; this field is the ONLY carrier of a claude-flow agent id. `None` when the
    /// task had no registry-provenance claude-flow agent.
    #[serde(default)]
    pub claude_flow_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub task_id: String,
    pub agent: String,
    pub task: String,
    pub provider: String,
    pub status: TaskState,
    pub start_time: u64,
    pub duration: u64,
    /// D2 (PRD-023 WP-3): the claude-flow swarm `agent_id` this task was created
    /// for, when the creator knew one — echoed back by the Management API's
    /// `GET /v1/tasks`. This is the ONLY field the interrupt resolver joins on: the
    /// `agent` field carries a role label ("coder"/"researcher"/…) and can never
    /// equal a claude-flow agent id, so matching `agent` was a false-positive
    /// hazard. `None` for role-only spawns (no interruptible-by-agent-id join).
    #[serde(default)]
    pub claude_flow_agent_id: Option<String>,
    /// The task's agent `did:nostr`, when the Management API echoes one
    /// (`didNostr`, or `did_nostr` as on the spawn response). Unvalidated here;
    /// the monitor carries it only after the `uri::did_nostr()` round-trip, and
    /// sidechain payment edges attach only to agents that have one (S5).
    #[serde(default, alias = "did_nostr")]
    pub did_nostr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListResponse {
    pub active_tasks: Vec<TaskInfo>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub timestamp: String,
    pub api: ApiStatus,
    pub tasks: TasksStatus,
    pub gpu: Option<GpuStatus>,
    pub providers: serde_json::Value,
    pub system: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiStatus {
    pub uptime: u64,
    pub version: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasksStatus {
    pub active: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuStatus {
    pub available: bool,
    pub gpus: Option<Vec<serde_json::Value>>,
}

// --- Briefing workflow response types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefResponse {
    pub brief_id: String,
    pub brief_path: String,
    pub bead_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteBriefResponse {
    pub brief_id: String,
    pub role_tasks: Vec<crate::types::user_context::RoleTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebriefResponse {
    pub debrief_path: String,
}

#[derive(Debug)]
pub enum ManagementApiError {
    NetworkError(String),
    ApiError(String, StatusCode),
    DeserializationError(String),
    Timeout,
}

impl std::fmt::Display for ManagementApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManagementApiError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            ManagementApiError::ApiError(msg, status) => {
                write!(f, "API error ({}): {}", status, msg)
            }
            ManagementApiError::DeserializationError(msg) => {
                write!(f, "Deserialization error: {}", msg)
            }
            ManagementApiError::Timeout => write!(f, "Request timeout"),
        }
    }
}

impl std::error::Error for ManagementApiError {}

impl ManagementApiClient {
    pub fn new(host: String, port: u16, api_key: String) -> Self {
        let base_url = format!("http://{}:{}", host, port);

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        info!(
            "[ManagementApiClient] Initialized with base_url: {}",
            base_url
        );

        Self {
            base_url,
            api_key,
            client,
        }
    }

    pub async fn create_task(
        &self,
        agent: &str,
        task: &str,
        provider: &str,
        claude_flow_agent_id: Option<&str>,
    ) -> Result<TaskResponse, ManagementApiError> {
        self.create_task_with_context(
            agent,
            task,
            provider,
            None,
            false,
            None,
            claude_flow_agent_id,
        )
        .await
    }

    /// Create a task with full user context and beads integration.
    ///
    /// When `user_context` is provided, the Management API will:
    /// - Create a user-scoped workspace directory
    /// - Inject VISIONCLAW_USER_* env vars into the agent process
    /// - Optionally create a Beads epic for task tracking (if `with_beads` is true)
    ///
    /// D2 (PRD-023 WP-3): `claude_flow_agent_id` is the OPTIONAL, additive
    /// claude-flow swarm `agent_id` this task is created for. The Management API
    /// persists it on the task record and echoes it in `GET /v1/tasks` / task
    /// status, so the interrupt resolver can join a swarm agent_id → task_id. Pass
    /// `None` where no genuine claude-flow id exists — a role label or a
    /// Management-API task_id is NOT a claude-flow agent id, and fabricating one
    /// would re-introduce the false-positive the join exists to close.
    pub async fn create_task_with_context(
        &self,
        agent: &str,
        task: &str,
        provider: &str,
        user_context: Option<&UserContext>,
        with_beads: bool,
        parent_bead_id: Option<&str>,
        claude_flow_agent_id: Option<&str>,
    ) -> Result<TaskResponse, ManagementApiError> {
        let url = format!("{}/v1/tasks", self.base_url);

        let mut request_body = serde_json::json!({
            "agent": agent,
            "task": task,
            "provider": provider,
        });

        // D2: additive snake_case join key, matching the agentbox create-task
        // schema. Only present when the creator genuinely knew a claude-flow id.
        if let Some(cfa) = claude_flow_agent_id {
            request_body["claude_flow_agent_id"] = serde_json::json!(cfa);
        }

        if let Some(ctx) = user_context {
            request_body["user_context"] = serde_json::json!({
                "user_id": ctx.user_id,
                "pubkey": ctx.pubkey,
                "display_name": ctx.display_name,
                "session_id": ctx.session_id,
                "is_power_user": ctx.is_power_user,
            });
        }

        if with_beads {
            request_body["with_beads"] = serde_json::json!(true);
        }

        if let Some(parent_id) = parent_bead_id {
            request_body["parent_bead_id"] = serde_json::json!(parent_id);
        }

        debug!(
            "[ManagementApiClient] Creating task: agent={}, provider={}, user={:?}",
            agent,
            provider,
            user_context.map(|c| &c.display_name)
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();

        if status == StatusCode::ACCEPTED || status == StatusCode::OK {
            let task_response: TaskResponse = response
                .json()
                .await
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()))?;

            info!(
                "[ManagementApiClient] Task created: {}",
                task_response.task_id
            );
            Ok(task_response)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(ManagementApiError::ApiError(error_text, status))
        }
    }

    pub async fn get_task_status(&self, task_id: &str) -> Result<TaskStatus, ManagementApiError> {
        let url = format!("{}/v1/tasks/{}", self.base_url, task_id);

        debug!("[ManagementApiClient] Getting task status: {}", task_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();

        if status == StatusCode::OK {
            let task_status: TaskStatus = response
                .json()
                .await
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()))?;
            Ok(task_status)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(ManagementApiError::ApiError(error_text, status))
        }
    }

    pub async fn list_tasks(&self) -> Result<TaskListResponse, ManagementApiError> {
        let url = format!("{}/v1/tasks", self.base_url);

        debug!("[ManagementApiClient] Listing tasks");

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();

        if status == StatusCode::OK {
            let task_list: TaskListResponse = response
                .json()
                .await
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()))?;
            Ok(task_list)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(ManagementApiError::ApiError(error_text, status))
        }
    }

    /// Sidechain payments between agents (`GET /v1/chain/payments`, S5).
    ///
    /// `Ok(None)` when the route is not deployed (404), so a Management API that
    /// predates the route is quiet rather than a poll failure. The body is the
    /// contract in [`crate::services::chain_payments`].
    pub async fn get_chain_payments(
        &self,
    ) -> Result<Option<crate::services::chain_payments::ChainPaymentsResponse>, ManagementApiError>
    {
        let url = format!(
            "{}{}",
            self.base_url,
            crate::services::chain_payments::CHAIN_PAYMENTS_PATH
        );
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == StatusCode::OK {
            return response
                .json()
                .await
                .map(Some)
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()));
        }
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        Err(ManagementApiError::ApiError(error_text, status))
    }

    pub async fn stop_task(&self, task_id: &str) -> Result<(), ManagementApiError> {
        let url = format!("{}/v1/tasks/{}", self.base_url, task_id);

        info!("[ManagementApiClient] Stopping task: {}", task_id);

        let response = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();

        if status == StatusCode::OK {
            info!("[ManagementApiClient] Task stopped: {}", task_id);
            Ok(())
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(ManagementApiError::ApiError(error_text, status))
        }
    }

    pub async fn get_system_status(&self) -> Result<SystemStatus, ManagementApiError> {
        let url = format!("{}/v1/status", self.base_url);

        debug!("[ManagementApiClient] Getting system status");

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();

        if status == StatusCode::OK {
            let system_status: SystemStatus = response
                .json()
                .await
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()))?;
            Ok(system_status)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(ManagementApiError::ApiError(error_text, status))
        }
    }

    /// Create a brief via the Management API briefing workflow.
    pub async fn create_brief(
        &self,
        content: &str,
        roles: &[String],
        user_context: &UserContext,
        version: Option<&str>,
        brief_type: Option<&str>,
        slug: Option<&str>,
    ) -> Result<BriefResponse, ManagementApiError> {
        let url = format!("{}/v1/briefs", self.base_url);

        let mut body = serde_json::json!({
            "content": content,
            "roles": roles,
            "user_context": {
                "user_id": user_context.user_id,
                "pubkey": user_context.pubkey,
                "display_name": user_context.display_name,
                "session_id": user_context.session_id,
                "is_power_user": user_context.is_power_user,
            }
        });

        if let Some(v) = version {
            body["version"] = serde_json::json!(v);
        }
        if let Some(bt) = brief_type {
            body["brief_type"] = serde_json::json!(bt);
        }
        if let Some(s) = slug {
            body["slug"] = serde_json::json!(s);
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();
        if status == StatusCode::CREATED || status == StatusCode::OK {
            response
                .json()
                .await
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()))
        } else {
            let err = response.text().await.unwrap_or_default();
            Err(ManagementApiError::ApiError(err, status))
        }
    }

    /// Execute a brief — spawn role-specific agents.
    pub async fn execute_brief(
        &self,
        brief_id: &str,
        brief_path: &str,
        roles: &[String],
        user_context: &UserContext,
        epic_bead_id: Option<&str>,
    ) -> Result<Vec<crate::types::user_context::RoleTask>, ManagementApiError> {
        let url = format!("{}/v1/briefs/{}/execute", self.base_url, brief_id);

        let mut body = serde_json::json!({
            "brief_path": brief_path,
            "roles": roles,
            "user_context": {
                "user_id": user_context.user_id,
                "pubkey": user_context.pubkey,
                "display_name": user_context.display_name,
                "session_id": user_context.session_id,
                "is_power_user": user_context.is_power_user,
            }
        });

        if let Some(eid) = epic_bead_id {
            body["epic_bead_id"] = serde_json::json!(eid);
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();
        if status == StatusCode::ACCEPTED || status == StatusCode::OK {
            let result: ExecuteBriefResponse = response
                .json()
                .await
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()))?;
            Ok(result.role_tasks)
        } else {
            let err = response.text().await.unwrap_or_default();
            Err(ManagementApiError::ApiError(err, status))
        }
    }

    /// Create a consolidated debrief from role responses.
    pub async fn create_debrief(
        &self,
        brief_id: &str,
        role_tasks: &[crate::types::user_context::RoleTask],
        user_context: &UserContext,
    ) -> Result<String, ManagementApiError> {
        let url = format!("{}/v1/briefs/{}/debrief", self.base_url, brief_id);

        let body = serde_json::json!({
            "role_responses": role_tasks.iter().map(|rt| serde_json::json!({
                "role": rt.role,
                "responsePath": rt.response_path,
                "taskId": rt.task_id,
                "status": if rt.bead_id.is_some() { "completed" } else { "pending" }
            })).collect::<Vec<_>>(),
            "user_context": {
                "user_id": user_context.user_id,
                "pubkey": user_context.pubkey,
                "display_name": user_context.display_name,
                "session_id": user_context.session_id,
                "is_power_user": user_context.is_power_user,
            }
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        let status = response.status();
        if status == StatusCode::CREATED || status == StatusCode::OK {
            let result: DebriefResponse = response
                .json()
                .await
                .map_err(|e| ManagementApiError::DeserializationError(e.to_string()))?;
            Ok(result.debrief_path)
        } else {
            let err = response.text().await.unwrap_or_default();
            Err(ManagementApiError::ApiError(err, status))
        }
    }

    pub async fn health_check(&self) -> Result<bool, ManagementApiError> {
        let url = format!("{}/health", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ManagementApiError::NetworkError(e.to_string()))?;

        Ok(response.status() == StatusCode::OK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = ManagementApiClient::new(
            "agentic-workstation".to_string(),
            9090,
            "test-key".to_string(),
        );

        assert_eq!(client.base_url, "http://agentic-workstation:9090");
        assert_eq!(client.api_key, "test-key");
    }
}
