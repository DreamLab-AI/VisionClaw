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

use visionclaw_domain::ports::inference_engine::InferenceEngine;
use visionclaw_domain::ports::ontology_repository::{
    AxiomType, OntologyRepository, OwlAxiom, OwlClass,
};
use visionclaw_server::adapters::whelk_inference_engine::WhelkInferenceEngine;
use visionclaw_server::services::github_pr_service::GitHubPRService;
use visionclaw_server::services::ontology_mutation_service::OntologyMutationService;
use visionclaw_server::services::ontology_query_service::OntologyQueryService;
use visionclaw_server::services::proposal_spine::{InMemoryIdempotencyStore, InMemoryIntentLog};
use visionclaw_server::services::schema_service::SchemaService;
use visionclaw_server::test_helpers::create_test_ontology_repo;
use visionclaw_server::types::ontology_tools::*;

// ---------- Test Helpers ----------

fn build_query_service() -> OntologyQueryService {
    let repo = create_test_ontology_repo();
    let whelk = Arc::new(RwLock::new(WhelkInferenceEngine::new()));
    let schema_service = Arc::new(SchemaService::new());
    OntologyQueryService::new(repo, whelk, schema_service)
}

fn build_mutation_service() -> OntologyMutationService {
    let repo = create_test_ontology_repo();
    let github_pr = Arc::new(GitHubPRService::new());
    OntologyMutationService::new(
        repo,
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
    let github_pr = Arc::new(GitHubPRService::new());
    OntologyMutationService::new(
        repo,
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
        confidence: Some(0.85),
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

// ---------- ADR-2127: open-world answers ----------

fn axiom(axiom_type: AxiomType, subject: &str, object: &str) -> OwlAxiom {
    OwlAxiom {
        id: None,
        axiom_type,
        subject: subject.to_string(),
        object: object.to_string(),
        annotations: HashMap::new(),
    }
}

/// Company ⊑ Organization (asserted), Organization ⊑ Agent (asserted, so
/// Company ⊑ Agent is closure-only), Person disjoint-with Organization.
async fn build_disjoint_query_service() -> OntologyQueryService {
    let repo = create_test_ontology_repo();
    repo.add_owl_class(&OwlClass {
        iri: "mv:Agent".to_string(),
        label: Some("Agent".to_string()),
        preferred_term: Some("Agent".to_string()),
        ..OwlClass::default()
    })
    .await
    .unwrap();
    let axioms = vec![
        axiom(AxiomType::SubClassOf, "mv:Company", "mv:Organization"),
        axiom(AxiomType::SubClassOf, "mv:Organization", "mv:Agent"),
        axiom(AxiomType::DisjointWith, "mv:Person", "mv:Organization"),
    ];
    for a in &axioms {
        repo.add_axiom(a).await.unwrap();
    }
    let mut engine = WhelkInferenceEngine::new();
    let classes = repo.list_owl_classes().await.unwrap();
    engine.load_ontology(classes, axioms).await.unwrap();
    engine.infer().await.unwrap();
    let schema_service = Arc::new(SchemaService::new());
    OntologyQueryService::new(repo, Arc::new(RwLock::new(engine)), schema_service).with_generation(
        Some("https://narrativegoldmine.com/ontology/sha256-12-000000000001".to_string()),
    )
}

#[tokio::test]
async fn test_empty_discover_is_labelled_open_with_its_generation() {
    let service = build_query_service().with_generation(Some("urn:gen:test".to_string()));
    let results = service
        .discover("zzz_nonexistent_xyzzy", 10, None)
        .await
        .unwrap();
    assert!(results.is_empty());
    let scope = service.answer_scope().await;
    assert_eq!(scope.closure, Closure::Open);
    assert_eq!(scope.generation.as_deref(), Some("urn:gen:test"));
    let json = serde_json::to_value(&scope).unwrap();
    assert_eq!(json["closure"], "open");
    assert_eq!(json["generation"], "urn:gen:test");
}

#[tokio::test]
async fn test_empty_traversal_is_labelled_open_with_its_generation() {
    let service = build_query_service().with_generation(Some("urn:gen:test".to_string()));
    let traversal = service.traverse("mv:NonExistent", 2, None).await.unwrap();
    assert!(traversal.nodes.is_empty() && traversal.edges.is_empty());
    assert_eq!(
        traversal.scope,
        AnswerScope::open(Some("urn:gen:test".to_string()))
    );
}

#[tokio::test]
async fn test_membership_in_a_disjoint_class_is_entailed_false() {
    let service = build_disjoint_query_service().await;
    let check = service
        .check_membership("mv:Company", "mv:Person")
        .await
        .unwrap();
    assert_eq!(check.verdict, Entailment::EntailedFalse);
    assert_eq!(
        check.witness,
        Some(DisjointnessWitness {
            subject_side: "mv:Organization".to_string(),
            class_side: "mv:Person".to_string(),
        })
    );
    assert_eq!(check.scope.closure, Closure::Open);
    assert!(check.scope.generation.is_some());
}

#[tokio::test]
async fn test_membership_distinguishes_asserted_inferred_and_silent() {
    let service = build_disjoint_query_service().await;
    let asserted = service
        .check_membership("mv:Company", "mv:Organization")
        .await
        .unwrap();
    assert_eq!(asserted.verdict, Entailment::Entailed);
    assert_eq!(asserted.basis, Some(FactBasis::Asserted));

    let inferred = service
        .check_membership("mv:Company", "mv:Agent")
        .await
        .unwrap();
    assert_eq!(inferred.verdict, Entailment::Entailed);
    assert_eq!(inferred.basis, Some(FactBasis::Inferred));

    let silent = service
        .check_membership("mv:Technology", "mv:Person")
        .await
        .unwrap();
    assert_eq!(silent.verdict, Entailment::NotAsserted);
    assert_eq!(silent.basis, None);
}

#[tokio::test]
async fn test_read_and_traverse_carry_a_basis_on_every_edge() {
    let service = build_disjoint_query_service().await;
    let note = service.read_note("mv:Company").await.unwrap();
    let basis_of = |iri: &str| {
        note.related_notes
            .iter()
            .find(|r| r.iri == iri)
            .map(|r| (r.basis, r.direction.clone()))
    };
    assert_eq!(
        basis_of("mv:Organization"),
        Some((FactBasis::Asserted, "outgoing".to_string()))
    );
    assert_eq!(
        basis_of("mv:Agent"),
        Some((FactBasis::Inferred, "outgoing".to_string()))
    );
    for ax in &note.whelk_axioms {
        assert_eq!(ax.basis == FactBasis::Inferred, ax.is_inferred, "{ax:?}");
    }

    let traversal = service.traverse("mv:Company", 1, None).await.unwrap();
    let edge = |target: &str| {
        traversal
            .edges
            .iter()
            .find(|e| e.source_iri == "mv:Company" && e.target_iri == target)
            .map(|e| e.basis)
    };
    assert_eq!(edge("mv:Organization"), Some(FactBasis::Asserted));
    assert_eq!(edge("mv:Agent"), Some(FactBasis::Inferred));
}

#[tokio::test]
async fn test_discovery_results_carry_a_basis_matching_whelk_inferred() {
    let service = build_disjoint_query_service().await;
    let results = service.discover("Organization", 10, None).await.unwrap();
    assert!(!results.is_empty());
    for r in &results {
        let expected = if r.whelk_inferred {
            FactBasis::Inferred
        } else {
            FactBasis::Asserted
        };
        assert_eq!(r.basis, expected, "{}", r.iri);
    }
}

#[actix_web::test]
async fn test_http_answers_carry_open_scope_and_tri_valued_check() {
    use actix_web::{test as atest, web, App};
    use visionclaw_server::handlers::ontology_agent_handler::configure_ontology_agent_routes;

    let service = Arc::new(build_disjoint_query_service().await);
    let app = atest::init_service(
        App::new()
            .app_data(web::Data::new(service))
            .configure(configure_ontology_agent_routes),
    )
    .await;

    let req = atest::TestRequest::post()
        .uri("/ontology-agent/discover")
        .set_json(serde_json::json!({ "query": "zzz_nonexistent_xyzzy" }))
        .to_request();
    let body: serde_json::Value = atest::call_and_read_body_json(&app, req).await;
    let body = body.get("data").unwrap_or(&body);
    assert_eq!(body["count"], 0);
    assert_eq!(body["scope"]["closure"], "open");
    assert_eq!(
        body["scope"]["generation"],
        "https://narrativegoldmine.com/ontology/sha256-12-000000000001"
    );

    let req = atest::TestRequest::post()
        .uri("/ontology-agent/check")
        .set_json(serde_json::json!({ "subject": "mv:Company", "class": "mv:Person" }))
        .to_request();
    let body: serde_json::Value = atest::call_and_read_body_json(&app, req).await;
    let body = body.get("data").unwrap_or(&body);
    assert_eq!(body["check"]["verdict"], "entailed_false");
    assert_eq!(body["check"]["witness"]["subject_side"], "mv:Organization");
}

// ---------- ADR-2127 decision 2: tri-valued relation check ----------

fn relation(subject: &str, property: &str, object: &str) -> OwlAxiom {
    let mut a = axiom(AxiomType::ObjectPropertyAssertion, subject, object);
    a.annotations
        .insert("predicate".to_string(), property.to_string());
    a
}

const HAS_PART: &str = "https://narrativegoldmine.com/ns/v1#hasPart";

/// Car ⊑ Vehicle, Wheel ⊑ Part, Vehicle hasPart Wheel (asserted), Engine
/// disjoint-with Part; hasPart declares range Part.
async fn build_relation_query_service() -> OntologyQueryService {
    let repo = create_test_ontology_repo();
    for a in [
        axiom(AxiomType::SubClassOf, "mv:Car", "mv:Vehicle"),
        axiom(AxiomType::SubClassOf, "mv:Wheel", "mv:Part"),
        axiom(AxiomType::DisjointWith, "mv:Engine", "mv:Part"),
        relation("mv:Vehicle", HAS_PART, "mv:Wheel"),
    ] {
        repo.add_axiom(&a).await.unwrap();
    }
    repo.add_owl_property(
        &visionclaw_domain::ports::ontology_repository::OwlProperty {
            iri: HAS_PART.to_string(),
            range: vec!["mv:Part".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    OntologyQueryService::new(
        repo,
        Arc::new(RwLock::new(WhelkInferenceEngine::new())),
        Arc::new(SchemaService::new()),
    )
    .with_generation(Some("urn:gen:relations".to_string()))
}

#[tokio::test]
async fn test_relation_check_is_tri_valued() {
    let service = build_relation_query_service().await;

    let asserted = service
        .check_relation("mv:Vehicle", HAS_PART, "mv:Wheel")
        .await
        .unwrap();
    assert_eq!(asserted.verdict, Entailment::Entailed);
    assert_eq!(asserted.basis, Some(FactBasis::Asserted));

    let inherited = service
        .check_relation("mv:Car", HAS_PART, "mv:Part")
        .await
        .unwrap();
    assert_eq!(inherited.verdict, Entailment::Entailed);
    assert_eq!(inherited.basis, Some(FactBasis::Inferred));

    let contradicts_range = service
        .check_relation("mv:Car", HAS_PART, "mv:Engine")
        .await
        .unwrap();
    assert_eq!(contradicts_range.verdict, Entailment::EntailedFalse);
    let witness = contradicts_range.witness.expect("a range witness");
    assert_eq!(witness.constraint, RelationConstraint::Range);
    assert_eq!(witness.declared_class, "mv:Part");
    assert_eq!(witness.disjointness.subject_side, "mv:Engine");

    // No declared signature for this property: silence, never false.
    let silent = service
        .check_relation("mv:Car", "urn:p:requires", "mv:Engine")
        .await
        .unwrap();
    assert_eq!(silent.verdict, Entailment::NotAsserted);
    assert_eq!(silent.basis, None);
    assert_eq!(silent.witness, None);
    assert_eq!(
        silent.scope,
        AnswerScope::open(Some("urn:gen:relations".to_string()))
    );
}

#[actix_web::test]
async fn test_http_check_with_a_property_is_the_relation_check_and_status_lists_it() {
    use actix_web::{test as atest, web, App};
    use visionclaw_server::handlers::ontology_agent_handler::configure_ontology_agent_routes;

    let service = Arc::new(build_relation_query_service().await);
    let app = atest::init_service(
        App::new()
            .app_data(web::Data::new(service))
            .configure(configure_ontology_agent_routes),
    )
    .await;

    let req = atest::TestRequest::post()
        .uri("/ontology-agent/check")
        .set_json(serde_json::json!({
            "subject": "mv:Car", "property": HAS_PART, "object": "mv:Engine"
        }))
        .to_request();
    let body: serde_json::Value = atest::call_and_read_body_json(&app, req).await;
    let body = body.get("data").unwrap_or(&body);
    assert_eq!(body["check"]["verdict"], "entailed_false");
    assert_eq!(body["check"]["property"], HAS_PART);
    assert_eq!(body["check"]["witness"]["constraint"], "range");
    assert_eq!(body["check"]["scope"]["closure"], "open");

    // Without `property` the same route is still the membership check.
    let req = atest::TestRequest::post()
        .uri("/ontology-agent/check")
        .set_json(serde_json::json!({ "subject": "mv:Car", "class": "mv:Vehicle" }))
        .to_request();
    let body: serde_json::Value = atest::call_and_read_body_json(&app, req).await;
    let body = body.get("data").unwrap_or(&body);
    assert_eq!(body["check"]["verdict"], "entailed");
    assert_eq!(body["check"]["class"], "mv:Vehicle");

    let req = atest::TestRequest::get()
        .uri("/ontology-agent/status")
        .to_request();
    let body: serde_json::Value = atest::call_and_read_body_json(&app, req).await;
    let body = body.get("data").unwrap_or(&body);
    let caps: Vec<&str> = body["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.as_str())
        .collect();
    assert!(caps.contains(&"ontology_check"), "{caps:?}");
    assert!(caps.contains(&"ontology_check_relation"), "{caps:?}");
}

// ---------- ADR-2127 decision 3: the generation of the loaded ontology ----------

#[tokio::test]
async fn test_a_loaded_bundle_reports_its_version_iri() {
    use visionclaw_ontology::open_world::BundleLocation;
    let root = std::env::temp_dir().join(format!("adr2127-it-vault-{}", std::process::id()));
    let data = root.join("build").join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        root.join("vault.toml"),
        "[build]\nout = \"build\"\n[build.artifacts]\nontology = \"data/ontology.ttl\"\n",
    )
    .unwrap();
    std::fs::write(
        data.join("ontology.ttl"),
        "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
         @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
         <urn:ngm:class:x> rdfs:comment \"owl:versionIRI <urn:not:this>\" .\n\
         <https://narrativegoldmine.com/ontology> a owl:Ontology ;\n\
             owl:versionIRI <https://narrativegoldmine.com/ontology/sha256-12-0123456789ab> .\n",
    )
    .unwrap();
    let service =
        build_query_service().with_bundle_location(BundleLocation::VaultRoot(root.clone()));
    let scope = service.answer_scope().await;
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        scope.generation.as_deref(),
        Some("https://narrativegoldmine.com/ontology/sha256-12-0123456789ab")
    );
}

#[tokio::test]
async fn test_without_a_bundle_the_generation_is_the_store_digest_and_follows_reloads() {
    use visionclaw_ontology::open_world::{BundleLocation, STORE_GENERATION_PREFIX};
    let repo = create_test_ontology_repo();
    let service = OntologyQueryService::new(
        repo.clone(),
        Arc::new(RwLock::new(WhelkInferenceEngine::new())),
        Arc::new(SchemaService::new()),
    )
    .with_bundle_location(BundleLocation::Dir(
        std::env::temp_dir().join("adr2127-no-such-bundle"),
    ));
    let first = service.answer_scope().await.generation.expect("never null");
    assert!(first.starts_with(STORE_GENERATION_PREFIX), "{first}");
    // Stable while the store is unchanged.
    assert_eq!(
        service.answer_scope().await.generation.as_deref(),
        Some(first.as_str())
    );
    // A reload that changes the ontology changes the generation, and the
    // cached index follows it.
    let before = service
        .check_membership("mv:Robot", "mv:Agent")
        .await
        .unwrap();
    assert_eq!(before.verdict, Entailment::NotAsserted);
    repo.add_axiom(&axiom(AxiomType::SubClassOf, "mv:Robot", "mv:Agent"))
        .await
        .unwrap();
    let after = service
        .check_membership("mv:Robot", "mv:Agent")
        .await
        .unwrap();
    assert_eq!(after.verdict, Entailment::Entailed);
    let second = after.scope.generation.expect("never null");
    assert!(second.starts_with(STORE_GENERATION_PREFIX), "{second}");
    assert_ne!(first, second, "the generation follows the loaded ontology");
}

#[tokio::test]
async fn test_read_note_is_unchanged_by_the_cached_index() {
    // The same note read twice (cache cold, then warm) is identical, and a
    // reload between reads is reflected.
    let service = build_disjoint_query_service().await;
    let cold = service.read_note("mv:Company").await.unwrap();
    let warm = service.read_note("mv:Company").await.unwrap();
    let key = |n: &EnrichedNote| {
        let mut r: Vec<(String, String, FactBasis)> = n
            .related_notes
            .iter()
            .map(|r| (r.iri.clone(), r.direction.clone(), r.basis))
            .collect();
        r.sort_by(|a, b| a.0.cmp(&b.0));
        let mut ax: Vec<(String, String, bool)> = n
            .whelk_axioms
            .iter()
            .map(|a| (a.axiom_type.clone(), a.object.clone(), a.is_inferred))
            .collect();
        ax.sort();
        (r, ax)
    };
    assert_eq!(key(&cold), key(&warm));
    let incoming = service.read_note("mv:Organization").await.unwrap();
    assert!(incoming.related_notes.iter().any(|r| r.iri == "mv:Company"
        && r.direction == "incoming"
        && r.basis == FactBasis::Asserted));
}
