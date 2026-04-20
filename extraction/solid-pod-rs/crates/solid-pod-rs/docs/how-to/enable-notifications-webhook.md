# How to enable webhook notifications

**Goal:** accept `WebhookChannel2023` subscriptions and POST Activity
Streams 2.0 notifications to each subscriber's URL.

## Step 1 — Wire the manager to storage

```rust
use solid_pod_rs::notifications::WebhookChannelManager;
use solid_pod_rs::storage::Storage;

let manager = WebhookChannelManager::new();

// Every event under `/` feeds the manager.
let rx = storage.watch("/").await?;
let pod_base = "https://pod.example".to_string();
tokio::spawn(manager.clone().pump_from_storage(rx, pod_base));
```

## Step 2 — Handle subscription POSTs

Subscribers POST to `/.notifications/webhook` with a JSON body like:

```json
{ "topic": "/public/", "receive_from": "https://subscriber.example/hook" }
```

Route handler:

```rust
async fn handle_subscribe(
    state: web::Data<AppState>,
    body: web::Json<SubscribeBody>,
) -> HttpResponse {
    let sub = state.webhook_mgr
        .subscribe(&body.topic, &body.receive_from)
        .await;
    HttpResponse::Created().json(sub)
}
```

`subscribe` returns the full `Subscription` record (id, topic,
channel_type, receive_from).

## Step 3 — Understand delivery semantics

`WebhookChannelManager::deliver_all` per target URL:

| Response | Behaviour |
|---|---|
| `2xx` | `WebhookDelivery::Delivered { status }` — no retry |
| `4xx` | `WebhookDelivery::FatalDrop { status }` — subscription removed |
| `5xx` | `WebhookDelivery::TransientRetry { reason }` — exponential backoff |
| network error | `TransientRetry` |

Retry policy is tunable:

```rust
let mut m = WebhookChannelManager::new();
m.retry_base  = Duration::from_millis(500);  // first backoff (default)
m.max_retries = 3;                            // total attempts (default)
```

Backoff is `retry_base * 2^attempt`: 500 ms, 1 s, 2 s, give up.

A `4xx` response drops the subscription permanently — the subscriber
asked for it. `5xx` does not. Network errors do not.

## Step 4 — Publish the discovery document

```rust
use solid_pod_rs::notifications::discovery_document;

async fn handle_discovery() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/ld+json")
        .json(discovery_document("https://pod.example"))
}
```

Clients fetch this at `/.notifications` to discover your endpoints.

## Step 5 — Secure the webhook endpoint (recommended)

Anyone who can POST to `/.notifications/webhook` can register a
subscription. Gate the handler behind your normal auth middleware
(NIP-98 or Solid-OIDC).

## Step 6 — Topic matching

`pump_from_storage` uses a prefix match: a subscription for `/public/`
receives events for `/public/any/child`. If you need a stricter match
(exact path) use the trait-level `publish()` method directly:

```rust
use solid_pod_rs::notifications::{Notifications, ChangeNotification};

let note = ChangeNotification::from_storage_event(&event, "https://pod.example");
manager.publish("/exact/path", note).await?;
```

`WebhookChannelManager::publish` (`Notifications` trait impl) fires
only to subscribers whose topic is a prefix of the supplied topic.

## Testing

Use `wiremock` (already a dev-dep) to stand up a fake webhook target:

```rust
use wiremock::{MockServer, Mock, ResponseTemplate, matchers::method};

let server = MockServer::start().await;
Mock::given(method("POST"))
    .respond_with(ResponseTemplate::new(200))
    .mount(&server)
    .await;

let mgr = WebhookChannelManager::new();
mgr.subscribe("/x/", &server.uri()).await;
let note = ChangeNotification::from_storage_event(
    &StorageEvent::Created("/x/a".into()),
    "https://pod.example",
);
let outcomes = mgr.deliver_all(&note, |t| "/x/a".starts_with(t)).await;
assert!(matches!(outcomes[0].1, WebhookDelivery::Delivered { .. }));
```

## See also

- Tutorial: [Subscribing to changes](../tutorials/04-subscribing-to-changes.md)
- [how-to/enable-notifications-websocket.md](enable-notifications-websocket.md)
- [reference/api.md §notifications](../reference/api.md#notifications)
