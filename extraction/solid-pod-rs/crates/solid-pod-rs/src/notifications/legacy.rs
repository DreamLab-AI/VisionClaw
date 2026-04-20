//! Legacy `solid-0.1` WebSocket notifications adapter (F3, Sprint 4).
//!
//! Bridges the existing `WebSocketChannelManager` broadcast stream to
//! SolidOS / old JSS clients which speak the pre-standardised
//! `solid-0.1` wire format. Text-framed over a WebSocket, one line
//! per frame, e.g.:
//!
//! ```text
//! protocol solid-0.1
//! sub https://pod.example.com/foo/
//! ack https://pod.example.com/foo/
//! pub https://pod.example.com/foo/bar
//! unsub https://pod.example.com/foo/
//! ```
//!
//! Reference: `JavaScriptSolidServer/src/notifications/websocket.js`.
//! Domain doc: `docs/design/jss-parity/02-notifications-compat-context.md`.
//!
//! ## Coexistence with Notifications 0.2
//!
//! Both the legacy adapter and `WebSocketChannel2023` subscribe to the
//! same upstream `StorageEvent` broadcast. A single storage event
//! produces both a JSON-LD Activity Streams 2.0 frame (modern clients)
//! and a bare `pub <uri>` line (legacy clients). Neither protocol's
//! failure affects the other.
//!
//! ## Binding to an HTTP server
//!
//! This module is transport-agnostic. A consumer mounts the handler
//! (see [`crate::handlers::legacy_notifications`]) at the path they
//! choose — typically `/ws/solid-0.1`. The adapter consumes inbound
//! `sub` / `unsub` text lines, emits outbound `ack` / `pub` / `err`
//! lines, and pings with a blank line every 30 s (matches JSS).
//!
//! The F7 library-server boundary applies: this crate never mounts
//! itself into an HTTP router. The example binders in
//! `examples/embed_in_actix.rs` show the consumer wiring.

use std::collections::HashSet;
use std::time::Duration;

use tokio::sync::broadcast::{error::RecvError, Receiver};

use crate::storage::StorageEvent;

/// Default per-connection subscription cap (matches JSS).
pub const MAX_SUBSCRIPTIONS_PER_CONNECTION: usize = 100;

/// Default target-URL cap in bytes (matches JSS `MAX_URL_LENGTH`).
pub const MAX_URL_LENGTH: usize = 2048;

/// Default heartbeat interval. JSS does not heartbeat; SolidOS data-
/// browser is happy without one, but intermediaries (nginx, Cloudflare)
/// usually idle-timeout idle WebSockets after ~60 s. Emitting a blank
/// line every 30 s keeps the connection warm without poisoning the
/// legacy parser (blank lines are ignored by SolidOS).
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Protocol greeting sent on connect.
pub const PROTOCOL_GREETING: &str = "protocol solid-0.1";

// ---------------------------------------------------------------------------
// Codec
// ---------------------------------------------------------------------------

/// One of the five `solid-0.1` opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SolidZeroOp {
    /// Client → server: subscribe to `<uri>`.
    Sub,
    /// Server → client: subscribe acknowledged for `<uri>`.
    Ack,
    /// Server → client: error for `<uri>` (e.g. WAC denied).
    Err,
    /// Server → client: resource at `<uri>` changed.
    Pub,
    /// Client → server: unsubscribe from `<uri>`.
    Unsub,
}

impl SolidZeroOp {
    /// Opcode as it appears on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            SolidZeroOp::Sub => "sub",
            SolidZeroOp::Ack => "ack",
            SolidZeroOp::Err => "err",
            SolidZeroOp::Pub => "pub",
            SolidZeroOp::Unsub => "unsub",
        }
    }
}

// ---------------------------------------------------------------------------
// LegacyNotificationChannel
// ---------------------------------------------------------------------------

/// Per-connection legacy adapter. One instance per upgraded WebSocket.
///
/// Owns the subscription set for that socket and a broadcast receiver
/// of upstream `StorageEvent`s. The aggregate is short-lived: created
/// on WS upgrade, dropped on close/error. Fan-out is lossy-by-design:
/// if the consumer's per-socket outbound queue saturates, older events
/// are dropped (matches JSS; prevents a slow client from back-
/// pressuring the storage layer).
///
/// # Example
///
/// ```ignore
/// use tokio::sync::broadcast;
/// use solid_pod_rs::notifications::legacy::LegacyNotificationChannel;
/// use solid_pod_rs::storage::StorageEvent;
///
/// let (tx, rx) = broadcast::channel::<StorageEvent>(1024);
/// let mut chan = LegacyNotificationChannel::new(rx);
///
/// // Client sent "sub https://pod.example.com/foo/":
/// if let Some(target) = LegacyNotificationChannel::parse_subscribe("sub https://pod.example.com/foo/") {
///     chan.subscribe(target);
/// }
///
/// // Upstream storage fan-out:
/// let _ = tx.send(StorageEvent::Updated("/foo/bar.ttl".into()));
///
/// // Would produce `pub https://pod.example.com/foo/bar.ttl` if the
/// // consumer normalises paths against the pod base URL; see
/// // [`LegacyNotificationChannel::matches_subscription`].
/// ```
pub struct LegacyNotificationChannel {
    storage_events: Receiver<StorageEvent>,
    subscriptions: HashSet<String>,
    url_cap_bytes: usize,
    max_subs_per_conn: usize,
    heartbeat_interval: Duration,
}

impl LegacyNotificationChannel {
    /// New channel bound to an upstream broadcast of storage events.
    pub fn new(storage_events: Receiver<StorageEvent>) -> Self {
        Self {
            storage_events,
            subscriptions: HashSet::new(),
            url_cap_bytes: MAX_URL_LENGTH,
            max_subs_per_conn: MAX_SUBSCRIPTIONS_PER_CONNECTION,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
        }
    }

    /// Override the heartbeat interval. Primarily for tests.
    pub fn with_heartbeat(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = interval;
        self
    }

    /// Override the URL length cap. Primarily for tests.
    pub fn with_url_cap(mut self, cap: usize) -> Self {
        self.url_cap_bytes = cap;
        self
    }

    /// Override the subscription cap. Primarily for tests.
    pub fn with_subscription_cap(mut self, cap: usize) -> Self {
        self.max_subs_per_conn = cap;
        self
    }

    /// Current heartbeat interval.
    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    /// Current URL length cap.
    pub fn url_cap(&self) -> usize {
        self.url_cap_bytes
    }

    /// Current subscription count.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Attempt to register a subscription for `target`. Returns `Err`
    /// (the wire-format `err` line payload) if the target violates
    /// invariants.
    pub fn subscribe(&mut self, target: String) -> Result<(), String> {
        if target.len() > self.url_cap_bytes {
            return Err(format!("err {} url-too-long", truncate(&target, 64)));
        }
        if self.subscriptions.len() >= self.max_subs_per_conn
            && !self.subscriptions.contains(&target)
        {
            return Err(format!("err {} subscription-limit", target));
        }
        self.subscriptions.insert(target);
        Ok(())
    }

    /// Remove a subscription. No-op if not present.
    pub fn unsubscribe(&mut self, target: &str) {
        self.subscriptions.remove(target);
    }

    /// True if any subscription covers the given resource URI (either
    /// exact match or prefix-match on a container URL ending in `/`).
    pub fn matches_subscription(&self, resource_uri: &str) -> bool {
        for sub in &self.subscriptions {
            if sub == resource_uri {
                return true;
            }
            if sub.ends_with('/') && resource_uri.starts_with(sub.as_str()) {
                return true;
            }
        }
        false
    }

    /// Await the next upstream storage event. Returns `None` on
    /// broadcast close. Lossy: if the receiver lagged, the skipped
    /// events are dropped rather than propagated (matches JSS).
    pub async fn next_event(&mut self) -> Option<StorageEvent> {
        loop {
            match self.storage_events.recv().await {
                Ok(ev) => return Some(ev),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    }

    // -----------------------------------------------------------------
    // Pure codec — static, no `self` state. Testable in isolation.
    // -----------------------------------------------------------------

    /// Convert a modern `StorageEvent` to a legacy wire-format line.
    ///
    /// All three event kinds map to `pub <uri>` — the legacy protocol
    /// does not distinguish Create / Update / Delete, and clients poll
    /// on `pub` to detect the new state. Returns `None` if the event
    /// cannot be expressed (currently: never; kept as `Option` so
    /// future event kinds can opt out without breaking the signature).
    ///
    /// The emitted URI is exactly the path carried by the event. If
    /// the consumer needs an absolute URL, they should map the path
    /// against their pod base URL before calling this function, or
    /// bind against a `StorageEvent` stream whose paths are already
    /// absolute. Kept as the wire-exact shape so callers are in
    /// control of URL canonicalisation.
    pub fn to_legacy_line(event: &StorageEvent) -> Option<String> {
        let uri = match event {
            StorageEvent::Created(p) | StorageEvent::Updated(p) | StorageEvent::Deleted(p) => p,
        };
        Some(format!("{} {}", SolidZeroOp::Pub.as_str(), uri))
    }

    /// Parse an inbound `sub <uri>` line. Returns the target URI with
    /// surrounding whitespace trimmed. Returns `None` for any line
    /// that does not match the `sub ` prefix followed by a non-empty
    /// target.
    pub fn parse_subscribe(line: &str) -> Option<String> {
        parse_prefixed(line, "sub ")
    }

    /// Parse an inbound `unsub <uri>` line. Returns the target URI.
    pub fn parse_unsubscribe(line: &str) -> Option<String> {
        parse_prefixed(line, "unsub ")
    }

    /// Build an `ack <uri>` line.
    pub fn ack_line(target: &str) -> String {
        format!("{} {}", SolidZeroOp::Ack.as_str(), target)
    }

    /// Build an `err <uri> <reason>` line.
    pub fn err_line(target: &str, reason: &str) -> String {
        format!("{} {} {}", SolidZeroOp::Err.as_str(), target, reason)
    }
}

/// Parse a line with `prefix` followed by a non-empty trimmed payload.
fn parse_prefixed(line: &str, prefix: &str) -> Option<String> {
    let trimmed = line.trim_end_matches(['\r', '\n']).trim_start();
    let rest = trimmed.strip_prefix(prefix)?;
    let target = rest.trim();
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

/// Truncate a string to at most `max` bytes, for safe inclusion in
/// error frames (avoids echoing multi-kilobyte hostile URLs).
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Find the largest char boundary ≤ max.
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

// ---------------------------------------------------------------------------
// Tests — unit-level codec round-trips. Integration behaviour
// (subscription fan-out, heartbeat timing against a broadcast source)
// lives in `tests/legacy_notifications_test.rs`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    fn parse_subscribe_valid() {
        let got = LegacyNotificationChannel::parse_subscribe("sub https://pod.example.com/x");
        assert_eq!(got, Some("https://pod.example.com/x".to_string()));
    }

    #[test]
    fn parse_subscribe_trims_whitespace_and_crlf() {
        let got = LegacyNotificationChannel::parse_subscribe("sub https://pod.example.com/x\r\n");
        assert_eq!(got, Some("https://pod.example.com/x".to_string()));
        let got = LegacyNotificationChannel::parse_subscribe("  sub   https://pod.example.com/x   ");
        assert_eq!(got, Some("https://pod.example.com/x".to_string()));
    }

    #[test]
    fn parse_subscribe_rejects_malformed() {
        assert!(LegacyNotificationChannel::parse_subscribe("sub").is_none());
        assert!(LegacyNotificationChannel::parse_subscribe("sub  ").is_none());
        assert!(LegacyNotificationChannel::parse_subscribe("subscribe foo").is_none());
        assert!(LegacyNotificationChannel::parse_subscribe("pub foo").is_none());
        assert!(LegacyNotificationChannel::parse_subscribe("").is_none());
    }

    #[test]
    fn parse_unsubscribe_valid() {
        let got = LegacyNotificationChannel::parse_unsubscribe("unsub https://p/x");
        assert_eq!(got, Some("https://p/x".to_string()));
    }

    #[test]
    fn to_legacy_line_created() {
        let ev = StorageEvent::Created("https://pod.example.com/x".into());
        assert_eq!(
            LegacyNotificationChannel::to_legacy_line(&ev),
            Some("pub https://pod.example.com/x".to_string())
        );
    }

    #[test]
    fn to_legacy_line_updated_and_deleted_also_map_to_pub() {
        let u = StorageEvent::Updated("https://pod.example.com/x".into());
        let d = StorageEvent::Deleted("https://pod.example.com/x".into());
        assert_eq!(
            LegacyNotificationChannel::to_legacy_line(&u),
            Some("pub https://pod.example.com/x".to_string())
        );
        assert_eq!(
            LegacyNotificationChannel::to_legacy_line(&d),
            Some("pub https://pod.example.com/x".to_string())
        );
    }

    #[test]
    fn subscription_cap_rejects_over_limit() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx).with_subscription_cap(2);
        assert!(chan.subscribe("https://p/a".into()).is_ok());
        assert!(chan.subscribe("https://p/b".into()).is_ok());
        let err = chan.subscribe("https://p/c".into()).unwrap_err();
        assert!(err.starts_with("err "));
        assert!(err.contains("subscription-limit"));
        assert_eq!(chan.subscription_count(), 2);
    }

    #[test]
    fn url_cap_rejects_over_limit() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx).with_url_cap(16);
        let err = chan
            .subscribe("https://pod.example.com/really/long/path".into())
            .unwrap_err();
        assert!(err.contains("url-too-long"));
        assert_eq!(chan.subscription_count(), 0);
    }

    #[test]
    fn matches_subscription_prefix_and_exact() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx);
        chan.subscribe("https://pod.example.com/foo/".into()).unwrap();
        chan.subscribe("https://pod.example.com/bar.ttl".into()).unwrap();
        assert!(chan.matches_subscription("https://pod.example.com/foo/"));
        assert!(chan.matches_subscription("https://pod.example.com/foo/deep/nested"));
        assert!(chan.matches_subscription("https://pod.example.com/bar.ttl"));
        assert!(!chan.matches_subscription("https://pod.example.com/other"));
        // Non-container subscription does NOT match a different path.
        assert!(!chan.matches_subscription("https://pod.example.com/bar.ttl.backup"));
    }

    #[test]
    fn unsubscribe_removes_target() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx);
        chan.subscribe("https://p/x".into()).unwrap();
        chan.unsubscribe("https://p/x");
        assert_eq!(chan.subscription_count(), 0);
        chan.unsubscribe("https://p/y"); // no-op
    }

    #[test]
    fn ack_and_err_lines() {
        assert_eq!(
            LegacyNotificationChannel::ack_line("https://p/x"),
            "ack https://p/x"
        );
        assert_eq!(
            LegacyNotificationChannel::err_line("https://p/x", "forbidden"),
            "err https://p/x forbidden"
        );
    }

    #[test]
    fn opcode_wire_names() {
        assert_eq!(SolidZeroOp::Sub.as_str(), "sub");
        assert_eq!(SolidZeroOp::Ack.as_str(), "ack");
        assert_eq!(SolidZeroOp::Err.as_str(), "err");
        assert_eq!(SolidZeroOp::Pub.as_str(), "pub");
        assert_eq!(SolidZeroOp::Unsub.as_str(), "unsub");
    }
}
