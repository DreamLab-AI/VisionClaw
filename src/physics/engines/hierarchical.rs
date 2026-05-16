//! Hierarchical (Sugiyama-inspired) layout engine. Two-dimensional;
//! `convergence_metric` returns 0.0 immediately since the placement is a
//! single CPU pass.

#![cfg(feature = "physics-v2")]

use anyhow::Result;

use super::LayoutEngine;
use crate::gpu::buffers::PhysicsGpuBuffers;
use crate::models::simulation_params::SimParams;

pub struct HierarchicalEngine;

impl HierarchicalEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HierarchicalEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for HierarchicalEngine {
    fn step(&self, _buffers: &mut PhysicsGpuBuffers, _params: &SimParams) -> Result<()> {
        // STUB: calls into `src/layout/engines.rs::hierarchical_layout` and
        // uploads via PhysicsGpuBuffers::upload_positions in the migration
        // commit. CPU-side; no kernel launches.
        Ok(())
    }

    fn supports_3d(&self) -> bool {
        false
    }

    fn convergence_metric(&self, _buffers: &PhysicsGpuBuffers) -> f32 {
        0.0
    }

    fn name(&self) -> &'static str {
        "Hierarchical"
    }
}
