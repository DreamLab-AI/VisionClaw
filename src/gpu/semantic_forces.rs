//! Semantic Forces Engine
//!
//! Derives the metadata the GPU semantic-force kernels (`semantic_forces.cu`, launched from
//! `utils/unified_gpu_compute/semantic.rs`) read: node type, DAG hierarchy level, physicality,
//! role, maturity, cross-domain link counts and edge types. Force parameters are NOT here: they
//! are the central settings store's `physics.semantic` group. The former CPU twins of the
//! kernels (which mutated `GraphData` positions behind a `gpu_available()` check) are gone:
//! a force either runs on the GPU from the stored settings or does not run.

use crate::services::semantic_type_registry::{RelationshipForceConfig, SemanticTypeRegistry};
use log::{debug, info};
use std::collections::HashMap;
use std::sync::Arc;
use visionclaw_domain::models::graph::GraphData;

// The force configuration is the central settings store's `physics.semantic` group; see
// `visionclaw_domain::types::semantic_config`. This module only DERIVES the per-node and
// per-edge metadata the GPU kernels read (types, hierarchy levels, physicality, role, maturity,
// cross-domain links, edge types); it holds no force parameters.

// =============================================================================
// Semantic Forces Engine
// =============================================================================

/// Metadata deriver for the GPU semantic forces.
pub struct SemanticForcesEngine {
    node_hierarchy_levels: Vec<i32>,
    node_types: Vec<i32>,
    edge_types: Vec<i32>,
    node_physicality: Vec<i32>, // 0=None, 1=VirtualEntity, 2=PhysicalEntity, 3=ConceptualEntity
    node_role: Vec<i32>,        // 0=None, 1=Process, 2=Agent, 3=Resource, 4=Concept
    node_maturity: Vec<i32>,    // 0=None, 1=emerging, 2=mature, 3=declining
    node_cross_domain_count: Vec<i32>, // Count of cross-domain links per node
    initialized: bool,
    /// Dynamic semantic type registry for ontology-code decoupling
    registry: Arc<SemanticTypeRegistry>,
}

impl SemanticForcesEngine {
    /// Create an engine over the global default registry.
    pub fn new() -> Self {
        Self::with_registry(Arc::new(SemanticTypeRegistry::new()))
    }

    /// Create an engine with a custom registry
    pub fn with_registry(registry: Arc<SemanticTypeRegistry>) -> Self {
        Self {
            node_hierarchy_levels: Vec::new(),
            node_types: Vec::new(),
            edge_types: Vec::new(),
            node_physicality: Vec::new(),
            node_role: Vec::new(),
            node_maturity: Vec::new(),
            node_cross_domain_count: Vec::new(),
            initialized: false,
            registry,
        }
    }

    /// Get a reference to the semantic type registry
    pub fn registry(&self) -> &SemanticTypeRegistry {
        &self.registry
    }

    /// Build GPU buffer of relationship force configurations
    pub fn build_relationship_gpu_buffer(&self) -> Vec<RelationshipForceConfig> {
        self.registry.build_gpu_buffer()
    }

    /// Derive every metadata array from `graph`. Everything is derived regardless of which
    /// forces are enabled: toggling a force in the settings store must not require re-deriving.
    pub fn initialize(&mut self, graph: &GraphData) -> Result<(), String> {
        info!(
            "Deriving semantic metadata for {} nodes, {} edges",
            graph.nodes.len(),
            graph.edges.len()
        );

        self.node_types = graph.nodes.iter().map(|node| self.node_type_to_int(&node.node_type)).collect();
        self.node_physicality = graph.nodes.iter().map(|node| self.extract_physicality(node)).collect();
        self.node_role = graph.nodes.iter().map(|node| self.extract_role(node)).collect();
        self.node_maturity = graph.nodes.iter().map(|node| self.extract_maturity(node)).collect();
        self.node_cross_domain_count = self.calculate_cross_domain_counts(graph);
        self.edge_types = graph.edges.iter().map(|edge| self.edge_type_to_int(&edge.edge_type)).collect();
        self.calculate_hierarchy_levels(graph)?;

        self.initialized = true;
        info!("Semantic metadata derived");
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get node hierarchy levels
    pub fn hierarchy_levels(&self) -> &[i32] {
        &self.node_hierarchy_levels
    }

    /// Get node types
    pub fn node_types(&self) -> &[i32] {
        &self.node_types
    }

    /// Per-node ontology physicality (0=None, 1=Virtual, 2=Physical, 3=Conceptual).
    pub fn node_physicality(&self) -> &[i32] {
        &self.node_physicality
    }

    /// Per-node ontology role (0=None, 1=Process, 2=Agent, 3=Resource, 4=Concept).
    pub fn node_role(&self) -> &[i32] {
        &self.node_role
    }

    /// Per-node maturity stage (0=None, 1=emerging, 2=mature, 3=declining).
    pub fn node_maturity(&self) -> &[i32] {
        &self.node_maturity
    }

    /// Per-node count of cross-domain links.
    pub fn node_cross_domain_count(&self) -> &[i32] {
        &self.node_cross_domain_count
    }

    /// Per-edge type id from the semantic type registry, aligned with `graph.edges`.
    pub fn edge_types(&self) -> &[i32] {
        &self.edge_types
    }

    // Private helper methods

    fn node_type_to_int(&self, node_type: &Option<String>) -> i32 {
        match node_type.as_deref() {
            None | Some("generic") => 0,
            Some("person") => 1,
            Some("organization") => 2,
            Some("project") => 3,
            Some("task") => 4,
            Some("concept") => 5,
            Some("class") => 6,
            Some("individual") => 7,
            Some(_) => 8, // Custom types
        }
    }

    /// Convert edge type to integer using dynamic registry lookup
    /// Decouples ontology from CUDA compilation - new types are registered at runtime
    fn edge_type_to_int(&self, edge_type: &Option<String>) -> i32 {
        self.registry.edge_type_to_int(edge_type)
    }

    /// Extract physicality classification from node metadata
    fn extract_physicality(&self, node: &visionclaw_domain::models::node::Node) -> i32 {
        // Check owl:physicality metadata
        if let Some(physicality) = node
            .metadata
            .get("owl:physicality")
            .or_else(|| node.metadata.get("physicality"))
        {
            return match physicality.as_str() {
                "VirtualEntity" => 1,
                "PhysicalEntity" => 2,
                "ConceptualEntity" => 3,
                _ => 0,
            };
        }
        0 // None
    }

    /// Extract role classification from node metadata
    fn extract_role(&self, node: &visionclaw_domain::models::node::Node) -> i32 {
        // Check owl:role metadata
        if let Some(role) = node
            .metadata
            .get("owl:role")
            .or_else(|| node.metadata.get("role"))
        {
            return match role.as_str() {
                "Process" => 1,
                "Agent" => 2,
                "Resource" => 3,
                "Concept" => 4,
                _ => 0,
            };
        }
        0 // None
    }

    /// Extract maturity stage from node metadata
    fn extract_maturity(&self, node: &visionclaw_domain::models::node::Node) -> i32 {
        // Check maturity metadata
        if let Some(maturity) = node.metadata.get("maturity") {
            return match maturity.as_str() {
                "emerging" => 1,
                "mature" => 2,
                "declining" => 3,
                _ => 0,
            };
        }
        0 // None
    }

    /// Calculate cross-domain link counts for each node
    fn calculate_cross_domain_counts(&self, graph: &GraphData) -> Vec<i32> {
        let mut counts = vec![0; graph.nodes.len()];

        // Build node ID to index map
        let node_id_to_idx: HashMap<u32, usize> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(idx, node)| (node.id, idx))
            .collect();

        // Count cross-domain links in metadata
        for (idx, node) in graph.nodes.iter().enumerate() {
            if let Some(links) = node.metadata.get("cross-domain-links") {
                // Count comma-separated links
                counts[idx] = links.split(',').filter(|s| !s.trim().is_empty()).count() as i32;
            }
        }

        // Also count edges with type "bridges-to"
        for edge in &graph.edges {
            if edge.edge_type.as_deref() == Some("bridges-to") {
                if let Some(&src_idx) = node_id_to_idx.get(&edge.source) {
                    counts[src_idx] += 1;
                }
                if let Some(&tgt_idx) = node_id_to_idx.get(&edge.target) {
                    counts[tgt_idx] += 1;
                }
            }
        }

        counts
    }

    fn calculate_hierarchy_levels(&mut self, graph: &GraphData) -> Result<(), String> {
        debug!(
            "Calculating hierarchy levels for {} nodes",
            graph.nodes.len()
        );

        // Initialize all levels to -1 (not in DAG)
        self.node_hierarchy_levels = vec![-1; graph.nodes.len()];

        // Build adjacency list for hierarchy edges
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut has_parent: HashMap<u32, bool> = HashMap::new();

        for edge in &graph.edges {
            if edge.edge_type.as_deref() == Some("hierarchy") {
                children.entry(edge.source).or_default().push(edge.target);
                has_parent.insert(edge.target, true);
            }
        }

        // id -> index, once (a per-edge scan of every node made this quadratic).
        let index_of: HashMap<u32, usize> =
            graph.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();

        // Find root nodes (nodes without parents)
        let mut roots = Vec::new();
        for (i, node) in graph.nodes.iter().enumerate() {
            if !has_parent.contains_key(&node.id) {
                // Check if this node has any hierarchy children
                if children.contains_key(&node.id) {
                    roots.push(i);
                    self.node_hierarchy_levels[i] = 0;
                }
            }
        }

        debug!("Found {} root nodes for DAG layout", roots.len());

        // BFS to assign levels
        let mut queue = roots.clone();
        let mut processed = 0;

        while !queue.is_empty() && processed < graph.nodes.len() * 2 {
            let mut next_queue = Vec::new();

            for node_idx in &queue {
                let node_id = graph.nodes[*node_idx].id;
                let current_level = self.node_hierarchy_levels[*node_idx];

                if let Some(child_ids) = children.get(&node_id) {
                    for child_id in child_ids {
                        if let Some(&child_idx) = index_of.get(child_id) {
                            let new_level = current_level + 1;
                            if self.node_hierarchy_levels[child_idx] < new_level {
                                self.node_hierarchy_levels[child_idx] = new_level;
                                next_queue.push(child_idx);
                            }
                        }
                    }
                }
            }

            queue = next_queue;
            processed += 1;
        }

        let nodes_in_dag = self
            .node_hierarchy_levels
            .iter()
            .filter(|&&l| l >= 0)
            .count();
        info!("Hierarchy levels calculated: {} nodes in DAG", nodes_in_dag);

        Ok(())
    }
}

impl Default for SemanticForcesEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::node::Node;

    #[test]
    fn test_engine_creation() {
        assert!(!SemanticForcesEngine::new().is_initialized());
    }

    #[test]
    fn test_engine_initialization_derives_every_array_per_node() {
        let mut engine = SemanticForcesEngine::new();
        let mut graph = GraphData::new();
        let mut node1 = Node::new("node1".to_string());
        node1.node_type = Some("person".to_string());
        graph.nodes.push(node1);
        graph.nodes.push(Node::new("node2".to_string()));

        engine.initialize(&graph).unwrap();
        assert!(engine.is_initialized());
        for (name, len) in [
            ("types", engine.node_types().len()),
            ("levels", engine.hierarchy_levels().len()),
            ("physicality", engine.node_physicality().len()),
            ("role", engine.node_role().len()),
            ("maturity", engine.node_maturity().len()),
            ("cross-domain", engine.node_cross_domain_count().len()),
        ] {
            assert_eq!(len, 2, "{name}: one entry per node, whichever forces are enabled");
        }
        assert_eq!(engine.node_types()[0], 1, "person -> 1");
    }

    #[test]
    fn test_hierarchy_calculation() {
        let mut engine = SemanticForcesEngine::new();

        let mut graph = GraphData::new();
        let parent = Node::new("parent".to_string()).with_label("Parent".to_string());
        let parent_id = parent.id;
        graph.nodes.push(parent);
        let child = Node::new("child".to_string()).with_label("Child".to_string());
        let child_id = child.id;
        graph.nodes.push(child);

        let mut edge = Edge::new(parent_id, child_id, 1.0);
        edge.edge_type = Some("hierarchy".to_string());
        graph.edges.push(edge);

        engine.initialize(&graph).unwrap();

        let levels = engine.hierarchy_levels();
        assert_eq!(levels, &[0, 1], "parent at level 0, child at level 1");
    }

    #[test]
    fn hierarchy_levels_scale_to_a_wide_tree_without_a_per_edge_node_scan() {
        // 5,000 children under one root: the old lookup scanned every node per edge (25M steps).
        let mut graph = GraphData::new();
        let root = Node::new("root".to_string());
        let root_id = root.id;
        graph.nodes.push(root);
        for i in 0..5000 {
            let c = Node::new(format!("c{i}"));
            let cid = c.id;
            graph.nodes.push(c);
            let mut e = Edge::new(root_id, cid, 1.0);
            e.edge_type = Some("hierarchy".to_string());
            graph.edges.push(e);
        }
        let mut engine = SemanticForcesEngine::new();
        let started = std::time::Instant::now();
        engine.initialize(&graph).unwrap();
        assert_eq!(engine.hierarchy_levels()[0], 0);
        assert!(engine.hierarchy_levels()[1..].iter().all(|&l| l == 1));
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "took {:?}", started.elapsed());
    }
}
