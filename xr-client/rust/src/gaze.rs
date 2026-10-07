//! User gaze-ray abstraction with one-euro smoothing (M4, ADR-130 Decision 4).
//!
//! Unifies two gaze sources behind a single [`GazeRay`]:
//! - **head-gaze** — the camera-forward vector. The *primary* path: calibration
//!   free, and the floor device (Quest 3) has no eye-tracking hardware.
//! - **eye-gaze** — the `XR_EXT_eye_gaze_interaction` pose. A *progressive
//!   enhancement*, used only when the GDScript side reports runtime support
//!   (`OpenXRInterface.is_eye_gaze_interaction_supported()`, queried **after**
//!   OpenXR init) via [`GazeResolver::set_eye_gaze_supported`].
//!
//! Eye-gaze pose is noisy (microsaccades) and even head-gaze carries tracker
//! jitter, so both are smoothed by the one-euro filter (Casiez, Roussel &
//! Vogel, CHI 2012) before the ray reaches the selection raycaster. One-euro
//! adapts its cutoff to signal speed: near-still gaze is smoothed hard (low
//! jitter), fast saccades pass through with low lag.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use tracing::trace;

#[cfg(not(test))]
use godot::prelude::*;

/// One-euro minimum cutoff frequency (Hz). Lower = more smoothing when still.
pub const DEFAULT_MIN_CUTOFF: f32 = 1.0;
/// One-euro speed coefficient. Higher = less lag on fast movement.
pub const DEFAULT_BETA: f32 = 0.007;
/// One-euro derivative cutoff frequency (Hz).
pub const DEFAULT_D_CUTOFF: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GazeSource {
    Head,
    Eye,
}

/// A smoothed gaze ray in world space. `dir` is unit length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GazeRay {
    pub origin: [f32; 3],
    pub dir: [f32; 3],
    pub source: GazeSource,
}

/// Scalar one-euro filter. See module docs for the reference.
#[derive(Debug, Clone, Copy)]
pub struct OneEuroFilter {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
    x_prev: f32,
    dx_prev: f32,
    initialised: bool,
}

impl OneEuroFilter {
    pub fn new(min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            x_prev: 0.0,
            dx_prev: 0.0,
            initialised: false,
        }
    }

    pub fn reset(&mut self) {
        self.initialised = false;
        self.x_prev = 0.0;
        self.dx_prev = 0.0;
    }

    /// Filter one sample. `dt_s` is the seconds since the previous sample; a
    /// non-positive `dt_s` holds the last output (avoids a divide-by-zero when
    /// two samples share a timestamp).
    pub fn filter(&mut self, x: f32, dt_s: f32) -> f32 {
        if !self.initialised {
            self.x_prev = x;
            self.dx_prev = 0.0;
            self.initialised = true;
            return x;
        }
        if dt_s <= 0.0 || !dt_s.is_finite() {
            return self.x_prev;
        }
        let dx = (x - self.x_prev) / dt_s;
        let edx = low_pass(dx, self.dx_prev, alpha(self.d_cutoff, dt_s));
        let cutoff = self.min_cutoff + self.beta * edx.abs();
        let x_hat = low_pass(x, self.x_prev, alpha(cutoff, dt_s));
        self.x_prev = x_hat;
        self.dx_prev = edx;
        x_hat
    }

    pub fn value(&self) -> f32 {
        self.x_prev
    }
}

fn alpha(cutoff: f32, dt_s: f32) -> f32 {
    let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
    1.0 / (1.0 + tau / dt_s)
}

fn low_pass(x: f32, x_prev: f32, a: f32) -> f32 {
    a * x + (1.0 - a) * x_prev
}

/// Resolves a smoothed [`GazeRay`] from raw head or eye poses. Holds one
/// one-euro filter per component of origin and direction, plus the runtime
/// eye-gaze support flag.
pub struct GazeResolver {
    origin: [OneEuroFilter; 3],
    dir: [OneEuroFilter; 3],
    eye_supported: bool,
}

impl GazeResolver {
    pub fn new() -> Self {
        Self::with_params(DEFAULT_MIN_CUTOFF, DEFAULT_BETA, DEFAULT_D_CUTOFF)
    }

    pub fn with_params(min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        let mk = || OneEuroFilter::new(min_cutoff, beta, d_cutoff);
        Self {
            origin: [mk(), mk(), mk()],
            dir: [mk(), mk(), mk()],
            eye_supported: false,
        }
    }

    /// Set from `OpenXRInterface.is_eye_gaze_interaction_supported()` — call only
    /// *after* OpenXR init (the query is invalid before it). Quest 3 reports
    /// false, keeping head-gaze primary.
    pub fn set_eye_gaze_supported(&mut self, supported: bool) {
        self.eye_supported = supported;
    }

    pub fn eye_gaze_supported(&self) -> bool {
        self.eye_supported
    }

    /// The source actually used for a requested source: an eye-gaze request
    /// degrades to head-gaze unless the runtime supports it.
    pub fn effective_source(&self, requested: GazeSource) -> GazeSource {
        match requested {
            GazeSource::Eye if self.eye_supported => GazeSource::Eye,
            _ => GazeSource::Head,
        }
    }

    /// Smooth a raw pose into a stable [`GazeRay`]. `forward` need not be
    /// normalised; the returned `dir` always is. `dt_s` is seconds since the
    /// previous call.
    pub fn resolve(
        &mut self,
        requested: GazeSource,
        origin: [f32; 3],
        forward: [f32; 3],
        dt_s: f32,
    ) -> GazeRay {
        let source = self.effective_source(requested);
        let smoothed_origin = [
            self.origin[0].filter(origin[0], dt_s),
            self.origin[1].filter(origin[1], dt_s),
            self.origin[2].filter(origin[2], dt_s),
        ];
        let smoothed_dir = [
            self.dir[0].filter(forward[0], dt_s),
            self.dir[1].filter(forward[1], dt_s),
            self.dir[2].filter(forward[2], dt_s),
        ];
        let ray = GazeRay {
            origin: smoothed_origin,
            dir: normalise(smoothed_dir),
            source,
        };
        trace!(?ray, "gaze resolved");
        ray
    }

    /// Reset all filters (e.g. on session recentre or tracking loss).
    pub fn reset(&mut self) {
        for f in self.origin.iter_mut().chain(self.dir.iter_mut()) {
            f.reset();
        }
    }
}

impl Default for GazeResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-6 || !len.is_finite() {
        return [0.0, 0.0, -1.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

// --- Godot node --------------------------------------------------------------

/// GDScript-facing gaze smoother. The scene feeds it the raw `XRCamera3D`
/// forward each frame (or the eye-gaze pose when supported) and reads back the
/// stable ray for the selection arbiter.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct GazeTracker {
    resolver: GazeResolver,
    last_origin: [f32; 3],
    last_dir: [f32; 3],
    last_source_eye: bool,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl GazeTracker {
    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            resolver: GazeResolver::new(),
            last_origin: [0.0, 0.0, 0.0],
            last_dir: [0.0, 0.0, -1.0],
            last_source_eye: false,
            base,
        })
    }

    /// Wire the runtime capability. Call once, after OpenXR init, with
    /// `OpenXRInterface.is_eye_gaze_interaction_supported()`.
    #[func]
    fn set_eye_gaze_supported(&mut self, supported: bool) {
        self.resolver.set_eye_gaze_supported(supported);
    }

    /// Smooth a raw pose and return the stable unit direction. `prefer_eye`
    /// requests the eye-gaze path; it degrades to head-gaze unless supported.
    #[func]
    fn resolve(&mut self, origin: Vector3, forward: Vector3, dt: f32, prefer_eye: bool) -> Vector3 {
        let requested = if prefer_eye {
            GazeSource::Eye
        } else {
            GazeSource::Head
        };
        let ray = self.resolver.resolve(
            requested,
            [origin.x, origin.y, origin.z],
            [forward.x, forward.y, forward.z],
            dt,
        );
        self.last_origin = ray.origin;
        self.last_dir = ray.dir;
        self.last_source_eye = ray.source == GazeSource::Eye;
        Vector3::new(ray.dir[0], ray.dir[1], ray.dir[2])
    }

    #[func]
    fn last_origin(&self) -> Vector3 {
        Vector3::new(
            self.last_origin[0],
            self.last_origin[1],
            self.last_origin[2],
        )
    }

    /// True when the last resolved ray actually used the eye-gaze pose.
    #[func]
    fn last_source_is_eye(&self) -> bool {
        self.last_source_eye
    }

    #[func]
    fn reset(&mut self) {
        self.resolver.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angle_between(a: [f32; 3], b: [f32; 3]) -> f32 {
        let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
        dot.acos()
    }

    #[test]
    fn first_sample_passes_through() {
        let mut f = OneEuroFilter::new(DEFAULT_MIN_CUTOFF, DEFAULT_BETA, DEFAULT_D_CUTOFF);
        assert_eq!(f.filter(3.5, 1.0 / 72.0), 3.5);
    }

    #[test]
    fn converges_to_constant_input() {
        let mut f = OneEuroFilter::new(DEFAULT_MIN_CUTOFF, DEFAULT_BETA, DEFAULT_D_CUTOFF);
        f.filter(0.0, 1.0 / 72.0);
        for _ in 0..500 {
            f.filter(10.0, 1.0 / 72.0);
        }
        assert!(
            (f.value() - 10.0).abs() < 1e-3,
            "did not converge: {}",
            f.value()
        );
    }

    #[test]
    fn output_never_overshoots_input_range() {
        // A low-pass output is a convex combination of samples, so it can never
        // leave the running [min, max] of the inputs — no overshoot / ringing.
        let mut f = OneEuroFilter::new(DEFAULT_MIN_CUTOFF, DEFAULT_BETA, DEFAULT_D_CUTOFF);
        let inputs = [0.0f32, 1.0, 0.9, 1.1, 1.0, 0.95, 1.05];
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &x in &inputs {
            lo = lo.min(x);
            hi = hi.max(x);
            let y = f.filter(x, 1.0 / 72.0);
            assert!(
                y >= lo - 1e-4 && y <= hi + 1e-4,
                "overshoot: {y} not in [{lo},{hi}]"
            );
        }
    }

    #[test]
    fn zero_dt_holds_last_value() {
        let mut f = OneEuroFilter::new(DEFAULT_MIN_CUTOFF, DEFAULT_BETA, DEFAULT_D_CUTOFF);
        f.filter(1.0, 1.0 / 72.0);
        let held = f.filter(99.0, 0.0);
        assert_eq!(held, f.value());
    }

    #[test]
    fn eye_gaze_degrades_to_head_when_unsupported() {
        let mut r = GazeResolver::new();
        assert_eq!(r.effective_source(GazeSource::Eye), GazeSource::Head);
        let ray = r.resolve(GazeSource::Eye, [0.0; 3], [0.0, 0.0, -1.0], 1.0 / 72.0);
        assert_eq!(ray.source, GazeSource::Head);
    }

    #[test]
    fn eye_gaze_used_when_supported() {
        let mut r = GazeResolver::new();
        r.set_eye_gaze_supported(true);
        assert_eq!(r.effective_source(GazeSource::Eye), GazeSource::Eye);
        let ray = r.resolve(GazeSource::Eye, [0.0; 3], [0.0, 0.0, -1.0], 1.0 / 72.0);
        assert_eq!(ray.source, GazeSource::Eye);
    }

    #[test]
    fn resolved_dir_is_unit_length() {
        let mut r = GazeResolver::new();
        let ray = r.resolve(GazeSource::Head, [0.0; 3], [0.0, 0.0, -5.0], 1.0 / 72.0);
        let len = (ray.dir[0].powi(2) + ray.dir[1].powi(2) + ray.dir[2].powi(2)).sqrt();
        assert!((len - 1.0).abs() < 1e-5);
    }

    #[test]
    fn smoothing_reduces_direction_jitter() {
        // Feed a jittery forward around a mean and confirm the smoothed output
        // is angularly closer to the mean than the raw samples are on average.
        let mut r = GazeResolver::new();
        let mean = normalise([0.0, 0.0, -1.0]);
        let jitter = [
            [0.05, 0.0, -1.0],
            [-0.05, 0.02, -1.0],
            [0.03, -0.04, -1.0],
            [-0.02, 0.03, -1.0],
            [0.04, 0.01, -1.0],
        ];
        // warm up
        r.resolve(GazeSource::Head, [0.0; 3], mean, 1.0 / 72.0);
        let mut raw_err = 0.0f32;
        let mut smooth_err = 0.0f32;
        for &j in jitter.iter().cycle().take(60) {
            let ray = r.resolve(GazeSource::Head, [0.0; 3], j, 1.0 / 72.0);
            raw_err += angle_between(normalise(j), mean);
            smooth_err += angle_between(ray.dir, mean);
        }
        assert!(
            smooth_err < raw_err,
            "smoothed {smooth_err} not < raw {raw_err}"
        );
    }
}
