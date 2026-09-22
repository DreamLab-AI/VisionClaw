//! `vault propose` → `vault create` through the real binary: the interface the
//! agentbox elevation path calls, `vault --repo <root> create <file>
//! --expect docs=1 --json`.

use std::path::Path;
use std::process::{Command, Output};

const VOCABULARY: &str = r#"version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
"#;

/// A throwaway secp256k1 secret with no standing anywhere.
const SECRET: &str = "0101010101010101010101010101010101010101010101010101010101010101";

const EXISTING: &str = "---\ntype: Class\ntitle: Alpha\npublic: true\nresource: urn:ngm:class:alpha\nstatus: stable\n---\nbody\n";
const STAGED: &str = "---\ntype: Class\ntitle: New Thing\npublic: true\nresource: urn:ngm:class:new-thing\nstatus: draft\nis-a:\n- '[[Alpha]]'\n---\nElevated from a working note.\n";

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn vault(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vault"))
        .arg("--repo")
        .arg(repo)
        .arg("--json")
        .args(args)
        .env("VAULT_NOSTR_SECRET", SECRET)
        .output()
        .expect("the vault binary runs")
}

fn json(output: &Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not one JSON document ({e}):\n{stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn propose_a_creation_then_create_it_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    write(&repo.join("ontology/vocabulary.yaml"), VOCABULARY);
    write(&repo.join("knowledge/pages/Alpha.md"), EXISTING);
    let staged = dir.path().join("staged.md");
    write(&staged, STAGED);
    let staged_arg = staged.to_str().unwrap();

    let proposed = vault(
        &repo,
        &[
            "propose",
            "--dry-run",
            "--diff",
            staged_arg,
            "urn:ngm:class:new-thing",
        ],
    );
    assert!(proposed.status.success(), "{proposed:?}");
    let proposal = &json(&proposed)["proposal"];
    assert_eq!(proposal["kind"], "create");
    assert_eq!(proposal["iri"], "urn:ngm:class:new-thing");
    assert_eq!(proposal["page"], "New Thing");
    assert_eq!(proposal["blockers"], serde_json::json!([]));
    assert!(proposal["diff"]
        .as_str()
        .unwrap()
        .starts_with("--- /dev/null\n+++ b/New Thing\n"));

    let target = repo.join("knowledge/pages/New Thing.md");
    let create = [
        "create",
        staged_arg,
        "--expect",
        "docs=1",
        "--set",
        "status=stable",
        "--set",
        "verified+={by: 'human:npub1test', at: '2026-09-22T00:00:00Z'}",
    ];
    let first = vault(&repo, &create);
    assert!(first.status.success(), "{first:?}");
    let outcome = json(&first);
    assert_eq!(outcome["created"], true);
    assert_eq!(outcome["path"], "knowledge/pages/New Thing.md");
    assert_eq!(outcome["iri"], "urn:ngm:class:new-thing");
    let written = std::fs::read_to_string(&target).unwrap();
    assert!(written.contains("status: stable"), "{written}");
    assert!(written.contains("human:npub1test"), "{written}");

    let second = vault(&repo, &create);
    assert_eq!(second.status.code(), Some(2), "{second:?}");
    let refusal = json(&second);
    assert_eq!(refusal["created"], false);
    assert_eq!(refusal["code"], "EXISTS");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), written);

    // Once created, the same staged page proposes as an amendment of it.
    let again = vault(
        &repo,
        &["propose", "--dry-run", "--diff", staged_arg, "New Thing"],
    );
    assert!(again.status.success(), "{again:?}");
    assert_eq!(json(&again)["proposal"]["kind"], "amend");
}

#[test]
fn create_refuses_an_invalid_page_or_an_undeclared_radius_with_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    write(&repo.join("ontology/vocabulary.yaml"), VOCABULARY);
    write(&repo.join("knowledge/pages/Alpha.md"), EXISTING);
    let staged = dir.path().join("staged.md");
    write(&staged, &STAGED.replace("status: draft\n", ""));
    let staged_arg = staged.to_str().unwrap();

    let invalid = vault(&repo, &["create", staged_arg, "--expect", "docs=1"]);
    assert_eq!(invalid.status.code(), Some(2), "{invalid:?}");
    assert_eq!(json(&invalid)["code"], "BLOCKED");

    let undeclared = vault(&repo, &["create", staged_arg, "--set", "status=stable"]);
    assert_eq!(undeclared.status.code(), Some(2), "{undeclared:?}");
    assert_eq!(json(&undeclared)["code"], "UNDECLARED_BLAST_RADIUS");

    assert!(!repo.join("knowledge/pages/New Thing.md").exists());
}
