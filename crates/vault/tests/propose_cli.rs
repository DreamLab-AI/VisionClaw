//! `vault propose --json` through the real binary: stdout must be exactly one
//! JSON document, and a grouped proposal names itself as its subject.

use std::path::Path;
use std::process::Command;

const VOCABULARY: &str = r#"version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
scalars:
  quality: { type: number, min: 0, max: 1 }
"#;

/// A throwaway secp256k1 secret with no standing anywhere.
const SECRET: &str = "0101010101010101010101010101010101010101010101010101010101010101";

fn class(id: &str, quality: &str) -> String {
    format!(
        "---\ntype: Class\npublic: true\nresource: urn:ngm:class:{}\nstatus: stable\nquality: {quality}\n---\nbody\n",
        id.to_lowercase()
    )
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn grouped_propose_json_stdout_is_one_document() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    write(&repo.join("ontology/vocabulary.yaml"), VOCABULARY);
    let manifest = dir.path().join("manifest");
    for id in ["Alpha", "Beta", "Gamma"] {
        write(
            &repo.join(format!("knowledge/pages/{id}.md")),
            &class(id, "0.35"),
        );
        write(&manifest.join(format!("{id}.md")), &class(id, "0.55"));
    }

    let output = Command::new(env!("CARGO_BIN_EXE_vault"))
        .arg("--repo")
        .arg(&repo)
        .arg("--json")
        .args(["propose", "--dry-run", "--level", "schema"])
        .arg("--diff")
        .arg(&manifest)
        .args(["--title", "Raise the quality floor", "--hypothesis", "h"])
        .arg("Alpha")
        .env("VAULT_NOSTR_SECRET", SECRET)
        .output()
        .expect("the vault binary runs");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stderr: {stderr}\nstdout: {stdout}"
    );

    // Exactly one JSON document: serde_json rejects trailing or leading prose.
    let value: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not one JSON document ({e}):\n{stdout}"));
    let proposal = &value["proposal"];
    assert_eq!(
        proposal["iri"], "urn:ngm:proposal:raise-the-quality-floor",
        "a grouped proposal is its own subject"
    );
    assert_eq!(proposal["page"], proposal["iri"]);
    assert_eq!(
        proposal["pages"],
        serde_json::json!(["Alpha", "Beta", "Gamma"])
    );
    assert_eq!(proposal["blockers"], serde_json::json!([]));
    let context = value["event"]["tags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t[0] == "context_url")
        .expect("a context_url tag");
    assert_eq!(context[1], "urn:ngm:proposal:raise-the-quality-floor");
    // The progress note went to stderr, not stdout.
    assert!(stderr.contains("grouped proposal: 3 pages"), "{stderr}");
}
