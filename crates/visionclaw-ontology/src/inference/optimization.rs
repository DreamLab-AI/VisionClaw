// src/inference/optimization.rs
//! Inference Optimization
//!
//! Performance optimizations for inference including batch processing,
//! incremental reasoning, and parallel classification.

use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use visionclaw_domain::ports::inference_engine::{InferenceEngine, Result as EngineResult};
use visionclaw_domain::ports::ontology_repository::{InferenceResults, OwlAxiom, OwlClass};

#[derive(Debug, Clone)]
pub struct BatchInferenceRequest {
    pub ontology_ids: Vec<String>,

    pub max_parallelism: usize,

    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct IncrementalChange {
    pub added_classes: Vec<OwlClass>,

    pub removed_classes: Vec<OwlClass>,

    pub added_axioms: Vec<OwlAxiom>,

    pub removed_axioms: Vec<OwlAxiom>,
}

pub struct IncrementalInference {
    previous_checksum: Option<String>,

    previous_results: Option<InferenceResults>,

    changes: Vec<IncrementalChange>,
}

impl IncrementalInference {
    pub fn new() -> Self {
        Self {
            previous_checksum: None,
            previous_results: None,
            changes: Vec::new(),
        }
    }

    pub fn add_change(&mut self, change: IncrementalChange) {
        self.changes.push(change);
    }

    pub fn can_use_incremental(&self) -> bool {
        self.previous_results.is_some() && !self.changes.is_empty()
    }

    pub fn get_accumulated_changes(&self) -> Vec<IncrementalChange> {
        self.changes.clone()
    }

    pub fn clear_changes(&mut self) {
        self.changes.clear();
    }

    pub fn update_state(&mut self, checksum: String, results: InferenceResults) {
        self.previous_checksum = Some(checksum);
        self.previous_results = Some(results);
        self.clear_changes();
    }
}

impl Default for IncrementalInference {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ParallelClassification {
    worker_count: usize,
}

impl ParallelClassification {
    pub fn new(worker_count: usize) -> Self {
        Self { worker_count }
    }

    pub async fn classify_batch(
        &self,
        engine: Arc<RwLock<dyn InferenceEngine>>,
        ontology_ids: Vec<String>,
    ) -> EngineResult<HashMap<String, Vec<(String, String)>>> {
        let chunk_size = ontology_ids.len().div_ceil(self.worker_count);
        let chunks: Vec<Vec<String>> = ontology_ids
            .chunks(chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        let tasks: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                let engine_clone = Arc::clone(&engine);
                tokio::spawn(async move {
                    let mut results = HashMap::new();
                    for ont_id in chunk {
                        let eng = engine_clone.read().await;
                        if let Ok(hierarchy) = eng.get_subclass_hierarchy().await {
                            results.insert(ont_id, hierarchy);
                        }
                    }
                    results
                })
            })
            .collect();

        let chunk_results = join_all(tasks).await;

        let mut final_results = HashMap::new();
        for chunk_map in chunk_results.into_iter().flatten() {
            final_results.extend(chunk_map);
        }

        Ok(final_results)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptimizationMetrics {
    pub total_time_ms: u64,

    pub ontologies_processed: usize,

    pub parallel_tasks: usize,

    pub avg_time_per_ontology: f64,

    pub speedup_factor: f64,

    pub cache_hit_rate: f64,
}

impl OptimizationMetrics {
    pub fn calculate(
        total_time_ms: u64,
        ontologies_processed: usize,
        parallel_tasks: usize,
        sequential_baseline_ms: u64,
        cache_hits: u64,
        cache_misses: u64,
    ) -> Self {
        let avg_time = if ontologies_processed > 0 {
            total_time_ms as f64 / ontologies_processed as f64
        } else {
            0.0
        };

        let speedup = if sequential_baseline_ms > 0 {
            sequential_baseline_ms as f64 / total_time_ms as f64
        } else {
            1.0
        };

        let total_cache_ops = cache_hits + cache_misses;
        let hit_rate = if total_cache_ops > 0 {
            cache_hits as f64 / total_cache_ops as f64
        } else {
            0.0
        };

        Self {
            total_time_ms,
            ontologies_processed,
            parallel_tasks,
            avg_time_per_ontology: avg_time,
            speedup_factor: speedup,
            cache_hit_rate: hit_rate,
        }
    }
}

pub struct InferenceOptimizer {
    incremental: Arc<RwLock<IncrementalInference>>,

    #[allow(dead_code)]
    parallel: ParallelClassification,

    metrics: Arc<RwLock<OptimizationMetrics>>,
}

impl InferenceOptimizer {
    pub fn new(worker_count: usize) -> Self {
        Self {
            incremental: Arc::new(RwLock::new(IncrementalInference::new())),
            parallel: ParallelClassification::new(worker_count),
            metrics: Arc::new(RwLock::new(OptimizationMetrics::default())),
        }
    }

    pub async fn process_batch(
        &self,
        engine: Arc<RwLock<dyn InferenceEngine>>,
        request: BatchInferenceRequest,
    ) -> EngineResult<HashMap<String, InferenceResults>> {
        let start = std::time::Instant::now();

        let chunk_size = request.ontology_ids.len().div_ceil(request.max_parallelism);

        let chunks: Vec<Vec<String>> = request
            .ontology_ids
            .chunks(chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        let tasks: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                let engine_clone = Arc::clone(&engine);
                tokio::spawn(async move {
                    let mut results = HashMap::new();
                    for ont_id in chunk {
                        let mut eng = engine_clone.write().await;
                        if let Ok(inference_results) = eng.infer().await {
                            results.insert(ont_id, inference_results);
                        }
                    }
                    results
                })
            })
            .collect();

        let chunk_results = join_all(tasks).await;

        let mut final_results = HashMap::new();
        for chunk_map in chunk_results.into_iter().flatten() {
            final_results.extend(chunk_map);
        }

        let elapsed = start.elapsed().as_millis() as u64;

        let mut metrics = self.metrics.write().await;
        metrics.total_time_ms = elapsed;
        metrics.ontologies_processed = request.ontology_ids.len();
        metrics.parallel_tasks = request.max_parallelism;
        metrics.avg_time_per_ontology = elapsed as f64 / request.ontology_ids.len() as f64;

        Ok(final_results)
    }

    pub async fn get_metrics(&self) -> OptimizationMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn add_change(&self, change: IncrementalChange) {
        let mut incr = self.incremental.write().await;
        incr.add_change(change);
    }

    pub async fn can_use_incremental(&self) -> bool {
        let incr = self.incremental.read().await;
        incr.can_use_incremental()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incremental_inference_creation() {
        let incr = IncrementalInference::new();
        assert!(!incr.can_use_incremental());
    }

    #[test]
    fn test_incremental_change_addition() {
        let mut incr = IncrementalInference::new();

        let change = IncrementalChange {
            added_classes: vec![],
            removed_classes: vec![],
            added_axioms: vec![],
            removed_axioms: vec![],
        };

        incr.add_change(change);
        assert_eq!(incr.changes.len(), 1);
    }

    #[test]
    fn test_optimization_metrics_calculation() {
        let metrics = OptimizationMetrics::calculate(1000, 10, 4, 5000, 80, 20);

        assert_eq!(metrics.total_time_ms, 1000);
        assert_eq!(metrics.ontologies_processed, 10);
        assert_eq!(metrics.avg_time_per_ontology, 100.0);
        assert!(metrics.speedup_factor > 1.0);
        assert_eq!(metrics.cache_hit_rate, 0.8);
    }

    #[test]
    fn test_parallel_classification_creation() {
        let parallel = ParallelClassification::new(4);
        assert_eq!(parallel.worker_count, 4);
    }
}
