// crates/visionclaw-domain/src/ports/owl_types.rs
//! Pure OWL data types — moved from src/ports/ontology_repository.rs per ADR-090 Phase 2.
//!
//! These types have zero dependency on `GraphData` or any webxr-internal model,
//! so they can safely live in the domain crate and be shared by both
//! `visionclaw-adapters` and the `webxr` monolith.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// OWL Class with rich metadata support (Schema V2)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OwlClass {
    // Core identification
    pub iri: String,
    pub term_id: Option<String>,
    pub preferred_term: Option<String>,

    // Basic metadata
    pub label: Option<String>,
    pub description: Option<String>,
    pub parent_classes: Vec<String>,

    // Classification metadata
    pub source_domain: Option<String>,
    pub version: Option<String>,
    pub class_type: Option<String>,

    // Quality metrics
    pub status: Option<String>,
    pub maturity: Option<String>,
    pub quality_score: Option<f32>,
    pub authority_score: Option<f32>,
    pub public_access: Option<bool>,
    pub content_status: Option<String>,

    // OWL2 properties
    pub owl_physicality: Option<String>,
    pub owl_role: Option<String>,

    // Domain relationships
    pub belongs_to_domain: Option<String>,
    pub bridges_to_domain: Option<String>,

    // Source tracking
    pub source_file: Option<String>,
    pub file_sha1: Option<String>,
    pub markdown_content: Option<String>,
    pub last_synced: Option<chrono::DateTime<chrono::Utc>>,

    // Semantic relationships (ADR-014)
    pub has_part: Vec<String>,
    pub is_part_of: Vec<String>,
    pub requires: Vec<String>,
    pub depends_on: Vec<String>,
    pub enables: Vec<String>,
    pub relates_to: Vec<String>,
    pub bridges_to: Vec<String>,
    pub bridges_from: Vec<String>,
    pub other_relationships: HashMap<String, Vec<String>>,

    // Additional metadata
    pub properties: HashMap<String, String>,
    pub additional_metadata: Option<String>,
}

/// OWL Property with quality metrics (Schema V2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwlProperty {
    pub iri: String,
    pub label: Option<String>,
    pub property_type: PropertyType,
    pub domain: Vec<String>,
    pub range: Vec<String>,
    pub quality_score: Option<f32>,
    pub authority_score: Option<f32>,
    pub source_file: Option<String>,
}

impl Default for OwlProperty {
    fn default() -> Self {
        Self {
            iri: String::new(),
            label: None,
            property_type: PropertyType::ObjectProperty,
            domain: Vec::new(),
            range: Vec::new(),
            quality_score: None,
            authority_score: None,
            source_file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PropertyType {
    ObjectProperty,
    DataProperty,
    AnnotationProperty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AxiomType {
    SubClassOf,
    EquivalentClass,
    DisjointWith,
    ObjectPropertyAssertion,
    DataPropertyAssertion,
    SubPropertyOf,
    TransitiveProperty,
    SymmetricProperty,
    InverseProperties,
    SomeValuesFrom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwlAxiom {
    pub id: Option<u64>,
    pub axiom_type: AxiomType,
    pub subject: String,
    pub object: String,
    pub annotations: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResults {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub inferred_axioms: Vec<OwlAxiom>,
    pub inference_time_ms: u64,
    pub reasoner_version: String,
}

/// Semantic relationship between OWL classes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwlRelationship {
    pub source_class_iri: String,
    pub relationship_type: String,
    pub target_class_iri: String,
    pub confidence: f32,
    pub is_inferred: bool,
}

/// Cross-reference from an OWL class to external resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwlCrossReference {
    pub source_class_iri: String,
    pub target_reference: String,
    pub reference_type: String,
}
