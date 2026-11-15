//! Fenced derived-ontology write surface (WS-9).
//!
//! `POST /api/ontology/derived` accepts a list of quads `{graph, s, p, o}` and
//! writes them into the derived named graphs **`urn:ngm:graph:ontology:summary`**
//! and **`urn:ngm:graph:ontology:observed`** only. Any quad targeting
//! `:assert` or `:inferred` is rejected with HTTP 400 — the **fence**.
//!
//! Defence-in-depth: the fence is enforced at BOTH layers —
//!   1. here, an early 400 before any Oxigraph call, and
//!   2. in [`OxigraphOntologyRepository::append_derived_quads`], which re-checks
//!      `DERIVED_FENCE` so a future direct caller of the repo method cannot
//!      bypass the handler.
//!
//! Scope is `/ontology/derived` (NOT `/ontology`), so it does not collide with
//! the single `/ontology` scope in `ontology_handler::config`. It is mounted at
//! the `/api` level via `main.rs`, and gated `RequireAuth::power_user()` —
//! mirroring the privileged ontology scope (`ontology_handler.rs:917-918`).

use actix_web::{web, HttpResponse};
use log::warn;
use serde::Deserialize;
use serde_json::json;

use crate::middleware::RequireAuth;
use crate::settings::auth_extractor::AuthenticatedUser;
use crate::AppState;

/// Forbidden graphs — mirrored locally for the early 400. Must match
/// `DERIVED_FENCE` in `oxigraph_ontology_repository.rs`.
const FORBIDDEN_GRAPHS: [&str; 2] = [
    "urn:ngm:graph:ontology:assert",
    "urn:ngm:graph:ontology:inferred",
];

/// The only writable derived graphs.
const ALLOWED_GRAPHS: [&str; 2] = [
    "urn:ngm:graph:ontology:summary",
    "urn:ngm:graph:ontology:observed",
];

#[derive(Debug, Deserialize)]
pub struct DerivedQuad {
    pub graph: String,
    pub s: String,
    pub p: String,
    pub o: String,
}

#[derive(Debug, Deserialize)]
pub struct DerivedWriteRequest {
    pub quads: Vec<DerivedQuad>,
}

/// `POST /api/ontology/derived` — fenced quad write to `:summary`/`:observed`.
async fn write_derived(
    _auth: AuthenticatedUser,
    state: web::Data<AppState>,
    body: web::Json<DerivedWriteRequest>,
) -> HttpResponse {
    let body = body.into_inner();

    if body.quads.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "error": "empty quads",
            "detail": "derived write requires at least one quad",
        }));
    }

    // Layer-1 fence: reject before any store interaction.
    for q in &body.quads {
        if FORBIDDEN_GRAPHS.contains(&q.graph.as_str()) {
            warn!(
                "[ontology-derived] rejected fenced graph write: {}",
                q.graph
            );
            return HttpResponse::BadRequest().json(json!({
                "error": "fenced graph",
                "graph": q.graph,
                "detail": "derived writes may not target :assert or :inferred",
            }));
        }
        if !ALLOWED_GRAPHS.contains(&q.graph.as_str()) {
            return HttpResponse::BadRequest().json(json!({
                "error": "unsupported graph",
                "graph": q.graph,
                "detail": "derived writes only to :summary/:observed",
            }));
        }
    }

    let quads: Vec<(String, String, String, String)> = body
        .quads
        .into_iter()
        .map(|q| (q.graph, q.s, q.p, q.o))
        .collect();

    // Layer-2 fence + injection guards live inside append_derived_quads.
    match state.ontology_repository.append_derived_quads(quads).await {
        Ok(n) => HttpResponse::Ok().json(json!({ "success": true, "written": n })),
        Err(e) => {
            warn!("[ontology-derived] write failed: {e}");
            HttpResponse::BadRequest().json(json!({
                "success": false,
                "error": e.to_string(),
            }))
        }
    }
}

/// Outcome of a successful [`regenerate_derived`].
#[derive(Debug)]
pub(crate) struct RegenerateReport {
    /// PROV-O activity URN of the regeneration marker.
    pub activity_urn: String,
    /// Approved write-backs replayed from `enrichment_decisions`.
    pub replayed: usize,
}

/// Clear `:summary` and `:observed`, replay every committed approved write-back
/// from the persisted decisions, then write the regeneration marker.
///
/// The decide path writes `:summary` directly, so clearing the graph without a
/// replay silently deleted every approved decision's write-back. The replay
/// rebuilds each write from exactly the columns the decide path used
/// (`owner_did`, `activity_urn`, `proposal_urn`/`case_id`, `outcome`), so the
/// result is the regenerated summary plus every approved write-back.
pub(crate) async fn regenerate_derived(
    ontology: &crate::adapters::OxigraphOntologyRepository,
    enrichment: &crate::adapters::sqlite_enrichment_repository::SqliteEnrichmentRepository,
) -> Result<RegenerateReport, String> {
    // Read the decisions BEFORE clearing: if the read fails nothing is lost.
    let writebacks = enrichment
        .committed_writebacks()
        .await
        .map_err(|e| format!("could not read persisted write-backs: {e}"))?;

    ontology
        .clear_derived_graph("summary")
        .await
        .map_err(|e| format!("clear summary failed: {e}"))?;
    ontology
        .clear_derived_graph("observed")
        .await
        .map_err(|e| format!("clear observed failed: {e}"))?;

    let mut replayed = 0usize;
    for d in &writebacks {
        ontology
            .append_derived_summary(
                d.owner_did.as_deref(),
                &d.activity_urn,
                crate::handlers::enrichment_proposals_handler::summary_triples(
                    d.proposal_urn.as_deref(),
                    &d.case_id,
                    &d.outcome,
                ),
            )
            .await
            .map_err(|e| format!("replay of write-back for case {} failed: {e}", d.case_id))?;
        replayed += 1;
    }

    // Provenance marker so the regeneration is observable + idempotent.
    let activity = format!(
        "urn:visionclaw:execution:regenerate-derived-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let marker = vec![(
        "urn:ngm:derived:regeneration".to_string(),
        "https://narrativegoldmine.com/ns/v1#regeneratedAt".to_string(),
        chrono::Utc::now().to_rfc3339(),
    )];
    ontology
        .append_derived_summary(None, &activity, marker)
        .await
        .map_err(|e| format!("regenerate marker failed: {e}"))?;

    Ok(RegenerateReport {
        activity_urn: activity,
        replayed,
    })
}

/// `POST /api/ontology/derived/regenerate` — clear `:summary`/`:observed`,
/// replay approved write-backs from the persisted decisions, then re-mark. A
/// full re-derivation from a SELECT over `:assert`/`:inferred` is the WS-0
/// LIMIT-clamp follow-on; it is deliberately NOT run unbounded here.
async fn regenerate(_auth: AuthenticatedUser, state: web::Data<AppState>) -> HttpResponse {
    match regenerate_derived(
        &state.ontology_repository,
        &state.sqlite_enrichment_repository,
    )
    .await
    {
        Ok(report) => HttpResponse::Ok().json(json!({
            "success": true,
            "cleared": ["summary", "observed"],
            "replayed_writebacks": report.replayed,
            "activity_urn": report.activity_urn,
        })),
        Err(e) => {
            warn!("[ontology-derived] regenerate failed: {e}");
            HttpResponse::InternalServerError().json(json!({ "success": false, "error": e }))
        }
    }
}

/// `GET /api/ontology/derived/{which}` — read a derived graph (summary|observed).
async fn read_derived(
    _auth: AuthenticatedUser,
    state: web::Data<AppState>,
    which: web::Path<String>,
) -> HttpResponse {
    match state.ontology_repository.read_derived_graph(&which).await {
        Ok(v) => HttpResponse::Ok().json(v),
        Err(e) => HttpResponse::BadRequest().json(json!({
            "success": false,
            "error": e.to_string(),
        })),
    }
}

/// Mounts `/ontology/derived` under the caller's scope (registered at `/api`).
/// `power_user`-gated — same posture as the privileged ontology mutators.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/ontology/derived")
            .wrap(RequireAuth::power_user())
            .route("", web::post().to(write_derived))
            .route("/regenerate", web::post().to(regenerate))
            .route("/{which}", web::get().to(read_derived)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite_enrichment_repository::{
        SqliteEnrichmentRepository, StoredDecision,
    };
    use crate::adapters::OxigraphOntologyRepository;

    const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const P_DECISION: &str = "https://narrativegoldmine.com/ns/v1#enrichmentDecision";

    async fn repos(
        dir: &std::path::Path,
    ) -> (OxigraphOntologyRepository, SqliteEnrichmentRepository) {
        let onto = OxigraphOntologyRepository::open(&dir.join("oxi"))
            .await
            .expect("open oxigraph");
        let enrich = SqliteEnrichmentRepository::open(&dir.join("enrich.sqlite3"))
            .await
            .expect("open sqlite");
        (onto, enrich)
    }

    fn proposal(
        case_id: &str,
    ) -> crate::adapters::sqlite_enrichment_repository::EnrichmentProposal {
        crate::adapters::sqlite_enrichment_repository::EnrichmentProposal {
            case_id: case_id.to_string(),
            category: Some("knowledge_enrichment".into()),
            source_iri: None,
            proposal_json: json!({}),
            status: "pending".into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn decision(case_id: &str, attributed: bool, committed: bool, at: i64) -> StoredDecision {
        StoredDecision {
            case_id: case_id.to_string(),
            outcome: "approve".into(),
            attributed,
            broker_pubkey: attributed.then(|| PK.to_string()),
            reasoning: None,
            writeback_triggered: true,
            writeback_committed: committed,
            activity_urn: format!("urn:visionclaw:execution:sha256-12-{case_id}"),
            proposal_urn: attributed.then(|| format!("urn:visionclaw:kg:{PK}:{case_id}")),
            owner_did: attributed.then(|| format!("did:nostr:{PK}")),
            decided_at_ms: at,
            decision_event_id: None,
            decision_created_at_s: None,
        }
    }

    /// Persist a decision the way the decide path does, then perform its
    /// `:summary` write-back and mark it committed.
    async fn decide_and_write_back(
        onto: &OxigraphOntologyRepository,
        enrich: &SqliteEnrichmentRepository,
        d: StoredDecision,
    ) {
        enrich
            .create_or_update(&proposal(&d.case_id))
            .await
            .unwrap();
        enrich.record_decision(&d).await.unwrap();
        if d.attributed && d.writeback_triggered {
            onto.append_derived_summary(
                d.owner_did.as_deref(),
                &d.activity_urn,
                crate::handlers::enrichment_proposals_handler::summary_triples(
                    d.proposal_urn.as_deref(),
                    &d.case_id,
                    &d.outcome,
                ),
            )
            .await
            .unwrap();
            enrich
                .mark_writeback_committed(&d.case_id, &d.activity_urn, 1)
                .await
                .unwrap();
        }
    }

    async fn summary_text(onto: &OxigraphOntologyRepository) -> String {
        onto.read_derived_graph("summary")
            .await
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn regenerate_keeps_approved_writebacks() {
        let dir = tempfile::tempdir().unwrap();
        let (onto, enrich) = repos(dir.path()).await;
        decide_and_write_back(&onto, &enrich, decision("case-a", true, false, 10)).await;
        decide_and_write_back(&onto, &enrich, decision("case-b", true, false, 20)).await;
        let subject_a = format!("urn:visionclaw:kg:{PK}:case-a");
        assert!(summary_text(&onto).await.contains(&subject_a));

        let report = regenerate_derived(&onto, &enrich)
            .await
            .expect("regenerate");

        let after = summary_text(&onto).await;
        assert!(
            after.contains(&subject_a),
            "case-a write-back wiped: {after}"
        );
        assert!(after.contains(&format!("urn:visionclaw:kg:{PK}:case-b")));
        assert!(after.contains(P_DECISION));
        assert!(after.contains("regeneratedAt"), "marker present");
        assert_eq!(report.replayed, 2);
    }

    #[tokio::test]
    async fn regenerate_does_not_resurrect_unattributed_or_uncommitted_decisions() {
        let dir = tempfile::tempdir().unwrap();
        let (onto, enrich) = repos(dir.path()).await;
        // Unattributed approval: recorded, never written back.
        decide_and_write_back(&onto, &enrich, decision("case-u", false, false, 10)).await;
        // Attributed but the write never committed.
        enrich.create_or_update(&proposal("case-x")).await.unwrap();
        enrich
            .record_decision(&decision("case-x", true, false, 20))
            .await
            .unwrap();

        let report = regenerate_derived(&onto, &enrich).await.unwrap();

        assert_eq!(report.replayed, 0);
        let after = summary_text(&onto).await;
        assert!(
            !after.contains("case-u") && !after.contains("case-x"),
            "{after}"
        );
    }

    #[tokio::test]
    async fn regenerate_is_idempotent_for_writebacks() {
        let dir = tempfile::tempdir().unwrap();
        let (onto, enrich) = repos(dir.path()).await;
        decide_and_write_back(&onto, &enrich, decision("case-a", true, false, 10)).await;
        regenerate_derived(&onto, &enrich).await.unwrap();
        let once = onto.read_derived_graph("summary").await.unwrap()["triples"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t.to_string().contains("case-a"))
            .count();
        regenerate_derived(&onto, &enrich).await.unwrap();
        let twice = onto.read_derived_graph("summary").await.unwrap()["triples"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t.to_string().contains("case-a"))
            .count();
        assert_eq!(once, twice);
        assert!(once > 0);
    }
}
