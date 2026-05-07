// tests/upstream_vectors/mod.rs
//! L1 reference-vector tests (per ADR-082 D6 + ADR-077 P1).
//!
//! Each test loads a fixture from `docs/specs/fixtures/<name>.json` and asserts
//! VisionClaw's substrate behaviour matches. Fixtures use the
//! `{"_meta": {...}, "vectors": [...]}` wrapper shape (or nested
//! `{"_meta": {...}, "vectors": {valid: ..., invalid: ...}}` for nip44-v2).
//!
//! Tests that depend on substrate features still pending are marked `#[ignore]`
//! with explanatory comments and stable issue references; the harness still
//! validates the fixture parses + has the expected vector count.

use std::fs;
use std::path::PathBuf;

/// Resolve a fixture file relative to the workspace root.
pub fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("docs");
    p.push("specs");
    p.push("fixtures");
    p.push(name);
    p
}

/// Load a fixture as parsed JSON. Panics with a clear message on miss/parse-fail.
pub fn load_fixture(name: &str) -> serde_json::Value {
    let path = fixture_path(name);
    let bytes = fs::read(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {}", path.display(), e));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("fixture {} is not valid JSON: {}", path.display(), e))
}

/// Assert the fixture has the expected wrapper shape.
pub fn assert_meta_block(fixture: &serde_json::Value, expected_spec_substring: &str) {
    let meta = fixture
        .get("_meta")
        .expect("fixture must have _meta block");
    let spec = meta
        .get("spec")
        .and_then(|v| v.as_str())
        .expect("_meta.spec must be a string");
    assert!(
        spec.contains(expected_spec_substring),
        "_meta.spec '{}' did not contain expected substring '{}'",
        spec,
        expected_spec_substring
    );
    assert!(
        meta.get("commit").is_some(),
        "_meta.commit must be present"
    );
}
