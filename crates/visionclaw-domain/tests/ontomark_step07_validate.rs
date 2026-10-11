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
fn step7_consumers_depend_on_ontomark_validation() {
    let mut out = vec![];
    require_dep(&mut out, "crates/vault/Cargo.toml", "ontomark-compile", "VC-25/26/27 validation, conflicts, gate");
    require_dep(&mut out, "crates/visionclaw-ontology/Cargo.toml", "ontomark-compile", "VC-51 validation (never infers, ADR-2126/2128)");
    finish(out);
}

#[test]
fn step7_legacy_validation_code_is_gone() {
    let mut out = vec![];
    forbid_file(&mut out, "crates/vault-core/src/okf.rs", "VC-09");
    forbid_file(&mut out, "crates/vault-core/src/promotion.rs", "VC-10");
    forbid_file(&mut out, "crates/vault-core/src/secrets.rs", "VC-12");
    forbid_file(&mut out, "crates/vault/src/validate.rs", "VC-25");
    forbid_file(&mut out, "crates/vault/src/conflicts.rs", "VC-26");
    forbid_file(&mut out, "crates/vault/src/gate.rs", "VC-27");
    forbid_file(&mut out, "crates/visionclaw-ontology/src/services/owl_validator.rs", "VC-51");
    for p in rs_files("crates/visionclaw-ontology/src") {
        if fs::read_to_string(&p).is_ok_and(|t| t.contains("fn validate_domain_range")) {
            out.push(format!("VC-51: domain/range enforcement `validate_domain_range` still in {}", p.display()));
        }
    }
    finish(out);
}
