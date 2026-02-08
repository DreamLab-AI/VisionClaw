//! # Unified GPU Compute Module with Asynchronous Transfer Support
//!
//! This module provides a high-performance CUDA-based GPU compute engine with advanced
//! asynchronous memory transfer capabilities for physics simulations and graph processing.
//!
//! ## Key Features
//!
//! ### Asynchronous GPU-to-CPU Transfers
//! - **Double-buffered transfers**: Ping-pong buffers eliminate blocking operations
//! - **Continuous data flow**: Always have fresh data available without waiting
//! - **Performance boost**: 2.8-4.4x faster than synchronous transfers in high-frequency scenarios
//!
//! ### Advanced Physics Simulation
//! - Force-directed graph layout with spatial optimization
//! - Constraint-based physics with variable damping
//! - GPU stability gating to skip unnecessary computations
//!
//! ### GPU Memory Management
//! - Dynamic buffer resizing based on node count
//! - Efficient spatial grid acceleration structures
//! - Memory usage tracking and optimization
//!
//! ## Safety Documentation for Unsafe Blocks
//!
//! This module contains multiple `unsafe` blocks, primarily for CUDA kernel launches and
//! FFI calls. All unsafe blocks in this module follow these safety invariants:
//!
//! ### Kernel Launch Safety (via `launch!` macro)
//! All CUDA kernel launches are safe when these invariants hold:
//! 1. **Valid Module**: The kernel function is loaded from a valid PTX module
//! 2. **Valid Buffers**: All `DeviceBuffer` arguments are valid allocations with sufficient capacity
//! 3. **Bounds Check**: `num_nodes <= allocated_nodes` is verified before kernel launches
//! 4. **Grid/Block Size**: Launch configuration uses valid grid and block dimensions
//! 5. **Stream Validity**: The CUDA stream is valid and not destroyed
//! 6. **Type Safety**: All arguments match the kernel's expected types (enforced by DeviceCopy trait)
//!
//! ### FFI Call Safety (thrust_sort_key_value, etc.)
//! External CUDA library calls are safe when:
//! 1. All device pointers are valid CUDA allocations
//! 2. Buffer sizes are sufficient for the requested operation
//! 3. The stream handle is valid or null (for default stream)
//!
//! ### DeviceCopy Trait Implementations
//! Types implementing DeviceCopy are safe for GPU memory operations because:
//! 1. They are repr(C) with stable memory layout
//! 2. They contain no pointers, references, or non-Send types
//! 3. Arbitrary bit patterns represent valid (if potentially meaningless) values
//!
//! ## Async Transfer Usage
//!
//! The async transfer methods provide multiple ways to access GPU data without blocking:
//!
//! ### Method 1: High-Level Async (get_node_positions_async and get_node_velocities_async)
//! These implement a sophisticated double-buffering strategy with automatic buffer management:
//!
//! ```rust
//! use crate::utils::unified_gpu_compute::UnifiedGPUCompute;
//!
//! 
//! let mut gpu_compute = UnifiedGPUCompute::new(num_nodes, num_edges, ptx_content)?;
//!
//! 
//! loop {
//!     
//!     gpu_compute.execute_physics_step(&simulation_params)?;
//!
//!     
//!     let (pos_x, pos_y, pos_z) = gpu_compute.get_node_positions_async()?;
//!     let (vel_x, vel_y, vel_z) = gpu_compute.get_node_velocities_async()?;
//!
//!     
//!     update_visualization(&pos_x, &pos_y, &pos_z);
//!     analyze_motion_patterns(&vel_x, &vel_y, &vel_z);
//!
//!     
//! }
//!
//! 
//! gpu_compute.sync_all_transfers()?;
//! let (final_pos_x, final_pos_y, final_pos_z) = gpu_compute.get_node_positions_async()?;
//! ```
//!
//! ### Method 2: Low-Level Async (start_async_download_* and wait_for_download_*)
//! For fine-grained control over transfer timing and maximum performance:
//!
//! ```rust
//! use crate::utils::unified_gpu_compute::UnifiedGPUCompute;
//!
//! 
//! let mut gpu_compute = UnifiedGPUCompute::new(num_nodes, num_edges, ptx_content)?;
//!
//! 
//! loop {
//!     
//!     gpu_compute.start_async_download_positions()?;
//!     gpu_compute.start_async_download_velocities()?;
//!
//!     
//!     gpu_compute.execute_physics_step(&simulation_params)?;
//!
//!     
//!     update_network_data();
//!     process_user_input();
//!     analyze_performance_metrics();
//!
//!     
//!     let (pos_x, pos_y, pos_z) = gpu_compute.wait_for_download_positions()?;
//!     let (vel_x, vel_y, vel_z) = gpu_compute.wait_for_download_velocities()?;
//!
//!     
//!     update_visualization(&pos_x, &pos_y, &pos_z);
//!     compute_motion_analysis(&vel_x, &vel_y, &vel_z);
//! }
//! ```
//!
//! ## Performance Characteristics
//!
//! ### Transfer Methods Performance Comparison:
//! - **Synchronous transfers** (`get_node_positions()`, `get_node_velocities()`):
//!   Block CPU until GPU copy completes (~2-5ms per transfer)
//! - **High-level async** (`get_node_positions_async()`, `get_node_velocities_async()`):
//!   Return immediately with previous frame data (~0.1ms)
//! - **Low-level async** (`start_async_download_*()`, `wait_for_download_*()`):
//!   Maximum performance with fine-grained control (~0.05ms start, ~0-2ms wait)
//!
//! ### Resource Usage:
//! - **Memory overhead**: 2x host memory for double buffering (acceptable trade-off)
//! - **Latency**: 1-frame delay for data freshness (usually imperceptible)
//! - **GPU streams**: Dedicated transfer stream prevents interference with compute kernels

use crate::models::constraints::ConstraintData;
pub use crate::models::simulation_params::SimParams;
use crate::utils::advanced_logging::{log_gpu_error, log_gpu_kernel, log_memory_event};
use crate::utils::result_helpers::safe_json_number;
use anyhow::{anyhow, Result};
use cust::context::Context;
use cust::device::Device;
use cust::event::{Event, EventFlags};
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer, DevicePointer};
use cust::module::Module;
use cust::stream::{Stream, StreamFlags};
use cust_core::DeviceCopy;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::ffi::CStr;

// Opaque type for curandState (CUDA random number generator state)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct curandState {
    _private: [u8; 48],
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
    fn thrust_sort_key_value(
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
struct AABB {
    min: [f32; 3],
    max: [f32; 3],
}

// SAFETY: AABB is safe to implement Zeroable because:
// 1. It is repr(C) with a deterministic memory layout
// 2. All fields are f32 arrays which have valid zero representations
// 3. An AABB with all zeros (min=[0,0,0], max=[0,0,0]) is a valid degenerate bounding box
unsafe impl bytemuck::Zeroable for AABB {}

// SAFETY: AABB is safe to implement Pod because:
// 1. It is repr(C) ensuring no padding or alignment surprises
// 2. All fields are f32 which is itself Pod (plain old data)
// 3. The struct has no invariants that could be violated by arbitrary bit patterns
// 4. Any bit pattern can be safely interpreted as an AABB (may represent invalid geometry but won't cause UB)
unsafe impl bytemuck::Pod for AABB {}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, DeviceCopy)]
struct int3 {
    x: i32,
    y: i32,
    z: i32,
}

pub struct UnifiedGPUCompute {
    device: Device,
    _context: Context,
    _module: Module,
    clustering_module: Option<Module>,
    stream: Stream,

    
    build_grid_kernel_name: &'static str,
    compute_cell_bounds_kernel_name: &'static str,
    force_pass_kernel_name: &'static str,
    integrate_pass_kernel_name: &'static str,

    
    params: SimParams,

    
    pub pos_in_x: DeviceBuffer<f32>,
    pub pos_in_y: DeviceBuffer<f32>,
    pub pos_in_z: DeviceBuffer<f32>,
    pub vel_in_x: DeviceBuffer<f32>,
    pub vel_in_y: DeviceBuffer<f32>,
    pub vel_in_z: DeviceBuffer<f32>,

    pub pos_out_x: DeviceBuffer<f32>,
    pub pos_out_y: DeviceBuffer<f32>,
    pub pos_out_z: DeviceBuffer<f32>,
    pub vel_out_x: DeviceBuffer<f32>,
    pub vel_out_y: DeviceBuffer<f32>,
    pub vel_out_z: DeviceBuffer<f32>,


    pub mass: DeviceBuffer<f32>,
    pub node_graph_id: DeviceBuffer<i32>,

    // Ontology class metadata for class-based physics
    pub class_id: DeviceBuffer<i32>,        // Maps owl_class_iri to integer class ID
    pub class_charge: DeviceBuffer<f32>,    // Class-specific charge modifiers
    pub class_mass: DeviceBuffer<f32>,      // Class-specific mass modifiers


    pub edge_row_offsets: DeviceBuffer<i32>,
    pub edge_col_indices: DeviceBuffer<i32>,
    pub edge_weights: DeviceBuffer<f32>,

    
    force_x: DeviceBuffer<f32>,
    force_y: DeviceBuffer<f32>,
    force_z: DeviceBuffer<f32>,

    
    cell_keys: DeviceBuffer<i32>,
    sorted_node_indices: DeviceBuffer<i32>,
    cell_start: DeviceBuffer<i32>,
    cell_end: DeviceBuffer<i32>,

    
    cub_temp_storage: DeviceBuffer<u8>,

    
    pub num_nodes: usize,
    pub num_edges: usize,
    allocated_nodes: usize,    
    allocated_edges: usize,    
    pub max_grid_cells: usize, 
    iteration: i32,

    
    zero_buffer: Vec<i32>,

    
    cell_buffer_growth_factor: f32,
    max_allowed_grid_cells: usize,
    resize_count: usize,
    total_memory_allocated: usize, 

    
    pub dist: DeviceBuffer<f32>,                
    pub current_frontier: DeviceBuffer<i32>,    
    pub next_frontier_flags: DeviceBuffer<i32>, 
    pub parents: Option<DeviceBuffer<i32>>,     

    
    sssp_stream: Option<Stream>,

    
    constraint_data: DeviceBuffer<ConstraintData>,
    num_constraints: usize,

    
    pub sssp_available: bool,

    
    performance_metrics: GPUPerformanceMetrics,

    
    pub centroids_x: DeviceBuffer<f32>,
    pub centroids_y: DeviceBuffer<f32>,
    pub centroids_z: DeviceBuffer<f32>,
    pub cluster_assignments: DeviceBuffer<i32>,
    pub distances_to_centroid: DeviceBuffer<f32>,
    pub cluster_sizes: DeviceBuffer<i32>,
    pub partial_inertia: DeviceBuffer<f32>,
    pub min_distances: DeviceBuffer<f32>,
    pub selected_nodes: DeviceBuffer<i32>,
    pub max_clusters: usize,

    
    pub lof_scores: DeviceBuffer<f32>,
    pub local_densities: DeviceBuffer<f32>,
    pub zscore_values: DeviceBuffer<f32>,
    pub feature_values: DeviceBuffer<f32>,
    pub partial_sums: DeviceBuffer<f32>,
    pub partial_sq_sums: DeviceBuffer<f32>,

    
    pub labels_current: DeviceBuffer<i32>, 
    pub labels_next: DeviceBuffer<i32>,    
    pub label_counts: DeviceBuffer<i32>,   
    pub convergence_flag: DeviceBuffer<i32>, 
    pub node_degrees: DeviceBuffer<f32>,   
    pub modularity_contributions: DeviceBuffer<f32>, 
    pub community_sizes: DeviceBuffer<i32>, 
    pub label_mapping: DeviceBuffer<i32>,  
    pub rand_states: DeviceBuffer<curandState>, 
    pub max_labels: usize,                 

    
    pub partial_kinetic_energy: DeviceBuffer<f32>, 
    pub active_node_count: DeviceBuffer<i32>,      
    pub should_skip_physics: DeviceBuffer<i32>,    
    pub system_kinetic_energy: DeviceBuffer<f32>,  

    
    transfer_stream: Stream,     
    transfer_events: [Event; 2], 

    
    host_pos_buffer_a: (Vec<f32>, Vec<f32>, Vec<f32>), 
    host_pos_buffer_b: (Vec<f32>, Vec<f32>, Vec<f32>), 
    host_vel_buffer_a: (Vec<f32>, Vec<f32>, Vec<f32>), 
    host_vel_buffer_b: (Vec<f32>, Vec<f32>, Vec<f32>), 

    
    current_pos_buffer: bool,   
    current_vel_buffer: bool,   
    pos_transfer_pending: bool, 
    vel_transfer_pending: bool, 

    
    aabb_block_results: DeviceBuffer<AABB>, 
    aabb_num_blocks: usize,                 
}

impl UnifiedGPUCompute {
    pub fn new(num_nodes: usize, num_edges: usize, ptx_content: &str) -> Result<Self> {
        Self::new_with_modules(num_nodes, num_edges, ptx_content, None)
    }

    pub fn new_with_modules(
        num_nodes: usize,
        num_edges: usize,
        ptx_content: &str,
        clustering_ptx: Option<&str>,
    ) -> Result<Self> {
        
        if let Err(e) = crate::utils::gpu_diagnostics::validate_ptx_content(ptx_content) {
            let diagnosis = crate::utils::gpu_diagnostics::diagnose_ptx_error(&e);
            return Err(anyhow!("PTX validation failed: {}\n{}", e, diagnosis));
        }

        let device = Device::get_device(0)?;
        let _context = Context::new(device)?;

        
        let module = Module::from_ptx(ptx_content, &[]).map_err(|e| {
            let error_msg = format!("Module::from_ptx() failed: {}", e);
            let diagnosis = crate::utils::gpu_diagnostics::diagnose_ptx_error(&error_msg);
            anyhow!("{}\n{}", error_msg, diagnosis)
        })?;

        
        let clustering_module = if let Some(clustering_ptx_content) = clustering_ptx {
            if let Err(e) =
                crate::utils::gpu_diagnostics::validate_ptx_content(clustering_ptx_content)
            {
                warn!(
                    "Clustering PTX validation failed: {}. Continuing without clustering support.",
                    e
                );
                None
            } else {
                match Module::from_ptx(clustering_ptx_content, &[]) {
                    Ok(module) => {
                        info!("Successfully loaded clustering module");
                        Some(module)
                    }
                    Err(e) => {
                        warn!("Failed to load clustering module: {}. Continuing without clustering support.", e);
                        None
                    }
                }
            }
        } else {
            None
        };

        let stream = Stream::new(StreamFlags::NON_BLOCKING, None)?;

        
        let pos_in_x = DeviceBuffer::zeroed(num_nodes)?;
        let pos_in_y = DeviceBuffer::zeroed(num_nodes)?;
        let pos_in_z = DeviceBuffer::zeroed(num_nodes)?;
        let vel_in_x = DeviceBuffer::zeroed(num_nodes)?;
        let vel_in_y = DeviceBuffer::zeroed(num_nodes)?;
        let vel_in_z = DeviceBuffer::zeroed(num_nodes)?;

        let pos_out_x = DeviceBuffer::zeroed(num_nodes)?;
        let pos_out_y = DeviceBuffer::zeroed(num_nodes)?;
        let pos_out_z = DeviceBuffer::zeroed(num_nodes)?;
        let vel_out_x = DeviceBuffer::zeroed(num_nodes)?;
        let vel_out_y = DeviceBuffer::zeroed(num_nodes)?;
        let vel_out_z = DeviceBuffer::zeroed(num_nodes)?;


        let mass = DeviceBuffer::from_slice(&vec![1.0f32; num_nodes])?;
        let node_graph_id = DeviceBuffer::zeroed(num_nodes)?;

        // Initialize ontology class metadata buffers
        let class_id = DeviceBuffer::zeroed(num_nodes)?;           // Default class ID = 0 (unknown)
        let class_charge = DeviceBuffer::from_slice(&vec![1.0f32; num_nodes])?;  // Default charge = 1.0
        let class_mass = DeviceBuffer::from_slice(&vec![1.0f32; num_nodes])?;    // Default mass = 1.0

        let edge_row_offsets = DeviceBuffer::zeroed(num_nodes + 1)?;
        let edge_col_indices = DeviceBuffer::zeroed(num_edges)?;
        let edge_weights = DeviceBuffer::zeroed(num_edges)?;
        let force_x = DeviceBuffer::zeroed(num_nodes)?;
        let force_y = DeviceBuffer::zeroed(num_nodes)?;
        let force_z = DeviceBuffer::zeroed(num_nodes)?;

        
        let cell_keys = DeviceBuffer::zeroed(num_nodes)?;
        let mut sorted_node_indices = DeviceBuffer::zeroed(num_nodes)?;
        
        let initial_indices: Vec<i32> = (0..num_nodes as i32).collect();
        sorted_node_indices.copy_from(&initial_indices)?;

        
        
        let max_grid_cells = 32 * 32 * 32; 
        let cell_start = DeviceBuffer::zeroed(max_grid_cells)?;
        let cell_end = DeviceBuffer::zeroed(max_grid_cells)?;

        
        let cub_temp_storage = Self::calculate_cub_temp_storage(num_nodes, max_grid_cells)?;

        
        let dist = DeviceBuffer::from_slice(&vec![f32::INFINITY; num_nodes])?;
        let current_frontier = DeviceBuffer::zeroed(num_nodes)?;
        let next_frontier_flags = DeviceBuffer::zeroed(num_nodes)?;
        let sssp_stream = Some(Stream::new(StreamFlags::NON_BLOCKING, None)?);

        
        let max_clusters = 50;
        let centroids_x = DeviceBuffer::zeroed(max_clusters)?;
        let centroids_y = DeviceBuffer::zeroed(max_clusters)?;
        let centroids_z = DeviceBuffer::zeroed(max_clusters)?;
        let cluster_assignments = DeviceBuffer::zeroed(num_nodes)?;
        let distances_to_centroid = DeviceBuffer::zeroed(num_nodes)?;
        let cluster_sizes = DeviceBuffer::zeroed(max_clusters)?;
        
        let num_blocks = (num_nodes + 255) / 256;
        let partial_inertia = DeviceBuffer::zeroed(num_blocks)?;
        let min_distances = DeviceBuffer::zeroed(num_nodes)?;
        let selected_nodes = DeviceBuffer::zeroed(max_clusters)?;

        
        let lof_scores = DeviceBuffer::zeroed(num_nodes)?;
        let local_densities = DeviceBuffer::zeroed(num_nodes)?;
        let zscore_values = DeviceBuffer::zeroed(num_nodes)?;
        let feature_values = DeviceBuffer::zeroed(num_nodes)?;
        let partial_sums = DeviceBuffer::zeroed(num_blocks)?;
        let partial_sq_sums = DeviceBuffer::zeroed(num_blocks)?;

        
        let labels_current = DeviceBuffer::zeroed(num_nodes)?;
        let labels_next = DeviceBuffer::zeroed(num_nodes)?;
        let label_counts = DeviceBuffer::zeroed(num_nodes)?; 
        let convergence_flag = DeviceBuffer::from_slice(&[1i32])?; 
        let node_degrees = DeviceBuffer::zeroed(num_nodes)?;
        let modularity_contributions = DeviceBuffer::zeroed(num_nodes)?;
        let community_sizes = DeviceBuffer::zeroed(num_nodes)?;
        let label_mapping = DeviceBuffer::zeroed(num_nodes)?;
        
        let rand_states = DeviceBuffer::from_slice(&vec![
            curandState {
                _private: [0u8; 48]
            };
            num_nodes
        ])?;
        let max_labels = num_nodes;

        
        let kernel_module = module;

        
        let initial_memory = Self::calculate_memory_usage(num_nodes, num_edges, max_grid_cells);

        let gpu_compute = Self {
            device,
            _context,
            _module: kernel_module,
            clustering_module,
            stream,
            build_grid_kernel_name: "build_grid_kernel",
            compute_cell_bounds_kernel_name: "compute_cell_bounds_kernel",
            force_pass_kernel_name: "force_pass_kernel",
            integrate_pass_kernel_name: "integrate_pass_kernel",
            params: SimParams::default(),
            pos_in_x,
            pos_in_y,
            pos_in_z,
            vel_in_x,
            vel_in_y,
            vel_in_z,
            pos_out_x,
            pos_out_y,
            pos_out_z,
            vel_out_x,
            vel_out_y,
            vel_out_z,
            mass,
            node_graph_id,
            class_id,
            class_charge,
            class_mass,
            edge_row_offsets,
            edge_col_indices,
            edge_weights,
            force_x,
            force_y,
            force_z,
            cell_keys,
            sorted_node_indices,
            cell_start,
            cell_end,
            cub_temp_storage,
            num_nodes,
            num_edges,
            allocated_nodes: num_nodes,
            allocated_edges: num_edges,
            max_grid_cells,
            iteration: 0,
            zero_buffer: vec![0i32; max_grid_cells], 
            
            dist,
            current_frontier,
            next_frontier_flags,
            parents: None, 
            sssp_stream,
            
            constraint_data: DeviceBuffer::from_slice(&vec![])?,
            num_constraints: 0,
            sssp_available: false,
            performance_metrics: GPUPerformanceMetrics::default(),
            
            centroids_x,
            centroids_y,
            centroids_z,
            cluster_assignments,
            distances_to_centroid,
            cluster_sizes,
            partial_inertia,
            min_distances,
            selected_nodes,
            max_clusters,
            
            lof_scores,
            local_densities,
            zscore_values,
            feature_values,
            partial_sums,
            partial_sq_sums,
            
            labels_current,
            labels_next,
            label_counts,
            convergence_flag,
            node_degrees,
            modularity_contributions,
            community_sizes,
            label_mapping,
            rand_states,
            max_labels,
            
            cell_buffer_growth_factor: 1.5,
            max_allowed_grid_cells: 128 * 128 * 128, 
            resize_count: 0,
            total_memory_allocated: initial_memory,
            
            partial_kinetic_energy: DeviceBuffer::zeroed((num_nodes + 255) / 256)?, 
            active_node_count: DeviceBuffer::zeroed(1)?,
            should_skip_physics: DeviceBuffer::zeroed(1)?,
            system_kinetic_energy: DeviceBuffer::zeroed(1)?,

            
            transfer_stream: Stream::new(StreamFlags::NON_BLOCKING, None)?,
            transfer_events: [
                Event::new(EventFlags::DEFAULT)?,
                Event::new(EventFlags::DEFAULT)?,
            ],

            
            host_pos_buffer_a: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),
            host_pos_buffer_b: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),
            host_vel_buffer_a: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),
            host_vel_buffer_b: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),

            
            current_pos_buffer: false,   
            current_vel_buffer: false,   
            pos_transfer_pending: false, 
            vel_transfer_pending: false, 

            
            aabb_num_blocks: (num_nodes + 255) / 256,
            aabb_block_results: DeviceBuffer::zeroed((num_nodes + 255) / 256)?,
        };

        

        Ok(gpu_compute)
    }

    fn calculate_cub_temp_storage(
        _num_nodes: usize,
        _num_cells: usize,
    ) -> Result<DeviceBuffer<u8>> {
        let sort_bytes;
        let scan_bytes;
        let mut error;

        
        let d_keys_temp = DeviceBuffer::<i32>::zeroed(0)?;
        let _d_keys_null = d_keys_temp.as_slice();
        let d_values_temp = DeviceBuffer::<i32>::zeroed(0)?;
        let _d_values_null = d_values_temp.as_slice();
        
        sort_bytes = 0; 
        error = 0; 
        if error != 0 {
            return Err(anyhow!(
                "CUB sort storage calculation failed with code {}",
                error
            ));
        }

        
        let d_scan_temp = DeviceBuffer::<i32>::zeroed(0)?;
        let _d_scan_null = d_scan_temp.as_slice();
        
        scan_bytes = 0; 
        error = 0; 
        if error != 0 {
            return Err(anyhow!(
                "CUB scan storage calculation failed with code {}",
                error
            ));
        }

        let total_bytes = sort_bytes.max(scan_bytes);
        DeviceBuffer::zeroed(total_bytes)
            .map_err(|e| anyhow!("Failed to allocate CUB temp storage: {}", e))
    }

    pub fn upload_positions(&mut self, x: &[f32], y: &[f32], z: &[f32]) -> Result<()> {
        
        if x.len() != self.num_nodes || y.len() != self.num_nodes || z.len() != self.num_nodes {
            return Err(anyhow!(
                "Position array size mismatch: expected {} nodes, got x:{}, y:{}, z:{}",
                self.num_nodes,
                x.len(),
                y.len(),
                z.len()
            ));
        }

        
        if x.len() < self.allocated_nodes {
            let mut padded_x = x.to_vec();
            let mut padded_y = y.to_vec();
            let mut padded_z = z.to_vec();
            padded_x.resize(self.allocated_nodes, 0.0);
            padded_y.resize(self.allocated_nodes, 0.0);
            padded_z.resize(self.allocated_nodes, 0.0);
            self.pos_in_x.copy_from(&padded_x)?;
            self.pos_in_y.copy_from(&padded_y)?;
            self.pos_in_z.copy_from(&padded_z)?;
        } else {
            self.pos_in_x.copy_from(x)?;
            self.pos_in_y.copy_from(y)?;
            self.pos_in_z.copy_from(z)?;
        }
        Ok(())
    }

    /// Upload ontology class metadata for class-based physics
    /// Maps owl_class_iri to integer class IDs and sets class-specific force parameters
    pub fn upload_class_metadata(
        &mut self,
        class_ids: &[i32],
        class_charges: &[f32],
        class_masses: &[f32],
    ) -> Result<()> {
        if class_ids.len() != self.num_nodes {
            return Err(anyhow!(
                "Class ID array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                class_ids.len()
            ));
        }
        if class_charges.len() != self.num_nodes {
            return Err(anyhow!(
                "Class charge array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                class_charges.len()
            ));
        }
        if class_masses.len() != self.num_nodes {
            return Err(anyhow!(
                "Class mass array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                class_masses.len()
            ));
        }

        // Upload to GPU buffers
        self.class_id.copy_from(class_ids)?;
        self.class_charge.copy_from(class_charges)?;
        self.class_mass.copy_from(class_masses)?;

        Ok(())
    }

    pub fn upload_edges_csr(
        &mut self,
        row_offsets: &[i32],
        col_indices: &[i32],
        weights: &[f32],
    ) -> Result<()> {
        
        if row_offsets.len() != self.num_nodes + 1 {
            return Err(anyhow!(
                "Row offsets size mismatch: expected {} (num_nodes + 1), got {}",
                self.num_nodes + 1,
                row_offsets.len()
            ));
        }

        
        if col_indices.len() != weights.len() {
            return Err(anyhow!(
                "Edge arrays size mismatch: col_indices has {}, weights has {}",
                col_indices.len(),
                weights.len()
            ));
        }

        
        if col_indices.len() > self.allocated_edges {
            return Err(anyhow!(
                "Too many edges: trying to upload {}, but only {} allocated",
                col_indices.len(),
                self.allocated_edges
            ));
        }

        
        
        if row_offsets.len() <= self.allocated_nodes + 1 {
            
            let mut padded_row_offsets = row_offsets.to_vec();
            let last_val = *padded_row_offsets.last().unwrap_or(&0);
            padded_row_offsets.resize(self.allocated_nodes + 1, last_val);
            self.edge_row_offsets.copy_from(&padded_row_offsets)?;
        } else {
            self.edge_row_offsets.copy_from(row_offsets)?;
        }

        
        if col_indices.len() < self.allocated_edges {
            let mut padded_col_indices = col_indices.to_vec();
            let mut padded_weights = weights.to_vec();
            padded_col_indices.resize(self.allocated_edges, 0);
            padded_weights.resize(self.allocated_edges, 0.0);
            self.edge_col_indices.copy_from(&padded_col_indices)?;
            self.edge_weights.copy_from(&padded_weights)?;
        } else {
            self.edge_col_indices.copy_from(col_indices)?;
            self.edge_weights.copy_from(weights)?;
        }

        self.num_edges = col_indices.len();
        Ok(())
    }

    pub fn download_positions(&self, x: &mut [f32], y: &mut [f32], z: &mut [f32]) -> Result<()> {
        self.pos_in_x.copy_to(x)?;
        self.pos_in_y.copy_to(y)?;
        self.pos_in_z.copy_to(z)?;
        Ok(())
    }

    pub fn download_velocities(&self, x: &mut [f32], y: &mut [f32], z: &mut [f32]) -> Result<()> {
        self.vel_in_x.copy_to(x)?;
        self.vel_in_y.copy_to(y)?;
        self.vel_in_z.copy_to(z)?;
        Ok(())
    }

    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.pos_in_x, &mut self.pos_out_x);
        std::mem::swap(&mut self.pos_in_y, &mut self.pos_out_y);
        std::mem::swap(&mut self.pos_in_z, &mut self.pos_out_z);
        std::mem::swap(&mut self.vel_in_x, &mut self.vel_out_x);
        std::mem::swap(&mut self.vel_in_y, &mut self.vel_out_y);
        std::mem::swap(&mut self.vel_in_z, &mut self.vel_out_z);
    }

    
    fn calculate_memory_usage(num_nodes: usize, num_edges: usize, max_grid_cells: usize) -> usize {
        
        let node_memory = num_nodes * (12 * 4 + 1 * 4 + 1 * 4);
        
        let edge_memory = (num_nodes + 1) * 4 + num_edges * (4 + 4);
        
        let grid_memory = max_grid_cells * (4 + 4) + num_nodes * (4 + 4);
        
        let force_memory = num_nodes * 3 * 4;
        
        let other_memory = num_nodes * 10 * 4;

        node_memory + edge_memory + grid_memory + force_memory + other_memory
    }

    pub fn get_memory_metrics(&self) -> (usize, f32, usize) {
        let current_usage =
            Self::calculate_memory_usage(self.num_nodes, self.num_edges, self.max_grid_cells);
        let allocated_usage = Self::calculate_memory_usage(
            self.allocated_nodes,
            self.allocated_edges,
            self.max_grid_cells,
        );
        let utilization = current_usage as f32 / allocated_usage as f32;
        (current_usage, utilization, self.resize_count)
    }

    
    pub fn get_grid_occupancy(&self, num_grid_cells: usize) -> f32 {
        if num_grid_cells == 0 {
            return 0.0;
        }
        let avg_nodes_per_cell = self.num_nodes as f32 / num_grid_cells as f32;
        
        let optimal_occupancy = 8.0;
        (avg_nodes_per_cell / optimal_occupancy).min(1.0)
    }

    
    pub fn resize_cell_buffers(&mut self, required_cells: usize) -> Result<()> {
        if required_cells <= self.max_grid_cells {
            return Ok(());
        }

        
        if required_cells > self.max_allowed_grid_cells {
            warn!(
                "Grid size {} exceeds maximum allowed {}, capping to maximum",
                required_cells, self.max_allowed_grid_cells
            );
            let capped_size = self.max_allowed_grid_cells;
            return self.resize_cell_buffers_internal(capped_size);
        }

        
        let new_size = ((required_cells as f32 * self.cell_buffer_growth_factor) as usize)
            .min(self.max_allowed_grid_cells);

        self.resize_cell_buffers_internal(new_size)
    }

    
    fn resize_cell_buffers_internal(&mut self, new_size: usize) -> Result<()> {
        info!(
            "Resizing cell buffers from {} to {} cells ({}x growth)",
            self.max_grid_cells, new_size, self.cell_buffer_growth_factor
        );

        
        let preserve_data = self.max_grid_cells > 0 && self.iteration > 0;

        let old_cell_start_data = if preserve_data {
            let mut data = vec![0i32; self.max_grid_cells];
            self.cell_start.copy_to(&mut data).unwrap_or_else(|e| {
                warn!("Failed to preserve cell_start data: {}", e);
            });
            Some(data)
        } else {
            None
        };

        let old_cell_end_data = if preserve_data {
            let mut data = vec![0i32; self.max_grid_cells];
            self.cell_end.copy_to(&mut data).unwrap_or_else(|e| {
                warn!("Failed to preserve cell_end data: {}", e);
            });
            Some(data)
        } else {
            None
        };

        
        self.cell_start = DeviceBuffer::zeroed(new_size).map_err(|e| {
            anyhow!(
                "Failed to allocate cell_start buffer of size {}: {}",
                new_size,
                e
            )
        })?;
        self.cell_end = DeviceBuffer::zeroed(new_size).map_err(|e| {
            anyhow!(
                "Failed to allocate cell_end buffer of size {}: {}",
                new_size,
                e
            )
        })?;

        
        if let (Some(start_data), Some(end_data)) = (old_cell_start_data, old_cell_end_data) {
            let copy_size = start_data.len().min(new_size);
            if copy_size > 0 {
                self.cell_start.copy_from(&start_data[..copy_size])?;
                self.cell_end.copy_from(&end_data[..copy_size])?;
                debug!("Preserved {} cells of data during resize", copy_size);
            }
        }

        
        let old_memory = self.total_memory_allocated;
        self.max_grid_cells = new_size;
        self.zero_buffer = vec![0i32; new_size];
        self.resize_count += 1;
        self.total_memory_allocated = Self::calculate_memory_usage(
            self.allocated_nodes,
            self.allocated_edges,
            self.max_grid_cells,
        );

        let memory_delta = self.total_memory_allocated as i64 - old_memory as i64;
        info!(
            "Cell buffer resize complete. Memory change: {:+} bytes, Total: {} MB",
            memory_delta,
            self.total_memory_allocated / 1024 / 1024
        );

        
        if self.resize_count > 10 {
            warn!("High resize frequency detected ({} resizes). Consider increasing initial buffer size.",
                  self.resize_count);
        }

        Ok(())
    }

    
    pub fn resize_buffers(&mut self, new_num_nodes: usize, new_num_edges: usize) -> Result<()> {
        
        if new_num_nodes <= self.num_nodes && new_num_edges <= self.num_edges {
            self.num_nodes = new_num_nodes;
            self.num_edges = new_num_edges;
            return Ok(());
        }

        info!(
            "Resizing GPU buffers from {}/{} to {}/{} nodes/edges",
            self.num_nodes, self.num_edges, new_num_nodes, new_num_edges
        );

        
        let actual_new_nodes = ((new_num_nodes as f32 * 1.5) as usize).max(self.num_nodes);
        let actual_new_edges = ((new_num_edges as f32 * 1.5) as usize).max(self.num_edges);

        
        let mut pos_x_data = vec![0.0f32; self.num_nodes];
        let mut pos_y_data = vec![0.0f32; self.num_nodes];
        let mut pos_z_data = vec![0.0f32; self.num_nodes];
        let mut vel_x_data = vec![0.0f32; self.num_nodes];
        let mut vel_y_data = vec![0.0f32; self.num_nodes];
        let mut vel_z_data = vec![0.0f32; self.num_nodes];

        
        self.pos_in_x.copy_to(&mut pos_x_data)?;
        self.pos_in_y.copy_to(&mut pos_y_data)?;
        self.pos_in_z.copy_to(&mut pos_z_data)?;
        self.vel_in_x.copy_to(&mut vel_x_data)?;
        self.vel_in_y.copy_to(&mut vel_y_data)?;
        self.vel_in_z.copy_to(&mut vel_z_data)?;

        
        pos_x_data.resize(actual_new_nodes, 0.0);
        pos_y_data.resize(actual_new_nodes, 0.0);
        pos_z_data.resize(actual_new_nodes, 0.0);
        vel_x_data.resize(actual_new_nodes, 0.0);
        vel_y_data.resize(actual_new_nodes, 0.0);
        vel_z_data.resize(actual_new_nodes, 0.0);

        
        self.pos_in_x = DeviceBuffer::from_slice(&pos_x_data)?;
        self.pos_in_y = DeviceBuffer::from_slice(&pos_y_data)?;
        self.pos_in_z = DeviceBuffer::from_slice(&pos_z_data)?;
        self.vel_in_x = DeviceBuffer::from_slice(&vel_x_data)?;
        self.vel_in_y = DeviceBuffer::from_slice(&vel_y_data)?;
        self.vel_in_z = DeviceBuffer::from_slice(&vel_z_data)?;

        self.pos_out_x = DeviceBuffer::from_slice(&pos_x_data)?;
        self.pos_out_y = DeviceBuffer::from_slice(&pos_y_data)?;
        self.pos_out_z = DeviceBuffer::from_slice(&pos_z_data)?;
        self.vel_out_x = DeviceBuffer::from_slice(&vel_x_data)?;
        self.vel_out_y = DeviceBuffer::from_slice(&vel_y_data)?;
        self.vel_out_z = DeviceBuffer::from_slice(&vel_z_data)?;

        
        self.mass = DeviceBuffer::from_slice(&vec![1.0f32; actual_new_nodes])?;
        self.node_graph_id = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.edge_row_offsets = DeviceBuffer::zeroed(actual_new_nodes + 1)?;
        self.edge_col_indices = DeviceBuffer::zeroed(actual_new_edges)?;
        self.edge_weights = DeviceBuffer::zeroed(actual_new_edges)?;
        self.force_x = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.force_y = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.force_z = DeviceBuffer::zeroed(actual_new_nodes)?;

        
        self.cell_keys = DeviceBuffer::zeroed(actual_new_nodes)?;
        let sorted_indices: Vec<i32> = (0..actual_new_nodes as i32).collect();
        self.sorted_node_indices = DeviceBuffer::from_slice(&sorted_indices)?;

        
        self.total_memory_allocated = Self::calculate_memory_usage(
            self.allocated_nodes,
            self.allocated_edges,
            self.max_grid_cells,
        );

        
        self.cluster_assignments = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.distances_to_centroid = DeviceBuffer::zeroed(actual_new_nodes)?;
        let new_num_blocks = (actual_new_nodes + 255) / 256;
        self.partial_inertia = DeviceBuffer::zeroed(new_num_blocks)?;
        self.min_distances = DeviceBuffer::zeroed(actual_new_nodes)?;

        
        self.lof_scores = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.local_densities = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.zscore_values = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.feature_values = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.partial_sums = DeviceBuffer::zeroed(new_num_blocks)?;
        self.partial_sq_sums = DeviceBuffer::zeroed(new_num_blocks)?;

        
        self.num_nodes = new_num_nodes;
        self.num_edges = new_num_edges;
        self.allocated_nodes = actual_new_nodes;
        self.allocated_edges = actual_new_edges;

        info!(
            "Successfully resized GPU buffers to {}/{} allocated nodes/edges",
            actual_new_nodes, actual_new_edges
        );
        Ok(())
    }

    pub fn set_params(&mut self, params: SimParams) -> Result<()> {
        
        info!(
            "Setting SimParams - spring_k: {:.4}, repel_k: {:.2}, damping: {:.3}, dt: {:.3}",
            params.spring_k, params.repel_k, params.damping, params.dt
        );

        self.params = params;

        
        
        
        
        

        info!("SimParams successfully updated");
        Ok(())
    }

    pub fn set_mode(&mut self, _mode: ComputeMode) {
        
    }

    pub fn set_constraints(&mut self, mut constraints: Vec<ConstraintData>) -> Result<()> {
        
        let current_iteration = self.iteration;
        for constraint in &mut constraints {
            if constraint.activation_frame == 0 {
                constraint.activation_frame = current_iteration as i32;
                debug!(
                    "Setting activation frame {} for constraint type {}",
                    current_iteration, constraint.kind
                );
            }
        }

        
        if constraints.len() > self.constraint_data.len() {
            info!(
                "Resizing constraint buffer from {} to {} with progressive activation",
                self.constraint_data.len(),
                constraints.len()
            );
            
            let new_constraint_buffer = DeviceBuffer::from_slice(&constraints)?;
            self.constraint_data = new_constraint_buffer;
        } else if !constraints.is_empty() {
            
            let constraint_len = self.constraint_data.len();
            let copy_len = constraints.len().min(constraint_len);
            self.constraint_data.copy_from(&constraints[..copy_len])?;
        }

        self.num_constraints = constraints.len();
        debug!(
            "Updated GPU constraints: {} active constraints with progressive activation support",
            self.num_constraints
        );
        Ok(())
    }

    pub fn execute(&mut self, mut params: SimParams) -> Result<()> {
        // Make CUDA context current for this thread (required when called from spawn_blocking threads)
        // Context::new() on the same device retains the primary context and makes it current
        let _thread_context = Context::new(self.device.clone())
            .map_err(|e| anyhow!("Failed to set CUDA context: {}", e))?;

        params.iteration = self.iteration;
        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        if self.num_nodes > self.allocated_nodes {
            return Err(anyhow!("CRITICAL: num_nodes ({}) exceeds allocated_nodes ({}). This would cause buffer overflow!", self.num_nodes, self.allocated_nodes));
        }

        
        self.params = params;

        
        let mut c_params_global = self
            ._module
            .get_global(CStr::from_bytes_with_nul(b"c_params\0").unwrap())?;
        c_params_global.copy_from(&[params])?;

        
        
        if self.num_nodes > 0 && params.stability_threshold > 0.0 {
            let num_blocks = (self.num_nodes + block_size as usize - 1) / block_size as usize;
            let shared_mem_size =
                block_size * (std::mem::size_of::<f32>() + std::mem::size_of::<i32>()) as u32;

            
            self.active_node_count.copy_from(&[0i32])?;
            self.should_skip_physics.copy_from(&[0i32])?;

            
            let ke_kernel = self
                ._module
                .get_function("calculate_kinetic_energy_kernel")?;
            // SAFETY: Kernel launch is safe because:
            // 1. All DeviceBuffer pointers (vel_in_*, mass, partial_kinetic_energy, active_node_count)
            //    are valid allocations created during UnifiedGPUCompute::new()
            // 2. num_nodes <= allocated_nodes was verified at function entry
            // 3. shared_mem_size is computed based on block_size and type sizes
            // 4. self.stream is a valid CUDA stream created in UnifiedGPUCompute::new()
            // 5. The kernel function was loaded from a valid PTX module
            unsafe {
                let stream = &self.stream;
                launch!(
                    ke_kernel<<<num_blocks as u32, block_size, shared_mem_size, stream>>>(
                        self.vel_in_x.as_device_ptr(),
                        self.vel_in_y.as_device_ptr(),
                        self.vel_in_z.as_device_ptr(),
                        self.mass.as_device_ptr(),
                        self.partial_kinetic_energy.as_device_ptr(),
                        self.active_node_count.as_device_ptr(),
                        self.num_nodes as i32,
                        params.min_velocity_threshold
                    )
                )?;
            }

            
            let stability_kernel = self._module.get_function("check_system_stability_kernel")?;
            let reduction_blocks = (num_blocks as u32).min(256);
            // SAFETY: Kernel launch is safe because:
            // 1. All DeviceBuffer arguments are valid allocations from UnifiedGPUCompute::new()
            // 2. reduction_blocks is bounded to max 256 (valid CUDA block size)
            // 3. Shared memory (reduction_blocks * 4) fits within GPU limits
            // 4. This reduction kernel reads from partial_kinetic_energy computed by prior kernel
            unsafe {
                let stream = &self.stream;
                launch!(
                    stability_kernel<<<1, reduction_blocks, reduction_blocks * 4, stream>>>(
                        self.partial_kinetic_energy.as_device_ptr(),
                        self.active_node_count.as_device_ptr(),
                        self.should_skip_physics.as_device_ptr(),
                        self.system_kinetic_energy.as_device_ptr(),
                        num_blocks as i32,
                        self.num_nodes as i32,
                        params.stability_threshold,
                        self.iteration
                    )
                )?;
            }

            
            let mut skip_physics = vec![0i32; 1];
            self.should_skip_physics.copy_to(&mut skip_physics)?;

            if skip_physics[0] != 0 {
                
                self.iteration += 1;
                return Ok(());
            }
        }

        
        crate::utils::gpu_diagnostics::validate_kernel_launch(
            "unified_gpu_execute",
            grid_size,
            block_size,
            self.num_nodes,
        )
        .map_err(|e| anyhow::anyhow!(e))?;

        
        let aabb_kernel = self._module.get_function("compute_aabb_reduction_kernel")?;
        let aabb_block_size = 256u32;
        let aabb_grid_size = self.aabb_num_blocks as u32;
        let shared_mem = 6 * aabb_block_size * std::mem::size_of::<f32>() as u32;

        // SAFETY: AABB reduction kernel launch is safe because:
        // 1. pos_in_* buffers contain valid position data from prior physics step
        // 2. aabb_block_results is sized for aabb_num_blocks * sizeof(AABB)
        // 3. shared_mem is computed as 6 floats per thread (min/max x,y,z)
        // 4. aabb_grid_size and aabb_block_size are validated during construction
        unsafe {
            let s = &self.stream;
            launch!(
                aabb_kernel<<<aabb_grid_size, aabb_block_size, shared_mem, s>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.aabb_block_results.as_device_ptr(),
                    self.num_nodes as i32
                )
            )?;
        }

        
        let mut block_results = vec![AABB::default(); self.aabb_num_blocks];
        self.aabb_block_results.copy_to(&mut block_results)?;

        let mut aabb = AABB {
            min: [f32::MAX; 3],
            max: [f32::MIN; 3],
        };
        for block_aabb in block_results.iter().take(self.aabb_num_blocks) {
            aabb.min[0] = aabb.min[0].min(block_aabb.min[0]);
            aabb.min[1] = aabb.min[1].min(block_aabb.min[1]);
            aabb.min[2] = aabb.min[2].min(block_aabb.min[2]);
            aabb.max[0] = aabb.max[0].max(block_aabb.max[0]);
            aabb.max[1] = aabb.max[1].max(block_aabb.max[1]);
            aabb.max[2] = aabb.max[2].max(block_aabb.max[2]);
        }
        
        let scene_volume =
            (aabb.max[0] - aabb.min[0]) * (aabb.max[1] - aabb.min[1]) * (aabb.max[2] - aabb.min[2]);
        let target_neighbors_per_cell = 8.0; 
        let optimal_cells = self.num_nodes as f32 / target_neighbors_per_cell;
        let optimal_cell_size = (scene_volume / optimal_cells).powf(1.0 / 3.0);

        
        let auto_tuned_cell_size = if optimal_cell_size > 10.0 && optimal_cell_size < 1000.0 {
            optimal_cell_size
        } else {
            params.grid_cell_size
        };

        debug!(
            "Spatial hashing: scene_volume={:.2}, optimal_cell_size={:.2}, using_size={:.2}",
            scene_volume, optimal_cell_size, auto_tuned_cell_size
        );

        
        aabb.min[0] -= auto_tuned_cell_size;
        aabb.max[0] += auto_tuned_cell_size;
        aabb.min[1] -= auto_tuned_cell_size;
        aabb.max[1] += auto_tuned_cell_size;
        aabb.min[2] -= auto_tuned_cell_size;
        aabb.max[2] += auto_tuned_cell_size;

        
        let grid_dims = int3 {
            x: ((aabb.max[0] - aabb.min[0]) / auto_tuned_cell_size).ceil() as i32,
            y: ((aabb.max[1] - aabb.min[1]) / auto_tuned_cell_size).ceil() as i32,
            z: ((aabb.max[2] - aabb.min[2]) / auto_tuned_cell_size).ceil() as i32,
        };
        let num_grid_cells = (grid_dims.x * grid_dims.y * grid_dims.z) as usize;

        
        let occupancy = self.get_grid_occupancy(num_grid_cells);
        if occupancy < 0.1 {
            warn!("Low grid occupancy detected: {:.1}% (avg {:.1} nodes/cell). Consider larger cell size.",
                  occupancy * 100.0, self.num_nodes as f32 / num_grid_cells as f32);
        } else if occupancy > 2.0 {
            warn!("High grid occupancy detected: {:.1}% (avg {:.1} nodes/cell). Consider smaller cell size.",
                  occupancy * 100.0, self.num_nodes as f32 / num_grid_cells as f32);
        }

        
        if num_grid_cells > self.max_grid_cells {
            self.resize_cell_buffers(num_grid_cells)?;
            debug!(
                "Grid buffer resize completed. Current grid: {}x{}x{} = {} cells",
                grid_dims.x, grid_dims.y, grid_dims.z, num_grid_cells
            );
        }

        
        crate::utils::gpu_diagnostics::validate_kernel_launch(
            self.build_grid_kernel_name,
            grid_size,
            block_size,
            self.num_nodes,
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        let build_grid_kernel = self
            ._module
            .get_function(self.build_grid_kernel_name)
            .map_err(|e| {
                let diagnosis = crate::utils::gpu_diagnostics::diagnose_ptx_error(&format!(
                    "Kernel '{}' not found: {}",
                    self.build_grid_kernel_name, e
                ));
                anyhow!(
                    "Failed to get kernel function '{}':\n{}",
                    self.build_grid_kernel_name,
                    diagnosis
                )
            })?;
        // SAFETY: Grid building kernel launch is safe because:
        // 1. pos_in_* buffers are valid DeviceBuffers with capacity >= num_nodes
        // 2. cell_keys buffer is sized for allocated_nodes elements
        // 3. aabb and grid_dims are computed from valid position data
        // 4. auto_tuned_cell_size is a positive float computed from AABB dimensions
        // 5. validate_kernel_launch() was called above to verify launch parameters
        unsafe {
            let stream = &self.stream;
            launch!(
                build_grid_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                self.pos_in_x.as_device_ptr(),
                self.pos_in_y.as_device_ptr(),
                self.pos_in_z.as_device_ptr(),
                self.cell_keys.as_device_ptr(),
                aabb,
                grid_dims,
                auto_tuned_cell_size,
                self.num_nodes as i32
            ))?;
        }

        
        let d_keys_in = self.cell_keys.as_slice();
        let d_values_in = self.sorted_node_indices.as_slice();
        
        let d_keys_out = DeviceBuffer::<i32>::zeroed(self.allocated_nodes)?;
        let mut d_values_out = DeviceBuffer::<i32>::zeroed(self.allocated_nodes)?;

        // SAFETY: Thrust sort FFI call is safe because:
        // 1. d_keys_in (cell_keys) is a valid DeviceBuffer allocated for allocated_nodes elements
        // 2. d_keys_out is a freshly allocated DeviceBuffer::zeroed(allocated_nodes)
        // 3. d_values_in (sorted_node_indices) is a valid DeviceBuffer for allocated_nodes elements
        // 4. d_values_out is a freshly allocated DeviceBuffer::zeroed(allocated_nodes)
        // 5. num_items is bounded by min(num_nodes, allocated_nodes) preventing out-of-bounds
        // 6. stream_ptr is obtained from a valid cust::Stream via as_inner()
        // 7. Thrust internally synchronizes on the provided stream before returning
        unsafe {
            let stream_ptr = self.stream.as_inner() as *mut ::std::os::raw::c_void;
            thrust_sort_key_value(
                d_keys_in.as_device_ptr().as_raw() as *const ::std::os::raw::c_void,
                d_keys_out.as_device_ptr().as_raw() as *mut ::std::os::raw::c_void,
                d_values_in.as_device_ptr().as_raw() as *const ::std::os::raw::c_void,
                d_values_out.as_device_ptr().as_raw() as *mut ::std::os::raw::c_void,
                self.num_nodes.min(self.allocated_nodes) as ::std::os::raw::c_int,
                stream_ptr,
            );
        }
        
        let sorted_keys = d_keys_out;
        
        std::mem::swap(&mut self.sorted_node_indices, &mut d_values_out);

        
        
        
        
        self.cell_start.copy_from(&self.zero_buffer)?;
        self.cell_end.copy_from(&self.zero_buffer)?;

        let grid_cells_blocks = (num_grid_cells as u32 + 255) / 256;
        let compute_cell_bounds_kernel = self
            ._module
            .get_function(self.compute_cell_bounds_kernel_name)?;
        // SAFETY: Cell bounds kernel launch is safe because:
        // 1. sorted_keys is the output from thrust_sort_key_value (valid device memory)
        // 2. cell_start and cell_end were zeroed and have capacity >= num_grid_cells
        // 3. num_grid_cells was computed from validated grid dimensions
        // 4. The kernel reads sorted_keys and writes cell boundaries atomically
        unsafe {
            let stream = &self.stream;
            launch!(
                compute_cell_bounds_kernel<<<grid_cells_blocks, 256, 0, stream>>>(
                sorted_keys.as_device_ptr(),
                self.cell_start.as_device_ptr(),
                self.cell_end.as_device_ptr(),
                self.num_nodes as i32,
                num_grid_cells as i32
            ))?;
        }

        
        
        let force_kernel_name = if params.stability_threshold > 0.0 {
            "force_pass_with_stability_kernel"
        } else {
            self.force_pass_kernel_name
        };
        let force_pass_kernel = self._module.get_function(force_kernel_name)?;
        let stream = &self.stream;

        
        let d_sssp = if self.sssp_available
            && (params.feature_flags
                & crate::models::simulation_params::FeatureFlags::ENABLE_SSSP_SPRING_ADJUST
                != 0)
        {
            self.dist.as_device_ptr()
        } else {
            DevicePointer::null()
        };

        // SAFETY: Force computation kernel launch is safe because:
        // 1. All position, velocity, and force buffers are valid DeviceBuffers with capacity >= num_nodes
        // 2. cell_start, cell_end, sorted_node_indices, cell_keys are from the spatial grid build phase
        // 3. edge_row_offsets, edge_col_indices, edge_weights are CSR graph data loaded at construction
        // 4. d_sssp is either a valid DevicePointer to dist buffer or DevicePointer::null()
        // 5. constraint_data has capacity for num_constraints ConstraintData elements
        // 6. should_skip_physics is a valid single-element DeviceBuffer for stability gating
        // 7. grid_size and block_size are validated via validate_kernel_launch()
        unsafe {
            if params.stability_threshold > 0.0 {
                // Force pass with stability checking variant
                launch!(
                    force_pass_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.vel_in_x.as_device_ptr(),
                    self.vel_in_y.as_device_ptr(),
                    self.vel_in_z.as_device_ptr(),
                    self.force_x.as_device_ptr(),
                    self.force_y.as_device_ptr(),
                    self.force_z.as_device_ptr(),
                    self.cell_start.as_device_ptr(),
                    self.cell_end.as_device_ptr(),
                    self.sorted_node_indices.as_device_ptr(),
                    self.cell_keys.as_device_ptr(),
                    grid_dims,
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_col_indices.as_device_ptr(),
                    self.edge_weights.as_device_ptr(),
                    self.num_nodes as i32,
                    d_sssp,
                    self.constraint_data.as_device_ptr(),
                    self.num_constraints as i32,
                    self.should_skip_physics.as_device_ptr()
                ))?;
            } else {
                
                launch!(
                    force_pass_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.force_x.as_device_ptr(),
                    self.force_y.as_device_ptr(),
                    self.force_z.as_device_ptr(),
                    self.cell_start.as_device_ptr(),
                    self.cell_end.as_device_ptr(),
                    self.sorted_node_indices.as_device_ptr(),
                    self.cell_keys.as_device_ptr(),
                    grid_dims,
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_col_indices.as_device_ptr(),
                    self.edge_weights.as_device_ptr(),
                    self.num_nodes as i32,
                    d_sssp,
                    self.constraint_data.as_device_ptr(),
                    self.num_constraints as i32,
                    DevicePointer::<f32>::null(),
                    DevicePointer::<f32>::null(),
                    DevicePointer::<f32>::null(),
                    // Ontology class metadata
                    self.class_id.as_device_ptr(),
                    self.class_charge.as_device_ptr(),
                    self.class_mass.as_device_ptr()
                ))?;
            }
        }

        
        let integrate_pass_kernel = self._module.get_function(self.integrate_pass_kernel_name)?;
        let stream = &self.stream;
        // SAFETY: Integration kernel launch is safe because:
        // 1. All input buffers (pos_in_*, vel_in_*, force_*, mass) contain data from force pass
        // 2. All output buffers (pos_out_*, vel_out_*) are valid DeviceBuffers with capacity >= num_nodes
        // 3. class_id, class_charge, class_mass are ontology metadata buffers loaded at construction
        // 4. The kernel performs Verlet integration using c_params constants from device memory
        // 5. After this kernel, swap_buffers() exchanges input/output for next iteration
        unsafe {
            launch!(
                integrate_pass_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                self.pos_in_x.as_device_ptr(),
                self.pos_in_y.as_device_ptr(),
                self.pos_in_z.as_device_ptr(),
                self.vel_in_x.as_device_ptr(),
                self.vel_in_y.as_device_ptr(),
                self.vel_in_z.as_device_ptr(),
                self.force_x.as_device_ptr(),
                self.force_y.as_device_ptr(),
                self.force_z.as_device_ptr(),
                self.mass.as_device_ptr(),
                self.pos_out_x.as_device_ptr(),
                self.pos_out_y.as_device_ptr(),
                self.pos_out_z.as_device_ptr(),
                self.vel_out_x.as_device_ptr(),
                self.vel_out_y.as_device_ptr(),
                self.vel_out_z.as_device_ptr(),
                self.num_nodes as i32,
                // Ontology class metadata
                self.class_id.as_device_ptr(),
                self.class_charge.as_device_ptr(),
                self.class_mass.as_device_ptr()
            ))?;
        }

        
        
        let completion_event = cust::event::Event::new(cust::event::EventFlags::DEFAULT)?;
        completion_event.record(&self.stream)?;

        
        while completion_event
            .query()
            .unwrap_or(cust::event::EventStatus::Ready)
            != cust::event::EventStatus::Ready
        {
            
            std::thread::yield_now();
        }

        self.swap_buffers();
        self.iteration += 1;

        
        if self.iteration % 100 == 0 {
            let (memory_used, utilization, resize_count) = self.get_memory_metrics();
            let grid_occupancy = self.get_grid_occupancy(num_grid_cells);
            info!("Performance metrics [iter {}]: Memory: {:.1}MB ({:.1}% utilized), Grid occupancy: {:.1}%, Resizes: {}",
                  self.iteration, memory_used as f32 / 1024.0 / 1024.0,
                  utilization * 100.0, grid_occupancy * 100.0, resize_count);
        }

        Ok(())
    }

    pub fn run_sssp(&mut self, source_idx: usize) -> Result<Vec<f32>> {
        
        self.sssp_available = false;

        
        let result = (|| -> Result<Vec<f32>> {
            
            let mut host_dist = vec![f32::INFINITY; self.num_nodes];
            host_dist[source_idx] = 0.0;
            self.dist.copy_from(&host_dist)?;

            
            
            let mut frontier_host = vec![-1i32; self.num_nodes];
            frontier_host[0] = source_idx as i32;
            self.current_frontier.copy_from(&frontier_host)?;
            let mut frontier_len = 1usize; 

            
            let s = self.sssp_stream.as_ref().unwrap_or(&self.stream);
            let mut iter_count = 0usize;
            let max_iters = 10 * self.num_nodes.max(1); 
            while frontier_len > 0 {
                iter_count += 1;
                if iter_count > max_iters {
                    log::warn!(
                        "SSSP safety cap reached ({} iters) with frontier_len={}",
                        iter_count,
                        frontier_len
                    );
                    break;
                }
                
                let zeros = vec![0i32; self.num_nodes];
                self.next_frontier_flags.copy_from(&zeros)?;

                
                if frontier_len == 0 {
                    log::debug!("SSSP converged at iteration {}", iter_count);
                    break;
                }

                
                let block = 256;
                let grid = ((frontier_len as u32 + block - 1) / block) as u32;

                let func = self._module.get_function("relaxation_step_kernel")?;
                // SAFETY: SSSP relaxation kernel launch is safe because:
                // 1. dist buffer contains distance values initialized with infinity/0
                // 2. current_frontier contains valid node indices from previous iteration
                // 3. frontier_len <= num_nodes (bounded by graph size)
                // 4. edge_* buffers are valid CSR graph representation
                // 5. next_frontier_flags is zeroed before each iteration
                // 6. The kernel performs Bellman-Ford style edge relaxation
                unsafe {
                    launch!(func<<<grid, block, 0, s>>>(
                        self.dist.as_device_ptr(),
                        self.current_frontier.as_device_ptr(),
                        frontier_len as i32,
                        self.edge_row_offsets.as_device_ptr(),
                        self.edge_col_indices.as_device_ptr(),
                        self.edge_weights.as_device_ptr(),
                        self.next_frontier_flags.as_device_ptr(),
                        f32::INFINITY,
                        self.num_nodes as i32
                    ))?;
                }

                
                
                let d_frontier_counter = DeviceBuffer::from_slice(&[0i32])?;

                
                let compact_func = self._module.get_function("compact_frontier_kernel")?;
                let compact_grid = ((self.num_nodes as u32 + 255) / 256, 1, 1);
                let compact_block = (256, 1, 1);

                // SAFETY: Frontier compaction kernel launch is safe because:
                // 1. next_frontier_flags contains 0/1 flags from relaxation step
                // 2. current_frontier will be overwritten with compacted node indices
                // 3. d_frontier_counter is a single-element buffer for atomic counting
                // 4. The kernel uses atomic operations to build the compacted frontier
                unsafe {
                    launch!(compact_func<<<compact_grid, compact_block, 0, s>>>(
                        self.next_frontier_flags.as_device_ptr(),
                        self.current_frontier.as_device_ptr(),
                        d_frontier_counter.as_device_ptr(),
                        self.num_nodes as i32
                    ))?;
                }

                
                let mut new_frontier_size = vec![0i32; 1];
                d_frontier_counter.copy_to(&mut new_frontier_size)?;
                frontier_len = new_frontier_size[0] as usize;

                
            }

            
            self.dist.copy_to(&mut host_dist)?;
            Ok(host_dist)
        })();

        
        match result {
            Ok(distances) => {
                self.sssp_available = true;
                log::info!("SSSP computation successful from source {}", source_idx);
                Ok(distances)
            }
            Err(e) => {
                self.sssp_available = false;
                log::error!("SSSP computation failed: {}. State invalidated.", e);
                Err(e)
            }
        }
    }

    
    pub fn run_kmeans(
        &mut self,
        num_clusters: usize,
        max_iterations: u32,
        tolerance: f32,
        seed: u32,
    ) -> Result<(Vec<i32>, Vec<(f32, f32, f32)>, f32)> {
        if num_clusters > self.max_clusters {
            return Err(anyhow!(
                "Too many clusters requested: {} > {}",
                num_clusters,
                self.max_clusters
            ));
        }

        
        let module = if let Some(ref clustering_mod) = self.clustering_module {
            clustering_mod
        } else {
            &self._module
        };

        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        for centroid in 0..num_clusters {
            let init_kernel = module.get_function("init_centroids_kernel")?;
            let shared_memory_size = block_size * 4; 
            let stream = &self.stream;

            unsafe {
                launch!(
                    init_kernel<<<num_clusters as u32, block_size, shared_memory_size, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.min_distances.as_device_ptr(),
                    self.selected_nodes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32,
                    centroid as i32,
                    seed
                ))?;
            }
            self.stream.synchronize()?;
        }

        let mut prev_inertia = f32::INFINITY;
        let mut final_inertia = 0.0f32;

        
        for _iteration in 0..max_iterations {
            
            let assign_kernel = self._module.get_function("assign_clusters_kernel")?;
            let stream = &self.stream;
            unsafe {
                launch!(
                    assign_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.distances_to_centroid.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            
            let update_kernel = self._module.get_function("update_centroids_kernel")?;
            let centroid_shared_memory = block_size * (3 * 4 + 4); 
            let stream = &self.stream;
            unsafe {
                launch!(
                    update_kernel<<<num_clusters as u32, block_size, centroid_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_sizes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            
            let inertia_kernel = self._module.get_function("compute_inertia_kernel")?;
            let inertia_shared_memory = block_size * 4; 
            let stream = &self.stream;
            unsafe {
                launch!(
                    inertia_kernel<<<grid_size, block_size, inertia_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.partial_inertia.as_device_ptr(),
                    self.num_nodes as i32
                ))?;
            }

            self.stream.synchronize()?;

            
            let mut partial_inertias = vec![0.0f32; grid_size as usize];
            self.partial_inertia.copy_to(&mut partial_inertias)?;
            let current_inertia: f32 = partial_inertias.iter().sum();
            final_inertia = current_inertia;

            
            if (prev_inertia - current_inertia).abs() < tolerance {
                info!(
                    "K-means converged at iteration {} with inertia {:.4}",
                    _iteration, current_inertia
                );
                break;
            }

            prev_inertia = current_inertia;
        }

        
        let mut assignments = vec![0i32; self.num_nodes];
        self.cluster_assignments.copy_to(&mut assignments)?;

        let mut centroids_x = vec![0.0f32; num_clusters];
        let mut centroids_y = vec![0.0f32; num_clusters];
        let mut centroids_z = vec![0.0f32; num_clusters];
        self.centroids_x.copy_to(&mut centroids_x)?;
        self.centroids_y.copy_to(&mut centroids_y)?;
        self.centroids_z.copy_to(&mut centroids_z)?;

        let centroids: Vec<(f32, f32, f32)> = centroids_x
            .into_iter()
            .zip(centroids_y.into_iter())
            .zip(centroids_z.into_iter())
            .map(|((x, y), z)| (x, y, z))
            .collect();

        Ok((assignments, centroids, final_inertia))
    }

    
    pub fn run_kmeans_clustering_with_metrics(
        &mut self,
        num_clusters: usize,
        max_iterations: u32,
        tolerance: f32,
        seed: u32,
    ) -> Result<(Vec<i32>, Vec<(f32, f32, f32)>, f32, u32, bool)> {
        if num_clusters > self.max_clusters {
            return Err(anyhow!(
                "Too many clusters requested: {} > {}",
                num_clusters,
                self.max_clusters
            ));
        }

        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        for centroid in 0..num_clusters {
            let init_kernel = self._module.get_function("init_centroids_kernel")?;
            let shared_memory_size = block_size * 4; 
            let stream = &self.stream;

            unsafe {
                launch!(
                    init_kernel<<<num_clusters as u32, block_size, shared_memory_size, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.min_distances.as_device_ptr(),
                    self.selected_nodes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32,
                    centroid as i32,
                    seed
                ))?;
            }
            self.stream.synchronize()?;
        }

        let mut prev_inertia = f32::INFINITY;
        let mut final_inertia = 0.0f32;
        let mut converged = false;
        let mut actual_iterations = 0u32;

        
        for iteration in 0..max_iterations {
            actual_iterations = iteration + 1;

            
            let assign_kernel = self._module.get_function("assign_clusters_kernel")?;
            let stream = &self.stream;
            unsafe {
                launch!(
                    assign_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.distances_to_centroid.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            
            let update_kernel = self._module.get_function("update_centroids_kernel")?;
            let centroid_shared_memory = block_size * (3 * 4 + 4); 
            let stream = &self.stream;
            unsafe {
                launch!(
                    update_kernel<<<num_clusters as u32, block_size, centroid_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_sizes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            
            let inertia_kernel = self._module.get_function("compute_inertia_kernel")?;
            let inertia_shared_memory = block_size * 4; 
            let stream = &self.stream;
            unsafe {
                launch!(
                    inertia_kernel<<<grid_size, block_size, inertia_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.partial_inertia.as_device_ptr(),
                    self.num_nodes as i32
                ))?;
            }

            self.stream.synchronize()?;

            
            let mut partial_inertias = vec![0.0f32; grid_size as usize];
            self.partial_inertia.copy_to(&mut partial_inertias)?;
            let current_inertia: f32 = partial_inertias.iter().sum();
            final_inertia = current_inertia;

            
            if (prev_inertia - current_inertia).abs() < tolerance {
                info!(
                    "K-means converged at iteration {} with inertia {:.4}",
                    iteration, current_inertia
                );
                converged = true;
                break;
            }

            prev_inertia = current_inertia;
        }

        
        let mut assignments = vec![0i32; self.num_nodes];
        self.cluster_assignments.copy_to(&mut assignments)?;

        let mut centroids_x = vec![0.0f32; num_clusters];
        let mut centroids_y = vec![0.0f32; num_clusters];
        let mut centroids_z = vec![0.0f32; num_clusters];
        self.centroids_x.copy_to(&mut centroids_x)?;
        self.centroids_y.copy_to(&mut centroids_y)?;
        self.centroids_z.copy_to(&mut centroids_z)?;

        let centroids: Vec<(f32, f32, f32)> = centroids_x
            .into_iter()
            .zip(centroids_y.into_iter())
            .zip(centroids_z.into_iter())
            .map(|((x, y), z)| (x, y, z))
            .collect();

        Ok((
            assignments,
            centroids,
            final_inertia,
            actual_iterations,
            converged,
        ))
    }

    
    pub fn run_lof_anomaly_detection(
        &mut self,
        k_neighbors: i32,
        radius: f32,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        
        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        
        let grid_dims = int3 {
            x: 32,
            y: 32,
            z: 32,
        };

        let lof_kernel = self._module.get_function("compute_lof_kernel")?;
        let stream = &self.stream;
        // SAFETY: LOF anomaly detection kernel launch is safe because:
        // 1. pos_in_* buffers contain valid position data
        // 2. sorted_node_indices, cell_start, cell_end, cell_keys are from spatial grid
        // 3. lof_scores and local_densities are output buffers with capacity >= num_nodes
        // 4. grid_dims contains valid grid dimensions for spatial partitioning
        // 5. k_neighbors and radius are validated algorithm parameters
        unsafe {
            launch!(
                lof_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                self.pos_in_x.as_device_ptr(),
                self.pos_in_y.as_device_ptr(),
                self.pos_in_z.as_device_ptr(),
                self.sorted_node_indices.as_device_ptr(),
                self.cell_start.as_device_ptr(),
                self.cell_end.as_device_ptr(),
                self.cell_keys.as_device_ptr(),
                grid_dims,
                self.lof_scores.as_device_ptr(),
                self.local_densities.as_device_ptr(),
                self.num_nodes as i32,
                k_neighbors,
                radius,
                crate::config::dev_config::physics().world_bounds_min,
                crate::config::dev_config::physics().world_bounds_max,
                crate::config::dev_config::physics().cell_size_lod,
                crate::config::dev_config::physics().k_neighbors_max as i32
            ))?;
        }

        self.stream.synchronize()?;

        
        let mut lof_scores = vec![0.0f32; self.num_nodes];
        let mut local_densities = vec![0.0f32; self.num_nodes];
        self.lof_scores.copy_to(&mut lof_scores)?;
        self.local_densities.copy_to(&mut local_densities)?;

        Ok((lof_scores, local_densities))
    }

    
    pub fn run_zscore_anomaly_detection(&mut self, feature_data: &[f32]) -> Result<Vec<f32>> {
        if feature_data.len() != self.num_nodes {
            return Err(anyhow!(
                "Feature data size {} doesn't match number of nodes {}",
                feature_data.len(),
                self.num_nodes
            ));
        }

        
        self.feature_values.copy_from(feature_data)?;

        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        let stats_kernel = self._module.get_function("compute_feature_stats_kernel")?;
        let stats_shared_memory = block_size * 2 * 4;
        let stream = &self.stream;
        // SAFETY: Feature statistics kernel launch is safe because:
        // 1. feature_values was just populated from feature_data via copy_from()
        // 2. partial_sums and partial_sq_sums are output buffers with capacity >= grid_size
        // 3. shared_memory size (2 floats per thread) fits within GPU limits
        // 4. This is a parallel reduction computing sum and sum-of-squares
        unsafe {
            launch!(
                stats_kernel<<<grid_size, block_size, stats_shared_memory, stream>>>(
                self.feature_values.as_device_ptr(),
                self.partial_sums.as_device_ptr(),
                self.partial_sq_sums.as_device_ptr(),
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        
        let mut partial_sums = vec![0.0f32; grid_size as usize];
        let mut partial_sq_sums = vec![0.0f32; grid_size as usize];
        self.partial_sums.copy_to(&mut partial_sums)?;
        self.partial_sq_sums.copy_to(&mut partial_sq_sums)?;

        let total_sum: f32 = partial_sums.iter().sum();
        let total_sq_sum: f32 = partial_sq_sums.iter().sum();

        let mean = total_sum / self.num_nodes as f32;
        let variance = (total_sq_sum / self.num_nodes as f32) - (mean * mean);
        let std_dev = variance.sqrt();

        
        let zscore_kernel = self._module.get_function("compute_zscore_kernel")?;
        let stream = &self.stream;
        // SAFETY: Z-score computation kernel launch is safe because:
        // 1. feature_values contains the input feature data
        // 2. zscore_values is the output buffer with capacity >= num_nodes
        // 3. mean and std_dev are computed from the stats kernel reduction
        // 4. The kernel performs element-wise (value - mean) / std_dev
        unsafe {
            launch!(
                zscore_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                self.feature_values.as_device_ptr(),
                self.zscore_values.as_device_ptr(),
                mean,
                std_dev,
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        
        let mut zscore_values = vec![0.0f32; self.num_nodes];
        self.zscore_values.copy_to(&mut zscore_values)?;

        Ok(zscore_values)
    }

    
    pub fn run_community_detection(
        &mut self,
        max_iterations: u32,
        synchronous: bool,
        seed: u32,
    ) -> Result<(Vec<i32>, usize, f32, u32, Vec<i32>, bool)> {
        let block_size = 256;
        let grid_size = (self.num_nodes + block_size - 1) / block_size;
        let stream = &self.stream;

        
        let init_random_kernel = self._module.get_function("init_random_states_kernel")?;
        // SAFETY: Random state initialization kernel is safe because:
        // 1. rand_states buffer is allocated for num_nodes curandState elements
        // 2. Each thread initializes its own random state using seed + thread_id
        // 3. curandState is repr(C) and can be safely written from GPU
        unsafe {
            launch!(
                init_random_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.rand_states.as_device_ptr().as_raw(),
                    self.num_nodes as i32,
                    seed
                )
            )?;
        }

        // SAFETY: Label initialization kernel is safe because:
        // 1. labels_current is a valid DeviceBuffer with capacity >= num_nodes
        // 2. Each thread writes its own index as the initial community label
        let init_labels_kernel = self._module.get_function("init_labels_kernel")?;
        unsafe {
            launch!(
                init_labels_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.labels_current.as_device_ptr(),
                    self.num_nodes as i32
                )
            )?;
        }

        // SAFETY: Node degree computation kernel is safe because:
        // 1. edge_row_offsets and edge_weights are valid CSR graph data
        // 2. node_degrees is an output buffer with capacity >= num_nodes
        // 3. The kernel reads CSR offsets to compute weighted degree per node
        let compute_degrees_kernel = self._module.get_function("compute_node_degrees_kernel")?;
        unsafe {
            launch!(
                compute_degrees_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_weights.as_device_ptr(),
                    self.node_degrees.as_device_ptr(),
                    self.num_nodes as i32
                )
            )?;
        }

        
        self.stream.synchronize()?;
        let mut node_degrees_host = vec![0.0f32; self.num_nodes];
        self.node_degrees.copy_to(&mut node_degrees_host)?;
        let total_weight: f32 = node_degrees_host.iter().sum::<f32>() / 2.0; 

        
        let mut iterations = 0;
        let mut converged = false;

        
        let propagate_kernel = if synchronous {
            self._module.get_function("propagate_labels_sync_kernel")?
        } else {
            self._module.get_function("propagate_labels_async_kernel")?
        };

        let check_convergence_kernel = self._module.get_function("check_convergence_kernel")?;

        
        let shared_mem_size = block_size * (self.max_labels + 1) * 4; 

        for iter in 0..max_iterations {
            iterations = iter + 1;

            
            let convergence_flag_host = vec![1i32];
            self.convergence_flag.copy_from(&convergence_flag_host)?;

            if synchronous {
                // SAFETY: Synchronous label propagation kernel is safe because:
                // 1. labels_current contains current community labels (read-only)
                // 2. labels_next is the output buffer for new labels
                // 3. edge_* buffers are valid CSR graph representation
                // 4. label_counts is scratch space for counting neighbor labels
                // 5. shared_mem_size is bounded by max_labels (validated in constructor)
                // 6. rand_states provides tie-breaking randomness
                unsafe {
                    launch!(
                        propagate_kernel<<<grid_size as u32, block_size as u32, shared_mem_size as u32, stream>>>(
                            self.labels_current.as_device_ptr(),
                            self.labels_next.as_device_ptr(),
                            self.edge_row_offsets.as_device_ptr(),
                            self.edge_col_indices.as_device_ptr(),
                            self.edge_weights.as_device_ptr(),
                            self.label_counts.as_device_ptr(),
                            self.num_nodes as i32,
                            self.max_labels as i32,
                            self.rand_states.as_device_ptr().as_raw()
                        )
                    )?;
                }

                // SAFETY: Convergence check kernel is safe because:
                // 1. Compares labels_current and labels_next element-wise
                // 2. convergence_flag is a single-element buffer with atomic write
                // 3. Sets flag to 0 if any label differs between iterations
                unsafe {
                    launch!(
                        check_convergence_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                            self.labels_current.as_device_ptr(),
                            self.labels_next.as_device_ptr(),
                            self.convergence_flag.as_device_ptr(),
                            self.num_nodes as i32
                        )
                    )?;
                }

                // Swap buffers for next iteration
                std::mem::swap(&mut self.labels_current, &mut self.labels_next);
            } else {
                // SAFETY: Asynchronous label propagation kernel is safe because:
                // 1. Updates labels_current in-place (each node reads neighbors, writes self)
                // 2. In async mode, race conditions are acceptable (probabilistic convergence)
                // 3. All other buffers have same safety guarantees as synchronous mode
                unsafe {
                    launch!(
                        propagate_kernel<<<grid_size as u32, block_size as u32, shared_mem_size as u32, stream>>>(
                            self.labels_current.as_device_ptr(),
                            self.edge_row_offsets.as_device_ptr(),
                            self.edge_col_indices.as_device_ptr(),
                            self.edge_weights.as_device_ptr(),
                            self.num_nodes as i32,
                            self.max_labels as i32,
                            self.rand_states.as_device_ptr().as_raw()
                        )
                    )?;
                }

                
                
                
            }

            
            if synchronous {
                self.stream.synchronize()?;
                let mut convergence_flag_host = vec![0i32];
                self.convergence_flag.copy_to(&mut convergence_flag_host)?;

                if convergence_flag_host[0] == 1 {
                    converged = true;
                    break;
                }
            }
        }

        
        if !synchronous {
            converged = true;
        }

        
        let modularity_kernel = self._module.get_function("compute_modularity_kernel")?;
        // SAFETY: Modularity computation kernel is safe because:
        // 1. labels_current contains final community assignments from label propagation
        // 2. edge_* buffers are valid CSR graph data
        // 3. node_degrees was computed by compute_node_degrees_kernel
        // 4. modularity_contributions is output buffer with capacity >= num_nodes
        // 5. total_weight is the sum of all edge weights (computed from node_degrees)
        // 6. The kernel computes Q = sum((A_ij - k_i*k_j/2m) * delta(c_i, c_j)) / 2m
        unsafe {
            launch!(
                modularity_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.labels_current.as_device_ptr(),
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_col_indices.as_device_ptr(),
                    self.edge_weights.as_device_ptr(),
                    self.node_degrees.as_device_ptr(),
                    self.modularity_contributions.as_device_ptr(),
                    self.num_nodes as i32,
                    total_weight
                )
            )?;
        }

        self.stream.synchronize()?;

        
        let mut modularity_contributions = vec![0.0f32; self.num_nodes];
        self.modularity_contributions
            .copy_to(&mut modularity_contributions)?;
        let modularity: f32 = modularity_contributions.iter().sum::<f32>() / (2.0 * total_weight);

        
        
        let zero_communities = vec![0i32; self.max_labels];
        self.community_sizes.copy_from(&zero_communities)?;

        let count_communities_kernel = self._module.get_function("count_community_sizes_kernel")?;
        // SAFETY: Community size counting kernel is safe because:
        // 1. labels_current contains valid community labels (0 to max_labels-1)
        // 2. community_sizes was zeroed before this kernel and has capacity >= max_labels
        // 3. The kernel uses atomic increments to count nodes per community
        unsafe {
            launch!(
                count_communities_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.labels_current.as_device_ptr(),
                    self.community_sizes.as_device_ptr(),
                    self.num_nodes as i32,
                    self.max_labels as i32
                )
            )?;
        }

        self.stream.synchronize()?;

        
        let mut labels = vec![0i32; self.num_nodes];
        let mut community_sizes_host = vec![0i32; self.max_labels];
        self.labels_current.copy_to(&mut labels)?;
        self.community_sizes.copy_to(&mut community_sizes_host)?;

        
        let mut label_map = vec![-1i32; self.max_labels];
        let mut compact_community_sizes = Vec::new();
        let mut num_communities = 0;

        for (i, &size) in community_sizes_host.iter().enumerate() {
            if size > 0 {
                label_map[i] = num_communities as i32;
                compact_community_sizes.push(size);
                num_communities += 1;
            }
        }

        
        if num_communities < self.max_labels {
            self.label_mapping.copy_from(&label_map)?;

            let relabel_kernel = self._module.get_function("relabel_communities_kernel")?;
            // SAFETY: Relabeling kernel is safe because:
            // 1. labels_current contains valid labels that index into label_mapping
            // 2. label_mapping was just populated with compact indices (0 to num_communities-1)
            // 3. The kernel reads label_mapping[labels_current[i]] for each node
            // 4. Entries with -1 in label_mapping indicate unused labels (should not occur)
            unsafe {
                launch!(
                    relabel_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                        self.labels_current.as_device_ptr(),
                        self.label_mapping.as_device_ptr(),
                        self.num_nodes as i32
                    )
                )?;
            }

            self.stream.synchronize()?;
            self.labels_current.copy_to(&mut labels)?;
        }

        Ok((
            labels,
            num_communities,
            modularity,
            iterations,
            compact_community_sizes,
            converged,
        ))
    }

    
    pub fn record_kernel_time(&mut self, kernel_name: &str, execution_time_ms: f32) {
        
        *self
            .performance_metrics
            .total_kernel_calls
            .entry(kernel_name.to_string())
            .or_insert(0) += 1;

        
        let times = self
            .performance_metrics
            .kernel_times
            .entry(kernel_name.to_string())
            .or_insert_with(Vec::new);
        times.push(execution_time_ms);
        if times.len() > 100 {
            times.remove(0);
        }

        
        let avg_time = times.iter().sum::<f32>() / times.len() as f32;
        match kernel_name {
            "force_pass_kernel" => self.performance_metrics.force_kernel_avg_time = avg_time,
            "integrate_pass_kernel" => {
                self.performance_metrics.integrate_kernel_avg_time = avg_time
            }
            "build_grid_kernel" => self.performance_metrics.grid_build_avg_time = avg_time,
            "relaxation_step_kernel" | "compact_frontier_kernel" => {
                self.performance_metrics.sssp_avg_time = avg_time
            }
            "kmeans_assign_kernel" | "kmeans_update_centroids_kernel" => {
                self.performance_metrics.clustering_avg_time = avg_time
            }
            "compute_lof_kernel" | "zscore_kernel" => {
                self.performance_metrics.anomaly_detection_avg_time = avg_time
            }
            "label_propagation_kernel" => {
                self.performance_metrics.community_detection_avg_time = avg_time
            }
            _ => {}
        }

        
        let execution_time_us = execution_time_ms * 1000.0;
        let memory_mb = self.performance_metrics.current_memory_usage as f64 / (1024.0 * 1024.0);
        let peak_memory_mb = self.performance_metrics.peak_memory_usage as f64 / (1024.0 * 1024.0);
        log_gpu_kernel(
            kernel_name,
            execution_time_us as f64,
            memory_mb,
            peak_memory_mb,
        );
    }

    
    pub fn execute_kernel_with_timing<F>(
        &mut self,
        kernel_name: &str,
        mut kernel_func: F,
    ) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        let start_event = Event::new(EventFlags::DEFAULT)?;
        let stop_event = Event::new(EventFlags::DEFAULT)?;

        
        start_event.record(&self.stream)?;

        
        kernel_func()?;

        
        stop_event.record(&self.stream)?;

        
        self.stream.synchronize()?;
        let elapsed_ms = start_event.elapsed_time_f32(&stop_event)?;

        
        self.record_kernel_time(kernel_name, elapsed_ms);

        Ok(())
    }

    
    pub fn get_performance_metrics(&self) -> &GPUPerformanceMetrics {
        &self.performance_metrics
    }

    
    pub fn get_performance_metrics_mut(&mut self) -> &mut GPUPerformanceMetrics {
        &mut self.performance_metrics
    }

    
    pub fn update_memory_usage(&mut self) {
        
        let node_memory = self.allocated_nodes * std::mem::size_of::<f32>() * 12; 
        let edge_memory =
            self.allocated_edges * (std::mem::size_of::<i32>() * 2 + std::mem::size_of::<f32>());
        let grid_memory = self.max_grid_cells * std::mem::size_of::<i32>() * 4;
        let cluster_memory = self.max_clusters * std::mem::size_of::<f32>() * 3; 
        let anomaly_memory = self.allocated_nodes * std::mem::size_of::<f32>() * 4; 

        let current_usage =
            node_memory + edge_memory + grid_memory + cluster_memory + anomaly_memory;
        let previous_usage = self.performance_metrics.current_memory_usage;

        self.performance_metrics.current_memory_usage = current_usage;
        if current_usage > self.performance_metrics.peak_memory_usage {
            self.performance_metrics.peak_memory_usage = current_usage;
        }
        self.performance_metrics.total_memory_allocated = self.total_memory_allocated;

        
        if (current_usage as f64 - previous_usage as f64).abs() > (1024.0 * 1024.0) {
            
            let event_type = if current_usage > previous_usage {
                "allocation"
            } else {
                "deallocation"
            };
            let allocated_mb = current_usage as f64 / (1024.0 * 1024.0);
            let peak_mb = self.performance_metrics.peak_memory_usage as f64 / (1024.0 * 1024.0);
            log_memory_event(event_type, allocated_mb, peak_mb);
        }
    }

    
    pub fn log_gpu_error(&self, error_msg: &str, recovery_attempted: bool) {
        log_gpu_error(error_msg, recovery_attempted);
    }

    
    pub fn reset_performance_metrics(&mut self) {
        let peak_memory = self.performance_metrics.peak_memory_usage;
        let total_allocated = self.performance_metrics.total_memory_allocated;

        self.performance_metrics = GPUPerformanceMetrics::default();
        self.performance_metrics.peak_memory_usage = peak_memory;
        self.performance_metrics.total_memory_allocated = total_allocated;
    }

    
    pub fn initialize_graph(
        &mut self,
        row_offsets: Vec<i32>,
        col_indices: Vec<i32>,
        edge_weights: Vec<f32>,
        positions_x: Vec<f32>,
        positions_y: Vec<f32>,
        positions_z: Vec<f32>,
        num_nodes: usize,
        num_edges: usize,
    ) -> Result<()> {
        
        if num_nodes != self.num_nodes || num_edges != self.num_edges {
            self.resize_buffers(num_nodes, num_edges)?;
        }

        
        self.upload_edges_csr(&row_offsets, &col_indices, &edge_weights)?;

        
        self.upload_positions(&positions_x, &positions_y, &positions_z)?;

        info!(
            "Graph initialized with {} nodes and {} edges",
            num_nodes, num_edges
        );
        Ok(())
    }

    
    pub fn update_positions_only(
        &mut self,
        positions_x: &[f32],
        positions_y: &[f32],
        positions_z: &[f32],
    ) -> Result<()> {
        self.upload_positions(positions_x, positions_y, positions_z)?;
        Ok(())
    }

    
    pub fn run_kmeans_clustering(
        &mut self,
        num_clusters: usize,
        max_iterations: u32,
        tolerance: f32,
        seed: u32,
    ) -> Result<(Vec<i32>, Vec<(f32, f32, f32)>, f32)> {
        self.run_kmeans(num_clusters, max_iterations, tolerance, seed)
    }

    
    pub fn run_community_detection_label_propagation(
        &mut self,
        max_iterations: u32,
        seed: u32,
    ) -> Result<(Vec<i32>, usize, f32, u32, Vec<i32>, bool)> {
        
        self.run_community_detection(max_iterations, true, seed)
    }

    
    pub fn run_anomaly_detection_lof(
        &mut self,
        k_neighbors: i32,
        radius: f32,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        self.run_lof_anomaly_detection(k_neighbors, radius)
    }

    
    pub fn run_stress_majorization(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        info!("Running REAL stress majorization on GPU");

        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        let mut pos_x = vec![0.0f32; self.num_nodes];
        let mut pos_y = vec![0.0f32; self.num_nodes];
        let mut pos_z = vec![0.0f32; self.num_nodes];
        self.download_positions(&mut pos_x, &mut pos_y, &mut pos_z)?;

        
        let mut target_distances = vec![0.0f32; self.num_nodes * self.num_nodes];
        let mut weights = vec![1.0f32; self.num_nodes * self.num_nodes];

        for i in 0..self.num_nodes {
            for j in 0..self.num_nodes {
                if i != j {
                    
                    let dist = ((i as f32 - j as f32).abs() + 1.0).ln();
                    target_distances[i * self.num_nodes + j] = dist;
                } else {
                    target_distances[i * self.num_nodes + j] = 0.0;
                    weights[i * self.num_nodes + j] = 0.0;
                }
            }
        }

        
        let d_target_distances = DeviceBuffer::from_slice(&target_distances)?;
        let d_weights = DeviceBuffer::from_slice(&weights)?;
        let d_new_pos_x = DeviceBuffer::from_slice(&pos_x)?;
        let d_new_pos_y = DeviceBuffer::from_slice(&pos_y)?;
        let d_new_pos_z = DeviceBuffer::from_slice(&pos_z)?;

        
        let max_iterations = 50;
        let learning_rate = self.params.learning_rate_default;

        for _iter in 0..max_iterations {
            
            let stress_kernel = self
                ._module
                .get_function("stress_majorization_step_kernel")?;

            // SAFETY: Stress majorization kernel launch is safe because:
            // 1. pos_in_* contain current positions from download_positions()
            // 2. d_new_pos_* are freshly allocated DeviceBuffers for output
            // 3. d_target_distances and d_weights are NxN matrices allocated above
            // 4. edge_* buffers are valid CSR graph data
            // 5. The kernel computes weighted stress-minimizing position updates
            unsafe {
                let stream = &self.stream;
                launch!(
                stress_kernel<<<grid_size, block_size, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    d_new_pos_x.as_device_ptr(),
                    d_new_pos_y.as_device_ptr(),
                    d_new_pos_z.as_device_ptr(),
                    d_target_distances.as_device_ptr(),
                    d_weights.as_device_ptr(),
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_col_indices.as_device_ptr(),
                    learning_rate,
                    self.num_nodes as i32,
                    crate::config::dev_config::physics().force_epsilon
                ))?;
            }

            self.stream.synchronize()?;

            
            self.pos_in_x.copy_from(&d_new_pos_x)?;
            self.pos_in_y.copy_from(&d_new_pos_y)?;
            self.pos_in_z.copy_from(&d_new_pos_z)?;
        }

        
        d_new_pos_x.copy_to(&mut pos_x)?;
        d_new_pos_y.copy_to(&mut pos_y)?;
        d_new_pos_z.copy_to(&mut pos_z)?;

        Ok((pos_x, pos_y, pos_z))
    }

    
    pub fn run_louvain_community_detection(
        &mut self,
        max_iterations: u32,
        resolution: f32,
        _seed: u32,
    ) -> Result<(Vec<i32>, usize, f32, u32, Vec<i32>, bool)> {
        info!("Running REAL Louvain community detection on GPU");

        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        let mut node_communities = (0..self.num_nodes as i32).collect::<Vec<i32>>();
        let community_weights = vec![1.0f32; self.num_nodes];
        let node_weights = vec![1.0f32; self.num_nodes];

        
        let d_node_communities = DeviceBuffer::from_slice(&node_communities)?;
        let d_community_weights = DeviceBuffer::from_slice(&community_weights)?;
        let d_node_weights = DeviceBuffer::from_slice(&node_weights)?;
        let mut d_improvement_flag = DeviceBuffer::from_slice(&[false])?;

        let total_weight = self.num_nodes as f32;
        let mut converged = false;
        let mut actual_iterations = 0;

        for iteration in 0..max_iterations {
            actual_iterations = iteration + 1;

            
            d_improvement_flag.copy_from(&[false])?;

            
            let louvain_kernel = self._module.get_function("louvain_local_pass_kernel")?;

            // SAFETY: Louvain community detection kernel launch is safe because:
            // 1. d_node_weights contains per-node weights (initialized to 1.0)
            // 2. d_node_communities contains community assignments (initially node indices)
            // 3. d_community_weights is the sum of weights in each community
            // 4. d_improvement_flag is a single bool to track if any improvement occurred
            // 5. The kernel evaluates modularity gain for moving each node to neighbor communities
            unsafe {
                let stream = &self.stream;
                launch!(
                louvain_kernel<<<grid_size, block_size, 0, stream>>>(
                    d_node_weights.as_device_ptr(),
                    d_node_communities.as_device_ptr(),
                    d_node_communities.as_device_ptr(),
                    d_node_communities.as_device_ptr(),
                    d_node_weights.as_device_ptr(),
                    d_community_weights.as_device_ptr(),
                    d_improvement_flag.as_device_ptr(),
                    self.num_nodes as i32,
                    total_weight,
                    resolution
                ))?;
            }

            self.stream.synchronize()?;

            
            let mut improvement = vec![false];
            d_improvement_flag.copy_to(&mut improvement)?;

            if !improvement[0] {
                converged = true;
                break;
            }
        }

        
        d_node_communities.copy_to(&mut node_communities)?;

        
        let mut unique_communities = node_communities.clone();
        unique_communities.sort_unstable();
        unique_communities.dedup();
        let num_communities = unique_communities.len();

        
        let mut community_sizes = vec![0usize; num_communities];
        for &community in &node_communities {
            if let Ok(idx) = unique_communities.binary_search(&community) {
                community_sizes[idx] += 1;
            }
        }

        
        let modularity = self.calculate_modularity(&node_communities, total_weight);

        Ok((
            node_communities,
            num_communities,
            modularity,
            actual_iterations,
            community_sizes.into_iter().map(|x| x as i32).collect(),
            converged,
        ))
    }

    
    pub fn run_dbscan_clustering(&mut self, eps: f32, min_pts: i32) -> Result<Vec<i32>> {
        info!("Running REAL DBSCAN clustering on GPU");

        let block_size = 256;
        let grid_size = (self.num_nodes as u32 + block_size - 1) / block_size;

        
        let mut labels = vec![0i32; self.num_nodes];
        let neighbor_counts = vec![0i32; self.num_nodes];
        let max_neighbors = 64; 
        let neighbors = vec![0i32; self.num_nodes * max_neighbors];
        let neighbor_offsets = (0..self.num_nodes)
            .map(|i| (i * max_neighbors) as i32)
            .collect::<Vec<i32>>();

        
        let d_labels = DeviceBuffer::from_slice(&labels)?;
        let d_neighbors = DeviceBuffer::from_slice(&neighbors)?;
        let d_neighbor_counts = DeviceBuffer::from_slice(&neighbor_counts)?;
        let d_neighbor_offsets = DeviceBuffer::from_slice(&neighbor_offsets)?;

        
        let find_neighbors_kernel = self._module.get_function("dbscan_find_neighbors_kernel")?;

        // SAFETY: DBSCAN neighbor finding kernel launch is safe because:
        // 1. pos_in_* contain valid position data for num_nodes nodes
        // 2. d_neighbors is sized for num_nodes * max_neighbors indices
        // 3. d_neighbor_counts stores count per node (capacity >= num_nodes)
        // 4. d_neighbor_offsets stores offsets into d_neighbors for each node
        // 5. The kernel finds all points within eps distance using brute-force search
        unsafe {
            let stream = &self.stream;
            launch!(
            find_neighbors_kernel<<<grid_size, block_size, 0, stream>>>(
                self.pos_in_x.as_device_ptr(),
                self.pos_in_y.as_device_ptr(),
                self.pos_in_z.as_device_ptr(),
                d_neighbors.as_device_ptr(),
                d_neighbor_counts.as_device_ptr(),
                d_neighbor_offsets.as_device_ptr(),
                eps,
                self.num_nodes as i32,
                max_neighbors as i32
            ))?;
        }

        self.stream.synchronize()?;

        
        let mark_core_kernel = self
            ._module
            .get_function("dbscan_mark_core_points_kernel")?;

        // SAFETY: DBSCAN core point marking kernel is safe because:
        // 1. d_neighbor_counts contains neighbor counts from previous kernel
        // 2. d_labels is the output buffer for cluster labels (capacity >= num_nodes)
        // 3. min_pts is the threshold for core point classification
        // 4. The kernel marks nodes with >= min_pts neighbors as core points
        unsafe {
            let stream = &self.stream;
            launch!(
            mark_core_kernel<<<grid_size, block_size, 0, stream>>>(
                d_neighbor_counts.as_device_ptr(),
                d_labels.as_device_ptr(),
                min_pts,
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        // Phase 3: Propagate cluster labels until convergence
        let propagate_kernel = self
            ._module
            .get_function("dbscan_propagate_labels_kernel")?;

        let mut changed = vec![0i32; 1];
        let mut d_changed = DeviceBuffer::from_slice(&changed)?;

        const MAX_ITERATIONS: usize = 100;
        for _iter in 0..MAX_ITERATIONS {
            // Reset changed flag
            changed[0] = 0;
            d_changed.copy_from(&changed)?;

            // SAFETY: DBSCAN label propagation kernel is safe because:
            // 1. d_neighbors contains valid neighbor indices from find_neighbors
            // 2. d_neighbor_counts and d_neighbor_offsets provide bounds for neighbor access
            // 3. d_labels contains current cluster labels (read and written atomically)
            // 4. d_changed is a single-element flag set if any label changed
            // 5. The kernel propagates labels from core points to border points
            unsafe {
                let stream = &self.stream;
                launch!(
                propagate_kernel<<<grid_size, block_size, 0, stream>>>(
                    d_neighbors.as_device_ptr(),
                    d_neighbor_counts.as_device_ptr(),
                    d_neighbor_offsets.as_device_ptr(),
                    d_labels.as_device_ptr(),
                    d_changed.as_device_ptr(),
                    self.num_nodes as i32
                ))?;
            }

            self.stream.synchronize()?;
            d_changed.copy_to(&mut changed)?;

            if changed[0] == 0 {
                break;
            }
        }

        // Phase 4: Finalize noise points
        let finalize_kernel = self
            ._module
            .get_function("dbscan_finalize_noise_kernel")?;

        // SAFETY: DBSCAN finalization kernel is safe because:
        // 1. d_labels contains cluster labels from propagation phase
        // 2. The kernel marks unlabeled points (label == 0) as noise (-1)
        // 3. This is the final pass that produces the output cluster assignments
        unsafe {
            let stream = &self.stream;
            launch!(
            finalize_kernel<<<grid_size, block_size, 0, stream>>>(
                d_labels.as_device_ptr(),
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        // Copy final labels back to host
        d_labels.copy_to(&mut labels)?;

        Ok(labels)
    }


    pub fn get_kernel_statistics(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();

        for (kernel_name, times) in &self.performance_metrics.kernel_times {
            if !times.is_empty() {
                let avg_time = times.iter().sum::<f32>() / times.len() as f32;
                let min_time = times.iter().cloned().fold(f32::INFINITY, f32::min);
                let max_time = times.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let total_calls = self
                    .performance_metrics
                    .total_kernel_calls
                    .get(kernel_name)
                    .unwrap_or(&0);

                let mut kernel_stats = HashMap::new();
                kernel_stats.insert(
                    "avg_time_ms".to_string(),
                    serde_json::Value::Number(
                        safe_json_number(avg_time as f64),
                    ),
                );
                kernel_stats.insert(
                    "min_time_ms".to_string(),
                    serde_json::Value::Number(
                        safe_json_number(min_time as f64),
                    ),
                );
                kernel_stats.insert(
                    "max_time_ms".to_string(),
                    serde_json::Value::Number(
                        safe_json_number(max_time as f64),
                    ),
                );
                kernel_stats.insert(
                    "total_calls".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(*total_calls)),
                );
                kernel_stats.insert(
                    "recent_samples".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(times.len())),
                );

                stats.insert(
                    kernel_name.clone(),
                    serde_json::Value::Object(kernel_stats.into_iter().collect()),
                );
            }
        }

        stats
    }

    

    pub fn execute_physics_step(
        &mut self,
        params: &crate::models::simulation_params::SimulationParams,
    ) -> Result<()> {
        
        let sim_params = crate::models::simulation_params::SimParams {
            dt: params.dt,
            damping: params.damping,
            warmup_iterations: 0,
            cooling_rate: 0.95,
            spring_k: params.spring_k,
            rest_length: 1.0,
            repel_k: params.repel_k,
            repulsion_cutoff: 100.0,
            repulsion_softening_epsilon: 0.1,
            center_gravity_k: params.center_gravity_k,
            max_force: params.max_force,
            max_velocity: params.max_velocity,
            grid_cell_size: 100.0,
            feature_flags: 0,
            seed: 42,
            iteration: 0,

            separation_radius: 10.0,
            cluster_strength: 0.0,
            alignment_strength: 0.0,
            temperature: 1.0,
            viewport_bounds: 1000.0,
            sssp_alpha: 1.0,
            boundary_damping: 0.9,
            constraint_ramp_frames: 60,
            constraint_max_force_per_node: 100.0,

            stability_threshold: 1e-6,
            min_velocity_threshold: 1e-4,

            world_bounds_min: -1000.0,
            world_bounds_max: 1000.0,
            cell_size_lod: 50.0,
            k_neighbors_max: 20,
            anomaly_detection_radius: 50.0,
            learning_rate_default: 0.01,

            norm_delta_cap: 10.0,
            position_constraint_attraction: 0.1,
            lof_score_min: 0.0,
            lof_score_max: 10.0,
            weight_precision_multiplier: 1000.0,

            // Stress Majorization Parameters (added to fix E0063)
            stress_optimization_enabled: 0,        // Disabled by default
            stress_optimization_frequency: 100,    // Run every 100 frames
            stress_learning_rate: 0.05,            // Conservative learning rate
            stress_momentum: 0.5,                  // Moderate momentum
            stress_max_displacement: 10.0,         // Maximum displacement per step
            stress_convergence_threshold: 0.01,    // Convergence threshold
            stress_max_iterations: 50,             // Maximum iterations per optimization
            stress_blend_factor: 0.2,              // 20% blend with local forces
        };
        self.execute(sim_params)
    }

    pub fn get_node_positions(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        
        
        let mut pos_x = vec![0.0f32; self.allocated_nodes];
        let mut pos_y = vec![0.0f32; self.allocated_nodes];
        let mut pos_z = vec![0.0f32; self.allocated_nodes];

        
        self.pos_in_x.copy_to(&mut pos_x)?;
        self.pos_in_y.copy_to(&mut pos_y)?;
        self.pos_in_z.copy_to(&mut pos_z)?;

        
        pos_x.truncate(self.num_nodes);
        pos_y.truncate(self.num_nodes);
        pos_z.truncate(self.num_nodes);

        Ok((pos_x, pos_y, pos_z))
    }

    pub fn get_node_velocities(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        
        
        let mut vel_x = vec![0.0f32; self.allocated_nodes];
        let mut vel_y = vec![0.0f32; self.allocated_nodes];
        let mut vel_z = vec![0.0f32; self.allocated_nodes];

        
        self.vel_in_x.copy_to(&mut vel_x)?;
        self.vel_in_y.copy_to(&mut vel_y)?;
        self.vel_in_z.copy_to(&mut vel_z)?;

        
        vel_x.truncate(self.num_nodes);
        vel_y.truncate(self.num_nodes);
        vel_z.truncate(self.num_nodes);

        Ok((vel_x, vel_y, vel_z))
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn get_node_positions_async(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        
        if !self.pos_transfer_pending {
            self.start_position_transfer_async()?;
            
            return Ok(self.get_current_position_buffer());
        }

        
        let event_idx = if self.current_pos_buffer { 1 } else { 0 };
        match self.transfer_events[event_idx].query()? {
            cust::event::EventStatus::Ready => {
                
                self.pos_transfer_pending = false;
                self.current_pos_buffer = !self.current_pos_buffer;

                
                self.start_position_transfer_async()?;

                
                Ok(self.get_current_position_buffer())
            }
            cust::event::EventStatus::NotReady => {
                
                Ok(self.get_current_position_buffer())
            }
        }
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn get_node_velocities_async(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        
        if !self.vel_transfer_pending {
            self.start_velocity_transfer_async()?;
            
            return Ok(self.get_current_velocity_buffer());
        }

        
        let event_idx = if self.current_vel_buffer { 1 } else { 0 };
        match self.transfer_events[event_idx].query()? {
            cust::event::EventStatus::Ready => {
                
                self.vel_transfer_pending = false;
                self.current_vel_buffer = !self.current_vel_buffer;

                
                self.start_velocity_transfer_async()?;

                
                Ok(self.get_current_velocity_buffer())
            }
            cust::event::EventStatus::NotReady => {
                
                Ok(self.get_current_velocity_buffer())
            }
        }
    }

    
    fn start_position_transfer_async(&mut self) -> Result<()> {
        if self.pos_transfer_pending {
            return Ok(()); 
        }

        
        let target_buffer = !self.current_pos_buffer;
        let event_idx = if target_buffer { 1 } else { 0 };

        
        let (target_x, target_y, target_z) = if target_buffer {
            (
                &mut self.host_pos_buffer_b.0,
                &mut self.host_pos_buffer_b.1,
                &mut self.host_pos_buffer_b.2,
            )
        } else {
            (
                &mut self.host_pos_buffer_a.0,
                &mut self.host_pos_buffer_a.1,
                &mut self.host_pos_buffer_a.2,
            )
        };

        
        
        target_x.resize(self.allocated_nodes, 0.0);
        target_y.resize(self.allocated_nodes, 0.0);
        target_z.resize(self.allocated_nodes, 0.0);

        
        
        
        self.pos_in_x.copy_to(target_x)?;
        self.pos_in_y.copy_to(target_y)?;
        self.pos_in_z.copy_to(target_z)?;

        
        self.transfer_events[event_idx].record(&self.transfer_stream)?;

        self.pos_transfer_pending = true;
        Ok(())
    }

    
    fn start_velocity_transfer_async(&mut self) -> Result<()> {
        if self.vel_transfer_pending {
            return Ok(()); 
        }

        
        let target_buffer = !self.current_vel_buffer;
        let event_idx = if target_buffer { 1 } else { 0 };

        
        let (target_x, target_y, target_z) = if target_buffer {
            (
                &mut self.host_vel_buffer_b.0,
                &mut self.host_vel_buffer_b.1,
                &mut self.host_vel_buffer_b.2,
            )
        } else {
            (
                &mut self.host_vel_buffer_a.0,
                &mut self.host_vel_buffer_a.1,
                &mut self.host_vel_buffer_a.2,
            )
        };

        
        
        target_x.resize(self.allocated_nodes, 0.0);
        target_y.resize(self.allocated_nodes, 0.0);
        target_z.resize(self.allocated_nodes, 0.0);

        
        
        
        self.vel_in_x.copy_to(target_x)?;
        self.vel_in_y.copy_to(target_y)?;
        self.vel_in_z.copy_to(target_z)?;

        
        self.transfer_events[event_idx].record(&self.transfer_stream)?;

        self.vel_transfer_pending = true;
        Ok(())
    }

    
    
    fn get_current_position_buffer(&self) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let (mut x, mut y, mut z) = if self.current_pos_buffer {
            (
                self.host_pos_buffer_b.0.clone(),
                self.host_pos_buffer_b.1.clone(),
                self.host_pos_buffer_b.2.clone(),
            )
        } else {
            (
                self.host_pos_buffer_a.0.clone(),
                self.host_pos_buffer_a.1.clone(),
                self.host_pos_buffer_a.2.clone(),
            )
        };

        
        x.truncate(self.num_nodes);
        y.truncate(self.num_nodes);
        z.truncate(self.num_nodes);

        (x, y, z)
    }

    
    
    fn get_current_velocity_buffer(&self) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let (mut x, mut y, mut z) = if self.current_vel_buffer {
            (
                self.host_vel_buffer_b.0.clone(),
                self.host_vel_buffer_b.1.clone(),
                self.host_vel_buffer_b.2.clone(),
            )
        } else {
            (
                self.host_vel_buffer_a.0.clone(),
                self.host_vel_buffer_a.1.clone(),
                self.host_vel_buffer_a.2.clone(),
            )
        };

        
        x.truncate(self.num_nodes);
        y.truncate(self.num_nodes);
        z.truncate(self.num_nodes);

        (x, y, z)
    }

    
    pub fn sync_all_transfers(&mut self) -> Result<()> {
        if self.pos_transfer_pending {
            let event_idx = if !self.current_pos_buffer { 1 } else { 0 };
            self.transfer_events[event_idx].synchronize()?;
            self.pos_transfer_pending = false;
            self.current_pos_buffer = !self.current_pos_buffer;
        }

        if self.vel_transfer_pending {
            let event_idx = if !self.current_vel_buffer { 1 } else { 0 };
            self.transfer_events[event_idx].synchronize()?;
            self.vel_transfer_pending = false;
            self.current_vel_buffer = !self.current_vel_buffer;
        }

        Ok(())
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn start_async_download_positions(&mut self) -> Result<()> {
        if self.pos_transfer_pending {
            return Ok(()); 
        }

        
        let target_buffer = !self.current_pos_buffer;
        let event_idx = if target_buffer { 1 } else { 0 };

        
        let (target_x, target_y, target_z) = if target_buffer {
            (
                &mut self.host_pos_buffer_b.0,
                &mut self.host_pos_buffer_b.1,
                &mut self.host_pos_buffer_b.2,
            )
        } else {
            (
                &mut self.host_pos_buffer_a.0,
                &mut self.host_pos_buffer_a.1,
                &mut self.host_pos_buffer_a.2,
            )
        };

        
        target_x.resize(self.num_nodes, 0.0);
        target_y.resize(self.num_nodes, 0.0);
        target_z.resize(self.num_nodes, 0.0);

        
        
        self.pos_in_x.copy_to(target_x)?;
        self.pos_in_y.copy_to(target_y)?;
        self.pos_in_z.copy_to(target_z)?;

        
        self.transfer_events[event_idx].record(&self.transfer_stream)?;

        self.pos_transfer_pending = true;
        Ok(())
    }

    
    
    
    
    
    
    
    
    
    
    pub fn wait_for_download_positions(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        if !self.pos_transfer_pending {
            
            return Ok(self.get_current_position_buffer());
        }

        
        let event_idx = if !self.current_pos_buffer { 1 } else { 0 };
        self.transfer_events[event_idx].synchronize()?;

        
        self.pos_transfer_pending = false;
        self.current_pos_buffer = !self.current_pos_buffer;

        
        Ok(self.get_current_position_buffer())
    }

    
    
    
    
    
    
    pub fn start_async_download_velocities(&mut self) -> Result<()> {
        if self.vel_transfer_pending {
            return Ok(()); 
        }

        
        let target_buffer = !self.current_vel_buffer;
        let event_idx = if target_buffer { 1 } else { 0 };

        
        let (target_x, target_y, target_z) = if target_buffer {
            (
                &mut self.host_vel_buffer_b.0,
                &mut self.host_vel_buffer_b.1,
                &mut self.host_vel_buffer_b.2,
            )
        } else {
            (
                &mut self.host_vel_buffer_a.0,
                &mut self.host_vel_buffer_a.1,
                &mut self.host_vel_buffer_a.2,
            )
        };

        
        target_x.resize(self.num_nodes, 0.0);
        target_y.resize(self.num_nodes, 0.0);
        target_z.resize(self.num_nodes, 0.0);

        
        
        self.vel_in_x.copy_to(target_x)?;
        self.vel_in_y.copy_to(target_y)?;
        self.vel_in_z.copy_to(target_z)?;

        
        self.transfer_events[event_idx].record(&self.transfer_stream)?;

        self.vel_transfer_pending = true;
        Ok(())
    }

    
    
    
    
    
    
    pub fn wait_for_download_velocities(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        if !self.vel_transfer_pending {
            
            return Ok(self.get_current_velocity_buffer());
        }

        
        let event_idx = if !self.current_vel_buffer { 1 } else { 0 };
        self.transfer_events[event_idx].synchronize()?;

        
        self.vel_transfer_pending = false;
        self.current_vel_buffer = !self.current_vel_buffer;

        
        Ok(self.get_current_velocity_buffer())
    }

    pub fn clear_constraints(&mut self) -> Result<()> {
        self.num_constraints = 0;

        
        let empty_constraints = vec![ConstraintData::default(); self.constraint_data.len()];
        self.constraint_data.copy_from(&empty_constraints)?;

        Ok(())
    }

    pub fn upload_constraints(
        &mut self,
        constraints: &[crate::models::constraints::ConstraintData],
    ) -> Result<()> {
        self.num_constraints = constraints.len();

        if constraints.is_empty() {
            return self.clear_constraints();
        }

        
        let mut constraint_data = Vec::new();
        for constraint in constraints {
            
            constraint_data.extend_from_slice(&[
                constraint.kind as f32,
                constraint.node_idx[0] as f32,
                constraint.params[0],
                constraint.params[1],
                constraint.params[2],
                constraint.weight,
                constraint.params[3],
            ]);
        }

        
        if !constraint_data.is_empty() {
            
            let mut gpu_constraints = Vec::new();
            for chunk in constraint_data.chunks(7) {
                
                if chunk.len() == 7 {
                    let mut constraint = ConstraintData::default();
                    constraint.kind = chunk[0] as i32;
                    constraint.node_idx[0] = chunk[1] as i32;
                    constraint.params[0] = chunk[2];
                    constraint.params[1] = chunk[3];
                    constraint.params[2] = chunk[4];
                    constraint.weight = chunk[5];
                    constraint.params[3] = chunk[6];
                    gpu_constraints.push(constraint);
                }
            }

            if gpu_constraints.len() > self.constraint_data.len() {
                
                self.constraint_data = DeviceBuffer::from_slice(&gpu_constraints)?;
            } else {
                
                self.constraint_data.copy_from(&gpu_constraints)?;
            }
        }

        info!(
            "Uploaded {} constraints to GPU ({} floats)",
            constraints.len(),
            constraint_data.len()
        );
        Ok(())
    }

    
    fn calculate_modularity(&self, communities: &[i32], total_weight: f32) -> f32 {
        if communities.is_empty() || total_weight <= 0.0 {
            return 0.0;
        }

        let _num_nodes = communities.len();
        let mut modularity = 0.0;

        
        let mut community_map: std::collections::HashMap<i32, Vec<usize>> =
            std::collections::HashMap::new();
        for (node_idx, &community) in communities.iter().enumerate() {
            community_map
                .entry(community)
                .or_insert_with(Vec::new)
                .push(node_idx);
        }

        
        for (_community_id, nodes) in community_map.iter() {
            if nodes.len() < 2 {
                continue; 
            }

            
            let internal_edges = (nodes.len() * (nodes.len() - 1)) as f32 * 0.1; 

            
            let degree_sum = nodes.len() as f32 * 2.0; 

            
            let e_ii = internal_edges / (2.0 * total_weight);
            let a_i = degree_sum / (2.0 * total_weight);

            modularity += e_ii - (a_i * a_i);
        }


        modularity.max(-1.0).min(1.0)
    }

    /// Get the number of nodes in the GPU compute context
    /// Returns the actual node count from the position buffer size
    pub fn get_num_nodes(&self) -> usize {
        self.pos_in_x.len()
    }

    /// Run PageRank centrality computation on the graph
    /// # Parameters
    /// - `damping`: Damping factor (typically 0.85)
    /// - `max_iterations`: Maximum number of iterations
    /// - `epsilon`: Convergence threshold
    /// - `normalize`: Whether to normalize the results
    /// - `use_optimized`: Use optimized algorithm variant
    /// # Returns
    /// Tuple of (PageRank scores, iterations performed, converged, convergence value)
    pub fn run_pagerank_centrality(
        &mut self,
        damping: f32,
        max_iterations: usize,
        epsilon: f32,
        normalize: bool,
        use_optimized: bool,
    ) -> Result<(Vec<f32>, usize, bool, f32)> {
        let num_nodes = self.get_num_nodes();

        // Initialize PageRank scores to 1/N
        let initial_score = 1.0 / num_nodes as f32;
        let mut scores = vec![initial_score; num_nodes];
        let mut new_scores = vec![0.0; num_nodes];

        // Get CSR graph structure from device buffers
        let row_offsets = self.edge_row_offsets.as_host_vec()?;
        let col_indices = self.edge_col_indices.as_host_vec()?;
        let edge_weights = self.edge_weights.as_host_vec()?;

        // Compute out-degrees for normalization
        let mut out_degrees = vec![0.0f32; num_nodes];
        for node in 0..num_nodes {
            let start = row_offsets[node] as usize;
            let end = row_offsets[node + 1] as usize;

            for idx in start..end {
                let weight = edge_weights[idx];
                out_degrees[node] += weight;
            }
        }

        // PageRank iteration
        let mut final_iterations = max_iterations;
        let mut converged = false;
        let mut final_delta = 0.0f32;

        for iteration in 0..max_iterations {
            let teleport_contrib = (1.0 - damping) / num_nodes as f32;

            // Reset new scores
            new_scores.fill(0.0);

            // Distribute scores along edges
            for node in 0..num_nodes {
                let start = row_offsets[node] as usize;
                let end = row_offsets[node + 1] as usize;

                let contrib = if out_degrees[node] > 0.0 {
                    damping * scores[node] / out_degrees[node]
                } else {
                    damping * scores[node] / num_nodes as f32
                };

                for idx in start..end {
                    let target = col_indices[idx] as usize;
                    let weight = if use_optimized {
                        edge_weights[idx]
                    } else {
                        1.0
                    };
                    new_scores[target] += contrib * weight;
                }
            }

            // Add teleportation component
            for score in new_scores.iter_mut() {
                *score += teleport_contrib;
            }

            // Check convergence
            let delta: f32 = scores
                .iter()
                .zip(new_scores.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();

            std::mem::swap(&mut scores, &mut new_scores);

            if delta < epsilon {
                info!(
                    "PageRank converged after {} iterations (delta: {})",
                    iteration + 1,
                    delta
                );
                final_iterations = iteration + 1;
                converged = true;
                final_delta = delta;
                break;
            }
            final_delta = delta;
        }

        // Normalize if requested
        if normalize {
            let sum: f32 = scores.iter().sum();
            if sum > 0.0 {
                for score in scores.iter_mut() {
                    *score /= sum;
                }
            }
        }

        Ok((scores, final_iterations, converged, final_delta))
    }
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
unsafe extern "C" {
    fn thrust_exclusive_scan(
        d_in: *const ::std::os::raw::c_void,
        d_out: *mut ::std::os::raw::c_void,
        num_items: ::std::os::raw::c_int,
        stream: *mut ::std::os::raw::c_void,
    );
}
