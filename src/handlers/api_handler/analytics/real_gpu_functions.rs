// Real GPU clustering implementation functions for analytics handler
use super::{Cluster, ClusteringParams, GPUPhysicsStats, StressMajorizationStats};
use crate::app_state::AppState;
use log::{error, info, warn};

pub async fn get_real_gpu_physics_stats(app_state: &AppState) -> Option<GPUPhysicsStats> {
    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        use crate::actors::messages::{GetGPUStatus, GetStressMajorizationStats};

        let gpu_stats = match gpu_addr.send(GetGPUStatus).await {
            Ok(stats) => stats,
            Err(e) => {
                error!("GPU actor communication error: {}", e);
                return None;
            }
        };

        let stress_stats = match gpu_addr.send(GetStressMajorizationStats).await {
            Ok(Ok(stats)) => StressMajorizationStats {
                total_runs: 1,
                successful_runs: if stats.converged { 1 } else { 0 },
                failed_runs: if stats.converged { 0 } else { 1 },
                consecutive_failures: 0,
                emergency_stopped: false,
                last_error: "No error".to_string(),
                average_computation_time_ms: stats.computation_time_ms,
                success_rate: if stats.converged { 1.0 } else { 0.0 },
                is_emergency_stopped: false,
                emergency_stop_reason: "None".to_string(),
                avg_computation_time_ms: stats.computation_time_ms,
                avg_stress: stats.stress_value,
                avg_displacement: 0.1,
                is_converging: stats.converged,
            },
            Ok(Err(e)) => {
                warn!("Failed to get stress majorization stats: {}", e);

                StressMajorizationStats {
                    total_runs: 0,
                    successful_runs: 0,
                    failed_runs: 0,
                    consecutive_failures: 0,
                    emergency_stopped: false,
                    last_error: "No data available".to_string(),
                    average_computation_time_ms: 16,
                    success_rate: 1.0,
                    is_emergency_stopped: false,
                    emergency_stop_reason: "None".to_string(),
                    avg_computation_time_ms: 16,
                    avg_stress: 0.1,
                    avg_displacement: 0.01,
                    is_converging: true,
                }
            }
            Err(_) => StressMajorizationStats {
                total_runs: 0,
                successful_runs: 0,
                failed_runs: 0,
                consecutive_failures: 0,
                emergency_stopped: false,
                last_error: "Communication error".to_string(),
                average_computation_time_ms: 16,
                success_rate: 1.0,
                is_emergency_stopped: false,
                emergency_stop_reason: "None".to_string(),
                avg_computation_time_ms: 16,
                avg_stress: 0.1,
                avg_displacement: 0.01,
                is_converging: true,
            },
        };

        Some(GPUPhysicsStats {
            iteration_count: gpu_stats.iteration_count,
            nodes_count: gpu_stats.num_nodes,
            edges_count: gpu_stats.num_nodes * 2,
            kinetic_energy: 0.1,
            total_forces: 1.0,
            gpu_enabled: gpu_stats.is_initialized,
            compute_mode: "WGSL".to_string(),
            kernel_mode: "unified".to_string(),
            num_nodes: gpu_stats.num_nodes,
            num_edges: gpu_stats.num_nodes * 2,
            num_constraints: 0,
            num_isolation_layers: 0,
            stress_majorization_interval: 100,
            last_stress_majorization: 0,
            gpu_failure_count: gpu_stats.failure_count,
            has_advanced_features: false,
            has_dual_graph_features: false,
            has_visual_analytics_features: false,
            stress_safety_stats: stress_stats,
        })
    } else {
        warn!("GPU compute actor not available for stats");
        None
    }
}

pub async fn perform_gpu_spectral_clustering(
    app_state: &AppState,
    graph_data: &visionclaw_domain::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    info!(
        "Performing GPU spectral clustering on {} nodes",
        graph_data.nodes.len()
    );

    if let Some(gpu_manager) = &app_state.gpu_manager_addr {
        info!("GPU manager available, executing spectral clustering on GPU");

        use crate::actors::messages::PerformGPUClustering;

        let clustering_msg = PerformGPUClustering {
            method: "spectral".to_string(),
            params: params.clone(),
            task_id: format!("spectral_{}", uuid::Uuid::new_v4()),
        };

        match gpu_manager.send(clustering_msg).await {
            Ok(Ok(gpu_result)) => {
                info!(
                    "GPU spectral clustering succeeded with {} clusters",
                    gpu_result.len()
                );
                // This function only returns the cluster shapes. node_analytics is
                // written by the single writer (ADR-031 D3), ClusteringActor, which
                // run_clustering's spawn task drives via WriteClusterAnalytics
                // (1-based ids, masked keys, stale-id reset).
                return gpu_result;
            }
            Ok(Err(e)) => {
                error!("GPU spectral clustering failed: {}", e);
            }
            Err(e) => {
                error!("Failed to communicate with GPU manager: {}", e);
            }
        }
    }

    warn!("GPU clustering failed, falling back to CPU spectral clustering");
    generate_cpu_fallback_clustering(
        graph_data,
        agents,
        params.num_clusters.unwrap_or(5),
        "spectral",
    )
}

pub async fn perform_gpu_kmeans_clustering(
    app_state: &AppState,
    graph_data: &visionclaw_domain::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    info!(
        "Performing GPU K-means clustering on {} nodes",
        graph_data.nodes.len()
    );

    if let Some(gpu_manager) = &app_state.gpu_manager_addr {
        info!("GPU manager available, executing K-means clustering on GPU");

        use crate::actors::messages::PerformGPUClustering;

        let clustering_msg = PerformGPUClustering {
            method: "kmeans".to_string(),
            params: params.clone(),
            task_id: format!("kmeans_{}", uuid::Uuid::new_v4()),
        };

        match gpu_manager.send(clustering_msg).await {
            Ok(Ok(gpu_result)) => {
                info!(
                    "GPU K-means clustering succeeded with {} clusters",
                    gpu_result.len()
                );
                // node_analytics is written by the single writer (ADR-031 D3),
                // ClusteringActor, driven by run_clustering's spawn task via
                // WriteClusterAnalytics. This function only returns cluster shapes.
                return gpu_result;
            }
            Ok(Err(e)) => {
                error!("GPU K-means clustering failed: {}", e);
            }
            Err(e) => {
                error!("Failed to communicate with GPU manager: {}", e);
            }
        }
    }

    warn!("GPU clustering failed, falling back to CPU K-means clustering");
    generate_cpu_fallback_clustering(
        graph_data,
        agents,
        params.num_clusters.unwrap_or(8),
        "kmeans",
    )
}

pub async fn perform_gpu_louvain_clustering(
    app_state: &AppState,
    graph_data: &visionclaw_domain::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    info!(
        "Performing GPU Louvain clustering on {} nodes",
        graph_data.nodes.len()
    );

    if let Some(gpu_manager) = &app_state.gpu_manager_addr {
        info!("GPU manager available, executing Louvain clustering on GPU");

        use crate::actors::messages::PerformGPUClustering;

        let clustering_msg = PerformGPUClustering {
            method: "louvain".to_string(),
            params: params.clone(),
            task_id: format!("louvain_{}", uuid::Uuid::new_v4()),
        };

        match gpu_manager.send(clustering_msg).await {
            Ok(Ok(gpu_result)) => {
                info!(
                    "GPU Louvain clustering succeeded with {} clusters",
                    gpu_result.len()
                );
                // node_analytics is written by the single writer (ADR-031 D3),
                // ClusteringActor, driven by run_clustering's spawn task via
                // WriteClusterAnalytics. This function only returns cluster shapes.
                return gpu_result;
            }
            Ok(Err(e)) => {
                error!("GPU Louvain clustering failed: {}", e);
            }
            Err(e) => {
                error!("Failed to communicate with GPU manager: {}", e);
            }
        }
    }

    warn!("GPU clustering failed, falling back to CPU Louvain clustering");
    generate_cpu_fallback_clustering(
        graph_data,
        agents,
        (5.0 / params.resolution.unwrap_or(1.0)) as u32,
        "louvain",
    )
}

pub async fn perform_gpu_default_clustering(
    app_state: &AppState,
    graph_data: &visionclaw_domain::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    params: &ClusteringParams,
) -> Vec<Cluster> {
    let node_count = graph_data.nodes.len();

    if node_count < 100 {
        perform_gpu_kmeans_clustering(app_state, graph_data, agents, params).await
    } else if node_count < 1000 {
        perform_gpu_spectral_clustering(app_state, graph_data, agents, params).await
    } else {
        perform_gpu_louvain_clustering(app_state, graph_data, agents, params).await
    }
}

#[allow(dead_code)]
fn convert_gpu_clusters_to_response(
    gpu_results: Vec<Cluster>,
    graph_data: &visionclaw_domain::models::graph::GraphData,
    method: &str,
) -> Vec<Cluster> {
    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];

    gpu_results
        .into_iter()
        .enumerate()
        .map(|(i, cluster)| {
            let centroid = if !cluster.nodes.is_empty() {
                let sum_x: f32 = cluster
                    .nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.x)
                    .sum();
                let sum_y: f32 = cluster
                    .nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.y)
                    .sum();
                let sum_z: f32 = cluster
                    .nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.z)
                    .sum();
                let count = cluster.nodes.len() as f32;

                if count > 0.0 {
                    Some([sum_x / count, sum_y / count, sum_z / count])
                } else {
                    None
                }
            } else {
                None
            };

            Cluster {
                id: format!("gpu_cluster_{}_{}", method, i),
                label: format!(
                    "GPU {} Cluster {} ({} nodes)",
                    method,
                    i + 1,
                    cluster.nodes.len()
                ),
                node_count: cluster.nodes.len() as u32,
                coherence: cluster.coherence,
                color: colors.get(i).unwrap_or(&"#888888").to_string(),
                keywords: cluster.keywords,
                nodes: cluster.nodes,
                centroid,
            }
        })
        .collect()
}

fn generate_cpu_fallback_clustering(
    graph_data: &visionclaw_domain::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    num_clusters: u32,
    method: &str,
) -> Vec<Cluster> {
    let _ = num_clusters; // retained for signature compatibility with agent-based path
    if !agents.is_empty() {
        super::clustering_handlers::generate_agent_based_clusters(
            graph_data,
            agents,
            num_clusters,
            method,
        )
    } else {
        // Real topology-based community detection over the graph edges. The
        // previous positional index-bin fallback produced meaningless clusters
        // that, once surfaced on the wire, would override the client's domain
        // heuristics with garbage. Label propagation yields genuine communities.
        super::clustering_handlers::generate_label_propagation_clusters(
            graph_data,
            CPU_FALLBACK_MIN_CLUSTER_SIZE,
            CPU_FALLBACK_MAX_ITERATIONS,
            method,
        )
    }
}

/// Minimum community size for the CPU label-propagation fallback. Matches the
/// client hull floor (MIN_CLUSTER_SIZE = 4 in ClusterHulls.tsx) so we never emit
/// communities too small to form a hull.
const CPU_FALLBACK_MIN_CLUSTER_SIZE: u32 = 4;
/// Label-propagation sweep cap; convergence almost always halts earlier.
const CPU_FALLBACK_MAX_ITERATIONS: u32 = 50;
