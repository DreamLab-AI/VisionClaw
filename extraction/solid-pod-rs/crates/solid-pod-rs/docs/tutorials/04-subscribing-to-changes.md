# Tutorial 4 — Subscribing to changes

**Goal:** stand up a `WebSocketChannelManager`, connect a client,
write to the pod, and see live change notifications in Activity
Streams 2.0 format. ≤ 15 minutes.

## Prerequisites

- Rust toolchain + the `solid-pod-rs` workspace.
- Familiarity with `tokio` and `async fn`.

The stock example server (`examples/standalone.rs`) does not wire up
Notifications — this tutorial adds a small program to exercise the
notifications module directly.

## Step 1 — Understand the wiring

Notifications ride on top of three primitives:

1. `Storage::watch(path)` returns a `tokio::sync::mpsc::Receiver<StorageEvent>`.
2. `WebSocketChannelManager::pump_from_storage(rx, pod_base)` converts
   each `StorageEvent` into an Activity Streams `ChangeNotification`.
3. Consumers call `.stream()` on the manager to get a
   `broadcast::Receiver<ChangeNotification>` and forward each frame to
   their WebSocket clients.

All three pieces live in the library. You wire them together.

## Step 2 — A standalone subscriber

Create `examples/subscribe.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use solid_pod_rs::notifications::WebSocketChannelManager;
use solid_pod_rs::storage::{memory::MemoryBackend, Storage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Storage with a watcher.
    let storage: Arc<dyn Storage> = Arc::new(MemoryBackend::new());
    let rx = storage.watch("/").await?;

    // 2. Notifications manager.
    let manager = WebSocketChannelManager::new();
    let mut stream = manager.stream();

    // 3. Pump storage events into the manager in the background.
    let pump_handle = tokio::spawn(manager.clone().pump_from_storage(
        rx,
        "https://pod.example".to_string(),
    ));

    // 4. Simulate activity.
    let s = storage.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        s.put("/notes/a.txt", Bytes::from_static(b"hi"), "text/plain")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        s.put("/notes/a.txt", Bytes::from_static(b"hi again"), "text/plain")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        s.delete("/notes/a.txt").await.unwrap();
    });

    // 5. Consume change notifications.
    for _ in 0..3 {
        let note = stream.recv().await?;
        println!(
            "{} -> {} @ {}",
            note.kind, note.object, note.published
        );
    }

    pump_handle.abort();
    Ok(())
}
```

## Step 3 — Run it

```bash
cargo run --example subscribe -p solid-pod-rs
```

Expected output (timestamps will differ):

```text
Create -> https://pod.example/notes/a.txt @ 2026-04-20T12:00:00.123Z
Update -> https://pod.example/notes/a.txt @ 2026-04-20T12:00:00.223Z
Delete -> https://pod.example/notes/a.txt @ 2026-04-20T12:00:00.323Z
```

The `kind` values match the Activity Streams 2.0 activity types. The
`object` is a fully qualified IRI using the `pod_base` you passed to
`pump_from_storage`.

## Step 4 — Inspect the JSON-LD shape

The payload the WebSocket writes is:

```json
{
  "@context": "https://www.w3.org/ns/activitystreams",
  "id": "urn:uuid:9a3f...",
  "type": "Create",
  "object": "https://pod.example/notes/a.txt",
  "published": "2026-04-20T12:00:00.123Z"
}
```

This is the [Solid Notifications Protocol
§7](https://solid.github.io/notifications/protocol/) content format.
Subscribers consume AS2.0 notifications; the server does not need to
ship any other shape.

## Step 5 — Add a webhook sibling (optional)

The same storage event stream can feed a
`WebhookChannelManager`. Because `Storage::watch` only gives you one
receiver per call, make two `.watch()` calls on the same storage
handle:

```rust
let rx_ws   = storage.watch("/").await?;
let rx_hook = storage.watch("/").await?;

let ws_mgr   = WebSocketChannelManager::new();
let hook_mgr = WebhookChannelManager::new();

tokio::spawn(ws_mgr.clone().pump_from_storage(rx_ws, pod_base.clone()));
tokio::spawn(hook_mgr.clone().pump_from_storage(rx_hook, pod_base));
```

See [how-to/enable-notifications-webhook.md](../how-to/enable-notifications-webhook.md)
and [how-to/enable-notifications-websocket.md](../how-to/enable-notifications-websocket.md).

## Step 6 — Expose the discovery document

Clients find your notification endpoints by fetching
`/.notifications`. Wire up a handler:

```rust
use solid_pod_rs::notifications::discovery_document;
let body = discovery_document("https://pod.example");
// Serve `body` at `/.notifications` with content-type `application/ld+json`.
```

A real HTTP integration is in
[how-to/enable-notifications-websocket.md](../how-to/enable-notifications-websocket.md).

## Where to next

- How-to: [wire notifications into your HTTP server](../how-to/enable-notifications-websocket.md).
- Reference: [`notifications` module API](../reference/api.md#notifications).
- Explanation: [why AS2.0 over custom events](../explanation/architecture-decisions.md#why-activity-streams-20-for-notifications).

## Troubleshooting

- **No events appear.** You called `.watch()` after the `put`. The
  watcher only sees events that happen while it is attached. Register
  watchers before producing events.
- **Events arrive out of order.** The `broadcast::Sender` inside the
  manager only guarantees ordering per-receiver; if you clone the
  manager into multiple receivers each sees the same order, but the
  wall clock may not reflect mutation order on busy backends. Use the
  `published` timestamp for ordering when it matters.
- **The receiver lagged.** `broadcast::channel` drops messages on
  slow consumers. If you see a `Lagged(_)` error, bump the buffer
  capacity or consume faster.
