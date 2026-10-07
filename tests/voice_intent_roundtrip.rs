//! COM-15 / V1 (PRD-023 WP-5): fake-endpoint integration test for the VisionClaw
//! voice-intent CONSUMER.
//!
//! Drives [`VoiceIntentClient`] end to end against a REAL local HTTP server that
//! mimics the agentbox `/v1/voice-intent` D7 contract (ADR-037 D7: additive
//! `actor_did`, mandate-authenticated). The full chain runs for real: build →
//! sign a kind-31402 targeted at the agent DID → NIP-98 mandate header → POST →
//! parse the acceptance → build the Kokoro ack sentence.
//!
//! The live cross-substrate round-trip against the un-gated agentbox producer is
//! **pending-live-session** (the producer un-gates in the same wave, agentbox
//! side); this test proves the consumer half against the contract. The standing
//! `CANARY-VC-COM15-PTT` fires on the live end-to-end, not here.
//!
//! The fake is a hand-rolled blocking HTTP/1.1 responder on a background thread —
//! no mock framework, no in-process shortcut: a real socket, real headers, a real
//! body, so the mandate header and the signed-31402 payload are asserted exactly
//! as they cross the wire.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use nostr_sdk::Keys;
use visionclaw_server::services::voice_intent_client::{
    ack_sentence, VoiceIntentClient, VoiceIntentError,
};

const DID: &str = "did:nostr:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Debug, Clone)]
struct CapturedReq {
    authorization: Option<String>,
    body: String,
}

/// Bind an ephemeral port and serve `accept_count` requests from a background
/// thread. When `require_auth`, a request without a `Nostr …` Authorization
/// header is answered 401 (mimicking the producer's mandate gate); otherwise
/// every request is accepted with a recognised-intent echo.
fn spawn_fake_server(
    accept_count: usize,
    require_auth: bool,
) -> (String, Arc<Mutex<Vec<CapturedReq>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let captured = Arc::new(Mutex::new(Vec::<CapturedReq>::new()));
    let cap = captured.clone();

    thread::spawn(move || {
        for _ in 0..accept_count {
            let (stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let mut write_stream = stream.try_clone().expect("clone stream");
            let mut reader = BufReader::new(stream);

            let mut authorization: Option<String> = None;
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    break; // end of headers
                }
                let lower = trimmed.to_ascii_lowercase();
                if lower.starts_with("authorization:") {
                    authorization = trimmed
                        .split_once(':')
                        .map(|x| x.1)
                        .map(|v| v.trim().to_string());
                } else if let Some(v) = lower.strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
            }

            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                let _ = reader.read_exact(&mut body);
            }
            let body_str = String::from_utf8_lossy(&body).to_string();
            cap.lock().unwrap().push(CapturedReq {
                authorization: authorization.clone(),
                body: body_str,
            });

            let unauthorised = require_auth
                && authorization
                    .as_deref()
                    .map(|a| !a.starts_with("Nostr "))
                    .unwrap_or(true);
            let (status, json) = if unauthorised {
                (
                    "401 Unauthorized",
                    r#"{"success":false,"error":"NIP-98 Authorization header required"}"#
                        .to_string(),
                )
            } else {
                (
                    "200 OK",
                    r#"{"success":true,"event_id":42,"intent":{"verb":"query","action_type_name":"query","subject":"the budget node","recognised":true}}"#
                        .to_string(),
                )
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                json.len()
            );
            let _ = write_stream.write_all(resp.as_bytes());
            let _ = write_stream.flush();
        }
    });

    (format!("http://{addr}/v1/voice-intent"), captured)
}

/// The happy path: a bound, canonical DID → a signed 31402 targeted at that DID
/// → a mandate-authenticated POST → an accepted dispatch → an ack that names the
/// agent and the understood intent. Every wire field is asserted as received.
#[tokio::test]
async fn dispatch_signs_targets_and_is_accepted() {
    let (endpoint, captured) = spawn_fake_server(1, true);
    let client = VoiceIntentClient::new(endpoint, Keys::generate(), Some("desktop".to_string()));

    let accepted = client
        .dispatch("find the budget node", DID, 200)
        .await
        .expect("the fake producer accepts a well-formed governed dispatch");
    assert!(accepted.success);
    assert_eq!(accepted.event_id, Some(42));

    // AC3 precondition: the ack the Kokoro path would speak names the agent and
    // the recognised intent.
    let ack = ack_sentence(&accepted, DID);
    assert!(
        ack.contains("nostr:aaaa"),
        "ack names the target agent: {ack}"
    );
    assert!(
        ack.contains("query"),
        "ack names the understood verb: {ack}"
    );
    assert!(ack.contains("budget node"), "ack names the subject: {ack}");

    // The wire carried the D7 contract exactly.
    let reqs = captured.lock().unwrap();
    assert_eq!(reqs.len(), 1, "exactly one dispatch reached the producer");
    let req = &reqs[0];
    let auth = req
        .authorization
        .as_deref()
        .expect("Authorization header present");
    assert!(
        auth.starts_with("Nostr "),
        "mandate is a NIP-98 header: {auth}"
    );

    let body: serde_json::Value = serde_json::from_str(&req.body).expect("body is JSON");
    assert_eq!(
        body["actor_did"], DID,
        "D7 additive actor_did carried the target"
    );
    assert_eq!(body["transcript"], "find the budget node");
    assert_eq!(
        body["actor"], "desktop",
        "the optional human label is preserved"
    );
    // The signed kind-31402 rides the wire (PRD-023 WP-5 AC2).
    assert_eq!(body["signed_action"]["kind"], 31402);
    assert_eq!(body["signed_action"]["target_did"], DID);
    assert_eq!(
        body["signed_action"]["sig"].as_str().map(str::len),
        Some(128),
        "a BIP-340 schnorr sig is 64 bytes / 128 hex"
    );
    assert_eq!(
        body["signed_action"]["pubkey"].as_str().map(str::len),
        Some(64),
        "an x-only pubkey is 32 bytes / 64 hex"
    );
}

/// Verify before trust: a non-DID target is refused BEFORE any signing or HTTP,
/// so a hashed nickname never reaches the producer (ADR-037 D7, DDD invariant 2).
#[tokio::test]
async fn non_did_target_is_refused_before_http() {
    let (endpoint, captured) = spawn_fake_server(1, true);
    let client = VoiceIntentClient::new(endpoint, Keys::generate(), None);

    let err = client
        .dispatch("do a thing", "researcher-7", 200)
        .await
        .expect_err("a non-did target must be refused");
    assert!(matches!(err, VoiceIntentError::NotADid(_)), "got {err:?}");

    // Nothing crossed the wire.
    assert!(
        captured.lock().unwrap().is_empty(),
        "a refused dispatch must not reach the producer"
    );
}

/// A producer that declines an unauthenticated dispatch surfaces as a rejection,
/// not a false success — the client never fabricates an acceptance.
#[tokio::test]
async fn producer_rejection_surfaces_as_error() {
    // A server that 401s everything (require_auth with a body the client always
    // signs would normally pass, so force rejection by answering non-2xx): here
    // we answer 401 whenever the body is empty — but the client always sends a
    // header, so instead we assert the rejection path via a server that returns
    // 401 for a missing header. To exercise it, point the client at a server that
    // requires auth but we strip nothing — so use a server that rejects all.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut write_stream = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let t = line.trim_end();
                if t.is_empty() {
                    break;
                }
                if let Some(v) = t.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                let _ = reader.read_exact(&mut body);
            }
            let json = r#"{"success":false,"error":"voice-intent-disabled"}"#;
            let resp = format!(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                json.len()
            );
            let _ = write_stream.write_all(resp.as_bytes());
            let _ = write_stream.flush();
        }
    });

    let client = VoiceIntentClient::new(
        format!("http://{addr}/v1/voice-intent"),
        Keys::generate(),
        None,
    );
    let err = client
        .dispatch("find the budget node", DID, 200)
        .await
        .expect_err("a 503 from the gated producer must surface as an error");
    assert!(matches!(err, VoiceIntentError::Rejected(_)), "got {err:?}");
}
