//! Ontology-domain messages: OWL validation, axiom loading, inference,
//! ontology-physics constraint integration, and legacy ontology actor messages.
//!
//! Domain-safe types have been moved to `visionclaw_actors::messages::ontology_messages`.
//! This file re-exports them and defines the webxr-internal types that cannot move.

// ---------------------------------------------------------------------------
// Re-export domain-safe types from the domain crate
// ---------------------------------------------------------------------------

pub use visionclaw_actors::messages::ontology_messages::{
    ApplyOntologyConstraints, CachedOntologyInfo, ClearOntologyCaches, ConstraintMergeMode,
    ConstraintStats, GetCachedOntologies, GetConstraintStats, GetOntologyConstraintStats,
    GetOntologyHealth, GetOntologyHealthLegacy, GetValidationReport, LoadOntologyAxioms,
    OntologyConstraintStats, OntologyHealth, SetConstraintGroupActive, ValidateGraph,
    ValidationMode,
};

// ---------------------------------------------------------------------------
// Webxr-internal types (cannot move to domain crate)
// ---------------------------------------------------------------------------

use actix::prelude::*;

/// Update the ontology validation config.
/// Blocked: references `services::owl_validator::ValidationConfig`.
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateOntologyMapping {
    pub config: crate::services::owl_validator::ValidationConfig,
}

/// Validate an ontology against a property graph.
/// Blocked: references `services::owl_validator::{PropertyGraph, ValidationReport}`.
#[derive(Message)]
#[rtype(result = "Result<crate::services::owl_validator::ValidationReport, String>")]
pub struct ValidateOntology {
    pub ontology_id: String,
    pub graph_data: crate::services::owl_validator::PropertyGraph,
    pub mode: ValidationMode,
    /// Optional caller-supplied job id. When set, the actor adopts it as the
    /// validation job id and caches the report (pending and completed) under it
    /// as well as under `ontology_id`, so the caller can retrieve the report by
    /// the exact id it was handed. When `None` the actor generates one.
    pub job_id: Option<String>,
}

/// Apply OWL inference rules to a set of RDF triples.
/// Blocked: references `services::owl_validator::RdfTriple`.
#[derive(Message)]
#[rtype(result = "Result<Vec<crate::services::owl_validator::RdfTriple>, String>")]
pub struct ApplyInferences {
    pub rdf_triples: Vec<crate::services::owl_validator::RdfTriple>,
    pub max_depth: Option<usize>,
}

/// Retrieve a cached ontology validation report.
/// Blocked: references `services::owl_validator::ValidationReport`.
#[derive(Message)]
#[rtype(result = "Result<Option<crate::services::owl_validator::ValidationReport>, String>")]
pub struct GetOntologyReport {
    pub report_id: Option<String>,
}

/// PRD-018 WS-3 / ADR-098 D1 — apply materialised OWL axioms (asserted +
/// Whelk-inferred) directly to the live-kernel constraint buffer via the
/// canonical `map_axioms_to_constraints` anti-corruption mapper.
///
/// Dispatched by `GitHubSyncService::run_post_sync_reasoning` after inference so
/// the semantic layout forces (subClassOf attraction, disjointWith separation,
/// sameAs/equivalentClass colocation) actually reach the GPU. Routed
/// GPUManagerActor → PhysicsSupervisor → OntologyConstraintActor. Returns the
/// number of live-kernel constraints produced.
#[derive(Message)]
#[rtype(result = "Result<usize, String>")]
pub struct ApplyMaterializedAxioms {
    pub axioms: Vec<visionclaw_domain::ports::owl_types::OwlAxiom>,
    pub graph_data: visionclaw_domain::models::graph::GraphData,
}
