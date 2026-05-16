// src/adapters/oxigraph_ontology_repository.rs
//! Oxigraph Ontology Repository Adapter (Phase 11 scaffolding).
//!
//! Implements [`OntologyRepository`] over an embedded Oxigraph quad-store
//! per ADR-11 §D1 + §D2. Asserted ontology triples live in the named graph
//! `<urn:visionflow:graph:ontology>`; whelk-derived inferred axioms in
//! `<urn:visionflow:graph:ontology:inferred>` (ADR-11 §D9).
//!
//! ## Phase-1 status
//!
//! This file is **scaffolding** in the sense described by ADR-11
//! §"Implementation order" step 1: every async method present on the trait
//! is present here with a matching signature, and either:
//!
//! - delegates to a `todo!(...)` macro carrying the SPARQL fragment that
//!   the next phase must finish; or
//! - returns a trivially-correct default (e.g. empty `Vec`) where the
//!   trait already supplies a default implementation.
//!
//! Method bodies are explicit even where the trait provides a default —
//! by design — so that the SPARQL surface is enumerated in a single place
//! for Phase 2 work.
//!
//! ## Named graph layout (ADR-11 §D2)
//!
//! | Named graph IRI                                 | Contents                        |
//! |-------------------------------------------------|---------------------------------|
//! | `urn:visionflow:graph:ontology`                 | asserted OntologyClass/Property/Axiom |
//! | `urn:visionflow:graph:ontology:inferred`        | whelk-derived inferred axioms    |
//! | `urn:visionflow:graph:knowledge`                | KGNode + KGEdge triples         |
//! | (default graph)                                 | cross-graph bridges + schema    |
//!
//! ## IRI minting (ADR-11 §D3)
//!
//! All IRIs use the `vc:` prefix expanding to
//! `https://visionflow.dreamlab/ns/`. OntologyClass IRIs follow the
//! pattern `vc:onto/<slug>`; Properties `vc:prop/<slug>`; Axioms
//! `vc:axiom/<sha256-12>` content-addressed.

#![cfg(feature = "persistence-oxigraph")]

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use oxigraph::store::Store;

use crate::models::graph::GraphData;
use crate::ports::ontology_repository::{
    AxiomType, InferenceResults, OntologyMetrics, OntologyRepository,
    OntologyRepositoryError, OwlAxiom, OwlClass, OwlProperty,
    PathfindingCacheEntry, PropertyType, Result as RepoResult,
    ValidationReport,
};

/// Canonical IRIs for the four named graphs ADR-11 §D2 enumerates.
/// Held as `&'static str` so SPARQL string construction is allocation-free.
pub const GRAPH_ONTOLOGY: &str          = "urn:visionflow:graph:ontology";
pub const GRAPH_ONTOLOGY_INFERRED: &str = "urn:visionflow:graph:ontology:inferred";
pub const GRAPH_KNOWLEDGE: &str         = "urn:visionflow:graph:knowledge";
pub const GRAPH_AGENT: &str             = "urn:visionflow:graph:agent";

/// `vc:` prefix expansion per ADR-11 §D3.
pub const VC_NS: &str = "https://visionflow.dreamlab/ns/";

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
    ///
    /// Phase 1: signature is final, body is a `todo!()`. The Phase 2 body
    /// will be roughly:
    ///
    /// ```ignore
    /// let store = Store::open(data_dir)
    ///     .map_err(|e| OntologyRepositoryError::DatabaseError(e.to_string()))?;
    /// Ok(Self { store: Arc::new(store) })
    /// ```
    pub async fn open(data_dir: &std::path::Path) -> RepoResult<Self> {
        // SPARQL: n/a — RocksDB open, not a query.
        // Next-phase body: `Store::open(data_dir)` then wrap in `Arc`.
        todo!("Open Oxigraph store at {data_dir:?}; wrap in Arc<Store>")
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
}

#[async_trait]
impl OntologyRepository for OxigraphOntologyRepository {
    // ------------------------------------------------------------------
    // Graph-level read/write
    // ------------------------------------------------------------------

    async fn load_ontology_graph(&self) -> RepoResult<Arc<GraphData>> {
        // SPARQL:
        //   SELECT ?s ?p ?o
        //   FROM <urn:visionflow:graph:ontology>
        //   WHERE { ?s ?p ?o }
        // Then project into GraphData (nodes from rdf:type vc:OntologyClass,
        // edges from vc:bridgeTo / rdfs:subClassOf / vc:relatesTo / ...).
        // This is one of the five "complex" methods flagged in SCAFFOLD-NOTES.md
        // because the projection from triples back into the GraphData
        // node/edge shape involves all of D2 (graph segregation), D3 (IRI
        // minting), and the semantic-relationship folding catalogued in
        // OwlClass (has_part, requires, enables, ...).
        todo!(
            "SPARQL: SELECT ?s ?p ?o FROM <{GRAPH_ONTOLOGY}> WHERE {{ ?s ?p ?o }} \
             -> fold into GraphData (nodes by rdf:type, edges by predicate)"
        )
    }

    async fn save_ontology_graph(&self, _graph: &GraphData) -> RepoResult<()> {
        // SPARQL:
        //   CLEAR GRAPH <urn:visionflow:graph:ontology> ;
        //   INSERT DATA {
        //     GRAPH <urn:visionflow:graph:ontology> {
        //       <vc:onto/foo> a vc:OntologyClass ;
        //                     vc:label "Foo" ;
        //                     vc:bridgeTo <vc:onto/bar> .
        //       ...
        //     }
        //   }
        // Constraint guard (ADR-11 §D6): pre-write ASK for IRI uniqueness
        // before bulk insert; reject the entire transaction on conflict.
        todo!(
            "SPARQL: CLEAR GRAPH <{GRAPH_ONTOLOGY}> ; INSERT DATA {{ GRAPH <{GRAPH_ONTOLOGY}> {{ ... }} }}"
        )
    }

    async fn save_ontology(
        &self,
        _classes: &[OwlClass],
        _properties: &[OwlProperty],
        _axioms: &[OwlAxiom],
    ) -> RepoResult<()> {
        // SPARQL: composite Update of three INSERT DATA blocks (one per
        // collection) inside `GRAPH <urn:visionflow:graph:ontology>`, all
        // in a single transaction. Replaces the historical "MERGE per
        // node" Cypher loop with a bulk INSERT DATA.
        todo!(
            "SPARQL: INSERT DATA {{ GRAPH <{GRAPH_ONTOLOGY}> {{ <classes...> <properties...> <axioms...> }} }}"
        )
    }

    // ------------------------------------------------------------------
    // OWL Class CRUD
    // ------------------------------------------------------------------

    async fn add_owl_class(&self, _class: &OwlClass) -> RepoResult<String> {
        // SPARQL:
        //   ASK FROM <urn:visionflow:graph:ontology> { <vc:onto/<slug>> a vc:OntologyClass }
        // If true -> OntologyRepositoryError::InvalidData (duplicate IRI per ADR-11 §D6).
        // Else:
        //   INSERT DATA { GRAPH <urn:visionflow:graph:ontology> {
        //     <vc:onto/<slug>> a vc:OntologyClass ;
        //                      rdfs:label "<label>" ;
        //                      vc:termId "<term_id>" ;
        //                      vc:qualityScore "<qs>"^^xsd:float ;
        //                      vc:authorityScore "<as>"^^xsd:float ;
        //                      vc:status "<status>" ;
        //                      vc:maturity "<maturity>" ;
        //                      vc:owlPhysicality "<phys>" ;
        //                      vc:owlRole "<role>" ;
        //                      vc:belongsToDomain "<dom>" ;
        //                      vc:bridgesToDomain "<bdom>" ;
        //                      ...all the OwlClass V2 metadata fields...
        //                      vc:hasPart <vc:onto/<other>> , ... ;
        //                      vc:requires <vc:onto/<other>> , ... ;
        //                      vc:enables <vc:onto/<other>> , ... ;
        //                      vc:relatesTo <vc:onto/<other>> , ... ;
        //                      vc:bridgeTo <vc:onto/<other>> , ... ;
        //   } }
        // Returns the minted IRI.
        // This is the second "complex" method flagged in SCAFFOLD-NOTES.md
        // due to the breadth of OwlClass V2 metadata (40+ fields).
        todo!(
            "SPARQL: ASK uniqueness + INSERT DATA full OwlClass V2 metadata \
             into <{GRAPH_ONTOLOGY}>; return minted IRI"
        )
    }

    async fn get_owl_class(&self, _iri: &str) -> RepoResult<Option<OwlClass>> {
        // SPARQL:
        //   SELECT ?p ?o
        //   FROM <urn:visionflow:graph:ontology>
        //   WHERE { <iri> ?p ?o }
        // Fold the property-bag rows back into an OwlClass struct.
        todo!(
            "SPARQL: SELECT ?p ?o FROM <{GRAPH_ONTOLOGY}> WHERE {{ <{{iri}}> ?p ?o }}; \
             fold into OwlClass"
        )
    }

    async fn list_owl_classes(&self) -> RepoResult<Vec<OwlClass>> {
        // SPARQL:
        //   SELECT ?s ?p ?o
        //   FROM <urn:visionflow:graph:ontology>
        //   WHERE { ?s a vc:OntologyClass ; ?p ?o }
        //   ORDER BY ?s
        // Group rows by ?s and fold each group into one OwlClass.
        // This is the third "complex" method in SCAFFOLD-NOTES.md: it is
        // the hot read path used by the OntologyActor on startup and
        // dominates SPARQL p99 in the perf budget (PRD-11 §7).
        todo!(
            "SPARQL: SELECT ?s ?p ?o FROM <{GRAPH_ONTOLOGY}> WHERE \
             {{ ?s a vc:OntologyClass ; ?p ?o }} ORDER BY ?s; \
             group-by-subject into Vec<OwlClass>"
        )
    }

    // ------------------------------------------------------------------
    // OWL Property CRUD
    // ------------------------------------------------------------------

    async fn add_owl_property(&self, _property: &OwlProperty) -> RepoResult<String> {
        // SPARQL:
        //   ASK FROM <urn:visionflow:graph:ontology> { <vc:prop/<slug>> a ?t }
        //     where ?t in (owl:ObjectProperty, owl:DatatypeProperty, owl:AnnotationProperty)
        //   INSERT DATA { GRAPH <urn:visionflow:graph:ontology> {
        //     <vc:prop/<slug>> a <type-iri> ;
        //                      rdfs:label "<label>" ;
        //                      rdfs:domain <vc:onto/<dom1>> , ... ;
        //                      rdfs:range  <vc:onto/<r1>> , ... ;
        //                      vc:qualityScore "<qs>"^^xsd:float ;
        //                      vc:authorityScore "<as>"^^xsd:float .
        //   } }
        todo!(
            "SPARQL: ASK + INSERT DATA OwlProperty (Object/Datatype/Annotation) \
             into <{GRAPH_ONTOLOGY}>; return minted IRI"
        )
    }

    async fn get_owl_property(&self, _iri: &str) -> RepoResult<Option<OwlProperty>> {
        // SPARQL: SELECT ?p ?o FROM <urn:visionflow:graph:ontology> WHERE { <iri> ?p ?o }
        // Decide PropertyType from the rdf:type triple.
        let _ = PropertyType::ObjectProperty; // silence dead-code on unused-in-scaffold enum
        todo!(
            "SPARQL: SELECT ?p ?o FROM <{GRAPH_ONTOLOGY}> WHERE {{ <{{iri}}> ?p ?o }}; \
             classify by rdf:type"
        )
    }

    async fn list_owl_properties(&self) -> RepoResult<Vec<OwlProperty>> {
        // SPARQL:
        //   SELECT ?s ?p ?o
        //   FROM <urn:visionflow:graph:ontology>
        //   WHERE {
        //     ?s a ?t .
        //     FILTER(?t IN (owl:ObjectProperty, owl:DatatypeProperty, owl:AnnotationProperty))
        //     ?s ?p ?o .
        //   }
        todo!(
            "SPARQL: SELECT ?s ?p ?o FROM <{GRAPH_ONTOLOGY}> WHERE \
             {{ ?s a ?t . FILTER(?t IN (owl:ObjectProperty, owl:DatatypeProperty, owl:AnnotationProperty)) . ?s ?p ?o }}"
        )
    }

    // ------------------------------------------------------------------
    // Aggregate accessors
    // ------------------------------------------------------------------

    async fn get_classes(&self) -> RepoResult<Vec<OwlClass>> {
        // Equivalent to list_owl_classes; trait has both for historical
        // reasons. Delegate semantics in Phase 2.
        todo!("SPARQL: same as list_owl_classes; delegate")
    }

    async fn get_axioms(&self) -> RepoResult<Vec<OwlAxiom>> {
        // SPARQL:
        //   SELECT ?axiom ?type ?subject ?object
        //   FROM <urn:visionflow:graph:ontology>
        //   WHERE {
        //     ?axiom a vc:Axiom ;
        //            vc:axiomType ?type ;
        //            vc:subject ?subject ;
        //            vc:object ?object .
        //   }
        // Fold to Vec<OwlAxiom>; map ?type string to AxiomType enum.
        let _ = AxiomType::SubClassOf; // silence unused warning in scaffold
        todo!(
            "SPARQL: SELECT ?axiom ?type ?subject ?object FROM <{GRAPH_ONTOLOGY}> \
             WHERE {{ ?axiom a vc:Axiom ; vc:axiomType ?type ; vc:subject ?subject ; vc:object ?object }}"
        )
    }

    async fn add_axiom(&self, _axiom: &OwlAxiom) -> RepoResult<u64> {
        // SPARQL:
        //   INSERT DATA { GRAPH <urn:visionflow:graph:ontology> {
        //     <vc:axiom/<sha256-12>> a vc:Axiom ;
        //                            vc:axiomType "SubClassOf" ;
        //                            vc:subject <vc:onto/<s>> ;
        //                            vc:object  <vc:onto/<o>> ;
        //                            vc:annotation "<k>:<v>" , ...
        //   } }
        // The returned u64 id is derived from sha256 first 8 bytes -> u64 BE.
        todo!(
            "SPARQL: INSERT DATA axiom triple into <{GRAPH_ONTOLOGY}>; \
             return u64 derived from sha256(subject||predicate||object)"
        )
    }

    async fn get_class_axioms(&self, _class_iri: &str) -> RepoResult<Vec<OwlAxiom>> {
        // SPARQL:
        //   SELECT ?axiom ?type ?object
        //   FROM <urn:visionflow:graph:ontology>
        //   WHERE {
        //     ?axiom a vc:Axiom ;
        //            vc:subject <class_iri> ;
        //            vc:axiomType ?type ;
        //            vc:object ?object .
        //   }
        todo!(
            "SPARQL: SELECT ?axiom ?type ?object FROM <{GRAPH_ONTOLOGY}> \
             WHERE {{ ?axiom a vc:Axiom ; vc:subject <{{class_iri}}> ; vc:axiomType ?type ; vc:object ?object }}"
        )
    }

    // ------------------------------------------------------------------
    // Inference (ADR-11 §D9 — materialised in the :inferred named graph)
    // ------------------------------------------------------------------

    async fn store_inference_results(&self, _results: &InferenceResults) -> RepoResult<()> {
        // SPARQL (ADR-11 §D9, two statements run as a single atomic Update):
        //   DELETE { GRAPH <urn:visionflow:graph:ontology:inferred> { ?s ?p ?o } }
        //   WHERE  { GRAPH <urn:visionflow:graph:ontology:inferred> { ?s ?p ?o } } ;
        //   INSERT DATA {
        //     GRAPH <urn:visionflow:graph:ontology:inferred> {
        //         <vc:onto/foo> rdfs:subClassOf <vc:onto/bar> .
        //         ...
        //     }
        //   }
        // Atomicity is provided by Oxigraph's SPARQL Update transaction
        // semantics; partial application is impossible. ADR-11 §D9.
        todo!(
            "SPARQL: DELETE ALL from <{GRAPH_ONTOLOGY_INFERRED}> ; \
             INSERT DATA inferred axioms into <{GRAPH_ONTOLOGY_INFERRED}>"
        )
    }

    async fn get_inference_results(&self) -> RepoResult<Option<InferenceResults>> {
        // SPARQL:
        //   SELECT ?s ?p ?o
        //   FROM <urn:visionflow:graph:ontology:inferred>
        //   WHERE { ?s ?p ?o }
        // Combined with the result-set metadata (timestamp, reasoner_version)
        // stored as a single triple cluster keyed on the inferred-graph IRI.
        todo!(
            "SPARQL: SELECT ?s ?p ?o FROM <{GRAPH_ONTOLOGY_INFERRED}> WHERE {{ ?s ?p ?o }}; \
             load metadata triples for timestamp/version"
        )
    }

    async fn validate_ontology(&self) -> RepoResult<ValidationReport> {
        // SPARQL: a battery of ASK queries enumerating the 5 UNIQUE
        // constraints ADR-11 §D6 says we will enforce in code rather than
        // in the store. Each violation becomes one entry in
        // ValidationReport::errors.
        //
        // Examples:
        //   ASK FROM <urn:visionflow:graph:ontology> {
        //     ?a a vc:OntologyClass . ?b a vc:OntologyClass .
        //     ?a vc:iri ?i . ?b vc:iri ?i . FILTER(?a != ?b)
        //   }
        //   -> if true: duplicate IRI in OntologyClass
        todo!(
            "SPARQL: ASK battery for 5 UNIQUE-style constraints from ADR-11 \u{00A7}D6"
        )
    }

    async fn query_ontology(&self, _query: &str) -> RepoResult<Vec<HashMap<String, String>>> {
        // SPARQL: pass-through arbitrary SELECT against the union of all
        // ontology named graphs. Used by the admin REPL surface only.
        // Output rows projected into HashMap<binding-name, lexical-form>.
        todo!(
            "SPARQL: execute caller-supplied SELECT against union of \
             <{GRAPH_ONTOLOGY}> + <{GRAPH_ONTOLOGY_INFERRED}>; project bindings into HashMap"
        )
    }

    // ------------------------------------------------------------------
    // Removal
    // ------------------------------------------------------------------

    async fn remove_owl_class(&self, _iri: &str) -> RepoResult<()> {
        // SPARQL:
        //   DELETE { GRAPH <urn:visionflow:graph:ontology> { <iri> ?p ?o } }
        //   WHERE  { GRAPH <urn:visionflow:graph:ontology> { <iri> ?p ?o } }
        // Also cascade-delete any axiom with vc:subject <iri> OR vc:object <iri>.
        todo!(
            "SPARQL: DELETE class + cascade axioms in <{GRAPH_ONTOLOGY}>"
        )
    }

    async fn remove_axiom(&self, _axiom_id: u64) -> RepoResult<()> {
        // SPARQL:
        //   DELETE { GRAPH <urn:visionflow:graph:ontology> { ?a ?p ?o } }
        //   WHERE  { GRAPH <urn:visionflow:graph:ontology> {
        //     ?a a vc:Axiom .
        //     FILTER(STR(?a) = "vc:axiom/<sha256-derived-from-id>") .
        //     ?a ?p ?o .
        //   } }
        todo!(
            "SPARQL: DELETE axiom by id from <{GRAPH_ONTOLOGY}>"
        )
    }

    // ------------------------------------------------------------------
    // Metrics
    // ------------------------------------------------------------------

    async fn get_metrics(&self) -> RepoResult<OntologyMetrics> {
        // SPARQL: 5 small COUNT queries.
        //   SELECT (COUNT(?s) AS ?n) FROM <urn:visionflow:graph:ontology>
        //   WHERE { ?s a vc:OntologyClass }
        // ...similar for properties and axioms. Plus a recursive subClassOf
        // pattern for max_depth — see PRD-11 §6 SPARQL benchmark notes;
        // this query is the depth-traversal hot path:
        //   SELECT (MAX(?depth) AS ?d) FROM <urn:visionflow:graph:ontology>
        //   WHERE { ?leaf rdfs:subClassOf+ ?root .
        //           BIND(... compute depth ...) AS ?depth }
        // This is the fourth "complex" method in SCAFFOLD-NOTES.md because
        // the depth computation needs explicit per-subject path enumeration
        // (SPARQL has no built-in depth aggregate).
        todo!(
            "SPARQL: 3 COUNT(*) queries (classes, properties, axioms) + \
             rdfs:subClassOf+ depth traversal for max_depth + avg branching"
        )
    }

    // ------------------------------------------------------------------
    // Pathfinding cache (optional — has default impl on the trait).
    // Override explicitly so the cache lives in a `vc:pathCache` sub-graph
    // and can be invalidated atomically by `CLEAR GRAPH`.
    // ------------------------------------------------------------------

    async fn cache_sssp_result(&self, _entry: &PathfindingCacheEntry) -> RepoResult<()> {
        // SPARQL:
        //   INSERT DATA { GRAPH <urn:visionflow:graph:cache:sssp> {
        //       <vc:pathcache/sssp/<source>> vc:computedAt "<ts>"^^xsd:dateTime ;
        //                                    vc:distances "<json>"^^xsd:string ;
        //                                    vc:paths "<json>"^^xsd:string .
        //   } }
        todo!("SPARQL: INSERT DATA SSSP cache entry into <urn:visionflow:graph:cache:sssp>")
    }

    async fn get_cached_sssp(&self, _source_node_id: u32) -> RepoResult<Option<PathfindingCacheEntry>> {
        // SPARQL:
        //   SELECT ?p ?o FROM <urn:visionflow:graph:cache:sssp>
        //   WHERE { <vc:pathcache/sssp/<source>> ?p ?o }
        todo!("SPARQL: SELECT SSSP cache by source node from <urn:visionflow:graph:cache:sssp>")
    }

    async fn cache_apsp_result(&self, _distance_matrix: &Vec<Vec<f32>>) -> RepoResult<()> {
        // SPARQL:
        //   CLEAR GRAPH <urn:visionflow:graph:cache:apsp> ;
        //   INSERT DATA { GRAPH <urn:visionflow:graph:cache:apsp> {
        //       <vc:pathcache/apsp> vc:computedAt "<ts>"^^xsd:dateTime ;
        //                           vc:matrix "<json-of-Vec<Vec<f32>>>"^^xsd:string .
        //   } }
        todo!("SPARQL: CLEAR + INSERT APSP matrix into <urn:visionflow:graph:cache:apsp>")
    }

    async fn get_cached_apsp(&self) -> RepoResult<Option<Vec<Vec<f32>>>> {
        // SPARQL:
        //   SELECT ?matrix FROM <urn:visionflow:graph:cache:apsp>
        //   WHERE { <vc:pathcache/apsp> vc:matrix ?matrix }
        todo!("SPARQL: SELECT APSP matrix from <urn:visionflow:graph:cache:apsp>")
    }

    async fn invalidate_pathfinding_caches(&self) -> RepoResult<()> {
        // SPARQL:
        //   CLEAR GRAPH <urn:visionflow:graph:cache:sssp> ;
        //   CLEAR GRAPH <urn:visionflow:graph:cache:apsp>
        todo!("SPARQL: CLEAR GRAPH <...:cache:sssp> ; CLEAR GRAPH <...:cache:apsp>")
    }
}

// Silence unused-import warnings in scaffold builds where the error
// variants aren't yet constructed. Phase 2 will use these.
#[allow(dead_code)]
fn _silence_unused_error_variant() -> OntologyRepositoryError {
    OntologyRepositoryError::NotFound
}
