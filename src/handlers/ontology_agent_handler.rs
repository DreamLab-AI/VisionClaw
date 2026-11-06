//! HTTP handler for ontology agent tools.
//!
//! Exposes the OntologyQueryService and OntologyMutationService as REST endpoints
//! that agents call via MCP tool routing. Each endpoint mirrors an MCP tool:
//!   POST /ontology-agent/discover
//!   POST /ontology-agent/read
//!   POST /ontology-agent/query
//!   POST /ontology-agent/traverse
//!   POST /ontology-agent/propose   (RETIRED — 410 Gone, ADR-2116)
//!   POST /ontology-agent/validate
//!   POST /ontology-agent/check     (ADR-2127 tri-valued membership; relation with `property`)
//!   GET  /ontology-agent/status
//!
//! ADR-2127: every answer is open-world. Each response carries `scope`
//! (`closure: "open"` + the generation it was computed against), every fact
//! carries a `basis` (asserted | inferred | provenance), and membership is
//! answered `entailed | entailed_false | not_asserted`, never a boolean.
//!
//! ADR-060: every read route takes the caller's [`HiddenClasses`] (the class IRIs of graph
//! pages hidden from them) and answers as if those classes had never been stored. Each route
//! is a thin extractor wrapper over an `answer_*` function that holds the logic.

// ADR-2116: `OntologyMutationService` and its error prefixes are no longer
// imported HERE — the retired `/propose` is the only route that ever called
// them. The service itself is untouched and still used by `decision_handler.rs`
// and `decision_service.rs`; only this handler's dependency on it is gone.
use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use crate::handlers::graph_access::caller_hidden_classes;
use crate::services::ontology_query_service::OntologyQueryService;
use crate::services::ontology_visibility::HiddenClasses;
use crate::settings::OptionalAuth;
use crate::types::ontology_tools::*;
use crate::{error_json, ok_json};
use actix::Addr;
use actix_web::{web, Error, HttpResponse};
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

fn default_limit() -> usize {
    20
}

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

fn default_depth() -> usize {
    3
}

/// ADR-2127: `{ "subject": iri, "class": iri }` — is subject ⊑ class? With
/// the optional `property`, the relation form: does subject stand in
/// `property` to `class` (accepted as `object` too)?
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRequest {
    pub subject: String,
    #[serde(alias = "object")]
    pub class: String,
    #[serde(default)]
    pub property: Option<String>,
}

// ADR-2116: `ProposeRequest` — the body of the retired `/propose` — is deleted
// rather than left parked. It had exactly one reader, and a DTO that nothing
// deserialises is a claim that the route still accepts something. `ProposeInput`
// and `AgentContext` are NOT deleted: they are `visionclaw-domain` types that
// `decision_service.rs` still uses.

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
    supervisor: web::Data<Addr<GraphServiceSupervisor>>,
    caller: OptionalAuth,
    request: web::Json<DiscoverRequest>,
) -> Result<HttpResponse, Error> {
    let hidden = caller_hidden_classes(&supervisor, &caller).await?;
    answer_discover(&query_service, request.into_inner(), &hidden).await
}

/// [`discover`] for a caller from whom `hidden` is hidden.
pub async fn answer_discover(
    query_service: &OntologyQueryService,
    req: DiscoverRequest,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, Error> {
    info!("ontology-agent/discover: query='{}'", req.query);

    match query_service
        .discover(&req.query, req.limit, req.domain.as_deref(), hidden)
        .await
    {
        Ok(results) => {
            ok_json!(serde_json::json!({
                "success": true,
                "results": results,
                "count": results.len(),
                "scope": query_service.answer_scope().await
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
    supervisor: web::Data<Addr<GraphServiceSupervisor>>,
    caller: OptionalAuth,
    request: web::Json<ReadNoteRequest>,
) -> Result<HttpResponse, Error> {
    let hidden = caller_hidden_classes(&supervisor, &caller).await?;
    answer_read_note(&query_service, request.into_inner(), &hidden).await
}

/// [`read_note`] for a caller from whom `hidden` is hidden.
pub async fn answer_read_note(
    query_service: &OntologyQueryService,
    req: ReadNoteRequest,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, Error> {
    info!("ontology-agent/read: iri='{}'", req.iri);

    match query_service.read_note(&req.iri, hidden).await {
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
    supervisor: web::Data<Addr<GraphServiceSupervisor>>,
    caller: OptionalAuth,
    request: web::Json<QueryRequest>,
) -> Result<HttpResponse, Error> {
    let hidden = caller_hidden_classes(&supervisor, &caller).await?;
    answer_query(&query_service, request.into_inner(), &hidden).await
}

/// [`query`] for a caller from whom `hidden` is hidden.
pub async fn answer_query(
    query_service: &OntologyQueryService,
    req: QueryRequest,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, Error> {
    info!(
        "ontology-agent/query: cypher='{}'",
        &req.cypher[..req.cypher.len().min(80)]
    );

    match query_service
        .validate_and_execute_cypher(&req.cypher, hidden)
        .await
    {
        Ok(validation) => {
            ok_json!(serde_json::json!({
                "success": true,
                "validation": validation,
                "scope": query_service.answer_scope().await
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
    supervisor: web::Data<Addr<GraphServiceSupervisor>>,
    caller: OptionalAuth,
    request: web::Json<TraverseRequest>,
) -> Result<HttpResponse, Error> {
    let hidden = caller_hidden_classes(&supervisor, &caller).await?;
    answer_traverse(&query_service, request.into_inner(), &hidden).await
}

/// [`traverse`] for a caller from whom `hidden` is hidden.
pub async fn answer_traverse(
    query_service: &OntologyQueryService,
    req: TraverseRequest,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, Error> {
    info!(
        "ontology-agent/traverse: start='{}', depth={}",
        req.start_iri, req.depth
    );

    // Traverse by reading the start note and following relationships; each
    // edge carries its basis and the result its open-world scope (ADR-2127).
    let result = query_service
        .traverse(
            &req.start_iri,
            req.depth,
            req.relationship_types.as_deref(),
            hidden,
        )
        .await;

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

/// POST /ontology-agent/propose — **RETIRED** (ADR-2116). Always 410 Gone.
///
/// This route was the sanctioned ontology write path: an agent posted a
/// proposal, the Whelk consistency gate ran, and an approved proposal became a
/// pull request against the corpus repository. `PRD-sovereign-corpus` §3.3
/// replaces that with a signed governance loop — `vault propose` builds a
/// `PatchProposal`, runs Whelk and the conflict detector as *blockers*, and
/// posts a forum kind-31402; a human's signed kind-31403 carries
/// `Promote{iri}` or `Demote{iri}`; agentbox applies it through a guarded
/// `vault edit`. A pull request is nobody's signature, so the PR path is not
/// deprecated-but-working — it is gone.
///
/// # Why 410, and why no auth in front of it
///
/// The route is kept, answering 410 with its replacement in the body, because
/// the callers that still hold this URL (agentbox's `ontology-propose.js`, the
/// `ontology-augment` and `podcast-knowledge-ingest` skills, the
/// `ontology-curator` agent) deserve to be told what to do instead. A 404
/// would read as a deployment fault and get retried; a 410 is terminal and
/// names its successor.
///
/// It is retired for EVERYONE, so `RequireAuth` and `RateLimit` come off with
/// it. A 401 on a route that can only refuse would tell a caller its
/// credentials were the problem, which is false, and would invite exactly the
/// retry loop the 410 exists to stop.
///
/// `OntologyMutationService` and its three error prefixes are NOT removed:
/// `decision_handler.rs` and `decision_service.rs` share them.
pub async fn propose() -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Gone().json(serde_json::json!({
        "success": false,
        "error": "route_retired",
        "message": "Ontology proposals are forum ActionRequests, not pull requests. \
                    Run `vault propose <iri> --level content|schema`: it runs Whelk \
                    and the conflict detector as blockers, then posts a kind-31402 \
                    to the forum relay for a human-signed kind-31403 decision.",
        "replacement": {
            "command": "vault propose <iri> --level content|schema [--hypothesis \"…\"] [--diff <file>]",
            "decision": "forum kind-31403 ActionResponse — Promote{iri} | Demote{iri} | Reject",
            "appliesThrough": "agentbox handleGovernanceDecision → vault edit --expect docs=1,blocks=1",
            "see": ["ADR-2116", "forum ADR-2013", "agentbox ADR-2106", "PRD-sovereign-corpus §3.3"]
        }
    })))
}

/// POST /ontology-agent/validate — Check axioms for Whelk consistency
pub async fn validate(
    query_service: web::Data<Arc<OntologyQueryService>>,
    supervisor: web::Data<Addr<GraphServiceSupervisor>>,
    caller: OptionalAuth,
    request: web::Json<ValidateRequest>,
) -> Result<HttpResponse, Error> {
    let hidden = caller_hidden_classes(&supervisor, &caller).await?;
    answer_validate(&query_service, request.into_inner(), &hidden).await
}

/// [`validate`] for a caller from whom `hidden` is hidden.
pub async fn answer_validate(
    query_service: &OntologyQueryService,
    req: ValidateRequest,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, Error> {
    info!("ontology-agent/validate: {} axioms", req.axioms.len());

    // Build Cypher-like validation by checking each axiom subject/object against known classes
    let mut all_errors = Vec::new();
    let mut all_hints = Vec::new();

    for axiom in &req.axioms {
        // Validate subject exists
        let subject_check = format!(
            "MATCH (n:{}) RETURN n",
            axiom
                .subject
                .split(':')
                .next_back()
                .unwrap_or(&axiom.subject)
        );
        if let Ok(validation) = query_service
            .validate_and_execute_cypher(&subject_check, hidden)
            .await
        {
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

/// POST /ontology-agent/check — ADR-2127 tri-valued membership or relation
/// check.
///
/// Answers `entailed` (with `basis`), `entailed_false` (with the `witness`),
/// or `not_asserted`, plus the open-world `scope`. Without `property` it is
/// the membership check `subject ⊑ class`; with it, the relation check
/// `(subject, property, class)`, where `entailed_false` needs a declared
/// domain or range the subject or object is disjoint with.
pub async fn check(
    query_service: web::Data<Arc<OntologyQueryService>>,
    supervisor: web::Data<Addr<GraphServiceSupervisor>>,
    caller: OptionalAuth,
    request: web::Json<CheckRequest>,
) -> Result<HttpResponse, Error> {
    let hidden = caller_hidden_classes(&supervisor, &caller).await?;
    answer_check(&query_service, request.into_inner(), &hidden).await
}

/// [`check`] for a caller from whom `hidden` is hidden.
pub async fn answer_check(
    query_service: &OntologyQueryService,
    req: CheckRequest,
    hidden: &HiddenClasses,
) -> Result<HttpResponse, Error> {
    if let Some(property) = req.property.as_deref().filter(|p| !p.trim().is_empty()) {
        info!(
            "ontology-agent/check: subject='{}', property='{}', object='{}'",
            req.subject, property, req.class
        );
        return match query_service
            .check_relation(&req.subject, property, &req.class, hidden)
            .await
        {
            Ok(check) => {
                ok_json!(serde_json::json!({
                    "success": true,
                    "check": check
                }))
            }
            Err(e) => {
                error!("ontology-agent/check (relation) failed: {}", e);
                error_json!("Relation check failed", e)
            }
        };
    }
    info!(
        "ontology-agent/check: subject='{}', class='{}'",
        req.subject, req.class
    );

    match query_service
        .check_membership(&req.subject, &req.class, hidden)
        .await
    {
        Ok(check) => {
            ok_json!(serde_json::json!({
                "success": true,
                "check": check
            }))
        }
        Err(e) => {
            error!("ontology-agent/check failed: {}", e);
            error_json!("Membership check failed", e)
        }
    }
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
            // ADR-2116: `ontology_propose` is gone from this surface. Writes go
            // through `vault propose` and a human-signed forum 31403, so
            // advertising it here would point an agent at a 410.
            "ontology_validate".to_string(),
            "ontology_check".to_string(),
            // ADR-2127 decision 2: `/check` with `property` answers the
            // tri-valued relation check.
            "ontology_check_relation".to_string(),
        ],
    })
}

// ---------- Helpers ----------

// The traversal walk moved to `OntologyQueryService::traverse` (ADR-2127) so
// its edges' basis and its scope are produced, and tested, with the service.

// ---------- Route Configuration ----------

/// WS-1 / ADR-120 as amended by ADR-2116: every route in the `/ontology-agent`
/// surface is now anonymous, because every route in it is now read-only. The
/// nested `/propose` sub-scope and its `RequireAuth` + `RateLimit` middleware
/// are gone with the write path they guarded; `/propose` remains as a plain
/// route that answers 410 Gone and names its replacement.
pub fn configure_ontology_agent_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/ontology-agent")
            .route("/discover", web::post().to(discover))
            .route("/read", web::post().to(read_note))
            .route("/query", web::post().to(query))
            .route("/traverse", web::post().to(traverse))
            .route("/validate", web::post().to(validate))
            .route("/check", web::post().to(check))
            .route("/status", web::get().to(status))
            // ADR-2116: /propose is retired and answers 410 Gone to every
            // caller, so it carries no auth or rate-limit middleware — it
            // reaches no service and mutates nothing.
            .route("/propose", web::post().to(propose)),
    );
}

/// ADR-060: a class hidden from the caller answers on every route exactly as a class that was
/// never stored. `mv:Secret` is the hidden class; `mv:Nowhere` was never stored.
#[cfg(test)]
mod hidden_class_tests {
    use super::*;
    use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
    use crate::services::ontology_visibility::testing::{mentions, reply, sorted};
    use crate::services::schema_service::SchemaService;
    use crate::test_helpers::MockOntologyRepository;
    use std::collections::{HashMap, HashSet};
    use tokio::sync::RwLock;
    use visionclaw_domain::ports::inference_engine::InferenceEngine;
    use visionclaw_domain::ports::ontology_repository::{
        AxiomType, OntologyRepository, OwlAxiom, OwlClass, OwlProperty,
    };

    const HIDDEN: &str = "mv:Secret";
    const NEVER: &str = "mv:Nowhere";
    const HAS_PART: &str = "https://narrativegoldmine.com/ns/v1#hasPart";
    /// Each probe IRI is echoed as the same placeholder, so only what differs beyond the echo
    /// counts.
    const ECHOES: &[(&str, &str)] = &[(HIDDEN, "<probe>"), (NEVER, "<probe>")];

    fn class(iri: &str, term: &str, parents: &[&str]) -> OwlClass {
        OwlClass {
            iri: iri.to_string(),
            label: Some(term.to_string()),
            preferred_term: Some(term.to_string()),
            description: Some(format!("{term} described")),
            parent_classes: parents.iter().map(|p| p.to_string()).collect(),
            markdown_content: Some(format!("- {term}")),
            ..OwlClass::default()
        }
    }

    fn axiom(axiom_type: AxiomType, subject: &str, object: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type,
            subject: subject.to_string(),
            object: object.to_string(),
            annotations: HashMap::new(),
        }
    }

    /// Agent ⊒ Organization; Person; and, when `with_hidden`, Secret ⊑ Organization, Secret
    /// disjoint-with Person, Organization hasPart Secret, hasPart's range Secret.
    async fn service(with_hidden: bool) -> OntologyQueryService {
        let repo = Arc::new(MockOntologyRepository::new());
        let mut classes = vec![
            class("mv:Agent", "Agent", &[]),
            class("mv:Organization", "Organization", &["mv:Agent"]),
            class("mv:Person", "Person", &[]),
        ];
        let mut axioms = vec![axiom(AxiomType::SubClassOf, "mv:Organization", "mv:Agent")];
        let mut range = Vec::new();
        if with_hidden {
            classes.push(class(HIDDEN, "Secretariat", &["mv:Organization"]));
            axioms.push(axiom(AxiomType::SubClassOf, HIDDEN, "mv:Organization"));
            axioms.push(axiom(AxiomType::DisjointWith, HIDDEN, "mv:Person"));
            let mut part = axiom(
                AxiomType::ObjectPropertyAssertion,
                "mv:Organization",
                HIDDEN,
            );
            part.annotations
                .insert("predicate".to_string(), HAS_PART.to_string());
            axioms.push(part);
            range.push(HIDDEN.to_string());
        }
        for c in &classes {
            repo.add_owl_class(c).await.expect("class stored");
        }
        for a in &axioms {
            repo.add_axiom(a).await.expect("axiom stored");
        }
        repo.add_owl_property(&OwlProperty {
            iri: HAS_PART.to_string(),
            range,
            ..OwlProperty::default()
        })
        .await
        .expect("property stored");
        let mut engine = WhelkInferenceEngine::new();
        engine
            .load_ontology(classes, axioms)
            .await
            .expect("whelk loads");
        engine.infer().await.expect("whelk classifies");
        OntologyQueryService::new(
            repo,
            Arc::new(RwLock::new(engine)),
            Arc::new(SchemaService::new()),
        )
        .with_generation(Some("urn:gen:hidden-class-tests".to_string()))
    }

    fn hidden() -> HiddenClasses {
        HiddenClasses::from(HashSet::from([HIDDEN.to_string()]))
    }

    fn read(iri: &str) -> ReadNoteRequest {
        ReadNoteRequest {
            iri: iri.to_string(),
        }
    }

    fn discover_req(query: &str) -> DiscoverRequest {
        DiscoverRequest {
            query: query.to_string(),
            limit: 20,
            domain: None,
        }
    }

    fn traverse_req(start: &str) -> TraverseRequest {
        TraverseRequest {
            start_iri: start.to_string(),
            depth: 3,
            relationship_types: None,
        }
    }

    fn check_req(subject: &str, class: &str, property: Option<&str>) -> CheckRequest {
        CheckRequest {
            subject: subject.to_string(),
            class: class.to_string(),
            property: property.map(str::to_string),
        }
    }

    fn validate_req(subject: &str) -> ValidateRequest {
        ValidateRequest {
            axioms: vec![AxiomInput {
                axiom_type: "SubClassOf".to_string(),
                subject: subject.to_string(),
                object: "mv:Agent".to_string(),
            }],
        }
    }

    #[tokio::test]
    async fn read_of_a_hidden_class_is_the_unknown_class_reply() {
        let s = service(true).await;
        let hidden_probe = reply(answer_read_note(&s, read(HIDDEN), &hidden()).await, ECHOES).await;
        let never_probe = reply(answer_read_note(&s, read(NEVER), &hidden()).await, ECHOES).await;
        assert_eq!(hidden_probe, never_probe);
    }

    #[tokio::test]
    async fn read_of_a_visible_class_omits_the_hidden_one_from_axioms_and_related_notes() {
        let got = reply(
            answer_read_note(&service(true).await, read("mv:Organization"), &hidden()).await,
            &[],
        )
        .await;
        let never_stored = reply(
            answer_read_note(
                &service(false).await,
                read("mv:Organization"),
                &HiddenClasses::none(),
            )
            .await,
            &[],
        )
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, "mv:Agent"), "the visible parent remains");
        assert_eq!(
            (got.0, sorted(got.1)),
            (never_stored.0, sorted(never_stored.1))
        );
    }

    #[tokio::test]
    async fn discover_never_finds_or_expands_into_a_hidden_class() {
        let s = service(true).await;
        let by_term = reply(
            answer_discover(&s, discover_req("Secretariat"), &hidden()).await,
            &[],
        )
        .await;
        let by_nothing = reply(
            answer_discover(&s, discover_req("Nowhereland"), &hidden()).await,
            &[],
        )
        .await;
        assert_eq!(by_term, by_nothing, "a hidden term matches like no term");

        let got = reply(
            answer_discover(&s, discover_req("Organization"), &hidden()).await,
            &[],
        )
        .await;
        let never_stored = reply(
            answer_discover(
                &service(false).await,
                discover_req("Organization"),
                &HiddenClasses::none(),
            )
            .await,
            &[],
        )
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, "mv:Organization"));
        assert_eq!(got, never_stored);
    }

    #[tokio::test]
    async fn query_validation_and_its_hints_never_name_a_hidden_class() {
        for cypher in [
            "MATCH (n:Secretariat) RETURN n",
            "MATCH (n:Secretariot) RETURN n",
        ] {
            let req = || QueryRequest {
                cypher: cypher.to_string(),
            };
            let got = reply(
                answer_query(&service(true).await, req(), &hidden()).await,
                &[],
            )
            .await;
            let never_stored = reply(
                answer_query(&service(false).await, req(), &HiddenClasses::none()).await,
                &[],
            )
            .await;
            assert!(!mentions(&got.1, HIDDEN), "{cypher}: {}", got.1);
            assert_eq!(got, never_stored, "{cypher}");
        }
    }

    #[tokio::test]
    async fn traverse_from_a_hidden_class_is_the_unknown_start_and_never_walks_into_one() {
        let s = service(true).await;
        let hidden_start = reply(
            answer_traverse(&s, traverse_req(HIDDEN), &hidden()).await,
            ECHOES,
        )
        .await;
        let never_start = reply(
            answer_traverse(&s, traverse_req(NEVER), &hidden()).await,
            ECHOES,
        )
        .await;
        assert_eq!(hidden_start, never_start);

        let got = reply(
            answer_traverse(&s, traverse_req("mv:Organization"), &hidden()).await,
            &[],
        )
        .await;
        let never_stored = reply(
            answer_traverse(
                &service(false).await,
                traverse_req("mv:Organization"),
                &HiddenClasses::none(),
            )
            .await,
            &[],
        )
        .await;
        assert!(!mentions(&got.1, HIDDEN), "{}", got.1);
        assert!(mentions(&got.1, "mv:Agent"));
        assert_eq!(
            (got.0, sorted(got.1)),
            (never_stored.0, sorted(never_stored.1))
        );
    }

    #[tokio::test]
    async fn validate_treats_a_hidden_class_as_unknown_and_never_hints_it() {
        let got = reply(
            answer_validate(&service(true).await, validate_req(HIDDEN), &hidden()).await,
            &[],
        )
        .await;
        let never_stored = reply(
            answer_validate(
                &service(false).await,
                validate_req(HIDDEN),
                &HiddenClasses::none(),
            )
            .await,
            &[],
        )
        .await;
        assert_eq!(got.1["data"]["valid"], false, "{}", got.1);
        assert!(!mentions(&got.1["data"]["hints"], HIDDEN), "{}", got.1);
        assert_eq!(got, never_stored);
    }

    #[tokio::test]
    async fn check_on_a_hidden_subject_or_object_is_the_unknown_answer() {
        let s = service(true).await;
        for (subject, class, property, never_subject, never_class) in [
            (HIDDEN, "mv:Organization", None, NEVER, "mv:Organization"),
            ("mv:Person", HIDDEN, None, "mv:Person", NEVER),
            (
                "mv:Organization",
                HIDDEN,
                Some(HAS_PART),
                "mv:Organization",
                NEVER,
            ),
        ] {
            let got = reply(
                answer_check(&s, check_req(subject, class, property), &hidden()).await,
                ECHOES,
            )
            .await;
            let unknown = reply(
                answer_check(
                    &s,
                    check_req(never_subject, never_class, property),
                    &hidden(),
                )
                .await,
                ECHOES,
            )
            .await;
            assert_eq!(got, unknown, "{subject} {property:?} {class}");
        }
    }

    #[tokio::test]
    async fn with_full_access_the_class_is_served_as_before() {
        let s = service(true).await;
        let note = reply(
            answer_read_note(&s, read(HIDDEN), &HiddenClasses::none()).await,
            &[],
        )
        .await;
        assert_eq!(note.0, 200);
        assert!(mentions(&note.1, "Secretariat"));
        let check = reply(
            answer_check(
                &s,
                check_req(HIDDEN, "mv:Organization", None),
                &HiddenClasses::none(),
            )
            .await,
            &[],
        )
        .await;
        assert_eq!(
            check.1["data"]["check"]["verdict"], "entailed",
            "{}",
            check.1
        );
        let found = reply(
            answer_discover(&s, discover_req("Secretariat"), &HiddenClasses::none()).await,
            &[],
        )
        .await;
        assert!(mentions(&found.1, HIDDEN));
    }
}
