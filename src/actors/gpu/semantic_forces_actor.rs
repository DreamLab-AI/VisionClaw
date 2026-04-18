//! Semantic Forces Actor - Handles DAG layout, type clustering, and collision detection
//! Integrates with GPU kernels in semantic_forces.cu for advanced graph layout

#![allow(dead_code)]
use actix::prelude::*;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::shared::{GPUState, SharedGPUContext};

// Re-export message types for handlers
pub use crate::actors::messages::{
    ConfigureCollision, ConfigureDAG, ConfigureMaturity, ConfigurePhysicality, ConfigureRole,
    ConfigureTypeClustering, GetHierarchyLevels, GetSemanticConfig, RecalculateHierarchy,
    ReloadRelationshipBuffer, SetSharedGPUContext,
};

// =============================================================================
// GPU Kernel FFI Declarations
// =============================================================================
//
// IMPORTANT: These structs MUST match the C++ definitions in semantic_forces.cu exactly.
// Any mismatch in size or alignment will cause memory corruption during FFI calls.
//
// C++ bool is typically 1 byte, but for alignment we use padding in #[repr(C)] structs.
// Each config struct has 3-4 floats followed by a bool, so we add explicit padding
// to ensure consistent memory layout across Rust and C++.

#[repr(C)]
#[derive(Clone, Copy)]
struct DAGConfigGPU {
    vertical_spacing: f32,    // 4 bytes, offset 0
    horizontal_spacing: f32,  // 4 bytes, offset 4
    level_attraction: f32,    // 4 bytes, offset 8
    sibling_repulsion: f32,   // 4 bytes, offset 12
    enabled: bool,            // 1 byte,  offset 16
    _pad: [u8; 3],            // 3 bytes padding to align to 4 bytes
}
// Expected size: 20 bytes (5 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct TypeClusterConfigGPU {
    cluster_attraction: f32,      // 4 bytes, offset 0
    cluster_radius: f32,          // 4 bytes, offset 4
    inter_cluster_repulsion: f32, // 4 bytes, offset 8
    enabled: bool,                // 1 byte,  offset 12
    _pad: [u8; 3],                // 3 bytes padding to align to 4 bytes
}
// Expected size: 16 bytes (4 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct CollisionConfigGPU {
    min_distance: f32,       // 4 bytes, offset 0
    collision_strength: f32, // 4 bytes, offset 4
    node_radius: f32,        // 4 bytes, offset 8
    enabled: bool,           // 1 byte,  offset 12
    _pad: [u8; 3],           // 3 bytes padding to align to 4 bytes
}
// Expected size: 16 bytes (4 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct AttributeSpringConfigGPU {
    base_spring_k: f32,      // 4 bytes, offset 0
    weight_multiplier: f32,  // 4 bytes, offset 4
    rest_length_min: f32,    // 4 bytes, offset 8
    rest_length_max: f32,    // 4 bytes, offset 12
    enabled: bool,           // 1 byte,  offset 16
    _pad: [u8; 3],           // 3 bytes padding to align to 4 bytes
}
// Expected size: 20 bytes (5 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct OntologyRelationshipConfigGPU {
    requires_strength: f32,      // 4 bytes, offset 0 (legacy, unused by GPU)
    requires_rest_length: f32,   // 4 bytes, offset 4 (legacy)
    enables_strength: f32,       // 4 bytes, offset 8 (legacy)
    enables_rest_length: f32,    // 4 bytes, offset 12 (legacy)
    has_part_strength: f32,      // 4 bytes, offset 16 (legacy)
    has_part_orbit_radius: f32,  // 4 bytes, offset 20 (legacy)
    bridges_to_strength: f32,    // 4 bytes, offset 24 (legacy)
    bridges_to_rest_length: f32, // 4 bytes, offset 28 (legacy)
    enabled: bool,               // 1 byte,  offset 32
    _pad: [u8; 3],               // 3 bytes padding to align to 4 bytes
}
// Expected size: 36 bytes (9 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct PhysicalityClusterConfigGPU {
    cluster_attraction: f32,           // 4 bytes, offset 0
    cluster_radius: f32,               // 4 bytes, offset 4
    inter_physicality_repulsion: f32,  // 4 bytes, offset 8
    enabled: bool,                     // 1 byte,  offset 12
    _pad: [u8; 3],                     // 3 bytes padding to align to 4 bytes
}
// Expected size: 16 bytes (4 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct RoleClusterConfigGPU {
    cluster_attraction: f32,     // 4 bytes, offset 0
    cluster_radius: f32,         // 4 bytes, offset 4
    inter_role_repulsion: f32,   // 4 bytes, offset 8
    enabled: bool,               // 1 byte,  offset 12
    _pad: [u8; 3],               // 3 bytes padding to align to 4 bytes
}
// Expected size: 16 bytes (4 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct MaturityLayoutConfigGPU {
    vertical_spacing: f32,   // 4 bytes, offset 0
    level_attraction: f32,   // 4 bytes, offset 4
    stage_separation: f32,   // 4 bytes, offset 8
    enabled: bool,           // 1 byte,  offset 12
    _pad: [u8; 3],           // 3 bytes padding to align to 4 bytes
}
// Expected size: 16 bytes (4 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct CrossDomainConfigGPU {
    base_strength: f32,          // 4 bytes, offset 0
    link_count_multiplier: f32,  // 4 bytes, offset 4
    max_strength_boost: f32,     // 4 bytes, offset 8
    rest_length: f32,            // 4 bytes, offset 12
    enabled: bool,               // 1 byte,  offset 16
    _pad: [u8; 3],               // 3 bytes padding to align to 4 bytes
}
// Expected size: 20 bytes (5 * 4-byte aligned fields)

#[repr(C)]
#[derive(Clone, Copy)]
struct SemanticConfigGPU {
    dag: DAGConfigGPU,                                // 20 bytes, offset 0
    type_cluster: TypeClusterConfigGPU,               // 16 bytes, offset 20
    collision: CollisionConfigGPU,                    // 16 bytes, offset 36
    attribute_spring: AttributeSpringConfigGPU,       // 20 bytes, offset 52
    ontology_relationship: OntologyRelationshipConfigGPU, // 36 bytes, offset 72
    physicality_cluster: PhysicalityClusterConfigGPU, // 16 bytes, offset 108
    role_cluster: RoleClusterConfigGPU,               // 16 bytes, offset 124
    maturity_layout: MaturityLayoutConfigGPU,         // 16 bytes, offset 140
    cross_domain: CrossDomainConfigGPU,               // 20 bytes, offset 156
}
// Expected size: 176 bytes

// =============================================================================
// Static Assertions for FFI Struct Sizes
// =============================================================================
//
// These compile-time checks ensure Rust struct sizes match C++ definitions.
// If any assertion fails, the struct layouts are incompatible and must be fixed.

use static_assertions::const_assert_eq;

// Individual config struct sizes
const_assert_eq!(std::mem::size_of::<DAGConfigGPU>(), 20);
const_assert_eq!(std::mem::size_of::<TypeClusterConfigGPU>(), 16);
const_assert_eq!(std::mem::size_of::<CollisionConfigGPU>(), 16);
const_assert_eq!(std::mem::size_of::<AttributeSpringConfigGPU>(), 20);
const_assert_eq!(std::mem::size_of::<OntologyRelationshipConfigGPU>(), 36);
const_assert_eq!(std::mem::size_of::<PhysicalityClusterConfigGPU>(), 16);
const_assert_eq!(std::mem::size_of::<RoleClusterConfigGPU>(), 16);
const_assert_eq!(std::mem::size_of::<MaturityLayoutConfigGPU>(), 16);
const_assert_eq!(std::mem::size_of::<CrossDomainConfigGPU>(), 20);

// Combined config struct size (must match C++ SemanticConfig)
const_assert_eq!(std::mem::size_of::<SemanticConfigGPU>(), 176);

// Float3 struct (matches CUDA float3)
const_assert_eq!(std::mem::size_of::<Float3>(), 12);
const_assert_eq!(std::mem::align_of::<Float3>(), 4);

// DynamicForceConfigGPU (matches C++ DynamicForceConfig)
const_assert_eq!(std::mem::size_of::<DynamicForceConfigGPU>(), 16);
const_assert_eq!(std::mem::align_of::<DynamicForceConfigGPU>(), 4);

// =============================================================================
// Dynamic Force Configuration (Schema-Code Decoupling)
// =============================================================================

/// GPU-compatible dynamic force configuration for a relationship type
/// Matches the DynamicForceConfig struct in semantic_forces.cu
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DynamicForceConfigGPU {
    pub strength: f32,        // Spring strength (can be negative for repulsion)
    pub rest_length: f32,     // Rest length for spring calculations
    pub is_directional: i32,  // 1 = directional, 0 = bidirectional
    pub force_type: u32,      // Force behavior type (0=spring, 1=orbit, 2=cross-domain, 3=repulsion)
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

#[repr(C)]
#[derive(Clone, Copy)]
struct Float3 {
    x: f32,
    y: f32,
    z: f32,
}
// =============================================================================
// GPU Kernel FFI Declarations (gated behind "gpu" feature)
// =============================================================================
//
// When compiled WITHOUT the "gpu" feature, these symbols are not linked and all
// call sites use the safe wrappers in `crate::gpu::kernel_bridge` which provide
// CPU fallback implementations.

#[cfg(feature = "gpu")]
extern "C" {
    /// Upload semantic configuration to GPU constant memory
    fn set_semantic_config(config: *const SemanticConfigGPU);

    /// Apply DAG layout forces based on hierarchy levels
    fn apply_dag_force(
        node_hierarchy_levels: *const i32,
        node_types: *const i32,
        positions: *mut Float3,
        forces: *mut Float3,
        num_nodes: i32,
    );

    /// Apply type clustering forces
    fn apply_type_cluster_force(
        node_types: *const i32,
        type_centroids: *const Float3,
        positions: *mut Float3,
        forces: *mut Float3,
        num_nodes: i32,
        num_types: i32,
    );

    /// Apply collision detection and response forces
    fn apply_collision_force(
        node_radii: *const f32,
        positions: *mut Float3,
        forces: *mut Float3,
        num_nodes: i32,
    );

    /// Apply attribute-weighted spring forces
    fn apply_attribute_spring_force(
        edge_sources: *const i32,
        edge_targets: *const i32,
        edge_weights: *const f32,
        edge_types: *const i32,
        positions: *mut Float3,
        forces: *mut Float3,
        num_edges: i32,
    );

    /// Apply dynamic relationship forces (schema-code decoupled)
    fn apply_dynamic_relationship_force(
        edge_sources: *const i32,
        edge_targets: *const i32,
        edge_types: *const i32,
        node_cross_domain_count: *const i32,
        positions: *mut Float3,
        forces: *mut Float3,
        num_edges: i32,
    );

    /// Apply physicality-based clustering forces
    fn apply_physicality_cluster_force(
        node_physicality: *const i32,
        physicality_centroids: *const Float3,
        positions: *mut Float3,
        forces: *mut Float3,
        num_nodes: i32,
    );

    /// Apply role-based clustering forces
    fn apply_role_cluster_force(
        node_role: *const i32,
        role_centroids: *const Float3,
        positions: *mut Float3,
        forces: *mut Float3,
        num_nodes: i32,
    );

    /// Apply maturity-based layout forces
    fn apply_maturity_layout_force(
        node_maturity: *const i32,
        positions: *mut Float3,
        forces: *mut Float3,
        num_nodes: i32,
    );

    /// Calculate physicality centroids
    fn calculate_physicality_centroids(
        node_physicality: *const i32,
        positions: *const Float3,
        physicality_centroids: *mut Float3,
        physicality_counts: *mut i32,
        num_nodes: i32,
    );

    /// Finalize physicality centroids (divide by count)
    fn finalize_physicality_centroids(
        physicality_centroids: *mut Float3,
        physicality_counts: *const i32,
    );

    /// Calculate role centroids
    fn calculate_role_centroids(
        node_role: *const i32,
        positions: *const Float3,
        role_centroids: *mut Float3,
        role_counts: *mut i32,
        num_nodes: i32,
    );

    /// Finalize role centroids (divide by count)
    fn finalize_role_centroids(
        role_centroids: *mut Float3,
        role_counts: *const i32,
    );
}

// Import the kernel bridge for safe access to gated FFI functions
use crate::gpu::kernel_bridge;

/// DAG layout configuration matching GPU kernel structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAGConfig {
    pub vertical_spacing: f32,      // Vertical separation between hierarchy levels
    pub horizontal_spacing: f32,    // Minimum horizontal separation within a level
    pub level_attraction: f32,      // Strength of attraction to target level
    pub sibling_repulsion: f32,     // Repulsion between nodes at same level
    pub enabled: bool,
    pub layout_mode: DAGLayoutMode,
}

/// DAG layout modes for different visual hierarchies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DAGLayoutMode {
    TopDown,      // Traditional top-down hierarchy
    Radial,       // Radial/circular hierarchy
    LeftRight,    // Left-to-right hierarchy
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
    pub cluster_attraction: f32,    // Attraction between nodes of same type
    pub cluster_radius: f32,        // Target radius for type clusters
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
    pub min_distance: f32,          // Minimum allowed distance between nodes
    pub collision_strength: f32,    // Force strength when colliding
    pub node_radius: f32,           // Default node radius
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
    pub base_spring_k: f32,         // Base spring constant
    pub weight_multiplier: f32,     // Multiplier for edge weight influence
    pub rest_length_min: f32,       // Minimum rest length
    pub rest_length_max: f32,       // Maximum rest length
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

/// Physicality-based cluster force configuration.
///
/// Controls attraction of nodes with the same `PhysicalityCode` and repulsion between
/// nodes in different physicality buckets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalityClusterConfig {
    pub cluster_attraction: f32,
    pub cluster_radius: f32,
    pub inter_physicality_repulsion: f32,
    pub enabled: bool,
}

impl Default for PhysicalityClusterConfig {
    fn default() -> Self {
        Self {
            cluster_attraction: 0.40,
            cluster_radius: 80.0,
            inter_physicality_repulsion: 0.20,
            enabled: false,
        }
    }
}

/// Role-based cluster force configuration.
///
/// Controls attraction of nodes with the same `RoleCode` and repulsion between nodes
/// in different role buckets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleClusterConfig {
    pub cluster_attraction: f32,
    pub cluster_radius: f32,
    pub inter_role_repulsion: f32,
    pub enabled: bool,
}

impl Default for RoleClusterConfig {
    fn default() -> Self {
        Self {
            cluster_attraction: 0.40,
            cluster_radius: 80.0,
            inter_role_repulsion: 0.20,
            enabled: false,
        }
    }
}

/// Maturity-based layout force configuration.
///
/// Drives vertical stratification of nodes by `MaturityLevel`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaturityLayoutConfig {
    pub vertical_spacing: f32,
    pub level_attraction: f32,
    pub enabled: bool,
}

impl Default for MaturityLayoutConfig {
    fn default() -> Self {
        Self {
            vertical_spacing: 100.0,
            level_attraction: 0.50,
            enabled: false,
        }
    }
}

/// Combined semantic configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConfig {
    pub dag: DAGConfig,
    pub type_cluster: TypeClusterConfig,
    pub collision: CollisionConfig,
    pub attribute_spring: AttributeSpringConfig,
    pub physicality: PhysicalityClusterConfig,
    pub role: RoleClusterConfig,
    pub maturity: MaturityLayoutConfig,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            dag: DAGConfig::default(),
            type_cluster: TypeClusterConfig::default(),
            collision: CollisionConfig::default(),
            attribute_spring: AttributeSpringConfig::default(),
            physicality: PhysicalityClusterConfig::default(),
            role: RoleClusterConfig::default(),
            maturity: MaturityLayoutConfig::default(),
        }
    }
}

/// Node hierarchy level assignment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyLevels {
    pub node_levels: Vec<i32>,      // Hierarchy level for each node (-1 = not in DAG)
    pub max_level: i32,             // Maximum hierarchy level
    pub level_counts: Vec<usize>,   // Number of nodes at each level
}

/// Type centroid positions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeCentroids {
    pub centroids: Vec<(f32, f32, f32)>,  // Centroid position for each type
    pub type_counts: Vec<usize>,          // Number of nodes of each type
}

/// Semantic Forces Actor - manages semantic layout forces
pub struct SemanticForcesActor {
    /// Shared GPU context for accessing GPU resources
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Current semantic configuration
    config: SemanticConfig,

    /// GPU state tracking
    gpu_state: GPUState,

    /// Cached hierarchy levels (computed on demand)
    hierarchy_levels: Option<HierarchyLevels>,

    /// Cached type centroids (recomputed each frame)
    type_centroids: Option<TypeCentroids>,

    /// Number of node types in the graph
    num_types: usize,

    /// Cached node types array for GPU access
    node_types: Vec<i32>,

    /// Cached edge data for attribute springs
    edge_sources: Vec<i32>,
    edge_targets: Vec<i32>,
    edge_weights: Vec<f32>,
    edge_types: Vec<i32>,

    // ── Phase A semantic metadata (host-side mirrors of UnifiedGPUCompute buffers) ──

    /// Per-node physicality codes (PhysicalityCode::as_i32()).
    /// Populated via `upload_semantic_metadata` on the shared compute context.
    node_physicality: Vec<i32>,

    /// Per-node role codes (RoleCode::as_i32()).
    node_role: Vec<i32>,

    /// Per-node maturity levels (MaturityLevel::as_i32()).
    node_maturity: Vec<i32>,

    /// Centroid scratch buffer for physicality clustering (4 buckets × Float3).
    physicality_centroids: Vec<kernel_bridge::Float3>,

    /// Count scratch buffer for physicality clustering (4 buckets).
    physicality_counts: Vec<i32>,

    /// Centroid scratch buffer for role clustering (7 buckets × Float3).
    role_centroids: Vec<kernel_bridge::Float3>,

    /// Count scratch buffer for role clustering (7 buckets).
    role_counts: Vec<i32>,
}

impl SemanticForcesActor {
    pub fn new() -> Self {
        Self {
            shared_context: None,
            config: SemanticConfig::default(),
            gpu_state: GPUState::default(),
            hierarchy_levels: None,
            type_centroids: None,
            num_types: 0,
            node_types: Vec::new(),
            edge_sources: Vec::new(),
            edge_targets: Vec::new(),
            edge_weights: Vec::new(),
            edge_types: Vec::new(),
        }
    }

    /// Convert Rust config to GPU C-compatible struct
    /// This function creates a SemanticConfigGPU struct that matches the C++ SemanticConfig
    /// layout exactly. All 9 sub-configs must be populated to prevent undefined behavior.
    fn config_to_gpu(&self) -> SemanticConfigGPU {
        SemanticConfigGPU {
            dag: DAGConfigGPU {
                vertical_spacing: self.config.dag.vertical_spacing,
                horizontal_spacing: self.config.dag.horizontal_spacing,
                level_attraction: self.config.dag.level_attraction,
                sibling_repulsion: self.config.dag.sibling_repulsion,
                enabled: self.config.dag.enabled,
                _pad: [0; 3],
            },
            type_cluster: TypeClusterConfigGPU {
                cluster_attraction: self.config.type_cluster.cluster_attraction,
                cluster_radius: self.config.type_cluster.cluster_radius,
                inter_cluster_repulsion: self.config.type_cluster.inter_cluster_repulsion,
                enabled: self.config.type_cluster.enabled,
                _pad: [0; 3],
            },
            collision: CollisionConfigGPU {
                min_distance: self.config.collision.min_distance,
                collision_strength: self.config.collision.collision_strength,
                node_radius: self.config.collision.node_radius,
                enabled: self.config.collision.enabled,
                _pad: [0; 3],
            },
            attribute_spring: AttributeSpringConfigGPU {
                base_spring_k: self.config.attribute_spring.base_spring_k,
                weight_multiplier: self.config.attribute_spring.weight_multiplier,
                rest_length_min: self.config.attribute_spring.rest_length_min,
                rest_length_max: self.config.attribute_spring.rest_length_max,
                enabled: self.config.attribute_spring.enabled,
                _pad: [0; 3],
            },
            // Legacy ontology relationship config - unused by GPU, uses DynamicRelationshipBuffer
            ontology_relationship: OntologyRelationshipConfigGPU {
                requires_strength: 0.0,
                requires_rest_length: 0.0,
                enables_strength: 0.0,
                enables_rest_length: 0.0,
                has_part_strength: 0.0,
                has_part_orbit_radius: 0.0,
                bridges_to_strength: 0.0,
                bridges_to_rest_length: 0.0,
                enabled: false,
                _pad: [0; 3],
            },
            // Physicality clustering config - defaults to disabled
            physicality_cluster: PhysicalityClusterConfigGPU {
                cluster_attraction: 0.4,
                cluster_radius: 80.0,
                inter_physicality_repulsion: 0.2,
                enabled: false,
                _pad: [0; 3],
            },
            // Role clustering config - defaults to disabled
            role_cluster: RoleClusterConfigGPU {
                cluster_attraction: 0.4,
                cluster_radius: 80.0,
                inter_role_repulsion: 0.2,
                enabled: false,
                _pad: [0; 3],
            },
            // Maturity layout config - defaults to disabled
            maturity_layout: MaturityLayoutConfigGPU {
                vertical_spacing: 100.0,
                level_attraction: 0.5,
                stage_separation: 150.0,
                enabled: false,
                _pad: [0; 3],
            },
            // Cross-domain config - defaults to disabled
            cross_domain: CrossDomainConfigGPU {
                base_strength: 0.3,
                link_count_multiplier: 0.1,
                max_strength_boost: 2.0,
                rest_length: 200.0,
                enabled: false,
                _pad: [0; 3],
            },
        }
    }

    /// Calculate hierarchy levels using topological sort (BFS-style on GPU)
    fn calculate_hierarchy_levels(
        &mut self,
        num_nodes: usize,
        num_edges: usize,
    ) -> Result<HierarchyLevels, String> {
        info!("SemanticForcesActor: Calculating hierarchy levels for {} nodes, {} edges",
              num_nodes, num_edges);

        let _shared_context = self.shared_context.as_ref()
            .ok_or("GPU context not initialized")?;

        // Initialize node levels to -1 (not in hierarchy)
        let mut node_levels = vec![-1i32; num_nodes];

        // Find root nodes (nodes with no incoming hierarchy edges)
        let mut has_incoming_hierarchy = vec![false; num_nodes];
        for i in 0..self.edge_sources.len() {
            if self.edge_types[i] == 2 { // Hierarchy edge type = 2
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

    /// Calculate centroids for each node type
    fn calculate_type_centroids(
        &mut self,
        positions: &[(f32, f32, f32)],
        num_nodes: usize,
    ) -> Result<TypeCentroids, String> {
        if self.num_types == 0 {
            return Ok(TypeCentroids {
                centroids: Vec::new(),
                type_counts: Vec::new(),
            });
        }

        let mut centroids = vec![(0.0f32, 0.0f32, 0.0f32); self.num_types];
        let mut type_counts = vec![0usize; self.num_types];

        // Calculate type centroids via kernel bridge (GPU or CPU fallback)
        if self.shared_context.is_some() && num_nodes > 0 && !self.node_types.is_empty() {
            let mut centroid_f3 = vec![kernel_bridge::Float3 { x: 0.0, y: 0.0, z: 0.0 }; self.num_types];
            let mut counts_i32 = vec![0i32; self.num_types];

            let positions_f3: Vec<kernel_bridge::Float3> = positions.iter()
                .map(|(x, y, z)| kernel_bridge::Float3 { x: *x, y: *y, z: *z })
                .collect();

            kernel_bridge::calculate_type_centroids(
                &self.node_types,
                &positions_f3,
                &mut centroid_f3,
                &mut counts_i32,
                num_nodes,
                self.num_types,
            );

            kernel_bridge::finalize_type_centroids(
                &mut centroid_f3,
                &counts_i32,
                self.num_types,
            );

            centroids = centroid_f3.iter()
                .map(|f3| (f3.x, f3.y, f3.z))
                .collect();
            type_counts = counts_i32.iter()
                .map(|&c| c as usize)
                .collect();
        }

        Ok(TypeCentroids {
            centroids,
            type_counts,
        })
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
        info!("SemanticForcesActor: DAG config updated, enabled={}", self.config.dag.enabled);
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
        info!("SemanticForcesActor: Type clustering config updated, enabled={}", self.config.type_cluster.enabled);
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
        info!("SemanticForcesActor: Collision config updated, enabled={}", self.config.collision.enabled);
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

        // Convert from actor's DynamicForceConfigGPU to kernel_bridge's canonical type.
        // Both are #[repr(C)] with identical layout (f32, f32, i32, u32 = 16 bytes).
        // SAFETY: This transmute-via-pointer-cast is safe because:
        // 1. Both types are #[repr(C)] with identical field layout (f32, f32, i32, u32)
        // 2. Compile-time size assertions elsewhere in this module verify
        //    size_of::<actor::DynamicForceConfigGPU>() == size_of::<kernel_bridge::DynamicForceConfigGPU>()
        // 3. `msg.buffer` is a valid, non-empty slice (checked above: buffer.is_empty() returns early)
        // 4. The resulting slice borrows from `msg.buffer` with the same lifetime,
        //    so the pointer remains valid for the duration of the borrow
        // 5. Alignment: both types have alignment of 4 (all fields are 4-byte types)
        let bridge_buffer: &[kernel_bridge::DynamicForceConfigGPU] = unsafe {
            std::slice::from_raw_parts(
                msg.buffer.as_ptr() as *const kernel_bridge::DynamicForceConfigGPU,
                msg.buffer.len(),
            )
        };
        let result = kernel_bridge::set_dynamic_relationship_buffer(bridge_buffer, true);

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