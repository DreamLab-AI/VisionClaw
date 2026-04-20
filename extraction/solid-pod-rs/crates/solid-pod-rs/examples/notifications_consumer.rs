//! WebSocket consumer for the `.notifications` channel.
//!
//! Connects to a pod's `WebSocketChannel2023` endpoint, subscribes to
//! a topic, and prints every `ChangeNotification` to stdout as
//! pretty-printed JSON.
//!
//! Usage:
//! ```bash
//! cargo run --example notifications_consumer -p solid-pod-rs -- \
//!     ws://127.0.0.1:8765/.notifications/websocket /public/
//! ```
//!
//! Expected output while another client modifies the pod:
//! ```text
//! connecting to ws://127.0.0.1:8765/.notifications/websocket
//! subscribed to topic: /public/
//! {
//!   "@context": "https://www.w3.org/ns/activitystreams",
//!   "id": "urn:uuid:...",
//!   "type": "Create",
//!   "object": "http://pod.example/public/file.ttl",
//!   "published": "2026-04-20T12:00:00Z"
//! }
//! ```
//!
//! Notes:
//! - The pod server must implement the WebSocket upgrade path that
//!   attaches a `WebSocketChannelManager::stream()` receiver to each
//!   socket. The standalone example does not (it's a REST-only demo);
//!   this example shows the *client* half so you can wire it to any
//!   host app that plugs `WebSocketChannelManager` into a real
//!   upgrade endpoint.

use futures_util::{SinkExt, StreamExt};
use solid_pod_rs::notifications::ChangeNotification;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let ws_url = args
        .next()
        .unwrap_or_else(|| "ws://127.0.0.1:8765/.notifications/websocket".to_string());
    let topic = args.next().unwrap_or_else(|| "/".to_string());

    println!("connecting to {ws_url}");
    let (mut ws, _response) = tokio_tungstenite::connect_async(&ws_url).await?;

    // Send a JSON subscription frame. The exact shape follows Solid
    // Notifications 0.2 §6 — a JSON-LD envelope naming the topic.
    let subscribe = serde_json::json!({
        "@context": "https://www.w3.org/ns/solid/notifications-context/v1",
        "type": "subscribe",
        "topic": topic,
    });
    ws.send(Message::Text(subscribe.to_string())).await?;
    println!("subscribed to topic: {topic}");

    // Every inbound text frame is an Activity Streams 2.0
    // `ChangeNotification`. Binary + ping/pong frames are ignored.
    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(text) => {
                match serde_json::from_str::<ChangeNotification>(&text) {
                    Ok(note) => {
                        let pretty = serde_json::to_string_pretty(&note)
                            .unwrap_or_else(|_| text.clone());
                        println!("{pretty}");
                    }
                    Err(_) => {
                        // Not a well-formed notification; print raw.
                        println!("raw frame: {text}");
                    }
                }
            }
            Message::Close(frame) => {
                println!("server closed connection: {frame:?}");
                break;
            }
            Message::Ping(data) => {
                ws.send(Message::Pong(data)).await?;
            }
            _ => {}
        }
    }

    Ok(())
}
