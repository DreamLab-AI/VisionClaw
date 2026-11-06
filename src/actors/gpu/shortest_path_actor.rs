//! Shortest Path Actor - Handles SSSP and APSP computations on GPU
//!
//! This actor wraps the existing GPU kernels for:
//! - Single-Source Shortest Path (SSSP) using Bellman-Ford-based frontier compaction
//! - All-Pairs Shortest Path (APSP) using landmark-based approximation
//!
//! Use cases:
//! - Path highlighting in graph visualization
//! - Route visualization for navigation
//! - Connectivity analysis
//! - Distance-based graph analytics

use actix::prelude::*;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use super::analytics_telemetry::{record_execution, AnalyticsKernel, ExecutionPath};
use super::shared::{GPUState, SharedGPUContext};
use crate::actors::messages::*;

/// SSSP computation parameters
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "Result<SSSPResult, String>")]
pub struct ComputeSSP {
    /// Source node index for SSSP computation
    pub source_idx: usize,
    /// Optional maximum distance cutoff
    pub max_distance: Option<f32>,
    /// Optional delta-stepping bucket width.  When `Some(d)`, edges are relaxed
    /// in distance buckets of width `d` instead of all-at-once (Bellman-Ford).
    /// Smaller deltas reduce work per iteration at the cost of more iterations.
    #[serde(default)]
    pub delta: Option<f32>,
    /// Unread. It asked for the run to be written to the shared `node_sssp` map behind wire
    /// slot 28; that map had no live reader and was removed (2026-11-06), so no run is ever
    /// shared. Its callers (`graph_access::stream_safe`) are removed with it separately.
    #[serde(default)]
    pub publish: bool,
}

/// APSP computation parameters
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "Result<APSPResult, String>")]
pub struct ComputeAPSP {
    /// Number of landmark nodes for approximation
    pub num_landmarks: usize,
    /// Optional seed for landmark selection
    pub seed: Option<u64>,
}

/// Query shortest path between two nodes
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "Result<PathResult, String>")]
pub struct QueryPath {
    /// Source node ID
    pub source_id: String,
    /// Target node ID
    pub target_id: String,
}

/// SSSP computation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSSPResult {
    /// Distance from source to each node (indexed by node index)
    pub distances: Vec<f32>,
    /// Source node index
    pub source_idx: usize,
    /// Number of nodes reached
    pub nodes_reached: usize,
    /// Maximum distance found
    pub max_distance: f32,
    /// Computation time in milliseconds
    pub computation_time_ms: u64,
}

/// APSP computation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct APSPResult {
    /// Approximate all-pairs distances [num_nodes x num_nodes]
    /// Stored in row-major order: distance[i][j] = distances[i * num_nodes + j]
    pub distances: Vec<f32>,
    /// Number of nodes
    pub num_nodes: usize,
    /// Number of landmarks used
    pub num_landmarks: usize,
    /// Landmark node indices
    pub landmarks: Vec<usize>,
    /// Average approximation error estimate
    pub avg_error_estimate: f32,
    /// Computation time in milliseconds
    pub computation_time_ms: u64,
}

/// Path query result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathResult {
    /// Path as sequence of node IDs
    pub path: Vec<String>,
    /// Total path distance
    pub distance: f32,
    /// Whether path exists
    pub exists: bool,
}

/// Shortest path computation statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortestPathStats {
    pub total_sssp_computations: u64,
    pub total_apsp_computations: u64,
    pub avg_sssp_time_ms: f32,
    pub avg_apsp_time_ms: f32,
    pub last_computation_time_ms: u64,
}

/// Shortest Path Actor
pub struct ShortestPathActor {
    /// GPU state tracking
    gpu_state: GPUState,

    /// Shared GPU context
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Computation statistics
    stats: ShortestPathStats,
}

impl ShortestPathActor {
    pub fn new() -> Self {
        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            stats: ShortestPathStats {
                total_sssp_computations: 0,
                total_apsp_computations: 0,
                avg_sssp_time_ms: 0.0,
                avg_apsp_time_ms: 0.0,
                last_computation_time_ms: 0,
            },
        }
    }

    /// Update statistics with new computation time
    fn update_stats(&mut self, is_sssp: bool, time_ms: u64) {
        self.stats.last_computation_time_ms = time_ms;

        if is_sssp {
            let total = self.stats.total_sssp_computations as f32;
            self.stats.avg_sssp_time_ms =
                (self.stats.avg_sssp_time_ms * total + time_ms as f32) / (total + 1.0);
            self.stats.total_sssp_computations += 1;
        } else {
            let total = self.stats.total_apsp_computations as f32;
            self.stats.avg_apsp_time_ms =
                (self.stats.avg_apsp_time_ms * total + time_ms as f32) / (total + 1.0);
            self.stats.total_apsp_computations += 1;
        }
    }
}

impl Default for ShortestPathActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for ShortestPathActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("ShortestPathActor started");
        ctx.notify(InitializeActor);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("ShortestPathActor stopped");
    }
}

// Message Handlers

impl Handler<InitializeActor> for ShortestPathActor {
    type Result = ();

    fn handle(&mut self, _msg: InitializeActor, _ctx: &mut Self::Context) -> Self::Result {
        info!("ShortestPathActor: Initializing");
        self.gpu_state.is_initialized = true;
    }
}

impl Handler<SetSharedGPUContext> for ShortestPathActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        info!("ShortestPathActor: Setting GPU context");
        self.shared_context = Some(msg.context);
        self.gpu_state.is_initialized = true;
        Ok(())
    }
}

impl Handler<ComputeSSP> for ShortestPathActor {
    type Result = Result<SSSPResult, String>;

    fn handle(&mut self, msg: ComputeSSP, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ShortestPathActor: Computing SSSP from node {}",
            msg.source_idx
        );

        // Acquire lock, compute, then drop lock before calling update_stats
        let (filtered_distances, nodes_reached, max_distance, computation_time) = {
            let mut unified_compute = match &self.shared_context {
                Some(ctx) => ctx
                    .unified_compute
                    .lock()
                    .map_err(|e| format!("Failed to acquire GPU compute lock: {}", e))?,
                None => {
                    return Err("GPU context not initialized".to_string());
                }
            };

            let start_time = Instant::now();

            // Call the existing GPU SSSP implementation
            let distances = unified_compute
                .run_sssp(msg.source_idx, msg.delta)
                .map_err(|e| {
                    error!("GPU SSSP computation failed: {}", e);
                    format!("SSSP computation failed: {}", e)
                })?;

            // Task #74: SSSP is GPU-only (failure above returns Err; there is no CPU
            // fallback). Record the GPU path on success.
            record_execution(AnalyticsKernel::Sssp, ExecutionPath::Gpu);

            let computation_time = start_time.elapsed().as_millis() as u64;

            // Calculate statistics
            let mut nodes_reached = 0;
            let mut max_distance = 0.0f32;

            for &dist in &distances {
                if dist < f32::MAX {
                    nodes_reached += 1;
                    max_distance = max_distance.max(dist);
                }
            }

            // Apply max_distance filter if specified
            let filtered_distances = if let Some(max_dist) = msg.max_distance {
                distances
                    .into_iter()
                    .map(|d| if d <= max_dist { d } else { f32::MAX })
                    .collect()
            } else {
                distances
            };

            (
                filtered_distances,
                nodes_reached,
                max_distance,
                computation_time,
            )
        }; // unified_compute lock dropped here

        // Now we can safely call update_stats with mutable borrow
        self.update_stats(true, computation_time);

        info!(
            "ShortestPathActor: SSSP completed in {}ms, reached {}/{} nodes",
            computation_time,
            nodes_reached,
            filtered_distances.len()
        );

        Ok(SSSPResult {
            distances: filtered_distances,
            source_idx: msg.source_idx,
            nodes_reached,
            max_distance,
            computation_time_ms: computation_time,
        })
    }
}

impl Handler<ComputeAPSP> for ShortestPathActor {
    type Result = Result<APSPResult, String>;

    fn handle(&mut self, _msg: ComputeAPSP, _ctx: &mut Self::Context) -> Self::Result {
        // Dense all-pairs shortest paths is permanently disabled (ADR-031 D8 /
        // NFR-7). The result is an [n][n] distance matrix — O(n^2) memory
        // (110 MB+ on the live 10,676-node graph, quadratic beyond). Both the
        // GPU kernel (`approximate_apsp_kernel`) and the former CPU fallback
        // were O(n^2) and are removed. Fail closed: callers that need
        // pairwise distance should query single-source SSSP per node instead.
        Err(
            "APSP (dense all-pairs distance matrix) is disabled by NFR-7: O(n^2) \
             memory is forbidden on the analytics path. Use single-source SSSP \
             (POST /api/analytics/pathfinding/sssp) per source instead."
                .to_string(),
        )
    }
}

/// Get shortest path statistics
#[derive(Debug, Clone, Serialize, Deserialize, Message)]
#[rtype(result = "ShortestPathStats")]
pub struct GetShortestPathStats;

impl Handler<GetShortestPathStats> for ShortestPathActor {
    type Result = MessageResult<GetShortestPathStats>;

    fn handle(&mut self, _msg: GetShortestPathStats, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.stats.clone())
    }
}

#[cfg(test)]
mod apsp_verdict_tests {
    use super::*;

    /// Security review H1 (APSP) verdict: the only `ComputeAPSP` handler refuses
    /// unconditionally, so `/analytics/pathfinding/apsp` (pathfinding.rs, via
    /// GraphAnalyticsSupervisor) can return no landmark distances for any caller.
    #[actix_rt::test]
    async fn apsp_refuses_for_every_request() {
        let mut actor = ShortestPathActor::new();
        let mut ctx = Context::new();
        for num_landmarks in [0, 4, 1_000] {
            let r = Handler::handle(
                &mut actor,
                ComputeAPSP {
                    num_landmarks,
                    seed: None,
                },
                &mut ctx,
            );
            assert!(
                r.is_err(),
                "APSP returned data for {num_landmarks} landmarks"
            );
        }
    }
}
