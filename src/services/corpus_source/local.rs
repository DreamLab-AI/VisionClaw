// src/services/corpus_source/local.rs
//! [`CorpusSource`] over a mounted vault directory — the default source
//! (ADR-2114, PRD-sovereign-corpus Q2).

use async_trait::async_trait;
use log::{debug, info};
use std::path::{Path, PathBuf};

use vault_core::vocabulary::Vocabulary;

use super::{CorpusPage, CorpusSource, SourceDescriptor};

/// Environment variable naming the vault root on disk.
pub const VAULT_ROOT_ENV: &str = "VAULT_ROOT";
/// Environment variable naming the comma-separated ingest base paths,
/// relative to the vault root. Mirrors `GITHUB_BASE_PATHS`.
pub const VAULT_BASE_PATHS_ENV: &str = "VAULT_BASE_PATHS";
/// Base paths used when [`VAULT_BASE_PATHS_ENV`] is unset — the two vault
/// roots of the sovereign corpus. Pages sit at each root (the flat Obsidian
/// layout, owner decision 2026-10-16); the Logseq-era `pages/` folder is gone.
pub const DEFAULT_VAULT_BASE_PATHS: &str = "knowledge,working";
/// The vocabulary's location relative to the vault root (contract C1).
pub const VOCABULARY_PATH: &str = "ontology/vocabulary.yaml";

/// Walks a vault on disk: every `.md` file under each configured base path,
/// keyed on `mtime:size` so an unchanged file is skipped by the incremental
/// filter without reading it.
#[derive(Debug, Clone)]
pub struct LocalDirectorySource {
    /// Absolute (or process-relative) path to the vault root.
    pub root: PathBuf,
    /// Ingest base paths relative to [`Self::root`], in order.
    pub base_paths: Vec<PathBuf>,
    /// The same paths rendered with `/` separators, built once at
    /// construction so [`CorpusSource::base_paths`] can hand out a slice.
    slash_paths: Vec<String>,
}

impl LocalDirectorySource {
    /// Construct from an explicit root and base paths.
    pub fn new(root: impl Into<PathBuf>, base_paths: Vec<PathBuf>) -> Self {
        let slash_paths = base_paths.iter().map(|p| path_to_slash(p)).collect();
        Self {
            root: root.into(),
            base_paths,
            slash_paths,
        }
    }

    /// Construct from the environment: `VAULT_ROOT` plus `VAULT_BASE_PATHS`
    /// (default `knowledge,working`).
    ///
    /// Fails when `VAULT_ROOT` is unset or empty — the local source cannot
    /// guess where the vault is mounted.
    pub fn from_env() -> Result<Self, String> {
        let root = std::env::var(VAULT_ROOT_ENV)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                format!(
                    "{} is not set — the local corpus source needs the vault mount point",
                    VAULT_ROOT_ENV
                )
            })?;

        Ok(Self::new(root, Self::base_paths_from_env()?))
    }

    /// The configured ingest base paths, from `VAULT_BASE_PATHS` or the
    /// default dual-source split.
    pub fn base_paths_from_env() -> Result<Vec<PathBuf>, String> {
        let raw = std::env::var(VAULT_BASE_PATHS_ENV)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_VAULT_BASE_PATHS.to_string());

        let base_paths = parse_base_paths(&raw);
        if base_paths.is_empty() {
            return Err(format!(
                "{}='{}' contains no usable path",
                VAULT_BASE_PATHS_ENV, raw
            ));
        }
        Ok(base_paths)
    }

    /// Collect every markdown page under one vault-relative prefix.
    fn collect_under(&self, prefix: &str) -> Result<Vec<CorpusPage>, String> {
        #[cfg(not(feature = "shared-corpus-layout"))]
        let dir = self.root.join(prefix);
        #[cfg(feature = "shared-corpus-layout")]
        let dir = {
            let roots = self.checked_roots()?;
            let requested =
                ontomark_compile::profile::resolve_roots(&self.root, &[PathBuf::from(prefix)])
                    .map_err(|e| format!("invalid corpus prefix: {e}"))?;
            let dir = requested
                .into_iter()
                .next()
                .expect("one requested prefix produces one path");
            if !roots.iter().any(|base| dir.starts_with(base)) {
                return Err(format!(
                    "corpus prefix {prefix} is outside the configured source roots"
                ));
            }
            dir
        };
        if !dir.is_dir() {
            return Err(format!("no such corpus path: {}", dir.display()));
        }
        let mut pages = Vec::new();
        walk(&dir, prefix, &self.slash_paths, &mut pages)?;
        pages.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(pages)
    }

    /// Validate configuration through the shared owner before reading any source.
    #[cfg(feature = "shared-corpus-layout")]
    fn checked_roots(&self) -> Result<Vec<PathBuf>, String> {
        ontomark_compile::profile::resolve_roots(&self.root, &self.base_paths)
            .map_err(|e| format!("invalid corpus roots: {e}"))
    }
}

/// Split a comma-separated base-path list, trimming whitespace and `/`.
fn parse_base_paths(raw: &str) -> Vec<PathBuf> {
    raw.split(',')
        .map(|p| p.trim().trim_matches('/'))
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Render a path with `/` separators, as GitHub paths and vault identities use.
fn path_to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// `mtime:size`, the local change marker. Stable across runs for an untouched
/// file and different the moment the file is rewritten.
fn change_marker(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format!("{}.{:09}", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|| "0".to_string());
    format!("{}:{}", mtime, meta.len())
}

/// Recursive walk rooted at `dir`, emitting vault-relative paths prefixed by
/// `rel_prefix`. A folder is pruned by the shared ingest rule
/// ([`visionclaw_domain::vault::is_excluded_from_ingest`], judged relative to its base path
/// in `bases`), the rule the GitHub listings and the sync's identity check use too.
fn walk(
    dir: &Path,
    rel_prefix: &str,
    bases: &[String],
    out: &mut Vec<CorpusPage>,
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry under {}: {}", dir.display(), e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();

        // Match Ontomark's page walk: configured roots may resolve symlinks,
        // but child symlinks are not followed (including cycles and escapes).
        #[cfg(feature = "shared-corpus-layout")]
        if entry
            .file_type()
            .map_err(|e| format!("file type {}: {e}", path.display()))?
            .is_symlink()
        {
            continue;
        }
        // The legacy feature-off path retains its existing link behaviour.
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                debug!("skipping unreadable vault entry {}: {}", path.display(), e);
                continue;
            }
        };

        let rel = format!("{}/{}", rel_prefix, name);

        if meta.is_dir() {
            if visionclaw_domain::vault::is_repo_path_excluded_from_ingest(&rel, bases) {
                debug!("skipping excluded vault directory: {}", rel);
                continue;
            }
            walk(&path, &rel, bases, out)?;
        } else if meta.is_file() {
            if !name.to_ascii_lowercase().ends_with(".md") || name.starts_with('.') {
                continue;
            }
            out.push(CorpusPage {
                name,
                path: rel,
                change_marker: change_marker(&meta),
                size: meta.len(),
                fetch_ref: path.to_string_lossy().to_string(),
            });
        }
    }
    Ok(())
}

#[async_trait]
impl CorpusSource for LocalDirectorySource {
    fn describe(&self) -> SourceDescriptor {
        SourceDescriptor {
            kind: "local",
            location: self.root.to_string_lossy().to_string(),
            base_paths: self.slash_paths.clone(),
        }
    }

    fn base_paths(&self) -> &[String] {
        &self.slash_paths
    }

    async fn list_pages(&self) -> Result<Vec<CorpusPage>, String> {
        #[cfg(feature = "shared-corpus-layout")]
        self.checked_roots()?;
        let mut all = Vec::new();
        for prefix in &self.slash_paths {
            match self.collect_under(prefix) {
                Ok(pages) => {
                    info!("Vault source: {} pages under {}", pages.len(), prefix);
                    all.extend(pages);
                }
                Err(e) => {
                    // A vault with only one of the two halves mounted is a
                    // legitimate configuration; the missing half is logged and
                    // skipped rather than failing the whole sync.
                    log::warn!("Vault source: {} — skipping this source path", e);
                }
            }
        }
        if all.is_empty() {
            return Err(format!(
                "no markdown pages found under {} (sources: {})",
                self.root.display(),
                self.slash_paths.join(", ")
            ));
        }
        Ok(all)
    }

    async fn list_pages_under(&self, prefix: &str) -> Result<Vec<CorpusPage>, String> {
        self.collect_under(prefix.trim_matches('/'))
    }

    async fn fetch_page(&self, page: &CorpusPage) -> Result<String, String> {
        let path = PathBuf::from(&page.fetch_ref);
        #[cfg(feature = "shared-corpus-layout")]
        let path = {
            let resolved = std::fs::canonicalize(&path)
                .map_err(|e| format!("Failed to resolve {}: {e}", path.display()))?;
            if !self
                .checked_roots()?
                .iter()
                .any(|base| resolved.starts_with(base))
            {
                return Err(format!(
                    "page {} is outside the configured corpus roots",
                    path.display()
                ));
            }
            resolved
        };
        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
    }

    /// `<root>/ontology/vocabulary.yaml`, parsed by `vault_core` — the same
    /// vocabulary model `vault build` projects the corpus through.
    async fn vocabulary(&self) -> Result<Option<Vocabulary>, String> {
        let path = self.root.join(VOCABULARY_PATH);
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("Failed to read {}: {}", path.display(), e)),
        };
        Vocabulary::from_yaml_str(&text)
            .map(Some)
            .map_err(|e| format!("Invalid vocabulary {}: {}", path.display(), e))
    }

    /// One `git log` over the vault; the vault is a git checkout in the default deployment. No
    /// git binary (the production images ship none) or no repository means no history.
    async fn page_history(&self, _pages: &[CorpusPage]) -> super::HistoryMap {
        match super::history::git_history(&self.root, &self.slash_paths).await {
            Ok(h) => {
                log::info!("Corpus history: {} pages with commit times", h.len());
                h
            }
            Err(e) => {
                log::warn!("Corpus history unavailable, temporal keys omitted: {}", e);
                super::HistoryMap::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A flat vault with both halves, the Obsidian furniture at each root
    /// (`.obsidian/`, `templates/`, `bases/`, `assets/`, `journals/`), an
    /// `_misc` namespace (ingested: unpublished is not unloaded), a nested page and a
    /// non-markdown asset.
    fn fixture_vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let write = |rel: &str, body: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        };

        write("knowledge/Alpha.md", "---\npublic: true\n---\nalpha\n");
        write("knowledge/Beta.md", "---\npublic: true\n---\nbeta\n");
        write("knowledge/nested/Gamma.md", "nested gamma\n");
        write("knowledge/.obsidian/workspace.json", "{}");
        write("knowledge/.obsidian/types.md", "not a page");
        write("knowledge/_misc/Scratch.md", "unpublished, still a page");
        write("knowledge/templates/Class.md", "---\ntype: Class\n---\n");
        write("knowledge/bases/All Classes.md", "not a page");
        write("knowledge/assets/diagram.md", "not a page");
        write("knowledge/assets/diagram.png", "not markdown");
        write("knowledge/.hidden.md", "hidden");
        write("working/Journal Entry.md", "working page\n");
        write("working/podcast-evidence/Episode One.md", "evidence\n");
        write("working/journals/2026-09-22.md", "journal");
        write("working/templates/Daily.md", "template");
        write("elsewhere/Unreachable.md", "outside the base paths");
        dir
    }

    fn source(root: &Path) -> LocalDirectorySource {
        LocalDirectorySource::new(
            root,
            vec![PathBuf::from("knowledge"), PathBuf::from("working")],
        )
    }

    #[tokio::test]
    async fn lists_both_base_paths_and_skips_non_content() {
        let vault = fixture_vault();
        let pages = source(vault.path()).list_pages().await.unwrap();
        let paths: HashSet<&str> = pages.iter().map(|p| p.path.as_str()).collect();

        assert_eq!(
            paths,
            HashSet::from([
                "knowledge/Alpha.md",
                "knowledge/Beta.md",
                "knowledge/nested/Gamma.md",
                "knowledge/_misc/Scratch.md",
                "working/Journal Entry.md",
                "working/podcast-evidence/Episode One.md",
            ]),
            "both vaults ingest with nested namespaces and _misc kept; .obsidian, \
             the furniture folders, dotfiles, non-markdown and out-of-base paths do not"
        );
    }

    /// The walk prunes with the shared rule (`is_excluded_from_ingest`), so it agrees with the
    /// GitHub listings: `_misc/` and a namespace named like furniture below the root are
    /// pages; a `bak/` folder at any depth is not.
    #[tokio::test]
    async fn the_walk_uses_the_shared_exclusion_rule() {
        let vault = fixture_vault();
        let write = |rel: &str| {
            let path = vault.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "page").unwrap();
        };
        write("knowledge/Ns/assets/Page.md");
        write("knowledge/Ns/bak/Old.md");
        write("knowledge/bak/Old.md");
        let pages = source(vault.path()).list_pages().await.unwrap();
        let paths: HashSet<&str> = pages.iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains("knowledge/_misc/Scratch.md"), "{paths:?}");
        assert!(paths.contains("knowledge/Ns/assets/Page.md"), "{paths:?}");
        assert!(!paths.contains("knowledge/Ns/bak/Old.md"), "{paths:?}");
        assert!(!paths.contains("knowledge/bak/Old.md"), "{paths:?}");
    }

    /// One assertion per furniture folder, so a regression names the folder.
    #[tokio::test]
    async fn each_furniture_folder_is_skipped() {
        let vault = fixture_vault();
        let pages = source(vault.path()).list_pages().await.unwrap();
        for furniture in [
            "knowledge/templates/",
            "knowledge/bases/",
            "knowledge/assets/",
            "working/journals/",
            "working/templates/",
            "knowledge/.obsidian/",
        ] {
            assert!(
                pages.iter().all(|p| !p.path.starts_with(furniture)),
                "{furniture} must yield no page"
            );
        }
    }

    #[tokio::test]
    async fn pages_carry_name_size_and_a_readable_fetch_ref() {
        let vault = fixture_vault();
        let src = source(vault.path());
        let pages = src.list_pages().await.unwrap();
        let alpha = pages
            .iter()
            .find(|p| p.path == "knowledge/Alpha.md")
            .unwrap();

        assert_eq!(alpha.name, "Alpha.md");
        assert_eq!(alpha.size, "---\npublic: true\n---\nalpha\n".len() as u64);
        assert_eq!(
            src.fetch_page(alpha).await.unwrap(),
            "---\npublic: true\n---\nalpha\n"
        );
    }

    #[tokio::test]
    async fn the_change_marker_is_stable_across_runs_and_moves_on_edit() {
        let vault = fixture_vault();
        let src = source(vault.path());

        let first = src.list_pages().await.unwrap();
        let second = src.list_pages().await.unwrap();
        assert_eq!(first, second, "an untouched vault lists identically");

        std::fs::write(
            vault.path().join("knowledge/Alpha.md"),
            "---\npublic: true\n---\nalpha rewritten\n",
        )
        .unwrap();

        let third = src.list_pages().await.unwrap();
        let marker_of = |pages: &[CorpusPage], path: &str| {
            pages
                .iter()
                .find(|p| p.path == path)
                .unwrap()
                .change_marker
                .clone()
        };
        assert_ne!(
            marker_of(&first, "knowledge/Alpha.md"),
            marker_of(&third, "knowledge/Alpha.md"),
            "a rewritten page gets a new marker"
        );
        assert_eq!(
            marker_of(&first, "knowledge/Beta.md"),
            marker_of(&third, "knowledge/Beta.md"),
            "an untouched sibling does not"
        );
    }

    #[tokio::test]
    async fn listing_is_ordered_and_scoped_by_prefix() {
        let vault = fixture_vault();
        let src = source(vault.path());

        let under = src.list_pages_under("knowledge").await.unwrap();
        let paths: Vec<&str> = under.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "knowledge/Alpha.md",
                "knowledge/Beta.md",
                "knowledge/_misc/Scratch.md",
                "knowledge/nested/Gamma.md",
            ]
        );

        assert!(
            src.list_pages_under("knowledge/decisions").await.is_err(),
            "an absent namespace is an error, not an empty list"
        );
    }

    #[tokio::test]
    async fn a_missing_half_is_skipped_not_fatal() {
        let vault = fixture_vault();
        std::fs::remove_dir_all(vault.path().join("working")).unwrap();
        let pages = source(vault.path()).list_pages().await.unwrap();
        assert!(pages.iter().all(|p| p.path.starts_with("knowledge/")));
        assert_eq!(pages.len(), 4);
    }

    #[tokio::test]
    async fn an_empty_vault_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(source(dir.path()).list_pages().await.is_err());
    }

    #[test]
    fn the_descriptor_names_the_root_and_the_sources() {
        let src = source(Path::new("/vault/visionGraph"));
        let d = src.describe();
        assert_eq!(d.kind, "local");
        assert_eq!(d.location, "/vault/visionGraph");
        assert_eq!(d.base_paths, vec!["knowledge", "working"]);
        assert_eq!(src.base_paths(), d.base_paths.as_slice());
    }

    #[cfg(feature = "shared-corpus-layout")]
    #[tokio::test]
    async fn shared_layout_rejects_aliases_nested_roots_and_parent_escapes() {
        let vault = fixture_vault();
        for roots in [
            ["knowledge", "./knowledge"],
            ["knowledge", "knowledge/nested"],
            ["knowledge", "../outside"],
        ] {
            let src = LocalDirectorySource::new(vault.path(), roots.map(PathBuf::from).to_vec());
            assert!(
                src.list_pages().await.is_err(),
                "invalid roots must fail before partial ingest: {roots:?}"
            );
        }
    }

    #[cfg(feature = "shared-corpus-layout")]
    #[tokio::test]
    async fn shared_layout_rejects_unconfigured_prefixes_and_fetches() {
        let vault = fixture_vault();
        let src = source(vault.path());
        assert!(src.list_pages_under("elsewhere").await.is_err());
        assert!(src
            .list_pages_under("knowledge/../elsewhere")
            .await
            .is_err());
        let mut page = src.list_pages().await.unwrap().remove(0);
        page.fetch_ref = vault
            .path()
            .join("elsewhere/Unreachable.md")
            .to_string_lossy()
            .into_owned();
        assert!(src.fetch_page(&page).await.is_err());
        assert_eq!(
            src.list_pages_under("knowledge/nested")
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[cfg(all(unix, feature = "shared-corpus-layout"))]
    #[tokio::test]
    async fn shared_layout_rejects_linked_root_aliases_and_escapes() {
        let vault = fixture_vault();
        let outside = fixture_vault();
        std::os::unix::fs::symlink("knowledge", vault.path().join("alias")).unwrap();
        std::os::unix::fs::symlink(outside.path(), vault.path().join("escape")).unwrap();
        for roots in [["knowledge", "alias"], ["knowledge", "escape"]] {
            let src = LocalDirectorySource::new(vault.path(), roots.map(PathBuf::from).to_vec());
            assert!(src.list_pages().await.is_err());
        }
    }

    #[cfg(all(unix, feature = "shared-corpus-layout"))]
    #[tokio::test]
    async fn shared_layout_skips_child_symlinks_like_the_owner_walk() {
        let vault = fixture_vault();
        let outside = fixture_vault();
        let src = source(vault.path());
        let before = src.list_pages().await.unwrap();
        std::os::unix::fs::symlink(outside.path(), vault.path().join("knowledge/external"))
            .unwrap();
        std::os::unix::fs::symlink("Alpha.md", vault.path().join("knowledge/alias.md")).unwrap();
        // Do not introduce a cycle until a finite linked-directory case passes:
        // the legacy walker recurses through symlinks.
        assert_eq!(src.list_pages().await.unwrap(), before);
        std::os::unix::fs::symlink(".", vault.path().join("knowledge/cycle")).unwrap();
        assert_eq!(src.list_pages().await.unwrap(), before);
    }

    #[test]
    fn base_paths_parse_from_a_comma_separated_list() {
        assert_eq!(
            parse_base_paths(" knowledge , /working/ ,, "),
            vec![PathBuf::from("knowledge"), PathBuf::from("working")]
        );
        assert!(parse_base_paths(" , ").is_empty());
    }
}
