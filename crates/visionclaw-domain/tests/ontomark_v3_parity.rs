//! The domain parser (`vault::parse`, which feeds the sync and the KG gate)
//! gives the same publication answer as ontomark for every page of the shared
//! fixture vault, and reports the same unreadable stamps.
//!
//! The fixture and `golden.json` live with `vault-core`
//! (`crates/vault-core/tests/fixtures/ontomark-vocab-v3`, see its README):
//! ontomark's own vocabulary-3 fixture plus VisionClaw's additions, and
//! ontomark's answers for each page.

use std::path::{Path, PathBuf};

use serde_json::Value;
use visionclaw_domain::vault::{parse, Rules};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../vault-core/tests/fixtures/ontomark-vocab-v3")
}

fn pages() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["knowledge", "working"] {
        for entry in std::fs::read_dir(fixture().join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "md") {
                let id = format!("{dir}/{}", path.file_stem().unwrap().to_string_lossy());
                out.push((id, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn golden() -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture().join("golden.json")).unwrap()).unwrap()
}

/// `(page, vocabulary version, reason)` where VisionClaw reads a page as NOT
/// public although ontomark reads it as public. The reverse is never allowed.
const FAIL_CLOSED: &[(&str, u32)] = &[("working/VcConflictPublic", 3)];

#[test]
fn the_kg_gate_and_the_publisher_predicate_match_ontomark() {
    let g = golden();
    let mut public = 0;
    for (id, text) in pages() {
        for version in [2u32, 3] {
            let meta = parse(&text, Rules::for_path(&id, Some(version)));
            let want = g["pages"][&id][format!("v{version}")]["public"]
                .as_bool()
                .unwrap();
            // None of the fixture's pages carries an `owl-class`, so the gate
            // is the publication answer; both predicates read the one function.
            assert_eq!(
                meta.is_publishable(),
                meta.is_kg_included(),
                "{id} v{version}"
            );
            let ours = meta.is_kg_included();
            assert!(
                !ours || want,
                "{id} under vocabulary {version}: VisionClaw publishes a page ontomark keeps private"
            );
            if ours != want {
                assert!(
                    FAIL_CLOSED.contains(&(id.as_str(), version)),
                    "{id} under vocabulary {version}: ontomark {want}, VisionClaw {ours}"
                );
            }
            public += usize::from(ours);
        }
    }
    assert!(
        public >= 16,
        "the fixture must exercise the public path, got {public}"
    );
}

#[test]
fn unreadable_stamps_are_reported_not_discarded() {
    let g = golden();
    for (id, text) in pages() {
        let meta = parse(&text, Rules::for_path(&id, Some(3)));
        let want = &g["pages"][&id]["v3"];
        let raw = meta.raw.clone().unwrap_or_default();
        let has = |k: &str| raw.contains_key(serde_yaml::Value::String(k.into()));
        let verified_raw = match raw.get(serde_yaml::Value::String("verified".into())) {
            Some(serde_yaml::Value::Sequence(s)) => s.len(),
            Some(_) => 1,
            None => 0,
        };
        let expected =
            usize::from(has("generated") && !want["generated_readable"].as_bool().unwrap())
                + (verified_raw - want["verified_readable"].as_u64().unwrap() as usize);
        assert_eq!(
            meta.stamp_problems.len(),
            expected,
            "{id}: {:?}",
            meta.stamp_problems
        );
    }
}
