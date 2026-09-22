// src/services/corpus_source/local.rs
//! [`CorpusSource`] over a mounted vault directory — the default source
//! (ADR-2114, PRD-sovereign-corpus Q2).

use async_trait::async_trait;
use log::{debug, info};
use std::path::{Path, PathBuf};

use super::{CorpusPage, CorpusSource, SourceDescriptor};

/// Environment variable naming the vault root on disk.
pub const VAULT_ROOT_ENV: &str = "VAULT_ROOT";
/// Environment variable naming the comma-separated ingest base paths,
/// relative to the vault root. Mirrors `GITHUB_BASE_PATHS`.
pub const VAULT_BASE_PATHS_ENV: &str = "VAULT_BASE_PATHS";
/// Base paths used when [`VAULT_BASE_PATHS_ENV`] is unset — the dual-source
/// split of the sovereign corpus.
pub const DEFAULT_VAULT_BASE_PATHS: &str = "knowledge/pages,working/pages";

/// Directory names never descended into: app config, backups, bins and the
/// non-content namespaces the vault contract excludes from KG ingest
/// (`docs/VAULT-corpus-format.md` V4, ADR-2040 D6). Any name starting with
/// `.` is skipped as well.
const SKIP_DIRS: &[&str] = &[
    ".obsidian",
    ".trash",
    ".recycle",
    ".git",
    "bak",
    "logseq",
    "journals",
    "_misc",
];

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
    /// (default `knowledge/pages,working/pages`).
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
        let dir = self.root.join(prefix);
        if !dir.is_dir() {
            return Err(format!("no such corpus path: {}", dir.display()));
        }
        let mut pages = Vec::new();
        walk(&dir, prefix, &mut pages)?;
        pages.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(pages)
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

/// Whether a directory name is excluded from the walk.
fn is_skipped_dir(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s))
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
/// `rel_prefix`.
fn walk(dir: &Path, rel_prefix: &str, out: &mut Vec<CorpusPage>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry under {}: {}", dir.display(), e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();

        // Symlinks are resolved once, so a linked page or directory still
        // ingests; `file_type` alone reports the link, not its target.
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                debug!("skipping unreadable vault entry {}: {}", path.display(), e);
                continue;
            }
        };

        let rel = format!("{}/{}", rel_prefix, name);

        if meta.is_dir() {
            if is_skipped_dir(&name) {
                debug!("skipping excluded vault directory: {}", rel);
                continue;
            }
            walk(&path, &rel, out)?;
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
        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A vault with both halves, an `.obsidian/` config dir, an `_misc`
    /// namespace, a nested page and a non-markdown asset.
    fn fixture_vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let write = |rel: &str, body: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        };

        write(
            "knowledge/pages/Alpha.md",
            "---\npublic: true\n---\nalpha\n",
        );
        write("knowledge/pages/Beta.md", "---\npublic: true\n---\nbeta\n");
        write("knowledge/pages/nested/Gamma.md", "nested gamma\n");
        write("knowledge/pages/.obsidian/workspace.json", "{}");
        write("knowledge/pages/.obsidian/types.md", "not a page");
        write("knowledge/pages/_misc/Scratch.md", "ignored");
        write("knowledge/pages/diagram.png", "not markdown");
        write("knowledge/pages/.hidden.md", "hidden");
        write("working/pages/Journal Entry.md", "working page\n");
        write("working/pages/journals/2026-09-22.md", "journal");
        write("elsewhere/pages/Unreachable.md", "outside the base paths");
        dir
    }

    fn source(root: &Path) -> LocalDirectorySource {
        LocalDirectorySource::new(
            root,
            vec![
                PathBuf::from("knowledge/pages"),
                PathBuf::from("working/pages"),
            ],
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
                "knowledge/pages/Alpha.md",
                "knowledge/pages/Beta.md",
                "knowledge/pages/nested/Gamma.md",
                "working/pages/Journal Entry.md",
            ]),
            "both halves ingest; .obsidian, _misc, journals, dotfiles, \
             non-markdown and out-of-base paths do not"
        );
    }

    #[tokio::test]
    async fn pages_carry_name_size_and_a_readable_fetch_ref() {
        let vault = fixture_vault();
        let src = source(vault.path());
        let pages = src.list_pages().await.unwrap();
        let alpha = pages
            .iter()
            .find(|p| p.path == "knowledge/pages/Alpha.md")
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
            vault.path().join("knowledge/pages/Alpha.md"),
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
            marker_of(&first, "knowledge/pages/Alpha.md"),
            marker_of(&third, "knowledge/pages/Alpha.md"),
            "a rewritten page gets a new marker"
        );
        assert_eq!(
            marker_of(&first, "knowledge/pages/Beta.md"),
            marker_of(&third, "knowledge/pages/Beta.md"),
            "an untouched sibling does not"
        );
    }

    #[tokio::test]
    async fn listing_is_ordered_and_scoped_by_prefix() {
        let vault = fixture_vault();
        let src = source(vault.path());

        let under = src.list_pages_under("knowledge/pages").await.unwrap();
        let paths: Vec<&str> = under.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "knowledge/pages/Alpha.md",
                "knowledge/pages/Beta.md",
                "knowledge/pages/nested/Gamma.md",
            ]
        );

        assert!(
            src.list_pages_under("knowledge/pages/decisions")
                .await
                .is_err(),
            "an absent namespace is an error, not an empty list"
        );
    }

    #[tokio::test]
    async fn a_missing_half_is_skipped_not_fatal() {
        let vault = fixture_vault();
        std::fs::remove_dir_all(vault.path().join("working")).unwrap();
        let pages = source(vault.path()).list_pages().await.unwrap();
        assert!(pages.iter().all(|p| p.path.starts_with("knowledge/pages/")));
        assert_eq!(pages.len(), 3);
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
        assert_eq!(d.base_paths, vec!["knowledge/pages", "working/pages"]);
        assert_eq!(src.base_paths(), d.base_paths.as_slice());
    }

    #[test]
    fn base_paths_parse_from_a_comma_separated_list() {
        assert_eq!(
            parse_base_paths(" knowledge/pages , /working/pages/ ,, "),
            vec![
                PathBuf::from("knowledge/pages"),
                PathBuf::from("working/pages")
            ]
        );
        assert!(parse_base_paths(" , ").is_empty());
    }
}
