// VisionClaw Unified GPU Kernel - Rewritten for correctness, performance, and clarity.
// Implements a two-pass (force/integrate) simulation with double-buffering,
// uniform grid spatial hashing for repulsion, and CSR for spring forces.

#include <cuda_runtime.h>
#include <device_launch_parameters.h>
#include <thrust/device_vector.h>
#include <thrust/sort.h>
#include <thrust/execution_policy.h>
#include <cub/cub.cuh>
#include <curand_kernel.h>
#include <cfloat>

extern "C" {

// =============================================================================
// Core Data Structures & Constants
// =============================================================================

// Matches the Rust SimParams struct for FFI.
struct SimParams {
    float dt;
    float damping;
    unsigned int warmup_iterations;
    float cooling_rate;
    float spring_k;
    float rest_length;
    float repel_k;
    float repulsion_cutoff;
    float repulsion_softening_epsilon;
    float center_gravity_k;
    float max_force;
    float max_velocity;
    float grid_cell_size;
    unsigned int feature_flags;
    unsigned int seed;
    int iteration;
    // Additional fields for compatibility
    float separation_radius;
    float cluster_strength;
    float alignment_strength;
    float temperature;
    float viewport_bounds;
    // SSSP parameters
    float sssp_alpha;  // Strength of SSSP influence on spring forces
    float boundary_damping;  // Damping applied at boundaries
    // Constraint progressive activation parameters
    unsigned int constraint_ramp_frames;  // Number of frames to fully activate constraints
    float constraint_max_force_per_node;  // Maximum force per node from all constraints
    // GPU Stability Gates
    float stability_threshold;  // Kinetic energy threshold below which physics is skipped
    float min_velocity_threshold;  // Minimum node velocity to consider for physics

    // GPU clustering and analytics parameters
    float world_bounds_min;      // Minimum world coordinate
    float world_bounds_max;      // Maximum world coordinate
    float cell_size_lod;         // Level of detail cell size
    unsigned int k_neighbors_max;       // Maximum k-neighbors for LOF
    float anomaly_detection_radius; // Default radius for anomaly detection
    float learning_rate_default; // Default learning rate for GPU algorithms

    // Additional kernel constants for fine-tuning
    float norm_delta_cap;                   // Cap for SSSP delta normalization
    float position_constraint_attraction;   // Gentle attraction factor for position constraints
    float lof_score_min;                    // Minimum LOF score clamp
    float lof_score_max;                    // Maximum LOF score clamp
    float weight_precision_multiplier;      // Weight precision multiplier for integer operations
    // NOTE: Stress majorization params removed (unused by GPU kernels, handled on CPU)

    float gravity;  // Gravity pull toward origin (center-pull force)

    // ForceAtlas2 / LinLog parameters
    unsigned int lin_log_mode;  // 1 = LinLog attraction log(1+d), 0 = linear Hooke spring
    float scaling_ratio;        // FA2 repulsion: repulsion ∝ scaling_ratio * (deg_i+1) * (deg_j+1) / d
    unsigned int adaptive_speed; // 1 = per-node adaptive speed (FA2 convergence), 0 = global dt
    float global_speed;         // Base speed for adaptive integration

    // DAG radial hierarchy bias (PHASE 2). Pulls each ranked node toward a shell
    // of radius = rank * dag_level_distance centred on the hierarchy root
    // (approximated by the world origin, where center gravity already gathers the
    // rank-0 roots). dag_bias_k = 0 disables the term entirely (default OFF).
    float dag_bias_k;           // Radial-bias spring strength; 0 = off
    float dag_level_distance;   // World-space radius per hierarchy level (rank)

    // Layout mode selector (ADR-141 P1 — constrained-layout engine programme).
    // 0=ForceDirected, 1=Hierarchical, 2=Radial, 3=Spectral, 4=Temporal, 5=Clustered.
    // Carried GPU-side so force terms can branch on the active layout. P1 only makes
    // the field visible (the shell term is still driven by dag_bias_k, which the
    // server raises when a GPU-resident mode like Radial is selected); P2–P4 add the
    // per-mode force branches (stratified planes, spherical shells, Sugiyama Y-by-rank).
    // Added at the end to preserve the existing repr(C) prefix layout.
    unsigned int layout_mode;

    // Stratified planes by type (ADR-141 P2). Springs each node's Z toward its
    // assigned plane (target_z = node_plane[idx] * plane_spacing). plane_bias_k = 0
    // disables the term entirely (default OFF). Added at the end to preserve the
    // existing repr(C) prefix layout.
    float plane_bias_k;         // Plane-bias spring strength; 0 = off
    float plane_spacing;        // World-space Z distance per plane offset unit

    // Radial shell centre (ADR-141 P3). The dag_radial_bias term springs each node
    // onto its shell around this point instead of the hard-coded world origin.
    // (0,0,0) = origin = legacy DAG behaviour. Added at the end to preserve the
    // existing repr(C) prefix layout.
    float radial_center_x;
    float radial_center_y;
    float radial_center_z;

    // Sugiyama Y-by-rank layer spring (ADR-141 P4). Springs each ranked node's Y
    // toward y = node_rank[idx] * layer_spacing, giving a top-down layered
    // (Sugiyama) layout. layer_bias_k = 0 disables the term (default OFF). Added
    // at the end to preserve the existing repr(C) prefix layout.
    float layer_bias_k;         // Layer-bias spring strength; 0 = off
    float layer_spacing;        // World-space Y distance per rank
};

static_assert(sizeof(SimParams) == 212, "SimParams size mismatch with Rust");

// Global constant memory for simulation parameters
__constant__ SimParams c_params;


struct FeatureFlags {
    static const unsigned int ENABLE_REPULSION = 1 << 0;
    static const unsigned int ENABLE_SPRINGS = 1 << 1;
    static const unsigned int ENABLE_CENTERING = 1 << 2;
    static const unsigned int ENABLE_CONSTRAINTS = 1 << 4;  // Enable semantic constraints
    static const unsigned int ENABLE_SSSP_SPRING_ADJUST = 1 << 6;  // Enable SSSP-based spring adjustment
};

struct AABB {
    float3 min;
    float3 max;
};

// GPU-compatible constraint data for CUDA kernel
struct ConstraintData {
    int kind;                    // Discriminant matching ConstraintKind
    int count;                   // Number of node indices used
    int node_idx[4];            // Node indices (max 4 for GPU efficiency)
    float params[8];            // Parameters (max 8 for various constraint types)
    float weight;               // Weight of this constraint
    int activation_frame;       // Frame when this constraint was activated (for progressive activation)
};

// Constraint kinds enum — LIVE-KERNEL numbering. The OWL→constraint mapper
// (ADR-098 D1) emits these integers DIRECTLY; it does NOT route through the
// divergent domain `ConstraintKind` enum (verified-topology break #3). The old
// legacy aliases (`SEPARATION = 1` colliding with POSITION, `BOUNDARY = 6`,
// etc.) were removed because they conflated distinct semantics with the live
// branches; SEPARATION is now the canonical one-sided min-distance push.
enum ConstraintKind {
    DISTANCE = 0,    // attraction / colocate (subClassOf, partOf, sameAs, equivalentClass)
    POSITION = 1,    // attract node to a fixed target position
    ANGLE = 2,
    SEMANTIC = 3,
    TEMPORAL = 4,
    GROUP = 5,
    SEPARATION = 6,  // one-sided push-apart when current_dist < params[0] (disjointWith)
    ALIGNMENT = 7    // align constrained nodes on one axis (params[0]=axis 0/1/2)
};

// =============================================================================
// Device Helper Functions
// =============================================================================

__device__ inline float3 make_vec3(float x, float y, float z) { return make_float3(x, y, z); }
__device__ inline float3 vec3_add(float3 a, float3 b) { return make_float3(a.x + b.x, a.y + b.y, a.z + b.z); }
__device__ inline float3 vec3_sub(float3 a, float3 b) { return make_float3(a.x - b.x, a.y - b.y, a.z - b.z); }
__device__ inline float3 vec3_scale(float3 v, float s) { return make_float3(v.x * s, v.y * s, v.z * s); }
__device__ inline float vec3_dot(float3 a, float3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
__device__ inline float vec3_length_sq(float3 v) {
    // Use FMA for better performance
    return fmaf(v.x, v.x, fmaf(v.y, v.y, v.z * v.z));
}
__device__ inline float vec3_length(float3 v) { return sqrtf(vec3_length_sq(v)); }

__device__ inline int clamp_int(int x, int min, int max) {
    return (x < min) ? min : (x > max) ? max : x;
}

__device__ inline float clamp_float(float x, float min, float max) {
    return fminf(fmaxf(x, min), max);
}

__device__ inline float3 vec3_normalize(float3 v) {
    float len = vec3_length(v);
    return (len > 1e-6f) ? vec3_scale(v, 1.0f / len) : make_float3(0.0f, 0.0f, 0.0f);
}

__device__ inline float3 vec3_clamp(float3 v, float limit) {
    float len_sq = vec3_length_sq(v);
    if (len_sq > limit * limit) {
        float len = sqrtf(len_sq);
        return vec3_scale(v, limit / len);
    }
    return v;
}

// Radial shell bias (radialout). Returns the force pulling `my_pos` toward the
// shell of radius `node_rank[idx] * dag_level_distance` centred on the settable
// point `radial_center` (ADR-141 P3). The centre defaults to the world origin
// (0,0,0), which reproduces the legacy DAG-rank behaviour where center gravity
// gathers rank-0 roots near the origin. The per-node shell KEY lives in the
// existing `node_rank` buffer and is re-filled CPU-side per RadialMode (DAG rank,
// node-type tier, or BFS ego-distance). A node with key < 0 (unassigned /
// unreachable) or a disabled bias (dag_bias_k <= 0) gets zero force. The term is
// a Hooke spring on the radial error, so keyed nodes settle onto concentric
// shells (key 0 at the centre, larger keys further out) without disturbing their
// angular placement, which the other forces still own.
__device__ inline float3 dag_radial_bias(float3 my_pos,
                                         const float* __restrict__ node_rank,
                                         int idx) {
    if (node_rank == nullptr || c_params.dag_bias_k <= 0.0f) {
        return make_float3(0.0f, 0.0f, 0.0f);
    }
    const float rank = node_rank[idx];
    if (rank < 0.0f) {
        return make_float3(0.0f, 0.0f, 0.0f);
    }
    const float3 center = make_float3(c_params.radial_center_x,
                                      c_params.radial_center_y,
                                      c_params.radial_center_z);
    const float3 delta = vec3_sub(my_pos, center);
    const float target_radius = rank * c_params.dag_level_distance;
    const float dist = vec3_length(delta);
    if (dist > 1e-6f) {
        // Radial error > 0 ⇒ push outward; < 0 ⇒ pull inward. Direction is the
        // unit vector from the shell centre to the node.
        const float radial_err = target_radius - dist;
        const float scale = radial_err * c_params.dag_bias_k / dist;
        return vec3_scale(delta, scale);
    }
    // Node sits on the root centroid but should live on an outer shell: nudge it
    // off the origin along a deterministic axis so the radial spring can engage.
    if (target_radius > 0.0f) {
        return make_float3(target_radius * c_params.dag_bias_k, 0.0f, 0.0f);
    }
    return make_float3(0.0f, 0.0f, 0.0f);
}

// Stratified-plane bias (ADR-141 P2). Springs each node's Z toward its assigned
// plane (target_z = node_plane[idx] * plane_spacing), separating node types into
// parallel horizontal strata. node_plane[idx] = NaN ⇒ unassigned (no force);
// plane_bias_k <= 0 ⇒ term disabled. plane_spacing <= 0 is also treated as disabled
// so a zero/absent spacing (e.g. an omitted settings field) can never silently
// collapse every plane onto z = 0. Only the Z axis is touched; X/Y placement stays
// owned by the other forces.
__device__ inline float3 stratified_plane_bias(float3 my_pos,
                                               const float* __restrict__ node_plane,
                                               int idx) {
    if (node_plane == nullptr || c_params.plane_bias_k <= 0.0f
        || c_params.plane_spacing <= 0.0f) {
        return make_float3(0.0f, 0.0f, 0.0f);
    }
    const float plane = node_plane[idx];
    if (!isfinite(plane)) {
        return make_float3(0.0f, 0.0f, 0.0f);
    }
    const float target_z = plane * c_params.plane_spacing;
    const float fz = (target_z - my_pos.z) * c_params.plane_bias_k;
    return make_float3(0.0f, 0.0f, fz);
}

// Sugiyama layered bias (ADR-141 P4). Springs each ranked node's Y toward its
// layer y = rank * layer_spacing, giving a top-down layered (Sugiyama) layout.
// rank < 0 (unranked) or layer_bias_k <= 0 disables it. layer_spacing <= 0 is also
// treated as disabled so a zero/omitted spacing can never collapse every rank onto
// Y = 0. Only Y is touched; X/Z placement (and crossing spread) stay owned by the
// other forces.
__device__ inline float3 sugiyama_layer_bias(float3 my_pos,
                                             const float* __restrict__ node_rank,
                                             int idx) {
    if (node_rank == nullptr || c_params.layer_bias_k <= 0.0f
        || c_params.layer_spacing <= 0.0f) {
        return make_float3(0.0f, 0.0f, 0.0f);
    }
    const float rank = node_rank[idx];
    if (rank < 0.0f) {
        return make_float3(0.0f, 0.0f, 0.0f);
    }
    const float target_y = rank * c_params.layer_spacing;
    return make_float3(0.0f, (target_y - my_pos.y) * c_params.layer_bias_k, 0.0f);
}

// CAS-based atomic min for float (maximum portability)
__device__ inline float atomicMinFloat(float* addr, float value) {
    float old = __int_as_float(atomicAdd((int*)addr, 0)); // initial read
    while (value < old) {
        int old_i = __float_as_int(old);
        int assumed = atomicCAS((int*)addr, old_i, __float_as_int(value));
        if (assumed == old_i) break;
        old = __int_as_float(assumed);
    }
    return old;
}

// =============================================================================
// Spatial Grid Kernels
// =============================================================================

__global__ void build_grid_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    int* __restrict__ cell_keys,
    const AABB aabb,
    const int3 grid_dims,
    const float cell_size,
    const int num_nodes,
    // Total cell count = grid_dims.x*y*z. Used as a hard upper bound on the
    // emitted key so a stale/oversized grid dimension can never produce a key
    // that overruns cell_start/cell_end downstream (defence-in-depth against the
    // #81 illegal-memory-access; the host now also guarantees this invariant).
    const int num_grid_cells)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    float3 pos = make_vec3(pos_x[idx], pos_y[idx], pos_z[idx]);

    // Guard against a degenerate cell size (zero/negative/NaN). Without this a
    // bad cell_size yields ±Inf/NaN grid coords; the clamp below would still
    // catch it, but computing on NaN first is wasteful and fragile.
    const float inv_cell = (cell_size > 0.0f) ? (1.0f / cell_size) : 0.0f;

    int grid_x = static_cast<int>((pos.x - aabb.min.x) * inv_cell);
    int grid_y = static_cast<int>((pos.y - aabb.min.y) * inv_cell);
    int grid_z = static_cast<int>((pos.z - aabb.min.z) * inv_cell);

    grid_x = clamp_int(grid_x, 0, grid_dims.x - 1);
    grid_y = clamp_int(grid_y, 0, grid_dims.y - 1);
    grid_z = clamp_int(grid_z, 0, grid_dims.z - 1);

    int key = grid_z * grid_dims.y * grid_dims.x + grid_y * grid_dims.x + grid_x;
    // Final clamp into the valid cell-buffer range [0, num_grid_cells).
    cell_keys[idx] = clamp_int(key, 0, num_grid_cells - 1);
}

__global__ void compute_cell_bounds_kernel(
    const int* __restrict__ sorted_cell_keys,
    int* __restrict__ cell_start,
    int* __restrict__ cell_end,
    const int num_nodes,
    const int num_grid_cells)
{
    // Each thread checks if the cell key for its corresponding node
    // is different from the previous one, indicating a boundary.
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    // The key for the current sorted node
    int current_key = sorted_cell_keys[idx];

    // The key for the previous sorted node (handle boundary case at index 0)
    int prev_key = (idx == 0) ? -1 : sorted_cell_keys[idx - 1];

    // If the key has changed, we've found the end of the previous cell
    // and the start of the current cell.
    if (current_key != prev_key) {
        // Mark the start of the current cell.
        if (current_key >= 0 && current_key < num_grid_cells) {
            cell_start[current_key] = idx;
        }
        // Mark the end of the previous cell.
        if (prev_key >= 0 && prev_key < num_grid_cells) {
            cell_end[prev_key] = idx;
        }
    }

    // The very last node marks the end of its cell.
    if (idx == num_nodes - 1) {
        if (current_key >= 0 && current_key < num_grid_cells) {
            cell_end[current_key] = num_nodes;
        }
    }
}


// =============================================================================
// Force Pass Kernel
// =============================================================================

__global__ void force_pass_kernel(
    const float* __restrict__ pos_in_x,
    const float* __restrict__ pos_in_y,
    const float* __restrict__ pos_in_z,
    float* __restrict__ force_out_x,
    float* __restrict__ force_out_y,
    float* __restrict__ force_out_z,
    const int* __restrict__ cell_start,
    const int* __restrict__ cell_end,
    const int* __restrict__ sorted_node_indices,
    const int* __restrict__ cell_keys,
    const int3 grid_dims,
    const int* __restrict__ edge_row_offsets,
    const int* __restrict__ edge_col_indices,
    const float* __restrict__ edge_weights,
    const int num_nodes,
    const float* __restrict__ d_sssp_dist,
    const ConstraintData* __restrict__ constraints,
    const int num_constraints,
    // Constraint telemetry buffers (optional, can be nullptr)
    float* __restrict__ constraint_violations,   // [num_constraints] violation magnitudes
    float* __restrict__ constraint_energy,       // [num_constraints] energy values
    float* __restrict__ node_constraint_force,   // [num_nodes] total constraint force per node
    // Ontology class metadata for class-based physics
    const int* __restrict__ class_id,            // [num_nodes] OWL class IDs
    const float* __restrict__ class_charge,      // [num_nodes] class-specific charge modifiers
    const float* __restrict__ class_mass,        // [num_nodes] class-specific mass modifiers
    // FA2: node degree buffer for degree-scaled repulsion (nullptr = use classic repulsion)
    const float* __restrict__ node_degrees,      // [num_nodes] sum of incident edge weights
    // Per-population spring strength multiplier (nullptr = uniform 1.0). Lets the
    // Knowledge/Ontology/Agent spring sliders act independently — applied to BOTH
    // the LinLog and Hooke attraction paths so the slider is never inert.
    const float* __restrict__ spring_scale,      // [num_nodes] per-node spring multiplier
    // ADR-070 D3.1 (P2) sparse compute mask (Epic E.4 persona masking).
    // When compute_mask != nullptr, thread t evaluates node compute_mask[t]
    // instead of node t, so hidden nodes cost no force computation. The host
    // builds the mask as visible-nodes ∪ their 1-hop neighbours, so the force
    // field felt by visible nodes stays coherent (ADR-070 §Risks). nullptr /
    // mask_len 0 = evaluate every node (identical to the pre-D3.1 behaviour).
    const int* __restrict__ compute_mask,        // [mask_len] node indices to evaluate
    const int mask_len,
    // PHASE 2 DAG radial bias: per-node hierarchy rank (nullptr = no bias). Rank
    // 0 = root, deeper = further out; < 0 = not in the hierarchy (no bias).
    const float* __restrict__ node_rank,
    // ADR-141 P2 stratified planes: per-node centered plane offset (nullptr =
    // no bias). NaN = node not assigned to any plane (no force).
    const float* __restrict__ node_plane)
{
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (compute_mask != nullptr) {
        if (idx >= mask_len) return;
        idx = compute_mask[idx];
    }
    if (idx < 0 || idx >= num_nodes) return;

    float3 my_pos = make_vec3(pos_in_x[idx], pos_in_y[idx], pos_in_z[idx]);
    float3 total_force = make_vec3(0.0f, 0.0f, 0.0f);

    if (c_params.feature_flags & FeatureFlags::ENABLE_REPULSION) {
        int my_cell_key = cell_keys[idx];
        int grid_x = my_cell_key % grid_dims.x;
        int grid_y = (my_cell_key / grid_dims.x) % grid_dims.y;
        int grid_z = my_cell_key / (grid_dims.x * grid_dims.y);

        // Unroll 3x3x3 neighbor cell iteration for better performance
        #pragma unroll 3
        for (int z = -1; z <= 1; ++z) {
            #pragma unroll 3
            for (int y = -1; y <= 1; ++y) {
                #pragma unroll 3
                for (int x = -1; x <= 1; ++x) {
                    const int neighbor_grid_x = grid_x + x;
                    const int neighbor_grid_y = grid_y + y;
                    const int neighbor_grid_z = grid_z + z;

                    if (neighbor_grid_x >= 0 && neighbor_grid_x < grid_dims.x &&
                        neighbor_grid_y >= 0 && neighbor_grid_y < grid_dims.y &&
                        neighbor_grid_z >= 0 && neighbor_grid_z < grid_dims.z) {

                        const int neighbor_cell_key = neighbor_grid_z * grid_dims.y * grid_dims.x + neighbor_grid_y * grid_dims.x + neighbor_grid_x;
                        const int start = cell_start[neighbor_cell_key];
                        const int end = cell_end[neighbor_cell_key];

                        for (int j = start; j < end; ++j) {
                            const int neighbor_idx = sorted_node_indices[j];
                            if (idx == neighbor_idx) continue;

                            const float3 neighbor_pos = make_vec3(pos_in_x[neighbor_idx], pos_in_y[neighbor_idx], pos_in_z[neighbor_idx]);
                            const float3 diff = vec3_sub(my_pos, neighbor_pos);
                            const float dist_sq = vec3_length_sq(diff);

                            if (dist_sq < c_params.repulsion_cutoff * c_params.repulsion_cutoff && dist_sq > 1e-6f) {
                                const float dist = sqrtf(dist_sq);

                                // Short-range hard separation: when two nodes are
                                // closer than separation_radius, apply a strong
                                // linear push so they cannot overlap. The force
                                // scales with the penetration depth and is capped
                                // by max_force. This consumes c_params.separation_radius
                                // (previously uploaded but never read).
                                if (c_params.separation_radius > 0.0f && dist < c_params.separation_radius) {
                                    const float penetration = (c_params.separation_radius - dist) / c_params.separation_radius;
                                    float sep_force = c_params.max_force * penetration;
                                    sep_force = fminf(sep_force, c_params.max_force);
                                    const float sep_scale = sep_force / dist;
                                    total_force.x = fmaf(diff.x, sep_scale, total_force.x);
                                    total_force.y = fmaf(diff.y, sep_scale, total_force.y);
                                    total_force.z = fmaf(diff.z, sep_scale, total_force.z);
                                }

                                float repulsion;

                                if (c_params.lin_log_mode && node_degrees != nullptr) {
                                    // FA2 degree-scaled repulsion:
                                    // repulsion = scaling_ratio * (deg_i + 1) * (deg_j + 1) / dist
                                    const float deg_i = node_degrees[idx] + 1.0f;
                                    const float deg_j = node_degrees[neighbor_idx] + 1.0f;
                                    repulsion = c_params.scaling_ratio * deg_i * deg_j / (dist + c_params.repulsion_softening_epsilon);
                                } else {
                                    // Classic repulsion with optional domain-aware and charge modulation
                                    repulsion = c_params.repel_k / (dist_sq + c_params.repulsion_softening_epsilon);

                                    // Domain-aware repulsion: same-domain nodes repel less,
                                    // different-domain nodes repel more, creating natural clusters.
                                    if (class_id != nullptr && class_id[idx] != 0 && class_id[neighbor_idx] != 0) {
                                        if (class_id[idx] == class_id[neighbor_idx]) {
                                            // Same domain: reduce repulsion by 90% → tight domain clusters
                                            repulsion *= 0.1f;
                                        } else {
                                            // Different domain: increase repulsion by 200% → strong separation
                                            repulsion *= 3.0f;
                                        }
                                    } else {
                                        // Fallback to charge-based modulation for unclassified nodes
                                        const float my_charge = (class_charge != nullptr) ? class_charge[idx] : 1.0f;
                                        const float neighbor_charge = (class_charge != nullptr) ? class_charge[neighbor_idx] : 1.0f;
                                        repulsion *= my_charge * neighbor_charge;
                                    }
                                }

                                // Prevent repulsion force overflow when nodes are too close
                                repulsion = fminf(repulsion, c_params.max_force);

                                // Safety check for NaN/Inf - use FMA for force accumulation
                                if (isfinite(repulsion) && dist > 0.0f) {
                                    const float force_scale = repulsion / dist;
                                    total_force.x = fmaf(diff.x, force_scale, total_force.x);
                                    total_force.y = fmaf(diff.y, force_scale, total_force.y);
                                    total_force.z = fmaf(diff.z, force_scale, total_force.z);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if (c_params.feature_flags & FeatureFlags::ENABLE_SPRINGS) {
        int start_edge = edge_row_offsets[idx];
        int end_edge = edge_row_offsets[idx + 1];
        
        float du = 0.0f;
        bool use_sssp = (d_sssp_dist != nullptr) &&
                       (c_params.feature_flags & FeatureFlags::ENABLE_SSSP_SPRING_ADJUST);
        if (use_sssp) {
            du = d_sssp_dist[idx];
        }
        
        // Unroll edge iteration for better instruction-level parallelism
        #pragma unroll 4
        for (int i = start_edge; i < end_edge; ++i) {
            const int neighbor_idx = edge_col_indices[i];
            const float3 neighbor_pos = make_vec3(pos_in_x[neighbor_idx], pos_in_y[neighbor_idx], pos_in_z[neighbor_idx]);

            const float3 diff = vec3_sub(neighbor_pos, my_pos);
            const float dist = vec3_length(diff);

            if (dist > 1e-6f) {
                float spring_force_mag;
                // Per-population spring multiplier (1.0 when buffer absent). Applied to
                // both paths so the Knowledge/Ontology/Agent spring sliders are live
                // even in the default LinLog mode (which otherwise ignores spring_k).
                const float s = (spring_scale != nullptr) ? spring_scale[idx] : 1.0f;

                if (c_params.lin_log_mode) {
                    // LinLog attraction: force = log(1 + dist) * edge_weight
                    // No rest length — pure logarithmic attraction.
                    // This makes energy minimization equivalent to modularity maximization.
                    spring_force_mag = log1pf(dist) * edge_weights[i] * s;
                } else {
                    // Classic Hooke's law spring with optional SSSP-adjusted rest length
                    float ideal = c_params.rest_length;
                    if (use_sssp) {
                        const float dv = d_sssp_dist[neighbor_idx];
                        // Handle disconnected components gracefully
                        if (isfinite(du) && isfinite(dv)) {
                            const float delta = fabsf(du - dv);
                            const float norm_delta = fminf(delta, c_params.norm_delta_cap);
                            // Use FMA for ideal distance calculation
                            ideal = fmaf(c_params.sssp_alpha, norm_delta, c_params.rest_length);
                        }
                    }
                    const float displacement = dist - ideal;
                    // Use FMA for spring force calculation
                    spring_force_mag = c_params.spring_k * displacement * edge_weights[i] * s;
                }

                const float force_scale = spring_force_mag / dist;
                total_force.x = fmaf(diff.x, force_scale, total_force.x);
                total_force.y = fmaf(diff.y, force_scale, total_force.y);
                total_force.z = fmaf(diff.z, force_scale, total_force.z);
            }
        }
    }
    
    if (c_params.feature_flags & FeatureFlags::ENABLE_CENTERING) {
        total_force = vec3_sub(total_force, vec3_scale(my_pos, c_params.center_gravity_k));
    }

    // Gravity: gentle center-pull force (additive to centering)
    if (c_params.gravity != 0.0f) {
        total_force.x -= my_pos.x * c_params.gravity;
        total_force.y -= my_pos.y * c_params.gravity;
        total_force.z -= my_pos.z * c_params.gravity;
    }

    // Constraint force accumulation
    float total_constraint_force_magnitude = 0.0f;
    if (c_params.feature_flags & FeatureFlags::ENABLE_CONSTRAINTS) {
        for (int c = 0; c < num_constraints; c++) {
            const ConstraintData& constraint = constraints[c];
            
            // Check if this node is involved in this constraint
            bool is_involved = false;
            int node_role = -1; // Which position in the constraint this node occupies
            for (int n = 0; n < constraint.count && n < 4; n++) {
                if (constraint.node_idx[n] == idx) {
                    is_involved = true;
                    node_role = n;
                    break;
                }
            }
            
            if (!is_involved) continue;
            
            float3 constraint_force = make_vec3(0.0f, 0.0f, 0.0f);
            
            // Calculate progressive activation multiplier
            float progressive_multiplier = 1.0f;
            if (c_params.constraint_ramp_frames > 0) {
                int frames_since_activation = c_params.iteration - constraint.activation_frame;
                if (frames_since_activation >= 0 && frames_since_activation < c_params.constraint_ramp_frames) {
                    // Linear ramp from 0 to 1 over constraint_ramp_frames
                    progressive_multiplier = (float)frames_since_activation / (float)c_params.constraint_ramp_frames;
                    progressive_multiplier = fminf(progressive_multiplier, 1.0f);
                }
            }
            
            // Process constraint based on type
            if (constraint.kind == ConstraintKind::DISTANCE && constraint.count >= 2) {
                // Distance constraint: maintain distance between two nodes
                int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                if (other_idx >= 0 && other_idx < num_nodes) {
                    float3 other_pos = make_vec3(pos_in_x[other_idx], pos_in_y[other_idx], pos_in_z[other_idx]);
                    float3 diff = vec3_sub(my_pos, other_pos);
                    float current_dist = vec3_length(diff);
                    float target_dist = constraint.params[0];
                    
                    if (current_dist > 1e-6f && isfinite(current_dist) && target_dist > 0.0f) {
                        float error = current_dist - target_dist;
                        // Apply progressive activation multiplier to constraint weight
                        float effective_weight = constraint.weight * progressive_multiplier;
                        float force_magnitude = -effective_weight * error;
                        
                        // Cap constraint forces to prevent instability
                        float max_constraint_force = c_params.constraint_max_force_per_node;
                        force_magnitude = fmaxf(-max_constraint_force, fminf(max_constraint_force, force_magnitude));
                        
                        constraint_force = vec3_scale(diff, force_magnitude / current_dist);
                    }
                }
            }
            else if (constraint.kind == ConstraintKind::POSITION && constraint.count >= 1) {
                // Position constraint: attract node to target position
                float3 target_pos = make_vec3(constraint.params[0], constraint.params[1], constraint.params[2]);
                float3 diff = vec3_sub(target_pos, my_pos);
                float distance = vec3_length(diff);
                
                if (distance > 1e-6f && isfinite(distance)) {
                    // Apply progressive activation multiplier to constraint weight
                    float effective_weight = constraint.weight * progressive_multiplier;
                    float force_magnitude = effective_weight * distance * c_params.position_constraint_attraction; // Gentle attraction
                    
                    // Cap constraint forces using per-node force limit
                    float max_constraint_force = c_params.constraint_max_force_per_node;
                    force_magnitude = fminf(force_magnitude, max_constraint_force);
                    
                    constraint_force = vec3_scale(diff, force_magnitude / distance);
                }
            }
            else if (constraint.kind == ConstraintKind::SEPARATION && constraint.count >= 2) {
                // Separation constraint (ADR-098 break #2): ONE-SIDED min-distance
                // push. Disjoint nodes are pushed apart ONLY when they are closer
                // than params[0]; beyond that the force is exactly zero so two
                // unrelated nodes are never pulled together (unlike a DISTANCE
                // spring which would attract at long range).
                int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                if (other_idx >= 0 && other_idx < num_nodes) {
                    float3 other_pos = make_vec3(pos_in_x[other_idx], pos_in_y[other_idx], pos_in_z[other_idx]);
                    float3 diff = vec3_sub(my_pos, other_pos);
                    float current_dist = vec3_length(diff);
                    float min_dist = constraint.params[0];

                    if (current_dist > 1e-6f && isfinite(current_dist) && current_dist < min_dist) {
                        // Penetration depth — only positive inside the clamp radius.
                        float penetration = min_dist - current_dist;
                        float effective_weight = constraint.weight * progressive_multiplier;
                        float force_magnitude = effective_weight * penetration;

                        // Cap to per-node force limit (always repulsive, so clamp to [0, max]).
                        float max_constraint_force = c_params.constraint_max_force_per_node;
                        force_magnitude = fminf(force_magnitude, max_constraint_force);

                        // Push my_pos away from other along the separation axis.
                        constraint_force = vec3_scale(diff, force_magnitude / current_dist);
                    }
                }
            }
            else if (constraint.kind == ConstraintKind::ALIGNMENT && constraint.count >= 2) {
                // Alignment constraint (ADR-141 P4b): pull two constrained nodes
                // onto a shared value on ONE axis (params[0] = 0/1/2 → x/y/z) by
                // springing each toward the pair midpoint. Complements the
                // Sugiyama Y-by-rank soft force for sibling classes.
                int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                if (other_idx >= 0 && other_idx < num_nodes) {
                    float3 other_pos = make_vec3(pos_in_x[other_idx], pos_in_y[other_idx], pos_in_z[other_idx]);
                    int axis = (int)constraint.params[0];
                    float my_a = (axis == 0) ? my_pos.x : (axis == 1) ? my_pos.y : my_pos.z;
                    float other_a = (axis == 0) ? other_pos.x : (axis == 1) ? other_pos.y : other_pos.z;
                    // Pull toward the midpoint (half the error) so both endpoints converge.
                    float err = (other_a - my_a) * 0.5f;
                    float effective_weight = constraint.weight * progressive_multiplier;
                    float fmag = err * effective_weight * c_params.alignment_strength;

                    // Cap to the per-node force limit.
                    float max_constraint_force = c_params.constraint_max_force_per_node;
                    fmag = fmaxf(-max_constraint_force, fminf(max_constraint_force, fmag));

                    if (isfinite(fmag)) {
                        // Force lands ONLY on the aligned axis.
                        constraint_force = make_vec3((axis == 0) ? fmag : 0.0f,
                                                     (axis == 1) ? fmag : 0.0f,
                                                     (axis == 2) ? fmag : 0.0f);
                    }
                }
            }

            // Apply constraint force with safety checks and collect telemetry
            if (isfinite(constraint_force.x) && isfinite(constraint_force.y) && isfinite(constraint_force.z)) {
                total_force = vec3_add(total_force, constraint_force);
                
                // Accumulate constraint force magnitude for this node
                float constraint_force_mag = vec3_length(constraint_force);
                total_constraint_force_magnitude += constraint_force_mag;
                
                // Record constraint-specific telemetry (if buffers provided)
                if (constraint_violations != nullptr && constraint_energy != nullptr) {
                    float violation = 0.0f;
                    float energy = 0.0f;
                    
                    // Calculate violation and energy based on constraint type
                    if (constraint.kind == ConstraintKind::DISTANCE && constraint.count >= 2) {
                        int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                        if (other_idx >= 0 && other_idx < num_nodes) {
                            float3 other_pos = make_vec3(pos_in_x[other_idx], pos_in_y[other_idx], pos_in_z[other_idx]);
                            float3 diff = vec3_sub(my_pos, other_pos);
                            float current_dist = vec3_length(diff);
                            float target_dist = constraint.params[0];
                            
                            violation = fabsf(current_dist - target_dist);
                            energy = 0.5f * constraint.weight * violation * violation; // Quadratic energy
                        }
                    } else if (constraint.kind == ConstraintKind::POSITION && constraint.count >= 1) {
                        float3 target_pos = make_vec3(constraint.params[0], constraint.params[1], constraint.params[2]);
                        float3 diff = vec3_sub(target_pos, my_pos);
                        violation = vec3_length(diff);
                        energy = 0.5f * constraint.weight * violation * violation;
                    } else if (constraint.kind == ConstraintKind::SEPARATION && constraint.count >= 2) {
                        // One-sided: violation is the penetration depth (0 once nodes
                        // are at or beyond the min-distance), energy is its quadratic.
                        int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                        if (other_idx >= 0 && other_idx < num_nodes) {
                            float3 other_pos = make_vec3(pos_in_x[other_idx], pos_in_y[other_idx], pos_in_z[other_idx]);
                            float3 diff = vec3_sub(my_pos, other_pos);
                            float current_dist = vec3_length(diff);
                            float min_dist = constraint.params[0];

                            violation = fmaxf(0.0f, min_dist - current_dist);
                            energy = 0.5f * constraint.weight * violation * violation;
                        }
                    }

                    // Atomically add to constraint telemetry (multiple threads might contribute to same constraint)
                    atomicAdd(&constraint_violations[c], violation);
                    atomicAdd(&constraint_energy[c], energy);
                }
            }
        }
    }

    // PHASE 2: DAG radial hierarchy bias (radialout) — settle ranked nodes onto
    // concentric shells around the root centroid. No-op when dag_bias_k <= 0.
    total_force = vec3_add(total_force, dag_radial_bias(my_pos, node_rank, idx));

    // ADR-141 P2: stratified planes by type — spring each node's Z toward its
    // assigned plane so node types separate into parallel strata. No-op when
    // plane_bias_k <= 0 or the node is unassigned (NaN).
    total_force = vec3_add(total_force, stratified_plane_bias(my_pos, node_plane, idx));

    // ADR-141 P4: Sugiyama Y-by-rank layered spring — pull each ranked node's Y
    // toward its layer (target_y = rank * layer_spacing). No-op when
    // layer_bias_k <= 0 or the node is unranked (rank < 0).
    total_force = vec3_add(total_force, sugiyama_layer_bias(my_pos, node_rank, idx));

    force_out_x[idx] = total_force.x;
    force_out_y[idx] = total_force.y;
    force_out_z[idx] = total_force.z;

    // Record per-node constraint force telemetry
    if (node_constraint_force != nullptr) {
        node_constraint_force[idx] = total_constraint_force_magnitude;
    }
}

// =============================================================================
// SSSP Relaxation Kernel
// =============================================================================

extern "C" __global__ void relaxation_step_kernel(
    float* __restrict__ d_dist,                // [n] distance array
    const int* __restrict__ d_current_frontier,// [frontier_size] active vertices
    int frontier_size,
    const int* __restrict__ d_row_offsets,     // [n+1] CSR row offsets
    const int* __restrict__ d_col_indices,     // [m] CSR column indices  
    const float* __restrict__ d_weights,       // [m] edge weights
    int* __restrict__ d_next_frontier_flags,   // [n] output flags (0/1)
    float B,                                   // distance boundary
    int n                                      // total vertices
) {
    int t = blockIdx.x * blockDim.x + threadIdx.x;
    if (t >= frontier_size) return;
    
    int u = d_current_frontier[t];
    float du = d_dist[u];
    if (!isfinite(du)) return; // Skip unreachable vertices
    
    int start = d_row_offsets[u];
    int end = d_row_offsets[u + 1];
    
    for (int e = start; e < end; ++e) {
        int v = d_col_indices[e];
        float w = d_weights[e];
        float nd = du + w;
        
        if (nd < B) {
            float old = atomicMinFloat(&d_dist[v], nd);
            if (nd < old) {
                d_next_frontier_flags[v] = 1; // Mark for next frontier
            }
        }
    }
}

// =============================================================================
// Cluster Cohesion Kernel
// Applies a gentle attraction toward each cluster's centroid, causing nodes
// in the same cluster to spatially group. Centroids are recomputed every
// N iterations by the host (passed as device buffers).
// =============================================================================

extern "C" __global__ void cluster_cohesion_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    float* __restrict__ force_x,
    float* __restrict__ force_y,
    float* __restrict__ force_z,
    const float* __restrict__ centroid_x,    // [num_clusters] cluster centroids
    const float* __restrict__ centroid_y,
    const float* __restrict__ centroid_z,
    const int* __restrict__ cluster_assignments,  // [num_nodes] cluster ID per node (-1 = unassigned)
    const int num_nodes,
    const int num_clusters,
    const float cohesion_strength)           // 0.005–0.02 recommended
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    int cluster_id = cluster_assignments[idx];
    if (cluster_id < 0 || cluster_id >= num_clusters) return;

    // Vector from node to its cluster centroid
    float dx = centroid_x[cluster_id] - pos_x[idx];
    float dy = centroid_y[cluster_id] - pos_y[idx];
    float dz = centroid_z[cluster_id] - pos_z[idx];

    // Apply as additive force (not velocity) — integrator handles dt
    force_x[idx] += dx * cohesion_strength;
    force_y[idx] += dy * cohesion_strength;
    force_z[idx] += dz * cohesion_strength;
}

// =============================================================================
// Integration Pass Kernel
// =============================================================================

__global__ void integrate_pass_kernel(
    const float* __restrict__ pos_in_x,
    const float* __restrict__ pos_in_y,
    const float* __restrict__ pos_in_z,
    const float* __restrict__ vel_in_x,
    const float* __restrict__ vel_in_y,
    const float* __restrict__ vel_in_z,
    const float* __restrict__ force_x,
    const float* __restrict__ force_y,
    const float* __restrict__ force_z,
    const float* __restrict__ mass,
    float* __restrict__ pos_out_x,
    float* __restrict__ pos_out_y,
    float* __restrict__ pos_out_z,
    float* __restrict__ vel_out_x,
    float* __restrict__ vel_out_y,
    float* __restrict__ vel_out_z,
    const int num_nodes,
    // Ontology class metadata
    const int* __restrict__ class_id,       // [num_nodes] OWL class IDs
    const float* __restrict__ class_charge, // [num_nodes] class-specific charge modifiers
    const float* __restrict__ class_mass,   // [num_nodes] class-specific mass modifiers
    // FA2 adaptive speed: previous-step forces (read this step, will be updated)
    float* __restrict__ prev_force_x,       // [num_nodes] force from prior step (in/out)
    float* __restrict__ prev_force_y,
    float* __restrict__ prev_force_z,
    // Per-node pinned mask (nullptr = nothing pinned). When pinned_mask[idx] != 0
    // the node is HELD: its position/velocity integration is skipped this step so
    // it stays exactly where the host placed it (client drag / anchor). Pinned
    // nodes STILL exert repulsion/spring forces on their neighbours, because the
    // force pass reads pos_in for every node regardless of pin state — only the
    // integration of the pinned node itself is suppressed here.
    const int* __restrict__ pinned_mask)    // [num_nodes] 0 = free, non-0 = pinned
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    // Pinned nodes are held in place: copy position through unchanged, zero the
    // velocity, and reset the FA2 prev-force accumulator so the node re-enters
    // free integration cleanly on unpin (no stale swing/traction memory).
    if (pinned_mask != nullptr && pinned_mask[idx] != 0) {
        pos_out_x[idx] = pos_in_x[idx];
        pos_out_y[idx] = pos_in_y[idx];
        pos_out_z[idx] = pos_in_z[idx];
        vel_out_x[idx] = 0.0f;
        vel_out_y[idx] = 0.0f;
        vel_out_z[idx] = 0.0f;
        if (prev_force_x != nullptr) {
            prev_force_x[idx] = 0.0f;
            prev_force_y[idx] = 0.0f;
            prev_force_z[idx] = 0.0f;
        }
        return;
    }

    float3 pos = make_vec3(pos_in_x[idx], pos_in_y[idx], pos_in_z[idx]);
    float3 vel = make_vec3(vel_in_x[idx], vel_in_y[idx], vel_in_z[idx]);
    float3 force = make_vec3(force_x[idx], force_y[idx], force_z[idx]);

    // Apply class-based mass modifier (default 1.0 if nullptr)
    float class_mass_modifier = (class_mass != nullptr) ? class_mass[idx] : 1.0f;
    float base_mass = (mass != nullptr && mass[idx] > 0.0f) ? mass[idx] : 1.0f;
    float node_mass = base_mass * class_mass_modifier;

    // Force capping using settings values only
    float force_mag = vec3_length(force);
    if (force_mag > c_params.max_force) {
        force = vec3_scale(force, c_params.max_force / force_mag);
    }

    // Use damping exactly as specified in settings
    float effective_damping = c_params.damping;

    // Apply warmup if configured in settings
    if (c_params.iteration < c_params.warmup_iterations) {
        float warmup_factor = (float)c_params.iteration / (float)c_params.warmup_iterations;
        force = vec3_scale(force, warmup_factor);
        // Use cooling_rate from settings for warmup damping adjustment
        effective_damping = c_params.damping + (c_params.cooling_rate - c_params.damping) * (1.0f - warmup_factor);
    }

    if (c_params.adaptive_speed && prev_force_x != nullptr) {
        // FA2 per-node adaptive speed integration:
        //   swing    = |force - prev_force|  (how much force direction changed — oscillation signal)
        //   traction = |force + prev_force| / 2  (how consistent the force is — convergence signal)
        //   speed    = global_speed / (1 + sqrt(swing))  (slow oscillating nodes)
        //
        // Velocity is set directly from force * speed / mass rather than accumulated,
        // which prevents velocity blow-up in FA2-style convergence.
        float3 prev_f = make_vec3(prev_force_x[idx], prev_force_y[idx], prev_force_z[idx]);
        float3 force_diff = vec3_sub(force, prev_f);
        float3 force_sum  = vec3_add(force, prev_f);
        float swing    = vec3_length(force_diff);
        // traction is unused beyond the speed calculation but left for future use
        // float traction = vec3_length(force_sum) * 0.5f;
        float speed = c_params.global_speed / (1.0f + sqrtf(swing));

        // Direct velocity set (FA2 style: no accumulation)
        const float speed_over_mass = speed / node_mass;
        vel.x = force.x * speed_over_mass;
        vel.y = force.y * speed_over_mass;
        vel.z = force.z * speed_over_mass;
        vel = vec3_clamp(vel, c_params.max_velocity);

        // Update prev_force for next iteration
        prev_force_x[idx] = force.x;
        prev_force_y[idx] = force.y;
        prev_force_z[idx] = force.z;
    } else {
        // Classic Verlet integration with settings-based damping using FMA
        const float dt_over_mass = c_params.dt / node_mass;
        vel.x = fmaf(force.x, dt_over_mass, vel.x) * effective_damping;
        vel.y = fmaf(force.y, dt_over_mass, vel.y) * effective_damping;
        vel.z = fmaf(force.z, dt_over_mass, vel.z) * effective_damping;
        vel = vec3_clamp(vel, c_params.max_velocity);
    }

    // Simulated-annealing jitter: per-node random velocity perturbation whose
    // magnitude is proportional to temperature and decays with iteration via
    // cooling_rate. Consumes c_params.temperature (previously uploaded but never
    // read by the kernel). Uses the shared c_params.seed so runs are reproducible.
    if (c_params.temperature > 0.0f) {
        // Exponential anneal: effective_temp = temperature * (1 - cooling_rate)^iteration.
        float decay = 1.0f - c_params.cooling_rate;
        decay = fmaxf(decay, 0.0f);
        float effective_temp = c_params.temperature * powf(decay, (float)c_params.iteration);
        if (effective_temp > 1e-6f) {
            curandState rng;
            // Unique, reproducible per-node/per-iteration sequence.
            unsigned long long seq = (unsigned long long)idx
                + (unsigned long long)c_params.iteration * 2654435761ULL;
            curand_init((unsigned long long)c_params.seed, seq, 0ULL, &rng);
            // Zero-mean jitter in [-effective_temp, +effective_temp] per axis.
            vel.x += (2.0f * curand_uniform(&rng) - 1.0f) * effective_temp;
            vel.y += (2.0f * curand_uniform(&rng) - 1.0f) * effective_temp;
            vel.z += (2.0f * curand_uniform(&rng) - 1.0f) * effective_temp;
            vel = vec3_clamp(vel, c_params.max_velocity);
        }
    }

    // Position update using FMA
    pos.x = fmaf(vel.x, c_params.dt, pos.x);
    pos.y = fmaf(vel.y, c_params.dt, pos.y);
    pos.z = fmaf(vel.z, c_params.dt, pos.z);

    // Soft boundary repulsion, followed by a hard finite-step overshoot clamp.
    // Nodes approaching the boundary get pushed back with increasing strength.
    float boundary_limit = c_params.viewport_bounds;
    if (boundary_limit > 0.0f) {
        // Soft zone starts at 80% of the boundary
        float soft_zone = boundary_limit * 0.8f;
        float inv_zone = 1.0f / fmaxf(boundary_limit - soft_zone, 1.0f);
        float max_boundary_force = c_params.max_force;

        // X axis
        float abs_x = fabsf(pos.x);
        if (abs_x > soft_zone) {
            float penetration = (abs_x - soft_zone) * inv_zone;
            float bf = max_boundary_force * penetration * penetration;
            bf = fminf(bf, max_boundary_force);
            vel.x -= copysignf(bf * c_params.dt, pos.x);
            vel.x *= c_params.boundary_damping;
        }

        // Y axis
        float abs_y = fabsf(pos.y);
        if (abs_y > soft_zone) {
            float penetration = (abs_y - soft_zone) * inv_zone;
            float bf = max_boundary_force * penetration * penetration;
            bf = fminf(bf, max_boundary_force);
            vel.y -= copysignf(bf * c_params.dt, pos.y);
            vel.y *= c_params.boundary_damping;
        }

        // Z axis
        float abs_z = fabsf(pos.z);
        if (abs_z > soft_zone) {
            float penetration = (abs_z - soft_zone) * inv_zone;
            float bf = max_boundary_force * penetration * penetration;
            bf = fminf(bf, max_boundary_force);
            vel.z -= copysignf(bf * c_params.dt, pos.z);
            vel.z *= c_params.boundary_damping;
        }
    }

    // Soft repulsion cannot recover a finite-step overshoot. Bound free nodes
    // at the configured viewport and remove only outward velocity; pinned
    // nodes retain their explicit host position via the early return above.
    if (boundary_limit > 0.0f) {
        if (pos.x > boundary_limit) { pos.x = boundary_limit; vel.x = fminf(vel.x, 0.0f); }
        if (pos.x < -boundary_limit) { pos.x = -boundary_limit; vel.x = fmaxf(vel.x, 0.0f); }
        if (pos.y > boundary_limit) { pos.y = boundary_limit; vel.y = fminf(vel.y, 0.0f); }
        if (pos.y < -boundary_limit) { pos.y = -boundary_limit; vel.y = fmaxf(vel.y, 0.0f); }
        if (pos.z > boundary_limit) { pos.z = boundary_limit; vel.z = fminf(vel.z, 0.0f); }
        if (pos.z < -boundary_limit) { pos.z = -boundary_limit; vel.z = fmaxf(vel.z, 0.0f); }
    }

    pos_out_x[idx] = pos.x;
    pos_out_y[idx] = pos.y;
    pos_out_z[idx] = pos.z;
    vel_out_x[idx] = vel.x;
    vel_out_y[idx] = vel.y;
    vel_out_z[idx] = vel.z;
}

// =============================================================================
// Device-side Frontier Compaction for SSSP
// =============================================================================

__global__ void compact_frontier_kernel(
    const int* __restrict__ flags,          // Input: per-node flags (1 if in frontier)
    int* __restrict__ compacted_frontier,   // Output: compacted frontier
    int* __restrict__ frontier_counter,     // Output: frontier size (atomic counter)
    const int num_nodes)
{
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    
    if (idx < num_nodes && flags[idx] != 0) {
        // Atomically get position in compacted array
        int pos = atomicAdd(frontier_counter, 1);
        compacted_frontier[pos] = idx;
    }
}

} // extern "C"

// =============================================================================
// GPU Memory Management with RAII-style cleanup (C++ template outside extern "C")
// =============================================================================

// RAII wrapper for GPU memory to prevent leaks
template<typename T>
class GPUMemoryRAII {
private:
    T* ptr;
    size_t size;

public:
    GPUMemoryRAII(size_t count) : ptr(nullptr), size(count * sizeof(T)) {
        cudaError_t err = cudaMalloc(&ptr, size);
        if (err != cudaSuccess) {
            printf("GPU allocation failed: %s\n", cudaGetErrorString(err));
            throw std::runtime_error("GPU allocation failed");
        }
    }

    ~GPUMemoryRAII() {
        if (ptr) {
            cudaFree(ptr);
            ptr = nullptr;
        }
    }

    T* get() { return ptr; }
    const T* get() const { return ptr; }

    size_t byte_size() const { return size; }

    // Disable copy constructor and assignment
    GPUMemoryRAII(const GPUMemoryRAII&) = delete;
    GPUMemoryRAII& operator=(const GPUMemoryRAII&) = delete;
};

// =============================================================================
// Thrust Wrapper Functions for Sorting and Scanning
// =============================================================================

extern "C" {

// Wrapper for thrust sort_by_key operation
void thrust_sort_key_value(
    void* d_keys_in,
    void* d_keys_out,
    void* d_values_in, 
    void* d_values_out,
    int num_items,
    cudaStream_t stream
) {
    // Copy input to output first
    cudaMemcpyAsync(d_keys_out, d_keys_in, 
                    num_items * sizeof(int), 
                    cudaMemcpyDeviceToDevice, stream);
    cudaMemcpyAsync(d_values_out, d_values_in,
                    num_items * sizeof(int),
                    cudaMemcpyDeviceToDevice, stream);
    
    // Sort in-place on output buffers
    thrust::device_ptr<int> keys(static_cast<int*>(d_keys_out));
    thrust::device_ptr<int> vals(static_cast<int*>(d_values_out));
    thrust::sort_by_key(thrust::cuda::par.on(stream),
                       keys, keys + num_items, vals);
}

// Wrapper for thrust exclusive_scan operation  
void thrust_exclusive_scan(
    void* d_in,
    void* d_out,
    int num_items,
    cudaStream_t stream
) {
    thrust::device_ptr<int> in_ptr(static_cast<int*>(d_in));
    thrust::device_ptr<int> out_ptr(static_cast<int*>(d_out));
    thrust::exclusive_scan(thrust::cuda::par.on(stream),
                          in_ptr, in_ptr + num_items, 
                          out_ptr, 0); // 0 = initial value
}

// =============================================================================
// K-means Clustering Kernels
// =============================================================================

/**
 * Initialize K-means centroids using K-means++ algorithm
 * Grid: (k, 1, 1), Block: (256, 1, 1) where k = num_clusters
 * Each block initializes one centroid
 */
__global__ void init_centroids_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    float* __restrict__ centroids_x,
    float* __restrict__ centroids_y,
    float* __restrict__ centroids_z,
    float* __restrict__ min_distances,
    int* __restrict__ selected_nodes,
    const int num_nodes,
    const int num_clusters,
    const int current_centroid,
    const unsigned int seed)
{
    const int k = blockIdx.x; // Current centroid index
    const int tid = threadIdx.x;
    const int block_size = blockDim.x;
    
    // Shared memory for reduction operations
    extern __shared__ float shared_data[];
    float* shared_distances = shared_data;
    
    if (current_centroid == 0 && k == 0) {
        // First centroid: select random node
        if (tid == 0) {
            curandState state;
            curand_init(seed, 0, 0, &state);
            int selected = curand(&state) % num_nodes;
            selected_nodes[0] = selected;
            centroids_x[0] = pos_x[selected];
            centroids_y[0] = pos_y[selected];
            centroids_z[0] = pos_z[selected];
        }
        return;
    }
    
    if (k != current_centroid) return; // Only one block processes current centroid
    
    // Calculate distances to nearest existing centroid for all nodes
    for (int node = tid; node < num_nodes; node += block_size) {
        float min_dist = FLT_MAX;
        
        // Find distance to nearest existing centroid
        for (int c = 0; c < current_centroid; c++) {
            float dx = pos_x[node] - centroids_x[c];
            float dy = pos_y[node] - centroids_y[c];
            float dz = pos_z[node] - centroids_z[c];
            float dist = dx * dx + dy * dy + dz * dz;
            min_dist = fminf(min_dist, dist);
        }
        
        min_distances[node] = min_dist;
    }
    
    __syncthreads();
    
    // Sum all squared distances for probability normalization
    float total_dist = 0.0f;
    for (int node = tid; node < num_nodes; node += block_size) {
        total_dist += min_distances[node];
    }
    
    // Block-level reduction to sum distances
    shared_distances[tid] = total_dist;
    __syncthreads();
    
    for (int s = block_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_distances[tid] += shared_distances[tid + s];
        }
        __syncthreads();
    }
    
    if (tid == 0) {
        float total_sum = shared_distances[0];
        
        // Generate random threshold for selection
        curandState state;
        curand_init(seed + current_centroid, 0, 0, &state);
        float threshold = curand_uniform(&state) * total_sum;
        
        // Select node based on probability proportional to squared distance
        float cumulative = 0.0f;
        int selected = 0;
        for (int node = 0; node < num_nodes; node++) {
            cumulative += min_distances[node];
            if (cumulative >= threshold) {
                selected = node;
                break;
            }
        }
        
        selected_nodes[current_centroid] = selected;
        centroids_x[current_centroid] = pos_x[selected];
        centroids_y[current_centroid] = pos_y[selected];
        centroids_z[current_centroid] = pos_z[selected];
    }
}

/**
 * Assign nodes to nearest centroid cluster
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread processes one node
 */
__global__ void assign_clusters_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    const float* __restrict__ centroids_x,
    const float* __restrict__ centroids_y,
    const float* __restrict__ centroids_z,
    int* __restrict__ cluster_assignments,
    float* __restrict__ distances_to_centroid,
    const int num_nodes,
    const int num_clusters)
{
    const int node_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (node_idx >= num_nodes) return;
    
    float node_x = pos_x[node_idx];
    float node_y = pos_y[node_idx];
    float node_z = pos_z[node_idx];
    
    float min_dist = FLT_MAX;
    int best_cluster = 0;
    
    // Find nearest centroid
    for (int k = 0; k < num_clusters; k++) {
        float dx = node_x - centroids_x[k];
        float dy = node_y - centroids_y[k];
        float dz = node_z - centroids_z[k];
        float dist = dx * dx + dy * dy + dz * dz;
        
        if (dist < min_dist) {
            min_dist = dist;
            best_cluster = k;
        }
    }
    
    cluster_assignments[node_idx] = best_cluster;
    distances_to_centroid[node_idx] = sqrtf(min_dist);
}

/**
 * Update centroids based on cluster assignments
 * Grid: (num_clusters, 1, 1), Block: (256, 1, 1)
 * Each block processes one cluster centroid
 */
__global__ void update_centroids_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    const int* __restrict__ cluster_assignments,
    float* __restrict__ centroids_x,
    float* __restrict__ centroids_y,
    float* __restrict__ centroids_z,
    int* __restrict__ cluster_sizes,
    const int num_nodes,
    const int num_clusters)
{
    const int cluster_id = blockIdx.x;
    const int tid = threadIdx.x;
    const int block_size = blockDim.x;
    
    if (cluster_id >= num_clusters) return;
    
    // Shared memory for reduction
    extern __shared__ float shared_mem[];
    float* shared_x = shared_mem;
    float* shared_y = shared_mem + block_size;
    float* shared_z = shared_mem + 2 * block_size;
    int* shared_count = (int*)(shared_mem + 3 * block_size);
    
    // Initialize shared memory
    shared_x[tid] = 0.0f;
    shared_y[tid] = 0.0f;
    shared_z[tid] = 0.0f;
    shared_count[tid] = 0;
    
    // Accumulate positions for nodes assigned to this cluster
    for (int node = tid; node < num_nodes; node += block_size) {
        if (cluster_assignments[node] == cluster_id) {
            shared_x[tid] += pos_x[node];
            shared_y[tid] += pos_y[node];
            shared_z[tid] += pos_z[node];
            shared_count[tid]++;
        }
    }
    
    __syncthreads();
    
    // Block-level reduction
    for (int s = block_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_x[tid] += shared_x[tid + s];
            shared_y[tid] += shared_y[tid + s];
            shared_z[tid] += shared_z[tid + s];
            shared_count[tid] += shared_count[tid + s];
        }
        __syncthreads();
    }
    
    // Update centroid
    if (tid == 0) {
        int count = shared_count[0];
        if (count > 0) {
            centroids_x[cluster_id] = shared_x[0] / count;
            centroids_y[cluster_id] = shared_y[0] / count;
            centroids_z[cluster_id] = shared_z[0] / count;
            cluster_sizes[cluster_id] = count;
        } else {
            // Keep previous centroid if no nodes assigned
            cluster_sizes[cluster_id] = 0;
        }
    }
}

/**
 * Compute inertia (sum of squared distances to centroids)
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each block computes partial inertia, needs reduction afterward
 */
__global__ void compute_inertia_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    const float* __restrict__ centroids_x,
    const float* __restrict__ centroids_y,
    const float* __restrict__ centroids_z,
    const int* __restrict__ cluster_assignments,
    float* __restrict__ partial_inertia,
    const int num_nodes)
{
    const int tid = threadIdx.x;
    const int block_id = blockIdx.x;
    const int block_size = blockDim.x;
    const int start = block_id * block_size;
    const int end = min(start + block_size, num_nodes);
    
    extern __shared__ float shared_inertia[];
    shared_inertia[tid] = 0.0f;
    
    // Compute squared distances for nodes in this block
    for (int node = start + tid; node < end; node += block_size) {
        if (node < num_nodes) {
            int cluster = cluster_assignments[node];
            float dx = pos_x[node] - centroids_x[cluster];
            float dy = pos_y[node] - centroids_y[cluster];
            float dz = pos_z[node] - centroids_z[cluster];
            shared_inertia[tid] += dx * dx + dy * dy + dz * dz;
        }
    }
    
    __syncthreads();
    
    // Block-level reduction
    for (int s = block_size / 2; s > 0; s >>= 1) {
        if (tid < s && tid + s < block_size) {
            shared_inertia[tid] += shared_inertia[tid + s];
        }
        __syncthreads();
    }
    
    // Store partial result
    if (tid == 0) {
        partial_inertia[block_id] = shared_inertia[0];
    }
}

// =============================================================================
// Anomaly Detection Kernels
// =============================================================================

/**
 * Compute Local Outlier Factor (LOF) for anomaly detection
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread processes one node
 */
__global__ void compute_lof_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    const int* __restrict__ sorted_node_indices,
    const int* __restrict__ cell_start,
    const int* __restrict__ cell_end,
    const int* __restrict__ cell_keys,
    const int3 grid_dims,
    float* __restrict__ lof_scores,
    float* __restrict__ local_densities,
    const int num_nodes,
    const int k_neighbors,
    const float radius)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    float3 my_pos = make_vec3(pos_x[idx], pos_y[idx], pos_z[idx]);
    
    // Arrays for k-nearest neighbors (using fixed-size for GPU efficiency)
    const int MAX_K = 32; // Compile-time constant
    float neighbor_dists[MAX_K];
    int neighbor_indices[MAX_K];
    int actual_k = min(k_neighbors, MAX_K);
    
    // Initialize neighbor arrays
    for (int i = 0; i < actual_k; i++) {
        neighbor_dists[i] = FLT_MAX;
        neighbor_indices[i] = -1;
    }
    
    // Get my grid cell
    int my_cell_key = cell_keys[idx];
    int grid_x = my_cell_key % grid_dims.x;
    int grid_y = (my_cell_key / grid_dims.x) % grid_dims.y;
    int grid_z = my_cell_key / (grid_dims.x * grid_dims.y);
    
    // Search neighboring cells for k-nearest neighbors
    for (int z = -1; z <= 1; ++z) {
        for (int y = -1; y <= 1; ++y) {
            for (int x = -1; x <= 1; ++x) {
                int neighbor_grid_x = grid_x + x;
                int neighbor_grid_y = grid_y + y;
                int neighbor_grid_z = grid_z + z;
                
                if (neighbor_grid_x >= 0 && neighbor_grid_x < grid_dims.x &&
                    neighbor_grid_y >= 0 && neighbor_grid_y < grid_dims.y &&
                    neighbor_grid_z >= 0 && neighbor_grid_z < grid_dims.z) {
                    
                    int neighbor_cell_key = neighbor_grid_z * grid_dims.y * grid_dims.x + 
                                          neighbor_grid_y * grid_dims.x + neighbor_grid_x;
                    int start = cell_start[neighbor_cell_key];
                    int end = cell_end[neighbor_cell_key];
                    
                    for (int j = start; j < end; ++j) {
                        int neighbor_idx = sorted_node_indices[j];
                        if (idx == neighbor_idx) continue;
                        
                        float3 neighbor_pos = make_vec3(pos_x[neighbor_idx], pos_y[neighbor_idx], pos_z[neighbor_idx]);
                        float3 diff = vec3_sub(my_pos, neighbor_pos);
                        float dist = vec3_length(diff);
                        
                        if (dist <= radius) {
                            // Insert into k-nearest neighbors if closer than furthest current neighbor
                            if (dist < neighbor_dists[actual_k - 1]) {
                                neighbor_dists[actual_k - 1] = dist;
                                neighbor_indices[actual_k - 1] = neighbor_idx;
                                
                                // Bubble sort to maintain order (small k makes this efficient)
                                for (int i = actual_k - 1; i > 0 && neighbor_dists[i] < neighbor_dists[i-1]; i--) {
                                    float temp_dist = neighbor_dists[i];
                                    int temp_idx = neighbor_indices[i];
                                    neighbor_dists[i] = neighbor_dists[i-1];
                                    neighbor_indices[i] = neighbor_indices[i-1];
                                    neighbor_dists[i-1] = temp_dist;
                                    neighbor_indices[i-1] = temp_idx;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Compute k-distance (distance to k-th nearest neighbor)
    float k_dist = 0.0f;
    int valid_neighbors = 0;
    for (int i = 0; i < actual_k && neighbor_indices[i] != -1; i++) {
        k_dist = neighbor_dists[i]; // Last valid distance is k-distance
        valid_neighbors++;
    }
    
    if (valid_neighbors == 0) {
        lof_scores[idx] = 1.0f; // Normal score for isolated nodes
        local_densities[idx] = 0.0f;
        return;
    }
    
    // Compute local reachability density (LRD)
    float sum_reach_dist = 0.0f;
    for (int i = 0; i < valid_neighbors; i++) {
        // Reachability distance = max(k-distance(neighbor), actual_distance)
        // For simplicity, we approximate neighbor k-distances with current k_dist
        float reach_dist = fmaxf(k_dist, neighbor_dists[i]);
        sum_reach_dist += reach_dist;
    }
    
    float lrd = valid_neighbors / (sum_reach_dist + 1e-6f); // Add epsilon for stability
    local_densities[idx] = lrd;
    
    // Compute LOF by comparing with neighbors' LRDs
    // For GPU efficiency, we approximate neighbors' LRDs
    float lof = 1.0f; // Default normal score
    if (lrd > 1e-6f) {
        float neighbor_lrd_sum = 0.0f;
        
        // Estimate neighbors' LRDs (simplified for GPU performance)
        for (int i = 0; i < valid_neighbors; i++) {
            // Approximate neighbor LRD based on local density estimation
            float approx_neighbor_lrd = valid_neighbors / (neighbor_dists[i] * actual_k + 1e-6f);
            neighbor_lrd_sum += approx_neighbor_lrd;
        }
        
        float avg_neighbor_lrd = neighbor_lrd_sum / valid_neighbors;
        lof = avg_neighbor_lrd / lrd;
    }
    
    // Clamp LOF score for numerical stability
    lof_scores[idx] = fminf(fmaxf(lof, c_params.lof_score_min), c_params.lof_score_max);
}

/**
 * Compute Z-score based anomaly detection
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Requires pre-computed mean and standard deviation
 */
__global__ void compute_zscore_kernel(
    const float* __restrict__ feature_values,
    float* __restrict__ zscore_values,
    const float mean_value,
    const float std_value,
    const int num_nodes)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    float feature = feature_values[idx];
    
    // Compute Z-score with numerical stability
    if (std_value > 1e-6f) {
        float zscore = (feature - mean_value) / std_value;
        // Clamp extreme values for stability
        zscore_values[idx] = fminf(fmaxf(zscore, -10.0f), 10.0f);
    } else {
        // If no variance, all values are normal
        zscore_values[idx] = 0.0f;
    }
}

/**
 * Compute feature statistics (mean, variance) for Z-score calculation
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each block computes partial sums, needs reduction afterward
 */
__global__ void compute_feature_stats_kernel(
    const float* __restrict__ feature_values,
    float* __restrict__ partial_sums,
    float* __restrict__ partial_sq_sums,
    const int num_nodes)
{
    const int tid = threadIdx.x;
    const int block_id = blockIdx.x;
    const int block_size = blockDim.x;
    const int start = block_id * block_size;
    
    extern __shared__ float shared_stats[];
    float* shared_sum = shared_stats;
    float* shared_sq_sum = shared_stats + block_size;
    
    // Initialize shared memory
    shared_sum[tid] = 0.0f;
    shared_sq_sum[tid] = 0.0f;
    
    // Accumulate values for this block
    for (int i = start + tid; i < num_nodes; i += blockDim.x * gridDim.x) {
        if (i < num_nodes) {
            float val = feature_values[i];
            shared_sum[tid] += val;
            shared_sq_sum[tid] += val * val;
        }
    }
    
    __syncthreads();
    
    // Block-level reduction
    for (int s = block_size / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_sum[tid] += shared_sum[tid + s];
            shared_sq_sum[tid] += shared_sq_sum[tid + s];
        }
        __syncthreads();
    }
    
    // Store partial results
    if (tid == 0) {
        partial_sums[block_id] = shared_sum[0];
        partial_sq_sums[block_id] = shared_sq_sum[0];
    }
}

// =============================================================================
// Community Detection Kernels (Label Propagation Algorithm)
// =============================================================================

/**
 * Initialize node labels with unique values
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread initializes one node's label
 */
__global__ void init_labels_kernel(
    int* __restrict__ labels,
    const int num_nodes)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    // Initialize each node with its own unique label (index)
    labels[idx] = idx;
}

/**
 * Synchronous label propagation kernel - all updates happen simultaneously
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread processes one node
 */
__global__ void propagate_labels_sync_kernel(
    const int* __restrict__ labels_in,
    int* __restrict__ labels_out,
    const int* __restrict__ edge_row_offsets,
    const int* __restrict__ edge_col_indices,
    const float* __restrict__ edge_weights,
    int* __restrict__ label_counts,
    const int num_nodes,
    const int max_label,
    curandState* __restrict__ rand_states)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    (void)label_counts; // no global scratch needed; tally is over the local list

    int start_edge = edge_row_offsets[idx];
    int end_edge = edge_row_offsets[idx + 1];

    if (start_edge == end_edge) {
        // Isolated node keeps its current label
        labels_out[idx] = labels_in[idx];
        return;
    }

    // D8/D7 fix — the old kernel histogrammed votes into a per-thread shared
    // array of size (max_label+1). With max_label == num_nodes the launch could
    // not allocate it, so the host silently clamped the shared size and the
    // `<= max_label` guard dropped every label above the clamp. We instead tally
    // the weighted vote for each DISTINCT neighbour label by scanning the node's
    // own (typically small) neighbour list — O(deg^2), ZERO shared memory, and
    // no label is ever out of range or dropped regardless of max_label.
    int best_label = labels_in[idx];
    int best_count = 0;     // weighted vote, fixed-point
    int best_ties = 0;

    for (int a = start_edge; a < end_edge; ++a) {
        const int cand = labels_in[edge_col_indices[a]];
        if (cand < 0 || cand > max_label) continue;

        // First-occurrence dedup: only tally a candidate at its earliest slot so
        // each distinct label is scored once.
        bool seen_earlier = false;
        for (int b = start_edge; b < a; ++b) {
            if (labels_in[edge_col_indices[b]] == cand) { seen_earlier = true; break; }
        }
        if (seen_earlier) continue;

        // Sum the weighted vote for this candidate label across all neighbours.
        int count = 0;
        for (int b = start_edge; b < end_edge; ++b) {
            if (labels_in[edge_col_indices[b]] == cand) {
                count += (int)(edge_weights[b] * c_params.weight_precision_multiplier);
            }
        }

        if (count > best_count) {
            best_count = count;
            best_label = cand;
            best_ties = 1;
        } else if (count == best_count && best_count > 0) {
            best_ties++;
        }
    }

    // Break ties uniformly among the tied labels (deterministic per-node RNG).
    if (best_ties > 1 && best_count > 0) {
        curandState local_state = rand_states[idx];
        int tie_breaker = curand(&local_state) % best_ties;
        rand_states[idx] = local_state;

        int current_tie = 0;
        for (int a = start_edge; a < end_edge; ++a) {
            const int cand = labels_in[edge_col_indices[a]];
            if (cand < 0 || cand > max_label) continue;
            bool seen_earlier = false;
            for (int b = start_edge; b < a; ++b) {
                if (labels_in[edge_col_indices[b]] == cand) { seen_earlier = true; break; }
            }
            if (seen_earlier) continue;
            int count = 0;
            for (int b = start_edge; b < end_edge; ++b) {
                if (labels_in[edge_col_indices[b]] == cand) {
                    count += (int)(edge_weights[b] * c_params.weight_precision_multiplier);
                }
            }
            if (count == best_count) {
                if (current_tie == tie_breaker) { best_label = cand; break; }
                current_tie++;
            }
        }
    }

    labels_out[idx] = best_label;
}

// D8 consolidation — the asynchronous in-place label-propagation kernel
// (`propagate_labels_async_kernel`) was the redundant second community-LPA
// implementation. It carried the same shared-memory histogram defect, produced
// nondeterministic results from its in-place reads, and was never the default
// path (the driver defaults synchronous=true). It is removed so
// `propagate_labels_sync_kernel` above is the single canonical community
// label-propagation kernel. The driver must drop its `synchronous=false`
// branch (see report).

/**
 * Check convergence by comparing old and new labels
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread checks one node, uses atomics for global convergence flag
 */
__global__ void check_convergence_kernel(
    const int* __restrict__ labels_old,
    const int* __restrict__ labels_new,
    int* __restrict__ convergence_flag,
    const int num_nodes)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    // If any label changed, mark as not converged
    if (labels_old[idx] != labels_new[idx]) {
        atomicExch(convergence_flag, 0);
    }
}

/**
 * Initialize random states for tie-breaking in label propagation
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread initializes one random state
 */
__global__ void init_random_states_kernel(
    curandState* __restrict__ rand_states,
    const int num_nodes,
    const unsigned int seed)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    // Initialize random state for this thread
    curand_init(seed + idx, idx, 0, &rand_states[idx]);
}

/**
 * Compute node degrees for modularity calculation
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread computes degree for one node
 */
__global__ void compute_node_degrees_kernel(
    const int* __restrict__ edge_row_offsets,
    const float* __restrict__ edge_weights,
    float* __restrict__ node_degrees,
    const int num_nodes)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    int start_edge = edge_row_offsets[idx];
    int end_edge = edge_row_offsets[idx + 1];
    
    float degree = 0.0f;
    for (int i = start_edge; i < end_edge; ++i) {
        degree += edge_weights[i];
    }
    
    node_degrees[idx] = degree;
}

/**
 * Count community sizes after label propagation
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread processes one node and atomically updates community counts
 */
__global__ void count_community_sizes_kernel(
    const int* __restrict__ labels,
    int* __restrict__ community_sizes,
    const int num_nodes,
    const int max_communities)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    int label = labels[idx];
    if (label >= 0 && label < max_communities) {
        atomicAdd(&community_sizes[label], 1);
    }
}

/**
 * Relabel communities to remove gaps (compact labeling)
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 * Each thread processes one node
 */
__global__ void relabel_communities_kernel(
    int* __restrict__ labels,
    const int* __restrict__ label_mapping,
    const int num_nodes)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    
    int old_label = labels[idx];
    if (old_label >= 0) {
        labels[idx] = label_mapping[old_label];
    }
}

// =============================================================================
// Semantic Force Kernels - Ontology-Based Physics
// =============================================================================

/**
 * Blend semantic forces with physics forces using priority weighting
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 */
__global__ void blend_semantic_physics_forces(
    float* __restrict__ force_x,
    float* __restrict__ force_y,
    float* __restrict__ force_z,
    const float3* __restrict__ semantic_forces,
    const ConstraintData* __restrict__ constraints,
    const int num_constraints,
    const int num_nodes)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    float3 base_force = make_vec3(force_x[idx], force_y[idx], force_z[idx]);
    float3 semantic_force = semantic_forces[idx];

    // Calculate average priority weight from all constraints involving this node
    float total_priority = 0.0f;
    int constraint_count = 0;

    for (int c = 0; c < num_constraints; c++) {
        const ConstraintData& constraint = constraints[c];

        // Check if this node is involved
        for (int n = 0; n < constraint.count && n < 4; n++) {
            if (constraint.node_idx[n] == idx) {
                // Use weight as priority (0-1 scale, but allow up to 10)
                total_priority += constraint.weight;
                constraint_count++;
                break;
            }
        }
    }

    // Blend forces based on priority
    float3 final_force;
    if (constraint_count > 0) {
        float avg_priority = total_priority / constraint_count;
        float priority_weight = fminf(avg_priority / 10.0f, 1.0f);

        // Weighted blend: higher priority = more semantic influence
        final_force = vec3_add(
            vec3_scale(base_force, 1.0f - priority_weight),
            vec3_scale(semantic_force, priority_weight)
        );
    } else {
        // No constraints - use base physics force only
        final_force = base_force;
    }

    // Safety checks
    if (!isfinite(final_force.x)) final_force.x = base_force.x;
    if (!isfinite(final_force.y)) final_force.y = base_force.y;
    if (!isfinite(final_force.z)) final_force.z = base_force.z;

    force_x[idx] = final_force.x;
    force_y[idx] = final_force.y;
    force_z[idx] = final_force.z;
}

// =============================================================================
// GPU Stability Gates - Kinetic Energy Monitoring and Early Exit
// =============================================================================

/**
 * Calculate total kinetic energy across all nodes with block-level reduction
 * Returns partial sums that need final reduction
 * Grid: (ceil(num_nodes/256), 1, 1), Block: (256, 1, 1)
 */
__global__ void calculate_kinetic_energy_kernel(
    const float* __restrict__ vel_x,
    const float* __restrict__ vel_y,
    const float* __restrict__ vel_z,
    const float* __restrict__ mass,
    float* __restrict__ partial_kinetic_energy,
    int* __restrict__ active_node_count,
    const int num_nodes,
    const float min_velocity_threshold)
{
    const int tid = threadIdx.x;
    const int idx = blockIdx.x * blockDim.x + tid;
    
    // Shared memory for block-level reduction
    extern __shared__ float shared_data[];
    float* shared_ke = shared_data;
    int* shared_active = (int*)&shared_data[blockDim.x];
    
    // Initialize shared memory
    shared_ke[tid] = 0.0f;
    shared_active[tid] = 0;
    
    // Calculate kinetic energy for this thread's node
    if (idx < num_nodes) {
        float vx = vel_x[idx];
        float vy = vel_y[idx];
        float vz = vel_z[idx];
        float vel_sq = vx * vx + vy * vy + vz * vz;
        
        // Use stability threshold from parameter
        float min_vel_sq = min_velocity_threshold * min_velocity_threshold;
        
        // Check if node is actively moving
        if (vel_sq > min_vel_sq) {
            float node_mass = (mass != nullptr && mass[idx] > 0.0f) ? mass[idx] : 1.0f;
            shared_ke[tid] = 0.5f * node_mass * vel_sq;
            shared_active[tid] = 1;
        }
    }
    
    __syncthreads();
    
    // Block-level reduction
    for (int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_ke[tid] += shared_ke[tid + s];
            shared_active[tid] += shared_active[tid + s];
        }
        __syncthreads();
    }
    
    // Store block results
    if (tid == 0) {
        partial_kinetic_energy[blockIdx.x] = shared_ke[0];
        atomicAdd(active_node_count, shared_active[0]);
    }
}

/**
 * Final reduction kernel to check system stability
 * Grid: (1, 1, 1), Block: (min(num_blocks, 256), 1, 1)
 */
__global__ void check_system_stability_kernel(
    const float* __restrict__ partial_kinetic_energy,
    const int* __restrict__ active_node_count,
    int* __restrict__ should_skip_physics,
    float* __restrict__ system_kinetic_energy,
    const int num_blocks,
    const int num_nodes,
    const float stability_threshold,
    const int iteration,
    // ADR-070 D2.2: per-node constraint-force magnitude (nullptr = criterion off)
    // and the per-preset epsilon (<= 0 = criterion off). The system is stable
    // ONLY when the KE/motion gate holds AND the largest constraint force is at
    // or below epsilon — a system pinned taut by hierarchy/disjoint constraints
    // is not "at rest" even when kinetic energy has bled off (fixes the
    // false-stable convergence report).
    const float* __restrict__ node_constraint_force,
    const float constraint_force_epsilon)
{
    // Two contiguous shared regions of blockDim.x floats: [0]=KE sum reduction,
    // [blockDim.x]=constraint-force max reduction. Host allocates 2*blockDim*4 B.
    extern __shared__ float sdata[];
    float* shared_ke = sdata;
    float* shared_cf = &sdata[blockDim.x];
    const int tid = threadIdx.x;

    // Load and sum partial kinetic energies
    float sum = 0.0f;
    for (int i = tid; i < num_blocks; i += blockDim.x) {
        sum += partial_kinetic_energy[i];
    }
    shared_ke[tid] = sum;

    // ADR-070 D2.2: local max of the per-node constraint-force magnitude.
    float cf_max = 0.0f;
    if (node_constraint_force != nullptr && constraint_force_epsilon > 0.0f) {
        for (int i = tid; i < num_nodes; i += blockDim.x) {
            float v = node_constraint_force[i];
            // A non-finite constraint force is decisively "not small": bias the
            // reduction to +inf so the constraint gate reports unstable.
            if (!isfinite(v)) {
                cf_max = INFINITY;
            } else if (v > cf_max) {
                cf_max = v;
            }
        }
    }
    shared_cf[tid] = cf_max;

    __syncthreads();

    // Final reduction: sum for KE, max for constraint force.
    for (int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (tid < s) {
            shared_ke[tid] += shared_ke[tid + s];
            shared_cf[tid] = fmaxf(shared_cf[tid], shared_cf[tid + s]);
        }
        __syncthreads();
    }

    // Check stability conditions
    if (tid == 0) {
        float total_ke = shared_ke[0];
        int active_nodes = *active_node_count;

        // Store system kinetic energy for monitoring
        *system_kinetic_energy = total_ke;

        // Calculate average KE per active node
        float avg_ke = (active_nodes > 0) ? (total_ke / active_nodes) : 0.0f;

        // System is stable if:
        // 1. Average KE is below threshold, OR
        // 2. Very few nodes are moving (< 1% of total)
        bool energy_stable = avg_ke < stability_threshold;
        bool motion_stable = active_nodes < max(1, num_nodes / 100);

        // ADR-070 D2.2 third criterion: a taut constraint field vetoes stability.
        bool constraint_stable = true;
        float max_cf = shared_cf[0];
        if (node_constraint_force != nullptr && constraint_force_epsilon > 0.0f) {
            constraint_stable = isfinite(max_cf) && (max_cf <= constraint_force_epsilon);
        }

        *should_skip_physics =
            ((energy_stable || motion_stable) && constraint_stable) ? 1 : 0;

        // Debug output periodically
        if (iteration % 600 == 0 && *should_skip_physics) {
            printf("[GPU Stability Gate] System stable: avg_KE=%.8f, active=%d/%d, max_constraint_force=%.6f\n",
                   avg_ke, active_nodes, num_nodes, max_cf);
        }
    }
}

/**
 * ADR-070 D3.1 (P2) — sparse compute-mask compaction.
 *
 * Compacts a per-node "must compute" flag array into the dense ascending index
 * list the masked force pass consumes, in one pass with a single atomic
 * counter. `include_flags[i] != 0` means node i is either visible under the
 * active persona OR a 1-hop neighbour of a visible node (the ADR §Risks force
 * coherence set); the host marks that set (or a companion neighbour-marking
 * kernel does) before this compaction. `out_count` must be zeroed by the host
 * before launch.
 *
 * Note: atomicAdd assigns slots in nondeterministic order, so the host sorts
 * `compute_mask[0..*out_count]` if a stable/ascending order is required (the
 * host CPU builder `build_compute_mask_with_neighbors` already emits sorted
 * output, so the two producers are interchangeable after that sort).
 *
 * Grid: (ceil(num_nodes/block), 1, 1).
 */
extern "C" __global__ void build_compute_mask_kernel(
    const int* __restrict__ include_flags,   // [num_nodes] 0/1 include flags
    int* __restrict__ compute_mask,          // [num_nodes] out: compacted indices
    int* __restrict__ out_count,             // [1] out: number of masked nodes
    const int num_nodes)
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;
    if (include_flags[idx] != 0) {
        int slot = atomicAdd(out_count, 1);
        compute_mask[slot] = idx;
    }
}

/**
 * Optimized force kernel with integrated stability checking
 * Adds early exit for stable nodes to reduce computation
 */
__global__ void force_pass_with_stability_kernel(
    const float* __restrict__ pos_in_x,
    const float* __restrict__ pos_in_y,
    const float* __restrict__ pos_in_z,
    const float* __restrict__ vel_in_x,
    const float* __restrict__ vel_in_y,
    const float* __restrict__ vel_in_z,
    float* __restrict__ force_out_x,
    float* __restrict__ force_out_y,
    float* __restrict__ force_out_z,
    const int* __restrict__ cell_start,
    const int* __restrict__ cell_end,
    const int* __restrict__ sorted_node_indices,
    const int* __restrict__ cell_keys,
    const int3 grid_dims,
    const int* __restrict__ edge_row_offsets,
    const int* __restrict__ edge_col_indices,
    const float* __restrict__ edge_weights,
    const int num_nodes,
    const float* __restrict__ d_sssp_dist,
    const ConstraintData* __restrict__ constraints,
    const int num_constraints,
    const int* __restrict__ should_skip_all_physics,
    // FA2: node degree buffer for degree-scaled repulsion (nullptr = use classic repulsion)
    const float* __restrict__ node_degrees,      // [num_nodes] sum of incident edge weights
    // Per-population spring strength multiplier (nullptr = uniform 1.0); mirrors
    // force_pass_kernel so the spring sliders behave identically on both paths.
    const float* __restrict__ spring_scale,      // [num_nodes] per-node spring multiplier
    // ADR-070 D2.2: per-node total constraint-force magnitude out (nullptr = skip).
    // The stability check reads this as its third (constraint-force) criterion.
    float* __restrict__ node_constraint_force,
    // ADR-070 D3.1 (P2) sparse compute mask (see force_pass_kernel). nullptr /
    // mask_len 0 = evaluate every node.
    const int* __restrict__ compute_mask,
    const int mask_len,
    // PHASE 2 DAG radial bias: per-node hierarchy rank (nullptr = no bias).
    const float* __restrict__ node_rank,
    // ADR-141 P2 stratified planes: per-node centered plane offset (nullptr =
    // no bias). NaN = node not assigned to any plane (no force).
    const float* __restrict__ node_plane)
{
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (compute_mask != nullptr) {
        if (idx >= mask_len) return;
        idx = compute_mask[idx];
    }
    if (idx < 0 || idx >= num_nodes) return;

    // Global stability check - skip all physics if system is stable
    if (*should_skip_all_physics) {
        force_out_x[idx] = 0.0f;
        force_out_y[idx] = 0.0f;
        force_out_z[idx] = 0.0f;
        if (node_constraint_force != nullptr) node_constraint_force[idx] = 0.0f;
        return;
    }

    // Per-node stability check
    float vx = vel_in_x[idx];
    float vy = vel_in_y[idx];
    float vz = vel_in_z[idx];
    float vel_sq = vx * vx + vy * vy + vz * vz;
    float min_vel_sq = c_params.min_velocity_threshold * c_params.min_velocity_threshold;

    // Skip force calculation for nearly stationary nodes
    if (vel_sq < min_vel_sq) {
        force_out_x[idx] = 0.0f;
        force_out_y[idx] = 0.0f;
        force_out_z[idx] = 0.0f;
        if (node_constraint_force != nullptr) node_constraint_force[idx] = 0.0f;
        return;
    }
    
    // Continue with normal force calculation for moving nodes
    float3 my_pos = make_vec3(pos_in_x[idx], pos_in_y[idx], pos_in_z[idx]);
    float3 total_force = make_vec3(0.0f, 0.0f, 0.0f);
    
    // Repulsion forces (spatial grid)
    if (c_params.feature_flags & FeatureFlags::ENABLE_REPULSION) {
        int my_cell_key = cell_keys[idx];
        int grid_x = my_cell_key % grid_dims.x;
        int grid_y = (my_cell_key / grid_dims.x) % grid_dims.y;
        int grid_z = my_cell_key / (grid_dims.x * grid_dims.y);

        for (int z = -1; z <= 1; ++z) {
            for (int y = -1; y <= 1; ++y) {
                for (int x = -1; x <= 1; ++x) {
                    int neighbor_grid_x = grid_x + x;
                    int neighbor_grid_y = grid_y + y;
                    int neighbor_grid_z = grid_z + z;

                    if (neighbor_grid_x >= 0 && neighbor_grid_x < grid_dims.x &&
                        neighbor_grid_y >= 0 && neighbor_grid_y < grid_dims.y &&
                        neighbor_grid_z >= 0 && neighbor_grid_z < grid_dims.z) {
                        
                        int neighbor_cell_key = neighbor_grid_z * grid_dims.y * grid_dims.x + 
                                              neighbor_grid_y * grid_dims.x + neighbor_grid_x;
                        int start = cell_start[neighbor_cell_key];
                        int end = cell_end[neighbor_cell_key];

                        for (int j = start; j < end; ++j) {
                            int neighbor_idx = sorted_node_indices[j];
                            if (idx == neighbor_idx) continue;

                            float3 neighbor_pos = make_vec3(pos_in_x[neighbor_idx], 
                                                          pos_in_y[neighbor_idx], 
                                                          pos_in_z[neighbor_idx]);
                            float3 diff = vec3_sub(my_pos, neighbor_pos);
                            float dist_sq = vec3_length_sq(diff);

                            if (dist_sq < c_params.repulsion_cutoff * c_params.repulsion_cutoff &&
                                dist_sq > 1e-6f) {
                                float dist = sqrtf(dist_sq);

                                // Short-range hard separation (mirrors force_pass_kernel):
                                // strong linear push when nodes are closer than
                                // separation_radius. Consumes c_params.separation_radius.
                                if (c_params.separation_radius > 0.0f && dist < c_params.separation_radius) {
                                    const float penetration = (c_params.separation_radius - dist) / c_params.separation_radius;
                                    float sep_force = fminf(c_params.max_force * penetration, c_params.max_force);
                                    total_force = vec3_add(total_force, vec3_scale(diff, sep_force / dist));
                                }

                                float repulsion;

                                if (c_params.lin_log_mode && node_degrees != nullptr) {
                                    // FA2 degree-scaled repulsion
                                    const float deg_i = node_degrees[idx] + 1.0f;
                                    const float deg_j = node_degrees[neighbor_idx] + 1.0f;
                                    repulsion = c_params.scaling_ratio * deg_i * deg_j / (dist + c_params.repulsion_softening_epsilon);
                                } else {
                                    repulsion = c_params.repel_k /
                                        (dist_sq + c_params.repulsion_softening_epsilon);
                                }
                                repulsion = fminf(repulsion, c_params.max_force);

                                if (isfinite(repulsion) && isfinite(dist) && dist > 0.0f) {
                                    total_force = vec3_add(total_force, vec3_scale(diff, repulsion / dist));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Spring forces
    if (c_params.feature_flags & FeatureFlags::ENABLE_SPRINGS) {
        int start_edge = edge_row_offsets[idx];
        int end_edge = edge_row_offsets[idx + 1];
        
        float du = 0.0f;
        bool use_sssp = (d_sssp_dist != nullptr) &&
                       (c_params.feature_flags & FeatureFlags::ENABLE_SSSP_SPRING_ADJUST);
        if (use_sssp) {
            du = d_sssp_dist[idx];
        }
        
        for (int i = start_edge; i < end_edge; ++i) {
            int neighbor_idx = edge_col_indices[i];
            float3 neighbor_pos = make_vec3(pos_in_x[neighbor_idx],
                                          pos_in_y[neighbor_idx],
                                          pos_in_z[neighbor_idx]);

            float3 diff = vec3_sub(neighbor_pos, my_pos);
            float dist = vec3_length(diff);

            if (dist > 1e-6f) {
                float spring_force_mag;
                const float s = (spring_scale != nullptr) ? spring_scale[idx] : 1.0f;

                if (c_params.lin_log_mode) {
                    // LinLog: log(1+d) attraction — modularity-equivalent energy minimization
                    spring_force_mag = log1pf(dist) * edge_weights[i] * s;
                } else {
                    float ideal = c_params.rest_length;
                    if (use_sssp) {
                        float dv = d_sssp_dist[neighbor_idx];
                        if (isfinite(du) && isfinite(dv)) {
                            float delta = fabsf(du - dv);
                            float norm_delta = fminf(delta, c_params.norm_delta_cap);
                            ideal = c_params.rest_length + c_params.sssp_alpha * norm_delta;
                        }
                    }
                    float displacement = dist - ideal;
                    spring_force_mag = c_params.spring_k * displacement * edge_weights[i] * s;
                }
                total_force = vec3_add(total_force, vec3_scale(diff, spring_force_mag / dist));
            }
        }
    }

    // Centering force
    if (c_params.feature_flags & FeatureFlags::ENABLE_CENTERING) {
        total_force = vec3_sub(total_force, vec3_scale(my_pos, c_params.center_gravity_k));
    }

    // Constraints processing (if enabled)
    // ADR-070 D2.2: accumulate the per-node constraint-force magnitude so the
    // stability check can gate on it (the third criterion). Mirrors the
    // main-pass `total_constraint_force_magnitude`.
    float total_constraint_force_magnitude = 0.0f;
    if ((c_params.feature_flags & FeatureFlags::ENABLE_CONSTRAINTS) && constraints != nullptr) {
        for (int c = 0; c < num_constraints; c++) {
            const ConstraintData& constraint = constraints[c];

            // Check if this node is involved
            bool is_involved = false;
            int node_role = -1;
            for (int n = 0; n < constraint.count && n < 4; n++) {
                if (constraint.node_idx[n] == idx) {
                    is_involved = true;
                    node_role = n;
                    break;
                }
            }

            if (!is_involved) continue;

            float3 constraint_force = make_vec3(0.0f, 0.0f, 0.0f);

            // Apply constraint forces (simplified for stability example)
            if (constraint.kind == ConstraintKind::DISTANCE && constraint.count >= 2) {
                int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                if (other_idx >= 0 && other_idx < num_nodes) {
                    float3 other_pos = make_vec3(pos_in_x[other_idx],
                                                pos_in_y[other_idx],
                                                pos_in_z[other_idx]);
                    float3 diff = vec3_sub(my_pos, other_pos);
                    float current_dist = vec3_length(diff);
                    float target_dist = constraint.params[0];

                    if (current_dist > 1e-6f && isfinite(current_dist) && target_dist > 0.0f) {
                        float error = current_dist - target_dist;
                        float force_magnitude = -constraint.weight * error;
                        force_magnitude = fmaxf(-c_params.constraint_max_force_per_node,
                                              fminf(c_params.constraint_max_force_per_node, force_magnitude));

                        constraint_force = vec3_scale(diff, force_magnitude / current_dist);
                    }
                }
            }
            else if (constraint.kind == ConstraintKind::SEPARATION && constraint.count >= 2) {
                // Mirror of the main-pass one-sided separation push (ADR-098 break #2)
                // so disjointness still holds while the bad-frame fallback is active.
                int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                if (other_idx >= 0 && other_idx < num_nodes) {
                    float3 other_pos = make_vec3(pos_in_x[other_idx],
                                                pos_in_y[other_idx],
                                                pos_in_z[other_idx]);
                    float3 diff = vec3_sub(my_pos, other_pos);
                    float current_dist = vec3_length(diff);
                    float min_dist = constraint.params[0];

                    if (current_dist > 1e-6f && isfinite(current_dist) && current_dist < min_dist) {
                        float penetration = min_dist - current_dist;
                        float force_magnitude = constraint.weight * penetration;
                        force_magnitude = fminf(force_magnitude, c_params.constraint_max_force_per_node);

                        constraint_force = vec3_scale(diff, force_magnitude / current_dist);
                    }
                }
            }
            else if (constraint.kind == ConstraintKind::ALIGNMENT && constraint.count >= 2) {
                // Mirror of the main-pass one-axis alignment pull (ADR-141 P4b) so
                // sibling alignment still holds while the bad-frame fallback is active.
                int other_idx = (node_role == 0) ? constraint.node_idx[1] : constraint.node_idx[0];
                if (other_idx >= 0 && other_idx < num_nodes) {
                    float3 other_pos = make_vec3(pos_in_x[other_idx],
                                                pos_in_y[other_idx],
                                                pos_in_z[other_idx]);
                    int axis = (int)constraint.params[0];
                    float my_a = (axis == 0) ? my_pos.x : (axis == 1) ? my_pos.y : my_pos.z;
                    float other_a = (axis == 0) ? other_pos.x : (axis == 1) ? other_pos.y : other_pos.z;
                    float err = (other_a - my_a) * 0.5f;
                    float fmag = err * constraint.weight * c_params.alignment_strength;
                    fmag = fmaxf(-c_params.constraint_max_force_per_node,
                                 fminf(c_params.constraint_max_force_per_node, fmag));

                    if (isfinite(fmag)) {
                        constraint_force = make_vec3((axis == 0) ? fmag : 0.0f,
                                                     (axis == 1) ? fmag : 0.0f,
                                                     (axis == 2) ? fmag : 0.0f);
                    }
                }
            }

            if (isfinite(constraint_force.x) && isfinite(constraint_force.y) &&
                isfinite(constraint_force.z)) {
                total_force = vec3_add(total_force, constraint_force);
                total_constraint_force_magnitude += vec3_length(constraint_force);
            }
        }
    }

    // PHASE 2: DAG radial hierarchy bias (radialout). Mirrors force_pass_kernel
    // so the bias applies identically on the stability-gated hot path. No-op when
    // dag_bias_k <= 0 or the node is unranked.
    total_force = vec3_add(total_force, dag_radial_bias(my_pos, node_rank, idx));

    // ADR-141 P2: stratified planes by type. Mirrors force_pass_kernel so the
    // bias applies identically on the stability-gated hot path. No-op when
    // plane_bias_k <= 0 or the node is unassigned (NaN).
    total_force = vec3_add(total_force, stratified_plane_bias(my_pos, node_plane, idx));

    // ADR-141 P4: Sugiyama Y-by-rank layered spring. Mirrors force_pass_kernel so
    // the bias applies identically on the stability-gated hot path. No-op when
    // layer_bias_k <= 0 or the node is unranked (rank < 0).
    total_force = vec3_add(total_force, sugiyama_layer_bias(my_pos, node_rank, idx));

    force_out_x[idx] = total_force.x;
    force_out_y[idx] = total_force.y;
    force_out_z[idx] = total_force.z;

    // ADR-070 D2.2: publish the per-node constraint-force magnitude for the
    // stability third criterion.
    if (node_constraint_force != nullptr) {
        node_constraint_force[idx] = total_constraint_force_magnitude;
    }
}

// =============================================================================
// AABB Reduction Kernel - Computes per-block axis-aligned bounding boxes
// Used for auto-tuning spatial hash grid cell size based on scene extent.
// Each block reduces its portion of positions into a single AABB written to
// block_results[blockIdx.x].  The host then does a final serial reduction.
// =============================================================================
__global__ void compute_aabb_reduction_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    AABB*        __restrict__ block_results,
    const int    num_nodes)
{
    // Shared memory layout: [min_x, min_y, min_z, max_x, max_y, max_z] * blockDim.x
    extern __shared__ float smem[];
    float* s_min_x = smem;
    float* s_min_y = smem + blockDim.x;
    float* s_min_z = smem + 2 * blockDim.x;
    float* s_max_x = smem + 3 * blockDim.x;
    float* s_max_y = smem + 4 * blockDim.x;
    float* s_max_z = smem + 5 * blockDim.x;

    int tid = threadIdx.x;
    int gid = blockIdx.x * blockDim.x + threadIdx.x;

    // Initialize with extreme values
    float local_min_x = FLT_MAX, local_min_y = FLT_MAX, local_min_z = FLT_MAX;
    float local_max_x = -FLT_MAX, local_max_y = -FLT_MAX, local_max_z = -FLT_MAX;

    // Grid-stride loop to handle more nodes than threads
    for (int i = gid; i < num_nodes; i += gridDim.x * blockDim.x) {
        float x = pos_x[i], y = pos_y[i], z = pos_z[i];
        local_min_x = fminf(local_min_x, x);
        local_min_y = fminf(local_min_y, y);
        local_min_z = fminf(local_min_z, z);
        local_max_x = fmaxf(local_max_x, x);
        local_max_y = fmaxf(local_max_y, y);
        local_max_z = fmaxf(local_max_z, z);
    }

    s_min_x[tid] = local_min_x; s_min_y[tid] = local_min_y; s_min_z[tid] = local_min_z;
    s_max_x[tid] = local_max_x; s_max_y[tid] = local_max_y; s_max_z[tid] = local_max_z;
    __syncthreads();

    // Tree reduction in shared memory
    for (unsigned int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (tid < (int)stride) {
            s_min_x[tid] = fminf(s_min_x[tid], s_min_x[tid + stride]);
            s_min_y[tid] = fminf(s_min_y[tid], s_min_y[tid + stride]);
            s_min_z[tid] = fminf(s_min_z[tid], s_min_z[tid + stride]);
            s_max_x[tid] = fmaxf(s_max_x[tid], s_max_x[tid + stride]);
            s_max_y[tid] = fmaxf(s_max_y[tid], s_max_y[tid + stride]);
            s_max_z[tid] = fmaxf(s_max_z[tid], s_max_z[tid + stride]);
        }
        __syncthreads();
    }

    // Thread 0 writes the block result
    if (tid == 0) {
        AABB result;
        result.min = make_float3(s_min_x[0], s_min_y[0], s_min_z[0]);
        result.max = make_float3(s_max_x[0], s_max_y[0], s_max_z[0]);
        block_results[blockIdx.x] = result;
    }
}

// Consumer extent excludes isolated nodes; spatial indexing still uses all nodes.
__global__ void compute_connected_aabb_reduction_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    const float* __restrict__ degree_weight,
    AABB*        __restrict__ block_results,
    const int    num_nodes)
{
    // Shared memory layout: [min_x, min_y, min_z, max_x, max_y, max_z] * blockDim.x
    extern __shared__ float smem[];
    float* s_min_x = smem;
    float* s_min_y = smem + blockDim.x;
    float* s_min_z = smem + 2 * blockDim.x;
    float* s_max_x = smem + 3 * blockDim.x;
    float* s_max_y = smem + 4 * blockDim.x;
    float* s_max_z = smem + 5 * blockDim.x;

    int tid = threadIdx.x;
    int gid = blockIdx.x * blockDim.x + threadIdx.x;

    // Initialize with extreme values
    float local_min_x = FLT_MAX, local_min_y = FLT_MAX, local_min_z = FLT_MAX;
    float local_max_x = -FLT_MAX, local_max_y = -FLT_MAX, local_max_z = -FLT_MAX;

    // Grid-stride loop to handle more nodes than threads
    for (int i = gid; i < num_nodes; i += gridDim.x * blockDim.x) {
        if (!(degree_weight[i] > 0.0f)) continue;
        float x = pos_x[i], y = pos_y[i], z = pos_z[i];
        local_min_x = fminf(local_min_x, x);
        local_min_y = fminf(local_min_y, y);
        local_min_z = fminf(local_min_z, z);
        local_max_x = fmaxf(local_max_x, x);
        local_max_y = fmaxf(local_max_y, y);
        local_max_z = fmaxf(local_max_z, z);
    }

    s_min_x[tid] = local_min_x; s_min_y[tid] = local_min_y; s_min_z[tid] = local_min_z;
    s_max_x[tid] = local_max_x; s_max_y[tid] = local_max_y; s_max_z[tid] = local_max_z;
    __syncthreads();

    // Tree reduction in shared memory
    for (unsigned int stride = blockDim.x / 2; stride > 0; stride >>= 1) {
        if (tid < (int)stride) {
            s_min_x[tid] = fminf(s_min_x[tid], s_min_x[tid + stride]);
            s_min_y[tid] = fminf(s_min_y[tid], s_min_y[tid + stride]);
            s_min_z[tid] = fminf(s_min_z[tid], s_min_z[tid + stride]);
            s_max_x[tid] = fmaxf(s_max_x[tid], s_max_x[tid + stride]);
            s_max_y[tid] = fmaxf(s_max_y[tid], s_max_y[tid + stride]);
            s_max_z[tid] = fmaxf(s_max_z[tid], s_max_z[tid + stride]);
        }
        __syncthreads();
    }

    // Thread 0 writes the block result
    if (tid == 0) {
        AABB result;
        result.min = make_float3(s_min_x[0], s_min_y[0], s_min_z[0]);
        result.max = make_float3(s_max_x[0], s_max_y[0], s_max_z[0]);
        block_results[blockIdx.x] = result;
    }
}

// =============================================================================
// Degree-Weighted Gravity Kernel
// Replaces uniform centering with degree-aware gravity:
// - Hub nodes (high degree) get stronger pull toward center via log(1+degree)
// - Leaf nodes (low degree) get weaker centering, allowing them to orbit hubs
// - Isolated nodes (degree 0) get a peripheral shell force that pushes them
//   to a spherical shell at `peripheral_radius`, keeping them out of the way
//   of the connected graph's community structure.
//
// This kernel runs AFTER the main force_pass_kernel and CORRECTS the uniform
// centering already applied. It:
//   1. Undoes uniform centering: +pos * center_gravity_k
//   2. Applies degree-weighted centering: -pos * center_gravity_k * degree_weight
//   3. For isolated nodes: applies gentle radial force toward peripheral shell
// =============================================================================
__global__ void degree_weighted_gravity_kernel(
    const float* __restrict__ pos_x,
    const float* __restrict__ pos_y,
    const float* __restrict__ pos_z,
    float* __restrict__ force_x,
    float* __restrict__ force_y,
    float* __restrict__ force_z,
    const float* __restrict__ degree_weight,  // [num_nodes] precomputed log(1+degree)
    const int num_nodes,
    const float center_gravity_k,             // same as c_params.center_gravity_k
    const float peripheral_radius,            // target radius for isolated nodes
    const float isolated_spring_k)            // spring constant for isolated shell force
{
    const int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= num_nodes) return;

    float dw = degree_weight[idx];
    float px = pos_x[idx];
    float py = pos_y[idx];
    float pz = pos_z[idx];

    if (dw < 1e-6f) {
        // Isolated node (degree 0): degree_weight == log(1+0) == 0
        // Undo the uniform centering that was already applied
        force_x[idx] += px * center_gravity_k;
        force_y[idx] += py * center_gravity_k;
        force_z[idx] += pz * center_gravity_k;

        // Apply peripheral shell force: spring toward spherical shell
        float dist = sqrtf(px * px + py * py + pz * pz);
        if (dist > 1e-4f) {
            float displacement = dist - peripheral_radius;
            float shell_force = -isolated_spring_k * displacement;
            float inv_dist = 1.0f / dist;
            force_x[idx] += px * inv_dist * shell_force;
            force_y[idx] += py * inv_dist * shell_force;
            force_z[idx] += pz * inv_dist * shell_force;
        }
    } else {
        // Connected node: replace uniform gravity with degree-weighted gravity
        // Undo uniform: +pos * center_gravity_k
        // Apply weighted: -pos * center_gravity_k * dw
        // Net correction: pos * center_gravity_k * (1.0 - dw)
        float correction = center_gravity_k * (1.0f - dw);
        force_x[idx] += px * correction;
        force_y[idx] += py * correction;
        force_z[idx] += pz * correction;
    }
}

} // extern "C"