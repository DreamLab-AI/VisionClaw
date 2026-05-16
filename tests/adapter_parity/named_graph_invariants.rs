// tests/adapter_parity/named_graph_invariants.rs
//! Named-graph segregation invariants.
//!
//! Per ADR-11 §D2 the Oxigraph dataset has three logical named graphs:
//!
//!   - `<urn:visionflow:graph:knowledge>` — KGNode + KG edges
//!   - `<urn:visionflow:graph:ontology>`  — OntologyClass + OwlProperty + OwlAxiom
//!   - `<urn:visionflow:graph:agent>`     — agent telemetry
//!
//! plus the derived inferred sub-graph `<urn:visionflow:graph:ontology:inferred>`
//! materialised by `store_inference_results`.
//!
//! The structural invariant: data written into one named graph MUST NOT
//! appear in queries against another. The Neo4j adapter encoded the same
//! discipline via node labels (`:KGNode` vs `:OntologyClass`) — the
//! migration 0045 bug-class (silent label drift creating orphan stubs) is
//! exactly the failure mode these tests catch.
//!
//! Five invariants:
//!
//! I1. Adding an OntologyClass via `add_owl_class` MUST NOT make that class
//!     visible in `get_graph()` as a KGNode.
//! I2. Adding a knowledge-tier Node via `add_nodes` MUST NOT make it
//!     visible in `list_owl_classes()` as an OntologyClass.
//! I3. Adding an agent-tier item (via `get_bots_graph` proxy or future
//!     `add_agent_*`) MUST NOT bleed into knowledge tier.
//! I4. `store_inference_results` writes that are subsequently CLEARed via
//!     a fresh inference must NOT leave residue in the asserted ontology
//!     graph.
//! I5. Removing a class from the ontology graph MUST NOT affect a KGNode
//!     with the same metadata IRI in the knowledge graph (decoupling
//!     check — this is the canonical fix for migration 0045's "orphan
//!     stub" class of bug).

use webxr::ports::graph_repository::GraphRepository;
use webxr::ports::ontology_repository::{AxiomType, InferenceResults, OntologyRepository};
use chrono::Utc;

use super::{make_node, make_owl_axiom, make_owl_class};

// ---------------------------------------------------------------------------
// I1. OntologyClass writes MUST NOT pollute the knowledge graph.
// ---------------------------------------------------------------------------

pub async fn invariant_ontology_class_not_in_knowledge_graph<O, G>(ont: O, graph: G)
where
    O: OntologyRepository,
    G: GraphRepository,
{
    let class = make_owl_class("inv1-ontology-only");
    ont.add_owl_class(&class)
        .await
        .expect("add_owl_class must succeed");

    let kg = graph.get_graph().await.expect("get_graph must succeed");
    assert!(
        !kg.nodes.iter().any(|n| n.metadata_id == class.iri
            || n.label == class.label.clone().unwrap_or_default()),
        "OntologyClass IRI={} MUST NOT appear in the knowledge graph. \
         This is the ADR-11 §D2 named-graph segregation invariant — \
         if it fails the adapter is writing across named graphs.",
        class.iri
    );
}

// ---------------------------------------------------------------------------
// I2. Knowledge Node writes MUST NOT pollute the ontology graph.
// ---------------------------------------------------------------------------

pub async fn invariant_knowledge_node_not_in_ontology_list<O, G>(ont: O, graph: G)
where
    O: OntologyRepository,
    G: GraphRepository,
{
    let node = make_node("inv2-knowledge-only", "Knowledge Only Node");
    let ids = graph
        .add_nodes(vec![node.clone()])
        .await
        .expect("add_nodes must succeed");
    assert_eq!(ids.len(), 1);

    let classes = ont
        .list_owl_classes()
        .await
        .expect("list_owl_classes must succeed");
    assert!(
        !classes.iter().any(|c| c.iri == node.metadata_id
            || c.label.as_deref() == Some(node.label.as_str())),
        "KGNode metadata_id={} MUST NOT appear as an OntologyClass. \
         This is the migration 0045 bug-class — adapter wrote a label \
         in one graph but resolved it in another.",
        node.metadata_id
    );
}

// ---------------------------------------------------------------------------
// I3. Knowledge graph MUST NOT bleed into agent graph and vice-versa.
// ---------------------------------------------------------------------------

pub async fn invariant_knowledge_vs_agent_graph_isolated<G>(graph: G)
where
    G: GraphRepository,
{
    let kg_node = make_node("inv3-kg", "KG node");
    let ids = graph
        .add_nodes(vec![kg_node])
        .await
        .expect("add_nodes for kg");
    assert_eq!(ids.len(), 1);
    let kg_id = ids[0];

    let kg = graph.get_graph().await.expect("get_graph (kg)");
    let bots = graph
        .get_bots_graph()
        .await
        .expect("get_bots_graph (agent)");

    assert!(
        kg.nodes.iter().any(|n| n.id == kg_id),
        "kg node must be in knowledge graph"
    );
    assert!(
        !bots.nodes.iter().any(|n| n.id == kg_id),
        "kg node id={} MUST NOT appear in the agent graph (named-graph segregation)",
        kg_id
    );
}

// ---------------------------------------------------------------------------
// I4. store_inference_results writes inferred axioms into the inferred
//     sub-graph and CLEAR-on-rewrite leaves no residue in the asserted graph.
// ---------------------------------------------------------------------------

pub async fn invariant_inferred_does_not_leak_into_asserted<O>(ont: O)
where
    O: OntologyRepository,
{
    let a = make_owl_class("inv4-a");
    let b = make_owl_class("inv4-b");
    let c = make_owl_class("inv4-c");
    ont.add_owl_class(&a).await.unwrap();
    ont.add_owl_class(&b).await.unwrap();
    ont.add_owl_class(&c).await.unwrap();

    // Asserted axioms: a sub b, b sub c.
    let asserted_axioms = vec![
        make_owl_axiom(&a.iri, AxiomType::SubClassOf, &b.iri),
        make_owl_axiom(&b.iri, AxiomType::SubClassOf, &c.iri),
    ];
    for ax in &asserted_axioms {
        ont.add_axiom(ax).await.expect("add_axiom (asserted)");
    }

    // Inferred axiom: a sub c (transitive closure). This is what
    // whelk-rs would emit.
    let inferred = vec![make_owl_axiom(&a.iri, AxiomType::SubClassOf, &c.iri)];

    let results_1 = InferenceResults {
        timestamp: Utc::now(),
        inferred_axioms: inferred.clone(),
        inference_time_ms: 10,
        reasoner_version: "parity-i4-v1".to_string(),
    };
    ont.store_inference_results(&results_1)
        .await
        .expect("store_inference_results v1");

    // After a re-inference with NO results, the inferred sub-graph must be
    // cleared. The asserted axioms MUST survive.
    let results_2 = InferenceResults {
        timestamp: Utc::now(),
        inferred_axioms: vec![],
        inference_time_ms: 5,
        reasoner_version: "parity-i4-v2".to_string(),
    };
    ont.store_inference_results(&results_2)
        .await
        .expect("store_inference_results v2 (empty)");

    let after = ont.get_axioms().await.expect("get_axioms");
    for original in &asserted_axioms {
        assert!(
            after.iter().any(|a| a.subject == original.subject
                && a.object == original.object
                && a.axiom_type == original.axiom_type),
            "asserted axiom ({}, {:?}, {}) MUST survive an inference rewrite. \
             ADR-11 §D2 says CLEAR GRAPH targets ONLY the :inferred sub-graph.",
            original.subject,
            original.axiom_type,
            original.object
        );
    }
}

// ---------------------------------------------------------------------------
// I5. Removing an OntologyClass MUST NOT cascade-delete a same-named KGNode.
// ---------------------------------------------------------------------------
//
// This is the structural fix for the migration 0045 orphan-stub class:
// the two tiers are independent named graphs, and removing one MUST NOT
// silently null the other.

pub async fn invariant_class_remove_does_not_cascade_to_kg<O, G>(ont: O, graph: G)
where
    O: OntologyRepository,
    G: GraphRepository,
{
    // Same logical identifier in both tiers (this is the BRIDGE_TO case).
    let shared_iri = "https://visionflow.dreamlab/ns/onto/inv5-shared";

    let mut class = make_owl_class("inv5-shared");
    class.iri = shared_iri.to_string();
    ont.add_owl_class(&class).await.expect("add_owl_class");

    let kg_node = make_node(shared_iri, "Shared identifier in both tiers");
    let ids = graph.add_nodes(vec![kg_node]).await.expect("add_nodes");
    let kg_id = ids[0];

    // Sanity: both tiers contain a representation.
    assert!(ont.get_owl_class(shared_iri).await.unwrap().is_some());
    let kg_before = graph.get_graph().await.unwrap();
    assert!(kg_before.nodes.iter().any(|n| n.id == kg_id));

    // Remove the ontology side.
    ont.remove_owl_class(shared_iri)
        .await
        .expect("remove_owl_class must succeed");

    // The ontology side is gone.
    assert!(
        ont.get_owl_class(shared_iri).await.unwrap().is_none(),
        "ontology side must be removed"
    );

    // The knowledge side MUST survive — different named graph.
    let kg_after = graph.get_graph().await.unwrap();
    assert!(
        kg_after.nodes.iter().any(|n| n.id == kg_id),
        "knowledge node id={} MUST survive remove_owl_class on the same IRI. \
         A cascade here means the adapter is mixing named graphs.",
        kg_id
    );
}
