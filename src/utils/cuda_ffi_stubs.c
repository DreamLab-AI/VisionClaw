// No-op stubs for CUDA FFI symbols when nvcc cannot compile object files
// (e.g. host GCC too new). GPU functions will use PTX JIT at runtime instead.
// These stubs satisfy the linker but return error codes / do nothing.

#include <stdint.h>
#include <stdbool.h>

typedef struct { float x, y, z; } Float3;
typedef struct { /* opaque */ char _pad[256]; } DynamicForceConfigGPU;
typedef struct { /* opaque */ char _pad[512]; } SemanticConfigGPU;

// --- visionflow_unified.cu (thrust_wrapper) ---
void thrust_sort_key_value(const void *k_in, void *k_out, const void *v_in, void *v_out,
                           int n, int key_size, int val_size, void *stream) {}
void thrust_exclusive_scan(const void *in, void *out, int n, void *stream) {}

// --- semantic_forces.cu ---
void set_semantic_config(const SemanticConfigGPU *config) {}
void apply_dag_force(const int *levels, const int *types, Float3 *pos, Float3 *forces, int n) {}
void apply_type_cluster_force(const int *types, const Float3 *centroids, Float3 *pos, Float3 *forces, int n, int nt) {}
void apply_collision_force(const float *radii, Float3 *pos, Float3 *forces, int n) {}
void apply_attribute_spring_force(const int *src, const int *tgt, const float *w, const int *et, Float3 *pos, Float3 *forces, int ne) {}
void apply_dynamic_relationship_force(const int *src, const int *tgt, const int *et, const int *cross, Float3 *pos, Float3 *forces, int ne) {}
void apply_physicality_cluster_force(const int *phys, const Float3 *centroids, Float3 *pos, Float3 *forces, int n) {}
void apply_role_cluster_force(const int *role, const Float3 *centroids, Float3 *pos, Float3 *forces, int n) {}
void apply_maturity_layout_force(const int *mat, Float3 *pos, Float3 *forces, int n) {}
void calculate_physicality_centroids(const int *phys, const Float3 *pos, Float3 *centroids, int *counts, int n) {}
void finalize_physicality_centroids(Float3 *centroids, const int *counts) {}
void calculate_role_centroids(const int *role, const Float3 *pos, Float3 *centroids, int *counts, int n) {}
void finalize_role_centroids(Float3 *centroids, const int *counts) {}
void calculate_type_centroids(const int *types, const Float3 *pos, Float3 *centroids, int *counts, int n, int nt) {}
void finalize_type_centroids(Float3 *centroids, const int *counts, int nt) {}
void calculate_hierarchy_levels(const int *src, const int *tgt, const int *et, int *levels, bool *changed, int ne, int nn) {}

// --- pagerank.cu ---
void pagerank_init(float *pr, int n, void *stream) {}
void pagerank_iterate(const int *row_off, const int *col_idx, const float *vals,
                      const float *pr_in, float *pr_out, int n, float damping, void *stream) {}
void pagerank_iterate_optimized(const int *row_off, const int *col_idx, const float *vals,
                                const float *pr_in, float *pr_out, int n, float damping, void *stream) {}
void pagerank_check_convergence(const float *pr_old, const float *pr_new, float *diff, int n, void *stream) {}
void pagerank_handle_dangling(const int *dangling, int nd, float dsum, float *pr, int n, float damping, void *stream) {}

// --- gpu_connected_components.cu ---
void compute_connected_components_gpu(const int *row_off, const int *col_idx, int *labels, int *num_comp, int n, void *stream) {}

// --- kernel_bridge.rs FFI ---
int set_dynamic_relationship_buffer(const DynamicForceConfigGPU *configs, int num_types, bool enabled) { return 0; }
int update_dynamic_relationship_config(int type_id, const DynamicForceConfigGPU *config) { return 0; }
int set_dynamic_relationships_enabled(bool enabled) { return 0; }
int get_dynamic_relationship_buffer_version(void) { return 0; }
int get_max_relationship_types(void) { return 32; }
