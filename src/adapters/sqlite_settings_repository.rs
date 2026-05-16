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

// ---------------------------------------------------------------------------
// Error-mapping helpers
// ---------------------------------------------------------------------------

/// Map `tokio_rusqlite::Error` into the trait error type.
fn map_db_err(e: tokio_rusqlite::Error) -> SettingsRepositoryError {
    SettingsRepositoryError::DatabaseError(e.to_string())
}

/// Map a raw `rusqlite::Error` (occurring inside a `call` closure) into
/// the trait error type.
fn map_rusqlite_err(e: rusqlite::Error) -> SettingsRepositoryError {
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
fn flatten_json(value: &serde_json::Value, prefix: &str, out: &mut Vec<(String, serde_json::Value)>) {
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
    pub async fn open(db_path: &Path) -> RepoResult<Self> {
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
    async fn get_setting(&self, key: &str) -> RepoResult<Option<SettingValue>> {
        let key_owned = key.to_string();
        let pubkey = current_owner_pubkey();
        self.conn
            .call(move |c| {
                // Per-user resolution (ADR-11 §D5):
                //   * If a row exists at (key, pubkey) return it.
                //   * Else if a row exists at (key, NULL) return it.
                //   * `ORDER BY owner_pubkey IS NULL ASC` puts non-NULL first.
                let mut stmt = c.prepare_cached(
                    "SELECT value FROM settings
                     WHERE key = ?1
                       AND (owner_pubkey = ?2 OR owner_pubkey IS NULL)
                     ORDER BY (owner_pubkey IS NULL) ASC
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
                            OR (?2 IS NULL AND owner_pubkey IS NULL))",
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
                       AND (owner_pubkey = ?2 OR owner_pubkey IS NULL)
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
                       AND (owner_pubkey = ?{p} OR owner_pubkey IS NULL)",
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
                let mut owners: Vec<Option<String>> = Vec::new();
                let mut rows = stmt.query(params.as_slice())?;
                while let Some(row) = rows.next()? {
                    let k: String = row.get(0)?;
                    let owner: Option<String> = row.get(1)?;
                    let v: String = row.get(2)?;
                    out.push((k, v));
                    owners.push(owner);
                }
                // Tag each row with owner flag so we can fold in Rust:
                // pack owner-flag into the value-string prefix is fragile;
                // instead return a parallel structure by encoding owner
                // presence into a third tuple element via re-collecting.
                let tagged: Vec<(String, String)> = out
                    .into_iter()
                    .zip(owners.into_iter())
                    .map(|((k, v), owner)| {
                        // Use a delimiter that cannot appear in JSON: NUL byte.
                        let tag = if owner.is_some() { "U" } else { "G" };
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
    async fn set_settings_batch(
        &self,
        updates: HashMap<String, SettingValue>,
    ) -> RepoResult<()> {
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
                     WHERE (owner_pubkey = ?1 OR owner_pubkey IS NULL)
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
                // `(owner_pubkey IS NULL) ASC` orders user-level rows before
                // global rows. Combined with the fold below, this means the
                // user value wins for any key that exists at both layers.
                let mut stmt = c.prepare_cached(
                    "SELECT key, (owner_pubkey IS NOT NULL) AS is_user, value
                     FROM settings
                     WHERE (owner_pubkey = ?1 OR owner_pubkey IS NULL)
                     ORDER BY (owner_pubkey IS NULL) ASC",
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
        let settings: AppFullSettings = serde_json::from_value(root).map_err(map_json_err)?;
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
                            OR (?1 IS NULL AND owner_pubkey IS NULL)",
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
                       AND (owner_pubkey = ?2 OR owner_pubkey IS NULL)
                     ORDER BY (owner_pubkey IS NULL) ASC
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
                     WHERE (owner_pubkey = ?1 OR owner_pubkey IS NULL)
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
                            OR (?2 IS NULL AND owner_pubkey IS NULL))",
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
        let rows: Vec<(String, Option<String>, String, Option<String>, i64)> = self
            .conn
            .call(|c| {
                let mut stmt = c.prepare_cached(
                    "SELECT key, owner_pubkey, value, description, updated_at
                     FROM settings
                     ORDER BY owner_pubkey, key",
                )?;
                let mut rows = stmt.query([])?;
                let mut out: Vec<(String, Option<String>, String, Option<String>, i64)> = Vec::new();
                while let Some(row) = rows.next()? {
                    let key: String = row.get(0)?;
                    let owner: Option<String> = row.get(1)?;
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
            row_obj.insert("updated_at".to_string(), serde_json::Value::from(updated_at));

            let row_value = serde_json::Value::Object(row_obj);
            match owner {
                None => {
                    global.insert(key, row_value);
                }
                Some(pk) => {
                    let user_entry = users
                        .entry(pk)
                        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                    if let Some(obj) = user_entry.as_object_mut() {
                        obj.insert(key, row_value);
                    }
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
        let mut staged: Vec<(String, Option<String>, String, Option<String>)> = Vec::new();

        if let Some(serde_json::Value::Object(globals)) = bundle.get("global") {
            for (key, entry) in globals {
                let (value_json, description) = stage_row_entry(entry)?;
                staged.push((key.clone(), None, value_json, description));
            }
        }
        if let Some(serde_json::Value::Object(users)) = bundle.get("users") {
            for (pubkey, settings_for_user) in users {
                if let serde_json::Value::Object(map) = settings_for_user {
                    for (key, entry) in map {
                        let (value_json, description) = stage_row_entry(entry)?;
                        staged.push((
                            key.clone(),
                            Some(pubkey.clone()),
                            value_json,
                            description,
                        ));
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
        // method exists on the trait for parity with the Neo4j adapter;
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
fn stage_row_entry(
    entry: &serde_json::Value,
) -> RepoResult<(String, Option<String>)> {
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

// Silence unused-error-variant warnings in scaffold builds where some
// error variants are not yet exercised by the code paths above.
#[allow(dead_code)]
fn _silence_unused_error_variant() -> SettingsRepositoryError {
    SettingsRepositoryError::NotFound(String::new())
}

#[allow(dead_code)]
fn _silence_unused_rusqlite_helper(e: rusqlite::Error) -> SettingsRepositoryError {
    map_rusqlite_err(e)
}
