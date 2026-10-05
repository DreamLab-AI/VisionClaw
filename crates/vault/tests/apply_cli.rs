//! `vault propose` → `vault apply` through the real binary: the interface the
//! governance apply path calls once a human approves an amendment,
//! `vault --repo <root> apply <proposal.json> --expect docs=1 --set … --json`.
//! Also `vault panel publish --dry-run`, which needs no repository.

use std::path::Path;
use std::process::{Command, Output};

const VOCABULARY: &str = r#"version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
  related-to: { owl: "skos:related" }
  disjoint-with: { owl: "owl:disjointWith" }
scalars:
  quality: { type: number, min: 0, max: 1 }
"#;

/// A throwaway secp256k1 secret with no standing anywhere.
const SECRET: &str = "0101010101010101010101010101010101010101010101010101010101010101";

const TOKEN: &str = "---\ntype: Class\ntitle: Token\npublic: true\nresource: urn:ngm:class:token\nstatus: stable\n---\nA token.\n";
const NFT: &str = "---\ntype: Class\ntitle: Non-Fungible Token\npublic: true\nresource: urn:ngm:class:non-fungible-token\nstatus: stable\nis-a:\n- '[[Token]]'\n---\nA unique token.\n";
const FT: &str = "---\ntype: Class\ntitle: Fungible Token\npublic: true\nresource: urn:ngm:class:fungible-token\nstatus: stable\nquality: 0.72\nis-a:\n- '[[Token]]'\nrelated-to:\n- '[[Non-Fungible Token]]'\n---\nAn interchangeable token.\n";

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn vault(repo: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vault"));
    if let Some(repo) = repo {
        command.arg("--repo").arg(repo);
    }
    command
        .arg("--json")
        .args(args)
        .env("VAULT_NOSTR_SECRET", SECRET)
        .env_remove("VAULT_RELAY_URL")
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

/// A repository holding the three token pages, and a proposal (as the 31402's
/// JSON content, made by `vault propose --dry-run`) adding `disjoint-with` to
/// Fungible Token after its `is-a`.
struct Fixture {
    _dir: tempfile::TempDir,
    repo: std::path::PathBuf,
    proposal: serde_json::Value,
    proposal_path: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        write(&repo.join("ontology/vocabulary.yaml"), VOCABULARY);
        write(&repo.join("knowledge/pages/Token.md"), TOKEN);
        write(&repo.join("knowledge/pages/Non-Fungible Token.md"), NFT);
        write(&repo.join("knowledge/pages/Fungible Token.md"), FT);
        let staged = dir.path().join("staged.md");
        write(
            &staged,
            &FT.replace(
                "- '[[Token]]'\n",
                "- '[[Token]]'\ndisjoint-with:\n- '[[Non-Fungible Token]]'\n",
            ),
        );
        let proposed = vault(
            Some(&repo),
            &[
                "propose",
                "--dry-run",
                "--level",
                "schema",
                "--hypothesis",
                "fungible and non-fungible tokens share no instance",
                "--diff",
                staged.to_str().unwrap(),
                "Fungible Token",
            ],
        );
        assert!(proposed.status.success(), "{proposed:?}");
        let value = json(&proposed);
        let proposal = value["proposal"].clone();
        assert_eq!(proposal["kind"], "amend");
        assert_eq!(proposal["blockers"], serde_json::json!([]));
        assert!(
            proposal["diff"]
                .as_str()
                .unwrap()
                .contains("+disjoint-with:\n+- '[[Non-Fungible Token]]'\n"),
            "{proposal}"
        );
        // The 31402 content is exactly this proposal.
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(value["event"]["content"].as_str().unwrap())
                .unwrap(),
            proposal
        );
        let proposal_path = dir.path().join("proposal.json");
        write(&proposal_path, &proposal.to_string());
        Self {
            _dir: dir,
            repo,
            proposal,
            proposal_path,
        }
    }

    fn page(&self) -> std::path::PathBuf {
        self.repo.join("knowledge/pages/Fungible Token.md")
    }

    /// Write a variant of the proposal and return its path.
    fn variant(&self, edit: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut proposal = self.proposal.clone();
        edit(&mut proposal);
        let path = self.proposal_path.with_file_name("variant.json");
        write(&path, &proposal.to_string());
        path.to_str().unwrap().to_owned()
    }

    fn apply(&self, file: &str, extra: &[&str]) -> Output {
        let mut args = vec!["apply", file];
        args.extend_from_slice(extra);
        vault(Some(&self.repo), &args)
    }

    /// Assert a refusal: exit 2, the code on stdout, the page untouched.
    fn assert_refused(&self, output: &Output, code: &str) {
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let value = json(output);
        assert_eq!(value["applied"], false, "{value}");
        assert_eq!(value["code"], code, "{value}");
        assert!(value["message"].as_str().is_some_and(|m| !m.is_empty()));
        assert!(value["blockers"].is_array());
        assert_eq!(std::fs::read_to_string(self.page()).unwrap(), FT);
    }
}

#[test]
fn an_approved_amendment_is_applied_once_with_its_verification() {
    let fx = Fixture::new();
    let file = fx.proposal_path.to_str().unwrap();
    let output = fx.apply(
        file,
        &[
            "--expect",
            "docs=1",
            "--set",
            "verified+={by: 'human:npub1test', at: '2026-10-05T19:00:00Z'}",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let value = json(&output);
    assert_eq!(
        value,
        serde_json::json!({
            "applied": true,
            "page": "Fungible Token",
            "path": "knowledge/pages/Fungible Token.md",
            "digest": fx.proposal["digest"],
        })
    );
    let written = std::fs::read_to_string(fx.page()).unwrap();
    assert!(
        written.contains("is-a:\n- '[[Token]]'\ndisjoint-with:\n- '[[Non-Fungible Token]]'\n"),
        "{written}"
    );
    assert!(written.contains("human:npub1test"), "{written}");
    assert!(
        written.ends_with("---\nAn interchangeable token.\n"),
        "{written}"
    );

    // The same proposal again: its removed/context lines no longer match.
    let again = fx.apply(file, &["--expect", "docs=1"]);
    assert_eq!(again.status.code(), Some(2), "{again:?}");
    assert_eq!(json(&again)["code"], "STALE");
    assert_eq!(std::fs::read_to_string(fx.page()).unwrap(), written);
}

#[test]
fn the_signed_event_itself_is_accepted_as_the_proposal() {
    let fx = Fixture::new();
    let event = serde_json::json!({
        "id": "00", "pubkey": "00", "created_at": 1, "kind": 31402,
        "tags": [], "content": fx.proposal.to_string(), "sig": "00"
    });
    let path = fx.proposal_path.with_file_name("event.json");
    write(&path, &event.to_string());
    let output = fx.apply(path.to_str().unwrap(), &["--expect", "docs=1"]);
    assert!(output.status.success(), "{output:?}");
    assert!(std::fs::read_to_string(fx.page())
        .unwrap()
        .contains("disjoint-with:"));
}

#[test]
fn a_page_changed_around_the_hunk_is_stale() {
    let fx = Fixture::new();
    // Shift every line by one: the context still exists, one line lower.
    let shifted = FT.replace("type: Class\n", "type: Class\naliases:\n- FT\n");
    std::fs::write(fx.page(), &shifted).unwrap();
    let output = fx.apply(fx.proposal_path.to_str().unwrap(), &["--expect", "docs=1"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(json(&output)["code"], "STALE");
    assert_eq!(std::fs::read_to_string(fx.page()).unwrap(), shifted);
    std::fs::write(fx.page(), FT).unwrap();

    // A context line edited in place.
    let edited = FT.replace("quality: 0.72\n", "quality: 0.8\n");
    std::fs::write(fx.page(), &edited).unwrap();
    let output = fx.apply(fx.proposal_path.to_str().unwrap(), &["--expect", "docs=1"]);
    assert_eq!(json(&output)["code"], "STALE");
    assert_eq!(std::fs::read_to_string(fx.page()).unwrap(), edited);
}

#[test]
fn a_tampered_proposal_is_a_digest_mismatch() {
    let fx = Fixture::new();
    let file = fx.variant(|p| {
        let diff = p["diff"].as_str().unwrap().replace(
            "+- '[[Non-Fungible Token]]'",
            "+- '[[Semi-Fungible Token]]'",
        );
        p["diff"] = diff.into();
    });
    fx.assert_refused(&fx.apply(&file, &["--expect", "docs=1"]), "DIGEST_MISMATCH");
}

#[test]
fn a_page_holding_another_iri_is_an_iri_mismatch() {
    let fx = Fixture::new();
    // The proposal is re-digested honestly, so only the IRI guard can catch it.
    let file = fx.variant(|p| {
        p["iri"] = "urn:ngm:class:something-else".into();
        let proposal: vault_core::proposal::PatchProposal =
            serde_json::from_value(p.clone()).unwrap();
        p["digest"] = vault::apply::expected_digest(&proposal).into();
    });
    fx.assert_refused(&fx.apply(&file, &["--expect", "docs=1"]), "IRI_MISMATCH");
}

#[test]
fn a_creation_is_not_amend() {
    let fx = Fixture::new();
    let file = fx.variant(|p| {
        p["kind"] = "create".into();
        let proposal: vault_core::proposal::PatchProposal =
            serde_json::from_value(p.clone()).unwrap();
        p["digest"] = vault::apply::expected_digest(&proposal).into();
    });
    fx.assert_refused(&fx.apply(&file, &["--expect", "docs=1"]), "NOT_AMEND");
}

#[test]
fn an_absent_page_is_no_page() {
    let fx = Fixture::new();
    std::fs::remove_file(fx.page()).unwrap();
    let output = fx.apply(fx.proposal_path.to_str().unwrap(), &["--expect", "docs=1"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(json(&output)["code"], "NO_PAGE");
    assert!(!fx.page().exists());
}

#[test]
fn an_undeclared_or_wrong_blast_radius_is_refused() {
    let fx = Fixture::new();
    let file = fx.proposal_path.to_str().unwrap();
    fx.assert_refused(&fx.apply(file, &[]), "EXPECT");
    fx.assert_refused(&fx.apply(file, &["--expect", "docs=2"]), "EXPECT");
    fx.assert_refused(
        &fx.apply(
            file,
            &["--expect", "docs=1,blocks=2", "--set", "status=stable"],
        ),
        "EXPECT",
    );
}

#[test]
fn a_result_failing_validation_is_invalid() {
    let fx = Fixture::new();
    // Unsetting a required key in the same write makes the result invalid.
    let output = fx.apply(
        fx.proposal_path.to_str().unwrap(),
        &["--expect", "docs=1", "--unset", "status"],
    );
    fx.assert_refused(&output, "INVALID");
    assert!(!json(&output)["blockers"].as_array().unwrap().is_empty());
}

#[test]
fn panel_publish_dry_run_signs_the_panel_with_the_proposing_key() {
    // No `--repo`, and run from a directory that is not a repository.
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vault"))
        .current_dir(dir.path())
        .args(["--json", "panel", "publish", "--dry-run"])
        .env("VAULT_NOSTR_SECRET", SECRET)
        .output()
        .expect("the vault binary runs");
    assert!(output.status.success(), "{output:?}");
    let value = json(&output);
    let event: nostr_bbs_core::NostrEvent = serde_json::from_value(value["event"].clone()).unwrap();
    assert!(nostr_bbs_core::event::verify_event(&event));
    assert_eq!(event.kind, 31400);
    assert_eq!(
        value["address"],
        format!("31400:{}:ontology-governance", event.pubkey)
    );
    let definition: nostr_bbs_core::governance::PanelDefinition =
        serde_json::from_str(&event.content).unwrap();
    assert_eq!(
        definition,
        nostr_bbs_core::ontology_governance::ontology_governance_panel()
    );

    // The same key's proposals name this panel and this author.
    let fx = Fixture::new();
    let proposed = vault(
        Some(&fx.repo),
        &[
            "propose",
            "--dry-run",
            "--diff",
            fx.proposal_path
                .with_file_name("staged.md")
                .to_str()
                .unwrap(),
            "Fungible Token",
        ],
    );
    let request = &json(&proposed)["event"];
    assert_eq!(request["pubkey"], event.pubkey);
    assert!(request["tags"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(["panel", "ontology-governance"])));

    // Without a key, nothing is signed.
    let keyless = Command::new(env!("CARGO_BIN_EXE_vault"))
        .current_dir(dir.path())
        .args(["panel", "publish", "--dry-run"])
        .env_remove("VAULT_NOSTR_SECRET")
        .output()
        .unwrap();
    assert!(!keyless.status.success());
}
