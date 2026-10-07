// Constraint Translation System - Module Root
// Week 3 Deliverable: OWL Axiom → Physics Constraint Translation

use log::warn;

pub mod axiom_mapper;
pub mod constraint_blender;
pub mod constraint_lod;
pub mod gpu_converter;
pub mod physics_constraint;
pub mod priority_resolver;

// Semantic physics extensions
pub mod semantic_axiom_translator;
pub mod semantic_gpu_buffer;
pub mod semantic_physics_types;

// Re-export main types
pub use physics_constraint::{
    NodeId, PhysicsConstraint, PhysicsConstraintType, PRIORITY_ASSERTED, PRIORITY_DEFAULT,
    PRIORITY_INFERRED, PRIORITY_USER_DEFINED,
};

pub use axiom_mapper::{AxiomMapper, AxiomType, OWLAxiom, TranslationConfig};

pub use priority_resolver::{ConstraintGroup, NodePair, PriorityResolver};

pub use constraint_blender::{BlenderConfig, BlendingStrategy, ConstraintBlender};

pub use gpu_converter::{
    gpu_constraint_kind, to_gpu_constraint_batch, to_gpu_constraint_data, ConstraintData,
    ConstraintStats, GPUConstraintBuffer,
};

pub use constraint_lod::{ConstraintLOD, LODConfig, LODLevel, LODStats};

pub use semantic_physics_types::{Axis, SemanticConstraintBuilder, SemanticPhysicsConstraint};

pub use semantic_axiom_translator::{
    PriorityBlendingStrategy, SemanticAxiomTranslator, SemanticPhysicsConfig,
};

pub use semantic_gpu_buffer::{
    gpu_semantic_types, SemanticConstraintStats, SemanticGPUConstraint, SemanticGPUConstraintBuffer,
};

pub struct ConstraintPipeline {
    mapper: AxiomMapper,
    resolver: PriorityResolver,
    blender: ConstraintBlender,
    lod: ConstraintLOD,
}

impl ConstraintPipeline {
    pub fn new() -> Self {
        Self {
            mapper: AxiomMapper::new(),
            resolver: PriorityResolver::new(),
            blender: ConstraintBlender::new(),
            lod: ConstraintLOD::new(),
        }
    }

    pub fn with_configs(
        translation_config: TranslationConfig,
        blender_config: BlenderConfig,
        lod_config: LODConfig,
    ) -> Self {
        Self {
            mapper: AxiomMapper::with_config(translation_config),
            resolver: PriorityResolver::new(),
            blender: ConstraintBlender::with_config(blender_config),
            lod: ConstraintLOD::with_config(lod_config),
        }
    }

    pub fn process(&mut self, axioms: &[OWLAxiom], zoom_level: f32) -> GPUConstraintBuffer {
        let constraints = self.mapper.translate_axioms(axioms);

        self.resolver.clear();
        self.resolver.add_constraints(constraints);
        let _resolved = self.resolver.resolve();

        let blended: Vec<PhysicsConstraint> = self
            .resolver
            .get_groups()
            .iter()
            .filter_map(|group| self.blender.blend_constraints(&group.constraints))
            .collect();

        self.lod.set_constraints(blended);
        self.lod.update_zoom(zoom_level);
        let active = self.lod.get_active_constraints();

        let mut buffer = GPUConstraintBuffer::new(active.len());
        if let Err(e) = buffer.add_constraints(active) {
            warn!(
                "Failed to add constraints to GPU buffer: {}. Returning empty buffer.",
                e
            );
        }

        buffer
    }

    pub fn update_frame_time(&mut self, frame_time_ms: f32) {
        self.lod.update_frame_time(frame_time_ms);
    }

    pub fn get_lod_stats(&self) -> LODStats {
        self.lod.get_stats()
    }

    pub fn get_constraint_stats(&self, buffer: &GPUConstraintBuffer) -> ConstraintStats {
        ConstraintStats::from_buffer(buffer)
    }

    pub fn get_lod_level(&self) -> LODLevel {
        self.lod.get_current_level()
    }
}

impl Default for ConstraintPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_pipeline() {
        let mut pipeline = ConstraintPipeline::new();

        let axioms = vec![
            OWLAxiom::asserted(AxiomType::SubClassOf {
                subclass: 1,
                superclass: 2,
            }),
            OWLAxiom::asserted(AxiomType::DisjointClasses {
                classes: vec![3, 4],
            }),
            OWLAxiom::inferred(AxiomType::SubClassOf {
                subclass: 5,
                superclass: 2,
            }),
        ];

        let buffer = pipeline.process(&axioms, 5.0);

        assert!(!buffer.is_empty());
        assert_eq!(pipeline.get_lod_level(), LODLevel::Close);
    }

    #[test]
    fn test_lod_reduction() {
        let mut pipeline = ConstraintPipeline::new();

        let axioms = vec![
            OWLAxiom::asserted(AxiomType::SubClassOf {
                subclass: 1,
                superclass: 2,
            }),
            OWLAxiom::asserted(AxiomType::DisjointClasses {
                classes: vec![3, 4],
            }),
        ];

        let buffer_far = pipeline.process(&axioms, 2000.0);
        assert_eq!(pipeline.get_lod_level(), LODLevel::Far);

        let buffer_close = pipeline.process(&axioms, 5.0);
        assert_eq!(pipeline.get_lod_level(), LODLevel::Close);

        assert!(buffer_far.len() <= buffer_close.len());
    }

    #[test]
    fn test_adaptive_lod() {
        let mut pipeline = ConstraintPipeline::new();

        let axioms = vec![OWLAxiom::asserted(AxiomType::SubClassOf {
            subclass: 1,
            superclass: 2,
        })];

        pipeline.process(&axioms, 5.0);

        pipeline.update_frame_time(30.0);

        let stats = pipeline.get_lod_stats();
        assert!(stats.frame_time_ms > stats.target_frame_time_ms);
    }
}
