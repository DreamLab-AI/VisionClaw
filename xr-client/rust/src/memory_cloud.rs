//! Live RuVector memory cloud for the headset (ADR-2133, XR WP6).
//!
//! The desktop explorer (`EmbeddingCloudLayer.tsx`) draws `GET /api/memory-cloud`
//! as point sprites around the graph. This module is the headset half of that
//! contract: it parses the snapshot JSON (positions + metadata only; the
//! vectors blob is never fetched, because the headset never runs HNSW),
//! colours rows with the same tables as `memoryCloud/cloudData.ts`, picks a
//! deterministic level-of-detail subset that fits the triangle budget, packs
//! the MultiMesh instance buffer, resolves `memory_flash` targets onto real
//! rows, ray-picks a hovered row, and owns the 401/403 · 409 · 503 load policy
//! from `memoryCloudStore.ts`.
//!
//! Everything above the `MemoryCloud` Godot class is plain Rust so the headless
//! `cargo test` covers it; the class is a thin adapter.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use thiserror::Error;

#[cfg(not(test))]
use godot::prelude::*;

// ── desktop constants (EmbeddingCloudLayer.tsx / GraphCanvas.tsx defaults) ──

/// `visualisation.embeddingCloud.cloudScale` default: the cloud group's scale
/// relative to graph (server) units.
pub const DESKTOP_CLOUD_SCALE: f32 = 5.0;
/// `pointSize` default for the size-attenuated `pointsMaterial`.
pub const DESKTOP_POINT_SIZE: f32 = 7.5;
/// Desktop camera vertical field of view (GraphCanvas `fov: 75`).
pub const DESKTOP_FOV_DEG: f32 = 75.0;
/// `opacity` default.
pub const DESKTOP_OPACITY: f32 = 0.6;
/// `dimOffRoute` default: focus-pull dimming applied to rows off the route.
pub const DESKTOP_DIM_OFF_ROUTE: f32 = 0.75;
/// `rotationSpeed` default, radians per rendered frame at the desktop's 60 Hz.
pub const DESKTOP_ROTATION_PER_FRAME: f32 = 0.0005;

/// Hard ceiling on a snapshot the headset will accept (the server clamps its
/// sample to 20 000; anything far past that is a malformed or hostile payload).
pub const MAX_SNAPSHOT_ROWS: usize = 50_000;
/// Triangles per sprite: one equilateral triangle circumscribing the disc
/// (`SPRITE_TRIANGLE_UV`), half a quad's cost for ~30 % more covered pixels,
/// all but the disc discarded.
pub const TRIANGLES_PER_SPRITE: usize = 1;
/// Most sprites the cloud draws (its demand in `frame_budget`, which may cap
/// it lower). The server's default sample (6000) draws in full; larger samples
/// are level-of-detail subsampled.
pub const DEFAULT_SPRITE_CAP: usize = 8_000;
/// UVs of the sprite triangle; the mesh position is `uv - 0.5` (unit-diameter
/// disc). Its incircle is the shader's disc (radius 0.5 about (0.5, 0.5)), so
/// the round mask never loses a pixel.
pub const SPRITE_TRIANGLE_UV: [[f32; 2]; 3] = [
    [0.5, -0.5],
    [0.5 - 0.866_025_4, 1.0],
    [0.5 + 0.866_025_4, 1.0],
];
/// MultiMesh stride: 12 transform + 4 colour (`use_colors`) + 4 custom
/// (`use_custom_data`, the flash emphasis).
pub const CLOUD_STRIDE: usize = 20;
/// Per-instance custom data for an unemphasised sprite: (tint rgb, gain).
/// `memory_point.gdshader` blends toward the tint by (gain - 1) / 1.5 and
/// brightens by gain, so gain 1 leaves the sprite untouched. memory_flash
/// bursts (`set_row_emphasis`) write it per instance; no geometry is added.
pub const NEUTRAL_EMPHASIS: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Categorical palette (16, then cycles). Mirror of `CLOUD_PALETTE` in
/// `client/src/features/visualisation/memoryCloud/cloudData.ts`; the drift test
/// parses that file.
pub const CLOUD_PALETTE: [u32; 16] = [
    0x4fc3f7, 0xaa96da, 0x81c784, 0xffb74d, 0xef5350, 0x26c6da, 0xfff176, 0xce93d8, 0xa1887f,
    0x90a4ae, 0x4db6ac, 0xf06292, 0xaed581, 0x7986cb, 0xffcc80, 0xe0e0e0,
];

/// Age ramp: oldest deep blue → mid blue → newest mint (`AGE_STOPS`).
pub const AGE_STOPS: [(f32, u32); 3] = [(0.0, 0x1d2a52), (0.6, 0x6f9bff), (1.0, 0x7ef0cf)];

/// Most namespace stand-ins lit for a flash whose key is not sampled
/// (`NAMESPACE_PICKS`).
pub const NAMESPACE_PICKS: usize = 3;

/// Reload delay after a 503 without Retry-After, and the clamp bounds
/// (`RETRY_DEFAULT_MS`, `RETRY_MIN_MS`, `RETRY_MAX_MS`).
pub const RETRY_DEFAULT_MS: i64 = 5_000;
pub const RETRY_MIN_MS: i64 = 1_000;
pub const RETRY_MAX_MS: i64 = 60_000;

/// Sprite diameter in cloud-local units that matches the desktop look.
///
/// three.js draws a size-attenuated point `size · (H/2) / d` pixels tall at
/// depth `d`; an object of world diameter `w` spans `w · (H/2) / (d · tan(fov/2))`
/// pixels. Equating them gives `w = size · tan(fov/2)` in desktop world units,
/// and the cloud group's `cloudScale` turns that into cloud-local units.
pub fn sprite_local_size(point_size: f32, fov_deg: f32, cloud_scale: f32) -> f32 {
    let half = (fov_deg.to_radians() * 0.5).tan();
    point_size * half / cloud_scale.max(1e-6)
}

// ── wire ──

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireMeta {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    updated_at: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSnapshot {
    version: u32,
    snapshot_id: String,
    #[serde(default)]
    generated_at: Option<f64>,
    count: usize,
    positions: Vec<f32>,
    metadata: Vec<WireMeta>,
    #[serde(default)]
    namespaces: Vec<String>,
    #[serde(default)]
    source_types: Vec<String>,
}

/// One sampled memory entry, in row order.
#[derive(Debug, Clone, PartialEq)]
pub struct CloudMeta {
    pub id: String,
    pub key: String,
    pub namespace: String,
    pub source_type: String,
    /// Unix milliseconds (0 when absent).
    pub updated_at: f64,
}

/// A validated snapshot.
#[derive(Debug, Clone)]
pub struct CloudSnapshot {
    pub snapshot_id: String,
    pub generated_at: f64,
    pub count: usize,
    /// `3 · count` finite floats, cloud-local (≈ ±100).
    pub positions: Vec<f32>,
    pub metadata: Vec<CloudMeta>,
    pub namespaces: Vec<String>,
    pub source_types: Vec<String>,
}

#[derive(Debug, Error, PartialEq)]
pub enum CloudError {
    #[error("snapshot is not valid JSON for the memory-cloud contract: {0}")]
    Json(String),
    #[error("unsupported snapshot version {0} (expected 1)")]
    Version(u32),
    #[error("snapshotId is empty")]
    EmptyId,
    #[error("count {count} disagrees with {positions} position floats / {metadata} metadata rows")]
    Shape {
        count: usize,
        positions: usize,
        metadata: usize,
    },
    #[error("snapshot has {0} rows, above the headset ceiling")]
    TooLarge(usize),
}

impl CloudSnapshot {
    /// Parse and validate `GET /api/memory-cloud`. Unknown fields (`dim`,
    /// `strata`, `vectorsUrl`, …) are ignored.
    pub fn parse(bytes: &[u8]) -> Result<CloudSnapshot, CloudError> {
        let w: WireSnapshot =
            serde_json::from_slice(bytes).map_err(|e| CloudError::Json(e.to_string()))?;
        if w.version != 1 {
            return Err(CloudError::Version(w.version));
        }
        if w.snapshot_id.trim().is_empty() {
            return Err(CloudError::EmptyId);
        }
        if w.count > MAX_SNAPSHOT_ROWS {
            return Err(CloudError::TooLarge(w.count));
        }
        if w.positions.len() != w.count * 3 || w.metadata.len() != w.count {
            return Err(CloudError::Shape {
                count: w.count,
                positions: w.positions.len(),
                metadata: w.metadata.len(),
            });
        }
        let positions = w
            .positions
            .into_iter()
            .map(|v| if v.is_finite() { v } else { 0.0 })
            .collect();
        let metadata = w
            .metadata
            .into_iter()
            .map(|m| CloudMeta {
                id: m.id.unwrap_or_default(),
                key: m.key.unwrap_or_default(),
                namespace: m.namespace.unwrap_or_default(),
                source_type: m.source_type.unwrap_or_default(),
                updated_at: m.updated_at.filter(|v| v.is_finite()).unwrap_or(0.0),
            })
            .collect();
        Ok(CloudSnapshot {
            snapshot_id: w.snapshot_id,
            generated_at: w.generated_at.unwrap_or(0.0),
            count: w.count,
            positions,
            metadata,
            namespaces: w.namespaces,
            source_types: w.source_types,
        })
    }

    pub fn position(&self, row: usize) -> Option<[f32; 3]> {
        if row >= self.count {
            return None;
        }
        let o = row * 3;
        Some([
            self.positions[o],
            self.positions[o + 1],
            self.positions[o + 2],
        ])
    }
}

// ── colour ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourMode {
    Namespace,
    SourceType,
    Age,
}

impl ColourMode {
    pub fn from_i32(v: i32) -> ColourMode {
        match v {
            1 => ColourMode::SourceType,
            2 => ColourMode::Age,
            _ => ColourMode::Namespace,
        }
    }
    pub fn as_i32(self) -> i32 {
        match self {
            ColourMode::Namespace => 0,
            ColourMode::SourceType => 1,
            ColourMode::Age => 2,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ColourMode::Namespace => "Namespace",
            ColourMode::SourceType => "Source",
            ColourMode::Age => "Age",
        }
    }
    pub fn next(self) -> ColourMode {
        match self {
            ColourMode::Namespace => ColourMode::SourceType,
            ColourMode::SourceType => ColourMode::Age,
            ColourMode::Age => ColourMode::Namespace,
        }
    }
}

pub fn hex_rgb(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 255) as f32 / 255.0,
        ((hex >> 8) & 255) as f32 / 255.0,
        (hex & 255) as f32 / 255.0,
    ]
}

/// Rank of each row's `updatedAt`, 0 = oldest, 1 = newest; ties share the
/// lower rank (`ageRanks`).
pub fn age_ranks(metadata: &[CloudMeta]) -> Vec<f32> {
    let n = metadata.len();
    if n <= 1 {
        return vec![1.0; n];
    }
    let mut order: Vec<(f64, usize)> = metadata
        .iter()
        .enumerate()
        .map(|(i, m)| (m.updated_at, i))
        .collect();
    order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = vec![0.0f32; n];
    let mut r = 0usize;
    for k in 0..n {
        if k > 0 && order[k].0 != order[k - 1].0 {
            r = k;
        }
        out[order[k].1] = r as f32 / (n - 1) as f32;
    }
    out
}

pub fn age_colour(u: f32) -> [f32; 3] {
    for s in 1..AGE_STOPS.len() {
        let (u1, c1) = AGE_STOPS[s];
        let (u0, c0) = AGE_STOPS[s - 1];
        if u <= u1 {
            let span = u1 - u0;
            let f = (u - u0) / if span != 0.0 { span } else { 1.0 };
            let (a, b) = (hex_rgb(c0), hex_rgb(c1));
            return [
                a[0] + (b[0] - a[0]) * f,
                a[1] + (b[1] - a[1]) * f,
                a[2] + (b[2] - a[2]) * f,
            ];
        }
    }
    hex_rgb(AGE_STOPS[AGE_STOPS.len() - 1].1)
}

/// Base RGB per row (`buildCloudColours`): categories index the server's
/// sorted `namespaces` / `sourceTypes` list; an unlisted category takes slot 0.
pub fn build_colours(snap: &CloudSnapshot, mode: ColourMode) -> Vec<[f32; 3]> {
    if mode == ColourMode::Age {
        return age_ranks(&snap.metadata)
            .into_iter()
            .map(age_colour)
            .collect();
    }
    let cats = if mode == ColourMode::Namespace {
        &snap.namespaces
    } else {
        &snap.source_types
    };
    let index: HashMap<&str, usize> = cats
        .iter()
        .enumerate()
        .map(|(i, c)| (c.as_str(), i))
        .collect();
    snap.metadata
        .iter()
        .map(|m| {
            let cat = if mode == ColourMode::Namespace {
                &m.namespace
            } else {
                &m.source_type
            };
            let slot = index.get(cat.as_str()).copied().unwrap_or(0);
            hex_rgb(CLOUD_PALETTE[slot % CLOUD_PALETTE.len()])
        })
        .collect()
}

// ── level of detail ──

/// Rows to draw when the sample exceeds `cap`: every pinned row first (route
/// and sidecar hits must stay visible), then the remaining budget split across
/// namespaces in proportion to their size (largest remainder, one row floor
/// per namespace while budget lasts), each namespace taking evenly spaced
/// rows. Deterministic, sorted ascending, never longer than `cap` unless the
/// pinned rows alone exceed it.
pub fn select_drawn(metadata: &[CloudMeta], cap: usize, pinned: &[usize]) -> Vec<u32> {
    let n = metadata.len();
    if n <= cap {
        return (0..n as u32).collect();
    }
    // Pinned rows (the shown route and its sidecar hits) always draw, even past
    // the cap: a route must land on visible points. At most MAX_PATH +
    // MAX_SIDECAR rows, far under frame_budget::CLOUD_MIN_SPRITES.
    let mut chosen: HashSet<usize> = HashSet::new();
    for &p in pinned {
        if p < n {
            chosen.insert(p);
        }
    }
    let budget = cap.saturating_sub(chosen.len());
    // namespace → rows not already pinned, in row order; namespaces in first-seen order
    let mut order: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, m) in metadata.iter().enumerate() {
        if chosen.contains(&i) {
            continue;
        }
        let e = groups.entry(m.namespace.as_str()).or_insert_with(|| {
            order.push(m.namespace.as_str());
            Vec::new()
        });
        e.push(i);
    }
    let pool: usize = groups.values().map(|g| g.len()).sum();
    if budget > 0 && pool > 0 {
        let mut quota: Vec<(usize, usize, f64)> = order
            .iter()
            .enumerate()
            .map(|(gi, ns)| {
                let size = groups[ns].len();
                let exact = budget as f64 * size as f64 / pool as f64;
                (gi, exact.floor() as usize, exact - exact.floor())
            })
            .collect();
        // floor of one per namespace while the budget allows
        let mut used: usize = quota.iter().map(|q| q.1).sum();
        for q in quota.iter_mut() {
            if q.1 == 0 && used < budget {
                q.1 = 1;
                used += 1;
            }
        }
        // largest remainder for what is left (stable on ties by namespace order)
        let mut rem: Vec<usize> = (0..quota.len()).collect();
        rem.sort_by(|&a, &b| {
            quota[b]
                .2
                .partial_cmp(&quota[a].2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        });
        let mut k = 0;
        while used < budget && !rem.is_empty() {
            let gi = rem[k % rem.len()];
            if quota[gi].1 < groups[order[gi]].len() {
                quota[gi].1 += 1;
                used += 1;
            }
            k += 1;
            if k > rem.len() * (budget + 1) {
                break;
            }
        }
        for (gi, take, _) in quota {
            let rows = &groups[order[gi]];
            let take = take.min(rows.len());
            for j in 0..take {
                // evenly spaced, centred in each stride
                let idx = ((2 * j + 1) * rows.len()) / (2 * take);
                chosen.insert(rows[idx.min(rows.len() - 1)]);
            }
        }
    }
    let mut out: Vec<u32> = chosen.into_iter().map(|i| i as u32).collect();
    out.sort_unstable();
    out
}

/// Instance buffer for `drawn` rows: a uniform-scale basis (the sprite size;
/// the shader billboards it), the row position, RGBA and neutral emphasis
/// (`NEUTRAL_EMPHASIS`). Rows outside `keep`
/// are dimmed by `dim` (0 = untouched, 1 = black), as `applyFocusDim` does.
pub fn build_buffer(
    snap: &CloudSnapshot,
    colours: &[[f32; 3]],
    drawn: &[u32],
    sprite: f32,
    opacity: f32,
    keep: &HashSet<u32>,
    dim: f32,
) -> Vec<f32> {
    let f = 1.0 - dim.clamp(0.0, 1.0);
    let mut buf = Vec::with_capacity(drawn.len() * CLOUD_STRIDE);
    for &row in drawn {
        let r = row as usize;
        let Some(p) = snap.position(r) else { continue };
        let c = colours.get(r).copied().unwrap_or([1.0, 1.0, 1.0]);
        let k = if dim <= 0.0 || keep.contains(&row) {
            1.0
        } else {
            f
        };
        buf.extend_from_slice(&[
            sprite,
            0.0,
            0.0,
            p[0],
            0.0,
            sprite,
            0.0,
            p[1],
            0.0,
            0.0,
            sprite,
            p[2],
            c[0] * k,
            c[1] * k,
            c[2] * k,
            opacity,
        ]);
        buf.extend_from_slice(&NEUTRAL_EMPHASIS);
    }
    buf
}

// ── memory_flash targeting ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashMatch {
    Key,
    Namespace,
    None,
}

impl FlashMatch {
    pub fn label(self) -> &'static str {
        match self {
            FlashMatch::Key => "key",
            FlashMatch::Namespace => "namespace",
            FlashMatch::None => "none",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct IndexMaps {
    /// key → rows, and `namespace:key` → rows
    pub by_key: HashMap<String, Vec<u32>>,
    pub by_namespace: HashMap<String, Vec<u32>>,
}

impl IndexMaps {
    pub fn build(metadata: &[CloudMeta]) -> IndexMaps {
        let mut m = IndexMaps::default();
        for (i, row) in metadata.iter().enumerate() {
            let i = i as u32;
            if !row.key.is_empty() {
                m.by_key.entry(row.key.clone()).or_default().push(i);
                if !row.namespace.is_empty() {
                    m.by_key
                        .entry(format!("{}:{}", row.namespace, row.key))
                        .or_default()
                        .push(i);
                }
            }
            if !row.namespace.is_empty() {
                m.by_namespace
                    .entry(row.namespace.clone())
                    .or_default()
                    .push(i);
            }
        }
        m
    }
}

/// xorshift64* — deterministic stand-in for `Math.random` in namespace picks.
fn next_rand(state: &mut u64) -> f64 {
    let mut x = *state;
    if x == 0 {
        x = 0x9E37_79B9_7F4A_7C15;
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
}

/// Rows a `memory_flash` lands on (`resolveFlashTargets`): the exact entry
/// (narrowed to its namespace when the key repeats), else up to three rows of
/// its namespace, else nothing — never a random point.
pub fn resolve_flash(
    key: &str,
    namespace: &str,
    maps: &IndexMaps,
    seed: u64,
) -> (Vec<u32>, FlashMatch) {
    if !key.is_empty() {
        let qualified = if namespace.is_empty() {
            None
        } else {
            maps.by_key.get(&format!("{namespace}:{key}"))
        };
        if let Some(hit) = qualified.or_else(|| maps.by_key.get(key)) {
            if !hit.is_empty() {
                return (hit.clone(), FlashMatch::Key);
            }
        }
    }
    if let Some(ns) = (!namespace.is_empty())
        .then(|| maps.by_namespace.get(namespace))
        .flatten()
    {
        if !ns.is_empty() {
            let picks = NAMESPACE_PICKS.min(ns.len());
            let mut state = seed;
            let mut chosen: Vec<u32> = Vec::with_capacity(picks);
            let mut guard = 0;
            while chosen.len() < picks && guard < picks * 8 {
                guard += 1;
                let idx = ((next_rand(&mut state) * ns.len() as f64) as usize).min(ns.len() - 1);
                if !chosen.contains(&ns[idx]) {
                    chosen.push(ns[idx]);
                }
            }
            return (chosen, FlashMatch::Namespace);
        }
    }
    (Vec::new(), FlashMatch::None)
}

// ── ray pick ──

/// The drawn row nearest the ray in angle (perpendicular distance over range),
/// within `max_angle` radians and in front of the origin. Coordinates are
/// cloud-local; `dir` need not be normalised.
pub fn pick_ray(
    snap: &CloudSnapshot,
    drawn: &[u32],
    origin: [f32; 3],
    dir: [f32; 3],
    max_angle: f32,
) -> Option<u32> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len.is_nan() || len <= 1e-9 {
        return None;
    }
    let d = [dir[0] / len, dir[1] / len, dir[2] / len];
    let tan_max = max_angle.tan();
    let mut best: Option<(u32, f32)> = None;
    for &row in drawn {
        let Some(p) = snap.position(row as usize) else {
            continue;
        };
        let v = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
        let t = v[0] * d[0] + v[1] * d[1] + v[2] * d[2];
        if t <= 1e-4 {
            continue;
        }
        let perp2 = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] - t * t).max(0.0);
        let ratio = perp2.sqrt() / t;
        if ratio <= tan_max && best.is_none_or(|(_, b)| ratio < b) {
            best = Some((row, ratio));
        }
    }
    best.map(|(r, _)| r)
}

// ── load policy ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    Ready,
    /// 401/403: the cloud needs power-user access — hide quietly.
    Forbidden,
    /// 503: building or disabled — retry after a delay.
    Unavailable,
    /// 409: the snapshot was rebuilt mid-request — reload once at once.
    Stale,
    /// Transport failure or any other status.
    Failed,
}

pub fn classify_status(transport_ok: bool, code: i64) -> LoadOutcome {
    if !transport_ok {
        return LoadOutcome::Failed;
    }
    match code {
        200..=299 => LoadOutcome::Ready,
        401 | 403 => LoadOutcome::Forbidden,
        409 => LoadOutcome::Stale,
        503 => LoadOutcome::Unavailable,
        _ => LoadOutcome::Failed,
    }
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse an IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) to Unix ms.
fn parse_http_date(v: &str) -> Option<i64> {
    let rest = v.split_once(',').map(|(_, r)| r).unwrap_or(v).trim();
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let day: i64 = parts[0].parse().ok()?;
    let month = MONTHS
        .iter()
        .position(|m| parts[1].eq_ignore_ascii_case(m))? as i64
        + 1;
    let year: i64 = parts[2].parse().ok()?;
    let hms: Vec<i64> = parts[3]
        .split(':')
        .map(|s| s.parse().ok())
        .collect::<Option<Vec<_>>>()?;
    if hms.len() != 3 || !(1..=31).contains(&day) || hms[0] > 23 || hms[1] > 59 || hms[2] > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(((days * 86_400) + hms[0] * 3600 + hms[1] * 60 + hms[2]) * 1000)
}

/// `Retry-After` in ms (`parseRetryAfter`): delta-seconds or an HTTP-date
/// relative to `now_ms`. `None` when absent or malformed; a past date is 0.
pub fn parse_retry_after(value: &str, now_ms: i64) -> Option<i64> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if v.bytes().all(|b| b.is_ascii_digit()) {
        return v.parse::<i64>().ok().map(|s| s.saturating_mul(1000));
    }
    if !v.bytes().any(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    parse_http_date(v).map(|at| (at - now_ms).max(0))
}

/// Delay before reloading after a 503 and the next streak: the server's
/// Retry-After when given, else `RETRY_DEFAULT_MS · 2^streak` (the streak only
/// grows on 503s without Retry-After), clamped to [1 s, 60 s].
pub fn unavailable_delay(retry_after_ms: Option<i64>, streak: u32) -> (i64, u32) {
    let fallback = RETRY_DEFAULT_MS.saturating_mul(1i64 << streak.min(20));
    let next = if retry_after_ms.is_none() {
        streak.saturating_add(1)
    } else {
        streak
    };
    (
        retry_after_ms
            .unwrap_or(fallback)
            .clamp(RETRY_MIN_MS, RETRY_MAX_MS),
        next,
    )
}

// ── state ──

/// Snapshot + derived views, independent of Godot.
#[derive(Debug, Default)]
pub struct CloudState {
    pub snapshot: Option<CloudSnapshot>,
    pub maps: IndexMaps,
    pub colours: Vec<[f32; 3]>,
    pub mode: Option<ColourMode>,
    pub drawn: Vec<u32>,
    pub cap: usize,
    pub keep: HashSet<u32>,
    pub last_error: String,
}

impl CloudState {
    pub fn new() -> CloudState {
        CloudState {
            cap: DEFAULT_SPRITE_CAP,
            ..Default::default()
        }
    }

    pub fn mode(&self) -> ColourMode {
        self.mode.unwrap_or(ColourMode::Namespace)
    }

    /// Replace the snapshot. On a parse error the previous snapshot is kept.
    pub fn load(&mut self, bytes: &[u8]) -> Result<usize, CloudError> {
        match CloudSnapshot::parse(bytes) {
            Ok(s) => {
                self.maps = IndexMaps::build(&s.metadata);
                self.colours = build_colours(&s, self.mode());
                self.keep.clear();
                let n = s.count;
                self.snapshot = Some(s);
                self.refresh_drawn();
                self.last_error.clear();
                Ok(n)
            }
            Err(e) => {
                self.last_error = e.to_string();
                Err(e)
            }
        }
    }

    pub fn clear(&mut self) {
        let cap = self.cap;
        let mode = self.mode;
        *self = CloudState::new();
        self.cap = cap;
        self.mode = mode;
    }

    pub fn set_mode(&mut self, mode: ColourMode) {
        self.mode = Some(mode);
        if let Some(s) = &self.snapshot {
            self.colours = build_colours(s, mode);
        }
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        self.refresh_drawn();
    }

    /// Rows held lit (and always drawn) while a route is shown.
    pub fn set_keep(&mut self, rows: &[u32]) {
        self.keep = rows.iter().copied().collect();
        self.refresh_drawn();
    }

    fn refresh_drawn(&mut self) {
        self.drawn = match &self.snapshot {
            Some(s) => {
                let mut pinned: Vec<usize> = self.keep.iter().map(|&r| r as usize).collect();
                pinned.sort_unstable();
                select_drawn(&s.metadata, self.cap, &pinned)
            }
            None => Vec::new(),
        };
    }

    pub fn buffer(&self, sprite: f32, opacity: f32, dim: f32) -> Vec<f32> {
        match &self.snapshot {
            Some(s) => build_buffer(
                s,
                &self.colours,
                &self.drawn,
                sprite,
                opacity,
                &self.keep,
                dim,
            ),
            None => Vec::new(),
        }
    }

    /// MultiMesh instance index of snapshot `row`, or -1 when it is not drawn
    /// (`drawn` is sorted ascending).
    pub fn instance_of_row(&self, row: i64) -> i64 {
        if row < 0 || row > u32::MAX as i64 {
            return -1;
        }
        self.drawn
            .binary_search(&(row as u32))
            .map_or(-1, |i| i as i64)
    }

    pub fn triangle_estimate(&self) -> usize {
        self.drawn.len() * TRIANGLES_PER_SPRITE
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct MemoryCloud {
    state: CloudState,
    last_match: FlashMatch,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl MemoryCloud {
    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            state: CloudState::new(),
            last_match: FlashMatch::None,
            base,
        })
    }

    /// Parse a `GET /api/memory-cloud` body. Returns the row count, or -1 with
    /// `last_error()` set (the previous snapshot stays loaded).
    #[func]
    fn load_bytes(&mut self, body: PackedByteArray) -> i64 {
        match self.state.load(body.as_slice()) {
            Ok(n) => n as i64,
            Err(_) => -1,
        }
    }

    #[func]
    fn clear(&mut self) {
        self.state.clear();
    }

    #[func]
    fn last_error(&self) -> GString {
        GString::from(self.state.last_error.as_str())
    }

    #[func]
    fn snapshot_id(&self) -> GString {
        GString::from(
            self.state
                .snapshot
                .as_ref()
                .map(|s| s.snapshot_id.as_str())
                .unwrap_or(""),
        )
    }

    #[func]
    fn count(&self) -> i64 {
        self.state.snapshot.as_ref().map_or(0, |s| s.count as i64)
    }

    /// MultiMesh instance of snapshot `row`, -1 when not drawn.
    #[func]
    fn instance_of_row(&self, row: i64) -> i64 {
        self.state.instance_of_row(row)
    }

    #[func]
    fn drawn_count(&self) -> i64 {
        self.state.drawn.len() as i64
    }

    #[func]
    fn triangle_estimate(&self) -> i64 {
        self.state.triangle_estimate() as i64
    }

    #[func]
    fn set_sprite_cap(&mut self, cap: i64) {
        self.state.set_cap(cap.max(1) as usize);
    }

    /// 0 namespace, 1 source type, 2 age.
    #[func]
    fn set_colour_mode(&mut self, mode: i32) {
        self.state.set_mode(ColourMode::from_i32(mode));
    }

    #[func]
    fn colour_mode(&self) -> i32 {
        self.state.mode().as_i32()
    }

    #[func]
    fn next_colour_mode(&self) -> i32 {
        self.state.mode().next().as_i32()
    }

    #[func]
    fn colour_mode_label(&self) -> GString {
        GString::from(self.state.mode().label())
    }

    #[func]
    fn set_keep(&mut self, rows: PackedInt32Array) {
        let rows: Vec<u32> = rows
            .as_slice()
            .iter()
            .filter(|&&r| r >= 0)
            .map(|&r| r as u32)
            .collect();
        self.state.set_keep(&rows);
    }

    /// Stride-16 instance buffer for the drawn rows.
    #[func]
    fn build_buffer(&self, sprite_size: f32, opacity: f32, dim: f32) -> PackedFloat32Array {
        PackedFloat32Array::from(self.state.buffer(sprite_size, opacity, dim).as_slice())
    }

    #[func]
    fn sprite_local_size(&self) -> f32 {
        sprite_local_size(DESKTOP_POINT_SIZE, DESKTOP_FOV_DEG, DESKTOP_CLOUD_SCALE)
    }

    /// UVs of the one-triangle sprite (`SPRITE_TRIANGLE_UV`); the mesh
    /// vertex is `Vector3(u - 0.5, 0.5 - v, 0)`.
    #[func]
    fn sprite_triangle_uv(&self) -> PackedVector2Array {
        SPRITE_TRIANGLE_UV
            .iter()
            .map(|&[u, v]| Vector2::new(u, v))
            .collect()
    }

    #[func]
    fn cloud_scale(&self) -> f32 {
        DESKTOP_CLOUD_SCALE
    }

    /// Cloud-local position of `row` (zero when out of range).
    #[func]
    fn point_local(&self, row: i64) -> Vector3 {
        let p = self
            .state
            .snapshot
            .as_ref()
            .and_then(|s| {
                if row >= 0 {
                    s.position(row as usize)
                } else {
                    None
                }
            })
            .unwrap_or([0.0; 3]);
        Vector3::new(p[0], p[1], p[2])
    }

    /// Whole position array (3 · count), for the route layer.
    #[func]
    fn positions(&self) -> PackedFloat32Array {
        PackedFloat32Array::from(
            self.state
                .snapshot
                .as_ref()
                .map(|s| s.positions.as_slice())
                .unwrap_or(&[]),
        )
    }

    #[func]
    fn row_info(&self, row: i64) -> Dictionary {
        let mut d = Dictionary::new();
        if let Some(m) = self
            .state
            .snapshot
            .as_ref()
            .and_then(|s| s.metadata.get(row.max(0) as usize))
            .filter(|_| row >= 0)
        {
            d.set("id", GString::from(m.id.as_str()));
            d.set("key", GString::from(m.key.as_str()));
            d.set("namespace", GString::from(m.namespace.as_str()));
            d.set("sourceType", GString::from(m.source_type.as_str()));
            d.set("updatedAt", m.updated_at);
        }
        d
    }

    /// Rows a memory_flash lands on; `last_flash_match()` names the rule used.
    #[func]
    fn resolve_flash(&mut self, key: GString, namespace: GString, seed: i64) -> PackedInt32Array {
        let (rows, m) = resolve_flash(
            &key.to_string(),
            &namespace.to_string(),
            &self.state.maps,
            seed as u64,
        );
        self.last_match = m;
        PackedInt32Array::from(
            rows.into_iter()
                .map(|r| r as i32)
                .collect::<Vec<i32>>()
                .as_slice(),
        )
    }

    #[func]
    fn last_flash_match(&self) -> GString {
        GString::from(self.last_match.label())
    }

    /// Hovered row under a cloud-local ray, or -1.
    #[func]
    fn pick(&self, origin: Vector3, dir: Vector3, max_angle: f32) -> i64 {
        let Some(s) = self.state.snapshot.as_ref() else {
            return -1;
        };
        pick_ray(
            s,
            &self.state.drawn,
            [origin.x, origin.y, origin.z],
            [dir.x, dir.y, dir.z],
            max_angle,
        )
        .map_or(-1, |r| r as i64)
    }

    /// 0 ready, 1 forbidden, 2 unavailable, 3 stale, 4 failed.
    #[func]
    fn classify_status(&self, transport_ok: bool, code: i64) -> i32 {
        match classify_status(transport_ok, code) {
            LoadOutcome::Ready => 0,
            LoadOutcome::Forbidden => 1,
            LoadOutcome::Unavailable => 2,
            LoadOutcome::Stale => 3,
            LoadOutcome::Failed => 4,
        }
    }

    /// Retry-After in ms, or -1 when absent/malformed.
    #[func]
    fn parse_retry_after(&self, value: GString, now_ms: i64) -> i64 {
        parse_retry_after(&value.to_string(), now_ms).unwrap_or(-1)
    }

    /// `[delay_ms, next_streak]` after a 503; pass retry_after_ms = -1 when absent.
    #[func]
    fn unavailable_delay(&self, retry_after_ms: i64, streak: i64) -> PackedInt64Array {
        let ra = if retry_after_ms >= 0 {
            Some(retry_after_ms)
        } else {
            None
        };
        let (d, s) = unavailable_delay(ra, streak.max(0) as u32);
        PackedInt64Array::from(&[d, s as i64][..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_json(n: usize) -> String {
        let mut pos = Vec::new();
        let mut meta = Vec::new();
        for i in 0..n {
            pos.push(format!("{}.0,{}.5,-{}.25", i, i, i));
            let ns = if i % 3 == 0 {
                "project-state"
            } else if i % 3 == 1 {
                "patterns"
            } else {
                "dream-cycle"
            };
            meta.push(format!(
                r#"{{"id":"id{i}","key":"k{i}","namespace":"{ns}","sourceType":"{}","updatedAt":{}}}"#,
                if i % 2 == 0 { "agent" } else { "hook" },
                1_000 + (i as i64 % 4) * 10
            ));
        }
        format!(
            r#"{{"version":1,"snapshotId":"s-1","generatedAt":5,"dim":384,"count":{n},"positions":[{}],"metadata":[{}],"namespaces":["dream-cycle","patterns","project-state"],"sourceTypes":["agent","hook"],"strata":[],"excludedNamespaces":["personal-context"],"vectorsUrl":"vectors?snapshot=s-1"}}"#,
            pos.join(","),
            meta.join(",")
        )
    }

    // ── parse ──

    #[test]
    fn parses_a_well_formed_snapshot_and_ignores_unknown_fields() {
        let s = CloudSnapshot::parse(snap_json(4).as_bytes()).unwrap();
        assert_eq!(s.snapshot_id, "s-1");
        assert_eq!(s.count, 4);
        assert_eq!(s.position(2), Some([2.0, 2.5, -2.25]));
        assert_eq!(s.metadata[1].namespace, "patterns");
        assert_eq!(s.metadata[1].source_type, "hook");
        assert_eq!(s.position(4), None);
    }

    #[test]
    fn empty_snapshot_is_valid() {
        let j = r#"{"version":1,"snapshotId":"e","count":0,"positions":[],"metadata":[]}"#;
        let s = CloudSnapshot::parse(j.as_bytes()).unwrap();
        assert_eq!(s.count, 0);
        assert!(s.namespaces.is_empty());
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(
            CloudSnapshot::parse(b"{not json"),
            Err(CloudError::Json(_))
        ));
        assert!(matches!(
            CloudSnapshot::parse(b""),
            Err(CloudError::Json(_))
        ));
        assert!(matches!(
            CloudSnapshot::parse(b"[1,2,3]"),
            Err(CloudError::Json(_))
        ));
        // positions of the wrong type
        let j =
            r#"{"version":1,"snapshotId":"x","count":1,"positions":["a","b","c"],"metadata":[{}]}"#;
        assert!(matches!(
            CloudSnapshot::parse(j.as_bytes()),
            Err(CloudError::Json(_))
        ));
    }

    #[test]
    fn rejects_missing_required_fields() {
        let j = r#"{"version":1,"count":0,"positions":[],"metadata":[]}"#;
        assert!(matches!(
            CloudSnapshot::parse(j.as_bytes()),
            Err(CloudError::Json(_))
        ));
        let j = r#"{"version":1,"snapshotId":"x","count":0,"metadata":[]}"#;
        assert!(matches!(
            CloudSnapshot::parse(j.as_bytes()),
            Err(CloudError::Json(_))
        ));
    }

    #[test]
    fn rejects_short_positions_and_metadata() {
        let full = snap_json(3);
        // drop the last position float
        let short = full.replacen(",-2.25]", "]", 1);
        assert!(matches!(
            CloudSnapshot::parse(short.as_bytes()),
            Err(CloudError::Shape {
                count: 3,
                positions: 8,
                metadata: 3
            })
        ));
        let j =
            r#"{"version":1,"snapshotId":"x","count":2,"positions":[0,0,0,1,1,1],"metadata":[{}]}"#;
        assert!(matches!(
            CloudSnapshot::parse(j.as_bytes()),
            Err(CloudError::Shape {
                count: 2,
                positions: 6,
                metadata: 1
            })
        ));
    }

    #[test]
    fn rejects_wrong_version_empty_id_and_oversize() {
        let j = r#"{"version":2,"snapshotId":"x","count":0,"positions":[],"metadata":[]}"#;
        assert_eq!(
            CloudSnapshot::parse(j.as_bytes()).unwrap_err(),
            CloudError::Version(2)
        );
        let j = r#"{"version":1,"snapshotId":"  ","count":0,"positions":[],"metadata":[]}"#;
        assert_eq!(
            CloudSnapshot::parse(j.as_bytes()).unwrap_err(),
            CloudError::EmptyId
        );
        let j = format!(
            r#"{{"version":1,"snapshotId":"x","count":{},"positions":[],"metadata":[]}}"#,
            MAX_SNAPSHOT_ROWS + 1
        );
        assert_eq!(
            CloudSnapshot::parse(j.as_bytes()).unwrap_err(),
            CloudError::TooLarge(MAX_SNAPSHOT_ROWS + 1)
        );
    }

    #[test]
    fn null_metadata_fields_become_defaults() {
        let j = r#"{"version":1,"snapshotId":"x","count":1,"positions":[1,2,3],"metadata":[{"key":null,"namespace":null,"updatedAt":null}]}"#;
        let s = CloudSnapshot::parse(j.as_bytes()).unwrap();
        assert_eq!(s.metadata[0].key, "");
        assert_eq!(s.metadata[0].updated_at, 0.0);
    }

    #[test]
    fn failed_load_keeps_previous_snapshot() {
        let mut st = CloudState::new();
        assert_eq!(st.load(snap_json(5).as_bytes()), Ok(5));
        assert!(st.load(b"garbage").is_err());
        assert_eq!(st.snapshot.as_ref().unwrap().count, 5);
        assert!(!st.last_error.is_empty());
    }

    // ── palette drift against the desktop source ──

    fn ts_source(rel: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// Every `0x……` literal between `start` and the next `end` after it.
    fn hex_literals(src: &str, start: &str, end: &str) -> Vec<u32> {
        let from = src
            .find(start)
            .unwrap_or_else(|| panic!("{start} not found"));
        let body = &src[from..];
        let body = &body[..body.find(end).expect("block end")];
        let mut out = Vec::new();
        let b = body.as_bytes();
        let mut i = 0;
        while i + 1 < b.len() {
            if b[i] == b'0' && b[i + 1] == b'x' {
                let mut j = i + 2;
                while j < b.len() && b[j].is_ascii_hexdigit() {
                    j += 1;
                }
                out.push(u32::from_str_radix(&body[i + 2..j], 16).unwrap());
                i = j;
            } else {
                i += 1;
            }
        }
        out
    }

    #[test]
    fn cloud_palette_matches_cloud_data_ts() {
        let src = ts_source("client/src/features/visualisation/memoryCloud/cloudData.ts");
        let ts = hex_literals(&src, "export const CLOUD_PALETTE", "] as const");
        assert_eq!(
            ts,
            CLOUD_PALETTE.to_vec(),
            "CLOUD_PALETTE drifted from cloudData.ts"
        );
    }

    #[test]
    fn age_stops_match_cloud_data_ts() {
        let src = ts_source("client/src/features/visualisation/memoryCloud/cloudData.ts");
        let ts = hex_literals(&src, "const AGE_STOPS", "];");
        let ours: Vec<u32> = AGE_STOPS.iter().map(|s| s.1).collect();
        assert_eq!(ts, ours, "AGE_STOPS colours drifted from cloudData.ts");
        // stop positions: `[0, hexRgb(`, `[0.6, hexRgb(`, `[1, hexRgb(`
        let from = src.find("const AGE_STOPS").unwrap();
        let block = &src[from..from + src[from..].find("];").unwrap()];
        let stops: Vec<f32> = block
            .split("hexRgb(")
            .filter_map(|s| s.rsplit('[').next())
            .filter_map(|s| s.trim().trim_end_matches(',').trim().parse::<f32>().ok())
            .collect();
        let ours: Vec<f32> = AGE_STOPS.iter().map(|s| s.0).collect();
        assert_eq!(stops, ours, "AGE_STOPS positions drifted from cloudData.ts");
    }

    #[test]
    fn desktop_defaults_match_embedding_cloud_layer() {
        let src = ts_source("client/src/features/visualisation/components/EmbeddingCloudLayer.tsx");
        for (needle, v) in [
            ("settings?.pointSize ?? ", DESKTOP_POINT_SIZE),
            ("settings?.opacity ?? ", DESKTOP_OPACITY),
            ("settings?.cloudScale ?? ", DESKTOP_CLOUD_SCALE),
            ("settings?.dimOffRoute ?? ", DESKTOP_DIM_OFF_ROUTE),
            ("settings?.rotationSpeed ?? ", DESKTOP_ROTATION_PER_FRAME),
        ] {
            let at = src
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} missing"))
                + needle.len();
            let lit: String = src[at..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            assert_eq!(lit.parse::<f32>().unwrap(), v, "{needle}");
        }
        let src = ts_source("client/src/features/visualisation/memoryCloud/cloudData.ts");
        assert!(src.contains("const NAMESPACE_PICKS = 3;"));
        let store = ts_source("client/src/features/visualisation/memoryCloud/memoryCloudStore.ts");
        assert!(store.contains("RETRY_DEFAULT_MS = 5000;"));
        assert!(store.contains("RETRY_MIN_MS = 1000;"));
        assert!(store.contains("RETRY_MAX_MS = 60_000;"));
    }

    // ── colour ──

    #[test]
    fn namespace_colours_index_the_sorted_server_list() {
        let s = CloudSnapshot::parse(snap_json(3).as_bytes()).unwrap();
        let c = build_colours(&s, ColourMode::Namespace);
        // row 0 project-state → slot 2, row 1 patterns → slot 1, row 2 dream-cycle → slot 0
        assert_eq!(c[0], hex_rgb(CLOUD_PALETTE[2]));
        assert_eq!(c[1], hex_rgb(CLOUD_PALETTE[1]));
        assert_eq!(c[2], hex_rgb(CLOUD_PALETTE[0]));
        let c = build_colours(&s, ColourMode::SourceType);
        assert_eq!(c[0], hex_rgb(CLOUD_PALETTE[0]));
        assert_eq!(c[1], hex_rgb(CLOUD_PALETTE[1]));
    }

    #[test]
    fn unlisted_category_takes_slot_zero_and_palette_cycles() {
        let mut s = CloudSnapshot::parse(snap_json(1).as_bytes()).unwrap();
        s.metadata[0].namespace = "unknown".into();
        assert_eq!(
            build_colours(&s, ColourMode::Namespace)[0],
            hex_rgb(CLOUD_PALETTE[0])
        );
        s.namespaces = (0..18).map(|i| format!("n{i:02}")).collect();
        s.metadata[0].namespace = "n17".into();
        assert_eq!(
            build_colours(&s, ColourMode::Namespace)[0],
            hex_rgb(CLOUD_PALETTE[1])
        );
    }

    #[test]
    fn age_ranks_share_ties_and_span_zero_to_one() {
        let m = |t: f64| CloudMeta {
            id: String::new(),
            key: String::new(),
            namespace: String::new(),
            source_type: String::new(),
            updated_at: t,
        };
        let r = age_ranks(&[m(30.0), m(10.0), m(10.0), m(20.0)]);
        assert_eq!(r, vec![1.0, 0.0, 0.0, 2.0 / 3.0]);
        assert_eq!(age_ranks(&[m(5.0)]), vec![1.0]);
        assert!(age_ranks(&[]).is_empty());
    }

    #[test]
    fn age_colour_hits_its_stops() {
        assert_eq!(age_colour(0.0), hex_rgb(0x1d2a52));
        assert_eq!(age_colour(0.6), hex_rgb(0x6f9bff));
        assert_eq!(age_colour(1.0), hex_rgb(0x7ef0cf));
        let mid = age_colour(0.3);
        let (a, b) = (hex_rgb(0x1d2a52), hex_rgb(0x6f9bff));
        assert!((mid[2] - (a[2] + b[2]) / 2.0).abs() < 1e-5);
    }

    #[test]
    fn sprite_size_matches_desktop_point_maths() {
        let s = sprite_local_size(7.5, 75.0, 5.0);
        assert!((s - 7.5 * (37.5f32.to_radians()).tan() / 5.0).abs() < 1e-6);
        assert!((s - 1.1509).abs() < 1e-3);
    }

    // ── LOD ──

    fn meta_ns(counts: &[(&str, usize)]) -> Vec<CloudMeta> {
        let mut v = Vec::new();
        for (ns, n) in counts {
            for i in 0..*n {
                v.push(CloudMeta {
                    id: String::new(),
                    key: format!("{ns}{i}"),
                    namespace: ns.to_string(),
                    source_type: String::new(),
                    updated_at: 0.0,
                });
            }
        }
        v
    }

    #[test]
    fn lod_keeps_everything_under_the_cap() {
        let m = meta_ns(&[("a", 10)]);
        assert_eq!(select_drawn(&m, 10, &[]), (0..10).collect::<Vec<u32>>());
    }

    #[test]
    fn lod_respects_cap_pins_and_represents_every_namespace() {
        let m = meta_ns(&[("big", 18_000), ("mid", 1_900), ("tiny", 3)]);
        let pinned = [19_900usize, 5, 19_902];
        let d = select_drawn(&m, 12_000, &pinned);
        assert_eq!(d.len(), 12_000);
        for p in pinned {
            assert!(
                d.binary_search(&(p as u32)).is_ok(),
                "pinned row {p} dropped"
            );
        }
        let count = |ns: &str| d.iter().filter(|&&r| m[r as usize].namespace == ns).count();
        assert!(count("tiny") >= 1);
        let big = count("big") as f64 / 12_000.0;
        assert!(
            (big - 18_000.0 / 19_903.0).abs() < 0.01,
            "proportional share, got {big}"
        );
        assert!(d.windows(2).all(|w| w[0] < w[1]), "sorted, unique");
        assert_eq!(select_drawn(&m, 12_000, &pinned), d, "deterministic");
    }

    #[test]
    fn lod_with_more_pins_than_cap_draws_exactly_the_pins() {
        let m = meta_ns(&[("a", 100)]);
        let pins: Vec<usize> = (0..50).collect();
        let d = select_drawn(&m, 10, &pins);
        assert_eq!(
            d,
            (0..50).collect::<Vec<u32>>(),
            "pins win over the cap, nothing else added"
        );
    }

    #[test]
    fn pinned_rows_draw_even_past_the_cap() {
        let m = meta_ns(&[("a", 20), ("b", 20), ("c", 10)]);
        let pins = [3usize, 17, 29, 44];
        let d = select_drawn(&m, 2, &pins);
        assert_eq!(d.len(), 4);
        for p in pins {
            assert!(d.contains(&(p as u32)), "{p} drawn");
        }
        const _: () = assert!(
            crate::memory_route::MAX_PATH + crate::memory_route::MAX_SIDECAR
                < crate::frame_budget::CLOUD_MIN_SPRITES
        );
    }

    #[test]
    fn twenty_thousand_rows_fit_the_triangle_budget() {
        let mut st = CloudState::new();
        st.load(snap_json(20_000).as_bytes()).unwrap();
        assert_eq!(st.drawn.len(), DEFAULT_SPRITE_CAP);
        assert_eq!(st.triangle_estimate(), 8_000);
        assert_eq!(
            st.triangle_estimate(),
            DEFAULT_SPRITE_CAP * TRIANGLES_PER_SPRITE
        );
    }

    #[test]
    fn sprite_triangle_circumscribes_the_disc() {
        // The fragment shader keeps UV distance <= 0.5 from (0.5, 0.5); every
        // edge must sit exactly 0.5 from the centre so no disc pixel is lost.
        let v = SPRITE_TRIANGLE_UV;
        let c = [0.5f32, 0.5];
        let cen = [
            (v[0][0] + v[1][0] + v[2][0]) / 3.0,
            (v[0][1] + v[1][1] + v[2][1]) / 3.0,
        ];
        assert!(
            (cen[0] - c[0]).abs() < 1e-6 && (cen[1] - c[1]).abs() < 1e-6,
            "centred"
        );
        for i in 0..3 {
            let (a, b) = (v[i], v[(i + 1) % 3]);
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let len = (ex * ex + ey * ey).sqrt();
            let dist = ((c[0] - a[0]) * ey - (c[1] - a[1]) * ex).abs() / len;
            assert!(
                (dist - 0.5).abs() < 1e-5,
                "edge {i} sits {dist} from the centre"
            );
            let r = ((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2)).sqrt();
            assert!((r - 1.0).abs() < 1e-5, "vertex {i} at circumradius {r}");
        }
    }

    // ── buffer ──

    #[test]
    fn buffer_is_stride_20_with_origin_colour_and_neutral_emphasis() {
        assert_eq!(
            CLOUD_STRIDE, 20,
            "12 transform + 4 colour + 4 custom (use_custom_data)"
        );
        let mut st = CloudState::new();
        st.load(snap_json(3).as_bytes()).unwrap();
        let b = st.buffer(1.5, 0.6, 0.0);
        assert_eq!(b.len(), 3 * CLOUD_STRIDE);
        let r = &b[CLOUD_STRIDE..2 * CLOUD_STRIDE];
        assert_eq!([r[0], r[5], r[10]], [1.5, 1.5, 1.5]);
        assert_eq!([r[3], r[7], r[11]], [1.0, 1.5, -1.25]);
        let c = hex_rgb(CLOUD_PALETTE[1]);
        assert_eq!(&r[12..16], &[c[0], c[1], c[2], 0.6]);
        assert_eq!(
            &r[16..20],
            &NEUTRAL_EMPHASIS,
            "gain 1: the shader leaves the sprite as is"
        );
    }

    #[test]
    fn instance_of_row_maps_drawn_rows_and_rejects_the_rest() {
        let mut st = CloudState::new();
        st.load(snap_json(20).as_bytes()).unwrap();
        st.set_cap(5);
        let drawn = st.drawn.clone();
        assert_eq!(drawn.len(), 5);
        for (i, &row) in drawn.iter().enumerate() {
            assert_eq!(st.instance_of_row(row as i64), i as i64);
        }
        let undrawn = (0..20u32).find(|r| !drawn.contains(r)).unwrap();
        assert_eq!(st.instance_of_row(undrawn as i64), -1);
        assert_eq!(st.instance_of_row(-3), -1);
        assert_eq!(st.instance_of_row(99), -1);
    }

    #[test]
    fn focus_dim_darkens_rows_off_the_route_only() {
        let mut st = CloudState::new();
        st.load(snap_json(3).as_bytes()).unwrap();
        st.set_keep(&[1]);
        let b = st.buffer(1.0, 0.6, 0.75);
        let c0 = hex_rgb(CLOUD_PALETTE[2]);
        assert!((b[12] - c0[0] * 0.25).abs() < 1e-6, "row 0 dimmed");
        let c1 = hex_rgb(CLOUD_PALETTE[1]);
        assert_eq!(b[CLOUD_STRIDE + 12], c1[0], "kept row untouched");
    }

    // ── flash ──

    #[test]
    fn flash_prefers_namespaced_key_then_bare_key() {
        let mut m = meta_ns(&[("a", 2), ("b", 2)]);
        m[3].key = "a0".into(); // same key in namespace b
        let maps = IndexMaps::build(&m);
        assert_eq!(
            resolve_flash("a0", "b", &maps, 1),
            (vec![3], FlashMatch::Key)
        );
        assert_eq!(
            resolve_flash("a0", "", &maps, 1),
            (vec![0, 3], FlashMatch::Key)
        );
        assert_eq!(
            resolve_flash("a1", "zzz", &maps, 1),
            (vec![1], FlashMatch::Key)
        );
    }

    #[test]
    fn flash_falls_back_to_at_most_three_namespace_rows() {
        let m = meta_ns(&[("a", 10), ("b", 2)]);
        let maps = IndexMaps::build(&m);
        let (rows, kind) = resolve_flash("missing", "a", &maps, 42);
        assert_eq!(kind, FlashMatch::Namespace);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|&r| r < 10));
        assert_eq!(
            resolve_flash("missing", "a", &maps, 42).0,
            rows,
            "deterministic for a seed"
        );
        let (rows, _) = resolve_flash("missing", "b", &maps, 7);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn flash_with_unknown_namespace_lands_nowhere() {
        let maps = IndexMaps::build(&meta_ns(&[("a", 3)]));
        assert_eq!(
            resolve_flash("x", "nope", &maps, 1),
            (vec![], FlashMatch::None)
        );
        assert_eq!(resolve_flash("", "", &maps, 1), (vec![], FlashMatch::None));
    }

    // ── pick ──

    #[test]
    fn pick_takes_the_smallest_angle_in_front() {
        let j = r#"{"version":1,"snapshotId":"p","count":3,"positions":[0,0,-10, 0.5,0,-5, 0,0,10],"metadata":[{},{},{}]}"#;
        let s = CloudSnapshot::parse(j.as_bytes()).unwrap();
        let drawn = [0u32, 1, 2];
        assert_eq!(
            pick_ray(&s, &drawn, [0.0; 3], [0.0, 0.0, -1.0], 0.2),
            Some(0)
        );
        assert_eq!(
            pick_ray(&s, &drawn, [0.0; 3], [0.1, 0.0, -1.0], 0.2),
            Some(1)
        );
        assert_eq!(pick_ray(&s, &drawn, [0.0; 3], [1.0, 0.0, 0.0], 0.05), None);
        assert_eq!(pick_ray(&s, &drawn, [0.0; 3], [0.0, 0.0, 0.0], 0.2), None);
        assert_eq!(
            pick_ray(&s, &[2], [0.0; 3], [0.0, 0.0, -1.0], 0.2),
            None,
            "behind"
        );
    }

    // ── load policy ──

    #[test]
    fn statuses_map_to_outcomes() {
        assert_eq!(classify_status(true, 200), LoadOutcome::Ready);
        assert_eq!(classify_status(true, 401), LoadOutcome::Forbidden);
        assert_eq!(classify_status(true, 403), LoadOutcome::Forbidden);
        assert_eq!(classify_status(true, 409), LoadOutcome::Stale);
        assert_eq!(classify_status(true, 503), LoadOutcome::Unavailable);
        assert_eq!(classify_status(true, 500), LoadOutcome::Failed);
        assert_eq!(classify_status(false, 200), LoadOutcome::Failed);
    }

    #[test]
    fn retry_after_parses_seconds_and_http_dates() {
        assert_eq!(parse_retry_after("12", 0), Some(12_000));
        assert_eq!(parse_retry_after(" 0 ", 0), Some(0));
        assert_eq!(parse_retry_after("", 0), None);
        assert_eq!(parse_retry_after("-5", 0), None);
        assert_eq!(parse_retry_after("1.5", 0), None);
        // RFC 9110 example date
        let at = 784_111_777_000i64;
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT", at - 3_000),
            Some(3_000)
        );
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT", at + 9_000),
            Some(0),
            "past date"
        );
        assert_eq!(parse_retry_after("Sun, 06 Foo 1994 08:49:37 GMT", 0), None);
        assert_eq!(parse_retry_after("soon", 0), None);
    }

    #[test]
    fn unavailable_backoff_doubles_without_retry_after_and_clamps() {
        assert_eq!(unavailable_delay(None, 0), (5_000, 1));
        assert_eq!(unavailable_delay(None, 1), (10_000, 2));
        assert_eq!(unavailable_delay(None, 3), (40_000, 4));
        assert_eq!(unavailable_delay(None, 4), (60_000, 5));
        assert_eq!(unavailable_delay(None, 60), (60_000, 61));
        assert_eq!(
            unavailable_delay(Some(2_000), 3),
            (2_000, 3),
            "server value wins, streak held"
        );
        assert_eq!(unavailable_delay(Some(10), 0), (1_000, 0));
        assert_eq!(unavailable_delay(Some(900_000), 0), (60_000, 0));
    }

    #[test]
    fn colour_mode_cycles() {
        assert_eq!(ColourMode::Namespace.next(), ColourMode::SourceType);
        assert_eq!(ColourMode::Age.next(), ColourMode::Namespace);
        assert_eq!(ColourMode::from_i32(9), ColourMode::Namespace);
    }
}
