//! Migration test for ontomark removal step 1 (`ontomark/parse`), DEPRECATION.md section 9.
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
fn step1_parse_consumers_on_ontomark_parse_and_legacy_gone() {
    let mut f = Vec::new();
    require_dep(&mut f, "ontomark-parse", "step 1 (P)");

    // VC-01, 02, 07, 08, 13: vault-core parse modules
    for (row, file) in [
        ("VC-01", "page.rs"),
        ("VC-02", "frontmatter.rs"),
        ("VC-07", "definition.rs"),
        ("VC-08", "graph.rs"),
        ("VC-13", "code.rs"),
        ("VC-13", "json.rs"),
        ("VC-13", "error.rs"),
    ] {
        require_absent_file(&mut f, &format!("crates/vault-core/src/{file}"), row);
    }
    // no remaining consumer of the deleted modules (root crate, vault, adapters)
    for m in ["vault_core::page", "vault_core::frontmatter", "vault_core::definition", "vault_core::graph"] {
        require_unreferenced(&mut f, m, "VC-01/02/07/08 consumers repinned");
    }

    // VC-40: second parser and writer in visionclaw-domain, callers repinned
    require_absent_file(&mut f, "crates/visionclaw-domain/src/vault/mod.rs", "VC-40");
    require_unreferenced(&mut f, "visionclaw_domain::vault", "VC-40 callers repinned");

    // VC-41: hand-rolled key: value reader and its (test-only) service
    require_absent_file(&mut f, "src/services/ontology_enrichment_service.rs", "VC-41");

    // VC-42: wrapper plus duplicate namespace constants
    require_absent_text(&mut f, "src/services/page_parser.rs", "pub const CLASS_NAMESPACE", "VC-42");
    require_absent_text(&mut f, "src/services/page_parser.rs", "pub const INDIVIDUAL_NAMESPACE", "VC-42");
    require_absent_text(&mut f, "src/services/page_parser.rs", "vault_core::", "VC-42");

    // VC-57: graph nodes built from ontomark-model types, not visionclaw_domain::vault::PageMeta
    require_absent_text(&mut f, "src/services/parsers/knowledge_graph_parser.rs", "visionclaw_domain::vault", "VC-57");

    // VC-58: wire type no longer carries jsonld_blocks
    require_absent_text(&mut f, "crates/visionclaw-contracts/src/github_adapter.rs", "jsonld_blocks", "VC-58");

    finish("1 (parse)", f);
}
