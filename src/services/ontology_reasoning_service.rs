// src/services/ontology_reasoning_service.rs
//! Ontology Reasoning Service
//!
//! Provides complete OWL reasoning using CustomReasoner with caching and persistence.
//! Infers missing axioms, computes class hierarchies, and identifies disjoint classes.
//! All data is stored in Neo4j using Neo4jOntologyRepository.

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;

use crate::adapters::whelk_inference_engine::WhelkInferenceEngine; // Currently used for initialization only
use crate::ports::ontology_repository::{
    AxiomType, OntologyRepository, OntologyRepositoryError, OwlAxiom,
};
use crate::utils::time;

/// Inferred axiom with metadata about the inference process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferredAxiom {
    pub id: String,
    pub ontology_id: String,
    pub axiom_type: String,  // "SubClassOf", "DisjointWith", "InverseOf"
    pub subject_iri: String,
    pub object_iri: Option<String>,
    pub property_iri: Option<String>,
    pub confidence: f32,
    pub inference_path: Vec<String>,
    pub user_defined: bool,
}

/// Class hierarchy representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassHierarchy {
    pub root_classes: Vec<String>,
    pub hierarchy: HashMap<String, ClassNode>,
}

/// Node in the class hierarchy tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassNode {
    pub iri: String,
    pub label: String,
    pub parent_iri: Option<String>,
    pub children_iris: Vec<String>,
    pub node_count: usize,
    pub depth: usize,
}

/// Pair of disjoint classes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisjointPair {
    pub class_a: String,
    pub class_b: String,
    pub reason: String,
}

/// Cached inference result
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InferenceCacheEntry {
    pub ontology_id: String,
    pub ontology_checksum: String,
    pub inferred_axioms: Vec<InferredAxiom>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub inference_time_ms: u64,
}

/// Ontology Reasoning Service with CustomReasoner integration
/// Uses CustomReasoner for actual inference operations. The WhelkInferenceEngine
/// is currently maintained for API compatibility but will be phased out.
/// All ontology data is persisted in Neo4j via Neo4jOntologyRepository.
pub struct OntologyReasoningService {
    inference_engine: Arc<WhelkInferenceEngine>, // Legacy - to be removed
    ontology_repo: Arc<dyn OntologyRepository>,
    cache: tokio::sync::RwLock<HashMap<String, InferenceCacheEntry>>,
}

impl OntologyReasoningService {
    /// Create a new OntologyReasoningService
    pub fn new(
        inference_engine: Arc<WhelkInferenceEngine>,
        ontology_repo: Arc<dyn OntologyRepository>,
    ) -> Self {
        info!("Initializing OntologyReasoningService");
        Self {
            inference_engine,
            ontology_repo,
            cache: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Infer axioms from the ontology using CustomReasoner
    /// This method:
    /// 1. Loads ontology data from Neo4j
    /// 2. Runs CustomReasoner for EL++ inference
    /// 3. Caches results with checksum validation
    /// 4. Stores inferred axioms back to Neo4j
    /// # Arguments
    /// * `ontology_id` - Ontology identifier
    /// # Returns
    /// Vector of inferred axioms with confidence scores and inference paths
    #[instrument(skip(self), level = "info")]
    pub async fn infer_axioms(
        &self,
        ontology_id: &str,
    ) -> Result<Vec<InferredAxiom>, OntologyRepositoryError> {
        let start = Instant::now();
        info!("Starting axiom inference for ontology: {}", ontology_id);

        // Check cache first
        let checksum = self.calculate_ontology_checksum(ontology_id).await?;
        if let Some(cached) = self.get_cached_inference(ontology_id, &checksum).await {
            info!("Using cached inference results for {}", ontology_id);
            return Ok(cached.inferred_axioms);
        }

        // Load ontology data
        let classes = self.ontology_repo.get_classes().await?;
        let axioms = self.ontology_repo.get_axioms().await?;

        debug!(
            "Loaded {} classes and {} axioms for inference",
            classes.len(),
            axioms.len()
        );

        // Build ontology for reasoning
        use crate::reasoning::custom_reasoner::{Ontology, OWLClass};
        use std::collections::HashSet;

        let mut ontology = Ontology::default();
        for class in &classes {
            ontology.classes.insert(
                class.iri.clone(),
                OWLClass {
                    iri: class.iri.clone(),
                    label: class.label.clone(),
                    parent_class_iri: None,
                },
            );
        }

        // Build subclass relationships from axioms
        for axiom in &axioms {
            if matches!(axiom.axiom_type, AxiomType::SubClassOf) {
                ontology.subclass_of
                    .entry(axiom.subject.clone())
                    .or_insert_with(HashSet::new)
                    .insert(axiom.object.clone());
            }
        }

        // Run inference using CustomReasoner
        use crate::reasoning::custom_reasoner::{CustomReasoner, OntologyReasoner as _};
        let reasoner = CustomReasoner::new();
        let inference_results = reasoner.infer_axioms(&ontology)
            .map_err(|e| OntologyRepositoryError::InvalidData(format!("Inference error: {}", e)))?;

        // Convert inferred axioms to our format
        let mut inferred_axioms = Vec::new();
        for axiom in &inference_results {
            use crate::reasoning::custom_reasoner::AxiomType as CustomAxiomType;
            let axiom_type_str = match axiom.axiom_type {
                CustomAxiomType::SubClassOf => "SubClassOf",
                CustomAxiomType::DisjointWith => "DisjointWith",
                CustomAxiomType::EquivalentTo => "EquivalentTo",
                CustomAxiomType::FunctionalProperty => "FunctionalProperty",
            };

            let inferred = InferredAxiom {
                id: uuid::Uuid::new_v4().to_string(),
                ontology_id: ontology_id.to_string(),
                axiom_type: axiom_type_str.to_string(),
                subject_iri: axiom.subject.clone(),
                object_iri: axiom.object.clone(),
                property_iri: None,
                confidence: axiom.confidence,
                inference_path: vec![], // Inference path tracking deferred to future enhancement
                user_defined: false,
            };
            inferred_axioms.push(inferred);
        }

        // Store inferred axioms in database
        self.store_inferred_axioms(&inferred_axioms).await?;

        // Cache the results
        let cache_entry = InferenceCacheEntry {
            ontology_id: ontology_id.to_string(),
            ontology_checksum: checksum,
            inferred_axioms: inferred_axioms.clone(),
            timestamp: time::now(),
            inference_time_ms: start.elapsed().as_millis() as u64,
        };
        self.cache_inference_results(cache_entry).await;

        info!(
            "Inference complete: {} axioms inferred in {:?}ms",
            inferred_axioms.len(),
            start.elapsed().as_millis()
        );

        Ok(inferred_axioms)
    }

    /// Get the class hierarchy for an ontology
    /// # Arguments
    /// * `ontology_id` - Ontology identifier
    /// # Returns
    /// Complete class hierarchy with depth and node counts
    #[instrument(skip(self), level = "info")]
    pub async fn get_class_hierarchy(
        &self,
        ontology_id: &str,
    ) -> Result<ClassHierarchy, OntologyRepositoryError> {
        info!("Computing class hierarchy for ontology: {}", ontology_id);

        let classes = self.ontology_repo.get_classes().await?;
        let axioms = self.ontology_repo.get_axioms().await?;

        // Build parent-child relationships
        let mut parent_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut child_map: HashMap<String, String> = HashMap::new();

        for axiom in &axioms {
            if axiom.axiom_type == AxiomType::SubClassOf {
                parent_map
                    .entry(axiom.object.clone())
                    .or_insert_with(Vec::new)
                    .push(axiom.subject.clone());
                child_map.insert(axiom.subject.clone(), axiom.object.clone());
            }
        }

        // Find root classes (classes with no parents)
        let all_iris: HashSet<String> = classes.iter().map(|c| c.iri.clone()).collect();
        let root_classes: Vec<String> = all_iris
            .iter()
            .filter(|iri| !child_map.contains_key(*iri))
            .cloned()
            .collect();

        // Build hierarchy nodes
        let mut hierarchy = HashMap::new();
        for class in &classes {
            let children = parent_map.get(&class.iri).cloned().unwrap_or_default();
            let parent = child_map.get(&class.iri).cloned();

            let node = ClassNode {
                iri: class.iri.clone(),
                label: class.label.clone().unwrap_or_else(|| class.iri.clone()),
                parent_iri: parent,
                children_iris: children.clone(),
                node_count: self.count_descendants(&children, &parent_map),
                depth: self.calculate_depth(&class.iri, &child_map),
            };
            hierarchy.insert(class.iri.clone(), node);
        }

        let class_hierarchy = ClassHierarchy {
            root_classes,
            hierarchy,
        };

        debug!(
            "Computed hierarchy with {} root classes and {} total nodes",
            class_hierarchy.root_classes.len(),
            class_hierarchy.hierarchy.len()
        );

        Ok(class_hierarchy)
    }

    /// Get disjoint class pairs
    /// # Arguments
    /// * `ontology_id` - Ontology identifier
    /// # Returns
    /// Vector of disjoint class pairs with explanations
    #[instrument(skip(self), level = "info")]
    pub async fn get_disjoint_classes(
        &self,
        ontology_id: &str,
    ) -> Result<Vec<DisjointPair>, OntologyRepositoryError> {
        info!("Finding disjoint classes for ontology: {}", ontology_id);

        let axioms = self.ontology_repo.get_axioms().await?;

        let mut disjoint_pairs = Vec::new();

        for axiom in &axioms {
            if axiom.axiom_type == AxiomType::DisjointWith {
                let pair = DisjointPair {
                    class_a: axiom.subject.clone(),
                    class_b: axiom.object.clone(),
                    reason: "Explicit DisjointWith axiom".to_string(),
                };
                disjoint_pairs.push(pair);
            }
        }

        debug!("Found {} disjoint class pairs", disjoint_pairs.len());

        Ok(disjoint_pairs)
    }

    /// Clear inference cache
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        info!("Cleared inference cache");
    }

    /// Calculate ontology checksum for cache invalidation
    async fn calculate_ontology_checksum(
        &self,
        ontology_id: &str,
    ) -> Result<String, OntologyRepositoryError> {
        let classes = self.ontology_repo.get_classes().await?;
        let axioms = self.ontology_repo.get_axioms().await?;

        use blake3::Hasher;
        let mut hasher = Hasher::new();

        hasher.update(ontology_id.as_bytes());
        hasher.update(&classes.len().to_le_bytes());
        hasher.update(&axioms.len().to_le_bytes());

        for class in &classes {
            hasher.update(class.iri.as_bytes());
        }

        for axiom in &axioms {
            hasher.update(axiom.subject.as_bytes());
            hasher.update(axiom.object.as_bytes());
        }

        Ok(hasher.finalize().to_hex().to_string())
    }

    /// Get cached inference results if valid
    async fn get_cached_inference(
        &self,
        ontology_id: &str,
        checksum: &str,
    ) -> Option<InferenceCacheEntry> {
        let cache = self.cache.read().await;
        cache.get(ontology_id).and_then(|entry| {
            if entry.ontology_checksum == checksum {
                Some(entry.clone())
            } else {
                None
            }
        })
    }

    /// Cache inference results
    async fn cache_inference_results(&self, entry: InferenceCacheEntry) {
        let mut cache = self.cache.write().await;
        cache.insert(entry.ontology_id.clone(), entry);
    }

    /// Store inferred axioms in database
    async fn store_inferred_axioms(
        &self,
        axioms: &[InferredAxiom],
    ) -> Result<(), OntologyRepositoryError> {
        for axiom in axioms {
            let owl_axiom = OwlAxiom {
                id: None,
                axiom_type: self.string_to_axiom_type(&axiom.axiom_type),
                subject: axiom.subject_iri.clone(),
                object: axiom.object_iri.clone().unwrap_or_default(),
                annotations: HashMap::from([
                    ("inferred".to_string(), "true".to_string()),
                    ("confidence".to_string(), axiom.confidence.to_string()),
                ]),
            };

            // Store in owl_axioms table with user_defined=false
            // Note: The table doesn't have user_defined column yet,
            // we'll use annotations to track this
            self.ontology_repo.add_axiom(&owl_axiom).await?;
        }

        Ok(())
    }

    /// Convert axiom type enum to string
    fn axiom_type_to_string(&self, axiom_type: &AxiomType) -> String {
        match axiom_type {
            AxiomType::SubClassOf => "SubClassOf".to_string(),
            AxiomType::EquivalentClass => "EquivalentClass".to_string(),
            AxiomType::DisjointWith => "DisjointWith".to_string(),
            AxiomType::ObjectPropertyAssertion => "ObjectPropertyAssertion".to_string(),
            AxiomType::DataPropertyAssertion => "DataPropertyAssertion".to_string(),
        }
    }

    /// Convert string to axiom type enum
    fn string_to_axiom_type(&self, s: &str) -> AxiomType {
        match s {
            "SubClassOf" => AxiomType::SubClassOf,
            "EquivalentClass" => AxiomType::EquivalentClass,
            "DisjointWith" => AxiomType::DisjointWith,
            "ObjectPropertyAssertion" => AxiomType::ObjectPropertyAssertion,
            "DataPropertyAssertion" => AxiomType::DataPropertyAssertion,
            _ => AxiomType::SubClassOf, // default
        }
    }

    /// Count total descendants in hierarchy
    fn count_descendants(
        &self,
        children: &[String],
        parent_map: &HashMap<String, Vec<String>>,
    ) -> usize {
        let mut count = children.len();
        for child in children {
            if let Some(grandchildren) = parent_map.get(child) {
                count += self.count_descendants(grandchildren, parent_map);
            }
        }
        count
    }

    /// Calculate depth in hierarchy
    fn calculate_depth(&self, iri: &str, child_map: &HashMap<String, String>) -> usize {
        let mut depth = 0;
        let mut current = iri;

        while let Some(parent) = child_map.get(current) {
            depth += 1;
            current = parent;

            // Prevent infinite loops
            if depth > 100 {
                warn!("Possible cycle detected in hierarchy for {}", iri);
                break;
            }
        }

        depth
    }
}

// TODO: Update all tests to use Neo4j test containers
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // TODO: Update tests to use Neo4j test containers
    // #[tokio::test]
    // async fn test_create_service() {
    //     let engine = Arc::new(WhelkInferenceEngine::new());
    //     let repo = Arc::new(/* TODO: Use Neo4j test container */);
    //
    //     let service = OntologyReasoningService::new(engine, repo);
    //
    //     // Service should initialize without errors
    //     service.clear_cache().await;
    // }

    // #[tokio::test]
    // async fn test_hierarchy_depth_calculation() {
    //     let engine = Arc::new(WhelkInferenceEngine::new());
    //     let repo = Arc::new(/* TODO: Use Neo4j test container */);
    //
    //     let service = OntologyReasoningService::new(engine, repo);
    //
    //     let mut child_map = HashMap::new();
    //     child_map.insert("child".to_string(), "parent".to_string());
    //     child_map.insert("parent".to_string(), "grandparent".to_string());
    //
    //     let depth = service.calculate_depth("child", &child_map);
    //     assert_eq!(depth, 2);
    // }

    // #[tokio::test]
    // async fn test_descendant_counting() {
    //     // Test disabled until Neo4j test containers are available
    //     let engine = Arc::new(WhelkInferenceEngine::new());
    //     let repo = Arc::new(/* TODO: Use Neo4j test container */);
    //
    //     let service = OntologyReasoningService::new(engine, repo);
    //
    //     let mut parent_map = HashMap::new();
    //     parent_map.insert(
    //         "parent".to_string(),
    //         vec!["child1".to_string(), "child2".to_string()],
    //     );
    //     parent_map.insert("child1".to_string(), vec!["grandchild".to_string()]);
    //
    //     let count = service.count_descendants(
    //         &vec!["child1".to_string(), "child2".to_string()],
    //         &parent_map,
    //     );
    //
    //     // 2 children + 1 grandchild = 3 total descendants
    //     assert_eq!(count, 3);
    // }
}
