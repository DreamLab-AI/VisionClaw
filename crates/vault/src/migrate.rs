//! `vault migrate --fences-to-properties` — the one remaining one-shot.
//!
//! It folds the two `json-ld` fences on every knowledge page into typed
//! Obsidian Properties (decision Q4), converts the surviving Logseq
//! `key:: value` lines and `{{embed}}`s, and stamps the OKF trust block.
//!
//! **It is lossless by construction.** Every key in every fence must be
//! covered by the vocabulary's `migration:` map — either mapped to a
//! frontmatter key or explicitly listed in `ignore:`. An uncovered key is a
//! hard failure (exit 2) and **nothing is written**. The same rule applies to
//! Logseq property keys. That is what makes `--dry-run --report` a real
//! evidence artefact rather than a hopeful preview.
//!
//! Three things the fences carried that a wikilink cannot recompute, and which
//! are therefore migrated verbatim:
//!
//! * `slug` — 343 of 8,446 pages have a slug that is not `slugify(title)`;
//! * `resource` — 96 classes have an IRI whose tail is not the page slug;
//! * `links` — the curated `vc:outboundWikilinks` set, which differs from a
//!   body scan on roughly half the corpus and is what every backlink list in
//!   the build is computed from.
//!
//! A fourth, `tail_iris`, is *reported* rather than written: a reference whose
//! IRI is neither a corpus page's `resource` nor `namespace + slugify(label)`
//! (163 of 114,625) cannot survive as a bare wikilink, so the migration emits
//! the exact `tail_iris:` block the vocabulary needs and refuses to guess.
//!
//! Delete this module, the `migrate` feature and the subcommand once the run
//! is done and committed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_yaml::Value as Yaml;
use vault_core::fences::{self, FencePage};
use vault_core::frontmatter::Frontmatter;
use vault_core::page::page_files;
use vault_core::slug::slugify;
use vault_core::vocabulary::{Destination, Vocabulary};

/// The actor stamped into `generated:`.
pub const MIGRATION_ACTOR: &str = "process:vault-migrate/1.0";

/// One tree the migration converts: a vault's `pages/` or its `journals/`.
#[derive(Debug, Clone)]
pub struct Scope {
    /// The vault name — `knowledge` or `working`. The key per-vault counts use.
    pub vault: String,
    /// The directory walked, e.g. `<repo>/knowledge/pages`.
    pub dir: PathBuf,
    /// `true` for a `journals/` tree, whose identity is the date and whose
    /// type is [`vault_core::page::JOURNAL_TYPE`].
    pub journals: bool,
}

/// What to migrate and how.
#[derive(Debug, Clone)]
pub struct Options {
    /// Every tree to convert. More than one means `--vault all`, and then the
    /// ORDER MATTERS: both vaults are indexed before either is converted, so a
    /// `knowledge/` block reference can still resolve into `working/` after
    /// `logseq_keys.id: drop` has removed the `id::` lines it reads.
    pub scopes: Vec<Scope>,
    /// The vault repository root, used to resolve
    /// `migration.block_refs.resolve_from`.
    pub repo_root: PathBuf,
    /// Compute everything, write nothing.
    pub dry_run: bool,
    /// Proceed even when a page infers a type its destination vault forbids.
    ///
    /// Off by default, and the default is the safe one: a page falling back to
    /// `Note` means the tool could not establish what the page IS, and doing
    /// that to 8,457 governed `Class` pages would erase the ontology at exit 0.
    /// It is therefore a **refusal** (exit 2, nothing written).
    ///
    /// The escape hatch exists because one case is a genuine content decision
    /// rather than a tool failure: `knowledge/journals/`'s 125 pages are
    /// `Journal`s, which is a `working_types` member and not a governed type.
    /// Those pages are misfiled, and that is for an owner to fix, not for the
    /// migration to decide. Passing this flag says "I have read the report".
    pub allow_type_fallback: bool,
    /// The ISO-8601 instant stamped into `generated:`.
    pub now: String,
}

/// One page's migration.
#[derive(Debug, Clone, Serialize)]
pub struct PageMigration {
    /// The page id.
    pub id: String,
    /// Absolute path.
    #[serde(skip)]
    pub path: PathBuf,
    /// The rewritten file.
    #[serde(skip)]
    pub after: String,
    /// A unified diff, present only when a report was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Fence keys this page carried that the map does not cover.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uncovered: Vec<String>,
    /// `true` when `after` differs from what is on disk, and therefore when the
    /// page is written. A page that does not differ is never rewritten, so the
    /// mtime of an unchanged page is not disturbed — which is what lets `git`
    /// and a file watcher tell a real run from a no-op.
    pub changed: bool,
}

/// What one vault contributed to the run.
///
/// `--vault all` used to run `knowledge/` only and say nothing about it. A
/// per-vault breakdown is the difference between "the migration ran" and "the
/// migration ran over these 10,235 files".
#[derive(Debug, Clone, Default, Serialize)]
pub struct VaultCounts {
    /// Files examined under `pages/`.
    pub pages_examined: usize,
    /// Files examined under `journals/`.
    pub journals_examined: usize,
    /// Files converted, across both trees.
    pub converted: usize,
}

/// Aggregate counts and the evidence.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Markdown files examined.
    pub pages_examined: usize,
    /// Pages that carried fences and were converted.
    pub pages_converted: usize,
    /// Pages whose conversion would CHANGE the file — the write set.
    ///
    /// Distinct from [`Report::pages_converted`], which counts pages *examined
    /// and converted in memory*. On an already-migrated corpus every page is
    /// converted and none is changed, and reporting only the first number made
    /// a correct no-op look like a broken write path. A run that says
    /// `8,446 converted, 0 changed` is telling you it had nothing to do.
    pub pages_changed: usize,
    /// Pages whose conversion is byte-identical to what is already on disk.
    pub pages_unchanged: usize,
    /// json-ld fences removed.
    pub fences_removed: usize,
    /// Logseq `key:: value` lines converted or dropped.
    pub logseq_lines: usize,
    /// `{{embed}}`s rewritten to `![[…]]`.
    pub embeds_rewritten: usize,
    /// `(page, alias)` pairs added so a reference label resolves.
    pub aliases_added: usize,
    /// Uncovered fence keys, with how often each occurred.
    pub uncovered_fence_fields: BTreeMap<String, usize>,
    /// Uncovered Logseq keys, with how often each occurred.
    pub uncovered_logseq_keys: BTreeMap<String, usize>,
    /// Pages whose block reference could not be resolved, with the count on
    /// each. The reference is **removed** and reported, not refused: the
    /// vocabulary's own rule is "reported, then removed", and two references
    /// in `working/` point at blocks that genuinely no longer exist.
    ///
    /// `{{embed [[Page]]}}` becomes `![[Page]]`. A Logseq **block** reference,
    /// `{{embed ((uuid))}}`, has no Obsidian equivalent — the uuid names a
    /// block in a database that no longer exists — so it is refused rather
    /// than silently left behind for `vault validate` to fail on later.
    pub unconvertible_embeds: BTreeMap<String, usize>,
    /// Pages whose inferred `type` is not permitted in the vault they sit in,
    /// with the type that was written.
    ///
    /// **Reported, not refused.** Nothing is lost: the page converts, and the
    /// type is the correct inference from its content — 188
    /// `podcast-evidence/` pages and one personal root page are `Note`s, not
    /// ontology classes. Refusing would gate a *content* decision (do those
    /// pages belong in `working/`?) behind a tool error, and the condition is
    /// already loud: `vault validate` reports `INVALID_TYPE` for every one.
    /// The refusals this migration does make are the lossy ones — a key with
    /// nowhere to go — and this is not that.
    pub type_not_permitted: BTreeMap<String, String>,
    /// Pages carrying an unmatched code-fence opener, with its 1-based line.
    ///
    /// A genuine corpus defect that survives the migration: 511 knowledge
    /// pages open a fence they never close, usually inside an OWL
    /// functional-syntax block. The converter treats the region as prose —
    /// which is what it is, and which is why the `key::` lines after it are
    /// not lost — and records the page here so the source can be cleaned up.
    pub unterminated_fences: BTreeMap<String, usize>,
    /// Long-tail references whose legacy IRI namespace was normalised onto
    /// `namespace`, keyed by the label, with the IRI that was replaced.
    ///
    /// This is **informational**, not a refusal: the slug — the only part any
    /// v1 artefact reads — is preserved verbatim in the link target, and the
    /// PRD's governing principle is one clean namespace with no compatibility
    /// shims. The map is emitted so the change is auditable page by page.
    pub normalised_tail_iris: BTreeMap<String, String>,
    /// Whether the run was invoked with `--allow-type-fallback`, so
    /// [`Report::exit_code`] can be read without the options to hand.
    pub type_fallback_allowed: bool,
    /// Per-vault extent, keyed by vault name. Always populated, even for a
    /// single-vault run, so a report never leaves the extent implicit.
    pub per_vault: BTreeMap<String, VaultCounts>,
    /// Markdown files found under a converted tree and not loaded, with the
    /// reason for each.
    pub skipped: Vec<vault_core::page::Skip>,
    /// Keys converted out of a page's **own** pre-existing frontmatter through
    /// `migration.frontmatter_keys`, with how often each occurred.
    ///
    /// The third input category beside the fences and the `key::` lines. A key
    /// absent from the map is authored and passes through untouched, so it is
    /// not counted here.
    pub frontmatter_keys_converted: BTreeMap<String, usize>,
    /// Per-page results.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<PageMigration>,
}

impl Report {
    /// `true` when every fence key and Logseq key is covered.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.uncovered_fence_fields.is_empty() && self.uncovered_logseq_keys.is_empty()
    }

    /// `true` when the run may be written: lossless, and no page lost its type.
    ///
    /// The second clause is the one that matters on a re-run. A page whose type
    /// falls back to `Note` has had its identity **erased**, not converted, and
    /// that is a loss like any other uncovered key — so it refuses on the same
    /// terms rather than being reported under a successful exit.
    #[must_use]
    pub fn is_safe_to_write(&self, allow_type_fallback: bool) -> bool {
        self.is_lossless() && (allow_type_fallback || self.type_not_permitted.is_empty())
    }

    /// `0` on success, `2` when the map does not cover the corpus or a page
    /// would lose its type.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        i32::from(!self.is_safe_to_write(self.type_fallback_allowed)) * 2
    }
}

fn logseq_property_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"^\s*(?:-\s*)?([A-Za-z0-9_.-]+)::\s*(.*)$")
            .expect("static property regex")
    })
}

fn embed_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"\{\{embed\s+\[\[([^\]]+)\]\]\s*\}\}").expect("static embed regex")
    })
}

/// Any `{{embed …}}`, convertible or not.
fn any_embed_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"\{\{embed").expect("static embed regex"))
}

/// A block the corpus can still resolve a `((uuid))` to.
struct BlockSource {
    /// Repo-relative file the block lives in.
    file: String,
    /// The block's own text plus its children, already stripped of Logseq
    /// property lines.
    text: Vec<String>,
    /// `true` when children were cut at `max_lines`.
    truncated: bool,
}

fn block_id_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"(?m)^(\s*)(?:-\s*)?id::\s*([0-9a-fA-F-]{36})\s*(.*)$")
            .expect("static block id regex")
    })
}

/// A Logseq **block** reference: `{{embed ((anything))}}`.
/// A Logseq block reference, in either form it is written.
///
/// `{{embed ((uuid))}}` is capture 1; a **bare** `((uuid))` is capture 2. The
/// bare form is restricted to a real uuid shape so ordinary double parentheses
/// in prose — and the escaped `((block-uuid))` placeholder in the page that
/// documents the syntax — are not mistaken for references.
fn block_ref_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(concat!(
            r"(?:`\s*)?\{\{embed\s+\(\(([^)]+)\)\)\s*\}\}(?:\s*`)?",
            r"|\(\(([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}",
            r"-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\)\)",
        ))
        .expect("static block ref regex")
    })
}

/// `[[Target]]` or `[[Target|Alias]]` — the target is capture 1.
fn wikilink_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"\[\[([^\]|#^]+)(?:[|#^][^\]]*)?\]\]").expect("static wikilink regex")
    })
}

/// The identity a union-merge deduplicates on: the link target, slugified, so
/// `[[Localization]]` and `[[Localisation|Localization]]` are one edge.
fn link_key(link: &str) -> String {
    wikilink_re()
        .captures(link)
        .map_or_else(|| slugify(link), |c| slugify(c[1].trim()))
}

/// The outliner's `- ### Definition` bullet and its indented children — a
/// verbatim copy of the fence `definition` on 3,450 pages.
fn definition_bullet_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(
            r"(?m)^[ \t]*-[ \t]*#{2,4}[ \t]*Definition[ \t]*\n(?:[ \t]+-[ \t]*.*\n?)*",
        )
        .expect("static definition bullet regex")
    })
}

/// Strip the outliner's `- ### Relationships` section — the heading bullet and
/// everything under it, up to the next sibling heading bullet or end of page.
///
/// Present on 7,303 pages. Its content is the relation edges, which now live in
/// frontmatter and which the build regenerates, so leaving it would publish the
/// same facts twice and in two formats. Written as a line scan because Rust's
/// `regex` has no lookahead and the terminator is "the next heading bullet".
fn strip_relationships_section(body: &str) -> String {
    let is_heading_bullet = |line: &str| {
        let rest = line.trim_start();
        let rest = rest.strip_prefix('-')?;
        let rest = rest.trim_start();
        let hashes = rest.chars().take_while(|c| *c == '#').count();
        (2..=4)
            .contains(&hashes)
            .then(|| rest[hashes..].trim().to_owned())
    };

    let mut out: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in body.lines() {
        match is_heading_bullet(line) {
            Some(heading) => {
                skipping = heading.eq_ignore_ascii_case("Relationships");
                if !skipping {
                    out.push(line);
                }
            }
            None if skipping => {}
            None => out.push(line),
        }
    }
    out.join("\n")
}

fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Index every `id:: <uuid>` under `block_refs.resolve_from`.
///
/// The corpus's `id::` lines are not uniformly well formed — some carry the
/// block's text on the same line after the uuid — so the regex is deliberately
/// lenient and the trailing remainder is kept as part of the block.
fn index_blocks(
    options: &Options,
    vocab: &Vocabulary,
) -> anyhow::Result<BTreeMap<String, BlockSource>> {
    let cfg = &vocab.migration.block_refs;
    let mut index: BTreeMap<String, BlockSource> = BTreeMap::new();
    for dir in &cfg.resolve_from {
        let root = options.repo_root.join(dir);
        if !root.exists() {
            continue;
        }
        for path in page_files(&root)? {
            let text = std::fs::read_to_string(&path)?;
            if !text.contains("id::") {
                continue;
            }
            let rel = path
                .strip_prefix(&options.repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let lines: Vec<&str> = text.lines().collect();
            for caps in block_id_re().captures_iter(&text) {
                let uuid = caps[2].to_lowercase();
                if index.contains_key(&uuid) {
                    continue;
                }
                let whole = caps.get(0).map_or("", |m| m.as_str());
                let line_no = text[..caps.get(0).map_or(0, |m| m.start())]
                    .matches('\n')
                    .count();
                let trailing = caps[3].trim();

                // The block's own line: the nearest preceding bullet, unless the
                // id:: line is itself the bullet.
                let mut own: Vec<String> = Vec::new();
                let self_bullet = whole.trim_start().starts_with('-');
                let mut base = indent_of(lines.get(line_no).copied().unwrap_or(""));
                if self_bullet {
                    if !trailing.is_empty() {
                        own.push(trailing.to_owned());
                    }
                } else {
                    let mut i = line_no;
                    while i > 0 {
                        i -= 1;
                        let candidate = lines[i];
                        if candidate.trim().is_empty() {
                            continue;
                        }
                        if candidate.trim_start().starts_with('-') {
                            base = indent_of(candidate);
                            own.push(
                                candidate
                                    .trim_start()
                                    .trim_start_matches('-')
                                    .trim()
                                    .to_owned(),
                            );
                        }
                        break;
                    }
                    if !trailing.is_empty() {
                        own.push(trailing.trim_start_matches('-').trim().to_owned());
                    }
                }

                // Children: deeper-indented lines until a sibling or shallower one.
                let mut truncated = false;
                for line in lines.iter().skip(line_no + 1) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if indent_of(line) <= base {
                        break;
                    }
                    if own.len() >= cfg.max_lines {
                        truncated = true;
                        break;
                    }
                    own.push(line.trim_start().trim_start_matches('-').trim().to_owned());
                }

                // A Logseq property line inside the block would become residue.
                // Strip the source block's own metadata and any reference it
                // nested: inlined text becomes body prose, and a live `key::`
                // or `((uuid))` in it would fail `vault validate` on the
                // migration's own output.
                own.retain(|l| !logseq_property_re().is_match(l) && !l.trim().is_empty());
                for line in &mut own {
                    if block_ref_re().is_match(line) {
                        *line = block_ref_re().replace_all(line, "").trim().to_owned();
                    }
                }
                own.retain(|l| !l.trim().is_empty());
                if own.is_empty() {
                    continue;
                }
                index.insert(
                    uuid,
                    BlockSource {
                        file: rel.clone(),
                        text: own,
                        truncated,
                    },
                );
            }
        }
    }
    Ok(index)
}

/// Rewrite every `{{embed ((…))}}` in `body`.
///
/// Returns the new body, how many were inlined, and how many remain
/// unconvertible (no `id::` target and no declared decision).
fn resolve_block_refs(
    body: &str,
    vocab: &Vocabulary,
    index: &BTreeMap<String, BlockSource>,
) -> (String, usize, usize) {
    let cfg = &vocab.migration.block_refs;
    let mut inlined = 0usize;
    let mut refused = 0usize;

    // Documentation ABOUT Logseq syntax, not a reference. `Dr O'Hare Writing
    // for LogSeq` carries two, both inside inline code: a prompt telling a
    // writer to use `{{embeds}}` and a line explaining
    // `{{embed ((block-uuid))}}`. Escaping the braces keeps the page reading
    // correctly while leaving no live construct for the residue check.
    let mut body = std::borrow::Cow::Borrowed(body);
    for placeholder in &cfg.literal_placeholders {
        for form in [
            format!("{{{{embed (({placeholder}))}}}}"),
            format!("{{{{{placeholder}}}}}"),
        ] {
            if body.contains(&form) {
                let escaped = form.replace("{{", "\\{\\{").replace("}}", "\\}\\}");
                body = std::borrow::Cow::Owned(body.replace(&form, &escaped));
            }
        }
    }
    let body: &str = &body;

    let mut out = String::with_capacity(body.len());
    let mut last = 0usize;
    for m in block_ref_re().find_iter(body) {
        let caps = block_ref_re()
            .captures(m.as_str())
            .expect("the match captures");
        let inner = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map(|c| c.as_str().trim().to_owned())
            .unwrap_or_default();
        out.push_str(&body[last..m.start()]);
        last = m.end();

        let key = inner.to_lowercase();
        if let Some(src) = index.get(&key) {
            let indent: String = body[..m.start()]
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let mut block = String::new();
            let _ = writeln!(
                block,
                "<!-- vault-migrate: inlined Logseq block {inner} from {} -->",
                src.file
            );
            for line in &src.text {
                let _ = writeln!(block, "{indent}> {line}");
            }
            if src.truncated {
                let _ = writeln!(block, "{indent}> …");
            }
            let _ = write!(block, "{indent}> — {}", src.file);
            out.push_str(block.trim_start_matches('\n'));
            inlined += 1;
            continue;
        }
        match cfg.unresolved.get(&inner).and_then(Yaml::as_str) {
            Some("drop") => {}
            Some(replacement) => out.push_str(replacement),
            // Reported, then removed — the vocabulary's own rule. The target
            // block no longer exists anywhere in either vault, so leaving the
            // reference would fail `vault validate` on the migration's own
            // output and point the reader at nothing.
            None => refused += 1,
        }
    }
    out.push_str(&body[last..]);
    (out, inlined, refused)
}

/// `true` when `type_name` is permitted in the vault `scope` belongs to.
///
/// The two vaults are disjoint, and deliberately so. `knowledge/` holds the
/// governed ontology and accepts `vocabulary.types` only — `Class`, `Property`,
/// `Individual`. `working/` holds drafting and episodic material and accepts
/// `vocabulary.working_types` only.
///
/// `working/` used to also accept the governed types, on the reasoning that a
/// page may be drafted there before promotion. It does not: the 8 legacy
/// `OntologyClass` fences sit on `working/` pages, and accepting their `@type`
/// wrote `Class` into a vault that has no `resource`, no `status` and no OWL
/// projection for it. A draft of a class is a `Draft Concept`; promotion is
/// what mints the `Class`, in `knowledge/`.
fn permits_type(vocab: &Vocabulary, scope: &Scope, type_name: &str) -> bool {
    if scope.vault == "working" {
        vocab.working_types.contains(type_name)
    } else {
        vocab.types.contains_key(type_name)
    }
}

/// The type the migrated page is given, and the type reported as impermissible.
///
/// Candidates are tried in authority order — the page's own authored `type`
/// first, then the fence `@type`, then `Journal` for a `journals/` tree — and
/// the first one the destination vault permits is written. This is what keeps an authored `type: Episode` on a
/// `working/` page after a run, and what keeps `Class` out of `working/` when
/// a legacy `OntologyClass` fence claims it.
///
/// When no candidate is permitted the page is a `Note`: it has made no claim
/// the vault accepts, and a note is what an unclaimed page is. The second
/// element then names the type **at issue** — the refused candidate when there
/// was one, because that explains why the page is a `Note`, and otherwise
/// `Note` itself when the vault does not list it. `knowledge/` does not list
/// `Note`, so its fence-less pages still report, exactly as before.
fn resolve_type(
    vocab: &Vocabulary,
    scope: &Scope,
    authored: Option<&str>,
    fence_type: Option<&str>,
) -> (String, Option<String>) {
    /// The type of a page that has made no claim its vault accepts.
    const FALLBACK: &str = "Note";
    let journal = scope.journals.then_some(vault_core::page::JOURNAL_TYPE);
    let mut refused: Option<String> = None;
    for candidate in [authored, fence_type, journal].into_iter().flatten() {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            continue;
        }
        if permits_type(vocab, scope, candidate) {
            return (candidate.to_owned(), None);
        }
        // The first refusal is the one worth reporting: it is the page's most
        // authoritative claim about itself.
        if refused.is_none() {
            refused = Some(candidate.to_owned());
        }
    }
    let reported =
        refused.or_else(|| (!permits_type(vocab, scope, FALLBACK)).then(|| FALLBACK.to_owned()));
    (FALLBACK.to_owned(), reported)
}

/// A page with no `json-ld` fence, presented to the converter as an empty one.
///
/// Its body is the whole file after any existing frontmatter, so the Logseq
/// and embed passes see everything. `wikilinks` stays empty and the caller
/// suppresses the `links` key for these pages: there is no curated outbound
/// set to preserve, so the build falls back to scanning the body.
fn fence_less_page(stem: &str, text: &str) -> FencePage {
    let split = vault_core::frontmatter::split(text);
    let existing = vault_core::frontmatter::Frontmatter::parse(split.yaml.unwrap_or_default())
        .unwrap_or_default();
    FencePage {
        page_iri: String::new(),
        slug: String::new(),
        title: existing
            .text("title")
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| stem.to_owned()),
        is_public: existing.boolean("public").unwrap_or(false),
        schema_version: 0,
        wikilinks: Vec::new(),
        entity: None,
        body: split.body.to_owned(),
        raw_page_block: serde_json::Value::Null,
        existing_frontmatter: split.yaml.unwrap_or_default().to_owned(),
    }
}

/// `true` when a fence is the `vc:LinkResolutionsAnnotation` block.
///
/// 3,712 pages carry one. It is wholly derived data — wikilink-to-IRI
/// resolutions the build recomputes — so the vocabulary covers the whole fence
/// with a single `link_resolutions_fence: drop` rather than naming each key.
fn is_link_resolutions(block: &serde_json::Value) -> bool {
    matches!(
        block.get("@type").and_then(serde_json::Value::as_str),
        Some("vc:LinkResolutionsAnnotation" | "LinkResolutionsAnnotation")
    )
}

/// A parsed page plus the facts the second pass needs.
struct Loaded {
    /// Index into [`Options::scopes`] — which tree this page came from.
    scope: usize,
    id: String,
    path: PathBuf,
    before: String,
    fence: FencePage,
    /// `false` for a page that carried no `json-ld` fence at all.
    ///
    /// 189 knowledge pages (188 under `podcast-evidence/`, one at the root)
    /// and every one of `working/`'s 574 pages are in this position. They were
    /// skipped wholesale, which left 16,145 `key:: value` lines untouched and
    /// made `--vault working` a no-op. They are converted by the same
    /// machinery minus the fence step.
    had_fences: bool,
}

/// Run the migration.
///
/// # Errors
/// I/O failure while reading the corpus or (when not a dry run) writing it
/// back.
///
/// # Panics
/// Never: the two `unwrap_or` fallbacks cover the only cases where a walked
/// path is not under `pages_dir`.
#[allow(clippy::too_many_lines)] // The one-shot, in the order it must run.
pub fn run(options: &Options, vocab: &Vocabulary, want_diff: bool) -> anyhow::Result<Report> {
    let mut report = Report {
        type_fallback_allowed: options.allow_type_fallback,
        ..Report::default()
    };
    let mut loaded: Vec<Loaded> = Vec::new();

    for (index, scope) in options.scopes.iter().enumerate() {
        // Register the vault even when the tree is absent, so the extent is
        // never implicit.
        report.per_vault.entry(scope.vault.clone()).or_default();
        if !scope.dir.is_dir() {
            continue; // a vault without a `journals/` tree is normal
        }
        let walk = vault_core::page::walk_pages(&scope.dir)?;
        for skip in walk.skipped {
            report.skipped.push(vault_core::page::Skip {
                path: Path::new(&scope.vault)
                    .join(if scope.journals { "journals" } else { "pages" })
                    .join(skip.path),
                reason: skip.reason,
            });
        }
        for path in walk.files {
            report.pages_examined += 1;
            let counts = report.per_vault.entry(scope.vault.clone()).or_default();
            if scope.journals {
                counts.journals_examined += 1;
            } else {
                counts.pages_examined += 1;
            }
            let before = std::fs::read_to_string(&path)?;
            let rel = path.strip_prefix(&scope.dir).unwrap_or(&path);
            let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
            let stem = Path::new(&id)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&id)
                .to_owned();
            let (fence, had_fences) = match fences::parse_fence_page(&stem, &before) {
                Some(fence) => {
                    report.fences_removed += fences::fence_blocks(&before).len();
                    (fence, true)
                }
                // No fence, but very possibly `key::` lines, embeds and block
                // references. Convert it through the same path with an empty
                // ontology block rather than leaving the residue behind.
                None => (fence_less_page(&stem, &before), false),
            };
            loaded.push(Loaded {
                scope: index,
                id,
                path,
                before,
                fence,
                had_fences,
            });
        }
    }

    // --- pass 1: resolution indexes -----------------------------------------
    let mut iri_to_title: BTreeMap<&str, &str> = BTreeMap::new();
    let mut known_titles: BTreeSet<String> = BTreeSet::new();
    for l in &loaded {
        known_titles.insert(l.fence.title.to_lowercase());
        if let Some(e) = &l.fence.entity {
            if !e.iri.is_empty() {
                iri_to_title.entry(&e.iri).or_insert(&l.fence.title);
            }
        }
        iri_to_title
            .entry(&l.fence.page_iri)
            .or_insert(&l.fence.title);
    }

    // Aliases a target page must gain so a reference label still resolves.
    let mut wanted_aliases: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut normalised_tail: BTreeMap<String, String> = BTreeMap::new();
    for l in &loaded {
        let Some(entity) = &l.fence.entity else {
            continue;
        };
        let refs = entity
            .sub_class_of
            .iter()
            .chain(entity.instance_of.iter())
            .chain(entity.relations.values().flatten())
            .chain(l.fence.wikilinks.iter());
        for r in refs {
            if let Some(title) = iri_to_title.get(r.iri.as_str()) {
                if !r.label.is_empty() && r.label != **title {
                    wanted_aliases
                        .entry((*title).to_owned())
                        .or_default()
                        .insert(r.label.clone());
                }
            } else {
                let preserved =
                    format!("{}{}", vocab.namespace, vault_core::slug::ref_slug(&r.iri));
                if preserved != r.iri && !vocab.tail_iris.contains_key(&r.label) {
                    normalised_tail.insert(r.label.clone(), r.iri.clone());
                }
            }
        }
    }
    report.normalised_tail_iris = normalised_tail;

    // --- pass 2: rewrite -----------------------------------------------------
    let block_index = index_blocks(options, vocab)?;
    for l in &loaded {
        let mut page = migrate_page(
            l,
            options,
            vocab,
            &iri_to_title,
            &wanted_aliases,
            &block_index,
        )?;
        for key in &page.migration.uncovered {
            *report
                .uncovered_fence_fields
                .entry(key.clone())
                .or_insert(0) += 1;
        }
        report
            .per_vault
            .entry(options.scopes[l.scope].vault.clone())
            .or_default()
            .converted += 1;
        report.logseq_lines += page.logseq_lines;
        report.embeds_rewritten += page.embeds;
        // The refusal is the *candidate the vault rejected*, not the type that
        // was written: the written one is permitted by construction now.
        if let Some(refused) = page.refused_type.take() {
            report.type_not_permitted.insert(l.id.clone(), refused);
        }
        for (key, count) in std::mem::take(&mut page.frontmatter_keys) {
            *report.frontmatter_keys_converted.entry(key).or_insert(0) += count;
        }
        if let Some(line) = vault_core::code::CodeMap::scan(&l.before).unterminated_fence {
            report.unterminated_fences.insert(l.id.clone(), line);
        }
        if page.unconvertible_embeds > 0 {
            report
                .unconvertible_embeds
                .insert(l.id.clone(), page.unconvertible_embeds);
        }
        for (key, count) in std::mem::take(&mut page.uncovered_logseq) {
            *report.uncovered_logseq_keys.entry(key).or_insert(0) += count;
        }
        report.pages_converted += 1;
        if page.migration.changed {
            report.pages_changed += 1;
        } else {
            report.pages_unchanged += 1;
        }
        if want_diff {
            page.migration.diff = Some(unified_diff(&l.id, &l.before, &page.migration.after));
        }
        report.pages.push(page.migration);
    }
    report.aliases_added = wanted_aliases.values().map(BTreeSet::len).sum();

    if !report.is_safe_to_write(options.allow_type_fallback) {
        return Ok(report); // exit 2; nothing written
    }
    if !options.dry_run {
        for page in &report.pages {
            if !page.changed {
                continue; // nothing to say; leave the mtime alone
            }
            std::fs::write(&page.path, &page.after)?;
        }
    }
    Ok(report)
}

struct Migrated {
    migration: PageMigration,
    /// A type candidate the destination vault refused, if there was one.
    refused_type: Option<String>,
    /// Keys converted out of the page's own frontmatter, by source key.
    frontmatter_keys: BTreeMap<String, usize>,
    logseq_lines: usize,
    embeds: usize,
    unconvertible_embeds: usize,
    uncovered_logseq: BTreeMap<String, usize>,
}

/// A scalar rendered as the text a `Destination` writes.
///
/// Anything that is not a scalar (a mapping, a nested list) has no single
/// value to move, so it is left alone rather than stringified into nonsense.
fn scalar_text(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Every scalar in `value`, whether it is one scalar or a sequence of them.
fn scalar_texts(value: &Yaml) -> Vec<String> {
    match value {
        Yaml::Sequence(items) => items.iter().filter_map(scalar_text).collect(),
        other => scalar_text(other).into_iter().collect(),
    }
}

/// Write an authored value over whatever the fences put in `key`.
///
/// Scalars replace: the author's statement is the better one. Sequences union,
/// authored entries appended after the fence's so the fence order — which the
/// golden artefacts are keyed on — is undisturbed.
fn authored_wins(fm: &mut Frontmatter, key: &str, value: &Yaml) {
    if let (Some(Yaml::Sequence(current)), Yaml::Sequence(incoming)) = (fm.get(key), value) {
        let mut merged = current.clone();
        for item in incoming {
            if !merged.contains(item) {
                merged.push(item.clone());
            }
        }
        fm.set(key.to_owned(), Yaml::Sequence(merged));
        return;
    }
    fm.set(key.to_owned(), value.clone());
}

/// Set `field` inside the mapping-valued `key`, creating the mapping if needed.
fn set_nested(fm: &mut Frontmatter, key: &str, field: &str, value: &str) {
    let mut mapping = match fm.get(key) {
        Some(Yaml::Mapping(m)) => m.clone(),
        _ => serde_yaml::Mapping::new(),
    };
    mapping.insert(
        Yaml::String(field.to_owned()),
        Yaml::String(value.to_owned()),
    );
    fm.set(key.to_owned(), Yaml::Mapping(mapping));
}

/// Set `field` on the `sources` entry with this `id`, appending the entry when
/// the page has none.
///
/// `sources` is a list of `{id, resource}` mappings ([`vault_core::okf::Source`]),
/// so a superseded identifier scheme becomes provenance rather than a second
/// frontmatter key nothing reads.
fn set_source_field(fm: &mut Frontmatter, id: &str, field: &str, value: &str) {
    let mut entries = match fm.get("sources") {
        Some(Yaml::Sequence(items)) => items.clone(),
        _ => Vec::new(),
    };
    let key_id = Yaml::String("id".to_owned());
    for entry in &mut entries {
        if let Yaml::Mapping(m) = entry {
            if m.get(&key_id).and_then(Yaml::as_str) == Some(id) {
                m.insert(
                    Yaml::String(field.to_owned()),
                    Yaml::String(value.to_owned()),
                );
                fm.set("sources", Yaml::Sequence(entries));
                return;
            }
        }
    }
    let mut m = serde_yaml::Mapping::new();
    m.insert(key_id, Yaml::String(id.to_owned()));
    m.insert(
        Yaml::String(field.to_owned()),
        Yaml::String(value.to_owned()),
    );
    entries.push(Yaml::Mapping(m));
    fm.set("sources", Yaml::Sequence(entries));
}

/// Convert the page's own pre-existing frontmatter.
///
/// This is the third input category, beside the `json-ld` fences and the
/// Logseq `key:: value` lines. Every key is one of two things:
///
/// * **Named** in `migration.frontmatter_keys` — converted through the same
///   [`Destination`] grammar the other two categories use. This is what moves
///   `legacy_iri` and `legacy_uri` into `sources` provenance, drops
///   `schema_version`, and folds `elevatedFrom` onto `sources[id=origin]`.
///   Without it the key passes through and `vault validate` reports it as
///   `UNKNOWN_KEY` — 64 of them across `knowledge/`.
/// * **Unnamed** — authored. It survives, and it wins over anything the fences
///   supplied, because a human wrote it and a fence did not.
///
/// `type` is handled by [`resolve_type`] before this runs, because the
/// destination vault has a veto over it that no other key has.
///
/// Returns the per-key conversion counts and any prose lines to prepend to the
/// body.
fn apply_existing_frontmatter(
    fm: &mut Frontmatter,
    existing: &Frontmatter,
    vocab: &Vocabulary,
) -> (BTreeMap<String, usize>, Vec<String>) {
    let map = &vocab.migration;
    let mut converted: BTreeMap<String, usize> = BTreeMap::new();
    let mut prose: Vec<String> = Vec::new();

    for key in existing.keys().map(str::to_owned).collect::<Vec<_>>() {
        if key == "type" {
            continue;
        }
        let Some(value) = existing.get(&key).cloned() else {
            continue;
        };
        let Some(destination) = map.frontmatter_destination(&key) else {
            authored_wins(fm, &key, &value);
            continue;
        };
        *converted.entry(key.clone()).or_insert(0) += 1;
        match destination {
            // `drop` is an owner decision recorded in the file: superseded
            // editor state, a schema version the vocabulary now owns, an asset
            // link the body already carries.
            //
            // `type` is resolved against the destination vault before this runs,
            // and `per_key`, `per_predicate` and `per_shape` expand a nested
            // fence object, which a flat frontmatter scalar is not. Different
            // reasons, the same outcome: the key contributes nothing here.
            Destination::Drop
            | Destination::Type
            | Destination::PerKey
            | Destination::PerPredicate
            | Destination::PerShape => {}
            Destination::Prose => {
                if let Some(text) = scalar_text(&value) {
                    if !text.is_empty() {
                        prose.push(format!("**{key}:** {text}"));
                    }
                }
            }
            Destination::Body => {
                if let Some(text) = scalar_text(&value) {
                    if !text.is_empty() {
                        prose.push(text);
                    }
                }
            }
            Destination::Key(target) => {
                let target = target.clone();
                authored_wins(fm, &target, &value);
            }
            Destination::Nested { key: k, field } => {
                let (k, field) = (k.clone(), field.clone());
                if let Some(text) = scalar_text(&value) {
                    set_nested(fm, &k, &field, &text);
                }
            }
            Destination::SourceField { id, field } => {
                let (id, field) = (id.clone(), field.clone());
                if let Some(text) = scalar_text(&value) {
                    if !text.is_empty() {
                        set_source_field(fm, &id, &field, &text);
                    }
                }
            }
            Destination::SourceAppend => {
                // The source key names the provenance entry, so several values
                // under one key stay distinguishable and the id is stable
                // across runs.
                for (n, text) in scalar_texts(&value).into_iter().enumerate() {
                    if text.is_empty() {
                        continue;
                    }
                    let id = if n == 0 {
                        key.clone()
                    } else {
                        format!("{key}-{}", n + 1)
                    };
                    set_source_field(fm, &id, "resource", &text);
                }
            }
        }
    }
    (converted, prose)
}

#[allow(clippy::too_many_lines)] // One faithful one-shot conversion.
fn migrate_page(
    loaded: &Loaded,
    options: &Options,
    vocab: &Vocabulary,
    iri_to_title: &BTreeMap<&str, &str>,
    wanted_aliases: &BTreeMap<String, BTreeSet<String>>,
    block_index: &BTreeMap<String, BlockSource>,
) -> anyhow::Result<Migrated> {
    let fence = &loaded.fence;
    let map = &vocab.migration;
    let mut fm = Frontmatter::default();
    let mut uncovered: Vec<String> = Vec::new();

    // Anything the page already had above the fences wins: the author wrote it.
    let existing = Frontmatter::parse(&fence.existing_frontmatter).unwrap_or_default();

    // Coverage: every key of every fence must have a destination. Per-fence-type
    // maps when the vocabulary spells them out, falling back to
    // the normalised union. `vault-core` folds both spellings of the migration
    // block into `migration.fences`, so a key declared either way is covered.
    let fence_map = |block: &serde_json::Value| -> &indexmap::IndexMap<String, Destination> {
        let per_type = match block.get("@type").and_then(serde_json::Value::as_str) {
            Some("Page") => &map.page_fence,
            Some("OntologyClass") => &map.ontology_class_fence,
            _ => &map.class_fence,
        };
        if per_type.is_empty() {
            &map.fences
        } else {
            per_type
        }
    };

    for block in [&fence.raw_page_block]
        .into_iter()
        .chain(fence.entity.as_ref().map(|e| &e.raw))
    {
        if is_link_resolutions(block) && map.link_resolutions_fence.is_some() {
            continue; // one decision covers a wholly derived fence
        }
        let per_fence = fence_map(block);
        if let Some(obj) = block.as_object() {
            for key in obj.keys() {
                if !per_fence.contains_key(key.as_str()) && !map.fences.contains_key(key.as_str()) {
                    uncovered.push(key.to_owned());
                }
                if key == "relations" {
                    if let Some(nested) = obj["relations"].as_object() {
                        for nested_key in nested.keys() {
                            let dotted = format!("relations.{nested_key}");
                            if !map.relation_aliases.contains_key(nested_key.as_str())
                                && !map.fences.contains_key(&dotted)
                                && !per_fence.contains_key(&dotted)
                            {
                                uncovered.push(dotted);
                            }
                        }
                    }
                }
            }
        }
    }
    uncovered.sort_unstable();
    uncovered.dedup();

    // --- body first: the Logseq properties may name an alias or a label -----
    let mut logseq_lines = 0usize;
    let mut uncovered_logseq: BTreeMap<String, usize> = BTreeMap::new();
    let mut logseq_props: Vec<(String, String)> = Vec::new();
    let mut body_lines: Vec<String> = Vec::new();
    for line in fence.body.lines() {
        if let Some(caps) = logseq_property_re().captures(line) {
            let key = caps[1].to_owned();
            let value = caps[2].trim().to_owned();
            logseq_lines += 1;
            match map.logseq_keys.get(&key) {
                // `Drop` duplicates a fence field, or is pipeline
                // bookkeeping, a self-measurement, or Logseq UI state.
                //
                // The `Some(_)` tail covers `generated.*`, `sources[…]`,
                // `body` and the per-* expansions: those are decisions whose
                // writer lives in the fence path, which supplies `generated`
                // and `sources` on every page carrying one of these lines, so
                // re-writing them here would clobber the more authoritative
                // value. Both cases therefore drop the line.
                // Block-level content: it cannot be lifted to a page-level
                // frontmatter key without collapsing many values into one, so
                // it is rendered into the bullet and the key is dropped.
                Some(Destination::Prose) => {
                    if !value.is_empty() {
                        let indent: String = line
                            .chars()
                            .take_while(|c| *c == ' ' || *c == '\t')
                            .collect();
                        let bullet = if line.trim_start().starts_with('-') {
                            "- "
                        } else {
                            ""
                        };
                        body_lines.push(format!("{indent}{bullet}**{key}:** {value}"));
                    }
                }
                Some(Destination::Key(target)) => {
                    if !value.is_empty() {
                        logseq_props.push((target.clone(), value));
                    }
                }
                Some(_) => {}
                None => {
                    *uncovered_logseq.entry(key).or_insert(0) += 1;
                    body_lines.push(line.to_owned());
                }
            }
            continue;
        }
        body_lines.push(line.to_owned());
    }
    let body = body_lines.join("\n");
    let embeds = embed_re().find_iter(&body).count();
    let body = embed_re().replace_all(&body, "![[$1]]").into_owned();
    let (body, inlined, refused) = resolve_block_refs(&body, vocab, block_index);
    let embeds = embeds + inlined;
    // A refused reference has already been removed by `resolve_block_refs`.
    // Anything still matching is an embed form nothing handles; it is reported
    // under the same key and removed, because leaving it would fail
    // `vault validate` on the migration's own output.
    let leftover = any_embed_re().find_iter(&body).count();
    let unconvertible_embeds = refused + leftover;
    let body = any_embed_re().replace_all(&body, "").into_owned();

    let link_of = |iri: &str, label: &str| -> String {
        if let Some(title) = iri_to_title.get(iri) {
            return if *title == label {
                format!("[[{title}]]")
            } else {
                format!("[[{title}|{label}]]")
            };
        }
        // A long-tail reference: no page owns this IRI, so the *slug* is the
        // only recoverable identity and it goes in the link target. The label
        // survives as the alias. This is what keeps `scaffold-index.json`
        // byte-identical: every artefact keys the tail on `ref_slug(iri)`.
        // Always slug-targeted, never a bare label: two distinct tail IRIs can
        // share a label (`5G New Radio` names both a page alias and a separate
        // concept), and a bare label would resolve through whichever alias
        // happened to be added first.
        let tail = vault_core::slug::ref_slug(iri);
        if tail.is_empty() || tail == label {
            format!("[[{label}]]")
        } else {
            format!("[[{tail}|{label}]]")
        }
    };
    let link_list = |refs: &[fences::Ref]| -> Yaml {
        Yaml::Sequence(
            refs.iter()
                .map(|r| Yaml::String(link_of(&r.iri, &r.label)))
                .collect(),
        )
    };

    // --- identity and OKF ----------------------------------------------------
    let entity = fence.entity.as_ref();
    // The page's own authored `type` outranks the fence's `@type`, a
    // `journals/` page is a `Journal` when it claims nothing, and the
    // destination vault outranks all three: see `resolve_type`.
    let scope = &options.scopes[loaded.scope];
    let (resolved_type, refused_type) = resolve_type(
        vocab,
        scope,
        existing.text("type").as_deref(),
        entity.map(|e| e.entity_type.as_str()),
    );
    fm.set("type", Yaml::String(resolved_type));
    fm.set("title", Yaml::String(fence.title.clone()));
    // A stem-derived title is a *fallback*, not an authored value. On the 188
    // fence-less `podcast-evidence/` pages there is no fence to supply one, so
    // without this the filename slug always beat the page's own `title::` line
    // and the published site got 188 slug-titled episode pages.
    let mut derived: BTreeSet<String> = BTreeSet::new();
    if !loaded.had_fences && existing.text("title").is_none() {
        derived.insert("title".to_owned());
    }
    if let Some(e) = entity {
        if !e.iri.is_empty() {
            fm.set("resource", Yaml::String(e.iri.clone()));
        }
        if !e.label.is_empty() && e.label != fence.title {
            fm.set("label", Yaml::String(e.label.clone()));
        }
        if !e.domain.is_empty() {
            fm.set("domain", Yaml::String(e.domain.clone()));
        }
        // `definition` placement is the vocabulary's call, and the governed
        // file declares `leading-paragraph` (contract C1 / VAULT-corpus-format
        // v2 §8). A vocabulary that says nothing gets the frontmatter form,
        // which is the conservative reading of silence. `leading-paragraph`
        // puts it at the
        // top of the body: it is prose, 451 characters at the median, and on
        // 2,970 pages it is the only text the page has — as a frontmatter key
        // those pages publish as a title with an empty body.
        if !e.definition.is_empty()
            && vocab.migration.body.definition_placement()
                == vault_core::vocabulary::DefinitionPlacement::Frontmatter
        {
            fm.set("definition", Yaml::String(e.definition.clone()));
        }
        fm.set("maturity", Yaml::String(e.maturity.clone()));
        if e.quality_score != 0.0 {
            fm.set("quality", Yaml::Number(e.quality_score.into()));
        }
    }
    fm.set("public", Yaml::Bool(fence.is_public));
    if !fence.slug.is_empty() && fence.slug != slugify(&fence.title) {
        fm.set("slug", Yaml::String(fence.slug.clone()));
    }
    if !fence.page_iri.is_empty() {
        fm.set("page_resource", Yaml::String(fence.page_iri.clone()));
    }

    let mut aliases: BTreeSet<String> = existing
        .strings("aliases")
        .into_iter()
        .collect::<BTreeSet<_>>();
    if let Some(extra) = wanted_aliases.get(&fence.title) {
        aliases.extend(extra.iter().cloned());
    }
    for (target, value) in &logseq_props {
        if target == "aliases" {
            aliases.insert(value.clone());
        }
    }
    // `vc:legacyProperties` is a Logseq carry-over the fences kept as an
    // array of `{vc:key, vc:value}`. Only `preferred-term` carries meaning
    // downstream (the search index reads it as an alternative label), so it
    // becomes an alias and the rest of the array is dropped by the map.
    if let Some(entries) = fence
        .raw_page_block
        .get("vc:legacyProperties")
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            let key = entry.get("vc:key").and_then(serde_json::Value::as_str);
            let value = entry.get("vc:value").and_then(serde_json::Value::as_str);
            match (key, value) {
                (Some("preferred-term"), Some(v)) if !v.is_empty() => {
                    aliases.insert(v.to_owned());
                }
                // The pre-urn identifier the WebVOWL node attributes carry as
                // `term_id`; 2,466 pages have one and it is the only other
                // legacy property with a downstream consumer.
                (Some("legacy-term-id" | "term-id"), Some(v))
                    if !v.is_empty() && !fm.has("legacy-term-id") =>
                {
                    fm.set("legacy-term-id", Yaml::String(v.to_owned()));
                }
                _ => {}
            }
        }
    }
    aliases.remove(&fence.title);
    if !aliases.is_empty() {
        fm.set(
            "aliases",
            Yaml::Sequence(aliases.into_iter().map(Yaml::String).collect()),
        );
    }

    if loaded.had_fences {
        // Always written, even empty: an absent `links` key means "scan the
        // body", and a body scan differs from the curated set on roughly half
        // the corpus. An explicit empty list is the honest statement that this
        // page's curated outbound set was empty.
        //
        // The list is keyed on the link's own IRI tail,
        // never on the target page's title: that tail is what every backlink
        // list in the bundle is computed from.
        fm.set(
            "links",
            Yaml::Sequence(
                fence
                    .wikilinks
                    .iter()
                    .map(|r| {
                        let tail = vault_core::slug::ref_slug(&r.iri);
                        Yaml::String(if tail.is_empty() || tail == r.label {
                            format!("[[{}]]", r.label)
                        } else {
                            format!("[[{tail}|{}]]", r.label)
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(e) = entity {
        if !e.sub_class_of.is_empty() {
            fm.set("is-a", link_list(&e.sub_class_of));
        }
        if !e.instance_of.is_empty() {
            fm.set("instance-of", link_list(&e.instance_of));
        }
        for (fm_key, _) in crate::model::RELATION_KEYS {
            let attr = fences::RELATION_TYPES
                .iter()
                .find(|(_, _, _, json)| {
                    crate::model::RELATION_KEYS
                        .iter()
                        .any(|(f, j)| f == fm_key && j == json)
                })
                .map(|(attr, _, _, _)| *attr);
            if let Some(attr) = attr {
                let refs = e.relation(attr);
                if !refs.is_empty() {
                    fm.set(*fm_key, link_list(refs));
                }
            }
        }
    }

    // Logseq properties.
    //
    // For a RELATION target the `key::` lines are not redundant: 36,632 edges
    // exist only there, 18,006 of which resolve to a page (WS-B census §9.1).
    // So a relation is UNION-merged with whatever the fence supplied,
    // deduplicated by link target and sorted, rather than being dropped or
    // allowed to overwrite. For a scalar the fence is the more authoritative
    // source and is never overwritten.
    for (target, value) in &logseq_props {
        if target == "aliases" {
            continue;
        }
        if vocab.relations.contains_key(target.as_str()) {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut merged: Vec<Yaml> = Vec::new();
            if let Some(Yaml::Sequence(existing)) = fm.get(target) {
                for item in existing {
                    if let Yaml::String(s) = item {
                        if seen.insert(link_key(s)) {
                            merged.push(Yaml::String(s.clone()));
                        }
                    }
                }
            }
            for label in wikilink_re().captures_iter(value) {
                let link = format!("[[{}]]", label[1].trim());
                if seen.insert(link_key(&link)) {
                    merged.push(Yaml::String(link));
                }
            }
            if !merged.is_empty() {
                fm.set(target.clone(), Yaml::Sequence(merged));
            }
        } else if !fm.has(target) || derived.remove(target.as_str()) {
            // `remove` also clears the marker, so only the FIRST authored value
            // displaces the fallback; a second `title::` does not win again.
            fm.set(target.clone(), Yaml::String(value.clone()));
        }
    }

    // --- OKF trust block -----------------------------------------------------
    fm.set("status", Yaml::String("stable".to_owned()));
    let mut generated = serde_yaml::Mapping::new();
    generated.insert(
        Yaml::String("by".into()),
        Yaml::String(MIGRATION_ACTOR.to_owned()),
    );
    generated.insert(Yaml::String("at".into()), Yaml::String(options.now.clone()));
    fm.set("generated", Yaml::Mapping(generated));

    // --- the page's own frontmatter: the third input category ---------------
    //
    // A key the vocabulary names in `migration.frontmatter_keys` is CONVERTED
    // through the same `Destination` grammar the fences and `key::` lines use.
    // A key it does not name is AUTHORED: it survives, and it wins over
    // anything the fences supplied, because a human wrote it and a fence did
    // not. Both rules are why `type: Episode` is still `Episode` after a run.
    let (frontmatter_keys, authored_prose) = apply_existing_frontmatter(&mut fm, &existing, vocab);

    // The outliner's own `### Definition` and `### Relationships` sections are
    // duplicates of data now held in frontmatter (`relationships_section_removed`)
    // or of the leading paragraph (`definition_dedupe`); the build regenerates
    // both from the frontmatter, so leaving them would publish the same facts
    // twice and in two formats.
    let mut body = body;
    if vocab.migration.body.relationships_section_removed {
        body = strip_relationships_section(&body);
    }
    if !authored_prose.is_empty() {
        body = format!("{}\n\n{}", authored_prose.join("\n"), body.trim_start());
    }
    let definition = fence
        .entity
        .as_ref()
        .map(|e| e.definition.trim())
        .unwrap_or_default();
    if !definition.is_empty() {
        body = definition_bullet_re().replace_all(&body, "").into_owned();
        if vocab.migration.body.definition_placement()
            == vault_core::vocabulary::DefinitionPlacement::LeadingParagraph
        {
            body = format!("{definition}\n\n{}", body.trim_start());
        }
    }

    // A canonical order, so the bytes do not depend on which input supplied
    // which key. See `Frontmatter::sort_canonical`.
    fm.sort_canonical();
    let rendered = fm.render()?;
    // `trim`, not `trim_end`. The separator below is unconditional, and a body
    // read back off an already-migrated page keeps the blank line that
    // separator produced — so trimming only the tail accumulated one blank line
    // per run and a second pass could never be a no-op. Leading blank lines
    // carry no meaning in markdown, and `Page::leading_paragraph` already skips
    // them, so normalising to exactly one is lossless and makes the conversion
    // IDEMPOTENT: migrating a migrated page returns the same bytes.
    let after = if body.trim().is_empty() {
        rendered
    } else {
        format!("{rendered}\n{}\n", body.trim())
    };

    Ok(Migrated {
        refused_type,
        frontmatter_keys,
        migration: PageMigration {
            id: loaded.id.clone(),
            path: loaded.path.clone(),
            changed: after != loaded.before,
            after,
            diff: None,
            uncovered,
        },
        logseq_lines,
        embeds,
        unconvertible_embeds,
        uncovered_logseq,
    })
}

/// A unified diff, for `--report`.
#[must_use]
pub fn unified_diff(name: &str, before: &str, after: &str) -> String {
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FENCED: &str = r#"---
public: true
aliases:
  - KG
---

# Knowledge Graph
```json-ld
{
  "@id": "urn:visionflow:page:abc",
  "@type": "Page",
  "vc:slug": "knowledge-graph",
  "title": "Knowledge Graph",
  "vc:public": true,
  "vc:schemaVersion": 2,
  "vc:outboundWikilinks": [ { "@id": "urn:ngm:class:ontology", "vc:label": "Ontology" } ]
}
```
```json-ld
{
  "@id": "urn:ngm:class:knowledge-graph",
  "@type": "Class",
  "label": "Knowledge Graph",
  "domain": "spatial-computing",
  "definition": "A graph of entities.",
  "maturity": "established",
  "quality": 0.35,
  "subClassOf": [ { "@id": "urn:ngm:class:ontology", "label": "Ontology" } ],
  "relations": { "requires": [ { "@id": "urn:ngm:class:ontology", "label": "Ontology" } ] }
}
```
- preferred-term:: Knowledge Graphs
- collapsed:: true
- Prose about {{embed [[Ontology]]}} here.
"#;

    const ONTOLOGY: &str = r#"```json-ld
{"@id":"urn:visionflow:page:ont","@type":"Page","vc:slug":"ontology","title":"Ontology","vc:public":true,"vc:schemaVersion":2}
```
```json-ld
{"@id":"urn:ngm:class:ontology","@type":"Class","label":"Ontology","domain":"spatial-computing","definition":"A shared vocabulary.","maturity":"established","quality":0.5}
```
"#;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a:     { owl: "rdfs:subClassOf" }
  requires: { owl: "vc:requires" }
scalars:
  domain:     { type: text }
  maturity:   { type: text }
  quality:    { type: number, min: 0, max: 1 }
  definition: { type: text }
migration:
  fence_fields:
    "vc:slug": slug
    "title": title
    "vc:public": public
    "vc:outboundWikilinks": links
    "label": label
    "domain": domain
    "definition": definition
    "maturity": maturity
    "quality": quality
    "subClassOf": is-a
    "relations.requires": requires
  ignore: ["@id", "@type", "@context", "vc:schemaVersion", "relations"]
  logseq_keys:
    preferred-term: aliases
    title: title
    collapsed: null
  frontmatter_keys:
    elevatedFrom: "sources[id=origin].resource"
    legacy_iri:   "sources[id=legacy-iri].resource"
    legacy_uri:   "sources[id=legacy-uri].resource"
    schema_version: drop
    icon: drop
    pubiic: public
    source: "sources[]"
    note: prose
"#,
        )
        .unwrap()
    }

    /// The same vocabulary, but for a `working/` vault: it declares
    /// `working_types` and therefore permits `Episode` and refuses `Class`.
    fn working_vocab() -> Vocabulary {
        let mut v = vocab();
        v.working_types = ["Note", "Episode", "Draft Concept", "Journal"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        v
    }

    /// A `working/pages` corpus, so `permits_type` takes the working branch.
    fn working_corpus(dir: &Path) -> PathBuf {
        let pages = dir.join("working").join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        pages
    }

    fn working_options(dir: &Path) -> Options {
        Options {
            scopes: vec![Scope {
                vault: "working".to_owned(),
                dir: dir.join("working").join("pages"),
                journals: false,
            }],
            ..options(dir, false)
        }
    }

    fn corpus_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(pages.join("Knowledge Graph.md"), FENCED).unwrap();
        std::fs::write(pages.join("Ontology.md"), ONTOLOGY).unwrap();
        dir
    }

    /// Options for a test about something other than typing.
    ///
    /// `allow_type_fallback: true` because most fixtures here are fence-less
    /// pages in a `knowledge`-shaped vocabulary, which legitimately refuse — and
    /// a test about slug derivation should not also be a test about that.
    /// `a_type_the_vault_does_not_permit_refuses_the_run` and
    /// `working_never_receives_class_from_a_legacy_ontology_class_fence` assert
    /// the default explicitly.
    fn options(dir: &Path, dry_run: bool) -> Options {
        Options {
            scopes: vec![Scope {
                vault: "knowledge".to_owned(),
                dir: dir.join("pages"),
                journals: false,
            }],
            repo_root: dir.to_path_buf(),
            allow_type_fallback: true,
            dry_run,
            now: "2026-09-22T00:00:00Z".to_owned(),
        }
    }

    /// A `knowledge/` + `working/`, `pages/` + `journals/` run: `--vault all`.
    fn all_options(dir: &Path) -> Options {
        let mut scopes = Vec::new();
        for vault in ["knowledge", "working"] {
            for journals in [false, true] {
                scopes.push(Scope {
                    vault: vault.to_owned(),
                    dir: dir
                        .join(vault)
                        .join(if journals { "journals" } else { "pages" }),
                    journals,
                });
            }
        }
        Options {
            scopes,
            repo_root: dir.to_path_buf(),
            allow_type_fallback: false,
            dry_run: false,
            now: "2026-09-22T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn a_dry_run_writes_nothing_and_reports_everything() {
        let dir = corpus_dir();
        let report = run(&options(dir.path(), true), &vocab(), true).unwrap();
        assert!(report.is_lossless());
        assert_eq!(report.exit_code(), 0);
        assert_eq!(report.pages_converted, 2);
        assert_eq!(report.fences_removed, 4);
        assert_eq!(report.embeds_rewritten, 1);
        assert_eq!(report.logseq_lines, 2);
        assert!(report.pages[0].diff.as_ref().unwrap().contains("+type:"));
        let on_disk = std::fs::read_to_string(dir.path().join("pages/Knowledge Graph.md")).unwrap();
        assert!(on_disk.contains("json-ld"), "dry run must not write");
    }

    #[test]
    fn the_real_run_produces_frontmatter_only_pages() {
        let dir = corpus_dir();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(dir.path().join("pages/Knowledge Graph.md")).unwrap();
        assert!(!after.contains("json-ld"));
        assert!(!after.contains("::"));
        assert!(!after.contains("{{embed"));
        assert!(after.contains("type: Class"));
        assert!(after.contains("resource: urn:ngm:class:knowledge-graph"));
        assert!(after.contains("status: stable"));
        assert!(after.contains("by: process:vault-migrate/1.0"));
        assert!(after.contains("at: 2026-09-22T00:00:00Z"));
        assert!(after.contains("![[Ontology]]"));
    }

    #[test]
    fn relations_become_wikilinks_resolved_through_the_target_title() {
        let dir = corpus_dir();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(dir.path().join("pages/Knowledge Graph.md")).unwrap();
        assert!(after.contains("is-a:\n- '[[Ontology]]'") || after.contains("- '[[Ontology]]'"));
        assert!(after.contains("requires:"));
        assert!(after.contains("links:"));
    }

    #[test]
    fn a_derived_slug_is_omitted_and_a_bespoke_one_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("2D LiDAR.md"),
            "```json-ld\n{\"@id\":\"urn:visionflow:page:x\",\"@type\":\"Page\",\"vc:slug\":\"2-d-li-dar\",\"title\":\"2D LiDAR\",\"vc:public\":true}\n```\n",
        )
        .unwrap();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("2D LiDAR.md")).unwrap();
        assert!(after.contains("slug: 2-d-li-dar"));
    }

    #[test]
    fn an_uncovered_fence_field_fails_with_exit_two_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        let text = "```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"T\",\"vc:mystery\":1}\n```\n";
        std::fs::write(pages.join("T.md"), text).unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert!(!report.is_lossless());
        assert_eq!(report.exit_code(), 2);
        assert_eq!(report.uncovered_fence_fields["vc:mystery"], 1);
        assert_eq!(std::fs::read_to_string(pages.join("T.md")).unwrap(), text);
    }

    #[test]
    fn an_unresolvable_block_reference_is_removed_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("T.md"),
            "```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"T\"}\n```\nSee {{embed ((66f13d66-1c8e-45a9-9f8b-41c4cc68ef9c))}} here.\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        // Reported, then removed — not refused. The vocabulary's own rule, and
        // two references in `working/` point at blocks that no longer exist.
        assert!(report.is_lossless());
        assert_eq!(report.exit_code(), 0);
        assert_eq!(report.unconvertible_embeds["T"], 1);
        let after = std::fs::read_to_string(pages.join("T.md")).unwrap();
        assert!(!after.contains("json-ld"));
        assert!(!after.contains("{{embed"), "the dead reference is removed");
    }

    #[test]
    fn a_page_embed_converts_and_does_not_count_as_unconvertible() {
        let dir = corpus_dir();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert_eq!(report.embeds_rewritten, 1);
        assert!(report.unconvertible_embeds.is_empty());
    }

    #[test]
    fn an_unterminated_fence_is_recorded_and_its_key_lines_survive() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Stray.md"),
            "```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"Stray\"}\n```\n             prose\n```\nan opener that never closes\n- preferred-term:: Alias Here\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert!(report.is_lossless(), "{report:?}");
        assert_eq!(report.unterminated_fences.get("Stray"), Some(&5));
        let after = std::fs::read_to_string(pages.join("Stray.md")).unwrap();
        assert!(
            after.contains("Alias Here"),
            "a `key::` line after a stray opener must still be converted: {after}"
        );
        assert!(!after.contains("preferred-term::"));
    }

    #[test]
    fn a_pending_frontmatter_rewrite_is_actually_written_and_counted_as_changed() {
        // The alarm this exists for: a correct no-op on an already-migrated
        // corpus was read as a broken write path, because the summary counted
        // pages CONVERTED (all of them) and not pages CHANGED (none).
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        let before = "---\ntype: Class\ntitle: Probe\nresource: urn:ngm:class:probe\n\
                      status: stable\nlegacy_iri: http://old/iri/probe\n\
                      schema_version: 2\n---\n\nA probe.\n";
        std::fs::write(pages.join("Probe.md"), before).unwrap();

        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert_eq!(report.pages_changed, 1, "the rewrite is pending");
        assert_eq!(report.pages_unchanged, 0);

        let after = std::fs::read_to_string(pages.join("Probe.md")).unwrap();
        assert_ne!(after, before, "the file must actually be written");
        assert!(after.contains("id: legacy-iri"), "{after}");
        assert!(!after.contains("schema_version"), "{after}");
        assert!(after.contains("type: Class"), "{after}");

        // And the second run is a zero-WRITE, not merely a zero-diff.
        let again = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert_eq!(again.pages_converted, 1, "still examined and converted");
        assert_eq!(again.pages_changed, 0, "but nothing to write");
        assert_eq!(again.pages_unchanged, 1);
        assert_eq!(
            std::fs::read_to_string(pages.join("Probe.md")).unwrap(),
            after
        );
    }

    #[test]
    fn an_unchanged_page_keeps_its_modification_time() {
        // `git` and a file watcher both use mtime to tell a real run from a
        // no-op; rewriting identical bytes destroys that signal on 8,446 files.
        let dir = corpus_dir();
        let pages = dir.path().join("pages");
        let mut v = vocab();
        v.migration.body.definition_placement = Some("leading-paragraph".to_owned());
        run(&options(dir.path(), false), &v, false).unwrap();

        let path = pages.join("Knowledge Graph.md");
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        // Sleep-free: the check is that the mtime is not *touched*, and a write
        // would replace it with a strictly later instant on any filesystem with
        // better than second granularity. Compare the bytes too, belt and braces.
        let bytes = std::fs::read_to_string(&path).unwrap();
        let report = run(&options(dir.path(), false), &v, false).unwrap();
        assert_eq!(report.pages_changed, 0);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), stamp);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    }

    #[test]
    fn migrating_an_already_migrated_page_is_a_byte_level_no_op() {
        // The gate condition. A second pass must produce the SAME BYTES, or
        // "did this run change anything?" is unanswerable from the diff — and
        // an operator who cannot answer that cannot safely re-run the tool over
        // 8,457 governed pages.
        let mut v = vocab();
        v.migration.body.definition_placement = Some("leading-paragraph".to_owned());
        let dir = corpus_dir();
        let pages = dir.path().join("pages");

        run(&options(dir.path(), false), &v, false).unwrap();
        let first: Vec<String> = ["Knowledge Graph", "Ontology"]
            .iter()
            .map(|n| std::fs::read_to_string(pages.join(format!("{n}.md"))).unwrap())
            .collect();

        // Second pass: no fences left, nothing to convert.
        let report = run(&options(dir.path(), false), &v, false).unwrap();
        let second: Vec<String> = ["Knowledge Graph", "Ontology"]
            .iter()
            .map(|n| std::fs::read_to_string(pages.join(format!("{n}.md"))).unwrap())
            .collect();
        assert_eq!(first, second, "a second migrate pass must be a no-op");

        // A third, to catch anything that accumulates on a two-run cycle.
        run(&options(dir.path(), false), &v, false).unwrap();
        let third: Vec<String> = ["Knowledge Graph", "Ontology"]
            .iter()
            .map(|n| std::fs::read_to_string(pages.join(format!("{n}.md"))).unwrap())
            .collect();
        assert_eq!(second, third);

        // And the governed type survived every pass. This is the regression
        // that would have turned 8,457 Classes into Notes: `type: Class` was
        // inferred from a fence, the fences are gone, and without the authored
        // value winning there is nothing left to infer from.
        assert!(first[0].contains("type: Class"), "{}", first[0]);
        assert!(third[0].contains("type: Class"), "{}", third[0]);
        assert!(
            report.type_not_permitted.is_empty(),
            "nothing should fall back to Note: {:?}",
            report.type_not_permitted
        );
    }

    #[test]
    fn a_definition_becomes_the_bodys_leading_paragraph_never_a_frontmatter_key() {
        // `definition_placement: leading-paragraph` (contract C1, VAULT-corpus
        // -format v2 §8). On 2,970 pages the definition is the only text the
        // page has; as a frontmatter key those pages publish as a title and an
        // empty body.
        let mut v = vocab();
        v.migration.body.definition_placement = Some("leading-paragraph".to_owned());
        let dir = corpus_dir();
        let pages = dir.path().join("pages");
        run(&options(dir.path(), false), &v, false).unwrap();
        let after = std::fs::read_to_string(pages.join("Knowledge Graph.md")).unwrap();
        let split = vault_core::frontmatter::split(&after);
        assert!(
            !split.yaml.unwrap_or_default().contains("definition:"),
            "the definition must not be a frontmatter key: {after}"
        );
        assert!(
            split.body.trim_start().starts_with("A graph of entities."),
            "the definition must lead the body: {after}"
        );
        // And the build derives it back out of the leading paragraph, which is
        // what keeps the scaffold index's `d` byte-identical.
        let page = vault_core::page::Page::parse(
            pages.join("Knowledge Graph.md"),
            "pages/Knowledge Graph.md",
            "Knowledge Graph",
            &after,
        )
        .unwrap();
        assert_eq!(page.leading_paragraph(), "A graph of entities.");
    }

    #[test]
    fn vault_all_runs_both_vaults_and_both_journal_trees() {
        // The regression: `--vault all` silently ran `knowledge/` only.
        let dir = tempfile::tempdir().unwrap();
        for vault in ["knowledge", "working"] {
            for tree in ["pages", "journals"] {
                std::fs::create_dir_all(dir.path().join(vault).join(tree)).unwrap();
            }
        }
        std::fs::write(
            dir.path().join("knowledge/pages/A.md"),
            "```json-ld\n{\"@type\": \"Class\", \"@id\": \"urn:ngm:class:a\", \"label\": \"A\"}\n```\n- Body.\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("working/pages/B.md"), "- Body.\n").unwrap();
        std::fs::write(
            dir.path().join("working/journals/2026-09-22.md"),
            "- A day's notes.\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("knowledge/journals/2026-09-21.md"),
            "- Another day.\n",
        )
        .unwrap();

        let report = run(
            &Options {
                allow_type_fallback: true,
                ..all_options(dir.path())
            },
            &working_vocab(),
            false,
        )
        .unwrap();
        assert_eq!(report.pages_examined, 4, "{report:?}");
        // Per-vault extent, stated rather than implied.
        let k = &report.per_vault["knowledge"];
        assert_eq!(
            (k.pages_examined, k.journals_examined, k.converted),
            (1, 1, 2)
        );
        let w = &report.per_vault["working"];
        assert_eq!(
            (w.pages_examined, w.journals_examined, w.converted),
            (1, 1, 2)
        );

        // A `working/` journal is a `Journal`, identified by its date.
        let journal =
            std::fs::read_to_string(dir.path().join("working/journals/2026-09-22.md")).unwrap();
        assert!(journal.contains("type: Journal"), "{journal}");
        assert!(journal.contains("title: 2026-09-22"), "{journal}");
        // `knowledge/` does not declare `Journal`, so that one is reported.
        assert_eq!(
            report.type_not_permitted.get("2026-09-21"),
            Some(&"Journal".to_owned())
        );
    }

    #[test]
    fn a_pages_own_frontmatter_keys_are_converted_not_passed_through() {
        // The third input category. Without `frontmatter_keys` these four keys
        // survive verbatim and `vault validate` reports each as UNKNOWN_KEY.
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Elevated.md"),
            "---\nschema_version: 2\nicon: 🧠\nelevatedFrom: \"[[working/Notes]]\"\nlegacy_iri: \"https://old/iri/a\"\nlegacy_uri: \"https://old/uri/a\"\n---\n- Prose.\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert!(report.is_lossless(), "{report:?}");
        let after = std::fs::read_to_string(pages.join("Elevated.md")).unwrap();

        // Dropped outright: an owner decision recorded in the vocabulary.
        assert!(!after.contains("schema_version"), "{after}");
        assert!(!after.contains("icon"), "{after}");
        // Superseded identifier schemes become provenance, not two new keys.
        assert!(!after.contains("legacy_iri"), "{after}");
        assert!(!after.contains("legacy_uri"), "{after}");
        assert!(after.contains("id: legacy-iri"), "{after}");
        assert!(after.contains("resource: https://old/iri/a"), "{after}");
        assert!(after.contains("id: legacy-uri"), "{after}");
        assert!(after.contains("id: origin"), "{after}");
        assert!(after.contains("[[working/Notes]]"), "{after}");

        // Every conversion is evidence in the report.
        assert_eq!(report.frontmatter_keys_converted["schema_version"], 1);
        assert_eq!(report.frontmatter_keys_converted["legacy_iri"], 1);
        assert_eq!(report.frontmatter_keys_converted["elevatedFrom"], 1);

        // And the page the migration wrote parses as a valid page.
        let page = vault_core::page::Page::parse(
            pages.join("Elevated.md"),
            "Elevated.md",
            "Elevated",
            &after,
        )
        .unwrap();
        assert_eq!(page.okf().sources.len(), 3);
    }

    #[test]
    fn a_frontmatter_key_the_vocabulary_renames_lands_on_the_new_key() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        // `pubiic` is a typo for `public` on one working page; `note` is prose.
        std::fs::write(
            pages.join("Typo.md"),
            "---\npubiic: true\nnote: seen in the wild\n---\n- Body.\n",
        )
        .unwrap();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("Typo.md")).unwrap();
        assert!(!after.contains("pubiic"), "{after}");
        assert!(after.contains("public: true"), "{after}");
        // `prose` keeps the value as text in the body, never as a key.
        let split = vault_core::frontmatter::split(&after);
        assert!(!split.yaml.unwrap_or_default().contains("note:"), "{after}");
        assert!(split.body.contains("**note:** seen in the wild"), "{after}");
    }

    #[test]
    fn a_frontmatter_key_the_vocabulary_does_not_name_is_authored_and_survives() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Authored.md"),
            "---\ndomain: Systems\nmy-own-key: kept\n---\n- Body.\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("Authored.md")).unwrap();
        assert!(after.contains("my-own-key: kept"), "{after}");
        // Authored, therefore not a conversion.
        assert!(!report.frontmatter_keys_converted.contains_key("my-own-key"));
    }

    #[test]
    fn an_authored_type_survives_the_migration() {
        // The regression this exists for: a run rewrote `type: Episode` to
        // `Note` on 295 `working/` pages, because the fence path set `type`
        // unconditionally and the author's value was only a fallback.
        let dir = tempfile::tempdir().unwrap();
        let pages = working_corpus(dir.path());
        std::fs::write(
            pages.join("AI Chips.md"),
            "---\ntype: Episode\ntitle: AI Chips\nstatus: draft\nepisodes: 5\n---\n- Body.\n",
        )
        .unwrap();
        run(&working_options(dir.path()), &working_vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("AI Chips.md")).unwrap();
        assert!(after.contains("type: Episode"), "{after}");
        assert!(!after.contains("type: Note"), "{after}");
        // The rest of the authored block survives with it.
        assert!(after.contains("status: draft"), "{after}");
        assert!(after.contains("episodes: 5"), "{after}");
    }

    #[test]
    fn an_authored_scalar_beats_the_migrations_own_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let pages = working_corpus(dir.path());
        std::fs::write(
            pages.join("Held.md"),
            "---\ntype: Note\nstatus: deprecated\npublic: true\n---\n- Body.\n",
        )
        .unwrap();
        run(&working_options(dir.path()), &working_vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("Held.md")).unwrap();
        assert!(after.contains("status: deprecated"), "{after}");
        assert!(!after.contains("status: stable"), "{after}");
        assert!(after.contains("public: true"), "{after}");
    }

    #[test]
    fn an_authored_list_unions_with_the_fence_rather_than_replacing_it() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Union.md"),
            "---\naliases: [Authored Alias]\n---\n- preferred-term:: Fence Alias\n",
        )
        .unwrap();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("Union.md")).unwrap();
        assert!(after.contains("Authored Alias"), "{after}");
        assert!(after.contains("Fence Alias"), "{after}");
    }

    #[test]
    fn working_never_receives_class_from_a_legacy_ontology_class_fence() {
        let dir = tempfile::tempdir().unwrap();
        let pages = working_corpus(dir.path());
        std::fs::write(
            pages.join("Legacy.md"),
            "```json-ld\n{\"@type\": \"Page\", \"@id\": \"urn:ngm:page:legacy\"}\n```\n\
             ```json-ld\n{\"@type\": \"Class\", \"@id\": \"urn:ngm:class:legacy\", \"label\": \"Legacy\"}\n```\n\
             - Body.\n",
        )
        .unwrap();
        // Refused: `Class` is not writable here and `Note` is not what the page
        // claims, so the run stops rather than pick one.
        let strict = Options {
            allow_type_fallback: false,
            ..working_options(dir.path())
        };
        let report = run(&strict, &working_vocab(), false).unwrap();
        assert_eq!(
            report.type_not_permitted.get("Legacy"),
            Some(&"Class".to_owned())
        );
        assert_eq!(report.exit_code(), 2);
        // With the hatch, `Class` still never reaches `working/`.
        let allowed = Options {
            allow_type_fallback: true,
            ..working_options(dir.path())
        };
        run(&allowed, &working_vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("Legacy.md")).unwrap();
        assert!(!after.contains("type: Class"), "{after}");
        assert!(after.contains("type: Note"), "{after}");
    }

    #[test]
    fn knowledge_accepts_only_the_governed_types() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        // `Episode` is a `working/` type; `knowledge/` does not list it.
        std::fs::write(
            pages.join("Misfiled.md"),
            "---\ntype: Episode\n---\n- Body.\n",
        )
        .unwrap();
        let opts = Options {
            allow_type_fallback: true,
            ..options(dir.path(), false)
        };
        let report = run(&opts, &working_vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("Misfiled.md")).unwrap();
        assert!(!after.contains("type: Episode"), "{after}");
        assert_eq!(
            report.type_not_permitted.get("Misfiled"),
            Some(&"Episode".to_owned())
        );
    }

    #[test]
    fn a_type_the_vault_does_not_permit_refuses_the_run() {
        // This was "reported, not refused", and the argument for that was sound
        // as far as it went: nothing is *lost* by writing `Note`, and refusing
        // gates a content decision behind a tool error.
        //
        // It was wrong about the stakes. `type` is inferred from a fence, and
        // `migrate` removes the fences — so on a re-run there is nothing left to
        // infer from and every page falls back. Reported-and-proceed turns
        // 8,457 governed `Class` pages into `Note`s and exits 0. A fallback is
        // not a lossless conversion; it is the tool failing to establish what
        // the page is, and it refuses on the same terms as an uncovered key.
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(pages.join("Episode.md"), "- Prose only.\n").unwrap();

        let strict = Options {
            allow_type_fallback: false,
            ..options(dir.path(), false)
        };
        let report = run(&strict, &vocab(), false).unwrap();
        assert_eq!(
            report.type_not_permitted.get("Episode"),
            Some(&"Note".to_owned())
        );
        assert!(report.is_lossless(), "no key was dropped");
        assert!(!report.is_safe_to_write(false), "but the type was erased");
        assert_eq!(report.exit_code(), 2);
        // Nothing written: the page is still the fence-less original.
        assert_eq!(
            std::fs::read_to_string(pages.join("Episode.md")).unwrap(),
            "- Prose only.\n"
        );
    }

    #[test]
    fn allow_type_fallback_writes_the_same_run() {
        // The escape hatch, for the one case that is a content decision:
        // `knowledge/journals/`'s 125 pages are `Journal`s, which the governed
        // types do not include.
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(pages.join("Episode.md"), "- Prose only.\n").unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert_eq!(report.exit_code(), 0);
        assert!(!report.type_not_permitted.is_empty(), "still reported");
        let after = std::fs::read_to_string(pages.join("Episode.md")).unwrap();
        assert!(after.contains("type: Note"), "{after}");
    }

    #[test]
    fn a_permitted_type_is_not_reported() {
        let dir = corpus_dir();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert!(
            report.type_not_permitted.is_empty(),
            "Class is permitted: {:?}",
            report.type_not_permitted
        );
    }

    #[test]
    fn an_authored_title_beats_the_filename_stem_on_a_fence_less_page() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("autoresearch-agent-loops.md"),
            "- title:: AI Daily Brief — Autoresearch, Agent Loops\n- Prose.\n",
        )
        .unwrap();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("autoresearch-agent-loops.md")).unwrap();
        assert!(
            after.contains("title: AI Daily Brief"),
            "the authored title must beat the stem: {after}"
        );
        assert!(!after.contains("title: autoresearch-agent-loops"));
    }

    #[test]
    fn a_fence_title_still_beats_a_logseq_title_line() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("T.md"),
            "```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"Fence Title\"}\n```\n             - title:: Logseq Title\n",
        )
        .unwrap();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(pages.join("T.md")).unwrap();
        assert!(after.contains("title: Fence Title"), "{after}");
        assert!(!after.contains("Logseq Title"));
    }

    #[test]
    fn a_page_with_no_fence_is_still_converted() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Episode.md"),
            "---\npublic: false\n---\n- preferred-term:: Some Alias\n- collapsed:: true\n             Prose body.\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert!(report.is_lossless(), "{report:?}");
        assert_eq!(
            report.pages_converted, 1,
            "a fence-less page is not skipped"
        );
        assert_eq!(report.fences_removed, 0);
        assert_eq!(report.logseq_lines, 2);
        let after = std::fs::read_to_string(pages.join("Episode.md")).unwrap();
        assert!(!after.contains("::"), "the residue is gone: {after}");
        assert!(after.contains("Some Alias"));
        assert!(after.contains("Prose body."));
        assert!(
            !after.contains("links:"),
            "a fence-less page has no curated outbound set to preserve"
        );
    }

    #[test]
    fn an_uncovered_logseq_key_also_fails() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("T.md"),
            "```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"T\"}\n```\n- weird-key:: value\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert_eq!(report.exit_code(), 2);
        assert_eq!(report.uncovered_logseq_keys["weird-key"], 1);
    }

    #[test]
    fn a_non_derivable_tail_slug_survives_in_the_link_target() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Planner.md"),
            "```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"Planner\"}\n```\n```json-ld\n{\"@id\":\"urn:ngm:class:planner\",\"@type\":\"Class\",\"relations\":{\"requires\":[{\"@id\":\"urn:ngm:class:ida-star\",\"label\":\"IDA*\"}]}}\n```\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert_eq!(report.exit_code(), 0, "a long-tail IRI is not a refusal");
        let after = std::fs::read_to_string(pages.join("Planner.md")).unwrap();
        // The slug goes in the target and the label survives as the alias.
        assert!(after.contains("[[ida-star|IDA*]]"), "{after}");
    }

    #[test]
    fn a_legacy_namespace_is_normalised_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Viewer.md"),
            "```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"Viewer\",\"vc:outboundWikilinks\":[{\"@id\":\"urn:visionflow:linked:3-d-engine\",\"vc:label\":\"3D Engine\"}]}\n```\n```json-ld\n{\"@id\":\"urn:ngm:class:viewer\",\"@type\":\"Class\"}\n```\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert!(report.is_lossless());
        assert_eq!(
            report.normalised_tail_iris["3D Engine"],
            "urn:visionflow:linked:3-d-engine"
        );
        let after = std::fs::read_to_string(pages.join("Viewer.md")).unwrap();
        assert!(after.contains("[[3-d-engine|3D Engine]]"), "{after}");
    }

    #[test]
    fn a_reference_label_that_differs_becomes_an_alias_on_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Localisation.md"),
            "```json-ld\n{\"@id\":\"p1\",\"@type\":\"Page\",\"title\":\"Localisation\",\"vc:public\":true}\n```\n```json-ld\n{\"@id\":\"urn:ngm:class:localisation\",\"@type\":\"Class\"}\n```\n",
        )
        .unwrap();
        std::fs::write(
            pages.join("LiDAR.md"),
            "```json-ld\n{\"@id\":\"p2\",\"@type\":\"Page\",\"title\":\"LiDAR\",\"vc:public\":true}\n```\n```json-ld\n{\"@id\":\"urn:ngm:class:lidar\",\"@type\":\"Class\",\"relations\":{\"requires\":[{\"@id\":\"urn:ngm:class:localisation\",\"label\":\"Localization\"}]}}\n```\n",
        )
        .unwrap();
        let report = run(&options(dir.path(), false), &vocab(), false).unwrap();
        assert!(report.is_lossless(), "{report:?}");
        assert_eq!(report.aliases_added, 1);
        let target = std::fs::read_to_string(pages.join("Localisation.md")).unwrap();
        assert!(target.contains("Localization"));
        let source = std::fs::read_to_string(pages.join("LiDAR.md")).unwrap();
        assert!(source.contains("[[Localisation|Localization]]"));
    }

    #[test]
    fn a_preferred_term_becomes_an_alias_and_collapsed_is_dropped() {
        let dir = corpus_dir();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(dir.path().join("pages/Knowledge Graph.md")).unwrap();
        assert!(!after.contains("collapsed"));
        assert!(
            after.contains("Knowledge Graphs"),
            "preferred-term becomes an alias"
        );
        assert!(!after.contains("::"));
    }

    #[test]
    fn existing_frontmatter_the_fences_do_not_supply_survives() {
        let dir = corpus_dir();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let after = std::fs::read_to_string(dir.path().join("pages/Knowledge Graph.md")).unwrap();
        assert!(after.contains("KG"), "the authored alias survives");
    }

    #[test]
    fn the_migrated_page_reparses_as_a_valid_page() {
        let dir = corpus_dir();
        run(&options(dir.path(), false), &vocab(), false).unwrap();
        let vault = vault_core::page::load_vault(dir.path()).unwrap();
        assert_eq!(vault.pages.len(), 2);
        let kg = vault.get("Knowledge Graph").unwrap();
        assert_eq!(kg.title(), "Knowledge Graph");
        assert!(kg.is_public());
        assert_eq!(kg.okf().status, Some(vault_core::okf::Status::Stable));
        assert_eq!(kg.frontmatter.wikilinks("is-a")[0].target, "Ontology");
    }
}
