//! Minimal metrics sink for security primitives.
//!
//! A zero-dependency, lock-free counter bundle for the Sprint 4 F1/F2
//! security aggregates. Prometheus export is intentionally out of
//! scope: the upstream binder crate (`webxr`) already runs a
//! Prometheus registry and can lift these atomics into gauges when it
//! wires the primitives in Sprint 4 / F7.
//!
//! The struct is `Clone` (cheap — a handful of `Arc<AtomicU64>`), so
//! a single instance can be cloned into both `SsrfPolicy` and
//! `DotfileAllowlist` via their `with_metrics(_)` builders and also
//! retained by the operator for scraping.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::security::ssrf::IpClass;

/// Atomic counter bundle, cheap to clone.
#[derive(Debug, Default, Clone)]
pub struct SecurityMetrics {
    inner: Arc<SecurityMetricsInner>,
}

#[derive(Debug, Default)]
struct SecurityMetricsInner {
    // SSRF block counters, labelled by IpClass.
    ssrf_blocked_private: AtomicU64,
    ssrf_blocked_loopback: AtomicU64,
    ssrf_blocked_link_local: AtomicU64,
    ssrf_blocked_multicast: AtomicU64,
    ssrf_blocked_reserved: AtomicU64,
    // `Public` is never blocked under the default classifier, but
    // callers that carry a denylist hit count it under `Reserved`
    // (denylist is operator-explicit intent).

    // Dotfile deny counter.
    dotfile_denied: AtomicU64,
}

impl SecurityMetrics {
    /// Construct a fresh counter bundle. All counters start at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the SSRF block counter for `class`.
    pub fn record_ssrf_block(&self, class: IpClass) {
        let counter = match class {
            IpClass::Private => &self.inner.ssrf_blocked_private,
            IpClass::Loopback => &self.inner.ssrf_blocked_loopback,
            IpClass::LinkLocal => &self.inner.ssrf_blocked_link_local,
            IpClass::Multicast => &self.inner.ssrf_blocked_multicast,
            IpClass::Reserved | IpClass::Public => &self.inner.ssrf_blocked_reserved,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Read the SSRF block counter for `class`.
    pub fn ssrf_blocked_total(&self, class: IpClass) -> u64 {
        let counter = match class {
            IpClass::Private => &self.inner.ssrf_blocked_private,
            IpClass::Loopback => &self.inner.ssrf_blocked_loopback,
            IpClass::LinkLocal => &self.inner.ssrf_blocked_link_local,
            IpClass::Multicast => &self.inner.ssrf_blocked_multicast,
            IpClass::Reserved | IpClass::Public => &self.inner.ssrf_blocked_reserved,
        };
        counter.load(Ordering::Relaxed)
    }

    /// Increment the dotfile-deny counter.
    pub fn record_dotfile_deny(&self) {
        self.inner.dotfile_denied.fetch_add(1, Ordering::Relaxed);
    }

    /// Read the dotfile-deny counter.
    pub fn dotfile_denied_total(&self) -> u64 {
        self.inner.dotfile_denied.load(Ordering::Relaxed)
    }
}
