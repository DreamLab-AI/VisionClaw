//! Periodic Oxigraph checkpoint task (ADR-2103).
//!
//! Oxigraph 0.4 gives one process read-write access to the store, so only the
//! server can copy it while it runs. This task asks RocksDB, through
//! `Store::backup`, for a point-in-time checkpoint every
//! `ONTOLOGY_CHECKPOINT_INTERVAL_SECS` and leaves it where the
//! `visionclaw-backup` tool picks it up. The first run happens as soon as the
//! task starts, which replaces the old one-shot boot checkpoint.
//!
//! The work is blocking file and RocksDB I/O, so each run goes through
//! `spawn_blocking`. A failure is logged and the next interval tries again; it
//! never stops the server. Shutdown waits for a run in progress to finish.

use crate::services::data_reconciliation::checkpoint_oxigraph;
use log::{info, warn};
use oxigraph::store::Store;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Default gap between checkpoints: six hours.
const DEFAULT_INTERVAL_SECS: u64 = 6 * 3600;

/// Settings for the checkpoint task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointConfig {
    /// Where checkpoints are written; `None` leaves the task off.
    pub dir: Option<PathBuf>,
    /// Gap between checkpoints; `None` (interval `0`) leaves the task off.
    pub interval: Option<Duration>,
}

impl CheckpointConfig {
    /// Reads `ONTOLOGY_CHECKPOINT_DIR` (falling back to the older
    /// `ONTOLOGY_BACKUP_DIR`) and `ONTOLOGY_CHECKPOINT_INTERVAL_SECS`
    /// (default 21600; `0` disables the task, including its first run).
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let non_empty = |key: &str| get(key).filter(|v| !v.trim().is_empty());
        let dir = non_empty("ONTOLOGY_CHECKPOINT_DIR")
            .or_else(|| non_empty("ONTOLOGY_BACKUP_DIR"))
            .map(PathBuf::from);
        let secs = non_empty("ONTOLOGY_CHECKPOINT_INTERVAL_SECS")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_INTERVAL_SECS);
        Self {
            dir,
            interval: (secs > 0).then(|| Duration::from_secs(secs)),
        }
    }
}

/// Handle to the running task; call [`CheckpointTask::shutdown`] on exit.
#[derive(Debug)]
pub struct CheckpointTask {
    stop: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl CheckpointTask {
    /// Stops the task after any checkpoint in progress has finished.
    pub async fn shutdown(self) {
        // The receiver lives in the task; a send error means it already ended.
        let _ = self.stop.send(true);
        if let Err(e) = self.join.await {
            warn!("ontology checkpoint: task ended abnormally: {e}");
        }
    }
}

/// Starts the checkpoint task, or logs why it is off and returns `None`.
///
/// `data_dir` is the server's data directory; the destination must lie outside
/// it.
pub fn spawn(
    store: Arc<Store>,
    data_dir: PathBuf,
    config: CheckpointConfig,
) -> Option<CheckpointTask> {
    let Some(interval) = config.interval else {
        info!("ontology checkpoint: disabled (ONTOLOGY_CHECKPOINT_INTERVAL_SECS=0)");
        return None;
    };
    let Some(dir) = config.dir else {
        info!(
            "ontology checkpoint: not running, no directory set \
             (ONTOLOGY_CHECKPOINT_DIR); the backup tool has nothing to read"
        );
        return None;
    };
    let (stop, mut stopped) = watch::channel(false);
    let join = tokio::spawn(async move {
        loop {
            let (store, data_dir, dir) = (Arc::clone(&store), data_dir.clone(), dir.clone());
            match tokio::task::spawn_blocking(move || run_once(&store, &data_dir, &dir)).await {
                Ok(()) => {}
                Err(e) => warn!("ontology checkpoint: run did not complete: {e}"),
            }
            // Sleep after the run, not on a fixed tick, so runs never overlap.
            tokio::select! {
                () = tokio::time::sleep(interval) => {}
                _ = stopped.changed() => break,
            }
        }
        info!("ontology checkpoint: stopped");
    });
    Some(CheckpointTask { stop, join })
}

fn run_once(store: &Store, data_dir: &std::path::Path, dir: &std::path::Path) {
    let started = Instant::now();
    match checkpoint_oxigraph(store, data_dir, dir) {
        Ok(path) => info!(
            "ontology checkpoint: wrote {} in {:.1}s",
            path.display(),
            started.elapsed().as_secs_f32()
        ),
        Err(e) => warn!("ontology checkpoint: failed, keeping the previous one: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{GraphName, NamedNode, Quad};
    use std::collections::HashMap;
    use visionclaw_backup::checkpoint;

    fn config(pairs: &[(&str, &str)]) -> CheckpointConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        CheckpointConfig::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn defaults_to_six_hours_and_needs_a_directory() {
        let c = config(&[]);
        assert_eq!(c.interval, Some(Duration::from_secs(21_600)));
        assert_eq!(c.dir, None);
    }

    #[test]
    fn checkpoint_dir_wins_over_the_older_backup_dir_and_zero_disables() {
        let c = config(&[
            ("ONTOLOGY_BACKUP_DIR", "/old"),
            ("ONTOLOGY_CHECKPOINT_DIR", "/new"),
            ("ONTOLOGY_CHECKPOINT_INTERVAL_SECS", "0"),
        ]);
        assert_eq!(c.dir, Some(PathBuf::from("/new")));
        assert_eq!(c.interval, None);
        let legacy = config(&[("ONTOLOGY_BACKUP_DIR", "/old")]);
        assert_eq!(legacy.dir, Some(PathBuf::from("/old")));
    }

    #[tokio::test]
    async fn task_writes_on_start_refreshes_each_interval_and_stops_cleanly() {
        let fixture = tempfile::tempdir().expect("tempdir");
        let data = fixture.path().join("data");
        std::fs::create_dir(&data).expect("data dir");
        let store = Arc::new(Store::open(data.join("oxigraph")).expect("open"));
        store
            .insert(&Quad::new(
                NamedNode::new("urn:a").expect("iri"),
                NamedNode::new("urn:p").expect("iri"),
                NamedNode::new("urn:b").expect("iri"),
                GraphName::DefaultGraph,
            ))
            .expect("insert");
        let dir = fixture.path().join("checkpoints");
        let task = spawn(
            Arc::clone(&store),
            data,
            CheckpointConfig {
                dir: Some(dir.clone()),
                interval: Some(Duration::from_millis(50)),
            },
        )
        .expect("task starts");
        let deadline = Instant::now() + Duration::from_secs(30);
        while checkpoint::list(&dir).map_or(0, |l| l.len()) < 2 {
            assert!(Instant::now() < deadline, "no second checkpoint appeared");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        task.shutdown().await;
        let latest = checkpoint::latest(&dir).expect("latest");
        assert_eq!(latest.sidecar.triples, 1);
        let before = checkpoint::list(&dir).expect("list");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            checkpoint::list(&dir).expect("list"),
            before,
            "stopped task writes nothing"
        );
    }

    #[tokio::test]
    async fn a_zero_interval_or_missing_directory_starts_nothing() {
        let store = Arc::new(Store::new().expect("memory store"));
        let off = CheckpointConfig {
            dir: Some(PathBuf::from("/nowhere")),
            interval: None,
        };
        assert!(spawn(Arc::clone(&store), PathBuf::from("/d"), off).is_none());
        let no_dir = CheckpointConfig {
            dir: None,
            interval: Some(Duration::from_secs(1)),
        };
        assert!(spawn(store, PathBuf::from("/d"), no_dir).is_none());
    }
}
