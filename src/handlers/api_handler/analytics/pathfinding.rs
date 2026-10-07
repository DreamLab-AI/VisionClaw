// Pathfinding API endpoints for SSSP, APSP, and connected components
//!
//! Provides REST API access to GPU-accelerated shortest path algorithms:
//! - Single-Source Shortest Path (SSSP)
//! - All-Pairs Shortest Path (APSP) approximation
//! - Connected Components analysis

use actix_web::{web, HttpResponse, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};

use crate::actors::gpu::connected_components_actor::{
    ComputeConnectedComponents, ConnectedComponentsResult,
};
use crate::actors::gpu::graph_analytics_supervisor::{
    GetSupervisedComponentsStats, GetSupervisedShortestPathStats,
};
use crate::actors::gpu::shortest_path_actor::{APSPResult, ComputeAPSP, ComputeSSP, SSSPResult};
use crate::ports::graph_repository::GraphRepository;
use crate::services::pathfinding::{
    AStarPathfinder, BidirectionalDijkstra, PathAlgorithm, PathResult as PfPathResult,
    SemanticPathfinder,
};
use crate::{error_json, ok_json, AppState};

/// SSSP request payload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SSSPRequest {
    /// Source node index
    pub source_idx: usize,
    /// Optional maximum distance cutoff
    pub max_distance: Option<f32>,
    /// Optional delta-stepping bucket width for SSSP.
    /// When provided, uses delta-stepping instead of Bellman-Ford.
    pub delta: Option<f32>,
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
            write!(
                f,
                "Connected components computation failed: {:?}",
                self.error
            )
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

    // ADR-2053: routed through GPUManagerActor -> GraphAnalyticsSupervisor -> the
    // SUPERVISED ShortestPathActor. The standalone actor this used to address was
    // never sent a SharedGPUContext, so it always took the GPU-absent arm.
    if let Some(ref gpu_manager) = data.gpu_manager_addr {
        let msg = ComputeSSP {
            source_idx: payload.source_idx,
            max_distance: payload.max_distance,
            delta: payload.delta,
        };

        match gpu_manager.send(msg).await {
            Ok(Ok(result)) => {
                info!(
                    "SSSP computed successfully: {} nodes reached",
                    result.nodes_reached
                );
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

    // ADR-2053: supervised path, see compute_sssp above.
    if let Some(ref gpu_manager) = data.gpu_manager_addr {
        // Default to sqrt(n) landmarks if not specified
        let num_landmarks = payload.num_landmarks.unwrap_or(0);

        let msg = ComputeAPSP {
            num_landmarks,
            seed: payload.seed,
        };

        match gpu_manager.send(msg).await {
            Ok(Ok(result)) => {
                info!(
                    "APSP computed successfully with {} landmarks",
                    result.num_landmarks
                );
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

    // ADR-2053: supervised path, see compute_sssp above.
    if let Some(ref gpu_manager) = data.gpu_manager_addr {
        let msg = ComputeConnectedComponents {
            max_iterations: payload.max_iterations,
            convergence_threshold: payload.convergence_threshold,
        };

        match gpu_manager.send(msg).await {
            Ok(Ok(result)) => {
                info!(
                    "Connected components computed: {} components found",
                    result.num_components
                );
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
                error!(
                    "Failed to send message to connected components actor: {}",
                    e
                );
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
pub async fn get_shortest_path_stats(data: web::Data<AppState>) -> Result<HttpResponse> {
    // ADR-2053: the supervised wrapper returns Result, so "no actor" stays
    // distinguishable from "genuinely all zeroes" — neither stats struct derives
    // Default, which is exactly why a bare forward would have had to invent one.
    if let Some(ref gpu_manager) = data.gpu_manager_addr {
        match gpu_manager.send(GetSupervisedShortestPathStats).await {
            Ok(Ok(stats)) => ok_json!(stats),
            Ok(Err(e)) => {
                error!("Shortest path stats unavailable: {}", e);
                error_json!(e)
            }
            Err(e) => {
                error!("Failed to get shortest path stats: {}", e);
                error_json!(format!("Failed to get stats: {}", e))
            }
        }
    } else {
        error_json!("GPU manager not available")
    }
}

/// Get connected components statistics
pub async fn get_connected_components_stats(data: web::Data<AppState>) -> Result<HttpResponse> {
    // ADR-2053: supervised wrapper, see get_shortest_path_stats above.
    if let Some(ref gpu_manager) = data.gpu_manager_addr {
        match gpu_manager.send(GetSupervisedComponentsStats).await {
            Ok(Ok(stats)) => ok_json!(stats),
            Ok(Err(e)) => {
                error!("Connected components stats unavailable: {}", e);
                error_json!(e)
            }
            Err(e) => {
                error!("Failed to get connected components stats: {}", e);
                error_json!(format!("Failed to get stats: {}", e))
            }
        }
    } else {
        error_json!("Connected components actor not available")
    }
}

/// Point-to-point pathfinding request
///
/// Dispatches to A*, Bidirectional Dijkstra, or Semantic SSSP based on `algorithm`.
/// # Example
/// ```json
/// POST /api/analytics/pathfinding/path
/// {
///   "sourceId": 1,
///   "targetId": 5,
///   "algorithm": "astar"
/// }
/// ```
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointToPointRequest {
    /// Source node ID
    pub source_id: u32,
    /// Target node ID
    pub target_id: u32,
    /// Algorithm: "astar" | "bidirectional" | "semantic" (default: "astar")
    #[serde(default)]
    pub algorithm: PathAlgorithm,
    /// Query string for semantic pathfinding (required when algorithm = "semantic")
    pub query: Option<String>,
    /// Semantic alpha parameter (0.0-2.0, default 0.5). Only used with "semantic".
    pub semantic_alpha: Option<f32>,
}

/// Point-to-point response wrapper
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointToPointResponse {
    pub success: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl std::fmt::Display for PointToPointResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.success {
            write!(f, "Point-to-point pathfinding successful")
        } else {
            write!(f, "Point-to-point pathfinding failed: {:?}", self.error)
        }
    }
}

/// Compute point-to-point shortest path using A*, Bidirectional Dijkstra, or Semantic search
pub async fn compute_point_to_point(
    data: web::Data<AppState>,
    payload: web::Json<PointToPointRequest>,
) -> Result<HttpResponse> {
    info!(
        "API: Computing point-to-point path from {} to {} using {:?}",
        payload.source_id, payload.target_id, payload.algorithm
    );

    // Retrieve graph data from the repository
    let graph_data = match data.graph_repository.get_graph().await {
        Ok(graph) => graph,
        Err(e) => {
            error!("Failed to get graph data: {:?}", e);
            return error_json!(PointToPointResponse {
                success: false,
                result: None,
                error: Some(format!("Failed to retrieve graph data: {:?}", e)),
            });
        }
    };

    match payload.algorithm {
        PathAlgorithm::Astar => {
            match AStarPathfinder::find_path(&graph_data, payload.source_id, payload.target_id) {
                Ok(result) => {
                    info!(
                        "A* path found: {} nodes, distance {:.3}, visited {}",
                        result.path.len(),
                        result.distance,
                        result.nodes_visited
                    );
                    let json_val = serde_json::to_value(&result).unwrap_or_default();
                    ok_json!(PointToPointResponse {
                        success: true,
                        result: Some(json_val),
                        error: None,
                    })
                }
                Err(e) => {
                    error!("A* pathfinding failed: {}", e);
                    error_json!(PointToPointResponse {
                        success: false,
                        result: None,
                        error: Some(e),
                    })
                }
            }
        }
        PathAlgorithm::Bidirectional => {
            match BidirectionalDijkstra::find_path(
                &graph_data,
                payload.source_id,
                payload.target_id,
            ) {
                Ok(result) => {
                    info!(
                        "Bidirectional path found: {} nodes, distance {:.3}, visited {}",
                        result.path.len(),
                        result.distance,
                        result.nodes_visited
                    );
                    let json_val = serde_json::to_value(&result).unwrap_or_default();
                    ok_json!(PointToPointResponse {
                        success: true,
                        result: Some(json_val),
                        error: None,
                    })
                }
                Err(e) => {
                    error!("Bidirectional Dijkstra failed: {}", e);
                    error_json!(PointToPointResponse {
                        success: false,
                        result: None,
                        error: Some(e),
                    })
                }
            }
        }
        PathAlgorithm::Semantic => {
            let query = match &payload.query {
                Some(q) if !q.is_empty() => q.clone(),
                _ => {
                    return error_json!(PointToPointResponse {
                        success: false,
                        result: None,
                        error: Some(
                            "Query string is required for semantic pathfinding".to_string()
                        ),
                    });
                }
            };

            let alpha = payload.semantic_alpha.unwrap_or(0.5);
            let pathfinder = SemanticPathfinder::new().with_alpha(alpha);

            match pathfinder.find_path(&graph_data, payload.source_id, payload.target_id, &query) {
                Ok(result) => {
                    info!(
                        "Semantic path found: {} nodes, distance {:.3}, relevance {:.3}",
                        result.path_result.path.len(),
                        result.path_result.distance,
                        result.relevance
                    );
                    let json_val = serde_json::to_value(&result).unwrap_or_default();
                    ok_json!(PointToPointResponse {
                        success: true,
                        result: Some(json_val),
                        error: None,
                    })
                }
                Err(e) => {
                    error!("Semantic pathfinding failed: {}", e);
                    error_json!(PointToPointResponse {
                        success: false,
                        result: None,
                        error: Some(e),
                    })
                }
            }
        }
        PathAlgorithm::Sssp => {
            // Fall through to the GPU SSSP actor (requires index, not ID).
            // For point-to-point via SSSP, find the node index first.
            // ADR-2053: supervised path, see compute_sssp above.
            if let Some(ref gpu_manager) = data.gpu_manager_addr {
                // Find source index in graph
                let source_idx = match graph_data
                    .nodes
                    .iter()
                    .position(|n| n.id == payload.source_id)
                {
                    Some(idx) => idx,
                    None => {
                        return error_json!(PointToPointResponse {
                            success: false,
                            result: None,
                            error: Some(format!(
                                "Source node {} not found in graph",
                                payload.source_id
                            )),
                        });
                    }
                };

                let msg = ComputeSSP {
                    source_idx,
                    max_distance: None,
                    delta: None,
                };

                match gpu_manager.send(msg).await {
                    Ok(Ok(sssp_result)) => {
                        // Extract distance to target
                        let target_idx = graph_data
                            .nodes
                            .iter()
                            .position(|n| n.id == payload.target_id);
                        let (distance, exists) = match target_idx {
                            Some(idx) if sssp_result.distances[idx] < f32::MAX => {
                                (sssp_result.distances[idx], true)
                            }
                            _ => (f32::MAX, false),
                        };

                        let result = PfPathResult {
                            path: Vec::new(), // SSSP does not track predecessors
                            distance,
                            exists,
                            nodes_visited: sssp_result.nodes_reached,
                            algorithm: "sssp".into(),
                        };
                        let json_val = serde_json::to_value(&result).unwrap_or_default();
                        ok_json!(PointToPointResponse {
                            success: true,
                            result: Some(json_val),
                            error: None,
                        })
                    }
                    Ok(Err(e)) => {
                        error!("GPU SSSP failed: {}", e);
                        error_json!(PointToPointResponse {
                            success: false,
                            result: None,
                            error: Some(e),
                        })
                    }
                    Err(e) => {
                        error!("SSSP actor communication error: {}", e);
                        error_json!(PointToPointResponse {
                            success: false,
                            result: None,
                            error: Some(format!("Actor communication error: {}", e)),
                        })
                    }
                }
            } else {
                error_json!(PointToPointResponse {
                    success: false,
                    result: None,
                    error: Some("Shortest path actor not available".to_string()),
                })
            }
        }
    }
}

// NOTE (task #70 D8b HTTP-route consolidation): the duplicate
// `configure_pathfinding_routes` scope previously registered a second
// `/analytics/pathfinding/*` prefix. actix-web does NOT fall through
// duplicate-prefix scopes — the FIRST registered `web::scope("/analytics")`
// (in `analytics::config`) already claims the prefix, so every route in this
// scope returned 404 and the scope was dead. The six pathfinding routes are now
// registered exactly once, in `analytics::config` (this module's handlers are
// referenced there as `pathfinding::*`). This dead `configure_pathfinding_routes`
// has been removed; the unrouted `/path`, `/stats/sssp`, and `/stats/components`
// handlers it formerly masked are now live under the single `/analytics` scope.
