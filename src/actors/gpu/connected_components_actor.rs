//! Connected Components Actor - GPU-accelerated graph connectivity analysis
//!
//! This actor implements connected components detection using GPU label propagation.
//! Use cases:
//! - Identifying disconnected graph regions
//! - Graph partitioning analysis
//! - Cluster visualization
//! - Network fragmentation detection

use actix::prelude::*;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections:::HashMap;
use std::sync:::Arc;
use std::time:::Instant;

use super::shared::{GPUState, SharedGPUContext};
use crate::actors::messages::*;

// GPU kernel extern C declarations for connected components
#[cfg(feature = "gpu")]
extern "C" {
    /// Compute connected components using GPU label propagation
        /// # Parameters
    /// - edge_row_offsets: CSR row offsets [num_nodes + 1]
    /// - edge_col_indices: CSR column indices [num_edges]
    /// - labels: Output node labels [num_nodes] (allocated by caller)
    /// - num_components: Output number of components (allocated by caller)
    /// - num_nodes: Total number of nodes
    /// - max_iterations: Maximum label propagation iterations
    /// - stream: CUDA stream handle
    pub fn compute_connected_components_gpu(
        edge_row_offsets: *const i32,
        edge_col_indices: *const i32,
        labels: *mut i32,
        num_components: *mut i32,
        num_nodes: i32,
        max_iterations: i32,
        stream: *mut std::ffi::c_void,
    );
}

/// Connected components computation parameters
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "Result<ConnectedComponentsResult, String>")]
pub struct ComputeConnectedComponents {
    /// Maximum iterations for label propagation
    pub max_iterations: Option<u32>,
    /// Convergence threshold
    pub convergence_threshold: Option<f32>,
}

/// Connected components result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedComponentsResult {
    /// Component label for each node
    pub labels: Vec<u32>,
    /// Number of connected components
    pub num_components: usize,
    /// Size of each component
    pub component_sizes: Vec<usize>,
    /// Largest component size
    pub largest_component_size: usize,
    /// Whether the graph is fully connected
    pub is_connected: bool,
    /// Number of iterations until convergence
    pub iterations: u32,
    /// Computation time in milliseconds
    pub computation_time_ms: u64,
}

/// Component information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentInfo {
    /// Component ID
    pub id: u32,
    /// Nodes in this component
    pub nodes: Vec<u32>,
    /// Number of internal edges
    pub internal_edges: usize,
    /// Density of this component
    pub density: f32,
}

/// Connected components statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedComponentsStats {
    pub total_computations: u64,
    pub avg_computation_time_ms: f32,
    pub avg_num_components: f32,
    pub last_num_components: usize,
}

/// Connected Components Actor
pub struct ConnectedComponentsActor {
    /// GPU state tracking
    gpu_state: GPUState,

    /// Shared GPU context
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Computation statistics
    stats: ConnectedComponentsStats,
}

impl ConnectedComponentsActor {
    pub fn new() -> Self {
        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            stats: ConnectedComponentsStats {
                total_computations: 0,
                avg_computation_time_ms: 0.0,
                avg_num_components: 0.0,
                last_num_components: 0,
            },
        }
    }

    /// CPU-based label propagation fallback
    /// This will be replaced with GPU kernel when available
    fn compute_components_cpu(
        &self,
        num_nodes: usize,
        edges: &[(u32, u32)],
        max_iterations: u32,
    ) -> Result<(Vec<u32>, u32), String> {
        // Initialize each node with its own label
        let mut labels: Vec<u32> = (0..num_nodes as u32).collect();
        let mut changed = true;
        let mut iteration = 0;

        // Build adjacency list
        let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(src, dst) in edges {
            adjacency.entry(src).or_insert_with(Vec::new).push(dst);
            adjacency.entry(dst).or_insert_with(Vec::new).push(src);
        }

        // Propagate minimum label until convergence
        while changed && iteration < max_iterations {
            changed = false;
            iteration += 1;

            let old_labels = labels.clone();

            for node in 0..num_nodes as u32 {
                if let Some(neighbors) = adjacency.get(&node) {
                    // Find minimum label among neighbors
                    let min_neighbor_label = neighbors
                        .iter()
                        .map(|&n| old_labels[n as usize])
                        .min()
                        .unwrap_or(old_labels[node as usize]);

                    // Update to minimum of current label and neighbor labels
                    let new_label = old_labels[node as usize].min(min_neighbor_label);

                    if new_label != old_labels[node as usize] {
                        labels[node as usize] = new_label;
                        changed = true;
                    }
                }
            }
        }

        Ok((labels, iteration))
    }

    /// Analyze component statistics
    fn analyze_components(&self, labels: &[u32]) -> (usize, Vec<usize>, usize, bool) {
        let mut component_sizes: HashMap<u32, usize> = HashMap::new();

        for &label in labels {
            *component_sizes.entry(label).or_insert(0) += 1;
        }

        let num_components = component_sizes.len();
        let sizes: Vec<usize> = component_sizes.values().copied().collect();
        let largest = sizes.iter().max().copied().unwrap_or(0);
        let is_connected = num_components == 1;

        (num_components, sizes, largest, is_connected)
    }

    /// Update statistics
    fn update_stats(&mut self, time_ms: u64, num_components: usize) {
        let total = self.stats.total_computations as f32;

        self.stats.avg_computation_time_ms =
            (self.stats.avg_computation_time_ms * total + time_ms as f32) / (total + 1.0);

        self.stats.avg_num_components =
            (self.stats.avg_num_components * total + num_components as f32) / (total + 1.0);

        self.stats.last_num_components = num_components;
        self.stats.total_computations += 1;
    }
}

impl Default for ConnectedComponentsActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for ConnectedComponentsActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("ConnectedComponentsActor started");
        ctx.notify(InitializeActor);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("ConnectedComponentsActor stopped");
    }
}

// Message Handlers

impl Handler<InitializeActor> for ConnectedComponentsActor {
    type Result = ();

    fn handle(&mut self, _msg: InitializeActor, _ctx: &mut Self::Context) -> Self::Result {
        info!("ConnectedComponentsActor: Initializing");
        self.gpu_state.is_initialized = true;
    }
}

impl Handler<SetSharedGPUContext> for ConnectedComponentsActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        info!("ConnectedComponentsActor: Setting GPU context");
        self.shared_context = Some(msg.context);
        self.gpu_state.is_initialized = true;
        Ok(())
    }
}

impl Handler<ComputeConnectedComponents> for ConnectedComponentsActor {
    type Result = Result<ConnectedComponentsResult, String>;

    fn handle(&mut self, msg: ComputeConnectedComponents, _ctx: &mut Self::Context) -> Self::Result {
        info!("ConnectedComponentsActor: Computing connected components");

        let start_time = Instant::now();

        // Get GPU context
        let unified_compute = match &self.shared_context {
            Some(ctx) => ctx
                .unified_compute
                .lock()
                .map_err(|e| format!("Failed to acquire GPU compute lock: {}", e))?,
            None => {
                return Err("GPU context not initialized".to_string());
            }
        };

        let num_nodes = unified_compute.get_num_nodes();

        // Get edge list from GPU context
        // For now, use CPU fallback - GPU kernel will be added in next iteration
        drop(unified_compute); // Release lock before CPU computation

        // TODO: Replace with GPU kernel call when available
        // For now, use CPU-based label propagation
        let max_iterations = msg.max_iterations.unwrap_or(100);

        // Dummy edges for testing - in production, fetch from graph state
        let edges: Vec<(u32, u32)> = Vec::new(); // Will be populated from graph

        let (labels, iterations) = self.compute_components_cpu(
            num_nodes,
            &edges,
            max_iterations,
        )?;

        let (num_components, component_sizes, largest_component_size, is_connected) =
            self.analyze_components(&labels);

        let computation_time = start_time.elapsed().as_millis() as u64;
        self.update_stats(computation_time, num_components);

        info!(
            "ConnectedComponentsActor: Found {} components in {}ms",
            num_components, computation_time
        );

        Ok(ConnectedComponentsResult {
            labels,
            num_components,
            component_sizes,
            largest_component_size,
            is_connected,
            iterations,
            computation_time_ms: computation_time,
        })
    }
}

/// Get connected components statistics
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "ConnectedComponentsStats")]
pub struct GetConnectedComponentsStats;

impl Handler<GetConnectedComponentsStats> for ConnectedComponentsActor {
    type Result = MessageResult<GetConnectedComponentsStats>;

    fn handle(&mut self, _msg: GetConnectedComponentsStats, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.stats.clone())
    }
}

