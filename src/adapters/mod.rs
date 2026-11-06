// src/adapters/mod.rs
//! Hexagonal Architecture Adapters
//!
//! Per ADR-090 Phase A1+: `messages` and `oxigraph_ontology_repository` have
//! been extracted to `crates/visionclaw-adapters/`. Shims below preserve all
//! existing `use crate::adapters::X` call sites.
//!
//! Still in webxr (actor / utils deps — resolved in Phase A3):
//! - `actor_graph_repository` (actors::graph_state_actor)
//! - `actix_physics_adapter` (actors::physics_orchestrator_actor)
//! - `actix_semantic_adapter` (actors::semantic_processor_actor)
//! - `gpu_semantic_analyzer` (utils::unified_gpu_compute)
//! - `oxigraph_graph_repository` (actors::graph_actor + utils::socket_flow_messages)
//! - `physics_orchestrator_adapter` (actors::*)
//! - `sqlite_settings_repository` (config::AppFullSettings)

pub mod actor_graph_repository;

#[cfg(feature = "gpu")]
pub mod gpu_semantic_analyzer;

/// ADR-090 shim — canonical source is visionclaw-adapters.
pub mod whelk_inference_engine {
    pub use visionclaw_adapters::whelk_inference_engine::*;
}

/// ADR-090 Phase A1+ shim — canonical source is visionclaw-adapters.
pub mod messages {
    pub use visionclaw_adapters::messages::*;
}

/// ADR-090 Phase A1+ shim — canonical source is visionclaw-adapters.
pub mod oxigraph_ontology_repository {
    pub use visionclaw_adapters::oxigraph_ontology_repository::*;
}

#[cfg(feature = "gpu")]
pub mod actix_physics_adapter;
pub mod actix_semantic_adapter;

pub mod physics_orchestrator_adapter;

pub use actor_graph_repository::ActorGraphRepository;

#[cfg(feature = "gpu")]
pub use gpu_semantic_analyzer::GpuSemanticAnalyzerAdapter;

// Canonical persistence adapters (ADR-11)
pub mod oxigraph_graph_repository;
pub mod sqlite_settings_repository;
// WS-9: durable EnrichmentProposal lifecycle store (data/enrichment.sqlite3)
pub mod sqlite_enrichment_repository;
// ADR-2101: durable case-state authority for ADR-050 decision elevation —
// a typed facade over the same enrichment store (data/enrichment.sqlite3)
pub mod decision_elevation_store;
// RES-a: durable LivenessCanary registry + fire log (data/liveness.sqlite3)
pub mod sqlite_canary_repository;
// REC-4: durable KPI snapshot + lineage store (data/kpi.sqlite3), ADR-130 D5
pub mod sqlite_kpi_repository;
// Graph shares (data/shares.sqlite3): one store for every actix worker, kept across restarts
pub mod sqlite_share_repository;

pub use decision_elevation_store::{DecisionCase, DecisionElevationStore};
pub use oxigraph_graph_repository::OxigraphGraphRepository;
pub use oxigraph_ontology_repository::OxigraphOntologyRepository;
pub use sqlite_canary_repository::{
    CanaryRegistration, CanaryStatus, CanaryStoreError, SqliteCanaryRepository,
};
pub use sqlite_enrichment_repository::{
    EnrichmentProposal, EnrichmentStoreError, SqliteEnrichmentRepository, StoredDecision,
};
pub use sqlite_kpi_repository::{
    KpiLineageRow, KpiSnapshotRow, KpiStoreError, NewKpiSnapshot, SqliteKpiRepository,
};
pub use sqlite_settings_repository::SqliteSettingsRepository;
pub use sqlite_share_repository::{AccessClaim, ShareStoreError, SqliteShareRepository};

pub use whelk_inference_engine::WhelkInferenceEngine;

#[cfg(feature = "gpu")]
pub use actix_physics_adapter::ActixPhysicsAdapter;
pub use actix_semantic_adapter::ActixSemanticAdapter;

#[cfg(test)]
mod tests;
