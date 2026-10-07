//! Server-side heartbeat for `/wss` sockets.
//!
//! Driven by `system.websocket.heartbeatInterval` / `heartbeatTimeout`
//! (milliseconds, `settings.yaml`, read once per connection at upgrade). The
//! socket pings the peer every `interval` and closes it once no inbound frame
//! (pong, ping, text or binary) has arrived for `timeout`. Browsers answer
//! pings from the network stack, not the page's main thread, so a busy tab
//! still counts as alive; only a peer that has gone away is closed, which
//! fires `stopped()` and unregisters it from the broadcast registry.
//!
//! Only *inbound* frames refresh liveness. Before this module the ping timer
//! stamped `last_activity` when it *sent* a ping, so no socket could ever
//! time out.

use std::time::{Duration, Instant};

use crate::config::WebSocketSettings;

/// Validated heartbeat timings for one socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatConfig {
    /// Time between server pings.
    pub interval: Duration,
    /// Inbound silence after which the socket is closed.
    pub timeout: Duration,
}

impl HeartbeatConfig {
    /// Shortest ping interval accepted; anything lower is raised to it.
    pub const MIN_INTERVAL: Duration = Duration::from_secs(1);

    /// Build from settings, raising an interval below [`Self::MIN_INTERVAL`]
    /// and a timeout below two intervals (one missed pong must not close a
    /// healthy socket).
    pub fn from_settings(ws: &WebSocketSettings) -> Self {
        let interval = Duration::from_millis(ws.heartbeat_interval).max(Self::MIN_INTERVAL);
        let timeout = Duration::from_millis(ws.heartbeat_timeout).max(interval * 2);
        Self { interval, timeout }
    }
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self::from_settings(&WebSocketSettings::default())
    }
}

/// What the heartbeat timer should do on a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatAction {
    /// The peer is alive: send the next ping.
    Ping,
    /// No inbound frame for `timeout`: close the socket.
    Close,
}

/// Per-socket liveness tracker.
#[derive(Debug, Clone)]
pub struct Heartbeat {
    config: HeartbeatConfig,
    last_inbound: Instant,
}

impl Heartbeat {
    /// A tracker whose peer counts as heard from at `now`.
    pub fn new(config: HeartbeatConfig, now: Instant) -> Self {
        Self {
            config,
            last_inbound: now,
        }
    }

    /// The timings in force.
    pub fn config(&self) -> HeartbeatConfig {
        self.config
    }

    /// Record an inbound frame from the peer.
    pub fn inbound(&mut self, now: Instant) {
        self.last_inbound = now;
    }

    /// Decide the timer tick at `now`. Never refreshes liveness itself.
    pub fn tick(&self, now: Instant) -> HeartbeatAction {
        if now.saturating_duration_since(self.last_inbound) >= self.config.timeout {
            HeartbeatAction::Close
        } else {
            HeartbeatAction::Ping
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(interval: u64, timeout: u64) -> WebSocketSettings {
        WebSocketSettings {
            heartbeat_interval: interval,
            heartbeat_timeout: timeout,
        }
    }

    #[test]
    fn config_carries_the_settings() {
        let c = HeartbeatConfig::from_settings(&ws(10_000, 60_000));
        assert_eq!(c.interval, Duration::from_secs(10));
        assert_eq!(c.timeout, Duration::from_secs(60));
        assert_eq!(
            HeartbeatConfig::default(),
            c,
            "defaults: 10 s ping, 60 s timeout"
        );
    }

    /// The coordinator's fallback stall timeout is the heartbeat default, and
    /// the shipped settings.yaml carries the same values as the Rust default.
    #[test]
    fn defaults_agree_across_code_and_settings_yaml() {
        assert_eq!(
            crate::actors::messages::client_messages::DEFAULT_STALL_TIMEOUT,
            HeartbeatConfig::default().timeout
        );
        let yaml: serde_yaml::Value =
            serde_yaml::from_str(include_str!("../../../data/settings.yaml")).unwrap();
        let ws_yaml = &yaml["system"]["websocket"];
        let from_yaml: WebSocketSettings = serde_yaml::from_value(ws_yaml.clone()).unwrap();
        let d = WebSocketSettings::default();
        assert_eq!(
            (from_yaml.heartbeat_interval, from_yaml.heartbeat_timeout),
            (d.heartbeat_interval, d.heartbeat_timeout)
        );
    }

    #[test]
    fn config_raises_unsafe_values() {
        let c = HeartbeatConfig::from_settings(&ws(10, 50));
        assert_eq!(c.interval, HeartbeatConfig::MIN_INTERVAL);
        assert_eq!(c.timeout, HeartbeatConfig::MIN_INTERVAL * 2);
        let c = HeartbeatConfig::from_settings(&ws(5_000, 6_000));
        assert_eq!(
            c.timeout,
            Duration::from_secs(10),
            "timeout >= two intervals"
        );
    }

    /// The defect this replaces: sending a ping must not count as hearing
    /// from the peer, so a silent peer is closed once the timeout passes.
    #[test]
    fn a_silent_peer_is_closed_after_the_timeout() {
        let t0 = Instant::now();
        let hb = Heartbeat::new(HeartbeatConfig::from_settings(&ws(1_000, 3_000)), t0);
        assert_eq!(hb.tick(t0 + Duration::from_secs(1)), HeartbeatAction::Ping);
        assert_eq!(hb.tick(t0 + Duration::from_secs(2)), HeartbeatAction::Ping);
        assert_eq!(hb.tick(t0 + Duration::from_secs(3)), HeartbeatAction::Close);
    }

    #[test]
    fn inbound_frames_keep_the_peer_alive() {
        let t0 = Instant::now();
        let mut hb = Heartbeat::new(HeartbeatConfig::from_settings(&ws(1_000, 3_000)), t0);
        for s in 1..=10u64 {
            hb.inbound(t0 + Duration::from_secs(s));
            assert_eq!(
                hb.tick(t0 + Duration::from_secs(s) + Duration::from_millis(500)),
                HeartbeatAction::Ping
            );
        }
        assert_eq!(
            hb.tick(t0 + Duration::from_secs(13)),
            HeartbeatAction::Close
        );
    }
}
