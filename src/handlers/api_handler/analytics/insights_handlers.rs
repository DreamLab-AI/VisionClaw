use actix_web::{web, HttpResponse, Result};
use log::{error, info};
use uuid::Uuid;

use crate::ok_json;
use crate::AppState;
use visionclaw_domain::models::graph::GraphData;

use super::state::{ANOMALY_STATE, CLUSTERING_TASKS};
use super::types::{GraphPattern, InsightsResponse};

/// The part of the graph and of the latest stored clusters an insights reply may describe for
/// one caller (ADR-060). `gate` is the caller's gate when something is hidden from them: then
/// the counts are the view's and the clusters lose hidden members (a hidden-only cluster
/// disappears), so a hidden node reads exactly as an absent one.
fn insight_scope(
    graph: Option<&std::sync::Arc<GraphData>>,
    clusters: Option<&[super::Cluster]>,
    gate: Option<&crate::handlers::graph_access::CallerGate>,
) -> (
    Option<std::sync::Arc<GraphData>>,
    Option<Vec<super::Cluster>>,
) {
    match gate {
        None => (graph.cloned(), clusters.map(<[super::Cluster]>::to_vec)),
        Some(gate) => (
            Some(std::sync::Arc::clone(gate.view())),
            clusters.map(|c| super::visible_analytics::gate_clusters(c, gate)),
        ),
    }
}

pub async fn get_ai_insights(
    app_state: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
) -> Result<HttpResponse> {
    info!("Generating AI insights for graph analysis");

    let graph = crate::handlers::graph_access::current_graph(&app_state.graph_service_addr)
        .await
        .ok();
    // ADR-060: no graph means no visibility decision, so nothing graph-derived is said.
    let gate = graph.as_ref().and_then(|g| {
        crate::handlers::graph_access::gate_for_graph(
            g,
            crate::handlers::graph_access::caller_pubkey(&caller),
        )
    });

    let clustering_tasks = CLUSTERING_TASKS.lock().await;
    let anomaly_state = ANOMALY_STATE.lock().await;
    let latest = clustering_tasks
        .values()
        .filter(|t| t.status == "completed")
        .max_by_key(|t| t.started_at)
        .and_then(|t| t.clusters.as_deref());
    let (graph_data, latest_clusters) = if graph.is_some() {
        insight_scope(graph.as_ref(), latest, gate.as_ref())
    } else {
        (None, None)
    };

    let mut insights = vec![
        "Graph structure analysis shows balanced connectivity patterns".to_string(),
        "Node distribution follows expected semantic clustering".to_string(),
    ];

    let mut patterns = vec![];
    let mut recommendations = vec![];

    if let Some(latest_clusters) = latest_clusters.as_ref() {
        insights.push(format!(
            "Identified {} distinct semantic clusters",
            latest_clusters.len()
        ));

        if latest_clusters.len() > 10 {
            recommendations.push(
                "Consider increasing clustering threshold to reduce cluster count".to_string(),
            );
        } else if latest_clusters.len() < 3 {
            recommendations.push(
                "Consider decreasing clustering threshold for more granular grouping".to_string(),
            );
        }

        if let Some(largest_cluster) = latest_clusters.iter().max_by_key(|c| c.node_count) {
            patterns.push(GraphPattern {
                id: Uuid::new_v4().to_string(),
                r#type: "dominant_cluster".to_string(),
                description: format!(
                    "Large semantic cluster '{}' with {} nodes",
                    largest_cluster.label, largest_cluster.node_count
                ),
                confidence: largest_cluster.coherence,
                nodes: largest_cluster.nodes.clone(),
                significance: if largest_cluster.node_count > 50 {
                    "high"
                } else {
                    "medium"
                }
                .to_string(),
            });
        }
    }

    if anomaly_state.enabled && anomaly_state.stats.total > 0 {
        insights.push(format!(
            "Detected {} anomalies across the graph",
            anomaly_state.stats.total
        ));

        if anomaly_state.stats.critical > 0 {
            recommendations.push(
                "Investigate critical anomalies that may indicate data quality issues".to_string(),
            );
        }

        patterns.push(GraphPattern {
            id: Uuid::new_v4().to_string(),
            r#type: "anomaly_pattern".to_string(),
            description: format!(
                "Anomaly distribution: {} critical, {} high, {} medium",
                anomaly_state.stats.critical, anomaly_state.stats.high, anomaly_state.stats.medium
            ),
            confidence: 0.9,
            nodes: anomaly_state
                .anomalies
                .iter()
                .take(10)
                .filter_map(|a| a.node_id.parse::<u32>().ok())
                .collect(),
            significance: "high".to_string(),
        });
    }

    if let Some(data) = graph_data {
        let node_count = data.nodes.len();
        let edge_count = data.edges.len();
        let density = if node_count > 1 {
            (2.0 * edge_count as f32) / (node_count as f32 * (node_count - 1) as f32)
        } else {
            0.0
        };

        insights.push(format!(
            "Graph contains {} nodes and {} edges with density {:.3}",
            node_count, edge_count, density
        ));

        if density > 0.5 {
            recommendations
                .push("High graph density may benefit from hierarchical layout".to_string());
        } else if density < 0.1 {
            recommendations
                .push("Low graph density suggests potential for force-directed layout".to_string());
        }
    }

    ok_json!(InsightsResponse {
        success: true,
        insights: Some(insights),
        patterns: Some(patterns),
        recommendations: Some(recommendations),
        analysis_timestamp: Some(chrono::Utc::now().timestamp() as u64),
        error: None,
    })
}

pub async fn get_realtime_insights(
    app_state: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
) -> Result<HttpResponse> {
    info!("Client requesting real-time AI insights");

    let graph = crate::handlers::graph_access::current_graph(&app_state.graph_service_addr)
        .await
        .map_err(|e| {
            error!("Failed to get graph data: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to get graph data")
        })?;
    // ADR-060: the density is the caller's view's.
    let gate = crate::handlers::graph_access::gate_for_graph(
        &graph,
        crate::handlers::graph_access::caller_pubkey(&caller),
    );
    let (graph_data, _) = insight_scope(Some(&graph), None, gate.as_ref());
    let graph_data = graph_data.unwrap_or(graph);

    let clustering_tasks = CLUSTERING_TASKS.lock().await;
    let anomaly_state = ANOMALY_STATE.lock().await;

    let mut insights = vec![];
    let mut urgency_level = "low";

    if !graph_data.nodes.is_empty() {
        let density = (2.0 * graph_data.edges.len() as f32)
            / (graph_data.nodes.len() as f32 * (graph_data.nodes.len() - 1) as f32);

        insights.push(format!(
            "Graph density: {:.3} - {}",
            density,
            if density > 0.5 {
                "highly connected"
            } else if density > 0.2 {
                "moderately connected"
            } else {
                "sparsely connected"
            }
        ));
    }

    if let Some(running_task) = clustering_tasks.values().find(|t| t.status == "running") {
        insights.push(format!(
            "Clustering in progress: {} method at {:.1}% completion",
            running_task.method,
            running_task.progress * 100.0
        ));
        urgency_level = "medium";
    }

    if anomaly_state.enabled {
        if anomaly_state.stats.critical > 0 {
            insights.push(format!(
                "CRITICAL: {} critical anomalies detected!",
                anomaly_state.stats.critical
            ));
            urgency_level = "critical";
        } else if anomaly_state.stats.high > 0 {
            insights.push(format!(
                "High priority: {} high-severity anomalies detected",
                anomaly_state.stats.high
            ));
            if urgency_level == "low" {
                urgency_level = "high";
            }
        }
    }

    if let Some(gpu_addr) = app_state.get_gpu_compute_addr().await {
        if let Ok(Ok(stats)) = gpu_addr
            .send(crate::actors::messages::GetPhysicsStats)
            .await
        {
            if stats.gpu_failure_count > 0 {
                insights.push(format!(
                    "Performance warning: {} GPU failures detected",
                    stats.gpu_failure_count
                ));
                if urgency_level == "low" {
                    urgency_level = "medium";
                }
            }
        }
    }

    ok_json!(serde_json::json!({
        "success": true,
        "insights": insights,
        "urgency_level": urgency_level,
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "requires_action": urgency_level != "low",
        "next_update_ms": 5000
    }))
}

pub async fn get_dashboard_status(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    info!("Control center requesting dashboard status");

    // GPU compute address is now Arc<RwLock<Option<...>>> - use async accessor
    let gpu_available = app_state.get_gpu_compute_addr().await.is_some();
    let clustering_tasks = CLUSTERING_TASKS.lock().await;
    let anomaly_state = ANOMALY_STATE.lock().await;

    let active_clustering = clustering_tasks
        .values()
        .filter(|t| t.status == "running")
        .count();

    let completed_clustering = clustering_tasks
        .values()
        .filter(|t| t.status == "completed")
        .count();

    let mut health_status = "healthy";
    let mut issues = vec![];

    if !gpu_available {
        issues.push("GPU acceleration not available - using CPU fallback".to_string());
        health_status = "degraded";
    }

    if anomaly_state.stats.critical > 0 {
        issues.push(format!(
            "{} critical anomalies require attention",
            anomaly_state.stats.critical
        ));
        health_status = "warning";
    }

    ok_json!(serde_json::json!({
        "success": true,
        "system": {
            "status": health_status,
            "gpu_available": gpu_available,
            "uptime_ms": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            "issues": issues
        },
        "analytics": {
            "clustering": {
                "active_tasks": active_clustering,
                "completed_tasks": completed_clustering,
                "total_tasks": clustering_tasks.len()
            },
            "anomaly_detection": {
                "enabled": anomaly_state.enabled,
                "total_anomalies": anomaly_state.stats.total,
                "critical": anomaly_state.stats.critical,
                "high": anomaly_state.stats.high,
                "medium": anomaly_state.stats.medium,
                "low": anomaly_state.stats.low
            }
        },
        "last_updated": chrono::Utc::now().timestamp_millis()
    }))
}

pub async fn get_health_check(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    // GPU compute address is now Arc<RwLock<Option<...>>> - use async accessor
    let gpu_available = app_state.get_gpu_compute_addr().await.is_some();
    let timestamp = chrono::Utc::now().timestamp_millis();

    let status = if gpu_available { "healthy" } else { "degraded" };

    ok_json!(serde_json::json!({
        "status": status,
        "gpu_available": gpu_available,
        "timestamp": timestamp,
        "service": "analytics"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::api_handler::analytics::Cluster;
    use crate::handlers::graph_access::gate_for_graph_with;
    use std::sync::Arc;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::node::Node;

    /// Public 0 and 1 joined by an edge; when `with_hidden`, a hidden 2 linked to both.
    fn graph(with_hidden: bool) -> Arc<GraphData> {
        let node = |id: u32, public: bool| {
            let mut n = Node::new_with_id(format!("n{id}"), None);
            n.id = id;
            if public {
                n.metadata.insert("public".into(), "true".into());
            }
            n
        };
        let mut g = GraphData {
            nodes: vec![node(0, true), node(1, true)],
            edges: vec![Edge::new(0, 1, 1.0)],
            ..GraphData::default()
        };
        if with_hidden {
            g.nodes.push(node(2, false));
            g.edges.push(Edge::new(0, 2, 1.0));
            g.edges.push(Edge::new(1, 2, 1.0));
        }
        Arc::new(g)
    }

    fn cluster(id: &str, nodes: Vec<u32>) -> Cluster {
        Cluster {
            id: id.into(),
            label: format!("Cluster {id}"),
            node_count: nodes.len() as u32,
            coherence: 0.5,
            color: "#000000".into(),
            keywords: vec![],
            centroid: None,
            nodes,
        }
    }

    fn scope(with_hidden: bool) -> String {
        let g = graph(with_hidden);
        let clusters = if with_hidden {
            vec![cluster("a", vec![0, 1]), cluster("b", vec![2])]
        } else {
            vec![cluster("a", vec![0, 1])]
        };
        let gate = gate_for_graph_with(&g, None, true);
        let (view, clusters) = insight_scope(Some(&g), Some(&clusters), gate.as_ref());
        let view = view.unwrap();
        format!(
            "{} nodes, {} edges, clusters {}",
            view.nodes.len(),
            view.edges.len(),
            serde_json::to_string(&clusters).unwrap()
        )
    }

    #[test]
    fn a_hidden_node_reads_in_insights_exactly_as_an_absent_one() {
        assert_eq!(scope(true), scope(false));
        assert!(
            !scope(true).contains("\"b\""),
            "a hidden-only cluster is not described"
        );
    }
}
