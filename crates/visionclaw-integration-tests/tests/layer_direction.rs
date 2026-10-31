//! ADR-2005 — machine check of crate dependency direction.
//!
//! The compiler already rejects a dependency cycle, but nothing stopped a
//! lower layer from quietly gaining an edge onto a higher one (an adapter
//! importing the actor crate, say) as long as it stayed acyclic. This test
//! reads `cargo metadata` for the workspace and asserts that every
//! workspace-internal edge is on the allow-list below, that nothing depends
//! on the root binary, and that every member is classified, so a new crate
//! must declare its layer here before CI goes green.
//!
//! Hermetic: it needs `cargo` and the manifests only (`--no-deps --offline`).

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

/// The root binary crate. It is the composition point and may depend on any
/// member; no member may depend on it.
const ROOT: &str = "visionclaw-server";

/// Allowed workspace-internal dependencies, per member (normal, build and dev
/// edges alike). An empty slice marks a leaf. Keep this in step with
/// `docs/BASELINE-architecture.md` ("Crate layout") and ADR-2005.
const ALLOWED: &[(&str, &[&str])] = &[
    // Leaves: independently buildable, no workspace dependency.
    ("visionclaw-contracts", &[]),
    ("visionclaw-domain", &[]),
    ("visionclaw-wire", &[]),
    ("visionclaw-tri-layout", &[]),
    ("visionclaw-memory-cloud", &[]),
    ("visionclaw-analytics-oracle", &[]),
    ("visionclaw-xr-presence", &[]),
    ("visionclaw-jev", &[]),
    ("vault-core", &[]),
    ("visionclaw-integration-tests", &[]),
    // Layers above the domain.
    ("visionclaw-adapters", &["visionclaw-domain"]),
    ("visionclaw-ontology", &["visionclaw-domain"]),
    ("visionclaw-actors", &["visionclaw-domain"]),
    (
        "visionclaw-protocol",
        &["visionclaw-domain", "visionclaw-wire"],
    ),
    (
        "visionclaw-gpu",
        &["visionclaw-domain", "visionclaw-analytics-oracle"],
    ),
    ("vault", &["vault-core", "visionclaw-jev"]),
];

/// `member -> workspace-internal dependencies`, from `cargo metadata` JSON.
fn workspace_edges(metadata: &Value) -> BTreeMap<String, BTreeSet<String>> {
    let packages = metadata["packages"].as_array().expect("packages array");
    let members: BTreeSet<&str> = packages
        .iter()
        .map(|p| p["name"].as_str().expect("package name"))
        .collect();
    packages
        .iter()
        .map(|p| {
            let name = p["name"].as_str().expect("package name").to_string();
            let deps = p["dependencies"]
                .as_array()
                .expect("dependencies array")
                .iter()
                .filter_map(|d| d["name"].as_str())
                .filter(|d| members.contains(d))
                .map(str::to_string)
                .collect();
            (name, deps)
        })
        .collect()
}

/// Every way `edges` departs from [`ALLOWED`]; empty means the graph is clean.
fn violations(edges: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let allowed: BTreeMap<&str, BTreeSet<&str>> = ALLOWED
        .iter()
        .map(|(n, d)| (*n, d.iter().copied().collect()))
        .collect();
    let mut out = Vec::new();
    for (name, deps) in edges {
        if deps.contains(ROOT) {
            out.push(format!("{name} depends on the root binary crate {ROOT}"));
        }
        if name == ROOT {
            continue;
        }
        let Some(ok) = allowed.get(name.as_str()) else {
            out.push(format!(
                "{name} is a workspace member with no declared layer; add it to ALLOWED"
            ));
            continue;
        };
        for dep in deps {
            if dep != ROOT && !ok.contains(dep.as_str()) {
                out.push(format!("{name} -> {dep} is not an allowed layer edge"));
            }
        }
    }
    for name in allowed.keys() {
        if !edges.contains_key(*name) {
            out.push(format!(
                "ALLOWED lists {name}, which is not a workspace member"
            ));
        }
    }
    out
}

fn load_metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("cargo metadata JSON")
}

#[test]
fn workspace_dependency_direction_is_clean() {
    let edges = workspace_edges(&load_metadata());
    let bad = violations(&edges);
    assert!(
        bad.is_empty(),
        "layer direction violated:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn checker_flags_an_upward_edge_a_stale_row_a_new_member_and_a_root_import() {
    let mut edges: BTreeMap<String, BTreeSet<String>> = ALLOWED
        .iter()
        .map(|(n, d)| (n.to_string(), d.iter().map(|s| s.to_string()).collect()))
        .collect();
    edges.insert(ROOT.to_string(), BTreeSet::new());
    assert!(violations(&edges).is_empty(), "baseline must be clean");

    let mut upward = edges.clone();
    upward
        .get_mut("visionclaw-adapters")
        .unwrap()
        .insert("visionclaw-actors".to_string());
    assert_eq!(violations(&upward).len(), 1);

    let mut rooted = edges.clone();
    rooted
        .get_mut("visionclaw-domain")
        .unwrap()
        .insert(ROOT.to_string());
    assert_eq!(violations(&rooted).len(), 1);

    // A deleted or renamed crate whose ALLOWED row was left behind.
    let mut stale = edges.clone();
    stale.remove("visionclaw-xr-presence");
    assert_eq!(violations(&stale).len(), 1);

    let mut unclassified = edges;
    unclassified.insert("visionclaw-new".to_string(), BTreeSet::new());
    assert_eq!(violations(&unclassified).len(), 1);
}
