//! Anomaly Detection Actor - Handles anomaly detection algorithms

use actix::prelude::*;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::analytics_telemetry::{record_execution, AnalyticsKernel, ExecutionPath};
use super::shared::{GPUState, SharedGPUContext};
use crate::actors::messages::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyNode {
    pub node_id: u32,
    pub anomaly_score: f32,
    pub reason: String,
    pub anomaly_type: String,
    pub severity: String,
    pub explanation: String,
    pub features: Vec<String>,
}

/// Type alias for the shared node analytics map: node_id -> NodeAnalytics
type NodeAnalyticsMap = Arc<
    std::sync::RwLock<std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>>,
>;

pub struct AnomalyDetectionActor {
    gpu_state: GPUState,

    shared_context: Option<Arc<SharedGPUContext>>,

    /// Maps GPU buffer index -> actual graph node ID.
    /// Populated lazily from the GPU `node_graph_id` buffer before detection.
    /// When empty, raw buffer indices are used as-is (backward compat).
    node_id_map: Vec<u32>,

    /// Shared analytics store — populated after anomaly detection so the binary broadcast
    /// path can embed real anomaly_score values in V3 wire format (ADR-014 DL4 fix).
    node_analytics: Option<NodeAnalyticsMap>,
}

impl AnomalyDetectionActor {
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
    /// Mirrors ClusteringActor::ensure_node_id_map so anomaly writes key by the
    /// same graph node id the V3 encoder masks-and-looks-up.
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
                                "AnomalyDetectionActor: Downloaded node_id_map ({} entries) from GPU",
                                self.node_id_map.len()
                            );
                        }
                    }
                }
            }
        }
    }

    /// Translate a GPU buffer index to the actual graph node ID, then mask to the
    /// 26-bit node-id space the V3 encoder keys node_analytics by. Falls back to
    /// the raw (masked) index when no mapping is available.
    #[inline]
    fn translate_masked(&self, gpu_index: usize) -> u32 {
        let raw = if gpu_index < self.node_id_map.len() {
            self.node_id_map[gpu_index]
        } else {
            gpu_index as u32
        };
        raw & crate::utils::binary_protocol::NODE_ID_MASK
    }
}

impl Default for AnomalyDetectionActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for AnomalyDetectionActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Anomaly Detection Actor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Anomaly Detection Actor stopped");
    }
}

// === Message Handlers ===

impl Handler<RunAnomalyDetection> for AnomalyDetectionActor {
    type Result = ResponseActFuture<Self, Result<AnomalyResult, String>>;

    fn handle(&mut self, msg: RunAnomalyDetection, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "AnomalyDetectionActor: Anomaly detection request received for method {:?}",
            msg.params.method
        );

        if self.shared_context.is_none() {
            error!("AnomalyDetectionActor: GPU not initialized for anomaly detection");
            return Box::pin(
                async move { Err("GPU not initialized".to_string()) }.into_actor(self),
            );
        }

        if self.gpu_state.num_nodes == 0 {
            error!("AnomalyDetectionActor: No nodes available for anomaly detection");
            return Box::pin(
                async move { Err("No nodes available for anomaly detection".to_string()) }
                    .into_actor(self),
            );
        }

        let params = msg.params;

        let num_nodes = self.gpu_state.num_nodes;
        let k_neighbors = params.k_neighbors;
        if k_neighbors as u32 >= num_nodes {
            let error_msg = format!(
                "k_neighbors ({}) must be less than total nodes ({})",
                k_neighbors, num_nodes
            );
            return Box::pin(async move { Err(error_msg) }.into_actor(self));
        }

        let internal_params = AnomalyDetectionParams {
            method: match params.method {
                crate::actors::messages::AnomalyMethod::LocalOutlierFactor => {
                    AnomalyDetectionMethod::LOF
                }
                crate::actors::messages::AnomalyMethod::ZScore => AnomalyDetectionMethod::ZScore,
            },
            threshold: Some(params.threshold),
            k_neighbors: Some(params.k_neighbors),
            window_size: Some(100),
            feature_data: None,
        };

        let start_time = std::time::Instant::now();

        // Clone Arc for move into spawn_blocking
        let shared_ctx = match self.shared_context.as_ref() {
            Some(ctx) => Arc::clone(ctx),
            None => {
                return Box::pin(
                    async move {
                        Err("Shared context not initialized for anomaly detection".to_string())
                    }
                    .into_actor(self),
                );
            }
        };
        let num_nodes = self.gpu_state.num_nodes;
        let internal_method = internal_params.method.clone();
        let internal_threshold = internal_params.threshold;
        let internal_k_neighbors = internal_params.k_neighbors;

        // Capture all values needed for the async + actor future
        let internal_params_method = internal_params.method.clone();
        let internal_params_threshold = internal_params.threshold;

        let future = async move {
            // Move blocking GPU operations to dedicated blocking thread pool
            // This prevents std::sync::Mutex::lock() from blocking Tokio worker threads
            let blocking_result = tokio::task::spawn_blocking(move || {
                let mut unified_compute = match shared_ctx.unified_compute.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("AnomalyDetectionActor: GPU mutex was poisoned, recovering");
                        poisoned.into_inner()
                    }
                };

                match internal_method {
                    AnomalyDetectionMethod::LOF => {
                        let k_neighbors = internal_k_neighbors.unwrap_or(5);
                        let threshold = internal_threshold.unwrap_or(0.5);

                        match unified_compute.run_lof_anomaly_detection(k_neighbors, threshold) {
                            Ok(lof_result) => {
                                // Task #74: LOF is GPU-only (the Err branch below surfaces
                                // a failure rather than substituting a CPU path). Record
                                // the GPU path on success.
                                record_execution(AnalyticsKernel::Lof, ExecutionPath::Gpu);
                                let lof_scores = lof_result.0;
                                let mut anomalies = Vec::new();

                                for (node_id, &score) in lof_scores.iter().enumerate() {
                                    if score > threshold {
                                        anomalies.push(AnomalyNode {
                                            node_id: node_id as u32,
                                            anomaly_score: score,
                                            reason: format!("LOF score {:.3} exceeds threshold {:.3}", score, threshold),
                                            anomaly_type: "outlier".to_string(),
                                            severity: if score > threshold * 3.0 { "high" } else { "medium" }.to_string(),
                                            explanation: format!("LOF anomaly detected with score {:.3}", score),
                                            features: vec!["lof_score".to_string()],
                                        });
                                    }
                                }

                                Ok((Some(lof_scores), anomalies))
                            }
                            Err(e) => Err(format!("GPU LOF detection failed: {}", e)),
                        }
                    }
                    AnomalyDetectionMethod::ZScore => {
                        // Use GPU positions as feature data instead of synthetic values
                        let feature_data: Vec<f32> = match unified_compute.get_node_positions() {
                            Ok((pos_x, pos_y, pos_z)) => {
                                (0..num_nodes as usize)
                                    .map(|i| {
                                        if i < pos_x.len() {
                                            (pos_x[i] * pos_x[i] + pos_y[i] * pos_y[i] + pos_z[i] * pos_z[i]).sqrt()
                                        } else {
                                            0.0
                                        }
                                    })
                                    .collect()
                            }
                            Err(e) => return Err(format!("Failed to get node positions for ZScore features: {}", e)),
                        };

                        match unified_compute.run_zscore_anomaly_detection(&feature_data) {
                            Ok(z_scores) => {
                                let threshold = internal_threshold.unwrap_or(3.0);
                                let mut anomalies = Vec::new();

                                for (node_id, &score) in z_scores.iter().enumerate() {
                                    let abs_score = score.abs();
                                    if abs_score > threshold {
                                        anomalies.push(AnomalyNode {
                                            node_id: node_id as u32,
                                            anomaly_score: abs_score,
                                            reason: format!("Z-score {:.3} exceeds threshold {:.3}", abs_score, threshold),
                                            anomaly_type: "statistical_outlier".to_string(),
                                            severity: if abs_score > threshold * 2.0 { "high" } else { "medium" }.to_string(),
                                            explanation: format!("Statistical anomaly detected with Z-score {:.3}", score),
                                            features: vec!["z_score".to_string()],
                                        });
                                    }
                                }

                                Ok((Some(z_scores), anomalies))
                            }
                            Err(e) => Err(format!("GPU Z-Score detection failed: {}", e)),
                        }
                    }
                    AnomalyDetectionMethod::DBSCAN => {
                        let eps = internal_threshold.unwrap_or(50.0);
                        let min_pts = 3;

                        match unified_compute.run_dbscan_clustering(eps, min_pts) {
                            Ok(cluster_labels) => {
                                let mut anomalies = Vec::new();

                                for (node_id, &label) in cluster_labels.iter().enumerate() {
                                    if label == -1 {
                                        anomalies.push(AnomalyNode {
                                            node_id: node_id as u32,
                                            anomaly_score: 1.0,
                                            reason: format!("Node classified as noise by DBSCAN (eps={:.2})", eps),
                                            anomaly_type: "spatial_outlier".to_string(),
                                            severity: "high".to_string(),
                                            explanation: "DBSCAN identified this node as noise (not belonging to any cluster)".to_string(),
                                            features: vec!["spatial_isolation".to_string()],
                                        });
                                    }
                                }

                                Ok((None, anomalies))
                            }
                            Err(e) => Err(format!("GPU DBSCAN detection failed: {}", e)),
                        }
                    }
                    _ => Err("Unsupported anomaly detection method".to_string()),
                }
            }).await;

            // Handle spawn_blocking join result
            let result: Result<(Option<Vec<f32>>, Vec<AnomalyNode>), String> = match blocking_result
            {
                Ok(inner_result) => inner_result,
                Err(join_err) => Err(format!("GPU blocking task panicked: {}", join_err)),
            };

            let computation_time = start_time.elapsed();

            match result {
                Ok((scores, anomalies)) => {
                    let anomalies_count = anomalies.len();
                    let avg_score = if !anomalies.is_empty() {
                        anomalies.iter().map(|a| a.anomaly_score).sum::<f32>()
                            / anomalies.len() as f32
                    } else {
                        0.0
                    };
                    let max_score = anomalies
                        .iter()
                        .map(|a| a.anomaly_score)
                        .fold(0.0, f32::max);
                    let min_score = anomalies
                        .iter()
                        .map(|a| a.anomaly_score)
                        .fold(f32::INFINITY, f32::min);

                    info!("AnomalyDetectionActor: GPU {:?} detection completed in {:?}, found {} anomalies",
                              internal_params_method, computation_time, anomalies_count);

                    Ok(AnomalyResult {
                        lof_scores: if matches!(internal_params_method, AnomalyDetectionMethod::LOF)
                        {
                            scores.clone()
                        } else {
                            None
                        },
                        local_densities: None,
                        zscore_values: if matches!(
                            internal_params_method,
                            AnomalyDetectionMethod::ZScore
                        ) {
                            scores
                        } else {
                            None
                        },
                        anomaly_threshold: internal_params_threshold.unwrap_or(0.5),
                        num_anomalies: anomalies_count,
                        anomalies,
                        stats: crate::actors::messages::AnomalyDetectionStats {
                            total_nodes_analyzed: num_nodes,
                            anomalies_found: anomalies_count,
                            detection_threshold: internal_params_threshold.unwrap_or(0.5),
                            computation_time_ms: computation_time.as_millis() as u64,
                            method: internal_params_method.clone(),
                            average_anomaly_score: avg_score,
                            max_anomaly_score: max_score,
                            min_anomaly_score: min_score,
                        },
                        method: internal_params_method.clone(),
                        threshold: internal_params_threshold.unwrap_or(0.5),
                    })
                }
                Err(e) => {
                    error!("AnomalyDetectionActor: GPU detection failed: {}", e);
                    Err(e)
                }
            }
        };

        Box::pin(future.into_actor(self).map(|result, actor, _ctx| {
            // ADR-014 DL4 / ADR-031 D3-D4: AnomalyDetectionActor is the single writer
            // of node_analytics.anomaly so the V3 binary broadcast carries real scores.
            // Keys MUST be the masked graph node id (the encoder does
            // `base_id = wire_id & NODE_ID_MASK` before lookup) — raw GPU buffer
            // indices silently miss. The previous unmasked `i as u32` keying was a
            // no-op on any graph whose node ids carry type-flag high bits.
            if let Ok(ref anomaly_result) = result {
                actor.ensure_node_id_map();
                if let Some(ref analytics_map) = actor.node_analytics {
                    if let Ok(mut map) = analytics_map.write() {
                        // Single-writer stale reset: nodes dropped from this run must
                        // not retain a previous detection's score.
                        for entry in map.values_mut() {
                            entry.anomaly = 0.0;
                        }

                        if let Some(ref lof_scores) = anomaly_result.lof_scores {
                            for (gpu_idx, &score) in lof_scores.iter().enumerate() {
                                let node_id = actor.translate_masked(gpu_idx);
                                map.entry(node_id).or_default().anomaly = score;
                            }
                        }
                        if let Some(ref zscore_values) = anomaly_result.zscore_values {
                            for (gpu_idx, &score) in zscore_values.iter().enumerate() {
                                let node_id = actor.translate_masked(gpu_idx);
                                map.entry(node_id).or_default().anomaly = score.abs();
                            }
                        }
                        // Methods without a full per-node score vector: key the
                        // anomaly-list node ids (raw GPU buffer indices) the same way.
                        if anomaly_result.lof_scores.is_none()
                            && anomaly_result.zscore_values.is_none()
                        {
                            for anomaly in &anomaly_result.anomalies {
                                let node_id = actor.translate_masked(anomaly.node_id as usize);
                                map.entry(node_id).or_default().anomaly = anomaly.anomaly_score;
                            }
                        }
                        info!(
                            "AnomalyDetectionActor: Populated node_analytics with anomaly scores for {} entries",
                            map.len()
                        );
                    }
                }
            }
            result
        }))
    }
}

impl Handler<SetSharedGPUContext> for AnomalyDetectionActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: SetSharedGPUContext, _ctx: &mut Self::Context) -> Self::Result {
        info!("AnomalyDetectionActor: Received SharedGPUContext from ResourceActor");
        self.shared_context = Some(msg.context);

        info!("AnomalyDetectionActor: SharedGPUContext stored successfully");
        Ok(())
    }
}

impl Handler<SetNodeAnalytics> for AnomalyDetectionActor {
    type Result = ();

    fn handle(&mut self, msg: SetNodeAnalytics, _ctx: &mut Self::Context) {
        info!("AnomalyDetectionActor: Received shared node_analytics map");
        self.node_analytics = Some(msg.node_analytics);
    }
}
