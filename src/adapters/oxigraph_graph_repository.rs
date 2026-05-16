// src/adapters/oxigraph_graph_repository.rs
//! Oxigraph Graph Repository Adapter (Phase 11 — Phase 1 implementation).
//!
//! Implements [`GraphRepository`] over the same Oxigraph store as
//! [`OxigraphOntologyRepository`], but operates on the
//! `<urn:visionflow:graph:knowledge>` and `<urn:visionflow:graph:agent>`
//! named graphs (ADR-11 §D2).
//!
//! ## Named graph routing (ADR-11 §D2 + T1-class-bits resolution)
//!
//! Each `Node` carries a 32-bit id whose high 6 bits encode a class:
//! `0x80000000 = Agent`, `0x40000000 = Page/Knowledge`,
//! `0x1C000000` mask = OntologyClass / LinkedPage / Axiom /
//! OntologyProperty.
//!
//! - Agent-flagged nodes are written to `<urn:visionflow:graph:agent>`.
//! - All other nodes (Knowledge, Ontology subtypes, unclassified) are
//!   written to `<urn:visionflow:graph:knowledge>`.
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

#![cfg(feature = "persistence-oxigraph")]

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::Vec3;
use oxigraph::sparql::QueryResults;
use oxigraph::store::{StorageError, Store};

use crate::actors::graph_actor::{AutoBalanceNotification, PhysicsState};
use crate::models::constraints::ConstraintSet;
use crate::models::edge::Edge;
use crate::models::graph::GraphData;
use crate::models::node::Node;
use crate::ports::graph_repository::{
    BinaryNodeData, GraphRepository, GraphRepositoryError, PathfindingParams,
    PathfindingResult, Result as RepoResult,
};
use crate::utils::socket_flow_messages::BinaryNodeData as RtBinaryNodeData;

// Re-use the named-graph constants from the ontology adapter; both modules
// live in the same crate and the IRIs are dataset-wide.
use crate::adapters::oxigraph_ontology_repository::{
    GRAPH_AGENT, GRAPH_KNOWLEDGE,
};

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
    format!("urn:visionflow:node:{}", id)
}

/// Mint the canonical edge IRI. The predicate-hash component is the
/// edge's stored `id` field (already in the `<source>-<target>` form for
/// the default `Edge::new` constructor, or whatever the upstream caller
/// chose). Distinct from the node IRI scheme on purpose.
#[inline]
fn edge_iri(edge: &Edge) -> String {
    format!(
        "urn:visionflow:edge:{}:{}:{}",
        edge.source, edge.target, edge.id
    )
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
        "PREFIX vc: <https://visionflow.dreamlab/ns/>\n",
        "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n",
        "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>\n",
    );
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

        // Build one INSERT DATA per node-batch. Splitting per named graph
        // is mandatory because INSERT DATA does not allow GRAPH ... WHERE
        // mixing; we instead emit one GRAPH block per node, which is
        // legal inside a single INSERT DATA.
        let mut update = String::from(Self::PROLOGUE);
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
        let mut bridge_edges: Vec<Edge> = Vec::new();

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
        if !bridge_edges.is_empty() {
            let prologue = Self::PROLOGUE.to_string();
            let bridges = bridge_edges.clone();
            let ask_result: RepoResult<()> =
                tokio::task::spawn_blocking(move || -> RepoResult<()> {
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
                            QueryResults::Boolean(true) => continue,
                            QueryResults::Boolean(false) => {
                                return Err(GraphRepositoryError::InvalidData(format!(
                                    "bridge edge {}->{}: endpoint not present in store",
                                    edge.source, edge.target
                                )));
                            }
                            _ => {
                                return Err(GraphRepositoryError::AccessError(
                                    "ASK returned non-boolean result".to_string(),
                                ));
                            }
                        }
                    }
                    Ok(())
                })
                .await
                .map_err(|e| GraphRepositoryError::AccessError(format!("join error: {e}")))?;
            ask_result?;
        }

        // Build a single INSERT DATA that places same-graph edges in
        // their owning named graph and bridge edges in the default graph.
        let mut update = String::from(Self::PROLOGUE);
        update.push_str("INSERT DATA {\n");

        for (edge, graph) in &same_graph_edges {
            let iri = edge_iri(edge);
            let src = node_iri(edge.source);
            let tgt = node_iri(edge.target);
            let etype = escape_literal(
                edge.edge_type.as_deref().unwrap_or("default"),
            );

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
            let etype = escape_literal(
                edge.edge_type.as_deref().unwrap_or("bridge_to"),
            );
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

    async fn update_positions(
        &self,
        updates: Vec<(u32, BinaryNodeData)>,
    ) -> RepoResult<()> {
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
                            insert.push_str(&format!(
                                "    <{iri}> vc:hasX \"{x}\"^^xsd:float .\n",
                            ));
                            insert.push_str(&format!(
                                "    <{iri}> vc:hasY \"{y}\"^^xsd:float .\n",
                            ));
                            insert.push_str(&format!(
                                "    <{iri}> vc:hasZ \"{z}\"^^xsd:float .\n",
                            ));
                            insert.push_str(&format!(
                                "    <{iri}> vc:velX \"{vx}\"^^xsd:float .\n",
                            ));
                            insert.push_str(&format!(
                                "    <{iri}> vc:velY \"{vy}\"^^xsd:float .\n",
                            ));
                            insert.push_str(&format!(
                                "    <{iri}> vc:velZ \"{vz}\"^^xsd:float .\n",
                            ));
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
        let map: HashMap<u32, Node> =
            graph.nodes.iter().map(|n| (n.id, n.clone())).collect();
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
                            let id = sol
                                .get("id")
                                .and_then(term_to_u32)
                                .ok_or_else(|| {
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

    let vc = "https://visionflow.dreamlab/ns/";
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
            },
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
            node_type: None,
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
        "{p}SELECT ?edge ?src ?tgt ?weight ?etype WHERE {{\n  \
         GRAPH <{graph}> {{\n    \
         ?edge a vc:KGEdge .\n    \
         ?edge vc:source ?src .\n    \
         ?edge vc:target ?tgt .\n    \
         OPTIONAL {{ ?edge vc:weight ?weight }} .\n    \
         OPTIONAL {{ ?edge vc:relationshipType ?etype }} .\n  \
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
        let weight = sol
            .get("weight")
            .and_then(term_to_f32)
            .unwrap_or(1.0);
        let etype = sol.get("etype").and_then(term_to_string);

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
    iri.strip_prefix("urn:visionflow:node:")
        .and_then(|tail| tail.parse::<u32>().ok())
}
