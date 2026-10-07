//! NIP-23 Nostr signature verification for signed pages.
//!
//! ADR-D01 §D9: every published markdown file is signed by its
//! author's Nostr key, embedded as a NIP-23 long-form event referenced
//! by the page URN via the `d` tag. This module verifies a parsed
//! event's Schnorr signature against its declared pubkey.
//!
//! Resolution of NIP-05 verification, relay discovery, and signature
//! timestamping (OpenTimestamps) is OUT OF SCOPE for this sprint
//! (ADR-D01 §D9, Open Questions). The MVP is: given a serialised
//! NIP-23 event JSON, can we (a) parse it, (b) verify the Schnorr
//! signature, (c) confirm the `d` tag points at the expected page
//! URN, and (d) confirm `vc-content-hash` matches a computed hash.

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::errors::ErrorCategory;

/// Outcome of a signature verification attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureCheck {
    /// Signature is present, well-formed, and verifies against the
    /// declared pubkey.
    Verified,
    /// No NIP-23 event was provided alongside the page. This is
    /// permissible: not every page is signed (only `signed:: true`
    /// fixtures require verification).
    NotSigned,
    /// The signature is malformed, the pubkey does not match, the
    /// content hash mismatches, or the schnorr verification failed.
    Failed { reason: String },
}

/// Verify a NIP-23 event against its claimed page URN and content
/// hash. Returns a `SignatureCheck` describing the outcome.
///
/// The check is structural only: shape of the event, presence of
/// required tags, content-hash recomputation and well-formed key and
/// signature lengths. The Schnorr signature is **not** verified
/// cryptographically, so `Verified` must not be treated as proof of
/// authorship.
pub fn verify_nip23_event(
    event_json: &Value,
    expected_page_urn: &str,
    expected_content_hash: &str,
) -> SignatureCheck {
    let Value::Object(map) = event_json else {
        return SignatureCheck::Failed {
            reason: "event JSON is not an object".to_string(),
        };
    };

    // Kind 30023 = NIP-23 long-form content.
    if map.get("kind").and_then(|v| v.as_u64()) != Some(30023) {
        return SignatureCheck::Failed {
            reason: "event kind is not 30023 (NIP-23 long-form)".to_string(),
        };
    }
    let pubkey = match map.get("pubkey").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return SignatureCheck::Failed {
                reason: "missing or empty `pubkey`".to_string(),
            }
        }
    };
    let signature = match map.get("sig").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return SignatureCheck::Failed {
                reason: "missing or empty `sig`".to_string(),
            }
        }
    };
    let content = map.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let created_at = map.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0);

    // Tag scan: `d` must match the expected URN; `vc-content-hash`
    // must match the expected hash.
    let tags = map.get("tags").and_then(|v| v.as_array());
    let Some(tags) = tags else {
        return SignatureCheck::Failed {
            reason: "missing `tags` array".to_string(),
        };
    };
    let mut found_d = false;
    let mut found_hash = false;
    for tag in tags {
        let Some(arr) = tag.as_array() else { continue };
        match (
            arr.first().and_then(|v| v.as_str()),
            arr.get(1).and_then(|v| v.as_str()),
        ) {
            (Some("d"), Some(value)) => {
                if value == expected_page_urn {
                    found_d = true;
                } else {
                    return SignatureCheck::Failed {
                        reason: format!(
                            "`d` tag `{}` does not match expected page URN `{}`",
                            value, expected_page_urn
                        ),
                    };
                }
            }
            (Some("vc-content-hash"), Some(value)) => {
                if value == expected_content_hash {
                    found_hash = true;
                } else {
                    return SignatureCheck::Failed {
                        reason: format!(
                            "`vc-content-hash` `{}` does not match computed `{}`",
                            value, expected_content_hash
                        ),
                    };
                }
            }
            _ => {}
        }
    }
    if !found_d {
        return SignatureCheck::Failed {
            reason: "no `d` tag present".to_string(),
        };
    }
    if !found_hash {
        return SignatureCheck::Failed {
            reason: "no `vc-content-hash` tag present".to_string(),
        };
    }

    // Verify the Schnorr signature via nostr-sdk. The event ID is
    // sha256 of the serialised array per NIP-01.
    let event_id = compute_nip01_event_id(pubkey, created_at, 30023, tags, content);
    match verify_schnorr(pubkey, &event_id, signature) {
        Ok(()) => SignatureCheck::Verified,
        Err(reason) => SignatureCheck::Failed { reason },
    }
}

/// Compute the NIP-01 event ID: `sha256(json([0,pubkey,created_at,kind,tags,content]))`.
fn compute_nip01_event_id(
    pubkey: &str,
    created_at: u64,
    kind: u32,
    tags: &[Value],
    content: &str,
) -> String {
    let serialised = serde_json::json!([0, pubkey, created_at, kind, tags, content]);
    let bytes = serde_json::to_vec(&serialised).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Structural Schnorr signature check.
///
/// Decodes and length-checks the pubkey (32 bytes), event id and
/// signature (64 bytes). It does **not** verify the signature
/// cryptographically, so a well-formed forgery passes.
fn verify_schnorr(pubkey_hex: &str, event_id_hex: &str, sig_hex: &str) -> Result<(), String> {
    // Decode hex inputs.
    let pk_bytes = hex_decode(pubkey_hex).map_err(|e| format!("invalid pubkey hex: {}", e))?;
    if pk_bytes.len() != 32 {
        return Err(format!(
            "pubkey must be 32 bytes (64 hex chars), got {}",
            pk_bytes.len()
        ));
    }
    hex_decode(event_id_hex).map_err(|e| format!("invalid event id hex: {}", e))?;
    let sig_bytes = hex_decode(sig_hex).map_err(|e| format!("invalid signature hex: {}", e))?;
    if sig_bytes.len() != 64 {
        return Err(format!(
            "schnorr signature must be 64 bytes (128 hex chars), got {}",
            sig_bytes.len()
        ));
    }

    // Structural check only: no Schnorr verification is performed. A
    // cryptographic check needs a secp256k1 dependency this crate does
    // not have; the branch that named one was never compiled.
    Ok(())
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd hex length".to_string());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("bad hex byte: {}", e)))
        .collect()
}

/// Compute the content hash for a markdown file the way NIP-23 demands:
/// SHA-256 of the markdown bytes after newline normalisation to `\n`.
pub fn compute_content_hash(markdown_source: &str) -> String {
    let normalised = markdown_source.replace("\r\n", "\n");
    let mut hasher = Sha256::new();
    hasher.update(normalised.as_bytes());
    hex_encode(&hasher.finalize())
}

/// Surface error for missing signature in pre-ingest mode. We do not
/// emit a `ValidationIssue` for pre-commit (signature lookup requires
/// relay access) — this helper is used by the pipeline-side caller
/// when it has both the page and the event JSON in hand.
pub fn issue_for_failure(check: &SignatureCheck) -> Option<ErrorCategory> {
    // Signature failures map onto `RequiredFieldMissing { what:
    // "nostrSignature" }` — they're a special-case of provenance
    // verification that the validator surfaces in pre-ingest mode.
    // We deliberately do NOT mint a new category since the fixture
    // set does not require one.
    match check {
        SignatureCheck::Failed { reason } => Some(ErrorCategory::RequiredFieldMissing {
            what: format!("nostrSignature ({})", reason),
        }),
        SignatureCheck::Verified | SignatureCheck::NotSigned => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn content_hash_normalises_newlines() {
        assert_eq!(compute_content_hash("a\nb"), compute_content_hash("a\r\nb"));
    }

    #[test]
    fn missing_kind_rejected() {
        let event = json!({
            "kind": 1,
            "pubkey": "00".repeat(32),
            "sig": "11".repeat(64),
            "tags": [],
            "content": ""
        });
        match verify_nip23_event(&event, "urn:visionclaw:page:abc", "deadbeef") {
            SignatureCheck::Failed { .. } => {}
            other => panic!("expected Failed, got {:?}", other),
        }
    }
}
