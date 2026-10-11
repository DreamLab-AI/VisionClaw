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

fn command_variants(main_rs: &str) -> Vec<String> {
    let start = main_rs.find("enum Command {").expect("enum Command in crates/vault/src/main.rs");
    let body = &main_rs[start..];
    let body = &body[..body.find("\n}").unwrap_or(body.len())];
    body.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.starts_with('#') && !l.is_empty())
        .filter_map(|l| l.split(['(', ',']).next())
        .filter(|w| w.chars().next().is_some_and(char::is_uppercase))
        .map(str::to_owned)
        .collect()
}

#[test]
fn step10_vault_is_a_governance_adapter_on_ontomark() {
    let mut out = vec![];
    require_dep(&mut out, "crates/vault/Cargo.toml", "ontomark-parse", "D4 adapter built on ontomark");
    require_dep(&mut out, "crates/vault/Cargo.toml", "ontomark-model", "D4 adapter built on ontomark");
    require_dep(&mut out, "crates/vault/Cargo.toml", "nostr-bbs-core", "VC-36 forum posting stays");
    if let Some(m) = read("crates/vault/Cargo.toml") {
        for dep in ["vault-core", "whelk", "horned-owl", "oxttl", "oxrdf"] {
            if declares_dep(&m, dep) {
                out.push(format!("D4: adapter must not carry legacy dependency `{dep}`"));
            }
        }
        if !m.contains("AGPL-3.0-only") {
            out.push("D4: the adapter stays AGPL-3.0-only".into());
        }
    }
    // Source shape: propose, apply, panel publish and the forum client only.
    let allowed = ["lib.rs", "main.rs", "nostr.rs", "propose.rs", "apply.rs"];
    if let Ok(rd) = fs::read_dir(root().join("crates/vault/src")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !allowed.contains(&n.as_str()) {
                out.push(format!("VC-23/24/32: crates/vault/src/{n} is outside the adapter shape {allowed:?}"));
            }
        }
    }
    if let Ok(rd) = fs::read_dir(root().join("crates/vault/tests")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !["apply_cli.rs", "propose_cli.rs", "panel_cli.rs"].contains(&n.as_str()) {
                out.push(format!("VC-32: crates/vault/tests/{n} tests a retired verb"));
            }
        }
    }
    if let Some(main) = read("crates/vault/src/main.rs") {
        let mut got = command_variants(&main);
        got.sort();
        if got != ["Apply", "Panel", "Propose"] {
            out.push(format!("D4: vault subcommands must be exactly Apply, Panel, Propose; found {got:?}"));
        }
    }
    finish(out);
}

#[test]
fn step10_vault_core_is_deleted_everywhere() {
    let mut out = vec![];
    forbid_file(&mut out, "crates/vault-core", "VC-01..13");
    for rel in ["Cargo.toml", ".github/workflows/ci.yml"] {
        forbid_text(&mut out, rel, "vault-core", "VC-01..13");
    }
    for dir in ["src", "crates", "tests"] {
        for p in rs_files(dir) {
            if fs::read_to_string(&p).is_ok_and(|t| t.contains("vault_core")) {
                out.push(format!("VC-01..13: `vault_core` still referenced in {}", p.display()));
            }
        }
    }
    finish(out);
}

#[test]
fn step10_publish_workflow_builds_with_the_released_ontomark_cli() {
    let mut out = vec![];
    let rel = ".github/workflows/ontology-publish.yml";
    let wf = read(rel).expect("ontology-publish.yml");
    if wf.contains("cargo build --release --locked -p vault") || wf.contains("-p vault") {
        out.push("VC-60: ontology-publish.yml still builds `cargo build -p vault`".into());
    }
    if wf.contains("target/release/vault") {
        out.push("VC-60: ontology-publish.yml still runs ./target/release/vault build".into());
    }
    if !wf.lines().any(|l| l.contains("ontomark") && l.contains("build")) {
        out.push("VC-60: ontology-publish.yml never runs `ontomark build`".into());
    }
    let pinned = wf.lines().any(|l| {
        let l = l.trim();
        l.starts_with("ONTOMARK_TAG:") && l.split(':').nth(1).is_some_and(|v| {
            let v = v.trim().trim_matches(['\'', '"']);
            v.starts_with('v') && v[1..].split('.').count() == 3 && v[1..].split('.').all(|p| p.parse::<u32>().is_ok())
        })
    });
    if !pinned {
        out.push("VC-60: ontology-publish.yml has no `ONTOMARK_TAG: vX.Y.Z` release pin".into());
    }
    finish(out);
}
