use super::api::GitHubClient;
use super::types::GitHubFileBasicMetadata;
use super::url_path::{contents_url, raw_download_url};
use crate::errors::VisionClawResult;
use crate::utils::time;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct EnhancedContentAPI {
    client: Arc<GitHubClient>,
}

impl EnhancedContentAPI {
    pub fn new(client: Arc<GitHubClient>) -> Self {
        Self { client }
    }

    /// The configured GitHub source prefixes (e.g. `mainKnowledgeGraph/pages`,
    /// `workingGraph/pages`). The sync strips these to derive a page's
    /// vault-relative identity (ADR-2040 §V1).
    pub fn base_paths(&self) -> &[String] {
        self.client.base_paths()
    }

    /// `owner/repo@branch` — how the GitHub corpus source identifies itself in
    /// telemetry and in the persisted sync-source identity.
    pub(crate) fn location(&self) -> String {
        format!(
            "{}/{}@{}",
            self.client.owner(),
            self.client.repo(),
            self.client.branch()
        )
    }

    /// List all markdown files using GitHub's Git Trees API (single API call).
    /// Returns all .md files under the configured base_path with their SHA hashes.
    /// This replaces the recursive Contents API approach that required one call per directory.
    pub async fn list_markdown_files_via_tree(
        &self,
    ) -> VisionClawResult<Vec<GitHubFileBasicMetadata>> {
        // Dual-source ingest: a single recursive tree call returns the whole
        // repo; keep every .md file under ANY configured source path. An empty
        // / "/" prefix means no filtering (whole repo).
        let base_prefixes: Vec<String> = self
            .client
            .base_paths()
            .iter()
            .map(|p| p.trim_matches('/').to_string())
            .filter(|p| !p.is_empty() && p != "/")
            .map(|p| format!("{}/", p))
            .collect();
        let branch = self.client.branch();

        // Git Trees API with recursive=1 returns the entire tree in one call
        let tree_url = format!(
            "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
            self.client.owner(),
            self.client.repo(),
            branch
        );

        info!(
            "list_markdown_files_via_tree: Fetching tree from: {}",
            tree_url
        );

        let response = self
            .client
            .client()
            .get(&tree_url)
            .header("Authorization", format!("Bearer {}", self.client.token()))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await?;
            error!(
                "list_markdown_files_via_tree: GitHub API error ({}): {}",
                status, error_text
            );
            return Err(format!("GitHub Trees API error ({}): {}", status, error_text).into());
        }

        let tree_data: Value = response.json().await?;
        let truncated = tree_data["truncated"].as_bool().unwrap_or(false);
        if truncated {
            warn!("list_markdown_files_via_tree: Tree response was truncated - some files may be missing");
        }

        let tree = tree_data["tree"]
            .as_array()
            .ok_or("GitHub Trees API returned no tree array")?;

        info!(
            "list_markdown_files_via_tree: Tree contains {} entries",
            tree.len()
        );

        let mut markdown_files = Vec::new();

        for entry in tree {
            let entry_type = entry["type"].as_str().unwrap_or("");
            let entry_path = entry["path"].as_str().unwrap_or("");

            // Only process blob (file) entries that are .md files under a source path
            if entry_type != "blob" || !entry_path.ends_with(".md") {
                continue;
            }

            // Keep the file if it sits under ANY configured source path. An
            // empty prefix list means no filtering (ingest the whole repo).
            if !base_prefixes.is_empty()
                && !base_prefixes
                    .iter()
                    .any(|prefix| entry_path.starts_with(prefix.as_str()))
            {
                continue;
            }

            // Skip backup, app-config and non-content paths (ADR-2040 D6:
            // `/.obsidian/` and `/.trash/` join the legacy Logseq exclusions).
            if entry_path.contains("/bak/")
                || entry_path.contains("/logseq/")
                || entry_path.contains("/.recycle/")
                || entry_path.contains("/journals/")
                || entry_path.contains("/.obsidian/")
                || entry_path.contains("/.trash/")
            {
                continue;
            }

            let sha = entry["sha"].as_str().unwrap_or("").to_string();
            let size = entry["size"].as_u64().unwrap_or(0);

            // Extract filename from path
            let name = entry_path
                .rsplit('/')
                .next()
                .unwrap_or(entry_path)
                .to_string();

            // Construct download URL from path. The path is percent-encoded
            // per segment: a filename may contain a literal `%`, which an
            // unescaped interpolation turns into a bogus escape (400) or a
            // different path (404). See `encode_repo_path`.
            let download_url =
                raw_download_url(self.client.owner(), self.client.repo(), branch, entry_path);

            markdown_files.push(GitHubFileBasicMetadata {
                name,
                path: entry_path.to_string(),
                sha,
                size,
                download_url,
            });
        }

        info!(
            "list_markdown_files_via_tree: Found {} markdown files under sources {:?}",
            markdown_files.len(),
            base_prefixes
        );
        Ok(markdown_files)
    }

    pub fn list_markdown_files<'a>(
        &'a self,
        path: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = VisionClawResult<Vec<GitHubFileBasicMetadata>>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { self.list_markdown_files_impl(path).await })
    }

    async fn list_markdown_files_impl(
        &self,
        path: &str,
    ) -> VisionClawResult<Vec<GitHubFileBasicMetadata>> {
        let mut all_markdown_files = Vec::new();

        // GitHub Contents API returns all items in a single response (no pagination).
        // per_page/page params are ignored by this endpoint.
        let contents_url = GitHubClient::get_contents_url(&self.client, path).await;

        debug!("list_markdown_files: Fetching from: {}", contents_url);

        let response = self
            .client
            .client()
            .get(&contents_url)
            .header("Authorization", format!("Bearer {}", self.client.token()))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;

        let status = response.status();
        debug!("list_markdown_files: Response status: {}", status);

        if !status.is_success() {
            let error_text = response.text().await?;
            error!(
                "list_markdown_files: GitHub API error for path '{}' ({}): {}",
                path, status, error_text
            );
            return Err(format!(
                "GitHub API error listing files for '{}' ({}): {}",
                path, status, error_text
            )
            .into());
        }

        let files: Vec<Value> = response.json().await?;
        info!(
            "list_markdown_files: Received {} items from GitHub for path '{}'",
            files.len(),
            path
        );

        for file in files {
            let file_type = file["type"].as_str().unwrap_or("unknown");
            let file_name = file["name"].as_str().unwrap_or("unnamed");

            if file_type == "file" && file_name.ends_with(".md") {
                debug!("list_markdown_files: Found markdown file: {}", file_name);
                all_markdown_files.push(GitHubFileBasicMetadata {
                    name: file_name.to_string(),
                    path: file["path"].as_str().unwrap_or("").to_string(),
                    sha: file["sha"].as_str().unwrap_or("").to_string(),
                    size: file["size"].as_u64().unwrap_or(0),
                    download_url: file["download_url"].as_str().unwrap_or("").to_string(),
                });
            } else if file_type == "dir" {
                let dir_path = file["path"].as_str().unwrap_or("");

                // Skip backup, recycle, journal, and app-config directories
                // (ADR-2040 D6 adds `.obsidian` and `.trash`).
                if dir_path.contains("/bak")
                    || dir_path.contains("/logseq/")
                    || dir_path.contains("/.recycle")
                    || dir_path.contains("/journals")
                    || dir_path.contains("/.obsidian")
                    || dir_path.contains("/.trash")
                {
                    debug!(
                        "list_markdown_files: Skipping excluded directory: {}",
                        dir_path
                    );
                    continue;
                }

                debug!(
                    "list_markdown_files: Recursively processing directory: {}",
                    dir_path
                );

                match self.list_markdown_files(dir_path).await {
                    Ok(mut subdir_files) => {
                        let count = subdir_files.len();
                        debug!(
                            "list_markdown_files: Found {} files in subdirectory {}",
                            count, dir_path
                        );
                        all_markdown_files.append(&mut subdir_files);
                    }
                    Err(e) => {
                        warn!(
                            "list_markdown_files: Failed to process subdirectory {}: {}",
                            dir_path, e
                        );
                    }
                }
            }
        }

        info!(
            "list_markdown_files: Found {} markdown files total for path '{}'",
            all_markdown_files.len(),
            path
        );
        Ok(all_markdown_files)
    }

    pub async fn fetch_file_content(&self, download_url: &str) -> VisionClawResult<String> {
        debug!("Fetching file content from: {}", download_url);
        let response = self
            .client
            .client()
            .get(download_url)
            .header("Authorization", format!("Bearer {}", self.client.token()))
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to fetch file content: {}", error_text).into());
        }

        Ok(response.text().await?)
    }

    pub async fn get_file_content_last_modified(
        &self,
        file_path: &str,
        check_actual_changes: bool,
    ) -> VisionClawResult<DateTime<Utc>> {
        // A RAW repository path. It is passed as a query parameter below, which
        // reqwest percent-encodes itself — encoding it here would double-encode.
        let repo_path = GitHubClient::get_full_path(&self.client, file_path).await;

        let commits_url = format!(
            "https://api.github.com/repos/{}/{}/commits",
            self.client.owner(),
            self.client.repo()
        );

        debug!("Fetching commits for path: {}", repo_path);

        let response = self
            .client
            .client()
            .get(&commits_url)
            .header("Authorization", format!("Bearer {}", self.client.token()))
            .header("Accept", "application/vnd.github+json")
            .query(&[
                ("path", repo_path.as_str()),
                ("ref", self.client.branch()),
                ("per_page", if check_actual_changes { "10" } else { "1" }),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("GitHub API error: {}", error_text).into());
        }

        let commits: Vec<Value> = response.json().await?;

        if commits.is_empty() {
            return Err(format!("No commit history found for {}", file_path).into());
        }

        if !check_actual_changes {
            return self.extract_commit_date(&commits[0]);
        }

        for commit in &commits {
            let sha = commit["sha"].as_str().ok_or("Missing commit SHA")?;

            if self.was_file_modified_in_commit(sha, &repo_path).await? {
                debug!("File was actually modified in commit: {}", sha);
                return self.extract_commit_date(commit);
            } else {
                debug!(
                    "File was not modified in commit: {} (likely a merge commit)",
                    sha
                );
            }
        }

        warn!("No actual content changes found in recent commits, using oldest available");
        self.extract_commit_date(&commits[commits.len() - 1])
    }

    async fn was_file_modified_in_commit(
        &self,
        commit_sha: &str,
        file_path: &str,
    ) -> VisionClawResult<bool> {
        let commit_url = format!(
            "https://api.github.com/repos/{}/{}/commits/{}",
            self.client.owner(),
            self.client.repo(),
            commit_sha
        );

        debug!("Checking commit {} for file changes", commit_sha);

        let response = self
            .client
            .client()
            .get(&commit_url)
            .header("Authorization", format!("Bearer {}", self.client.token()))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            warn!("Failed to get commit details: {}", error_text);

            return Ok(true);
        }

        let commit_data: Value = response.json().await?;

        if let Some(files) = commit_data["files"].as_array() {
            for file in files {
                if let Some(filename) = file["filename"].as_str() {
                    // Namespace decode (ADR-2040 §V1): a vault page may be
                    // addressed by its folder path while the commit still names
                    // the legacy encoded file, or vice versa. Both `%2F` and
                    // `___` decode to `/`.
                    let decoded = file_path.replace("%2F", "/").replace("___", "/");
                    if filename == file_path
                        || filename.ends_with(&format!("/{}", file_path))
                        || filename == decoded
                        || filename.ends_with(&format!("/{}", decoded))
                    {
                        let additions = file["additions"].as_u64().unwrap_or(0);
                        let deletions = file["deletions"].as_u64().unwrap_or(0);
                        let changes = file["changes"].as_u64().unwrap_or(0);

                        debug!(
                            "File {} in commit {}: +{} -{} (total: {} changes)",
                            filename, commit_sha, additions, deletions, changes
                        );

                        return Ok(changes > 0);
                    }
                }
            }
        }

        Ok(false)
    }

    fn extract_commit_date(&self, commit: &Value) -> VisionClawResult<DateTime<Utc>> {
        let date_str = commit["commit"]["committer"]["date"]
            .as_str()
            .or_else(|| commit["commit"]["author"]["date"].as_str())
            .ok_or("No commit date found")?;

        DateTime::parse_from_rfc3339(date_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| format!("Failed to parse date {}: {}", date_str, e).into())
    }

    pub async fn get_file_metadata_extended(
        &self,
        file_path: &str,
    ) -> VisionClawResult<ExtendedFileMetadata> {
        let repo_path = GitHubClient::get_full_path(&self.client, file_path).await;

        let contents_url = contents_url(
            self.client.owner(),
            self.client.repo(),
            &repo_path,
            Some(self.client.branch()),
        );

        let response = self
            .client
            .client()
            .get(&contents_url)
            .header("Authorization", format!("Bearer {}", self.client.token()))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to get file metadata: {}", error_text).into());
        }

        let content_data: Value = response.json().await?;

        let last_content_modified = match self.get_file_content_last_modified(file_path, true).await
        {
            Ok(date) => date,
            Err(e) => {
                debug!(
                    "Could not get commit history for {}: {}. Using current time.",
                    file_path, e
                );
                time::now()
            }
        };

        Ok(ExtendedFileMetadata {
            name: content_data["name"].as_str().unwrap_or("").to_string(),
            path: content_data["path"].as_str().unwrap_or("").to_string(),
            sha: content_data["sha"].as_str().unwrap_or("").to_string(),
            size: content_data["size"].as_u64().unwrap_or(0),
            download_url: content_data["download_url"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            last_content_modified,
            file_type: content_data["type"].as_str().unwrap_or("file").to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ExtendedFileMetadata {
    pub name: String,
    pub path: String,
    pub sha: String,
    pub size: u64,
    pub download_url: String,
    pub last_content_modified: DateTime<Utc>,
    pub file_type: String,
}
