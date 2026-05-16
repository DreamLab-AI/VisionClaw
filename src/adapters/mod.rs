// src/adapters/mod.rs
//! Hexagonal Architecture Adapters
//!
//! This module contains adapters that implement the port interfaces
//! using concrete technologies (actors, GPU compute, Neo4j, etc.)

// CQRS Phase 1: Actor-based adapter for gradual migration
pub mod actor_graph_repository;

// CQRS Phase 2: Neo4j direct query adapter (professional, scalable)
pub mod neo4j_graph_repository;

// Legacy adapters
// pub mod gpu_physics_adapter;
#[cfg(feature = "gpu")]
pub mod gpu_semantic_analyzer;

// New hexser-based adapters (legacy removed, using unified repositories)
pub mod whelk_inference_engine;

// Neo4j integration adapters
pub mod neo4j_adapter;

// Phase 2.2: Actor system adapter wrappers
#[cfg(feature = "gpu")]
pub mod actix_physics_adapter;
pub mod actix_semantic_adapter;
pub mod messages;

// Compatibility alias for physics orchestrator adapter
pub mod physics_orchestrator_adapter;

// CQRS Phase 1: Actor-based adapter exports
pub use actor_graph_repository::ActorGraphRepository;

// CQRS Phase 2: Neo4j direct query adapter exports
pub use neo4j_graph_repository::Neo4jGraphRepository;

// GPU adapter implementation exports (these implement the traits from crate::ports)
// pub use gpu_physics_adapter::GpuPhysicsAdapter as GpuPhysicsAdapterImpl;
#[cfg(feature = "gpu")]
pub use gpu_semantic_analyzer::GpuSemanticAnalyzerAdapter;

// Settings repository adapters
// REMOVED: sqlite_settings_repository - migrated to Neo4j
pub mod neo4j_settings_repository;
pub mod neo4j_ontology_repository;

pub use neo4j_settings_repository::{Neo4jSettingsRepository, Neo4jSettingsConfig};
pub use neo4j_ontology_repository::{Neo4jOntologyRepository, Neo4jOntologyConfig};

// Phase 11 persistence migration (ADR-11): Oxigraph + SQLite adapters.
// Gated by `persistence-oxigraph` feature so the radical-rollback baseline
// still compiles without these deps. Cutover (delete neo4j_* adapters)
// happens in a later phase per ADR-11 §"Implementation order" step 8.
#[cfg(feature = "persistence-oxigraph")]
pub mod oxigraph_ontology_repository;
#[cfg(feature = "persistence-oxigraph")]
pub mod oxigraph_graph_repository;
#[cfg(feature = "persistence-oxigraph")]
pub mod sqlite_settings_repository;

#[cfg(feature = "persistence-oxigraph")]
pub use oxigraph_ontology_repository::OxigraphOntologyRepository;
#[cfg(feature = "persistence-oxigraph")]
pub use oxigraph_graph_repository::OxigraphGraphRepository;
#[cfg(feature = "persistence-oxigraph")]
pub use sqlite_settings_repository::SqliteSettingsRepository;

// Inference engine exports
pub use whelk_inference_engine::WhelkInferenceEngine;

// Neo4j integration exports
pub use neo4j_adapter::{Neo4jAdapter, Neo4jConfig};

// Phase 2.2: Actor wrapper adapter exports
#[cfg(feature = "gpu")]
pub use actix_physics_adapter::ActixPhysicsAdapter;
pub use actix_semantic_adapter::ActixSemanticAdapter;

// Tests module
#[cfg(test)]
mod tests;
