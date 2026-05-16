//! Stress-majorization layout engine (ADR-01 D5). Wraps the existing
//! `StressMajorizationSolver` in `src/physics/stress_majorization.rs`.

#![cfg(feature = "physics-v2")]

use anyhow::Result;

use super::LayoutEngine;
use crate::gpu::buffers::PhysicsGpuBuffers;
use crate::models::simulation_params::SimParams;

pub struct StressMajorizationEngine;

impl StressMajorizationEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StressMajorizationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for StressMajorizationEngine {
    fn step(&self, _buffers: &mut PhysicsGpuBuffers, _params: &SimParams) -> Result<()> {
        // STUB: wraps StressMajorizationSolver::optimize_once. The solver
        // currently operates on host-side GraphData; the bridge to
        // PhysicsGpuBuffers (download positions, run solver, upload back) is
        // wired in the ForceComputeActor migration commit.
        Ok(())
    }

    fn supports_3d(&self) -> bool {
        true
    }

    fn convergence_metric(&self, _buffers: &PhysicsGpuBuffers) -> f32 {
        // Stress is the natural metric here. Returns 0.0 until the
        // host-to-device bridge is in place so the actor treats the stub as
        // converged.
        0.0
    }

    fn name(&self) -> &'static str {
        "StressMajorization"
    }
}
