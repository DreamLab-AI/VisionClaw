-- ----------------------------------------------------------------------------
-- migrations/sqlite/0001_initial.sql
--
-- Initial SQLite schema for VisionFlow `settings.sqlite3`.
--
-- This schema is the **sole authoritative copy** of every table that lives
-- in the settings database. It mirrors ADR-11 §D5 verbatim and is the
-- contract that ADR-05 (Settings & Control Panel) and ADR-06 (Auth &
-- Security) defer to for all storage and operational concerns.
--
-- Per TENSIONS-RESOLVED.md §TC-5: any `CREATE TABLE` statement living
-- outside `migrations/sqlite/` (or `docs/migration-sprint/11-persistence-migration/`)
-- is a CI failure. ADR-05 owns the `AppFullSettings` domain shape only;
-- ADR-06 owns audit event semantics only.
--
-- This file is applied exactly once by the SQLite settings adapter on
-- startup, and is registered in `schema_migrations` with id `0001_initial`.
-- ----------------------------------------------------------------------------

PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store   = MEMORY;

-- ----------------------------------------------------------------------------
-- settings: the canonical key/value/owner triple.
--
-- Per-user resolution semantics (D5 §"layered in the adapter"):
--   * A read for key K by user U returns the row with (K, U) if present,
--     else (K, NULL) (global default).
--   * Writes always specify the pubkey explicitly (NULL = global).
--   * Anonymous sessions read from the NULL-owner layer only.
--
-- The `value` column holds a JSON-encoded `SettingValue` (matching the
-- existing serde tag shape). SQLite's JSON1 extension allows ad-hoc
-- inspection of the column without schema rework.
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS settings (
    key            TEXT    NOT NULL,
    owner_pubkey   TEXT,                       -- NULL = global
    value          TEXT    NOT NULL,           -- JSON-encoded SettingValue
    description    TEXT,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (key, owner_pubkey)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS settings_owner_idx
    ON settings(owner_pubkey, key);

-- ----------------------------------------------------------------------------
-- physics_profiles: per-user named physics profile JSON.
--
-- Profile name is unique per owner. Named profiles allow a user to switch
-- between layout configurations without overwriting their default.
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS physics_profiles (
    profile_name   TEXT    NOT NULL,
    owner_pubkey   TEXT,                       -- NULL = global default
    settings_json  TEXT    NOT NULL,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (profile_name, owner_pubkey)
) WITHOUT ROWID;

-- ----------------------------------------------------------------------------
-- schema_migrations: records which SQL migrations have been applied.
--
-- Distinct from `AppFullSettings::schema_version` (owned by ADR-05) which
-- tracks the document shape of an individual user's stored settings row;
-- this table tracks which migrations have run against the database itself.
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS schema_migrations (
    id          TEXT PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

-- ----------------------------------------------------------------------------
-- audit_log: append-only audit event log.
--
-- Semantics owned by ADR-06 §D6 (audit event types, redaction rules,
-- retention policy). Storage owned here. Rotation into monthly archive
-- tables `audit_log_archive_YYYYMM` is performed by the audit-log adapter
-- on a schedule defined in ADR-06; this initial migration creates the
-- live `audit_log` table only.
--
-- A new archive table is created lazily on first write to a new month;
-- the layout is identical to `audit_log`. See
-- `docs/operations/audit-log-retention.md` (Section 9) for the operator
-- runbook covering `DROP TABLE audit_log_archive_YYYYMM` and the default
-- 24-month retention policy.
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    actor_pubkey    TEXT,                      -- NULL = anonymous / system
    request_method  TEXT    NOT NULL,
    request_path    TEXT    NOT NULL,
    status_code     INTEGER NOT NULL,
    detail_json     TEXT
);

CREATE INDEX IF NOT EXISTS audit_log_occurred_idx
    ON audit_log(occurred_at);

CREATE INDEX IF NOT EXISTS audit_log_actor_idx
    ON audit_log(actor_pubkey, occurred_at);

-- ----------------------------------------------------------------------------
-- audit_log_archive_template (DDL reference, NOT created).
--
-- Monthly archive tables are created on demand by the audit-log adapter
-- when the first event of a new month is written. The DDL below is the
-- canonical shape, kept here as documentation; the adapter substitutes
-- `YYYYMM` and executes the statement at rotation time.
--
--   CREATE TABLE IF NOT EXISTS audit_log_archive_YYYYMM (
--       id              INTEGER PRIMARY KEY AUTOINCREMENT,
--       occurred_at     INTEGER NOT NULL,
--       actor_pubkey    TEXT,
--       request_method  TEXT    NOT NULL,
--       request_path    TEXT    NOT NULL,
--       status_code     INTEGER NOT NULL,
--       detail_json     TEXT
--   );
--   CREATE INDEX IF NOT EXISTS audit_log_archive_YYYYMM_occurred_idx
--       ON audit_log_archive_YYYYMM(occurred_at);
-- ----------------------------------------------------------------------------

-- Mark this migration as applied. (The adapter will idempotently insert
-- the row on first apply; this statement is harmless on subsequent runs.)
INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0001_initial');
