//! Debounced file watching.
//!
//! Directories are registered one at a time (non-recursive watches), walking
//! from the root and never entering a directory the [`PathFilter`] rejects:
//! hidden, gitignored, excluded, graphicle noise (`node_modules`, `target`…)
//! or the out dir. A subtree that cannot be watched or read (`EACCES`, a
//! directory deleted mid-walk, the inotify watch limit) is skipped and logged
//! at debug; only a root that cannot be watched is an error. Directories
//! created or moved in later are registered when their event arrives.
//!
//! A `notify` recommended watcher (inotify on Linux) feeds raw paths to one
//! thread. Each path is first filtered through the same [`PathFilter`] the
//! cold walk uses (graphicle's ignore rules, the configured excludes,
//! supported code extensions, no hidden directories, nothing under the out
//! dir), so ignored churn such as a log written every 100 ms never delays a
//! batch. Relevant paths collect until [`DEBOUNCE`] passes without another, or
//! [`MAX_BATCH_WAIT`] after the first, and go out as one sorted batch.
//! Directory paths pass too (a moved directory is one event); paths that no
//! longer exist pass so a removal reaches [`crate::Ingestor::apply_changed`],
//! which drops whatever it has not indexed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{RecvTimeoutError, SendTimeoutError, Sender};
use notify::Watcher;
use parking_lot::Mutex;

use crate::engine::PathFilter;
use crate::IngestError;

/// Quiet time after the last relevant event before a batch is sent.
pub const DEBOUNCE: Duration = Duration::from_millis(250);
/// Longest a relevant event waits: a batch is sent this long after its first
/// path even if relevant events keep arriving.
pub const MAX_BATCH_WAIT: Duration = Duration::from_secs(1);
/// How often the thread checks the stop flag while idle.
const POLL: Duration = Duration::from_millis(100);

/// A running watcher. Dropping it stops the OS watch and joins the thread.
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
    registry: Arc<Mutex<Registry>>,
    thread: Option<JoinHandle<()>>,
}

/// The OS watcher and the directories it holds a watch on.
struct Registry {
    watcher: notify::RecommendedWatcher,
    dirs: BTreeSet<PathBuf>,
}

impl Registry {
    /// Watch `start` and every admitted directory below it. Returns the
    /// number of directories added. Fails only when `start` is the root and
    /// cannot be watched.
    fn register(
        &mut self,
        start: &Path,
        root: &Path,
        filter: &PathFilter,
    ) -> Result<usize, IngestError> {
        let mut added = 0;
        let mut stack = vec![start.to_path_buf()];
        while let Some(dir) = stack.pop() {
            if dir != root && !filter.admits_dir(&dir) {
                continue;
            }
            if !self.dirs.contains(&dir) {
                if let Err(e) = self
                    .watcher
                    .watch(&dir, notify::RecursiveMode::NonRecursive)
                {
                    if dir == root {
                        return Err(IngestError::Watch(format!("{}: {e}", root.display())));
                    }
                    tracing::debug!(dir = %dir.display(), error = %e, "codescape watch: skipping subtree");
                    continue;
                }
                self.dirs.insert(dir.clone());
                added += 1;
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) => {
                    tracing::debug!(dir = %dir.display(), error = %e, "codescape watch: cannot list directory");
                    continue;
                }
            };
            for entry in entries.flatten() {
                // Symlinked directories are not followed (no cycles, no escapes).
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(entry.path());
                }
            }
        }
        Ok(added)
    }

    /// Forget watches the OS dropped with their (removed or moved) directory.
    fn forget_under(&mut self, gone: &Path) {
        self.dirs.retain(|d| !d.starts_with(gone) || d.is_dir());
    }
}

impl std::fmt::Debug for WatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchHandle")
            .field("running", &self.is_running())
            .finish()
    }
}

impl WatchHandle {
    /// Whether the batching thread is still alive (it exits when the
    /// receiving end of the batch channel is dropped).
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Number of directories currently watched (skipped subtrees excluded).
    pub fn watched_dirs(&self) -> usize {
        self.registry.lock().dirs.len()
    }

    /// Whether `dir` (absolute, canonical) holds a watch.
    #[cfg(test)]
    pub(crate) fn is_watching(&self, dir: &Path) -> bool {
        self.registry.lock().dirs.contains(dir)
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        // The thread sees the flag within one poll; the OS watcher goes with
        // the last reference to the registry, after the join.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start watching `root` (and every admitted directory below it), sending
/// filtered batches to `tx`.
pub(crate) fn spawn(
    root: &Path,
    filter: Arc<PathFilter>,
    tx: Sender<Vec<PathBuf>>,
) -> Result<WatchHandle, IngestError> {
    // Each raw path carries whether its event was structural (create, rename,
    // remove): only those may stand for a whole directory, so touching a
    // directory's metadata cannot trigger a re-extract of everything below it.
    let (raw_tx, raw_rx) = crossbeam_channel::unbounded::<(PathBuf, bool)>();
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            use notify::event::{EventKind, ModifyKind};
            let structural = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Name(_))
            );
            if !matches!(event.kind, EventKind::Access(_)) {
                for path in event.paths {
                    let _ = raw_tx.send((path, structural));
                }
            }
        }
    })
    .map_err(|e| IngestError::Watch(e.to_string()))?;
    let mut registry = Registry {
        watcher,
        dirs: BTreeSet::new(),
    };
    let count = registry.register(root, root, &filter)?;
    tracing::debug!(root = %root.display(), dirs = count, "codescape watch: registered");
    let registry = Arc::new(Mutex::new(registry));
    let thread_registry = registry.clone();
    let root = root.to_path_buf();

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("codescape-watch".into())
        .spawn(move || {
            let mut pending = Debouncer::default();
            // Every iteration waits at most one POLL, so the stop flag is seen
            // within ~100 ms even while events stream in continuously.
            while !stop_flag.load(Ordering::Relaxed) {
                let wait = pending.wait(Instant::now()).map_or(POLL, |w| w.min(POLL));
                match raw_rx.recv_timeout(wait) {
                    Ok((path, structural)) => {
                        // Watches follow the tree as it changes, whatever the filter says.
                        if structural {
                            if path.is_dir() {
                                if let Err(e) =
                                    thread_registry.lock().register(&path, &root, &filter)
                                {
                                    tracing::debug!(error = %e, "codescape watch: cannot register");
                                }
                            } else if !path.exists() {
                                thread_registry.lock().forget_under(&path);
                            }
                        }
                        // Filtered before the debounce: an ignored, excluded or
                        // unsupported path never opens or extends a batch.
                        if filter.admits(&path) || (structural && filter.admits_dir(&path)) {
                            pending.push(path, Instant::now());
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        if let Some(batch) = pending.take_if_due(Instant::now() + MAX_BATCH_WAIT) {
                            let _ = tx.try_send(batch);
                        }
                        return;
                    }
                }
                if let Some(batch) = pending.take_if_due(Instant::now()) {
                    if !send(&tx, batch, &stop_flag) {
                        return;
                    }
                }
            }
        })
        .map_err(|e| IngestError::Watch(e.to_string()))?;
    Ok(WatchHandle {
        stop,
        registry,
        thread: Some(thread),
    })
}

/// Send `batch`, giving up when the receiver is gone or the watcher is
/// stopping. A bounded, full channel must not keep `Drop` from joining.
fn send(tx: &Sender<Vec<PathBuf>>, mut batch: Vec<PathBuf>, stop: &AtomicBool) -> bool {
    loop {
        match tx.send_timeout(batch, POLL) {
            Ok(()) => return true,
            Err(SendTimeoutError::Timeout(back)) => {
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                batch = back;
            }
            Err(SendTimeoutError::Disconnected(_)) => return false,
        }
    }
}

/// The pending batch and its two clocks: it is due [`DEBOUNCE`] after its
/// last path, or [`MAX_BATCH_WAIT`] after its first, whichever comes first.
/// Time is passed in, so the rules are testable without sleeping.
#[derive(Debug, Default)]
struct Debouncer {
    paths: BTreeSet<PathBuf>,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Debouncer {
    /// Add a relevant path seen at `now`.
    fn push(&mut self, path: PathBuf, now: Instant) {
        self.paths.insert(path);
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// When the batch becomes due, or `None` when nothing is pending.
    fn due_at(&self) -> Option<Instant> {
        Some((self.last? + DEBOUNCE).min(self.first? + MAX_BATCH_WAIT))
    }

    /// Time left until the batch is due (zero if overdue).
    fn wait(&self, now: Instant) -> Option<Duration> {
        self.due_at().map(|d| d.saturating_duration_since(now))
    }

    /// The sorted batch, if it is due at `now`; the debouncer is then empty.
    fn take_if_due(&mut self, now: Instant) -> Option<Vec<PathBuf>> {
        if self.due_at()? > now {
            return None;
        }
        self.first = None;
        self.last = None;
        Some(std::mem::take(&mut self.paths).into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_period_closes_a_batch() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        assert_eq!(d.wait(t0), None);
        d.push("a.rs".into(), t0);
        d.push("b.rs".into(), t0 + Duration::from_millis(100));
        d.push("a.rs".into(), t0 + Duration::from_millis(150));
        assert_eq!(d.wait(t0 + Duration::from_millis(150)), Some(DEBOUNCE));
        assert!(d.take_if_due(t0 + Duration::from_millis(399)).is_none());
        let batch = d.take_if_due(t0 + Duration::from_millis(400)).unwrap();
        assert_eq!(batch, vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
        assert_eq!(d.wait(t0), None, "empty after a flush");
    }

    #[test]
    fn continuous_writes_flush_at_the_max_wait() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        let mut flushed_at = None;
        // A write every 200 ms forever: the quiet period never arrives.
        for step in 0..20u64 {
            let now = t0 + Duration::from_millis(200 * step);
            d.push(format!("hot{step}.rs").into(), now);
            if let Some(batch) = d.take_if_due(now) {
                flushed_at = Some((now - t0, batch.len()));
                break;
            }
        }
        assert_eq!(
            flushed_at,
            Some((MAX_BATCH_WAIT, 6)),
            "flushed at 1 s with writes 0..=1000 ms"
        );
    }
}
