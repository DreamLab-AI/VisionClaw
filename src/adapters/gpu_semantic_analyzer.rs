// src/adapters/gpu_semantic_analyzer.rs
//! GPU Semantic Analyzer Adapter
//!
//! Implements SemanticAnalyzer port using GPU compute for graph algorithms
//! integrating CUDA kernels for pathfinding (SSSP, landmark APSP)

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, instrument, warn};

use crate::utils::unified_gpu_compute::UnifiedGPUCompute;
use visionclaw_domain::models::constraints::ConstraintSet;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::ports::gpu_semantic_analyzer::{
    ClusteringAlgorithm, CommunityDetectionResult, GpuSemanticAnalyzer, GpuSemanticAnalyzerError,
    ImportanceAlgorithm, OptimizationResult, PathfindingResult, Result, SemanticConstraintConfig,
    SemanticStatistics,
};

pub struct GpuSemanticAnalyzerAdapter {
    gpu_compute: Option<UnifiedGPUCompute>,

    graph_data: Option<Arc<GraphData>>,

    sssp_cache: HashMap<u32, Vec<f32>>,

    apsp_cache: Option<Vec<Vec<f32>>>,

    total_sssp_computations: u64,
    total_apsp_computations: u64,
    cache_hits: u64,
    cache_misses: u64,
}

impl GpuSemanticAnalyzerAdapter {
    pub fn new() -> Self {
        Self {
            gpu_compute: None,
            graph_data: None,
            sssp_cache: HashMap::new(),
            apsp_cache: None,
            total_sssp_computations: 0,
            total_apsp_computations: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    fn initialize_gpu(&mut self, num_nodes: usize, num_edges: usize) -> Result<()> {
        let ptx_paths = [
            include_str!("../utils/ptx/sssp_compact.ptx"),
            include_str!("../utils/ptx/gpu_landmark_apsp.ptx"),
            include_str!("../utils/ptx/gpu_clustering_kernels.ptx"),
        ];

        let ptx_combined = ptx_paths.join("\n");

        let gpu_compute =
            UnifiedGPUCompute::new(num_nodes, num_edges, &ptx_combined).map_err(|e| {
                GpuSemanticAnalyzerError::CudaError(format!("Failed to initialize GPU: {}", e))
            })?;

        self.gpu_compute = Some(gpu_compute);
        info!(
            "Initialized GPU semantic analyzer with {} nodes, {} edges",
            num_nodes, num_edges
        );
        Ok(())
    }

    fn gpu(&mut self) -> Result<&mut UnifiedGPUCompute> {
        self.gpu_compute
            .as_mut()
            .ok_or(GpuSemanticAnalyzerError::GpuNotAvailable)
    }

    fn reconstruct_path(
        &self,
        distances: &[f32],
        source: u32,
        target: u32,
        graph: &GraphData,
    ) -> Vec<u32> {
        if distances[target as usize].is_infinite() {
            return Vec::new();
        }

        let mut path = vec![target];
        let mut current = target;

        while current != source {
            let current_dist = distances[current as usize];

            let mut found_predecessor = false;

            for edge in &graph.edges {
                if edge.target == current {
                    let neighbor = edge.source;
                    let neighbor_dist = distances[neighbor as usize];

                    if (neighbor_dist + edge.weight - current_dist).abs() < 0.0001 {
                        path.push(neighbor);
                        current = neighbor;
                        found_predecessor = true;
                        break;
                    }
                }
            }

            if !found_predecessor {
                warn!("Path reconstruction failed at node {}", current);
                break;
            }

            if path.len() > distances.len() {
                warn!("Path reconstruction loop detected");
                break;
            }
        }

        path.reverse();
        path
    }

    fn build_paths_from_distances(
        &self,
        distances: &[f32],
        source: u32,
        graph: &GraphData,
    ) -> HashMap<u32, Vec<u32>> {
        let mut paths = HashMap::new();

        for node_id in 0..distances.len() {
            if node_id != source as usize && !distances[node_id].is_infinite() {
                let path = self.reconstruct_path(distances, source, node_id as u32, graph);
                if !path.is_empty() {
                    paths.insert(node_id as u32, path);
                }
            }
        }

        paths
    }

    async fn compute_landmark_apsp_internal(
        &mut self,
        num_landmarks: usize,
    ) -> Result<Vec<Vec<f32>>> {
        let graph = self
            .graph_data
            .as_ref()
            .ok_or(GpuSemanticAnalyzerError::InvalidGraph(
                "No graph loaded".to_string(),
            ))?;

        let num_nodes = graph.nodes.len();

        let mut landmarks = Vec::new();
        let stride = num_nodes / num_landmarks;
        for i in 0..num_landmarks {
            let landmark_idx = (i * stride).min(num_nodes - 1);
            landmarks.push(landmark_idx as u32);
        }

        info!(
            "Computing landmark APSP with {} landmarks from {} nodes",
            num_landmarks, num_nodes
        );

        let mut landmark_distances = Vec::new();
        for &landmark in &landmarks {
            let distances = self.compute_sssp_distances(landmark).await?;
            landmark_distances.push(distances);
        }

        let mut distance_matrix = vec![vec![f32::INFINITY; num_nodes]; num_nodes];

        for i in 0..num_nodes {
            distance_matrix[i][i] = 0.0;

            for j in (i + 1)..num_nodes {
                let mut min_dist = f32::INFINITY;

                for k in 0..num_landmarks {
                    let dist_ik = landmark_distances[k][i];
                    let dist_kj = landmark_distances[k][j];

                    if !dist_ik.is_infinite() && !dist_kj.is_infinite() {
                        min_dist = min_dist.min(dist_ik + dist_kj);
                    }
                }

                distance_matrix[i][j] = min_dist;
                distance_matrix[j][i] = min_dist;
            }
        }

        info!("Landmark APSP computation complete");
        Ok(distance_matrix)
    }
}

impl Default for GpuSemanticAnalyzerAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GpuSemanticAnalyzer for GpuSemanticAnalyzerAdapter {
    #[instrument(skip(self, graph))]
    async fn initialize(&mut self, graph: Arc<GraphData>) -> Result<()> {
        let num_nodes = graph.nodes.len();
        let num_edges = graph.edges.len();

        if num_nodes == 0 {
            return Err(GpuSemanticAnalyzerError::InvalidGraph(
                "Graph has no nodes".to_string(),
            ));
        }

        self.initialize_gpu(num_nodes, num_edges)?;

        let gpu = self.gpu()?;

        let mut edge_row_offsets = vec![0i32; num_nodes + 1];
        let mut edge_col_indices = Vec::new();
        let mut edge_weights = Vec::new();

        let mut edge_counts = vec![0usize; num_nodes];
        for edge in &graph.edges {
            if (edge.source as usize) < num_nodes {
                edge_counts[edge.source as usize] += 1;
            }
        }

        let mut offset = 0;
        for i in 0..num_nodes {
            edge_row_offsets[i] = offset;
            offset += edge_counts[i] as i32;
        }
        edge_row_offsets[num_nodes] = offset;

        let mut edge_list: Vec<_> = graph.edges.to_vec();
        edge_list.sort_by_key(|e| e.source);

        for edge in edge_list {
            edge_col_indices.push(edge.target as i32);
            edge_weights.push(edge.weight);
        }

        gpu.upload_edges_csr(&edge_row_offsets, &edge_col_indices, &edge_weights)
            .map_err(|e| {
                GpuSemanticAnalyzerError::CudaError(format!("Failed to upload graph: {}", e))
            })?;

        self.graph_data = Some(graph);
        info!("GPU semantic analyzer initialized with graph structure");
        Ok(())
    }

    #[instrument(skip(self))]
    async fn detect_communities(
        &mut self,
        _algorithm: ClusteringAlgorithm,
    ) -> Result<CommunityDetectionResult> {
        let start = Instant::now();

        let graph = self
            .graph_data
            .as_ref()
            .ok_or(GpuSemanticAnalyzerError::InvalidGraph(
                "No graph loaded".to_string(),
            ))?;

        let _num_nodes = graph.nodes.len();
        let clusters = HashMap::new();
        let cluster_sizes = HashMap::new();

        Ok(CommunityDetectionResult {
            clusters,
            cluster_sizes,
            modularity: 0.0,
            computation_time_ms: start.elapsed().as_secs_f32() * 1000.0,
        })
    }

    #[instrument(skip(self))]
    async fn compute_shortest_paths(&mut self, source_node_id: u32) -> Result<PathfindingResult> {
        let start = Instant::now();

        let distances_vec = self.compute_sssp_distances(source_node_id).await?;

        let graph = self
            .graph_data
            .as_ref()
            .ok_or(GpuSemanticAnalyzerError::InvalidGraph(
                "No graph loaded".to_string(),
            ))?;

        let paths = self.build_paths_from_distances(&distances_vec, source_node_id, graph);

        let mut distances = HashMap::new();
        for (i, &dist) in distances_vec.iter().enumerate() {
            if !dist.is_infinite() {
                distances.insert(i as u32, dist);
            }
        }

        let computation_time_ms = start.elapsed().as_secs_f32() * 1000.0;

        info!(
            "SSSP from node {} computed in {:.2}ms, {} reachable nodes",
            source_node_id,
            computation_time_ms,
            distances.len()
        );

        Ok(PathfindingResult {
            source_node: source_node_id,
            distances,
            paths,
            computation_time_ms,
        })
    }

    #[instrument(skip(self))]
    async fn compute_sssp_distances(&mut self, source_node_id: u32) -> Result<Vec<f32>> {
        if let Some(cached) = self.sssp_cache.get(&source_node_id) {
            self.cache_hits += 1;
            debug!("SSSP cache hit for source {}", source_node_id);
            return Ok(cached.clone());
        }

        self.cache_misses += 1;
        let start = Instant::now();

        let graph = self
            .graph_data
            .as_ref()
            .ok_or(GpuSemanticAnalyzerError::InvalidGraph(
                "No graph loaded".to_string(),
            ))?;

        if source_node_id as usize >= graph.nodes.len() {
            return Err(GpuSemanticAnalyzerError::InvalidGraph(format!(
                "Source node {} out of range",
                source_node_id
            )));
        }

        let gpu = self.gpu()?;
        let distances = gpu
            .run_sssp(source_node_id as usize, None)
            .map_err(|e| GpuSemanticAnalyzerError::CudaError(format!("SSSP failed: {}", e)))?;

        let computation_time_ms = start.elapsed().as_secs_f32() * 1000.0;
        self.total_sssp_computations += 1;

        info!(
            "GPU SSSP from node {} completed in {:.2}ms",
            source_node_id, computation_time_ms
        );

        self.sssp_cache.insert(source_node_id, distances.clone());

        Ok(distances)
    }

    #[instrument(skip(self))]
    async fn compute_all_pairs_shortest_paths(&mut self) -> Result<HashMap<(u32, u32), Vec<u32>>> {
        let graph = self
            .graph_data
            .as_ref()
            .ok_or(GpuSemanticAnalyzerError::InvalidGraph(
                "No graph loaded".to_string(),
            ))?;

        let num_nodes = graph.nodes.len();

        let num_landmarks = (num_nodes as f32).sqrt().ceil() as usize;
        let distance_matrix = self.compute_landmark_apsp(num_landmarks).await?;

        let mut all_paths = HashMap::new();

        for i in 0..num_nodes {
            for j in 0..num_nodes {
                if i != j && !distance_matrix[i][j].is_infinite() {
                    let path = vec![i as u32, j as u32];
                    all_paths.insert((i as u32, j as u32), path);
                }
            }
        }

        Ok(all_paths)
    }

    #[instrument(skip(self))]
    async fn compute_landmark_apsp(&mut self, num_landmarks: usize) -> Result<Vec<Vec<f32>>> {
        let start = Instant::now();

        if let Some(ref cached) = self.apsp_cache {
            self.cache_hits += 1;
            debug!("APSP cache hit");
            return Ok(cached.clone());
        }

        self.cache_misses += 1;

        let distance_matrix = self.compute_landmark_apsp_internal(num_landmarks).await?;

        let computation_time_ms = start.elapsed().as_secs_f32() * 1000.0;
        self.total_apsp_computations += 1;

        info!(
            "Landmark APSP with {} landmarks completed in {:.2}ms",
            num_landmarks, computation_time_ms
        );

        self.apsp_cache = Some(distance_matrix.clone());

        Ok(distance_matrix)
    }

    async fn generate_semantic_constraints(
        &mut self,
        _config: SemanticConstraintConfig,
    ) -> Result<ConstraintSet> {
        Ok(ConstraintSet::default())
    }

    async fn optimize_layout(
        &mut self,
        _constraints: &ConstraintSet,
        _max_iterations: usize,
    ) -> Result<OptimizationResult> {
        Ok(OptimizationResult {
            converged: true,
            iterations: 0,
            final_stress: 0.0,
            convergence_delta: 0.0,
            computation_time_ms: 0.0,
        })
    }

    async fn analyze_node_importance(
        &mut self,
        _algorithm: ImportanceAlgorithm,
    ) -> Result<HashMap<u32, f32>> {
        Ok(HashMap::new())
    }

    async fn update_graph_data(&mut self, graph: Arc<GraphData>) -> Result<()> {
        self.invalidate_pathfinding_cache().await?;
        self.initialize(graph).await
    }

    async fn get_statistics(&self) -> Result<SemanticStatistics> {
        let cache_total = self.cache_hits + self.cache_misses;
        let cache_hit_rate = if cache_total > 0 {
            self.cache_hits as f32 / cache_total as f32
        } else {
            0.0
        };

        let gpu_memory_mb = if let Some(ref _gpu) = self.gpu_compute {
            let graph = self.graph_data.as_ref().map(|g| g.nodes.len()).unwrap_or(0);
            (graph * 4 * 10) as f32 / 1_048_576.0
        } else {
            0.0
        };

        Ok(SemanticStatistics {
            total_analyses: self.total_sssp_computations + self.total_apsp_computations,
            average_clustering_time_ms: 0.0,
            average_pathfinding_time_ms: 0.0,
            cache_hit_rate,
            gpu_memory_used_mb: gpu_memory_mb,
        })
    }

    #[instrument(skip(self))]
    async fn invalidate_pathfinding_cache(&mut self) -> Result<()> {
        self.sssp_cache.clear();
        self.apsp_cache = None;
        debug!("Pathfinding cache invalidated");
        Ok(())
    }
}
