//! DPoP `jti` replay cache (F5, Sprint 4).
//!
//! Per Solid-OIDC §5.2 and RFC 9449 §4.3, a relying party MUST reject
//! DPoP proofs whose `jti` has already been observed within the allowed
//! iat-skew window. This module exposes [`DpopReplayCache`] — a
//! bounded LRU over `(jti, first_seen)` tuples, shared across all
//! requests in a single process.
//!
//! Gated behind the `dpop-replay-cache` feature. When the feature is
//! off, the module is absent entirely and
//! [`super::verify_dpop_proof`] remains replay-cache-unaware
//! (pre-F5 behaviour).
//!
//! # Invariants
//!
//! 1. A `jti` first-seen within TTL is accepted exactly once.
//! 2. A `jti` re-submitted within TTL returns [`ReplayError::Replayed`]
//!    and does NOT refresh the entry (i.e. LRU position is untouched
//!    on replay).
//! 3. A `jti` re-submitted strictly after TTL is treated as a fresh
//!    first-seen — entry is re-inserted.
//! 4. Cache is bounded by `max_size`; the oldest entry is evicted on
//!    overflow. This opens a **capacity-driven replay window**; size
//!    the cache for worst-case rate (10_000 default ≈ 166 rps at 60s
//!    TTL).
//! 5. All operations are `async` and use a single `tokio::sync::Mutex`;
//!    the critical section is panic-free-by-construction (no indexing,
//!    no unwrap, checked arithmetic only).
//!
//! # Limitations
//!
//! The cache is process-local. Multi-replica deployments share no
//! state; operators running HA should either (a) stick
//! DPoP-authenticated sessions to a single replica or (b) reduce TTL
//! to an acceptable replay window. A Redis-backed cache is out of
//! scope for the 0.4.0 Sprint 4 deliverable.

#![cfg(feature = "dpop-replay-cache")]

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lru::LruCache;
use thiserror::Error;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Configuration constants
// ---------------------------------------------------------------------------

/// Default TTL for a remembered `jti`: 60 seconds, matching the
/// Solid-OIDC §5.2 recommended iat-skew window and JSS's
/// `src/auth/solid-oidc.js` default.
pub const DEFAULT_TTL_SECS: u64 = 60;

/// Default capacity: 10_000 entries. At ≈150 bytes/entry (string jti
/// + Instant) this bounds worst-case memory to ~1.5 MB.
pub const DEFAULT_MAX_SIZE: usize = 10_000;

/// Environment variable names consumed by [`DpopReplayCache::from_env`].
pub const ENV_TTL_SECS: &str = "SOLID_POD_DPOP_REPLAY_TTL_SECS";
pub const ENV_MAX_SIZE: &str = "SOLID_POD_DPOP_REPLAY_MAX_SIZE";

// ---------------------------------------------------------------------------
// ReplayError
// ---------------------------------------------------------------------------

/// Error returned by [`DpopReplayCache::check_and_record`].
#[derive(Debug, Error)]
pub enum ReplayError {
    /// The `jti` was already recorded within the TTL window. This is a
    /// P1 security event — a client presented the same DPoP proof
    /// twice.
    #[error("DPoP jti already used within TTL window ({ttl:?})")]
    Replayed { ttl: Duration },
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Bounded LRU tracking recently-seen DPoP `jti` values.
///
/// Cheap to `Clone` — the underlying state is behind an `Arc<Mutex<…>>`
/// so clones share storage. Typical usage: construct once at startup,
/// hand a clone to each request handler.
#[derive(Debug, Clone)]
pub struct DpopReplayCache {
    inner: Arc<Mutex<LruCacheInner>>,
    ttl: Duration,
    max_size: usize,
}

#[derive(Debug)]
struct LruCacheInner {
    /// `jti` → first-seen `Instant`. On check, entries whose first-seen
    /// is older than `ttl` are considered expired and replaced.
    entries: LruCache<String, Instant>,
}

impl DpopReplayCache {
    /// Construct with defaults (TTL 60s, max_size 10_000), optionally
    /// overridden by environment variables
    /// `SOLID_POD_DPOP_REPLAY_TTL_SECS` / `SOLID_POD_DPOP_REPLAY_MAX_SIZE`.
    pub fn from_env() -> Self {
        let ttl_secs = std::env::var(ENV_TTL_SECS)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TTL_SECS);
        let max_size = std::env::var(ENV_MAX_SIZE)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_SIZE);
        Self::with_config(Duration::from_secs(ttl_secs), max_size)
    }

    /// Construct with an explicit TTL and maximum entry count.
    ///
    /// `max_size` is clamped to at least 1; a zero-capacity LRU is
    /// meaningless for replay detection.
    pub fn with_config(ttl: Duration, max_size: usize) -> Self {
        let cap = NonZeroUsize::new(max_size.max(1))
            .expect("max_size clamped to >= 1 above");
        Self {
            inner: Arc::new(Mutex::new(LruCacheInner {
                entries: LruCache::new(cap),
            })),
            ttl,
            max_size: max_size.max(1),
        }
    }

    /// Configured TTL.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Configured maximum entry count.
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Current number of entries in the cache (approximate; reads
    /// hold the mutex briefly).
    pub async fn len(&self) -> usize {
        self.inner.lock().await.entries.len()
    }

    /// `true` when no entries are tracked.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Check whether `jti` has been seen within TTL; if not, record
    /// it and return `Ok(())`. If already seen within TTL, return
    /// [`ReplayError::Replayed`] without refreshing the entry.
    ///
    /// Entries whose first-seen is strictly older than `ttl` are
    /// treated as expired and overwritten.
    pub async fn check_and_record(&self, jti: &str) -> Result<(), ReplayError> {
        let now = Instant::now();
        let mut guard = self.inner.lock().await;

        // `peek` does not promote the entry — we want LRU order to
        // reflect insertion age, not every-check age.
        if let Some(first_seen) = guard.entries.peek(jti).copied() {
            let age = now.saturating_duration_since(first_seen);
            if age < self.ttl {
                // Still within the replay window.
                return Err(ReplayError::Replayed { ttl: self.ttl });
            }
            // Expired: treat as fresh insertion (overwrites).
        }
        // First-seen (or expired) — insert. `put` automatically evicts
        // the LRU entry when at capacity.
        guard.entries.put(jti.to_string(), now);
        Ok(())
    }

    /// Evict all entries whose first-seen is strictly older than the
    /// configured TTL. Returns the number of entries removed.
    ///
    /// Eviction is lazy (driven by `check_and_record`), so this is
    /// optional; call periodically (e.g. every 30s) to bound idle
    /// memory in long-lived servers.
    pub async fn evict_expired(&self) -> usize {
        let now = Instant::now();
        let mut guard = self.inner.lock().await;

        let expired: Vec<String> = guard
            .entries
            .iter()
            .filter_map(|(jti, seen)| {
                if now.saturating_duration_since(*seen) >= self.ttl {
                    Some(jti.clone())
                } else {
                    None
                }
            })
            .collect();

        let removed = expired.len();
        for jti in expired {
            guard.entries.pop(&jti);
        }
        removed
    }

    /// Spawn a background task that calls [`Self::evict_expired`] on
    /// the given period. Useful for long-running servers where lazy
    /// eviction alone would let the cache stay near capacity
    /// indefinitely.
    ///
    /// The returned [`tokio::task::JoinHandle`] can be aborted to
    /// stop the evictor; the cache itself is unaffected.
    pub fn spawn_evictor(self, period: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(period);
            // Skip the immediate first tick — no entries yet.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let _ = self.evict_expired().await;
            }
        })
    }
}

impl Default for DpopReplayCache {
    fn default() -> Self {
        Self::with_config(Duration::from_secs(DEFAULT_TTL_SECS), DEFAULT_MAX_SIZE)
    }
}

// ---------------------------------------------------------------------------
// Metric (F5 PRD §F5.metrics): rejection counter.
// ---------------------------------------------------------------------------

/// Monotonic counter of replay rejections. Framework-agnostic —
/// expose via your metrics sink of choice (Prometheus, OpenTelemetry,
/// ...). Typical usage:
///
/// ```ignore
/// if let Err(ReplayError::Replayed { .. }) =
///     replay_cache.check_and_record(&jti).await
/// {
///     DPOP_REPLAY_REJECTED_TOTAL.increment();
///     return Err(PodError::Nip98("DPoP jti replay detected".into()));
/// }
/// ```
#[derive(Debug, Default)]
pub struct ReplayRejectedCounter {
    value: std::sync::atomic::AtomicU64,
}

impl ReplayRejectedCounter {
    /// Construct a zeroed counter.
    pub const fn new() -> Self {
        Self {
            value: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Increment by one. `Relaxed` ordering is sufficient — the
    /// counter is observational, never used for synchronisation.
    pub fn increment(&self) {
        self.value
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Current value.
    pub fn get(&self) -> u64 {
        self.value.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Process-wide replay-rejection counter. Consumers reading this
/// should expose it via their metrics endpoint as
/// `solid_pod_rs_dpop_replay_rejected_total`.
pub static DPOP_REPLAY_REJECTED_TOTAL: ReplayRejectedCounter =
    ReplayRejectedCounter::new();

// ---------------------------------------------------------------------------
// Unit tests (module-local; integration coverage lives in
// `tests/dpop_replay_test.rs`).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_seen_jti_is_accepted() {
        let cache = DpopReplayCache::with_config(Duration::from_secs(60), 16);
        assert!(cache.check_and_record("jti-1").await.is_ok());
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn replay_within_ttl_is_rejected() {
        let cache = DpopReplayCache::with_config(Duration::from_secs(60), 16);
        cache.check_and_record("jti-1").await.unwrap();
        let err = cache.check_and_record("jti-1").await.unwrap_err();
        assert!(matches!(err, ReplayError::Replayed { .. }));
        // Entry count stays at 1 — replay does not insert.
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn default_config_matches_constants() {
        let cache = DpopReplayCache::default();
        assert_eq!(cache.ttl(), Duration::from_secs(DEFAULT_TTL_SECS));
        assert_eq!(cache.max_size(), DEFAULT_MAX_SIZE);
    }

    #[tokio::test]
    async fn max_size_clamped_to_at_least_one() {
        let cache = DpopReplayCache::with_config(Duration::from_secs(1), 0);
        assert_eq!(cache.max_size(), 1);
    }

    #[test]
    fn counter_increments() {
        let c = ReplayRejectedCounter::new();
        assert_eq!(c.get(), 0);
        c.increment();
        c.increment();
        assert_eq!(c.get(), 2);
    }
}
