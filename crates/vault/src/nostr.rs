//! The forum 31402 `ActionRequest` (contract C5).
//!
//! **No cryptography is implemented here.** The event id, the BIP-340
//! signature and the key handling all come from `nostr-bbs-core`, which is the
//! forum's own published crate and the only implementation in the estate that
//! is allowed to touch a Nostr key. This module assembles tags and content, and
//! nothing else.
//!
//! The tag set is fixed by C5:
//!
//! | tag | value |
//! |---|---|
//! | `d` | the proposal digest — the replaceable-event address, and the case id |
//! | `context_url` | the subject IRI |
//! | `level` | `content`, `schema` or `demotion` |
//! | `tp-verifiability` / `tp-reversibility` / `tp-stakes` | the operator's task properties |
//!
//! A schema-level proposal declares `Stakes::Critical`, which floors the
//! panel's risk tier at High (decision Q7).

use nostr_bbs_core::event::{compute_event_id, NostrEvent, UnsignedEvent};
use nostr_bbs_core::governance::{
    Reversibility, Stakes, TaskProperties, Verifiability, KIND_ACTION_REQUEST,
};
use nostr_bbs_core::keys::SecretKey;
use vault_core::proposal::{Level, PatchProposal};

/// The panel a proposal is filed against.
pub const PANEL_IDENTIFIER: &str = "ontology-governance";

/// The task properties `vault propose` declares for a given level.
///
/// A patch is **inspectable** (the reviewer reads the diff, and the build
/// either reproduces it or does not) and **reversible** (git is the rollback).
/// Only the stakes move: a schema change is `Critical`, a demotion
/// `Significant`, content `Bounded`.
#[must_use]
pub fn task_properties(level: Level) -> TaskProperties {
    TaskProperties::new(
        Verifiability::Inspectable,
        Reversibility::Reversible,
        match level {
            Level::Schema => Stakes::Critical,
            Level::Demotion => Stakes::Significant,
            Level::Content => Stakes::Bounded,
        },
    )
}

/// Build the unsigned 31402 for `proposal`.
///
/// # Errors
/// When the proposal carries blockers: a blocked proposal is never posted.
pub fn action_request(
    proposal: &PatchProposal,
    pubkey_hex: &str,
    created_at: u64,
) -> Result<UnsignedEvent, String> {
    if !proposal.is_postable() {
        return Err(format!(
            "refusing to post: {} blocker(s); first: {}",
            proposal.blockers.len(),
            proposal.blockers[0]
        ));
    }
    let mut tags: Vec<Vec<String>> = vec![
        vec!["d".into(), proposal.digest_hex().to_owned()],
        vec!["context_url".into(), proposal.iri.clone()],
        vec!["level".into(), proposal.level.as_str().to_owned()],
        vec!["panel".into(), PANEL_IDENTIFIER.to_owned()],
        vec!["generation".into(), proposal.generation.clone()],
        vec!["expiration".into(), proposal.stale_after.clone()],
    ];
    // A grouped proposal announces its SCOPE in the tags, not only inside the
    // content. The single most decision-relevant fact for a human deciding
    // whether to open a case is how many pages it changes, and a reviewer
    // scanning a relay sees tags before content. Added only when grouped, so a
    // single-page event is byte-identical to what it was before grouping
    // existed.
    if proposal.is_grouped() {
        tags.push(vec!["pages".into(), proposal.pages.len().to_string()]);
    }
    tags.extend(task_properties(proposal.level).to_tags());

    Ok(UnsignedEvent {
        pubkey: pubkey_hex.to_owned(),
        created_at,
        kind: KIND_ACTION_REQUEST,
        tags,
        content: serde_json::to_string(proposal).map_err(|e| e.to_string())?,
    })
}

/// Load a signing key from a 64-character hex secret.
///
/// # Errors
/// On a malformed hex string or an invalid secp256k1 scalar.
pub fn signing_key_from_hex(hex_secret: &str) -> Result<SecretKey, String> {
    let bytes = hex::decode(hex_secret.trim()).map_err(|e| format!("secret is not hex: {e}"))?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "secret must be 32 bytes".to_owned())?;
    SecretKey::from_bytes(array).map_err(|e| format!("invalid secret key: {e}"))
}

/// The hex public key a signing key derives.
#[must_use]
pub fn public_key_hex(key: &SecretKey) -> String {
    key.public_key().to_hex()
}

/// Sign a 31402.
///
/// The event id and the BIP-340 signature both come from `nostr-bbs-core`;
/// this function only checks that the declared `pubkey` is the one the key
/// derives, so a self-invalid event can never be produced.
///
/// # Errors
/// When the event's `pubkey` does not match the signing key, or signing fails.
pub fn sign(event: UnsignedEvent, key: &SecretKey) -> Result<NostrEvent, String> {
    let derived = public_key_hex(key);
    if event.pubkey != derived {
        return Err(format!(
            "pubkey mismatch: event has {}, signing key derives {derived}",
            event.pubkey
        ));
    }
    let id = compute_event_id(&event);
    let signature = key.sign(&id).map_err(|e| format!("schnorr sign: {e}"))?;
    Ok(NostrEvent {
        id: hex::encode(id),
        pubkey: event.pubkey,
        created_at: event.created_at,
        kind: event.kind,
        tags: event.tags,
        content: event.content,
        sig: signature.to_hex(),
    })
}

/// NIP-42 client authentication event kind.
pub const KIND_CLIENT_AUTH: u64 = 22242;

/// Build and sign the NIP-42 kind-22242 answer to a relay's `AUTH` challenge.
///
/// # Errors
/// When signing fails.
pub fn auth_event(
    challenge: &str,
    relay_url: &str,
    key: &SecretKey,
    created_at: u64,
) -> Result<NostrEvent, String> {
    sign(
        UnsignedEvent {
            pubkey: public_key_hex(key),
            created_at,
            kind: KIND_CLIENT_AUTH,
            tags: vec![
                vec!["relay".into(), relay_url.to_owned()],
                vec!["challenge".into(), challenge.to_owned()],
            ],
            content: String::new(),
        },
        key,
    )
}

/// What one relay frame means to [`publish`].
#[derive(Debug, PartialEq, Eq)]
enum Frame {
    /// `["AUTH", <challenge>]`.
    Challenge(String),
    /// `["OK", <id>, <accepted>, <message>]`.
    Ok { id: String, accepted: bool, message: String },
    /// Anything else (`NOTICE`, `EOSE`, non-JSON).
    Other,
}

fn parse_frame(text: &str) -> Frame {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Frame::Other;
    };
    match value[0].as_str() {
        Some("AUTH") => value[1]
            .as_str()
            .map_or(Frame::Other, |c| Frame::Challenge(c.to_owned())),
        Some("OK") => Frame::Ok {
            id: value[1].as_str().unwrap_or_default().to_owned(),
            accepted: value[2].as_bool().unwrap_or(false),
            message: value[3].as_str().unwrap_or_default().to_owned(),
        },
        _ => Frame::Other,
    }
}

/// Publish a signed event to a relay over one WebSocket connection.
///
/// Speaks NIP-42: a relay that rejects the event with `auth-required:` is
/// answered with a kind-22242 over its challenge, signed by `key`, and the
/// event is sent once more. The challenge may arrive before or after the
/// rejection. Without a `key`, an `auth-required` rejection is returned as is.
///
/// # Errors
/// Any transport failure, a rejected `AUTH`, or an `OK` frame whose acceptance
/// flag is `false`.
pub fn publish(
    event: &NostrEvent,
    relay_url: &str,
    key: Option<&SecretKey>,
) -> anyhow::Result<String> {
    use tungstenite::Message;

    let (mut socket, _) = tungstenite::connect(relay_url)
        .map_err(|e| anyhow::anyhow!("connecting to {relay_url}: {e}"))?;
    let event_frame = serde_json::to_string(&serde_json::json!(["EVENT", event]))?;
    socket.send(Message::Text(event_frame.clone()))?;

    let mut challenge: Option<String> = None;
    let mut awaiting_auth: Option<String> = None; // id of the 22242 sent
    let mut needs_auth = false;
    let mut retried = false;

    let result = (|| -> anyhow::Result<String> {
        for _ in 0..32 {
            let Message::Text(text) = socket.read()? else {
                continue;
            };
            match parse_frame(&text) {
                Frame::Challenge(c) => challenge = Some(c),
                Frame::Ok { id, accepted, message } if Some(&id) == awaiting_auth.as_ref() => {
                    anyhow::ensure!(accepted, "relay rejected NIP-42 AUTH: {message}");
                    awaiting_auth = None;
                    retried = true;
                    socket.send(Message::Text(event_frame.clone()))?;
                }
                Frame::Ok { id, accepted, message } if id == event.id => {
                    if accepted {
                        return Ok(event.id.clone());
                    }
                    anyhow::ensure!(
                        message.starts_with("auth-required") && key.is_some() && !retried,
                        "relay rejected the event: {message}"
                    );
                    needs_auth = true;
                }
                _ => {}
            }
            if needs_auth && awaiting_auth.is_none() && !retried {
                if let (Some(c), Some(k)) = (challenge.as_deref(), key) {
                    let created_at = u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp())
                        .unwrap_or_default();
                    let auth = auth_event(c, relay_url, k, created_at).map_err(anyhow::Error::msg)?;
                    awaiting_auth = Some(auth.id.clone());
                    socket.send(Message::Text(serde_json::to_string(&serde_json::json!([
                        "AUTH", auth
                    ]))?))?;
                }
            }
        }
        anyhow::bail!("relay {relay_url} never acknowledged the event")
    })();
    let _ = socket.close(None);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::promotion::Blocker;

    const PUBKEY: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn proposal(level: Level, blockers: Vec<Blocker>) -> PatchProposal {
        PatchProposal::new(
            level,
            "urn:ngm:class:knowledge-graph",
            "Knowledge Graph",
            "quality is understated",
            "-quality: 0.35\n+quality: 0.55\n",
            blockers,
            "process:vault/1.0",
            "visionGraph@abc1234",
            "2026-10-06T00:00:00Z",
        )
    }

    fn tag<'a>(event: &'a UnsignedEvent, name: &str) -> Option<&'a str> {
        event
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some(name))
            .and_then(|t| t.get(1))
            .map(String::as_str)
    }

    #[test]
    fn the_event_carries_every_c5_tag() {
        let p = proposal(Level::Content, vec![]);
        let e = action_request(&p, PUBKEY, 1_790_000_000).unwrap();
        assert_eq!(e.kind, 31402);
        assert_eq!(tag(&e, "d"), Some(p.digest_hex()));
        assert_eq!(
            tag(&e, "context_url"),
            Some("urn:ngm:class:knowledge-graph")
        );
        assert_eq!(tag(&e, "level"), Some("content"));
        assert_eq!(tag(&e, "panel"), Some("ontology-governance"));
        assert_eq!(tag(&e, "generation"), Some("visionGraph@abc1234"));
        assert_eq!(tag(&e, "expiration"), Some("2026-10-06T00:00:00Z"));
    }

    #[test]
    fn the_content_is_the_patch_proposal_json() {
        let p = proposal(Level::Content, vec![]);
        let e = action_request(&p, PUBKEY, 0).unwrap();
        let round_trip: PatchProposal = serde_json::from_str(&e.content).unwrap();
        assert_eq!(round_trip, p);
    }

    #[test]
    fn a_schema_proposal_declares_critical_stakes() {
        let e = action_request(&proposal(Level::Schema, vec![]), PUBKEY, 0).unwrap();
        let props = TaskProperties::from_tags(&e.tags).unwrap();
        assert_eq!(props.stakes, Stakes::Critical);
        assert_eq!(
            props.tier_floor(),
            nostr_bbs_core::governance::RiskTier::High
        );
    }

    #[test]
    fn a_content_proposal_does_not_floor_the_tier_at_high() {
        let e = action_request(&proposal(Level::Content, vec![]), PUBKEY, 0).unwrap();
        let props = TaskProperties::from_tags(&e.tags).unwrap();
        assert_eq!(props.stakes, Stakes::Bounded);
    }

    #[test]
    fn a_blocked_proposal_is_never_turned_into_an_event() {
        let p = proposal(
            Level::Content,
            vec![Blocker::new("SUBCLASS_CYCLE", "A -> B -> A")],
        );
        let err = action_request(&p, PUBKEY, 0).unwrap_err();
        assert!(err.contains("SUBCLASS_CYCLE"), "{err}");
    }

    #[test]
    fn signing_round_trips_through_nostr_bbs_core() {
        let secret = "0000000000000000000000000000000000000000000000000000000000000042";
        let key = signing_key_from_hex(secret).unwrap();
        let pubkey = public_key_hex(&key);
        let event =
            action_request(&proposal(Level::Content, vec![]), &pubkey, 1_790_000_000).unwrap();
        let signed = sign(event, &key).unwrap();
        assert_eq!(signed.id.len(), 64);
        assert_eq!(signed.sig.len(), 128);
        assert_eq!(signed.pubkey, pubkey);
        assert_eq!(
            hex::encode(nostr_bbs_core::event::recompute_event_id(&signed)),
            signed.id
        );
    }

    #[test]
    fn signing_with_the_wrong_pubkey_is_refused() {
        let key = signing_key_from_hex(
            "0000000000000000000000000000000000000000000000000000000000000042",
        )
        .unwrap();
        let event = action_request(&proposal(Level::Content, vec![]), PUBKEY, 0).unwrap();
        assert!(sign(event, &key).is_err());
    }

    #[test]
    fn a_malformed_secret_is_rejected() {
        assert!(signing_key_from_hex("nothex").is_err());
        assert!(signing_key_from_hex("00").is_err());
    }

    #[test]
    fn demotion_sits_between_content_and_schema() {
        assert_eq!(task_properties(Level::Demotion).stakes, Stakes::Significant);
    }

    const SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000003";

    #[test]
    fn auth_event_is_a_verifiable_22242_over_relay_and_challenge() {
        let key = signing_key_from_hex(SECRET).unwrap();
        let e = auth_event("chal-1", "wss://relay.example", &key, 1_700_000_000).unwrap();
        assert_eq!(e.kind, KIND_CLIENT_AUTH);
        assert_eq!(e.pubkey, public_key_hex(&key));
        assert_eq!(e.content, "");
        assert!(e.tags.contains(&vec!["relay".to_owned(), "wss://relay.example".to_owned()]));
        assert!(e.tags.contains(&vec!["challenge".to_owned(), "chal-1".to_owned()]));
        assert!(nostr_bbs_core::event::verify_event(&e));
    }

    #[test]
    fn frames_parse_into_challenge_ok_and_other() {
        assert_eq!(parse_frame(r#"["AUTH","abc"]"#), Frame::Challenge("abc".into()));
        assert_eq!(
            parse_frame(r#"["OK","id1",false,"auth-required: x"]"#),
            Frame::Ok { id: "id1".into(), accepted: false, message: "auth-required: x".into() }
        );
        assert_eq!(parse_frame(r#"["NOTICE","hi"]"#), Frame::Other);
        assert_eq!(parse_frame("not json"), Frame::Other);
    }

    /// A one-connection relay that demands NIP-42 before accepting an EVENT.
    /// `challenge_first` sends the challenge on connect; otherwise only after
    /// the first rejection. Returns the relay URL and a handle yielding the
    /// AUTH event it accepted.
    fn auth_relay(challenge_first: bool) -> (String, std::thread::JoinHandle<NostrEvent>) {
        use tungstenite::Message;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let relay_url = url.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let send = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>, v: serde_json::Value| {
                ws.send(Message::Text(v.to_string())).unwrap();
            };
            if challenge_first {
                send(&mut ws, serde_json::json!(["AUTH", "chal-xyz"]));
            }
            let mut authed: Option<NostrEvent> = None;
            loop {
                let Ok(Message::Text(text)) = ws.read() else { break };
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                match v[0].as_str() {
                    Some("EVENT") => {
                        let id = v[1]["id"].as_str().unwrap().to_owned();
                        if authed.is_some() {
                            send(&mut ws, serde_json::json!(["OK", id, true, ""]));
                            break;
                        }
                        send(&mut ws, serde_json::json!(["OK", id, false, "auth-required: NIP-42 AUTH required to publish"]));
                        if !challenge_first {
                            send(&mut ws, serde_json::json!(["AUTH", "chal-xyz"]));
                        }
                    }
                    Some("AUTH") => {
                        let e: NostrEvent = serde_json::from_value(v[1].clone()).unwrap();
                        let ok = e.kind == KIND_CLIENT_AUTH
                            && nostr_bbs_core::event::verify_event(&e)
                            && e.tags.contains(&vec!["challenge".to_owned(), "chal-xyz".to_owned()])
                            && e.tags.contains(&vec!["relay".to_owned(), relay_url.clone()]);
                        send(&mut ws, serde_json::json!(["OK", e.id, ok, if ok { "" } else { "invalid: auth" }]));
                        if ok {
                            authed = Some(e);
                        }
                    }
                    _ => {}
                }
            }
            authed.expect("relay never saw a valid AUTH")
        });
        (url, handle)
    }

    fn signed_request(key: &SecretKey) -> NostrEvent {
        let unsigned = action_request(&proposal(Level::Schema, vec![]), &public_key_hex(key), 1).unwrap();
        sign(unsigned, key).unwrap()
    }

    #[test]
    fn publish_answers_nip42_when_the_challenge_comes_first() {
        let key = signing_key_from_hex(SECRET).unwrap();
        let (url, relay) = auth_relay(true);
        let event = signed_request(&key);
        assert_eq!(publish(&event, &url, Some(&key)).unwrap(), event.id);
        assert_eq!(relay.join().unwrap().pubkey, public_key_hex(&key));
    }

    #[test]
    fn publish_answers_nip42_when_the_challenge_follows_the_rejection() {
        let key = signing_key_from_hex(SECRET).unwrap();
        let (url, relay) = auth_relay(false);
        let event = signed_request(&key);
        assert_eq!(publish(&event, &url, Some(&key)).unwrap(), event.id);
        relay.join().unwrap();
    }

    #[test]
    fn publish_without_a_key_returns_the_auth_rejection() {
        let key = signing_key_from_hex(SECRET).unwrap();
        let (url, _relay) = auth_relay(true);
        let err = publish(&signed_request(&key), &url, None).unwrap_err().to_string();
        assert!(err.contains("auth-required"), "{err}");
    }
}
