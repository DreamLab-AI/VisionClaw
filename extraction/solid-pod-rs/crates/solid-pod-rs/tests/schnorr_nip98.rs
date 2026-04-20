//! NIP-98 Schnorr signature verification tests.
//!
//! These are feature-gated behind `nip98-schnorr`. They verify that
//! the structural checks still pass when signature validation is
//! enabled, and that tampered events are rejected.

#![cfg(feature = "nip98-schnorr")]

use solid_pod_rs::auth::nip98::{compute_event_id, verify_schnorr_signature, Nip98Event};
use solid_pod_rs::PodError;

// We use a canonically-hashed event with a placeholder signature. The
// Schnorr verifier must reject it (the signature isn't real), proving
// the code path is wired up.

#[test]
fn schnorr_verify_rejects_fake_signature() {
    let mut event = Nip98Event {
        id: String::new(),
        pubkey: "f".repeat(64),
        created_at: 1_700_000_000,
        kind: 27235,
        tags: vec![
            vec!["u".into(), "https://api.example.com/x".into()],
            vec!["method".into(), "GET".into()],
        ],
        content: String::new(),
        sig: "0".repeat(128),
    };
    event.id = compute_event_id(&event);
    // Pubkey / signature are bogus — verifier must return an error.
    let err = verify_schnorr_signature(&event).unwrap_err();
    assert!(matches!(err, PodError::Nip98(_)));
}

#[test]
fn schnorr_verify_rejects_tampered_event_id() {
    let event = Nip98Event {
        id: "0".repeat(64),
        pubkey: "f".repeat(64),
        created_at: 1_700_000_000,
        kind: 27235,
        tags: vec![vec!["u".into(), "https://x".into()]],
        content: String::new(),
        sig: "0".repeat(128),
    };
    let err = verify_schnorr_signature(&event).unwrap_err();
    match err {
        PodError::Nip98(msg) => {
            assert!(msg.contains("event id"), "expected id-mismatch error, got: {msg}");
        }
        _ => panic!("wrong error type"),
    }
}
