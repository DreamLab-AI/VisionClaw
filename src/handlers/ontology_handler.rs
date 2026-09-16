// CQRS-Based Ontology Handler
// Uses Ontology application layer for all OWL operations

use crate::handlers::utils::execute_in_thread;
use crate::AppState;
use crate::{error_json, not_found, ok_json};
use actix_web::{web, HttpResponse};
use log::{error, info};
use serde::Deserialize;

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
    InferenceResults, OwlAxiom, OwlClass, OwlProperty,
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
) -> Result<HttpResponse, actix_web::Error> {
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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
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

pub async fn get_owl_class(
    state: web::Data<AppState>,
    iri: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
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

pub async fn list_owl_classes(
    state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
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

pub async fn get_class_hierarchy(
    state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    use serde::Serialize;
    use std::collections::HashMap;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ClassNode {
        iri: String,
        label: String,
        parent_iri: Option<String>,
        children_iris: Vec<String>,
        node_count: usize,
        depth: usize,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ClassHierarchy {
        root_classes: Vec<String>,
        hierarchy: HashMap<String, ClassNode>,
    }

    let handler = ListOwlClassesHandler::new(state.ontology_repository.clone());
    let result = execute_in_thread(move || handler.handle(ListOwlClasses)).await;

    let classes = match result {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => return error_json!("Failed to list OWL classes", e.to_string()),
        Err(e) => return error_json!("Internal server error", e),
    };

    let mut children_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut root_classes: Vec<String> = Vec::new();

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

    fn depth_of(iri: &str, classes: &[OwlClass], memo: &mut HashMap<String, usize>) -> usize {
        if let Some(&d) = memo.get(iri) {
            return d;
        }
        let d = classes
            .iter()
            .find(|c| c.iri == iri)
            .map(|c| {
                c.parent_classes
                    .iter()
                    .map(|p| depth_of(p, classes, memo) + 1)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        memo.insert(iri.to_string(), d);
        d
    }

    fn descendants(
        iri: &str,
        children_map: &HashMap<String, Vec<String>>,
        memo: &mut HashMap<String, usize>,
    ) -> usize {
        if let Some(&n) = memo.get(iri) {
            return n;
        }
        let n = children_map
            .get(iri)
            .map(|ch| {
                ch.len()
                    + ch.iter()
                        .map(|c| descendants(c, children_map, memo))
                        .sum::<usize>()
            })
            .unwrap_or(0);
        memo.insert(iri.to_string(), n);
        n
    }

    let mut depth_memo = HashMap::new();
    let mut count_memo = HashMap::new();
    let mut hierarchy: HashMap<String, ClassNode> = HashMap::new();

    for class in &classes {
        let depth = depth_of(&class.iri, &classes, &mut depth_memo);
        let node_count = descendants(&class.iri, &children_map, &mut count_memo);
        let children_iris = children_map.get(&class.iri).cloned().unwrap_or_default();
        let parent_iri = class.parent_classes.first().cloned();
        let label = class.label.clone().unwrap_or_else(|| {
            class
                .iri
                .split('#')
                .last()
                .or_else(|| class.iri.split('/').last())
                .unwrap_or(&class.iri)
                .to_string()
        });
        hierarchy.insert(
            class.iri.clone(),
            ClassNode {
                iri: class.iri.clone(),
                label,
                parent_iri,
                children_iris,
                node_count,
                depth,
            },
        );
    }

    ok_json!(ClassHierarchy {
        root_classes,
        hierarchy
    })
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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
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

    // The sync QueryHandler enters a nested block_on; it must run on a blocking
    // thread like every sibling here, or it panics with "Cannot start a runtime
    // from within a runtime" on the actix worker (same defect as the 2026-08-10
    // hierarchy fix; this and add_owl_property were the last two bare calls).
    let iri_for_query = property_iri.clone();
    let result =
        execute_in_thread(move || handler.handle(GetOwlProperty { iri: iri_for_query })).await;

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
) -> Result<HttpResponse, actix_web::Error> {
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
    let result = execute_in_thread(move || handler.handle(AddOwlProperty { property })).await;

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
    let result = execute_in_thread(move || handler.handle(AddAxiom { axiom })).await;
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

    let result = execute_in_thread(move || handler.handle(RemoveAxiom { axiom_id: id })).await;

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
) -> Result<HttpResponse, actix_web::Error> {
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

    let result = execute_in_thread(move || handler.handle(StoreInferenceResults { results })).await;

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
) -> Result<HttpResponse, actix_web::Error> {
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
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    state: web::Data<AppState>,
    request: web::Json<QueryRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let query = request.into_inner().query;

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

    let handler = QueryOntologyHandler::new(state.ontology_repository.clone());

    let result = execute_in_thread(move || handler.handle(QueryOntology { query })).await;

    match result {
        Ok(Ok(results)) => {
            // ADR-117 WS-0 backstop: cap rows + serialised bytes and surface an
            // explicit `truncated` flag instead of returning an unbounded array.
            let (results, truncated) = cap_result_rows(results);
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
pub async fn sparql_query(
    state: web::Data<AppState>,
    request: web::Json<QueryRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let query = request.into_inner().query;

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

    match state.ontology_repository.sparql_select_json(query).await {
        Ok(json) => ok_json!(json),
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
) -> Result<HttpResponse, actix_web::Error> {
    match state.ontology_repository.read_inferred_graph().await {
        Ok(json) => ok_json!(json),
        Err(e) => {
            error!("Failed to read inferred graph: {}", e);
            error_json!("Failed to read inferred graph", e.to_string())
        }
    }
}

pub async fn get_ontology_metrics(
    state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
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
        let added = execute_in_thread(move || add.handle(AddOwlProperty { property: p }))
            .await
            .expect("blocking task joined");
        assert!(added.is_ok(), "add failed: {:?}", added.err());

        let get = GetOwlPropertyHandler::new(repo.clone());
        let q = iri.clone();
        let got = execute_in_thread(move || get.handle(GetOwlProperty { iri: q }))
            .await
            .expect("blocking task joined")
            .expect("query ok");
        assert_eq!(got.map(|p| p.iri), Some(iri.clone()));

        let get = GetOwlPropertyHandler::new(repo);
        let missing = execute_in_thread(move || {
            get.handle(GetOwlProperty {
                iri: "http://example.org/absent".into(),
            })
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
