// Compiled by build.rs as both PTX (for kernel launches) and an object file
// linked into libthrust_wrapper.a (for host-callable FFI).
// FFI declarations in src/gpu/kernel_bridge.rs.
//
// Semantic Forces GPU Kernel - Type-aware physics for knowledge graphs
// Implements DAG layout, type clustering, collision detection, and attribute-weighted springs.

#include <cuda_runtime.h>
#include <device_launch_parameters.h>
#include <cfloat>
#include <cmath>

extern "C" {
// Link-time probe paired with the stub in src/utils/cuda_ffi_stubs.c (returns 1
// there). 0 means the semantic-force host wrappers in this file are real.
int visionclaw_ffi_stubbed(void) { return 0; }
}

extern "C" {

// =============================================================================
// Configuration Structures
// =============================================================================

// DAG layout configuration
struct DAGConfig {
    float vertical_spacing;      // Vertical separation between hierarchy levels
    float horizontal_spacing;    // Minimum horizontal separation within a level
    float level_attraction;      // Strength of attraction to target level
    float sibling_repulsion;     // Repulsion between nodes at same level
    bool enabled;
};

// Type clustering configuration
struct TypeClusterConfig {
    float cluster_attraction;    // Attraction between nodes of same type
    float cluster_radius;        // Target radius for type clusters
    float inter_cluster_repulsion; // Repulsion between different type clusters
    bool enabled;
};

// Collision detection configuration
struct CollisionConfig {
    float min_distance;          // Minimum allowed distance between nodes
    float collision_strength;    // Force strength when colliding
    float node_radius;           // Default node radius
    bool enabled;
};

// Attribute-weighted spring configuration
struct AttributeSpringConfig {
    float base_spring_k;         // Base spring constant
    float weight_multiplier;     // Multiplier for edge weight influence
    float rest_length_min;       // Minimum rest length
    float rest_length_max;       // Maximum rest length
    bool enabled;
};

// Ontology relationship forces configuration
// NOTE: The 'enabled' flag is used by CPU fallback; GPU uses DynamicRelationshipBuffer.
// Force parameters in this struct are legacy - CPU fallback uses SemanticTypeRegistry instead.
struct OntologyRelationshipConfig {
    float requires_strength;      // Legacy: unused by GPU (uses DynamicRelationshipBuffer)
    float requires_rest_length;   // Legacy: unused by GPU
    float enables_strength;       // Legacy: unused by GPU
    float enables_rest_length;    // Legacy: unused by GPU
    float has_part_strength;      // Legacy: unused by GPU
    float has_part_orbit_radius;  // Legacy: unused by GPU
    float bridges_to_strength;    // Legacy: unused by GPU
    float bridges_to_rest_length; // Legacy: unused by GPU
    bool enabled;                 // Used by CPU fallback as feature toggle
};

// =============================================================================
// Dynamic Semantic Force Configuration Buffer (Schema-Code Decoupling)
// =============================================================================

// Maximum number of dynamically registered relationship types
// This allows ontology to add new types without CUDA recompilation
#define MAX_RELATIONSHIP_TYPES 256

// Force configuration for a single relationship type
// Matches Rust's RelationshipForceConfig struct layout
struct DynamicForceConfig {
    float strength;           // Spring strength (can be negative for repulsion)
    float rest_length;        // Rest length for spring calculations
    int is_directional;       // 1 = directional (source → target), 0 = bidirectional
    unsigned int force_type;  // Force behavior type:
                              //   0 = standard spring
                              //   1 = orbit clustering (has-part)
                              //   2 = cross-domain long-range
                              //   3 = repulsion
};

// Dynamic configuration buffer for all relationship types
// Indexed by edge_type ID from SemanticTypeRegistry
struct DynamicRelationshipBuffer {
    DynamicForceConfig configs[MAX_RELATIONSHIP_TYPES];
    int num_types;            // Number of registered types
    int buffer_version;       // Version for hot-reload detection
    bool enabled;             // Master enable for dynamic forces
};

// Physicality clustering configuration
struct PhysicalityClusterConfig {
    float cluster_attraction;
    float cluster_radius;
    float inter_physicality_repulsion;
    bool enabled;
};

// Role clustering configuration
struct RoleClusterConfig {
    float cluster_attraction;
    float cluster_radius;
    float inter_role_repulsion;
    bool enabled;
};

// Maturity layout configuration
struct MaturityLayoutConfig {
    float vertical_spacing;
    float level_attraction;
    float stage_separation;
    bool enabled;
};

// Cross-domain configuration
struct CrossDomainConfig {
    float base_strength;
    float link_count_multiplier;
    float max_strength_boost;
    float rest_length;
    bool enabled;
};

// Unified semantic configuration
struct SemanticConfig {
    DAGConfig dag;
    TypeClusterConfig type_cluster;
    CollisionConfig collision;
    AttributeSpringConfig attribute_spring;
    OntologyRelationshipConfig ontology_relationship;
    PhysicalityClusterConfig physicality_cluster;
    RoleClusterConfig role_cluster;
    MaturityLayoutConfig maturity_layout;
    CrossDomainConfig cross_domain;
};

// Global constant memory for semantic configuration
__constant__ SemanticConfig c_semantic_config;

// Global constant memory for dynamic relationship buffer
// This enables ontology changes without CUDA recompilation
__constant__ DynamicRelationshipBuffer c_dynamic_relationships;

// Device-side relationship buffer pointer (for larger configurations)
__device__ DynamicForceConfig* d_relationship_buffer = nullptr;
__device__ int d_buffer_size = 0;

// =============================================================================
// Helper Functions
// =============================================================================

__device__ inline float3 operator+(const float3& a, const float3& b) {
    return make_float3(a.x + b.x, a.y + b.y, a.z + b.z);
}

__device__ inline float3 operator-(const float3& a, const float3& b) {
    return make_float3(a.x - b.x, a.y - b.y, a.z - b.z);
}

__device__ inline float3 operator*(const float3& a, float s) {
    return make_float3(a.x * s, a.y * s, a.z * s);
}

__device__ inline float length(const float3& v) {
    // Use FMA for better performance and accuracy
    return sqrtf(fmaf(v.x, v.x, fmaf(v.y, v.y, v.z * v.z)));
}

__device__ inline float3 normalize(const float3& v) {
    float len = length(v);
    if (len > 1e-6f) {
        return make_float3(v.x / len, v.y / len, v.z / len);
    }
    return make_float3(0.0f, 0.0f, 0.0f);
}

// =============================================================================
// DAG Layout Kernel
// =============================================================================

// Apply hierarchical layout forces based on node hierarchy levels
__global__ void apply_dag_force(
    const int* node_hierarchy_levels,  // Hierarchy level for each node
    const int* node_types,             // Node type for each node
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    if (!c_semantic_config.dag.enabled) return;

    int level = node_hierarchy_levels[idx];
    if (level < 0) return; // Skip nodes not in DAG

    // Calculate target Y position based on hierarchy level
    float target_y = level * c_semantic_config.dag.vertical_spacing;
    float current_y = positions[idx].y;
    float dy = target_y - current_y;

    // Apply vertical attraction to target level
    float3 level_force = make_float3(
        0.0f,
        dy * c_semantic_config.dag.level_attraction,
        0.0f
    );

    // Apply horizontal repulsion from siblings at same level
    float3 sibling_repulsion = make_float3(0.0f, 0.0f, 0.0f);
    for (int i = 0; i < num_nodes; i++) {
        if (i == idx) continue;
        if (node_hierarchy_levels[i] != level) continue;

        float3 delta = positions[idx] - positions[i];
        delta.y = 0.0f; // Only horizontal repulsion
        float dist = length(delta);

        if (dist < c_semantic_config.dag.horizontal_spacing && dist > 1e-6f) {
            float force_mag = c_semantic_config.dag.sibling_repulsion *
                            (c_semantic_config.dag.horizontal_spacing - dist) / dist;
            sibling_repulsion = sibling_repulsion + (delta * force_mag);
        }
    }

    // Accumulate forces
    atomicAdd(&forces[idx].x, level_force.x + sibling_repulsion.x);
    atomicAdd(&forces[idx].y, level_force.y + sibling_repulsion.y);
    atomicAdd(&forces[idx].z, level_force.z + sibling_repulsion.z);
}

// =============================================================================
// Type Clustering Kernel
// =============================================================================

// Apply type-based clustering forces
__global__ void apply_type_cluster_force(
    const int* node_types,             // Node type for each node
    const float3* type_centroids,      // Centroid position for each type
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_nodes,
    const int num_types
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    if (!c_semantic_config.type_cluster.enabled) return;

    int node_type = node_types[idx];
    if (node_type < 0 || node_type >= num_types) return;

    // Attraction to type centroid
    float3 to_centroid = type_centroids[node_type] - positions[idx];
    float dist_to_centroid = length(to_centroid);

    float3 cluster_force = make_float3(0.0f, 0.0f, 0.0f);
    if (dist_to_centroid > c_semantic_config.type_cluster.cluster_radius) {
        // Outside cluster radius - attract inward
        float force_mag = c_semantic_config.type_cluster.cluster_attraction *
                        (dist_to_centroid - c_semantic_config.type_cluster.cluster_radius);
        cluster_force = normalize(to_centroid) * force_mag;
    }

    // Repulsion from nodes of different types
    float3 inter_cluster_repulsion = make_float3(0.0f, 0.0f, 0.0f);
    for (int i = 0; i < num_nodes; i++) {
        if (i == idx) continue;
        if (node_types[i] == node_type) continue; // Same type

        float3 delta = positions[idx] - positions[i];
        float dist = length(delta);

        if (dist < c_semantic_config.type_cluster.cluster_radius * 2.0f && dist > 1e-6f) {
            float force_mag = c_semantic_config.type_cluster.inter_cluster_repulsion / (dist * dist);
            inter_cluster_repulsion = inter_cluster_repulsion + (normalize(delta) * force_mag);
        }
    }

    // Accumulate forces
    atomicAdd(&forces[idx].x, cluster_force.x + inter_cluster_repulsion.x);
    atomicAdd(&forces[idx].y, cluster_force.y + inter_cluster_repulsion.y);
    atomicAdd(&forces[idx].z, cluster_force.z + inter_cluster_repulsion.z);
}

// =============================================================================
// Collision Detection Kernel
// =============================================================================

// Apply collision detection and response forces
__global__ void apply_collision_force(
    const float* node_radii,           // Radius for each node (can be NULL for default)
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    if (!c_semantic_config.collision.enabled) return;

    float radius_a = node_radii ? node_radii[idx] : c_semantic_config.collision.node_radius;

    float3 collision_force = make_float3(0.0f, 0.0f, 0.0f);

    // Check collision with all other nodes
    for (int i = 0; i < num_nodes; i++) {
        if (i == idx) continue;

        float radius_b = node_radii ? node_radii[i] : c_semantic_config.collision.node_radius;
        float min_dist = radius_a + radius_b + c_semantic_config.collision.min_distance;

        float3 delta = positions[idx] - positions[i];
        float dist = length(delta);

        if (dist < min_dist && dist > 1e-6f) {
            // Collision detected - apply repulsion
            float overlap = min_dist - dist;
            float force_mag = c_semantic_config.collision.collision_strength * overlap;
            collision_force = collision_force + (normalize(delta) * force_mag);
        }
    }

    // Accumulate forces
    atomicAdd(&forces[idx].x, collision_force.x);
    atomicAdd(&forces[idx].y, collision_force.y);
    atomicAdd(&forces[idx].z, collision_force.z);
}

// =============================================================================
// Attribute-Weighted Spring Kernel
// =============================================================================

// Apply spring forces weighted by edge attributes
__global__ void apply_attribute_spring_force(
    const int* edge_sources,           // Source node index for each edge
    const int* edge_targets,           // Target node index for each edge
    const float* edge_weights,         // Weight/strength for each edge
    const int* edge_types,             // Type for each edge
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_edges
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_edges) return;

    if (!c_semantic_config.attribute_spring.enabled) return;

    int src = edge_sources[idx];
    int tgt = edge_targets[idx];
    float weight = edge_weights[idx];

    // Calculate spring force
    float3 delta = positions[tgt] - positions[src];
    float dist = length(delta);

    if (dist < 1e-6f) return;

    // Weight influences spring constant and rest length
    float spring_k = c_semantic_config.attribute_spring.base_spring_k *
                    (1.0f + weight * c_semantic_config.attribute_spring.weight_multiplier);

    // Rest length inversely proportional to weight (stronger edges = shorter rest length)
    float rest_length = c_semantic_config.attribute_spring.rest_length_max -
                       (weight * (c_semantic_config.attribute_spring.rest_length_max -
                                c_semantic_config.attribute_spring.rest_length_min));

    // Hooke's law: F = -k * (x - x0)
    float displacement = dist - rest_length;
    float force_mag = spring_k * displacement;

    float3 spring_force = normalize(delta) * force_mag;

    // Apply equal and opposite forces
    atomicAdd(&forces[src].x, spring_force.x);
    atomicAdd(&forces[src].y, spring_force.y);
    atomicAdd(&forces[src].z, spring_force.z);

    atomicAdd(&forces[tgt].x, -spring_force.x);
    atomicAdd(&forces[tgt].y, -spring_force.y);
    atomicAdd(&forces[tgt].z, -spring_force.z);
}

// =============================================================================
// Hierarchy Level Calculation (Utility)
// =============================================================================

// Calculate hierarchy levels for DAG layout using BFS-style parallel approach
__global__ void calculate_hierarchy_levels(
    const int* edge_sources,           // Source node index for each edge
    const int* edge_targets,           // Target node index for each edge
    const int* edge_types,             // Edge type (use hierarchy type = 2)
    int* node_levels,                  // Output: hierarchy level for each node
    bool* changed,                     // Flag indicating if any level changed
    const int num_edges,
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_edges) return;

    int edge_type = edge_types[idx];

    // Only process hierarchy edges (type 2 = Hierarchy)
    if (edge_type != 2) return;

    int parent = edge_sources[idx];
    int child = edge_targets[idx];

    int parent_level = node_levels[parent];
    if (parent_level >= 0) {
        int new_child_level = parent_level + 1;
        int old_child_level = atomicMax(&node_levels[child], new_child_level);

        if (old_child_level < new_child_level) {
            *changed = true;
        }
    }
}

// =============================================================================
// Type Centroid Calculation (Utility)
// =============================================================================

// Calculate centroid positions for each node type
__global__ void calculate_type_centroids(
    const int* node_types,             // Node type for each node
    const float3* positions,           // Current positions
    float3* type_centroids,            // Output: centroid for each type
    int* type_counts,                  // Output: count for each type
    const int num_nodes,
    const int num_types
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    int node_type = node_types[idx];
    if (node_type < 0 || node_type >= num_types) return;

    // Atomic add to accumulate positions
    atomicAdd(&type_centroids[node_type].x, positions[idx].x);
    atomicAdd(&type_centroids[node_type].y, positions[idx].y);
    atomicAdd(&type_centroids[node_type].z, positions[idx].z);
    atomicAdd(&type_counts[node_type], 1);
}

// Finalize centroids by dividing by count
__global__ void finalize_type_centroids(
    float3* type_centroids,            // Centroids to finalize
    const int* type_counts,            // Count for each type
    const int num_types
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_types) return;

    int count = type_counts[idx];
    if (count > 0) {
        type_centroids[idx].x /= count;
        type_centroids[idx].y /= count;
        type_centroids[idx].z /= count;
    }
}

// =============================================================================
// Dynamic Relationship Forces Kernel (Schema-Code Decoupled)
// =============================================================================

// Apply dynamic relationship forces using registry-based configuration
// This kernel reads force parameters from the dynamic buffer, enabling
// new ontology relationship types without CUDA recompilation
__global__ void apply_dynamic_relationship_force(
    const int* edge_sources,           // Source node index for each edge
    const int* edge_targets,           // Target node index for each edge
    const int* edge_types,             // Type ID for each edge (from SemanticTypeRegistry)
    const int* node_cross_domain_count, // Cross-domain link count per node
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_edges
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_edges) return;

    if (!c_dynamic_relationships.enabled) return;

    int src = edge_sources[idx];
    int tgt = edge_targets[idx];
    int edge_type_id = edge_types[idx];

    // Bounds check for edge type ID
    if (edge_type_id < 0 || edge_type_id >= c_dynamic_relationships.num_types) return;

    // Lookup force configuration from dynamic buffer (no switch statement!)
    DynamicForceConfig config = c_dynamic_relationships.configs[edge_type_id];

    // Skip if strength is effectively zero
    if (fabsf(config.strength) < 1e-6f) return;

    float3 delta = positions[tgt] - positions[src];
    float dist = length(delta);
    if (dist < 1e-6f) return;

    float strength = config.strength;
    float rest_length = config.rest_length;

    // Apply force type-specific modifiers
    switch (config.force_type) {
        case 0: // Standard spring force
            // No modification needed
            break;

        case 1: // Orbit clustering (has-part)
            // Parts orbit around the whole - use rest_length as orbit radius
            // Add slight tangential component for orbital motion
            break;

        case 2: // Cross-domain long-range spring
            // Strength increases with cross-domain link count
            if (node_cross_domain_count != nullptr) {
                float src_count = (float)node_cross_domain_count[src];
                float tgt_count = (float)node_cross_domain_count[tgt];
                float avg_count = (src_count + tgt_count) * 0.5f;
                float boost = fminf(1.0f + avg_count * c_semantic_config.cross_domain.link_count_multiplier,
                                   c_semantic_config.cross_domain.max_strength_boost);
                strength *= boost;
            }
            break;

        case 3: // Repulsion force
            // Negative strength creates repulsion
            // Only apply when distance is less than rest_length
            if (dist >= rest_length) return;
            break;

        default:
            // Unknown force type - use as standard spring
            break;
    }

    // Hooke's law: F = -k * (x - x0)
    float displacement = dist - rest_length;
    float force_mag = strength * displacement / dist;
    float3 spring_force = normalize(delta) * force_mag;

    if (config.is_directional) {
        // Directional: only source is pulled toward target
        atomicAdd(&forces[src].x, spring_force.x);
        atomicAdd(&forces[src].y, spring_force.y);
        atomicAdd(&forces[src].z, spring_force.z);
    } else {
        // Bidirectional spring force
        atomicAdd(&forces[src].x, spring_force.x);
        atomicAdd(&forces[src].y, spring_force.y);
        atomicAdd(&forces[src].z, spring_force.z);
        atomicAdd(&forces[tgt].x, -spring_force.x);
        atomicAdd(&forces[tgt].y, -spring_force.y);
        atomicAdd(&forces[tgt].z, -spring_force.z);
    }
}

// =============================================================================
// Physicality Clustering Kernel
// =============================================================================

// Apply physicality-based clustering forces (VirtualEntity, PhysicalEntity, ConceptualEntity)
__global__ void apply_physicality_cluster_force(
    const int* node_physicality,       // Physicality type for each node (0=None, 1=Virtual, 2=Physical, 3=Conceptual)
    const float3* physicality_centroids, // Centroid position for each physicality type
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    if (!c_semantic_config.physicality_cluster.enabled) return;

    int physicality = node_physicality[idx];
    if (physicality <= 0 || physicality > 3) return;

    // Attraction to physicality centroid
    float3 to_centroid = physicality_centroids[physicality] - positions[idx];
    float dist_to_centroid = length(to_centroid);

    float3 cluster_force = make_float3(0.0f, 0.0f, 0.0f);
    if (dist_to_centroid > c_semantic_config.physicality_cluster.cluster_radius) {
        // Outside cluster radius - attract inward
        float force_mag = c_semantic_config.physicality_cluster.cluster_attraction *
                        (dist_to_centroid - c_semantic_config.physicality_cluster.cluster_radius);
        cluster_force = normalize(to_centroid) * force_mag;
    }

    // Repulsion from nodes of different physicality
    float3 inter_physicality_repulsion = make_float3(0.0f, 0.0f, 0.0f);
    for (int i = 0; i < num_nodes; i++) {
        if (i == idx) continue;
        int other_physicality = node_physicality[i];
        if (other_physicality == physicality || other_physicality == 0) continue;

        float3 delta = positions[idx] - positions[i];
        float dist = length(delta);

        if (dist < c_semantic_config.physicality_cluster.cluster_radius * 2.0f && dist > 1e-6f) {
            float force_mag = c_semantic_config.physicality_cluster.inter_physicality_repulsion / (dist * dist);
            inter_physicality_repulsion = inter_physicality_repulsion + (normalize(delta) * force_mag);
        }
    }

    // Accumulate forces
    atomicAdd(&forces[idx].x, cluster_force.x + inter_physicality_repulsion.x);
    atomicAdd(&forces[idx].y, cluster_force.y + inter_physicality_repulsion.y);
    atomicAdd(&forces[idx].z, cluster_force.z + inter_physicality_repulsion.z);
}

// =============================================================================
// Role Clustering Kernel
// =============================================================================

// Apply role-based clustering forces (Process, Agent, Resource, Concept)
__global__ void apply_role_cluster_force(
    const int* node_role,              // Role type for each node (0=None, 1=Process, 2=Agent, 3=Resource, 4=Concept)
    const float3* role_centroids,      // Centroid position for each role type
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    if (!c_semantic_config.role_cluster.enabled) return;

    int role = node_role[idx];
    if (role <= 0 || role > 4) return;

    // Attraction to role centroid
    float3 to_centroid = role_centroids[role] - positions[idx];
    float dist_to_centroid = length(to_centroid);

    float3 cluster_force = make_float3(0.0f, 0.0f, 0.0f);
    if (dist_to_centroid > c_semantic_config.role_cluster.cluster_radius) {
        // Outside cluster radius - attract inward
        float force_mag = c_semantic_config.role_cluster.cluster_attraction *
                        (dist_to_centroid - c_semantic_config.role_cluster.cluster_radius);
        cluster_force = normalize(to_centroid) * force_mag;
    }

    // Repulsion from nodes of different roles
    float3 inter_role_repulsion = make_float3(0.0f, 0.0f, 0.0f);
    for (int i = 0; i < num_nodes; i++) {
        if (i == idx) continue;
        int other_role = node_role[i];
        if (other_role == role || other_role == 0) continue;

        float3 delta = positions[idx] - positions[i];
        float dist = length(delta);

        if (dist < c_semantic_config.role_cluster.cluster_radius * 2.0f && dist > 1e-6f) {
            float force_mag = c_semantic_config.role_cluster.inter_role_repulsion / (dist * dist);
            inter_role_repulsion = inter_role_repulsion + (normalize(delta) * force_mag);
        }
    }

    // Accumulate forces
    atomicAdd(&forces[idx].x, cluster_force.x + inter_role_repulsion.x);
    atomicAdd(&forces[idx].y, cluster_force.y + inter_role_repulsion.y);
    atomicAdd(&forces[idx].z, cluster_force.z + inter_role_repulsion.z);
}

// =============================================================================
// Maturity Layout Kernel
// =============================================================================

// Apply maturity-based layout forces (emerging → mature → declining)
__global__ void apply_maturity_layout_force(
    const int* node_maturity,          // Maturity stage for each node (0=None, 1=emerging, 2=mature, 3=declining)
    float3* positions,                 // Current positions
    float3* forces,                    // Force accumulator
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    if (!c_semantic_config.maturity_layout.enabled) return;

    int maturity = node_maturity[idx];
    if (maturity <= 0 || maturity > 3) return;

    // Calculate target Z position based on maturity stage
    // emerging=1 → z=-stage_separation
    // mature=2   → z=0
    // declining=3 → z=+stage_separation
    float target_z;
    switch (maturity) {
        case 1: // emerging
            target_z = -c_semantic_config.maturity_layout.stage_separation;
            break;
        case 2: // mature
            target_z = 0.0f;
            break;
        case 3: // declining
            target_z = c_semantic_config.maturity_layout.stage_separation;
            break;
        default:
            return;
    }

    float dz = target_z - positions[idx].z;
    float3 maturity_force = make_float3(
        0.0f,
        0.0f,
        dz * c_semantic_config.maturity_layout.level_attraction
    );

    // Accumulate forces
    atomicAdd(&forces[idx].x, maturity_force.x);
    atomicAdd(&forces[idx].y, maturity_force.y);
    atomicAdd(&forces[idx].z, maturity_force.z);
}

// =============================================================================
// Calculate Physicality Centroids (Utility)
// =============================================================================

// Calculate centroid positions for each physicality type
__global__ void calculate_physicality_centroids(
    const int* node_physicality,       // Physicality type for each node
    const float3* positions,           // Current positions
    float3* physicality_centroids,     // Output: centroid for each physicality type
    int* physicality_counts,           // Output: count for each physicality type
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    int physicality = node_physicality[idx];
    if (physicality <= 0 || physicality > 3) return;

    // Atomic add to accumulate positions
    atomicAdd(&physicality_centroids[physicality].x, positions[idx].x);
    atomicAdd(&physicality_centroids[physicality].y, positions[idx].y);
    atomicAdd(&physicality_centroids[physicality].z, positions[idx].z);
    atomicAdd(&physicality_counts[physicality], 1);
}

// Finalize physicality centroids by dividing by count
__global__ void finalize_physicality_centroids(
    float3* physicality_centroids,     // Centroids to finalize
    const int* physicality_counts      // Count for each physicality type
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx > 3) return; // Only 3 physicality types

    int count = physicality_counts[idx];
    if (count > 0) {
        physicality_centroids[idx].x /= count;
        physicality_centroids[idx].y /= count;
        physicality_centroids[idx].z /= count;
    }
}

// =============================================================================
// Calculate Role Centroids (Utility)
// =============================================================================

// Calculate centroid positions for each role type
__global__ void calculate_role_centroids(
    const int* node_role,              // Role type for each node
    const float3* positions,           // Current positions
    float3* role_centroids,            // Output: centroid for each role type
    int* role_counts,                  // Output: count for each role type
    const int num_nodes
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    int role = node_role[idx];
    if (role <= 0 || role > 4) return;

    // Atomic add to accumulate positions
    atomicAdd(&role_centroids[role].x, positions[idx].x);
    atomicAdd(&role_centroids[role].y, positions[idx].y);
    atomicAdd(&role_centroids[role].z, positions[idx].z);
    atomicAdd(&role_counts[role], 1);
}

// Finalize role centroids by dividing by count
__global__ void finalize_role_centroids(
    float3* role_centroids,            // Centroids to finalize
    const int* role_counts             // Count for each role type
) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx > 4) return; // Only 4 role types

    int count = role_counts[idx];
    if (count > 0) {
        role_centroids[idx].x /= count;
        role_centroids[idx].y /= count;
        role_centroids[idx].z /= count;
    }
}

// =============================================================================
// Configuration Setup
// =============================================================================

// Upload semantic configuration to constant memory
void set_semantic_config(const SemanticConfig* config) {
    cudaMemcpyToSymbol(c_semantic_config, config, sizeof(SemanticConfig));
}

// =============================================================================
// Dynamic Relationship Buffer Management (Hot-Reload Support)
// =============================================================================

// Current buffer version for hot-reload detection
static int s_current_buffer_version = 0;

// Upload dynamic relationship configurations to GPU constant memory
// This function enables hot-reload: call whenever ontology changes
// Returns 0 on success, CUDA error code on failure
int set_dynamic_relationship_buffer(
    const DynamicForceConfig* configs,  // Array of force configurations
    int num_types,                       // Number of relationship types
    bool enabled                         // Enable dynamic relationship forces
) {
    if (num_types > MAX_RELATIONSHIP_TYPES) {
        return -1; // Too many types
    }
    if (configs == nullptr && num_types > 0) {
        return -2; // Invalid configuration
    }

    // Prepare the buffer
    DynamicRelationshipBuffer buffer;
    buffer.num_types = num_types;
    buffer.buffer_version = ++s_current_buffer_version;
    buffer.enabled = enabled;

    // Copy configurations
    if (configs != nullptr && num_types > 0) {
        memcpy(buffer.configs, configs, num_types * sizeof(DynamicForceConfig));
    }

    // Zero out remaining slots
    for (int i = num_types; i < MAX_RELATIONSHIP_TYPES; i++) {
        buffer.configs[i].strength = 0.0f;
        buffer.configs[i].rest_length = 100.0f;
        buffer.configs[i].is_directional = 0;
        buffer.configs[i].force_type = 0;
    }

    // Upload to GPU constant memory
    cudaError_t err = cudaMemcpyToSymbol(c_dynamic_relationships, &buffer, sizeof(DynamicRelationshipBuffer));
    return (err == cudaSuccess) ? 0 : (int)err;
}

// Update a single relationship type configuration (hot-reload single type)
// Useful for live-editing force parameters without full buffer upload
int update_dynamic_relationship_config(
    int type_id,                        // Relationship type ID
    const DynamicForceConfig* config    // New configuration
) {
    if (type_id < 0 || type_id >= MAX_RELATIONSHIP_TYPES || config == nullptr) {
        return -1;
    }

    // Calculate offset into the configs array within constant memory
    size_t offset = offsetof(DynamicRelationshipBuffer, configs) + type_id * sizeof(DynamicForceConfig);

    cudaError_t err = cudaMemcpyToSymbol(
        c_dynamic_relationships,
        config,
        sizeof(DynamicForceConfig),
        offset,
        cudaMemcpyHostToDevice
    );

    if (err == cudaSuccess) {
        // Update version to signal hot-reload
        s_current_buffer_version++;
        int new_version = s_current_buffer_version;
        size_t version_offset = offsetof(DynamicRelationshipBuffer, buffer_version);
        cudaMemcpyToSymbol(c_dynamic_relationships, &new_version, sizeof(int), version_offset, cudaMemcpyHostToDevice);
    }

    return (err == cudaSuccess) ? 0 : (int)err;
}

// Enable or disable dynamic relationship forces
int set_dynamic_relationships_enabled(bool enabled) {
    size_t offset = offsetof(DynamicRelationshipBuffer, enabled);
    cudaError_t err = cudaMemcpyToSymbol(c_dynamic_relationships, &enabled, sizeof(bool), offset, cudaMemcpyHostToDevice);
    return (err == cudaSuccess) ? 0 : (int)err;
}

// Get current buffer version (for hot-reload detection)
int get_dynamic_relationship_buffer_version() {
    return s_current_buffer_version;
}

// Get maximum supported relationship types
int get_max_relationship_types() {
    return MAX_RELATIONSHIP_TYPES;
}

} // extern "C"
