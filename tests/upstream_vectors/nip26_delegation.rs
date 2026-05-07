// tests/upstream_vectors/nip26_delegation.rs
//! L1 reference vectors: NIP-26 delegation tokens.
//!
//! The canonical spec example pair (delegator/delegatee privkey + sig) is the
//! Schnorr ground truth for substrate-side NIP-26 verifier wiring (PRD-010 F8).

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn nip26_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("nip26-delegation.json");
    assert_meta_block(&f, "NIP-26");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(vectors.len() >= 5, "nip26-delegation fixture must have >= 5 vectors");
}

#[test]
fn nip26_canonical_delegation_string_format() {
    let f = load_fixture("nip26-delegation.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let Some(delegation_string) = v["delegation_string"].as_str() else {
            continue;
        };
        assert!(
            delegation_string.starts_with("nostr:delegation:"),
            "case '{}': delegation_string must start with 'nostr:delegation:', got: {}",
            v["case"].as_str().unwrap_or(""),
            delegation_string
        );
        // Format: nostr:delegation:<delegatee_pk_hex>:<conditions>
        let parts: Vec<&str> = delegation_string.splitn(4, ':').collect();
        assert_eq!(parts.len(), 4, "delegation_string must have 4 colon-separated parts");
        assert_eq!(parts[0], "nostr");
        assert_eq!(parts[1], "delegation");
        let pk = parts[2];
        assert_eq!(pk.len(), 64, "delegatee pubkey segment must be 64 hex chars");
        assert!(pk.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
#[ignore = "wires into PRD-010 F8 NIP-26 Schnorr verifier when forum kit is absorbed; canonical privkey/sig pair in fixture provides ground truth — Phase 2 deliverable"]
fn nip26_canonical_signature_verifies_under_delegator_pubkey() {
    let _ = load_fixture("nip26-delegation.json");
    // Phase 2: when nostr-core::nip26 is reachable, assert:
    //   nostr_core::nip26::verify_delegation_token(
    //       delegator_pubkey, delegatee_pubkey, conditions, token
    //   ).is_ok()
}
