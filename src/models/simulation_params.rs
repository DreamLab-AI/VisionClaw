use crate::config::{AutoBalanceConfig, AutoPauseConfig, PhysicsSettings};
use crate::layout::types::LayoutMode;
use bytemuck::{Pod, Zeroable};
use cudarc::driver::DeviceRepr;
use cust_core::DeviceCopy;
use serde::{Deserialize, Serialize};

fn default_lin_log_mode() -> bool { true }
fn default_scaling_ratio() -> f32 { 10.0 }
fn default_adaptive_speed() -> bool { true }

/// Controls how the physics simulation converges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SettleMode {
    /// Standard continuous simulation driven by a fixed-rate timer tick.
    Continuous,
    /// Aggressive convergence: override damping, iterate as fast as the GPU can
    /// compute until the system reaches the energy threshold (or hits the iteration
    /// cap), then broadcast final positions and pause.
    FastSettle {
        /// Override damping to this value during the settle phase (e.g. 0.95).
        damping_override: f32,
        /// Maximum iterations before giving up on convergence.
        max_settle_iterations: u32,
        /// Total kinetic energy below which the system is considered settled.
        energy_threshold: f64,
    },
}

impl Default for SettleMode {
    fn default() -> Self {
        SettleMode::FastSettle {
            damping_override: 0.85,
            max_settle_iterations: 5000,
            energy_threshold: 0.001,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SimulationMode {
    Remote, 
    Local,  
}

impl Default for SimulationMode {
    fn default() -> Self {
        SimulationMode::Remote
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SimulationPhase {
    Initial,  
    Dynamic,  
    Finalize, 
}

impl Default for SimulationPhase {
    fn default() -> Self {
        SimulationPhase::Initial
    }
}

// GPU-compatible simulation parameters, matching the new CUDA kernel design.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SimParams {
    
    pub dt: f32,
    pub damping: f32,
    pub warmup_iterations: u32,
    pub cooling_rate: f32,

    
    pub spring_k: f32,
    pub rest_length: f32,

    
    pub repel_k: f32,
    pub repulsion_cutoff: f32,
    pub repulsion_softening_epsilon: f32,

    
    pub center_gravity_k: f32,
    pub max_force: f32,
    pub max_velocity: f32,

    
    pub grid_cell_size: f32,

    
    pub feature_flags: u32,
    pub seed: u32,
    pub iteration: i32,

    
    pub separation_radius: f32,
    pub cluster_strength: f32,
    pub alignment_strength: f32,
    pub temperature: f32,
    pub viewport_bounds: f32,
    pub sssp_alpha: f32,       
    pub boundary_damping: f32, 

    
    pub constraint_ramp_frames: u32, 
    pub constraint_max_force_per_node: f32, 

    
    pub stability_threshold: f32, 
    pub min_velocity_threshold: f32, 

    
    pub world_bounds_min: f32,         
    pub world_bounds_max: f32,         
    pub cell_size_lod: f32,            
    pub k_neighbors_max: u32,          
    pub anomaly_detection_radius: f32, 
    pub learning_rate_default: f32,    

    
    pub norm_delta_cap: f32,
    pub position_constraint_attraction: f32,
    pub lof_score_min: f32,
    pub lof_score_max: f32,
    pub weight_precision_multiplier: f32,
    // NOTE: Stress majorization params removed from GPU SimParams (not used by GPU kernels).
    // Stress optimization is handled by SemanticProcessorActor on CPU.

    /// Gravity pull toward origin (center-pull force). Added at end to preserve repr(C) layout.
    pub gravity: f32,

    // ForceAtlas2 / LinLog parameters
    /// 1 = LinLog attraction (log(1+d) per edge, modularity-equivalent), 0 = linear Hooke springs
    pub lin_log_mode: u32,
    /// FA2 repulsion scaling factor: repulsion ∝ scaling_ratio * (deg_i+1) * (deg_j+1) / d
    pub scaling_ratio: f32,
    /// 1 = per-node adaptive speed (FA2 convergence), 0 = global dt
    pub adaptive_speed: u32,
    /// Base speed for adaptive integration (scales per-node swing/traction)
    pub global_speed: f32,

    /// Z-axis suppression: 0.0 = full 3D, 1.0 = fully planar (2D on XY plane).
    /// Dampens Z velocity and gently pulls Z positions toward zero.
    pub z_damping: f32,
}

// SAFETY: SimParams is repr(C) with only POD types, safe for GPU transfer
// All fields are primitives (f32, u32, i32) with well-defined memory layout
unsafe impl DeviceRepr for SimParams {}

// SAFETY: SimParams is repr(C) with only POD types, safe for GPU transfer
// All fields are primitives (f32, u32, i32) with well-defined memory layout
unsafe impl DeviceCopy for SimParams {}

pub struct FeatureFlags;
impl FeatureFlags {
    pub const ENABLE_REPULSION: u32 = 1 << 0;
    pub const ENABLE_SPRINGS: u32 = 1 << 1;
    pub const ENABLE_CENTERING: u32 = 1 << 2;
    pub const ENABLE_TEMPORAL_COHERENCE: u32 = 1 << 3;
    pub const ENABLE_CONSTRAINTS: u32 = 1 << 4; 
    pub const ENABLE_STRESS_MAJORIZATION: u32 = 1 << 5;
    pub const ENABLE_SSSP_SPRING_ADJUST: u32 = 1 << 6; 
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SimulationParams {
    
    pub enabled: bool, 

    
    pub auto_balance: bool,                     
    pub auto_balance_interval_ms: u32,          
    pub auto_balance_config: AutoBalanceConfig, 

    
    pub auto_pause_config: AutoPauseConfig, 
    pub is_physics_paused: bool,            
    pub equilibrium_stability_counter: u32, 

    
    pub iterations: u32, 
    pub dt: f32,         

    
    pub spring_k: f32, 
    pub repel_k: f32,  

    
    pub damping: f32,          
    pub boundary_damping: f32, 

    
    pub viewport_bounds: f32, 
    pub enable_bounds: bool,  

    
    pub max_velocity: f32,      
    pub max_force: f32,         
    pub separation_radius: f32, 
    pub temperature: f32,       
    pub center_gravity_k: f32,  

    
    pub alignment_strength: f32,
    pub cluster_strength: f32,
    pub compute_mode: i32,
    pub min_distance: f32,
    pub max_repulsion_dist: f32,
    pub boundary_margin: f32,
    pub boundary_force_strength: f32,
    pub warmup_iterations: u32,
    pub cooling_rate: f32,

    /// Softening epsilon for repulsion to avoid singularities at zero distance
    pub repulsion_softening_epsilon: f32,
    /// Grid cell size for spatial hashing (defaults to 40.0)
    pub grid_cell_size: f32,
    /// Gravity pull toward center (defaults to 0.0001)
    pub gravity: f32,

    
    pub rest_length: f32,
    pub use_sssp_distances: bool,
    pub sssp_alpha: Option<f32>,

    
    pub constraint_ramp_frames: u32, 
    pub constraint_max_force_per_node: f32, 

    
    pub phase: SimulationPhase,
    pub mode: SimulationMode,

    /// Controls simulation convergence behavior.
    /// `FastSettle` (default) runs tight iterations until energy drops below
    /// threshold, then pauses. `Continuous` keeps the old timer-driven tick.
    #[serde(default)]
    pub settle_mode: SettleMode,

    /// X-axis separation between knowledge and ontology graph populations.
    /// 0 = merged (default), positive = knowledge at -X, ontology at +X, agents at origin.
    #[serde(default)]
    pub graph_separation_x: f32,

    /// Active layout algorithm (ADR-031). Defaults to ForceDirected.
    #[serde(default)]
    pub layout_mode: LayoutMode,

    /// ForceAtlas2 LinLog mode: attraction uses log(1+d) instead of Hooke's law.
    /// Makes energy minimization equivalent to modularity maximization.
    /// Defaults to true.
    #[serde(default = "default_lin_log_mode")]
    pub lin_log_mode: bool,

    /// FA2 repulsion scaling ratio. Repulsion ∝ scaling_ratio × (deg_i+1) × (deg_j+1) / d.
    /// Defaults to 10.0.
    #[serde(default = "default_scaling_ratio")]
    pub scaling_ratio: f32,

    /// FA2 per-node adaptive speed for convergence. Slows oscillating nodes.
    /// Defaults to true.
    #[serde(default = "default_adaptive_speed")]
    pub adaptive_speed: bool,

    /// Z-axis suppression: 0.0 = full 3D, 1.0 = fully planar (2D on XY plane).
    /// Dampens Z velocity and gently pulls Z positions toward zero.
    #[serde(default)]
    pub z_damping: f32,

    /// Semantic force strength for physicality clustering (OWL physicality codes).
    /// Nodes sharing the same physicality code attract; disparate codes repel.
    /// Sourced from `PhysicsSettings::physicality_strength`. Default 0.40.
    #[serde(default)]
    pub physicality_strength: f32,

    /// Semantic force strength for role clustering (OWL role codes).
    /// Concept/Object/Process etc. nodes cluster by role. Default 0.30.
    #[serde(default)]
    pub role_strength: f32,

    /// Semantic force strength for maturity clustering.
    /// Emerging/Mature/Declining nodes experience mild clustering pressure. Default 0.15.
    #[serde(default)]
    pub maturity_strength: f32,
}

impl Default for SimulationParams {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationParams {
    /// Validate all physics parameters are within safe ranges for GPU simulation.
    /// Returns Ok(()) if valid, or Err with a semicolon-separated list of violations.
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        // Time step: must be positive and small enough to avoid numerical explosion
        if self.dt <= 0.0 || self.dt > 0.1 {
            errors.push(format!("dt must be in (0, 0.1], got {}", self.dt));
        }
        // Damping: must be positive (energy drain) and at most 1.0 (full drain per step)
        if self.damping <= 0.0 || self.damping > 1.0 {
            errors.push(format!("damping must be in (0, 1], got {}", self.damping));
        }
        // Repulsion strength: negative would cause attraction collapse
        if self.repel_k < 0.0 {
            errors.push(format!("repel_k must be >= 0, got {}", self.repel_k));
        }
        // Spring strength: negative would invert spring forces
        if self.spring_k < 0.0 {
            errors.push(format!("spring_k must be >= 0, got {}", self.spring_k));
        }
        // Max velocity: must be positive to clamp motion
        if self.max_velocity <= 0.0 {
            errors.push(format!("max_velocity must be > 0, got {}", self.max_velocity));
        }
        // Max force: must be positive to clamp forces
        if self.max_force <= 0.0 {
            errors.push(format!("max_force must be > 0, got {}", self.max_force));
        }
        // Cooling rate: must be in [0, 1] (fraction of temperature retained per step)
        if self.cooling_rate < 0.0 || self.cooling_rate > 1.0 {
            errors.push(format!("cooling_rate must be in [0, 1], got {}", self.cooling_rate));
        }
        // Boundary damping: must be in [0, 1]
        if self.boundary_damping < 0.0 || self.boundary_damping > 1.0 {
            errors.push(format!("boundary_damping must be in [0, 1], got {}", self.boundary_damping));
        }
        // Temperature: must be non-negative
        if self.temperature < 0.0 {
            errors.push(format!("temperature must be >= 0, got {}", self.temperature));
        }
        // Center gravity: must be non-negative
        if self.center_gravity_k < 0.0 {
            errors.push(format!("center_gravity_k must be >= 0, got {}", self.center_gravity_k));
        }
        // Rest length: must be positive for spring equilibrium
        if self.rest_length <= 0.0 {
            errors.push(format!("rest_length must be > 0, got {}", self.rest_length));
        }
        // Separation radius: must be non-negative
        if self.separation_radius < 0.0 {
            errors.push(format!("separation_radius must be >= 0, got {}", self.separation_radius));
        }
        // Gravity: must be non-negative
        if self.gravity < 0.0 {
            errors.push(format!("gravity must be >= 0, got {}", self.gravity));
        }

        // Check all f32 fields are finite (not NaN or Inf)
        let float_fields: &[(&str, f32)] = &[
            ("dt", self.dt),
            ("damping", self.damping),
            ("spring_k", self.spring_k),
            ("repel_k", self.repel_k),
            ("max_velocity", self.max_velocity),
            ("max_force", self.max_force),
            ("temperature", self.temperature),
            ("center_gravity_k", self.center_gravity_k),
            ("cooling_rate", self.cooling_rate),
            ("boundary_damping", self.boundary_damping),
            ("viewport_bounds", self.viewport_bounds),
            ("separation_radius", self.separation_radius),
            ("cluster_strength", self.cluster_strength),
            ("alignment_strength", self.alignment_strength),
            ("rest_length", self.rest_length),
            ("gravity", self.gravity),
            ("repulsion_softening_epsilon", self.repulsion_softening_epsilon),
            ("grid_cell_size", self.grid_cell_size),
            ("min_distance", self.min_distance),
            ("max_repulsion_dist", self.max_repulsion_dist),
            ("boundary_margin", self.boundary_margin),
            ("boundary_force_strength", self.boundary_force_strength),
            ("constraint_max_force_per_node", self.constraint_max_force_per_node),
        ];
        for &(name, value) in float_fields {
            if !value.is_finite() {
                errors.push(format!("{} must be finite, got {}", name, value));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn new() -> Self {

        let default_physics = PhysicsSettings::default();
        Self::from(&default_physics)
    }

    pub fn with_phase(phase: SimulationPhase) -> Self {
        let mut params = Self::new();
        params.phase = phase;

        
        
        match phase {
            SimulationPhase::Initial => {
                
                params.iterations = params.iterations.max(500);
                params.warmup_iterations = params.warmup_iterations.max(300);
            }
            SimulationPhase::Dynamic => {
                
            }
            SimulationPhase::Finalize => {
                
                params.iterations = params.iterations.max(300);
            }
        }

        params
    }

    
    pub fn to_sim_params(&self) -> SimParams {
        
        let mut feature_flags = 0;
        if self.repel_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_REPULSION;
        }
        if self.spring_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_SPRINGS;
        }
        
        if self.center_gravity_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_CENTERING;
        }
        
        if self.use_sssp_distances {
            feature_flags |= FeatureFlags::ENABLE_SSSP_SPRING_ADJUST;
        }
        

        SimParams {
            dt: self.dt,
            damping: self.damping,
            warmup_iterations: self.warmup_iterations,
            cooling_rate: self.cooling_rate,
            spring_k: self.spring_k,
            rest_length: self.rest_length,
            repel_k: self.repel_k,
            repulsion_cutoff: self.max_repulsion_dist,
            repulsion_softening_epsilon: self.repulsion_softening_epsilon,
            center_gravity_k: self.center_gravity_k,
            max_force: self.max_force,
            max_velocity: self.max_velocity,
            grid_cell_size: self.grid_cell_size,
            feature_flags,
            seed: 1337,
            iteration: 0, 
            separation_radius: self.separation_radius,
            cluster_strength: self.cluster_strength,
            alignment_strength: self.alignment_strength,
            temperature: self.temperature,
            viewport_bounds: self.viewport_bounds,
            sssp_alpha: self.sssp_alpha.unwrap_or(0.0),
            boundary_damping: self.boundary_damping,
            constraint_ramp_frames: self.constraint_ramp_frames,
            constraint_max_force_per_node: self.constraint_max_force_per_node,
            
            stability_threshold: crate::config::dev_config::physics().stability_threshold,
            min_velocity_threshold: crate::config::dev_config::physics().min_velocity_threshold,

            
            world_bounds_min: crate::config::dev_config::physics().world_bounds_min,
            world_bounds_max: crate::config::dev_config::physics().world_bounds_max,
            cell_size_lod: crate::config::dev_config::physics().cell_size_lod,
            k_neighbors_max: crate::config::dev_config::physics().k_neighbors_max,
            anomaly_detection_radius: crate::config::dev_config::physics().anomaly_detection_radius,
            learning_rate_default: crate::config::dev_config::physics().learning_rate_default,

            
            norm_delta_cap: crate::config::dev_config::physics().norm_delta_cap,
            position_constraint_attraction: crate::config::dev_config::physics()
                .position_constraint_attraction,
            lof_score_min: crate::config::dev_config::physics().lof_score_min,
            lof_score_max: crate::config::dev_config::physics().lof_score_max,
            weight_precision_multiplier: crate::config::dev_config::physics()
                .weight_precision_multiplier,
            gravity: self.gravity,
            lin_log_mode: if self.lin_log_mode { 1 } else { 0 },
            scaling_ratio: self.scaling_ratio,
            adaptive_speed: if self.adaptive_speed { 1 } else { 0 },
            global_speed: self.dt * 10.0, // sensible default: dt-relative base speed
            z_damping: self.z_damping,
        }
    }
}

// Implementation for SimParams (GPU-aligned struct)
impl Default for SimParams {
    fn default() -> Self {
        Self::new()
    }
}

impl SimParams {
    pub fn new() -> Self {
        
        let params = SimulationParams::new();
        params.to_sim_params()
    }

    
    pub fn set_iteration(&mut self, iteration: i32) {
        self.iteration = iteration;
    }

    
    pub fn to_simulation_params(&self) -> SimulationParams {
        SimulationParams {
            enabled: true,
            auto_balance: false,
            auto_balance_interval_ms: 100,
            auto_balance_config: AutoBalanceConfig::default(),
            auto_pause_config: AutoPauseConfig::default(),
            equilibrium_stability_counter: 0,
            is_physics_paused: false,
            iterations: 100,
            dt: self.dt,
            repel_k: self.repel_k,
            damping: self.damping,
            boundary_damping: 0.9,
            viewport_bounds: self.viewport_bounds,
            enable_bounds: true,
            max_velocity: self.max_velocity,
            max_force: self.max_force,
            spring_k: 0.0, 
            separation_radius: self.separation_radius,
            center_gravity_k: self.center_gravity_k, 
            temperature: self.temperature,
            alignment_strength: self.alignment_strength,
            cluster_strength: self.cluster_strength,
            compute_mode: 0,
            min_distance: 1.0,
            max_repulsion_dist: self.repulsion_cutoff,
            boundary_margin: 50.0,
            boundary_force_strength: 1.0,
            warmup_iterations: self.warmup_iterations,
            cooling_rate: self.cooling_rate,
            rest_length: self.rest_length,
            use_sssp_distances: true,
            sssp_alpha: Some(self.sssp_alpha),
            constraint_ramp_frames: self.constraint_ramp_frames,
            constraint_max_force_per_node: self.constraint_max_force_per_node,
            repulsion_softening_epsilon: self.repulsion_softening_epsilon,
            grid_cell_size: self.grid_cell_size,
            gravity: 0.0001, // SimParams doesn't carry gravity; use default
            phase: SimulationPhase::Dynamic,
            mode: SimulationMode::Remote,
            settle_mode: SettleMode::default(),
            graph_separation_x: 0.0,
            layout_mode: LayoutMode::default(),
            lin_log_mode: self.lin_log_mode != 0,
            scaling_ratio: self.scaling_ratio,
            adaptive_speed: self.adaptive_speed != 0,
            z_damping: 0.0,
            physicality_strength: 0.40,
            role_strength: 0.30,
            maturity_strength: 0.15,
        }
    }
}

// Conversion from SimulationParams to SimParams
impl From<&SimulationParams> for SimParams {
    fn from(params: &SimulationParams) -> Self {
        params.to_sim_params()
    }
}

// Compile-time size assertion: SimParams must match the CUDA struct exactly.
const _: () = assert!(std::mem::size_of::<SimParams>() == 176);

// Conversion from SimParams to SimulationParams
impl From<&SimParams> for SimulationParams {
    fn from(params: &SimParams) -> Self {
        params.to_simulation_params()
    }
}

// Direct conversion from PhysicsSettings to SimParams for the new CUDA kernel
impl From<&PhysicsSettings> for SimParams {
    fn from(physics: &PhysicsSettings) -> Self {
        let mut feature_flags = 0;
        if physics.repel_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_REPULSION;
        }
        if physics.spring_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_SPRINGS;
        }
        if physics.center_gravity_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_CENTERING;
        }
        // Enable SSSP spring adjustment for ontology-aware edge rest lengths
        feature_flags |= FeatureFlags::ENABLE_SSSP_SPRING_ADJUST;

        SimParams {
            dt: physics.dt,
            damping: physics.damping,
            warmup_iterations: physics.warmup_iterations,
            cooling_rate: physics.cooling_rate,
            spring_k: physics.spring_k,
            rest_length: physics.rest_length,
            repel_k: physics.repel_k,
            repulsion_cutoff: physics.max_repulsion_dist,
            repulsion_softening_epsilon: physics.repulsion_softening_epsilon,
            center_gravity_k: physics.center_gravity_k,
            max_force: physics.max_force,
            max_velocity: physics.max_velocity,
            grid_cell_size: physics.grid_cell_size,
            feature_flags,
            seed: 1337,
            iteration: 0,
            separation_radius: physics.separation_radius,
            cluster_strength: physics.cluster_strength,
            alignment_strength: physics.alignment_strength,
            temperature: physics.temperature,
            viewport_bounds: physics.bounds_size,
            sssp_alpha: 1.5,  // Enable SSSP-adaptive rest lengths for ontology edges
            boundary_damping: physics.boundary_damping,
            constraint_ramp_frames: physics.constraint_ramp_frames,
            constraint_max_force_per_node: physics.constraint_max_force_per_node,

            stability_threshold: crate::config::dev_config::physics().stability_threshold,
            min_velocity_threshold: crate::config::dev_config::physics().min_velocity_threshold,


            world_bounds_min: crate::config::dev_config::physics().world_bounds_min,
            world_bounds_max: crate::config::dev_config::physics().world_bounds_max,
            cell_size_lod: crate::config::dev_config::physics().cell_size_lod,
            k_neighbors_max: crate::config::dev_config::physics().k_neighbors_max,
            anomaly_detection_radius: crate::config::dev_config::physics().anomaly_detection_radius,
            learning_rate_default: crate::config::dev_config::physics().learning_rate_default,


            norm_delta_cap: crate::config::dev_config::physics().norm_delta_cap,
            position_constraint_attraction: crate::config::dev_config::physics()
                .position_constraint_attraction,
            lof_score_min: crate::config::dev_config::physics().lof_score_min,
            lof_score_max: crate::config::dev_config::physics().lof_score_max,
            weight_precision_multiplier: crate::config::dev_config::physics()
                .weight_precision_multiplier,
            gravity: physics.gravity,
            lin_log_mode: if physics.lin_log_mode { 1 } else { 0 },
            scaling_ratio: physics.scaling_ratio,
            adaptive_speed: if physics.adaptive_speed { 1 } else { 0 },
            global_speed: physics.dt * 10.0,
            z_damping: physics.z_damping,
        }
    }
}

// Conversion from PhysicsSettings to SimulationParams
impl From<&PhysicsSettings> for SimulationParams {
    fn from(physics: &PhysicsSettings) -> Self {
        Self {
            enabled: physics.enabled,
            auto_balance: physics.auto_balance,
            auto_balance_interval_ms: physics.auto_balance_interval_ms,
            auto_balance_config: physics.auto_balance_config.clone(),
            auto_pause_config: physics.auto_pause.clone(),
            is_physics_paused: false, 
            equilibrium_stability_counter: 0,
            iterations: physics.iterations,
            dt: physics.dt,
            spring_k: physics.spring_k,
            repel_k: physics.repel_k,
            damping: physics.damping,
            boundary_damping: physics.boundary_damping,
            viewport_bounds: physics.bounds_size,
            enable_bounds: physics.enable_bounds,
            max_velocity: physics.max_velocity,
            max_force: physics.max_force, 
            separation_radius: physics.separation_radius,
            temperature: physics.temperature,
            center_gravity_k: physics.center_gravity_k,
            
            alignment_strength: physics.alignment_strength,
            cluster_strength: physics.cluster_strength,
            compute_mode: physics.compute_mode,
            min_distance: physics.min_distance,
            max_repulsion_dist: physics.max_repulsion_dist,
            boundary_margin: physics.boundary_margin,
            boundary_force_strength: physics.boundary_force_strength,
            warmup_iterations: physics.warmup_iterations,
            cooling_rate: physics.cooling_rate,
            rest_length: physics.rest_length,
            use_sssp_distances: true,
            sssp_alpha: Some(1.5),     // SSSP-adaptive rest lengths
            constraint_ramp_frames: physics.constraint_ramp_frames,
            constraint_max_force_per_node: physics.constraint_max_force_per_node,
            repulsion_softening_epsilon: physics.repulsion_softening_epsilon,
            grid_cell_size: physics.grid_cell_size,
            gravity: physics.gravity,
            phase: SimulationPhase::Dynamic,
            mode: SimulationMode::Remote,
            settle_mode: SettleMode::default(),
            graph_separation_x: physics.graph_separation_x,
            layout_mode: LayoutMode::default(),
            lin_log_mode: physics.lin_log_mode,
            scaling_ratio: physics.scaling_ratio,
            adaptive_speed: physics.adaptive_speed,
            z_damping: physics.z_damping,
            physicality_strength: physics.physicality_strength,
            role_strength: physics.role_strength,
            maturity_strength: physics.maturity_strength,
        }
    }
}
