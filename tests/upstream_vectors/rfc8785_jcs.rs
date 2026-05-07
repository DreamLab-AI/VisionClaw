// tests/upstream_vectors/rfc8785_jcs.rs
//! L1 reference vectors: RFC 8785 JSON Canonicalization Scheme (JCS).
//!
//! Used for IS-Envelope canonical serialisation per ADR-075 D5.

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn rfc8785_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("rfc8785-jcs.json");
    assert_meta_block(&f, "RFC 8785");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(vectors.len() >= 6, "rfc8785-jcs fixture must have >= 6 vectors");
}

#[test]
fn rfc8785_each_vector_has_input_and_expected_output() {
    let f = load_fixture("rfc8785-jcs.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        assert!(v["name"].is_string(), "name required");
        assert!(v["input"].is_object() || v["input"].is_array(), "input required");
        assert!(
            v["expected_output"].is_string(),
            "expected_output (canonical UTF-8 string) required"
        );
    }
}

#[test]
#[ignore = "JCS canonicaliser is not currently a direct VisionClaw substrate dep; ADR-075 mandates it for IS-Envelope signing — wire via the cyberphone/json-canonicalization Rust port in Phase 2"]
fn rfc8785_canonicaliser_matches_reference() {
    let _ = load_fixture("rfc8785-jcs.json");
}
