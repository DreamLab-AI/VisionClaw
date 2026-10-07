//! GitHubPRService — Creates GitHub branches, commits, and pull requests
//! for agent-proposed ontology changes.
//!
//! Agents inside the container are authorized to write directly to GitHub.
//! This service uses the GitHub REST API (via reqwest) to:
//! 1. Get the base branch SHA
//! 2. Create a blob with the markdown content
//! 3. Create a tree with the file change
//! 4. Create a commit
//! 5. Create a branch reference
//! 6. Open a pull request
//!
//! Notes are per-user — each user's agents write to their own path namespace.

use crate::types::ontology_tools::AgentContext;
use log::{info, warn};
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::env;

pub struct GitHubPRService {
    token: String,
    owner: String,
    repo: String,
    base_branch: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct CreateBlobRequest {
    content: String,
    encoding: String,
}

#[derive(Debug, Deserialize)]
struct BlobResponse {
    sha: String,
}

#[derive(Debug, Serialize)]
struct CreateTreeRequest {
    base_tree: String,
    tree: Vec<TreeEntry>,
}

#[derive(Debug, Serialize)]
struct TreeEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    entry_type: String,
    sha: String,
}

#[derive(Debug, Deserialize)]
struct TreeResponse {
    sha: String,
}

#[derive(Debug, Serialize)]
struct CreateCommitRequest {
    message: String,
    tree: String,
    parents: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CommitResponse {
    sha: String,
}

#[derive(Debug, Serialize)]
struct CreateRefRequest {
    #[serde(rename = "ref")]
    ref_name: String,
    sha: String,
}

#[derive(Debug, Serialize)]
struct CreatePRRequest {
    title: String,
    body: String,
    head: String,
    base: String,
    labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PRResponse {
    html_url: String,
    number: u64,
}

#[derive(Debug, Deserialize)]
struct RefResponse {
    object: RefObject,
}

#[derive(Debug, Deserialize)]
struct RefObject {
    sha: String,
}

/// Terminal-or-open git state of an opened PR (GOV-2 merge detection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    /// Still open — keep polling.
    Open,
    /// Merged — the elevation committed to the corpus (the terminal success).
    Merged,
    /// Closed without merging — the elevation was abandoned.
    ClosedUnmerged,
}

/// GitHub `GET /pulls/{n}` projection: enough to classify [`PrState`].
#[derive(Debug, Deserialize)]
struct PrStateResponse {
    state: String,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    merged: Option<bool>,
}

impl GitHubPRService {
    pub fn new() -> Self {
        let token = crate::services::github::config::github_token_from_env().unwrap_or_default();
        let owner = env::var("GITHUB_OWNER")
            .or_else(|_| env::var("GITHUB_REPO_OWNER"))
            .unwrap_or_else(|_| {
                warn!("Neither GITHUB_OWNER nor GITHUB_REPO_OWNER set in .env");
                String::new()
            });
        let repo = env::var("GITHUB_REPO")
            .or_else(|_| env::var("GITHUB_REPO_NAME"))
            .unwrap_or_else(|_| {
                warn!("Neither GITHUB_REPO nor GITHUB_REPO_NAME set in .env");
                String::new()
            });
        let base_branch = env::var("GITHUB_BRANCH")
            .or_else(|_| env::var("GITHUB_BASE_BRANCH"))
            .unwrap_or_else(|_| "main".to_string());

        Self {
            token,
            owner,
            repo,
            base_branch,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_config(token: String, owner: String, repo: String, base_branch: String) -> Self {
        Self {
            token,
            owner,
            repo,
            base_branch,
            client: reqwest::Client::new(),
        }
    }

    /// Whether a GitHub write token is configured. GOV-2: the elevation actor
    /// logs loudly (degraded-visible) when this is false, because without it the
    /// merge poll can never resolve a PR to `concept_elevated`.
    pub fn has_github_token() -> bool {
        !crate::services::github::config::github_token_from_env()
            .unwrap_or_default()
            .is_empty()
    }

    /// Extract a PR number from a full html URL (`…/pull/123`) or a bare number
    /// string. Pure — unit-testable without the network.
    pub fn pr_number_from_ref(pr_ref: &str) -> Option<u64> {
        let t = pr_ref.trim();
        if let Ok(n) = t.parse::<u64>() {
            return Some(n);
        }
        if let Some(idx) = t.rfind("/pull/") {
            let tail = &t[idx + "/pull/".len()..];
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u64>() {
                return Some(n);
            }
        }
        // Fallback: last path segment that parses as a number.
        t.rsplit('/').find_map(|seg| seg.parse::<u64>().ok())
    }

    /// Pure classifier from the GitHub PR fields to [`PrState`]. `merged_at`
    /// present (or the `merged` bool true) ⇒ Merged; otherwise a `closed` state
    /// with no merge ⇒ ClosedUnmerged; anything else ⇒ Open. Factored out so the
    /// GOV-2 state transitions are testable without a live GitHub call.
    fn classify_pr_state(state: &str, merged_at: Option<&str>, merged: Option<bool>) -> PrState {
        if merged.unwrap_or(false) || merged_at.is_some() {
            PrState::Merged
        } else if state.eq_ignore_ascii_case("closed") {
            PrState::ClosedUnmerged
        } else {
            PrState::Open
        }
    }

    /// Poll the terminal-or-open git state of a previously opened PR (GOV-2).
    /// Accepts a full html URL (`…/pull/123`) or a bare PR number.
    pub async fn pr_state(&self, pr_ref: &str) -> Result<PrState, String> {
        if self.token.is_empty() {
            return Err(
                "PRIVATE_REPO_GITHUB_PAT not configured — cannot poll PR state".to_string(),
            );
        }
        let number = Self::pr_number_from_ref(pr_ref)
            .ok_or_else(|| format!("cannot extract a PR number from '{}'", pr_ref))?;
        let url = self.api_url(&format!("pulls/{}", number));

        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("Failed to get PR state: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Get PR state failed ({}): {}", status, body));
        }

        let pr: PrStateResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse PR state response: {}", e))?;

        Ok(Self::classify_pr_state(
            &pr.state,
            pr.merged_at.as_deref(),
            pr.merged,
        ))
    }

    fn api_url(&self, path: &str) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/{}",
            self.owner, self.repo, path
        )
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(auth_value) = format!("Bearer {}", self.token).parse() {
            headers.insert(AUTHORIZATION, auth_value);
        }
        headers.insert(
            ACCEPT,
            "application/vnd.github+json"
                .parse()
                .expect("static header value is always valid"),
        );
        headers.insert(
            USER_AGENT,
            "VisionClaw-OntologyAgent/1.0"
                .parse()
                .expect("static header value is always valid"),
        );
        headers
    }

    /// Create a full GitHub PR for an ontology change.
    ///
    /// Returns the PR URL on success.
    pub async fn create_ontology_pr(
        &self,
        file_path: &str,
        content: &str,
        title: &str,
        body: &str,
        agent_ctx: &AgentContext,
    ) -> Result<String, String> {
        if self.token.is_empty() {
            return Err("PRIVATE_REPO_GITHUB_PAT not configured — cannot create PR".to_string());
        }

        info!("Creating ontology PR: '{}' for file '{}'", title, file_path);

        // 1. Get base branch SHA
        let base_sha = self.get_ref_sha(&self.base_branch).await?;

        // 2. Create blob
        let blob_sha = self.create_blob(content).await?;

        // 3. Create tree
        let tree_sha = self.create_tree(&base_sha, file_path, &blob_sha).await?;

        // 4. Create commit
        let commit_message = format!(
            "{}\n\nAgent: {} ({})\nUser: {}\nTask: {}",
            title,
            agent_ctx.agent_type,
            agent_ctx.agent_id,
            agent_ctx.user_id,
            agent_ctx.task_description
        );
        let commit_sha = self
            .create_commit(&commit_message, &tree_sha, &base_sha)
            .await?;

        // 5. Create branch
        let branch_name = format!(
            "ontology/{}-{}",
            agent_ctx.agent_type,
            &agent_ctx.agent_id[..8.min(agent_ctx.agent_id.len())]
        );
        self.create_ref(&branch_name, &commit_sha).await?;

        // 6. Create PR
        let pr_url = self.create_pull_request(title, body, &branch_name).await?;

        info!("Created ontology PR: {}", pr_url);
        Ok(pr_url)
    }

    async fn get_ref_sha(&self, branch: &str) -> Result<String, String> {
        let url = self.api_url(&format!("git/ref/heads/{}", branch));
        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("Failed to get ref: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Get ref failed ({}): {}", status, body));
        }

        let ref_resp: RefResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse ref response: {}", e))?;

        Ok(ref_resp.object.sha)
    }

    async fn create_blob(&self, content: &str) -> Result<String, String> {
        let url = self.api_url("git/blobs");
        let body = CreateBlobRequest {
            content: content.to_string(),
            encoding: "utf-8".to_string(),
        };

        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to create blob: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Create blob failed ({}): {}", status, body));
        }

        let blob: BlobResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse blob response: {}", e))?;

        Ok(blob.sha)
    }

    async fn create_tree(
        &self,
        base_tree_sha: &str,
        file_path: &str,
        blob_sha: &str,
    ) -> Result<String, String> {
        let url = self.api_url("git/trees");
        let body = CreateTreeRequest {
            base_tree: base_tree_sha.to_string(),
            tree: vec![TreeEntry {
                path: file_path.to_string(),
                mode: "100644".to_string(),
                entry_type: "blob".to_string(),
                sha: blob_sha.to_string(),
            }],
        };

        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to create tree: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Create tree failed ({}): {}", status, body));
        }

        let tree: TreeResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse tree response: {}", e))?;

        Ok(tree.sha)
    }

    async fn create_commit(
        &self,
        message: &str,
        tree_sha: &str,
        parent_sha: &str,
    ) -> Result<String, String> {
        let url = self.api_url("git/commits");
        let body = CreateCommitRequest {
            message: message.to_string(),
            tree: tree_sha.to_string(),
            parents: vec![parent_sha.to_string()],
        };

        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to create commit: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Create commit failed ({}): {}", status, body));
        }

        let commit: CommitResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse commit response: {}", e))?;

        Ok(commit.sha)
    }

    async fn create_ref(&self, branch: &str, sha: &str) -> Result<(), String> {
        let url = self.api_url("git/refs");
        let body = CreateRefRequest {
            ref_name: format!("refs/heads/{}", branch),
            sha: sha.to_string(),
        };

        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to create ref: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 422 {
                // Branch already exists — force-update it to the new commit
                info!(
                    "Branch '{}' already exists, force-updating to SHA {}",
                    branch, sha
                );
                self.update_ref(branch, sha).await?;
            } else {
                return Err(format!("Create ref failed ({}): {}", status, body));
            }
        }

        Ok(())
    }

    /// Force-update an existing branch ref to a new SHA.
    async fn update_ref(&self, branch: &str, sha: &str) -> Result<(), String> {
        let url = self.api_url(&format!("git/refs/heads/{}", branch));

        let body = serde_json::json!({
            "sha": sha,
            "force": true,
        });

        let resp = self
            .client
            .patch(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to update ref: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let resp_body = resp.text().await.unwrap_or_default();
            return Err(format!("Update ref failed ({}): {}", status, resp_body));
        }

        info!("Force-updated branch '{}' to SHA {}", branch, sha);
        Ok(())
    }

    async fn create_pull_request(
        &self,
        title: &str,
        body: &str,
        head_branch: &str,
    ) -> Result<String, String> {
        let url = self.api_url("pulls");
        let pr_body = CreatePRRequest {
            title: title.to_string(),
            body: body.to_string(),
            head: head_branch.to_string(),
            base: self.base_branch.clone(),
            labels: Some(vec!["ontology".to_string(), "agent-proposed".to_string()]),
        };

        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(&pr_body)
            .send()
            .await
            .map_err(|e| format!("Failed to create PR: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let resp_body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 422 {
                // PR already exists for this head/base pair — fetch the existing one
                info!(
                    "PR already exists for branch '{}', fetching existing PR URL",
                    head_branch
                );
                return self.get_existing_pr_url(head_branch).await;
            }
            return Err(format!("Create PR failed ({}): {}", status, resp_body));
        }

        let pr: PRResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse PR response: {}", e))?;

        Ok(pr.html_url)
    }

    /// Fetch the URL of an existing open PR for the given head branch.
    async fn get_existing_pr_url(&self, head_branch: &str) -> Result<String, String> {
        let url = format!(
            "{}?head={}:{}&state=open",
            self.api_url("pulls"),
            self.owner,
            head_branch
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("Failed to fetch existing PRs: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Fetch existing PRs failed ({}): {}", status, body));
        }

        let prs: Vec<PRResponse> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse PR list response: {}", e))?;

        prs.first().map(|pr| pr.html_url.clone()).ok_or_else(|| {
            format!(
                "PR creation returned 422 but no open PR found for branch '{}'",
                head_branch
            )
        })
    }
}

impl Default for GitHubPRService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_number_parses_url_and_bare_number() {
        assert_eq!(
            GitHubPRService::pr_number_from_ref("https://github.com/o/r/pull/42"),
            Some(42)
        );
        assert_eq!(GitHubPRService::pr_number_from_ref("42"), Some(42));
        assert_eq!(
            GitHubPRService::pr_number_from_ref("https://github.com/o/r/pull/123#issuecomment-9"),
            Some(123)
        );
        assert_eq!(GitHubPRService::pr_number_from_ref("not-a-url"), None);
    }

    /// GOV-2 state transitions from the GitHub PR fields — the "fake PR-state
    /// source" the merge poll classifies.
    #[test]
    fn classify_pr_state_covers_merged_closed_open() {
        // merged_at present ⇒ Merged (the terminal success).
        assert_eq!(
            GitHubPRService::classify_pr_state("closed", Some("2026-07-10T00:00:00Z"), None),
            PrState::Merged
        );
        // merged bool true ⇒ Merged even without merged_at.
        assert_eq!(
            GitHubPRService::classify_pr_state("closed", None, Some(true)),
            PrState::Merged
        );
        // closed, never merged ⇒ ClosedUnmerged (abandoned).
        assert_eq!(
            GitHubPRService::classify_pr_state("closed", None, Some(false)),
            PrState::ClosedUnmerged
        );
        assert_eq!(
            GitHubPRService::classify_pr_state("closed", None, None),
            PrState::ClosedUnmerged
        );
        // still open ⇒ Open (keep polling).
        assert_eq!(
            GitHubPRService::classify_pr_state("open", None, Some(false)),
            PrState::Open
        );
    }
}
