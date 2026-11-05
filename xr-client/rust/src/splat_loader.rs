//! Loader for Gaussian splat scenes (ADR-2170 decisions 6 and 10).
//!
//! The server lists what it serves at `GET /api/splats` (a
//! [`visionclaw_splat::SplatListing`]) and serves each variant by content address
//! at `/api/splats/blob/<sha256-12>`. This module is everything the headset does
//! with that, apart from the HTTP request itself, which GDScript makes through
//! the backend connection (`BackendConnection.make_http`, signed for the exact
//! URL) so the transport, the auth and the cancellation stay in one place:
//!
//! 1. [`plan`]: parse the listing and pick the headset variant for a tier.
//!    `xr-1`, `xr-2` and `xr-3` are tiers 1 to 3; a tier with no variant of its
//!    own falls back to the largest smaller one, never a larger one.
//! 2. [`read_cache`]: `user://splat-cache/<address>.spz`, **re-verified** against
//!    the listing's full SHA-256 on every hit. A cache file that fails is deleted
//!    and treated as a miss.
//! 3. [`verify`]: a download whose size or SHA-256 differs from the listing is a
//!    **refusal** ([`LoadError::is_refusal`]), not a retry: nothing is written to
//!    the cache and nothing is decoded.
//! 4. [`pack`]: [`visionclaw_splat::spz::decode`] the SPZ v3 file and lay the
//!    static splat data out for the shader (see [`Packed`]), once.
//!
//! The decode and the pack run on a worker thread ([`SplatLoader`]) so a 100k
//! splat scene never costs the render thread a frame.
//!
//! # Data texture layout
//!
//! One RGBA32F texture, [`DATA_TEX_WIDTH`] texels wide: each splat is
//! [`TEXELS_PER_SPLAT`] consecutive texels, [`DATA_ROW_SPLATS`] splats a row, so
//! splat `i` starts at texel `((i % DATA_ROW_SPLATS) * 4, i / DATA_ROW_SPLATS)`.
//! `materials/splat.gdshader` repeats these constants and a GUT test pins them.
//!
//! | texel | x | y | z | w |
//! |-------|---|---|---|---|
//! | 0 | position x | position y | position z | opacity (0..1) |
//! | 1 | red | green | blue | 0 |
//! | 2 | cov xx | cov xy | cov xz | cov yy |
//! | 3 | cov yz | cov zz | 0 | 0 |
//!
//! `cov` is the 3D covariance `R S S^T R^T` computed here from the decoded scale
//! and rotation, so the vertex shader never builds a rotation. Colours are the
//! decoder's 0..1 base colour (SH degree 0: the headset variants carry no
//! view-dependent colour).

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use visionclaw_splat::address::{is_content_address, is_sha256_hex, sha256_hex};
use visionclaw_splat::manifest::{ListedAsset, ListedVariant, SplatListing, Stage};
use visionclaw_splat::spz::{self, Splats};

/// Texels a splat occupies in the data texture.
pub const TEXELS_PER_SPLAT: usize = 4;
/// Splats in one row of the data texture.
pub const DATA_ROW_SPLATS: usize = 1024;
/// Width of the data texture in texels.
pub const DATA_TEX_WIDTH: usize = DATA_ROW_SPLATS * TEXELS_PER_SPLAT;
/// Cache sub-directory under `user://`.
pub const CACHE_DIR: &str = "splat-cache";

/// Why a splat scene did not load.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    #[error("the splat listing is not valid: {0}")]
    Listing(String),
    #[error("the server lists no splat asset{}", .0.as_ref().map(|a| format!(" {a}")).unwrap_or_default())]
    NoAsset(Option<String>),
    #[error("asset {asset} has no headset variant at or below tier {tier}")]
    NoVariant { asset: String, tier: usize },
    #[error("refused: variant address {0:?} is not a sha256-12 content address")]
    BadAddress(String),
    #[error("refused: the listing's SHA-256 {0:?} is not a full lowercase hex digest")]
    BadDigest(String),
    #[error("refused: {got} bytes received, the listing says {want}")]
    Size { want: u64, got: u64 },
    #[error("refused: SHA-256 {got} does not match the listing's {want}")]
    HashMismatch { want: String, got: String },
    #[error("refused: the file has {got} splats, the listing says {want}")]
    Count { want: u32, got: usize },
    #[error("the SPZ file did not decode: {0}")]
    Decode(String),
    #[error("{0}")]
    Io(String),
}

impl LoadError {
    /// True for the errors that mean the bytes are not what the server listed.
    /// These are final: the caller shows the scene as refused and does not
    /// retry the download.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            Self::BadAddress(_)
                | Self::BadDigest(_)
                | Self::Size { .. }
                | Self::HashMismatch { .. }
                | Self::Count { .. }
        )
    }
}

/// The variant chosen for a tier, with what the HUD and the shader need.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub asset_id: String,
    pub title: String,
    /// "attribution (SPDX url), modified (decimated)": the credits line.
    pub credits: String,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    /// The asset's stage (ADR-2178), when it has one: the merged splat space
    /// is only offered for a staged asset.
    pub stage: Option<Stage>,
    /// The tier of the variant chosen (at most the tier asked for).
    pub tier: usize,
    pub variant: ListedVariant,
}

/// Parse a `GET /api/splats` body and pick the headset variant for `tier`
/// (1..=3) of `asset_id`, or of the first asset with a headset variant when
/// `asset_id` is `None` or empty.
pub fn plan(listing_json: &str, asset_id: Option<&str>, tier: usize) -> Result<Plan, LoadError> {
    let listing: SplatListing =
        serde_json::from_str(listing_json).map_err(|e| LoadError::Listing(e.to_string()))?;
    let wanted = asset_id.filter(|s| !s.is_empty());
    let asset = listing
        .assets
        .iter()
        .find(|a| match wanted {
            Some(id) => a.id == id,
            None => a.variants.iter().any(|v| v.name.is_headset()),
        })
        .ok_or_else(|| LoadError::NoAsset(wanted.map(str::to_owned)))?;
    let (variant, got_tier) = pick_variant(asset, tier).ok_or_else(|| LoadError::NoVariant {
        asset: asset.id.clone(),
        tier,
    })?;
    Ok(Plan {
        asset_id: asset.id.clone(),
        title: asset.title.clone(),
        credits: asset.credits(),
        bounds_min: asset.bounds.min,
        bounds_max: asset.bounds.max,
        stage: asset.stage,
        tier: got_tier,
        variant: variant.clone(),
    })
}

/// The headset variant for `tier`: the one named for that tier, else the largest
/// headset variant of a lower tier. `None` for tier 0 or when nothing fits.
pub fn pick_variant(asset: &ListedAsset, tier: usize) -> Option<(&ListedVariant, usize)> {
    asset
        .variants
        .iter()
        .filter_map(|v| v.name.headset_tier().map(|t| (v, t as usize)))
        .filter(|&(_, t)| t <= tier)
        .max_by_key(|&(_, t)| t)
}

/// `dir/<address>.spz`. The address comes from the server, so it is checked to
/// be a content address first: no separator or `..` can reach this join.
pub fn cache_file(dir: &Path, address: &str) -> Result<PathBuf, LoadError> {
    if !is_content_address(address) {
        return Err(LoadError::BadAddress(address.to_owned()));
    }
    Ok(dir.join(format!("{address}.spz")))
}

/// Check `bytes` against the listing: the digest must be a well-formed full
/// SHA-256, the address its 12-hex prefix, the length the listed one and the
/// SHA-256 the listed one. Any difference is a refusal (see
/// [`LoadError::is_refusal`]).
pub fn verify(bytes: &[u8], v: &ListedVariant) -> Result<(), LoadError> {
    if !is_sha256_hex(&v.sha256) {
        return Err(LoadError::BadDigest(v.sha256.clone()));
    }
    if !is_content_address(&v.address) || !v.address.ends_with(&v.sha256[..12]) {
        return Err(LoadError::BadAddress(v.address.clone()));
    }
    if bytes.len() as u64 != v.bytes {
        return Err(LoadError::Size { want: v.bytes, got: bytes.len() as u64 });
    }
    let got = sha256_hex(bytes);
    if got != v.sha256 {
        return Err(LoadError::HashMismatch { want: v.sha256.clone(), got });
    }
    Ok(())
}

/// The cached bytes of `v`, if the file exists and still verifies. A file that
/// fails verification (a torn write, bit rot, tampering) is removed so the next
/// request downloads it again.
pub fn read_cache(dir: &Path, v: &ListedVariant) -> Option<Vec<u8>> {
    let path = cache_file(dir, &v.address).ok()?;
    let bytes = std::fs::read(&path).ok()?;
    if verify(&bytes, v).is_ok() {
        Some(bytes)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Store verified `bytes` under `dir/<address>.spz`, through a temporary file and
/// a rename so a reader never sees a partial file.
pub fn write_cache(dir: &Path, v: &ListedVariant, bytes: &[u8]) -> Result<(), LoadError> {
    let path = cache_file(dir, &v.address)?;
    let io = |e: std::io::Error| LoadError::Io(format!("{}: {e}", path.display()));
    std::fs::create_dir_all(dir).map_err(io)?;
    let tmp = path.with_extension("spz.part");
    std::fs::write(&tmp, bytes).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

/// The static splat data laid out for the shader (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct Packed {
    /// Splats packed.
    pub count: usize,
    /// Data texture width in texels ([`DATA_TEX_WIDTH`]).
    pub width: usize,
    /// Data texture height in rows.
    pub height: usize,
    /// RGBA32F texels, `width * height * 4` floats (the last row is padded).
    pub texels: Vec<f32>,
    /// xyz per splat, for the depth sort.
    pub positions: Vec<f32>,
    /// Bounds of the decoded positions.
    pub bounds_min: [f32; 3],
    /// Bounds of the decoded positions.
    pub bounds_max: [f32; 3],
}

/// The 3D covariance `R S S^T R^T` of a splat with linear scales `s` and unit
/// quaternion `q` (xyzw), as `[xx, xy, xz, yy, yz, zz]`. The quaternion is
/// renormalised; a zero quaternion is the identity.
pub fn covariance(s: [f32; 3], q: [f32; 4]) -> [f32; 6] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let (x, y, z, w) = if n > 0.0 && n.is_finite() {
        (q[0] / n, q[1] / n, q[2] / n, q[3] / n)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    // Rotation matrix, row-major.
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let v = [s[0] * s[0], s[1] * s[1], s[2] * s[2]];
    let c = |i: usize, j: usize| r[i][0] * v[0] * r[j][0] + r[i][1] * v[1] * r[j][1] + r[i][2] * v[2] * r[j][2];
    [c(0, 0), c(0, 1), c(0, 2), c(1, 1), c(1, 2), c(2, 2)]
}

/// Lay `splats` out as the data texture. Allocates the whole padded texture
/// once; call it on a worker thread.
pub fn pack(splats: &Splats) -> Packed {
    let n = splats.len();
    let height = n.div_ceil(DATA_ROW_SPLATS).max(1);
    let mut texels = vec![0.0f32; DATA_TEX_WIDTH * height * 4];
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for i in 0..n {
        let p = [splats.positions[i * 3], splats.positions[i * 3 + 1], splats.positions[i * 3 + 2]];
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
        let s = [splats.scales[i * 3], splats.scales[i * 3 + 1], splats.scales[i * 3 + 2]];
        let q = [
            splats.rotations[i * 4],
            splats.rotations[i * 4 + 1],
            splats.rotations[i * 4 + 2],
            splats.rotations[i * 4 + 3],
        ];
        let cov = covariance(s, q);
        let c = &splats.colors[i * 4..i * 4 + 4];
        let base = ((i / DATA_ROW_SPLATS) * DATA_TEX_WIDTH + (i % DATA_ROW_SPLATS) * TEXELS_PER_SPLAT) * 4;
        let t = &mut texels[base..base + TEXELS_PER_SPLAT * 4];
        t[0..4].copy_from_slice(&[p[0], p[1], p[2], c[3].clamp(0.0, 1.0)]);
        t[4..8].copy_from_slice(&[c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), 0.0]);
        t[8..12].copy_from_slice(&[cov[0], cov[1], cov[2], cov[3]]);
        t[12..16].copy_from_slice(&[cov[4], cov[5], 0.0, 0.0]);
    }
    if n == 0 {
        lo = [0.0; 3];
        hi = [0.0; 3];
    }
    Packed {
        count: n,
        width: DATA_TEX_WIDTH,
        height,
        texels,
        positions: splats.positions.clone(),
        bounds_min: lo,
        bounds_max: hi,
    }
}

/// Decode and pack `bytes`, which must already have passed [`verify`]; the
/// decoded count must match the listing.
pub fn decode_packed(bytes: &[u8], v: &ListedVariant) -> Result<Packed, LoadError> {
    let splats = spz::decode(bytes).map_err(|e| LoadError::Decode(e.to_string()))?;
    if splats.len() != v.splats as usize {
        return Err(LoadError::Count { want: v.splats, got: splats.len() });
    }
    Ok(pack(&splats))
}

/// The bytes of `floats` as little-endian `f32`, the layout of an
/// `Image.FORMAT_RGBAF` data texture.
pub fn f32_le_bytes(floats: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(floats.len() * 4);
    for f in floats {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// Where a load stands.
#[derive(Debug, Clone)]
pub enum State {
    /// Nothing started.
    Idle,
    /// A worker is reading the cache, or decoding.
    Working,
    /// The cache missed: GDScript must download `url` and call `supply`.
    NeedDownload { url: String, bytes: u64 },
    /// Decoded and packed; collect it with `take_data`.
    Ready(Arc<Packed>),
    /// Failed; `refused` means the bytes were not what the server listed.
    Failed { error: String, refused: bool },
}

/// The state and the generation of the request that owns it.
#[derive(Debug)]
struct Inner {
    generation: u64,
    state: State,
}

/// One load of one splat variant, run on worker threads. Used by the gdext
/// class and by the tests.
///
/// Every `begin`, `supply`, `fail` and `reset` starts a new *generation*, and a
/// worker may only publish its result into the generation it was started for. A
/// tier change re-plans while an earlier worker may still be decoding: that
/// worker's result is discarded instead of landing as the new plan's.
#[derive(Debug, Clone)]
pub struct LoadJob {
    inner: Arc<Mutex<Inner>>,
}

impl Default for LoadJob {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadJob {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner { generation: 0, state: State::Idle })),
        }
    }

    /// The current state.
    pub fn state(&self) -> State {
        self.lock().state.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start a new generation in `state`; returns its number.
    fn advance(&self, state: State) -> u64 {
        let mut g = self.lock();
        g.generation += 1;
        g.state = state;
        g.generation
    }

    /// Publish `state` if `generation` is still the current one.
    fn publish(inner: &Mutex<Inner>, generation: u64, state: State) {
        let mut g = inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.generation == generation {
            g.state = state;
        }
    }

    /// Start: read and verify the cache on a worker; on a hit decode and pack, on
    /// a miss go to [`State::NeedDownload`]. Without a `cache_dir` it always
    /// misses.
    pub fn begin(&self, plan: &Plan, cache_dir: Option<PathBuf>) {
        let generation = self.advance(State::Working);
        let (inner, v) = (self.inner.clone(), plan.variant.clone());
        std::thread::spawn(move || {
            let cached = cache_dir.as_deref().and_then(|d| read_cache(d, &v));
            let next = match cached {
                Some(bytes) => finish(decode_packed(&bytes, &v)),
                None => State::NeedDownload { url: v.url.clone(), bytes: v.bytes },
            };
            Self::publish(&inner, generation, next);
        });
    }

    /// Hand over the downloaded bytes: verify against the listing (a mismatch is
    /// a refusal, final, and writes nothing), store them in the cache, decode and
    /// pack, on a worker.
    pub fn supply(&self, plan: &Plan, bytes: Vec<u8>, cache_dir: Option<PathBuf>) {
        let generation = self.advance(State::Working);
        let (inner, v) = (self.inner.clone(), plan.variant.clone());
        std::thread::spawn(move || {
            let next = match verify(&bytes, &v) {
                Err(e) => State::Failed { refused: e.is_refusal(), error: e.to_string() },
                Ok(()) => {
                    // A cache that cannot be written costs a re-download next
                    // time, not the scene.
                    if let Some(d) = cache_dir.as_deref() {
                        if let Err(e) = write_cache(d, &v, &bytes) {
                            tracing::warn!("splat cache not written: {e}");
                        }
                    }
                    finish(decode_packed(&bytes, &v))
                }
            };
            Self::publish(&inner, generation, next);
        });
    }

    /// Mark a failed download (a transport error, not a refusal).
    pub fn fail(&self, error: String) {
        self.advance(State::Failed { error, refused: false });
    }

    /// Back to [`State::Idle`], abandoning any worker still running.
    pub fn reset(&self) {
        self.advance(State::Idle);
    }
}

fn finish(r: Result<Packed, LoadError>) -> State {
    match r {
        Ok(p) => State::Ready(Arc::new(p)),
        Err(e) => State::Failed { refused: e.is_refusal(), error: e.to_string() },
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
use godot::prelude::*;

#[cfg(not(test))]
fn v3(a: [f32; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

/// Godot face of the loader: `SplatLoader.new()`, `configure(cache_dir)`, then
/// `plan(listing_json, asset_id, tier)`, `begin()`, and poll `poll()` each frame:
/// `need_download` means fetch `url` and `supply(bytes)`; `ready` means
/// `take_data()`.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(base = RefCounted, rename = SplatLoader)]
pub struct SplatLoaderNode {
    base: Base<RefCounted>,
    cache_dir: Option<PathBuf>,
    plan: Option<Plan>,
    job: LoadJob,
}

#[cfg(not(test))]
#[godot_api]
impl IRefCounted for SplatLoaderNode {
    fn init(base: Base<RefCounted>) -> Self {
        Self { base, cache_dir: None, plan: None, job: LoadJob::new() }
    }
}

#[cfg(not(test))]
#[godot_api]
impl SplatLoaderNode {
    /// The cache directory as an absolute path (GDScript passes
    /// `ProjectSettings.globalize_path("user://splat-cache")`). Empty disables
    /// the cache.
    #[func]
    fn configure(&mut self, cache_dir: GString) {
        let s = cache_dir.to_string();
        self.cache_dir = (!s.is_empty()).then(|| PathBuf::from(s));
    }

    /// Parse the `/api/splats` body and pick the variant of `asset_id` (empty =
    /// the first headset asset) for `tier`. Returns {ok, error, refused,
    /// asset_id, title, credits, tier, variant, url, address, sha256, bytes,
    /// splats, bounds_min, bounds_max}.
    #[func]
    fn plan(&mut self, listing_json: GString, asset_id: GString, tier: i64) -> Dictionary {
        let mut d = Dictionary::new();
        self.job.reset();
        match plan(&listing_json.to_string(), Some(&asset_id.to_string()), tier.max(0) as usize) {
            Ok(p) => {
                d.set("ok", true);
                d.set("asset_id", GString::from(p.asset_id.as_str()));
                d.set("title", GString::from(p.title.as_str()));
                d.set("credits", GString::from(p.credits.as_str()));
                d.set("tier", p.tier as i64);
                d.set("variant", GString::from(p.variant.name.as_str()));
                d.set("url", GString::from(p.variant.url.as_str()));
                d.set("address", GString::from(p.variant.address.as_str()));
                d.set("sha256", GString::from(p.variant.sha256.as_str()));
                d.set("bytes", p.variant.bytes as i64);
                d.set("splats", p.variant.splats as i64);
                d.set("bounds_min", v3(p.bounds_min));
                d.set("bounds_max", v3(p.bounds_max));
                d.set("staged", p.stage.is_some());
                if let Some(st) = p.stage {
                    d.set("metres_per_unit", st.metres_per_unit as f64);
                    d.set("focus", v3(st.focus));
                }
                self.plan = Some(p);
            }
            Err(e) => {
                d.set("ok", false);
                d.set("error", GString::from(e.to_string().as_str()));
                d.set("refused", e.is_refusal());
                self.plan = None;
            }
        }
        d
    }

    /// Start the load: the cache is read and verified on a worker.
    #[func]
    fn begin(&mut self) {
        if let Some(p) = &self.plan {
            self.job.begin(p, self.cache_dir.clone());
        }
    }

    /// Hand over the downloaded bytes. A SHA-256 or size mismatch is a refusal:
    /// `poll()` reports `failed` with `refused` true, and the caller does not
    /// retry.
    #[func]
    fn supply(&mut self, bytes: PackedByteArray) {
        if let Some(p) = &self.plan {
            self.job.supply(p, bytes.to_vec(), self.cache_dir.clone());
        }
    }

    /// Report a transport failure (HTTP error, timeout). Not a refusal.
    #[func]
    fn fail(&mut self, error: GString) {
        self.job.fail(error.to_string());
    }

    /// {state: idle | working | need_download | ready | failed, url, bytes,
    /// error, refused}.
    #[func]
    fn poll(&self) -> Dictionary {
        let mut d = Dictionary::new();
        match self.job.state() {
            State::Idle => d.set("state", "idle"),
            State::Working => d.set("state", "working"),
            State::NeedDownload { url, bytes } => {
                d.set("state", "need_download");
                d.set("url", GString::from(url.as_str()));
                d.set("bytes", bytes as i64);
            }
            State::Ready(_) => d.set("state", "ready"),
            State::Failed { error, refused } => {
                d.set("state", "failed");
                d.set("error", GString::from(error.as_str()));
                d.set("refused", refused);
            }
        }
        d
    }

    /// The packed scene once `poll()` says ready, else empty: {count, width,
    /// height, data (RGBA32F bytes), positions, bounds_min, bounds_max}.
    #[func]
    fn take_data(&mut self) -> Dictionary {
        let mut d = Dictionary::new();
        if let State::Ready(p) = self.job.state() {
            d.set("count", p.count as i64);
            d.set("width", p.width as i64);
            d.set("height", p.height as i64);
            d.set("data", PackedByteArray::from(f32_le_bytes(&p.texels).as_slice()));
            d.set("positions", PackedFloat32Array::from(p.positions.as_slice()));
            d.set("bounds_min", v3(p.bounds_min));
            d.set("bounds_max", v3(p.bounds_max));
        }
        d
    }

    /// Texels a splat occupies, splats a row, and the texture width: the shader's
    /// constants.
    #[func]
    fn layout(&self) -> Dictionary {
        let mut d = Dictionary::new();
        d.set("texels_per_splat", TEXELS_PER_SPLAT as i64);
        d.set("row_splats", DATA_ROW_SPLATS as i64);
        d.set("width", DATA_TEX_WIDTH as i64);
        d.set("index_width", crate::splat_sort::INDEX_TEX_WIDTH as i64);
        d
    }

    /// Back to idle, dropping any pending result.
    #[func]
    fn reset(&mut self) {
        self.job.reset();
        self.plan = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use visionclaw_splat::address::address_of_hex;
    use visionclaw_splat::manifest::{Bounds, Licence, VariantName};

    /// `n` splats on a lattice with distinct scales and a rotation about z.
    fn fixture(n: usize) -> Splats {
        let mut s = Splats::default();
        for i in 0..n {
            let f = i as f32;
            s.positions.extend_from_slice(&[(f % 10.0) * 0.5 - 2.0, (f / 10.0).floor() * 0.25, -(f % 7.0)]);
            s.scales.extend_from_slice(&[0.02 + 0.001 * (f % 5.0), 0.03, 0.05]);
            let a = 0.3 * f;
            s.rotations.extend_from_slice(&[0.0, 0.0, (a / 2.0).sin(), (a / 2.0).cos()]);
            s.colors.extend_from_slice(&[(f % 5.0) / 5.0, 0.5, 1.0 - (f % 3.0) / 3.0, 0.5 + 0.4 * ((f % 4.0) / 4.0)]);
        }
        s
    }

    fn spz_of(n: usize) -> Vec<u8> {
        spz::encode_v3(&fixture(n), 12)
    }

    fn variant(name: VariantName, bytes: &[u8], splats: u32) -> ListedVariant {
        let sha = sha256_hex(bytes);
        let address = address_of_hex(&sha);
        ListedVariant {
            name,
            splats,
            sh_degree: 0,
            sha256: sha,
            url: format!("/api/splats/blob/{address}"),
            address,
            bytes: bytes.len() as u64,
        }
    }

    fn asset(id: &str, variants: Vec<ListedVariant>) -> ListedAsset {
        ListedAsset {
            id: id.into(),
            title: format!("{id} scene"),
            author: "A. Author".into(),
            source_url: "https://example.org/scene".into(),
            licence: Licence { spdx: "CC-BY-4.0".into(), url: "https://creativecommons.org/licenses/by/4.0/".into() },
            attribution: "A. Author, ISS".into(),
            modified: true,
            bounds: Bounds { min: [-3.0, 0.0, -7.0], max: [2.5, 3.0, 0.0] },
            stage: None,
            variants,
        }
    }

    fn listing_json(assets: Vec<ListedAsset>) -> String {
        serde_json::to_string(&SplatListing { assets }).unwrap()
    }

    /// A listing with every tier's variant, each its own bytes.
    fn full_listing() -> (String, Vec<Vec<u8>>) {
        let files: Vec<Vec<u8>> = vec![spz_of(30), spz_of(20), spz_of(10)];
        let vs = vec![
            variant(VariantName::Xr3, &files[0], 30),
            variant(VariantName::Xr2, &files[1], 20),
            variant(VariantName::Xr1, &files[2], 10),
            // a desktop variant the headset must never pick
            variant(VariantName::Desktop, &spz_of(40), 40),
        ];
        (listing_json(vec![asset("iss", vs)]), files)
    }

    fn wait(job: &LoadJob, done: impl Fn(&State) -> bool) -> State {
        let t0 = Instant::now();
        loop {
            let s = job.state();
            if done(&s) {
                return s;
            }
            assert!(t0.elapsed() < Duration::from_secs(20), "timed out in {s:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn settled(s: &State) -> bool {
        !matches!(s, State::Working | State::Idle)
    }

    // ── planning ──

    #[test]
    fn a_staged_asset_carries_its_stage_into_the_plan() {
        let files = vec![spz_of(10)];
        let mut a = asset("sentinel", vec![variant(VariantName::Xr1, &files[0], 10)]);
        a.stage = Some(Stage { metres_per_unit: 2.25, focus: [0.0, 2.0, -6.0] });
        let p = plan(&listing_json(vec![a]), None, 1).unwrap();
        assert_eq!(p.stage, Some(Stage { metres_per_unit: 2.25, focus: [0.0, 2.0, -6.0] }));
    }

    #[test]
    fn a_listing_from_a_server_without_stages_still_plans() {
        let (json, _) = full_listing();
        assert!(!json.contains("stage"));
        assert_eq!(plan(&json, None, 3).unwrap().stage, None);
    }

    #[test]
    fn tiers_one_to_three_pick_xr_1_to_xr_3() {
        let (json, _) = full_listing();
        for (tier, want) in [(1, VariantName::Xr1), (2, VariantName::Xr2), (3, VariantName::Xr3)] {
            let p = plan(&json, Some("iss"), tier).unwrap();
            assert_eq!((p.variant.name, p.tier), (want, tier));
            assert!(p.variant.url.starts_with("/api/splats/blob/sha256-12-"));
        }
    }

    #[test]
    fn a_missing_tier_falls_back_downwards_never_upwards_and_never_to_a_desktop_variant() {
        let f = spz_of(10);
        let only_xr1 = listing_json(vec![asset("a", vec![variant(VariantName::Xr1, &f, 10), variant(VariantName::DesktopHi, &f, 10)])]);
        assert_eq!(plan(&only_xr1, Some("a"), 3).unwrap().tier, 1, "tier 3 asked, tier 1 served");
        let only_xr3 = listing_json(vec![asset("a", vec![variant(VariantName::Xr3, &f, 10)])]);
        assert_eq!(
            plan(&only_xr3, Some("a"), 2),
            Err(LoadError::NoVariant { asset: "a".into(), tier: 2 }),
            "a larger scene than the governor allows is never served"
        );
        assert!(matches!(plan(&only_xr1, Some("a"), 0), Err(LoadError::NoVariant { .. })), "tier 0 is off");
        let desktop_only = listing_json(vec![asset("a", vec![variant(VariantName::Desktop, &f, 10)])]);
        assert!(matches!(plan(&desktop_only, Some("a"), 3), Err(LoadError::NoVariant { .. })));
        assert!(matches!(plan(&desktop_only, None, 3), Err(LoadError::NoAsset(None))));
    }

    #[test]
    fn an_empty_asset_id_takes_the_first_headset_asset() {
        let f = spz_of(10);
        let json = listing_json(vec![
            asset("desk", vec![variant(VariantName::Desktop, &f, 10)]),
            asset("hmd", vec![variant(VariantName::Xr1, &f, 10)]),
        ]);
        assert_eq!(plan(&json, None, 3).unwrap().asset_id, "hmd");
        assert_eq!(plan(&json, Some(""), 3).unwrap().asset_id, "hmd");
        assert!(matches!(plan(&json, Some("nope"), 3), Err(LoadError::NoAsset(Some(_)))));
    }

    #[test]
    fn the_plan_carries_the_credits_line_and_bounds() {
        let (json, _) = full_listing();
        let p = plan(&json, Some("iss"), 3).unwrap();
        assert_eq!(
            p.credits,
            "A. Author, ISS (CC-BY-4.0 https://creativecommons.org/licenses/by/4.0/), modified (decimated)"
        );
        assert_eq!((p.bounds_min, p.bounds_max), ([-3.0, 0.0, -7.0], [2.5, 3.0, 0.0]));
    }

    #[test]
    fn a_malformed_listing_is_an_error_not_a_panic() {
        for bad in ["", "{", "[]", r#"{"assets":[{"id":1}]}"#] {
            assert!(matches!(plan(bad, None, 1), Err(LoadError::Listing(_))), "{bad:?}");
        }
    }

    // ── verification: a mismatch is a refusal ──

    #[test]
    fn the_right_bytes_verify() {
        let f = spz_of(10);
        assert_eq!(verify(&f, &variant(VariantName::Xr1, &f, 10)), Ok(()));
    }

    #[test]
    fn a_flipped_byte_is_a_refusal_naming_both_digests() {
        let f = spz_of(10);
        let v = variant(VariantName::Xr1, &f, 10);
        let mut bad = f.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 0x01;
        let e = verify(&bad, &v).unwrap_err();
        assert!(e.is_refusal(), "{e}");
        match e {
            LoadError::HashMismatch { want, got } => {
                assert_eq!(want, v.sha256);
                assert_eq!(got, sha256_hex(&bad));
                assert_ne!(want, got);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_wrong_length_or_malformed_digest_or_address_is_a_refusal() {
        let f = spz_of(10);
        let v = variant(VariantName::Xr1, &f, 10);
        assert!(matches!(verify(&f[..f.len() - 1], &v), Err(LoadError::Size { .. })));
        assert!(matches!(verify(&f, &ListedVariant { sha256: "ABC".into(), ..v.clone() }), Err(LoadError::BadDigest(_))));
        assert!(matches!(
            verify(&f, &ListedVariant { sha256: v.sha256.to_uppercase(), ..v.clone() }),
            Err(LoadError::BadDigest(_))
        ));
        assert!(matches!(
            verify(&f, &ListedVariant { address: "sha256-12-000000000000".into(), ..v.clone() }),
            Err(LoadError::BadAddress(_))
        )); // an address that is not the digest's prefix
        for e in [
            LoadError::Size { want: 1, got: 2 },
            LoadError::BadAddress(String::new()),
            LoadError::BadDigest(String::new()),
            LoadError::Count { want: 1, got: 2 },
        ] {
            assert!(e.is_refusal());
        }
        for e in [LoadError::Decode("x".into()), LoadError::Io("x".into()), LoadError::NoAsset(None)] {
            assert!(!e.is_refusal(), "{e}");
        }
    }

    #[test]
    fn a_traversing_address_never_reaches_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        for a in ["../../etc/passwd", "sha256-12-../../x", "sha256-12-AAAAAAAAAAAA", "sha256-12-0123456789a", "", "/abs"] {
            assert!(matches!(cache_file(dir.path(), a), Err(LoadError::BadAddress(_))), "{a:?}");
        }
        let ok = cache_file(dir.path(), "sha256-12-0123456789ab").unwrap();
        assert_eq!(ok, dir.path().join("sha256-12-0123456789ab.spz"));
    }

    // ── cache ──

    #[test]
    fn the_cache_round_trips_and_re_verifies_every_hit() {
        let dir = tempfile::tempdir().unwrap();
        let f = spz_of(10);
        let v = variant(VariantName::Xr1, &f, 10);
        assert!(read_cache(dir.path(), &v).is_none(), "cold");
        write_cache(dir.path(), &v, &f).unwrap();
        let path = cache_file(dir.path(), &v.address).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("spz.part").exists(), "no temp file left");
        assert_eq!(read_cache(dir.path(), &v).as_deref(), Some(&f[..]));
        // The file is corrupted on disk: the hit is re-verified, fails, and the
        // file is removed so it is downloaded again.
        let mut bad = f.clone();
        bad[20] ^= 0xff;
        std::fs::write(&path, &bad).unwrap();
        assert!(read_cache(dir.path(), &v).is_none());
        assert!(!path.exists(), "a corrupt entry is dropped");
        // A truncated file too.
        std::fs::write(&path, &f[..f.len() / 2]).unwrap();
        assert!(read_cache(dir.path(), &v).is_none());
    }

    // ── packing ──

    #[test]
    fn covariance_of_an_axis_aligned_splat_is_its_squared_scales() {
        let c = covariance([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(c, [1.0, 0.0, 0.0, 4.0, 0.0, 9.0]);
        // not normalised and zero quaternions are tolerated
        assert_eq!(covariance([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 5.0]), c);
        assert_eq!(covariance([1.0, 2.0, 3.0], [0.0; 4]), c);
    }

    #[test]
    fn covariance_rotates_the_axes() {
        // 90 degrees about z swaps x and y: variance 1 along y, 4 along x
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let c = covariance([1.0, 2.0, 3.0], [0.0, 0.0, h, h]);
        let want = [4.0, 0.0, 0.0, 1.0, 0.0, 9.0];
        for (g, w) in c.iter().zip(want) {
            assert!((g - w).abs() < 1.0e-5, "{c:?} vs {want:?}");
        }
        // a 45 degree turn puts covariance in xy: cos sin (1 - 4) = -1.5, and the
        // trace of the block is preserved
        let (s, k) = ((std::f32::consts::FRAC_PI_8).sin(), (std::f32::consts::FRAC_PI_8).cos());
        let c = covariance([1.0, 2.0, 3.0], [0.0, 0.0, s, k]);
        assert!((c[0] + c[3] - 5.0).abs() < 1.0e-5, "trace of the xy block");
        assert!((c[1] + 1.5).abs() < 1.0e-5, "xy {}", c[1]);
        // the matrix is positive semi-definite for any rotation
        let c = covariance([0.3, 0.1, 0.7], [0.3, -0.5, 0.2, 0.6]);
        assert!(c[0] >= 0.0 && c[3] >= 0.0 && c[5] >= 0.0);
        assert!(c[0] * c[3] - c[1] * c[1] >= -1.0e-6);
    }

    #[test]
    fn pack_lays_out_four_texels_a_splat_in_rows_of_1024() {
        let n = DATA_ROW_SPLATS + 5; // spills into a second row
        let s = fixture(n);
        let p = pack(&s);
        assert_eq!((p.count, p.width, p.height), (n, 4096, 2));
        assert_eq!(p.texels.len(), 4096 * 2 * 4);
        assert_eq!(p.positions, s.positions);
        for &i in &[0usize, 1, 7, DATA_ROW_SPLATS - 1, DATA_ROW_SPLATS, n - 1] {
            let base = ((i / DATA_ROW_SPLATS) * DATA_TEX_WIDTH + (i % DATA_ROW_SPLATS) * TEXELS_PER_SPLAT) * 4;
            let t = &p.texels[base..base + 16];
            assert_eq!(&t[0..3], &s.positions[i * 3..i * 3 + 3], "splat {i} position");
            assert_eq!(t[3], s.colors[i * 4 + 3], "splat {i} opacity");
            assert_eq!(&t[4..7], &s.colors[i * 4..i * 4 + 3], "splat {i} colour");
            let cov = covariance(
                [s.scales[i * 3], s.scales[i * 3 + 1], s.scales[i * 3 + 2]],
                [s.rotations[i * 4], s.rotations[i * 4 + 1], s.rotations[i * 4 + 2], s.rotations[i * 4 + 3]],
            );
            assert_eq!(&t[8..12], &cov[0..4], "splat {i} cov xx xy xz yy");
            assert_eq!(&t[12..14], &cov[4..6], "splat {i} cov yz zz");
        }
        // the padding past the last splat is zero
        let after = ((n % DATA_ROW_SPLATS) * TEXELS_PER_SPLAT) * 4 + DATA_TEX_WIDTH * 4;
        assert!(p.texels[after..after + 64].iter().all(|&v| v == 0.0));
        assert_eq!(p.bounds_min[0], -2.0);
        assert!(p.bounds_max[0] > p.bounds_min[0]);
    }

    #[test]
    fn packing_an_empty_scene_is_one_zero_row() {
        let p = pack(&Splats::default());
        assert_eq!((p.count, p.height, p.bounds_min, p.bounds_max), (0, 1, [0.0; 3], [0.0; 3]));
        assert!(p.texels.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn f32_bytes_are_little_endian() {
        assert_eq!(f32_le_bytes(&[1.0, -2.0]), [0, 0, 128, 63, 0, 0, 0, 192]);
    }

    // ── the job ──

    #[test]
    fn a_cold_load_asks_for_the_download_then_verifies_caches_and_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let (json, files) = full_listing();
        let p = plan(&json, Some("iss"), 3).unwrap();
        let job = LoadJob::new();
        job.begin(&p, Some(dir.path().to_path_buf()));
        match wait(&job, settled) {
            State::NeedDownload { url, bytes } => {
                assert_eq!(url, p.variant.url);
                assert_eq!(bytes, files[0].len() as u64);
            }
            other => panic!("{other:?}"),
        }
        job.supply(&p, files[0].clone(), Some(dir.path().to_path_buf()));
        match wait(&job, settled) {
            State::Ready(d) => {
                assert_eq!(d.count, 30);
                assert_eq!(d.positions.len(), 90);
            }
            other => panic!("{other:?}"),
        }
        assert!(cache_file(dir.path(), &p.variant.address).unwrap().exists(), "stored under the address");
        // A warm load never asks for the download.
        let warm = LoadJob::new();
        warm.begin(&p, Some(dir.path().to_path_buf()));
        assert!(matches!(wait(&warm, settled), State::Ready(d) if d.count == 30));
    }

    #[test]
    fn a_download_that_fails_the_hash_is_refused_not_retried_and_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let (json, files) = full_listing();
        let p = plan(&json, Some("iss"), 3).unwrap();
        let mut tampered = files[0].clone();
        let at = tampered.len() - 3;
        tampered[at] ^= 0x40; // same length, one bit different
        let job = LoadJob::new();
        job.supply(&p, tampered, Some(dir.path().to_path_buf()));
        match wait(&job, settled) {
            State::Failed { error, refused } => {
                assert!(refused, "a hash mismatch is a refusal");
                assert!(error.contains("does not match"), "{error}");
                assert!(error.contains(&p.variant.sha256), "{error}");
            }
            other => panic!("{other:?}"),
        }
        assert!(!cache_file(dir.path(), &p.variant.address).unwrap().exists(), "nothing cached");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "no temp file either");
        // And it stays refused: no hidden second attempt moves the state.
        std::thread::sleep(Duration::from_millis(50));
        assert!(matches!(job.state(), State::Failed { refused: true, .. }));
    }

    #[test]
    fn a_short_or_wrong_file_is_refused_by_size_before_any_hashing() {
        let (json, files) = full_listing();
        let p = plan(&json, Some("iss"), 3).unwrap();
        for bad in [files[0][..10].to_vec(), Vec::new(), files[1].clone()] {
            let job = LoadJob::new();
            job.supply(&p, bad, None);
            assert!(matches!(wait(&job, settled), State::Failed { refused: true, .. }));
        }
    }

    #[test]
    fn a_corrupt_cache_entry_falls_back_to_a_download_request() {
        let dir = tempfile::tempdir().unwrap();
        let (json, files) = full_listing();
        let p = plan(&json, Some("iss"), 3).unwrap();
        let path = cache_file(dir.path(), &p.variant.address).unwrap();
        let mut bad = files[0].clone();
        bad[30] ^= 0x10;
        std::fs::write(&path, bad).unwrap();
        let job = LoadJob::new();
        job.begin(&p, Some(dir.path().to_path_buf()));
        assert!(matches!(wait(&job, settled), State::NeedDownload { .. }), "re-verified on the hit, then re-fetched");
        assert!(!path.exists());
    }

    #[test]
    fn a_listing_that_lies_about_the_splat_count_is_refused() {
        let f = spz_of(10);
        let mut v = variant(VariantName::Xr1, &f, 11); // the file has 10
        v.name = VariantName::Xr1;
        let json = listing_json(vec![asset("a", vec![v])]);
        let p = plan(&json, Some("a"), 1).unwrap();
        let job = LoadJob::new();
        job.supply(&p, f, None);
        match wait(&job, settled) {
            State::Failed { refused, error } => {
                assert!(refused, "{error}");
                assert!(error.contains("11"), "{error}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bytes_that_hash_right_but_are_not_spz_fail_to_decode_without_a_refusal_flag() {
        let junk = vec![7u8; 200];
        let v = variant(VariantName::Xr1, &junk, 5);
        let p = plan(&listing_json(vec![asset("a", vec![v])]), Some("a"), 1).unwrap();
        let job = LoadJob::new();
        job.supply(&p, junk, None);
        match wait(&job, settled) {
            State::Failed { refused, error } => {
                assert!(!refused, "the hash matched; the server listed a bad file");
                assert!(error.contains("did not decode"), "{error}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_result_from_an_abandoned_plan_never_lands() {
        // A tier change re-plans while the old worker is still decoding.
        let (json, files) = full_listing();
        let old = plan(&json, Some("iss"), 3).unwrap();
        let new = plan(&json, Some("iss"), 1).unwrap();
        let job = LoadJob::new();
        job.supply(&old, files[0].clone(), None); // worker for the old plan starts
        job.reset(); // the re-plan abandons it at once
        job.begin(&new, None);
        assert!(matches!(wait(&job, settled), State::NeedDownload { url, .. } if url == new.variant.url));
        // Give the abandoned worker ample time to finish; it must not overwrite.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            matches!(job.state(), State::NeedDownload { ref url, .. } if *url == new.variant.url),
            "the old plan's Ready leaked into the new plan: {:?}",
            job.state()
        );
        // Likewise a worker that would have failed.
        let job = LoadJob::new();
        job.supply(&old, vec![1, 2, 3], None);
        job.fail("HTTP 503".into());
        std::thread::sleep(Duration::from_millis(100));
        assert!(matches!(job.state(), State::Failed { refused: false, .. }), "{:?}", job.state());
    }

    #[test]
    fn a_transport_failure_is_not_a_refusal() {
        let job = LoadJob::new();
        job.fail("HTTP 503".into());
        assert!(matches!(job.state(), State::Failed { refused: false, .. }));
        job.reset();
        assert!(matches!(job.state(), State::Idle));
    }

    #[test]
    fn without_a_cache_directory_it_always_asks_for_the_download() {
        let (json, _) = full_listing();
        let p = plan(&json, Some("iss"), 1).unwrap();
        let job = LoadJob::new();
        job.begin(&p, None);
        assert!(matches!(wait(&job, settled), State::NeedDownload { .. }));
    }

    /// Dev check against a real SPZ v3 file (not committed): set `SPLAT_FIXTURE`
    /// to its path and run with `--ignored --nocapture`.
    #[test]
    #[ignore = "needs SPLAT_FIXTURE=<path to a real SPZ v3 file>"]
    fn a_real_spz_file_decodes_and_packs() {
        let path = std::env::var("SPLAT_FIXTURE").expect("SPLAT_FIXTURE");
        let bytes = std::fs::read(path).unwrap();
        let t0 = Instant::now();
        let splats = spz::decode(&bytes).unwrap();
        let decode_ms = t0.elapsed().as_secs_f32() * 1000.0;
        let t1 = Instant::now();
        let p = pack(&splats);
        let pack_ms = t1.elapsed().as_secs_f32() * 1000.0;
        println!(
            "real SPZ: {} splats, decode {:.1} ms, pack {:.1} ms (this CPU), texture {}x{}, bounds {:?} .. {:?}",
            p.count, decode_ms, pack_ms, p.width, p.height, p.bounds_min, p.bounds_max
        );
        assert!(p.count > 0);
        assert!(p.texels.iter().all(|v| v.is_finite()), "every texel finite");
        let mut order = Vec::new();
        let mut scratch = crate::splat_sort::SortScratch::default();
        let t2 = Instant::now();
        crate::splat_sort::sort_back_to_front(&p.positions, [0.0; 3], [0.0, 0.0, -1.0], &mut order, &mut scratch);
        println!("sort of {} real splats: {:.2} ms (this CPU)", p.count, t2.elapsed().as_secs_f32() * 1000.0);
    }
}
