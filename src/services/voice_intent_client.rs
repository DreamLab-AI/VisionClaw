//! VoiceIntentClient — VisionClaw's consumer of agentbox `/v1/voice-intent`
//! (COM-15 / V1 / D6 / M5, PRD-023 WP-5).
//!
//! Owns the consumer half of the governed voice loop. A spoken command bound to
//! a *selected* agent's `did:nostr` (the D6 binding carried on the
//! [`crate::services::audio_router::UserVoiceSession`]) is turned into a SIGNED
//! kind-31402 `ActionRequest` ([`crate::services::acsp::events`], ADR-110)
//! targeted at that DID, then POSTed to the agentbox producer
//! (`/v1/voice-intent`, ADR-037 D7: additive `actor_did`, mandate-authenticated
//! with a NIP-98 header). On accepted dispatch the caller speaks the
//! acknowledgement over the PocketTts TTS path.
//!
//! ## Cross-substrate boundary (honest label)
//!
//! agentbox owns the producer and un-gates it in this same wave; VisionClaw owns
//! capture, the selected-agent binding, the call, and the acknowledgement (DDD
//! §10). Until the un-gated producer is reachable from this container, the
//! client is exercised against a local fake of the D7 endpoint
//! (`tests/voice_intent_roundtrip.rs`) and the live cross-substrate round-trip is
//! labelled **pending-live-session** in the evidence file. `CANARY-VC-COM15-PTT`
//! (standing, P1) fires on the live end-to-end, never on the unit path.
//!
//! ## Verify before trust (DDD invariant 2)
//!
//! A dispatch is refused before any HTTP when `actor_did` is not a canonical
//! `did:nostr` (`uri::parse`), so a hashed nickname or a malformed claim can
//! never become the target of a governed command (ADR-037 D7 rejects the
//! free-text `hashString(actor)` target for exactly this reason).

use std::sync::Arc;
use std::time::Duration;

use log::{debug, info, warn};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};

use crate::services::acsp::events::{
    build_action_request, ActionPriority, ActionRequest, CaseCategory, CaseSpec, SubjectKind,
    UnsignedAcspEvent, KIND_ACTION_REQUEST,
};
use crate::utils::nip98::{build_auth_header, generate_nip98_token, Nip98Config};

/// Default target when only a management-API base URL is configured.
const VOICE_INTENT_PATH: &str = "/v1/voice-intent";

/// What went wrong on a dispatch. Kept coarse — the caller only needs to decide
/// whether to speak an ack or a failure line.
#[derive(Debug)]
pub enum VoiceIntentError {
    /// `actor_did` is not a canonical `did:nostr` — refused before any HTTP.
    NotADid(String),
    /// Building or signing the 31402 / mandate failed.
    Sign(String),
    /// The HTTP call itself failed (transport, timeout, un-gated 503, …).
    Http(String),
    /// The producer answered but declined the dispatch.
    Rejected(String),
}

impl std::fmt::Display for VoiceIntentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotADid(d) => write!(f, "target is not a did:nostr: {d}"),
            Self::Sign(e) => write!(f, "sign failed: {e}"),
            Self::Http(e) => write!(f, "voice-intent call failed: {e}"),
            Self::Rejected(e) => write!(f, "voice-intent rejected: {e}"),
        }
    }
}

impl std::error::Error for VoiceIntentError {}

/// The producer's acceptance echo. Lenient by design: the D7 producer returns a
/// rich body, but only these fields shape the spoken acknowledgement, and every
/// one is optional so a minimal `{ "success": true }` still parses.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VoiceIntentAccepted {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub event_id: Option<i64>,
    #[serde(default)]
    pub intent: VoiceIntentEcho,
}

/// The recognised-intent echo inside an acceptance.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VoiceIntentEcho {
    #[serde(default)]
    pub verb: String,
    #[serde(default)]
    pub action_type_name: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub recognised: bool,
}

/// The signed-31402 summary carried alongside the D7 fields, so the governed
/// action VisionClaw built and signed rides the wire (PRD-023 WP-5 AC2). The
/// producer verifies the `sig` against `pubkey` and matches `target_did`.
#[derive(Debug, Clone, Serialize)]
struct SignedActionField {
    kind: u16,
    id: String,
    pubkey: String,
    sig: String,
    target_did: String,
}

/// The POST body: the ADR-037 D7 minimal contract (`transcript`, optional
/// `actor` label, additive `actor_did`, `duration_ms`) plus the additive signed
/// 31402 summary.
#[derive(Debug, Clone, Serialize)]
struct VoiceIntentRequest {
    transcript: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    /// ADR-037 D7 additive field: the verified target agent `did:nostr`.
    actor_did: String,
    duration_ms: u32,
    signed_action: SignedActionField,
}

/// The consumer client. Cheap to clone via `Arc`; one HTTP client, one signing
/// key (the panel/mandate identity), one endpoint.
pub struct VoiceIntentClient {
    http: reqwest::Client,
    endpoint: String,
    keys: Keys,
    actor_label: Option<String>,
}

impl VoiceIntentClient {
    /// Build from the environment, or `None` when unconfigured (the governed
    /// loop stays off and the caller falls back to the settings assistant —
    /// honest gating, never a fabricated dispatch). Requires:
    ///  - `AGENTBOX_VOICE_INTENT_URL` (full URL) or `AGENTBOX_MANAGEMENT_URL`
    ///    (base; `/v1/voice-intent` is appended);
    ///  - `ACSP_PANEL_NOSTR_PRIVKEY` (64-hex) — the same panel identity the ACSP
    ///    producer already uses, reused as the voice mandate signer.
    pub fn from_env() -> Option<Arc<Self>> {
        let endpoint = std::env::var("AGENTBOX_VOICE_INTENT_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("AGENTBOX_MANAGEMENT_URL")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|base| format!("{}{}", base.trim_end_matches('/'), VOICE_INTENT_PATH))
            })?;
        let secret = std::env::var("ACSP_PANEL_NOSTR_PRIVKEY")
            .ok()
            .filter(|s| !s.is_empty())?;
        let secret_key = match SecretKey::from_hex(&secret) {
            Ok(k) => k,
            Err(e) => {
                warn!("[voice-intent] ACSP_PANEL_NOSTR_PRIVKEY invalid, governed voice loop disabled: {e}");
                return None;
            }
        };
        let actor_label = std::env::var("VISIONCLAW_VOICE_ACTOR_LABEL")
            .ok()
            .filter(|s| !s.is_empty());
        info!("[voice-intent] governed voice loop configured → {endpoint}");
        Some(Arc::new(Self::new(
            endpoint,
            Keys::new(secret_key),
            actor_label,
        )))
    }

    /// Direct constructor (used by [`Self::from_env`] and the round-trip tests).
    pub fn new(endpoint: String, keys: Keys, actor_label: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            http,
            endpoint,
            keys,
            actor_label,
        }
    }

    /// The mandate/panel signer's x-only pubkey (hex).
    pub fn pubkey_hex(&self) -> String {
        self.keys.public_key().to_hex()
    }

    /// Build a signed, targeted governed dispatch from a transcript + the bound
    /// agent DID, and POST it to the D7 producer. On acceptance returns the echo
    /// the caller turns into a PocketTts acknowledgement.
    pub async fn dispatch(
        &self,
        transcript: &str,
        actor_did: &str,
        duration_ms: u32,
    ) -> Result<VoiceIntentAccepted, VoiceIntentError> {
        // Verify before trust (DDD invariant 2): the target must be a canonical
        // did:nostr BEFORE any signing or HTTP. A hashed nickname never targets a
        // governed command.
        if !is_canonical_did(actor_did) {
            return Err(VoiceIntentError::NotADid(actor_did.to_string()));
        }

        let unsigned = build_voice_action_request(transcript, actor_did);
        let signed = sign_unsigned(&unsigned, &self.keys).map_err(VoiceIntentError::Sign)?;

        let body = VoiceIntentRequest {
            transcript: transcript.to_string(),
            actor: self.actor_label.clone(),
            actor_did: actor_did.to_string(),
            duration_ms,
            signed_action: SignedActionField {
                kind: KIND_ACTION_REQUEST,
                id: signed.id.to_hex(),
                pubkey: signed.pubkey.to_hex(),
                sig: signed.sig.to_string(),
                target_did: actor_did.to_string(),
            },
        };
        let body_json =
            serde_json::to_string(&body).map_err(|e| VoiceIntentError::Sign(e.to_string()))?;

        // The mandate: a NIP-98 header over POST + endpoint + the exact body, so
        // the un-gated producer authenticates the dispatch (ADR-037 D7).
        let token = generate_nip98_token(
            &self.keys,
            &Nip98Config {
                url: self.endpoint.clone(),
                method: "POST".to_string(),
                body: Some(body_json.clone()),
            },
        )
        .map_err(|e| VoiceIntentError::Sign(e.to_string()))?;

        let resp = self
            .http
            .post(&self.endpoint)
            .header("Authorization", build_auth_header(&token))
            .header("Content-Type", "application/json")
            .body(body_json)
            .send()
            .await
            .map_err(|e| VoiceIntentError::Http(e.to_string()))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| VoiceIntentError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(VoiceIntentError::Rejected(format!("HTTP {status}: {text}")));
        }
        let accepted: VoiceIntentAccepted = serde_json::from_str(&text).map_err(|e| {
            VoiceIntentError::Rejected(format!("unparseable acceptance: {e}: {text}"))
        })?;
        if !accepted.success {
            return Err(VoiceIntentError::Rejected(text));
        }
        debug!(
            "[voice-intent] dispatched → {} (event {:?}, verb '{}')",
            short_did(actor_did),
            accepted.event_id,
            accepted.intent.verb
        );
        Ok(accepted)
    }
}

/// True iff `s` is a canonical `did:nostr:<64-hex>` (ADR-125 I1).
pub fn is_canonical_did(s: &str) -> bool {
    matches!(
        crate::uri::parse(s),
        Ok(crate::uri::ParsedUri::DidNostr { .. })
    )
}

/// Build the unsigned kind-31402 `ActionRequest` for a voice command targeted at
/// `actor_did`. The target DID is the 31402 `subject-id`, so the governed action
/// is addressed at the selected agent (not a hashed label). The transcript rides
/// the content `fields`.
pub fn build_voice_action_request(transcript: &str, actor_did: &str) -> UnsignedAcspEvent {
    let case_id = format!("vc-voice-{}", short_key(actor_did, transcript));
    let spec = CaseSpec {
        case_id,
        title: title_for(transcript),
        priority: ActionPriority::Medium,
        category: CaseCategory::ManualSubmission,
        subject_kind: SubjectKind::AutomationProposal,
        subject_id: actor_did.to_string(),
        request: ActionRequest {
            fields: serde_json::json!({
                "transcript": transcript,
                "target_did": actor_did,
                "origin": "voice-ptt",
            }),
            reasoning: Some("Spoken command bound to the selected agent (PTT).".to_string()),
            context_url: None,
        },
    };
    build_action_request(&spec)
}

/// Sign an [`UnsignedAcspEvent`] with `keys` into a `nostr_sdk` [`Event`] — the
/// same idiom as [`crate::services::acsp::client::AcspClient::publish`], so a
/// voice 31402 and a broker 31402 sign identically.
fn sign_unsigned(ev: &UnsignedAcspEvent, keys: &Keys) -> Result<Event, String> {
    let tags: Vec<Tag> = ev
        .tags
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| Tag::custom(TagKind::Custom(t[0].clone().into()), t[1..].to_vec()))
        .collect();
    EventBuilder::new(Kind::Custom(ev.kind), &ev.content)
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| format!("31402 sign failed: {e}"))
}

/// A spoken acknowledgement for an accepted dispatch, played over PocketTts TTS
/// (COM-15 AC3). Keeps it short and legible — the operator hears which agent it
/// reached and what was understood.
pub fn ack_sentence(accepted: &VoiceIntentAccepted, actor_did: &str) -> String {
    let who = short_did(actor_did);
    let verb = if accepted.intent.verb.is_empty() {
        "your command".to_string()
    } else {
        accepted.intent.verb.clone()
    };
    match &accepted.intent.subject {
        Some(subject) if !subject.is_empty() && accepted.intent.recognised => {
            format!("Sent to {who}. Understood: {verb} — {subject}.")
        }
        _ => format!("Sent to {who}. Understood: {verb}."),
    }
}

/// Legible short form of a did:nostr for the spoken ack: `nostr:ab12…cd34`.
fn short_did(did: &str) -> String {
    let hex = did.strip_prefix("did:nostr:").unwrap_or(did);
    if hex.len() > 12 {
        format!("nostr:{}…{}", &hex[..4], &hex[hex.len() - 4..])
    } else {
        format!("nostr:{hex}")
    }
}

/// A stable-ish short key for the case id from the target + transcript, so the
/// same command to the same agent yields the same 31402 d-tag without pulling in
/// a uuid dependency here.
fn short_key(actor_did: &str, transcript: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    actor_did.hash(&mut h);
    transcript.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn title_for(transcript: &str) -> String {
    let trimmed = transcript.trim();
    let clipped: String = trimmed.chars().take(60).collect();
    if trimmed.chars().count() > 60 {
        format!("Voice: {clipped}…")
    } else {
        format!("Voice: {clipped}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::acsp::events::extract_tag;

    const DID_A: &str =
        "did:nostr:1111111111111111111111111111111111111111111111111111111111111111";

    #[test]
    fn builds_31402_targeted_at_the_did() {
        let ev = build_voice_action_request("find the budget node", DID_A);
        assert_eq!(ev.kind, KIND_ACTION_REQUEST, "voice action is a kind-31402");
        // The target DID is the 31402 subject-id — the governed action is
        // addressed at the selected agent, not a hashed label.
        assert_eq!(extract_tag(&ev.tags, "subject-id"), Some(DID_A));
        assert_eq!(ev.tags[0][0], "d", "d-tag first per the ACSP invariant");
        assert!(ev.tags[0][1].starts_with("vc-voice-"));
        // The transcript rides the content fields.
        let content: serde_json::Value = serde_json::from_str(&ev.content).unwrap();
        assert_eq!(content["fields"]["transcript"], "find the budget node");
        assert_eq!(content["fields"]["target_did"], DID_A);
    }

    #[test]
    fn signs_31402_verifiably() {
        let keys = Keys::generate();
        let ev = build_voice_action_request("summarise the ontology", DID_A);
        let signed = sign_unsigned(&ev, &keys).expect("sign");
        assert_eq!(signed.kind, Kind::Custom(KIND_ACTION_REQUEST));
        assert_eq!(signed.pubkey, keys.public_key());
        signed.verify().expect("a freshly signed 31402 verifies");
    }

    #[test]
    fn non_did_target_is_refused_before_http() {
        // A hashed nickname / free-text label is never a governed target
        // (ADR-037 D7). `dispatch` refuses it before any signing or HTTP, so this
        // resolves without a server.
        let keys = Keys::generate();
        let client =
            VoiceIntentClient::new("http://127.0.0.1:1/v1/voice-intent".to_string(), keys, None);
        let err = tokio_test::block_on(client.dispatch("do a thing", "researcher-7", 200))
            .expect_err("a non-did target must be refused");
        assert!(matches!(err, VoiceIntentError::NotADid(_)));
    }

    #[test]
    fn canonical_did_gate_matches_uri_primitive() {
        assert!(is_canonical_did(DID_A));
        assert!(!is_canonical_did("researcher-7"));
        assert!(!is_canonical_did("did:nostr:xyz"));
        assert!(!is_canonical_did(""));
    }

    #[test]
    fn ack_sentence_names_agent_and_intent() {
        let accepted = VoiceIntentAccepted {
            success: true,
            event_id: Some(7),
            intent: VoiceIntentEcho {
                verb: "query".to_string(),
                action_type_name: "query".to_string(),
                subject: Some("budget node".to_string()),
                recognised: true,
            },
        };
        let line = ack_sentence(&accepted, DID_A);
        assert!(
            line.contains("nostr:1111"),
            "names the target agent: {line}"
        );
        assert!(line.contains("query"), "names the understood verb: {line}");
        assert!(line.contains("budget node"), "names the subject: {line}");
    }
}
