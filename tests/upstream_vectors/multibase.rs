// tests/upstream_vectors/multibase.rs
//! L1 reference vectors: Multibase self-describing base encoding.
//!
//! Used by ADR-074 D3 for `publicKeyMultibase` (z + base58btc(0xe7 0x01 || pk_32)).

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn multibase_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("multibase.json");
    assert_meta_block(&f, "Multibase");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(vectors.len() >= 27, "multibase fixture must have >= 27 vectors");
}

#[test]
fn multibase_encoded_strings_have_known_prefix() {
    // Each multibase-encoded string starts with a 1-char base prefix.
    // Build a quick allowlist from known prefixes used in the fixture.
    let f = load_fixture("multibase.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let encoded = v["encoded"].as_str().unwrap();
        assert!(
            !encoded.is_empty(),
            "case '{}': encoded must be non-empty for valid vector",
            v["case"].as_str().unwrap_or("")
        );
        let prefix = encoded.chars().next().unwrap();
        // Known prefixes: base2='0', base8='7', base10='9', base16='f'/'F',
        // base32='b'/'B'/'c'/'C'/'v'/'V'/'t'/'T'/'h', base36='k'/'K',
        // base58btc='z', base58flickr='Z', base64='m'/'M', base64url='u'/'U',
        // base256emoji='🚀'.
        let known: &str = "0789fFbBcCvVtThkKzZmMuU🚀";
        assert!(
            known.contains(prefix),
            "case '{}': prefix '{}' is not a known multibase code",
            v["case"].as_str().unwrap_or(""),
            prefix
        );
    }
}

#[test]
#[ignore = "requires `multibase` crate dep + base58btc decoder; substrate uses it indirectly via DID Document publicKeyMultibase — wire decoder round-trip in Phase 2"]
fn multibase_canonical_round_trip() {
    let _ = load_fixture("multibase.json");
}
