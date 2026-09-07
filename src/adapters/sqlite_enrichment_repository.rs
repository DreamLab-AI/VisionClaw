// src/adapters/sqlite_enrichment_repository.rs
//! SQLite EnrichmentProposal Repository Adapter (WS-9).
//!
//! Durable lifecycle store for broker enrichment proposals + their governance
//! decisions, backing both the WS-9 decide path and the WS-12 broker inbox.
//!
//! ## Persistence choice
//!
//! `tokio-rusqlite`, mirroring [`crate::adapters::sqlite_settings_repository`]
//! verbatim. SQLite is the project's established embedded durable store
//! (ADR-11 §D1 single-binary), already in the deps (`Cargo.toml` rusqlite
//! bundled + tokio-rusqlite). Oxigraph was the alternative but is wrong for a
//! *mutable lifecycle* store (decide transitions, status filters, offset/limit
//! pagination); Oxigraph holds RDF/KG facts, and the KG *consequence* of an
//! approval is a separate fenced write to `:summary`/`:observed` performed via
//! [`crate::adapters::oxigraph_ontology_repository`]. Postgres rejected: the
//! project's own SQLite is canonical.
//!
//! Lives in its OWN file (`data/enrichment.sqlite3`), not `settings.sqlite3`,
//! so the single-writer constraint is isolated and migrations are independent.
//! Because it is a separate file, [`CREATE_SCHEMA`] self-bootstraps its own
//! `schema_migrations` table (settings.sqlite3's does not exist here).
//!
//! ## Single-writer note
//!
//! Like the settings adapter, SQLite is single-writer. `tokio-rusqlite`
//! serialises every `call` onto its own worker thread, so concurrent
//! `decide()` calls are safe. [`record_decision`](SqliteEnrichmentRepository::record_decision)
//! is one transaction (INSERT decision + UPDATE proposal) so the lifecycle
//! stays atomic — never split across two `call` closures.

use std::path::Path;
use std::sync::Arc;

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio_rusqlite::Connection;

/// Embedded canonical schema. Self-bootstrapping (single-binary deployments,
/// ADR-11 §D1). The on-disk file at `migrations/sqlite/0003_enrichment_proposals.sql`
/// is the human-authoring source; changes there must be mirrored here.
///
/// Unlike the settings DB, this file has no pre-existing `schema_migrations`
/// table — so it is created first.
pub const CREATE_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store   = MEMORY;

CREATE TABLE IF NOT EXISTS schema_migrations (
    id          TEXT PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS enrichment_proposals (
    case_id        TEXT    PRIMARY KEY,
    category       TEXT,
    source_iri     TEXT,
    proposal_json  TEXT    NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'pending',
    created_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch())
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS enrichment_status_idx
    ON enrichment_proposals(status, updated_at);

CREATE TABLE IF NOT EXISTS enrichment_decisions (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    case_id              TEXT    NOT NULL,
    outcome              TEXT    NOT NULL,
    attributed           INTEGER NOT NULL,
    broker_pubkey        TEXT,
    reasoning            TEXT,
    writeback_triggered  INTEGER NOT NULL,
    writeback_committed  INTEGER NOT NULL DEFAULT 0,
    -- REC-10 (PRD-023 WP-12, Insight Ingestion Loop v1): the wall-clock at which
    -- the merged-enrichment stage (the fenced Oxigraph :summary write) actually
    -- landed. NULL until the write-back commits, so Mesh Velocity
    -- (insight-to-integration time) is computable per closed loop. Additive; an
    -- on-disk store predating REC-10 gains it via `apply_additive_migrations`.
    writeback_committed_at_ms INTEGER,
    -- ADR-2006: the id of the signed kind-31403 event this decision came from.
    -- NULL for a locally minted decision (a gate-reject answers no signed
    -- event). Additive; an on-disk store predating ADR-2006 gains it via
    -- `apply_additive_migrations`.
    decision_event_id    TEXT,
    -- ADR-2006: the signed event's own `created_at` (epoch seconds), kept
    -- beside the local receipt time so subscription lag stays measurable.
    decision_created_at_s INTEGER,
    activity_urn         TEXT    NOT NULL,
    proposal_urn         TEXT,
    owner_did            TEXT,
    decided_at_ms        INTEGER NOT NULL,
    FOREIGN KEY(case_id) REFERENCES enrichment_proposals(case_id)
);

CREATE INDEX IF NOT EXISTS enrichment_decision_case_idx
    ON enrichment_decisions(case_id, decided_at_ms);

-- ADR-2006: the partial unique index on `decision_event_id` is created by
-- `apply_additive_migrations`, NOT here. This batch runs first, before the
-- column migration, so on a store predating ADR-2006 the index would reference
-- a column the table does not yet have and abort the whole open.

INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0003_enrichment_proposals');
"#;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum EnrichmentStoreError {
    #[error("db: {0}")]
    Database(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("json: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, EnrichmentStoreError>;

fn map_db_err(e: tokio_rusqlite::Error) -> EnrichmentStoreError {
    EnrichmentStoreError::Database(e.to_string())
}

fn map_json_err<E: std::fmt::Display>(e: E) -> EnrichmentStoreError {
    EnrichmentStoreError::Serialization(e.to_string())
}

/// Idempotent additive migrations for an on-disk store created before a column
/// was added. `CREATE_SCHEMA`'s `CREATE TABLE IF NOT EXISTS` cannot alter an
/// existing table, so a pre-REC-10 `enrichment.sqlite3` needs the new
/// stage-timestamp column added explicitly. Guarded by a `PRAGMA table_info`
/// check so it is a no-op once applied (and safe to run on every `open`).
fn apply_additive_migrations(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    add_column_if_missing(
        c,
        "enrichment_decisions",
        "writeback_committed_at_ms",
        "INTEGER",
    )?;
    // ADR-2006 — signed-event correlation. The unique index is created HERE,
    // after `add_column_if_missing`, so it works for both a fresh store (the
    // column arrives from CREATE TABLE) and one predating ADR-2006 (the column
    // arrives from the ALTER above). A signed decision event is recorded at
    // most once however many times the relay delivers it; the index is partial
    // so locally minted decisions, which carry no event id, are never
    // constrained against each other.
    add_column_if_missing(c, "enrichment_decisions", "decision_event_id", "TEXT")?;
    add_column_if_missing(
        c,
        "enrichment_decisions",
        "decision_created_at_s",
        "INTEGER",
    )?;
    c.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS enrichment_decision_event_idx
             ON enrichment_decisions(decision_event_id)
             WHERE decision_event_id IS NOT NULL;",
    )
}

/// Add `column` to `table` iff it is not already present. `ALTER TABLE … ADD
/// COLUMN` cannot express `IF NOT EXISTS` in SQLite, so the presence check is
/// explicit.
fn add_column_if_missing(
    c: &rusqlite::Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    let present = {
        let mut stmt = c.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = stmt.query([])?;
        let mut found = false;
        while let Some(r) = rows.next()? {
            let name: String = r.get(1)?;
            if name == column {
                found = true;
                break;
            }
        }
        found
    };
    if !present {
        c.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    }
    Ok(())
}

/// Map a broker outcome string to the durable proposal status. Approvals (in
/// any spelling the broker uses) → `approved`; rejections → `rejected`;
/// everything else (amend/delegate/precedent/...) → `reviewed`. Mirrors the
/// broker-bridge status enum (pending|claimed|decided is the *coarse* enum the
/// bridge filters on; here we keep the fine-grained decided sub-state).
pub fn status_for_outcome(outcome: &str) -> &'static str {
    let o = outcome.trim().to_ascii_lowercase();
    match o.as_str() {
        "approve" | "approved" | "accept" | "accepted" | "promote" => "approved",
        s if s.starts_with("reject") => "rejected",
        _ => "reviewed",
    }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// A durable enrichment proposal row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentProposal {
    pub case_id: String,
    pub category: Option<String>,
    pub source_iri: Option<String>,
    pub proposal_json: serde_json::Value,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A durable governance decision row. Mirrors the handler's `RecordedDecision`
/// fields plus the `writeback_committed` truth bit that the in-memory ghost
/// never had (it always claimed `triggered` regardless of whether any write
/// landed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDecision {
    pub case_id: String,
    pub outcome: String,
    pub attributed: bool,
    pub broker_pubkey: Option<String>,
    pub reasoning: Option<String>,
    pub writeback_triggered: bool,
    pub writeback_committed: bool,
    pub activity_urn: String,
    pub proposal_urn: Option<String>,
    pub owner_did: Option<String>,
    pub decided_at_ms: i64,
    /// ADR-2006 — the signed kind-31403 event id this decision came from, or
    /// `None` for a locally minted decision. This is the correlation key back to
    /// the original request; it is unique per signed decision, so a replayed or
    /// re-delivered event is recognisable rather than being recorded twice.
    #[serde(default)]
    pub decision_event_id: Option<String>,
    /// The signed event's `created_at`, in epoch **seconds**. `None` for a
    /// locally minted decision. Kept beside `decided_at_ms` (the local receipt
    /// time) so subscription lag stays measurable after a restart.
    #[serde(default)]
    pub decision_created_at_s: Option<i64>,
}

/// One case's joined loop-trace row (REC-10, Insight Ingestion Loop v1). Joins a
/// governed-write-back-queue proposal to its terminal decision (the latest
/// `decided_at_ms`) so the five-stage loop timeline is assembled from one read.
/// The decision columns are `None` for a proposal still pending in the queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTraceRow {
    pub case_id: String,
    pub category: Option<String>,
    /// Fine-grained proposal status (`pending` / `approved` / `rejected` / …).
    pub status: String,
    /// The proposal body. Stage timestamps that agentbox/the discovery pipeline
    /// stamps on the `ontology_propose` event ride here (`proposed_at_ms` /
    /// `queued_at_ms`); the assembler falls back to `created_at` when absent.
    pub proposal_json: serde_json::Value,
    /// Proposal-row creation instant (unix **seconds**). The moment the insight
    /// entered the governed write-back queue when no finer stamp is on the body.
    pub created_at_s: i64,
    pub updated_at_s: i64,
    /// Terminal decision fields (latest `decided_at_ms`), `None` while pending.
    pub decision_outcome: Option<String>,
    pub decided_at_ms: Option<i64>,
    pub writeback_triggered: Option<bool>,
    pub writeback_committed: Option<bool>,
    /// Merged-enrichment instant — when the fenced Oxigraph write landed (REC-10).
    pub writeback_committed_at_ms: Option<i64>,
    pub activity_urn: Option<String>,
    pub owner_did: Option<String>,
}

/// A broker decision projected for the unified provenance trace (REC-11). Carries
/// the `did:nostr` attribution (`owner_did`) the trace joins on, the PROV-O
/// activity URN, the outcome and the decision instant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceDecisionRow {
    pub case_id: String,
    pub owner_did: Option<String>,
    pub activity_urn: String,
    pub proposal_urn: Option<String>,
    pub outcome: String,
    pub attributed: bool,
    pub decided_at_ms: i64,
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

/// SQLite-backed enrichment proposal + decision store. Holds one
/// `tokio-rusqlite` connection in `Arc` so it can be cheaply cloned into
/// handlers and `AppState`. See module docs for the single-writer posture.
pub struct SqliteEnrichmentRepository {
    conn: Arc<Connection>,
}

impl SqliteEnrichmentRepository {
    /// Open (or create) the enrichment DB at `db_path` and apply [`CREATE_SCHEMA`].
    pub async fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path).await.map_err(map_db_err)?;
        conn.call(|c| {
            c.execute_batch(CREATE_SCHEMA)?;
            apply_additive_migrations(c)?;
            Ok(())
        })
        .await
        .map_err(map_db_err)?;
        Ok(Self {
            conn: Arc::new(conn),
        })
    }

    /// Construct over an already-opened connection (tests).
    pub fn from_connection(conn: Arc<Connection>) -> Self {
        Self { conn }
    }

    /// Convenience accessor for tests.
    pub fn connection(&self) -> &Arc<Connection> {
        &self.conn
    }

    /// Insert or replace a proposal row (idempotent upsert keyed on `case_id`).
    /// `created_at` is preserved on replace via COALESCE against the prior row.
    pub async fn create_or_update(&self, p: &EnrichmentProposal) -> Result<()> {
        let case_id = p.case_id.clone();
        let category = p.category.clone();
        let source_iri = p.source_iri.clone();
        let proposal_json = serde_json::to_string(&p.proposal_json).map_err(map_json_err)?;
        let status = p.status.clone();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT INTO enrichment_proposals
                         (case_id, category, source_iri, proposal_json, status, created_at, updated_at)
                     VALUES
                         (?1, ?2, ?3, ?4, ?5, unixepoch(), unixepoch())
                     ON CONFLICT(case_id) DO UPDATE SET
                         category      = excluded.category,
                         source_iri    = excluded.source_iri,
                         proposal_json = excluded.proposal_json,
                         status        = excluded.status,
                         updated_at    = unixepoch()",
                )?;
                stmt.execute(rusqlite::params![
                    &case_id,
                    &category,
                    &source_iri,
                    &proposal_json,
                    &status,
                ])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Force a proposal's fine-grained status to a terminal lifecycle value
    /// (GOV-2 / ADR-130 Decision 2). Distinct from [`record_decision`], which
    /// derives status from a *governance* outcome (approve/reject): this is the
    /// post-decision, post-PR terminal set the elevation actor's merge poll
    /// writes — `elevated` when the elevation PR merges, `abandoned` when it
    /// closes unmerged. A no-op if the case id is unknown (0 rows updated).
    pub async fn set_status(&self, case_id: &str, status: &str) -> Result<()> {
        let case_id = case_id.to_string();
        let status = status.to_string();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "UPDATE enrichment_proposals
                     SET status = ?2, updated_at = unixepoch()
                     WHERE case_id = ?1",
                )?;
                stmt.execute(rusqlite::params![&case_id, &status])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Fetch one proposal by `case_id`.
    pub async fn get(&self, case_id: &str) -> Result<Option<EnrichmentProposal>> {
        let case_id_owned = case_id.to_string();
        let row: Option<(String, Option<String>, Option<String>, String, String, i64, i64)> = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT case_id, category, source_iri, proposal_json, status, created_at, updated_at
                     FROM enrichment_proposals WHERE case_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![&case_id_owned])?;
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

        match row {
            Some((
                case_id,
                category,
                source_iri,
                proposal_json,
                status,
                created_at,
                updated_at,
            )) => {
                let proposal_json: serde_json::Value =
                    serde_json::from_str(&proposal_json).map_err(map_json_err)?;
                Ok(Some(EnrichmentProposal {
                    case_id,
                    category,
                    source_iri,
                    proposal_json,
                    status,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// List proposals, optionally filtered by status, newest-updated first.
    /// Backs the WS-12 inbox (offset/limit pagination).
    pub async fn list(
        &self,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EnrichmentProposal>> {
        let status_owned = status.map(|s| s.to_string());
        let rows: Vec<(String, Option<String>, Option<String>, String, String, i64, i64)> = self
            .conn
            .call(move |c| {
                // `?1 IS NULL OR status = ?1` collapses the optional filter into
                // one prepared statement.
                let mut stmt = c.prepare_cached(
                    "SELECT case_id, category, source_iri, proposal_json, status, created_at, updated_at
                     FROM enrichment_proposals
                     WHERE (?1 IS NULL OR status = ?1)
                     ORDER BY updated_at DESC
                     LIMIT ?2 OFFSET ?3",
                )?;
                let mut q = stmt.query(rusqlite::params![&status_owned, limit, offset])?;
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
                    ));
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)?;

        let mut out = Vec::with_capacity(rows.len());
        for (case_id, category, source_iri, proposal_json, status, created_at, updated_at) in rows {
            let proposal_json: serde_json::Value =
                serde_json::from_str(&proposal_json).map_err(map_json_err)?;
            out.push(EnrichmentProposal {
                case_id,
                category,
                source_iri,
                proposal_json,
                status,
                created_at,
                updated_at,
            });
        }
        Ok(out)
    }

    /// Count proposals, optionally filtered by status.
    pub async fn count(&self, status: Option<&str>) -> Result<i64> {
        let status_owned = status.map(|s| s.to_string());
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT COUNT(*) FROM enrichment_proposals
                     WHERE (?1 IS NULL OR status = ?1)",
                )?;
                let mut rows = stmt.query(rusqlite::params![&status_owned])?;
                let n: i64 = if let Some(r) = rows.next()? {
                    r.get(0)?
                } else {
                    0
                };
                Ok(n)
            })
            .await
            .map_err(map_db_err)
    }

    /// Atomically claim an approved case before an external operation. False
    /// means another consumer claimed it or the case is not approved.
    pub async fn claim_application(&self, case_id: &str) -> Result<bool> {
        let case_id = case_id.to_owned();
        self.conn
            .call(move |c| {
                Ok(c.execute(
                    "UPDATE enrichment_proposals SET status = 'applying', updated_at = unixepoch()
                 WHERE case_id = ?1 AND status = 'approved'",
                    [case_id],
                )? == 1)
            })
            .await
            .map_err(map_db_err)
    }

    /// Atomically record a decision and transition the parent proposal's status.
    /// ONE transaction — INSERT decision + UPDATE proposal — mirroring the
    /// `upsert_file_sha1s` tx pattern (sqlite_settings_repository.rs:296-310).
    /// Never split into two `call` closures (single-writer atomicity).
    pub async fn record_decision(&self, d: &StoredDecision) -> Result<i64> {
        let case_id = d.case_id.clone();
        let outcome = d.outcome.clone();
        let attributed = d.attributed as i64;
        let broker_pubkey = d.broker_pubkey.clone();
        let reasoning = d.reasoning.clone();
        let writeback_triggered = d.writeback_triggered as i64;
        let writeback_committed = d.writeback_committed as i64;
        let activity_urn = d.activity_urn.clone();
        let proposal_urn = d.proposal_urn.clone();
        let owner_did = d.owner_did.clone();
        let decided_at_ms = d.decided_at_ms;
        let decision_event_id = d.decision_event_id.clone();
        let decision_created_at_s = d.decision_created_at_s;
        let status = status_for_outcome(&outcome).to_string();

        self.conn
            .call(move |c| {
                let tx = c.transaction()?;
                let decision_id: i64;
                {
                    // ADR-2006 — a re-delivered signed decision must not be
                    // recorded twice. The relay can replay a 31403 after a
                    // reconnect or restart, and the subscription starts at
                    // "now" without knowing what it already saw. Returning the
                    // existing row makes redelivery an exact no-op rather than
                    // a duplicate audit entry.
                    if let Some(ref event_id) = decision_event_id {
                        let existing: Option<i64> = tx
                            .query_row(
                                "SELECT id FROM enrichment_decisions
                                 WHERE decision_event_id = ?1",
                                [event_id],
                                |r| r.get(0),
                            )
                            .optional()?;
                        if let Some(id) = existing {
                            tx.commit()?;
                            return Ok(id);
                        }
                    }

                    let mut ins = tx.prepare_cached(
                        "INSERT INTO enrichment_decisions
                             (case_id, outcome, attributed, broker_pubkey, reasoning,
                              writeback_triggered, writeback_committed, activity_urn,
                              proposal_urn, owner_did, decided_at_ms,
                              decision_event_id, decision_created_at_s)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    )?;
                    ins.execute(rusqlite::params![
                        &case_id,
                        &outcome,
                        attributed,
                        &broker_pubkey,
                        &reasoning,
                        writeback_triggered,
                        writeback_committed,
                        &activity_urn,
                        &proposal_urn,
                        &owner_did,
                        decided_at_ms,
                        &decision_event_id,
                        decision_created_at_s,
                    ])?;
                    decision_id = tx.last_insert_rowid();

                    let mut upd = tx.prepare_cached(
                        "UPDATE enrichment_proposals
                         SET status = ?2, updated_at = unixepoch()
                         WHERE case_id = ?1",
                    )?;
                    upd.execute(rusqlite::params![&case_id, &status])?;
                }
                tx.commit()?;
                Ok(decision_id)
            })
            .await
            .map_err(map_db_err)
    }

    /// Flip `writeback_committed = 1` on the latest decision for a case + an
    /// activity URN, after the Oxigraph derived write returned `Ok`, and stamp
    /// the merged-enrichment stage timestamp (`committed_at_ms`, REC-10). Scoped
    /// by `activity_urn` so the correct decision row is marked even if a case has
    /// multiple decisions. The timestamp is only written when it is not already
    /// set, so a re-mark never rewrites the first-commit time (the Mesh Velocity
    /// integration instant stays stable).
    pub async fn mark_writeback_committed(
        &self,
        case_id: &str,
        activity_urn: &str,
        committed_at_ms: i64,
    ) -> Result<()> {
        let case_id_owned = case_id.to_string();
        let activity_owned = activity_urn.to_string();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "UPDATE enrichment_decisions
                     SET writeback_committed = 1,
                         writeback_committed_at_ms = COALESCE(writeback_committed_at_ms, ?3)
                     WHERE case_id = ?1 AND activity_urn = ?2",
                )?;
                stmt.execute(rusqlite::params![
                    &case_id_owned,
                    &activity_owned,
                    committed_at_ms
                ])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Decision outcomes recorded at or after `cutoff_ms`, newest first. Backs
    /// the REC-4 KPI compute (ADR-130 D5): the escalation volume is the row
    /// count and the Trust-Variance dispersion is over the `outcome` column, both
    /// windowed on `decided_at_ms`. Returns `(outcome, activity_urn, decided_at_ms)`
    /// so the KPI lineage can trace a value back to each contributing decision.
    pub async fn decisions_since(&self, cutoff_ms: i64) -> Result<Vec<(String, String, i64)>> {
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT outcome, activity_urn, decided_at_ms
                     FROM enrichment_decisions
                     WHERE decided_at_ms >= ?1
                     ORDER BY decided_at_ms DESC, id DESC",
                )?;
                let mut q = stmt.query(rusqlite::params![cutoff_ms])?;
                let mut out = Vec::new();
                while let Some(r) = q.next()? {
                    out.push((r.get(0)?, r.get(1)?, r.get(2)?));
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)
    }

    /// All decisions for a case, newest first. Backs the WS-9 decision-log read
    /// (the broker-bridge `cases/{id}/history` follow-on).
    pub async fn decisions_for(&self, case_id: &str) -> Result<Vec<StoredDecision>> {
        let case_id_owned = case_id.to_string();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT case_id, outcome, attributed, broker_pubkey, reasoning,
                            writeback_triggered, writeback_committed, activity_urn,
                            proposal_urn, owner_did, decided_at_ms,
                            decision_event_id, decision_created_at_s
                     FROM enrichment_decisions
                     WHERE case_id = ?1
                     ORDER BY decided_at_ms DESC, id DESC",
                )?;
                let mut q = stmt.query(rusqlite::params![&case_id_owned])?;
                let mut out = Vec::new();
                while let Some(r) = q.next()? {
                    let attributed: i64 = r.get(2)?;
                    let writeback_triggered: i64 = r.get(5)?;
                    let writeback_committed: i64 = r.get(6)?;
                    out.push(StoredDecision {
                        case_id: r.get(0)?,
                        outcome: r.get(1)?,
                        attributed: attributed != 0,
                        broker_pubkey: r.get(3)?,
                        reasoning: r.get(4)?,
                        writeback_triggered: writeback_triggered != 0,
                        writeback_committed: writeback_committed != 0,
                        activity_urn: r.get(7)?,
                        proposal_urn: r.get(8)?,
                        owner_did: r.get(9)?,
                        decided_at_ms: r.get(10)?,
                        decision_event_id: r.get(11)?,
                        decision_created_at_s: r.get(12)?,
                    });
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)
    }

    /// Joined loop-trace rows (REC-10), newest proposal first, capped at `limit`.
    /// Each proposal is joined to its **terminal** decision — the one with the
    /// greatest `decided_at_ms` — so the five-stage loop is assembled from a
    /// single read. A pending proposal joins to a row with `None` decision fields.
    pub async fn loop_traces(&self, limit: i64) -> Result<Vec<LoopTraceRow>> {
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT p.case_id, p.category, p.status, p.proposal_json,
                            p.created_at, p.updated_at,
                            d.outcome, d.decided_at_ms, d.writeback_triggered,
                            d.writeback_committed, d.writeback_committed_at_ms,
                            d.activity_urn, d.owner_did
                     FROM enrichment_proposals p
                     LEFT JOIN enrichment_decisions d
                       ON d.case_id = p.case_id
                      AND d.decided_at_ms = (
                            SELECT MAX(d2.decided_at_ms)
                            FROM enrichment_decisions d2
                            WHERE d2.case_id = p.case_id)
                     ORDER BY p.created_at DESC, p.case_id DESC
                     LIMIT ?1",
                )?;
                let mut q = stmt.query(rusqlite::params![limit])?;
                let mut out = Vec::new();
                while let Some(r) = q.next()? {
                    out.push(row_to_loop_trace(r)?);
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)?
            .into_iter()
            .map(finalise_loop_trace)
            .collect()
    }

    /// One case's joined loop-trace row (REC-10), or `None` if the case id is
    /// unknown to the governed write-back queue.
    pub async fn loop_trace_for(&self, case_id: &str) -> Result<Option<LoopTraceRow>> {
        let case_id_owned = case_id.to_string();
        let raw: Option<RawLoopTrace> = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT p.case_id, p.category, p.status, p.proposal_json,
                            p.created_at, p.updated_at,
                            d.outcome, d.decided_at_ms, d.writeback_triggered,
                            d.writeback_committed, d.writeback_committed_at_ms,
                            d.activity_urn, d.owner_did
                     FROM enrichment_proposals p
                     LEFT JOIN enrichment_decisions d
                       ON d.case_id = p.case_id
                      AND d.decided_at_ms = (
                            SELECT MAX(d2.decided_at_ms)
                            FROM enrichment_decisions d2
                            WHERE d2.case_id = p.case_id)
                     WHERE p.case_id = ?1",
                )?;
                let mut q = stmt.query(rusqlite::params![&case_id_owned])?;
                if let Some(r) = q.next()? {
                    Ok(Some(row_to_loop_trace(r)?))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(map_db_err)?;
        raw.map(finalise_loop_trace).transpose()
    }

    /// Broker decisions recorded at or after `cutoff_ms` for the unified
    /// provenance trace (REC-11), newest first. Distinct from
    /// [`decisions_since`](Self::decisions_since) (the KPI numerator read) in that
    /// it carries the `did:nostr` attribution and PROV-O URNs the trace joins on.
    pub async fn provenance_decisions_since(
        &self,
        cutoff_ms: i64,
    ) -> Result<Vec<ProvenanceDecisionRow>> {
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT case_id, owner_did, activity_urn, proposal_urn, outcome,
                            attributed, decided_at_ms
                     FROM enrichment_decisions
                     WHERE decided_at_ms >= ?1
                     ORDER BY decided_at_ms DESC, id DESC",
                )?;
                let mut q = stmt.query(rusqlite::params![cutoff_ms])?;
                let mut out = Vec::new();
                while let Some(r) = q.next()? {
                    let attributed: i64 = r.get(5)?;
                    out.push(ProvenanceDecisionRow {
                        case_id: r.get(0)?,
                        owner_did: r.get(1)?,
                        activity_urn: r.get(2)?,
                        proposal_urn: r.get(3)?,
                        outcome: r.get(4)?,
                        attributed: attributed != 0,
                        decided_at_ms: r.get(6)?,
                    });
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)
    }
}

/// A raw loop-trace row before `proposal_json` is parsed (the SQL text column
/// carries the JSON blob). Deserialised on the worker thread, parsed after.
struct RawLoopTrace {
    case_id: String,
    category: Option<String>,
    status: String,
    proposal_json_text: String,
    created_at_s: i64,
    updated_at_s: i64,
    decision_outcome: Option<String>,
    decided_at_ms: Option<i64>,
    writeback_triggered: Option<bool>,
    writeback_committed: Option<bool>,
    writeback_committed_at_ms: Option<i64>,
    activity_urn: Option<String>,
    owner_did: Option<String>,
}

fn row_to_loop_trace(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawLoopTrace> {
    let triggered: Option<i64> = r.get(8)?;
    let committed: Option<i64> = r.get(9)?;
    Ok(RawLoopTrace {
        case_id: r.get(0)?,
        category: r.get(1)?,
        status: r.get(2)?,
        proposal_json_text: r.get(3)?,
        created_at_s: r.get(4)?,
        updated_at_s: r.get(5)?,
        decision_outcome: r.get(6)?,
        decided_at_ms: r.get(7)?,
        writeback_triggered: triggered.map(|n| n != 0),
        writeback_committed: committed.map(|n| n != 0),
        writeback_committed_at_ms: r.get(10)?,
        activity_urn: r.get(11)?,
        owner_did: r.get(12)?,
    })
}

fn finalise_loop_trace(raw: RawLoopTrace) -> Result<LoopTraceRow> {
    let proposal_json: serde_json::Value =
        serde_json::from_str(&raw.proposal_json_text).map_err(map_json_err)?;
    Ok(LoopTraceRow {
        case_id: raw.case_id,
        category: raw.category,
        status: raw.status,
        proposal_json,
        created_at_s: raw.created_at_s,
        updated_at_s: raw.updated_at_s,
        decision_outcome: raw.decision_outcome,
        decided_at_ms: raw.decided_at_ms,
        writeback_triggered: raw.writeback_triggered,
        writeback_committed: raw.writeback_committed,
        writeback_committed_at_ms: raw.writeback_committed_at_ms,
        activity_urn: raw.activity_urn,
        owner_did: raw.owner_did,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn temp_repo() -> SqliteEnrichmentRepository {
        // tempfile-backed; mirrors sqlite_settings_repository test posture.
        let dir = std::env::temp_dir().join(format!("enrich-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!(
            "enrichment-{}.sqlite3",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        SqliteEnrichmentRepository::open(&path).await.expect("open")
    }

    fn proposal(case_id: &str) -> EnrichmentProposal {
        EnrichmentProposal {
            case_id: case_id.to_string(),
            category: Some("knowledge_enrichment".into()),
            source_iri: Some("urn:ngm:node/foo".into()),
            proposal_json: serde_json::json!({"target_path": "pages/foo.md"}),
            status: "pending".into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn decision(case_id: &str, committed: bool) -> StoredDecision {
        StoredDecision {
            case_id: case_id.to_string(),
            outcome: "approve".into(),
            attributed: true,
            broker_pubkey: Some("a".repeat(64)),
            reasoning: Some("looks good".into()),
            writeback_triggered: true,
            writeback_committed: committed,
            activity_urn: "urn:visionclaw:execution:sha256-12-abcdef012345".into(),
            proposal_urn: Some("urn:visionclaw:kg:pk:sha256-12-deadbeef0000".into()),
            owner_did: Some("did:nostr:pk".into()),
            decided_at_ms: 1_700_000_000_000,
            decision_event_id: None,
            decision_created_at_s: None,
        }
    }

    /// A decision carrying a signed 31403 event id (ADR-2006).
    fn signed_decision(case_id: &str, event_id: &str) -> StoredDecision {
        StoredDecision {
            decision_event_id: Some(event_id.to_string()),
            decision_created_at_s: Some(1_700_000_000),
            ..decision(case_id, false)
        }
    }

    #[tokio::test]
    async fn upsert_get_and_list_roundtrip() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-1")).await.unwrap();
        let got = repo.get("case-1").await.unwrap().expect("present");
        assert_eq!(got.case_id, "case-1");
        assert_eq!(got.status, "pending");
        assert_eq!(repo.count(None).await.unwrap(), 1);
        let listed = repo.list(Some("pending"), 50, 0).await.unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn record_decision_transitions_status_atomically() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-2")).await.unwrap();
        let id = repo
            .record_decision(&decision("case-2", false))
            .await
            .unwrap();
        assert!(id > 0);
        let got = repo.get("case-2").await.unwrap().unwrap();
        assert_eq!(got.status, "approved", "approve outcome ⇒ approved status");
        let decisions = repo.decisions_for("case-2").await.unwrap();
        assert_eq!(decisions.len(), 1);
        assert!(!decisions[0].writeback_committed);
    }

    #[tokio::test]
    async fn mark_writeback_committed_flips_truth_bit() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-3")).await.unwrap();
        let d = decision("case-3", false);
        repo.record_decision(&d).await.unwrap();
        repo.mark_writeback_committed("case-3", &d.activity_urn, 1_700_000_003_000)
            .await
            .unwrap();
        let decisions = repo.decisions_for("case-3").await.unwrap();
        assert!(decisions[0].writeback_committed, "committed now true");
    }

    #[tokio::test]
    async fn loop_trace_joins_proposal_to_terminal_decision_with_monotonic_stamps() {
        // REC-10 (Insight Ingestion Loop v1) end-to-end over the store: a proposal
        // enters the governed write-back queue, is decided, and the merged-
        // enrichment stage commits. The joined loop-trace row must carry an
        // ascending stage timeline so Mesh Velocity is computable.
        let repo = temp_repo().await;
        // The ontology_propose event stamps its capture + queue-entry instants on
        // the proposal body; the assembler prefers them over the coarse created_at.
        let mut p = proposal("loop-1");
        p.proposal_json = serde_json::json!({
            "target_path": "pages/foo.md",
            "proposed_at_ms": 1_700_000_000_000_i64,
            "queued_at_ms":   1_700_000_001_000_i64,
        });
        repo.create_or_update(&p).await.unwrap();

        let mut d = decision("loop-1", false);
        d.decided_at_ms = 1_700_000_002_000; // stage 3
        repo.record_decision(&d).await.unwrap();
        repo.mark_writeback_committed("loop-1", &d.activity_urn, 1_700_000_003_000) // stage 4
            .await
            .unwrap();

        let row = repo
            .loop_trace_for("loop-1")
            .await
            .unwrap()
            .expect("present");
        assert_eq!(row.decision_outcome.as_deref(), Some("approve"));
        assert_eq!(row.decided_at_ms, Some(1_700_000_002_000));
        assert_eq!(row.writeback_committed, Some(true));
        assert_eq!(row.writeback_committed_at_ms, Some(1_700_000_003_000));
        // Propose ≤ queued ≤ decided ≤ merged (monotonic, proven end to end).
        let propose = row
            .proposal_json
            .get("proposed_at_ms")
            .and_then(|v| v.as_i64())
            .unwrap();
        let queued = row
            .proposal_json
            .get("queued_at_ms")
            .and_then(|v| v.as_i64())
            .unwrap();
        assert!(propose <= queued);
        assert!(queued <= row.decided_at_ms.unwrap());
        assert!(row.decided_at_ms.unwrap() <= row.writeback_committed_at_ms.unwrap());
    }

    #[tokio::test]
    async fn loop_trace_for_pending_proposal_has_no_decision() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("loop-pending"))
            .await
            .unwrap();
        let row = repo
            .loop_trace_for("loop-pending")
            .await
            .unwrap()
            .expect("present");
        assert_eq!(row.status, "pending");
        assert!(row.decided_at_ms.is_none());
        assert!(row.writeback_committed_at_ms.is_none());
    }

    #[tokio::test]
    async fn provenance_decisions_since_carries_owner_did_and_activity() {
        // REC-11: the trace-facing decision read carries the did:nostr attribution
        // and PROV-O activity URN the unified trace joins on.
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("prov-1")).await.unwrap();
        let mut d = decision("prov-1", true);
        d.owner_did = Some("did:nostr:aaaa".into());
        d.decided_at_ms = 2_000;
        repo.record_decision(&d).await.unwrap();

        let rows = repo.provenance_decisions_since(1_000).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].owner_did.as_deref(), Some("did:nostr:aaaa"));
        assert!(rows[0]
            .activity_urn
            .starts_with("urn:visionclaw:execution:"));
        assert!(rows[0].attributed);
        // A cutoff after the decision returns nothing.
        assert!(repo
            .provenance_decisions_since(3_000)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn decisions_since_windows_on_decided_at() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-w1")).await.unwrap();
        repo.create_or_update(&proposal("case-w2")).await.unwrap();
        // Two decisions inside the window, one before the cutoff.
        let mut d1 = decision("case-w1", true);
        d1.decided_at_ms = 2_000;
        d1.activity_urn = "urn:visionclaw:execution:sha256-12-aaaaaaaaaaaa".into();
        let mut d2 = decision("case-w2", true);
        d2.outcome = "reject".into();
        d2.decided_at_ms = 3_000;
        d2.activity_urn = "urn:visionclaw:execution:sha256-12-bbbbbbbbbbbb".into();
        let mut d0 = decision("case-w1", true);
        d0.decided_at_ms = 500;
        d0.activity_urn = "urn:visionclaw:execution:sha256-12-cccccccccccc".into();
        repo.record_decision(&d0).await.unwrap();
        repo.record_decision(&d1).await.unwrap();
        repo.record_decision(&d2).await.unwrap();

        let windowed = repo.decisions_since(1_000).await.unwrap();
        assert_eq!(
            windowed.len(),
            2,
            "only decisions at/after cutoff are returned"
        );
        // Newest first.
        assert_eq!(windowed[0].0, "reject");
        assert_eq!(windowed[0].2, 3_000);
        assert!(windowed.iter().all(|(_, _, ts)| *ts >= 1_000));
    }

    #[tokio::test]
    async fn set_status_marks_terminal_elevated_and_abandoned() {
        // GOV-2: after a decision, the merge poll flips the row to its terminal
        // lifecycle status directly (bypassing the outcome→status derivation).
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("term-1")).await.unwrap();
        repo.record_decision(&decision("term-1", false))
            .await
            .unwrap();
        assert_eq!(
            repo.get("term-1").await.unwrap().unwrap().status,
            "approved"
        );

        repo.set_status("term-1", "elevated").await.unwrap();
        assert_eq!(
            repo.get("term-1").await.unwrap().unwrap().status,
            "elevated"
        );

        repo.create_or_update(&proposal("term-2")).await.unwrap();
        repo.set_status("term-2", "abandoned").await.unwrap();
        assert_eq!(
            repo.get("term-2").await.unwrap().unwrap().status,
            "abandoned"
        );

        // Unknown case id ⇒ no-op, not an error.
        repo.set_status("no-such-case", "elevated").await.unwrap();
    }

    #[test]
    fn status_mapping_matches_outcomes() {
        assert_eq!(status_for_outcome("approve"), "approved");
        assert_eq!(status_for_outcome("Accepted"), "approved");
        assert_eq!(status_for_outcome("reject"), "rejected");
        assert_eq!(status_for_outcome("rejected"), "rejected");
        assert_eq!(status_for_outcome("amend"), "reviewed");
    }

    // ---- ADR-2006: signed-event correlation, duplicates, lag, restart ------

    /// The signed event id survives the round trip, so a stored decision can be
    /// traced back to the request that produced it.
    #[tokio::test]
    async fn signed_event_id_round_trips() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-sig")).await.unwrap();
        repo.record_decision(&signed_decision("case-sig", "e".repeat(64).as_str()))
            .await
            .unwrap();

        let rows = repo.decisions_for("case-sig").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].decision_event_id.as_deref(),
            Some("e".repeat(64).as_str())
        );
        assert_eq!(rows[0].decision_created_at_s, Some(1_700_000_000));
    }

    /// The reproduced hazard: the relay re-delivers a decision after a
    /// reconnect. It must be recorded once, and the second call must return the
    /// same row id rather than a new one.
    #[tokio::test]
    async fn a_redelivered_signed_decision_is_recorded_once() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-dup")).await.unwrap();
        let d = signed_decision("case-dup", "f".repeat(64).as_str());

        let first = repo.record_decision(&d).await.unwrap();
        let second = repo.record_decision(&d).await.unwrap();
        assert_eq!(first, second, "redelivery must return the existing row");
        assert_eq!(
            repo.decisions_for("case-dup").await.unwrap().len(),
            1,
            "a re-delivered decision must not create a second audit row"
        );
    }

    /// Two genuinely distinct decisions on the same case, with the same action
    /// and responder, are both recorded — they differ only by event id, which
    /// is exactly what the old tuple-based correlation could not see.
    #[tokio::test]
    async fn distinct_signed_decisions_are_both_recorded() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-two")).await.unwrap();
        repo.record_decision(&signed_decision("case-two", "a".repeat(64).as_str()))
            .await
            .unwrap();
        repo.record_decision(&signed_decision("case-two", "b".repeat(64).as_str()))
            .await
            .unwrap();

        let rows = repo.decisions_for("case-two").await.unwrap();
        assert_eq!(rows.len(), 2);
    }

    /// Locally minted decisions carry no event id, so the partial unique index
    /// must not constrain them against each other.
    #[tokio::test]
    async fn local_decisions_without_an_event_id_are_not_deduplicated() {
        let repo = temp_repo().await;
        repo.create_or_update(&proposal("case-local"))
            .await
            .unwrap();
        repo.record_decision(&decision("case-local", false))
            .await
            .unwrap();
        repo.record_decision(&decision("case-local", false))
            .await
            .unwrap();

        assert_eq!(
            repo.decisions_for("case-local").await.unwrap().len(),
            2,
            "NULL event ids are not duplicates of one another"
        );
    }

    /// Restart: a store reopened from the same file still recognises a
    /// previously recorded signed decision, so a subscription that replays from
    /// "now" cannot double-record across a process boundary.
    #[tokio::test]
    async fn duplicate_suppression_survives_a_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "adr2006-reopen-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("enrichment.sqlite3");

        let d = signed_decision("case-restart", "c".repeat(64).as_str());
        let first_id = {
            let repo = SqliteEnrichmentRepository::open(&path).await.unwrap();
            repo.create_or_update(&proposal("case-restart"))
                .await
                .unwrap();
            repo.record_decision(&d).await.unwrap()
        };

        // Reopen — a fresh process seeing the same replayed event.
        let repo = SqliteEnrichmentRepository::open(&path).await.unwrap();
        let second_id = repo.record_decision(&d).await.unwrap();
        assert_eq!(first_id, second_id, "the reopened store must recognise it");
        assert_eq!(repo.decisions_for("case-restart").await.unwrap().len(), 1);

        drop(repo);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A store created before ADR-2006 gains the column and the index on open,
    /// and then deduplicates correctly.
    #[tokio::test]
    async fn a_pre_adr_2006_store_is_migrated_on_open() {
        let dir = std::env::temp_dir().join(format!(
            "adr2006-migrate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("enrichment.sqlite3");

        // Build a store, then drop the ADR-2006 column to simulate the old
        // schema. SQLite supports DROP COLUMN from 3.35.
        {
            let repo = SqliteEnrichmentRepository::open(&path).await.unwrap();
            repo.conn
                .call(|c| {
                    c.execute_batch(
                        "DROP INDEX IF EXISTS enrichment_decision_event_idx;
                         ALTER TABLE enrichment_decisions DROP COLUMN decision_created_at_s;
                         ALTER TABLE enrichment_decisions DROP COLUMN decision_event_id;",
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
        }

        // Reopening applies the additive migration.
        let repo = SqliteEnrichmentRepository::open(&path).await.unwrap();
        repo.create_or_update(&proposal("case-mig")).await.unwrap();
        let d = signed_decision("case-mig", "d".repeat(64).as_str());
        let a = repo.record_decision(&d).await.unwrap();
        let b = repo.record_decision(&d).await.unwrap();
        assert_eq!(a, b, "the migrated store must deduplicate");
        assert_eq!(repo.decisions_for("case-mig").await.unwrap().len(), 1);

        drop(repo);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
