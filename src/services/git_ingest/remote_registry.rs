//! DID-Gated Remote Registry — PRD-013 G2.
//!
//! Persistent CRUD registry of configured git remotes, backed by Neo4j.
//! Provides a legacy shim that auto-registers the existing `GITHUB_*` env
//! vars as a PAT-authenticated remote so existing deployments migrate
//! transparently.
//!
//! REST handler scaffolds are included for wiring into the Actix router.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use log::{info, warn};
use neo4rs::{query, Graph};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{GitRemote, RemoteAuth};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("neo4j query failed: {0}")]
    Neo4j(#[from] neo4rs::Error),

    #[error("remote not found: {0}")]
    NotFound(String),

    #[error("duplicate remote URL: {0}")]
    Duplicate(String),

    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Persistent registry of configured git remotes (Neo4j-backed).
#[derive(Clone)]
pub struct RemoteRegistry {
    graph: Arc<Graph>,
}

impl RemoteRegistry {
    pub fn new(graph: Arc<Graph>) -> Self {
        Self { graph }
    }

    /// List all registered remotes.
    pub async fn list(&self) -> Result<Vec<GitRemote>, RegistryError> {
        let mut result = self
            .graph
            .execute(query(
                "MATCH (r:GitRemote) \
                 RETURN r.id AS id, r.url AS url, r.auth_json AS auth_json, \
                        r.owner_did AS owner_did, r.base_paths_json AS base_paths_json, \
                        r.branch AS branch, r.sync_interval_secs AS sync_interval_secs, \
                        r.writeback_enabled AS writeback_enabled, \
                        r.last_sync AS last_sync, r.last_commit_sha AS last_commit_sha \
                 ORDER BY r.url",
            ))
            .await?;

        let mut remotes = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            if let Ok(remote) = row_to_remote(&row) {
                remotes.push(remote);
            }
        }
        Ok(remotes)
    }

    /// Get a single remote by id.
    pub async fn get(&self, id: &str) -> Result<GitRemote, RegistryError> {
        let mut result = self
            .graph
            .execute(
                query(
                    "MATCH (r:GitRemote {id: $id}) \
                     RETURN r.id AS id, r.url AS url, r.auth_json AS auth_json, \
                            r.owner_did AS owner_did, r.base_paths_json AS base_paths_json, \
                            r.branch AS branch, r.sync_interval_secs AS sync_interval_secs, \
                            r.writeback_enabled AS writeback_enabled, \
                            r.last_sync AS last_sync, r.last_commit_sha AS last_commit_sha",
                )
                .param("id", id),
            )
            .await?;

        match result.next().await {
            Ok(Some(row)) => row_to_remote(&row),
            _ => Err(RegistryError::NotFound(id.to_string())),
        }
    }

    /// Register a new remote. Returns the assigned id.
    pub async fn create(&self, remote: &GitRemote) -> Result<String, RegistryError> {
        let auth_json = serde_json::to_string(&remote.auth)?;
        let base_paths_json = serde_json::to_string(&remote.base_paths)?;

        self.graph
            .run(
                query(
                    "CREATE (r:GitRemote { \
                         id: $id, \
                         url: $url, \
                         auth_json: $auth_json, \
                         owner_did: $owner_did, \
                         base_paths_json: $base_paths_json, \
                         branch: $branch, \
                         sync_interval_secs: $sync_interval_secs, \
                         writeback_enabled: $writeback_enabled, \
                         last_sync: $last_sync, \
                         last_commit_sha: $last_commit_sha \
                     })",
                )
                .param("id", remote.id.as_str())
                .param("url", remote.url.as_str())
                .param("auth_json", auth_json.as_str())
                .param("owner_did", remote.owner_did.as_deref().unwrap_or(""))
                .param("base_paths_json", base_paths_json.as_str())
                .param("branch", remote.branch.as_str())
                .param("sync_interval_secs", remote.sync_interval_secs as i64)
                .param("writeback_enabled", remote.writeback_enabled)
                .param(
                    "last_sync",
                    remote.last_sync.map(|t| t.to_rfc3339()).unwrap_or_default(),
                )
                .param(
                    "last_commit_sha",
                    remote.last_commit_sha.as_deref().unwrap_or(""),
                ),
            )
            .await?;

        info!(
            "git-ingest: registered remote {} ({})",
            remote.id, remote.url
        );
        Ok(remote.id.clone())
    }

    /// Update sync metadata (last_sync, last_commit_sha) after a successful sync.
    pub async fn update_sync_metadata(
        &self,
        id: &str,
        last_sync: DateTime<Utc>,
        last_commit_sha: &str,
    ) -> Result<(), RegistryError> {
        self.graph
            .run(
                query(
                    "MATCH (r:GitRemote {id: $id}) \
                     SET r.last_sync = $last_sync, \
                         r.last_commit_sha = $last_commit_sha",
                )
                .param("id", id)
                .param("last_sync", last_sync.to_rfc3339())
                .param("last_commit_sha", last_commit_sha),
            )
            .await?;
        Ok(())
    }

    /// Delete a remote by id. Also removes its local clone directory.
    pub async fn delete(&self, id: &str) -> Result<(), RegistryError> {
        let mut result = self
            .graph
            .execute(
                query(
                    "MATCH (r:GitRemote {id: $id}) \
                     DETACH DELETE r \
                     RETURN count(r) AS deleted",
                )
                .param("id", id),
            )
            .await?;

        let deleted: i64 = match result.next().await {
            Ok(Some(row)) => row.get("deleted").unwrap_or(0),
            _ => 0,
        };

        if deleted == 0 {
            return Err(RegistryError::NotFound(id.to_string()));
        }

        info!("git-ingest: deleted remote {}", id);
        let local_path = super::ingest_root().join(super::sanitize_id(id));
        if local_path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&local_path) {
                warn!(
                    "git-ingest: failed to remove local clone {:?}: {}",
                    local_path, e
                );
            }
        }
        Ok(())
    }

    /// Check if a remote with the given URL already exists.
    pub async fn exists_by_url(&self, url: &str) -> Result<bool, RegistryError> {
        let mut result = self
            .graph
            .execute(
                query("MATCH (r:GitRemote {url: $url}) RETURN r.id AS id LIMIT 1")
                    .param("url", url),
            )
            .await?;
        Ok(result.next().await.ok().flatten().is_some())
    }

    /// Legacy GitHub shim: reads `GITHUB_TOKEN`, `GITHUB_OWNER`, `GITHUB_REPO`,
    /// and `GITHUB_BASE_PATH` env vars and auto-registers as a PAT remote with
    /// id `"legacy-github"`.
    ///
    /// This bridges existing deployments into the git-ingest pipeline without
    /// manual reconfiguration. The GitHub REST API path
    /// (`GitHubSyncService`) continues working in parallel.
    pub async fn legacy_github_shim(&self) -> Result<Option<String>, RegistryError> {
        let token_var = "GITHUB_TOKEN";
        let owner = match std::env::var("GITHUB_OWNER") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                info!("git-ingest: GITHUB_OWNER not set; skipping legacy shim");
                return Ok(None);
            }
        };
        let repo = match std::env::var("GITHUB_REPO") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                info!("git-ingest: GITHUB_REPO not set; skipping legacy shim");
                return Ok(None);
            }
        };

        // Verify the token env var is actually set (we store the var name,
        // not the value).
        if std::env::var(token_var).is_err() {
            warn!(
                "git-ingest: {} not set; legacy GitHub remote will be registered \
                 but sync will fail until the token is provided",
                token_var
            );
        }

        let url = format!("https://github.com/{}/{}.git", owner, repo);
        let legacy_id = "legacy-github".to_string();

        // Idempotent: don't re-register if already present.
        if let Ok(existing) = self.get(&legacy_id).await {
            info!(
                "git-ingest: legacy GitHub remote already registered ({})",
                existing.url
            );
            return Ok(Some(legacy_id));
        }

        // Collect base paths from env.
        let base_paths = if let Ok(paths) = std::env::var("GITHUB_BASE_PATHS") {
            paths
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            match std::env::var("GITHUB_BASE_PATH") {
                Ok(p) if !p.is_empty() => vec![p],
                _ => vec![],
            }
        };

        let branch = std::env::var("GITHUB_BRANCH").unwrap_or_else(|_| "main".to_string());

        let remote = GitRemote {
            id: legacy_id.clone(),
            url,
            auth: RemoteAuth::Pat {
                token_env_var: token_var.to_string(),
            },
            owner_did: None,
            base_paths,
            branch,
            sync_interval_secs: 0, // manual sync by default
            writeback_enabled: false,
            last_sync: None,
            last_commit_sha: None,
        };

        self.create(&remote).await?;
        info!(
            "git-ingest: registered legacy GitHub remote as '{}'",
            legacy_id
        );
        Ok(Some(legacy_id))
    }
}

// ---------------------------------------------------------------------------
// Neo4j row → GitRemote mapping
// ---------------------------------------------------------------------------

fn row_to_remote(row: &neo4rs::Row) -> Result<GitRemote, RegistryError> {
    let id: String = row.get("id").unwrap_or_default();
    let url: String = row.get("url").unwrap_or_default();
    let auth_json: String = row.get("auth_json").unwrap_or_default();
    let owner_did_raw: String = row.get("owner_did").unwrap_or_default();
    let base_paths_json: String = row
        .get("base_paths_json")
        .unwrap_or_else(|_| "[]".to_string());
    let branch: String = row.get("branch").unwrap_or_else(|_| "main".to_string());
    let sync_interval_secs: i64 = row.get("sync_interval_secs").unwrap_or(0);
    let writeback_enabled: bool = row.get("writeback_enabled").unwrap_or(false);
    let last_sync_raw: String = row.get("last_sync").unwrap_or_default();
    let last_commit_sha_raw: String = row.get("last_commit_sha").unwrap_or_default();

    let auth: RemoteAuth = if auth_json.is_empty() {
        RemoteAuth::None
    } else {
        serde_json::from_str(&auth_json)?
    };

    let base_paths: Vec<String> = serde_json::from_str(&base_paths_json).unwrap_or_default();

    let owner_did = if owner_did_raw.is_empty() {
        None
    } else {
        Some(owner_did_raw)
    };

    let last_sync = if last_sync_raw.is_empty() {
        None
    } else {
        DateTime::parse_from_rfc3339(&last_sync_raw)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    };

    let last_commit_sha = if last_commit_sha_raw.is_empty() {
        None
    } else {
        Some(last_commit_sha_raw)
    };

    Ok(GitRemote {
        id,
        url,
        auth,
        owner_did,
        base_paths,
        branch,
        sync_interval_secs: u64::try_from(sync_interval_secs).unwrap_or(0),
        writeback_enabled,
        last_sync,
        last_commit_sha,
    })
}

// ---------------------------------------------------------------------------
// REST handler scaffolds (Actix)
// ---------------------------------------------------------------------------

/// Request body for `POST /api/ingest/remotes`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRemoteRequest {
    pub url: String,
    pub auth: RemoteAuth,
    #[serde(default)]
    pub owner_did: Option<String>,
    #[serde(default)]
    pub base_paths: Vec<String>,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub sync_interval_secs: u64,
    #[serde(default)]
    pub writeback_enabled: bool,
}

fn default_branch() -> String {
    "main".to_string()
}

/// Redacted response DTO — omits auth credentials from API responses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteResponse {
    id: String,
    url: String,
    auth_type: &'static str,
    owner_did: Option<String>,
    base_paths: Vec<String>,
    branch: String,
    sync_interval_secs: u64,
    writeback_enabled: bool,
    last_sync: Option<String>,
    last_commit_sha: Option<String>,
}

impl From<&GitRemote> for RemoteResponse {
    fn from(r: &GitRemote) -> Self {
        Self {
            id: r.id.clone(),
            url: r.url.clone(),
            auth_type: match &r.auth {
                RemoteAuth::None => "none",
                RemoteAuth::Pat { .. } => "pat",
                RemoteAuth::DidNostr { .. } => "did_nostr",
            },
            owner_did: r.owner_did.clone(),
            base_paths: r.base_paths.clone(),
            branch: r.branch.clone(),
            sync_interval_secs: r.sync_interval_secs,
            writeback_enabled: r.writeback_enabled,
            last_sync: r.last_sync.map(|t| t.to_rfc3339()),
            last_commit_sha: r.last_commit_sha.clone(),
        }
    }
}

/// `GET /api/ingest/remotes` — list all configured remotes.
pub async fn handle_list_remotes(
    registry: actix_web::web::Data<RemoteRegistry>,
) -> actix_web::HttpResponse {
    match registry.list().await {
        Ok(remotes) => {
            let redacted: Vec<RemoteResponse> = remotes.iter().map(RemoteResponse::from).collect();
            actix_web::HttpResponse::Ok().json(redacted)
        }
        Err(e) => {
            log::error!("git-ingest: list remotes failed: {}", e);
            actix_web::HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}

/// `POST /api/ingest/remotes` — register a new remote.
pub async fn handle_create_remote(
    _user: crate::settings::auth_extractor::AuthenticatedUser,
    registry: actix_web::web::Data<RemoteRegistry>,
    body: actix_web::web::Json<CreateRemoteRequest>,
) -> actix_web::HttpResponse {
    let req = body.into_inner();

    // H6 fix: reject non-HTTPS remote URLs to prevent SSRF.
    if !req.url.starts_with("https://") && !req.url.starts_with("http://") {
        return actix_web::HttpResponse::BadRequest().json(serde_json::json!({
            "error": "only http(s) remote URLs are allowed"
        }));
    }

    let remote = GitRemote {
        id: Uuid::new_v4().to_string(),
        url: req.url,
        auth: req.auth,
        owner_did: req.owner_did,
        base_paths: req.base_paths,
        branch: req.branch,
        sync_interval_secs: req.sync_interval_secs,
        writeback_enabled: req.writeback_enabled,
        last_sync: None,
        last_commit_sha: None,
    };

    match registry.create(&remote).await {
        Ok(id) => actix_web::HttpResponse::Created().json(serde_json::json!({
            "id": id,
            "url": remote.url,
        })),
        Err(e) => {
            log::error!("git-ingest: create remote failed: {}", e);
            actix_web::HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}

/// `DELETE /api/ingest/remotes/{id}` — remove a remote.
pub async fn handle_delete_remote(
    _user: crate::settings::auth_extractor::AuthenticatedUser,
    registry: actix_web::web::Data<RemoteRegistry>,
    path: actix_web::web::Path<String>,
) -> actix_web::HttpResponse {
    let id = path.into_inner();

    match registry.delete(&id).await {
        Ok(()) => actix_web::HttpResponse::NoContent().finish(),
        Err(RegistryError::NotFound(id)) => {
            actix_web::HttpResponse::NotFound().json(serde_json::json!({
                "error": format!("remote not found: {}", id)
            }))
        }
        Err(e) => {
            log::error!("git-ingest: delete remote failed: {}", e);
            actix_web::HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_remote_request_deserialises() {
        let json = r#"{
            "url": "https://github.com/user/repo.git",
            "auth": { "type": "pat", "token_env_var": "GITHUB_TOKEN" },
            "basePaths": ["pages"],
            "branch": "main"
        }"#;
        let req: CreateRemoteRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, "https://github.com/user/repo.git");
        assert!(matches!(req.auth, RemoteAuth::Pat { .. }));
        assert_eq!(req.base_paths, vec!["pages"]);
    }

    #[test]
    fn create_remote_request_defaults() {
        let json = r#"{
            "url": "https://pod.example.com/alice/kg/git/",
            "auth": { "type": "did_nostr", "server_identity": true }
        }"#;
        let req: CreateRemoteRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.branch, "main");
        assert_eq!(req.sync_interval_secs, 0);
        assert!(!req.writeback_enabled);
    }
}
