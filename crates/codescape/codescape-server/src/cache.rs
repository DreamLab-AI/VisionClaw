//! A small bounded cache for per-revision derived values (serialised snapshot
//! bodies, `initialGraphLoad` frames, projections, SSSP overlays).
//!
//! Reads take a shared lock, so concurrent clients asking for the same value do
//! not serialise. When full, the least recently *inserted* entry goes: these
//! values are keyed by graph revision, so the oldest insert is almost always
//! the stale one.
//!
//! # Example
//!
//! ```
//! use codescape_server::cache::BoundedCache;
//!
//! let c: BoundedCache<u64, &str> = BoundedCache::new(2);
//! assert_eq!(c.get_or_insert_with(1, || "one"), "one");
//! assert_eq!(c.get_or_insert_with(1, || "never built"), "one");
//! c.get_or_insert_with(2, || "two");
//! c.get_or_insert_with(3, || "three"); // evicts 1
//! assert_eq!(c.get(&1), None);
//! ```

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use parking_lot::RwLock;

/// Bounded map with insertion-order eviction.
#[derive(Debug)]
pub struct BoundedCache<K, V> {
    cap: usize,
    inner: RwLock<Entries<K, V>>,
}

/// Values with their insertion stamps, and the stamp clock.
#[derive(Debug)]
struct Entries<K, V> {
    map: HashMap<K, (V, u64)>,
    clock: u64,
}

impl<K: Eq + Hash + Clone, V: Clone> BoundedCache<K, V> {
    /// A cache holding at most `cap` entries (at least one).
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            inner: RwLock::new(Entries {
                map: HashMap::new(),
                clock: 0,
            }),
        }
    }

    /// The cached value for `key`.
    pub fn get(&self, key: &K) -> Option<V> {
        self.inner.read().map.get(key).map(|(v, _)| v.clone())
    }

    /// Store `value`, evicting the oldest insert when full.
    pub fn insert(&self, key: K, value: V) {
        let mut g = self.inner.write();
        let Entries { map, clock } = &mut *g;
        *clock += 1;
        if !map.contains_key(&key) && map.len() >= self.cap {
            if let Some(oldest) = map
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone())
            {
                map.remove(&oldest);
            }
        }
        map.insert(key, (value, *clock));
    }

    /// The cached value, or `build()` stored and returned. `build` runs without
    /// the lock held; two racing callers may both build, and one result is kept.
    pub fn get_or_insert_with(&self, key: K, build: impl FnOnce() -> V) -> V {
        if let Some(v) = self.get(&key) {
            return v;
        }
        let v = build();
        self.insert(key, v.clone());
        v
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.inner.read().map.len()
    }

    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Order-sensitive 64-bit hash of a slice of ids, for cache keys over a focus set.
pub fn ids_key(ids: &[u32]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ids.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_and_evicts_the_oldest_insert() {
        let c = BoundedCache::new(3);
        for k in 0..5u32 {
            c.insert(k, k * 10);
        }
        assert_eq!(c.len(), 3);
        assert_eq!(c.get(&0), None);
        assert_eq!(c.get(&1), None);
        assert_eq!(c.get(&4), Some(40));
        c.insert(4, 41); // replacing never evicts
        assert_eq!(c.len(), 3);
        assert_eq!(c.get(&2), Some(20));
    }

    #[test]
    fn get_or_insert_builds_once() {
        let c = BoundedCache::new(8);
        let mut builds = 0;
        for _ in 0..5 {
            c.get_or_insert_with("k", || {
                builds += 1;
                7
            });
        }
        assert_eq!(builds, 1);
        assert_ne!(ids_key(&[1, 2]), ids_key(&[2, 1]));
        assert_eq!(ids_key(&[1, 2]), ids_key(&[1, 2]));
    }
}
