// src/application/knowledge_graph/queries.rs
//! Knowledge Graph Domain - Read Operations (Queries)
//!
//! All queries for reading graph state following CQRS patterns.

use hexser::{HexResult, Hexserror, QueryHandler};
use serde::Serialize;
use std::sync::Arc;

use crate::ports::knowledge_graph_repository::{GraphStatistics, KnowledgeGraphRepository};
use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;

#[derive(Debug, Clone, Serialize)]
pub enum QueryResult {
    Graph(#[serde(serialize_with = "serialize_arc")] Arc<GraphData>),
    Node(Option<Box<Node>>),
    Nodes(Vec<Node>),
    Edges(Vec<Edge>),
    Statistics(GraphStatistics),
}

// Custom serializer for Arc<GraphData>
fn serialize_arc<S>(arc: &Arc<GraphData>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    arc.as_ref().serialize(serializer)
}

// ============================================================================
// LOAD GRAPH
// ============================================================================

#[derive(Debug, Clone)]
pub struct LoadGraph;

pub struct LoadGraphHandler<R: KnowledgeGraphRepository> {
    repository: Arc<R>,
}

impl<R: KnowledgeGraphRepository> LoadGraphHandler<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R: KnowledgeGraphRepository + Send + Sync + 'static> QueryHandler<LoadGraph, QueryResult>
    for LoadGraphHandler<R>
{
    fn handle(&self, _query: LoadGraph) -> HexResult<QueryResult> {
        let repository = self.repository.clone();
        tokio::runtime::Handle::current().block_on(async move {
            log::debug!("Executing LoadGraph query");

            let graph = repository.load_graph().await.map_err(|e| {
                Hexserror::adapter("E_KG_LOAD_GRAPH", &format!("Failed to load graph: {}", e))
            })?;

            Ok(QueryResult::Graph(graph))
        })
    }
}

// ============================================================================
// GET NODE
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetNode {
    pub node_id: u32,
}

pub struct GetNodeHandler<R: KnowledgeGraphRepository> {
    repository: Arc<R>,
}

impl<R: KnowledgeGraphRepository> GetNodeHandler<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R: KnowledgeGraphRepository + Send + Sync + 'static> QueryHandler<GetNode, QueryResult>
    for GetNodeHandler<R>
{
    fn handle(&self, query: GetNode) -> HexResult<QueryResult> {
        let repository = self.repository.clone();
        tokio::runtime::Handle::current().block_on(async move {
            log::debug!("Executing GetNode query: id={}", query.node_id);

            let node = repository.get_node(query.node_id).await.map_err(|e| {
                Hexserror::adapter("E_KG_GET_NODE", &format!("Failed to get node: {}", e))
            })?;

            Ok(QueryResult::Node(node.map(Box::new)))
        })
    }
}

// ============================================================================
// GET NODES BY METADATA ID
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetNodesByMetadataId {
    pub metadata_id: String,
}

pub struct GetNodesByMetadataIdHandler<R: KnowledgeGraphRepository> {
    repository: Arc<R>,
}

impl<R: KnowledgeGraphRepository> GetNodesByMetadataIdHandler<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R: KnowledgeGraphRepository + Send + Sync + 'static>
    QueryHandler<GetNodesByMetadataId, QueryResult> for GetNodesByMetadataIdHandler<R>
{
    fn handle(&self, query: GetNodesByMetadataId) -> HexResult<QueryResult> {
        let repository = self.repository.clone();
        tokio::runtime::Handle::current().block_on(async move {
            log::debug!(
                "Executing GetNodesByMetadataId query: metadata_id={}",
                query.metadata_id
            );

            let nodes = repository
                .get_nodes_by_metadata_id(&query.metadata_id)
                .await
                .map_err(|e| {
                    Hexserror::adapter(
                        "E_KG_GET_NODES_META",
                        &format!("Failed to get nodes by metadata ID: {}", e),
                    )
                })?;

            Ok(QueryResult::Nodes(nodes))
        })
    }
}

// ============================================================================
// GET NODE EDGES
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetNodeEdges {
    pub node_id: u32,
}

pub struct GetNodeEdgesHandler<R: KnowledgeGraphRepository> {
    repository: Arc<R>,
}

impl<R: KnowledgeGraphRepository> GetNodeEdgesHandler<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R: KnowledgeGraphRepository + Send + Sync + 'static> QueryHandler<GetNodeEdges, QueryResult>
    for GetNodeEdgesHandler<R>
{
    fn handle(&self, query: GetNodeEdges) -> HexResult<QueryResult> {
        let repository = self.repository.clone();
        tokio::runtime::Handle::current().block_on(async move {
            log::debug!("Executing GetNodeEdges query: node_id={}", query.node_id);

            let edges = repository
                .get_node_edges(query.node_id)
                .await
                .map_err(|e| {
                    Hexserror::adapter(
                        "E_KG_GET_EDGES",
                        &format!("Failed to get node edges: {}", e),
                    )
                })?;

            Ok(QueryResult::Edges(edges))
        })
    }
}

// ============================================================================
// QUERY NODES
// ============================================================================

#[derive(Debug, Clone)]
pub struct QueryNodes {
    pub query_string: String,
}

pub struct QueryNodesHandler<R: KnowledgeGraphRepository> {
    repository: Arc<R>,
}

impl<R: KnowledgeGraphRepository> QueryNodesHandler<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R: KnowledgeGraphRepository + Send + Sync + 'static> QueryHandler<QueryNodes, QueryResult>
    for QueryNodesHandler<R>
{
    fn handle(&self, query: QueryNodes) -> HexResult<QueryResult> {
        let repository = self.repository.clone();
        tokio::runtime::Handle::current().block_on(async move {
            log::debug!(
                "Executing QueryNodes query: query_string={}",
                query.query_string
            );

            let nodes = repository
                .query_nodes(&query.query_string)
                .await
                .map_err(|e| {
                    Hexserror::adapter("E_KG_QUERY_NODES", &format!("Failed to query nodes: {}", e))
                })?;

            Ok(QueryResult::Nodes(nodes))
        })
    }
}

// ============================================================================
// GET GRAPH STATISTICS
// ============================================================================

#[derive(Debug, Clone)]
pub struct GetGraphStatistics;

pub struct GetGraphStatisticsHandler<R: KnowledgeGraphRepository> {
    repository: Arc<R>,
}

impl<R: KnowledgeGraphRepository> GetGraphStatisticsHandler<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R: KnowledgeGraphRepository + Send + Sync + 'static>
    QueryHandler<GetGraphStatistics, QueryResult> for GetGraphStatisticsHandler<R>
{
    fn handle(&self, _query: GetGraphStatistics) -> HexResult<QueryResult> {
        let repository = self.repository.clone();
        tokio::runtime::Handle::current().block_on(async move {
            log::debug!("Executing GetGraphStatistics query");

            let stats = repository.get_statistics().await.map_err(|e| {
                Hexserror::adapter(
                    "E_KG_GET_STATS",
                    &format!("Failed to get graph statistics: {}", e),
                )
            })?;

            Ok(QueryResult::Statistics(stats))
        })
    }
}
