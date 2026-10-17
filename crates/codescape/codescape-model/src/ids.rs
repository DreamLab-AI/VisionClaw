//! Interned 26-bit wire ids.
//!
//! The frozen V3 record carries a `u32` whose low 26 bits identify the node and
//! whose high bits are class flags (`docs/PROTOCOL-registry.md`). Hashing a
//! 200k-symbol graph into 2^26 slots would collide roughly 300 times, so ids are
//! handed out in order and remembered. The server persists the table with
//! [`IdInterner::to_bytes`]; a full re-extract bumps the epoch and starts over,
//! which matches ADR-2024's ephemeral wire id.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Low 26 bits of the wire id field. Mirrors `visionclaw-protocol`.
pub const NODE_ID_MASK: u32 = 0x03FF_FFFF;

/// Errors from the interner.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InternError {
    /// Every one of the 2^26 ids in this epoch has been handed out.
    #[error("wire id space exhausted in epoch {epoch}")]
    Exhausted {
        /// The epoch that ran out.
        epoch: u32,
    },
    /// The persisted bytes were not written by [`IdInterner::to_bytes`].
    #[error("interner bytes malformed: {0}")]
    Malformed(&'static str),
}

/// Monotonic key → wire-id table.
///
/// `Deserialize` goes through a private raw form so the lookup index is always
/// rebuilt; a table with an empty index would hand out a second id for a key
/// it already knows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(from = "RawInterner")]
pub struct IdInterner {
    epoch: u32,
    next: u32,
    keys: Vec<Arc<str>>,
    #[serde(skip)]
    index: HashMap<Arc<str>, u32>,
}

/// The serialised shape of [`IdInterner`] (no index).
#[derive(Deserialize)]
struct RawInterner {
    epoch: u32,
    #[allow(dead_code)]
    next: u32,
    keys: Vec<Arc<str>>,
}

impl From<RawInterner> for IdInterner {
    fn from(raw: RawInterner) -> Self {
        let mut keys = raw.keys;
        if keys.is_empty() {
            keys.push(Arc::from(""));
        }
        let mut me = Self {
            epoch: raw.epoch,
            next: 1,
            keys,
            index: HashMap::new(),
        };
        me.reindex();
        me
    }
}

impl Default for IdInterner {
    fn default() -> Self {
        Self::new(1)
    }
}

impl IdInterner {
    /// Serialisation magic: `CSID` + format version 1.
    const MAGIC: &'static [u8; 5] = b"CSID\x01";

    /// An empty interner for `epoch`. Id 0 is reserved (it reads as "no parent"
    /// and as the neutral SSSP parent), so the first interned key gets 1.
    pub fn new(epoch: u32) -> Self {
        Self {
            epoch,
            next: 1,
            keys: vec![Arc::from("")],
            index: HashMap::new(),
        }
    }

    /// The epoch this table belongs to.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Number of ids handed out so far.
    pub fn len(&self) -> usize {
        self.keys.len() - 1
    }

    /// `true` when nothing has been interned.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The id for `key`, allocating the next free one when it is new.
    pub fn intern(&mut self, key: &str) -> Result<u32, InternError> {
        if let Some(id) = self.index.get(key) {
            return Ok(*id);
        }
        if self.next > NODE_ID_MASK {
            return Err(InternError::Exhausted { epoch: self.epoch });
        }
        let id = self.next;
        self.next += 1;
        let key: Arc<str> = Arc::from(key);
        self.keys.push(key.clone());
        self.index.insert(key, id);
        Ok(id)
    }

    /// The id already assigned to `key`, if any. Never allocates.
    pub fn get(&self, key: &str) -> Option<u32> {
        self.index.get(key).copied()
    }

    /// The key behind an id (flag bits are ignored).
    pub fn key_of(&self, id: u32) -> Option<&Arc<str>> {
        let id = (id & NODE_ID_MASK) as usize;
        if id == 0 {
            return None;
        }
        self.keys.get(id)
    }

    /// Start a fresh epoch, dropping every assignment.
    pub fn next_epoch(&self) -> Self {
        Self::new(self.epoch.wrapping_add(1).max(1))
    }

    /// Compact binary form: magic, epoch, count, then `u32 len + utf8` per key.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.keys.len() * 24);
        out.extend_from_slice(Self::MAGIC);
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out.extend_from_slice(&(self.len() as u32).to_le_bytes());
        for key in &self.keys[1..] {
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
        }
        out
    }

    /// Inverse of [`IdInterner::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InternError> {
        let mut at = 0usize;
        let take = |at: &mut usize, n: usize| -> Result<&[u8], InternError> {
            let end = at
                .checked_add(n)
                .ok_or(InternError::Malformed("length overflow"))?;
            let slice = bytes
                .get(*at..end)
                .ok_or(InternError::Malformed("truncated"))?;
            *at = end;
            Ok(slice)
        };
        if take(&mut at, 5)? != Self::MAGIC {
            return Err(InternError::Malformed("bad magic"));
        }
        let epoch = u32::from_le_bytes(take(&mut at, 4)?.try_into().unwrap());
        let count = u32::from_le_bytes(take(&mut at, 4)?.try_into().unwrap()) as usize;
        if count > NODE_ID_MASK as usize {
            return Err(InternError::Malformed("count exceeds id space"));
        }
        // Every key costs at least its 4-byte length prefix, so a header that
        // declares more keys than the remaining bytes can hold is refused
        // before any allocation sized by `count` (hostile 13-byte input would
        // otherwise reserve ~1 GiB).
        if bytes.len().saturating_sub(at) < count.saturating_mul(4) {
            return Err(InternError::Malformed("count exceeds remaining bytes"));
        }
        let mut me = Self::new(epoch);
        me.keys.reserve(count);
        for _ in 0..count {
            let len = u32::from_le_bytes(take(&mut at, 4)?.try_into().unwrap()) as usize;
            let raw = take(&mut at, len)?;
            let key =
                std::str::from_utf8(raw).map_err(|_| InternError::Malformed("key not utf8"))?;
            if me.index.contains_key(key) {
                return Err(InternError::Malformed("duplicate key"));
            }
            me.intern(key)?;
        }
        if at != bytes.len() {
            return Err(InternError::Malformed("trailing bytes"));
        }
        Ok(me)
    }

    /// Rebuild the lookup index after `serde` deserialisation (which skips it).
    pub fn reindex(&mut self) {
        self.index = self
            .keys
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, k)| (k.clone(), i as u32))
            .collect();
        self.next = self.keys.len() as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_monotonic_and_stable() {
        let mut t = IdInterner::new(7);
        let a = t.intern("src/a.rs::foo").unwrap();
        let b = t.intern("src/a.rs::bar").unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(t.intern("src/a.rs::foo").unwrap(), 1);
        assert_eq!(t.key_of(2).map(|k| &**k), Some("src/a.rs::bar"));
        assert_eq!(
            t.key_of(2 | 0x8000_0000).map(|k| &**k),
            Some("src/a.rs::bar")
        );
        assert_eq!(t.key_of(0), None);
        assert_eq!(t.get("missing"), None);
    }

    #[test]
    fn round_trips_through_bytes() {
        let mut t = IdInterner::new(3);
        for i in 0..1000 {
            t.intern(&format!("k{i}/ü")).unwrap();
        }
        let back = IdInterner::from_bytes(&t.to_bytes()).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.get("k999/ü"), Some(1000));
        assert_eq!(back.epoch(), 3);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(
            IdInterner::from_bytes(b"nope").unwrap_err(),
            InternError::Malformed("truncated")
        );
        assert_eq!(
            IdInterner::from_bytes(b"NOPE\x01junk").unwrap_err(),
            InternError::Malformed("bad magic")
        );
        let mut good = IdInterner::new(1);
        good.intern("x").unwrap();
        let mut bytes = good.to_bytes();
        bytes.push(0);
        assert_eq!(
            IdInterner::from_bytes(&bytes).unwrap_err(),
            InternError::Malformed("trailing bytes")
        );
        bytes.truncate(bytes.len() - 3);
        assert_eq!(
            IdInterner::from_bytes(&bytes).unwrap_err(),
            InternError::Malformed("count exceeds remaining bytes")
        );
        // Enough bytes for the length prefix, but not for the key body itself.
        let mut two = IdInterner::new(1);
        two.intern("abcdef").unwrap();
        let mut bytes = two.to_bytes();
        bytes.truncate(bytes.len() - 2);
        assert_eq!(
            IdInterner::from_bytes(&bytes).unwrap_err(),
            InternError::Malformed("truncated")
        );
    }

    #[test]
    fn hostile_count_is_refused_before_allocation() {
        // Header only: magic, epoch, count = NODE_ID_MASK, no key bytes.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(IdInterner::MAGIC);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&NODE_ID_MASK.to_le_bytes());
        assert_eq!(
            IdInterner::from_bytes(&bytes).unwrap_err(),
            InternError::Malformed("count exceeds remaining bytes")
        );
    }

    #[test]
    fn serde_round_trip_restores_the_index() {
        // The index is #[serde(skip)]; Deserialize must rebuild it or a
        // deserialised table would hand out a second id for a known key.
        let mut t = IdInterner::new(2);
        t.intern("a").unwrap();
        t.intern("b").unwrap();
        let json = serde_json::to_string(&t).unwrap();
        let mut back: IdInterner = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.intern("a").unwrap(),
            1,
            "existing key keeps its id after serde"
        );
        assert_eq!(back.intern("c").unwrap(), 3);
    }

    #[test]
    fn exhaustion_is_an_error_not_a_wrap() {
        let mut t = IdInterner::new(1);
        t.next = NODE_ID_MASK;
        assert_eq!(t.intern("last").unwrap(), NODE_ID_MASK);
        assert_eq!(
            t.intern("one too many").unwrap_err(),
            InternError::Exhausted { epoch: 1 }
        );
    }

    #[test]
    fn next_epoch_forgets_everything() {
        let mut t = IdInterner::new(5);
        t.intern("a").unwrap();
        let n = t.next_epoch();
        assert_eq!(n.epoch(), 6);
        assert!(n.is_empty());
    }
}
