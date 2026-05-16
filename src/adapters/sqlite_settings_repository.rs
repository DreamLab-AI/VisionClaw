// src/adapters/sqlite_settings_repository.rs
//! SQLite Settings Repository Adapter (Phase 11 scaffolding).
//!
//! Implements [`SettingsRepository`] over a `tokio-rusqlite` connection,
//! per ADR-11 §D5. This adapter is the **sole authority** for every
//! table living in `settings.sqlite3`, including the audit log catalogued
//! from Section 6 (TENSIONS-RESOLVED §TC-5).
//!
//! ## Schema
//!
//! The schema is held in [`CREATE_SCHEMA`] as a single embedded
//! constant matching `migrations/sqlite/0001_initial.sql` verbatim
//! (modulo whitespace). The constant is the source the adapter applies
//! on first open; the migrations directory is the source the migration
//! tool applies and is the human-authoring surface.
//!
//! ## Per-user resolution (ADR-11 §D5)
//!
//! A read for key `K` by user `U` returns the row at `(K, U)` if present,
//! else `(K, NULL)` (the global default). Writes always specify the
//! pubkey explicitly, sourced from the per-request auth context. The
//! pubkey is **not** a method parameter on the trait — the trait surface
//! is frozen (PRD-11 A2). Instead, the adapter holds a task-local
//! context populated by NIP-98 middleware (Section 6) and consults it on
//! every method.
//!
//! ## Phase-1 status
//!
//! See `oxigraph_ontology_repository.rs` header. Method bodies are
//! mostly `todo!(SQL: ...)`; type signatures match the trait exactly.

#![cfg(feature = "persistence-oxigraph")]

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio_rusqlite::Connection;

use crate::config::PhysicsSettings;
use crate::ports::settings_repository::{
    AppFullSettings, Result as RepoResult, SettingValue, SettingsRepository,
    SettingsRepositoryError,
};

/// Embedded canonical schema. Kept here so the adapter is self-bootstrapping
/// when no `migrations/` directory is shipped alongside the binary (e.g.
/// in single-binary deployments per ADR-11 §D1). The on-disk file at
/// `migrations/sqlite/0001_initial.sql` is the human-authoring source;
/// changes there must be mirrored here and vice versa. A unit test in
/// Phase 2 will assert byte equality (modulo whitespace).
pub const CREATE_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store   = MEMORY;

CREATE TABLE IF NOT EXISTS settings (
    key            TEXT    NOT NULL,
    owner_pubkey   TEXT,
    value          TEXT    NOT NULL,
    description    TEXT,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (key, owner_pubkey)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS settings_owner_idx
    ON settings(owner_pubkey, key);

CREATE TABLE IF NOT EXISTS physics_profiles (
    profile_name   TEXT    NOT NULL,
    owner_pubkey   TEXT,
    settings_json  TEXT    NOT NULL,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (profile_name, owner_pubkey)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS schema_migrations (
    id          TEXT PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    actor_pubkey    TEXT,
    request_method  TEXT    NOT NULL,
    request_path    TEXT    NOT NULL,
    status_code     INTEGER NOT NULL,
    detail_json     TEXT
);

CREATE INDEX IF NOT EXISTS audit_log_occurred_idx
    ON audit_log(occurred_at);

CREATE INDEX IF NOT EXISTS audit_log_actor_idx
    ON audit_log(actor_pubkey, occurred_at);

INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0001_initial');
"#;

tokio::task_local! {
    /// Per-request owner pubkey for layered resolution (ADR-11 §D5).
    /// Set by NIP-98 middleware (Section 6) before any handler runs.
    /// Unset → adapter reads/writes the global layer (NULL owner_pubkey).
    pub static CURRENT_OWNER_PUBKEY: Option<String>;
}

/// Helper: pull the current owner pubkey from the task-local, falling back
/// to `None` (global). Wrapped in a free function so non-async sites
/// (Drop impls, sync helpers) can use it via `try_with`.
fn current_owner_pubkey() -> Option<String> {
    CURRENT_OWNER_PUBKEY
        .try_with(|cell| cell.clone())
        .unwrap_or(None)
}

/// SQLite-backed `SettingsRepository`. Holds one `tokio-rusqlite`
/// connection in `Arc` so it can be cheaply cloned into handlers and
/// background actors.
///
/// Note: SQLite is single-writer (ADR-11 §D1 non-goal). Multiple readers
/// are fine; multiple writers are not. The connection is wrapped in
/// `Arc` not `Arc<Mutex>` because `tokio-rusqlite::Connection` already
/// serialises calls onto its own worker thread.
pub struct SqliteSettingsRepository {
    conn: Arc<Connection>,
}

impl SqliteSettingsRepository {
    /// Open (or create) a SQLite database at `db_path`, apply the embedded
    /// [`CREATE_SCHEMA`], and return a new adapter handle.
    ///
    /// Phase 1: signature is final, body is `todo!`. The Phase 2 body
    /// will be roughly:
    ///
    /// ```ignore
    /// let conn = Connection::open(db_path).await
    ///     .map_err(|e| SettingsRepositoryError::DatabaseError(e.to_string()))?;
    /// conn.call(|c| { c.execute_batch(CREATE_SCHEMA)?; Ok(()) }).await
    ///     .map_err(|e| SettingsRepositoryError::DatabaseError(e.to_string()))?;
    /// Ok(Self { conn: Arc::new(conn) })
    /// ```
    pub async fn open(db_path: &Path) -> RepoResult<Self> {
        // SQL: applies CREATE_SCHEMA via execute_batch (PRAGMA + DDL + idempotent INSERT).
        todo!("Open SQLite at {db_path:?}; execute_batch(CREATE_SCHEMA); wrap in Arc<Connection>")
    }

    /// Construct over an already-opened connection (used by tests).
    pub fn from_connection(conn: Arc<Connection>) -> Self {
        Self { conn }
    }

    /// Convenience accessor for tests and the audit-log adapter (which
    /// co-locates per ADR-11 §D5).
    pub fn connection(&self) -> &Arc<Connection> {
        &self.conn
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    // ------------------------------------------------------------------
    // 1. get_setting — per-user layered read
    // ------------------------------------------------------------------
    async fn get_setting(&self, _key: &str) -> RepoResult<Option<SettingValue>> {
        // SQL (per-user resolution, ADR-11 §D5):
        //   SELECT value FROM settings
        //   WHERE key = ?1
        //     AND (owner_pubkey = ?2 OR owner_pubkey IS NULL)
        //   ORDER BY owner_pubkey IS NULL ASC   -- non-NULL first
        //   LIMIT 1;
        // params: (key, current_owner_pubkey())
        // Decode `value` (JSON) into SettingValue via serde_json.
        let _ = current_owner_pubkey();
        todo!("SQL: SELECT value FROM settings WHERE key=?1 AND (owner_pubkey=?2 OR owner_pubkey IS NULL) ORDER BY owner_pubkey IS NULL ASC LIMIT 1")
    }

    // ------------------------------------------------------------------
    // 2. set_setting — pubkey-explicit write
    // ------------------------------------------------------------------
    async fn set_setting(
        &self,
        _key: &str,
        _value: SettingValue,
        _description: Option<&str>,
    ) -> RepoResult<()> {
        // SQL (UPSERT on the WITHOUT ROWID PK):
        //   INSERT INTO settings (key, owner_pubkey, value, description, updated_at)
        //   VALUES (?1, ?2, ?3, ?4, unixepoch())
        //   ON CONFLICT(key, owner_pubkey)
        //   DO UPDATE SET value = excluded.value,
        //                 description = COALESCE(excluded.description, description),
        //                 updated_at = unixepoch();
        // params: (key, current_owner_pubkey(), json(value), description)
        todo!("SQL: UPSERT INTO settings(key, owner_pubkey, value, description, updated_at)")
    }

    // ------------------------------------------------------------------
    // 3. delete_setting
    // ------------------------------------------------------------------
    async fn delete_setting(&self, _key: &str) -> RepoResult<()> {
        // SQL:
        //   DELETE FROM settings
        //   WHERE key = ?1
        //     AND (owner_pubkey = ?2 OR (?2 IS NULL AND owner_pubkey IS NULL));
        todo!("SQL: DELETE FROM settings WHERE key=?1 AND owner_pubkey matches current pubkey (or NULL)")
    }

    // ------------------------------------------------------------------
    // 4. has_setting
    // ------------------------------------------------------------------
    async fn has_setting(&self, _key: &str) -> RepoResult<bool> {
        // SQL:
        //   SELECT 1 FROM settings
        //   WHERE key = ?1
        //     AND (owner_pubkey = ?2 OR owner_pubkey IS NULL)
        //   LIMIT 1;
        todo!("SQL: SELECT 1 FROM settings WHERE key=?1 AND (owner_pubkey=?2 OR owner_pubkey IS NULL) LIMIT 1")
    }

    // ------------------------------------------------------------------
    // 5. get_settings_batch
    // ------------------------------------------------------------------
    async fn get_settings_batch(&self, _keys: &[String]) -> RepoResult<HashMap<String, SettingValue>> {
        // SQL (parameterised IN clause; Phase 2 builds the placeholders
        // dynamically or uses sqlite carray extension):
        //   SELECT key, value
        //   FROM settings
        //   WHERE key IN (?1, ?2, ..., ?N)
        //     AND (owner_pubkey = ?M OR owner_pubkey IS NULL)
        // Then per-key reduce to non-NULL owner first, NULL owner fallback.
        todo!("SQL: SELECT key,value FROM settings WHERE key IN (?...) AND owner_pubkey layered; fold")
    }

    // ------------------------------------------------------------------
    // 6. set_settings_batch — atomic UPSERT batch
    // ------------------------------------------------------------------
    async fn set_settings_batch(&self, _updates: HashMap<String, SettingValue>) -> RepoResult<()> {
        // SQL: a single transaction containing one UPSERT per entry,
        // all using the same `owner_pubkey` from the task-local context.
        //   BEGIN;
        //   INSERT INTO settings (...) VALUES (...) ON CONFLICT(...) DO UPDATE ...;
        //   -- repeated for each (key, value) pair
        //   COMMIT;
        todo!("SQL: BEGIN; bulk UPSERT INTO settings; COMMIT")
    }

    // ------------------------------------------------------------------
    // 7. list_settings
    // ------------------------------------------------------------------
    async fn list_settings(&self, _prefix: Option<&str>) -> RepoResult<Vec<String>> {
        // SQL:
        //   SELECT DISTINCT key FROM settings
        //   WHERE (owner_pubkey = ?1 OR owner_pubkey IS NULL)
        //     AND (?2 IS NULL OR key LIKE (?2 || '%'))
        //   ORDER BY key;
        todo!("SQL: SELECT DISTINCT key FROM settings WHERE owner layered AND key LIKE prefix")
    }

    // ------------------------------------------------------------------
    // 8. load_all_settings — composite document load
    // ------------------------------------------------------------------
    async fn load_all_settings(&self) -> RepoResult<Option<AppFullSettings>> {
        // SQL (composite document load):
        //   SELECT key, value FROM settings
        //   WHERE (owner_pubkey = ?1 OR owner_pubkey IS NULL)
        //   ORDER BY owner_pubkey IS NULL ASC;
        // Then in Rust: fold (key, value) rows into a flat JSON object,
        // applying the layered-resolution rule (non-NULL wins), then
        // deserialize via `serde_json::from_value::<AppFullSettings>(...)`.
        // The shape conversion (key-path -> nested JSON) uses the same
        // path-accessor utility as the existing Neo4j adapter.
        todo!("SQL: SELECT key,value FROM settings WHERE owner layered; fold into AppFullSettings via path-accessor")
    }

    // ------------------------------------------------------------------
    // 9. save_all_settings — composite document save
    // ------------------------------------------------------------------
    async fn save_all_settings(&self, _settings: &AppFullSettings) -> RepoResult<()> {
        // SQL: flatten AppFullSettings into (key, value) leaf pairs via
        // the path-accessor, then within a single transaction:
        //   BEGIN;
        //   DELETE FROM settings WHERE owner_pubkey = ?1;  -- replace
        //   INSERT INTO settings(key, owner_pubkey, value, updated_at)
        //   VALUES (?, ?, ?, unixepoch()) ...;
        //   COMMIT;
        todo!("SQL: flatten settings; BEGIN; DELETE+INSERT bulk; COMMIT")
    }

    // ------------------------------------------------------------------
    // 10. get_physics_settings
    // ------------------------------------------------------------------
    async fn get_physics_settings(&self, _profile_name: &str) -> RepoResult<PhysicsSettings> {
        // SQL:
        //   SELECT settings_json FROM physics_profiles
        //   WHERE profile_name = ?1
        //     AND (owner_pubkey = ?2 OR owner_pubkey IS NULL)
        //   ORDER BY owner_pubkey IS NULL ASC
        //   LIMIT 1;
        // Deserialize the JSON column into PhysicsSettings.
        // Not-found maps to SettingsRepositoryError::NotFound(profile_name).
        todo!("SQL: SELECT settings_json FROM physics_profiles WHERE profile_name=?1 AND owner layered LIMIT 1")
    }

    // ------------------------------------------------------------------
    // 11. save_physics_settings
    // ------------------------------------------------------------------
    async fn save_physics_settings(
        &self,
        _profile_name: &str,
        _settings: &PhysicsSettings,
    ) -> RepoResult<()> {
        // SQL:
        //   INSERT INTO physics_profiles(profile_name, owner_pubkey, settings_json, updated_at)
        //   VALUES (?1, ?2, ?3, unixepoch())
        //   ON CONFLICT(profile_name, owner_pubkey)
        //   DO UPDATE SET settings_json = excluded.settings_json,
        //                 updated_at = unixepoch();
        todo!("SQL: UPSERT INTO physics_profiles(profile_name, owner_pubkey, settings_json)")
    }

    // ------------------------------------------------------------------
    // 12. list_physics_profiles
    // ------------------------------------------------------------------
    async fn list_physics_profiles(&self) -> RepoResult<Vec<String>> {
        // SQL:
        //   SELECT DISTINCT profile_name FROM physics_profiles
        //   WHERE (owner_pubkey = ?1 OR owner_pubkey IS NULL)
        //   ORDER BY profile_name;
        todo!("SQL: SELECT DISTINCT profile_name FROM physics_profiles WHERE owner layered")
    }

    // ------------------------------------------------------------------
    // 13. delete_physics_profile
    // ------------------------------------------------------------------
    async fn delete_physics_profile(&self, _profile_name: &str) -> RepoResult<()> {
        // SQL:
        //   DELETE FROM physics_profiles
        //   WHERE profile_name = ?1
        //     AND (owner_pubkey = ?2 OR (?2 IS NULL AND owner_pubkey IS NULL));
        todo!("SQL: DELETE FROM physics_profiles WHERE profile_name=?1 AND owner_pubkey matches current")
    }

    // ------------------------------------------------------------------
    // 14. export_settings
    // ------------------------------------------------------------------
    async fn export_settings(&self) -> RepoResult<serde_json::Value> {
        // SQL:
        //   SELECT key, owner_pubkey, value, description, updated_at
        //   FROM settings
        //   ORDER BY owner_pubkey, key;
        // Build a serde_json::Value with shape:
        //   { "global": { key: value, ... },
        //     "users":  { "<pubkey-hex>": { key: value, ... }, ... } }
        // Phase 2 will decide whether physics_profiles is included.
        todo!("SQL: SELECT * FROM settings; group by owner_pubkey into JSON {{global,users}}")
    }

    // ------------------------------------------------------------------
    // 15. import_settings
    // ------------------------------------------------------------------
    async fn import_settings(&self, _settings_json: &serde_json::Value) -> RepoResult<()> {
        // SQL: inverse of export. Transactional:
        //   BEGIN;
        //   DELETE FROM settings;
        //   INSERT INTO settings(...) VALUES (?, ?, ?, ?, unixepoch()) -- per leaf;
        //   COMMIT;
        // Failure rolls back; either we have the new bundle or the old.
        todo!("SQL: BEGIN; DELETE FROM settings; bulk INSERT from JSON; COMMIT")
    }

    // ------------------------------------------------------------------
    // 16. clear_cache — adapter has no cache (ADR-11 §D5 anti-cache stance)
    // ------------------------------------------------------------------
    async fn clear_cache(&self) -> RepoResult<()> {
        // SQLite page cache is internal; ADR-11 §D5 explicitly rejects
        // an application-level read-through cache at our scale. The
        // method exists on the trait for parity with the Neo4j adapter;
        // here it is a no-op.
        Ok(())
    }

    // ------------------------------------------------------------------
    // 17. health_check
    // ------------------------------------------------------------------
    async fn health_check(&self) -> RepoResult<bool> {
        // SQL: a trivial round-trip plus an integrity check on startup
        // hardening (ADR-11 §D10 demands `PRAGMA integrity_check` on
        // restore). For per-request health we just SELECT 1.
        //   SELECT 1;
        todo!("SQL: SELECT 1; (optionally PRAGMA integrity_check on startup)")
    }
}

// Silence unused-error-variant warning on scaffold builds where the
// errors are not yet constructed.
#[allow(dead_code)]
fn _silence_unused_error_variant() -> SettingsRepositoryError {
    SettingsRepositoryError::NotFound(String::new())
}
