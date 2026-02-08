// CQRS-Based Ontology Handler
// Uses Ontology application layer for all OWL operations

use crate::handlers::utils::execute_in_thread;
use crate::{ok_json, error_json, not_found};
use crate::AppState;
use actix_web::{web, HttpResponse};
use log::{error, info};
use serde::Deserialize;

// Import CQRS handlers
use crate::application::ontology::{
    AddAxiom,
    AddAxiomHandler,
    
    AddOwlClass,
    AddOwlClassHandler,
    AddOwlProperty,
    AddOwlPropertyHandler,
    GetClassAxioms,
    GetClassAxiomsHandler,
    GetInferenceResults,
    GetInferenceResultsHandler,
    GetOntologyMetrics,
    GetOntologyMetricsHandler,
    GetOwlClass,
    GetOwlClassHandler,
    GetOwlProperty,
    GetOwlPropertyHandler,
    ListOwlClasses,
    ListOwlClassesHandler,
    ListOwlProperties,
    ListOwlPropertiesHandler,
    
    LoadOntologyGraph,
    LoadOntologyGraphHandler,
    QueryOntology,
    QueryOntologyHandler,
    RemoveAxiom,
    RemoveAxiomHandler,
    RemoveOwlClass,
    RemoveOwlClassHandler,
    SaveOntologyGraph,
    SaveOntologyGraphHandler,
    StoreInferenceResults,
    StoreInferenceResultsHandler,
    UpdateOwlClass,
    UpdateOwlClassHandler,
    UpdateOwlProperty,
    UpdateOwlPropertyHandler,
    ValidateOntology,
    ValidateOntologyHandler,
};
use crate::models::graph::GraphData;
use crate::ports::ontology_repository::{InferenceResults, OwlAxiom, OwlClass, OwlProperty};
use hexser::{DirectiveHandler, QueryHandler};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddClassRequest {
    pub class: OwlClass,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClassRequest {
    pub class: OwlClass,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddPropertyRequest {
    pub property: OwlProperty,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePropertyRequest {
    pub property: OwlProperty,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddAxiomRequest {
    pub axiom: OwlAxiom,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreInferenceRequest {
    pub results: InferenceResults,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveGraphRequest {
    pub graph: GraphData,
}

pub async fn get_ontology_graph(state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    info!("Getting ontology graph via CQRS query");

    
    let handler = LoadOntologyGraphHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(LoadOntologyGraph)).await;

    
    match result {
        Ok(Ok(graph)) => {
            info!("Ontology graph loaded successfully via CQRS");
            ok_json!(&*graph)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to load ontology graph: {}", e);
            error_json!("Failed to load ontology graph", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error in get_ontology_graph: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn save_ontology_graph(
    state: web::Data<AppState>,
    request: web::Json<SaveGraphRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let graph = request.into_inner().graph;
    info!("Saving ontology graph via CQRS directive");

    
    let handler = SaveOntologyGraphHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(SaveOntologyGraph { graph })).await;

    match result {
        Ok(Ok(())) => {
            info!("Ontology graph saved successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to save ontology graph: {}", e);
            error_json!("Failed to save ontology graph", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn get_owl_class(state: web::Data<AppState>, iri: web::Path<String>) -> Result<HttpResponse, actix_web::Error> {
    let class_iri = iri.into_inner();
    info!("Getting OWL class via CQRS query: iri={}", class_iri);

    
    let handler = GetOwlClassHandler::new(state.ontology_repository.clone());

    
    let iri_clone = class_iri.clone();
    let result = execute_in_thread(move || handler.handle(GetOwlClass { iri: iri_clone })).await;

    match result {
        Ok(Ok(Some(class))) => {
            info!("OWL class found via CQRS: iri={}", class_iri);
            ok_json!(class)
        }
        Ok(Ok(None)) => {
            info!("OWL class not found: iri={}", class_iri);
            not_found!("OWL class not found")
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get OWL class: {}", e);
            error_json!("Failed to get OWL class", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn list_owl_classes(state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    info!("Listing all OWL classes via CQRS query");

    
    let handler = ListOwlClassesHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(ListOwlClasses)).await;

    match result {
        Ok(Ok(classes)) => {
            info!(
                "OWL classes listed successfully via CQRS: {} classes",
                classes.len()
            );
            ok_json!(classes)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to list OWL classes: {}", e);
            error_json!("Failed to list OWL classes", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn add_owl_class(
    state: web::Data<AppState>,
    request: web::Json<AddClassRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let class = request.into_inner().class;
    info!("Adding OWL class via CQRS directive: iri={}", class.iri);

    
    let handler = AddOwlClassHandler::new(state.ontology_repository.clone());

    
    let class_iri = class.iri.clone();
    let result = execute_in_thread(move || handler.handle(AddOwlClass { class })).await;

    match result {
        Ok(Ok(())) => {
            info!("OWL class added successfully via CQRS: iri={}", class_iri);
            ok_json!(serde_json::json!({
                "success": true,
                "iri": class_iri
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to add OWL class: {}", e);
            error_json!("Failed to add OWL class", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn update_owl_class(
    state: web::Data<AppState>,
    request: web::Json<UpdateClassRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let class = request.into_inner().class;
    info!("Updating OWL class via CQRS directive: iri={}", class.iri);

    
    let handler = UpdateOwlClassHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(UpdateOwlClass { class })).await;

    match result {
        Ok(Ok(())) => {
            info!("OWL class updated successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to update OWL class: {}", e);
            error_json!("Failed to update OWL class", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn remove_owl_class(
    state: web::Data<AppState>,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let class_iri = iri.into_inner();
    info!("Removing OWL class via CQRS directive: iri={}", class_iri);

    
    let handler = RemoveOwlClassHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(RemoveOwlClass { iri: class_iri })).await;

    match result {
        Ok(Ok(())) => {
            info!("OWL class removed successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to remove OWL class: {}", e);
            error_json!("Failed to remove OWL class", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn get_owl_property(
    state: web::Data<AppState>,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let property_iri = iri.into_inner();
    info!("Getting OWL property via CQRS query: iri={}", property_iri);

    
    let handler = GetOwlPropertyHandler::new(state.ontology_repository.clone());

    
    match handler.handle(GetOwlProperty {
        iri: property_iri.clone(),
    }) {
        Ok(Some(property)) => {
            info!("OWL property found via CQRS: iri={}", property_iri);
            ok_json!(property)
        }
        Ok(None) => {
            info!("OWL property not found: iri={}", property_iri);
            not_found!("OWL property not found")
        }
        Err(e) => {
            error!("CQRS query failed to get OWL property: {}", e);
            error_json!("Failed to get OWL property", e.to_string())
        }
    }
}

pub async fn list_owl_properties(state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    info!("Listing all OWL properties via CQRS query");

    
    let handler = ListOwlPropertiesHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(ListOwlProperties)).await;

    match result {
        Ok(Ok(properties)) => {
            info!(
                "OWL properties listed successfully via CQRS: {} properties",
                properties.len()
            );
            ok_json!(properties)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to list OWL properties: {}", e);
            error_json!("Failed to list OWL properties", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn add_owl_property(
    state: web::Data<AppState>,
    request: web::Json<AddPropertyRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let property = request.into_inner().property;
    info!(
        "Adding OWL property via CQRS directive: iri={}",
        property.iri
    );

    
    let handler = AddOwlPropertyHandler::new(state.ontology_repository.clone());

    
    let property_iri = property.iri.clone();
    match handler.handle(AddOwlProperty { property }) {
        Ok(()) => {
            info!(
                "OWL property added successfully via CQRS: iri={}",
                property_iri
            );
            ok_json!(serde_json::json!({
                "success": true,
                "iri": property_iri
            }))
        }
        Err(e) => {
            error!("CQRS directive failed to add OWL property: {}", e);
            error_json!("Failed to add OWL property", e.to_string())
        }
    }
}

pub async fn update_owl_property(
    state: web::Data<AppState>,
    request: web::Json<UpdatePropertyRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let property = request.into_inner().property;
    info!(
        "Updating OWL property via CQRS directive: iri={}",
        property.iri
    );

    
    let handler = UpdateOwlPropertyHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(UpdateOwlProperty { property })).await;

    match result {
        Ok(Ok(())) => {
            info!("OWL property updated successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to update OWL property: {}", e);
            error_json!("Failed to update OWL property", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn get_class_axioms(
    state: web::Data<AppState>,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let class_iri = iri.into_inner();
    info!("Getting class axioms via CQRS query: iri={}", class_iri);

    
    let handler = GetClassAxiomsHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(GetClassAxioms { class_iri })).await;

    match result {
        Ok(Ok(axioms)) => {
            info!(
                "Class axioms retrieved successfully via CQRS: {} axioms",
                axioms.len()
            );
            ok_json!(axioms)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get class axioms: {}", e);
            error_json!("Failed to get class axioms", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn add_axiom(
    state: web::Data<AppState>,
    request: web::Json<AddAxiomRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let axiom = request.into_inner().axiom;
    info!(
        "Adding axiom via CQRS directive: type={:?}",
        axiom.axiom_type
    );

    
    let handler = AddAxiomHandler::new(state.ontology_repository.clone());

    
    let axiom_type = format!("{:?}", axiom.axiom_type);
    match handler.handle(AddAxiom { axiom }) {
        Ok(()) => {
            info!("Axiom added successfully via CQRS: type={}", axiom_type);
            ok_json!(serde_json::json!({
                "success": true,
                "message": format!("Axiom of type {} added", axiom_type)
            }))
        }
        Err(e) => {
            error!("CQRS directive failed to add axiom: {}", e);
            error_json!("Failed to add axiom", e.to_string())
        }
    }
}

pub async fn remove_axiom(state: web::Data<AppState>, axiom_id: web::Path<u64>) -> Result<HttpResponse, actix_web::Error> {
    let id = axiom_id.into_inner();
    info!("Removing axiom via CQRS directive: id={}", id);

    
    let handler = RemoveAxiomHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(RemoveAxiom { axiom_id: id })).await;

    match result {
        Ok(Ok(())) => {
            info!("Axiom removed successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to remove axiom: {}", e);
            error_json!("Failed to remove axiom", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn get_inference_results(state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    info!("Getting inference results via CQRS query");

    
    let handler = GetInferenceResultsHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(GetInferenceResults)).await;

    match result {
        Ok(Ok(Some(results))) => {
            info!("Inference results retrieved successfully via CQRS");
            ok_json!(results)
        }
        Ok(Ok(None)) => {
            info!("No inference results found");
            not_found!("No inference results available")
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get inference results: {}", e);
            error_json!("Failed to get inference results", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn store_inference_results(
    state: web::Data<AppState>,
    request: web::Json<StoreInferenceRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let results = request.into_inner().results;
    info!(
        "Storing inference results via CQRS directive: {} axioms",
        results.inferred_axioms.len()
    );

    
    let handler = StoreInferenceResultsHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(StoreInferenceResults { results })).await;

    match result {
        Ok(Ok(())) => {
            info!("Inference results stored successfully via CQRS");
            ok_json!(serde_json::json!({
                "success": true
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to store inference results: {}", e);
            error_json!("Failed to store inference results", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn validate_ontology(state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    info!("Validating ontology via CQRS query");

    
    let handler = ValidateOntologyHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(ValidateOntology)).await;

    match result {
        Ok(Ok(report)) => {
            info!(
                "Ontology validation completed via CQRS: is_valid={}",
                report.is_valid
            );
            ok_json!(report)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to validate ontology: {}", e);
            error_json!("Failed to validate ontology", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn query_ontology(
    state: web::Data<AppState>,
    request: web::Json<QueryRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let query = request.into_inner().query;
    info!("Querying ontology via CQRS query");

    
    let handler = QueryOntologyHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(QueryOntology { query })).await;

    match result {
        Ok(Ok(results)) => {
            info!(
                "Ontology query successful via CQRS: {} results",
                results.len()
            );
            ok_json!(results)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to query ontology: {}", e);
            error_json!("Failed to query ontology", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn get_ontology_metrics(state: web::Data<AppState>) -> Result<HttpResponse, actix_web::Error> {
    info!("Getting ontology metrics via CQRS query");

    
    let handler = GetOntologyMetricsHandler::new(state.ontology_repository.clone());

    
    let result = execute_in_thread(move || handler.handle(GetOntologyMetrics)).await;

    match result {
        Ok(Ok(metrics)) => {
            info!("Ontology metrics retrieved successfully via CQRS");
            ok_json!(metrics)
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get ontology metrics: {}", e);
            error_json!("Failed to get ontology metrics", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/ontology")
            
            .route("/graph", web::get().to(get_ontology_graph))
            .route("/graph", web::post().to(save_ontology_graph))
            
            .route("/classes", web::get().to(list_owl_classes))
            .route("/classes", web::post().to(add_owl_class))
            .route("/classes/{iri}", web::get().to(get_owl_class))
            .route("/classes/{iri}", web::put().to(update_owl_class))
            .route("/classes/{iri}", web::delete().to(remove_owl_class))
            .route("/classes/{iri}/axioms", web::get().to(get_class_axioms))
            
            .route("/properties", web::get().to(list_owl_properties))
            .route("/properties", web::post().to(add_owl_property))
            .route("/properties/{iri}", web::get().to(get_owl_property))
            .route("/properties/{iri}", web::put().to(update_owl_property))
            
            .route("/axioms", web::post().to(add_axiom))
            .route("/axioms/{id}", web::delete().to(remove_axiom))
            
            .route("/inference", web::get().to(get_inference_results))
            .route("/inference", web::post().to(store_inference_results))
            
            .route("/validate", web::get().to(validate_ontology))
            .route("/query", web::post().to(query_ontology))
            .route("/metrics", web::get().to(get_ontology_metrics)),
    );
}
