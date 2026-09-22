//! `vault propose` — build a [`PatchProposal`], run the blockers, and (unless
//! `--dry-run`) post the 31402.
//!
//! The order is the point. Whelk and the conflict detector run **before**
//! anything is published, and everything they find becomes a blocker. A
//! proposal with blockers is still *emitted* — so the refusal is auditable and
//! the agent can see what to fix — but it is never posted, and no human is ever
//! asked to wave it through (decision Q6).

use std::path::Path;

use vault_core::page::{Page, Vault};
use vault_core::promotion::Blocker;
use vault_core::proposal::{Level, PatchProposal, STALE_AFTER_DAYS};
use vault_core::vocabulary::Vocabulary;

use crate::conflicts;
use crate::migrate::unified_diff;
use crate::model::Corpus;
use crate::validate;
use crate::whelk;

/// What to propose.
#[derive(Debug, Clone)]
pub struct Options {
    /// The subject: a page id or a `resource` IRI.
    pub subject: String,
    /// Content, schema or demotion.
    pub level: Level,
    /// Why the proposer believes the change is right.
    pub hypothesis: String,
    /// A file holding the proposed page, or `None` for a demotion.
    pub diff_source: Option<std::path::PathBuf>,
    /// The proposing actor.
    pub proposer: String,
    /// `now`, ISO-8601, so the whole thing stays deterministic under test.
    pub now: time::OffsetDateTime,
}

/// Locate the subject page by id, title or `resource` IRI.
#[must_use]
pub fn find_subject<'a>(vault: &'a Vault, corpus: &Corpus, subject: &str) -> Option<&'a Page> {
    if let Some(page) = vault.get(subject) {
        return Some(page);
    }
    let lower = subject.to_lowercase();
    if let Some(page) = vault
        .pages
        .iter()
        .find(|p| p.title().to_lowercase() == lower)
    {
        return Some(page);
    }
    let index = corpus.records.iter().position(|r| r.iri == subject)?;
    vault.pages.get(index)
}

/// Collect every automatic blocker for `subject`.
///
/// Three sources, all machine-decidable:
///
/// * the vocabulary and OKF validation errors on the page itself;
/// * `conflicts` at severity high (duplicate concepts, subclass cycles);
/// * Whelk's unsatisfiable classes.
#[must_use]
pub fn blockers(
    vault: &Vault,
    corpus: &Corpus,
    vocab: &Vocabulary,
    subject_id: &str,
) -> Vec<Blocker> {
    let mut out = Vec::new();

    let report = validate::validate(vault, corpus, vocab);
    for issue in report.errors() {
        if issue.path == subject_id {
            out.push(Blocker::new(issue.code.clone(), issue.message.clone()));
        }
    }

    let conflict_report = conflicts::analyse(corpus, "high");
    let subject_iri = corpus
        .records
        .iter()
        .find(|r| r.page_id == subject_id)
        .map(|r| r.iri.clone())
        .unwrap_or_default();
    for conflict in conflict_report.blocking() {
        if conflict.subjects.contains(&subject_iri) {
            out.push(conflict.to_blocker());
        }
    }

    let graph = crate::build::turtle::build_graph(corpus, vocab, false);
    let reasoning = whelk::reason(&graph);
    if !reasoning.is_consistent() {
        let http = crate::build::turtle::iri_to_uri(&subject_iri);
        for class in &reasoning.unsatisfiable {
            if *class == http || *class == subject_iri {
                out.push(Blocker::new(
                    "WHELK_INCONSISTENT",
                    format!("{class} is subsumed by owl:Nothing"),
                ));
            }
        }
    }

    out
}

/// Build the proposal.
///
/// # Errors
/// When the subject is not a page, or the `--diff` file cannot be read.
pub fn build(
    vault: &Vault,
    corpus: &Corpus,
    vocab: &Vocabulary,
    generation: &str,
    options: &Options,
) -> anyhow::Result<PatchProposal> {
    let page = find_subject(vault, corpus, &options.subject).ok_or_else(|| {
        anyhow::anyhow!("no page, title or resource matching `{}`", options.subject)
    })?;
    let record = corpus
        .records
        .iter()
        .find(|r| r.page_id == page.id)
        .ok_or_else(|| anyhow::anyhow!("page `{}` has no corpus record", page.id))?;

    // A `--diff` that is a DIRECTORY is a manifest: one proposed page per file,
    // named by page id. That is how a decision spanning 513 pages becomes one
    // proposal instead of 513.
    let manifest = match &options.diff_source {
        Some(path) if path.is_dir() => Some(read_manifest(vault, path)?),
        _ => None,
    };

    let (diff, mut pages) = if let Some(changes) = manifest {
        let mut diff = String::new();
        let mut pages = Vec::with_capacity(changes.len());
        for (id, before, after) in &changes {
            let part = unified_diff(id, before, after);
            if part.trim().is_empty() {
                continue; // a manifest entry identical to the corpus is a no-op
            }
            diff.push_str(&part);
            pages.push(id.clone());
        }
        (diff, pages)
    } else {
        let before = page.render()?;
        let after = match (&options.diff_source, options.level) {
            (Some(path), _) => read_proposed(path)?,
            (None, Level::Demotion) => demote(page)?,
            (None, _) => {
                anyhow::bail!(
                    "--diff <file|dir> is required for a {} proposal",
                    options.level.as_str()
                )
            }
        };
        (
            unified_diff(&page.id, &before, &after),
            vec![page.id.clone()],
        )
    };
    anyhow::ensure!(!diff.trim().is_empty(), "the proposal changes nothing");
    pages.sort();
    pages.dedup();

    let stale_after = (options.now + time::Duration::days(STALE_AFTER_DAYS))
        .format(&time::format_description::well_known::Rfc3339)?;

    // Blockers are evaluated for EVERY page the proposal touches, not just the
    // subject. A grouped proposal is one decision, so one blocked page blocks the
    // whole thing — signing 512 good changes to get one bad one through is
    // exactly what the gate exists to prevent.
    let mut all_blockers = Vec::new();
    for id in &pages {
        for blocker in blockers(vault, corpus, vocab, id) {
            if !all_blockers.contains(&blocker) {
                all_blockers.push(blocker);
            }
        }
    }

    let proposal = PatchProposal::new(
        options.level,
        record.iri.clone(),
        page.id.clone(),
        options.hypothesis.clone(),
        diff,
        all_blockers,
        options.proposer.clone(),
        generation,
        stale_after,
    );
    Ok(if pages.len() > 1 {
        proposal.with_pages(pages)
    } else {
        proposal
    })
}

/// Read a proposal manifest directory: `(page id, current text, proposed text)`
/// for every `*.md` in it, in page-id order.
///
/// A file's **page id** is its path relative to the manifest root without the
/// `.md`, which is the same identity the vault uses — so a manifest mirrors the
/// `pages/` layout and can be produced by copying and editing a subtree.
///
/// A file naming a page the vault does not hold is an error, not a silent skip:
/// a manifest is a claim about existing pages, and a typo in one of 513 entries
/// would otherwise be invisible.
fn read_manifest(vault: &Vault, root: &Path) -> anyhow::Result<Vec<(String, String, String)>> {
    let mut out = Vec::new();
    let files = vault_core::page::walk_pages(root)
        .map_err(|e| anyhow::anyhow!("reading the manifest {}: {e}", root.display()))?;
    for path in files.files {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
        let page = vault
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("the manifest names `{id}`, which is not a page"))?;
        let after = read_proposed(&path)?;
        out.push((id, page.render()?, after));
    }
    anyhow::ensure!(
        !out.is_empty(),
        "the manifest {} holds no .md files",
        root.display()
    );
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn read_proposed(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))
}

/// The demotion form: the same page with `status: deprecated`.
fn demote(page: &Page) -> anyhow::Result<String> {
    let mut next = page.clone();
    next.frontmatter
        .set("status", serde_yaml::Value::String("deprecated".to_owned()));
    Ok(next.render()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest directory: one proposed page per file, named by page id.
    fn manifest(pages: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manifest");
        std::fs::create_dir_all(&root).unwrap();
        for (id, text) in pages {
            let path = root.join(format!("{id}.md"));
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }
    use vault_core::page::VaultKind;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
scalars:
  quality: { type: number, min: 0, max: 1 }
"#,
        )
        .unwrap()
    }

    fn vault_of(pages: Vec<(&str, &str)>) -> Vault {
        Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages: pages
                .into_iter()
                .map(|(id, text)| {
                    Page::parse(
                        format!("/v/pages/{id}.md"),
                        format!("pages/{id}.md"),
                        id,
                        text,
                    )
                    .unwrap()
                })
                .collect(),
        }
    }

    const GOOD: &str = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:a\nstatus: stable\nquality: 0.35\n---\nbody\n";

    fn options(level: Level, diff_source: Option<std::path::PathBuf>) -> Options {
        Options {
            subject: "A".into(),
            level,
            hypothesis: "quality is understated".into(),
            diff_source,
            proposer: "process:vault/1.0".into(),
            now: time::OffsetDateTime::from_unix_timestamp(1_790_000_000).unwrap(),
        }
    }

    fn proposed_file(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proposed.md");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn a_manifest_directory_becomes_one_grouped_proposal() {
        // The 513-page `domain: ai` merge and the 125-journal base-copy choice
        // are single decisions. Filing them page by page would put a
        // schema-shaped judgement through the content tier 638 times.
        let a = GOOD;
        let b = GOOD.replace("urn:ngm:class:a", "urn:ngm:class:b");
        let c = GOOD.replace("urn:ngm:class:a", "urn:ngm:class:c");
        let vault = vault_of(vec![("A", a), ("B", &b), ("C", &c)]);
        let corpus = Corpus::build(&vault, &vocab());

        // Three proposed pages, one of them unchanged from the corpus.
        let (_dir, root) = manifest(&[
            ("A", &a.replace("0.35", "0.55")),
            ("B", &b.replace("0.35", "0.65")),
            ("C", &c),
        ]);
        let p = build(
            &vault,
            &corpus,
            &vocab(),
            "visionGraph@abc",
            &options(Level::Content, Some(root)),
        )
        .expect("a manifest builds");

        assert!(p.is_grouped());
        // `C` changed nothing, so it is not part of the proposal.
        assert_eq!(p.pages, vec!["A".to_owned(), "B".to_owned()]);
        // The diff is multi-file and names both pages.
        assert!(p.diff.contains("a/A"), "{}", p.diff);
        assert!(p.diff.contains("a/B"), "{}", p.diff);
        assert!(!p.diff.contains("a/C"), "{}", p.diff);
        // The subject still names the case.
        assert_eq!(p.page, "A");
    }

    #[test]
    fn a_grouped_proposal_hashes_differently_from_the_single_page_one() {
        // Two different decisions must not share a case id. Equally, a
        // single-page proposal's digest must be exactly what it was before
        // grouping existed, or every existing case id moves.
        let a = GOOD;
        let b = GOOD.replace("urn:ngm:class:a", "urn:ngm:class:b");
        let vault = vault_of(vec![("A", a), ("B", &b)]);
        let corpus = Corpus::build(&vault, &vocab());

        let (_d1, single) = proposed_file(&a.replace("0.35", "0.55"));
        let one = build(
            &vault,
            &corpus,
            &vocab(),
            "visionGraph@abc",
            &options(Level::Content, Some(single)),
        )
        .unwrap();
        assert!(!one.is_grouped());
        assert_eq!(one.pages, vec!["A".to_owned()]);

        let (_d2, root) = manifest(&[
            ("A", &a.replace("0.35", "0.55")),
            ("B", &b.replace("0.35", "0.65")),
        ]);
        let many = build(
            &vault,
            &corpus,
            &vocab(),
            "visionGraph@abc",
            &options(Level::Content, Some(root)),
        )
        .unwrap();
        assert_ne!(
            one.digest, many.digest,
            "different decisions, different cases"
        );

        // And the single-page digest is the pre-grouping one.
        assert_eq!(
            one.digest,
            PatchProposal::compute_digest(
                Level::Content,
                &one.iri,
                &one.page,
                &one.hypothesis,
                &one.diff
            )
        );
    }

    #[test]
    fn a_manifest_naming_an_unknown_page_is_refused() {
        // A typo in one of 513 entries would otherwise be invisible.
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, root) = manifest(&[("A", &GOOD.replace("0.35", "0.55")), ("Typo", GOOD)]);
        let error = build(
            &vault,
            &corpus,
            &vocab(),
            "visionGraph@abc",
            &options(Level::Content, Some(root)),
        )
        .expect_err("an unknown page must refuse the manifest");
        assert!(error.to_string().contains("Typo"), "{error}");
    }

    #[test]
    fn one_blocked_page_blocks_the_whole_grouped_proposal() {
        // A grouped proposal is ONE decision. Signing 512 good changes to carry
        // one bad one through is what the gate exists to prevent.
        let a = GOOD;
        // An undeclared frontmatter key is an UNKNOWN_KEY error in `knowledge/`.
        let bad = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:bad\n\
                   status: stable\nnot-a-declared-key: x\n---\nbody\n";
        let vault = vault_of(vec![("A", a), ("Bad", bad)]);
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, root) = manifest(&[
            ("A", &a.replace("0.35", "0.55")),
            ("Bad", &bad.replace("body", "changed body")),
        ]);
        let p = build(
            &vault,
            &corpus,
            &vocab(),
            "visionGraph@abc",
            &options(Level::Content, Some(root)),
        )
        .expect("it builds, and reports the refusal");
        assert!(p.is_grouped());
        assert!(
            !p.is_postable(),
            "a blocked member must block the group: {:?}",
            p.blockers
        );
    }

    #[test]
    fn builds_a_postable_content_proposal() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, path) = proposed_file(&GOOD.replace("0.35", "0.55"));
        let p = build(
            &vault,
            &corpus,
            &vocab(),
            "visionGraph@abc",
            &options(Level::Content, Some(path)),
        )
        .unwrap();
        assert!(p.is_postable());
        assert_eq!(p.iri, "urn:ngm:class:a");
        assert_eq!(p.page, "A");
        assert!(p.diff.contains("-quality: 0.35"));
        assert!(p.diff.contains("+quality: 0.55"));
        assert_eq!(p.generation, "visionGraph@abc");
    }

    #[test]
    fn the_expiry_is_fourteen_days_out() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, path) = proposed_file(&GOOD.replace("0.35", "0.55"));
        let p = build(
            &vault,
            &corpus,
            &vocab(),
            "g",
            &options(Level::Content, Some(path)),
        )
        .unwrap();
        let expiry = time::OffsetDateTime::parse(
            &p.stale_after,
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        assert_eq!(expiry.unix_timestamp(), 1_790_000_000 + 14 * 86_400);
    }

    #[test]
    fn a_demotion_needs_no_diff_file() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let p = build(
            &vault,
            &corpus,
            &vocab(),
            "g",
            &options(Level::Demotion, None),
        )
        .unwrap();
        assert!(p.diff.contains("+status: deprecated"));
    }

    #[test]
    fn a_content_proposal_without_a_diff_file_is_an_error() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let err = build(
            &vault,
            &corpus,
            &vocab(),
            "g",
            &options(Level::Content, None),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("--diff"), "{err}");
    }

    #[test]
    fn a_proposal_that_changes_nothing_is_an_error() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, path) = proposed_file(GOOD);
        let err = build(
            &vault,
            &corpus,
            &vocab(),
            "g",
            &options(Level::Content, Some(path)),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("changes nothing"), "{err}");
    }

    #[test]
    fn a_subclass_cycle_blocks_the_subject() {
        let a = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:a\nstatus: stable\nis-a: [\"[[B]]\"]\n---\n";
        let b = "---\ntype: Class\npublic: true\nresource: urn:ngm:class:b\nstatus: stable\nis-a: [\"[[A]]\"]\n---\n";
        let vault = vault_of(vec![("A", a), ("B", b)]);
        let corpus = Corpus::build(&vault, &vocab());
        let found = blockers(&vault, &corpus, &vocab(), "A");
        assert!(
            found.iter().any(|b| b.code == "SUBCLASS_CYCLE"),
            "{found:?}"
        );
    }

    #[test]
    fn a_validation_error_on_the_subject_blocks_it() {
        let broken = "---\ntype: Class\npublic: true\n---\n";
        let vault = vault_of(vec![("A", broken)]);
        let corpus = Corpus::build(&vault, &vocab());
        let found = blockers(&vault, &corpus, &vocab(), "A");
        assert!(!found.is_empty());
    }

    #[test]
    fn a_clean_subject_has_no_blockers() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        assert!(blockers(&vault, &corpus, &vocab(), "A").is_empty());
    }

    #[test]
    fn the_subject_resolves_by_id_title_or_iri() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        for subject in ["A", "a", "urn:ngm:class:a"] {
            assert!(
                find_subject(&vault, &corpus, subject).is_some(),
                "{subject}"
            );
        }
        assert!(find_subject(&vault, &corpus, "nope").is_none());
    }

    #[test]
    fn an_unknown_subject_is_an_error() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let mut opts = options(Level::Demotion, None);
        opts.subject = "Nowhere".into();
        assert!(build(&vault, &corpus, &vocab(), "g", &opts).is_err());
    }
}
