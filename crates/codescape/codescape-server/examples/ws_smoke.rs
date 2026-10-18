//! Smoke test for a running codescape-server: `/wss` XR emulation, V5 sequencing,
//! `0x34` ack pacing, `0x23` agent actions and `/ws/events`.
//!
//! ```text
//! cargo run --example ws_smoke -- 127.0.0.1:4100 127.0.0.1:4101 <token-file> <abs-path-of-a-repo-file> [voice] [proxy=off|on|narrow]
//! ```
//!
//! Every run includes the `/ws/presence` leg (two XR clients speaking the exact
//! client handshake, pose relay, the 20/s cap, `avatar_left`, desktop presence).
//! With `voice`, a push-to-talk leg speaks a pocket-tts phrase into `/ws/voice`
//! (needs pocket-tts and nemotron-asr on the network). With `proxy=off|on`, a
//! handshake-origin leg checks that the server was started with
//! `--trust-proxy` off (forged `X-Forwarded-Host` ignored) or on (forwarded host
//! honoured, foreign origins still refused); `proxy=narrow` expects
//! `--trust-proxy-from` to exclude this client, whose headers are then ignored.
//!
//! Exits non-zero on the first violated expectation.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use codescape_model::wire::{
    decode_record, encode_ack, v5_sequence_of, OPCODE_AGENT_ACTION, PROTOCOL_V5, V5_SEQ_BYTES,
};
use codescape_model::NODE_RECORD_BYTES;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

static PASSED: AtomicUsize = AtomicUsize::new(0);
static FAILED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Record one expectation; never aborts, so every check runs and the summary is complete.
fn check(cond: bool, what: &str) -> bool {
    if cond {
        PASSED.fetch_add(1, Ordering::Relaxed);
        println!("ok   {what}");
    } else {
        FAILED.lock().unwrap().push(what.to_string());
        println!("FAIL {what}");
    }
    cond
}

/// Print the summary and exit non-zero if anything failed.
fn finish() -> ! {
    let failed = FAILED.lock().unwrap();
    println!(
        "\nsummary: {} passed, {} failed",
        PASSED.load(Ordering::Relaxed),
        failed.len()
    );
    for f in failed.iter() {
        println!("  FAIL {f}");
    }
    std::process::exit(if failed.is_empty() { 0 } else { 1 });
}

/// The agent flag on a V5 record's wire id: actor records, which `initialGraphLoad` never lists.
const AGENT_FLAG: u32 = 0x8000_0000;

/// One received V5 frame.
#[derive(Debug, Clone, Copy)]
struct Frame {
    seq: u64,
    /// Records without the agent flag (graph nodes).
    plain: usize,
    /// Records with the agent flag (actors).
    flagged: usize,
}

async fn next_msg(ws: &mut Ws, wait: Duration) -> Option<Message> {
    tokio::time::timeout(wait, ws.next())
        .await
        .ok()
        .flatten()
        .and_then(Result::ok)
}

/// Collect binary position frames for `dur`, optionally acking each. Returns the frames
/// and the number of `0x23` frames seen.
async fn collect(ws: &mut Ws, dur: Duration, ack: bool) -> (Vec<Frame>, usize) {
    let end = Instant::now() + dur;
    let mut frames = vec![];
    let mut actions = 0;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        let Some(m) = next_msg(ws, left).await else {
            break;
        };
        if let Message::Binary(b) = m {
            if b.first() == Some(&OPCODE_AGENT_ACTION) {
                actions += 1;
                continue;
            }
            if b.first() == Some(&PROTOCOL_V5) {
                let seq = v5_sequence_of(&b).unwrap();
                let n = (b.len() - 1 - V5_SEQ_BYTES) / NODE_RECORD_BYTES;
                let mut flagged = 0;
                let mut finite = true;
                for rec in b[1 + V5_SEQ_BYTES..].as_chunks::<NODE_RECORD_BYTES>().0 {
                    match decode_record(&rec[..]) {
                        Some(r) => {
                            finite &= r.position.iter().all(|x| x.is_finite());
                            if r.wire_id & AGENT_FLAG != 0 {
                                flagged += 1;
                            }
                        }
                        None => finite = false,
                    }
                }
                if !finite {
                    check(
                        false,
                        &format!("frame seq {seq}: every record decodes with finite positions"),
                    );
                }
                frames.push(Frame {
                    seq,
                    plain: n - flagged,
                    flagged,
                });
                if ack {
                    ws.send(Message::Binary(encode_ack(seq, n as u32, 0).to_vec()))
                        .await
                        .unwrap();
                }
            }
        }
    }
    (frames, actions)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: ws_smoke <main host:port> <ingest host:port> <token-file> <abs-path-of-a-repo-file> [voice]");
        std::process::exit(2);
    }
    let (main, ingest, token_file, touched) = (&args[1], &args[2], &args[3], &args[4]);
    // A transport failure (connect, send) still prints what passed and failed so far.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        if let Ok(mut f) = FAILED.lock() {
            f.push(format!("aborted: {info}"));
        }
        finish();
    }));

    // 1. /wss: initialGraphLoad first, in the XR parser's shape.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{main}/wss"))
        .await
        .expect("connect /wss");
    let first = match next_msg(&mut ws, Duration::from_secs(5)).await {
        Some(Message::Text(t)) => t.to_string(),
        _ => String::new(),
    };
    let v: serde_json::Value = serde_json::from_str(&first).unwrap_or_default();
    check(
        v["type"] == "initialGraphLoad",
        "first /wss frame is an initialGraphLoad text frame",
    );
    let empty = Vec::new();
    let nodes = v["nodes"].as_array().unwrap_or(&empty);
    let edges = v["edges"].as_array().unwrap_or(&empty);
    let t = first;
    check(
        !nodes.is_empty()
            && nodes.iter().all(|n| {
                n["id"].is_u64()
                    && n["label"].is_string()
                    && n["node_type"].is_string()
                    && n["metadata"]["source_file"].is_string()
            }),
        "nodes carry id/label/node_type/metadata.source_file",
    );
    check(
        edges.iter().all(|e| {
            e["source_id"].is_u64() && e["target_id"].is_u64() && e["edge_type"].is_string()
        }),
        "edges carry source_id/target_id/edge_type",
    );
    println!(
        "     initialGraphLoad: {} nodes, {} edges, {} bytes",
        nodes.len(),
        edges.len(),
        t.len()
    );
    let some_file = nodes
        .iter()
        .find(|n| n["node_type"] == "file")
        .and_then(|n| n["id"].as_u64())
        .unwrap_or(0);

    // 2. Keyframe, acked: pacing switches on.
    let (f, _) = collect(&mut ws, Duration::from_millis(400), true).await;
    let key = f.first().copied();
    check(
        key.is_some_and(|k| k.seq == 1 && k.plain == nodes.len()),
        &format!(
            "keyframe seq 1: unflagged records equal the initialGraphLoad node count {} ({key:?})",
            nodes.len()
        ),
    );
    // Actor records ride the stream with the agent flag; XR upserts them by masked id.
    println!(
        "     keyframe agent-flagged (actor) records: {}",
        key.map_or(0, |k| k.flagged)
    );

    // 3. Wake the layout and stop acking: at most two frames go out, then the window holds.
    ws.send(Message::Text(format!(
        r#"{{"type":"focus","node_id":{some_file}}}"#
    )))
    .await
    .unwrap();
    let (unacked, _) = collect(&mut ws, Duration::from_millis(1500), false).await;
    check(
        unacked.len() <= 2,
        &format!(
            "without acks at most 2 frames in flight (got {})",
            unacked.len()
        ),
    );
    // 4. After the 2 s ack timeout the server recovers with a keyframe, sequence still rising.
    let (recovered, _) = collect(&mut ws, Duration::from_millis(1500), true).await;
    let mut all: Vec<u64> = f
        .iter()
        .chain(&unacked)
        .chain(&recovered)
        .map(|x| x.seq)
        .collect();
    let sorted = all.windows(2).all(|w| w[1] > w[0]);
    check(sorted, &format!("sequences strictly increase: {all:?}"));
    if unacked.len() == 2 {
        println!("     recovered frames: {recovered:?}");
        check(recovered.iter().any(|x| x.plain >= nodes.len()), "window was full: recovery keyframe (projection now includes the focused file's symbols) after the ack timeout");
    } else {
        println!(
            "     (layout produced {} unacked frame(s); window never filled, no recovery expected)",
            unacked.len()
        );
    }
    all.clear();

    // 5. Presence ingest → 0x23 on /wss.
    let token = std::fs::read_to_string(token_file).unwrap();
    let body = serde_json::json!({"actor":"smoke:agent","kind":"agent","event":"post_tool_use","tool":"Read","paths":[touched]});
    let client = reqwest::Client::new();
    let r = client
        .post(format!("http://{ingest}/ingest/v1/activity"))
        .json(&body)
        .send()
        .await
        .unwrap();
    check(r.status() == 401, "ingest without token is 401");
    let r = client
        .post(format!("http://{ingest}/ingest/v1/activity"))
        .bearer_auth(token.trim())
        .json(&body)
        .send()
        .await
        .unwrap();
    check(r.status() == 202, "ingest with token is 202");
    let (_, actions) = collect(&mut ws, Duration::from_millis(800), true).await;
    check(actions >= 1, "0x23 agent action delivered on /wss");

    // 6. /ws/events: hello, then a typed command answered with a query result.
    let (mut ev, _) = tokio_tungstenite::connect_async(format!("ws://{main}/ws/events"))
        .await
        .expect("connect /ws/events");
    let h = match next_msg(&mut ev, Duration::from_secs(5)).await {
        Some(Message::Text(h)) => serde_json::from_str::<serde_json::Value>(&h).unwrap_or_default(),
        _ => serde_json::Value::Null,
    };
    check(
        h["type"] == "hello" && h["nodes"].as_u64().unwrap_or(0) > 0,
        "events hello with a non-empty graph",
    );
    ev.send(Message::Text(
        r#"{"type":"command","text":"focus delta"}"#.into(),
    ))
    .await
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut answered = false;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if let Some(Message::Text(t)) = next_msg(&mut ev, left).await {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
            if v["type"] == "query" {
                println!("     answer: {}", v["answer"]);
                answered = v["node_ids"].as_array().is_some_and(|a| !a.is_empty());
                break;
            }
        }
    }
    check(answered, "typed command answered with highlighted nodes");

    presence_leg(main).await;
    let flags = &args[5..];
    for f in flags {
        match f.as_str() {
            "proxy=off" => proxy_leg(main, false).await,
            "proxy=on" => proxy_leg(main, true).await,
            "proxy=narrow" => narrow_leg(main).await,
            _ => {}
        }
    }
    if flags.iter().any(|f| f == "voice") {
        voice_leg(main).await;
    }
    finish();
}

/// HTTP status of a WebSocket handshake with extra headers (101 = upgraded).
async fn upgrade_status(url: &str, headers: &[(&'static str, &str)]) -> u16 {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
    let mut req = url.into_client_request().unwrap();
    for (k, v) in headers {
        req.headers_mut().insert(
            HeaderName::from_static(k),
            HeaderValue::from_str(v).unwrap(),
        );
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((mut ws, _)) => {
            let _ = ws.close(None).await;
            101
        }
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => r.status().as_u16(),
        Err(_) => 0,
    }
}

/// Origin checks against the dev nginx's header set (`Host` without port,
/// `X-Forwarded-Host` with it).
async fn proxy_leg(main: &str, trusted: bool) {
    let url = format!("ws://{main}/ws/positions");
    let own = format!("http://{main}");
    if trusted {
        check(
            upgrade_status(
                &url,
                &[
                    ("origin", "http://box:3001"),
                    ("host", "box"),
                    ("x-forwarded-host", "box:3001"),
                ],
            )
            .await
                == 101,
            "trust on: Origin http://box:3001 matches X-Forwarded-Host box:3001",
        );
        check(
            upgrade_status(
                &url,
                &[
                    ("origin", "http://evil:9"),
                    ("x-forwarded-host", "box:3001"),
                ],
            )
            .await
                == 403,
            "trust on: a foreign Origin is still refused",
        );
        check(
            upgrade_status(&url, &[("origin", "http://box:3001"), ("host", "box")]).await == 403,
            "trust on: without X-Forwarded-Host, Host (no port) does not match :3001",
        );
    } else {
        check(
            upgrade_status(
                &url,
                &[("origin", own.as_str()), ("x-forwarded-host", "evil:9")],
            )
            .await
                == 101,
            "trust off: normal Origin with a forged X-Forwarded-Host evil:9 still upgrades",
        );
        check(
            upgrade_status(
                &url,
                &[("origin", "http://evil:9"), ("x-forwarded-host", "evil:9")],
            )
            .await
                == 403,
            "trust off: Origin http://evil:9 is refused although X-Forwarded-Host claims evil:9",
        );
        check(
            upgrade_status(
                &url,
                &[
                    ("origin", "http://box:3001"),
                    ("host", "box"),
                    ("x-forwarded-host", "box:3001"),
                ],
            )
            .await
                == 403,
            "trust off: the proxied Origin is not same-origin (headers ignored)",
        );
    }
}

/// Server started with `--trust-proxy 1 --trust-proxy-from <an address that is
/// not this client>`: the smoke client is an unlisted peer, so its forwarded
/// headers are ignored exactly as with trust off.
async fn narrow_leg(main: &str) {
    let url = format!("ws://{main}/ws/positions");
    check(
        upgrade_status(
            &url,
            &[
                ("origin", "http://box:3001"),
                ("host", "box"),
                ("x-forwarded-host", "box:3001"),
            ],
        )
        .await
            == 403,
        "trust narrowed to another peer: this peer's X-Forwarded-Host is ignored",
    );
    check(
        upgrade_status(
            &url,
            &[("origin", "http://evil:9"), ("x-forwarded-host", "evil:9")],
        )
        .await
            == 403,
        "trust narrowed to another peer: forged evil:9 pair refused",
    );
    let own = format!("http://{main}");
    check(
        upgrade_status(
            &url,
            &[("origin", own.as_str()), ("x-forwarded-host", "evil:9")],
        )
        .await
            == 101,
        "trust narrowed to another peer: same-origin by Host still upgrades",
    );
}

/// One XR client: challenge, the client's exact `auth`, `joined`. Returns the socket,
/// the `joined` JSON and the pubkey hex it claimed.
async fn xr_join(main: &str, room: &str, name: &str, key: char) -> (Ws, serde_json::Value, String) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{main}/ws/presence"))
        .await
        .expect("connect /ws/presence");
    let ch = match next_msg(&mut ws, Duration::from_secs(5)).await {
        Some(Message::Text(t)) => serde_json::from_str::<serde_json::Value>(&t).unwrap_or_default(),
        _ => serde_json::Value::Null,
    };
    check(
        ch["type"] == "challenge"
            && ch["nonce"].as_str().is_some_and(|n| n.len() == 64)
            && ch["ts"].as_u64().is_some(),
        &format!("{name}: challenge with a 32-byte nonce and ts"),
    );
    let hex = key.to_string().repeat(64);
    // Byte for byte what PresenceClient::handshake sends.
    let auth = format!(
        r#"{{"type":"auth","did":"did:nostr:{hex}","signature":"{}","room_id":"{room}","metadata":{{"display_name":"{name}","model_uri":null}}}}"#,
        "c".repeat(128)
    );
    ws.send(Message::Text(auth)).await.unwrap();
    let joined = match next_msg(&mut ws, Duration::from_secs(5)).await {
        Some(Message::Text(t)) => serde_json::from_str(&t).unwrap_or_default(),
        _ => serde_json::Value::Null,
    };
    check(
        joined["type"] == "joined"
            && joined["room_id"] == room
            && joined["avatar_id"] == format!("urn:visionclaw:avatar:{hex}"),
        &format!("{name}: joined with avatar urn:visionclaw:avatar:<pubkey>"),
    );
    (ws, joined, hex)
}

/// Next text frame parsed as JSON, skipping binary, within `wait`.
async fn next_json(ws: &mut Ws, wait: Duration) -> Option<serde_json::Value> {
    let end = Instant::now() + wait;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Message::Text(t) = next_msg(ws, left).await? {
            return serde_json::from_str(&t).ok();
        }
    }
    None
}

async fn presence_leg(main: &str) {
    use codescape_server::xr_presence::{decode_sibling, encode_pose, room_u32};
    let room = "urn:visionclaw:room:sha256-12-5c0de5e55e55";
    let (mut events, _) = tokio_tungstenite::connect_async(format!("ws://{main}/ws/events"))
        .await
        .unwrap();
    let (mut a, _, a_hex) = xr_join(main, room, "alice-xr", 'a').await;
    // The desktop hears the XR member as a human actor labelled with its pubkey.
    let end = Instant::now() + Duration::from_secs(3);
    let mut halo = false;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        let Some(v) = next_json(&mut events, left).await else {
            break;
        };
        if v["type"] == "presence" && v["actor_label"] == a_hex.as_str() {
            halo = v["actor_kind"] == "human" && v["kind"] == "focus";
            break;
        }
    }
    check(
        halo,
        "desktop /ws/events sees the XR member as a human presence",
    );

    let (mut b, b_joined, b_hex) = xr_join(main, room, "bob-xr", 'b').await;
    let roster: Vec<&str> = b_joined["members"]
        .as_array()
        .map(|m| {
            m.iter()
                .filter_map(|x| x["display_name"].as_str())
                .collect()
        })
        .unwrap_or_default();
    check(
        roster == ["alice-xr", "bob-xr"],
        "bob's roster lists alice and bob",
    );
    let a_local = b_joined["members"][0]["local_id"].as_u64().unwrap_or(0) as u32;
    let aj = next_json(&mut a, Duration::from_secs(3))
        .await
        .unwrap_or_default();
    check(
        aj["type"] == "avatar_joined"
            && aj["did"] == format!("did:nostr:{b_hex}")
            && aj["local_id"].as_u64().is_some(),
        "alice receives avatar_joined for bob with a local_id",
    );

    // Alice's pose reaches bob as a sibling frame tagged with her local_id.
    let mut tf = Vec::new();
    for f in [0.5f32, 1.6, -2.0, 0.0, 0.0, 0.0, 1.0] {
        tf.extend_from_slice(&f.to_le_bytes());
    }
    let avatar = format!("urn:visionclaw:avatar:{a_hex}");
    a.send(Message::Binary(encode_pose(room, &avatar, 1, 0b001, &tf)))
        .await
        .unwrap();
    let mut relayed = None;
    let end = Instant::now() + Duration::from_secs(3);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Some(Message::Binary(f)) = next_msg(&mut b, left).await {
            relayed = decode_sibling(&f);
            break;
        }
    }
    check(
        relayed
            .as_ref()
            .is_some_and(|(_, r, e)| *r == room_u32(room) && e == &[(a_local, 1, 0b001)]),
        "bob receives alice's pose as a 0x43 sibling frame",
    );

    // 60 poses at once: at most 20 per second are relayed.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    for i in 0..60u64 {
        a.send(Message::Binary(encode_pose(
            room,
            &avatar,
            100 + i,
            0b001,
            &tf,
        )))
        .await
        .unwrap();
    }
    let mut got = 0;
    let end = Instant::now() + Duration::from_millis(700);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match next_msg(&mut b, left).await {
            Some(Message::Binary(_)) => got += 1,
            Some(_) => {}
            None => break,
        }
    }
    println!("     relayed {got} of 60 burst poses");
    check(
        (1..=20).contains(&got),
        "pose burst capped at 20/s per member",
    );

    // Bob disconnects: alice gets avatar_left.
    let _ = b.close(None).await;
    drop(b);
    let mut left_ok = false;
    let end = Instant::now() + Duration::from_secs(3);
    while let Some(rem) = end.checked_duration_since(Instant::now()) {
        let Some(v) = next_json(&mut a, rem).await else {
            break;
        };
        if v["type"] == "avatar_left" {
            left_ok = v["avatar_id"] == format!("urn:visionclaw:avatar:{b_hex}");
            break;
        }
    }
    check(left_ok, "alice receives avatar_left when bob disconnects");
    let _ = a.close(None).await;
}

/// Push-to-talk: `start`, PCM streamed straight away (exercising the pre-roll before the
/// floor echo), `stop`; expect `listening`, a final caption on `/ws/events`, a `query`
/// answer and a spoken reply framed by `tts_start`/`tts_end`.
async fn voice_leg(main: &str) {
    let speech = reqwest::Client::new()
        .post("http://pocket-tts:8000/v1/audio/speech")
        .json(&serde_json::json!({"input":"Show callers of stable match please.","voice":"alba","response_format":"pcm","stream":true}))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let pcm: Vec<u8> = speech
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|b| (i16::from_le_bytes(*b) as f32 / 32768.0).to_le_bytes())
        .collect();
    // A named events socket for this speaker, to observe its captions.
    let (mut events, _) =
        tokio_tungstenite::connect_async(format!("ws://{main}/ws/events?name=smoke-voice"))
            .await
            .unwrap();
    let (mut voice, _) =
        tokio_tungstenite::connect_async(format!("ws://{main}/ws/voice?name=smoke-voice"))
            .await
            .expect("connect /ws/voice");
    voice
        .send(Message::Text(r#"{"type":"start"}"#.into()))
        .await
        .unwrap();
    for frame in pcm.chunks(1920 * 4) {
        voice.send(Message::Binary(frame.to_vec())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    voice
        .send(Message::Text(r#"{"type":"stop"}"#.into()))
        .await
        .unwrap();

    let (mut listening, mut query, mut tts_start, mut tts_end, mut pcm_bytes) =
        (false, None, false, false, 0usize);
    let deadline = Instant::now() + Duration::from_secs(30);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match next_msg(&mut voice, left).await {
            Some(Message::Text(t)) => {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                match v["type"].as_str() {
                    Some("listening") => listening = true,
                    Some("query") => query = Some(v["answer"].as_str().unwrap_or("").to_string()),
                    Some("tts_start") => tts_start = true,
                    Some("tts_end") => {
                        tts_end = true;
                        break;
                    }
                    Some("error") => println!("     voice error: {}", v["message"]),
                    _ => {}
                }
            }
            Some(Message::Binary(b)) => pcm_bytes += b.len(),
            Some(_) => {}
            None => break,
        }
    }
    check(listening, "voice: listening after the floor was granted");
    let mut caption = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Some(Message::Text(t)) = next_msg(&mut events, left).await else {
            break;
        };
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "caption" && v["partial"] == false {
            caption = v["text"].as_str().map(str::to_string);
        }
    }
    println!("     final caption: {caption:?}");
    check(
        caption.as_deref().is_some_and(|c| c.starts_with("Show")),
        "voice: final caption begins with the first word (no clipped pre-roll)",
    );
    println!("     spoken answer: {query:?}");
    check(query.is_some(), "voice: transcript executed as a command");
    check(
        tts_start && tts_end && pcm_bytes > 0 && pcm_bytes % 2 == 0,
        &format!("voice: answer spoken back ({pcm_bytes} bytes s16le)"),
    );
}
