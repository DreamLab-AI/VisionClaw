// src/application/graph/queries.rs
//! Graph Domain - Read Operations (Queries)
//!
//! All queries for reading graph state following CQRS patterns.

use hexser::{HexResult, Hexserror, QueryHandler};
use std::collections::HashMap;
use std::sync::Arc;

use crate::actors::graph_actor::{AutoBalanceNotification, PhysicsState};
use crate::ports::graph_repository::{GraphRepository, PathfindingParams, PathfindingResult};
use visionclaw_domain::models::constraints::ConstraintSet;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;

// ============================================================================
// GET GRAPH DATA
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetGraphData;

pub struct GetGraphDataHandler {
    repository: Arc<dyn GraphRepository>,
}

impl GetGraphDataHandler {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetGraphData, Arc<GraphData>> for GetGraphDataHandler {
    fn handle(&self, _query: GetGraphData) -> HexResult<Arc<GraphData>> {
        log::debug!("Executing GetGraphData query");

        let repository = self.repository.clone();

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            Hexserror::adapter("E_GRAPH_001", &format!("Failed to create runtime: {}", e))
        })?;

        runtime.block_on(async move {
            repository.get_graph().await.map_err(|e| {
                Hexserror::adapter("E_GRAPH_001", &format!("Failed to get graph data: {}", e))
            })
        })
    }
}

// ============================================================================
// GET NODE MAP
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetNodeMap;

pub struct GetNodeMapHandler {
    repository: Arc<dyn GraphRepository>,
}

impl GetNodeMapHandler {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetNodeMap, Arc<HashMap<u32, Node>>> for GetNodeMapHandler {
    fn handle(&self, _query: GetNodeMap) -> HexResult<Arc<HashMap<u32, Node>>> {
        log::debug!("Executing GetNodeMap query");

        let repository = self.repository.clone();

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            Hexserror::adapter("E_GRAPH_002", &format!("Failed to create runtime: {}", e))
        })?;

        runtime.block_on(async move {
            repository.get_node_map().await.map_err(|e| {
                Hexserror::adapter("E_GRAPH_002", &format!("Failed to get node map: {}", e))
            })
        })
    }
}

// ============================================================================
// GET PHYSICS STATE
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetPhysicsState;

pub struct GetPhysicsStateHandler {
    repository: Arc<dyn GraphRepository>,
}

impl GetPhysicsStateHandler {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetPhysicsState, PhysicsState> for GetPhysicsStateHandler {
    fn handle(&self, _query: GetPhysicsState) -> HexResult<PhysicsState> {
        log::debug!("Executing GetPhysicsState query");

        let repository = self.repository.clone();

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            Hexserror::adapter("E_GRAPH_003", &format!("Failed to create runtime: {}", e))
        })?;

        runtime.block_on(async move {
            repository.get_physics_state().await.map_err(|e| {
                Hexserror::adapter(
                    "E_GRAPH_003",
                    &format!("Failed to get physics state: {}", e),
                )
            })
        })
    }
}

// ============================================================================
// GET AUTO-BALANCE NOTIFICATIONS
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetAutoBalanceNotifications {
    pub since_timestamp: Option<i64>,
}

pub struct GetAutoBalanceNotificationsHandler {
    repository: Arc<dyn GraphRepository>,
}

impl GetAutoBalanceNotificationsHandler {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetAutoBalanceNotifications, Vec<AutoBalanceNotification>>
    for GetAutoBalanceNotificationsHandler
{
    fn handle(
        &self,
        query: GetAutoBalanceNotifications,
    ) -> HexResult<Vec<AutoBalanceNotification>> {
        log::debug!(
            "Executing GetAutoBalanceNotifications query (since_timestamp: {:?})",
            query.since_timestamp
        );

        let repository = self.repository.clone();

        tokio::runtime::Handle::current().block_on(async move {
            repository
                .get_auto_balance_notifications()
                .await
                .map_err(|e| {
                    Hexserror::adapter(
                        "E_GRAPH_004",
                        &format!("Failed to get auto-balance notifications: {}", e),
                    )
                })
        })
    }
}

// ============================================================================
// GET CONSTRAINTS
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetConstraints;

pub struct GetConstraintsHandler {
    repository: Arc<dyn GraphRepository>,
}

impl GetConstraintsHandler {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetConstraints, ConstraintSet> for GetConstraintsHandler {
    fn handle(&self, _query: GetConstraints) -> HexResult<ConstraintSet> {
        log::debug!("Executing GetConstraints query");

        let repository = self.repository.clone();

        tokio::runtime::Handle::current().block_on(async move {
            repository.get_constraints().await.map_err(|e| {
                Hexserror::adapter("E_GRAPH_006", &format!("Failed to get constraints: {}", e))
            })
        })
    }
}

// ============================================================================
// GET EQUILIBRIUM STATUS
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetEquilibriumStatus;

pub struct GetEquilibriumStatusHandler {
    repository: Arc<dyn GraphRepository>,
}

impl GetEquilibriumStatusHandler {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetEquilibriumStatus, bool> for GetEquilibriumStatusHandler {
    fn handle(&self, _query: GetEquilibriumStatus) -> HexResult<bool> {
        log::debug!("Executing GetEquilibriumStatus query");

        let repository = self.repository.clone();

        tokio::runtime::Handle::current().block_on(async move {
            repository.get_equilibrium_status().await.map_err(|e| {
                Hexserror::adapter(
                    "E_GRAPH_007",
                    &format!("Failed to get equilibrium status: {}", e),
                )
            })
        })
    }
}

// ============================================================================
// COMPUTE SHORTEST PATHS
// ============================================================================

#[derive(Debug, Clone)]
pub struct ComputeShortestPaths {
    pub start_node: u32,
    pub end_node: u32,
    pub max_depth: Option<usize>,
}

pub struct ComputeShortestPathsHandler {
    repository: Arc<dyn GraphRepository>,
}

impl ComputeShortestPathsHandler {
    pub fn new(repository: Arc<dyn GraphRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ComputeShortestPaths, PathfindingResult> for ComputeShortestPathsHandler {
    fn handle(&self, query: ComputeShortestPaths) -> HexResult<PathfindingResult> {
        log::debug!(
            "Executing ComputeShortestPaths query: start={}, end={}, max_depth={:?}",
            query.start_node,
            query.end_node,
            query.max_depth
        );

        let repository = self.repository.clone();
        let params = PathfindingParams {
            start_node: query.start_node,
            end_node: query.end_node,
            max_depth: query.max_depth,
        };

        tokio::runtime::Handle::current().block_on(async move {
            repository
                .compute_shortest_paths(params)
                .await
                .map_err(|e| {
                    Hexserror::adapter(
                        "E_GRAPH_008",
                        &format!("Failed to compute shortest paths: {}", e),
                    )
                })
        })
    }
}
