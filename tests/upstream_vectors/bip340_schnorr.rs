// tests/upstream_vectors/bip340_schnorr.rs
//! L1 reference vectors: BIP-340 Schnorr signatures (secp256k1).
//!
//! C2 regression guard. Substrates rely on Schnorr/BIP-340 for all event
//! signature verification (NIP-01 sig field, NIP-26 delegation tokens,
//! NIP-98 auth tokens).

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn bip340_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("bip340-schnorr.json");
    assert_meta_block(&f, "BIP-340");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert_eq!(vectors.len(), 19, "bip340 fixture must have exactly 19 reference vectors");
}

#[test]
fn bip340_each_vector_has_deterministic_fields() {
    let f = load_fixture("bip340-schnorr.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        let pk = v["public_key_hex"].as_str().expect("public_key_hex required");
        assert_eq!(pk.len(), 64, "public_key_hex must be 64 hex chars (32-byte x-only)");
        let sig = v["signature_hex"].as_str().expect("signature_hex required");
        assert_eq!(sig.len(), 128, "signature_hex must be 128 hex chars (64-byte BIP-340 sig)");
        assert!(
            v["verification_result"].is_boolean(),
            "verification_result must be a boolean for vector index {}",
            v["index"]
        );
    }
}

#[test]
#[ignore = "requires secp256k1 BIP-340 verifier in scope at the test boundary; substrate verifier is in `nostr-sdk` (transitive dep) but not directly re-exported — wire via `secp256k1::Secp256k1::verification_only()` in Phase 2"]
fn bip340_canonical_vectors_verify() {
    let _ = load_fixture("bip340-schnorr.json");
    // Phase 2: import secp256k1 + use bip340::verify_schnorr_signature(pk, msg, sig)
    // for each vector and assert verification_result matches.
}
