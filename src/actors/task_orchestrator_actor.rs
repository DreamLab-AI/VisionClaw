//! Task Orchestrator Actor
//!
//! Actix actor wrapper for ManagementApiClient that provides:
//! - Async task creation with retry logic
//! - Task state caching and tracking
//! - Background polling for task completion
//! - Actor message-based API for integration with VisionClaw actor system
//!
//! ## Architecture
//!
//! ```text
//! VisionClaw API Handler
//!     ↓ (send CreateTask message)
//! TaskOrchestratorActor
//!     ↓ (HTTP POST)
//! ManagementApiClient
//!     ↓ (HTTP request)
//! agentic-workstation:9090/v1/tasks
//! ```

use actix::prelude::*;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::time::Duration;

use crate::services::management_api_client::{
    ManagementApiClient, ManagementApiError, TaskInfo, TaskResponse, TaskState as ApiTaskState,
    TaskStatus as ApiTaskStatus,
};
use crate::utils::time;

#[derive(Debug, Clone)]
pub struct TaskState {
    pub task_id: String,
    pub agent: String,
    pub task_description: String,
    pub provider: String,
    pub status: ApiTaskState,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub retry_count: u32,
    /// ADR-031 gap 5a: JSON-encoded result payload, populated on completion.
    pub result: Option<String>,
}

pub struct TaskOrchestratorActor {
    api_client: ManagementApiClient,
    active_tasks: HashMap<String, TaskState>,
    max_retries: u32,
    retry_delay: Duration,
    /// ADR-031 item 2: Capacity ceiling.
    /// `CreateTask` is rejected immediately when `running_count >= max_concurrent_tasks`,
    /// avoiding wasted HTTP retries. Configurable via `MAX_CONCURRENT_TASKS` env var.
    max_concurrent_tasks: usize,
    /// ADR-031 item 7: When `false` (drain mode) all new task creation is rejected.
    /// Set by `DrainTasksBeforeShutdown`.
    accepting_tasks: bool,
    /// ADR-031 item 3: Optional address for push status notifications.
    /// Set via `SetAgentMonitorAddr`. When present, `TaskStatusChanged` is sent
    /// on every task state transition to trigger an immediate re-poll.
    agent_monitor_addr: Option<Addr<crate::actors::AgentMonitorActor>>,
}

impl TaskOrchestratorActor {
    pub fn new(api_client: ManagementApiClient) -> Self {
        info!("[TaskOrchestratorActor] Initializing");
        let max_concurrent_tasks = std::env::var("MAX_CONCURRENT_TASKS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(20);
        Self {
            api_client,
            active_tasks: HashMap::new(),
            max_retries: 3,
            retry_delay: Duration::from_secs(2),
            max_concurrent_tasks,
            accepting_tasks: true,
            agent_monitor_addr: None,
        }
    }
}

/// Perform HTTP task creation with retry logic.
///
/// This is a free function (not a method) so it can be called from an async
/// block without borrowing the actor. Only the HTTP client and retry
/// parameters are captured — actor state is mutated later via
/// `ResponseActFuture::map`.
async fn create_task_with_retry(
    client: ManagementApiClient,
    agent: &str,
    task: &str,
    provider: &str,
    claude_flow_agent_id: Option<&str>,
    max_retries: u32,
    retry_delay: Duration,
) -> Result<(TaskResponse, u32), ManagementApiError> {
    let mut attempts = 0u32;

    loop {
        match client
            .create_task(agent, task, provider, claude_flow_agent_id)
            .await
        {
            Ok(response) => {
                info!(
                    "[TaskOrchestratorActor] Task created successfully: {}",
                    response.task_id
                );
                return Ok((response, attempts));
            }
            Err(e) => {
                attempts += 1;
                if attempts >= max_retries {
                    error!(
                        "[TaskOrchestratorActor] Task creation failed after {} attempts: {}",
                        attempts, e
                    );
                    return Err(e);
                }

                warn!(
                    "[TaskOrchestratorActor] Task creation attempt {} failed: {}, retrying...",
                    attempts, e
                );
                tokio::time::sleep(retry_delay * attempts).await;
            }
        }
    }
}

impl Actor for TaskOrchestratorActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("[TaskOrchestratorActor] Actor started");

        ctx.address()
            .do_send(crate::actors::messages::InitializeActor);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("[TaskOrchestratorActor] Actor stopped");
    }
}

impl Handler<crate::actors::messages::InitializeActor> for TaskOrchestratorActor {
    type Result = ();

    fn handle(
        &mut self,
        _msg: crate::actors::messages::InitializeActor,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("[TaskOrchestratorActor] Initializing periodic cleanup (deferred from started)");

        ctx.run_interval(Duration::from_secs(300), |act, _ctx| {
            let now = time::now();
            let mut to_remove = Vec::new();

            for (task_id, task) in &act.active_tasks {
                if matches!(
                    task.status,
                    ApiTaskState::Completed | ApiTaskState::Failed | ApiTaskState::Stopped
                ) && (now - task.last_updated).num_minutes() > 5
                {
                    to_remove.push(task_id.clone());
                }
            }

            for task_id in to_remove {
                debug!(
                    "[TaskOrchestratorActor] Removing old task from cache: {}",
                    task_id
                );
                act.active_tasks.remove(&task_id);
            }
        });
    }
}

// ========================================
// Message Definitions
// ========================================

#[derive(Message)]
#[rtype(result = "Result<TaskResponse, String>")]
pub struct CreateTask {
    pub agent: String,
    pub task: String,
    pub provider: String,
    /// D2 (PRD-023 WP-3): the claude-flow swarm `agent_id` this task is created
    /// for, when the caller genuinely knows one. Carried to the Management API so a
    /// later interrupt can join a swarm agent_id → task_id. `None` where no real
    /// claude-flow id exists — do NOT fabricate (a role label / task_id is not a
    /// claude-flow agent id, and a fabricated join re-opens the closed bug).
    pub claude_flow_agent_id: Option<String>,
}

#[derive(Message)]
#[rtype(result = "Result<ApiTaskStatus, String>")]
pub struct GetTaskStatus {
    pub task_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct StopTask {
    pub task_id: String,
}

/// D2 (PRD-023 WP-3): interrupt a steerable agent by an id that may be in
/// EITHER namespace — a Management-API `task_id` OR a claude-flow swarm
/// `agent_id` (the two are disjoint minted namespaces). The orchestrator
/// resolves the id to a concrete Management-API `task_id` before issuing the
/// stop, so a bare swarm `agent_id` from `AgentDetailPanel` no longer 404s the
/// Management API on every interrupt. Returns the resolved `task_id` on success
/// (so the caller can report exactly what was stopped), and a typed
/// [`InterruptError`] — never a blind stop of an unrelated task — otherwise.
#[derive(Message)]
#[rtype(result = "Result<String, InterruptError>")]
pub struct InterruptAgentTask {
    /// A `task_id` or a swarm `agent_id`; resolved server-side.
    pub id: String,
}

/// Typed failure modes for [`InterruptAgentTask`], so the HTTP layer can disclose
/// the honest capability boundary instead of collapsing every miss into a 500.
///
/// The load-bearing distinction is `Unresolved`: an id that maps to no task in
/// the registry and to no task's `claude_flow_agent_id` belongs to an agent with
/// no task-registry provenance — e.g. an MCP-native claude-flow agent surfaced by
/// `agent_list`. Such an agent is **not interruptible from here**: the MCP TCP
/// surface (`crate::utils::mcp_tcp_client`) exposes only `agent_list` /
/// `swarm_status` / `server_info` / `initialize` — there is NO terminate/stop
/// verb — so the client discloses a disabled explanatory state rather than
/// retrying a stop that can never land. `Transport`/`Stop` are operational faults.
#[derive(Debug, Clone, PartialEq)]
pub enum InterruptError {
    /// The id matched neither a `task_id` nor any task's `claude_flow_agent_id`.
    Unresolved(String),
    /// The Management-API task list could not be fetched (resolve step failed).
    Transport(String),
    /// The id resolved to a `task_id` but the stop call itself failed.
    Stop(String),
}

impl std::fmt::Display for InterruptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterruptError::Unresolved(m)
            | InterruptError::Transport(m)
            | InterruptError::Stop(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for InterruptError {}

#[derive(Message)]
#[rtype(result = "Result<Vec<TaskInfo>, String>")]
pub struct ListActiveTasks;

/// ADR-031 gap 5a: Retrieve the stored result payload for a completed task.
#[derive(Message)]
#[rtype(result = "Result<Option<String>, String>")]
pub struct GetTaskResult {
    pub task_id: String,
}

/// ADR-031 gap 5a: Mark a task as completed and store its result payload.
#[derive(Message)]
#[rtype(result = "()")]
pub struct CompleteTask {
    pub task_id: String,
    /// JSON-encoded result payload.
    pub result: Option<String>,
}

#[derive(Message)]
#[rtype(result = "Result<SystemStatusInfo, String>")]
pub struct GetSystemStatus;

#[derive(Debug, Clone)]
pub struct SystemStatusInfo {
    pub active_tasks: usize,
    pub cached_tasks: usize,
    pub api_available: bool,
}

// ========================================
// Message Handlers
// ========================================

impl Handler<CreateTask> for TaskOrchestratorActor {
    type Result = ResponseActFuture<Self, Result<TaskResponse, String>>;

    fn handle(&mut self, msg: CreateTask, _ctx: &mut Self::Context) -> Self::Result {
        // ADR-031 item 7: reject during drain.
        if !self.accepting_tasks {
            return Box::pin(
                async { Err("Task creation rejected: actor is draining".to_string()) }
                    .into_actor(self),
            );
        }

        // ADR-031 item 2: capacity-aware claiming.
        // Count only Running tasks — Completed/Failed entries may linger until cleanup.
        let running_count = self
            .active_tasks
            .values()
            .filter(|t| t.status == ApiTaskState::Running)
            .count();
        if running_count >= self.max_concurrent_tasks {
            warn!(
                "[TaskOrchestratorActor] At capacity ({}/{}) — rejecting task for agent={}",
                running_count, self.max_concurrent_tasks, msg.agent
            );
            let cap = self.max_concurrent_tasks;
            return Box::pin(
                async move {
                    Err(format!(
                        "At capacity: {}/{} tasks running",
                        running_count, cap
                    ))
                }
                .into_actor(self),
            );
        }

        info!(
            "[TaskOrchestratorActor] Received CreateTask: agent={}, provider={}",
            msg.agent, msg.provider
        );

        // Capture only non-actor data for the async block.
        let client = self.api_client.clone();
        let max_retries = self.max_retries;
        let retry_delay = self.retry_delay;
        let agent = msg.agent;
        let task = msg.task;
        let provider = msg.provider;
        let claude_flow_agent_id = msg.claude_flow_agent_id;

        Box::pin(
            async move {
                create_task_with_retry(
                    client,
                    &agent,
                    &task,
                    &provider,
                    claude_flow_agent_id.as_deref(),
                    max_retries,
                    retry_delay,
                )
                .await
                .map(|(response, attempts)| (response, attempts, agent, task, provider))
                .map_err(|e| e.to_string())
            }
            .into_actor(self)
            .map(|result, act, _ctx| {
                // This closure runs with &mut Self — mutations persist.
                match result {
                    Ok((response, attempts, agent, task_desc, provider)) => {
                        let agent_type = agent.clone();
                        act.active_tasks.insert(
                            response.task_id.clone(),
                            TaskState {
                                task_id: response.task_id.clone(),
                                agent,
                                task_description: task_desc,
                                provider,
                                status: ApiTaskState::Running,
                                created_at: time::now(),
                                last_updated: time::now(),
                                retry_count: attempts,
                                result: None,
                            },
                        );

                        // ADR-031 item 3: observational status inference.
                        // Push immediate notification so AgentMonitorActor re-polls
                        // without waiting for its 3 s interval.
                        let running = act
                            .active_tasks
                            .values()
                            .filter(|t| t.status == ApiTaskState::Running && t.agent == agent_type)
                            .count();
                        if let Some(ref addr) = act.agent_monitor_addr {
                            addr.do_send(crate::actors::messages::TaskStatusChanged {
                                agent_type,
                                running_task_count: running,
                            });
                        }

                        Ok(response)
                    }
                    Err(e) => Err(e),
                }
            }),
        )
    }
}

impl Handler<GetTaskStatus> for TaskOrchestratorActor {
    type Result = ResponseFuture<Result<ApiTaskStatus, String>>;

    fn handle(&mut self, msg: GetTaskStatus, _ctx: &mut Self::Context) -> Self::Result {
        debug!(
            "[TaskOrchestratorActor] Received GetTaskStatus: {}",
            msg.task_id
        );

        let client = self.api_client.clone();
        let task_id = msg.task_id.clone();

        Box::pin(async move {
            client
                .get_task_status(&task_id)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

impl Handler<StopTask> for TaskOrchestratorActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: StopTask, _ctx: &mut Self::Context) -> Self::Result {
        info!("[TaskOrchestratorActor] Received StopTask: {}", msg.task_id);

        let client = self.api_client.clone();
        let task_id = msg.task_id.clone();

        Box::pin(async move { client.stop_task(&task_id).await.map_err(|e| e.to_string()) })
    }
}

impl Handler<InterruptAgentTask> for TaskOrchestratorActor {
    type Result = ResponseFuture<Result<String, InterruptError>>;

    fn handle(&mut self, msg: InterruptAgentTask, _ctx: &mut Self::Context) -> Self::Result {
        let id = msg.id.clone();
        info!(
            "[TaskOrchestratorActor] Received InterruptAgentTask: {}",
            id
        );

        // Fast path: the id is already a known Management-API task_id held in the
        // local registry (e.g. a task this orchestrator itself created). No
        // round-trip needed to resolve it.
        let local_hit = self.active_tasks.contains_key(&id);
        let client = self.api_client.clone();

        Box::pin(async move {
            let resolved = if local_hit {
                id.clone()
            } else {
                // The id is (most often) a claude-flow swarm agent_id — a DISJOINT
                // namespace from the Management-API task_id. Resolve it against the
                // live task list. Fail honestly when nothing maps — the caller
                // surfaces a typed error instead of the wrong stop.
                let list = client.list_tasks().await.map_err(|e| {
                    InterruptError::Transport(format!(
                        "interrupt resolve: task list unavailable: {e}"
                    ))
                })?;
                // Resolution order:
                //   1. the id IS a Management-API `task_id`;
                //   2. the id is the task's `claude_flow_agent_id` — the ONLY field
                //      that can carry a claude-flow swarm agent id (the genuine
                //      end-to-end join).
                // The role-label `agent` field is deliberately NEVER matched: it
                // carries "coder"/"researcher"/… and can never equal a claude-flow
                // agent id, so the old `t.agent == id` fallback was a false-positive
                // hazard (a role label colliding with an agent id would stop an
                // unrelated task). D2 re-verification removed it.
                list.active_tasks
                    .iter()
                    .find(|t| t.task_id == id)
                    .or_else(|| {
                        list.active_tasks
                            .iter()
                            .find(|t| t.claude_flow_agent_id.as_deref() == Some(id.as_str()))
                    })
                    .map(|t| t.task_id.clone())
                    .ok_or_else(|| {
                        InterruptError::Unresolved(format!("no active task resolves id '{id}'"))
                    })?
            };

            client
                .stop_task(&resolved)
                .await
                .map(|()| resolved)
                .map_err(|e| InterruptError::Stop(e.to_string()))
        })
    }
}

impl Handler<ListActiveTasks> for TaskOrchestratorActor {
    type Result = ResponseFuture<Result<Vec<TaskInfo>, String>>;

    fn handle(&mut self, _msg: ListActiveTasks, _ctx: &mut Self::Context) -> Self::Result {
        debug!("[TaskOrchestratorActor] Received ListActiveTasks");

        let client = self.api_client.clone();

        Box::pin(async move {
            client
                .list_tasks()
                .await
                .map(|response| response.active_tasks)
                .map_err(|e| e.to_string())
        })
    }
}

impl Handler<GetSystemStatus> for TaskOrchestratorActor {
    type Result = ResponseFuture<Result<SystemStatusInfo, String>>;

    fn handle(&mut self, _msg: GetSystemStatus, _ctx: &mut Self::Context) -> Self::Result {
        debug!("[TaskOrchestratorActor] Received GetSystemStatus");

        let client = self.api_client.clone();
        let cached_tasks = self.active_tasks.len();

        Box::pin(async move {
            match client.get_system_status().await {
                Ok(status) => Ok(SystemStatusInfo {
                    active_tasks: status.tasks.active as usize,
                    cached_tasks,
                    api_available: true,
                }),
                Err(e) => {
                    warn!("[TaskOrchestratorActor] Failed to get system status: {}", e);
                    Ok(SystemStatusInfo {
                        active_tasks: 0,
                        cached_tasks,
                        api_available: false,
                    })
                }
            }
        })
    }
}

// ========================================
// ADR-031 gap 5a: Task result storage handlers
// ========================================

impl Handler<GetTaskResult> for TaskOrchestratorActor {
    type Result = Result<Option<String>, String>;

    fn handle(&mut self, msg: GetTaskResult, _ctx: &mut Self::Context) -> Self::Result {
        match self.active_tasks.get(&msg.task_id) {
            Some(task) => {
                if task.status == ApiTaskState::Completed {
                    Ok(task.result.clone())
                } else if task.status == ApiTaskState::Running {
                    // Task still running — no result yet.
                    Ok(None)
                } else {
                    // Failed or other terminal state — return whatever we have.
                    Ok(task.result.clone())
                }
            }
            None => Err(format!("Task not found: {}", msg.task_id)),
        }
    }
}

impl Handler<CompleteTask> for TaskOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: CompleteTask, _ctx: &mut Self::Context) {
        if let Some(task) = self.active_tasks.get_mut(&msg.task_id) {
            task.status = ApiTaskState::Completed;
            task.last_updated = time::now();
            task.result = msg.result;

            info!(
                "[TaskOrchestratorActor] Task {} completed (result payload: {} bytes)",
                msg.task_id,
                task.result.as_ref().map_or(0, |r| r.len())
            );

            // ADR-031 item 3: push notification to AgentMonitorActor.
            let agent_type = task.agent.clone();
            let running = self
                .active_tasks
                .values()
                .filter(|t| t.status == ApiTaskState::Running && t.agent == agent_type)
                .count();
            if let Some(ref addr) = self.agent_monitor_addr {
                addr.do_send(crate::actors::messages::TaskStatusChanged {
                    agent_type,
                    running_task_count: running,
                });
            }
        } else {
            warn!(
                "[TaskOrchestratorActor] CompleteTask for unknown task_id={}",
                msg.task_id
            );
        }
    }
}

// ========================================
// ADR-031: Orchestration Improvement Messages
// ========================================

/// ADR-031 item 7: Graceful task drain.
///
/// On receipt the actor stops accepting new tasks (`accepting_tasks = false`)
/// and polls every second until all running tasks finish or the timeout
/// elapses, then stops itself. Adapted from Multica's WaitGroup-based 30-second
/// drain on daemon shutdown.
#[derive(Message)]
#[rtype(result = "()")]
pub struct DrainTasksBeforeShutdown {
    /// Maximum seconds to wait for running tasks to complete.
    pub timeout_secs: u64,
}

impl Handler<DrainTasksBeforeShutdown> for TaskOrchestratorActor {
    type Result = ();

    fn handle(&mut self, msg: DrainTasksBeforeShutdown, ctx: &mut Self::Context) {
        let running = self
            .active_tasks
            .values()
            .filter(|t| t.status == ApiTaskState::Running)
            .count();
        info!(
            "[TaskOrchestratorActor] Drain initiated — rejecting new tasks, \
             waiting up to {}s for {} running task(s)",
            msg.timeout_secs, running
        );
        self.accepting_tasks = false;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(msg.timeout_secs);

        ctx.run_interval(std::time::Duration::from_secs(1), move |act, ctx| {
            let remaining = act
                .active_tasks
                .values()
                .filter(|t| t.status == ApiTaskState::Running)
                .count();

            if remaining == 0 {
                info!("[TaskOrchestratorActor] All tasks drained — stopping actor");
                ctx.stop();
            } else if std::time::Instant::now() >= deadline {
                warn!(
                    "[TaskOrchestratorActor] Drain timeout exceeded with {} task(s) still running — stopping",
                    remaining
                );
                ctx.stop();
            }
        });
    }
}

/// ADR-031 item 3: Register `AgentMonitorActor` for push status notifications.
impl Handler<crate::actors::messages::SetAgentMonitorAddr> for TaskOrchestratorActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::SetAgentMonitorAddr,
        _ctx: &mut Self::Context,
    ) {
        info!(
            "[TaskOrchestratorActor] AgentMonitorActor address registered for status notifications"
        );
        self.agent_monitor_addr = Some(msg.addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::management_api_client::ManagementApiClient;

    #[test]
    fn test_actor_creation() {
        let client = ManagementApiClient::new(
            "agentic-workstation".to_string(),
            9090,
            "test-key".to_string(),
        );

        let actor = TaskOrchestratorActor::new(client);
        assert_eq!(actor.max_retries, 3);
        assert_eq!(actor.active_tasks.len(), 0);
    }
}
