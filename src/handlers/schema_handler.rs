//! Schema Handler
//!
//! Provides REST API endpoints for accessing graph schema information.
//! Enables natural language query generation by exposing available node types,
//! edge types, and properties to LLMs.

use actix_web::{web, HttpResponse, Responder};
use log::{debug, info};
use serde::Serialize;
use std::sync:::Arc;

use crate::services::schema_service::{SchemaService, GraphSchema};
use crate::actors::graph_state_actor:::GraphStateActor;
use actix:::Addr;

// Response macros
use crate::{ok_json, error_json};

/// Schema response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaResponse {
    /// Graph schema metadata
    pub schema: GraphSchema,
    /// Schema formatted for LLM context
    pub llm_context: String,
}

/// Node type information response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTypeResponse {
    /// Node type name
    pub node_type: String,
    /// Number of nodes with this type
    pub count: usize,
}

/// Edge type information response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeTypeResponse {
    /// Edge type name
    pub edge_type: String,
    /// Number of edges with this type
    pub count: usize,
}

/// Get complete graph schema
/// GET /api/schema
/// Returns comprehensive schema information including:
/// - Available node types with counts
/// - Available edge types with counts
/// - Node and edge properties
/// - Sample labels and values
/// - OWL classes and properties
/// - LLM-formatted context string
/// # Example Response
/// ```json
/// {
///   "schema": {
///     "node_types": {"person": 150, "organization": 45, "project": 30},
///     "edge_types": {"dependency": 200, "hierarchy": 120},
///     "total_nodes": 225,
///     "total_edges": 320
///   },
///   "llm_context": "# Graph Schema\n\nTotal Nodes: 225..."
/// }
/// ```
pub async fn get_schema(
    schema_service: web::Data<Arc<SchemaService>>,
    graph_state_actor: web::Data<Addr<GraphStateActor>>,
) -> impl Responder {
    debug!("Retrieving graph schema");

    // Get current graph from actor
    let graph_result = graph_state_actor
        .send(crate::actors::messages::GetGraphData)
        .await;

    match graph_result {
        Ok(Ok(graph_data)) => {
            // Update schema from current graph - graph_data is Arc<GraphData>
            schema_service.update_schema(&graph_data).await;

            // Get schema and LLM context
            let schema = schema_service.get_schema().await;
            let llm_context = schema_service.get_llm_context().await;

            let response = SchemaResponse {
                schema,
                llm_context,
            };

            info!("Schema retrieved successfully");
            ok_json!(response)
        }
        Ok(Err(e)) => {
            error_json!("Failed to retrieve graph data", e.to_string())
        }
        Err(e) => {
            error_json!("Actor communication error", e.to_string())
        }
    }
}

/// Get LLM context string
/// GET /api/schema/llm-context
/// Returns a human-readable schema description optimized for LLM consumption.
/// This includes:
/// - Graph statistics
/// - Available types
/// - Sample Cypher queries
/// - Property examples
/// # Example Response
/// ```text
/// # Graph Schema
/// Total Nodes: 225
/// Total Edges: 320
/// ## Node Types
/// - person (150 nodes)
/// - organization (45 nodes)
/// ...
/// ```
pub async fn get_llm_context(
    schema_service: web::Data<Arc<SchemaService>>,
) -> impl Responder {
    debug!("Retrieving LLM context");

    let context = schema_service.get_llm_context().await;

    HttpResponse::Ok()
        .content_type("text/plain")
        .body(context)
}

/// Get all available node types
/// GET /api/schema/node-types
/// Returns list of all node types present in the graph with their counts.
/// # Example Response
/// ```json
/// {
///   "node_types": [
///     {"node_type": "person", "count": 150},
///     {"node_type": "organization", "count": 45}
///   ]
/// }
/// ```
pub async fn get_node_types(
    schema_service: web::Data<Arc<SchemaService>>,
) -> Result<HttpResponse, actix_web::Error> {
    debug!("Retrieving node types");

    let schema = schema_service.get_schema().await;
    let node_types: Vec<NodeTypeResponse> = schema
        .node_types
        .into_iter()
        .map(|(node_type, count)| NodeTypeResponse { node_type, count })
        .collect();

    ok_json!(serde_json::json!({ "node_types": node_types }))
}

/// Get all available edge types
/// GET /api/schema/edge-types
/// Returns list of all edge types present in the graph with their counts.
/// # Example Response
/// ```json
/// {
///   "edge_types": [
///     {"edge_type": "dependency", "count": 200},
///     {"edge_type": "hierarchy", "count": 120}
///   ]
/// }
/// ```
pub async fn get_edge_types(
    schema_service: web::Data<Arc<SchemaService>>,
) -> Result<HttpResponse, actix_web::Error> {
    debug!("Retrieving edge types");

    let schema = schema_service.get_schema().await;
    let edge_types: Vec<EdgeTypeResponse> = schema
        .edge_types
        .into_iter()
        .map(|(edge_type, count)| EdgeTypeResponse { edge_type, count })
        .collect();

    ok_json!(serde_json::json!({ "edge_types": edge_types }))
}

/// Get information about specific node type
/// GET /api/schema/node-types/{type}
/// Returns count of nodes with the specified type.
/// # Example Response
/// ```json
/// {
///   "node_type": "person",
///   "count": 150
/// }
/// ```
pub async fn get_node_type_info(
    schema_service: web::Data<Arc<SchemaService>>,
    path: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let node_type = path.into_inner();
    debug!("Retrieving info for node type: {}", node_type);

    let type_info: Option<usize> = schema_service.get_node_type_info(&node_type).await;
    match type_info {
        Some(count) => {
            let response = NodeTypeResponse {
                node_type: node_type.clone(),
                count,
            };
            ok_json!(response)
        }
        None => {
            error_json!("Node type not found", format!("No nodes with type '{}'", node_type))
        }
    }
}

/// Get information about specific edge type
/// GET /api/schema/edge-types/{type}
/// Returns count of edges with the specified type.
/// # Example Response
/// ```json
/// {
///   "edge_type": "dependency",
///   "count": 200
/// }
/// ```
pub async fn get_edge_type_info(
    schema_service: web::Data<Arc<SchemaService>>,
    path: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let edge_type = path.into_inner();
    debug!("Retrieving info for edge type: {}", edge_type);

    let type_info: Option<usize> = schema_service.get_edge_type_info(&edge_type).await;
    match type_info {
        Some(count) => {
            let response = EdgeTypeResponse {
                edge_type: edge_type.clone(),
                count,
            };
            ok_json!(response)
        }
        None => {
            error_json!("Edge type not found", format!("No edges with type '{}'", edge_type))
        }
    }
}

/// Configure schema routes
pub fn configure_schema_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/schema")
            .route("", web::get().to(get_schema))
            .route("/llm-context", web::get().to(get_llm_context))
            .route("/node-types", web::get().to(get_node_types))
            .route("/edge-types", web::get().to(get_edge_types))
            .route("/node-types/{type}", web::get().to(get_node_type_info))
            .route("/edge-types/{type}", web::get().to(get_edge_type_info))
    );
}

