// src/adapters/oxigraph_ontology_repository.rs
//! Oxigraph Ontology Repository Adapter (ADR-11 Phase 1 implementation).
//!
//! Implements [`OntologyRepository`] over an embedded Oxigraph quad-store
//! per ADR-11 §D1 + §D2. Asserted ontology triples live in the named graph
//! `<urn:visionflow:graph:ontology:assert>`; whelk-derived inferred axioms
//! in `<urn:visionflow:graph:ontology:inferred>` (ADR-11 §D9).
//!
//! ## Named graph layout (ADR-11 §D2)
//!
//! | Named graph IRI                                 | Contents                              |
//! |-------------------------------------------------|---------------------------------------|
//! | `urn:visionflow:graph:ontology:assert`          | asserted OntologyClass/Property/Axiom |
//! | `urn:visionflow:graph:ontology:inferred`        | whelk-derived inferred axioms          |
//! | `urn:visionflow:graph:knowledge`                | KGNode + KGEdge triples               |
//! | (default graph)                                 | cross-graph bridges + schema          |
//!
//! ## IRI minting (ADR-11 §D3)
//!
//! All IRIs use the `vc:` prefix expanding to
//! `https://visionflow.dreamlab/ns/`. OntologyClass IRIs follow the
//! pattern `urn:visionflow:owl:class:<slug>`; Properties
//! `urn:visionflow:owl:property:<slug>`; Axioms
//! `urn:visionflow:owl:axiom:<sha256-12>` content-addressed.

#![cfg(feature = "persistence-oxigraph")]

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use oxigraph::model::Term;
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;

use crate::models::edge::Edge;
use crate::models::graph::GraphData;
use crate::models::node::Node;
use crate::ports::ontology_repository::{
    AxiomType, InferenceResults, OntologyMetrics, OntologyRepository,
    OntologyRepositoryError, OwlAxiom, OwlClass, OwlProperty,
    PathfindingCacheEntry, PropertyType, Result as RepoResult,
    ValidationReport,
};

/// Canonical IRIs for the four named graphs ADR-11 §D2 enumerates.
/// Held as `&'static str` so SPARQL string construction is allocation-free.
pub const GRAPH_ONTOLOGY: &str          = "urn:visionflow:graph:ontology:assert";
pub const GRAPH_ONTOLOGY_INFERRED: &str = "urn:visionflow:graph:ontology:inferred";
pub const GRAPH_KNOWLEDGE: &str         = "urn:visionflow:graph:knowledge";
pub const GRAPH_AGENT: &str             = "urn:visionflow:graph:agent";

/// Cache named graphs (own sub-domain so `CLEAR GRAPH` invalidates atomically).
pub const GRAPH_CACHE_SSSP: &str = "urn:visionflow:graph:cache:sssp";
pub const GRAPH_CACHE_APSP: &str = "urn:visionflow:graph:cache:apsp";

/// `vc:` prefix expansion per ADR-11 §D3.
pub const VC_NS: &str = "https://visionflow.dreamlab/ns/";

/// SPARQL prologue applied to every UPDATE/QUERY string this adapter emits.
const PROLOGUE: &str = concat!(
    "PREFIX vc: <https://visionflow.dreamlab/ns/>\n",
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
        format!("urn:visionflow:owl:class:{}", slug(basis))
    }
}

/// Mint the canonical property IRI.
fn property_iri(p: &OwlProperty) -> String {
    if !p.iri.is_empty() {
        p.iri.clone()
    } else {
        let basis = p.label.as_deref().unwrap_or("unnamed");
        format!("urn:visionflow:owl:property:{}", slug(basis))
    }
}

/// Content-address an axiom: sha256(subject || predicate || object) → 12 hex chars.
fn axiom_iri(a: &OwlAxiom) -> String {
    let mut hasher = Sha256::new();
    hasher.update(a.subject.as_bytes());
    hasher.update(format!("{:?}", a.axiom_type).as_bytes());
    hasher.update(a.object.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(6).map(|b| format!("{:02x}", b)).collect();
    format!("urn:visionflow:owl:axiom:{}", hex)
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
    }
}

fn parse_axiom_type(s: &str) -> AxiomType {
    match s {
        "SubClassOf" => AxiomType::SubClassOf,
        "EquivalentClass" | "EquivalentClasses" => AxiomType::EquivalentClass,
        "DisjointWith" | "DisjointClasses" => AxiomType::DisjointWith,
        "ObjectPropertyAssertion" | "SubObjectProperty" => AxiomType::ObjectPropertyAssertion,
        "DataPropertyAssertion" | "Domain" | "Range" => AxiomType::DataPropertyAssertion,
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
// Predicate catalogue (vc: namespace, ADR-11 §D3).
// Kept here so the property-bag fold has one source of truth.
// ----------------------------------------------------------------------

const P_LABEL: &str            = "http://www.w3.org/2000/01/rdf-schema#label";
const P_COMMENT: &str          = "http://www.w3.org/2000/01/rdf-schema#comment";
const P_SUBCLASS_OF: &str      = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const P_DOMAIN: &str           = "http://www.w3.org/2000/01/rdf-schema#domain";
const P_RANGE: &str            = "http://www.w3.org/2000/01/rdf-schema#range";
const P_TYPE: &str             = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

const T_OWL_CLASS: &str        = "http://www.w3.org/2002/07/owl#Class";
const T_OWL_OBJECT_PROP: &str  = "http://www.w3.org/2002/07/owl#ObjectProperty";
const T_OWL_DATA_PROP: &str    = "http://www.w3.org/2002/07/owl#DatatypeProperty";
const T_OWL_ANNOT_PROP: &str   = "http://www.w3.org/2002/07/owl#AnnotationProperty";
const T_VC_ONTOLOGY_CLASS: &str = "https://visionflow.dreamlab/ns/OntologyClass";
const T_VC_AXIOM: &str         = "https://visionflow.dreamlab/ns/Axiom";

// VisionFlow-specific predicates.
const P_TERM_ID: &str           = "https://visionflow.dreamlab/ns/termId";
const P_PREFERRED_TERM: &str    = "https://visionflow.dreamlab/ns/preferredTerm";
const P_DESCRIPTION: &str       = "https://visionflow.dreamlab/ns/description";
const P_SOURCE_DOMAIN: &str     = "https://visionflow.dreamlab/ns/sourceDomain";
const P_VERSION: &str           = "https://visionflow.dreamlab/ns/version";
const P_CLASS_TYPE: &str        = "https://visionflow.dreamlab/ns/classType";
const P_STATUS: &str            = "https://visionflow.dreamlab/ns/status";
const P_MATURITY: &str          = "https://visionflow.dreamlab/ns/maturity";
const P_QUALITY_SCORE: &str     = "https://visionflow.dreamlab/ns/qualityScore";
const P_AUTHORITY_SCORE: &str   = "https://visionflow.dreamlab/ns/authorityScore";
const P_PUBLIC_ACCESS: &str     = "https://visionflow.dreamlab/ns/publicAccess";
const P_CONTENT_STATUS: &str    = "https://visionflow.dreamlab/ns/contentStatus";
const P_OWL_PHYSICALITY: &str   = "https://visionflow.dreamlab/ns/owlPhysicality";
const P_OWL_ROLE: &str          = "https://visionflow.dreamlab/ns/owlRole";
const P_BELONGS_TO_DOMAIN: &str = "https://visionflow.dreamlab/ns/belongsToDomain";
const P_BRIDGES_TO_DOMAIN: &str = "https://visionflow.dreamlab/ns/bridgesToDomain";
const P_SOURCE_FILE: &str       = "https://visionflow.dreamlab/ns/sourceFile";
const P_FILE_SHA1: &str         = "https://visionflow.dreamlab/ns/fileSha1";
const P_MARKDOWN_CONTENT: &str  = "https://visionflow.dreamlab/ns/markdownContent";
const P_LAST_SYNCED: &str       = "https://visionflow.dreamlab/ns/lastSynced";
const P_ADDITIONAL_META: &str   = "https://visionflow.dreamlab/ns/additionalMetadata";

const P_HAS_PART: &str          = "https://visionflow.dreamlab/ns/hasPart";
const P_IS_PART_OF: &str        = "https://visionflow.dreamlab/ns/isPartOf";
const P_REQUIRES: &str          = "https://visionflow.dreamlab/ns/requires";
const P_DEPENDS_ON: &str        = "https://visionflow.dreamlab/ns/dependsOn";
const P_ENABLES: &str           = "https://visionflow.dreamlab/ns/enables";
const P_RELATES_TO: &str        = "https://visionflow.dreamlab/ns/relatesTo";
const P_BRIDGES_TO: &str        = "https://visionflow.dreamlab/ns/bridgesTo";
const P_BRIDGES_FROM: &str      = "https://visionflow.dreamlab/ns/bridgesFrom";
const P_OTHER_REL_PREFIX: &str  = "https://visionflow.dreamlab/ns/otherRel/";
const P_PROPERTY_PREFIX: &str   = "https://visionflow.dreamlab/ns/property/";

const P_AXIOM_TYPE: &str        = "https://visionflow.dreamlab/ns/axiomType";
const P_AXIOM_SUBJECT: &str     = "https://visionflow.dreamlab/ns/subject";
const P_AXIOM_OBJECT: &str      = "https://visionflow.dreamlab/ns/object";
const P_AXIOM_ANNOTATION: &str  = "https://visionflow.dreamlab/ns/annotation";
const P_AXIOM_ID: &str          = "https://visionflow.dreamlab/ns/axiomId";

const P_INFERRED_AT: &str       = "https://visionflow.dreamlab/ns/inferredAt";
const P_INFERRED_TIME_MS: &str  = "https://visionflow.dreamlab/ns/inferenceTimeMs";
const P_INFERRED_VERSION: &str  = "https://visionflow.dreamlab/ns/reasonerVersion";
const INFER_META_IRI: &str      = "urn:visionflow:owl:inference:meta";

const P_CACHE_COMPUTED_AT: &str = "https://visionflow.dreamlab/ns/computedAt";
const P_CACHE_DISTANCES: &str   = "https://visionflow.dreamlab/ns/distances";
const P_CACHE_PATHS: &str       = "https://visionflow.dreamlab/ns/paths";
const P_CACHE_MATRIX: &str      = "https://visionflow.dreamlab/ns/matrix";
const P_CACHE_TARGET: &str      = "https://visionflow.dreamlab/ns/targetNode";
const P_CACHE_COMP_TIME: &str   = "https://visionflow.dreamlab/ns/computationTimeMs";
const APSP_IRI: &str            = "urn:visionflow:pathcache:apsp";

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
        Ok(Self { store: Arc::new(store) })
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
        tokio::task::spawn_blocking(move || -> RepoResult<(Vec<String>, Vec<Vec<Option<Term>>>)> {
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
                        let row: Vec<Option<Term>> = vars
                            .iter()
                            .map(|v| sol.get(v.as_str()).cloned())
                            .collect();
                        rows.push(row);
                    }
                    Ok((vars, rows))
                }
                _ => Err(db_err("SELECT did not return Solutions")),
            }
        })
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

        buf.push_str(&format!("<{iri}> a <{T_VC_ONTOLOGY_CLASS}> , <{T_OWL_CLASS}> .\n"));

        if let Some(v) = &c.label {
            buf.push_str(&format!("<{iri}> <{P_LABEL}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.term_id {
            buf.push_str(&format!("<{iri}> <{P_TERM_ID}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.preferred_term {
            buf.push_str(&format!("<{iri}> <{P_PREFERRED_TERM}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.description {
            buf.push_str(&format!("<{iri}> <{P_DESCRIPTION}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.source_domain {
            buf.push_str(&format!("<{iri}> <{P_SOURCE_DOMAIN}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.version {
            buf.push_str(&format!("<{iri}> <{P_VERSION}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.class_type {
            buf.push_str(&format!("<{iri}> <{P_CLASS_TYPE}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.status {
            buf.push_str(&format!("<{iri}> <{P_STATUS}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.maturity {
            buf.push_str(&format!("<{iri}> <{P_MATURITY}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = c.quality_score {
            buf.push_str(&format!("<{iri}> <{P_QUALITY_SCORE}> \"{v}\"^^xsd:float .\n"));
        }
        if let Some(v) = c.authority_score {
            buf.push_str(&format!("<{iri}> <{P_AUTHORITY_SCORE}> \"{v}\"^^xsd:float .\n"));
        }
        if let Some(v) = c.public_access {
            buf.push_str(&format!("<{iri}> <{P_PUBLIC_ACCESS}> \"{v}\"^^xsd:boolean .\n"));
        }
        if let Some(v) = &c.content_status {
            buf.push_str(&format!("<{iri}> <{P_CONTENT_STATUS}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.owl_physicality {
            buf.push_str(&format!("<{iri}> <{P_OWL_PHYSICALITY}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.owl_role {
            buf.push_str(&format!("<{iri}> <{P_OWL_ROLE}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.belongs_to_domain {
            buf.push_str(&format!("<{iri}> <{P_BELONGS_TO_DOMAIN}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.bridges_to_domain {
            buf.push_str(&format!("<{iri}> <{P_BRIDGES_TO_DOMAIN}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.source_file {
            buf.push_str(&format!("<{iri}> <{P_SOURCE_FILE}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.file_sha1 {
            buf.push_str(&format!("<{iri}> <{P_FILE_SHA1}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = &c.markdown_content {
            buf.push_str(&format!("<{iri}> <{P_MARKDOWN_CONTENT}> \"{}\" .\n", escape_literal(v)));
        }
        if let Some(v) = c.last_synced {
            buf.push_str(&format!(
                "<{iri}> <{P_LAST_SYNCED}> \"{}\"^^xsd:dateTime .\n",
                v.to_rfc3339()
            ));
        }
        if let Some(v) = &c.additional_metadata {
            buf.push_str(&format!("<{iri}> <{P_ADDITIONAL_META}> \"{}\" .\n", escape_literal(v)));
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
            buf.push_str(&format!(
                "<{iri}> <{pred}> \"{}\" .\n",
                escape_literal(v)
            ));
        }

        buf
    }

    fn property_insert_block(p: &OwlProperty) -> String {
        let iri = property_iri(p);
        let type_iri = property_type_str(&p.property_type);
        let mut buf = String::with_capacity(512);
        buf.push_str(&format!("<{iri}> a {type_iri} .\n"));
        if let Some(v) = &p.label {
            buf.push_str(&format!("<{iri}> <{P_LABEL}> \"{}\" .\n", escape_literal(v)));
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
            buf.push_str(&format!("<{iri}> <{P_QUALITY_SCORE}> \"{v}\"^^xsd:float .\n"));
        }
        if let Some(v) = p.authority_score {
            buf.push_str(&format!("<{iri}> <{P_AUTHORITY_SCORE}> \"{v}\"^^xsd:float .\n"));
        }
        if let Some(v) = &p.source_file {
            buf.push_str(&format!("<{iri}> <{P_SOURCE_FILE}> \"{}\" .\n", escape_literal(v)));
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
                    if let Some(t) = o { entry.source_domain = Some(term_lexical(&t)); }
                }
                P_VERSION => {
                    if let Some(t) = o { entry.version = Some(term_lexical(&t)); }
                }
                P_CLASS_TYPE => {
                    if let Some(t) = o { entry.class_type = Some(term_lexical(&t)); }
                }
                P_STATUS => {
                    if let Some(t) = o { entry.status = Some(term_lexical(&t)); }
                }
                P_MATURITY => {
                    if let Some(t) = o { entry.maturity = Some(term_lexical(&t)); }
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
                    if let Some(t) = o { entry.owl_physicality = Some(term_lexical(&t)); }
                }
                P_OWL_ROLE => {
                    if let Some(t) = o { entry.owl_role = Some(term_lexical(&t)); }
                }
                P_BELONGS_TO_DOMAIN => {
                    if let Some(t) = o { entry.belongs_to_domain = Some(term_lexical(&t)); }
                }
                P_BRIDGES_TO_DOMAIN => {
                    if let Some(t) = o { entry.bridges_to_domain = Some(term_lexical(&t)); }
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
                .unwrap_or_else(|| format!("urn:visionflow:owl:class:{}", slug(&node.metadata_id)));
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
                .unwrap_or_else(|| format!("urn:visionflow:owl:class:{}", slug(&node.metadata_id)));
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
            let predicate = match edge.edge_type.as_deref() {
                Some("is_subclass_of") | Some("subclass_of") | Some("SUBCLASS_OF") => P_SUBCLASS_OF,
                Some("has_part") => P_HAS_PART,
                Some("is_part_of") => P_IS_PART_OF,
                Some("requires") => P_REQUIRES,
                Some("depends_on") => P_DEPENDS_ON,
                Some("enables") => P_ENABLES,
                Some("relates_to") => P_RELATES_TO,
                Some("bridges_to") => P_BRIDGES_TO,
                Some("bridges_from") => P_BRIDGES_FROM,
                _ => P_RELATES_TO,
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
                P_TERM_ID => { if let Some(t) = o_opt { c.term_id = Some(term_lexical(&t)); } }
                P_PREFERRED_TERM => { if let Some(t) = o_opt { c.preferred_term = Some(term_lexical(&t)); } }
                P_SOURCE_DOMAIN => { if let Some(t) = o_opt { c.source_domain = Some(term_lexical(&t)); } }
                P_VERSION => { if let Some(t) = o_opt { c.version = Some(term_lexical(&t)); } }
                P_CLASS_TYPE => { if let Some(t) = o_opt { c.class_type = Some(term_lexical(&t)); } }
                P_STATUS => { if let Some(t) = o_opt { c.status = Some(term_lexical(&t)); } }
                P_MATURITY => { if let Some(t) = o_opt { c.maturity = Some(term_lexical(&t)); } }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o_opt { c.quality_score = l.value().parse().ok(); }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o_opt { c.authority_score = l.value().parse().ok(); }
                }
                P_PUBLIC_ACCESS => {
                    if let Some(Term::Literal(l)) = o_opt { c.public_access = l.value().parse().ok(); }
                }
                P_CONTENT_STATUS => { if let Some(t) = o_opt { c.content_status = Some(term_lexical(&t)); } }
                P_OWL_PHYSICALITY => { if let Some(t) = o_opt { c.owl_physicality = Some(term_lexical(&t)); } }
                P_OWL_ROLE => { if let Some(t) = o_opt { c.owl_role = Some(term_lexical(&t)); } }
                P_BELONGS_TO_DOMAIN => { if let Some(t) = o_opt { c.belongs_to_domain = Some(term_lexical(&t)); } }
                P_BRIDGES_TO_DOMAIN => { if let Some(t) = o_opt { c.bridges_to_domain = Some(term_lexical(&t)); } }
                P_SOURCE_FILE => { if let Some(t) = o_opt { c.source_file = Some(term_lexical(&t)); } }
                P_FILE_SHA1 => { if let Some(t) = o_opt { c.file_sha1 = Some(term_lexical(&t)); } }
                P_MARKDOWN_CONTENT => { if let Some(t) = o_opt { c.markdown_content = Some(term_lexical(&t)); } }
                P_LAST_SYNCED => {
                    if let Some(Term::Literal(l)) = o_opt {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(l.value()) {
                            c.last_synced = Some(dt.with_timezone(&chrono::Utc));
                        }
                    }
                }
                P_ADDITIONAL_META => {
                    if let Some(t) = o_opt { c.additional_metadata = Some(term_lexical(&t)); }
                }
                P_SUBCLASS_OF => { if let Some(t) = o_opt { c.parent_classes.push(term_lexical(&t)); } }
                P_HAS_PART => { if let Some(t) = o_opt { c.has_part.push(term_lexical(&t)); } }
                P_IS_PART_OF => { if let Some(t) = o_opt { c.is_part_of.push(term_lexical(&t)); } }
                P_REQUIRES => { if let Some(t) = o_opt { c.requires.push(term_lexical(&t)); } }
                P_DEPENDS_ON => { if let Some(t) = o_opt { c.depends_on.push(term_lexical(&t)); } }
                P_ENABLES => { if let Some(t) = o_opt { c.enables.push(term_lexical(&t)); } }
                P_RELATES_TO => { if let Some(t) = o_opt { c.relates_to.push(term_lexical(&t)); } }
                P_BRIDGES_TO => { if let Some(t) = o_opt { c.bridges_to.push(term_lexical(&t)); } }
                P_BRIDGES_FROM => { if let Some(t) = o_opt { c.bridges_from.push(term_lexical(&t)); } }
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
                P_LABEL => { if let Some(Term::Literal(l)) = o { current.label = Some(l.value().to_string()); } }
                P_DESCRIPTION => { if let Some(Term::Literal(l)) = o { current.description = Some(l.value().to_string()); } }
                P_TERM_ID => { if let Some(t) = o { current.term_id = Some(term_lexical(&t)); } }
                P_PREFERRED_TERM => { if let Some(t) = o { current.preferred_term = Some(term_lexical(&t)); } }
                P_SOURCE_DOMAIN => { if let Some(t) = o { current.source_domain = Some(term_lexical(&t)); } }
                P_VERSION => { if let Some(t) = o { current.version = Some(term_lexical(&t)); } }
                P_CLASS_TYPE => { if let Some(t) = o { current.class_type = Some(term_lexical(&t)); } }
                P_STATUS => { if let Some(t) = o { current.status = Some(term_lexical(&t)); } }
                P_MATURITY => { if let Some(t) = o { current.maturity = Some(term_lexical(&t)); } }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o { current.quality_score = l.value().parse().ok(); }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o { current.authority_score = l.value().parse().ok(); }
                }
                P_PUBLIC_ACCESS => {
                    if let Some(Term::Literal(l)) = o { current.public_access = l.value().parse().ok(); }
                }
                P_CONTENT_STATUS => { if let Some(t) = o { current.content_status = Some(term_lexical(&t)); } }
                P_OWL_PHYSICALITY => { if let Some(t) = o { current.owl_physicality = Some(term_lexical(&t)); } }
                P_OWL_ROLE => { if let Some(t) = o { current.owl_role = Some(term_lexical(&t)); } }
                P_BELONGS_TO_DOMAIN => { if let Some(t) = o { current.belongs_to_domain = Some(term_lexical(&t)); } }
                P_BRIDGES_TO_DOMAIN => { if let Some(t) = o { current.bridges_to_domain = Some(term_lexical(&t)); } }
                P_SOURCE_FILE => { if let Some(t) = o { current.source_file = Some(term_lexical(&t)); } }
                P_FILE_SHA1 => { if let Some(t) = o { current.file_sha1 = Some(term_lexical(&t)); } }
                P_MARKDOWN_CONTENT => { if let Some(t) = o { current.markdown_content = Some(term_lexical(&t)); } }
                P_LAST_SYNCED => {
                    if let Some(Term::Literal(l)) = o {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(l.value()) {
                            current.last_synced = Some(dt.with_timezone(&chrono::Utc));
                        }
                    }
                }
                P_ADDITIONAL_META => { if let Some(t) = o { current.additional_metadata = Some(term_lexical(&t)); } }
                P_SUBCLASS_OF => { if let Some(t) = o { current.parent_classes.push(term_lexical(&t)); } }
                P_HAS_PART => { if let Some(t) = o { current.has_part.push(term_lexical(&t)); } }
                P_IS_PART_OF => { if let Some(t) = o { current.is_part_of.push(term_lexical(&t)); } }
                P_REQUIRES => { if let Some(t) = o { current.requires.push(term_lexical(&t)); } }
                P_DEPENDS_ON => { if let Some(t) = o { current.depends_on.push(term_lexical(&t)); } }
                P_ENABLES => { if let Some(t) = o { current.enables.push(term_lexical(&t)); } }
                P_RELATES_TO => { if let Some(t) = o { current.relates_to.push(term_lexical(&t)); } }
                P_BRIDGES_TO => { if let Some(t) = o { current.bridges_to.push(term_lexical(&t)); } }
                P_BRIDGES_FROM => { if let Some(t) = o { current.bridges_from.push(term_lexical(&t)); } }
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
                    if let Some(Term::Literal(l)) = o { prop.label = Some(l.value().to_string()); }
                }
                P_DOMAIN => { if let Some(t) = o { prop.domain.push(term_lexical(&t)); } }
                P_RANGE => { if let Some(t) = o { prop.range.push(term_lexical(&t)); } }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o { prop.quality_score = l.value().parse().ok(); }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o { prop.authority_score = l.value().parse().ok(); }
                }
                P_SOURCE_FILE => { if let Some(t) = o { prop.source_file = Some(term_lexical(&t)); } }
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
                    if let Some(Term::Literal(l)) = o { current.label = Some(l.value().to_string()); }
                }
                P_DOMAIN => { if let Some(t) = o { current.domain.push(term_lexical(&t)); } }
                P_RANGE => { if let Some(t) = o { current.range.push(term_lexical(&t)); } }
                P_QUALITY_SCORE => {
                    if let Some(Term::Literal(l)) = o { current.quality_score = l.value().parse().ok(); }
                }
                P_AUTHORITY_SCORE => {
                    if let Some(Term::Literal(l)) = o { current.authority_score = l.value().parse().ok(); }
                }
                P_SOURCE_FILE => { if let Some(t) = o { current.source_file = Some(term_lexical(&t)); } }
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
        let q = format!(
            "{PROLOGUE}\
             SELECT ?axiom ?type ?subject ?object ?id\n\
             FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{\n\
               ?axiom a <{T_VC_AXIOM}> ;\n\
                      <{P_AXIOM_TYPE}> ?type ;\n\
                      <{P_AXIOM_SUBJECT}> ?subject ;\n\
                      <{P_AXIOM_OBJECT}> ?object .\n\
               OPTIONAL {{ ?axiom <{P_AXIOM_ID}> ?id }}\n\
             }}\n"
        );
        let (_vars, rows) = self.run_select(q).await?;
        let mut out: Vec<OwlAxiom> = Vec::with_capacity(rows.len());
        for row in rows {
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
            out.push(OwlAxiom {
                id,
                axiom_type,
                subject,
                object,
                annotations: HashMap::new(),
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
        // Build the INSERT body before the format!.
        let mut body = String::with_capacity(1024);
        // Metadata triples on a sentinel IRI inside the inferred graph.
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
        for axiom in &results.inferred_axioms {
            body.push_str(&Self::axiom_insert_block(axiom));
        }
        // Single SPARQL Update with two statements separated by `;` —
        // Oxigraph commits them atomically per ADR-11 §D9.
        let update = format!(
            "{PROLOGUE}\
             DELETE {{ GRAPH <{GRAPH_ONTOLOGY_INFERRED}> {{ ?s ?p ?o }} }}\n\
             WHERE  {{ GRAPH <{GRAPH_ONTOLOGY_INFERRED}> {{ ?s ?p ?o }} }} ;\n\
             INSERT DATA {{ GRAPH <{GRAPH_ONTOLOGY_INFERRED}> {{\n{body}  }} }}\n"
        );
        self.run_update(update).await
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
             SELECT (COUNT(?s) AS ?n) FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ ?s a <{T_VC_ONTOLOGY_CLASS}> . ?s a <{T_OWL_CLASS}> }}\n"
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
               FILTER(?t NOT IN (\"SubClassOf\", \"EquivalentClass\", \"DisjointWith\", \"ObjectPropertyAssertion\", \"DataPropertyAssertion\"))\n\
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
        // 1. Class count.
        let class_q = format!(
            "{PROLOGUE}\
             SELECT (COUNT(?s) AS ?n) FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ ?s a <{T_VC_ONTOLOGY_CLASS}> }}\n"
        );
        let class_count = scalar_count(&self.run_select(class_q).await?).unwrap_or(0);

        // 2. Property count.
        let prop_q = format!(
            "{PROLOGUE}\
             SELECT (COUNT(?s) AS ?n) FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ ?s a ?t .\n\
                      FILTER(?t IN (<{T_OWL_OBJECT_PROP}>, <{T_OWL_DATA_PROP}>, <{T_OWL_ANNOT_PROP}>)) }}\n"
        );
        let property_count = scalar_count(&self.run_select(prop_q).await?).unwrap_or(0);

        // 3. Axiom count.
        let ax_q = format!(
            "{PROLOGUE}\
             SELECT (COUNT(?s) AS ?n) FROM <{GRAPH_ONTOLOGY}>\n\
             WHERE {{ ?s a <{T_VC_AXIOM}> }}\n"
        );
        let axiom_count = scalar_count(&self.run_select(ax_q).await?).unwrap_or(0);

        // 4. Branching factor: average number of direct sub-classes per
        //    parent. COUNT children grouped by parent, then average.
        let branch_q = format!(
            "{PROLOGUE}\
             SELECT (AVG(?children) AS ?avg) WHERE {{\n\
               {{ SELECT ?parent (COUNT(?child) AS ?children) FROM <{GRAPH_ONTOLOGY}>\n\
                 WHERE {{ ?child <{P_SUBCLASS_OF}> ?parent }} GROUP BY ?parent }}\n\
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
                chained.push_str(&format!(
                    "?n{k} <{P_SUBCLASS_OF}> ?n{} .\n",
                    k + 1
                ));
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
        let iri = format!("urn:visionflow:pathcache:sssp:{}", entry.source_node_id);
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

    async fn get_cached_sssp(&self, source_node_id: u32) -> RepoResult<Option<PathfindingCacheEntry>> {
        let iri = format!("urn:visionflow:pathcache:sssp:{}", source_node_id);
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
                        if let Ok(parsed) = serde_json::from_str::<HashMap<u32, Vec<u32>>>(l.value()) {
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
