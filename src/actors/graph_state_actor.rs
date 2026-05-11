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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use log::{debug, info, warn, error};

use crate::actors::messages::*;
use crate::models::node::Node;
use crate::models::edge::Edge;
use crate::models::metadata::{MetadataStore, FileMetadata};
use crate::models::graph::GraphData;
use crate::models::graph_types::{classify_node_population, classify_ontology_subtype, NodePopulation, OntologySubtype};

// Ports (hexagonal architecture)
use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;

pub struct GraphStateActor {

    repository: Arc<dyn KnowledgeGraphRepository>,

    graph_data: Arc<GraphData>,

    node_map: Arc<HashMap<u32, Node>>,

    bots_graph_data: Arc<GraphData>,

    next_node_id: std::sync::atomic::AtomicU32,

    // Full metadata store — kept in sync so edge generation always has complete context
    metadata_store: MetadataStore,

    // Node type classification sets for binary protocol flags
    knowledge_node_ids: HashSet<u32>,
    ontology_class_ids: HashSet<u32>,
    ontology_individual_ids: HashSet<u32>,
    ontology_property_ids: HashSet<u32>,
    agent_node_ids: HashSet<u32>,

    /// Maps compact ID (index) → original Neo4j ID for write-back operations.
    /// After remapping, graph_data.nodes[i].id == i, so compact_to_neo4j[i] gives
    /// the original Neo4j ID needed when persisting changes back to the database.
    compact_to_neo4j: Vec<u32>,
}

impl GraphStateActor {
    
    pub fn new(repository: Arc<dyn KnowledgeGraphRepository>) -> Self {
        info!("Initializing GraphStateActor with repository injection");
        Self {
            repository,
            graph_data: Arc::new(GraphData::new()),
            node_map: Arc::new(HashMap::new()),
            bots_graph_data: Arc::new(GraphData::new()),
            next_node_id: std::sync::atomic::AtomicU32::new(1),
            metadata_store: HashMap::new(),
            knowledge_node_ids: HashSet::new(),
            ontology_class_ids: HashSet::new(),
            ontology_individual_ids: HashSet::new(),
            ontology_property_ids: HashSet::new(),
            agent_node_ids: HashSet::new(),
            compact_to_neo4j: Vec::new(),
        }
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
    ///
    /// ADR-050 (H2), updated by ADR-061 §D3: also populates
    /// `private_node_owners` with the owner pubkey of every
    /// `visibility=Private` node. `ClientCoordinatorActor` uses this map to
    /// derive the per-client hidden-id set; private nodes owned by other
    /// clients are DROPPED from the per-frame position stream rather than
    /// being opacified with a wire flag (the wire is plain id+pos+vel).
    pub fn get_node_type_arrays(&self) -> NodeTypeArrays {
        use crate::models::node::Visibility;
        let mut private_node_owners = HashMap::new();
        for node in self.node_map.values() {
            if node.visibility == Visibility::Private {
                if let Some(owner) = &node.owner_pubkey {
                    private_node_owners.insert(node.id, owner.clone());
                }
            }
        }
        NodeTypeArrays {
            knowledge_ids: self.knowledge_node_ids.iter().copied().collect(),
            agent_ids: self.agent_node_ids.iter().copied().collect(),
            ontology_class_ids: self.ontology_class_ids.iter().copied().collect(),
            ontology_individual_ids: self.ontology_individual_ids.iter().copied().collect(),
            ontology_property_ids: self.ontology_property_ids.iter().copied().collect(),
            private_node_owners,
        }
    }

    /// Returns the compact-to-Neo4j reverse mapping for write-back operations.
    pub fn get_compact_to_neo4j(&self) -> &Vec<u32> {
        &self.compact_to_neo4j
    }

    /// Strip empty kg_stub orphans from loaded graph data so the GPU binary
    /// stream and REST /api/graph/data return the same node set. Empty stubs
    /// (node_type="kg_stub", empty metadata_id, empty label) are MERGE-on-id
    /// artefacts with no visual representation — computing their physics is
    /// wasted GPU work and the client will never display them.
    fn filter_empty_stubs(graph_data: Arc<GraphData>) -> Arc<GraphData> {
        let before = graph_data.nodes.len();
        let has_empty_stubs = graph_data.nodes.iter().any(|n| {
            n.node_type.as_deref() == Some("kg_stub")
                && n.metadata_id.is_empty()
                && n.label.is_empty()
        });
        if !has_empty_stubs {
            return graph_data;
        }

        let mut filtered = (*graph_data).clone();
        filtered.nodes.retain(|n| {
            !(n.node_type.as_deref() == Some("kg_stub")
                && n.metadata_id.is_empty()
                && n.label.is_empty())
        });
        let retained_ids: HashSet<u32> = filtered.nodes.iter().map(|n| n.id).collect();
        filtered.edges.retain(|e| {
            retained_ids.contains(&e.source) && retained_ids.contains(&e.target)
        });
        let after = filtered.nodes.len();
        info!(
            "GraphStateActor: Filtered {} empty kg_stub orphans ({} → {} nodes, {} edges)",
            before - after, before, after, filtered.edges.len()
        );
        Arc::new(filtered)
    }

    /// Remap all node IDs to compact sequential IDs (0..N-1) and translate
    /// edge source/target through the same mapping. After this call,
    /// `graph_data.nodes[i].id == i` and all edges reference compact IDs.
    /// The original Neo4j IDs are preserved in `compact_to_neo4j` for write-back.
    fn remap_to_compact_ids(&mut self) {
        let graph_data = Arc::make_mut(&mut self.graph_data);

        // Identity mapping: keep original Neo4j IDs throughout so the
        // binary position stream and REST /api/graph/data share the
        // same ID space. compact_to_neo4j[i] is the Neo4j ID of the
        // i-th node (for edge write-back compatibility).
        self.compact_to_neo4j = graph_data.nodes.iter().map(|n| n.id).collect();

        // Rebuild node_map keyed by original IDs
        let mut new_node_map = HashMap::with_capacity(graph_data.nodes.len());
        for node in &graph_data.nodes {
            new_node_map.insert(node.id, node.clone());
        }
        self.node_map = Arc::new(new_node_map);

        debug!(
            "Graph loaded: {} nodes (IDs preserved), {} edges",
            graph_data.nodes.len(),
            graph_data.edges.len()
        );
    }

    /// ADR-036: Classify a node using the canonical classify_node_population function.
    /// Also handles owl_class_iri fallback for nodes without explicit type.
    fn classify_node(&mut self, node: &Node) {
        let node_id = node.id;
        let pop = classify_node_population(node.node_type.as_deref());

        match pop {
            NodePopulation::Knowledge => {
                // Secondary check: owl_class_iri overrides to ontology even without type string
                if node.owl_class_iri.is_some() {
                    self.ontology_class_ids.insert(node_id);
                } else {
                    self.knowledge_node_ids.insert(node_id);
                }
            }
            NodePopulation::Ontology => {
                match classify_ontology_subtype(node.node_type.as_deref()) {
                    OntologySubtype::Class | OntologySubtype::Unspecified => {
                        self.ontology_class_ids.insert(node_id);
                    }
                    OntologySubtype::Individual => {
                        self.ontology_individual_ids.insert(node_id);
                    }
                    OntologySubtype::Property => {
                        self.ontology_property_ids.insert(node_id);
                    }
                }
            }
            NodePopulation::Agent => {
                self.agent_node_ids.insert(node_id);
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

        // ADR-036: Use canonical classify_node via collected node refs to avoid borrow conflict
        let node_refs: Vec<(u32, Option<String>, Option<String>)> = self.graph_data.nodes.iter()
            .map(|n| (n.id, n.node_type.clone(), n.owl_class_iri.clone()))
            .collect();

        for (node_id, node_type, owl_class_iri) in &node_refs {
            let pop = classify_node_population(node_type.as_deref());
            match pop {
                NodePopulation::Knowledge => {
                    if owl_class_iri.is_some() {
                        self.ontology_class_ids.insert(*node_id);
                    } else {
                        self.knowledge_node_ids.insert(*node_id);
                    }
                }
                NodePopulation::Ontology => {
                    match classify_ontology_subtype(node_type.as_deref()) {
                        OntologySubtype::Class | OntologySubtype::Unspecified => {
                            self.ontology_class_ids.insert(*node_id);
                        }
                        OntologySubtype::Individual => {
                            self.ontology_individual_ids.insert(*node_id);
                        }
                        OntologySubtype::Property => {
                            self.ontology_property_ids.insert(*node_id);
                        }
                    }
                }
                NodePopulation::Agent => {
                    self.agent_node_ids.insert(*node_id);
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

        // Track compact→neo4j mapping (for metadata-built nodes, compact == neo4j)
        if self.compact_to_neo4j.len() <= node_id as usize {
            self.compact_to_neo4j.resize(node_id as usize + 1, 0);
        }
        self.compact_to_neo4j[node_id as usize] = node_id;

        // Persist to Neo4j (fire-and-forget)
        let repository = Arc::clone(&self.repository);
        actix::spawn(async move {
            if let Err(e) = repository.add_node(&node).await {
                error!("Failed to persist add_node({}) to Neo4j: {}", node_id, e);
            }
        });

        info!("Added node {} to graph", node_id);
    }

    
    fn remove_node(&mut self, node_id: u32) {
        if Arc::make_mut(&mut self.node_map).remove(&node_id).is_some() {
            let graph_data_mut = Arc::make_mut(&mut self.graph_data);
            graph_data_mut.nodes.retain(|n| n.id != node_id);

            graph_data_mut.edges.retain(|e| e.source != node_id && e.target != node_id);

            // Remove from all type classification sets
            self.knowledge_node_ids.remove(&node_id);
            self.ontology_class_ids.remove(&node_id);
            self.ontology_individual_ids.remove(&node_id);
            self.ontology_property_ids.remove(&node_id);
            self.agent_node_ids.remove(&node_id);

            // Persist to Neo4j (fire-and-forget)
            let repository = Arc::clone(&self.repository);
            actix::spawn(async move {
                if let Err(e) = repository.remove_node(node_id).await {
                    error!("Failed to persist remove_node({}) to Neo4j: {}", node_id, e);
                }
            });

            info!("Removed node {} and its edges from graph", node_id);
        } else {
            warn!("Attempted to remove non-existent node {}", node_id);
        }
    }

    
    fn add_edge(&mut self, edge: Edge) {

        if !self.node_map.contains_key(&edge.source) {
            warn!("Cannot add edge: source node {} does not exist", edge.source);
            return;
        }
        if !self.node_map.contains_key(&edge.target) {
            warn!("Cannot add edge: target node {} does not exist", edge.target);
            return;
        }


        Arc::make_mut(&mut self.graph_data).edges.push(edge.clone());

        // Persist to Neo4j (fire-and-forget)
        let repository = Arc::clone(&self.repository);
        let edge_clone = edge.clone();
        actix::spawn(async move {
            if let Err(e) = repository.add_edge(&edge_clone).await {
                error!("Failed to persist add_edge({}->{}) to Neo4j: {}", edge_clone.source, edge_clone.target, e);
            }
        });

        info!("Added edge from {} to {} with weight {}", edge.source, edge.target, edge.weight);
    }

    
    fn remove_edge(&mut self, edge_id: &str) {
        let graph_data_mut = Arc::make_mut(&mut self.graph_data);
        let initial_count = graph_data_mut.edges.len();

        graph_data_mut.edges.retain(|e| e.id != edge_id);

        let removed_count = initial_count - graph_data_mut.edges.len();
        if removed_count > 0 {
            // Persist to Neo4j (fire-and-forget)
            let repository = Arc::clone(&self.repository);
            let edge_id_owned = edge_id.to_string();
            actix::spawn(async move {
                if let Err(e) = repository.remove_edge(&edge_id_owned).await {
                    error!("Failed to persist remove_edge({}) to Neo4j: {}", edge_id_owned, e);
                }
            });

            info!("Removed {} edge(s) with ID {}", removed_count, edge_id);
        } else {
            warn!("No edges found with ID {}", edge_id);
        }
    }

    
    fn build_from_metadata(&mut self, metadata: MetadataStore) -> Result<(), String> {
        let mut new_graph_data = GraphData::new();

        // Preserve existing positions by metadata_id
        let mut existing_positions: HashMap<String, (crate::types::vec3::Vec3Data, crate::types::vec3::Vec3Data)> = HashMap::new();
        for node in &self.graph_data.nodes {
            existing_positions.insert(node.metadata_id.clone(), (node.data.position(), node.data.velocity()));
        }

        // Assign compact IDs directly (0..N-1)
        let mut compact_id: u32 = 0;
        // compact_to_neo4j not meaningful here (metadata-built nodes don't come from Neo4j)
        // but we keep the vector consistent: compact_to_neo4j[i] = compact_id = i
        self.compact_to_neo4j = Vec::with_capacity(metadata.len());

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

            self.compact_to_neo4j.push(compact_id);
            new_graph_data.nodes.push(node);
            compact_id += 1;
        }

        // ADR-014: Edges come from Neo4j (stored by github_sync_service and
        // neo4j_ontology_repository). No client-side edge generation.

        self.graph_data = Arc::new(new_graph_data);
        self.next_node_id.store(compact_id, std::sync::atomic::Ordering::SeqCst);
        self.metadata_store = metadata.clone();

        // Rebuild node_map with compact IDs
        let mut new_node_map = HashMap::with_capacity(self.graph_data.nodes.len());
        for node in &self.graph_data.nodes {
            new_node_map.insert(node.id, node.clone());
        }
        self.node_map = Arc::new(new_node_map);

        // Classify all nodes into type sets (compact IDs)
        self.reclassify_all_nodes();

        // Persist edges to Neo4j so they survive restart
        if !self.graph_data.edges.is_empty() {
            let repo = Arc::clone(&self.repository);
            let graph_snapshot = Arc::clone(&self.graph_data);
            actix::spawn(async move {
                if let Err(e) = repo.save_graph(&graph_snapshot).await {
                    error!("Failed to persist graph with edges to Neo4j: {}", e);
                } else {
                    info!("Persisted {} edges to Neo4j after metadata build", graph_snapshot.edges.len());
                }
            });
        }

        info!("Built graph from metadata: {} nodes, {} edges (compact IDs 0..{})",
              self.graph_data.nodes.len(), self.graph_data.edges.len(),
              self.graph_data.nodes.len().saturating_sub(1));

        Ok(())
    }

    
    fn generate_random_position(&self, node: &mut Node) {
        use rand::{Rng, SeedableRng};
        use rand::rngs::{StdRng, OsRng};

        let mut rng = StdRng::from_seed(OsRng.gen());
        // Initial scatter within bounds_size (≈±80 units) — keeps the warmup
        // phase from spending its energy budget pulling nodes inward from
        // 100+ units away. The post-2026-04-30 viewport-calibrated physics
        // defaults expect a tight initial cloud.
        let radius = 5.0 + rng.gen::<f32>() * 25.0;
        let theta = rng.gen::<f32>() * 2.0 * std::f32::consts::PI;
        let phi = rng.gen::<f32>() * std::f32::consts::PI;

        node.data.x = radius * phi.sin() * theta.cos();
        node.data.y = radius * phi.sin() * theta.sin();
        node.data.z = radius * phi.cos();

        node.data.vx = rng.gen_range(-0.2..0.2);
        node.data.vy = rng.gen_range(-0.2..0.2);
        node.data.vz = rng.gen_range(-0.2..0.2);
    }

    
    fn configure_node_from_metadata(&self, node: &mut Node, metadata: &FileMetadata) {

        node.label = metadata.file_name.clone();

        let path = std::path::Path::new(&metadata.file_name);
        node.color = Some(Self::color_for_extension(path));

        let size = metadata.file_size;
        node.size = Some(10.0 + (size as f32 / 1000.0).min(50.0));

        // ADR-036: Set node_type from metadata so classification works for incremental adds.
        // Ontology nodes have owl_class or source_domain set; default to "page" for knowledge nodes.
        if node.node_type.is_none() {
            if metadata.owl_class.is_some() || metadata.source_domain.is_some() {
                node.node_type = Some("owl_class".to_string());
            } else {
                node.node_type = Some("page".to_string());
            }
        }

        node.metadata.insert("file_name".to_string(), metadata.file_name.clone());
        node.metadata.insert("file_size".to_string(), size.to_string());
        node.metadata.insert("last_modified".to_string(), metadata.last_modified.to_string());

        // Copy ontology fields to node metadata for edge generation and client display
        if let Some(ref domain) = metadata.source_domain {
            node.metadata.insert("source_domain".to_string(), domain.clone());
        }
        if !metadata.is_subclass_of.is_empty() {
            node.metadata.insert("is_subclass_of".to_string(), metadata.is_subclass_of.join(","));
        }

        // Copy quality and authority scores to node.metadata for filtering
        if let Some(quality) = metadata.quality_score {
            node.metadata.insert("quality_score".to_string(), quality.to_string());
        }
        if let Some(authority) = metadata.authority_score {
            node.metadata.insert("authority_score".to_string(), authority.to_string());
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
    // All edges now come from Neo4j (stored by github_sync_service + neo4j_ontology_repository).

    fn add_nodes_from_metadata(&mut self, metadata: MetadataStore) -> Result<(), String> {
        let mut added_count = 0;
        let mut current_id = self.next_node_id.load(std::sync::atomic::Ordering::SeqCst);

        for (metadata_id, file_metadata) in metadata.iter() {

            if self.node_map.values().any(|n| n.metadata_id == *metadata_id) {
                continue;
            }

            let mut node = Node::new_with_id(metadata_id.clone(), Some(current_id));
            self.generate_random_position(&mut node);
            self.configure_node_from_metadata(&mut node, file_metadata);

            self.add_node(node);
            current_id += 1;
            added_count += 1;
        }

        self.next_node_id.store(current_id, std::sync::atomic::Ordering::SeqCst);
        info!("Added {} new nodes from metadata", added_count);

        // Merge new metadata into stored metadata for node configuration.
        for (id, meta) in metadata {
            self.metadata_store.insert(id, meta);
        }
        // ADR-014: Edges come from Neo4j, not generated client-side.

        Ok(())
    }

    
    fn update_node_from_metadata(&mut self, metadata_id: String, metadata: FileMetadata) -> Result<(), String> {
        
        let mut node_found = false;

        // Scope the mutable borrow of node_map
        {
            let node_map_mut = Arc::make_mut(&mut self.node_map);
            for (_, node) in node_map_mut.iter_mut() {
                if node.metadata_id == metadata_id {
                    // Inline configuration to avoid borrowing self
                    node.label = metadata.file_name.clone();
                    let path = std::path::Path::new(&metadata.file_name);
                    node.color = Some(Self::color_for_extension(path));
                    let size = metadata.file_size;
                    node.size = Some(10.0 + (size as f32 / 1000.0).min(50.0));
                    node.metadata.insert("file_name".to_string(), metadata.file_name.clone());
                    node.metadata.insert("file_size".to_string(), size.to_string());
                    node.metadata.insert("last_modified".to_string(), metadata.last_modified.to_string());
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
                        node.metadata.insert("file_name".to_string(), metadata.file_name.clone());
                        node.metadata.insert("file_size".to_string(), size.to_string());
                        node.metadata.insert("last_modified".to_string(), metadata.last_modified.to_string());
                        break;
                    }
                }
            } // Release mutable borrow
            info!("Updated node with metadata_id: {}", metadata_id);
            Ok(())
        } else {
            warn!("Node with metadata_id {} not found for update", metadata_id);
            Err(format!("Node with metadata_id {} not found", metadata_id))
        }
    }

    
    fn remove_node_by_metadata(&mut self, metadata_id: String) -> Result<(), String> {
        
        let node_id = self.node_map.values()
            .find(|n| n.metadata_id == metadata_id)
            .map(|n| n.id);

        if let Some(id) = node_id {
            self.remove_node(id);
            Ok(())
        } else {
            warn!("Node with metadata_id {} not found for removal", metadata_id);
            Err(format!("Node with metadata_id {} not found", metadata_id))
        }
    }

    
    fn compute_shortest_paths(&self, source_node_id: u32) -> Result<HashMap<u32, (f32, Vec<u32>)>, String> {
        if !self.node_map.contains_key(&source_node_id) {
            return Err(format!("Source node {} not found", source_node_id));
        }

        let mut distances: HashMap<u32, f32> = HashMap::new();
        let mut predecessors: HashMap<u32, u32> = HashMap::new();
        let mut unvisited: std::collections::BTreeSet<(ordered_float::OrderedFloat<f32>, u32)> = std::collections::BTreeSet::new();

        
        for &node_id in self.node_map.keys() {
            let distance = if node_id == source_node_id { 0.0 } else { f32::INFINITY };
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

        info!("Computed shortest paths from node {} to {} reachable nodes",
              source_node_id, result.len());

        Ok(result)
    }
}

impl Actor for GraphStateActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("GraphStateActor started - loading graph from Neo4j");

        let repository = Arc::clone(&self.repository);

        // Spawn async task to load graph from Neo4j
        ctx.spawn(
            async move {
                match repository.load_graph().await {
                    Ok(arc_graph_data) => {
                        info!("Successfully loaded graph from Neo4j: {} nodes, {} edges",
                              arc_graph_data.nodes.len(), arc_graph_data.edges.len());
                        Some(arc_graph_data)
                    }
                    Err(e) => {
                        error!("Failed to load graph from Neo4j: {}", e);
                        None
                    }
                }
            }
            .into_actor(self)
            .map(|graph_opt, act, _ctx| {
                if let Some(arc_graph_data) = graph_opt {
                    let arc_graph_data = Self::filter_empty_stubs(arc_graph_data);
                    act.graph_data = arc_graph_data.clone();

                    // ADR-014: No fallback edge generation. Edges come from Neo4j.

                    // Rebuild node_map and compact_to_neo4j (identity mapping).
                    act.remap_to_compact_ids();

                    // Next ID must exceed the max existing Neo4j ID
                    let max_id = act.graph_data.nodes.iter().map(|n| n.id).max().unwrap_or(0);
                    act.next_node_id.store(max_id + 1, std::sync::atomic::Ordering::SeqCst);

                    // Classify all loaded nodes into type sets (using compact IDs)
                    act.reclassify_all_nodes();

                    // Persist edges to Neo4j (fire-and-forget) — IDs are
                    // already the original Neo4j IDs (no remap to undo).
                    if !act.graph_data.edges.is_empty() {
                        let repo = Arc::clone(&act.repository);
                        let edges_to_save: Vec<Edge> = act.graph_data.edges.to_vec();
                        let node_count = act.graph_data.nodes.len();
                        actix::spawn(async move {
                            for edge in &edges_to_save {
                                if let Err(e) = repo.add_edge(edge).await {
                                    error!("Failed to persist edge {}->{}: {}", edge.source, edge.target, e);
                                }
                            }
                            debug!("Persisted {} edges to Neo4j for {} nodes", edges_to_save.len(), node_count);
                        });
                    }

                    debug!("GraphStateActor initialized with {} nodes, {} edges from Neo4j",
                          act.graph_data.nodes.len(), act.graph_data.edges.len());
                } else {
                    warn!("GraphStateActor starting with empty graph due to load failure");
                }
            }),
        );
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

        // Build a lookup using real graph node IDs (from gpu_index_to_graph_id)
        // when available, otherwise fall back to wire IDs from the tuple.
        let pos_map: std::collections::HashMap<u32, &crate::utils::socket_flow_messages::BinaryNodeDataClient> =
            if let Some(ref graph_ids) = msg.graph_node_ids {
                graph_ids.iter().zip(msg.positions.iter())
                    .map(|(gid, (_, data))| (*gid, data))
                    .collect()
            } else {
                msg.positions.iter().map(|(id, data)| (*id, data)).collect()
            };

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

        // Also update the node_map using real graph IDs
        let node_map = Arc::make_mut(&mut self.node_map);
        if let Some(ref graph_ids) = msg.graph_node_ids {
            for (gid, (_, pos)) in graph_ids.iter().zip(msg.positions.iter()) {
                if let Some(node) = node_map.get_mut(gid) {
                    node.data.x = pos.x;
                    node.data.y = pos.y;
                    node.data.z = pos.z;
                    node.data.vx = pos.vx;
                    node.data.vy = pos.vy;
                    node.data.vz = pos.vz;
                }
            }
        } else {
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
        }

        if updated > 0 {
            debug!("GraphStateActor: Updated {} node positions from GPU", updated);
        }
        Ok(())
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
        info!("BuildGraphFromMetadata handler called with {} metadata entries", msg.metadata.len());
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
        info!("Updating graph data with {} nodes, {} edges",
              msg.graph_data.nodes.len(), msg.graph_data.edges.len());

        self.graph_data = msg.graph_data;

        Arc::make_mut(&mut self.node_map).clear();
        for node in &self.graph_data.nodes {
            Arc::make_mut(&mut self.node_map).insert(node.id, node.clone());
        }

        // Reclassify all nodes after graph data update
        self.reclassify_all_nodes();

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

        
        let mut existing_positions: HashMap<String, (crate::types::vec3::Vec3Data, crate::types::vec3::Vec3Data)> = HashMap::new();
        for node in &self.bots_graph_data.nodes {
            existing_positions.insert(node.metadata_id.clone(), (node.data.position(), node.data.velocity()));
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

            
            node.metadata.insert("agent_type".to_string(), agent.agent_type.clone());
            node.metadata.insert("status".to_string(), agent.status.clone());
            node.metadata.insert("cpu_usage".to_string(), agent.cpu_usage.to_string());
            node.metadata.insert("memory_usage".to_string(), agent.memory_usage.to_string());
            node.metadata.insert("health".to_string(), agent.health.to_string());
            node.metadata.insert("is_agent".to_string(), "true".to_string());

            nodes.push(node);
        }

        
        for (i, source_agent) in msg.agents.iter().enumerate() {
            for (j, target_agent) in msg.agents.iter().enumerate() {
                if i != j {
                    let source_node_id = bot_id_offset + i as u32;
                    let target_node_id = bot_id_offset + j as u32;

                    let communication_intensity = if source_agent.agent_type == "coordinator" || target_agent.agent_type == "coordinator" {
                        0.8
                    } else if source_agent.status == "active" && target_agent.status == "active" {
                        0.5
                    } else {
                        0.2
                    };

                    if communication_intensity > 0.1 {
                        let mut edge = Edge::new(source_node_id, target_node_id, communication_intensity);
                        let metadata = edge.metadata.get_or_insert_with(HashMap::new);
                        metadata.insert("communication_type".to_string(), "agent_collaboration".to_string());
                        metadata.insert("intensity".to_string(), communication_intensity.to_string());
                        edges.push(edge);
                    }
                }
            }
        }

        
        let bots_graph_data_mut = Arc::make_mut(&mut self.bots_graph_data);
        bots_graph_data_mut.nodes = nodes;
        bots_graph_data_mut.edges = edges;

        // Classify all bots_graph_data nodes as agent nodes so binary protocol
        // sets bit 31 for them via NodeTypeArrays.
        for node in &self.bots_graph_data.nodes {
            self.agent_node_ids.insert(node.id);
        }

        info!("Updated bots graph with {} agents and {} edges (agent_node_ids={})",
             msg.agents.len(), self.bots_graph_data.edges.len(), self.agent_node_ids.len());
    }
}

impl Handler<ReloadGraphFromDatabase> for GraphStateActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, _msg: ReloadGraphFromDatabase, _ctx: &mut Self::Context) -> Self::Result {
        info!("GraphStateActor: ReloadGraphFromDatabase - reloading graph from Neo4j");

        let repository = Arc::clone(&self.repository);

        Box::pin(
            async move {
                match repository.load_graph().await {
                    Ok(arc_graph_data) => {
                        info!(
                            "GraphStateActor: Reloaded graph from Neo4j: {} nodes, {} edges",
                            arc_graph_data.nodes.len(),
                            arc_graph_data.edges.len()
                        );
                        Ok(arc_graph_data)
                    }
                    Err(e) => {
                        error!("GraphStateActor: Failed to reload graph from Neo4j: {}", e);
                        Err(format!("Failed to reload graph: {}", e))
                    }
                }
            }
            .into_actor(self)
            .map(|result, act, _ctx| {
                match result {
                    Ok(arc_graph_data) => {
                        let arc_graph_data = Self::filter_empty_stubs(arc_graph_data);
                        act.graph_data = arc_graph_data.clone();

                        // ADR-014: No fallback edge generation. Edges come from Neo4j.

                        // Rebuild node_map (identity mapping, no ID rewrite)
                        act.remap_to_compact_ids();

                        // Next ID must exceed the max existing Neo4j ID
                        let max_id = act.graph_data.nodes.iter().map(|n| n.id).max().unwrap_or(0);
                        act.next_node_id.store(max_id + 1, std::sync::atomic::Ordering::SeqCst);

                        // Reclassify all nodes after reload (using compact IDs)
                        act.reclassify_all_nodes();

                        // Persist edges to Neo4j (fire-and-forget) — IDs
                        // are the original Neo4j IDs, no translation needed.
                        if !act.graph_data.edges.is_empty() {
                            let repo = Arc::clone(&act.repository);
                            let edges_to_save: Vec<Edge> = act.graph_data.edges.to_vec();
                            actix::spawn(async move {
                                for edge in &edges_to_save {
                                    if let Err(e) = repo.add_edge(edge).await {
                                        error!("Failed to persist edge: {}", e);
                                    }
                                }
                                debug!("Persisted {} generated edges to Neo4j", edges_to_save.len());
                            });
                        }

                        debug!(
                            "GraphStateActor: State updated after reload - {} nodes, {} edges",
                            act.graph_data.nodes.len(),
                            act.graph_data.edges.len(),
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
                info!("Computed shortest paths from node {}: {} reachable nodes",
                      msg.source_node_id, paths.len());

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

// =============================================================================
// Unit tests for configure_node_from_metadata node_type population (ADR-036)
// =============================================================================
// These tests verify that configure_node_from_metadata correctly populates
// node.node_type based on metadata fields. Before the fix, node_type was
// never set, causing all incrementally-added nodes to fall into the Knowledge
// population by default.
//
// Tests call configure_node_from_metadata directly (private method accessible
// from inline test module) to avoid requiring an Actix runtime. Classification
// tests call classify_node directly for the same reason.
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::metadata::Metadata;
    use crate::ports::knowledge_graph_repository::{
        GraphStatistics, KnowledgeGraphRepository,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;

    // ── Stub repository (no-op persistence for unit tests) ──────────
    struct StubRepository;

    #[async_trait]
    impl KnowledgeGraphRepository for StubRepository {
        async fn load_graph(&self) -> crate::ports::knowledge_graph_repository::Result<Arc<GraphData>> {
            Ok(Arc::new(GraphData::new()))
        }
        async fn save_graph(&self, _graph: &GraphData) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn add_node(&self, _node: &Node) -> crate::ports::knowledge_graph_repository::Result<u32> {
            Ok(0)
        }
        async fn batch_add_nodes(&self, _nodes: Vec<Node>) -> crate::ports::knowledge_graph_repository::Result<Vec<u32>> {
            Ok(vec![])
        }
        async fn update_node(&self, _node: &Node) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn batch_update_nodes(&self, _nodes: Vec<Node>) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn remove_node(&self, _node_id: u32) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn batch_remove_nodes(&self, _node_ids: Vec<u32>) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn get_node(&self, _node_id: u32) -> crate::ports::knowledge_graph_repository::Result<Option<Node>> {
            Ok(None)
        }
        async fn get_nodes(&self, _node_ids: Vec<u32>) -> crate::ports::knowledge_graph_repository::Result<Vec<Node>> {
            Ok(vec![])
        }
        async fn get_nodes_by_metadata_id(&self, _metadata_id: &str) -> crate::ports::knowledge_graph_repository::Result<Vec<Node>> {
            Ok(vec![])
        }
        async fn get_nodes_by_owl_class_iri(&self, _iri: &str) -> crate::ports::knowledge_graph_repository::Result<Vec<Node>> {
            Ok(vec![])
        }
        async fn search_nodes_by_label(&self, _label: &str) -> crate::ports::knowledge_graph_repository::Result<Vec<Node>> {
            Ok(vec![])
        }
        async fn add_edge(&self, _edge: &Edge) -> crate::ports::knowledge_graph_repository::Result<String> {
            Ok(String::new())
        }
        async fn batch_add_edges(&self, _edges: Vec<Edge>) -> crate::ports::knowledge_graph_repository::Result<Vec<String>> {
            Ok(vec![])
        }
        async fn update_edge(&self, _edge: &Edge) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn remove_edge(&self, _edge_id: &str) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn batch_remove_edges(&self, _edge_ids: Vec<String>) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn get_node_edges(&self, _node_id: u32) -> crate::ports::knowledge_graph_repository::Result<Vec<Edge>> {
            Ok(vec![])
        }
        async fn get_edges_between(&self, _src: u32, _tgt: u32) -> crate::ports::knowledge_graph_repository::Result<Vec<Edge>> {
            Ok(vec![])
        }
        async fn batch_update_positions(&self, _positions: Vec<(u32, f32, f32, f32)>) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn get_all_positions(&self) -> crate::ports::knowledge_graph_repository::Result<HashMap<u32, (f32, f32, f32)>> {
            Ok(HashMap::new())
        }
        async fn query_nodes(&self, _query: &str) -> crate::ports::knowledge_graph_repository::Result<Vec<Node>> {
            Ok(vec![])
        }
        async fn get_neighbors(&self, _node_id: u32) -> crate::ports::knowledge_graph_repository::Result<Vec<Node>> {
            Ok(vec![])
        }
        async fn get_statistics(&self) -> crate::ports::knowledge_graph_repository::Result<GraphStatistics> {
            Ok(GraphStatistics {
                node_count: 0,
                edge_count: 0,
                average_degree: 0.0,
                connected_components: 0,
                last_updated: chrono::Utc::now(),
            })
        }
        async fn clear_graph(&self) -> crate::ports::knowledge_graph_repository::Result<()> {
            Ok(())
        }
        async fn health_check(&self) -> crate::ports::knowledge_graph_repository::Result<bool> {
            Ok(true)
        }
    }

    // ── Helper: create actor with stub repository ───────────────────
    fn make_actor() -> GraphStateActor {
        GraphStateActor::new(Arc::new(StubRepository))
    }

    // ── Helper: create minimal Metadata ─────────────────────────────
    fn base_metadata() -> Metadata {
        Metadata {
            file_name: "test-node.md".to_string(),
            file_size: 1024,
            ..Default::default()
        }
    }

    // ── Helper: create a fresh Node with node_type = None ───────────
    fn fresh_node(id: u32) -> Node {
        Node::new_with_id("test".to_string(), Some(id))
    }

    // ================================================================
    // Test 1: Metadata with owl_class sets node_type to "owl_class"
    // ================================================================
    // GIVEN: A fresh Node (node_type = None) and Metadata with owl_class set
    // WHEN:  configure_node_from_metadata is called
    // THEN:  node.node_type == Some("owl_class")
    #[test]
    fn test_metadata_with_owl_class_sets_ontology_type() {
        let actor = make_actor();
        let mut node = fresh_node(100);
        let mut meta = base_metadata();
        meta.owl_class = Some("MyClass".to_string());

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.node_type,
            Some("owl_class".to_string()),
            "Metadata with owl_class should produce node_type 'owl_class'"
        );
    }

    // ================================================================
    // Test 2: Metadata with source_domain sets node_type to "owl_class"
    // ================================================================
    // GIVEN: A fresh Node and Metadata with source_domain but no owl_class
    // WHEN:  configure_node_from_metadata is called
    // THEN:  node.node_type == Some("owl_class")
    #[test]
    fn test_metadata_with_source_domain_sets_ontology_type() {
        let actor = make_actor();
        let mut node = fresh_node(101);
        let mut meta = base_metadata();
        meta.source_domain = Some("physics".to_string());

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.node_type,
            Some("owl_class".to_string()),
            "Metadata with source_domain should produce node_type 'owl_class'"
        );
    }

    // ================================================================
    // Test 3: Plain metadata without ontology fields defaults to "page"
    // ================================================================
    // GIVEN: A fresh Node and Metadata with no owl_class, no source_domain
    // WHEN:  configure_node_from_metadata is called
    // THEN:  node.node_type == Some("page")
    #[test]
    fn test_metadata_without_ontology_sets_page_type() {
        let actor = make_actor();
        let mut node = fresh_node(102);
        let meta = base_metadata();

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.node_type,
            Some("page".to_string()),
            "Plain metadata should produce node_type 'page'"
        );
    }

    // ================================================================
    // Test 4: Pre-existing node_type is not overwritten
    // ================================================================
    // GIVEN: A Node with node_type = Some("agent")
    // WHEN:  configure_node_from_metadata is called with plain metadata
    // THEN:  node_type remains Some("agent")
    #[test]
    fn test_existing_node_type_not_overwritten() {
        let actor = make_actor();
        let mut node = fresh_node(103);
        node.node_type = Some("agent".to_string());
        let meta = base_metadata();

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.node_type,
            Some("agent".to_string()),
            "Pre-existing node_type 'agent' must not be overwritten"
        );
    }

    // ================================================================
    // Test 5: Pre-existing "owl_individual" not overwritten by owl_class metadata
    // ================================================================
    // GIVEN: A Node with node_type = Some("owl_individual")
    // WHEN:  configure_node_from_metadata is called with owl_class metadata
    // THEN:  node_type remains Some("owl_individual"), not changed to "owl_class"
    #[test]
    fn test_existing_owl_individual_not_overwritten() {
        let actor = make_actor();
        let mut node = fresh_node(104);
        node.node_type = Some("owl_individual".to_string());
        let mut meta = base_metadata();
        meta.owl_class = Some("SomeClass".to_string());

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.node_type,
            Some("owl_individual".to_string()),
            "Pre-existing node_type 'owl_individual' must not be overwritten"
        );
    }

    // ================================================================
    // Test 6: Both owl_class and source_domain present
    // ================================================================
    // GIVEN: Metadata with both owl_class and source_domain
    // WHEN:  configure_node_from_metadata is called
    // THEN:  node_type == Some("owl_class") (OR condition, not AND)
    #[test]
    fn test_metadata_with_both_owl_class_and_source_domain() {
        let actor = make_actor();
        let mut node = fresh_node(105);
        let mut meta = base_metadata();
        meta.owl_class = Some("PhysicalEntity".to_string());
        meta.source_domain = Some("physics".to_string());

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.node_type,
            Some("owl_class".to_string()),
            "Metadata with both owl_class and source_domain should produce 'owl_class'"
        );
    }

    // ================================================================
    // Test 7: Classification of node_type="owl_class" into Ontology set
    // ================================================================
    // GIVEN: A node with node_type = Some("owl_class") (as set by the fix)
    // WHEN:  classify_node is called
    // THEN:  The node ID appears in ontology_class_ids, not knowledge_node_ids
    #[test]
    fn test_owl_class_classified_as_ontology() {
        let mut actor = make_actor();
        let mut node = fresh_node(200);
        node.node_type = Some("owl_class".to_string());

        actor.classify_node(&node);

        assert!(
            actor.ontology_class_ids.contains(&200),
            "Node with type 'owl_class' should be in ontology_class_ids"
        );
        assert!(
            !actor.knowledge_node_ids.contains(&200),
            "Node with type 'owl_class' should NOT be in knowledge_node_ids"
        );
    }

    // ================================================================
    // Test 8: Classification of node_type="page" into Knowledge set
    // ================================================================
    // GIVEN: A node with node_type = Some("page")
    // WHEN:  classify_node is called
    // THEN:  The node ID appears in knowledge_node_ids, not ontology sets
    #[test]
    fn test_page_classified_as_knowledge() {
        let mut actor = make_actor();
        let mut node = fresh_node(201);
        node.node_type = Some("page".to_string());

        actor.classify_node(&node);

        assert!(
            actor.knowledge_node_ids.contains(&201),
            "Node with type 'page' should be in knowledge_node_ids"
        );
        assert!(
            !actor.ontology_class_ids.contains(&201),
            "Node with type 'page' should NOT be in ontology_class_ids"
        );
    }

    // ================================================================
    // Test 9: End-to-end configure + classify for ontology node
    // ================================================================
    // GIVEN: A fresh node and metadata with owl_class
    // WHEN:  configure_node_from_metadata then classify_node are called
    // THEN:  node_type is "owl_class" AND node lands in ontology_class_ids
    #[test]
    fn test_configure_then_classify_ontology() {
        let mut actor = make_actor();
        let mut node = fresh_node(300);
        let mut meta = base_metadata();
        meta.owl_class = Some("Concept".to_string());

        actor.configure_node_from_metadata(&mut node, &meta);
        actor.classify_node(&node);

        assert_eq!(node.node_type, Some("owl_class".to_string()));
        assert!(actor.ontology_class_ids.contains(&300));
        assert!(!actor.knowledge_node_ids.contains(&300));
    }

    // ================================================================
    // Test 10: End-to-end configure + classify for knowledge node
    // ================================================================
    // GIVEN: A fresh node and plain metadata (no ontology fields)
    // WHEN:  configure_node_from_metadata then classify_node are called
    // THEN:  node_type is "page" AND node lands in knowledge_node_ids
    #[test]
    fn test_configure_then_classify_knowledge() {
        let mut actor = make_actor();
        let mut node = fresh_node(301);
        let meta = base_metadata();

        actor.configure_node_from_metadata(&mut node, &meta);
        actor.classify_node(&node);

        assert_eq!(node.node_type, Some("page".to_string()));
        assert!(actor.knowledge_node_ids.contains(&301));
        assert!(!actor.ontology_class_ids.contains(&301));
    }

    // ================================================================
    // Test 11: Metadata fields copied to node.metadata map
    // ================================================================
    // GIVEN: Metadata with source_domain, quality_score, authority_score
    // WHEN:  configure_node_from_metadata is called
    // THEN:  Those values appear in node.metadata HashMap
    #[test]
    fn test_metadata_fields_copied_to_node_metadata() {
        let actor = make_actor();
        let mut node = fresh_node(400);
        let mut meta = base_metadata();
        meta.source_domain = Some("chemistry".to_string());
        meta.quality_score = Some(0.95);
        meta.authority_score = Some(0.87);

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.metadata.get("source_domain"),
            Some(&"chemistry".to_string()),
            "source_domain should be copied to node.metadata"
        );
        assert_eq!(
            node.metadata.get("quality_score"),
            Some(&"0.95".to_string()),
            "quality_score should be copied to node.metadata"
        );
        assert_eq!(
            node.metadata.get("authority_score"),
            Some(&"0.87".to_string()),
            "authority_score should be copied to node.metadata"
        );
    }

    // ================================================================
    // Test 12: Label and color set from metadata
    // ================================================================
    // GIVEN: Metadata with file_name "concept.rs"
    // WHEN:  configure_node_from_metadata is called
    // THEN:  node.label == "concept.rs" and node.color is the Rust color
    #[test]
    fn test_label_and_color_from_metadata() {
        let actor = make_actor();
        let mut node = fresh_node(401);
        let mut meta = base_metadata();
        meta.file_name = "concept.rs".to_string();

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(node.label, "concept.rs");
        assert_eq!(
            node.color,
            Some("#CE422B".to_string()),
            ".rs files should get the Rust color"
        );
    }

    // ================================================================
    // Test 13: Size calculated from file_size
    // ================================================================
    // GIVEN: Metadata with file_size = 5000
    // WHEN:  configure_node_from_metadata is called
    // THEN:  node.size == Some(10.0 + (5000.0 / 1000.0).min(50.0)) == Some(15.0)
    #[test]
    fn test_size_from_file_size() {
        let actor = make_actor();
        let mut node = fresh_node(402);
        let mut meta = base_metadata();
        meta.file_size = 5000;

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.size,
            Some(15.0),
            "Size should be 10.0 + (5000/1000).min(50) = 15.0"
        );
    }

    // ================================================================
    // Test 14: is_subclass_of copied to node.metadata
    // ================================================================
    // GIVEN: Metadata with is_subclass_of = ["ParentA", "ParentB"]
    // WHEN:  configure_node_from_metadata is called
    // THEN:  node.metadata["is_subclass_of"] == "ParentA,ParentB"
    #[test]
    fn test_is_subclass_of_copied() {
        let actor = make_actor();
        let mut node = fresh_node(403);
        let mut meta = base_metadata();
        meta.is_subclass_of = vec!["ParentA".to_string(), "ParentB".to_string()];

        actor.configure_node_from_metadata(&mut node, &meta);

        assert_eq!(
            node.metadata.get("is_subclass_of"),
            Some(&"ParentA,ParentB".to_string()),
            "is_subclass_of should be joined with commas"
        );
    }
}