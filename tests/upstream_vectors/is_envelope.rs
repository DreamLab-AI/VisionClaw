// tests/upstream_vectors/is_envelope.rs
//! L1 reference vectors: DreamLab IS-Envelope v1 (ADR-075 D1+D3).

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn is_envelope_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("is-envelope-v1.json");
    assert_meta_block(&f, "ADR-075");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(vectors.len() >= 11, "is-envelope-v1 must have >= 11 vectors");
}

#[test]
fn is_envelope_required_fields_present_in_valid_cases() {
    let f = load_fixture("is-envelope-v1.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let env = &v["envelope"];
        assert_eq!(env["v"].as_i64(), Some(1), "v must be 1");
        assert!(env["to"].is_string(), "to required");
        assert!(env["from"].is_string(), "from required");
        let kind = env["kind"].as_str().expect("kind required");
        let allowed = [
            "chat",
            "tool_invoke",
            "tool_result",
            "knowledge_link",
            "moderation",
            "mesh_ping",
        ];
        assert!(
            allowed.contains(&kind),
            "case '{}': kind '{}' not in canonical enumeration",
            v["case"].as_str().unwrap_or(""),
            kind
        );
        assert!(env.get("body").is_some(), "body required");
    }
}

#[test]
fn is_envelope_negative_cases_have_violation() {
    let f = load_fixture("is-envelope-v1.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() != Some(false) {
            continue;
        }
        assert!(
            v["violation"].is_string(),
            "case '{}' MUST cite an ADR-075 D-rule violation",
            v["case"].as_str().unwrap_or("")
        );
    }
}

#[test]
#[ignore = "JCS canonicaliser is not currently wired into VisionClaw; expected_jcs assertion deferred to Phase 2 alongside rfc8785-jcs wiring"]
fn is_envelope_jcs_canonicalisation_matches_expected() {
    let _ = load_fixture("is-envelope-v1.json");
}
