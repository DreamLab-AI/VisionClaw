//! Ontology Repository Port — ADR-090 Phase 1b.
//!
//! Moved from webxr `src/ports/ontology_repository.rs`. OWL data types already
//! live in `ports::owl_types` (promoted in Phase 2). `OntologyRepository` could
//! not move until Phase 1b unified `GraphData` as a domain type.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::models::graph::GraphData;

// Re-export OWL types so callers can use `ontology_repository::OwlClass` etc.
pub use crate::ports::owl_types::{
    AxiomType, InferenceResults, OwlAxiom, OwlClass, OwlCrossReference, OwlProperty,
    OwlRelationship, PropertyType,
};

pub type Result<T> = std::result::Result<T, OntologyRepositoryError>;

#[derive(Debug, thiserror::Error)]
pub enum OntologyRepositoryError {
    #[error("Ontology not found")]
    NotFound,

    #[error("OWL class not found: {0}")]
    ClassNotFound(String),

    #[error("OWL property not found: {0}")]
    PropertyNotFound(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Invalid OWL data: {0}")]
    InvalidData(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyMetrics {
    pub class_count: usize,
    pub property_count: usize,
    pub axiom_count: usize,
    pub max_depth: usize,
    pub average_branching_factor: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathfindingCacheEntry {
    pub source_node_id: u32,
    pub target_node_id: Option<u32>,
    pub distances: Vec<f32>,
    pub paths: HashMap<u32, Vec<u32>>,
    pub computed_at: chrono::DateTime<chrono::Utc>,
    pub computation_time_ms: f32,
}

#[async_trait]
pub trait OntologyRepository: Send + Sync {
    async fn load_ontology_graph(&self) -> Result<Arc<GraphData>>;

    async fn save_ontology_graph(&self, graph: &GraphData) -> Result<()>;

    async fn save_ontology(
        &self,
        classes: &[OwlClass],
        properties: &[OwlProperty],
        axioms: &[OwlAxiom],
    ) -> Result<()>;

    async fn add_owl_class(&self, class: &OwlClass) -> Result<String>;

    async fn get_owl_class(&self, iri: &str) -> Result<Option<OwlClass>>;

    async fn list_owl_classes(&self) -> Result<Vec<OwlClass>>;

    /// The IRIs of every class [`Self::list_owl_classes`] returns, sorted.
    ///
    /// A cheap probe for callers that only fingerprint the class set; an
    /// adapter overrides it to avoid materialising each class.
    async fn class_iris(&self) -> Result<Vec<String>> {
        let mut iris: Vec<String> = self
            .list_owl_classes()
            .await?
            .into_iter()
            .map(|c| c.iri)
            .collect();
        iris.sort();
        Ok(iris)
    }

    async fn add_owl_property(&self, property: &OwlProperty) -> Result<String>;

    async fn get_owl_property(&self, iri: &str) -> Result<Option<OwlProperty>>;

    async fn list_owl_properties(&self) -> Result<Vec<OwlProperty>>;

    async fn get_classes(&self) -> Result<Vec<OwlClass>>;

    async fn get_axioms(&self) -> Result<Vec<OwlAxiom>>;

    async fn add_axiom(&self, axiom: &OwlAxiom) -> Result<u64>;

    async fn get_class_axioms(&self, class_iri: &str) -> Result<Vec<OwlAxiom>>;

    async fn store_inference_results(&self, _results: &InferenceResults) -> Result<()> {
        Ok(())
    }

    async fn get_inference_results(&self) -> Result<Option<InferenceResults>> {
        Ok(None)
    }

    async fn validate_ontology(&self) -> Result<ValidationReport> {
        Ok(ValidationReport {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            timestamp: chrono::Utc::now(),
        })
    }

    async fn query_ontology(&self, _query: &str) -> Result<Vec<HashMap<String, String>>> {
        Ok(Vec::new())
    }

    async fn remove_owl_class(&self, iri: &str) -> Result<()>;

    async fn remove_axiom(&self, axiom_id: u64) -> Result<()>;

    async fn get_metrics(&self) -> Result<OntologyMetrics>;

    async fn cache_sssp_result(&self, _entry: &PathfindingCacheEntry) -> Result<()> {
        Ok(())
    }

    async fn get_cached_sssp(&self, _source_node_id: u32) -> Result<Option<PathfindingCacheEntry>> {
        Ok(None)
    }

    async fn cache_apsp_result(&self, _distance_matrix: &Vec<Vec<f32>>) -> Result<()> {
        Ok(())
    }

    async fn get_cached_apsp(&self) -> Result<Option<Vec<Vec<f32>>>> {
        Ok(None)
    }

    async fn invalidate_pathfinding_caches(&self) -> Result<()> {
        Ok(())
    }
}
