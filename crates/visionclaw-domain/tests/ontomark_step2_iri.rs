//! Migration test for ontomark removal step 2 (`ontomark/iri`), DEPRECATION.md section 9.
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
fn step2_iri_consumers_on_ontomark_model_and_legacy_gone() {
    let mut f = Vec::new();
    require_dep(&mut f, "ontomark-model", "step 2 (I)");

    // VC-04: the canonical slug rule and IRI minting
    require_absent_file(&mut f, "crates/vault-core/src/slug.rs", "VC-04");
    require_unreferenced(&mut f, "vault_core::slug", "VC-04 consumers repinned");

    // VC-54: every slug rule but ontomark-model's is deleted
    for (file, needle) in [
        ("src/uri/mod.rs", "pub fn slugify"),
        ("crates/visionclaw-ontology/src/services/canonical_iri.rs", "pub fn slugify"),
        ("crates/visionclaw-ontology/src/services/jsonld_ingest/expander.rs", "pub fn slugify"),
        ("src/services/parsers/knowledge_graph_parser.rs", "pub fn slugify"),
        ("crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs", "fn slug(s: &str)"),
        ("client/src/features/graph/utils/pageLinks.ts", "export function slugifyLabel"),
    ] {
        require_absent_text(&mut f, file, needle, "VC-54");
    }

    // VC-55: namespace and mint constants come from the profile, not local literals
    require_absent_text(&mut f, "crates/visionclaw-domain/src/uri.rs", "\"urn:ngm:class:\"", "VC-55");
    require_absent_text(&mut f, "src/uri/mod.rs", "LEGACY_NGM_NS", "VC-55");
    require_absent_text(&mut f, "crates/visionclaw-adapters/src/rdf_serializer.rs", "\"https://narrativegoldmine.com/ns/v1#\"", "VC-55");
    require_absent_text(&mut f, "crates/visionclaw-adapters/src/provenance_emitter.rs", "\"https://narrativegoldmine.com/ns/v1#\"", "VC-55");
    require_absent_text(&mut f, "crates/visionclaw-adapters/src/sparql_migrations.rs", "\"urn:ngm:graph:migrations\"", "VC-55");
    require_absent_text(&mut f, "client/src/services/solidPod/typeIndex.ts", "narrativegoldmine.com/ontology#", "VC-55 drift (differs from VC_NS .../ns/v1#)");
    require_absent_text(&mut f, "client/src/features/ontology/services/inferredAxiomsService.ts", "'urn:ngm:graph:ontology:inferred'", "VC-55");
    require_absent_text(&mut f, "scripts/ontology/pack-pod-resources.py", "narrativegoldmine.com/ns/v1#", "VC-55");
    // PROV namespace constant: defined locally in five files today (the row says four);
    // none may remain, the value comes from ontomark-model's namespaces.
    let prov: Vec<String> = rust_sources()
        .into_iter()
        .filter(|(_, t)| {
            t.lines().any(|l| {
                let l = l.trim();
                l.contains("const PROV") && l.contains("&str = \"http://www.w3.org/ns/prov#\";")
            })
        })
        .map(|(p, _)| p)
        .collect();
    if !prov.is_empty() {
        f.push(format!("VC-55: PROV namespace constant still defined in {} files: {}", prov.len(), prov.join(", ")));
    }

    finish("2 (iri)", f);
}
