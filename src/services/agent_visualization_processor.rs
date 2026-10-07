use crate::config::dev_config;
use crate::time;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use visionclaw_domain::types::claude_flow::{AgentStatus, Vec3};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizedAgent {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub status: String,

    pub position: Vec3,
    pub velocity: Vec3,
    pub color: String,
    pub size: f32,
    pub glow_intensity: f32,

    pub health: f32,
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub activity_level: f32,

    pub active_tasks: u32,
    pub completed_tasks: u32,
    pub success_rate: f32,
    pub current_task: Option<String>,

    pub token_usage: u64,
    pub token_rate: f32,

    pub metadata: AgentMetadata,

    pub shape_type: ShapeType,
    pub animation_state: AnimationState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub created_at: DateTime<Utc>,
    pub age_seconds: u64,
    pub last_activity: DateTime<Utc>,
    pub capabilities: Vec<String>,
    pub error_count: u32,
    pub warning_count: u32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeType {
    Sphere,
    Cube,
    Octahedron,
    Cylinder,
    Torus,
    Cone,
    Pyramid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationState {
    Idle,
    Pulsing,
    Rotating,
    Bouncing,
    Glowing,
    Flashing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizedConnection {
    pub id: String,
    pub source_id: String,
    pub target_id: String,

    pub strength: f32,
    pub flow_rate: f32,
    pub color: String,
    pub particle_count: u32,

    pub data_volume: u64,
    pub message_count: u64,
    pub latency_ms: f32,
    pub error_rate: f32,

    pub is_active: bool,
    pub pulse_frequency: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmVisualization {
    pub swarm_id: String,
    pub topology: String,
    pub total_agents: u32,
    pub active_agents: u32,

    pub overall_health: f32,
    pub total_cpu_usage: f32,
    pub total_memory_usage: f32,
    pub total_token_usage: u64,
    pub tokens_per_second: f32,

    pub clusters: Vec<AgentCluster>,

    pub performance_history: Vec<PerformanceSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCluster {
    pub id: String,
    pub center: Vec3,
    pub radius: f32,
    pub agent_ids: Vec<String>,
    pub cluster_type: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSnapshot {
    pub timestamp: DateTime<Utc>,
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub active_tasks: u32,
    pub token_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVisualizationData {
    pub timestamp: DateTime<Utc>,
    pub swarm: SwarmVisualization,
    pub agents: Vec<VisualizedAgent>,
    pub connections: Vec<VisualizedConnection>,

    pub physics_config: PhysicsConfig,

    pub visual_config: VisualConfig,
}

// Use PhysicsConfig from agent_visualization_protocol module
use crate::services::agent_visualization_protocol::PhysicsConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualConfig {
    pub color_scheme: HashMap<String, String>,
    pub size_multipliers: HashMap<String, f32>,
    pub glow_settings: GlowSettings,
    pub animation_speeds: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlowSettings {
    pub base_intensity: f32,
    pub health_multiplier: f32,
    pub activity_multiplier: f32,
    pub error_intensity: f32,
}

pub struct AgentVisualizationProcessor {
    _performance_history: HashMap<String, Vec<PerformanceSnapshot>>,
    _last_update: DateTime<Utc>,
}

impl AgentVisualizationProcessor {
    pub fn new() -> Self {
        Self {
            _performance_history: HashMap::new(),
            _last_update: time::now(),
        }
    }

    pub fn process_agents(&mut self, agents: Vec<AgentStatus>) -> Vec<VisualizedAgent> {
        agents
            .into_iter()
            .map(|agent| {
                let agent_type = agent.agent_type.clone();

                let health = agent.health;
                let cpu_usage = agent.cpu_usage;
                let memory_usage = agent.memory_usage;
                let activity_level = agent.activity;

                let (color, shape, animation) =
                    self.get_visual_properties(&agent_type, &agent.status, health);

                let size = agent
                    .workload
                    .unwrap_or_else(|| 1.0 + (agent.active_tasks_count as f32 * 0.2).min(2.0));

                let glow_intensity = 0.3 + activity_level * 0.7;

                let token_usage = agent.tokens;
                let token_rate = agent.token_rate;

                let position = agent.position.unwrap_or_else(|| {
                    use rand::Rng;
                    let mut rng = rand::thread_rng();
                    let radius = 30.0;
                    let theta = rng.gen::<f32>() * 2.0 * std::f32::consts::PI;
                    let phi = rng.gen::<f32>() * std::f32::consts::PI;
                    let r = rng.gen::<f32>().powf(1.0 / 3.0) * radius;

                    Vec3 {
                        x: r * phi.sin() * theta.cos(),
                        y: r * phi.sin() * theta.sin(),
                        z: r * phi.cos(),
                    }
                });

                VisualizedAgent {
                    id: agent.agent_id.clone(),
                    name: agent.profile.name.clone(),
                    agent_type: agent_type.clone(),
                    status: agent.status.clone(),

                    position,
                    velocity: Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    color,
                    size,
                    glow_intensity,

                    health,
                    cpu_usage,
                    memory_usage,
                    activity_level,

                    active_tasks: agent.tasks_active,
                    completed_tasks: agent.tasks_completed,
                    success_rate: agent.success_rate_normalized,
                    current_task: agent.current_task_description.clone(),

                    token_usage,
                    token_rate,

                    metadata: AgentMetadata {
                        created_at: agent.timestamp,
                        age_seconds: agent.age,
                        last_activity: agent.timestamp,
                        capabilities: agent.capabilities.clone(),
                        error_count: agent.failed_tasks_count,
                        warning_count: 0,
                        tags: vec![agent_type.clone(), agent.status.clone()],
                    },

                    shape_type: shape,
                    animation_state: animation,
                }
            })
            .collect()
    }

    fn get_visual_properties(
        &self,
        agent_type: &str,
        status: &str,
        health: f32,
    ) -> (String, ShapeType, AnimationState) {
        let colors = &dev_config::rendering().agent_colors;
        let color = match agent_type {
            "coordinator" => &colors.coordinator,
            "coder" => &colors.coder,
            "architect" => &colors.architect,
            "analyst" => &colors.analyst,
            "tester" => &colors.tester,
            "researcher" => &colors.researcher,
            "reviewer" => &colors.reviewer,
            "optimizer" => &colors.optimizer,
            "documenter" => &colors.documenter,
            _ => &colors.default,
        }
        .to_string();

        let shape = match agent_type {
            "coordinator" => ShapeType::Octahedron,
            "architect" => ShapeType::Cone,
            "analyst" => ShapeType::Cylinder,
            "tester" => ShapeType::Torus,
            _ => match status {
                "error" => ShapeType::Pyramid,
                "busy" => ShapeType::Cube,
                _ => ShapeType::Sphere,
            },
        };

        let animation = match status {
            "error" => AnimationState::Flashing,
            "busy" => AnimationState::Rotating,
            "idle" => AnimationState::Idle,
            _ => {
                if health < 0.3 {
                    AnimationState::Pulsing
                } else {
                    AnimationState::Glowing
                }
            }
        };

        (color, shape, animation)
    }

    pub fn create_visualization_packet(
        &mut self,
        agents: Vec<AgentStatus>,
        swarm_id: String,
        topology: String,
    ) -> AgentVisualizationData {
        let processed_agents = self.process_agents(agents);

        let total_agents = processed_agents.len() as u32;
        let active_agents = processed_agents
            .iter()
            .filter(|a| a.status != "idle" && a.status != "error")
            .count() as u32;

        let overall_health =
            processed_agents.iter().map(|a| a.health).sum::<f32>() / total_agents.max(1) as f32;

        let total_cpu_usage = processed_agents.iter().map(|a| a.cpu_usage).sum::<f32>();

        let total_token_usage = processed_agents.iter().map(|a| a.token_usage).sum::<u64>();

        let tokens_per_second = processed_agents.iter().map(|a| a.token_rate).sum::<f32>();

        let connections = self.create_connections(&processed_agents);

        let clusters = self.create_clusters(&processed_agents);

        AgentVisualizationData {
            timestamp: time::now(),
            swarm: SwarmVisualization {
                swarm_id,
                topology,
                total_agents,
                active_agents,
                overall_health,
                total_cpu_usage,
                total_memory_usage: processed_agents.iter().map(|a| a.memory_usage).sum::<f32>(),
                total_token_usage,
                tokens_per_second,
                clusters,
                performance_history: self.get_performance_history(),
            },
            agents: processed_agents,
            connections,
            physics_config: PhysicsConfig {
                spring_k: 0.05,
                link_distance: 50.0,
                damping: 0.9,
                repel_k: 5000.0,
                gravity_k: 0.001,
                max_velocity: crate::config::CANONICAL_MAX_VELOCITY,
                ..PhysicsConfig::default()
            },
            visual_config: self.create_visual_config(),
        }
    }

    fn create_connections(&self, agents: &[VisualizedAgent]) -> Vec<VisualizedConnection> {
        let mut connections = Vec::new();

        for coordinator in agents.iter().filter(|a| a.agent_type == "coordinator") {
            for agent in agents.iter().filter(|a| a.id != coordinator.id) {
                connections.push(VisualizedConnection {
                    id: format!("{}-{}", coordinator.id, agent.id),
                    source_id: coordinator.id.clone(),
                    target_id: agent.id.clone(),
                    strength: 0.5,
                    flow_rate: agent.activity_level,
                    color: "#4444FF".to_string(),
                    particle_count: 10,
                    data_volume: 1000,
                    message_count: 10,
                    latency_ms: 5.0,
                    error_rate: 0.0,
                    is_active: agent.active_tasks > 0,
                    pulse_frequency: 1.0,
                });
            }
        }

        connections
    }

    fn create_clusters(&self, agents: &[VisualizedAgent]) -> Vec<AgentCluster> {
        let mut clusters = Vec::new();
        let mut type_groups: HashMap<String, Vec<String>> = HashMap::new();

        for agent in agents {
            type_groups
                .entry(agent.agent_type.clone())
                .or_default()
                .push(agent.id.clone());
        }

        for (agent_type, agent_ids) in type_groups {
            if agent_ids.len() > 1 {
                clusters.push(AgentCluster {
                    id: format!("cluster-{}", agent_type),
                    center: Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    radius: 15.0,
                    agent_ids,
                    cluster_type: agent_type.clone(),
                    color: self.get_cluster_color(&agent_type),
                });
            }
        }

        clusters
    }

    fn get_cluster_color(&self, agent_type: &str) -> String {
        match agent_type {
            "coordinator" => "#00FFFF33",
            "coder" => "#00FF0033",
            "architect" => "#FFA50033",
            _ => "#FFFFFF22",
        }
        .to_string()
    }

    fn create_visual_config(&self) -> VisualConfig {
        let mut color_scheme = HashMap::new();
        color_scheme.insert("background".to_string(), "#000033".to_string());
        color_scheme.insert("grid".to_string(), "#003366".to_string());
        color_scheme.insert("text".to_string(), "#FFFFFF".to_string());

        let mut size_multipliers = HashMap::new();
        size_multipliers.insert("coordinator".to_string(), 1.5);
        size_multipliers.insert("architect".to_string(), 1.3);
        size_multipliers.insert("default".to_string(), 1.0);

        let mut animation_speeds = HashMap::new();
        animation_speeds.insert("pulse".to_string(), 2.0);
        animation_speeds.insert("rotate".to_string(), 1.0);
        animation_speeds.insert("glow".to_string(), 0.5);

        VisualConfig {
            color_scheme,
            size_multipliers,
            glow_settings: GlowSettings {
                base_intensity: 0.3,
                health_multiplier: 0.5,
                activity_multiplier: 0.8,
                error_intensity: 1.0,
            },
            animation_speeds,
        }
    }

    fn get_performance_history(&self) -> Vec<PerformanceSnapshot> {
        let now = time::now();
        let mut history = Vec::new();

        for i in 0..10 {
            let timestamp = now - chrono::Duration::minutes(i);

            let variation = (i as f32 * 0.1).sin() * 0.1;

            history.push(PerformanceSnapshot {
                timestamp,
                cpu_usage: (0.4 + variation).clamp(0.0, 1.0),
                memory_usage: (0.3 + variation * 0.5).clamp(0.0, 1.0),
                active_tasks: (5.0 + variation * 10.0).max(0.0) as u32,
                token_rate: (10.0 + variation * 20.0).max(0.0),
            });
        }

        history.reverse();
        history
    }
}

impl Default for AgentVisualizationProcessor {
    fn default() -> Self {
        Self::new()
    }
}
