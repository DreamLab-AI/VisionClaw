//! Pages and vaults: the shared reader `VisionClaw`'s ingest and the `vault` CLI
//! both call, so the corpus is parsed by exactly one implementation (Q11).
//!
//! Identity rule (contract C2): a page's id is its path relative to the vault's
//! `pages/` directory with the `.md` suffix removed. `knowledge/pages/Knowledge
//! Graph.md` has the id `Knowledge Graph`; `knowledge/pages/ETSI/Domain.md` has
//! the id `ETSI/Domain`.
//!
//! Three fields are *data*, not derivations, and the reader never invents them:
//!
//! * `title` — 313 pages in the corpus have a title that is not the file stem;
//! * `slug` — 343 have a slug that is not `slugify(title)`;
//! * `resource` — 96 classes have an IRI whose tail is not the page slug.
//!
//! When a page omits one, the reader falls back (stem, `slugify(title)`,
//! `namespace + slug`), which is what a newly authored page relies on.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{Result, VaultError};
use crate::frontmatter::{self, Frontmatter, Wikilink};
use crate::okf::OkfBlock;
use crate::slug::slugify;

/// Namespace folders that are **not published**. These pages stay in the
/// vault, are loaded by [`Vault::load`] and are ingested by `VisionClaw` —
/// only the publish scope ignores them (owner decision 2026-09-02, carried
/// over from `jsonld_parser.UNPUBLISHED_DIRS`).
///
/// This list is a *publication* rule and belongs to the projection, not to
/// enumeration. Applying it in [`page_files`] meant `vault validate` never
/// saw a `_misc` page, `vault find` could not retrieve one, and `vault
/// migrate` left them unconverted — a publication decision silently became an
/// existence decision.
pub const UNPUBLISHED_DIRS: &[&str] = &["_misc"];

/// The OKF type a `journals/` page carries. Declared in
/// `vocabulary.working_types`.
pub const JOURNAL_TYPE: &str = "Journal";

/// The journal date in a file stem, when the stem is one.
///
/// A journal's identity is its date — `2026-09-22` — not a slugified title, and
/// not a path. Obsidian's daily-note format and Logseq's exported format agree
/// on `YYYY-MM-DD`, so a stem that is not one is not a journal page and is
/// loaded as an ordinary page of the `journals/` tree.
#[must_use]
pub fn journal_date(stem: &str) -> Option<&str> {
    let b = stem.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    (digits(0..4) && digits(5..7) && digits(8..10)).then_some(stem)
}

/// Why the vault walk did not load a markdown file it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// A path component starts with `.` — `.obsidian/`, `.trash/`,
    /// `.deleted/`. Editor and tombstone state, not corpus.
    HiddenDirectory,
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HiddenDirectory => f.write_str("hidden directory"),
        }
    }
}

/// A markdown file the walk found and did not load.
///
/// Every skip is reported. A page that is silently absent cannot be
/// distinguished from a page that does not exist, and the two have very
/// different consequences for a link that points at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skip {
    /// The path, relative to the `pages/` directory that was walked.
    pub path: PathBuf,
    /// Why it was skipped.
    pub reason: SkipReason,
}

/// The result of walking a `pages/` tree: what was loaded, and what was not.
#[derive(Debug, Clone, Default)]
pub struct PageWalk {
    /// Page files in corpus order.
    pub files: Vec<PathBuf>,
    /// Markdown files the walk declined to load, with the reason.
    pub skipped: Vec<Skip>,
}

/// Which of the two vaults a page belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VaultKind {
    /// `knowledge/` — the governed, public-gated OKF bundle. Unknown
    /// frontmatter keys are a validation error.
    Knowledge,
    /// `working/` — the curator's space. Unknown keys are tolerated (OKF §4.1).
    Working,
}

impl VaultKind {
    /// The directory name.
    #[must_use]
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::Working => "working",
        }
    }

    /// `true` when unknown frontmatter keys fail validation.
    #[must_use]
    pub fn rejects_unknown_keys(self) -> bool {
        self == Self::Knowledge
    }
}

/// One markdown page.
#[derive(Debug, Clone)]
pub struct Page {
    /// Vault-relative id: the path under `pages/` without `.md`.
    pub id: String,
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Path relative to the vault root, e.g. `pages/ETSI/Domain.md`.
    pub rel_path: PathBuf,
    /// Parsed frontmatter, in author key order.
    pub frontmatter: Frontmatter,
    /// Everything after the closing `---`.
    pub body: String,
    /// The 1-based file line [`Page::body`] starts on, so a body offset can be
    /// reported as a file position.
    pub body_line: usize,
}

impl Page {
    /// Parse a page from its text. `id` and `rel_path` come from the caller
    /// because they are positional facts about the vault, not about the file.
    ///
    /// # Errors
    /// [`VaultError::Frontmatter`] when the YAML block is present but invalid.
    pub fn parse(
        path: impl Into<PathBuf>,
        rel_path: impl Into<PathBuf>,
        id: impl Into<String>,
        text: &str,
    ) -> Result<Self> {
        let path = path.into();
        let split = frontmatter::split(text);
        let fm = match split.yaml {
            Some(yaml) => Frontmatter::parse(yaml).map_err(|source| VaultError::Frontmatter {
                path: path.clone(),
                source,
            })?,
            None => Frontmatter::default(),
        };
        Ok(Self {
            id: id.into(),
            path,
            rel_path: rel_path.into(),
            frontmatter: fm,
            body: split.body.to_owned(),
            body_line: split.body_line,
        })
    }

    /// The page's title: the `title` property, else the file stem.
    #[must_use]
    pub fn title(&self) -> String {
        self.frontmatter
            .text("title")
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| {
                Path::new(&self.id)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&self.id)
                    .to_owned()
            })
    }

    /// The page's publish slug: the `slug` property, else `slugify(title)`.
    #[must_use]
    pub fn slug(&self) -> String {
        self.frontmatter
            .text("slug")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| slugify(&self.title()))
    }

    /// The page's stable IRI: the `resource` property, else
    /// `namespace + slug`.
    #[must_use]
    pub fn resource(&self, namespace: &str) -> String {
        self.frontmatter
            .text("resource")
            .filter(|r| !r.trim().is_empty())
            .unwrap_or_else(|| format!("{namespace}{}", self.slug()))
    }

    /// `public: true`? Absent means private — the gate fails closed.
    #[must_use]
    pub fn is_public(&self) -> bool {
        self.frontmatter.boolean("public").unwrap_or(false)
    }

    /// The OKF lifecycle and trust block.
    #[must_use]
    pub fn okf(&self) -> OkfBlock {
        OkfBlock::from_frontmatter(&self.frontmatter)
    }

    /// The page's curated outbound wikilinks.
    ///
    /// Prefers the `links` property, which the migration carries over verbatim
    /// from the json-ld fence's `vc:outboundWikilinks`. That set is a curated
    /// one and differs from a naive body scan on roughly half the corpus, so
    /// dropping it would silently change every backlink list the build emits.
    /// A page without `links` falls back to [`Page::body_wikilinks`].
    #[must_use]
    pub fn outbound_links(&self) -> Vec<Wikilink> {
        let declared = self.frontmatter.wikilinks("links");
        if declared.is_empty() && !self.frontmatter.has("links") {
            self.body_wikilinks()
        } else {
            declared
        }
    }

    /// The body's **leading paragraph** — the page's definition after the
    /// migration (`definition_placement: leading-paragraph`).
    ///
    /// Frontmatter is metadata and prose is body (Q4), so the fence
    /// `definition` became the first paragraph rather than a frontmatter key.
    /// The build reads it back from here to derive `scaffold-index`'s `d` and
    /// the OKF `description`, which is what keeps those byte-identical.
    ///
    /// A body that opens with a heading or a bullet has no leading paragraph:
    /// the definition is prose, and a `# Title` or `- item` is not.
    ///
    /// ```
    /// # use vault_core::page::Page;
    /// let page = Page::parse("/p.md", "pages/p.md", "p",
    ///     "---\ntype: Class\n---\nA graph of entities.\n\n## Detail\nmore\n").unwrap();
    /// assert_eq!(page.leading_paragraph(), "A graph of entities.");
    ///
    /// let headed = Page::parse("/q.md", "pages/q.md", "q",
    ///     "---\ntype: Class\n---\n## Detail\nmore\n").unwrap();
    /// assert_eq!(headed.leading_paragraph(), "");
    /// ```
    #[must_use]
    pub fn leading_paragraph(&self) -> String {
        let mut lines: Vec<&str> = Vec::new();
        for raw in self.body.lines() {
            let line = raw.trim();
            if line.is_empty() {
                if lines.is_empty() {
                    continue; // skip the blank run before the paragraph
                }
                break; // the paragraph ends at the first blank line after it
            }
            if lines.is_empty()
                && (line.starts_with('#')
                    || line.starts_with('-')
                    || line.starts_with('*')
                    || line.starts_with("```")
                    || line.starts_with('>'))
            {
                return String::new(); // a heading, bullet, fence or quote, not prose
            }
            lines.push(line);
        }
        lines.join(" ")
    }

    /// Every `[[…]]` in the body, deduplicated by target, in first-seen order.
    #[must_use]
    pub fn body_wikilinks(&self) -> Vec<Wikilink> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for raw in wikilink_tokens(&self.body) {
            if let Some(w) = Wikilink::parse(&raw) {
                if seen.insert(w.target.clone()) {
                    out.push(w);
                }
            }
        }
        out
    }

    /// The wikilink targets under every relation key the vocabulary declares.
    #[must_use]
    pub fn relations<'a>(
        &self,
        relation_keys: impl IntoIterator<Item = &'a str>,
    ) -> BTreeMap<String, Vec<Wikilink>> {
        relation_keys
            .into_iter()
            .filter_map(|key| {
                let links = self.frontmatter.wikilinks(key);
                (!links.is_empty()).then(|| (key.to_owned(), links))
            })
            .collect()
    }

    /// Re-render the page as it should appear on disk.
    ///
    /// # Errors
    /// Propagates a `serde_yaml` error if a frontmatter value is not
    /// representable.
    pub fn render(&self) -> Result<String> {
        let fm = self
            .frontmatter
            .render()
            .map_err(|source| VaultError::Frontmatter {
                path: self.path.clone(),
                source,
            })?;
        Ok(format!("{fm}{}", self.body))
    }
}

/// Extract the raw `[[…]]` tokens from `text`, skipping fenced code blocks.
fn wikilink_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'[' && bytes[i + 1] == b'[' {
                if let Some(end) = line[i + 2..].find("]]") {
                    out.push(line[i..i + 2 + end + 2].to_owned());
                    i += 2 + end + 2;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

/// A loaded vault: a root directory, its `pages/` tree, and every page in it.
#[derive(Debug, Clone)]
pub struct Vault {
    /// The vault root, e.g. `<repo>/knowledge`.
    pub root: PathBuf,
    /// Which vault this is.
    pub kind: VaultKind,
    /// Pages in corpus order (see [`walk_pages`]).
    pub pages: Vec<Page>,
    /// The `journals/` tree, in date order. Kept separate from
    /// [`Vault::pages`] deliberately: a journal's identity is its date, its
    /// type is [`JOURNAL_TYPE`], and it is not an ontology page. Mixing the
    /// two would silently enlarge every projection keyed on `pages`.
    pub journals: Vec<Page>,
    /// Markdown files under `pages/` and `journals/` the walk declined to
    /// load, with the reason for each. Never silently empty: an absent page is
    /// reported.
    pub skipped: Vec<Skip>,
}

impl Vault {
    /// Load every page under `root/pages`.
    ///
    /// # Errors
    /// [`VaultError::NotAVault`] when `root/pages` does not exist;
    /// [`VaultError::Io`] or [`VaultError::Frontmatter`] on a bad page.
    ///
    /// # Panics
    /// Never in practice: the walk only yields paths under `root/pages`, and
    /// the two `strip_prefix` calls assert that invariant rather than hide it.
    pub fn load(root: impl AsRef<Path>, kind: VaultKind) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let pages_dir = root.join("pages");
        if !pages_dir.is_dir() {
            return Err(VaultError::NotAVault {
                root,
                message: "no pages/ directory".into(),
            });
        }
        let walk = walk_pages(&pages_dir)?;
        let mut pages = Vec::new();
        for path in walk.files {
            let rel_to_pages = path
                .strip_prefix(&pages_dir)
                .expect("walk yields paths under pages_dir");
            let id = rel_to_pages
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            let rel_path = path
                .strip_prefix(&root)
                .expect("walk yields paths under root")
                .to_path_buf();
            let text = std::fs::read_to_string(&path).map_err(|e| VaultError::io(&path, e))?;
            pages.push(Page::parse(&path, rel_path, id, &text)?);
        }
        // `journals/` is a sibling of `pages/`, not a subtree of it, so it
        // needs its own walk. A vault without one is normal.
        let journals_dir = root.join("journals");
        let mut skipped = walk.skipped;
        let mut journals = Vec::new();
        if journals_dir.is_dir() {
            let jwalk = walk_pages(&journals_dir)?;
            for path in jwalk.files {
                let rel_to_journals = path
                    .strip_prefix(&journals_dir)
                    .expect("walk yields paths under journals_dir");
                let stem = rel_to_journals
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                let rel_path = path
                    .strip_prefix(&root)
                    .expect("walk yields paths under root")
                    .to_path_buf();
                let text = std::fs::read_to_string(&path).map_err(|e| VaultError::io(&path, e))?;
                journals.push(Page::parse(&path, rel_path, stem, &text)?);
            }
            skipped.extend(jwalk.skipped.into_iter().map(|s| Skip {
                path: PathBuf::from("journals").join(s.path),
                reason: s.reason,
            }));
        }
        Ok(Self {
            root,
            kind,
            pages,
            journals,
            skipped,
        })
    }

    /// Look a page up by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    /// Look a page up by id, mutably.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Page> {
        self.pages.iter_mut().find(|p| p.id == id)
    }

    /// Every page the vault holds: `pages/` then `journals/`.
    ///
    /// What an extent count, a link-integrity check or a secret scan wants —
    /// as opposed to [`Vault::pages`], which is the ontology projection's
    /// input and must not silently grow.
    pub fn all_pages(&self) -> impl Iterator<Item = &Page> {
        self.pages.iter().chain(&self.journals)
    }

    /// Only the pages with `public: true`.
    pub fn public(&self) -> impl Iterator<Item = &Page> {
        self.pages.iter().filter(|p| p.is_public())
    }
}

/// Every page file under `pages_dir`, in the corpus order the Python pipeline
/// used.
///
/// Ordering is a **component-wise** sort of the vault-relative path, which is
/// what `sorted(pages_dir.rglob("*.md"))` does in `CPython` (`PurePath.__lt__`
/// compares part tuples). A plain string sort differs whenever a file name
/// contains a byte below `/`, so it is not interchangeable.
///
/// Only dot-directories are skipped, and each skip is reported. [`UNPUBLISHED_DIRS`]
/// is **not** applied here: it is a publication rule, and the publish scope in
/// `vault`'s projection is where it belongs.
///
/// # Errors
/// [`VaultError::Io`] when the tree cannot be walked.
pub fn walk_pages(pages_dir: impl AsRef<Path>) -> Result<PageWalk> {
    let pages_dir = pages_dir.as_ref();
    let mut rels: Vec<Vec<OsString>> = Vec::new();
    let mut skipped: Vec<Skip> = Vec::new();
    for entry in walkdir::WalkDir::new(pages_dir).follow_links(false) {
        let entry = entry.map_err(|e| {
            let path = e.path().unwrap_or(pages_dir).to_path_buf();
            VaultError::io(path, e.into())
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(rel) = path.strip_prefix(pages_dir) else {
            continue;
        };
        let parts: Vec<OsString> = rel.iter().map(std::ffi::OsStr::to_os_string).collect();
        if parts.iter().any(|p| p.to_string_lossy().starts_with('.')) {
            skipped.push(Skip {
                path: rel.to_path_buf(),
                reason: SkipReason::HiddenDirectory,
            });
            continue;
        }
        rels.push(parts);
    }
    rels.sort();
    skipped.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(PageWalk {
        files: rels
            .into_iter()
            .map(|parts| parts.iter().fold(pages_dir.to_path_buf(), |a, p| a.join(p)))
            .collect(),
        skipped,
    })
}

/// [`walk_pages`], keeping only the files.
///
/// # Errors
/// As [`walk_pages`].
pub fn page_files(pages_dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    Ok(walk_pages(pages_dir)?.files)
}

/// `true` when the first path component of `rel` is an [`UNPUBLISHED_DIRS`]
/// folder — the pages that are loaded but never published.
#[must_use]
pub fn is_unpublished(rel: impl AsRef<Path>) -> bool {
    rel.as_ref()
        .iter()
        .next()
        .is_some_and(|p| UNPUBLISHED_DIRS.contains(&p.to_string_lossy().as_ref()))
}

/// Parse a single page given its path within a vault root.
///
/// This is the entry point `VisionClaw`'s `CorpusSource::LocalDirectory` calls
/// for one file; [`Vault::load`] is the whole-tree form.
///
/// # Errors
/// [`VaultError::Frontmatter`] on an invalid YAML block.
pub fn parse_page(pages_dir: impl AsRef<Path>, path: impl AsRef<Path>, text: &str) -> Result<Page> {
    let (pages_dir, path) = (pages_dir.as_ref(), path.as_ref());
    let rel = path.strip_prefix(pages_dir).unwrap_or(path);
    let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
    Page::parse(path, PathBuf::from("pages").join(rel), id, text)
}

/// Load a vault, inferring its kind from the directory name.
///
/// # Errors
/// As [`Vault::load`].
pub fn load_vault(root: impl AsRef<Path>) -> Result<Vault> {
    let root = root.as_ref();
    let kind = if root.file_name().and_then(|n| n.to_str()) == Some("working") {
        VaultKind::Working
    } else {
        VaultKind::Knowledge
    };
    Vault::load(root, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(text: &str) -> Page {
        Page::parse(
            "/v/pages/Knowledge Graph.md",
            "pages/Knowledge Graph.md",
            "Knowledge Graph",
            text,
        )
        .unwrap()
    }

    #[test]
    fn falls_back_to_the_stem_and_derived_slug() {
        let p = page("---\npublic: true\n---\nbody\n");
        assert_eq!(p.title(), "Knowledge Graph");
        assert_eq!(p.slug(), "knowledge-graph");
        assert_eq!(
            p.resource("urn:ngm:class:"),
            "urn:ngm:class:knowledge-graph"
        );
        assert!(p.is_public());
    }

    #[test]
    fn declared_slug_and_resource_win_over_derivation() {
        let p =
            page("---\ntitle: 2D LiDAR\nslug: 2-d-li-dar\nresource: urn:ngm:class:lidar\n---\n");
        assert_eq!(p.slug(), "2-d-li-dar");
        assert_eq!(p.resource("urn:ngm:class:"), "urn:ngm:class:lidar");
    }

    #[test]
    fn a_page_with_no_frontmatter_is_private() {
        let p = page("# Just a body\n");
        assert!(!p.is_public());
        assert!(p.frontmatter.map.is_empty());
    }

    #[test]
    fn body_wikilinks_dedupe_and_skip_code_fences() {
        let p = page("---\na: 1\n---\nSee [[Ontology]] and [[Ontology]] and [[Triple Store|TS]].\n\n```\n[[Not A Link]]\n```\n");
        let links = p.body_wikilinks();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "Ontology");
        assert_eq!(links[1].label(), "TS");
    }

    #[test]
    fn declared_links_override_the_body_scan() {
        let p = page("---\nlinks: [\"[[Curated]]\"]\n---\n[[Body Only]]\n");
        let links = p.outbound_links();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Curated");
    }

    #[test]
    fn the_leading_paragraph_joins_wrapped_lines() {
        let p = page("---\na: 1\n---\nA graph\nof entities.\n\n## Detail\n");
        assert_eq!(p.leading_paragraph(), "A graph of entities.");
    }

    #[test]
    fn a_body_opening_with_structure_has_no_leading_paragraph() {
        for body in [
            "## Heading\ntext\n",
            "- bullet\n",
            "> quote\n",
            "```\ncode\n```\n",
        ] {
            let p = page(&format!("---\na: 1\n---\n{body}"));
            assert_eq!(p.leading_paragraph(), "", "{body:?}");
        }
    }

    #[test]
    fn blank_lines_before_the_paragraph_are_skipped() {
        let p = page("---\na: 1\n---\n\n\nThe definition.\n\nmore\n");
        assert_eq!(p.leading_paragraph(), "The definition.");
    }

    #[test]
    fn an_empty_body_has_no_leading_paragraph() {
        assert_eq!(page("---\na: 1\n---\n").leading_paragraph(), "");
    }

    #[test]
    fn render_round_trips() {
        let text = "---\npublic: true\n---\n# Title\n\nbody\n";
        assert_eq!(page(text).render().unwrap(), text);
    }

    #[test]
    fn corpus_order_is_component_wise() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(pages.join("a")).unwrap();
        std::fs::create_dir_all(pages.join("_misc")).unwrap();
        std::fs::create_dir_all(pages.join(".trash")).unwrap();
        for rel in ["a-b.md", "a/c.md", "_misc/hidden.md", ".trash/gone.md"] {
            std::fs::write(pages.join(rel), "---\npublic: true\n---\n").unwrap();
        }
        let walk = walk_pages(&pages).unwrap();
        let names: Vec<String> = walk
            .files
            .iter()
            .map(|p| {
                p.strip_prefix(&pages)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        // ("a",) < ("a-b.md",) component-wise, though "a-b.md" < "a/c.md" as
        // strings. `_misc/` is enumerated: it is a publication rule, not an
        // existence rule, so the page is loaded and simply never published.
        assert_eq!(
            names,
            vec![
                "_misc/hidden.md".to_owned(),
                "a/c.md".to_owned(),
                "a-b.md".to_owned()
            ]
        );
        // The dot-directory is the only skip, and it is reported.
        assert_eq!(
            walk.skipped,
            vec![Skip {
                path: PathBuf::from(".trash/gone.md"),
                reason: SkipReason::HiddenDirectory
            }]
        );
    }

    #[test]
    fn an_unpublished_dir_is_loaded_but_outside_the_publish_scope() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(pages.join("_misc")).unwrap();
        std::fs::write(
            pages.join("_misc/Held.md"),
            "---\ntype: Class\npublic: true\n---\n",
        )
        .unwrap();
        let vault = Vault::load(dir.path(), VaultKind::Knowledge).unwrap();
        // Reachable by validate, find, retrieve and migrate…
        assert_eq!(vault.pages.len(), 1);
        assert_eq!(vault.pages[0].id, "_misc/Held");
        assert!(vault.skipped.is_empty());
        // …and still outside the publish scope.
        assert!(is_unpublished("_misc/Held.md"));
        assert!(!is_unpublished("a/Held.md"));
    }

    #[test]
    fn ids_are_vault_relative_paths_without_the_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let pages = dir.path().join("pages");
        std::fs::create_dir_all(pages.join("ETSI")).unwrap();
        std::fs::write(pages.join("ETSI/Domain.md"), "---\npublic: true\n---\n").unwrap();
        let vault = Vault::load(dir.path(), VaultKind::Knowledge).unwrap();
        assert_eq!(vault.pages.len(), 1);
        assert_eq!(vault.pages[0].id, "ETSI/Domain");
        assert_eq!(vault.pages[0].rel_path, Path::new("pages/ETSI/Domain.md"));
    }

    #[test]
    fn a_root_without_pages_is_not_a_vault() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Vault::load(dir.path(), VaultKind::Knowledge),
            Err(VaultError::NotAVault { .. })
        ));
    }
}
