//! Memory-query trajectory ribbon for the headset (XR WP7).
//!
//! The desktop explorer runs the HNSW route search and relays the winning
//! path as a `memoryRoute` text frame (snapshot row indices, root → answer);
//! the headset never runs HNSW. This module parses that frame, gates it
//! against the loaded snapshot and the last applied frame, samples the route
//! through the cloud exactly as the desktop "space" view does (quadratic
//! Bézier edges bowed out of the cloud, `layout.ts::spaceLayout`), and writes
//! the geometry `TrajectoryLayer.tsx` draws:
//!
//! - one ArrayMesh surface holding five parallel-transported tubes (outer and
//!   inner glow sheath, root → white → tip gradient body, white core, comet
//!   tail). Each vertex stores its centreline point, its unit ring direction
//!   (NORMAL), its route parameter (UV.x) and its layer radius (UV2.x), so the
//!   shader reveals the trace, tapers the tail behind the comet and pulses
//!   the glow without a per-frame rebuild;
//! - per-frame bead / comet instance buffers (stride 16);
//! - per-frame ring instance buffers (answer, pulse, root, sidecar marks).
//!
//! Glow is additive geometry only — no post-process (XR-client Invariant 2).

use serde::Deserialize;
use thiserror::Error;

#[cfg(not(test))]
use godot::prelude::*;

// ── desktop look (routeMath.ts / TrajectoryLayer.tsx) ──

/// `ROUTE_PALETTE` (root, mid, tip, mint, sidecar, miss); the drift test parses
/// `routeMath.ts`.
pub const ROUTE_ROOT: u32 = 0x6f9bff;
pub const ROUTE_MID: u32 = 0xffffff;
pub const ROUTE_TIP: u32 = 0xff7a3d;
pub const ROUTE_MINT: u32 = 0x7ef0cf;
pub const ROUTE_SIDECAR: u32 = 0xffd36e;
pub const ROUTE_MISS: u32 = 0xff5f6e;
/// Where the white core sits on the gradient.
pub const GRADIENT_MID: f32 = 0.42;
/// Path trace duration (s). XR has no search tree, so the trace starts at once.
pub const PATH_DUR: f32 = 2.8;
/// Converged comet loop period (s) and answer-pulse period without a beat (s).
pub const COMET_LOOP_S: f32 = 2.8;
pub const PULSE_PERIOD_S: f32 = 0.9;
/// Fraction of the route the comet tail covers.
pub const TAIL_FRACTION: f32 = 0.18;

// sizes in cloud-local units (TrajectoryLayer.tsx)
pub const TUBE_R: f32 = 0.35;
pub const BEAD_R: f32 = 0.55;
pub const COMET_R: f32 = 0.5;
pub const RING_R: f32 = 1.4;
pub const MARK_R: f32 = 1.7;

/// Ring vertices per tube cross-section (desktop 8; 6 keeps the headset share
/// of the triangle budget small).
pub const RADIAL: usize = 6;
/// Desktop samples per hop.
pub const ROUTE_SAMPLES: usize = 16;
/// Most centreline samples a route may use; long routes get fewer per hop.
pub const ROUTE_RING_CAP: usize = 320;
/// Longest path accepted from the wire.
pub const MAX_PATH: usize = 256;
/// Longest echoed query text.
pub const MAX_QUERY_CHARS: usize = 120;
/// Sidecar marks accepted (the sidecar returns at most 50).
pub const MAX_SIDECAR: usize = 64;
pub const STRIDE: usize = 16;

/// Tube layers in the single route surface, and their radius multiples of
/// `TUBE_R` (tail uses `COMET_R · 0.9`). Colour alpha is the layer's additive
/// weight (`sheathOuterMat 0.05`, `sheathInnerMat 0.11`, body/core opaque).
pub const LAYER_SHEATH_OUTER: usize = 0;
pub const LAYER_SHEATH_INNER: usize = 1;
pub const LAYER_BODY: usize = 2;
pub const LAYER_CORE: usize = 3;
pub const LAYER_TAIL: usize = 4;
pub const LAYERS: usize = 5;

pub type Vec3 = [f32; 3];

pub fn hex_rgb(hex: u32) -> Vec3 {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0]
}

fn lerp3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn normalise(v: Vec3) -> Vec3 {
    let l = dot(v, v).sqrt();
    if l > 1e-12 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0; 3]
    }
}
pub fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

pub mod ease {
    /// quadratic ease-in-out
    pub fn io(t: f32) -> f32 {
        if t < 0.5 {
            2.0 * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
        }
    }
    /// ease-out-back (bead pop)
    pub fn back(t: f32) -> f32 {
        let c = 1.70158f32;
        1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
    }
}

/// Route colour at u ∈ [0, 1]: root blue → white at 0.42 → orange tip.
pub fn route_gradient(u: f32) -> Vec3 {
    let t = clamp01(u);
    if t <= GRADIENT_MID {
        lerp3(hex_rgb(ROUTE_ROOT), hex_rgb(ROUTE_MID), t / GRADIENT_MID)
    } else {
        lerp3(hex_rgb(ROUTE_MID), hex_rgb(ROUTE_TIP), (t - GRADIENT_MID) / (1.0 - GRADIENT_MID))
    }
}

/// Bead pop-in at knot sample `knot` as the head passes (`beadScale`).
pub fn bead_scale(head: f32, knot: f32) -> f32 {
    let a = clamp01((head - knot) / 10.0 + 1.0);
    if a <= 0.0 {
        0.0
    } else {
        ease::back(a).max(0.0)
    }
}

// ── wire ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRoute {
    #[serde(rename = "type")]
    kind: String,
    snapshot_id: String,
    #[serde(default)]
    seq: Option<f64>,
    path: Vec<serde_json::Value>,
    #[serde(default)]
    sidecar: Vec<serde_json::Value>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    sent_at: Option<f64>,
}

/// A parsed `memoryRoute` frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteFrame {
    pub snapshot_id: String,
    pub seq: u64,
    pub sent_at: f64,
    /// Snapshot rows root → answer. Empty = clear the route.
    pub path: Vec<u32>,
    /// Sampled sidecar top-k rows (gold marks).
    pub sidecar: Vec<u32>,
    pub query: String,
}

#[derive(Debug, Error, PartialEq)]
pub enum RouteError {
    #[error("memoryRoute is not valid JSON: {0}")]
    Json(String),
    #[error("not a memoryRoute frame")]
    WrongType,
    #[error("memoryRoute snapshotId is empty")]
    EmptySnapshot,
    #[error("memoryRoute path entry {0} is not a row index")]
    BadIndex(usize),
    #[error("memoryRoute path has {0} entries (max {MAX_PATH})")]
    TooLong(usize),
}

fn as_row(v: &serde_json::Value) -> Option<u32> {
    let f = v.as_f64()?;
    if f >= 0.0 && f.fract() == 0.0 && f <= u32::MAX as f64 {
        Some(f as u32)
    } else {
        None
    }
}

pub fn parse_route(json: &str) -> Result<RouteFrame, RouteError> {
    let w: WireRoute = serde_json::from_str(json).map_err(|e| RouteError::Json(e.to_string()))?;
    if w.kind != "memoryRoute" {
        return Err(RouteError::WrongType);
    }
    if w.snapshot_id.trim().is_empty() {
        return Err(RouteError::EmptySnapshot);
    }
    if w.path.len() > MAX_PATH {
        return Err(RouteError::TooLong(w.path.len()));
    }
    let mut path = Vec::with_capacity(w.path.len());
    for (i, v) in w.path.iter().enumerate() {
        path.push(as_row(v).ok_or(RouteError::BadIndex(i))?);
    }
    // sidecar marks are decoration: drop bad entries rather than the frame
    let sidecar = w.sidecar.iter().filter_map(as_row).take(MAX_SIDECAR).collect();
    let query: String = w.query.unwrap_or_default().chars().take(MAX_QUERY_CHARS).collect();
    Ok(RouteFrame {
        snapshot_id: w.snapshot_id,
        seq: w.seq.filter(|s| s.is_finite() && *s >= 0.0).map_or(0, |s| s as u64),
        sent_at: w.sent_at.filter(|s| s.is_finite()).unwrap_or(0.0),
        path,
        sidecar,
        query,
    })
}

/// What to do with an incoming frame.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteDecision {
    /// Draw this route (path ≥ 2 rows, all in range).
    Apply(RouteFrame),
    /// Remove the current route.
    Clear,
    /// Older than (or equal to) the last applied frame.
    Stale,
    /// The route names another snapshot: reload the cloud once, then retry.
    Reload,
    /// Mismatched again after the reload, or out-of-range rows — drop it.
    Drop,
}

/// Orders frames and reconciles them with the loaded snapshot.
///
/// Ordering is `(sentAt, seq)`, so a desktop page reload (seq restarting at
/// 0, later sentAt) is accepted while a re-delivered older frame is not.
#[derive(Debug, Default)]
pub struct RouteGate {
    last: Option<(f64, u64)>,
    /// frame waiting for a snapshot reload, and the snapshot it named
    pending: Option<RouteFrame>,
    reloaded_for: Option<String>,
}

impl RouteGate {
    fn newer(&self, f: &RouteFrame) -> bool {
        match self.last {
            None => true,
            Some((t, s)) => f.sent_at > t || (f.sent_at == t && f.seq > s),
        }
    }

    pub fn offer(&mut self, f: RouteFrame, loaded: Option<&str>, row_count: usize) -> RouteDecision {
        if !self.newer(&f) {
            return RouteDecision::Stale;
        }
        if f.path.is_empty() {
            self.last = Some((f.sent_at, f.seq));
            self.pending = None;
            return RouteDecision::Clear;
        }
        if loaded != Some(f.snapshot_id.as_str()) {
            if self.reloaded_for.as_deref() == Some(f.snapshot_id.as_str()) {
                return RouteDecision::Drop;
            }
            self.reloaded_for = Some(f.snapshot_id.clone());
            self.pending = Some(f);
            return RouteDecision::Reload;
        }
        if f.path.len() < 2 || f.path.iter().any(|&r| r as usize >= row_count) {
            return RouteDecision::Drop;
        }
        self.last = Some((f.sent_at, f.seq));
        self.pending = None;
        RouteDecision::Apply(f)
    }

    /// After a reload attempt: re-offer the pending frame against the
    /// (possibly new) snapshot. A second mismatch drops it.
    pub fn after_reload(&mut self, loaded: Option<&str>, row_count: usize) -> Option<RouteDecision> {
        let f = self.pending.take()?;
        Some(self.offer(f, loaded, row_count))
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

// ── sampling (space view) ──

fn quad_point(a: Vec3, c: Vec3, b: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    [
        u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0],
        u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1],
        u * u * a[2] + 2.0 * u * t * c[2] + t * t * b[2],
    ]
}

/// Bézier control for edge a → b in the space view: the midpoint drawn a
/// quarter of the way back toward the parent and lifted by 0.12 · length.
pub fn space_control(a: Vec3, b: Vec3) -> Vec3 {
    let len = dot(sub(b, a), sub(b, a)).sqrt();
    [
        (a[0] + b[0]) / 2.0 + ((a[0] - b[0]) / 2.0) * 0.25,
        (a[1] + b[1]) / 2.0 + ((a[1] - b[1]) / 2.0) * 0.25 + 0.12 * len,
        (a[2] + b[2]) / 2.0 + ((a[2] - b[2]) / 2.0) * 0.25,
    ]
}

/// Samples per hop for a path of `rows` entries: the desktop's 16, reduced so
/// the whole route stays within `ROUTE_RING_CAP` centreline samples.
pub fn samples_per_hop(rows: usize) -> usize {
    let hops = rows.saturating_sub(1).max(1);
    ROUTE_SAMPLES.min((ROUTE_RING_CAP - 1) / hops).max(1)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteSamples {
    pub pts: Vec<Vec3>,
    /// sample index of each path node (first is 0)
    pub knots: Vec<usize>,
}

/// Dense polyline through the path rows (`sampleRoute`), `n` samples per hop,
/// shared endpoints once. Out-of-range rows sit on their parent, as
/// `spaceLayout` does.
pub fn sample_route(path: &[u32], positions: &[f32], n: usize) -> RouteSamples {
    if path.len() < 2 || n == 0 {
        return RouteSamples::default();
    }
    let mut at: Vec<Vec3> = Vec::with_capacity(path.len());
    for (i, &r) in path.iter().enumerate() {
        let o = r as usize * 3;
        let ok = o + 2 < positions.len() && positions[o..o + 3].iter().all(|v| v.is_finite());
        at.push(if ok {
            [positions[o], positions[o + 1], positions[o + 2]]
        } else if i > 0 {
            at[i - 1]
        } else {
            [0.0; 3]
        });
    }
    let mut pts = Vec::with_capacity((path.len() - 1) * n + 1);
    let mut knots = vec![0];
    for i in 0..path.len() - 1 {
        let (a, b) = (at[i], at[i + 1]);
        let c = space_control(a, b);
        for k in (if i == 0 { 0 } else { 1 })..=n {
            pts.push(quad_point(a, c, b, k as f32 / n as f32));
        }
        knots.push(pts.len() - 1);
    }
    RouteSamples { pts, knots }
}

/// Point at a fractional sample index, clamped.
pub fn point_at(pts: &[Vec3], idx: f32) -> Vec3 {
    let last = pts.len().saturating_sub(1);
    if pts.is_empty() {
        return [0.0; 3];
    }
    if idx <= 0.0 {
        return pts[0];
    }
    if idx >= last as f32 {
        return pts[last];
    }
    let i = idx.floor() as usize;
    lerp3(pts[i], pts[i + 1], idx - i as f32)
}

// ── tube mesh ──

/// Arrays for one ArrayMesh surface (Godot `Mesh.ARRAY_*`).
#[derive(Debug, Default, Clone)]
pub struct TubeMesh {
    /// centreline point per vertex (the shader pushes it out along `normal`)
    pub vertex: Vec<Vec3>,
    /// unit ring direction
    pub normal: Vec<Vec3>,
    pub colour: Vec<[f32; 4]>,
    /// (u along the route, layer id)
    pub uv: Vec<[f32; 2]>,
    /// (radius, 0)
    pub uv2: Vec<[f32; 2]>,
    pub index: Vec<i32>,
}

impl TubeMesh {
    pub fn triangles(&self) -> usize {
        self.index.len() / 3
    }
}

/// Parallel-transported ring frames along `pts` (`writeTube`): returns the
/// (normal, binormal) at each sample. Frames never twist.
pub fn transport_frames(pts: &[Vec3]) -> Vec<(Vec3, Vec3)> {
    let n = pts.len();
    if n == 0 {
        return Vec::new();
    }
    let tangent_at = |i: usize| normalise(sub(pts[(i + 1).min(n - 1)], pts[i.saturating_sub(1)]));
    let mut t = tangent_at(0);
    if dot(t, t) == 0.0 {
        t = [1.0, 0.0, 0.0];
    }
    let seed: Vec3 = if t[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let mut nrm = normalise(cross(cross(t, seed), t));
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i > 0 {
            let t2 = tangent_at(i);
            if dot(t2, t2) > 0.0 {
                let proj = dot(nrm, t2);
                let nn = normalise([nrm[0] - proj * t2[0], nrm[1] - proj * t2[1], nrm[2] - proj * t2[2]]);
                if dot(nn, nn) > 0.0 {
                    nrm = nn;
                }
                t = t2;
            }
        }
        out.push((nrm, normalise(cross(t, nrm))));
    }
    out
}

/// The five-layer route surface. Colours are at glow 1 (the desktop default
/// `routeGlow` is 1.2; the shader multiplies by the live glow).
pub fn build_route_mesh(pts: &[Vec3], glow: f32) -> TubeMesh {
    let mut m = TubeMesh::default();
    let n = pts.len();
    if n < 2 {
        return m;
    }
    let frames = transport_frames(pts);
    let tip = hex_rgb(ROUTE_TIP);
    let sheath_k = glow.min(1.5);
    let body_k = 0.85 + 0.55 * glow;
    let core_k = 0.9 + 0.5 * glow;
    let tail_rgb = [1.0, 244.0 / 255.0, 230.0 / 255.0];
    for layer in 0..LAYERS {
        let base = m.vertex.len() as i32;
        let radius = match layer {
            LAYER_SHEATH_OUTER => TUBE_R * 7.0,
            LAYER_SHEATH_INNER => TUBE_R * 3.6,
            LAYER_BODY => TUBE_R,
            LAYER_CORE => TUBE_R * 0.38,
            _ => COMET_R * 0.9,
        };
        for (i, p) in pts.iter().enumerate() {
            let u = i as f32 / (n - 1) as f32;
            let c: [f32; 4] = match layer {
                LAYER_SHEATH_OUTER => [tip[0] * sheath_k, tip[1] * sheath_k, tip[2] * sheath_k, 0.05],
                LAYER_SHEATH_INNER => [tip[0] * sheath_k, tip[1] * sheath_k, tip[2] * sheath_k, 0.11],
                LAYER_BODY => {
                    let g = route_gradient(u);
                    [g[0] * body_k, g[1] * body_k, g[2] * body_k, 1.0]
                }
                LAYER_CORE => [core_k, core_k, core_k, 1.0],
                _ => {
                    let k = 0.8 + 0.4 * glow;
                    [tail_rgb[0] * k, tail_rgb[1] * k, tail_rgb[2] * k, 0.9]
                }
            };
            let (nrm, bin) = frames[i];
            for k in 0..RADIAL {
                let a = k as f32 / RADIAL as f32 * std::f32::consts::TAU;
                let (s, co) = a.sin_cos();
                m.vertex.push(*p);
                m.normal.push([nrm[0] * co + bin[0] * s, nrm[1] * co + bin[1] * s, nrm[2] * co + bin[2] * s]);
                m.colour.push(c);
                m.uv.push([u, layer as f32]);
                m.uv2.push([radius, 0.0]);
            }
        }
        for i in 0..n - 1 {
            for k in 0..RADIAL {
                let a = base + (i * RADIAL + k) as i32;
                let b = base + (i * RADIAL + (k + 1) % RADIAL) as i32;
                let c = base + ((i + 1) * RADIAL + k) as i32;
                let d = base + ((i + 1) * RADIAL + (k + 1) % RADIAL) as i32;
                m.index.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
    }
    m
}

/// Triangles the route surface spends for `pts` samples.
pub fn route_triangles(samples: usize) -> usize {
    LAYERS * samples.saturating_sub(1) * RADIAL * 2
}

// ── animation ──

/// One frame of the route animation, in route-sample units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteFrameState {
    /// trace progress 0..1
    pub path_t: f32,
    /// head as a fractional sample index
    pub head: f32,
    /// head as u ∈ [0, 1] (shader `head_u`)
    pub head_u: f32,
    /// comet position (fractional sample) or None
    pub comet: Option<f32>,
    pub comet_size: f32,
    /// answer ring shown
    pub answer: bool,
    /// pulse ring phase 0..1, None when hidden
    pub pulse: Option<f32>,
    /// glow multiplier including the beat
    pub glow: f32,
}

/// `el` seconds after the route arrived. `beat` is `Some((phase, pulse))` when
/// a beat clock is locked (pulse is the 0..1 envelope). Under reduced motion
/// the route is shown converged with no comet, no pulse ring and no beat.
pub fn animate(samples: usize, el: f32, glow: f32, beat: Option<(f32, f32)>, reduced_motion: bool) -> RouteFrameState {
    let last = samples.saturating_sub(1) as f32;
    let beat = if reduced_motion { None } else { beat };
    let path_t = if reduced_motion { 1.0 } else { clamp01(el / PATH_DUR) };
    let head = if path_t < 1.0 { ease::io(path_t) * last } else { last };
    let glow = glow * beat.map_or(1.0, |(_, p)| 1.0 + 0.2 * p);
    let (comet, comet_size) = if samples < 2 {
        (None, 1.0)
    } else if path_t < 1.0 {
        (Some((head + beat.map_or(0.0, |(_, p)| 0.015 * p * last)).min(last)), 1.0)
    } else if !reduced_motion {
        (Some(((el / COMET_LOOP_S) % 1.0) * last), 0.7)
    } else {
        (None, 1.0)
    };
    let answer = path_t >= 1.0 && samples >= 2;
    let pulse = if answer && !reduced_motion {
        Some(beat.map_or((el / PULSE_PERIOD_S) % 1.0, |(ph, _)| clamp01(ph)))
    } else {
        None
    };
    RouteFrameState {
        path_t,
        head,
        head_u: if last > 0.0 { head / last } else { 0.0 },
        comet,
        comet_size,
        answer,
        pulse,
        glow,
    }
}

fn push_instance(buf: &mut Vec<f32>, p: Vec3, s: f32, c: [f32; 4]) {
    buf.extend_from_slice(&[s, 0.0, 0.0, p[0], 0.0, s, 0.0, p[1], 0.0, 0.0, s, p[2], c[0], c[1], c[2], c[3]]);
}

/// Bead + halo per knot (the root bead stays hidden), then the comet head and
/// its glow — one additive sphere MultiMesh, stride 16.
pub fn bead_buffer(r: &RouteSamples, st: &RouteFrameState, beat_pulse: f32) -> Vec<f32> {
    let mut buf = Vec::with_capacity((r.knots.len() * 2 + 2) * STRIDE);
    let last = r.pts.len().saturating_sub(1).max(1) as f32;
    let bright = 1.2 + 0.4 * st.glow;
    for (j, &ki) in r.knots.iter().enumerate() {
        let sc = if j == 0 || st.path_t <= 0.0 { 0.0 } else { bead_scale(st.head, ki as f32) };
        let p = r.pts.get(ki).copied().unwrap_or([0.0; 3]);
        let halo = if (ki as f32) / last < GRADIENT_MID { hex_rgb(ROUTE_ROOT) } else { hex_rgb(ROUTE_TIP) };
        push_instance(&mut buf, p, (BEAD_R * sc).max(1e-5), [bright, bright, bright, 1.0]);
        push_instance(&mut buf, p, (BEAD_R * 2.6 * sc * st.glow.min(1.5)).max(1e-5), [halo[0], halo[1], halo[2], 0.35]);
    }
    let (cp, size) = match st.comet {
        Some(c) => (point_at(&r.pts, c), st.comet_size),
        None => ([0.0; 3], 0.0),
    };
    let pulse = 1.0 + 0.35 * beat_pulse;
    let core = 1.5 + st.glow;
    let tip = hex_rgb(ROUTE_TIP);
    push_instance(&mut buf, cp, (COMET_R * size * pulse).max(1e-5), [core, core, core, 1.0]);
    push_instance(&mut buf, cp, (COMET_R * 5.0 * size * st.glow.min(1.6) * pulse).max(1e-5), [tip[0], tip[1], tip[2], 0.6]);
    buf
}

/// Camera-facing rings (stride 16; the shader billboards): root ring, answer
/// ring, pulse ring, then a gold mark per sidecar row present in the cloud.
/// Hidden rings get a near-zero scale so the instance count stays fixed.
pub fn ring_buffer(
    r: &RouteSamples,
    st: &RouteFrameState,
    sidecar: &[Vec3],
    root_pulse: f32,
) -> Vec<f32> {
    let mut buf = Vec::with_capacity((3 + sidecar.len()) * STRIDE);
    let root = r.pts.first().copied().unwrap_or([0.0; 3]);
    let tipp = r.pts.last().copied().unwrap_or([0.0; 3]);
    let tip = hex_rgb(ROUTE_TIP);
    let shown = r.pts.len() >= 2;
    push_instance(&mut buf, root, if shown { 1.8 * root_pulse } else { 1e-5 }, [0.9, 0.9, 0.9, 0.9]);
    let ak = 1.0 + 0.6 * st.glow;
    push_instance(&mut buf, tipp, if st.answer { RING_R } else { 1e-5 }, [tip[0] * ak, tip[1] * ak, tip[2] * ak, 1.0]);
    match st.pulse {
        Some(pr) => push_instance(&mut buf, tipp, RING_R * (1.0 + pr * 2.7), [tip[0], tip[1], tip[2], 1.0 - pr]),
        None => push_instance(&mut buf, tipp, 1e-5, [0.0; 4]),
    }
    let gold = hex_rgb(ROUTE_SIDECAR);
    for &p in sidecar {
        push_instance(&mut buf, p, MARK_R * 1.25, [gold[0], gold[1], gold[2], 0.95]);
    }
    buf
}

// ── state ──

/// A route being shown.
#[derive(Debug, Clone, Default)]
pub struct ActiveRoute {
    pub frame: Option<RouteFrame>,
    pub samples: RouteSamples,
    pub sidecar_pts: Vec<Vec3>,
    pub el: f32,
}

impl ActiveRoute {
    pub fn set(&mut self, f: RouteFrame, positions: &[f32]) {
        self.samples = sample_route(&f.path, positions, samples_per_hop(f.path.len()));
        self.sidecar_pts = f
            .sidecar
            .iter()
            .filter_map(|&r| {
                let o = r as usize * 3;
                (o + 2 < positions.len()).then(|| [positions[o], positions[o + 1], positions[o + 2]])
            })
            .collect();
        self.frame = Some(f);
        self.el = 0.0;
    }
    pub fn clear(&mut self) {
        *self = ActiveRoute::default();
    }
    /// Rows to keep lit in the cloud: the path and the sidecar hits.
    pub fn keep_rows(&self) -> Vec<u32> {
        self.frame.as_ref().map_or(Vec::new(), |f| f.path.iter().chain(f.sidecar.iter()).copied().collect())
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
fn v3(p: Vec3) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct MemoryRoute {
    gate: RouteGate,
    route: ActiveRoute,
    state: Option<RouteFrameState>,
    last_error: String,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl MemoryRoute {
    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            gate: RouteGate::default(),
            route: ActiveRoute::default(),
            state: None,
            last_error: String::new(),
            base,
        })
    }

    /// Offer a `memoryRoute` text frame. Returns one of "apply", "clear",
    /// "stale", "reload", "drop" or "error" (see `last_error()`). On "apply"
    /// the route is sampled from `positions` (the loaded cloud's 3·count floats).
    #[func]
    fn offer_json(&mut self, json: GString, loaded_snapshot: GString, positions: PackedFloat32Array) -> GString {
        let f = match parse_route(&json.to_string()) {
            Ok(f) => f,
            Err(e) => {
                self.last_error = e.to_string();
                return GString::from("error");
            }
        };
        let loaded = loaded_snapshot.to_string();
        let d = self.gate.offer(f, (!loaded.is_empty()).then_some(loaded.as_str()), positions.len() / 3);
        self.apply_decision(d, positions.as_slice())
    }

    /// Re-offer the frame held for a reload, against the snapshot now loaded.
    /// Returns "" when nothing was pending.
    #[func]
    fn after_reload(&mut self, loaded_snapshot: GString, positions: PackedFloat32Array) -> GString {
        let loaded = loaded_snapshot.to_string();
        match self.gate.after_reload((!loaded.is_empty()).then_some(loaded.as_str()), positions.len() / 3) {
            Some(d) => self.apply_decision(d, positions.as_slice()),
            None => GString::new(),
        }
    }

    #[func]
    fn has_pending(&self) -> bool {
        self.gate.has_pending()
    }

    #[func]
    fn clear(&mut self) {
        self.route.clear();
        self.state = None;
    }

    #[func]
    fn last_error(&self) -> GString {
        GString::from(self.last_error.as_str())
    }

    #[func]
    fn is_active(&self) -> bool {
        self.route.frame.is_some() && self.route.samples.pts.len() >= 2
    }

    #[func]
    fn snapshot_id(&self) -> GString {
        GString::from(self.route.frame.as_ref().map_or("", |f| f.snapshot_id.as_str()))
    }

    #[func]
    fn query_text(&self) -> GString {
        GString::from(self.route.frame.as_ref().map_or("", |f| f.query.as_str()))
    }

    #[func]
    fn hop_count(&self) -> i64 {
        self.route.frame.as_ref().map_or(0, |f| f.path.len().saturating_sub(1) as i64)
    }

    #[func]
    fn keep_rows(&self) -> PackedInt32Array {
        PackedInt32Array::from(self.route.keep_rows().into_iter().map(|r| r as i32).collect::<Vec<_>>().as_slice())
    }

    #[func]
    fn sample_count(&self) -> i64 {
        self.route.samples.pts.len() as i64
    }

    #[func]
    fn triangle_estimate(&self) -> i64 {
        route_triangles(self.route.samples.pts.len()) as i64
    }

    /// Mesh arrays for the route surface (Mesh.ARRAY_MAX-sized Array).
    #[func]
    fn mesh_arrays(&self, glow: f32) -> VariantArray {
        let m = build_route_mesh(&self.route.samples.pts, glow);
        let mut arr = VariantArray::new();
        // Mesh.ARRAY_MAX = 13: VERTEX, NORMAL, TANGENT, COLOR, TEX_UV, TEX_UV2,
        // CUSTOM0..3, BONES, WEIGHTS, INDEX
        for _ in 0..13 {
            arr.push(&Variant::nil());
        }
        if m.vertex.is_empty() {
            return arr;
        }
        let verts: Vec<Vector3> = m.vertex.iter().map(|&p| v3(p)).collect();
        let norms: Vec<Vector3> = m.normal.iter().map(|&p| v3(p)).collect();
        let cols: Vec<Color> = m.colour.iter().map(|c| Color::from_rgba(c[0], c[1], c[2], c[3])).collect();
        let uv: Vec<Vector2> = m.uv.iter().map(|u| Vector2::new(u[0], u[1])).collect();
        let uv2: Vec<Vector2> = m.uv2.iter().map(|u| Vector2::new(u[0], u[1])).collect();
        arr.set(0, &PackedVector3Array::from(verts.as_slice()).to_variant());
        arr.set(1, &PackedVector3Array::from(norms.as_slice()).to_variant());
        arr.set(3, &PackedColorArray::from(cols.as_slice()).to_variant());
        arr.set(4, &PackedVector2Array::from(uv.as_slice()).to_variant());
        arr.set(5, &PackedVector2Array::from(uv2.as_slice()).to_variant());
        arr.set(12, &PackedInt32Array::from(m.index.as_slice()).to_variant());
        arr
    }

    /// Advance the animation by `dt`. `beat_phase` < 0 means no locked beat.
    /// Returns the shader parameters as a Dictionary: head_u, tail_u, glow,
    /// path_t, answer, comet (bool).
    #[func]
    fn tick(&mut self, dt: f32, glow: f32, beat_phase: f32, beat_pulse: f32, reduced_motion: bool) -> Dictionary {
        self.route.el += dt.clamp(0.0, 0.1);
        let beat = (beat_phase >= 0.0).then_some((beat_phase, clamp01(beat_pulse)));
        let st = animate(self.route.samples.pts.len(), self.route.el, glow, beat, reduced_motion);
        self.state = Some(st);
        let last = self.route.samples.pts.len().saturating_sub(1).max(1) as f32;
        let mut d = Dictionary::new();
        d.set("head_u", st.head_u);
        d.set("comet_u", st.comet.map_or(-1.0, |c| c / last));
        d.set("tail_u", TAIL_FRACTION);
        d.set("glow", st.glow);
        d.set("path_t", st.path_t);
        d.set("answer", st.answer);
        d
    }

    #[func]
    fn bead_buffer(&self, beat_pulse: f32) -> PackedFloat32Array {
        let Some(st) = self.state else { return PackedFloat32Array::new() };
        PackedFloat32Array::from(bead_buffer(&self.route.samples, &st, clamp01(beat_pulse)).as_slice())
    }

    #[func]
    fn ring_buffer(&self, root_pulse: f32) -> PackedFloat32Array {
        let Some(st) = self.state else { return PackedFloat32Array::new() };
        PackedFloat32Array::from(ring_buffer(&self.route.samples, &st, &self.route.sidecar_pts, root_pulse).as_slice())
    }
}

#[cfg(not(test))]
impl MemoryRoute {
    fn apply_decision(&mut self, d: RouteDecision, positions: &[f32]) -> GString {
        match d {
            RouteDecision::Apply(f) => {
                self.route.set(f, positions);
                self.state = None;
                GString::from("apply")
            }
            RouteDecision::Clear => {
                self.route.clear();
                self.state = None;
                GString::from("clear")
            }
            RouteDecision::Stale => GString::from("stale"),
            RouteDecision::Reload => GString::from("reload"),
            RouteDecision::Drop => GString::from("drop"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(snap: &str, seq: u64, sent: f64, path: &[u32]) -> RouteFrame {
        RouteFrame { snapshot_id: snap.into(), seq, sent_at: sent, path: path.to_vec(), sidecar: vec![], query: String::new() }
    }

    // ── parse ──

    #[test]
    fn parses_the_agreed_frame() {
        let j = r#"{"type":"memoryRoute","snapshotId":"s1","seq":7,"path":[4,9,2],"sidecar":[2,11,-1,"x"],"query":"how do ADRs work","sentAt":1000,"extra":true}"#;
        let f = parse_route(j).unwrap();
        assert_eq!(f.snapshot_id, "s1");
        assert_eq!(f.seq, 7);
        assert_eq!(f.path, vec![4, 9, 2]);
        assert_eq!(f.sidecar, vec![2, 11], "bad sidecar entries dropped, frame kept");
        assert_eq!(f.query, "how do ADRs work");
        assert_eq!(f.sent_at, 1000.0);
    }

    #[test]
    fn rejects_malformed_routes() {
        assert!(matches!(parse_route("nope"), Err(RouteError::Json(_))));
        assert!(matches!(parse_route(r#"{"type":"memoryRoute"}"#), Err(RouteError::Json(_))));
        assert_eq!(parse_route(r#"{"type":"beatClock","snapshotId":"s","path":[]}"#), Err(RouteError::WrongType));
        assert_eq!(parse_route(r#"{"type":"memoryRoute","snapshotId":"","path":[]}"#), Err(RouteError::EmptySnapshot));
        assert_eq!(parse_route(r#"{"type":"memoryRoute","snapshotId":"s","path":[1,-2]}"#), Err(RouteError::BadIndex(1)));
        assert_eq!(parse_route(r#"{"type":"memoryRoute","snapshotId":"s","path":[1,2.5]}"#), Err(RouteError::BadIndex(1)));
        assert_eq!(parse_route(r#"{"type":"memoryRoute","snapshotId":"s","path":["1"]}"#), Err(RouteError::BadIndex(0)));
        let long: Vec<String> = (0..MAX_PATH + 1).map(|i| i.to_string()).collect();
        let j = format!(r#"{{"type":"memoryRoute","snapshotId":"s","path":[{}]}}"#, long.join(","));
        assert_eq!(parse_route(&j), Err(RouteError::TooLong(MAX_PATH + 1)));
    }

    #[test]
    fn query_is_truncated_and_missing_seq_is_zero() {
        let q = "x".repeat(500);
        let j = format!(r#"{{"type":"memoryRoute","snapshotId":"s","path":[],"query":"{q}"}}"#);
        let f = parse_route(&j).unwrap();
        assert_eq!(f.query.chars().count(), MAX_QUERY_CHARS);
        assert_eq!(f.seq, 0);
    }

    // ── gate ──

    #[test]
    fn gate_applies_in_order_and_drops_stale() {
        let mut g = RouteGate::default();
        assert!(matches!(g.offer(frame("s", 2, 100.0, &[0, 1]), Some("s"), 5), RouteDecision::Apply(_)));
        assert_eq!(g.offer(frame("s", 1, 100.0, &[0, 1]), Some("s"), 5), RouteDecision::Stale);
        assert_eq!(g.offer(frame("s", 2, 100.0, &[0, 1]), Some("s"), 5), RouteDecision::Stale);
        assert!(matches!(g.offer(frame("s", 3, 100.0, &[0, 1]), Some("s"), 5), RouteDecision::Apply(_)));
        // a reloaded desktop restarts seq but sends later
        assert!(matches!(g.offer(frame("s", 0, 200.0, &[1, 2]), Some("s"), 5), RouteDecision::Apply(_)));
    }

    #[test]
    fn gate_clears_on_empty_path() {
        let mut g = RouteGate::default();
        assert_eq!(g.offer(frame("s", 1, 1.0, &[]), Some("s"), 5), RouteDecision::Clear);
        // a clear for another snapshot still clears: there is nothing to place
        assert_eq!(g.offer(frame("other", 2, 2.0, &[]), Some("s"), 5), RouteDecision::Clear);
    }

    #[test]
    fn gate_reloads_once_on_snapshot_mismatch() {
        let mut g = RouteGate::default();
        assert_eq!(g.offer(frame("new", 1, 1.0, &[0, 1]), Some("old"), 5), RouteDecision::Reload);
        assert!(g.has_pending());
        // the reload brought the matching snapshot → applied
        assert!(matches!(g.after_reload(Some("new"), 5), Some(RouteDecision::Apply(_))));
        assert!(!g.has_pending());
        assert_eq!(g.after_reload(Some("new"), 5), None);
    }

    #[test]
    fn gate_drops_after_a_reload_that_did_not_help() {
        let mut g = RouteGate::default();
        assert_eq!(g.offer(frame("new", 1, 1.0, &[0, 1]), Some("old"), 5), RouteDecision::Reload);
        assert_eq!(g.after_reload(Some("old"), 5), Some(RouteDecision::Drop));
        // later frames naming the same unseen snapshot are dropped, not reloaded again
        assert_eq!(g.offer(frame("new", 2, 2.0, &[0, 1]), Some("old"), 5), RouteDecision::Drop);
        // but a different snapshot gets its own single reload
        assert_eq!(g.offer(frame("newer", 3, 3.0, &[0, 1]), Some("old"), 5), RouteDecision::Reload);
    }

    #[test]
    fn gate_reloads_when_nothing_is_loaded() {
        let mut g = RouteGate::default();
        assert_eq!(g.offer(frame("s", 1, 1.0, &[0, 1]), None, 0), RouteDecision::Reload);
    }

    #[test]
    fn gate_drops_out_of_range_rows_and_single_node_paths() {
        let mut g = RouteGate::default();
        assert_eq!(g.offer(frame("s", 1, 1.0, &[0, 5]), Some("s"), 5), RouteDecision::Drop);
        assert_eq!(g.offer(frame("s", 2, 2.0, &[3]), Some("s"), 5), RouteDecision::Drop);
    }

    // ── palette drift ──

    fn ts_source(rel: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    fn ts_hex(src: &str, field: &str) -> u32 {
        let block = &src[src.find("export const ROUTE_PALETTE").unwrap()..];
        let needle = format!("{field}: '#");
        let at = block.find(&needle).unwrap_or_else(|| panic!("{field} missing")) + needle.len();
        u32::from_str_radix(&block[at..at + 6], 16).unwrap()
    }

    #[test]
    fn route_palette_matches_route_math_ts() {
        let src = ts_source("client/src/features/visualisation/memoryCloud/routeMath.ts");
        assert_eq!(ts_hex(&src, "root"), ROUTE_ROOT);
        assert_eq!(ts_hex(&src, "mid"), ROUTE_MID);
        assert_eq!(ts_hex(&src, "tip"), ROUTE_TIP);
        assert_eq!(ts_hex(&src, "mint"), ROUTE_MINT);
        assert_eq!(ts_hex(&src, "sidecar"), ROUTE_SIDECAR);
        assert_eq!(ts_hex(&src, "miss"), ROUTE_MISS);
        assert!(src.contains("export const GRADIENT_MID = 0.42;"));
        assert!(src.contains("export const PATH_DUR = 2.8;"));
        assert!(src.contains("const tail = Math.min(head, last * 0.18);"));
    }

    #[test]
    fn trajectory_sizes_match_trajectory_layer_tsx() {
        let src = ts_source("client/src/features/visualisation/memoryCloud/TrajectoryLayer.tsx");
        for (name, v) in [("TUBE_R", TUBE_R), ("BEAD_R", BEAD_R), ("COMET_R", COMET_R), ("RING_R", RING_R), ("MARK_R", MARK_R)] {
            let needle = format!("const {name} = ");
            let at = src.find(&needle).unwrap_or_else(|| panic!("{name}")) + needle.len();
            let lit: String = src[at..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            assert_eq!(lit.parse::<f32>().unwrap(), v, "{name}");
        }
        assert!(src.contains(&format!("const ROUTE_SAMPLES = {ROUTE_SAMPLES};")));
        let layout = ts_source("client/src/features/visualisation/memoryTrajectory/layout.ts");
        assert!(layout.contains("0.12 * len"), "space-view lift");
    }

    // ── maths ──

    #[test]
    fn gradient_runs_root_white_tip() {
        assert_eq!(route_gradient(0.0), hex_rgb(ROUTE_ROOT));
        assert!(route_gradient(GRADIENT_MID).iter().all(|c| (c - 1.0).abs() < 1e-6), "white core");
        assert!(route_gradient(1.0).iter().zip(hex_rgb(ROUTE_TIP)).all(|(a, b)| (a - b).abs() < 1e-6));
        assert_eq!(route_gradient(-3.0), hex_rgb(ROUTE_ROOT));
    }

    #[test]
    fn bead_pops_past_its_knot() {
        assert_eq!(bead_scale(0.0, 20.0), 0.0);
        assert!((bead_scale(20.0, 20.0) - 1.0).abs() < 1e-6);
        assert!(bead_scale(15.0, 20.0) > 1.0, "overshoot on the way in");
    }

    #[test]
    fn route_samples_share_endpoints_and_hit_the_rows() {
        let pos = [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0];
        let r = sample_route(&[0, 1, 2], &pos, 4);
        assert_eq!(r.pts.len(), 9);
        assert_eq!(r.knots, vec![0, 4, 8]);
        assert_eq!(r.pts[0], [0.0, 0.0, 0.0]);
        assert_eq!(r.pts[4], [10.0, 0.0, 0.0]);
        assert_eq!(r.pts[8], [10.0, 10.0, 0.0]);
        // edges bow upward (space-view lift)
        assert!(r.pts[2][1] > 0.0);
    }

    #[test]
    fn route_with_missing_rows_sits_on_the_parent() {
        let pos = [1.0, 2.0, 3.0];
        let r = sample_route(&[0, 99], &pos, 2);
        assert_eq!(r.pts, vec![[1.0, 2.0, 3.0]; 3]);
        assert!(sample_route(&[0], &pos, 2).pts.is_empty());
    }

    #[test]
    fn long_routes_stay_within_the_ring_cap() {
        assert_eq!(samples_per_hop(2), ROUTE_SAMPLES);
        assert_eq!(samples_per_hop(8), ROUTE_SAMPLES);
        for rows in [2usize, 10, 21, 40, 100, MAX_PATH] {
            let total = (rows - 1) * samples_per_hop(rows) + 1;
            assert!(total <= ROUTE_RING_CAP, "{rows} rows → {total} samples");
        }
        assert!(route_triangles(ROUTE_RING_CAP) <= 20_000);
    }

    #[test]
    fn transport_frames_are_orthonormal_and_do_not_twist() {
        let pts: Vec<Vec3> = (0..50).map(|i| { let a = i as f32 * 0.1; [a.cos() * 10.0, i as f32 * 0.2, a.sin() * 10.0] }).collect();
        let fr = transport_frames(&pts);
        for (i, (n, b)) in fr.iter().enumerate() {
            assert!((dot(*n, *n) - 1.0).abs() < 1e-4, "unit normal at {i}");
            assert!((dot(*b, *b) - 1.0).abs() < 1e-4);
            assert!(dot(*n, *b).abs() < 1e-4);
            if i > 0 {
                assert!(dot(fr[i - 1].0, *n) > 0.9, "no flip between {i} and its predecessor");
            }
        }
        // a straight vertical line (tangent parallel to the default seed)
        let fr = transport_frames(&[[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]]);
        assert!((dot(fr[0].0, fr[0].0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn route_mesh_has_five_layers_and_valid_indices() {
        let pos = [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0];
        let r = sample_route(&[0, 1, 2], &pos, 16);
        let m = build_route_mesh(&r.pts, 1.2);
        let per_layer = r.pts.len() * RADIAL;
        assert_eq!(m.vertex.len(), per_layer * LAYERS);
        assert_eq!(m.triangles(), route_triangles(r.pts.len()));
        assert!(m.index.iter().all(|&i| i >= 0 && (i as usize) < m.vertex.len()));
        for l in 0..LAYERS {
            assert_eq!(m.uv[l * per_layer][1], l as f32);
        }
        // body gradient: root blue at the start, tip orange at the end
        let body0 = m.colour[LAYER_BODY * per_layer];
        let body_end = m.colour[LAYER_BODY * per_layer + per_layer - 1];
        assert!(body0[2] > body0[0], "root is blue");
        assert!(body_end[0] > body_end[2], "tip is orange");
        // ring directions are unit length and vertices sit on the centreline
        assert!(m.normal.iter().all(|n| (dot(*n, *n) - 1.0).abs() < 1e-3));
        assert_eq!(m.vertex[0], r.pts[0]);
        assert!(build_route_mesh(&r.pts[..1], 1.0).vertex.is_empty());
    }

    #[test]
    fn animation_traces_then_loops_and_pulses() {
        let st = animate(101, 0.0, 1.2, None, false);
        assert_eq!(st.path_t, 0.0);
        assert_eq!(st.head, 0.0);
        assert!(!st.answer);
        let st = animate(101, PATH_DUR / 2.0, 1.2, None, false);
        assert!((st.head - 50.0).abs() < 1e-3);
        assert_eq!(st.comet, Some(st.head));
        let st = animate(101, PATH_DUR + 0.45, 1.2, None, false);
        assert!(st.answer);
        assert_eq!(st.head_u, 1.0);
        assert_eq!(st.comet_size, 0.7);
        assert!((st.pulse.unwrap() - ((PATH_DUR + 0.45) / PULSE_PERIOD_S) % 1.0).abs() < 1e-5);
    }

    #[test]
    fn beat_locks_the_pulse_and_lifts_the_glow() {
        let st = animate(101, 10.0, 1.0, Some((0.25, 1.0)), false);
        assert_eq!(st.pulse, Some(0.25));
        assert!((st.glow - 1.2).abs() < 1e-6);
    }

    #[test]
    fn reduced_motion_shows_a_still_converged_route() {
        let st = animate(101, 0.0, 1.0, Some((0.5, 1.0)), true);
        assert_eq!(st.path_t, 1.0);
        assert!(st.answer);
        assert_eq!(st.comet, None);
        assert_eq!(st.pulse, None);
        assert_eq!(st.glow, 1.0, "beat ignored under reduced motion");
    }

    #[test]
    fn instance_buffers_have_fixed_counts() {
        let pos = [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0];
        let r = sample_route(&[0, 1, 2], &pos, 16);
        let st = animate(r.pts.len(), 1.0, 1.2, None, false);
        let b = bead_buffer(&r, &st, 0.0);
        assert_eq!(b.len(), (r.knots.len() * 2 + 2) * STRIDE);
        assert!(b[0] < 1e-4, "root bead hidden");
        let rings = ring_buffer(&r, &st, &[[1.0, 1.0, 1.0]], 1.0);
        assert_eq!(rings.len(), 4 * STRIDE);
        assert!(rings[STRIDE] < 1e-4, "answer ring hidden mid-trace");
        let st = animate(r.pts.len(), 10.0, 1.2, None, false);
        let rings = ring_buffer(&r, &st, &[], 1.0);
        assert_eq!(rings[STRIDE], RING_R);
        assert_eq!([rings[STRIDE + 3], rings[STRIDE + 7], rings[STRIDE + 11]], [10.0, 10.0, 0.0]);
    }

    #[test]
    fn active_route_keeps_path_and_sidecar_rows_lit() {
        let mut a = ActiveRoute::default();
        let mut f = frame("s", 1, 1.0, &[0, 1]);
        f.sidecar = vec![1, 7];
        a.set(f, &[0.0; 6]);
        assert_eq!(a.keep_rows(), vec![0, 1, 1, 7]);
        assert_eq!(a.sidecar_pts.len(), 1, "row 7 is out of range");
        a.clear();
        assert!(a.keep_rows().is_empty());
    }
}
