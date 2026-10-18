//! Git-ingest write-back endpoint — `POST /api/ingest/writeback` (GOV-4).
//!
//! The agentbox git-bridge (`management-api/routes/git-bridge.js:733`) POSTs an
//! approved-enrichment write-back here after a broker approve. On `main` this
//! route was never registered, so the git-bridge's WriteBackSaga call 404'd and
//! the approve→write-back loop was broken (GOV-4). ADR-130 Decision 2 made the
//! enrichment-decide handler the single decision core; this endpoint adapts the
//! git-bridge write-back payload onto that core so the loop closes on the
//! architecture `main` actually runs.
//!
//! It does NOT re-implement a git-push saga (the `crashbug`/PRD-013
//! `writeback_saga.rs` was never on `main`). It funnels the decision through
//! [`apply_decision`](crate::handlers::enrichment_proposals_handler::apply_decision),
//! which records the durable decision, performs the fenced Oxigraph `:summary`
//! write-back on an attributed approval, emits `broker:case_decided`, and
//! projects the decision back to the forum (gap-close item 2). The ontology-PR
//! form of write-back is owned by `ElevationActor` on approve of its own cases.
//!
//! Auth: the git-bridge's `vcFetch` sends no `X-Agent-Key`, so this route is not
//! agent-key gated (matching the documented PRD-013 `/api/ingest/*` posture). A
//! real KG write only happens on an *attributed* approval — `approvedBy` must be
//! a canonical `did:nostr`/hex pubkey — so an owner-less body records the
//! decision but writes no fact (the enrichment-decide invariant).

use actix_web::{web, HttpResponse};
use serde::Deserialize;

use crate::actors::ClientCoordinatorActor;
use crate::handlers::enrichment_proposals_handler::{apply_decision, BrokerDecisionRequest};
use crate::AppState;

/// The `decision` block of the git-bridge write-back payload
/// (`git-bridge.js:722-731`).
#[derive(Debug, Clone, Deserialize)]
pub struct WritebackDecision {
    #[serde(rename = "caseId")]
    pub case_id: String,
    /// The verdict — always `"approve"` from the git-bridge (it only calls the
    /// write-back on approve), but accept any verb the decide core classifies.
    pub decision: String,
    #[serde(default, rename = "approvedBy")]
    pub approved_by: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
}

/// The git-bridge write-back payload. `remoteId` / `enrichment` describe the
/// git-push side the (never-merged) saga owned; the decide core consumes the
/// `decision` block. The extra fields are accepted and ignored, not rejected.
#[derive(Debug, Clone, Deserialize)]
pub struct WritebackRequest {
    #[serde(default, rename = "remoteId")]
    pub remote_id: Option<String>,
    #[serde(default)]
    pub enrichment: Option<serde_json::Value>,
    pub decision: WritebackDecision,
}

/// Normalise an `approvedBy` field to a bare x-only hex pubkey for attribution,
/// stripping a `did:nostr:` prefix if present. Returns `None` when the value is
/// absent or not a canonical pubkey (→ recorded unattributed, no KG write).
fn attribution_pubkey(approved_by: Option<String>) -> Option<String> {
    let raw = approved_by?;
    let candidate = raw.strip_prefix("did:nostr:").unwrap_or(&raw);
    if crate::uri::is_pubkey_hex(candidate) {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// `POST /api/ingest/writeback` — adapt the git-bridge write-back onto the shared
/// enrichment-decide core (GOV-4). Reachability guarded by
/// `tests/gov4_ingest_writeback_route.rs`.
pub async fn writeback(
    body: web::Json<WritebackRequest>,
    state: web::Data<AppState>,
    client_coordinator: web::Data<actix::Addr<ClientCoordinatorActor>>,
) -> HttpResponse {
    let wb = body.into_inner();
    let decision = wb.decision;
    log::info!(
        "[ingest-writeback] case={} decision={} remote={:?}",
        decision.case_id,
        decision.decision,
        wb.remote_id
    );
    let req = BrokerDecisionRequest {
        outcome: decision.decision,
        broker_pubkey: attribution_pubkey(decision.approved_by),
        reasoning: decision.reasoning,
        effective_tier: None,
    };
    apply_decision(
        decision.case_id,
        req,
        state.get_ref(),
        client_coordinator.get_ref(),
    )
    .await
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/ingest/writeback", web::post().to(writeback));
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn attribution_accepts_bare_hex_and_did_nostr() {
        assert_eq!(attribution_pubkey(Some(PK.into())).as_deref(), Some(PK));
        assert_eq!(
            attribution_pubkey(Some(format!("did:nostr:{PK}"))).as_deref(),
            Some(PK)
        );
    }

    #[test]
    fn attribution_rejects_malformed_and_absent() {
        assert!(attribution_pubkey(None).is_none());
        assert!(attribution_pubkey(Some("not-a-pubkey".into())).is_none());
        assert!(attribution_pubkey(Some("did:nostr:short".into())).is_none());
    }

    #[test]
    fn writeback_payload_deserialises_git_bridge_shape() {
        let raw = serde_json::json!({
            "remoteId": "remote-1",
            "enrichment": {"targetPath": "pages/foo.md", "content": "x"},
            "decision": {
                "caseId": "vc-elev-foo",
                "decision": "approve",
                "approvedBy": format!("did:nostr:{PK}"),
                "reasoning": "looks good",
                "serverDid": "did:nostr:server",
                "entityUrn": "urn:ngm:class:foo"
            }
        })
        .to_string();
        let req: WritebackRequest = serde_json::from_str(&raw).unwrap();
        assert_eq!(req.decision.case_id, "vc-elev-foo");
        assert_eq!(req.decision.decision, "approve");
        assert_eq!(req.remote_id.as_deref(), Some("remote-1"));
        assert_eq!(
            attribution_pubkey(req.decision.approved_by).as_deref(),
            Some(PK)
        );
    }
}
