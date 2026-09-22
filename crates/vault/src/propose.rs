//! `vault propose` — build a [`PatchProposal`], run the blockers, and (unless
//! `--dry-run`) post the 31402.
//!
//! The order is the point. Whelk and the conflict detector run **before**
//! anything is published, and everything they find becomes a blocker. A
//! proposal with blockers is still *emitted* — so the refusal is auditable and
//! the agent can see what to fix — but it is never posted, and no human is ever
//! asked to wave it through (decision Q6).
//!
//! # Blockers are deltas
//!
//! The proposed pages are applied to an in-memory copy of the corpus, and the
//! corpus is assessed **once before and once after** — validation, the
//! conflict detector and Whelk, each a single whole-corpus pass however many
//! pages the proposal touches. A blocker is a finding present after the change
//! and absent before it: a proposal is answerable for what it introduces, not
//! for a cycle that was already there. Findings that pre-date the change and
//! involve a touched page are reported in `preexisting`, informationally.

use std::collections::HashSet;
use std::path::Path;

use vault_core::page::{Page, Vault};
use vault_core::promotion::Blocker;
use vault_core::proposal::{Level, PatchProposal, ProposalKind, STALE_AFTER_DAYS};
use vault_core::vocabulary::Vocabulary;

use crate::conflicts;
use crate::migrate::unified_diff;
use crate::model::Corpus;
use crate::validate;
use crate::whelk;

/// What to propose.
#[derive(Debug, Clone)]
pub struct Options {
    /// The subject: a page id or a `resource` IRI. For a grouped proposal
    /// with no [`Options::title`], the text the proposal's own IRI is minted
    /// from.
    pub subject: String,
    /// A grouped proposal's title: its subject IRI is
    /// `urn:ngm:proposal:<slug of title>`. Ignored for a single-page proposal.
    pub title: Option<String>,
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

/// Every machine-decidable finding on one corpus state: validation errors
/// (with the page they sit on), blocking conflicts (with the IRIs they
/// involve) and Whelk's unsatisfiable classes.
#[derive(Debug, Default)]
pub struct Assessment {
    /// `(page id, blocker)` for every validation error.
    pub validation: Vec<(String, Blocker)>,
    /// `(subject IRIs, blocker)` for every conflict at severity high.
    pub conflicts: Vec<(Vec<String>, Blocker)>,
    /// Classes subsumed by `owl:Nothing`.
    pub unsatisfiable: Vec<String>,
}

impl Assessment {
    /// Assess a whole corpus: one validation pass, one conflict scan, one
    /// Whelk run.
    #[must_use]
    pub fn of(vault: &Vault, corpus: &Corpus, vocab: &Vocabulary) -> Self {
        let validation = validate::validate(vault, corpus, vocab)
            .errors()
            .into_iter()
            .map(|issue| {
                (
                    issue.path.clone(),
                    Blocker::new(issue.code.clone(), issue.message.clone()),
                )
            })
            .collect();
        let conflicts = conflicts::analyse(corpus, "high")
            .blocking()
            .into_iter()
            .map(|c| (c.subjects.clone(), c.to_blocker()))
            .collect();
        let graph = crate::build::turtle::build_graph(corpus, vocab, false);
        let unsatisfiable = whelk::reason(&graph).unsatisfiable;
        Self {
            validation,
            conflicts,
            unsatisfiable,
        }
    }
}

fn whelk_blocker(class: &str) -> Blocker {
    Blocker::new(
        "WHELK_INCONSISTENT",
        format!("{class} is subsumed by owl:Nothing"),
    )
}

fn push_unique(out: &mut Vec<Blocker>, blocker: Blocker) {
    if !out.contains(&blocker) {
        out.push(blocker);
    }
}

/// Split the `after` findings into `(blockers, preexisting)` against `before`.
///
/// * A **blocker** is any finding in `after` that `before` lacks. Validation
///   errors count only on the touched pages; a new conflict or a newly
///   unsatisfiable class counts wherever it appears, since the change is the
///   only thing that differs.
/// * **Pre-existing** findings are those in both states that involve a
///   touched page (by page id, or by the IRI in either state).
#[must_use]
fn delta(
    before: &Assessment,
    after: &Assessment,
    touched_pages: &HashSet<String>,
    touched_iris: &HashSet<String>,
) -> (Vec<Blocker>, Vec<Blocker>) {
    let mut blockers = Vec::new();
    let mut preexisting = Vec::new();

    let old_validation: HashSet<(&str, &Blocker)> = before
        .validation
        .iter()
        .map(|(path, b)| (path.as_str(), b))
        .collect();
    for (path, blocker) in &after.validation {
        if !touched_pages.contains(path) {
            continue;
        }
        if old_validation.contains(&(path.as_str(), blocker)) {
            push_unique(&mut preexisting, blocker.clone());
        } else {
            push_unique(&mut blockers, blocker.clone());
        }
    }

    let old_conflicts: HashSet<&Blocker> = before.conflicts.iter().map(|(_, b)| b).collect();
    for (subjects, blocker) in &after.conflicts {
        if !old_conflicts.contains(blocker) {
            push_unique(&mut blockers, blocker.clone());
        } else if subjects.iter().any(|s| touched_iris.contains(s)) {
            push_unique(&mut preexisting, blocker.clone());
        }
    }

    let old_unsat: HashSet<&str> = before.unsatisfiable.iter().map(String::as_str).collect();
    for class in &after.unsatisfiable {
        if !old_unsat.contains(class.as_str()) {
            push_unique(&mut blockers, whelk_blocker(class));
        } else if touched_iris.contains(class) {
            push_unique(&mut preexisting, whelk_blocker(class));
        }
    }

    (blockers, preexisting)
}

/// One page the proposal touches.
#[derive(Debug, Clone)]
struct PageChange {
    /// The page id.
    id: String,
    /// The page as the corpus holds it, or `None` for a page being created.
    before: Option<String>,
    /// The proposed page in full.
    after: String,
}

impl PageChange {
    fn is_creation(&self) -> bool {
        self.before.is_none()
    }

    /// This page's part of the unified diff. A creation is diffed against
    /// `/dev/null`, the form `git apply` and every reviewer already read.
    fn diff(&self) -> String {
        match &self.before {
            Some(before) => unified_diff(&self.id, before, &self.after),
            None => similar::TextDiff::from_lines("", self.after.as_str())
                .unified_diff()
                .header("/dev/null", &format!("b/{}", self.id))
                .to_string(),
        }
    }
}

/// `vault` with every change applied: an amendment substitutes the page of
/// that id, a creation adds a page at `pages/<id>.md`. A proposed page whose
/// frontmatter does not parse cannot be applied; it is returned as a blocker
/// instead, and the base page (or no page) stays in place.
fn apply(vault: &Vault, changes: &[PageChange]) -> (Vault, Vec<Blocker>) {
    let mut patched = vault.clone();
    let mut unparseable = Vec::new();
    for change in changes {
        let id = &change.id;
        let (path, rel_path) = match patched.get(id) {
            Some(slot) => (slot.path.clone(), slot.rel_path.clone()),
            None if change.is_creation() => (
                vault.root.join("pages").join(format!("{id}.md")),
                std::path::PathBuf::from("pages").join(format!("{id}.md")),
            ),
            None => continue,
        };
        match Page::parse(path, rel_path, id.clone(), &change.after) {
            Ok(page) => match patched.get_mut(id) {
                Some(slot) => *slot = page,
                None => patched.pages.push(page),
            },
            Err(e) => unparseable.push(Blocker::new(
                "FRONTMATTER_INVALID",
                format!("the proposed {id} does not parse: {e}"),
            )),
        }
    }
    (patched, unparseable)
}

/// `true` when `id` can be a knowledge page's filename: one path component,
/// not hidden, no control characters.
///
/// A `/` in a title once created phantom directories (`/pages/A/B-Testing`);
/// knowledge pages are flat, so a creation may not introduce one.
#[must_use]
pub fn is_valid_page_filename(id: &str) -> bool {
    !id.trim().is_empty()
        && id.trim() == id
        && !id.starts_with('.')
        && !id.contains(['/', '\\'])
        && !id.chars().any(char::is_control)
}

/// Everything that stops `page` — a page absent from `vault` — from being
/// created, beyond what validation reports.
///
/// * **`FILENAME_INVALID`** — the id cannot be a flat knowledge filename.
/// * **`FILENAME_COLLISION`** — an existing page's id differs only in case,
///   which is the same file on a case-insensitive checkout and the same
///   published URL on a case-folding host.
/// * **`IRI_COLLISION`** — the page's `resource` is an existing page's.
/// * **`SLUG_COLLISION`** — the page's publish slug is an existing page's, or
///   adding it would re-key an existing page's slug. This is the build's own
///   rule: [`Corpus::build`] gives each public record a unique slug by
///   re-keying all but one claimant, so a clash is detected by comparing each
///   existing record's slug before and after the page is added, plus the new
///   page's own derived slug against every existing one.
///
/// `before` is `vault`'s corpus; `after` is the corpus with the page added.
#[must_use]
pub fn creation_blockers(
    vault: &Vault,
    before: &Corpus,
    after: &Corpus,
    vocab: &Vocabulary,
    page: &Page,
) -> Vec<Blocker> {
    let mut out = Vec::new();
    let id = page.id.as_str();
    if !is_valid_page_filename(id) {
        out.push(Blocker::new(
            "FILENAME_INVALID",
            format!("`{id}` cannot be a knowledge page filename (one flat, visible name)"),
        ));
    }
    let lower = id.to_lowercase();
    if let Some(existing) = vault.pages.iter().find(|p| p.id.to_lowercase() == lower) {
        out.push(Blocker::new(
            "FILENAME_COLLISION",
            format!("`{id}` collides with the existing page `{}`", existing.id),
        ));
    }
    let iri = page.resource(&vocab.namespace);
    if let Some(existing) = before.records.iter().find(|r| r.iri == iri) {
        out.push(Blocker::new(
            "IRI_COLLISION",
            format!("`{iri}` is already the resource of `{}`", existing.page_id),
        ));
    }
    let slug = page.slug();
    let mut slug_clash: Vec<&str> = before
        .records
        .iter()
        .filter(|r| r.slug == slug)
        .map(|r| r.page_id.as_str())
        .collect();
    for (old, new) in before.records.iter().zip(&after.records) {
        if old.page_id == new.page_id && old.slug != new.slug {
            slug_clash.push(old.page_id.as_str());
        }
    }
    slug_clash.sort_unstable();
    slug_clash.dedup();
    if !slug_clash.is_empty() {
        out.push(Blocker::new(
            "SLUG_COLLISION",
            format!(
                "the publish slug `{slug}` is already claimed by {}",
                slug_clash
                    .iter()
                    .map(|p| format!("`{p}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    out
}

/// `true` when the page declares a key that changes the schema rather than
/// using it: a key the vocabulary does not declare, or a provisional relation
/// (one the emitter discards until a schema-level proposal adopts it).
fn declares_schema_key(page: &Page, vocab: &Vocabulary) -> bool {
    page.frontmatter.keys().any(|key| {
        !vocab.is_known_key(key)
            || vocab
                .relations
                .get(key)
                .is_some_and(|r| r.status == vault_core::vocabulary::RelationStatus::Provisional)
    })
}

/// The `title` a staged page declares, when its frontmatter parses and has
/// one. A creation is always explicit: the staged page names itself.
fn declared_title(text: &str) -> Option<String> {
    let page = Page::parse("staged.md", "staged.md", "staged", text).ok()?;
    page.frontmatter
        .text("title")
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

/// The single-file form: the subject names an existing page (an amendment),
/// or the staged page declares a `title` no page has (a creation).
///
/// Resolution order keeps every pre-existing amendment an amendment: a subject
/// matching a page by id or title is always that page. Only then is the staged
/// title consulted, so a proposal whose subject is a *new* IRI — or an IRI an
/// existing page already holds — and whose staged page is titled afresh is a
/// creation, and the IRI clash becomes a blocker rather than a silent rewrite of
/// the other page.
fn single_change(
    vault: &Vault,
    corpus: &Corpus,
    vocab: &Vocabulary,
    options: &Options,
) -> anyhow::Result<PageChange> {
    let by_name = vault.get(&options.subject).or_else(|| {
        let lower = options.subject.to_lowercase();
        vault
            .pages
            .iter()
            .find(|p| p.title().to_lowercase() == lower)
    });
    let staged = match &options.diff_source {
        Some(path) => Some(read_proposed(path)?),
        None => None,
    };
    if by_name.is_none() {
        if let Some(text) = &staged {
            if let Some(title) = declared_title(text) {
                if vault.get(&title).is_none() {
                    anyhow::ensure!(
                        options.level != Level::Demotion,
                        "`{title}` is not a page; a demotion cannot create one"
                    );
                    let resource = Page::parse("staged.md", "staged.md", &title, text)
                        .map(|p| p.resource(&vocab.namespace))
                        .unwrap_or_default();
                    anyhow::ensure!(
                        options.subject == title
                            || options.subject.to_lowercase() == title.to_lowercase()
                            || options.subject == resource,
                        "the subject `{}` names neither an existing page nor the staged \
                         page (title `{title}`, resource `{resource}`)",
                        options.subject
                    );
                    return Ok(PageChange {
                        id: title,
                        before: None,
                        after: text.clone(),
                    });
                }
            }
        }
    }
    let page = by_name
        .or_else(|| find_subject(vault, corpus, &options.subject))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no page, title or resource matching `{}`; to create a page, the \
                 --diff file must declare its new `title`",
                options.subject
            )
        })?;
    let after = match (staged, options.level) {
        (Some(text), _) => text,
        (None, Level::Demotion) => demote(page)?,
        (None, _) => {
            anyhow::bail!(
                "--diff <file|dir> is required for a {} proposal",
                options.level.as_str()
            )
        }
    };
    Ok(PageChange {
        id: page.id.clone(),
        before: Some(page.render()?),
        after,
    })
}

/// Build the proposal.
///
/// # Errors
/// When the subject is neither a page nor a staged creation, the `--diff`
/// source cannot be read, or the proposal changes nothing.
pub fn build(
    vault: &Vault,
    corpus: &Corpus,
    vocab: &Vocabulary,
    generation: &str,
    options: &Options,
) -> anyhow::Result<PatchProposal> {
    // A `--diff` that is a DIRECTORY is a manifest: one proposed page per file,
    // named by page id. That is how a decision spanning 513 pages becomes one
    // proposal instead of 513.
    let changes = match &options.diff_source {
        Some(path) if path.is_dir() => read_manifest(vault, path)?,
        _ => vec![single_change(vault, corpus, vocab, options)?],
    };

    // A manifest entry identical to the corpus is a no-op and is dropped.
    let mut diff = String::new();
    let mut effective = Vec::with_capacity(changes.len());
    for change in changes {
        let part = change.diff();
        if part.trim().is_empty() {
            continue;
        }
        diff.push_str(&part);
        effective.push(change);
    }
    anyhow::ensure!(!effective.is_empty(), "the proposal changes nothing");
    let pages: Vec<String> = effective.iter().map(|c| c.id.clone()).collect();

    // One assessment of the base corpus and one of the patched corpus, however
    // many pages the proposal touches.
    let (patched, unparseable) = apply(vault, &effective);
    let patched_corpus = Corpus::build(&patched, vocab);
    let before = Assessment::of(vault, corpus, vocab);
    let after = Assessment::of(&patched, &patched_corpus, vocab);

    let touched_pages: HashSet<String> = pages.iter().cloned().collect();
    let touched_iris: HashSet<String> = corpus
        .records
        .iter()
        .chain(&patched_corpus.records)
        .filter(|r| touched_pages.contains(&r.page_id) && !r.iri.is_empty())
        .flat_map(|r| [r.iri.clone(), crate::build::turtle::iri_to_uri(&r.iri)])
        .collect();
    let (mut blockers, preexisting) = delta(&before, &after, &touched_pages, &touched_iris);
    for blocker in unparseable {
        push_unique(&mut blockers, blocker);
    }

    // Creations answer to more than the delta: a new page must not take an
    // existing page's file, IRI or published URL, and one that brings a new
    // key into the corpus is a schema decision whatever it was filed as.
    let mut level = options.level;
    let created: Vec<&Page> = effective
        .iter()
        .filter(|c| c.is_creation())
        .filter_map(|c| patched.get(&c.id))
        .collect();
    for page in &created {
        for blocker in creation_blockers(vault, corpus, &patched_corpus, vocab, page) {
            push_unique(&mut blockers, blocker);
        }
        if declares_schema_key(page, vocab) {
            level = Level::Schema;
        }
    }
    let kind = if effective.iter().any(PageChange::is_creation) {
        ProposalKind::Create
    } else {
        ProposalKind::Amend
    };

    let stale_after = (options.now + time::Duration::days(STALE_AFTER_DAYS))
        .format(&time::format_description::well_known::Rfc3339)?;

    // A grouped proposal is one decision, so one blocked page blocks the whole
    // thing — signing 512 good changes to get one bad one through is exactly
    // what the gate exists to prevent. Its subject is the decision itself.
    let proposal = if pages.len() > 1 {
        let title = options.title.as_deref().unwrap_or(&options.subject);
        anyhow::ensure!(
            !vault_core::slug::slugify(title).is_empty(),
            "a grouped proposal needs a --title to mint its subject IRI"
        );
        PatchProposal::grouped(
            level,
            title,
            options.hypothesis.clone(),
            diff,
            blockers,
            options.proposer.clone(),
            generation,
            stale_after,
            pages,
        )
    } else {
        let id = &pages[0];
        // A creation's IRI is the staged page's `resource`, read from the
        // patched corpus; an amendment's is the page's current one.
        let source = if kind == ProposalKind::Create {
            &patched_corpus
        } else {
            corpus
        };
        let iri = source
            .records
            .iter()
            .find(|r| r.page_id == *id)
            .map(|r| r.iri.clone())
            .or_else(|| {
                // An unparseable staged page has no record; name it by the
                // subject so the blocked proposal is still addressable.
                (kind == ProposalKind::Create).then(|| options.subject.clone())
            })
            .ok_or_else(|| anyhow::anyhow!("page `{id}` has no corpus record"))?;
        PatchProposal::new(
            level,
            iri,
            id.clone(),
            options.hypothesis.clone(),
            diff,
            blockers,
            options.proposer.clone(),
            generation,
            stale_after,
        )
    };
    Ok(proposal.with_kind(kind).with_preexisting(preexisting))
}

/// Read a proposal manifest directory: one [`PageChange`] for every `*.md` in
/// it, in page-id order.
///
/// A file's **page id** is its path relative to the manifest root without the
/// `.md`, which is the same identity the vault uses — so a manifest mirrors the
/// `pages/` layout and can be produced by copying and editing a subtree.
///
/// A file naming a page the vault does not hold is a **creation** only when
/// it says so — its frontmatter declares `title: <that id>`. Anything else is
/// an error, not a silent skip or an accidental creation: a manifest is
/// otherwise a claim about existing pages, and a typo in one of 513 entries
/// would be invisible.
fn read_manifest(vault: &Vault, root: &Path) -> anyhow::Result<Vec<PageChange>> {
    let mut out = Vec::new();
    let files = vault_core::page::walk_pages(root)
        .map_err(|e| anyhow::anyhow!("reading the manifest {}: {e}", root.display()))?;
    for path in files.files {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
        let after = read_proposed(&path)?;
        let before = match vault.get(&id) {
            Some(page) => Some(page.render()?),
            None if declared_title(&after).as_deref() == Some(id.as_str()) => None,
            None => anyhow::bail!(
                "the manifest names `{id}`, which is not a page (a new page must \
                 declare `title: {id}`)"
            ),
        };
        out.push(PageChange { id, before, after });
    }
    anyhow::ensure!(
        !out.is_empty(),
        "the manifest {} holds no .md files",
        root.display()
    );
    out.sort_by(|a, b| a.id.cmp(&b.id));
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
            title: None,
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
        // The subject is the decision itself, not the first page.
        assert_eq!(p.iri, "urn:ngm:proposal:a");
        assert_eq!(p.page, p.iri);

        let (_titled_dir, titled_root) = p_root(a, &b);
        let mut titled = options(Level::Content, Some(titled_root));
        titled.title = Some("Raise the quality floor".into());
        let titled_proposal = build(&vault, &corpus, &vocab(), "visionGraph@abc", &titled).unwrap();
        assert_eq!(
            titled_proposal.iri,
            "urn:ngm:proposal:raise-the-quality-floor"
        );
    }

    /// A two-page manifest raising both qualities; the guard keeps it alive.
    fn p_root(a: &str, b: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let (dir, root) = manifest(&[
            ("A", &a.replace("0.35", "0.55")),
            ("B", &b.replace("0.35", "0.65")),
        ]);
        (dir, root)
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
        let b = GOOD.replace("urn:ngm:class:a", "urn:ngm:class:b");
        let vault = vault_of(vec![("A", a), ("B", &b)]);
        let corpus = Corpus::build(&vault, &vocab());
        // An undeclared frontmatter key is an UNKNOWN_KEY error in `knowledge/`,
        // introduced here by the proposed B.
        let (_dir, root) = manifest(&[
            ("A", &a.replace("0.35", "0.55")),
            ("B", &b.replace("quality: 0.35", "not-a-declared-key: x")),
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

    fn class(id: &str, extra: &str) -> String {
        format!(
            "---\ntype: Class\npublic: true\nresource: urn:ngm:class:{}\nstatus: stable\nquality: 0.35\n{extra}---\nbody\n",
            id.to_lowercase()
        )
    }

    fn propose_one(vault: &Vault, text: &str) -> PatchProposal {
        let corpus = Corpus::build(vault, &vocab());
        let (_dir, path) = proposed_file(text);
        build(
            vault,
            &corpus,
            &vocab(),
            "g",
            &options(Level::Content, Some(path)),
        )
        .unwrap()
    }

    #[test]
    fn a_cycle_the_change_introduces_blocks_it() {
        let b = class("B", "is-a: [\"[[A]]\"]\n");
        let vault = vault_of(vec![("A", &class("A", "")), ("B", &b)]);
        let p = propose_one(&vault, &class("A", "is-a: [\"[[B]]\"]\n"));
        assert!(
            p.blockers.iter().any(|b| b.code == "SUBCLASS_CYCLE"),
            "{:?}",
            p.blockers
        );
        assert!(!p.is_postable());
    }

    #[test]
    fn a_cycle_that_already_existed_is_preexisting_not_a_blocker() {
        // A `quality` edit cannot introduce an is-a cycle; blaming it for one
        // would make every page on the cycle permanently unproposable.
        let a = class("A", "is-a: [\"[[B]]\"]\n");
        let b = class("B", "is-a: [\"[[A]]\"]\n");
        let vault = vault_of(vec![("A", &a), ("B", &b)]);
        let p = propose_one(&vault, &a.replace("0.35", "0.55"));
        assert!(p.is_postable(), "{:?}", p.blockers);
        assert!(
            p.preexisting.iter().any(|b| b.code == "SUBCLASS_CYCLE"),
            "{:?}",
            p.preexisting
        );
    }

    #[test]
    fn a_validation_error_the_change_introduces_blocks_it() {
        let vault = vault_of(vec![("A", GOOD)]);
        let p = propose_one(&vault, &GOOD.replace("status: stable\n", ""));
        assert!(!p.is_postable(), "dropping a required key is a new error");
        assert!(p.preexisting.is_empty());
    }

    #[test]
    fn a_validation_error_that_already_existed_does_not_block() {
        let broken = GOOD.replace("status: stable\n", "");
        let vault = vault_of(vec![("A", &broken)]);
        let p = propose_one(&vault, &broken.replace("0.35", "0.55"));
        assert!(p.is_postable(), "{:?}", p.blockers);
        assert!(!p.preexisting.is_empty());
    }

    #[test]
    fn an_unparseable_proposed_page_blocks() {
        let vault = vault_of(vec![("A", GOOD)]);
        let p = propose_one(&vault, "---\ntype: [unclosed\n---\nbody\n");
        assert!(
            p.blockers.iter().any(|b| b.code == "FRONTMATTER_INVALID"),
            "{:?}",
            p.blockers
        );
    }

    #[test]
    fn a_clean_change_has_no_blockers() {
        let vault = vault_of(vec![("A", GOOD)]);
        let p = propose_one(&vault, &GOOD.replace("0.35", "0.55"));
        assert!(p.blockers.is_empty() && p.preexisting.is_empty());
    }

    /// Grouped proposals assess the corpus twice in total — before and after
    /// — not twice per page. 513 pages took 767 s when it was per page.
    #[test]
    fn a_two_hundred_page_grouped_proposal_is_fast() {
        let texts: Vec<(String, String)> = (0..200)
            .map(|i| {
                let id = format!("P{i:03}");
                let parent = if i == 0 {
                    String::new()
                } else {
                    format!("is-a: [\"[[P{:03}]]\"]\n", i - 1)
                };
                let text = class(&id, &parent);
                (id, text)
            })
            .collect();
        let vault = vault_of(
            texts
                .iter()
                .map(|(id, t)| (id.as_str(), t.as_str()))
                .collect(),
        );
        let corpus = Corpus::build(&vault, &vocab());
        let proposed: Vec<(String, String)> = texts
            .iter()
            .map(|(id, t)| (id.clone(), t.replace("0.35", "0.55")))
            .collect();
        let refs: Vec<(&str, &str)> = proposed
            .iter()
            .map(|(id, t)| (id.as_str(), t.as_str()))
            .collect();
        let (_dir, root) = manifest(&refs);
        let mut opts = options(Level::Schema, Some(root));
        opts.title = Some("Raise every quality".into());

        let start = std::time::Instant::now();
        let p = build(&vault, &corpus, &vocab(), "g", &opts).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(p.pages.len(), 200);
        assert!(p.is_postable(), "{:?}", p.blockers);
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "200-page grouped proposal took {elapsed:?}"
        );
    }

    /// A staged elevation: a new Class under `A`.
    fn staged_new(extra: &str) -> String {
        format!(
            "---\ntype: Class\ntitle: New Thing\npublic: true\nresource: urn:ngm:class:new-thing\nstatus: draft\n{extra}---\nA new class.\n"
        )
    }

    fn propose_create(vault: &Vault, subject: &str, text: &str) -> PatchProposal {
        let corpus = Corpus::build(vault, &vocab());
        let (_dir, path) = proposed_file(text);
        let mut opts = options(Level::Content, Some(path));
        opts.subject = subject.into();
        build(vault, &corpus, &vocab(), "g", &opts).unwrap()
    }

    #[test]
    fn a_page_absent_from_the_vault_is_a_creation() {
        let vault = vault_of(vec![("A", GOOD)]);
        let p = propose_create(
            &vault,
            "urn:ngm:class:new-thing",
            &staged_new("is-a:\n- '[[A]]'\n"),
        );
        assert_eq!(p.kind, ProposalKind::Create);
        assert!(p.is_postable(), "{:?}", p.blockers);
        assert_eq!(p.iri, "urn:ngm:class:new-thing");
        assert_eq!(p.page, "New Thing");
        assert_eq!(p.pages, vec!["New Thing".to_owned()]);
        assert_eq!(p.level, Level::Content);
        assert!(
            p.diff.starts_with("--- /dev/null\n+++ b/New Thing\n"),
            "{}",
            p.diff
        );
        assert!(p.diff.contains("+title: New Thing"), "{}", p.diff);
        // The subject may equally be the staged title.
        let by_title = propose_create(&vault, "New Thing", &staged_new("is-a:\n- '[[A]]'\n"));
        assert_eq!(by_title.digest, p.digest);
        // A creation never shares a case id with an amendment.
        assert_ne!(
            p.digest,
            PatchProposal::compute_digest(p.level, &p.iri, &p.page, &p.hypothesis, &p.diff)
        );
    }

    #[test]
    fn an_amendment_is_kind_amend_with_its_old_digest() {
        let vault = vault_of(vec![("A", GOOD)]);
        let p = propose_one(&vault, &GOOD.replace("0.35", "0.55"));
        assert_eq!(p.kind, ProposalKind::Amend);
        assert_eq!(
            p.digest,
            PatchProposal::compute_digest(p.level, &p.iri, &p.page, &p.hypothesis, &p.diff)
        );
        assert_eq!(serde_json::to_value(&p).unwrap()["kind"], "amend");
    }

    #[test]
    fn a_creation_taking_an_existing_iri_is_blocked_not_an_amendment() {
        // The subject resolves to A by IRI, but the staged page titles itself
        // afresh: that is a new page claiming A's identity, and must never
        // become a silent rewrite of A.
        let vault = vault_of(vec![("A", GOOD)]);
        let clash = staged_new("").replace("urn:ngm:class:new-thing", "urn:ngm:class:a");
        let p = propose_create(&vault, "urn:ngm:class:a", &clash);
        assert_eq!(p.kind, ProposalKind::Create);
        assert!(
            p.blockers.iter().any(|b| b.code == "IRI_COLLISION"),
            "{:?}",
            p.blockers
        );
    }

    #[test]
    fn a_creation_taking_an_existing_slug_or_filename_is_blocked() {
        let vault = vault_of(vec![("A", GOOD)]);
        // `slug: a` is A's published slug.
        let p = propose_create(&vault, "New Thing", &staged_new("slug: a\n"));
        assert!(
            p.blockers.iter().any(|b| b.code == "SLUG_COLLISION"),
            "{:?}",
            p.blockers
        );
        // `a` is `A` on a case-insensitive checkout.
        let lower = staged_new("").replace("title: New Thing", "title: a");
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, path) = proposed_file(&lower);
        let mut opts = options(Level::Content, Some(path));
        opts.subject = "urn:ngm:class:new-thing".into();
        // No page has the exact id `a`, so this is a creation — and the
        // case-folded name is refused rather than written beside `A.md`.
        let folded = build(&vault, &corpus, &vocab(), "g", &opts).unwrap();
        assert_eq!(folded.kind, ProposalKind::Create);
        assert!(
            folded
                .blockers
                .iter()
                .any(|b| b.code == "FILENAME_COLLISION"),
            "{:?}",
            folded.blockers
        );
    }

    #[test]
    fn an_invalid_created_page_is_blocked() {
        let vault = vault_of(vec![("A", GOOD)]);
        // A Class without its required `status`.
        let p = propose_create(
            &vault,
            "New Thing",
            &staged_new("").replace("status: draft\n", ""),
        );
        assert_eq!(p.kind, ProposalKind::Create);
        assert!(!p.is_postable(), "a missing required key blocks");
        // A `/` in the title would put the page in a phantom directory.
        let slashed = staged_new("").replace("title: New Thing", "title: A/B Testing");
        let p = propose_create(&vault, "A/B Testing", &slashed);
        assert!(
            p.blockers.iter().any(|b| b.code == "FILENAME_INVALID"),
            "{:?}",
            p.blockers
        );
    }

    #[test]
    fn a_new_key_makes_a_creation_schema_level() {
        let vocab = Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
  enables: { owl: "ngm:enables", status: provisional }
"#,
        )
        .unwrap();
        let vault = vault_of(vec![("A", GOOD.replace("quality: 0.35\n", "").as_str())]);
        let corpus = Corpus::build(&vault, &vocab);
        let (_dir, path) = proposed_file(&staged_new("enables:\n- '[[A]]'\n"));
        let mut opts = options(Level::Content, Some(path));
        opts.subject = "New Thing".into();
        let p = build(&vault, &corpus, &vocab, "g", &opts).unwrap();
        assert_eq!(p.level, Level::Schema, "{:?}", p.blockers);
        let (_dir, plain) = proposed_file(&staged_new(""));
        let mut opts = options(Level::Content, Some(plain));
        opts.subject = "New Thing".into();
        assert_eq!(
            build(&vault, &corpus, &vocab, "g", &opts).unwrap().level,
            Level::Content
        );
    }

    #[test]
    fn a_manifest_entry_declaring_its_title_is_a_creation() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, root) = manifest(&[
            ("A", &GOOD.replace("0.35", "0.55")),
            ("New Thing", &staged_new("")),
        ]);
        let mut opts = options(Level::Content, Some(root));
        opts.title = Some("Elevate New Thing".into());
        let p = build(&vault, &corpus, &vocab(), "g", &opts).unwrap();
        assert_eq!(p.kind, ProposalKind::Create);
        assert_eq!(p.pages, vec!["A".to_owned(), "New Thing".to_owned()]);
        assert!(
            p.diff.contains("--- /dev/null\n+++ b/New Thing"),
            "{}",
            p.diff
        );
        assert!(p.is_postable(), "{:?}", p.blockers);
    }

    #[test]
    fn a_staged_page_naming_neither_subject_nor_itself_is_an_error() {
        let vault = vault_of(vec![("A", GOOD)]);
        let corpus = Corpus::build(&vault, &vocab());
        let (_dir, path) = proposed_file(&staged_new(""));
        let mut opts = options(Level::Content, Some(path));
        opts.subject = "Something Else".into();
        let err = build(&vault, &corpus, &vocab(), "g", &opts).unwrap_err();
        assert!(err.to_string().contains("names neither"), "{err}");
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
