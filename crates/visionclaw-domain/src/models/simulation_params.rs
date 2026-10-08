//! Simulation parameters — domain representation.
//!
//! This file contains the framework-agnostic parts of the physics simulation
//! parameter model: high-level `SimulationParams`, the convergence `SettleMode`,
//! and the auxiliary enums. The GPU-aligned `SimParams` struct (and any
//! conversion that depends on `crate::config::dev_config`) lives in the
//! monolith's `src/models/simulation_params.rs` since it requires CUDA/PTX
//! types and runtime config that have no place in pure domain logic.

use serde::{Deserialize, Serialize};

use crate::types::layout::LayoutMode;
use crate::types::physics_config::{AutoBalanceConfig, AutoPauseConfig, PhysicsSettings};

fn default_lin_log_mode() -> bool {
    true
}
fn default_scaling_ratio() -> f32 {
    10.0
}
fn default_adaptive_speed() -> bool {
    true
}
fn default_global_speed() -> f32 {
    0.16
}
fn default_spring_pop_scale() -> f32 {
    1.0
}
fn default_dag_bias_k() -> f32 {
    0.0
}
fn default_dag_level_distance() -> f32 {
    60.0
}
fn default_plane_bias_k() -> f32 {
    0.0
}
fn default_plane_spacing() -> f32 {
    60.0
}
fn default_layer_bias_k() -> f32 {
    0.0
}
fn default_layer_spacing() -> f32 {
    60.0
}
fn default_radial_center() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

/// Controls how the physics simulation converges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SettleMode {
    /// Standard continuous simulation driven by a fixed-rate timer tick.
    // Continuous is the default so the GPU physics loop keeps stepping and
    // broadcasting indefinitely (required for the live XR client: the headset
    // must keep animating and stay grab-responsive). FastSettle's bounded burst
    // latched the loop off on convergence, freezing the graph. FastSettle remains
    // available as an explicit runtime mode for one-shot layout passes.
    #[default]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SimulationMode {
    #[default]
    Remote,
    Local,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SimulationPhase {
    #[default]
    Initial,
    Dynamic,
    Finalize,
}

/// Feature-flag bit constants for `SimParams.feature_flags`. The monolith's
/// GPU adapter uses these when building the GPU-ready `SimParams` struct.
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

/// High-level physics simulation parameters. The actor system and HTTP API
/// work with this representation; the GPU adapter converts it to the
/// CUDA-aligned `SimParams` struct (defined in the monolith).
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
    pub warmup_iterations: u32,
    pub cooling_rate: f32,

    /// Softening epsilon for repulsion to avoid singularities at zero distance.
    pub repulsion_softening_epsilon: f32,
    /// Grid cell size for spatial hashing (defaults to 40.0).
    pub grid_cell_size: f32,
    /// Gravity pull toward center (defaults to 0.0001).
    pub gravity: f32,

    pub rest_length: f32,
    pub use_sssp_distances: bool,
    pub sssp_alpha: Option<f32>,

    pub constraint_ramp_frames: u32,
    pub constraint_max_force_per_node: f32,

    pub phase: SimulationPhase,
    pub mode: SimulationMode,

    /// Controls simulation convergence behavior.
    #[serde(default)]
    pub settle_mode: SettleMode,

    /// Continuous Z-scale factor (1.0 = no compression → fully 3D; down to 0.05).
    /// Applied in the force sim as the Z multiplier; also drives the dual-disc
    /// face thinness when `enable_dual_disc_layout` is on.
    #[serde(default)]
    pub axis_compression_z: f32,

    /// Opt-in canonical dual-disc display layout (default OFF → fully 3D). Gates
    /// only the disc re-centre projection; `axis_compression_z` is the
    /// continuous Z compression applied in both modes.
    #[serde(default)]
    pub enable_dual_disc_layout: bool,

    /// Active layout algorithm (ADR-031). Defaults to ForceDirected.
    #[serde(default)]
    pub layout_mode: LayoutMode,

    /// ForceAtlas2 LinLog mode: attraction uses log(1+d) instead of Hooke's law.
    #[serde(default = "default_lin_log_mode")]
    pub lin_log_mode: bool,

    /// FA2 repulsion scaling ratio.
    #[serde(default = "default_scaling_ratio")]
    pub scaling_ratio: f32,

    /// FA2 per-node adaptive speed for convergence.
    #[serde(default = "default_adaptive_speed")]
    pub adaptive_speed: bool,

    /// FA2 base integration speed.
    #[serde(default = "default_global_speed")]
    pub global_speed: f32,

    /// Per-population spring strength multipliers. Each is the literal coefficient
    /// applied to that population's attraction force in BOTH the LinLog and Hooke
    /// kernel paths (1.0 == current LinLog identity). This is what makes the spring
    /// sliders independently steer Knowledge / Ontology / Agent layouts; the global
    /// `spring_k` remains the Hooke-mode stiffness.
    #[serde(default = "default_spring_pop_scale")]
    pub spring_k_knowledge: f32,
    #[serde(default = "default_spring_pop_scale")]
    pub spring_k_ontology: f32,
    #[serde(default = "default_spring_pop_scale")]
    pub spring_k_agent: f32,

    /// DAG radial hierarchy bias strength (PHASE 2). `0` = off (default). Pulls
    /// each ranked node toward a shell of radius `rank * dag_level_distance`
    /// around the hierarchy root centroid (radialout layout).
    #[serde(default = "default_dag_bias_k")]
    pub dag_bias_k: f32,

    /// World-space radius per hierarchy level for the DAG radial bias (default 60).
    #[serde(default = "default_dag_level_distance")]
    pub dag_level_distance: f32,

    /// Stratified-plane bias strength (ADR-141 P2). `0` = off (default). Springs
    /// each node along Z toward its assigned plane (target_z = plane_offset *
    /// plane_spacing), separating node types into parallel strata.
    #[serde(default = "default_plane_bias_k")]
    pub plane_bias_k: f32,

    /// World-space Z distance per plane offset unit for the stratified-plane bias
    /// (default 60).
    #[serde(default = "default_plane_spacing")]
    pub plane_spacing: f32,

    /// Sugiyama Y-by-rank layer spring strength (ADR-141 P4). `0` = off (default).
    /// Springs each ranked node's Y toward `rank * layer_spacing`, giving a
    /// top-down layered (Sugiyama) layout over the DAG rank.
    #[serde(default = "default_layer_bias_k")]
    pub layer_bias_k: f32,

    /// World-space Y distance per rank for the Sugiyama layer spring (default 60).
    #[serde(default = "default_layer_spacing")]
    pub layer_spacing: f32,

    /// Radial shell centre (ADR-141 P3) for the `dag_radial_bias` term. Default
    /// `[0,0,0]` = origin = legacy DAG behaviour. This is NOT a PhysicsSettings /
    /// wire field: it is actor-authoritative, owned by the `SetRadialLayout`
    /// message (RadialMode::Ego centres it on the focus node's live position), and
    /// a settings PUT must never reset it.
    #[serde(default = "default_radial_center")]
    pub radial_center: [f32; 3],
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

        if self.dt <= 0.0 || self.dt > 0.1 {
            errors.push(format!("dt must be in (0, 0.1], got {}", self.dt));
        }
        if self.damping <= 0.0 || self.damping > 1.0 {
            errors.push(format!("damping must be in (0, 1], got {}", self.damping));
        }
        if self.repel_k < 0.0 {
            errors.push(format!("repel_k must be >= 0, got {}", self.repel_k));
        }
        if self.spring_k < 0.0 {
            errors.push(format!("spring_k must be >= 0, got {}", self.spring_k));
        }
        if self.max_velocity <= 0.0 {
            errors.push(format!(
                "max_velocity must be > 0, got {}",
                self.max_velocity
            ));
        }
        if self.max_force <= 0.0 {
            errors.push(format!("max_force must be > 0, got {}", self.max_force));
        }
        if self.cooling_rate < 0.0 || self.cooling_rate > 1.0 {
            errors.push(format!(
                "cooling_rate must be in [0, 1], got {}",
                self.cooling_rate
            ));
        }
        if self.boundary_damping < 0.0 || self.boundary_damping > 1.0 {
            errors.push(format!(
                "boundary_damping must be in [0, 1], got {}",
                self.boundary_damping
            ));
        }
        if self.temperature < 0.0 {
            errors.push(format!(
                "temperature must be >= 0, got {}",
                self.temperature
            ));
        }
        if self.center_gravity_k < 0.0 {
            errors.push(format!(
                "center_gravity_k must be >= 0, got {}",
                self.center_gravity_k
            ));
        }
        if self.rest_length <= 0.0 {
            errors.push(format!("rest_length must be > 0, got {}", self.rest_length));
        }
        if self.separation_radius < 0.0 {
            errors.push(format!(
                "separation_radius must be >= 0, got {}",
                self.separation_radius
            ));
        }
        if self.gravity < 0.0 {
            errors.push(format!("gravity must be >= 0, got {}", self.gravity));
        }
        if self.max_repulsion_dist < 10.0 || self.max_repulsion_dist > 5000.0 {
            errors.push(format!(
                "max_repulsion_dist must be in [10, 5000], got {}",
                self.max_repulsion_dist
            ));
        }
        // cluster_strength is the raw kernel coefficient (no scale factor).
        if self.cluster_strength < 0.0 || self.cluster_strength > 0.02 {
            errors.push(format!(
                "cluster_strength must be in [0, 0.02], got {}",
                self.cluster_strength
            ));
        }
        match self.sssp_alpha {
            Some(a) if !a.is_finite() => {
                errors.push(format!("sssp_alpha must be finite, got {}", a));
            }
            Some(a) if a < 0.0 || a > 5.0 => {
                errors.push(format!("sssp_alpha must be in [0, 5], got {}", a));
            }
            _ => {}
        }

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
            (
                "repulsion_softening_epsilon",
                self.repulsion_softening_epsilon,
            ),
            ("grid_cell_size", self.grid_cell_size),
            ("min_distance", self.min_distance),
            ("max_repulsion_dist", self.max_repulsion_dist),
            (
                "constraint_max_force_per_node",
                self.constraint_max_force_per_node,
            ),
            ("spring_k_knowledge", self.spring_k_knowledge),
            ("spring_k_ontology", self.spring_k_ontology),
            ("spring_k_agent", self.spring_k_agent),
            ("dag_bias_k", self.dag_bias_k),
            ("dag_level_distance", self.dag_level_distance),
            ("layer_bias_k", self.layer_bias_k),
            ("layer_spacing", self.layer_spacing),
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
            SimulationPhase::Dynamic => {}
            SimulationPhase::Finalize => {
                params.iterations = params.iterations.max(300);
            }
        }

        params
    }
}

// Conversion from PhysicsSettings to SimulationParams — no dev_config refs,
// safe to live in the domain crate.
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
            // alignment_strength (ADR-141 P4b) is a live scalar again — the kernel
            // ALIGNMENT constraint branch scales its per-axis pull by it.
            alignment_strength: physics.alignment_strength,
            cluster_strength: physics.cluster_strength,
            // compute_mode is no longer a user-facing setting; the live physics
            // step always runs the unified kernel (ComputeMode::Basic). Kept as
            // an internal field for the actor layout-override paths.
            compute_mode: 0,
            // min_distance never reached the GPU; retained internally as the
            // collision floor used by CPU-side helpers, defaulted to 1.0.
            min_distance: 1.0,
            max_repulsion_dist: physics.max_repulsion_dist,
            warmup_iterations: physics.warmup_iterations,
            cooling_rate: physics.cooling_rate,
            rest_length: physics.rest_length,
            use_sssp_distances: true,
            sssp_alpha: Some(physics.sssp_alpha),
            constraint_ramp_frames: physics.constraint_ramp_frames,
            constraint_max_force_per_node: physics.constraint_max_force_per_node,
            repulsion_softening_epsilon: physics.repulsion_softening_epsilon,
            grid_cell_size: physics.grid_cell_size,
            gravity: physics.gravity,
            phase: SimulationPhase::Dynamic,
            mode: SimulationMode::Remote,
            settle_mode: SettleMode::default(),
            axis_compression_z: physics.axis_compression_z,
            enable_dual_disc_layout: physics.enable_dual_disc_layout,
            layout_mode: LayoutMode::default(),
            lin_log_mode: physics.lin_log_mode,
            scaling_ratio: physics.scaling_ratio,
            adaptive_speed: physics.adaptive_speed,
            global_speed: physics.global_speed,
            spring_k_knowledge: physics.spring_k_knowledge,
            spring_k_ontology: physics.spring_k_ontology,
            spring_k_agent: physics.spring_k_agent,
            dag_bias_k: physics.dag_bias_k,
            dag_level_distance: physics.dag_level_distance,
            plane_bias_k: physics.plane_bias_k,
            plane_spacing: physics.plane_spacing,
            layer_bias_k: physics.layer_bias_k,
            layer_spacing: physics.layer_spacing,
            // Actor-authoritative (SetRadialLayout); PhysicsSettings has no source.
            radial_center: default_radial_center(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let p = SimulationParams::default();
        assert!(p.enabled);
        assert_eq!(p.phase, SimulationPhase::Dynamic);
    }

    #[test]
    fn test_validate_ok() {
        assert!(SimulationParams::default().validate().is_ok());
    }

    #[test]
    fn test_validate_bad_dt() {
        let p = SimulationParams {
            dt: -1.0,
            ..Default::default()
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn test_settle_mode_default() {
        // Default is Continuous so the physics loop never latches off on
        // convergence — the live XR client needs perpetual stepping + broadcast.
        assert_eq!(SettleMode::default(), SettleMode::Continuous);
    }

    #[test]
    fn test_settle_mode_continuous_serde() {
        let m = SettleMode::Continuous;
        let json = serde_json::to_string(&m).unwrap();
        let back: SettleMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SettleMode::Continuous);
    }

    #[test]
    fn test_simulation_mode_and_phase_defaults() {
        assert_eq!(SimulationMode::default(), SimulationMode::Remote);
        assert_eq!(SimulationPhase::default(), SimulationPhase::Initial);
    }

    #[test]
    fn test_validate_multiple_errors_collected() {
        let p = SimulationParams {
            dt: -1.0,      // invalid
            damping: 2.0,  // invalid (> 1)
            repel_k: -1.0, // invalid
            ..Default::default()
        };
        let err = p.validate().unwrap_err();
        // All three errors should be in the semicolon-separated message
        let count = err.split(';').count();
        assert!(count >= 3, "expected >= 3 errors, got: {}", err);
    }

    #[test]
    fn test_validate_nan_field_caught() {
        let p = SimulationParams {
            dt: f32::NAN,
            ..Default::default()
        };
        let err = p.validate().unwrap_err();
        assert!(err.contains("dt"), "should mention dt: {}", err);
    }

    #[test]
    fn test_validate_infinite_field_caught() {
        let p = SimulationParams {
            max_velocity: f32::INFINITY,
            ..Default::default()
        };
        let err = p.validate().unwrap_err();
        assert!(
            err.contains("max_velocity"),
            "should mention max_velocity: {}",
            err
        );
    }

    #[test]
    fn test_with_phase_initial_sets_min_iterations() {
        let p = SimulationParams::with_phase(SimulationPhase::Initial);
        assert_eq!(p.phase, SimulationPhase::Initial);
        assert!(p.iterations >= 500);
        assert!(p.warmup_iterations >= 300);
    }

    #[test]
    fn test_with_phase_finalize_sets_min_iterations() {
        let p = SimulationParams::with_phase(SimulationPhase::Finalize);
        assert_eq!(p.phase, SimulationPhase::Finalize);
        assert!(p.iterations >= 300);
    }

    #[test]
    fn test_feature_flags_are_distinct_powers_of_two() {
        let flags = [
            FeatureFlags::ENABLE_REPULSION,
            FeatureFlags::ENABLE_SPRINGS,
            FeatureFlags::ENABLE_CENTERING,
            FeatureFlags::ENABLE_TEMPORAL_COHERENCE,
            FeatureFlags::ENABLE_CONSTRAINTS,
            FeatureFlags::ENABLE_STRESS_MAJORIZATION,
            FeatureFlags::ENABLE_SSSP_SPRING_ADJUST,
        ];
        // All flags are distinct
        for (i, &a) in flags.iter().enumerate() {
            for (j, &b) in flags.iter().enumerate() {
                if i != j {
                    assert_eq!(a & b, 0, "flags {} and {} overlap", i, j);
                }
            }
        }
    }

    #[test]
    fn test_simulation_params_serde_roundtrip() {
        let p = SimulationParams::default();
        let json = serde_json::to_string(&p).unwrap();
        let back: SimulationParams = serde_json::from_str(&json).unwrap();
        assert!((back.dt - p.dt).abs() < f32::EPSILON);
        assert_eq!(back.enabled, p.enabled);
    }

    // Regression: the graph-layout controls (Z-axis compression, dual-disc,
    // adaptive speed) must propagate verbatim from persisted
    // PhysicsSettings into the GPU SimulationParams. A previous bug hardcoded
    // these on the GPU path, so non-default user values had no visible effect.
    #[test]
    fn test_layout_controls_propagate_from_physics_settings() {
        let physics = PhysicsSettings {
            axis_compression_z: 0.5,
            enable_dual_disc_layout: true,
            adaptive_speed: false,
            ..Default::default()
        };

        let params = SimulationParams::from(&physics);

        assert!((params.axis_compression_z - 0.5).abs() < f32::EPSILON);
        assert!(params.enable_dual_disc_layout);
        assert!(!params.adaptive_speed);
    }

    // Regression: persisted physics arrives from SQLite as a complete camelCase
    // JSON object. The boot read path (app_state.rs) and the GET handler both
    // deserialize that stored value, so a full round-trip must preserve the
    // three layout controls rather than silently falling back to 0.0 / true.
    #[test]
    fn test_physics_settings_camelcase_roundtrip_preserves_layout_controls() {
        let physics = PhysicsSettings {
            axis_compression_z: 0.5,
            enable_dual_disc_layout: true,
            adaptive_speed: false,
            ..Default::default()
        };

        let stored = serde_json::to_value(&physics).unwrap();
        // The stored object uses camelCase keys (serde rename_all).
        assert!(stored.get("axisCompressionZ").is_some());

        let loaded: PhysicsSettings = serde_json::from_value(stored).unwrap();
        assert!((loaded.axis_compression_z - 0.5).abs() < f32::EPSILON);
        assert!(loaded.enable_dual_disc_layout);
        assert!(!loaded.adaptive_speed);
    }

    // Regression: sssp_alpha is now a real user-settable field, not a hardcoded
    // 1.5. It must survive PhysicsSettings -> SimulationParams conversion verbatim.
    #[test]
    fn test_sssp_alpha_propagates_and_is_not_hardcoded() {
        let physics = PhysicsSettings {
            sssp_alpha: 3.25,
            ..Default::default()
        };
        let params = SimulationParams::from(&physics);
        assert_eq!(params.sssp_alpha, Some(3.25));
        // A hardcoded 1.5 would have ignored the 3.25 source value.
        assert_ne!(params.sssp_alpha, Some(1.5));
    }

    // Regression: gravity must survive PhysicsSettings -> SimulationParams; a
    // prior hardcoded 0.0001 override in the reverse path clobbered user values.
    #[test]
    fn test_gravity_propagates_from_physics_settings() {
        let physics = PhysicsSettings {
            gravity: 0.5,
            ..Default::default()
        };
        let params = SimulationParams::from(&physics);
        assert!((params.gravity - 0.5).abs() < f32::EPSILON);
    }

    // cluster_strength is the raw kernel coefficient and the community-cohesion
    // force is opt-in, so the contract default is 0.0 (below the >0.0001 gate).
    #[test]
    fn test_cluster_strength_default_is_raw_coefficient() {
        let physics = PhysicsSettings::default();
        assert_eq!(physics.cluster_strength, 0.0);
        let params = SimulationParams::from(&physics);
        assert_eq!(params.cluster_strength, 0.0);
    }

    // alignment_strength (ADR-141 P4b) is a live scalar again, but defaults to 0.0
    // so the ALIGNMENT constraint branch is inert until the user raises it.
    #[test]
    fn test_alignment_strength_defaults_to_zero() {
        let params = SimulationParams::from(&PhysicsSettings::default());
        assert_eq!(params.alignment_strength, 0.0);
    }

    // Regression: the three layout-control fields carry snake_case serde aliases
    // so a legacy/persisted object that used snake_case keys for them still
    // deserializes (the rest of the object stays camelCase).
    #[test]
    fn test_physics_settings_snake_case_aliases_for_layout_controls() {
        let physics = PhysicsSettings::default();
        let mut stored = serde_json::to_value(&physics).unwrap();
        let obj = stored.as_object_mut().unwrap();
        obj.remove("axisCompressionZ");
        obj.remove("enableDualDiscLayout");
        obj.remove("adaptiveSpeed");
        obj.insert("axis_compression_z".into(), serde_json::json!(0.5));
        obj.insert("enable_dual_disc_layout".into(), serde_json::json!(true));
        obj.insert("adaptive_speed".into(), serde_json::json!(false));

        let loaded: PhysicsSettings = serde_json::from_value(stored).unwrap();
        assert!((loaded.axis_compression_z - 0.5).abs() < f32::EPSILON);
        assert!(loaded.enable_dual_disc_layout);
        assert!(!loaded.adaptive_speed);
    }

    // Retired setting (ADR-2135 amendment, 2026-10-08): the separation is fixed,
    // so a stored or sent `graphSeparationX` (either spelling) still loads, is
    // ignored, and is not written back.
    #[test]
    fn test_retired_graph_separation_x_loads_and_is_ignored() {
        let base = serde_json::to_value(PhysicsSettings::default()).unwrap();
        for key in ["graphSeparationX", "graph_separation_x"] {
            let mut stored = base.clone();
            stored
                .as_object_mut()
                .unwrap()
                .insert(key.into(), serde_json::json!(250.0));
            let loaded: PhysicsSettings = serde_json::from_value(stored).unwrap();
            let again = serde_json::to_value(&loaded).unwrap();
            assert_eq!(again, base, "{key} is dropped on load");
            let params = SimulationParams::from(&loaded);
            let params_json = serde_json::to_value(&params).unwrap();
            assert!(params_json.get("graphSeparationX").is_none());
            assert!(params_json.get("graph_separation_x").is_none());
        }
    }

    // Round-trip / migration: an absent axisCompressionZ defaults to 1.0 (no
    // compression), and a LEGACY blob still carrying the old 0.9 value loads
    // verbatim as a gentle 0.9 z-scale (near-3D) — under the flipped semantic
    // this does NOT resurrect the old flatten, so no migration heuristic runs.
    #[test]
    fn test_axis_compression_z_absent_defaults_and_legacy_value_loads_verbatim() {
        // Absent axisCompressionZ → serde default 1.0 (no compression). Start from
        // a full serialized default and remove just that key (PhysicsSettings has
        // many required fields, so a bare {} would not deserialize).
        let mut stored = serde_json::to_value(PhysicsSettings::default()).unwrap();
        stored.as_object_mut().unwrap().remove("axisCompressionZ");
        let loaded: PhysicsSettings = serde_json::from_value(stored).unwrap();
        assert!((loaded.axis_compression_z - 1.0).abs() < f32::EPSILON);
        assert!(!loaded.enable_dual_disc_layout);

        // Legacy stored blob carrying the OLD 0.9 default and no
        // enableDualDiscLayout: loads verbatim as a gentle 0.9 z-scale (near-3D),
        // NOT the old flatten — dual-disc stays OFF, so no migration heuristic.
        let mut legacy = serde_json::to_value(PhysicsSettings::default()).unwrap();
        {
            let o = legacy.as_object_mut().unwrap();
            o.insert("axisCompressionZ".into(), serde_json::json!(0.9));
            o.remove("enableDualDiscLayout");
        }
        let loaded: PhysicsSettings = serde_json::from_value(legacy).unwrap();
        assert!((loaded.axis_compression_z - 0.9).abs() < f32::EPSILON);
        assert!(!loaded.enable_dual_disc_layout);
    }
}
