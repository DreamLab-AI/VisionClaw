//! Integration tests for the Ontology Agent pipeline.
//!
//! Tests the OntologyQueryService and OntologyMutationService using mock
//! repositories and a real WhelkInferenceEngine to verify:
//!   - Semantic discovery with keyword matching and Whelk expansion
//!   - Enriched note reading with axioms and related notes
//!   - Cypher query validation against OWL schema
//!   - Proposal creation with Whelk consistency checks
//!   - Vault markdown generation with YAML frontmatter (ADR-2040 §V5)
//!   - Amendment workflow for existing notes

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;
use visionclaw_server::adapters::whelk_inference_engine::WhelkInferenceEngine;
use visionclaw_server::models::edge::Edge;
use visionclaw_server::models::graph::GraphData;
use visionclaw_server::models::node::Node;
use visionclaw_server::ports::knowledge_graph_repository::{
    GraphStatistics, KnowledgeGraphRepository, Result as KGResult,
};
use visionclaw_server::services::github_pr_service::GitHubPRService;
use visionclaw_server::services::ontology_mutation_service::OntologyMutationService;
use visionclaw_server::services::ontology_query_service::OntologyQueryService;
use visionclaw_server::services::proposal_spine::{InMemoryIdempotencyStore, InMemoryIntentLog};
use visionclaw_server::services::schema_service::SchemaService;
use visionclaw_server::test_helpers::create_test_ontology_repo;
use visionclaw_server::types::ontology_tools::*;

// ---------- Minimal KG repo mock ----------

struct EmptyKGRepo;

#[async_trait]
impl KnowledgeGraphRepository for EmptyKGRepo {
    async fn load_graph(&self) -> KGResult<Arc<GraphData>> {
        Ok(Arc::new(GraphData::default()))
    }
    async fn save_graph(&self, _g: &GraphData) -> KGResult<()> {
        Ok(())
    }
    async fn add_node(&self, _n: &Node) -> KGResult<u32> {
        Ok(0)
    }
    async fn batch_add_nodes(&self, _n: Vec<Node>) -> KGResult<Vec<u32>> {
        Ok(vec![])
    }
    async fn batch_add_nodes_if_absent(&self, _n: Vec<Node>) -> KGResult<Vec<u32>> {
        Ok(vec![])
    }
    async fn update_node(&self, _n: &Node) -> KGResult<()> {
        Ok(())
    }
    async fn batch_update_nodes(&self, _n: Vec<Node>) -> KGResult<()> {
        Ok(())
    }
    async fn remove_node(&self, _id: u32) -> KGResult<()> {
        Ok(())
    }
    async fn batch_remove_nodes(&self, _ids: Vec<u32>) -> KGResult<()> {
        Ok(())
    }
    async fn get_node(&self, _id: u32) -> KGResult<Option<Node>> {
        Ok(None)
    }
    async fn get_nodes(&self, _ids: Vec<u32>) -> KGResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_nodes_by_metadata_id(&self, _metadata_id: &str) -> KGResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_nodes_by_owl_class_iri(&self, _iri: &str) -> KGResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn search_nodes_by_label(&self, _label: &str) -> KGResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn add_edge(&self, _e: &Edge) -> KGResult<String> {
        Ok(String::new())
    }
    async fn batch_add_edges(&self, _edges: Vec<Edge>) -> KGResult<Vec<String>> {
        Ok(vec![])
    }
    async fn update_edge(&self, _e: &Edge) -> KGResult<()> {
        Ok(())
    }
    async fn remove_edge(&self, _id: &str) -> KGResult<()> {
        Ok(())
    }
    async fn batch_remove_edges(&self, _ids: Vec<String>) -> KGResult<()> {
        Ok(())
    }
    async fn get_node_edges(&self, _id: u32) -> KGResult<Vec<Edge>> {
        Ok(vec![])
    }
    async fn get_edges_between(&self, _source: u32, _target: u32) -> KGResult<Vec<Edge>> {
        Ok(vec![])
    }
    async fn batch_update_positions(&self, _positions: Vec<(u32, f32, f32, f32)>) -> KGResult<()> {
        Ok(())
    }
    async fn get_all_positions(&self) -> KGResult<HashMap<u32, (f32, f32, f32)>> {
        Ok(HashMap::new())
    }
    async fn query_nodes(&self, _query: &str) -> KGResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_neighbors(&self, _id: u32) -> KGResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_statistics(&self) -> KGResult<GraphStatistics> {
        Ok(GraphStatistics {
            node_count: 0,
            edge_count: 0,
            average_degree: 0.0,
            connected_components: 0,
            last_updated: chrono::Utc::now(),
        })
    }
    async fn clear_graph(&self) -> KGResult<()> {
        Ok(())
    }
    async fn health_check(&self) -> KGResult<bool> {
        Ok(true)
    }
}

// ---------- Test Helpers ----------

fn build_query_service() -> OntologyQueryService {
    let repo = create_test_ontology_repo();
    let whelk = Arc::new(RwLock::new(WhelkInferenceEngine::new()));
    let schema_service = Arc::new(SchemaService::new());
    OntologyQueryService::new(repo, Arc::new(EmptyKGRepo), whelk, schema_service)
}

fn build_mutation_service() -> OntologyMutationService {
    let repo = create_test_ontology_repo();
    let whelk = Arc::new(RwLock::new(WhelkInferenceEngine::new()));
    let github_pr = Arc::new(GitHubPRService::new());
    OntologyMutationService::new(
        repo,
        whelk,
        github_pr,
        Arc::new(InMemoryIdempotencyStore::new()),
        Arc::new(InMemoryIntentLog::new()),
    )
}

fn build_mutation_service_with_markdown() -> OntologyMutationService {
    let repo = create_test_ontology_repo();
    {
        let mut classes = repo
            .classes
            .try_write()
            .expect("lock available in test setup");
        if let Some(person) = classes.get_mut("mv:Person") {
            person.markdown_content = Some(
                "- Person\n  - ### OntologyBlock\n    - ontology:: true\n    - definition:: A human being\n"
                    .to_string(),
            );
            person.source_domain = Some("mv".to_string());
            person.term_id = Some("MV-0001".to_string());
        }
    }
    let whelk = Arc::new(RwLock::new(WhelkInferenceEngine::new()));
    let github_pr = Arc::new(GitHubPRService::new());
    OntologyMutationService::new(
        repo,
        whelk,
        github_pr,
        Arc::new(InMemoryIdempotencyStore::new()),
        Arc::new(InMemoryIntentLog::new()),
    )
}

fn test_agent_context() -> AgentContext {
    AgentContext {
        agent_id: "test-agent-001".to_string(),
        agent_type: "researcher".to_string(),
        task_description: "Integration test task".to_string(),
        session_id: Some("test-session".to_string()),
        confidence: 0.85,
        user_id: "test-user".to_string(),
    }
}

// ---------- Discovery Tests ----------

#[tokio::test]
async fn test_discover_finds_matching_classes() {
    let service = build_query_service();
    let results = service.discover("Person", 10, None).await.unwrap();
    assert!(
        !results.is_empty(),
        "Should find at least one result for 'Person'"
    );
    assert_eq!(results[0].preferred_term, "Person");
    assert!(results[0].relevance_score > 0.0);
}

#[tokio::test]
async fn test_discover_respects_limit() {
    let service = build_query_service();
    let results = service.discover("o", 2, None).await.unwrap();
    assert!(results.len() <= 2, "Should respect limit parameter");
}

#[tokio::test]
async fn test_discover_nonexistent_returns_empty() {
    let service = build_query_service();
    let results = service
        .discover("zzz_nonexistent_xyzzy", 10, None)
        .await
        .unwrap();
    assert!(
        results.is_empty(),
        "Nonsense query should return no results"
    );
}

#[tokio::test]
async fn test_discover_multi_term_query() {
    let service = build_query_service();
    let results = service
        .discover("Company Organization", 10, None)
        .await
        .unwrap();
    assert!(!results.is_empty(), "Multi-term query should match classes");
}

// ---------- Read Note Tests ----------

#[tokio::test]
async fn test_read_note_existing_class() {
    let service = build_query_service();
    let note = service.read_note("mv:Person").await.unwrap();
    assert_eq!(note.iri, "mv:Person");
    assert_eq!(note.preferred_term, "Person");
}

#[tokio::test]
async fn test_read_note_missing_class_returns_error() {
    let service = build_query_service();
    let result = service.read_note("mv:NonExistent").await;
    assert!(result.is_err(), "Should error for missing class");
}

// ---------- Cypher Validation Tests ----------

#[tokio::test]
async fn test_validate_cypher_known_label() {
    let service = build_query_service();
    let result = service
        .validate_and_execute_cypher("MATCH (n:Person) RETURN n")
        .await
        .unwrap();
    assert!(result.valid, "Person is a known class — should validate");
    assert!(result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_cypher_unknown_label_with_hint() {
    let service = build_query_service();
    let result = service
        .validate_and_execute_cypher("MATCH (n:Perzon) RETURN n")
        .await
        .unwrap();
    assert!(!result.valid, "Perzon is not a known class — should fail");
    assert!(!result.errors.is_empty());
    assert!(
        result
            .hints
            .iter()
            .any(|h| h.to_lowercase().contains("person")),
        "Should hint 'Person' for 'Perzon': {:?}",
        result.hints
    );
}

#[tokio::test]
async fn test_validate_cypher_builtin_labels_pass() {
    let service = build_query_service();
    let result = service
        .validate_and_execute_cypher("MATCH (n:OwlClass) RETURN n LIMIT 10")
        .await
        .unwrap();
    assert!(
        result.valid,
        "OwlClass is a built-in label — should validate"
    );
}

// ---------- Proposal Tests ----------

#[tokio::test]
async fn test_propose_create_generates_valid_result() {
    let mutation_service = build_mutation_service();
    let proposal = NoteProposal {
        preferred_term: "Quantum Computing".to_string(),
        definition: "A type of computation using quantum mechanics".to_string(),
        owl_class: "mv:QuantumComputing".to_string(),
        physicality: "non-physical".to_string(),
        role: "concept".to_string(),
        domain: "tc".to_string(),
        is_subclass_of: vec!["mv:Technology".to_string()],
        relationships: HashMap::new(),
        alt_terms: vec!["QC".to_string()],
        owner_user_id: Some("test-user".to_string()),
    };

    let result = mutation_service
        .propose_create(proposal, test_agent_context(), None, None)
        .await
        .unwrap();

    assert_eq!(result.action, "create");
    assert!(
        result.consistency.consistent,
        "Should pass Whelk consistency"
    );
    assert!(
        result.quality_score > 0.5,
        "Fully-specified proposal should score well"
    );
    assert!(!result.proposal_id.is_empty());
    assert!(!result.markdown_preview.is_empty());
    assert!(
        result.pr_url.is_none(),
        "PR should not be created without PRIVATE_REPO_GITHUB_PAT"
    );
    match result.status {
        ProposalStatus::Staged => {}
        _ => panic!(
            "Expected Staged status without PRIVATE_REPO_GITHUB_PAT, got: {:?}",
            result.status
        ),
    }
}

#[tokio::test]
async fn test_propose_create_markdown_has_v2_frontmatter() {
    let mutation_service = build_mutation_service();
    let proposal = NoteProposal {
        preferred_term: "Neural Network".to_string(),
        definition: "A computational model inspired by biological neural networks".to_string(),
        owl_class: "ai:NeuralNetwork".to_string(),
        physicality: "non-physical".to_string(),
        role: "concept".to_string(),
        domain: "ai".to_string(),
        is_subclass_of: vec!["ai:MachineLearning".to_string()],
        relationships: {
            let mut r = HashMap::new();
            r.insert("requires".to_string(), vec!["ai:TrainingData".to_string()]);
            r
        },
        alt_terms: vec!["ANN".to_string(), "NN".to_string()],
        owner_user_id: Some("test-user".to_string()),
    };

    let result = mutation_service
        .propose_create(proposal, test_agent_context(), None, None)
        .await
        .unwrap();

    // ADR-2040 §V5: `generate_vault_markdown` no longer emits an indented
    // `- ### OntologyBlock` of Logseq `key:: value` lines — Obsidian rendered
    // those as plain text, so the pages were invisible to the owner's editor
    // and to the §V4 gate. It now emits a §V2 YAML frontmatter block. These
    // assertions track that contract; see
    // src/services/ontology_mutation_service.rs:42-51.
    let preview = &result.markdown_preview;
    assert!(
        preview.starts_with("---\n"),
        "Should open with a §V2 YAML frontmatter fence, got: {preview}"
    );
    assert!(
        preview.contains("ontology: 'true'"),
        "Should carry the ontology marker (serde_yaml quotes the string \"true\"), got: {preview}"
    );
    assert!(
        preview.contains("Neural Network"),
        "Should contain preferred term"
    );
    assert!(
        preview.contains("owl-class: ai:NeuralNetwork"),
        "Should contain OWL class under the §V2 `owl-class` key, got: {preview}"
    );
    assert!(
        preview.contains("status: agent-proposed"),
        "Should be agent-proposed"
    );
}

#[tokio::test]
async fn test_propose_amend_existing_class() {
    let mutation_service = build_mutation_service_with_markdown();
    let amendment = NoteAmendment {
        add_relationships: {
            let mut r = HashMap::new();
            r.insert("has-part".to_string(), vec!["mv:Brain".to_string()]);
            r
        },
        remove_relationships: HashMap::new(),
        update_definition: Some("A human being or sentient entity".to_string()),
        update_quality_score: Some(0.8),
        add_alt_terms: vec![],
        custom_fields: HashMap::new(),
    };

    let result = mutation_service
        .propose_amend("mv:Person", amendment, test_agent_context(), None, None)
        .await
        .unwrap();

    assert_eq!(result.action, "amend");
    assert!(result.consistency.consistent);
    assert!(result.markdown_preview.contains("sentient entity"));
}

// ---------- Quality Score Tests ----------

#[tokio::test]
async fn test_quality_score_fully_specified() {
    let mutation_service = build_mutation_service();
    let proposal = NoteProposal {
        preferred_term: "Test Concept".to_string(),
        definition: "A well-defined concept for testing".to_string(),
        owl_class: "mv:TestConcept".to_string(),
        physicality: "non-physical".to_string(),
        role: "concept".to_string(),
        domain: "mv".to_string(),
        is_subclass_of: vec!["mv:Thing".to_string()],
        relationships: {
            let mut r = HashMap::new();
            r.insert("related-to".to_string(), vec!["mv:Person".to_string()]);
            r
        },
        alt_terms: vec!["TC".to_string()],
        owner_user_id: Some("test-user".to_string()),
    };

    let result = mutation_service
        .propose_create(proposal, test_agent_context(), None, None)
        .await
        .unwrap();

    assert!(
        result.quality_score >= 0.8,
        "Fully specified proposal should have high quality score, got: {}",
        result.quality_score
    );
}

#[tokio::test]
async fn test_quality_score_minimal() {
    let mutation_service = build_mutation_service();
    let proposal = NoteProposal {
        preferred_term: "Bare".to_string(),
        definition: "".to_string(),
        owl_class: "mv:Bare".to_string(),
        physicality: "".to_string(),
        role: "".to_string(),
        domain: "mv".to_string(),
        is_subclass_of: vec![],
        relationships: HashMap::new(),
        alt_terms: vec![],
        owner_user_id: None,
    };

    let result = mutation_service
        .propose_create(proposal, test_agent_context(), None, None)
        .await
        .unwrap();

    assert!(
        result.quality_score < 0.8,
        "Minimal proposal should have lower quality score, got: {}",
        result.quality_score
    );
}
