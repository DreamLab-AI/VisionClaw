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
//! ## Implementation status (updated 2026-07-03)
//!
//! Fully implemented. Every method body executes real SQL against the
//! `tokio-rusqlite` connection and returns concrete values — there are no
//! live `todo!(SQL: ...)` macros remaining; type signatures match the trait
//! exactly. (Earlier revisions of this header described Phase-1 scaffolding
//! stubs; that description is no longer accurate.)

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
/// changes there must be mirrored here and vice versa. This equality is
/// **not** currently guarded by a test, so the two sources must be kept in
/// sync by hand until such a test is added.
pub const CREATE_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store   = MEMORY;

CREATE TABLE IF NOT EXISTS settings (
    key            TEXT    NOT NULL,
    owner_pubkey   TEXT    NOT NULL DEFAULT '',
    value          TEXT    NOT NULL,
    description    TEXT,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (key, owner_pubkey)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS settings_owner_idx
    ON settings(owner_pubkey, key);

CREATE TABLE IF NOT EXISTS physics_profiles (
    profile_name   TEXT    NOT NULL,
    owner_pubkey   TEXT    NOT NULL DEFAULT '',
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

CREATE TABLE IF NOT EXISTS sync_file_metadata (
    file_name   TEXT PRIMARY KEY,
    sha1        TEXT NOT NULL,
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch())
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS sync_config (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
) WITHOUT ROWID;

INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0002_sync_metadata');
"#;

/// Marker of the one-time migration that turns stored `ssspEnabled: true` off (ADR-2191).
pub const SSSP_SPRING_ADJUST_MIGRATION: &str = "0003_sssp_spring_adjust_off";

/// One-time migration for the `ssspEnabled` default change (ADR-2191, operator decision of
/// 10 November 2026). For every stored `physics` row (every owner layer) whose document has
/// `ssspEnabled` (or its alias `sssp_enabled`) set to true, the row is first copied verbatim to
/// `settings_archive`, then that key alone is set to false. No other key or row is touched;
/// `physics_profiles` are not migrated. The run is one transaction, recorded in
/// `schema_migrations`, so it happens once: a user who turns the setting back on afterwards
/// keeps it on. A row that does not decode as a JSON document is left as it is.
///
/// Returns the owner pubkey of each migrated row (`""` for the global layer).
fn migrate_sssp_spring_adjust_off(c: &mut rusqlite::Connection) -> rusqlite::Result<Vec<String>> {
    let tx = c.transaction()?;
    let done: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE id = ?1)",
        [SSSP_SPRING_ADJUST_MIGRATION],
        |r| r.get(0),
    )?;
    if done {
        return Ok(Vec::new());
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings_archive (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            key           TEXT    NOT NULL,
            owner_pubkey  TEXT    NOT NULL,
            value         TEXT    NOT NULL,
            archived_at   INTEGER NOT NULL DEFAULT (unixepoch()),
            reason        TEXT    NOT NULL
        );",
    )?;
    let rows: Vec<(String, String)> = {
        let mut stmt =
            tx.prepare("SELECT owner_pubkey, value FROM settings WHERE key = 'physics'")?;
        let mapped = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        mapped.collect::<rusqlite::Result<_>>()?
    };
    let mut migrated = Vec::new();
    for (owner, stored) in rows {
        let Ok(SettingValue::Json(mut doc)) = decode_setting_value(&stored) else {
            continue;
        };
        let Some(map) = doc.as_object_mut() else {
            continue;
        };
        let keys = ["ssspEnabled", "sssp_enabled"];
        if !keys
            .iter()
            .any(|k| map.get(*k) == Some(&serde_json::Value::Bool(true)))
        {
            continue;
        }
        for key in keys {
            if map.contains_key(key) {
                map.insert(key.to_string(), serde_json::Value::Bool(false));
            }
        }
        let updated = encode_setting_value(&SettingValue::Json(doc))
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        tx.execute(
            "INSERT INTO settings_archive (key, owner_pubkey, value, reason)
             VALUES ('physics', ?1, ?2, ?3)",
            rusqlite::params![
                &owner,
                &stored,
                format!("{SSSP_SPRING_ADJUST_MIGRATION}: ssspEnabled true -> false (ADR-2191)")
            ],
        )?;
        tx.execute(
            "UPDATE settings SET value = ?1, updated_at = unixepoch()
             WHERE key = 'physics' AND owner_pubkey = ?2",
            rusqlite::params![&updated, &owner],
        )?;
        migrated.push(owner);
    }
    tx.execute(
        "INSERT INTO schema_migrations (id) VALUES (?1)",
        [SSSP_SPRING_ADJUST_MIGRATION],
    )?;
    tx.commit()?;
    Ok(migrated)
}

tokio::task_local! {
    /// Per-request owner pubkey for layered resolution (ADR-11 §D5).
    /// Set by NIP-98 middleware (Section 6) before any handler runs.
    /// Unset → adapter reads/writes the global layer (empty string sentinel).
    pub static CURRENT_OWNER_PUBKEY: Option<String>;
}

/// Pull the current owner pubkey from the task-local, falling back to
/// empty string (global layer). Empty string is the PK sentinel for
/// "no specific user" — avoids NULL in composite PK which breaks ON CONFLICT.
fn current_owner_pubkey() -> String {
    CURRENT_OWNER_PUBKEY
        .try_with(|cell| cell.clone())
        .unwrap_or(None)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Error-mapping helpers
// ---------------------------------------------------------------------------

/// Map `tokio_rusqlite::Error` into the trait error type.
fn map_db_err(e: tokio_rusqlite::Error) -> SettingsRepositoryError {
    SettingsRepositoryError::DatabaseError(e.to_string())
}

/// Map a JSON (de)serialisation error into the trait error type.
fn map_json_err<E: std::fmt::Display>(e: E) -> SettingsRepositoryError {
    SettingsRepositoryError::SerializationError(e.to_string())
}

/// Decode a TEXT column that contains a JSON-encoded `SettingValue`.
fn decode_setting_value(json_text: &str) -> RepoResult<SettingValue> {
    serde_json::from_str::<SettingValue>(json_text).map_err(map_json_err)
}

/// Encode a `SettingValue` to a JSON string for storage in TEXT.
fn encode_setting_value(value: &SettingValue) -> RepoResult<String> {
    serde_json::to_string(value).map_err(map_json_err)
}

/// Flatten a JSON object into dotted key paths → leaf JSON values.
/// Used by `save_all_settings` / `import_settings` to project the
/// `AppFullSettings` document into the `settings` table's key/value rows.
///
/// Arrays and primitive leaves are stored as a single JSON value at the
/// path leading to them; only object branches recurse.
fn flatten_json(
    value: &serde_json::Value,
    prefix: &str,
    out: &mut Vec<(String, serde_json::Value)>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                flatten_json(v, &key, out);
            }
        }
        // Arrays and primitives are leaves — store the JSON at this path.
        _ => out.push((prefix.to_string(), value.clone())),
    }
}

/// Inverse of `flatten_json`: take dotted key paths → JSON values and
/// reconstruct a nested `serde_json::Value` object.
fn unflatten_pairs(pairs: Vec<(String, serde_json::Value)>) -> serde_json::Value {
    let mut root = serde_json::Value::Object(serde_json::Map::new());
    for (path, leaf) in pairs {
        insert_at_path(&mut root, &path, leaf);
    }
    root
}

fn insert_at_path(root: &mut serde_json::Value, path: &str, leaf: serde_json::Value) {
    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        *root = leaf;
        return;
    }

    let mut current = root;
    for (i, seg) in segments.iter().enumerate() {
        // Promote the current node to an object if it isn't one.
        if !current.is_object() {
            *current = serde_json::Value::Object(serde_json::Map::new());
        }
        let obj = current.as_object_mut().expect("just promoted to object");

        if i == segments.len() - 1 {
            obj.insert((*seg).to_string(), leaf);
            return;
        }

        current = obj
            .entry((*seg).to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

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
    /// Also applies the one-time stored-settings migrations, each behind its
    /// `schema_migrations` marker, before any reader can see the rows.
    pub async fn open(db_path: &Path) -> RepoResult<Self> {
        let conn = Connection::open(db_path).await.map_err(map_db_err)?;
        let migrated = conn
            .call(|c| {
                c.execute_batch(CREATE_SCHEMA)?;
                Ok(migrate_sssp_spring_adjust_off(c)?)
            })
            .await
            .map_err(map_db_err)?;
        for owner in migrated {
            let layer = if owner.is_empty() { "global" } else { "user" };
            log::warn!(
                "settings migration {SSSP_SPRING_ADJUST_MIGRATION}: the stored {layer} physics \
                 row{} had ssspEnabled=true; archived it to settings_archive and set \
                 ssspEnabled=false (the new default; ADR-2191). Set it to true again to restore \
                 SSSP spring adjustment.",
                if owner.is_empty() {
                    String::new()
                } else {
                    format!(" for {owner}")
                }
            );
        }
        Ok(Self {
            conn: Arc::new(conn),
        })
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

    // ------------------------------------------------------------------
    // Sync metadata methods (inherent, not on SettingsRepository trait)
    // ------------------------------------------------------------------

    /// Retrieve all file SHA1 hashes from sync_file_metadata.
    pub async fn get_file_sha1s(&self) -> Result<HashMap<String, String>, SettingsRepositoryError> {
        self.conn
            .call(|c| {
                let mut stmt =
                    c.prepare_cached("SELECT file_name, sha1 FROM sync_file_metadata")?;
                let mut rows = stmt.query([])?;
                let mut map: HashMap<String, String> = HashMap::new();
                while let Some(row) = rows.next()? {
                    let name: String = row.get(0)?;
                    let sha1: String = row.get(1)?;
                    map.insert(name, sha1);
                }
                Ok(map)
            })
            .await
            .map_err(map_db_err)
    }

    /// Batch upsert file SHA1 hashes into sync_file_metadata.
    pub async fn upsert_file_sha1s(
        &self,
        files: &[(String, String)],
    ) -> Result<(), SettingsRepositoryError> {
        if files.is_empty() {
            return Ok(());
        }
        let files_owned: Vec<(String, String)> = files.to_vec();
        self.conn
            .call(move |c| {
                let tx = c.transaction()?;
                {
                    let mut stmt = tx.prepare_cached(
                        "INSERT OR REPLACE INTO sync_file_metadata (file_name, sha1, updated_at)
                         VALUES (?1, ?2, unixepoch())",
                    )?;
                    for (name, sha1) in &files_owned {
                        stmt.execute(rusqlite::params![name, sha1])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Read a sync config value by key.
    pub async fn get_sync_config(
        &self,
        key: &str,
    ) -> Result<Option<String>, SettingsRepositoryError> {
        let key_owned = key.to_string();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached("SELECT value FROM sync_config WHERE key = ?1")?;
                let mut rows = stmt.query(rusqlite::params![&key_owned])?;
                if let Some(row) = rows.next()? {
                    let val: String = row.get(0)?;
                    Ok(Some(val))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(map_db_err)
    }

    /// Write a sync config value by key (upsert).
    pub async fn set_sync_config(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(), SettingsRepositoryError> {
        let key_owned = key.to_string();
        let value_owned = value.to_string();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT OR REPLACE INTO sync_config (key, value)
                     VALUES (?1, ?2)",
                )?;
                stmt.execute(rusqlite::params![&key_owned, &value_owned])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Delete all sync metadata (file hashes and config).
    pub async fn clear_sync_metadata(&self) -> Result<(), SettingsRepositoryError> {
        self.conn
            .call(|c| {
                c.execute("DELETE FROM sync_file_metadata", [])?;
                c.execute("DELETE FROM sync_config", [])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    // ------------------------------------------------------------------
    // 1. get_setting — per-user layered read
    // ------------------------------------------------------------------
    async fn get_setting(&self, key: &str) -> RepoResult<Option<SettingValue>> {
        let key_owned = key.to_string();
        let pubkey = current_owner_pubkey();
        self.conn
            .call(move |c| {
                // Per-user resolution (ADR-11 §D5):
                //   * If a row exists at (key, pubkey) return it.
                //   * Else if a row exists at (key, NULL) return it.
                //   * `ORDER BY owner_pubkey = '' ASC` puts non-NULL first.
                let mut stmt = c.prepare_cached(
                    "SELECT value FROM settings
                     WHERE key = ?1
                       AND (owner_pubkey = ?2 OR owner_pubkey = '')
                     ORDER BY (owner_pubkey = '') ASC
                     LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![&key_owned, &pubkey])?;
                if let Some(row) = rows.next()? {
                    let json_text: String = row.get(0)?;
                    Ok(Some(json_text))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(map_db_err)?
            .map(|text| decode_setting_value(&text))
            .transpose()
    }

    // ------------------------------------------------------------------
    // 2. set_setting — pubkey-explicit write
    // ------------------------------------------------------------------
    async fn set_setting(
        &self,
        key: &str,
        value: SettingValue,
        description: Option<&str>,
    ) -> RepoResult<()> {
        let key_owned = key.to_string();
        let description_owned = description.map(|s| s.to_string());
        let pubkey = current_owner_pubkey();
        let value_json = encode_setting_value(&value)?;

        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT INTO settings (key, owner_pubkey, value, description, updated_at)
                     VALUES (?1, ?2, ?3, ?4, unixepoch())
                     ON CONFLICT(key, owner_pubkey)
                     DO UPDATE SET value       = excluded.value,
                                   description = COALESCE(excluded.description, settings.description),
                                   updated_at  = unixepoch()",
                )?;
                stmt.execute(rusqlite::params![
                    &key_owned,
                    &pubkey,
                    &value_json,
                    &description_owned,
                ])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 3. delete_setting
    // ------------------------------------------------------------------
    async fn delete_setting(&self, key: &str) -> RepoResult<()> {
        let key_owned = key.to_string();
        let pubkey = current_owner_pubkey();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "DELETE FROM settings
                     WHERE key = ?1
                       AND ((owner_pubkey = ?2)
                            OR (?2 = '' AND owner_pubkey = ''))",
                )?;
                stmt.execute(rusqlite::params![&key_owned, &pubkey])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 4. has_setting
    // ------------------------------------------------------------------
    async fn has_setting(&self, key: &str) -> RepoResult<bool> {
        let key_owned = key.to_string();
        let pubkey = current_owner_pubkey();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT 1 FROM settings
                     WHERE key = ?1
                       AND (owner_pubkey = ?2 OR owner_pubkey = '')
                     LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![&key_owned, &pubkey])?;
                Ok(rows.next()?.is_some())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 5. get_settings_batch
    // ------------------------------------------------------------------
    async fn get_settings_batch(
        &self,
        keys: &[String],
    ) -> RepoResult<HashMap<String, SettingValue>> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let keys_owned: Vec<String> = keys.to_vec();
        let pubkey = current_owner_pubkey();

        // Build a parameterised IN clause: ?1, ?2, ... ?N plus the pubkey
        // placeholder. We assemble all params dynamically into a Vec.
        let raw_rows: Vec<(String, String)> = self
            .conn
            .call(move |c| {
                let placeholders = (1..=keys_owned.len())
                    .map(|i| format!("?{}", i))
                    .collect::<Vec<_>>()
                    .join(",");
                let pubkey_idx = keys_owned.len() + 1;
                let sql = format!(
                    "SELECT key, owner_pubkey, value FROM settings
                     WHERE key IN ({})
                       AND (owner_pubkey = ?{p} OR owner_pubkey = '')",
                    placeholders,
                    p = pubkey_idx
                );
                let mut stmt = c.prepare(&sql)?;
                let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(pubkey_idx);
                for k in &keys_owned {
                    params.push(k);
                }
                params.push(&pubkey);

                let mut out: Vec<(String, String)> = Vec::new();
                let mut owners: Vec<String> = Vec::new();
                let mut rows = stmt.query(params.as_slice())?;
                while let Some(row) = rows.next()? {
                    let k: String = row.get(0)?;
                    let owner: String = row.get(1)?;
                    let v: String = row.get(2)?;
                    out.push((k, v));
                    owners.push(owner);
                }
                let tagged: Vec<(String, String)> = out
                    .into_iter()
                    .zip(owners)
                    .map(|((k, v), owner)| {
                        let tag = if !owner.is_empty() { "U" } else { "G" };
                        (k, format!("{}\u{0}{}", tag, v))
                    })
                    .collect();
                Ok(tagged)
            })
            .await
            .map_err(map_db_err)?;

        // Per-user resolution fold: prefer the non-NULL-owner row when both
        // are present for the same key.
        let mut layered: HashMap<String, (bool, String)> = HashMap::new();
        for (k, tagged) in raw_rows {
            let mut parts = tagged.splitn(2, '\u{0}');
            let tag = parts.next().unwrap_or("G");
            let json = parts.next().unwrap_or("").to_string();
            let is_user = tag == "U";
            match layered.get(&k) {
                Some((existing_is_user, _)) if *existing_is_user => {
                    // Already have the user-level value, ignore the global.
                }
                _ => {
                    layered.insert(k, (is_user, json));
                }
            }
        }

        let mut out: HashMap<String, SettingValue> = HashMap::with_capacity(layered.len());
        for (k, (_is_user, json)) in layered {
            out.insert(k, decode_setting_value(&json)?);
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // 6. set_settings_batch — atomic UPSERT batch
    // ------------------------------------------------------------------
    async fn set_settings_batch(&self, updates: HashMap<String, SettingValue>) -> RepoResult<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Pre-encode all values outside the worker thread so we can map
        // serialisation failures to the right error variant.
        let mut encoded: Vec<(String, String)> = Vec::with_capacity(updates.len());
        for (k, v) in updates {
            encoded.push((k, encode_setting_value(&v)?));
        }
        let pubkey = current_owner_pubkey();

        self.conn
            .call(move |c| {
                let tx = c.transaction()?;
                {
                    let mut stmt = tx.prepare_cached(
                        "INSERT INTO settings (key, owner_pubkey, value, updated_at)
                         VALUES (?1, ?2, ?3, unixepoch())
                         ON CONFLICT(key, owner_pubkey)
                         DO UPDATE SET value      = excluded.value,
                                       updated_at = unixepoch()",
                    )?;
                    for (k, v) in &encoded {
                        stmt.execute(rusqlite::params![k, &pubkey, v])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 7. list_settings
    // ------------------------------------------------------------------
    async fn list_settings(&self, prefix: Option<&str>) -> RepoResult<Vec<String>> {
        let prefix_owned = prefix.map(|s| s.to_string());
        let pubkey = current_owner_pubkey();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT DISTINCT key FROM settings
                     WHERE (owner_pubkey = ?1 OR owner_pubkey = '')
                       AND (?2 IS NULL OR key LIKE (?2 || '%'))
                     ORDER BY key",
                )?;
                let mut rows = stmt.query(rusqlite::params![&pubkey, &prefix_owned])?;
                let mut out: Vec<String> = Vec::new();
                while let Some(row) = rows.next()? {
                    out.push(row.get(0)?);
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 8. load_all_settings — composite document load
    // ------------------------------------------------------------------
    async fn load_all_settings(&self) -> RepoResult<Option<AppFullSettings>> {
        let pubkey = current_owner_pubkey();
        let rows: Vec<(String, bool, String)> = self
            .conn
            .call(move |c| {
                // `(owner_pubkey = '') ASC` orders user-level rows before
                // global rows. Combined with the fold below, this means the
                // user value wins for any key that exists at both layers.
                let mut stmt = c.prepare_cached(
                    "SELECT key, (owner_pubkey != '') AS is_user, value
                     FROM settings
                     WHERE (owner_pubkey = ?1 OR owner_pubkey = '')
                     ORDER BY (owner_pubkey = '') ASC",
                )?;
                let mut rows = stmt.query(rusqlite::params![&pubkey])?;
                let mut out: Vec<(String, bool, String)> = Vec::new();
                while let Some(row) = rows.next()? {
                    let key: String = row.get(0)?;
                    let is_user: i64 = row.get(1)?;
                    let value: String = row.get(2)?;
                    out.push((key, is_user != 0, value));
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)?;

        if rows.is_empty() {
            return Ok(None);
        }

        // Fold rows into (key → leaf JSON), preferring user-level values.
        let mut layered: HashMap<String, (bool, serde_json::Value)> = HashMap::new();
        for (key, is_user, json_text) in rows {
            // `value` is a JSON-encoded SettingValue. Convert to a plain
            // serde_json::Value so the resulting document is shaped like
            // the AppFullSettings struct serialises to.
            let setting: SettingValue = decode_setting_value(&json_text)?;
            let leaf: serde_json::Value = match setting {
                SettingValue::String(s) => serde_json::Value::String(s),
                SettingValue::Integer(i) => serde_json::Value::from(i),
                SettingValue::Float(f) => serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                SettingValue::Boolean(b) => serde_json::Value::Bool(b),
                SettingValue::Json(v) => v,
            };
            match layered.get(&key) {
                Some((existing_is_user, _)) if *existing_is_user => {
                    // user-level already recorded — global must not overwrite
                }
                _ => {
                    layered.insert(key, (is_user, leaf));
                }
            }
        }

        let pairs: Vec<(String, serde_json::Value)> = layered
            .into_iter()
            .map(|(k, (_is_user, v))| (k, v))
            .collect();
        let root = unflatten_pairs(pairs);
        let mut settings: AppFullSettings = serde_json::from_value(root).map_err(map_json_err)?;
        // The rows are read by key, not from the unflattened tree: a legacy `physics.*` leaf
        // would otherwise be merged into the `physics` document itself.
        let physics_row = match self.get_setting("physics").await {
            Ok(Some(SettingValue::Json(json))) => Some(json),
            _ => None,
        };
        let quality_gates = match self.get_setting("quality_gates").await {
            Ok(Some(SettingValue::Json(json))) => Some(json),
            _ => None,
        };
        seed_physics_from_blob(physics_row, quality_gates.as_ref(), &mut settings);
        let rendering_row = match self.get_setting(RENDERING_ROW).await {
            Ok(Some(SettingValue::Json(json))) => Some(json),
            _ => None,
        };
        seed_rendering_from_blob(rendering_row, &mut settings);
        Ok(Some(settings))
    }

    // ------------------------------------------------------------------
    // 9. save_all_settings — composite document save
    // ------------------------------------------------------------------
    async fn save_all_settings(&self, settings: &AppFullSettings) -> RepoResult<()> {
        // Project AppFullSettings → JSON → flat leaf pairs.
        let root = serde_json::to_value(settings).map_err(map_json_err)?;
        let mut leaves: Vec<(String, serde_json::Value)> = Vec::new();
        flatten_json(&root, "", &mut leaves);

        // Encode each leaf as a JSON-encoded SettingValue::Json so the on-disk
        // representation matches the `set_setting` write path.
        let encoded: Vec<(String, String)> = leaves
            .into_iter()
            .map(|(k, v)| {
                let sv = SettingValue::Json(v);
                let json = serde_json::to_string(&sv).map_err(map_json_err)?;
                Ok::<_, SettingsRepositoryError>((k, json))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let pubkey = current_owner_pubkey();

        self.conn
            .call(move |c| {
                let tx = c.transaction()?;
                {
                    // Replace this owner's slice atomically. NULL-owner rows
                    // (global defaults) are left intact when a user is writing.
                    let mut del = tx.prepare_cached(
                        "DELETE FROM settings
                         WHERE (owner_pubkey = ?1)
                            OR (?1 = '' AND owner_pubkey = '')",
                    )?;
                    del.execute(rusqlite::params![&pubkey])?;

                    let mut ins = tx.prepare_cached(
                        "INSERT INTO settings (key, owner_pubkey, value, updated_at)
                         VALUES (?1, ?2, ?3, unixepoch())",
                    )?;
                    for (k, v) in &encoded {
                        ins.execute(rusqlite::params![k, &pubkey, v])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 10. get_physics_settings
    // ------------------------------------------------------------------
    async fn get_physics_settings(&self, profile_name: &str) -> RepoResult<PhysicsSettings> {
        let name = profile_name.to_string();
        let pubkey = current_owner_pubkey();
        let json: Option<String> = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT settings_json FROM physics_profiles
                     WHERE profile_name = ?1
                       AND (owner_pubkey = ?2 OR owner_pubkey = '')
                     ORDER BY (owner_pubkey = '') ASC
                     LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![&name, &pubkey])?;
                if let Some(row) = rows.next()? {
                    let s: String = row.get(0)?;
                    Ok(Some(s))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(map_db_err)?;

        match json {
            Some(text) => serde_json::from_str::<PhysicsSettings>(&text).map_err(map_json_err),
            None => Err(SettingsRepositoryError::NotFound(profile_name.to_string())),
        }
    }

    // ------------------------------------------------------------------
    // 11. save_physics_settings
    // ------------------------------------------------------------------
    async fn save_physics_settings(
        &self,
        profile_name: &str,
        settings: &PhysicsSettings,
    ) -> RepoResult<()> {
        let name = profile_name.to_string();
        let pubkey = current_owner_pubkey();
        let json = serde_json::to_string(settings).map_err(map_json_err)?;

        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT INTO physics_profiles (profile_name, owner_pubkey, settings_json, updated_at)
                     VALUES (?1, ?2, ?3, unixepoch())
                     ON CONFLICT(profile_name, owner_pubkey)
                     DO UPDATE SET settings_json = excluded.settings_json,
                                   updated_at    = unixepoch()",
                )?;
                stmt.execute(rusqlite::params![&name, &pubkey, &json])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 12. list_physics_profiles
    // ------------------------------------------------------------------
    async fn list_physics_profiles(&self) -> RepoResult<Vec<String>> {
        let pubkey = current_owner_pubkey();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT DISTINCT profile_name FROM physics_profiles
                     WHERE (owner_pubkey = ?1 OR owner_pubkey = '')
                     ORDER BY profile_name",
                )?;
                let mut rows = stmt.query(rusqlite::params![&pubkey])?;
                let mut out: Vec<String> = Vec::new();
                while let Some(row) = rows.next()? {
                    out.push(row.get(0)?);
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 13. delete_physics_profile
    // ------------------------------------------------------------------
    async fn delete_physics_profile(&self, profile_name: &str) -> RepoResult<()> {
        let name = profile_name.to_string();
        let pubkey = current_owner_pubkey();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "DELETE FROM physics_profiles
                     WHERE profile_name = ?1
                       AND ((owner_pubkey = ?2)
                            OR (?2 = '' AND owner_pubkey = ''))",
                )?;
                stmt.execute(rusqlite::params![&name, &pubkey])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 14. export_settings
    // ------------------------------------------------------------------
    async fn export_settings(&self) -> RepoResult<serde_json::Value> {
        // Pull everything, regardless of current pubkey.
        let rows: Vec<(String, String, String, Option<String>, i64)> = self
            .conn
            .call(|c| {
                let mut stmt = c.prepare_cached(
                    "SELECT key, owner_pubkey, value, description, updated_at
                     FROM settings
                     ORDER BY owner_pubkey, key",
                )?;
                let mut rows = stmt.query([])?;
                let mut out: Vec<(String, String, String, Option<String>, i64)> = Vec::new();
                while let Some(row) = rows.next()? {
                    let key: String = row.get(0)?;
                    let owner: String = row.get(1)?;
                    let value: String = row.get(2)?;
                    let description: Option<String> = row.get(3)?;
                    let updated_at: i64 = row.get(4)?;
                    out.push((key, owner, value, description, updated_at));
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)?;

        let mut global = serde_json::Map::new();
        let mut users: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

        for (key, owner, value_json, description, updated_at) in rows {
            let value: serde_json::Value =
                serde_json::from_str(&value_json).unwrap_or(serde_json::Value::Null);
            let mut row_obj = serde_json::Map::new();
            row_obj.insert("value".to_string(), value);
            if let Some(d) = description {
                row_obj.insert("description".to_string(), serde_json::Value::String(d));
            }
            row_obj.insert(
                "updated_at".to_string(),
                serde_json::Value::from(updated_at),
            );

            let row_value = serde_json::Value::Object(row_obj);
            if owner.is_empty() {
                global.insert(key, row_value);
            } else {
                let user_entry = users
                    .entry(owner)
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                if let Some(obj) = user_entry.as_object_mut() {
                    obj.insert(key, row_value);
                }
            }
        }

        let mut bundle = serde_json::Map::new();
        bundle.insert("global".to_string(), serde_json::Value::Object(global));
        bundle.insert("users".to_string(), serde_json::Value::Object(users));
        Ok(serde_json::Value::Object(bundle))
    }

    // ------------------------------------------------------------------
    // 15. import_settings
    // ------------------------------------------------------------------
    async fn import_settings(&self, settings_json: &serde_json::Value) -> RepoResult<()> {
        let bundle = settings_json.as_object().ok_or_else(|| {
            SettingsRepositoryError::InvalidValue(
                "import_settings expects a JSON object with 'global' and 'users' keys".into(),
            )
        })?;

        // Pre-encode rows so we can hand a fully-owned Vec to the worker
        // thread. Each row is (key, owner_pubkey, value_json, description).
        let mut staged: Vec<(String, String, String, Option<String>)> = Vec::new();

        if let Some(serde_json::Value::Object(globals)) = bundle.get("global") {
            for (key, entry) in globals {
                let (value_json, description) = stage_row_entry(entry)?;
                staged.push((key.clone(), String::new(), value_json, description));
            }
        }
        if let Some(serde_json::Value::Object(users)) = bundle.get("users") {
            for (pubkey, settings_for_user) in users {
                if let serde_json::Value::Object(map) = settings_for_user {
                    for (key, entry) in map {
                        let (value_json, description) = stage_row_entry(entry)?;
                        staged.push((key.clone(), pubkey.clone(), value_json, description));
                    }
                }
            }
        }

        self.conn
            .call(move |c| {
                let tx = c.transaction()?;
                tx.execute("DELETE FROM settings", [])?;
                {
                    let mut ins = tx.prepare_cached(
                        "INSERT INTO settings (key, owner_pubkey, value, description, updated_at)
                         VALUES (?1, ?2, ?3, ?4, unixepoch())",
                    )?;
                    for (k, owner, v, desc) in &staged {
                        ins.execute(rusqlite::params![k, owner, v, desc])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    // ------------------------------------------------------------------
    // 16. clear_cache — adapter has no cache (ADR-11 §D5 anti-cache stance)
    // ------------------------------------------------------------------
    async fn clear_cache(&self) -> RepoResult<()> {
        // SQLite page cache is internal; ADR-11 §D5 explicitly rejects
        // an application-level read-through cache at our scale. The
        // method exists on the trait for parity with the original adapter;
        // here it is a no-op.
        Ok(())
    }

    // ------------------------------------------------------------------
    // 17. health_check
    // ------------------------------------------------------------------
    async fn health_check(&self) -> RepoResult<bool> {
        self.conn
            .call(|c| {
                let mut stmt = c.prepare_cached("SELECT 1")?;
                let mut rows = stmt.query([])?;
                let ok = if let Some(row) = rows.next()? {
                    let v: i64 = row.get(0)?;
                    v == 1
                } else {
                    false
                };
                Ok(ok)
            })
            .await
            .map_err(map_db_err)
    }
}

/// Pull `(value_json, description)` out of one row of the export bundle.
///
/// Two input shapes are accepted:
///   * the canonical export shape — an object with `value` and optional
///     `description` fields, or
///   * a bare value — interpreted as the JSON-encoded value with no
///     description.
fn stage_row_entry(entry: &serde_json::Value) -> RepoResult<(String, Option<String>)> {
    if let serde_json::Value::Object(obj) = entry {
        if let Some(value) = obj.get("value") {
            // Canonical shape. The stored `value` column holds a JSON-encoded
            // SettingValue; wrap whatever leaf JSON we got into `SettingValue::Json`
            // so reads round-trip cleanly.
            let sv = SettingValue::Json(value.clone());
            let value_json = serde_json::to_string(&sv).map_err(map_json_err)?;
            let description = obj
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            return Ok((value_json, description));
        }
    }
    // Fallback: treat the entry itself as the leaf value.
    let sv = SettingValue::Json(entry.clone());
    let value_json = serde_json::to_string(&sv).map_err(map_json_err)?;
    Ok((value_json, None))
}

/// ADR-2140 decision 1: the stored `physics` document is the single source of truth, so it is
/// the settings actor's snapshot. The live store holds only that row (the flattened
/// `visualisation.graphs.knowledge.physics.*` leaves are frozen or absent, because
/// `UpdateSettings` persists nothing and `save_all_settings` has no live caller). The row
/// replaces the snapshot's physics wholesale, with its layout mode migrated from the legacy
/// `quality_gates.layoutMode` string when it predates the field, so the merge base of
/// `apply_physics_patch` is the store and a frozen leaf can never win over it. A row that does
/// not load as physics is logged and leaves the snapshot as it was, so the load never fails
/// because of it.
/// The row PUT /api/settings/rendering stores the rendering document under (settings audit
/// 2026-11-06: before it, the PUT only reached the actor's memory and was lost on restart).
pub const RENDERING_ROW: &str = "rendering";

/// The stored `rendering` row replaces the snapshot's rendering settings. A row that does not
/// load as RenderingSettings is logged and leaves the snapshot as it was.
fn seed_rendering_from_blob(
    row: Option<serde_json::Value>,
    settings: &mut crate::config::AppFullSettings,
) {
    let Some(blob) = row.filter(|r| r.is_object()) else {
        return;
    };
    match serde_json::from_value::<crate::config::RenderingSettings>(blob) {
        Ok(rendering) => settings.visualisation.rendering = rendering,
        Err(e) => log::warn!("stored rendering document is unusable ({e}); keeping the defaults"),
    }
}

fn seed_physics_from_blob(
    row: Option<serde_json::Value>,
    quality_gates: Option<&serde_json::Value>,
    settings: &mut crate::config::AppFullSettings,
) {
    let Some(mut blob) = row.filter(|p| p.is_object()) else {
        return;
    };
    crate::actors::gpu::physics_restore::migrate_layout_mode(&mut blob, quality_gates);
    match serde_json::from_value::<crate::config::PhysicsSettings>(blob) {
        Ok(physics) => settings.visualisation.graphs.knowledge.physics = physics,
        Err(e) => log::warn!("stored physics document is unusable as the settings snapshot ({e})"),
    }
}

#[cfg(test)]
mod user_filter_persistence_tests {
    //! Round-trip coverage for the per-user filter/settings persistence path
    //! wired by the REST (`PUT/GET /api/user/filter`) and WebSocket
    //! (`filter_update`) handlers. Both scope [`CURRENT_OWNER_PUBKEY`] to the
    //! authenticated pubkey and read/write the `settings` table's owner layer
    //! (ADR-11 §D5), so exercising the adapter under that scope verifies the
    //! mechanism the handlers depend on.
    use super::*;

    async fn mem_repo() -> SqliteSettingsRepository {
        SqliteSettingsRepository::open(Path::new(":memory:"))
            .await
            .expect("open in-memory settings repo")
    }

    #[tokio::test]
    async fn user_filter_round_trips_within_owner_scope() {
        let repo = mem_repo().await;
        let alice = "npub_alice".to_string();

        let filter = serde_json::json!({
            "pubkey": alice,
            "enabled": true,
            "quality_threshold": 0.9,
            "max_nodes": 500,
        });

        CURRENT_OWNER_PUBKEY
            .scope(Some(alice.clone()), async {
                repo.set_setting(
                    "user_filter",
                    SettingValue::Json(filter.clone()),
                    Some("test"),
                )
                .await
            })
            .await
            .expect("persist alice filter");

        let got = CURRENT_OWNER_PUBKEY
            .scope(Some(alice.clone()), async {
                repo.get_setting("user_filter").await
            })
            .await
            .expect("read alice filter");

        if let Some(SettingValue::Json(v)) = got {
            assert_eq!(v, filter, "persisted filter must round-trip unchanged");
        } else {
            panic!("expected Alice's persisted filter, got {:?}", got.is_some());
        }
    }

    #[tokio::test]
    async fn user_filter_is_isolated_per_owner() {
        let repo = mem_repo().await;
        let alice = "npub_alice".to_string();
        let bob = "npub_bob".to_string();

        CURRENT_OWNER_PUBKEY
            .scope(Some(alice.clone()), async {
                repo.set_setting(
                    "user_filter",
                    SettingValue::Json(serde_json::json!({ "pubkey": alice, "enabled": true })),
                    Some("test"),
                )
                .await
            })
            .await
            .expect("persist alice filter");

        // Bob stored nothing and no global row exists → the layered read yields None.
        let bob_view = CURRENT_OWNER_PUBKEY
            .scope(Some(bob.clone()), async {
                repo.get_setting("user_filter").await
            })
            .await
            .expect("read bob filter");

        assert!(
            bob_view.is_none(),
            "Bob must not observe Alice's per-user filter"
        );
    }
}

#[cfg(test)]
mod layout_mode_snapshot_tests {
    //! ADR-2140 decision 1: the `physics` row seeds the settings actor's snapshot.
    use super::seed_physics_from_blob;
    use crate::config::{AppFullSettings, PhysicsSettings};
    use serde_json::json;
    use visionclaw_domain::types::layout::LayoutMode;

    fn document(mode: &str, repel_k: f64) -> serde_json::Value {
        let mut doc = serde_json::to_value(PhysicsSettings::default()).unwrap();
        doc["layoutMode"] = json!(mode);
        doc["repelK"] = json!(repel_k);
        doc
    }

    fn defaults() -> AppFullSettings {
        AppFullSettings::new().expect("default settings")
    }

    #[test]
    fn the_row_replaces_the_snapshot_physics() {
        let mut s = defaults();
        seed_physics_from_blob(Some(document("radial", 321.0)), None, &mut s);
        let p = &s.visualisation.graphs.knowledge.physics;
        assert_eq!((p.layout_mode, p.repel_k), (LayoutMode::Radial, 321.0));
    }

    #[test]
    fn the_legacy_quality_gate_string_migrates_a_row_that_predates_the_field() {
        let mut row = document("radial", 5.0);
        row.as_object_mut().unwrap().remove("layoutMode");
        let mut s = defaults();
        let gates = json!({ "layoutMode": "dag-topdown" });
        seed_physics_from_blob(Some(row), Some(&gates), &mut s);
        assert_eq!(
            s.visualisation.graphs.knowledge.physics.layout_mode,
            LayoutMode::Hierarchical
        );
    }

    #[test]
    fn an_unusable_or_absent_row_leaves_the_snapshot_alone() {
        let mut s = defaults();
        seed_physics_from_blob(Some(json!({ "repelK": "not a number" })), None, &mut s);
        seed_physics_from_blob(Some(json!("not an object")), None, &mut s);
        seed_physics_from_blob(None, None, &mut s);
        assert_eq!(
            s.visualisation.graphs.knowledge.physics.repel_k,
            PhysicsSettings::default().repel_k
        );
    }

    /// The real load path against a store shaped like the live one: the `physics` row and
    /// nothing else (R13 F2). The actor loads this at startup.
    #[tokio::test]
    async fn the_actor_load_of_a_blob_only_store_sees_the_blob() {
        use super::{SettingValue, SettingsRepository, SqliteSettingsRepository};
        let repo = SqliteSettingsRepository::open(std::path::Path::new(":memory:"))
            .await
            .expect("open in-memory repo");
        repo.set_setting(
            "physics",
            SettingValue::Json(document("radial", 321.0)),
            Some("test"),
        )
        .await
        .unwrap();
        let loaded = repo.load_all_settings().await.unwrap().unwrap();
        let p = loaded.visualisation.graphs.knowledge.physics;
        assert_eq!((p.layout_mode, p.repel_k), (LayoutMode::Radial, 321.0));
    }

    /// A store that also holds frozen leaves (an older install): the row still wins.
    #[tokio::test]
    async fn a_frozen_leaf_never_wins_over_the_row() {
        use super::{SettingValue, SettingsRepository, SqliteSettingsRepository};
        let repo = SqliteSettingsRepository::open(std::path::Path::new(":memory:"))
            .await
            .expect("open in-memory repo");
        let mut frozen = defaults();
        frozen.visualisation.graphs.knowledge.physics.layout_mode = LayoutMode::Radial;
        frozen.visualisation.graphs.knowledge.physics.repel_k = 1.0;
        repo.save_all_settings(&frozen).await.expect("save leaves");
        repo.set_setting(
            "physics",
            SettingValue::Json(document("hierarchical", 321.0)),
            Some("test"),
        )
        .await
        .unwrap();
        let loaded = repo.load_all_settings().await.unwrap().unwrap();
        let p = loaded.visualisation.graphs.knowledge.physics;
        assert_eq!(
            (p.layout_mode, p.repel_k),
            (LayoutMode::Hierarchical, 321.0)
        );
    }

    /// Settings audit 2026-11-06: PUT /api/settings/rendering only updated the actor's memory
    /// (`UpdateSettings` persists nothing), so ambient/direct light and the background reset on
    /// every restart. The handler now stores a `rendering` row; the load must seed it.
    #[tokio::test]
    async fn a_stored_rendering_row_seeds_the_actor_load() {
        use super::{SettingValue, SettingsRepository, SqliteSettingsRepository};
        let repo = SqliteSettingsRepository::open(std::path::Path::new(":memory:"))
            .await
            .expect("open in-memory repo");
        let mut rendering =
            serde_json::to_value(crate::config::RenderingSettings::default()).unwrap();
        rendering["ambientLightIntensity"] = json!(0.7);
        rendering["backgroundColor"] = json!("#102030");
        repo.set_setting(
            super::RENDERING_ROW,
            SettingValue::Json(rendering),
            Some("test"),
        )
        .await
        .unwrap();
        let loaded = repo.load_all_settings().await.unwrap().unwrap();
        let r = loaded.visualisation.rendering;
        assert!((r.ambient_light_intensity - 0.7).abs() < 1e-6);
        assert_eq!(r.background_color, "#102030");
    }

    /// A rendering row that does not load as RenderingSettings leaves the defaults in place.
    #[tokio::test]
    async fn an_unusable_rendering_row_never_fails_the_load() {
        use super::{SettingValue, SettingsRepository, SqliteSettingsRepository};
        let repo = SqliteSettingsRepository::open(std::path::Path::new(":memory:"))
            .await
            .expect("open in-memory repo");
        repo.set_setting(
            super::RENDERING_ROW,
            SettingValue::Json(json!({"ambientLightIntensity": "loud"})),
            Some("test"),
        )
        .await
        .unwrap();
        let loaded = repo.load_all_settings().await.unwrap().unwrap();
        assert_eq!(
            loaded.visualisation.rendering.background_color,
            crate::config::RenderingSettings::default().background_color
        );
    }
}

#[cfg(test)]
mod sssp_default_migration_tests {
    use super::*;

    fn store(c: &rusqlite::Connection, key: &str, owner: &str, value: serde_json::Value) {
        c.execute(
            "INSERT INTO settings (key, owner_pubkey, value) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                key,
                owner,
                encode_setting_value(&SettingValue::Json(value)).unwrap()
            ],
        )
        .unwrap();
    }

    fn physics(c: &rusqlite::Connection, owner: &str) -> serde_json::Value {
        let text: String = c
            .query_row(
                "SELECT value FROM settings WHERE key = 'physics' AND owner_pubkey = ?1",
                [owner],
                |r| r.get(0),
            )
            .unwrap();
        match decode_setting_value(&text).unwrap() {
            SettingValue::Json(v) => v,
            other => panic!("not a document: {other:?}"),
        }
    }

    fn archive(c: &rusqlite::Connection) -> Vec<(String, String)> {
        let mut stmt = c
            .prepare("SELECT owner_pubkey, value FROM settings_archive ORDER BY owner_pubkey")
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn stored_sssp_true_is_archived_then_turned_off_once_and_nothing_else_moves() {
        let mut c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(CREATE_SCHEMA).unwrap();
        let global = serde_json::json!({"ssspEnabled": true, "linLogMode": false, "springK": 3.5});
        store(&c, "physics", "", global.clone());
        store(
            &c,
            "physics",
            "npub-off",
            serde_json::json!({"ssspEnabled": false, "springK": 1.0}),
        );
        store(
            &c,
            "physics",
            "npub-alias",
            serde_json::json!({"sssp_enabled": true}),
        );
        store(
            &c,
            "quality_gates",
            "",
            serde_json::json!({"ssspEnabled": true}),
        );
        let before_global = physics(&c, "");

        let mut migrated = migrate_sssp_spring_adjust_off(&mut c).unwrap();
        migrated.sort();
        assert_eq!(migrated, vec!["".to_string(), "npub-alias".to_string()]);

        // Only the one key changed, on the rows that had it on.
        let mut expected = before_global.clone();
        expected["ssspEnabled"] = serde_json::json!(false);
        assert_eq!(physics(&c, ""), expected);
        assert_eq!(
            physics(&c, "npub-alias"),
            serde_json::json!({"sssp_enabled": false})
        );
        assert_eq!(
            physics(&c, "npub-off"),
            serde_json::json!({"ssspEnabled": false, "springK": 1.0})
        );
        let gates: String = c
            .query_row(
                "SELECT value FROM settings WHERE key = 'quality_gates'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            decode_setting_value(&gates).unwrap(),
            SettingValue::Json(serde_json::json!({"ssspEnabled": true})),
            "another key was touched"
        );

        // The archive holds the rows exactly as they were stored.
        let archived = archive(&c);
        assert_eq!(archived.len(), 2);
        assert_eq!(archived[0].0, "");
        assert_eq!(
            decode_setting_value(&archived[0].1).unwrap(),
            SettingValue::Json(global)
        );

        // Once only: a user who turns it back on keeps it on.
        c.execute(
            "UPDATE settings SET value = ?1 WHERE key = 'physics' AND owner_pubkey = ''",
            [encode_setting_value(&SettingValue::Json(
                serde_json::json!({"ssspEnabled": true}),
            ))
            .unwrap()],
        )
        .unwrap();
        assert!(migrate_sssp_spring_adjust_off(&mut c).unwrap().is_empty());
        assert_eq!(physics(&c, "")["ssspEnabled"], serde_json::json!(true));
        assert_eq!(archive(&c).len(), 2);
    }

    #[tokio::test]
    async fn open_migrates_an_existing_database_before_settings_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.db");
        {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.execute_batch(CREATE_SCHEMA).unwrap();
            c.execute(
                "DELETE FROM schema_migrations WHERE id = ?1",
                [SSSP_SPRING_ADJUST_MIGRATION],
            )
            .unwrap();
            let mut doc = serde_json::to_value(crate::config::PhysicsSettings::default()).unwrap();
            doc["ssspEnabled"] = serde_json::json!(true);
            store(&c, "physics", "", doc);
        }
        let repo = SqliteSettingsRepository::open(&path).await.unwrap();
        let settings = repo.load_all_settings().await.unwrap().expect("settings");
        assert!(!settings.visualisation.graphs.knowledge.physics.sssp_enabled);
    }
}
