//! `/wss/agent-events` — authenticated inbound `agent_action` ingest (ADR-059 §1, Phase 2).
//!
//! Closes the X2 consume-side gap. agentbox PUSHES `notifications/agent_action`
//! over this socket (`management-api/utils/agent-event-publisher.js`); before
//! this handler VisionClaw had **no JSON consumer** for those pushes at all (see
//! ADR-059 Design log, Finding 1 — the path was absent, not lossy). Every frame
//! is parsed against the canonical [`AgentActionNotification`] mirror, validated,
//! and published to the process-global [`hub`]. The GPU beam render actor
//! (`AgentBeamActor`, `src/actors/agent_beam_actor.rs`) subscribes to the hub and
//! is fully shipped; the attractive "gluon" transient edge is a separate,
//! deferred sub-feature (see `agent_beam_actor.rs:327` for why it is not
//! low-risk on the current packed-CSR GPU edge layout). ADR-2084 records the
//! correction of this module's own stale framing.
//!
//! Out of scope here, by design:
//!   * The `:9500` MCP-TCP path (`services/bots_client.rs`) is untouched and is
//!     **not deprecated**: it remains the sole source of agent *state
//!     snapshots* (`query_agent_list`, polled every 2s), a different payload
//!     from `agent_action`, and no replacement exists yet. Cutting state over
//!     to this WS transport is a planned, unbuilt follow-on — see ADR-2084.
//!   * The GLUON attractive transient edge (distinct from the shipped beam
//!     render above) is deferred — it would touch the spring system and the
//!     `Edge` struct and must not be bolted onto the shipped beam render in
//!     the same increment; see `agent_beam_actor.rs:327` for the CSR reason.
//!
//! Auth model: this is a **server-to-server** ingest socket (agentbox →
//! VisionClaw), not a browser socket. A valid session token is the gate
//! (Bearer header or `?token=`, validated via `NostrService::get_session`, the
//! same primitive the binary `/wss` socket uses). Origin is not enforced —
//! non-browser clients do not send it, and a cross-site browser script cannot
//! forge the bearer token cross-origin, so CSWSH is already mitigated.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use actix::{Actor, ActorContext, StreamHandler};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use log::{debug, info, warn};

use crate::app_state::AppState;
use crate::services::liveness_harness::{LivenessHarness, CANARY_REC3_CTC};

use super::hub;
use super::provenance::{self, ProvenanceStatus};
use super::schema::AgentActionNotification;

/// One-shot latch for `CANARY-VC-REC3-CTC` (REC-3, WP-7). The canary is one-shot,
/// so the ingest fires it at most once per process even though many CTC-bearing
/// frames may cross the wire; the harness re-arms it by the SHA/staleness rule.
static CTC_CANARY_FIRED: AtomicBool = AtomicBool::new(false);

/// Negotiated WebSocket subprotocol (ADR-059 §1).
const SUBPROTOCOL: &str = "vc-agent-events.v1";

/// Check whether insecure defaults are allowed (ADR-06 §D1). Compile-gated:
/// honoured only in `debug_assertions` / `--features dev-auth` builds with
/// `ALLOW_INSECURE_DEFAULTS` set; a const-`false` stub in release builds.
#[cfg(any(debug_assertions, feature = "dev-auth"))]
fn is_insecure_defaults_allowed() -> bool {
    std::env::var("ALLOW_INSECURE_DEFAULTS").is_ok()
}

#[cfg(not(any(debug_assertions, feature = "dev-auth")))]
#[inline(always)]
fn is_insecure_defaults_allowed() -> bool {
    false
}

/// Outcome of processing one inbound text frame. Pure and deterministic so the
/// ingest contract is unit-testable without spinning the actix actor.
#[derive(Debug, PartialEq)]
pub(crate) enum IngestOutcome {
    /// Canonical envelope parsed, validated, and published to the hub.
    Published {
        action: String,
        attributed: bool,
        /// Recorded provenance status (attributed / malformed / anonymous) —
        /// structural attribution only, not signature verification (the wire
        /// carries no signature; see [`super::provenance::ProvenanceStatus`]).
        /// The frame is published regardless of status (render compatibility);
        /// this is the audit dimension the Phase 3 trail distinguishes on.
        provenance_status: ProvenanceStatus,
        /// Number of foreign `urn:agentbox:*` source/target URNs translated
        /// through the BC20 bridge on this frame (0, 1, or 2).
        crossings_recorded: usize,
        /// REC-3 (WP-7): true when the envelope carried a populated typed CTC
        /// field. Drives the `CANARY-VC-REC3-CTC` fire from the actor.
        ctc_present: bool,
        receivers: usize,
    },
    /// Parsed as JSON-RPC but failed [`AgentActionNotification::is_canonical`].
    NonCanonical,
    /// Not parseable as an `AgentActionNotification`.
    Malformed,
}

/// Parse → validate → publish one inbound frame. The single source of truth for
/// the ingest contract; the actor's `StreamHandler` is a thin adapter over it.
pub(crate) fn process_frame(text: &str) -> IngestOutcome {
    match serde_json::from_str::<AgentActionNotification>(text) {
        Ok(notif) if notif.is_canonical() => {
            let event = notif.params.event;
            let action = event.action_type_name.clone();

            // Identity stays optional for render compatibility (ADR-059 §5: never
            // reject on absence), but is now RECORDED rather than discarded:
            //  (a) any foreign urn:agentbox:* source/target URN is translated
            //      through the BC20 bridge (crate::uri) so the namespace crossing
            //      is stored, not treated as an opaque blob; and
            //  (b) a provenance status is stamped so the audit surface can
            //      distinguish signed from unsigned actions.
            let prov = provenance::record(&event);
            let provenance_status = prov.status;
            let attributed = provenance_status.is_attributed();
            let crossings_recorded =
                prov.source_crossing.is_some() as usize + prov.target_crossing.is_some() as usize;

            // REC-3 (WP-7): read CTC presence before the envelope moves into the
            // hub — a populated typed CTC field is the CANARY-VC-REC3-CTC predicate.
            let ctc_present = event.has_ctc();

            if let Some(c) = prov.source_crossing.as_ref() {
                debug!(
                    "agent-events: BC20 source crossing {} → {}",
                    c.agentbox_urn, c.visionclaw_id
                );
            }
            if let Some(c) = prov.target_crossing.as_ref() {
                debug!(
                    "agent-events: BC20 target crossing {} → {}",
                    c.agentbox_urn, c.visionclaw_id
                );
            }

            // Render path is unchanged: the identity-bearing envelope is still
            // published verbatim to the hub for the beam/gluon GPU render.
            let receivers = hub::publish(event);
            IngestOutcome::Published {
                action,
                attributed,
                provenance_status,
                crossings_recorded,
                ctc_present,
                receivers,
            }
        }
        Ok(_) => IngestOutcome::NonCanonical,
        Err(_) => IngestOutcome::Malformed,
    }
}

/// Per-connection ingest actor. Holds the authenticated session pubkey
/// (ADR-059 §1: "the authenticated pubkey becomes the session pubkey").
pub struct AgentEventsIngestWs {
    /// did:nostr hex of the authenticated session, if any (Phase 1: optional).
    session_pubkey: Option<String>,
    /// RES-a harness handle so a CTC-bearing frame can fire `CANARY-VC-REC3-CTC`
    /// (REC-3, WP-7). `None` only in unit tests that drive the pure ingest.
    harness: Option<Arc<LivenessHarness>>,
}

impl AgentEventsIngestWs {
    fn new(session_pubkey: Option<String>, harness: Option<Arc<LivenessHarness>>) -> Self {
        Self {
            session_pubkey,
            harness,
        }
    }

    /// REC-3 (WP-7): fire `CANARY-VC-REC3-CTC` once per process on the first
    /// CTC-bearing envelope. The harness records it as observed live traffic.
    fn fire_ctc_canary(&self, action: &str) {
        let Some(harness) = self.harness.clone() else {
            return;
        };
        // One-shot: only the first CTC frame this process sees fires.
        if CTC_CANARY_FIRED.swap(true, Ordering::SeqCst) {
            return;
        }
        let evidence =
            format!("agent-events envelope carried a populated typed CTC field (action={action})");
        actix::spawn(async move {
            if let Err(e) = harness.observe(CANARY_REC3_CTC, &evidence).await {
                warn!("[agent-events] failed to record {CANARY_REC3_CTC} fire: {e}");
            } else {
                info!("[agent-events] {CANARY_REC3_CTC} fired: {evidence}");
            }
        });
    }
}

impl Actor for AgentEventsIngestWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!(
            "agent-events: ingest socket open (session_pubkey={})",
            self.session_pubkey.as_deref().unwrap_or("<anon>")
        );
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for AgentEventsIngestWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => match process_frame(&text) {
                IngestOutcome::Published {
                    action,
                    attributed,
                    provenance_status,
                    crossings_recorded,
                    ctc_present,
                    receivers,
                } => {
                    debug!(
                        "agent-events: published action={action} attributed={attributed} \
                         provenance={provenance_status:?} crossings={crossings_recorded} \
                         ctc={ctc_present} → {receivers} subscriber(s)"
                    );
                    if ctc_present {
                        self.fire_ctc_canary(&action);
                    }
                }
                IngestOutcome::NonCanonical => {
                    warn!("agent-events: non-canonical envelope rejected");
                    ctx.text(r#"{"error":"non_canonical_envelope"}"#);
                }
                IngestOutcome::Malformed => {
                    warn!("agent-events: malformed frame rejected");
                    ctx.text(r#"{"error":"malformed_json"}"#);
                }
            },
            Ok(ws::Message::Binary(_)) => {
                // Phase 1 ingest is JSON-only. The 0x23 binary frame is the
                // server→browser hot path (ADR-059 §4 / Finding 2), not inbound.
                warn!("agent-events: binary frames not accepted on ingest socket");
            }
            Ok(ws::Message::Ping(payload)) => ctx.pong(&payload),
            Ok(ws::Message::Pong(_)) => {}
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) => ctx.stop(),
            Ok(ws::Message::Nop) => {}
            Err(e) => {
                warn!("agent-events: ws protocol error: {e}");
                ctx.stop();
            }
        }
    }
}

/// Validate the session token and return the authenticated pubkey (if any).
/// `Err(HttpResponse)` short-circuits the upgrade with the failure response.
async fn authenticate(
    req: &HttpRequest,
    app_state: &AppState,
) -> Result<Option<String>, HttpResponse> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::to_string)
        .or_else(|| {
            url::form_urlencoded::parse(req.query_string().as_bytes())
                .find(|(k, _)| k == "token")
                .map(|(_, v)| v.to_string())
        });

    match (token.as_deref(), app_state.nostr_service.as_ref()) {
        (Some(t), Some(ns)) if !t.is_empty() => match ns.get_session(t).await {
            Some(user) => Ok(Some(user.pubkey)),
            None if is_insecure_defaults_allowed() => {
                warn!(
                    "agent-events: token failed validation but ALLOW_INSECURE_DEFAULTS \
                     set — accepting unauthenticated (dev build)"
                );
                Ok(None)
            }
            None => {
                Err(HttpResponse::Unauthorized().body("Invalid or expired authentication token"))
            }
        },
        _ if is_insecure_defaults_allowed() => {
            warn!("agent-events: unauthenticated ingest accepted (dev / insecure defaults)");
            Ok(None)
        }
        _ => Err(HttpResponse::Unauthorized()
            .body("Authentication required for the agent-events socket")),
    }
}

/// HTTP upgrade handler for `/wss/agent-events`.
pub async fn agent_events_ws(
    req: HttpRequest,
    stream: web::Payload,
    app_state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let session_pubkey = match authenticate(&req, &app_state).await {
        Ok(pk) => pk,
        Err(resp) => return Ok(resp),
    };

    // REC-3 (WP-7): give the ingest actor the harness so a CTC-bearing frame
    // fires CANARY-VC-REC3-CTC as observed live traffic.
    let harness = Some(app_state.liveness_harness.clone());

    ws::WsResponseBuilder::new(
        AgentEventsIngestWs::new(session_pubkey, harness),
        &req,
        stream,
    )
    .protocols(&[SUBPROTOCOL])
    .start()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal canonical envelope (the exact shape is exhaustively guarded by the
    // cross-repo fixture in `schema.rs`; here we only need a canonical frame to
    // drive the ingest contract).
    fn canonical_frame(id: u64) -> String {
        format!(
            r#"{{
              "jsonrpc": "2.0",
              "method": "notifications/agent_action",
              "params": {{
                "type": "agent_action",
                "event": {{
                  "version": 3, "id": {id}, "source_agent_id": 7, "target_node_id": 4242,
                  "action_type": 1, "action_type_name": "update",
                  "timestamp": 1748500000000, "duration_ms": 250,
                  "pubkey": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                  "metadata": {{ "note": "x" }}
                }},
                "message_type": 35, "protocol_version": 2,
                "timestamp": "2026-05-29T00:00:00.000Z"
              }}
            }}"#
        )
    }

    /// Shared fixture for the sending side (agentbox): one valid inbound frame
    /// exactly as [`process_frame`] (parsing `AgentActionNotification`, then
    /// `is_canonical`) accepts it. Agentbox should test its emitter against
    /// `tests/fixtures/agent_event_ingest_frame.json`; if this test needs the
    /// fixture changed, the wire contract has changed.
    #[test]
    fn shared_fixture_frame_is_accepted() {
        let frame = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/agent_event_ingest_frame.json"
        ));
        match process_frame(frame) {
            IngestOutcome::Published {
                action,
                provenance_status,
                crossings_recorded,
                ctc_present,
                ..
            } => {
                assert_eq!(action, "update");
                assert_eq!(provenance_status, ProvenanceStatus::Attributed);
                assert_eq!(crossings_recorded, 0);
                assert!(!ctc_present);
            }
            other => panic!("fixture must be accepted as Published, got {other:?}"),
        }
    }

    #[test]
    fn canonical_frame_publishes_to_hub_and_round_trips() {
        // Subscribe before publishing so this receiver observes our own frame.
        let mut rx = hub::subscribe();
        let unique_id = 918_273_645;

        let outcome = process_frame(&canonical_frame(unique_id));
        match outcome {
            IngestOutcome::Published {
                action,
                attributed,
                provenance_status,
                crossings_recorded,
                ctc_present,
                receivers,
            } => {
                assert_eq!(action, "update");
                assert!(attributed, "valid pubkey present ⇒ attributed");
                assert_eq!(provenance_status, ProvenanceStatus::Attributed);
                // canonical_frame carries no source/target URN ⇒ no crossing.
                assert_eq!(crossings_recorded, 0);
                // canonical_frame carries no CTC field ⇒ canary predicate false.
                assert!(!ctc_present, "no CTC field ⇒ ctc_present false");
                assert!(receivers >= 1, "our own subscriber must be counted");
            }
            other => panic!("expected Published, got {other:?}"),
        }

        // Drain until we see our id (other parallel tests may publish too).
        let mut seen = false;
        while let Ok(env) = rx.try_recv() {
            if env.id == unique_id {
                assert_eq!(env.action_type, 1);
                assert_eq!(env.pubkey.as_deref().unwrap().len(), 64);
                seen = true;
                break;
            }
        }
        assert!(seen, "published envelope must reach the subscriber");
    }

    // A canonical frame carrying foreign urn:agentbox:* source/target URNs and
    // no pubkey — exercises BC20 crossing recording + anonymous provenance while
    // still publishing (render compatibility must not break).
    fn frame_with_foreign_urns_unauthenticated(id: u64) -> String {
        let pk = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        format!(
            r#"{{
              "jsonrpc": "2.0",
              "method": "notifications/agent_action",
              "params": {{
                "type": "agent_action",
                "event": {{
                  "version": 3, "id": {id}, "source_agent_id": 7, "target_node_id": 4242,
                  "action_type": 1, "action_type_name": "update",
                  "timestamp": 1748500000000, "duration_ms": 250,
                  "source_urn": "urn:agentbox:thing:{pk}:proposal-9",
                  "target_urn": "urn:agentbox:activity:{pk}:run-9",
                  "metadata": {{ "note": "x" }}
                }},
                "message_type": 35, "protocol_version": 2,
                "timestamp": "2026-05-29T00:00:00.000Z"
              }}
            }}"#
        )
    }

    #[test]
    fn foreign_urns_recorded_and_unauthenticated_frame_still_published() {
        let outcome = process_frame(&frame_with_foreign_urns_unauthenticated(123_456_789));
        match outcome {
            IngestOutcome::Published {
                attributed,
                provenance_status,
                crossings_recorded,
                ..
            } => {
                // No pubkey ⇒ anonymous, but still published (not rejected).
                assert!(!attributed);
                assert_eq!(provenance_status, ProvenanceStatus::Anonymous);
                // both source + target were foreign agentbox URNs ⇒ 2 crossings.
                assert_eq!(crossings_recorded, 2);
            }
            other => panic!("expected Published, got {other:?}"),
        }
    }

    // REC-3 (WP-7): a canonical frame carrying a typed CTC field reports
    // ctc_present — the CANARY-VC-REC3-CTC predicate the actor fires on.
    fn canonical_frame_with_ctc(id: u64) -> String {
        format!(
            r#"{{
              "jsonrpc": "2.0",
              "method": "notifications/agent_action",
              "params": {{
                "type": "agent_action",
                "event": {{
                  "version": 3, "id": {id}, "source_agent_id": 7, "target_node_id": 4242,
                  "action_type": 2, "action_type_name": "create",
                  "timestamp": 1748500000000, "duration_ms": 250,
                  "token_count": 4200000000, "handoff_id": "urn:agentbox:activity:chain-3",
                  "verification": "passed",
                  "metadata": {{ "note": "x" }}
                }},
                "message_type": 35, "protocol_version": 2,
                "timestamp": "2026-05-29T00:00:00.000Z"
              }}
            }}"#
        )
    }

    #[test]
    fn ctc_bearing_frame_reports_ctc_present() {
        let outcome = process_frame(&canonical_frame_with_ctc(555_000_111));
        match outcome {
            IngestOutcome::Published { ctc_present, .. } => {
                assert!(ctc_present, "populated CTC field ⇒ ctc_present true");
            }
            other => panic!("expected Published, got {other:?}"),
        }
    }

    #[test]
    fn malformed_frame_is_rejected() {
        assert_eq!(process_frame("{ not json"), IngestOutcome::Malformed);
        assert_eq!(process_frame(""), IngestOutcome::Malformed);
    }

    #[test]
    fn non_canonical_frame_is_rejected() {
        // Valid JSON-RPC but wrong method ⇒ not canonical, not published.
        let wrong_method = r#"{
          "jsonrpc": "2.0",
          "method": "notifications/something_else",
          "params": {
            "type": "agent_action",
            "event": {
              "version": 3, "id": 1, "source_agent_id": 1, "target_node_id": 2,
              "action_type": 0, "action_type_name": "query",
              "timestamp": 1748500000000, "duration_ms": 100
            },
            "message_type": 35, "protocol_version": 2,
            "timestamp": "2026-05-29T00:00:00.000Z"
          }
        }"#;
        assert_eq!(process_frame(wrong_method), IngestOutcome::NonCanonical);

        // version < 3 ⇒ pre-ADR-059 envelope, also non-canonical.
        let old_version = r#"{
          "jsonrpc": "2.0",
          "method": "notifications/agent_action",
          "params": {
            "type": "agent_action",
            "event": {
              "version": 2, "id": 1, "source_agent_id": 1, "target_node_id": 2,
              "action_type": 0, "action_type_name": "query",
              "timestamp": 1748500000000, "duration_ms": 100
            },
            "message_type": 35, "protocol_version": 2,
            "timestamp": "2026-05-29T00:00:00.000Z"
          }
        }"#;
        assert_eq!(process_frame(old_version), IngestOutcome::NonCanonical);
    }
}
