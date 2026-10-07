//! PageRank Actor - Handles PageRank centrality computation on GPU
//!
//! This actor implements the PageRank algorithm using GPU acceleration for efficient
//! centrality computation on large graphs. PageRank is a measure of node importance
//! based on the structure of incoming links.
//!
//! ## Algorithm
//!
//! PageRank uses the power iteration method:
//! ```text
//! PR(v) = (1-d)/N + d * Σ(PR(u)/out_degree(u))
//! ```
//! where:
//! - d = damping factor (typically 0.85)
//! - N = number of nodes
//! - The sum is over all nodes u that link to v
//!
//! ## Features
//!
//! - GPU-accelerated computation using CUDA
//! - Convergence detection with configurable epsilon
//! - Dangling node handling (nodes with no outgoing edges)
//! - Normalization to ensure sum of PageRank values = 1.0
//! - Performance metrics tracking
//!
//! ## Visual Analytics Integration
//!
//! PageRank values can be used to:
//! - Size nodes proportionally to their importance
//! - Color nodes using a gradient (low → high centrality)
//! - Filter/highlight influential nodes
//! - Drive layout forces (important nodes at center)

use actix::prelude::*;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use super::analytics_telemetry::{record_execution, AnalyticsKernel, ExecutionPath};
use super::shared::{GPUState, SharedGPUContext};
use crate::actors::messages::*;

/// PageRank computation parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRankParams {
    /// Damping factor (probability of following a link vs random jump)
    /// Typical value: 0.85
    pub damping_factor: Option<f32>,

    /// Maximum number of iterations
    pub max_iterations: Option<u32>,

    /// Convergence threshold (L1 norm of difference between iterations)
    pub epsilon: Option<f32>,

    /// Whether to normalize results (ensure sum = 1.0)
    pub normalize: Option<bool>,

    /// Use optimized kernel with shared memory
    pub use_optimized: Option<bool>,
}

impl Default for PageRankParams {
    fn default() -> Self {
        Self {
            damping_factor: Some(0.85),
            max_iterations: Some(100),
            epsilon: Some(1e-6),
            normalize: Some(true),
            use_optimized: Some(true),
        }
    }
}

/// PageRank computation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRankResult {
    /// PageRank value for each node (index = node_id)
    pub pagerank_values: Vec<f32>,

    /// Number of iterations performed
    pub iterations: u32,

    /// Whether the algorithm converged
    pub converged: bool,

    /// Final convergence metric (L1 norm of difference)
    pub convergence_value: f32,

    /// Top K most important nodes (sorted by PageRank)
    pub top_nodes: Vec<PageRankNode>,

    /// Statistical summary
    pub stats: PageRankStats,
}

/// Node with its PageRank value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRankNode {
    pub node_id: u32,
    pub pagerank: f32,
    pub rank: usize, // 1-based rank (1 = highest)
}

/// Statistical summary of PageRank results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRankStats {
    pub total_nodes: u32,
    pub max_pagerank: f32,
    pub min_pagerank: f32,
    pub mean_pagerank: f32,
    pub median_pagerank: f32,
    pub std_deviation: f32,
    pub computation_time_ms: u64,
    pub converged: bool,
    pub iterations: u32,
}

/// Type alias for the shared node analytics map: node_id -> NodeAnalytics
type NodeAnalyticsMap = Arc<
    std::sync::RwLock<std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>>,
>;

/// PageRank Actor for GPU-accelerated centrality computation
pub struct PageRankActor {
    /// GPU state and resource management
    gpu_state: GPUState,

    /// Shared GPU context with compute engine
    shared_context: Option<Arc<SharedGPUContext>>,

    /// Last computed PageRank results (cached)
    last_result: Option<PageRankResult>,

    /// Maps GPU buffer index -> actual graph node ID.
    /// Populated lazily from the GPU `node_graph_id` buffer before publishing.
    /// When empty, raw buffer indices are used as-is (backward compat).
    node_id_map: Vec<u32>,

    /// Shared analytics store — populated after PageRank so the binary broadcast
    /// path can embed real centrality@48 values in V3 wire format (ADR-031 D3:
    /// PageRank is the single writer of `centrality`).
    node_analytics: Option<NodeAnalyticsMap>,
}

impl PageRankActor {
    pub fn new() -> Self {
        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            last_result: None,
            node_id_map: Vec::new(),
            node_analytics: None,
        }
    }

    /// Download the buffer_index -> graph_node_id mapping from the GPU
    /// `node_graph_id` DeviceBuffer. Caches the result in `self.node_id_map`.
    fn ensure_node_id_map(&mut self) {
        if !self.node_id_map.is_empty() {
            return;
        }
        if let Some(ref ctx) = self.shared_context {
            if let Ok(uc) = ctx.unified_compute.lock() {
                let n = uc.num_nodes;
                if n > 0 {
                    let alloc_n = uc.node_graph_id.len();
                    let mut ids = vec![0i32; alloc_n];
                    use cust::memory::CopyDestination;
                    if uc.node_graph_id.copy_to(&mut ids).is_ok() {
                        ids.truncate(n);
                        let has_real_ids = ids.iter().any(|&id| id != 0);
                        if has_real_ids {
                            self.node_id_map = ids.iter().map(|&id| id as u32).collect();
                            info!(
                                "PageRankActor: Downloaded node_id_map ({} entries) from GPU",
                                self.node_id_map.len()
                            );
                        }
                    }
                }
                if self.gpu_state.num_nodes == 0 && n > 0 {
                    self.gpu_state.num_nodes = n as u32;
                }
            }
        }
    }

    /// Translate a GPU buffer index to the actual graph node ID.
    /// Falls back to the raw index if no mapping is available.
    #[inline]
    fn translate_gpu_index(&self, gpu_index: usize) -> u32 {
        if gpu_index < self.node_id_map.len() {
            self.node_id_map[gpu_index]
        } else {
            gpu_index as u32
        }
    }

    /// ADR-031 D3: write PageRank centrality into the shared node_analytics map.
    /// PageRank is the SOLE writer of `centrality@48`. Values are normalised to
    /// [0,1] by the per-run maximum so the wire slot is self-descriptive; the
    /// client ramp (GemNodes) divides by its own observed max, leaving the ramp
    /// unchanged. Entries are reset to 0 first so nodes absent from this run do
    /// not retain a stale centrality.
    fn publish_centrality(&mut self, pagerank_values: &[f32]) {
        self.ensure_node_id_map();
        let analytics_map = match &self.node_analytics {
            Some(m) => m.clone(),
            None => return,
        };
        let max_pr = pagerank_values.iter().cloned().fold(0.0f32, f32::max);
        let inv_max = if max_pr > 0.0 { 1.0 / max_pr } else { 0.0 };

        // Resolve (masked node_id, normalised centrality) pairs before taking the
        // write lock — translate_gpu_index borrows &self, kept out of the guard scope.
        let updates: Vec<(u32, f32)> = pagerank_values
            .iter()
            .enumerate()
            .map(|(gpu_idx, &pr)| {
                let node_id =
                    self.translate_gpu_index(gpu_idx) & crate::utils::binary_protocol::NODE_ID_MASK;
                (node_id, (pr * inv_max).clamp(0.0, 1.0))
            })
            .collect();

        let Ok(mut map) = analytics_map.write() else {
            return;
        };
        for entry in map.values_mut() {
            entry.centrality = 0.0;
        }
        for (node_id, normalized) in &updates {
            map.entry(*node_id).or_default().centrality = *normalized;
        }
        info!(
            "PageRankActor: Populated node_analytics with centrality for {} nodes (max_pr {:.6})",
            updates.len(),
            max_pr
        );
    }

    /// Calculate statistics from PageRank values
    fn calculate_statistics(
        &self,
        values: &[f32],
        iterations: u32,
        converged: bool,
        computation_time_ms: u64,
    ) -> PageRankStats {
        if values.is_empty() {
            return PageRankStats {
                total_nodes: 0,
                max_pagerank: 0.0,
                min_pagerank: 0.0,
                mean_pagerank: 0.0,
                median_pagerank: 0.0,
                std_deviation: 0.0,
                computation_time_ms,
                converged,
                iterations,
            };
        }

        let total_nodes = values.len() as u32;
        let max_pagerank = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let min_pagerank = values.iter().cloned().fold(f32::INFINITY, f32::min);
        let sum: f32 = values.iter().sum();
        let mean_pagerank = sum / total_nodes as f32;

        // Calculate median
        let mut sorted_values = values.to_vec();
        sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median_pagerank = if sorted_values.len().is_multiple_of(2) {
            let mid = sorted_values.len() / 2;
            (sorted_values[mid - 1] + sorted_values[mid]) / 2.0
        } else {
            sorted_values[sorted_values.len() / 2]
        };

        // Calculate standard deviation
        let variance: f32 = values
            .iter()
            .map(|&v| {
                let diff = v - mean_pagerank;
                diff * diff
            })
            .sum::<f32>()
            / total_nodes as f32;
        let std_deviation = variance.sqrt();

        PageRankStats {
            total_nodes,
            max_pagerank,
            min_pagerank,
            mean_pagerank,
            median_pagerank,
            std_deviation,
            computation_time_ms,
            converged,
            iterations,
        }
    }

    /// Extract top K nodes sorted by PageRank
    fn extract_top_nodes(&self, values: &[f32], k: usize) -> Vec<PageRankNode> {
        let mut nodes_with_values: Vec<(u32, f32)> = values
            .iter()
            .enumerate()
            .map(|(idx, &val)| (idx as u32, val))
            .collect();

        // Sort by PageRank descending
        nodes_with_values
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top K
        nodes_with_values
            .iter()
            .take(k.min(values.len()))
            .enumerate()
            .map(|(rank, &(node_id, pagerank))| PageRankNode {
                node_id,
                pagerank,
                rank: rank + 1,
            })
            .collect()
    }

    /// Get cached PageRank results
    fn get_cached_result(&self) -> Option<PageRankResult> {
        self.last_result.clone()
    }

    /// Clear cached results
    fn clear_cache(&mut self) {
        self.last_result = None;
    }
}

impl Default for PageRankActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for PageRankActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("PageRankActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("PageRankActor stopped");
    }
}

// Message handler for computing PageRank
impl Handler<ComputePageRank> for PageRankActor {
    type Result = ResponseActFuture<Self, Result<PageRankResult, String>>;

    fn handle(&mut self, msg: ComputePageRank, _ctx: &mut Context<Self>) -> Self::Result {
        info!("PageRankActor: Received ComputePageRank message");

        let params = msg.params.unwrap_or_default();

        // Get shared context before async boundary
        let shared_ctx = match &self.shared_context {
            Some(ctx) => Arc::clone(ctx),
            None => {
                return Box::pin(
                    async { Err("GPU context not initialized".to_string()) }.into_actor(self),
                );
            }
        };

        // Create the async computation future
        let future = async move {
            // Clone Arc for move into spawn_blocking
            let unified_compute_arc = Arc::clone(&shared_ctx.unified_compute);

            // Move blocking GPU operations to dedicated blocking thread pool
            // This prevents std::sync::Mutex::lock() from blocking Tokio worker threads
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match unified_compute_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("PageRankActor: GPU mutex was poisoned, recovering");
                        poisoned.into_inner()
                    }
                };

                let start_time = Instant::now();

                // Extract parameters with defaults
                let damping = params.damping_factor.unwrap_or(0.85);
                let max_iter = params.max_iterations.unwrap_or(100) as usize;
                let epsilon = params.epsilon.unwrap_or(1e-6);
                let normalize = params.normalize.unwrap_or(true);
                let use_optimized = params.use_optimized.unwrap_or(true);

                // Call GPU PageRank computation
                let gpu_result = unified_compute
                    .run_pagerank_centrality(damping, max_iter, epsilon, normalize, use_optimized)
                    .map_err(|e| {
                        error!("GPU PageRank computation failed: {}", e);
                        format!("PageRank computation failed: {}", e)
                    })?;

                // Task #74: GPU-only path. record_execution uses process-global atomics
                // so it is safe to call inside this spawn_blocking closure.
                record_execution(AnalyticsKernel::Pagerank, ExecutionPath::Gpu);

                let computation_time = start_time.elapsed();
                info!(
                    "PageRankActor: PageRank computation completed in {:?}",
                    computation_time
                );

                // Unpack GPU result and convert iterations to u32 for PageRankResult
                let (pagerank_values, iterations, converged, convergence_value) = gpu_result;
                let iterations = iterations as u32;

                Ok((
                    pagerank_values,
                    iterations,
                    converged,
                    convergence_value,
                    computation_time,
                ))
            })
            .await;

            // Handle spawn_blocking join result
            match blocking_result {
                Ok(inner_result) => inner_result,
                Err(join_err) => Err(format!("GPU blocking task panicked: {}", join_err)),
            }
        };

        // Use into_actor to re-enter actor context and finish processing
        Box::pin(future.into_actor(self).map(|result, actor, _ctx| {
            match result {
                Ok((
                    pagerank_values,
                    iterations,
                    converged,
                    convergence_value,
                    computation_time,
                )) => {
                    // Compute statistics in actor context
                    let stats = actor.calculate_statistics(
                        &pagerank_values,
                        iterations,
                        converged,
                        computation_time.as_millis() as u64,
                    );

                    // Extract top K nodes (top 10 by default)
                    let top_nodes = actor.extract_top_nodes(&pagerank_values, 10);

                    let result = PageRankResult {
                        pagerank_values,
                        iterations,
                        converged,
                        convergence_value,
                        top_nodes,
                        stats,
                    };

                    // Cache the result
                    actor.last_result = Some(result.clone());
                    actor.gpu_state.record_utilization(0.8);

                    // ADR-031 D3: PageRank is the single writer of centrality@48.
                    actor.publish_centrality(&result.pagerank_values);

                    Ok(result)
                }
                Err(e) => Err(e),
            }
        }))
    }
}

// Message handler for getting cached PageRank results
impl Handler<GetPageRankResult> for PageRankActor {
    type Result = MessageResult<GetPageRankResult>;

    fn handle(&mut self, _msg: GetPageRankResult, _ctx: &mut Context<Self>) -> Self::Result {
        MessageResult(self.get_cached_result())
    }
}

// Message handler for clearing PageRank cache
impl Handler<ClearPageRankCache> for PageRankActor {
    type Result = MessageResult<ClearPageRankCache>;

    fn handle(&mut self, _msg: ClearPageRankCache, _ctx: &mut Context<Self>) -> Self::Result {
        self.clear_cache();
        MessageResult(())
    }
}

// Message handler for updating GPU context
impl Handler<SetSharedGPUContext> for PageRankActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Context<Self>) -> Self::Result {
        info!("PageRankActor: Updating GPU context");
        self.shared_context = Some(msg.context);
        self.gpu_state.is_initialized = true;
        Ok(())
    }
}

// Message handler for receiving the shared node_analytics map (ADR-031 D3)
impl Handler<SetNodeAnalytics> for PageRankActor {
    type Result = ();

    fn handle(&mut self, msg: SetNodeAnalytics, _ctx: &mut Context<Self>) {
        info!("PageRankActor: Received shared node_analytics map");
        self.node_analytics = Some(msg.node_analytics);
    }
}

// Message handler for initializing actor
impl Handler<InitializeActor> for PageRankActor {
    type Result = ();

    fn handle(&mut self, _msg: InitializeActor, _ctx: &mut Context<Self>) {
        info!("PageRankActor: Actor initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagerank_params_default() {
        let params = PageRankParams::default();
        assert_eq!(params.damping_factor, Some(0.85));
        assert_eq!(params.max_iterations, Some(100));
        assert!(params.epsilon.unwrap() < 1e-5);
        assert_eq!(params.normalize, Some(true));
    }

    #[test]
    fn test_extract_top_nodes() {
        let actor = PageRankActor::new();
        let values = vec![0.1, 0.5, 0.2, 0.8, 0.3];
        let top = actor.extract_top_nodes(&values, 3);

        assert_eq!(top.len(), 3);
        assert_eq!(top[0].node_id, 3); // Index 3 has value 0.8
        assert_eq!(top[0].rank, 1);
        assert_eq!(top[1].node_id, 1); // Index 1 has value 0.5
        assert_eq!(top[1].rank, 2);
    }

    #[test]
    fn test_calculate_statistics() {
        let actor = PageRankActor::new();
        let values = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let stats = actor.calculate_statistics(&values, 10, true, 100);

        assert_eq!(stats.total_nodes, 5);
        assert_eq!(stats.max_pagerank, 0.5);
        assert_eq!(stats.min_pagerank, 0.1);
        assert!((stats.mean_pagerank - 0.3).abs() < 0.001);
        assert_eq!(stats.median_pagerank, 0.3);
        assert!(stats.converged);
        assert_eq!(stats.iterations, 10);
    }

    #[test]
    fn test_publish_centrality_normalises_and_resets_stale() {
        use crate::utils::binary_protocol::NodeAnalytics;
        use std::collections::HashMap;
        use std::sync::{Arc, RwLock};

        let mut actor = PageRankActor::new();

        // Seed: node 99 is absent from this run (its stale centrality must reset to
        // 0), and node 0 carries a stale value that must be overwritten.
        let mut seed: HashMap<u32, NodeAnalytics> = HashMap::new();
        seed.insert(
            0,
            NodeAnalytics {
                centrality: 0.9,
                ..Default::default()
            },
        );
        seed.insert(
            99,
            NodeAnalytics {
                centrality: 0.7,
                ..Default::default()
            },
        );
        let shared = Arc::new(RwLock::new(seed));
        actor.node_analytics = Some(shared.clone());

        // shared_context is None -> ensure_node_id_map no-ops -> raw indices used as
        // node_ids. Per-run max is 0.4, so values normalise by /0.4.
        actor.publish_centrality(&[0.1, 0.4, 0.2]);

        let map = shared.read().unwrap();
        assert!((map[&0].centrality - 0.25).abs() < 1e-6, "node 0 = 0.1/0.4");
        assert!((map[&1].centrality - 1.0).abs() < 1e-6, "node 1 = 0.4/0.4");
        assert!((map[&2].centrality - 0.5).abs() < 1e-6, "node 2 = 0.2/0.4");
        assert_eq!(map[&99].centrality, 0.0, "stale node reset to 0");
    }

    #[test]
    fn test_publish_centrality_all_zero_pagerank() {
        use crate::utils::binary_protocol::NodeAnalytics;
        use std::collections::HashMap;
        use std::sync::{Arc, RwLock};

        let mut actor = PageRankActor::new();
        let mut seed: HashMap<u32, NodeAnalytics> = HashMap::new();
        seed.insert(
            0,
            NodeAnalytics {
                centrality: 0.5,
                ..Default::default()
            },
        );
        let shared = Arc::new(RwLock::new(seed));
        actor.node_analytics = Some(shared.clone());

        // max_pr == 0 -> inv_max == 0 -> every node normalises to 0 (no div-by-zero).
        actor.publish_centrality(&[0.0, 0.0]);

        let map = shared.read().unwrap();
        assert_eq!(map[&0].centrality, 0.0);
        assert_eq!(map[&1].centrality, 0.0);
    }
}
