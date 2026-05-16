//! Circular / radial layout engine. Single CPU pass; node positions placed on
//! one or more concentric rings using golden-angle distribution.

#![cfg(feature = "physics-v2")]

use anyhow::Result;

use super::LayoutEngine;
use crate::gpu::buffers::PhysicsGpuBuffers;
use crate::models::simulation_params::SimParams;

pub struct CircularEngine;

impl CircularEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CircularEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for CircularEngine {
    fn step(&self, _buffers: &mut PhysicsGpuBuffers, _params: &SimParams) -> Result<()> {
        // STUB: calls into `src/layout/engines.rs::radial_layout` and uploads
        // via PhysicsGpuBuffers::upload_positions in the migration commit.
        Ok(())
    }

    fn supports_3d(&self) -> bool {
        false
    }

    fn convergence_metric(&self, _buffers: &PhysicsGpuBuffers) -> f32 {
        0.0
    }

    fn name(&self) -> &'static str {
        "Circular"
    }
}
