//! Semantic Forces Actor - Handles DAG layout, type clustering, and collision detection
//! Integrates with GPU kernels in semantic_forces.cu for advanced graph layout

use actix::prelude::*;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::shared::SharedGPUContext;

// Re-export message types for handlers
pub use crate::actors::messages::{
    ConfigureCollision, ConfigureDAG, ConfigureTypeClustering, GetHierarchyLevels,
    GetSemanticConfig, RecalculateHierarchy, ReloadRelationshipBuffer, SetSharedGPUContext,
};

// Import the kernel bridge for safe access to gated FFI functions
use crate::gpu::kernel_bridge;

/// DAG layout configuration matching GPU kernel structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAGConfig {
    pub vertical_spacing: f32,   // Vertical separation between hierarchy levels
    pub horizontal_spacing: f32, // Minimum horizontal separation within a level
    pub level_attraction: f32,   // Strength of attraction to target level
    pub sibling_repulsion: f32,  // Repulsion between nodes at same level
    pub enabled: bool,
    pub layout_mode: DAGLayoutMode,
}

/// DAG layout modes for different visual hierarchies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DAGLayoutMode {
    TopDown,   // Traditional top-down hierarchy
    Radial,    // Radial/circular hierarchy
    LeftRight, // Left-to-right hierarchy
}

impl Default for DAGConfig {
    fn default() -> Self {
        Self {
            vertical_spacing: 100.0,
            horizontal_spacing: 50.0,
            level_attraction: 0.5,
            sibling_repulsion: 0.3,
            enabled: true,
            layout_mode: DAGLayoutMode::TopDown,
        }
    }
}

/// Type clustering configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeClusterConfig {
    pub cluster_attraction: f32,      // Attraction between nodes of same type
    pub cluster_radius: f32,          // Target radius for type clusters
    pub inter_cluster_repulsion: f32, // Repulsion between different type clusters
    pub enabled: bool,
}

impl Default for TypeClusterConfig {
    fn default() -> Self {
        Self {
            cluster_attraction: 0.4,
            cluster_radius: 80.0,
            inter_cluster_repulsion: 0.2,
            enabled: true,
        }
    }
}

/// Collision detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionConfig {
    pub min_distance: f32,       // Minimum allowed distance between nodes
    pub collision_strength: f32, // Force strength when colliding
    pub node_radius: f32,        // Default node radius
    pub enabled: bool,
}

impl Default for CollisionConfig {
    fn default() -> Self {
        Self {
            min_distance: 10.0,
            collision_strength: 0.8,
            node_radius: 15.0,
            enabled: true,
        }
    }
}

/// Attribute-weighted spring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeSpringConfig {
    pub base_spring_k: f32,     // Base spring constant
    pub weight_multiplier: f32, // Multiplier for edge weight influence
    pub rest_length_min: f32,   // Minimum rest length
    pub rest_length_max: f32,   // Maximum rest length
    pub enabled: bool,
}

impl Default for AttributeSpringConfig {
    fn default() -> Self {
        Self {
            base_spring_k: 0.1,
            weight_multiplier: 1.5,
            rest_length_min: 50.0,
            rest_length_max: 200.0,
            enabled: true,
        }
    }
}

/// Combined semantic configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SemanticConfig {
    pub dag: DAGConfig,
    pub type_cluster: TypeClusterConfig,
    pub collision: CollisionConfig,
    pub attribute_spring: AttributeSpringConfig,
}

/// Node hierarchy level assignment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyLevels {
    pub node_levels: Vec<i32>, // Hierarchy level for each node (-1 = not in DAG)
    pub max_level: i32,        // Maximum hierarchy level
    pub level_counts: Vec<usize>, // Number of nodes at each level
}

/// Semantic Forces Actor - manages semantic layout forces
pub struct SemanticForcesActor {
    /// Shared GPU context for accessing GPU resources
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Current semantic configuration
    config: SemanticConfig,

    /// Cached hierarchy levels (computed on demand)
    hierarchy_levels: Option<HierarchyLevels>,

    /// Cached node types array for GPU access
    node_types: Vec<i32>,

    /// Cached edge data for attribute springs
    edge_sources: Vec<i32>,
    edge_targets: Vec<i32>,
    edge_types: Vec<i32>,
}

impl SemanticForcesActor {
    pub fn new() -> Self {
        Self {
            shared_context: None,
            config: SemanticConfig::default(),
            hierarchy_levels: None,
            node_types: Vec::new(),
            edge_sources: Vec::new(),
            edge_targets: Vec::new(),
            edge_types: Vec::new(),
        }
    }

    /// Calculate hierarchy levels using topological sort (BFS-style on GPU)
    fn calculate_hierarchy_levels(
        &mut self,
        num_nodes: usize,
        num_edges: usize,
    ) -> Result<HierarchyLevels, String> {
        info!(
            "SemanticForcesActor: Calculating hierarchy levels for {} nodes, {} edges",
            num_nodes, num_edges
        );

        let _shared_context = self
            .shared_context
            .as_ref()
            .ok_or("GPU context not initialized")?;

        // Initialize node levels to -1 (not in hierarchy)
        let mut node_levels = vec![-1i32; num_nodes];

        // Find root nodes (nodes with no incoming hierarchy edges)
        let mut has_incoming_hierarchy = vec![false; num_nodes];
        for i in 0..self.edge_sources.len() {
            if self.edge_types[i] == 2 {
                // Hierarchy edge type = 2
                let target = self.edge_targets[i] as usize;
                if target < num_nodes {
                    has_incoming_hierarchy[target] = true;
                }
            }
        }

        // Set root nodes to level 0
        for (i, &has_incoming) in has_incoming_hierarchy.iter().enumerate() {
            if !has_incoming {
                node_levels[i] = 0;
            }
        }
        {
            // Hierarchy computation via kernel bridge (GPU when available, CPU fallback)
            if num_edges > 0 && !self.edge_sources.is_empty() {
                let mut changed = true;
                let mut iteration = 0;
                const MAX_ITERATIONS: usize = 100;

                while changed && iteration < MAX_ITERATIONS {
                    changed = false;
                    kernel_bridge::calculate_hierarchy_levels(
                        &self.edge_sources,
                        &self.edge_targets,
                        &self.edge_types,
                        &mut node_levels,
                        &mut changed,
                        num_edges,
                        num_nodes,
                    );
                    iteration += 1;
                }

                if iteration >= MAX_ITERATIONS {
                    warn!("SemanticForcesActor: Hierarchy calculation reached max iterations");
                }
            }
        }

        // Calculate max_level and level_counts before moving node_levels
        let max_level = node_levels.iter().copied().max().unwrap_or(0);
        let mut level_counts = vec![0; (max_level + 1) as usize];
        for &level in &node_levels {
            if level >= 0 {
                level_counts[level as usize] += 1;
            }
        }

        // Return computed hierarchy levels
        Ok(HierarchyLevels {
            node_levels,
            max_level,
            level_counts,
        })
    }
}

impl Default for SemanticForcesActor {
    fn default() -> Self {
        Self::new()
    }
}

// Actor implementation
impl Actor for SemanticForcesActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("SemanticForcesActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("SemanticForcesActor stopped");
    }
}

// =============================================================================
// Message Handlers
// =============================================================================

impl Handler<ConfigureDAG> for SemanticForcesActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ConfigureDAG, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(v) = msg.vertical_spacing {
            self.config.dag.vertical_spacing = v;
        }
        if let Some(h) = msg.horizontal_spacing {
            self.config.dag.horizontal_spacing = h;
        }
        if let Some(a) = msg.level_attraction {
            self.config.dag.level_attraction = a;
        }
        if let Some(r) = msg.sibling_repulsion {
            self.config.dag.sibling_repulsion = r;
        }
        if let Some(e) = msg.enabled {
            self.config.dag.enabled = e;
        }
        info!(
            "SemanticForcesActor: DAG config updated, enabled={}",
            self.config.dag.enabled
        );
        Ok(())
    }
}

impl Handler<ConfigureTypeClustering> for SemanticForcesActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ConfigureTypeClustering, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(a) = msg.cluster_attraction {
            self.config.type_cluster.cluster_attraction = a;
        }
        if let Some(r) = msg.cluster_radius {
            self.config.type_cluster.cluster_radius = r;
        }
        if let Some(i) = msg.inter_cluster_repulsion {
            self.config.type_cluster.inter_cluster_repulsion = i;
        }
        if let Some(e) = msg.enabled {
            self.config.type_cluster.enabled = e;
        }
        info!(
            "SemanticForcesActor: Type clustering config updated, enabled={}",
            self.config.type_cluster.enabled
        );
        Ok(())
    }
}

impl Handler<ConfigureCollision> for SemanticForcesActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ConfigureCollision, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(d) = msg.min_distance {
            self.config.collision.min_distance = d;
        }
        if let Some(s) = msg.collision_strength {
            self.config.collision.collision_strength = s;
        }
        if let Some(r) = msg.node_radius {
            self.config.collision.node_radius = r;
        }
        if let Some(e) = msg.enabled {
            self.config.collision.enabled = e;
        }
        info!(
            "SemanticForcesActor: Collision config updated, enabled={}",
            self.config.collision.enabled
        );
        Ok(())
    }
}

impl Handler<GetSemanticConfig> for SemanticForcesActor {
    type Result = Result<SemanticConfig, String>;

    fn handle(&mut self, _msg: GetSemanticConfig, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.config.clone())
    }
}

impl Handler<GetHierarchyLevels> for SemanticForcesActor {
    type Result = Result<HierarchyLevels, String>;

    fn handle(&mut self, _msg: GetHierarchyLevels, _ctx: &mut Self::Context) -> Self::Result {
        self.hierarchy_levels
            .clone()
            .ok_or_else(|| "Hierarchy levels not yet calculated".to_string())
    }
}

impl Handler<RecalculateHierarchy> for SemanticForcesActor {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: RecalculateHierarchy, _ctx: &mut Self::Context) -> Self::Result {
        let num_nodes = self.node_types.len();
        let num_edges = self.edge_sources.len();
        if num_nodes == 0 {
            return Err("No nodes loaded".to_string());
        }
        let levels = self.calculate_hierarchy_levels(num_nodes, num_edges)?;
        self.hierarchy_levels = Some(levels);
        Ok(())
    }
}

impl Handler<SetSharedGPUContext> for SemanticForcesActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        info!("SemanticForcesActor: Received SharedGPUContext");
        self.shared_context = Some(msg.context);
        Ok(())
    }
}

impl Handler<ReloadRelationshipBuffer> for SemanticForcesActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ReloadRelationshipBuffer, _ctx: &mut Self::Context) -> Self::Result {
        let num_types = msg.buffer.len();
        info!(
            "SemanticForcesActor: Reloading dynamic relationship buffer ({} types, version {})",
            num_types, msg.version
        );

        if msg.buffer.is_empty() {
            warn!("SemanticForcesActor: Empty buffer, disabling dynamic relationships");
            kernel_bridge::set_dynamic_relationships_enabled(false);
            return Ok(());
        }

        let result = kernel_bridge::set_dynamic_relationship_buffer(&msg.buffer, true);

        if result == 0 {
            info!(
                "SemanticForcesActor: Dynamic relationship buffer uploaded ({} types, version {}, gpu={})",
                num_types, msg.version, kernel_bridge::gpu_available()
            );
            Ok(())
        } else {
            let err = format!(
                "GPU FFI set_dynamic_relationship_buffer returned error code {}",
                result
            );
            error!("SemanticForcesActor: {}", err);
            Err(err)
        }
    }
}
