//! Broker inbox read surface — serves the agentbox broker-bridge.
//!
//! `agentbox/management-api/routes/broker-bridge.js` (the S12 enrichment-review
//! pane bridge) issues three read calls into the VisionClaw substrate that, on
//! `main`, have **no** Rust route:
//!
//!   * `GET /api/broker/inbox`            (broker-bridge.js:224) → `{cases, total}`
//!   * `GET /api/broker/cases/{id}`       (broker-bridge.js:293) → one case
//!   * `GET /api/broker/cases/{id}/history` (broker-bridge.js:641) → decision log
//!
//! WS-12 supplies the first two. The decide *write* path already lands in
//! [`crate::handlers::enrichment_proposals_handler`] (WS-9); the bridge POSTs
//! decisions there at `/api/enrichment-proposals/{id}/decide`. This module is
//! the matching *read* side and it draws from the **same durable store WS-9
//! introduces** — it does not open a second store. It reads through the public
//! accessors on [`enrichment_proposals_handler::store`] so there is exactly one
//! source of truth for proposals across the read and write surfaces.
//!
//! Shape contract (broker-bridge.js):
//!   - inbox top-level: `{ "cases": [..], "total": N }` (broker-bridge.js:233)
//!   - each case the bridge consumes (broker-bridge.js:143-179): `id`,
//!     `category`, `status`, `metadata` { target_path, content, provenance,
//!     proposed_by, reasoning_summary, reasoning_hash, agent_identity,
//!     broker_did, enrichment_type }. The bridge filters to
//!     `category == "knowledge_enrichment"` and `status ∈ {pending,claimed,decided}`.
//!
//! Gating: the broker inbox is a privileged review surface. The agentbox bridge
//! is a service-to-service caller that already presents `X-Agent-Key`
//! (broker-bridge.js:85, the same credential the WS-9 decide route requires via
//! `require_agent_key`). We gate the scope at `RequireAuth::power_user()` to
//! mirror the ontology privileged-scope posture (`ontology_handler.rs:918`) and
//! accept the service credential through the same auth middleware path. Reads
//! are non-mutating, so the whole scope is gated (not `mutations_only`).

use actix_web::{web, HttpResponse};
use log::debug;
use serde::Serialize;

use crate::handlers::enrichment_proposals_handler::store::{self, EnrichmentProposal, ProposalStatus};
use crate::middleware::RequireAuth;
use std::sync::OnceLock;
use visionclaw_jev::proposal_triage::{self, ProposalFacts, Triage, TriageCache};
use visionclaw_jev::{Item as JevItem, Jev};

/// Top-level inbox envelope — the exact shape broker-bridge.js:233 destructures
/// (`inbox.cases || inbox.items`, `inbox.total`). We emit `cases` + `total`.
#[derive(Debug, Serialize)]
struct InboxResponse {
    cases: Vec<BrokerCase>,
    total: usize,
}

/// One broker case, projected from an [`EnrichmentProposal`]. Field names match
/// what broker-bridge.js reads off each case object verbatim, so the bridge's
/// `_enrichCase` (broker-bridge.js:142) and category/status filters
/// (broker-bridge.js:240-246) operate without translation.
#[derive(Debug, Serialize)]
struct BrokerCase {
    /// `c.id` (broker-bridge.js:257).
    id: String,
    /// `c.category` — fixed to `knowledge_enrichment` so the bridge's
    /// `ENRICHMENT_CATEGORIES` filter (broker-bridge.js:60,240) keeps the case.
    category: String,
    /// `c.status` — one of `pending` | `claimed` | `decided`
    /// (broker-bridge.js:204 enum, :246 filter).
    status: String,
    /// `c.metadata` — the bag `_enrichCase` reads source/provenance fields from
    /// (broker-bridge.js:143-179).
    metadata: CaseMetadata,
    /// Echoed for the single-case view; harmless extra field on the list view.
    created_at_ms: u64,
    decided_at_ms: Option<u64>,
    /// Advisory triage from the opt-in Jev option (VC-24-S1-01). Absent from
    /// the JSON unless `VISIONCLAW_JEV_PROPOSAL_TRIAGE=1` produced one, so the
    /// default wire shape is unchanged. It never carries authority: decide
    /// routes, signed decisions and receipts do not read it.
    #[serde(skip_serializing_if = "Option::is_none")]
    jev: Option<Triage>,
}

/// Metadata bag — keys mirror the lookups in `_enrichCase` (broker-bridge.js:146
/// `target_path`; :153 `content`; :164 `provenance`; :169-173 provenance subkeys;
/// :178 `enrichment_type`). Unset keys serialise to `null`, which the bridge's
/// `||` fallbacks already tolerate.
#[derive(Debug, Serialize)]
struct CaseMetadata {
    /// Source file path. broker-bridge.js:146 reads `target_path` first.
    target_path: Option<String>,
    /// Proposed enrichment body. broker-bridge.js:153 reads `content` first.
    content: Option<String>,
    /// Classification (broker-bridge.js:178).
    enrichment_type: Option<String>,
    /// Proposing agent's did:nostr — broker-bridge.js:169 `proposed_by` / `agent_did`.
    proposed_by: Option<String>,
    agent_did: Option<String>,
    agent_identity: Option<String>,
    /// Reasoning trailer (broker-bridge.js:170-171).
    reasoning_summary: Option<String>,
    reasoning_hash: Option<String>,
    /// Broker DID that handled the case (broker-bridge.js:173).
    broker_did: Option<String>,
    /// PROV-O proposal URN (owner-scoped, content-addressed) when attributed.
    proposal_urn: Option<String>,
    /// PROV-O activity URN of the decision, when decided.
    activity_urn: Option<String>,
    /// FR2.3 (EXP-AC-002) — the agent's SELF-DECLARED tier. The case queue uses
    /// it to decide whether a typed rationale is mandatory, and renders it BELOW
    /// the decision controls so it cannot anchor the reviewer.
    risk_tier: Option<String>,
    /// FR2.4 — the agent's SELF-ASSESSED confidence. `null` when no model
    /// produced one; the UI then renders nothing (absence as absence).
    confidence: Option<f64>,
    /// FR2.3 — generation provenance (`model`, `source_excerpt`, `confidence`)
    /// as recorded, or `null`. Rendered field by field, only where present.
    provenance: Option<serde_json::Value>,
}

impl From<&EnrichmentProposal> for BrokerCase {
    fn from(p: &EnrichmentProposal) -> Self {
        BrokerCase {
            id: p.id.clone(),
            // The bridge's enrichment-review pane only renders this category;
            // every proposal in the WS-9 store is a knowledge enrichment.
            category: "knowledge_enrichment".to_string(),
            status: p.status.as_str().to_string(),
            metadata: CaseMetadata {
                target_path: p.target_path.clone(),
                content: p.content.clone(),
                enrichment_type: p.enrichment_type.clone(),
                proposed_by: p.proposer_did.clone(),
                agent_did: p.proposer_did.clone(),
                agent_identity: p.proposer_did.clone(),
                reasoning_summary: p.reasoning_summary.clone(),
                reasoning_hash: p.reasoning_hash.clone(),
                broker_did: p.broker_did.clone(),
                proposal_urn: p.proposal_urn.clone(),
                activity_urn: p.activity_urn.clone(),
                risk_tier: p.risk_tier.clone(),
                confidence: p.confidence,
                provenance: p.provenance.clone(),
            },
            created_at_ms: p.created_at_ms,
            decided_at_ms: p.decided_at_ms,
            jev: None,
        }
    }
}

/// The opt-in Jev triage (VC-24-S1-01), resolved once from the environment.
struct JevTriage {
    jev: Jev,
    params: proposal_triage::Params,
    cache: TriageCache,
}

static JEV_TRIAGE: OnceLock<Option<JevTriage>> = OnceLock::new();

fn jev_triage() -> Option<&'static JevTriage> {
    JEV_TRIAGE
        .get_or_init(|| {
            Jev::from_env_logged(JevItem::ProposalTriage, proposal_triage::QSET_VERSION).map(|jev| JevTriage {
                jev,
                params: proposal_triage::Params::from_lookup(&|k| std::env::var(k).ok()),
                cache: TriageCache::new(),
            })
        })
        .as_ref()
}

/// The facts Jev may see for a proposal; `None` for a decided case (only the
/// exact `pending`/`claimed` states are triaged).
fn triage_facts(p: &EnrichmentProposal) -> Option<ProposalFacts> {
    matches!(p.status, ProposalStatus::Pending | ProposalStatus::Claimed).then(|| ProposalFacts {
        id: p.id.clone(),
        status: p.status.as_str().to_string(),
        target_path: p.target_path.clone(),
        enrichment_type: p.enrichment_type.clone(),
        content: p.content.clone(),
        rationale: p.reasoning_summary.clone(),
        source_excerpt: p
            .provenance
            .as_ref()
            .and_then(|v| v.get("source_excerpt"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// Project proposals into the inbox. With no triage state, or when every Jev
/// call falls back, the result is exactly the pre-change projection (same
/// cases, same order, no `jev` key). Otherwise annotated cases are ordered by
/// triage priority within the slots annotated cases already occupy.
async fn assemble_inbox(proposals: Vec<EnrichmentProposal>, triage: Option<&JevTriage>) -> InboxResponse {
    let total = proposals.len();
    let Some(t) = triage else {
        let cases = proposals.iter().map(BrokerCase::from).collect();
        return InboxResponse { cases, total };
    };
    let (ordered, ann) =
        proposal_triage::triage_queue(Some(&t.jev), &t.cache, proposals, triage_facts, &t.params).await;
    let cases = ordered
        .iter()
        .zip(ann)
        .map(|(p, a)| {
            let mut c = BrokerCase::from(p);
            c.jev = a;
            c
        })
        .collect();
    InboxResponse { cases, total }
}

/// `GET /api/broker/inbox` → `{ cases: [..], total: N }`.
///
/// Reads every proposal from the WS-9 durable store and projects it into the
/// broker-case shape. The bridge does its own category/status filtering and
/// pagination (broker-bridge.js:239-252), so this returns the full set; `total`
/// is the unfiltered count the bridge reports as `total` upstream
/// (broker-bridge.js:236,264).
pub async fn inbox() -> HttpResponse {
    let proposals = store::all().await;
    let body = assemble_inbox(proposals, jev_triage()).await;
    debug!("[broker-inbox] serving {} proposal(s) to bridge", body.total);
    HttpResponse::Ok().json(body)
}

/// `GET /api/broker/cases/{id}` → one enriched case (broker-bridge.js:293).
/// 404 when the proposal id is unknown, so the bridge surfaces a clean upstream
/// error (broker-bridge.js:296) instead of a malformed body.
pub async fn case_by_id(path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    match store::get(&id).await {
        Some(p) => HttpResponse::Ok().json(BrokerCase::from(&p)),
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": "not-found",
            "message": format!("no broker case with id {id}"),
        })),
    }
}

/// Register the broker read surface under `/api`.
///
/// Mounted as a dedicated `web::scope("/broker")` so the privileged
/// `RequireAuth::power_user()` middleware wraps exactly these read routes and
/// nothing else — mirroring the isolated privileged scope at
/// `ontology_handler.rs:913-918`. The agentbox bridge authenticates through the
/// same middleware via its `X-Agent-Key` service credential.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/broker")
            .wrap(RequireAuth::power_user())
            .route("/inbox", web::get().to(inbox))
            .route("/cases/{id}", web::get().to(case_by_id))
            // REC-2 / D3 (PRD-023 WP-4): the control-centre operator decide
            // path. Power-user-gated by the surrounding scope; funnels through
            // the same decision core as the agentbox `X-Agent-Key` route.
            .route(
                "/cases/{id}/decide",
                web::post().to(crate::handlers::enrichment_proposals_handler::decide_as_operator),
            ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::enrichment_proposals_handler::store::ProposalStatus;

    fn sample() -> EnrichmentProposal {
        EnrichmentProposal {
            id: "case-7".into(),
            status: ProposalStatus::Pending,
            target_path: Some("knowledge/foo.md".into()),
            content: Some("proposed body".into()),
            enrichment_type: Some("wikilink".into()),
            proposer_did: Some("did:nostr:aaaa".into()),
            reasoning_summary: Some("relates A to B".into()),
            reasoning_hash: Some("sha256-12-deadbeef0000".into()),
            broker_did: None,
            proposal_urn: Some("urn:visionclaw:kg:aaaa:sha256-12-abc".into()),
            activity_urn: None,
            risk_tier: None,
            confidence: None,
            provenance: None,
            created_at_ms: 1_700_000_000_000,
            decided_at_ms: None,
        }
    }

    #[test]
    fn a_proposal_with_no_self_assessment_projects_absence_not_a_default() {
        // FR2.4 (EXP-AC-002): the queue must be able to render nothing. A `0.5`
        // here would reach the reviewer as the agent's own confidence.
        let c = BrokerCase::from(&sample());
        assert_eq!(c.metadata.confidence, None);
        assert_eq!(c.metadata.risk_tier, None);
        assert_eq!(c.metadata.provenance, None);
        // The age clock the queue sorts and badges on (FR6.5).
        assert_eq!(c.created_at_ms, 1_700_000_000_000);
    }

    #[test]
    fn self_assessment_and_provenance_pass_through_when_the_agent_produced_them() {
        let mut p = sample();
        p.risk_tier = Some("critical".into());
        p.confidence = Some(0.82);
        p.provenance = Some(serde_json::json!({
            "model": "qwen3.8-27B",
            "source_excerpt": "the seed sentence",
        }));
        let c = BrokerCase::from(&p);
        assert_eq!(c.metadata.risk_tier.as_deref(), Some("critical"));
        assert_eq!(c.metadata.confidence, Some(0.82));
        assert_eq!(
            c.metadata.provenance.as_ref().unwrap()["model"],
            serde_json::json!("qwen3.8-27B")
        );
    }

    #[test]
    fn projects_proposal_into_bridge_case_shape() {
        let p = sample();
        let c = BrokerCase::from(&p);
        // Category is fixed so the bridge ENRICHMENT_CATEGORIES filter keeps it.
        assert_eq!(c.category, "knowledge_enrichment");
        assert_eq!(c.status, "pending");
        assert_eq!(c.id, "case-7");
        // metadata keys the bridge reads in _enrichCase.
        assert_eq!(c.metadata.target_path.as_deref(), Some("knowledge/foo.md"));
        assert_eq!(c.metadata.content.as_deref(), Some("proposed body"));
        assert_eq!(c.metadata.proposed_by.as_deref(), Some("did:nostr:aaaa"));
        assert_eq!(c.metadata.enrichment_type.as_deref(), Some("wikilink"));
    }

    #[test]
    fn inbox_envelope_carries_cases_and_total() {
        // Serialised shape must expose `cases` and `total` (broker-bridge.js:233).
        let resp = InboxResponse {
            cases: vec![BrokerCase::from(&sample())],
            total: 1,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v.get("cases").and_then(|c| c.as_array()).is_some());
        assert_eq!(v.get("total").and_then(|t| t.as_u64()), Some(1));
    }
    // ---- VC-24-S1-01: the opt-in Jev triage -------------------------------
    use visionclaw_jev::client::testing::{jev as test_jev, FnTransport, MockTransport, Script};
    use visionclaw_jev::client::{RawResponse, TransportError};

    fn case(id: &str, status: ProposalStatus, change: &str) -> EnrichmentProposal {
        let mut p = sample();
        p.id = id.into();
        p.status = status;
        p.content = Some(change.into());
        p
    }

    fn queue() -> Vec<EnrichmentProposal> {
        vec![
            case("low", ProposalStatus::Pending, "change low"),
            case("done", ProposalStatus::Decided, "change done"),
            case("high", ProposalStatus::Claimed, "change high"),
        ]
    }

    fn answers(c: f64, r: f64, u: f64, conf: f64) -> String {
        serde_json::json!({"model":"m","usage":{"input_tokens":1,"output_tokens":1},"answers":{
            "completeness":{"type":"score","score":c,"confidence":conf},
            "relevance":{"type":"score","score":r,"confidence":conf},
            "unsupported":{"type":"noul","noul":u}}})
        .to_string()
    }

    fn state_with(t: std::sync::Arc<dyn visionclaw_jev::Transport>) -> JevTriage {
        JevTriage {
            jev: test_jev(JevItem::ProposalTriage, t),
            params: proposal_triage::Params::default(),
            cache: TriageCache::new(),
        }
    }

    fn ids(r: &InboxResponse) -> Vec<String> {
        r.cases.iter().map(|c| c.id.clone()).collect()
    }

    #[tokio::test]
    async fn jev_off_is_the_pre_change_projection_exactly() {
        let baseline = InboxResponse { cases: queue().iter().map(BrokerCase::from).collect(), total: 3 };
        let out = assemble_inbox(queue(), None).await;
        assert_eq!(serde_json::to_value(&out).unwrap(), serde_json::to_value(&baseline).unwrap());
        assert!(serde_json::to_string(&out).unwrap().find("\"jev\"").is_none(), "no jev key when off");
    }

    #[tokio::test]
    async fn jev_every_fallback_is_the_pre_change_projection_exactly() {
        let baseline = serde_json::to_value(InboxResponse { cases: queue().iter().map(BrokerCase::from).collect(), total: 3 }).unwrap();
        for script in [
            Script::Reply(500, "x".into()),
            Script::Reply(429, "x".into()),
            Script::Reply(200, "garbage".into()),
            Script::Reply(200, "{\"answers\":{\"completeness\":{\"type\":\"score\",\"score\":1,\"confidence\":0.9}}}".into()),
            Script::Reply(200, answers(3.0, 2.0, 0.9, 0.0)),
            Script::Fail(TransportError::Other),
            Script::Fail(TransportError::Timeout),
            Script::Hang,
        ] {
            let st = state_with(MockTransport::new(script));
            let out = assemble_inbox(queue(), Some(&st)).await;
            assert_eq!(serde_json::to_value(&out).unwrap(), baseline);
        }
    }

    #[tokio::test]
    async fn jev_accept_path_annotates_orders_and_leaves_decided_cases_alone() {
        let t = FnTransport::new(|body| {
            let change = body["state"]["requested_change"].as_str().unwrap_or("");
            let b = if change.contains("low") { answers(0.0, 0.0, 0.1, 0.9) } else { answers(3.0, 2.0, 0.95, 0.9) };
            Ok(RawResponse { status: 200, body: b })
        });
        let st = state_with(t.clone());
        let out = assemble_inbox(queue(), Some(&st)).await;
        assert_eq!(ids(&out), ["high", "done", "low"]);
        assert_eq!(out.total, 3);
        assert_eq!(t.calls(), 2, "the decided case is never sent");
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["cases"][0]["jev"]["unsupportedFlag"], true);
        assert!(v["cases"][1].get("jev").is_none());
        // status and decide-relevant fields are untouched by triage
        assert_eq!(out.cases[0].status, "claimed");
        assert_eq!(out.cases[1].status, "decided");
    }

    #[test]
    fn only_pending_and_claimed_cases_expose_facts() {
        assert!(triage_facts(&case("a", ProposalStatus::Pending, "x")).is_some());
        assert!(triage_facts(&case("a", ProposalStatus::Claimed, "x")).is_some());
        assert!(triage_facts(&case("a", ProposalStatus::Decided, "x")).is_none());
    }
}
