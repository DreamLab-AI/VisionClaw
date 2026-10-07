// src/adapters/sqlite_canary_repository.rs
//! SQLite LivenessCanary Repository Adapter (RES-a, ADR-130 Decision 3).
//!
//! Durable registry + fire-log for the sprint-wide `LivenessHarness`. Backs the
//! three canary surfaces (`register`, `observe`, `status`) and the KG-backend
//! watchdog. A canary is registered once (idempotently) and then fires only on
//! observed live traffic; each fire is bound to the git SHA it fired at so a
//! fire recorded against a stale commit no longer counts toward closure.
//!
//! ## Persistence choice
//!
//! `tokio-rusqlite`, mirroring [`crate::adapters::sqlite_enrichment_repository`]
//! verbatim: same worker-thread `call` serialisation, same self-bootstrapping
//! schema, same `Arc<Connection>` sharing posture. Lives in its OWN file
//! (`data/liveness.sqlite3`) so its single-writer constraint and migrations stay
//! isolated from `settings.sqlite3` and `enrichment.sqlite3`.
//!
//! ## Staleness rule (WP-11 falsification)
//!
//! [`all_status`](SqliteCanaryRepository::all_status) treats a canary as *fired*
//! (valid for closure) only when it has a fire whose SHA equals the current
//! runtime SHA AND whose timestamp is within the freshness window. A fire bound
//! to an older SHA, or older than the window, re-arms the canary — the exact
//! condition the RES-a falsification statement forbids ("a fired canary older
//! than its SHA still counts toward closure").

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_rusqlite::Connection;

/// Embedded canonical schema. Self-bootstrapping (single-binary deployments,
/// ADR-11 §D1), mirroring the enrichment repository's posture. The registry
/// (`liveness_canaries`) is `WITHOUT ROWID` keyed on `canary_id`; the fire log
/// (`canary_fires`) is an append-only child indexed for the status aggregate.
pub const CREATE_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store   = MEMORY;

CREATE TABLE IF NOT EXISTS schema_migrations (
    id          TEXT PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS liveness_canaries (
    canary_id            TEXT    PRIMARY KEY,
    description          TEXT    NOT NULL,
    kind                 TEXT    NOT NULL DEFAULT 'standing',
    owner_repo           TEXT    NOT NULL DEFAULT 'unknown',
    wave                 TEXT,
    sha_at_registration  TEXT    NOT NULL,
    registered_at_ms     INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS canary_fires (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    canary_id    TEXT    NOT NULL,
    evidence     TEXT    NOT NULL,
    sha          TEXT    NOT NULL,
    fired_at_ms  INTEGER NOT NULL,
    FOREIGN KEY(canary_id) REFERENCES liveness_canaries(canary_id)
);

CREATE INDEX IF NOT EXISTS canary_fire_lookup_idx
    ON canary_fires(canary_id, fired_at_ms);

INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0004_liveness_canaries');
"#;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum CanaryStoreError {
    #[error("db: {0}")]
    Database(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, CanaryStoreError>;

/// One `liveness_canaries` row as read: canary_id, description, kind,
/// owner_repo, wave, sha_at_registration, registered_at_ms.
type RawCanaryRow = (String, String, String, String, Option<String>, String, i64);

/// A canary joined with its fires: canary_id, description, kind, owner_repo,
/// wave, sha_at_registration, obs_count, last_fired, has_fresh.
type RawCanaryStatusRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    i64,
    Option<i64>,
    Option<i64>,
);

fn map_db_err(e: tokio_rusqlite::Error) -> CanaryStoreError {
    CanaryStoreError::Database(e.to_string())
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// A canary registration declared by any repository. `sha_at_registration` is
/// captured on first insert and preserved across idempotent re-registrations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryRegistration {
    pub canary_id: String,
    pub description: String,
    /// `standing` | `one-shot` (PRD-023 canary table).
    pub kind: String,
    pub owner_repo: String,
    pub wave: Option<String>,
    pub sha_at_registration: String,
    pub registered_at_ms: i64,
}

/// The per-canary status projection returned by `GET /api/canary/status`.
/// `fired`/`armed` apply the staleness rule; `last_fired_at` and
/// `observation_count` are informational across all fires (any SHA).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryStatus {
    pub canary_id: String,
    pub description: String,
    pub kind: String,
    pub owner_repo: String,
    pub wave: Option<String>,
    pub armed: bool,
    pub fired: bool,
    pub last_fired_at: Option<i64>,
    pub observation_count: i64,
    pub sha_at_registration: String,
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

/// SQLite-backed canary registry + fire log. Holds one `tokio-rusqlite`
/// connection in `Arc` so it can be cheaply cloned into the harness, handlers
/// and the watchdog task. See module docs for the single-writer posture.
pub struct SqliteCanaryRepository {
    conn: Arc<Connection>,
}

impl SqliteCanaryRepository {
    /// Open (or create) the canary DB at `db_path` and apply [`CREATE_SCHEMA`].
    pub async fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path).await.map_err(map_db_err)?;
        conn.call(|c| {
            c.execute_batch(CREATE_SCHEMA)?;
            Ok(())
        })
        .await
        .map_err(map_db_err)?;
        Ok(Self {
            conn: Arc::new(conn),
        })
    }

    /// Register (or idempotently update) a canary. First insert fixes
    /// `sha_at_registration` + `registered_at_ms`; a later call with the same
    /// `canary_id` refreshes the mutable descriptor fields but leaves the
    /// registration SHA and timestamp untouched, so start-up re-seeding is a
    /// no-op on the identity of an existing canary.
    pub async fn register(&self, reg: &CanaryRegistration) -> Result<()> {
        let canary_id = reg.canary_id.clone();
        let description = reg.description.clone();
        let kind = reg.kind.clone();
        let owner_repo = reg.owner_repo.clone();
        let wave = reg.wave.clone();
        let sha = reg.sha_at_registration.clone();
        let registered_at_ms = reg.registered_at_ms;
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT INTO liveness_canaries
                         (canary_id, description, kind, owner_repo, wave,
                          sha_at_registration, registered_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(canary_id) DO UPDATE SET
                         description = excluded.description,
                         kind        = excluded.kind,
                         owner_repo  = excluded.owner_repo,
                         wave        = excluded.wave",
                )?;
                stmt.execute(rusqlite::params![
                    &canary_id,
                    &description,
                    &kind,
                    &owner_repo,
                    &wave,
                    &sha,
                    registered_at_ms,
                ])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Fetch one canary registration by id.
    pub async fn get(&self, canary_id: &str) -> Result<Option<CanaryRegistration>> {
        let id = canary_id.to_string();
        let row: Option<RawCanaryRow> = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT canary_id, description, kind, owner_repo, wave,
                            sha_at_registration, registered_at_ms
                     FROM liveness_canaries WHERE canary_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![&id])?;
                if let Some(r) = rows.next()? {
                    Ok(Some((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    )))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(map_db_err)?;

        Ok(row.map(
            |(
                canary_id,
                description,
                kind,
                owner_repo,
                wave,
                sha_at_registration,
                registered_at_ms,
            )| {
                CanaryRegistration {
                    canary_id,
                    description,
                    kind,
                    owner_repo,
                    wave,
                    sha_at_registration,
                    registered_at_ms,
                }
            },
        ))
    }

    /// Record a fire (an observed live-traffic event) against a registered
    /// canary. The `sha` binds the fire to the commit it fired at (staleness
    /// rule). Returns the fire row id. Errors [`CanaryStoreError::NotFound`] if
    /// the canary was never registered — a fire can only land on a declared wire.
    pub async fn observe(
        &self,
        canary_id: &str,
        evidence: &str,
        sha: &str,
        fired_at_ms: i64,
    ) -> Result<i64> {
        if self.get(canary_id).await?.is_none() {
            return Err(CanaryStoreError::NotFound(canary_id.to_string()));
        }
        let id = canary_id.to_string();
        let evidence = evidence.to_string();
        let sha = sha.to_string();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT INTO canary_fires (canary_id, evidence, sha, fired_at_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                )?;
                stmt.execute(rusqlite::params![&id, &evidence, &sha, fired_at_ms])?;
                Ok(c.last_insert_rowid())
            })
            .await
            .map_err(map_db_err)
    }

    /// Project every registered canary into its status, applying the staleness
    /// rule against `current_sha` and the `[now_ms - window_ms, now_ms]` window.
    /// `fired` is true only when a fire exists at the current SHA within the
    /// window; otherwise the canary is `armed` (re-armed if it was stale).
    pub async fn all_status(
        &self,
        current_sha: &str,
        now_ms: i64,
        window_ms: i64,
    ) -> Result<Vec<CanaryStatus>> {
        let current_sha = current_sha.to_string();
        let cutoff = now_ms.saturating_sub(window_ms);
        let rows: Vec<RawCanaryStatusRow> = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT c.canary_id, c.description, c.kind, c.owner_repo, c.wave,
                            c.sha_at_registration,
                            COUNT(f.id)          AS obs_count,
                            MAX(f.fired_at_ms)   AS last_fired,
                            MAX(CASE WHEN f.sha = ?1 AND f.fired_at_ms >= ?2
                                     THEN 1 ELSE 0 END) AS has_fresh
                     FROM liveness_canaries c
                     LEFT JOIN canary_fires f ON f.canary_id = c.canary_id
                     GROUP BY c.canary_id
                     ORDER BY c.canary_id",
                )?;
                let mut q = stmt.query(rusqlite::params![&current_sha, cutoff])?;
                let mut out = Vec::new();
                while let Some(r) = q.next()? {
                    out.push((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ));
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    canary_id,
                    description,
                    kind,
                    owner_repo,
                    wave,
                    sha_at_registration,
                    observation_count,
                    last_fired_at,
                    has_fresh,
                )| {
                    let fired = has_fresh.unwrap_or(0) == 1;
                    CanaryStatus {
                        canary_id,
                        description,
                        kind,
                        owner_repo,
                        wave,
                        armed: !fired,
                        fired,
                        last_fired_at,
                        observation_count,
                        sha_at_registration,
                    }
                },
            )
            .collect())
    }
}
