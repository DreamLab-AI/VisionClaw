//! Clustering Actor - Handles K-means clustering and community detection algorithms

use actix::prelude::*;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use super::analytics_telemetry::{record_execution, AnalyticsKernel, ExecutionPath};
use super::shared::{GPUState, SharedGPUContext};
use crate::actors::messages::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusteringStats {
    pub total_clusters: usize,
    pub average_cluster_size: f32,
    pub largest_cluster_size: usize,
    pub smallest_cluster_size: usize,
    pub silhouette_score: f32,
    pub computation_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityDetectionStats {
    pub total_communities: usize,
    pub modularity: f32,
    pub average_community_size: f32,
    pub largest_community: usize,
    pub smallest_community: usize,
    pub computation_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Community {
    pub id: String,
    pub nodes: Vec<u32>,
    pub internal_edges: usize,
    pub external_edges: usize,
    pub density: f32,
}

/// Type alias for the shared node analytics map: node_id -> NodeAnalytics
type NodeAnalyticsMap = Arc<
    std::sync::RwLock<std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>>,
>;

pub struct ClusteringActor {
    gpu_state: GPUState,

    shared_context: Option<Arc<SharedGPUContext>>,

    /// Maps GPU buffer index -> actual graph node ID.
    /// Populated lazily from the GPU `node_graph_id` buffer before clustering.
    /// When empty, raw buffer indices are used as-is (backward compat).
    node_id_map: Vec<u32>,

    /// Shared analytics store — populated after clustering so the binary broadcast
    /// path can embed real cluster_id values in V3 wire format (ADR-014 DL4 fix).
    node_analytics: Option<NodeAnalyticsMap>,
}

impl ClusteringActor {
    pub fn new() -> Self {
        Self {
            gpu_state: GPUState::default(),
            shared_context: None,
            node_id_map: Vec::new(),
            node_analytics: None,
        }
    }

    /// Download the buffer_index -> graph_node_id mapping from the GPU
    /// `node_graph_id` DeviceBuffer. Caches the result in `self.node_id_map`.
    /// Returns an empty Vec if the GPU context is unavailable.
    fn ensure_node_id_map(&mut self) {
        if !self.node_id_map.is_empty() {
            return;
        }
        if let Some(ref ctx) = self.shared_context {
            if let Ok(uc) = ctx.unified_compute.lock() {
                let n = uc.num_nodes;
                if n > 0 {
                    // Use allocated_nodes for copy_to (device buffer may be overallocated)
                    let alloc_n = uc.node_graph_id.len();
                    let mut ids = vec![0i32; alloc_n];
                    use cust::memory::CopyDestination;
                    if uc.node_graph_id.copy_to(&mut ids).is_ok() {
                        ids.truncate(n);
                        // Check whether the buffer was actually populated
                        // (all zeros means it was never uploaded).
                        let has_real_ids = ids.iter().any(|&id| id != 0);
                        if has_real_ids {
                            self.node_id_map = ids.iter().map(|&id| id as u32).collect();
                            info!(
                                "ClusteringActor: Downloaded node_id_map ({} entries) from GPU",
                                self.node_id_map.len()
                            );
                        }
                    }
                }
                // Also fix gpu_state.num_nodes if it was never set
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

    /// Canonical single writer (ADR-031 D3) for `node_analytics.cluster_id`.
    ///
    /// Applies the invariant write semantics used everywhere cluster ids are
    /// published: a stale-id reset (every entry's `cluster_id` cleared so nodes
    /// dropped from this run revert to 0), then a masked-key write of 1-based
    /// cluster ids (`0 == unclustered`, invariant I-6). `community_id` is a
    /// distinct field and is never touched here.
    ///
    /// `assignments` are `(graph_node_id, cluster_id_1based)` pairs where
    /// `graph_node_id` has NOT yet been masked — masking with `NODE_ID_MASK`
    /// happens here so it always matches the V3 encoder's masked lookup. A
    /// `cluster_id_1based` of 0 contributes nothing beyond the reset (the node
    /// stays unclustered). Returns the number of nodes assigned a non-zero id.
    fn write_cluster_id_from_assignments(
        &self,
        map: &mut std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
        assignments: &[(u32, u32)],
    ) -> usize {
        // Single-writer reset: clear stale cluster_id from any prior run.
        for entry in map.values_mut() {
            entry.cluster_id = 0;
        }
        let mut assigned = 0usize;
        for &(graph_node_id, cluster_id_1based) in assignments {
            if cluster_id_1based == 0 {
                continue;
            }
            // Masked key to match the V3 encoder lookup (NODE_ID_MASK); without
            // it every lookup misses and cluster_id silently stays 0.
            let node_id = graph_node_id & crate::utils::binary_protocol::NODE_ID_MASK;
            let entry = map.entry(node_id).or_default();
            entry.cluster_id = cluster_id_1based;
            assigned += 1;
        }
        assigned
    }

    fn generate_cluster_keywords(nodes: &[u32]) -> Vec<String> {
        if nodes.is_empty() {
            return vec!["empty".to_string()];
        }

        let mut keywords = Vec::new();
        match nodes.len() {
            1 => keywords.push("singleton".to_string()),
            2..=5 => keywords.push("small".to_string()),
            6..=20 => keywords.push("medium".to_string()),
            _ => keywords.push("large".to_string()),
        }

        keywords.push(format!("cluster-{}", nodes[0] % 10));
        keywords
    }

    async fn perform_kmeans_clustering(
        &mut self,
        params: KMeansParams,
    ) -> Result<KMeansResult, String> {
        info!(
            "ClusteringActor: Starting K-means clustering with {} clusters",
            params.num_clusters
        );

        let unified_compute_arc = match &self.shared_context {
            Some(ctx) => Arc::clone(&ctx.unified_compute),
            None => {
                return Err("GPU context not initialized".to_string());
            }
        };

        let num_clusters = params.num_clusters;
        let max_iterations = params.max_iterations.unwrap_or(100);
        let tolerance = params.tolerance.unwrap_or(0.001);
        let seed = params.seed.unwrap_or(42);

        // Deadlock hardening: refuse a degenerate request before touching the GPU
        // so the shared `unified_compute` lock is never held across a kernel that
        // would wedge the CUDA stream (see MIN_NODES_FOR_CLUSTERING).
        if num_clusters == 0 {
            return Err("K-means clustering refused: num_clusters must be >= 1".to_string());
        }

        // Move blocking GPU operations to dedicated blocking thread pool
        // This prevents std::sync::Mutex::lock() from blocking Tokio worker threads
        let blocking_result = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let mut unified_compute = match unified_compute_arc.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("ClusteringActor: GPU mutex was poisoned, recovering");
                    poisoned.into_inner()
                }
            };

            // Precondition guard (inside the lock, before any kernel launch): a
            // zero/degenerate node set or coincident positions would launch the
            // centroid-init kernel with an invalid grid and wedge the shared
            // stream, deadlocking physics. Refuse cleanly — the guard drops here.
            let (sx, sy, sz) = gpu_position_spread(&unified_compute);
            if let Some(reason) = position_clustering_refusal(unified_compute.num_nodes, sx, sy, sz)
            {
                warn!("ClusteringActor: K-means refused before launch: {}", reason);
                return Err(format!("K-means clustering refused: {}", reason));
            }
            if num_clusters > unified_compute.num_nodes {
                return Err(format!(
                    "K-means clustering refused: num_clusters {} exceeds num_nodes {}",
                    num_clusters, unified_compute.num_nodes
                ));
            }

            let start_time = Instant::now();

            let gpu_result = unified_compute
                .run_kmeans_clustering_with_metrics(num_clusters, max_iterations, tolerance, seed)
                .map_err(|e| {
                    error!("GPU K-means clustering failed: {}", e);
                    format!("K-means clustering failed: {}", e)
                })?;

            // Task #74: K-means is GPU-only (failure above returns Err — no CPU
            // substitute). Record the GPU path on success.
            record_execution(AnalyticsKernel::Kmeans, ExecutionPath::Gpu);

            let computation_time = start_time.elapsed();
            info!(
                "ClusteringActor: K-means clustering completed in {:?}",
                computation_time
            );

            Ok((gpu_result, computation_time))
        })
        .await;

        let (gpu_result, computation_time) = match blocking_result {
            Ok(inner_result) => inner_result?,
            Err(join_err) => return Err(format!("GPU blocking task panicked: {}", join_err)),
        };

        let (assignments, centroids, inertia, actual_iterations, converged) = gpu_result;

        // Ensure we have the GPU buffer index -> graph node ID mapping
        self.ensure_node_id_map();

        let clusters = self.convert_gpu_kmeans_result_to_clusters(
            assignments.iter().map(|&x| x as u32).collect(),
            params.num_clusters as u32,
        )?;

        let cluster_sizes: Vec<usize> = clusters.iter().map(|c| c.nodes.len()).collect();
        let avg_cluster_size = if !cluster_sizes.is_empty() {
            cluster_sizes.iter().sum::<usize>() as f32 / cluster_sizes.len() as f32
        } else {
            0.0
        };

        let silhouette_score = if clusters.len() > 1 && !assignments.is_empty() {
            self.calculate_silhouette_score(&assignments, &centroids, &clusters)?
        } else {
            0.0
        };

        let cluster_stats = ClusteringStats {
            total_clusters: clusters.len(),
            average_cluster_size: avg_cluster_size,
            largest_cluster_size: cluster_sizes.iter().max().copied().unwrap_or(0),
            smallest_cluster_size: cluster_sizes.iter().min().copied().unwrap_or(0),
            silhouette_score,
            computation_time_ms: computation_time.as_millis() as u64,
        };

        // ADR-014 DL4 fix: Populate shared node_analytics with cluster assignments
        // so the V3 binary broadcast path carries real cluster_id values. Routes
        // through the single writer (ADR-031 D3) below.
        if let Some(ref analytics_map) = self.node_analytics {
            // Resolve graph node ids for each GPU buffer index. ADR-031 I-6:
            // cluster_id is 1-based with 0 == unclustered; K-means assigns dense
            // 0-based ids to every node, so +1 (negative ids map to 0).
            let pairs: Vec<(u32, u32)> = assignments
                .iter()
                .enumerate()
                .map(|(gpu_idx, &cluster_id)| {
                    let node_id = self.translate_gpu_index(gpu_idx);
                    let id_1based = if cluster_id < 0 {
                        0
                    } else {
                        (cluster_id + 1) as u32
                    };
                    (node_id, id_1based)
                })
                .collect();
            if let Ok(mut map) = analytics_map.write() {
                self.write_cluster_id_from_assignments(&mut map, &pairs);
                info!(
                    "ClusteringActor: Populated node_analytics with cluster assignments for {} nodes",
                    assignments.len()
                );
            }
        }

        Ok(KMeansResult {
            cluster_assignments: assignments,
            centroids,
            inertia,
            iterations: actual_iterations,
            clusters,
            stats: cluster_stats,
            converged,
            final_iteration: actual_iterations,
        })
    }

    async fn perform_community_detection(
        &mut self,
        params: CommunityDetectionParams,
    ) -> Result<CommunityDetectionResult, String> {
        info!(
            "ClusteringActor: Starting {:?} community detection",
            params.algorithm
        );

        let unified_compute_arc = match &self.shared_context {
            Some(ctx) => Arc::clone(&ctx.unified_compute),
            None => {
                return Err("GPU context not initialized".to_string());
            }
        };

        let algorithm = params.algorithm.clone();
        let max_iterations = params.max_iterations.unwrap_or(100);
        let seed = params.seed.unwrap_or(42);
        let resolution = params.resolution.unwrap_or(1.0);

        // Move blocking GPU operations to dedicated blocking thread pool
        // This prevents std::sync::Mutex::lock() from blocking Tokio worker threads
        let blocking_result = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let mut unified_compute = match unified_compute_arc.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("ClusteringActor: GPU mutex was poisoned, recovering");
                    poisoned.into_inner()
                }
            };

            // Deadlock hardening: refuse a degenerate node set before any kernel
            // launch. Community detection is edge-based (no position-spread check),
            // but a zero/near-empty graph still launches kernels with an invalid
            // grid and can wedge the shared GPU context.
            if unified_compute.num_nodes < MIN_NODES_FOR_CLUSTERING {
                warn!(
                    "ClusteringActor: community detection refused before launch: only {} node(s)",
                    unified_compute.num_nodes
                );
                return Err(format!(
                    "community detection refused: only {} GPU node(s); need >= {}",
                    unified_compute.num_nodes, MIN_NODES_FOR_CLUSTERING
                ));
            }

            let start_time = Instant::now();

            let gpu_result = match algorithm {
                CommunityDetectionAlgorithm::LabelPropagation => {
                    let r = unified_compute
                        .run_community_detection_label_propagation(max_iterations, seed)
                        .map_err(|e| {
                            error!("GPU label propagation failed: {}", e);
                            format!("Label propagation failed: {}", e)
                        })?;
                    // Task #74: GPU-only community detection. Record the path on success.
                    record_execution(AnalyticsKernel::LabelPropagation, ExecutionPath::Gpu);
                    r
                }
                CommunityDetectionAlgorithm::Louvain => {
                    let r = unified_compute
                        .run_louvain_community_detection(max_iterations, resolution, seed)
                        .map_err(|e| {
                            error!("GPU Louvain community detection failed: {}", e);
                            format!("Louvain community detection failed: {}", e)
                        })?;
                    // Task #74: GPU-only Louvain. Record the path on success.
                    record_execution(AnalyticsKernel::Louvain, ExecutionPath::Gpu);
                    r
                }
                CommunityDetectionAlgorithm::Leiden => {
                    // Leiden: refined Louvain with a connected-community guarantee —
                    // the same detector that drives the Community Cohesion force, so
                    // analytics coloring matches cohesion grouping.
                    let r = unified_compute
                        .run_leiden_community_detection(max_iterations, resolution, seed)
                        .map_err(|e| {
                            error!("GPU Leiden community detection failed: {}", e);
                            format!("Leiden community detection failed: {}", e)
                        })?;
                    record_execution(AnalyticsKernel::Leiden, ExecutionPath::Gpu);
                    r
                }
            };

            let computation_time = start_time.elapsed();
            info!(
                "ClusteringActor: Community detection completed in {:?}",
                computation_time
            );

            Ok((gpu_result, computation_time))
        })
        .await;

        let (gpu_result, computation_time) = match blocking_result {
            Ok(inner_result) => inner_result?,
            Err(join_err) => return Err(format!("GPU blocking task panicked: {}", join_err)),
        };

        // Ensure we have the GPU buffer index -> graph node ID mapping
        self.ensure_node_id_map();

        let (node_labels, num_communities, modularity, iterations, community_sizes, converged) =
            gpu_result;
        let communities = self.convert_gpu_community_result_to_communities(
            node_labels.iter().map(|&x| x as u32).collect(),
        )?;

        let actual_community_sizes: Vec<usize> =
            communities.iter().map(|c| c.nodes.len()).collect();
        // ADR-031 D1: there is ONE modularity implementation — the canonical Newman
        // Q computed by `modularity_csr` (unified_gpu_compute/community.rs) on the
        // final partition over the original CSR. The GPU detection path already
        // returns exactly that value in `modularity`; the shadow
        // `calculate_modularity` heuristic (which double-counted bridge edges and
        // clamped to [0,1]) has been removed. The stats field and the gate now agree.
        let actual_modularity = modularity;

        // ADR-031 D1 modularity gate: a partition below Q=0.3 has no meaningful
        // community structure (it is statistical noise). Fail closed — leave every
        // node at community_id=0 rather than publishing a spurious partition. The
        // GPU-returned `modularity` is computed on the ORIGINAL graph with the final
        // labels and is authoritative.
        //
        // ADR-031 D3: community detection is the single writer of community_id only.
        // cluster_id is a distinct field (invariant I-6) owned by K-means/DBSCAN and
        // is left untouched here — the legacy dup-write into cluster_id is removed.
        if let Some(ref analytics_map) = self.node_analytics {
            // translate_gpu_index needs &self, so resolve the masked graph node ids
            // here; the gate itself (apply_modularity_gate) is a pure, unit-tested
            // function over the resulting (node_id, label) pairs.
            let node_ids: Vec<u32> = (0..node_labels.len())
                .map(|gpu_idx| {
                    self.translate_gpu_index(gpu_idx) & crate::utils::binary_protocol::NODE_ID_MASK
                })
                .collect();
            if let Ok(mut map) = analytics_map.write() {
                if apply_modularity_gate(&mut map, &node_ids, &node_labels, modularity) {
                    info!(
                        "ClusteringActor: Populated node_analytics with community assignments for {} nodes (modularity {:.4})",
                        node_labels.len(), modularity
                    );
                } else {
                    info!(
                        "ClusteringActor: community partition rejected (modularity {:.4} < {:.2}); \
                         node_analytics.community_id reset to 0",
                        modularity, MODULARITY_GATE
                    );
                }
            }
        }

        let stats = CommunityDetectionStats {
            total_communities: communities.len(),
            modularity: actual_modularity,
            average_community_size: if !actual_community_sizes.is_empty() {
                actual_community_sizes.iter().sum::<usize>() as f32
                    / actual_community_sizes.len() as f32
            } else {
                0.0
            },
            largest_community: actual_community_sizes.iter().max().copied().unwrap_or(0),
            smallest_community: actual_community_sizes.iter().min().copied().unwrap_or(0),
            computation_time_ms: computation_time.as_millis() as u64,
        };

        Ok(CommunityDetectionResult {
            node_labels,
            num_communities,
            modularity,
            iterations,
            community_sizes,
            converged,
            communities,
            stats,
            algorithm: params.algorithm,
        })
    }

    /// Run standalone DBSCAN clustering on the GPU.
    async fn perform_dbscan_clustering(
        &mut self,
        params: DBSCANParams,
    ) -> Result<DBSCANResult, String> {
        info!(
            "ClusteringActor: Starting DBSCAN clustering with epsilon={}, min_points={}",
            params.epsilon, params.min_points
        );

        let unified_compute_arc = match &self.shared_context {
            Some(ctx) => Arc::clone(&ctx.unified_compute),
            None => {
                return Err("GPU context not initialized".to_string());
            }
        };

        let epsilon = params.epsilon;
        let min_points = params.min_points;

        // Deadlock hardening: reject pathological params before any GPU access so
        // the shared `unified_compute` lock is never held across a wedged kernel.
        if !(epsilon.is_finite() && epsilon > 0.0) {
            return Err(format!(
                "DBSCAN clustering refused: epsilon must be finite and > 0 (got {})",
                epsilon
            ));
        }
        if min_points == 0 {
            return Err("DBSCAN clustering refused: min_points must be >= 1".to_string());
        }

        // Move blocking GPU operations to dedicated blocking thread pool
        let blocking_result = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let mut unified_compute = match unified_compute_arc.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("ClusteringActor: GPU mutex was poisoned, recovering");
                    poisoned.into_inner()
                }
            };

            // Precondition guard (inside the lock, before launch): refuse a
            // zero/degenerate node set or coincident positions that would wedge
            // the neighbour-scan kernel and deadlock the shared GPU context.
            let (sx, sy, sz) = gpu_position_spread(&unified_compute);
            if let Some(reason) = position_clustering_refusal(unified_compute.num_nodes, sx, sy, sz)
            {
                warn!("ClusteringActor: DBSCAN refused before launch: {}", reason);
                return Err(format!("DBSCAN clustering refused: {}", reason));
            }

            let start_time = Instant::now();

            let labels = unified_compute
                .run_dbscan_clustering(epsilon, min_points as i32)
                .map_err(|e| {
                    error!("GPU DBSCAN clustering failed: {}", e);
                    format!("DBSCAN clustering failed: {}", e)
                })?;

            // Task #74: DBSCAN is GPU-only (failure above returns Err). Record the path.
            record_execution(AnalyticsKernel::Dbscan, ExecutionPath::Gpu);

            let computation_time = start_time.elapsed();
            info!(
                "ClusteringActor: DBSCAN clustering completed in {:?}",
                computation_time
            );

            Ok((labels, computation_time))
        })
        .await;

        let (labels, computation_time) = match blocking_result {
            Ok(inner_result) => inner_result?,
            Err(join_err) => return Err(format!("GPU blocking task panicked: {}", join_err)),
        };

        // Ensure we have the GPU buffer index -> graph node ID mapping
        self.ensure_node_id_map();

        // Build cluster groups from labels. run_dbscan_clustering returns
        // 1-based contiguous ids with 0 == noise/unclustered (ADR-031 I-6).
        let mut cluster_nodes: std::collections::HashMap<i32, Vec<u32>> =
            std::collections::HashMap::new();
        let mut num_noise = 0usize;

        for (gpu_idx, &label) in labels.iter().enumerate() {
            if label == 0 {
                num_noise += 1;
                continue;
            }
            let graph_node_id = self.translate_gpu_index(gpu_idx);
            cluster_nodes.entry(label).or_default().push(graph_node_id);
        }

        let num_clusters = cluster_nodes.len();

        // Convert to Cluster structs matching the existing pattern
        let mut clusters = Vec::new();
        for (cluster_label, nodes) in &cluster_nodes {
            clusters.push(crate::handlers::api_handler::analytics::Cluster {
                id: Uuid::new_v4().to_string(),
                label: format!("DBSCAN Cluster {}", cluster_label),
                node_count: nodes.len() as u32,
                coherence: self.calculate_cluster_coherence(nodes, &labels),
                color: format!(
                    "#{:02X}{:02X}{:02X}",
                    (*cluster_label as usize * 50 % 255) as u8,
                    (*cluster_label as usize * 100 % 255) as u8,
                    (*cluster_label as usize * 150 % 255) as u8
                ),
                keywords: Self::generate_cluster_keywords(nodes),
                centroid: Some(self.calculate_cluster_centroid(nodes)),
                nodes: nodes.clone(),
            });
        }

        let cluster_sizes: Vec<usize> = clusters.iter().map(|c| c.nodes.len()).collect();

        // Populate shared node_analytics with DBSCAN cluster assignments through
        // the single writer (ADR-031 D3). run_dbscan_clustering returns 1-based
        // ids with 0 == noise/unclustered (ADR-031 I-6); noise keeps the default 0.
        if let Some(ref analytics_map) = self.node_analytics {
            let pairs: Vec<(u32, u32)> = labels
                .iter()
                .enumerate()
                .map(|(gpu_idx, &label)| {
                    let node_id = self.translate_gpu_index(gpu_idx);
                    let id = if label > 0 { label as u32 } else { 0 };
                    (node_id, id)
                })
                .collect();
            if let Ok(mut map) = analytics_map.write() {
                self.write_cluster_id_from_assignments(&mut map, &pairs);
                info!(
                    "ClusteringActor: Populated node_analytics with DBSCAN cluster assignments for {} nodes",
                    labels.len() - num_noise
                );
            }
        }

        let stats = DBSCANStats {
            total_nodes: labels.len(),
            num_clusters,
            num_noise_points: num_noise,
            largest_cluster_size: cluster_sizes.iter().max().copied().unwrap_or(0),
            smallest_cluster_size: cluster_sizes.iter().min().copied().unwrap_or(0),
            average_cluster_size: if !cluster_sizes.is_empty() {
                cluster_sizes.iter().sum::<usize>() as f32 / cluster_sizes.len() as f32
            } else {
                0.0
            },
            computation_time_ms: computation_time.as_millis() as u64,
        };

        Ok(DBSCANResult {
            labels,
            num_clusters,
            num_noise_points: num_noise,
            clusters,
            stats,
        })
    }

    fn convert_gpu_kmeans_result_to_clusters(
        &self,
        gpu_result: Vec<u32>,
        num_clusters: u32,
    ) -> Result<Vec<crate::handlers::api_handler::analytics::Cluster>, String> {
        // gpu_state.num_nodes may lag behind the actual GPU node count when
        // context was set but gpu_state was not forwarded; skip the check when 0.
        if self.gpu_state.num_nodes > 0 && gpu_result.len() != self.gpu_state.num_nodes as usize {
            return Err(format!(
                "GPU result size mismatch: expected {}, got {}",
                self.gpu_state.num_nodes,
                gpu_result.len()
            ));
        }

        let mut cluster_nodes: Vec<Vec<u32>> = vec![Vec::new(); num_clusters as usize];

        for (gpu_idx, &cluster_id) in gpu_result.iter().enumerate() {
            if (cluster_id as usize) < cluster_nodes.len() {
                let graph_node_id = self.translate_gpu_index(gpu_idx);
                cluster_nodes[cluster_id as usize].push(graph_node_id);
            }
        }

        let mut clusters = Vec::new();
        for (cluster_id, nodes) in cluster_nodes.into_iter().enumerate() {
            if !nodes.is_empty() {
                clusters.push(crate::handlers::api_handler::analytics::Cluster {
                    id: Uuid::new_v4().to_string(),
                    label: format!("Cluster {}", cluster_id),
                    node_count: nodes.len() as u32,
                    coherence: {
                        let assignments_i32: Vec<i32> =
                            gpu_result.iter().map(|&x| x as i32).collect();
                        self.calculate_cluster_coherence(&nodes, &assignments_i32)
                    },
                    color: format!(
                        "#{:02X}{:02X}{:02X}",
                        (cluster_id * 50 % 255) as u8,
                        (cluster_id * 100 % 255) as u8,
                        (cluster_id * 150 % 255) as u8
                    ),
                    keywords: Self::generate_cluster_keywords(&nodes),
                    centroid: Some(self.calculate_cluster_centroid(&nodes)),
                    nodes,
                });
            }
        }

        info!(
            "ClusteringActor: Generated {} non-empty clusters",
            clusters.len()
        );
        Ok(clusters)
    }

    fn convert_gpu_community_result_to_communities(
        &self,
        gpu_result: Vec<u32>,
    ) -> Result<Vec<Community>, String> {
        if self.gpu_state.num_nodes > 0 && gpu_result.len() != self.gpu_state.num_nodes as usize {
            return Err(format!(
                "GPU result size mismatch: expected {}, got {}",
                self.gpu_state.num_nodes,
                gpu_result.len()
            ));
        }

        let mut community_nodes: std::collections::HashMap<u32, Vec<u32>> =
            std::collections::HashMap::new();

        for (gpu_idx, &community_id) in gpu_result.iter().enumerate() {
            let graph_node_id = self.translate_gpu_index(gpu_idx);
            community_nodes
                .entry(community_id)
                .or_default()
                .push(graph_node_id);
        }

        let mut communities = Vec::new();
        for (community_id, nodes) in community_nodes {
            let internal_edges = self.calculate_internal_edges(&nodes);
            let external_edges = self.calculate_external_edges(&nodes);
            let density = self.calculate_community_density(&nodes);

            communities.push(Community {
                id: community_id.to_string(),
                nodes,
                internal_edges,
                external_edges,
                density,
            });
        }

        info!(
            "ClusteringActor: Generated {} communities",
            communities.len()
        );
        Ok(communities)
    }

    fn calculate_silhouette_score(
        &self,
        assignments: &[i32],
        centroids: &[(f32, f32, f32)],
        clusters: &[crate::handlers::api_handler::analytics::Cluster],
    ) -> Result<f32, String> {
        if clusters.len() < 2 || assignments.is_empty() {
            return Ok(0.0);
        }

        let mut total_silhouette = 0.0;
        let mut valid_samples = 0;

        for (node_idx, &cluster_id) in assignments.iter().enumerate() {
            if cluster_id < 0 || cluster_id as usize >= centroids.len() {
                continue;
            }

            let own_cluster_nodes: Vec<usize> = assignments
                .iter()
                .enumerate()
                .filter(|(_, &cid)| cid == cluster_id)
                .map(|(idx, _)| idx)
                .collect();

            let intra_cluster_distance = if own_cluster_nodes.len() > 1 {
                let mut total_distance = 0.0;
                let mut count = 0;
                for &other_node in &own_cluster_nodes {
                    if other_node != node_idx {
                        total_distance +=
                            self.calculate_node_distance(node_idx, other_node, centroids);
                        count += 1;
                    }
                }
                if count > 0 {
                    total_distance / count as f32
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let mut min_inter_cluster_distance = f32::INFINITY;
            for other_cluster_id in 0..centroids.len() {
                if other_cluster_id != cluster_id as usize {
                    let other_cluster_nodes: Vec<usize> = assignments
                        .iter()
                        .enumerate()
                        .filter(|(_, &cid)| cid == other_cluster_id as i32)
                        .map(|(idx, _)| idx)
                        .collect();

                    if !other_cluster_nodes.is_empty() {
                        let mut total_distance = 0.0;
                        for &other_node in &other_cluster_nodes {
                            total_distance +=
                                self.calculate_node_distance(node_idx, other_node, centroids);
                        }
                        let avg_distance = total_distance / other_cluster_nodes.len() as f32;
                        min_inter_cluster_distance = min_inter_cluster_distance.min(avg_distance);
                    }
                }
            }

            if min_inter_cluster_distance.is_finite() && intra_cluster_distance.is_finite() {
                let max_distance = intra_cluster_distance.max(min_inter_cluster_distance);
                if max_distance > 0.0 {
                    let silhouette =
                        (min_inter_cluster_distance - intra_cluster_distance) / max_distance;
                    total_silhouette += silhouette;
                    valid_samples += 1;
                }
            }
        }

        Ok(if valid_samples > 0 {
            total_silhouette / valid_samples as f32
        } else {
            0.0
        })
    }

    /// Compute Euclidean distance between two nodes using actual GPU positions.
    /// Falls back to centroid Euclidean distance if positions are unavailable.
    fn calculate_node_distance(
        &self,
        node1: usize,
        node2: usize,
        centroids: &[(f32, f32, f32)],
    ) -> f32 {
        // Try to use actual spatial positions from GPU
        if let Some(ref ctx) = self.shared_context {
            if let Ok(uc) = ctx.unified_compute.lock() {
                let n = uc.num_nodes;
                if node1 < n && node2 < n {
                    let mut px = vec![0.0f32; n];
                    let mut py = vec![0.0f32; n];
                    let mut pz = vec![0.0f32; n];
                    if uc.download_positions(&mut px, &mut py, &mut pz).is_ok() {
                        let dx = px[node1] - px[node2];
                        let dy = py[node1] - py[node2];
                        let dz = pz[node1] - pz[node2];
                        return (dx * dx + dy * dy + dz * dz).sqrt();
                    }
                }
            }
        }

        // Fallback: Euclidean distance between cluster centroids
        if centroids.len() >= 2 {
            let c1 = node1 % centroids.len();
            let c2 = node2 % centroids.len();
            let (x1, y1, z1) = centroids[c1];
            let (x2, y2, z2) = centroids[c2];
            let dx = x1 - x2;
            let dy = y1 - y2;
            let dz = z1 - z2;
            (dx * dx + dy * dy + dz * dz).sqrt()
        } else {
            1.0
        }
    }

    // ADR-031 D1: the shadow `calculate_modularity` heuristic was removed. It
    // double-counted bridge edges (each shared edge appeared in both communities'
    // external_edges, inflating m), used a per-community internal+external sum in
    // place of σtot, and clamped to [0,1] — diverging from the canonical Newman Q.
    // Modularity now has a single implementation, `modularity_csr`
    // (unified_gpu_compute/community.rs), whose value the detection path returns
    // and which `actual_modularity` reuses for the stats field.

    /// Compute cluster coherence using actual Euclidean distances between
    /// node positions from the GPU layout. High coherence means nodes in
    /// this cluster are spatially close together.
    fn calculate_cluster_coherence(&self, nodes: &[u32], _assignments: &[i32]) -> f32 {
        if nodes.len() < 2 {
            return 1.0;
        }

        // Try to load actual positions from GPU for Euclidean distance
        let positions: Option<(Vec<f32>, Vec<f32>, Vec<f32>)> =
            self.shared_context.as_ref().and_then(|ctx| {
                ctx.unified_compute.lock().ok().and_then(|uc| {
                    let n = uc.num_nodes;
                    let mut px = vec![0.0f32; n];
                    let mut py = vec![0.0f32; n];
                    let mut pz = vec![0.0f32; n];
                    uc.download_positions(&mut px, &mut py, &mut pz)
                        .ok()
                        .map(|_| (px, py, pz))
                })
            });

        let mut total_distance = 0.0f32;
        let mut pair_count = 0u64;

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let dist = if let Some((ref px, ref py, ref pz)) = positions {
                    let ni = nodes[i] as usize;
                    let nj = nodes[j] as usize;
                    if ni < px.len() && nj < px.len() {
                        let dx = px[ni] - px[nj];
                        let dy = py[ni] - py[nj];
                        let dz = pz[ni] - pz[nj];
                        (dx * dx + dy * dy + dz * dz).sqrt()
                    } else {
                        1.0
                    }
                } else {
                    // No GPU positions available; use constant to avoid fake ordering
                    1.0
                };
                total_distance += dist;
                pair_count += 1;
            }
        }

        if pair_count > 0 {
            let avg_distance = total_distance / pair_count as f32;
            // Inverse relationship: smaller avg distance = higher coherence
            let coherence = 1.0 / (1.0 + avg_distance);
            // A NaN position yields a NaN distance; score it at the floor.
            if coherence.is_nan() {
                0.1
            } else {
                coherence.clamp(0.1, 1.0)
            }
        } else {
            1.0
        }
    }

    /// Compute cluster centroid from actual GPU node positions.
    /// Falls back to origin if positions are unavailable.
    fn calculate_cluster_centroid(&self, nodes: &[u32]) -> [f32; 3] {
        if nodes.is_empty() {
            return [0.0, 0.0, 0.0];
        }

        // Try actual positions from GPU
        if let Some(ref ctx) = self.shared_context {
            if let Ok(uc) = ctx.unified_compute.lock() {
                let n = uc.num_nodes;
                let mut px = vec![0.0f32; n];
                let mut py = vec![0.0f32; n];
                let mut pz = vec![0.0f32; n];
                if uc.download_positions(&mut px, &mut py, &mut pz).is_ok() {
                    let mut sx = 0.0f32;
                    let mut sy = 0.0f32;
                    let mut sz = 0.0f32;
                    let mut count = 0u32;
                    for &nid in nodes {
                        let idx = nid as usize;
                        if idx < n {
                            sx += px[idx];
                            sy += py[idx];
                            sz += pz[idx];
                            count += 1;
                        }
                    }
                    if count > 0 {
                        let c = count as f32;
                        return [sx / c, sy / c, sz / c];
                    }
                }
            }
        }

        // Fallback: return origin
        [0.0, 0.0, 0.0]
    }

    /// Count edges where both endpoints are in the node set (internal)
    /// by downloading the CSR graph from the GPU.
    /// Returns 0 if GPU edge data is unavailable.
    fn calculate_internal_edges(&self, nodes: &[u32]) -> usize {
        if nodes.len() < 2 {
            return 0;
        }

        let node_set: std::collections::HashSet<u32> = nodes.iter().copied().collect();

        if let Some(ref ctx) = self.shared_context {
            if let Ok(uc) = ctx.unified_compute.lock() {
                let n = uc.num_nodes;
                let num_edges = uc.num_edges;
                if n > 0 && num_edges > 0 {
                    // Use allocated sizes for copy_to (device buffers may be overallocated)
                    let mut row_offsets = vec![0i32; uc.edge_row_offsets.len()];
                    let mut col_indices = vec![0i32; uc.edge_col_indices.len()];
                    use cust::memory::CopyDestination;
                    if uc.edge_row_offsets.copy_to(&mut row_offsets).is_ok()
                        && uc.edge_col_indices.copy_to(&mut col_indices).is_ok()
                    {
                        row_offsets.truncate(n + 1);
                        col_indices.truncate(num_edges);
                        let mut internal_count = 0usize;
                        for &node_id in nodes {
                            let idx = node_id as usize;
                            if idx < n {
                                let start = row_offsets[idx] as usize;
                                let end = row_offsets[idx + 1] as usize;
                                for &neighbor in &col_indices[start..end.min(col_indices.len())] {
                                    if node_set.contains(&(neighbor as u32)) {
                                        internal_count += 1;
                                    }
                                }
                            }
                        }
                        // Each internal edge counted twice (once from each endpoint)
                        return internal_count / 2;
                    }
                }
            }
        }

        // GPU edge data unavailable
        0
    }

    /// Count edges where exactly one endpoint is in the node set (external)
    /// by downloading the CSR graph from the GPU.
    /// Returns 0 if GPU edge data is unavailable.
    fn calculate_external_edges(&self, nodes: &[u32]) -> usize {
        if nodes.is_empty() {
            return 0;
        }

        let node_set: std::collections::HashSet<u32> = nodes.iter().copied().collect();

        if let Some(ref ctx) = self.shared_context {
            if let Ok(uc) = ctx.unified_compute.lock() {
                let n = uc.num_nodes;
                let num_edges = uc.num_edges;
                if n > 0 && num_edges > 0 {
                    let mut row_offsets = vec![0i32; uc.edge_row_offsets.len()];
                    let mut col_indices = vec![0i32; uc.edge_col_indices.len()];
                    use cust::memory::CopyDestination;
                    if uc.edge_row_offsets.copy_to(&mut row_offsets).is_ok()
                        && uc.edge_col_indices.copy_to(&mut col_indices).is_ok()
                    {
                        row_offsets.truncate(n + 1);
                        col_indices.truncate(num_edges);
                        let mut external_count = 0usize;
                        for &node_id in nodes {
                            let idx = node_id as usize;
                            if idx < n {
                                let start = row_offsets[idx] as usize;
                                let end = row_offsets[idx + 1] as usize;
                                for &neighbor in &col_indices[start..end.min(col_indices.len())] {
                                    if !node_set.contains(&(neighbor as u32)) {
                                        external_count += 1;
                                    }
                                }
                            }
                        }
                        return external_count;
                    }
                }
            }
        }

        // GPU edge data unavailable
        0
    }

    fn calculate_community_density(&self, nodes: &[u32]) -> f32 {
        let n = nodes.len();
        if n < 2 {
            return 1.0;
        }

        let max_possible_edges = n * (n - 1) / 2;
        let actual_edges = self.calculate_internal_edges(nodes);

        (actual_edges as f32 / max_possible_edges as f32).min(1.0)
    }
}

impl Default for ClusteringActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for ClusteringActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Clustering Actor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Clustering Actor stopped");
    }
}

// === Message Handlers ===

impl Handler<SetSharedGPUContext> for ClusteringActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        info!("ClusteringActor: Received SharedGPUContext from ResourceActor");
        self.shared_context = Some(msg.context);
        // Invalidate cached node_id_map so it gets rebuilt from the new context
        self.node_id_map.clear();
        info!("ClusteringActor: SharedGPUContext stored successfully");
        Ok(())
    }
}

impl Handler<RunKMeans> for ClusteringActor {
    type Result = actix::ResponseFuture<Result<KMeansResult, String>>;

    fn handle(&mut self, msg: RunKMeans, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ClusteringActor: Received RunKMeans request with {} clusters",
            msg.params.num_clusters
        );

        let mut actor_clone = Self {
            gpu_state: self.gpu_state.clone(),
            shared_context: self.shared_context.clone(),
            node_id_map: self.node_id_map.clone(),
            node_analytics: self.node_analytics.clone(),
        };

        Box::pin(async move { actor_clone.perform_kmeans_clustering(msg.params).await })
    }
}

impl Handler<RunCommunityDetection> for ClusteringActor {
    type Result = actix::ResponseFuture<Result<CommunityDetectionResult, String>>;

    fn handle(&mut self, msg: RunCommunityDetection, _ctx: &mut Self::Context) -> Self::Result {
        info!("ClusteringActor: Received RunCommunityDetection request");

        let mut actor_clone = Self {
            gpu_state: self.gpu_state.clone(),
            shared_context: self.shared_context.clone(),
            node_id_map: self.node_id_map.clone(),
            node_analytics: self.node_analytics.clone(),
        };

        Box::pin(async move { actor_clone.perform_community_detection(msg.params).await })
    }
}

impl Handler<RunDBSCAN> for ClusteringActor {
    type Result = actix::ResponseFuture<Result<DBSCANResult, String>>;

    fn handle(&mut self, msg: RunDBSCAN, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ClusteringActor: Received RunDBSCAN request (epsilon={}, min_points={})",
            msg.params.epsilon, msg.params.min_points
        );

        let mut actor_clone = Self {
            gpu_state: self.gpu_state.clone(),
            shared_context: self.shared_context.clone(),
            node_id_map: self.node_id_map.clone(),
            node_analytics: self.node_analytics.clone(),
        };

        Box::pin(async move { actor_clone.perform_dbscan_clustering(msg.params).await })
    }
}

impl Handler<SetNodeAnalytics> for ClusteringActor {
    type Result = ();

    fn handle(&mut self, msg: SetNodeAnalytics, _ctx: &mut Self::Context) {
        info!("ClusteringActor: Received shared node_analytics map");
        self.node_analytics = Some(msg.node_analytics);
    }
}

impl Handler<WriteClusterAnalytics> for ClusteringActor {
    type Result = ();

    /// Persist a finished clustering run's per-node `cluster_id` through the single
    /// writer (ADR-031 D3). The spawn task in `clustering_handlers::run_clustering`
    /// routes its final `Vec<Cluster>` here so node_analytics is populated whether
    /// the GPU kernels or the CPU label-propagation fallback produced the result.
    ///
    /// `Cluster.nodes` already carry graph node ids (the kernels translate GPU
    /// buffer indices before building clusters, and the CPU fallback emits compact
    /// node ids), so no GPU-index translation is needed — only the shared
    /// masked-key / 1-based / stale-reset write applied by
    /// `write_cluster_id_from_assignments`.
    fn handle(&mut self, msg: WriteClusterAnalytics, _ctx: &mut Self::Context) {
        let Some(ref analytics_map) = self.node_analytics else {
            warn!(
                "ClusteringActor: WriteClusterAnalytics received but node_analytics \
                 store is not wired; skipping (hulls will not render this run)"
            );
            return;
        };

        // Build (graph_node_id, cluster_id_1based) pairs. ADR-031 I-6: cluster_id
        // is 1-based with 0 == unclustered. Each Cluster's positional index is its
        // 0-based id; +1 yields the 1-based wire id. Empty clusters contribute
        // nothing (their absence of nodes leaves those nodes at the reset 0).
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for (cluster_idx, cluster) in msg.clusters.iter().enumerate() {
            let cluster_id_1based = (cluster_idx as u32) + 1;
            for &node_id in &cluster.nodes {
                pairs.push((node_id, cluster_id_1based));
            }
        }

        if let Ok(mut map) = analytics_map.write() {
            let assigned = self.write_cluster_id_from_assignments(&mut map, &pairs);
            info!(
                "ClusteringActor: Wrote node_analytics.cluster_id for {} nodes across {} clusters \
                 (WriteClusterAnalytics single-writer path)",
                assigned,
                msg.clusters.len()
            );
        } else {
            warn!("ClusteringActor: node_analytics RwLock poisoned; cluster_id write skipped");
        }
    }
}

impl Handler<UpdateGPUGraphData> for ClusteringActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGPUGraphData, _ctx: &mut Self::Context) -> Self::Result {
        let num_nodes = msg.graph.nodes.len() as u32;
        let num_edges = msg.graph.edges.len() as u32;
        info!(
            "ClusteringActor: UpdateGPUGraphData received — {} nodes, {} edges",
            num_nodes, num_edges
        );
        self.gpu_state.num_nodes = num_nodes;
        self.gpu_state.num_edges = num_edges;
        // Force re-download of the node_id_map on next clustering operation
        self.node_id_map.clear();
        Ok(())
    }
}

impl Handler<PerformGPUClustering> for ClusteringActor {
    type Result = actix::ResponseFuture<
        Result<Vec<crate::handlers::api_handler::analytics::Cluster>, String>,
    >;

    fn handle(&mut self, msg: PerformGPUClustering, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "ClusteringActor: Received PerformGPUClustering request with method: {}",
            msg.method
        );

        let mut actor_clone = Self {
            gpu_state: self.gpu_state.clone(),
            shared_context: self.shared_context.clone(),
            node_id_map: self.node_id_map.clone(),
            node_analytics: self.node_analytics.clone(),
        };

        let method = msg.method.clone();
        Box::pin(async move {
            match method.to_lowercase().as_str() {
                "dbscan" => {
                    let dbscan_params = DBSCANParams {
                        epsilon: msg.params.eps.unwrap_or(0.5),
                        min_points: msg.params.min_samples.unwrap_or(5),
                    };
                    let result = actor_clone.perform_dbscan_clustering(dbscan_params).await?;
                    Ok(result.clusters)
                }
                "louvain" | "leiden" | "communities" => {
                    // "communities" (and "leiden") select the refined Leiden detector —
                    // the same partition that drives the Community Cohesion force, so
                    // analytics coloring matches cohesion grouping. "louvain" keeps the
                    // un-refined variant for comparison.
                    let algorithm = if method.eq_ignore_ascii_case("louvain") {
                        CommunityDetectionAlgorithm::Louvain
                    } else {
                        CommunityDetectionAlgorithm::Leiden
                    };
                    let community_params = CommunityDetectionParams {
                        algorithm,
                        max_iterations: msg.params.max_iterations,
                        convergence_tolerance: msg.params.convergence_threshold,
                        synchronous: None,
                        seed: msg.params.seed.map(|s| s as u32),
                        resolution: msg.params.resolution,
                    };
                    let result = actor_clone
                        .perform_community_detection(community_params)
                        .await?;
                    // Convert communities to Cluster format
                    Ok(result
                        .communities
                        .into_iter()
                        .map(|c| crate::handlers::api_handler::analytics::Cluster {
                            id: c.id.clone(),
                            label: format!("Community {}", c.id),
                            node_count: c.nodes.len() as u32,
                            coherence: c.density,
                            color: format!(
                                "#{:02X}{:02X}{:02X}",
                                (c.id.parse::<usize>().unwrap_or(0) * 50 % 255) as u8,
                                (c.id.parse::<usize>().unwrap_or(0) * 100 % 255) as u8,
                                (c.id.parse::<usize>().unwrap_or(0) * 150 % 255) as u8
                            ),
                            keywords: vec![format!("community-{}", c.id)],
                            centroid: None,
                            nodes: c.nodes,
                        })
                        .collect())
                }
                _ => {
                    // Default: K-means
                    let params = KMeansParams {
                        num_clusters: msg.params.num_clusters.unwrap_or(8) as usize,
                        max_iterations: msg.params.max_iterations,
                        tolerance: msg.params.convergence_threshold,
                        seed: None,
                    };
                    let result = actor_clone.perform_kmeans_clustering(params).await?;
                    Ok(result.clusters)
                }
            }
        })
    }
}

/// ADR-031 D1: a partition whose Newman modularity is below this threshold has no
/// meaningful community structure (it is statistical noise), so it is rejected and
/// every node is left at `community_id = 0`. Strict `<`, so exactly 0.30 accepts.
pub(crate) const MODULARITY_GATE: f32 = 0.3;

/// Minimum GPU node count below which any clustering run is refused outright.
/// Below this the centroid-init / neighbour-scan kernels launch with a zero or
/// negative grid and read past valid buffer extents — an illegal access that
/// wedges the shared CUDA stream. Because clustering holds the shared
/// `unified_compute` mutex across the kernel, a wedged stream never releases the
/// lock and the physics step (which shares that context) deadlocks system-wide.
/// Refusing degenerate input BEFORE launch lets the lock drop cleanly instead.
pub(crate) const MIN_NODES_FOR_CLUSTERING: usize = 3;

/// Largest per-axis position spread (max − min, world units) still treated as
/// "no spread". When all three axes are below this, the nodes are effectively
/// coincident (the pre-warmup / un-laid-out state where every node sits at the
/// origin) and distance-based clustering is meaningless — k-means centroid init
/// can spin selecting coincident seeds. Refused as a degenerate run.
pub(crate) const MIN_POSITION_SPREAD: f32 = 1e-3;

/// Pure precondition check for position-based GPU clustering (k-means / DBSCAN).
/// Returns `Some(reason)` when the run must be refused BEFORE any kernel launch
/// (so the shared `unified_compute` lock releases cleanly rather than being held
/// across a kernel that never returns), or `None` when the input is safe to run.
/// Pure over its scalar inputs so it is unit-testable without a GPU.
pub(crate) fn position_clustering_refusal(
    num_nodes: usize,
    spread_x: f32,
    spread_y: f32,
    spread_z: f32,
) -> Option<String> {
    if num_nodes < MIN_NODES_FOR_CLUSTERING {
        return Some(format!(
            "only {} GPU node(s); need >= {}",
            num_nodes, MIN_NODES_FOR_CLUSTERING
        ));
    }
    if spread_x < MIN_POSITION_SPREAD
        && spread_y < MIN_POSITION_SPREAD
        && spread_z < MIN_POSITION_SPREAD
    {
        return Some(format!(
            "node positions are coincident (spread {:.2e}/{:.2e}/{:.2e} < {:.0e}); \
             layout has not warmed up",
            spread_x, spread_y, spread_z, MIN_POSITION_SPREAD
        ));
    }
    None
}

/// Download the GPU position buffers and return per-axis spread (max − min),
/// ignoring non-finite entries. A cheap, occasional device→host copy used by the
/// precondition guard before launching a position-based kernel. On any copy
/// failure an axis contributes 0.0 spread, which conservatively triggers the
/// coincident-positions refusal rather than risking a degenerate launch.
fn gpu_position_spread(
    uc: &crate::utils::unified_gpu_compute::UnifiedGPUCompute,
) -> (f32, f32, f32) {
    use cust::memory::CopyDestination;
    let n = uc.num_nodes;
    if n == 0 {
        return (0.0, 0.0, 0.0);
    }
    // The device→host `copy_to` below requires the CUDA context to be CURRENT on
    // this thread. The guard runs inside `spawn_blocking`, where a fresh tokio
    // worker thread does NOT inherit the primary context — so without this the
    // copies fail and every axis reads 0.0, producing a FALSE coincident-positions
    // refusal even when the GPU holds a valid spread layout. Retaining + making the
    // primary context current (mirrors `get_node_positions`, execution.rs:836) is
    // what makes the read observe real positions. Hold it for the copies' lifetime.
    let _thread_context = match cust::context::Context::new(uc.device) {
        Ok(ctx) => ctx,
        Err(e) => {
            log::warn!(
                "gpu_position_spread: failed to set CUDA context ({e}); treating as coincident"
            );
            return (0.0, 0.0, 0.0);
        }
    };
    let axis_spread = |host: &[f32]| -> f32 {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &v in host {
            if v.is_finite() {
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
        }
        if lo.is_finite() && hi.is_finite() {
            hi - lo
        } else {
            0.0
        }
    };
    let read = |buf: &cust::memory::DeviceBuffer<f32>| -> f32 {
        let blen = buf.len();
        if blen == 0 {
            return 0.0;
        }
        let mut host = vec![0f32; blen];
        if buf.copy_to(&mut host).is_err() {
            return 0.0;
        }
        host.truncate(n);
        axis_spread(&host)
    };
    (read(&uc.pos_in_x), read(&uc.pos_in_y), read(&uc.pos_in_z))
}

/// Pure, behaviour-preserving extraction of the modularity gate's effect on the
/// analytics map (ADR-031 D1 + D3 single-writer reset). Always clears every
/// entry's `community_id` first; then, only if `modularity >= MODULARITY_GATE`,
/// writes `labels[i]` into the entry keyed by the pre-translated, masked
/// `node_ids[i]`. Returns `true` when the partition was accepted and populated,
/// `false` when it was rejected (all `community_id` left at 0).
///
/// Callers pass `node_ids[i]` already resolved via `translate_gpu_index` and
/// masked with `NODE_ID_MASK`; `node_ids` and `labels` align by index.
pub(crate) fn apply_modularity_gate(
    map: &mut std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
    node_ids: &[u32],
    labels: &[i32],
    modularity: f32,
) -> bool {
    // Single-writer reset: clear stale community_id from any prior run (and on
    // gate-reject) so the field reflects only the current pass.
    for entry in map.values_mut() {
        entry.community_id = 0;
    }
    if modularity < MODULARITY_GATE {
        return false;
    }
    for (&node_id, &label) in node_ids.iter().zip(labels.iter()) {
        let entry = map.entry(node_id).or_default();
        entry.community_id = if label < 0 { 0 } else { label as u32 };
    }
    true
}

#[cfg(test)]
mod gate_tests {
    use super::{apply_modularity_gate, MODULARITY_GATE};
    use crate::utils::binary_protocol::NodeAnalytics;
    use std::collections::HashMap;

    fn map_with(ids: &[u32]) -> HashMap<u32, NodeAnalytics> {
        let mut m = HashMap::new();
        for &id in ids {
            // Seed a stale non-zero community_id to prove the reset path runs.
            let e = NodeAnalytics {
                community_id: 999,
                ..Default::default()
            };
            m.insert(id, e);
        }
        m
    }

    #[test]
    fn accepts_at_or_above_gate_and_populates() {
        let mut m = map_with(&[10, 20, 30]);
        let accepted = apply_modularity_gate(&mut m, &[10, 20, 30], &[1, 1, 2], MODULARITY_GATE);
        assert!(accepted);
        assert_eq!(m[&10].community_id, 1);
        assert_eq!(m[&20].community_id, 1);
        assert_eq!(m[&30].community_id, 2);
    }

    #[test]
    fn rejects_below_gate_and_resets_all_to_zero() {
        let mut m = map_with(&[10, 20, 30]);
        let accepted = apply_modularity_gate(&mut m, &[10, 20, 30], &[1, 1, 2], 0.2999);
        assert!(!accepted);
        assert!(m.values().all(|e| e.community_id == 0));
    }

    #[test]
    fn boundary_exactly_at_gate_accepts() {
        // Strict `<` means Q == 0.30 is accepted, not rejected.
        let mut m = map_with(&[7]);
        assert!(apply_modularity_gate(&mut m, &[7], &[3], 0.30));
        assert_eq!(m[&7].community_id, 3);
    }

    #[test]
    fn negative_label_maps_to_zero() {
        let mut m = map_with(&[1, 2]);
        assert!(apply_modularity_gate(&mut m, &[1, 2], &[-1, 4], 0.5));
        assert_eq!(m[&1].community_id, 0);
        assert_eq!(m[&2].community_id, 4);
    }

    #[test]
    fn populates_entries_absent_from_prior_map() {
        // node_ids not previously present must be inserted (or_default).
        let mut m: HashMap<u32, NodeAnalytics> = HashMap::new();
        assert!(apply_modularity_gate(&mut m, &[100, 200], &[5, 6], 0.4));
        assert_eq!(m[&100].community_id, 5);
        assert_eq!(m[&200].community_id, 6);
    }
}

#[cfg(test)]
mod precondition_tests {
    use super::{position_clustering_refusal, MIN_NODES_FOR_CLUSTERING, MIN_POSITION_SPREAD};

    #[test]
    fn refuses_zero_nodes() {
        let r = position_clustering_refusal(0, 100.0, 100.0, 100.0);
        assert!(r.is_some());
        assert!(r.unwrap().contains("0 GPU node"));
    }

    #[test]
    fn refuses_below_min_node_count() {
        // One below the floor is refused; exactly at the floor (with spread) runs.
        assert!(
            position_clustering_refusal(MIN_NODES_FOR_CLUSTERING - 1, 50.0, 50.0, 50.0).is_some()
        );
        assert!(position_clustering_refusal(MIN_NODES_FOR_CLUSTERING, 50.0, 50.0, 50.0).is_none());
    }

    #[test]
    fn refuses_coincident_positions() {
        // All nodes at the origin (pre-warmup): every axis spread is ~0 → refuse.
        let r = position_clustering_refusal(1000, 0.0, 0.0, 0.0);
        assert!(r.is_some());
        assert!(r.unwrap().contains("coincident"));
    }

    #[test]
    fn accepts_when_any_axis_has_spread() {
        // A single axis above the floor is enough structure to cluster on.
        assert!(position_clustering_refusal(1000, MIN_POSITION_SPREAD * 10.0, 0.0, 0.0).is_none());
        assert!(position_clustering_refusal(1000, 0.0, MIN_POSITION_SPREAD * 10.0, 0.0).is_none());
        assert!(position_clustering_refusal(1000, 0.0, 0.0, MIN_POSITION_SPREAD * 10.0).is_none());
    }

    #[test]
    fn node_count_refusal_takes_precedence_over_spread() {
        // Too few nodes is reported first even if positions are well spread.
        let r = position_clustering_refusal(1, 100.0, 100.0, 100.0);
        assert!(r.unwrap().contains("GPU node"));
    }
}
