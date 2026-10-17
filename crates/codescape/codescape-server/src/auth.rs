//! Authentication seams.
//!
//! * [`IngestToken`]: the `:4101` bearer token, compared in constant time with
//!   [`subtle::ConstantTimeEq`]. It fails closed: a missing or empty token file
//!   is a startup error, never an "accept everything".
//! * [`Verifier`]: the request verifier for the main port. P1–P5 run
//!   [`OpenAccess`] on an unpublished port. NIP-98 lands in P6 as another
//!   `Verifier` implementation (using the `nostr` crate, never hand-rolled
//!   Schnorr). Until it exists, `--require-auth` refuses to start rather than
//!   pretending to verify.
//! * [`RateLimiter`]: a token bucket for the ingest listener.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use subtle::ConstantTimeEq;

/// Errors from authentication.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// No credentials were presented.
    #[error("missing credentials")]
    Missing,
    /// Credentials were presented and are wrong.
    #[error("invalid credentials")]
    Invalid,
    /// Configuration cannot support the requested mode.
    #[error("auth configuration: {0}")]
    Config(String),
}

/// The shared secret guarding presence ingest.
#[derive(Clone)]
pub struct IngestToken {
    secret: Arc<[u8]>,
}

impl std::fmt::Debug for IngestToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IngestToken(<redacted>)")
    }
}

impl IngestToken {
    /// Minimum accepted secret length in bytes (the operator generates 32 random bytes, hex or base64).
    pub const MIN_LEN: usize = 16;

    /// Wrap a secret. Surrounding whitespace is trimmed; shorter than [`Self::MIN_LEN`] is refused.
    pub fn new(secret: &str) -> Result<Self, AuthError> {
        let s = secret.trim();
        if s.len() < Self::MIN_LEN {
            return Err(AuthError::Config(format!(
                "ingest token must be at least {} bytes",
                Self::MIN_LEN
            )));
        }
        Ok(Self {
            secret: Arc::from(s.as_bytes()),
        })
    }

    /// Read the token from a file.
    pub fn from_file(path: &Path) -> Result<Self, AuthError> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            AuthError::Config(format!(
                "cannot read ingest token file {}: {e}",
                path.display()
            ))
        })?;
        Self::new(&raw)
    }

    /// Check an `Authorization` header value (`Bearer <token>`).
    pub fn check_header(&self, header: Option<&str>) -> Result<(), AuthError> {
        let h = header.ok_or(AuthError::Missing)?;
        let presented = h
            .strip_prefix("Bearer ")
            .or_else(|| h.strip_prefix("bearer "))
            .ok_or(AuthError::Invalid)?
            .trim();
        // ct_eq on slices of different lengths returns 0 without early exit on content.
        if bool::from(presented.as_bytes().ct_eq(&self.secret)) {
            Ok(())
        } else {
            Err(AuthError::Invalid)
        }
    }
}

/// Who made a request, once verified.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Principal {
    /// Hex x-only pubkey when authenticated by NIP-98; `None` under open access.
    pub pubkey: Option<String>,
}

/// Verifies requests on the main port. Implementations must be cheap to call per request.
pub trait Verifier: Send + Sync {
    /// Short name for logs and `/api/health`.
    fn name(&self) -> &'static str;
    /// Verify an HTTP request from its method, absolute URL and `Authorization` header.
    fn verify_http(
        &self,
        method: &str,
        url: &str,
        authorization: Option<&str>,
    ) -> Result<Principal, AuthError>;
    /// Verify a WebSocket first-message envelope (`{"type":"authenticate", "event": …}`).
    fn verify_ws_envelope(
        &self,
        url: &str,
        envelope: &serde_json::Value,
    ) -> Result<Principal, AuthError>;
}

/// No verification: P1–P5 on an unpublished port.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAccess;

impl Verifier for OpenAccess {
    fn name(&self) -> &'static str {
        "open"
    }
    fn verify_http(&self, _: &str, _: &str, _: Option<&str>) -> Result<Principal, AuthError> {
        Ok(Principal::default())
    }
    fn verify_ws_envelope(&self, _: &str, _: &serde_json::Value) -> Result<Principal, AuthError> {
        Ok(Principal::default())
    }
}

/// The verifier for this configuration. `--require-auth` has no implementation
/// until P6, so it is a startup error instead of a silent pass-through.
pub fn build_verifier(require_auth: bool) -> Result<Arc<dyn Verifier>, AuthError> {
    if require_auth {
        return Err(AuthError::Config(
            "--require-auth needs the NIP-98 verifier, which ships in P6; refusing to start without it".into(),
        ));
    }
    Ok(Arc::new(OpenAccess))
}

/// Token-bucket rate limiter.
#[derive(Debug)]
pub struct RateLimiter {
    capacity: f64,
    per_sec: f64,
    state: Mutex<(f64, Instant)>,
}

impl RateLimiter {
    /// A bucket holding `capacity` tokens, refilled at `per_sec`.
    pub fn new(capacity: u32, per_sec: u32) -> Self {
        Self {
            capacity: capacity as f64,
            per_sec: per_sec as f64,
            state: Mutex::new((capacity as f64, Instant::now())),
        }
    }

    /// Take one token now.
    pub fn try_acquire(&self) -> bool {
        self.try_acquire_at(Instant::now())
    }

    /// Take one token at `now` (monotonic; earlier instants refill nothing).
    pub fn try_acquire_at(&self, now: Instant) -> bool {
        let mut g = self.state.lock();
        let elapsed = now.saturating_duration_since(g.1).as_secs_f64();
        g.0 = (g.0 + elapsed * self.per_sec).min(self.capacity);
        g.1 = g.1.max(now);
        if g.0 >= 1.0 {
            g.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

/// One token bucket per source address (`rate` per second, `burst` capacity).
#[derive(Debug)]
pub struct IpBuckets {
    rate: f64,
    burst: f64,
    map: Mutex<std::collections::HashMap<std::net::IpAddr, (f64, Instant)>>,
}

impl IpBuckets {
    /// Requests per second per address on `/api/*`.
    pub const API_RATE: u32 = 20;
    /// Burst per address on `/api/*`.
    pub const API_BURST: u32 = 40;
    /// Addresses tracked before refilled (idle) buckets are dropped.
    const MAX_TRACKED: usize = 4096;

    /// `rate` per second with `burst` capacity per address.
    pub fn new(rate: u32, burst: u32) -> Self {
        Self {
            rate: rate as f64,
            burst: burst.max(1) as f64,
            map: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// The `/api/*` default: 20 r/s, burst 40.
    pub fn api() -> Self {
        Self::new(Self::API_RATE, Self::API_BURST)
    }

    /// Take a token for `ip` at `now`.
    pub fn try_take_at(&self, ip: std::net::IpAddr, now: Instant) -> bool {
        let mut g = self.map.lock();
        if g.len() >= Self::MAX_TRACKED && !g.contains_key(&ip) {
            let (rate, burst) = (self.rate, self.burst);
            g.retain(|_, (t, last)| {
                *t + now.saturating_duration_since(*last).as_secs_f64() * rate < burst
            });
        }
        let (tokens, last) = g.entry(ip).or_insert((self.burst, now));
        *tokens = (*tokens + now.saturating_duration_since(*last).as_secs_f64() * self.rate)
            .min(self.burst);
        *last = (*last).max(now);
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// [`Self::try_take_at`] now.
    pub fn try_take(&self, ip: std::net::IpAddr) -> bool {
        self.try_take_at(ip, Instant::now())
    }
}

/// Sliding-window cap on new actor allocations per source address
/// (`per_window` allocations per `window`). Lookups of known labels never count.
#[derive(Debug)]
pub struct AllocLimiter {
    per_window: usize,
    window: std::time::Duration,
    seen: Mutex<std::collections::HashMap<std::net::IpAddr, std::collections::VecDeque<Instant>>>,
}

impl AllocLimiter {
    /// New allocations allowed per address per hour.
    pub const DEFAULT_PER_HOUR: usize = 64;
    /// Addresses tracked before idle ones are dropped.
    const MAX_TRACKED: usize = 4096;

    /// `per_window` allocations per `window` per address.
    pub fn new(per_window: usize, window: std::time::Duration) -> Self {
        Self {
            per_window,
            window,
            seen: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// The default: 64 per hour.
    pub fn hourly() -> Self {
        Self::new(Self::DEFAULT_PER_HOUR, std::time::Duration::from_secs(3600))
    }

    /// Take one allocation for `ip` at `now`; `false` when the window is full.
    pub fn try_take_at(&self, ip: std::net::IpAddr, now: Instant) -> bool {
        let mut g = self.seen.lock();
        if g.len() >= Self::MAX_TRACKED && !g.contains_key(&ip) {
            let window = self.window;
            g.retain(|_, q| {
                q.back()
                    .is_some_and(|t| now.saturating_duration_since(*t) < window)
            });
        }
        let q = g.entry(ip).or_default();
        while q
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= self.window)
        {
            q.pop_front();
        }
        if q.len() >= self.per_window {
            return false;
        }
        q.push_back(now);
        true
    }

    /// [`Self::try_take_at`] now.
    pub fn try_take(&self, ip: std::net::IpAddr) -> bool {
        self.try_take_at(ip, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const T: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn token_compare() {
        let t = IngestToken::new(&format!("{T}\n")).unwrap();
        assert_eq!(t.check_header(None), Err(AuthError::Missing));
        assert_eq!(t.check_header(Some("Basic abc")), Err(AuthError::Invalid));
        assert_eq!(t.check_header(Some("Bearer ")), Err(AuthError::Invalid));
        assert_eq!(
            t.check_header(Some("Bearer 0123456789abcdef0123456789abcdee")),
            Err(AuthError::Invalid)
        );
        assert_eq!(
            t.check_header(Some(&format!("Bearer {T}x"))),
            Err(AuthError::Invalid)
        );
        assert_eq!(t.check_header(Some(&format!("Bearer {T}"))), Ok(()));
        assert!(!format!("{t:?}").contains("0123"), "token never printed");
    }

    #[test]
    fn token_fails_closed() {
        assert!(IngestToken::new("").is_err());
        assert!(IngestToken::new("   \n").is_err());
        assert!(IngestToken::new("short").is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(IngestToken::from_file(&dir.path().join("missing")).is_err());
        let p = dir.path().join("tok");
        std::fs::write(&p, "").unwrap();
        assert!(IngestToken::from_file(&p).is_err());
        std::fs::write(&p, T).unwrap();
        assert!(IngestToken::from_file(&p).is_ok());
    }

    #[test]
    fn require_auth_refuses_until_p6() {
        assert!(build_verifier(true).is_err());
        let v = build_verifier(false).unwrap();
        assert_eq!(v.name(), "open");
        assert!(v.verify_http("GET", "http://x/api/health", None).is_ok());
    }

    #[test]
    fn alloc_limiter_caps_per_address_per_window() {
        let l = AllocLimiter::new(64, Duration::from_secs(3600));
        let a: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        let b: std::net::IpAddr = "10.0.0.2".parse().unwrap();
        let t0 = Instant::now();
        assert_eq!((0..100).filter(|_| l.try_take_at(a, t0)).count(), 64);
        assert!(l.try_take_at(b, t0), "addresses are independent");
        assert!(!l.try_take_at(a, t0 + Duration::from_secs(3599)));
        assert!(
            l.try_take_at(a, t0 + Duration::from_secs(3600)),
            "window slides"
        );
    }

    #[test]
    fn ip_buckets_allow_the_burst_then_the_rate() {
        let b = IpBuckets::new(20, 40);
        let a: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        let t0 = Instant::now();
        assert_eq!((0..100).filter(|_| b.try_take_at(a, t0)).count(), 40);
        assert!(
            b.try_take_at("10.0.0.9".parse().unwrap(), t0),
            "per address"
        );
        assert_eq!(
            (0..100)
                .filter(|_| b.try_take_at(a, t0 + Duration::from_millis(500)))
                .count(),
            10,
            "20/s refill"
        );
    }

    #[test]
    fn rate_limiter_refills() {
        let r = RateLimiter::new(100, 100);
        let t0 = Instant::now();
        let granted = (0..150).filter(|_| r.try_acquire_at(t0)).count();
        assert_eq!(granted, 100);
        assert!(!r.try_acquire_at(t0));
        assert!(
            r.try_acquire_at(t0 + Duration::from_millis(20)),
            "two tokens refilled after 20 ms"
        );
    }
}
