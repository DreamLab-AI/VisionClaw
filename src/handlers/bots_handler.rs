use crate::actors::messages::GetBotsGraphData;
use crate::actors::{CreateTask, GetTaskStatus, InterruptAgentTask, InterruptError, StopTask};
use crate::services::bots_client::{Agent, BotsClient};
use crate::services::liveness_harness::CANARY_D2_STEER;
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::AppState;
use actix_web::{web, HttpResponse, Responder, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::metadata::MetadataStore;
use visionclaw_domain::models::node::Node;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotsDataRequest {
    pub nodes: Vec<Agent>,
    pub edges: Vec<serde_json::Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotsResponse {
    pub success: bool,
    pub message: String,
    pub nodes: Option<Vec<Node>>,
    pub edges: Option<Vec<Edge>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeSwarmRequest {
    pub topology: String,
    pub max_agents: u32,
    pub strategy: String,
    pub enable_neural: bool,
    pub agent_types: Vec<String>,
    pub custom_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsCommandRequest {
    /// Natural-language view/graph configuration request from the command box.
    pub command: String,
    /// Flattened settings catalogue (path | label | range | current | description)
    /// assembled client-side from UNIFIED_SETTINGS_CONFIG + live store values.
    pub settings_context: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnAgentHybridRequest {
    pub agent_type: String,
    pub swarm_id: String,
    pub method: String,
    pub priority: Option<String>,
    pub strategy: Option<String>,
    pub config: Option<SpawnAgentConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnAgentConfig {
    pub auto_scale: Option<bool>,
    pub monitor: Option<bool>,
    pub max_workers: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnAgentResponse {
    pub success: bool,
    pub swarm_id: Option<String>,
    pub error: Option<String>,
    pub method_used: Option<String>,
    pub message: Option<String>,
}

// Static bots graph data storage
use crate::{accepted, error_json, ok_json};
use once_cell::sync::Lazy;

static BOTS_GRAPH: Lazy<Arc<RwLock<GraphData>>> =
    Lazy::new(|| Arc::new(RwLock::new(GraphData::new())));
static CURRENT_SWARM_ID: Lazy<Arc<RwLock<Option<String>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

// Legacy converter functions removed - agent data now comes from MCP TCP via AgentMonitorActor

pub async fn fetch_hive_mind_agents(
    state: &AppState,
    _hybrid_manager: Option<()>,
) -> Result<Vec<Agent>, Box<dyn std::error::Error>> {
    match state.bots_client.get_agents_snapshot().await {
        Ok(agents) => {
            info!("Retrieved {} agents from BotsClient cache", agents.len());
            Ok(agents)
        }
        Err(e) => {
            error!("Failed to get agents from BotsClient: {}", e);
            Err(e.into())
        }
    }
}

// Enhanced agent to nodes conversion with hive-mind properties and Queen agent special handling
fn convert_agents_to_nodes(agents: Vec<Agent>) -> Vec<Node> {
    agents
        .into_iter()
        .enumerate()
        .map(|(idx, agent)| {
            let node_id = (idx + 1000) as u32;

            let (_radius, vertical_offset) = match agent.agent_type.as_str() {
                "queen" => (0.0, 0.0),
                "coordinator" => (20.0, 2.0),
                "researcher" => (30.0, 0.0),
                "analyst" => (30.0, 0.0),
                "coder" => (40.0, -1.0),
                "optimizer" => (40.0, -1.0),
                "tester" => (50.0, -2.0),
                _ => (60.0, -3.0),
            };

            let (color, size) = match agent.agent_type.as_str() {
                "queen" => ("#FFD700", 25.0),
                "coordinator" => ("#FF6B6B", 20.0),
                "researcher" => ("#4ECDC4", 18.0),
                "analyst" => ("#45B7D1", 18.0),
                "coder" => ("#95E1D3", 16.0),
                "optimizer" => ("#F38181", 16.0),
                "tester" => ("#F6B93B", 14.0),
                "worker" => ("#B8E994", 12.0),
                _ => ("#DFE4EA", 10.0),
            };

            Node {
                id: node_id,
                metadata_id: agent.id.clone(),
                label: format!("{} ({})", agent.name, agent.agent_type),
                data: BinaryNodeData {
                    node_id,
                    x: agent.x,
                    y: agent.y + vertical_offset,
                    z: agent.z,
                    vx: 0.0,
                    vy: 0.0,
                    vz: 0.0,
                }
                .into(),
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("agent_type".to_string(), agent.agent_type.clone());
                    meta.insert("name".to_string(), agent.name.clone());
                    meta.insert("status".to_string(), agent.status.clone());
                    meta.insert("cpu_usage".to_string(), agent.cpu_usage.to_string());
                    meta.insert("memory_usage".to_string(), agent.memory_usage.to_string());
                    meta.insert("health".to_string(), agent.health.to_string());
                    meta.insert("workload".to_string(), agent.workload.to_string());
                    if let Some(age) = agent.age {
                        meta.insert("age".to_string(), age.to_string());
                    }
                    meta
                },
                file_size: 0,
                node_type: Some("agent".to_string()),
                size: Some(size),
                color: Some(color.to_string()),
                group: None,
                user_data: None,
                weight: Some(1.0),
                mass: Some(1.0),
                x: Some(agent.x),
                y: Some(agent.y + vertical_offset),
                z: Some(agent.z),
                vx: Some(0.0),
                vy: Some(0.0),
                vz: Some(0.0),
                owl_class_iri: None,
            }
        })
        .collect()
}

pub async fn update_bots_graph(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    request: web::Json<BotsDataRequest>,
    _state: web::Data<AppState>,
) -> Result<impl Responder> {
    info!(
        "Received bots graph update with {} nodes",
        request.nodes.len()
    );

    let nodes = convert_agents_to_nodes(request.nodes.clone());
    let edges = vec![];

    let mut graph = BOTS_GRAPH.write().await;
    graph.nodes = nodes;
    graph.edges = edges;
    graph.metadata = MetadataStore::default();

    ok_json!(BotsResponse {
        success: true,
        message: "Graph updated successfully".to_string(),
        nodes: Some(graph.nodes.clone()),
        edges: Some(graph.edges.clone()),
    })
}

pub async fn get_bots_data(state: web::Data<AppState>) -> Result<impl Responder> {
    // S5: the sidechain payment projection rides alongside the agent graph:
    // the payments panel, the node badges' anchor state and the rehearsal
    // script's settled-figure diff all read this one object.
    let chain = state
        .agent_monitor_addr
        .send(crate::actors::messages::GetChainPaymentsView)
        .await
        .unwrap_or_default();

    if let Ok(Ok(graph)) = state.graph_service_addr.send(GetBotsGraphData).await {
        let nodes = &graph.nodes;
        let edges = &graph.edges;
        if !nodes.is_empty() {
            info!(
                "Retrieved bots data from graph actor: {} nodes",
                nodes.len()
            );
            return ok_json!(json!({
                "success": true,
                "nodes": nodes,
                "edges": edges,
                "chain": chain,
            }));
        }
    }

    let graph = BOTS_GRAPH.read().await;
    info!(
        "Retrieved bots data from static storage: {} nodes",
        graph.nodes.len()
    );

    ok_json!(json!({
        "success": true,
        "nodes": graph.nodes.clone(),
        "edges": graph.edges.clone(),
        "metadata": graph.metadata,
        "chain": chain,
    }))
}

pub async fn initialize_hive_mind_swarm(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    request: web::Json<InitializeSwarmRequest>,
    state: web::Data<AppState>,
    _hybrid_manager: Option<()>,
) -> Result<impl Responder> {
    info!(
        "🐝 Initializing hive mind swarm via Management API with topology: {}",
        request.topology
    );

    let base_task = if let Some(custom_prompt) = &request.custom_prompt {
        if !custom_prompt.trim().is_empty() {
            custom_prompt.trim().to_string()
        } else {
            format!(
                "Initialize {} swarm with {} strategy and {} agents. Agent types: {}. Neural enabled: {}",
                request.topology,
                request.strategy,
                request.max_agents,
                request.agent_types.join(", "),
                request.enable_neural
            )
        }
    } else {
        format!(
            "Initialize {} swarm with {} strategy and {} agents. Agent types: {}. Neural enabled: {}",
            request.topology,
            request.strategy,
            request.max_agents,
            request.agent_types.join(", "),
            request.enable_neural
        )
    };

    let task = format!(
        "{}\n\n**IMPORTANT COMMUNICATION PROTOCOL:**\n\
        Messages will be displayed in the user's telemetry panel in real-time.\n\
        Use this for progress updates, decisions, questions, results, and errors.",
        base_task
    );

    info!("Swarm initialization task: {}", task);

    let agent_type = match request.strategy.as_str() {
        "strategic" => "planner",
        "tactical" => "coder",
        "adaptive" => "researcher",
        _ => "coder",
    };

    let provider = std::env::var("PRIMARY_PROVIDER").unwrap_or_else(|_| "gemini".to_string());

    let create_task_msg = CreateTask {
        agent: agent_type.to_string(),
        task: task.clone(),
        provider: provider.clone(),
        // Hive-mind init spawns by role; no distinct claude-flow agent id exists
        // at creation time. None (do not fabricate).
        claude_flow_agent_id: None,
    };

    match state
        .get_task_orchestrator_addr()
        .send(create_task_msg)
        .await
    {
        Ok(Ok(task_response)) => {
            info!(
                "Successfully created task via Management API - Task ID: {}",
                task_response.task_id
            );

            {
                let mut current_id = CURRENT_SWARM_ID.write().await;
                *current_id = Some(task_response.task_id.clone());
            }

            accepted!(json!({
                "success": true,
                "message": "Hive mind swarm task created. Agents will appear shortly.",
                "task_id": task_response.task_id,
                "topology": request.topology,
                "strategy": request.strategy,
                "agent_types": request.agent_types,
                "max_agents": request.max_agents,
                "enable_neural": request.enable_neural,
                "provider": provider,
            }))
        }
        Ok(Err(e)) => {
            error!("✗ Failed to create swarm task: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Failed to create task: {}", e),
                "topology": request.topology,
                "strategy": request.strategy,
            })))
        }
        Err(e) => {
            error!("✗ Actor communication error: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Actor communication error: {}", e),
            })))
        }
    }
}

pub async fn get_bots_connection_status(state: web::Data<AppState>) -> Result<impl Responder> {
    match state.bots_client.get_status().await {
        Ok(status) => ok_json!(status),
        Err(e) => error_json!("Failed to get bots status: {}", e),
    }
}

pub async fn get_bots_agents(
    state: web::Data<AppState>,
    _hybrid_manager: Option<()>,
) -> Result<impl Responder> {
    match fetch_hive_mind_agents(&state, None).await {
        Ok(agents) => ok_json!(json!({
            "success": true,
            "agents": agents,
            "count": agents.len(),
        })),
        Err(e) => Ok(HttpResponse::InternalServerError().json(json!({
            "success": false,
            "error": format!("Failed to fetch agents: {}", e)
        }))),
    }
}

// Structure for bot node data used by socket handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotsNodeData {
    pub id: u32,
    pub data: BotData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotData {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
}

pub async fn spawn_agent_hybrid(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    req: web::Json<SpawnAgentHybridRequest>,
) -> Result<impl Responder> {
    info!("Spawning agent via Management API: {:?}", req);

    let task = format!("Spawn {} agent for swarm {}", req.agent_type, req.swarm_id);
    let provider = std::env::var("PRIMARY_PROVIDER").unwrap_or_else(|_| "gemini".to_string());

    let create_task_msg = CreateTask {
        agent: req.agent_type.clone(),
        task,
        provider: provider.clone(),
        // `spawn_agent_hybrid` mints `swarm_id = task_response.task_id` (see the
        // response below), so the spawned agent is already resolvable via the
        // task_id path — there is no separate claude-flow agent id to carry, and a
        // task_id is NOT a claude-flow agent id. None (do not fabricate the join).
        claude_flow_agent_id: None,
    };

    match state
        .get_task_orchestrator_addr()
        .send(create_task_msg)
        .await
    {
        Ok(Ok(task_response)) => {
            info!(
                "Successfully spawned {} agent via Management API - Task ID: {}",
                req.agent_type, task_response.task_id
            );
            accepted!(SpawnAgentResponse {
                success: true,
                swarm_id: Some(task_response.task_id),
                error: None,
                method_used: Some("management-api".to_string()),
                message: Some(format!(
                    "Successfully spawned {} agent via Management API",
                    req.agent_type
                )),
            })
        }
        Ok(Err(e)) => {
            error!("Failed to spawn {} agent: {}", req.agent_type, e);
            Ok(
                HttpResponse::InternalServerError().json(SpawnAgentResponse {
                    success: false,
                    swarm_id: None,
                    error: Some(format!("Failed to create task: {}", e)),
                    method_used: None,
                    message: None,
                }),
            )
        }
        Err(e) => {
            error!("Actor communication error: {}", e);
            Ok(
                HttpResponse::InternalServerError().json(SpawnAgentResponse {
                    success: false,
                    swarm_id: None,
                    error: Some(format!("Actor communication error: {}", e)),
                    method_used: None,
                    message: None,
                }),
            )
        }
    }
}

/// Settings-aware LLM command handler. Reuses the existing CreateTask transport
/// (TaskOrchestratorActor → ManagementApiClient → agentbox /v1/tasks) rather than
/// introducing a new path to the agents. The agent receives the live settings +
/// descriptions context and applies changes back through the existing
/// /api/settings/* REST API.
/// Build the settings-assistant LLM task prompt. Shared by the Control
/// Center command box (HTTP, with the client's live settings catalogue) and
/// the voice path (`VoiceInterfaceActor`, no catalogue — the agent is told to
/// fetch current values itself).
pub fn settings_assistant_task(command: &str, context: &str, settings_base: &str) -> String {
    let catalogue = if context.trim().is_empty() {
        format!(
            "(no catalogue attached — fetch the current settings with an HTTP GET to \
             {settings_base}/api/settings/all before deciding what to change)"
        )
    } else {
        context.to_string()
    };
    format!(
        "You are the VisionClaw graph settings assistant. The user issued this \
view/graph configuration request via the in-app command box:\n\n\
\"{command}\"\n\n\
Below is the live settings catalogue. Each line is:\n  \
<path> :: <label> (<type>) range=[min..max step S] :: current=<value> :: <description>\n\n\
{catalogue}\n\n\
TASK: Decide which setting path(s) best satisfy the request and compute new values \
within the stated ranges. Apply EACH change with an HTTP PUT to \
{settings_base}/api/settings/<path> with JSON body {{\"value\": <new_value>}} \
(use the dotted path verbatim, e.g. \
visualisation.graphs.knowledge.physics.springKKnowledge). \
Only change settings clearly implied by the request; do not touch unrelated paths. \
If the request is not a view/graph configuration change, make no changes. \
After applying, report a one-line summary of each path you changed and its new value.\n\n\
**COMMUNICATION PROTOCOL:** Messages are shown in the user's telemetry panel in \
real-time. Use it for progress, decisions, and results."
    )
}

pub async fn process_settings_command(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    req: web::Json<SettingsCommandRequest>,
) -> Result<impl Responder> {
    let command = req.command.trim().to_string();
    if command.is_empty() {
        return Ok(HttpResponse::BadRequest().json(json!({
            "success": false,
            "error": "command is empty",
        })));
    }

    // Base URL the agentbox agent should hit to apply settings. Defaults to the
    // visionclaw container's backend on the shared docker network.
    let settings_base = std::env::var("VISIONCLAW_INTERNAL_URL")
        .unwrap_or_else(|_| "http://visionclaw_container:4000".to_string());

    // agentbox /v1/tasks rejects task strings over 10000 chars. The static
    // template below is ~1KB, so cap the catalogue at 8500 chars (char-boundary
    // safe) as a defensive net in case the client sends an oversized context.
    let mut context = req.settings_context.clone().unwrap_or_default();
    const MAX_CONTEXT: usize = 8500;
    if context.len() > MAX_CONTEXT {
        let mut end = MAX_CONTEXT;
        while end > 0 && !context.is_char_boundary(end) {
            end -= 1;
        }
        context.truncate(end);
        context.push_str("\n…(catalogue truncated to fit task limit)");
    }

    let task = settings_assistant_task(&command, &context, &settings_base);

    let provider = std::env::var("PRIMARY_PROVIDER").unwrap_or_else(|_| "gemini".to_string());

    let create_task_msg = CreateTask {
        agent: "researcher".to_string(),
        task,
        provider: provider.clone(),
        // Settings-assistant dispatch is role-only; no claude-flow agent id exists.
        claude_flow_agent_id: None,
    };

    match state
        .get_task_orchestrator_addr()
        .send(create_task_msg)
        .await
    {
        Ok(Ok(task_response)) => {
            info!(
                "Settings command dispatched via Management API - Task ID: {}",
                task_response.task_id
            );
            accepted!(json!({
                "success": true,
                "taskId": task_response.task_id,
                "message": "Settings assistant dispatched",
                "provider": provider,
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to dispatch settings command: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Failed to create task: {}", e),
            })))
        }
        Err(e) => {
            error!("Settings command actor communication error: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Actor communication error: {}", e),
            })))
        }
    }
}

// Legacy spawn helper functions removed - all task creation now via TaskOrchestratorActor

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResponse {
    pub success: bool,
    pub message: String,
    pub task_id: Option<String>,
    pub error: Option<String>,
}

pub async fn remove_task(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> Result<impl Responder> {
    let task_id = path.into_inner();
    info!("Stopping task via Management API: {}", task_id);

    let stop_task_msg = StopTask {
        task_id: task_id.clone(),
    };

    match state.get_task_orchestrator_addr().send(stop_task_msg).await {
        Ok(Ok(())) => {
            info!("Successfully stopped task: {}", task_id);
            ok_json!(TaskResponse {
                success: true,
                message: format!("Task {} stopped successfully", task_id),
                task_id: Some(task_id),
                error: None,
            })
        }
        Ok(Err(e)) => {
            error!("Failed to stop task {}: {}", task_id, e);
            Ok(HttpResponse::InternalServerError().json(TaskResponse {
                success: false,
                message: format!("Failed to stop task: {}", e),
                task_id: Some(task_id),
                error: Some(e),
            }))
        }
        Err(e) => {
            error!("Actor communication error: {}", e);
            Ok(HttpResponse::InternalServerError().json(TaskResponse {
                success: false,
                message: format!("Actor communication error: {}", e),
                task_id: Some(task_id),
                error: Some(e.to_string()),
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// D2 steering surface (PRD-023 WP-3): per-agent submit-task / interrupt.
//
// `AgentDetailPanel.tsx` is the sole client call site of `/bots/submit-task`
// (line 269) and — once mounted behind a node selection — of the new
// `/bots/interrupt` route. Both reach the swarm through the SAME
// `TaskOrchestratorActor` transport the sibling swarm routes already use
// (`CreateTask` for submit; `InterruptAgentTask` for interrupt), and both
// observe `CANARY-VC-D2-STEER` as live traffic (DDD invariant 5 — the route was
// genuinely invoked, never a synthetic probe). The responses are deliberately
// BARE JSON (not the `StandardResponse` envelope) so the panel reads
// `response.taskId` / `statusRes.status` at the top level, matching its
// existing contract.
//
// D2 id-namespace fix: the panel sends `selectedAgent.id`, which is a
// claude-flow swarm agent_id — a DISJOINT namespace from the Management-API
// task_id `StopTask` needs. Passing it straight through 404'd every interrupt
// (the Management API has no such task), so the canary — which only observes on
// success — could never fire. `interrupt_task` now routes through
// `InterruptAgentTask`, which resolves the id (task_id OR swarm agent_id) to a
// concrete task_id server-side before the stop.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTaskRequest {
    /// Free-text task description entered in the panel.
    pub task: String,
    /// `low` | `medium` | `high` | `critical` (advisory; recorded in evidence).
    #[serde(default)]
    pub priority: Option<String>,
    /// Swarm the steer targets; defaults to `default`.
    #[serde(default)]
    pub swarm_id: Option<String>,
    /// Task-orchestration strategy hint (`adaptive` | `strategic` | `tactical`).
    #[serde(default)]
    pub strategy: Option<String>,
    /// Explicit agent type; when absent it is derived from `strategy`.
    #[serde(default)]
    pub agent_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptRequest {
    /// The id to interrupt — EITHER a Management-API `task_id` OR a claude-flow
    /// swarm `agent_id` (the panel sends `selectedAgent.id`, a swarm agent_id).
    /// The two are disjoint namespaces; `interrupt_task` resolves whichever it
    /// is to a concrete task_id server-side. The field renames to `agentId`
    /// (camelCase); `taskId`/`task_id` are accepted as aliases for the same value.
    #[serde(alias = "taskId", alias = "task_id")]
    pub agent_id: String,
    /// Swarm the interrupted agent belongs to (advisory; recorded in evidence).
    #[serde(default)]
    pub swarm_id: Option<String>,
}

/// `POST /api/bots/submit-task` — steer the selected agent's swarm with a new
/// task. Mirrors `spawn_agent_hybrid`'s `CreateTask` transport; fires
/// `CANARY-VC-D2-STEER` on a live invocation.
pub async fn submit_task(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    req: web::Json<SubmitTaskRequest>,
) -> Result<impl Responder> {
    let task = req.task.trim().to_string();
    if task.is_empty() {
        return Ok(HttpResponse::BadRequest().json(json!({
            "success": false,
            "error": "task is empty",
        })));
    }

    let strategy = req
        .strategy
        .clone()
        .unwrap_or_else(|| "adaptive".to_string());
    let agent_type = req.agent_type.clone().unwrap_or_else(|| {
        match strategy.as_str() {
            "strategic" => "planner",
            "tactical" => "coder",
            _ => "researcher",
        }
        .to_string()
    });
    let provider = std::env::var("PRIMARY_PROVIDER").unwrap_or_else(|_| "gemini".to_string());

    let create_task_msg = CreateTask {
        agent: agent_type.clone(),
        task: task.clone(),
        provider: provider.clone(),
        // Steer submit-task spawns by role/strategy; no distinct claude-flow agent
        // id is known at creation time. None (do not fabricate).
        claude_flow_agent_id: None,
    };

    match state
        .get_task_orchestrator_addr()
        .send(create_task_msg)
        .await
    {
        Ok(Ok(task_response)) => {
            info!(
                "Steer submit-task dispatched via Management API - Task ID: {}",
                task_response.task_id
            );

            // D2 canary: a steer action reached the server from a mounted panel.
            let evidence = format!(
                "submit-task agent={agent_type} priority={:?} swarm={:?} task_id={}",
                req.priority, req.swarm_id, task_response.task_id
            );
            if let Err(e) = state
                .liveness_harness
                .observe(CANARY_D2_STEER, &evidence)
                .await
            {
                log::debug!("[bots/submit-task] D2 canary observe skipped: {e}");
            }

            // BARE JSON: the panel reads `response.taskId` at the top level.
            Ok(HttpResponse::Accepted().json(json!({
                "success": true,
                "taskId": task_response.task_id,
                "message": "Task submitted to swarm",
                "agentType": agent_type,
                "provider": provider,
            })))
        }
        Ok(Err(e)) => {
            error!("Failed to submit steer task: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Failed to create task: {}", e),
            })))
        }
        Err(e) => {
            error!("Steer submit-task actor communication error: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Actor communication error: {}", e),
            })))
        }
    }
}

/// `POST /api/bots/interrupt` — stop the selected agent's task. The panel sends
/// `selectedAgent.id` (a claude-flow swarm agent_id), which is a DISJOINT
/// namespace from the Management-API task_id: `InterruptAgentTask` resolves it
/// to a concrete task_id server-side before the stop (a bare swarm agent_id sent
/// straight to `StopTask` 404'd every interrupt). Fires `CANARY-VC-D2-STEER` on
/// a live, resolved interrupt.
pub async fn interrupt_task(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    req: web::Json<InterruptRequest>,
) -> Result<impl Responder> {
    let requested_id = req.agent_id.trim().to_string();
    if requested_id.is_empty() {
        return Ok(HttpResponse::BadRequest().json(json!({
            "success": false,
            "error": "agent/task id is empty",
        })));
    }

    info!(
        "Interrupting agent/task via Management API: {}",
        requested_id
    );

    // Resolve the id (task_id OR swarm agent_id) to a concrete task_id, then stop.
    let interrupt_msg = InterruptAgentTask {
        id: requested_id.clone(),
    };

    match state.get_task_orchestrator_addr().send(interrupt_msg).await {
        Ok(Ok(resolved_task_id)) => {
            info!(
                "Successfully interrupted task: {} (from id {})",
                resolved_task_id, requested_id
            );

            // D2 canary: an interrupt steer action reached the server AND resolved
            // to a real task that was stopped — the success the canary observes.
            let evidence = format!(
                "interrupt id={requested_id} resolved_task_id={resolved_task_id} swarm={:?}",
                req.swarm_id
            );
            if let Err(e) = state
                .liveness_harness
                .observe(CANARY_D2_STEER, &evidence)
                .await
            {
                log::debug!("[bots/interrupt] D2 canary observe skipped: {e}");
            }

            Ok(HttpResponse::Ok().json(json!({
                "success": true,
                "taskId": resolved_task_id,
                "requestedId": requested_id,
                "message": format!("Agent task {} interrupted", resolved_task_id),
            })))
        }
        Ok(Err(InterruptError::Unresolved(msg))) => {
            // Honest capability boundary (D2 final close): the id resolved to NO
            // task in the registry and to no task's claude_flow_agent_id, so this is
            // an externally-spawned / MCP-native claude-flow agent. The MCP surface
            // (`utils/mcp_tcp_client.rs`) has NO terminate verb — it cannot be
            // stopped from here. Return 422 (the client does NOT retry 4xx, so the
            // panel no longer spins a dead retrying button) with a DISTINCT
            // `resolution: "unresolved"` + `interruptible: false` so the panel
            // discloses a disabled explanatory state.
            log::warn!(
                "Interrupt id {requested_id} unresolved — externally-spawned / MCP-native agent \
                 (no terminate verb on the MCP surface): {msg}"
            );
            Ok(HttpResponse::UnprocessableEntity().json(json!({
                "success": false,
                "interruptible": false,
                "resolution": "unresolved",
                "requestedId": requested_id,
                "error": msg,
                "message": "externally spawned — not interruptible from here",
            })))
        }
        Ok(Err(e)) => {
            // Operational fault (task list unavailable, or the stop call failed).
            // Retryable — a genuine 500.
            error!("Failed to interrupt id {}: {}", requested_id, e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "taskId": requested_id,
                "error": format!("Failed to interrupt task: {}", e),
            })))
        }
        Err(e) => {
            error!("Interrupt actor communication error: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "taskId": requested_id,
                "error": format!("Actor communication error: {}", e),
            })))
        }
    }
}

/// `GET /api/bots/task-status/{id}` — poll a steered task's real status through
/// the orchestrator (`GetTaskStatus`). Returns the bare `TaskStatus` so the
/// panel reads `statusRes.status` (`running` | `completed` | `failed`) directly;
/// an unknown id is a clean 404 that ends the panel's poll.
pub async fn get_task_status(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<impl Responder> {
    let task_id = path.into_inner();

    match state
        .get_task_orchestrator_addr()
        .send(GetTaskStatus {
            task_id: task_id.clone(),
        })
        .await
    {
        Ok(Ok(status)) => Ok(HttpResponse::Ok().json(status)),
        Ok(Err(e)) => {
            log::debug!("[bots/task-status] {task_id} not resolvable: {e}");
            Ok(HttpResponse::NotFound().json(json!({
                "success": false,
                "taskId": task_id,
                "status": "not_found",
                "error": e,
            })))
        }
        Err(e) => {
            error!("Task-status actor communication error: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Actor communication error: {}", e),
            })))
        }
    }
}

// pause_task and resume_task removed - Management API does not support pause/resume

// Helper function for socket handler to get bot positions
pub async fn get_bots_positions(bots_client: &Arc<BotsClient>) -> Vec<BotsNodeData> {
    match bots_client.get_agents_snapshot().await {
        Ok(agents) => agents
            .into_iter()
            .enumerate()
            .map(|(idx, agent)| BotsNodeData {
                id: (idx as u32) + 1000,
                data: BotData {
                    x: agent.x,
                    y: agent.y,
                    z: agent.z,
                    vx: 0.0,
                    vy: 0.0,
                    vz: 0.0,
                },
            })
            .collect(),
        Err(e) => {
            error!("Failed to get bots positions: {}", e);
            vec![]
        }
    }
}

#[cfg(test)]
mod steering_tests {
    use super::*;

    #[test]
    fn submit_task_request_parses_camel_case() {
        let req: SubmitTaskRequest = serde_json::from_str(
            r#"{"task":"do the thing","priority":"high","strategy":"adaptive","swarmId":"swarm-a"}"#,
        )
        .unwrap();
        assert_eq!(req.task, "do the thing");
        assert_eq!(req.priority.as_deref(), Some("high"));
        assert_eq!(req.swarm_id.as_deref(), Some("swarm-a"));
        assert_eq!(req.strategy.as_deref(), Some("adaptive"));
    }

    #[test]
    fn interrupt_request_accepts_task_and_agent_id_aliases() {
        // Primary camelCase rename.
        let by_agent: InterruptRequest =
            serde_json::from_str(r#"{"agentId":"task-1","swarmId":"swarm-a"}"#).unwrap();
        assert_eq!(by_agent.agent_id, "task-1");
        assert_eq!(by_agent.swarm_id.as_deref(), Some("swarm-a"));

        // taskId alias resolves to the same field.
        let by_task: InterruptRequest = serde_json::from_str(r#"{"taskId":"task-2"}"#).unwrap();
        assert_eq!(by_task.agent_id, "task-2");
        assert!(by_task.swarm_id.is_none());

        // snake_case alias too.
        let by_snake: InterruptRequest = serde_json::from_str(r#"{"task_id":"task-3"}"#).unwrap();
        assert_eq!(by_snake.agent_id, "task-3");
    }
}
