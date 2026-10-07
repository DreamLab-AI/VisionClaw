//! Live presence-handshake smoke test against the running backend.
//!
//! Drives the real `NostrSigner` through the server-initiated challenge flow on
//! `/ws/presence` and prints each step, so we can confirm a correctly-signed
//! client reaches `joined` end-to-end. Run inside the xr-runtime sidecar:
//!
//!   XR_BACKEND_WS=ws://visionclaw_container:4000 cargo run --example live_handshake
//!
//! Not a unit test — it needs a live backend, so it lives as an example.

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use visionclaw_xr_gdext::ports::Signer;
use visionclaw_xr_gdext::signer::NostrSigner;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let base = std::env::var("XR_BACKEND_WS")
        .unwrap_or_else(|_| "ws://visionclaw_container:4000".to_string());
    let url = format!("{base}/ws/presence");
    eprintln!("[handshake] connecting {url}");

    let (mut ws, _resp) = match connect_async(&url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[handshake] CONNECT FAILED: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("[handshake] OPEN");

    // Step 1: receive challenge {type:challenge, nonce:hex, ts:u64}.
    let challenge = match ws.next().await {
        Some(Ok(Message::Text(t))) => t,
        other => {
            eprintln!("[handshake] expected challenge, got {other:?}");
            std::process::exit(2);
        }
    };
    eprintln!("[handshake] CHALLENGE {challenge}");
    let cv: serde_json::Value = serde_json::from_str(&challenge).unwrap();
    let nonce_hex = cv["nonce"].as_str().expect("nonce");
    let ts = cv["ts"].as_u64().expect("ts");
    let nonce_vec = hex::decode(nonce_hex).expect("nonce hex");
    let mut nonce = [0u8; 32];
    nonce.copy_from_slice(&nonce_vec);

    // Step 2: sign with the real Nostr signer and send auth.
    let signer = NostrSigner::generate();
    let did = signer.did().expect("did").as_str().to_owned();
    let signed = signer.sign_challenge(&nonce, ts).expect("sign");
    let auth = serde_json::json!({
        "type": "auth",
        "did": did,
        "signature": signed.signature_hex,
        "room_id": std::env::var("ROOM_URN").unwrap_or_else(|_| "urn:visionclaw:room:sha256-12-deadbeefcafe".to_string()),
        "metadata": { "display_name": "live-handshake-probe", "model_uri": null }
    });
    eprintln!("[handshake] AUTH did={did}");
    ws.send(Message::Text(auth.to_string()))
        .await
        .expect("send auth");

    // Step 3: expect joined, or a close with a code/reason.
    match ws.next().await {
        Some(Ok(Message::Text(t))) => eprintln!("[handshake] RESULT TEXT {t}"),
        Some(Ok(Message::Close(c))) => eprintln!("[handshake] RESULT CLOSE {c:?}"),
        other => eprintln!("[handshake] RESULT OTHER {other:?}"),
    }
}
