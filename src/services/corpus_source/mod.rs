// src/services/corpus_source/mod.rs
//! The corpus source port (ADR-2114).
//!
//! Ingest used to be spelled "GitHub": [`crate::services::github_sync_service`]
//! reached straight into the GitHub content API for the page listing, the change
//! marker and the page body. The sovereign-corpus work makes the *local vault*
//! the default source, so those three needs are lifted into one port —
//! [`CorpusSource`] — and the whole pipeline (parse → dual graph → post-sync
//! Whelk reasoning → inferred-edge materialisation → `ReloadGraphFromDatabase`)
//! runs unchanged over whichever implementation is selected.
//!
//! Two implementations ship:
//!
//! * [`LocalDirectorySource`] — walks `VAULT_ROOT`'s configured base paths on
//!   disk (the mounted vault volume). The default.
//! * [`GitHubSource`] — the pre-existing remote pull, behaviour unchanged.
//!
//! Selection is [`corpus_source_kind`] over `CORPUS_SOURCE`: `local` when
//! `VAULT_ROOT` is set, `github` otherwise.

mod github;
mod local;

pub use github::GitHubSource;
pub use local::{LocalDirectorySource, VAULT_BASE_PATHS_ENV, VAULT_ROOT_ENV};

use async_trait::async_trait;
use vault_core::vocabulary::Vocabulary;

/// One markdown page offered by a [`CorpusSource`].
///
/// The field set is exactly what the sync pipeline consumes: an identity
/// (`path`, resolved against the source's base paths into a vault identity), a
/// display name, a change marker for the incremental filter, and an opaque
/// handle the source itself understands for fetching the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusPage {
    /// File name including the `.md` extension.
    pub name: String,
    /// Source-relative path with `/` separators, e.g. `knowledge/pages/Foo.md`.
    /// Always prefixed by one of [`CorpusSource::base_paths`].
    pub path: String,
    /// Opaque change marker: the blob SHA for GitHub, `mtime:size` on disk.
    /// Equal markers across two listings mean the page did not change.
    pub change_marker: String,
    /// Size in bytes, for telemetry only.
    pub size: u64,
    /// Handle [`CorpusSource::fetch_page`] resolves: a download URL for GitHub,
    /// an absolute filesystem path for a local directory.
    pub fetch_ref: String,
}

/// How a source describes itself in logs, statistics and the sync-identity
/// marker that triggers a forced full re-sync when the source changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDescriptor {
    /// Stable discriminator: `github` or `local`.
    pub kind: &'static str,
    /// Where the corpus is read from: `owner/repo@branch` or a vault root.
    pub location: String,
    /// The configured ingest base paths, in order.
    pub base_paths: Vec<String>,
}

impl SourceDescriptor {
    /// The identity string persisted in the sync database. A change forces a
    /// full re-sync so a store built from one source is never topped up
    /// incrementally from another.
    pub fn identity(&self) -> String {
        format!(
            "{}:{}:{}",
            self.kind,
            self.location,
            self.base_paths.join(",")
        )
    }
}

impl std::fmt::Display for SourceDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} corpus at {} (sources: {})",
            self.kind,
            self.location,
            self.base_paths.join(", ")
        )
    }
}

/// Where the knowledge corpus is read from.
///
/// Implementations must be cheap to clone behind an `Arc` and safe to call
/// concurrently — the sync pipeline fetches eight pages at a time.
#[async_trait]
pub trait CorpusSource: Send + Sync {
    /// Self-description for telemetry and the sync-identity marker.
    fn describe(&self) -> SourceDescriptor;

    /// The configured ingest base paths, in order. Page identity is resolved
    /// relative to these, so a knowledge page and its working twin share one
    /// identity (ADR-2040 §V1).
    fn base_paths(&self) -> &[String];

    /// Every markdown page under every base path, with its change marker.
    async fn list_pages(&self) -> Result<Vec<CorpusPage>, String>;

    /// Every markdown page under one prefix (a namespace such as the decisions
    /// directory). An absent prefix is an error, not an empty list, so callers
    /// can tell "no such namespace" from "namespace is empty".
    async fn list_pages_under(&self, prefix: &str) -> Result<Vec<CorpusPage>, String>;

    /// Fetch one page's body via its [`CorpusPage::fetch_ref`].
    async fn fetch_page(&self, page: &CorpusPage) -> Result<String, String>;

    /// The corpus vocabulary (`ontology/vocabulary.yaml`, contract C1), which
    /// maps every frontmatter relation key to its OWL property.
    ///
    /// `Ok(None)` when the source cannot supply one — the default, and the
    /// GitHub source's answer. The sync then ingests pages and wikilinks but
    /// emits no typed relation edges. An `Err` is a vocabulary that exists
    /// but does not load, which must not be silently ignored.
    async fn vocabulary(&self) -> Result<Option<Vocabulary>, String> {
        Ok(None)
    }
}

/// Build the configured corpus source.
///
/// `github` is invoked lazily and only when the GitHub source is selected, so
/// a local-source process never needs GitHub credentials. When the local
/// source is selected but the vault is unconfigured, the error names
/// `VAULT_ROOT` rather than silently falling back to the network.
pub fn source_from_env<F>(github: F) -> Result<std::sync::Arc<dyn CorpusSource>, String>
where
    F: FnOnce() -> Result<std::sync::Arc<dyn CorpusSource>, String>,
{
    match corpus_source_kind() {
        CorpusSourceKind::Local => {
            let source = LocalDirectorySource::from_env()?;
            log::info!("Corpus source: {}", source.describe());
            Ok(std::sync::Arc::new(source))
        }
        CorpusSourceKind::GitHub => {
            let source = github()?;
            log::info!("Corpus source: {}", source.describe());
            Ok(source)
        }
    }
}

/// Build the configured corpus source for a server process, which always has
/// a GitHub content API to hand (possibly the disabled placeholder).
///
/// Boot-safe by construction: a misconfigured local source logs an error and
/// falls back to the GitHub source rather than failing startup.
pub fn source_from_env_with_github(
    content_api: std::sync::Arc<crate::services::github::content_enhanced::EnhancedContentAPI>,
) -> std::sync::Arc<dyn CorpusSource> {
    let github = || -> Result<std::sync::Arc<dyn CorpusSource>, String> {
        Ok(std::sync::Arc::new(GitHubSource::new(content_api.clone())))
    };
    match source_from_env(github) {
        Ok(source) => source,
        Err(e) => {
            log::error!(
                "Corpus source unavailable ({}) — falling back to the GitHub source;                  set {} to the vault mount point to ingest locally",
                e,
                VAULT_ROOT_ENV
            );
            std::sync::Arc::new(GitHubSource::new(content_api))
        }
    }
}

/// Which source the process should ingest from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusSourceKind {
    /// Walk a mounted vault directory.
    Local,
    /// Pull from the GitHub corpus repository.
    GitHub,
}

/// Environment variable selecting the corpus source: `local` or `github`.
pub const CORPUS_SOURCE_ENV: &str = "CORPUS_SOURCE";

/// Resolve the selected source from the environment.
///
/// `CORPUS_SOURCE=local|github` decides outright (case-insensitive). With the
/// variable unset the default is [`CorpusSourceKind::Local`] when `VAULT_ROOT`
/// is set and non-empty, otherwise [`CorpusSourceKind::GitHub`].
pub fn corpus_source_kind() -> CorpusSourceKind {
    resolve_source_kind(
        std::env::var(CORPUS_SOURCE_ENV).ok().as_deref(),
        std::env::var(VAULT_ROOT_ENV).ok().as_deref(),
    )
}

/// The pure half of [`corpus_source_kind`], exercised by the tests.
pub(crate) fn resolve_source_kind(
    corpus_source: Option<&str>,
    vault_root: Option<&str>,
) -> CorpusSourceKind {
    match corpus_source
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("local") => CorpusSourceKind::Local,
        Some("github") => CorpusSourceKind::GitHub,
        Some(other) if !other.is_empty() => {
            log::warn!(
                "{}='{}' is not recognised (expected local|github) — falling back to the default",
                CORPUS_SOURCE_ENV,
                other
            );
            default_source_kind(vault_root)
        }
        _ => default_source_kind(vault_root),
    }
}

fn default_source_kind(vault_root: Option<&str>) -> CorpusSourceKind {
    match vault_root {
        Some(root) if !root.trim().is_empty() => CorpusSourceKind::Local,
        _ => CorpusSourceKind::GitHub,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `set_var`/`remove_var` are process-wide; the env-driven tests below
    /// serialise on this.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn explicit_selection_wins_over_the_vault_root_default() {
        assert_eq!(
            resolve_source_kind(Some("github"), Some("/vault/visionGraph")),
            CorpusSourceKind::GitHub
        );
        assert_eq!(
            resolve_source_kind(Some("LOCAL"), None),
            CorpusSourceKind::Local
        );
    }

    #[test]
    fn the_default_follows_vault_root() {
        assert_eq!(
            resolve_source_kind(None, Some("/vault/visionGraph")),
            CorpusSourceKind::Local
        );
        assert_eq!(resolve_source_kind(None, None), CorpusSourceKind::GitHub);
        assert_eq!(
            resolve_source_kind(None, Some("  ")),
            CorpusSourceKind::GitHub
        );
    }

    #[test]
    fn an_unrecognised_value_falls_back_to_the_default() {
        assert_eq!(
            resolve_source_kind(Some("gitlab"), Some("/vault")),
            CorpusSourceKind::Local
        );
        assert_eq!(
            resolve_source_kind(Some(""), None),
            CorpusSourceKind::GitHub
        );
    }

    #[test]
    fn a_local_source_boots_without_any_github_configuration() {
        let _guard = ENV_LOCK.lock().unwrap();
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(vault.path().join("knowledge/pages")).unwrap();

        std::env::remove_var("PRIVATE_REPO_GITHUB_PAT");
        std::env::remove_var("GITHUB_OWNER");
        std::env::remove_var("GITHUB_REPO");
        std::env::remove_var("GITHUB_BASE_PATH");
        std::env::remove_var("GITHUB_BASE_PATHS");
        std::env::remove_var(CORPUS_SOURCE_ENV);
        std::env::remove_var(VAULT_BASE_PATHS_ENV);
        std::env::set_var(VAULT_ROOT_ENV, vault.path());

        let source = source_from_env(|| Err("GitHub must not be consulted".to_string()))
            .expect("the local source needs no GitHub configuration");
        let descriptor = source.describe();
        assert_eq!(descriptor.kind, "local");
        assert_eq!(
            descriptor.base_paths,
            vec!["knowledge/pages", "working/pages"],
            "the default dual-source split applies"
        );

        std::env::remove_var(VAULT_ROOT_ENV);
    }

    #[test]
    fn selecting_github_without_a_vault_uses_the_github_builder() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(CORPUS_SOURCE_ENV, "github");
        std::env::remove_var(VAULT_ROOT_ENV);

        match source_from_env(|| Err("no credentials".to_string())) {
            Err(e) => assert_eq!(
                e, "no credentials",
                "the GitHub builder is consulted and its error surfaces"
            ),
            Ok(_) => panic!("CORPUS_SOURCE=github must not build a local source"),
        }

        std::env::remove_var(CORPUS_SOURCE_ENV);
    }

    #[test]
    fn the_descriptor_identity_separates_sources() {
        let local = SourceDescriptor {
            kind: "local",
            location: "/vault/visionGraph".to_string(),
            base_paths: vec!["knowledge/pages".into(), "working/pages".into()],
        };
        let github = SourceDescriptor {
            kind: "github",
            location: "jjohare/visionGraph@main".to_string(),
            base_paths: vec!["knowledge/pages".into(), "working/pages".into()],
        };
        assert_ne!(local.identity(), github.identity());
        assert_eq!(
            local.identity(),
            "local:/vault/visionGraph:knowledge/pages,working/pages"
        );
    }
}
