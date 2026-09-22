// src/adapters/oxigraph_ontology_repository.rs
//! Oxigraph Ontology Repository Adapter (ADR-11 Phase 1 implementation).
//!
//! Implements [`OntologyRepository`] over an embedded Oxigraph quad-store
//! per ADR-11 §D1 + §D2. Asserted ontology triples live in the named graph
//! `<urn:ngm:graph:ontology:assert>`; whelk-derived inferred axioms
//! in `<urn:ngm:graph:ontology:inferred>` (ADR-11 §D9).
//!
//! ## Named graph layout (ADR-11 §D2)
//!
//! | Named graph IRI                                 | Contents                              |
//! |-------------------------------------------------|---------------------------------------|
//! | `urn:ngm:graph:ontology:assert`          | asserted OntologyClass/Property/Axiom |
//! | `urn:ngm:graph:ontology:inferred`        | whelk-derived inferred axioms          |
//! | `urn:ngm:graph:knowledge`                | KGNode + KGEdge triples               |
//! | (default graph)                                 | cross-graph bridges + schema          |
//!
//! ## IRI minting (ADR-11 §D3)
//!
//! All IRIs use the `vc:` prefix expanding to
//! `https://narrativegoldmine.com/ns/v1#`. OntologyClass IRIs follow the
//! pattern minted by `visionclaw_domain::uri::ngm::class_iri` (ADR-2095 —
//! never by hand); Properties
//! `urn:ngm:property:<slug>`; Axioms
//! `urn:ngm:axiom:<sha256-12>` content-addressed.

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use oxigraph::model::Term;
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;

use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;
use visionclaw_domain::ports::ontology_repository::{
    AxiomType, InferenceResults, OntologyMetrics, OntologyRepository, OntologyRepositoryError,
    OwlAxiom, OwlClass, OwlProperty, PathfindingCacheEntry, PropertyType, Result as RepoResult,
    ValidationReport,
};
use visionclaw_domain::uri::ngm;

/// Canonical IRIs for the four named graphs ADR-11 §D2 enumerates.
/// Held as `&'static str` so SPARQL string construction is allocation-free.
pub const GRAPH_ONTOLOGY: &str = "urn:ngm:graph:ontology:assert";
pub const GRAPH_ONTOLOGY_INFERRED: &str = "urn:ngm:graph:ontology:inferred";
pub const GRAPH_KNOWLEDGE: &str = "urn:ngm:graph:knowledge";
pub const GRAPH_AGENT: &str = "urn:ngm:graph:agent";

/// PRD-022 trust-layer named graphs.
/// `:shapes` holds W3C SHACL shape triples loaded from .ttl files at startup.
/// `:provenance` holds append-only PROV-O activity triples (reified provenance).
pub const GRAPH_SHAPES: &str = "urn:ngm:graph:shapes";
pub const GRAPH_PROVENANCE: &str = "urn:ngm:graph:provenance";

/// WS-9 derived named graphs. These are the ONLY graphs writable through the
/// fenced `/api/ontology/derived` path: `:summary` holds approval-driven
/// summary triples (broker write-back consequences), `:observed` holds
/// externally-observed facts. `:assert`/`:inferred` are NEVER writable here.
pub const GRAPH_ONTOLOGY_SUMMARY: &str = "urn:ngm:graph:ontology:summary";
pub const GRAPH_ONTOLOGY_OBSERVED: &str = "urn:ngm:graph:ontology:observed";

/// Server-side fence (WS-9): graphs the derived write path must REJECT.
/// `GRAPH_ONTOLOGY` is `:assert`. Defence-in-depth — the handler also rejects
/// these, but the repo method re-checks so any future caller (e.g. the approve
/// path) inherits the fence and cannot bypass it.
const DERIVED_FENCE: [&str; 2] = [GRAPH_ONTOLOGY, GRAPH_ONTOLOGY_INFERRED];

/// Cache named graphs (own sub-domain so `CLEAR GRAPH` invalidates atomically).
pub const GRAPH_CACHE_SSSP: &str = "urn:ngm:graph:cache:sssp";
pub const GRAPH_CACHE_APSP: &str = "urn:ngm:graph:cache:apsp";

/// `vc:` prefix expansion per ADR-11 §D3.
pub const VC_NS: &str = "https://narrativegoldmine.com/ns/v1#";

/// PRD-020 WS-0: server-side hard caps so a naive SPARQL SELECT
/// (`SELECT ?s ?p ?o WHERE { ?s ?p ?o }`) cannot serialise the entire store.
/// LIMIT is injected/clamped BEFORE execution so Oxigraph never materialises
/// more than the cap; the row + byte fences below apply during serialisation
/// as parse-independent defence in depth.
const MAX_SPARQL_ROWS: usize = 10_000;
const DEFAULT_SPARQL_LIMIT: usize = 10_000;
const MAX_SPARQL_RESULT_BYTES: usize = 8 * 1024 * 1024;

/// WS-0: inject/clamp a top-level LIMIT on a pre-validated read query.
///
/// A SELECT without a trailing LIMIT gets `LIMIT DEFAULT_SPARQL_LIMIT`; a SELECT
/// whose trailing LIMIT exceeds the cap is rewritten down to `MAX_SPARQL_ROWS`.
/// ASK/CONSTRUCT/DESCRIBE are returned unchanged (ASK is a single bool;
/// CONSTRUCT/DESCRIBE are rejected by the result-kind arm in `sparql_select_json`).
///
/// This is a best-effort string rewrite, not a SPARQL parse — a LIMIT inside a
/// sub-SELECT or appearing in a literal could be misdetected. The row + byte
/// serialisation fences in `sparql_select_json` are the hard, parse-independent
/// backstop. `validate_read_only_sparql` has already run on the input.
fn clamp_select_limit(query: &str) -> String {
    let upper = query.to_uppercase();
    if !upper.contains("SELECT") {
        return query.to_string();
    }
    // Scan from the end of the trimmed query for a trailing top-level LIMIT N.
    let trimmed = query.trim_end();
    let tail_upper = trimmed.to_uppercase();
    if let Some(pos) = tail_upper.rfind("LIMIT") {
        let after = trimmed[pos + 5..].trim_start();
        // Only treat as a real LIMIT clause if what follows starts with digits.
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let n: usize = digits.parse().unwrap_or(usize::MAX);
            if n > MAX_SPARQL_ROWS {
                // Rewrite the numeric value down to the cap, preserving any
                // trailing clause (e.g. OFFSET) after the digits.
                let head = &trimmed[..pos + 5];
                let rest = &after[digits.len()..];
                return format!("{} {}{}", head, MAX_SPARQL_ROWS, rest);
            }
            return query.to_string();
        }
    }
    format!("{}\nLIMIT {}", trimmed, DEFAULT_SPARQL_LIMIT)
}

/// SPARQL prologue applied to every UPDATE/QUERY string this adapter emits.
const PROLOGUE: &str = concat!(
    "PREFIX vc: <https://narrativegoldmine.com/ns/v1#>\n",
    "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n",
    "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n",
    "PREFIX owl: <http://www.w3.org/2002/07/owl#>\n",
    "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>\n",
);

// ----------------------------------------------------------------------
// IRI minting helpers (ADR-11 §D3 + DDD-08).
// ----------------------------------------------------------------------

/// NFKC-normalise, lowercase, non-alnum → dash, collapse repeats, trim.
fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = true; // leading dash trimmed
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    out
}

/// Mint the canonical class IRI from an OwlClass. Prefers an explicit
/// non-empty `iri` field over a label-derived slug, so round-tripping
/// preserves existing IRIs from the migration tool.
fn class_iri(c: &OwlClass) -> String {
    if !c.iri.is_empty() {
        c.iri.clone()
    } else {
        let basis = c
            .label
            .as_deref()
            .or(c.preferred_term.as_deref())
            .or(c.term_id.as_deref())
            .unwrap_or("unnamed");
        ngm::class_iri(&slug(basis))
    }
}

/// Mint the canonical property IRI.
fn property_iri(p: &OwlProperty) -> String {
    if !p.iri.is_empty() {
        p.iri.clone()
    } else {
        let basis = p.label.as_deref().unwrap_or("unnamed");
        format!("urn:ngm:property:{}", slug(basis))
    }
}

/// Content-address an axiom: sha256(subject || predicate || object) → 12 hex chars.
fn axiom_iri(a: &OwlAxiom) -> String {
    let mut hasher = Sha256::new();
    hasher.update(a.subject.as_bytes());
    hasher.update(format!("{:?}", a.axiom_type).as_bytes());
    hasher.update(a.object.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest
        .iter()
        .take(6)
        .map(|b| format!("{:02x}", b))
        .collect();
    format!("urn:ngm:axiom:{}", hex)
}

/// Derive a stable `u64` id for an axiom from its content hash. Used as
/// the trait's `OwlAxiom::id` field and the public `remove_axiom` key.
fn axiom_id(a: &OwlAxiom) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(a.subject.as_bytes());
    hasher.update(format!("{:?}", a.axiom_type).as_bytes());
    hasher.update(a.object.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes)
}

/// Map an `AxiomType` to its string literal for the `vc:axiomType` triple.
fn axiom_type_str(t: &AxiomType) -> &'static str {
    match t {
        AxiomType::SubClassOf => "SubClassOf",
        AxiomType::EquivalentClass => "EquivalentClass",
        AxiomType::DisjointWith => "DisjointWith",
        AxiomType::ObjectPropertyAssertion => "ObjectPropertyAssertion",
        AxiomType::DataPropertyAssertion => "DataPropertyAssertion",
        AxiomType::SubPropertyOf => "SubPropertyOf",
        AxiomType::TransitiveProperty => "TransitiveProperty",
        AxiomType::SymmetricProperty => "SymmetricProperty",
        AxiomType::InverseProperties => "InverseProperties",
        AxiomType::SomeValuesFrom => "SomeValuesFrom",
    }
}

fn parse_axiom_type(s: &str) -> AxiomType {
    match s {
        "SubClassOf" => AxiomType::SubClassOf,
        "EquivalentClass" | "EquivalentClasses" => AxiomType::EquivalentClass,
        "DisjointWith" | "DisjointClasses" => AxiomType::DisjointWith,
        "ObjectPropertyAssertion" | "SubObjectProperty" => AxiomType::ObjectPropertyAssertion,
        "DataPropertyAssertion" | "Domain" | "Range" => AxiomType::DataPropertyAssertion,
        "SubPropertyOf" | "SubObjectPropertyOf" => AxiomType::SubPropertyOf,
        "TransitiveProperty" | "TransitiveObjectProperty" => AxiomType::TransitiveProperty,
        "SymmetricProperty" | "SymmetricObjectProperty" => AxiomType::SymmetricProperty,
        "InverseProperties" | "InverseObjectProperties" => AxiomType::InverseProperties,
        "SomeValuesFrom" | "ObjectSomeValuesFrom" => AxiomType::SomeValuesFrom,
        _ => AxiomType::SubClassOf,
    }
}

fn property_type_str(t: &PropertyType) -> &'static str {
    match t {
        PropertyType::ObjectProperty => "owl:ObjectProperty",
        PropertyType::DataProperty => "owl:DatatypeProperty",
        PropertyType::AnnotationProperty => "owl:AnnotationProperty",
    }
}

fn parse_property_type(iri: &str) -> PropertyType {
    if iri.ends_with("DatatypeProperty") || iri.ends_with("DataProperty") {
        PropertyType::DataProperty
    } else if iri.ends_with("AnnotationProperty") {
        PropertyType::AnnotationProperty
    } else {
        PropertyType::ObjectProperty
    }
}

/// True when `s` looks like an IRI we will interpolate as `<s>` (vs a literal).
/// Same detection `store_inference_results` uses for axiom subjects/objects.
fn looks_like_iri(s: &str) -> bool {
    s.starts_with("urn:") || s.starts_with("http://") || s.starts_with("https://")
}

/// WS-9 SPARQL-injection guard for IRIs interpolated as `<iri>` in derived
/// writes. The s/p/o come from a request body, so an IRI containing `>`,
/// whitespace, or control characters could break out of the `<...>` token and
/// inject arbitrary SPARQL. Accept only `urn:`/`http(s):` IRIs with no `<`,
/// `>`, `"`, backslash, brace, or whitespace. Mirrors how
/// `store_inference_results` only interpolates pre-validated axiom IRIs.
fn iri_is_safe(s: &str) -> bool {
    if !looks_like_iri(s) {
        return false;
    }
    !s.chars().any(|c| {
        c.is_whitespace()
            || matches!(c, '<' | '>' | '"' | '\\' | '{' | '}' | '`' | '|' | '^')
            || c.is_control()
    })
}

/// Escape a string for embedding in a SPARQL string literal.
fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Lift the lexical form of a `Term` into a plain `String`. For named
/// nodes this is the IRI; for literals the value (without datatype tag).
fn term_lexical(t: &Term) -> String {
    match t {
        Term::NamedNode(n) => n.as_str().to_string(),
        Term::BlankNode(b) => b.as_str().to_string(),
        Term::Literal(l) => l.value().to_string(),
        _ => String::new(),
    }
}

fn db_err<E: std::fmt::Display>(e: E) -> OntologyRepositoryError {
    OntologyRepositoryError::DatabaseError(e.to_string())
}

// ----------------------------------------------------------------------
// PRD-022 WS-1 — SHACL shape loading (real W3C shapes into GRAPH_SHAPES).
//
// The five canonical `.shacl.ttl` NodeShape files live in the sibling
// `visionclaw-ontology` crate. They are embedded here at compile time and
// loaded into `GRAPH_SHAPES` on startup so the shape catalogue is queryable
// (`trust-status.shapesLoaded`) and available to the SHACL engine.
//
// NOTE ON ENFORCEMENT (audit truth): the ingest gate is now shape-driven and
// ENFORCING by default (config knob `ontology_agent.shacl_mode`, default
// "enforcing"). In enforcing mode shape-invalid JSON-LD is rejected with
// `JsonLdIngestError::ShaclViolations` and writes zero quads; "advisory" mode
// downgrades violations to warnings. These embedded files are the canonical
// W3C shape definitions the gate validates against.
// ----------------------------------------------------------------------

/// The five embedded W3C SHACL shape files: `(logical-name, turtle-body)`.
const SHACL_SHAPE_FILES: &[(&str, &str)] = &[
    (
        "knowledge-node",
        include_str!("../../visionclaw-ontology/shapes/knowledge-node.shacl.ttl"),
    ),
    (
        "ontology-class",
        include_str!("../../visionclaw-ontology/shapes/ontology-class.shacl.ttl"),
    ),
    (
        "agent-node",
        include_str!("../../visionclaw-ontology/shapes/agent-node.shacl.ttl"),
    ),
    (
        "bridge-record",
        include_str!("../../visionclaw-ontology/shapes/bridge-record.shacl.ttl"),
    ),
    (
        "inferred-axiom",
        include_str!("../../visionclaw-ontology/shapes/inferred-axiom.shacl.ttl"),
    ),
];

/// Catalogue sentinel IRI (mirrors migration 0002); re-inserted on reload.
const SHAPES_CATALOGUE_IRI: &str = "urn:ngm:shape:catalogue";
/// Marker predicate carrying the embedded-shapes content hash so a reload runs
/// exactly once per distinct shape corpus (idempotent across restarts).
const P_SHAPES_CONTENT_HASH: &str = "https://narrativegoldmine.com/ns/v1#shapesContentHash";
const RDF_TYPE_IRI: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SH_SCHEMA_IRI: &str = "http://www.w3.org/ns/shacl#Schema";

/// Stable 16-hex content hash over the embedded shape corpus (name + body).
fn shacl_shapes_hash() -> String {
    let mut hasher = Sha256::new();
    for (name, body) in SHACL_SHAPE_FILES {
        hasher.update(name.as_bytes());
        hasher.update([0u8]);
        hasher.update(body.as_bytes());
        hasher.update([0u8]);
    }
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Read the content-hash marker currently stored in `GRAPH_SHAPES`, if any.
fn stored_shapes_hash(store: &Store) -> Option<String> {
    let q = format!(
        "SELECT ?h FROM <{GRAPH_SHAPES}> WHERE {{ <{SHAPES_CATALOGUE_IRI}> <{P_SHAPES_CONTENT_HASH}> ?h }} LIMIT 1"
    );
    match store.query(&q) {
        Ok(QueryResults::Solutions(mut sols)) => {
            sols.next()
                .and_then(|r| r.ok())
                .and_then(|s| match s.get("h") {
                    Some(Term::Literal(l)) => Some(l.value().to_string()),
                    _ => None,
                })
        }
        _ => None,
    }
}

/// Load the embedded W3C SHACL shapes into `GRAPH_SHAPES`.
///
/// Idempotent and reload-safe. Keyed by a content hash: if the graph already
/// carries the current corpus hash the load is skipped (no blank-node
/// duplication on restart); a deploy with edited shapes clears the graph and
/// reloads exactly once. Returns the number of `sh:NodeShape` subjects present
/// after loading.
pub fn load_shacl_shapes(store: &Store) -> RepoResult<usize> {
    use oxigraph::io::{RdfFormat, RdfParser};
    use oxigraph::model::{GraphName, GraphNameRef, Literal, NamedNode, NamedNodeRef, QuadRef};

    let want_hash = shacl_shapes_hash();
    if stored_shapes_hash(store).as_deref() == Some(want_hash.as_str()) {
        // Already current — count and return without touching the graph.
        return crate::provenance_emitter::count_shapes_loaded(store).map_err(db_err);
    }

    // Stale or absent: clear the shapes graph and reload from scratch so blank
    // property-shape nodes are never duplicated across deploys.
    store
        .update(&format!("CLEAR SILENT GRAPH <{GRAPH_SHAPES}>"))
        .map_err(db_err)?;

    let shapes_graph = NamedNode::new_unchecked(GRAPH_SHAPES);
    for (name, body) in SHACL_SHAPE_FILES {
        let parser = RdfParser::from_format(RdfFormat::Turtle)
            .with_default_graph(GraphName::NamedNode(shapes_graph.clone()));
        store
            .load_from_reader(parser, body.as_bytes())
            .map_err(|e| db_err(format!("loading SHACL shape '{name}': {e}")))?;
    }

    // Re-establish the catalogue sentinel + the content-hash marker.
    let gref = GraphNameRef::NamedNode(shapes_graph.as_ref());
    let marker = NamedNodeRef::new_unchecked(SHAPES_CATALOGUE_IRI);
    store
        .insert(QuadRef::new(
            marker,
            NamedNodeRef::new_unchecked(RDF_TYPE_IRI),
            NamedNodeRef::new_unchecked(SH_SCHEMA_IRI),
            gref,
        ))
        .map_err(db_err)?;
    let hash_lit = Literal::new_simple_literal(&want_hash);
    store
        .insert(QuadRef::new(
            marker,
            NamedNodeRef::new_unchecked(P_SHAPES_CONTENT_HASH),
            hash_lit.as_ref(),
            gref,
        ))
        .map_err(db_err)?;

    crate::provenance_emitter::count_shapes_loaded(store).map_err(db_err)
}

/// System agent DID attributed to machine-run Whelk EL++ inference activities
/// in the PROV-O provenance ledger (PRD-022 WS-2).
const REASONER_AGENT_DID: &str = "urn:visionclaw:agent:whelk-reasoner";

/// Encode a `Term` as a SPARQL 1.1 Query Results JSON binding object
/// (`{ "type": "uri"|"literal"|"bnode", "value": ..., "datatype"?, "xml:lang"? }`).
fn term_to_json(t: &Term) -> serde_json::Value {
    const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
    const RDF_LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";
    match t {
        Term::NamedNode(n) => serde_json::json!({ "type": "uri", "value": n.as_str() }),
        Term::BlankNode(b) => serde_json::json!({ "type": "bnode", "value": b.as_str() }),
        Term::Literal(l) => {
            let mut obj = serde_json::Map::new();
            obj.insert("type".into(), serde_json::Value::from("literal"));
            obj.insert("value".into(), serde_json::Value::from(l.value()));
            if let Some(lang) = l.language() {
                obj.insert("xml:lang".into(), serde_json::Value::from(lang));
            } else {
                let dt = l.datatype().as_str();
                // Omit the implicit xsd:string / rdf:langString datatypes per
                // the SPARQL JSON results convention.
                if dt != XSD_STRING && dt != RDF_LANG_STRING {
                    obj.insert("datatype".into(), serde_json::Value::from(dt));
                }
            }
            serde_json::Value::Object(obj)
        }
        _ => serde_json::json!({ "type": "literal", "value": "" }),
    }
}

// ----------------------------------------------------------------------
// Predicate catalogue (vc: namespace, ADR-11 §D3).
// Kept here so the property-bag fold has one source of truth.
// ----------------------------------------------------------------------

const P_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const P_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";
const P_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const P_DOMAIN: &str = "http://www.w3.org/2000/01/rdf-schema#domain";
const P_RANGE: &str = "http://www.w3.org/2000/01/rdf-schema#range";
const P_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

const T_OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const T_OWL_OBJECT_PROP: &str = "http://www.w3.org/2002/07/owl#ObjectProperty";
const T_OWL_DATA_PROP: &str = "http://www.w3.org/2002/07/owl#DatatypeProperty";
const T_OWL_ANNOT_PROP: &str = "http://www.w3.org/2002/07/owl#AnnotationProperty";
const T_VC_ONTOLOGY_CLASS: &str = "https://narrativegoldmine.com/ns/v1#OntologyClass";
const T_VC_AXIOM: &str = "https://narrativegoldmine.com/ns/v1#Axiom";

// VisionClaw-specific predicates.
const P_TERM_ID: &str = "https://narrativegoldmine.com/ns/v1#termId";
const P_PREFERRED_TERM: &str = "https://narrativegoldmine.com/ns/v1#preferredTerm";
const P_DESCRIPTION: &str = "https://narrativegoldmine.com/ns/v1#description";
const P_SOURCE_DOMAIN: &str = "https://narrativegoldmine.com/ns/v1#sourceDomain";
const P_VERSION: &str = "https://narrativegoldmine.com/ns/v1#version";
const P_CLASS_TYPE: &str = "https://narrativegoldmine.com/ns/v1#classType";
const P_STATUS: &str = "https://narrativegoldmine.com/ns/v1#status";
const P_MATURITY: &str = "https://narrativegoldmine.com/ns/v1#maturity";
const P_QUALITY_SCORE: &str = "https://narrativegoldmine.com/ns/v1#qualityScore";
const P_AUTHORITY_SCORE: &str = "https://narrativegoldmine.com/ns/v1#authorityScore";
const P_PUBLIC_ACCESS: &str = "https://narrativegoldmine.com/ns/v1#publicAccess";
const P_CONTENT_STATUS: &str = "https://narrativegoldmine.com/ns/v1#contentStatus";
const P_OWL_PHYSICALITY: &str = "https://narrativegoldmine.com/ns/v1#owlPhysicality";
const P_OWL_ROLE: &str = "https://narrativegoldmine.com/ns/v1#owlRole";
const P_BELONGS_TO_DOMAIN: &str = "https://narrativegoldmine.com/ns/v1#belongsToDomain";
const P_BRIDGES_TO_DOMAIN: &str = "https://narrativegoldmine.com/ns/v1#bridgesToDomain";
const P_SOURCE_FILE: &str = "https://narrativegoldmine.com/ns/v1#sourceFile";
const P_FILE_SHA1: &str = "https://narrativegoldmine.com/ns/v1#fileSha1";
const P_MARKDOWN_CONTENT: &str = "https://narrativegoldmine.com/ns/v1#markdownContent";
const P_LAST_SYNCED: &str = "https://narrativegoldmine.com/ns/v1#lastSynced";
const P_ADDITIONAL_META: &str = "https://narrativegoldmine.com/ns/v1#additionalMetadata";

const P_HAS_PART: &str = "https://narrativegoldmine.com/ns/v1#hasPart";
const P_IS_PART_OF: &str = "https://narrativegoldmine.com/ns/v1#isPartOf";
const P_REQUIRES: &str = "https://narrativegoldmine.com/ns/v1#requires";
const P_DEPENDS_ON: &str = "https://narrativegoldmine.com/ns/v1#dependsOn";
const P_ENABLES: &str = "https://narrativegoldmine.com/ns/v1#enables";
const P_RELATES_TO: &str = "https://narrativegoldmine.com/ns/v1#relatesTo";
const P_BRIDGES_TO: &str = "https://narrativegoldmine.com/ns/v1#bridgesTo";
const P_BRIDGES_FROM: &str = "https://narrativegoldmine.com/ns/v1#bridgesFrom";
const P_OTHER_REL_PREFIX: &str = "https://narrativegoldmine.com/ns/v1#otherRel/";
const P_PROPERTY_PREFIX: &str = "https://narrativegoldmine.com/ns/v1#property/";

const P_AXIOM_TYPE: &str = "https://narrativegoldmine.com/ns/v1#axiomType";
const P_AXIOM_SUBJECT: &str = "https://narrativegoldmine.com/ns/v1#subject";
const P_AXIOM_OBJECT: &str = "https://narrativegoldmine.com/ns/v1#object";
const P_AXIOM_ANNOTATION: &str = "https://narrativegoldmine.com/ns/v1#annotation";
const P_AXIOM_ID: &str = "https://narrativegoldmine.com/ns/v1#axiomId";

const P_INFERRED_AT: &str = "https://narrativegoldmine.com/ns/v1#inferredAt";
const P_INFERRED_TIME_MS: &str = "https://narrativegoldmine.com/ns/v1#inferenceTimeMs";
const P_INFERRED_VERSION: &str = "https://narrativegoldmine.com/ns/v1#reasonerVersion";
const INFER_META_IRI: &str = "urn:ngm:inference:meta";

// ADR-099 D3: provenance vocabulary attached to every inferred quad so the
// asserted-vs-inferred distinction is queryable and each inference traces to a
// reasoner run.
const P_PROV_GENERATED_BY: &str = "http://www.w3.org/ns/prov#wasGeneratedBy";
const T_PROV_ACTIVITY: &str = "http://www.w3.org/ns/prov#Activity";
const P_PROV_ENDED_AT: &str = "http://www.w3.org/ns/prov#endedAtTime";
const P_DERIVATION: &str = "https://narrativegoldmine.com/ns/v1#derivation";
const P_CONFIDENCE: &str = "https://narrativegoldmine.com/ns/v1#confidence";
const P_RUN_ID: &str = "https://narrativegoldmine.com/ns/v1#runId";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const OWL_EQUIVALENT_CLASS: &str = "http://www.w3.org/2002/07/owl#equivalentClass";
/// Derivation marker value: machine-inferred (vs asserted).
const DERIVATION_INFERRED: &str = "inferred";

const P_CACHE_COMPUTED_AT: &str = "https://narrativegoldmine.com/ns/v1#computedAt";
const P_CACHE_DISTANCES: &str = "https://narrativegoldmine.com/ns/v1#distances";
const P_CACHE_PATHS: &str = "https://narrativegoldmine.com/ns/v1#paths";
const P_CACHE_MATRIX: &str = "https://narrativegoldmine.com/ns/v1#matrix";
const P_CACHE_TARGET: &str = "https://narrativegoldmine.com/ns/v1#targetNode";
const P_CACHE_COMP_TIME: &str = "https://narrativegoldmine.com/ns/v1#computationTimeMs";
const APSP_IRI: &str = "urn:ngm:pathcache:apsp";

/// Oxigraph-backed `OntologyRepository` implementation.
///
/// The `store` field is wrapped in `Arc` so the adapter can be cloned
/// cheaply into Actix actors and request handlers without re-opening the
/// RocksDB column families.
pub struct OxigraphOntologyRepository {
    store: Arc<Store>,
}

impl OxigraphOntologyRepository {
    /// Open (or create) an Oxigraph store at `data_dir` and return a new
    /// adapter handle. The store is persistent (RocksDB backend); call
    /// sites are expected to keep a single global instance per ADR-11 §D1
    /// (single-binary deployment, single writer).
    pub async fn open(data_dir: &std::path::Path) -> RepoResult<Self> {
        let path = data_dir.to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::open(&path))
            .await
            .map_err(|e| db_err(format!("join error: {e}")))?
            .map_err(db_err)?;
        let store = Arc::new(store);

        // ADR-101 D2: apply any pending SPARQL migrations exactly once on
        // startup, recorded in `urn:ngm:graph:migrations`. Idempotent —
        // already-applied migrations are skipped via the ledger.
        let mig_store = Arc::clone(&store);
        let applied =
            tokio::task::spawn_blocking(move || crate::sparql_migrations::run_pending(&mig_store))
                .await
                .map_err(|e| db_err(format!("join error: {e}")))?
                .map_err(db_err)?;
        if !applied.is_empty() {
            tracing::info!(
                "applied {} SPARQL migration(s): {:?}",
                applied.len(),
                applied
            );
        }

        // PRD-022 WS-1: load the real W3C SHACL shape files into GRAPH_SHAPES so
        // the shape catalogue is populated and queryable at startup (the
        // trust-status canary counts these). Idempotent + reload-safe. A load
        // failure is fatal — a trust layer that silently ships with an empty
        // shapes graph would be exactly the "loaded on paper" gap this closes.
        let shape_store = Arc::clone(&store);
        let shape_count = tokio::task::spawn_blocking(move || load_shacl_shapes(&shape_store))
            .await
            .map_err(|e| db_err(format!("join error: {e}")))??;
        tracing::info!("loaded {shape_count} SHACL NodeShape(s) into {GRAPH_SHAPES}");

        Ok(Self { store })
    }

    /// Construct over an already-opened store (used by tests + the migration
    /// tool which opens once and writes via several adapters).
    pub fn from_store(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// Convenience accessor for tests / migration tooling.
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// PROV-O emission seam (PRD-022 WS-2). Every production write path that
    /// should leave a queryable provenance trail — ontology mutation, inference,
    /// decision recording — reifies its event here as an append-only PROV-O
    /// activity (the full `prov:Entity`/`prov:Activity`/`prov:Agent` triad) in
    /// `GRAPH_PROVENANCE`. Returns the triple count. This is the single wiring
    /// point over `provenance_emitter::reify_activity`, so callers never
    /// re-implement the `spawn_blocking` + error-mapping dance.
    pub async fn emit_provenance(
        &self,
        record: crate::provenance_emitter::ActivityRecord,
    ) -> RepoResult<usize> {
        crate::provenance_emitter::emit_activity(Arc::clone(&self.store), record)
            .await
            .map_err(db_err)
    }

    /// Answer "provenance for entity X": walk the `prov:wasGeneratedBy` /
    /// `prov:wasDerivedFrom` chain in `GRAPH_PROVENANCE` (seeding through
    /// `rdf:subject` so a governed statement subject resolves to its reified
    /// assertion-version provenance) and return the chain.
    pub async fn provenance_for_entity(
        &self,
        entity_urn: String,
        max_depth: usize,
    ) -> RepoResult<crate::provenance_emitter::ProvenanceChain> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            crate::provenance_emitter::provenance_for_entity(&store, &entity_urn, max_depth)
        })
        .await
        .map_err(|e| db_err(format!("join error: {e}")))?
        .map_err(db_err)
    }

    /// Content-address a reasoner run (ADR-099 D3 provenance). Stable for an
    /// identical `(timestamp, version, axiom-count)` triple; distinct runs get
    /// distinct ids. 12 hex chars, same discipline as axiom IRIs.
    fn inference_run_id(results: &InferenceResults) -> String {
        let mut hasher = Sha256::new();
        hasher.update(results.timestamp.to_rfc3339().as_bytes());
        hasher.update(results.reasoner_version.as_bytes());
        hasher.update(results.inferred_axioms.len().to_le_bytes());
        let digest = hasher.finalize();
        digest
            .iter()
            .take(6)
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    /// ADR-099 D3 — "clear inferred" is a single-graph `CLEAR`. Idempotent;
    /// touches only `urn:ngm:graph:ontology:inferred`.
    pub async fn clear_inferred_graph(&self) -> RepoResult<()> {
        self.run_update(format!(
            "{PROLOGUE}CLEAR GRAPH <{GRAPH_ONTOLOGY_INFERRED}>\n"
        ))
        .await
    }

    // ------------------------------------------------------------------
    // WS-9 — fenced derived-graph writes (:summary / :observed only)
    //
    // Inherent methods, NOT on the `OntologyRepository` trait — the trait
    // surface is frozen. Mirrors `sparql_select_json`/`read_inferred_graph`
    // which are likewise inherent. Handlers call the concrete
    // `Arc<OxigraphOntologyRepository>` in `AppState` directly.
    // ------------------------------------------------------------------

    /// True iff `which` is one of the writable derived graphs (`summary` |
    /// `observed`). Returns the canonical graph IRI on success.
    fn derived_graph_iri(which: &str) -> RepoResult<&'static str> {
        match which {
            "summary" => Ok(GRAPH_ONTOLOGY_SUMMARY),
            "observed" => Ok(GRAPH_ONTOLOGY_OBSERVED),
            other => Err(db_err(format!(
                "derived graph must be 'summary' or 'observed', got '{other}'"
            ))),
        }
    }

    /// Append quads to the derived graphs. Each tuple is `(graph, s, p, o)`.
    ///
    /// FENCE (defence-in-depth): every quad whose graph is `:assert` or
    /// `:inferred` is REJECTED ([`DERIVED_FENCE`]); only `:summary`/`:observed`
    /// are accepted. Subjects/predicates and IRI objects are validated by
    /// [`iri_is_safe`] before interpolation; literal objects are escaped via
    /// [`escape_literal`] — so a hostile `o` cannot break out of the
    /// `INSERT DATA` block (SPARQL-injection guard). Mirrors
    /// `store_inference_results`' single-`run_update` atomic INSERT DATA pattern.
    pub async fn append_derived_quads(
        &self,
        quads: Vec<(String, String, String, String)>,
    ) -> RepoResult<usize> {
        if quads.is_empty() {
            return Ok(0);
        }

        // Group bodies per graph so each named graph gets one INSERT DATA block.
        let mut summary_body = String::new();
        let mut observed_body = String::new();

        for (g, s, p, o) in &quads {
            // Server-side fence — reject assert/inferred outright.
            if DERIVED_FENCE.contains(&g.as_str()) {
                return Err(db_err(format!(
                    "fenced graph {g} is not writable via the derived path"
                )));
            }
            if g != GRAPH_ONTOLOGY_SUMMARY && g != GRAPH_ONTOLOGY_OBSERVED {
                return Err(db_err(format!(
                    "derived writes only target :summary/:observed, got {g}"
                )));
            }
            // s and p MUST be safe IRIs.
            if !iri_is_safe(s) {
                return Err(db_err(format!("unsafe subject IRI in derived quad: {s}")));
            }
            if !iri_is_safe(p) {
                return Err(db_err(format!("unsafe predicate IRI in derived quad: {p}")));
            }
            // o is an IRI when it looks like one, else a quoted literal.
            let object_term = if looks_like_iri(o) {
                if !iri_is_safe(o) {
                    return Err(db_err(format!("unsafe object IRI in derived quad: {o}")));
                }
                format!("<{o}>")
            } else {
                format!("\"{}\"", escape_literal(o))
            };

            let triple = format!("<{s}> <{p}> {object_term} .\n");
            if g == GRAPH_ONTOLOGY_SUMMARY {
                summary_body.push_str(&triple);
            } else {
                observed_body.push_str(&triple);
            }
        }

        // Combine into ONE update so both graphs commit atomically (store
        // `;`-separated statements per ADR-11 §D9 / store_inference_results).
        let mut stmts: Vec<String> = Vec::new();
        if !summary_body.is_empty() {
            stmts.push(format!(
                "INSERT DATA {{ GRAPH <{GRAPH_ONTOLOGY_SUMMARY}> {{\n{summary_body}}} }}"
            ));
        }
        if !observed_body.is_empty() {
            stmts.push(format!(
                "INSERT DATA {{ GRAPH <{GRAPH_ONTOLOGY_OBSERVED}> {{\n{observed_body}}} }}"
            ));
        }
        let combined = stmts.join(" ;\n");
        self.run_update(format!("{PROLOGUE}{combined}\n")).await?;
        Ok(quads.len())
    }

    /// Convenience used by the WS-9 approve path: write `summary_triples`
    /// `(s, p, o)` into `:summary`, plus a `prov:wasGeneratedBy <activity_urn>`
    /// marker on each subject. Routes through [`append_derived_quads`] so it
    /// INHERITS the fence + injection guards (never call `run_update` directly).
    pub async fn append_derived_summary(
        &self,
        owner_did: Option<&str>,
        activity_urn: &str,
        summary_triples: Vec<(String, String, String)>,
    ) -> RepoResult<()> {
        const P_PROV_GENERATED_BY: &str = "http://www.w3.org/ns/prov#wasGeneratedBy";
        const P_OWNER_DID: &str = "https://narrativegoldmine.com/ns/v1#ownerDid";

        if !iri_is_safe(activity_urn) {
            return Err(db_err(format!("unsafe activity URN: {activity_urn}")));
        }

        let mut quads: Vec<(String, String, String, String)> =
            Vec::with_capacity(summary_triples.len() * 2 + 1);

        for (s, p, o) in summary_triples {
            // Provenance marker linking the subject to the generating activity.
            quads.push((
                GRAPH_ONTOLOGY_SUMMARY.to_string(),
                s.clone(),
                P_PROV_GENERATED_BY.to_string(),
                activity_urn.to_string(),
            ));
            // Optional owner attribution (only when scoped).
            if let Some(did) = owner_did {
                quads.push((
                    GRAPH_ONTOLOGY_SUMMARY.to_string(),
                    s.clone(),
                    P_OWNER_DID.to_string(),
                    did.to_string(),
                ));
            }
            // The summary triple itself.
            quads.push((GRAPH_ONTOLOGY_SUMMARY.to_string(), s, p, o));
        }

        self.append_derived_quads(quads).await.map(|_| ())
    }

    /// Idempotent single-graph `CLEAR` of a derived graph (`summary`|`observed`).
    /// Mirrors [`clear_inferred_graph`]. Never touches `:assert`/`:inferred`.
    pub async fn clear_derived_graph(&self, which: &str) -> RepoResult<()> {
        let graph = Self::derived_graph_iri(which)?;
        self.run_update(format!("{PROLOGUE}CLEAR GRAPH <{graph}>\n"))
            .await
    }

    /// Read a derived graph as `{ namedGraph, triples: [{s,p,o}] }`. Mirrors
    /// [`read_inferred_graph`] but FROM `:summary` or `:observed` (validated).
    pub async fn read_derived_graph(&self, which: &str) -> RepoResult<serde_json::Value> {
        let graph = Self::derived_graph_iri(which)?;
        let q = format!(
            "{PROLOGUE}\
             SELECT ?s ?p ?o\n\
             FROM <{graph}>\n\
             WHERE {{ ?s ?p ?o }}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        let triples: Vec<serde_json::Value> = rows
            .iter()
            .filter_map(|row| {
                let s = row.first().and_then(|t| t.as_ref()).map(term_lexical)?;
                let p = row.get(1).and_then(|t| t.as_ref()).map(term_lexical)?;
                let o = row.get(2).and_then(|t| t.as_ref()).map(term_lexical)?;
                Some(serde_json::json!({ "s": s, "p": p, "o": o }))
            })
            .collect();
        Ok(serde_json::json!({
            "namedGraph": graph,
            "triples": triples,
        }))
    }

    /// Execute a *read-only* SPARQL SELECT/ASK and return SPARQL 1.1 Query
    /// Results JSON (`{ head: { vars }, results: { bindings } }` for SELECT,
    /// `{ head: {}, boolean }` for ASK). GPU-only constraint (PRD-018): this
    /// executes server-side over Oxigraph and only returns rows — it never
    /// solves layout. The caller MUST pre-validate the query is read-only
    /// (`validate_read_only_sparql`); this method additionally only accepts
    /// `Solutions`/`Boolean` result kinds and rejects anything else.
    pub async fn sparql_select_json(&self, query: String) -> RepoResult<serde_json::Value> {
        let store = Arc::clone(&self.store);
        // WS-0: clamp/inject the LIMIT BEFORE execution so Oxigraph never
        // materialises more than MAX_SPARQL_ROWS rows for a SELECT.
        let query = clamp_select_limit(&query);
        tokio::task::spawn_blocking(move || -> RepoResult<serde_json::Value> {
            match store.query(&query).map_err(db_err)? {
                QueryResults::Solutions(solutions) => {
                    let vars: Vec<String> = solutions
                        .variables()
                        .iter()
                        .map(|v| v.as_str().to_string())
                        .collect();
                    let mut bindings: Vec<serde_json::Value> = Vec::new();
                    // WS-0 defence in depth: row + running serialised-byte fences
                    // independent of the LIMIT rewrite (a sub-SELECT LIMIT could
                    // slip past the string rewrite; these caps cannot).
                    let mut est_bytes = 0usize;
                    let mut truncated = false;
                    for sol in solutions {
                        if bindings.len() >= MAX_SPARQL_ROWS {
                            truncated = true;
                            break;
                        }
                        let sol = sol.map_err(db_err)?;
                        let mut row = serde_json::Map::new();
                        for v in &vars {
                            if let Some(term) = sol.get(v.as_str()) {
                                row.insert(v.clone(), term_to_json(term));
                            }
                        }
                        let row = serde_json::Value::Object(row);
                        est_bytes += serde_json::to_string(&row).map(|s| s.len()).unwrap_or(0);
                        if est_bytes > MAX_SPARQL_RESULT_BYTES {
                            return Err(db_err("SPARQL result exceeds 8MiB cap"));
                        }
                        bindings.push(row);
                    }
                    Ok(serde_json::json!({
                        "head": { "vars": vars },
                        "results": { "bindings": bindings },
                        "truncated": truncated,
                    }))
                }
                QueryResults::Boolean(b) => Ok(serde_json::json!({
                    "head": {},
                    "boolean": b,
                })),
                // CONSTRUCT/DESCRIBE return graphs; not exposed on this JSON
                // results contract.
                _ => Err(db_err("query did not return SELECT/ASK results")),
            }
        })
        .await
        .map_err(|e| db_err(format!("join error: {e}")))?
    }

    /// Read the inferred named graph as `{ namedGraph, runId?, triples: [...] }`
    /// where each triple is `{ s, p, o }` lexical strings (ADR-099 D4 surface
    /// for the `InferencePanel`). Reuses the store's pattern scan over
    /// `urn:ngm:graph:ontology:inferred` only.
    pub async fn read_inferred_graph(&self) -> RepoResult<serde_json::Value> {
        // WS-0: bound the server-built inferred-graph read so a large inferred
        // closure cannot serialise unboundedly.
        let q = format!(
            "{PROLOGUE}\
             SELECT ?s ?p ?o\n\
             FROM <{GRAPH_ONTOLOGY_INFERRED}>\n\
             WHERE {{ ?s ?p ?o }}\n\
             LIMIT {MAX_SPARQL_ROWS}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        let truncated = rows.len() >= MAX_SPARQL_ROWS;
        let triples: Vec<serde_json::Value> = rows
            .iter()
            .filter_map(|row| {
                let s = row.first().and_then(|t| t.as_ref()).map(term_lexical)?;
                let p = row.get(1).and_then(|t| t.as_ref()).map(term_lexical)?;
                let o = row.get(2).and_then(|t| t.as_ref()).map(term_lexical)?;
                Some(serde_json::json!({ "s": s, "p": p, "o": o }))
            })
            .collect();

        // Surface the latest run id if recorded.
        let run_q = format!(
            "{PROLOGUE}\
             SELECT ?run\n\
             FROM <{GRAPH_ONTOLOGY_INFERRED}>\n\
             WHERE {{ <{INFER_META_IRI}> <{P_RUN_ID}> ?run }}\n"
        );
        let (_v, run_rows) = self.run_select(run_q).await?;
        let run_id = run_rows
            .first()
            .and_then(|r| r.first())
            .and_then(|t| t.as_ref())
            .map(term_lexical);

        Ok(serde_json::json!({
            "namedGraph": GRAPH_ONTOLOGY_INFERRED,
            "runId": run_id,
            "triples": triples,
            "truncated": truncated,
        }))
    }

    // ------------------------------------------------------------------
    // Internal helpers wrapping the synchronous Oxigraph API in async
    // contexts via `spawn_blocking`.
    // ------------------------------------------------------------------

    async fn run_update(&self, sparql: String) -> RepoResult<()> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || store.update(&sparql).map_err(db_err))
            .await
            .map_err(|e| db_err(format!("join error: {e}")))?
    }

    /// Execute a SELECT query and collect every solution into a vector of
    /// per-variable lexical strings. None where the binding was absent.
    async fn run_select(
        &self,
        sparql: String,
    ) -> RepoResult<(Vec<String>, Vec<Vec<Option<Term>>>)> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(
            move || -> RepoResult<(Vec<String>, Vec<Vec<Option<Term>>>)> {
                let results = store.query(&sparql).map_err(db_err)?;
                match results {
                    QueryResults::Solutions(solutions) => {
                        let vars: Vec<String> = solutions
                            .variables()
                            .iter()
                            .map(|v| v.as_str().to_string())
                            .collect();
                        let mut rows: Vec<Vec<Option<Term>>> = Vec::new();
                        for sol in solutions {
                            let sol = sol.map_err(db_err)?;
                            let row: Vec<Option<Term>> =
                                vars.iter().map(|v| sol.get(v.as_str()).cloned()).collect();
                            rows.push(row);
                        }
                        Ok((vars, rows))
                    }
                    _ => Err(db_err("SELECT did not return Solutions")),
                }
            },
        )
        .await
        .map_err(|e| db_err(format!("join error: {e}")))?
    }

    async fn run_ask(&self, sparql: String) -> RepoResult<bool> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || -> RepoResult<bool> {
            match store.query(&sparql).map_err(db_err)? {
                QueryResults::Boolean(b) => Ok(b),
                _ => Err(db_err("ASK did not return Boolean")),
            }
        })
        .await
        .map_err(|e| db_err(format!("join error: {e}")))?
    }

    /// Build the INSERT DATA block for a single OwlClass. Caller wraps in
    /// `GRAPH <…> { … }` + the outer `INSERT DATA { … }`.
    fn class_insert_block(c: &OwlClass) -> String {
        let iri = class_iri(c);
        let mut buf = String::with_capacity(1024);

        buf.push_str(&format!(
            "<{iri}> a <{T_VC_ONTOLOGY_CLASS}> , <{T_OWL_CLASS}> .\n"
        ));

        if let Some(v) = &c.label {
            buf.push_str(&format!(
                "<{iri}> <{P_LABEL}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.term_id {
            buf.push_str(&format!(
                "<{iri}> <{P_TERM_ID}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.preferred_term {
            buf.push_str(&format!(
                "<{iri}> <{P_PREFERRED_TERM}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.description {
            buf.push_str(&format!(
                "<{iri}> <{P_DESCRIPTION}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.source_domain {
            buf.push_str(&format!(
                "<{iri}> <{P_SOURCE_DOMAIN}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.version {
            buf.push_str(&format!(
                "<{iri}> <{P_VERSION}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.class_type {
            buf.push_str(&format!(
                "<{iri}> <{P_CLASS_TYPE}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.status {
            buf.push_str(&format!(
                "<{iri}> <{P_STATUS}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.maturity {
            buf.push_str(&format!(
                "<{iri}> <{P_MATURITY}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = c.quality_score {
            buf.push_str(&format!(
                "<{iri}> <{P_QUALITY_SCORE}> \"{v}\"^^xsd:float .\n"
            ));
        }
        if let Some(v) = c.authority_score {
            buf.push_str(&format!(
                "<{iri}> <{P_AUTHORITY_SCORE}> \"{v}\"^^xsd:float .\n"
            ));
        }
        if let Some(v) = c.public_access {
            buf.push_str(&format!(
                "<{iri}> <{P_PUBLIC_ACCESS}> \"{v}\"^^xsd:boolean .\n"
            ));
        }
        if let Some(v) = &c.content_status {
            buf.push_str(&format!(
                "<{iri}> <{P_CONTENT_STATUS}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.owl_physicality {
            buf.push_str(&format!(
                "<{iri}> <{P_OWL_PHYSICALITY}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.owl_role {
            buf.push_str(&format!(
                "<{iri}> <{P_OWL_ROLE}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.belongs_to_domain {
            buf.push_str(&format!(
                "<{iri}> <{P_BELONGS_TO_DOMAIN}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.bridges_to_domain {
            buf.push_str(&format!(
                "<{iri}> <{P_BRIDGES_TO_DOMAIN}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.source_file {
            buf.push_str(&format!(
                "<{iri}> <{P_SOURCE_FILE}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.file_sha1 {
            buf.push_str(&format!(
                "<{iri}> <{P_FILE_SHA1}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = &c.markdown_content {
            buf.push_str(&format!(
                "<{iri}> <{P_MARKDOWN_CONTENT}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        if let Some(v) = c.last_synced {
            buf.push_str(&format!(
                "<{iri}> <{P_LAST_SYNCED}> \"{}\"^^xsd:dateTime .\n",
                v.to_rfc3339()
            ));
        }
        if let Some(v) = &c.additional_metadata {
            buf.push_str(&format!(
                "<{iri}> <{P_ADDITIONAL_META}> \"{}\" .\n",
                escape_literal(v)
            ));
        }

        // Parent classes (rdfs:subClassOf) — one triple per parent.
        for parent in &c.parent_classes {
            buf.push_str(&format!("<{iri}> <{P_SUBCLASS_OF}> <{parent}> .\n"));
        }

        // Vec<String> semantic-relationship fields. Targets stay as
        // string-literals when they don't look like IRIs (callers often
        // pass `[[wikilink]]`-style labels); the resolution back to
        // OntologyClass IRIs happens in the OntologyMutationService.
        let vec_predicates: [(&str, &Vec<String>); 8] = [
            (P_HAS_PART, &c.has_part),
            (P_IS_PART_OF, &c.is_part_of),
            (P_REQUIRES, &c.requires),
            (P_DEPENDS_ON, &c.depends_on),
            (P_ENABLES, &c.enables),
            (P_RELATES_TO, &c.relates_to),
            (P_BRIDGES_TO, &c.bridges_to),
            (P_BRIDGES_FROM, &c.bridges_from),
        ];
        for (pred, vals) in vec_predicates.iter() {
            for v in vals.iter() {
                let trimmed = v.trim_start_matches("[[").trim_end_matches("]]").trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with("urn:") || trimmed.starts_with("http") {
                    buf.push_str(&format!("<{iri}> <{pred}> <{trimmed}> .\n"));
                } else {
                    buf.push_str(&format!(
                        "<{iri}> <{pred}> \"{}\" .\n",
                        escape_literal(trimmed)
                    ));
                }
            }
        }

        // other_relationships: per-key namespace under vc:otherRel/<key>.
        for (rel_name, targets) in &c.other_relationships {
            let pred = format!("{P_OTHER_REL_PREFIX}{}", slug(rel_name));
            for v in targets {
                let trimmed = v.trim_start_matches("[[").trim_end_matches("]]").trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with("urn:") || trimmed.starts_with("http") {
                    buf.push_str(&format!("<{iri}> <{pred}> <{trimmed}> .\n"));
                } else {
                    buf.push_str(&format!(
                        "<{iri}> <{pred}> \"{}\" .\n",
                        escape_literal(trimmed)
                    ));
                }
            }
        }

        // properties: per-key namespace under vc:property/<key>.
        for (k, v) in &c.properties {
            let pred = format!("{P_PROPERTY_PREFIX}{}", slug(k));
            buf.push_str(&format!("<{iri}> <{pred}> \"{}\" .\n", escape_literal(v)));
        }

        buf
    }

    fn property_insert_block(p: &OwlProperty) -> String {
        let iri = property_iri(p);
        let type_iri = property_type_str(&p.property_type);
        let mut buf = String::with_capacity(512);
        buf.push_str(&format!("<{iri}> a {type_iri} .\n"));
        if let Some(v) = &p.label {
            buf.push_str(&format!(
                "<{iri}> <{P_LABEL}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        for d in &p.domain {
            if d.starts_with("urn:") || d.starts_with("http") {
                buf.push_str(&format!("<{iri}> <{P_DOMAIN}> <{d}> .\n"));
            } else {
                buf.push_str(&format!(
                    "<{iri}> <{P_DOMAIN}> \"{}\" .\n",
                    escape_literal(d)
                ));
            }
        }
        for r in &p.range {
            if r.starts_with("urn:") || r.starts_with("http") {
                buf.push_str(&format!("<{iri}> <{P_RANGE}> <{r}> .\n"));
            } else {
                buf.push_str(&format!(
                    "<{iri}> <{P_RANGE}> \"{}\" .\n",
                    escape_literal(r)
                ));
            }
        }
        if let Some(v) = p.quality_score {
            buf.push_str(&format!(
                "<{iri}> <{P_QUALITY_SCORE}> \"{v}\"^^xsd:float .\n"
            ));
        }
        if let Some(v) = p.authority_score {
            buf.push_str(&format!(
                "<{iri}> <{P_AUTHORITY_SCORE}> \"{v}\"^^xsd:float .\n"
            ));
        }
        if let Some(v) = &p.source_file {
            buf.push_str(&format!(
                "<{iri}> <{P_SOURCE_FILE}> \"{}\" .\n",
                escape_literal(v)
            ));
        }
        buf
    }

    fn axiom_insert_block(a: &OwlAxiom) -> String {
        let iri = axiom_iri(a);
        let id = axiom_id(a);
        let at = axiom_type_str(&a.axiom_type);
        let mut buf = String::with_capacity(256);
        buf.push_str(&format!("<{iri}> a <{T_VC_AXIOM}> .\n"));
        buf.push_str(&format!("<{iri}> <{P_AXIOM_TYPE}> \"{at}\" .\n"));
        buf.push_str(&format!("<{iri}> <{P_AXIOM_ID}> \"{id}\"^^xsd:integer .\n"));
        // Subject + object are IRIs if they look like IRIs, otherwise literals.
        if a.subject.starts_with("urn:") || a.subject.starts_with("http") {
            buf.push_str(&format!("<{iri}> <{P_AXIOM_SUBJECT}> <{}> .\n", a.subject));
        } else {
            buf.push_str(&format!(
                "<{iri}> <{P_AXIOM_SUBJECT}> \"{}\" .\n",
                escape_literal(&a.subject)
            ));
        }
        if a.object.starts_with("urn:") || a.object.starts_with("http") {
            buf.push_str(&format!("<{iri}> <{P_AXIOM_OBJECT}> <{}> .\n", a.object));
        } else {
            buf.push_str(&format!(
                "<{iri}> <{P_AXIOM_OBJECT}> \"{}\" .\n",
                escape_literal(&a.object)
            ));
        }
        for (k, v) in &a.annotations {
            buf.push_str(&format!(
                "<{iri}> <{P_AXIOM_ANNOTATION}> \"{}={}\" .\n",
                escape_literal(k),
                escape_literal(v)
            ));
        }
        buf
    }
}

#[async_trait]
impl OntologyRepository for OxigraphOntologyRepository {
    // ------------------------------------------------------------------
    // Graph-level read/write
    // ------------------------------------------------------------------

    async fn load_ontology_graph(&self) -> RepoResult<Arc<GraphData>> {
        // SPARQL: stream every triple from the assert graph then fold
        // class subjects into Nodes, edge predicates into Edges.
        let q = format!(
            "{PROLOGUE}\
             SELECT ?s ?p ?o\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ ?s ?p ?o }}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;

        // Subject IRI → assigned numeric node id (sequential u32).
        let mut node_ids: HashMap<String, u32> = HashMap::new();
        // Subject IRI → accumulating OwlClass (label, etc.).
        let mut class_attrs: HashMap<String, OwlClass> = HashMap::new();
        // (source-IRI, target-IRI, edge-type-str)
        let mut pending_edges: Vec<(String, String, String)> = Vec::new();

        let mut next_id: u32 = 1;
        let mut alloc_id = |iri: &str, table: &mut HashMap<String, u32>, nx: &mut u32| -> u32 {
            *table.entry(iri.to_string()).or_insert_with(|| {
                let id = *nx;
                *nx += 1;
                id
            })
        };

        for row in &rows {
            let (s, p, o) = match (&row[0], &row[1], &row[2]) {
                (Some(Term::NamedNode(s)), Some(Term::NamedNode(p)), o) => {
                    (s.as_str().to_string(), p.as_str().to_string(), o.clone())
                }
                _ => continue, // ignore blank-node subjects/predicates
            };

            // Always ensure subject has an entry in class_attrs.
            let entry = class_attrs.entry(s.clone()).or_insert_with(|| {
                let mut c = OwlClass::default();
                c.iri = s.clone();
                c
            });

            match p.as_str() {
                P_TYPE => { /* keep — type triple already handled by class membership */ }
                P_LABEL => {
                    if let Some(Term::Literal(l)) = o {
                        entry.label = Some(l.value().to_string());
                    }
                }
                P_DESCRIPTION => {
                    if let Some(Term::Literal(l)) = o {
                        entry.description = Some(l.value().to_string());
                    }
                }
                P_TERM_ID => {
                    if let Some(t) = o {
                        entry.term_id = Some(term_lexical(&t));
                    }
                }
                P_PREFERRED_TERM => {
                    if let Some(t) = o {
                        entry.preferred_term = Some(term_lexical(&t));
                    }
                }
                P_SOURCE_DOMAIN => {
                    if let Some(t) = o {
                        entry.source_domain = Some(term_lexical(&t));
                    }
                }
                P_VERSION => {
                    if let Some(t) = o {
                        entry.version = Some(term_lexical(&t));
                    }
                }
                P_CLASS_TYPE => {
                    if let Some(t) = o {
                        entry.class_type = Some(term_lexical(&t));
                    }
                }
                P_STATUS => {
                    if let Some(t) = o {
                        entry.status = Some(term_lexical(&t));
                    }
                }
                P_MATURITY => {
                    if let Some(t) = o {
                        entry.maturity = Some(term_lexical(&t));
                    }
                }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        entry.quality_score = l.value().parse().ok();
                    }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        entry.authority_score = l.value().parse().ok();
                    }
                }
                P_PUBLIC_ACCESS => {
                    if let Some(Term::Literal(l)) = o {
                        entry.public_access = l.value().parse().ok();
                    }
                }
                P_OWL_PHYSICALITY => {
                    if let Some(t) = o {
                        entry.owl_physicality = Some(term_lexical(&t));
                    }
                }
                P_OWL_ROLE => {
                    if let Some(t) = o {
                        entry.owl_role = Some(term_lexical(&t));
                    }
                }
                P_BELONGS_TO_DOMAIN => {
                    if let Some(t) = o {
                        entry.belongs_to_domain = Some(term_lexical(&t));
                    }
                }
                P_BRIDGES_TO_DOMAIN => {
                    if let Some(t) = o {
                        entry.bridges_to_domain = Some(term_lexical(&t));
                    }
                }
                P_SUBCLASS_OF => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.parent_classes.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "is_subclass_of".into()));
                    }
                }
                P_HAS_PART => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.has_part.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "has_part".into()));
                    }
                }
                P_IS_PART_OF => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.is_part_of.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "is_part_of".into()));
                    }
                }
                P_REQUIRES => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.requires.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "requires".into()));
                    }
                }
                P_DEPENDS_ON => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.depends_on.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "depends_on".into()));
                    }
                }
                P_ENABLES => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.enables.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "enables".into()));
                    }
                }
                P_RELATES_TO => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.relates_to.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "relates_to".into()));
                    }
                }
                P_BRIDGES_TO => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.bridges_to.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "bridges_to".into()));
                    }
                }
                P_BRIDGES_FROM => {
                    if let Some(t) = o {
                        let tgt = term_lexical(&t);
                        entry.bridges_from.push(tgt.clone());
                        pending_edges.push((s.clone(), tgt, "bridges_from".into()));
                    }
                }
                _ => {
                    // Other vc:otherRel/* and vc:property/* predicates.
                    if p.starts_with(P_OTHER_REL_PREFIX) {
                        if let Some(t) = o {
                            let key = p.trim_start_matches(P_OTHER_REL_PREFIX).to_string();
                            let tgt = term_lexical(&t);
                            entry
                                .other_relationships
                                .entry(key.clone())
                                .or_insert_with(Vec::new)
                                .push(tgt.clone());
                            pending_edges.push((s.clone(), tgt, key));
                        }
                    } else if p.starts_with(P_PROPERTY_PREFIX) {
                        if let Some(t) = o {
                            let key = p.trim_start_matches(P_PROPERTY_PREFIX).to_string();
                            entry.properties.insert(key, term_lexical(&t));
                        }
                    }
                }
            }
        }

        // Materialise GraphNode entries. Each OntologyClass carries the
        // T1-class-bits-resolved class-bit (0x04000000) in the high-bits
        // of the numeric id. Phase 2 will swap this for a richer wire
        // encoding; for now we just allocate sequential ids and stash the
        // IRI in `metadata_id` + `owl_class_iri` for round-trip.
        let mut nodes: Vec<Node> = Vec::with_capacity(class_attrs.len());
        for (iri, c) in class_attrs.iter() {
            let id = alloc_id(iri, &mut node_ids, &mut next_id);
            let label = c.label.clone().unwrap_or_else(|| iri.clone());
            let n = Node::new_with_id(iri.clone(), Some(id))
                .with_label(label)
                .with_owl_class_iri(iri.clone())
                .with_type("owl_class".to_string());
            nodes.push(n);
        }

        // Materialise edges. Drop those whose target IRI was never seen
        // in the assert graph — they are dangling references (resolved
        // by name in OntologyMutationService at higher level).
        let mut edges: Vec<Edge> = Vec::with_capacity(pending_edges.len());
        for (src, tgt, etype) in pending_edges {
            let src_id = node_ids.get(&src).copied();
            let tgt_id = node_ids.get(&tgt).copied();
            if let (Some(s), Some(t)) = (src_id, tgt_id) {
                let e = Edge::new(s, t, 1.0).with_edge_type(etype);
                edges.push(e);
            }
        }

        Ok(Arc::new(GraphData {
            nodes,
            edges,
            metadata: Default::default(),
            id_to_metadata: HashMap::new(),
        }))
    }

    async fn save_ontology_graph(&self, graph: &GraphData) -> RepoResult<()> {
        // CLEAR + bulk INSERT pattern. Single atomic SPARQL Update.
        let mut update = String::with_capacity(8192);
        update.push_str(PROLOGUE);
        update.push_str(&format!("CLEAR GRAPH <{GRAPH_ONTOLOGY}> ;\n"));
        update.push_str(&format!("INSERT DATA {{\n  GRAPH <{GRAPH_ONTOLOGY}> {{\n"));
        for node in &graph.nodes {
            let iri = node
                .owl_class_iri
                .clone()
                .unwrap_or_else(|| ngm::class_iri(&slug(&node.metadata_id)));
            update.push_str(&format!(
                "    <{iri}> a <{T_VC_ONTOLOGY_CLASS}> , <{T_OWL_CLASS}> .\n"
            ));
            let label = if node.label.is_empty() {
                node.metadata_id.clone()
            } else {
                node.label.clone()
            };
            update.push_str(&format!(
                "    <{iri}> <{P_LABEL}> \"{}\" .\n",
                escape_literal(&label)
            ));
        }
        // Edges → vc:relatesTo (or typed predicate) triples.
        // Build id→iri map from current nodes.
        let mut id_to_iri: HashMap<u32, String> = HashMap::new();
        for node in &graph.nodes {
            let iri = node
                .owl_class_iri
                .clone()
                .unwrap_or_else(|| ngm::class_iri(&slug(&node.metadata_id)));
            id_to_iri.insert(node.id, iri);
        }
        for edge in &graph.edges {
            let src = match id_to_iri.get(&edge.source) {
                Some(v) => v,
                None => continue,
            };
            let tgt = match id_to_iri.get(&edge.target) {
                Some(v) => v,
                None => continue,
            };
            // An edge that carries its OWL property IRI (a frontmatter
            // relation projected through the vocabulary) is written with that
            // predicate; the coarse `edge_type` label is the fallback.
            let predicate = match edge.owl_property_iri.as_deref() {
                Some(owl) if is_insertable_iri(owl) => owl,
                _ => match edge.edge_type.as_deref() {
                    Some("is_subclass_of") | Some("subclass_of") | Some("SUBCLASS_OF") => {
                        P_SUBCLASS_OF
                    }
                    Some("has_part") => P_HAS_PART,
                    Some("is_part_of") => P_IS_PART_OF,
                    Some("requires") => P_REQUIRES,
                    Some("depends_on") => P_DEPENDS_ON,
                    Some("enables") => P_ENABLES,
                    Some("relates_to") => P_RELATES_TO,
                    Some("bridges_to") => P_BRIDGES_TO,
                    Some("bridges_from") => P_BRIDGES_FROM,
                    _ => P_RELATES_TO,
                },
            };
            update.push_str(&format!("    <{src}> <{predicate}> <{tgt}> .\n"));
        }
        update.push_str("  }\n}\n");
        self.run_update(update).await
    }

    async fn save_ontology(
        &self,
        classes: &[OwlClass],
        properties: &[OwlProperty],
        axioms: &[OwlAxiom],
    ) -> RepoResult<()> {
        if classes.is_empty() && properties.is_empty() && axioms.is_empty() {
            return Ok(());
        }
        let mut update = String::with_capacity(8192);
        update.push_str(PROLOGUE);
        update.push_str(&format!("INSERT DATA {{\n  GRAPH <{GRAPH_ONTOLOGY}> {{\n"));
        for c in classes {
            update.push_str(&Self::class_insert_block(c));
        }
        for p in properties {
            update.push_str(&Self::property_insert_block(p));
        }
        for a in axioms {
            update.push_str(&Self::axiom_insert_block(a));
        }
        update.push_str("  }\n}\n");
        self.run_update(update).await
    }

    // ------------------------------------------------------------------
    // OWL Class CRUD
    // ------------------------------------------------------------------

    async fn add_owl_class(&self, class: &OwlClass) -> RepoResult<String> {
        let iri = class_iri(class);
        // ADR-11 §D6 uniqueness guard — ASK against the assert graph.
        let ask = format!(
            "{PROLOGUE}\
             ASK FROM <{GRAPH_ONTOLOGY}> {{ <{iri}> a <{T_VC_ONTOLOGY_CLASS}> }}\n"
        );
        if self.run_ask(ask).await? {
            return Err(OntologyRepositoryError::InvalidData(format!(
                "OntologyClass IRI already exists: {iri}"
            )));
        }
        let body = Self::class_insert_block(class);
        let update = format!(
            "{PROLOGUE}\
             INSERT DATA {{\n  GRAPH <{GRAPH_ONTOLOGY}> {{\n{body}  }}\n}}\n"
        );
        self.run_update(update).await?;
        Ok(iri)
    }

    async fn get_owl_class(&self, iri: &str) -> RepoResult<Option<OwlClass>> {
        let q = format!(
            "{PROLOGUE}\
             SELECT ?p ?o\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ <{iri}> ?p ?o }}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let mut c = OwlClass::default();
        c.iri = iri.to_string();
        let mut has_type = false;
        for row in rows {
            let p = match &row[0] {
                Some(Term::NamedNode(p)) => p.as_str().to_string(),
                _ => continue,
            };
            let o_opt = row[1].clone();
            match p.as_str() {
                P_TYPE => {
                    if let Some(Term::NamedNode(t)) = o_opt {
                        if t.as_str() == T_VC_ONTOLOGY_CLASS || t.as_str() == T_OWL_CLASS {
                            has_type = true;
                        }
                    }
                }
                P_LABEL => {
                    if let Some(Term::Literal(l)) = o_opt {
                        c.label = Some(l.value().to_string());
                    }
                }
                P_DESCRIPTION => {
                    if let Some(Term::Literal(l)) = o_opt {
                        c.description = Some(l.value().to_string());
                    }
                }
                P_TERM_ID => {
                    if let Some(t) = o_opt {
                        c.term_id = Some(term_lexical(&t));
                    }
                }
                P_PREFERRED_TERM => {
                    if let Some(t) = o_opt {
                        c.preferred_term = Some(term_lexical(&t));
                    }
                }
                P_SOURCE_DOMAIN => {
                    if let Some(t) = o_opt {
                        c.source_domain = Some(term_lexical(&t));
                    }
                }
                P_VERSION => {
                    if let Some(t) = o_opt {
                        c.version = Some(term_lexical(&t));
                    }
                }
                P_CLASS_TYPE => {
                    if let Some(t) = o_opt {
                        c.class_type = Some(term_lexical(&t));
                    }
                }
                P_STATUS => {
                    if let Some(t) = o_opt {
                        c.status = Some(term_lexical(&t));
                    }
                }
                P_MATURITY => {
                    if let Some(t) = o_opt {
                        c.maturity = Some(term_lexical(&t));
                    }
                }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o_opt {
                        c.quality_score = l.value().parse().ok();
                    }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o_opt {
                        c.authority_score = l.value().parse().ok();
                    }
                }
                P_PUBLIC_ACCESS => {
                    if let Some(Term::Literal(l)) = o_opt {
                        c.public_access = l.value().parse().ok();
                    }
                }
                P_CONTENT_STATUS => {
                    if let Some(t) = o_opt {
                        c.content_status = Some(term_lexical(&t));
                    }
                }
                P_OWL_PHYSICALITY => {
                    if let Some(t) = o_opt {
                        c.owl_physicality = Some(term_lexical(&t));
                    }
                }
                P_OWL_ROLE => {
                    if let Some(t) = o_opt {
                        c.owl_role = Some(term_lexical(&t));
                    }
                }
                P_BELONGS_TO_DOMAIN => {
                    if let Some(t) = o_opt {
                        c.belongs_to_domain = Some(term_lexical(&t));
                    }
                }
                P_BRIDGES_TO_DOMAIN => {
                    if let Some(t) = o_opt {
                        c.bridges_to_domain = Some(term_lexical(&t));
                    }
                }
                P_SOURCE_FILE => {
                    if let Some(t) = o_opt {
                        c.source_file = Some(term_lexical(&t));
                    }
                }
                P_FILE_SHA1 => {
                    if let Some(t) = o_opt {
                        c.file_sha1 = Some(term_lexical(&t));
                    }
                }
                P_MARKDOWN_CONTENT => {
                    if let Some(t) = o_opt {
                        c.markdown_content = Some(term_lexical(&t));
                    }
                }
                P_LAST_SYNCED => {
                    if let Some(Term::Literal(l)) = o_opt {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(l.value()) {
                            c.last_synced = Some(dt.with_timezone(&chrono::Utc));
                        }
                    }
                }
                P_ADDITIONAL_META => {
                    if let Some(t) = o_opt {
                        c.additional_metadata = Some(term_lexical(&t));
                    }
                }
                P_SUBCLASS_OF => {
                    if let Some(t) = o_opt {
                        c.parent_classes.push(term_lexical(&t));
                    }
                }
                P_HAS_PART => {
                    if let Some(t) = o_opt {
                        c.has_part.push(term_lexical(&t));
                    }
                }
                P_IS_PART_OF => {
                    if let Some(t) = o_opt {
                        c.is_part_of.push(term_lexical(&t));
                    }
                }
                P_REQUIRES => {
                    if let Some(t) = o_opt {
                        c.requires.push(term_lexical(&t));
                    }
                }
                P_DEPENDS_ON => {
                    if let Some(t) = o_opt {
                        c.depends_on.push(term_lexical(&t));
                    }
                }
                P_ENABLES => {
                    if let Some(t) = o_opt {
                        c.enables.push(term_lexical(&t));
                    }
                }
                P_RELATES_TO => {
                    if let Some(t) = o_opt {
                        c.relates_to.push(term_lexical(&t));
                    }
                }
                P_BRIDGES_TO => {
                    if let Some(t) = o_opt {
                        c.bridges_to.push(term_lexical(&t));
                    }
                }
                P_BRIDGES_FROM => {
                    if let Some(t) = o_opt {
                        c.bridges_from.push(term_lexical(&t));
                    }
                }
                _ if p.starts_with(P_OTHER_REL_PREFIX) => {
                    if let Some(t) = o_opt {
                        let key = p.trim_start_matches(P_OTHER_REL_PREFIX).to_string();
                        c.other_relationships
                            .entry(key)
                            .or_insert_with(Vec::new)
                            .push(term_lexical(&t));
                    }
                }
                _ if p.starts_with(P_PROPERTY_PREFIX) => {
                    if let Some(t) = o_opt {
                        let key = p.trim_start_matches(P_PROPERTY_PREFIX).to_string();
                        c.properties.insert(key, term_lexical(&t));
                    }
                }
                _ => { /* unknown predicate — silently drop */ }
            }
        }
        if !has_type {
            return Ok(None);
        }
        Ok(Some(c))
    }

    async fn list_owl_classes(&self) -> RepoResult<Vec<OwlClass>> {
        // Hot read path. Single SELECT, ORDER BY ?s so we can stream a
        // group-by-subject reducer without materialising a HashMap of
        // partial classes.
        let q = format!(
            "{PROLOGUE}\
             SELECT ?s ?p ?o\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ ?s a <{T_VC_ONTOLOGY_CLASS}> . ?s ?p ?o }}\n\
             ORDER BY ?s\n"
        );
        let (_vars, rows) = self.run_select(q).await?;

        let mut classes: Vec<OwlClass> = Vec::new();
        let mut current_iri: Option<String> = None;
        let mut current: OwlClass = OwlClass::default();

        let flush = |c: &mut OwlClass, out: &mut Vec<OwlClass>| {
            if !c.iri.is_empty() {
                let finished = std::mem::take(c);
                out.push(finished);
            }
        };

        for row in rows {
            let s = match &row[0] {
                Some(Term::NamedNode(s)) => s.as_str().to_string(),
                _ => continue,
            };
            let p = match &row[1] {
                Some(Term::NamedNode(p)) => p.as_str().to_string(),
                _ => continue,
            };
            let o = row[2].clone();

            // Group boundary: flush the previous class.
            if current_iri.as_deref() != Some(&s) {
                flush(&mut current, &mut classes);
                current_iri = Some(s.clone());
                current.iri = s.clone();
            }

            match p.as_str() {
                P_TYPE => { /* class type already filtered by the WHERE */ }
                P_LABEL => {
                    if let Some(Term::Literal(l)) = o {
                        current.label = Some(l.value().to_string());
                    }
                }
                P_DESCRIPTION => {
                    if let Some(Term::Literal(l)) = o {
                        current.description = Some(l.value().to_string());
                    }
                }
                P_TERM_ID => {
                    if let Some(t) = o {
                        current.term_id = Some(term_lexical(&t));
                    }
                }
                P_PREFERRED_TERM => {
                    if let Some(t) = o {
                        current.preferred_term = Some(term_lexical(&t));
                    }
                }
                P_SOURCE_DOMAIN => {
                    if let Some(t) = o {
                        current.source_domain = Some(term_lexical(&t));
                    }
                }
                P_VERSION => {
                    if let Some(t) = o {
                        current.version = Some(term_lexical(&t));
                    }
                }
                P_CLASS_TYPE => {
                    if let Some(t) = o {
                        current.class_type = Some(term_lexical(&t));
                    }
                }
                P_STATUS => {
                    if let Some(t) = o {
                        current.status = Some(term_lexical(&t));
                    }
                }
                P_MATURITY => {
                    if let Some(t) = o {
                        current.maturity = Some(term_lexical(&t));
                    }
                }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        current.quality_score = l.value().parse().ok();
                    }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        current.authority_score = l.value().parse().ok();
                    }
                }
                P_PUBLIC_ACCESS => {
                    if let Some(Term::Literal(l)) = o {
                        current.public_access = l.value().parse().ok();
                    }
                }
                P_CONTENT_STATUS => {
                    if let Some(t) = o {
                        current.content_status = Some(term_lexical(&t));
                    }
                }
                P_OWL_PHYSICALITY => {
                    if let Some(t) = o {
                        current.owl_physicality = Some(term_lexical(&t));
                    }
                }
                P_OWL_ROLE => {
                    if let Some(t) = o {
                        current.owl_role = Some(term_lexical(&t));
                    }
                }
                P_BELONGS_TO_DOMAIN => {
                    if let Some(t) = o {
                        current.belongs_to_domain = Some(term_lexical(&t));
                    }
                }
                P_BRIDGES_TO_DOMAIN => {
                    if let Some(t) = o {
                        current.bridges_to_domain = Some(term_lexical(&t));
                    }
                }
                P_SOURCE_FILE => {
                    if let Some(t) = o {
                        current.source_file = Some(term_lexical(&t));
                    }
                }
                P_FILE_SHA1 => {
                    if let Some(t) = o {
                        current.file_sha1 = Some(term_lexical(&t));
                    }
                }
                P_MARKDOWN_CONTENT => {
                    if let Some(t) = o {
                        current.markdown_content = Some(term_lexical(&t));
                    }
                }
                P_LAST_SYNCED => {
                    if let Some(Term::Literal(l)) = o {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(l.value()) {
                            current.last_synced = Some(dt.with_timezone(&chrono::Utc));
                        }
                    }
                }
                P_ADDITIONAL_META => {
                    if let Some(t) = o {
                        current.additional_metadata = Some(term_lexical(&t));
                    }
                }
                P_SUBCLASS_OF => {
                    if let Some(t) = o {
                        current.parent_classes.push(term_lexical(&t));
                    }
                }
                P_HAS_PART => {
                    if let Some(t) = o {
                        current.has_part.push(term_lexical(&t));
                    }
                }
                P_IS_PART_OF => {
                    if let Some(t) = o {
                        current.is_part_of.push(term_lexical(&t));
                    }
                }
                P_REQUIRES => {
                    if let Some(t) = o {
                        current.requires.push(term_lexical(&t));
                    }
                }
                P_DEPENDS_ON => {
                    if let Some(t) = o {
                        current.depends_on.push(term_lexical(&t));
                    }
                }
                P_ENABLES => {
                    if let Some(t) = o {
                        current.enables.push(term_lexical(&t));
                    }
                }
                P_RELATES_TO => {
                    if let Some(t) = o {
                        current.relates_to.push(term_lexical(&t));
                    }
                }
                P_BRIDGES_TO => {
                    if let Some(t) = o {
                        current.bridges_to.push(term_lexical(&t));
                    }
                }
                P_BRIDGES_FROM => {
                    if let Some(t) = o {
                        current.bridges_from.push(term_lexical(&t));
                    }
                }
                _ if p.starts_with(P_OTHER_REL_PREFIX) => {
                    if let Some(t) = o {
                        let key = p.trim_start_matches(P_OTHER_REL_PREFIX).to_string();
                        current
                            .other_relationships
                            .entry(key)
                            .or_insert_with(Vec::new)
                            .push(term_lexical(&t));
                    }
                }
                _ if p.starts_with(P_PROPERTY_PREFIX) => {
                    if let Some(t) = o {
                        let key = p.trim_start_matches(P_PROPERTY_PREFIX).to_string();
                        current.properties.insert(key, term_lexical(&t));
                    }
                }
                _ => { /* unknown predicate */ }
            }
        }
        // Flush trailing.
        flush(&mut current, &mut classes);
        Ok(classes)
    }

    // ------------------------------------------------------------------
    // OWL Property CRUD
    // ------------------------------------------------------------------

    async fn add_owl_property(&self, property: &OwlProperty) -> RepoResult<String> {
        let iri = property_iri(property);
        let ask = format!(
            "{PROLOGUE}\
             ASK FROM <{GRAPH_ONTOLOGY}> {{\n\
               <{iri}> a ?t .\n\
               FILTER(?t IN (<{T_OWL_OBJECT_PROP}>, <{T_OWL_DATA_PROP}>, <{T_OWL_ANNOT_PROP}>))\n\
             }}\n"
        );
        if self.run_ask(ask).await? {
            return Err(OntologyRepositoryError::InvalidData(format!(
                "OwlProperty IRI already exists: {iri}"
            )));
        }
        let body = Self::property_insert_block(property);
        let update = format!(
            "{PROLOGUE}\
             INSERT DATA {{\n  GRAPH <{GRAPH_ONTOLOGY}> {{\n{body}  }}\n}}\n"
        );
        self.run_update(update).await?;
        Ok(iri)
    }

    async fn get_owl_property(&self, iri: &str) -> RepoResult<Option<OwlProperty>> {
        let q = format!(
            "{PROLOGUE}\
             SELECT ?p ?o\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ <{iri}> ?p ?o }}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let mut prop = OwlProperty {
            iri: iri.to_string(),
            label: None,
            property_type: PropertyType::ObjectProperty,
            domain: Vec::new(),
            range: Vec::new(),
            quality_score: None,
            authority_score: None,
            source_file: None,
        };
        let mut classified = false;
        for row in rows {
            let p = match &row[0] {
                Some(Term::NamedNode(p)) => p.as_str().to_string(),
                _ => continue,
            };
            let o = row[1].clone();
            match p.as_str() {
                P_TYPE => {
                    if let Some(Term::NamedNode(t)) = o {
                        prop.property_type = parse_property_type(t.as_str());
                        classified = true;
                    }
                }
                P_LABEL => {
                    if let Some(Term::Literal(l)) = o {
                        prop.label = Some(l.value().to_string());
                    }
                }
                P_DOMAIN => {
                    if let Some(t) = o {
                        prop.domain.push(term_lexical(&t));
                    }
                }
                P_RANGE => {
                    if let Some(t) = o {
                        prop.range.push(term_lexical(&t));
                    }
                }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        prop.quality_score = l.value().parse().ok();
                    }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        prop.authority_score = l.value().parse().ok();
                    }
                }
                P_SOURCE_FILE => {
                    if let Some(t) = o {
                        prop.source_file = Some(term_lexical(&t));
                    }
                }
                _ => {}
            }
        }
        if !classified {
            return Ok(None);
        }
        Ok(Some(prop))
    }

    async fn list_owl_properties(&self) -> RepoResult<Vec<OwlProperty>> {
        let q = format!(
            "{PROLOGUE}\
             SELECT ?s ?p ?o\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{\n\
               ?s a ?t .\n\
               FILTER(?t IN (<{T_OWL_OBJECT_PROP}>, <{T_OWL_DATA_PROP}>, <{T_OWL_ANNOT_PROP}>))\n\
               ?s ?p ?o .\n\
             }}\n\
             ORDER BY ?s\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        let mut props: Vec<OwlProperty> = Vec::new();
        let mut current_iri: Option<String> = None;
        let mut current = OwlProperty::default();
        let mut classified = false;

        let flush = |cur: &mut OwlProperty, cls: &mut bool, out: &mut Vec<OwlProperty>| {
            if !cur.iri.is_empty() && *cls {
                let finished = std::mem::take(cur);
                out.push(finished);
            }
            *cls = false;
        };

        for row in rows {
            let s = match &row[0] {
                Some(Term::NamedNode(s)) => s.as_str().to_string(),
                _ => continue,
            };
            let p = match &row[1] {
                Some(Term::NamedNode(p)) => p.as_str().to_string(),
                _ => continue,
            };
            let o = row[2].clone();
            if current_iri.as_deref() != Some(&s) {
                flush(&mut current, &mut classified, &mut props);
                current_iri = Some(s.clone());
                current.iri = s.clone();
            }
            match p.as_str() {
                P_TYPE => {
                    if let Some(Term::NamedNode(t)) = o {
                        current.property_type = parse_property_type(t.as_str());
                        classified = true;
                    }
                }
                P_LABEL => {
                    if let Some(Term::Literal(l)) = o {
                        current.label = Some(l.value().to_string());
                    }
                }
                P_DOMAIN => {
                    if let Some(t) = o {
                        current.domain.push(term_lexical(&t));
                    }
                }
                P_RANGE => {
                    if let Some(t) = o {
                        current.range.push(term_lexical(&t));
                    }
                }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        current.quality_score = l.value().parse().ok();
                    }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o {
                        current.authority_score = l.value().parse().ok();
                    }
                }
                P_SOURCE_FILE => {
                    if let Some(t) = o {
                        current.source_file = Some(term_lexical(&t));
                    }
                }
                _ => {}
            }
        }
        flush(&mut current, &mut classified, &mut props);
        Ok(props)
    }

    // ------------------------------------------------------------------
    // Aggregate accessors
    // ------------------------------------------------------------------

    async fn get_classes(&self) -> RepoResult<Vec<OwlClass>> {
        self.list_owl_classes().await
    }

    async fn get_axioms(&self) -> RepoResult<Vec<OwlAxiom>> {
        // ADR-098 / PRD-018 fix: the canonical ingest (matching the Turtle
        // `vault build` emits — `crates/vault/src/build/turtle.rs`, which
        // replaced the retired logseq `jsonld_to_turtle.py` converter) writes
        // the OWL structure as
        // PLAIN triples in the assert graph — `<C> rdfs:subClassOf <D>`,
        // `<C> owl:equivalentClass <D>`, `<C> owl:disjointWith <D>`, the
        // mereological `<C> vc:hasPart|isPartOf <D>` object properties, and
        // existential restrictions `<C> rdfs:subClassOf [owl:onProperty P;
        // owl:someValuesFrom D]`. It does NOT reify them as `vc:Axiom`
        // instances. Reading only reified `vc:Axiom` (the historical behaviour)
        // therefore returned 0 class axioms even with 5k+ subClassOf triples in
        // the store, starving Whelk and the GPU constraint mapper. This query
        // reads the plain structural triples AND keeps the reified path for any
        // axioms minted via `add_axiom`.
        let q = format!(
            "{PROLOGUE}\
             SELECT ?type ?subject ?object ?predicate ?id\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{\n\
               {{\n\
                 ?axiom a vc:Axiom ;\n\
                        vc:axiomType ?type ;\n\
                        vc:subject ?subject ;\n\
                        vc:object ?object .\n\
                 OPTIONAL {{ ?axiom vc:axiomId ?id }}\n\
                 OPTIONAL {{ ?axiom vc:onProperty ?predicate }}\n\
               }} UNION {{\n\
                 ?subject rdfs:subClassOf ?object .\n\
                 FILTER(isIRI(?object))\n\
                 FILTER(?object != owl:Thing && ?object != owl:Nothing)\n\
                 BIND(\"SubClassOf\" AS ?type)\n\
               }} UNION {{\n\
                 ?subject owl:equivalentClass ?object .\n\
                 FILTER(isIRI(?object))\n\
                 BIND(\"EquivalentClass\" AS ?type)\n\
               }} UNION {{\n\
                 ?subject owl:disjointWith ?object .\n\
                 FILTER(isIRI(?object))\n\
                 BIND(\"DisjointWith\" AS ?type)\n\
               }} UNION {{\n\
                 ?subject ?predicate ?object .\n\
                 FILTER(isIRI(?object))\n\
                 FILTER(?predicate IN (vc:hasPart, vc:isPartOf, vc:partOf, owl:sameAs))\n\
                 BIND(\"ObjectPropertyAssertion\" AS ?type)\n\
               }} UNION {{\n\
                 ?subject rdfs:subClassOf ?restriction .\n\
                 ?restriction a owl:Restriction ;\n\
                              owl:onProperty ?predicate ;\n\
                              owl:someValuesFrom ?object .\n\
                 FILTER(isIRI(?object))\n\
                 BIND(\"SomeValuesFrom\" AS ?type)\n\
               }}\n\
             }}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        let mut out: Vec<OwlAxiom> = Vec::with_capacity(rows.len());
        for row in rows {
            // SELECT order: 0=type 1=subject 2=object 3=predicate 4=id
            let axiom_type = match &row[0] {
                Some(Term::Literal(l)) => parse_axiom_type(l.value()),
                _ => AxiomType::SubClassOf,
            };
            let subject = row[1].as_ref().map(term_lexical).unwrap_or_default();
            let object = row[2].as_ref().map(term_lexical).unwrap_or_default();
            let id = match &row[4] {
                Some(Term::Literal(l)) => l.value().parse::<u64>().ok(),
                _ => None,
            };
            // The object-property predicate (hasPart/isPartOf/…) and the
            // restriction's onProperty are surfaced under "predicate" so the
            // constraint mapper (ADR-098) can classify the force kind and the
            // Whelk adapter can build the existential restriction.
            let mut annotations = HashMap::new();
            if let Some(Term::NamedNode(p)) = &row[3] {
                annotations.insert("predicate".to_string(), p.as_str().to_string());
                // Whelk's SomeValuesFrom translation reads the restriction's
                // onProperty under "property"; keep both keys in sync so the
                // EL existential restriction is built with the right property.
                annotations.insert("property".to_string(), p.as_str().to_string());
            }
            out.push(OwlAxiom {
                id,
                axiom_type,
                subject,
                object,
                annotations,
            });
        }
        Ok(out)
    }

    async fn add_axiom(&self, axiom: &OwlAxiom) -> RepoResult<u64> {
        let id = axiom_id(axiom);
        let body = Self::axiom_insert_block(axiom);
        let update = format!(
            "{PROLOGUE}\
             INSERT DATA {{\n  GRAPH <{GRAPH_ONTOLOGY}> {{\n{body}  }}\n}}\n"
        );
        self.run_update(update).await?;
        Ok(id)
    }

    async fn get_class_axioms(&self, class_iri: &str) -> RepoResult<Vec<OwlAxiom>> {
        let q = format!(
            "{PROLOGUE}\
             SELECT ?axiom ?type ?object ?id\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{\n\
               ?axiom a <{T_VC_AXIOM}> ;\n\
                      <{P_AXIOM_SUBJECT}> <{class_iri}> ;\n\
                      <{P_AXIOM_TYPE}> ?type ;\n\
                      <{P_AXIOM_OBJECT}> ?object .\n\
               OPTIONAL {{ ?axiom <{P_AXIOM_ID}> ?id }}\n\
             }}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let axiom_type = match &row[1] {
                Some(Term::Literal(l)) => parse_axiom_type(l.value()),
                _ => AxiomType::SubClassOf,
            };
            let object = row[2].as_ref().map(term_lexical).unwrap_or_default();
            let id = match &row[3] {
                Some(Term::Literal(l)) => l.value().parse::<u64>().ok(),
                _ => None,
            };
            out.push(OwlAxiom {
                id,
                axiom_type,
                subject: class_iri.to_string(),
                object,
                annotations: HashMap::new(),
            });
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Inference (ADR-11 §D9 — atomic DELETE + INSERT into :inferred)
    // ------------------------------------------------------------------

    async fn store_inference_results(&self, results: &InferenceResults) -> RepoResult<()> {
        // ADR-099 D3: each materialisation is a `prov:Activity` (the whelk run);
        // every inferred quad is `prov:wasGeneratedBy` that run, carries a
        // `vc:derivation "inferred"` marker and a confidence value, so a SPARQL
        // query can cleanly separate asserted from inferred. The run id is
        // content-addressed over the (timestamp, version, axiom count) so the
        // same closure re-materialised is stable, but distinct runs differ.
        let run_id = Self::inference_run_id(results);
        let run_iri = format!("urn:ngm:inference:run:{run_id}");

        let mut body = String::with_capacity(2048);

        // Run activity (provenance anchor).
        body.push_str(&format!("<{run_iri}> a <{T_PROV_ACTIVITY}> .\n"));
        body.push_str(&format!(
            "<{run_iri}> <{P_PROV_ENDED_AT}> \"{}\"^^xsd:dateTime .\n",
            results.timestamp.to_rfc3339()
        ));
        body.push_str(&format!(
            "<{run_iri}> <{P_INFERRED_VERSION}> \"{}\" .\n",
            escape_literal(&results.reasoner_version)
        ));

        // Metadata triples on the sentinel IRI (unchanged read contract) plus a
        // pointer to the current run.
        body.push_str(&format!(
            "<{INFER_META_IRI}> <{P_INFERRED_AT}> \"{}\"^^xsd:dateTime .\n",
            results.timestamp.to_rfc3339()
        ));
        body.push_str(&format!(
            "<{INFER_META_IRI}> <{P_INFERRED_TIME_MS}> \"{}\"^^xsd:integer .\n",
            results.inference_time_ms
        ));
        body.push_str(&format!(
            "<{INFER_META_IRI}> <{P_INFERRED_VERSION}> \"{}\" .\n",
            escape_literal(&results.reasoner_version)
        ));
        body.push_str(&format!(
            "<{INFER_META_IRI}> <{P_RUN_ID}> \"{}\" .\n",
            escape_literal(&run_id)
        ));
        body.push_str(&format!(
            "<{INFER_META_IRI}> <{P_PROV_GENERATED_BY}> <{run_iri}> .\n"
        ));

        for axiom in &results.inferred_axioms {
            // Reified axiom record (existing read path) ...
            body.push_str(&Self::axiom_insert_block(axiom));
            let iri = axiom_iri(axiom);
            // ... plus provenance + derivation marker on the axiom IRI.
            body.push_str(&format!("<{iri}> <{P_PROV_GENERATED_BY}> <{run_iri}> .\n"));
            body.push_str(&format!(
                "<{iri}> <{P_DERIVATION}> \"{DERIVATION_INFERRED}\" .\n"
            ));
            // EL closure is sound: confidence is 1.0. The marker exists so a
            // future probabilistic path can vary it without a schema change.
            body.push_str(&format!(
                "<{iri}> <{P_CONFIDENCE}> \"1.0\"^^xsd:decimal .\n"
            ));
            // Materialise the *direct* RDF relation so the inferred graph is a
            // first-class graph (rdfs:subClassOf / owl:equivalentClass), not
            // only reified axiom records (ADR-099 D2/D3).
            if axiom.subject.starts_with("urn:") || axiom.subject.starts_with("http") {
                if axiom.object.starts_with("urn:") || axiom.object.starts_with("http") {
                    match axiom.axiom_type {
                        AxiomType::SubClassOf => {
                            body.push_str(&format!(
                                "<{}> <{RDFS_SUBCLASS_OF}> <{}> .\n",
                                axiom.subject, axiom.object
                            ));
                        }
                        AxiomType::EquivalentClass => {
                            // Bidirectional materialisation (ADR-099 D2).
                            body.push_str(&format!(
                                "<{}> <{OWL_EQUIVALENT_CLASS}> <{}> .\n",
                                axiom.subject, axiom.object
                            ));
                            body.push_str(&format!(
                                "<{}> <{RDFS_SUBCLASS_OF}> <{}> .\n",
                                axiom.subject, axiom.object
                            ));
                            body.push_str(&format!(
                                "<{}> <{RDFS_SUBCLASS_OF}> <{}> .\n",
                                axiom.object, axiom.subject
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        // Single SPARQL Update with two statements separated by `;` —
        // Oxigraph commits them atomically per ADR-11 §D9. The CLEAR confines
        // re-materialisation to the inferred graph alone (ADR-099 D3).
        let update = format!(
            "{PROLOGUE}\
             DELETE {{ GRAPH <{GRAPH_ONTOLOGY_INFERRED}> {{ ?s ?p ?o }} }}\n\
             WHERE  {{ GRAPH <{GRAPH_ONTOLOGY_INFERRED}> {{ ?s ?p ?o }} }} ;\n\
             INSERT DATA {{ GRAPH <{GRAPH_ONTOLOGY_INFERRED}> {{\n{body}  }} }}\n"
        );
        self.run_update(update).await?;

        // PRD-022 WS-2: reify the reasoner run as an append-only PROV-O activity
        // in `GRAPH_PROVENANCE` so the inference is queryable through the trust
        // ledger. This is an audit side-effect over an already-committed
        // materialisation — a provenance failure is logged, never propagated
        // back to the caller. Routed through the single `emit_provenance` seam.
        let record = crate::provenance_emitter::ActivityRecord {
            activity_urn: format!("urn:visionclaw:execution:inference-{run_id}"),
            agent_did: REASONER_AGENT_DID.to_string(),
            timestamp: results.timestamp.to_rfc3339(),
            action: "infer".to_string(),
            derivation: DERIVATION_INFERRED.to_string(),
            used: Some(GRAPH_ONTOLOGY.to_string()),
            generated: Some(GRAPH_ONTOLOGY_INFERRED.to_string()),
            informed_by: None,
        };
        match self.emit_provenance(record).await {
            Ok(n) => tracing::debug!(
                "inference provenance: reified {n} PROV-O triple(s) for run {run_id}"
            ),
            Err(e) => tracing::warn!("inference provenance emit failed (non-fatal): {e}"),
        }
        Ok(())
    }

    async fn get_inference_results(&self) -> RepoResult<Option<InferenceResults>> {
        // Read metadata triples first.
        let meta_q = format!(
            "{PROLOGUE}\
             SELECT ?p ?o\n\
             FROM <{GRAPH_ONTOLOGY_INFERRED}>\n\
             WHERE {{ <{INFER_META_IRI}> ?p ?o }}\n"
        );
        let (_v, meta_rows) = self.run_select(meta_q).await?;
        if meta_rows.is_empty() {
            return Ok(None);
        }
        let mut timestamp: Option<chrono::DateTime<chrono::Utc>> = None;
        let mut time_ms: u64 = 0;
        let mut version = String::new();
        for row in meta_rows {
            let p = match &row[0] {
                Some(Term::NamedNode(p)) => p.as_str().to_string(),
                _ => continue,
            };
            let o = row[1].clone();
            match p.as_str() {
                P_INFERRED_AT => {
                    if let Some(Term::Literal(l)) = o {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(l.value()) {
                            timestamp = Some(dt.with_timezone(&chrono::Utc));
                        }
                    }
                }
                P_INFERRED_TIME_MS => {
                    if let Some(Term::Literal(l)) = o {
                        time_ms = l.value().parse().unwrap_or(0);
                    }
                }
                P_INFERRED_VERSION => {
                    if let Some(Term::Literal(l)) = o {
                        version = l.value().to_string();
                    }
                }
                _ => {}
            }
        }

        // Then collect the inferred axioms.
        let axioms_q = format!(
            "{PROLOGUE}\
             SELECT ?axiom ?type ?subject ?object ?id\n\
             FROM <{GRAPH_ONTOLOGY_INFERRED}>\n\
             WHERE {{\n\
               ?axiom a <{T_VC_AXIOM}> ;\n\
                      <{P_AXIOM_TYPE}> ?type ;\n\
                      <{P_AXIOM_SUBJECT}> ?subject ;\n\
                      <{P_AXIOM_OBJECT}> ?object .\n\
               OPTIONAL {{ ?axiom <{P_AXIOM_ID}> ?id }}\n\
             }}\n"
        );
        let (_v, ax_rows) = self.run_select(axioms_q).await?;
        let mut axioms = Vec::with_capacity(ax_rows.len());
        for row in ax_rows {
            let axiom_type = match &row[1] {
                Some(Term::Literal(l)) => parse_axiom_type(l.value()),
                _ => AxiomType::SubClassOf,
            };
            let subject = row[2].as_ref().map(term_lexical).unwrap_or_default();
            let object = row[3].as_ref().map(term_lexical).unwrap_or_default();
            let id = match &row[4] {
                Some(Term::Literal(l)) => l.value().parse::<u64>().ok(),
                _ => None,
            };
            axioms.push(OwlAxiom {
                id,
                axiom_type,
                subject,
                object,
                annotations: HashMap::new(),
            });
        }

        Ok(Some(InferenceResults {
            timestamp: timestamp.unwrap_or_else(chrono::Utc::now),
            inferred_axioms: axioms,
            inference_time_ms: time_ms,
            reasoner_version: version,
        }))
    }

    async fn validate_ontology(&self) -> RepoResult<ValidationReport> {
        // ADR-11 §D6 — UNIQUE-style constraint battery. Each ASK that
        // returns true contributes one entry to errors.
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // 1. Duplicate OntologyClass IRI (impossible in RDF, but a class
        //    declared twice with both `:OntologyClass` and `:OwlClass`
        //    types is a smell — flag as warning).
        let dup_class_q = format!(
            "{PROLOGUE}\
             SELECT (COUNT(?s) AS ?n)\n\
             WHERE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ?s a <{T_VC_ONTOLOGY_CLASS}> . ?s a <{T_OWL_CLASS}> }} }}\n"
        );
        let (_v, rows) = self.run_select(dup_class_q).await?;
        // (info — not an error)
        if let Some(row) = rows.first() {
            if let Some(Term::Literal(l)) = &row[0] {
                let count: u64 = l.value().parse().unwrap_or(0);
                if count > 0 {
                    warnings.push(format!(
                        "{count} class(es) declared as both vc:OntologyClass and owl:Class"
                    ));
                }
            }
        }

        // 2. Axioms with subject IRI not present in the assert graph.
        let dangling_subj_q = format!(
            "{PROLOGUE}\
             ASK FROM <{GRAPH_ONTOLOGY}> {{\n\
               ?a a <{T_VC_AXIOM}> ; <{P_AXIOM_SUBJECT}> ?s .\n\
               FILTER(isIRI(?s))\n\
               FILTER NOT EXISTS {{ ?s ?p ?o }}\n\
             }}\n"
        );
        if self.run_ask(dangling_subj_q).await? {
            errors.push("axiom with dangling subject IRI".to_string());
        }

        // 3. Axioms with unrecognised axiom_type.
        let bad_type_q = format!(
            "{PROLOGUE}\
             ASK FROM <{GRAPH_ONTOLOGY}> {{\n\
               ?a a <{T_VC_AXIOM}> ; <{P_AXIOM_TYPE}> ?t .\n\
               FILTER(?t NOT IN (\"SubClassOf\", \"EquivalentClass\", \"DisjointWith\", \"ObjectPropertyAssertion\", \"DataPropertyAssertion\", \"SubPropertyOf\", \"TransitiveProperty\", \"SymmetricProperty\", \"InverseProperties\", \"SomeValuesFrom\"))\n\
             }}\n"
        );
        if self.run_ask(bad_type_q).await? {
            errors.push("axiom with unrecognised axiom_type".to_string());
        }

        // 4. Property without a recognised rdf:type.
        let bad_prop_q = format!(
            "{PROLOGUE}\
             ASK FROM <{GRAPH_ONTOLOGY}> {{\n\
               ?p <{P_DOMAIN}> ?d .\n\
               FILTER NOT EXISTS {{\n\
                 ?p a ?t .\n\
                 FILTER(?t IN (<{T_OWL_OBJECT_PROP}>, <{T_OWL_DATA_PROP}>, <{T_OWL_ANNOT_PROP}>))\n\
               }}\n\
             }}\n"
        );
        if self.run_ask(bad_prop_q).await? {
            warnings.push("property with rdfs:domain but no rdf:type".to_string());
        }

        // 5. Class without rdfs:label (warning, not error).
        let no_label_q = format!(
            "{PROLOGUE}\
             ASK FROM <{GRAPH_ONTOLOGY}> {{\n\
               ?c a <{T_VC_ONTOLOGY_CLASS}> .\n\
               FILTER NOT EXISTS {{ ?c <{P_LABEL}> ?l }}\n\
             }}\n"
        );
        if self.run_ask(no_label_q).await? {
            warnings.push("at least one OntologyClass missing rdfs:label".to_string());
        }

        let is_valid = errors.is_empty();
        Ok(ValidationReport {
            is_valid,
            errors,
            warnings,
            timestamp: chrono::Utc::now(),
        })
    }

    async fn query_ontology(&self, query: &str) -> RepoResult<Vec<HashMap<String, String>>> {
        let q = query.to_string();
        let (vars, rows) = self.run_select(q).await?;
        let mut out: Vec<HashMap<String, String>> = Vec::with_capacity(rows.len());
        for row in rows {
            let mut m = HashMap::with_capacity(vars.len());
            for (i, v) in vars.iter().enumerate() {
                if let Some(Some(t)) = row.get(i) {
                    m.insert(v.clone(), term_lexical(t));
                }
            }
            out.push(m);
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    async fn remove_owl_class(&self, iri: &str) -> RepoResult<()> {
        // Two-statement update: drop the class's own triples, then any
        // axiom whose subject is the removed class.
        let update = format!(
            "{PROLOGUE}\
             DELETE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ <{iri}> ?p ?o }} }}\n\
             WHERE  {{ GRAPH <{GRAPH_ONTOLOGY}> {{ <{iri}> ?p ?o }} }} ;\n\
             DELETE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ?a ?p ?o }} }}\n\
             WHERE  {{ GRAPH <{GRAPH_ONTOLOGY}> {{\n\
               ?a a <{T_VC_AXIOM}> ;\n\
                  <{P_AXIOM_SUBJECT}> <{iri}> ;\n\
                  ?p ?o .\n\
             }} }}\n"
        );
        self.run_update(update).await
    }

    async fn remove_axiom(&self, axiom_id: u64) -> RepoResult<()> {
        let update = format!(
            "{PROLOGUE}\
             DELETE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ?a ?p ?o }} }}\n\
             WHERE  {{ GRAPH <{GRAPH_ONTOLOGY}> {{\n\
               ?a a <{T_VC_AXIOM}> ;\n\
                  <{P_AXIOM_ID}> \"{axiom_id}\"^^xsd:integer ;\n\
                  ?p ?o .\n\
             }} }}\n"
        );
        self.run_update(update).await
    }

    // ------------------------------------------------------------------
    // Metrics
    // ------------------------------------------------------------------

    async fn get_metrics(&self) -> RepoResult<OntologyMetrics> {
        // SPARQL 1.1 with aggregates cannot mix `FROM <iri>` (dataset clause)
        // with `SELECT (COUNT…)`. Oxigraph's strict parser rejects it. The
        // canonical form scopes the pattern via `GRAPH <iri> { … }` inside
        // WHERE instead.
        //
        // 1. Class count.
        let class_q = format!(
            "{PROLOGUE}\
             SELECT (COUNT(?s) AS ?n)\n\
             WHERE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ?s a <{T_VC_ONTOLOGY_CLASS}> }} }}\n"
        );
        let class_count = scalar_count(&self.run_select(class_q).await?).unwrap_or(0);

        // 2. Property count.
        let prop_q = format!(
            "{PROLOGUE}\
             SELECT (COUNT(?s) AS ?n)\n\
             WHERE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ?s a ?t .\n\
                      FILTER(?t IN (<{T_OWL_OBJECT_PROP}>, <{T_OWL_DATA_PROP}>, <{T_OWL_ANNOT_PROP}>)) }} }}\n"
        );
        let property_count = scalar_count(&self.run_select(prop_q).await?).unwrap_or(0);

        // 3. Axiom count.
        let ax_q = format!(
            "{PROLOGUE}\
             SELECT (COUNT(?s) AS ?n)\n\
             WHERE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ?s a <{T_VC_AXIOM}> }} }}\n"
        );
        let axiom_count = scalar_count(&self.run_select(ax_q).await?).unwrap_or(0);

        // 4. Branching factor: average number of direct sub-classes per
        //    parent. COUNT children grouped by parent, then average.
        let branch_q = format!(
            "{PROLOGUE}\
             SELECT (AVG(?children) AS ?avg) WHERE {{\n\
               {{ SELECT ?parent (COUNT(?child) AS ?children)\n\
                 WHERE {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ?child <{P_SUBCLASS_OF}> ?parent }} }}\n\
                 GROUP BY ?parent }}\n\
             }}\n"
        );
        let avg_branching = scalar_f32(&self.run_select(branch_q).await?).unwrap_or(0.0);

        // 5. Max depth: iterative ASK loop. SPARQL property paths give
        //    reachability via `subClassOf+` but not depth. We walk
        //    increasing explicit-hop chains until ASK returns false.
        //    Capped at MAX_DEPTH_CAP to bound runtime on cyclic data;
        //    cycles in subClassOf are a constraint violation anyway.
        const MAX_DEPTH_CAP: usize = 64;
        let mut max_depth: usize = 0;
        for depth in 1..=MAX_DEPTH_CAP {
            let mut chained = String::new();
            for k in 0..depth {
                chained.push_str(&format!("?n{k} <{P_SUBCLASS_OF}> ?n{} .\n", k + 1));
            }
            let ask = format!(
                "{PROLOGUE}\
                 ASK FROM <{GRAPH_ONTOLOGY}> {{\n{chained}\
                 }}\n"
            );
            if self.run_ask(ask).await? {
                max_depth = depth;
            } else {
                break;
            }
        }

        Ok(OntologyMetrics {
            class_count: class_count as usize,
            property_count: property_count as usize,
            axiom_count: axiom_count as usize,
            max_depth,
            average_branching_factor: avg_branching,
        })
    }

    // ------------------------------------------------------------------
    // Pathfinding cache (override the trait defaults so cache lives in
    // dedicated named graphs and can be invalidated atomically).
    // ------------------------------------------------------------------

    async fn cache_sssp_result(&self, entry: &PathfindingCacheEntry) -> RepoResult<()> {
        let iri = format!("urn:ngm:pathcache:sssp:{}", entry.source_node_id);
        let distances = serde_json::to_string(&entry.distances)
            .map_err(|e| OntologyRepositoryError::SerializationError(e.to_string()))?;
        let paths = serde_json::to_string(&entry.paths)
            .map_err(|e| OntologyRepositoryError::SerializationError(e.to_string()))?;
        let mut body = String::with_capacity(1024);
        body.push_str(&format!(
            "<{iri}> <{P_CACHE_COMPUTED_AT}> \"{}\"^^xsd:dateTime .\n",
            entry.computed_at.to_rfc3339()
        ));
        body.push_str(&format!(
            "<{iri}> <{P_CACHE_COMP_TIME}> \"{}\"^^xsd:float .\n",
            entry.computation_time_ms
        ));
        body.push_str(&format!(
            "<{iri}> <{P_CACHE_DISTANCES}> \"{}\" .\n",
            escape_literal(&distances)
        ));
        body.push_str(&format!(
            "<{iri}> <{P_CACHE_PATHS}> \"{}\" .\n",
            escape_literal(&paths)
        ));
        if let Some(tgt) = entry.target_node_id {
            body.push_str(&format!(
                "<{iri}> <{P_CACHE_TARGET}> \"{}\"^^xsd:integer .\n",
                tgt
            ));
        }
        // Refresh the entry: delete prior entry for this source then insert.
        let update = format!(
            "{PROLOGUE}\
             DELETE {{ GRAPH <{GRAPH_CACHE_SSSP}> {{ <{iri}> ?p ?o }} }}\n\
             WHERE  {{ GRAPH <{GRAPH_CACHE_SSSP}> {{ <{iri}> ?p ?o }} }} ;\n\
             INSERT DATA {{ GRAPH <{GRAPH_CACHE_SSSP}> {{\n{body}  }} }}\n"
        );
        self.run_update(update).await
    }

    async fn get_cached_sssp(
        &self,
        source_node_id: u32,
    ) -> RepoResult<Option<PathfindingCacheEntry>> {
        let iri = format!("urn:ngm:pathcache:sssp:{}", source_node_id);
        let q = format!(
            "{PROLOGUE}\
             SELECT ?p ?o\n\
             FROM <{GRAPH_CACHE_SSSP}>\n\
             WHERE {{ <{iri}> ?p ?o }}\n"
        );
        let (_v, rows) = self.run_select(q).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        let mut distances: Vec<f32> = Vec::new();
        let mut paths: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut computed_at: Option<chrono::DateTime<chrono::Utc>> = None;
        let mut computation_time_ms: f32 = 0.0;
        let mut target_node_id: Option<u32> = None;
        for row in rows {
            let p = match &row[0] {
                Some(Term::NamedNode(p)) => p.as_str().to_string(),
                _ => continue,
            };
            let o = row[1].clone();
            match p.as_str() {
                P_CACHE_COMPUTED_AT => {
                    if let Some(Term::Literal(l)) = o {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(l.value()) {
                            computed_at = Some(dt.with_timezone(&chrono::Utc));
                        }
                    }
                }
                P_CACHE_COMP_TIME => {
                    if let Some(Term::Literal(l)) = o {
                        computation_time_ms = l.value().parse().unwrap_or(0.0);
                    }
                }
                P_CACHE_DISTANCES => {
                    if let Some(Term::Literal(l)) = o {
                        if let Ok(parsed) = serde_json::from_str::<Vec<f32>>(l.value()) {
                            distances = parsed;
                        }
                    }
                }
                P_CACHE_PATHS => {
                    if let Some(Term::Literal(l)) = o {
                        if let Ok(parsed) =
                            serde_json::from_str::<HashMap<u32, Vec<u32>>>(l.value())
                        {
                            paths = parsed;
                        }
                    }
                }
                P_CACHE_TARGET => {
                    if let Some(Term::Literal(l)) = o {
                        target_node_id = l.value().parse().ok();
                    }
                }
                _ => {}
            }
        }
        Ok(Some(PathfindingCacheEntry {
            source_node_id,
            target_node_id,
            distances,
            paths,
            computed_at: computed_at.unwrap_or_else(chrono::Utc::now),
            computation_time_ms,
        }))
    }

    async fn cache_apsp_result(&self, distance_matrix: &Vec<Vec<f32>>) -> RepoResult<()> {
        let matrix_json = serde_json::to_string(distance_matrix)
            .map_err(|e| OntologyRepositoryError::SerializationError(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        let update = format!(
            "{PROLOGUE}\
             CLEAR GRAPH <{GRAPH_CACHE_APSP}> ;\n\
             INSERT DATA {{ GRAPH <{GRAPH_CACHE_APSP}> {{\n\
               <{APSP_IRI}> <{P_CACHE_COMPUTED_AT}> \"{now}\"^^xsd:dateTime .\n\
               <{APSP_IRI}> <{P_CACHE_MATRIX}> \"{}\" .\n\
             }} }}\n",
            escape_literal(&matrix_json)
        );
        self.run_update(update).await
    }

    async fn get_cached_apsp(&self) -> RepoResult<Option<Vec<Vec<f32>>>> {
        let q = format!(
            "{PROLOGUE}\
             SELECT ?matrix\n\
             FROM <{GRAPH_CACHE_APSP}>\n\
             WHERE {{ <{APSP_IRI}> <{P_CACHE_MATRIX}> ?matrix }}\n"
        );
        let (_v, rows) = self.run_select(q).await?;
        if rows.is_empty() {
            return Ok(None);
        }
        match &rows[0][0] {
            Some(Term::Literal(l)) => {
                let parsed = serde_json::from_str::<Vec<Vec<f32>>>(l.value())
                    .map_err(|e| OntologyRepositoryError::DeserializationError(e.to_string()))?;
                Ok(Some(parsed))
            }
            _ => Ok(None),
        }
    }

    async fn invalidate_pathfinding_caches(&self) -> RepoResult<()> {
        let update = format!(
            "{PROLOGUE}\
             CLEAR GRAPH <{GRAPH_CACHE_SSSP}> ;\n\
             CLEAR GRAPH <{GRAPH_CACHE_APSP}>\n"
        );
        self.run_update(update).await
    }
}

// ----------------------------------------------------------------------
// Scalar helpers for COUNT/AVG result rows.
// ----------------------------------------------------------------------

fn scalar_count(result: &(Vec<String>, Vec<Vec<Option<Term>>>)) -> Option<u64> {
    let (_vars, rows) = result;
    rows.first().and_then(|row| {
        row.first().and_then(|cell| match cell {
            Some(Term::Literal(l)) => l.value().parse::<u64>().ok(),
            _ => None,
        })
    })
}

fn scalar_f32(result: &(Vec<String>, Vec<Vec<Option<Term>>>)) -> Option<f32> {
    let (_vars, rows) = result;
    rows.first().and_then(|row| {
        row.first().and_then(|cell| match cell {
            Some(Term::Literal(l)) => l.value().parse::<f32>().ok(),
            _ => None,
        })
    })
}

// Silence unused-import lint warnings for items only referenced in
// helpers above (kept here so the imports list stays canonical).
#[allow(dead_code)]
fn _force_imports() -> (HashSet<u32>, OntologyRepositoryError) {
    (HashSet::new(), OntologyRepositoryError::NotFound)
}

/// `true` when `iri` can be written between `<…>` in a SPARQL update without
/// escaping: non-empty, absolute (`scheme:`), and free of whitespace and the
/// characters the IRIREF production forbids.
fn is_insertable_iri(iri: &str) -> bool {
    !iri.is_empty()
        && iri.contains(':')
        && !iri.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '<' | '>' | '"' | '{' | '}' | '|' | '^' | '`' | '\\')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::ports::ontology_repository::OntologyRepository;
    use visionclaw_domain::ports::ontology_repository::{AxiomType, OwlAxiom};
    use visionclaw_domain::ports::owl_types::OwlClass;

    fn in_memory_repo() -> OxigraphOntologyRepository {
        OxigraphOntologyRepository::from_store(Arc::new(Store::new().unwrap()))
    }

    // ---- WS-1: real SHACL shape loading ----------------------------------

    #[test]
    fn load_shacl_shapes_populates_shapes_graph() {
        let store = Store::new().unwrap();
        // Before loading, the catalogue carries no NodeShapes.
        assert_eq!(
            crate::provenance_emitter::count_shapes_loaded(&store).unwrap(),
            0
        );

        let n = load_shacl_shapes(&store).expect("shapes load");
        // All five embedded shape files declare at least one NodeShape.
        assert!(n >= 5, "expected >=5 NodeShapes loaded, got {n}");
        assert_eq!(
            crate::provenance_emitter::count_shapes_loaded(&store).unwrap(),
            n
        );
        // The content-hash marker is present so a restart is a no-op.
        assert_eq!(
            stored_shapes_hash(&store).as_deref(),
            Some(shacl_shapes_hash().as_str())
        );
    }

    #[test]
    fn load_shacl_shapes_is_idempotent_across_reloads() {
        let store = Store::new().unwrap();
        let first = load_shacl_shapes(&store).expect("first load");

        // A second load with the same corpus hash must not duplicate blank-node
        // property shapes: the total quad count in the shapes graph is stable.
        let graph = oxigraph::model::NamedNodeRef::new_unchecked(GRAPH_SHAPES);
        let before = store
            .quads_for_pattern(None, None, None, Some(graph.into()))
            .count();
        let second = load_shacl_shapes(&store).expect("second load");
        let after = store
            .quads_for_pattern(None, None, None, Some(graph.into()))
            .count();

        assert_eq!(first, second, "NodeShape count stable across reloads");
        assert_eq!(before, after, "reload must not duplicate shape triples");
    }

    // ---- WS-0: SPARQL LIMIT clamp ----------------------------------------

    #[test]
    fn clamp_appends_limit_when_absent() {
        let q = "SELECT ?s ?p ?o WHERE { ?s ?p ?o }";
        let out = clamp_select_limit(q);
        assert!(
            out.ends_with(&format!("LIMIT {}", DEFAULT_SPARQL_LIMIT)),
            "got: {out}"
        );
    }

    #[test]
    fn clamp_rewrites_oversized_limit() {
        let q = "SELECT ?s WHERE { ?s ?p ?o } LIMIT 999999";
        let out = clamp_select_limit(q);
        assert!(
            out.contains(&format!("LIMIT {}", MAX_SPARQL_ROWS)),
            "got: {out}"
        );
        assert!(
            !out.contains("999999"),
            "oversized limit not removed: {out}"
        );
    }

    #[test]
    fn clamp_preserves_small_limit() {
        let q = "SELECT ?s WHERE { ?s ?p ?o } LIMIT 50";
        assert_eq!(clamp_select_limit(q), q);
    }

    #[test]
    fn clamp_passes_through_ask() {
        let q = "ASK { ?s ?p ?o }";
        assert_eq!(clamp_select_limit(q), q);
    }

    fn make_class(iri: &str, label: &str) -> OwlClass {
        OwlClass {
            iri: iri.to_string(),
            label: Some(label.to_string()),
            ..Default::default()
        }
    }

    fn make_axiom(sub: &str, sup: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::SubClassOf,
            subject: sub.to_string(),
            object: sup.to_string(),
            annotations: std::collections::HashMap::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn list_classes_on_empty_store_returns_empty() {
        let repo = in_memory_repo();
        let classes = repo.list_owl_classes().await.unwrap();
        assert!(classes.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn add_and_get_class_round_trips() {
        let repo = in_memory_repo();
        let c = make_class("urn:test:Dog", "Dog");
        let iri = repo.add_owl_class(&c).await.unwrap();
        assert!(!iri.is_empty());

        let fetched = repo.get_owl_class(&iri).await.unwrap();
        assert!(
            fetched.is_some(),
            "get_owl_class should return the saved class"
        );
        let fetched = fetched.unwrap();
        assert_eq!(fetched.iri, iri);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn save_ontology_then_list_classes_round_trips() {
        let repo = in_memory_repo();
        let classes = vec![
            make_class("urn:test:Animal", "Animal"),
            make_class("urn:test:Dog", "Dog"),
        ];
        let axioms = vec![make_axiom("urn:test:Dog", "urn:test:Animal")];
        repo.save_ontology(&classes, &[], &axioms).await.unwrap();

        let stored = repo.list_owl_classes().await.unwrap();
        assert!(
            stored.len() >= 2,
            "Expected at least 2 classes, got {}",
            stored.len()
        );
        let iris: Vec<&str> = stored.iter().map(|c| c.iri.as_str()).collect();
        assert!(iris.contains(&"urn:test:Animal"), "Missing Animal");
        assert!(iris.contains(&"urn:test:Dog"), "Missing Dog");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_axioms_returns_saved_axioms() {
        let repo = in_memory_repo();
        let axioms = vec![make_axiom("urn:test:Cat", "urn:test:Animal")];
        let classes = vec![
            make_class("urn:test:Animal", "Animal"),
            make_class("urn:test:Cat", "Cat"),
        ];
        repo.save_ontology(&classes, &[], &axioms).await.unwrap();

        let stored_axioms = repo.get_axioms().await.unwrap();
        assert!(
            stored_axioms
                .iter()
                .any(|a| a.subject == "urn:test:Cat" && a.object == "urn:test:Animal"),
            "Expected Cat subClassOf Animal; got {:?}",
            stored_axioms
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remove_owl_class_removes_it() {
        let repo = in_memory_repo();
        let iri = repo
            .add_owl_class(&make_class("urn:test:Temp", "Temp"))
            .await
            .unwrap();
        repo.remove_owl_class(&iri).await.unwrap();
        let fetched = repo.get_owl_class(&iri).await.unwrap();
        assert!(fetched.is_none(), "Class should have been removed");
    }

    // Fixed: SPARQL aggregate+FROM clause bug in get_metrics(). Now uses
    // `WHERE { GRAPH <iri> { … } }` inside the WHERE block instead of the
    // `FROM <iri>` dataset clause, which Oxigraph's strict 1.1 parser
    // accepts with aggregates.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_metrics_returns_non_negative_counts() {
        let repo = in_memory_repo();
        repo.add_owl_class(&make_class("urn:test:X", "X"))
            .await
            .unwrap();
        let metrics = repo.get_metrics().await.unwrap();
        assert!(metrics.class_count >= 1);
    }

    fn inference_results_with(axioms: Vec<OwlAxiom>) -> InferenceResults {
        InferenceResults {
            timestamp: chrono::Utc::now(),
            inferred_axioms: axioms,
            inference_time_ms: 7,
            reasoner_version: "whelk-rs-test".to_string(),
        }
    }

    fn equiv(a: &str, b: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::EquivalentClass,
            subject: a.to_string(),
            object: b.to_string(),
            annotations: std::collections::HashMap::new(),
        }
    }

    /// ADR-099 D3: inferred quads land in the inferred named graph and carry
    /// provenance (`prov:wasGeneratedBy` a run) + a derivation marker.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inferred_quads_land_in_inferred_graph_with_provenance() {
        let repo = in_memory_repo();
        let results = inference_results_with(vec![make_axiom("urn:test:A", "urn:test:B")]);
        repo.store_inference_results(&results).await.unwrap();

        // The direct rdfs:subClassOf triple is in the inferred graph.
        let json = repo
            .sparql_select_json(format!(
                "ASK FROM <{GRAPH_ONTOLOGY_INFERRED}> {{ <urn:test:A> <{RDFS_SUBCLASS_OF}> <urn:test:B> }}"
            ))
            .await
            .unwrap();
        assert_eq!(
            json["boolean"],
            serde_json::Value::Bool(true),
            "subClassOf must be materialised: {json}"
        );

        // Every axiom record is wasGeneratedBy a prov:Activity and marked inferred.
        let prov = repo
            .sparql_select_json(format!(
                "ASK FROM <{GRAPH_ONTOLOGY_INFERRED}> {{ \
                   ?ax <{P_PROV_GENERATED_BY}> ?run . \
                   ?run a <{T_PROV_ACTIVITY}> . \
                   ?ax <{P_DERIVATION}> \"{DERIVATION_INFERRED}\" }}"
            ))
            .await
            .unwrap();
        assert_eq!(
            prov["boolean"],
            serde_json::Value::Bool(true),
            "provenance + derivation must be present: {prov}"
        );

        // read_inferred_graph exposes the runId + triples for the panel.
        let g = repo.read_inferred_graph().await.unwrap();
        assert_eq!(g["namedGraph"], GRAPH_ONTOLOGY_INFERRED);
        assert!(g["runId"].is_string(), "runId surfaced: {g}");
        assert!(g["triples"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false));
    }

    /// PRD-022 WS-2: a completed inference run is reified as an append-only
    /// PROV-O activity in GRAPH_PROVENANCE (the reified emitter's production
    /// caller), queryable through the trust ledger, attributed to the reasoner.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inference_run_reified_into_provenance_graph() {
        let repo = in_memory_repo();
        // Provenance graph starts empty.
        assert_eq!(
            crate::provenance_emitter::count_provenance_triples(repo.store()).unwrap(),
            0
        );

        let results = inference_results_with(vec![make_axiom("urn:test:A", "urn:test:B")]);
        repo.store_inference_results(&results).await.unwrap();

        // The reasoner run is present as a prov:Activity attributed to the
        // system reasoner DID with action="infer" / derivation="inferred".
        let acts =
            crate::provenance_emitter::query_agent_activities(repo.store(), REASONER_AGENT_DID, 10)
                .unwrap();
        assert_eq!(acts.len(), 1, "exactly one inference activity reified");
        assert_eq!(acts[0].action, "infer");
        assert_eq!(acts[0].derivation, "inferred");
        assert_eq!(acts[0].generated.as_deref(), Some(GRAPH_ONTOLOGY_INFERRED));
        assert!(acts[0]
            .activity_urn
            .starts_with("urn:visionclaw:execution:inference-"));
        assert!(
            crate::provenance_emitter::count_provenance_triples(repo.store()).unwrap() >= 5,
            "reified activity writes 5+ provenance triples"
        );
    }

    /// ADR-099 D2: an EquivalentClass inference materialises owl:equivalentClass
    /// AND both rdfs:subClassOf directions — no downgrade in the store.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn equivalent_class_materialised_bidirectionally() {
        let repo = in_memory_repo();
        let results = inference_results_with(vec![equiv("urn:test:A", "urn:test:B")]);
        repo.store_inference_results(&results).await.unwrap();

        let ask = repo
            .sparql_select_json(format!(
                "ASK FROM <{GRAPH_ONTOLOGY_INFERRED}> {{ \
                   <urn:test:A> <{OWL_EQUIVALENT_CLASS}> <urn:test:B> . \
                   <urn:test:A> <{RDFS_SUBCLASS_OF}> <urn:test:B> . \
                   <urn:test:B> <{RDFS_SUBCLASS_OF}> <urn:test:A> }}"
            ))
            .await
            .unwrap();
        assert_eq!(
            ask["boolean"],
            serde_json::Value::Bool(true),
            "equivalence must be bidirectional: {ask}"
        );
    }

    /// ADR-099 D3: "clear inferred" empties only the inferred graph.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn clear_inferred_graph_empties_only_inferred() {
        let repo = in_memory_repo();
        // Seed the assert graph so we can prove it survives.
        repo.add_owl_class(&make_class("urn:test:Survivor", "Survivor"))
            .await
            .unwrap();
        let results = inference_results_with(vec![make_axiom("urn:test:A", "urn:test:B")]);
        repo.store_inference_results(&results).await.unwrap();

        repo.clear_inferred_graph().await.unwrap();

        let g = repo.read_inferred_graph().await.unwrap();
        assert!(
            g["triples"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false),
            "inferred graph must be empty: {g}"
        );

        // Assert graph untouched.
        let classes = repo.list_owl_classes().await.unwrap();
        assert!(
            classes.iter().any(|c| c.iri == "urn:test:Survivor"),
            "assert graph must survive clear"
        );
    }

    /// SPARQL JSON shape: SELECT returns head.vars + results.bindings.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sparql_select_json_has_sparql11_shape() {
        let repo = in_memory_repo();
        repo.add_owl_class(&make_class("urn:test:Q", "Q"))
            .await
            .unwrap();
        let json = repo
            .sparql_select_json("SELECT ?s WHERE { ?s ?p ?o } LIMIT 1".to_string())
            .await
            .unwrap();
        assert!(
            json["head"]["vars"].as_array().is_some(),
            "head.vars present: {json}"
        );
        assert!(
            json["results"]["bindings"].as_array().is_some(),
            "results.bindings present: {json}"
        );
    }

    // ---- ADR-2015: derived write-back fence --------------------------------

    /// Count the quads in one named graph.
    fn graph_len(repo: &OxigraphOntologyRepository, graph: &str) -> usize {
        let g = oxigraph::model::NamedNodeRef::new_unchecked(graph);
        repo.store
            .quads_for_pattern(
                None,
                None,
                None,
                Some(oxigraph::model::GraphNameRef::NamedNode(g)),
            )
            .count()
    }

    fn derived_quads_total(repo: &OxigraphOntologyRepository) -> usize {
        graph_len(repo, GRAPH_ONTOLOGY_SUMMARY) + graph_len(repo, GRAPH_ONTOLOGY_OBSERVED)
    }

    fn quad(g: &str, s: &str, p: &str, o: &str) -> (String, String, String, String) {
        (g.to_string(), s.to_string(), p.to_string(), o.to_string())
    }

    const S: &str = "urn:ngm:class:subject";
    const P: &str = "urn:ngm:property:predicate";

    /// The permitted graphs accept writes, and both commit.
    #[tokio::test]
    async fn permitted_graphs_accept_derived_writes() {
        let repo = in_memory_repo();
        let n = repo
            .append_derived_quads(vec![
                quad(GRAPH_ONTOLOGY_SUMMARY, S, P, "a summary value"),
                quad(GRAPH_ONTOLOGY_OBSERVED, S, P, "an observed value"),
            ])
            .await
            .expect("permitted graphs accept writes");
        assert_eq!(n, 2);
        assert_eq!(graph_len(&repo, GRAPH_ONTOLOGY_SUMMARY), 1);
        assert_eq!(graph_len(&repo, GRAPH_ONTOLOGY_OBSERVED), 1);
    }

    /// ADR-2015: a mixed batch is rejected **whole**. One forbidden graph in a
    /// batch must not let the permitted quads through — the fence is on the
    /// batch, not on the individual quad, so a caller cannot smuggle a write
    /// past it by burying it among legitimate ones.
    #[tokio::test]
    async fn a_mixed_batch_is_rejected_whole() {
        for fenced in DERIVED_FENCE {
            let repo = in_memory_repo();
            let err = repo
                .append_derived_quads(vec![
                    quad(GRAPH_ONTOLOGY_SUMMARY, S, P, "legitimate"),
                    quad(fenced, S, P, "smuggled"),
                    quad(GRAPH_ONTOLOGY_OBSERVED, S, P, "also legitimate"),
                ])
                .await
                .expect_err("a fenced graph anywhere in the batch must reject it");
            assert!(
                format!("{err}").contains("fenced graph"),
                "unexpected error: {err}"
            );
            assert_eq!(
                derived_quads_total(&repo),
                0,
                "the permitted quads in a rejected batch must not be written"
            );
        }
    }

    /// Every fenced graph is refused on its own too.
    #[tokio::test]
    async fn each_fenced_graph_is_refused() {
        for fenced in DERIVED_FENCE {
            let repo = in_memory_repo();
            assert!(repo
                .append_derived_quads(vec![quad(fenced, S, P, "x")])
                .await
                .is_err());
            assert_eq!(graph_len(&repo, fenced), 0);
        }
    }

    /// A graph outside the allow-list is refused even when it is not on the
    /// fence list: the policy is an allow-list, not a deny-list, so a graph
    /// added later is refused by default rather than accepted by omission.
    #[tokio::test]
    async fn an_unlisted_graph_is_refused_by_default() {
        let repo = in_memory_repo();
        for graph in [
            GRAPH_KNOWLEDGE,
            GRAPH_AGENT,
            GRAPH_SHAPES,
            GRAPH_PROVENANCE,
            "urn:ngm:graph:invented:later",
        ] {
            let err = repo
                .append_derived_quads(vec![quad(graph, S, P, "x")])
                .await
                .expect_err("only :summary/:observed are writable here");
            let msg = format!("{err}");
            assert!(
                msg.contains("derived writes only target") || msg.contains("fenced graph"),
                "unexpected error for {graph}: {msg}"
            );
        }
        assert_eq!(derived_quads_total(&repo), 0);
    }

    /// Unsafe subject and predicate IRIs are rejected wherever they appear.
    #[tokio::test]
    async fn unsafe_iris_are_rejected_in_every_position() {
        let hostile = "urn:ngm:class:x> <urn:p> <urn:o> . } INSERT DATA { GRAPH <urn:ngm:graph:ontology:assert> { <urn:evil";
        let cases = [
            (
                "subject",
                quad(GRAPH_ONTOLOGY_SUMMARY, hostile, P, "v"),
                "unsafe subject IRI",
            ),
            (
                "predicate",
                quad(GRAPH_ONTOLOGY_SUMMARY, S, hostile, "v"),
                "unsafe predicate IRI",
            ),
            (
                "object",
                quad(GRAPH_ONTOLOGY_SUMMARY, S, P, hostile),
                "unsafe object IRI",
            ),
        ];
        for (position, bad, expected) in cases {
            let repo = in_memory_repo();
            let err = repo
                .append_derived_quads(vec![quad(GRAPH_ONTOLOGY_SUMMARY, S, P, "legitimate"), bad])
                .await
                .unwrap_err();
            assert!(
                format!("{err}").contains(expected),
                "an unsafe {position} IRI must be rejected with {expected}, got: {err}"
            );
            assert_eq!(
                derived_quads_total(&repo),
                0,
                "the legitimate quad beside an unsafe {position} must not be written"
            );
        }
    }

    /// The escape hatch that matters: an unsafe IRI in a mixed batch rejects
    /// the batch and leaves the fenced graphs untouched.
    #[tokio::test]
    async fn an_unsafe_iri_rejects_the_batch_and_writes_nothing() {
        let repo = in_memory_repo();
        let hostile = "urn:x> <urn:p> <urn:o> . } INSERT DATA { GRAPH <urn:ngm:graph:ontology:assert> { <urn:evil";
        let err = repo
            .append_derived_quads(vec![
                quad(GRAPH_ONTOLOGY_SUMMARY, S, P, "legitimate"),
                quad(GRAPH_ONTOLOGY_SUMMARY, hostile, P, "hostile"),
            ])
            .await
            .expect_err("an unsafe subject IRI must reject the batch");
        assert!(format!("{err}").contains("unsafe subject IRI"), "{err}");
        assert_eq!(derived_quads_total(&repo), 0);
        assert_eq!(
            graph_len(&repo, GRAPH_ONTOLOGY),
            0,
            "the injection target must be empty"
        );
        assert_eq!(graph_len(&repo, GRAPH_ONTOLOGY_INFERRED), 0);
    }

    /// A literal object containing SPARQL metacharacters is escaped, not
    /// rejected, and cannot break out of the INSERT DATA block.
    #[tokio::test]
    async fn hostile_literal_objects_are_escaped_not_executed() {
        let repo = in_memory_repo();
        let hostile_literal =
            "value\" . } INSERT DATA { GRAPH <urn:ngm:graph:ontology:assert> { <urn:e> <urn:p> \"x";
        repo.append_derived_quads(vec![quad(GRAPH_ONTOLOGY_SUMMARY, S, P, hostile_literal)])
            .await
            .expect("a literal is escaped, not refused");

        assert_eq!(graph_len(&repo, GRAPH_ONTOLOGY_SUMMARY), 1);
        assert_eq!(
            graph_len(&repo, GRAPH_ONTOLOGY),
            0,
            "the escaped literal must not have reached the asserted graph"
        );
    }

    /// ADR-2015: the fence lives in the repository, not in the HTTP handler.
    /// A direct call — the path an in-process actor or a future route would
    /// take — is fenced identically.
    #[tokio::test]
    async fn the_fence_holds_on_a_direct_repository_call() {
        let repo = in_memory_repo();
        // No handler, no request, no middleware: the repository method itself.
        assert!(repo
            .append_derived_quads(vec![quad(GRAPH_ONTOLOGY, S, P, "x")])
            .await
            .is_err());
        assert!(repo
            .append_derived_quads(vec![quad(GRAPH_ONTOLOGY_INFERRED, S, P, "x")])
            .await
            .is_err());
        assert_eq!(graph_len(&repo, GRAPH_ONTOLOGY), 0);
        assert_eq!(graph_len(&repo, GRAPH_ONTOLOGY_INFERRED), 0);
    }

    /// An empty batch is a no-op, not an error.
    #[tokio::test]
    async fn an_empty_batch_is_a_no_op() {
        let repo = in_memory_repo();
        assert_eq!(repo.append_derived_quads(vec![]).await.unwrap(), 0);
        assert_eq!(derived_quads_total(&repo), 0);
    }

    /// The fence constant covers exactly the two graphs the ADR names, so a new
    /// graph cannot be added to the fence without this test noticing.
    #[test]
    fn the_fence_covers_assert_and_inferred() {
        assert_eq!(DERIVED_FENCE.len(), 2);
        assert!(DERIVED_FENCE.contains(&GRAPH_ONTOLOGY));
        assert!(DERIVED_FENCE.contains(&GRAPH_ONTOLOGY_INFERRED));
    }
}
