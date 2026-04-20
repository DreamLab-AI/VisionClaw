# How to enable WebSocket notifications

**Goal:** accept `WebSocketChannel2023` subscriptions and stream AS2.0
change notifications to connected clients.

## Step 1 — Construct the manager

```rust
use solid_pod_rs::notifications::WebSocketChannelManager;

let ws_mgr = WebSocketChannelManager::new();
```

Default heartbeat interval is 30 s. Override:

```rust
let ws_mgr = WebSocketChannelManager::new()
    .with_heartbeat(Duration::from_secs(15));
```

## Step 2 — Pump storage events

```rust
let rx = storage.watch("/").await?;
tokio::spawn(ws_mgr.clone().pump_from_storage(
    rx,
    "https://pod.example".to_string(),
));
```

The pump task converts each `StorageEvent` into a
`ChangeNotification` and broadcasts it on the manager's internal
channel.

## Step 3 — Subscribe and acknowledge

When a client POSTs `/.notifications/websocket` with a body like
`{ "topic": "/public/" }`:

```rust
async fn handle_ws_subscribe(
    state: web::Data<AppState>,
    body: web::Json<WsSubscribeBody>,
) -> HttpResponse {
    let sub = state.ws_mgr
        .subscribe(&body.topic, "wss://pod.example")
        .await;
    HttpResponse::Created().json(sub)
}
```

The `receive_from` URL the manager returns (e.g.
`wss://pod.example/subscription/%2Fpublic%2F`) is what the client
connects to next.

## Step 4 — Upgrade to WebSocket

Using `tokio-tungstenite` (a runtime dep of the crate):

```rust
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

async fn handle_websocket_upgrade(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    // Use actix-ws / axum upgrade helpers to get the raw socket.
    // Then:
    let mut stream_rx = state.ws_mgr.stream();  // broadcast::Receiver<ChangeNotification>

    actix_web_actors::ws::start_with_addr(
        WsActor { stream_rx },
        &req,
        stream,
    )
}
```

Each frame you write to the WebSocket is the JSON serialisation of a
`ChangeNotification`:

```json
{
  "@context": "https://www.w3.org/ns/activitystreams",
  "id": "urn:uuid:9f2b...",
  "type": "Update",
  "object": "https://pod.example/notes/hello.jsonld",
  "published": "2026-04-20T12:00:00Z"
}
```

## Step 5 — Heartbeats

The manager exposes `heartbeat_interval()` so your WebSocket task
can schedule periodic ping frames:

```rust
let hb = ws_mgr.heartbeat_interval();
let mut tick = tokio::time::interval(hb);
loop {
    tokio::select! {
        _ = tick.tick() => socket.send(Message::Ping(vec![])).await?,
        Ok(note) = stream_rx.recv() => {
            let payload = serde_json::to_string(&note)?;
            socket.send(Message::Text(payload)).await?;
        }
    }
}
```

## Step 6 — Topic matching

Unlike webhooks, the built-in `stream()` receiver emits every event.
Filter per-client in your WebSocket task:

```rust
if note.object.starts_with(&subscription_topic_uri) {
    socket.send(Message::Text(serde_json::to_string(&note)?)).await?;
}
```

## Subscription lifecycle

| Call | When |
|---|---|
| `subscribe(topic, base_url)` | POST `/.notifications/websocket` |
| `unsubscribe(id)`            | DELETE `/.notifications/websocket/{id}` or on socket close |
| `active_subscriptions()`     | metrics endpoint |

Always call `unsubscribe` on socket close, otherwise the manager holds
the subscription record indefinitely.

## Discovery

Same `discovery_document("https://pod.example")` as
[webhooks](enable-notifications-webhook.md). The discovery response
advertises both channel types; clients pick one.

## See also

- Tutorial: [Subscribing to changes](../tutorials/04-subscribing-to-changes.md)
- [reference/api.md §WebSocketChannelManager](../reference/api.md#websocketchannelmanager)
- [Solid Notifications Protocol 0.2](https://solid.github.io/notifications/protocol/)
