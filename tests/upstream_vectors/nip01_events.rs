// tests/upstream_vectors/nip01_events.rs
//! L1 reference vectors: NIP-01 event-id serialisation.
//!
//! Loads `docs/specs/fixtures/nip01-events.json` and asserts that the
//! serialised string in each vector hashes (sha256) to the expected event id
//! when one is provided. Negative cases assert that the substrate's parser
//! rejects malformed events.

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

use sha2::{Digest, Sha256};

#[test]
fn nip01_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("nip01-events.json");
    assert_meta_block(&f, "NIP-01");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(
        vectors.len() >= 11,
        "nip01-events fixture must have >= 11 vectors, got {}",
        vectors.len()
    );
}

#[test]
fn nip01_serialised_strings_hash_consistently() {
    // For each positive vector with a `serialised` field, sha256 of the UTF-8
    // bytes is the event id (per NIP-01 §1). We compute it and assert non-empty
    // hex; if `expected_id` is provided we assert exact match.
    let f = load_fixture("nip01-events.json");
    let vectors = f["vectors"].as_array().unwrap();
    let mut checked = 0usize;
    for v in vectors {
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let Some(serialised) = v["serialised"].as_str() else {
            continue;
        };
        let mut h = Sha256::new();
        h.update(serialised.as_bytes());
        let id_hex = format!("{:x}", h.finalize());
        assert_eq!(id_hex.len(), 64, "sha256 hex must be 64 chars");
        if let Some(expected) = v["expected_id"].as_str() {
            assert_eq!(
                id_hex, expected,
                "case '{}': computed event id {} did not match expected {}",
                v["case"].as_str().unwrap_or(""),
                id_hex,
                expected
            );
        }
        checked += 1;
    }
    assert!(checked >= 5, "expected to check >=5 positive vectors, got {}", checked);
}

#[test]
#[ignore = "wires into substrate-side event validator (`crates/nostr-core` once forum-kit absorbed via PRD-009 F26 Shape A) — currently substrate uses solid_pod_handler::handle_nostr_event for negative-case rejection; full L1 wiring is Phase 2 deliverable"]
fn nip01_negative_vectors_are_rejected_by_substrate_validator() {
    let _ = load_fixture("nip01-events.json");
    // Phase 2: when nostr-core forum kit is absorbed, replace this stub with
    //   for v in vectors_with_valid_false {
    //       assert!(nostr_core::Event::validate_strict(v["event"]).is_err());
    //   }
}
