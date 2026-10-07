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
//! ```rust,ignore
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
//! ```rust,ignore
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

// Submodules
mod async_transfer;
mod clustering;
mod community;
mod construction;
mod execution;
mod leiden;
mod memory;
mod metrics;
mod sssp;
mod types;

// Re-export all public types from types module
pub use types::{
    curandState, ComputeMode, GPUPerformanceMetrics, GpuCommunityOutput, GpuKMeansMetricsOutput,
    GpuKMeansOutput,
};

// Re-export the main struct from construction
pub use construction::UnifiedGPUCompute;

// ADR-098 D3: the specialized `ontology_constraints.cu` dispatch path (and its
// GpuOntologyNode / GpuOntologyConstraint / CONSTRAINT_* types) is retired.
// Ontology constraints flow through the generic live force_pass_kernel loop.

// Re-export SimParams (it was `pub use` in the original file)
pub use crate::models::simulation_params::SimParams;
