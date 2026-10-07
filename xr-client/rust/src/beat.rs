//! Beat clock for the headset — ports of the desktop memory explorer's
//! `memoryCloud/beatClock.ts` (`beatAt`, tap tempo) and `memoryCloud/beat.ts`
//! (`onsetEnvelope`, `estimateTempo`), plus the pieces only the headset needs:
//!
//! - [`ClockOffset`]: server-clock estimate from `/wss` JSON ping/pong round
//!   trips (`offset = serverTime − (sent + rtt/2)`), keeping the sample with the
//!   smallest round trip so an asymmetric or congested trip cannot skew it;
//! - [`BeatSync`]: arbitration between the desktop's relayed clock
//!   (`beatClock` text frames, phase in server-epoch ms), a local tap tempo and
//!   the opt-in microphone, with a staleness timeout on the relayed clock;
//! - [`MicBeat`]: a rolling analysis window over captured microphone samples
//!   that only reports a lock at high confidence, twice in agreement, so the
//!   wearer's speech does not latch a false tempo. Samples are analysed in
//!   memory and discarded; nothing is recorded or transmitted.
//!
//! All functions take explicit millisecond clocks so they are unit-tested
//! against `tests/fixtures/desktop_parity.json`, which the desktop's own
//! functions write (`client/src/features/visualisation/__tests__/xrParityFixtures.test.ts`).

/// Wire sources (`BeatSource` in TS) plus the headset-local microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatSource {
    Off,
    File,
    Tap,
    Spotify,
    /// Headset microphone (WP8). Never sent on the wire.
    Mic,
}

impl BeatSource {
    /// Parse a wire source. `mic` is not a wire value and is rejected.
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "file" => Some(Self::File),
            "tap" => Some(Self::Tap),
            "spotify" => Some(Self::Spotify),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::File => "file",
            Self::Tap => "tap",
            Self::Spotify => "spotify",
            Self::Mic => "mic",
        }
    }
}

/// `{ bpm, phaseAt, confidence, source }`. `phase_at` is epoch ms of a beat in
/// whichever clock the owner states (server epoch for relayed frames, local
/// epoch for tap and mic); 0 means not locked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatClockState {
    pub bpm: f64,
    pub phase_at: f64,
    pub confidence: f64,
    pub source: BeatSource,
}

pub const DEFAULT_BPM: f64 = 120.0;
pub const OFF_BEAT: BeatClockState = BeatClockState { bpm: DEFAULT_BPM, phase_at: 0.0, confidence: 0.0, source: BeatSource::Off };

/// A pause longer than this starts a new tap sequence.
pub const TAP_RESET_MS: f64 = 2500.0;
const TAP_KEEP: usize = 9;
const TAP_MIN_BPM: f64 = 40.0;
const TAP_MAX_BPM: f64 = 220.0;
/// Pulse decay per beat: `exp(-phase · k)`.
const PULSE_DECAY: f64 = 6.0;

/// `Math.round` (half towards +∞), not Rust's half-away-from-zero.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

fn tenth(v: f64) -> f64 {
    js_round(v * 10.0) / 10.0
}

fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n >> 1]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// One tap-tempo reading (`TapReading`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapReading {
    pub bpm: f64,
    /// Epoch ms of the last tap — the phase lock.
    pub phase_at: f64,
    pub confidence: f64,
    pub taps: usize,
}

/// Tap tempo: median of the last eight intervals, reset after 2.5 s, phase
/// locked to the last tap (`createTapTempo`).
#[derive(Debug, Clone, Default)]
pub struct TapTempo {
    times: Vec<f64>,
}

impl TapTempo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tap; a reading once three taps are in, else `None`.
    pub fn tap(&mut self, at_ms: f64) -> Option<TapReading> {
        if let Some(&last) = self.times.last() {
            if at_ms - last > TAP_RESET_MS {
                self.times.clear();
            }
        }
        self.times.push(at_ms);
        if self.times.len() > TAP_KEEP {
            self.times.remove(0);
        }
        if self.times.len() < 3 {
            return None;
        }
        let n = self.times.len();
        let iv: Vec<f64> = (n.saturating_sub(8).max(1)..n).map(|i| self.times[i] - self.times[i - 1]).collect();
        let mut sorted = iv.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let m = median(&sorted);
        let mut dev: Vec<f64> = iv.iter().map(|d| (d - m).abs()).collect();
        dev.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mad = median(&dev);
        let bpm = tenth(60_000.0 / m).clamp(TAP_MIN_BPM, TAP_MAX_BPM);
        let confidence = (1.0 - (4.0 * mad) / m).clamp(0.0, 1.0) * (iv.len() as f64 / 4.0).min(1.0);
        Some(TapReading { bpm, phase_at: at_ms, confidence, taps: n })
    }

    pub fn reset(&mut self) {
        self.times.clear();
    }

    pub fn count(&self) -> usize {
        self.times.len()
    }

    /// Epoch ms of the most recent tap in the current sequence.
    pub fn last_tap(&self) -> Option<f64> {
        self.times.last().copied()
    }
}

/// The clock evaluated at an instant (`BeatSample`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatSample {
    pub on: bool,
    /// 0..1 position within the current beat.
    pub phase: f64,
    /// 1 on the beat, exponential decay through it.
    pub pulse: f64,
    /// `pulse` on the first beat of each 4-beat bar counted from `phase_at`, else 0.
    pub bar: f64,
    pub beat_index: i64,
}

pub const OFF_SAMPLE: BeatSample = BeatSample { on: false, phase: 0.0, pulse: 0.0, bar: 0.0, beat_index: 0 };

/// Evaluate a clock at `now_ms`, which must be in the same epoch as `phase_at`.
pub fn beat_at(state: &BeatClockState, now_ms: f64) -> BeatSample {
    // NaN fails both positivity checks, as the negated comparisons did.
    if state.source == BeatSource::Off
        || state.bpm.is_nan()
        || state.bpm <= 0.0
        || state.phase_at.is_nan()
        || state.phase_at <= 0.0
    {
        return OFF_SAMPLE;
    }
    let pos = (now_ms - state.phase_at) / (60_000.0 / state.bpm);
    let beat_index = (pos + 1e-9).floor();
    let phase = (pos - beat_index).max(0.0);
    let pulse = (-phase * PULSE_DECAY).exp();
    let bar = if (beat_index as i64).rem_euclid(4) == 0 { pulse } else { 0.0 };
    BeatSample { on: true, phase, pulse, bar, beat_index: beat_index as i64 }
}

// ─── wire: beatClock / pong text frames ──────────────────────────────────────

/// Validated `beatClock` relay frame: the clock (phase in server epoch) and the
/// server's relay time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteBeatFrame {
    pub state: BeatClockState,
    pub server_time: f64,
}

fn finite(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64()).filter(|x| x.is_finite())
}

/// Parse a relayed `beatClock` frame with the same bounds the server enforces
/// (bpm 40–220, confidence 0–1, phaseAt ≥ 0, source off|file|tap|spotify).
/// A frame without `serverTime` is accepted with `server_time = 0`.
pub fn parse_beat_clock_frame(json: &str) -> Option<RemoteBeatFrame> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("type")?.as_str()? != "beatClock" {
        return None;
    }
    let bpm = finite(v.get("bpm"))?;
    let phase_at = finite(v.get("phaseAt"))?;
    let confidence = finite(v.get("confidence"))?;
    let source = BeatSource::from_wire(v.get("source")?.as_str()?)?;
    if !(TAP_MIN_BPM..=TAP_MAX_BPM).contains(&bpm) || phase_at < 0.0 || !(0.0..=1.0).contains(&confidence) {
        return None;
    }
    let server_time = finite(v.get("serverTime")).unwrap_or(0.0);
    Some(RemoteBeatFrame { state: BeatClockState { bpm, phase_at, confidence, source }, server_time })
}

/// `{"type":"ping","timestamp":<local ms>}` — the `/wss` JSON ping the server
/// answers with `{"type":"pong","timestamp":<echo>,"serverTime":<server ms>}`.
pub fn build_ping(local_ms: f64) -> String {
    format!(r#"{{"type":"ping","timestamp":{}}}"#, local_ms.max(0.0).floor() as u64)
}

/// Parse a pong: `(echoed local send ms, server ms)`. A pong from a server that
/// does not stamp `serverTime` yields `None` (no offset can be derived).
pub fn parse_pong(json: &str) -> Option<(f64, f64)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("type")?.as_str()? != "pong" {
        return None;
    }
    Some((finite(v.get("timestamp"))?, finite(v.get("serverTime"))?))
}

// ─── clock offset ────────────────────────────────────────────────────────────

/// Server-clock estimate from ping/pong round trips. Each sample assumes the
/// server stamped its reply halfway through the trip; the sample with the
/// smallest round trip of the last [`ClockOffset::WINDOW`] is the least
/// distorted, so it alone sets the offset (the NTP minimum-delay filter).
#[derive(Debug, Clone, Default)]
pub struct ClockOffset {
    samples: Vec<(f64, f64)>, // (rtt_ms, offset_ms)
}

impl ClockOffset {
    pub const WINDOW: usize = 8;
    /// Round trips beyond this are discarded as useless for sync.
    pub const MAX_RTT_MS: f64 = 2000.0;

    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in one round trip. Returns false when the sample was rejected.
    pub fn add_sample(&mut self, sent_local_ms: f64, server_ms: f64, recv_local_ms: f64) -> bool {
        let rtt = recv_local_ms - sent_local_ms;
        if rtt.is_nan() || rtt < 0.0 || rtt > Self::MAX_RTT_MS || !server_ms.is_finite() {
            return false;
        }
        let offset = server_ms - (sent_local_ms + rtt / 2.0);
        self.samples.push((rtt, offset));
        if self.samples.len() > Self::WINDOW {
            self.samples.remove(0);
        }
        true
    }

    fn best(&self) -> Option<(f64, f64)> {
        self.samples
            .iter()
            .copied()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// `server_ms ≈ local_ms + offset_ms()`; `None` until a sample lands.
    pub fn offset_ms(&self) -> Option<f64> {
        self.best().map(|b| b.1)
    }

    /// Round trip of the sample in use.
    pub fn rtt_ms(&self) -> Option<f64> {
        self.best().map(|b| b.0)
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

// ─── arbitration ─────────────────────────────────────────────────────────────

/// The desktop heartbeats every 2 s; three missed heartbeats drop the relayed
/// clock so the headset never pulses to a desktop that has gone away.
pub const REMOTE_STALE_MS: f64 = 6500.0;

/// Which clock is driving the pulse right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveSource {
    None,
    Remote,
    Tap,
    Mic,
}

impl ActiveSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Remote => "desktop",
            Self::Tap => "tap",
            Self::Mic => "mic",
        }
    }
}

/// Arbitrates the relayed desktop clock, the local tap tempo and the mic:
/// - a mic lock wins while listening;
/// - a tap wins until the desktop's clock *changes* after it (heartbeats with
///   an unchanged clock do not take the beat back);
/// - otherwise a fresh relayed clock;
/// - otherwise the last tap clock, if any.
#[derive(Debug, Clone)]
pub struct BeatSync {
    pub offset: ClockOffset,
    tap: TapTempo,
    tap_clock: Option<BeatClockState>,
    last_tap_local: f64,
    remote: Option<BeatClockState>,
    remote_received_local: f64,
    remote_changed_local: f64,
    mic_clock: Option<BeatClockState>,
    mic_listening: bool,
}

impl Default for BeatSync {
    fn default() -> Self {
        Self::new()
    }
}

impl BeatSync {
    pub fn new() -> Self {
        Self {
            offset: ClockOffset::new(),
            tap: TapTempo::new(),
            tap_clock: None,
            last_tap_local: f64::NEG_INFINITY,
            remote: None,
            remote_received_local: f64::NEG_INFINITY,
            remote_changed_local: f64::NEG_INFINITY,
            mic_clock: None,
            mic_listening: false,
        }
    }

    /// Adopt a relayed frame received at `local_ms`. If the server stamped
    /// `serverTime` and no ping offset exists yet, a provisional offset from
    /// that stamp (one-way, so biased by the downlink latency) seeds it.
    pub fn on_remote(&mut self, frame: RemoteBeatFrame, local_ms: f64) {
        if self.offset.offset_ms().is_none() && frame.server_time > 0.0 {
            self.offset.add_sample(local_ms, frame.server_time, local_ms);
        }
        let changed = match self.remote {
            Some(prev) => {
                prev.source != frame.state.source
                    || (prev.bpm - frame.state.bpm).abs() > 1e-9
                    || (prev.phase_at - frame.state.phase_at).abs() > 0.5
            }
            None => true,
        };
        if changed {
            self.remote_changed_local = local_ms;
        }
        self.remote = Some(frame.state);
        self.remote_received_local = local_ms;
    }

    /// A tap at `local_ms`. Returns the reading once three taps are in.
    pub fn tap(&mut self, local_ms: f64) -> Option<TapReading> {
        let reading = self.tap.tap(local_ms);
        self.last_tap_local = local_ms;
        self.tap_clock = Some(match reading {
            Some(r) => BeatClockState { bpm: r.bpm, phase_at: r.phase_at, confidence: r.confidence, source: BeatSource::Tap },
            None => {
                let prev = self.tap_clock.unwrap_or(BeatClockState { source: BeatSource::Tap, ..OFF_BEAT });
                BeatClockState { phase_at: local_ms, source: BeatSource::Tap, ..prev }
            }
        });
        reading
    }

    pub fn tap_count(&self) -> usize {
        self.tap.count()
    }

    pub fn set_mic_listening(&mut self, on: bool) {
        self.mic_listening = on;
        if !on {
            self.mic_clock = None;
        }
    }

    pub fn mic_listening(&self) -> bool {
        self.mic_listening
    }

    /// Publish (or clear) the mic's locked clock, phase in local epoch ms.
    pub fn set_mic_clock(&mut self, clock: Option<BeatClockState>) {
        self.mic_clock = if self.mic_listening { clock } else { None };
    }

    fn remote_fresh(&self, local_ms: f64) -> bool {
        matches!(self.remote, Some(r) if r.source != BeatSource::Off)
            && local_ms - self.remote_received_local <= REMOTE_STALE_MS
    }

    /// The driving clock with its phase converted to local epoch ms.
    pub fn active(&self, local_ms: f64) -> (ActiveSource, BeatClockState) {
        if self.mic_listening {
            if let Some(m) = self.mic_clock {
                return (ActiveSource::Mic, m);
            }
        }
        let tap_wins = self.tap_clock.is_some() && self.last_tap_local >= self.remote_changed_local;
        if tap_wins {
            return (ActiveSource::Tap, self.tap_clock.unwrap());
        }
        if self.remote_fresh(local_ms) {
            let r = self.remote.unwrap();
            let local_phase = match self.offset.offset_ms() {
                Some(off) if r.phase_at > 0.0 => r.phase_at - off,
                _ => r.phase_at,
            };
            return (ActiveSource::Remote, BeatClockState { phase_at: local_phase, ..r });
        }
        if let Some(t) = self.tap_clock {
            return (ActiveSource::Tap, t);
        }
        (ActiveSource::None, OFF_BEAT)
    }

    /// Evaluate the driving clock at `local_ms`.
    pub fn sample(&self, local_ms: f64) -> (ActiveSource, BeatClockState, BeatSample) {
        let (src, st) = self.active(local_ms);
        (src, st, beat_at(&st, local_ms))
    }

    /// Pulse intensity 0..1 for the shaders and bursts. Exactly 0 under
    /// reduced motion (the comfort default, ADR-2107): nothing pulses, and the
    /// tempo is shown only by the HUD readout, which reads `sample` directly.
    pub fn pulse_intensity(&self, local_ms: f64, reduced_motion: bool) -> f64 {
        if reduced_motion {
            return 0.0;
        }
        let (_, _, s) = self.sample(local_ms);
        if !s.on {
            return 0.0;
        }
        s.pulse.clamp(0.0, 1.0)
    }
}

// ─── onset analysis (beat.ts) ────────────────────────────────────────────────

/// Analysis rate after decimation.
pub const ANALYSIS_RATE: f64 = 11025.0;
const HOP: usize = 128;
const WIN: usize = 256;

/// `OnsetEnvelope`: rectified, local-mean-subtracted log-energy flux per hop.
#[derive(Debug, Clone)]
pub struct OnsetEnvelope {
    pub env: Vec<f32>,
    pub frame_rate: f64,
    /// Seconds to add to `index / frame_rate` to land on the onset.
    pub lag: f64,
}

/// Port of `onsetEnvelope`. Intermediate arrays are f32, as the TS stores into
/// `Float32Array`; sums run in f64, as JS arithmetic does.
pub fn onset_envelope(samples: &[f32], sample_rate: f64) -> OnsetEnvelope {
    let nf = if samples.len() > WIN { (samples.len() - WIN) / HOP } else { 0 };
    let frame_rate = sample_rate / HOP as f64;
    let mut energy = vec![0f32; nf];
    for (i, e) in energy.iter_mut().enumerate() {
        let o = i * HOP;
        let mut a = 0f64;
        for j in 0..WIN {
            let s = samples[o + j] as f64;
            a += s * s;
        }
        *e = (200.0 * (a / WIN as f64).sqrt()).ln_1p() as f32;
    }
    let mut flux = vec![0f32; nf];
    let mut mean = 0f64;
    for i in 1..nf {
        flux[i] = (energy[i] as f64 - energy[i - 1] as f64).max(0.0) as f32;
        mean += flux[i] as f64;
    }
    mean /= nf.max(1) as f64;
    if mean == 0.0 || mean.is_nan() {
        mean = 1.0;
    }
    for f in flux.iter_mut() {
        *f = (*f as f64 / mean) as f32;
    }
    let w = js_round(frame_rate * 0.25).max(1.0) as i64;
    let nfi = nf as i64;
    let mut env = vec![0f32; nf];
    let mut acc = 0f64;
    for f in flux.iter().take(nf.min(w as usize)) {
        acc += *f as f64;
    }
    for i in 0..nfi {
        let lo = i - w - 1;
        let hi = i + w;
        if hi < nfi {
            acc += flux[hi as usize] as f64;
        }
        if lo >= 0 {
            acc -= flux[lo as usize] as f64;
        }
        let cnt = (nfi - 1).min(hi) - 0i64.max(lo + 1) + 1;
        env[i as usize] = (flux[i as usize] as f64 - acc / cnt as f64).max(0.0) as f32;
    }
    OnsetEnvelope { env, frame_rate, lag: (WIN as f64 - HOP as f64 / 2.0) / sample_rate }
}

/// `TempoEstimate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoEstimate {
    pub bpm: f64,
    /// Seconds from t = 0 to the first beat, in `[0, 60 / bpm)`.
    pub offset: f64,
    /// 0..1 normalised autocorrelation at the chosen tempo.
    pub confidence: f64,
}

fn at(env: &[f32], k: i64) -> f64 {
    if k >= 0 && (k as usize) < env.len() {
        env[k as usize] as f64
    } else {
        0.0
    }
}

fn sample_at(v: &[f32], x: f64) -> f64 {
    let k = x.floor();
    let f = x - k;
    let ki = k as i64;
    if ki + 1 < v.len() as i64 && ki >= 0 {
        v[ki as usize] as f64 * (1.0 - f) + v[ki as usize + 1] as f64 * f
    } else {
        0.0
    }
}

fn prior(bpm: f64) -> f64 {
    (-0.5 * ((bpm / 118.0).log2() / 0.9).powi(2)).exp()
}

/// Port of `estimateTempo` (autocorrelation with a log-normal prior near
/// 118 bpm, then a comb fit for tempo and phase). Loop increments are the same
/// accumulated f64 steps as the TS so the argmax lands on the same grid point.
pub fn estimate_tempo(env: &[f32], frame_rate: f64, min_bpm: f64, max_bpm: f64, lag_sec: f64) -> TempoEstimate {
    let nf = env.len();
    if nf < 8 {
        return TempoEstimate { bpm: 120.0, offset: 0.0, confidence: 0.0 };
    }
    let nff = nf as f64;
    let acf = |bpm: f64| -> f64 {
        let lag = (60.0 * frame_rate) / bpm;
        let n2 = nff - 2.0 * lag - 2.0;
        let mut sum = 0.0;
        let mut t = 0usize;
        while (t as f64) < n2 {
            let v = env[t] as f64;
            if v != 0.0 {
                sum += v * (sample_at(env, t as f64 + lag) + 0.5 * sample_at(env, t as f64 + 2.0 * lag));
            }
            t += 1;
        }
        sum / n2.max(1.0)
    };
    let score = |bpm: f64| acf(bpm) * (0.6 + 0.4 * prior(bpm));

    let mut best = 0.0;
    let mut best_b = 120.0;
    let mut bp = min_bpm;
    while bp <= max_bpm {
        let sc = score(bp);
        if sc > best {
            best = sc;
            best_b = bp;
        }
        bp += 0.5;
    }
    let (lo, hi) = (best_b - 0.5, best_b + 0.5);
    let mut bp = lo;
    while bp <= hi {
        let sc = score(bp);
        if sc > best {
            best = sc;
            best_b = bp;
        }
        bp += 0.05;
    }
    let (mut m1, mut m2) = (0.0, 0.0);
    for &v in env {
        m1 += v as f64;
        m2 += v as f64 * v as f64;
    }
    m1 /= nff;
    m2 /= nff;
    let base = 1.5 * m1 * m1;
    let top = 1.5 * m2;
    let confidence = if top > base { ((acf(best_b) - base) / (top - base)).clamp(0.0, 1.0) } else { 0.0 };

    let comb = |bpm: f64, ph: f64| -> f64 {
        let p = (60.0 * frame_rate) / bpm;
        let mut s = 0.0;
        let mut x = ph;
        while x < nff - 1.0 {
            let k = js_round(x) as i64;
            s += at(env, k).max(0.5 * at(env, k - 1)).max(0.5 * at(env, k + 1));
            x += p;
        }
        s
    };
    let mut best_ph = 0.0;
    let mut best_s = -1.0;
    let mut cb = best_b;
    let (lo, hi) = (best_b - 1.5, best_b + 1.5);
    let mut bb = lo;
    while bb <= hi {
        let pb = (60.0 * frame_rate) / bb;
        let mut ph = 0.0;
        while ph < pb {
            let s = comb(bb, ph);
            if s > best_s {
                best_s = s;
                best_ph = ph;
                cb = bb;
            }
            ph += 0.5;
        }
        bb += 0.02;
    }
    best_b = cb;
    let p = (60.0 * frame_rate) / best_b;
    best_s = -1.0;
    let hi = best_ph + 1.0;
    let mut ph = (best_ph - 1.0).max(0.0);
    while ph <= hi {
        let mut s = 0.0;
        let mut x = ph;
        while x < nff - 1.0 {
            s += sample_at(env, x);
            x += p;
        }
        if s > best_s {
            best_s = s;
            best_ph = ph;
        }
        ph += 0.05;
    }
    let period = 60.0 / best_b;
    let mut offset = best_ph / frame_rate + lag_sec;
    offset = ((offset % period) + period) % period;
    TempoEstimate {
        bpm: js_round(best_b * 100.0) / 100.0,
        offset: js_round(offset * 10000.0) / 10000.0,
        confidence: js_round(confidence * 100.0) / 100.0,
    }
}

/// `estimateTempo` with its defaults (70–180 bpm) and the envelope's own lag.
pub fn estimate_tempo_env(e: &OnsetEnvelope) -> TempoEstimate {
    estimate_tempo(&e.env, e.frame_rate, 70.0, 180.0, e.lag)
}

/// Box-filter decimation to roughly [`ANALYSIS_RATE`] (`decimate`).
pub fn decimate(samples: &[f32], sample_rate: f64) -> (Vec<f32>, f64) {
    let factor = js_round(sample_rate / ANALYSIS_RATE).max(1.0) as usize;
    if factor == 1 {
        return (samples.to_vec(), sample_rate);
    }
    let n = samples.len() / factor;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let o = i * factor;
        let s: f64 = samples[o..o + factor].iter().map(|&v| v as f64).sum();
        out.push((s / factor as f64) as f32);
    }
    (out, sample_rate / factor as f64)
}

// ─── microphone (WP8, opt-in) ────────────────────────────────────────────────

/// Confidence a mic estimate must reach before it may drive the pulse.
pub const MIC_LOCK_CONFIDENCE: f64 = 0.6;
/// Two consecutive confident estimates must agree this closely to lock.
pub const MIC_AGREE_BPM: f64 = 2.0;
/// Seconds of decimated audio kept for analysis.
pub const MIC_WINDOW_SEC: f64 = 8.0;
/// Analyse at most this often.
pub const MIC_ANALYSE_EVERY_MS: f64 = 2000.0;
/// A lock lapses after this long without a confirming estimate.
pub const MIC_LOCK_HOLD_MS: f64 = 8000.0;

/// What the mic analyser is doing, for the on-screen indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicState {
    Off,
    /// Capturing, not enough audio for a first estimate yet.
    Listening,
    /// Estimating; no confident, agreeing tempo.
    Searching,
    /// Locked to a confident tempo.
    Locked,
}

impl MicState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Listening => "listening",
            Self::Searching => "searching",
            Self::Locked => "locked",
        }
    }
}

/// Rolling mic analyser. Audio is held only as a ring of decimated samples in
/// memory; it is never written anywhere or sent.
#[derive(Debug, Clone)]
pub struct MicBeat {
    enabled: bool,
    capture_rate: f64,
    factor: usize,
    acc: f64,
    acc_n: usize,
    window: std::collections::VecDeque<f32>,
    /// Local epoch ms of the newest sample in `window`.
    window_end_ms: f64,
    last_analysis_ms: f64,
    last_estimate: Option<TempoEstimate>,
    lock: Option<BeatClockState>,
    lock_confirmed_ms: f64,
    /// Capture → analysis latency compensation added to phase (ms).
    pub latency_ms: f64,
}

impl Default for MicBeat {
    fn default() -> Self {
        Self::new()
    }
}

impl MicBeat {
    pub fn new() -> Self {
        Self {
            enabled: false,
            capture_rate: 0.0,
            factor: 1,
            acc: 0.0,
            acc_n: 0,
            window: std::collections::VecDeque::new(),
            window_end_ms: 0.0,
            last_analysis_ms: f64::NEG_INFINITY,
            last_estimate: None,
            lock: None,
            lock_confirmed_ms: f64::NEG_INFINITY,
            latency_ms: 0.0,
        }
    }

    /// Turn listening on or off. Off drops every buffered sample.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        self.window.clear();
        self.acc = 0.0;
        self.acc_n = 0;
        self.last_estimate = None;
        self.lock = None;
        self.last_analysis_ms = f64::NEG_INFINITY;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn rate(&self) -> f64 {
        self.capture_rate / self.factor as f64
    }

    fn capacity(&self) -> usize {
        (MIC_WINDOW_SEC * self.rate()).round().max(1.0) as usize
    }

    /// Append mono samples captured at `sample_rate`, the last of which was
    /// captured at local epoch `end_ms`. No-op while disabled.
    pub fn push(&mut self, samples: &[f32], sample_rate: f64, end_ms: f64) {
        if !self.enabled || samples.is_empty() || sample_rate.is_nan() || sample_rate <= 0.0 {
            return;
        }
        if sample_rate != self.capture_rate {
            self.capture_rate = sample_rate;
            self.factor = js_round(sample_rate / ANALYSIS_RATE).max(1.0) as usize;
            self.window.clear();
            self.acc = 0.0;
            self.acc_n = 0;
        }
        let cap = self.capacity();
        for &s in samples {
            self.acc += s as f64;
            self.acc_n += 1;
            if self.acc_n == self.factor {
                self.window.push_back((self.acc / self.factor as f64) as f32);
                self.acc = 0.0;
                self.acc_n = 0;
                if self.window.len() > cap {
                    self.window.pop_front();
                }
            }
        }
        self.window_end_ms = end_ms;
    }

    /// Whether an analysis is due at `now_ms`.
    pub fn due(&self, now_ms: f64) -> bool {
        self.enabled
            && self.window.len() as f64 >= 4.0 * self.rate().max(1.0)
            && now_ms - self.last_analysis_ms >= MIC_ANALYSE_EVERY_MS
    }

    /// Snapshot for analysis: (decimated samples, their rate, local epoch ms of
    /// the first sample). Marks the analysis as started.
    pub fn take_snapshot(&mut self, now_ms: f64) -> (Vec<f32>, f64, f64) {
        self.last_analysis_ms = now_ms;
        let data: Vec<f32> = self.window.iter().copied().collect();
        let rate = self.rate();
        let start = self.window_end_ms - (data.len() as f64 / rate) * 1000.0;
        (data, rate, start)
    }

    /// Fold in an analysis result computed over a snapshot that started at
    /// local epoch `start_ms`. Locks only on two consecutive confident,
    /// agreeing estimates; a lock lapses after [`MIC_LOCK_HOLD_MS`].
    pub fn apply_estimate(&mut self, est: TempoEstimate, start_ms: f64, now_ms: f64) {
        if !self.enabled {
            return;
        }
        let confident = est.confidence >= MIC_LOCK_CONFIDENCE;
        let agrees = matches!(self.last_estimate,
            Some(prev) if prev.confidence >= MIC_LOCK_CONFIDENCE && (prev.bpm - est.bpm).abs() <= MIC_AGREE_BPM);
        if confident && agrees {
            self.lock = Some(BeatClockState {
                bpm: est.bpm,
                phase_at: start_ms + est.offset * 1000.0 + self.latency_ms,
                confidence: est.confidence,
                source: BeatSource::Mic,
            });
            self.lock_confirmed_ms = now_ms;
        } else if now_ms - self.lock_confirmed_ms > MIC_LOCK_HOLD_MS {
            self.lock = None;
        }
        self.last_estimate = Some(est);
    }

    /// Analyse the current window synchronously (tests; the Godot class runs
    /// the same steps on a worker thread).
    pub fn analyse_now(&mut self, now_ms: f64) -> Option<TempoEstimate> {
        if !self.due(now_ms) {
            return None;
        }
        let (data, rate, start) = self.take_snapshot(now_ms);
        let est = analyse_window(&data, rate);
        self.apply_estimate(est, start, now_ms);
        Some(est)
    }

    pub fn lock(&self, now_ms: f64) -> Option<BeatClockState> {
        if now_ms - self.lock_confirmed_ms > MIC_LOCK_HOLD_MS {
            None
        } else {
            self.lock
        }
    }

    pub fn last_estimate(&self) -> Option<TempoEstimate> {
        self.last_estimate
    }

    pub fn state(&self, now_ms: f64) -> MicState {
        if !self.enabled {
            MicState::Off
        } else if self.lock(now_ms).is_some() {
            MicState::Locked
        } else if self.last_estimate.is_some() {
            MicState::Searching
        } else {
            MicState::Listening
        }
    }

    /// Samples currently held (decimated) — for tests and the privacy check
    /// that disabling drops them.
    pub fn buffered(&self) -> usize {
        self.window.len()
    }
}

/// Onset envelope + tempo over an already-decimated window.
pub fn analyse_window(data: &[f32], rate: f64) -> TempoEstimate {
    let e = onset_envelope(data, rate);
    estimate_tempo_env(&e)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_760_000_000_000.0;
    const SR: f64 = 11025.0;

    fn fixture() -> serde_json::Value {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/desktop_parity.json");
        serde_json::from_str(&std::fs::read_to_string(p).expect("fixture")).expect("json")
    }

    /// The JS LCG step `(s * 1103515245 + 12345) & 0x7fffffff`, including the
    /// double-precision rounding of the product and ToInt32 of the result.
    fn js_lcg(s: f64) -> f64 {
        let x = s * 1103515245.0 + 12345.0;
        let m = x.trunc() % 4294967296.0;
        let m = if m < 0.0 { m + 4294967296.0 } else { m };
        ((m as u64) & 0x7fff_ffff) as f64
    }

    fn click_track(bpm: f64, offset: f64, seconds: f64, sr: f64, seed: f64) -> Vec<f32> {
        let n = js_round(seconds * sr) as usize;
        let mut out = vec![0f32; n];
        let mut s = seed;
        for o in out.iter_mut() {
            s = js_lcg(s);
            *o = (0.01 * ((s / 2147483647.0) * 2.0 - 1.0)) as f32;
        }
        let period = 60.0 / bpm;
        let mut t = offset;
        while t < seconds {
            let start = js_round(t * sr) as usize;
            let mut k = 0usize;
            while (k as f64) < sr * 0.03 && start + k < n {
                let kf = k as f64;
                let add = (2.0 * std::f64::consts::PI * 1500.0 * kf / sr).sin() * (-kf / (sr * 0.006)).exp();
                out[start + k] = (out[start + k] as f64 + add) as f32;
                k += 1;
            }
            t += period;
        }
        out
    }

    fn noise(seconds: usize, seed: f64) -> Vec<f32> {
        let mut s = seed;
        (0..(SR as usize * seconds))
            .map(|_| {
                s = js_lcg(s);
                (s / 2147483647.0 - 0.5) as f32
            })
            .collect()
    }

    fn src(s: &str) -> BeatSource {
        BeatSource::from_wire(s).unwrap()
    }

    fn state_of(v: &serde_json::Value) -> BeatClockState {
        BeatClockState {
            bpm: v["bpm"].as_f64().unwrap(),
            phase_at: v["phaseAt"].as_f64().unwrap(),
            confidence: v["confidence"].as_f64().unwrap(),
            source: src(v["source"].as_str().unwrap()),
        }
    }

    #[test]
    fn beat_at_matches_every_desktop_fixture_sample() {
        let f = fixture();
        let rows = f["beatSamples"].as_array().unwrap();
        assert!(rows.len() >= 50);
        for row in rows {
            let st = state_of(&row["state"]);
            let now = row["now"].as_f64().unwrap();
            let got = beat_at(&st, now);
            let want = &row["sample"];
            assert_eq!(got.on, want["on"].as_bool().unwrap(), "{row}");
            assert_eq!(got.beat_index, want["beatIndex"].as_i64().unwrap(), "{row}");
            for (g, k) in [(got.phase, "phase"), (got.pulse, "pulse"), (got.bar, "bar")] {
                assert!((g - want[k].as_f64().unwrap()).abs() < 1e-9, "{k} {row}");
            }
        }
    }

    #[test]
    fn tap_tempo_matches_every_desktop_fixture_sequence() {
        let f = fixture();
        for seq in f["tapSequences"].as_array().unwrap() {
            let mut tt = TapTempo::new();
            let taps = seq["taps"].as_array().unwrap();
            let readings = seq["readings"].as_array().unwrap();
            for (t, want) in taps.iter().zip(readings) {
                let got = tt.tap(t.as_f64().unwrap());
                if want.is_null() {
                    assert!(got.is_none(), "{seq}");
                    continue;
                }
                let g = got.expect("reading");
                assert_eq!(g.bpm, want["bpm"].as_f64().unwrap(), "{seq}");
                assert_eq!(g.phase_at, want["phaseAt"].as_f64().unwrap());
                assert!((g.confidence - want["confidence"].as_f64().unwrap()).abs() < 1e-12);
                assert_eq!(g.taps as u64, want["taps"].as_u64().unwrap());
            }
        }
    }

    #[test]
    fn tap_median_rejects_a_fumbled_tap_and_resets_after_a_pause() {
        let mut tt = TapTempo::new();
        let mut r = None;
        for d in [0.0, 500.0, 1000.0, 1500.0, 1830.0, 2500.0, 3000.0, 3500.0] {
            r = tt.tap(T0 + d);
        }
        assert_eq!(r.unwrap().bpm, 120.0, "one outlier interval does not move the median");
        let after = T0 + 3500.0 + TAP_RESET_MS + 1.0;
        assert!(tt.tap(after).is_none());
        assert_eq!(tt.count(), 1);
    }

    #[test]
    fn onset_envelope_and_tempo_match_the_desktop_click_tracks() {
        let f = fixture();
        for row in f["tempo"].as_array().unwrap() {
            let (bpm, off, secs) = (row["bpm"].as_f64().unwrap(), row["offset"].as_f64().unwrap(), row["seconds"].as_f64().unwrap());
            let track = click_track(bpm, off, secs, SR, row["seed"].as_f64().unwrap());
            let e = onset_envelope(&track, SR);
            assert_eq!(e.env.len() as u64, row["envLen"].as_u64().unwrap());
            assert!((e.frame_rate - row["frameRate"].as_f64().unwrap()).abs() < 1e-12);
            assert!((e.lag - row["lag"].as_f64().unwrap()).abs() < 1e-12);
            let sum: f64 = e.env.iter().map(|&v| v as f64).sum();
            let want_sum = row["envSum"].as_f64().unwrap();
            assert!((sum - want_sum).abs() / want_sum < 1e-4, "envelope sum {sum} vs {want_sum}");
            let got = estimate_tempo_env(&e);
            let want = &row["result"];
            assert!((got.bpm - want["bpm"].as_f64().unwrap()).abs() <= 0.05, "bpm {got:?} vs {want}");
            assert!((got.offset - want["offset"].as_f64().unwrap()).abs() <= 0.002, "offset {got:?} vs {want}");
            assert!((got.confidence - want["confidence"].as_f64().unwrap()).abs() <= 0.02, "conf {got:?} vs {want}");
            // and the beat.test.ts acceptance bounds
            assert!((got.bpm - bpm).abs() < 1.0);
            let period = 60.0 / bpm;
            let d = ((got.offset - off) % period + period) % period;
            assert!(d.min(period - d) < 0.025);
            assert!(got.confidence > 0.6);
        }
    }

    #[test]
    fn noise_scores_low_like_the_desktop() {
        let f = fixture();
        let row = &f["noiseTempo"];
        let e = onset_envelope(&noise(10, 3.0), SR);
        assert_eq!(e.env.len() as u64, row["envLen"].as_u64().unwrap());
        let got = estimate_tempo_env(&e);
        assert!((got.confidence - row["result"]["confidence"].as_f64().unwrap()).abs() <= 0.02, "{got:?}");
        assert!(got.confidence < 0.4);
        assert!(got.confidence < MIC_LOCK_CONFIDENCE, "noise can never lock the mic");
    }

    #[test]
    fn decimation_of_44k_finds_the_tempo() {
        let mono = click_track(110.0, 0.2, 12.0, 44100.0, 7.0);
        let (data, rate) = decimate(&mono, 44100.0);
        assert_eq!(rate, 11025.0);
        let r = analyse_window(&data, rate);
        assert!((r.bpm - 110.0).abs() < 1.0, "{r:?}");
    }

    #[test]
    fn beat_clock_frame_validation() {
        let ok = r#"{"type":"beatClock","bpm":128,"phaseAt":1760000000000,"confidence":0.5,"source":"tap","serverTime":1760000000500}"#;
        let f = parse_beat_clock_frame(ok).unwrap();
        assert_eq!(f.state.bpm, 128.0);
        assert_eq!(f.server_time, 1_760_000_000_500.0);
        for bad in [
            r#"{"type":"beatClock","bpm":30,"phaseAt":1,"confidence":0.5,"source":"tap"}"#,
            r#"{"type":"beatClock","bpm":221,"phaseAt":1,"confidence":0.5,"source":"tap"}"#,
            r#"{"type":"beatClock","bpm":120,"phaseAt":-1,"confidence":0.5,"source":"tap"}"#,
            r#"{"type":"beatClock","bpm":120,"phaseAt":1,"confidence":1.5,"source":"tap"}"#,
            r#"{"type":"beatClock","bpm":120,"phaseAt":1,"confidence":0.5,"source":"mic"}"#,
            r#"{"type":"beatClock","bpm":"120","phaseAt":1,"confidence":0.5,"source":"tap"}"#,
            r#"{"type":"beatClock","phaseAt":1,"confidence":0.5,"source":"tap"}"#,
            r#"{"type":"other","bpm":120,"phaseAt":1,"confidence":0.5,"source":"tap"}"#,
            "nope",
        ] {
            assert!(parse_beat_clock_frame(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn ping_pong_round_trip() {
        assert_eq!(build_ping(1234.9), r#"{"type":"ping","timestamp":1234}"#);
        assert_eq!(parse_pong(r#"{"type":"pong","timestamp":1234,"serverTime":5000}"#), Some((1234.0, 5000.0)));
        assert_eq!(parse_pong(r#"{"type":"pong","timestamp":1234}"#), None, "legacy pong has no server time");
    }

    #[test]
    fn offset_estimate_survives_asymmetric_round_trips() {
        // True offset: server = local + 10_000. Each trip has a 5 ms uplink; the
        // downlink varies 5..200 ms (congestion on the way back). Only the
        // symmetric trip gives the true offset; the min-RTT filter picks it.
        let true_off = 10_000.0;
        let mut c = ClockOffset::new();
        let mut t = 0.0;
        for down in [120.0, 60.0, 200.0, 5.0, 90.0, 150.0] {
            let sent = t;
            let server = sent + 5.0 + true_off;
            let recv = sent + 5.0 + down;
            assert!(c.add_sample(sent, server, recv));
            t += 2000.0;
        }
        assert!((c.offset_ms().unwrap() - true_off).abs() < 1e-9);
        assert_eq!(c.rtt_ms(), Some(10.0));
        // Worst-case error of any single sample is rtt/2; the naive mean would
        // be off by tens of ms.
        assert!(!c.add_sample(0.0, 1.0, -1.0), "negative RTT rejected");
        assert!(!c.add_sample(0.0, 1.0, 5000.0), "huge RTT rejected");
    }

    #[test]
    fn remote_phase_is_continuous_across_heartbeats_and_converted_to_local_time() {
        let mut bs = BeatSync::new();
        let off = 3_000.0; // server = local + 3000
        bs.offset.add_sample(T0, T0 + 5.0 + off, T0 + 10.0);
        let st = BeatClockState { bpm: 120.0, phase_at: T0 + off + 250.0, confidence: 0.9, source: BeatSource::Tap };
        bs.on_remote(RemoteBeatFrame { state: st, server_time: 0.0 }, T0);
        // At local T0+250 the server clock reads T0+3250 = phase_at → on the beat.
        let (src, _, s) = bs.sample(T0 + 250.0);
        assert_eq!(src, ActiveSource::Remote);
        assert!((s.pulse - 1.0).abs() < 1e-6, "{s:?}");
        let before = bs.sample(T0 + 1900.0).2;
        // A heartbeat with the same clock 2 s later must not jump the phase.
        bs.on_remote(RemoteBeatFrame { state: st, server_time: 0.0 }, T0 + 2000.0);
        let after = bs.sample(T0 + 1900.0).2;
        assert_eq!(before, after);
        assert!((bs.sample(T0 + 2250.0).2.pulse - 1.0).abs() < 1e-6, "still on the 500 ms grid");
        // Silence beyond the stale window drops the relayed clock.
        assert_eq!(bs.sample(T0 + 2000.0 + REMOTE_STALE_MS + 1.0).0, ActiveSource::None);
    }

    #[test]
    fn arbitration_tap_then_desktop_change_then_mic() {
        let mut bs = BeatSync::new();
        let remote = BeatClockState { bpm: 100.0, phase_at: T0, confidence: 0.8, source: BeatSource::File };
        bs.on_remote(RemoteBeatFrame { state: remote, server_time: 0.0 }, T0);
        assert_eq!(bs.active(T0 + 10.0).0, ActiveSource::Remote);
        for i in 0..4 {
            bs.tap(T0 + 100.0 + i as f64 * 500.0);
        }
        assert_eq!(bs.active(T0 + 2000.0).0, ActiveSource::Tap, "a tap takes over");
        assert_eq!(bs.active(T0 + 2000.0).1.bpm, 120.0);
        bs.on_remote(RemoteBeatFrame { state: remote, server_time: 0.0 }, T0 + 2100.0);
        assert_eq!(bs.active(T0 + 2200.0).0, ActiveSource::Tap, "an unchanged heartbeat does not take it back");
        let changed = BeatClockState { bpm: 101.0, ..remote };
        bs.on_remote(RemoteBeatFrame { state: changed, server_time: 0.0 }, T0 + 2300.0);
        assert_eq!(bs.active(T0 + 2400.0).0, ActiveSource::Remote, "a changed desktop clock wins it back");
        bs.set_mic_listening(true);
        bs.set_mic_clock(Some(BeatClockState { bpm: 90.0, phase_at: T0, confidence: 0.7, source: BeatSource::Mic }));
        assert_eq!(bs.active(T0 + 2500.0).0, ActiveSource::Mic);
        bs.set_mic_listening(false);
        assert_eq!(bs.active(T0 + 2500.0).0, ActiveSource::Remote, "mic off drops its clock");
        let off = BeatClockState { source: BeatSource::Off, ..changed };
        bs.on_remote(RemoteBeatFrame { state: off, server_time: 0.0 }, T0 + 2600.0);
        assert_eq!(bs.active(T0 + 2700.0).0, ActiveSource::Tap, "desktop off falls back to the tap clock");
    }

    #[test]
    fn reduced_motion_stops_the_pulse_entirely() {
        // ADR-2107: reduced motion stops pulsing. Halos, edges and bursts hold
        // steady brightness; only the HUD readout shows the tempo.
        let mut bs = BeatSync::new();
        for i in 0..4 {
            bs.tap(T0 + i as f64 * 500.0);
        }
        let full = bs.pulse_intensity(T0 + 1500.0, false);
        assert!((full - 1.0).abs() < 1e-6, "on the beat with motion allowed");
        for k in 0..200 {
            let t = T0 + 1500.0 + k as f64 * 5.0;
            assert_eq!(bs.pulse_intensity(t, true), 0.0, "exactly 0 under reduced motion at {t}");
        }
        assert!(bs.sample(T0 + 1500.0).2.on, "the clock itself keeps running for the HUD");
        assert_eq!(BeatSync::new().pulse_intensity(T0, false), 0.0, "no clock, no pulse");
    }

    #[test]
    fn mic_locks_on_a_click_track_and_never_on_noise() {
        let mut mic = MicBeat::new();
        assert_eq!(mic.state(T0), MicState::Off);
        mic.push(&[0.5; 1000], 44100.0, T0);
        assert_eq!(mic.buffered(), 0, "nothing is held while disabled");
        mic.set_enabled(true);
        assert_eq!(mic.state(T0), MicState::Listening);
        let audio = click_track(120.0, 0.25, 20.0, 44100.0, 7.0);
        // Feed 20 s in 100 ms chunks, analysing as the window fills.
        let chunk = 4410;
        let mut now = T0;
        let mut last = None;
        for c in audio.chunks(chunk) {
            now += 100.0;
            mic.push(c, 44100.0, now);
            if let Some(e) = mic.analyse_now(now) {
                last = Some(e);
            }
        }
        assert!(last.is_some());
        let lock = mic.lock(now).expect("click track locks");
        assert_eq!(mic.state(now), MicState::Locked);
        assert!((lock.bpm - 120.0).abs() < 1.0, "{lock:?}");
        // Lock phase is in local epoch: beats at T0 + 250 + k·500 (audio t=0 at T0).
        let s = beat_at(&lock, T0 + 250.0 + 500.0 * 30.0);
        let err_ms = s.phase.min(1.0 - s.phase) * 500.0;
        assert!(err_ms < 30.0, "phase error {err_ms} ms");
        mic.set_enabled(false);
        assert_eq!(mic.buffered(), 0, "disabling drops the audio");
        assert!(mic.lock(now).is_none());

        let mut quiet = MicBeat::new();
        quiet.set_enabled(true);
        let nz = noise(20, 3.0);
        let mut now = T0;
        for c in nz.chunks(1102) {
            now += 100.0;
            quiet.push(c, 11025.0, now);
            quiet.analyse_now(now);
        }
        assert!(quiet.lock(now).is_none(), "noise never locks");
        assert_eq!(quiet.state(now), MicState::Searching);
    }
}
