//! `vault propose --json` through the real binary: stdout must be exactly one
//! JSON document, and a grouped proposal names itself as its subject.
//!
//! Grouped signing is refused until the ontology consumer applies multi-page
//! proposals atomically (`nostr::action_request`), so a grouped `--dry-run`
//! is how the group is inspected: it emits the unsigned proposal and needs no
//! key. A single-page proposal still needs its key, dry run or not.

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

/// A three-page grouped proposal: the repo pages at quality 0.35, the manifest
/// raising each to 0.55.
fn grouped_fixture(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let repo = dir.join("repo");
    write(&repo.join("ontology/vocabulary.yaml"), VOCABULARY);
    let manifest = dir.join("manifest");
    for id in ["Alpha", "Beta", "Gamma"] {
        write(&repo.join(format!("knowledge/{id}.md")), &class(id, "0.35"));
        write(&manifest.join(format!("{id}.md")), &class(id, "0.55"));
    }
    (repo, manifest)
}

/// Run `vault --json propose` at schema level against `diff` (a manifest
/// directory for a grouped proposal, a page file for a single one).
fn propose(
    repo: &Path,
    diff: &Path,
    dry_run: bool,
    secret: Option<&str>,
) -> (std::process::ExitStatus, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vault"));
    cmd.arg("--repo").arg(repo).arg("--json").arg("propose");
    if dry_run {
        cmd.arg("--dry-run");
    } else {
        // Never a real relay: every non-dry-run here must be refused first.
        cmd.args(["--relay", "ws://127.0.0.1:9"]);
    }
    cmd.args(["--level", "schema"])
        .arg("--diff")
        .arg(diff)
        .args(["--title", "Raise the quality floor", "--hypothesis", "h"])
        .arg("Alpha")
        .env_remove("VAULT_NOSTR_SECRET");
    if let Some(secret) = secret {
        cmd.env("VAULT_NOSTR_SECRET", secret);
    }
    let output = cmd.output().expect("the vault binary runs");
    (
        output.status,
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The proposal half of a grouped dry run, checked the same way whether or not
/// a key was supplied.
fn assert_unsigned_group(stdout: &str) {
    // Exactly one JSON document: serde_json rejects trailing or leading prose.
    let value: serde_json::Value = serde_json::from_str(stdout)
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
    assert_eq!(value["signed"], serde_json::json!(false), "{value}");
    assert!(
        value.get("event").is_none(),
        "a grouped proposal is never signed: {value}"
    );
}

#[test]
fn grouped_propose_json_stdout_is_one_document() {
    let dir = tempfile::tempdir().unwrap();
    let (repo, manifest) = grouped_fixture(dir.path());
    let (status, stdout, stderr) = propose(&repo, &manifest, true, Some(SECRET));
    assert!(status.success(), "stderr: {stderr}\nstdout: {stdout}");
    assert_unsigned_group(&stdout);
    // The progress note went to stderr, not stdout.
    assert!(stderr.contains("grouped proposal: 3 pages"), "{stderr}");
}

#[test]
fn grouped_dry_run_needs_no_key_and_emits_the_unsigned_group() {
    let dir = tempfile::tempdir().unwrap();
    let (repo, manifest) = grouped_fixture(dir.path());
    let (status, stdout, stderr) = propose(&repo, &manifest, true, None);
    assert!(status.success(), "stderr: {stderr}\nstdout: {stdout}");
    assert_unsigned_group(&stdout);
}

#[test]
fn grouped_propose_without_dry_run_is_still_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (repo, manifest) = grouped_fixture(dir.path());
    let (status, stdout, stderr) = propose(&repo, &manifest, false, Some(SECRET));
    assert!(!status.success(), "stdout: {stdout}");
    assert!(stderr.contains("atomic multi-page"), "{stderr}");
}

#[test]
fn a_single_page_dry_run_still_needs_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let (repo, _manifest) = grouped_fixture(dir.path());
    let page = dir.path().join("Alpha.md");
    write(&page, &class("Alpha", "0.55"));
    let (status, stdout, stderr) = propose(&repo, &page, true, None);
    assert!(!status.success(), "stdout: {stdout}");
    assert!(stderr.contains("a signing key is required"), "{stderr}");
}
