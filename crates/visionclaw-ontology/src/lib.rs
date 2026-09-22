//! VisionClaw Ontology Crate (ADR-090 Phase A4)
//!
//! Extracted ontology subsystem: OWL parsing, inference, JSON-LD validation,
//! custom reasoning, and supporting services.
//!
//! ## DAG position
//! `contracts → domain → adapters → ontology`
//!
//! ## What lives here (Phase A4 extraction)
//! - `inference`  — OWL 2 EL++ parser, inference cache, optimisation
//! - `reasoning`  — custom Whelk-backed reasoner
//! - `ontology`   — OWL services, stub actor/physics modules
//! - `validation` — actor-state validation helpers
//! - `types`      — ontology MCP tool surface types
//! - `services`   — json-ld ingest pipeline, json-ld validator, OWL validator
//!                  service, ontology content analyser, ontology parser
//! - `utils`      — local copy of time utilities (chrono wrappers)
//!
//! ## What stays in webxr (needs actors / GPU / config)
//! - `services::ontology_query_service`     — needs WhelkInferenceEngine (adapters) + schema_service
//! - `services::ontology_mutation_service`  — needs file_service, github_pr_service
//! - `services::ontology_pipeline_service` — needs actors
//! - `services::ontology_enrichment_service` — needs ontology_reasoner
//! - `services::ontology_reasoner`          — needs adapters
//! - `services::schema_service`             — needs crate::models (not in domain yet)

pub mod inference;
pub mod ontology;
pub mod reasoning;
pub mod services;
pub mod types;
pub mod utils;
pub mod validation;
