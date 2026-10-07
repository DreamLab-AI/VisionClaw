//! Graph State Actor - Refactored with Hexagonal Architecture
//!
//! This module implements a specialized actor focused exclusively on graph state management.
//! Now uses KnowledgeGraphRepository port for persistence operations.
//!
//! ## Hexagonal Architecture
//!
//! - **Port**: KnowledgeGraphRepository (in-memory interface)
//! - **Adapter**: UnifiedGraphRepository (unified database implementation)
//! - **Actor**: Maintains in-memory state and coordinates operations
//!
//! ## Core Responsibilities
//!
//! ### 1. Graph Data Management
//! - **Primary Graph**: Maintains the main graph data structure with nodes and edges
//! - **Node Map**: Provides efficient O(1) node lookup by ID
//! - **Bots Graph**: Manages separate graph data for agent visualization
//! - **Persistence**: Uses repository port for database operations
//!
//! ### 2. Node Operations (via Repository)
//! - **AddNode**: Add new nodes to the graph with proper ID management
//! - **RemoveNode**: Remove nodes and clean up associated edges
//! - **UpdateNodeFromMetadata**: Update existing nodes based on metadata changes
//!
//! ### 3. Edge Operations (via Repository)
//! - **AddEdge**: Create connections between nodes
//! - **RemoveEdge**: Remove specific edges by ID
//! - **Edge consistency**: Maintain edge integrity during node operations
//!
//! ### 4. Metadata Integration
//! - **BuildGraphFromMetadata**: Rebuild entire graph from metadata store
//! - **AddNodesFromMetadata**: Add multiple nodes from metadata
//! - **RemoveNodeByMetadata**: Remove nodes by metadata ID
//!
//! ### 5. Path Computation
//! - **ComputeShortestPaths**: Calculate shortest paths from source nodes
//! - **Graph traversal**: Provide efficient path finding algorithms
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//!
//! let graph_data = graph_state_actor.send(GetGraphData).await?;
//!
//!
//! graph_state_actor.send(AddNode { node }).await?;
//!
//!
//! graph_state_actor.send(BuildGraphFromMetadata { metadata }).await?;
//! ```

use actix::prelude::*;
use log::{debug, error, info, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::actors::messages::*;
use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::metadata::{FileMetadata, MetadataStore};
use visionclaw_domain::models::node::Node;

// Ports (hexagonal architecture)
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;

pub struct GraphStateActor {
    repository: Arc<dyn KnowledgeGraphRepository>,

    graph_data: Arc<GraphData>,

    node_map: Arc<HashMap<u32, Node>>,

    bots_graph_data: Arc<GraphData>,
    /// S5: latest sidechain payment projection, re-applied on every roster
    /// update because bots node ids are reassigned each time.
    chain_payments: Option<Arc<crate::services::chain_payments::ChainPaymentsSnapshot>>,

    next_node_id: std::sync::atomic::AtomicU32,

    // Full metadata store — kept in sync so edge generation always has complete context
    metadata_store: MetadataStore,

    // Node type classification sets for binary protocol flags
    knowledge_node_ids: HashSet<u32>,
    ontology_class_ids: HashSet<u32>,
    ontology_individual_ids: HashSet<u32>,
    ontology_property_ids: HashSet<u32>,
    agent_node_ids: HashSet<u32>,

    /// Maps compact ID (index) → original persistent node ID for write-back operations.
    /// After remapping, graph_data.nodes[i].id == i, so compact_to_original[i] gives
    /// the original node ID needed when persisting changes back to the Oxigraph store.
    compact_to_persistent: Vec<u32>,

    /// Phase 3 (ADR-02 D4): canonical position snapshot. Updated atomically on
    /// every `UpdateNodePositions` apply. Read by the
    /// `GET /api/graph/positions` REST handler — the single source of truth
    /// for "what positions does the client see right now".
    position_snapshot: Arc<crate::actors::messages::PositionFrameSnapshot>,

    /// Monotonic epoch incremented on every `UpdateNodePositions` apply.
    /// Broadcast actor uses this to short-circuit redundant encodes.
    position_epoch: u64,
}

impl GraphStateActor {
    pub fn new(repository: Arc<dyn KnowledgeGraphRepository>) -> Self {
        info!("Initializing GraphStateActor with repository injection");
        Self {
            repository,
            graph_data: Arc::new(GraphData::new()),
            node_map: Arc::new(HashMap::new()),
            bots_graph_data: Arc::new(GraphData::new()),
            chain_payments: None,
            next_node_id: std::sync::atomic::AtomicU32::new(1),
            metadata_store: HashMap::new(),
            knowledge_node_ids: HashSet::new(),
            ontology_class_ids: HashSet::new(),
            ontology_individual_ids: HashSet::new(),
            ontology_property_ids: HashSet::new(),
            agent_node_ids: HashSet::new(),
            compact_to_persistent: Vec::new(),
            position_snapshot: Arc::new(crate::actors::messages::PositionFrameSnapshot::default()),
            position_epoch: 0,
        }
    }

    /// Rebuild the position snapshot from the current `graph_data`.
    /// Called whenever positions change (apply of `UpdateNodePositions`,
    /// graph reload, etc.). Per ADR-02 D4 this is the only writer.
    fn rebuild_position_snapshot(&mut self) {
        use crate::actors::messages::{PositionFrameSnapshot, PositionRow};
        self.position_epoch = self.position_epoch.wrapping_add(1);
        let rows: Vec<PositionRow> = self
            .graph_data
            .nodes
            .iter()
            .map(|n| PositionRow {
                node_id: n.id,
                x: n.data.x,
                y: n.data.y,
                z: n.data.z,
                vx: n.data.vx,
                vy: n.data.vy,
                vz: n.data.vz,
            })
            .collect();
        self.position_snapshot = Arc::new(PositionFrameSnapshot {
            epoch: self.position_epoch,
            node_count: rows.len() as u32,
            rows,
        });
    }

    /// ADR-02 D4 read API. Cheap clone of an `Arc` — no allocation, no copy.
    pub fn current_snapshot(&self) -> Arc<crate::actors::messages::PositionFrameSnapshot> {
        Arc::clone(&self.position_snapshot)
    }

    pub fn get_graph_data(&self) -> &GraphData {
        &self.graph_data
    }

    pub fn get_node_map(&self) -> &HashMap<u32, Node> {
        &self.node_map
    }

    /// Returns node type arrays for binary protocol encoding.
    /// Node IDs are already compact (0..N-1) after source remapping,
    /// so no additional translation is needed.
    pub fn get_node_type_arrays(&self) -> NodeTypeArrays {
        NodeTypeArrays {
            knowledge_ids: self.knowledge_node_ids.iter().copied().collect(),
            agent_ids: self.agent_node_ids.iter().copied().collect(),
            ontology_class_ids: self.ontology_class_ids.iter().copied().collect(),
            ontology_individual_ids: self.ontology_individual_ids.iter().copied().collect(),
            ontology_property_ids: self.ontology_property_ids.iter().copied().collect(),
        }
    }

    /// Returns the compact-to-original-id reverse mapping for write-back operations.
    pub fn get_compact_to_persistent(&self) -> &Vec<u32> {
        &self.compact_to_persistent
    }

    /// Remap all node IDs to compact sequential IDs (0..N-1) and translate
    /// edge source/target through the same mapping. After this call,
    /// `graph_data.nodes[i].id == i` and all edges reference compact IDs.
    /// The original persistent IDs are preserved in `compact_to_persistent` for write-back to Oxigraph.
    fn remap_to_compact_ids(&mut self) {
        let graph_data = Arc::make_mut(&mut self.graph_data);

        // Build original_id → compact_id mapping
        let mut persistent_to_compact: HashMap<u32, u32> =
            HashMap::with_capacity(graph_data.nodes.len());
        self.compact_to_persistent = Vec::with_capacity(graph_data.nodes.len());

        for (compact_id, node) in graph_data.nodes.iter_mut().enumerate() {
            let persistent_id = node.id;
            let compact = compact_id as u32;
            persistent_to_compact.insert(persistent_id, compact);
            self.compact_to_persistent.push(persistent_id);
            node.id = compact;
        }

        // Remap edge source/target
        for edge in &mut graph_data.edges {
            if let Some(&compact_src) = persistent_to_compact.get(&edge.source) {
                edge.source = compact_src;
            } else {
                warn!(
                    "Edge source {} has no compact mapping — orphan edge",
                    edge.source
                );
            }
            if let Some(&compact_tgt) = persistent_to_compact.get(&edge.target) {
                edge.target = compact_tgt;
            } else {
                warn!(
                    "Edge target {} has no compact mapping — orphan edge",
                    edge.target
                );
            }
        }

        // Rebuild node_map with compact IDs
        let mut new_node_map = HashMap::with_capacity(graph_data.nodes.len());
        for node in &graph_data.nodes {
            new_node_map.insert(node.id, node.clone());
        }
        self.node_map = Arc::new(new_node_map);

        info!(
            "Remapped {} nodes to compact IDs 0..{} (edges: {})",
            self.compact_to_persistent.len(),
            self.compact_to_persistent.len().saturating_sub(1),
            graph_data.edges.len()
        );
    }

    /// Classify a single node into the appropriate type set based on its node_type and owl_class_iri fields
    fn classify_node(&mut self, node: &Node) {
        let node_id = node.id;
        // SINGLE SOURCE OF TRUTH: Node::population_type() resolves the authoritative
        // origin from metadata["type"] (node_type is non-classifying elevation
        // scaffold, used only as a legacy fallback). The match arms below refine the
        // ontology population into owl_class / owl_individual / owl_property ID sets,
        // but the field-resolution itself is centralised in visionclaw_domain so
        // every reader classifies a node identically.
        let effective_type = node.population_type();
        match effective_type {
            Some("page") | Some("linked_page") => {
                self.knowledge_node_ids.insert(node_id);
            }
            Some("owl_class") | Some("ontology_node") => {
                self.ontology_class_ids.insert(node_id);
            }
            Some("owl_individual") => {
                self.ontology_individual_ids.insert(node_id);
            }
            Some("owl_property") => {
                self.ontology_property_ids.insert(node_id);
            }
            Some("agent") | Some("bot") => {
                self.agent_node_ids.insert(node_id);
            }
            _ => {
                // Check owl_class_iri as secondary signal for ontology class
                if node.owl_class_iri.is_some() {
                    self.ontology_class_ids.insert(node_id);
                } else {
                    // Default: most nodes from the vault are knowledge nodes
                    self.knowledge_node_ids.insert(node_id);
                }
            }
        }
    }

    /// Reclassify all nodes in the current graph_data into type sets
    fn reclassify_all_nodes(&mut self) {
        self.knowledge_node_ids.clear();
        self.ontology_class_ids.clear();
        self.ontology_individual_ids.clear();
        self.ontology_property_ids.clear();
        self.agent_node_ids.clear();

        // Collect node data first to avoid borrow conflict. SINGLE SOURCE OF TRUTH:
        // Node::population_type() resolves the authoritative origin from
        // metadata["type"] (node_type is non-classifying elevation scaffold, used
        // only as a legacy fallback), so this reclassification agrees with every
        // other reader. The match arms below still refine the ontology population
        // into owl_class / owl_individual / owl_property ID sets.
        let nodes: Vec<(u32, Option<String>, Option<String>)> = self
            .graph_data
            .nodes
            .iter()
            .map(|n| {
                (
                    n.id,
                    n.population_type().map(|s| s.to_string()),
                    n.owl_class_iri.clone(),
                )
            })
            .collect();

        for (node_id, node_type, owl_class_iri) in &nodes {
            match node_type.as_deref() {
                Some("page") | Some("linked_page") => {
                    self.knowledge_node_ids.insert(*node_id);
                }
                Some("owl_class") | Some("ontology_node") => {
                    self.ontology_class_ids.insert(*node_id);
                }
                Some("owl_individual") => {
                    self.ontology_individual_ids.insert(*node_id);
                }
                Some("owl_property") => {
                    self.ontology_property_ids.insert(*node_id);
                }
                Some("agent") | Some("bot") => {
                    self.agent_node_ids.insert(*node_id);
                }
                _ => {
                    if owl_class_iri.is_some() {
                        self.ontology_class_ids.insert(*node_id);
                    } else {
                        self.knowledge_node_ids.insert(*node_id);
                    }
                }
            }
        }

        info!(
            "Node type classification: knowledge={}, agent={}, owl_class={}, owl_individual={}, owl_property={} (compact IDs 0..{})",
            self.knowledge_node_ids.len(),
            self.agent_node_ids.len(),
            self.ontology_class_ids.len(),
            self.ontology_individual_ids.len(),
            self.ontology_property_ids.len(),
            self.graph_data.nodes.len().saturating_sub(1),
        );
    }

    fn add_node(&mut self, node: Node) {
        let node_id = node.id;

        // Classify the node by type (uses compact ID)
        self.classify_node(&node);

        Arc::make_mut(&mut self.node_map).insert(node_id, node.clone());
        Arc::make_mut(&mut self.graph_data).nodes.push(node.clone());

        // Track compact→original mapping (for metadata-built nodes, compact == original)
        if self.compact_to_persistent.len() <= node_id as usize {
            self.compact_to_persistent.resize(node_id as usize + 1, 0);
        }
        self.compact_to_persistent[node_id as usize] = node_id;

        // Persist to Oxigraph store (fire-and-forget)
        let repository = Arc::clone(&self.repository);
        actix::spawn(async move {
            if let Err(e) = repository.add_node(&node).await {
                error!(
                    "Failed to persist add_node({}) to Oxigraph store: {}",
                    node_id, e
                );
            }
        });

        debug!("Added node {} to graph", node_id);
    }

    fn remove_node(&mut self, node_id: u32) {
        if Arc::make_mut(&mut self.node_map).remove(&node_id).is_some() {
            let graph_data_mut = Arc::make_mut(&mut self.graph_data);
            graph_data_mut.nodes.retain(|n| n.id != node_id);

            graph_data_mut
                .edges
                .retain(|e| e.source != node_id && e.target != node_id);

            // Remove from all type classification sets
            self.knowledge_node_ids.remove(&node_id);
            self.ontology_class_ids.remove(&node_id);
            self.ontology_individual_ids.remove(&node_id);
            self.ontology_property_ids.remove(&node_id);
            self.agent_node_ids.remove(&node_id);

            // Persist removal to Oxigraph (fire-and-forget)
            let repository = Arc::clone(&self.repository);
            actix::spawn(async move {
                if let Err(e) = repository.remove_node(node_id).await {
                    error!(
                        "Failed to persist remove_node({}) to Oxigraph: {}",
                        node_id, e
                    );
                }
            });

            debug!("Removed node {} and its edges from graph", node_id);
        } else {
            warn!("Attempted to remove non-existent node {}", node_id);
        }
    }

    fn add_edge(&mut self, edge: Edge) {
        if !self.node_map.contains_key(&edge.source) {
            warn!(
                "Cannot add edge: source node {} does not exist",
                edge.source
            );
            return;
        }
        if !self.node_map.contains_key(&edge.target) {
            warn!(
                "Cannot add edge: target node {} does not exist",
                edge.target
            );
            return;
        }

        Arc::make_mut(&mut self.graph_data).edges.push(edge.clone());

        // Persist to Oxigraph (fire-and-forget)
        let repository = Arc::clone(&self.repository);
        let edge_clone = edge.clone();
        actix::spawn(async move {
            if let Err(e) = repository.add_edge(&edge_clone).await {
                error!(
                    "Failed to persist add_edge({}->{}) to Oxigraph: {}",
                    edge_clone.source, edge_clone.target, e
                );
            }
        });

        debug!(
            "Added edge from {} to {} with weight {}",
            edge.source, edge.target, edge.weight
        );
    }

    fn remove_edge(&mut self, edge_id: &str) {
        let graph_data_mut = Arc::make_mut(&mut self.graph_data);
        let initial_count = graph_data_mut.edges.len();

        graph_data_mut.edges.retain(|e| e.id != edge_id);

        let removed_count = initial_count - graph_data_mut.edges.len();
        if removed_count > 0 {
            // Persist to Oxigraph (fire-and-forget)
            let repository = Arc::clone(&self.repository);
            let edge_id_owned = edge_id.to_string();
            actix::spawn(async move {
                if let Err(e) = repository.remove_edge(&edge_id_owned).await {
                    error!(
                        "Failed to persist remove_edge({}) to Oxigraph: {}",
                        edge_id_owned, e
                    );
                }
            });

            debug!("Removed {} edge(s) with ID {}", removed_count, edge_id);
        } else {
            warn!("No edges found with ID {}", edge_id);
        }
    }

    fn build_from_metadata(&mut self, metadata: MetadataStore) -> Result<(), String> {
        let mut new_graph_data = GraphData::new();

        // Preserve existing positions by metadata_id
        let mut existing_positions: HashMap<
            String,
            (crate::types::vec3::Vec3Data, crate::types::vec3::Vec3Data),
        > = HashMap::new();
        for node in &self.graph_data.nodes {
            existing_positions.insert(
                node.metadata_id.clone(),
                (node.data.position().into(), node.data.velocity().into()),
            );
        }

        // Assign compact IDs directly (0..N-1)
        let mut compact_id: u32 = 0;
        // compact_to_persistent not meaningful here (metadata-built nodes are assigned fresh IDs)
        // but we keep the vector consistent: compact_to_persistent[i] = compact_id = i
        self.compact_to_persistent = Vec::with_capacity(metadata.len());

        for (metadata_id, file_metadata) in metadata.iter() {
            let mut node = Node::new_with_id(metadata_id.clone(), Some(compact_id));

            if let Some((position, velocity)) = existing_positions.get(metadata_id) {
                node.data.x = position.x;
                node.data.y = position.y;
                node.data.z = position.z;
                node.data.vx = velocity.x;
                node.data.vy = velocity.y;
                node.data.vz = velocity.z;
            } else {
                self.generate_random_position(&mut node);
            }

            self.configure_node_from_metadata(&mut node, file_metadata);

            self.compact_to_persistent.push(compact_id);
            new_graph_data.nodes.push(node);
            compact_id += 1;
        }

        // ADR-014: Edges come from Oxigraph (stored by github_sync_service and
        // OxigraphOntologyRepository). No client-side edge generation.

        self.graph_data = Arc::new(new_graph_data);
        self.next_node_id
            .store(compact_id, std::sync::atomic::Ordering::SeqCst);
        self.metadata_store = metadata.clone();

        // Rebuild node_map with compact IDs
        let mut new_node_map = HashMap::with_capacity(self.graph_data.nodes.len());
        for node in &self.graph_data.nodes {
            new_node_map.insert(node.id, node.clone());
        }
        self.node_map = Arc::new(new_node_map);

        // Classify all nodes into type sets (compact IDs)
        self.reclassify_all_nodes();

        // Phase 3 (ADR-02 D4): refresh the canonical position snapshot for
        // newly loaded graph so the broadcast actor and REST endpoint can
        // serve positions immediately after reload.
        self.rebuild_position_snapshot();

        // Persist edges to Oxigraph so they survive restart
        if !self.graph_data.edges.is_empty() {
            let repo = Arc::clone(&self.repository);
            let graph_snapshot = Arc::clone(&self.graph_data);
            actix::spawn(async move {
                if let Err(e) = repo.save_graph(&graph_snapshot).await {
                    error!("Failed to persist graph with edges to Oxigraph: {}", e);
                } else {
                    debug!(
                        "Persisted {} edges to Oxigraph after metadata build",
                        graph_snapshot.edges.len()
                    );
                }
            });
        }

        info!(
            "Built graph from metadata: {} nodes, {} edges (compact IDs 0..{})",
            self.graph_data.nodes.len(),
            self.graph_data.edges.len(),
            self.graph_data.nodes.len().saturating_sub(1)
        );

        Ok(())
    }

    fn generate_random_position(&self, node: &mut Node) {
        use rand::rngs::{OsRng, StdRng};
        use rand::{Rng, SeedableRng};

        let mut rng = StdRng::from_seed(OsRng.gen());
        let radius = 50.0 + rng.gen::<f32>() * 100.0;
        let theta = rng.gen::<f32>() * 2.0 * std::f32::consts::PI;
        let phi = rng.gen::<f32>() * std::f32::consts::PI;

        node.data.x = radius * phi.sin() * theta.cos();
        node.data.y = radius * phi.sin() * theta.sin();
        node.data.z = radius * phi.cos();

        node.data.vx = rng.gen_range(-1.0..1.0);
        node.data.vy = rng.gen_range(-1.0..1.0);
        node.data.vz = rng.gen_range(-1.0..1.0);
    }

    fn configure_node_from_metadata(&self, node: &mut Node, metadata: &FileMetadata) {
        node.label = metadata.file_name.clone();

        let path = std::path::Path::new(&metadata.file_name);
        node.color = Some(Self::color_for_extension(path));

        let size = metadata.file_size;
        node.size = Some(10.0 + (size as f32 / 1000.0).min(50.0));

        node.metadata
            .insert("file_name".to_string(), metadata.file_name.clone());
        node.metadata
            .insert("file_size".to_string(), size.to_string());
        node.metadata.insert(
            "last_modified".to_string(),
            metadata.last_modified.to_string(),
        );

        // Copy ontology fields to node metadata for edge generation and client display
        if let Some(ref domain) = metadata.source_domain {
            node.metadata
                .insert("source_domain".to_string(), domain.clone());
        }
        if !metadata.is_subclass_of.is_empty() {
            node.metadata.insert(
                "is_subclass_of".to_string(),
                metadata.is_subclass_of.join(","),
            );
        }

        // Copy quality and authority scores to node.metadata for filtering
        if let Some(quality) = metadata.quality_score {
            node.metadata
                .insert("quality_score".to_string(), quality.to_string());
        }
        if let Some(authority) = metadata.authority_score {
            node.metadata
                .insert("authority_score".to_string(), authority.to_string());
        }
    }

    fn color_for_extension(path: &std::path::Path) -> String {
        match path.extension().and_then(|s| s.to_str()) {
            Some("rs") => "#CE422B".to_string(),
            Some("js") | Some("ts") => "#F7DF1E".to_string(),
            Some("py") => "#3776AB".to_string(),
            Some("html") => "#E34F26".to_string(),
            Some("css") => "#1572B6".to_string(),
            Some("json") => "#000000".to_string(),
            Some("md") => "#083FA1".to_string(),
            Some("txt") => "#808080".to_string(),
            _ => "#95A5A6".to_string(),
        }
    }

    // ADR-014: generate_edges_from_metadata() and generate_edges_from_labels() deleted.
    // All edges now come from Oxigraph (stored by github_sync_service + OxigraphOntologyRepository).

    fn add_nodes_from_metadata(&mut self, metadata: MetadataStore) -> Result<(), String> {
        let mut added_count = 0;
        let mut current_id = self.next_node_id.load(std::sync::atomic::Ordering::SeqCst);

        for (metadata_id, file_metadata) in metadata.iter() {
            if self
                .node_map
                .values()
                .any(|n| n.metadata_id == *metadata_id)
            {
                continue;
            }

            let mut node = Node::new_with_id(metadata_id.clone(), Some(current_id));
            self.generate_random_position(&mut node);
            self.configure_node_from_metadata(&mut node, file_metadata);

            self.add_node(node);
            current_id += 1;
            added_count += 1;
        }

        self.next_node_id
            .store(current_id, std::sync::atomic::Ordering::SeqCst);
        info!("Added {} new nodes from metadata", added_count);

        // Merge new metadata into stored metadata for node configuration.
        for (id, meta) in metadata {
            self.metadata_store.insert(id, meta);
        }
        // ADR-014: Edges come from Oxigraph, not generated client-side.

        Ok(())
    }

    fn update_node_from_metadata(
        &mut self,
        metadata_id: String,
        metadata: FileMetadata,
    ) -> Result<(), String> {
        let mut node_found = false;

        // Scope the mutable borrow of node_map
        {
            let node_map_mut = Arc::make_mut(&mut self.node_map);
            for node in node_map_mut.values_mut() {
                if node.metadata_id == metadata_id {
                    // Inline configuration to avoid borrowing self
                    node.label = metadata.file_name.clone();
                    let path = std::path::Path::new(&metadata.file_name);
                    node.color = Some(Self::color_for_extension(path));
                    let size = metadata.file_size;
                    node.size = Some(10.0 + (size as f32 / 1000.0).min(50.0));
                    node.metadata
                        .insert("file_name".to_string(), metadata.file_name.clone());
                    node.metadata
                        .insert("file_size".to_string(), size.to_string());
                    node.metadata.insert(
                        "last_modified".to_string(),
                        metadata.last_modified.to_string(),
                    );
                    node_found = true;
                    break;
                }
            }
        } // Release mutable borrow

        if node_found {
            // Scope the mutable borrow of graph_data
            {
                let graph_data_mut = Arc::make_mut(&mut self.graph_data);
                for node in &mut graph_data_mut.nodes {
                    if node.metadata_id == metadata_id {
                        // Inline configuration to avoid borrowing self
                        node.label = metadata.file_name.clone();
                        let path = std::path::Path::new(&metadata.file_name);
                        node.color = Some(Self::color_for_extension(path));
                        let size = metadata.file_size;
                        node.size = Some(10.0 + (size as f32 / 1000.0).min(50.0));
                        node.metadata
                            .insert("file_name".to_string(), metadata.file_name.clone());
                        node.metadata
                            .insert("file_size".to_string(), size.to_string());
                        node.metadata.insert(
                            "last_modified".to_string(),
                            metadata.last_modified.to_string(),
                        );
                        break;
                    }
                }
            } // Release mutable borrow
            debug!("Updated node with metadata_id: {}", metadata_id);
            Ok(())
        } else {
            warn!("Node with metadata_id {} not found for update", metadata_id);
            Err(format!("Node with metadata_id {} not found", metadata_id))
        }
    }

    fn remove_node_by_metadata(&mut self, metadata_id: String) -> Result<(), String> {
        let node_id = self
            .node_map
            .values()
            .find(|n| n.metadata_id == metadata_id)
            .map(|n| n.id);

        if let Some(id) = node_id {
            self.remove_node(id);
            Ok(())
        } else {
            warn!(
                "Node with metadata_id {} not found for removal",
                metadata_id
            );
            Err(format!("Node with metadata_id {} not found", metadata_id))
        }
    }

    fn compute_shortest_paths(
        &self,
        source_node_id: u32,
    ) -> Result<HashMap<u32, (f32, Vec<u32>)>, String> {
        if !self.node_map.contains_key(&source_node_id) {
            return Err(format!("Source node {} not found", source_node_id));
        }

        let mut distances: HashMap<u32, f32> = HashMap::new();
        let mut predecessors: HashMap<u32, u32> = HashMap::new();
        let mut unvisited: std::collections::BTreeSet<(ordered_float::OrderedFloat<f32>, u32)> =
            std::collections::BTreeSet::new();

        for &node_id in self.node_map.keys() {
            let distance = if node_id == source_node_id {
                0.0
            } else {
                f32::INFINITY
            };
            distances.insert(node_id, distance);
            unvisited.insert((ordered_float::OrderedFloat(distance), node_id));
        }

        while let Some((current_distance, current_node)) = unvisited.pop_first() {
            let current_distance = current_distance.into_inner();

            if current_distance == f32::INFINITY {
                break;
            }

            for edge in &self.graph_data.edges {
                let (neighbor, edge_weight) = if edge.source == current_node {
                    (edge.target, edge.weight)
                } else if edge.target == current_node {
                    (edge.source, edge.weight)
                } else {
                    continue;
                };

                let new_distance = current_distance + edge_weight;
                let old_distance = distances.get(&neighbor).copied().unwrap_or(f32::INFINITY);

                if new_distance < old_distance {
                    unvisited.remove(&(ordered_float::OrderedFloat(old_distance), neighbor));

                    distances.insert(neighbor, new_distance);
                    predecessors.insert(neighbor, current_node);

                    unvisited.insert((ordered_float::OrderedFloat(new_distance), neighbor));
                }
            }
        }

        let mut result: HashMap<u32, (f32, Vec<u32>)> = HashMap::new();

        for (&target_node, &distance) in &distances {
            if distance != f32::INFINITY {
                let mut path = Vec::new();
                let mut current = target_node;

                while current != source_node_id {
                    path.push(current);
                    if let Some(&prev) = predecessors.get(&current) {
                        current = prev;
                    } else {
                        break;
                    }
                }
                path.push(source_node_id);
                path.reverse();

                result.insert(target_node, (distance, path));
            }
        }

        info!(
            "Computed shortest paths from node {} to {} reachable nodes",
            source_node_id,
            result.len()
        );

        Ok(result)
    }
}

impl Actor for GraphStateActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        // Start with empty state. main.rs runs the Data Orchestration Sequence
        // (Step 2: load_graph_from_files → save_graph into Oxigraph, then
        // Step 3: send ReloadGraphFromDatabase to this actor). The eager
        // load that previously lived here raced with Step 3 — `started()`
        // would async-read Oxigraph BEFORE Step 2 had populated it, then
        // its completion callback could fire AFTER ReloadGraphFromDatabase
        // had loaded fresh state, overwriting it with stale/empty data.
        // Letting Step 3 do the single canonical load eliminates the race.
        info!("GraphStateActor started with empty state - waiting for ReloadGraphFromDatabase");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("GraphStateActor stopped");
    }
}

// Handler implementations

/// Handler for GPU-computed position updates.
/// Updates node positions in-place so that GetGraphData returns GPU-computed layout.
impl Handler<UpdateNodePositions> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateNodePositions, _ctx: &mut Self::Context) -> Self::Result {
        if msg.positions.is_empty() {
            return Ok(());
        }

        // Build a lookup from the incoming positions
        let pos_map: std::collections::HashMap<
            u32,
            &crate::utils::socket_flow_messages::BinaryNodeDataClient,
        > = msg.positions.iter().map(|(id, data)| (*id, data)).collect();

        // Diagnostic: log ID mismatch on first occurrence
        if !self.graph_data.nodes.is_empty() && !msg.positions.is_empty() {
            let first_gpu_id = msg.positions[0].0;
            let first_graph_id = self.graph_data.nodes[0].id;
            if !pos_map.contains_key(&first_graph_id) {
                warn!(
                    "GPU→GraphState ID mismatch: GPU sends id={}, graph has id={} (GPU count={}, graph count={})",
                    first_gpu_id, first_graph_id, msg.positions.len(), self.graph_data.nodes.len()
                );
            }
        }

        // Mutate the Arc<GraphData> in-place (clones on first mutation if shared)
        let graph_data = Arc::make_mut(&mut self.graph_data);
        let mut updated = 0usize;
        for node in &mut graph_data.nodes {
            if let Some(pos) = pos_map.get(&node.id) {
                node.data.x = pos.x;
                node.data.y = pos.y;
                node.data.z = pos.z;
                node.data.vx = pos.vx;
                node.data.vy = pos.vy;
                node.data.vz = pos.vz;
                updated += 1;
            }
        }

        // Also update the node_map
        let node_map = Arc::make_mut(&mut self.node_map);
        for (id, pos) in &msg.positions {
            if let Some(node) = node_map.get_mut(id) {
                node.data.x = pos.x;
                node.data.y = pos.y;
                node.data.z = pos.z;
                node.data.vx = pos.vx;
                node.data.vy = pos.vy;
                node.data.vz = pos.vz;
            }
        }

        // Phase 3 (ADR-02 D4): rebuild the canonical snapshot atomically after
        // every position apply. Broadcast actor and REST endpoint both read
        // from this single source.
        self.rebuild_position_snapshot();

        debug!(
            "GraphStateActor: Updated {} node positions from GPU",
            updated
        );
        Ok(())
    }
}

/// Phase 3 (ADR-02 D4): canonical read path for position data.
impl Handler<crate::actors::messages::GetPositionFrameSnapshot> for GraphStateActor {
    type Result = Result<Arc<crate::actors::messages::PositionFrameSnapshot>, String>;

    fn handle(
        &mut self,
        _msg: crate::actors::messages::GetPositionFrameSnapshot,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(self.current_snapshot())
    }
}

impl Handler<GetGraphData> for GraphStateActor {
    type Result = Result<Arc<GraphData>, String>;

    fn handle(&mut self, _msg: GetGraphData, _ctx: &mut Self::Context) -> Self::Result {
        debug!("GraphStateActor handling GetGraphData with Arc reference");
        Ok(Arc::clone(&self.graph_data))
    }
}

impl Handler<AddNode> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: AddNode, _ctx: &mut Self::Context) -> Self::Result {
        self.add_node(msg.node);
        Ok(())
    }
}

impl Handler<RemoveNode> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RemoveNode, _ctx: &mut Self::Context) -> Self::Result {
        self.remove_node(msg.node_id);
        Ok(())
    }
}

impl Handler<AddEdge> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: AddEdge, _ctx: &mut Self::Context) -> Self::Result {
        self.add_edge(msg.edge);
        Ok(())
    }
}

impl Handler<RemoveEdge> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RemoveEdge, _ctx: &mut Self::Context) -> Self::Result {
        self.remove_edge(&msg.edge_id);
        Ok(())
    }
}

impl Handler<GetNodeMap> for GraphStateActor {
    type Result = Result<Arc<HashMap<u32, Node>>, String>;

    fn handle(&mut self, _msg: GetNodeMap, _ctx: &mut Self::Context) -> Self::Result {
        debug!("GraphStateActor handling GetNodeMap with Arc reference");
        Ok(Arc::clone(&self.node_map))
    }
}

impl Handler<BuildGraphFromMetadata> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: BuildGraphFromMetadata, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "BuildGraphFromMetadata handler called with {} metadata entries",
            msg.metadata.len()
        );
        self.build_from_metadata(msg.metadata)
    }
}

impl Handler<AddNodesFromMetadata> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: AddNodesFromMetadata, _ctx: &mut Self::Context) -> Self::Result {
        self.add_nodes_from_metadata(msg.metadata)
    }
}

impl Handler<UpdateNodeFromMetadata> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateNodeFromMetadata, _ctx: &mut Self::Context) -> Self::Result {
        self.update_node_from_metadata(msg.metadata_id, msg.metadata)
    }
}

impl Handler<RemoveNodeByMetadata> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: RemoveNodeByMetadata, _ctx: &mut Self::Context) -> Self::Result {
        self.remove_node_by_metadata(msg.metadata_id)
    }
}

impl Handler<UpdateGraphData> for GraphStateActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGraphData, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Updating graph data with {} nodes, {} edges",
            msg.graph_data.nodes.len(),
            msg.graph_data.edges.len()
        );

        self.graph_data = msg.graph_data;

        Arc::make_mut(&mut self.node_map).clear();
        for node in &self.graph_data.nodes {
            Arc::make_mut(&mut self.node_map).insert(node.id, node.clone());
        }

        // Reclassify all nodes after graph data update
        self.reclassify_all_nodes();

        // Phase 3 (ADR-02 D4): refresh the canonical position snapshot so a
        // cold-connecting client immediately reads accurate positions.
        self.rebuild_position_snapshot();

        info!("Graph data updated successfully");
        Ok(())
    }
}

impl Handler<GetBotsGraphData> for GraphStateActor {
    type Result = Result<Arc<GraphData>, String>;

    fn handle(&mut self, _msg: GetBotsGraphData, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(Arc::clone(&self.bots_graph_data))
    }
}

impl Handler<UpdateBotsGraph> for GraphStateActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateBotsGraph, _ctx: &mut Context<Self>) -> Self::Result {
        let mut nodes = vec![];
        let mut edges = vec![];

        let bot_id_offset = 10000;

        let mut existing_positions: HashMap<
            String,
            (crate::types::vec3::Vec3Data, crate::types::vec3::Vec3Data),
        > = HashMap::new();
        for node in &self.bots_graph_data.nodes {
            existing_positions.insert(
                node.metadata_id.clone(),
                (node.data.position().into(), node.data.velocity().into()),
            );
        }

        for (i, agent) in msg.agents.iter().enumerate() {
            let node_id = bot_id_offset + i as u32;
            let mut node = Node::new_with_id(agent.id.clone(), Some(node_id));

            if let Some((saved_position, saved_velocity)) = existing_positions.get(&agent.id) {
                node.data.x = saved_position.x;
                node.data.y = saved_position.y;
                node.data.z = saved_position.z;
                node.data.vx = saved_velocity.x;
                node.data.vy = saved_velocity.y;
                node.data.vz = saved_velocity.z;
            } else {
                self.generate_random_position(&mut node);
            }

            node.color = Some(match agent.agent_type.as_str() {
                "coordinator" => "#FF6B6B".to_string(),
                "researcher" => "#4ECDC4".to_string(),
                "coder" => "#45B7D1".to_string(),
                "analyst" => "#FFA07A".to_string(),
                "architect" => "#98D8C8".to_string(),
                "tester" => "#F7DC6F".to_string(),
                _ => "#95A5A6".to_string(),
            });

            node.label = agent.name.clone();
            node.size = Some(20.0 + (agent.workload * 25.0));

            node.metadata
                .insert("agent_type".to_string(), agent.agent_type.clone());
            node.metadata
                .insert("status".to_string(), agent.status.clone());
            node.metadata
                .insert("cpu_usage".to_string(), agent.cpu_usage.to_string());
            node.metadata
                .insert("memory_usage".to_string(), agent.memory_usage.to_string());
            node.metadata
                .insert("health".to_string(), agent.health.to_string());
            node.metadata
                .insert("is_agent".to_string(), "true".to_string());

            // PRD-023 WP-9 M4: carry the agent's validated did:nostr onto the
            // graph node so it rides `initialGraphLoad` to the Godot XR client
            // (parse_agent_identities). Absent for agents without a DID.
            if let Some(did) = &agent.did_nostr {
                if !did.is_empty() {
                    node.metadata.insert("did_nostr".to_string(), did.clone());
                }
            }

            nodes.push(node);
        }

        for (i, source_agent) in msg.agents.iter().enumerate() {
            for (j, target_agent) in msg.agents.iter().enumerate() {
                if i != j {
                    let source_node_id = bot_id_offset + i as u32;
                    let target_node_id = bot_id_offset + j as u32;

                    let communication_intensity = if source_agent.agent_type == "coordinator"
                        || target_agent.agent_type == "coordinator"
                    {
                        0.8
                    } else if source_agent.status == "active" && target_agent.status == "active" {
                        0.5
                    } else {
                        0.2
                    };

                    if communication_intensity > 0.1 {
                        let mut edge =
                            Edge::new(source_node_id, target_node_id, communication_intensity);
                        let metadata = edge.metadata.get_or_insert_with(HashMap::new);
                        metadata.insert(
                            "communication_type".to_string(),
                            "agent_collaboration".to_string(),
                        );
                        metadata
                            .insert("intensity".to_string(), communication_intensity.to_string());
                        edges.push(edge);
                    }
                }
            }
        }

        let bots_graph_data_mut = Arc::make_mut(&mut self.bots_graph_data);
        bots_graph_data_mut.nodes = nodes;
        bots_graph_data_mut.edges = edges;
        // S5: node ids are reassigned on every roster update, so the sidechain
        // payment edges are re-resolved by did:nostr against the new nodes.
        crate::services::chain_payments::apply_to_bots_graph(
            bots_graph_data_mut,
            self.chain_payments.as_deref(),
        );

        // Classify all bots_graph_data nodes as agent nodes so binary protocol
        // sets bit 31 for them via NodeTypeArrays.
        for node in &self.bots_graph_data.nodes {
            self.agent_node_ids.insert(node.id);
        }

        info!(
            "Updated bots graph with {} agents and {} edges (agent_node_ids={})",
            msg.agents.len(),
            self.bots_graph_data.edges.len(),
            self.agent_node_ids.len()
        );
    }
}

impl Handler<UpdateChainPayments> for GraphStateActor {
    type Result = ();

    fn handle(&mut self, msg: UpdateChainPayments, _ctx: &mut Context<Self>) -> Self::Result {
        self.chain_payments = msg.snapshot;
        let bots = Arc::make_mut(&mut self.bots_graph_data);
        crate::services::chain_payments::apply_to_bots_graph(bots, self.chain_payments.as_deref());
        debug!(
            "Applied chain payments to bots graph: {} chain_payment edges",
            bots.edges
                .iter()
                .filter(|e| e.edge_type.as_deref()
                    == Some(visionclaw_domain::models::edge::CHAIN_PAYMENT_EDGE_TYPE))
                .count()
        );
    }
}

impl Handler<ReloadGraphFromDatabase> for GraphStateActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, _msg: ReloadGraphFromDatabase, _ctx: &mut Self::Context) -> Self::Result {
        info!("GraphStateActor: ReloadGraphFromDatabase - reloading graph from Oxigraph");

        let repository = Arc::clone(&self.repository);

        Box::pin(
            async move {
                match repository.load_graph().await {
                    Ok(arc_graph_data) => {
                        info!(
                            "GraphStateActor: Reloaded graph from Oxigraph: {} nodes, {} edges",
                            arc_graph_data.nodes.len(),
                            arc_graph_data.edges.len()
                        );
                        Ok(arc_graph_data)
                    }
                    Err(e) => {
                        error!("GraphStateActor: Failed to reload graph from Oxigraph: {}", e);
                        Err(format!("Failed to reload graph: {}", e))
                    }
                }
            }
            .into_actor(self)
            .map(|result, act, _ctx| {
                match result {
                    Ok(arc_graph_data) => {
                        // Update actor state with reloaded graph
                        act.graph_data = arc_graph_data.clone();

                        // ADR-014: No fallback edge generation. Edges come from Oxigraph.

                        // Remap all IDs to compact 0..N-1
                        act.remap_to_compact_ids();

                        // Update next_node_id
                        act.next_node_id.store(act.graph_data.nodes.len() as u32, std::sync::atomic::Ordering::SeqCst);

                        // Reclassify all nodes after reload (using compact IDs)
                        act.reclassify_all_nodes();

                        // No edge re-persistence here. Edges arrived FROM Oxigraph;
                        // writing them back triggers the per-edge bridge-integrity
                        // ASK loop in add_edge, which on a 196K-edge corpus with
                        // stale bridges becomes a self-DoS (169K cascading ASK
                        // failures observed in production). The compact-ID remap
                        // is purely in-memory; Oxigraph keeps the persistent IDs
                        // that load_graph_from_files originally wrote in Step 2.

                        info!(
                            "GraphStateActor: State updated after reload - {} nodes, {} edges (compact IDs 0..{})",
                            act.graph_data.nodes.len(),
                            act.graph_data.edges.len(),
                            act.graph_data.nodes.len().saturating_sub(1),
                        );
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }),
        )
    }
}

impl Handler<GetNodeTypeArrays> for GraphStateActor {
    type Result = NodeTypeArrays;

    fn handle(&mut self, _msg: GetNodeTypeArrays, _ctx: &mut Self::Context) -> Self::Result {
        self.get_node_type_arrays()
    }
}

impl Handler<GetNodeIdMapping> for GraphStateActor {
    type Result = NodeIdMapping;

    fn handle(&mut self, _msg: GetNodeIdMapping, _ctx: &mut Self::Context) -> Self::Result {
        // Node IDs are now compact (0..N-1) at the source — no remapping needed.
        // Return empty map for backward compatibility with any remaining callers.
        NodeIdMapping(HashMap::new())
    }
}

impl Handler<ComputeShortestPaths> for GraphStateActor {
    type Result = Result<crate::ports::gpu_semantic_analyzer::PathfindingResult, String>;

    fn handle(&mut self, msg: ComputeShortestPaths, _ctx: &mut Self::Context) -> Self::Result {
        use std::collections::HashMap;
        let start_time = std::time::Instant::now();

        match self.compute_shortest_paths(msg.source_node_id) {
            Ok(paths) => {
                info!(
                    "Computed shortest paths from node {}: {} reachable nodes",
                    msg.source_node_id,
                    paths.len()
                );

                // Convert HashMap<u32, Option<f32>> to HashMap<u32, f32> and Vec<u32>
                let mut distances = HashMap::new();
                let mut path_map = HashMap::new();

                for (node_id, (distance, path)) in paths {
                    distances.insert(node_id, distance);
                    // Use the actual path from the algorithm
                    path_map.insert(node_id, path);
                }

                Ok(crate::ports::gpu_semantic_analyzer::PathfindingResult {
                    source_node: msg.source_node_id,
                    distances,
                    paths: path_map,
                    computation_time_ms: start_time.elapsed().as_secs_f32() * 1000.0,
                })
            }
            Err(e) => {
                error!("Failed to compute shortest paths: {}", e);
                Err(e)
            }
        }
    }
}
