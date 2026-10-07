//! Core types, FFI declarations, and constants for the unified GPU compute module.

use cust_core::DeviceCopy;
use std::collections::HashMap;

// Opaque type for curandState (CUDA random number generator state)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct curandState {
    pub(crate) _private: [u8; 48],
}

// SAFETY: curandState is safe to implement DeviceCopy because:
// 1. It is repr(C) ensuring a stable memory layout compatible with CUDA
// 2. The struct contains only plain bytes with no pointers or references
// 3. The CUDA runtime treats this as opaque state that can be safely memcpy'd
// 4. The 48-byte size matches the curandState size in the CUDA runtime headers
unsafe impl DeviceCopy for curandState {}

// GPU Performance Metrics tracking structure
#[derive(Debug, Clone)]
pub struct GPUPerformanceMetrics {
    pub kernel_times: HashMap<String, Vec<f32>>,
    pub total_kernel_calls: HashMap<String, u64>,

    pub total_memory_allocated: usize,
    pub peak_memory_usage: usize,
    pub current_memory_usage: usize,

    pub force_kernel_avg_time: f32,
    pub integrate_kernel_avg_time: f32,
    pub grid_build_avg_time: f32,
    pub sssp_avg_time: f32,
    pub clustering_avg_time: f32,
    pub anomaly_detection_avg_time: f32,
    pub community_detection_avg_time: f32,

    pub gpu_utilization_percent: f32,
    pub memory_bandwidth_utilization: f32,

    pub frames_per_second: f32,
    pub total_simulation_time: f32,
    pub last_frame_time: f32,
}

impl Default for GPUPerformanceMetrics {
    fn default() -> Self {
        Self {
            kernel_times: HashMap::new(),
            total_kernel_calls: HashMap::new(),
            total_memory_allocated: 0,
            peak_memory_usage: 0,
            current_memory_usage: 0,
            force_kernel_avg_time: 0.0,
            integrate_kernel_avg_time: 0.0,
            grid_build_avg_time: 0.0,
            sssp_avg_time: 0.0,
            clustering_avg_time: 0.0,
            anomaly_detection_avg_time: 0.0,
            community_detection_avg_time: 0.0,
            gpu_utilization_percent: 0.0,
            memory_bandwidth_utilization: 0.0,
            frames_per_second: 0.0,
            total_simulation_time: 0.0,
            last_frame_time: 0.0,
        }
    }
}

// External CUDA/Thrust function for sorting
// This is provided by the compiled CUDA object file
//
// SAFETY: This extern block declares FFI functions that are safe to call when:
// 1. All device pointers (d_keys_in, d_keys_out, d_values_in, d_values_out) are valid
//    CUDA device memory pointers allocated via cudaMalloc or DeviceBuffer
// 2. The pointers have sufficient allocated size for num_items elements
// 3. The stream pointer is a valid CUDA stream handle or null for default stream
// 4. The caller ensures proper synchronization before reading output buffers
unsafe extern "C" {
    pub(crate) fn thrust_sort_key_value(
        d_keys_in: *const ::std::os::raw::c_void,
        d_keys_out: *mut ::std::os::raw::c_void,
        d_values_in: *const ::std::os::raw::c_void,
        d_values_out: *mut ::std::os::raw::c_void,
        num_items: ::std::os::raw::c_int,
        stream: *mut ::std::os::raw::c_void,
    );
}

// Define AABB and int3 structs to match CUDA
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, DeviceCopy)]
pub(crate) struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

// SAFETY: AABB is safe to implement Zeroable because:
// 1. It is repr(C) with a deterministic memory layout
// 2. All fields are f32 arrays which have valid zero representations
// 3. An AABB with all zeros (min=[0,0,0], max=[0,0,0]) is a valid degenerate bounding box
unsafe impl bytemuck::Zeroable for Aabb {}

// SAFETY: AABB is safe to implement Pod because:
// 1. It is repr(C) ensuring no padding or alignment surprises
// 2. All fields are f32 which is itself Pod (plain old data)
// 3. The struct has no invariants that could be violated by arbitrary bit patterns
// 4. Any bit pattern can be safely interpreted as an AABB (may represent invalid geometry but won't cause UB)
unsafe impl bytemuck::Pod for Aabb {}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, DeviceCopy)]
pub(crate) struct int3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum ComputeMode {
    Basic,
    DualGraph,
    Advanced,
    Constraints,
}

// Additional Thrust wrapper function for scanning
//
// SAFETY: This extern block declares the thrust_exclusive_scan FFI function.
// The function is safe to call when:
// 1. d_in is a valid CUDA device pointer to at least num_items elements
// 2. d_out is a valid CUDA device pointer to at least num_items elements
// 3. d_in and d_out may alias (in-place scan is supported)
// 4. num_items is a non-negative count of elements to scan
// 5. stream is a valid CUDA stream handle or null for default stream
// 6. The caller ensures synchronization before reading d_out
#[allow(dead_code)]
unsafe extern "C" {
    pub(crate) fn thrust_exclusive_scan(
        d_in: *const ::std::os::raw::c_void,
        d_out: *mut ::std::os::raw::c_void,
        num_items: ::std::os::raw::c_int,
        stream: *mut ::std::os::raw::c_void,
    );
}

// PageRank GPU kernel FFI functions from pagerank.cu
//
// SAFETY: These extern functions are safe to call when:
// 1. All device pointers (pagerank, pagerank_old, pagerank_new, etc.) are valid
//    CUDA device memory pointers with sufficient allocation for num_nodes elements
// 2. row_offsets has num_nodes+1 elements in CSR format
// 3. col_indices and out_degree have appropriate sizes matching the CSR structure
// 4. num_nodes > 0
// 5. stream is a valid CUDA stream handle or null for default stream
// 6. The caller ensures proper synchronization before reading output buffers
unsafe extern "C" {
    pub(crate) fn pagerank_init(
        pagerank: *mut f32,
        num_nodes: ::std::os::raw::c_int,
        stream: *mut ::std::os::raw::c_void,
    );

    pub(crate) fn pagerank_iterate(
        pagerank_old: *const f32,
        pagerank_new: *mut f32,
        row_offsets: *const ::std::os::raw::c_int,
        col_indices: *const ::std::os::raw::c_int,
        out_degree: *const ::std::os::raw::c_int,
        num_nodes: ::std::os::raw::c_int,
        damping: f32,
        stream: *mut ::std::os::raw::c_void,
    );

    pub(crate) fn pagerank_iterate_optimized(
        pagerank_old: *const f32,
        pagerank_new: *mut f32,
        row_offsets: *const ::std::os::raw::c_int,
        col_indices: *const ::std::os::raw::c_int,
        out_degree: *const ::std::os::raw::c_int,
        num_nodes: ::std::os::raw::c_int,
        damping: f32,
        stream: *mut ::std::os::raw::c_void,
    );

    pub(crate) fn pagerank_check_convergence(
        pagerank_old: *const f32,
        pagerank_new: *const f32,
        diff_buffer: *mut f32,
        num_nodes: ::std::os::raw::c_int,
        stream: *mut ::std::os::raw::c_void,
    ) -> f32;

    pub(crate) fn pagerank_handle_dangling_global(
        pagerank_new: *mut f32,
        pagerank_old: *const f32,
        out_degree: *const ::std::os::raw::c_int,
        num_nodes: ::std::os::raw::c_int,
        damping: f32,
        stream: *mut ::std::os::raw::c_void,
    );
}

// Connected components GPU kernel FFI function from gpu_connected_components.cu
//
// SAFETY: This extern function is safe to call when:
// 1. edge_row_offsets points to a valid device buffer of [num_nodes + 1] i32
//    values in CSR format (monotonically non-decreasing)
// 2. edge_col_indices points to a valid device buffer of [num_edges] i32 values
//    where num_edges = edge_row_offsets[num_nodes]
// 3. labels points to a valid device buffer of [num_nodes] i32 values (output)
// 4. num_components points to a valid device allocation for a single i32 (output)
// 5. num_nodes > 0 and matches the graph size used to construct the CSR arrays
// 6. max_iterations > 0
// 7. stream is a valid CUDA stream handle or null for default stream
unsafe extern "C" {
    pub(crate) fn compute_connected_components_gpu(
        edge_row_offsets: *const ::std::os::raw::c_int,
        edge_col_indices: *const ::std::os::raw::c_int,
        labels: *mut ::std::os::raw::c_int,
        num_components: *mut ::std::os::raw::c_int,
        num_nodes: ::std::os::raw::c_int,
        max_iterations: ::std::os::raw::c_int,
        stream: *mut ::std::os::raw::c_void,
    );
}

/// K-means result: per-node cluster assignment, cluster centroids and final
/// inertia.
pub type GpuKMeansOutput = (Vec<i32>, Vec<(f32, f32, f32)>, f32);

/// K-means result with run metrics: assignments, centroids, final inertia,
/// iterations performed and whether it converged.
pub type GpuKMeansMetricsOutput = (Vec<i32>, Vec<(f32, f32, f32)>, f32, u32, bool);

/// Community-detection result: per-node community label, community count,
/// modularity, iterations performed, compact community sizes and whether it
/// converged.
pub type GpuCommunityOutput = (Vec<i32>, usize, f32, u32, Vec<i32>, bool);
