//! Visual Analytics GPU Interface - Optimal data pipeline for GPU kernel
//!
//! Enhanced version with comprehensive GPU safety measures, memory bounds checking,
//! overflow protection, robust error handling, and designed to maximize A6000 throughput.

use cudarc::driver::{DeviceRepr, ValidAsZeroBits};
use serde::{Deserialize, Serialize};

use crate::utils::gpu_safety::GPUSafetyError;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub t: f32,
}

impl Vec4 {
    pub fn new(x: f32, y: f32, z: f32, t: f32) -> Result<Self, GPUSafetyError> {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !t.is_finite() {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid Vec4 components: ({}, {}, {}, {})", x, y, z, t),
            });
        }

        const MAX_VAL: f32 = 1e6;
        if x.abs() > MAX_VAL || y.abs() > MAX_VAL || z.abs() > MAX_VAL || t.abs() > MAX_VAL {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Vec4 components exceed safe bounds: ({}, {}, {}, {})",
                    x, y, z, t
                ),
            });
        }

        Ok(Self { x, y, z, t })
    }

    pub fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            t: 0.0,
        }
    }

    pub fn validate(&self) -> Result<(), GPUSafetyError> {
        Self::new(self.x, self.y, self.z, self.t)?;
        Ok(())
    }

    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.t * self.t).sqrt()
    }

    pub fn normalize(&self) -> Result<Self, GPUSafetyError> {
        let mag = self.magnitude();
        if mag < 1e-8 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: "Cannot normalize zero-magnitude vector".to_string(),
            });
        }
        Self::new(self.x / mag, self.y / mag, self.z / mag, self.t / mag)
    }
}

unsafe impl DeviceRepr for Vec4 {}
unsafe impl ValidAsZeroBits for Vec4 {}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct IsolationLayer {
    pub layer_id: i32,
    pub opacity: f32,
    pub z_offset: f32,

    pub focus_center: Vec4,
    pub focus_radius: f32,
    pub context_falloff: f32,

    pub importance_threshold: f32,
    pub community_filter: i32,
    pub topology_filter_mask: i32,
    pub temporal_range: [f32; 2],

    pub force_modulation: f32,
    pub edge_opacity: f32,
    pub color_scheme: i32,
}

impl IsolationLayer {
    pub fn new(layer_id: i32) -> Self {
        Self {
            layer_id,
            opacity: 1.0,
            z_offset: 0.0,
            focus_center: Vec4::zero(),
            focus_radius: 500.0,
            context_falloff: 0.001,
            importance_threshold: 0.0,
            community_filter: -1,
            topology_filter_mask: 0,
            temporal_range: [0.0, 1000.0],
            force_modulation: 1.0,
            edge_opacity: 1.0,
            color_scheme: 0,
        }
    }

    pub fn validate(&self) -> Result<(), GPUSafetyError> {
        if self.layer_id < 0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Layer ID cannot be negative: {}", self.layer_id),
            });
        }

        if !self.opacity.is_finite() || self.opacity < 0.0 || self.opacity > 1.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid opacity: {}", self.opacity),
            });
        }

        if !self.edge_opacity.is_finite() || self.edge_opacity < 0.0 || self.edge_opacity > 1.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid edge_opacity: {}", self.edge_opacity),
            });
        }

        self.focus_center
            .validate()
            .map_err(|_| GPUSafetyError::InvalidKernelParams {
                reason: "Invalid focus_center".to_string(),
            })?;

        if !self.focus_radius.is_finite() || self.focus_radius <= 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid focus_radius: {}", self.focus_radius),
            });
        }

        if !self.context_falloff.is_finite() || self.context_falloff < 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid context_falloff: {}", self.context_falloff),
            });
        }

        if !self.importance_threshold.is_finite()
            || self.importance_threshold < 0.0
            || self.importance_threshold > 1.0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Invalid importance_threshold: {}",
                    self.importance_threshold
                ),
            });
        }

        if !self.temporal_range[0].is_finite() || !self.temporal_range[1].is_finite() {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Invalid temporal_range: [{}, {}]",
                    self.temporal_range[0], self.temporal_range[1]
                ),
            });
        }

        if self.temporal_range[0] > self.temporal_range[1] {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Temporal range start {} > end {}",
                    self.temporal_range[0], self.temporal_range[1]
                ),
            });
        }

        if !self.force_modulation.is_finite() || self.force_modulation <= 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid force_modulation: {}", self.force_modulation),
            });
        }

        if !self.z_offset.is_finite() {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid z_offset: {}", self.z_offset),
            });
        }

        Ok(())
    }
}

unsafe impl DeviceRepr for IsolationLayer {}
unsafe impl ValidAsZeroBits for IsolationLayer {}

impl Default for IsolationLayer {
    fn default() -> Self {
        Self::new(0)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualAnalyticsParams {
    pub total_nodes: i32,
    pub total_edges: i32,
    pub active_layers: i32,
    pub hierarchy_depth: i32,

    pub current_frame: i32,
    pub time_step: f32,
    pub temporal_decay: f32,
    pub history_weight: f32,

    pub force_scale: [f32; 4],
    pub damping: [f32; 4],
    pub temperature: [f32; 4],

    pub rest_length: f32,
    pub repulsion_cutoff: f32,
    pub repulsion_softening_epsilon: f32,
    pub center_gravity_k: f32,
    pub grid_cell_size: f32,
    pub warmup_iterations: i32,
    pub cooling_rate: f32,

    pub boundary_extreme_multiplier: f32,
    pub boundary_extreme_force_multiplier: f32,
    pub boundary_velocity_damping: f32,

    pub isolation_strength: f32,
    pub focus_gamma: f32,
    pub primary_focus_node: i32,
    pub context_alpha: f32,

    pub complexity_threshold: f32,
    pub saliency_boost: f32,
    pub information_bandwidth: f32,

    pub community_algorithm: i32,
    pub modularity_resolution: f32,
    pub topology_update_interval: i32,

    pub semantic_influence: f32,
    pub drift_threshold: f32,
    pub embedding_dims: i32,

    pub camera_position: Vec4,
    pub viewport_bounds: Vec4,
    pub zoom_level: f32,
    pub time_window: f32,
}

impl Default for VisualAnalyticsParams {
    fn default() -> Self {
        Self {
            total_nodes: 0,
            total_edges: 0,
            active_layers: 1,
            hierarchy_depth: 1,

            current_frame: 0,
            time_step: 0.016,
            temporal_decay: 0.95,
            history_weight: 0.1,

            force_scale: [1.0, 0.8, 0.6, 0.4],
            damping: [0.9, 0.95, 0.98, 0.99],
            temperature: [1.0, 0.5, 0.25, 0.1],

            rest_length: 50.0,
            repulsion_cutoff: 100.0,
            repulsion_softening_epsilon: 1.0,
            center_gravity_k: 0.1,
            grid_cell_size: 100.0,
            warmup_iterations: 10,
            cooling_rate: 0.95,

            boundary_extreme_multiplier: 2.0,
            boundary_extreme_force_multiplier: 5.0,
            boundary_velocity_damping: 0.8,

            isolation_strength: 0.5,
            focus_gamma: 2.0,
            primary_focus_node: -1,
            context_alpha: 0.3,

            complexity_threshold: 0.7,
            saliency_boost: 1.5,
            information_bandwidth: 0.8,

            community_algorithm: 0,
            modularity_resolution: 1.0,
            topology_update_interval: 60,

            semantic_influence: 0.2,
            drift_threshold: 0.1,
            embedding_dims: 128,

            camera_position: Vec4::zero(),
            viewport_bounds: Vec4::new(0.0, 0.0, 1920.0, 1080.0).unwrap_or(Vec4::zero()),
            zoom_level: 1.0,
            time_window: 5.0,
        }
    }
}

impl VisualAnalyticsParams {
    pub fn validate(&self) -> Result<(), GPUSafetyError> {
        if self.total_nodes < 0
            || self.total_edges < 0
            || self.active_layers < 0
            || self.hierarchy_depth < 0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Negative counts: nodes={}, edges={}, layers={}, depth={}",
                    self.total_nodes, self.total_edges, self.active_layers, self.hierarchy_depth
                ),
            });
        }

        if self.total_nodes > 10_000_000 {
            return Err(GPUSafetyError::ResourceExhaustion {
                resource: "total_nodes".to_string(),
                current: self.total_nodes as usize,
                limit: 10_000_000,
            });
        }

        if self.total_edges > 50_000_000 {
            return Err(GPUSafetyError::ResourceExhaustion {
                resource: "total_edges".to_string(),
                current: self.total_edges as usize,
                limit: 50_000_000,
            });
        }

        if !self.rest_length.is_finite() || self.rest_length <= 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid rest_length: {}", self.rest_length),
            });
        }

        if !self.repulsion_cutoff.is_finite() || self.repulsion_cutoff <= 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid repulsion_cutoff: {}", self.repulsion_cutoff),
            });
        }

        if !self.repulsion_softening_epsilon.is_finite() || self.repulsion_softening_epsilon < 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Invalid repulsion_softening_epsilon: {}",
                    self.repulsion_softening_epsilon
                ),
            });
        }

        if !self.center_gravity_k.is_finite() || self.center_gravity_k < 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid center_gravity_k: {}", self.center_gravity_k),
            });
        }

        if !self.grid_cell_size.is_finite()
            || self.grid_cell_size <= 0.0
            || self.grid_cell_size > 1000.0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid grid_cell_size: {}", self.grid_cell_size),
            });
        }

        if self.warmup_iterations < 0 || self.warmup_iterations > 10000 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid warmup_iterations: {}", self.warmup_iterations),
            });
        }

        if !self.cooling_rate.is_finite() || self.cooling_rate < 0.0 || self.cooling_rate > 1.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid cooling_rate: {}", self.cooling_rate),
            });
        }

        if !self.boundary_extreme_multiplier.is_finite() || self.boundary_extreme_multiplier <= 0.0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Invalid boundary_extreme_multiplier: {}",
                    self.boundary_extreme_multiplier
                ),
            });
        }

        if !self.boundary_extreme_force_multiplier.is_finite()
            || self.boundary_extreme_force_multiplier <= 0.0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Invalid boundary_extreme_force_multiplier: {}",
                    self.boundary_extreme_force_multiplier
                ),
            });
        }

        if !self.boundary_velocity_damping.is_finite()
            || self.boundary_velocity_damping < 0.0
            || self.boundary_velocity_damping > 1.0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!(
                    "Invalid boundary_velocity_damping: {}",
                    self.boundary_velocity_damping
                ),
            });
        }

        if !self.time_step.is_finite() || self.time_step <= 0.0 || self.time_step > 1.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid time_step: {}", self.time_step),
            });
        }

        if !self.temporal_decay.is_finite()
            || self.temporal_decay < 0.0
            || self.temporal_decay > 1.0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid temporal_decay: {}", self.temporal_decay),
            });
        }

        if !self.history_weight.is_finite()
            || self.history_weight < 0.0
            || self.history_weight > 1.0
        {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid history_weight: {}", self.history_weight),
            });
        }

        for (i, &scale) in self.force_scale.iter().enumerate() {
            if !scale.is_finite() || scale <= 0.0 {
                return Err(GPUSafetyError::InvalidKernelParams {
                    reason: format!("Invalid force_scale[{}]: {}", i, scale),
                });
            }
        }

        for (i, &damp) in self.damping.iter().enumerate() {
            if !damp.is_finite() || damp < 0.0 || damp > 1.0 {
                return Err(GPUSafetyError::InvalidKernelParams {
                    reason: format!("Invalid damping[{}]: {}", i, damp),
                });
            }
        }

        for (i, &temp) in self.temperature.iter().enumerate() {
            if !temp.is_finite() || temp < 0.0 {
                return Err(GPUSafetyError::InvalidKernelParams {
                    reason: format!("Invalid temperature[{}]: {}", i, temp),
                });
            }
        }

        if !self.focus_gamma.is_finite() || self.focus_gamma <= 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid focus_gamma: {}", self.focus_gamma),
            });
        }

        if !self.zoom_level.is_finite() || self.zoom_level <= 0.0 {
            return Err(GPUSafetyError::InvalidKernelParams {
                reason: format!("Invalid zoom_level: {}", self.zoom_level),
            });
        }

        self.camera_position
            .validate()
            .map_err(|_| GPUSafetyError::InvalidKernelParams {
                reason: "Invalid camera_position".to_string(),
            })?;

        self.viewport_bounds
            .validate()
            .map_err(|_| GPUSafetyError::InvalidKernelParams {
                reason: "Invalid viewport_bounds".to_string(),
            })?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PerformanceMetrics {
    pub avg_kernel_time_ms: f32,
    pub avg_transfer_time_ms: f32,
    pub current_frame: u32,
    pub total_memory_allocated: usize,
    pub active_allocations: usize,
    pub gpu_memory_usage_mb: f32,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_layers: usize,
    pub kernel_execution_count: usize,
    pub last_validation_time: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum HealthLevel {
    Healthy,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct SafetyStatus {
    pub health_level: HealthLevel,
    pub should_use_cpu_fallback: bool,
    pub memory_usage_percentage: f64,
    pub active_allocations: usize,
    pub current_memory_mb: f32,
    pub max_memory_mb: f32,
    pub frames_processed: u32,
    pub average_kernel_time_ms: f32,
}

// Import canonical RenderData from gpu::types
// Note: frame field changed from i32 to u32 in canonical definition
pub use crate::gpu::types::RenderData;

pub struct VisualAnalyticsBuilder {
    params: VisualAnalyticsParams,
}

impl VisualAnalyticsBuilder {
    pub fn new() -> Self {
        Self {
            params: VisualAnalyticsParams {
                total_nodes: 0,
                total_edges: 0,
                active_layers: 1,
                hierarchy_depth: 3,
                current_frame: 0,
                time_step: 0.016,
                temporal_decay: 0.1,
                history_weight: 0.8,
                force_scale: [1.0, 0.5, 0.25, 0.125],
                damping: [0.9, 0.85, 0.8, 0.75],
                temperature: [1.0; 4],

                rest_length: 50.0,
                repulsion_cutoff: 50.0,
                repulsion_softening_epsilon: 0.0001,
                center_gravity_k: 0.0,
                grid_cell_size: 50.0,
                warmup_iterations: 100,
                cooling_rate: 0.001,
                boundary_extreme_multiplier: 2.0,
                boundary_extreme_force_multiplier: 10.0,
                boundary_velocity_damping: 0.5,
                isolation_strength: 1.0,
                focus_gamma: 2.2,
                primary_focus_node: -1,
                context_alpha: 0.3,
                complexity_threshold: 0.5,
                saliency_boost: 1.5,
                information_bandwidth: 100.0,
                community_algorithm: 0,
                modularity_resolution: 1.0,
                topology_update_interval: 30,
                semantic_influence: 0.7,
                drift_threshold: 0.1,
                embedding_dims: 16,
                camera_position: Vec4::zero(),
                viewport_bounds: Vec4 {
                    x: 2000.0,
                    y: 2000.0,
                    z: 1000.0,
                    t: 100.0,
                },
                zoom_level: 1.0,
                time_window: 100.0,
            },
        }
    }

    pub fn with_nodes(mut self, count: i32) -> Self {
        self.params.total_nodes = count;
        self
    }

    pub fn with_edges(mut self, count: i32) -> Self {
        self.params.total_edges = count;
        self
    }

    pub fn with_focus(mut self, node_id: i32, gamma: f32) -> Self {
        self.params.primary_focus_node = node_id;
        self.params.focus_gamma = gamma;
        self
    }

    pub fn with_temporal_decay(mut self, decay: f32) -> Self {
        self.params.temporal_decay = decay;
        self
    }

    pub fn build(self) -> VisualAnalyticsParams {
        self.params
    }
}

impl Default for VisualAnalyticsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec4_validation() {
        assert!(Vec4::new(1.0, 2.0, 3.0, 4.0).is_ok());

        assert!(Vec4::new(f32::NAN, 2.0, 3.0, 4.0).is_err());

        assert!(Vec4::new(f32::INFINITY, 2.0, 3.0, 4.0).is_err());

        assert!(Vec4::new(1e7, 2.0, 3.0, 4.0).is_err());
    }

    #[test]
    fn test_isolation_layer_validation() {
        let layer = IsolationLayer::new(0);
        assert!(layer.validate().is_ok());

        let layer = IsolationLayer::new(-1);
        assert!(layer.validate().is_err());

        let mut layer = IsolationLayer::new(0);
        layer.opacity = 1.5;
        assert!(layer.validate().is_err());

        let mut layer = IsolationLayer::new(0);
        layer.focus_radius = -10.0;
        assert!(layer.validate().is_err());
    }

    #[test]
    fn test_visual_analytics_params_validation() {
        let mut params = VisualAnalyticsParams {
            total_nodes: 1000,
            total_edges: 2000,
            active_layers: 1,
            hierarchy_depth: 3,
            current_frame: 0,
            time_step: 0.016,
            temporal_decay: 0.1,
            history_weight: 0.8,
            force_scale: [1.0, 0.5, 0.25, 0.125],
            damping: [0.9, 0.85, 0.8, 0.75],
            temperature: [1.0; 4],
            rest_length: 10.0,
            repulsion_cutoff: 50.0,
            repulsion_softening_epsilon: 0.001,
            center_gravity_k: 0.01,
            grid_cell_size: 20.0,
            warmup_iterations: 100,
            cooling_rate: 0.95,
            boundary_extreme_multiplier: 1.5,
            boundary_extreme_force_multiplier: 2.0,
            boundary_velocity_damping: 0.8,
            isolation_strength: 1.0,
            focus_gamma: 2.2,
            primary_focus_node: -1,
            context_alpha: 0.3,
            complexity_threshold: 0.5,
            saliency_boost: 1.5,
            information_bandwidth: 100.0,
            community_algorithm: 0,
            modularity_resolution: 1.0,
            topology_update_interval: 30,
            semantic_influence: 0.7,
            drift_threshold: 0.1,
            embedding_dims: 16,
            camera_position: Vec4::zero(),
            viewport_bounds: Vec4 {
                x: 2000.0,
                y: 2000.0,
                z: 1000.0,
                t: 100.0,
            },
            zoom_level: 1.0,
            time_window: 100.0,
        };

        assert!(params.validate().is_ok());

        params.total_nodes = -1;
        assert!(params.validate().is_err());

        params.total_nodes = 20_000_000;
        assert!(params.validate().is_err());

        params.total_nodes = 1000;
        params.time_step = -0.1;
        assert!(params.validate().is_err());
    }
}
