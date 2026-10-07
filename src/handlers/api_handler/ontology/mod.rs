//! Ontology REST and WebSocket API endpoints
//!
//! This module provides comprehensive API endpoints for ontology operations including:
//! - Loading ontology axioms from files/URLs
//! - Updating mapping configurations
//! - Running validation with different modes
//! - Real-time WebSocket updates for validation progress
//! - Applying inferences to the graph
//! - System health monitoring and cache management

use crate::{accepted, ok_json};
use actix::Addr;
use actix_web::{web, Error as ActixError, HttpRequest, HttpResponse, Responder};
use actix_web_actors::ws;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration as StdDuration;
use uuid::Uuid;
use visionclaw_domain::ports::ontology_repository::OntologyRepository;

use crate::actors::messages::{
    ApplyInferences, ClearOntologyCaches, GetOntologyHealth, GetOntologyReport, LoadOntologyAxioms,
    OntologyHealth, UpdateOntologyMapping, ValidateOntology, ValidationMode,
};
use crate::actors::ontology_actor::OntologyActor;
use crate::handlers::api_handler::analytics::FEATURE_FLAGS;
use crate::services::owl_validator::{PropertyGraph, RdfTriple, ValidationConfig};
use crate::AppState;

// ============================================================================
// REQUEST/RESPONSE DTOs
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadOntologyRequest {
    pub content: String,

    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadOntologyResponse {
    pub ontology_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub axiom_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadAxiomsRequest {
    pub source: String,

    pub format: Option<String>,

    pub validate_immediately: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadAxiomsResponse {
    pub ontology_id: String,

    pub loaded_at: DateTime<Utc>,

    pub axiom_count: Option<u32>,

    pub loading_time_ms: u64,

    pub validation_job_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateRequest {
    pub ontology_id: Option<String>,

    pub mode: Option<ValidationModeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingRequest {
    pub config: ValidationConfigDto,

    pub apply_to_all: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationConfigDto {
    pub enable_reasoning: Option<bool>,

    pub reasoning_timeout_seconds: Option<u64>,

    pub enable_inference: Option<bool>,

    pub max_inference_depth: Option<usize>,

    pub enable_caching: Option<bool>,

    pub cache_ttl_seconds: Option<u64>,

    pub validate_cardinality: Option<bool>,

    pub validate_domains_ranges: Option<bool>,

    pub validate_disjoint_classes: Option<bool>,
}

impl From<ValidationConfigDto> for ValidationConfig {
    fn from(dto: ValidationConfigDto) -> Self {
        ValidationConfig {
            enable_reasoning: dto.enable_reasoning.unwrap_or(true),
            reasoning_timeout_seconds: dto.reasoning_timeout_seconds.unwrap_or(30),
            enable_inference: dto.enable_inference.unwrap_or(true),
            max_inference_depth: dto.max_inference_depth.unwrap_or(3),
            enable_caching: dto.enable_caching.unwrap_or(true),
            cache_ttl_seconds: dto.cache_ttl_seconds.unwrap_or(3600),
            validate_cardinality: dto.validate_cardinality.unwrap_or(true),
            validate_domains_ranges: dto.validate_domains_ranges.unwrap_or(true),
            validate_disjoint_classes: dto.validate_disjoint_classes.unwrap_or(true),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRequest {
    pub ontology_id: String,

    pub mode: ValidationModeDto,

    pub priority: Option<u8>,

    pub enable_websocket_updates: Option<bool>,

    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationModeDto {
    Quick,
    Full,
    Incremental,
}

impl From<ValidationModeDto> for ValidationMode {
    fn from(dto: ValidationModeDto) -> Self {
        match dto {
            ValidationModeDto::Quick => ValidationMode::Quick,
            ValidationModeDto::Full => ValidationMode::Full,
            ValidationModeDto::Incremental => ValidationMode::Incremental,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResponse {
    pub job_id: String,

    pub status: String,

    pub estimated_completion: Option<DateTime<Utc>>,

    pub queue_position: Option<usize>,

    pub websocket_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInferencesRequest {
    pub rdf_triples: Vec<RdfTripleDto>,

    pub max_depth: Option<usize>,

    pub update_graph: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RdfTripleDto {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub is_literal: Option<bool>,
    pub datatype: Option<String>,
    pub language: Option<String>,
}

impl From<RdfTripleDto> for RdfTriple {
    fn from(dto: RdfTripleDto) -> Self {
        RdfTriple {
            subject: dto.subject,
            predicate: dto.predicate,
            object: dto.object,
            is_literal: dto.is_literal.unwrap_or(false),
            datatype: dto.datatype,
            language: dto.language,
        }
    }
}

impl From<RdfTriple> for RdfTripleDto {
    fn from(triple: RdfTriple) -> Self {
        RdfTripleDto {
            subject: triple.subject,
            predicate: triple.predicate,
            object: triple.object,
            is_literal: Some(triple.is_literal),
            datatype: triple.datatype,
            language: triple.language,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceResult {
    pub input_count: usize,

    pub inferred_triples: Vec<RdfTripleDto>,

    pub processing_time_ms: u64,

    pub graph_updated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatusResponse {
    pub status: String,

    pub health: OntologyHealthDto,

    pub ontology_validation_enabled: bool,

    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyHealthDto {
    pub loaded_ontologies: u32,
    pub cached_reports: u32,
    pub validation_queue_size: u32,
    pub last_validation: Option<DateTime<Utc>>,
    pub cache_hit_rate: f32,
    pub avg_validation_time_ms: f32,
    pub active_jobs: u32,
    pub memory_usage_mb: f32,
}

impl From<OntologyHealth> for OntologyHealthDto {
    fn from(health: OntologyHealth) -> Self {
        OntologyHealthDto {
            loaded_ontologies: health.loaded_ontologies,
            cached_reports: health.cached_reports,
            validation_queue_size: health.validation_queue_size,
            last_validation: health.last_validation,
            cache_hit_rate: health.cache_hit_rate,
            avg_validation_time_ms: health.avg_validation_time_ms,
            active_jobs: health.active_jobs,
            memory_usage_mb: health.memory_usage_mb,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassNode {
    pub iri: String,
    pub label: String,
    pub parent_iri: Option<String>,
    pub children_iris: Vec<String>,
    pub node_count: usize,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassHierarchy {
    pub root_classes: Vec<String>,
    pub hierarchy: HashMap<String, ClassNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyParams {
    pub ontology_id: Option<String>,
    pub max_depth: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
    pub details: Option<HashMap<String, serde_json::Value>>,
    pub timestamp: DateTime<Utc>,
    pub trace_id: String,
}

impl ErrorResponse {
    pub fn new(error: &str, code: &str) -> Self {
        Self {
            error: error.to_string(),
            code: code.to_string(),
            details: None,
            timestamp: Utc::now(),
            trace_id: Uuid::new_v4().to_string(),
        }
    }

    pub fn with_details(mut self, details: HashMap<String, serde_json::Value>) -> Self {
        self.details = Some(details);
        self
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

async fn check_feature_enabled() -> Result<(), ErrorResponse> {
    let flags = FEATURE_FLAGS.lock().await;

    if !flags.ontology_validation {
        let mut details = HashMap::new();
        details.insert(
            "message".to_string(),
            serde_json::json!("Enable the ontology_validation feature flag to use this endpoint"),
        );

        return Err(ErrorResponse::new(
            "Ontology validation feature is disabled",
            "FEATURE_DISABLED",
        )
        .with_details(details));
    }

    Ok(())
}

#[allow(dead_code)]
fn actor_timeout() -> StdDuration {
    StdDuration::from_secs(30)
}

async fn extract_property_graph(state: &AppState) -> Result<PropertyGraph, ErrorResponse> {
    use crate::services::owl_validator::{GraphEdge, GraphNode};

    match state.ontology_repository.load_ontology_graph().await {
        Ok(graph_data) => {
            let nodes: Vec<GraphNode> = graph_data
                .nodes
                .iter()
                .map(|n| {
                    let mut properties = HashMap::new();
                    properties.insert("label".to_string(), serde_json::json!(n.label));
                    if let Some(ref iri) = n.owl_class_iri {
                        properties.insert("owl_class_iri".to_string(), serde_json::json!(iri));
                    }
                    GraphNode {
                        id: n.metadata_id.clone(),
                        labels: vec![n.label.clone()],
                        properties,
                    }
                })
                .collect();

            let edges: Vec<GraphEdge> = graph_data
                .edges
                .iter()
                .map(|e| GraphEdge {
                    id: format!("{}-{}", e.source, e.target),
                    source: e.source.to_string(),
                    target: e.target.to_string(),
                    relationship_type: e.edge_type.clone().unwrap_or_else(|| "RELATES".to_string()),
                    properties: HashMap::new(),
                })
                .collect();

            Ok(PropertyGraph {
                nodes,
                edges,
                metadata: HashMap::new(),
            })
        }
        Err(e) => Err(ErrorResponse::new(
            &format!("Failed to extract property graph: {}", e),
            "PROPERTY_GRAPH_EXTRACTION_FAILED",
        )),
    }
}

// ============================================================================
// REST ENDPOINTS
// ============================================================================

/// Maximum axiom content size: 10 MB
const MAX_AXIOM_CONTENT_SIZE: usize = 10_000_000;

pub async fn load_axioms(state: web::Data<AppState>, body: web::Bytes) -> impl Responder {
    // SECURITY: Reject oversized payloads to prevent resource exhaustion
    if body.len() > MAX_AXIOM_CONTENT_SIZE {
        warn!(
            "Axiom payload rejected: {} bytes exceeds limit of {} bytes",
            body.len(),
            MAX_AXIOM_CONTENT_SIZE
        );
        let error_response = ErrorResponse::new(
            &format!(
                "Payload too large: {} bytes exceeds maximum of {} bytes",
                body.len(),
                MAX_AXIOM_CONTENT_SIZE
            ),
            "PAYLOAD_TOO_LARGE",
        );
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::PayloadTooLarge().json(error_response),
        );
    }

    let (source, format) = if let Ok(req) = serde_json::from_slice::<LoadOntologyRequest>(&body) {
        info!("Loading ontology from content string");
        (req.content, req.format)
    } else if let Ok(req) = serde_json::from_slice::<LoadAxiomsRequest>(&body) {
        info!("Loading ontology axioms from source: {}", req.source);
        (req.source, req.format)
    } else {
        let error_response = ErrorResponse::new("Invalid request format", "INVALID_REQUEST");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::BadRequest().json(error_response),
        );
    };

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let start_time = std::time::Instant::now();

    let load_msg = LoadOntologyAxioms { source, format };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(load_msg).await {
        Ok(Ok(ontology_id)) => {
            let _loading_time_ms = start_time.elapsed().as_millis() as u64;

            let response = LoadOntologyResponse {
                ontology_id: ontology_id.clone(),
                axiom_count: None,
            };

            info!("Successfully loaded ontology: {}", response.ontology_id);
            ok_json!(response)
        }
        Ok(Err(error)) => {
            error!("Failed to load ontology: {}", error);
            let error_response = ErrorResponse::new(&error, "LOAD_FAILED");
            Ok(HttpResponse::BadRequest().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn update_mapping(
    state: web::Data<AppState>,
    req: web::Json<MappingRequest>,
) -> impl Responder {
    info!("Updating ontology mapping configuration");

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let config = ValidationConfig::from(req.config.clone());

    let update_msg = UpdateOntologyMapping { config };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(update_msg).await {
        Ok(Ok(())) => {
            info!("Successfully updated ontology mapping");
            ok_json!(serde_json::json!({
                "status": "success",
                "message": "Mapping configuration updated",
                "timestamp": Utc::now()
            }))
        }
        Ok(Err(error)) => {
            error!("Failed to update mapping: {}", error);
            let error_response = ErrorResponse::new(&error, "MAPPING_UPDATE_FAILED");
            Ok(HttpResponse::BadRequest().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn validate_ontology(
    state: web::Data<AppState>,
    req: web::Json<ValidationRequest>,
) -> impl Responder {
    info!(
        "Starting ontology validation: {} (mode: {:?})",
        req.ontology_id, req.mode
    );

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let property_graph = match extract_property_graph(&state).await {
        Ok(graph) => graph,
        Err(error) => {
            return Ok::<HttpResponse, actix_web::Error>(
                HttpResponse::InternalServerError().json(error),
            )
        }
    };

    let validation_msg = ValidateOntology {
        ontology_id: req.ontology_id.clone(),
        graph_data: property_graph,
        mode: ValidationMode::from(req.mode.clone()),
        job_id: None,
    };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(validation_msg).await {
        Ok(Ok(report)) => {
            let response = ValidationResponse {
                job_id: report.id.clone(),
                status: "completed".to_string(),
                estimated_completion: Some(Utc::now()),
                queue_position: None,
                websocket_url: req
                    .client_id
                    .as_ref()
                    .map(|id| format!("/api/ontology/ws?client_id={}", id)),
            };

            info!(
                "Validation completed for {}: {} violations found",
                req.ontology_id,
                report.violations.len()
            );
            ok_json!(response)
        }
        Ok(Err(error)) => {
            error!("Validation failed: {}", error);
            let error_response = ErrorResponse::new(&error, "VALIDATION_FAILED");
            Ok(HttpResponse::BadRequest().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn get_validation_report(
    state: web::Data<AppState>,
    query: web::Query<HashMap<String, String>>,
) -> impl Responder {
    // The report is cached under BOTH the job id and the ontology id, so accept
    // whichever key the client supplies (`jobId`, `ontologyId`, or the legacy
    // `report_id`). Absent all three, the actor returns the latest report.
    let report_id = query
        .get("jobId")
        .or_else(|| query.get("ontologyId"))
        .or_else(|| query.get("report_id"))
        .cloned();

    info!("Retrieving validation report: {:?}", report_id);

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let report_msg = GetOntologyReport { report_id };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(report_msg).await {
        Ok(Ok(Some(report))) => {
            // A job that is enqueued but not yet finished has a placeholder
            // report; surface it as 202 (pending) rather than 200 or 404.
            if report.graph_signature == "pending" {
                info!("Validation report {} still pending", report.id);
                accepted!(report)
            } else {
                info!("Retrieved validation report: {}", report.id);
                ok_json!(report)
            }
        }
        Ok(Ok(None)) => {
            warn!("Validation report not found");
            let error_response = ErrorResponse::new("Report not found", "REPORT_NOT_FOUND");
            Ok(HttpResponse::NotFound().json(error_response))
        }
        Ok(Err(error)) => {
            error!("Failed to retrieve report: {}", error);
            let error_response = ErrorResponse::new(&error, "REPORT_RETRIEVAL_FAILED");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn apply_inferences(
    state: web::Data<AppState>,
    req: web::Json<ApplyInferencesRequest>,
) -> impl Responder {
    info!("Applying inferences to {} triples", req.rdf_triples.len());

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let start_time = std::time::Instant::now();

    let triples: Vec<RdfTriple> = req
        .rdf_triples
        .iter()
        .map(|dto| RdfTriple::from(dto.clone()))
        .collect();

    let apply_msg = ApplyInferences {
        rdf_triples: triples,
        max_depth: req.max_depth,
    };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(apply_msg).await {
        Ok(Ok(inferred_triples)) => {
            let processing_time_ms = start_time.elapsed().as_millis() as u64;

            let response = InferenceResult {
                input_count: req.rdf_triples.len(),
                inferred_triples: inferred_triples
                    .into_iter()
                    .map(RdfTripleDto::from)
                    .collect(),
                processing_time_ms,
                graph_updated: req.update_graph.unwrap_or(false),
            };

            info!(
                "Generated {} inferred triples",
                response.inferred_triples.len()
            );
            ok_json!(response)
        }
        Ok(Err(error)) => {
            error!("Failed to apply inferences: {}", error);
            let error_response = ErrorResponse::new(&error, "INFERENCE_FAILED");
            Ok(HttpResponse::BadRequest().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn get_health_status(state: web::Data<AppState>) -> impl Responder {
    info!("Retrieving ontology system health");

    let health_msg = GetOntologyHealth;

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(health_msg).await {
        Ok(Ok(health)) => {
            let response = HealthStatusResponse {
                status: if health.validation_queue_size > 100 {
                    "degraded"
                } else {
                    "healthy"
                }
                .to_string(),
                health: OntologyHealthDto::from(health),
                ontology_validation_enabled: true,
                timestamp: Utc::now(),
            };

            ok_json!(response)
        }
        Ok(Err(error)) => {
            error!("Failed to retrieve health status: {}", error);
            let error_response = ErrorResponse::new(&error, "HEALTH_CHECK_FAILED");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn clear_caches(state: web::Data<AppState>) -> impl Responder {
    info!("Clearing ontology caches");

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let clear_msg = ClearOntologyCaches;

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(clear_msg).await {
        Ok(Ok(())) => {
            info!("Successfully cleared ontology caches");
            ok_json!(serde_json::json!({
                "status": "success",
                "message": "All caches cleared",
                "timestamp": Utc::now()
            }))
        }
        Ok(Err(error)) => {
            error!("Failed to clear caches: {}", error);
            let error_response = ErrorResponse::new(&error, "CACHE_CLEAR_FAILED");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn list_axioms(state: web::Data<AppState>) -> impl Responder {
    info!("Listing all loaded axioms");

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    use crate::actors::messages::GetCachedOntologies;
    let list_msg = GetCachedOntologies;

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(list_msg).await {
        Ok(Ok(ontologies)) => {
            info!("Retrieved {} loaded ontologies", ontologies.len());
            ok_json!(serde_json::json!({
                "axioms": ontologies,
                "count": ontologies.len(),
                "timestamp": Utc::now()
            }))
        }
        Ok(Err(error)) => {
            error!("Failed to list axioms: {}", error);
            let error_response = ErrorResponse::new(&error, "AXIOM_LIST_FAILED");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn get_inferences(
    state: web::Data<AppState>,
    query: web::Query<HashMap<String, String>>,
) -> impl Responder {
    info!("Retrieving inferred relationships");

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let ontology_id = query.get("ontology_id").cloned();

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    let report_msg = GetOntologyReport {
        report_id: ontology_id,
    };

    match ontology_addr.send(report_msg).await {
        Ok(Ok(Some(report))) => {
            info!("Retrieved inferences from report: {}", report.id);

            let inferences = serde_json::json!({
                "report_id": report.id,
                "inferred_count": report.inferred_triples.len(),
                "inferences": report.inferred_triples,
                "generated_at": report.timestamp,
                "inference_depth": 3,
                "timestamp": Utc::now()
            });

            ok_json!(inferences)
        }
        Ok(Ok(None)) => {
            warn!("No validation report found for inference retrieval");
            ok_json!(serde_json::json!({
                "inferred_count": 0,
                "inferences": [],
                "message": "No inferences available. Run validation first.",
                "timestamp": Utc::now()
            }))
        }
        Ok(Err(error)) => {
            error!("Failed to retrieve inferences: {}", error);
            let error_response = ErrorResponse::new(&error, "INFERENCE_RETRIEVAL_FAILED");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn validate_graph(
    state: web::Data<AppState>,
    req: web::Json<ValidationRequest>,
) -> impl Responder {
    info!(
        "Triggering validation job for ontology: {} (mode: {:?})",
        req.ontology_id, req.mode
    );

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let property_graph = match extract_property_graph(&state).await {
        Ok(graph) => graph,
        Err(error) => {
            return Ok::<HttpResponse, actix_web::Error>(
                HttpResponse::InternalServerError().json(error),
            )
        }
    };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    // Mint the job id here and hand it to the actor so the report it computes is
    // cached under this exact id (and the ontology id). The actor owns the single
    // report store: it registers a "pending" report on enqueue and overwrites it
    // with the finished report on completion — so the report is always
    // retrievable via GET /report by jobId or ontologyId. This replaces the old
    // fire-and-forget spawn whose awaited report was dropped on the floor.
    let job_id = Uuid::new_v4().to_string();
    let validation_msg = ValidateOntology {
        ontology_id: req.ontology_id.clone(),
        graph_data: property_graph,
        mode: ValidationMode::from(req.mode.clone()),
        job_id: Some(job_id.clone()),
    };

    match ontology_addr.send(validation_msg).await {
        Ok(Ok(_pending_report)) => {
            let response = ValidationResponse {
                job_id: job_id.clone(),
                status: "queued".to_string(),
                estimated_completion: Some(Utc::now() + chrono::Duration::seconds(30)),
                queue_position: Some(1),
                websocket_url: req
                    .client_id
                    .as_ref()
                    .map(|id| format!("/api/ontology/ws?client_id={}", id)),
            };

            info!("Validation job queued with ID: {}", response.job_id);
            accepted!(response)
        }
        Ok(Err(error)) => {
            error!("Failed to enqueue validation job {}: {}", job_id, error);
            let error_response = ErrorResponse::new(&error, "VALIDATION_FAILED");
            Ok(HttpResponse::BadRequest().json(error_response))
        }
        Err(mailbox_error) => {
            error!(
                "Actor communication error for job {}: {}",
                job_id, mailbox_error
            );
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

/// Get Ontology Class Hierarchy
/// Returns the complete class hierarchy for the ontology with parent-child relationships,
/// depth information, and descendant counts.
/// # OpenAPI Specification
/// **GET** `/api/ontology/hierarchy`
/// ## Query Parameters
/// - `ontology_id` (optional): Specific ontology identifier. Defaults to "default"
/// - `max_depth` (optional): Maximum depth to traverse. No limit if not specified
/// ## Response Schema (200 OK)
/// ```json
/// {
///   "rootClasses": ["http://example.org/Class1", "http://example.org/Class2"],
///   "hierarchy": {
///     "http://example.org/Class1": {
///       "iri": "http://example.org/Class1",
///       "label": "Person",
///       "parentIri": null,
///       "childrenIris": ["http://example.org/Student", "http://example.org/Teacher"],
///       "nodeCount": 5,
///       "depth": 0
///     },
///     "http://example.org/Student": {
///       "iri": "http://example.org/Student",
///       "label": "Student",
///       "parentIri": "http://example.org/Class1",
///       "childrenIris": ["http://example.org/GraduateStudent"],
///       "nodeCount": 2,
///       "depth": 1
///     }
///   }
/// }
/// ```
/// ## Response Fields
/// - `rootClasses`: Array of IRIs representing top-level classes (no parents)
/// - `hierarchy`: Map of class IRI to ClassNode objects containing:
///   - `iri`: The class IRI
///   - `label`: Human-readable label (extracted from IRI if not available)
///   - `parentIri`: IRI of the first parent class (null for root classes)
///   - `childrenIris`: Array of child class IRIs
///   - `nodeCount`: Total number of descendants (children + grandchildren + ...)
///   - `depth`: Distance from nearest root class (0 for roots)
/// ## Error Responses
/// - `503 Service Unavailable`: Ontology validation feature is disabled
/// - `500 Internal Server Error`: Failed to build hierarchy
/// ## Example Request
/// ```bash
/// curl -X GET "http://localhost:8080/api/ontology/hierarchy?ontology_id=default&max_depth=5"
/// ```
/// ## Caching
/// Results are computed on-demand. For large ontologies, consider caching the response
/// on the client side or implementing server-side caching.
/// ## Performance Notes
/// - Time complexity: O(n) where n is the number of classes
/// - Space complexity: O(n)
/// - Memoization is used for depth and descendant count calculations
pub async fn get_hierarchy(
    state: web::Data<AppState>,
    _query: web::Query<HierarchyParams>,
) -> impl Responder {
    info!("Retrieving ontology class hierarchy");

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    // Call the async repository directly. The sync ListOwlClassesHandler wraps
    // this same call in `tokio::runtime::Handle::current().block_on` (queries.rs:97),
    // which panics with "Cannot start a runtime from within a runtime" when invoked
    // from this async actix worker — crashing the endpoint with an empty reply.
    // Going async-direct avoids the sync-over-async bridge entirely.
    match state.ontology_repository.list_owl_classes().await {
        Ok(classes) => {
            info!("Building class hierarchy from {} classes", classes.len());

            let mut hierarchy_map: HashMap<String, ClassNode> = HashMap::new();
            let mut root_classes: Vec<String> = Vec::new();
            let mut children_map: HashMap<String, Vec<String>> = HashMap::new();

            for class in &classes {
                if class.parent_classes.is_empty() {
                    root_classes.push(class.iri.clone());
                }

                for parent_iri in &class.parent_classes {
                    children_map
                        .entry(parent_iri.clone())
                        .or_default()
                        .push(class.iri.clone());
                }
            }

            fn calculate_depth(
                iri: &str,
                classes: &[crate::ports::ontology_repository::OwlClass],
                memo: &mut HashMap<String, usize>,
                visiting: &mut std::collections::HashSet<String>,
            ) -> usize {
                if let Some(&depth) = memo.get(iri) {
                    return depth;
                }

                // Cycle detection: if already visiting this node, break the cycle
                if !visiting.insert(iri.to_string()) {
                    return 0;
                }

                let class = classes.iter().find(|c| c.iri == iri);
                let depth = if let Some(c) = class {
                    if c.parent_classes.is_empty() {
                        0
                    } else {
                        c.parent_classes
                            .iter()
                            .map(|p| calculate_depth(p, classes, memo, visiting) + 1)
                            .max()
                            .unwrap_or(0)
                    }
                } else {
                    0
                };

                visiting.remove(iri);
                memo.insert(iri.to_string(), depth);
                depth
            }

            fn count_descendants(
                iri: &str,
                children_map: &HashMap<String, Vec<String>>,
                memo: &mut HashMap<String, usize>,
                visiting: &mut std::collections::HashSet<String>,
            ) -> usize {
                if let Some(&count) = memo.get(iri) {
                    return count;
                }

                // Cycle detection: if already visiting this node, break the cycle
                if !visiting.insert(iri.to_string()) {
                    return 0;
                }

                let count = if let Some(children) = children_map.get(iri) {
                    children.len()
                        + children
                            .iter()
                            .map(|child| count_descendants(child, children_map, memo, visiting))
                            .sum::<usize>()
                } else {
                    0
                };

                visiting.remove(iri);
                memo.insert(iri.to_string(), count);
                count
            }

            let mut depth_memo: HashMap<String, usize> = HashMap::new();
            let mut count_memo: HashMap<String, usize> = HashMap::new();
            let mut depth_visiting = std::collections::HashSet::new();
            let mut count_visiting = std::collections::HashSet::new();

            for class in &classes {
                let depth =
                    calculate_depth(&class.iri, &classes, &mut depth_memo, &mut depth_visiting);
                let node_count = count_descendants(
                    &class.iri,
                    &children_map,
                    &mut count_memo,
                    &mut count_visiting,
                );
                let children_iris = children_map.get(&class.iri).cloned().unwrap_or_default();

                let parent_iri = if class.parent_classes.is_empty() {
                    None
                } else {
                    class.parent_classes.first().cloned()
                };

                let node = ClassNode {
                    iri: class.iri.clone(),
                    label: class.label.clone().unwrap_or_else(|| {
                        class
                            .iri
                            .split('#')
                            .next_back()
                            .or_else(|| class.iri.split('/').next_back())
                            .unwrap_or(&class.iri)
                            .to_string()
                    }),
                    parent_iri,
                    children_iris,
                    node_count,
                    depth,
                };

                hierarchy_map.insert(class.iri.clone(), node);
            }

            let response = ClassHierarchy {
                root_classes,
                hierarchy: hierarchy_map,
            };

            info!(
                "Class hierarchy built successfully: {} root classes, {} total classes",
                response.root_classes.len(),
                response.hierarchy.len()
            );

            ok_json!(response)
        }
        Err(e) => {
            error!("Failed to retrieve classes for hierarchy: {}", e);
            let error_response = ErrorResponse::new(&e.to_string(), "HIERARCHY_BUILD_FAILED");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

pub async fn get_report_by_id(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let report_id = path.into_inner();
    info!("Retrieving validation report by ID: {}", report_id);

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let report_msg = GetOntologyReport {
        report_id: Some(report_id.clone()),
    };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    match ontology_addr.send(report_msg).await {
        Ok(Ok(Some(report))) => {
            if report.graph_signature == "pending" {
                info!("Validation report {} still pending", report.id);
                accepted!(report)
            } else {
                info!("Retrieved validation report: {}", report.id);
                ok_json!(report)
            }
        }
        Ok(Ok(None)) => {
            warn!("Validation report not found: {}", report_id);
            let error_response = ErrorResponse::new("Report not found", "REPORT_NOT_FOUND");
            Ok(HttpResponse::NotFound().json(error_response))
        }
        Ok(Err(error)) => {
            error!("Failed to retrieve report: {}", error);
            let error_response = ErrorResponse::new(&error, "REPORT_RETRIEVAL_FAILED");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
        Err(mailbox_error) => {
            error!("Actor communication error: {}", mailbox_error);
            let error_response = ErrorResponse::new("Internal server error", "ACTOR_ERROR");
            Ok(HttpResponse::InternalServerError().json(error_response))
        }
    }
}

// ============================================================================
// WEBSOCKET IMPLEMENTATION
// ============================================================================

#[allow(dead_code)]
pub struct OntologyWebSocket {
    client_id: String,

    ontology_addr: Addr<OntologyActor>,
}

impl OntologyWebSocket {
    pub fn new(client_id: String, ontology_addr: Addr<OntologyActor>) -> Self {
        Self {
            client_id,
            ontology_addr,
        }
    }
}

impl actix::Actor for OntologyWebSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(
            "WebSocket connection started for client: {}",
            self.client_id
        );

        let msg = serde_json::json!({
            "type": "connection_established",
            "client_id": self.client_id,
            "timestamp": Utc::now()
        });
        ctx.text(msg.to_string());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "WebSocket connection stopped for client: {}",
            self.client_id
        );
    }
}

impl actix::StreamHandler<Result<ws::Message, ws::ProtocolError>> for OntologyWebSocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                debug!(
                    "Received WebSocket message from {}: {}",
                    self.client_id, text
                );

                let response = serde_json::json!({
                    "type": "echo",
                    "original": &*text,
                    "timestamp": Utc::now()
                });
                ctx.text(response.to_string());
            }
            Ok(ws::Message::Ping(msg)) => {
                ctx.pong(&msg);
            }
            Ok(ws::Message::Close(reason)) => {
                info!(
                    "WebSocket close received from {}: {:?}",
                    self.client_id, reason
                );
                ctx.close(reason);
            }
            _ => {}
        }
    }
}

pub async fn websocket_handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, ActixError> {
    info!("WebSocket upgrade request for ontology updates");

    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error),
        );
    }

    let client_id = query
        .get("client_id")
        .cloned()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(
            HttpResponse::ServiceUnavailable().json(error_response),
        );
    };

    let websocket = OntologyWebSocket::new(client_id, ontology_addr.clone());

    ws::start(websocket, &req, stream)
}

// ============================================================================
// BI-TEMPORAL STATE-AT (ADR-049)
// ============================================================================

/// Query parameters for [`state_at`]: `t` (required, RFC3339 valid-time point)
/// and optional `recorded_as_of` (RFC3339 point-in-time-of-knowledge).
#[derive(Debug, Clone, Deserialize)]
pub struct StateAtParams {
    /// Valid-time point (RFC3339). The projection returns assertion-versions
    /// whose half-open `[validFrom, validTo)` interval contains this instant.
    pub t: Option<String>,
    /// Optional recorded-time bound (RFC3339). When present, only versions
    /// `generatedAt <= recorded_as_of` are returned (as-of-knowledge view).
    pub recorded_as_of: Option<String>,
}

/// Canonical UTC RFC-3339, second precision, `Z` suffix — the timestamp form
/// `provenance_writer` writes into the `dl:validFrom`/`dl:validTo`/
/// `prov:generatedAtTime` `xsd:dateTime` literals. Using the same form for the
/// query literals keeps the comparison boundary aligned with stored values.
fn state_at_iso_utc(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Parse an RFC3339 timestamp into a UTC instant, returning the offending value
/// on failure so the caller can emit a precise 400.
fn parse_rfc3339_utc(raw: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("'{raw}' is not a valid RFC3339 timestamp: {e}"))
}

/// Build the bi-temporal `state_at(t)` SPARQL SELECT over
/// `urn:ngm:graph:provenance` (ADR-049 portable reification, PRD-022 WS-2 unified).
///
/// Joins the reified statement (`rdf:subject`/`rdf:predicate`/`rdf:object`), the
/// valid-time start (`dl:validFrom`), recorded time (`prov:generatedAtTime`), the
/// generating activity (`prov:wasGeneratedBy`) and the attributed agent
/// (`prov:wasAttributedTo`), with an OPTIONAL `dl:validTo`.
///
/// The valid-time filter is the half-open interval predicate — start inclusive
/// (`?validFrom <= t`), end exclusive (`!BOUND(?validTo) || t < ?validTo`) —
/// byte-matching `provenance_writer::state_at` semantics. When `recorded_as_of`
/// is `Some`, an additional `?generatedAt <= recorded` conjunct restricts the
/// result to the point-in-time-of-knowledge view. `t`/`recorded` are bound as
/// `xsd:dateTime` literals in canonical UTC form.
pub fn build_state_at_sparql(t: DateTime<Utc>, recorded_as_of: Option<DateTime<Utc>>) -> String {
    let t_lit = state_at_iso_utc(&t);
    let recorded_clause = match recorded_as_of {
        Some(r) => format!(
            "\n             && ?generatedAt <= \"{}\"^^xsd:dateTime",
            state_at_iso_utc(&r)
        ),
        None => String::new(),
    };
    format!(
        r#"PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX prov: <http://www.w3.org/ns/prov#>
PREFIX dl: <{DL_NS}>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
SELECT ?subject ?predicate ?object ?validFrom ?validTo ?generatedAt ?activity ?agent
WHERE {{
  GRAPH <{GRAPH_PROVENANCE}> {{
    ?entity a prov:Entity ;
            rdf:subject ?subject ;
            rdf:predicate ?predicate ;
            rdf:object ?object ;
            dl:validFrom ?validFrom ;
            prov:generatedAtTime ?generatedAt ;
            prov:wasGeneratedBy ?activity ;
            prov:wasAttributedTo ?agent .
    OPTIONAL {{ ?entity dl:validTo ?validTo }}
    FILTER ( ?validFrom <= "{t_lit}"^^xsd:dateTime
             && ( !BOUND(?validTo) || "{t_lit}"^^xsd:dateTime < ?validTo ){recorded_clause} )
  }}
}}"#,
        DL_NS = crate::services::provenance_writer::DL_NS,
        GRAPH_PROVENANCE = crate::services::provenance_writer::GRAPH_PROVENANCE,
    )
}

/// Pull the lexical `value` of a SPARQL-JSON binding cell (`{type, value, ...}`).
fn binding_value<'a>(row: &'a serde_json::Value, var: &str) -> Option<&'a str> {
    row.get(var)
        .and_then(|cell| cell.get("value"))
        .and_then(|v| v.as_str())
}

/// Map a `sparql_select_json` result document into the `assertions[]` contract
/// shape. Rows missing any required binding are skipped (fail-graceful); an
/// absent `?validTo` becomes JSON `null` (open interval).
fn map_state_at_rows(sparql_json: &serde_json::Value) -> Vec<serde_json::Value> {
    let bindings = sparql_json
        .get("results")
        .and_then(|r| r.get("bindings"))
        .and_then(|b| b.as_array());
    let Some(bindings) = bindings else {
        return Vec::new();
    };
    bindings
        .iter()
        .filter_map(|row| {
            let subject = binding_value(row, "subject")?;
            let predicate = binding_value(row, "predicate")?;
            let object = binding_value(row, "object")?;
            let valid_from = binding_value(row, "validFrom")?;
            let generated_at = binding_value(row, "generatedAt")?;
            let activity = binding_value(row, "activity")?;
            let agent = binding_value(row, "agent")?;
            let valid_to = match binding_value(row, "validTo") {
                Some(v) => serde_json::Value::from(v),
                None => serde_json::Value::Null,
            };
            Some(serde_json::json!({
                "subject": subject,
                "predicate": predicate,
                "object": object,
                "validFrom": valid_from,
                "validTo": valid_to,
                "generatedAt": generated_at,
                "activityUrn": activity,
                "agentIri": agent,
            }))
        })
        .collect()
}

/// `GET /api/ontology/state-at?t=<RFC3339>[&recorded_as_of=<RFC3339>]`
///
/// Bi-temporal valid-time projection (ADR-049): returns the assertion-version
/// entities in `urn:ngm:graph:provenance` whose half-open valid-time
/// interval contains `t` (start inclusive, end exclusive). With
/// `recorded_as_of`, additionally restricts to `generatedAt <= recorded_as_of`
/// (point-in-time-of-knowledge). This is the TEMPORAL subset (runtime governed
/// writes only) — atemporal corpus-bulk classes have no `dl:validFrom` and are
/// deliberately excluded; the client overlays this on the corpus backdrop.
///
/// Read-auth mirrors `/api/ontology/query` (the `AuthenticatedUser` extractor;
/// dev bypass = `Bearer dev-session-token` + `X-Nostr-Pubkey`). Fails graceful:
/// an empty provenance graph yields `assertions: []`, `count: 0`.
pub async fn state_at(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    params: web::Query<StateAtParams>,
) -> Result<HttpResponse, actix_web::Error> {
    let params = params.into_inner();

    let Some(ref t_raw) = params.t else {
        return crate::bad_request!("Missing required query parameter 't' (RFC3339)");
    };
    let t = match parse_rfc3339_utc(t_raw) {
        Ok(t) => t,
        Err(reason) => return crate::bad_request!("Invalid 't' parameter", reason),
    };
    let recorded_as_of = match params.recorded_as_of {
        Some(ref raw) => match parse_rfc3339_utc(raw) {
            Ok(r) => Some(r),
            Err(reason) => {
                return crate::bad_request!("Invalid 'recorded_as_of' parameter", reason)
            }
        },
        None => None,
    };

    let query = build_state_at_sparql(t, recorded_as_of);
    info!(
        "state_at(t={}, recorded_as_of={:?}) over provenance graph",
        state_at_iso_utc(&t),
        recorded_as_of.map(|r| state_at_iso_utc(&r))
    );

    match state.ontology_repository.sparql_select_json(query).await {
        Ok(sparql_json) => {
            let assertions = map_state_at_rows(&sparql_json);
            let count = assertions.len();
            ok_json!(serde_json::json!({
                "t": state_at_iso_utc(&t),
                "assertions": assertions,
                "count": count,
            }))
        }
        Err(e) => {
            error!("state_at SPARQL query failed: {}", e);
            crate::error_json!("Failed to compute state_at projection", e.to_string())
        }
    }
}

/// Query parameters for [`get_provenance`].
#[derive(Debug, serde::Deserialize)]
pub struct ProvenanceParams {
    /// The entity URN/IRI to resolve provenance for (required).
    pub entity: Option<String>,
    /// Max `prov:wasDerivedFrom` hops to walk (default 16, hard-capped 64).
    pub depth: Option<usize>,
}

/// `GET /api/ontology/provenance?entity=<urn>[&depth=<n>]`
///
/// Answers "provenance for entity X" (PRD-022 WS-2): walks the PROV-O
/// `wasGeneratedBy` / `wasDerivedFrom` chain in the unified
/// `urn:ngm:graph:provenance` ledger — seeding through `rdf:subject` so a
/// governed statement subject (a class or decision URN) resolves to its reified
/// assertion-version provenance — and returns `{ root, nodes, count }`. One
/// query surfaces inference, mutation AND decision provenance now that all three
/// paths share the one ledger. Read-auth mirrors `/query` / `/state-at`.
/// Fails graceful: an entity with no provenance yields `nodes: []`, `count: 0`.
pub async fn get_provenance(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    params: web::Query<ProvenanceParams>,
) -> Result<HttpResponse, actix_web::Error> {
    let params = params.into_inner();
    let Some(entity) = params.entity.filter(|e| !e.trim().is_empty()) else {
        return crate::bad_request!("Missing required query parameter 'entity' (URN/IRI)");
    };
    let depth = params.depth.unwrap_or(16).min(64);

    info!("provenance chain requested for entity={entity} (depth={depth})");
    match state
        .ontology_repository
        .provenance_for_entity(entity.clone(), depth)
        .await
    {
        Ok(chain) => ok_json!(serde_json::json!({
            "root": chain.root,
            "nodes": chain.nodes,
            "count": chain.nodes.len(),
        })),
        Err(e) => {
            error!("provenance chain for {entity} failed: {e}");
            crate::error_json!("Failed to resolve provenance chain", e.to_string())
        }
    }
}

// ============================================================================
// ROUTE CONFIGURATION
// ============================================================================

/// SECURITY: All ontology endpoints require authentication
/// WS-0: SINGLE canonical `/ontology` scope.
///
/// Previously TWO `web::scope("/ontology")` were mounted under `/api`:
///   1. `ontology_handler::config` — `power_user().mutations_only()`, owned the
///      schema-rewrite mutators + `/query` + `/sparql` + the read GETs.
///   2. this `ontology::config` — `authenticated()`, owned `/load`,
///      `/load-axioms`, `/validate` (POST), `/apply`, `/mapping`, `/cache`,
///      `/report(s)`, `/inferences`, `/health`, `/ws`.
///
/// actix-web matches by registration order and does NOT fall through duplicate
/// prefixes, so scope-2's `/axioms` GET + `/hierarchy` GET were dead-shadowed
/// while its unique writes (`/load`, `/load-axioms`, …) ran at the *weaker*
/// `authenticated()` gate. We collapse both into ONE scope wrapped
/// `power_user().mutations_only()`:
///   * every safe GET stays public (mutations_only bypasses GET),
///   * every POST/PUT/DELETE — including `/load` + `/load-axioms` axiom ingest —
///     is now gated at power_user (was `authenticated()`).
///
/// The read/mutate handlers are imported from `ontology_handler`; the GET-only
/// `validate_ontology` is aliased to avoid clashing with the local POST
/// `validate_graph` (`/validate` is registered for BOTH methods).
pub fn config(cfg: &mut web::ServiceConfig) {
    use crate::handlers::ontology_handler::{
        add_axiom, add_owl_class, add_owl_property, get_class_axioms, get_inference_results,
        get_inferred_graph, get_ontology_graph, get_ontology_metrics, get_owl_class,
        get_owl_property, list_owl_classes, list_owl_properties, query_ontology, remove_axiom,
        remove_owl_class, save_ontology_graph, sparql_query, store_inference_results,
        update_owl_class, update_owl_property, validate_ontology as validate_ontology_ro,
    };
    use crate::middleware::RequireAuth;

    cfg.service(
        web::scope("/ontology")
            // WS-0: every state-changing op (POST/PUT/DELETE) gated at power_user;
            // safe GET reads remain public (mutations_only bypasses GET).
            .wrap(RequireAuth::power_user().mutations_only())
            // ---- Mutating / SPARQL ops (power_user via mutations_only) ----
            .route("/graph", web::post().to(save_ontology_graph))
            .route("/classes", web::post().to(add_owl_class))
            .route("/classes/{iri}", web::put().to(update_owl_class))
            .route("/classes/{iri}", web::delete().to(remove_owl_class))
            .route("/properties", web::post().to(add_owl_property))
            .route("/properties/{iri}", web::put().to(update_owl_property))
            .route("/axioms", web::post().to(add_axiom))
            .route("/axioms/{id}", web::delete().to(remove_axiom))
            .route("/inference", web::post().to(store_inference_results))
            .route("/query", web::post().to(query_ontology))
            // WS-4: read-only SPARQL passthrough (POST body → power_user-gated as
            // defence in depth; also validated read-only before reaching Oxigraph).
            .route("/sparql", web::post().to(sparql_query))
            // Axiom ingest — now power_user (was authenticated()). WS-0 hardening.
            .route("/load", web::post().to(load_axioms))
            .route("/load-axioms", web::post().to(load_axioms))
            // Reasoner / mapping / cache mutators — power_user.
            .route("/validate", web::post().to(validate_graph))
            .route("/mapping", web::post().to(update_mapping))
            .route("/apply", web::post().to(apply_inferences))
            .route("/cache", web::delete().to(clear_caches))
            // ---- Read-only inspection — public (safe GET bypasses auth) ----
            .route("/graph", web::get().to(get_ontology_graph))
            .route("/inferred", web::get().to(get_inferred_graph))
            .route("/classes", web::get().to(list_owl_classes))
            .route("/classes/{iri}", web::get().to(get_owl_class))
            .route("/classes/{iri}/axioms", web::get().to(get_class_axioms))
            .route("/properties", web::get().to(list_owl_properties))
            .route("/properties/{iri}", web::get().to(get_owl_property))
            .route("/inference", web::get().to(get_inference_results))
            // /validate served on BOTH methods (GET read-only report; POST graph).
            .route("/validate", web::get().to(validate_ontology_ro))
            .route("/metrics", web::get().to(get_ontology_metrics))
            // /axioms served on BOTH methods (POST add; GET list).
            .route("/axioms", web::get().to(list_axioms))
            .route("/inferences", web::get().to(get_inferences))
            .route("/hierarchy", web::get().to(get_hierarchy))
            // ADR-049 bi-temporal valid-time projection (read; authed via the
            // AuthenticatedUser extractor on the handler, like /query).
            .route("/state-at", web::get().to(state_at))
            // PRD-022 WS-2 "provenance for entity X" — walks wasGeneratedBy /
            // wasDerivedFrom in the unified provenance ledger (read; authed via
            // the AuthenticatedUser extractor on the handler, like /query).
            .route("/provenance", web::get().to(get_provenance))
            .route("/reports/{id}", web::get().to(get_report_by_id))
            .route("/report", web::get().to(get_validation_report))
            .route("/health", web::get().to(get_health_status))
            .route("/ws", web::get().to(websocket_handler)),
    );
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[actix_web::test]
    async fn test_health_endpoint_structure() {
        let health = OntologyHealthDto {
            loaded_ontologies: 5,
            cached_reports: 10,
            validation_queue_size: 2,
            last_validation: Some(Utc::now()),
            cache_hit_rate: 0.85,
            avg_validation_time_ms: 1500.0,
            active_jobs: 1,
            memory_usage_mb: 256.0,
        };

        let response = HealthStatusResponse {
            status: "healthy".to_string(),
            health,
            ontology_validation_enabled: true,
            timestamp: Utc::now(),
        };

        let json =
            serde_json::to_value(&response).expect("HealthStatusResponse should serialize to JSON");
        assert!(json.get("status").is_some());
        assert!(json.get("health").is_some());
        assert!(json.get("ontologyValidationEnabled").is_some());
    }

    #[tokio::test]
    async fn test_validation_config_conversion() {
        let dto = ValidationConfigDto {
            enable_reasoning: Some(true),
            reasoning_timeout_seconds: Some(60),
            enable_inference: Some(false),
            max_inference_depth: Some(5),
            enable_caching: Some(true),
            cache_ttl_seconds: Some(7200),
            validate_cardinality: Some(true),
            validate_domains_ranges: Some(true),
            validate_disjoint_classes: Some(false),
        };

        let config = ValidationConfig::from(dto);
        assert!(config.enable_reasoning);
        assert_eq!(config.reasoning_timeout_seconds, 60);
        assert!(!config.enable_inference);
        assert_eq!(config.max_inference_depth, 5);
    }

    #[tokio::test]
    async fn test_rdf_triple_conversion() {
        let dto = RdfTripleDto {
            subject: "http://example.org/subject".to_string(),
            predicate: "http://example.org/predicate".to_string(),
            object: "http://example.org/object".to_string(),
            is_literal: Some(false),
            datatype: Some("uri".to_string()),
            language: None,
        };

        let triple = RdfTriple::from(dto.clone());
        let back_to_dto = RdfTripleDto::from(triple);

        assert_eq!(dto.subject, back_to_dto.subject);
        assert_eq!(dto.predicate, back_to_dto.predicate);
        assert_eq!(dto.object, back_to_dto.object);
        assert_eq!(dto.is_literal, back_to_dto.is_literal);
        assert_eq!(dto.datatype, back_to_dto.datatype);
        assert_eq!(dto.language, back_to_dto.language);
    }

    // ── state_at: RFC3339 param parsing ─────────────────────────────────────

    #[::core::prelude::v1::test]
    fn parse_rfc3339_accepts_z_and_offset_forms() {
        // `Z` UTC form.
        let z = parse_rfc3339_utc("2026-08-07T00:00:00Z").expect("Z form parses");
        assert_eq!(state_at_iso_utc(&z), "2026-08-07T00:00:00Z");
        // Explicit offset is normalised to UTC.
        let off = parse_rfc3339_utc("2026-08-07T02:00:00+02:00").expect("offset form parses");
        assert_eq!(state_at_iso_utc(&off), "2026-08-07T00:00:00Z");
        // Sub-second precision truncates to canonical second form.
        let ms = parse_rfc3339_utc("2026-08-07T00:00:00.750Z").expect("ms form parses");
        assert_eq!(state_at_iso_utc(&ms), "2026-08-07T00:00:00Z");
    }

    #[::core::prelude::v1::test]
    fn parse_rfc3339_rejects_malformed() {
        assert!(parse_rfc3339_utc("not-a-timestamp").is_err());
        assert!(
            parse_rfc3339_utc("2026-08-07").is_err(),
            "date-only is not RFC3339 datetime"
        );
        assert!(parse_rfc3339_utc("").is_err());
    }

    // ── state_at: SPARQL builder (interval + recorded_as_of branches) ───────

    fn ts(raw: &str) -> DateTime<Utc> {
        parse_rfc3339_utc(raw).expect("valid ts")
    }

    #[::core::prelude::v1::test]
    fn build_state_at_sparql_is_bounded_half_open_read() {
        let q = build_state_at_sparql(ts("2026-08-07T00:00:00Z"), None);
        // Scopes to the unified provenance graph (PRD-022 WS-2 reconciliation).
        assert!(
            q.contains("GRAPH <urn:ngm:graph:provenance>"),
            "provenance graph scope"
        );
        // Joins the full portable-reification shape the contract requires.
        for needle in [
            "rdf:subject ?subject",
            "rdf:predicate ?predicate",
            "rdf:object ?object",
            "dl:validFrom ?validFrom",
            "prov:generatedAtTime ?generatedAt",
            "prov:wasGeneratedBy ?activity",
            "prov:wasAttributedTo ?agent",
            "OPTIONAL { ?entity dl:validTo ?validTo }",
        ] {
            assert!(q.contains(needle), "missing clause: {needle}");
        }
        // Half-open interval: start inclusive (<=), end exclusive (<) with unbound tolerance.
        assert!(q.contains(r#"?validFrom <= "2026-08-07T00:00:00Z"^^xsd:dateTime"#));
        assert!(
            q.contains(r#"!BOUND(?validTo) || "2026-08-07T00:00:00Z"^^xsd:dateTime < ?validTo"#)
        );
        // It is a read — never a mutation of the classified graph.
        assert!(q.trim_start().contains("SELECT"));
        assert!(!q.to_uppercase().contains("INSERT"));
        assert!(!q.to_uppercase().contains("DELETE"));
    }

    #[::core::prelude::v1::test]
    fn build_state_at_sparql_omits_recorded_clause_when_none() {
        let q = build_state_at_sparql(ts("2026-08-07T00:00:00Z"), None);
        assert!(
            !q.contains("?generatedAt <="),
            "no recorded-time conjunct without recorded_as_of"
        );
    }

    #[::core::prelude::v1::test]
    fn build_state_at_sparql_adds_recorded_clause_when_some() {
        let q = build_state_at_sparql(ts("2026-08-07T00:00:00Z"), Some(ts("2026-08-06T12:00:00Z")));
        // Recorded-time (point-in-time-of-knowledge) conjunct present and bounded.
        assert!(q.contains(r#"?generatedAt <= "2026-08-06T12:00:00Z"^^xsd:dateTime"#));
        // Valid-time predicate is still present and unchanged.
        assert!(q.contains(r#"?validFrom <= "2026-08-07T00:00:00Z"^^xsd:dateTime"#));
        assert!(!q.to_uppercase().contains("INSERT"));
    }

    // ── state_at: row mapping to the assertions[] contract shape ────────────

    #[::core::prelude::v1::test]
    fn map_state_at_rows_maps_contract_shape_with_open_and_closed_intervals() {
        let doc = serde_json::json!({
            "head": { "vars": ["subject","predicate","object","validFrom","validTo","generatedAt","activity","agent"] },
            "results": { "bindings": [
                {
                    "subject":   { "type": "uri", "value": "urn:s:1" },
                    "predicate": { "type": "uri", "value": "http://www.w3.org/2000/01/rdf-schema#subClassOf" },
                    "object":    { "type": "uri", "value": "urn:o:1" },
                    "validFrom": { "type": "literal", "value": "2026-08-01T00:00:00Z", "datatype": "http://www.w3.org/2001/XMLSchema#dateTime" },
                    "validTo":   { "type": "literal", "value": "2026-09-01T00:00:00Z", "datatype": "http://www.w3.org/2001/XMLSchema#dateTime" },
                    "generatedAt": { "type": "literal", "value": "2026-08-01T00:00:00Z", "datatype": "http://www.w3.org/2001/XMLSchema#dateTime" },
                    "activity":  { "type": "uri", "value": "urn:agentbox:activity:aa:sha256-12-deadbeef0011" },
                    "agent":     { "type": "uri", "value": "did:nostr:aa" }
                },
                {
                    "subject":   { "type": "uri", "value": "urn:s:2" },
                    "predicate": { "type": "uri", "value": "http://www.w3.org/2000/01/rdf-schema#subClassOf" },
                    "object":    { "type": "literal", "value": "a literal object" },
                    "validFrom": { "type": "literal", "value": "2026-08-05T00:00:00Z" },
                    "generatedAt": { "type": "literal", "value": "2026-08-05T00:00:00Z" },
                    "activity":  { "type": "uri", "value": "urn:agentbox:activity:bb:sha256-12-cafebabe0022" },
                    "agent":     { "type": "uri", "value": "did:nostr:bb" }
                }
            ] }
        });
        let out = map_state_at_rows(&doc);
        assert_eq!(out.len(), 2);
        // Closed interval keeps its validTo string.
        assert_eq!(out[0]["subject"], serde_json::json!("urn:s:1"));
        assert_eq!(out[0]["validTo"], serde_json::json!("2026-09-01T00:00:00Z"));
        assert_eq!(
            out[0]["activityUrn"],
            serde_json::json!("urn:agentbox:activity:aa:sha256-12-deadbeef0011")
        );
        assert_eq!(out[0]["agentIri"], serde_json::json!("did:nostr:aa"));
        // Open interval (absent validTo) becomes JSON null; literal object preserved.
        assert_eq!(out[1]["object"], serde_json::json!("a literal object"));
        assert_eq!(out[1]["validTo"], serde_json::Value::Null);
    }

    #[::core::prelude::v1::test]
    fn map_state_at_rows_empty_on_no_data() {
        let empty = serde_json::json!({ "head": { "vars": [] }, "results": { "bindings": [] } });
        assert!(map_state_at_rows(&empty).is_empty());
        // Missing results object → still graceful empty (not a panic).
        let malformed = serde_json::json!({ "head": {} });
        assert!(map_state_at_rows(&malformed).is_empty());
    }

    #[::core::prelude::v1::test]
    fn map_state_at_rows_skips_rows_missing_required_bindings() {
        // A row without an agent binding is dropped (fail-graceful, not a panic).
        let doc = serde_json::json!({
            "results": { "bindings": [
                {
                    "subject":   { "type": "uri", "value": "urn:s:1" },
                    "predicate": { "type": "uri", "value": "urn:p:1" },
                    "object":    { "type": "uri", "value": "urn:o:1" },
                    "validFrom": { "type": "literal", "value": "2026-08-01T00:00:00Z" },
                    "generatedAt": { "type": "literal", "value": "2026-08-01T00:00:00Z" },
                    "activity":  { "type": "uri", "value": "urn:a:1" }
                    // no "agent"
                }
            ] }
        });
        assert!(map_state_at_rows(&doc).is_empty());
    }
}
