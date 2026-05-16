// src/adapters/oxigraph_graph_repository.rs
//! Oxigraph Graph Repository Adapter (Phase 11 scaffolding).
//!
//! Implements [`GraphRepository`] over the same Oxigraph store as
//! [`OxigraphOntologyRepository`], but operates on the
//! `<urn:visionflow:graph:knowledge>` and `<urn:visionflow:graph:agent>`
//! named graphs (ADR-11 §D2).
//!
//! ## Position semantics (ADR-11 §D4)
//!
//! Live physics positions live in `GraphStateActor` RAM. They are **not**
//! round-tripped through Oxigraph on every tick. The
//! [`GraphRepository::update_positions`] method is the snapshot path,
//! invoked at the cadence configured by Section 1 (currently 60 s
//! wall-clock). Each position is materialised as a triple-cluster:
//!
//! ```sparql
//! GRAPH <urn:visionflow:graph:knowledge> {
//!     <vc:kg/<slug>> vc:hasX "1.23"^^xsd:float ;
//!                    vc:hasY "4.56"^^xsd:float ;
//!                    vc:hasZ "7.89"^^xsd:float ;
//!                    vc:hasVX "0.0"^^xsd:float ;
//!                    vc:hasVY "0.0"^^xsd:float ;
//!                    vc:hasVZ "0.0"^^xsd:float .
//! }
//! ```
//!
//! Cold start reads these triples once to seed the actor; warm-loop reads
//! never touch the store.
//!
//! ## Phase-1 status
//!
//! See header of [`oxigraph_ontology_repository`] — same conventions.

#![cfg(feature = "persistence-oxigraph")]

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::Vec3;
use oxigraph::store::Store;

use crate::actors::graph_actor::{AutoBalanceNotification, PhysicsState};
use crate::models::constraints::ConstraintSet;
use crate::models::edge::Edge;
use crate::models::graph::GraphData;
use crate::models::node::Node;
use crate::ports::graph_repository::{
    BinaryNodeData, GraphRepository, GraphRepositoryError, PathfindingParams,
    PathfindingResult, Result as RepoResult,
};

// Re-use the named-graph constants from the ontology adapter; both modules
// live in the same crate and the IRIs are dataset-wide.
use crate::adapters::oxigraph_ontology_repository::{
    GRAPH_AGENT, GRAPH_KNOWLEDGE,
};

/// Oxigraph-backed `GraphRepository` implementation. See module-level
/// docs for the named-graph layout and the position-snapshot model.
pub struct OxigraphGraphRepository {
    store: Arc<Store>,
}

impl OxigraphGraphRepository {
    /// Construct from an already-opened store. The store is expected to
    /// be shared with the ontology and settings adapters in the
    /// destination architecture (single-binary, single-writer, ADR-11 §D1).
    pub fn from_store(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// Convenience accessor for tests + migration tooling.
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }
}

#[async_trait]
impl GraphRepository for OxigraphGraphRepository {
    // ------------------------------------------------------------------
    // Write path
    // ------------------------------------------------------------------

    async fn add_nodes(&self, _nodes: Vec<Node>) -> RepoResult<Vec<u32>> {
        // SPARQL:
        //   INSERT DATA { GRAPH <urn:visionflow:graph:knowledge> {
        //     <vc:kg/<slug-1>> a vc:KGNode ;
        //                      vc:nodeId "<u32>"^^xsd:integer ;
        //                      vc:label "<label>" ;
        //                      vc:metadataKey "<k>" , ... .
        //     <vc:kg/<slug-2>> ...
        //   } }
        // The returned Vec<u32> is the dense node-id list (from Node.id);
        // assignment is upstream of this adapter (ID allocation lives in
        // the graph-state actor).
        todo!(
            "SPARQL: INSERT DATA Vec<Node> into <{GRAPH_KNOWLEDGE}>; return Vec<u32> of assigned ids"
        )
    }

    async fn add_edges(&self, _edges: Vec<Edge>) -> RepoResult<Vec<String>> {
        // SPARQL: predicate-IRI per relationship type (ADR-11 §D7 rule 4):
        //   INSERT DATA { GRAPH <urn:visionflow:graph:knowledge> {
        //     <vc:edge/<sha256-12>> a vc:KGEdge ;
        //                           vc:source <vc:kg/<src-slug>> ;
        //                           vc:target <vc:kg/<tgt-slug>> ;
        //                           vc:weight "<w>"^^xsd:float ;
        //                           vc:relationshipType "<type>" .
        //   } }
        // Returned Vec<String> is the list of canonical edge IRIs (or just
        // sha256-12 prefixes — Phase 2 picks the contract).
        todo!(
            "SPARQL: INSERT DATA reified edges (vc:edge/<sha256-12>) into <{GRAPH_KNOWLEDGE}>; \
             return Vec<String> of canonical edge IRIs"
        )
    }

    async fn update_positions(&self, _updates: Vec<(u32, BinaryNodeData)>) -> RepoResult<()> {
        // SPARQL (ADR-11 §D4 position-triple snapshot):
        //   DELETE { GRAPH <urn:visionflow:graph:knowledge> {
        //              ?n vc:hasX ?x ; vc:hasY ?y ; vc:hasZ ?z ;
        //                 vc:hasVX ?vx ; vc:hasVY ?vy ; vc:hasVZ ?vz . } }
        //   WHERE  { GRAPH <urn:visionflow:graph:knowledge> {
        //              ?n a vc:KGNode ; vc:nodeId ?id .
        //              FILTER(?id IN (<u32-list>))
        //              ?n vc:hasX ?x ; ... } } ;
        //   INSERT DATA { GRAPH <urn:visionflow:graph:knowledge> {
        //              <vc:kg/<slug-of-id>>
        //                  vc:hasX "<x>"^^xsd:float ;
        //                  vc:hasY "<y>"^^xsd:float ;
        //                  vc:hasZ "<z>"^^xsd:float ;
        //                  vc:hasVX "<vx>"^^xsd:float ;
        //                  vc:hasVY "<vy>"^^xsd:float ;
        //                  vc:hasVZ "<vz>"^^xsd:float .
        //              ...one block per update...
        //   } }
        // This is the fifth "complex" method in SCAFFOLD-NOTES.md: it is
        // the position-snapshot hot path, and the DELETE-then-INSERT
        // pattern needs to batch ~5k nodes into a single Oxigraph
        // transaction without bloating the SPARQL string. Phase 2 will
        // likely materialise updates via the lower-level `Store::insert`
        // / `Store::remove` API rather than building one giant SPARQL
        // Update string.
        todo!(
            "SPARQL: DELETE old vc:hasX/Y/Z + vc:hasVX/VY/VZ triples for each id; \
             INSERT DATA new position triples; batch into one transaction"
        )
    }

    async fn clear_dirty_nodes(&self) -> RepoResult<()> {
        // The "dirty nodes" concept lives in the actor layer (in-memory
        // tracking bitmap). Oxigraph has no notion of dirtiness, so the
        // adapter is a no-op once the snapshot has been written via
        // update_positions. Phase 2 may make this an explicit assertion
        // that the in-flight snapshot has been flushed.
        Ok(())
    }

    // ------------------------------------------------------------------
    // Read path
    // ------------------------------------------------------------------

    async fn get_graph(&self) -> RepoResult<Arc<GraphData>> {
        // SPARQL:
        //   SELECT ?node ?p ?o
        //   FROM <urn:visionflow:graph:knowledge>
        //   WHERE { ?node a vc:KGNode ; ?p ?o }
        //   ORDER BY ?node
        // Then a separate edge query:
        //   SELECT ?edge ?src ?tgt ?weight ?relType
        //   FROM <urn:visionflow:graph:knowledge>
        //   WHERE {
        //     ?edge a vc:KGEdge ;
        //           vc:source ?src ;
        //           vc:target ?tgt ;
        //           vc:weight ?weight ;
        //           vc:relationshipType ?relType .
        //   }
        // Fold both into a single GraphData. Cold-start path; called once
        // per actor lifetime.
        todo!(
            "SPARQL: SELECT ?node ?p ?o + SELECT ?edge ... from <{GRAPH_KNOWLEDGE}>; fold into GraphData"
        )
    }

    async fn get_node_map(&self) -> RepoResult<Arc<HashMap<u32, Node>>> {
        // SPARQL:
        //   SELECT ?id ?node ?p ?o
        //   FROM <urn:visionflow:graph:knowledge>
        //   WHERE { ?node a vc:KGNode ; vc:nodeId ?id ; ?p ?o }
        //   ORDER BY ?id
        // Group-by-?id, fold each group into one Node value, key the
        // resulting HashMap by ?id (u32).
        todo!(
            "SPARQL: SELECT ?id ?node ?p ?o FROM <{GRAPH_KNOWLEDGE}> ORDER BY ?id; \
             group into HashMap<u32, Node>"
        )
    }

    async fn get_physics_state(&self) -> RepoResult<PhysicsState> {
        // Physics state is volatile and lives in the actor; the adapter
        // never returns it from disk in the destination architecture
        // (ADR-11 §D4). This implementation returns
        // `PhysicsState::default()` and is expected to be overridden by
        // an upstream supervisor when needed.
        Ok(PhysicsState::default())
    }

    async fn get_node_positions(&self) -> RepoResult<Vec<(u32, Vec3)>> {
        // SPARQL (cold-start position load; ADR-11 §D4):
        //   SELECT ?id ?x ?y ?z
        //   FROM <urn:visionflow:graph:knowledge>
        //   WHERE {
        //     ?node a vc:KGNode ;
        //           vc:nodeId ?id ;
        //           vc:hasX ?x ;
        //           vc:hasY ?y ;
        //           vc:hasZ ?z .
        //   }
        //   ORDER BY ?id
        todo!(
            "SPARQL: SELECT ?id ?x ?y ?z FROM <{GRAPH_KNOWLEDGE}> WHERE \
             {{ ?node a vc:KGNode ; vc:nodeId ?id ; vc:hasX ?x ; vc:hasY ?y ; vc:hasZ ?z }}"
        )
    }

    async fn get_bots_graph(&self) -> RepoResult<Arc<GraphData>> {
        // SPARQL: same shape as get_graph, but against
        //   <urn:visionflow:graph:agent>
        // Section 7 (Bots & Agent Telemetry) drives the schema in this
        // named graph; this adapter just enumerates triples.
        todo!(
            "SPARQL: SELECT ?node ?p ?o FROM <{GRAPH_AGENT}> WHERE {{ ?node ?p ?o }}; \
             fold into GraphData with agent-tier classification"
        )
    }

    async fn get_constraints(&self) -> RepoResult<ConstraintSet> {
        // Constraints are owned by the constraint-set actor (Section 1).
        // Persisted constraints (cold start) live as triples under
        //   GRAPH <urn:visionflow:graph:knowledge> { ?c a vc:Constraint ; ... }
        // but loading them is a Phase 2 task. Default to empty.
        Ok(ConstraintSet::default())
    }

    async fn get_auto_balance_notifications(&self) -> RepoResult<Vec<AutoBalanceNotification>> {
        // Notifications are volatile (in-memory ring buffer). The adapter
        // returns empty here; the actor layer surfaces live notifications
        // through a different channel.
        Ok(Vec::new())
    }

    async fn get_equilibrium_status(&self) -> RepoResult<bool> {
        // Equilibrium is a runtime signal owned by PhysicsOrchestratorActor.
        // Cold-start default is `false`.
        Ok(false)
    }

    async fn compute_shortest_paths(&self, _params: PathfindingParams) -> RepoResult<PathfindingResult> {
        // SPARQL property-path traversal:
        //   SELECT ?path WHERE {
        //     <vc:kg/<start>> (vc:bridgeTo|vc:edge/predicate)+ <vc:kg/<end>>
        //   }
        // Oxigraph supports property paths natively. This is a candidate
        // for delegation to a CPU/GPU SSSP kernel rather than SPARQL —
        // the SPARQL form is only viable for small graphs. Phase 2
        // decision; trait default is unimplemented.
        Err(GraphRepositoryError::NotImplemented)
    }

    async fn get_dirty_nodes(&self) -> RepoResult<HashSet<u32>> {
        // See `clear_dirty_nodes` rationale. The adapter has no notion of
        // dirtiness; return empty.
        Ok(HashSet::new())
    }
}
