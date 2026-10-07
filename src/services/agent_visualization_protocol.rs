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
    /// Free-form agent status string. Recognised values (extend freely — this is
    /// an unconstrained `String`, so adding values needs no schema change):
    /// `idle | busy | active | initializing | terminating | offline | error`,
    /// plus the XR-swarm 4-channel additions `blocked` and `done` (ADR: XR agent
    /// swarm, Pillar 3). The XR client derives its status halo via
    /// `render_store::agent_status_code`: busy/active/working ⇒ working,
    /// blocked/error ⇒ blocked, done/terminating/offline ⇒ done, else idle.
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MassFunction {
    /// `mass = 1.0 + log2(1 + degree)` — ADR-01 D6 default.
    #[default]
    Log,
    /// `mass = 1.0 + degree as f32`.
    Linear,
    /// `mass = 1.0 + (degree as f32).sqrt()`.
    Sqrt,
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
    /// Raw `did:nostr` claim as received on the agentbox agent record (spawn
    /// response or the agent-list snapshot — same record shape). Carried
    /// unvalidated here; the `uri::did_nostr()` gate runs at the
    /// `Agent::from(MultiMcpAgentStatus)` carry boundary (COM-14, WP-1). `None`
    /// when agentbox emits no DID for the agent.
    #[serde(default)]
    pub did_nostr: Option<String>,
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

/// Aggregate performance metrics across every discovered MCP swarm.
///
/// ADR-2066: retained when the dead `MultiMcpVisualizationMessage` family around
/// it was removed — this struct is still imported by
/// `src/actors/multi_mcp_visualization_actor.rs` and
/// `src/services/multi_mcp_agent_discovery.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalPerformanceMetrics {
    pub total_throughput: f32,
    pub average_latency: f32,
    pub system_efficiency: f32,
    pub resource_utilization: f32,
    pub error_rate: f32,
    pub coordination_overhead: f32,
}

// ADR-2066: a second `MultiMcpVisualizationMessage` enum (and its
// `DiscoveryMessage`/`SwarmInfo`/`GlobalTopology`/`TopologyUpdateMessage`/
// `NeuralUpdateMessage`/`PerformanceAnalysisMessage`/`CoordinationEventMessage`
// companion family, plus the `AgentVisualizationProtocol` methods that built
// them — `create_discovery_message`, `create_agent_update_message`,
// `create_topology_update`, `create_performance_analysis`, and their exclusive
// private helpers) used to live here. It duplicated the name of
// `src/actors/multi_mcp_visualization_actor.rs`'s own (live, actix `Handler`-backed)
// `MultiMcpVisualizationMessage` enum, was constructed nowhere in the running
// server, and was called only from `tests/examples/multi_mcp_integration_demo.rs`
// — a file cargo never compiles (not under `examples/`, no `[[test]]`/`[[example]]`
// entry) and which itself references a nonexistent `visionclaw_ext` crate. Removed.

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

    pub fn get_agent_count_by_server(&self, server_type: &McpServerType) -> u32 {
        self.agent_cache
            .values()
            .filter(|agent| {
                std::mem::discriminant(&agent.server_source) == std::mem::discriminant(server_type)
            })
            .count() as u32
    }

    pub fn needs_discovery(&self) -> bool {
        self.last_discovery
            .is_none_or(|last| time::now().signed_duration_since(last).num_seconds() > 30)
    }

    pub fn create_init_message(
        swarm_id: &str,
        topology: &str,
        agents: Vec<visionclaw_domain::types::claude_flow::AgentStatus>,
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
}

impl Default for AgentVisualizationProtocol {
    fn default() -> Self {
        Self::new()
    }
}
