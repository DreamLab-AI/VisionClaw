//! Same-user session relay for the memory explorer's beat clock and route
//! (ADR-2134).
//!
//! The desktop sends two client text frames on `/wss`; the server validates
//! them and rebroadcasts them to the **same authenticated pubkey's other
//! sessions** (in practice the Godot XR client), never to anyone else:
//!
//! - `{"type":"beatClock","bpm","phaseAt","confidence","source","sentAt"?}` —
//!   the memory explorer's beat clock (`memoryCloud/beatClock.ts`). `phaseAt`
//!   arrives in the sender's epoch; when `sentAt` (the sender's clock at send)
//!   is present the relay rebases it onto the server clock
//!   (`phaseAt − sentAt + serverNow`), so receivers only need their own
//!   offset to the server, not to the desktop. The relayed frame carries
//!   `serverTime`.
//! - `{"type":"memoryRoute","snapshotId","seq","sentAt","path":[…],"sidecar":[…],"query"}`
//!   — the current query route through the memory cloud as snapshot row
//!   indices, root → answer (an empty `path` clears it). The shape and limits
//!   are those of the headset's parser, `xr-client/rust/src/memory_route.rs`
//!   (`parse_route`): the relay never forwards a frame that parser would reject.
//!
//! Both are re-serialised from validated fields (unknown keys are dropped, so
//! the relay can never be used to smuggle arbitrary JSON to another session)
//! and throttled per session to ≤ 4 Hz with a trailing flush, so the last
//! state always lands. The desktop heartbeats the clock every 2 s.

use serde_json::{json, Value};

/// Minimum spacing between relayed frames of one kind from one session (4 Hz).
pub const RELAY_MIN_INTERVAL_MS: f64 = 250.0;
/// Largest relay frame accepted, in bytes of JSON text.
pub const MAX_RELAY_FRAME_BYTES: usize = 16 * 1024;
/// Longest route path accepted (`memory_route.rs` MAX_PATH).
pub const MAX_ROUTE_PATH: usize = 64;
/// Sidecar marks kept (`memory_route.rs` MAX_SIDECAR).
pub const MAX_ROUTE_SIDECAR: usize = 64;
/// Query echo length in characters (`memory_route.rs` MAX_QUERY_CHARS).
pub const MAX_ROUTE_QUERY_CHARS: usize = 120;
/// Longest snapshot id accepted.
pub const MAX_RELAY_ID_LEN: usize = 128;
/// Accepted tempo range (beatClock.ts TAP_MIN_BPM / TAP_MAX_BPM).
pub const MIN_BPM: f64 = 40.0;
pub const MAX_BPM: f64 = 220.0;
/// Wire beat sources (beatClock.ts BEAT_SOURCES).
pub const BEAT_SOURCES: [&str; 4] = ["off", "file", "tap", "spotify"];

/// The two relayed kinds; each has its own throttle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelayKind {
    BeatClock,
    MemoryRoute,
}

impl RelayKind {
    pub fn from_type(t: &str) -> Option<Self> {
        match t {
            "beatClock" => Some(Self::BeatClock),
            "memoryRoute" => Some(Self::MemoryRoute),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BeatClock => "beatClock",
            Self::MemoryRoute => "memoryRoute",
        }
    }
}

/// Why a relay frame was refused (logged, and echoed to the sender as an
/// `error` frame so a desktop bug is visible rather than silent).
#[derive(Debug, Clone, PartialEq)]
pub enum RelayReject {
    TooLarge(usize),
    Field(&'static str),
    Unauthenticated,
}

impl std::fmt::Display for RelayReject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(n) => write!(f, "frame of {n} bytes exceeds {MAX_RELAY_FRAME_BYTES}"),
            Self::Field(name) => write!(f, "invalid or missing field '{name}'"),
            Self::Unauthenticated => write!(f, "relay requires an authenticated session"),
        }
    }
}

fn finite(v: &Value, key: &'static str) -> Result<f64, RelayReject> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|x| x.is_finite())
        .ok_or(RelayReject::Field(key))
}

fn short_id(v: &Value, key: &'static str) -> Result<String, RelayReject> {
    match v.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() && s.len() <= MAX_RELAY_ID_LEN => Ok(s.to_owned()),
        _ => Err(RelayReject::Field(key)),
    }
}

/// Validate a `beatClock` frame and build the canonical relayed JSON.
/// `server_now_ms` is the server's Unix-ms clock at receipt.
pub fn validate_beat_clock(msg: &Value, server_now_ms: f64) -> Result<String, RelayReject> {
    let bpm = finite(msg, "bpm")?;
    if !(MIN_BPM..=MAX_BPM).contains(&bpm) {
        return Err(RelayReject::Field("bpm"));
    }
    let mut phase_at = finite(msg, "phaseAt")?;
    if phase_at < 0.0 {
        return Err(RelayReject::Field("phaseAt"));
    }
    let confidence = finite(msg, "confidence")?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err(RelayReject::Field("confidence"));
    }
    let source = msg.get("source").and_then(Value::as_str).ok_or(RelayReject::Field("source"))?;
    if !BEAT_SOURCES.contains(&source) {
        return Err(RelayReject::Field("source"));
    }
    if let Some(sent) = msg.get("sentAt") {
        let sent_at = sent.as_f64().filter(|x| x.is_finite() && *x > 0.0).ok_or(RelayReject::Field("sentAt"))?;
        // Rebase the phase onto the server clock; the one-way uplink latency is
        // the only residual error. A locked phase never rebases below 1 ms
        // (0 means "not locked" on the wire).
        if phase_at > 0.0 {
            phase_at = (phase_at - sent_at + server_now_ms).max(1.0);
        }
    }
    Ok(json!({
        "type": "beatClock",
        "bpm": bpm,
        "phaseAt": phase_at,
        "confidence": confidence,
        "source": source,
        "serverTime": server_now_ms,
    })
    .to_string())
}

/// A snapshot row index: a non-negative whole number that fits u32.
fn as_row(v: &Value) -> Option<u32> {
    let f = v.as_f64()?;
    (f >= 0.0 && f.fract() == 0.0 && f <= u32::MAX as f64).then_some(f as u32)
}

/// Validate a `memoryRoute` frame and build the canonical relayed JSON, with
/// the headset parser's rules: non-blank `snapshotId`; `path` ≤ 64 row indices
/// (any bad entry rejects the frame); `sidecar` bad entries dropped, ≤ 64 kept;
/// `query` cut to 120 characters; `seq` / `sentAt` finite and ≥ 0 (0 when
/// absent). The headset orders frames by (`sentAt`, `seq`).
pub fn validate_memory_route(msg: &Value, server_now_ms: f64) -> Result<String, RelayReject> {
    let snapshot_id = short_id(msg, "snapshotId")?;
    if snapshot_id.trim().is_empty() {
        return Err(RelayReject::Field("snapshotId"));
    }
    let raw = msg.get("path").and_then(Value::as_array).ok_or(RelayReject::Field("path"))?;
    if raw.len() > MAX_ROUTE_PATH {
        return Err(RelayReject::Field("path"));
    }
    let path = raw.iter().map(as_row).collect::<Option<Vec<u32>>>().ok_or(RelayReject::Field("path"))?;
    let sidecar: Vec<u32> = match msg.get("sidecar") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.iter().filter_map(as_row).take(MAX_ROUTE_SIDECAR).collect(),
        Some(_) => return Err(RelayReject::Field("sidecar")),
    };
    let query: String = match msg.get("query") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(q)) => q.chars().take(MAX_ROUTE_QUERY_CHARS).collect(),
        Some(_) => return Err(RelayReject::Field("query")),
    };
    let non_neg = |key: &'static str| -> Result<f64, RelayReject> {
        match msg.get(key) {
            None | Some(Value::Null) => Ok(0.0),
            Some(v) => v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or(RelayReject::Field(key)),
        }
    };
    let seq = non_neg("seq")?.floor();
    let sent_at = non_neg("sentAt")?;
    Ok(json!({
        "type": "memoryRoute",
        "snapshotId": snapshot_id,
        "seq": seq,
        "sentAt": sent_at,
        "path": path,
        "sidecar": sidecar,
        "query": query,
        "serverTime": server_now_ms,
    })
    .to_string())
}

/// Validate a relay frame of `kind` from its raw text and parsed value.
pub fn validate(kind: RelayKind, raw_len: usize, msg: &Value, server_now_ms: f64) -> Result<String, RelayReject> {
    if raw_len > MAX_RELAY_FRAME_BYTES {
        return Err(RelayReject::TooLarge(raw_len));
    }
    match kind {
        RelayKind::BeatClock => validate_beat_clock(msg, server_now_ms),
        RelayKind::MemoryRoute => validate_memory_route(msg, server_now_ms),
    }
}

/// What the throttle decided for one offered frame.
#[derive(Debug, Clone, PartialEq)]
pub enum ThrottleDecision {
    /// Relay this now.
    Send(String),
    /// Held as the pending frame; schedule one flush after `delay_ms`.
    Schedule { delay_ms: f64 },
    /// Replaced an already-pending frame; a flush is already scheduled.
    Coalesced,
}

/// Per-session, per-kind throttle: at most one frame per
/// [`RELAY_MIN_INTERVAL_MS`], with the newest held frame flushed at the end of
/// the interval so the final state is never lost.
#[derive(Debug, Clone, Default)]
pub struct RelayThrottle {
    last_sent_ms: Option<f64>,
    pending: Option<String>,
    flush_scheduled: bool,
}

impl RelayThrottle {
    pub fn offer(&mut self, now_ms: f64, frame: String) -> ThrottleDecision {
        let since = self.last_sent_ms.map(|t| now_ms - t).unwrap_or(f64::INFINITY);
        if since >= RELAY_MIN_INTERVAL_MS && !self.flush_scheduled {
            self.last_sent_ms = Some(now_ms);
            return ThrottleDecision::Send(frame);
        }
        self.pending = Some(frame);
        if self.flush_scheduled {
            return ThrottleDecision::Coalesced;
        }
        self.flush_scheduled = true;
        ThrottleDecision::Schedule { delay_ms: (RELAY_MIN_INTERVAL_MS - since).max(0.0) }
    }

    /// The scheduled flush fired: the newest pending frame, if any.
    pub fn flush(&mut self, now_ms: f64) -> Option<String> {
        self.flush_scheduled = false;
        let f = self.pending.take();
        if f.is_some() {
            self.last_sent_ms = Some(now_ms);
        }
        f
    }
}

// ─── actor glue ─────────────────────────────────────────────────────────────

use actix::prelude::*;
use log::{debug, warn};

use super::types::SocketFlowServer;

fn server_now_ms() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64
}

fn deliver(act: &SocketFlowServer, kind: RelayKind, frame: String) {
    let Some(pubkey) = act.pubkey.clone() else {
        return;
    };
    debug!("[relay] {} from client {:?} to other sessions of {}", kind.as_str(), act.client_id, pubkey);
    act.client_manager_addr.do_send(crate::actors::messages::RelayToUserSessions {
        pubkey,
        exclude_client_id: act.client_id,
        message: frame,
    });
}

/// Route one `beatClock` / `memoryRoute` client frame: authenticate, validate,
/// throttle, then hand to the coordinator for same-pubkey delivery.
pub(crate) fn handle_relay(
    act: &mut SocketFlowServer,
    kind: RelayKind,
    raw_len: usize,
    msg: &Value,
    ctx: &mut <SocketFlowServer as Actor>::Context,
) {
    if act.pubkey.is_none() {
        if !act.relay_unauth_reported {
            act.relay_unauth_reported = true;
            warn!("[relay] {} from an unauthenticated session dropped", kind.as_str());
            ctx.text(json!({"type": "error", "message": format!("{}: {}", kind.as_str(), RelayReject::Unauthenticated)}).to_string());
        }
        return;
    }
    let now = server_now_ms();
    let frame = match validate(kind, raw_len, msg, now) {
        Ok(f) => f,
        Err(e) => {
            warn!("[relay] {} rejected: {}", kind.as_str(), e);
            ctx.text(json!({"type": "error", "message": format!("{} rejected: {}", kind.as_str(), e)}).to_string());
            return;
        }
    };
    let decision = act.relay_throttles.entry(kind).or_default().offer(now, frame);
    match decision {
        ThrottleDecision::Send(f) => deliver(act, kind, f),
        ThrottleDecision::Schedule { delay_ms } => {
            let delay = std::time::Duration::from_millis(delay_ms.ceil() as u64);
            ctx.run_later(delay, move |act, _ctx| {
                let now = server_now_ms();
                let pending = act.relay_throttles.get_mut(&kind).and_then(|t| t.flush(now));
                if let Some(f) = pending {
                    deliver(act, kind, f);
                }
            });
        }
        ThrottleDecision::Coalesced => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_760_000_000_000.0;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn beat_clock_is_validated_and_canonicalised() {
        let m = json!({"type":"beatClock","bpm":128.0,"phaseAt":NOW - 300.0,"confidence":0.6,"source":"tap","evil":"<x>"});
        let out = parse(&validate_beat_clock(&m, NOW).unwrap());
        assert_eq!(out["type"], "beatClock");
        assert_eq!(out["bpm"], 128.0);
        assert_eq!(out["phaseAt"], NOW - 300.0, "no sentAt → passed through");
        assert_eq!(out["serverTime"], NOW);
        assert!(out.get("evil").is_none(), "unknown keys never relayed");
    }

    #[test]
    fn beat_clock_phase_is_rebased_onto_the_server_clock() {
        // Desktop clock runs 5 s fast: it says the beat was 300 ms before it sent.
        let skew = 5_000.0;
        let m = json!({"bpm":120,"phaseAt":NOW + skew - 300.0,"confidence":1,"source":"file","sentAt":NOW + skew});
        let out = parse(&validate_beat_clock(&m, NOW).unwrap());
        assert_eq!(out["phaseAt"], NOW - 300.0, "beat lands 300 ms before receipt on the server clock");
        let unlocked = json!({"bpm":120,"phaseAt":0,"confidence":0,"source":"tap","sentAt":NOW});
        assert_eq!(parse(&validate_beat_clock(&unlocked, NOW).unwrap())["phaseAt"], 0.0, "0 = not locked stays 0");
    }

    #[test]
    fn beat_clock_rejects_out_of_range_and_non_finite_fields() {
        let ok = json!({"bpm":120,"phaseAt":NOW,"confidence":0.5,"source":"spotify"});
        assert!(validate_beat_clock(&ok, NOW).is_ok());
        let cases: Vec<(Value, &str)> = vec![
            (json!({"bpm":39.9,"phaseAt":NOW,"confidence":0.5,"source":"tap"}), "bpm"),
            (json!({"bpm":220.1,"phaseAt":NOW,"confidence":0.5,"source":"tap"}), "bpm"),
            (json!({"bpm":"120","phaseAt":NOW,"confidence":0.5,"source":"tap"}), "bpm"),
            (json!({"phaseAt":NOW,"confidence":0.5,"source":"tap"}), "bpm"),
            (json!({"bpm":120,"phaseAt":-1,"confidence":0.5,"source":"tap"}), "phaseAt"),
            (json!({"bpm":120,"phaseAt":NOW,"confidence":1.01,"source":"tap"}), "confidence"),
            (json!({"bpm":120,"phaseAt":NOW,"confidence":-0.1,"source":"tap"}), "confidence"),
            (json!({"bpm":120,"phaseAt":NOW,"confidence":0.5,"source":"mic"}), "source"),
            (json!({"bpm":120,"phaseAt":NOW,"confidence":0.5,"source":7}), "source"),
            (json!({"bpm":120,"phaseAt":NOW,"confidence":0.5,"source":"tap","sentAt":"x"}), "sentAt"),
            (json!({"bpm":120,"phaseAt":NOW,"confidence":0.5,"source":"tap","sentAt":-5}), "sentAt"),
        ];
        for (m, field) in cases {
            assert_eq!(validate_beat_clock(&m, NOW), Err(RelayReject::Field(field)), "{m}");
        }
        // serde_json cannot represent NaN/∞ from JSON text; a 1e400 literal parses
        // to ∞ only through f64 overflow, which serde rejects at parse time.
        assert!(serde_json::from_str::<Value>(r#"{"bpm":1e400}"#).is_err());
    }

    #[test]
    fn memory_route_matches_the_headset_parser_rules() {
        let m = json!({"type":"memoryRoute","snapshotId":"s-1","seq":7,"sentAt":NOW - 5.0,
            "path":[3,1,4],"sidecar":[2,-1,"x",9.5,5],"query":"q".repeat(200),"x":1});
        let out = parse(&validate_memory_route(&m, NOW).unwrap());
        assert_eq!(out["path"], json!([3, 1, 4]));
        assert_eq!(out["sidecar"], json!([2, 5]), "bad sidecar marks dropped, like parse_route");
        assert_eq!(out["query"].as_str().unwrap().chars().count(), MAX_ROUTE_QUERY_CHARS);
        assert_eq!(out["seq"], 7.0);
        assert_eq!(out["sentAt"], NOW - 5.0);
        assert_eq!(out["serverTime"], NOW);
        assert!(out.get("x").is_none(), "unknown keys never relayed");
        let clear = json!({"snapshotId":"s-1","path":[]});
        let c = parse(&validate_memory_route(&clear, NOW).unwrap());
        assert_eq!(c["path"], json!([]), "empty path clears");
        assert_eq!((c["seq"].clone(), c["sentAt"].clone(), c["query"].clone()), (json!(0.0), json!(0.0), json!("")));

        let too_long: Vec<u32> = (0..=MAX_ROUTE_PATH as u32).collect();
        let bad: Vec<(Value, &str)> = vec![
            (json!({"path":[1,2]}), "snapshotId"),
            (json!({"snapshotId":"   ","path":[1,2]}), "snapshotId"),
            (json!({"snapshotId":"x".repeat(MAX_RELAY_ID_LEN + 1),"path":[1,2]}), "snapshotId"),
            (json!({"snapshotId":"s"}), "path"),
            (json!({"snapshotId":"s","path":too_long}), "path"),
            (json!({"snapshotId":"s","path":[1,-2]}), "path"),
            (json!({"snapshotId":"s","path":[1,2.5]}), "path"),
            (json!({"snapshotId":"s","path":["1"]}), "path"),
            (json!({"snapshotId":"s","path":[1],"sidecar":"2"}), "sidecar"),
            (json!({"snapshotId":"s","path":[1],"query":5}), "query"),
            (json!({"snapshotId":"s","path":[1],"seq":-1}), "seq"),
            (json!({"snapshotId":"s","path":[1],"sentAt":"now"}), "sentAt"),
        ];
        for (m, field) in bad {
            assert_eq!(validate_memory_route(&m, NOW), Err(RelayReject::Field(field)), "{m}");
        }
        assert_eq!(
            validate(RelayKind::MemoryRoute, MAX_RELAY_FRAME_BYTES + 1, &json!({}), NOW),
            Err(RelayReject::TooLarge(MAX_RELAY_FRAME_BYTES + 1))
        );
    }

    /// The headset parser (`xr-client/rust/src/memory_route.rs`) reads the same
    /// cases in `tests/memory_route_relay_parity.rs`, so the two cannot drift.
    #[test]
    fn relay_agrees_with_the_headset_parser_on_the_shared_cases() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("xr-client/rust/tests/fixtures/memory_route_cases.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        for c in v["cases"].as_array().unwrap() {
            let ok = validate_memory_route(&c["frame"], NOW).is_ok();
            assert_eq!(ok, c["accept"].as_bool().unwrap(), "{}", c["frame"]);
        }
    }

    #[test]
    fn the_largest_valid_route_fits_the_frame_cap() {
        let path: Vec<u32> = (0..MAX_ROUTE_PATH as u32).map(|i| u32::MAX - i).collect();
        let text = json!({"type":"memoryRoute","snapshotId":"s".repeat(MAX_RELAY_ID_LEN),"seq":u32::MAX,
            "sentAt":NOW,"path":path.clone(),"sidecar":path,"query":"€".repeat(MAX_ROUTE_QUERY_CHARS)}).to_string();
        assert!(text.len() <= MAX_RELAY_FRAME_BYTES, "{} bytes", text.len());
    }

    #[test]
    fn throttle_caps_at_four_hz_and_always_delivers_the_last_state() {
        let mut t = RelayThrottle::default();
        assert_eq!(t.offer(0.0, "a".into()), ThrottleDecision::Send("a".into()));
        assert_eq!(t.offer(100.0, "b".into()), ThrottleDecision::Schedule { delay_ms: 150.0 });
        assert_eq!(t.offer(180.0, "c".into()), ThrottleDecision::Coalesced);
        assert_eq!(t.offer(240.0, "d".into()), ThrottleDecision::Coalesced);
        assert_eq!(t.flush(250.0), Some("d".into()), "the newest pending frame flushes");
        // The flush counts as a send: the next frame inside 250 ms is held again.
        assert!(matches!(t.offer(300.0, "e".into()), ThrottleDecision::Schedule { .. }));
        assert_eq!(t.flush(500.0), Some("e".into()));
        assert_eq!(t.offer(2500.0, "hb".into()), ThrottleDecision::Send("hb".into()), "2 s heartbeat passes");
        // Simulate 10 s of a 60 Hz sender: no more than 4 relays per second.
        let mut t = RelayThrottle::default();
        let mut sent = 0;
        let mut flush_at: Option<f64> = None;
        for i in 0..600 {
            let now = i as f64 * (1000.0 / 60.0);
            if let Some(at) = flush_at {
                if now >= at {
                    if t.flush(now).is_some() {
                        sent += 1;
                    }
                    flush_at = None;
                }
            }
            match t.offer(now, format!("{i}")) {
                ThrottleDecision::Send(_) => sent += 1,
                ThrottleDecision::Schedule { delay_ms } => flush_at = Some(now + delay_ms),
                ThrottleDecision::Coalesced => {}
            }
        }
        assert!(sent <= 41, "{sent} relays in 10 s");
        assert!(sent >= 35, "{sent} relays in 10 s");
    }

    #[test]
    fn kinds_round_trip() {
        for k in [RelayKind::BeatClock, RelayKind::MemoryRoute] {
            assert_eq!(RelayKind::from_type(k.as_str()), Some(k));
        }
        assert_eq!(RelayKind::from_type("ping"), None);
    }
}
