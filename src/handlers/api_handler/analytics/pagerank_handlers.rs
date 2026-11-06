//! PageRank HTTP API endpoints
//!
//! Provides REST API access to GPU-accelerated PageRank centrality computation:
//! - Compute PageRank scores for all nodes
//! - Retrieve cached PageRank results
//! - Clear the PageRank cache

use actix_web::{web, HttpResponse, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};

use crate::actors::gpu::pagerank_actor::{PageRankParams, PageRankResult};
use crate::actors::messages::analytics_messages::{
    ClearPageRankCache, ComputePageRank, GetPageRankResult,
};
use crate::{error_json, ok_json, AppState};

/// PageRank computation request payload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRankRequest {
    /// Damping factor (probability of following a link vs random jump).
    /// Typical value: 0.85
    pub damping_factor: Option<f32>,

    /// Maximum number of iterations (default: 100)
    pub max_iterations: Option<u32>,

    /// Convergence threshold - L1 norm of difference between iterations (default: 1e-6)
    pub epsilon: Option<f32>,

    /// Whether to normalize results so that sum = 1.0 (default: true)
    pub normalize: Option<bool>,

    /// Use optimized kernel with shared memory (default: true)
    pub use_optimized: Option<bool>,
}

/// PageRank API response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRankResponse {
    pub success: bool,
    pub result: Option<PageRankPayload>,
    pub error: Option<String>,
}

impl std::fmt::Display for PageRankResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.success {
            write!(f, "PageRank computation successful")
        } else {
            write!(f, "PageRank computation failed: {:?}", self.error)
        }
    }
}

/// PageRank cache status response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRankCacheResponse {
    pub success: bool,
    pub cached: bool,
    pub result: Option<PageRankPayload>,
    pub error: Option<String>,
}

impl std::fmt::Display for PageRankCacheResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.success {
            write!(f, "PageRank result retrieved (cached={})", self.cached)
        } else {
            write!(f, "PageRank result retrieval failed: {:?}", self.error)
        }
    }
}

/// PageRank cache clear response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRankClearResponse {
    pub success: bool,
    pub message: String,
    pub error: Option<String>,
}

impl std::fmt::Display for PageRankClearResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Top nodes listed in a gated reply: the actor's own k (`extract_top_nodes(.., 10)`).
const VISIBLE_TOP_K: usize = 10;

/// A PageRank result: the actor's whole-graph run, or the caller's view (ADR-060).
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PageRankPayload {
    /// Whole graph, indexed by position.
    Whole(PageRankResult),
    /// Computed over the caller's view, keyed by visible node id.
    Visible(super::visible_analytics::VisiblePageRankResult),
}

/// Compute PageRank centrality for all nodes in the graph
///
/// # Use Cases
/// - Identify the most important/influential nodes
/// - Size nodes proportionally to their centrality
/// - Color nodes using a gradient (low -> high centrality)
/// - Filter/highlight influential nodes
///
/// # Example
/// ```json
/// POST /api/analytics/pagerank/compute
/// {
///   "dampingFactor": 0.85,
///   "maxIterations": 100,
///   "epsilon": 1e-6,
///   "normalize": true,
///   "useOptimized": true
/// }
/// ```
pub async fn compute_pagerank(
    data: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
    payload: web::Json<PageRankRequest>,
) -> Result<HttpResponse> {
    info!(
        "API: Computing PageRank (damping={:?}, maxIter={:?}, eps={:?})",
        payload.damping_factor, payload.max_iterations, payload.epsilon
    );

    // ADR-060: a caller whose view hides nodes is answered over that view on the CPU, keyed by
    // visible node id; the shared actor (and its cache) only ever serves whole-graph callers.
    match crate::handlers::graph_access::caller_gate(
        &data.graph_service_addr,
        crate::handlers::graph_access::caller_pubkey(&caller),
    )
    .await
    {
        Ok(Some(gate)) => {
            return ok_json!(PageRankResponse {
                success: true,
                result: Some(PageRankPayload::Visible(
                    super::visible_analytics::visible_pagerank(
                        &gate,
                        payload.damping_factor.unwrap_or(0.85),
                        payload.max_iterations.unwrap_or(100).clamp(1, 1000),
                        payload.epsilon.unwrap_or(1e-6),
                        VISIBLE_TOP_K,
                    )
                )),
                error: None,
            });
        }
        Ok(None) => {}
        Err(e) => {
            return error_json!(PageRankResponse {
                success: false,
                result: None,
                error: Some(e),
            })
        }
    }

    if let Some(ref pagerank_actor) = data.analytics.pagerank {
        let params = PageRankParams {
            damping_factor: payload.damping_factor,
            max_iterations: payload.max_iterations,
            epsilon: payload.epsilon,
            normalize: payload.normalize,
            use_optimized: payload.use_optimized,
        };

        let msg = ComputePageRank {
            params: Some(params),
        };

        match pagerank_actor.send(msg).await {
            Ok(Ok(result)) => {
                info!(
                    "PageRank computed successfully: {} nodes, converged={}, iterations={}",
                    result.stats.total_nodes, result.converged, result.iterations
                );
                ok_json!(PageRankResponse {
                    success: true,
                    result: Some(PageRankPayload::Whole(result)),
                    error: None,
                })
            }
            Ok(Err(e)) => {
                error!("PageRank computation failed: {}", e);
                error_json!(PageRankResponse {
                    success: false,
                    result: None,
                    error: Some(e),
                })
            }
            Err(e) => {
                error!("Failed to send message to PageRank actor: {}", e);
                error_json!(PageRankResponse {
                    success: false,
                    result: None,
                    error: Some(format!("Actor communication error: {}", e)),
                })
            }
        }
    } else {
        error_json!(PageRankResponse {
            success: false,
            result: None,
            error: Some("PageRank actor not available".to_string()),
        })
    }
}

/// Retrieve the most recently cached PageRank result
///
/// Returns the last computed PageRank result without re-running the algorithm.
/// Returns `cached: false` if no result is available yet.
///
/// # Example
/// ```json
/// GET /api/analytics/pagerank/result
/// ```
pub async fn get_pagerank_result(
    data: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
) -> Result<HttpResponse> {
    info!("API: Getting cached PageRank result");

    // ADR-060: the cache holds a whole-graph run. A caller whose view hides nodes has no cached
    // result of their own (POST /compute answers them over their view), so they get exactly the
    // "nothing cached" reply; an unreadable graph fails closed.
    match crate::handlers::graph_access::caller_gate(
        &data.graph_service_addr,
        crate::handlers::graph_access::caller_pubkey(&caller),
    )
    .await
    {
        Ok(None) => {}
        Ok(Some(_)) => {
            return ok_json!(PageRankCacheResponse {
                success: true,
                cached: false,
                result: None,
                error: None,
            })
        }
        Err(e) => {
            return error_json!(PageRankCacheResponse {
                success: false,
                cached: false,
                result: None,
                error: Some(e),
            })
        }
    }

    if let Some(ref pagerank_actor) = data.analytics.pagerank {
        match pagerank_actor.send(GetPageRankResult).await {
            Ok(Some(result)) => {
                info!(
                    "Returning cached PageRank result: {} nodes",
                    result.stats.total_nodes
                );
                ok_json!(PageRankCacheResponse {
                    success: true,
                    cached: true,
                    result: Some(PageRankPayload::Whole(result)),
                    error: None,
                })
            }
            Ok(None) => {
                info!("No cached PageRank result available");
                ok_json!(PageRankCacheResponse {
                    success: true,
                    cached: false,
                    result: None,
                    error: None,
                })
            }
            Err(e) => {
                error!("Failed to send message to PageRank actor: {}", e);
                error_json!(PageRankCacheResponse {
                    success: false,
                    cached: false,
                    result: None,
                    error: Some(format!("Actor communication error: {}", e)),
                })
            }
        }
    } else {
        error_json!(PageRankCacheResponse {
            success: false,
            cached: false,
            result: None,
            error: Some("PageRank actor not available".to_string()),
        })
    }
}

/// Clear the cached PageRank result
///
/// Evicts any previously computed PageRank data from the actor's cache.
/// The next call to `/compute` will run the algorithm from scratch.
///
/// # Example
/// ```json
/// POST /api/analytics/pagerank/clear
/// ```
pub async fn clear_pagerank_cache(data: web::Data<AppState>) -> Result<HttpResponse> {
    info!("API: Clearing PageRank cache");

    if let Some(ref pagerank_actor) = data.analytics.pagerank {
        match pagerank_actor.send(ClearPageRankCache).await {
            Ok(()) => {
                info!("PageRank cache cleared successfully");
                ok_json!(PageRankClearResponse {
                    success: true,
                    message: "PageRank cache cleared".to_string(),
                    error: None,
                })
            }
            Err(e) => {
                error!("Failed to send message to PageRank actor: {}", e);
                error_json!(PageRankClearResponse {
                    success: false,
                    message: "Failed to clear PageRank cache".to_string(),
                    error: Some(format!("Actor communication error: {}", e)),
                })
            }
        }
    } else {
        error_json!(PageRankClearResponse {
            success: false,
            message: "PageRank actor not available".to_string(),
            error: Some("PageRank actor not available".to_string()),
        })
    }
}
