//! Durable state under the data directory: atomic writes, warm positions and the
//! actor id table.
//!
//! Every file is written to a sibling temporary and renamed into place, so a
//! crash never leaves a torn `ids.bin` or `positions.bin`. Positions and actor
//! ids carry the interner epoch and are discarded on mismatch: after a
//! re-extract the old ids name different nodes.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use codescape_model::NODE_ID_MASK;

/// Write `bytes` to `path` atomically (temp file in the same directory, fsync, rename).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("path has no file name"))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Errors decoding persisted files.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PersistError {
    /// Wrong magic, truncated or trailing bytes.
    #[error("malformed {0}")]
    Malformed(&'static str),
    /// Written under another interner epoch.
    #[error("epoch mismatch: file {file}, interner {current}")]
    Epoch {
        /// Epoch in the file.
        file: u32,
        /// Current epoch.
        current: u32,
    },
}

fn read_u32(b: &[u8], at: &mut usize) -> Result<u32, PersistError> {
    let s = b
        .get(*at..*at + 4)
        .ok_or(PersistError::Malformed("truncated"))?;
    *at += 4;
    Ok(u32::from_le_bytes(s.try_into().unwrap()))
}

fn read_f32(b: &[u8], at: &mut usize) -> Result<f32, PersistError> {
    read_u32(b, at).map(f32::from_bits)
}

/// Warm-start positions keyed by wire id.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WarmPositions {
    /// Interner epoch the ids belong to.
    pub epoch: u32,
    /// `(id, position, velocity)`.
    pub entries: Vec<(u32, [f32; 3], [f32; 3])>,
}

impl WarmPositions {
    const MAGIC: &'static [u8; 6] = b"CSPOS\x01";

    /// Binary form: magic, epoch, count, then `u32 id, 3×f32 pos, 3×f32 vel` per entry.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(14 + self.entries.len() * 28);
        out.extend_from_slice(Self::MAGIC);
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for (id, p, v) in &self.entries {
            out.extend_from_slice(&id.to_le_bytes());
            for x in p.iter().chain(v.iter()) {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        out
    }

    /// Decode, refusing a file from another epoch and dropping non-finite entries.
    pub fn from_bytes(b: &[u8], expect_epoch: u32) -> Result<Self, PersistError> {
        if b.get(..6) != Some(&Self::MAGIC[..]) {
            return Err(PersistError::Malformed("positions magic"));
        }
        let mut at = 6;
        let epoch = read_u32(b, &mut at)?;
        if epoch != expect_epoch {
            return Err(PersistError::Epoch {
                file: epoch,
                current: expect_epoch,
            });
        }
        let n = read_u32(b, &mut at)? as usize;
        if b.len() != at + n * 28 {
            return Err(PersistError::Malformed("positions length"));
        }
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let id = read_u32(b, &mut at)?;
            let mut p = [0f32; 3];
            let mut v = [0f32; 3];
            for x in p.iter_mut().chain(v.iter_mut()) {
                *x = read_f32(b, &mut at)?;
            }
            if p.iter().chain(v.iter()).all(|x| x.is_finite()) {
                entries.push((id, p, v));
            }
        }
        Ok(Self { epoch, entries })
    }
}

/// Size of each actor id range.
pub const ACTOR_RANGE: u32 = 1 << 16;

/// Labels the server mints for anonymous connections (`guest-N`, `voice-N`), and any
/// client-chosen label with the same prefix: never persisted.
pub fn is_ephemeral_label(label: &str) -> bool {
    label.starts_with("guest-") || label.starts_with("voice-")
}

/// Actor label → id, allocated downward inside a fixed range at the top of the
/// 26-bit id space, so actors never meet the interner (which allocates upward
/// from 1). Two ranges exist: [`ActorIds::new`] is the persisted table
/// (`actors.bin`, named actors); [`ActorIds::ephemeral`] sits directly below it,
/// lives only in memory and starts empty on every boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorIds {
    epoch: u32,
    by_label: BTreeMap<String, u32>,
    next_down: u32,
    bottom: u32,
}

impl ActorIds {
    const MAGIC: &'static [u8; 6] = b"CSACT\x01";

    /// An empty persisted table for `epoch`: ids `NODE_ID_MASK` down to
    /// `NODE_ID_MASK - ACTOR_RANGE + 1`.
    pub fn new(epoch: u32) -> Self {
        Self {
            epoch,
            by_label: BTreeMap::new(),
            next_down: NODE_ID_MASK,
            bottom: NODE_ID_MASK - ACTOR_RANGE,
        }
    }

    /// An empty per-boot table for anonymous actors, the range directly below [`ActorIds::new`].
    pub fn ephemeral() -> Self {
        let top = NODE_ID_MASK - ACTOR_RANGE;
        Self {
            epoch: 0,
            by_label: BTreeMap::new(),
            next_down: top,
            bottom: top - ACTOR_RANGE,
        }
    }

    /// Ids still available in this range.
    pub fn remaining(&self) -> u32 {
        self.next_down - self.bottom
    }

    /// The id for `label`, allocating when new. `interner_len` is the highest code
    /// id in use; allocation fails rather than meeting it or leaving the range.
    pub fn get_or_alloc(&mut self, label: &str, interner_len: u32) -> Option<u32> {
        if let Some(&id) = self.by_label.get(label) {
            return Some(id);
        }
        if self.next_down <= self.bottom || self.next_down <= interner_len.saturating_add(1) {
            return None;
        }
        let id = self.next_down;
        self.next_down -= 1;
        self.by_label.insert(label.to_string(), id);
        Some(id)
    }

    /// Lookup by label, never allocating.
    pub fn get(&self, label: &str) -> Option<u32> {
        self.by_label.get(label).copied()
    }

    /// Binary form: magic, epoch, count, then `u32 id, u32 len, utf8` per actor.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(Self::MAGIC);
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out.extend_from_slice(&(self.by_label.len() as u32).to_le_bytes());
        for (label, id) in &self.by_label {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&(label.len() as u32).to_le_bytes());
            out.extend_from_slice(label.as_bytes());
        }
        out
    }

    /// Decode a persisted table, refusing another epoch, ids outside the persisted
    /// range, and ephemeral labels (which a file must never contain).
    pub fn from_bytes(b: &[u8], expect_epoch: u32) -> Result<Self, PersistError> {
        if b.get(..6) != Some(&Self::MAGIC[..]) {
            return Err(PersistError::Malformed("actors magic"));
        }
        let mut at = 6;
        let epoch = read_u32(b, &mut at)?;
        if epoch != expect_epoch {
            return Err(PersistError::Epoch {
                file: epoch,
                current: expect_epoch,
            });
        }
        let n = read_u32(b, &mut at)?;
        let mut me = Self::new(epoch);
        for _ in 0..n {
            let id = read_u32(b, &mut at)?;
            let len = read_u32(b, &mut at)? as usize;
            let raw = b
                .get(at..at + len)
                .ok_or(PersistError::Malformed("actor label"))?;
            at += len;
            let label =
                std::str::from_utf8(raw).map_err(|_| PersistError::Malformed("actor utf8"))?;
            if id <= me.bottom || id > NODE_ID_MASK || me.by_label.values().any(|&v| v == id) {
                return Err(PersistError::Malformed("actor id"));
            }
            if is_ephemeral_label(label) {
                // Written by an older build; drop it rather than keep a squatted name.
                continue;
            }
            me.by_label.insert(label.to_string(), id);
            me.next_down = me.next_down.min(id - 1);
        }
        if at != b.len() {
            return Err(PersistError::Malformed("actors trailing bytes"));
        }
        Ok(me)
    }
}

/// An append-only JSON-lines log rotated by size: `log` → `log.1` → … → `log.<keep>`,
/// the oldest dropped. Bounds `timeline.jsonl` to about `max_bytes × (keep + 1)`.
#[derive(Debug)]
pub struct RotatingLog {
    path: std::path::PathBuf,
    max_bytes: u64,
    keep: u32,
    size: u64,
    file: Option<std::fs::File>,
}

impl RotatingLog {
    /// Rotate at 10 MB.
    pub const DEFAULT_MAX_BYTES: u64 = 10 * 1024 * 1024;
    /// Keep three rotated files.
    pub const DEFAULT_KEEP: u32 = 3;

    /// Open (append) `path`, picking up its current size.
    pub fn open(path: &Path, max_bytes: u64, keep: u32) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path: path.to_path_buf(),
            max_bytes: max_bytes.max(1),
            keep,
            size,
            file: Some(file),
        })
    }

    fn rotated(&self, n: u32) -> std::path::PathBuf {
        let mut p = self.path.clone().into_os_string();
        p.push(format!(".{n}"));
        p.into()
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file = None;
        if self.keep == 0 {
            std::fs::remove_file(&self.path).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
        } else {
            let _ = std::fs::remove_file(self.rotated(self.keep));
            for n in (1..self.keep).rev() {
                let from = self.rotated(n);
                if from.exists() {
                    std::fs::rename(&from, self.rotated(n + 1))?;
                }
            }
            std::fs::rename(&self.path, self.rotated(1))?;
        }
        self.file = Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );
        self.size = 0;
        Ok(())
    }

    /// Append one line (a newline is added), rotating first if it would cross the limit.
    pub fn append(&mut self, line: &str) -> std::io::Result<()> {
        let n = line.len() as u64 + 1;
        if self.size > 0 && self.size + n > self.max_bytes {
            self.rotate()?;
        }
        let f = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("log closed"))?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        self.size += n;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/ids.bin");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn positions_round_trip_and_epoch_guard() {
        let w = WarmPositions {
            epoch: 3,
            entries: vec![
                (1, [1.0, 2.0, 3.0], [0.1, 0.2, 0.3]),
                (9, [4.0; 3], [0.0; 3]),
            ],
        };
        let b = w.to_bytes();
        assert_eq!(WarmPositions::from_bytes(&b, 3).unwrap(), w);
        assert_eq!(
            WarmPositions::from_bytes(&b, 4),
            Err(PersistError::Epoch {
                file: 3,
                current: 4
            })
        );
        assert!(WarmPositions::from_bytes(&b[..b.len() - 1], 3).is_err());
        let mut nan = w.clone();
        nan.entries[0].1[0] = f32::NAN;
        assert_eq!(
            WarmPositions::from_bytes(&nan.to_bytes(), 3)
                .unwrap()
                .entries
                .len(),
            1
        );
    }

    #[test]
    fn actor_ids_allocate_downward_and_persist() {
        let mut a = ActorIds::new(1);
        let x = a.get_or_alloc("claude:tab0", 100).unwrap();
        let y = a.get_or_alloc("john", 100).unwrap();
        assert_eq!(x, NODE_ID_MASK);
        assert_eq!(y, NODE_ID_MASK - 1);
        assert_eq!(a.get_or_alloc("claude:tab0", 100), Some(x));
        let back = ActorIds::from_bytes(&a.to_bytes(), 1).unwrap();
        assert_eq!(back, a);
        assert!(ActorIds::from_bytes(&a.to_bytes(), 2).is_err());
        let mut full = ActorIds::new(1);
        assert_eq!(
            full.get_or_alloc("z", NODE_ID_MASK - 1),
            None,
            "never meets the interner"
        );
    }

    #[test]
    fn ephemeral_range_sits_below_and_never_overlaps() {
        let mut p = ActorIds::new(1);
        let mut e = ActorIds::ephemeral();
        let pid = p.get_or_alloc("john", 10).unwrap();
        let eid = e.get_or_alloc("guest-1", 10).unwrap();
        assert_eq!(eid, NODE_ID_MASK - ACTOR_RANGE);
        assert!(eid < pid);
        assert!(
            is_ephemeral_label("guest-1")
                && is_ephemeral_label("voice-9")
                && !is_ephemeral_label("john")
        );
        // The persisted range is bounded: it cannot spill into the ephemeral one.
        let mut small = ActorIds::new(1);
        for i in 0..ACTOR_RANGE {
            assert!(small.get_or_alloc(&format!("a{i}"), 10).is_some());
        }
        assert_eq!(small.remaining(), 0);
        assert_eq!(small.get_or_alloc("one-more", 10), None);
    }

    #[test]
    fn persisted_ephemeral_labels_are_dropped_on_load() {
        let mut old = ActorIds::new(1);
        old.get_or_alloc("guest-3", 10);
        old.get_or_alloc("john", 10);
        let back = ActorIds::from_bytes(&old.to_bytes(), 1).unwrap();
        assert_eq!(back.get("guest-3"), None);
        assert!(back.get("john").is_some());
    }

    #[test]
    fn rotating_log_bounds_size_and_keeps_three() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("timeline.jsonl");
        let mut log = RotatingLog::open(&p, 100, 3).unwrap();
        for i in 0..40 {
            log.append(&format!("{{\"n\":{i:04},\"pad\":\"xxxxxxxxxx\"}}"))
                .unwrap();
        }
        let mut names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "timeline.jsonl",
                "timeline.jsonl.1",
                "timeline.jsonl.2",
                "timeline.jsonl.3"
            ]
        );
        for n in &names {
            assert!(
                std::fs::metadata(d.path().join(n)).unwrap().len() <= 100,
                "{n}"
            );
        }
        let newest = std::fs::read_to_string(&p).unwrap();
        assert!(
            newest.contains("\"n\":0039"),
            "the newest line is in the live file"
        );
        // Reopening picks up the existing size instead of growing past the limit.
        let mut again = RotatingLog::open(&p, 100, 3).unwrap();
        again.append("{\"x\":1}").unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() <= 100);
    }
}
