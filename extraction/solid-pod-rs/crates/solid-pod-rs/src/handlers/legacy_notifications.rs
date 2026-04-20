//! Transport-agnostic driver for the legacy `solid-0.1` WebSocket
//! notifications adapter (F3, Sprint 4).
//!
//! # Library-server boundary (F7)
//!
//! This crate does **not** mount itself at `/ws/solid-0.1`. The
//! consumer is responsible for:
//!
//! 1. Performing the HTTP → WebSocket upgrade in their framework
//!    (actix-ws, axum::extract::ws, tokio-tungstenite, …).
//! 2. Forwarding inbound text frames to [`LegacyWsDriver::handle_line`].
//! 3. Forwarding outbound frames from the driver's `outbound`
//!    receiver to the WebSocket.
//! 4. Calling [`LegacyWsDriver::run`] as a background task for the
//!    connection's lifetime.
//!
//! Recommended mount path: `/ws/solid-0.1` or
//! `/.well-known/solid/notifications-legacy`. SolidOS mashlib defaults
//! to the former; match JSS for drop-in compat.
//!
//! # Example (actix-web + actix-ws)
//!
//! ```ignore
//! use actix_web::{get, web, HttpRequest, HttpResponse, Error};
//! use tokio::sync::broadcast;
//! use solid_pod_rs::handlers::legacy_notifications::LegacyWsDriver;
//! use solid_pod_rs::storage::StorageEvent;
//!
//! #[get("/ws/solid-0.1")]
//! async fn ws_legacy(
//!     req: HttpRequest,
//!     body: web::Payload,
//!     events: web::Data<broadcast::Sender<StorageEvent>>,
//! ) -> Result<HttpResponse, Error> {
//!     let (resp, mut session, mut msg_stream) = actix_ws::handle(&req, body)?;
//!     let driver = LegacyWsDriver::new(events.subscribe());
//!     actix_web::rt::spawn(async move { driver.run(session, msg_stream).await });
//!     Ok(resp)
//! }
//! ```
//!
//! The [`LegacyWsDriver::run`] adapter above is a convenience that
//! works with any transport exposing the `LegacySocket` trait; see
//! this module's trait definition.
//!
//! # Coexistence
//!
//! The legacy driver shares the upstream `broadcast::Sender<StorageEvent>`
//! with the existing `WebSocketChannel2023` and `WebhookChannel2023`
//! managers. Mount all three against the same sender to give modern
//! and legacy clients simultaneous live fan-out from a single storage
//! event source.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::notifications::legacy::{LegacyNotificationChannel, PROTOCOL_GREETING};
use crate::storage::StorageEvent;
use tokio::sync::broadcast::Receiver;

/// Outbound frame destined for the WebSocket client.
///
/// `Text` carries a wire-format line (`ack …`, `pub …`, `err …`, or
/// the empty string used as a heartbeat). `Close` asks the transport
/// to terminate the connection cleanly, typically after an
/// unrecoverable protocol error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundFrame {
    /// A plain-text frame to send as-is.
    Text(String),
    /// Close the WebSocket. The payload is a short human-readable
    /// reason for logging.
    Close(String),
}

/// Driver for a single legacy WebSocket connection.
///
/// Feeds the adapter three inputs — upstream storage events, inbound
/// text frames from the client, and a heartbeat tick — and emits
/// outbound text frames on an mpsc channel the transport consumes.
pub struct LegacyWsDriver {
    channel: LegacyNotificationChannel,
    outbound_cap: usize,
}

impl LegacyWsDriver {
    /// New driver bound to an upstream storage event broadcast
    /// receiver. Uses default caps and a 30 s heartbeat.
    pub fn new(events: Receiver<StorageEvent>) -> Self {
        Self {
            channel: LegacyNotificationChannel::new(events),
            outbound_cap: 256,
        }
    }

    /// Override the outbound mpsc capacity. Lower caps mean earlier
    /// frame drops under back-pressure (lossy-by-design, matches JSS).
    pub fn with_outbound_capacity(mut self, cap: usize) -> Self {
        self.outbound_cap = cap;
        self
    }

    /// Override the heartbeat interval (default 30 s).
    pub fn with_heartbeat(mut self, interval: Duration) -> Self {
        self.channel = self.channel.with_heartbeat(interval);
        self
    }

    /// Split the driver into its three transport-facing parts:
    ///
    /// - `inbound_tx`: the transport `.send()`s each received text
    ///   frame here.
    /// - `outbound_rx`: the transport forwards each emitted
    ///   [`OutboundFrame`] to the client.
    /// - `task`: a future the transport spawns for the life of the
    ///   connection. When it resolves, the connection has ended and
    ///   the transport should close the socket.
    pub fn split(
        self,
    ) -> (
        mpsc::Sender<String>,
        mpsc::Receiver<OutboundFrame>,
        impl std::future::Future<Output = ()> + Send,
    ) {
        let (in_tx, in_rx) = mpsc::channel::<String>(64);
        let (out_tx, out_rx) = mpsc::channel::<OutboundFrame>(self.outbound_cap);
        let fut = run_loop(self.channel, in_rx, out_tx);
        (in_tx, out_rx, fut)
    }
}

async fn run_loop(
    mut chan: LegacyNotificationChannel,
    mut inbound: mpsc::Receiver<String>,
    outbound: mpsc::Sender<OutboundFrame>,
) {
    // Greeting first — JSS sends `protocol solid-0.1` immediately on
    // connect so the client knows which dialect to speak.
    if outbound
        .send(OutboundFrame::Text(PROTOCOL_GREETING.to_string()))
        .await
        .is_err()
    {
        return;
    }

    let heartbeat = chan.heartbeat_interval();
    let mut ticker = tokio::time::interval(heartbeat);
    // The first tick fires immediately; discard it so the heartbeat
    // lines up with the advertised interval.
    ticker.tick().await;

    loop {
        tokio::select! {
            // Inbound: client → server text frames.
            maybe_line = inbound.recv() => {
                let Some(line) = maybe_line else {
                    // Transport closed; drop channel & exit.
                    return;
                };
                for frame in handle_line(&mut chan, &line) {
                    if outbound.send(frame).await.is_err() {
                        return;
                    }
                }
            }
            // Upstream: storage events → `pub` lines for matching
            // subscribers. `None` means the broadcast source closed.
            maybe_event = chan.next_event() => {
                let Some(event) = maybe_event else { return; };
                let uri = match &event {
                    StorageEvent::Created(p)
                    | StorageEvent::Updated(p)
                    | StorageEvent::Deleted(p) => p.clone(),
                };
                if chan.matches_subscription(&uri) {
                    if let Some(line) = LegacyNotificationChannel::to_legacy_line(&event) {
                        // try_send so a slow client does not back-pressure
                        // the broadcast. Dropped frames are logged upstream
                        // via tracing.
                        if outbound.try_send(OutboundFrame::Text(line)).is_err() {
                            tracing::warn!(
                                target: "solid_pod_rs::legacy_notifications",
                                "outbound queue saturated, dropping pub frame"
                            );
                        }
                    }
                }
            }
            // Heartbeat: blank line to defeat idle-timeout intermediaries.
            _ = ticker.tick() => {
                if outbound
                    .send(OutboundFrame::Text(String::new()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

/// Stateless helper: interpret one inbound text line and produce the
/// outbound frames it elicits. Public for unit testing.
pub fn handle_line(chan: &mut LegacyNotificationChannel, line: &str) -> Vec<OutboundFrame> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new(); // treat blank inbound as client heartbeat; ignore
    }
    if let Some(target) = LegacyNotificationChannel::parse_subscribe(trimmed) {
        match chan.subscribe(target.clone()) {
            Ok(()) => {
                return vec![OutboundFrame::Text(LegacyNotificationChannel::ack_line(
                    &target,
                ))];
            }
            Err(err_line) => {
                return vec![OutboundFrame::Text(err_line)];
            }
        }
    }
    if let Some(target) = LegacyNotificationChannel::parse_unsubscribe(trimmed) {
        chan.unsubscribe(&target);
        return Vec::new();
    }
    // Unknown opcode: JSS policy is to close the connection.
    vec![OutboundFrame::Close(format!(
        "unknown opcode: {}",
        first_token(trimmed)
    ))]
}

fn first_token(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    fn handle_line_sub_emits_ack() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx);
        let frames = handle_line(&mut chan, "sub https://p/x");
        assert_eq!(frames, vec![OutboundFrame::Text("ack https://p/x".into())]);
        assert_eq!(chan.subscription_count(), 1);
    }

    #[test]
    fn handle_line_unsub_is_silent() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx);
        chan.subscribe("https://p/x".into()).unwrap();
        let frames = handle_line(&mut chan, "unsub https://p/x");
        assert!(frames.is_empty());
        assert_eq!(chan.subscription_count(), 0);
    }

    #[test]
    fn handle_line_unknown_opcode_closes() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx);
        let frames = handle_line(&mut chan, "wat foo");
        assert_eq!(frames.len(), 1);
        assert!(matches!(frames[0], OutboundFrame::Close(_)));
    }

    #[test]
    fn handle_line_blank_is_noop() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx);
        assert!(handle_line(&mut chan, "").is_empty());
        assert!(handle_line(&mut chan, "   ").is_empty());
    }

    #[test]
    fn handle_line_sub_over_cap_emits_err() {
        let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
        let mut chan = LegacyNotificationChannel::new(rx).with_subscription_cap(1);
        let _ = handle_line(&mut chan, "sub https://p/a");
        let frames = handle_line(&mut chan, "sub https://p/b");
        assert_eq!(frames.len(), 1);
        match &frames[0] {
            OutboundFrame::Text(t) => {
                assert!(t.starts_with("err "));
                assert!(t.contains("subscription-limit"));
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }
}
