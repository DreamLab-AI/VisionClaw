//! Consumer-side migration test. It reads files and manifests as text and never
#![allow(dead_code)]
//! imports ontomark, so it compiles without ontomark present and goes red by
//! assertion failure only.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Option<String> {
    fs::read_to_string(root().join(rel)).ok()
}

fn exists(rel: &str) -> bool {
    root().join(rel).exists()
}

/// True when `manifest` declares dependency `name` (inline or table form),
/// ignoring comment lines.
fn declares_dep(manifest: &str, name: &str) -> bool {
    manifest.lines().any(|l| {
        let l = l.trim();
        if l.starts_with('#') {
            return false;
        }
        l.strip_prefix(name)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
            || l == format!("[dependencies.{name}]")
            || l == format!("[dev-dependencies.{name}]")
    })
}

fn require_dep(out: &mut Vec<String>, manifest_rel: &str, dep: &str, why: &str) {
    match read(manifest_rel) {
        Some(m) if declares_dep(&m, dep) => {}
        Some(_) => out.push(format!("{manifest_rel} does not depend on `{dep}` ({why})")),
        None => out.push(format!("{manifest_rel} is missing ({why})")),
    }
}

fn forbid_file(out: &mut Vec<String>, rel: &str, row: &str) {
    if exists(rel) {
        out.push(format!("{row}: legacy file still present: {rel}"));
    }
}

/// Flag `needle` appearing in `rel` (a missing file passes).
fn forbid_text(out: &mut Vec<String>, rel: &str, needle: &str, row: &str) {
    if read(rel).is_some_and(|t| t.contains(needle)) {
        out.push(format!("{row}: legacy symbol `{needle}` still in {rel}"));
    }
}

/// Recursively collect `.rs` files under `rel`.
fn rs_files(rel: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, acc: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, acc);
            } else if p.extension().is_some_and(|x| x == "rs") {
                acc.push(p);
            }
        }
    }
    let mut acc = Vec::new();
    walk(&root().join(rel), &mut acc);
    acc
}

fn finish(out: Vec<String>) {
    assert!(out.is_empty(), "\n  {}\n", out.join("\n  "));
}

#[test]
fn step6_consumers_depend_on_ontomark_compile() {
    let mut out = vec![];
    require_dep(&mut out, "crates/vault/Cargo.toml", "ontomark-compile", "VC-19/20/22/28 RDF compile");
    require_dep(&mut out, "crates/visionclaw-ontology/Cargo.toml", "ontomark-compile", "VC-50/52 open-world reader and SHACL");
    finish(out);
}

#[test]
fn step6_legacy_rdf_code_is_gone() {
    let mut out = vec![];
    forbid_file(&mut out, "crates/vault/src/build/turtle.rs", "VC-19");
    forbid_file(&mut out, "crates/vault/src/build/webvowl.rs", "VC-20");
    for f in ["indexes.rs", "page_api.rs", "okf.rs"] {
        forbid_file(&mut out, &format!("crates/vault/src/build/{f}"), "VC-22");
    }
    forbid_file(&mut out, "crates/vault/src/projection.rs", "VC-28");
    forbid_text(&mut out, "crates/visionclaw-ontology/src/open_world.rs", "fn turtle_statements", "VC-50");
    forbid_text(&mut out, "crates/visionclaw-ontology/src/open_world.rs", "fn version_iri_from_turtle", "VC-50");
    for f in ["shacl_lite.rs", "owl_el_profile.rs", "class_bit.rs", "frame.rs", "shacl.rs", "mod.rs"] {
        forbid_file(&mut out, &format!("crates/visionclaw-ontology/src/services/jsonld_validator/{f}"), "VC-52");
    }
    forbid_file(&mut out, "crates/visionclaw-ontology/src/inference/owl_parser.rs", "VC-53");
    finish(out);
}
