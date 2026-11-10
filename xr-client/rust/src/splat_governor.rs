//! The splat tier governor (ADR-2170 decision 7).
//!
//! The same stepping rule as the visual-extras governor in `eye_candy.gd`
//! (ADR-2167), parameterised for the splat tiers 0 to 3 instead of re-derived:
//! pure arithmetic over a stream of load samples, tested with synthetic series.
//! A parity test in `tests/unit/test_splat_governor.gd` feeds one series to this
//! governor and to the GDScript `Governor`, so the two cannot drift apart.
//!
//! Samples are GPU milliseconds ([`Signal::Gpu`]) or frame milliseconds
//! ([`Signal::Frame`]). Once a window of [`WINDOW_SEC`] has filled, its p95
//! decides:
//!
//! * above `down_ms`: step down at once, never below [`FLOOR_TIER`], and double
//!   the climb hold-off (up to [`BACKOFF_MAX`]), so a tier that does not fit is
//!   not retried quickly;
//! * below `up_ms` for `up_hold_sec x backoff` of consecutive windows: step up
//!   one tier, but only while a splat volume is **entered** (ADR-2166). Outside
//!   an entered volume the governor never climbs and its calm count stays at 0;
//! * in between: hold, and restart the climb count.
//!
//! The GPU thresholds are PRD-008's: drop above 8 ms, climb below 5.5 ms, after
//! 8 s. When the GPU timer reads zero (ADR-2167 notes it is unverified on the XR
//! viewport) the caller switches to [`SplatGovernor::frame`]: frame-time p95
//! against 1.15x and 1.03x of the frame budget, a 20 s hold, and a ceiling of
//! tier 2. `XR_VISUAL_QUALITY=low` is [`SplatGovernor::set_low_cost`]: tier 0 in
//! the same call, whatever the window says.
//!
//! **The floor is tier 1, not 0** (a stated interpretation of "tiers 0 to 3"). A
//! governor that could step to 0 would remove the scene, and with no splat on
//! screen it would have no load to measure and no way to climb back: the volume
//! would vanish until restart. Like the visual extras' free sky, the smallest
//! scene is the floor; only low cost (tier 0, and out of the governor's hands)
//! takes the splats away, and the caller feeds samples only while splats draw.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

/// Highest tier the GPU signal may reach; mirrors
/// [`crate::frame_budget::SPLAT_MAX_TIER`].
pub const MAX_TIER: usize = crate::frame_budget::SPLAT_MAX_TIER;
/// The ceiling when only frame time vouches for the load.
pub const FRAME_FALLBACK_MAX_TIER: usize = 2;
/// Seconds a window of samples spans before its p95 is judged.
pub const WINDOW_SEC: f32 = 1.0;
/// Largest climb hold-off multiplier.
pub const BACKOFF_MAX: f32 = 8.0;
/// Seconds without a step down before the hold-off halves.
pub const BACKOFF_DECAY_SEC: f32 = 60.0;
/// GPU p95 above this steps down (PRD-008 section 6).
pub const GPU_DOWN_MS: f32 = 8.0;
/// GPU p95 below this, for the hold, steps up.
pub const GPU_UP_MS: f32 = 5.5;
/// Seconds of calm before a GPU-mode climb.
pub const GPU_UP_HOLD_SEC: f32 = 8.0;
/// Seconds of calm before a frame-mode climb.
pub const FRAME_UP_HOLD_SEC: f32 = 20.0;
/// The tier a splat volume starts at: the smallest scene that still reads.
pub const START_TIER: usize = 1;
/// The lowest tier the governor steps to (see the module docs).
pub const FLOOR_TIER: usize = 1;

/// Which stream the samples come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Measured GPU milliseconds a frame.
    Gpu,
    /// Frame-time milliseconds (the GPU timer reads zero).
    Frame,
}

/// The tier governor. See the module docs.
#[derive(Debug, Clone)]
pub struct SplatGovernor {
    signal: Signal,
    tier: usize,
    min_tier: usize,
    max_tier: usize,
    down_ms: f32,
    up_ms: f32,
    up_hold_sec: f32,
    backoff: f32,
    low_cost: bool,
    samples: Vec<f32>,
    elapsed: f32,
    calm: f32,
    since_down: f32,
    ups: u32,
    downs: u32,
}

/// The p95 of `samples`: the element at `min(n - 1, floor(0.95 n))` of the
/// sorted series, as `Governor.percentile` in `eye_candy.gd`.
pub fn p95(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    s[(s.len() - 1).min((0.95 * s.len() as f32).floor() as usize)]
}

impl SplatGovernor {
    fn new(signal: Signal, max_tier: usize, down_ms: f32, up_ms: f32, up_hold_sec: f32) -> Self {
        Self {
            signal,
            tier: START_TIER.min(max_tier),
            min_tier: FLOOR_TIER,
            max_tier,
            down_ms,
            up_ms,
            up_hold_sec,
            backoff: 1.0,
            low_cost: false,
            samples: Vec::new(),
            elapsed: 0.0,
            calm: 0.0,
            since_down: 0.0,
            ups: 0,
            downs: 0,
        }
    }

    /// GPU-time governor: drop above 8 ms p95, climb below 5.5 ms after 8 s.
    pub fn gpu() -> Self {
        Self::new(
            Signal::Gpu,
            MAX_TIER,
            GPU_DOWN_MS,
            GPU_UP_MS,
            GPU_UP_HOLD_SEC,
        )
    }

    /// Frame-time fallback: only a missed frame is evidence, so it is
    /// conservative on purpose (tier 2 at most, a 20 s hold).
    pub fn frame(budget_ms: f32) -> Self {
        Self::new(
            Signal::Frame,
            FRAME_FALLBACK_MAX_TIER,
            budget_ms * 1.15,
            budget_ms * 1.03,
            FRAME_UP_HOLD_SEC,
        )
    }

    /// Switch to the frame-time fallback, keeping the tier the GPU governor had
    /// reached (clamped to the fallback ceiling) and its hold-off.
    pub fn switch_to_frame(&mut self, budget_ms: f32) {
        let mut next = Self::frame(budget_ms);
        next.tier = self.tier.clamp(next.min_tier, next.max_tier);
        next.backoff = self.backoff;
        next.low_cost = self.low_cost;
        next.ups = self.ups;
        next.downs = self.downs;
        *self = next;
    }

    /// The tier to draw: 0 under low cost, else the governor's.
    pub fn tier(&self) -> usize {
        if self.low_cost {
            0
        } else {
            self.tier
        }
    }

    /// The signal the samples are judged as.
    pub fn signal(&self) -> Signal {
        self.signal
    }

    /// The highest tier this governor will climb to.
    pub fn max_tier(&self) -> usize {
        self.max_tier
    }

    /// The climb hold-off multiplier (1.0 to [`BACKOFF_MAX`]).
    pub fn backoff(&self) -> f32 {
        self.backoff
    }

    /// Steps taken so far: `(up, down)`.
    pub fn steps(&self) -> (u32, u32) {
        (self.ups, self.downs)
    }

    /// `XR_VISUAL_QUALITY=low` on or off. On, [`tier`](Self::tier) is 0 at once
    /// and the window is discarded; off, the governor resumes at the tier it
    /// held. Returns true when the drawn tier changed.
    pub fn set_low_cost(&mut self, low: bool) -> bool {
        let before = self.tier();
        self.low_cost = low;
        if low {
            self.samples.clear();
            self.elapsed = 0.0;
            self.calm = 0.0;
        }
        self.tier() != before
    }

    /// Feed one sample taken `delta` seconds after the last. `entered` is true
    /// while a splat volume is the entered room: only then may the tier climb.
    /// Returns true when the drawn tier changed. Samples are ignored under low
    /// cost.
    pub fn feed(&mut self, ms: f32, delta: f32, entered: bool) -> bool {
        if self.low_cost || !ms.is_finite() {
            return false;
        }
        self.samples.push(ms);
        self.elapsed += delta.max(0.0);
        // 90 samples of 1/90 s sum a hair under 1.0
        if self.elapsed < WINDOW_SEC - 1.0e-4 {
            return false;
        }
        let p = p95(&self.samples);
        let win = self.elapsed;
        self.samples.clear();
        self.elapsed = 0.0;
        self.since_down += win;
        if p > self.down_ms {
            self.calm = 0.0;
            self.since_down = 0.0;
            self.backoff = (self.backoff * 2.0).min(BACKOFF_MAX);
            if self.tier > self.min_tier {
                self.tier -= 1;
                self.downs += 1;
                return true;
            }
            return false;
        }
        if self.since_down >= BACKOFF_DECAY_SEC && self.backoff > 1.0 {
            self.backoff = (self.backoff * 0.5).max(1.0);
            self.since_down = 0.0;
        }
        if p < self.up_ms && entered {
            self.calm += win;
            if self.calm >= self.up_hold_sec * self.backoff && self.tier < self.max_tier {
                self.calm = 0.0;
                self.tier += 1;
                self.ups += 1;
                return true;
            }
        } else {
            self.calm = 0.0;
        }
        false
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
use godot::prelude::*;

/// Godot face of [`SplatGovernor`]: `SplatGovernor.new()`, then `use_gpu()` or
/// `use_frame(budget_ms)`, `feed(ms, delta, entered)` each frame and `tier()`.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(base = RefCounted, rename = SplatGovernor)]
pub struct SplatGovernorNode {
    base: Base<RefCounted>,
    inner: SplatGovernor,
}

#[cfg(not(test))]
#[godot_api]
impl IRefCounted for SplatGovernorNode {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            base,
            inner: SplatGovernor::gpu(),
        }
    }
}

#[cfg(not(test))]
#[godot_api]
impl SplatGovernorNode {
    /// Restart as the GPU-time governor (tier 1, hold-off 1).
    #[func]
    fn use_gpu(&mut self) {
        self.inner = SplatGovernor::gpu();
    }

    /// Restart as the frame-time governor for a `budget_ms` frame (tier 1).
    #[func]
    fn use_frame(&mut self, budget_ms: f64) {
        self.inner = SplatGovernor::frame(budget_ms as f32);
    }

    /// Fall back from GPU time to frame time, keeping the tier (capped at 2).
    #[func]
    fn switch_to_frame(&mut self, budget_ms: f64) {
        self.inner.switch_to_frame(budget_ms as f32);
    }

    /// One sample (`ms`) taken `delta` seconds after the last; `entered` is
    /// whether a splat volume is the entered room. True when the tier changed.
    #[func]
    fn feed(&mut self, ms: f64, delta: f64, entered: bool) -> bool {
        self.inner.feed(ms as f32, delta as f32, entered)
    }

    /// `XR_VISUAL_QUALITY=low` on or off; tier 0 in the same call when on.
    /// True when the drawn tier changed.
    #[func]
    fn set_low_cost(&mut self, low: bool) -> bool {
        self.inner.set_low_cost(low)
    }

    /// The tier to draw now (0 under low cost).
    #[func]
    fn tier(&self) -> i64 {
        self.inner.tier() as i64
    }

    /// "gpu" or "frame".
    #[func]
    fn signal(&self) -> GString {
        match self.inner.signal() {
            Signal::Gpu => "gpu".into(),
            Signal::Frame => "frame".into(),
        }
    }

    /// The highest tier this governor climbs to.
    #[func]
    fn max_tier(&self) -> i64 {
        self.inner.max_tier() as i64
    }

    /// The climb hold-off multiplier.
    #[func]
    fn backoff(&self) -> f64 {
        self.inner.backoff() as f64
    }

    /// Steps taken: {"up": n, "down": n}.
    #[func]
    fn steps(&self) -> Dictionary {
        let (up, down) = self.inner.steps();
        let mut d = Dictionary::new();
        d.set("up", up as i64);
        d.set("down", down as i64);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 90.0;

    /// Feed `secs` of a constant `ms` at 90 Hz; returns the tier changes seen.
    fn run(g: &mut SplatGovernor, ms: f32, secs: f32, entered: bool) -> usize {
        let mut changes = 0;
        for _ in 0..(secs * 90.0).round() as usize {
            if g.feed(ms, DT, entered) {
                changes += 1;
            }
        }
        changes
    }

    #[test]
    fn tiers_run_from_the_floor_to_the_largest_variant_and_start_at_the_smallest_scene() {
        assert_eq!(MAX_TIER, 3);
        let g = SplatGovernor::gpu();
        assert_eq!((g.tier(), g.max_tier(), g.signal()), (1, 3, Signal::Gpu));
        assert_eq!(g.steps(), (0, 0));
    }

    #[test]
    fn it_drops_at_once_above_eight_ms_and_never_below_the_floor() {
        let mut g = SplatGovernor::gpu();
        g.tier = 3;
        // The window is one second: the first full window above 8 ms drops one tier.
        assert_eq!(run(&mut g, 8.5, 1.0, true), 1);
        assert_eq!(g.tier(), 2);
        assert_eq!(run(&mut g, 9.0, 1.0, true), 1);
        assert_eq!(g.tier(), FLOOR_TIER);
        assert_eq!(
            run(&mut g, 12.0, 3.0, true),
            0,
            "and no lower: the scene is never removed by load"
        );
        assert_eq!(g.tier(), 1);
        assert_eq!(g.steps(), (0, 2));
        // Exactly 8 ms is not above the threshold.
        let mut g = SplatGovernor::gpu();
        assert_eq!(run(&mut g, 8.0, 3.0, true), 0);
        assert_eq!(g.tier(), 1);
    }

    #[test]
    fn it_climbs_below_five_and_a_half_ms_after_eight_seconds() {
        let mut g = SplatGovernor::gpu();
        // 7 s of calm: no step yet; the 8th window makes it.
        assert_eq!(run(&mut g, 5.0, 7.0, true), 0);
        assert_eq!(g.tier(), 1);
        assert_eq!(run(&mut g, 5.0, 1.0, true), 1);
        assert_eq!(g.tier(), 2);
        // The hold restarts after a step: 8 s again for the next one.
        assert_eq!(run(&mut g, 5.0, 7.0, true), 0);
        assert_eq!(run(&mut g, 5.0, 1.0, true), 1);
        assert_eq!(g.tier(), 3);
        assert_eq!(run(&mut g, 5.0, 30.0, true), 0, "tier 3 is the ceiling");
        assert_eq!(g.steps(), (2, 0));
    }

    #[test]
    fn between_the_thresholds_it_holds_and_restarts_the_climb_count() {
        let mut g = SplatGovernor::gpu();
        run(&mut g, 5.0, 6.0, true);
        run(&mut g, 7.0, 1.0, true); // between 5.5 and 8: hold, calm back to 0
        assert_eq!(run(&mut g, 5.0, 7.0, true), 0, "the count restarted");
        assert_eq!(run(&mut g, 5.0, 1.0, true), 1);
    }

    #[test]
    fn a_drop_doubles_the_hold_off_up_to_eight_and_it_decays() {
        let mut g = SplatGovernor::gpu();
        g.tier = 3;
        run(&mut g, 9.0, 1.0, true);
        assert_eq!((g.tier(), g.backoff()), (2, 2.0));
        // The climb now needs 16 s of calm.
        assert_eq!(run(&mut g, 5.0, 15.0, true), 0);
        assert_eq!(run(&mut g, 5.0, 1.0, true), 1);
        assert_eq!(g.tier(), 3);
        for _ in 0..5 {
            run(&mut g, 9.0, 1.0, true);
        }
        assert_eq!(g.backoff(), BACKOFF_MAX, "capped");
        // 60 s with no drop halves it (the decay runs on windows that did not drop).
        run(&mut g, 7.0, 61.0, true);
        assert_eq!(g.backoff(), 4.0);
    }

    #[test]
    fn it_never_climbs_unless_a_splat_volume_is_entered() {
        let mut g = SplatGovernor::gpu();
        assert_eq!(
            run(&mut g, 2.0, 60.0, false),
            0,
            "a minute of calm, not entered"
        );
        assert_eq!(g.tier(), 1);
        // Calm does not bank while not entered: the hold starts on entering.
        assert_eq!(run(&mut g, 2.0, 7.0, true), 0);
        assert_eq!(run(&mut g, 2.0, 1.0, true), 1);
        // It still drops when not entered.
        assert_eq!(run(&mut g, 9.0, 1.0, false), 1);
        assert_eq!(g.tier(), 1);
    }

    #[test]
    fn low_cost_is_tier_zero_in_the_same_call_and_resumes() {
        let mut g = SplatGovernor::gpu();
        g.tier = 3;
        assert!(g.set_low_cost(true), "the drawn tier changed at once");
        assert_eq!(g.tier(), 0);
        assert!(!g.feed(1.0, 2.0, true), "samples are ignored while low");
        assert_eq!(run(&mut g, 2.0, 60.0, true), 0);
        assert_eq!(g.tier(), 0);
        assert!(!g.set_low_cost(true), "idempotent");
        assert!(g.set_low_cost(false));
        assert_eq!(g.tier(), 3, "it resumes where it was");
    }

    #[test]
    fn the_frame_time_fallback_is_capped_at_tier_two_with_a_twenty_second_hold() {
        let budget = 1000.0 / 90.0;
        let mut g = SplatGovernor::frame(budget);
        assert_eq!((g.signal(), g.max_tier(), g.tier()), (Signal::Frame, 2, 1));
        // down above 1.15 x budget, climb below 1.03 x budget
        assert_eq!(run(&mut g, budget * 1.1, 5.0, true), 0, "between: hold");
        assert_eq!(run(&mut g, budget, 19.0, true), 0, "19 s is not enough");
        assert_eq!(run(&mut g, budget, 1.0, true), 1);
        assert_eq!(g.tier(), 2);
        assert_eq!(
            run(&mut g, budget, 120.0, true),
            0,
            "tier 2 is the ceiling in frame mode"
        );
        assert_eq!(
            run(&mut g, budget * 1.2, 1.0, true),
            1,
            "a missed frame drops it"
        );
        assert_eq!(g.tier(), 1);
    }

    #[test]
    fn switching_from_gpu_to_frame_keeps_the_tier_but_clamps_it() {
        let mut g = SplatGovernor::gpu();
        g.tier = 3;
        g.backoff = 4.0;
        g.switch_to_frame(1000.0 / 90.0);
        assert_eq!((g.signal(), g.tier(), g.max_tier()), (Signal::Frame, 2, 2));
        assert_eq!(g.backoff(), 4.0, "the hold-off carries over");
        let mut g = SplatGovernor::gpu();
        g.switch_to_frame(1000.0 / 90.0);
        assert_eq!(g.tier(), 1, "a lower tier is kept as it is");
    }

    #[test]
    fn the_percentile_matches_the_gdscript_governor() {
        assert_eq!(p95(&[]), 0.0);
        assert_eq!(p95(&[3.0]), 3.0);
        // 90 samples: index floor(0.95 * 90) = 85 of the sorted series
        let mut s: Vec<f32> = (0..90).map(|i| i as f32).collect();
        s.reverse();
        assert_eq!(p95(&s), 85.0);
        // one spike in 90 is not the p95
        let mut s = vec![5.0f32; 90];
        s[10] = 40.0;
        assert_eq!(p95(&s), 5.0);
        // ten spikes are
        for v in s.iter_mut().take(10) {
            *v = 40.0;
        }
        assert_eq!(p95(&s), 40.0);
    }

    #[test]
    fn non_finite_samples_are_ignored() {
        let mut g = SplatGovernor::gpu();
        assert!(!g.feed(f32::NAN, 5.0, true));
        assert!(!g.feed(f32::INFINITY, 5.0, true));
        assert_eq!(g.tier(), 1);
    }
}
