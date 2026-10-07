use log::{error, info};

use super::{Cluster, ClusteringRequest};
use crate::AppState;

pub async fn perform_clustering(
    app_state: &actix_web::web::Data<AppState>,
    request: &ClusteringRequest,
    task_id: &str,
) -> Result<Vec<Cluster>, String> {
    info!(
        "Performing {} clustering for task {}",
        request.method, task_id
    );

    // Route through GPUManagerActor -> AnalyticsSupervisor -> ClusteringActor
    // (not ForceComputeActor which only stubs clustering)
    if let Some(gpu_manager) = app_state.gpu_manager_addr.as_ref() {
        use crate::actors::messages::PerformGPUClustering;

        info!(
            "Using GPU-accelerated clustering via GPUManager for method: {}",
            request.method
        );

        if let Err(validation_error) = validate_clustering_params(request) {
            error!(
                "Clustering parameter validation failed: {}",
                validation_error
            );
            return Err(validation_error);
        }

        let msg = PerformGPUClustering {
            method: request.method.clone(),
            params: request.params.clone(),
            task_id: task_id.to_string(),
        };

        match gpu_manager.send(msg).await {
            Ok(Ok(clusters)) => {
                info!(
                    "GPU clustering completed successfully: {} clusters found",
                    clusters.len()
                );
                // ADR-031 D3 single-writer: ClusteringActor populates node_analytics
                // (masked key, 1-based id, stale reset) while running the kernels. This
                // path no longer writes node_analytics — the previous 0-based unmasked
                // write here both violated I-6 and missed the encoder's masked lookup.
                return Ok(clusters);
            }
            Ok(Err(e)) => {
                error!("GPU clustering failed: {}", e);
                return Err(format!("GPU clustering failed: {}. GPU is required for clustering; CPU fallback not available.", e));
            }
            Err(e) => {
                error!("GPU actor mailbox error: {}", e);
                return Err(format!("GPU compute actor unavailable: {}. GPU is required for clustering; CPU fallback not available.", e));
            }
        }
    }

    // No GPU manager available -- GPU is required for all clustering methods
    error!(
        "GPU compute not available. GPU is required for clustering; CPU fallback not available."
    );
    Err(
        "GPU compute not available. GPU is required for clustering; CPU fallback not available."
            .to_string(),
    )
}

fn validate_clustering_params(request: &ClusteringRequest) -> Result<(), String> {
    let valid_methods = [
        "spectral",
        "hierarchical",
        "dbscan",
        "kmeans",
        "louvain",
        "leiden",
        "communities",
        "affinity",
    ];
    if !valid_methods.contains(&request.method.as_str()) {
        return Err(format!(
            "Unsupported clustering method: {}. Valid methods: {:?}",
            request.method, valid_methods
        ));
    }

    match request.method.as_str() {
        "kmeans" | "spectral" => {
            if let Some(num_clusters) = request.params.num_clusters {
                if !(2..=1000).contains(&num_clusters) {
                    return Err("num_clusters must be between 2 and 1000".to_string());
                }
            }
        }
        "dbscan" => {
            if let Some(eps) = request.params.eps {
                if eps <= 0.0 || eps > 10.0 {
                    return Err("eps must be between 0.0 and 10.0".to_string());
                }
            }
            if let Some(min_samples) = request.params.min_samples {
                if !(1..=1000).contains(&min_samples) {
                    return Err("min_samples must be between 1 and 1000".to_string());
                }
            }
        }
        "hierarchical" => {
            if let Some(threshold) = request.params.distance_threshold {
                if threshold <= 0.0 || threshold > 1.0 {
                    return Err("distance_threshold must be between 0.0 and 1.0".to_string());
                }
            }
        }
        "affinity" => {
            if let Some(damping) = request.params.damping {
                if damping <= 0.0 || damping >= 1.0 {
                    return Err("damping must be between 0.0 and 1.0 (exclusive)".to_string());
                }
            }
        }
        "louvain" | "leiden" | "communities" => {
            if let Some(resolution) = request.params.resolution {
                if resolution <= 0.0 || resolution > 10.0 {
                    return Err("resolution must be between 0.0 and 10.0".to_string());
                }
            }
        }
        _ => {}
    }

    Ok(())
}
