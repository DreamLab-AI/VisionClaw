use actix::{Actor, ActorContext, AsyncContext, Handler, Message, StreamHandler};
use actix_web::{web, HttpResponse, Responder};
use actix_web_actors::ws;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};

use crate::actors::messages::UpdateBotsGraph;
use crate::ok_json;
use crate::services::agent_visualization_protocol::{
    AgentStateUpdate, AgentVisualizationProtocol, PositionUpdate,
};
use crate::services::bots_client::Agent;
use crate::AppState;

pub struct AgentVisualizationWs {
    _app_state: web::Data<AppState>,
    protocol: AgentVisualizationProtocol,
    last_heartbeat: Instant,
    _last_position_update: Instant,
    /// ADR-2066 addendum: honours the `pause_updates`/`resume_updates` client
    /// opcodes. Previously both opcodes only logged, so a client that asked for
    /// a pause kept receiving a 16 ms position stream.
    paused: bool,
}

impl AgentVisualizationWs {
    pub fn new(app_state: web::Data<AppState>) -> Self {
        Self {
            _app_state: app_state,
            protocol: AgentVisualizationProtocol::new(),
            last_heartbeat: Instant::now(),
            _last_position_update: Instant::now(),
            paused: false,
        }
    }

    #[allow(dead_code)]
    /// Sends the initialisation frame.
    ///
    /// ADR-2066 addendum: this still reports an EMPTY roster, and that is now
    /// explicit rather than disguised. The dead `get_real_agent_data()` helper
    /// that pretended to be a source (it returned `vec![]` unconditionally) has
    /// been removed. A real roster is reachable — `AppState.bots_client`
    /// (`app_state.rs:382`) exposes `get_agents_snapshot() -> Result<Vec<Agent>>`
    /// (`bots_client.rs:231`) — but wiring it needs two things this change does
    /// not invent: an async fetch spawned into the actor context, and an
    /// `Agent -> AgentStatus` mapping that does not exist. `AgentStatus`
    /// (`crates/visionclaw-domain/src/types/claude_flow.rs:19`) requires
    /// `profile: AgentProfile`, `active/completed/failed_tasks_count`,
    /// `success_rate` and a `timestamp` that `Agent` (`bots_client.rs:16`) does
    /// not carry, so a faithful conversion needs a decided contract rather than
    /// invented defaults. Tracked as PROPOSED work, not silently faked here.
    fn send_init_state(&self, ctx: &mut ws::WebsocketContext<Self>) {
        let agents: Vec<visionclaw_domain::types::claude_flow::AgentStatus> = Vec::new();

        let init_json =
            AgentVisualizationProtocol::create_init_message("swarm-001", "hierarchical", agents);

        let agent_count = init_json.matches("agentId").count();
        ctx.text(init_json);
        info!(
            "Sent initialization message with {} agents to client",
            agent_count
        );
    }

    fn start_position_updates(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(Duration::from_millis(16), |act, ctx| {
            if act.paused {
                return;
            }
            if let Some(update_json) = act.protocol.create_position_update() {
                ctx.text(update_json);
            }
        });
    }

    fn start_heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(Duration::from_secs(5), |act, ctx| {
            if Instant::now().duration_since(act.last_heartbeat) > Duration::from_secs(10) {
                warn!("WebSocket client heartbeat timeout, disconnecting");
                ctx.stop();
                return;
            }

            ctx.ping(b"ping");
        });
    }
}

impl Actor for AgentVisualizationWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Agent visualization WebSocket connection established");

        ctx.address().do_send(InitConnection);

        self.start_heartbeat(ctx);

        self.start_position_updates(ctx);
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("Agent visualization WebSocket connection closed");
    }
}

struct InitConnection;

impl Message for InitConnection {
    type Result = ();
}

struct UpdatePositions(Vec<PositionUpdate>);

impl Message for UpdatePositions {
    type Result = ();
}

struct UpdateStates(Vec<AgentStateUpdate>);

impl Message for UpdateStates {
    type Result = ();
}

impl Handler<InitConnection> for AgentVisualizationWs {
    type Result = ();

    fn handle(&mut self, _: InitConnection, ctx: &mut Self::Context) {
        self.send_init_state(ctx);
    }
}

impl Handler<UpdatePositions> for AgentVisualizationWs {
    type Result = ();

    fn handle(&mut self, msg: UpdatePositions, _ctx: &mut Self::Context) {
        for update in msg.0 {
            self.protocol.add_position_update(
                update.id,
                update.x,
                update.y,
                update.z,
                update.vx.unwrap_or(0.0),
                update.vy.unwrap_or(0.0),
                update.vz.unwrap_or(0.0),
            );
        }
    }
}

impl Handler<UpdateStates> for AgentVisualizationWs {
    type Result = ();

    fn handle(&mut self, msg: UpdateStates, ctx: &mut Self::Context) {
        let state_json = AgentVisualizationProtocol::create_state_update(msg.0);
        ctx.text(state_json);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for AgentVisualizationWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.last_heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.last_heartbeat = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                if let Ok(request) = serde_json::from_str::<ClientRequest>(&text) {
                    match request.action.as_str() {
                        "refresh" => {
                            self.send_init_state(ctx);
                        }
                        "pause_updates" => {
                            self.paused = true;
                            debug!("Paused position updates");
                        }
                        "resume_updates" => {
                            self.paused = false;
                            debug!("Resumed position updates");
                        }
                        _ => {
                            warn!("Unknown client action: {}", request.action);
                        }
                    }
                }
            }
            Ok(ws::Message::Binary(_)) => {
                warn!("Binary messages not supported");
            }
            Ok(ws::Message::Close(reason)) => {
                info!("WebSocket closing: {:?}", reason);
                ctx.close(reason);
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}

#[derive(Deserialize)]
struct ClientRequest {
    action: String,
    #[allow(dead_code)]
    params: Option<serde_json::Value>,
}

pub async fn agent_visualization_ws(
    req: actix_web::HttpRequest,
    stream: web::Payload,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    ws::start(AgentVisualizationWs::new(app_state), &req, stream)
}

pub async fn get_agent_visualization_snapshot(app_state: web::Data<AppState>) -> impl Responder {
    let agents = get_real_agents_from_app_state(&app_state).await;

    let agent_statuses: Vec<visionclaw_domain::types::claude_flow::AgentStatus> = agents
        .into_iter()
        .map(
            |update| visionclaw_domain::types::claude_flow::AgentStatus {
                agent_id: update.id.clone(),
                profile: visionclaw_domain::types::claude_flow::AgentProfile {
                    name: update.id.clone(),
                    agent_type: visionclaw_domain::types::claude_flow::AgentType::Generic,
                    capabilities: vec!["general".to_string()],
                    description: Some("Agent".to_string()),
                    version: "1.0".to_string(),
                    tags: vec![],
                },
                status: update.status.unwrap_or_else(|| "active".to_string()),
                active_tasks_count: update.tasks_active.unwrap_or(0),
                completed_tasks_count: 0,
                failed_tasks_count: 0,
                success_rate: 1.0,
                timestamp: chrono::Utc::now(),
                current_task: update.current_task.as_ref().map(|task| {
                    visionclaw_domain::types::claude_flow::TaskReference {
                        task_id: "current".to_string(),
                        description: task.clone(),
                        priority: visionclaw_domain::types::claude_flow::TaskPriority::Medium,
                    }
                }),

                agent_type: "generic".to_string(),
                current_task_description: update.current_task.clone(),
                capabilities: vec!["general".to_string()],
                position: None,
                cpu_usage: update.cpu.unwrap_or(0.0),
                memory_usage: update.memory.unwrap_or(0.0),
                health: update.health.unwrap_or(1.0),
                activity: update.activity.unwrap_or(0.0),
                tasks_active: update.tasks_active.unwrap_or(0),
                tasks_completed: 0,
                success_rate_normalized: 1.0,
                tokens: 0,
                token_rate: 0.0,
                created_at: chrono::Utc::now().to_rfc3339(),
                age: 0,
                workload: Some(0.5),

                performance_metrics: visionclaw_domain::types::claude_flow::PerformanceMetrics {
                    tasks_completed: 0,
                    success_rate: 1.0,
                },
                token_usage: visionclaw_domain::types::claude_flow::TokenUsage {
                    total: 0,
                    token_rate: 0.0,
                },
                swarm_id: None,
                agent_mode: Some("agent".to_string()),
                parent_queen_id: None,
                processing_logs: None,
            },
        )
        .collect();

    let init_json = AgentVisualizationProtocol::create_init_message(
        "swarm-001",
        "hierarchical",
        agent_statuses,
    );

    HttpResponse::Ok()
        .content_type("application/json")
        .body(init_json)
}

#[derive(Deserialize)]
pub struct InitializeSwarmRequest {
    pub topology: String,
    pub max_agents: u32,
    pub agent_types: Vec<String>,
    pub custom_prompt: Option<String>,
}

pub async fn initialize_swarm_visualization(
    req: web::Json<InitializeSwarmRequest>,
    _app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    info!(
        "Initializing swarm visualization with topology: {}",
        req.topology
    );

    ok_json!(json!({
        "success": true,
        "message": "Swarm initialization started",
        "swarm_id": "swarm-001",
        "topology": req.topology,
        "max_agents": req.max_agents
    }))
}

// ---------------------------------------------------------------------------
// Mock Agent Injection
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MockAgentDef {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    #[serde(default = "default_active")]
    pub status: String,
}

fn default_active() -> String {
    "active".to_string()
}

#[derive(Debug, Deserialize)]
pub struct MockAgentsRequest {
    pub agents: Vec<MockAgentDef>,
}

#[derive(Debug, Serialize)]
struct MockAgentResult {
    id: String,
    node_id: u32,
    edges_to: Vec<u32>,
}

/// POST /api/bots/mock-agents
///
/// Inject mock agents into the live 3D graph. Each agent is placed on a golden
/// angle spiral (radius ~30 units) and connected to 3-5 random knowledge nodes
/// with relationship edges ("reads", "modifies", "monitors").
pub async fn inject_mock_agents(
    req: web::Json<MockAgentsRequest>,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let bot_id_offset: u32 = 10_000;
    let agent_count = req.agents.len() as f32;

    let edge_labels = ["reads", "modifies", "monitors"];

    // Gather knowledge node IDs from the existing graph so we can create
    // cross-edges. We read from graph_service_addr via a GetGraphData message.
    let knowledge_ids: Vec<u32> = {
        use crate::actors::messages::GetGraphData;
        match app_state.graph_service_addr.send(GetGraphData).await {
            Ok(Ok(graph_data)) => graph_data
                .nodes
                .iter()
                .filter(|n| {
                    matches!(
                        n.node_type.as_deref(),
                        Some("page") | Some("linked_page") | None
                    )
                })
                .map(|n| n.id)
                .collect(),
            _ => vec![],
        }
    };

    let mut agents: Vec<Agent> = Vec::with_capacity(req.agents.len());
    let mut results: Vec<MockAgentResult> = Vec::with_capacity(req.agents.len());

    // Deterministic but varied seed based on agent id
    let mut pseudo_rng_state: u64 = 0xDEAD_BEEF;
    let next_rand = |state: &mut u64| -> u32 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*state >> 33) as u32
    };

    for (i, def) in req.agents.iter().enumerate() {
        let node_id = bot_id_offset + i as u32;

        // Golden angle spiral position at radius ~30
        let golden_angle = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
        let theta = golden_angle * i as f64;
        let y_norm = if agent_count <= 1.0 {
            0.0
        } else {
            1.0 - (i as f64 / (agent_count as f64 - 1.0)) * 2.0
        };
        let radius_at_y = (1.0 - y_norm * y_norm).sqrt();
        let scale = 30.0;

        let x = (radius_at_y * theta.cos() * scale) as f32;
        let y = (y_norm * scale) as f32;
        let z = (radius_at_y * theta.sin() * scale) as f32;

        let workload = match def.status.as_str() {
            "active" => 0.7,
            "thinking" => 0.9,
            "idle" => 0.2,
            _ => 0.5,
        };

        agents.push(Agent {
            id: def.id.clone(),
            name: def.label.clone(),
            agent_type: def.agent_type.clone(),
            status: def.status.clone(),
            x,
            y,
            z,
            cpu_usage: 15.0 + (i as f32 * 7.3) % 40.0,
            memory_usage: 128.0 + (i as f32 * 23.7) % 256.0,
            health: 95.0,
            workload,
            created_at: Some(chrono::Utc::now().to_rfc3339()),
            age: Some(i as u64 * 12_000),
            did_nostr: None,
        });

        // Pick 3-5 random knowledge node targets for cross-edges
        let mut edge_targets: Vec<u32> = Vec::new();
        if !knowledge_ids.is_empty() {
            let edge_count = 3 + (next_rand(&mut pseudo_rng_state) % 3) as usize; // 3..5
            for _ in 0..edge_count {
                let idx = next_rand(&mut pseudo_rng_state) as usize % knowledge_ids.len();
                let target = knowledge_ids[idx];
                if !edge_targets.contains(&target) {
                    edge_targets.push(target);
                }
            }
        }

        results.push(MockAgentResult {
            id: def.id.clone(),
            node_id,
            edges_to: edge_targets,
        });
    }

    // Send UpdateBotsGraph with agent-to-knowledge edges baked in.
    // The UpdateBotsGraph handler in GraphStateActor already creates inter-agent
    // edges; we extend it by also sending a separate message for knowledge edges.
    // However, UpdateBotsGraph only takes agents — knowledge edges are injected
    // via a custom approach: we store them in agent metadata so the handler
    // can read them, OR we post-process. For simplicity, we send the agents
    // now and then inject knowledge edges directly.
    app_state
        .graph_service_addr
        .do_send(UpdateBotsGraph { agents });

    // Inject agent→knowledge edges via AddEdgeBatch if available,
    // or via individual AddEdge messages. We use the graph_state_actor
    // approach: send an InjectMockEdges message. Since that message doesn't
    // exist yet, we create the edges in a follow-up UpdateBotsGraph extension.
    // For now, the cross-edges are reported in the response and the graph_state_actor
    // handler already creates inter-agent edges. The knowledge edges are injected
    // via a dedicated AddEdge call through GraphServiceSupervisor.
    {
        use crate::actors::messages::AddEdge;
        for result in &results {
            for (j, &target_id) in result.edges_to.iter().enumerate() {
                let label_idx = j % edge_labels.len();
                let mut edge =
                    visionclaw_domain::models::edge::Edge::new(result.node_id, target_id, 0.3);
                edge.edge_type = Some(edge_labels[label_idx].to_string());
                let metadata = edge
                    .metadata
                    .get_or_insert_with(std::collections::HashMap::new);
                metadata.insert("mock_agent_edge".to_string(), "true".to_string());
                metadata.insert(
                    "interaction".to_string(),
                    edge_labels[label_idx].to_string(),
                );
                app_state.graph_service_addr.do_send(AddEdge { edge });
            }
        }
    }

    info!(
        "Injected {} mock agents with knowledge edges into graph",
        results.len()
    );

    ok_json!(json!({
        "success": true,
        "injected": results.len(),
        "agents": results.iter().map(|r| json!({
            "id": r.id,
            "node_id": r.node_id,
            "edges_to_knowledge": r.edges_to,
        })).collect::<Vec<_>>(),
    }))
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/visualization")
            .route("/agents/ws", web::get().to(agent_visualization_ws))
            .route(
                "/agents/snapshot",
                web::get().to(get_agent_visualization_snapshot),
            )
            .route(
                "/swarm/initialize",
                web::post().to(initialize_swarm_visualization),
            ),
    );
    // Mock agent injection — outside /visualization scope, under /bots (no /api prefix;
    // this configure_routes is called inside the /api scope in main.rs)
    cfg.route("/bots/mock-agents", web::post().to(inject_mock_agents));
}

async fn get_real_agents_from_app_state(
    app_state: &AppState,
) -> Vec<crate::services::agent_visualization_protocol::AgentStateUpdate> {
    if let Ok(agents) = app_state.bots_client.get_agents_snapshot().await {
        return agents
            .into_iter()
            .map(
                |agent| crate::services::agent_visualization_protocol::AgentStateUpdate {
                    id: agent.id,
                    status: Some(agent.status),
                    health: Some(agent.health),
                    cpu: Some(agent.cpu_usage),
                    memory: Some(agent.memory_usage),
                    activity: Some(agent.workload),
                    tasks_active: Some(1),
                    current_task: Some("Agent running".to_string()),
                },
            )
            .collect();
    }

    vec![
        crate::services::agent_visualization_protocol::AgentStateUpdate {
            id: "system-coordinator".to_string(),
            status: Some("active".to_string()),
            health: Some(100.0),
            cpu: Some(15.0),
            memory: Some(128.0),
            activity: Some(0.1),
            tasks_active: Some(1),
            current_task: Some("System coordination and monitoring".to_string()),
        },
    ]
}
