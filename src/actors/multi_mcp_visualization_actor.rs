//! Multi-MCP Visualization Actor
//!
//! This actor manages visualization of multiple MCP (Model Context Protocol) server
//! connections and agent swarms. It handles:
//! - Real-time visualization of agent positions across multiple MCP servers
//! - Topology changes and swarm reorganization events
//! - Cross-server agent communication patterns
//! - Performance metrics and connection health monitoring
//! - Dynamic layout updates based on swarm topology

use actix::prelude::*;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::services::agent_visualization_protocol::{
    AgentInit, AgentMetrics, AgentStateUpdate, AgentVisualizationMessage, Bottleneck,
    ConnectionInit, ConnectionUpdateMessage, CriticalPath, GlobalPerformanceMetrics,
    InitializeMessage, LayerLoad, MetricsUpdateMessage, PhysicsConfig, Position, PositionUpdate,
    PositionUpdateMessage, StateUpdateMessage, SwarmMetrics, SwarmTopologyData, VisualConfig,
};
use crate::services::multi_mcp_agent_discovery::McpServerConfig;
use crate::types::AgentStatus;
use crate::types::Vec3Data;
use crate::utils::time;

#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct AgentVisualizationMessageWrapper(pub AgentVisualizationMessage);

#[derive(Debug)]
pub struct MultiMcpVisualizationActor {
    
    pub mcp_servers: HashMap<String, McpServerConfig>,

    
    pub agent_positions: HashMap<String, Position>,

    
    pub agents: HashMap<String, AgentInit>,

    
    pub connections: HashMap<String, ConnectionInit>,

    
    pub server_metrics: HashMap<String, McpServerMetrics>,

    
    pub layout_algorithm: LayoutAlgorithm,

    
    pub physics_config: PhysicsConfig,

    
    pub visual_config: VisualConfig,

    
    pub last_update: Instant,
    pub update_interval: Duration,

    
    pub subscribers: Vec<Recipient<AgentVisualizationMessageWrapper>>,

    
    pub topology_data: SwarmTopologyData,

    
    pub global_metrics: GlobalPerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerMetrics {
    pub server_id: String,
    pub agents_count: u32,
    pub connections_count: u32,
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub network_latency: f32,
    pub messages_per_second: f32,
    pub error_rate: f32,
    pub last_updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayoutAlgorithm {
    
    ForceDirected {
        attraction_strength: f32,
        repulsion_strength: f32,
        damping_factor: f32,
    },
    
    Hierarchical {
        server_separation: f32,
        layer_height: f32,
        node_spacing: f32,
    },
    
    Circular {
        radius_base: f32,
        radius_increment: f32,
        angular_spacing: f32,
    },
    
    Grid {
        grid_spacing: f32,
        cluster_size: u32,
        padding: f32,
    },
}

impl Default for LayoutAlgorithm {
    fn default() -> Self {
        LayoutAlgorithm::ForceDirected {
            attraction_strength: 0.5,
            repulsion_strength: 1.0,
            damping_factor: 0.95,
        }
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub enum MultiMcpVisualizationMessage {
    
    Initialize {
        servers: Vec<McpServerConfig>,
        layout: LayoutAlgorithm,
        physics: PhysicsConfig,
        visual: VisualConfig,
    },

    
    UpdateAgentPositions {
        server_id: String,
        positions: HashMap<String, Position>,
        timestamp: i64,
    },

    
    AddAgent {
        server_id: String,
        agent: AgentInit,
        position: Option<Position>,
    },

    
    RemoveAgent { server_id: String, agent_id: String },

    
    UpdateAgentStatus {
        server_id: String,
        agent_id: String,
        status: AgentStatus,
        metadata: HashMap<String, serde_json::Value>,
    },

    
    AddConnection { connection: ConnectionInit },

    
    RemoveConnection { connection_id: String },

    
    UpdateServerMetrics {
        server_id: String,
        metrics: McpServerMetrics,
    },

    
    Subscribe {
        recipient: Recipient<AgentVisualizationMessageWrapper>,
    },

    
    Unsubscribe {
        recipient: Recipient<AgentVisualizationMessageWrapper>,
    },

    
    ChangeLayout { algorithm: LayoutAlgorithm },

    
    AnalyzeTopology,

    
    GetVisualizationState,

    
    Reset,
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum MultiMcpVisualizationResponse {
    
    VisualizationState {
        agents: HashMap<String, AgentInit>,
        positions: HashMap<String, Position>,
        connections: HashMap<String, ConnectionInit>,
        servers: HashMap<String, McpServerConfig>,
        metrics: HashMap<String, McpServerMetrics>,
        topology: SwarmTopologyData,
        global_metrics: GlobalPerformanceMetrics,
    },

    
    TopologyAnalysis {
        topology_data: SwarmTopologyData,
        recommendations: Vec<TopologyRecommendation>,
    },

    
    PerformanceMetrics {
        global_metrics: GlobalPerformanceMetrics,
        server_metrics: HashMap<String, McpServerMetrics>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyRecommendation {
    pub recommendation_type: RecommendationType,
    pub description: String,
    pub impact: f32,
    pub server_id: Option<String>,
    pub agent_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecommendationType {
    LoadBalance,
    OptimizeConnections,
    RelocateAgent,
    ScaleServer,
    MergeServers,
    SplitServer,
}

impl Default for MultiMcpVisualizationActor {
    fn default() -> Self {
        Self {
            mcp_servers: HashMap::new(),
            agent_positions: HashMap::new(),
            agents: HashMap::new(),
            connections: HashMap::new(),
            server_metrics: HashMap::new(),
            layout_algorithm: LayoutAlgorithm::default(),
            physics_config: PhysicsConfig::default(),
            visual_config: VisualConfig::default(),
            last_update: Instant::now(),
            update_interval: Duration::from_millis(33), 
            subscribers: Vec::new(),
            topology_data: SwarmTopologyData::default(),
            global_metrics: GlobalPerformanceMetrics::default(),
        }
    }
}

impl Actor for MultiMcpVisualizationActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("MultiMcpVisualizationActor started");

        
        ctx.run_interval(self.update_interval, |act, ctx| {
            act.update_visualization(ctx);
        });

        
        ctx.run_interval(Duration::from_secs(10), |act, _ctx| {
            act.analyze_topology();
        });


        ctx.run_interval(Duration::from_secs(5), |act, _ctx| {
            act.collect_global_metrics();
        });
    }

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        info!("MultiMcpVisualizationActor stopping");
        Running::Stop
    }
}

impl Handler<MultiMcpVisualizationMessage> for MultiMcpVisualizationActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        msg: MultiMcpVisualizationMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        match msg {
            MultiMcpVisualizationMessage::Initialize {
                servers,
                layout,
                physics,
                visual,
            } => self.initialize_visualization(servers, layout, physics, visual),

            MultiMcpVisualizationMessage::UpdateAgentPositions {
                server_id,
                positions,
                timestamp,
            } => self.update_agent_positions(server_id, positions, timestamp),

            MultiMcpVisualizationMessage::AddAgent {
                server_id,
                agent,
                position,
            } => self.add_agent(server_id, agent, position),

            MultiMcpVisualizationMessage::RemoveAgent {
                server_id,
                agent_id,
            } => self.remove_agent(server_id, agent_id),

            MultiMcpVisualizationMessage::UpdateAgentStatus {
                server_id,
                agent_id,
                status,
                metadata,
            } => self.update_agent_status(server_id, agent_id, status, metadata),

            MultiMcpVisualizationMessage::AddConnection { connection } => {
                self.add_connection(connection)
            }

            MultiMcpVisualizationMessage::RemoveConnection { connection_id } => {
                self.remove_connection(connection_id)
            }

            MultiMcpVisualizationMessage::UpdateServerMetrics { server_id, metrics } => {
                self.update_server_metrics(server_id, metrics)
            }

            MultiMcpVisualizationMessage::Subscribe { recipient } => self.subscribe(recipient),

            MultiMcpVisualizationMessage::Unsubscribe { recipient: _recipient } => {
                
                warn!("Unsubscribe not fully implemented - requires subscriber identification");
                Ok(())
            }

            MultiMcpVisualizationMessage::ChangeLayout { algorithm } => {
                self.change_layout(algorithm)
            }

            MultiMcpVisualizationMessage::AnalyzeTopology => {
                self.analyze_topology();
                Ok(())
            }

            MultiMcpVisualizationMessage::GetVisualizationState => {
                let _response = MultiMcpVisualizationResponse::VisualizationState {
                    agents: self.agents.clone(),
                    positions: self.agent_positions.clone(),
                    connections: self.connections.clone(),
                    servers: self.mcp_servers.clone(),
                    metrics: self.server_metrics.clone(),
                    topology: self.topology_data.clone(),
                    global_metrics: self.global_metrics.clone(),
                };
                
                debug!("Visualization state requested");
                Ok(())
            }

            MultiMcpVisualizationMessage::Reset => self.reset_visualization(),
        }
    }
}

impl MultiMcpVisualizationActor {
    
    pub fn new() -> Self {
        Self::default()
    }

    
    fn initialize_visualization(
        &mut self,
        servers: Vec<McpServerConfig>,
        layout: LayoutAlgorithm,
        physics: PhysicsConfig,
        visual: VisualConfig,
    ) -> Result<(), String> {
        info!(
            "Initializing Multi-MCP visualization with {} servers",
            servers.len()
        );

        
        for server in servers {
            self.mcp_servers.insert(server.server_id.clone(), server);
        }

        
        self.layout_algorithm = layout;
        self.physics_config = physics;
        self.visual_config = visual;

        
        self.initialize_server_layout()?;

        
        self.broadcast_initialization();

        Ok(())
    }

    
    fn initialize_server_layout(&mut self) -> Result<(), String> {
        let server_count = self.mcp_servers.len() as f32;

        match &self.layout_algorithm {
            LayoutAlgorithm::Hierarchical {
                server_separation, ..
            } => {
                #[allow(unused_assignments)]
                let mut x_offset = 0.0;
                for (i, server_id) in self.mcp_servers.keys().enumerate() {

                    x_offset = i as f32 * server_separation;
                    debug!("Positioning server {} at x_offset: {}", server_id, x_offset);
                }
            }
            LayoutAlgorithm::Circular {
                radius_base,
                radius_increment,
                ..
            } => {
                for (i, server_id) in self.mcp_servers.keys().enumerate() {
                    let angle = 2.0 * std::f32::consts::PI * i as f32 / server_count;
                    let radius = radius_base + i as f32 * radius_increment;
                    debug!(
                        "Positioning server {} at angle: {}, radius: {}",
                        server_id, angle, radius
                    );
                }
            }
            _ => {
                debug!("Using default server positioning");
            }
        }

        Ok(())
    }

    
    fn update_agent_positions(
        &mut self,
        server_id: String,
        positions: HashMap<String, Position>,
        timestamp: i64,
    ) -> Result<(), String> {
        for (agent_id, position) in positions {
            let full_agent_id = format!("{}:{}", server_id, agent_id);
            self.agent_positions.insert(full_agent_id, position);
        }

        
        self.broadcast_position_update(timestamp);

        Ok(())
    }

    
    fn add_agent(
        &mut self,
        server_id: String,
        mut agent: AgentInit,
        position: Option<Position>,
    ) -> Result<(), String> {
        let full_agent_id = format!("{}:{}", server_id, agent.id);

        
        agent.id = full_agent_id.clone();
        

        
        let pos = position.unwrap_or_else(|| self.generate_agent_position(&agent.id));

        self.agents.insert(full_agent_id.clone(), agent);
        self.agent_positions.insert(full_agent_id, pos);

        
        self.broadcast_state_update();

        Ok(())
    }

    
    fn remove_agent(&mut self, server_id: String, agent_id: String) -> Result<(), String> {
        let full_agent_id = format!("{}:{}", server_id, agent_id);

        self.agents.remove(&full_agent_id);
        self.agent_positions.remove(&full_agent_id);

        
        self.connections
            .retain(|_, conn| conn.source != full_agent_id && conn.target != full_agent_id);

        
        self.broadcast_state_update();

        Ok(())
    }

    
    fn update_agent_status(
        &mut self,
        server_id: String,
        agent_id: String,
        status: AgentStatus,
        _metadata: HashMap<String, serde_json::Value>,
    ) -> Result<(), String> {
        let full_agent_id = format!("{}:{}", server_id, agent_id);

        if let Some(agent) = self.agents.get_mut(&full_agent_id) {
            
            agent.status = format!("{:?}", status);
            
            

            
            self.broadcast_state_update();
        }

        Ok(())
    }

    
    fn add_connection(&mut self, connection: ConnectionInit) -> Result<(), String> {
        self.connections.insert(connection.id.clone(), connection);

        
        self.broadcast_connection_update();

        Ok(())
    }

    
    fn remove_connection(&mut self, connection_id: String) -> Result<(), String> {
        self.connections.remove(&connection_id);

        
        self.broadcast_connection_update();

        Ok(())
    }

    
    fn update_server_metrics(
        &mut self,
        server_id: String,
        metrics: McpServerMetrics,
    ) -> Result<(), String> {
        self.server_metrics.insert(server_id, metrics);

        
        self.collect_global_metrics();

        
        self.broadcast_metrics_update();

        Ok(())
    }

    
    fn subscribe(
        &mut self,
        recipient: Recipient<AgentVisualizationMessageWrapper>,
    ) -> Result<(), String> {
        self.subscribers.push(recipient);
        Ok(())
    }

    
    fn change_layout(&mut self, algorithm: LayoutAlgorithm) -> Result<(), String> {
        self.layout_algorithm = algorithm;

        
        self.recalculate_layout()?;

        
        self.broadcast_position_update(time::timestamp_seconds());

        Ok(())
    }

    
    fn reset_visualization(&mut self) -> Result<(), String> {
        self.agents.clear();
        self.agent_positions.clear();
        self.connections.clear();
        self.server_metrics.clear();
        self.topology_data = SwarmTopologyData::default();
        self.global_metrics = GlobalPerformanceMetrics::default();

        
        self.broadcast_state_update();

        Ok(())
    }

    
    fn generate_agent_position(&self, agent_id: &str) -> Position {
        
        let server_id = agent_id.split(':').next().unwrap_or("unknown");

        match &self.layout_algorithm {
            LayoutAlgorithm::ForceDirected { .. } => {
                
                let server_offset = self.get_server_offset(server_id);
                Position {
                    x: server_offset.x + (rand::random::<f32>() - 0.5) * 10.0,
                    y: server_offset.y + (rand::random::<f32>() - 0.5) * 10.0,
                    z: server_offset.z + (rand::random::<f32>() - 0.5) * 10.0,
                }
            }
            LayoutAlgorithm::Hierarchical {
                layer_height,
                node_spacing,
                ..
            } => {
                let server_index = self.get_server_index(server_id);
                let agent_count_in_server = self.get_agent_count_in_server(server_id);

                Position {
                    x: server_index as f32 * 20.0,
                    y: (agent_count_in_server as f32 * node_spacing) % layer_height,
                    z: 0.0,
                }
            }
            LayoutAlgorithm::Circular {
                radius_base,
                angular_spacing,
                ..
            } => {
                let server_index = self.get_server_index(server_id);
                let angle = server_index as f32 * angular_spacing;

                Position {
                    x: radius_base * angle.cos(),
                    y: radius_base * angle.sin(),
                    z: 0.0,
                }
            }
            LayoutAlgorithm::Grid {
                grid_spacing,
                cluster_size,
                ..
            } => {
                let server_index = self.get_server_index(server_id);
                let agent_count = self.get_agent_count_in_server(server_id);

                let grid_x = (server_index % *cluster_size as usize) as f32 * grid_spacing;
                let grid_y = (server_index / *cluster_size as usize) as f32 * grid_spacing;
                let local_offset = (agent_count % 4) as f32 * 2.0;

                Position {
                    x: grid_x + local_offset,
                    y: grid_y + local_offset,
                    z: 0.0,
                }
            }
        }
    }

    
    fn get_server_offset(&self, server_id: &str) -> Position {
        let server_index = self.get_server_index(server_id);
        Position {
            x: server_index as f32 * 25.0,
            y: 0.0,
            z: 0.0,
        }
    }

    
    fn get_server_index(&self, server_id: &str) -> usize {
        self.mcp_servers
            .keys()
            .position(|id| id == server_id)
            .unwrap_or(0)
    }

    
    fn get_agent_count_in_server(&self, server_id: &str) -> usize {
        self.agents
            .values()
            .filter(|agent| {
                
                agent.id.split(':').next().unwrap_or("") == server_id
            })
            .count()
    }

    
    fn recalculate_layout(&mut self) -> Result<(), String> {
        let agent_ids: Vec<String> = self.agents.keys().cloned().collect();

        for agent_id in agent_ids {
            let new_position = self.generate_agent_position(&agent_id);
            self.agent_positions.insert(agent_id, new_position);
        }

        Ok(())
    }

    
    fn analyze_topology(&mut self) {
        let mut recommendations = Vec::new();

        
        let mut total_agents = 0;
        let mut max_agents = 0;
        let mut min_agents = usize::MAX;

        for server_id in self.mcp_servers.keys() {
            let agent_count = self.get_agent_count_in_server(server_id);
            total_agents += agent_count;
            max_agents = max_agents.max(agent_count);
            min_agents = min_agents.min(agent_count);
        }

        
        if max_agents > 0 && min_agents < usize::MAX {
            let imbalance_ratio = max_agents as f32 / min_agents.max(1) as f32;
            if imbalance_ratio > 2.0 {
                recommendations.push(TopologyRecommendation {
                    recommendation_type: RecommendationType::LoadBalance,
                    description: format!(
                        "Load imbalance detected: max {} agents, min {} agents",
                        max_agents, min_agents
                    ),
                    impact: imbalance_ratio,
                    server_id: None,
                    agent_ids: Vec::new(),
                });
            }
        }

        
        let mut cross_server_connections = 0;
        let total_connections = self.connections.len();

        for connection in self.connections.values() {
            let source_server = connection.source.split(':').next().unwrap_or("");
            let target_server = connection.target.split(':').next().unwrap_or("");

            if source_server != target_server {
                cross_server_connections += 1;
            }
        }

        if total_connections > 0 {
            let cross_server_ratio = cross_server_connections as f32 / total_connections as f32;
            if cross_server_ratio > 0.3 {
                recommendations.push(TopologyRecommendation {
                    recommendation_type: RecommendationType::OptimizeConnections,
                    description: format!(
                        "High cross-server communication: {:.1}%",
                        cross_server_ratio * 100.0
                    ),
                    impact: cross_server_ratio,
                    server_id: None,
                    agent_ids: Vec::new(),
                });
            }
        }

        
        self.topology_data = SwarmTopologyData {
            topology_type: "multi_server".to_string(),
            total_agents: total_agents as u32,
            coordination_layers: self.mcp_servers.len() as u32, 
            efficiency_score: if total_agents > 0 && cross_server_connections > 0 {
                
                1.0 - (cross_server_connections as f32 / total_connections.max(1) as f32)
            } else {
                1.0
            },
            load_distribution: self.calculate_load_distribution(),
            critical_paths: self.analyze_critical_paths(),
            bottlenecks: self.detect_bottlenecks(),
        };

        debug!(
            "Topology analysis complete: {} agents, {} connections, {} servers",
            total_agents,
            total_connections,
            self.mcp_servers.len()
        );
    }

    
    fn collect_global_metrics(&mut self) {
        let mut total_cpu = 0.0;
        let mut total_memory = 0.0;
        let mut total_messages = 0.0;
        let mut total_errors = 0.0;
        let server_count = self.server_metrics.len() as f32;

        for metrics in self.server_metrics.values() {
            total_cpu += metrics.cpu_usage;
            total_memory += metrics.memory_usage;
            total_messages += metrics.messages_per_second;
            total_errors += metrics.error_rate;
        }

        self.global_metrics = GlobalPerformanceMetrics {
            total_throughput: total_messages,
            average_latency: self.calculate_average_latency() as f32,
            system_efficiency: if server_count > 0.0 {
                1.0 - (total_errors / server_count)
            } else {
                1.0
            },
            resource_utilization: if server_count > 0.0 {
                (total_cpu + total_memory) / (2.0 * server_count)
            } else {
                0.0
            },
            error_rate: if server_count > 0.0 {
                total_errors / server_count
            } else {
                0.0
            },
            coordination_overhead: self.calculate_coordination_overhead() as f32,
        };
    }

    
    fn update_visualization(&mut self, _ctx: &mut Context<Self>) {
        if self.last_update.elapsed() >= self.update_interval {
            self.last_update = Instant::now();

            
            self.apply_physics_simulation();

            
            self.broadcast_position_update(time::timestamp_seconds());
        }
    }

    
    fn apply_physics_simulation(&mut self) {
        
        if self.agent_positions.is_empty() {
            return;
        }

        
        let dt = self.update_interval.as_secs_f32();
        let mut forces: HashMap<String, Vec3Data> = HashMap::new();

        
        let agent_ids: Vec<String> = self.agent_positions.keys().cloned().collect();
        for i in 0..agent_ids.len() {
            for j in (i + 1)..agent_ids.len() {
                let id1 = &agent_ids[i];
                let id2 = &agent_ids[j];

                if let (Some(pos1), Some(pos2)) =
                    (self.agent_positions.get(id1), self.agent_positions.get(id2))
                {
                    let dx = pos1.x - pos2.x;
                    let dy = pos1.y - pos2.y;
                    let dz = pos1.z - pos2.z;
                    let dist_sq = dx * dx + dy * dy + dz * dz + 0.1; 
                    let dist = dist_sq.sqrt();

                    let force_magnitude = self.physics_config.repel_k / dist_sq;
                    let fx = (dx / dist) * force_magnitude;
                    let fy = (dy / dist) * force_magnitude;
                    let fz = (dz / dist) * force_magnitude;

                    forces.entry(id1.clone()).or_insert_with(Vec3Data::zero).x += fx;
                    forces.entry(id1.clone()).or_insert_with(Vec3Data::zero).y += fy;
                    forces.entry(id1.clone()).or_insert_with(Vec3Data::zero).z += fz;

                    forces.entry(id2.clone()).or_insert_with(Vec3Data::zero).x -= fx;
                    forces.entry(id2.clone()).or_insert_with(Vec3Data::zero).y -= fy;
                    forces.entry(id2.clone()).or_insert_with(Vec3Data::zero).z -= fz;
                }
            }
        }

        
        for connection in self.connections.values() {
            if let (Some(pos1), Some(pos2)) = (
                self.agent_positions.get(&connection.source),
                self.agent_positions.get(&connection.target),
            ) {
                let dx = pos2.x - pos1.x;
                let dy = pos2.y - pos1.y;
                let dz = pos2.z - pos1.z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist > 0.0 {
                    let force_magnitude = self.physics_config.spring_k * dist * connection.strength;
                    let fx = (dx / dist) * force_magnitude;
                    let fy = (dy / dist) * force_magnitude;
                    let fz = (dz / dist) * force_magnitude;

                    forces
                        .entry(connection.source.clone())
                        .or_insert_with(Vec3Data::zero)
                        .x += fx;
                    forces
                        .entry(connection.source.clone())
                        .or_insert_with(Vec3Data::zero)
                        .y += fy;
                    forces
                        .entry(connection.source.clone())
                        .or_insert_with(Vec3Data::zero)
                        .z += fz;

                    forces
                        .entry(connection.target.clone())
                        .or_insert_with(Vec3Data::zero)
                        .x -= fx;
                    forces
                        .entry(connection.target.clone())
                        .or_insert_with(Vec3Data::zero)
                        .y -= fy;
                    forces
                        .entry(connection.target.clone())
                        .or_insert_with(Vec3Data::zero)
                        .z -= fz;
                }
            }
        }

        
        for (agent_id, force) in forces {
            if let Some(position) = self.agent_positions.get_mut(&agent_id) {
                position.x += force.x * dt;
                position.y += force.y * dt;
                position.z += force.z * dt;

                
                position.x *= self.physics_config.damping;
                position.y *= self.physics_config.damping;
                position.z *= self.physics_config.damping;

                
                position.x = position.x.clamp(-100.0, 100.0);
                position.y = position.y.clamp(-100.0, 100.0);
                position.z = position.z.clamp(-100.0, 100.0);
            }
        }
    }

    
    fn broadcast_initialization(&self) {
        let message = AgentVisualizationMessage::Initialize(InitializeMessage {
            timestamp: time::timestamp_seconds(),
            swarm_id: "multi_mcp_swarm".to_string(),
            session_uuid: None, 
            topology: "multi_server".to_string(),
            agents: self.agents.values().cloned().collect(),
            connections: self.connections.values().cloned().collect(),
            visual_config: self.visual_config.clone(),
            physics_config: self.physics_config.clone(),
            positions: self.agent_positions.clone(),
        });

        self.broadcast_message(message);
    }

    
    fn broadcast_position_update(&self, timestamp: i64) {
        let message = AgentVisualizationMessage::PositionUpdate(PositionUpdateMessage {
            timestamp,
            positions: self
                .agent_positions
                .iter()
                .map(|(id, pos)| PositionUpdate {
                    id: id.clone(),
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    vx: None, 
                    vy: None,
                    vz: None,
                })
                .collect(),
        });

        self.broadcast_message(message);
    }

    
    fn broadcast_state_update(&self) {
        let message = AgentVisualizationMessage::StateUpdate(StateUpdateMessage {
            timestamp: time::timestamp_seconds(),
            updates: self
                .agents
                .values()
                .map(|agent| AgentStateUpdate {
                    id: agent.id.clone(),
                    status: Some(agent.status.clone()),
                    health: Some(agent.health),
                    cpu: Some(agent.cpu),
                    memory: Some(agent.memory),
                    activity: Some(agent.activity),
                    tasks_active: Some(agent.tasks_active),
                    current_task: None, 
                })
                .collect(),
        });

        self.broadcast_message(message);
    }

    
    fn broadcast_connection_update(&self) {
        let message = AgentVisualizationMessage::ConnectionUpdate(ConnectionUpdateMessage {
            timestamp: time::timestamp_seconds(),
            added: self.connections.values().cloned().collect(),
            removed: Vec::new(), 
            updated: Vec::new(), 
        });

        self.broadcast_message(message);
    }

    
    fn broadcast_metrics_update(&self) {
        let message = AgentVisualizationMessage::MetricsUpdate(MetricsUpdateMessage {
            timestamp: time::timestamp_seconds(),
            overall: SwarmMetrics {
                total_agents: self.agents.len() as u32,
                active_agents: self
                    .agents
                    .values()
                    .filter(|a| a.status == "active")
                    .count() as u32,
                health_avg: if !self.agents.is_empty() {
                    self.agents.values().map(|a| a.health).sum::<f32>() / self.agents.len() as f32
                } else {
                    0.0
                },
                cpu_total: self.agents.values().map(|a| a.cpu).sum(),
                memory_total: self.agents.values().map(|a| a.memory).sum(),
                tokens_total: self.agents.values().map(|a| a.tokens).sum(),
                tokens_per_second: self.agents.values().map(|a| a.token_rate).sum(),
            },
            agent_metrics: self
                .agents
                .values()
                .map(|agent| AgentMetrics {
                    id: agent.id.clone(),
                    tokens: agent.tokens,
                    token_rate: agent.token_rate,
                    tasks_completed: agent.tasks_completed,
                    success_rate: agent.success_rate,
                })
                .collect(),
        });

        self.broadcast_message(message);
    }

    
    fn broadcast_message(&self, message: AgentVisualizationMessage) {
        let wrapped_message = AgentVisualizationMessageWrapper(message);
        let mut failed_subscribers = Vec::new();

        for (index, subscriber) in self.subscribers.iter().enumerate() {
            if let Err(_) = subscriber.try_send(wrapped_message.clone()) {
                failed_subscribers.push(index);
            }
        }

        
        if !failed_subscribers.is_empty() {
            warn!(
                "Failed to send message to {} subscribers",
                failed_subscribers.len()
            );
        }
    }

    
    fn calculate_load_distribution(&self) -> Vec<LayerLoad> {
        self.server_metrics
            .iter()
            .enumerate()
            .map(|(i, (_name, metrics))| LayerLoad {
                layer_id: i as u32,
                agent_count: 1, 
                average_load: metrics.cpu_usage + metrics.memory_usage,
                max_capacity: 100, 
                utilization: (metrics.cpu_usage + metrics.memory_usage) / 2.0,
            })
            .collect()
    }

    
    fn analyze_critical_paths(&self) -> Vec<CriticalPath> {
        
        self.server_metrics
            .iter()
            .filter(|(_, metrics)| metrics.error_rate > 0.1)
            .enumerate()
            .map(|(i, (name, metrics))| CriticalPath {
                path_id: format!("path_{}", i),
                agent_sequence: vec![name.clone()],
                total_latency_ms: metrics.network_latency,
                bottleneck_agent: Some(name.clone()),
            })
            .collect()
    }

    
    fn detect_bottlenecks(&self) -> Vec<Bottleneck> {
        let mut bottlenecks = Vec::new();

        
        for (name, metrics) in &self.server_metrics {
            if metrics.cpu_usage > 0.8 {
                bottlenecks.push(Bottleneck {
                    agent_id: name.clone(),
                    bottleneck_type: "cpu".to_string(),
                    severity: metrics.cpu_usage,
                    impact_agents: vec![], 
                    suggested_action: "Scale CPU resources".to_string(),
                });
            }
            if metrics.memory_usage > 0.8 {
                bottlenecks.push(Bottleneck {
                    agent_id: name.clone(),
                    bottleneck_type: "memory".to_string(),
                    severity: metrics.memory_usage,
                    impact_agents: vec![], 
                    suggested_action: "Scale memory resources".to_string(),
                });
            }
        }

        bottlenecks
    }

    
    fn calculate_average_latency(&self) -> f64 {
        if self.server_metrics.is_empty() {
            return 0.0;
        }

        let total_latency: f64 = self
            .server_metrics
            .values()
            .map(|metrics| metrics.network_latency as f64) 
            .sum();

        total_latency / self.server_metrics.len() as f64
    }

    
    fn calculate_coordination_overhead(&self) -> f64 {
        
        let server_count = self.server_metrics.len() as f64;
        let connection_count = self.connections.len() as f64;

        if server_count <= 1.0 {
            0.0
        } else {
            
            (connection_count / (server_count * server_count)).min(1.0)
        }
    }
}

impl Default for SwarmTopologyData {
    fn default() -> Self {
        Self {
            topology_type: "default".to_string(),
            total_agents: 0,
            coordination_layers: 0,
            efficiency_score: 1.0,
            load_distribution: Vec::new(),
            critical_paths: Vec::new(),
            bottlenecks: Vec::new(),
        }
    }
}

impl Default for GlobalPerformanceMetrics {
    fn default() -> Self {
        Self {
            total_throughput: 0.0,
            average_latency: 0.0,
            system_efficiency: 1.0,
            resource_utilization: 0.0,
            error_rate: 0.0,
            coordination_overhead: 0.0,
        }
    }
}
