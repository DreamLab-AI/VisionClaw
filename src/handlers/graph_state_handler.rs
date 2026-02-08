// CQRS-Based Graph State Handler
// Uses Knowledge Graph application layer for all graph operations

use crate::handlers::utils::execute_in_thread;
use crate::{ok_json, error_json, not_found};
use crate::AppState;
use actix_web::{web, Responder};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};

// Import CQRS handlers
use crate::application::knowledge_graph::{
    AddEdge,
    AddEdgeHandler,
    
    AddNode,
    AddNodeHandler,
    BatchUpdatePositions,
    BatchUpdatePositionsHandler,
    GetGraphStatistics,
    GetGraphStatisticsHandler,
    GetNode,
    GetNodeHandler,
    
    LoadGraph,
    LoadGraphHandler,
    RemoveNode,
    RemoveNodeHandler,
    UpdateEdge,
    UpdateEdgeHandler,
    UpdateNode,
    UpdateNodeHandler,
};
use crate::models::edge::Edge;
use crate::models::node::Node;
use hexser::{DirectiveHandler, QueryHandler};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStateResponse {
    pub nodes_count: usize,
    pub edges_count: usize,
    pub metadata_count: usize,
    pub positions: Vec<NodePosition>,
    pub settings_version: String,
    pub timestamp: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePosition {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddNodeRequest {
    pub node: Node,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNodeRequest {
    pub node: Node,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddEdgeRequest {
    pub edge: Edge,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPositionsRequest {
    pub positions: Vec<(u32, f32, f32, f32)>,
}

pub async fn get_graph_state(state: web::Data<AppState>) -> impl Responder {
    info!("Received request for complete graph state via CQRS");

    
    let load_handler = LoadGraphHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || load_handler.handle(LoadGraph)).await;

    match result {
        Ok(Ok(query_result)) => {
            
            let graph_data = match query_result {
                crate::application::knowledge_graph::QueryResult::Graph(graph_arc) => graph_arc,
                _ => {
                    error!("Unexpected query result type");
                    return error_json!("Unexpected query result type");
                }
            };

            
            let graph_ref = graph_data.as_ref();
            let positions: Vec<NodePosition> = graph_ref
                .nodes
                .iter()
                .map(|node| NodePosition {
                    id: node.id,
                    x: node.data.x,
                    y: node.data.y,
                    z: node.data.z,
                })
                .collect();

            let response = GraphStateResponse {
                nodes_count: graph_ref.nodes.len(),
                edges_count: graph_ref.edges.len(),
                metadata_count: graph_ref.metadata.len(),
                positions,
                settings_version: "1.0.0".to_string(), 
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };

            debug!(
                "Returning graph state via CQRS: {} nodes, {} edges, {} metadata entries",
                response.nodes_count, response.edges_count, response.metadata_count
            );

            ok_json!(response)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get graph data: {}", e);
            error_json!("Failed to retrieve graph state", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn get_graph_statistics(state: web::Data<AppState>) -> impl Responder {
    info!("Received request for graph statistics via CQRS");

    
    let handler = GetGraphStatisticsHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || handler.handle(GetGraphStatistics)).await;

    match result {
        Ok(Ok(query_result)) => {
            
            let statistics = match query_result {
                crate::application::knowledge_graph::QueryResult::Statistics(stats) => stats,
                _ => {
                    error!("Unexpected query result type");
                    return error_json!("Unexpected query result type");
                }
            };

            info!("Graph statistics retrieved successfully via CQRS");
            ok_json!(statistics)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get statistics: {}", e);
            error_json!("Failed to retrieve statistics", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn add_node(
    state: web::Data<AppState>,
    request: web::Json<AddNodeRequest>,
) -> impl Responder {
    let node = request.into_inner().node;
    let node_id = node.id;
    info!(
        "Adding node via CQRS directive: metadata_id={}",
        node.metadata_id
    );

    
    let handler = AddNodeHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || handler.handle(AddNode { node })).await;

    match result {
        Ok(Ok(())) => {
            info!("Node added successfully via CQRS: id={}", node_id);
            ok_json!(serde_json::json!({
                "success": true,
                "node_id": node_id
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to add node: {}", e);
            error_json!("Failed to add node", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn update_node(
    state: web::Data<AppState>,
    request: web::Json<UpdateNodeRequest>,
) -> impl Responder {
    let node = request.into_inner().node;
    info!("Updating node via CQRS directive: id={}", node.id);

    
    let handler = UpdateNodeHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || handler.handle(UpdateNode { node })).await;

    match result {
        Ok(Ok(())) => {
            info!("Node updated successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to update node: {}", e);
            error_json!("Failed to update node", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn remove_node(state: web::Data<AppState>, node_id: web::Path<u32>) -> impl Responder {
    let id = node_id.into_inner();
    info!("Removing node via CQRS directive: id={}", id);

    
    let handler = RemoveNodeHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || handler.handle(RemoveNode { node_id: id })).await;

    match result {
        Ok(Ok(())) => {
            info!("Node removed successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to remove node: {}", e);
            error_json!("Failed to remove node", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn get_node(state: web::Data<AppState>, node_id: web::Path<u32>) -> impl Responder {
    let id = node_id.into_inner();
    info!("Getting node via CQRS query: id={}", id);

    
    let handler = GetNodeHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || handler.handle(GetNode { node_id: id })).await;

    match result {
        Ok(Ok(query_result)) => {
            
            let node_opt = match query_result {
                crate::application::knowledge_graph::QueryResult::Node(node) => node,
                _ => {
                    error!("Unexpected query result type");
                    return error_json!("Unexpected query result type");
                }
            };

            match node_opt {
                Some(node) => {
                    info!("Node found via CQRS: id={}", id);
                    ok_json!(node)
                }
                None => {
                    info!("Node not found: id={}", id);
                    not_found!("Node not found")
                }
            }
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get node: {}", e);
            error_json!("Failed to get node", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn add_edge(
    state: web::Data<AppState>,
    request: web::Json<AddEdgeRequest>,
) -> impl Responder {
    let edge = request.into_inner().edge;
    let edge_id = edge.id.clone();
    let edge_source = edge.source;
    let edge_target = edge.target;
    info!(
        "Adding edge via CQRS directive: source={}, target={}",
        edge_source, edge_target
    );

    
    let handler = AddEdgeHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || handler.handle(AddEdge { edge })).await;

    match result {
        Ok(Ok(())) => {
            info!("Edge added successfully via CQRS: id={}", edge_id);
            ok_json!(serde_json::json!({
                "success": true,
                "edge_id": edge_id
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to add edge: {}", e);
            error_json!("Failed to add edge", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn update_edge(state: web::Data<AppState>, request: web::Json<Edge>) -> impl Responder {
    let edge = request.into_inner();
    info!("Updating edge via CQRS directive: id={}", edge.id);

    
    let handler = UpdateEdgeHandler::new(state.neo4j_adapter.clone());

    
    let result = execute_in_thread(move || handler.handle(UpdateEdge { edge })).await;

    match result {
        Ok(Ok(())) => {
            info!("Edge updated successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to update edge: {}", e);
            error_json!("Failed to update edge", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn batch_update_positions(
    state: web::Data<AppState>,
    request: web::Json<BatchPositionsRequest>,
) -> impl Responder {
    let positions = request.into_inner().positions;
    info!(
        "Batch updating {} positions via CQRS directive",
        positions.len()
    );

    
    let handler = BatchUpdatePositionsHandler::new(state.neo4j_adapter.clone());

    
    let result =
        execute_in_thread(move || handler.handle(BatchUpdatePositions { positions })).await;

    match result {
        Ok(Ok(())) => {
            info!("Positions updated successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to batch update positions: {}", e);
            error_json!("Failed to update positions", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/graph")
            .route("/state", web::get().to(get_graph_state))
            .route("/statistics", web::get().to(get_graph_statistics))
            .route("/nodes", web::post().to(add_node))
            .route("/nodes/{id}", web::get().to(get_node))
            .route("/nodes/{id}", web::put().to(update_node))
            .route("/nodes/{id}", web::delete().to(remove_node))
            .route("/edges", web::post().to(add_edge))
            .route("/edges/{id}", web::put().to(update_edge))
            .route("/positions/batch", web::post().to(batch_update_positions)),
    );
}
