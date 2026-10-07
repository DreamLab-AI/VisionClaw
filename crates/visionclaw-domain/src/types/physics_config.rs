//! Physics configuration types — domain representation.
//!
//! Mirrors `src/config/physics.rs` without specta/validator dependencies.
//! Canonical constants are inlined rather than imported from the monolith.

use serde::{Deserialize, Serialize};
use specta::Type;
use validator::Validate;

/// Canonical maximum velocity (from `src/config/mod.rs`).
pub const CANONICAL_MAX_VELOCITY: f32 = 200.0;

/// Canonical maximum force (from `src/config/mod.rs`).
pub const CANONICAL_MAX_FORCE: f32 = 50.0;

fn default_auto_balance_interval() -> u32 {
    500
}

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
    0.4
}

fn default_spring_pop_scale() -> f32 {
    1.0
}

fn default_sssp_alpha() -> f32 {
    1.5
}

fn default_constraint_ramp_frames() -> u32 {
    60
}

fn default_constraint_max_force_per_node() -> f32 {
    50.0
}

fn default_bounds_size() -> f32 {
    400.0
}

/// Default Z-scale factor: 1.0 = no compression (fully 3D). See the field doc
/// on `PhysicsSettings::axis_compression_z` for the semantic-flip note.
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
fn default_alignment_strength() -> f32 {
    0.0
}
fn default_axis_compression_z() -> f32 {
    1.0
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AutoPauseConfig {
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[validate(range(min = 0.0, max = 10.0))]
    #[serde(alias = "equilibrium_velocity_threshold")]
    pub equilibrium_velocity_threshold: f32,
    #[validate(range(min = 1, max = 300))]
    #[serde(alias = "equilibrium_check_frames")]
    pub equilibrium_check_frames: u32,
    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(alias = "equilibrium_energy_threshold")]
    pub equilibrium_energy_threshold: f32,
    #[serde(alias = "pause_on_equilibrium")]
    pub pause_on_equilibrium: bool,
    #[serde(alias = "resume_on_interaction")]
    pub resume_on_interaction: bool,
}

impl AutoPauseConfig {
    /// The tuned auto-pause settings the physics engine ships with. Distinct
    /// from the derived `Default` (all zero/false), which serde uses for an
    /// absent field.
    pub fn recommended() -> Self {
        Self {
            enabled: true,
            equilibrium_velocity_threshold: 0.1,
            equilibrium_check_frames: 30,
            equilibrium_energy_threshold: 0.01,
            pause_on_equilibrium: false,
            resume_on_interaction: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct AutoBalanceConfig {
    #[serde(alias = "stability_variance_threshold")]
    pub stability_variance_threshold: f32,
    #[serde(alias = "stability_frame_count")]
    pub stability_frame_count: u32,
    #[serde(alias = "clustering_distance_threshold")]
    pub clustering_distance_threshold: f32,
    #[serde(alias = "clustering_hysteresis_buffer")]
    pub clustering_hysteresis_buffer: f32,
    #[serde(alias = "bouncing_node_percentage")]
    pub bouncing_node_percentage: f32,
    #[serde(alias = "boundary_min_distance")]
    pub boundary_min_distance: f32,
    #[serde(alias = "boundary_max_distance")]
    pub boundary_max_distance: f32,
    #[serde(alias = "extreme_distance_threshold")]
    pub extreme_distance_threshold: f32,
    #[serde(alias = "explosion_distance_threshold")]
    pub explosion_distance_threshold: f32,
    #[serde(alias = "spreading_distance_threshold")]
    pub spreading_distance_threshold: f32,
    #[serde(alias = "spreading_hysteresis_buffer")]
    pub spreading_hysteresis_buffer: f32,
    #[serde(alias = "oscillation_detection_frames")]
    pub oscillation_detection_frames: usize,
    #[serde(alias = "oscillation_change_threshold")]
    pub oscillation_change_threshold: f32,
    #[serde(alias = "min_oscillation_changes")]
    pub min_oscillation_changes: usize,

    #[serde(alias = "parameter_adjustment_rate")]
    pub parameter_adjustment_rate: f32,
    #[serde(alias = "max_adjustment_factor")]
    pub max_adjustment_factor: f32,
    #[serde(alias = "min_adjustment_factor")]
    pub min_adjustment_factor: f32,
    #[serde(alias = "adjustment_cooldown_ms")]
    pub adjustment_cooldown_ms: u64,
    #[serde(alias = "state_change_cooldown_ms")]
    pub state_change_cooldown_ms: u64,
    #[serde(alias = "parameter_dampening_factor")]
    pub parameter_dampening_factor: f32,
    #[serde(alias = "hysteresis_delay_frames")]
    pub hysteresis_delay_frames: u32,

    #[serde(alias = "grid_cell_size_min")]
    pub grid_cell_size_min: f32,
    #[serde(alias = "grid_cell_size_max")]
    pub grid_cell_size_max: f32,
    #[serde(alias = "repulsion_cutoff_min")]
    pub repulsion_cutoff_min: f32,
    #[serde(alias = "repulsion_cutoff_max")]
    pub repulsion_cutoff_max: f32,
    #[serde(alias = "repulsion_softening_min")]
    pub repulsion_softening_min: f32,
    #[serde(alias = "repulsion_softening_max")]
    pub repulsion_softening_max: f32,
    #[serde(alias = "center_gravity_min")]
    pub center_gravity_min: f32,
    #[serde(alias = "center_gravity_max")]
    pub center_gravity_max: f32,

    #[serde(alias = "spatial_hash_efficiency_threshold")]
    pub spatial_hash_efficiency_threshold: f32,
    #[serde(alias = "cluster_density_threshold")]
    pub cluster_density_threshold: f32,
    #[serde(alias = "numerical_instability_threshold")]
    pub numerical_instability_threshold: f32,
}

impl AutoBalanceConfig {
    /// The tuned auto-balance settings the physics engine ships with. Distinct
    /// from the derived `Default` (all zero/false), which serde uses for an
    /// absent field.
    pub fn recommended() -> Self {
        Self {
            stability_variance_threshold: 100.0,
            stability_frame_count: 180,
            clustering_distance_threshold: 20.0,
            clustering_hysteresis_buffer: 5.0,
            bouncing_node_percentage: 0.33,
            boundary_min_distance: 90.0,
            boundary_max_distance: 110.0,
            extreme_distance_threshold: 1000.0,
            explosion_distance_threshold: 10000.0,
            spreading_distance_threshold: 500.0,
            spreading_hysteresis_buffer: 50.0,
            oscillation_detection_frames: 20,
            oscillation_change_threshold: 10.0,
            min_oscillation_changes: 8,

            parameter_adjustment_rate: 0.1,
            max_adjustment_factor: 0.2,
            min_adjustment_factor: -0.2,
            adjustment_cooldown_ms: 2000,
            state_change_cooldown_ms: 1000,
            parameter_dampening_factor: 0.05,
            hysteresis_delay_frames: 30,

            grid_cell_size_min: 1.0,
            grid_cell_size_max: 50.0,
            repulsion_cutoff_min: 5.0,
            repulsion_cutoff_max: 200.0,
            repulsion_softening_min: 1e-6,
            repulsion_softening_max: 1.0,
            center_gravity_min: 0.0,
            center_gravity_max: 0.1,

            spatial_hash_efficiency_threshold: 0.3,
            cluster_density_threshold: 50.0,
            numerical_instability_threshold: 1e-3,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsSettings {
    #[serde(default, alias = "auto_balance")]
    pub auto_balance: bool,
    #[serde(
        default = "default_auto_balance_interval",
        alias = "auto_balance_interval_ms"
    )]
    pub auto_balance_interval_ms: u32,
    #[serde(default, alias = "auto_balance_config")]
    #[validate(nested)]
    pub auto_balance_config: AutoBalanceConfig,
    #[serde(default, alias = "auto_pause")]
    #[validate(nested)]
    pub auto_pause: AutoPauseConfig,
    #[serde(default = "default_bounds_size", alias = "bounds_size")]
    pub bounds_size: f32,
    #[serde(alias = "separation_radius")]
    pub separation_radius: f32,
    #[serde(alias = "damping")]
    pub damping: f32,
    #[serde(alias = "enable_bounds")]
    pub enable_bounds: bool,
    #[serde(alias = "enabled")]
    pub enabled: bool,
    #[serde(alias = "iterations")]
    pub iterations: u32,
    #[serde(alias = "max_velocity")]
    pub max_velocity: f32,
    #[serde(alias = "max_force")]
    pub max_force: f32,
    #[serde(alias = "repel_k")]
    pub repel_k: f32,
    #[serde(alias = "spring_k")]
    pub spring_k: f32,
    #[serde(alias = "boundary_damping")]
    pub boundary_damping: f32,
    #[serde(alias = "dt")]
    pub dt: f32,
    #[serde(alias = "temperature")]
    pub temperature: f32,
    #[serde(alias = "gravity")]
    pub gravity: f32,

    #[serde(alias = "cluster_strength")]
    pub cluster_strength: f32,

    #[serde(alias = "rest_length")]
    pub rest_length: f32,
    #[serde(alias = "repulsion_softening_epsilon")]
    pub repulsion_softening_epsilon: f32,
    #[serde(alias = "center_gravity_k")]
    pub center_gravity_k: f32,
    #[serde(alias = "grid_cell_size")]
    pub grid_cell_size: f32,
    #[serde(alias = "warmup_iterations")]
    pub warmup_iterations: u32,
    #[serde(alias = "cooling_rate")]
    pub cooling_rate: f32,

    /// GPU repulsion distance cutoff (also the spatial-hash neighbour radius).
    #[serde(alias = "max_repulsion_dist")]
    pub max_repulsion_dist: f32,

    /// Strength of SSSP-derived rest-length adjustment on spring forces.
    #[serde(default = "default_sssp_alpha", alias = "sssp_alpha")]
    pub sssp_alpha: f32,

    #[serde(
        alias = "constraint_ramp_frames",
        default = "default_constraint_ramp_frames"
    )]
    pub constraint_ramp_frames: u32,
    #[serde(
        alias = "constraint_max_force_per_node",
        default = "default_constraint_max_force_per_node"
    )]
    pub constraint_max_force_per_node: f32,

    #[serde(alias = "clustering_algorithm")]
    pub clustering_algorithm: String,
    #[serde(alias = "cluster_count")]
    pub cluster_count: u32,
    #[serde(alias = "clustering_resolution")]
    pub clustering_resolution: f32,
    #[serde(alias = "clustering_iterations")]
    pub clustering_iterations: u32,

    /// X-axis separation between knowledge and ontology graph populations.
    #[serde(default, alias = "graph_separation_x")]
    pub graph_separation_x: f32,

    /// Continuous Z-axis scale factor applied to node positions in the force
    /// sim. `1.0` = no compression (fully 3D — the default); smaller values
    /// squash the layout toward the X-Y plane, down to a clamp floor of 0.05.
    ///
    /// NOTE — semantic flip vs. the pre-2026-08 field: the old `axis_compression_z`
    /// was a *compression amount* (0 = none, 0.9 = the disliked near-flat default).
    /// It is now a *Z-scale factor* (1.0 = none), so the default is 1.0 and the
    /// desktop slider maps directly to how "tall" the layout is. A stored legacy
    /// blob whose value was the old 0.9 default therefore now reads as a gentle
    /// 0.9 z-scale (near-3D), NOT the old flatten — so no migration heuristic is
    /// needed (see the round-trip test).
    #[serde(default = "default_axis_compression_z", alias = "axis_compression_z")]
    pub axis_compression_z: f32,

    /// Opt-in canonical "facing dual-disc" display layout (default OFF → fully
    /// 3D force layout). When enabled, the two graph populations are re-centred
    /// into X-Y discs separated along Z by `graph_separation_x`, with disc
    /// thinness driven by `axis_compression_z`. Complementary to the continuous
    /// `axis_compression_z`: this bool gates the disc *re-centre*, the float is
    /// the Z-scale applied in both modes.
    #[serde(default, alias = "enable_dual_disc_layout")]
    pub enable_dual_disc_layout: bool,

    /// Top-N node cap for the initial graph load pushed to a new/anon client
    /// (quality-ranked). `0` or absent = the built-in default (3000). The
    /// consumer clamps to a sanity ceiling (100_000) and may be raised to the
    /// full graph via the settings API.
    #[serde(default, alias = "initial_node_limit")]
    pub initial_node_limit: u32,

    /// ForceAtlas2 LinLog mode: log(1+d) attraction per edge.
    #[serde(default = "default_lin_log_mode", alias = "lin_log_mode")]
    pub lin_log_mode: bool,

    /// FA2 repulsion scaling ratio (default 10.0).
    #[serde(default = "default_scaling_ratio", alias = "scaling_ratio")]
    pub scaling_ratio: f32,

    /// FA2 per-node adaptive speed convergence (default true).
    #[serde(default = "default_adaptive_speed", alias = "adaptive_speed")]
    pub adaptive_speed: bool,

    /// DAG radial hierarchy bias strength (PHASE 2). `0` = off (default). When
    /// > 0, ranked nodes are pulled onto concentric shells (radius =
    /// > `rank * dag_level_distance`) around the hierarchy root, giving a radialout
    /// > tree layout over SUBCLASS_OF / namespace hierarchy edges.
    #[serde(default = "default_dag_bias_k", alias = "dag_bias_k")]
    pub dag_bias_k: f32,

    /// World-space radius added per hierarchy level for the DAG radial bias
    /// (default 60). Larger = more spread-out concentric rings.
    #[serde(default = "default_dag_level_distance", alias = "dag_level_distance")]
    pub dag_level_distance: f32,

    /// Stratified-plane bias strength (ADR-141 P2). `0` = off (default). When
    /// > 0, each node is sprung along Z toward its assigned plane (target_z =
    /// > plane_offset * plane_spacing), separating node types into parallel strata.
    #[serde(default = "default_plane_bias_k", alias = "plane_bias_k")]
    pub plane_bias_k: f32,

    /// World-space Z distance per plane offset unit for the stratified-plane bias
    /// (default 60). Larger = more separated strata.
    #[serde(default = "default_plane_spacing", alias = "plane_spacing")]
    pub plane_spacing: f32,

    /// Sugiyama Y-by-rank layer spring strength (ADR-141 P4). `0` = off (default).
    /// When > 0, each ranked node is sprung along Y toward `rank * layer_spacing`,
    /// giving a top-down layered (Sugiyama) layout over the DAG rank.
    #[serde(default = "default_layer_bias_k", alias = "layer_bias_k")]
    pub layer_bias_k: f32,

    /// World-space Y distance per rank for the Sugiyama layer spring (default 60).
    /// Larger = more separated layers.
    #[serde(default = "default_layer_spacing", alias = "layer_spacing")]
    pub layer_spacing: f32,

    /// Alignment constraint strength (ADR-141 P4b). `0` = off (default). Scales the
    /// per-axis pull of live-kernel ALIGNMENT constraints (e.g. sibling-class
    /// alignment emitted by the OWL→constraint mapper).
    #[serde(default = "default_alignment_strength", alias = "alignment_strength")]
    pub alignment_strength: f32,

    /// FA2 base integration speed.
    #[serde(default = "default_global_speed", alias = "global_speed")]
    pub global_speed: f32,

    /// Per-population spring strength multipliers (literal kernel coefficient,
    /// 1.0 == current LinLog identity). These drive the independent
    /// Knowledge/Ontology/Agent spring sliders end-to-end into the GPU spring_scale
    /// buffer; the global `spring_k` stays the Hooke-mode stiffness.
    #[serde(default = "default_spring_pop_scale", alias = "spring_k_knowledge")]
    pub spring_k_knowledge: f32,
    #[serde(default = "default_spring_pop_scale", alias = "spring_k_ontology")]
    pub spring_k_ontology: f32,
    #[serde(default = "default_spring_pop_scale", alias = "spring_k_agent")]
    pub spring_k_agent: f32,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            auto_balance: false,
            auto_balance_interval_ms: 500,
            auto_balance_config: AutoBalanceConfig::recommended(),
            auto_pause: AutoPauseConfig::recommended(),
            // Canonical compact profile (single source of truth). The YAML
            // visualisation.graphs.knowledge.physics block is NOT applied to the
            // running simulation — boot uses these defaults whenever the sqlite
            // "physics" key is absent (app_state.rs). centerGravityK is the
            // dominant scale control: it sets the equilibrium radius against
            // repelK and reins in disconnected nodes springs can't reach.
            // Measured: 90% of nodes within ~45u of their cluster centroid, the
            // whole graph sitting well inside the ~400u soft-cube bounds.
            bounds_size: 400.0,
            separation_radius: 2.1155233,
            damping: 0.9,
            enable_bounds: true,
            enabled: true,
            iterations: 50,
            max_velocity: 100.0,
            max_force: 150.0,
            repel_k: 120.0,
            spring_k: 12.0,
            boundary_damping: 0.95,
            dt: 0.016,
            temperature: 0.0,
            gravity: 0.002,
            // Community-cohesion force is opt-in: off by default so a fresh graph
            // opens out under repulsion. The detector auto-runs in the force loop
            // only when the user raises this above the >0.0001 gate, so a non-zero
            // default would silently compress every community into its centroid.
            cluster_strength: 0.0,

            rest_length: 50.0,
            repulsion_softening_epsilon: 0.0001,
            center_gravity_k: 0.2,
            grid_cell_size: 50.0,
            warmup_iterations: 100,
            cooling_rate: 0.001,

            max_repulsion_dist: 400.0,
            sssp_alpha: default_sssp_alpha(),

            constraint_ramp_frames: default_constraint_ramp_frames(),
            constraint_max_force_per_node: default_constraint_max_force_per_node(),

            clustering_algorithm: "leiden".to_string(),
            cluster_count: 5,
            clustering_resolution: 1.0,
            clustering_iterations: 50,

            // Dual-disc layout is OFF by default → clients get a natural, fully
            // 3D force layout. `graph_separation_x` still defines the disc gap
            // for users who opt into `enable_dual_disc_layout`; ~100 keeps the
            // knowledge/ontology discs close when that mode is enabled.
            graph_separation_x: 100.0,
            // 1.0 = no Z compression → fully 3D by default (was 0.9, the flatten
            // the user disliked).
            axis_compression_z: default_axis_compression_z(),
            enable_dual_disc_layout: false,
            // 0 = defer to the consumer's built-in default (3000).
            initial_node_limit: 0,
            lin_log_mode: true,
            scaling_ratio: 10.0,
            adaptive_speed: true,
            global_speed: 0.4,
            dag_bias_k: default_dag_bias_k(),
            dag_level_distance: default_dag_level_distance(),
            plane_bias_k: default_plane_bias_k(),
            plane_spacing: default_plane_spacing(),
            layer_bias_k: default_layer_bias_k(),
            layer_spacing: default_layer_spacing(),
            alignment_strength: default_alignment_strength(),
            spring_k_knowledge: 1.0,
            spring_k_ontology: 1.0,
            spring_k_agent: 1.0,
        }
    }
}

/// Legacy constraint shape used by the web API.
/// Modern constraint storage lives in `models::constraints::ConstraintData`.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct LegacyConstraintData {
    #[serde(alias = "constraint_type")]
    pub constraint_type: i32,
    #[serde(alias = "strength")]
    pub strength: f32,
    #[serde(alias = "param1")]
    pub param1: f32,
    #[serde(alias = "param2")]
    pub param2: f32,
    #[serde(alias = "node_mask")]
    pub node_mask: i32,
    #[serde(alias = "enabled")]
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintSystem {
    #[serde(alias = "separation")]
    pub separation: LegacyConstraintData,
    #[serde(alias = "boundary")]
    pub boundary: LegacyConstraintData,
    #[serde(alias = "alignment")]
    pub alignment: LegacyConstraintData,
    #[serde(alias = "cluster")]
    pub cluster: LegacyConstraintData,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ClusteringConfiguration {
    #[serde(alias = "algorithm")]
    pub algorithm: String,
    #[serde(alias = "num_clusters")]
    pub num_clusters: u32,
    #[serde(alias = "resolution")]
    pub resolution: f32,
    #[serde(alias = "iterations")]
    pub iterations: u32,
    #[serde(alias = "export_assignments")]
    pub export_assignments: bool,
    #[serde(alias = "auto_update")]
    pub auto_update: bool,
}

/// Partial physics update payload — every field is `Option<T>` so the API
/// can patch only the keys the client specifies.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Type, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PhysicsUpdate {
    #[serde(alias = "damping")]
    pub damping: Option<f32>,
    #[serde(alias = "spring_k")]
    pub spring_k: Option<f32>,
    #[serde(alias = "repel_k")]
    pub repel_k: Option<f32>,
    #[serde(alias = "iterations")]
    pub iterations: Option<u32>,
    #[serde(alias = "enabled")]
    pub enabled: Option<bool>,
    #[serde(alias = "bounds_size")]
    pub bounds_size: Option<f32>,
    #[serde(alias = "enable_bounds")]
    pub enable_bounds: Option<bool>,
    #[serde(alias = "max_velocity")]
    pub max_velocity: Option<f32>,
    #[serde(alias = "max_force")]
    pub max_force: Option<f32>,
    #[serde(alias = "separation_radius")]
    pub separation_radius: Option<f32>,
    #[serde(alias = "boundary_damping")]
    pub boundary_damping: Option<f32>,
    #[serde(alias = "dt")]
    pub dt: Option<f32>,
    #[serde(alias = "temperature")]
    pub temperature: Option<f32>,
    #[serde(alias = "gravity")]
    pub gravity: Option<f32>,
    #[serde(alias = "cluster_strength")]
    pub cluster_strength: Option<f32>,
    #[serde(alias = "sssp_alpha")]
    pub sssp_alpha: Option<f32>,
    #[serde(alias = "max_repulsion_dist")]
    pub max_repulsion_dist: Option<f32>,
    #[serde(alias = "warmup_iterations")]
    pub warmup_iterations: Option<u32>,
    #[serde(alias = "cooling_rate")]
    pub cooling_rate: Option<f32>,
    #[serde(alias = "clustering_algorithm")]
    pub clustering_algorithm: Option<String>,
    #[serde(alias = "cluster_count")]
    pub cluster_count: Option<u32>,
    #[serde(alias = "clustering_resolution")]
    pub clustering_resolution: Option<f32>,
    #[serde(alias = "clustering_iterations")]
    pub clustering_iterations: Option<u32>,
    #[serde(alias = "repulsion_softening_epsilon")]
    pub repulsion_softening_epsilon: Option<f32>,
    #[serde(alias = "center_gravity_k")]
    pub center_gravity_k: Option<f32>,
    #[serde(alias = "grid_cell_size")]
    pub grid_cell_size: Option<f32>,
    #[serde(alias = "rest_length")]
    pub rest_length: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physics_settings_default() {
        let ps = PhysicsSettings::default();
        assert_eq!(ps.max_velocity, 100.0);
        // Canonical compact profile (single source of truth): forces tuned so the
        // ~10.7k-node graph settles within the ~400u soft-cube bounds.
        assert_eq!(ps.max_force, 150.0);
        assert_eq!(ps.repel_k, 120.0);
        assert_eq!(ps.spring_k, 12.0);
        assert_eq!(ps.center_gravity_k, 0.2);
        assert_eq!(ps.bounds_size, 400.0);
        assert!(ps.enable_bounds);
        assert_eq!(ps.max_repulsion_dist, 400.0);
        assert_eq!(ps.sssp_alpha, 1.5);
        assert_eq!(ps.cluster_strength, 0.0);
        assert!(ps.enabled);
        assert!(!ps.auto_balance);
        // Close full-size dual-disc envelope: the canonical separation must be
        // the close value (~100), never 0 (merged single plane) or 250 (far
        // apart). reset_layout and the boot SQLite seed both source this.
        assert_eq!(ps.graph_separation_x, 100.0);
        // Z-scale default is 1.0 = no compression (fully 3D). NOT the old 0.9.
        assert!((ps.axis_compression_z - 1.0).abs() < 1e-9);
        // Dual-disc layout is opt-in; default OFF → fully 3D.
        assert!(!ps.enable_dual_disc_layout);
        // 0 = defer to the consumer default (3000).
        assert_eq!(ps.initial_node_limit, 0);
    }

    #[test]
    fn test_physics_settings_camelcase_sssp_alpha() {
        let ps = PhysicsSettings {
            sssp_alpha: 3.0,
            ..Default::default()
        };
        let stored = serde_json::to_value(&ps).unwrap();
        // serde rename_all = camelCase emits ssspAlpha.
        assert!(stored.get("ssspAlpha").is_some());
        let loaded: PhysicsSettings = serde_json::from_value(stored).unwrap();
        assert!((loaded.sssp_alpha - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_auto_pause_default() {
        let ap = AutoPauseConfig::recommended();
        assert!(ap.enabled);
        assert!(ap.resume_on_interaction);
    }

    #[test]
    fn test_auto_balance_default() {
        let ab = AutoBalanceConfig::recommended();
        assert_eq!(ab.stability_frame_count, 180);
        assert_eq!(ab.min_oscillation_changes, 8);
    }

    #[test]
    fn test_serde_round_trip() {
        let ps = PhysicsSettings::default();
        let json = serde_json::to_string(&ps).unwrap();
        let back: PhysicsSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.max_velocity, ps.max_velocity);
        assert_eq!(back.damping, ps.damping);
    }
}
