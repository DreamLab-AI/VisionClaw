//! Periodic Oxigraph checkpoints written by the owning process.
//!
//! Oxigraph 0.4 on RocksDB allows one read-write process, so only the server
//! can copy its own store while it runs. [`write()`] does that with
//! [`Store::backup`], which asks RocksDB to create a checkpoint of the open
//! transactional database: a point-in-time image (memtable flushed or WAL
//! copied, immutable SST files hard-linked on the same filesystem) that opens
//! like any Oxigraph store and is independent of the live one.
//!
//! # Layout
//!
//! ```text
//! <dir>/checkpoint-20261116T120000123Z/
//!     store/            RocksDB checkpoint, openable with Store::open_read_only
//!     checkpoint.json   when it was taken, triple count, format
//! <dir>/.checkpoint-20261116T180000456Z.partial/   a write in progress
//! ```
//!
//! A checkpoint is built under a dot-prefixed `.partial` name and renamed once
//! the store and sidecar are on disk, so a reader never sees half of one. A
//! folder only counts as complete when its name matches the pattern *and* its
//! sidecar parses. [`write()`] keeps the newest [`KEEP`] complete checkpoints.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use oxigraph::store::Store;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

/// File name of the sidecar inside a checkpoint folder.
pub const SIDECAR_FILE: &str = "checkpoint.json";
/// Folder inside a checkpoint that holds the RocksDB checkpoint.
pub const STORE_SUBDIR: &str = "store";
/// Value of [`Sidecar::format`] for the checkpoints this module writes.
pub const FORMAT: &str = "oxigraph-0.4-rocksdb-checkpoint";
/// Complete checkpoints kept: the latest plus one previous.
pub const KEEP: usize = 2;

const PREFIX: &str = "checkpoint-";
const PARTIAL_SUFFIX: &str = ".partial";
/// Millisecond resolution so quick successive runs get distinct, ordered names.
const STAMP_FORMAT: &str = "%Y%m%dT%H%M%S%3fZ";
const STAMP_LEN: usize = 19;

/// What a checkpoint's sidecar records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidecar {
    /// UTC time the checkpoint was taken, RFC 3339.
    pub taken_utc: String,
    /// Number of quads in the checkpointed store, counted from the checkpoint.
    pub triples: u64,
    /// Format identifier, [`FORMAT`] for checkpoints written here.
    pub format: String,
}

/// A complete checkpoint on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// Folder name, `checkpoint-<stamp>`.
    pub name: String,
    /// The folder itself.
    pub path: PathBuf,
    /// The sidecar read from it.
    pub sidecar: Sidecar,
}

impl Checkpoint {
    /// The RocksDB checkpoint inside, openable as an Oxigraph store.
    #[must_use]
    pub fn store_dir(&self) -> PathBuf {
        self.path.join(STORE_SUBDIR)
    }

    /// The time the checkpoint was taken.
    ///
    /// # Errors
    ///
    /// Fails if the sidecar's `taken_utc` is not RFC 3339.
    pub fn taken(&self) -> Result<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&self.sidecar.taken_utc)
            .map(|t| t.with_timezone(&Utc))
            .with_context(|| format!("checkpoint {} has an unreadable taken_utc", self.name))
    }
}

fn is_checkpoint_name(name: &str) -> bool {
    name.strip_prefix(PREFIX).is_some_and(|stamp| {
        stamp.len() == STAMP_LEN
            && stamp.bytes().enumerate().all(|(i, b)| match i {
                8 => b == b'T',
                18 => b == b'Z',
                _ => b.is_ascii_digit(),
            })
    })
}

fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .with_context(|| format!("syncing {}", dir.display()))
}

/// Takes a checkpoint of `store` into `dir` and prunes to the newest [`KEEP`].
///
/// Blocking: call it from a blocking thread when running under an async
/// executor. Stale `.partial` folders from an interrupted earlier run are
/// removed first. Only folders matching this module's naming are ever pruned.
///
/// # Errors
///
/// Fails if `store` is in memory, `dir` cannot be created, or the checkpoint,
/// its triple count, the sidecar or the final rename fails. On failure the
/// partial folder is removed and no new complete checkpoint exists.
pub fn write(store: &Store, dir: &Path) -> Result<Checkpoint> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    clear_partials(dir)?;

    let taken = Utc::now();
    let mut stamp_time = taken;
    let (name, final_dir) = loop {
        let name = format!("{PREFIX}{}", stamp_time.format(STAMP_FORMAT));
        let candidate = dir.join(&name);
        if !candidate.exists() {
            break (name, candidate);
        }
        stamp_time += chrono::Duration::milliseconds(1);
    };
    let partial = dir.join(format!(".{name}{PARTIAL_SUFFIX}"));
    fs::create_dir(&partial).with_context(|| format!("creating {}", partial.display()))?;

    let result = fill(store, &partial, taken);
    let sidecar = match result {
        Ok(sidecar) => sidecar,
        Err(err) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(err);
        }
    };
    if let Err(err) = fs::rename(&partial, &final_dir)
        .with_context(|| format!("publishing {}", final_dir.display()))
    {
        let _ = fs::remove_dir_all(&partial);
        return Err(err);
    }
    sync_dir(dir)?;
    prune(dir, KEEP)?;
    Ok(Checkpoint {
        name,
        path: final_dir,
        sidecar,
    })
}

/// Fills a partial folder: the RocksDB checkpoint first, the sidecar last.
fn fill(store: &Store, partial: &Path, taken: DateTime<Utc>) -> Result<Sidecar> {
    let store_dir = partial.join(STORE_SUBDIR);
    store
        .backup(&store_dir)
        .map_err(|e| anyhow!("Oxigraph checkpoint failed: {e}"))?;
    // Counting from the checkpoint (not the live store) keeps the figure true
    // to the image; it also proves the image opens.
    let triples = {
        let copy = Store::open_read_only(&store_dir)
            .map_err(|e| anyhow!("checkpoint does not open: {e}"))?;
        copy.len()
            .map_err(|e| anyhow!("counting checkpoint quads: {e}"))? as u64
    };
    let sidecar = Sidecar {
        taken_utc: taken.to_rfc3339_opts(SecondsFormat::Millis, true),
        triples,
        format: FORMAT.to_owned(),
    };
    let path = partial.join(SIDECAR_FILE);
    fs::write(&path, serde_json::to_vec_pretty(&sidecar)?)
        .with_context(|| format!("writing {}", path.display()))?;
    File::open(&path)?.sync_all()?;
    sync_dir(partial)?;
    Ok(sidecar)
}

/// Removes `.checkpoint-*.partial` folders left by an interrupted write.
fn clear_partials(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!(".{PREFIX}")) && name.ends_with(PARTIAL_SUFFIX) {
            fs::remove_dir_all(entry.path())
                .with_context(|| format!("removing stale {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Complete checkpoints under `dir`, oldest first.
///
/// `.partial` folders, folders whose name does not match the pattern and
/// folders without a parsable sidecar are not checkpoints and are skipped.
///
/// # Errors
///
/// Fails if `dir` cannot be read.
pub fn list(dir: &Path) -> Result<Vec<Checkpoint>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_checkpoint_name(&name) || !entry.path().join(STORE_SUBDIR).is_dir() {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path().join(SIDECAR_FILE)) else {
            continue;
        };
        let Ok(sidecar) = serde_json::from_slice::<Sidecar>(&bytes) else {
            continue;
        };
        found.push(Checkpoint {
            name,
            path: entry.path(),
            sidecar,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// The newest complete checkpoint under `dir`.
///
/// # Errors
///
/// Fails if `dir` cannot be read or holds no complete checkpoint.
pub fn latest(dir: &Path) -> Result<Checkpoint> {
    list(dir)?
        .pop()
        .ok_or_else(|| anyhow!("no complete Oxigraph checkpoint under {}", dir.display()))
}

/// Removes complete checkpoints beyond the newest `keep`; returns those removed.
fn prune(dir: &Path, keep: usize) -> Result<Vec<PathBuf>> {
    let all = list(dir)?;
    let excess = all.len().saturating_sub(keep);
    let mut removed = Vec::new();
    for old in all.into_iter().take(excess) {
        fs::remove_dir_all(&old.path).with_context(|| format!("pruning {}", old.path.display()))?;
        removed.push(old.path);
    }
    Ok(removed)
}

/// Checks that a checkpoint is young enough to back up.
///
/// # Errors
///
/// Fails, naming the checkpoint and both ages, when it was taken more than
/// `max_age` before `now`, or when its taken time cannot be read.
pub fn require_fresh(
    checkpoint: &Checkpoint,
    now: DateTime<Utc>,
    max_age: std::time::Duration,
) -> Result<()> {
    let taken = checkpoint.taken()?;
    let age = now.signed_duration_since(taken);
    let limit = chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::MAX);
    if age > limit {
        bail!(
            "latest Oxigraph checkpoint {} was taken {} ({} h old), older than the {} h maximum; \
             the server's checkpoint task has stopped or is failing: check its log",
            checkpoint.name,
            checkpoint.sidecar.taken_utc,
            age.num_hours(),
            limit.num_hours()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{GraphName, NamedNode, Quad};

    fn quad(n: u32) -> Quad {
        Quad::new(
            NamedNode::new(format!("urn:s:{n}")).expect("valid IRI"),
            NamedNode::new("urn:p").expect("valid IRI"),
            NamedNode::new(format!("urn:o:{n}")).expect("valid IRI"),
            GraphName::NamedNode(NamedNode::new("urn:g").expect("valid IRI")),
        )
    }

    fn live_store(root: &Path, quads: u32) -> Store {
        let store = Store::open(root.join("live")).expect("open store");
        for n in 0..quads {
            store.insert(&quad(n)).expect("insert");
        }
        store
    }

    #[test]
    fn writes_a_complete_checkpoint_with_sidecar() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = live_store(tmp.path(), 7);
        let dir = tmp.path().join("cp");
        let cp = write(&store, &dir).expect("checkpoint");
        assert_eq!(cp.sidecar.triples, 7);
        assert_eq!(cp.sidecar.format, FORMAT);
        assert!(cp.path.join(SIDECAR_FILE).is_file());
        assert_eq!(latest(&dir).expect("latest"), cp);
        let copy = Store::open_read_only(cp.store_dir()).expect("open copy");
        assert_eq!(copy.len().expect("len"), 7);
    }

    #[test]
    fn keeps_the_latest_and_one_previous() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = live_store(tmp.path(), 1);
        let dir = tmp.path().join("cp");
        let first = write(&store, &dir).expect("first");
        store.insert(&quad(100)).expect("insert");
        let second = write(&store, &dir).expect("second");
        store.insert(&quad(101)).expect("insert");
        let third = write(&store, &dir).expect("third");
        let names: Vec<String> = list(&dir)
            .expect("list")
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, vec![second.name, third.name.clone()]);
        assert!(!first.path.exists(), "oldest checkpoint pruned");
        assert_eq!(latest(&dir).expect("latest").sidecar.triples, 3);
    }

    #[test]
    fn failed_write_leaves_nothing_visible_and_keeps_the_previous() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = live_store(tmp.path(), 2);
        let dir = tmp.path().join("cp");
        let good = write(&store, &dir).expect("good");
        // An in-memory store cannot be checkpointed: a deterministic failure.
        let memory = Store::new().expect("memory store");
        assert!(write(&memory, &dir).is_err());
        assert_eq!(latest(&dir).expect("latest"), good);
        let leftovers: Vec<String> = fs::read_dir(&dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec![good.name], "no partial folder remains");
    }

    #[test]
    fn interrupted_writes_are_never_complete() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("cp");
        // Killed mid-write: a dot-prefixed partial, and a published-looking
        // folder whose sidecar was never written, and one with a torn sidecar.
        fs::create_dir_all(dir.join(".checkpoint-20261116T120000000Z.partial/store")).expect("mk");
        fs::create_dir_all(dir.join("checkpoint-20261116T130000000Z/store")).expect("mk");
        fs::create_dir_all(dir.join("checkpoint-20261116T140000000Z/store")).expect("mk");
        fs::write(
            dir.join("checkpoint-20261116T140000000Z")
                .join(SIDECAR_FILE),
            b"{\"taken",
        )
        .expect("torn sidecar");
        assert!(list(&dir).expect("list").is_empty());
        assert!(latest(&dir).is_err());

        let store = live_store(tmp.path(), 1);
        let cp = write(&store, &dir).expect("write");
        assert!(!dir.join(".checkpoint-20261116T120000000Z.partial").exists());
        assert_eq!(latest(&dir).expect("latest"), cp);
        // Prune touched only complete checkpoints; the sidecar-less folders stay.
        assert!(dir.join("checkpoint-20261116T130000000Z").exists());
    }

    #[test]
    fn unrelated_folders_are_never_pruned() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("cp");
        fs::create_dir_all(dir.join("ontology-1234")).expect("legacy folder");
        let store = live_store(tmp.path(), 1);
        for _ in 0..4 {
            write(&store, &dir).expect("write");
        }
        assert!(dir.join("ontology-1234").exists());
        assert_eq!(list(&dir).expect("list").len(), KEEP);
    }

    #[test]
    fn freshness_is_judged_against_the_given_clock() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = live_store(tmp.path(), 1);
        let cp = write(&store, &tmp.path().join("cp")).expect("checkpoint");
        let taken = cp.taken().expect("taken");
        let day = std::time::Duration::from_secs(24 * 3600);
        require_fresh(&cp, taken + chrono::Duration::hours(23), day).expect("fresh");
        let err = require_fresh(&cp, taken + chrono::Duration::hours(25), day)
            .expect_err("stale")
            .to_string();
        assert!(
            err.contains(&cp.name) && err.contains("24 h maximum"),
            "{err}"
        );
    }
}
