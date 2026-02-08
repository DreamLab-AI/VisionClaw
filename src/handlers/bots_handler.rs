use crate::actors::messages::GetBotsGraphData;
use crate::actors::{CreateTask, StopTask};
use crate::models::edge::Edge;
use crate::models::graph::GraphData;
use crate::models::metadata::MetadataStore;
use crate::models::node::Node;
use crate::services::bots_client::{Agent, BotsClient};
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::AppState;
use actix_web::{web, HttpResponse, Responder, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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
use once_cell::sync::Lazy;
use crate::{
    ok_json, created_json, error_json, bad_request, not_found,
    unauthorized, forbidden, conflict, no_content, accepted,
    too_many_requests, service_unavailable, payload_too_large
};

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
                },
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
    
    if let Ok(graph_data) = state.graph_service_addr.send(GetBotsGraphData).await {
        if let Ok(graph) = graph_data {
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
                }));
            }
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

    info!("🔧 Swarm initialization task: {}", task);

    
    
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
    };

    match state
        .get_task_orchestrator_addr()
        .send(create_task_msg)
        .await
    {
        Ok(Ok(task_response)) => {
            info!(
                "✓ Successfully created task via Management API - Task ID: {}",
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
                })
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
                })
            )
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

// pause_task and resume_task removed - Management API does not support pause/resume

// Helper function for socket handler to get bot positions
pub async fn get_bots_positions(bots_client: &Arc<BotsClient>) -> Vec<BotsNodeData> {
    match bots_client.get_agents_snapshot().await {
        Ok(agents) => {
            agents
                .into_iter()
                .enumerate()
                .map(|(idx, agent)| {
                    BotsNodeData {
                        id: (idx as u32) + 1000, 
                        data: BotData {
                            x: agent.x,
                            y: agent.y,
                            z: agent.z,
                            vx: 0.0, 
                            vy: 0.0,
                            vz: 0.0,
                        },
                    }
                })
                .collect()
        }
        Err(e) => {
            error!("Failed to get bots positions: {}", e);
            vec![]
        }
    }
}
