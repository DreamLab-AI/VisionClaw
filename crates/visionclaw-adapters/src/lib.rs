//! VisionClaw Adapters Crate
//!
//! Adapter implementations per ADR-090 Phase 2 (Hexagonal Crate Modularisation).
//! This crate implements the port traits defined in `visionclaw-domain` using
//! real infrastructure: Oxigraph (RDF/SPARQL) and Whelk (EL reasoning).
//!
//! ## What lives here
//! - `oxigraph_ontology_repository` — OWL ontology persistence over Oxigraph quad-store
//! - `whelk_inference_engine` — EL reasoning via horned-owl + whelk-rs
//! - `provenance_writer` — ADR-049 assertion-version quad builders, pure `state_at`, state-at SPARQL builders
//! - `messages` — Actix actor message types for GPU adapter bridging
//!
//! ## What stayed in webxr (blocked on actor extraction — ADR-090 Realisation)
//! - `actor_graph_repository` — depends on `crate::actors::graph_state_actor`
//! - `actix_physics_adapter` — depends on `crate::actors::physics_orchestrator_actor`
//! - `actix_semantic_adapter` — depends on `crate::actors::semantic_processor_actor`
//! - `oxigraph_graph_repository` — depends on `crate::actors::graph_actor` + socket_flow_messages
//! - `physics_orchestrator_adapter` — depends on `crate::actors::*` + `crate::utils::socket_flow_messages`
//!
//! `gpu_semantic_analyzer` and `sqlite_settings_repository` were unblocked by Phase 1b
//! and moved to `visionclaw-domain` (port trait) and `visionclaw-gpu` / `visionclaw-adapters`
//! respectively. They no longer live in webxr.
//!
//! DAG position: `contracts → domain → adapters`

// Phase 2 actually-moved adapters (the only ones that have zero deps on
// webxr-local types like crate::actors::*, crate::config::AppFullSettings,
// or crate::utils::socket_flow_messages — see "what stayed in webxr" above).
pub mod whelk_inference_engine;
pub use whelk_inference_engine::WhelkInferenceEngine;

// ADR-090 Phase A1+ — adapters unblocked by Phase 1b (domain now owns
// GraphData/Node/Edge, GpuPhysicsAdapter, GpuSemanticAnalyzer, OntologyRepository).
pub mod messages;
pub mod oxigraph_ontology_repository;
pub mod provenance_writer;
pub use oxigraph_ontology_repository::OxigraphOntologyRepository;

// ADR-101 — versioned, idempotent SPARQL migration framework for the Oxigraph
// triple store (parity with the SQLite `schema_migrations` discipline).
pub mod sparql_migrations;
pub use sparql_migrations::{run_pending as run_sparql_migrations, MigrationError, MIGRATIONS};

// WS-1 / ADR-100 — standards-grade RDF round-trip serialisation over oxigraph's
// bundled oxrdfio (Turtle / JSON-LD / N-Quads). No new dependency.
pub mod rdf_serializer;

// PRD-022 WS-2 — PROV-O provenance reification emitter. Reifies activity
// URNs as RDF triples in the append-only urn:ngm:graph:provenance graph.
pub mod provenance_emitter;
pub use provenance_emitter::{
    emit_activity, emit_activity_nonfatal, provenance_for_entity, reify_activity, ActivityRecord,
    ProvenanceChain, ProvenanceError, ProvenanceNode,
};
