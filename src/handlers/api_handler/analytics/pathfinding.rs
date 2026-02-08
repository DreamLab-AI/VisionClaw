// Pathfinding API endpoints for SSSP, APSP, and connected components
//!
//! Provides REST API access to GPU-accelerated shortest path algorithms:
//! - Single-Source Shortest Path (SSSP)
//! - All-Pairs Shortest Path (APSP) approximation
//! - Connected Components analysis

use actix_web::{web, HttpResponse, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};

use crate::actors::gpu::shortest_path_actor::{
    ComputeSSP, ComputeAPSP, GetShortestPathStats, SSSPResult, APSPResult, ShortestPathStats
};
use crate::actors::gpu::connected_components_actor::{
    ComputeConnectedComponents, GetConnectedComponentsStats,
    ConnectedComponentsResult, ConnectedComponentsStats
};
use crate::{ok_json, error_json, AppState};

/// SSSP request payload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SSSPRequest {
    /// Source node index
    pub source_idx: usize,
    /// Optional maximum distance cutoff
    pub max_distance: Option<f32>,
}

/// APSP request payload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct APSPRequest {
    /// Number of landmark nodes for approximation (default: sqrt(n))
    pub num_landmarks: Option<usize>,
    /// Random seed for landmark selection
    pub seed: Option<u64>,
}

/// Connected components request payload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedComponentsRequest {
    /// Maximum iterations (default: 100)
    pub max_iterations: Option<u32>,
    /// Convergence threshold (default: 0.001)
    pub convergence_threshold: Option<f32>,
}

/// SSSP API response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SSSPResponse {
    pub success: bool,
    pub result: Option<SSSPResult>,
    pub error: Option<String>,
}

/// APSP API response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct APSPResponse {
    pub success: bool,
    pub result: Option<APSPResult>,
    pub error: Option<String>,
}

/// Connected components API response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedComponentsResponse {
    pub success: bool,
    pub result: Option<ConnectedComponentsResult>,
    pub error: Option<String>,
}

// Display implementations for response types (required by error macros)
impl std::fmt::Display for SSSPResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.success {
            write!(f, "SSSP computation successful")
        } else {
            write!(f, "SSSP computation failed: {:?}", self.error)
        }
    }
}

impl std::fmt::Display for APSPResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.success {
            write!(f, "APSP computation successful")
        } else {
            write!(f, "APSP computation failed: {:?}", self.error)
        }
    }
}

impl std::fmt::Display for ConnectedComponentsResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.success {
            write!(f, "Connected components computation successful")
        } else {
            write!(f, "Connected components computation failed: {:?}", self.error)
        }
    }
}

/// Compute single-source shortest paths from a given node
/// # Use Cases
/// - Path highlighting in visualization
/// - Reachability analysis
/// - Proximity-based queries
/// - Distance-based filtering
/// # Example
/// ```json
/// POST /api/analytics/pathfinding/sssp
/// {
///   "sourceIdx": 0,
///   "maxDistance": 5.0
/// }
/// ```
pub async fn compute_sssp(
    data: web::Data<AppState>,
    payload: web::Json<SSSPRequest>,
) -> Result<HttpResponse> {
    info!("API: Computing SSSP from node {}", payload.source_idx);

    if let Some(ref shortest_path_actor) = data.shortest_path_actor {
            let msg = ComputeSSP {
                source_idx: payload.source_idx,
                max_distance: payload.max_distance,
            };

            match shortest_path_actor.send(msg).await {
                Ok(Ok(result)) => {
                    info!("SSSP computed successfully: {} nodes reached", result.nodes_reached);
                    ok_json!(SSSPResponse {
                        success: true,
                        result: Some(result),
                        error: None,
                    })
                }
                Ok(Err(e)) => {
                    error!("SSSP computation failed: {}", e);
                    error_json!(SSSPResponse {
                        success: false,
                        result: None,
                        error: Some(e),
                    })
                }
                Err(e) => {
                    error!("Failed to send message to shortest path actor: {}", e);
                    error_json!(SSSPResponse {
                        success: false,
                        result: None,
                        error: Some(format!("Actor communication error: {}", e)),
                    })
                }
            }
        } else {
            error_json!(SSSPResponse {
                success: false,
                result: None,
                error: Some("Shortest path actor not available".to_string()),
            })
        }
}

/// Compute approximate all-pairs shortest paths using landmark-based method
/// # Use Cases
/// - Distance matrix visualization
/// - Graph layout with distance preservation
/// - Centrality analysis
/// - Similarity-based clustering
/// # Example
/// ```json
/// POST /api/analytics/pathfinding/apsp
/// {
///   "numLandmarks": 10,
///   "seed": 42
/// }
/// ```
pub async fn compute_apsp(
    data: web::Data<AppState>,
    payload: web::Json<APSPRequest>,
) -> Result<HttpResponse> {
    info!("API: Computing APSP");

    if let Some(ref shortest_path_actor) = data.shortest_path_actor {
            // Default to sqrt(n) landmarks if not specified
            let num_landmarks = payload.num_landmarks.unwrap_or(0);

            let msg = ComputeAPSP {
                num_landmarks,
                seed: payload.seed,
            };

            match shortest_path_actor.send(msg).await {
                Ok(Ok(result)) => {
                    info!("APSP computed successfully with {} landmarks", result.num_landmarks);
                    ok_json!(APSPResponse {
                        success: true,
                        result: Some(result),
                        error: None,
                    })
                }
                Ok(Err(e)) => {
                    error!("APSP computation failed: {}", e);
                    error_json!(APSPResponse {
                        success: false,
                        result: None,
                        error: Some(e),
                    })
                }
                Err(e) => {
                    error!("Failed to send message to shortest path actor: {}", e);
                    error_json!(APSPResponse {
                        success: false,
                        result: None,
                        error: Some(format!("Actor communication error: {}", e)),
                    })
                }
            }
        } else {
            error_json!(APSPResponse {
                success: false,
                result: None,
                error: Some("Shortest path actor not available".to_string()),
            })
        }
}

/// Compute connected components of the graph
/// # Use Cases
/// - Detecting disconnected graph regions
/// - Network fragmentation analysis
/// - Component-based visualization
/// - Graph partitioning
/// # Example
/// ```json
/// POST /api/analytics/pathfinding/connected-components
/// {
///   "maxIterations": 100
/// }
/// ```
pub async fn compute_connected_components(
    data: web::Data<AppState>,
    payload: web::Json<ConnectedComponentsRequest>,
) -> Result<HttpResponse> {
    info!("API: Computing connected components");

    if let Some(ref connected_components_actor) = data.connected_components_actor {
            let msg = ComputeConnectedComponents {
                max_iterations: payload.max_iterations,
                convergence_threshold: payload.convergence_threshold,
            };

            match connected_components_actor.send(msg).await {
                Ok(Ok(result)) => {
                    info!("Connected components computed: {} components found", result.num_components);
                    ok_json!(ConnectedComponentsResponse {
                        success: true,
                        result: Some(result),
                        error: None,
                    })
                }
                Ok(Err(e)) => {
                    error!("Connected components computation failed: {}", e);
                    error_json!(ConnectedComponentsResponse {
                        success: false,
                        result: None,
                        error: Some(e),
                    })
                }
                Err(e) => {
                    error!("Failed to send message to connected components actor: {}", e);
                    error_json!(ConnectedComponentsResponse {
                        success: false,
                        result: None,
                        error: Some(format!("Actor communication error: {}", e)),
                    })
                }
            }
        } else {
            error_json!(ConnectedComponentsResponse {
                success: false,
                result: None,
                error: Some("Connected components actor not available".to_string()),
            })
        }
}

/// Get shortest path statistics
pub async fn get_shortest_path_stats(
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    if let Some(ref shortest_path_actor) = data.shortest_path_actor {
            match shortest_path_actor.send(GetShortestPathStats).await {
                Ok(stats) => ok_json!(stats),
                Err(e) => {
                    error!("Failed to get shortest path stats: {}", e);
                    error_json!(format!("Failed to get stats: {}", e))
                }
            }
        } else {
            error_json!("Shortest path actor not available")
        }
}

/// Get connected components statistics
pub async fn get_connected_components_stats(
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    if let Some(ref connected_components_actor) = data.connected_components_actor {
            match connected_components_actor.send(GetConnectedComponentsStats).await {
                Ok(stats) => ok_json!(stats),
                Err(e) => {
                    error!("Failed to get connected components stats: {}", e);
                    error_json!(format!("Failed to get stats: {}", e))
                }
            }
        } else {
            error_json!("Connected components actor not available")
        }
}

/// Configure pathfinding API routes
pub fn configure_pathfinding_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/analytics/pathfinding")
            .route("/sssp", web::post().to(compute_sssp))
            .route("/apsp", web::post().to(compute_apsp))
            .route("/connected-components", web::post().to(compute_connected_components))
            .route("/stats/sssp", web::get().to(get_shortest_path_stats))
            .route("/stats/components", web::get().to(get_connected_components_stats))
    );
}
