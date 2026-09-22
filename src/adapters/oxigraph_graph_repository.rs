// src/adapters/oxigraph_graph_repository.rs
//! Oxigraph Graph Repository Adapter (Phase 11 — Phase 1 implementation).
//!
//! Implements [`GraphRepository`] over the same Oxigraph store as
//! [`OxigraphOntologyRepository`], but operates on the
//! `<urn:ngm:graph:knowledge>` and `<urn:ngm:graph:agent>`
//! named graphs (ADR-11 §D2).
//!
//! ## Named graph routing (ADR-11 §D2 + T1-class-bits resolution)
//!
//! Each `Node` carries a 32-bit id whose high 6 bits encode a class:
//! `0x80000000 = Agent`, `0x40000000 = Page/Knowledge`,
//! `0x1C000000` mask = OntologyClass / LinkedPage / Axiom /
//! OntologyProperty.
//!
//! - Agent-flagged nodes are written to `<urn:ngm:graph:agent>`.
//! - All other nodes (Knowledge, Ontology subtypes, unclassified) are
//!   written to `<urn:ngm:graph:knowledge>`.
//!
//! Cross-graph edges (one endpoint Agent, other endpoint Knowledge) are
//! written into the **default graph** with an integrity ASK guard that
//! both endpoints exist (ADR-11 §D6 bridge invariant).
//!
//! ## Position semantics (ADR-11 §D4)
//!
//! Live physics positions live in `GraphStateActor` RAM. The
//! [`GraphRepository::update_positions`] method is the snapshot path,
//! materialising each position as the triple-cluster
//! `vc:hasX/Y/Z/vc:velX/Y/Z`. Atomic DELETE-then-INSERT per id; multi-id
//! batches are wrapped in `Store::transaction` so that all positions
//! land or none do (DDD-11 invariant). Above ~5_000 ids the batch is
//! split into multiple transactions to fit Oxigraph SPARQL parser
//! limits — the cross-batch atomicity is documented as best-effort in
//! PORTS-AUDIT §"top-3 highest-risk".

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::Vec3;
use oxigraph::sparql::QueryResults;
use oxigraph::store::{StorageError, Store};

use crate::actors::graph_actor::{AutoBalanceNotification, PhysicsState};
use crate::ports::graph_repository::{
    BinaryNodeData, GraphRepository, GraphRepositoryError, PathfindingParams, PathfindingResult,
    Result as RepoResult,
};
use crate::utils::socket_flow_messages::BinaryNodeData as RtBinaryNodeData;
use visionclaw_domain::models::constraints::ConstraintSet;
use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;

// Re-use the named-graph constants from the ontology adapter; both modules
// live in the same crate and the IRIs are dataset-wide.
use crate::adapters::oxigraph_ontology_repository::{GRAPH_AGENT, GRAPH_KNOWLEDGE};

// ----------------------------------------------------------------------
// Class-bit routing (T1-class-bits.md). The wire flag bits are reproduced
// here to avoid pulling in the binary_protocol module from a persistence
// adapter — the constants are part of the persisted ID surface.
// ----------------------------------------------------------------------

const AGENT_NODE_FLAG: u32 = 0x80000000;
const KNOWLEDGE_NODE_FLAG: u32 = 0x40000000;

/// Maximum nodes per `update_positions` SPARQL Update. Above this the
/// adapter splits the batch into multiple transactions (see module docs).
const POSITION_UPDATE_CHUNK: usize = 5_000;

/// Pick the named-graph IRI a node should land in based on its class bits.
#[inline]
fn graph_for_node_id(id: u32) -> &'static str {
    if (id & AGENT_NODE_FLAG) != 0 {
        GRAPH_AGENT
    } else {
        // Knowledge (0x40000000), any ontology subtype (mask 0x1C000000),
        // and unclassified ids all live in the knowledge graph.
        GRAPH_KNOWLEDGE
    }
}

/// Mint the canonical node IRI. Uses the full 32-bit id (class bits
/// included) so the IRI round-trips losslessly to `NodeId`.
#[inline]
fn node_iri(id: u32) -> String {
    format!("urn:ngm:node:{}", id)
}

/// Mint the canonical edge IRI. The predicate-hash component is the
/// edge's stored `id` field (already in the `<source>-<target>` form for
/// the default `Edge::new` constructor, or whatever the upstream caller
/// chose). Distinct from the node IRI scheme on purpose.
#[inline]
fn edge_iri(edge: &Edge) -> String {
    format!("urn:ngm:edge:{}:{}:{}", edge.source, edge.target, edge.id)
}

/// Escape a string literal for embedding in a SPARQL literal. Backslash
/// and double-quote are the only sequences that can break the lexical
/// form for the simple `"..."^^xsd:string` literals we emit; newlines are
/// rare in metadata values but mapped to `\\n` for safety.
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

/// Convert an Oxigraph storage/eval error into the port's error variant.
fn access<E: std::fmt::Display>(e: E) -> GraphRepositoryError {
    GraphRepositoryError::AccessError(e.to_string())
}

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

    /// Build the SPARQL Update fragment for a single node. The node lands
    /// in the named graph picked by its class bits.
    fn node_insert_block(node: &Node) -> String {
        let iri = node_iri(node.id);
        let graph = graph_for_node_id(node.id);

        let rdf_type = if (node.id & AGENT_NODE_FLAG) != 0 {
            "vc:Agent"
        } else if (node.id & KNOWLEDGE_NODE_FLAG) != 0 {
            "vc:KnowledgeNode"
        } else {
            // Ontology subtypes + unclassified all map to OntologyClass
            // for the storage type triple; richer class metadata is
            // written by OxigraphOntologyRepository in a separate graph.
            "vc:OntologyClass"
        };

        let label = escape_literal(&node.label);
        let metadata_id = escape_literal(&node.metadata_id);

        let mut buf = String::with_capacity(512);
        buf.push_str("  GRAPH <");
        buf.push_str(graph);
        buf.push_str("> {\n");

        buf.push_str(&format!("    <{iri}> a {rdf_type} .\n"));
        buf.push_str(&format!(
            "    <{iri}> vc:nodeId \"{id}\"^^xsd:integer .\n",
            iri = iri,
            id = node.id
        ));
        buf.push_str(&format!(
            "    <{iri}> rdfs:label \"{label}\" .\n",
            iri = iri,
            label = label
        ));
        buf.push_str(&format!(
            "    <{iri}> vc:metadataId \"{metadata_id}\" .\n",
            iri = iri,
            metadata_id = metadata_id
        ));

        // Position
        buf.push_str(&format!(
            "    <{iri}> vc:hasX \"{x}\"^^xsd:float .\n",
            iri = iri,
            x = node.data.x
        ));
        buf.push_str(&format!(
            "    <{iri}> vc:hasY \"{y}\"^^xsd:float .\n",
            iri = iri,
            y = node.data.y
        ));
        buf.push_str(&format!(
            "    <{iri}> vc:hasZ \"{z}\"^^xsd:float .\n",
            iri = iri,
            z = node.data.z
        ));

        // Velocity
        buf.push_str(&format!(
            "    <{iri}> vc:velX \"{vx}\"^^xsd:float .\n",
            iri = iri,
            vx = node.data.vx
        ));
        buf.push_str(&format!(
            "    <{iri}> vc:velY \"{vy}\"^^xsd:float .\n",
            iri = iri,
            vy = node.data.vy
        ));
        buf.push_str(&format!(
            "    <{iri}> vc:velZ \"{vz}\"^^xsd:float .\n",
            iri = iri,
            vz = node.data.vz
        ));

        // Mass (optional)
        if let Some(m) = node.mass {
            buf.push_str(&format!(
                "    <{iri}> vc:mass \"{m}\"^^xsd:float .\n",
                iri = iri,
                m = m
            ));
        }

        // owl_class_iri (optional) — links a KG node to its ontology class
        if let Some(owl) = &node.owl_class_iri {
            buf.push_str(&format!(
                "    <{iri}> vc:owlClass <{owl}> .\n",
                iri = iri,
                owl = owl
            ));
        }

        // node_type (optional)
        if let Some(nt) = &node.node_type {
            let nt_esc = escape_literal(nt);
            buf.push_str(&format!(
                "    <{iri}> vc:nodeType \"{nt}\" .\n",
                iri = iri,
                nt = nt_esc
            ));
        }

        // Free-form metadata key/values
        for (k, v) in &node.metadata {
            let k_esc = escape_literal(k);
            let v_esc = escape_literal(v);
            buf.push_str(&format!(
                "    <{iri}> vc:meta \"{k}={v}\" .\n",
                iri = iri,
                k = k_esc,
                v = v_esc
            ));
        }

        buf.push_str("  }\n");
        buf
    }

    /// SPARQL prologue applied to every UPDATE/QUERY string this adapter
    /// emits. Kept inline (no PREFIX) for cold-path simplicity; Oxigraph
    /// has no measurable benefit from PREFIX vs full IRIs.
    const PROLOGUE: &'static str = concat!(
        "PREFIX vc: <https://narrativegoldmine.com/ns/v1#>\n",
        "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n",
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>\n",
    );

    // ------------------------------------------------------------------
    // Bridge-edge garbage collection (T1 fix)
    // ------------------------------------------------------------------

    /// Delete all bridge edges in the default graph whose source or target
    /// node is no longer present in either the knowledge or agent named
    /// graph. Returns the number of triples removed.
    ///
    /// This is a best-effort cleanup: callers such as `clear_graph` and a
    /// future startup hook invoke this to prevent the default graph from
    /// accumulating stale cross-graph edges over time.
    pub async fn bridge_edge_gc(&self) -> RepoResult<usize> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || -> RepoResult<usize> {
            // Count triples in the default graph before the DELETE so we
            // can return a meaningful delta without a separate SELECT.
            let count_before_q = format!(
                "{p}SELECT (COUNT(*) AS ?n) WHERE {{\n  \
                 ?s ?p ?o .\n  \
                 FILTER (STRSTARTS(STR(?s), \"urn:ngm:edge:\"))\n}}",
                p = Self::PROLOGUE,
            );
            let before: usize = match store.query(count_before_q.as_str()).map_err(access)? {
                QueryResults::Solutions(mut sols) => {
                    if let Some(Ok(row)) = sols.next() {
                        row.get("n")
                            .and_then(|t| {
                                if let oxigraph::model::Term::Literal(lit) = t {
                                    lit.value().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0)
                    } else {
                        0
                    }
                }
                _ => 0,
            };

            // DELETE triples for bridge edges whose source or target has
            // no vc:nodeId in either named graph. We use NOT EXISTS across
            // both named graphs to handle partial deletions gracefully.
            let delete_q = format!(
                concat!(
                    "{p}",
                    "DELETE {{ ?edge_s ?edge_p ?edge_o }}\n",
                    "WHERE {{\n",
                    "  ?edge_s ?edge_p ?edge_o .\n",
                    "  FILTER (STRSTARTS(STR(?edge_s), \"urn:ngm:edge:\"))\n",
                    "  {{ ?edge_s vc:source ?src }} UNION {{ ?edge_s vc:target ?src }}\n",
                    "  FILTER NOT EXISTS {{\n",
                    "    {{ GRAPH <{kg}> {{ ?src vc:nodeId ?_a }} }}\n",
                    "    UNION\n",
                    "    {{ GRAPH <{ag}> {{ ?src vc:nodeId ?_a }} }}\n",
                    "  }}\n",
                    "}}\n",
                ),
                p = Self::PROLOGUE,
                kg = GRAPH_KNOWLEDGE,
                ag = GRAPH_AGENT,
            );

            store.update(delete_q.as_str()).map_err(access)?;

            // Count remaining to compute delta.
            let count_after_q = format!(
                "{p}SELECT (COUNT(*) AS ?n) WHERE {{\n  \
                 ?s ?p ?o .\n  \
                 FILTER (STRSTARTS(STR(?s), \"urn:ngm:edge:\"))\n}}",
                p = Self::PROLOGUE,
            );
            let after: usize = match store.query(count_after_q.as_str()).map_err(access)? {
                QueryResults::Solutions(mut sols) => {
                    if let Some(Ok(row)) = sols.next() {
                        row.get("n")
                            .and_then(|t| {
                                if let oxigraph::model::Term::Literal(lit) = t {
                                    lit.value().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0)
                    } else {
                        0
                    }
                }
                _ => 0,
            };

            Ok(before.saturating_sub(after))
        })
        .await
        .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))?
    }
}

#[async_trait]
impl GraphRepository for OxigraphGraphRepository {
    // ------------------------------------------------------------------
    // Write path
    // ------------------------------------------------------------------

    async fn add_nodes(&self, nodes: Vec<Node>) -> RepoResult<Vec<u32>> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        // Upsert semantics: drop each node's existing triples before the
        // INSERT so a re-synced node replaces its prior state instead of
        // accumulating conflicting values (a node carrying both
        // `vc:meta "type=page"` and `vc:meta "type=ontology_node"` made
        // population classification a per-reload coin flip). The DELETE
        // WHERE sequence and the INSERT DATA run as one SPARQL update, so
        // the whole upsert is atomic per batch.
        //
        // Splitting per named graph is mandatory because INSERT DATA does
        // not allow GRAPH ... WHERE mixing; we instead emit one GRAPH
        // block per node, which is legal inside a single INSERT DATA.
        let mut update = String::from(Self::PROLOGUE);
        for node in &nodes {
            update.push_str(&format!(
                "DELETE WHERE {{ GRAPH <{graph}> {{ <{iri}> ?p ?o }} }} ;\n",
                graph = graph_for_node_id(node.id),
                iri = node_iri(node.id),
            ));
        }
        update.push_str("INSERT DATA {\n");
        for node in &nodes {
            update.push_str(&Self::node_insert_block(node));
        }
        update.push_str("}\n");

        let store = self.store.clone();
        let added_ids: Vec<u32> = nodes.iter().map(|n| n.id).collect();

        tokio::task::spawn_blocking(move || -> RepoResult<()> {
            store.update(update.as_str()).map_err(access)?;
            Ok(())
        })
        .await
        .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))??;

        Ok(added_ids)
    }

    async fn add_edges(&self, edges: Vec<Edge>) -> RepoResult<Vec<String>> {
        if edges.is_empty() {
            return Ok(Vec::new());
        }

        let store_for_ask = self.store.clone();
        let store_for_update = self.store.clone();

        // Classify each edge by whether its endpoints sit in the same
        // named graph. Same-graph edges are written into that graph;
        // cross-graph (bridge) edges land in the default graph, after
        // verifying both endpoints exist via an ASK guard.
        let mut same_graph_edges: Vec<(Edge, &'static str)> = Vec::with_capacity(edges.len());
        let mut bridge_edges: Vec<Edge> = Vec::new(); // may be filtered in-place by ASK guard below

        for edge in &edges {
            let src_graph = graph_for_node_id(edge.source);
            let tgt_graph = graph_for_node_id(edge.target);
            if src_graph == tgt_graph {
                same_graph_edges.push((edge.clone(), src_graph));
            } else {
                bridge_edges.push(edge.clone());
            }
        }

        // Run bridge integrity ASK queries (one per bridge edge). The ASK
        // checks both endpoints are present somewhere in the dataset; we
        // do not constrain which graph, since vc:nodeId is asserted in
        // whichever graph the node was inserted into.
        //
        // T1 fix: a missing endpoint now logs a warning and skips the
        // bridge rather than aborting the entire batch. Valid bridges are
        // still written. Skipped count is logged as a summary at the end.
        if !bridge_edges.is_empty() {
            let prologue = Self::PROLOGUE.to_string();
            let bridges = bridge_edges.clone();
            let valid_bridges: RepoResult<Vec<Edge>> =
                tokio::task::spawn_blocking(move || -> RepoResult<Vec<Edge>> {
                    let mut kept: Vec<Edge> = Vec::with_capacity(bridges.len());
                    let mut skipped: usize = 0;
                    for edge in &bridges {
                        let q = format!(
                            "{p}ASK {{\n  {{ <{src_iri}> vc:nodeId ?_a }} UNION \
                             {{ GRAPH ?g1 {{ <{src_iri}> vc:nodeId ?_a }} }}\n  \
                             {{ <{tgt_iri}> vc:nodeId ?_b }} UNION \
                             {{ GRAPH ?g2 {{ <{tgt_iri}> vc:nodeId ?_b }} }}\n}}",
                            p = prologue,
                            src_iri = node_iri(edge.source),
                            tgt_iri = node_iri(edge.target),
                        );
                        match store_for_ask.query(q.as_str()).map_err(access)? {
                            QueryResults::Boolean(true) => kept.push(edge.clone()),
                            QueryResults::Boolean(false) => {
                                log::warn!(
                                    "bridge edge {}->{}: endpoint(s) not present in store — skipping",
                                    edge.source, edge.target
                                );
                                skipped += 1;
                            }
                            _ => {
                                return Err(GraphRepositoryError::AccessError(
                                    "ASK returned non-boolean result".to_string(),
                                ));
                            }
                        }
                    }
                    if skipped > 0 {
                        log::warn!(
                            "add_edges: skipped {} of {} bridge edge(s) due to missing endpoints; {} will be written",
                            skipped,
                            bridges.len(),
                            kept.len()
                        );
                    }
                    Ok(kept)
                })
                .await
                .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))?;
            bridge_edges = valid_bridges?;
        }

        // Build a single update that places same-graph edges in their
        // owning named graph and bridge edges in the default graph. Each
        // edge's existing triples are dropped first (upsert) so re-synced
        // edges replace prior state instead of accumulating conflicting
        // weight/relationshipType values across syncs.
        let mut update = String::from(Self::PROLOGUE);
        for (edge, graph) in &same_graph_edges {
            update.push_str(&format!(
                "DELETE WHERE {{ GRAPH <{graph}> {{ <{iri}> ?p ?o }} }} ;\n",
                graph = graph,
                iri = edge_iri(edge),
            ));
        }
        for edge in &bridge_edges {
            update.push_str(&format!(
                "DELETE WHERE {{ <{iri}> ?p ?o }} ;\n",
                iri = edge_iri(edge),
            ));
        }
        update.push_str("INSERT DATA {\n");

        for (edge, graph) in &same_graph_edges {
            let iri = edge_iri(edge);
            let src = node_iri(edge.source);
            let tgt = node_iri(edge.target);
            let etype = escape_literal(edge.edge_type.as_deref().unwrap_or("default"));

            update.push_str("  GRAPH <");
            update.push_str(graph);
            update.push_str("> {\n");
            update.push_str(&format!("    <{iri}> a vc:KGEdge .\n"));
            update.push_str(&format!("    <{iri}> vc:source <{src}> .\n"));
            update.push_str(&format!("    <{iri}> vc:target <{tgt}> .\n"));
            update.push_str(&format!(
                "    <{iri}> vc:weight \"{w}\"^^xsd:float .\n",
                w = edge.weight
            ));
            update.push_str(&format!(
                "    <{iri}> vc:relationshipType \"{etype}\" .\n",
                etype = etype
            ));
            if let Some(owl) = &edge.owl_property_iri {
                update.push_str(&format!(
                    "    <{iri}> vc:owlProperty <{owl}> .\n",
                    owl = owl
                ));
            }
            update.push_str("  }\n");
        }

        for edge in &bridge_edges {
            // Default graph: no GRAPH wrapper, triples are bare.
            let iri = edge_iri(edge);
            let src = node_iri(edge.source);
            let tgt = node_iri(edge.target);
            let etype = escape_literal(edge.edge_type.as_deref().unwrap_or("bridge_to"));
            update.push_str(&format!("  <{iri}> a vc:BridgeEdge .\n"));
            update.push_str(&format!("  <{iri}> vc:source <{src}> .\n"));
            update.push_str(&format!("  <{iri}> vc:target <{tgt}> .\n"));
            update.push_str(&format!(
                "  <{iri}> vc:weight \"{w}\"^^xsd:float .\n",
                w = edge.weight
            ));
            update.push_str(&format!(
                "  <{iri}> vc:relationshipType \"{etype}\" .\n",
                etype = etype
            ));
        }

        update.push_str("}\n");

        let returned: Vec<String> = same_graph_edges
            .iter()
            .map(|(e, _)| edge_iri(e))
            .chain(bridge_edges.iter().map(edge_iri))
            .collect();

        tokio::task::spawn_blocking(move || -> RepoResult<()> {
            store_for_update.update(update.as_str()).map_err(access)?;
            Ok(())
        })
        .await
        .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))??;

        Ok(returned)
    }

    async fn update_positions(&self, updates: Vec<(u32, BinaryNodeData)>) -> RepoResult<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Chunk above POSITION_UPDATE_CHUNK to fit Oxigraph's SPARQL
        // parser memory profile. Each chunk runs inside a single
        // `Store::transaction` so the DELETE+INSERT pair is atomic per
        // chunk (ADR-11 §D9, DDD-11 invariant).
        for chunk in updates.chunks(POSITION_UPDATE_CHUNK) {
            let chunk_owned: Vec<(u32, BinaryNodeData)> = chunk.to_vec();
            let store = self.store.clone();

            tokio::task::spawn_blocking(move || -> RepoResult<()> {
                store
                    .transaction(|mut tx| -> Result<(), TxError> {
                        // Split DELETE and INSERT into separate Update
                        // statements within the transaction. Each id gets
                        // its own DELETE WHERE so velocities/positions are
                        // removed even if any of the six properties are
                        // currently missing (OPTIONAL semantics by
                        // emitting independent triples per property).
                        for (id, _data) in &chunk_owned {
                            let iri = node_iri(*id);
                            let graph = graph_for_node_id(*id);

                            // DELETE WHERE — six independent triple
                            // patterns; missing properties are no-ops.
                            let delete = format!(
                                "{p}DELETE {{ GRAPH <{graph}> {{\n  \
                                 <{iri}> vc:hasX ?_x .\n  \
                                 <{iri}> vc:hasY ?_y .\n  \
                                 <{iri}> vc:hasZ ?_z .\n  \
                                 <{iri}> vc:velX ?_vx .\n  \
                                 <{iri}> vc:velY ?_vy .\n  \
                                 <{iri}> vc:velZ ?_vz .\n}} }}\n\
                                 WHERE {{ GRAPH <{graph}> {{\n  \
                                 OPTIONAL {{ <{iri}> vc:hasX ?_x }}\n  \
                                 OPTIONAL {{ <{iri}> vc:hasY ?_y }}\n  \
                                 OPTIONAL {{ <{iri}> vc:hasZ ?_z }}\n  \
                                 OPTIONAL {{ <{iri}> vc:velX ?_vx }}\n  \
                                 OPTIONAL {{ <{iri}> vc:velY ?_vy }}\n  \
                                 OPTIONAL {{ <{iri}> vc:velZ ?_vz }}\n}} }}",
                                p = Self::PROLOGUE,
                                graph = graph,
                                iri = iri,
                            );
                            tx.update(delete.as_str())
                                .map_err(|e| TxError(e.to_string()))?;
                        }

                        // One INSERT DATA carrying every id in the chunk.
                        let mut insert = String::from(Self::PROLOGUE);
                        insert.push_str("INSERT DATA {\n");
                        for (id, data) in &chunk_owned {
                            let iri = node_iri(*id);
                            let graph = graph_for_node_id(*id);
                            let (x, y, z, vx, vy, vz) = *data;
                            insert.push_str("  GRAPH <");
                            insert.push_str(graph);
                            insert.push_str("> {\n");
                            insert
                                .push_str(&format!("    <{iri}> vc:hasX \"{x}\"^^xsd:float .\n",));
                            insert
                                .push_str(&format!("    <{iri}> vc:hasY \"{y}\"^^xsd:float .\n",));
                            insert
                                .push_str(&format!("    <{iri}> vc:hasZ \"{z}\"^^xsd:float .\n",));
                            insert
                                .push_str(&format!("    <{iri}> vc:velX \"{vx}\"^^xsd:float .\n",));
                            insert
                                .push_str(&format!("    <{iri}> vc:velY \"{vy}\"^^xsd:float .\n",));
                            insert
                                .push_str(&format!("    <{iri}> vc:velZ \"{vz}\"^^xsd:float .\n",));
                            insert.push_str("  }\n");
                        }
                        insert.push_str("}\n");
                        tx.update(insert.as_str())
                            .map_err(|e| TxError(e.to_string()))?;

                        Ok(())
                    })
                    .map_err(|e| match e {
                        TxError(msg) => GraphRepositoryError::AccessError(msg),
                    })?;
                Ok(())
            })
            .await
            .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))??;
        }

        Ok(())
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
        // Cold-start path: union of knowledge + agent named graphs.
        let store = self.store.clone();
        let nodes_edges: RepoResult<(Vec<Node>, Vec<Edge>)> =
            tokio::task::spawn_blocking(move || -> RepoResult<(Vec<Node>, Vec<Edge>)> {
                let mut nodes = load_nodes_in_graph(&store, GRAPH_KNOWLEDGE)?;
                let mut agent_nodes = load_nodes_in_graph(&store, GRAPH_AGENT)?;
                nodes.append(&mut agent_nodes);

                let mut edges = load_edges_in_graph(&store, GRAPH_KNOWLEDGE)?;
                let mut agent_edges = load_edges_in_graph(&store, GRAPH_AGENT)?;
                edges.append(&mut agent_edges);
                let mut bridge_edges = load_bridge_edges(&store)?;
                edges.append(&mut bridge_edges);

                Ok((nodes, edges))
            })
            .await
            .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))?;

        let (nodes, edges) = nodes_edges?;

        let graph = GraphData {
            nodes,
            edges,
            metadata: HashMap::new(),
            id_to_metadata: HashMap::new(),
        };
        Ok(Arc::new(graph))
    }

    async fn get_node_map(&self) -> RepoResult<Arc<HashMap<u32, Node>>> {
        let graph = self.get_graph().await?;
        let map: HashMap<u32, Node> = graph.nodes.iter().map(|n| (n.id, n.clone())).collect();
        Ok(Arc::new(map))
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
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || -> RepoResult<Vec<(u32, Vec3)>> {
            let mut out = Vec::new();
            for graph_iri in [GRAPH_KNOWLEDGE, GRAPH_AGENT] {
                let q = format!(
                    "{p}SELECT ?id ?x ?y ?z WHERE {{\n  \
                     GRAPH <{graph}> {{\n    \
                     ?node vc:nodeId ?id ;\n          \
                     vc:hasX ?x ;\n          \
                     vc:hasY ?y ;\n          \
                     vc:hasZ ?z .\n  }}\n}}",
                    p = Self::PROLOGUE,
                    graph = graph_iri,
                );
                match store.query(q.as_str()).map_err(access)? {
                    QueryResults::Solutions(iter) => {
                        for sol in iter {
                            let sol = sol.map_err(access)?;
                            let id = sol.get("id").and_then(term_to_u32).ok_or_else(|| {
                                GraphRepositoryError::DeserializationError(
                                    "missing or non-integer ?id".to_string(),
                                )
                            })?;
                            let x = sol.get("x").and_then(term_to_f32).unwrap_or(0.0);
                            let y = sol.get("y").and_then(term_to_f32).unwrap_or(0.0);
                            let z = sol.get("z").and_then(term_to_f32).unwrap_or(0.0);
                            out.push((id, Vec3::new(x, y, z)));
                        }
                    }
                    _ => {
                        return Err(GraphRepositoryError::AccessError(
                            "unexpected SPARQL result shape".to_string(),
                        ));
                    }
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))?
    }

    async fn get_bots_graph(&self) -> RepoResult<Arc<GraphData>> {
        let store = self.store.clone();
        let nodes_edges: RepoResult<(Vec<Node>, Vec<Edge>)> =
            tokio::task::spawn_blocking(move || -> RepoResult<(Vec<Node>, Vec<Edge>)> {
                let nodes = load_nodes_in_graph(&store, GRAPH_AGENT)?;
                let edges = load_edges_in_graph(&store, GRAPH_AGENT)?;
                Ok((nodes, edges))
            })
            .await
            .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))?;

        let (nodes, edges) = nodes_edges?;
        let graph = GraphData {
            nodes,
            edges,
            metadata: HashMap::new(),
            id_to_metadata: HashMap::new(),
        };
        Ok(Arc::new(graph))
    }

    async fn get_constraints(&self) -> RepoResult<ConstraintSet> {
        // Constraints are owned by the constraint-set actor (Section 1).
        // Persisted constraints (cold start) live as triples under
        //   GRAPH <urn:ngm:graph:knowledge> { ?c a vc:Constraint ; ... }
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

    async fn compute_shortest_paths(
        &self,
        _params: PathfindingParams,
    ) -> RepoResult<PathfindingResult> {
        // SPARQL property-path traversal is feasible but slow on large
        // graphs; PORTS-AUDIT §3 documents this as the top-1 highest-risk
        // method. Phase 2 will delegate to a CPU/GPU SSSP kernel.
        Err(GraphRepositoryError::NotImplemented)
    }

    async fn get_dirty_nodes(&self) -> RepoResult<HashSet<u32>> {
        // See `clear_dirty_nodes` rationale. The adapter has no notion of
        // dirtiness; return empty.
        Ok(HashSet::new())
    }
}

// ----------------------------------------------------------------------
// KnowledgeGraphRepository implementation — bridges the fine-grained
// CQRS port to the Oxigraph SPARQL store.
// ----------------------------------------------------------------------

use crate::ports::knowledge_graph_repository::{
    GraphStatistics, KnowledgeGraphRepository, KnowledgeGraphRepositoryError, Result as KgResult,
};

fn kg_err(e: GraphRepositoryError) -> KnowledgeGraphRepositoryError {
    match e {
        GraphRepositoryError::NotFound => KnowledgeGraphRepositoryError::NotFound,
        GraphRepositoryError::InvalidData(s) => KnowledgeGraphRepositoryError::InvalidData(s),
        other => KnowledgeGraphRepositoryError::DatabaseError(other.to_string()),
    }
}

#[async_trait]
impl KnowledgeGraphRepository for OxigraphGraphRepository {
    async fn load_graph(&self) -> KgResult<Arc<GraphData>> {
        self.get_graph().await.map_err(kg_err)
    }

    async fn save_graph(&self, graph: &GraphData) -> KgResult<()> {
        self.clear_graph().await?;
        if !graph.nodes.is_empty() {
            self.batch_add_nodes(graph.nodes.clone()).await?;
        }
        if !graph.edges.is_empty() {
            self.batch_add_edges(graph.edges.clone()).await?;
        }
        Ok(())
    }

    async fn add_node(&self, node: &Node) -> KgResult<u32> {
        let ids = GraphRepository::add_nodes(self, vec![node.clone()])
            .await
            .map_err(kg_err)?;
        ids.into_iter()
            .next()
            .ok_or(KnowledgeGraphRepositoryError::DatabaseError(
                "add_nodes returned empty".into(),
            ))
    }

    async fn batch_add_nodes(&self, nodes: Vec<Node>) -> KgResult<Vec<u32>> {
        GraphRepository::add_nodes(self, nodes)
            .await
            .map_err(kg_err)
    }

    async fn batch_add_nodes_if_absent(&self, nodes: Vec<Node>) -> KgResult<Vec<u32>> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        // Single-writer architecture (ADR-11 §D1) makes the check-then-insert
        // pair race-free. One VALUES-constrained SELECT finds which candidate
        // ids already exist anywhere in the dataset; only the absent ones are
        // inserted. Existing nodes are left untouched — this is the stub path,
        // and a stub must never replace or pollute an authored node.
        let candidate_ids: Vec<u32> = nodes.iter().map(|n| n.id).collect();
        let values: String = candidate_ids
            .iter()
            .map(|id| format!("\"{id}\"^^xsd:integer"))
            .collect::<Vec<_>>()
            .join(" ");
        let exists_q = format!(
            "{p}SELECT DISTINCT ?id WHERE {{\n  \
             GRAPH ?g {{ ?s vc:nodeId ?id }}\n  \
             VALUES ?id {{ {values} }}\n}}",
            p = Self::PROLOGUE,
        );

        let store = self.store.clone();
        let existing: std::collections::HashSet<u32> =
            tokio::task::spawn_blocking(move || -> RepoResult<std::collections::HashSet<u32>> {
                let mut found = std::collections::HashSet::new();
                if let QueryResults::Solutions(sols) =
                    store.query(exists_q.as_str()).map_err(access)?
                {
                    for sol in sols {
                        let sol = sol.map_err(access)?;
                        if let Some(oxigraph::model::Term::Literal(lit)) = sol.get("id") {
                            if let Ok(id) = lit.value().parse::<u32>() {
                                found.insert(id);
                            }
                        }
                    }
                }
                Ok(found)
            })
            .await
            .map_err(|e| {
                kg_err(GraphRepositoryError::AccessError(format!(
                    "join error: {e}"
                )))
            })?
            .map_err(kg_err)?;

        let absent: Vec<Node> = nodes
            .into_iter()
            .filter(|n| !existing.contains(&n.id))
            .collect();
        if absent.is_empty() {
            return Ok(Vec::new());
        }
        GraphRepository::add_nodes(self, absent)
            .await
            .map_err(kg_err)
    }

    async fn update_node(&self, node: &Node) -> KgResult<()> {
        self.remove_node(node.id).await?;
        self.add_node(node).await?;
        Ok(())
    }

    async fn batch_update_nodes(&self, nodes: Vec<Node>) -> KgResult<()> {
        for node in &nodes {
            self.remove_node(node.id).await?;
        }
        self.batch_add_nodes(nodes).await?;
        Ok(())
    }

    async fn remove_node(&self, node_id: u32) -> KgResult<()> {
        let iri = node_iri(node_id);
        let graph = graph_for_node_id(node_id);
        let store = self.store.clone();
        let update = format!(
            "{p}DELETE WHERE {{ GRAPH <{graph}> {{ <{iri}> ?p ?o }} }}",
            p = Self::PROLOGUE,
            graph = graph,
            iri = iri,
        );
        tokio::task::spawn_blocking(move || store.update(update.as_str()).map_err(access))
            .await
            .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
            .map_err(kg_err)
    }

    async fn batch_remove_nodes(&self, node_ids: Vec<u32>) -> KgResult<()> {
        for id in node_ids {
            self.remove_node(id).await?;
        }
        Ok(())
    }

    async fn get_node(&self, node_id: u32) -> KgResult<Option<Node>> {
        let store = self.store.clone();
        let graph = graph_for_node_id(node_id);
        tokio::task::spawn_blocking(move || -> KgResult<Option<Node>> {
            let nodes = load_nodes_in_graph(&store, graph).map_err(kg_err)?;
            Ok(nodes.into_iter().find(|n| n.id == node_id))
        })
        .await
        .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
    }

    async fn get_nodes(&self, node_ids: Vec<u32>) -> KgResult<Vec<Node>> {
        let graph = self.load_graph().await?;
        let id_set: HashSet<u32> = node_ids.into_iter().collect();
        Ok(graph
            .nodes
            .iter()
            .filter(|n| id_set.contains(&n.id))
            .cloned()
            .collect())
    }

    async fn get_nodes_by_metadata_id(&self, metadata_id: &str) -> KgResult<Vec<Node>> {
        let store = self.store.clone();
        let mid = metadata_id.to_string();
        tokio::task::spawn_blocking(move || -> KgResult<Vec<Node>> {
            let mut out = Vec::new();
            for graph_iri in [GRAPH_KNOWLEDGE, GRAPH_AGENT] {
                let nodes = load_nodes_in_graph(&store, graph_iri).map_err(kg_err)?;
                out.extend(nodes.into_iter().filter(|n| n.metadata_id == mid));
            }
            Ok(out)
        })
        .await
        .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
    }

    async fn get_nodes_by_owl_class_iri(&self, owl_class_iri: &str) -> KgResult<Vec<Node>> {
        let store = self.store.clone();
        let iri = owl_class_iri.to_string();
        tokio::task::spawn_blocking(move || -> KgResult<Vec<Node>> {
            let mut out = Vec::new();
            for graph_iri in [GRAPH_KNOWLEDGE, GRAPH_AGENT] {
                let nodes = load_nodes_in_graph(&store, graph_iri).map_err(kg_err)?;
                out.extend(
                    nodes
                        .into_iter()
                        .filter(|n| n.owl_class_iri.as_deref() == Some(iri.as_str())),
                );
            }
            Ok(out)
        })
        .await
        .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
    }

    async fn search_nodes_by_label(&self, label: &str) -> KgResult<Vec<Node>> {
        let store = self.store.clone();
        let needle = label.to_lowercase();
        tokio::task::spawn_blocking(move || -> KgResult<Vec<Node>> {
            let mut out = Vec::new();
            for graph_iri in [GRAPH_KNOWLEDGE, GRAPH_AGENT] {
                let nodes = load_nodes_in_graph(&store, graph_iri).map_err(kg_err)?;
                out.extend(
                    nodes
                        .into_iter()
                        .filter(|n| n.label.to_lowercase().contains(&needle)),
                );
            }
            Ok(out)
        })
        .await
        .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
    }

    async fn add_edge(&self, edge: &Edge) -> KgResult<String> {
        let ids = GraphRepository::add_edges(self, vec![edge.clone()])
            .await
            .map_err(kg_err)?;
        ids.into_iter()
            .next()
            .ok_or(KnowledgeGraphRepositoryError::DatabaseError(
                "add_edges returned empty".into(),
            ))
    }

    async fn batch_add_edges(&self, edges: Vec<Edge>) -> KgResult<Vec<String>> {
        GraphRepository::add_edges(self, edges)
            .await
            .map_err(kg_err)
    }

    async fn update_edge(&self, edge: &Edge) -> KgResult<()> {
        self.remove_edge(&edge.id).await?;
        self.add_edge(edge).await?;
        Ok(())
    }

    async fn remove_edge(&self, edge_id: &str) -> KgResult<()> {
        // ADR-2021 — the mint site and the lookup site must agree. This method
        // used to build `urn:ngm:edge:<edge_id>`, a ONE-segment IRI, while
        // `edge_iri` writes the THREE-segment
        // `urn:ngm:edge:<source>:<target>:<id>`. No subject could ever match,
        // so every delete silently removed nothing. The two forms a caller can
        // hold — the full IRI handed back by a read, or the bare stored id from
        // `Edge::new` — are resolved by `uri::ngm::edge_lookup`.
        let prologue = Self::PROLOGUE;
        let (subject_pattern, filter) = match crate::uri::ngm::edge_lookup(edge_id) {
            crate::uri::ngm::EdgeLookup::Exact(iri) => (format!("<{iri}>"), String::new()),
            crate::uri::ngm::EdgeLookup::TrailingId(id) => (
                "?e".to_string(),
                format!(
                    "\n  FILTER(STRSTARTS(STR(?e), \"{prefix}\") && STRENDS(STR(?e), \":{id}\"))",
                    prefix = crate::uri::ngm::EDGE_PREFIX,
                    id = escape_literal(&id),
                ),
            ),
        };

        // Delete from default graph (bridge edges)
        let store = self.store.clone();
        let del_default = format!(
            "{prologue}DELETE {{ {subject_pattern} ?p ?o }} \
             WHERE {{ {subject_pattern} ?p ?o .{filter} }}"
        );
        tokio::task::spawn_blocking(move || store.update(del_default.as_str()).map_err(access))
            .await
            .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
            .map_err(kg_err)?;

        // Delete from named graphs
        for graph_iri in [GRAPH_KNOWLEDGE, GRAPH_AGENT] {
            let s = self.store.clone();
            let del = format!(
                "{prologue}DELETE {{ GRAPH <{graph_iri}> {{ {subject_pattern} ?p ?o }} }} \
                 WHERE {{ GRAPH <{graph_iri}> {{ {subject_pattern} ?p ?o }}{filter} }}"
            );
            tokio::task::spawn_blocking(move || s.update(del.as_str()).map_err(access))
                .await
                .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
                .map_err(kg_err)?;
        }
        Ok(())
    }

    async fn batch_remove_edges(&self, edge_ids: Vec<String>) -> KgResult<()> {
        for id in edge_ids {
            self.remove_edge(&id).await?;
        }
        Ok(())
    }

    async fn get_node_edges(&self, node_id: u32) -> KgResult<Vec<Edge>> {
        let graph = self.load_graph().await?;
        Ok(graph
            .edges
            .iter()
            .filter(|e| e.source == node_id || e.target == node_id)
            .cloned()
            .collect())
    }

    async fn get_edges_between(&self, source_id: u32, target_id: u32) -> KgResult<Vec<Edge>> {
        let graph = self.load_graph().await?;
        Ok(graph
            .edges
            .iter()
            .filter(|e| e.source == source_id && e.target == target_id)
            .cloned()
            .collect())
    }

    async fn batch_update_positions(&self, positions: Vec<(u32, f32, f32, f32)>) -> KgResult<()> {
        let updates: Vec<(u32, BinaryNodeData)> = positions
            .into_iter()
            .map(|(id, x, y, z)| (id, (x, y, z, 0.0, 0.0, 0.0)))
            .collect();
        GraphRepository::update_positions(self, updates)
            .await
            .map_err(kg_err)
    }

    async fn get_all_positions(&self) -> KgResult<HashMap<u32, (f32, f32, f32)>> {
        let positions = self.get_node_positions().await.map_err(kg_err)?;
        Ok(positions
            .into_iter()
            .map(|(id, v)| (id, (v.x, v.y, v.z)))
            .collect())
    }

    async fn query_nodes(&self, query: &str) -> KgResult<Vec<Node>> {
        self.search_nodes_by_label(query).await
    }

    async fn get_neighbors(&self, node_id: u32) -> KgResult<Vec<Node>> {
        let graph = self.load_graph().await?;
        let neighbor_ids: HashSet<u32> = graph
            .edges
            .iter()
            .filter_map(|e| {
                if e.source == node_id {
                    Some(e.target)
                } else if e.target == node_id {
                    Some(e.source)
                } else {
                    None
                }
            })
            .collect();
        Ok(graph
            .nodes
            .iter()
            .filter(|n| neighbor_ids.contains(&n.id))
            .cloned()
            .collect())
    }

    async fn get_statistics(&self) -> KgResult<GraphStatistics> {
        let graph = self.load_graph().await?;
        let node_count = graph.nodes.len();
        let edge_count = graph.edges.len();
        let avg_degree = if node_count > 0 {
            (2.0 * edge_count as f32) / node_count as f32
        } else {
            0.0
        };
        Ok(GraphStatistics {
            node_count,
            edge_count,
            average_degree: avg_degree,
            connected_components: 1,
            last_updated: chrono::Utc::now(),
        })
    }

    async fn clear_graph(&self) -> KgResult<()> {
        let store = self.store.clone();
        let drop_result = tokio::task::spawn_blocking(move || -> KgResult<()> {
            for graph_iri in [GRAPH_KNOWLEDGE, GRAPH_AGENT] {
                let drop = format!("DROP SILENT GRAPH <{}>", graph_iri);
                store
                    .update(drop.as_str())
                    .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(e.to_string()))?;
            }
            Ok(())
        })
        .await
        .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?;
        drop_result?;

        // T1 fix: after named graphs are gone, purge any bridge edges in the
        // default graph whose endpoints are now orphaned. Failure is logged
        // but does not propagate — clear_graph's primary contract is fulfilled.
        match self.bridge_edge_gc().await {
            Ok(n) if n > 0 => log::info!("clear_graph: removed {} stale bridge triple(s)", n),
            Ok(_) => {}
            Err(e) => log::warn!("clear_graph: bridge_edge_gc failed (non-fatal): {}", e),
        }
        Ok(())
    }

    async fn health_check(&self) -> KgResult<bool> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || -> KgResult<bool> {
            match store.query("ASK { ?s ?p ?o }") {
                Ok(_) => Ok(true),
                Err(e) => Err(KnowledgeGraphRepositoryError::DatabaseError(e.to_string())),
            }
        })
        .await
        .map_err(|e| KnowledgeGraphRepositoryError::DatabaseError(format!("join: {e}")))?
    }
}

// ----------------------------------------------------------------------
// Helpers: SELECT-then-fold for nodes/edges in a single named graph.
// These run on the blocking pool's thread (called from spawn_blocking).
// ----------------------------------------------------------------------

/// Internal error wrapper for the `Store::transaction` closure; the
/// transaction API requires `E: Error + From<StorageError>`.
#[derive(Debug)]
struct TxError(String);

impl std::fmt::Display for TxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TxError {}

impl From<StorageError> for TxError {
    fn from(e: StorageError) -> Self {
        TxError(e.to_string())
    }
}

fn term_to_u32(t: &oxigraph::model::Term) -> Option<u32> {
    match t {
        oxigraph::model::Term::Literal(lit) => lit.value().parse::<u32>().ok(),
        _ => None,
    }
}

fn term_to_f32(t: &oxigraph::model::Term) -> Option<f32> {
    match t {
        oxigraph::model::Term::Literal(lit) => lit.value().parse::<f32>().ok(),
        _ => None,
    }
}

fn term_to_string(t: &oxigraph::model::Term) -> Option<String> {
    match t {
        oxigraph::model::Term::Literal(lit) => Some(lit.value().to_string()),
        oxigraph::model::Term::NamedNode(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

/// SELECT all (?node ?p ?o) triples under the given named graph and fold
/// them into a `Vec<Node>` keyed on `vc:nodeId`.
fn load_nodes_in_graph(store: &Store, graph_iri: &str) -> RepoResult<Vec<Node>> {
    let prologue = OxigraphGraphRepository::PROLOGUE;
    let q = format!(
        "{p}SELECT ?node ?prop ?val WHERE {{\n  \
         GRAPH <{graph}> {{\n    \
         ?node vc:nodeId ?_id .\n    \
         ?node ?prop ?val .\n  }}\n}}",
        p = prologue,
        graph = graph_iri,
    );

    let res = store.query(q.as_str()).map_err(access)?;
    let solutions = match res {
        QueryResults::Solutions(iter) => iter,
        _ => {
            return Err(GraphRepositoryError::AccessError(
                "unexpected SPARQL result shape".to_string(),
            ))
        }
    };

    // Group (?prop, ?val) rows by ?node IRI then fold each group into a
    // Node value. Using IRI string as the grouping key keeps the
    // intermediate map small.
    let mut grouped: HashMap<String, Vec<(String, oxigraph::model::Term)>> = HashMap::new();
    for sol in solutions {
        let sol = sol.map_err(access)?;
        let node = match sol.get("node") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let prop = match sol.get("prop") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let val = match sol.get("val") {
            Some(t) => t.clone(),
            None => continue,
        };
        grouped.entry(node).or_default().push((prop, val));
    }

    let vc = "https://narrativegoldmine.com/ns/v1#";
    let rdfs_label = "http://www.w3.org/2000/01/rdf-schema#label";

    let mut out = Vec::with_capacity(grouped.len());
    for (_node_iri, props) in grouped {
        let mut id: u32 = 0;
        let mut metadata_id = String::new();
        let mut label = String::new();
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut z = 0.0f32;
        let mut vx = 0.0f32;
        let mut vy = 0.0f32;
        let mut vz = 0.0f32;
        let mut mass: Option<f32> = None;
        let mut owl_class_iri: Option<String> = None;
        let mut node_type: Option<String> = None;
        let mut metadata: HashMap<String, String> = HashMap::new();

        for (p, v) in props {
            let p_rest = p.strip_prefix(vc);
            match (p.as_str(), p_rest) {
                (s, _) if s == rdfs_label => {
                    if let Some(t) = term_to_string(&v) {
                        label = t;
                    }
                }
                (_, Some("nodeId")) => {
                    if let Some(n) = term_to_u32(&v) {
                        id = n;
                    }
                }
                (_, Some("metadataId")) => {
                    if let Some(t) = term_to_string(&v) {
                        metadata_id = t;
                    }
                }
                (_, Some("hasX")) => x = term_to_f32(&v).unwrap_or(0.0),
                (_, Some("hasY")) => y = term_to_f32(&v).unwrap_or(0.0),
                (_, Some("hasZ")) => z = term_to_f32(&v).unwrap_or(0.0),
                (_, Some("velX")) => vx = term_to_f32(&v).unwrap_or(0.0),
                (_, Some("velY")) => vy = term_to_f32(&v).unwrap_or(0.0),
                (_, Some("velZ")) => vz = term_to_f32(&v).unwrap_or(0.0),
                (_, Some("mass")) => mass = term_to_f32(&v),
                (_, Some("owlClass")) => {
                    if let oxigraph::model::Term::NamedNode(n) = &v {
                        owl_class_iri = Some(n.as_str().to_string());
                    }
                }
                (_, Some("nodeType")) => {
                    if let Some(t) = term_to_string(&v) {
                        node_type = Some(t);
                    }
                }
                (_, Some("meta")) => {
                    if let Some(s) = term_to_string(&v) {
                        if let Some((k, val)) = s.split_once('=') {
                            metadata.insert(k.to_string(), val.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        if id == 0 {
            // node without a nodeId triple is malformed; skip.
            continue;
        }

        let n = Node {
            id,
            metadata_id,
            label,
            data: RtBinaryNodeData {
                node_id: id,
                x,
                y,
                z,
                vx,
                vy,
                vz,
            }
            .into(),
            x: Some(x),
            y: Some(y),
            z: Some(z),
            vx: Some(vx),
            vy: Some(vy),
            vz: Some(vz),
            mass,
            owl_class_iri,
            metadata,
            file_size: 0,
            node_type,
            size: None,
            color: None,
            weight: None,
            group: None,
            user_data: None,
        };
        out.push(n);
    }

    Ok(out)
}

/// SELECT all reified `vc:KGEdge` rows in a single named graph and fold
/// them into a `Vec<Edge>`.
fn load_edges_in_graph(store: &Store, graph_iri: &str) -> RepoResult<Vec<Edge>> {
    let prologue = OxigraphGraphRepository::PROLOGUE;
    let q = format!(
        "{p}SELECT ?edge ?src ?tgt ?weight ?etype ?owl WHERE {{\n  \
         GRAPH <{graph}> {{\n    \
         ?edge a vc:KGEdge .\n    \
         ?edge vc:source ?src .\n    \
         ?edge vc:target ?tgt .\n    \
         OPTIONAL {{ ?edge vc:weight ?weight }} .\n    \
         OPTIONAL {{ ?edge vc:relationshipType ?etype }} .\n    \
         OPTIONAL {{ ?edge vc:owlProperty ?owl }} .\n  \
         }}\n}}",
        p = prologue,
        graph = graph_iri,
    );

    let res = store.query(q.as_str()).map_err(access)?;
    let solutions = match res {
        QueryResults::Solutions(iter) => iter,
        _ => {
            return Err(GraphRepositoryError::AccessError(
                "unexpected SPARQL result shape".to_string(),
            ))
        }
    };

    let mut out = Vec::new();
    for sol in solutions {
        let sol = sol.map_err(access)?;
        let edge_iri = match sol.get("edge") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let src = match sol.get("src") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let tgt = match sol.get("tgt") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let source = iri_to_node_id(&src).unwrap_or(0);
        let target = iri_to_node_id(&tgt).unwrap_or(0);
        let weight = sol.get("weight").and_then(term_to_f32).unwrap_or(1.0);
        let etype = sol.get("etype").and_then(term_to_string);
        // The typed predicate the edge was written with (`vc:owlProperty`),
        // so the assert-graph rebuild can re-emit `rdfs:subClassOf`,
        // `vc:requires`, … rather than collapsing every edge to one predicate.
        let owl_property_iri = match sol.get("owl") {
            Some(oxigraph::model::Term::NamedNode(n)) => Some(n.as_str().to_string()),
            _ => None,
        };

        out.push(Edge {
            id: edge_iri,
            source,
            target,
            weight,
            edge_type: etype,
            owl_property_iri,
            metadata: None,
        });
    }

    Ok(out)
}

/// SELECT bridge edges in the default graph (cross-graph edges per
/// ADR-11 §D6). Same shape as `load_edges_in_graph` but no GRAPH
/// wrapper and with the `vc:BridgeEdge` rdf:type.
fn load_bridge_edges(store: &Store) -> RepoResult<Vec<Edge>> {
    let prologue = OxigraphGraphRepository::PROLOGUE;
    let q = format!(
        "{p}SELECT ?edge ?src ?tgt ?weight ?etype WHERE {{\n  \
         ?edge a vc:BridgeEdge .\n  \
         ?edge vc:source ?src .\n  \
         ?edge vc:target ?tgt .\n  \
         OPTIONAL {{ ?edge vc:weight ?weight }} .\n  \
         OPTIONAL {{ ?edge vc:relationshipType ?etype }} .\n}}",
        p = prologue,
    );

    let res = store.query(q.as_str()).map_err(access)?;
    let solutions = match res {
        QueryResults::Solutions(iter) => iter,
        _ => {
            return Err(GraphRepositoryError::AccessError(
                "unexpected SPARQL result shape".to_string(),
            ))
        }
    };

    let mut out = Vec::new();
    for sol in solutions {
        let sol = sol.map_err(access)?;
        let edge_iri = match sol.get("edge") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let src = match sol.get("src") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let tgt = match sol.get("tgt") {
            Some(oxigraph::model::Term::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        let source = iri_to_node_id(&src).unwrap_or(0);
        let target = iri_to_node_id(&tgt).unwrap_or(0);
        let weight = sol.get("weight").and_then(term_to_f32).unwrap_or(1.0);
        let etype = sol
            .get("etype")
            .and_then(term_to_string)
            .or_else(|| Some("bridge_to".to_string()));

        out.push(Edge {
            id: edge_iri,
            source,
            target,
            weight,
            edge_type: etype,
            owl_property_iri: None,
            metadata: None,
        });
    }

    Ok(out)
}

/// Parse a node IRI back into its full `u32` id (class bits + sequence).
fn iri_to_node_id(iri: &str) -> Option<u32> {
    iri.strip_prefix("urn:ngm:node:")
        .and_then(|tail| tail.parse::<u32>().ok())
}

#[cfg(test)]
mod upsert_tests {
    use super::*;
    use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;

    fn test_node(id: u32, type_value: &str) -> Node {
        let mut node = Node::default();
        node.id = id;
        node.metadata_id = format!("node-{id}");
        node.label = format!("Node {id}");
        node.node_type = Some(type_value.to_string());
        node.metadata
            .insert("type".to_string(), type_value.to_string());
        node
    }

    fn repo() -> OxigraphGraphRepository {
        OxigraphGraphRepository::from_store(Arc::new(Store::new().expect("in-memory store")))
    }

    /// Re-adding a node must REPLACE its triples, not accumulate conflicting
    /// values. Regression: nodes carried both `vc:meta "type=page"` and
    /// `vc:meta "type=ontology_node"` after incremental re-syncs, making
    /// population classification a per-reload coin flip (28 of 196 authored
    /// pages survived in the knowledge graph).
    #[tokio::test]
    async fn re_added_node_replaces_prior_triples() {
        let repo = repo();

        KnowledgeGraphRepository::batch_add_nodes(&repo, vec![test_node(42, "page")])
            .await
            .expect("first add");
        KnowledgeGraphRepository::batch_add_nodes(&repo, vec![test_node(42, "ontology_node")])
            .await
            .expect("second add");

        // The store must hold exactly ONE type= meta triple for the node.
        let q = format!(
            "{p}SELECT (COUNT(?m) AS ?n) WHERE {{ GRAPH ?g {{ <{iri}> vc:meta ?m . \
             FILTER(STRSTARTS(?m, \"type=\")) }} }}",
            p = OxigraphGraphRepository::PROLOGUE,
            iri = node_iri(42),
        );
        let count = match repo.store().query(q.as_str()).expect("query") {
            QueryResults::Solutions(mut sols) => sols
                .next()
                .and_then(|r| r.ok())
                .and_then(|r| r.get("n").cloned())
                .and_then(|t| match t {
                    oxigraph::model::Term::Literal(l) => l.value().parse::<usize>().ok(),
                    _ => None,
                })
                .unwrap_or(0),
            _ => 0,
        };
        assert_eq!(count, 1, "duplicate type= meta triples must not accumulate");

        let graph = repo.load_graph().await.expect("load");
        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == 42)
            .expect("node present");
        assert_eq!(
            node.metadata.get("type").map(String::as_str),
            Some("ontology_node")
        );
        assert_eq!(node.node_type.as_deref(), Some("ontology_node"));
    }

    /// A stub written via the if-absent path must never overwrite an
    /// authored node, while genuinely new stubs are still written.
    #[tokio::test]
    async fn if_absent_never_overwrites_authored_node() {
        let repo = repo();

        KnowledgeGraphRepository::batch_add_nodes(&repo, vec![test_node(7, "page")])
            .await
            .expect("authored add");
        let written = repo
            .batch_add_nodes_if_absent(vec![
                test_node(7, "linked_page"),
                test_node(8, "linked_page"),
            ])
            .await
            .expect("if-absent add");

        assert_eq!(written, vec![8], "only the absent id is written");

        let graph = repo.load_graph().await.expect("load");
        let authored = graph.nodes.iter().find(|n| n.id == 7).expect("node 7");
        assert_eq!(
            authored.metadata.get("type").map(String::as_str),
            Some("page"),
            "stub must not replace the authored node"
        );
        assert!(graph.nodes.iter().any(|n| n.id == 8), "new stub written");
    }
}
