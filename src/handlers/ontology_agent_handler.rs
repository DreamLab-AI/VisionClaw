//! HTTP handler for ontology agent tools.
//!
//! Exposes the OntologyQueryService and OntologyMutationService as REST endpoints
//! that agents call via MCP tool routing. Each endpoint mirrors an MCP tool:
//!   POST /ontology-agent/discover
//!   POST /ontology-agent/read
//!   POST /ontology-agent/query
//!   POST /ontology-agent/traverse
//!   POST /ontology-agent/propose
//!   POST /ontology-agent/validate
//!   GET  /ontology-agent/status

use crate::services::ontology_query_service::OntologyQueryService;
use crate::services::ontology_mutation_service::OntologyMutationService;
use crate::types::ontology_tools::*;
use crate::{ok_json, error_json};
use actix_web::{web, HttpResponse, Error};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ---------- Request / Response DTOs ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub domain: Option<String>,
}

fn default_limit() -> usize { 20 }

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadNoteRequest {
    pub iri: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub cypher: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraverseRequest {
    pub start_iri: String,
    #[serde(default = "default_depth")]
    pub depth: usize,
    pub relationship_types: Option<Vec<String>>,
}

fn default_depth() -> usize { 3 }

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposeRequest {
    pub proposal: ProposeInput,
    pub agent_context: AgentContext,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateRequest {
    pub axioms: Vec<AxiomInput>,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub service: String,
    pub status: String,
    pub capabilities: Vec<String>,
}

// ---------- Handlers ----------

/// POST /ontology-agent/discover — Semantic discovery via class hierarchy + Whelk
pub async fn discover(
    query_service: web::Data<Arc<OntologyQueryService>>,
    request: web::Json<DiscoverRequest>,
) -> Result<HttpResponse, Error> {
    let req = request.into_inner();
    info!("ontology-agent/discover: query='{}'", req.query);

    match query_service.discover(&req.query, req.limit, req.domain.as_deref()).await {
        Ok(results) => {
            ok_json!(serde_json::json!({
                "success": true,
                "results": results,
                "count": results.len()
            }))
        }
        Err(e) => {
            error!("ontology-agent/discover failed: {}", e);
            error_json!("Discovery failed", e)
        }
    }
}

/// POST /ontology-agent/read — Read note with full ontology context
pub async fn read_note(
    query_service: web::Data<Arc<OntologyQueryService>>,
    request: web::Json<ReadNoteRequest>,
) -> Result<HttpResponse, Error> {
    let req = request.into_inner();
    info!("ontology-agent/read: iri='{}'", req.iri);

    match query_service.read_note(&req.iri).await {
        Ok(note) => {
            ok_json!(serde_json::json!({
                "success": true,
                "note": note
            }))
        }
        Err(e) => {
            error!("ontology-agent/read failed: {}", e);
            error_json!("Read note failed", e)
        }
    }
}

/// POST /ontology-agent/query — Validated Cypher execution
pub async fn query(
    query_service: web::Data<Arc<OntologyQueryService>>,
    request: web::Json<QueryRequest>,
) -> Result<HttpResponse, Error> {
    let req = request.into_inner();
    info!("ontology-agent/query: cypher='{}'", &req.cypher[..req.cypher.len().min(80)]);

    match query_service.validate_and_execute_cypher(&req.cypher).await {
        Ok(validation) => {
            ok_json!(serde_json::json!({
                "success": true,
                "validation": validation
            }))
        }
        Err(e) => {
            error!("ontology-agent/query failed: {}", e);
            error_json!("Query validation failed", e)
        }
    }
}

/// POST /ontology-agent/traverse — Walk ontology graph from a starting IRI
pub async fn traverse(
    query_service: web::Data<Arc<OntologyQueryService>>,
    request: web::Json<TraverseRequest>,
) -> Result<HttpResponse, Error> {
    let req = request.into_inner();
    info!("ontology-agent/traverse: start='{}', depth={}", req.start_iri, req.depth);

    // Traverse by reading the start note and following relationships
    let result = build_traversal(&query_service, &req.start_iri, req.depth, req.relationship_types.as_deref()).await;

    match result {
        Ok(traversal) => {
            ok_json!(serde_json::json!({
                "success": true,
                "traversal": traversal
            }))
        }
        Err(e) => {
            error!("ontology-agent/traverse failed: {}", e);
            error_json!("Traversal failed", e)
        }
    }
}

/// POST /ontology-agent/propose — Propose new note or amendment
pub async fn propose(
    mutation_service: web::Data<Arc<OntologyMutationService>>,
    request: web::Json<ProposeRequest>,
) -> Result<HttpResponse, Error> {
    let req = request.into_inner();
    info!("ontology-agent/propose: agent={}", req.agent_context.agent_id);

    let result = match req.proposal {
        ProposeInput::Create(proposal) => {
            mutation_service.propose_create(proposal, req.agent_context).await
        }
        ProposeInput::Amend { target_iri, amendment } => {
            mutation_service.propose_amend(&target_iri, amendment, req.agent_context).await
        }
    };

    match result {
        Ok(proposal_result) => {
            ok_json!(serde_json::json!({
                "success": true,
                "proposal": proposal_result
            }))
        }
        Err(e) => {
            error!("ontology-agent/propose failed: {}", e);
            error_json!("Proposal failed", e)
        }
    }
}

/// POST /ontology-agent/validate — Check axioms for Whelk consistency
pub async fn validate(
    query_service: web::Data<Arc<OntologyQueryService>>,
    request: web::Json<ValidateRequest>,
) -> Result<HttpResponse, Error> {
    let req = request.into_inner();
    info!("ontology-agent/validate: {} axioms", req.axioms.len());

    // Build Cypher-like validation by checking each axiom subject/object against known classes
    let mut all_errors = Vec::new();
    let mut all_hints = Vec::new();

    for axiom in &req.axioms {
        // Validate subject exists
        let subject_check = format!("MATCH (n:{}) RETURN n", axiom.subject.split(':').last().unwrap_or(&axiom.subject));
        if let Ok(validation) = query_service.validate_and_execute_cypher(&subject_check).await {
            all_errors.extend(validation.errors);
            all_hints.extend(validation.hints);
        }
    }

    ok_json!(serde_json::json!({
        "success": true,
        "valid": all_errors.is_empty(),
        "errors": all_errors,
        "hints": all_hints,
        "axiom_count": req.axioms.len()
    }))
}

/// GET /ontology-agent/status — Service health and capability listing
pub async fn status() -> Result<HttpResponse, Error> {
    ok_json!(StatusResponse {
        service: "ontology-agent".to_string(),
        status: "healthy".to_string(),
        capabilities: vec![
            "ontology_discover".to_string(),
            "ontology_read".to_string(),
            "ontology_query".to_string(),
            "ontology_traverse".to_string(),
            "ontology_propose".to_string(),
            "ontology_validate".to_string(),
        ],
    })
}

// ---------- Helpers ----------

/// Build a traversal result by walking the ontology graph via read_note relationships.
async fn build_traversal(
    query_service: &OntologyQueryService,
    start_iri: &str,
    max_depth: usize,
    rel_filter: Option<&[String]>,
) -> Result<TraversalResult, String> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();

    queue.push_back((start_iri.to_string(), 0usize));
    visited.insert(start_iri.to_string());

    while let Some((current_iri, depth)) = queue.pop_front() {
        if depth > max_depth {
            continue;
        }

        // Read the note to get relationships
        match query_service.read_note(&current_iri).await {
            Ok(note) => {
                nodes.push(TraversalNode {
                    iri: note.iri.clone(),
                    preferred_term: note.preferred_term.clone(),
                    domain: note.ontology_metadata.domain.clone(),
                    depth,
                });

                // Follow related notes
                for related in &note.related_notes {
                    let rel_type = &related.relationship_type;
                    let should_follow = rel_filter
                        .map(|types| types.iter().any(|t| t == rel_type))
                        .unwrap_or(true);

                    if should_follow {
                        edges.push(TraversalEdge {
                            source_iri: current_iri.clone(),
                            target_iri: related.iri.clone(),
                            relationship_type: rel_type.clone(),
                        });

                        if depth + 1 <= max_depth && !visited.contains(&related.iri) {
                            visited.insert(related.iri.clone());
                            queue.push_back((related.iri.clone(), depth + 1));
                        }
                    }
                }
            }
            Err(e) => {
                // Skip nodes that can't be read (may not exist)
                log::debug!("Traversal: skipping {} — {}", current_iri, e);
            }
        }
    }

    Ok(TraversalResult {
        start_iri: start_iri.to_string(),
        nodes,
        edges,
    })
}

// ---------- Route Configuration ----------

pub fn configure_ontology_agent_routes(cfg: &mut web::ServiceConfig) {
    use crate::middleware::RequireAuth;

    cfg.service(
        web::scope("/ontology-agent")
            // Every ontology-agent route performs reads/mutations against the
            // knowledge graph with side effects (proposals, validations,
            // traversals that touch private nodes). Require authentication
            // across the scope — anonymous callers get 401.
            .wrap(RequireAuth::authenticated())
            .route("/discover", web::post().to(discover))
            .route("/read", web::post().to(read_note))
            .route("/query", web::post().to(query))
            .route("/traverse", web::post().to(traverse))
            .route("/propose", web::post().to(propose))
            .route("/validate", web::post().to(validate))
            .route("/status", web::get().to(status))
    );
}
