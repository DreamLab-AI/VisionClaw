//! Minimal Axum server that receives Solid Notifications webhooks.
//!
//! Pods using `WebhookChannel2023` POST an Activity Streams 2.0
//! `ChangeNotification` (JSON-LD) to the subscription `receive_from`
//! URL. This example stands up an Axum server that accepts those
//! POSTs, logs them, and returns `204 No Content`.
//!
//! Run with:
//! ```bash
//! cargo run --example webhook_receiver -p solid-pod-rs
//! ```
//!
//! Expected output when a pod fires a notification at it:
//! ```text
//! webhook receiver listening on http://127.0.0.1:8767/hook
//! [Create] http://pod.example/public/thing.ttl (id=urn:uuid:...)
//! [Update] http://pod.example/public/thing.ttl (id=urn:uuid:...)
//! [Delete] http://pod.example/public/thing.ttl (id=urn:uuid:...)
//! ```
//!
//! Test locally with curl:
//! ```bash
//! curl -i -X POST http://127.0.0.1:8767/hook \
//!     -H 'Content-Type: application/ld+json' \
//!     -d '{
//!       "@context": "https://www.w3.org/ns/activitystreams",
//!       "id": "urn:uuid:test",
//!       "type": "Create",
//!       "object": "http://pod.example/public/thing.ttl",
//!       "published": "2026-04-20T12:00:00Z"
//!     }'
//! ```

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use solid_pod_rs::notifications::ChangeNotification;

#[derive(Clone, Default)]
struct AppState {
    received: Arc<AtomicU64>,
}

async fn hook(
    State(state): State<AppState>,
    Json(note): Json<ChangeNotification>,
) -> StatusCode {
    let n = state.received.fetch_add(1, Ordering::Relaxed) + 1;
    println!(
        "[{kind}] {object} (id={id}) total_received={n}",
        kind = note.kind,
        object = note.object,
        id = note.id,
    );
    StatusCode::NO_CONTENT
}

async fn health() -> StatusCode {
    StatusCode::OK
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState::default();
    let app = Router::new()
        .route("/hook", post(hook))
        .route("/health", axum::routing::get(health))
        .with_state(state);

    let addr: SocketAddr = "127.0.0.1:8767".parse()?;
    eprintln!("webhook receiver listening on http://{addr}/hook");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
