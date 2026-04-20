//! Integration tests for the F3 (Sprint 4) legacy `solid-0.1`
//! notifications adapter. Exercises the public API at the crate
//! boundary; finer codec coverage lives in the module's own `#[cfg(test)]`.
//!
//! Gated behind the `legacy-notifications` feature — matches the
//! module itself.

#![cfg(feature = "legacy-notifications")]

use std::time::Duration;

use solid_pod_rs::handlers::legacy_notifications::{LegacyWsDriver, OutboundFrame};
use solid_pod_rs::notifications::legacy::{LegacyNotificationChannel, PROTOCOL_GREETING};
use solid_pod_rs::storage::StorageEvent;
use tokio::sync::broadcast;
use tokio::time::timeout;

/// F3a: `to_legacy_line` for a Created event renders `pub <uri>`.
#[test]
fn f3a_to_legacy_line_created_is_pub_uri() {
    let ev = StorageEvent::Created("https://pod.example.com/foo.ttl".into());
    let line = LegacyNotificationChannel::to_legacy_line(&ev);
    assert_eq!(line, Some("pub https://pod.example.com/foo.ttl".to_string()));
}

/// F3b: `parse_subscribe` extracts the target URI from a valid `sub` line.
#[test]
fn f3b_parse_subscribe_valid_line() {
    let got = LegacyNotificationChannel::parse_subscribe("sub https://pod.example.com/bar/");
    assert_eq!(got, Some("https://pod.example.com/bar/".to_string()));
}

/// F3c: `parse_subscribe` returns `None` for malformed input.
#[test]
fn f3c_parse_subscribe_rejects_malformed() {
    assert!(LegacyNotificationChannel::parse_subscribe("").is_none());
    assert!(LegacyNotificationChannel::parse_subscribe("sub").is_none());
    assert!(LegacyNotificationChannel::parse_subscribe("sub   ").is_none());
    assert!(LegacyNotificationChannel::parse_subscribe("subscribe foo").is_none());
    assert!(LegacyNotificationChannel::parse_subscribe("pub https://p/x").is_none());
    assert!(LegacyNotificationChannel::parse_subscribe("unsub https://p/x").is_none());
}

/// F3d: A container subscription (URI ending in `/`) prefix-matches
/// events on any child resource.
#[test]
fn f3d_subscription_prefix_match() {
    let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
    let mut chan = LegacyNotificationChannel::new(rx);
    chan.subscribe("https://example.org/foo/".into()).unwrap();

    // Direct container match.
    assert!(chan.matches_subscription("https://example.org/foo/"));
    // Child resource underneath the container.
    assert!(chan.matches_subscription("https://example.org/foo/bar"));
    assert!(chan.matches_subscription("https://example.org/foo/bar/baz.ttl"));
    // Sibling path must NOT match.
    assert!(!chan.matches_subscription("https://example.org/foobar"));
    // Unrelated origin must NOT match.
    assert!(!chan.matches_subscription("https://other.example/foo/"));
}

/// F3d (end-to-end): a child-resource storage event is observed by a
/// container subscriber as a `pub <child-uri>` frame.
#[tokio::test]
async fn f3d_end_to_end_pub_fanout_to_container_subscriber() {
    let (tx, rx) = broadcast::channel::<StorageEvent>(16);
    let driver = LegacyWsDriver::new(rx);
    let (in_tx, mut out_rx, task) = driver.split();
    let handle = tokio::spawn(task);

    // First frame is the greeting.
    let first = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .expect("greeting within 1s")
        .expect("some greeting frame");
    assert_eq!(first, OutboundFrame::Text(PROTOCOL_GREETING.to_string()));

    // Client subscribes to the container.
    in_tx
        .send("sub https://example.org/foo/".to_string())
        .await
        .unwrap();
    let ack = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .expect("ack within 1s")
        .expect("ack frame");
    assert_eq!(
        ack,
        OutboundFrame::Text("ack https://example.org/foo/".into())
    );

    // Fan out a storage event for a child resource.
    tx.send(StorageEvent::Updated(
        "https://example.org/foo/bar.ttl".into(),
    ))
    .expect("broadcast send");

    // The driver should emit a `pub` frame for the child URI.
    let pub_frame = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .expect("pub within 1s")
        .expect("pub frame");
    assert_eq!(
        pub_frame,
        OutboundFrame::Text("pub https://example.org/foo/bar.ttl".into())
    );

    // Close by dropping the inbound sender; the driver loop exits.
    drop(in_tx);
    let _ = timeout(Duration::from_secs(2), handle).await;
}

/// F3e: the heartbeat emits an empty-text frame on the configured
/// interval. Uses a short interval (50 ms) to keep the test fast
/// without relying on a mock clock (tokio's `Instant::now()` drives
/// `tokio::time::interval` on the current runtime, so this is stable).
#[tokio::test]
async fn f3e_heartbeat_emits_blank_line_on_interval() {
    let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
    let driver = LegacyWsDriver::new(rx).with_heartbeat(Duration::from_millis(50));
    let (_in_tx, mut out_rx, task) = driver.split();
    let handle = tokio::spawn(task);

    // Discard the greeting frame.
    let first = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .expect("greeting within 1s")
        .expect("greeting");
    assert_eq!(first, OutboundFrame::Text(PROTOCOL_GREETING.to_string()));

    // The next frame (no inbound traffic, no storage events) must be
    // a heartbeat: a Text frame with an empty string.
    let hb = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .expect("heartbeat within 1s")
        .expect("heartbeat frame");
    assert_eq!(hb, OutboundFrame::Text(String::new()));

    // And another one on the next tick — confirms it's periodic, not
    // a one-off.
    let hb2 = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .expect("second heartbeat within 1s")
        .expect("second heartbeat frame");
    assert_eq!(hb2, OutboundFrame::Text(String::new()));

    handle.abort();
}

/// Unknown opcodes close the connection (matches JSS policy).
#[tokio::test]
async fn unknown_opcode_closes_connection() {
    let (_tx, rx) = broadcast::channel::<StorageEvent>(16);
    let driver = LegacyWsDriver::new(rx);
    let (in_tx, mut out_rx, task) = driver.split();
    let handle = tokio::spawn(task);

    // Discard the greeting.
    let _ = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .unwrap();

    in_tx.send("gibberish foo".to_string()).await.unwrap();
    let frame = timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .expect("close frame within 1s")
        .expect("close frame");
    assert!(matches!(frame, OutboundFrame::Close(_)));

    handle.abort();
}
