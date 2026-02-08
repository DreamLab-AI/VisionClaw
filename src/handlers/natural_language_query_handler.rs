//! Natural Language Query Handler
//!
//! REST API endpoints for translating natural language to Cypher queries

use actix_web::{web, HttpResponse, Responder};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::services::natural_language_query_service::{
    NaturalLanguageQueryService, QueryTranslation as CypherTranslation, QueryPatterns
};

// Response macros
use crate::{ok_json, error_json};

/// Natural language query request
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NaturalLanguageQueryRequest {
    /// Natural language query
    pub query: String,
    /// Whether to return multiple suggestions
    #[serde(default)]
    pub suggest_alternatives: bool,
}

/// Query translation response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTranslationResponse {
    /// Translated query/queries
    pub translations: Vec<CypherTranslation>,
    /// Example queries for reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<ExampleQuery>>,
}

/// Example query
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExampleQuery {
    /// Natural language description
    pub description: String,
    /// Cypher query
    pub cypher: String,
}

/// Cypher explanation request
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainCypherRequest {
    /// Cypher query to explain
    pub cypher: String,
}

/// Cypher explanation response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainCypherResponse {
    /// Original Cypher query
    pub cypher: String,
    /// Natural language explanation
    pub explanation: String,
}

/// Translate natural language to Cypher
/// POST /api/nl-query/translate
/// Translates a natural language query into one or more Cypher queries.
/// Uses the current graph schema to generate contextually appropriate queries.
/// # Request Body
/// ```json
/// {
///   "query": "Show me all person nodes connected to Project X",
///   "suggestAlternatives": false
/// }
/// ```
/// # Response
/// ```json
/// {
///   "translations": [{
///     "originalQuery": "Show me all person nodes connected to Project X",
///     "cypherQuery": "MATCH (p:GraphNode {node_type: 'person'})-[r:EDGE]-(x:GraphNode {label: 'Project X'}) RETURN p, r",
///     "explanation": "Finds all person nodes connected to Project X",
///     "confidence": 0.85,
///     "warnings": []
///   }]
/// }
/// ```
pub async fn translate_query(
    nl_service: web::Data<Arc<NaturalLanguageQueryService>>,
    request: web::Json<NaturalLanguageQueryRequest>,
) -> impl Responder {
    info!("Translating natural language query: {}", request.query);

    let result = if request.suggest_alternatives {
        // Get multiple suggestions
        nl_service.suggest_queries(&request.query).await
    } else {
        // Get single best translation
        nl_service.translate_to_cypher(&request.query)
            .await
            .map(|t| vec![t])
    };

    let result: Result<Vec<CypherTranslation>, String> = result;
    match result {
        Ok(translations) => {
            let response = QueryTranslationResponse {
                translations,
                examples: None,
            };
            ok_json!(response)
        }
        Err(e) => {
            error_json!("Translation failed", e)
        }
    }
}

/// Get example queries
/// GET /api/nl-query/examples
/// Returns a list of example natural language queries and their Cypher translations.
/// Useful for helping users understand what kinds of queries they can ask.
/// # Response
/// ```json
/// {
///   "examples": [
///     {
///       "description": "Show me all person nodes",
///       "cypher": "MATCH (n:GraphNode {node_type: 'person'}) RETURN n LIMIT 50"
///     }
///   ]
/// }
/// ```
pub async fn get_examples() -> Result<HttpResponse, actix_web::Error> {
    debug!("Retrieving example queries");

    let examples: Vec<ExampleQuery> = QueryPatterns::examples()
        .into_iter()
        .map(|(desc, cypher)| ExampleQuery {
            description: desc.to_string(),
            cypher: cypher.to_string(),
        })
        .collect();

    ok_json!(serde_json::json!({ "examples": examples }))
}

/// Explain Cypher query in natural language
/// POST /api/nl-query/explain
/// Takes a Cypher query and generates a natural language explanation
/// of what it does.
/// # Request Body
/// ```json
/// {
///   "cypher": "MATCH (n:GraphNode)-[r:EDGE*1..3]-(m:GraphNode) RETURN n, m LIMIT 10"
/// }
/// ```
/// # Response
/// ```json
/// {
///   "cypher": "MATCH (n:GraphNode)-[r:EDGE*1..3]-(m:GraphNode) RETURN n, m LIMIT 10",
///   "explanation": "This query finds pairs of nodes that are connected by 1 to 3 relationships..."
/// }
/// ```
pub async fn explain_cypher(
    nl_service: web::Data<Arc<NaturalLanguageQueryService>>,
    request: web::Json<ExplainCypherRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    debug!("Explaining Cypher query");

    // Validate syntax first
    if let Err(e) = nl_service.validate_cypher(&request.cypher) {
        return error_json!("Invalid Cypher syntax", e);
    }

    match nl_service.explain_cypher(&request.cypher).await {
        Ok(explanation) => {
            let response = ExplainCypherResponse {
                cypher: request.cypher.clone(),
                explanation,
            };
            ok_json!(response)
        }
        Err(e) => {
            error_json!("Explanation failed", e)
        }
    }
}

/// Validate Cypher syntax
/// POST /api/nl-query/validate
/// Validates a Cypher query for basic syntax errors and dangerous operations.
/// # Request Body
/// ```json
/// {
///   "cypher": "MATCH (n:GraphNode) RETURN n"
/// }
/// ```
/// # Response
/// ```json
/// {
///   "valid": true,
///   "errors": []
/// }
/// ```
pub async fn validate_cypher(
    nl_service: web::Data<Arc<NaturalLanguageQueryService>>,
    request: web::Json<ExplainCypherRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    debug!("Validating Cypher query");

    let validation_result: Result<(), String> = nl_service.validate_cypher(&request.cypher);
    match validation_result {
        Ok(()) => {
            ok_json!(serde_json::json!({
                "valid": true,
                "errors": []
            }))
        }
        Err(e) => {
            ok_json!(serde_json::json!({
                "valid": false,
                "errors": [e]
            }))
        }
    }
}

/// Configure natural language query routes
pub fn configure_nl_query_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/nl-query")
            .route("/translate", web::post().to(translate_query))
            .route("/examples", web::get().to(get_examples))
            .route("/explain", web::post().to(explain_cypher))
            .route("/validate", web::post().to(validate_cypher))
    );
}
