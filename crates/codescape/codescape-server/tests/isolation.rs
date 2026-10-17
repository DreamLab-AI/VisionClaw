//! Isolation from the knowledge-work system is a tested property (ADR-2143 §6).
//!
//! The Codescape build graph must never contain the ontology, memory-cloud,
//! Oxigraph, RuVector/Postgres, Solid pod or VisionClaw server crates. This test
//! runs `cargo metadata --locked` on the Codescape workspace and fails on any of
//! them. Allowed estate crates are `visionclaw-gpu` (PTX) and, transitively,
//! `visionclaw-domain` (types only).

use std::path::PathBuf;
use std::process::Command;

const FORBIDDEN: &[&str] = &[
    "visionclaw-server",
    "visionclaw-ontology",
    "visionclaw-memory-cloud",
    "visionclaw-adapters",
    "visionclaw-actors",
    "visionclaw-contracts",
    "vault",
    "vault-core",
    "whelk",
    "whelk-rs",
    "oxigraph",
    "horned-owl",
    "tokio-postgres",
    "postgres",
    "sqlx",
    "deadpool-postgres",
    "solid-pod-rs",
    "solid-pod-rs-core",
];

const FORBIDDEN_PREFIXES: &[&str] = &["ruvector", "solid-pod-rs"];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn package_names(manifest_dir: &std::path::Path, extra_args: &[&str]) -> Vec<String> {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--offline")
        .current_dir(manifest_dir);
    for a in extra_args {
        cmd.arg(a);
    }
    let out = cmd.output().expect("cargo metadata runs");
    if !out.status.success() {
        // Offline may fail on a cold cache; retry online once, but keep the
        // offline diagnostic visible so a network-dependent gate is noticed.
        eprintln!(
            "isolation: offline cargo metadata failed, retrying online:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut cmd = Command::new(env!("CARGO"));
        cmd.arg("metadata")
            .arg("--format-version")
            .arg("1")
            .current_dir(manifest_dir);
        for a in extra_args {
            cmd.arg(a);
        }
        let out = cmd.output().expect("cargo metadata runs");
        assert!(
            out.status.success(),
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return names_from(&out.stdout);
    }
    names_from(&out.stdout)
}

fn names_from(stdout: &[u8]) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_slice(stdout).expect("metadata is json");
    v["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect()
}

fn violations(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| {
            FORBIDDEN.contains(&n.as_str()) || FORBIDDEN_PREFIXES.iter().any(|p| n.starts_with(p))
        })
        .cloned()
        .collect()
}

#[test]
fn build_graph_contains_no_knowledge_work_crates() {
    let names = package_names(&workspace_root(), &["--locked"]);
    assert!(
        names.iter().any(|n| n == "codescape-server"),
        "metadata covered the Codescape workspace"
    );
    assert!(names.iter().any(|n| n == "graphicle"), "graphicle resolved");
    let bad = violations(&names);
    assert!(
        bad.is_empty(),
        "forbidden crates in the Codescape build graph: {bad:?}"
    );
}

#[test]
fn cuda_feature_graph_contains_no_knowledge_work_crates() {
    // Compose builds with `--features cuda`, which pulls visionclaw-gpu (PTX)
    // and, transitively, visionclaw-domain. Neither may drag the knowledge
    // stack in. `cargo metadata` resolves features without compiling, so this
    // runs on hosts without CUDA.
    let names = package_names(&workspace_root(), &["--locked", "--all-features"]);
    assert!(
        names.iter().any(|n| n == "visionclaw-gpu"),
        "cuda feature resolved visionclaw-gpu"
    );
    let bad = violations(&names);
    assert!(
        bad.is_empty(),
        "forbidden crates in the Codescape cuda build graph: {bad:?}"
    );
}

#[test]
fn the_check_itself_catches_an_injected_dependency() {
    // Build a throwaway workspace that depends on codescape-model AND on the
    // ontology crate by path, and prove the filter flags it. This guards the
    // test against silently passing because the ban list stopped matching.
    let tmp = tempfile::tempdir().unwrap();
    let root = workspace_root();
    let ontology = root.parent().unwrap().join("visionclaw-ontology");
    // The negative control must run: a missing fixture is a failure, not a
    // silent pass (the auditor's false-green path).
    assert!(
        ontology.join("Cargo.toml").exists(),
        "negative-control fixture missing: {} (Codescape lives inside the VisionClaw checkout)",
        ontology.display()
    );
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "injected"
version = "0.0.0"
edition = "2021"
[dependencies]
codescape-model = {{ path = "{}" }}
visionclaw-ontology = {{ path = "{}", default-features = false }}
[workspace]
"#,
            root.join("codescape-model").display(),
            ontology.display()
        ),
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "").unwrap();
    let names = package_names(tmp.path(), &[]);
    let bad = violations(&names);
    assert!(
        bad.iter().any(|n| n == "visionclaw-ontology"),
        "injected dependency must be flagged, got {bad:?}"
    );
}

#[test]
fn server_refuses_to_boot_with_a_ruvector_connection_string() {
    // The agentbox environment sets RUVECTOR_PG_CONNINFO for every process, so
    // the property under test is the binary's refusal, not the test's own env.
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_codescape-server"))
        .env(
            "RUVECTOR_PG_CONNINFO",
            "host=ruvector-postgres dbname=ruvector",
        )
        .env("CODESCAPE_DATA_DIR", tmp.path())
        .env("CODESCAPE_PORT", "0")
        .env("CODESCAPE_INGEST_PORT", "0")
        .arg("--repo")
        .arg(format!(
            "model={}",
            workspace_root().join("codescape-model").display()
        ))
        .output()
        .expect("binary runs");
    assert_eq!(
        out.status.code(),
        Some(2),
        "exit 2 expected; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("RUVECTOR_PG_CONNINFO"),
        "refusal must name the variable: {stderr}"
    );
}
