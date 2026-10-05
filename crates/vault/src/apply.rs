//! `vault apply` — the corpus write for a human-approved amendment.
//!
//! `vault propose` files an amendment as a forum 31402 whose content is a
//! `PatchProposal` (`kind: "amend"`). Once a human approves the case, the apply
//! path calls
//!
//! ```text
//! vault --repo <root> apply <proposal.json> --expect docs=1 \
//!       --set 'verified+={by: human:npub1…, at: 2026-10-05T19:00:00Z}' --json
//! ```
//!
//! and the proposal's unified diff is applied to its page, with the `--set`
//! keys, in one atomic write (a temporary file renamed over the page). `vault`
//! stays the only writer of the corpus: `edit` mutates keys, `create` writes a
//! new page, `apply` lands an approved amendment.
//!
//! `<proposal.json>` is the `PatchProposal` JSON, or the whole signed 31402
//! event whose `content` it is.
//!
//! # The guards
//!
//! Every refusal writes nothing and exits **2**, naming a stable code:
//!
//! | code | refused because |
//! |---|---|
//! | `MALFORMED` | the file is not a proposal, or its `diff` is not a unified diff of its page |
//! | `NOT_AMEND` | `kind` is not `amend` — a creation is applied with `vault create` |
//! | `DIGEST_MISMATCH` | `digest` is not the digest `vault propose` computes over the proposal's fields |
//! | `EXPECT` | `--expect docs=1` was not declared, the proposal changes more than one document, or `blocks=N` disagrees with the keys `--set`/`--unset` change |
//! | `NO_PAGE` | the proposal's page is not in the knowledge vault |
//! | `IRI_MISMATCH` | the page's `resource` IRI is not the proposal's `iri` |
//! | `STALE` | the diff does not apply exactly to the page as it is now |
//! | `INVALID` | the result introduces a validation error on the page |
//!
//! # Staleness is the diff context, not the generation
//!
//! The proposal's `generation` is **not** compared: generations are routinely
//! `+dirty`, so they would refuse nearly every real apply while proving nothing
//! about the one page that matters. Instead the diff is applied with no fuzz and
//! no offset search — every context and removed line must equal the current
//! page at exactly the line its hunk header states. A page that drifted in or
//! around the changed region is `STALE`; an unrelated edit elsewhere in the page
//! that leaves every hunk in place is not.
//!
//! The base the diff is applied to is the page as `vault` renders it — the text
//! `vault propose` diffed against. For a page `vault` wrote, that is the file
//! byte for byte. The file is read once at preparation, and re-read just before
//! the rename: if it changed in between, the write is refused as `STALE`.

use std::collections::{BTreeMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use vault_core::page::{Page, Vault};
use vault_core::promotion::Blocker;
use vault_core::proposal::{PatchProposal, ProposalKind};
use vault_core::vocabulary::Vocabulary;

use crate::diff::{self, PatchError};
use crate::edit::{apply_changes, Change, Expectation};
use crate::model::Corpus;
use crate::validate;

/// Why an apply was refused. Every variant means nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    /// The input is not a proposal, or its diff cannot be read.
    #[error("refused: {0}")]
    Malformed(String),
    /// The proposal is not an amendment.
    #[error("refused: the proposal is kind `{0}`; only `amend` is applied here (a creation goes through `vault create`)")]
    NotAmend(String),
    /// The stated digest is not the one the proposal's fields hash to.
    #[error("refused: the proposal states digest {stated} but its fields hash to {computed}")]
    DigestMismatch {
        /// The `digest` field.
        stated: String,
        /// What `vault propose` would compute.
        computed: String,
    },
    /// The declared blast radius is absent or wrong.
    #[error("refused: {0}")]
    Expect(String),
    /// The proposal's page is not in the vault.
    #[error("refused: no knowledge page `{0}`")]
    NoPage(String),
    /// The page's IRI is not the proposal's.
    #[error("refused: page `{page}` has IRI {actual}, but the proposal is for {expected}")]
    IriMismatch {
        /// The page id.
        page: String,
        /// The proposal's `iri`.
        expected: String,
        /// The page's `resource` IRI.
        actual: String,
    },
    /// The diff does not apply exactly to the current page.
    #[error("refused: page `{page}` has changed since the proposal: {detail}")]
    Stale {
        /// The page id.
        page: String,
        /// Where the diff stopped matching.
        detail: String,
    },
    /// The result introduces validation errors.
    #[error("refused: {} new validation error(s) on the result: {}", .0.len(), join(.0))]
    Invalid(Vec<Blocker>),
}

impl ApplyError {
    /// A stable machine code for the refusal, for `--json` consumers.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Malformed(_) => "MALFORMED",
            Self::NotAmend(_) => "NOT_AMEND",
            Self::DigestMismatch { .. } => "DIGEST_MISMATCH",
            Self::Expect(_) => "EXPECT",
            Self::NoPage(_) => "NO_PAGE",
            Self::IriMismatch { .. } => "IRI_MISMATCH",
            Self::Stale { .. } => "STALE",
            Self::Invalid(_) => "INVALID",
        }
    }

    /// The individual blockers, when the refusal carries any.
    #[must_use]
    pub fn blockers(&self) -> &[Blocker] {
        match self {
            Self::Invalid(b) => b,
            _ => &[],
        }
    }
}

fn join(blockers: &[Blocker]) -> String {
    blockers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Read a proposal file: the `PatchProposal` JSON itself, or a signed 31402
/// event whose `content` is that JSON.
///
/// # Errors
/// [`ApplyError::Malformed`] when neither form parses.
pub fn read_proposal(text: &str) -> Result<PatchProposal, ApplyError> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ApplyError::Malformed(format!("the proposal is not JSON: {e}")))?;
    // A Nostr event has a numeric `kind` and a string `content`; a proposal's
    // `kind` is the string `amend` or `create`.
    let value = match (value.get("kind"), value.get("content")) {
        (Some(serde_json::Value::Number(_)), Some(serde_json::Value::String(content))) => {
            serde_json::from_str(content).map_err(|e| {
                ApplyError::Malformed(format!("the event's content is not JSON: {e}"))
            })?
        }
        _ => value,
    };
    serde_json::from_value(value)
        .map_err(|e| ApplyError::Malformed(format!("not a PatchProposal: {e}")))
}

/// The digest `vault propose` computes for `proposal`, over its own fields.
#[must_use]
pub fn expected_digest(proposal: &PatchProposal) -> String {
    PatchProposal::compute_digest_of_kind(
        proposal.level,
        proposal.kind,
        &proposal.iri,
        &proposal.page,
        &proposal.hypothesis,
        &proposal.diff,
        &proposal.pages,
    )
}

/// What an apply does: the page, where it lives, and the proposal it lands.
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    /// Always `true` in a returned outcome; refusals are errors.
    pub applied: bool,
    /// The page id.
    pub page: String,
    /// The file written, relative to the repository root.
    pub path: String,
    /// The applied proposal's digest.
    pub digest: String,
    /// Key to `(before, after)` for the keys `--set`/`--unset` changed.
    #[serde(skip)]
    pub changes: BTreeMap<String, [Option<String>; 2]>,
    /// The page's new text.
    #[serde(skip)]
    pub rendered: String,
    /// The file's text as read at preparation; the write is refused if the
    /// file no longer holds exactly this.
    #[serde(skip)]
    pub original: String,
    /// The page file.
    #[serde(skip)]
    pub target: PathBuf,
}

/// Prepare the application of `proposal` to the knowledge `vault`, with
/// `changes` applied to the result, enforcing `expect`. Nothing is written.
///
/// The page is read from disk afresh, so the staleness check runs against the
/// file as it is now rather than as it was when `vault` was loaded.
///
/// # Errors
/// [`ApplyError`] on any of the guards in the module documentation.
pub fn prepare(
    vault: &Vault,
    vocab: &Vocabulary,
    proposal: &PatchProposal,
    changes: &[Change],
    expect: Expectation,
) -> Result<Outcome, ApplyError> {
    let patch = single_page_patch(proposal, expect)?;
    let id = proposal.page.as_str();
    let loaded = vault
        .get(id)
        .ok_or_else(|| ApplyError::NoPage(id.to_owned()))?;
    let original = std::fs::read_to_string(&loaded.path)
        .map_err(|e| ApplyError::NoPage(format!("{id} ({}: {e})", loaded.path.display())))?;
    let current = Page::parse(&loaded.path, &loaded.rel_path, id, &original).map_err(|e| {
        ApplyError::Stale {
            page: id.to_owned(),
            detail: format!("the current page does not parse: {e}"),
        }
    })?;
    let actual_iri = current.resource(&vocab.namespace);
    if actual_iri != proposal.iri {
        return Err(ApplyError::IriMismatch {
            page: id.to_owned(),
            expected: proposal.iri.clone(),
            actual: actual_iri,
        });
    }

    let base = current.render().map_err(|e| ApplyError::Stale {
        page: id.to_owned(),
        detail: format!("the current page does not render: {e}"),
    })?;
    let patched = patch.apply(&base).map_err(|e| match e {
        PatchError::Stale(detail) => ApplyError::Stale {
            page: id.to_owned(),
            detail,
        },
        PatchError::Malformed(m) => ApplyError::Malformed(m),
    })?;

    let invalid = |e: &dyn std::fmt::Display| {
        ApplyError::Invalid(vec![Blocker::new(
            "FRONTMATTER_INVALID",
            format!("the patched {id} does not parse: {e}"),
        )])
    };
    let mut page =
        Page::parse(&current.path, &current.rel_path, id, &patched).map_err(|e| invalid(&e))?;
    let touched = apply_changes(&mut page, changes);
    if let Some(declared) = expect.blocks {
        if declared != touched.len() {
            return Err(ApplyError::Expect(format!(
                "--expect blocks={declared}, but --set/--unset change {} key(s)",
                touched.len()
            )));
        }
    }
    // Without `--set`, the approved text lands verbatim; with it, the page is
    // re-rendered exactly as `vault edit` renders an edit.
    let rendered = if changes.is_empty() {
        patched
    } else {
        page.render().map_err(|e| invalid(&e))?
    };
    if rendered == original {
        return Err(ApplyError::Expect(
            "--expect docs=1, but the result is identical to the page: 0 documents change".into(),
        ));
    }
    let page =
        Page::parse(&current.path, &current.rel_path, id, &rendered).map_err(|e| invalid(&e))?;

    let introduced = introduced_errors(vault, vocab, &current, page);
    if !introduced.is_empty() {
        return Err(ApplyError::Invalid(introduced));
    }

    let vault_dir = vault
        .root
        .file_name()
        .map_or_else(String::new, |n| format!("{}/", n.to_string_lossy()));
    Ok(Outcome {
        applied: true,
        page: id.to_owned(),
        path: format!("{vault_dir}{}", current.rel_path.display()),
        digest: proposal.digest.clone(),
        changes: touched,
        rendered,
        original,
        target: current.path,
    })
}

/// The proposal-only guards, in order: the declared radius, the kind, the
/// digest, and a diff that changes exactly the one page the proposal names.
fn single_page_patch(
    proposal: &PatchProposal,
    expect: Expectation,
) -> Result<diff::FilePatch, ApplyError> {
    let Some(declared_docs) = expect.docs else {
        return Err(ApplyError::Expect(
            "--expect must declare docs=1 for an apply".into(),
        ));
    };
    if proposal.kind != ProposalKind::Amend {
        return Err(ApplyError::NotAmend(proposal.kind.as_str().into()));
    }
    let computed = expected_digest(proposal);
    if computed != proposal.digest {
        return Err(ApplyError::DigestMismatch {
            stated: proposal.digest.clone(),
            computed,
        });
    }

    let mut files =
        diff::parse(&proposal.diff).map_err(|e| ApplyError::Malformed(e.to_string()))?;
    let mut documents: HashSet<&str> = proposal.pages.iter().map(String::as_str).collect();
    documents.extend(files.iter().map(|f| f.old_name.as_str()));
    documents.insert(&proposal.page);
    if declared_docs != 1 || documents.len() != 1 || files.len() != 1 {
        return Err(ApplyError::Expect(format!(
            "--expect docs={declared_docs}, but the proposal changes {} document(s)",
            documents.len().max(files.len())
        )));
    }
    let patch = files.remove(0);
    if patch.new_name != patch.old_name {
        return Err(ApplyError::Malformed(format!(
            "the diff renames `{}` to `{}`",
            patch.old_name, patch.new_name
        )));
    }
    Ok(patch)
}

/// Validation errors on the page after the apply that were not on it before.
///
/// The apply answers for what it introduces, exactly as `vault propose` does:
/// a pre-existing defect on the page does not stop an approved amendment that
/// leaves it no worse.
fn introduced_errors(
    vault: &Vault,
    vocab: &Vocabulary,
    current: &Page,
    after: Page,
) -> Vec<Blocker> {
    let page_errors = |v: &Vault| -> Vec<Blocker> {
        let corpus = Corpus::build(v, vocab);
        validate::validate(v, &corpus, vocab)
            .errors()
            .into_iter()
            .filter(|issue| issue.path == current.id)
            .map(|issue| Blocker::new(issue.code.clone(), issue.message.clone()))
            .collect()
    };
    let mut before_vault = vault.clone();
    if let Some(slot) = before_vault.get_mut(&current.id) {
        *slot = current.clone();
    }
    let before = page_errors(&before_vault);
    let mut after_vault = vault.clone();
    if let Some(slot) = after_vault.get_mut(&current.id) {
        *slot = after;
    }
    let mut introduced: Vec<Blocker> = Vec::new();
    for blocker in page_errors(&after_vault) {
        if !before.contains(&blocker) && !introduced.contains(&blocker) {
            introduced.push(blocker);
        }
    }
    introduced
}

/// Write a prepared apply atomically: a temporary file beside the page,
/// synced, then renamed over it.
///
/// The page is re-read immediately before the rename; if it no longer holds
/// the text [`prepare`] read, the temporary file is removed and the write is
/// refused as [`ApplyError::Stale`].
///
/// # Errors
/// [`ApplyError::Stale`] (inside the `anyhow::Error`) when the page changed;
/// any I/O failure, wrapped in `anyhow`.
pub fn write(outcome: &Outcome) -> anyhow::Result<()> {
    let temp = temp_path(&outcome.target);
    write_temp(&temp, &outcome.rendered)
        .map_err(|e| anyhow::Error::new(e).context(format!("writing {}", temp.display())))?;
    let result = (|| -> anyhow::Result<()> {
        let now = std::fs::read_to_string(&outcome.target)
            .map_err(|e| anyhow::anyhow!("re-reading {}: {e}", outcome.target.display()))?;
        if now != outcome.original {
            return Err(anyhow::Error::new(ApplyError::Stale {
                page: outcome.page.clone(),
                detail: "the file changed while the apply was being prepared".into(),
            }));
        }
        std::fs::rename(&temp, &outcome.target).map_err(|e| {
            anyhow::anyhow!(
                "renaming {} over {}: {e}",
                temp.display(),
                outcome.target.display()
            )
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// `pages/.Fungible Token.md.vault-apply-<pid>.tmp`: hidden, beside the target
/// so the rename stays on one filesystem.
fn temp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map_or_else(|| "page".into(), |n| n.to_string_lossy().into_owned());
    target.with_file_name(format!(".{name}.vault-apply-{}.tmp", std::process::id()))
}

fn write_temp(path: &Path, text: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(e) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::unified_diff;
    use vault_core::page::VaultKind;
    use vault_core::proposal::Level;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
  disjoint-with: { owl: "owl:disjointWith" }
"#,
        )
        .unwrap()
    }

    const TOKEN: &str = "---\ntype: Class\ntitle: Token\npublic: true\nresource: urn:ngm:class:token\nstatus: stable\n---\nA token.\n";
    const NFT: &str = "---\ntype: Class\ntitle: Non-Fungible Token\npublic: true\nresource: urn:ngm:class:non-fungible-token\nstatus: stable\nis-a:\n- '[[Token]]'\n---\nAn NFT.\n";
    const FT: &str = "---\ntype: Class\ntitle: Fungible Token\npublic: true\nresource: urn:ngm:class:fungible-token\nstatus: stable\nis-a:\n- '[[Token]]'\n---\nA fungible token.\n";

    fn amended() -> String {
        FT.replace(
            "- '[[Token]]'\n",
            "- '[[Token]]'\ndisjoint-with:\n- '[[Non-Fungible Token]]'\n",
        )
    }

    fn vault_in(dir: &Path) -> Vault {
        let pages = dir.join("knowledge/pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(pages.join("Token.md"), TOKEN).unwrap();
        std::fs::write(pages.join("Non-Fungible Token.md"), NFT).unwrap();
        std::fs::write(pages.join("Fungible Token.md"), FT).unwrap();
        Vault::load(dir.join("knowledge"), VaultKind::Knowledge).unwrap()
    }

    fn proposal(after: &str) -> PatchProposal {
        PatchProposal::new(
            Level::Content,
            "urn:ngm:class:fungible-token",
            "Fungible Token",
            "fungible and non-fungible tokens are disjoint",
            unified_diff("Fungible Token", FT, after),
            vec![],
            "process:vault/1.0",
            "visionGraph@abc1234+dirty",
            "2026-10-19T00:00:00Z",
        )
    }

    fn docs1() -> Expectation {
        Expectation::parse("docs=1").unwrap()
    }

    #[test]
    fn an_approved_amendment_lands_with_its_set_keys_in_one_write() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let sets = vec![Change::parse_set(
            "verified+={by: 'human:npub1test', at: '2026-10-05T19:00:00Z'}",
        )
        .unwrap()];
        let p = proposal(&amended());
        let outcome = prepare(&vault, &vocab(), &p, &sets, docs1()).unwrap();
        assert_eq!(outcome.page, "Fungible Token");
        assert_eq!(outcome.path, "knowledge/pages/Fungible Token.md");
        assert_eq!(outcome.digest, p.digest);
        write(&outcome).unwrap();
        let written =
            std::fs::read_to_string(dir.path().join("knowledge/pages/Fungible Token.md")).unwrap();
        assert!(written.contains("disjoint-with:\n- '[[Non-Fungible Token]]'\n"));
        assert!(written.contains("human:npub1test"), "{written}");
        // No temporary file left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("knowledge/pages"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn without_set_the_approved_text_lands_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let outcome = prepare(&vault, &vocab(), &proposal(&amended()), &[], docs1()).unwrap();
        assert_eq!(outcome.rendered, amended());
    }

    #[test]
    fn the_blast_radius_must_be_declared_and_one_document() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let p = proposal(&amended());
        for expect in ["", "docs=2", "docs=1,blocks=1"] {
            let err = prepare(
                &vault,
                &vocab(),
                &p,
                &[],
                Expectation::parse(expect).unwrap(),
            )
            .unwrap_err();
            assert_eq!(err.code(), "EXPECT", "{expect}: {err}");
        }
    }

    #[test]
    fn a_creation_is_not_applied_here() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let p = proposal(&amended()).with_kind(ProposalKind::Create);
        let err = prepare(&vault, &vocab(), &p, &[], docs1()).unwrap_err();
        assert_eq!(err, ApplyError::NotAmend("create".into()));
    }

    #[test]
    fn a_tampered_field_breaks_the_digest() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let mut p = proposal(&amended());
        p.hypothesis.push_str(" (edited)");
        assert_eq!(
            prepare(&vault, &vocab(), &p, &[], docs1())
                .unwrap_err()
                .code(),
            "DIGEST_MISMATCH"
        );
        // The digest is propose's own: it matches what `new` computed.
        assert_eq!(
            expected_digest(&proposal(&amended())),
            proposal(&amended()).digest
        );
    }

    #[test]
    fn a_page_whose_iri_moved_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let mut p = proposal(&amended());
        p.iri = "urn:ngm:class:something-else".into();
        p.digest = expected_digest(&p);
        assert_eq!(
            prepare(&vault, &vocab(), &p, &[], docs1())
                .unwrap_err()
                .code(),
            "IRI_MISMATCH"
        );
    }

    #[test]
    fn a_page_that_drifted_is_stale_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let path = dir.path().join("knowledge/pages/Fungible Token.md");
        let drifted = FT.replace("status: stable\n", "status: draft\n");
        std::fs::write(&path, &drifted).unwrap();
        let err = prepare(&vault, &vocab(), &proposal(&amended()), &[], docs1()).unwrap_err();
        assert_eq!(err.code(), "STALE", "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), drifted);
    }

    #[test]
    fn a_change_between_prepare_and_write_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let outcome = prepare(&vault, &vocab(), &proposal(&amended()), &[], docs1()).unwrap();
        let path = dir.path().join("knowledge/pages/Fungible Token.md");
        std::fs::write(&path, format!("{FT}late edit\n")).unwrap();
        let err = write(&outcome).unwrap_err();
        assert_eq!(
            err.downcast_ref::<ApplyError>().map(ApplyError::code),
            Some("STALE")
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{FT}late edit\n")
        );
    }

    #[test]
    fn a_result_that_breaks_validation_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let broken = FT.replace("status: stable\n", "");
        let err = prepare(&vault, &vocab(), &proposal(&broken), &[], docs1()).unwrap_err();
        assert_eq!(err.code(), "INVALID", "{err}");
        assert_ne!(err.blockers(), []);
    }

    #[test]
    fn a_grouped_proposal_is_more_than_one_document() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_in(dir.path());
        let p = proposal(&amended()).with_pages(["Fungible Token".into(), "Token".into()]);
        assert_eq!(
            prepare(&vault, &vocab(), &p, &[], docs1())
                .unwrap_err()
                .code(),
            "EXPECT"
        );
    }

    #[test]
    fn the_proposal_reads_bare_or_inside_its_event() {
        let p = proposal(&amended());
        let bare = serde_json::to_string(&p).unwrap();
        assert_eq!(read_proposal(&bare).unwrap(), p);
        let event = serde_json::json!({
            "id": "00", "pubkey": "00", "created_at": 1, "kind": 31402,
            "tags": [], "content": bare, "sig": "00"
        });
        assert_eq!(read_proposal(&event.to_string()).unwrap(), p);
        assert_eq!(read_proposal("{}").unwrap_err().code(), "MALFORMED");
    }
}
