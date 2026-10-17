//! Smoke test for a running codescape-server: `/wss` XR emulation, V5 sequencing,
//! `0x34` ack pacing, `0x23` agent actions and `/ws/events`.
//!
//! ```text
//! cargo run --example ws_smoke -- 127.0.0.1:4100 127.0.0.1:4101 <token-file> <abs-path-of-a-repo-file> [voice]
//! ```
//!
//! With `voice`, a push-to-talk leg speaks a pocket-tts phrase into `/ws/voice`
//! (needs pocket-tts and nemotron-asr on the network).
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

    if args.get(5).map(String::as_str) == Some("voice") {
        voice_leg(main).await;
    }
    finish();
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
