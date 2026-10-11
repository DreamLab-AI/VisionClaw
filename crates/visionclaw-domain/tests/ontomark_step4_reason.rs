//! Migration test for ontomark removal step 4 (`ontomark/reason`), DEPRECATION.md section 9.
//!
//! Passes only when (a) the workspace depends on the named ontomark crate and
//! (b) the legacy VisionClaw rows for this step are gone. It reads files and
//! manifests as text and never imports ontomark, so it compiles and runs
//! without ontomark present. Red means an assertion failure.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(rel: &str) -> Option<String> {
    fs::read_to_string(root().join(rel)).ok()
}

/// Every workspace manifest (root and `crates/*`); `agentbox` is a submodule and is not scanned.
fn manifests() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(t) = read("Cargo.toml") {
        out.push(("Cargo.toml".to_string(), t));
    }
    if let Ok(rd) = fs::read_dir(root().join("crates")) {
        for e in rd.flatten() {
            let rel = format!("crates/{}/Cargo.toml", e.file_name().to_string_lossy());
            if let Some(t) = read(&rel) {
                out.push((rel, t));
            }
        }
    }
    out
}

/// True when any manifest declares `name` as a dependency (inline key or `[dependencies.name]` table).
fn depends_on(name: &str) -> bool {
    manifests().iter().any(|(_, t)| {
        t.lines().any(|l| {
            let l = l.trim();
            let table = l.starts_with('[') && l.ends_with(&format!(".{name}]"));
            let key = l.strip_prefix(name).is_some_and(|rest| {
                let rest = rest.trim_start();
                rest.starts_with('=') || rest.starts_with('.')
            });
            table || key
        })
    })
}

fn exists(rel: &str) -> bool {
    root().join(rel).exists()
}

/// Rust sources under `src/` and `crates/` (not `tests/`, not `target`, not vendored).
fn rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if name == "target" || name == "tests" || name == "node_modules" || name == ".git" {
                    continue;
                }
                walk(&p, out);
            } else if name.ends_with(".rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root().join("src"), &mut files);
    walk(&root().join("crates"), &mut files);
    // this migration test lives in a `tests/` directory, so it is already excluded
    files
        .into_iter()
        .filter_map(|p| {
            let rel = p.strip_prefix(root()).ok()?.to_string_lossy().to_string();
            Some((rel, fs::read_to_string(&p).ok()?))
        })
        .collect()
}

/// Files in `src/` and `crates/` whose text contains `needle`.
fn files_containing(needle: &str) -> Vec<String> {
    rust_sources()
        .into_iter()
        .filter(|(_, t)| t.contains(needle))
        .map(|(p, _)| p)
        .collect()
}

fn require_dep(failures: &mut Vec<String>, krate: &str, what: &str) {
    if !depends_on(krate) {
        failures.push(format!("{what}: no workspace manifest depends on `{krate}`"));
    }
}

fn require_absent_file(failures: &mut Vec<String>, rel: &str, row: &str) {
    if exists(rel) {
        failures.push(format!("{row}: `{rel}` must be deleted"));
    }
}

fn require_absent_text(failures: &mut Vec<String>, rel: &str, needle: &str, row: &str) {
    if read(rel).is_some_and(|t| t.contains(needle)) {
        failures.push(format!("{row}: `{rel}` must not contain `{needle}`"));
    }
}

fn require_unreferenced(failures: &mut Vec<String>, needle: &str, row: &str) {
    let hits = files_containing(needle);
    if !hits.is_empty() {
        failures.push(format!("{row}: `{needle}` still referenced in {}", hits.join(", ")));
    }
}

fn finish(step: &str, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "ontomark step {step} not landed ({} failing):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn step4_reason_consumers_on_ontomark_reason_and_legacy_gone() {
    let mut f = Vec::new();
    require_dep(&mut f, "ontomark-reason", "step 4 (R)");

    // VC-06 (checks)
    require_unreferenced(&mut f, "fn check_disjoint_pair", "VC-06 checks");
    require_unreferenced(&mut f, "fn whelk_inconsistent", "VC-06 checks");

    // VC-15, VC-16: Whelk wrapper and navigation closure in crates/vault
    require_absent_file(&mut f, "crates/vault/src/whelk.rs", "VC-15");
    require_absent_file(&mut f, "crates/vault/src/closure.rs", "VC-16");

    // VC-46: second Whelk adapter (the InferenceEngine port stays, repinned to ontomark-reason)
    require_absent_file(&mut f, "crates/visionclaw-adapters/src/whelk_inference_engine.rs", "VC-46");
    require_unreferenced(&mut f, "WhelkInferenceEngine", "VC-46 port repinned");

    // VC-47 (D5): hand-rolled closure
    require_absent_file(&mut f, "crates/visionclaw-ontology/src/reasoning/custom_reasoner.rs", "VC-47 (D5)");
    require_unreferenced(&mut f, "custom_reasoner", "VC-47 (D5)");
    require_unreferenced(&mut f, "CustomReasoner", "VC-47 (D5)");

    // VC-48 (D5): third closure store, test-only
    require_absent_file(&mut f, "src/services/ontology_reasoner.rs", "VC-48 (D5)");
    require_unreferenced(&mut f, "precompute_transitive_closure", "VC-48 (D5)");

    // VC-49 (D5): projection over ontomark-reason's inferred set, no local transitive reduction
    require_unreferenced(&mut f, "fn immediate_inferred_parents", "VC-49 (D5)");

    finish("4 (reason)", f);
}
