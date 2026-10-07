// Internal Developer Configuration
// This file contains server-side only settings that are not exposed to clients
// These settings control internal behavior, performance tuning, and debug features

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static DEV_CONFIG: OnceLock<DevConfig> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevConfig {
    pub physics: PhysicsInternals,
    pub cuda: CudaInternals,
    pub network: NetworkInternals,
    pub rendering: RenderingInternals,
    pub performance: PerformanceInternals,
    pub debug: DebugInternals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsInternals {
    pub force_epsilon: f32,
    pub spring_length_multiplier: f32,
    pub spring_length_max: f32,
    pub spring_force_clamp_factor: f32,

    pub rest_length: f32,
    pub repulsion_cutoff: f32,
    pub repulsion_softening_epsilon: f32,
    pub center_gravity_k: f32,
    pub grid_cell_size: f32,
    pub warmup_iterations: u32,
    pub cooling_rate: f32,

    pub max_force: f32,
    pub max_velocity: f32,
    pub world_bounds_min: f32,
    pub world_bounds_max: f32,
    pub cell_size_lod: f32,
    pub k_neighbors_max: u32,
    pub anomaly_detection_radius: f32,
    pub learning_rate_default: f32,
    pub min_velocity_threshold: f32,
    pub stability_threshold: f32,

    pub norm_delta_cap: f32,
    pub position_constraint_attraction: f32,
    pub lof_score_min: f32,
    pub lof_score_max: f32,
    pub weight_precision_multiplier: f32,

    pub boundary_extreme_multiplier: f32,
    pub boundary_extreme_force_multiplier: f32,
    pub boundary_velocity_damping: f32,

    pub golden_ratio: f32,
    pub initial_radius_min: f32,
    pub initial_radius_range: f32,

    pub cross_graph_repulsion_scale: f32,
    pub cross_graph_spring_scale: f32,

    pub cluster_repulsion_scale: f32,
    pub importance_scale_factor: f32,

    pub repulsion_distance_squared_min: f32,
    pub stress_majorization_epsilon: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CudaInternals {
    pub warmup_iterations_default: u32,
    pub warmup_damping_start: f32,
    pub warmup_damping_end: f32,
    pub warmup_temperature_scale: f32,
    pub warmup_cooling_iterations: u32,

    pub max_kernel_time_ms: u32,
    pub max_gpu_failures: u32,
    pub debug_output_throttle: u32,
    pub debug_node_count: u32,

    pub max_nodes: u32,
    pub max_edges: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInternals {
    pub pool_max_idle_per_host: usize,
    pub pool_idle_timeout_secs: u64,
    pub pool_connect_timeout_secs: u64,

    pub circuit_failure_threshold: u32,
    pub circuit_recovery_timeout_secs: u64,
    pub circuit_half_open_max_requests: u32,

    pub max_retry_attempts: u32,
    pub retry_base_delay_ms: u64,
    pub retry_max_delay_ms: u64,
    pub retry_exponential_base: f32,

    pub ws_ping_interval_secs: u64,
    pub ws_pong_timeout_secs: u64,
    pub ws_frame_size: usize,
    pub ws_max_pending_messages: usize,

    pub rate_limit_burst_size: u32,
    pub rate_limit_refill_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderingInternals {
    pub agent_colors: AgentColors,

    pub agent_base_size: f32,
    pub agent_size_per_task: f32,
    pub agent_max_size: f32,
    pub node_base_radius: f32,

    pub pulse_speed: f32,
    pub rotate_speed: f32,
    pub glow_speed: f32,
    pub wave_speed: f32,

    pub lod_distance_high: f32,
    pub lod_distance_medium: f32,
    pub lod_distance_low: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentColors {
    pub coordinator: String,
    pub coder: String,
    pub architect: String,
    pub analyst: String,
    pub tester: String,
    pub researcher: String,
    pub reviewer: String,
    pub optimizer: String,
    pub documenter: String,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceInternals {
    pub batch_size_nodes: usize,
    pub batch_size_edges: usize,
    pub batch_timeout_ms: u64,

    pub cache_ttl_secs: u64,
    pub cache_max_entries: usize,
    pub cache_eviction_percentage: f32,

    pub worker_threads: usize,
    pub blocking_threads: usize,
    pub stack_size_mb: usize,

    pub gc_interval_secs: u64,
    pub memory_warning_threshold_mb: usize,
    pub memory_critical_threshold_mb: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugInternals {
    pub enable_cuda_debug: bool,
    pub enable_physics_debug: bool,
    pub enable_network_debug: bool,
    pub enable_memory_tracking: bool,
    pub enable_performance_tracking: bool,

    pub log_slow_operations_ms: u64,
    pub log_memory_usage_interval_secs: u64,
    pub profile_sample_rate: f32,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            physics: PhysicsInternals {
                force_epsilon: 1e-8,
                spring_length_multiplier: 5.0,
                spring_length_max: 10.0,
                spring_force_clamp_factor: 0.5,

                rest_length: 50.0,
                repulsion_cutoff: 50.0,
                repulsion_softening_epsilon: 0.0001,
                center_gravity_k: 0.005,
                grid_cell_size: 50.0,
                warmup_iterations: 100,
                cooling_rate: 0.001,

                max_force: crate::config::CANONICAL_MAX_FORCE,
                max_velocity: crate::config::CANONICAL_MAX_VELOCITY,
                world_bounds_min: -1000.0,
                world_bounds_max: 1000.0,
                cell_size_lod: 100.0,
                k_neighbors_max: 32,
                anomaly_detection_radius: 150.0,
                learning_rate_default: 0.1,
                min_velocity_threshold: 0.01,
                stability_threshold: 1e-4, // Relaxed: allows system to settle without over-damping

                norm_delta_cap: 100.0, // Cap SSSP delta to prevent ideal > world bounds
                position_constraint_attraction: 0.1,
                lof_score_min: 0.1,
                lof_score_max: 10.0,
                weight_precision_multiplier: 1000.0,

                boundary_extreme_multiplier: 2.0,
                boundary_extreme_force_multiplier: 10.0,
                boundary_velocity_damping: 0.5,
                golden_ratio: std::f32::consts::GOLDEN_RATIO,
                initial_radius_min: 100.0,
                initial_radius_range: 300.0,
                cross_graph_repulsion_scale: 0.3,
                cross_graph_spring_scale: 0.5,
                cluster_repulsion_scale: 0.5,
                importance_scale_factor: 1.0,
                repulsion_distance_squared_min: 100.0,
                stress_majorization_epsilon: 0.001,
            },
            cuda: CudaInternals {
                warmup_iterations_default: 200,
                warmup_damping_start: 0.98,
                warmup_damping_end: 0.85,
                warmup_temperature_scale: 0.0001,
                warmup_cooling_iterations: 5,
                max_kernel_time_ms: 5000,
                max_gpu_failures: 5,
                debug_output_throttle: 60,
                debug_node_count: 3,
                max_nodes: 1_000_000,
                max_edges: 10_000_000,
            },
            network: NetworkInternals {
                pool_max_idle_per_host: 32,
                pool_idle_timeout_secs: 90,
                pool_connect_timeout_secs: 10,
                circuit_failure_threshold: 5,
                circuit_recovery_timeout_secs: 30,
                circuit_half_open_max_requests: 3,
                max_retry_attempts: 3,
                retry_base_delay_ms: 100,
                retry_max_delay_ms: 30000,
                retry_exponential_base: 2.0,
                ws_ping_interval_secs: 30,
                ws_pong_timeout_secs: 10,
                ws_frame_size: 65536,
                ws_max_pending_messages: 100,
                rate_limit_burst_size: 10,
                rate_limit_refill_rate: 1.0,
            },
            rendering: RenderingInternals {
                agent_colors: AgentColors {
                    coordinator: "#00FFFF".to_string(),
                    coder: "#00FF00".to_string(),
                    architect: "#FFA500".to_string(),
                    analyst: "#9370DB".to_string(),
                    tester: "#FF6347".to_string(),
                    researcher: "#FFD700".to_string(),
                    reviewer: "#4169E1".to_string(),
                    optimizer: "#7FFFD4".to_string(),
                    documenter: "#FF69B4".to_string(),
                    default: "#CCCCCC".to_string(),
                },
                agent_base_size: 1.0,
                agent_size_per_task: 0.2,
                agent_max_size: 2.0,
                node_base_radius: 15.0,
                pulse_speed: 2.0,
                rotate_speed: 1.0,
                glow_speed: 0.5,
                wave_speed: 0.5,
                lod_distance_high: 100.0,
                lod_distance_medium: 500.0,
                lod_distance_low: 1000.0,
            },
            performance: PerformanceInternals {
                batch_size_nodes: 1000,
                batch_size_edges: 5000,
                batch_timeout_ms: 100,
                cache_ttl_secs: 300,
                cache_max_entries: 10000,
                cache_eviction_percentage: 0.2,
                worker_threads: 4,
                blocking_threads: 512,
                stack_size_mb: 2,
                gc_interval_secs: 60,
                memory_warning_threshold_mb: 1024,
                memory_critical_threshold_mb: 2048,
            },
            debug: DebugInternals {
                enable_cuda_debug: false,
                enable_physics_debug: false,
                enable_network_debug: false,
                enable_memory_tracking: false,
                enable_performance_tracking: false,
                log_slow_operations_ms: 100,
                log_memory_usage_interval_secs: 60,
                profile_sample_rate: 0.01,
            },
        }
    }
}

impl DevConfig {
    pub fn load() -> &'static Self {
        DEV_CONFIG.get_or_init(|| match std::fs::read_to_string("data/dev_config.toml") {
            Ok(content) => match toml::from_str::<DevConfig>(&content) {
                Ok(config) => {
                    log::info!("Loaded developer configuration from data/dev_config.toml");
                    config
                }
                Err(e) => {
                    log::warn!("Failed to parse dev_config.toml: {}, using defaults", e);
                    Self::default()
                }
            },
            Err(_) => {
                log::info!("No dev_config.toml found, using default developer configuration");
                Self::default()
            }
        })
    }

    pub fn get() -> &'static Self {
        Self::load()
    }

    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let toml_string = toml::to_string_pretty(self)?;
        std::fs::write(path, toml_string)?;
        Ok(())
    }
}

// Convenience functions for common access patterns
pub fn physics() -> &'static PhysicsInternals {
    &DevConfig::get().physics
}

pub fn cuda() -> &'static CudaInternals {
    &DevConfig::get().cuda
}

pub fn network() -> &'static NetworkInternals {
    &DevConfig::get().network
}

pub fn rendering() -> &'static RenderingInternals {
    &DevConfig::get().rendering
}

pub fn performance() -> &'static PerformanceInternals {
    &DevConfig::get().performance
}

pub fn debug() -> &'static DebugInternals {
    &DevConfig::get().debug
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_golden_ratio_is_phi() {
        let phi = (1.0_f64 + 5.0_f64.sqrt()) / 2.0;
        let got = DevConfig::default().physics.golden_ratio;
        assert_eq!(got, phi as f32);
    }
}
