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

/// Publish a signed event to a relay over a single WebSocket round trip.
///
/// # Errors
/// Any transport failure, or an `OK` frame whose acceptance flag is `false`.
pub fn publish(event: &NostrEvent, relay_url: &str) -> anyhow::Result<String> {
    use tungstenite::Message;

    let (mut socket, _) = tungstenite::connect(relay_url)
        .map_err(|e| anyhow::anyhow!("connecting to {relay_url}: {e}"))?;
    let frame = serde_json::to_string(&serde_json::json!(["EVENT", event]))?;
    socket.send(Message::Text(frame))?;

    // The relay answers with ["OK", <id>, <accepted>, <message>].
    for _ in 0..8 {
        let Message::Text(text) = socket.read()? else {
            continue;
        };
        let value: serde_json::Value = serde_json::from_str(&text)?;
        if value[0] == "OK" && value[1] == event.id.as_str() {
            let accepted = value[2].as_bool().unwrap_or(false);
            let message = value[3].as_str().unwrap_or_default().to_owned();
            let _ = socket.close(None);
            anyhow::ensure!(accepted, "relay rejected the event: {message}");
            return Ok(event.id.clone());
        }
    }
    let _ = socket.close(None);
    anyhow::bail!("relay {relay_url} never acknowledged the event")
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
}
