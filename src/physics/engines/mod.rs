//! `LayoutEngine` trait and the five concrete engine implementations
//! (ADR-01 D5). All engines share the `PhysicsGpuBuffers` owned by
//! `ForceComputeActor`; no engine allocates its own device memory.
//!
//! This module is gated behind the `physics-v2` Cargo feature. The legacy
//! `if/else` dispatch in `ForceComputeActor` (currently routed through
//! `src/layout/engines.rs::compute_layout`) remains the production path until
//! the actor is migrated to hold `Box<dyn LayoutEngine>`.
//!
//! Phasing:
//!
//! - Trait + stub `step` implementations land first (this commit) so the
//!   surface is reviewable in isolation.
//! - Engine bodies will be wired to the existing kernel launchers
//!   (`force_pass_kernel`, `integrate_pass_kernel`, `StressMajorizationSolver`,
//!   etc.) in a follow-up commit that also migrates `ForceComputeActor`.
//! - 2D engines (`Hierarchical`, `Circular`, `Geographic`) compute positions
//!   on the CPU and upload once per `step`; they do not require an integrate
//!   pass and their `convergence_metric` returns 0.0 immediately.

#![cfg(feature = "physics-v2")]

use anyhow::Result;

use crate::gpu::buffers::PhysicsGpuBuffers;
use crate::models::simulation_params::SimParams;

pub mod circular;
pub mod force_directed;
pub mod geographic;
pub mod hierarchical;
pub mod stress_majorization;

pub use circular::CircularEngine;
pub use force_directed::ForceDirectedEngine;
pub use geographic::GeographicEngine;
pub use hierarchical::HierarchicalEngine;
pub use stress_majorization::StressMajorizationEngine;

/// One-step layout engine. Implementations may dispatch CUDA kernels via the
/// shared `PhysicsGpuBuffers`, run CPU-side algorithms that upload results, or
/// any combination. The trait deliberately exposes no engine-specific state —
/// per-engine configuration travels via `SimParams` and the engine's
/// constructor.
pub trait LayoutEngine: Send + Sync {
    /// Execute one integration step. Force-directed engines run a force pass
    /// plus an integrate pass; CPU-side engines compute positions and copy
    /// them into the device buffers.
    fn step(&self, buffers: &mut PhysicsGpuBuffers, params: &SimParams) -> Result<()>;

    /// True if this engine produces three-dimensional positions; false for
    /// 2-D planar engines (`Hierarchical`, `Circular`, `Geographic`).
    fn supports_3d(&self) -> bool;

    /// Scalar convergence metric used by `ForceComputeActor`'s settlement
    /// hysteresis (PRD-01 A1). Typically RMS velocity over the last tick.
    /// CPU-placement engines return `0.0` because their layout is computed in
    /// one pass and is considered immediately settled.
    fn convergence_metric(&self, buffers: &PhysicsGpuBuffers) -> f32;

    /// Engine identifier used in logs, metrics, and `LayoutStarted` events.
    fn name(&self) -> &'static str;
}

/// Engine registry. Maps the canonical `LayoutMode` enum to a concrete
/// `Box<dyn LayoutEngine>`. The mapping is exhaustive on the legacy
/// `LayoutMode` variants so that adding a new variant is a compile-time error.
pub fn engine_for(mode: crate::layout::types::LayoutMode) -> Box<dyn LayoutEngine> {
    use crate::layout::types::LayoutMode;
    match mode {
        LayoutMode::ForceDirected => Box::new(ForceDirectedEngine::new()),
        LayoutMode::Hierarchical => Box::new(HierarchicalEngine::new()),
        // The legacy LayoutMode set (`Radial`, `Spectral`, `Temporal`,
        // `Clustered`) predates ADR-01 D5's five-engine registry. The
        // mapping below maps legacy variants onto the ADR-01 set; the
        // legacy variants will be retired in a follow-up commit that
        // unifies the public LayoutMode enum on the registry below.
        LayoutMode::Radial => Box::new(CircularEngine::new()),
        LayoutMode::Spectral => Box::new(StressMajorizationEngine::new()),
        LayoutMode::Temporal => Box::new(GeographicEngine::new()),
        LayoutMode::Clustered => Box::new(StressMajorizationEngine::new()),
    }
}
