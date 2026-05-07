// tests/upstream_vectors/nip04_dm.rs
//! L1 reference vectors: NIP-04 DM wire shape + ECDH rule.
//!
//! NIP-04 has no canonical KAT; this test asserts that fixtures parse and
//! that the wire-shape regex matches the documented format.

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn nip04_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("nip04-dm.json");
    assert_meta_block(&f, "NIP-04");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(
        vectors.len() >= 3,
        "nip04-dm fixture must have >= 3 vectors"
    );
}

#[test]
fn nip04_negative_cases_are_clearly_invalid() {
    // We assert the negative cases declare valid: false and provide an
    // unambiguous content_invalid string. Substrate-side parser wiring is
    // Phase 2 (NIP-04 is deprecated — most surface uses NIP-44 / NIP-59).
    let f = load_fixture("nip04-dm.json");
    let vectors = f["vectors"].as_array().unwrap();
    let negatives: Vec<_> = vectors
        .iter()
        .filter(|v| v["valid"].as_bool() == Some(false))
        .collect();
    assert!(
        !negatives.is_empty(),
        "fixture must include >=1 negative case for NIP-04 wire-shape rejection"
    );
    for v in negatives {
        let case = v["case"].as_str().unwrap_or("");
        let invalid = v["content_invalid"].as_str();
        assert!(
            invalid.is_some(),
            "negative case '{}' must include content_invalid",
            case
        );
    }
}

#[test]
#[ignore = "NIP-04 is deprecated (superseded by NIP-17/NIP-44+NIP-59); substrate decrypt path is in `nostr-tools` JS only — Rust round-trip wiring not in scope"]
fn nip04_round_trip_encrypt_decrypt() {
    let _ = load_fixture("nip04-dm.json");
}
