//! Force Compute Actor - Handles physics force computation and simulation

use actix::prelude::*;
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use super::shared::{GPUOperation, GPUState, SharedGPUContext};
use crate::actors::messages::*;
use crate::gpu::backpressure::{BackpressureConfig, NetworkBackpressure};
use crate::gpu::broadcast_optimizer::{BroadcastConfig, BroadcastOptimizer};
use crate::models::simulation_params::{SimulationParams, ToSimParams};
use crate::telemetry::agent_telemetry::{
    get_telemetry_logger, CorrelationId, LogLevel, TelemetryEvent,
};
use crate::utils::socket_flow_messages::{glam_to_vec3data, BinaryNodeDataClient};
use crate::utils::unified_gpu_compute::ComputeMode;
use crate::utils::unified_gpu_compute::SimParams;
use glam::Vec3;

use cudarc::driver::CudaDevice;

/// Per-node graph population classification for dual-graph X-axis separation.
/// Stored per GPU index during graph upload, used during position broadcast.
///
/// This is the GPU-local mirror of the canonical
/// [`visionclaw_domain::models::Population`]; the single source of truth for
/// classifying a node is [`visionclaw_domain::models::Node::population`], which
/// reads the authoritative `metadata["type"]` origin field. This enum exists
/// only because the GPU buffers/centroids index by it; it is produced solely
/// via the `From<Population>` conversion below — never re-derived from fields.
#[derive(Debug, Clone, Copy, PartialEq)]
enum GraphPopulation {
    Knowledge,
    Ontology,
    Agent,
}

impl From<visionclaw_domain::models::Population> for GraphPopulation {
    fn from(p: visionclaw_domain::models::Population) -> Self {
        match p {
            visionclaw_domain::models::Population::Knowledge => GraphPopulation::Knowledge,
            visionclaw_domain::models::Population::Ontology => GraphPopulation::Ontology,
            visionclaw_domain::models::Population::Agent => GraphPopulation::Agent,
        }
    }
}

/// Median (x, y) in-plane centroid per population, indexed
/// [Knowledge, Ontology, Agent]. Index `i` aligns with the i-th (x, y)
/// returned by `xy`. Median (not mean) so a handful of physics outliers
/// flung to the simulation boundary can't drag a disc's centre.
fn population_centroids_xy(
    populations: &[GraphPopulation],
    xy: impl Fn(usize) -> (f32, f32),
    n: usize,
) -> [(f32, f32); 3] {
    let mut bx: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut by: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for i in 0..n {
        let b = match populations.get(i) {
            Some(GraphPopulation::Knowledge) => 0,
            Some(GraphPopulation::Ontology) => 1,
            Some(GraphPopulation::Agent) => 2,
            None => continue,
        };
        let (x, y) = xy(i);
        if x.is_finite() && y.is_finite() {
            bx[b].push(x);
            by[b].push(y);
        }
    }
    let median = |v: &mut Vec<f32>| -> f32 {
        if v.is_empty() {
            return 0.0;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[v.len() / 2]
    };
    let mut out = [(0.0f32, 0.0f32); 3];
    for b in 0..3 {
        out[b] = (median(&mut bx[b]), median(&mut by[b]));
    }
    out
}

/// Generous, separation-INDEPENDENT rim-clamp radius for each facing disc.
///
/// The rim clamp pulls true outliers back onto the disc so a few stray nodes
/// can't spray across the gap and visually merge the two populations. It must
/// NOT be coupled to `graph_separation_x`: previously `r_max = sep * 0.85`
/// shrank every disc as the user pulled the discs together (sep~100 collapsed
/// each disc to radius ~85), which is the opposite of the desired "full-size
/// discs, close together" view. Driving the radius from a fixed generous value
/// keeps each disc its natural size regardless of how close the two discs sit.
/// Healthy steady-state layout radius is ~±2400, so this comfortably contains a
/// normal disc and only clamps genuine runaways.
const DISC_RIM_RADIUS: f32 = 2600.0;

/// Clamp floor for the continuous `axis_compression_z` Z-scale factor. `1.0` is
/// no compression (fully 3D, the default); this floor caps how flat the layout
/// can be squashed. The field is a real user tunable applied in the sim as the
/// Z multiplier, and — when `enable_dual_disc_layout` is on — as the disc face
/// scale (thinness) too.
const Z_SCALE_MIN: f32 = 0.05;

/// Clamp `axis_compression_z` into the valid Z-scale range `[Z_SCALE_MIN, 1.0]`.
#[inline]
fn clamp_z_scale(axis_compression_z: f32) -> f32 {
    axis_compression_z.clamp(Z_SCALE_MIN, 1.0)
}

/// Re-centre a node onto its population's disc, flatten Z, and offset along Z.
/// Each disc is centred at its population's median in the X-Y plane, flattened
/// thin on Z, then translated to its target Z (∓sep for Knowledge/Ontology,
/// 0 for Agent), so the two discs become parallel X-Y planes facing one another
/// across a depth gap regardless of where the raw physics layout drifts.
/// Flatten and separation share the Z axis (the disc normal), which is what
/// makes the faces point at each other. Nodes beyond `r_max` from the disc
/// centre are pulled to the rim so each disc stays compact and outliers can't
/// spray across the gap and merge the two populations. `r_max` is the disc's
/// own rim radius (see `DISC_RIM_RADIUS`), DECOUPLED from the separation so the
/// discs keep their full size when pulled close (sep small).
#[inline]
fn project_node_xy(
    pos: &mut Vec3,
    pop: GraphPopulation,
    centroids: &[(f32, f32); 3],
    sep: f32,
    face_scale: f32,
    r_max: f32,
) {
    // Discs FACE one another: flatten and separate on the SAME axis (Z).
    // Each disc is centred + rim-clamped in its X-Y plane, flattened thin along
    // Z, then offset along Z by ±sep. The result is two parallel X-Y discs whose
    // faces point at each other across a depth gap, with agents on the mid plane
    // (z=0) and the KG<->ontology cross-links spanning the gap between the faces.
    let (idx, target_z) = match pop {
        GraphPopulation::Knowledge => (0usize, -sep),
        GraphPopulation::Ontology => (1usize, sep),
        GraphPopulation::Agent => (2usize, 0.0),
    };
    let (cx, cy) = centroids[idx];
    let mut dx = pos.x - cx;
    let mut dy = pos.y - cy;
    if r_max > 0.0 {
        let d2 = dx * dx + dy * dy;
        if d2 > r_max * r_max {
            let s = r_max / d2.sqrt();
            dx *= s;
            dy *= s;
        }
    }
    pos.x = dx;
    pos.y = dy;
    pos.z = pos.z * face_scale + target_z;
}

// ---------------------------------------------------------------------------
// Divergence hardening constants
//
// The CUDA force-directed layout can, under data-load churn, blow positions
// toward infinity ("nodes at infinity / two planes of edges"). These bounds
// are DEFENSIVE: they only clamp *true* divergence, never healthy spread.
// Healthy steady-state radius is ~±2400, so the bounds below are ~40x larger.
// ---------------------------------------------------------------------------

/// Absolute world-coordinate bound (per axis). Any healthy layout sits well
/// inside this (~±2400 observed); we clamp only runaway divergence. Generous
/// so legitimate large graphs are never distorted.
const MAX_COORD: f32 = 100_000.0;

/// Maximum per-node velocity magnitude. A node moving faster than this is
/// diverging, not settling. SimulationParams::max_velocity governs the healthy
/// path on the GPU; this is a hard backstop after readback in case the kernel
/// clamp was bypassed or the value itself was corrupted to NaN/Inf.
const MAX_VELOCITY_MAGNITUDE: f32 = 1_000.0;

/// Average kinetic energy above which the simulation is considered divergent.
/// Healthy KE is small (sub-unity at convergence, single/double digits while
/// reheating). 1e9 is far above any sane transient and only trips on explosion.
const MAX_KINETIC_ENERGY: f64 = 1.0e9;

/// Consecutive divergent/bad frames tolerated before the circuit breaker halts
/// stepping. A handful of bad frames can be transient GPU glitches; sustained
/// divergence means the layout has exploded and must be frozen.
const MAX_CONSECUTIVE_BAD_FRAMES: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsStats {
    pub iteration_count: u32,
    pub gpu_failure_count: u32,
    pub current_params: SimulationParams,
    pub compute_mode: ComputeMode,
    pub nodes_count: u32,
    pub edges_count: u32,

    pub average_velocity: f32,
    pub kinetic_energy: f32,
    pub total_forces: f32,

    pub last_step_duration_ms: f32,
    pub fps: f32,

    pub num_edges: u32,
    pub total_force_calculations: u32,
}

#[allow(dead_code)]
pub struct ForceComputeActor {
    gpu_state: GPUState,

    shared_context: Option<Arc<SharedGPUContext>>,

    simulation_params: SimulationParams,

    unified_params: SimParams,

    compute_mode: ComputeMode,

    last_step_start: Option<Instant>,
    last_step_duration_ms: f32,

    is_computing: bool,

    skipped_frames: u32,

    reheat_factor: f32,

    stability_iterations: u32,

    /// Frames to bypass GPU stability-skip after a parameter change.
    /// When >0, stability_threshold is forced to 0.0 so physics always runs.
    stability_warmup_remaining: u32,

    graph_service_addr: Option<Addr<crate::actors::GraphServiceSupervisor>>,

    ontology_constraint_addr:
        Option<Addr<super::ontology_constraint_actor::OntologyConstraintActor>>,

    /// Cached constraint buffer from OntologyConstraintActor for GPU upload
    cached_constraint_buffer: Vec<crate::models::constraints::ConstraintData>,

    /// Semantic forces actor for DAG layout, type clustering, and collision
    semantic_forces_addr: Option<Addr<super::semantic_forces_actor::SemanticForcesActor>>,

    /// Broadcast optimizer for delta compression and spatial culling
    broadcast_optimizer: BroadcastOptimizer,

    /// When true, skip intermediate broadcasts (FastSettle burst in progress).
    /// Cleared by `force_full_broadcast` flag to send final converged positions.
    suppress_intermediate_broadcasts: bool,

    /// Force next broadcast to include ALL nodes (bypass delta filter).
    force_full_broadcast: bool,

    /// Network backpressure controller with token bucket algorithm
    backpressure: NetworkBackpressure,

    /// Iteration count of the last full (non-delta) broadcast.
    /// Used to periodically send ALL positions so late-connecting clients get state.
    last_full_broadcast_iteration: u32,

    /// Pre-allocated buffer for position/velocity data (reused every frame to avoid 60Hz allocations)
    position_velocity_buffer: Vec<(Vec3, Vec3)>,

    /// Pre-allocated buffer for node IDs (reused every frame to avoid 60Hz allocations)
    node_id_buffer: Vec<u32>,

    /// Maps GPU buffer index → actual graph node ID (populated during graph upload)
    gpu_index_to_node_id: Vec<u32>,

    /// Client-dragged nodes pinned to a fixed position (node_id → world position).
    /// Each step, pinned nodes are rewritten to these positions on the GPU buffer
    /// (and held there rather than integrated) so the force kernel relaxes their
    /// neighbours around the dragged location. Populated by `PinNodePositions`.
    pinned_nodes: std::collections::HashMap<u32, Vec3>,

    /// Set whenever `pinned_nodes` changes (pin/unpin). The next physics step
    /// rebuilds the per-node GPU pinned mask from `pinned_nodes` and uploads it,
    /// then clears this. Ensures the last unpin still uploads a cleared mask.
    pinned_mask_dirty: bool,

    /// Per-node graph population classification for dual-graph X-axis offset.
    /// Indexed by GPU buffer index. Populated during graph upload from node_type field.
    node_population: Vec<GraphPopulation>,

    /// ADR-141 P3: cached DAG hierarchy ranks (the `node_rank` key vec computed at
    /// upload). Retained so `SetRadialLayout { DagRank }` can re-key on demand
    /// without recomputing. `-1.0` = unranked. Empty until the first graph upload.
    dag_ranks: Vec<f32>,

    /// ADR-141 P3: undirected neighbour indices (GPU index → neighbour GPU indices)
    /// for the RadialMode::Ego BFS hop-distance. Built from the same edge loop that
    /// forms the CSR adjacency. Empty until the first graph upload.
    graph_adjacency: Vec<Vec<u32>>,

    /// ADR-141 P3: node-id → GPU index map, so `SetRadialLayout { Ego }` can resolve
    /// a focus node id to its buffer index. Empty until the first graph upload.
    radial_node_index: std::collections::HashMap<u32, usize>,

    /// Graph data waiting to be uploaded to GPU (set by InitializeGPU/UpdateGPUGraphData,
    /// consumed when shared_context becomes available)
    pending_graph_data: Option<Arc<visionclaw_domain::models::graph::GraphData>>,

    /// Back-channel to PhysicsOrchestratorActor for the sequential pipeline.
    /// When set, a PhysicsStepCompleted message is sent after each ComputeForces
    /// step, enabling the orchestrator to drive the next step instead of using
    /// an independent timer.
    physics_orchestrator_addr:
        Option<Addr<crate::actors::physics_orchestrator_actor::PhysicsOrchestratorActor>>,

    /// Number of GPU self-initialization attempts made so far.
    gpu_self_init_attempts: u32,
    /// Maximum number of GPU self-init retries before giving up.
    gpu_self_init_max_retries: u32,
    /// Timestamp of the last failed GPU self-init attempt (for exponential backoff).
    gpu_self_init_last_attempt: Option<Instant>,

    /// Consecutive divergent/bad frames seen (NaN/Inf, KE explosion). Reset to 0
    /// on any clean frame. When it reaches `MAX_CONSECUTIVE_BAD_FRAMES` the
    /// circuit breaker trips and stepping halts.
    consecutive_bad_frames: u32,

    /// Set once the circuit breaker has tripped. While true the actor stops
    /// running physics steps and never broadcasts; it must be re-armed
    /// (recovery) before stepping resumes.
    simulation_halted: bool,

    /// Last frame whose positions were finite and bounded. Reused as the
    /// broadcast payload when the current frame is bad, so clients never see
    /// infinity. Stored as (node_id, position, velocity-zeroed-on-recovery).
    last_good_positions: Vec<(u32, Vec3, Vec3)>,

    /// Consecutive physics ticks whose mean per-node kinetic energy stayed below
    /// `SETTLE_KE_EPSILON`. Reset to 0 the instant KE rises above epsilon (e.g. a
    /// springK reheat). Settlement is declared when this reaches
    /// `SETTLE_FRAME_THRESHOLD`. This is the honest settlement truth the REST
    /// `/api/graph/data` telemetry reports — not the hardcoded `!is_running`.
    settle_stable_frames: u32,

    /// Most recent mean per-node kinetic energy (`0.5·|v|²` averaged over nodes)
    /// from the physics tick loop. The live aggregate surfaced by settlement
    /// telemetry; 0.0 before the first tick.
    last_mean_kinetic_energy: f64,

    /// Set when the orchestrator signals the physics loop paused at convergence
    /// (energy plateau / auto-pause). While latched, telemetry reports settled
    /// with the measured-final KE — the loop has stopped calling
    /// `update_settlement`, so this is the only truthful "at rest" signal.
    /// Cleared by any real compute tick (motion resumed) or a resume signal.
    settle_paused: bool,
}

impl ForceComputeActor {
    pub fn new() -> Self {
        // Initialize broadcast optimizer with default config
        let broadcast_config = BroadcastConfig {
            target_fps: 10, // 10fps full snapshots — client tweens at 60fps
            enable_spatial_culling: false,
            camera_bounds: None,
        };

        // Initialize network backpressure with token bucket
        let backpressure_config = BackpressureConfig {
            max_tokens: 100,
            initial_tokens: 100,
            refill_rate_per_sec: 30.0, // Match target broadcast rate
            broadcast_cost: 1,
            ack_restore_tokens: 1,
            enable_time_refill: true,
            log_interval_frames: 60,
        };

        let initial_params = SimulationParams::default();
        info!(
            "ForceComputeActor::new() — initial params: dt={}, damping={}, repel_k={}, spring_k={}, center_gravity_k={}, max_force={}, max_velocity={}",
            initial_params.dt, initial_params.damping, initial_params.repel_k,
            initial_params.spring_k, initial_params.center_gravity_k,
            initial_params.max_force, initial_params.max_velocity
        );

        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            simulation_params: initial_params,
            unified_params: SimParams::default(),
            compute_mode: ComputeMode::Basic,
            last_step_start: None,
            last_step_duration_ms: 0.0,
            is_computing: false,
            skipped_frames: 0,
            reheat_factor: 0.0,
            stability_iterations: 0,
            // Start with warmup so the initial random layout converges while
            // broadcasting position updates.  Without this, the stability check
            // quickly declares equilibrium and stops physics before the graph has
            // time to spread out from its random initial positions.
            // 600 frames (~10s at 60fps) — edge-sparse graphs (e.g. 0 edges)
            // reach equilibrium quickly on repulsion+gravity alone and need more
            // runway before the stability kernel is allowed to halt physics.
            stability_warmup_remaining: 600,
            last_full_broadcast_iteration: 0,
            graph_service_addr: None,
            ontology_constraint_addr: None,
            cached_constraint_buffer: Vec::new(),
            semantic_forces_addr: None,
            broadcast_optimizer: BroadcastOptimizer::new(broadcast_config),
            suppress_intermediate_broadcasts: false,
            force_full_broadcast: false,
            backpressure: NetworkBackpressure::new(backpressure_config),
            position_velocity_buffer: Vec::with_capacity(10000),
            node_id_buffer: Vec::with_capacity(10000),
            gpu_index_to_node_id: Vec::new(),
            node_population: Vec::new(),
            dag_ranks: Vec::new(),
            graph_adjacency: Vec::new(),
            radial_node_index: std::collections::HashMap::new(),
            pinned_nodes: std::collections::HashMap::new(),
            pinned_mask_dirty: false,
            pending_graph_data: None,
            physics_orchestrator_addr: None,
            gpu_self_init_attempts: 0,
            gpu_self_init_max_retries: 3,
            gpu_self_init_last_attempt: None,
            consecutive_bad_frames: 0,
            simulation_halted: false,
            last_good_positions: Vec::new(),
            settle_stable_frames: 0,
            last_mean_kinetic_energy: 0.0,
            settle_paused: false,
        }
    }

    /// Mean per-node kinetic energy below which a tick counts toward settlement.
    /// Matches the physics `stability_threshold` default (1e-4) — a node barely
    /// drifting. Units: energy per node (`0.5·|v|²`).
    const SETTLE_KE_EPSILON: f64 = 1e-4;

    /// Consecutive sub-epsilon ticks required before the graph is declared
    /// settled. Mirrors the auto-balance `stabilityFrameCount` config (180
    /// frames ≈ 3 s at 60 fps) so REST and auto-balance agree on "settled".
    const SETTLE_FRAME_THRESHOLD: u32 = 180;

    /// Pure settlement-counter transition: the new stable-frame count given the
    /// previous count and this tick's mean per-node KE. A finite sub-epsilon KE
    /// increments the run; anything else (KE at/above epsilon, or a non-finite
    /// skip sentinel like `f64::MAX`) resets it to 0. Actor-free for testing.
    fn next_stable_frames(prev: u32, mean_ke: f64, epsilon: f64) -> u32 {
        if mean_ke.is_finite() && mean_ke < epsilon {
            prev.saturating_add(1)
        } else {
            0
        }
    }

    /// Pure settlement decision. Returns `(is_settled, reported_frame_count)`.
    ///
    /// Two truthful paths to "settled":
    /// - **Paused latch** — the orchestrator declared an energy plateau and
    ///   stopped the loop. Authoritative for this no-cooling FA2 layout, whose
    ///   residual energy never crosses an absolute floor. Reported frame count is
    ///   pinned to the threshold so the readout is self-consistent (settled ⇒
    ///   full rest count).
    /// - **Counter** — enough consecutive sub-epsilon ticks accumulated while the
    ///   loop was still running (covers cooled/quiescent layouts).
    ///
    /// Actor-free for testing.
    fn settled_state(paused: bool, frames: u32, threshold: u32) -> (bool, u32) {
        if paused {
            (true, threshold.max(frames))
        } else {
            (frames >= threshold, frames)
        }
    }

    /// Fold one tick's mean per-node KE into the settlement tracker. Called from
    /// the physics loop after `step_kinetic_energy` is computed. A real compute
    /// tick means the loop is running, so it clears any stale pause latch.
    fn update_settlement(&mut self, mean_ke: f64) {
        self.last_mean_kinetic_energy = mean_ke;
        self.settle_stable_frames =
            Self::next_stable_frames(self.settle_stable_frames, mean_ke, Self::SETTLE_KE_EPSILON);
        self.settle_paused = false;
    }

    /// Latch/unlatch settlement when the orchestrator pauses at convergence or
    /// resumes. On resume a fresh settle cycle begins, so the stale rest count is
    /// dropped; the mean KE is left intact for continuity until the first new
    /// tick overwrites it.
    fn set_settlement_paused(&mut self, paused: bool) {
        self.settle_paused = paused;
        if !paused {
            self.settle_stable_frames = 0;
        }
    }

    /// Build the live settlement telemetry snapshot from tracked state.
    fn settlement_snapshot(&self) -> crate::actors::messages::SettlementSnapshot {
        let (is_settled, stable_frame_count) = Self::settled_state(
            self.settle_paused,
            self.settle_stable_frames,
            Self::SETTLE_FRAME_THRESHOLD,
        );
        crate::actors::messages::SettlementSnapshot {
            is_settled,
            stable_frame_count,
            kinetic_energy: self.last_mean_kinetic_energy,
        }
    }

    /// Apply client node pins for the current frame (grab→spring support).
    ///
    /// For every pinned node: overwrite its entry in `position_velocity_buffer`
    /// with the client-supplied position (velocity zeroed) so the broadcast shows
    /// it locked to the hand, then re-upload the full position buffer to the GPU so
    /// the NEXT force step computes neighbour springs against the pinned location.
    /// Non-pinned nodes are re-uploaded with the positions the GPU just integrated
    /// (a no-op for them), so normal integration continues. Cheap host→device copy
    /// (~3·N f32); only runs while at least one node is actively pinned.
    /// Pure pin/unpin bookkeeping: apply a set of pins and unpins to a pinned-node
    /// map. Pins insert/update `node_id → position`; unpins remove. Returns true if
    /// the map changed (so the caller can mark the GPU mask dirty). Actor-free so
    /// the mask bookkeeping is unit-testable without a GPU context.
    fn apply_pin_ops(
        pinned: &mut std::collections::HashMap<u32, Vec3>,
        pins: &[(u32, [f32; 3])],
        unpin: &[u32],
    ) -> bool {
        let mut changed = false;
        for (id, p) in pins {
            let v = Vec3::new(p[0], p[1], p[2]);
            if pinned.insert(*id, v) != Some(v) {
                changed = true;
            }
        }
        for id in unpin {
            if pinned.remove(id).is_some() {
                changed = true;
            }
        }
        changed
    }

    /// Build the per-node GPU pinned mask (0 = free, 1 = pinned) aligned to GPU
    /// buffer indices. `node_ids[i]` is the graph node ID at GPU index `i`; the
    /// mask bit is set when that ID is in the pinned set. Pure/actor-free for tests.
    fn build_pinned_mask(
        node_ids: &[u32],
        pinned: &std::collections::HashMap<u32, Vec3>,
    ) -> Vec<i32> {
        node_ids
            .iter()
            .map(|id| if pinned.contains_key(id) { 1 } else { 0 })
            .collect()
    }

    /// Compute per-node DAG hierarchy ranks via cycle-safe multi-source BFS over
    /// the directed hierarchy edges (PHASE 2). `hierarchy_edges` are `(parent_idx,
    /// child_idx)` pairs (for `subClassOf`: parent = superclass = edge.target,
    /// child = subclass = edge.source). Roots (rank 0) are hierarchy nodes that
    /// are never a child; BFS assigns rank = depth from the nearest root. Back- and
    /// cross-edges are ignored via the visited set (cycle-safe). Nodes not in any
    /// hierarchy edge, or unreachable, get rank `-1.0` (no radial bias applied).
    ///
    /// If the hierarchy is non-empty but has no natural root (a pure cycle), the
    /// lowest-index participating node is seeded as a root so the layer assignment
    /// is deterministic rather than empty. Pure/actor-free for unit testing.
    /// The `(parent_idx, child_idx)` pairs the DAG ranker layers, mapped from
    /// graph node IDs to GPU indices. An edge whose endpoint is not on the GPU
    /// is skipped. Pure/actor-free for unit testing.
    ///
    /// Only class-subsumption edges qualify ([`Edge::asserts_subsumption`]):
    /// the `hierarchical` label is a force category the ingest also writes for
    /// equivalence, sameAs, sub-property and instance-of, and domain membership
    /// has its own label. Ranking any of those would fabricate layers (ADR-2035,
    /// amended 2026-10-02).
    ///
    /// [`Edge::asserts_subsumption`]: visionclaw_domain::models::edge::Edge::asserts_subsumption
    fn hierarchy_pairs(
        edges: &[visionclaw_domain::models::edge::Edge],
        node_indices: &std::collections::HashMap<u32, usize>,
    ) -> Vec<(usize, usize)> {
        edges
            .iter()
            .filter(|edge| edge.asserts_subsumption())
            .filter_map(|edge| {
                match (
                    node_indices.get(&edge.source),
                    node_indices.get(&edge.target),
                ) {
                    (Some(&child_idx), Some(&parent_idx)) => Some((parent_idx, child_idx)),
                    _ => None,
                }
            })
            .collect()
    }

    fn compute_dag_ranks(num_nodes: usize, hierarchy_edges: &[(usize, usize)]) -> Vec<f32> {
        let mut ranks = vec![-1.0f32; num_nodes];
        if num_nodes == 0 || hierarchy_edges.is_empty() {
            return ranks;
        }

        let mut children: Vec<Vec<usize>> = vec![Vec::new(); num_nodes];
        let mut is_child = vec![false; num_nodes];
        let mut in_hierarchy = vec![false; num_nodes];
        for &(parent, child) in hierarchy_edges {
            if parent >= num_nodes || child >= num_nodes {
                continue; // Defensive: ignore out-of-range indices.
            }
            children[parent].push(child);
            is_child[child] = true;
            in_hierarchy[parent] = true;
            in_hierarchy[child] = true;
        }

        // Roots = hierarchy participants that are never a child.
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        for i in 0..num_nodes {
            if in_hierarchy[i] && !is_child[i] {
                ranks[i] = 0.0;
                queue.push_back(i);
            }
        }

        // Pure-cycle fallback: no natural root but edges exist → seed the lowest
        // participating index so the layout still gets a deterministic layering.
        if queue.is_empty() {
            if let Some(seed) = (0..num_nodes).find(|&i| in_hierarchy[i]) {
                ranks[seed] = 0.0;
                queue.push_back(seed);
            }
        }

        // Multi-source BFS: first visit fixes the (shortest-depth) rank; the
        // visited guard (rank already set) skips back-/cross-edges → cycle-safe.
        while let Some(node) = queue.pop_front() {
            let next_rank = ranks[node] + 1.0;
            for &child in &children[node] {
                if ranks[child] < 0.0 {
                    ranks[child] = next_rank;
                    queue.push_back(child);
                }
            }
        }

        ranks
    }

    /// ADR-141 P3: BFS hop-distance from `focus` over the undirected `adjacency`
    /// (GPU index → neighbour GPU indices). Used as the radial shell KEY for
    /// RadialMode::Ego. The focus node gets distance `0.0`, each reachable node its
    /// hop count, and unreachable nodes `-1.0` (no radial force, matching the
    /// `dag_radial_bias` key < 0 convention). Pure/actor-free for unit testing.
    fn compute_ego_distances(num_nodes: usize, adjacency: &[Vec<u32>], focus: usize) -> Vec<f32> {
        let mut dist = vec![-1.0f32; num_nodes];
        if focus >= num_nodes {
            return dist;
        }
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        dist[focus] = 0.0;
        queue.push_back(focus);
        while let Some(node) = queue.pop_front() {
            let next = dist[node] + 1.0;
            if let Some(neighbours) = adjacency.get(node) {
                for &n in neighbours {
                    let n = n as usize;
                    if n < num_nodes && dist[n] < 0.0 {
                        dist[n] = next;
                        queue.push_back(n);
                    }
                }
            }
        }
        dist
    }

    /// Rebuild and upload the GPU pinned mask from `pinned_nodes` when dirty.
    /// Runs once per step (cheap: only rebuilds after a pin/unpin). Indexing
    /// mirrors `apply_node_pins` (`node_id_buffer[idx]` = GPU index idx), so the
    /// mask and the pinned-position rewrite always agree on which node is pinned.
    fn sync_pinned_mask(&mut self) {
        if !self.pinned_mask_dirty {
            return;
        }
        let Some(shared_context) = &self.shared_context else {
            return;
        };
        if self.node_id_buffer.is_empty() {
            return; // No graph uploaded yet; retry on a later step (stay dirty).
        }
        let flags = Self::build_pinned_mask(&self.node_id_buffer, &self.pinned_nodes);
        let mut unified = match shared_context.unified_compute.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match unified.upload_pinned_mask(&flags) {
            Ok(()) => {
                self.pinned_mask_dirty = false;
                debug!(
                    "ForceComputeActor: uploaded pinned mask ({} pinned / {} nodes)",
                    self.pinned_nodes.len(),
                    flags.len()
                );
            }
            Err(e) => warn!("ForceComputeActor: pinned mask upload failed: {}", e),
        }
    }

    fn apply_node_pins(&mut self) {
        if self.pinned_nodes.is_empty() {
            return;
        }

        let mut any = false;
        for idx in 0..self.node_id_buffer.len() {
            let id = self.node_id_buffer[idx];
            if let Some(pos) = self.pinned_nodes.get(&id).copied() {
                if idx < self.position_velocity_buffer.len() {
                    self.position_velocity_buffer[idx] = (pos, Vec3::ZERO);
                    any = true;
                }
            }
        }

        if !any {
            return;
        }

        // Re-upload the (pin-corrected) positions so the force kernel sees the pin.
        if let Some(shared_context) = &self.shared_context {
            let n = self.position_velocity_buffer.len();
            let mut xs = Vec::with_capacity(n);
            let mut ys = Vec::with_capacity(n);
            let mut zs = Vec::with_capacity(n);
            for (p, _v) in &self.position_velocity_buffer {
                xs.push(p.x);
                ys.push(p.y);
                zs.push(p.z);
            }
            let unified = match shared_context.unified_compute.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let mut unified = unified;
            if let Err(e) = unified.update_positions_only(&xs, &ys, &zs) {
                warn!("ForceComputeActor: pin re-upload failed: {}", e);
            }
        }
    }

    /// Self-initialize the GPU context by creating a CUDA device, loading PTX modules,
    /// and building a SharedGPUContext directly. This eliminates the dependency on the
    /// supervisor chain (GPUResourceActor -> GPUManagerActor -> ResourceSupervisor ->
    /// PhysicsSupervisor -> ForceComputeActor) which is prone to race conditions
    /// and message delivery failures.
    ///
    /// If a SharedGPUContext was already set (e.g., via SetSharedGPUContext from the
    /// supervisor chain), this method is a no-op.
    fn initialize_own_gpu_context(&mut self) {
        if self.shared_context.is_some() {
            trace!("ForceComputeActor: GPU context already present, skipping self-init");
            return;
        }

        // Check retry budget: if all retries exhausted, do not attempt again
        if self.gpu_self_init_attempts >= self.gpu_self_init_max_retries {
            trace!(
                "ForceComputeActor: GPU self-init exhausted all {} retries, skipping",
                self.gpu_self_init_max_retries
            );
            return;
        }

        // Exponential backoff: wait 2^(attempt-1) seconds between retries
        // (no delay on first attempt)
        if self.gpu_self_init_attempts > 0 {
            if let Some(last) = self.gpu_self_init_last_attempt {
                let backoff_secs = 1u64 << (self.gpu_self_init_attempts - 1); // 1s, 2s, 4s, ...
                if last.elapsed() < std::time::Duration::from_secs(backoff_secs) {
                    trace!(
                        "ForceComputeActor: GPU self-init backoff not elapsed (attempt {}, waiting {}s)",
                        self.gpu_self_init_attempts, backoff_secs
                    );
                    return;
                }
            }
        }

        self.gpu_self_init_attempts += 1;
        self.gpu_self_init_last_attempt = Some(Instant::now());

        info!(
            "ForceComputeActor: Self-initializing GPU context (attempt {}/{}, bypassing supervisor chain)",
            self.gpu_self_init_attempts, self.gpu_self_init_max_retries
        );

        // Helper macro-like closure to send GPUInitFailed on error and return early.
        // We capture the error reason inline at each failure point below.

        // 1. Create UnifiedGPUCompute engine FIRST — it initializes the cust CUDA context
        //    internally. Creating CudaDevice before this causes a dual-context conflict
        //    where Module::from_ptx() fails with "unknown error".
        let ptx_content = match visionclaw_gpu::ptx_loader::load_ptx_module_sync(
            visionclaw_gpu::ptx_loader::PTXModule::VisionflowUnified,
        ) {
            Ok(c) => c,
            Err(e) => {
                let reason = format!("Failed to load main PTX: {}", e);
                error!("ForceComputeActor: {}", reason);
                self.notify_gpu_init_failed(reason);
                return;
            }
        };

        let clustering_ptx = match visionclaw_gpu::ptx_loader::load_ptx_module_sync(
            visionclaw_gpu::ptx_loader::PTXModule::GpuClusteringKernels,
        ) {
            Ok(c) => Some(c),
            Err(e) => {
                warn!("ForceComputeActor: Clustering PTX not available: {}", e);
                None
            }
        };

        let apsp_ptx = match visionclaw_gpu::ptx_loader::load_ptx_module_sync(
            visionclaw_gpu::ptx_loader::PTXModule::GpuLandmarkApsp,
        ) {
            Ok(c) => Some(c),
            Err(e) => {
                warn!("ForceComputeActor: APSP PTX not available: {}", e);
                None
            }
        };

        // ADR-098 D3: the separate ontology_constraints.cu PTX is retired;
        // ontology constraints drive the generic live force_pass_kernel loop.

        let unified_compute =
            match crate::utils::unified_gpu_compute::UnifiedGPUCompute::new_with_all_modules(
                1000,
                1000,
                &ptx_content,
                clustering_ptx.as_deref(),
                apsp_ptx.as_deref(),
            ) {
                Ok(c) => {
                    info!("ForceComputeActor: UnifiedGPUCompute engine created successfully");
                    c
                }
                Err(e) => {
                    let reason = format!("Failed to create UnifiedGPUCompute: {}", e);
                    error!("ForceComputeActor: {}", reason);
                    self.notify_gpu_init_failed(reason);
                    return;
                }
            };

        // 2. Now create CudaDevice — attaches to the already-active primary context
        let device = match CudaDevice::new(0) {
            Ok(d) => {
                info!("ForceComputeActor: CUDA device 0 initialized");
                d
            }
            Err(e) => {
                let reason = format!("Failed to create CUDA device: {}", e);
                error!("ForceComputeActor: {}. GPU physics will not work.", reason);
                self.notify_gpu_init_failed(reason);
                return;
            }
        };

        // 3. Create CUDA stream from the device
        let cuda_stream = match device.fork_default_stream() {
            Ok(s) => s,
            Err(e) => {
                let reason = format!("Failed to create CUDA stream: {}", e);
                error!("ForceComputeActor: {}", reason);
                self.notify_gpu_init_failed(reason);
                return;
            }
        };

        // 4. Build SharedGPUContext
        let safe_stream = super::cuda_stream_wrapper::SafeCudaStream::new(cuda_stream);

        // Initialize GpuMemoryManager with 80% of reported GPU memory (or 6GB default)
        let memory_limit = match cudarc::driver::result::mem_get_info() {
            Ok((_free, total)) => {
                let limit = (total as f64 * 0.8) as usize;
                info!("ForceComputeActor: GPU total memory {} bytes, memory manager limit set to {} bytes (80%)", total, limit);
                limit
            }
            Err(e) => {
                warn!("ForceComputeActor: Could not query GPU memory info ({}), using 6GB default limit", e);
                6 * 1024 * 1024 * 1024
            }
        };
        let memory_manager =
            match crate::gpu::memory_manager::GpuMemoryManager::with_limit(memory_limit) {
                Ok(mgr) => {
                    info!(
                        "ForceComputeActor: GpuMemoryManager initialized with {} byte limit",
                        memory_limit
                    );
                    Arc::new(std::sync::Mutex::new(mgr))
                }
                Err(e) => {
                    warn!(
                    "ForceComputeActor: GpuMemoryManager init failed ({}), creating with default",
                    e
                );
                    match crate::gpu::memory_manager::GpuMemoryManager::new() {
                        Ok(mgr) => Arc::new(std::sync::Mutex::new(mgr)),
                        Err(e2) => {
                            let reason = format!("GpuMemoryManager completely failed: {}", e2);
                            error!("ForceComputeActor: {}", reason);
                            self.notify_gpu_init_failed(reason);
                            return;
                        }
                    }
                }
            };

        let shared_context = Arc::new(SharedGPUContext {
            device: device.clone(),
            stream: Arc::new(std::sync::Mutex::new(safe_stream)),
            unified_compute: Arc::new(std::sync::Mutex::new(unified_compute)),
            memory_manager,
            gpu_access_lock: Arc::new(tokio::sync::RwLock::new(())),
            resource_metrics: Arc::new(std::sync::Mutex::new(
                super::shared::GPUResourceMetrics::default(),
            )),
            operation_batch: Arc::new(std::sync::Mutex::new(Vec::new())),
            batch_timeout: std::time::Duration::from_millis(10),
        });

        self.shared_context = Some(shared_context);
        self.gpu_state.is_initialized = true;
        info!("ForceComputeActor: GPU context self-initialized successfully — GPU physics enabled");
    }

    /// Send GPUInitFailed to the physics orchestrator if all retries are exhausted.
    /// On intermediate failures (retries remaining), only logs — the next call to
    /// initialize_own_gpu_context() will retry after the backoff period.
    fn notify_gpu_init_failed(&self, reason: String) {
        if self.gpu_self_init_attempts < self.gpu_self_init_max_retries {
            warn!(
                "ForceComputeActor: GPU init attempt {}/{} failed ({}), will retry after backoff",
                self.gpu_self_init_attempts, self.gpu_self_init_max_retries, reason
            );
            return;
        }
        error!(
            "ForceComputeActor: GPU init PERMANENTLY failed after {} attempts: {}",
            self.gpu_self_init_attempts, reason
        );
        if let Some(ref orchestrator_addr) = self.physics_orchestrator_addr {
            orchestrator_addr.do_send(crate::actors::messages::GPUInitFailed {
                reason,
                attempts: self.gpu_self_init_attempts,
            });
            info!("ForceComputeActor: GPUInitFailed sent to PhysicsOrchestratorActor");
        } else {
            warn!("ForceComputeActor: No orchestrator address — cannot send GPUInitFailed");
        }
    }

    /// Upload pending graph data to the GPU compute engine.
    /// Called when both shared_context and pending_graph_data become available.
    fn try_upload_pending_graph_data(&mut self) {
        let (Some(ref ctx), Some(ref graph_data)) =
            (&self.shared_context, &self.pending_graph_data)
        else {
            return;
        };

        let num_nodes = graph_data.nodes.len();
        let num_edges = graph_data.edges.len();
        if num_nodes == 0 {
            warn!("ForceComputeActor: Skipping graph upload — 0 nodes");
            return;
        }

        info!(
            "ForceComputeActor: Uploading {} nodes, {} edges to GPU",
            num_nodes, num_edges
        );

        // Build CSR representation, GPU-index-to-node-ID mapping, and population classification
        let mut node_indices = std::collections::HashMap::new();
        self.gpu_index_to_node_id = Vec::with_capacity(num_nodes);
        self.node_population = Vec::with_capacity(num_nodes);
        let mut pop_counts = [0usize; 3]; // [knowledge, ontology, agent]
        for (i, node) in graph_data.nodes.iter().enumerate() {
            node_indices.insert(node.id, i);
            // Use compact wire ID (= GPU index) instead of persistent store ID.
            // This keeps IDs within 26 bits so binary protocol type flags
            // in bits 26-31 don't collide with real node IDs.
            self.gpu_index_to_node_id.push(i as u32);

            // Classify node into graph population for dual-graph X-axis separation.
            // SINGLE SOURCE OF TRUTH: Node::population() reads the authoritative
            // metadata["type"] origin field (node_type is non-classifying elevation
            // scaffold, consulted only as a legacy fallback). Centralised in
            // visionclaw_domain so every reader agrees on a node's population.
            let pop = GraphPopulation::from(node.population());
            match pop {
                GraphPopulation::Knowledge => pop_counts[0] += 1,
                GraphPopulation::Ontology => pop_counts[1] += 1,
                GraphPopulation::Agent => pop_counts[2] += 1,
            }
            self.node_population.push(pop);
        }
        debug!(
            "ForceComputeActor: GPU index→wire_id mapping: 0..{} ({} entries, compact IDs)",
            self.gpu_index_to_node_id.len().saturating_sub(1),
            self.gpu_index_to_node_id.len()
        );
        debug!(
            "ForceComputeActor: Node populations — knowledge: {}, ontology: {}, agent: {}",
            pop_counts[0], pop_counts[1], pop_counts[2]
        );

        // ADR-141 P3: retain node-id → GPU-index map for on-demand radial re-keying
        // (SetRadialLayout resolves a focus node id to its buffer index via this).
        self.radial_node_index = node_indices.clone();

        let mut positions_x: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.x).collect();
        let mut positions_y: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.y).collect();
        let mut positions_z: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.z).collect();

        let mut adjacency_lists: Vec<Vec<(u32, f32)>> = vec![Vec::new(); num_nodes];
        for edge in &graph_data.edges {
            if let (Some(&src), Some(&tgt)) = (
                node_indices.get(&edge.source),
                node_indices.get(&edge.target),
            ) {
                adjacency_lists[src].push((tgt as u32, edge.weight));
                if src != tgt {
                    adjacency_lists[tgt].push((src as u32, edge.weight));
                }
            }
        }

        // ADR-141 P3: retain the undirected neighbour indices for the
        // RadialMode::Ego BFS hop-distance (weightless projection of adjacency_lists).
        self.graph_adjacency = adjacency_lists
            .iter()
            .map(|adj| adj.iter().map(|&(t, _w)| t).collect())
            .collect();

        let mut row_offsets = vec![0u32; num_nodes + 1];
        let mut col_indices = Vec::new();
        let mut edge_weights = Vec::new();
        let mut edge_count = 0u32;
        for (i, adj) in adjacency_lists.iter().enumerate() {
            row_offsets[i] = edge_count;
            for &(target, weight) in adj {
                col_indices.push(target);
                edge_weights.push(weight);
                edge_count += 1;
            }
        }
        row_offsets[num_nodes] = edge_count;

        // Place isolated nodes (degree 0) on a spherical shell so they don't
        // clump in the center and obscure community structure of connected nodes.
        // The shell radius is set to 2x the average connected-node distance from origin.
        {
            use rand::Rng;
            let mut rng = rand::thread_rng();

            // Compute average distance of connected nodes from origin
            let mut sum_dist = 0.0f64;
            let mut connected_count = 0usize;
            for (i, adj) in adjacency_lists.iter().enumerate() {
                if !adj.is_empty() {
                    let dx = positions_x[i] as f64;
                    let dy = positions_y[i] as f64;
                    let dz = positions_z[i] as f64;
                    sum_dist += (dx * dx + dy * dy + dz * dz).sqrt();
                    connected_count += 1;
                }
            }
            let avg_dist = if connected_count > 0 {
                sum_dist / connected_count as f64
            } else {
                100.0 // fallback if everything is isolated
            };
            let shell_radius = (avg_dist * 2.0).max(200.0) as f32;

            let mut isolated_count = 0usize;
            for (i, adj) in adjacency_lists.iter().enumerate() {
                if adj.is_empty() {
                    // Fibonacci sphere distribution for even spacing
                    let golden_ratio = (1.0 + 5.0f32.sqrt()) / 2.0;
                    let theta = 2.0 * std::f32::consts::PI * (i as f32) / golden_ratio;
                    let phi = (1.0 - 2.0 * (i as f32 + 0.5) / num_nodes as f32).acos();
                    // Add small random jitter to prevent perfect lattice artifacts
                    let r = shell_radius * (1.0 + rng.gen_range(-0.05f32..0.05f32));
                    positions_x[i] = r * phi.sin() * theta.cos();
                    positions_y[i] = r * phi.sin() * theta.sin();
                    positions_z[i] = r * phi.cos();
                    isolated_count += 1;
                }
            }
            if isolated_count > 0 {
                info!(
                    "ForceComputeActor: Placed {} isolated nodes on spherical shell (radius={:.1})",
                    isolated_count, shell_radius
                );
            }
        }

        // Upload to GPU via shared context (recover from poisoned mutex if needed)
        let mut compute = match ctx.unified_compute.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                error!("ForceComputeActor: GPU mutex was POISONED — a previous GPU operation panicked. Recovering for graph upload, but GPU state may be corrupt.");
                poisoned.into_inner()
            }
        };
        match compute.initialize_graph(
            row_offsets.iter().map(|&x| x as i32).collect(),
            col_indices.iter().map(|&x| x as i32).collect(),
            edge_weights,
            positions_x,
            positions_y,
            positions_z,
            num_nodes,
            edge_count as usize,
        ) {
            Ok(_) => {
                info!("ForceComputeActor: Graph data uploaded to GPU successfully ({} nodes, {} CSR edges)", num_nodes, edge_count);

                // Upload node_graph_id mapping so ClusteringActor's ensure_node_id_map
                // can download it.  With compact IDs (node.id == sequential 0..N-1),
                // this is an identity mapping, but initialize_graph's resize_buffers
                // zeroes the buffer so we must re-upload it explicitly.
                debug!("ForceComputeActor: [DIAG] About to upload node_graph_id ({} entries, buffer len {})",
                      self.gpu_index_to_node_id.len(), compute.node_graph_id.len());
                let mut node_graph_ids: Vec<i32> = self
                    .gpu_index_to_node_id
                    .iter()
                    .map(|&id| id as i32)
                    .collect();
                if !node_graph_ids.is_empty() {
                    use cust::memory::CopyDestination;
                    // Pad to allocated_nodes since device buffer may be overallocated
                    if node_graph_ids.len() < compute.node_graph_id.len() {
                        node_graph_ids.resize(compute.node_graph_id.len(), 0);
                    }
                    if let Err(e) = compute.node_graph_id.copy_from(&node_graph_ids) {
                        error!(
                            "ForceComputeActor: Failed to upload node_graph_id buffer: {}",
                            e
                        );
                    } else {
                        debug!(
                            "ForceComputeActor: Uploaded node_graph_id mapping ({} entries)",
                            node_graph_ids.len()
                        );
                    }
                }
                debug!(
                    "ForceComputeActor: [DIAG] node_graph_id done, about to upload class metadata"
                );

                // Upload domain-based class_id and class_charge for domain clustering.
                if let Some(ref graph_data) = self.pending_graph_data {
                    let mut class_ids = Vec::with_capacity(num_nodes);
                    let mut class_charges = Vec::with_capacity(num_nodes);
                    let class_masses = vec![1.0f32; num_nodes];

                    for node in &graph_data.nodes {
                        let domain = node
                            .metadata
                            .get("source_domain")
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        let id = vault_core::domains::domain_class_id(domain);
                        let charge = if id == 0 { 1.2f32 } else { 0.6f32 };
                        class_ids.push(id);
                        class_charges.push(charge);
                    }

                    if let Err(e) =
                        compute.upload_class_metadata(&class_ids, &class_charges, &class_masses)
                    {
                        warn!("ForceComputeActor: Failed to upload class metadata: {}", e);
                    } else {
                        debug!(
                            "ForceComputeActor: Uploaded class metadata ({} entries)",
                            class_ids.len()
                        );
                    }
                }

                // Per-population spring multipliers → spring_scale buffer. Maps each
                // node's classified population to its independent spring strength so
                // the Knowledge/Ontology/Agent sliders steer layout directly (live in
                // both LinLog and Hooke kernel paths). node_population is populated
                // earlier in this same upload pass.
                if self.node_population.len() == num_nodes {
                    let k = self.simulation_params.spring_k_knowledge;
                    let o = self.simulation_params.spring_k_ontology;
                    let a = self.simulation_params.spring_k_agent;
                    let spring_scales: Vec<f32> = self
                        .node_population
                        .iter()
                        .map(|pop| match pop {
                            GraphPopulation::Knowledge => k,
                            GraphPopulation::Ontology => o,
                            GraphPopulation::Agent => a,
                        })
                        .collect();
                    if let Err(e) = compute.upload_spring_scale(&spring_scales) {
                        warn!("ForceComputeActor: Failed to upload spring_scale: {}", e);
                    } else {
                        debug!(
                            "ForceComputeActor: Uploaded spring_scale (k={:.2} o={:.2} a={:.2}, {} nodes)",
                            k, o, a, spring_scales.len()
                        );
                    }
                }

                // PHASE 2: compute + upload per-node DAG hierarchy ranks for the
                // radial bias force. Ranks come from a cycle-safe BFS over the
                // directed subClassOf-family (Hierarchical) edges: parent =
                // superclass = edge.target, child = subclass = edge.source. Node
                // IDs are mapped to GPU indices via node_indices. Namespace edges
                // are excluded — they group by shared prefix and carry no
                // parent/child direction, so they cannot define a rank. The bias
                // itself stays inert until dagBiasK > 0, so uploading ranks
                // unconditionally is safe and keeps them ready for a later toggle.
                if let Some(ref graph_data) = self.pending_graph_data {
                    let hierarchy_edges = Self::hierarchy_pairs(&graph_data.edges, &node_indices);
                    if !hierarchy_edges.is_empty() {
                        let ranks = Self::compute_dag_ranks(num_nodes, &hierarchy_edges);
                        let ranked = ranks.iter().filter(|&&r| r >= 0.0).count();
                        let max_rank = ranks.iter().cloned().fold(-1.0f32, f32::max);
                        // ADR-141 P3: cache ranks so SetRadialLayout { DagRank } can
                        // re-key on demand without recomputing.
                        self.dag_ranks = ranks.clone();
                        match compute.upload_node_rank(&ranks) {
                            Ok(()) => info!(
                                "ForceComputeActor: Uploaded DAG ranks — {} hierarchy edges, {} ranked nodes, max rank {}",
                                hierarchy_edges.len(), ranked, max_rank
                            ),
                            Err(e) => warn!("ForceComputeActor: DAG rank upload failed: {}", e),
                        }
                    } else {
                        // No hierarchy → all-unranked cache so a later DagRank re-key
                        // is inert rather than reading a stale prior-graph vec.
                        self.dag_ranks = vec![-1.0; num_nodes];
                        debug!("ForceComputeActor: No hierarchy edges — DAG ranks left at -1 (bias inert)");
                    }
                }

                // ADR-141 P2: compute + upload per-node centered plane offsets for
                // the stratified-plane bias. Each node's population maps to a target
                // Z-plane (Knowledge = -1, Ontology = 0, Agent = +1), so node types
                // separate into parallel horizontal strata. Unknown population = NaN
                // (no force). The bias stays inert until planeBiasK > 0, so uploading
                // planes unconditionally is safe and keeps them ready for a later toggle.
                if self.node_population.len() == num_nodes {
                    let planes: Vec<f32> = self
                        .node_population
                        .iter()
                        .map(|pop| match pop {
                            GraphPopulation::Knowledge => -1.0,
                            GraphPopulation::Ontology => 0.0,
                            GraphPopulation::Agent => 1.0,
                        })
                        .collect();
                    match compute.upload_node_plane(&planes) {
                        Ok(()) => info!(
                            "ForceComputeActor: Uploaded stratified planes for {} nodes",
                            planes.len()
                        ),
                        Err(e) => warn!("ForceComputeActor: node plane upload failed: {}", e),
                    }
                } else {
                    debug!("ForceComputeActor: node_population len mismatch — planes left at NaN (bias inert)");
                }

                // Compute and upload degree weights for degree-weighted gravity.
                // degree_weight[i] = log(1 + degree[i]), where degree is computed
                // from the CSR row_offsets. This causes hubs to be pulled toward
                // the center more strongly and isolates (degree 0) to receive
                // peripheral shell forces instead of uniform centering.
                {
                    let degree_weights: Vec<f32> = (0..num_nodes)
                        .map(|i| {
                            let start = row_offsets[i] as usize;
                            let end = row_offsets[i + 1] as usize;
                            let degree = end - start;
                            (1.0f32 + degree as f32).ln()
                        })
                        .collect();

                    // Normalize so the median-degree node gets weight ~1.0
                    // This preserves the overall gravity magnitude while redistributing it
                    let mut sorted_weights: Vec<f32> = degree_weights
                        .iter()
                        .copied()
                        .filter(|&w| w > 1e-6)
                        .collect();
                    sorted_weights
                        .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let median_weight = if sorted_weights.is_empty() {
                        1.0f32
                    } else {
                        sorted_weights[sorted_weights.len() / 2]
                    };
                    let norm_factor = if median_weight > 1e-6 {
                        1.0 / median_weight
                    } else {
                        1.0
                    };

                    let normalized_weights: Vec<f32> = degree_weights
                        .iter()
                        .map(|&w| if w < 1e-6 { 0.0 } else { w * norm_factor })
                        .collect();

                    let isolated_count = normalized_weights.iter().filter(|&&w| w < 1e-6).count();
                    info!(
                        "ForceComputeActor: Degree weights computed — {} isolated nodes, median_weight={:.3}, norm_factor={:.3}",
                        isolated_count, median_weight, norm_factor
                    );

                    if let Err(e) = compute.upload_degree_weights(&normalized_weights) {
                        warn!("ForceComputeActor: Failed to upload degree weights: {}", e);
                    }

                    // Also set class_mass = degree_weight so high-degree hubs have
                    // more inertia — they resist sudden position changes during layout
                    // transitions and settle more smoothly. Mass range: 0.5 (isolated)
                    // to ~5.0 (max hub), clamped to prevent extreme sluggishness.
                    let mass_weights: Vec<f32> = normalized_weights
                        .iter()
                        .map(|w| (0.5 + w * 2.0).min(5.0))
                        .collect();
                    // Pad to allocated_nodes (= compute.class_id.len()) so the
                    // replacement DeviceBuffer stays the same size as class_id and
                    // class_charge. Without padding, from_slice creates a buffer of
                    // exactly num_nodes elements; the next upload_class_metadata call
                    // then tries to copy a padded (allocated_nodes) slice into this
                    // smaller buffer and panics with checked_copy_from size mismatch.
                    let allocated = compute.class_id.len();
                    let mut padded_mass = vec![1.0f32; allocated];
                    let copy_len = mass_weights.len().min(allocated);
                    padded_mass[..copy_len].copy_from_slice(&mass_weights[..copy_len]);
                    match cust::memory::DeviceBuffer::from_slice(&padded_mass) {
                        Ok(new_mass) => {
                            compute.class_mass = new_mass;
                        }
                        Err(e) => {
                            warn!("ForceComputeActor: Failed to upload mass weights: {}", e);
                        }
                    }
                }

                debug!("ForceComputeActor: [DIAG] class metadata done, about to update gpu_state");
                let was_uninitialized = self.gpu_state.num_nodes == 0;
                self.gpu_state.num_nodes = num_nodes as u32;
                self.gpu_state.num_edges = edge_count;
                self.pending_graph_data = None;

                // initialize_graph may have resized the GPU buffers, which zeroes
                // the pinned_mask. Re-upload it SYNCHRONOUSLY here — while the
                // `compute` lock is still held and BEFORE any subsequent physics
                // launch — using the authoritative gpu_index_to_node_id map just
                // rebuilt at the top of this upload. Deferring to the post-step
                // sync_pinned_mask would let the first post-resize step integrate
                // pinned nodes against a zeroed mask (they would move one frame).
                // On failure, fall back to the dirty-flag path.
                if !self.pinned_nodes.is_empty() {
                    let flags =
                        Self::build_pinned_mask(&self.gpu_index_to_node_id, &self.pinned_nodes);
                    match compute.upload_pinned_mask(&flags) {
                        Ok(()) => {
                            self.pinned_mask_dirty = false;
                            debug!(
                                "ForceComputeActor: re-uploaded pinned mask after graph upload ({} pinned)",
                                self.pinned_nodes.len()
                            );
                        }
                        Err(e) => {
                            warn!(
                                "ForceComputeActor: post-upload pinned mask sync failed ({}), deferring to step-loop sync",
                                e
                            );
                            self.pinned_mask_dirty = true;
                        }
                    }
                }

                // Fresh graph data needs a full warmup window so the layout can
                // converge before the GPU stability kernel is allowed to halt
                // physics.  Edge-sparse graphs (0 edges → only repulsion + gravity)
                // reach equilibrium extremely fast; give them extra runway.
                let warmup = if edge_count == 0 { 1200 } else { 600 };
                self.stability_warmup_remaining = warmup;
                self.broadcast_optimizer.reset_broadcast_timer();
                debug!("ForceComputeActor: Stability warmup reset to {} frames after graph upload ({} edges)",
                      warmup, edge_count);

                // ADR-031: Track GPU buffer allocations in GpuMemoryManager
                // so it knows current memory usage. Positions+velocities use
                // 12 f32 buffers (6 in, 6 out) of actual_nodes * 4 bytes each.
                // CSR edges use (num_nodes+1 + num_edges) * 4 bytes for offsets
                // + col_indices, plus num_edges * 4 for weights.
                if let Some(ref gpu_ctx) = self.shared_context {
                    if let Ok(mgr) = gpu_ctx.memory_manager.lock() {
                        let pos_vel_bytes = num_nodes * std::mem::size_of::<f32>() * 12;
                        let csr_bytes = ((num_nodes + 1) + edge_count as usize)
                            * std::mem::size_of::<i32>()
                            + edge_count as usize * std::mem::size_of::<f32>();
                        mgr.track_external_allocation("positions", pos_vel_bytes);
                        mgr.track_external_allocation("edges_csr", csr_bytes);
                        debug!("ForceComputeActor: Tracked GPU allocations — positions: {} bytes, CSR: {} bytes",
                              pos_vel_bytes, csr_bytes);
                    }
                }

                // If this is the first successful upload (deferred from InitializeGPU
                // because shared_context wasn't available yet), send the GPUInitialized
                // confirmation now so PhysicsOrchestratorActor can start the pipeline.
                if was_uninitialized {
                    if let Some(ref orchestrator_addr) = self.physics_orchestrator_addr {
                        orchestrator_addr.do_send(crate::actors::messages::GPUInitialized);
                        debug!("ForceComputeActor: Deferred GPUInitialized confirmation sent after successful graph upload");
                    }
                }
            }
            Err(e) => {
                error!("ForceComputeActor: Failed to upload graph to GPU: {}", e);
            }
        }
    }

    fn sync_simulation_to_unified_params(&self, unified_params: &mut SimParams) {
        unified_params.spring_k = self.simulation_params.spring_k;
        unified_params.repel_k = self.simulation_params.repel_k;
        unified_params.damping = self.simulation_params.damping;
        unified_params.dt = self.simulation_params.dt;
        unified_params.max_velocity = self.simulation_params.max_velocity;
        unified_params.center_gravity_k = self.simulation_params.center_gravity_k;

        match self.compute_mode {
            ComputeMode::Basic => {}
            ComputeMode::Advanced => {
                unified_params.temperature = self.simulation_params.temperature;
                unified_params.alignment_strength = self.simulation_params.alignment_strength;
                unified_params.cluster_strength = self.simulation_params.cluster_strength;
            }
            ComputeMode::DualGraph => {
                unified_params.temperature = self.simulation_params.temperature;
                unified_params.alignment_strength = self.simulation_params.alignment_strength;
                unified_params.cluster_strength = self.simulation_params.cluster_strength;
            }
            ComputeMode::Constraints => {
                unified_params.temperature = self.simulation_params.temperature;
                unified_params.alignment_strength = self.simulation_params.alignment_strength;
                unified_params.cluster_strength = self.simulation_params.cluster_strength;
                unified_params.constraint_ramp_frames =
                    self.simulation_params.constraint_ramp_frames;
                unified_params.constraint_max_force_per_node =
                    self.simulation_params.constraint_max_force_per_node;
            }
        }

        trace!("Unified params updated: spring_k={:.3}, repel_k={:.3}, center_gravity_k={:.3}, damping={:.3}",
               unified_params.spring_k, unified_params.repel_k, unified_params.center_gravity_k, unified_params.damping);
    }

    fn iteration_count(&self) -> u32 {
        self.gpu_state.iteration_count
    }

    fn update_simulation_parameters(&mut self, params: SimulationParams) {
        debug!("ForceComputeActor: Updating simulation parameters");
        info!(
            "  spring_k: {:.3} -> {:.3}",
            self.simulation_params.spring_k, params.spring_k
        );
        info!(
            "  repel_k: {:.3} -> {:.3}",
            self.simulation_params.repel_k, params.repel_k
        );
        info!(
            "  damping: {:.3} -> {:.3}",
            self.simulation_params.damping, params.damping
        );
        info!(
            "  center_gravity_k: {:.3} -> {:.3}",
            self.simulation_params.center_gravity_k, params.center_gravity_k
        );
        info!(
            "  cluster_strength: {:.3} -> {:.3}",
            self.simulation_params.cluster_strength, params.cluster_strength
        );
        info!(
            "  alignment_strength: {:.3} -> {:.3}",
            self.simulation_params.alignment_strength, params.alignment_strength
        );
        info!(
            "  temperature: {:.4} -> {:.4}",
            self.simulation_params.temperature, params.temperature
        );

        self.simulation_params = params;

        // Sync ALL GPU-relevant fields to unified_params
        {
            let unified_params = &mut self.unified_params;
            unified_params.spring_k = self.simulation_params.spring_k;
            unified_params.repel_k = self.simulation_params.repel_k;
            unified_params.damping = self.simulation_params.damping;
            unified_params.dt = self.simulation_params.dt;
            unified_params.max_velocity = self.simulation_params.max_velocity;
            unified_params.max_force = self.simulation_params.max_force;
            unified_params.center_gravity_k = self.simulation_params.center_gravity_k;
            unified_params.temperature = self.simulation_params.temperature;
            unified_params.cluster_strength = self.simulation_params.cluster_strength;
            unified_params.alignment_strength = self.simulation_params.alignment_strength;
            unified_params.separation_radius = self.simulation_params.separation_radius;
            unified_params.cooling_rate = self.simulation_params.cooling_rate;
            unified_params.warmup_iterations = self.simulation_params.warmup_iterations;
            unified_params.viewport_bounds = self.simulation_params.viewport_bounds;
            unified_params.boundary_damping = self.simulation_params.boundary_damping;
            unified_params.constraint_ramp_frames = self.simulation_params.constraint_ramp_frames;
            unified_params.constraint_max_force_per_node =
                self.simulation_params.constraint_max_force_per_node;
            // Rebuild feature flags from current params
            let new_sim_params = self.simulation_params.to_sim_params();
            unified_params.feature_flags = new_sim_params.feature_flags;
            if let Some(alpha) = self.simulation_params.sssp_alpha {
                unified_params.sssp_alpha = alpha;
            }
        }
    }

    fn get_physics_stats(&self) -> PhysicsStats {
        let (average_velocity, kinetic_energy, total_forces) = self.calculate_physics_metrics();

        let fps = if self.last_step_duration_ms > 0.0 {
            1000.0 / self.last_step_duration_ms
        } else {
            0.0
        };

        PhysicsStats {
            iteration_count: self.gpu_state.iteration_count,
            gpu_failure_count: self.gpu_state.gpu_failure_count,
            current_params: self.simulation_params.clone(),
            compute_mode: self.compute_mode,
            nodes_count: self.gpu_state.num_nodes,
            edges_count: self.gpu_state.num_edges,

            average_velocity,
            kinetic_energy,
            total_forces,

            last_step_duration_ms: self.last_step_duration_ms,
            fps,

            num_edges: self.gpu_state.num_edges,
            total_force_calculations: self.gpu_state.iteration_count * self.gpu_state.num_nodes,
        }
    }

    /// Calculate physics metrics from GPU state
    /// Uses try_lock() to avoid blocking Tokio threads - returns estimates if GPU is busy
    fn calculate_physics_metrics(&self) -> (f32, f32, f32) {
        // Use try_lock() to avoid blocking - if GPU is busy, return estimates
        if let Some(ctx) = &self.shared_context {
            if let Ok(unified_compute) = ctx.unified_compute.try_lock() {
                return self.extract_gpu_metrics(&unified_compute);
            }
            // GPU mutex busy, fall through to estimates
        }

        // Return estimates when GPU access not available
        let estimated_velocity = self.simulation_params.max_velocity * 0.3;
        let estimated_kinetic_energy =
            0.5 * (self.gpu_state.num_nodes as f32) * estimated_velocity.powi(2);
        let estimated_total_forces =
            self.simulation_params.spring_k * (self.gpu_state.num_edges as f32) * 0.5;

        (
            estimated_velocity,
            estimated_kinetic_energy,
            estimated_total_forces,
        )
    }

    fn extract_gpu_metrics(
        &self,
        unified_compute: &crate::utils::unified_gpu_compute::UnifiedGPUCompute,
    ) -> (f32, f32, f32) {
        let num_nodes = unified_compute.num_nodes;

        let mut vel_x = vec![0.0f32; num_nodes];
        let mut vel_y = vec![0.0f32; num_nodes];
        let mut vel_z = vec![0.0f32; num_nodes];

        if unified_compute
            .download_velocities(&mut vel_x, &mut vel_y, &mut vel_z)
            .is_ok()
        {
            let total_velocity: f32 = vel_x
                .iter()
                .zip(&vel_y)
                .zip(&vel_z)
                .map(|((vx, vy), vz)| (vx * vx + vy * vy + vz * vz).sqrt())
                .sum();
            let average_velocity = if num_nodes > 0 {
                total_velocity / num_nodes as f32
            } else {
                0.0
            };

            let kinetic_energy: f32 = vel_x
                .iter()
                .zip(&vel_y)
                .zip(&vel_z)
                .map(|((vx, vy), vz)| 0.5 * (vx * vx + vy * vy + vz * vz))
                .sum();

            let estimated_total_forces =
                total_velocity * self.simulation_params.damping * num_nodes as f32;

            (average_velocity, kinetic_energy, estimated_total_forces)
        } else {
            let estimated_velocity = self.simulation_params.max_velocity * 0.3;
            let estimated_kinetic_energy = 0.5 * (num_nodes as f32) * estimated_velocity.powi(2);
            let estimated_total_forces =
                self.simulation_params.spring_k * (self.gpu_state.num_edges as f32) * 0.5;

            (
                estimated_velocity,
                estimated_kinetic_energy,
                estimated_total_forces,
            )
        }
    }

    fn calculate_gpu_utilization(&self, execution_time_ms: f64) -> f32 {
        const TARGET_FRAME_TIME_MS: f64 = 16.67;

        let utilization_percent = (execution_time_ms / TARGET_FRAME_TIME_MS * 100.0) as f32;

        utilization_percent.clamp(0.0, 100.0)
    }

    /// Recover from a tripped divergence circuit breaker.
    ///
    /// Drains the runaway kinetic energy (zero all GPU velocities) and restores
    /// the last-known-good positions to the GPU so the simulation can re-settle
    /// from a sane layout instead of re-exploding. Uses `try_lock` to avoid
    /// blocking the Tokio worker; if the GPU mutex is held this frame, recovery
    /// is a no-op and will be retried (the breaker stays tripped until then).
    ///
    /// This does NOT clear `simulation_halted` — re-arming the simulation is an
    /// explicit operator/recovery action (a parameter change resets the breaker
    /// via `clear_divergence_state`), so a freshly-recovered-but-not-re-armed
    /// graph stays frozen at its good positions and never re-broadcasts garbage.
    fn recover_from_divergence(&mut self) {
        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                warn!("ForceComputeActor: recover_from_divergence called with no GPU context");
                return;
            }
        };

        let mut unified_compute = match shared_context.unified_compute.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                warn!("ForceComputeActor: GPU mutex busy during divergence recovery — will retry");
                return;
            }
        };

        // 1. Drain kinetic energy: zero all velocities.
        if let Err(e) = unified_compute.reset_velocities() {
            error!(
                "ForceComputeActor: failed to zero velocities during recovery: {}",
                e
            );
        } else {
            info!("ForceComputeActor: divergence recovery — velocities zeroed");
        }

        // 2. Restore last-known-good positions if the GPU buffer size matches.
        if !self.last_good_positions.is_empty() {
            let n = self.last_good_positions.len();
            let mut px = Vec::with_capacity(n);
            let mut py = Vec::with_capacity(n);
            let mut pz = Vec::with_capacity(n);
            for (_id, pos, _vel) in self.last_good_positions.iter() {
                px.push(pos.x);
                py.push(pos.y);
                pz.push(pos.z);
            }
            match unified_compute.upload_positions(&px, &py, &pz) {
                Ok(()) => info!(
                    "ForceComputeActor: divergence recovery — restored {} last-known-good positions",
                    n
                ),
                Err(e) => warn!(
                    "ForceComputeActor: could not restore last-good positions during recovery \
                     (likely a node-count change since last good frame): {}",
                    e
                ),
            }
        }
    }

    /// Clear divergence/circuit-breaker state and re-arm the simulation.
    /// Called on recovery triggers (e.g. a parameter change) so a previously
    /// halted layout resumes stepping from its restored good positions.
    fn clear_divergence_state(&mut self) {
        if self.simulation_halted || self.consecutive_bad_frames > 0 {
            info!(
                "ForceComputeActor: re-arming simulation (was halted={}, bad_frames={})",
                self.simulation_halted, self.consecutive_bad_frames
            );
        }
        self.simulation_halted = false;
        self.consecutive_bad_frames = 0;
    }

    /// Apply ontology-derived constraint forces to the physics simulation
    /// This method integrates ontology constraints from the OntologyConstraintActor
    /// into the physics pipeline, enabling semantic relationships to influence node positions.
    /// # Implementation Notes
    /// This is the final integration point for P0-2 ontology constraints. It:
    /// 1. Retrieves constraint buffer from OntologyConstraintActor (via shared memory/coordination)
    /// 2. Uploads constraints to GPU via UnifiedGPUCompute::upload_constraints()
    /// 3. Constraints are automatically applied during execute_physics_step()
    ///
    /// The constraint buffer contains ConstraintData structs generated from OWL axioms
    /// by OntologyConstraintTranslator, which are processed by ontology_constraints.cu kernels.
    /// # Thread Safety
    /// This method uses try_lock() to avoid blocking Tokio threads. If the GPU mutex
    /// is held, constraint upload is deferred to the next frame. This is acceptable
    /// because constraint uploads are idempotent and the GPU will apply the cached
    /// constraints on subsequent physics steps.
    fn apply_ontology_forces(&mut self) -> Result<(), String> {
        trace!("ForceComputeActor: Applying ontology constraint forces");

        // Check if we have a shared context with access to the GPU compute system
        let shared_context = match &self.shared_context {
            Some(ctx) => ctx,
            None => {
                trace!("ForceComputeActor: No shared context available for ontology forces");
                return Ok(()); // Not an error, just not available yet
            }
        };

        // Use the cached constraint buffer (updated via UpdateOntologyConstraintBuffer message)
        let constraint_buffer = &self.cached_constraint_buffer;

        // Skip if no constraints to apply
        if constraint_buffer.is_empty() {
            trace!("ForceComputeActor: No ontology constraints to apply");
            return Ok(());
        }

        // Use try_lock() to avoid blocking Tokio threads
        // If mutex is held by spawn_blocking task, skip this frame (constraints are idempotent)
        let mut unified_compute = match shared_context.unified_compute.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                trace!(
                    "ForceComputeActor: GPU mutex busy, deferring constraint upload to next frame"
                );
                return Ok(()); // Not an error, will retry next frame
            }
        };

        // Upload constraints to GPU via the LOSSLESS writer (ADR-098 break #4).
        // set_constraints preserves all four node_idx (the *other* endpoint of
        // every pairwise constraint, which the old upload_constraints 7-float
        // round-trip dropped) and all eight params, and stamps activation_frame
        // so the live constraint_ramp_frames progressive ramp engages. The live
        // force_pass_kernel constraint loop consumes this buffer directly once
        // ENABLE_CONSTRAINTS is set (execution.rs).
        unified_compute
            .set_constraints(constraint_buffer.clone())
            .map_err(|e| format!("Failed to upload ontology constraints to GPU: {}", e))?;

        debug!(
            "ForceComputeActor: Uploaded {} ontology constraints to GPU",
            constraint_buffer.len()
        );

        // Constraints are now on GPU and will be automatically applied
        // during the next execute_physics_step() call
        trace!("ForceComputeActor: Ontology constraint upload complete");
        Ok(())
    }
}

impl Default for ForceComputeActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for ForceComputeActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("ForceComputeActor: Started — initializing GPU context");

        // Self-initialize the GPU context immediately on startup.
        // This is the primary init path. The supervisor chain (GPUResourceActor ->
        // GPUManagerActor -> ResourceSupervisor -> PhysicsSupervisor -> here) is a
        // secondary path that can also set the context via SetSharedGPUContext.
        // If the supervisor chain delivers a context later, it will be accepted and
        // the self-created context will be replaced (see SetSharedGPUContext handler).
        self.initialize_own_gpu_context();
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("[ForceComputeActor] Stopped — cleaning up CUDA stability buffers");

        // ADR-031: Deallocate GPU memory tracked by the memory manager
        if let Some(ref ctx) = self.shared_context {
            if let Ok(mgr) = ctx.memory_manager.lock() {
                mgr.track_external_deallocation("positions");
                mgr.track_external_deallocation("edges_csr");
            }
        }

        // Drop the shared GPU context reference. When the last Arc<SharedGPUContext>
        // drops, the CudaDevice and all associated DeviceBuffers (positions,
        // velocities, forces, CSR edges, and the stability scratch buffers
        // partial_kinetic_energy / active_node_count / should_skip_physics owned
        // by UnifiedGPUCompute) are freed by the CUDA driver context teardown.
        if self.shared_context.take().is_some() {
            info!("[ForceComputeActor] Released SharedGPUContext reference");
        }
    }
}

// === Message Handlers ===

impl Handler<ComputeForces> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, _msg: ComputeForces, _ctx: &mut Self::Context) -> Self::Result {
        // Helper: notify orchestrator on early exit so the pipeline doesn't stall.
        macro_rules! notify_skip {
            ($self:ident) => {
                if let Some(ref orch_addr) = $self.physics_orchestrator_addr {
                    orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                        step_duration_ms: 0.0,
                        nodes_broadcast: 0,
                        iteration: $self.gpu_state.iteration_count,
                        kinetic_energy: f64::MAX, // Unknown — don't trigger false convergence
                        skipped: true, // benign skip, NOT a GPU failure — must not trip the breaker
                    });
                }
            };
        }

        // Circuit breaker: if the simulation has been halted due to sustained
        // divergence, do not step the GPU or broadcast. The layout is frozen at
        // the last-known-good positions until a recovery (UpdateSimulationParams
        // or an explicit reset) re-arms it.
        if self.simulation_halted {
            self.skipped_frames += 1;
            if self.skipped_frames.is_multiple_of(300) {
                error!(
                    "ForceComputeActor: simulation HALTED (divergence circuit breaker tripped) — \
                     stepping suspended, last-known-good positions retained. \
                     Apply a parameter change to recover."
                );
            }
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        // Early checks that don't need async
        if self.gpu_state.is_gpu_overloaded() {
            self.skipped_frames += 1;
            if self.skipped_frames.is_multiple_of(60) {
                debug!("ForceComputeActor: Skipped {} frames due to GPU overload (utilization: {:.1}%, concurrent ops: {})",
                      self.skipped_frames, self.gpu_state.get_average_utilization(), self.gpu_state.concurrent_access_count);
            }
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        if self.is_computing {
            self.skipped_frames += 1;
            if self.skipped_frames.is_multiple_of(60) {
                info!(
                    "ForceComputeActor: Skipped {} frames due to ongoing GPU computation",
                    self.skipped_frames
                );
            }
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        // Check for shared context; attempt self-init if missing
        if self.shared_context.is_none() {
            self.initialize_own_gpu_context();
        }
        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                // GPU init failed — this is a hard error, not transient
                if self.skipped_frames.is_multiple_of(300) {
                    error!(
                        "ForceComputeActor: GPU context unavailable after init attempt (frame {})",
                        self.skipped_frames
                    );
                }
                self.skipped_frames += 1;
                notify_skip!(self);
                return Box::pin(
                    futures::future::ready(Err("GPU context not initialized".to_string()))
                        .into_actor(self),
                );
            }
        };

        // Guard: skip compute when graph data hasn't been uploaded to GPU yet
        if self.gpu_state.num_nodes == 0 {
            if self.skipped_frames.is_multiple_of(60) {
                debug!("ForceComputeActor: Skipping compute — no graph data uploaded to GPU yet (waiting for InitializeGPU)");
            }
            self.skipped_frames += 1;
            notify_skip!(self);
            return Box::pin(futures::future::ready(Ok(())).into_actor(self));
        }

        self.is_computing = true;
        self.gpu_state
            .start_operation(GPUOperation::ForceComputation);

        // Apply ontology forces before async GPU access
        if let Err(e) = self.apply_ontology_forces() {
            warn!("ForceComputeActor: Failed to apply ontology forces: {}", e);
        }

        let step_start = Instant::now();
        let correlation_id = CorrelationId::new();
        let iteration = self.iteration_count();

        if iteration.is_multiple_of(60) {
            info!(
                "ForceComputeActor: Computing forces (iteration {}), nodes: {}",
                iteration, self.gpu_state.num_nodes
            );
        }

        // Log telemetry event
        if let Some(logger) = get_telemetry_logger() {
            let event = TelemetryEvent::new(
                correlation_id.clone(),
                LogLevel::DEBUG,
                "gpu_compute",
                "force_computation_start",
                &format!(
                    "Starting force computation iteration {} for {} nodes",
                    iteration, self.gpu_state.num_nodes
                ),
                "force_compute_actor",
            )
            .with_metadata("iteration", serde_json::json!(iteration))
            .with_metadata("node_count", serde_json::json!(self.gpu_state.num_nodes))
            .with_metadata("edge_count", serde_json::json!(self.gpu_state.num_edges))
            .with_metadata(
                "compute_mode",
                serde_json::json!(format!("{:?}", self.compute_mode)),
            );

            logger.log_event(event);
        }

        // Capture values needed for async block
        let sim_params = self.simulation_params.clone();
        let stability_bypass = self.stability_warmup_remaining > 0;
        if stability_bypass {
            self.stability_warmup_remaining -= 1;
        }
        let reheat_factor = self.reheat_factor;
        let current_iteration = self.gpu_state.iteration_count;

        // Log GPU params on first iteration to verify non-zero values
        if current_iteration == 0 {
            info!(
                "ForceComputeActor: FIRST GPU step — dt={}, damping={}, repel_k={}, spring_k={}, center_gravity_k={}, stability_bypass={}",
                sim_params.dt, sim_params.damping, sim_params.repel_k,
                sim_params.spring_k, sim_params.center_gravity_k, stability_bypass
            );
        }

        // Use spawn_blocking to prevent Tokio thread starvation from blocking mutex locks
        // GPU operations are inherently blocking (waiting for GPU kernels), so we move them
        // to the blocking thread pool to keep async executor threads responsive
        let fut = async move {
            // Acquire GPU access asynchronously (this uses tokio::sync::RwLock - non-blocking)
            let _gpu_guard = match shared_context.acquire_gpu_access().await {
                Ok(guard) => guard,
                Err(e) => {
                    let error_msg = format!("Failed to acquire GPU lock: {}", e);
                    return Err(error_msg);
                }
            };

            // Clone Arc for move into spawn_blocking
            let unified_compute_arc = shared_context.unified_compute.clone();

            // Move blocking GPU operations to dedicated blocking thread pool
            // This prevents std::sync::Mutex::lock() from blocking Tokio worker threads
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED by previous panic — recovering for physics step. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                if reheat_factor > 0.0 {
                    info!(
                        "Reheating physics with factor {:.2} to break equilibrium after parameter change",
                        reheat_factor
                    );
                    if let Err(e) = unified_compute.inject_velocity_perturbation(reheat_factor) {
                        warn!("Failed to inject velocity perturbation: {}", e);
                    }
                }

                let gpu_result = unified_compute.execute_physics_step_with_bypass(&sim_params, stability_bypass);
                let execution_duration = step_start.elapsed().as_secs_f64() * 1000.0;

                // Get positions and velocities for broadcast
                let positions_result = unified_compute.get_node_positions();
                let velocities_result = unified_compute.get_node_velocities();

                Ok((gpu_result, execution_duration, positions_result, velocities_result))
            }).await;

            // Handle spawn_blocking join result
            match blocking_result {
                Ok(inner_result) => inner_result.map(
                    |(gpu_result, execution_duration, positions_result, velocities_result)| {
                        (
                            gpu_result,
                            execution_duration,
                            positions_result,
                            velocities_result,
                            correlation_id,
                            iteration,
                            step_start,
                        )
                    },
                ),
                Err(join_err) => Err(format!("GPU blocking task panicked: {}", join_err)),
            }
        };

        Box::pin(fut.into_actor(self).map(move |result, actor, _ctx| {
            match result {
                Ok((gpu_result, execution_duration, positions_result, velocities_result, _correlation_id, _iteration, step_start)) => {
                    // Decay reheat factor gradually over ~30 steps so the layout has
                    // enough iterations to explore structure before settling. Multiply
                    // by 0.95 each step: step 0: 1.0, step 10: 0.60, step 20: 0.36,
                    // step 30: 0.21, step 50: 0.08 → cleared.
                    // (Previously 0.7x which decayed in ~10 steps — too fast for
                    // 2000+ node graphs to find community structure.)
                    if actor.reheat_factor > 0.0 {
                        // Decay 0.997 gives a ~230-step half-life: a reheat of 1.0 sustains
                        // ~21s at 60fps before hitting the 0.02 floor — matched to the 30s
                        // stability_warmup_remaining window. The previous 0.985 (~46-step
                        // half-life) burned out in ~4s, so the orchestrator's settle
                        // detector paused physics before a new equilibrium could form and
                        // every slider change looked dead at deep convergence.
                        actor.reheat_factor *= 0.997;
                        if actor.reheat_factor < 0.02 {
                            actor.reheat_factor = 0.0;
                        }
                    }
                    actor.stability_iterations += 1;
                    actor.last_step_duration_ms = execution_duration as f32;

                    match gpu_result {
                        Ok(_) => {
                            let gpu_utilization = actor.calculate_gpu_utilization(execution_duration);
                            actor.gpu_state.record_utilization(gpu_utilization);

                            if let Some(ctx) = &actor.shared_context {
                                if let Err(e) = ctx.update_utilization(gpu_utilization) {
                                    log::warn!("Failed to update shared GPU utilization metrics: {}", e);
                                }
                            }

                            // Log telemetry
                            if let Some(logger) = get_telemetry_logger() {
                                let gpu_memory_mb = (actor.gpu_state.num_nodes as f32 * 48.0 +
                                                    actor.gpu_state.num_edges as f32 * 24.0) / (1024.0 * 1024.0);

                                logger.log_gpu_execution(
                                    "force_computation_kernel",
                                    actor.gpu_state.num_nodes,
                                    execution_duration,
                                    gpu_memory_mb
                                );
                            }

                            // Process positions for broadcast
                            if let (Ok((pos_x, pos_y, pos_z)), Ok((vel_x, vel_y, vel_z))) =
                                (positions_result, velocities_result) {

                                // Reuse pre-allocated buffers to avoid 60Hz allocations
                                actor.position_velocity_buffer.clear();
                                actor.node_id_buffer.clear();

                                // Guard: GPU may have resized during graph reload.
                                // Clamp iteration count to the id map to prevent
                                // out-of-bounds access on gpu_index_to_node_id.
                                let usable_len = pos_x.len().min(pos_y.len()).min(pos_z.len())
                                    .min(vel_x.len()).min(vel_y.len()).min(vel_z.len());

                                if usable_len == 0 {
                                    warn!("ForceComputeActor: GPU returned 0-length position arrays — skipping broadcast");
                                } else {
                                    if usable_len != actor.gpu_index_to_node_id.len() {
                                        warn!(
                                            "ForceComputeActor: GPU buffer size ({}) != node id map size ({}) — clamping to min",
                                            usable_len, actor.gpu_index_to_node_id.len()
                                        );
                                    }
                                    let safe_len = usable_len.min(actor.gpu_index_to_node_id.len());

                                    // Reserve capacity if graph grew beyond initial allocation
                                    if safe_len > actor.position_velocity_buffer.capacity() {
                                        actor.position_velocity_buffer.reserve(safe_len - actor.position_velocity_buffer.capacity());
                                        actor.node_id_buffer.reserve(safe_len - actor.node_id_buffer.capacity());
                                    }

                                    for i in 0..safe_len {
                                        let position = Vec3::new(pos_x[i], pos_y[i], pos_z[i]);
                                        let velocity = Vec3::new(vel_x[i], vel_y[i], vel_z[i]);
                                        actor.position_velocity_buffer.push((position, velocity));
                                        let node_id = actor.gpu_index_to_node_id.get(i).copied().unwrap_or(i as u32);
                                        actor.node_id_buffer.push(node_id);
                                    }

                                // Grab→spring: hold any client-pinned (dragged) nodes at
                                // their client position and push those positions back to the
                                // GPU so the NEXT force step relaxes their neighbours around
                                // the moved node. Runs before the display-only projection
                                // (which is undone before the next step), so the re-uploaded
                                // buffer carries true physics positions with the pin applied.
                                // Upload the GPU pinned mask if pins changed, so the
                                // integrate kernel holds pinned nodes (skips their
                                // integration) while they still exert forces. Must run
                                // before apply_node_pins so the mask is live for the
                                // position rewrite + next force step.
                                actor.sync_pinned_mask();
                                actor.apply_node_pins();

                                // Two complementary Z controls:
                                //  * `axis_compression_z` (clamped) is the continuous
                                //    Z-scale, applied ALWAYS as the Z multiplier — 1.0 = no
                                //    compression (fully 3D, default). It drives the disc
                                //    face thinness when dual-disc is on, and squashes the
                                //    plain 3D layout when it is off.
                                //  * `enable_dual_disc_layout` (default OFF) gates the disc
                                //    re-centre: flatten each population into an X-Y disc and
                                //    separate along Z (Knowledge -sep, Ontology +sep, agents
                                //    at 0). OFF → no re-centre, natural 3D.
                                let sep = actor.simulation_params.graph_separation_x;
                                let face_scale =
                                    clamp_z_scale(actor.simulation_params.axis_compression_z);
                                let project = actor.simulation_params.enable_dual_disc_layout
                                    && !actor.node_population.is_empty();
                                // Once-per-300-iter diagnostic to verify the params reach this site.
                                if actor.gpu_state.iteration_count % 300 == 0 && project {
                                    info!(
                                        "ForceComputeActor: facing-disc projection iter={} sep_z={:.1} face_scale={:.2} populations={} (k+o+a)",
                                        actor.gpu_state.iteration_count, sep, face_scale, actor.node_population.len()
                                    );
                                }
                                // NOTE: projection is applied DISPLAY-ONLY, after the
                                // divergence guard and last-known-good capture, then undone
                                // before the next physics step (see below). It must NOT feed
                                // back into the simulation buffer: doing so makes the 56k
                                // KG<->ontology cross-links (restLength ~30) span the full
                                // separation gap every frame and yank both populations back
                                // together, so the discs never actually separate.

                                // ---------------------------------------------------------
                                // Divergence guard: detect corrupted/exploding GPU output
                                // BEFORE broadcasting. Three failure modes are caught:
                                //   1. NaN/Inf in positions OR velocities (corrupt state).
                                //   2. Position beyond MAX_COORD on any axis (runaway).
                                //   3. Velocity magnitude beyond MAX_VELOCITY_MAGNITUDE.
                                // A frame failing any of these is "bad": its positions are
                                // never broadcast (clients would see infinity), the last
                                // known-good frame is re-broadcast instead, and the circuit
                                // breaker counter advances. After MAX_CONSECUTIVE_BAD_FRAMES
                                // the simulation halts and recovery resets velocities.
                                // ---------------------------------------------------------
                                let mut nan_count = 0usize;
                                let mut oob_count = 0usize;
                                for (p, v) in actor.position_velocity_buffer.iter() {
                                    if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite()
                                        || !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite()
                                    {
                                        nan_count += 1;
                                    } else if p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD || p.z.abs() > MAX_COORD
                                        || v.length() > MAX_VELOCITY_MAGNITUDE
                                    {
                                        oob_count += 1;
                                    }
                                }

                                // Average kinetic energy of this frame (used both as a
                                // divergence signal and reported to the orchestrator).
                                let frame_ke: f64 = if actor.position_velocity_buffer.is_empty() {
                                    0.0
                                } else {
                                    let total: f64 = actor.position_velocity_buffer.iter()
                                        .map(|(_p, v)| 0.5 * (v.x as f64 * v.x as f64
                                            + v.y as f64 * v.y as f64
                                            + v.z as f64 * v.z as f64))
                                        .sum();
                                    total / actor.position_velocity_buffer.len() as f64
                                };
                                let ke_diverged = !frame_ke.is_finite() || frame_ke > MAX_KINETIC_ENERGY;

                                let frame_is_bad = nan_count > 0 || oob_count > 0 || ke_diverged;

                                if frame_is_bad {
                                    actor.consecutive_bad_frames += 1;
                                    error!(
                                        "[ForceComputeActor] Divergent frame at iter {} \
                                         (nan/inf={}, out-of-bounds={}, ke={:.3e}, ke_diverged={}) \
                                         — skipping broadcast, re-using last-known-good. \
                                         Consecutive bad frames: {}/{}",
                                        actor.gpu_state.iteration_count,
                                        nan_count, oob_count, frame_ke, ke_diverged,
                                        actor.consecutive_bad_frames, MAX_CONSECUTIVE_BAD_FRAMES
                                    );

                                    // Re-broadcast the last known-good positions so clients
                                    // hold a sane layout instead of receiving garbage. Only
                                    // possible once we have captured at least one good frame.
                                    if !actor.last_good_positions.is_empty() {
                                        if let Some(_seq) = actor.backpressure.try_acquire() {
                                            let mut node_updates =
                                                Vec::with_capacity(actor.last_good_positions.len());
                                            // Compute population centroids from the last-known-good
                                            // layout so the fallback applies the IDENTICAL projection
                                            // as the healthy path (Z-separation, per-population median
                                            // re-centre, fixed DISC_RIM_RADIUS rim-clamp) instead of a
                                            // divergent Y-separation. A bad frame must not briefly
                                            // re-orient or collapse the two discs.
                                            let centroids = if project {
                                                population_centroids_xy(
                                                    &actor.node_population,
                                                    |i| {
                                                        let (_, p, _) = actor.last_good_positions[i];
                                                        (p.x, p.y)
                                                    },
                                                    actor.last_good_positions.len(),
                                                )
                                            } else {
                                                Default::default()
                                            };
                                            let r_max = DISC_RIM_RADIUS;
                                            for (idx, (node_id, pos, vel)) in actor.last_good_positions.iter().enumerate() {
                                                let mut p = *pos;
                                                if project {
                                                    if let Some(&pop) = actor.node_population.get(idx) {
                                                        project_node_xy(&mut p, pop, &centroids, sep, face_scale, r_max);
                                                    }
                                                }
                                                node_updates.push((*node_id, BinaryNodeDataClient::new(
                                                    *node_id,
                                                    glam_to_vec3data(p),
                                                    glam_to_vec3data(*vel),
                                                )));
                                            }
                                            if let Some(ref graph_addr) = actor.graph_service_addr {
                                                graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                    positions: node_updates,
                                                    correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                                });
                                            }
                                        }
                                    }

                                    // Trip the circuit breaker on sustained divergence.
                                    if actor.consecutive_bad_frames >= MAX_CONSECUTIVE_BAD_FRAMES {
                                        actor.simulation_halted = true;
                                        error!(
                                            "[ForceComputeActor] CIRCUIT BREAKER TRIPPED at iter {} — \
                                             {} consecutive divergent frames. HALTING simulation. \
                                             Velocities will be zeroed and last-known-good positions \
                                             restored to the GPU on next recovery.",
                                            actor.gpu_state.iteration_count, actor.consecutive_bad_frames
                                        );
                                        actor.recover_from_divergence();
                                    }
                                    // Skip all normal broadcast logic for this frame.
                                } else {

                                // Frame is good. Defensive clamp as a backstop (no-op for the
                                // healthy ~±2400 range) so any value that slipped just past a
                                // GPU-side clamp is corrected before it is stored/broadcast.
                                for (pos, vel) in actor.position_velocity_buffer.iter_mut() {
                                    pos.x = pos.x.clamp(-MAX_COORD, MAX_COORD);
                                    pos.y = pos.y.clamp(-MAX_COORD, MAX_COORD);
                                    pos.z = pos.z.clamp(-MAX_COORD, MAX_COORD);
                                    let speed = vel.length();
                                    if speed > MAX_VELOCITY_MAGNITUDE {
                                        *vel *= MAX_VELOCITY_MAGNITUDE / speed;
                                    }
                                }

                                // Reset the divergence counter — we have a healthy frame.
                                actor.consecutive_bad_frames = 0;

                                // Snapshot this frame as the last-known-good layout for
                                // recovery / bad-frame fallback. Reuse the buffer capacity.
                                actor.last_good_positions.clear();
                                if actor.last_good_positions.capacity() < actor.position_velocity_buffer.len() {
                                    actor.last_good_positions.reserve(
                                        actor.position_velocity_buffer.len() - actor.last_good_positions.capacity()
                                    );
                                }
                                for idx in 0..actor.position_velocity_buffer.len() {
                                    let node_id = actor.node_id_buffer[idx];
                                    let (pos, vel) = actor.position_velocity_buffer[idx];
                                    actor.last_good_positions.push((node_id, pos, vel));
                                }

                                // Display-only dual-graph co-planar projection. The pristine
                                // physics state is now safely captured in last_good_positions;
                                // mutate the broadcast buffer in place (separate populations
                                // along Y, flatten Z into the shared X-Y plane), broadcast,
                                // then restore the buffer from last_good_positions before the
                                // next physics step so the integrator never sees the offsets.
                                if project {
                                    let centroids = population_centroids_xy(
                                        &actor.node_population,
                                        |i| {
                                            let (p, _) = actor.position_velocity_buffer[i];
                                            (p.x, p.y)
                                        },
                                        actor.position_velocity_buffer.len(),
                                    );
                                    // Rim radius is the disc's own fixed size, NOT sep —
                                    // close discs (small sep) stay full-size.
                                    let r_max = DISC_RIM_RADIUS;
                                    for (i, (pos, _vel)) in actor.position_velocity_buffer.iter_mut().enumerate() {
                                        if let Some(&pop) = actor.node_population.get(i) {
                                            project_node_xy(pos, pop, &centroids, sep, face_scale, r_max);
                                        }
                                    }
                                } else if (face_scale - 1.0).abs() > f32::EPSILON {
                                    // Dual-disc OFF but the user set a continuous Z compression:
                                    // apply the Z-scale directly (display-only, undone before
                                    // the next step like the disc projection). face_scale==1.0
                                    // is a no-op → fully 3D.
                                    for (pos, _vel) in actor.position_velocity_buffer.iter_mut() {
                                        pos.z *= face_scale;
                                    }
                                }

                                // Diagnostic: log first few positions on early frames (6 decimal places for velocity)
                                if actor.gpu_state.iteration_count < 5 || actor.gpu_state.iteration_count % 300 == 0 {
                                    let n = actor.position_velocity_buffer.len().min(3);
                                    for i in 0..n {
                                        let (p, v) = actor.position_velocity_buffer[i];
                                        debug!("ForceComputeActor: iter={} node[{}] pos=({:.2},{:.2},{:.2}) vel=({:.6},{:.6},{:.6})",
                                            actor.gpu_state.iteration_count, actor.node_id_buffer[i],
                                            p.x, p.y, p.z, v.x, v.y, v.z);
                                    }
                                }

                                // FastSettle broadcast control:
                                // - suppress_intermediate_broadcasts: skip during settle burst
                                // - force_full_broadcast: send ALL nodes (final converged positions)
                                if actor.force_full_broadcast {
                                    // Final broadcast after settle — send ALL nodes
                                    actor.force_full_broadcast = false;
                                    actor.suppress_intermediate_broadcasts = false;
                                    actor.broadcast_optimizer.reset_broadcast_timer();

                                    if let Some(_sequence_id) = actor.backpressure.try_acquire() {
                                        let mut node_updates = Vec::with_capacity(actor.node_id_buffer.len());
                                        for idx in 0..actor.node_id_buffer.len() {
                                            let node_id = actor.node_id_buffer[idx];
                                            let (position, velocity) = actor.position_velocity_buffer[idx];
                                            if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                                                continue;
                                            }
                                            node_updates.push((node_id, BinaryNodeDataClient::new(
                                                node_id,
                                                glam_to_vec3data(position),
                                                glam_to_vec3data(velocity),
                                            )));
                                        }
                                        if let Some(ref graph_addr) = actor.graph_service_addr {
                                            info!(
                                                "ForceComputeActor: FINAL full broadcast — {} nodes (iter {})",
                                                node_updates.len(), actor.gpu_state.iteration_count
                                            );
                                            graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                positions: node_updates,
                                                correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                            });
                                        }
                                    }
                                } else if actor.suppress_intermediate_broadcasts {
                                    // FastSettle burst in progress — skip intermediate broadcasts.
                                    // Still call process_frame to advance the rate-limit timer.
                                    let _ = actor.broadcast_optimizer.process_frame(&actor.position_velocity_buffer, &actor.node_id_buffer);
                                } else {
                                    // Continuous mode: always send full position snapshots.
                                    // Clients tween at 60fps — they need complete target state,
                                    // not incremental deltas. Rate-limited by broadcast_optimizer.
                                    let (should_broadcast, _) =
                                        actor.broadcast_optimizer.process_frame(&actor.position_velocity_buffer, &actor.node_id_buffer);

                                    if should_broadcast {
                                        if let Some(_sequence_id) = actor.backpressure.try_acquire() {
                                            let mut node_updates = Vec::with_capacity(actor.node_id_buffer.len());
                                            for idx in 0..actor.node_id_buffer.len() {
                                                let node_id = actor.node_id_buffer[idx];
                                                let (position, velocity) = actor.position_velocity_buffer[idx];
                                                if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                                                    continue;
                                                }
                                                node_updates.push((node_id, BinaryNodeDataClient::new(
                                                    node_id,
                                                    glam_to_vec3data(position),
                                                    glam_to_vec3data(velocity),
                                                )));
                                            }
                                            if let Some(ref graph_addr) = actor.graph_service_addr {
                                                if actor.gpu_state.iteration_count % 300 == 0 {
                                                    info!(
                                                        "ForceComputeActor: Full snapshot — {} nodes (iter {})",
                                                        node_updates.len(), actor.gpu_state.iteration_count
                                                    );
                                                }
                                                graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                    positions: node_updates,
                                                    correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                                });
                                            }
                                            actor.last_full_broadcast_iteration = actor.gpu_state.iteration_count;
                                            actor.broadcast_optimizer.reset_broadcast_timer();
                                        } else {
                                            actor.backpressure.record_skip();
                                        }
                                    } else if actor.gpu_state.iteration_count.saturating_sub(actor.last_full_broadcast_iteration) >= 300 {
                                        // Periodic full broadcast for late-connecting clients
                                        if let Some(_sequence_id) = actor.backpressure.try_acquire() {
                                            let mut node_updates = Vec::with_capacity(actor.node_id_buffer.len());
                                            for idx in 0..actor.node_id_buffer.len() {
                                                let node_id = actor.node_id_buffer[idx];
                                                let (position, velocity) = actor.position_velocity_buffer[idx];
                                                // Skip NaN/Inf positions
                                                if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                                                    continue;
                                                }
                                                node_updates.push((node_id, BinaryNodeDataClient::new(
                                                    node_id,
                                                    glam_to_vec3data(position),
                                                    glam_to_vec3data(velocity),
                                                )));
                                            }

                                            if let Some(ref graph_addr) = actor.graph_service_addr {
                                                info!(
                                                    "ForceComputeActor: Periodic full broadcast — sending ALL {} positions (iter {}, last full at {})",
                                                    node_updates.len(), actor.gpu_state.iteration_count,
                                                    actor.last_full_broadcast_iteration
                                                );
                                                graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                                                    positions: node_updates,
                                                    correlation_id: Some(crate::actors::messaging::MessageId::new()),
                                                });
                                            }

                                            actor.last_full_broadcast_iteration = actor.gpu_state.iteration_count;
                                            // Reset the broadcast timer so the next snapshot goes out promptly
                                            actor.broadcast_optimizer.reset_broadcast_timer();
                                        }
                                    }
                                } // end normal broadcast else branch

                                // Undo the display-only projection/Z-scale: restore the
                                // pristine physics positions captured in last_good_positions
                                // so the next integration step computes forces on the true,
                                // un-separated, un-flattened layout. CRITICAL: this must
                                // cover the continuous Z-scale branch too (project == false
                                // but face_scale != 1.0) — otherwise the compression compounds
                                // every frame and collapses the graph to z=0.
                                if project || (face_scale - 1.0).abs() > f32::EPSILON {
                                    for idx in 0..actor.position_velocity_buffer.len() {
                                        let (_node_id, pos, vel) = actor.last_good_positions[idx];
                                        actor.position_velocity_buffer[idx] = (pos, vel);
                                    }
                                }
                                } // end NaN guard else (clean positions)
                                }
                            }

                            actor.gpu_state.iteration_count += 1;
                            actor.last_step_duration_ms = step_start.elapsed().as_millis() as f32;

                            if actor.iteration_count() % 300 == 0 {
                                debug!("ForceComputeActor: {} iterations completed, {} GPU failures, {} skipped frames, last step: {:.2}ms",
                                      actor.iteration_count(), actor.gpu_state.gpu_failure_count, actor.skipped_frames, actor.last_step_duration_ms);
                            }

                            // Compute kinetic energy from velocity buffer for convergence detection.
                            // KE = 0.5 * sum(vx^2 + vy^2 + vz^2), averaged over node count.
                            let step_kinetic_energy = if actor.position_velocity_buffer.is_empty() {
                                0.0_f64
                            } else {
                                let total_ke: f64 = actor.position_velocity_buffer.iter()
                                    .map(|(_pos, vel)| {
                                        0.5 * (vel.x as f64 * vel.x as f64
                                             + vel.y as f64 * vel.y as f64
                                             + vel.z as f64 * vel.z as f64)
                                    })
                                    .sum();
                                total_ke / actor.position_velocity_buffer.len() as f64
                            };

                            // Fold this tick's mean KE into the honest settlement
                            // tracker (consecutive sub-epsilon frames). Surfaced by
                            // GetSettlementState / GetCurrentPositions.
                            actor.update_settlement(step_kinetic_energy);

                            // Sequential pipeline: notify orchestrator that this step is done
                            // so it can trigger broadcast and schedule the next step.
                            if let Some(ref orch_addr) = actor.physics_orchestrator_addr {
                                orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                                    step_duration_ms: actor.last_step_duration_ms,
                                    nodes_broadcast: actor.position_velocity_buffer.len() as u32,
                                    iteration: actor.gpu_state.iteration_count,
                                    kinetic_energy: step_kinetic_energy,
                                    skipped: false,
                                });
                            }

                            actor.is_computing = false;
                            actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                            Ok(())
                        }
                        Err(e) => {
                            let error_msg = format!("GPU force computation failed: {}", e);
                            error!("{}", error_msg);
                            actor.gpu_state.gpu_failure_count += 1;

                            // Sequential pipeline: notify orchestrator even on failure
                            // so the pipeline doesn't stall.
                            if let Some(ref orch_addr) = actor.physics_orchestrator_addr {
                                orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                                    step_duration_ms: actor.last_step_duration_ms,
                                    nodes_broadcast: 0,
                                    iteration: actor.gpu_state.iteration_count,
                                    kinetic_energy: f64::MAX,
                                    skipped: false, // real GPU compute failure
                                });
                            }

                            actor.is_computing = false;
                            actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                            Err(error_msg)
                        }
                    }
                }
                Err(e) => {
                    error!("GPU access failed: {}", e);

                    // Sequential pipeline: notify orchestrator even on failure.
                    // Note: step_start is not in scope here (only destructured in Ok arm),
                    // so we report 0.0 since the GPU step never actually executed.
                    if let Some(ref orch_addr) = actor.physics_orchestrator_addr {
                        orch_addr.do_send(crate::actors::messages::PhysicsStepCompleted {
                            step_duration_ms: 0.0,
                            nodes_broadcast: 0,
                            iteration: actor.gpu_state.iteration_count,
                            kinetic_energy: f64::MAX,
                            skipped: false, // real GPU access failure
                        });
                    }

                    actor.is_computing = false;
                    actor.gpu_state.complete_operation(&GPUOperation::ForceComputation);
                    Err(e)
                }
            }
        }))
    }
}

/// ADR-141 P1: switch the active layout mode on the physics actor. The mode is
/// carried GPU-side via `SimParams.layout_mode`; for GPU-resident modes it also
/// primes the associated force-term scalars (Radial ⇒ raise the `dag_radial_bias`
/// shell strength; ForceDirected ⇒ disable it) so the single "set the mode" call
/// produces a visible relayout. CPU one-shot modes (Hierarchical/Spectral/Temporal)
/// still route through the layout handler's `compute_layout`; here they only record
/// the GPU-visible discriminant. The change is applied through the same
/// `UpdateSimulationParams` path so validation, idempotency, resync and reheat all
/// behave identically.
impl Handler<SetLayoutMode> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetLayoutMode, ctx: &mut Self::Context) -> Self::Result {
        use crate::layout::types::LayoutMode;

        // Default radial shell strength when Radial is selected with the DAG bias
        // still at its off-default (0.0). Kept modest so the shell guides rather than
        // dominates springs; the user can retune via /api/settings/physics dagBiasK.
        const RADIAL_DEFAULT_DAG_BIAS_K: f32 = 1.0;
        const RADIAL_DEFAULT_DAG_LEVEL_DISTANCE: f32 = 60.0;

        // Default Sugiyama layer-spring strength/spacing when Hierarchical is
        // selected with the layer bias still at its off-default (ADR-141 P4).
        const HIERARCHICAL_DEFAULT_LAYER_BIAS_K: f32 = 1.0;
        const HIERARCHICAL_DEFAULT_LAYER_SPACING: f32 = 60.0;

        // Layout mode is authoritative on the actor and only SetLayoutMode may change
        // it. Commit the new mode to self FIRST so the UpdateSimulationParams handler
        // (which preserves self.simulation_params.layout_mode — see below) carries it
        // through even if a settings-driven update races in with a default mode.
        self.simulation_params.layout_mode = msg.mode;

        let mut params = self.simulation_params.clone();
        params.layout_mode = msg.mode;

        match msg.mode {
            LayoutMode::Radial => {
                if params.dag_bias_k <= 0.0 {
                    params.dag_bias_k = RADIAL_DEFAULT_DAG_BIAS_K;
                }
                if params.dag_level_distance <= 0.0 {
                    params.dag_level_distance = RADIAL_DEFAULT_DAG_LEVEL_DISTANCE;
                }
                // Radial owns the shell term, not the Sugiyama layer spring.
                params.layer_bias_k = 0.0;
            }
            // Hierarchical (ADR-141 P4) is now a GPU-resident layered layout: prime
            // the Sugiyama Y-by-rank layer spring so ranked nodes settle onto
            // top-down layers. The radial shell is cleared so the two don't fight.
            LayoutMode::Hierarchical => {
                if params.layer_bias_k <= 0.0 {
                    params.layer_bias_k = HIERARCHICAL_DEFAULT_LAYER_BIAS_K;
                }
                if params.layer_spacing <= 0.0 {
                    params.layer_spacing = HIERARCHICAL_DEFAULT_LAYER_SPACING;
                }
                params.dag_bias_k = 0.0;
            }
            // Every other mode clears both the radial-shell bias and the Sugiyama
            // layer spring so a mode switch (e.g. Radial/Hierarchical →
            // Clustered/ForceDirected) does not leave a stale bias term firing. The
            // mode is authoritative over these; dagBiasK/layerBiasK can still be
            // raised explicitly via /api/settings/physics. Clustered rides the
            // existing cluster-cohesion term (cluster_strength); the remaining CPU
            // one-shot modes (Spectral/Temporal) are placed by the layout handler.
            _ => {
                params.dag_bias_k = 0.0;
                params.layer_bias_k = 0.0;
            }
        }

        info!(
            "ForceComputeActor: SetLayoutMode -> {:?} (gpu_resident={}, dag_bias_k={:.3}, layer_bias_k={:.3})",
            msg.mode,
            msg.mode.is_gpu_resident(),
            params.dag_bias_k,
            params.layer_bias_k
        );

        <Self as Handler<UpdateSimulationParams>>::handle(
            self,
            UpdateSimulationParams { params },
            ctx,
        )
    }
}

/// ADR-141 P3: re-key the `dag_radial_bias` radial shells. Reuses the existing
/// shell term — only the per-node KEY (uploaded via the `node_rank` buffer) and
/// the shell CENTRE (`SimulationParams.radial_center`) change per RadialMode:
///   - DagRank : key = cached DAG hierarchy rank;      centre = origin (legacy).
///   - TypeTier: key = node-type tier (Agent 0 → Knowledge 1 → Ontology 2);
///     centre = origin.
///   - Ego     : key = BFS hop-distance from `focus_node`; centre = the focus
///     node's live GPU position (origin if unreadable).
///
/// The centre is actor-authoritative (preserved through settings PUTs by the
/// UpdateSimulationParams handler). Applied through the same UpdateSimulationParams
/// path as SetLayoutMode so validation, resync and reheat behave identically.
impl Handler<SetRadialLayout> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetRadialLayout, ctx: &mut Self::Context) -> Self::Result {
        use crate::layout::types::RadialMode;

        // Prime the shell strength/spacing when Radial re-keying is requested with
        // the DAG bias still at its off-default, so the shells actually engage
        // (mirrors the Radial arm of SetLayoutMode).
        const RADIAL_DEFAULT_DAG_BIAS_K: f32 = 1.0;
        const RADIAL_DEFAULT_DAG_LEVEL_DISTANCE: f32 = 60.0;

        let num_nodes = self.node_population.len();
        if num_nodes == 0 {
            return Err("no graph uploaded — radial layout cannot be keyed".to_string());
        }

        // Build the per-node shell KEY vec and the shell CENTRE for the mode.
        let (keys, center): (Vec<f32>, [f32; 3]) = match msg.mode {
            RadialMode::DagRank => {
                let keys = if self.dag_ranks.len() == num_nodes {
                    self.dag_ranks.clone()
                } else {
                    // No cached ranks (no hierarchy) → all-unranked (bias inert).
                    vec![-1.0; num_nodes]
                };
                (keys, [0.0, 0.0, 0.0])
            }
            RadialMode::TypeTier => {
                let keys = if self.node_population.len() == num_nodes {
                    self.node_population
                        .iter()
                        .map(|pop| match pop {
                            GraphPopulation::Agent => 0.0,
                            GraphPopulation::Knowledge => 1.0,
                            GraphPopulation::Ontology => 2.0,
                        })
                        .collect()
                } else {
                    vec![-1.0; num_nodes]
                };
                (keys, [0.0, 0.0, 0.0])
            }
            RadialMode::Ego => {
                let focus = msg
                    .focus_node
                    .ok_or_else(|| "ego radial mode requires focus_node".to_string())?;
                let focus_idx = *self
                    .radial_node_index
                    .get(&focus)
                    .ok_or_else(|| "focus node not in graph".to_string())?;
                let keys = Self::compute_ego_distances(num_nodes, &self.graph_adjacency, focus_idx);
                // Centre the shells on the focus node's live position; fall back to
                // the origin if positions are unreadable.
                let mut center = [0.0f32, 0.0, 0.0];
                if let Some(shared) = &self.shared_context {
                    let mut unified = match shared.unified_compute.lock() {
                        Ok(g) => g,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    if let Ok((xs, ys, zs)) = unified.get_node_positions() {
                        if focus_idx < xs.len() {
                            center = [xs[focus_idx], ys[focus_idx], zs[focus_idx]];
                        }
                    }
                }
                (keys, center)
            }
        };

        // Upload the shell keys via the existing node_rank buffer.
        if let Some(shared) = &self.shared_context {
            let mut unified = match shared.unified_compute.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Err(e) = unified.upload_node_rank(&keys) {
                return Err(format!("radial key upload failed: {}", e));
            }
        } else {
            return Err("GPU context unavailable — radial layout not applied".to_string());
        }

        // Commit the actor-authoritative shell centre FIRST so the
        // UpdateSimulationParams handler (which preserves radial_center) carries it.
        // NOTE: the per-step physics rebuilds `sim_params` from `self.simulation_params`
        // via a fresh clone every step (see the ComputeForces path, ~`let sim_params =
        // self.simulation_params.clone()`), so this centre reaches the GPU on the next
        // step even when the UpdateSimulationParams idempotency guard skips the reheat —
        // and the forced reheat below guarantees the graph re-settles onto it.
        self.simulation_params.radial_center = center;

        let mut params = self.simulation_params.clone();
        params.radial_center = center;
        if params.dag_bias_k <= 0.0 {
            params.dag_bias_k = RADIAL_DEFAULT_DAG_BIAS_K;
        }
        if params.dag_level_distance <= 0.0 {
            params.dag_level_distance = RADIAL_DEFAULT_DAG_LEVEL_DISTANCE;
        }

        info!(
            "ForceComputeActor: SetRadialLayout -> {:?} (focus={:?}, center=[{:.1},{:.1},{:.1}], dag_bias_k={:.3})",
            msg.mode, msg.focus_node, center[0], center[1], center[2], params.dag_bias_k
        );

        let result = <Self as Handler<UpdateSimulationParams>>::handle(
            self,
            UpdateSimulationParams { params },
            ctx,
        );

        // Re-keying the shells only changes the per-node node_rank buffer (already
        // uploaded above), which UpdateSimulationParams' idempotency guard cannot see
        // — a pure key-source switch (e.g. DagRank→TypeTier) with unchanged SimParams
        // would otherwise skip reheat and leave the new shells un-engaged at deep
        // equilibrium. Force a modest reheat + stability-bypass so the graph always
        // re-settles onto the new shells.
        if result.is_ok() {
            self.reheat_factor = self.reheat_factor.max(1.5);
            self.stability_warmup_remaining = self.stability_warmup_remaining.max(900);
            self.broadcast_optimizer.reset_broadcast_timer();
        }

        result
    }
}

impl Handler<UpdateSimulationParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        mut msg: UpdateSimulationParams,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        // Layout mode is owned exclusively by SetLayoutMode. A settings-driven update
        // is built via `PhysicsSettings -> SimulationParams`, which cannot recover the
        // mode (PhysicsSettings has no mode field) and so defaults it to ForceDirected.
        // Preserve the actor's current mode here so changing any physics slider never
        // silently resets an active Radial/Clustered/CPU mode. SetLayoutMode commits
        // the new mode to `self` before delegating here, so this preserves the *new*
        // mode on that path (no clobber, no lost switch).
        msg.params.layout_mode = self.simulation_params.layout_mode;

        // ADR-141 P3: the radial shell centre is owned by SetRadialLayout (Ego mode
        // centres it on the focus node). A settings-driven update is built from
        // PhysicsSettings, which has no radial_center source and defaults it to the
        // origin — preserve the actor's current centre so a physics PUT never resets
        // an active Ego centre. SetRadialLayout commits the new centre to `self`
        // before delegating here, so this preserves the *new* centre on that path.
        msg.params.radial_center = self.simulation_params.radial_center;

        // Validate incoming parameters before applying — reject unsafe values
        // that could cause GPU explosion (dt=1000), infinite energy (damping=0),
        // or gravitational collapse (repel_k=-1).
        if let Err(validation_errors) = msg.params.validate() {
            error!(
                "ForceComputeActor: UpdateSimulationParams REJECTED — validation failed: {}",
                validation_errors
            );
            return Err(format!(
                "Parameter validation failed: {}",
                validation_errors
            ));
        }

        // Idempotency: skip reset if ALL GPU-relevant params haven't changed.
        // The client autoSaveManager may fire redundant updates (GET-merge-PUT with same values).
        // Compare the full set of GPU-relevant fields, not just the original 6.
        //
        // CRITICAL: fields used by post-GPU Rust position-modification code (eg.
        // graph_separation_x, axis_compression_z, enable_dual_disc_layout) and
        // feature-flag-derived fields (eg. adaptive_speed) MUST appear here —
        // otherwise their value gets silently dropped when no other field changed.
        let cur = &self.simulation_params;
        let eps = 1e-5_f32; // Slightly larger than EPSILON to catch floating-point round-trips
        let physics_unchanged = (cur.spring_k - msg.params.spring_k).abs() < eps
            && (cur.repel_k - msg.params.repel_k).abs() < eps
            && (cur.damping - msg.params.damping).abs() < eps
            && (cur.dt - msg.params.dt).abs() < eps
            && (cur.max_velocity - msg.params.max_velocity).abs() < eps
            && (cur.max_force - msg.params.max_force).abs() < eps
            && (cur.center_gravity_k - msg.params.center_gravity_k).abs() < eps
            && (cur.temperature - msg.params.temperature).abs() < eps
            && (cur.cluster_strength - msg.params.cluster_strength).abs() < eps
            && (cur.alignment_strength - msg.params.alignment_strength).abs() < eps
            && (cur.separation_radius - msg.params.separation_radius).abs() < eps
            && (cur.cooling_rate - msg.params.cooling_rate).abs() < eps
            && (cur.viewport_bounds - msg.params.viewport_bounds).abs() < eps
            && (cur.boundary_damping - msg.params.boundary_damping).abs() < eps
            && (cur.gravity - msg.params.gravity).abs() < eps
            && (cur.graph_separation_x - msg.params.graph_separation_x).abs() < eps
            && (cur.axis_compression_z - msg.params.axis_compression_z).abs() < eps
            && cur.enable_dual_disc_layout == msg.params.enable_dual_disc_layout
            && cur.adaptive_speed == msg.params.adaptive_speed
            && cur.iterations == msg.params.iterations
            && cur.use_sssp_distances == msg.params.use_sssp_distances
            && cur.warmup_iterations == msg.params.warmup_iterations
            && cur.constraint_ramp_frames == msg.params.constraint_ramp_frames
            && (cur.constraint_max_force_per_node - msg.params.constraint_max_force_per_node).abs()
                < eps
            && (cur.spring_k_knowledge - msg.params.spring_k_knowledge).abs() < eps
            && (cur.spring_k_ontology - msg.params.spring_k_ontology).abs() < eps
            && (cur.spring_k_agent - msg.params.spring_k_agent).abs() < eps
            // DAG radial bias (PHASE 2) is GPU-relevant — omitting these fields
            // silently drops DAG-only settings changes at the early return below.
            && (cur.dag_bias_k - msg.params.dag_bias_k).abs() < eps
            && (cur.dag_level_distance - msg.params.dag_level_distance).abs() < eps
            // Stratified planes (ADR-141 P2) are GPU-relevant — omitting these fields
            // silently drops plane-only settings changes at the early return below.
            && (cur.plane_bias_k - msg.params.plane_bias_k).abs() < eps
            && (cur.plane_spacing - msg.params.plane_spacing).abs() < eps
            // Sugiyama layer spring (ADR-141 P4) is GPU-relevant — omitting these
            // fields silently drops layer-only settings changes at the early return.
            && (cur.layer_bias_k - msg.params.layer_bias_k).abs() < eps
            && (cur.layer_spacing - msg.params.layer_spacing).abs() < eps
            // Radial shell centre (ADR-141 P3) rides SimParams.radial_center — omitting
            // it would silently drop an Ego re-centre at the early return below.
            && (cur.radial_center[0] - msg.params.radial_center[0]).abs() < eps
            && (cur.radial_center[1] - msg.params.radial_center[1]).abs() < eps
            && (cur.radial_center[2] - msg.params.radial_center[2]).abs() < eps
            // Layout mode (ADR-141 P1) is GPU-relevant — it rides SimParams.layout_mode.
            // Omitting it would silently drop a mode-only switch at the early return.
            && cur.layout_mode == msg.params.layout_mode;

        if physics_unchanged {
            debug!(
                "ForceComputeActor: UpdateSimulationParams — GPU-relevant fields unchanged, skipping reset"
            );
            return Ok(());
        }

        info!("ForceComputeActor: UpdateSimulationParams received — params CHANGED");
        debug!(
            "  New params - spring_k: {:.3}, repel_k: {:.3}, damping: {:.3}, center_gravity_k: {:.3}, cluster: {:.3}, align: {:.3}",
            msg.params.spring_k, msg.params.repel_k, msg.params.damping,
            msg.params.center_gravity_k, msg.params.cluster_strength, msg.params.alignment_strength
        );

        // Capture prior force coefficients BEFORE update_simulation_parameters overwrites
        // self.simulation_params. Used below to scale reheat energy proportional to the
        // magnitude of the user's change — spring changes must reheat too, or the spring
        // sliders appear dead at deep equilibrium (one 4s nudge, no visible relayout).
        let prior_repel_k = self.simulation_params.repel_k.max(1.0);
        let new_repel_k = msg.params.repel_k.max(1.0);
        let prior_spring_k = self.simulation_params.spring_k.max(0.01);
        let new_spring_k = msg.params.spring_k.max(0.01);

        // Detect per-population spring changes so we re-upload the spring_scale buffer
        // (it is otherwise only set on graph load). These drive the independent
        // Knowledge/Ontology/Agent spring sliders.
        let spring_pop_changed =
            (self.simulation_params.spring_k_knowledge - msg.params.spring_k_knowledge).abs()
                >= eps
                || (self.simulation_params.spring_k_ontology - msg.params.spring_k_ontology).abs()
                    >= eps
                || (self.simulation_params.spring_k_agent - msg.params.spring_k_agent).abs() >= eps;

        self.update_simulation_parameters(msg.params);

        // Re-upload spring_scale on any per-population spring change so the new
        // coefficients reach the GPU without requiring a full graph re-upload.
        if spring_pop_changed && !self.node_population.is_empty() {
            if let Some(ref ctx) = self.shared_context {
                if let Ok(mut compute) = ctx.unified_compute.lock() {
                    if self.node_population.len() == compute.num_nodes {
                        let k = self.simulation_params.spring_k_knowledge;
                        let o = self.simulation_params.spring_k_ontology;
                        let a = self.simulation_params.spring_k_agent;
                        let spring_scales: Vec<f32> = self
                            .node_population
                            .iter()
                            .map(|pop| match pop {
                                GraphPopulation::Knowledge => k,
                                GraphPopulation::Ontology => o,
                                GraphPopulation::Agent => a,
                            })
                            .collect();
                        if let Err(e) = compute.upload_spring_scale(&spring_scales) {
                            warn!("ForceComputeActor: spring_scale re-upload failed: {}", e);
                        } else {
                            info!(
                                "ForceComputeActor: spring_scale re-uploaded (k={:.2} o={:.2} a={:.2})",
                                k, o, a
                            );
                        }
                    }
                }
            }
        }

        // Recovery: a deliberate parameter change re-arms a divergence-halted
        // simulation. If the breaker was tripped, restore last-good positions and
        // zero velocities before resuming so the layout re-settles cleanly.
        if self.simulation_halted {
            self.recover_from_divergence();
        }
        self.clear_divergence_state();

        // Reset the broadcast timer so the next frame re-broadcasts a full snapshot
        // immediately. Without this, clients would wait for the rate-limit interval
        // before seeing the effect of parameter changes.
        self.broadcast_optimizer.reset_broadcast_timer();

        // Bypass GPU stability-skip for 1800 frames (~30 seconds at 60fps).
        // Previously 600 (~10s) — too short for dense graphs to fully re-layout under
        // new force parameters before check_system_stability_kernel re-suppressed physics.
        // 30s gives the system enough time to find the new equilibrium under large
        // repulsion/spring changes (Slice A audit, 2026-05-26).
        self.stability_warmup_remaining = 1800;

        // Scale reheat by the log-ratio of the LARGEST force-coefficient change in
        // either direction (repel_k or spring_k). A 10x bump produces reheat
        // ≈ 1 + ln(10)*2 = 5.6 → clamped to 5.0; a 1.5x change produces ≈ 1.8.
        // Direction-agnostic: weakening springs 40→6 must relayout just as much as
        // strengthening them. Capped at 5.0 to stay bounded by max_velocity.
        let repel_ratio = (new_repel_k / prior_repel_k).max(prior_repel_k / new_repel_k);
        let spring_ratio = (new_spring_k / prior_spring_k).max(prior_spring_k / new_spring_k);
        let ratio = repel_ratio.max(spring_ratio);
        let reheat = if ratio > 1.0 {
            (1.0 + ratio.ln() * 2.0).clamp(1.0, 5.0)
        } else {
            1.0
        };
        self.reheat_factor = reheat;

        // DO NOT suppress intermediate broadcasts on param change, and DO NOT fire a
        // premature full snapshot here. The whole graph is atomic: it either settles
        // or it doesn't, so the morph is shown via the continuous full-snapshot stream
        // every tick (reheat is applied on the NEXT step — a snapshot taken now would
        // just be the pre-reheat layout). The continuous branch streams full position
        // snapshots while the sim runs; the orchestrator fires one final
        // ForceFullBroadcast only when the graph reaches genuine, sustained rest.
        self.suppress_intermediate_broadcasts = false;
        self.force_full_broadcast = false;

        info!(
            "ForceComputeActor: Stability warmup=1800 (30s), reheat={:.2} (scaled by repel_k change); streaming full snapshots during settle",
            reheat
        );

        debug!(
            "ForceComputeActor: Parameters updated (iteration_count={}, stability={})",
            self.gpu_state.iteration_count, self.stability_iterations
        );

        Ok(())
    }
}

impl Handler<UpdateClusteringParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateClusteringParams, _ctx: &mut Self::Context) -> Self::Result {
        let Some(ctx) = &self.shared_context else {
            return Err("GPU context not initialized".to_string());
        };
        let mut compute = ctx
            .unified_compute
            .lock()
            .map_err(|e| format!("GPU lock poisoned: {}", e))?;
        compute.set_community_detector(&msg.algorithm, msg.resolution, msg.iterations);
        Ok(())
    }
}

/// Message to force a full broadcast of ALL node positions (bypass delta filter).
/// Sent by PhysicsOrchestratorActor after FastSettle convergence.
///
/// This performs an immediate position snapshot and broadcast WITHOUT running
/// another physics integration step.  Before this fix, the handler merely set a
/// flag and the orchestrator sent a follow-up `ComputeForces`, which ran one
/// more integration pass — slightly moving nodes after the convergence decision.
#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct ForceFullBroadcast;

impl Handler<ForceFullBroadcast> for ForceComputeActor {
    type Result = ResponseActFuture<Self, ()>;

    fn handle(&mut self, _msg: ForceFullBroadcast, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: ForceFullBroadcast received — reading current GPU positions for immediate broadcast");

        // Clear suppression state regardless of whether GPU is available
        self.force_full_broadcast = false;
        self.suppress_intermediate_broadcasts = false;
        self.broadcast_optimizer.reset_broadcast_timer();

        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                warn!("ForceComputeActor: ForceFullBroadcast — no GPU context, skipping");
                return Box::pin(futures::future::ready(()).into_actor(self));
            }
        };

        if self.gpu_state.num_nodes == 0 {
            warn!("ForceComputeActor: ForceFullBroadcast — 0 nodes, skipping");
            return Box::pin(futures::future::ready(()).into_actor(self));
        }

        let fut = async move {
            // Acquire GPU access (non-blocking tokio RwLock)
            let _gpu_guard = match shared_context.acquire_gpu_access().await {
                Ok(guard) => guard,
                Err(e) => {
                    warn!(
                        "ForceComputeActor: ForceFullBroadcast — failed to acquire GPU lock: {}",
                        e
                    );
                    return Err(());
                }
            };

            let unified_compute_arc = shared_context.unified_compute.clone();

            // Read positions and velocities on blocking thread — NO physics step
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED — recovering for ForceFullBroadcast position read. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                let positions_result = unified_compute.get_node_positions();
                let velocities_result = unified_compute.get_node_velocities();
                Ok((positions_result, velocities_result))
            }).await;

            match blocking_result {
                Ok(inner) => inner,
                Err(join_err) => {
                    warn!(
                        "ForceComputeActor: ForceFullBroadcast — spawn_blocking panicked: {}",
                        join_err
                    );
                    Err(())
                }
            }
        };

        Box::pin(fut.into_actor(self).map(move |result, actor, _ctx| {
            match result {
                Ok((Ok((pos_x, pos_y, pos_z)), Ok((vel_x, vel_y, vel_z)))) => {
                    // Mirror the main-loop dual-graph projection so the immediate
                    // snapshot is consistent with physics-stepped broadcasts. Without
                    // this, a settings change broadcasts raw GPU positions and, if the
                    // sim has converged, the projected positions never overwrite them.
                    let sep = actor.simulation_params.graph_separation_x;
                    let face_scale = clamp_z_scale(actor.simulation_params.axis_compression_z);
                    let project = actor.simulation_params.enable_dual_disc_layout
                        && !actor.node_population.is_empty();

                    // Mirror the main loop: per-population median centring + rim clamp.
                    let centroids = if project {
                        population_centroids_xy(&actor.node_population, |i| (pos_x[i], pos_y[i]), pos_x.len())
                    } else {
                        [(0.0f32, 0.0f32); 3]
                    };
                    // Rim radius is the disc's own fixed size, NOT sep, so the
                    // discs keep full size when the user pulls them close together.
                    let r_max = DISC_RIM_RADIUS;

                    let mut node_updates = Vec::with_capacity(pos_x.len());
                    for i in 0..pos_x.len() {
                        let mut position = Vec3::new(pos_x[i], pos_y[i], pos_z[i]);
                        let velocity = Vec3::new(vel_x[i], vel_y[i], vel_z[i]);
                        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
                            continue;
                        }
                        if project {
                            if let Some(&pop) = actor.node_population.get(i) {
                                project_node_xy(&mut position, pop, &centroids, sep, face_scale, r_max);
                            }
                        } else if (face_scale - 1.0).abs() > f32::EPSILON {
                            // Dual-disc OFF: apply the continuous Z compression only.
                            position.z *= face_scale;
                        }
                        let node_id = actor.gpu_index_to_node_id.get(i).copied().unwrap_or(i as u32);
                        node_updates.push((node_id, BinaryNodeDataClient::new(
                            node_id,
                            glam_to_vec3data(position),
                            glam_to_vec3data(velocity),
                        )));
                    }

                    if let Some(ref graph_addr) = actor.graph_service_addr {
                        info!(
                            "ForceComputeActor: IMMEDIATE full broadcast — {} nodes (pure snapshot, projected={} sep_y={:.1} face_scale={:.2})",
                            node_updates.len(), project, sep, face_scale
                        );
                        graph_addr.do_send(crate::actors::messages::UpdateNodePositions {
                            positions: node_updates,
                            correlation_id: Some(crate::actors::messaging::MessageId::new()),
                        });
                    }
                }
                _ => {
                    warn!("ForceComputeActor: ForceFullBroadcast — failed to read GPU positions/velocities");
                }
            }
        }))
    }
}

impl Handler<SetComputeMode> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: SetComputeMode, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: Setting compute mode to {:?}", msg.mode);

        self.compute_mode = msg.mode;

        let mut temp_params = self.unified_params;
        self.sync_simulation_to_unified_params(&mut temp_params);
        self.unified_params = temp_params;

        use futures::future::ready;
        Box::pin(ready(Ok(())).into_actor(self))
    }
}

impl Handler<GetPhysicsStats> for ForceComputeActor {
    type Result = Result<PhysicsStats, String>;

    fn handle(&mut self, _msg: GetPhysicsStats, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.get_physics_stats())
    }
}

impl Handler<UpdateAdvancedParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateAdvancedParams, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: UpdateAdvancedParams received");
        info!("  Advanced params - semantic_weight: {:.2}, temporal_weight: {:.2}, constraint_weight: {:.2}",
              msg.params.semantic_force_weight, msg.params.temporal_force_weight, msg.params.constraint_force_weight);

        // Write through to simulation_params (the canonical source) so that the
        // live physics step path — which clones simulation_params and rebuilds
        // SimParams via to_sim_params() — picks up these changes.
        if msg.params.semantic_force_weight > 0.0 {
            self.simulation_params.temperature *= msg.params.semantic_force_weight;
        }

        if msg.params.temporal_force_weight > 0.0 {
            self.simulation_params.alignment_strength *= msg.params.temporal_force_weight;
        }

        if msg.params.constraint_force_weight > 0.0 {
            self.simulation_params.cluster_strength *= msg.params.constraint_force_weight;
        }

        // Rebuild unified_params from the updated simulation_params so the
        // derived cache stays in sync.
        self.update_simulation_parameters(self.simulation_params.clone());

        info!("Advanced physics parameters written to simulation_params (canonical) and unified_params (cache)");

        if matches!(self.compute_mode, ComputeMode::Basic) {
            info!("ForceComputeActor: Switching to Advanced compute mode due to advanced params");
            self.compute_mode = ComputeMode::Advanced;
        }

        Ok(())
    }
}

// Position upload support for external updates
// Uses ResponseActFuture to allow spawn_blocking without blocking Tokio threads
impl Handler<UploadPositions> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: UploadPositions, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ForceComputeActor: UploadPositions received - {} nodes",
            msg.positions_x.len()
        );

        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                return Box::pin(
                    futures::future::ready(Err("GPU context not initialized".to_string()))
                        .into_actor(self),
                );
            }
        };

        // Clone data for move into spawn_blocking
        let positions_x = msg.positions_x;
        let positions_y = msg.positions_y;
        let positions_z = msg.positions_z;

        let fut = async move {
            let unified_compute_arc = shared_context.unified_compute.clone();

            // Move blocking GPU upload to dedicated blocking thread pool
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED — recovering for position upload. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                unified_compute
                    .update_positions_only(&positions_x, &positions_y, &positions_z)
                    .map_err(|e| format!("Failed to upload positions: {}", e))
            })
            .await;

            match blocking_result {
                Ok(inner_result) => inner_result,
                Err(join_err) => Err(format!("GPU blocking task panicked: {}", join_err)),
            }
        };

        Box::pin(fut.into_actor(self).map(|result, _actor, _ctx| {
            if result.is_ok() {
                info!("ForceComputeActor: Position upload completed successfully");
            }
            result
        }))
    }
}

/// Grab→spring: pin/unpin client-dragged nodes on the GPU compute buffer.
///
/// Pinned nodes are held at their client position every step (see
/// `apply_node_pins`) so the force kernel relaxes their neighbours around the
/// moved node. `reheat` injects a mild global velocity perturbation so a settled
/// graph has energy to visibly react to the grab (set on drag-start only).
impl Handler<PinNodePositions> for ForceComputeActor {
    type Result = ();

    fn handle(&mut self, msg: PinNodePositions, _ctx: &mut Self::Context) -> Self::Result {
        // Bookkeeping: apply pins/unpins to the map. Mark the GPU mask dirty when
        // membership changed so the next step re-uploads it (drag-end leaves the
        // node pinned in place until an explicit unpin arrives in msg.unpin).
        if Self::apply_pin_ops(&mut self.pinned_nodes, &msg.pins, &msg.unpin) {
            self.pinned_mask_dirty = true;
        }
        if msg.reheat {
            // Mild reheat: enough to give neighbours energy to spring, gentle enough
            // not to disturb the wider layout. Decays over ~230 steps (see step loop).
            self.reheat_factor = self.reheat_factor.max(0.3);
        }
        debug!(
            "ForceComputeActor: PinNodePositions — {} pinned total (this msg: +{} pins, -{} unpins, reheat={})",
            self.pinned_nodes.len(),
            msg.pins.len(),
            msg.unpin.len(),
            msg.reheat
        );
    }
}

// === Additional Message Handlers for Compatibility ===

impl Handler<InitializeGPU> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitializeGPU, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ForceComputeActor: InitializeGPU received with {} nodes, {} edges",
            msg.graph.nodes.len(),
            msg.graph.edges.len()
        );

        // NOTE: Do NOT set gpu_state.num_nodes here — only set it after successful GPU upload
        // in try_upload_pending_graph_data(). This prevents ComputeForces from running on
        // uninitialized GPU buffers (which causes a CUDA panic and mutex poisoning).

        if msg.graph_service_addr.is_some() {
            self.graph_service_addr = msg.graph_service_addr;
            info!("ForceComputeActor: GraphServiceActor address stored for position updates");
        }

        // Store physics orchestrator address for sequential pipeline back-channel
        if msg.physics_orchestrator_addr.is_some() && self.physics_orchestrator_addr.is_none() {
            self.physics_orchestrator_addr = msg.physics_orchestrator_addr.clone();
            info!("ForceComputeActor: PhysicsOrchestratorActor address stored for sequential pipeline");
        }

        // Store graph data for GPU upload
        self.pending_graph_data = Some(msg.graph);

        // Ensure GPU context is available before attempting upload
        if self.shared_context.is_none() {
            self.initialize_own_gpu_context();
        }
        self.try_upload_pending_graph_data();

        // Send GPUInitialized confirmation ONLY if graph data was successfully uploaded
        // to GPU (gpu_state.num_nodes > 0 means try_upload_pending_graph_data succeeded).
        // If shared_context is not yet available, the upload is deferred and
        // GPUInitialized will be sent later from try_upload_pending_graph_data()
        // when the context arrives via SetSharedGPUContext.
        if self.gpu_state.num_nodes > 0 {
            if let Some(ref orchestrator_addr) = msg.physics_orchestrator_addr {
                orchestrator_addr.do_send(crate::actors::messages::GPUInitialized);
                info!("ForceComputeActor: GPUInitialized confirmation sent to PhysicsOrchestratorActor");
            }
        } else if self.shared_context.is_none()
            && self.gpu_self_init_attempts >= self.gpu_self_init_max_retries
        {
            // GPU init permanently failed — notify orchestrator immediately so it
            // does not defer GPUInitialized indefinitely.
            error!(
                "ForceComputeActor: GPU context unavailable after {} init attempts — sending GPUInitFailed",
                self.gpu_self_init_attempts
            );
            if let Some(ref orchestrator_addr) = self.physics_orchestrator_addr {
                orchestrator_addr.do_send(crate::actors::messages::GPUInitFailed {
                    reason: format!(
                        "GPU self-init failed after {} attempts, shared_context is None",
                        self.gpu_self_init_attempts
                    ),
                    attempts: self.gpu_self_init_attempts,
                });
            }
        } else {
            info!("ForceComputeActor: Deferring GPUInitialized — graph not yet uploaded (shared_context={}, pending_data={}, init_attempts={}/{})",
                  self.shared_context.is_some(), self.pending_graph_data.is_some(),
                  self.gpu_self_init_attempts, self.gpu_self_init_max_retries);
        }

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            use crate::actors::messaging::MessageAck;
            if let Some(ref orchestrator_addr) = msg.physics_orchestrator_addr {
                orchestrator_addr.do_send(
                    MessageAck::success(correlation_id)
                        .with_metadata("nodes", self.gpu_state.num_nodes.to_string())
                        .with_metadata("edges", self.gpu_state.num_edges.to_string()),
                );
            }
        }

        Ok(())
    }
}

impl Handler<UpdateGPUGraphData> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGPUGraphData, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ForceComputeActor: UpdateGPUGraphData received with {} nodes, {} edges",
            msg.graph.nodes.len(),
            msg.graph.edges.len()
        );

        // Store graph data and attempt upload (num_nodes set only after successful upload)
        self.pending_graph_data = Some(msg.graph);
        if self.shared_context.is_none() {
            self.initialize_own_gpu_context();
        }
        self.try_upload_pending_graph_data();

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            debug!(
                "UpdateGPUGraphData completed with correlation_id: {}",
                correlation_id
            );
        }

        Ok(())
    }
}

impl Handler<GetNodeData> for ForceComputeActor {
    type Result = Result<Vec<crate::utils::socket_flow_messages::BinaryNodeData>, String>;

    fn handle(&mut self, _msg: GetNodeData, _ctx: &mut Self::Context) -> Self::Result {
        Ok(Vec::new())
    }
}

impl Handler<GetGPUStatus> for ForceComputeActor {
    type Result = GPUStatus;

    fn handle(&mut self, _msg: GetGPUStatus, _ctx: &mut Self::Context) -> Self::Result {
        GPUStatus {
            is_initialized: self.shared_context.is_some(),
            failure_count: self.gpu_state.gpu_failure_count,
            iteration_count: self.gpu_state.iteration_count,
            num_nodes: self.gpu_state.num_nodes,
        }
    }
}

impl Handler<GetCurrentPositions> for ForceComputeActor {
    type Result = Result<CurrentPositionsSnapshot, String>;

    fn handle(&mut self, _msg: GetCurrentPositions, _ctx: &mut Self::Context) -> Self::Result {
        if self.position_velocity_buffer.is_empty() {
            return Err("No GPU-computed positions available yet".to_string());
        }

        let num = self.position_velocity_buffer.len();
        let mut positions = Vec::with_capacity(num);
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut min_z = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut max_z = f32::MIN;
        let mut total_ke: f64 = 0.0;

        for (i, (pos, vel)) in self.position_velocity_buffer.iter().enumerate() {
            let node_id = self
                .gpu_index_to_node_id
                .get(i)
                .copied()
                .unwrap_or(i as u32);
            positions.push((node_id, pos.x, pos.y, pos.z));

            if pos.x < min_x {
                min_x = pos.x;
            }
            if pos.y < min_y {
                min_y = pos.y;
            }
            if pos.z < min_z {
                min_z = pos.z;
            }
            if pos.x > max_x {
                max_x = pos.x;
            }
            if pos.y > max_y {
                max_y = pos.y;
            }
            if pos.z > max_z {
                max_z = pos.z;
            }

            let v2 = (vel.x as f64).powi(2) + (vel.y as f64).powi(2) + (vel.z as f64).powi(2);
            total_ke += 0.5 * v2;
        }

        let avg_ke = if num > 0 { total_ke / num as f64 } else { 0.0 };
        // Honest settlement: the pause latch (energy plateau) or a sub-epsilon
        // frame run — not the instantaneous single-frame KE. `avg_ke` (this
        // frame's mean per-node KE) is still reported as the live energy.
        let (settled, stable_frame_count) = Self::settled_state(
            self.settle_paused,
            self.settle_stable_frames,
            Self::SETTLE_FRAME_THRESHOLD,
        );

        Ok(CurrentPositionsSnapshot {
            positions,
            num_nodes: num as u32,
            settled,
            kinetic_energy: avg_ke,
            stable_frame_count,
            bounding_box: BoundingBox {
                min_x,
                min_y,
                min_z,
                max_x,
                max_y,
                max_z,
            },
        })
    }
}

/// Cheap settlement telemetry query — no position payload, just the honest
/// convergence readout for the REST `/api/graph/data` status panel.
impl Handler<crate::actors::messages::GetSettlementState> for ForceComputeActor {
    type Result = Result<crate::actors::messages::SettlementSnapshot, String>;

    fn handle(
        &mut self,
        _msg: crate::actors::messages::GetSettlementState,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(self.settlement_snapshot())
    }
}

/// Settlement pause/resume latch driven by the PhysicsOrchestratorActor. Keeps
/// the telemetry truthful once the physics loop stops ticking at convergence.
impl Handler<crate::actors::messages::SetPhysicsSettled> for ForceComputeActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::SetPhysicsSettled,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        self.set_settlement_paused(msg.settled);
    }
}

impl Handler<GetGPUMetrics> for ForceComputeActor {
    type Result = Result<serde_json::Value, String>;

    fn handle(&mut self, _msg: GetGPUMetrics, _ctx: &mut Self::Context) -> Self::Result {
        use serde_json::json;

        Ok(json!({
            "memory_usage_mb": 0.0,
            "gpu_utilization": 0.0,
            "temperature_c": 0.0,
            "power_usage_w": 0.0,
            "compute_units": 0,
            "max_threads": 0,
            "clock_speed_mhz": 0,
        }))
    }
}

impl Handler<RunCommunityDetection> for ForceComputeActor {
    type Result = Result<CommunityDetectionResult, String>;

    fn handle(&mut self, _msg: RunCommunityDetection, _ctx: &mut Self::Context) -> Self::Result {
        Err("Community detection should be handled by ClusteringActor".to_string())
    }
}

impl Handler<UpdateVisualAnalyticsParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: UpdateVisualAnalyticsParams,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: UpdateVisualAnalyticsParams received (no-op, handled by other actors)");
        Ok(())
    }
}

impl Handler<GetConstraints> for ForceComputeActor {
    type Result = Result<visionclaw_domain::models::constraints::ConstraintSet, String>;

    fn handle(&mut self, _msg: GetConstraints, _ctx: &mut Self::Context) -> Self::Result {
        Err("Constraints should be handled by ConstraintActor".to_string())
    }
}

impl Handler<UpdateConstraints> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateConstraints, _ctx: &mut Self::Context) -> Self::Result {
        // ADR-098 break #4/#5: parse the JSON constraint payload into the
        // lossless GPU ConstraintData buffer and cache it. The next physics step
        // uploads it via apply_ontology_forces → set_constraints (the live path).
        let constraints: Vec<crate::models::constraints::ConstraintData> =
            serde_json::from_value(msg.constraint_data)
                .map_err(|e| format!("Failed to parse constraint_data: {}", e))?;

        info!(
            "ForceComputeActor: UpdateConstraints cached {} constraints for the live force_pass_kernel loop",
            constraints.len()
        );
        self.cached_constraint_buffer = constraints;
        Ok(())
    }
}

impl Handler<UploadConstraintsToGPU> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UploadConstraintsToGPU, _ctx: &mut Self::Context) -> Self::Result {
        // ADR-098 break #4/#5: cache the buffer through the canonical
        // cached_constraint_buffer + UpdateOntologyConstraintBuffer seam, then
        // upload immediately via the LOSSLESS set_constraints writer if the GPU
        // is available; otherwise the next physics step picks it up.
        info!(
            "ForceComputeActor: UploadConstraintsToGPU caching {} constraints for the live constraint loop",
            msg.constraint_data.len()
        );
        self.cached_constraint_buffer = msg.constraint_data;

        if let Err(e) = self.apply_ontology_forces() {
            warn!(
                "ForceComputeActor: immediate constraint upload deferred: {}",
                e
            );
        }
        Ok(())
    }
}

impl Handler<TriggerStressMajorization> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: TriggerStressMajorization,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Err("Stress majorization should be handled by StressMajorizationActor".to_string())
    }
}

impl Handler<GetStressMajorizationStats> for ForceComputeActor {
    type Result =
        Result<crate::actors::gpu::stress_majorization_actor::StressMajorizationStats, String>;

    fn handle(
        &mut self,
        _msg: GetStressMajorizationStats,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Err(
            "Stress majorization stats should be retrieved from StressMajorizationActor"
                .to_string(),
        )
    }
}

impl Handler<ResetStressMajorizationSafety> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: ResetStressMajorizationSafety,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Err(
            "Stress majorization safety reset should be handled by StressMajorizationActor"
                .to_string(),
        )
    }
}

impl Handler<UpdateStressMajorizationParams> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: UpdateStressMajorizationParams,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: UpdateStressMajorizationParams received (forwarding to StressMajorizationActor would be done by GPUManagerActor)");
        Ok(())
    }
}

impl Handler<PerformGPUClustering> for ForceComputeActor {
    type Result = Result<Vec<crate::handlers::api_handler::analytics::Cluster>, String>;

    fn handle(&mut self, _msg: PerformGPUClustering, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: PerformGPUClustering received - forwarding to ClusteringActor would be done by GPUManagerActor");

        Err("Clustering should be handled by ClusteringActor, not ForceComputeActor".to_string())
    }
}

impl Handler<GetClusteringResults> for ForceComputeActor {
    type Result = Result<serde_json::Value, String>;

    fn handle(&mut self, _msg: GetClusteringResults, _ctx: &mut Self::Context) -> Self::Result {
        info!("ForceComputeActor: GetClusteringResults received - forwarding to ClusteringActor would be done by GPUManagerActor");

        Err(
            "Clustering results should be retrieved from ClusteringActor, not ForceComputeActor"
                .to_string(),
        )
    }
}

/// Handler for UpdateOntologyConstraintBuffer
/// Updates the cached constraint buffer when ontology constraints change
impl Handler<crate::actors::messages::UpdateOntologyConstraintBuffer> for ForceComputeActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::UpdateOntologyConstraintBuffer,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!(
            "ForceComputeActor: Received updated ontology constraint buffer with {} constraints",
            msg.constraint_buffer.len()
        );

        // Update the cached constraint buffer
        self.cached_constraint_buffer = msg.constraint_buffer;

        debug!("ForceComputeActor: Ontology constraint buffer cached, will be uploaded to GPU on next physics step");
    }
}

impl Handler<SetSharedGPUContext> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        let had_context = self.shared_context.is_some();
        if had_context {
            info!("ForceComputeActor: Received SharedGPUContext from supervisor chain (replacing self-initialized context)");
        } else {
            info!("ForceComputeActor: Received SharedGPUContext from supervisor chain");
        }

        // Accept the externally-provided context. This replaces any self-created
        // context and ensures all GPU actors share the same CUDA device/stream,
        // which is important for clustering and analytics actors that also need
        // the same SharedGPUContext.
        self.shared_context = Some(msg.context);

        if let Some(addr) = msg.graph_service_addr {
            self.graph_service_addr = Some(addr);
            info!("ForceComputeActor: GraphServiceActor address stored for position broadcasts");
        } else if self.graph_service_addr.is_none() {
            debug!("ForceComputeActor: No GraphServiceActor address provided with context");
        }

        self.gpu_state.is_initialized = true;

        info!("ForceComputeActor: SharedGPUContext stored successfully — GPU physics enabled");

        // If graph data was received before the context, upload it now
        if self.pending_graph_data.is_some() {
            info!("ForceComputeActor: Pending graph data found — uploading to GPU now");
            self.try_upload_pending_graph_data();
        }

        info!(
            "ForceComputeActor: Physics can now run with {} nodes and {} edges",
            self.gpu_state.num_nodes, self.gpu_state.num_edges
        );

        // H4: Send acknowledgment
        if let Some(correlation_id) = msg.correlation_id {
            debug!(
                "SetSharedGPUContext completed with correlation_id: {}",
                correlation_id
            );
        }

        Ok(())
    }
}

/// Handler for SetPhysicsOrchestratorAddr — wires up the back-channel for the
/// sequential physics pipeline so that PhysicsStepCompleted messages flow back
/// to the orchestrator after each GPU step.
impl Handler<crate::actors::messages::SetPhysicsOrchestratorAddr> for ForceComputeActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::SetPhysicsOrchestratorAddr,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: PhysicsOrchestratorActor address set for sequential pipeline");
        self.physics_orchestrator_addr = Some(msg.addr);
    }
}

/// Handler for ResetPositions — re-randomizes all positions on a uniform 3D sphere,
/// re-uploads to GPU, and reheats the simulation so it re-converges from a fresh layout.
impl Handler<crate::actors::messages::ResetPositions> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: crate::actors::messages::ResetPositions,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let ctx = match &self.shared_context {
            Some(c) => c.clone(),
            None => {
                warn!(
                    "ForceComputeActor: ResetPositions received but GPU context is not initialized"
                );
                return Err("GPU context not available".to_string());
            }
        };

        let num_nodes = self.gpu_state.num_nodes as usize;
        if num_nodes == 0 {
            warn!("ForceComputeActor: ResetPositions received but no nodes are loaded");
            return Err("No nodes loaded in GPU".to_string());
        }

        // Generate uniform sphere distribution for all nodes. Seed the sphere
        // inside the soft-cube envelope (viewport_bounds, the single source of
        // the layout scale) so the reset re-settles compactly instead of
        // spraying to a radius unrelated to the configured bounds. Fall back to
        // a node-count heuristic only when bounds are disabled.
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let sphere_radius = if self.simulation_params.viewport_bounds > 0.0 {
            self.simulation_params.viewport_bounds * 0.6
        } else {
            (num_nodes as f32).cbrt() * 50.0 + 100.0
        };

        let mut positions_x = Vec::with_capacity(num_nodes);
        let mut positions_y = Vec::with_capacity(num_nodes);
        let mut positions_z = Vec::with_capacity(num_nodes);

        for _ in 0..num_nodes {
            // Rejection sampling for uniform sphere distribution
            loop {
                let x: f32 = rng.gen_range(-1.0f32..1.0f32);
                let y: f32 = rng.gen_range(-1.0f32..1.0f32);
                let z: f32 = rng.gen_range(-1.0f32..1.0f32);
                let r2 = x * x + y * y + z * z;
                if r2 <= 1.0 && r2 > 0.0 {
                    let r = r2.sqrt();
                    positions_x.push(x / r * sphere_radius * rng.gen_range(0.1f32..1.0f32));
                    positions_y.push(y / r * sphere_radius * rng.gen_range(0.1f32..1.0f32));
                    positions_z.push(z / r * sphere_radius * rng.gen_range(0.1f32..1.0f32));
                    break;
                }
            }
        }

        // Upload randomized positions to GPU
        let mut compute = match ctx.unified_compute.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                error!("ForceComputeActor: ResetPositions — GPU mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };

        compute
            .upload_positions(&positions_x, &positions_y, &positions_z)
            .map_err(|e| format!("Failed to upload reset positions to GPU: {}", e))?;

        drop(compute);

        // Trigger a full reheat so the simulation re-explores from the new positions
        self.stability_warmup_remaining = 600;
        self.reheat_factor = 1.0;
        self.suppress_intermediate_broadcasts = false;
        self.force_full_broadcast = true;
        self.stability_iterations = 0;
        self.gpu_state.iteration_count = 0;

        info!(
            "ForceComputeActor: Positions reset to uniform sphere (r={:.1}, {} nodes) — full reheat triggered",
            sphere_radius, num_nodes
        );

        Ok(())
    }
}

/// Handler for ConfigureStressMajorization message
impl Handler<ConfigureStressMajorization> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        msg: ConfigureStressMajorization,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: ConfigureStressMajorization received");

        // Store stress majorization configuration in unified params
        // These parameters affect graph layout optimization
        if let Some(learning_rate) = msg.learning_rate {
            info!("  Setting learning_rate: {:.3}", learning_rate);
            // Apply learning rate to temperature for optimization
            self.unified_params.temperature = learning_rate * 100.0;
        }

        if let Some(momentum) = msg.momentum {
            info!("  Setting momentum: {:.3}", momentum);
            // Momentum affects velocity damping
            self.unified_params.damping = 1.0 - momentum;
        }

        if let Some(max_iterations) = msg.max_iterations {
            info!("  Setting max_iterations: {}", max_iterations);
            // This would be used by stress majorization algorithm
            // For now, we log it as it affects the optimization convergence
        }

        if let Some(auto_run_interval) = msg.auto_run_interval {
            info!("  Setting auto_run_interval: {} frames", auto_run_interval);
            // Auto-run interval affects periodic layout optimization
        }

        info!("ForceComputeActor: Stress majorization configuration applied");
        Ok(())
    }
}

/// Handler for GetStressMajorizationConfig message
impl Handler<GetStressMajorizationConfig> for ForceComputeActor {
    type Result = Result<StressMajorizationConfig, String>;

    fn handle(
        &mut self,
        _msg: GetStressMajorizationConfig,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: GetStressMajorizationConfig received");

        // Return current stress majorization configuration based on unified params
        let config = StressMajorizationConfig {
            learning_rate: self.unified_params.temperature / 100.0,
            momentum: 1.0 - self.unified_params.damping,
            max_iterations: 100,                        // Default value
            auto_run_interval: 60,                      // Default: every 60 frames
            current_stress: 0.0,                        // Would be computed from current layout
            converged: self.stability_iterations > 600, // Converged after stability
            iterations_completed: self.gpu_state.iteration_count as usize,
        };

        info!("ForceComputeActor: Returning stress majorization config (learning_rate: {:.3}, momentum: {:.3})",
              config.learning_rate, config.momentum);

        Ok(config)
    }
}

// =============================================================================
// Phase 7: Broadcast Optimization Message Handlers
// =============================================================================

/// Handler for ConfigureBroadcastOptimization
impl Handler<crate::actors::messages::ConfigureBroadcastOptimization> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        msg: crate::actors::messages::ConfigureBroadcastOptimization,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        info!("ForceComputeActor: ConfigureBroadcastOptimization received");

        // Get current stats before update
        let old_stats = self.broadcast_optimizer.get_performance_stats();

        // Build new config from current + updates.
        // NOTE: `delta_threshold` is accepted on the wire for backward
        // compatibility but ignored — the broadcast is full-snapshot only
        // (BROADCAST-001). There is no delta path to configure.
        if msg.delta_threshold.is_some() {
            info!("  delta_threshold ignored — broadcast is full-snapshot only (BROADCAST-001)");
        }
        let new_config = BroadcastConfig {
            target_fps: msg.target_fps.unwrap_or(old_stats.target_fps),
            enable_spatial_culling: msg.enable_spatial_culling.unwrap_or(false),
            camera_bounds: None, // Updated separately via UpdateCameraFrustum
        };

        // Validate parameters
        if new_config.target_fps == 0 || new_config.target_fps > 60 {
            return Err(format!(
                "Invalid target_fps: {} (must be 1-60)",
                new_config.target_fps
            ));
        }

        info!(
            "  Target FPS: {} -> {}",
            old_stats.target_fps, new_config.target_fps
        );
        info!("  Spatial culling: {}", new_config.enable_spatial_culling);

        // Apply new configuration
        self.broadcast_optimizer.update_config(new_config);

        Ok(())
    }
}

/// Handler for UpdateCameraFrustum
impl Handler<crate::actors::messages::UpdateCameraFrustum> for ForceComputeActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        msg: crate::actors::messages::UpdateCameraFrustum,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        debug!(
            "ForceComputeActor: UpdateCameraFrustum received - min: {:?}, max: {:?}",
            msg.min, msg.max
        );

        let min = Vec3::new(msg.min.0, msg.min.1, msg.min.2);
        let max = Vec3::new(msg.max.0, msg.max.1, msg.max.2);
        self.broadcast_optimizer.update_camera_bounds(min, max);
        Ok(())
    }
}

/// Handler for GetBroadcastStats
impl Handler<crate::actors::messages::GetBroadcastStats> for ForceComputeActor {
    type Result = Result<crate::actors::messages::BroadcastPerformanceStats, String>;

    fn handle(
        &mut self,
        _msg: crate::actors::messages::GetBroadcastStats,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        let stats = self.broadcast_optimizer.get_performance_stats();

        // Convert from gpu::broadcast_optimizer::BroadcastPerformanceStats
        // to actors::messages::BroadcastPerformanceStats
        Ok(crate::actors::messages::BroadcastPerformanceStats {
            total_frames_processed: stats.total_frames_processed,
            total_nodes_sent: stats.total_nodes_sent,
            total_nodes_processed: stats.total_nodes_processed,
            average_bandwidth_reduction: stats.average_bandwidth_reduction,
            target_fps: stats.target_fps,
            // Retained on the wire for compatibility; broadcast is full-snapshot
            // only, so there is no delta threshold — always reported as 0.0.
            delta_threshold: 0.0,
        })
    }
}

// =============================================================================
// Phase 5: GPU Backpressure - Token Bucket Flow Control Handler
// =============================================================================

/// Handler for RunAnomalyDetection - delegates anomaly detection to GPU compute
/// Supports LocalOutlierFactor (LOF) and ZScore methods via the unified GPU compute engine
impl Handler<RunAnomalyDetection> for ForceComputeActor {
    type Result = ResponseActFuture<Self, Result<AnomalyResult, String>>;

    fn handle(&mut self, msg: RunAnomalyDetection, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ForceComputeActor: RunAnomalyDetection received for method {:?}",
            msg.params.method
        );

        let shared_context = match &self.shared_context {
            Some(ctx) => ctx.clone(),
            None => {
                return Box::pin(
                    futures::future::ready(Err("GPU context not initialized".to_string()))
                        .into_actor(self),
                );
            }
        };

        if self.gpu_state.num_nodes == 0 {
            return Box::pin(
                futures::future::ready(Err("No graph data uploaded to GPU".to_string()))
                    .into_actor(self),
            );
        }

        let params = msg.params;
        let num_nodes = self.gpu_state.num_nodes;
        let start_time = Instant::now();

        let fut = async move {
            let unified_compute_arc = shared_context.unified_compute.clone();

            type AnomalyBlockingResult = (
                Option<Vec<f32>>,
                Option<Vec<f32>>,
                Vec<crate::actors::gpu::anomaly_detection_actor::AnomalyNode>,
                f32,
                AnomalyDetectionMethod,
            );

            let blocking_result = tokio::task::spawn_blocking(move || -> Result<AnomalyBlockingResult, String> {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        error!("ForceComputeActor: GPU mutex was POISONED — recovering for anomaly detection. GPU state may be corrupt.");
                        poisoned.into_inner()
                    }
                };

                match params.method {
                    AnomalyMethod::LocalOutlierFactor => {
                        let lof_result = unified_compute
                            .run_lof_anomaly_detection(params.k_neighbors, params.threshold)
                            .map_err(|e| format!("GPU LOF detection failed: {}", e))?;

                        let lof_scores = lof_result.0;
                        let mut anomalies = Vec::new();

                        for (node_id, &score) in lof_scores.iter().enumerate() {
                            if score > params.threshold {
                                anomalies.push(
                                    crate::actors::gpu::anomaly_detection_actor::AnomalyNode {
                                        node_id: node_id as u32,
                                        anomaly_score: score,
                                        reason: format!(
                                            "LOF score {:.3} exceeds threshold {:.3}",
                                            score, params.threshold
                                        ),
                                        anomaly_type: "outlier".to_string(),
                                        severity: if score > params.threshold * 3.0 {
                                            "high"
                                        } else {
                                            "medium"
                                        }
                                        .to_string(),
                                        explanation: format!(
                                            "LOF anomaly detected with score {:.3}",
                                            score
                                        ),
                                        features: vec![
                                            "lof_score".to_string(),
                                            "local_density".to_string(),
                                        ],
                                    },
                                );
                            }
                        }

                        Ok((
                            Some(lof_scores),
                            None::<Vec<f32>>,
                            anomalies,
                            params.threshold,
                            AnomalyDetectionMethod::LOF,
                        ))
                    }
                    AnomalyMethod::ZScore => {
                        let feature_data = params.feature_data.unwrap_or_else(|| {
                            (0..num_nodes)
                                .map(|i| {
                                    (i as f32 + 1.0) / num_nodes as f32
                                        + (i as f32).sin() * 0.1
                                        + (i as f32).cos() * 0.05
                                })
                                .collect()
                        });

                        let z_scores = unified_compute
                            .run_zscore_anomaly_detection(&feature_data)
                            .map_err(|e| format!("GPU Z-Score detection failed: {}", e))?;

                        let mut anomalies = Vec::new();

                        for (node_id, &score) in z_scores.iter().enumerate() {
                            let abs_score = score.abs();
                            if abs_score > params.threshold {
                                anomalies.push(
                                    crate::actors::gpu::anomaly_detection_actor::AnomalyNode {
                                        node_id: node_id as u32,
                                        anomaly_score: abs_score,
                                        reason: format!(
                                            "Z-score {:.3} exceeds threshold {:.3}",
                                            abs_score, params.threshold
                                        ),
                                        anomaly_type: "statistical_outlier".to_string(),
                                        severity: if abs_score > params.threshold * 2.0 {
                                            "high"
                                        } else {
                                            "medium"
                                        }
                                        .to_string(),
                                        explanation: format!(
                                            "Statistical anomaly detected with Z-score {:.3}",
                                            score
                                        ),
                                        features: vec![
                                            "z_score".to_string(),
                                            "statistical_deviation".to_string(),
                                        ],
                                    },
                                );
                            }
                        }

                        Ok((
                            None::<Vec<f32>>,
                            Some(z_scores),
                            anomalies,
                            params.threshold,
                            AnomalyDetectionMethod::ZScore,
                        ))
                    }
                }
            })
            .await;

            match blocking_result {
                Ok(inner_result) => {
                    let (lof_scores, zscore_values, anomalies, threshold, method) = inner_result?;
                    let computation_time = start_time.elapsed();
                    let anomalies_count = anomalies.len();
                    let avg_score = if !anomalies.is_empty() {
                        anomalies.iter().map(|a| a.anomaly_score).sum::<f32>()
                            / anomalies.len() as f32
                    } else {
                        0.0
                    };
                    let max_score = anomalies
                        .iter()
                        .map(|a| a.anomaly_score)
                        .fold(0.0f32, f32::max);
                    let min_score = anomalies
                        .iter()
                        .map(|a| a.anomaly_score)
                        .fold(f32::INFINITY, f32::min);

                    Ok(AnomalyResult {
                        lof_scores,
                        local_densities: None,
                        zscore_values,
                        anomaly_threshold: threshold,
                        num_anomalies: anomalies_count,
                        anomalies,
                        stats: AnomalyDetectionStats {
                            total_nodes_analyzed: num_nodes,
                            anomalies_found: anomalies_count,
                            detection_threshold: threshold,
                            computation_time_ms: computation_time.as_millis() as u64,
                            method: method.clone(),
                            average_anomaly_score: avg_score,
                            max_anomaly_score: max_score,
                            min_anomaly_score: if min_score == f32::INFINITY {
                                0.0
                            } else {
                                min_score
                            },
                        },
                        method,
                        threshold,
                    })
                }
                Err(join_err) => Err(format!("GPU blocking task panicked: {}", join_err)),
            }
        };

        Box::pin(fut.into_actor(self).map(|result, _actor, _ctx| result))
    }
}

/// Handler for PositionBroadcastAck - replenishes tokens when network confirms delivery
/// This implements token bucket flow control between GPU producer and network consumer
impl Handler<crate::actors::messages::PositionBroadcastAck> for ForceComputeActor {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::PositionBroadcastAck,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        // Acknowledge to backpressure controller - this restores tokens
        self.backpressure
            .acknowledge(msg.clients_delivered as usize);

        // Log token restoration at debug level (every 300 acks to avoid spam)
        if msg.correlation_id.is_multiple_of(300) {
            let metrics = self.backpressure.metrics();
            debug!("ForceComputeActor: Broadcast ack received (correlation_id: {}, clients: {}), tokens: {}/{}, congestion: {:.1}ms",
                   msg.correlation_id, msg.clients_delivered,
                   metrics.available_tokens, metrics.max_tokens,
                   metrics.total_congestion_duration.as_secs_f32() * 1000.0);
        }
    }
}

#[cfg(test)]
mod pinning_tests {
    //! Pure, actor-free tests for the pinned-node mask bookkeeping (PHASE 1):
    //! pin/unpin map mutation and GPU mask construction, tested without a live
    //! actor or GPU context (mirrors the settlement_tests precedent).
    use super::ForceComputeActor as FCA;
    use glam::Vec3;
    use std::collections::HashMap;

    #[test]
    fn pin_inserts_and_reports_change() {
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        let changed = FCA::apply_pin_ops(&mut pinned, &[(7, [1.0, 2.0, 3.0])], &[]);
        assert!(changed, "inserting a new pin must report a change");
        assert_eq!(pinned.get(&7), Some(&Vec3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn repinning_same_position_is_not_a_change() {
        // A per-frame drag update that repeats the exact position must not mark the
        // mask dirty (avoids a pointless GPU re-upload every drag frame).
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        FCA::apply_pin_ops(&mut pinned, &[(7, [1.0, 2.0, 3.0])], &[]);
        let changed = FCA::apply_pin_ops(&mut pinned, &[(7, [1.0, 2.0, 3.0])], &[]);
        assert!(
            !changed,
            "re-pinning identical position must not report a change"
        );
    }

    #[test]
    fn moving_a_pin_reports_change() {
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        FCA::apply_pin_ops(&mut pinned, &[(7, [1.0, 2.0, 3.0])], &[]);
        let changed = FCA::apply_pin_ops(&mut pinned, &[(7, [9.0, 9.0, 9.0])], &[]);
        assert!(
            changed,
            "moving a pinned node to a new position must report a change"
        );
        assert_eq!(pinned.get(&7), Some(&Vec3::new(9.0, 9.0, 9.0)));
    }

    #[test]
    fn unpin_removes_and_reports_change() {
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        FCA::apply_pin_ops(&mut pinned, &[(7, [1.0, 2.0, 3.0])], &[]);
        let changed = FCA::apply_pin_ops(&mut pinned, &[], &[7]);
        assert!(changed, "removing an existing pin must report a change");
        assert!(pinned.is_empty());
    }

    #[test]
    fn unpin_absent_node_is_not_a_change() {
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        let changed = FCA::apply_pin_ops(&mut pinned, &[], &[42]);
        assert!(
            !changed,
            "unpinning a node that was never pinned is a no-op"
        );
    }

    #[test]
    fn pin_and_unpin_in_one_op() {
        // A single message may both pin some nodes and unpin others.
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        FCA::apply_pin_ops(&mut pinned, &[(1, [0.0; 3]), (2, [0.0; 3])], &[]);
        let changed = FCA::apply_pin_ops(&mut pinned, &[(3, [0.0; 3])], &[1]);
        assert!(changed);
        assert!(pinned.contains_key(&2));
        assert!(pinned.contains_key(&3));
        assert!(!pinned.contains_key(&1));
    }

    #[test]
    fn mask_sets_bit_only_for_pinned_gpu_indices() {
        // node_ids[i] is the graph node ID at GPU index i. The mask bit is set at
        // the GPU index whose node ID is pinned — regardless of ID ordering.
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        FCA::apply_pin_ops(&mut pinned, &[(20, [0.0; 3]), (40, [0.0; 3])], &[]);
        let node_ids = [10u32, 20, 30, 40, 50];
        let mask = FCA::build_pinned_mask(&node_ids, &pinned);
        assert_eq!(mask, vec![0, 1, 0, 1, 0]);
    }

    #[test]
    fn empty_pin_set_yields_all_zero_mask() {
        let pinned: HashMap<u32, Vec3> = HashMap::new();
        let mask = FCA::build_pinned_mask(&[1, 2, 3], &pinned);
        assert_eq!(mask, vec![0, 0, 0]);
    }

    #[test]
    fn pinned_id_absent_from_gpu_map_sets_no_bit() {
        // A stale pin for a node no longer in the graph must not set any bit and
        // must not panic — the mask length always matches the GPU node count.
        let mut pinned: HashMap<u32, Vec3> = HashMap::new();
        FCA::apply_pin_ops(&mut pinned, &[(999, [0.0; 3])], &[]);
        let mask = FCA::build_pinned_mask(&[1, 2, 3], &pinned);
        assert_eq!(mask, vec![0, 0, 0]);
    }
}

#[cfg(test)]
mod dag_rank_tests {
    //! Pure, actor-free tests for the PHASE 2 DAG rank BFS: root detection,
    //! layered depth, cycle-safety, unreachable/unranked handling.
    use super::ForceComputeActor as FCA;
    use std::collections::HashMap;

    #[test]
    fn empty_hierarchy_yields_all_unranked() {
        let ranks = FCA::compute_dag_ranks(4, &[]);
        assert_eq!(ranks, vec![-1.0, -1.0, -1.0, -1.0]);
    }

    // ── ADR-2035 amendment (N-14): rank from provenance, not the force label ──

    const SUBCLASS: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
    const EQUIVALENT: &str = "http://www.w3.org/2002/07/owl#equivalentClass";
    const SUB_PROPERTY: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";

    /// The edge `relation_edges` writes for an `is-a` frontmatter relation:
    /// the collapsed force label plus the predicate it was folded from.
    fn ingested(
        source: u32,
        target: u32,
        predicate: &str,
    ) -> visionclaw_domain::models::edge::Edge {
        visionclaw_domain::models::edge::Edge::new(source, target, 1.6)
            .with_edge_type("hierarchical".to_string())
            .with_owl_property_iri(predicate.to_string())
    }

    fn identity_indices(ids: &[u32]) -> HashMap<u32, usize> {
        ids.iter().enumerate().map(|(i, &id)| (id, i)).collect()
    }

    #[test]
    fn a_domain_root_never_ranks_below_its_own_members() {
        // The live inversion. Members A ⊑ B (an asserted subclass edge), and the
        // synthetic domain root that `materialise_domain_roots` hangs over both.
        // Membership is not subsumption: the root must not become a CHILD of
        // the pages it groups.
        const A: u32 = 1;
        const B: u32 = 2;
        const ROOT: u32 = 900;
        let edges = vec![
            ingested(A, B, SUBCLASS),
            crate::services::github_sync_service::domain_membership_edge(ROOT, A),
            crate::services::github_sync_service::domain_membership_edge(ROOT, B),
        ];
        let idx = identity_indices(&[A, B, ROOT]);
        let ranks = FCA::compute_dag_ranks(3, &FCA::hierarchy_pairs(&edges, &idx));

        assert_eq!(ranks[idx[&B]], 0.0, "B is the superclass: the top layer");
        assert_eq!(ranks[idx[&A]], 1.0, "A ⊑ B sits one layer down");
        assert!(
            !(ranks[idx[&ROOT]] > ranks[idx[&A]] || ranks[idx[&ROOT]] > ranks[idx[&B]]),
            "domain root ranked below its members: {ranks:?}"
        );
        assert_eq!(
            ranks[idx[&ROOT]], -1.0,
            "membership contributes no layer, so the root takes no DAG bias"
        );
    }

    #[test]
    fn folded_non_subsumption_predicates_do_not_rank() {
        // The ingest folds owl:equivalentClass and rdfs:subPropertyOf into the
        // same `hierarchical` force label as rdfs:subClassOf. Neither is class
        // subsumption (one is symmetric, the other is a property hierarchy), so
        // the provenance must keep them out of the rank space.
        let edges = vec![ingested(1, 2, EQUIVALENT), ingested(3, 4, SUB_PROPERTY)];
        let idx = identity_indices(&[1, 2, 3, 4]);
        assert!(FCA::hierarchy_pairs(&edges, &idx).is_empty());
    }

    #[test]
    fn asserted_and_inferred_subclass_edges_rank_child_below_parent() {
        // Asserted (ingest) and entailed (Whelk materialiser) subsumption both
        // carry the subClassOf provenance and rank as (parent, child).
        let edges = vec![
            ingested(1, 2, SUBCLASS),
            crate::services::inferred_edge_materialiser::build_inferred_edge(3, 2),
        ];
        let idx = identity_indices(&[1, 2, 3]);
        let mut pairs = FCA::hierarchy_pairs(&edges, &idx);
        pairs.sort();
        assert_eq!(pairs, vec![(1, 0), (1, 2)]);
    }

    #[test]
    fn simple_chain_ranks_by_depth() {
        // (parent, child): 0→1→2→3. Root 0 = rank 0, then 1,2,3.
        let ranks = FCA::compute_dag_ranks(4, &[(0, 1), (1, 2), (2, 3)]);
        assert_eq!(ranks, vec![0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn tree_assigns_shared_depth_to_siblings() {
        // 0 is root; 1 and 2 are its children (rank 1); 3 is child of 1 (rank 2).
        let ranks = FCA::compute_dag_ranks(4, &[(0, 1), (0, 2), (1, 3)]);
        assert_eq!(ranks, vec![0.0, 1.0, 1.0, 2.0]);
    }

    #[test]
    fn unreachable_node_stays_unranked() {
        // Node 3 participates in no hierarchy edge → rank -1 (no bias).
        let ranks = FCA::compute_dag_ranks(4, &[(0, 1), (1, 2)]);
        assert_eq!(ranks, vec![0.0, 1.0, 2.0, -1.0]);
    }

    #[test]
    fn diamond_takes_shortest_depth() {
        // 0→1, 0→2, 1→3, 2→3. Node 3 reachable at depth 2 via both paths.
        let ranks = FCA::compute_dag_ranks(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);
        assert_eq!(ranks[3], 2.0);
        assert_eq!(ranks[0], 0.0);
    }

    #[test]
    fn back_edge_does_not_inflate_or_loop() {
        // 0→1→2 plus a back-edge 2→0. Root 0 is never a child (the back-edge makes
        // 0 a child too), so this is a pure cycle → deterministic seed at index 0.
        let ranks = FCA::compute_dag_ranks(3, &[(0, 1), (1, 2), (2, 0)]);
        // Terminates (cycle-safe) and every participant gets a finite layered rank.
        assert_eq!(ranks[0], 0.0);
        assert_eq!(ranks[1], 1.0);
        assert_eq!(ranks[2], 2.0);
    }

    #[test]
    fn multiple_roots_form_a_forest() {
        // Two independent trees: 0→1 and 2→3. Both roots at rank 0.
        let ranks = FCA::compute_dag_ranks(4, &[(0, 1), (2, 3)]);
        assert_eq!(ranks, vec![0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn out_of_range_indices_are_ignored() {
        // A stale edge referencing index >= num_nodes must not panic.
        let ranks = FCA::compute_dag_ranks(2, &[(0, 1), (5, 9)]);
        assert_eq!(ranks, vec![0.0, 1.0]);
    }

    #[test]
    fn only_subsumption_provenance_ranks() {
        // ADR-2035, amended 2026-10-02. The 2026-09-05 contract accepted the
        // bare `hierarchical` label and claimed no consumer could tell a
        // subclass edge from anything else folded into it. That was wrong: the
        // ingest keeps the folded predicate in `owl_property_iri`. Rank on that.
        let edge = |label: &str, iri: Option<&str>| {
            let e = visionclaw_domain::models::edge::Edge::new(1, 2, 1.0)
                .with_edge_type(label.to_string());
            match iri {
                Some(i) => e.with_owl_property_iri(i.to_string()),
                None => e,
            }
        };
        for label in ["is_subclass_of", "subclass_of", "SUBCLASS_OF"] {
            assert!(
                edge(label, None).asserts_subsumption(),
                "{label} names the relation"
            );
        }
        for label in ["hierarchical", "HIERARCHICAL"] {
            assert!(
                edge(label, Some(SUBCLASS)).asserts_subsumption(),
                "{label} folded from rdfs:subClassOf ranks"
            );
            assert!(
                !edge(label, None).asserts_subsumption(),
                "{label} with no provenance is a force category, not a relation"
            );
            for iri in [
                EQUIVALENT,
                SUB_PROPERTY,
                "http://www.w3.org/2002/07/owl#sameAs",
                "https://narrativegoldmine.com/ns/v1#instanceOf",
            ] {
                assert!(
                    !edge(label, Some(iri)).asserts_subsumption(),
                    "{label} folded from {iri} must not rank"
                );
            }
        }
        for label in [
            "domain_member",
            "chain_payment",
            "equivalent_class",
            "same_as",
            "sub_property_of",
            "relates_to",
            "has_part",
            "namespace",
            "member_of",
            "",
        ] {
            assert!(
                !edge(label, Some(SUBCLASS)).asserts_subsumption(),
                "{label}"
            );
        }
    }

    #[test]
    fn a_subclass_fixture_ranks_by_depth_from_its_root() {
        // Entity -> Animal -> {Dog, Cat}, as the ingest writes it (child -> parent).
        const ENTITY: u32 = 10;
        const ANIMAL: u32 = 11;
        const DOG: u32 = 12;
        const CAT: u32 = 13;
        let edges = vec![
            ingested(ANIMAL, ENTITY, SUBCLASS),
            ingested(DOG, ANIMAL, SUBCLASS),
            ingested(CAT, ANIMAL, SUBCLASS),
        ];
        let idx = identity_indices(&[ENTITY, ANIMAL, DOG, CAT]);
        let ranks = FCA::compute_dag_ranks(4, &FCA::hierarchy_pairs(&edges, &idx));
        assert_eq!(ranks[idx[&ENTITY]], 0.0, "the root sits at rank 0");
        assert_eq!(ranks[idx[&ANIMAL]], 1.0);
        assert_eq!(ranks[idx[&DOG]], 2.0, "depth from the nearest root");
        assert_eq!(ranks[idx[&CAT]], 2.0, "siblings share a layer");
    }

    #[test]
    fn membership_edges_neither_rank_nor_shortcut_a_subclass_chain() {
        // Before the amendment a membership edge under `hierarchical` joined the
        // rank space, and shortest-depth BFS let it lift a deep class a layer.
        // Domain root R groups every node of the chain C0 <- C1 <- C2.
        const R: u32 = 99;
        let edges = vec![
            ingested(1, 0, SUBCLASS),
            ingested(2, 1, SUBCLASS),
            crate::services::github_sync_service::domain_membership_edge(R, 0),
            crate::services::github_sync_service::domain_membership_edge(R, 1),
            crate::services::github_sync_service::domain_membership_edge(R, 2),
        ];
        let idx = identity_indices(&[0, 1, 2, R]);
        let ranks = FCA::compute_dag_ranks(4, &FCA::hierarchy_pairs(&edges, &idx));
        assert_eq!(ranks, vec![0.0, 1.0, 2.0, -1.0]);
    }

    #[test]
    fn shortest_depth_wins_when_two_subclass_paths_reach_a_node() {
        // A BFS property, independent of labels: a node reachable at two depths
        // takes the shallower one.
        //  0 -> 1 -> 2 -> 3, plus 0 -> 3
        let ranks = FCA::compute_dag_ranks(4, &[(0, 1), (1, 2), (2, 3), (0, 3)]);
        assert_eq!(ranks, vec![0.0, 1.0, 2.0, 1.0]);
    }
    #[test]
    fn nodes_outside_any_hierarchy_edge_stay_unranked() {
        // Rank -1 means "apply no radial bias", which is how a disconnected or
        // non-hierarchical node opts out of the layout term entirely.
        let ranks = FCA::compute_dag_ranks(4, &[(0, 1)]);
        assert_eq!(ranks[0], 0.0);
        assert_eq!(ranks[1], 1.0);
        assert_eq!(ranks[2], -1.0, "not in any hierarchy edge");
        assert_eq!(ranks[3], -1.0);

        // An empty hierarchy leaves everything unranked rather than seeding a
        // spurious root.
        assert_eq!(FCA::compute_dag_ranks(3, &[]), vec![-1.0, -1.0, -1.0]);
    }

    #[test]
    fn a_wholly_cyclic_hierarchy_is_seeded_deterministically() {
        // Ingest is not guaranteed acyclic: a pure cycle has no natural root, so
        // the lowest participating index is seeded rather than leaving the whole
        // component unranked. Rank is a layout projection, not a proof of DAG-ness.
        let ranks = FCA::compute_dag_ranks(3, &[(0, 1), (1, 2), (2, 0)]);
        assert_eq!(ranks[0], 0.0, "lowest index seeded as the root");
        assert_eq!(ranks[1], 1.0);
        assert_eq!(ranks[2], 2.0);
        assert!(ranks.iter().all(|r| *r >= 0.0), "no node left unranked");
    }
}

#[cfg(test)]
mod settlement_tests {
    //! Pure, actor-free tests for the settlement decision logic (epsilon /
    //! threshold / counter-reset semantics), following the `ingest::process_frame`
    //! precedent of testing the transition function without a live actor.
    use super::ForceComputeActor as FCA;

    const EPS: f64 = FCA::SETTLE_KE_EPSILON;
    const THRESH: u32 = FCA::SETTLE_FRAME_THRESHOLD;

    #[test]
    fn sub_epsilon_ke_increments_the_run() {
        // A tick below epsilon advances the consecutive-stable counter by one.
        assert_eq!(FCA::next_stable_frames(0, EPS / 2.0, EPS), 1);
        assert_eq!(FCA::next_stable_frames(41, 0.0, EPS), 42);
    }

    #[test]
    fn ke_at_or_above_epsilon_resets_the_run() {
        // Epsilon is a strict floor: exactly-epsilon does NOT count as settled.
        assert_eq!(FCA::next_stable_frames(100, EPS, EPS), 0);
        // A reheat (KE spikes well above epsilon) zeroes the counter.
        assert_eq!(FCA::next_stable_frames(179, 5.0, EPS), 0);
    }

    #[test]
    fn non_finite_ke_resets_the_run() {
        // Skip/failure sentinels (`f64::MAX`, NaN, inf) must never be read as
        // "settled" — they reset the run so a stalled pipeline can't fake rest.
        assert_eq!(FCA::next_stable_frames(150, f64::MAX, EPS), 0);
        assert_eq!(FCA::next_stable_frames(150, f64::NAN, EPS), 0);
        assert_eq!(FCA::next_stable_frames(150, f64::INFINITY, EPS), 0);
    }

    #[test]
    fn settled_only_at_or_past_threshold_when_running() {
        // Not paused: settlement is driven purely by the sub-epsilon counter.
        assert_eq!(
            FCA::settled_state(false, THRESH - 1, THRESH),
            (false, THRESH - 1)
        );
        assert_eq!(FCA::settled_state(false, THRESH, THRESH), (true, THRESH));
        assert_eq!(
            FCA::settled_state(false, THRESH + 50, THRESH),
            (true, THRESH + 50)
        );
    }

    #[test]
    fn pause_latch_reports_settled_with_full_count() {
        // The plateau-pause path: the loop stopped at a residual KE far above
        // epsilon (counter == 0), yet the graph IS at rest. Must report settled
        // with the count pinned to the threshold for a self-consistent readout.
        assert_eq!(FCA::settled_state(true, 0, THRESH), (true, THRESH));
        // A larger live count is preserved rather than clamped down.
        assert_eq!(
            FCA::settled_state(true, THRESH + 5, THRESH),
            (true, THRESH + 5)
        );
    }

    #[test]
    fn reheat_flips_settled_false_and_resets_counter() {
        // Drive the counter to settled, then perturb (springK reheat): KE rises,
        // the counter resets, and the graph is no longer settled — the exact
        // live semantics the telemetry must show during a perturbation.
        let mut frames = 0u32;
        for _ in 0..THRESH {
            frames = FCA::next_stable_frames(frames, 0.0, EPS);
        }
        assert_eq!(frames, THRESH);
        assert_eq!(FCA::settled_state(false, frames, THRESH), (true, THRESH));

        // Reheat.
        frames = FCA::next_stable_frames(frames, 12.5, EPS);
        assert_eq!(frames, 0);
        assert_eq!(FCA::settled_state(false, frames, THRESH), (false, 0));

        // Re-settling then re-accumulates from zero.
        frames = FCA::next_stable_frames(frames, EPS / 10.0, EPS);
        assert_eq!(frames, 1);
        assert_eq!(FCA::settled_state(false, frames, THRESH), (false, 1));
    }

    #[test]
    fn pause_then_resume_end_to_end() {
        // Mirrors the live defect the queen caught: plateau at KE≈2.9 (counter 0),
        // loop pauses → must read settled; then a springK reheat resumes ticks →
        // must read unsettled with the counter reset.
        let mut ctor = FcaSettlement::default();

        // Converging ticks keep KE above epsilon → counter stays 0, not settled.
        ctor.tick(2.9039521);
        assert_eq!(ctor.snapshot(), (false, 0, 2.9039521));

        // Orchestrator declares the energy plateau and pauses the loop.
        ctor.pause(true);
        assert_eq!(ctor.snapshot(), (true, THRESH, 2.9039521));

        // 60s later, still paused, no ticks: readout stays honestly settled
        // (was previously frozen at false).
        assert_eq!(ctor.snapshot(), (true, THRESH, 2.9039521));

        // springK perturbation → resume. First live tick clears the latch and
        // reports the rising energy as unsettled.
        ctor.pause(false);
        assert_eq!(ctor.snapshot(), (false, 0, 2.9039521));
        ctor.tick(26.1);
        assert_eq!(ctor.snapshot(), (false, 0, 26.1));
    }

    /// Actor-free replica of the settlement fields + transitions, so the
    /// pause/tick interplay is exercised without a live GPU actor.
    #[derive(Default)]
    struct FcaSettlement {
        frames: u32,
        ke: f64,
        paused: bool,
    }
    impl FcaSettlement {
        fn tick(&mut self, mean_ke: f64) {
            self.ke = mean_ke;
            self.frames = FCA::next_stable_frames(self.frames, mean_ke, EPS);
            self.paused = false;
        }
        fn pause(&mut self, paused: bool) {
            self.paused = paused;
            if !paused {
                self.frames = 0;
            }
        }
        fn snapshot(&self) -> (bool, u32, f64) {
            let (s, f) = FCA::settled_state(self.paused, self.frames, THRESH);
            (s, f, self.ke)
        }
    }

    #[test]
    fn counter_saturates_without_overflow() {
        // A graph parked at rest indefinitely must not panic on counter overflow.
        assert_eq!(FCA::next_stable_frames(u32::MAX, 0.0, EPS), u32::MAX);
    }
}

#[cfg(test)]
mod z_scale_tests {
    use super::{clamp_z_scale, project_node_xy, GraphPopulation, Vec3, Z_SCALE_MIN};

    #[test]
    fn clamp_z_scale_bounds() {
        assert_eq!(clamp_z_scale(1.0), 1.0); // default: no compression
        assert_eq!(clamp_z_scale(0.5), 0.5); // mid: honoured
        assert_eq!(clamp_z_scale(0.0), Z_SCALE_MIN); // below floor → clamped
        assert_eq!(clamp_z_scale(-3.0), Z_SCALE_MIN);
        assert_eq!(clamp_z_scale(2.0), 1.0); // above 1.0 → clamped
    }

    #[test]
    fn face_scale_one_preserves_z_in_disc_mode() {
        // Agent population sits on the z=0 mid-plane (target_z = 0), so with
        // face_scale = 1.0 the Z coordinate is unchanged → fully 3D.
        let centroids = [(0.0f32, 0.0f32); 3];
        let mut p = Vec3::new(1.0, 2.0, 7.5);
        project_node_xy(&mut p, GraphPopulation::Agent, &centroids, 0.0, 1.0, 0.0);
        assert!((p.z - 7.5).abs() < 1e-6);
    }

    #[test]
    fn face_scale_derived_from_axis_compression_flattens_disc() {
        // face_scale = clamp_z_scale(axis_compression_z). At 0.1 the disc is thin:
        // Agent z scales by 0.1 (target_z = 0).
        let centroids = [(0.0f32, 0.0f32); 3];
        let mut p = Vec3::new(0.0, 0.0, 10.0);
        let face_scale = clamp_z_scale(0.1);
        project_node_xy(
            &mut p,
            GraphPopulation::Agent,
            &centroids,
            0.0,
            face_scale,
            0.0,
        );
        assert!((p.z - 1.0).abs() < 1e-6);
    }
}
