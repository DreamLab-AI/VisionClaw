//! NIP-98 client — sign an HTTP request against a running pod, PUT a
//! Turtle resource, read it back.
//!
//! This client constructs a NIP-98 kind-27235 event, base64-encodes
//! it, and sends it as the `Authorization: Nostr <b64>` header. It
//! builds both the `u` (URL) and the `payload` (SHA-256 of the body)
//! tags so the pod's body-hash binding (ADR-055 B3) validates.
//!
//! Start a pod first in another shell:
//! ```bash
//! cargo run --example standalone -p solid-pod-rs
//! ```
//!
//! Then run the client:
//! ```bash
//! cargo run --example nip98_client -p solid-pod-rs -- \
//!     http://127.0.0.1:8765/notes/demo.ttl
//! ```
//!
//! Expected output:
//! ```text
//! PUT http://127.0.0.1:8765/notes/demo.ttl
//!   Authorization: Nostr <base64-event>
//!   status: 201 Created
//!   etag:   <hex>
//! GET http://127.0.0.1:8765/notes/demo.ttl
//!   status: 200 OK
//!   body:
//! @prefix ex: <https://example.org/> .
//! ex:demo a ex:Note ; ex:content "hello nip-98" .
//! ```
//!
//! Note: the pod's Phase 1 NIP-98 path performs structural
//! verification (URL + method + payload match, timestamp tolerance,
//! size limits). Schnorr signature verification is a Phase 2
//! deliverable — the `sig` field is populated with a dummy value here
//! so the event deserialises, but not cryptographically checked.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sha2::{Digest, Sha256};

const TURTLE_BODY: &str = r#"@prefix ex: <https://example.org/> .
ex:demo a ex:Note ; ex:content "hello nip-98" .
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8765/notes/demo.ttl".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    // ------------------------------------------------------------------
    // PUT — body is Turtle, payload tag binds the hash.
    // ------------------------------------------------------------------
    let body = TURTLE_BODY.as_bytes();
    let put_header = build_nip98_header(&url, "PUT", Some(body));
    println!("PUT {url}");
    println!("  Authorization: {put_header}");
    let resp = client
        .put(&url)
        .header(reqwest::header::AUTHORIZATION, &put_header)
        .header(reqwest::header::CONTENT_TYPE, "text/turtle")
        .body(body.to_vec())
        .send()
        .await?;
    let status = resp.status();
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>")
        .to_string();
    println!("  status: {status}");
    println!("  etag:   {etag}");

    // ------------------------------------------------------------------
    // GET — no body, so no payload tag needed.
    // ------------------------------------------------------------------
    let get_header = build_nip98_header(&url, "GET", None);
    println!("GET {url}");
    let resp = client
        .get(&url)
        .header(reqwest::header::AUTHORIZATION, &get_header)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    println!("  status: {status}");
    println!("  body:");
    println!("{text}");

    Ok(())
}

/// Build the `Authorization: Nostr <b64>` header.
fn build_nip98_header(url: &str, method: &str, body: Option<&[u8]>) -> String {
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut tags = vec![
        vec!["u".to_string(), url.to_string()],
        vec!["method".to_string(), method.to_string()],
    ];
    if let Some(b) = body {
        if !b.is_empty() {
            tags.push(vec![
                "payload".to_string(),
                hex::encode(Sha256::digest(b)),
            ]);
        }
    }

    let event = serde_json::json!({
        // These two are placeholders. A real client computes the
        // event id as SHA-256 over the canonical serialisation and
        // produces a Schnorr signature over that id; the pod's
        // Phase 1 path only checks structure.
        "id": "0".repeat(64),
        "pubkey": "a".repeat(64),
        "created_at": created_at,
        "kind": 27235,
        "tags": tags,
        "content": "",
        "sig": "0".repeat(128),
    });

    let token = BASE64.encode(serde_json::to_string(&event).unwrap_or_default());
    format!("Nostr {token}")
}
