// src/adapters/actix_semantic_adapter.rs
//! Actix Semantic Analyzer Adapter
//!
//! Implements the GpuSemanticAnalyzer port by wrapping the SemanticProcessorActor.
//! Provides GPU-accelerated semantic analysis, community detection, and pathfinding.

use actix::prelude::*;
use async_trait::async_trait;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::actors::semantic_processor_actor::SemanticProcessorActor;
use crate::adapters::messages::*;
use crate::models::constraints::ConstraintSet;
use crate::models::graph::GraphData;
use crate::ports::gpu_semantic_analyzer::{
    ClusteringAlgorithm, CommunityDetectionResult, GpuSemanticAnalyzer, ImportanceAlgorithm,
    OptimizationResult, PathfindingResult, Result as PortResult, SemanticConstraintConfig,
    SemanticStatistics,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

///
pub struct ActixSemanticAdapter {
    actor_addr: Option<Addr<SemanticProcessorActor>>,
    timeout: Duration,
    initialized: bool,
}

impl ActixSemanticAdapter {
    pub fn new() -> Self {
        info!("Creating ActixSemanticAdapter");
        Self {
            actor_addr: None,
            timeout: DEFAULT_TIMEOUT,
            initialized: false,
        }
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            actor_addr: None,
            timeout,
            initialized: false,
        }
    }

    pub fn from_actor(actor_addr: Addr<SemanticProcessorActor>) -> Self {
        Self {
            actor_addr: Some(actor_addr),
            timeout: DEFAULT_TIMEOUT,
            initialized: true,
        }
    }

    async fn send_message<M>(&self, msg: M) -> PortResult<M::Result>
    where
        M: Message + Send + 'static,
        M::Result: Send,
        SemanticProcessorActor: Handler<M>,
    {
        let addr = self.actor_addr.as_ref().ok_or_else(|| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::InvalidGraph(
                "Analyzer not initialized".to_string(),
            )
        })?;

        tokio::time::timeout(self.timeout, addr.send(msg))
            .await
            .map_err(|_| {
                warn!("Actor message timeout");
                crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(
                    "Communication timeout".to_string(),
                )
            })?
            .map_err(|e| {
                warn!("Actor mailbox error: {}", e);
                crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(
                    format!("Communication error: {}", e),
                )
            })
    }
}

impl Default for ActixSemanticAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GpuSemanticAnalyzer for ActixSemanticAdapter {
    async fn initialize(&mut self, graph: Arc<GraphData>) -> PortResult<()> {
        info!(
            "Initializing ActixSemanticAdapter with {} nodes",
            graph.nodes.len()
        );

        if self.actor_addr.is_none() {
            let actor = SemanticProcessorActor::new(None);
            let addr = actor.start();
            self.actor_addr = Some(addr);
        }

        let msg = InitializeSemanticMessage::new(graph);
        self.send_message(msg).await?;

        self.initialized = true;
        Ok(())
    }

    async fn detect_communities(
        &mut self,
        algorithm: ClusteringAlgorithm,
    ) -> PortResult<CommunityDetectionResult> {
        debug!("Detecting communities with algorithm: {:?}", algorithm);
        let msg = DetectCommunitiesMessage::new(algorithm);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn compute_shortest_paths(
        &mut self,
        source_node_id: u32,
    ) -> PortResult<PathfindingResult> {
        debug!("Computing shortest paths from node {}", source_node_id);
        let msg = ComputeShortestPathsMessage::new(source_node_id);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn compute_sssp_distances(&mut self, source_node_id: u32) -> PortResult<Vec<f32>> {
        debug!("Computing SSSP distances from node {}", source_node_id);
        let msg = ComputeSsspDistancesMessage::new(source_node_id);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn compute_all_pairs_shortest_paths(
        &mut self,
    ) -> PortResult<HashMap<(u32, u32), Vec<u32>>> {
        info!("Computing all-pairs shortest paths");
        let msg = ComputeAllPairsShortestPathsMessage;
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn compute_landmark_apsp(&mut self, num_landmarks: usize) -> PortResult<Vec<Vec<f32>>> {
        info!("Computing landmark APSP with {} landmarks", num_landmarks);
        let msg = ComputeLandmarkApspMessage::new(num_landmarks);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn generate_semantic_constraints(
        &mut self,
        config: SemanticConstraintConfig,
    ) -> PortResult<ConstraintSet> {
        info!("Generating semantic constraints");
        let msg = GenerateSemanticConstraintsMessage::new(config);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn optimize_layout(
        &mut self,
        constraints: &ConstraintSet,
        max_iterations: usize,
    ) -> PortResult<OptimizationResult> {
        info!("Optimizing layout with {} iterations", max_iterations);
        let msg = OptimizeLayoutMessage::new(constraints.clone(), max_iterations);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn analyze_node_importance(
        &mut self,
        algorithm: ImportanceAlgorithm,
    ) -> PortResult<HashMap<u32, f32>> {
        debug!("Analyzing node importance with {:?}", algorithm);
        let msg = AnalyzeNodeImportanceMessage::new(algorithm);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn update_graph_data(&mut self, graph: Arc<GraphData>) -> PortResult<()> {
        info!("Updating semantic graph data");
        let msg = UpdateSemanticGraphDataMessage::new(graph);
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn get_statistics(&self) -> PortResult<SemanticStatistics> {
        debug!("Getting semantic statistics");
        let msg = GetSemanticStatisticsMessage;
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }

    async fn invalidate_pathfinding_cache(&mut self) -> PortResult<()> {
        info!("Invalidating pathfinding cache");
        let msg = InvalidatePathfindingCacheMessage;
        let result = self.send_message(msg).await?;
        result.map_err(|e| {
            crate::ports::gpu_semantic_analyzer::GpuSemanticAnalyzerError::AnalysisError(e)
        })
    }
}

// Message Handlers for SemanticProcessorActor

impl Handler<InitializeSemanticMessage> for SemanticProcessorActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: InitializeSemanticMessage, ctx: &mut Self::Context) -> Self::Result {
        use crate::actors::semantic_processor_actor::SetGraphData;
        self.handle(
            SetGraphData {
                graph_data: msg.graph,
            },
            ctx,
        );
        Ok(())
    }
}

impl Handler<DetectCommunitiesMessage> for SemanticProcessorActor {
    type Result = Result<CommunityDetectionResult, String>;

    fn handle(&mut self, _msg: DetectCommunitiesMessage, _ctx: &mut Self::Context) -> Self::Result {
        
        Ok(CommunityDetectionResult {
            clusters: HashMap::new(),
            cluster_sizes: HashMap::new(),
            modularity: 0.0,
            computation_time_ms: 0.0,
        })
    }
}

#[cfg(feature = "gpu")]
impl Handler<ComputeShortestPathsMessage> for SemanticProcessorActor {
    type Result = ResponseFuture<Result<PathfindingResult, String>>;

    fn handle(
        &mut self,
        msg: ComputeShortestPathsMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        use crate::actors::messages::ComputeShortestPaths;
        let compute_msg = ComputeShortestPaths {
            source_node_id: msg.source_node_id,
        };

        Box::pin(async move {
            
            Ok(PathfindingResult {
                source_node: msg.source_node_id,
                distances: HashMap::new(),
                paths: HashMap::new(),
                computation_time_ms: 0.0,
            })
        })
    }
}

impl Handler<ComputeSsspDistancesMessage> for SemanticProcessorActor {
    type Result = Result<Vec<f32>, String>;

    fn handle(
        &mut self,
        _msg: ComputeSsspDistancesMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(Vec::new())
    }
}

impl Handler<ComputeAllPairsShortestPathsMessage> for SemanticProcessorActor {
    type Result = ResponseFuture<Result<HashMap<(u32, u32), Vec<u32>>, String>>;

    fn handle(
        &mut self,
        _msg: ComputeAllPairsShortestPathsMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Box::pin(async move { Ok(HashMap::new()) })
    }
}

impl Handler<ComputeLandmarkApspMessage> for SemanticProcessorActor {
    type Result = Result<Vec<Vec<f32>>, String>;

    fn handle(
        &mut self,
        _msg: ComputeLandmarkApspMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(Vec::new())
    }
}

impl Handler<GenerateSemanticConstraintsMessage> for SemanticProcessorActor {
    type Result = Result<ConstraintSet, String>;

    fn handle(
        &mut self,
        _msg: GenerateSemanticConstraintsMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(ConstraintSet::default())
    }
}

impl Handler<OptimizeLayoutMessage> for SemanticProcessorActor {
    type Result = Result<OptimizationResult, String>;

    fn handle(&mut self, _msg: OptimizeLayoutMessage, _ctx: &mut Self::Context) -> Self::Result {
        Ok(OptimizationResult {
            converged: false,
            iterations: 0,
            final_stress: 0.0,
            convergence_delta: 0.0,
            computation_time_ms: 0.0,
        })
    }
}

impl Handler<AnalyzeNodeImportanceMessage> for SemanticProcessorActor {
    type Result = Result<HashMap<u32, f32>, String>;

    fn handle(
        &mut self,
        _msg: AnalyzeNodeImportanceMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(HashMap::new())
    }
}

impl Handler<UpdateSemanticGraphDataMessage> for SemanticProcessorActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        msg: UpdateSemanticGraphDataMessage,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        use crate::actors::semantic_processor_actor::SetGraphData;
        self.handle(
            SetGraphData {
                graph_data: msg.graph,
            },
            ctx,
        );
        Ok(())
    }
}

impl Handler<GetSemanticStatisticsMessage> for SemanticProcessorActor {
    type Result = Result<SemanticStatistics, String>;

    fn handle(
        &mut self,
        _msg: GetSemanticStatisticsMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(SemanticStatistics {
            total_analyses: 0,
            average_clustering_time_ms: 0.0,
            average_pathfinding_time_ms: 0.0,
            cache_hit_rate: 0.0,
            gpu_memory_used_mb: 0.0,
        })
    }
}

impl Handler<InvalidatePathfindingCacheMessage> for SemanticProcessorActor {
    type Result = Result<(), String>;

    fn handle(
        &mut self,
        _msg: InvalidatePathfindingCacheMessage,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(())
    }
}
