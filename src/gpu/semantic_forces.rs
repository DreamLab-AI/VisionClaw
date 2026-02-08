//! Semantic Forces Engine
//!
//! GPU-accelerated semantic physics forces for knowledge graph layout.
//! Implements DAG layout, type clustering, collision detection, and attribute-weighted springs.

use crate::models::graph::GraphData;
use crate::services::semantic_type_registry::{SemanticTypeRegistry, RelationshipForceConfig};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// =============================================================================
// Configuration Structures
// =============================================================================

/// DAG layout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAGConfig {
    /// Vertical separation between hierarchy levels
    pub vertical_spacing: f32,
    /// Minimum horizontal separation within a level
    pub horizontal_spacing: f32,
    /// Strength of attraction to target level
    pub level_attraction: f32,
    /// Repulsion between nodes at same level
    pub sibling_repulsion: f32,
    /// Enable DAG layout forces
    pub enabled: bool,
}

impl Default for DAGConfig {
    fn default() -> Self {
        Self {
            vertical_spacing: 100.0,
            horizontal_spacing: 50.0,
            level_attraction: 0.5,
            sibling_repulsion: 0.3,
            enabled: true,
        }
    }
}

/// Type clustering configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeClusterConfig {
    /// Attraction between nodes of same type
    pub cluster_attraction: f32,
    /// Target radius for type clusters
    pub cluster_radius: f32,
    /// Repulsion between different type clusters
    pub inter_cluster_repulsion: f32,
    /// Enable type clustering
    pub enabled: bool,
}

impl Default for TypeClusterConfig {
    fn default() -> Self {
        Self {
            cluster_attraction: 0.4,
            cluster_radius: 150.0,
            inter_cluster_repulsion: 0.2,
            enabled: true,
        }
    }
}

/// Collision detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionConfig {
    /// Minimum allowed distance between nodes
    pub min_distance: f32,
    /// Force strength when colliding
    pub collision_strength: f32,
    /// Default node radius
    pub node_radius: f32,
    /// Enable collision detection
    pub enabled: bool,
}

impl Default for CollisionConfig {
    fn default() -> Self {
        Self {
            min_distance: 5.0,
            collision_strength: 1.0,
            node_radius: 10.0,
            enabled: true,
        }
    }
}

/// Attribute-weighted spring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeSpringConfig {
    /// Base spring constant
    pub base_spring_k: f32,
    /// Multiplier for edge weight influence
    pub weight_multiplier: f32,
    /// Minimum rest length
    pub rest_length_min: f32,
    /// Maximum rest length
    pub rest_length_max: f32,
    /// Enable attribute-weighted springs
    pub enabled: bool,
}

impl Default for AttributeSpringConfig {
    fn default() -> Self {
        Self {
            base_spring_k: 0.01,
            weight_multiplier: 0.5,
            rest_length_min: 30.0,
            rest_length_max: 200.0,
            enabled: true,
        }
    }
}

/// Ontology relationship forces configuration
/// NOTE: The `enabled` field is used as a feature toggle for the CPU fallback.
/// Force parameters in this struct are legacy defaults - actual force configs
/// are now loaded dynamically from SemanticTypeRegistry at runtime.
/// GPU uses DynamicRelationshipBuffer for ontology-to-code decoupling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyRelationshipConfig {
    /// Legacy: "requires" strength (actual config from SemanticTypeRegistry)
    pub requires_strength: f32,
    /// Legacy: "requires" rest length
    pub requires_rest_length: f32,
    /// Legacy: "enables" strength
    pub enables_strength: f32,
    /// Legacy: "enables" rest length
    pub enables_rest_length: f32,
    /// Legacy: "has-part" strength
    pub has_part_strength: f32,
    /// Legacy: "has-part" orbit radius
    pub has_part_orbit_radius: f32,
    /// Legacy: "bridges-to" strength
    pub bridges_to_strength: f32,
    /// Legacy: "bridges-to" rest length
    pub bridges_to_rest_length: f32,
    /// Enable ontology relationship forces (feature toggle for CPU fallback)
    pub enabled: bool,
}

impl Default for OntologyRelationshipConfig {
    fn default() -> Self {
        Self {
            requires_strength: 0.7,
            requires_rest_length: 80.0,
            enables_strength: 0.4,
            enables_rest_length: 120.0,
            has_part_strength: 0.9,
            has_part_orbit_radius: 60.0,
            bridges_to_strength: 0.3,
            bridges_to_rest_length: 250.0,
            enabled: true,
        }
    }
}

/// Physicality-based clustering configuration (VirtualEntity, PhysicalEntity, ConceptualEntity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalityClusterConfig {
    /// Attraction between nodes of same physicality
    pub cluster_attraction: f32,
    /// Target radius for physicality clusters
    pub cluster_radius: f32,
    /// Repulsion between different physicality types
    pub inter_physicality_repulsion: f32,
    /// Enable physicality clustering
    pub enabled: bool,
}

impl Default for PhysicalityClusterConfig {
    fn default() -> Self {
        Self {
            cluster_attraction: 0.5,
            cluster_radius: 180.0,
            inter_physicality_repulsion: 0.25,
            enabled: true,
        }
    }
}

/// Role-based clustering configuration (Process, Agent, Resource, Concept)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleClusterConfig {
    /// Attraction between nodes of same role
    pub cluster_attraction: f32,
    /// Target radius for role clusters
    pub cluster_radius: f32,
    /// Repulsion between different roles
    pub inter_role_repulsion: f32,
    /// Enable role clustering
    pub enabled: bool,
}

impl Default for RoleClusterConfig {
    fn default() -> Self {
        Self {
            cluster_attraction: 0.45,
            cluster_radius: 160.0,
            inter_role_repulsion: 0.2,
            enabled: true,
        }
    }
}

/// Maturity-based layout configuration (emerging → mature → declining)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaturityLayoutConfig {
    /// Vertical spacing between maturity stages
    pub vertical_spacing: f32,
    /// Attraction to target maturity level
    pub level_attraction: f32,
    /// Maturity stage ordering: emerging=0, mature=1, declining=2
    pub stage_separation: f32,
    /// Enable maturity-based layout
    pub enabled: bool,
}

impl Default for MaturityLayoutConfig {
    fn default() -> Self {
        Self {
            vertical_spacing: 150.0,
            level_attraction: 0.4,
            stage_separation: 100.0,
            enabled: true,
        }
    }
}

/// Cross-domain link strength configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDomainConfig {
    /// Base strength for cross-domain links
    pub base_strength: f32,
    /// Multiplier based on link count (more links = stronger forces)
    pub link_count_multiplier: f32,
    /// Maximum strength boost from link count
    pub max_strength_boost: f32,
    /// Rest length for cross-domain connections
    pub rest_length: f32,
    /// Enable cross-domain forces
    pub enabled: bool,
}

impl Default for CrossDomainConfig {
    fn default() -> Self {
        Self {
            base_strength: 0.3,
            link_count_multiplier: 0.1,
            max_strength_boost: 2.0,
            rest_length: 200.0,
            enabled: true,
        }
    }
}

/// Unified semantic configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConfig {
    pub dag: DAGConfig,
    pub type_cluster: TypeClusterConfig,
    pub collision: CollisionConfig,
    pub attribute_spring: AttributeSpringConfig,
    pub ontology_relationship: OntologyRelationshipConfig,
    pub physicality_cluster: PhysicalityClusterConfig,
    pub role_cluster: RoleClusterConfig,
    pub maturity_layout: MaturityLayoutConfig,
    pub cross_domain: CrossDomainConfig,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            dag: DAGConfig::default(),
            type_cluster: TypeClusterConfig::default(),
            collision: CollisionConfig::default(),
            attribute_spring: AttributeSpringConfig::default(),
            ontology_relationship: OntologyRelationshipConfig::default(),
            physicality_cluster: PhysicalityClusterConfig::default(),
            role_cluster: RoleClusterConfig::default(),
            maturity_layout: MaturityLayoutConfig::default(),
            cross_domain: CrossDomainConfig::default(),
        }
    }
}

// =============================================================================
// Semantic Forces Engine
// =============================================================================

/// GPU-accelerated semantic forces engine
pub struct SemanticForcesEngine {
    config: SemanticConfig,
    node_hierarchy_levels: Vec<i32>,
    node_types: Vec<i32>,
    type_centroids: HashMap<i32, (f32, f32, f32)>,
    edge_types: Vec<i32>,
    // New ontology-based node properties
    node_physicality: Vec<i32>, // 0=None, 1=VirtualEntity, 2=PhysicalEntity, 3=ConceptualEntity
    physicality_centroids: HashMap<i32, (f32, f32, f32)>,
    node_role: Vec<i32>, // 0=None, 1=Process, 2=Agent, 3=Resource, 4=Concept
    role_centroids: HashMap<i32, (f32, f32, f32)>,
    node_maturity: Vec<i32>, // 0=None, 1=emerging, 2=mature, 3=declining
    node_cross_domain_count: Vec<i32>, // Count of cross-domain links per node
    initialized: bool,
    /// Dynamic semantic type registry for ontology-code decoupling
    registry: Arc<SemanticTypeRegistry>,
}

impl SemanticForcesEngine {
    /// Create a new semantic forces engine
    pub fn new(config: SemanticConfig) -> Self {
        Self {
            config,
            node_hierarchy_levels: Vec::new(),
            node_types: Vec::new(),
            type_centroids: HashMap::new(),
            edge_types: Vec::new(),
            node_physicality: Vec::new(),
            physicality_centroids: HashMap::new(),
            node_role: Vec::new(),
            role_centroids: HashMap::new(),
            node_maturity: Vec::new(),
            node_cross_domain_count: Vec::new(),
            initialized: false,
            registry: Arc::new(SemanticTypeRegistry::new()),
        }
    }

    /// Create a new semantic forces engine with custom registry
    pub fn with_registry(config: SemanticConfig, registry: Arc<SemanticTypeRegistry>) -> Self {
        Self {
            config,
            node_hierarchy_levels: Vec::new(),
            node_types: Vec::new(),
            type_centroids: HashMap::new(),
            edge_types: Vec::new(),
            node_physicality: Vec::new(),
            physicality_centroids: HashMap::new(),
            node_role: Vec::new(),
            role_centroids: HashMap::new(),
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

    /// Initialize engine with graph data
    pub fn initialize(&mut self, graph: &GraphData) -> Result<(), String> {
        info!("Initializing SemanticForcesEngine with {} nodes, {} edges",
              graph.nodes.len(), graph.edges.len());

        // Extract node types
        self.node_types = graph.nodes.iter()
            .map(|node| self.node_type_to_int(&node.node_type))
            .collect();

        // Extract ontology-based properties
        self.node_physicality = graph.nodes.iter()
            .map(|node| self.extract_physicality(node))
            .collect();

        self.node_role = graph.nodes.iter()
            .map(|node| self.extract_role(node))
            .collect();

        self.node_maturity = graph.nodes.iter()
            .map(|node| self.extract_maturity(node))
            .collect();

        // Count cross-domain links per node
        self.node_cross_domain_count = self.calculate_cross_domain_counts(graph);

        // Extract edge types
        self.edge_types = graph.edges.iter()
            .map(|edge| self.edge_type_to_int(&edge.edge_type))
            .collect();

        // Calculate hierarchy levels if DAG is enabled
        if self.config.dag.enabled {
            self.calculate_hierarchy_levels(graph)?;
        }

        // Calculate type centroids if type clustering is enabled
        if self.config.type_cluster.enabled {
            self.calculate_type_centroids(graph)?;
        }

        // Calculate physicality centroids if physicality clustering is enabled
        if self.config.physicality_cluster.enabled {
            self.calculate_physicality_centroids(graph)?;
        }

        // Calculate role centroids if role clustering is enabled
        if self.config.role_cluster.enabled {
            self.calculate_role_centroids(graph)?;
        }

        self.initialized = true;
        info!("SemanticForcesEngine initialized successfully");
        Ok(())
    }

    /// Update configuration
    pub fn update_config(&mut self, config: SemanticConfig) {
        self.config = config;
        debug!("Semantic forces configuration updated");
    }

    /// Get current configuration
    pub fn config(&self) -> &SemanticConfig {
        &self.config
    }

    /// Check if engine is initialized
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

    /// Get type centroids
    pub fn type_centroids(&self) -> &HashMap<i32, (f32, f32, f32)> {
        &self.type_centroids
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

    /// Get force configuration for an edge type
    fn get_edge_force_config(&self, edge_type_id: i32) -> Option<RelationshipForceConfig> {
        self.registry.get_config(edge_type_id as u32)
    }

    /// Extract physicality classification from node metadata
    fn extract_physicality(&self, node: &crate::models::node::Node) -> i32 {
        // Check owl:physicality metadata
        if let Some(physicality) = node.metadata.get("owl:physicality")
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
    fn extract_role(&self, node: &crate::models::node::Node) -> i32 {
        // Check owl:role metadata
        if let Some(role) = node.metadata.get("owl:role")
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
    fn extract_maturity(&self, node: &crate::models::node::Node) -> i32 {
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
        let node_id_to_idx: HashMap<u32, usize> = graph.nodes.iter()
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
        debug!("Calculating hierarchy levels for {} nodes", graph.nodes.len());

        // Initialize all levels to -1 (not in DAG)
        self.node_hierarchy_levels = vec![-1; graph.nodes.len()];

        // Build adjacency list for hierarchy edges
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut has_parent: HashMap<u32, bool> = HashMap::new();

        for edge in &graph.edges {
            if edge.edge_type.as_deref() == Some("hierarchy") {
                children.entry(edge.source).or_insert_with(Vec::new).push(edge.target);
                has_parent.insert(edge.target, true);
            }
        }

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
                        // Find child index
                        if let Some(child_idx) = graph.nodes.iter().position(|n| n.id == *child_id) {
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

        let nodes_in_dag = self.node_hierarchy_levels.iter().filter(|&&l| l >= 0).count();
        info!("Hierarchy levels calculated: {} nodes in DAG", nodes_in_dag);

        Ok(())
    }

    fn calculate_type_centroids(&mut self, graph: &GraphData) -> Result<(), String> {
        debug!("Calculating type centroids");

        // Group nodes by type
        let mut type_positions: HashMap<i32, Vec<(f32, f32, f32)>> = HashMap::new();

        for (i, node) in graph.nodes.iter().enumerate() {
            let node_type = self.node_types[i];
            let pos = (
                node.data.x,
                node.data.y,
                node.data.z,
            );
            type_positions.entry(node_type).or_insert_with(Vec::new).push(pos);
        }

        // Calculate centroids
        self.type_centroids.clear();
        for (node_type, positions) in type_positions {
            if !positions.is_empty() {
                let sum: (f32, f32, f32) = positions.iter()
                    .fold((0.0, 0.0, 0.0), |acc, &pos| {
                        (acc.0 + pos.0, acc.1 + pos.1, acc.2 + pos.2)
                    });
                let count = positions.len() as f32;
                let centroid = (sum.0 / count, sum.1 / count, sum.2 / count);
                self.type_centroids.insert(node_type, centroid);
            }
        }

        info!("Calculated centroids for {} node types", self.type_centroids.len());
        Ok(())
    }

    fn calculate_physicality_centroids(&mut self, graph: &GraphData) -> Result<(), String> {
        debug!("Calculating physicality centroids");

        // Group nodes by physicality
        let mut physicality_positions: HashMap<i32, Vec<(f32, f32, f32)>> = HashMap::new();

        for (i, node) in graph.nodes.iter().enumerate() {
            let physicality = self.node_physicality[i];
            if physicality > 0 {
                let pos = (node.data.x, node.data.y, node.data.z);
                physicality_positions.entry(physicality).or_insert_with(Vec::new).push(pos);
            }
        }

        // Calculate centroids
        self.physicality_centroids.clear();
        for (physicality, positions) in physicality_positions {
            if !positions.is_empty() {
                let sum: (f32, f32, f32) = positions.iter()
                    .fold((0.0, 0.0, 0.0), |acc, &pos| {
                        (acc.0 + pos.0, acc.1 + pos.1, acc.2 + pos.2)
                    });
                let count = positions.len() as f32;
                let centroid = (sum.0 / count, sum.1 / count, sum.2 / count);
                self.physicality_centroids.insert(physicality, centroid);
            }
        }

        info!("Calculated centroids for {} physicality types", self.physicality_centroids.len());
        Ok(())
    }

    fn calculate_role_centroids(&mut self, graph: &GraphData) -> Result<(), String> {
        debug!("Calculating role centroids");

        // Group nodes by role
        let mut role_positions: HashMap<i32, Vec<(f32, f32, f32)>> = HashMap::new();

        for (i, node) in graph.nodes.iter().enumerate() {
            let role = self.node_role[i];
            if role > 0 {
                let pos = (node.data.x, node.data.y, node.data.z);
                role_positions.entry(role).or_insert_with(Vec::new).push(pos);
            }
        }

        // Calculate centroids
        self.role_centroids.clear();
        for (role, positions) in role_positions {
            if !positions.is_empty() {
                let sum: (f32, f32, f32) = positions.iter()
                    .fold((0.0, 0.0, 0.0), |acc, &pos| {
                        (acc.0 + pos.0, acc.1 + pos.1, acc.2 + pos.2)
                    });
                let count = positions.len() as f32;
                let centroid = (sum.0 / count, sum.1 / count, sum.2 / count);
                self.role_centroids.insert(role, centroid);
            }
        }

        info!("Calculated centroids for {} role types", self.role_centroids.len());
        Ok(())
    }

    /// Apply semantic forces to graph (CPU fallback implementation)
    /// In production, this would call CUDA kernels
    pub fn apply_semantic_forces(
        &self,
        graph: &mut GraphData,
    ) -> Result<(), String> {
        if !self.initialized {
            return Err("Engine not initialized".to_string());
        }

        // CPU implementation as fallback
        // In production, this would delegate to CUDA kernels

        // Apply DAG forces
        if self.config.dag.enabled {
            self.apply_dag_forces_cpu(graph);
        }

        // Apply type clustering forces
        if self.config.type_cluster.enabled {
            self.apply_type_cluster_forces_cpu(graph);
        }

        // Apply collision forces
        if self.config.collision.enabled {
            self.apply_collision_forces_cpu(graph);
        }

        // Apply attribute-weighted spring forces
        if self.config.attribute_spring.enabled {
            self.apply_attribute_spring_forces_cpu(graph);
        }

        // Apply ontology relationship forces
        if self.config.ontology_relationship.enabled {
            self.apply_ontology_relationship_forces_cpu(graph);
        }

        // Apply physicality clustering forces
        if self.config.physicality_cluster.enabled {
            self.apply_physicality_cluster_forces_cpu(graph);
        }

        // Apply role clustering forces
        if self.config.role_cluster.enabled {
            self.apply_role_cluster_forces_cpu(graph);
        }

        // Apply maturity layout forces
        if self.config.maturity_layout.enabled {
            self.apply_maturity_layout_forces_cpu(graph);
        }

        // Apply cross-domain forces
        if self.config.cross_domain.enabled {
            self.apply_cross_domain_forces_cpu(graph);
        }

        Ok(())
    }

    // CPU fallback implementations (simplified)

    fn apply_dag_forces_cpu(&self, graph: &mut GraphData) {
        // Simplified CPU implementation
        // Real implementation would use CUDA kernel
        for (i, node) in graph.nodes.iter_mut().enumerate() {
            let level = self.node_hierarchy_levels[i];
            if level >= 0 {
                let target_y = level as f32 * self.config.dag.vertical_spacing;
                let dy = target_y - node.data.y;
                node.data.vy += dy * self.config.dag.level_attraction * 0.01;
            }
        }
    }

    fn apply_type_cluster_forces_cpu(&self, graph: &mut GraphData) {
        // Simplified CPU implementation
        for (i, node) in graph.nodes.iter_mut().enumerate() {
            let node_type = self.node_types[i];
            if let Some(&centroid) = self.type_centroids.get(&node_type) {
                let dx = centroid.0 - node.data.x;
                let dy = centroid.1 - node.data.y;
                let dz = centroid.2 - node.data.z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist > self.config.type_cluster.cluster_radius {
                    let force = self.config.type_cluster.cluster_attraction * 0.01;
                    node.data.vx += dx * force;
                    node.data.vy += dy * force;
                    node.data.vz += dz * force;
                }
            }
        }
    }

    fn apply_collision_forces_cpu(&self, graph: &mut GraphData) {
        // Simplified CPU implementation
        let node_count = graph.nodes.len();
        for i in 0..node_count {
            for j in (i + 1)..node_count {
                let dx = graph.nodes[i].data.x - graph.nodes[j].data.x;
                let dy = graph.nodes[i].data.y - graph.nodes[j].data.y;
                let dz = graph.nodes[i].data.z - graph.nodes[j].data.z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                let min_dist = 2.0 * self.config.collision.node_radius + self.config.collision.min_distance;
                if dist < min_dist && dist > 0.001 {
                    let force = self.config.collision.collision_strength * (min_dist - dist) / dist * 0.01;
                    graph.nodes[i].data.vx += dx * force;
                    graph.nodes[i].data.vy += dy * force;
                    graph.nodes[i].data.vz += dz * force;
                    graph.nodes[j].data.vx -= dx * force;
                    graph.nodes[j].data.vy -= dy * force;
                    graph.nodes[j].data.vz -= dz * force;
                }
            }
        }
    }

    fn apply_attribute_spring_forces_cpu(&self, graph: &mut GraphData) {
        // Simplified CPU implementation
        for edge in &graph.edges {
            // Find source and target nodes
            let src_idx = graph.nodes.iter().position(|n| n.id == edge.source);
            let tgt_idx = graph.nodes.iter().position(|n| n.id == edge.target);

            if let (Some(src_idx), Some(tgt_idx)) = (src_idx, tgt_idx) {
                let dx = graph.nodes[tgt_idx].data.x - graph.nodes[src_idx].data.x;
                let dy = graph.nodes[tgt_idx].data.y - graph.nodes[src_idx].data.y;
                let dz = graph.nodes[tgt_idx].data.z - graph.nodes[src_idx].data.z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist > 0.001 {
                    let weight = edge.weight;
                    let spring_k = self.config.attribute_spring.base_spring_k *
                                  (1.0 + weight * self.config.attribute_spring.weight_multiplier);

                    let rest_length = self.config.attribute_spring.rest_length_max -
                                    (weight * (self.config.attribute_spring.rest_length_max -
                                             self.config.attribute_spring.rest_length_min));

                    let displacement = dist - rest_length;
                    let force = spring_k * displacement / dist * 0.01;

                    // Would need mutable access to both nodes - skipping for CPU fallback
                    // Real implementation uses CUDA with atomic operations
                }
            }
        }
    }

    /// Apply ontology relationship forces using CPU fallback.
    /// Uses SemanticTypeRegistry for dynamic force configuration lookup.
    /// Gated by `config.ontology_relationship.enabled` feature toggle.
    fn apply_ontology_relationship_forces_cpu(&self, graph: &mut GraphData) {
        // Build node ID to index map
        let node_id_to_idx: HashMap<u32, usize> = graph.nodes.iter()
            .enumerate()
            .map(|(idx, node)| (node.id, idx))
            .collect();

        for (edge_idx, edge) in graph.edges.iter().enumerate() {
            let edge_type_id = self.edge_types[edge_idx];

            // Get force configuration from registry (dynamic lookup)
            let force_config = match self.get_edge_force_config(edge_type_id) {
                Some(config) => config,
                None => continue, // Skip edges with unknown types
            };

            // Skip generic type (id=0) unless it has meaningful config
            if edge_type_id == 0 && force_config.strength < 0.1 {
                continue;
            }

            let (src_idx, tgt_idx) = match (
                node_id_to_idx.get(&edge.source),
                node_id_to_idx.get(&edge.target)
            ) {
                (Some(&src), Some(&tgt)) => (src, tgt),
                _ => continue,
            };

            let dx = graph.nodes[tgt_idx].data.x - graph.nodes[src_idx].data.x;
            let dy = graph.nodes[tgt_idx].data.y - graph.nodes[src_idx].data.y;
            let dz = graph.nodes[tgt_idx].data.z - graph.nodes[src_idx].data.z;
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();

            if dist < 0.001 {
                continue;
            }

            // Use registry-based force configuration
            let strength = force_config.strength;
            let rest_length = force_config.rest_length;
            let is_directional = force_config.is_directional;

            // Hooke's law: F = -k * (x - x0)
            let displacement = dist - rest_length;
            let force_mag = strength * displacement / dist * 0.01;

            if is_directional {
                // For directional edges: target attracts source (dependency pulls toward prerequisite)
                graph.nodes[src_idx].data.vx += dx * force_mag;
                graph.nodes[src_idx].data.vy += dy * force_mag;
                graph.nodes[src_idx].data.vz += dz * force_mag;
            } else {
                // Bidirectional spring force
                graph.nodes[src_idx].data.vx += dx * force_mag;
                graph.nodes[src_idx].data.vy += dy * force_mag;
                graph.nodes[src_idx].data.vz += dz * force_mag;
                graph.nodes[tgt_idx].data.vx -= dx * force_mag;
                graph.nodes[tgt_idx].data.vy -= dy * force_mag;
                graph.nodes[tgt_idx].data.vz -= dz * force_mag;
            }
        }
    }

    fn apply_physicality_cluster_forces_cpu(&self, graph: &mut GraphData) {
        let node_count = graph.nodes.len();
        let mut forces: Vec<(f32, f32, f32)> = vec![(0.0, 0.0, 0.0); node_count];

        for i in 0..node_count {
            let physicality = self.node_physicality[i];
            if physicality == 0 {
                continue;
            }

            let node_x = graph.nodes[i].data.x;
            let node_y = graph.nodes[i].data.y;
            let node_z = graph.nodes[i].data.z;

            if let Some(&centroid) = self.physicality_centroids.get(&physicality) {
                let dx = centroid.0 - node_x;
                let dy = centroid.1 - node_y;
                let dz = centroid.2 - node_z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist > self.config.physicality_cluster.cluster_radius {
                    let force = self.config.physicality_cluster.cluster_attraction * 0.01;
                    forces[i].0 += dx * force;
                    forces[i].1 += dy * force;
                    forces[i].2 += dz * force;
                }
            }

            // Repulsion from nodes of different physicality
            for j in 0..node_count {
                if i == j {
                    continue;
                }
                let other_physicality = self.node_physicality[j];
                if other_physicality == 0 || other_physicality == physicality {
                    continue;
                }

                let dx = node_x - graph.nodes[j].data.x;
                let dy = node_y - graph.nodes[j].data.y;
                let dz = node_z - graph.nodes[j].data.z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist < self.config.physicality_cluster.cluster_radius * 2.0 && dist > 0.001 {
                    let force = self.config.physicality_cluster.inter_physicality_repulsion / (dist * dist) * 0.01;
                    forces[i].0 += dx * force / dist;
                    forces[i].1 += dy * force / dist;
                    forces[i].2 += dz * force / dist;
                }
            }
        }

        for (i, node) in graph.nodes.iter_mut().enumerate() {
            node.data.vx += forces[i].0;
            node.data.vy += forces[i].1;
            node.data.vz += forces[i].2;
        }
    }

    fn apply_role_cluster_forces_cpu(&self, graph: &mut GraphData) {
        let node_count = graph.nodes.len();
        let mut forces: Vec<(f32, f32, f32)> = vec![(0.0, 0.0, 0.0); node_count];

        for i in 0..node_count {
            let role = self.node_role[i];
            if role == 0 {
                continue;
            }

            let node_x = graph.nodes[i].data.x;
            let node_y = graph.nodes[i].data.y;
            let node_z = graph.nodes[i].data.z;

            if let Some(&centroid) = self.role_centroids.get(&role) {
                let dx = centroid.0 - node_x;
                let dy = centroid.1 - node_y;
                let dz = centroid.2 - node_z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist > self.config.role_cluster.cluster_radius {
                    let force = self.config.role_cluster.cluster_attraction * 0.01;
                    forces[i].0 += dx * force;
                    forces[i].1 += dy * force;
                    forces[i].2 += dz * force;
                }
            }

            // Repulsion from nodes of different roles
            for j in 0..node_count {
                if i == j {
                    continue;
                }
                let other_role = self.node_role[j];
                if other_role == 0 || other_role == role {
                    continue;
                }

                let dx = node_x - graph.nodes[j].data.x;
                let dy = node_y - graph.nodes[j].data.y;
                let dz = node_z - graph.nodes[j].data.z;
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist < self.config.role_cluster.cluster_radius * 2.0 && dist > 0.001 {
                    let force = self.config.role_cluster.inter_role_repulsion / (dist * dist) * 0.01;
                    forces[i].0 += dx * force / dist;
                    forces[i].1 += dy * force / dist;
                    forces[i].2 += dz * force / dist;
                }
            }
        }

        for (i, node) in graph.nodes.iter_mut().enumerate() {
            node.data.vx += forces[i].0;
            node.data.vy += forces[i].1;
            node.data.vz += forces[i].2;
        }
    }

    fn apply_maturity_layout_forces_cpu(&self, graph: &mut GraphData) {
        for (i, node) in graph.nodes.iter_mut().enumerate() {
            let maturity = self.node_maturity[i];
            if maturity == 0 {
                continue;
            }

            // Calculate target Z position based on maturity stage
            // emerging=1 → z=-stage_separation
            // mature=2   → z=0
            // declining=3 → z=+stage_separation
            let target_z = match maturity {
                1 => -self.config.maturity_layout.stage_separation,
                2 => 0.0,
                3 => self.config.maturity_layout.stage_separation,
                _ => 0.0,
            };

            let dz = target_z - node.data.z;
            node.data.vz += dz * self.config.maturity_layout.level_attraction * 0.01;
        }
    }

    fn apply_cross_domain_forces_cpu(&self, graph: &mut GraphData) {
        // Build node ID to index map
        let node_id_to_idx: HashMap<u32, usize> = graph.nodes.iter()
            .enumerate()
            .map(|(idx, node)| (node.id, idx))
            .collect();

        for (edge_idx, edge) in graph.edges.iter().enumerate() {
            let edge_type = self.edge_types[edge_idx];

            // Only process bridges-to edges
            if edge_type != 10 {
                continue;
            }

            let (src_idx, tgt_idx) = match (
                node_id_to_idx.get(&edge.source),
                node_id_to_idx.get(&edge.target)
            ) {
                (Some(&src), Some(&tgt)) => (src, tgt),
                _ => continue,
            };

            // Calculate strength based on cross-domain link count
            let src_count = self.node_cross_domain_count[src_idx] as f32;
            let tgt_count = self.node_cross_domain_count[tgt_idx] as f32;
            let avg_count = (src_count + tgt_count) / 2.0;

            let strength_boost = (1.0 + avg_count * self.config.cross_domain.link_count_multiplier)
                .min(self.config.cross_domain.max_strength_boost);
            let strength = self.config.cross_domain.base_strength * strength_boost;

            let dx = graph.nodes[tgt_idx].data.x - graph.nodes[src_idx].data.x;
            let dy = graph.nodes[tgt_idx].data.y - graph.nodes[src_idx].data.y;
            let dz = graph.nodes[tgt_idx].data.z - graph.nodes[src_idx].data.z;
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();

            if dist < 0.001 {
                continue;
            }

            // Long-range spring force
            let displacement = dist - self.config.cross_domain.rest_length;
            let force_mag = strength * displacement / dist * 0.01;

            graph.nodes[src_idx].data.vx += dx * force_mag;
            graph.nodes[src_idx].data.vy += dy * force_mag;
            graph.nodes[src_idx].data.vz += dz * force_mag;
            graph.nodes[tgt_idx].data.vx -= dx * force_mag;
            graph.nodes[tgt_idx].data.vy -= dy * force_mag;
            graph.nodes[tgt_idx].data.vz -= dz * force_mag;
        }
    }
}

impl Default for SemanticForcesEngine {
    fn default() -> Self {
        Self::new(SemanticConfig::default())
    }
}

// =============================================================================
// Dynamic GPU Buffer Management for Schema-Code Decoupling
// =============================================================================

/// GPU-compatible dynamic force configuration
/// Matches DynamicForceConfig struct in semantic_forces.cu
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DynamicForceConfigGPU {
    pub strength: f32,
    pub rest_length: f32,
    pub is_directional: i32,
    pub force_type: u32,
}

impl Default for DynamicForceConfigGPU {
    fn default() -> Self {
        Self {
            strength: 0.5,
            rest_length: 100.0,
            is_directional: 0,
            force_type: 0,
        }
    }
}

impl From<&RelationshipForceConfig> for DynamicForceConfigGPU {
    fn from(config: &RelationshipForceConfig) -> Self {
        Self {
            strength: config.strength,
            rest_length: config.rest_length,
            is_directional: if config.is_directional { 1 } else { 0 },
            force_type: config.force_type,
        }
    }
}

// FFI declarations for dynamic relationship buffer management
extern "C" {
    fn set_dynamic_relationship_buffer(
        configs: *const DynamicForceConfigGPU,
        num_types: i32,
        enabled: bool,
    ) -> i32;

    fn update_dynamic_relationship_config(
        type_id: i32,
        config: *const DynamicForceConfigGPU,
    ) -> i32;

    fn set_dynamic_relationships_enabled(enabled: bool) -> i32;

    fn get_dynamic_relationship_buffer_version() -> i32;

    fn get_max_relationship_types() -> i32;
}

/// Manager for dynamic relationship buffer on GPU
/// Enables hot-reload of ontology force configurations without CUDA recompilation
pub struct DynamicRelationshipBufferManager {
    /// Current buffer version (for change detection)
    current_version: i32,
    /// Whether dynamic mode is enabled
    enabled: bool,
    /// Last uploaded configuration count
    last_type_count: usize,
}

impl DynamicRelationshipBufferManager {
    /// Create a new buffer manager
    pub fn new() -> Self {
        Self {
            current_version: 0,
            enabled: false,
            last_type_count: 0,
        }
    }

    /// Upload relationship configurations from registry to GPU
    /// Call this whenever ontology changes to enable new relationship types
    pub fn upload_from_registry(&mut self, registry: &SemanticTypeRegistry) -> Result<(), String> {
        let buffer = registry.build_gpu_buffer();
        let gpu_buffer: Vec<DynamicForceConfigGPU> = buffer
            .iter()
            .map(|c| DynamicForceConfigGPU::from(c))
            .collect();

        self.upload_buffer(&gpu_buffer)
    }

    /// Upload a raw buffer of configurations to GPU
    pub fn upload_buffer(&mut self, configs: &[DynamicForceConfigGPU]) -> Result<(), String> {
        let max_types = unsafe { get_max_relationship_types() } as usize;

        if configs.len() > max_types {
            return Err(format!(
                "Too many relationship types: {} (max: {})",
                configs.len(),
                max_types
            ));
        }

        let result = unsafe {
            set_dynamic_relationship_buffer(
                if configs.is_empty() { std::ptr::null() } else { configs.as_ptr() },
                configs.len() as i32,
                true,
            )
        };

        if result != 0 {
            return Err(format!("CUDA error uploading relationship buffer: {}", result));
        }

        self.current_version = unsafe { get_dynamic_relationship_buffer_version() };
        self.enabled = true;
        self.last_type_count = configs.len();

        info!(
            "Uploaded {} relationship types to GPU (version {})",
            configs.len(),
            self.current_version
        );

        Ok(())
    }

    /// Hot-reload a single relationship type configuration
    /// More efficient than full buffer upload for single changes
    pub fn update_single_type(
        &mut self,
        type_id: u32,
        config: &DynamicForceConfigGPU,
    ) -> Result<(), String> {
        let max_types = unsafe { get_max_relationship_types() } as usize;

        if type_id as usize >= max_types {
            return Err(format!(
                "Type ID {} exceeds maximum ({})",
                type_id, max_types
            ));
        }

        let result = unsafe { update_dynamic_relationship_config(type_id as i32, config) };

        if result != 0 {
            return Err(format!("CUDA error updating relationship config: {}", result));
        }

        self.current_version = unsafe { get_dynamic_relationship_buffer_version() };

        debug!(
            "Hot-reloaded relationship type {} (version {})",
            type_id, self.current_version
        );

        Ok(())
    }

    /// Enable or disable dynamic relationship forces on GPU
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        let result = unsafe { set_dynamic_relationships_enabled(enabled) };

        if result != 0 {
            return Err(format!("CUDA error setting dynamic relationships enabled: {}", result));
        }

        self.enabled = enabled;
        info!("Dynamic relationship forces {}", if enabled { "enabled" } else { "disabled" });

        Ok(())
    }

    /// Check if dynamic mode is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get current buffer version
    pub fn version(&self) -> i32 {
        self.current_version
    }

    /// Get last uploaded type count
    pub fn type_count(&self) -> usize {
        self.last_type_count
    }

    /// Get maximum supported relationship types
    pub fn max_types(&self) -> usize {
        unsafe { get_max_relationship_types() as usize }
    }
}

impl Default for DynamicRelationshipBufferManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::node::Node;
    use crate::models::edge::Edge;

    #[test]
    fn test_semantic_config_defaults() {
        let config = SemanticConfig::default();
        assert!(config.dag.enabled);
        assert!(config.type_cluster.enabled);
        assert!(config.collision.enabled);
        assert!(config.attribute_spring.enabled);
    }

    #[test]
    fn test_engine_creation() {
        let config = SemanticConfig::default();
        let engine = SemanticForcesEngine::new(config);
        assert!(!engine.is_initialized());
    }

    #[test]
    fn test_engine_initialization() {
        let mut engine = SemanticForcesEngine::new(SemanticConfig::default());

        let mut graph = GraphData::new();
        let mut node1 = Node::new("node1".to_string());
        node1.node_type = Some("person".to_string());
        graph.nodes.push(node1);

        let result = engine.initialize(&graph);
        assert!(result.is_ok());
        assert!(engine.is_initialized());
        assert_eq!(engine.node_types().len(), 1);
    }

    #[test]
    fn test_hierarchy_calculation() {
        let mut config = SemanticConfig::default();
        config.dag.enabled = true;

        let mut engine = SemanticForcesEngine::new(config);

        let mut graph = GraphData::new();
        let mut parent = Node::new("parent".to_string());
        parent = parent.with_label("Parent".to_string());
        let parent_id = parent.id;
        graph.nodes.push(parent);

        let mut child = Node::new("child".to_string());
        child = child.with_label("Child".to_string());
        let child_id = child.id;
        graph.nodes.push(child);

        let mut edge = Edge::new(parent_id, child_id, 1.0);
        edge.edge_type = Some("hierarchy".to_string());
        graph.edges.push(edge);

        engine.initialize(&graph).unwrap();

        let levels = engine.hierarchy_levels();
        assert_eq!(levels.len(), 2);
        // Parent should be at level 0
        assert_eq!(levels[0], 0);
        // Child should be at level 1
        assert_eq!(levels[1], 1);
    }

    #[test]
    fn test_type_clustering() {
        let mut config = SemanticConfig::default();
        config.type_cluster.enabled = true;

        let mut engine = SemanticForcesEngine::new(config);

        let mut graph = GraphData::new();
        for i in 0..5 {
            let mut node = Node::new(format!("node{}", i));
            node.node_type = Some("person".to_string());
            graph.nodes.push(node);
        }

        engine.initialize(&graph).unwrap();

        let centroids = engine.type_centroids();
        assert_eq!(centroids.len(), 1); // Only one type
        assert!(centroids.contains_key(&1)); // person type
    }

    // ==========================================================================
    // Dynamic GPU Buffer Tests
    // ==========================================================================

    #[test]
    fn test_dynamic_force_config_default() {
        let config = DynamicForceConfigGPU::default();
        assert_eq!(config.strength, 0.5);
        assert_eq!(config.rest_length, 100.0);
        assert_eq!(config.is_directional, 0);
        assert_eq!(config.force_type, 0);
    }

    #[test]
    fn test_dynamic_force_config_from_relationship_config() {
        let relationship_config = RelationshipForceConfig {
            strength: 0.8,
            rest_length: 75.0,
            is_directional: true,
            force_type: 2,
        };

        let gpu_config = DynamicForceConfigGPU::from(&relationship_config);

        assert_eq!(gpu_config.strength, 0.8);
        assert_eq!(gpu_config.rest_length, 75.0);
        assert_eq!(gpu_config.is_directional, 1);
        assert_eq!(gpu_config.force_type, 2);
    }

    #[test]
    fn test_dynamic_buffer_manager_creation() {
        let manager = DynamicRelationshipBufferManager::new();
        assert!(!manager.is_enabled());
        assert_eq!(manager.version(), 0);
        assert_eq!(manager.type_count(), 0);
    }

    #[test]
    fn test_dynamic_force_config_struct_size() {
        // Verify the struct layout matches CUDA expectations
        assert_eq!(std::mem::size_of::<DynamicForceConfigGPU>(), 16);
        assert_eq!(std::mem::align_of::<DynamicForceConfigGPU>(), 4);
    }
}
