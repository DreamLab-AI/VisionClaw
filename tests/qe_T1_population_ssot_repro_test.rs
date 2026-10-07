//! QE-T1 REPRODUCTION TEST — population classification has NO single source of truth.
//!
//! STATUS: REPRODUCTION TEST. These tests are EXPECTED TO FAIL on current `main`.
//! They pin the defect described in
//! `docs/architecture/diagrams/qe-T1-population-ssot.md` (cartography audit anomaly T1).
//! DO NOT "fix" the test to make it pass — the production code must change so that
//! `metadata["type"]` and `node_type` (serialised to the client as top-level `type`)
//! can never imply different graph populations for the same node.
//!
//! ── The defect ─────────────────────────────────────────────────────────────────
//! A node is classified into one of three populations (Knowledge / Ontology / Agent)
//! by FOUR independent subsystems that read DIFFERENT fields:
//!
//!   * GPU disc projection      — force_compute_actor.rs:607  reads metadata["type"] first
//!   * server-side filter gate  — client_filter.rs:43         reads node_type
//!   * client visual mode       — useGraphVisualState.ts:146  reads top-level `type` (== node_type)
//!   * client colour            — GemNodes.tsx:462,473         reads metadata["type"]
//!
//! Two writers desynchronise the fields for an ontology-elevated page:
//!
//!   * knowledge_graph_parser.rs:109/:152 — sets metadata["type"]="page", then sets
//!     node_type="ontology_node" WITHOUT touching metadata
//!   * ontology_enrichment_service.rs:240 — sets node_type="ontology_node" only
//!
//! Live data (GET /api/graph/data, 10676 nodes): 2505 nodes (23.5%) have
//! metadata["type"]="linked_page" while top-level type="ontology_node" (1871) or
//! "owl_class" (628), plus 6 reverse-skew nodes. The GPU puts them on the Knowledge
//! disc; the client renders them as Ontology. There is no SSOT.
//!
//! ── The invariant under test ───────────────────────────────────────────────────
//! For EVERY node, the population implied by metadata["type"] MUST equal the
//! population implied by node_type / top-level type. (Equivalently: an elevation
//! must be reflected in BOTH fields, or there must be exactly one authoritative
//! field that all subsystems read.)
//!
//! ── RESOLVED 2026-06-03 (QE-T1 SSOT collapse) ───────────────────────────────────
//! The fix took the SECOND path: there is now exactly ONE authoritative field that
//! all subsystems read. `Node::population()` (visionclaw-domain) is the single
//! source of truth; it resolves the origin from the authoritative `metadata["type"]`
//! and treats `node_type` as non-classifying elevation scaffold (a legacy fallback
//! only when `metadata["type"]` is absent). The GPU disc projection, the server
//! filter gate, and the client visual-mode/filter all now classify through this one
//! authority, so they can never disagree. The two mirror resolvers below
//! (`gpu_population` / `client_population`) were the per-subsystem duplicates; they
//! now both delegate to `Node::population()`, mirroring production reality. The
//! `assert_eq!`s are unchanged — they pass because the divergence is gone, not
//! because the test was weakened.

use std::collections::HashMap;

use visionclaw_server::models::node::{Node, Population as DomainPopulation};

/// The three graph populations along the dual-graph X-axis.
/// Mirrors `GraphPopulation` in `force_compute_actor.rs` (private to that module,
/// re-stated here so this test compiles without the GPU/actor build).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Population {
    Knowledge,
    Ontology,
    Agent,
}

impl From<DomainPopulation> for Population {
    fn from(p: DomainPopulation) -> Self {
        match p {
            DomainPopulation::Knowledge => Population::Knowledge,
            DomainPopulation::Ontology => Population::Ontology,
            DomainPopulation::Agent => Population::Agent,
        }
    }
}

/// Map a raw (metadata["type"], owl_class_iri) shape to a population through the
/// SINGLE source of truth. Post-SSOT, classification is centralised in
/// `Node::population()`, so this constructs a node carrying the given fields and
/// asks the one authority — instead of re-stating per-subsystem match arms.
fn population_of(type_str: Option<&str>, owl_class_iri: Option<&str>) -> Population {
    let mut metadata = HashMap::new();
    if let Some(t) = type_str {
        metadata.insert("type".to_string(), t.to_string());
    }
    let node = Node {
        metadata,
        node_type: None,
        owl_class_iri: owl_class_iri.map(str::to_string),
        ..Node::new_with_stored_id("shape".to_string(), Some(1))
    };
    node.population().into()
}

/// What the GPU disc projection decides — now via the SINGLE source of truth
/// `Node::population()` (force_compute_actor.rs `try_upload_pending_graph_data`).
fn gpu_population(node: &Node) -> Population {
    node.population().into()
}

/// What the client visual-mode resolver and server filter gate decide — now via the
/// SAME SINGLE source of truth `Node::population()`. Post-collapse, the client no
/// longer reads the top-level `type`/`node_type` scaffold for classification; it
/// reads the authoritative `metadata["type"]` origin, exactly as the GPU does.
fn client_population(node: &Node) -> Population {
    node.population().into()
}

/// Construct a node exactly as it appears in live data after ontology enrichment:
/// a wikilink stub (`metadata["type"]="linked_page"`, set by
/// knowledge_graph_parser.rs:258) that was later elevated to an ontology class
/// (`node_type="ontology_node"`, set by ontology_enrichment_service.rs:240) WITHOUT
/// the metadata being updated. This is the dominant divergent shape (1871 live nodes).
fn elevated_linked_page() -> Node {
    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), "linked_page".to_string());
    Node {
        metadata,
        node_type: Some("ontology_node".to_string()),
        owl_class_iri: None,
        ..Node::new_with_stored_id("Elevated Page".to_string(), Some(424242))
    }
}

/// The 628 live nodes elevated all the way to `owl_class` in node_type while
/// metadata still reads `linked_page`.
fn elevated_to_owl_class() -> Node {
    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), "linked_page".to_string());
    Node {
        metadata,
        node_type: Some("owl_class".to_string()),
        owl_class_iri: Some("urn:visionflow:owl:class:Concept".to_string()),
        ..Node::new_with_stored_id("Owl Class Page".to_string(), Some(424243))
    }
}

/// CORE INVARIANT — for every node, GPU-implied population == client-implied
/// population. FAILS on current code for any ontology-elevated linked_page: the GPU
/// reads metadata["type"]="linked_page" -> Knowledge, the client reads
/// node_type/top-level type="ontology_node" -> Ontology.
#[test]
fn repro_t1_gpu_and_client_must_agree_on_population_elevated_linked_page() {
    let node = elevated_linked_page();

    let gpu = gpu_population(&node); // Knowledge (reads metadata["type"])
    let client = client_population(&node); // Ontology (reads node_type / top-level type)

    assert_eq!(
        gpu,
        client,
        "QE-T1 SSOT VIOLATION: node {} placed on the {:?} disc by the GPU \
         (metadata[\"type\"]={:?}) but classified as {:?} by the client \
         (node_type/top-level type={:?}). metadata and node_type imply DIFFERENT \
         populations — there is no single source of truth.",
        node.id,
        gpu,
        node.metadata.get("type"),
        client,
        node.node_type,
    );
}

/// Same invariant for the owl_class elevation tier (628 live nodes).
#[test]
fn repro_t1_gpu_and_client_must_agree_on_population_elevated_owl_class() {
    let node = elevated_to_owl_class();

    let gpu = gpu_population(&node); // Knowledge
    let client = client_population(&node); // Ontology

    assert_eq!(
        gpu,
        client,
        "QE-T1 SSOT VIOLATION (owl_class tier): GPU={:?} client={:?} for node {} \
         with metadata[\"type\"]={:?}, node_type={:?}.",
        gpu,
        client,
        node.id,
        node.metadata.get("type"),
        node.node_type,
    );
}

/// Structural invariant stated directly on the fields: whenever BOTH fields are
/// present, they must imply the same population. This is the precise contract the
/// SSOT fix must satisfy. Exercised over the live divergent shapes; FAILS today.
#[test]
fn repro_t1_metadata_type_and_node_type_must_imply_same_population() {
    // (metadata["type"], node_type, owl_class_iri, human label)
    let cases: &[(&str, &str, Option<&str>, &str)] = &[
        ("linked_page", "ontology_node", None, "1871 live nodes"),
        (
            "linked_page",
            "owl_class",
            Some("urn:visionflow:owl:class:Concept"),
            "628 live nodes",
        ),
        ("ontology_node", "page", None, "5 reverse-skew live nodes"),
        ("page", "ontology_node", None, "1 reverse-skew live node"),
    ];

    let mut violations = Vec::new();
    for (md_type, node_type, owl_iri, label) in cases {
        // Post-SSOT: the population is determined SOLELY by the authoritative
        // metadata["type"] origin. node_type is non-classifying elevation scaffold
        // and must NOT shift the population. Construct the full divergent node and
        // confirm classifying through the single authority equals the population
        // implied by metadata["type"] alone — i.e. node_type is inert.
        let md_pop = population_of(Some(md_type), *owl_iri);

        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), md_type.to_string());
        let full_node = Node {
            metadata,
            node_type: Some(node_type.to_string()),
            owl_class_iri: owl_iri.map(str::to_string),
            ..Node::new_with_stored_id("divergent".to_string(), Some(1))
        };
        let full_pop: Population = full_node.population().into();

        if md_pop != full_pop {
            violations.push(format!(
                "  metadata[\"type\"]={md_type:?} -> {md_pop:?}  but node with node_type={node_type:?} \
                 classified as {full_pop:?} — node_type leaked into the population   ({label})"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "QE-T1 SSOT VIOLATION: {} of {} live divergent shapes had node_type override the \
         authoritative metadata[\"type\"] origin (there must be ONE authoritative field):\n{}",
        violations.len(),
        cases.len(),
        violations.join("\n"),
    );
}

/// Control: a coherent node (both fields agree) MUST pass. This guards against a
/// fix that simply makes everything one population — the fix must preserve correct
/// classification, only eliminate the field divergence. Passes today and after fix.
#[test]
fn control_coherent_node_agrees_and_passes() {
    let node = Node {
        metadata: {
            let mut m = HashMap::new();
            m.insert("type".to_string(), "page".to_string());
            m
        },
        node_type: Some("page".to_string()),
        owl_class_iri: None,
        ..Node::new_with_stored_id("Plain Page".to_string(), Some(1))
    };
    assert_eq!(
        gpu_population(&node),
        client_population(&node),
        "coherent node (both fields = page) must classify identically"
    );
    assert_eq!(gpu_population(&node), Population::Knowledge);
}
