//! Durable per-store erasure/restore reconciliation, not a distributed transaction.
//! Backends must make `apply(operation_id, subject)` idempotent: a crash can occur
//! after the store commits but before its receipt is recorded. No backend is
//! silently omitted; absent adapters and changed membership fail closed.
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreManifest {
    pub scope: String,
    pub stores: BTreeSet<String>,
}
impl StoreManifest {
    pub fn selected(solid: bool, redis: bool, payments: bool) -> Self {
        let mut stores = BTreeSet::from_iter(
            [
                "sqlite-settings",
                "sqlite-enrichment",
                "sqlite-kpi",
                "sqlite-liveness",
                "sqlite-roles",
                "oxigraph",
                "nostr-session-memory",
                "github-authored-content",
                "external-agent-memory",
                "credential-custody",
            ]
            .into_iter()
            .map(str::to_string),
        );
        if solid {
            stores.insert("solid-pod".into());
        }
        if redis {
            stores.insert("redis-sessions".into());
        }
        if payments {
            stores.insert("payment-ledger-exchange".into());
        }
        Self { scope: "VisionClaw selected persistence surfaces; availability and subject mapping require each registered adapter".into(), stores }
    }
    pub fn from_process() -> Self {
        Self::selected(
            cfg!(feature = "solid-pod-embed"),
            cfg!(feature = "redis") && std::env::var("REDIS_URL").is_ok(),
            cfg!(feature = "solid-pod-embed")
                && std::env::var("PAY_ENABLED").is_ok_and(|v| matches!(v.as_str(), "true" | "1")),
        )
    }
}
pub trait ReconciliationBackend {
    fn store_id(&self) -> &str;
    /// Must persist or enforce the operation ID at the store boundary. Receipt
    /// failure is retried with the SAME ID, never a new destructive operation.
    fn apply(&mut self, operation_id: &str, subject: &str, action: &str) -> Result<String, String>;
}
pub struct ReconciliationJournal {
    db: Connection,
}
impl ReconciliationJournal {
    pub fn open(path: &Path) -> Result<Self, String> {
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,subject TEXT NOT NULL,action TEXT NOT NULL,manifest TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS store_receipts(operation_id TEXT NOT NULL,store_id TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('pending','failed','complete')),receipt TEXT,error TEXT,attempts INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(operation_id,store_id));") .map_err(|e|e.to_string())?;
        Ok(Self { db })
    }
    pub fn begin(
        &mut self,
        id: &str,
        subject: &str,
        action: &str,
        manifest: &StoreManifest,
    ) -> Result<(), String> {
        if id.trim().is_empty()
            || subject.trim().is_empty()
            || !matches!(action, "erase" | "restore")
            || manifest.stores.is_empty()
        {
            return Err(
                "explicit operation ID, subject, action and store membership required".into(),
            );
        }
        let encoded = serde_json::to_string(manifest).map_err(|e| e.to_string())?;
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT OR IGNORE INTO operations VALUES(?1,?2,?3,?4)",
            params![id, subject, action, encoded],
        )
        .map_err(|e| e.to_string())?;
        let existing: (String, String, String) = tx
            .query_row(
                "SELECT subject,action,manifest FROM operations WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if existing != (subject.into(), action.into(), encoded) {
            return Err("operation ID already binds a different subject/action/manifest".into());
        }
        for store in &manifest.stores {
            tx.execute("INSERT OR IGNORE INTO store_receipts(operation_id,store_id,status) VALUES(?1,?2,'pending')",params![id,store]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn resume(
        &mut self,
        id: &str,
        current: &StoreManifest,
        backends: &mut [&mut dyn ReconciliationBackend],
    ) -> Result<bool, String> {
        let (subject, action, encoded): (String, String, String) = self
            .db
            .query_row(
                "SELECT subject,action,manifest FROM operations WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        let stored: StoreManifest = serde_json::from_str(&encoded).map_err(|e| e.to_string())?;
        if &stored != current {
            return Err(
                "selected store membership changed; explicit reconciliation required".into(),
            );
        }
        let registered: BTreeSet<String> =
            backends.iter().map(|b| b.store_id().to_string()).collect();
        if registered != current.stores || registered.len() != backends.len() {
            return Err(
                "exactly one adapter is required for every selected store; none may be omitted"
                    .into(),
            );
        }
        for backend in backends {
            let store = backend.store_id().to_string();
            let status: String = self
                .db
                .query_row(
                    "SELECT status FROM store_receipts WHERE operation_id=?1 AND store_id=?2",
                    params![id, store],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if status == "complete" {
                continue;
            }
            let result = backend.apply(id, &subject, &action);
            let (status, receipt, error) = match result {
                Ok(receipt) if !receipt.is_empty() => ("complete", Some(receipt), None),
                Ok(_) => ("failed", None, Some("empty applied receipt".to_string())),
                Err(e) => ("failed", None, Some(e)),
            };
            let changed=self.db.execute("UPDATE store_receipts SET status=?3,receipt=?4,error=?5,attempts=attempts+1 WHERE operation_id=?1 AND store_id=?2 AND status!='complete'",params![id,store,status,receipt,error]).map_err(|e|e.to_string())?;
            if changed != 1 {
                return Err("receipt transition did not affect exactly one pending member".into());
            }
        }
        let remaining: i64 = self
            .db
            .query_row(
                "SELECT COUNT(*) FROM store_receipts WHERE operation_id=?1 AND status!='complete'",
                [id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(remaining == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        id: &'static str,
        fail: bool,
        calls: usize,
        operations: BTreeSet<String>,
    }
    impl ReconciliationBackend for Fake {
        fn store_id(&self) -> &str {
            self.id
        }
        fn apply(&mut self, id: &str, _: &str, _: &str) -> Result<String, String> {
            self.calls += 1;
            if self.fail {
                return Err("injected store outage".into());
            }
            self.operations.insert(id.into());
            Ok(format!("applied:{id}"))
        }
    }
    #[test]
    fn restart_retries_failed_member_without_repeating_completed_member() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.sqlite3");
        let manifest = StoreManifest {
            scope: "test".into(),
            stores: BTreeSet::from(["a".into(), "b".into()]),
        };
        let mut a = Fake {
            id: "a",
            fail: false,
            calls: 0,
            operations: BTreeSet::new(),
        };
        let mut b = Fake {
            id: "b",
            fail: true,
            calls: 0,
            operations: BTreeSet::new(),
        };
        let mut journal = ReconciliationJournal::open(&path).unwrap();
        journal
            .begin("op1", "subject1", "erase", &manifest)
            .unwrap();
        assert!(!journal
            .resume("op1", &manifest, &mut [&mut a, &mut b])
            .unwrap());
        drop(journal);
        b.fail = false;
        let mut restarted = ReconciliationJournal::open(&path).unwrap();
        assert!(restarted
            .resume("op1", &manifest, &mut [&mut a, &mut b])
            .unwrap());
        assert_eq!(a.calls, 1);
        assert_eq!(b.calls, 2);
        assert!(restarted
            .begin("op1", "other-subject", "erase", &manifest)
            .is_err());
    }
    #[test]
    fn applied_then_receipt_failure_retries_the_same_idempotency_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = ReconciliationJournal::open(&dir.path().join("j.db")).unwrap();
        let manifest = StoreManifest {
            scope: "test".into(),
            stores: BTreeSet::from(["a".into()]),
        };
        let mut a = Fake {
            id: "a",
            fail: false,
            calls: 0,
            operations: BTreeSet::new(),
        };
        journal
            .begin("operation", "subject", "erase", &manifest)
            .unwrap();
        journal.db.execute_batch("CREATE TRIGGER receipt_failure BEFORE UPDATE ON store_receipts BEGIN SELECT RAISE(ABORT,'injected receipt failure'); END;").unwrap();
        assert!(journal
            .resume("operation", &manifest, &mut [&mut a])
            .is_err());
        journal
            .db
            .execute_batch("DROP TRIGGER receipt_failure")
            .unwrap();
        assert!(journal
            .resume("operation", &manifest, &mut [&mut a])
            .unwrap());
        assert_eq!(a.calls, 2);
        assert_eq!(a.operations.len(), 1);
    }

    #[test]
    fn missing_adapter_or_changed_optional_store_cannot_report_success() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = ReconciliationJournal::open(&dir.path().join("j.db")).unwrap();
        let selected = StoreManifest::selected(true, true, false);
        journal
            .begin("op", "subject", "restore", &selected)
            .unwrap();
        assert!(journal.resume("op", &selected, &mut []).is_err());
        assert!(journal
            .resume("op", &StoreManifest::selected(true, false, false), &mut [])
            .is_err());
        assert!(selected.stores.contains("redis-sessions"));
        assert!(!StoreManifest::selected(true, false, false)
            .stores
            .contains("redis-sessions"));
    }
}

/// Consistent RocksDB checkpoint from the OPEN writer, not a filesystem copy.
/// The destination must be outside the source data tree. A checkpoint on the
/// same filesystem may share immutable SST hardlinks; off-device replication is
/// still an operations requirement before claiming independent disaster recovery.
pub fn checkpoint_oxigraph(
    store: &oxigraph::store::Store,
    data_root: &Path,
    backup_root: &Path,
) -> Result<std::path::PathBuf, String> {
    let source = data_root.canonicalize().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(backup_root).map_err(|e| e.to_string())?;
    let destination = backup_root.canonicalize().map_err(|e| e.to_string())?;
    if destination.starts_with(&source) {
        return Err("ontology checkpoint destination must be outside the source data tree".into());
    }
    let target = destination.join(format!("ontology-{}", uuid::Uuid::new_v4()));
    store.backup(&target).map_err(|e| e.to_string())?;
    Ok(target)
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    use oxigraph::{
        model::{GraphName, NamedNode, Quad},
        store::Store,
    };
    #[test]
    fn open_writer_checkpoint_restores_authored_provenance_after_source_loss() {
        let fixture = tempfile::tempdir().unwrap();
        let data = fixture.path().join("data");
        std::fs::create_dir(&data).unwrap();
        let store = Store::open(data.join("oxigraph")).unwrap();
        let q = Quad::new(
            NamedNode::new("urn:activity:one").unwrap(),
            NamedNode::new("http://www.w3.org/ns/prov#used").unwrap(),
            NamedNode::new("urn:entity:one").unwrap(),
            GraphName::NamedNode(NamedNode::new("urn:graph:provenance").unwrap()),
        );
        store.insert(&q).unwrap();
        assert!(checkpoint_oxigraph(&store, &data, &data.join("bad-backup")).is_err());
        let target = checkpoint_oxigraph(&store, &data, &fixture.path().join("backups")).unwrap();
        store.remove(&q).unwrap();
        drop(store);
        std::fs::remove_dir_all(&data).unwrap();
        let restored = Store::open(target).unwrap();
        assert!(restored.contains(&q).unwrap());
    }
}
