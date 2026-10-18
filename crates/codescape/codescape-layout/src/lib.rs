//! Codescape layout engines.
//!
//! This crate positions a [`GraphSnapshot`] in 3-D. There are two engines
//! behind one trait, [`LayoutEngine`]:
//!
//! * [`CpuEngine`] runs Barnes-Hut repulsion over an octree, CSR springs and
//!   the same damped semi-implicit Euler integrator as the GPU kernel, on rayon.
//!   It is always available, and it is the conformance oracle for the GPU
//!   engine (ADR-2061 analytics-oracle pattern).
//! * `CudaEngine` (feature `cuda`) is a thin cust 0.3.2 driver over the
//!   build-compiled `visionclaw_unified` PTX from `visionclaw-gpu`, read at run
//!   time from `$CODESCAPE_PTX_DIR` ([`ptx_source`]; see *Obtaining the PTX*
//!   below). It refuses PTX whose ABI or build provenance does not match what
//!   it was written against ([`abi`]). `CODESCAPE_CUDA_DEVICE` picks the device ordinal. Set
//!   `CUDA_DEVICE_ORDER=PCI_BUS_ID` so the ordinal means what `nvidia-smi` shows.
//!   Its steps are pipelined with the caller (see [`StepStats`]).
//!
//! [`select_engine`] picks one at boot and returns the reason as a string, for
//! `/api/health`.
//!
//! # Obtaining the PTX
//!
//! This crate does not build or link `visionclaw-gpu`; the `cuda` feature
//! needs only `cust`. `CODESCAPE_PTX_DIR` must name a directory holding two
//! files from one `visionclaw-gpu` build: `visionclaw_unified.ptx` and
//! `ptx-build-manifest.txt`. That build's script writes both into its Cargo
//! `OUT_DIR`. In the VisionClaw dev container (`target/` is the
//! `visionclaw-cargo-target-cache` volume, profile `dev-runtime`) that is
//!
//! ```text
//! /app/target/dev-runtime/build/visionclaw-gpu-<hash>/out/
//! ```
//!
//! The `<hash>` changes with the build, and stale siblings from earlier builds
//! stay behind, so do not glob for it. Copy the two files to a stable
//! directory from the same build and point `CODESCAPE_PTX_DIR` at that copy.
//! The gitignored source-tree copy in `crates/visionclaw-gpu/src/ptx/` has
//! no manifest, so it is refused. The PTX also targets the `sm_` architecture
//! it was built for (`CUDA_ARCH`), so a device below that cannot load it.
//! When the artefacts are missing or fail a check, [`select_engine`] returns
//! the CPU engine and says why.
//!
//! # The physical model
//!
//! Both engines simulate the same set of **bodies** and compute the same
//! host-side terms from one shared model module, and they integrate the same
//! way. They differ only in how repulsion is computed.
//!
//! * **Bodies.** Every L2 symbol is a body, and so is any L0/L1 node that has
//!   no members, such as a file with no extracted symbols. Directories and
//!   files *with* members are **aggregates**. Aggregates are not simulated:
//!   after every step they are placed at the centroid of their members, with
//!   the same L2 → L1 → L0 rule as [`codescape_model::lod::aggregate_positions`],
//!   except that a body is never overwritten.
//! * **Repulsion** between bodies is `repulsion / (d² + ε)`.
//!   * The CPU engine sums it over a Barnes-Hut octree with opening angle `theta`.
//!   * The CUDA engine sums it over `force_pass_kernel`'s uniform grid up to a
//!     cut-off of `shell_radius / 3`. Beyond that, the host adds a far field,
//!     summed over file centroids (weighted by member count) and orphan bodies
//!     binned into cut-off-sized cells. It is computed in the background from
//!     the previous step's positions, so it is one step old.
//! * **Springs** join bodies along graph edges. A spring has zero rest length,
//!   and its stiffness is `spring_k × edge.weight × m`, where `m` is chosen from
//!   the relation and the endpoints' communities:
//!   * `m = structural_weight` when the relation is structural (`contains`,
//!     `method`, `case_of`, `defines`);
//!   * `m = intra_community_weight` when both endpoints share a non-zero community;
//!   * `m = inter_community_weight` otherwise.
//! * **Containment.** Each body whose parent is an aggregate is pulled towards
//!   that aggregate's current centroid with stiffness
//!   `spring_k × structural_weight`. This keeps files compact.
//! * **Aggregate edges.** An edge with an aggregate endpoint (file → file
//!   `imports`, directory → file `contains`) acts between the endpoints'
//!   current positions. Its force on an aggregate is shared equally among the
//!   aggregate's member bodies.
//! * **Community cohesion** pulls each body towards its community's centroid
//!   with stiffness `gravity × COMMUNITY_COHESION_PER_GRAVITY`.
//! * **Gravity** pulls every body towards the origin with stiffness `gravity`.
//! * **Integration.** Both engines use `integrate_pass_kernel`'s scheme: clamp
//!   the force to `max_force`, then `v ← clamp((v + F·dt)·damping, max_velocity)`,
//!   then `x ← x + v·dt`. With exact forces on both sides (θ = 0.3 on the CPU,
//!   a cut-off covering the whole layout on the GPU), a 20k-symbol graph
//!   settled in 4921 and 4877 steps.
//!
//! Containment, aggregate edges and cohesion reach the device through
//! `cluster_cohesion_kernel`. Each body is its own "cluster", with target
//! `pos + F_host` at strength 1, so the term adds `F_host` exactly.
//!
//! # Conformance
//!
//! The CPU engine is the oracle. Different numerics alone reshuffle symbols
//! inside a file: the CPU engine against itself, with a 10% smaller `dt`,
//! keeps only about 0.73 of each symbol's 10 nearest neighbours. The device
//! test therefore requires file-level k-NN overlap ≥ 0.9, and symbol-level
//! overlap no worse than that CPU self-baseline minus 0.1. Measured on a
//! 2k-symbol graph after 400 steps: 0.868 (symbols) and 1.000 (files),
//! against a baseline of 0.734 and 0.980.
//!
//! Step count to [`LayoutEngine::settled`] is not a reproducible per-run
//! quantity. After the steep early decay, a layout spends thousands of steps
//! on a plateau where whole files keep rearranging, and over 90% of the
//! remaining kinetic energy is in whole-file motion. On a 20k-symbol graph,
//! the CPU engine with only its layout seed changed settled 3686 vs 5507
//! steps apart. Step-count parity is therefore tested where the decay is
//! steep: the first step at which the 32-step average of the mean squared
//! speed falls below 0.2, compared by median over four seeds.
//!
//! # Example
//!
//! ```
//! use codescape_layout::{CpuEngine, LayoutEngine, LayoutParams};
//! use codescape_model::GraphSnapshot;
//!
//! let snapshot = GraphSnapshot::default();
//! let mut engine = CpuEngine::new(LayoutParams::default());
//! engine.load(&snapshot, None).unwrap();
//! let stats = engine.step(10);
//! assert_eq!(stats.steps, 10);
//! assert!(engine.positions().ids.is_empty());
//! ```

#![cfg_attr(not(feature = "cuda"), forbid(unsafe_code))]
#![cfg_attr(feature = "cuda", deny(unsafe_code))]
#![warn(missing_docs)]
// The physics loops run `for k in 0..3` over x/y/z and index several
// 3-vectors in lockstep; iterator zips would obscure them.
#![allow(clippy::needless_range_loop)]

pub mod abi;
mod cpu;
#[cfg(feature = "cuda")]
#[allow(unsafe_code)]
mod cuda;
mod knn;
mod model;
mod octree;
pub mod ptx_source;
mod seed;

use codescape_model::{GraphDelta, GraphSnapshot};
use serde::{Deserialize, Serialize};

pub use cpu::CpuEngine;
#[cfg(feature = "cuda")]
pub use cuda::{CudaEngine, CUTOFF_PER_SHELL_RADIUS, DEVICE_ENV};
pub use knn::knn_overlap;
pub use model::COMMUNITY_COHESION_PER_GRAVITY;
pub use seed::hierarchical_seed;

/// Tunables shared by every engine. [`Default`] gives values that settle a
/// typical repository graph from a [`hierarchical_seed`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutParams {
    /// Repulsion constant `k` in `k / (d² + ε)` between every pair of bodies.
    pub repulsion: f32,
    /// Base spring stiffness. Every spring and the containment pull scale with it.
    pub spring_k: f32,
    /// Spring multiplier for structural relations (`contains`, `method`, `case_of`, `defines`).
    pub structural_weight: f32,
    /// Spring multiplier for semantic edges whose endpoints share a community.
    pub intra_community_weight: f32,
    /// Spring multiplier for every other semantic edge.
    pub inter_community_weight: f32,
    /// Stiffness of the pull towards the origin. Community cohesion is
    /// `gravity × COMMUNITY_COHESION_PER_GRAVITY`.
    pub gravity: f32,
    /// Velocity retained per step, in `0..=1` (1 = no damping).
    pub damping: f32,
    /// Integration time step.
    pub dt: f32,
    /// Speed limit for any body, in world units per unit time. It also bounds
    /// the net force on a body, at `max_velocity / dt`.
    pub max_velocity: f32,
    /// Barnes-Hut opening angle: a cell of width `s` at distance `d` is
    /// approximated by its centre of mass when `s / d < theta`. 0 = exact.
    /// The default, 0.5, tracks exact repulsion: on a 20k-symbol graph its
    /// early energy decay was within 1% of θ = 0.3 and of the CUDA engine with
    /// exact repulsion, where 0.8 ran 18% slower. It costs about 1.7× the
    /// step time of 0.8.
    pub theta: f32,
    /// Mean squared body speed below which a step counts as calm.
    /// [`LayoutEngine::settled`] needs [`SETTLE_STEPS`] calm steps in a row.
    pub settle_energy: f32,
    /// Radius of the shell that [`hierarchical_seed`] places top-level directories on.
    pub shell_radius: f32,
    /// Seed for every random draw (seed placement, jitter for new nodes).
    pub seed: u64,
}

impl Default for LayoutParams {
    /// The defaults, including `theta = 0.5` (approved by the queen). On a
    /// 20k-symbol graph its early energy decay was within 1% of exact
    /// repulsion, where 0.8 ran 18% slow and broke CPU/CUDA step parity.
    fn default() -> Self {
        Self {
            repulsion: 400.0,
            spring_k: 0.02,
            structural_weight: 4.0,
            intra_community_weight: 1.0,
            inter_community_weight: 0.25,
            gravity: 0.002,
            damping: 0.85,
            dt: 0.5,
            max_velocity: 40.0,
            theta: 0.5,
            settle_energy: 0.05,
            shell_radius: 600.0,
            seed: 0x00C0_DE5C_A9E0,
        }
    }
}

impl LayoutParams {
    /// The cap on the net force on one body, `max_velocity / dt`.
    pub fn max_force(&self) -> f32 {
        if self.dt > 0.0 {
            self.max_velocity / self.dt
        } else {
            self.max_velocity
        }
    }
}

/// Positions and velocities of every node of the loaded snapshot.
///
/// `ids`, `pos` and `vel` are aligned with the `nodes` order of the snapshot
/// passed to the last [`LayoutEngine::load`] or [`LayoutEngine::apply_delta`].
/// Aggregates carry the centroid of their members and a zero velocity.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PositionSet {
    /// Node ids, in snapshot order.
    pub ids: Vec<u32>,
    /// World positions, aligned with `ids`.
    pub pos: Vec<[f32; 3]>,
    /// Velocities, aligned with `ids` (zero for aggregates).
    pub vel: Vec<[f32; 3]>,
}

impl PositionSet {
    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// `true` when there are no nodes.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Position of `id`, scanning linearly (build a map for loops).
    pub fn position_of(&self, id: u32) -> Option<[f32; 3]> {
        self.ids.iter().position(|&i| i == id).map(|i| self.pos[i])
    }
}

/// What one call to [`LayoutEngine::step`] did.
///
/// The CUDA engine overlaps each step's device work with the caller: a call
/// launches its steps and returns once the last one is queued, and the next
/// call waits for it. Its `energy`, `max_displacement` and device times
/// therefore describe the steps that *finished* during the call, which is
/// everything launched except the last step. Device times are measured with
/// CUDA events, so on a shared GPU they include time the device spent on
/// other processes.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct StepStats {
    /// Σ |v|² over the bodies integrated in the last step.
    pub energy: f32,
    /// Largest distance any body moved during the call.
    pub max_displacement: f32,
    /// Steps taken.
    pub steps: u32,
    /// Wall-clock time of the whole call, in milliseconds.
    pub millis: f32,
    /// Host time in the force terms and set-up that run on the CPU, in
    /// milliseconds. On the CPU engine this is the whole step.
    #[serde(default)]
    pub host_ms: f32,
    /// Device time of the host → device copies (CUDA engine; 0 on the CPU).
    #[serde(default)]
    pub upload_ms: f32,
    /// Device time of the kernels (CUDA engine; 0 on the CPU).
    #[serde(default)]
    pub kernel_ms: f32,
    /// Device time of the device → host copies (CUDA engine; 0 on the CPU).
    #[serde(default)]
    pub download_ms: f32,
    /// Host time spent blocked on the device (CUDA engine; 0 on the CPU).
    /// On a GPU shared with other processes this is where contention shows.
    #[serde(default)]
    pub wait_ms: f32,
}

/// Why a layout operation failed.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum LayoutError {
    /// The snapshot breaks a model invariant (see `GraphSnapshot::validate`).
    #[error("invalid snapshot: {0}")]
    InvalidSnapshot(String),
    /// The delta does not lead to the snapshot it came with, or nothing is loaded yet.
    #[error("delta mismatch: {0}")]
    DeltaMismatch(String),
    /// The PTX does not match the ABI this crate was written against, or its
    /// provenance is not a fresh compile.
    #[error("PTX ABI mismatch: {0}")]
    AbiMismatch(String),
    /// A CUDA driver call failed, or the PTX artefacts in `CODESCAPE_PTX_DIR`
    /// could not be read.
    #[error("CUDA: {0}")]
    Device(String),
}

/// A force-directed layout engine over a code graph.
pub trait LayoutEngine: Send {
    /// Engine name: `"cpu"` or `"cuda"`.
    fn name(&self) -> &'static str;

    /// Replace the graph. Nodes found in `warm` (by id) keep their position and
    /// velocity. The others are placed near their parent, or by
    /// [`hierarchical_seed`] when they have none.
    fn load(
        &mut self,
        snapshot: &GraphSnapshot,
        warm: Option<&PositionSet>,
    ) -> Result<(), LayoutError>;

    /// Move to `snapshot_after`, the result of applying `delta` to the loaded
    /// graph. Surviving nodes keep their position. New nodes start at their
    /// parent's position plus a small jitter. The springs are rebuilt.
    fn apply_delta(
        &mut self,
        snapshot_after: &GraphSnapshot,
        delta: &GraphDelta,
    ) -> Result<(), LayoutError>;

    /// Restrict integration to these ids and their 1-hop neighbours. An
    /// aggregate id stands for all its member bodies. Every other body is
    /// frozen exactly, and still exerts its forces. `None` lifts the mask.
    fn set_focus_mask(&mut self, ids: Option<&[u32]>);

    /// Advance `n` steps.
    fn step(&mut self, n: u32) -> StepStats;

    /// Current positions, aligned with the loaded snapshot's node order.
    fn positions(&self) -> &PositionSet;

    /// `true` once the mean squared speed has stayed below `settle_energy`
    /// for [`SETTLE_STEPS`] consecutive steps.
    fn settled(&self) -> bool;
}

/// Number of consecutive calm steps after which an engine reports [`LayoutEngine::settled`].
pub const SETTLE_STEPS: u32 = 5;

/// Choose an engine. CUDA is used when `prefer_cuda` is set, the crate was
/// built with the `cuda` feature, a device exists and the PTX passes the ABI
/// and provenance checks. Otherwise the CPU engine is returned. The string
/// says which engine was picked and why.
///
/// ```
/// use codescape_layout::{select_engine, LayoutParams};
///
/// let (engine, reason) = select_engine(LayoutParams::default(), false);
/// assert_eq!(engine.name(), "cpu");
/// assert!(reason.starts_with("cpu"));
/// ```
pub fn select_engine(params: LayoutParams, prefer_cuda: bool) -> (Box<dyn LayoutEngine>, String) {
    if !prefer_cuda {
        return (
            Box::new(CpuEngine::new(params)),
            "cpu: CUDA not requested".to_string(),
        );
    }
    #[cfg(feature = "cuda")]
    {
        match CudaEngine::new(params) {
            Ok(engine) => {
                let reason = format!("cuda: {}", engine.describe());
                (Box::new(engine), reason)
            }
            Err(e) => (
                Box::new(CpuEngine::new(params)),
                format!("cpu: CUDA unavailable ({e})"),
            ),
        }
    }
    #[cfg(not(feature = "cuda"))]
    {
        (
            Box::new(CpuEngine::new(params)),
            "cpu: built without the `cuda` feature".to_string(),
        )
    }
}
