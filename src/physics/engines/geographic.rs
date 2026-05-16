//! Geographic layout engine. Maps node attributes (e.g. lat/lon properties on
//! ontology individuals) onto a 2-D plane. CPU-side single pass.

#![cfg(feature = "physics-v2")]

use anyhow::Result;

use super::LayoutEngine;
use crate::gpu::buffers::PhysicsGpuBuffers;
use crate::models::simulation_params::SimParams;

pub struct GeographicEngine;

impl GeographicEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeographicEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for GeographicEngine {
    fn step(&self, _buffers: &mut PhysicsGpuBuffers, _params: &SimParams) -> Result<()> {
        // STUB: reads geographic coordinate properties from node metadata
        // (queried via the actor's GraphStateActor handle) and writes a
        // Mercator-projected XY pair per node. Wired in the migration commit.
        Ok(())
    }

    fn supports_3d(&self) -> bool {
        false
    }

    fn convergence_metric(&self, _buffers: &PhysicsGpuBuffers) -> f32 {
        0.0
    }

    fn name(&self) -> &'static str {
        "Geographic"
    }
}
