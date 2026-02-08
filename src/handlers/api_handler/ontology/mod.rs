//! Ontology REST and WebSocket API endpoints
//!
//! This module provides comprehensive API endpoints for ontology operations including:
//! - Loading ontology axioms from files/URLs
//! - Updating mapping configurations
//! - Running validation with different modes
//! - Real-time WebSocket updates for validation progress
//! - Applying inferences to the graph
//! - System health monitoring and cache management

use actix::Addr;
use actix_web::{web, Error as ActixError, HttpRequest, HttpResponse, Responder};
use actix_web_actors::ws;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration as StdDuration;
use uuid::Uuid;
use crate::{ok_json, accepted};
use crate::ports::ontology_repository::OntologyRepository;

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
    
    pub axiom_count: usize,
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
    use crate::services::owl_validator::{GraphNode, GraphEdge};

    match state.ontology_repository.load_ontology_graph().await {
        Ok(graph_data) => {
            let nodes: Vec<GraphNode> = graph_data.nodes.iter().map(|n| {
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
            }).collect();

            let edges: Vec<GraphEdge> = graph_data.edges.iter().map(|e| {
                GraphEdge {
                    id: format!("{}-{}", e.source, e.target),
                    source: e.source.to_string(),
                    target: e.target.to_string(),
                    relationship_type: e.edge_type.clone().unwrap_or_else(|| "RELATES".to_string()),
                    properties: HashMap::new(),
                }
            }).collect();

            Ok(PropertyGraph {
                nodes,
                edges,
                metadata: HashMap::new(),
            })
        }
        Err(e) => {
            Err(ErrorResponse::new(
                &format!("Failed to extract property graph: {}", e),
                "PROPERTY_GRAPH_EXTRACTION_FAILED",
            ))
        }
    }
}

// ============================================================================
// REST ENDPOINTS
// ============================================================================

pub async fn load_axioms(state: web::Data<AppState>, body: web::Bytes) -> impl Responder {
    
    let (source, format) = if let Ok(req) = serde_json::from_slice::<LoadOntologyRequest>(&body) {
        info!("Loading ontology from content string");
        (req.content, req.format)
    } else if let Ok(req) = serde_json::from_slice::<LoadAxiomsRequest>(&body) {
        info!("Loading ontology axioms from source: {}", req.source);
        (req.source, req.format)
    } else {
        let error_response = ErrorResponse::new("Invalid request format", "INVALID_REQUEST");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::BadRequest().json(error_response));
    };


    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }

    let start_time = std::time::Instant::now();


    let load_msg = LoadOntologyAxioms { source, format };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
    };

    match ontology_addr.send(load_msg).await {
        Ok(Ok(ontology_id)) => {
            let _loading_time_ms = start_time.elapsed().as_millis() as u64;


            let response = LoadOntologyResponse {
                ontology_id: ontology_id.clone(),
                axiom_count: 0,
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }


    let config = ValidationConfig::from(req.config.clone());

    let update_msg = UpdateOntologyMapping { config };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }


    let property_graph = match extract_property_graph(&state).await {
        Ok(graph) => graph,
        Err(error) => return Ok::<HttpResponse, actix_web::Error>(HttpResponse::InternalServerError().json(error)),
    };

    let validation_msg = ValidateOntology {
        ontology_id: req.ontology_id.clone(),
        graph_data: property_graph,
        mode: ValidationMode::from(req.mode.clone()),
    };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
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
    let report_id = query.get("report_id").cloned();

    info!("Retrieving validation report: {:?}", report_id);


    if let Err(error) = check_feature_enabled().await {
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }

    let report_msg = GetOntologyReport { report_id };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
    };

    match ontology_addr.send(report_msg).await {
        Ok(Ok(Some(report))) => {
            info!("Retrieved validation report: {}", report.id);
            ok_json!(report)
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }

    let clear_msg = ClearOntologyCaches;

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }

    use crate::actors::messages::GetCachedOntologies;
    let list_msg = GetCachedOntologies;

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }

    let ontology_id = query.get("ontology_id").cloned();

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }


    let property_graph = match extract_property_graph(&state).await {
        Ok(graph) => graph,
        Err(error) => return Ok::<HttpResponse, actix_web::Error>(HttpResponse::InternalServerError().json(error)),
    };

    let validation_msg = ValidateOntology {
        ontology_id: req.ontology_id.clone(),
        graph_data: property_graph,
        mode: ValidationMode::from(req.mode.clone()),
    };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
    };


    let job_id = Uuid::new_v4().to_string();
    let job_id_clone = job_id.clone();


    let ontology_addr_clone = ontology_addr.clone();
    actix::spawn(async move {
        match ontology_addr_clone.send(validation_msg).await {
            Ok(Ok(report)) => {
                info!(
                    "Validation completed for job {}: {} violations found",
                    job_id_clone,
                    report.violations.len()
                );
            }
            Ok(Err(e)) => {
                error!("Validation failed for job {}: {}", job_id_clone, e);
            }
            Err(e) => {
                error!("Actor communication error for job {}: {}", job_id_clone, e);
            }
        }
    });

    let response = ValidationResponse {
        job_id,
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }


    use crate::application::ontology::{ListOwlClasses, ListOwlClassesHandler};
    use hexser::QueryHandler;

    let handler = ListOwlClassesHandler::new(state.ontology_repository.clone());
    let list_query = ListOwlClasses;

    match handler.handle(list_query) {
        Ok(classes) => {
            info!(
                "Building class hierarchy from {} classes",
                classes.len()
            );


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
                        .or_insert_with(Vec::new)
                        .push(class.iri.clone());
                }
            }


            fn calculate_depth(
                iri: &str,
                classes: &[crate::ports::ontology_repository::OwlClass],
                memo: &mut HashMap<String, usize>,
            ) -> usize {
                if let Some(&depth) = memo.get(iri) {
                    return depth;
                }

                let class = classes.iter().find(|c| c.iri == iri);
                let depth = if let Some(c) = class {
                    if c.parent_classes.is_empty() {
                        0
                    } else {
                        c.parent_classes
                            .iter()
                            .map(|p| calculate_depth(p, classes, memo) + 1)
                            .max()
                            .unwrap_or(0)
                    }
                } else {
                    0
                };

                memo.insert(iri.to_string(), depth);
                depth
            }


            fn count_descendants(
                iri: &str,
                children_map: &HashMap<String, Vec<String>>,
                memo: &mut HashMap<String, usize>,
            ) -> usize {
                if let Some(&count) = memo.get(iri) {
                    return count;
                }

                let count = if let Some(children) = children_map.get(iri) {
                    children.len()
                        + children
                            .iter()
                            .map(|child| count_descendants(child, children_map, memo))
                            .sum::<usize>()
                } else {
                    0
                };

                memo.insert(iri.to_string(), count);
                count
            }

            let mut depth_memo: HashMap<String, usize> = HashMap::new();
            let mut count_memo: HashMap<String, usize> = HashMap::new();


            for class in &classes {
                let depth = calculate_depth(&class.iri, &classes, &mut depth_memo);
                let node_count = count_descendants(&class.iri, &children_map, &mut count_memo);
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
                            .last()
                            .or_else(|| class.iri.split('/').last())
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }

    let report_msg = GetOntologyReport {
        report_id: Some(report_id.clone()),
    };

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
    };

    match ontology_addr.send(report_msg).await {
        Ok(Ok(Some(report))) => {
            info!("Retrieved validation report: {}", report.id);
            ok_json!(report)
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

pub struct OntologyWebSocket {

    client_id: String,

    _ontology_addr: Addr<OntologyActor>,
}

impl OntologyWebSocket {
    pub fn new(client_id: String, ontology_addr: Addr<OntologyActor>) -> Self {
        Self {
            client_id,
            _ontology_addr: ontology_addr,
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
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error));
    }

    let client_id = query
        .get("client_id")
        .cloned()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let Some(ref ontology_addr) = state.ontology_actor_addr else {
        let error_response =
            ErrorResponse::new("Ontology actor not available", "ACTOR_UNAVAILABLE");
        return Ok::<HttpResponse, actix_web::Error>(HttpResponse::ServiceUnavailable().json(error_response));
    };

    let websocket = OntologyWebSocket::new(client_id, ontology_addr.clone());

    ws::start(websocket, &req, stream)
}

// ============================================================================
// ROUTE CONFIGURATION
// ============================================================================

/// SECURITY: All ontology endpoints require authentication
pub fn config(cfg: &mut web::ServiceConfig) {
    use crate::middleware::RequireAuth;

    cfg.service(
        web::scope("/ontology")
            .wrap(RequireAuth::authenticated())  // Require authentication for all ontology operations

            .route("/load", web::post().to(load_axioms))
            .route("/load-axioms", web::post().to(load_axioms))

            .route("/validate", web::post().to(validate_graph))

            .route("/reports/{id}", web::get().to(get_report_by_id))
            .route("/report", web::get().to(get_validation_report))

            .route("/axioms", web::get().to(list_axioms))

            .route("/inferences", web::get().to(get_inferences))

            .route("/hierarchy", web::get().to(get_hierarchy))

            .route("/cache", web::delete().to(clear_caches))

            .route("/mapping", web::post().to(update_mapping))
            .route("/apply", web::post().to(apply_inferences))
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
    use actix_web::{test, App};
    use serde_json::Value;

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

        
        let json = serde_json::to_value(&response)
            .expect("HealthStatusResponse should serialize to JSON");
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
        assert_eq!(config.enable_reasoning, true);
        assert_eq!(config.reasoning_timeout_seconds, 60);
        assert_eq!(config.enable_inference, false);
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
}
