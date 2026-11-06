// src/adapters/sqlite_share_repository.rs
//! SQLite store for graph shares (`/api/graph-export/share`, `/shared/{id}`).
//!
//! A share used to live in a `HashMap` that each actix worker built for itself
//! inside `configure_routes`, so a share made on one worker was not found on
//! another, and every share was lost on restart while its file stayed in
//! `data/shared/`. One store, opened once in `AppState::new` and handed to every
//! worker, fixes both.
//!
//! ## Persistence choice
//!
//! `tokio-rusqlite`, the same posture as the enrichment, canary and KPI
//! adapters: worker-thread `call` serialisation, a self-bootstrapping schema,
//! `Arc<Connection>` sharing, and its own file (`data/shares.sqlite3`).
//!
//! The record is the serialised [`SharedGraph`]. The columns the handlers
//! decide on (`creator_id`, `expires_at_ms`, `access_count`, `max_access_count`)
//! are real columns, so the access claim and the owner-only delete are single
//! statements and cannot race across workers. `access_count` is authoritative in
//! its column; the copy inside `record` is overwritten on every read.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio_rusqlite::Connection;
use uuid::Uuid;
use visionclaw_domain::models::graph_export::SharedGraph;

/// Embedded schema, applied idempotently on [`SqliteShareRepository::open`].
pub const CREATE_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA temp_store   = MEMORY;

CREATE TABLE IF NOT EXISTS schema_migrations (
    id          TEXT PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS shared_graphs (
    id                TEXT    PRIMARY KEY,
    creator_id        TEXT,
    expires_at_ms     INTEGER,
    access_count      INTEGER NOT NULL DEFAULT 0,
    max_access_count  INTEGER,
    record            TEXT    NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS shared_graphs_expiry_idx ON shared_graphs(expires_at_ms);

INSERT OR IGNORE INTO schema_migrations (id) VALUES ('0001_shared_graphs');
"#;

/// A share-store failure.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ShareStoreError {
    /// The database refused or failed the operation.
    #[error("db: {0}")]
    Database(String),
    /// A stored record did not round-trip through serde.
    #[error("record: {0}")]
    Record(String),
    /// A new share must name its creator.
    #[error("a share must have a creator")]
    NoCreator,
}

/// Result alias for the share store.
pub type Result<T> = std::result::Result<T, ShareStoreError>;

fn map_db_err(e: tokio_rusqlite::Error) -> ShareStoreError {
    ShareStoreError::Database(e.to_string())
}

/// The outcome of [`SqliteShareRepository::claim_access`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessClaim {
    /// The access was counted.
    Granted,
    /// The share has used every access it allows.
    LimitReached,
    /// No such share.
    NotFound,
}

/// Durable, process-wide graph-share store.
#[derive(Clone)]
pub struct SqliteShareRepository {
    conn: Arc<Connection>,
}

impl SqliteShareRepository {
    /// Open (creating when absent) the store at `db_path`.
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

    /// Record a new share. A second insert with the same id replaces the first. A share with no
    /// creator is refused: nobody could ever delete it.
    pub async fn insert(&self, share: &SharedGraph) -> Result<()> {
        if share.creator_id.is_none() {
            return Err(ShareStoreError::NoCreator);
        }
        let record =
            serde_json::to_string(share).map_err(|e| ShareStoreError::Record(e.to_string()))?;
        let id = share.id.to_string();
        let creator = share.creator_id.clone();
        let expires = share.expires_at.map(|t| t.timestamp_millis());
        let count = i64::from(share.access_count);
        let max = share.max_access_count.map(i64::from);
        self.conn
            .call(move |c| {
                c.execute(
                    "INSERT OR REPLACE INTO shared_graphs
                         (id, creator_id, expires_at_ms, access_count, max_access_count, record)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![id, creator, expires, count, max, record],
                )?;
                Ok(())
            })
            .await
            .map_err(map_db_err)
    }

    /// The share with `id`, when one is stored. Expired shares are returned too;
    /// the handler answers them with 410.
    pub async fn get(&self, id: Uuid) -> Result<Option<SharedGraph>> {
        let key = id.to_string();
        let row: Option<(String, i64)> = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare_cached(
                    "SELECT record, access_count FROM shared_graphs WHERE id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![key])?;
                match rows.next()? {
                    Some(r) => Ok(Some((r.get(0)?, r.get(1)?))),
                    None => Ok(None),
                }
            })
            .await
            .map_err(map_db_err)?;
        row.map(|(record, count)| {
            let mut share: SharedGraph = serde_json::from_str(&record)
                .map_err(|e| ShareStoreError::Record(e.to_string()))?;
            share.access_count = u32::try_from(count).unwrap_or(u32::MAX);
            Ok(share)
        })
        .transpose()
    }

    /// Count one access, in one statement, unless the share has used every access
    /// it allows. Two workers claiming the last access cannot both win.
    pub async fn claim_access(&self, id: Uuid) -> Result<AccessClaim> {
        let key = id.to_string();
        self.conn
            .call(move |c| {
                let updated = c.execute(
                    "UPDATE shared_graphs SET access_count = access_count + 1
                     WHERE id = ?1
                       AND (max_access_count IS NULL OR access_count < max_access_count)",
                    rusqlite::params![key],
                )?;
                if updated > 0 {
                    return Ok(AccessClaim::Granted);
                }
                let exists: bool = c
                    .prepare_cached("SELECT 1 FROM shared_graphs WHERE id = ?1")?
                    .exists(rusqlite::params![key])?;
                Ok(if exists {
                    AccessClaim::LimitReached
                } else {
                    AccessClaim::NotFound
                })
            })
            .await
            .map_err(map_db_err)
    }

    /// Delete the share with `id` when `caller` is its recorded creator, returning it.
    /// Anyone else, an anonymous caller, and a share with no recorded creator get
    /// `None`, the same answer as a share that does not exist.
    pub async fn delete_owned(
        &self,
        id: Uuid,
        caller: Option<&str>,
    ) -> Result<Option<SharedGraph>> {
        let Some(caller) = caller else {
            return Ok(None);
        };
        let Some(share) = self.get(id).await? else {
            return Ok(None);
        };
        let key = id.to_string();
        let owner = caller.to_string();
        let deleted = self
            .conn
            .call(move |c| {
                Ok(c.execute(
                    "DELETE FROM shared_graphs WHERE id = ?1 AND creator_id = ?2",
                    rusqlite::params![key, owner],
                )?)
            })
            .await
            .map_err(map_db_err)?;
        Ok((deleted > 0).then_some(share))
    }

    /// Delete a legacy share that has no recorded creator, returning it. A share with a creator
    /// is never deleted here (`None`), so only [`delete_owned`](Self::delete_owned) removes it.
    /// The handler allows this to power users only; without it a creator-less share made
    /// before creators were required could never be deleted.
    pub async fn delete_unowned(&self, id: Uuid) -> Result<Option<SharedGraph>> {
        let Some(share) = self.get(id).await? else {
            return Ok(None);
        };
        let key = id.to_string();
        let deleted = self
            .conn
            .call(move |c| {
                Ok(c.execute(
                    "DELETE FROM shared_graphs WHERE id = ?1 AND creator_id IS NULL",
                    rusqlite::params![key],
                )?)
            })
            .await
            .map_err(map_db_err)?;
        Ok((deleted > 0).then_some(share))
    }

    /// Delete every share that expired before `now` and return their file paths,
    /// so the caller can remove the files too.
    pub async fn remove_expired(&self, now: DateTime<Utc>) -> Result<Vec<String>> {
        let cutoff = now.timestamp_millis();
        let records: Vec<String> = self
            .conn
            .call(move |c| {
                let tx = c.transaction()?;
                let records = {
                    let mut stmt = tx.prepare(
                        "SELECT record FROM shared_graphs
                         WHERE expires_at_ms IS NOT NULL AND expires_at_ms < ?1",
                    )?;
                    let rows = stmt.query_map(rusqlite::params![cutoff], |r| r.get(0))?;
                    rows.collect::<rusqlite::Result<Vec<String>>>()?
                };
                tx.execute(
                    "DELETE FROM shared_graphs
                     WHERE expires_at_ms IS NOT NULL AND expires_at_ms < ?1",
                    rusqlite::params![cutoff],
                )?;
                tx.commit()?;
                Ok(records)
            })
            .await
            .map_err(map_db_err)?;
        Ok(records
            .iter()
            .filter_map(|r| serde_json::from_str::<SharedGraph>(r).ok())
            .map(|s| s.file_path)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::graph_export::{ExportFormat, NewSharedGraph};

    fn share(creator: Option<&str>, max: Option<u32>) -> SharedGraph {
        let mut s = SharedGraph::new(NewSharedGraph {
            title: "t".into(),
            description: None,
            creator_id: creator.map(str::to_string),
            file_path: "/tmp/share.json".into(),
            file_size: 3,
            compressed: true,
            original_format: ExportFormat::Json,
            node_count: 1,
            edge_count: 0,
        });
        s.max_access_count = max;
        s
    }

    /// A row as written before every share had a creator: no `creator_id`. The store no longer
    /// writes one, so the test puts it in directly.
    async fn insert_legacy_row(store: &SqliteShareRepository, s: &SharedGraph) {
        let record = serde_json::to_string(s).unwrap();
        let id = s.id.to_string();
        store
            .conn
            .call(move |c| {
                c.execute(
                    "INSERT INTO shared_graphs
                         (id, creator_id, expires_at_ms, access_count, max_access_count, record)
                     VALUES (?1, NULL, NULL, 0, NULL, ?2)",
                    rusqlite::params![id, record],
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }

    /// VC-19 residual: a share with no creator could never be deleted by anyone. New shares
    /// therefore always carry one.
    #[tokio::test]
    async fn a_share_without_a_creator_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteShareRepository::open(&dir.path().join("s.sqlite3"))
            .await
            .unwrap();
        let s = share(None, None);
        assert!(store.insert(&s).await.is_err());
        assert!(store.get(s.id).await.unwrap().is_none());
    }

    /// A legacy share with no creator may be deleted (by a power user, the handler's rule);
    /// a share with a creator is never deleted this way.
    #[tokio::test]
    async fn only_a_legacy_unowned_share_is_deleted_without_its_creator() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteShareRepository::open(&dir.path().join("s.sqlite3"))
            .await
            .unwrap();
        let owned = share(Some("a"), None);
        let legacy = share(None, None);
        store.insert(&owned).await.unwrap();
        insert_legacy_row(&store, &legacy).await;
        assert!(store.delete_unowned(owned.id).await.unwrap().is_none());
        assert!(store.get(owned.id).await.unwrap().is_some());
        assert!(store.delete_unowned(legacy.id).await.unwrap().is_some());
        assert!(store.get(legacy.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_share_survives_reopening_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shares.sqlite3");
        let s = share(Some("a"), None);
        {
            let store = SqliteShareRepository::open(&path).await.unwrap();
            store.insert(&s).await.unwrap();
        }
        let reopened = SqliteShareRepository::open(&path).await.unwrap();
        let got = reopened
            .get(s.id)
            .await
            .unwrap()
            .expect("share after restart");
        assert_eq!(got.id, s.id);
        assert_eq!(got.creator_id.as_deref(), Some("a"));
        assert_eq!(got.file_path, s.file_path);
    }

    #[tokio::test]
    async fn the_access_limit_is_counted_in_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteShareRepository::open(&dir.path().join("s.sqlite3"))
            .await
            .unwrap();
        let s = share(Some("a"), Some(2));
        store.insert(&s).await.unwrap();
        assert_eq!(
            store.claim_access(s.id).await.unwrap(),
            AccessClaim::Granted
        );
        assert_eq!(
            store.claim_access(s.id).await.unwrap(),
            AccessClaim::Granted
        );
        assert_eq!(
            store.claim_access(s.id).await.unwrap(),
            AccessClaim::LimitReached
        );
        assert_eq!(store.get(s.id).await.unwrap().unwrap().access_count, 2);
        assert_eq!(
            store.claim_access(Uuid::new_v4()).await.unwrap(),
            AccessClaim::NotFound
        );
    }

    #[tokio::test]
    async fn only_the_creator_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteShareRepository::open(&dir.path().join("s.sqlite3"))
            .await
            .unwrap();
        let owned = share(Some("a"), None);
        let orphan = share(None, None);
        store.insert(&owned).await.unwrap();
        insert_legacy_row(&store, &orphan).await;
        assert!(store.delete_owned(owned.id, None).await.unwrap().is_none());
        assert!(store
            .delete_owned(owned.id, Some("b"))
            .await
            .unwrap()
            .is_none());
        assert!(store
            .delete_owned(orphan.id, Some("a"))
            .await
            .unwrap()
            .is_none());
        assert!(store
            .delete_owned(owned.id, Some("a"))
            .await
            .unwrap()
            .is_some());
        assert!(store.get(owned.id).await.unwrap().is_none());
        assert!(store.get(orphan.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn expired_shares_are_removed_with_their_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteShareRepository::open(&dir.path().join("s.sqlite3"))
            .await
            .unwrap();
        let mut old = share(Some("a"), None);
        old.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        let live = share(Some("a"), None);
        store.insert(&old).await.unwrap();
        store.insert(&live).await.unwrap();
        let paths = store.remove_expired(Utc::now()).await.unwrap();
        assert_eq!(paths, vec![old.file_path.clone()]);
        assert!(store.get(old.id).await.unwrap().is_none());
        assert!(store.get(live.id).await.unwrap().is_some());
    }
}
