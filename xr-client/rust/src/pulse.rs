//! Godot-facing glue for the beat clock and memory bursts (WP3/WP5/WP8).
//!
//! `BeatPulse` owns a [`crate::beat::BeatSync`] (relayed desktop clock, local
//! tap tempo, clock offset) and an opt-in [`crate::beat::MicBeat`]; the scene
//! feeds it `/wss` text frames, controller taps and captured mic frames, and
//! reads back one pulse intensity per frame for the halo, edge and burst
//! shaders. `MemoryFlashCodec` decodes `memory_flash` frames into burst
//! descriptors with the desktop's semantic colour and shape.
//!
//! All decision logic lives in the pure modules (`beat`, `semantic`) so it is
//! unit-tested headless; this file only converts types and owns the mic
//! analysis worker thread. Mic samples exist only in memory (the analyser's
//! rolling window and one analysis snapshot) and are never written or sent.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use std::sync::mpsc::{channel, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

use godot::prelude::*;

use crate::beat::{self, ActiveSource, BeatSync, MicBeat, TempoEstimate};
use crate::semantic;

fn epoch_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

struct PendingAnalysis {
    rx: Receiver<TempoEstimate>,
    start_ms: f64,
}

#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct BeatPulse {
    sync: BeatSync,
    mic: MicBeat,
    pending: Option<PendingAnalysis>,
    last_pong_ms: f64,
    base: Base<RefCounted>,
}

#[godot_api]
impl BeatPulse {
    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            sync: BeatSync::new(),
            mic: MicBeat::new(),
            pending: None,
            last_pong_ms: 0.0,
            base,
        })
    }

    /// Offer a `/wss` text frame. Returns `"beatClock"` or `"pong"` when the
    /// frame was consumed here, `""` otherwise (the scene routes it on).
    #[func]
    fn handle_text(&mut self, json: GString) -> GString {
        let text = json.to_string();
        let now = epoch_ms();
        if let Some(frame) = beat::parse_beat_clock_frame(&text) {
            self.sync.on_remote(frame, now);
            return "beatClock".into();
        }
        if let Some((sent, server)) = beat::parse_pong(&text) {
            if self.sync.offset.add_sample(sent, server, now) {
                self.last_pong_ms = now;
            }
            return "pong".into();
        }
        GString::new()
    }

    /// The JSON ping to send on the graph socket (stamped with local epoch ms).
    #[func]
    fn make_ping(&self) -> GString {
        beat::build_ping(epoch_ms()).into()
    }

    /// Register a tap-tempo tap now. True once a tempo reading exists.
    #[func]
    fn tap(&mut self) -> bool {
        self.sync.tap(epoch_ms()).is_some()
    }

    #[func]
    fn tap_count(&self) -> i64 {
        self.sync.tap_count() as i64
    }

    /// Opt-in mic analysis. Off drops every buffered sample immediately.
    #[func]
    fn set_mic_enabled(&mut self, on: bool) {
        self.mic.set_enabled(on);
        self.sync.set_mic_listening(on);
        if !on {
            self.pending = None;
        }
    }

    #[func]
    fn mic_enabled(&self) -> bool {
        self.mic.enabled()
    }

    /// Feed stereo frames from `AudioEffectCapture.get_buffer`, mixed to mono.
    #[func]
    fn push_mic_frames(&mut self, frames: PackedVector2Array, mix_rate: f32) {
        if !self.mic.enabled() {
            return;
        }
        let mono: Vec<f32> = frames.as_slice().iter().map(|v| 0.5 * (v.x + v.y)).collect();
        self.mic.push(&mono, mix_rate as f64, epoch_ms());
    }

    /// Per-frame housekeeping: collect a finished mic analysis and start the
    /// next one on a worker thread when due.
    #[func]
    fn process(&mut self) {
        let now = epoch_ms();
        if let Some(p) = &self.pending {
            if let Ok(est) = p.rx.try_recv() {
                let start = p.start_ms;
                self.pending = None;
                self.mic.apply_estimate(est, start, now);
            }
        }
        if self.pending.is_none() && self.mic.due(now) {
            let (data, rate, start_ms) = self.mic.take_snapshot(now);
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(beat::analyse_window(&data, rate));
            });
            self.pending = Some(PendingAnalysis { rx, start_ms });
        }
        let lock = self.mic.lock(now);
        self.sync.set_mic_clock(lock);
    }

    /// Pulse intensity 0..1 now; exactly 0 under reduced motion (ADR-2107).
    #[func]
    fn pulse(&self, reduced_motion: bool) -> f32 {
        self.sync.pulse_intensity(epoch_ms(), reduced_motion) as f32
    }

    /// `{active, on, bpm, confidence, phase, bar, mic_state, rtt_ms, offset_ms, synced}`.
    #[func]
    fn status(&self) -> Dictionary {
        let now = epoch_ms();
        let (src, st, s) = self.sync.sample(now);
        let mut d = Dictionary::new();
        d.set("active", src.as_str());
        d.set("on", s.on);
        d.set("bpm", st.bpm);
        d.set("confidence", st.confidence);
        d.set("phase", s.phase);
        d.set("bar", s.bar);
        d.set("mic_state", self.mic.state(now).as_str());
        d.set("rtt_ms", self.sync.offset.rtt_ms().unwrap_or(-1.0));
        d.set("offset_ms", self.sync.offset.offset_ms().unwrap_or(0.0));
        d.set("synced", self.sync.offset.offset_ms().is_some());
        d
    }

    /// One HUD line: source, bpm and confidence, e.g. `desktop · 120.0 bpm · 82%`.
    #[func]
    fn status_line(&self) -> GString {
        let now = epoch_ms();
        let (src, st, s) = self.sync.sample(now);
        let mic = self.mic.state(now);
        let mic_txt = match mic {
            beat::MicState::Off => String::new(),
            m => format!(" · mic {}", m.as_str()),
        };
        if src == ActiveSource::None || !s.on {
            let taps = self.sync.tap_count();
            let hint = if taps > 0 { format!("{taps} tap{}", if taps == 1 { "" } else { "s" }) } else { "off".into() };
            return format!("Beat: {hint}{mic_txt}").into();
        }
        format!("Beat: {} · {:.1} bpm · {:.0}%{mic_txt}", src.as_str(), st.bpm, st.confidence * 100.0).into()
    }
}

/// Static decoder for `memory_flash` frames.
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct MemoryFlashCodec {
    base: Base<RefCounted>,
}

#[godot_api]
impl MemoryFlashCodec {
    /// Decode a `memory_flash` frame (single or batch `data`) into burst
    /// descriptors: `{key, namespace, action, color: Color, max_scale,
    /// duration, implode: bool, rings}`. Empty for anything else.
    #[func]
    fn parse(json: GString) -> VariantArray {
        let mut out = VariantArray::new();
        for f in semantic::parse_memory_flash(&json.to_string()) {
            let p = semantic::memory_action_profile(&f.action);
            let [r, g, b] = semantic::hex_to_rgb01(semantic::semantic_burst_color(&f.action, &f.namespace));
            let mut d = Dictionary::new();
            d.set("key", f.key.as_str());
            d.set("namespace", f.namespace.as_str());
            d.set("action", f.action.as_str());
            d.set("color", Color::from_rgb(r, g, b));
            d.set("max_scale", p.max_scale);
            d.set("duration", p.duration);
            d.set("implode", p.motion == semantic::BurstMotion::Implode);
            d.set("rings", p.rings as i64);
            out.push(&d.to_variant());
        }
        out
    }
}
