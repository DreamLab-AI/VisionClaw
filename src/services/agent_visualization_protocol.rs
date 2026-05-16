use crate::time;
use crate::utils::json::to_json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentVisualizationMessage {
    
    #[serde(rename = "init")]
    Initialize(InitializeMessage),

    
    #[serde(rename = "positions")]
    PositionUpdate(PositionUpdateMessage),

    
    #[serde(rename = "state")]
    StateUpdate(StateUpdateMessage),

    
    #[serde(rename = "connections")]
    ConnectionUpdate(ConnectionUpdateMessage),

    
    #[serde(rename = "metrics")]
    MetricsUpdate(MetricsUpdateMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeMessage {
    pub timestamp: i64, 
    pub swarm_id: String,
    pub session_uuid: Option<String>, 
    pub topology: String,

    
    pub agents: Vec<AgentInit>,

    
    pub connections: Vec<ConnectionInit>,

    
    pub visual_config: VisualConfig,

    
    pub physics_config: PhysicsConfig,

    
    pub positions: HashMap<String, Position>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInit {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub status: String,

    
    pub color: String,
    pub shape: String, 
    pub size: f32,

    
    pub health: f32,
    pub cpu: f32,
    pub memory: f32,
    pub activity: f32,

    
    pub tasks_active: u32,
    pub tasks_completed: u32,
    pub success_rate: f32,

    
    pub tokens: u64,
    pub token_rate: f32,

    
    pub capabilities: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInit {
    pub id: String,
    pub source: String,
    pub target: String,
    pub strength: f32,  
    pub flow_rate: f32, 
    pub color: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdateMessage {
    pub timestamp: i64,
    pub positions: Vec<PositionUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdate {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    
    pub vx: Option<f32>,
    pub vy: Option<f32>,
    pub vz: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateUpdateMessage {
    pub timestamp: i64,
    pub updates: Vec<AgentStateUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStateUpdate {
    pub id: String,
    pub status: Option<String>,
    pub health: Option<f32>,
    pub cpu: Option<f32>,
    pub memory: Option<f32>,
    pub activity: Option<f32>,
    pub tasks_active: Option<u32>,
    pub current_task: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionUpdateMessage {
    pub timestamp: i64,
    pub added: Vec<ConnectionInit>,
    pub removed: Vec<String>, 
    pub updated: Vec<ConnectionStateUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStateUpdate {
    pub id: String,
    pub active: Option<bool>,
    pub flow_rate: Option<f32>,
    pub strength: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsUpdateMessage {
    pub timestamp: i64,
    pub overall: SwarmMetrics,
    pub agent_metrics: Vec<AgentMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmMetrics {
    pub total_agents: u32,
    pub active_agents: u32,
    pub health_avg: f32,
    pub cpu_total: f32,
    pub memory_total: f32,
    pub tokens_total: u64,
    pub tokens_per_second: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub id: String,
    pub tokens: u64,
    pub token_rate: f32,
    pub tasks_completed: u32,
    pub success_rate: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisualConfig {
    pub colors: HashMap<String, String>,
    pub sizes: HashMap<String, f32>,
    pub animations: HashMap<String, AnimationConfig>,
    pub effects: EffectsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnimationConfig {
    pub speed: f32,
    pub amplitude: f32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EffectsConfig {
    pub glow: bool,
    pub particles: bool,
    pub bloom: bool,
    pub shadows: bool,
}

/// Mass-derivation strategy (ADR-01 D6 / R3). `Log` is the recommended
/// default; `Linear` and `Sqrt` are exposed so the empirical choice can be
/// re-evaluated per graph topology without recompiling.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MassFunction {
    /// `mass = 1.0 + log2(1 + degree)` — ADR-01 D6 default.
    Log,
    /// `mass = 1.0 + degree as f32`.
    Linear,
    /// `mass = 1.0 + (degree as f32).sqrt()`.
    Sqrt,
}

impl Default for MassFunction {
    fn default() -> Self {
        MassFunction::Log
    }
}

impl MassFunction {
    /// Apply the mass function to a node degree.
    pub fn apply(self, degree: u32) -> f32 {
        let d = degree as f32;
        match self {
            MassFunction::Log => 1.0_f32 + (1.0_f32 + d).log2(),
            MassFunction::Linear => 1.0_f32 + d,
            MassFunction::Sqrt => 1.0_f32 + d.sqrt(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsConfig {
    pub spring_k: f32,
    pub link_distance: f32,
    pub damping: f32,
    pub repel_k: f32,
    pub gravity_k: f32,
    pub max_velocity: f32,
    /// Mass-derivation strategy (ADR-01 D6 / R3). Defaults to `Log`.
    #[serde(default)]
    pub mass_function: MassFunction,
    /// Settlement hysteresis window in physics ticks (ADR-01 D9). Default 10.
    #[serde(default = "PhysicsConfig::default_settlement_window")]
    pub settlement_window: u32,
    /// `PhysicsGpuBuffers::resize` emits a warning when capacity exceeds this
    /// value. Not a hard limit. Default `INITIAL_CAPACITY_CEILING = 16384`.
    #[serde(default = "PhysicsConfig::default_max_nodes_warning")]
    pub max_nodes_warning: u32,
}

impl PhysicsConfig {
    fn default_settlement_window() -> u32 {
        10
    }
    fn default_max_nodes_warning() -> u32 {
        16_384
    }
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            spring_k: 0.05,
            link_distance: 50.0,
            damping: 0.9,
            repel_k: 5000.0,
            gravity_k: 0.01,
            max_velocity: crate::config::CANONICAL_MAX_VELOCITY,
            mass_function: MassFunction::default(),
            settlement_window: Self::default_settlement_window(),
            max_nodes_warning: Self::default_max_nodes_warning(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub server_id: String,
    pub server_type: McpServerType,
    pub host: String,
    pub port: u16,
    pub is_connected: bool,
    pub last_heartbeat: i64,
    pub supported_tools: Vec<String>,
    pub agent_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerType {
    ClaudeFlow,
    RuvSwarm,
    Daa,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiMcpAgentStatus {
    pub agent_id: String,
    pub swarm_id: String,
    pub server_source: McpServerType,
    pub name: String,
    pub agent_type: String,
    pub status: String,
    pub capabilities: Vec<String>,
    pub metadata: AgentExtendedMetadata,
    pub performance: AgentPerformanceData,
    pub neural_info: Option<NeuralAgentData>,
    pub created_at: i64,
    pub last_active: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExtendedMetadata {
    pub session_id: Option<String>,
    pub parent_id: Option<String>,
    pub topology_position: Option<TopologyPosition>,
    pub coordination_role: Option<String>,
    pub task_queue_size: u32,
    pub error_count: u32,
    pub warning_count: u32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyPosition {
    pub layer: u32,
    pub index_in_layer: u32,
    pub connections: Vec<String>, 
    pub is_coordinator: bool,
    pub coordination_level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPerformanceData {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub health_score: f32,
    pub activity_level: f32,
    pub tasks_active: u32,
    pub tasks_completed: u32,
    pub tasks_failed: u32,
    pub success_rate: f32,
    pub token_usage: u64,
    pub token_rate: f32,
    pub response_time_ms: f32,
    pub throughput: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralAgentData {
    pub model_type: String,
    pub model_size: String,
    pub training_status: String,
    pub cognitive_pattern: String,
    pub learning_rate: f32,
    pub adaptation_score: f32,
    pub memory_capacity: u64,
    pub knowledge_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTopologyData {
    pub topology_type: String,
    pub total_agents: u32,
    pub coordination_layers: u32,
    pub efficiency_score: f32,
    pub load_distribution: Vec<LayerLoad>,
    pub critical_paths: Vec<CriticalPath>,
    pub bottlenecks: Vec<Bottleneck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerLoad {
    pub layer_id: u32,
    pub agent_count: u32,
    pub average_load: f32,
    pub max_capacity: u32,
    pub utilization: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalPath {
    pub path_id: String,
    pub agent_sequence: Vec<String>,
    pub total_latency_ms: f32,
    pub bottleneck_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bottleneck {
    pub agent_id: String,
    pub bottleneck_type: String,
    pub severity: f32,
    pub impact_agents: Vec<String>,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MultiMcpVisualizationMessage {
    
    #[serde(rename = "discovery")]
    Discovery(DiscoveryMessage),

    
    #[serde(rename = "multi_agent_update")]
    MultiAgentUpdate(MultiAgentUpdateMessage),

    
    #[serde(rename = "topology_update")]
    TopologyUpdate(TopologyUpdateMessage),

    
    #[serde(rename = "neural_update")]
    NeuralUpdate(NeuralUpdateMessage),

    
    #[serde(rename = "performance_analysis")]
    PerformanceAnalysis(PerformanceAnalysisMessage),

    
    #[serde(rename = "coordination_event")]
    CoordinationEvent(CoordinationEventMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryMessage {
    pub timestamp: i64,
    pub servers: Vec<McpServerInfo>,
    pub total_agents: u32,
    pub swarms: Vec<SwarmInfo>,
    pub global_topology: GlobalTopology,
    
    pub session_registry: std::collections::HashMap<String, SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub uuid: String,
    pub swarm_id: Option<String>,
    pub task: String,
    pub created_at: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmInfo {
    pub swarm_id: String,
    pub server_source: McpServerType,
    pub topology: String,
    pub agent_count: u32,
    pub health_score: f32,
    pub coordination_efficiency: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalTopology {
    pub inter_swarm_connections: Vec<InterSwarmConnection>,
    pub coordination_hierarchy: Vec<CoordinationLevel>,
    pub data_flow_patterns: Vec<DataFlowPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterSwarmConnection {
    pub source_swarm: String,
    pub target_swarm: String,
    pub connection_strength: f32,
    pub message_rate: f32,
    pub coordination_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationLevel {
    pub level: u32,
    pub coordinator_agents: Vec<String>,
    pub managed_agents: Vec<String>,
    pub coordination_load: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFlowPattern {
    pub pattern_id: String,
    pub source_agents: Vec<String>,
    pub target_agents: Vec<String>,
    pub flow_rate: f32,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentUpdateMessage {
    pub timestamp: i64,
    pub agents: Vec<MultiMcpAgentStatus>,
    pub differential_updates: Vec<AgentDifferentialUpdate>,
    pub removed_agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDifferentialUpdate {
    pub agent_id: String,
    pub field_updates: std::collections::HashMap<String, serde_json::Value>,
    pub performance_delta: Option<PerformanceDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceDelta {
    pub cpu_change: f32,
    pub memory_change: f32,
    pub task_completion_rate: f32,
    pub error_rate_change: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyUpdateMessage {
    pub timestamp: i64,
    pub swarm_id: String,
    pub topology_changes: Vec<TopologyChange>,
    pub new_connections: Vec<AgentConnection>,
    pub removed_connections: Vec<String>,
    pub coordination_updates: Vec<CoordinationUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyChange {
    pub change_type: String,
    pub affected_agents: Vec<String>,
    pub new_structure: Option<serde_json::Value>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConnection {
    pub connection_id: String,
    pub source_agent: String,
    pub target_agent: String,
    pub connection_type: String,
    pub strength: f32,
    pub bidirectional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationUpdate {
    pub coordinator_id: String,
    pub managed_agents: Vec<String>,
    pub coordination_load: f32,
    pub efficiency_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralUpdateMessage {
    pub timestamp: i64,
    pub neural_agents: Vec<NeuralAgentUpdate>,
    pub learning_events: Vec<LearningEvent>,
    pub adaptation_metrics: Vec<AdaptationMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralAgentUpdate {
    pub agent_id: String,
    pub neural_data: NeuralAgentData,
    pub learning_progress: f32,
    pub recent_adaptations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningEvent {
    pub event_id: String,
    pub agent_id: String,
    pub event_type: String,
    pub learning_data: serde_json::Value,
    pub performance_impact: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationMetric {
    pub metric_name: String,
    pub current_value: f32,
    pub target_value: f32,
    pub progress: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceAnalysisMessage {
    pub timestamp: i64,
    pub global_metrics: GlobalPerformanceMetrics,
    pub bottlenecks: Vec<Bottleneck>,
    pub optimization_suggestions: Vec<OptimizationSuggestion>,
    pub trend_analysis: Vec<TrendAnalysis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalPerformanceMetrics {
    pub total_throughput: f32,
    pub average_latency: f32,
    pub system_efficiency: f32,
    pub resource_utilization: f32,
    pub error_rate: f32,
    pub coordination_overhead: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSuggestion {
    pub suggestion_id: String,
    pub target_component: String,
    pub optimization_type: String,
    pub expected_improvement: f32,
    pub implementation_complexity: String,
    pub risk_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    pub metric_name: String,
    pub trend_direction: String,
    pub rate_of_change: f32,
    pub confidence: f32,
    pub prediction_horizon_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationEventMessage {
    pub timestamp: i64,
    pub event_type: String,
    pub source_agent: String,
    pub target_agents: Vec<String>,
    pub event_data: serde_json::Value,
    pub coordination_impact: f32,
}

pub struct AgentVisualizationProtocol {
    _update_interval_ms: u64,
    position_buffer: Vec<PositionUpdate>,
    mcp_servers: std::collections::HashMap<String, McpServerInfo>,
    agent_cache: std::collections::HashMap<String, MultiMcpAgentStatus>,
    topology_cache: std::collections::HashMap<String, SwarmTopologyData>,
    last_discovery: Option<chrono::DateTime<chrono::Utc>>,

    
    session_uuid_map: std::collections::HashMap<String, String>, 
    session_metadata: std::collections::HashMap<String, SessionMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub uuid: String,
    pub swarm_id: Option<String>,
    pub task: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub working_dir: String,
    pub output_dir: String,
}

impl AgentVisualizationProtocol {
    pub fn new() -> Self {
        Self {
            _update_interval_ms: 16, 
            position_buffer: Vec::new(),
            mcp_servers: std::collections::HashMap::new(),
            agent_cache: std::collections::HashMap::new(),
            topology_cache: std::collections::HashMap::new(),
            last_discovery: None,
            session_uuid_map: std::collections::HashMap::new(),
            session_metadata: std::collections::HashMap::new(),
        }
    }

    
    pub fn register_session(&mut self, uuid: String, metadata: SessionMetadata) {
        log::info!("Registering session {} with metadata", uuid);
        self.session_metadata.insert(uuid, metadata);
    }

    
    pub fn link_swarm_to_session(&mut self, swarm_id: String, session_uuid: String) {
        log::info!("Linking swarm {} to session {}", swarm_id, session_uuid);
        self.session_uuid_map
            .insert(swarm_id.clone(), session_uuid.clone());

        
        if let Some(metadata) = self.session_metadata.get_mut(&session_uuid) {
            metadata.swarm_id = Some(swarm_id);
        }
    }

    
    pub fn get_session_for_swarm(&self, swarm_id: &str) -> Option<&String> {
        self.session_uuid_map.get(swarm_id)
    }

    
    pub fn get_session_metadata(&self, uuid: &str) -> Option<&SessionMetadata> {
        self.session_metadata.get(uuid)
    }

    
    pub fn register_mcp_server(&mut self, server_info: McpServerInfo) {
        log::info!(
            "Registering MCP server: {} ({}:{})",
            server_info.server_id,
            server_info.host,
            server_info.port
        );
        self.mcp_servers
            .insert(server_info.server_id.clone(), server_info);
    }

    
    pub fn update_agents_from_server(
        &mut self,
        server_type: McpServerType,
        agents: Vec<MultiMcpAgentStatus>,
    ) {
        for agent in agents {
            self.agent_cache.insert(agent.agent_id.clone(), agent);
        }
        log::debug!(
            "Updated {} agents from {:?} server",
            self.agent_cache.len(),
            server_type
        );
    }

    
    pub fn create_discovery_message(&mut self) -> String {
        let timestamp = time::now();
        self.last_discovery = Some(timestamp);

        let servers: Vec<McpServerInfo> = self.mcp_servers.values().cloned().collect();
        let total_agents = self.agent_cache.len() as u32;

        
        let mut swarms: std::collections::HashMap<String, Vec<&MultiMcpAgentStatus>> =
            std::collections::HashMap::new();
        for agent in self.agent_cache.values() {
            swarms
                .entry(agent.swarm_id.clone())
                .or_insert_with(Vec::new)
                .push(agent);
        }

        let swarm_infos: Vec<SwarmInfo> = swarms
            .into_iter()
            .map(|(swarm_id, agents)| {
                let total_health: f32 = agents.iter().map(|a| a.performance.health_score).sum();
                let avg_health = if !agents.is_empty() {
                    total_health / agents.len() as f32
                } else {
                    0.0
                };

                SwarmInfo {
                    swarm_id,
                    server_source: agents
                        .first()
                        .map(|a| a.server_source.clone())
                        .unwrap_or(McpServerType::Custom("unknown".to_string())),
                    topology: agents
                        .first()
                        .and_then(|a| a.metadata.topology_position.as_ref())
                        .map(|tp| {
                            if tp.is_coordinator {
                                "hierarchical"
                            } else {
                                "mesh"
                            }
                        })
                        .unwrap_or("mesh")
                        .to_string(),
                    agent_count: agents.len() as u32,
                    health_score: avg_health,
                    coordination_efficiency: {
                        let active_tasks: u32 =
                            agents.iter().map(|a| a.performance.tasks_active).sum();
                        let total_agents = agents.len() as u32;
                        if total_agents > 0 {
                            let load_balance =
                                1.0 - (active_tasks as f32 / (total_agents as f32 * 5.0)).min(1.0);
                            let health_factor = avg_health;
                            (load_balance * 0.6 + health_factor * 0.4).clamp(0.0, 1.0)
                        } else {
                            0.0
                        }
                    },
                }
            })
            .collect();

        let global_topology = GlobalTopology {
            inter_swarm_connections: self.discover_inter_swarm_connections(),
            coordination_hierarchy: self.build_coordination_hierarchy(),
            data_flow_patterns: self.analyze_data_flow_patterns(),
        };

        
        let session_registry: std::collections::HashMap<String, SessionInfo> = self
            .session_metadata
            .iter()
            .map(|(uuid, metadata)| {
                (
                    uuid.clone(),
                    SessionInfo {
                        uuid: uuid.clone(),
                        swarm_id: metadata.swarm_id.clone(),
                        task: metadata.task.clone(),
                        created_at: metadata.created_at.timestamp(),
                        status: "running".to_string(), 
                    },
                )
            })
            .collect();

        let discovery = DiscoveryMessage {
            timestamp: timestamp.timestamp_millis(),
            servers,
            total_agents,
            swarms: swarm_infos,
            global_topology,
            session_registry,
        };

        let message = MultiMcpVisualizationMessage::Discovery(discovery);
        to_json(&message).unwrap_or_default()
    }

    
    pub fn create_agent_update_message(&self, updated_agents: Vec<MultiMcpAgentStatus>) -> String {
        let differential_updates: Vec<AgentDifferentialUpdate> = updated_agents
            .iter()
            .map(|agent| {
                let mut field_updates = std::collections::HashMap::new();
                field_updates.insert("status".to_string(), serde_json::json!(agent.status));
                field_updates.insert(
                    "last_active".to_string(),
                    serde_json::json!(agent.last_active),
                );

                let performance_delta = PerformanceDelta {
                    cpu_change: self
                        .calculate_cpu_delta(&agent.agent_id, agent.performance.cpu_usage),
                    memory_change: self
                        .calculate_memory_delta(&agent.agent_id, agent.performance.memory_usage),
                    task_completion_rate: agent.performance.success_rate,
                    error_rate_change: self
                        .calculate_error_rate_delta(&agent.agent_id, &agent.performance),
                };

                AgentDifferentialUpdate {
                    agent_id: agent.agent_id.clone(),
                    field_updates,
                    performance_delta: Some(performance_delta),
                }
            })
            .collect();

        let update_msg = MultiAgentUpdateMessage {
            timestamp: time::timestamp_millis(),
            agents: updated_agents,
            differential_updates,
            removed_agents: self.get_removed_agents(),
        };

        let message = MultiMcpVisualizationMessage::MultiAgentUpdate(update_msg);
        to_json(&message).unwrap_or_default()
    }

    
    pub fn create_topology_update(
        &mut self,
        swarm_id: String,
        topology_data: SwarmTopologyData,
    ) -> String {
        self.topology_cache
            .insert(swarm_id.clone(), topology_data.clone());

        let topology_update = TopologyUpdateMessage {
            timestamp: time::timestamp_millis(),
            swarm_id,
            topology_changes: self.detect_topology_changes(&topology_data),
            new_connections: self.get_new_connections(),
            removed_connections: self.get_removed_connections(),
            coordination_updates: self.get_coordination_updates(),
        };

        let message = MultiMcpVisualizationMessage::TopologyUpdate(topology_update);
        to_json(&message).unwrap_or_default()
    }

    
    pub fn create_performance_analysis(&self) -> String {
        let agents: Vec<&MultiMcpAgentStatus> = self.agent_cache.values().collect();

        let total_throughput: f32 = agents.iter().map(|a| a.performance.throughput).sum();
        let avg_latency: f32 = if !agents.is_empty() {
            agents
                .iter()
                .map(|a| a.performance.response_time_ms)
                .sum::<f32>()
                / agents.len() as f32
        } else {
            0.0
        };

        let global_metrics = GlobalPerformanceMetrics {
            total_throughput,
            average_latency: avg_latency,
            system_efficiency: self.calculate_system_efficiency(&agents),
            resource_utilization: agents
                .iter()
                .map(|a| (a.performance.cpu_usage + a.performance.memory_usage) / 2.0)
                .sum::<f32>()
                / agents.len().max(1) as f32,
            error_rate: agents
                .iter()
                .map(|a| {
                    a.performance.tasks_failed as f32
                        / (a.performance.tasks_completed + a.performance.tasks_failed).max(1) as f32
                })
                .sum::<f32>()
                / agents.len().max(1) as f32,
            coordination_overhead: self.calculate_coordination_overhead(&agents),
        };

        
        let bottlenecks: Vec<Bottleneck> = agents
            .iter()
            .filter_map(|agent| {
                if agent.performance.cpu_usage > 0.9 || agent.performance.memory_usage > 0.9 {
                    Some(Bottleneck {
                        agent_id: agent.agent_id.clone(),
                        bottleneck_type: if agent.performance.cpu_usage > 0.9 {
                            "cpu"
                        } else {
                            "memory"
                        }
                        .to_string(),
                        severity: (agent.performance.cpu_usage + agent.performance.memory_usage)
                            / 2.0,
                        impact_agents: self.calculate_bottleneck_impact(&agent.agent_id),
                        suggested_action: "Scale resources or redistribute workload".to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let optimization_suggestions =
            self.generate_optimization_suggestions(&agents, &bottlenecks);
        let trend_analysis = self.analyze_performance_trends(&agents);

        let performance_analysis = PerformanceAnalysisMessage {
            timestamp: time::timestamp_millis(),
            global_metrics,
            bottlenecks,
            optimization_suggestions,
            trend_analysis,
        };

        let message = MultiMcpVisualizationMessage::PerformanceAnalysis(performance_analysis);
        to_json(&message).unwrap_or_default()
    }

    
    pub fn get_agent_count_by_server(&self, server_type: &McpServerType) -> u32 {
        self.agent_cache
            .values()
            .filter(|agent| {
                std::mem::discriminant(&agent.server_source) == std::mem::discriminant(server_type)
            })
            .count() as u32
    }

    
    pub fn needs_discovery(&self) -> bool {
        self.last_discovery.map_or(true, |last| {
            time::now().signed_duration_since(last).num_seconds() > 30
        })
    }

    
    pub fn create_init_message(
        swarm_id: &str,
        topology: &str,
        agents: Vec<crate::types::claude_flow::AgentStatus>,
    ) -> String {
        use crate::services::agent_visualization_processor::AgentVisualizationProcessor;
use crate::utils::json::to_json;

        let mut processor = AgentVisualizationProcessor::new();
        let viz_data = processor.create_visualization_packet(
            agents,
            swarm_id.to_string(),
            topology.to_string(),
        );

        
        let init_agents: Vec<AgentInit> = viz_data
            .agents
            .into_iter()
            .map(|agent| AgentInit {
                id: agent.id,
                name: agent.name,
                agent_type: agent.agent_type,
                status: agent.status,
                color: agent.color,
                shape: match agent.shape_type {
                    crate::services::agent_visualization_processor::ShapeType::Sphere => "sphere",
                    crate::services::agent_visualization_processor::ShapeType::Cube => "cube",
                    crate::services::agent_visualization_processor::ShapeType::Octahedron => {
                        "octahedron"
                    }
                    crate::services::agent_visualization_processor::ShapeType::Cylinder => {
                        "cylinder"
                    }
                    crate::services::agent_visualization_processor::ShapeType::Torus => "torus",
                    crate::services::agent_visualization_processor::ShapeType::Cone => "cone",
                    crate::services::agent_visualization_processor::ShapeType::Pyramid => "pyramid",
                }
                .to_string(),
                size: agent.size,
                health: agent.health,
                cpu: agent.cpu_usage,
                memory: agent.memory_usage,
                activity: agent.activity_level,
                tasks_active: agent.active_tasks,
                tasks_completed: agent.completed_tasks,
                success_rate: agent.success_rate,
                tokens: agent.token_usage,
                token_rate: agent.token_rate,
                capabilities: agent.metadata.capabilities,
                created_at: agent.metadata.created_at.timestamp(),
            })
            .collect();

        let init_connections: Vec<ConnectionInit> = viz_data
            .connections
            .into_iter()
            .map(|conn| ConnectionInit {
                id: conn.id,
                source: conn.source_id,
                target: conn.target_id,
                strength: conn.strength,
                flow_rate: conn.flow_rate,
                color: conn.color,
                active: conn.is_active,
            })
            .collect();

        let visual_config = VisualConfig {
            colors: viz_data.visual_config.color_scheme,
            sizes: viz_data.visual_config.size_multipliers,
            animations: {
                let mut anims = HashMap::new();
                anims.insert(
                    "pulse".to_string(),
                    AnimationConfig {
                        speed: 1.0,
                        amplitude: 0.8,
                        enabled: true,
                    },
                );
                anims.insert(
                    "glow".to_string(),
                    AnimationConfig {
                        speed: 0.8,
                        amplitude: 0.6,
                        enabled: true,
                    },
                );
                anims.insert(
                    "rotate".to_string(),
                    AnimationConfig {
                        speed: 0.5,
                        amplitude: 1.0,
                        enabled: true,
                    },
                );
                anims
            },
            effects: EffectsConfig {
                glow: true,
                particles: true,
                bloom: true,
                shadows: false,
            },
        };

        let init_msg = InitializeMessage {
            timestamp: time::timestamp_seconds(),
            swarm_id: swarm_id.to_string(),
            session_uuid: None, 
            topology: topology.to_string(),
            agents: init_agents,
            connections: init_connections,
            visual_config,
            physics_config: viz_data.physics_config,
            positions: HashMap::new(), 
        };

        let message = AgentVisualizationMessage::Initialize(init_msg);
        to_json(&message).unwrap_or_default()
    }

    
    pub fn add_position_update(
        &mut self,
        id: String,
        x: f32,
        y: f32,
        z: f32,
        vx: f32,
        vy: f32,
        vz: f32,
    ) {
        self.position_buffer.push(PositionUpdate {
            id,
            x,
            y,
            z,
            vx: Some(vx),
            vy: Some(vy),
            vz: Some(vz),
        });
    }

    
    pub fn create_position_update(&mut self) -> Option<String> {
        if self.position_buffer.is_empty() {
            return None;
        }

        let msg = PositionUpdateMessage {
            timestamp: time::timestamp_millis(),
            positions: std::mem::take(&mut self.position_buffer),
        };

        let message = AgentVisualizationMessage::PositionUpdate(msg);
        Some(to_json(&message).unwrap_or_default())
    }

    
    pub fn create_state_update(updates: Vec<AgentStateUpdate>) -> String {
        let msg = StateUpdateMessage {
            timestamp: time::timestamp_millis(),
            updates,
        };

        let message = AgentVisualizationMessage::StateUpdate(msg);
        to_json(&message).unwrap_or_default()
    }

    
    fn discover_inter_swarm_connections(&self) -> Vec<InterSwarmConnection> {
        let mut connections = Vec::new();
        let swarm_ids: std::collections::HashSet<String> = self
            .agent_cache
            .values()
            .map(|a| a.swarm_id.clone())
            .collect();

        
        let swarm_list: Vec<_> = swarm_ids.into_iter().collect();
        for i in 0..swarm_list.len() {
            for j in (i + 1)..swarm_list.len() {
                connections.push(InterSwarmConnection {
                    source_swarm: swarm_list[i].clone(),
                    target_swarm: swarm_list[j].clone(),
                    connection_strength: 0.3, 
                    message_rate: 1.5,        
                    coordination_type: "peer".to_string(),
                });
            }
        }
        connections
    }

    fn build_coordination_hierarchy(&self) -> Vec<CoordinationLevel> {
        let coordinators: Vec<_> = self
            .agent_cache
            .values()
            .filter(|a| {
                a.metadata
                    .coordination_role
                    .as_ref()
                    .map_or(false, |r| r == "coordinator")
            })
            .collect();

        let mut levels = Vec::new();

        
        let top_coordinators: Vec<String> = coordinators
            .iter()
            .filter(|c| {
                c.metadata
                    .topology_position
                    .as_ref()
                    .map_or(false, |tp| tp.coordination_level == 0)
            })
            .map(|c| c.agent_id.clone())
            .collect();

        if !top_coordinators.is_empty() {
            let managed: Vec<String> = self
                .agent_cache
                .values()
                .filter(|a| !coordinators.iter().any(|c| c.agent_id == a.agent_id))
                .map(|a| a.agent_id.clone())
                .collect();

            levels.push(CoordinationLevel {
                level: 0,
                coordinator_agents: top_coordinators.clone(),
                managed_agents: managed,
                coordination_load: top_coordinators.len() as f32 * 0.7,
            });
        }

        levels
    }

    fn analyze_data_flow_patterns(&self) -> Vec<DataFlowPattern> {
        let mut patterns = Vec::new();

        
        let coordinators: Vec<_> = self
            .agent_cache
            .values()
            .filter(|a| {
                a.metadata
                    .coordination_role
                    .as_ref()
                    .map_or(false, |r| r == "coordinator")
            })
            .collect();

        for coordinator in coordinators {
            let workers: Vec<String> = self
                .agent_cache
                .values()
                .filter(|a| {
                    a.agent_id != coordinator.agent_id && a.swarm_id == coordinator.swarm_id
                })
                .map(|a| a.agent_id.clone())
                .collect();

            if !workers.is_empty() {
                patterns.push(DataFlowPattern {
                    pattern_id: format!("coord-{}", coordinator.agent_id),
                    source_agents: vec![coordinator.agent_id.clone()],
                    target_agents: workers,
                    flow_rate: coordinator.performance.throughput,
                    data_type: "task_coordination".to_string(),
                });
            }
        }

        patterns
    }

    fn calculate_cpu_delta(&self, _agent_id: &str, current_cpu: f32) -> f32 {
        
        
        (current_cpu - 0.5).clamp(-0.2, 0.2)
    }

    fn calculate_memory_delta(&self, _agent_id: &str, current_memory: f32) -> f32 {
        
        (current_memory - 0.4).clamp(-0.1, 0.1)
    }

    fn calculate_error_rate_delta(
        &self,
        _agent_id: &str,
        performance: &AgentPerformanceData,
    ) -> f32 {
        let current_error_rate = if performance.tasks_completed + performance.tasks_failed > 0 {
            performance.tasks_failed as f32
                / (performance.tasks_completed + performance.tasks_failed) as f32
        } else {
            0.0
        };

        
        (current_error_rate - 0.05).clamp(-0.1, 0.1)
    }

    fn get_removed_agents(&self) -> Vec<String> {
        
        
        Vec::new()
    }

    fn detect_topology_changes(&self, _topology_data: &SwarmTopologyData) -> Vec<TopologyChange> {
        
        Vec::new()
    }

    fn get_new_connections(&self) -> Vec<AgentConnection> {
        
        Vec::new()
    }

    fn get_removed_connections(&self) -> Vec<String> {
        
        Vec::new()
    }

    fn get_coordination_updates(&self) -> Vec<CoordinationUpdate> {
        let coordinators: Vec<_> = self
            .agent_cache
            .values()
            .filter(|a| {
                a.metadata
                    .coordination_role
                    .as_ref()
                    .map_or(false, |r| r == "coordinator")
            })
            .collect();

        coordinators
            .into_iter()
            .map(|coord| {
                let managed_count = self
                    .agent_cache
                    .values()
                    .filter(|a| a.swarm_id == coord.swarm_id && a.agent_id != coord.agent_id)
                    .count();

                CoordinationUpdate {
                    coordinator_id: coord.agent_id.clone(),
                    managed_agents: self
                        .agent_cache
                        .values()
                        .filter(|a| a.swarm_id == coord.swarm_id && a.agent_id != coord.agent_id)
                        .map(|a| a.agent_id.clone())
                        .collect(),
                    coordination_load: (managed_count as f32 * 0.1).min(1.0),
                    efficiency_score: coord.performance.health_score,
                }
            })
            .collect()
    }

    fn calculate_system_efficiency(&self, agents: &[&MultiMcpAgentStatus]) -> f32 {
        if agents.is_empty() {
            return 0.0;
        }

        let total_throughput: f32 = agents.iter().map(|a| a.performance.throughput).sum();
        let avg_health: f32 = agents
            .iter()
            .map(|a| a.performance.health_score)
            .sum::<f32>()
            / agents.len() as f32;
        let resource_efficiency = 1.0
            - (agents
                .iter()
                .map(|a| (a.performance.cpu_usage + a.performance.memory_usage) / 2.0)
                .sum::<f32>()
                / agents.len() as f32);

        ((total_throughput / agents.len() as f32) * 0.4
            + avg_health * 0.3
            + resource_efficiency * 0.3)
            .min(1.0)
    }

    fn calculate_bottleneck_impact(&self, agent_id: &str) -> Vec<String> {
        
        if let Some(agent) = self.agent_cache.get(agent_id) {
            self.agent_cache
                .values()
                .filter(|a| a.swarm_id == agent.swarm_id && a.agent_id != agent_id)
                .map(|a| a.agent_id.clone())
                .take(3) 
                .collect()
        } else {
            Vec::new()
        }
    }

    fn calculate_coordination_overhead(&self, agents: &[&MultiMcpAgentStatus]) -> f32 {
        if agents.is_empty() {
            return 0.0;
        }

        let coordinator_count = agents
            .iter()
            .filter(|a| {
                a.metadata
                    .coordination_role
                    .as_ref()
                    .map_or(false, |r| r == "coordinator")
            })
            .count() as f32;

        let total_agents = agents.len() as f32;
        let coordinator_ratio = coordinator_count / total_agents;

        
        (coordinator_ratio * 0.3 + 0.05).min(0.8)
    }

    fn generate_optimization_suggestions(
        &self,
        agents: &[&MultiMcpAgentStatus],
        bottlenecks: &[Bottleneck],
    ) -> Vec<OptimizationSuggestion> {
        let mut suggestions = Vec::new();

        
        for bottleneck in bottlenecks {
            suggestions.push(OptimizationSuggestion {
                suggestion_id: format!("scale-{}", bottleneck.agent_id),
                target_component: bottleneck.agent_id.clone(),
                optimization_type: "resource_scaling".to_string(),
                expected_improvement: (1.0 - bottleneck.severity) * 100.0,
                implementation_complexity: "medium".to_string(),
                risk_level: "low".to_string(),
            });
        }

        
        let avg_cpu: f32 =
            agents.iter().map(|a| a.performance.cpu_usage).sum::<f32>() / agents.len() as f32;
        let high_load_agents: Vec<_> = agents
            .iter()
            .filter(|a| a.performance.cpu_usage > avg_cpu * 1.5)
            .collect();

        if !high_load_agents.is_empty() {
            suggestions.push(OptimizationSuggestion {
                suggestion_id: "load-balance".to_string(),
                target_component: "swarm".to_string(),
                optimization_type: "load_balancing".to_string(),
                expected_improvement: 25.0,
                implementation_complexity: "high".to_string(),
                risk_level: "medium".to_string(),
            });
        }

        suggestions
    }

    fn analyze_performance_trends(&self, agents: &[&MultiMcpAgentStatus]) -> Vec<TrendAnalysis> {
        let mut trends = Vec::new();

        if !agents.is_empty() {
            let avg_cpu: f32 =
                agents.iter().map(|a| a.performance.cpu_usage).sum::<f32>() / agents.len() as f32;
            let avg_memory: f32 = agents
                .iter()
                .map(|a| a.performance.memory_usage)
                .sum::<f32>()
                / agents.len() as f32;

            trends.push(TrendAnalysis {
                metric_name: "cpu_usage".to_string(),
                trend_direction: if avg_cpu > 0.7 {
                    "increasing"
                } else {
                    "stable"
                }
                .to_string(),
                rate_of_change: (avg_cpu - 0.5) * 0.1,
                confidence: 0.75,
                prediction_horizon_minutes: 15,
            });

            trends.push(TrendAnalysis {
                metric_name: "memory_usage".to_string(),
                trend_direction: if avg_memory > 0.6 {
                    "increasing"
                } else {
                    "stable"
                }
                .to_string(),
                rate_of_change: (avg_memory - 0.4) * 0.08,
                confidence: 0.80,
                prediction_horizon_minutes: 20,
            });
        }

        trends
    }
}
