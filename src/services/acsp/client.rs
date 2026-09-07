//! ACSP relay client: signs unsigned ACSP events with the panel keypair and
//! publishes them to the forum relay; subscribes to kind-31403 ActionResponse
//! events so agentic actors receive human decisions.
//!
//! Built on `nostr_sdk::Client` (relay pool with automatic reconnection, OK
//! acknowledgement handling, subscription streams) — no hand-rolled relay
//! websockets.
//!
//! The relay only accepts kinds 31400-31402 from pubkeys registered in its
//! `agent_registry` D1 table; [`AcspClient::pubkey_hex`] is logged at startup
//! so an admin can register it (`POST /api/governance/agents/register`,
//! NIP-98 admin-gated). Until registered, publishes fail with
//! `blocked: pubkey not in agent registry` — surfaced as an error here.

use log::{debug, info, warn};
use nostr_sdk::prelude::*;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::{fs, io::Write, path::PathBuf};

use super::events::{ActionResponse, UnsignedAcspEvent, KIND_ACTION_RESPONSE};

/// A human decision delivered for a previously opened broker case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseDecision {
    /// The case id (the 31402 d-tag this response answers).
    pub case_id: String,
    /// `approve` / `reject` / `amend` / `delegate`.
    pub action: String,
    pub reasoning: String,
    /// Pubkey of the responding admin (relay enforces admin-only 31403).
    pub responder_pubkey: String,
    /// ADR-2006 — the id of the **signed** 31403 event this decision came from.
    ///
    /// Without it a decision could only be correlated by re-deriving an
    /// identifier from `(case_id, action, responder_pubkey)`, which is not
    /// unique: a replayed or re-sent response, or an admin who answers the same
    /// case twice the same way, collides with the first. The signed event id is
    /// the one identifier the relay guarantees is unique per decision, so it is
    /// what the provenance record correlates on and what duplicate suppression
    /// keys off.
    pub event_id: String,
    /// The 31403 event's `created_at`, in seconds since the Unix epoch.
    ///
    /// Retained separately from the local receipt time so subscription lag —
    /// the gap between when the human decided and when this process saw it —
    /// is measurable after the fact rather than only loggable at the moment.
    pub created_at: u64,
}

impl CaseDecision {
    /// A stable, per-decision correlation key: the signed event id.
    ///
    /// This is what makes duplicate detection exact. Two deliveries of the same
    /// signed decision share it; two genuinely distinct decisions never do,
    /// even when case, action and responder all match.
    pub fn correlation_id(&self) -> &str {
        &self.event_id
    }

    /// Seconds between the decision being signed and `observed_at` (also
    /// epoch seconds). Saturates at zero for an event dated in the future,
    /// which a relay with a skewed clock can produce.
    pub fn lag_seconds(&self, observed_at: u64) -> u64 {
        observed_at.saturating_sub(self.created_at)
    }
}

pub struct AcspClient {
    keys: Keys,
    client: Client,
    request_dir: PathBuf,
}

impl AcspClient {
    /// Build from a 64-hex secret key and connect the relay pool to the forum
    /// relay.
    pub async fn connect(secret_hex: &str, forum_relay_url: &str) -> Result<Self, String> {
        let secret_key =
            SecretKey::from_hex(secret_hex).map_err(|e| format!("ACSP secret key: {e}"))?;
        let keys = Keys::new(secret_key);
        let client = Client::new(keys.clone());
        client
            .add_relay(forum_relay_url)
            .await
            .map_err(|e| format!("ACSP add_relay: {e}"))?;
        client.connect().await;
        info!(
            "[ACSP] panel identity {} connected to {} (register this pubkey in the relay agent_registry)",
            keys.public_key().to_hex(),
            forum_relay_url
        );
        let request_dir =
            PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into()))
                .join("acsp-requests");
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        directory.mode(0o700);
        directory
            .create(&request_dir)
            .map_err(|e| format!("ACSP request journal: {e}"))?;
        Ok(Self {
            keys,
            client,
            request_dir,
        })
    }

    /// x-only pubkey (hex) — must be registered in the relay's agent_registry.
    pub fn pubkey_hex(&self) -> String {
        self.keys.public_key().to_hex()
    }

    /// Sign an unsigned ACSP event and publish it, awaiting relay
    /// acknowledgement from the pool.
    pub async fn publish(&self, ev: &UnsignedAcspEvent) -> Result<String, String> {
        let tags: Vec<Tag> = ev
            .tags
            .iter()
            .filter(|t| !t.is_empty())
            .map(|t| Tag::custom(TagKind::Custom(t[0].clone().into()), t[1..].to_vec()))
            .collect();

        let event = EventBuilder::new(Kind::Custom(ev.kind), &ev.content)
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|e| format!("ACSP sign failed: {e}"))?;

        // Retain the exact signed payload before publication. A failed publish
        // can leave an unacknowledged request, but never an unbound decision.
        if ev.kind == super::events::KIND_ACTION_REQUEST {
            persist_once(
                &self.request_dir.join(format!("{}.json", event.id)),
                event.as_json().as_bytes(),
            )
            .map_err(|e| format!("ACSP request journal: {e}"))?;
        }
        let output = self
            .client
            .send_event(&event)
            .await
            .map_err(|e| format!("ACSP publish failed: {e}"))?;
        if output.success.is_empty() {
            let reason = output
                .failed
                .values()
                .next()
                .cloned()
                .unwrap_or_else(|| "no relay accepted the event".into());
            return Err(format!("forum rejected: {reason}"));
        }
        debug!("[ACSP] published kind {} event {}", ev.kind, output.id());
        Ok(output.id().to_hex())
    }

    /// Long-lived subscription for kind-31403 ActionResponse events whose
    /// d-tag starts with `case_prefix` (each agentic actor namespaces its case
    /// ids, e.g. `vc-elev-`). Decisions are delivered through `sink`. The SDK
    /// pool owns reconnection; this future runs until the receiver side of
    /// `sink` drops.
    pub async fn run_decision_subscription(
        &self,
        case_prefix: String,
        sink: tokio::sync::mpsc::UnboundedSender<CaseDecision>,
    ) {
        let filter = Filter::new()
            .kind(Kind::Custom(KIND_ACTION_RESPONSE))
            .since(Timestamp::now());
        if let Err(e) = self.client.subscribe(filter, None).await {
            warn!("[ACSP] decision subscribe failed: {e}");
            return;
        }

        let mut notifications = self.client.notifications();
        loop {
            match notifications.recv().await {
                Ok(RelayPoolNotification::Event { event, .. }) => {
                    if let Some(decision) = decision_from_event(&event, &case_prefix) {
                        let Some(request_id) = request_reference(&event) else {
                            continue;
                        };
                        let request =
                            fs::read_to_string(self.request_dir.join(format!("{request_id}.json")))
                                .ok()
                                .and_then(|raw| Event::from_json(raw).ok());
                        let Some(request) = request else {
                            continue;
                        };
                        if request.pubkey != self.keys.public_key()
                            || !response_matches_request(&event, &request)
                        {
                            warn!("[ACSP] refusing unbound decision {}", event.id);
                            continue;
                        }
                        // Claim once before dispatch. A crash after this point requires
                        // reconciliation; an uncertain mutation must never be replayed.
                        if persist_once(
                            &self
                                .request_dir
                                .join(format!("{request_id}.dispatched.json")),
                            event.as_json().as_bytes(),
                        )
                        .is_err()
                        {
                            warn!("[ACSP] decision already dispatched or journal unavailable: {request_id}");
                            continue;
                        }
                        if sink.send(decision).is_err() {
                            info!("[ACSP] decision sink closed; subscription ending");
                            return;
                        }
                    }
                }
                Ok(RelayPoolNotification::Shutdown) => {
                    warn!("[ACSP] relay pool shut down; decision subscription ending");
                    return;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("[ACSP] decision stream lagged, skipped {n} notifications");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }
}

/// Write an immutable, durable journal record. Interrupted partial writes are
/// left present and rejected on read/retry, requiring operator reconciliation.
fn persist_once(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::File::open(
        path.parent()
            .ok_or_else(|| std::io::Error::other("missing journal directory"))?,
    )?
    .sync_all()
}

fn request_reference(event: &Event) -> Option<String> {
    let mut ids = std::collections::BTreeSet::new();
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) == Some("e")
            && (parts.len() < 4 || parts[3].is_empty() || parts[3] == "request")
        {
            let id = parts.get(1)?;
            EventId::from_hex(id).ok()?;
            ids.insert(id.clone());
        }
    }
    if ids.len() == 1 {
        ids.into_iter().next()
    } else {
        None
    }
}

fn response_matches_request(response: &Event, request: &Event) -> bool {
    response.verify().is_ok()
        && request.verify().is_ok()
        && request.kind == Kind::Custom(super::events::KIND_ACTION_REQUEST)
        && response.kind == Kind::Custom(KIND_ACTION_RESPONSE)
        && request_reference(response).as_deref() == Some(request.id.to_hex().as_str())
        && response.tags.identifier().is_some()
        && response.tags.identifier() == request.tags.identifier()
}

/// Convert a kind-31403 event into a [`CaseDecision`] when its d-tag falls in
/// our case namespace.
pub fn decision_from_event(event: &Event, case_prefix: &str) -> Option<CaseDecision> {
    if event.kind != Kind::Custom(KIND_ACTION_RESPONSE) || event.verify().is_err() {
        return None;
    }
    let case_id = event
        .tags
        .iter()
        .find(|t| t.kind() == TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)))
        .and_then(|t| t.content())?
        .to_string();
    if !case_id.starts_with(case_prefix) {
        return None;
    }
    let content: ActionResponse = serde_json::from_str(&event.content).ok()?;
    Some(CaseDecision {
        case_id,
        action: content.action,
        reasoning: content.reasoning,
        responder_pubkey: event.pubkey.to_hex(),
        // ADR-2006: preserve the signed event's identity and timestamp so the
        // decision can be correlated back to the request that produced it.
        event_id: event.id.to_hex(),
        created_at: event.created_at.as_u64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_response(case_id: &str, action: &str) -> Event {
        let keys = Keys::generate();
        EventBuilder::new(
            Kind::Custom(KIND_ACTION_RESPONSE),
            serde_json::json!({"action": action, "reasoning": "Human approve via governance UI"})
                .to_string(),
        )
        .tags([Tag::identifier(case_id)])
        .sign_with_keys(&keys)
        .unwrap()
    }

    #[test]
    fn exact_signed_request_binding_rejects_mismatch_ambiguity_and_tampering() {
        let keys = Keys::generate();
        let request = EventBuilder::new(
            Kind::Custom(31402),
            r#"{"fields":{"operation":{"draft":"exact bytes"}}}"#,
        )
        .tags([Tag::identifier("vc-elev-42")])
        .sign_with_keys(&keys)
        .unwrap();
        let response = |id: EventId, case: &str, extra: bool| {
            let mut tags = vec![Tag::identifier(case), Tag::event(id)];
            if extra {
                tags.push(Tag::event(EventId::all_zeros()));
            }
            EventBuilder::new(
                Kind::Custom(31403),
                r#"{"action":"approve","reasoning":"ok"}"#,
            )
            .tags(tags)
            .sign_with_keys(&keys)
            .unwrap()
        };
        assert!(response_matches_request(
            &response(request.id, "vc-elev-42", false),
            &request
        ));
        assert!(!response_matches_request(
            &response(EventId::all_zeros(), "vc-elev-42", false),
            &request
        ));
        assert!(!response_matches_request(
            &response(request.id, "vc-elev-other", false),
            &request
        ));
        assert!(!response_matches_request(
            &response(request.id, "vc-elev-42", true),
            &request
        ));
        assert!(!response_matches_request(
            &signed_response("vc-elev-42", "approve"),
            &request
        ));
        let mut tampered = request.clone();
        tampered.content = "changed operation".into();
        assert!(!response_matches_request(
            &response(request.id, "vc-elev-42", false),
            &tampered
        ));
    }

    #[test]
    fn dispatch_journal_survives_restart_and_refuses_duplicate_or_unavailable_storage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request.dispatched.json");
        persist_once(&path, b"signed response").unwrap();
        assert!(persist_once(&path, b"replay").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"signed response");
        assert!(persist_once(&dir.path().join("missing/request.json"), b"x").is_err());
    }

    #[test]
    fn parses_matching_decision() {
        let ev = signed_response("vc-elev-42", "approve");
        let d = decision_from_event(&ev, "vc-elev-").unwrap();
        assert_eq!(d.case_id, "vc-elev-42");
        assert_eq!(d.action, "approve");
        assert_eq!(d.responder_pubkey, ev.pubkey.to_hex());
    }

    #[test]
    fn ignores_foreign_namespace_and_other_kinds() {
        assert!(decision_from_event(&signed_response("other-1", "approve"), "vc-elev-").is_none());
        let keys = Keys::generate();
        let wrong_kind = EventBuilder::new(Kind::Custom(31401), "{}")
            .tags([Tag::identifier("vc-elev-1")])
            .sign_with_keys(&keys)
            .unwrap();
        assert!(decision_from_event(&wrong_kind, "vc-elev-").is_none());
    }
}
