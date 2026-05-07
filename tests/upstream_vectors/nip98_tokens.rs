// tests/upstream_vectors/nip98_tokens.rs
//! L1 reference vectors: NIP-98 HTTP Auth (kind-27235).

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn nip98_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("nip98-tokens.json");
    assert_meta_block(&f, "NIP-98");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(vectors.len() >= 6, "nip98-tokens fixture must have >= 6 vectors");
}

#[test]
fn nip98_canonical_event_has_required_tags() {
    let f = load_fixture("nip98-tokens.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let event = &v["event"];
        assert_eq!(
            event["kind"].as_i64(),
            Some(27235),
            "valid NIP-98 event MUST have kind 27235"
        );
        let tags = event["tags"].as_array().unwrap();
        let has_u = tags.iter().any(|t| t[0].as_str() == Some("u"));
        let has_method = tags.iter().any(|t| t[0].as_str() == Some("method"));
        assert!(has_u, "valid NIP-98 event MUST have a 'u' tag");
        assert!(has_method, "valid NIP-98 event MUST have a 'method' tag");
    }
}

#[test]
#[ignore = "wires into solid-pod-rs handle_nip98_auth and forum auth/mod.rs::verify_event; canonical signed event in fixture verifies under canonical pubkey via Schnorr — Phase 2 deliverable"]
fn nip98_canonical_signature_verifies() {
    let _ = load_fixture("nip98-tokens.json");
}
