//! Git Ingest Surface — PRD-013 (G1 + G2 + G3 + G4).
//!
//! Replaces the GitHub REST API ingest with a git-over-HTTP pipeline that
//! treats every knowledge source identically: GitHub, GitLab, Solid pod, or
//! bare repo. Identity-mediated access via `did:nostr` + NIP-98 is layered on
//! top. Write-back through the Judgment Broker completes the bidirectional flow.
//!
//! # Module layout
//!
//! | File                  | Component | PRD-013 ref |
//! |-----------------------|-----------|-------------|
//! | `mod.rs`              | GitIngestService (clone/fetch) + route config | G1 |
//! | `remote_registry.rs`  | RemoteRegistry + REST handlers | G2 |
//! | `provenance.rs`       | ProvenanceTrailer encoder      | G3 |
//! | `writeback_saga.rs`   | WriteBackSaga (full impl)      | G4 |

pub mod provenance;
pub mod remote_registry;
pub mod writeback_saga;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use git2::{build::RepoBuilder, Cred, FetchOptions, RemoteCallbacks, Repository};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use remote_registry::RemoteRegistry;
pub use writeback_saga::WriteBackSaga;

// ---------------------------------------------------------------------------
// Feature flag
// ---------------------------------------------------------------------------

pub const GIT_INGEST_ENABLED_ENV: &str = "GIT_INGEST_ENABLED";
pub const GIT_INGEST_ROOT_ENV: &str = "GIT_INGEST_ROOT";

const DEFAULT_INGEST_ROOT: &str = "/app/data/git-ingest";

pub fn git_ingest_enabled() -> bool {
    std::env::var(GIT_INGEST_ENABLED_ENV)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn ingest_root() -> PathBuf {
    PathBuf::from(
        std::env::var(GIT_INGEST_ROOT_ENV).unwrap_or_else(|_| DEFAULT_INGEST_ROOT.to_string()),
    )
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteAuth {
    None,
    Pat {
        token_env_var: String,
    },
    DidNostr {
        server_identity: bool,
        keypair_env_var: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote {
    pub id: String,
    pub url: String,
    pub auth: RemoteAuth,
    pub owner_did: Option<String>,
    pub base_paths: Vec<String>,
    pub branch: String,
    pub sync_interval_secs: u64,
    pub writeback_enabled: bool,
    pub last_sync: Option<DateTime<Utc>>,
    pub last_commit_sha: Option<String>,
}

impl GitRemote {
    pub fn local_path(&self) -> PathBuf {
        ingest_root().join(sanitize_id(&self.id))
    }
}

pub(super) fn sanitize_id(id: &str) -> &str {
    // Strip path separators and traversal components — defence-in-depth.
    let s = id.trim().trim_start_matches('/').trim_start_matches('\\');
    if s.contains("..") || s.contains('/') || s.contains('\\') {
        "invalid-id"
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum GitIngestError {
    #[error("git operation failed: {0}")]
    Git(#[from] git2::Error),

    #[error("remote not found: {0}")]
    RemoteNotFound(String),

    #[error("local clone path is not a valid UTF-8 string")]
    InvalidPath,

    #[error("auth env var `{0}` not set")]
    MissingAuthEnvVar(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("git-ingest is disabled (set {GIT_INGEST_ENABLED_ENV}=true)")]
    Disabled,

    #[error("registry error: {0}")]
    Registry(#[from] remote_registry::RegistryError),
}

// ---------------------------------------------------------------------------
// Sync result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    pub changed_files: Vec<PathBuf>,
    pub deleted_files: Vec<PathBuf>,
    pub head_sha: String,
}

// ---------------------------------------------------------------------------
// GitIngestService
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GitIngestService {
    registry: RemoteRegistry,
}

impl GitIngestService {
    pub fn new(registry: RemoteRegistry) -> Self {
        Self { registry }
    }

    pub async fn sync_remote(&self, remote: &mut GitRemote) -> Result<SyncResult, GitIngestError> {
        if !git_ingest_enabled() {
            return Err(GitIngestError::Disabled);
        }

        let local_path = remote.local_path();
        let url = remote.url.clone();
        let branch = remote.branch.clone();
        let auth = remote.auth.clone();
        let last_sha = remote.last_commit_sha.clone();

        let result = tokio::task::spawn_blocking(move || {
            if local_path.exists() {
                fetch_and_diff(&local_path, &url, &branch, &auth, last_sha.as_deref())
            } else {
                clone_remote(&local_path, &url, &branch, &auth)
            }
        })
        .await
        .map_err(|e| GitIngestError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))??;

        remote.last_sync = Some(Utc::now());
        remote.last_commit_sha = Some(result.head_sha.clone());

        self.registry
            .update_sync_metadata(&remote.id, remote.last_sync.unwrap(), &result.head_sha)
            .await?;

        info!(
            "git-ingest: synced remote {} ({}) — {} changed, {} deleted",
            remote.id,
            remote.url,
            result.changed_files.len(),
            result.deleted_files.len(),
        );

        Ok(result)
    }

    pub async fn sync_by_id(&self, remote_id: &str) -> Result<SyncResult, GitIngestError> {
        let mut remote = self
            .registry
            .get(remote_id)
            .await
            .map_err(|_| GitIngestError::RemoteNotFound(remote_id.to_string()))?;
        self.sync_remote(&mut remote).await
    }

    pub async fn sync_all(&self) -> Vec<(String, Result<SyncResult, GitIngestError>)> {
        let remotes = match self.registry.list().await {
            Ok(r) => r,
            Err(e) => {
                warn!("git-ingest: failed to list remotes: {}", e);
                return Vec::new();
            }
        };

        // M2: sync remotes concurrently (bounded to 4 at a time).
        let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
        let mut handles = Vec::with_capacity(remotes.len());

        for remote in remotes {
            let svc = self.clone();
            let sem = semaphore.clone();
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await;
                let id = remote.id.clone();
                let mut remote = remote;
                let res = svc.sync_remote(&mut remote).await;
                (id, res)
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(pair) => results.push(pair),
                Err(e) => warn!("git-ingest: sync task panicked: {}", e),
            }
        }
        results
    }
}

// ---------------------------------------------------------------------------
// git2 helpers (blocking, run on spawn_blocking)
// ---------------------------------------------------------------------------

fn build_callbacks(auth: &RemoteAuth) -> Result<RemoteCallbacks<'_>, GitIngestError> {
    let mut callbacks = RemoteCallbacks::new();

    match auth {
        RemoteAuth::None => {}
        RemoteAuth::Pat { token_env_var } => {
            let token = std::env::var(token_env_var)
                .map_err(|_| GitIngestError::MissingAuthEnvVar(token_env_var.clone()))?;
            let token_owned = token.clone();
            callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext("x-access-token", &token_owned)
            });
        }
        RemoteAuth::DidNostr {
            server_identity,
            keypair_env_var,
        } => {
            let key_var = if *server_identity {
                "SERVER_NOSTR_PRIVKEY".to_string()
            } else {
                keypair_env_var
                    .clone()
                    .unwrap_or_else(|| "SERVER_NOSTR_PRIVKEY".to_string())
            };

            match std::env::var(&key_var) {
                Ok(hex_key) if hex_key.len() == 64 => {
                    callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                        Cred::userpass_plaintext("nostr", &hex_key)
                    });
                }
                _ => {
                    warn!(
                        "git-ingest: {} not set or invalid; attempting unauthenticated access",
                        key_var
                    );
                }
            }
        }
    }

    Ok(callbacks)
}

fn clone_remote(
    local_path: &Path,
    url: &str,
    branch: &str,
    auth: &RemoteAuth,
) -> Result<SyncResult, GitIngestError> {
    info!(
        "git-ingest: cloning {} (branch: {}) → {:?}",
        url, branch, local_path
    );

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let callbacks = build_callbacks(auth)?;
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let repo = RepoBuilder::new()
        .branch(branch)
        .fetch_options(fetch_opts)
        .clone(url, local_path)?;

    let head = repo.head()?;
    let head_sha = head.target().map(|oid| oid.to_string()).unwrap_or_default();
    let changed_files = collect_worktree_files(local_path)?;

    Ok(SyncResult {
        changed_files,
        deleted_files: Vec::new(),
        head_sha,
    })
}

fn fetch_and_diff(
    local_path: &Path,
    url: &str,
    branch: &str,
    auth: &RemoteAuth,
    last_sha: Option<&str>,
) -> Result<SyncResult, GitIngestError> {
    info!(
        "git-ingest: fetching {} (branch: {}) in {:?}",
        url, branch, local_path
    );

    let repo = Repository::open(local_path)?;

    let callbacks = build_callbacks(auth)?;
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut origin = repo
        .find_remote("origin")
        .or_else(|_| repo.remote("origin", url))?;

    let refspec = format!("refs/heads/{}:refs/remotes/origin/{}", branch, branch);
    origin.fetch(&[&refspec], Some(&mut fetch_opts), None)?;
    drop(origin);

    let fetch_head = repo.find_reference(&format!("refs/remotes/origin/{}", branch))?;
    let new_oid = fetch_head
        .target()
        .ok_or_else(|| git2::Error::from_str("fetch head has no target"))?;
    let new_sha = new_oid.to_string();

    let fetch_commit = repo.find_commit(new_oid)?;
    let local_branch_ref = format!("refs/heads/{}", branch);
    if repo.find_reference(&local_branch_ref).is_ok() {
        repo.reference(&local_branch_ref, new_oid, true, "git-ingest: fast-forward")?;
    } else {
        repo.branch(branch, &fetch_commit, true)?;
    }
    repo.set_head(&local_branch_ref)?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;

    let (changed_files, deleted_files) = if let Some(old_sha_str) = last_sha {
        if old_sha_str == new_sha {
            debug!("git-ingest: no new commits since {}", old_sha_str);
            return Ok(SyncResult {
                changed_files: Vec::new(),
                deleted_files: Vec::new(),
                head_sha: new_sha,
            });
        }
        diff_trees(&repo, old_sha_str, &new_sha, local_path)?
    } else {
        (collect_worktree_files(local_path)?, Vec::new())
    };

    Ok(SyncResult {
        changed_files,
        deleted_files,
        head_sha: new_sha,
    })
}

fn diff_trees(
    repo: &Repository,
    old_sha: &str,
    new_sha: &str,
    local_path: &Path,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), GitIngestError> {
    let old_oid = git2::Oid::from_str(old_sha)?;
    let new_oid = git2::Oid::from_str(new_sha)?;

    let old_commit = repo.find_commit(old_oid)?;
    let new_commit = repo.find_commit(new_oid)?;

    let old_tree = old_commit.tree()?;
    let new_tree = new_commit.tree()?;

    let diff = repo.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)?;

    let mut changed = Vec::new();
    let mut deleted = Vec::new();

    diff.foreach(
        &mut |delta, _progress| {
            match delta.status() {
                git2::Delta::Deleted => {
                    if let Some(path) = delta.old_file().path() {
                        deleted.push(local_path.join(path));
                    }
                }
                _ => {
                    if let Some(path) = delta.new_file().path() {
                        changed.push(local_path.join(path));
                    }
                }
            }
            true
        },
        None,
        None,
        None,
    )?;

    Ok((changed, deleted))
}

fn collect_worktree_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    collect_files_recursive(root, root, &mut files)?;
    Ok(files)
}

fn collect_files_recursive(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            collect_files_recursive(root, &path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// REST handlers + Actix route config
// ---------------------------------------------------------------------------

use actix_web::{web, HttpResponse};
use crate::settings::auth_extractor::AuthenticatedUser;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncRequest {
    pub remote_id: Option<String>,
}

async fn handle_trigger_sync(
    _user: AuthenticatedUser,
    ingest: web::Data<Arc<GitIngestService>>,
    body: web::Json<TriggerSyncRequest>,
) -> HttpResponse {
    let req = body.into_inner();
    match req.remote_id {
        Some(id) => match ingest.sync_by_id(&id).await {
            Ok(result) => HttpResponse::Ok().json(serde_json::json!({
                "remoteId": id,
                "headSha": result.head_sha,
                "changedFiles": result.changed_files.len(),
                "deletedFiles": result.deleted_files.len(),
            })),
            Err(e) => {
                log::error!("git-ingest: sync failed for {}: {}", id, e);
                HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": e.to_string()
                }))
            }
        },
        None => {
            let results = ingest.sync_all().await;
            let summary: Vec<_> = results
                .into_iter()
                .map(|(id, res)| match res {
                    Ok(r) => serde_json::json!({
                        "remoteId": id,
                        "headSha": r.head_sha,
                        "changedFiles": r.changed_files.len(),
                        "deletedFiles": r.deleted_files.len(),
                        "status": "ok",
                    }),
                    Err(e) => serde_json::json!({
                        "remoteId": id,
                        "error": e.to_string(),
                        "status": "error",
                    }),
                })
                .collect();
            HttpResponse::Ok().json(summary)
        }
    }
}

async fn handle_get_remote(
    registry: web::Data<RemoteRegistry>,
    path: web::Path<String>,
) -> HttpResponse {
    let id = path.into_inner();
    match registry.get(&id).await {
        Ok(remote) => HttpResponse::Ok().json(remote_registry::RemoteResponse::from(&remote)),
        Err(remote_registry::RegistryError::NotFound(id)) => {
            HttpResponse::NotFound().json(serde_json::json!({
                "error": format!("remote not found: {}", id)
            }))
        }
        Err(e) => {
            log::error!("git-ingest: get remote failed: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}

async fn handle_writeback(
    _user: AuthenticatedUser,
    saga: web::Data<Arc<WriteBackSaga>>,
    body: web::Json<WriteBackRequest>,
) -> HttpResponse {
    let req = body.into_inner();
    match saga
        .execute(&req.remote_id, &req.enrichment, &req.decision)
        .await
    {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            log::error!("write-back: saga failed: {}", e);
            let status = match &e {
                writeback_saga::WriteBackError::GloballyDisabled
                | writeback_saga::WriteBackError::RemoteDisabled(_) => {
                    actix_web::http::StatusCode::FORBIDDEN
                }
                writeback_saga::WriteBackError::RemoteNotFound(_) => {
                    actix_web::http::StatusCode::NOT_FOUND
                }
                writeback_saga::WriteBackError::Conflict { .. } => {
                    actix_web::http::StatusCode::CONFLICT
                }
                _ => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            };
            HttpResponse::build(status).json(serde_json::json!({
                "error": e.to_string()
            }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteBackRequest {
    pub remote_id: String,
    pub enrichment: writeback_saga::EnrichmentPayload,
    pub decision: writeback_saga::DecisionReport,
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/ingest")
            .route(
                "/remotes",
                web::get().to(remote_registry::handle_list_remotes),
            )
            .route(
                "/remotes",
                web::post().to(remote_registry::handle_create_remote),
            )
            .route("/remotes/{id}", web::get().to(handle_get_remote))
            .route(
                "/remotes/{id}",
                web::delete().to(remote_registry::handle_delete_remote),
            )
            .route("/sync", web::post().to(handle_trigger_sync))
            .route("/writeback", web::post().to(handle_writeback)),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_local_path_uses_id() {
        let remote = GitRemote {
            id: "abc-123".to_string(),
            url: "https://github.com/user/repo.git".to_string(),
            auth: RemoteAuth::None,
            owner_did: None,
            base_paths: vec!["pages".to_string()],
            branch: "main".to_string(),
            sync_interval_secs: 0,
            writeback_enabled: false,
            last_sync: None,
            last_commit_sha: None,
        };
        let path = remote.local_path();
        assert!(path.ends_with("abc-123"));
    }

    #[test]
    fn remote_auth_pat_serialises() {
        let auth = RemoteAuth::Pat {
            token_env_var: "GITHUB_TOKEN".to_string(),
        };
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"type\":\"pat\""));
        assert!(json.contains("GITHUB_TOKEN"));
    }

    #[test]
    fn remote_auth_did_nostr_serialises() {
        let auth = RemoteAuth::DidNostr {
            server_identity: true,
            keypair_env_var: None,
        };
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"type\":\"did_nostr\""));
    }

    #[test]
    fn feature_flag_default_off() {
        std::env::remove_var(GIT_INGEST_ENABLED_ENV);
        assert!(!git_ingest_enabled());
    }

    #[test]
    fn ingest_root_default() {
        std::env::remove_var(GIT_INGEST_ROOT_ENV);
        assert_eq!(ingest_root(), PathBuf::from(DEFAULT_INGEST_ROOT));
    }
}
