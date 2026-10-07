use actix_web::{web, HttpResponse, Result};
use log::{info, warn};
use std::collections::HashMap;
use uuid::Uuid;

use crate::actors::messages::GetGraphData;
use crate::services::agent_visualization_protocol::McpServerType;
use crate::utils::mcp_tcp_client::create_mcp_client;
use crate::AppState;
use crate::{not_found, ok_json};

use super::params_handlers::set_focus;
use super::real_gpu_functions::*;
use super::state::CLUSTERING_TASKS;
use super::types::{
    Cluster, ClusterFocusRequest, ClusteringRequest, ClusteringResponse, ClusteringStatusResponse,
    ClusteringTask, FocusRegion, SetFocusRequest,
};

pub async fn run_clustering(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<ClusteringRequest>,
) -> Result<HttpResponse> {
    info!(
        "Starting clustering analysis with method: {}",
        request.method
    );

    let task_id = Uuid::new_v4().to_string();
    let method = request.method.clone();

    let task = ClusteringTask {
        task_id: task_id.clone(),
        method: method.clone(),
        status: "running".to_string(),
        progress: 0.0,
        started_at: chrono::Utc::now().timestamp() as u64,
        clusters: None,
        error: None,
    };

    {
        let mut tasks = CLUSTERING_TASKS.lock().await;
        tasks.insert(task_id.clone(), task);
    }

    let app_state_clone = app_state.clone();
    let task_id_clone = task_id.clone();
    let request_clone = request.into_inner();

    tokio::spawn(async move {
        let clusters = perform_clustering(&app_state_clone, &request_clone, &task_id_clone).await;

        let mut tasks = CLUSTERING_TASKS.lock().await;
        if let Some(task) = tasks.get_mut(&task_id_clone) {
            match clusters {
                Ok(clusters) => {
                    // ADR-031 D3 single-writer: route the finished assignment back
                    // through ClusteringActor (the sole owner of node_analytics) so
                    // node_analytics.cluster_id is populated with masked keys, 1-based
                    // ids, and a stale reset. This covers BOTH outcomes of
                    // perform_clustering — GPU kernels and the CPU label-propagation
                    // fallback — closing the gap where the fallback left the store
                    // null and hull rendering showed empty. The WriteClusterAnalytics
                    // message fans out GPUManagerActor → AnalyticsSupervisor →
                    // ClusteringActor, exactly like SetNodeAnalytics.
                    if let Some(gpu_manager) = app_state_clone.gpu_manager_addr.as_ref() {
                        gpu_manager.do_send(crate::actors::messages::WriteClusterAnalytics {
                            clusters: clusters.clone(),
                        });
                    } else {
                        warn!(
                            "Clustering run {}: GPU manager unavailable; node_analytics.cluster_id \
                             not written (hulls will not render this run)",
                            task_id_clone
                        );
                    }
                    task.status = "completed".to_string();
                    task.progress = 1.0;
                    task.clusters = Some(clusters);
                }
                Err(e) => {
                    task.status = "failed".to_string();
                    task.error = Some(e);
                }
            }
        }
    });

    ok_json!(ClusteringResponse {
        success: true,
        clusters: None,
        method: Some(method),
        execution_time_ms: None,
        task_id: Some(task_id),
        error: None,
    })
}

pub async fn get_clustering_status(
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse> {
    let task_id = query.get("task_id");

    if let Some(task_id) = task_id {
        let tasks = CLUSTERING_TASKS.lock().await;
        if let Some(task) = tasks.get(task_id) {
            let estimated_completion = if task.status == "running" {
                Some(chrono::Utc::now().timestamp() as u64 + 30)
            } else {
                None
            };

            return ok_json!(ClusteringStatusResponse {
                success: true,
                task_id: Some(task.task_id.clone()),
                status: task.status.clone(),
                progress: task.progress,
                method: Some(task.method.clone()),
                started_at: Some(task.started_at.to_string()),
                estimated_completion: estimated_completion.map(|t| t.to_string()),
                error: task.error.clone(),
            });
        }
    }

    Ok(HttpResponse::NotFound().json(ClusteringStatusResponse {
        success: false,
        task_id: None,
        status: "not_found".to_string(),
        progress: 0.0,
        method: None,
        started_at: None,
        estimated_completion: None,
        error: Some("Task not found".to_string()),
    }))
}

pub async fn focus_cluster(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<ClusterFocusRequest>,
) -> Result<HttpResponse> {
    info!("Focusing on cluster: {}", request.cluster_id);

    let tasks = CLUSTERING_TASKS.lock().await;
    let cluster = tasks
        .values()
        .filter_map(|task| task.clusters.as_ref())
        .flatten()
        .find(|c| c.id == request.cluster_id)
        .cloned();

    if let Some(cluster) = cluster {
        if let Some(centroid) = cluster.centroid {
            let focus_request = SetFocusRequest {
                node_id: None,
                region: Some(FocusRegion {
                    center_x: centroid[0],
                    center_y: centroid[1],
                    center_z: centroid[2],
                    radius: request.zoom_level.unwrap_or(5.0),
                }),
                radius: Some(request.zoom_level.unwrap_or(5.0)),
                intensity: Some(1.0),
            };

            let focus_response = set_focus(auth, app_state, web::Json(focus_request)).await?;
            return Ok(focus_response);
        }
    }

    ok_json!(super::types::FocusResponse {
        success: true,
        focus_node: None,
        focus_region: None,
        error: Some("Cluster not found or no centroid available".to_string()),
    })
}

pub async fn cancel_clustering(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse> {
    let task_id = query.get("task_id");

    if let Some(task_id) = task_id {
        info!("Canceling clustering task: {}", task_id);

        let mut tasks = CLUSTERING_TASKS.lock().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = "cancelled".to_string();
            task.error = Some("Cancelled by user".to_string());

            return ok_json!(serde_json::json!({
                "success": true,
                "message": "Task cancelled successfully",
                "task_id": task_id
            }));
        }
    }

    not_found!("Task not found or not cancellable")
}

/// POST /analytics/clustering/dbscan
///
/// Runs standalone DBSCAN clustering as an analytics algorithm.
/// Request body: `{ "epsilon": 0.5, "minPoints": 5 }`
pub async fn run_dbscan_clustering(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    app_state: web::Data<AppState>,
    request: web::Json<serde_json::Value>,
) -> Result<HttpResponse> {
    let body = request.into_inner();

    let epsilon = body
        .get("epsilon")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.5);

    let min_points = body
        .get("minPoints")
        .or_else(|| body.get("min_points"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(5);

    if epsilon <= 0.0 {
        return ok_json!(serde_json::json!({
            "success": false,
            "error": "epsilon must be positive"
        }));
    }
    if min_points == 0 {
        return ok_json!(serde_json::json!({
            "success": false,
            "error": "minPoints must be at least 1"
        }));
    }

    info!(
        "Analytics DBSCAN clustering: epsilon={}, minPoints={}",
        epsilon, min_points
    );

    let task_id = Uuid::new_v4().to_string();

    if let Some(gpu_manager) = app_state.gpu_manager_addr.as_ref() {
        use crate::actors::messages::{DBSCANParams, RunDBSCAN};

        let msg = RunDBSCAN {
            params: DBSCANParams {
                epsilon,
                min_points,
            },
        };

        match gpu_manager.send(msg).await {
            Ok(Ok(result)) => {
                // Store as a clustering task for status tracking
                {
                    let task = ClusteringTask {
                        task_id: task_id.clone(),
                        method: "dbscan".to_string(),
                        status: "completed".to_string(),
                        progress: 1.0,
                        started_at: chrono::Utc::now().timestamp() as u64,
                        clusters: Some(result.clusters.clone()),
                        error: None,
                    };
                    let mut tasks = CLUSTERING_TASKS.lock().await;
                    tasks.insert(task_id.clone(), task);
                }

                ok_json!(serde_json::json!({
                    "success": true,
                    "taskId": task_id,
                    "method": "dbscan",
                    "epsilon": epsilon,
                    "minPoints": min_points,
                    "numClusters": result.num_clusters,
                    "numNoisePoints": result.num_noise_points,
                    "clusters": result.clusters.iter().map(|c| serde_json::json!({
                        "id": c.id,
                        "label": c.label,
                        "nodeCount": c.node_count,
                        "coherence": c.coherence,
                        "nodes": c.nodes,
                        "centroid": c.centroid,
                        "color": c.color,
                        "keywords": c.keywords,
                    })).collect::<Vec<_>>(),
                    "stats": {
                        "totalNodes": result.stats.total_nodes,
                        "numClusters": result.stats.num_clusters,
                        "numNoisePoints": result.stats.num_noise_points,
                        "largestClusterSize": result.stats.largest_cluster_size,
                        "smallestClusterSize": result.stats.smallest_cluster_size,
                        "averageClusterSize": result.stats.average_cluster_size,
                        "computationTimeMs": result.stats.computation_time_ms
                    },
                    "gpuAccelerated": true
                }))
            }
            Ok(Err(e)) => {
                warn!("DBSCAN clustering failed: {}", e);
                ok_json!(serde_json::json!({
                    "success": false,
                    "method": "dbscan",
                    "error": format!("DBSCAN clustering failed: {}", e)
                }))
            }
            Err(e) => {
                warn!("GPU actor communication error: {}", e);
                ok_json!(serde_json::json!({
                    "success": false,
                    "error": "GPU compute actor unavailable"
                }))
            }
        }
    } else {
        ok_json!(serde_json::json!({
            "success": false,
            "error": "GPU compute not available"
        }))
    }
}

pub(crate) async fn perform_clustering(
    app_state: &web::Data<AppState>,
    request: &ClusteringRequest,
    task_id: &str,
) -> Result<Vec<Cluster>, String> {
    info!("Performing real clustering analysis using MCP agent data");

    let graph_data = {
        match app_state.graph_service_addr.send(GetGraphData).await {
            Ok(Ok(data)) => data,
            _ => return Err("Failed to get graph data".to_string()),
        }
    };

    let host = std::env::var("MCP_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("MCP_TCP_PORT")
        .unwrap_or_else(|_| "9500".to_string())
        .parse::<u16>()
        .unwrap_or(9500);

    let mcp_client = create_mcp_client(&McpServerType::ClaudeFlow, &host, port);

    let agents = match mcp_client.query_agent_list().await {
        Ok(agent_list) => {
            info!(
                "Retrieved {} agents from MCP server for clustering",
                agent_list.len()
            );
            agent_list
        }
        Err(e) => {
            warn!(
                "Failed to get agents from MCP server, using graph data: {}",
                e
            );
            Vec::new()
        }
    };

    let clusters = match request.method.as_str() {
        "spectral" => {
            perform_gpu_spectral_clustering(app_state, &graph_data, &agents, &request.params).await
        }
        "kmeans" => {
            perform_gpu_kmeans_clustering(app_state, &graph_data, &agents, &request.params).await
        }
        "louvain" => {
            perform_gpu_louvain_clustering(app_state, &graph_data, &agents, &request.params).await
        }
        _ => perform_gpu_default_clustering(app_state, &graph_data, &agents, &request.params).await,
    };

    let mut tasks = CLUSTERING_TASKS.lock().await;
    if let Some(task) = tasks.get_mut(task_id) {
        task.progress = 0.5;
    }
    drop(tasks);

    Ok(clusters)
}

pub(crate) fn generate_agent_based_clusters(
    graph_data: &visionclaw_domain::models::graph::GraphData,
    agents: &[crate::services::agent_visualization_protocol::MultiMcpAgentStatus],
    num_clusters: u32,
    method: &str,
) -> Vec<Cluster> {
    if agents.is_empty() {
        warn!("No agent data available for clustering, using graph-based clustering");
        return generate_graph_based_clusters(graph_data, num_clusters, method);
    }

    info!(
        "Generating {} clusters from {} real agents using {} method",
        num_clusters,
        agents.len(),
        method
    );

    let mut agent_type_groups: std::collections::HashMap<
        String,
        Vec<&crate::services::agent_visualization_protocol::MultiMcpAgentStatus>,
    > = std::collections::HashMap::new();

    for agent in agents {
        agent_type_groups
            .entry(agent.agent_type.clone())
            .or_default()
            .push(agent);
    }

    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];

    let mut clusters = Vec::new();
    let mut cluster_id = 0;

    for (agent_type, type_agents) in agent_type_groups {
        if cluster_id >= num_clusters {
            break;
        }

        let _avg_cpu = type_agents
            .iter()
            .map(|a| a.performance.cpu_usage)
            .sum::<f32>()
            / type_agents.len() as f32;
        let _avg_memory = type_agents
            .iter()
            .map(|a| a.performance.memory_usage)
            .sum::<f32>()
            / type_agents.len() as f32;
        let avg_health = type_agents
            .iter()
            .map(|a| a.performance.health_score)
            .sum::<f32>()
            / type_agents.len() as f32;
        let _total_tasks = type_agents
            .iter()
            .map(|a| a.performance.tasks_completed)
            .sum::<u32>();

        let cluster_nodes: Vec<u32> = type_agents
            .iter()
            .enumerate()
            .map(|(idx, _)| cluster_id * 100 + idx as u32)
            .take(graph_data.nodes.len() / num_clusters as usize)
            .collect();

        let centroid = if !cluster_nodes.is_empty() && !graph_data.nodes.is_empty() {
            let node_subset: Vec<_> = cluster_nodes
                .iter()
                .filter_map(|&id| graph_data.nodes.get(id as usize))
                .collect();

            if !node_subset.is_empty() {
                let sum_x: f32 = node_subset.iter().map(|n| n.data.x).sum();
                let sum_y: f32 = node_subset.iter().map(|n| n.data.y).sum();
                let sum_z: f32 = node_subset.iter().map(|n| n.data.z).sum();
                let count = node_subset.len() as f32;
                Some([sum_x / count, sum_y / count, sum_z / count])
            } else {
                None
            }
        } else {
            None
        };

        let keywords: Vec<String> = type_agents
            .iter()
            .flat_map(|agent| agent.capabilities.iter())
            .take(5)
            .cloned()
            .collect();

        let coherence = (avg_health / 100.0).min(1.0).max(0.0);

        clusters.push(Cluster {
            id: format!("cluster_{}_{}", method, cluster_id),
            label: format!("{} Agents ({})", agent_type, type_agents.len()),
            node_count: type_agents.len() as u32,
            coherence,
            color: colors
                .get(cluster_id as usize)
                .unwrap_or(&"#888888")
                .to_string(),
            keywords,
            nodes: cluster_nodes,
            centroid,
        });

        cluster_id += 1;
    }

    while clusters.len() < num_clusters as usize && cluster_id < num_clusters {
        clusters.push(Cluster {
            id: format!("cluster_{}_{}", method, cluster_id),
            label: format!("Mixed Cluster {}", cluster_id + 1),
            node_count: 0,
            coherence: 0.5,
            color: colors
                .get(cluster_id as usize)
                .unwrap_or(&"#888888")
                .to_string(),
            keywords: vec![format!("{}_analysis", method)],
            nodes: vec![],
            centroid: None,
        });
        cluster_id += 1;
    }

    info!("Generated {} real clusters from agent data", clusters.len());
    clusters
}

/// Real community detection over the graph topology via asynchronous label
/// propagation. This replaces the previous positional index-bin fallback, which
/// produced meaningless contiguous-range "clusters". Label propagation groups
/// nodes by their actual edge connectivity, so the resulting `cluster.nodes`
/// reflect genuine communities that are worth inspecting in 3D.
///
/// Returned `Cluster.nodes` contain COMPACT node ids (matching `Node.id`), which
/// is what the node_analytics store and the V3 binary broadcast are keyed by.
/// Deterministic: fixed-order sweeps with (max weight, then smallest label)
/// tie-breaking, so repeated runs on the same graph yield identical clusters.
pub(crate) fn generate_label_propagation_clusters(
    graph_data: &visionclaw_domain::models::graph::GraphData,
    min_cluster_size: u32,
    max_iterations: u32,
    method: &str,
) -> Vec<Cluster> {
    let n = graph_data.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    // Dense index <-> compact node id mapping (node ids are compact but not
    // guaranteed contiguous in nodes-vec order).
    let mut id_to_idx: HashMap<u32, usize> = HashMap::with_capacity(n);
    let mut idx_to_id: Vec<u32> = Vec::with_capacity(n);
    for node in &graph_data.nodes {
        id_to_idx.insert(node.id, idx_to_id.len());
        idx_to_id.push(node.id);
    }

    // Weighted undirected adjacency.
    let mut adjacency: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    for edge in &graph_data.edges {
        let (Some(&s), Some(&t)) = (id_to_idx.get(&edge.source), id_to_idx.get(&edge.target))
        else {
            continue;
        };
        if s == t {
            continue;
        }
        let w = if edge.weight > 0.0 { edge.weight } else { 1.0 };
        adjacency[s].push((t, w));
        adjacency[t].push((s, w));
    }

    // Asynchronous label propagation. Async (in-place) updates avoid the
    // bipartite oscillation that synchronous LPA suffers from, and converge
    // quickly. Deterministic tie-break keeps results reproducible.
    let mut labels: Vec<usize> = (0..n).collect();
    let iters = max_iterations.clamp(5, 1000) as usize;
    let mut votes: HashMap<usize, f32> = HashMap::new();
    for _ in 0..iters {
        let mut changed = false;
        for i in 0..n {
            if adjacency[i].is_empty() {
                continue;
            }
            votes.clear();
            for &(nbr, w) in &adjacency[i] {
                *votes.entry(labels[nbr]).or_insert(0.0) += w;
            }
            let mut best_label = labels[i];
            let mut best_w = f32::NEG_INFINITY;
            for (&lbl, &w) in &votes {
                if w > best_w || (w == best_w && lbl < best_label) {
                    best_w = w;
                    best_label = lbl;
                }
            }
            if best_label != labels[i] {
                labels[i] = best_label;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Per-community internal/total incident edge weight, for a real coherence score.
    let mut internal_w: HashMap<usize, f32> = HashMap::new();
    let mut total_w: HashMap<usize, f32> = HashMap::new();
    for edge in &graph_data.edges {
        let (Some(&s), Some(&t)) = (id_to_idx.get(&edge.source), id_to_idx.get(&edge.target))
        else {
            continue;
        };
        let w = if edge.weight > 0.0 { edge.weight } else { 1.0 };
        *total_w.entry(labels[s]).or_insert(0.0) += w;
        *total_w.entry(labels[t]).or_insert(0.0) += w;
        if labels[s] == labels[t] {
            *internal_w.entry(labels[s]).or_insert(0.0) += 2.0 * w;
        }
    }

    // Group nodes by final community label.
    let mut groups: HashMap<usize, Vec<u32>> = HashMap::new();
    for (idx, &lbl) in labels.iter().enumerate() {
        groups.entry(lbl).or_default().push(idx_to_id[idx]);
    }

    // Drop communities smaller than the requested floor; order remaining ones
    // largest-first (then by first node id) so colouring is stable across runs.
    let min_size = min_cluster_size.max(1) as usize;
    let mut groups_vec: Vec<(usize, Vec<u32>)> = groups
        .into_iter()
        .filter(|(_, nodes)| nodes.len() >= min_size)
        .collect();
    groups_vec.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.1[0].cmp(&b.1[0])));

    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];

    info!(
        "generate_label_propagation_clusters: {} communities (>= {} nodes) from {} nodes / {} edges via {}",
        groups_vec.len(),
        min_size,
        n,
        graph_data.edges.len(),
        method
    );

    groups_vec
        .into_iter()
        .enumerate()
        .map(|(i, (lbl, nodes))| {
            let centroid = {
                let mut sum = [0.0f32; 3];
                let mut count = 0.0f32;
                for &id in &nodes {
                    if let Some(&idx) = id_to_idx.get(&id) {
                        if let Some(node) = graph_data.nodes.get(idx) {
                            sum[0] += node.data.x;
                            sum[1] += node.data.y;
                            sum[2] += node.data.z;
                            count += 1.0;
                        }
                    }
                }
                if count > 0.0 {
                    Some([sum[0] / count, sum[1] / count, sum[2] / count])
                } else {
                    None
                }
            };

            let coherence = match (internal_w.get(&lbl), total_w.get(&lbl)) {
                (Some(&i_w), Some(&t_w)) if t_w > 0.0 => (i_w / t_w).clamp(0.0, 1.0),
                _ => 0.0,
            };

            Cluster {
                id: format!("{}_community_{}", method, i),
                label: format!("Community {} ({} nodes)", i + 1, nodes.len()),
                node_count: nodes.len() as u32,
                coherence,
                color: colors[i % colors.len()].to_string(),
                keywords: vec![format!("{}-community", method)],
                nodes,
                centroid,
            }
        })
        .collect()
}

#[allow(dead_code)]
pub(crate) fn generate_graph_based_clusters(
    graph_data: &visionclaw_domain::models::graph::GraphData,
    num_clusters: u32,
    method: &str,
) -> Vec<Cluster> {
    let nodes_per_cluster = if graph_data.nodes.is_empty() {
        0
    } else {
        graph_data.nodes.len() / num_clusters as usize
    };
    let colors = [
        "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F",
    ];
    let labels = [
        "Core Concepts",
        "Implementation",
        "Documentation",
        "Testing",
        "Infrastructure",
        "UI Components",
        "API Layer",
        "Data Models",
    ];

    (0..num_clusters)
        .map(|i| {
            let start_idx = (i as usize) * nodes_per_cluster;
            let end_idx = ((i + 1) as usize * nodes_per_cluster).min(graph_data.nodes.len());
            let cluster_nodes: Vec<u32> = (start_idx..end_idx).map(|idx| idx as u32).collect();

            let centroid = if !cluster_nodes.is_empty() {
                let sum_x: f32 = cluster_nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.x)
                    .sum();
                let sum_y: f32 = cluster_nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.y)
                    .sum();
                let sum_z: f32 = cluster_nodes
                    .iter()
                    .filter_map(|&id| graph_data.nodes.get(id as usize))
                    .map(|n| n.data.z)
                    .sum();
                let count = cluster_nodes.len() as f32;
                Some([sum_x / count, sum_y / count, sum_z / count])
            } else {
                None
            };

            Cluster {
                id: format!("cluster_{}", i),
                label: labels.get(i as usize).unwrap_or(&"Cluster").to_string(),
                node_count: cluster_nodes.len() as u32,
                coherence: 0.75 + (i as f32 * 0.03),
                color: colors.get(i as usize).unwrap_or(&"#888888").to_string(),
                keywords: vec![
                    format!("{}_keyword1", method),
                    format!("{}_keyword2", method),
                ],
                nodes: cluster_nodes,
                centroid,
            }
        })
        .collect()
}
