//! Physics engine modules for advanced knowledge graph layout optimization
//!
//! This module provides sophisticated physics-based algorithms for knowledge graph
//! layout optimization, including stress majorization and semantic constraint generation.
//! The physics engine integrates with the GPU compute pipeline for high-performance
//! real-time graph visualization and layout optimization.
//!
//! ## Architecture
//!
//! The physics module is organized into specialized components:
//!
//! - **Stress Majorization**: Implements stress majorization algorithms for global
//!   layout optimization, minimizing the stress function to achieve visually pleasing
//!   node positions that satisfy multiple constraint types.
//!
//! - **Semantic Constraints**: Generates constraints based on semantic relationships,
//!   topic similarity, hierarchical structures, and domain knowledge to create
//!   meaningful spatial arrangements.
//!
//! - **Ontology Constraints**: Translates OWL axioms and logical inferences into
//!   physics constraints, bridging semantic reasoning with physical simulation to
//!   enforce ontological relationships in graph layout.
//!
//! ## Integration
//!
//! This module integrates with:
//! - GPU compute kernels for high-performance matrix operations
//! - Constraint system defined in `models::constraints`
//! - Graph data structures and node/edge representations
//! - Real-time visualization pipeline
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::physics::{StressMajorizationSolver, SemanticConstraintGenerator, OntologyConstraintTranslator};
//! use crate::models::constraints::ConstraintSet;
//! use crate::models::graph::GraphData;
//!
//! 
//! let mut solver = StressMajorizationSolver::new(params);
//!
//! 
//! let constraint_generator = SemanticConstraintGenerator::new();
//! let semantic_constraints = constraint_generator.generate_constraints(&graph_data)?;
//!
//! 
//! let mut ontology_translator = OntologyConstraintTranslator::new();
//! let ontology_constraints = ontology_translator.apply_ontology_constraints(&graph_data, &reasoning_report)?;
//!
//! 
//! let mut combined_constraints = semantic_constraints.constraints;
//! combined_constraints.extend(ontology_constraints.constraints);
//!
//! let final_constraint_set = ConstraintSet {
//!     constraints: combined_constraints,
//!     advanced_params: ontology_constraints.advanced_params
//! };
//!
//! solver.optimize(&mut graph_data, &final_constraint_set)?;
//! ```

pub mod lsh;
pub mod ontology_constraints;
pub mod semantic_constraints;
pub mod simd_forces;
pub mod stress_majorization;

// Phase 5 (ADR-01 D5): LayoutEngine trait + five engine implementations.
// Feature-gated behind `physics-v2`; legacy dispatch via
// `src/layout/engines.rs::compute_layout` remains the production path.
#[cfg(feature = "physics-v2")]
pub mod engines;

#[cfg(test)]
mod integration_tests;

pub use ontology_constraints::{
    OWLAxiom, OWLAxiomType, OntologyConstraintTranslator, OntologyInference,
    OntologyReasoningReport,
};
pub use semantic_constraints::SemanticConstraintGenerator;
pub use stress_majorization::StressMajorizationSolver;

pub use crate::models::constraints::{AdvancedParams, Constraint, ConstraintKind, ConstraintSet};
pub use crate::models::graph::GraphData;
pub use crate::models::metadata::Metadata;
pub use crate::models::node::Node;
