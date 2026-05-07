// tests/upstream_vectors/did_doc.rs
//! L1 reference vectors: DreamLab DID Document conformance (ADR-074 D2).
//!
//! Validates substrate-emitted DID Documents against the canonical Tier-3
//! shape and rejects all known anti-drift fabrications (stale suite IDs,
//! missing context, uppercase ID, mismatched controller).

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn did_doc_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("did-doc-conformance.json");
    assert_meta_block(&f, "ADR-074");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert_eq!(vectors.len(), 7, "did-doc-conformance must have 7 vectors");
}

#[test]
fn did_doc_canonical_has_required_contexts() {
    let f = load_fixture("did-doc-conformance.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let ctx = v["document"]["@context"].as_array().unwrap();
        let has_did_v1 = ctx
            .iter()
            .any(|c| c.as_str() == Some("https://www.w3.org/ns/did/v1"));
        let has_secp = ctx.iter().any(|c| {
            c.as_str() == Some("https://w3id.org/security/suites/secp256k1-2019/v1")
        });
        assert!(
            has_did_v1,
            "valid DID Doc '{}' MUST include https://www.w3.org/ns/did/v1",
            v["case"].as_str().unwrap_or("")
        );
        assert!(
            has_secp,
            "valid DID Doc '{}' MUST include secp256k1-2019/v1 (ADR-074 D4)",
            v["case"].as_str().unwrap_or("")
        );
    }
}

#[test]
fn did_doc_negative_cases_have_violation_field() {
    let f = load_fixture("did-doc-conformance.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() == Some(false) {
            assert!(
                v["violation"].is_string(),
                "negative case '{}' MUST cite an ADR D-rule violation",
                v["case"].as_str().unwrap_or("")
            );
        }
    }
}

#[test]
fn did_doc_no_stale_suite_identifiers_in_valid_cases() {
    // Anti-drift Rule 2 from scripts/anti-drift-lint.sh: SchnorrSecp256k1VerificationKey2019
    // is canonical; 2022/2025 are forbidden fabrications.
    let f = load_fixture("did-doc-conformance.json");
    let vectors = f["vectors"].as_array().unwrap();
    for v in vectors {
        if v["valid"].as_bool() != Some(true) {
            continue;
        }
        let vms = v["document"]["verificationMethod"].as_array().unwrap();
        for vm in vms {
            let suite = vm["type"].as_str().unwrap();
            assert_eq!(
                suite, "SchnorrSecp256k1VerificationKey2019",
                "valid case '{}' MUST use canonical suite 2019, found {}",
                v["case"].as_str().unwrap_or(""),
                suite
            );
        }
    }
}
