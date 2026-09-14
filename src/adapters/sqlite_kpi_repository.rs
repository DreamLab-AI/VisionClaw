// src/adapters/sqlite_kpi_repository.rs
//! SQLite KPI Repository Adapter (REC-4, ADR-043 resurrection, ADR-130 Decision 5).
//!
//! ADR-043 specified four organisational KPIs with a Neo4j
//! `OrganisationalMetricSnapshot` and `DERIVED_FROM` lineage edges. No Neo4j runs
//! in this stack (the graph store is Oxigraph + SQLite), so ADR-130 Decision 5
//! re-targets the snapshot store to a SQLite metrics table analogous to
//! [`crate::adapters::sqlite_enrichment_repository`] and [`crate::adapters::sqlite_canary_repository`].
//!
//! Three tables:
//!   * `kpi_agent_events` — the agent-action volume source. A passive tap on the
//!     existing `/wss/agent-events` hub (`crate::agent_events::hub::subscribe`)
//!     records one lightweight row per envelope. No change to the emit site and
//!     no new fields on the wire — the volume is read from an existing seam
//!     (ADR-130 D5 "without new instrumentation").
//!   * `kpi_snapshots` — a point-in-time KPI value with its confidence and
//!     numerator/denominator, computed from real source events and persisted so
//!     the dashboard reads a stored value (ADR-043's Option 3 rejection stands:
//!     no live re-aggregation on the read path).
//!   * `kpi_lineage` — `DERIVED_FROM` re-targeted onto SQLite: each snapshot's
//!     contributing source events, so a KPI value is traceable back to the
//!     decisions and volume that produced it (WP-8 AC3).
//!
//! ## Persistence choice
//!
//! `tokio-rusqlite`, mirroring the enrichment + canary adapters verbatim: same
//! worker-thread `call` serialisation, same self-bootstrapping schema, same
//! `Arc<Connection>` sharing. Lives in its OWN file (`data/kpi.sqlite3`) so its
//! single-writer constraint stays isolated.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_rusqlite::Connection;

/// Embedded canonical schema. Self-bootstrapping (single-binary deployments,
/// ADR-11 §D1), mirroring the enrichment/canary repositories' posture.
pub const CREATE_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store   = MEMORY;

CREATE TABLE IF NOT EXISTS schema_migrations (
    id          TEXT PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS kpi_agent_events (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id         INTEGER NOT NULL,
    source_agent_id  INTEGER NOT NULL,
    action_type      INTEGER NOT NULL,
    observed_at_ms   INTEGER NOT NULL,
    -- REC-11 (PRD-023 WP-12, data-moat consolidation): the identity + CTC
    -- attribution the unified provenance trace joins on. Nullable + additive so
    -- the KPI volume count is untouched (it counts rows, not these columns) and a
    -- pre-REC-11 store gains them via `apply_additive_migrations`. This is the
    -- durable projection of the /wss/agent-events wire — the "agent-events /
    -- hook-trajectory" source (agentbox emits the CTC fields since P1); the trace
    -- is a read-time JOIN over it, not a new store (ADR-130).
    agent_did        TEXT,
    action_type_name TEXT,
    source_urn       TEXT,
    target_urn       TEXT,
    handoff_id       TEXT,
    token_count      INTEGER,
    verification     TEXT,
    -- FR5.1 (EXP-AC-005, D7 intent legibility): the agent's DECLARED intent,
    -- persisted verbatim from the `/wss/agent-events` envelope. NULL when the
    -- agent declared none — the column is NEVER synthesised from
    -- `action_type_name` after the fact (that would fabricate the very claim the
    -- intent_match verdict is supposed to test). Additive + nullable, so a store
    -- predating this gains it via `apply_additive_migrations`.
    intent           TEXT
);

CREATE INDEX IF NOT EXISTS kpi_agent_events_time_idx
    ON kpi_agent_events(observed_at_ms);

CREATE INDEX IF NOT EXISTS kpi_agent_events_agent_idx
    ON kpi_agent_events(agent_did, observed_at_ms);

CREATE TABLE IF NOT EXISTS kpi_snapshots (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    kpi              TEXT    NOT NULL,
    value            REAL    NOT NULL,
    confidence       REAL    NOT NULL,
    numerator        REAL,
    denominator      REAL,
    sample_count     INTEGER NOT NULL,
    window_start_ms  INTEGER NOT NULL,
    window_end_ms    INTEGER NOT NULL,
    computed_at_ms   INTEGER NOT NULL,
    sha              TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS kpi_snapshots_kpi_idx
    ON kpi_snapshots(kpi, computed_at_ms);

CREATE TABLE IF NOT EXISTS kpi_lineage (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id   INTEGER NOT NULL,
    source_kind   TEXT    NOT NULL,
    source_ref    TEXT    NOT NULL,
    contribution  REAL,
    FOREIGN KEY(snapshot_id) REFERENCES kpi_snapshots(id)
);

CREATE INDEX IF NOT EXISTS kpi_lineage_snapshot_idx
    ON kpi_lineage(snapshot_id);

INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0005_kpi_snapshots');
INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0006_kpi_agent_event_intent');
"#;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum KpiStoreError {
    #[error("db: {0}")]
    Database(String),
}

pub type Result<T> = std::result::Result<T, KpiStoreError>;

fn map_db_err(e: tokio_rusqlite::Error) -> KpiStoreError {
    KpiStoreError::Database(e.to_string())
}

/// Idempotent additive migrations for a store created before the REC-11 identity
/// + CTC columns existed. `CREATE_SCHEMA`'s `IF NOT EXISTS` cannot alter an
/// existing table, so a pre-REC-11 `kpi.sqlite3` needs them added explicitly.
/// Guarded by a `PRAGMA table_info` check so it is a no-op once applied.
fn apply_additive_migrations(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    for (col, decl) in [
        ("agent_did", "TEXT"),
        ("action_type_name", "TEXT"),
        ("source_urn", "TEXT"),
        ("target_urn", "TEXT"),
        ("handoff_id", "TEXT"),
        ("token_count", "INTEGER"),
        ("verification", "TEXT"),
        // FR5.1 — 0006_kpi_agent_event_intent.
        ("intent", "TEXT"),
    ] {
        add_column_if_missing(c, "kpi_agent_events", col, decl)?;
    }
    Ok(())
}

/// Add `column` to `table` iff not already present (SQLite `ADD COLUMN` has no
/// `IF NOT EXISTS`).
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

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// A persisted KPI snapshot row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KpiSnapshotRow {
    pub id: i64,
    pub kpi: String,
    pub value: f64,
    pub confidence: f64,
    pub numerator: Option<f64>,
    pub denominator: Option<f64>,
    pub sample_count: i64,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    pub computed_at_ms: i64,
    pub sha: String,
}

/// The fields needed to persist a new snapshot (id is assigned by SQLite).
#[derive(Debug, Clone)]
pub struct NewKpiSnapshot {
    pub kpi: String,
    pub value: f64,
    pub confidence: f64,
    pub numerator: Option<f64>,
    pub denominator: Option<f64>,
    pub sample_count: i64,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    pub computed_at_ms: i64,
    pub sha: String,
}

/// A `DERIVED_FROM` lineage row linking a snapshot to a contributing source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KpiLineageRow {
    pub snapshot_id: i64,
    pub source_kind: String,
    pub source_ref: String,
    pub contribution: Option<f64>,
}

/// The identity + CTC attributes of one observed agent-event, captured at the
/// hub tap for the REC-11 unified provenance trace. All attribution fields are
/// optional (the wire keeps identity optional for render compatibility).
#[derive(Debug, Clone)]
pub struct NewAgentTrajectory {
    pub event_id: u64,
    pub source_agent_id: u32,
    pub action_type: u8,
    pub action_type_name: Option<String>,
    /// `did:nostr:<pubkey>` derived from the envelope's `pubkey` / `source_urn`.
    pub agent_did: Option<String>,
    pub source_urn: Option<String>,
    pub target_urn: Option<String>,
    /// CTC handoff-chain correlation URN (REC-3), the trace's activity join key.
    pub handoff_id: Option<String>,
    pub token_count: Option<u64>,
    pub verification: Option<String>,
    /// FR5.1 — the envelope's declared `intent`, persisted VERBATIM. `None`
    /// stays NULL; the tap never synthesises one.
    pub intent: Option<String>,
    pub observed_at_ms: i64,
}

/// A persisted agent-event trajectory row read back for the provenance trace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentTrajectoryRow {
    pub event_id: i64,
    pub source_agent_id: i64,
    pub action_type: i64,
    pub action_type_name: Option<String>,
    pub agent_did: Option<String>,
    pub source_urn: Option<String>,
    pub target_urn: Option<String>,
    pub handoff_id: Option<String>,
    pub token_count: Option<i64>,
    pub verification: Option<String>,
    /// The declared intent as persisted, or `None` when the agent declared none.
    pub intent: Option<String>,
    pub observed_at_ms: i64,
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

/// SQLite-backed KPI snapshot + lineage store, plus the agent-event volume tap.
/// Holds one `tokio-rusqlite` connection in `Arc` so it can be cheaply cloned
/// into the compute service, the hub tap task, and the handler.
pub struct SqliteKpiRepository {
    conn: Arc<Connection>,
}

impl SqliteKpiRepository {
    /// Open (or create) the KPI DB at `db_path` and apply [`CREATE_SCHEMA`].
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

    // -- agent-action volume (the Augmentation Ratio numerator source) --------

    /// Record one observed agent-event (the passive hub tap). Cheap append; the
    /// window count is computed on demand at KPI-compute time.
    pub async fn record_agent_event(
        &self,
        event_id: u64,
        source_agent_id: u32,
        action_type: u8,
        observed_at_ms: i64,
    ) -> Result<()> {
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT INTO kpi_agent_events
                         (event_id, source_agent_id, action_type, observed_at_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                )?;
                stmt.execute(rusqlite::params![
                    event_id as i64,
                    source_agent_id as i64,
                    action_type as i64,
                    observed_at_ms,
                ])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Record one observed agent-event WITH its identity + CTC attribution (the
    /// REC-11 trajectory capture). Superset of [`record_agent_event`]: the KPI
    /// volume count still counts the row, and the unified provenance trace reads
    /// the attribution columns. Used by the hub tap so both REC-4 volume and
    /// REC-11 trace read one durable capture of the `/wss/agent-events` wire.
    pub async fn record_agent_trajectory(&self, t: &NewAgentTrajectory) -> Result<()> {
        let t = t.clone();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "INSERT INTO kpi_agent_events
                         (event_id, source_agent_id, action_type, observed_at_ms,
                          agent_did, action_type_name, source_urn, target_urn,
                          handoff_id, token_count, verification, intent)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                )?;
                stmt.execute(rusqlite::params![
                    t.event_id as i64,
                    t.source_agent_id as i64,
                    t.action_type as i64,
                    t.observed_at_ms,
                    &t.agent_did,
                    &t.action_type_name,
                    &t.source_urn,
                    &t.target_urn,
                    &t.handoff_id,
                    t.token_count.map(|n| n as i64),
                    &t.verification,
                    &t.intent,
                ])?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// Agent-event trajectories observed at or after `cutoff_ms`, newest first,
    /// for the unified provenance trace (REC-11). Returns every row (identity may
    /// be `None` for an anonymous frame); the trace join filters on `agent_did`.
    pub async fn trajectories_since(&self, cutoff_ms: i64) -> Result<Vec<AgentTrajectoryRow>> {
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT event_id, source_agent_id, action_type, action_type_name,
                            agent_did, source_urn, target_urn, handoff_id,
                            token_count, verification, intent, observed_at_ms
                     FROM kpi_agent_events
                     WHERE observed_at_ms >= ?1
                     ORDER BY observed_at_ms DESC, id DESC",
                )?;
                let mut q = stmt.query(rusqlite::params![cutoff_ms])?;
                let mut out = Vec::new();
                while let Some(r) = q.next()? {
                    out.push(AgentTrajectoryRow {
                        event_id: r.get(0)?,
                        source_agent_id: r.get(1)?,
                        action_type: r.get(2)?,
                        action_type_name: r.get(3)?,
                        agent_did: r.get(4)?,
                        source_urn: r.get(5)?,
                        target_urn: r.get(6)?,
                        handoff_id: r.get(7)?,
                        token_count: r.get(8)?,
                        verification: r.get(9)?,
                        intent: r.get(10)?,
                        observed_at_ms: r.get(11)?,
                    });
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)
    }

    /// Count agent-events observed at or after `cutoff_ms` (the rolling window).
    pub async fn count_agent_events_since(&self, cutoff_ms: i64) -> Result<i64> {
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT COUNT(*) FROM kpi_agent_events WHERE observed_at_ms >= ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![cutoff_ms])?;
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

    // -- snapshots + lineage --------------------------------------------------

    /// Persist a snapshot and its lineage rows in one transaction, returning the
    /// snapshot id. The lineage is written with the snapshot so a value is never
    /// stored without its `DERIVED_FROM` trail (WP-8 AC3).
    pub async fn insert_snapshot_with_lineage(
        &self,
        snap: &NewKpiSnapshot,
        lineage: &[(String, String, Option<f64>)],
    ) -> Result<i64> {
        let snap = snap.clone();
        let lineage: Vec<(String, String, Option<f64>)> = lineage.to_vec();
        self.conn
            .call(move |c| {
                let tx = c.transaction()?;
                let snapshot_id: i64;
                {
                    let mut ins = tx.prepare_cached(
                        "INSERT INTO kpi_snapshots
                             (kpi, value, confidence, numerator, denominator,
                              sample_count, window_start_ms, window_end_ms,
                              computed_at_ms, sha)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    )?;
                    ins.execute(rusqlite::params![
                        &snap.kpi,
                        snap.value,
                        snap.confidence,
                        snap.numerator,
                        snap.denominator,
                        snap.sample_count,
                        snap.window_start_ms,
                        snap.window_end_ms,
                        snap.computed_at_ms,
                        &snap.sha,
                    ])?;
                    snapshot_id = tx.last_insert_rowid();

                    let mut lin = tx.prepare_cached(
                        "INSERT INTO kpi_lineage
                             (snapshot_id, source_kind, source_ref, contribution)
                         VALUES (?1, ?2, ?3, ?4)",
                    )?;
                    for (kind, reference, contribution) in &lineage {
                        lin.execute(rusqlite::params![
                            snapshot_id,
                            kind,
                            reference,
                            contribution,
                        ])?;
                    }
                }
                tx.commit()?;
                Ok(snapshot_id)
            })
            .await
            .map_err(map_db_err)
    }

    /// The most recent snapshot for a KPI, if any.
    pub async fn latest_snapshot(&self, kpi: &str) -> Result<Option<KpiSnapshotRow>> {
        let kpi = kpi.to_string();
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT id, kpi, value, confidence, numerator, denominator,
                            sample_count, window_start_ms, window_end_ms,
                            computed_at_ms, sha
                     FROM kpi_snapshots WHERE kpi = ?1
                     ORDER BY computed_at_ms DESC, id DESC LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![&kpi])?;
                if let Some(r) = rows.next()? {
                    Ok(Some(row_to_snapshot(r)?))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(map_db_err)
    }

    /// All lineage rows for a snapshot (the queryable `DERIVED_FROM` trail).
    pub async fn lineage_for(&self, snapshot_id: i64) -> Result<Vec<KpiLineageRow>> {
        self.conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT snapshot_id, source_kind, source_ref, contribution
                     FROM kpi_lineage WHERE snapshot_id = ?1
                     ORDER BY id ASC",
                )?;
                let mut q = stmt.query(rusqlite::params![snapshot_id])?;
                let mut out = Vec::new();
                while let Some(r) = q.next()? {
                    out.push(KpiLineageRow {
                        snapshot_id: r.get(0)?,
                        source_kind: r.get(1)?,
                        source_ref: r.get(2)?,
                        contribution: r.get(3)?,
                    });
                }
                Ok(out)
            })
            .await
            .map_err(map_db_err)
    }

    /// Count persisted snapshots for a KPI (test/introspection helper).
    pub async fn snapshot_count(&self, kpi: &str) -> Result<i64> {
        let kpi = kpi.to_string();
        self.conn
            .call(move |c| {
                let mut stmt =
                    c.prepare_cached("SELECT COUNT(*) FROM kpi_snapshots WHERE kpi = ?1")?;
                let mut rows = stmt.query(rusqlite::params![&kpi])?;
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
}

fn row_to_snapshot(r: &rusqlite::Row<'_>) -> rusqlite::Result<KpiSnapshotRow> {
    Ok(KpiSnapshotRow {
        id: r.get(0)?,
        kpi: r.get(1)?,
        value: r.get(2)?,
        confidence: r.get(3)?,
        numerator: r.get(4)?,
        denominator: r.get(5)?,
        sample_count: r.get(6)?,
        window_start_ms: r.get(7)?,
        window_end_ms: r.get(8)?,
        computed_at_ms: r.get(9)?,
        sha: r.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn temp_repo() -> SqliteKpiRepository {
        let dir = std::env::temp_dir().join(format!("kpi-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!(
            "kpi-{}.sqlite3",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        SqliteKpiRepository::open(&path).await.expect("open")
    }

    #[tokio::test]
    async fn agent_event_volume_window_count() {
        let repo = temp_repo().await;
        // Three inside the window, one outside.
        repo.record_agent_event(1, 7, 1, 10_000).await.unwrap();
        repo.record_agent_event(2, 7, 1, 20_000).await.unwrap();
        repo.record_agent_event(3, 8, 2, 30_000).await.unwrap();
        repo.record_agent_event(4, 8, 2, 5_000).await.unwrap();
        assert_eq!(repo.count_agent_events_since(10_000).await.unwrap(), 3);
        assert_eq!(repo.count_agent_events_since(0).await.unwrap(), 4);
    }

    #[tokio::test]
    async fn declared_intent_round_trips_verbatim_and_absence_stays_null() {
        // FR5.1 / EXP-AC-005: the envelope's intent is stored exactly as the
        // agent wrote it, and an envelope with none leaves NULL — never a
        // string derived from `action_type_name`.
        let repo = temp_repo().await;
        let declared = "op=update target=urn:visionclaw:kg:aaaa:node-7";
        repo.record_agent_trajectory(&NewAgentTrajectory {
            event_id: 1,
            source_agent_id: 7,
            action_type: 1,
            action_type_name: Some("graph_update".into()),
            agent_did: Some("did:nostr:aaaa".into()),
            source_urn: None,
            target_urn: Some("urn:visionclaw:kg:aaaa:node-7".into()),
            handoff_id: None,
            token_count: None,
            verification: None,
            intent: Some(declared.into()),
            observed_at_ms: 10_000,
        })
        .await
        .unwrap();
        repo.record_agent_trajectory(&NewAgentTrajectory {
            event_id: 2,
            source_agent_id: 7,
            action_type: 1,
            action_type_name: Some("graph_update".into()),
            agent_did: None,
            source_urn: None,
            target_urn: None,
            handoff_id: None,
            token_count: None,
            verification: None,
            intent: None,
            observed_at_ms: 20_000,
        })
        .await
        .unwrap();

        let rows = repo.trajectories_since(0).await.unwrap();
        assert_eq!(rows.len(), 2);
        let with_intent = rows.iter().find(|r| r.event_id == 1).unwrap();
        assert_eq!(with_intent.intent.as_deref(), Some(declared));
        let without = rows.iter().find(|r| r.event_id == 2).unwrap();
        assert_eq!(without.intent, None, "absence stays absence");
    }

    #[tokio::test]
    async fn snapshot_persists_with_queryable_lineage() {
        let repo = temp_repo().await;
        let snap = NewKpiSnapshot {
            kpi: "augmentation_ratio".into(),
            value: 3.5,
            confidence: 0.4,
            numerator: Some(42.0),
            denominator: Some(12.0),
            sample_count: 54,
            window_start_ms: 0,
            window_end_ms: 100,
            computed_at_ms: 100,
            sha: "abc123".into(),
        };
        let lineage = vec![
            (
                "agent_event_volume".to_string(),
                "window_count".to_string(),
                Some(42.0),
            ),
            (
                "acsp_escalation".to_string(),
                "enrichment_decisions_window_count".to_string(),
                Some(12.0),
            ),
        ];
        let id = repo
            .insert_snapshot_with_lineage(&snap, &lineage)
            .await
            .unwrap();
        assert!(id > 0);

        let latest = repo
            .latest_snapshot("augmentation_ratio")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.value, 3.5);
        assert_eq!(latest.numerator, Some(42.0));

        let trail = repo.lineage_for(id).await.unwrap();
        assert_eq!(trail.len(), 2, "both source contributions are traceable");
        assert!(trail.iter().any(|l| l.source_kind == "agent_event_volume"));
        assert!(trail.iter().any(|l| l.source_kind == "acsp_escalation"));
        assert_eq!(repo.snapshot_count("augmentation_ratio").await.unwrap(), 1);
    }
}
