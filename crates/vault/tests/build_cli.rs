//! `vault build` through the real binary: two artefacts claiming one output
//! path refuse the build with exit code 2, naming both pages, and nothing is
//! promoted.

use std::path::Path;
use std::process::Command;

const VOCABULARY: &str = r#"version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
"#;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn class(extra: &str) -> String {
    format!("---\ntype: Class\npublic: true\nstatus: stable\n{extra}---\nA page.\n")
}

#[test]
fn a_path_claimed_twice_exits_2_and_promotes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    write(&repo.join("ontology/vocabulary.yaml"), VOCABULARY);
    let pages = repo.join("knowledge/pages");
    // `Foo` loses the slug `foo` to the page declaring it, and its fallback —
    // its resource tail `bar` — is the slug `Bar` derives from its title.
    write(
        &pages.join("Declared.md"),
        &class("slug: foo\nresource: urn:ngm:class:declared\n"),
    );
    write(
        &pages.join("Foo.md"),
        &class("resource: urn:ngm:class:bar\n"),
    );
    write(
        &pages.join("Bar.md"),
        &class("resource: urn:ngm:class:bar-page\n"),
    );
    let out = dir.path().join("www");

    let output = Command::new(env!("CARGO_BIN_EXE_vault"))
        .arg("--repo")
        .arg(&repo)
        .args(["build", "--vault", "knowledge", "--out"])
        .arg(&out)
        .output()
        .expect("the vault binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(stderr.contains("api/pages/bar.json"), "{stderr}");
    assert!(
        stderr.contains("`Foo`") && stderr.contains("`Bar`"),
        "{stderr}"
    );
    assert!(!out.exists(), "nothing is promoted");
}
