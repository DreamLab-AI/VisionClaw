//! Simulation parameters — re-exported from `visionclaw-domain` per ADR-090.
//!
//! The framework-agnostic data shapes (SimulationParams, SettleMode,
//! SimulationMode, SimulationPhase, FeatureFlags) live in the domain crate.
//! This module retains the GPU-aligned `SimParams` struct and the conversion
//! impls that depend on `crate::config::dev_config` — those cannot live in
//! the domain crate because they pull in CUDA/runtime config.

use bytemuck::{Pod, Zeroable};
use cudarc::driver::DeviceRepr;
use cust_core::DeviceCopy;

// Re-export the domain-owned shapes so existing
// `use crate::models::simulation_params::SimulationParams` imports keep working.
pub use visionclaw_domain::models::simulation_params::{
    FeatureFlags, SettleMode, SimulationMode, SimulationParams, SimulationPhase,
};

use visionclaw_domain::types::layout::LayoutMode;
use visionclaw_domain::types::physics_config::{
    AutoBalanceConfig, AutoPauseConfig, PhysicsSettings,
};

// GPU-aligned simulation parameters. Mirrors the CUDA `SimParams` struct;
// must match its size and layout exactly (see `const _:()` assertion below).
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
    // Stress majorization params live on CPU (SemanticProcessorActor); not in GPU SimParams.
    /// Gravity pull toward origin. Added at end to preserve repr(C) layout.
    pub gravity: f32,

    // ForceAtlas2 / LinLog parameters
    pub lin_log_mode: u32,
    pub scaling_ratio: f32,
    pub adaptive_speed: u32,
    pub global_speed: f32,

    // DAG radial hierarchy bias (PHASE 2). `dag_bias_k` = 0 disables the term.
    // Added at the end to preserve the existing repr(C) prefix layout.
    pub dag_bias_k: f32,
    pub dag_level_distance: f32,

    // Layout mode selector (ADR-141 P1). GPU-visible discriminant of the active
    // `LayoutMode` (0=ForceDirected … 5=Clustered). Mirrors the CUDA field added at
    // the tail of `SimParams`; the seam P2–P4 branch on. Added at the end to
    // preserve the existing repr(C) prefix layout.
    pub layout_mode: u32,

    // ADR-141 P2 stratified planes. `plane_bias_k` = 0 disables the term.
    // Added at the end to preserve the existing repr(C) prefix layout.
    pub plane_bias_k: f32,
    pub plane_spacing: f32,

    // ADR-141 P3 radial shell centre. The dag_radial_bias term springs each node
    // onto its shell around this point; (0,0,0) = origin = legacy DAG behaviour.
    // Added at the end to preserve the existing repr(C) prefix layout.
    pub radial_center_x: f32,
    pub radial_center_y: f32,
    pub radial_center_z: f32,

    // ADR-141 P4 Sugiyama Y-by-rank layer spring; 0 = off. Springs each ranked
    // node's Y toward `rank * layer_spacing`. Added at the end to preserve the
    // existing repr(C) prefix layout.
    pub layer_bias_k: f32,
    pub layer_spacing: f32,
}

// SAFETY: SimParams is repr(C) with only POD types; safe for GPU transfer.
unsafe impl DeviceRepr for SimParams {}
unsafe impl DeviceCopy for SimParams {}

impl Default for SimParams {
    fn default() -> Self {
        Self::new()
    }
}

impl SimParams {
    pub fn new() -> Self {
        let params = SimulationParams::new();
        SimParams::from(&params)
    }

    pub fn set_iteration(&mut self, iteration: i32) {
        self.iteration = iteration;
    }

    pub fn to_simulation_params(&self) -> SimulationParams {
        SimulationParams {
            enabled: true,
            auto_balance: false,
            auto_balance_interval_ms: 100,
            auto_balance_config: AutoBalanceConfig::recommended(),
            auto_pause_config: AutoPauseConfig::recommended(),
            equilibrium_stability_counter: 0,
            is_physics_paused: false,
            iterations: 100,
            dt: self.dt,
            repel_k: self.repel_k,
            damping: self.damping,
            // Carry the authoritative GPU value rather than a hardcoded override.
            boundary_damping: self.boundary_damping,
            viewport_bounds: self.viewport_bounds,
            enable_bounds: true,
            max_velocity: self.max_velocity,
            max_force: self.max_force,
            // Carry the authoritative GPU value rather than a hardcoded 0.0.
            spring_k: self.spring_k,
            separation_radius: self.separation_radius,
            center_gravity_k: self.center_gravity_k,
            temperature: self.temperature,
            // alignment_strength / compute_mode / min_distance are internal-only
            // fields with no GPU source; default them deterministically.
            alignment_strength: self.alignment_strength,
            cluster_strength: self.cluster_strength,
            compute_mode: 0,
            min_distance: 1.0,
            max_repulsion_dist: self.repulsion_cutoff,
            warmup_iterations: self.warmup_iterations,
            cooling_rate: self.cooling_rate,
            rest_length: self.rest_length,
            use_sssp_distances: true,
            sssp_alpha: Some(self.sssp_alpha),
            constraint_ramp_frames: self.constraint_ramp_frames,
            constraint_max_force_per_node: self.constraint_max_force_per_node,
            repulsion_softening_epsilon: self.repulsion_softening_epsilon,
            grid_cell_size: self.grid_cell_size,
            // Carry the authoritative GPU value rather than a hardcoded 0.0001.
            gravity: self.gravity,
            phase: SimulationPhase::Dynamic,
            mode: SimulationMode::Remote,
            settle_mode: SettleMode::default(),
            // axis_compression_z / enable_dual_disc_layout
            // are CPU-side projection params with no field in the GPU-aligned
            // SimParams struct, so this reverse conversion cannot recover the live
            // value. Source them from the canonical PhysicsSettings::default().
            axis_compression_z: PhysicsSettings::default().axis_compression_z,
            enable_dual_disc_layout: PhysicsSettings::default().enable_dual_disc_layout,
            layout_mode: LayoutMode::from_gpu_u32(self.layout_mode),
            lin_log_mode: self.lin_log_mode != 0,
            scaling_ratio: self.scaling_ratio,
            adaptive_speed: self.adaptive_speed != 0,
            global_speed: self.global_speed,
            dag_bias_k: self.dag_bias_k,
            dag_level_distance: self.dag_level_distance,
            plane_bias_k: self.plane_bias_k,
            plane_spacing: self.plane_spacing,
            radial_center: [
                self.radial_center_x,
                self.radial_center_y,
                self.radial_center_z,
            ],
            layer_bias_k: self.layer_bias_k,
            layer_spacing: self.layer_spacing,
            // Per-population spring multipliers (LinLog identity). This GPU-side
            // struct has no per-population source, so default to 1.0.
            spring_k_knowledge: 1.0,
            spring_k_ontology: 1.0,
            spring_k_agent: 1.0,
        }
    }
}

/// Local extension trait so existing call sites can keep using
/// `params.to_sim_params()` even though `SimulationParams` itself lives in
/// the domain crate (which knows nothing about CUDA-aligned `SimParams`).
pub trait ToSimParams {
    fn to_sim_params(&self) -> SimParams;
}

impl ToSimParams for SimulationParams {
    fn to_sim_params(&self) -> SimParams {
        SimParams::from(self)
    }
}

// Compile-time size assertion: SimParams must match the CUDA struct exactly.
const _: () = assert!(std::mem::size_of::<SimParams>() == 212);

// ── ADR-2028: versioned SimParams field/type/offset ABI manifest ────────────

/// Scalar type of one [`SimParams`] field on the wire to the device. All three
/// are 4 bytes, which is exactly why a size guard alone cannot detect drift:
/// swapping two fields, or changing `f32` to `u32`, leaves `size_of` unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    F32,
    U32,
    I32,
}

impl FieldType {
    /// Every SimParams scalar is 4 bytes wide.
    pub const fn size(self) -> usize {
        4
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            FieldType::F32 => "f32",
            FieldType::U32 => "u32",
            FieldType::I32 => "i32",
        }
    }
}

/// One entry of the [`SIMPARAMS_MANIFEST`]: a field's name, scalar type and byte
/// offset within the `repr(C)` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimParamsField {
    pub name: &'static str,
    pub ty: FieldType,
    pub offset: usize,
}

/// How a candidate layout departs from the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiDrift {
    /// The number of fields changed.
    FieldCount { expected: usize, actual: usize },
    /// A field is not where the manifest says it is.
    Offset {
        name: &'static str,
        expected: usize,
        actual: usize,
    },
    /// A field's scalar type changed without moving — invisible to `size_of`.
    Type {
        name: &'static str,
        expected: FieldType,
        actual: FieldType,
    },
    /// A field was renamed or reordered such that position `index` holds a
    /// different field than the manifest declares.
    Name {
        index: usize,
        expected: &'static str,
        actual: &'static str,
    },
    /// Total struct size changed.
    Size { expected: usize, actual: usize },
}

/// Frozen total size of the `repr(C)` `SimParams`, mirrored by the CUDA
/// `static_assert` in `visionclaw_unified.cu`.
pub const SIMPARAMS_SIZE: usize = 212;

/// Frozen alignment. Every member is a 4-byte scalar, so the struct aligns to 4
/// and contains no padding — which is what makes a raw byte copy to the device
/// well-defined in the first place.
pub const SIMPARAMS_ALIGN: usize = 4;

/// Version of the SimParams ABI this binary speaks (ADR-2028).
///
/// Bump this **whenever [`SIMPARAMS_MANIFEST`] changes in any way** — a field
/// added, removed, renamed, retyped or moved. It is the coordination token for
/// rollout and rollback: a precompiled device module, a raw-copy consumer or an
/// older client can be checked against this number instead of inferring
/// compatibility from a size that did not happen to change.
pub const SIMPARAMS_ABI_VERSION: u32 = 1;

/// The declared field/type/offset layout of [`SimParams`] (ADR-2028).
///
/// # Why a manifest and not just a size assertion
///
/// The existing `const _: () = assert!(size_of::<SimParams>() == 212)` pair on
/// the Rust and CUDA sides detects *growth*. It does not detect reordering or
/// retyping: the closeout probe swapped `dt` and `damping` in a fixture and the
/// original size assertion still passed while both offsets moved. Since every
/// field is a 4-byte scalar, same-size drift is the *likely* failure mode, not an
/// exotic one — and it silently feeds the damping value into the timestep on the
/// device.
///
/// This manifest pins name, type and offset for all 53 fields, so any such change
/// fails [`verify_simparams_abi`] with a precise [`AbiDrift`] rather than passing
/// a size check. Keep it ordered exactly as the struct is declared.
pub const SIMPARAMS_MANIFEST: [SimParamsField; 53] = [
    SimParamsField {
        name: "dt",
        ty: FieldType::F32,
        offset: 0,
    },
    SimParamsField {
        name: "damping",
        ty: FieldType::F32,
        offset: 4,
    },
    SimParamsField {
        name: "warmup_iterations",
        ty: FieldType::U32,
        offset: 8,
    },
    SimParamsField {
        name: "cooling_rate",
        ty: FieldType::F32,
        offset: 12,
    },
    SimParamsField {
        name: "spring_k",
        ty: FieldType::F32,
        offset: 16,
    },
    SimParamsField {
        name: "rest_length",
        ty: FieldType::F32,
        offset: 20,
    },
    SimParamsField {
        name: "repel_k",
        ty: FieldType::F32,
        offset: 24,
    },
    SimParamsField {
        name: "repulsion_cutoff",
        ty: FieldType::F32,
        offset: 28,
    },
    SimParamsField {
        name: "repulsion_softening_epsilon",
        ty: FieldType::F32,
        offset: 32,
    },
    SimParamsField {
        name: "center_gravity_k",
        ty: FieldType::F32,
        offset: 36,
    },
    SimParamsField {
        name: "max_force",
        ty: FieldType::F32,
        offset: 40,
    },
    SimParamsField {
        name: "max_velocity",
        ty: FieldType::F32,
        offset: 44,
    },
    SimParamsField {
        name: "grid_cell_size",
        ty: FieldType::F32,
        offset: 48,
    },
    SimParamsField {
        name: "feature_flags",
        ty: FieldType::U32,
        offset: 52,
    },
    SimParamsField {
        name: "seed",
        ty: FieldType::U32,
        offset: 56,
    },
    SimParamsField {
        name: "iteration",
        ty: FieldType::I32,
        offset: 60,
    },
    SimParamsField {
        name: "separation_radius",
        ty: FieldType::F32,
        offset: 64,
    },
    SimParamsField {
        name: "cluster_strength",
        ty: FieldType::F32,
        offset: 68,
    },
    SimParamsField {
        name: "alignment_strength",
        ty: FieldType::F32,
        offset: 72,
    },
    SimParamsField {
        name: "temperature",
        ty: FieldType::F32,
        offset: 76,
    },
    SimParamsField {
        name: "viewport_bounds",
        ty: FieldType::F32,
        offset: 80,
    },
    SimParamsField {
        name: "sssp_alpha",
        ty: FieldType::F32,
        offset: 84,
    },
    SimParamsField {
        name: "boundary_damping",
        ty: FieldType::F32,
        offset: 88,
    },
    SimParamsField {
        name: "constraint_ramp_frames",
        ty: FieldType::U32,
        offset: 92,
    },
    SimParamsField {
        name: "constraint_max_force_per_node",
        ty: FieldType::F32,
        offset: 96,
    },
    SimParamsField {
        name: "stability_threshold",
        ty: FieldType::F32,
        offset: 100,
    },
    SimParamsField {
        name: "min_velocity_threshold",
        ty: FieldType::F32,
        offset: 104,
    },
    SimParamsField {
        name: "world_bounds_min",
        ty: FieldType::F32,
        offset: 108,
    },
    SimParamsField {
        name: "world_bounds_max",
        ty: FieldType::F32,
        offset: 112,
    },
    SimParamsField {
        name: "cell_size_lod",
        ty: FieldType::F32,
        offset: 116,
    },
    SimParamsField {
        name: "k_neighbors_max",
        ty: FieldType::U32,
        offset: 120,
    },
    SimParamsField {
        name: "anomaly_detection_radius",
        ty: FieldType::F32,
        offset: 124,
    },
    SimParamsField {
        name: "learning_rate_default",
        ty: FieldType::F32,
        offset: 128,
    },
    SimParamsField {
        name: "norm_delta_cap",
        ty: FieldType::F32,
        offset: 132,
    },
    SimParamsField {
        name: "position_constraint_attraction",
        ty: FieldType::F32,
        offset: 136,
    },
    SimParamsField {
        name: "lof_score_min",
        ty: FieldType::F32,
        offset: 140,
    },
    SimParamsField {
        name: "lof_score_max",
        ty: FieldType::F32,
        offset: 144,
    },
    SimParamsField {
        name: "weight_precision_multiplier",
        ty: FieldType::F32,
        offset: 148,
    },
    SimParamsField {
        name: "gravity",
        ty: FieldType::F32,
        offset: 152,
    },
    SimParamsField {
        name: "lin_log_mode",
        ty: FieldType::U32,
        offset: 156,
    },
    SimParamsField {
        name: "scaling_ratio",
        ty: FieldType::F32,
        offset: 160,
    },
    SimParamsField {
        name: "adaptive_speed",
        ty: FieldType::U32,
        offset: 164,
    },
    SimParamsField {
        name: "global_speed",
        ty: FieldType::F32,
        offset: 168,
    },
    SimParamsField {
        name: "dag_bias_k",
        ty: FieldType::F32,
        offset: 172,
    },
    SimParamsField {
        name: "dag_level_distance",
        ty: FieldType::F32,
        offset: 176,
    },
    SimParamsField {
        name: "layout_mode",
        ty: FieldType::U32,
        offset: 180,
    },
    SimParamsField {
        name: "plane_bias_k",
        ty: FieldType::F32,
        offset: 184,
    },
    SimParamsField {
        name: "plane_spacing",
        ty: FieldType::F32,
        offset: 188,
    },
    SimParamsField {
        name: "radial_center_x",
        ty: FieldType::F32,
        offset: 192,
    },
    SimParamsField {
        name: "radial_center_y",
        ty: FieldType::F32,
        offset: 196,
    },
    SimParamsField {
        name: "radial_center_z",
        ty: FieldType::F32,
        offset: 200,
    },
    SimParamsField {
        name: "layer_bias_k",
        ty: FieldType::F32,
        offset: 204,
    },
    SimParamsField {
        name: "layer_spacing",
        ty: FieldType::F32,
        offset: 208,
    },
];

/// Feature-flag bit manifest (ADR-2028). The `feature_flags` word is as much a
/// part of the device ABI as the field offsets: a bit reassigned on one side and
/// not the other silently enables the wrong force term.
pub const SIMPARAMS_FEATURE_BITS: [(&str, u32); 7] = [
    ("ENABLE_REPULSION", FeatureFlags::ENABLE_REPULSION),
    ("ENABLE_SPRINGS", FeatureFlags::ENABLE_SPRINGS),
    ("ENABLE_CENTERING", FeatureFlags::ENABLE_CENTERING),
    (
        "ENABLE_TEMPORAL_COHERENCE",
        FeatureFlags::ENABLE_TEMPORAL_COHERENCE,
    ),
    ("ENABLE_CONSTRAINTS", FeatureFlags::ENABLE_CONSTRAINTS),
    (
        "ENABLE_STRESS_MAJORIZATION",
        FeatureFlags::ENABLE_STRESS_MAJORIZATION,
    ),
    (
        "ENABLE_SSSP_SPRING_ADJUST",
        FeatureFlags::ENABLE_SSSP_SPRING_ADJUST,
    ),
];

/// The layout the compiler actually produced for [`SimParams`], read with
/// `offset_of!`. Compared against [`SIMPARAMS_MANIFEST`] by
/// [`verify_simparams_abi`].
pub fn simparams_actual_layout() -> Vec<SimParamsField> {
    macro_rules! field {
        ($name:ident, $ty:ident) => {
            SimParamsField {
                name: stringify!($name),
                ty: FieldType::$ty,
                offset: std::mem::offset_of!(SimParams, $name),
            }
        };
    }
    vec![
        field!(dt, F32),
        field!(damping, F32),
        field!(warmup_iterations, U32),
        field!(cooling_rate, F32),
        field!(spring_k, F32),
        field!(rest_length, F32),
        field!(repel_k, F32),
        field!(repulsion_cutoff, F32),
        field!(repulsion_softening_epsilon, F32),
        field!(center_gravity_k, F32),
        field!(max_force, F32),
        field!(max_velocity, F32),
        field!(grid_cell_size, F32),
        field!(feature_flags, U32),
        field!(seed, U32),
        field!(iteration, I32),
        field!(separation_radius, F32),
        field!(cluster_strength, F32),
        field!(alignment_strength, F32),
        field!(temperature, F32),
        field!(viewport_bounds, F32),
        field!(sssp_alpha, F32),
        field!(boundary_damping, F32),
        field!(constraint_ramp_frames, U32),
        field!(constraint_max_force_per_node, F32),
        field!(stability_threshold, F32),
        field!(min_velocity_threshold, F32),
        field!(world_bounds_min, F32),
        field!(world_bounds_max, F32),
        field!(cell_size_lod, F32),
        field!(k_neighbors_max, U32),
        field!(anomaly_detection_radius, F32),
        field!(learning_rate_default, F32),
        field!(norm_delta_cap, F32),
        field!(position_constraint_attraction, F32),
        field!(lof_score_min, F32),
        field!(lof_score_max, F32),
        field!(weight_precision_multiplier, F32),
        field!(gravity, F32),
        field!(lin_log_mode, U32),
        field!(scaling_ratio, F32),
        field!(adaptive_speed, U32),
        field!(global_speed, F32),
        field!(dag_bias_k, F32),
        field!(dag_level_distance, F32),
        field!(layout_mode, U32),
        field!(plane_bias_k, F32),
        field!(plane_spacing, F32),
        field!(radial_center_x, F32),
        field!(radial_center_y, F32),
        field!(radial_center_z, F32),
        field!(layer_bias_k, F32),
        field!(layer_spacing, F32),
    ]
}

/// Compare a candidate layout against a declared manifest, returning every
/// departure (ADR-2028).
///
/// Generic over the candidate so a *fixture* struct can be checked with the same
/// code path the real struct uses — that is what makes the same-size drift
/// negative test meaningful rather than a restatement of the manifest.
pub fn verify_simparams_abi(
    manifest: &[SimParamsField],
    actual: &[SimParamsField],
    actual_size: usize,
    expected_size: usize,
) -> Vec<AbiDrift> {
    let mut drifts = Vec::new();
    if actual_size != expected_size {
        drifts.push(AbiDrift::Size {
            expected: expected_size,
            actual: actual_size,
        });
    }
    if manifest.len() != actual.len() {
        drifts.push(AbiDrift::FieldCount {
            expected: manifest.len(),
            actual: actual.len(),
        });
    }
    for (i, (want, got)) in manifest.iter().zip(actual.iter()).enumerate() {
        if want.name != got.name {
            drifts.push(AbiDrift::Name {
                index: i,
                expected: want.name,
                actual: got.name,
            });
        }
        if want.ty != got.ty {
            drifts.push(AbiDrift::Type {
                name: want.name,
                expected: want.ty,
                actual: got.ty,
            });
        }
        if want.offset != got.offset {
            drifts.push(AbiDrift::Offset {
                name: want.name,
                expected: want.offset,
                actual: got.offset,
            });
        }
    }
    drifts
}

/// A stable digest over a layout's (name, type, offset) triples plus the feature
/// bits (ADR-2028).
///
/// Recorded alongside a build so a shipped artefact — a precompiled PTX module,
/// a release binary, a raw-copy consumer — can be matched to the exact ABI it was
/// compiled against. Two builds agreeing on `size_of` but disagreeing here are
/// **not** interchangeable, which is the distinction the size guard cannot make.
/// FNV-1a: short, dependency-free and stable across runs and platforms (unlike
/// `DefaultHasher`, which is explicitly not stable).
pub fn simparams_abi_digest(fields: &[SimParamsField]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    let eat = |bytes: &[u8], h: &mut u64| {
        for b in bytes {
            *h ^= *b as u64;
            *h = h.wrapping_mul(PRIME);
        }
    };
    eat(&SIMPARAMS_ABI_VERSION.to_le_bytes(), &mut h);
    for f in fields {
        eat(f.name.as_bytes(), &mut h);
        eat(f.ty.as_str().as_bytes(), &mut h);
        eat(&(f.offset as u64).to_le_bytes(), &mut h);
    }
    for (name, bit) in SIMPARAMS_FEATURE_BITS {
        eat(name.as_bytes(), &mut h);
        eat(&bit.to_le_bytes(), &mut h);
    }
    h
}

impl From<&SimParams> for SimulationParams {
    fn from(params: &SimParams) -> Self {
        params.to_simulation_params()
    }
}

impl From<&SimulationParams> for SimParams {
    fn from(params: &SimulationParams) -> Self {
        let mut feature_flags = 0;
        if params.repel_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_REPULSION;
        }
        if params.spring_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_SPRINGS;
        }
        if params.center_gravity_k > 0.0 {
            feature_flags |= FeatureFlags::ENABLE_CENTERING;
        }
        if params.use_sssp_distances {
            feature_flags |= FeatureFlags::ENABLE_SSSP_SPRING_ADJUST;
        }

        SimParams {
            dt: params.dt,
            damping: params.damping,
            warmup_iterations: params.warmup_iterations,
            cooling_rate: params.cooling_rate,
            spring_k: params.spring_k,
            rest_length: params.rest_length,
            repel_k: params.repel_k,
            repulsion_cutoff: params.max_repulsion_dist,
            repulsion_softening_epsilon: params.repulsion_softening_epsilon,
            center_gravity_k: params.center_gravity_k,
            max_force: params.max_force,
            max_velocity: params.max_velocity,
            grid_cell_size: params.grid_cell_size,
            feature_flags,
            seed: 1337,
            iteration: 0,
            separation_radius: params.separation_radius,
            cluster_strength: params.cluster_strength,
            alignment_strength: params.alignment_strength,
            temperature: params.temperature,
            viewport_bounds: if params.enable_bounds {
                params.viewport_bounds
            } else {
                0.0
            },
            sssp_alpha: params.sssp_alpha.unwrap_or(0.0),
            boundary_damping: params.boundary_damping,
            constraint_ramp_frames: params.constraint_ramp_frames,
            constraint_max_force_per_node: params.constraint_max_force_per_node,

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
            gravity: params.gravity,
            lin_log_mode: if params.lin_log_mode { 1 } else { 0 },
            scaling_ratio: params.scaling_ratio,
            adaptive_speed: if params.adaptive_speed { 1 } else { 0 },
            global_speed: params.global_speed,
            dag_bias_k: params.dag_bias_k,
            dag_level_distance: params.dag_level_distance,
            layout_mode: params.layout_mode.as_gpu_u32(),
            plane_bias_k: params.plane_bias_k,
            plane_spacing: params.plane_spacing,
            radial_center_x: params.radial_center[0],
            radial_center_y: params.radial_center[1],
            radial_center_z: params.radial_center[2],
            layer_bias_k: params.layer_bias_k,
            layer_spacing: params.layer_spacing,
        }
    }
}

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
        // Enable SSSP spring adjustment for ontology-aware edge rest lengths.
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
            // alignment_strength (ADR-141 P4b) is a live scalar again: the kernel
            // ALIGNMENT constraint branch scales its per-axis pull by it.
            alignment_strength: physics.alignment_strength,
            temperature: physics.temperature,
            viewport_bounds: if physics.enable_bounds {
                physics.bounds_size
            } else {
                0.0
            },
            sssp_alpha: physics.sssp_alpha,
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
            global_speed: physics.global_speed,
            dag_bias_k: physics.dag_bias_k,
            dag_level_distance: physics.dag_level_distance,
            // PhysicsSettings carries no layout mode — the authoritative mode rides
            // SimulationParams.layout_mode (set via POST /api/layout/mode). Default
            // to ForceDirected here so this wire path never silently forces a mode.
            layout_mode: LayoutMode::ForceDirected.as_gpu_u32(),
            plane_bias_k: physics.plane_bias_k,
            plane_spacing: physics.plane_spacing,
            // PhysicsSettings carries no radial centre — it is actor-authoritative
            // (owned by SetRadialLayout). Default to the origin so this wire path
            // never resets an active radial centre back to nothing.
            radial_center_x: 0.0,
            radial_center_y: 0.0,
            radial_center_z: 0.0,
            layer_bias_k: physics.layer_bias_k,
            layer_spacing: physics.layer_spacing,
        }
    }
}

#[cfg(test)]
mod adr_2028_abi_manifest {
    //! ADR-2028: the SimParams ABI is pinned by a versioned field/type/offset
    //! manifest, not only by a total-size assertion.
    //!
    //! The closeout probe showed the gap: swapping `dt` and `damping` in a
    //! fixture kept `size_of` at 212 and the original assertion still passed,
    //! while both offsets had moved. Every field here is a 4-byte scalar, so
    //! *same-size drift is the ordinary failure mode*, and a size guard is blind
    //! to all of it. These tests check the real struct against the manifest and
    //! then prove, on a deliberately drifted fixture, that the check actually
    //! catches what the size assertion misses.
    use super::*;

    #[test]
    fn the_real_struct_matches_the_declared_manifest() {
        let actual = simparams_actual_layout();
        let drifts = verify_simparams_abi(
            &SIMPARAMS_MANIFEST,
            &actual,
            std::mem::size_of::<SimParams>(),
            SIMPARAMS_SIZE,
        );
        assert!(
            drifts.is_empty(),
            "SimParams has drifted from its declared ABI manifest: {drifts:#?}\n\
             If this change is intended, update SIMPARAMS_MANIFEST *and* bump \
             SIMPARAMS_ABI_VERSION, and coordinate every raw-copy consumer and \
             precompiled device module."
        );
    }

    #[test]
    fn the_frozen_size_and_alignment_hold() {
        assert_eq!(std::mem::size_of::<SimParams>(), SIMPARAMS_SIZE);
        assert_eq!(std::mem::align_of::<SimParams>(), SIMPARAMS_ALIGN);
        // 53 fields x 4 bytes with no padding is what makes the raw device copy
        // well-defined; if this ever fails the struct has gained padding.
        assert_eq!(SIMPARAMS_MANIFEST.len() * 4, SIMPARAMS_SIZE);
    }

    #[test]
    fn manifest_offsets_are_dense_ascending_and_in_range() {
        for (i, f) in SIMPARAMS_MANIFEST.iter().enumerate() {
            assert_eq!(f.offset, i * 4, "field {} is not at its dense slot", f.name);
            assert!(f.offset + f.ty.size() <= SIMPARAMS_SIZE);
        }
    }

    #[test]
    fn manifest_field_names_are_unique() {
        let mut names: Vec<&str> = SIMPARAMS_MANIFEST.iter().map(|f| f.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate field name in the manifest");
    }

    // ── The negative: same-size drift the size assertion cannot see ─────────

    /// A fixture with `dt` and `damping` swapped — exactly the mutation the
    /// closeout probe applied to the CUDA declaration. Same 53 fields, same
    /// 4-byte scalars, same 212 bytes.
    fn swapped_dt_and_damping() -> Vec<SimParamsField> {
        let mut fixture = SIMPARAMS_MANIFEST.to_vec();
        let dt = fixture.iter().position(|f| f.name == "dt").unwrap();
        let damping = fixture.iter().position(|f| f.name == "damping").unwrap();
        // Swap the two fields' positions, keeping the offsets dense — i.e. the
        // declaration order changed, which is what a source edit would do.
        let (a, b) = (fixture[dt].offset, fixture[damping].offset);
        fixture.swap(dt, damping);
        fixture[dt].offset = a;
        fixture[damping].offset = b;
        fixture
    }

    #[test]
    fn a_same_size_field_swap_still_passes_the_size_assertion() {
        // This is the point of the whole manifest: the guard that exists today
        // is satisfied by a layout that is genuinely incompatible.
        let drifted = swapped_dt_and_damping();
        assert_eq!(
            drifted.len() * 4,
            SIMPARAMS_SIZE,
            "the drifted fixture is the same total size — the size guard passes"
        );
    }

    #[test]
    fn the_manifest_catches_the_same_size_field_swap() {
        let drifted = swapped_dt_and_damping();
        let drifts = verify_simparams_abi(
            &SIMPARAMS_MANIFEST,
            &drifted,
            drifted.len() * 4,
            SIMPARAMS_SIZE,
        );
        assert!(!drifts.is_empty(), "same-size drift must be detected");
        // No size drift is reported — only the reordering.
        assert!(
            !drifts.iter().any(|d| matches!(d, AbiDrift::Size { .. })),
            "the size is unchanged; the manifest is what catches this"
        );
        assert!(
            drifts
                .iter()
                .any(|d| matches!(d, AbiDrift::Name { expected, actual, .. }
                    if *expected == "dt" && *actual == "damping")),
            "expected a name drift at dt's slot, got {drifts:#?}"
        );
    }

    #[test]
    fn the_manifest_catches_a_same_size_retype() {
        // f32 -> u32 does not move a single byte, and reinterprets every value.
        let mut drifted = SIMPARAMS_MANIFEST.to_vec();
        let i = drifted.iter().position(|f| f.name == "dt").unwrap();
        drifted[i].ty = FieldType::U32;
        let drifts = verify_simparams_abi(
            &SIMPARAMS_MANIFEST,
            &drifted,
            SIMPARAMS_SIZE,
            SIMPARAMS_SIZE,
        );
        assert!(
            drifts.iter().any(|d| matches!(
                d,
                AbiDrift::Type { name, expected: FieldType::F32, actual: FieldType::U32 }
                    if *name == "dt"
            )),
            "expected a type drift on dt, got {drifts:#?}"
        );
    }

    #[test]
    fn the_manifest_catches_a_tail_append_that_grows_the_struct() {
        // Appending preserves every existing offset — old consumers keep reading
        // the right fields — but a shorter allocation or an older device module
        // is still unsafe against the new size. Growth must be reported.
        let mut drifted = SIMPARAMS_MANIFEST.to_vec();
        drifted.push(SimParamsField {
            name: "future_field",
            ty: FieldType::F32,
            offset: SIMPARAMS_SIZE,
        });
        let drifts = verify_simparams_abi(
            &SIMPARAMS_MANIFEST,
            &drifted,
            SIMPARAMS_SIZE + 4,
            SIMPARAMS_SIZE,
        );
        assert!(drifts.iter().any(|d| matches!(d, AbiDrift::Size { .. })));
        assert!(drifts
            .iter()
            .any(|d| matches!(d, AbiDrift::FieldCount { .. })));
    }

    // ── Digest: binds a shipped artefact to the exact ABI ───────────────────

    #[test]
    fn the_digest_is_stable_and_order_sensitive() {
        let a = simparams_abi_digest(&SIMPARAMS_MANIFEST);
        assert_eq!(a, simparams_abi_digest(&SIMPARAMS_MANIFEST), "stable");
        assert_eq!(
            a,
            simparams_abi_digest(&simparams_actual_layout()),
            "the real layout must digest identically to the manifest"
        );
        // Same size, different digest — the discrimination the size guard lacks.
        assert_ne!(
            a,
            simparams_abi_digest(&swapped_dt_and_damping()),
            "a same-size reorder must change the digest"
        );
    }

    #[test]
    fn the_digest_covers_the_feature_bits() {
        // The feature word is part of the device ABI: a bit reassigned on one
        // side only silently enables the wrong force term.
        let mut seen = std::collections::HashSet::new();
        for (name, bit) in SIMPARAMS_FEATURE_BITS {
            assert!(bit.count_ones() == 1, "{name} must be a single bit");
            assert!(seen.insert(bit), "{name} reuses an already-claimed bit");
        }
        assert_eq!(
            SIMPARAMS_FEATURE_BITS.len(),
            7,
            "a new feature bit needs a manifest entry and an ABI version bump"
        );
    }

    #[test]
    fn the_manifest_covers_the_fields_the_force_derivation_reads() {
        // Cross-check against the fields ADR-2029's final device word writes, so
        // the two ADRs cannot drift apart silently.
        for required in [
            "dt",
            "damping",
            "feature_flags",
            "repel_k",
            "spring_k",
            "center_gravity_k",
        ] {
            assert!(
                SIMPARAMS_MANIFEST.iter().any(|f| f.name == required),
                "{required} missing from the manifest"
            );
        }
    }
}
