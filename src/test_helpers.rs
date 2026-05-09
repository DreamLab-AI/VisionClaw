//! Shared test helpers for Neo4j-dependent tests
//!
//! Provides:
//! - `MockOntologyRepository`: In-memory implementation of `OntologyRepository` for unit tests
//! - `Neo4jTestConfig`: Connection config for integration tests against a real Neo4j instance
//! - `neo4j_available()`: Async check for Neo4j availability (skips tests in CI without Neo4j)
//! - `create_test_ontology_repo()`: Convenience factory for mock repos pre-loaded with test data

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::models::graph::GraphData;
use crate::ports::ontology_repository::{
    AxiomType, InferenceResults, OntologyMetrics, OntologyRepository, OntologyRepositoryError,
    OwlAxiom, OwlClass, OwlProperty, PathfindingCacheEntry, PropertyType, ValidationReport,
    Result as OntResult,
};

// ---------------------------------------------------------------------------
// Neo4j test connection config (for integration tests)
// ---------------------------------------------------------------------------

/// Neo4j test connection configuration.
/// Reads from environment variables with sensible defaults for local dev.
pub struct Neo4jTestConfig {
    pub uri: String,
    pub username: String,
    pub password: String,
}

impl Default for Neo4jTestConfig {
    fn default() -> Self {
        Self {
            uri: std::env::var("NEO4J_TEST_URI")
                .unwrap_or_else(|_| "bolt://localhost:7687".to_string()),
            username: std::env::var("NEO4J_TEST_USER")
                .unwrap_or_else(|_| "neo4j".to_string()),
            password: std::env::var("NEO4J_TEST_PASSWORD")
                .unwrap_or_else(|_| "testpassword".to_string()),
        }
    }
}

/// Check if a Neo4j test instance is reachable.
/// Returns `false` when unavailable so tests can be skipped gracefully.
pub async fn neo4j_available() -> bool {
    let config = Neo4jTestConfig::default();
    match neo4rs::Graph::new(&config.uri, &config.username, &config.password) {
        Ok(graph) => graph.run(neo4rs::query("RETURN 1")).await.is_ok(),
        Err(_) => false,
    }
}

/// Skip the current test if Neo4j is not available.
/// Usage: `skip_without_neo4j!();` at the top of an async test.
#[macro_export]
macro_rules! skip_without_neo4j {
    () => {
        if !crate::test_helpers::neo4j_available().await {
            eprintln!("SKIPPED: Neo4j test instance not available");
            return;
        }
    };
}

// ---------------------------------------------------------------------------
// MockOntologyRepository -- in-memory impl for unit tests
// ---------------------------------------------------------------------------

/// In-memory `OntologyRepository` implementation for unit testing.
/// No Neo4j connection required.
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
    let mut class_map = repo.classes.try_write().expect("RwLock should be available in test setup");
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
    let engine = Arc::new(crate::adapters::whelk_inference_engine::WhelkInferenceEngine::new());
    let repo = create_test_ontology_repo();
    crate::services::ontology_reasoner::OntologyReasoner::new(engine, repo)
}

/// Create an `OntologyEnrichmentService` backed by mock implementations for unit testing.
pub fn create_test_enrichment_service() -> crate::services::ontology_enrichment_service::OntologyEnrichmentService {
    let reasoner = Arc::new(create_test_reasoner());
    let classifier = Arc::new(crate::services::edge_classifier::EdgeClassifier::new());
    crate::services::ontology_enrichment_service::OntologyEnrichmentService::new(reasoner, classifier)
}

// ---------------------------------------------------------------------------
// Graph fixture factories
// ---------------------------------------------------------------------------

/// Create a test `Node` with sensible defaults.
/// The `id` field is deterministic (from `provided_id`), avoiding the global
/// `NEXT_NODE_ID` atomic counter so tests don't interfere with each other.
pub fn make_test_node(id: u32, label: &str) -> crate::models::node::Node {
    use crate::utils::socket_flow_messages::BinaryNodeData;
    crate::models::node::Node {
        id,
        metadata_id: format!("test-meta-{}", id),
        label: label.to_string(),
        data: BinaryNodeData {
            node_id: id,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        },
        x: Some(0.0),
        y: Some(0.0),
        z: Some(0.0),
        vx: Some(0.0),
        vy: Some(0.0),
        vz: Some(0.0),
        mass: Some(1.0),
        owl_class_iri: None,
        metadata: HashMap::new(),
        file_size: 0,
        node_type: None,
        size: None,
        color: None,
        weight: None,
        group: None,
        user_data: None,
        visibility: crate::models::node::Visibility::Public,
        owner_pubkey: None,
        opaque_id: None,
        pod_url: None,
        canonical_iri: None,
        visionclaw_uri: None,
        rdf_type: None,
        same_as: None,
        domain: None,
        content_hash: None,
        quality_score: None,
        authority_score: None,
        preferred_term: None,
        graph_source: None,
        kind_id: None,
    }
}

/// Create a test `Edge` between two node IDs with a given weight.
pub fn make_test_edge(source: u32, target: u32, weight: f32) -> crate::models::edge::Edge {
    crate::models::edge::Edge::new(source, target, weight)
}

/// Create a `GraphData` with the given number of nodes and a chain of edges.
/// Useful for layout and physics tests.
pub fn make_test_graph(node_count: u32) -> GraphData {
    let mut gd = GraphData::new();
    for i in 1..=node_count {
        gd.nodes.push(make_test_node(i, &format!("node_{}", i)));
    }
    for i in 1..node_count {
        gd.edges.push(make_test_edge(i, i + 1, 1.0));
    }
    gd
}

/// Create a test `UserContext` for handler tests.
pub fn make_test_user_context() -> crate::types::user_context::UserContext {
    crate::types::user_context::UserContext {
        user_id: "npub1testuser00000000000000000000000000000000000000000000000000".to_string(),
        pubkey: "a".repeat(64),
        display_name: "test_user".to_string(),
        session_id: "test-session-001".to_string(),
        is_power_user: false,
    }
}

/// Create a test `NostrUser` with deterministic values.
pub fn make_test_nostr_user(pubkey: &str) -> crate::models::protected_settings::NostrUser {
    crate::models::protected_settings::NostrUser {
        pubkey: pubkey.to_string(),
        npub: format!("npub1{}", &pubkey[..pubkey.len().min(58)]),
        is_power_user: false,
        api_keys: crate::models::protected_settings::ApiKeys::default(),
        last_seen: crate::utils::time::timestamp_seconds(),
        session_token: Some("test-token-12345".to_string()),
    }
}

/// Create a test `BriefingRequest` with standard values.
pub fn make_test_briefing_request() -> crate::types::user_context::BriefingRequest {
    crate::types::user_context::BriefingRequest {
        content: "Test briefing content for analysis".to_string(),
        roles: vec!["architect".to_string(), "dev".to_string()],
        version: Some("v0.1.0".to_string()),
        brief_type: Some("feature-request".to_string()),
        slug: Some("test-brief".to_string()),
    }
}

/// Create a test `RoleTask`.
pub fn make_test_role_task(role: &str) -> crate::types::user_context::RoleTask {
    crate::types::user_context::RoleTask {
        role: role.to_string(),
        task_id: format!("task-{}-001", role),
        bead_id: Some(format!("bead-{}-001", role)),
        response_path: format!("/briefs/test/{}_response.md", role),
    }
}

// ---------------------------------------------------------------------------
// Self-tests for test_helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_test_node_deterministic_id() {
        let node = make_test_node(42, "hello");
        assert_eq!(node.id, 42);
        assert_eq!(node.label, "hello");
        assert_eq!(node.metadata_id, "test-meta-42");
    }

    #[test]
    fn test_make_test_edge() {
        let edge = make_test_edge(1, 2, 0.75);
        assert_eq!(edge.source, 1);
        assert_eq!(edge.target, 2);
        assert!((edge.weight - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn test_make_test_graph_structure() {
        let graph = make_test_graph(5);
        assert_eq!(graph.nodes.len(), 5);
        assert_eq!(graph.edges.len(), 4); // chain: 1-2, 2-3, 3-4, 4-5
    }

    #[test]
    fn test_make_test_user_context() {
        let ctx = make_test_user_context();
        assert_eq!(ctx.display_name, "test_user");
        assert!(!ctx.is_power_user);
        assert_eq!(ctx.pubkey.len(), 64);
    }

    #[test]
    fn test_make_test_nostr_user() {
        let pubkey = "b".repeat(64);
        let user = make_test_nostr_user(&pubkey);
        assert_eq!(user.pubkey, pubkey);
        assert!(!user.is_power_user);
        assert!(user.session_token.is_some());
    }

    #[test]
    fn test_make_test_briefing_request() {
        let req = make_test_briefing_request();
        assert_eq!(req.roles.len(), 2);
        assert!(req.content.contains("briefing"));
    }

    #[test]
    fn test_make_test_role_task() {
        let task = make_test_role_task("architect");
        assert_eq!(task.role, "architect");
        assert!(task.task_id.contains("architect"));
        assert!(task.bead_id.is_some());
    }

    #[tokio::test]
    async fn test_mock_ontology_repo_roundtrip() {
        let repo = create_test_ontology_repo();
        let classes = repo.list_owl_classes().await.unwrap();
        assert_eq!(classes.len(), 6); // 6 pre-loaded classes
        let person = repo.get_owl_class("mv:Person").await.unwrap();
        assert!(person.is_some());
        assert_eq!(person.unwrap().label, Some("Person".to_string()));
    }

    #[tokio::test]
    async fn test_mock_ontology_repo_add_and_remove() {
        let repo = MockOntologyRepository::new();
        let cls = OwlClass {
            iri: "test:Foo".to_string(),
            label: Some("Foo".to_string()),
            ..OwlClass::default()
        };
        repo.add_owl_class(&cls).await.unwrap();
        assert!(repo.get_owl_class("test:Foo").await.unwrap().is_some());
        repo.remove_owl_class("test:Foo").await.unwrap();
        assert!(repo.get_owl_class("test:Foo").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_ontology_repo_metrics() {
        let repo = create_test_ontology_repo();
        let metrics = repo.get_metrics().await.unwrap();
        assert_eq!(metrics.class_count, 6);
        assert_eq!(metrics.property_count, 0);
        assert_eq!(metrics.axiom_count, 0);
    }
}
