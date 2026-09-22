//! Domain-safe ontology message types.
//!
//! Blocked in webxr (stay in src/actors/messages/ontology_messages.rs):
//!   - `UpdateOntologyMapping` — refs `services::owl_validator::ValidationConfig`
//!   - `ValidateOntology`      — refs `services::owl_validator::PropertyGraph`
//!   - `ApplyInferences`       — refs `services::owl_validator::RdfTriple`
//!   - `GetOntologyReport`     — refs `services::owl_validator::ValidationReport`
//!
//! Everything below depends only on `actix`, `chrono`, `serde`, `std`,
//! and `visionclaw_domain::models::constraints`.

use actix::prelude::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use visionclaw_domain::models::constraints::ConstraintSet;

// ---------------------------------------------------------------------------
// Validation mode (shared by multiple message types)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationMode {
    Quick,
    Full,
    Incremental,
}

// ---------------------------------------------------------------------------
// OWL Ontology Actor Messages
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<String, String>")]
pub struct LoadOntologyAxioms {
    pub source: String,
    pub format: Option<String>,
}

#[derive(Message)]
#[rtype(result = "Result<OntologyHealth, String>")]
pub struct GetOntologyHealth;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyHealth {
    pub loaded_ontologies: u32,
    pub cached_reports: u32,
    pub validation_queue_size: u32,
    pub last_validation: Option<DateTime<Utc>>,
    pub cache_hit_rate: f32,
    pub avg_validation_time_ms: f32,
    pub active_jobs: u32,
    pub memory_usage_mb: f32,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ClearOntologyCaches;

#[derive(Message)]
#[rtype(result = "Result<Vec<CachedOntologyInfo>, String>")]
pub struct GetCachedOntologies;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedOntologyInfo {
    pub id: String,
    pub loaded_at: DateTime<Utc>,
    pub signature: String,
    pub source: String,
    pub size_kb: u32,
    pub access_count: u32,
}

// ---------------------------------------------------------------------------
// Legacy Ontology Actor Messages (validation/report surface)
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<String, String>")]
pub struct ValidateGraph {
    pub mode: ValidationMode,
}

#[derive(Message)]
#[rtype(result = "Result<Option<String>, String>")]
pub struct GetValidationReport {
    pub report_id: Option<String>,
}

#[derive(Message)]
#[rtype(result = "Result<String, String>")]
pub struct GetOntologyHealthLegacy;

// ---------------------------------------------------------------------------
// Ontology-Physics Integration Messages
// ---------------------------------------------------------------------------

#[derive(Message, Clone)]
#[rtype(result = "Result<(), String>")]
pub struct ApplyOntologyConstraints {
    pub constraint_set: ConstraintSet,
    pub merge_mode: ConstraintMergeMode,
    pub graph_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConstraintMergeMode {
    Replace,
    Merge,
    AddIfNoConflict,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SetConstraintGroupActive {
    pub group_name: String,
    pub active: bool,
}

#[derive(Message)]
#[rtype(result = "Result<ConstraintStats, String>")]
pub struct GetConstraintStats;

#[derive(Message)]
#[rtype(result = "Result<OntologyConstraintStats, String>")]
pub struct GetOntologyConstraintStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyConstraintStats {
    pub total_axioms_processed: u32,
    pub active_ontology_constraints: u32,
    pub constraint_evaluation_count: u32,
    pub last_update_time_ms: f32,
    pub gpu_failure_count: u32,
    pub cpu_fallback_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintStats {
    pub total_constraints: usize,
    pub active_constraints: usize,
    pub constraint_groups: HashMap<String, usize>,
    pub ontology_constraints: usize,
    pub user_constraints: usize,
}
