//! NIP-98 HTTP authentication: structural verification.
//!
//! Reference: <https://github.com/nostr-protocol/nips/blob/master/98.md>
//!
//! Wire format: `Authorization: Nostr <base64(json(event))>` where
//! the event is a kind-27235 Nostr event with tags `u` (URL),
//! `method`, and optional `payload` (SHA-256 of request body).
//!
//! This Phase 1 implementation performs all structural checks:
//!
//! - Base64 + JSON decode with a strict size cap.
//! - Event kind must be 27235.
//! - Pubkey must be 64 hex chars.
//! - `created_at` must be within a configurable tolerance.
//! - `u` tag (trailing-slash-normalized) must match expected URL.
//! - `method` tag (case-insensitive) must match expected method.
//! - If a body is supplied, its SHA-256 must match the `payload`
//!   tag; if no body is supplied, any `payload` tag present is
//!   echoed back unchanged.
//!
//! Cryptographic signature verification (Schnorr over secp256k1) is
//! the Phase 2 deliverable; see `verify`'s doc for how to wire in a
//! signer-verifier without changing this module's public API.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::PodError;

const HTTP_AUTH_KIND: u64 = 27235;
const TIMESTAMP_TOLERANCE: u64 = 60;
const MAX_EVENT_SIZE: usize = 64 * 1024;
const NOSTR_PREFIX: &str = "Nostr ";

/// Parsed NIP-98 event.
#[derive(Debug, Clone, Deserialize)]
pub struct Nip98Event {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

/// Verified NIP-98 token fields returned to the caller.
#[derive(Debug, Clone)]
pub struct Nip98Verified {
    pub pubkey: String,
    pub url: String,
    pub method: String,
    pub payload_hash: Option<String>,
    pub created_at: u64,
}

/// Verify a NIP-98 `Authorization` header against expected URL +
/// method + optional body.
///
/// Returns the signer pubkey on success.
///
/// # Arguments
/// * `header` — full `Authorization` header value, e.g. `"Nostr base64..."`.
/// * `url` — expected URL.
/// * `method` — expected HTTP method.
/// * `body_hash` — `Some(h)` where `h` is the body bytes, for
///    payload-tag matching; `None` if no body.
pub async fn verify(
    header: &str,
    url: &str,
    method: &str,
    body_hash: Option<&[u8]>,
) -> Result<String, PodError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    verify_at(header, url, method, body_hash, now).map(|v| v.pubkey)
}

/// `verify` with an explicit timestamp (for deterministic testing).
pub fn verify_at(
    header: &str,
    expected_url: &str,
    expected_method: &str,
    body: Option<&[u8]>,
    now: u64,
) -> Result<Nip98Verified, PodError> {
    let token = header
        .strip_prefix(NOSTR_PREFIX)
        .ok_or_else(|| PodError::Nip98("missing 'Nostr ' prefix".into()))?
        .trim();

    if token.len() > MAX_EVENT_SIZE {
        return Err(PodError::Nip98("token too large".into()));
    }
    let json_bytes = BASE64.decode(token)?;
    if json_bytes.len() > MAX_EVENT_SIZE {
        return Err(PodError::Nip98("decoded token too large".into()));
    }
    let event: Nip98Event = serde_json::from_slice(&json_bytes)?;

    if event.kind != HTTP_AUTH_KIND {
        return Err(PodError::Nip98(format!(
            "wrong kind: expected {HTTP_AUTH_KIND}, got {}",
            event.kind
        )));
    }
    if event.pubkey.len() != 64 || hex::decode(&event.pubkey).is_err() {
        return Err(PodError::Nip98("invalid pubkey".into()));
    }
    if now.abs_diff(event.created_at) > TIMESTAMP_TOLERANCE {
        return Err(PodError::Nip98(format!(
            "timestamp outside tolerance: event={}, now={now}",
            event.created_at
        )));
    }

    let token_url = get_tag(&event, "u")
        .ok_or_else(|| PodError::Nip98("missing 'u' tag".into()))?;
    if normalize_url(&token_url) != normalize_url(expected_url) {
        return Err(PodError::Nip98(format!(
            "URL mismatch: token={token_url}, expected={expected_url}"
        )));
    }

    let token_method = get_tag(&event, "method")
        .ok_or_else(|| PodError::Nip98("missing 'method' tag".into()))?;
    if token_method.to_uppercase() != expected_method.to_uppercase() {
        return Err(PodError::Nip98(format!(
            "method mismatch: token={token_method}, expected={expected_method}"
        )));
    }

    let payload_tag = get_tag(&event, "payload");
    let verified_payload_hash = match body {
        Some(b) if !b.is_empty() => {
            let expected = payload_tag
                .as_ref()
                .ok_or_else(|| PodError::Nip98("body provided but no payload tag".into()))?;
            let actual = hex::encode(Sha256::digest(b));
            if expected.to_lowercase() != actual.to_lowercase() {
                return Err(PodError::Nip98("payload hash mismatch".into()));
            }
            Some(expected.clone())
        }
        _ => payload_tag,
    };

    // Signature verification is the Phase 2 deliverable. When wired
    // in, add a final step here that recomputes the NIP-01 canonical
    // event ID, verifies it matches `event.id`, and verifies the
    // Schnorr signature over the id using `event.pubkey`.

    Ok(Nip98Verified {
        pubkey: event.pubkey,
        url: token_url,
        method: token_method,
        payload_hash: verified_payload_hash,
        created_at: event.created_at,
    })
}

fn get_tag(event: &Nip98Event, name: &str) -> Option<String> {
    event
        .tags
        .iter()
        .find(|t| t.first().map(|s| s.as_str()) == Some(name))
        .and_then(|t| t.get(1).cloned())
}

fn normalize_url(u: &str) -> &str {
    u.trim_end_matches('/')
}

/// Helper: build an `Authorization: Nostr <token>` header value.
pub fn authorization_header(token_b64: &str) -> String {
    format!("{NOSTR_PREFIX}{token_b64}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_event(event: &serde_json::Value) -> String {
        BASE64.encode(serde_json::to_string(event).unwrap().as_bytes())
    }

    fn valid_event(url: &str, method: &str, ts: u64, body: Option<&[u8]>) -> serde_json::Value {
        let mut tags = vec![
            vec!["u".to_string(), url.to_string()],
            vec!["method".to_string(), method.to_string()],
        ];
        if let Some(b) = body {
            tags.push(vec![
                "payload".to_string(),
                hex::encode(Sha256::digest(b)),
            ]);
        }
        serde_json::json!({
            "id": "0".repeat(64),
            "pubkey": "a".repeat(64),
            "created_at": ts,
            "kind": 27235,
            "tags": tags,
            "content": "",
            "sig": "0".repeat(128),
        })
    }

    #[test]
    fn rejects_missing_prefix() {
        let err = verify_at("Bearer xyz", "https://a/b", "GET", None, 0).unwrap_err();
        assert!(matches!(err, PodError::Nip98(_)));
    }

    #[test]
    fn accepts_well_formed_event_no_body() {
        let ts = 1_700_000_000u64;
        let ev = valid_event("https://api.example.com/x", "GET", ts, None);
        let hdr = authorization_header(&encode_event(&ev));
        let r = verify_at(&hdr, "https://api.example.com/x", "GET", None, ts).unwrap();
        assert_eq!(r.pubkey, "a".repeat(64));
        assert_eq!(r.url, "https://api.example.com/x");
    }

    #[test]
    fn accepts_trailing_slash_variation() {
        let ts = 1_700_000_000u64;
        let ev = valid_event("https://api.example.com/x/", "GET", ts, None);
        let hdr = authorization_header(&encode_event(&ev));
        verify_at(&hdr, "https://api.example.com/x", "GET", None, ts).unwrap();
    }

    #[test]
    fn rejects_url_mismatch() {
        let ts = 1_700_000_000u64;
        let ev = valid_event("https://good/x", "GET", ts, None);
        let hdr = authorization_header(&encode_event(&ev));
        let err = verify_at(&hdr, "https://evil/x", "GET", None, ts).unwrap_err();
        assert!(matches!(err, PodError::Nip98(_)));
    }

    #[test]
    fn rejects_payload_mismatch() {
        let ts = 1_700_000_000u64;
        let ev = valid_event("https://a/b", "POST", ts, Some(b"original"));
        let hdr = authorization_header(&encode_event(&ev));
        let err = verify_at(&hdr, "https://a/b", "POST", Some(b"tampered"), ts).unwrap_err();
        assert!(matches!(err, PodError::Nip98(_)));
    }

    #[test]
    fn rejects_body_without_payload_tag() {
        let ts = 1_700_000_000u64;
        let ev = valid_event("https://a/b", "POST", ts, None);
        let hdr = authorization_header(&encode_event(&ev));
        let err = verify_at(&hdr, "https://a/b", "POST", Some(b"sneaky"), ts).unwrap_err();
        assert!(matches!(err, PodError::Nip98(_)));
    }

    #[test]
    fn rejects_expired_timestamp() {
        let ts = 1_700_000_000u64;
        let ev = valid_event("https://a/b", "GET", ts, None);
        let hdr = authorization_header(&encode_event(&ev));
        let err = verify_at(&hdr, "https://a/b", "GET", None, ts + 120).unwrap_err();
        assert!(matches!(err, PodError::Nip98(_)));
    }

    #[test]
    fn rejects_wrong_kind() {
        let ts = 1_700_000_000u64;
        let mut ev = valid_event("https://a/b", "GET", ts, None);
        ev["kind"] = serde_json::json!(1);
        let hdr = authorization_header(&encode_event(&ev));
        let err = verify_at(&hdr, "https://a/b", "GET", None, ts).unwrap_err();
        assert!(matches!(err, PodError::Nip98(_)));
    }
}
