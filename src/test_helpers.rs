//! Shared test helpers for repository-backed tests
//!
//! Provides:
//! - `MockOntologyRepository`: In-memory implementation of `OntologyRepository` for unit tests
//! - `create_test_ontology_repo()`: Convenience factory for mock repos pre-loaded with test data
//!
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::ports::ontology_repository::{
    InferenceResults, OntologyMetrics, OntologyRepository, OwlAxiom, OwlClass, OwlProperty,
    Result as OntResult,
};

// ---------------------------------------------------------------------------
// Oxigraph test store helpers (for integration tests)
// ---------------------------------------------------------------------------

/// Check if an Oxigraph in-memory store is available for testing.
/// Always returns `true` — Oxigraph is embedded and requires no external service.
pub async fn oxigraph_available() -> bool {
    true
}

/// Skip the current test if the Oxigraph store cannot be opened.
/// In practice this never fires — Oxigraph is embedded (ADR-11).
/// Usage: `skip_without_oxigraph!();` at the top of an async test.
#[macro_export]
macro_rules! skip_without_oxigraph {
    () => {
        if !$crate::test_helpers::oxigraph_available().await {
            eprintln!("SKIPPED: Oxigraph store not available");
            return;
        }
    };
}

// ---------------------------------------------------------------------------
// MockOntologyRepository -- in-memory impl for unit tests
// ---------------------------------------------------------------------------

/// In-memory `OntologyRepository` implementation for unit testing.
/// No external database required — backed entirely by in-process HashMaps.
pub struct MockOntologyRepository {
    pub classes: RwLock<HashMap<String, OwlClass>>,
    pub properties: RwLock<HashMap<String, OwlProperty>>,
    pub axioms: RwLock<Vec<OwlAxiom>>,
    pub next_axiom_id: RwLock<u64>,
    pub graph: RwLock<Option<GraphData>>,
    pub inference_results: RwLock<Option<InferenceResults>>,
}

impl MockOntologyRepository {
    pub fn new() -> Self {
        Self {
            classes: RwLock::new(HashMap::new()),
            properties: RwLock::new(HashMap::new()),
            axioms: RwLock::new(Vec::new()),
            next_axiom_id: RwLock::new(1),
            graph: RwLock::new(None),
            inference_results: RwLock::new(None),
        }
    }
}

impl Default for MockOntologyRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OntologyRepository for MockOntologyRepository {
    async fn load_ontology_graph(&self) -> OntResult<Arc<GraphData>> {
        let g = self.graph.read().await;
        match &*g {
            Some(graph) => Ok(Arc::new(graph.clone())),
            None => Ok(Arc::new(GraphData::default())),
        }
    }

    async fn save_ontology_graph(&self, graph: &GraphData) -> OntResult<()> {
        let mut g = self.graph.write().await;
        *g = Some(graph.clone());
        Ok(())
    }

    async fn save_ontology(
        &self,
        classes: &[OwlClass],
        properties: &[OwlProperty],
        axioms: &[OwlAxiom],
    ) -> OntResult<()> {
        {
            let mut c = self.classes.write().await;
            for class in classes {
                c.insert(class.iri.clone(), class.clone());
            }
        }
        {
            let mut p = self.properties.write().await;
            for prop in properties {
                p.insert(prop.iri.clone(), prop.clone());
            }
        }
        {
            let mut a = self.axioms.write().await;
            for axiom in axioms {
                a.push(axiom.clone());
            }
        }
        Ok(())
    }

    async fn add_owl_class(&self, class: &OwlClass) -> OntResult<String> {
        let mut c = self.classes.write().await;
        c.insert(class.iri.clone(), class.clone());
        Ok(class.iri.clone())
    }

    async fn get_owl_class(&self, iri: &str) -> OntResult<Option<OwlClass>> {
        let c = self.classes.read().await;
        Ok(c.get(iri).cloned())
    }

    async fn list_owl_classes(&self) -> OntResult<Vec<OwlClass>> {
        let c = self.classes.read().await;
        Ok(c.values().cloned().collect())
    }

    async fn add_owl_property(&self, property: &OwlProperty) -> OntResult<String> {
        let mut p = self.properties.write().await;
        p.insert(property.iri.clone(), property.clone());
        Ok(property.iri.clone())
    }

    async fn get_owl_property(&self, iri: &str) -> OntResult<Option<OwlProperty>> {
        let p = self.properties.read().await;
        Ok(p.get(iri).cloned())
    }

    async fn list_owl_properties(&self) -> OntResult<Vec<OwlProperty>> {
        let p = self.properties.read().await;
        Ok(p.values().cloned().collect())
    }

    async fn get_classes(&self) -> OntResult<Vec<OwlClass>> {
        self.list_owl_classes().await
    }

    async fn get_axioms(&self) -> OntResult<Vec<OwlAxiom>> {
        let a = self.axioms.read().await;
        Ok(a.clone())
    }

    async fn add_axiom(&self, axiom: &OwlAxiom) -> OntResult<u64> {
        let mut id_lock = self.next_axiom_id.write().await;
        let id = *id_lock;
        *id_lock += 1;

        let mut stored = axiom.clone();
        stored.id = Some(id);

        let mut a = self.axioms.write().await;
        a.push(stored);
        Ok(id)
    }

    async fn get_class_axioms(&self, class_iri: &str) -> OntResult<Vec<OwlAxiom>> {
        let a = self.axioms.read().await;
        Ok(a.iter()
            .filter(|ax| ax.subject == class_iri || ax.object == class_iri)
            .cloned()
            .collect())
    }

    async fn store_inference_results(&self, results: &InferenceResults) -> OntResult<()> {
        let mut ir = self.inference_results.write().await;
        *ir = Some(results.clone());
        Ok(())
    }

    async fn get_inference_results(&self) -> OntResult<Option<InferenceResults>> {
        let ir = self.inference_results.read().await;
        Ok(ir.clone())
    }

    async fn remove_owl_class(&self, iri: &str) -> OntResult<()> {
        let mut c = self.classes.write().await;
        c.remove(iri);
        Ok(())
    }

    async fn remove_axiom(&self, axiom_id: u64) -> OntResult<()> {
        let mut a = self.axioms.write().await;
        a.retain(|ax| ax.id != Some(axiom_id));
        Ok(())
    }

    async fn get_metrics(&self) -> OntResult<OntologyMetrics> {
        let c = self.classes.read().await;
        let p = self.properties.read().await;
        let a = self.axioms.read().await;
        Ok(OntologyMetrics {
            class_count: c.len(),
            property_count: p.len(),
            axiom_count: a.len(),
            max_depth: 0,
            average_branching_factor: 0.0,
        })
    }
}

// ---------------------------------------------------------------------------
// Convenience factories
// ---------------------------------------------------------------------------

/// Create a `MockOntologyRepository` pre-loaded with standard MindVault ontology classes.
/// Includes: mv:Person, mv:Company, mv:Technology, mv:Event, mv:Location, mv:Organization.
pub fn create_test_ontology_repo() -> Arc<MockOntologyRepository> {
    let repo = MockOntologyRepository::new();

    // Pre-populate with common test classes synchronously via direct lock
    let classes = vec![
        ("mv:Person", "Person"),
        ("mv:Company", "Company"),
        ("mv:Technology", "Technology"),
        ("mv:Event", "Event"),
        ("mv:Location", "Location"),
        ("mv:Organization", "Organization"),
    ];

    // Use try_write since this may be called from within a tokio runtime (e.g. #[tokio::test])
    // where blocking_write() would panic with "Cannot block the current thread"
    let mut class_map = repo
        .classes
        .try_write()
        .expect("RwLock should be available in test setup");
    for (iri, label) in classes {
        class_map.insert(
            iri.to_string(),
            OwlClass {
                iri: iri.to_string(),
                label: Some(label.to_string()),
                preferred_term: Some(label.to_string()),
                ..OwlClass::default()
            },
        );
    }
    drop(class_map);

    Arc::new(repo)
}

/// Create an `OntologyReasoner` backed by a mock repository for unit testing.
pub fn create_test_reasoner() -> crate::services::ontology_reasoner::OntologyReasoner {
    let repo = create_test_ontology_repo();
    crate::services::ontology_reasoner::OntologyReasoner::new(repo)
}

/// Create an `OntologyEnrichmentService` backed by mock implementations for unit testing.
pub fn create_test_enrichment_service(
) -> crate::services::ontology_enrichment_service::OntologyEnrichmentService {
    let reasoner = Arc::new(create_test_reasoner());
    let classifier = Arc::new(crate::services::edge_classifier::EdgeClassifier::new());
    crate::services::ontology_enrichment_service::OntologyEnrichmentService::new(
        reasoner, classifier,
    )
}

/// Create an `OntologyReasoningService` backed by a mock repository for unit testing.
pub fn create_test_reasoning_service(
) -> crate::services::ontology_reasoning_service::OntologyReasoningService {
    let repo = create_test_ontology_repo();
    crate::services::ontology_reasoning_service::OntologyReasoningService::new(repo)
}
