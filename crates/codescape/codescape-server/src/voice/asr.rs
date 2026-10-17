//! nemotron streaming ASR client (`agentbox/voice/nemotron/server.py`).
//!
//! Wire: msgpack over WebSocket `/api/asr-streaming`.
//!
//! * Client → server: `{type:"Audio", pcm:[f32; 1920]}` (80 ms at 24 kHz) and
//!   `{type:"Marker", id}`.
//! * Server → client: `Ready`, `Word{text,start_time}`, `Step{step_idx,prs}`,
//!   `Marker{id}`, `Error{message}`.
//!
//! The server echoes a Marker only after another 0.5 s of model audio arrives
//! behind it, so [`AsrSession::finish`] sends the Marker followed by
//! [`MARKER_FLUSH_FRAMES`] frames of silence. The echo is emitted when the model
//! *pulls* the Marker, which is before the text of the preceding audio has
//! streamed out, and the cache-aware decoder only finishes its lookahead as more
//! audio arrives. So after release the session keeps sending real-time silence,
//! and the transcript is final once the Marker echo (or the server's silence
//! endpoint) has been seen and no word has arrived for [`TRAILING_QUIET_MS`], or
//! after [`FINAL_CAP_MS`] at most ([`Finaliser`]). nemotron is single-stream: a
//! second consumer gets `Error("ASR is serving another stream")`, surfaced as
//! [`AsrUpdate::Busy`]. One connection is opened per utterance, so the model is
//! held only while the push-to-talk button is.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::egress::{EgressAllowlist, EgressError};

/// Input sample rate the ASR expects.
pub const INPUT_RATE: u32 = 24_000;
/// Samples per 80 ms frame.
pub const FRAME_SAMPLES: usize = 1920;
/// Silence frames sent after a Marker (8 × 80 ms = 640 ms > the 0.5 s flush the server waits for).
pub const MARKER_FLUSH_FRAMES: usize = 8;
/// Hard cap from release to the final transcript, in ms.
pub const FINAL_CAP_MS: u64 = 3_000;
/// Quiet window: once settled, the transcript is final after this long without a new word, in ms.
pub const TRAILING_QUIET_MS: u64 = 500;
/// `prs[2]` at or above this is the server's end-of-speech signal.
pub const ENDPOINT_PROBABILITY: f32 = 0.95;

/// A message to the ASR.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum AsrRequest<'a> {
    /// One frame of mono f32 PCM at 24 kHz.
    Audio {
        /// Samples.
        pcm: &'a [f32],
    },
    /// End-of-utterance marker, echoed once the audio before it is decoded.
    Marker {
        /// Caller-chosen id.
        id: i64,
    },
}

/// A message from the ASR.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum AsrEvent {
    /// Session accepted.
    Ready,
    /// A decoded text fragment (fragments carry their own leading spaces).
    Word {
        /// Fragment.
        text: String,
        /// Seconds since session start.
        #[serde(default)]
        start_time: f64,
    },
    /// Per-frame step with `prs[2]` = silence probability.
    Step {
        /// Frame index.
        step_idx: i64,
        /// Probabilities.
        prs: Vec<f32>,
    },
    /// Marker echo.
    Marker {
        /// The id sent.
        id: i64,
    },
    /// Server-side failure or contention.
    Error {
        /// Message.
        message: String,
    },
}

/// Encode a request as a msgpack map (field names kept, as Python's `msgpack.unpackb` expects).
pub fn encode_request(req: &AsrRequest<'_>) -> Vec<u8> {
    rmp_serde::to_vec_named(req).expect("ASR request serialises")
}

/// Decode a server message.
pub fn decode_event(bytes: &[u8]) -> Result<AsrEvent, String> {
    rmp_serde::from_slice(bytes).map_err(|e| format!("bad ASR message: {e}"))
}

/// `true` for the contention error nemotron sends when another stream holds the model.
pub fn is_busy_error(message: &str) -> bool {
    message.contains("another stream")
}

/// Cuts arbitrary-length PCM into exact 1920-sample frames.
#[derive(Debug, Clone, Default)]
pub struct FrameSlicer {
    pending: Vec<f32>,
}

impl FrameSlicer {
    /// Append samples; returns every complete frame.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend(samples.iter().map(|s| {
            if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
        let whole = self.pending.len() / FRAME_SAMPLES;
        let frames = self
            .pending
            .as_chunks::<FRAME_SAMPLES>()
            .0
            .iter()
            .map(|c| c.to_vec())
            .collect();
        self.pending.drain(..whole * FRAME_SAMPLES);
        frames
    }

    /// The remainder zero-padded to a full frame, if any samples are pending.
    pub fn flush(&mut self) -> Option<Vec<f32>> {
        if self.pending.is_empty() {
            return None;
        }
        let mut f = std::mem::take(&mut self.pending);
        f.resize(FRAME_SAMPLES, 0.0);
        Some(f)
    }

    /// Samples waiting for a full frame.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

/// Decode little-endian f32 PCM from a browser binary frame (trailing partial sample dropped).
pub fn f32le_samples(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// Assemble fragments into a transcript.
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    raw: String,
}

impl Transcript {
    /// Append a fragment.
    pub fn push(&mut self, fragment: &str) {
        self.raw.push_str(fragment);
    }
    /// Whitespace-normalised text so far.
    pub fn text(&self) -> String {
        self.raw.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// When a released utterance is final. Times are ms on any monotonic clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finaliser {
    cap: u64,
    quiet_until: u64,
    settled: bool,
}

impl Finaliser {
    /// Released at `now`.
    pub fn new(now: u64) -> Self {
        Self {
            cap: now + FINAL_CAP_MS,
            quiet_until: now + TRAILING_QUIET_MS,
            settled: false,
        }
    }
    /// A word arrived: extend the quiet window.
    pub fn on_word(&mut self, now: u64) {
        self.quiet_until = now + TRAILING_QUIET_MS;
    }
    /// The Marker echo or the silence endpoint arrived.
    pub fn on_settled(&mut self, now: u64) {
        self.settled = true;
        self.quiet_until = self.quiet_until.max(now + TRAILING_QUIET_MS);
    }
    /// Is the transcript final at `now`?
    pub fn due(&self, now: u64) -> bool {
        now >= self.cap || (self.settled && now >= self.quiet_until)
    }
    /// When to check again.
    pub fn next_wake_ms(&self) -> u64 {
        if self.settled {
            self.quiet_until.min(self.cap)
        } else {
            self.cap
        }
    }
}

/// What the session reports to the voice route.
#[derive(Debug, Clone, PartialEq)]
pub enum AsrUpdate {
    /// Transcript so far.
    Partial(String),
    /// Final transcript (possibly empty).
    Final(String),
    /// The shared ASR is serving another stream.
    Busy,
    /// Connection or protocol failure.
    Failed(String),
}

/// Errors starting a session.
#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    /// The URL is not allowed.
    #[error(transparent)]
    Egress(#[from] EgressError),
    /// Connect failed or timed out.
    #[error("ASR connect: {0}")]
    Connect(String),
}

enum Input {
    Audio(Vec<f32>),
    Finish,
}

/// One utterance's connection. Dropping it without [`AsrSession::finish`] abandons the utterance.
pub struct AsrSession {
    tx: mpsc::Sender<Input>,
}

impl AsrSession {
    /// Connect (after the egress check) and wait for `Ready`; updates go to `updates`.
    pub async fn start(
        allow: &EgressAllowlist,
        url: &str,
        updates: mpsc::Sender<AsrUpdate>,
    ) -> Result<Self, AsrError> {
        allow.check(url)?;
        let (ws, _) = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(url),
        )
        .await
        .map_err(|_| AsrError::Connect("timed out".into()))?
        .map_err(|e| AsrError::Connect(e.to_string()))?;
        let (mut sink, mut stream) = ws.split();
        // Wait for Ready (or an early Error).
        let ready = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Binary(b)) => match decode_event(&b) {
                        Ok(AsrEvent::Ready) => return Ok(()),
                        Ok(AsrEvent::Error { message }) => return Err(message),
                        _ => continue,
                    },
                    Ok(Message::Close(_)) | Err(_) => return Err("closed before Ready".to_string()),
                    _ => continue,
                }
            }
            Err("closed before Ready".to_string())
        })
        .await
        .map_err(|_| AsrError::Connect("no Ready within 5 s".into()))?;
        if let Err(m) = ready {
            if is_busy_error(&m) {
                let _ = updates.send(AsrUpdate::Busy).await;
            }
            return Err(AsrError::Connect(m));
        }

        let (tx, mut rx) = mpsc::channel::<Input>(256);
        tokio::spawn(async move {
            let mut slicer = FrameSlicer::default();
            let mut transcript = Transcript::default();
            let marker_id: i64 = 1;
            let t0 = tokio::time::Instant::now();
            let ms = move || t0.elapsed().as_millis() as u64;
            let mut fin: Option<Finaliser> = None;
            let silence = encode_request(&AsrRequest::Audio {
                pcm: &[0f32; FRAME_SAMPLES],
            });
            let mut pad = tokio::time::interval(Duration::from_millis(80));
            pad.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                let wake = fin
                    .as_ref()
                    .map(|f| t0 + Duration::from_millis(f.next_wake_ms()));
                let sleep = async {
                    match wake {
                        Some(d) => tokio::time::sleep_until(d).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    inp = rx.recv(), if fin.is_none() => match inp {
                        Some(Input::Audio(samples)) => {
                            for f in slicer.push(&samples) {
                                if sink.send(Message::Binary(encode_request(&AsrRequest::Audio { pcm: &f }))).await.is_err() {
                                    let _ = updates.send(AsrUpdate::Failed("ASR socket closed".into())).await;
                                    return;
                                }
                            }
                        }
                        Some(Input::Finish) | None => {
                            let mut out: Vec<Vec<u8>> = slicer.flush().iter().map(|f| encode_request(&AsrRequest::Audio { pcm: f })).collect();
                            out.push(encode_request(&AsrRequest::Marker { id: marker_id }));
                            out.extend(std::iter::repeat_n(silence.clone(), MARKER_FLUSH_FRAMES));
                            for b in out {
                                if sink.send(Message::Binary(b)).await.is_err() {
                                    break;
                                }
                            }
                            fin = Some(Finaliser::new(ms()));
                        }
                    },
                    _ = pad.tick(), if fin.is_some() => {
                        // The cache-aware model decodes its lookahead only as more audio arrives:
                        // keep real-time silence flowing until the transcript is final.
                        let _ = sink.send(Message::Binary(silence.clone())).await;
                    }
                    msg = stream.next() => match msg {
                        Some(Ok(Message::Binary(b))) => match decode_event(&b) {
                            Ok(AsrEvent::Word { text, .. }) => {
                                transcript.push(&text);
                                if let Some(f) = fin.as_mut() {
                                    f.on_word(ms());
                                }
                                let _ = updates.send(AsrUpdate::Partial(transcript.text())).await;
                            }
                            Ok(AsrEvent::Marker { id }) if id == marker_id => {
                                if let Some(f) = fin.as_mut() {
                                    f.on_settled(ms());
                                }
                            }
                            Ok(AsrEvent::Step { prs, .. }) => {
                                if let Some(f) = fin.as_mut() {
                                    if prs.get(2).copied().unwrap_or(0.0) >= ENDPOINT_PROBABILITY {
                                        f.on_settled(ms());
                                    }
                                }
                            }
                            Ok(AsrEvent::Error { message }) => {
                                let u = if is_busy_error(&message) { AsrUpdate::Busy } else { AsrUpdate::Failed(message) };
                                let _ = updates.send(u).await;
                                let _ = sink.close().await;
                                return;
                            }
                            Ok(_) => {}
                            Err(e) => tracing::debug!(error = %e, "ignoring ASR message"),
                        },
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                            let text = transcript.text();
                            let u = if fin.is_none() && text.is_empty() { AsrUpdate::Failed("ASR closed the stream".into()) } else { AsrUpdate::Final(text) };
                            let _ = updates.send(u).await;
                            return;
                        }
                        Some(Ok(_)) => {}
                    },
                    _ = sleep => {
                        if fin.as_ref().is_some_and(|f| f.due(ms())) {
                            let _ = updates.send(AsrUpdate::Final(transcript.text())).await;
                            let _ = sink.close().await;
                            return;
                        }
                    }
                }
            }
        });
        Ok(Self { tx })
    }

    /// Queue PCM samples (any length; sliced into 80 ms frames).
    pub async fn send_audio(&self, samples: Vec<f32>) -> bool {
        self.tx.send(Input::Audio(samples)).await.is_ok()
    }

    /// End the utterance: flush, Marker, silence; [`AsrUpdate::Final`] follows.
    pub async fn finish(&self) {
        let _ = self.tx.send(Input::Finish).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_encodes_as_a_named_msgpack_map_of_float32() {
        let pcm = vec![0.5f32; FRAME_SAMPLES];
        let bytes = encode_request(&AsrRequest::Audio { pcm: &pcm });
        let v: rmpv::Value = rmpv::decode::read_value(&mut &bytes[..]).unwrap();
        let map = v
            .as_map()
            .expect("a map, as server.py's message.get() needs");
        let get = |k: &str| {
            map.iter()
                .find(|(kk, _)| kk.as_str() == Some(k))
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("type").unwrap().as_str(), Some("Audio"));
        let arr = get("pcm").unwrap();
        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1920);
        assert!(
            matches!(arr[0], rmpv::Value::F32(x) if x == 0.5),
            "pcm elements are float32"
        );
        let m = encode_request(&AsrRequest::Marker { id: 7 });
        let v: rmpv::Value = rmpv::decode::read_value(&mut &m[..]).unwrap();
        assert_eq!(v.as_map().unwrap().len(), 2);
    }

    /// Bytes shaped like `msgpack.packb(message, use_bin_type=True, use_single_float=True)`.
    fn python_pack(entries: Vec<(&str, rmpv::Value)>) -> Vec<u8> {
        let v = rmpv::Value::Map(
            entries
                .into_iter()
                .map(|(k, v)| (rmpv::Value::from(k), v))
                .collect(),
        );
        let mut out = vec![];
        rmpv::encode::write_value(&mut out, &v).unwrap();
        out
    }

    #[test]
    fn server_messages_decode() {
        assert_eq!(
            decode_event(&python_pack(vec![("type", "Ready".into())])).unwrap(),
            AsrEvent::Ready
        );
        assert_eq!(
            decode_event(&python_pack(vec![
                ("type", "Word".into()),
                ("text", " hello".into()),
                ("start_time", rmpv::Value::F32(0.25))
            ]))
            .unwrap(),
            AsrEvent::Word {
                text: " hello".into(),
                start_time: 0.25
            }
        );
        assert_eq!(
            decode_event(&python_pack(vec![
                ("type", "Step".into()),
                ("step_idx", 12.into()),
                (
                    "prs",
                    rmpv::Value::Array(vec![
                        rmpv::Value::F32(0.0),
                        rmpv::Value::F32(0.0),
                        rmpv::Value::F32(0.95)
                    ])
                ),
            ]))
            .unwrap(),
            AsrEvent::Step {
                step_idx: 12,
                prs: vec![0.0, 0.0, 0.95]
            }
        );
        assert_eq!(
            decode_event(&python_pack(vec![
                ("type", "Marker".into()),
                ("id", 1.into())
            ]))
            .unwrap(),
            AsrEvent::Marker { id: 1 }
        );
        let busy = decode_event(&python_pack(vec![
            ("type", "Error".into()),
            ("message", "ASR is serving another stream".into()),
        ]))
        .unwrap();
        assert!(matches!(&busy, AsrEvent::Error { message } if is_busy_error(message)));
        assert!(decode_event(&python_pack(vec![("type", "Nope".into())])).is_err());
    }

    #[test]
    fn slicer_emits_exact_frames_and_pads_the_tail() {
        let mut s = FrameSlicer::default();
        assert!(s.push(&[0.1; 1000]).is_empty());
        let frames = s.push(&[0.1; 3000]);
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().all(|f| f.len() == FRAME_SAMPLES));
        assert_eq!(s.pending(), 4000 - 2 * FRAME_SAMPLES);
        let tail = s.flush().unwrap();
        assert_eq!(tail.len(), FRAME_SAMPLES);
        assert_eq!(tail[159], 0.1);
        assert_eq!(tail[160], 0.0);
        assert!(s.flush().is_none());
        let f = s.push(&[f32::NAN, 4.0, -4.0]);
        assert!(f.is_empty());
        assert_eq!(
            s.flush().unwrap()[..3],
            [0.0, 1.0, -1.0],
            "non-finite zeroed, clipped to ±1"
        );
    }

    /// Live round trip on the compose network: pocket-tts speaks a phrase, the s16le PCM
    /// is converted to f32 and streamed through [`AsrSession`], and the Marker-finalised
    /// transcript must contain the words. Run with `cargo test -- --ignored live_`.
    #[tokio::test]
    #[ignore = "needs pocket-tts and nemotron-asr on the network"]
    async fn live_tts_to_asr_round_trip() {
        use futures_util::StreamExt;
        let allow = EgressAllowlist::from_urls([
            "http://pocket-tts:8000",
            "ws://nemotron-asr:8000/api/asr-streaming",
        ])
        .unwrap();
        let tts = crate::voice::tts::TtsClient::new(&allow, "http://pocket-tts:8000", "alba");
        let mut pcm = vec![];
        // nemotron's TextIteratorStreamer holds the text after the last space until generation
        // ends (only on disconnect), so the final word of an utterance is never delivered
        // (server.py defect, reported). A trailing filler keeps the command words testable.
        let phrase = std::env::var("CODESCAPE_LIVE_PHRASE")
            .unwrap_or_else(|_| "Show callers of main please.".into());
        let mut s = tts.speak(&phrase).await.unwrap();
        while let Some(c) = s.next().await {
            pcm.extend(
                c.unwrap()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0),
            );
        }
        assert!(pcm.len() > 12_000, "at least half a second of speech");
        let (tx, mut rx) = mpsc::channel(64);
        let started = std::time::Instant::now();
        let session = AsrSession::start(&allow, "ws://nemotron-asr:8000/api/asr-streaming", tx)
            .await
            .unwrap();
        // Real time, as the browser worklet delivers it (80 ms frames).
        for chunk in pcm.chunks(FRAME_SAMPLES) {
            assert!(session.send_audio(chunk.to_vec()).await);
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
        session.finish().await;
        let mut last = None;
        while let Ok(Some(u)) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
            match u {
                AsrUpdate::Partial(p) => println!("partial at {:?}: {p:?}", started.elapsed()),
                other => {
                    last = Some(other);
                    break;
                }
            }
        }
        let text = match last {
            Some(AsrUpdate::Final(t)) => t,
            Some(AsrUpdate::Busy) => panic!("ASR busy with another stream; retry"),
            other => panic!("expected a final transcript, got {other:?}"),
        };
        println!("transcript after {:?}: {text:?}", started.elapsed());
        let lower = text.to_lowercase();
        assert!(
            lower.contains("callers") && lower.contains("main"),
            "{text}"
        );
    }

    #[test]
    fn finaliser_waits_for_settle_and_quiet() {
        let mut f = Finaliser::new(0);
        assert!(!f.due(600), "not settled yet: words may still be decoding");
        f.on_word(700);
        f.on_settled(800); // marker echo
        assert!(!f.due(1_000));
        f.on_word(1_100); // trailing word after the echo
        assert!(!f.due(1_500));
        assert!(f.due(1_600));
        assert_eq!(f.next_wake_ms(), 1_600);
        let never = Finaliser::new(0);
        assert!(never.due(FINAL_CAP_MS), "hard cap");
    }

    #[test]
    fn pcm_bytes_and_transcript() {
        let b: Vec<u8> = [0.5f32, -0.25]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .chain([1u8])
            .collect();
        assert_eq!(f32le_samples(&b), vec![0.5, -0.25]);
        let mut t = Transcript::default();
        t.push(" focus");
        t.push(" main");
        t.push(".rs ");
        assert_eq!(t.text(), "focus main.rs");
    }
}
