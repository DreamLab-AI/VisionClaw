// CQRS-Based Ontology Handler
// Uses Ontology application layer for all OWL operations
//
// ADR-060: every read route answers for the caller. The route resolves the caller's
// `HiddenClasses` (the class IRIs of graph pages hidden from them) and hands it to an
// `answer_*` function, which passes what the store returns through the matching
// `HiddenClasses` scrubber so a hidden class reads exactly as one never stored.

use crate::adapters::oxigraph_ontology_repository::{axiom_iri, OxigraphOntologyRepository};
use crate::handlers::graph_access::caller_hidden_classes;
use crate::handlers::utils::execute_in_thread;
use crate::services::ontology_view_reasoner::{ViewReasoner, VIEW_REASONING_UNAVAILABLE};
use crate::services::ontology_visibility::HiddenClasses;
use crate::settings::OptionalAuth;
use crate::AppState;
use crate::{error_json, not_found, ok_json, service_unavailable};
use actix_web::{web, HttpResponse};
use log::{error, info};
use serde::Deserialize;
use std::sync::Arc;

// Import CQRS handlers
use crate::application::ontology::{
    AddAxiom, AddAxiomHandler, AddOwlClass, AddOwlClassHandler, AddOwlProperty,
    AddOwlPropertyHandler, GetClassAxioms, GetClassAxiomsHandler, GetInferenceResults,
    GetInferenceResultsHandler, GetOntologyMetrics, GetOntologyMetricsHandler, GetOwlClass,
    GetOwlClassHandler, GetOwlProperty, GetOwlPropertyHandler, ListOwlClasses,
    ListOwlClassesHandler, ListOwlProperties, ListOwlPropertiesHandler, LoadOntologyGraph,
    LoadOntologyGraphHandler, QueryOntology, QueryOntologyHandler, RemoveAxiom, RemoveAxiomHandler,
    RemoveOwlClass, RemoveOwlClassHandler, SaveOntologyGraph, SaveOntologyGraphHandler,
    StoreInferenceResults, StoreInferenceResultsHandler, UpdateOwlClass, UpdateOwlClassHandler,
    UpdateOwlProperty, UpdateOwlPropertyHandler, ValidateOntology, ValidateOntologyHandler,
};
use hexser::{DirectiveHandler, QueryHandler};
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::ports::ontology_repository::{
    InferenceResults, OntologyRepository, OwlAxiom, OwlClass, OwlProperty,
};

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

pub async fn get_ontology_graph(
    state: web::Data<AppState>,
    caller: OptionalAuth,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_get_ontology_graph(&state.ontology_repository, &hidden).await
}

/// [`get_ontology_graph`] for a caller from whom `hidden` is hidden.
pub async fn answer_get_ontology_graph(
    repo: &Arc<OxigraphOntologyRepository>,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Getting ontology graph via CQRS query");

    let handler = LoadOntologyGraphHandler::new(repo.clone());

    let result =
        execute_in_thread(move || handler.handle(LoadOntologyGraph).map_err(Box::new)).await;

    match result {
        Ok(Ok(graph)) => {
            info!("Ontology graph loaded successfully via CQRS");
            if hidden.is_empty() {
                return ok_json!(&*graph);
            }
            match repo.get_axioms().await {
                Ok(axioms) => ok_json!(hidden.graph(&graph, &axioms, axiom_iri)),
                Err(e) => {
                    error!("Failed to read axioms to scrub the ontology graph: {}", e);
                    error_json!("Failed to load ontology graph", e.to_string())
                }
            }
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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<SaveGraphRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let graph = request.into_inner().graph;
    info!("Saving ontology graph via CQRS directive");

    let handler = SaveOntologyGraphHandler::new(state.ontology_repository.clone());

    let result = execute_in_thread(move || {
        handler
            .handle(SaveOntologyGraph { graph })
            .map_err(Box::new)
    })
    .await;

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

pub async fn get_owl_class(
    state: web::Data<AppState>,
    caller: OptionalAuth,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_get_owl_class(&state.ontology_repository, iri.into_inner(), &hidden).await
}

/// [`get_owl_class`] for a caller from whom `hidden` is hidden.
pub async fn answer_get_owl_class(
    repo: &Arc<OxigraphOntologyRepository>,
    class_iri: String,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Getting OWL class via CQRS query: iri={}", class_iri);

    let handler = GetOwlClassHandler::new(repo.clone());

    let iri_clone = class_iri.clone();
    let result = execute_in_thread(move || {
        handler
            .handle(GetOwlClass { iri: iri_clone })
            .map_err(Box::new)
    })
    .await
    .map(|r| r.map(|class| hidden.class(class)));

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

pub async fn list_owl_classes(
    state: web::Data<AppState>,
    caller: OptionalAuth,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_list_owl_classes(&state.ontology_repository, &hidden).await
}

/// [`list_owl_classes`] for a caller from whom `hidden` is hidden.
pub async fn answer_list_owl_classes(
    repo: &Arc<OxigraphOntologyRepository>,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Listing all OWL classes via CQRS query");

    let handler = ListOwlClassesHandler::new(repo.clone());

    let result = execute_in_thread(move || handler.handle(ListOwlClasses).map_err(Box::new))
        .await
        .map(|r| r.map(|classes| hidden.classes(classes)));

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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<AddClassRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let class = request.into_inner().class;
    info!("Adding OWL class via CQRS directive: iri={}", class.iri);

    let handler = AddOwlClassHandler::new(state.ontology_repository.clone());

    let class_iri = class.iri.clone();
    let result =
        execute_in_thread(move || handler.handle(AddOwlClass { class }).map_err(Box::new)).await;

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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<UpdateClassRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let class = request.into_inner().class;
    info!("Updating OWL class via CQRS directive: iri={}", class.iri);

    let handler = UpdateOwlClassHandler::new(state.ontology_repository.clone());

    let result =
        execute_in_thread(move || handler.handle(UpdateOwlClass { class }).map_err(Box::new)).await;

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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let class_iri = iri.into_inner();
    info!("Removing OWL class via CQRS directive: iri={}", class_iri);

    let handler = RemoveOwlClassHandler::new(state.ontology_repository.clone());

    let result = execute_in_thread(move || {
        handler
            .handle(RemoveOwlClass { iri: class_iri })
            .map_err(Box::new)
    })
    .await;

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
    caller: OptionalAuth,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_get_owl_property(&state.ontology_repository, iri.into_inner(), &hidden).await
}

/// [`get_owl_property`] for a caller from whom `hidden` is hidden.
pub async fn answer_get_owl_property(
    repo: &Arc<OxigraphOntologyRepository>,
    property_iri: String,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Getting OWL property via CQRS query: iri={}", property_iri);

    let handler = GetOwlPropertyHandler::new(repo.clone());

    // The sync QueryHandler enters a nested block_on; it must run on a blocking
    // thread like every sibling here, or it panics with "Cannot start a runtime
    // from within a runtime" on the actix worker (same defect as the 2026-08-10
    // hierarchy fix; this and add_owl_property were the last two bare calls).
    let iri_for_query = property_iri.clone();
    let result = execute_in_thread(move || {
        handler
            .handle(GetOwlProperty { iri: iri_for_query })
            .map_err(Box::new)
    })
    .await
    .map(|r| r.map(|property| property.map(|p| hidden.property(p))));

    match result {
        Ok(Ok(Some(property))) => {
            info!("OWL property found via CQRS: iri={}", property_iri);
            ok_json!(property)
        }
        Ok(Ok(None)) => {
            info!("OWL property not found: iri={}", property_iri);
            not_found!("OWL property not found")
        }
        Ok(Err(e)) => {
            error!("CQRS query failed to get OWL property: {}", e);
            error_json!("Failed to get OWL property", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn list_owl_properties(
    state: web::Data<AppState>,
    caller: OptionalAuth,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_list_owl_properties(&state.ontology_repository, &hidden).await
}

/// [`list_owl_properties`] for a caller from whom `hidden` is hidden.
pub async fn answer_list_owl_properties(
    repo: &Arc<OxigraphOntologyRepository>,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Listing all OWL properties via CQRS query");

    let handler = ListOwlPropertiesHandler::new(repo.clone());

    let result = execute_in_thread(move || handler.handle(ListOwlProperties).map_err(Box::new))
        .await
        .map(|r| r.map(|properties| hidden.properties(properties)));

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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
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
    // Off the async worker — see get_owl_property.
    let result = execute_in_thread(move || {
        handler
            .handle(AddOwlProperty { property })
            .map_err(Box::new)
    })
    .await;

    match result {
        Ok(Ok(())) => {
            info!(
                "OWL property added successfully via CQRS: iri={}",
                property_iri
            );
            ok_json!(serde_json::json!({
                "success": true,
                "iri": property_iri
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to add OWL property: {}", e);
            error_json!("Failed to add OWL property", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn update_owl_property(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<UpdatePropertyRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let property = request.into_inner().property;
    info!(
        "Updating OWL property via CQRS directive: iri={}",
        property.iri
    );

    let handler = UpdateOwlPropertyHandler::new(state.ontology_repository.clone());

    let result = execute_in_thread(move || {
        handler
            .handle(UpdateOwlProperty { property })
            .map_err(Box::new)
    })
    .await;

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
    caller: OptionalAuth,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_get_class_axioms(&state.ontology_repository, iri.into_inner(), &hidden).await
}

/// [`get_class_axioms`] for a caller from whom `hidden` is hidden.
pub async fn answer_get_class_axioms(
    repo: &Arc<OxigraphOntologyRepository>,
    class_iri: String,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Getting class axioms via CQRS query: iri={}", class_iri);

    let handler = GetClassAxiomsHandler::new(repo.clone());

    let result = execute_in_thread(move || {
        handler
            .handle(GetClassAxioms { class_iri })
            .map_err(Box::new)
    })
    .await
    .map(|r| r.map(|axioms| hidden.axioms(axioms)));

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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
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
    // The CQRS handler is sync-over-async (block_on inside): calling it inline
    // on an actix worker panics with "Cannot start a runtime from within a
    // runtime" and drops the connection. Run it on a dedicated thread, exactly
    // as remove_axiom below already does.
    let result =
        execute_in_thread(move || handler.handle(AddAxiom { axiom }).map_err(Box::new)).await;
    match result {
        Ok(Ok(())) => {
            info!("Axiom added successfully via CQRS: type={}", axiom_type);
            // Live linkage: the reasoned ontology just changed — nudge
            // connected clients (debounced broadcast → topology + inferred-
            // axioms refresh) so the visible graph tracks the evolving data.
            state.graph_service_addr.do_send(
                crate::actors::graph_service_supervisor::NotifyGraphUpdated {
                    reason: "ontology_axiom_added",
                },
            );
            ok_json!(serde_json::json!({
                "success": true,
                "message": format!("Axiom of type {} added", axiom_type)
            }))
        }
        Ok(Err(e)) => {
            error!("CQRS directive failed to add axiom: {}", e);
            error_json!("Failed to add axiom", e.to_string())
        }
        Err(e) => {
            error!("Thread execution error: {}", e);
            error_json!("Internal server error")
        }
    }
}

pub async fn remove_axiom(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    axiom_id: web::Path<u64>,
) -> Result<HttpResponse, actix_web::Error> {
    let id = axiom_id.into_inner();
    info!("Removing axiom via CQRS directive: id={}", id);

    let handler = RemoveAxiomHandler::new(state.ontology_repository.clone());

    let result = execute_in_thread(move || {
        handler
            .handle(RemoveAxiom { axiom_id: id })
            .map_err(Box::new)
    })
    .await;

    match result {
        Ok(Ok(())) => {
            info!("Axiom removed successfully via CQRS");
            // Live linkage: reasoned ontology changed — nudge clients.
            state.graph_service_addr.do_send(
                crate::actors::graph_service_supervisor::NotifyGraphUpdated {
                    reason: "ontology_axiom_removed",
                },
            );
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

pub async fn get_inference_results(
    state: web::Data<AppState>,
    caller: OptionalAuth,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_get_inference_results(
        &state.ontology_repository,
        &state.ontology_view_reasoner,
        &hidden,
    )
    .await
}

fn unavailable(e: impl std::fmt::Display) -> String {
    format!("{VIEW_REASONING_UNAVAILABLE}: {e}")
}

/// The stored whole-store run as `hidden`'s caller may be served it (ADR-2184 Q1): only the
/// inferred axioms their own view also derives ([`ViewReasoner::closure`]), so none reached
/// only through a hidden class survives. The run's time is the view's reasoning time; its
/// timestamp and reasoner version are the stored run's. `None` when no run is stored.
/// Computed once per view and stored run. An unreadable store or a failed view reasoning is
/// an `Err`, never the stored run.
async fn view_inference_results(
    repo: &Arc<OxigraphOntologyRepository>,
    reasoner: &ViewReasoner,
    hidden: &HiddenClasses,
) -> Result<Arc<Option<InferenceResults>>, String> {
    let run_id = repo.inference_run_id_stored().await.map_err(unavailable)?;
    // No stored run (reasoning off, ADR-2192, or never run): nothing to serve, so the view is
    // not reasoned over.
    if run_id.is_none() {
        return Ok(Arc::new(None));
    }
    let view = reasoner.closure(repo.as_ref(), hidden).await?;
    reasoner
        .derived()
        .inference_results(&view, run_id.as_deref(), || async {
            let Some(stored) = repo.get_inference_results().await.map_err(unavailable)? else {
                return Ok(None);
            };
            let key = |a: &OwlAxiom| {
                (
                    format!("{:?}", a.axiom_type),
                    a.subject.clone(),
                    a.object.clone(),
                )
            };
            let derivable: std::collections::HashSet<_> =
                view.inferred_axioms.iter().map(key).collect();
            Ok(Some(InferenceResults {
                timestamp: stored.timestamp,
                inferred_axioms: hidden.axioms(
                    stored
                        .inferred_axioms
                        .into_iter()
                        .filter(|a| derivable.contains(&key(a)))
                        .collect(),
                ),
                inference_time_ms: view.inference_time_ms,
                reasoner_version: stored.reasoner_version,
            }))
        })
        .await
}

/// [`get_inference_results`] for a caller from whom `hidden` is hidden. A gated caller is
/// served [`view_inference_results`]; a full-access caller the stored run, as before.
pub async fn answer_get_inference_results(
    repo: &Arc<OxigraphOntologyRepository>,
    reasoner: &ViewReasoner,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    if !hidden.is_empty() {
        return match view_inference_results(repo, reasoner, hidden)
            .await
            .as_deref()
        {
            Ok(Some(results)) => ok_json!(results),
            Ok(None) => not_found!("No inference results available"),
            Err(e) => {
                error!("Per-view inference results failed: {}", e);
                service_unavailable!(VIEW_REASONING_UNAVAILABLE)
            }
        };
    }
    info!("Getting inference results via CQRS query");

    let handler = GetInferenceResultsHandler::new(repo.clone());

    let result =
        execute_in_thread(move || handler.handle(GetInferenceResults).map_err(Box::new)).await;

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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<StoreInferenceRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let results = request.into_inner().results;
    info!(
        "Storing inference results via CQRS directive: {} axioms",
        results.inferred_axioms.len()
    );

    let handler = StoreInferenceResultsHandler::new(state.ontology_repository.clone());

    let result = execute_in_thread(move || {
        handler
            .handle(StoreInferenceResults { results })
            .map_err(Box::new)
    })
    .await;

    match result {
        Ok(Ok(())) => {
            info!("Inference results stored successfully via CQRS");
            // Live linkage: fresh Whelk inferences just landed — this is THE
            // "reasoned data evolved" event. Nudge clients so the inferred-
            // edges overlay refreshes without a reload.
            state.graph_service_addr.do_send(
                crate::actors::graph_service_supervisor::NotifyGraphUpdated {
                    reason: "inference_results_stored",
                },
            );
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

pub async fn validate_ontology(
    state: web::Data<AppState>,
    caller: OptionalAuth,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_validate_ontology(&state.ontology_repository, &hidden).await
}

/// [`validate_ontology`] for a caller from whom `hidden` is hidden.
pub async fn answer_validate_ontology(
    repo: &Arc<OxigraphOntologyRepository>,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    // A store-wide check cannot be filtered after the fact: run it over the store as this
    // caller sees it.
    if !hidden.is_empty() {
        return match repo.validate_ontology_excluding(hidden.as_set()).await {
            Ok(report) => ok_json!(report),
            Err(e) => {
                error!("Failed to validate ontology for a restricted caller: {}", e);
                error_json!("Failed to validate ontology", e.to_string())
            }
        };
    }
    info!("Validating ontology via CQRS query");

    let handler = ValidateOntologyHandler::new(repo.clone());

    let result =
        execute_in_thread(move || handler.handle(ValidateOntology).map_err(Box::new)).await;

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

/// S1 (SPARQL-injection / privilege surface): the `/ontology/query` endpoint
/// accepts a free-form SPARQL string and returns SELECT-style rows
/// (`Vec<HashMap<String,String>>`). Nothing downstream restricts the operation
/// type, so a caller could submit a SPARQL UPDATE/INSERT/DELETE/DROP/CLEAR/LOAD
/// and mutate the triple store through what is meant to be a read query path.
///
/// This validator enforces read-only SPARQL at the handler boundary: it rejects
/// any query whose first significant keyword (after stripping PREFIX/BASE
/// declarations and comments) is a mutating operation, and rejects update-only
/// keywords appearing anywhere as a standalone token. Legitimate read forms
/// (SELECT / ASK / CONSTRUCT / DESCRIBE) pass through. Mutations must go through
/// the dedicated, power-user-gated write endpoints (e.g. `/ontology/graph` POST,
/// axiom/class mutators) rather than raw SPARQL passthrough.
pub(crate) fn validate_read_only_sparql(query: &str) -> Result<(), String> {
    // Strip line comments (`# ...`) and normalise to uppercase tokens.
    let mut cleaned = String::with_capacity(query.len());
    for line in query.lines() {
        let line = match line.find('#') {
            Some(idx) => &line[..idx],
            None => line,
        };
        cleaned.push_str(line);
        cleaned.push(' ');
    }
    let upper = cleaned.to_uppercase();

    // Tokenise on non-word boundaries so `INSERT{` and `DELETE WHERE` are caught.
    let tokens: Vec<&str> = upper
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();

    // SPARQL Update / management keywords that must never reach the read path.
    // This is the primary guard: a mutation keyword appearing as a standalone
    // token anywhere in the (comment-stripped) query is rejected, which also
    // catches multi-statement smuggling like `SELECT ...; DELETE ...`.
    // SERVICE is a *read* keyword (federated query) but on a public-ish read
    // endpoint it is an SSRF / data-exfiltration vector (`SELECT … SERVICE
    // <http://attacker/>`), so it is forbidden here (PRD-020 WS-0 / ADR-117).
    const FORBIDDEN: [&str; 11] = [
        "INSERT", "DELETE", "DROP", "CLEAR", "LOAD", "CREATE", "ADD", "MOVE", "COPY", "WITH",
        "SERVICE",
    ];
    for tok in &tokens {
        if FORBIDDEN.contains(tok) {
            return Err(format!(
                "SPARQL operation '{}' is not permitted on the read-only query endpoint",
                tok
            ));
        }
    }

    // Positive check: the query must actually contain a recognised read form.
    // We scan for the keyword as a token rather than requiring strict adjacency
    // after the prologue, because PREFIX/BASE declarations embed IRIs
    // (e.g. `PREFIX ex: <http://e/>`) whose tokens would otherwise be mistaken
    // for the query body. The FORBIDDEN scan above already guarantees there is no
    // mutation, so a present read form is sufficient to classify this as a read.
    const READ_FORMS: [&str; 4] = ["SELECT", "ASK", "CONSTRUCT", "DESCRIBE"];
    if !tokens.iter().any(|t| READ_FORMS.contains(t)) {
        return Err(format!(
            "Only read-only SPARQL (SELECT/ASK/CONSTRUCT/DESCRIBE) is permitted; got '{}'",
            tokens.first().copied().unwrap_or("<empty>")
        ));
    }

    Ok(())
}

/// ADR-117 WS-0: server-side SPARQL result clamp for the read-only query paths.
///
/// The repository's `sparql_select_json` (crates/visionclaw-adapters) already
/// fences rows + bytes internally for the `/ontology/sparql` path. The CQRS
/// `/ontology/query` path returns `Vec<HashMap<String,String>>` straight from
/// `run_select` with no bound, so the same invariant is enforced here at the
/// handler boundary: a missing LIMIT is injected, an oversize LIMIT is rewritten
/// down, and the serialised response is capped by row count and byte size,
/// surfacing an explicit `truncated` flag rather than a silent cut.
///
/// Constants mirror the adapter fences and follow the agentbox client-side
/// budget governor (`agentbox/mcp/servers/lib/ontology-budget.js`), which
/// truncates-with-flag rather than cutting silently.
const MAX_SPARQL_ROWS: usize = 10_000;
const DEFAULT_SPARQL_LIMIT: usize = 10_000;
const MAX_SPARQL_RESULT_BYTES: usize = 8 * 1024 * 1024;

/// Inject or clamp a top-level `LIMIT` on a pre-validated read query.
///
/// A SELECT without a trailing LIMIT gets `LIMIT DEFAULT_SPARQL_LIMIT`; a SELECT
/// whose trailing LIMIT exceeds the cap is rewritten down to `MAX_SPARQL_ROWS`.
/// Non-SELECT forms (ASK/CONSTRUCT/DESCRIBE) are returned unchanged. This is a
/// best-effort string rewrite; the row/byte fence in [`cap_result_rows`] is the
/// parse-independent backstop. `validate_read_only_sparql` has already run on
/// the input, so mutation smuggling is not a concern here.
pub fn clamp_sparql_limit(query: &str) -> String {
    if !query.to_uppercase().contains("SELECT") {
        return query.to_string();
    }
    // Scan from the end of the trimmed query for a trailing top-level LIMIT N.
    let trimmed = query.trim_end();
    let tail_upper = trimmed.to_uppercase();
    if let Some(pos) = tail_upper.rfind("LIMIT") {
        let after = trimmed[pos + 5..].trim_start();
        // Only treat as a real LIMIT clause if what follows starts with digits.
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let n: usize = digits.parse().unwrap_or(usize::MAX);
            if n > MAX_SPARQL_ROWS {
                // Rewrite the numeric value down to the cap, preserving any
                // trailing clause (e.g. OFFSET) after the digits.
                let head = &trimmed[..pos + 5];
                let rest = &after[digits.len()..];
                return format!("{} {}{}", head, MAX_SPARQL_ROWS, rest);
            }
            return query.to_string();
        }
    }
    format!("{}\nLIMIT {}", trimmed, DEFAULT_SPARQL_LIMIT)
}

/// Parse-independent backstop: cap a materialised result set by row count and
/// cumulative serialised bytes, returning the (possibly shortened) rows and a
/// `truncated` flag. A sub-SELECT LIMIT that slipped past [`clamp_sparql_limit`]
/// cannot slip past this — it caps what has actually been materialised.
pub fn cap_result_rows(
    mut rows: Vec<std::collections::HashMap<String, String>>,
) -> (Vec<std::collections::HashMap<String, String>>, bool) {
    let mut truncated = false;
    if rows.len() > MAX_SPARQL_ROWS {
        rows.truncate(MAX_SPARQL_ROWS);
        truncated = true;
    }
    // Running serialised-byte fence, independent of the row cap above.
    let mut est_bytes = 0usize;
    let mut keep = rows.len();
    for (i, row) in rows.iter().enumerate() {
        est_bytes += serde_json::to_string(row).map(|s| s.len()).unwrap_or(0);
        if est_bytes > MAX_SPARQL_RESULT_BYTES {
            keep = i;
            truncated = true;
            break;
        }
    }
    rows.truncate(keep);
    (rows, truncated)
}

pub async fn query_ontology(
    auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<QueryRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let caller = OptionalAuth(Some(auth));
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_query_ontology(
        &state.ontology_repository,
        request.into_inner().query,
        &hidden,
    )
    .await
}

/// [`query_ontology`] for a caller from whom `hidden` is hidden.
pub async fn answer_query_ontology(
    repo: &Arc<OxigraphOntologyRepository>,
    query: String,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    // S1: reject mutating SPARQL on the read endpoint before it reaches the store.
    if let Err(reason) = validate_read_only_sparql(&query) {
        info!(
            "Rejected non-read-only SPARQL on /ontology/query: {}",
            reason
        );
        return crate::bad_request!(&reason);
    }

    // ADR-117 WS-0: inject/clamp the LIMIT so Oxigraph never materialises an
    // unbounded SELECT via the CQRS read path (the `run_select` behind
    // `QueryOntology` has no internal fence, unlike `sparql_select_json`).
    let query = clamp_sparql_limit(&query);

    info!("Querying ontology via CQRS query");

    let handler = QueryOntologyHandler::new(repo.clone());

    let result =
        execute_in_thread(move || handler.handle(QueryOntology { query }).map_err(Box::new)).await;

    match result {
        Ok(Ok(results)) => {
            // ADR-117 WS-0 backstop: cap rows + serialised bytes and surface an
            // explicit `truncated` flag instead of returning an unbounded array.
            let (results, truncated) = cap_result_rows(hidden.query_rows(results));
            info!(
                "Ontology query successful via CQRS: {} results (truncated={})",
                results.len(),
                truncated
            );
            let row_count = results.len();
            ok_json!(serde_json::json!({
                "results": results,
                "rowCount": row_count,
                "truncated": truncated,
            }))
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

/// PRD-018 WS-4: `POST /api/ontology/sparql` — body `{ "query": "<SPARQL>" }`.
///
/// Returns SPARQL 1.1 Query Results JSON (`{ head: { vars }, results: { bindings } }`
/// for SELECT; `{ head: {}, boolean }` for ASK). READ-ONLY: the query is
/// validated against the mutating-keyword denylist
/// (INSERT/DELETE/LOAD/CLEAR/DROP/CREATE/ADD/MOVE/COPY/WITH) before it ever
/// reaches the store (defence in depth — the client guards too), and only
/// `Solutions`/`Boolean` result kinds are accepted by the repository.
///
/// GPU-only constraint (PRD-018): this executes server-side over Oxigraph and
/// returns rows only; it never solves or returns layout.
/// @ontology urn:ngm:class:sparql
pub async fn sparql_query(
    state: web::Data<AppState>,
    caller: OptionalAuth,
    request: web::Json<QueryRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_sparql_query(
        &state.ontology_repository,
        request.into_inner().query,
        &hidden,
    )
    .await
}

/// [`sparql_query`] for a caller from whom `hidden` is hidden.
pub async fn answer_sparql_query(
    repo: &Arc<OxigraphOntologyRepository>,
    query: String,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    if let Err(reason) = validate_read_only_sparql(&query) {
        info!(
            "Rejected non-read-only SPARQL on /ontology/sparql: {}",
            reason
        );
        return crate::bad_request!(&reason);
    }

    // ADR-117 WS-0: inject/clamp the LIMIT at the handler boundary. The
    // repository re-clamps and row/byte-fences idempotently; this makes the
    // read-only bound explicit and uniform across both read endpoints.
    let query = clamp_sparql_limit(&query);

    match repo.sparql_select_json(query).await {
        Ok(json) => ok_json!(hidden.sparql_json(json)),
        Err(e) => {
            error!("SPARQL query failed: {}", e);
            error_json!("Failed to execute SPARQL query", e.to_string())
        }
    }
}

/// PRD-018 WS-4 / ADR-099 D4: `GET /api/ontology/inferred` — read the inferred
/// named graph the post-sync reasoner materialises.
///
/// Returns `{ namedGraph: "urn:ngm:graph:ontology:inferred", runId?, triples: [...] }`
/// where each triple is `{ s, p, o }`. Surfaces the provenance-tagged inferences
/// (rdfs:subClassOf closure, owl:equivalentClass, derivation markers) for the
/// `InferencePanel`.
pub async fn get_inferred_graph(
    state: web::Data<AppState>,
    caller: OptionalAuth,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_get_inferred_graph(
        &state.ontology_repository,
        &state.ontology_view_reasoner,
        &hidden,
    )
    .await
}

/// The inferred graph of [`view_inference_results`]: that run materialised into a scratch
/// in-memory store by the same writer the post-sync reasoner uses, then read back, so the
/// shape is the stored graph's. Its run id and run activity are content-addressed over the
/// view's run, so the whole-store run (whose id counts every stored inference) is never
/// named. No stored run reads as the empty graph, as it does with full access. The render
/// costs seconds on a live-sized corpus, so it is computed once per view and stored run.
async fn view_inferred_graph(
    repo: &Arc<OxigraphOntologyRepository>,
    reasoner: &ViewReasoner,
    hidden: &HiddenClasses,
) -> Result<Arc<serde_json::Value>, String> {
    let run_id = repo.inference_run_id_stored().await.map_err(unavailable)?;
    // No stored run reads as the empty graph, without reasoning over the view (ADR-2192).
    if run_id.is_none() {
        let store = oxigraph::store::Store::new().map_err(unavailable)?;
        let empty = OxigraphOntologyRepository::from_store(Arc::new(store));
        return Ok(Arc::new(
            empty.read_inferred_graph().await.map_err(unavailable)?,
        ));
    }
    let view = reasoner.closure(repo.as_ref(), hidden).await?;
    reasoner
        .derived()
        .inferred_graph(&view, run_id.as_deref(), || async {
            let results = view_inference_results(repo, reasoner, hidden).await?;
            let store = oxigraph::store::Store::new().map_err(unavailable)?;
            let scratch = OxigraphOntologyRepository::from_store(Arc::new(store));
            if let Some(results) = results.as_ref() {
                scratch
                    .store_inference_results(results)
                    .await
                    .map_err(unavailable)?;
            }
            scratch.read_inferred_graph().await.map_err(unavailable)
        })
        .await
}

/// [`get_inferred_graph`] for a caller from whom `hidden` is hidden. A gated caller is served
/// [`view_inferred_graph`]; a full-access caller the stored graph, as before.
pub async fn answer_get_inferred_graph(
    repo: &Arc<OxigraphOntologyRepository>,
    reasoner: &ViewReasoner,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    if !hidden.is_empty() {
        return match view_inferred_graph(repo, reasoner, hidden).await {
            Ok(json) => ok_json!(hidden.triples_json(json.as_ref().clone())),
            Err(e) => {
                error!("Per-view inferred graph failed: {}", e);
                service_unavailable!(VIEW_REASONING_UNAVAILABLE)
            }
        };
    }
    match repo.read_inferred_graph().await {
        Ok(json) => ok_json!(json),
        Err(e) => {
            error!("Failed to read inferred graph: {}", e);
            error_json!("Failed to read inferred graph", e.to_string())
        }
    }
}

pub async fn get_ontology_metrics(
    state: web::Data<AppState>,
    caller: OptionalAuth,
) -> Result<HttpResponse, actix_web::Error> {
    let hidden = caller_hidden_classes(&state.graph_service_addr, &caller).await?;
    answer_get_ontology_metrics(&state.ontology_repository, &hidden).await
}

/// [`get_ontology_metrics`] for a caller from whom `hidden` is hidden.
pub async fn answer_get_ontology_metrics(
    repo: &Arc<OxigraphOntologyRepository>,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, actix_web::Error> {
    // Counts are of what survives: count over the store as this caller sees it.
    if !hidden.is_empty() {
        return match repo.get_metrics_excluding(hidden.as_set()).await {
            Ok(metrics) => ok_json!(metrics),
            Err(e) => {
                error!(
                    "Failed to get ontology metrics for a restricted caller: {}",
                    e
                );
                error_json!("Failed to get ontology metrics", e.to_string())
            }
        };
    }
    info!("Getting ontology metrics via CQRS query");

    let handler = GetOntologyMetricsHandler::new(repo.clone());

    let result =
        execute_in_thread(move || handler.handle(GetOntologyMetrics).map_err(Box::new)).await;

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

// WS-0: the `/ontology` scope previously declared here is REMOVED. actix-web
// does not fall through duplicate-prefix scopes, and a second `web::scope(
// "/ontology")` was simultaneously mounted by api_handler::ontology::config.
// Both routing tables are now collapsed into the single canonical scope in
// `crate::handlers::api_handler::ontology::config`, which imports the read /
// mutate handlers below and gates every state-changing op at power_user via
// `RequireAuth::power_user().mutations_only()`. The handler fns
// (`save_ontology_graph`..`get_class_hierarchy`, `query_ontology`,
// `sparql_query`, `get_inferred_graph`, `validate_ontology`, etc.) remain `pub`
// here and are re-used from that scope; `validate_read_only_sparql` and its
// tests also remain. There is no longer a `config` fn in this module.

#[cfg(test)]
mod owl_property_dispatch_tests {
    //! Regression for the nested-runtime panic. The OWL *property* handlers
    //! were the last two actix handlers in this file to call a sync CQRS
    //! handler directly on the async worker; every other handler already went
    //! through `execute_in_thread`. Both directions are pinned here: the CQRS
    //! handlers work when dispatched the way the actix handlers now dispatch
    //! them, and the bare call is documented as the panic it is.

    use super::execute_in_thread;
    use crate::application::ontology::{
        AddOwlProperty, AddOwlPropertyHandler, GetOwlProperty, GetOwlPropertyHandler,
    };
    use crate::test_helpers::MockOntologyRepository;
    use hexser::{DirectiveHandler, QueryHandler};
    use std::sync::Arc;
    use visionclaw_domain::ports::ontology_repository::{OntologyRepository, OwlProperty};

    fn property(iri: &str) -> OwlProperty {
        OwlProperty {
            iri: iri.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn owl_property_handlers_round_trip_off_the_async_worker() {
        let repo: Arc<dyn OntologyRepository> = Arc::new(MockOntologyRepository::new());
        let iri = "http://example.org/hasPart".to_string();

        let add = AddOwlPropertyHandler::new(repo.clone());
        let p = property(&iri);
        let added =
            execute_in_thread(move || add.handle(AddOwlProperty { property: p }).map_err(Box::new))
                .await
                .expect("blocking task joined");
        assert!(added.is_ok(), "add failed: {:?}", added.err());

        let get = GetOwlPropertyHandler::new(repo.clone());
        let q = iri.clone();
        let got =
            execute_in_thread(move || get.handle(GetOwlProperty { iri: q }).map_err(Box::new))
                .await
                .expect("blocking task joined")
                .expect("query ok");
        assert_eq!(got.map(|p| p.iri), Some(iri.clone()));

        let get = GetOwlPropertyHandler::new(repo);
        let missing = execute_in_thread(move || {
            get.handle(GetOwlProperty {
                iri: "http://example.org/absent".into(),
            })
            .map_err(Box::new)
        })
        .await
        .expect("blocking task joined")
        .expect("query ok");
        assert!(missing.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bare_sync_handle_on_the_async_worker_is_the_panic_this_guards_against() {
        let repo: Arc<dyn OntologyRepository> = Arc::new(MockOntologyRepository::new());
        let get = GetOwlPropertyHandler::new(repo);
        // Calling the sync handler directly from an async context is exactly what
        // the actix handlers used to do. Tokio refuses the nested block_on. If this
        // ever stops panicking (e.g. the handlers become async-direct), the wrapping
        // in ontology_handler.rs is no longer load-bearing and this test can go.
        let outcome = tokio::task::spawn(async move {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                get.handle(GetOwlProperty {
                    iri: "http://example.org/x".into(),
                })
                .map_err(Box::new)
            }))
        })
        .await
        .expect("task joined");
        assert!(
            outcome.is_err(),
            "nested block_on did not panic — re-check the guard"
        );
    }
}

#[cfg(test)]
mod sparql_validation_tests {
    use super::validate_read_only_sparql;

    #[test]
    fn allows_read_only_forms() {
        assert!(validate_read_only_sparql("SELECT ?s WHERE { ?s ?p ?o }").is_ok());
        assert!(validate_read_only_sparql("ASK { ?s ?p ?o }").is_ok());
        assert!(validate_read_only_sparql("CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }").is_ok());
        assert!(validate_read_only_sparql("DESCRIBE <urn:x>").is_ok());
        // Prologue (PREFIX/BASE) before SELECT is fine.
        assert!(
            validate_read_only_sparql("PREFIX ex: <http://e/> SELECT ?s WHERE { ?s ex:p ?o }")
                .is_ok()
        );
        // Lowercase + comments tolerated.
        assert!(validate_read_only_sparql("# comment\nselect ?s where { ?s ?p ?o }").is_ok());
    }

    #[test]
    fn rejects_mutating_sparql() {
        assert!(validate_read_only_sparql("INSERT DATA { <a> <b> <c> }").is_err());
        assert!(validate_read_only_sparql("DELETE WHERE { ?s ?p ?o }").is_err());
        assert!(validate_read_only_sparql("DROP GRAPH <urn:g>").is_err());
        assert!(validate_read_only_sparql("CLEAR ALL").is_err());
        assert!(validate_read_only_sparql("LOAD <http://evil/data>").is_err());
        // Mutation smuggled after a comment-stripped prologue.
        assert!(validate_read_only_sparql(
            "PREFIX ex: <http://e/>\nINSERT { ex:a ex:b ex:c } WHERE {}"
        )
        .is_err());
        // Mutation hidden behind a SELECT prefix as a later operation.
        assert!(validate_read_only_sparql(
            "SELECT ?s WHERE { ?s ?p ?o }; DELETE WHERE { ?s ?p ?o }"
        )
        .is_err());
    }

    #[test]
    fn rejects_unknown_first_token() {
        assert!(validate_read_only_sparql("WITH <urn:g> WHERE {}").is_err());
        assert!(validate_read_only_sparql("   ").is_err());
    }
}

/// ADR-060: every `/api/ontology` read answers for a caller as if the classes hidden from them
/// had never been stored. `Secret` is hidden; `Nowhere` was never stored. "Never stored" is
/// the same fixture built without `Secret` and without every edge and axiom that names it.
#[cfg(test)]
pub(crate) mod hidden_class_tests {
    use super::*;
    use crate::services::ontology_visibility::testing::{mentions, reply, sorted};
    use oxigraph::store::Store;
    use serde_json::Value;
    use std::collections::HashSet;
    use visionclaw_domain::ports::ontology_repository::AxiomType;

    pub(crate) const HIDDEN: &str = "http://example.org/onto#Secret";
    pub(crate) const NEVER: &str = "http://example.org/onto#Nowhere";
    const ECHOES: &[(&str, &str)] = &[(HIDDEN, "<probe>"), (NEVER, "<probe>")];

    fn iri(local: &str) -> String {
        format!("http://example.org/onto#{local}")
    }

    fn class(local: &str, label: Option<&str>, parents: &[String]) -> OwlClass {
        OwlClass {
            iri: iri(local),
            label: label.map(str::to_string),
            parent_classes: parents.to_vec(),
            ..OwlClass::default()
        }
    }

    fn axiom(axiom_type: AxiomType, subject: &str, object: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type,
            subject: subject.to_string(),
            object: object.to_string(),
            annotations: Default::default(),
        }
    }

    /// Agent ⊒ Org; Company; hasPart (domain Org, range Company); Org ⊑ owl:Thing inferred.
    /// With the hidden class: Secret (unlabelled) ⊑ Org, Company ⊑ Secret, hasPart's domain
    /// also Secret, Secret ⊑ Agent inferred, and a provenance record for Secret.
    pub(crate) async fn repo(with_hidden: bool) -> Arc<OxigraphOntologyRepository> {
        let store = Store::new().expect("in-memory store");
        let repo = Arc::new(OxigraphOntologyRepository::from_store(Arc::new(store)));
        let company_parents = if with_hidden {
            vec![HIDDEN.to_string()]
        } else {
            Vec::new()
        };
        let mut classes = vec![
            class("Agent", Some("Agent"), &[]),
            class("Org", Some("Org"), &[iri("Agent")]),
            class("Company", Some("Company"), &company_parents),
        ];
        let mut axioms = vec![axiom(AxiomType::SubClassOf, &iri("Org"), &iri("Agent"))];
        let mut domain = vec![iri("Org")];
        let mut inferred = vec![axiom(
            AxiomType::SubClassOf,
            &iri("Org"),
            "http://www.w3.org/2002/07/owl#Thing",
        )];
        if with_hidden {
            classes.push(class("Secret", None, &[iri("Org")]));
            axioms.push(axiom(AxiomType::SubClassOf, HIDDEN, &iri("Org")));
            axioms.push(axiom(AxiomType::SubClassOf, &iri("Company"), HIDDEN));
            domain.push(HIDDEN.to_string());
            inferred.push(axiom(AxiomType::SubClassOf, HIDDEN, &iri("Agent")));
        }
        for c in &classes {
            repo.add_owl_class(c).await.expect("class stored");
        }
        for a in &axioms {
            repo.add_axiom(a).await.expect("axiom stored");
        }
        repo.add_owl_property(&OwlProperty {
            iri: iri("hasPart"),
            domain,
            range: vec![iri("Company")],
            ..OwlProperty::default()
        })
        .await
        .expect("property stored");
        repo.store_inference_results(&InferenceResults {
            timestamp: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid time"),
            inferred_axioms: inferred,
            inference_time_ms: 7,
            reasoner_version: "whelk-test".to_string(),
        })
        .await
        .expect("inference stored");
        repo.emit_provenance(visionclaw_adapters::provenance_emitter::ActivityRecord {
            activity_urn: "urn:visionclaw:execution:sha256-12-abcdefabcdef".to_string(),
            agent_did: "did:nostr:abcdef".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            action: "ingest".to_string(),
            derivation: "asserted".to_string(),
            used: with_hidden.then(|| HIDDEN.to_string()),
            generated: Some(iri("Company")),
            informed_by: None,
        })
        .await
        .expect("provenance stored");
        if with_hidden {
            repo.emit_provenance(visionclaw_adapters::provenance_emitter::ActivityRecord {
                activity_urn: "urn:visionclaw:execution:sha256-12-feedfeedfeed".to_string(),
                agent_did: "did:nostr:abcdef".to_string(),
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                action: "ingest".to_string(),
                derivation: "asserted".to_string(),
                used: None,
                generated: Some(HIDDEN.to_string()),
                informed_by: None,
            })
            .await
            .expect("provenance stored");
        }
        repo
    }

    pub(crate) fn hidden() -> HiddenClasses {
        HiddenClasses::from(HashSet::from([HIDDEN.to_string()]))
    }

    fn none() -> HiddenClasses {
        HiddenClasses::none()
    }

    /// The hidden caller's answer over the store holding `Secret`, and the full-access answer
    /// over the store that never held it, both order-normalised.
    async fn hidden_and_never_stored<F, Fut>(answer: F) -> ((u16, Value), (u16, Value))
    where
        F: Fn(Arc<OxigraphOntologyRepository>, HiddenClasses) -> Fut,
        Fut: std::future::Future<Output = Result<HttpResponse, actix_web::Error>>,
    {
        let got = reply(answer(repo(true).await, hidden()).await, &[]).await;
        let never = reply(answer(repo(false).await, none()).await, &[]).await;
        ((got.0, sorted(got.1)), (never.0, sorted(never.1)))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_class_of_a_hidden_iri_is_the_unknown_class_reply() {
        let r = repo(true).await;
        let got = reply(
            answer_get_owl_class(&r, HIDDEN.to_string(), &hidden()).await,
            ECHOES,
        )
        .await;
        let unknown = reply(
            answer_get_owl_class(&r, NEVER.to_string(), &hidden()).await,
            ECHOES,
        )
        .await;
        assert_eq!(got, unknown);
        assert_eq!(got.0, 404);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn class_list_drops_the_hidden_class_and_every_parent_link_to_it() {
        let (got, never) =
            hidden_and_never_stored(|r, h| async move { answer_list_owl_classes(&r, &h).await })
                .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, &iri("Company")));
        assert_eq!(got, never);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn properties_drop_the_hidden_class_from_domain_and_range() {
        let (got, never) =
            hidden_and_never_stored(|r, h| async move { answer_list_owl_properties(&r, &h).await })
                .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert_eq!(got, never);
        let (got, never) = hidden_and_never_stored(|r, h| async move {
            answer_get_owl_property(&r, iri("hasPart"), &h).await
        })
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, &iri("Org")));
        assert_eq!(got, never);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn class_axioms_of_a_hidden_class_are_the_unknown_reply_and_others_drop_it() {
        let r = repo(true).await;
        let got = reply(
            answer_get_class_axioms(&r, HIDDEN.to_string(), &hidden()).await,
            ECHOES,
        )
        .await;
        let unknown = reply(
            answer_get_class_axioms(&r, NEVER.to_string(), &hidden()).await,
            ECHOES,
        )
        .await;
        assert_eq!(got, unknown);
        let (got, never) = hidden_and_never_stored(|r, h| async move {
            answer_get_class_axioms(&r, iri("Org"), &h).await
        })
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert_eq!(got, never);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn graph_drops_the_hidden_node_its_axiom_records_and_edges_without_id_gaps() {
        let shape = |body: &Value| {
            let nodes = body["data"]["nodes"].as_array().expect("nodes").clone();
            let name_of = |id: &Value| {
                nodes
                    .iter()
                    .find(|n| &n["id"] == id)
                    .map(|n| n["metadataId"].as_str().unwrap_or_default().to_string())
                    .unwrap_or_default()
            };
            let mut names: Vec<String> = nodes
                .iter()
                .map(|n| n["metadataId"].as_str().unwrap_or_default().to_string())
                .collect();
            names.sort();
            let mut edges: Vec<(String, String, String)> = body["data"]["edges"]
                .as_array()
                .expect("edges")
                .iter()
                .map(|e| {
                    (
                        name_of(&e["source"]),
                        name_of(&e["target"]),
                        e["edgeType"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect();
            edges.sort();
            let mut ids: Vec<u64> = nodes.iter().filter_map(|n| n["id"].as_u64()).collect();
            ids.sort_unstable();
            (names, edges, ids)
        };
        let got = reply(
            answer_get_ontology_graph(&repo(true).await, &hidden()).await,
            &[],
        )
        .await;
        let never = reply(
            answer_get_ontology_graph(&repo(false).await, &none()).await,
            &[],
        )
        .await;
        assert!(!mentions(&got.1, HIDDEN), "the hidden class is gone");
        let (names, edges, ids) = shape(&got.1);
        assert_eq!((names, edges), {
            let (n, e, _) = shape(&never.1);
            (n, e)
        });
        assert_eq!(
            ids,
            (1..=ids.len() as u64).collect::<Vec<_>>(),
            "ids are dense, so no gap marks a removed node"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inference_results_drop_axioms_naming_the_hidden_class() {
        let (got, never) = hidden_and_never_stored(|r, h| async move {
            answer_get_inference_results(&r, &ViewReasoner::new(), &h).await
        })
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, &iri("Org")));
        assert_eq!(
            got.1["data"]["inferred_axioms"],
            never.1["data"]["inferred_axioms"]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inferred_graph_drops_triples_and_axiom_records_naming_the_hidden_class() {
        let got = reply(
            answer_get_inferred_graph(&repo(true).await, &ViewReasoner::new(), &hidden()).await,
            &[],
        )
        .await;
        let never = reply(
            answer_get_inferred_graph(&repo(false).await, &ViewReasoner::new(), &none()).await,
            &[],
        )
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        // Every triple except the run bookkeeping (whose content-addressed id counts the
        // stored run's axioms) is the same as in the store that never held the class.
        let records = |body: &Value| {
            let mut t: Vec<String> = body["data"]["triples"]
                .as_array()
                .expect("triples")
                .iter()
                .filter(|t| {
                    let s = t["s"].as_str().unwrap_or_default();
                    let o = t["o"].as_str().unwrap_or_default();
                    !s.starts_with("urn:ngm:inference:")
                        && !o.starts_with("urn:ngm:inference:")
                        && !s.contains("inference-meta")
                })
                .map(Value::to_string)
                .collect();
            t.sort();
            t
        };
        assert!(!records(&never.1).is_empty());
        assert_eq!(records(&got.1), records(&never.1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn validate_reports_on_the_store_as_if_the_hidden_class_never_existed() {
        let (got, never) =
            hidden_and_never_stored(|r, h| async move { answer_validate_ontology(&r, &h).await })
                .await;
        assert_eq!(got, never);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn metrics_count_only_what_survives() {
        let (got, never) =
            hidden_and_never_stored(
                |r, h| async move { answer_get_ontology_metrics(&r, &h).await },
            )
            .await;
        assert_eq!(got, never);
    }

    const SUBCLASS_ROWS: &str = "SELECT ?c ?p WHERE { GRAPH <urn:ngm:graph:ontology:assert> \
        { ?c <http://www.w3.org/2000/01/rdf-schema#subClassOf> ?p } }";

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn query_drops_rows_binding_the_hidden_class() {
        let (got, never) = hidden_and_never_stored(|r, h| async move {
            answer_query_ontology(&r, SUBCLASS_ROWS.to_string(), &h).await
        })
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, &iri("Agent")));
        assert_eq!(got, never);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sparql_drops_rows_binding_the_hidden_class() {
        let (got, never) = hidden_and_never_stored(|r, h| async move {
            answer_sparql_query(&r, SUBCLASS_ROWS.to_string(), &h).await
        })
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, &iri("Agent")));
        assert_eq!(got, never);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn with_full_access_the_hidden_class_is_served_as_before() {
        let r = repo(true).await;
        let class = reply(
            answer_get_owl_class(&r, HIDDEN.to_string(), &none()).await,
            &[],
        )
        .await;
        assert_eq!(class.0, 200);
        let list = reply(answer_list_owl_classes(&r, &none()).await, &[]).await;
        assert!(mentions(&list.1, HIDDEN));
        let metrics = reply(answer_get_ontology_metrics(&r, &none()).await, &[]).await;
        let native = r.get_metrics().await.expect("metrics");
        assert_eq!(metrics.1["data"]["class_count"], native.class_count);
        let rows = reply(
            answer_sparql_query(&r, SUBCLASS_ROWS.to_string(), &none()).await,
            &[],
        )
        .await;
        assert!(mentions(&rows.1, HIDDEN));
    }
}

/// ADR-2184 Q1 residual on the stored inference reads: `Ant ⊑ Hive ⊑ Bee` with `Hive` hidden.
/// The stored run is the real whole-store Whelk closure (as the post-sync reasoner writes
/// it), so it holds `Ant ⊑ Bee`, which a gated caller must not see.
#[cfg(test)]
pub(crate) mod per_view_reasoning_tests {
    use super::*;
    use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
    use crate::services::github_sync_service::GitHubSyncService;
    use crate::services::ontology_visibility::testing::{mentions, reply};
    use oxigraph::store::Store;
    use serde_json::Value;
    use std::collections::HashSet;
    use visionclaw_domain::ports::inference_engine::InferenceEngine;
    use visionclaw_domain::ports::ontology_repository::AxiomType;

    pub(crate) const ANT: &str = "http://example.org/onto#Ant";
    pub(crate) const HIVE: &str = "http://example.org/onto#Hive";
    pub(crate) const BEE: &str = "http://example.org/onto#Bee";
    const SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
    const TIME_MS: &str = "https://narrativegoldmine.com/ns/v1#inferenceTimeMs";

    fn class(iri: &str) -> OwlClass {
        OwlClass {
            iri: iri.to_string(),
            label: Some(iri.rsplit('#').next().unwrap_or(iri).to_string()),
            ..OwlClass::default()
        }
    }

    fn sub(subject: &str, object: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::SubClassOf,
            subject: subject.to_string(),
            object: object.to_string(),
            annotations: Default::default(),
        }
    }

    /// The chain store, with or without `Hive`, holding the Whelk closure of its own content.
    pub(crate) async fn repo(with_hidden: bool) -> Arc<OxigraphOntologyRepository> {
        let store = Store::new().expect("in-memory store");
        let repo = Arc::new(OxigraphOntologyRepository::from_store(Arc::new(store)));
        let (classes, axioms) = if with_hidden {
            (
                vec![class(ANT), class(HIVE), class(BEE)],
                vec![sub(ANT, HIVE), sub(HIVE, BEE)],
            )
        } else {
            (vec![class(ANT), class(BEE)], Vec::new())
        };
        for c in &classes {
            repo.add_owl_class(c).await.expect("class stored");
        }
        for a in &axioms {
            repo.add_axiom(a).await.expect("axiom stored");
        }
        let mut input = axioms.clone();
        input.extend(GitHubSyncService::ngm_property_hierarchy_axioms());
        let mut engine = WhelkInferenceEngine::new();
        engine.load_ontology(classes, input).await.expect("loads");
        let mut results = engine.infer().await.expect("classifies");
        results.timestamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid time");
        repo.store_inference_results(&results)
            .await
            .expect("inference stored");
        repo
    }

    pub(crate) fn hidden() -> HiddenClasses {
        HiddenClasses::from(HashSet::from([HIVE.to_string()]))
    }

    /// The stored whole-store run id.
    async fn whole_store_run_id(repo: &OxigraphOntologyRepository) -> String {
        repo.read_inferred_graph().await.expect("reads")["runId"]
            .as_str()
            .expect("a run id")
            .to_string()
    }

    /// `body` without its timing triple, which is the reasoner's measured time.
    fn untimed(mut body: Value) -> Value {
        if let Some(t) = body
            .pointer_mut("/data/triples")
            .and_then(Value::as_array_mut)
        {
            t.retain(|t| t["p"] != TIME_MS);
            t.sort_by_key(|a| a.to_string());
        }
        body
    }

    fn states(body: &Value, s: &str, p: &str, o: &str) -> bool {
        body["data"]["triples"]
            .as_array()
            .expect("triples")
            .iter()
            .any(|t| t["s"] == s && t["p"] == p && t["o"] == o)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inferred_graph_omits_inferences_through_a_hidden_class_and_the_whole_run_id() {
        let whole = repo(true).await;
        let run_id = whole_store_run_id(&whole).await;
        let gated = reply(
            answer_get_inferred_graph(&whole, &ViewReasoner::new(), &hidden()).await,
            &[],
        )
        .await;
        let never = reply(
            answer_get_inferred_graph(
                &repo(false).await,
                &ViewReasoner::new(),
                &HiddenClasses::none(),
            )
            .await,
            &[],
        )
        .await;
        assert_eq!(gated.0, 200);
        assert!(!states(&gated.1, ANT, SUBCLASS_OF, BEE), "{}", gated.1);
        assert!(
            !mentions(&gated.1, &run_id),
            "whole-store run {run_id}: {}",
            gated.1
        );
        assert_eq!(untimed(gated.1), untimed(never.1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inferred_graph_with_full_access_still_states_the_whole_closure() {
        let whole = repo(true).await;
        let full = reply(
            answer_get_inferred_graph(&whole, &ViewReasoner::new(), &HiddenClasses::none()).await,
            &[],
        )
        .await;
        assert!(states(&full.1, ANT, SUBCLASS_OF, BEE), "{}", full.1);
        assert!(mentions(&full.1, &whole_store_run_id(&whole).await));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn inference_results_omit_inferences_through_a_hidden_class() {
        let gated = reply(
            answer_get_inference_results(&repo(true).await, &ViewReasoner::new(), &hidden()).await,
            &[],
        )
        .await;
        let never = reply(
            answer_get_inference_results(
                &repo(false).await,
                &ViewReasoner::new(),
                &HiddenClasses::none(),
            )
            .await,
            &[],
        )
        .await;
        let pairs = |body: &Value| {
            let mut p: Vec<String> = body["data"]["inferred_axioms"]
                .as_array()
                .expect("axioms")
                .iter()
                .map(|a| format!("{} {} {}", a["axiom_type"], a["subject"], a["object"]))
                .collect();
            p.sort();
            p
        };
        assert_eq!(gated.0, 200);
        assert!(
            !pairs(&gated.1)
                .iter()
                .any(|p| p.contains(ANT) && p.contains(BEE)),
            "{:?}",
            pairs(&gated.1)
        );
        assert_eq!(pairs(&gated.1), pairs(&never.1));
    }

    fn failing(_: Vec<OwlClass>, _: Vec<OwlAxiom>) -> Result<Vec<OwlAxiom>, String> {
        Err("reasoner down".to_string())
    }

    /// Full access is served the stored run and graph exactly as stored, and never reasons
    /// over a view (the reasoner here always fails, so any use would show).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_access_inference_reads_are_the_stored_run_byte_for_byte() {
        let whole = repo(true).await;
        let reasoner = ViewReasoner::with_reasoner(failing);
        let graph = reply(
            answer_get_inferred_graph(&whole, &reasoner, &HiddenClasses::none()).await,
            &[],
        )
        .await;
        let stored_graph = reply(
            ok_json!(whole.read_inferred_graph().await.expect("reads")),
            &[],
        )
        .await;
        assert_eq!(graph, stored_graph);
        let results = reply(
            answer_get_inference_results(&whole, &reasoner, &HiddenClasses::none()).await,
            &[],
        )
        .await;
        let stored_results = reply(
            ok_json!(whole
                .get_inference_results()
                .await
                .expect("reads")
                .expect("a run")),
            &[],
        )
        .await;
        assert_eq!(results, stored_results);
        assert_eq!(reasoner.runs(), 0);
    }

    /// ADR-2192: with no stored run (reasoning off clears it), a gated caller's inference
    /// reads answer "none" and the empty graph without a per-view Whelk run.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn with_no_stored_run_the_gated_reads_do_not_reason() {
        let store = oxigraph::store::Store::new().expect("store");
        let repo = Arc::new(OxigraphOntologyRepository::from_store(Arc::new(store)));
        let reasoner = ViewReasoner::new();
        let hidden = HiddenClasses::from(HashSet::from(["urn:test:Hidden".to_string()]));
        let results = view_inference_results(&repo, &reasoner, &hidden)
            .await
            .expect("answers");
        assert!(results.is_none());
        let graph = view_inferred_graph(&repo, &reasoner, &hidden)
            .await
            .expect("answers");
        let empty = repo.read_inferred_graph().await.expect("reads");
        assert_eq!(graph.as_ref(), &empty);
        assert_eq!(reasoner.runs(), 0, "no per-view Whelk run");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_view_reasoning_is_503_on_both_inference_reads() {
        let whole = repo(true).await;
        let run_id = whole_store_run_id(&whole).await;
        let reasoner = ViewReasoner::with_reasoner(failing);
        for got in [
            reply(
                answer_get_inferred_graph(&whole, &reasoner, &hidden()).await,
                &[],
            )
            .await,
            reply(
                answer_get_inference_results(&whole, &reasoner, &hidden()).await,
                &[],
            )
            .await,
        ] {
            assert_eq!(got.0, 503, "{}", got.1);
            assert!(
                !mentions(&got.1, &run_id) && !mentions(&got.1, ANT),
                "{}",
                got.1
            );
        }
    }

    /// With nothing hidden the per-view input is the post-sync reasoner's own, so its closure
    /// is the stored whole-store closure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_view_closure_of_the_whole_store_is_the_stored_closure() {
        let whole = repo(true).await;
        let pairs = |axioms: &[OwlAxiom]| -> HashSet<(String, String, String)> {
            axioms
                .iter()
                .map(|a| {
                    (
                        format!("{:?}", a.axiom_type),
                        a.subject.clone(),
                        a.object.clone(),
                    )
                })
                .collect()
        };
        let view = ViewReasoner::new()
            .closure(whole.as_ref(), &HiddenClasses::none())
            .await
            .expect("reasons");
        let stored = whole
            .get_inference_results()
            .await
            .expect("reads")
            .expect("a run");
        assert!(!stored.inferred_axioms.is_empty());
        assert_eq!(pairs(&view.inferred_axioms), pairs(&stored.inferred_axioms));
    }

    /// The cost of per-view reasoning on a corpus shaped like the live one (9,244 classes, a
    /// tree of branching ~4.8 and depth up to 16, so ~9.2k `SubClassOf` axioms, plus 1,000
    /// second parents), with 5% of the classes hidden. Prints the cold closure, the cached
    /// closure and the `/inferred` render. Run with
    /// `cargo test --lib per_view_reasoning_cost -- --ignored --nocapture`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "measurement, not a check: builds a 9k-class store"]
    async fn per_view_reasoning_cost() {
        use std::time::Instant;
        const CLASSES: usize = 9_244;
        let iri = |i: usize| format!("http://example.org/onto#C{i}");
        let store = Store::new().expect("in-memory store");
        let whole = Arc::new(OxigraphOntologyRepository::from_store(Arc::new(store)));
        let mut classes = Vec::with_capacity(CLASSES);
        let mut axioms = Vec::with_capacity(CLASSES + 1_000);
        // Parent of i is (i - 1) / 5 for a branching of ~5 (depth ~6); every 577th class
        // instead hangs off a deep spine so the deepest chain reaches 16.
        for i in 0..CLASSES {
            classes.push(class(&iri(i)));
            if i == 0 {
                continue;
            }
            let parent = if i < 16 { i - 1 } else { (i - 1) / 5 };
            axioms.push(sub(&iri(i), &iri(parent)));
        }
        for k in 0..1_000 {
            let i = 100 + k * 9;
            axioms.push(sub(&iri(i), &iri((i * 7) % 90 + 1)));
        }
        let started = Instant::now();
        for c in &classes {
            whole.add_owl_class(c).await.expect("class stored");
        }
        for a in &axioms {
            whole.add_axiom(a).await.expect("axiom stored");
        }
        let build = started.elapsed();
        let mut input = axioms.clone();
        input.extend(GitHubSyncService::ngm_property_hierarchy_axioms());
        let started = Instant::now();
        let mut engine = WhelkInferenceEngine::new();
        engine.load_ontology(classes, input).await.expect("loads");
        let results = engine.infer().await.expect("classifies");
        let whole_infer = started.elapsed();
        whole
            .store_inference_results(&results)
            .await
            .expect("stored");
        let hidden = HiddenClasses::from((0..CLASSES).step_by(20).map(iri).collect::<HashSet<_>>());
        let reasoner = ViewReasoner::new();
        let started = Instant::now();
        let view = reasoner
            .closure(whole.as_ref(), &hidden)
            .await
            .expect("reasons");
        let cold = started.elapsed();
        let started = Instant::now();
        reasoner
            .closure(whole.as_ref(), &hidden)
            .await
            .expect("reasons");
        let warm = started.elapsed();
        let started = Instant::now();
        let results_reply = answer_get_inference_results(&whole, &reasoner, &hidden).await;
        let results_time = started.elapsed();
        let started = Instant::now();
        let graph_reply = answer_get_inferred_graph(&whole, &reasoner, &hidden).await;
        let render = started.elapsed();
        let started = Instant::now();
        answer_get_inference_results(&whole, &reasoner, &hidden)
            .await
            .expect("reply");
        let results_warm = started.elapsed();
        let started = Instant::now();
        answer_get_inferred_graph(&whole, &reasoner, &hidden)
            .await
            .expect("reply");
        let render_warm = started.elapsed();
        let started = Instant::now();
        answer_get_inference_results(&whole, &reasoner, &HiddenClasses::none())
            .await
            .expect("reply");
        let full_results = started.elapsed();
        let started = Instant::now();
        let full_reply = answer_get_inferred_graph(&whole, &reasoner, &HiddenClasses::none()).await;
        let full = started.elapsed();
        assert_eq!(results_reply.expect("reply").status(), 200);
        assert_eq!(graph_reply.expect("reply").status(), 200);
        assert_eq!(full_reply.expect("reply").status(), 200);
        println!(
            "corpus: {CLASSES} classes, {} axioms (store built in {build:?})\n\
             whole-store load+infer: {whole_infer:?}, {} inferred axioms\n\
             per-view closure, cold (repo reads + digest + whelk): {cold:?}, {} inferred axioms, \
             whelk {} ms\n\
             per-view closure, cached: {warm:?}\n\
             gated /inference/results, first (cached closure): {results_time:?}, \
             then cached: {results_warm:?}\n\
             gated /inferred, first (cached results + scratch render): {render:?}, \
             then cached: {render_warm:?}\n\
             full-access /inference/results: {full_results:?}, /inferred: {full:?}",
            axioms.len(),
            results.inferred_axioms.len(),
            view.inferred_axioms.len(),
            view.inference_time_ms,
        );
    }
}
