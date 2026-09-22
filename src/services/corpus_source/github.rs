// src/services/corpus_source/github.rs
//! [`CorpusSource`] over the GitHub corpus repository — the pre-ADR-2114
//! behaviour, unchanged, now expressed through the port.

use async_trait::async_trait;
use log::{info, warn};
use std::sync::Arc;

use super::{CorpusPage, CorpusSource, SourceDescriptor};
use crate::services::github::content_enhanced::EnhancedContentAPI;
use crate::services::github::types::GitHubFileBasicMetadata;

/// Reads the corpus from a GitHub repository via the Trees API (with a
/// Contents API fallback), keyed on blob SHAs.
pub struct GitHubSource {
    content_api: Arc<EnhancedContentAPI>,
    base_paths: Vec<String>,
    location: String,
}

impl GitHubSource {
    /// Wrap a configured content API.
    pub fn new(content_api: Arc<EnhancedContentAPI>) -> Self {
        let base_paths = content_api.base_paths().to_vec();
        let location = content_api.location();
        Self {
            content_api,
            base_paths,
            location,
        }
    }
}

/// The GitHub blob SHA is the change marker; the raw download URL is the
/// fetch handle.
fn page_from_metadata(file: GitHubFileBasicMetadata) -> CorpusPage {
    CorpusPage {
        name: file.name,
        path: file.path,
        change_marker: file.sha,
        size: file.size,
        fetch_ref: file.download_url,
    }
}

#[async_trait]
impl CorpusSource for GitHubSource {
    fn describe(&self) -> SourceDescriptor {
        SourceDescriptor {
            kind: "github",
            location: self.location.clone(),
            base_paths: self.base_paths.clone(),
        }
    }

    fn base_paths(&self) -> &[String] {
        &self.base_paths
    }

    async fn list_pages(&self) -> Result<Vec<CorpusPage>, String> {
        let files = match self.content_api.list_markdown_files_via_tree().await {
            Ok(files) => {
                info!("Trees API returned {} markdown files", files.len());
                files
            }
            Err(e) => {
                warn!("Trees API failed ({}), falling back to Contents API", e);
                self.content_api
                    .list_markdown_files("")
                    .await
                    .map_err(|e| format!("GitHub API error: {}", e))?
            }
        };
        Ok(files.into_iter().map(page_from_metadata).collect())
    }

    async fn list_pages_under(&self, prefix: &str) -> Result<Vec<CorpusPage>, String> {
        let files = self
            .content_api
            .list_markdown_files(prefix)
            .await
            .map_err(|e| format!("GitHub API error: {}", e))?;
        Ok(files.into_iter().map(page_from_metadata).collect())
    }

    async fn fetch_page(&self, page: &CorpusPage) -> Result<String, String> {
        self.content_api
            .fetch_file_content(&page.fetch_ref)
            .await
            .map_err(|e| format!("Failed to fetch content: {}", e))
    }
}
