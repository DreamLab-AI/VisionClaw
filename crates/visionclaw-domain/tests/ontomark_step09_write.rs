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
fn step9_consumers_depend_on_ontomark_parse() {
    let mut out = vec![];
    require_dep(&mut out, "crates/vault/Cargo.toml", "ontomark-parse", "VC-29/30 guarded patch");
    require_dep(&mut out, "Cargo.toml", "ontomark-parse", "VC-43/44/45 page generation and patch");
    for rel in ["crates/vault/src/propose.rs", "crates/vault/src/apply.rs"] {
        match read(rel) {
            Some(t) if t.contains("ontomark_") => {}
            Some(_) => out.push(format!("{rel} does not use any ontomark_* crate (VC-30/D4 adapter re-wiring)")),
            None => out.push(format!("{rel} is missing: D4 keeps the propose and apply verbs in the adapter")),
        }
    }
    finish(out);
}

#[test]
fn step9_legacy_write_code_is_gone() {
    let mut out = vec![];
    for (f, row) in [("edit.rs", "VC-29"), ("create.rs", "VC-29"), ("diff.rs", "VC-29"), ("repair.rs", "VC-31"), ("bodies.rs", "VC-31")] {
        forbid_file(&mut out, &format!("crates/vault/src/{f}"), row);
    }
    // apply.rs survives only as the D4 adapter verb: its own patch engine must go.
    for needle in ["use crate::diff", "use crate::edit", "fn apply_changes"] {
        forbid_text(&mut out, "crates/vault/src/apply.rs", needle, "VC-29");
    }
    forbid_text(&mut out, "crates/vault/src/propose.rs", "vault_core::proposal", "VC-30");
    // VC-36: the AGPL forum code stays.
    if !exists("crates/vault/src/nostr.rs") {
        out.push("VC-36: crates/vault/src/nostr.rs must be retained (AGPL)".into());
    }
    forbid_text(&mut out, "src/services/ontology_mutation_service.rs", "fn generate_vault_markdown", "VC-43");
    forbid_text(&mut out, "src/services/ontology_mutation_service.rs", "fn check_consistency", "VC-43");
    forbid_text(&mut out, "src/services/ontology_mutation_service.rs", "\"ai\" => \"AI\"", "VC-43");
    forbid_text(&mut out, "src/services/decision_elevation.rs", "fn slugify", "VC-44");
    forbid_text(&mut out, "src/actors/elevation_actor.rs", "fn slugify", "VC-45");
    finish(out);
}
