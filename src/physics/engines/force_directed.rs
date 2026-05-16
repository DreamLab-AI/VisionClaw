//! ForceAtlas2 LinLog force-directed engine (ADR-01 D5).
//!
//! This is the default engine. It runs the existing `force_pass_kernel` +
//! `integrate_pass_kernel` CUDA pair against `PhysicsGpuBuffers`. The kernel
//! launchers themselves live in `src/utils/unified_gpu_compute/execution.rs`
//! and will be re-pointed at `PhysicsGpuBuffers` in the T1-callsite-migration
//! commit; until then `step` is a documented stub.

#![cfg(feature = "physics-v2")]

use anyhow::Result;

use super::LayoutEngine;
use crate::gpu::buffers::PhysicsGpuBuffers;
use crate::models::simulation_params::SimParams;

pub struct ForceDirectedEngine;

impl ForceDirectedEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ForceDirectedEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for ForceDirectedEngine {
    fn step(&self, _buffers: &mut PhysicsGpuBuffers, _params: &SimParams) -> Result<()> {
        // STUB: wired to force_pass_kernel + integrate_pass_kernel in the
        // T1-callsite-migration commit. ForceComputeActor still drives those
        // kernels directly today.
        Ok(())
    }

    fn supports_3d(&self) -> bool {
        true
    }

    fn convergence_metric(&self, _buffers: &PhysicsGpuBuffers) -> f32 {
        // Computed from the `partial_kinetic_energy` reduction once the
        // ForceComputeActor migration is complete. Returns 0.0 here so the
        // settlement hysteresis treats the stub as "no motion".
        0.0
    }

    fn name(&self) -> &'static str {
        "ForceDirected"
    }
}
