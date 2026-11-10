//! CPU depth sort for the headset's splat layer (ADR-2170 decision 8).
//!
//! Gaussian splats blend, so they must be drawn far to near. Compatibility has
//! no compute, so the order comes from a CPU radix sort on 16-bit view depth.
//! Three rules keep it off the frame:
//!
//! * **One order for both eyes.** The key is the depth along the head (mid-eye)
//!   forward axis, so the left and right eye draw the same order. At an IPD of
//!   about 63 mm the order error is negligible beyond about 0.5 m, which the
//!   shader's near fade hides.
//! * **It runs on a worker thread**, never the render thread ([`SplatSorter`]).
//!   The render thread only decides whether to ask ([`PoseTrigger`]), hands over
//!   the head pose, and picks up a finished order.
//! * **Re-sort cadence.** A sort is asked for when the head has moved
//!   [`MOVE_THRESHOLD_M`] (5 cm) or turned [`TURN_THRESHOLD_DEG`] (2 degrees)
//!   since the last sort *that was started*, and no more often than
//!   [`MAX_SORT_HZ`] (30 Hz). A request made while the worker is busy is dropped,
//!   not queued, and a result that arrives more than [`DEFAULT_MAX_AGE`] after it
//!   was asked for is dropped as late. Between sorts the previous order is drawn.
//!
//! The output is an index buffer of 4 bytes a splat: the sorted splat indices as
//! little-endian `f32` (exact below 2^24), which is the byte layout of an
//! `Image.FORMAT_RF` index texture [`INDEX_TEX_WIDTH`] texels wide. The vertex
//! shader reads instance `i`'s splat with `texelFetch` on that texture.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Head translation since the last sort that asks for a new one, metres.
pub const MOVE_THRESHOLD_M: f32 = 0.05;
/// Head rotation since the last sort that asks for a new one, degrees.
pub const TURN_THRESHOLD_DEG: f32 = 2.0;
/// Most sorts started a second.
pub const MAX_SORT_HZ: f32 = 30.0;
/// A sort finished this long after it was asked for is dropped (two periods of
/// [`MAX_SORT_HZ`]).
pub const DEFAULT_MAX_AGE: Duration = Duration::from_millis(66);
/// Width of the index texture in texels; the shader's `INDEX_W`.
pub const INDEX_TEX_WIDTH: usize = 1024;

/// Slack for float rounding when comparing a period against `1 / MAX_SORT_HZ`.
const PERIOD_EPS: f32 = 1.0e-6;

/// Depth of each position along `fwd` from `eye`: the view-space depth of a
/// camera at `eye` looking along `fwd` (larger is farther).
fn depth_of(p: &[f32], eye: [f32; 3], fwd: [f32; 3]) -> f32 {
    (p[0] - eye[0]) * fwd[0] + (p[1] - eye[1]) * fwd[1] + (p[2] - eye[2]) * fwd[2]
}

/// Reusable buffers for [`sort_back_to_front`], so a sort allocates nothing in
/// steady state.
#[derive(Debug, Default)]
pub struct SortScratch {
    keys: Vec<u16>,
    tmp: Vec<u32>,
}

/// Sort `positions` (xyz a splat) back to front from a head at `eye` facing
/// `fwd` (a unit vector), into `order` (splat indices, farthest first).
///
/// The key is the depth along `fwd` quantised to 16 bits over this call's own
/// depth range, then sorted by two 8-bit LSD radix passes (stable, so equal keys
/// keep index order and the result is deterministic). Quantisation puts splats
/// within one 1/65535 of the scene depth range in index order, which is below a
/// pixel at any distance the layer draws. A non-finite depth sorts as the
/// farthest.
pub fn sort_back_to_front(
    positions: &[f32],
    eye: [f32; 3],
    fwd: [f32; 3],
    order: &mut Vec<u32>,
    scratch: &mut SortScratch,
) {
    let n = positions.len() / 3;
    order.clear();
    scratch.keys.clear();
    scratch.tmp.clear();
    if n == 0 {
        return;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in positions.chunks_exact(3) {
        let d = depth_of(p, eye, fwd);
        if d.is_finite() {
            lo = lo.min(d);
            hi = hi.max(d);
        }
    }
    let range = hi - lo;
    let scale = if range.is_finite() && range > 0.0 {
        65535.0 / range
    } else {
        0.0
    };
    // Far first = ascending on the inverted key.
    scratch.keys.extend(positions.chunks_exact(3).map(|p| {
        let d = depth_of(p, eye, fwd);
        if d.is_finite() {
            65535 - (((d - lo) * scale).clamp(0.0, 65535.0) as u16)
        } else {
            0
        }
    }));
    // one scan for both digit histograms
    let mut lo_hist = [0u32; 256];
    let mut hi_hist = [0u32; 256];
    for &k in &scratch.keys {
        lo_hist[(k & 0xff) as usize] += 1;
        hi_hist[(k >> 8) as usize] += 1;
    }
    let prefix = |h: &mut [u32; 256]| {
        let mut sum = 0;
        for c in h.iter_mut() {
            let v = *c;
            *c = sum;
            sum += v;
        }
    };
    prefix(&mut lo_hist);
    prefix(&mut hi_hist);
    scratch.tmp.resize(n, 0);
    for (i, &k) in scratch.keys.iter().enumerate() {
        let b = &mut lo_hist[(k & 0xff) as usize];
        scratch.tmp[*b as usize] = i as u32;
        *b += 1;
    }
    order.resize(n, 0);
    for &i in &scratch.tmp {
        let k = scratch.keys[i as usize];
        let b = &mut hi_hist[(k >> 8) as usize];
        order[*b as usize] = i;
        *b += 1;
    }
}

/// The bytes of the `FORMAT_RF` index texture for `order`: each index as a
/// little-endian `f32`, padded with zeros to whole rows of [`INDEX_TEX_WIDTH`]
/// texels (the padding is never drawn: the MultiMesh shows `order.len()`
/// instances). 4 bytes a splat.
pub fn index_texture_bytes(order: &[u32]) -> Vec<u8> {
    let rows = order.len().div_ceil(INDEX_TEX_WIDTH).max(1);
    let mut out = Vec::with_capacity(rows * INDEX_TEX_WIDTH * 4);
    for &i in order {
        out.extend_from_slice(&(i as f32).to_le_bytes());
    }
    out.resize(rows * INDEX_TEX_WIDTH * 4, 0);
    out
}

/// Texture height for `n` splats at [`INDEX_TEX_WIDTH`] texels a row.
pub fn index_texture_height(n: usize) -> usize {
    n.div_ceil(INDEX_TEX_WIDTH).max(1)
}

/// A head pose in the splat volume's local frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// Head position, local metres.
    pub pos: [f32; 3],
    /// Head forward, a unit vector.
    pub fwd: [f32; 3],
}

/// Decides when the head has moved enough to ask for a new order. Pure and
/// clocked by the caller, so a scripted head path tests the cadence exactly.
///
/// The reference is the pose of the last sort *requested* ([`PoseTrigger::mark`]),
/// not the last frame: slow drift accumulates until it crosses the threshold
/// instead of never triggering, and jitter inside the threshold never does.
#[derive(Debug, Clone, Default)]
pub struct PoseTrigger {
    last: Option<(Pose, f32)>,
}

impl PoseTrigger {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when a sort should be asked for at `now` (seconds). `scale` is the
    /// metres of world per local unit, so the 5 cm threshold is measured in the
    /// world (the volume's fit scale is not 1). The first call after
    /// construction or [`reset`](Self::reset) is always true.
    pub fn should_sort(&self, pose: &Pose, scale: f32, now: f32) -> bool {
        let Some((last, at)) = self.last else {
            return true;
        };
        if now - at < 1.0 / MAX_SORT_HZ - PERIOD_EPS {
            return false;
        }
        let d = [
            pose.pos[0] - last.pos[0],
            pose.pos[1] - last.pos[1],
            pose.pos[2] - last.pos[2],
        ];
        let moved = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * scale.max(0.0);
        if moved >= MOVE_THRESHOLD_M {
            return true;
        }
        let dot =
            (pose.fwd[0] * last.fwd[0] + pose.fwd[1] * last.fwd[1] + pose.fwd[2] * last.fwd[2])
                .clamp(-1.0, 1.0);
        dot.acos().to_degrees() >= TURN_THRESHOLD_DEG
    }

    /// Record that a sort was started from `pose` at `now`.
    pub fn mark(&mut self, pose: &Pose, now: f32) {
        self.last = Some((*pose, now));
    }

    /// Forget the reference: the next [`should_sort`](Self::should_sort) is true
    /// (a new volume transform, a new splat set).
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// What a finished sort hands the render thread.
#[derive(Debug, Clone)]
pub struct SortedOrder {
    /// Sequence number of the request that produced it.
    pub seq: u64,
    /// Splat indices, farthest first.
    pub order: Vec<u32>,
    /// [`index_texture_bytes`] of `order`.
    pub texels: Vec<u8>,
    /// Time the sort itself took, milliseconds.
    pub sort_ms: f32,
}

/// Counters for the HUD and the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SortStats {
    /// Sorts the worker finished and the render thread accepted.
    pub accepted: u64,
    /// Requests refused because the worker was still busy (never queued).
    pub skipped_busy: u64,
    /// Sorts finished too late and dropped.
    pub dropped_late: u64,
    /// The last sort's duration, ms.
    pub last_ms: f32,
    /// p95 of the last 128 sort durations, ms.
    pub p95_ms: f32,
}

struct Job {
    seq: u64,
    pose: Pose,
    asked: Instant,
}

enum Done {
    Order(SortedOrder),
    Late,
}

/// A worker thread that sorts one splat set. Created per loaded splat set; the
/// thread is joined on drop. All methods are non-blocking.
pub struct SplatSorter {
    jobs: Sender<Job>,
    results: Receiver<Done>,
    busy: Arc<AtomicBool>,
    durations: Arc<Mutex<Vec<f32>>>,
    skipped: AtomicU64,
    late: Arc<AtomicU64>,
    accepted: AtomicU64,
    seq: AtomicU64,
    last_ms: Mutex<f32>,
    handle: Option<JoinHandle<()>>,
}

impl SplatSorter {
    /// Start a worker for `positions` (xyz a splat) with the default lateness
    /// bound.
    pub fn new(positions: Arc<Vec<f32>>) -> Self {
        Self::with_options(positions, DEFAULT_MAX_AGE, Duration::ZERO)
    }

    /// As [`new`](Self::new) with an explicit lateness bound and, for tests, an
    /// artificial delay inside the worker (to hold it busy or make it late).
    pub fn with_options(positions: Arc<Vec<f32>>, max_age: Duration, work_delay: Duration) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, results) = channel::<Done>();
        let busy = Arc::new(AtomicBool::new(false));
        let durations = Arc::new(Mutex::new(Vec::<f32>::new()));
        let late = Arc::new(AtomicU64::new(0));
        let (b, d, l) = (busy.clone(), durations.clone(), late.clone());
        let handle = std::thread::Builder::new()
            .name("splat-sort".into())
            .spawn(move || {
                let mut order = Vec::new();
                let mut scratch = SortScratch::default();
                while let Ok(job) = job_rx.recv() {
                    let t0 = Instant::now();
                    sort_back_to_front(
                        &positions,
                        job.pose.pos,
                        job.pose.fwd,
                        &mut order,
                        &mut scratch,
                    );
                    if !work_delay.is_zero() {
                        std::thread::sleep(work_delay);
                    }
                    let sort_ms = t0.elapsed().as_secs_f32() * 1000.0;
                    let msg = if job.asked.elapsed() > max_age {
                        l.fetch_add(1, Ordering::Relaxed);
                        Done::Late
                    } else {
                        let mut ds = d.lock().unwrap_or_else(|e| e.into_inner());
                        if ds.len() >= 128 {
                            ds.remove(0);
                        }
                        ds.push(sort_ms);
                        Done::Order(SortedOrder {
                            seq: job.seq,
                            texels: index_texture_bytes(&order),
                            order: order.clone(),
                            sort_ms,
                        })
                    };
                    b.store(false, Ordering::Release);
                    if done_tx.send(msg).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Self {
            jobs,
            results,
            busy,
            durations,
            skipped: AtomicU64::new(0),
            late,
            accepted: AtomicU64::new(0),
            seq: AtomicU64::new(0),
            last_ms: Mutex::new(0.0),
            handle,
        }
    }

    /// Ask for a sort from `pose`. Returns false, and queues nothing, while the
    /// worker is still busy with the previous request.
    pub fn request(&self, pose: Pose) -> bool {
        if self.busy.swap(true, Ordering::AcqRel) {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        if self
            .jobs
            .send(Job {
                seq,
                pose,
                asked: Instant::now(),
            })
            .is_err()
        {
            self.busy.store(false, Ordering::Release);
            return false;
        }
        true
    }

    /// The newest finished order, if one is waiting. Late sorts were already
    /// dropped by the worker. Never blocks.
    pub fn poll(&self) -> Option<SortedOrder> {
        let mut newest = None;
        while let Ok(done) = self.results.try_recv() {
            if let Done::Order(o) = done {
                self.accepted.fetch_add(1, Ordering::Relaxed);
                *self.last_ms.lock().unwrap_or_else(|e| e.into_inner()) = o.sort_ms;
                newest = Some(o);
            }
        }
        newest
    }

    /// True while a request is in flight.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// Counters since creation.
    pub fn stats(&self) -> SortStats {
        let mut ds = self
            .durations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        ds.sort_by(|a, b| a.total_cmp(b));
        let p95_ms = if ds.is_empty() {
            0.0
        } else {
            ds[(ds.len() - 1).min((0.95 * ds.len() as f32) as usize)]
        };
        SortStats {
            accepted: self.accepted.load(Ordering::Relaxed),
            skipped_busy: self.skipped.load(Ordering::Relaxed),
            dropped_late: self.late.load(Ordering::Relaxed),
            last_ms: *self.last_ms.lock().unwrap_or_else(|e| e.into_inner()),
            p95_ms,
        }
    }
}

impl Drop for SplatSorter {
    fn drop(&mut self) {
        // Closing the job channel ends the worker's loop.
        let (dead, _) = channel::<Job>();
        self.jobs = dead;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// The render-thread side of the sorter, as one pure object: the cadence rule
/// ([`PoseTrigger`]) in front of the worker ([`SplatSorter`]), clocked by the
/// caller. [`SortDriver::update`] never blocks. It also repairs the one hole in
/// "drop a late sort": the trigger reference moves when a sort is *asked for*,
/// so a sort dropped as late would leave a still head holding a stale order
/// indefinitely. A late drop therefore resets the trigger, and the next frame
/// asks again.
pub struct SplatDriver {
    sorter: SplatSorter,
    trigger: PoseTrigger,
    late_seen: u64,
}

impl SplatDriver {
    pub fn new(sorter: SplatSorter) -> Self {
        Self {
            sorter,
            trigger: PoseTrigger::new(),
            late_seen: 0,
        }
    }

    /// One frame: ask for a sort if the rule says so and the worker is free, and
    /// return the newest finished order, if any.
    pub fn update(&mut self, pose: &Pose, scale: f32, now: f32) -> Option<SortedOrder> {
        if self.trigger.should_sort(pose, scale, now) && self.sorter.request(*pose) {
            self.trigger.mark(pose, now);
        }
        let order = self.sorter.poll();
        let late = self.sorter.stats().dropped_late;
        if late != self.late_seen {
            self.late_seen = late;
            if order.is_none() {
                self.trigger.reset();
            }
        }
        order
    }

    /// The worker's counters.
    pub fn stats(&self) -> SortStats {
        self.sorter.stats()
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
use godot::prelude::*;

/// Godot face of the sorter: `SplatSorter.new()`, `set_positions(...)`, then
/// each frame `update(head_pos, head_forward, content_to_world, now_sec)` and,
/// when it returns true, `take_index_bytes()` for the `FORMAT_RF` index texture.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(base = RefCounted, rename = SplatSorter)]
pub struct SplatSorterNode {
    base: Base<RefCounted>,
    driver: Option<SplatDriver>,
    count: usize,
    pending: Option<SortedOrder>,
}

#[cfg(not(test))]
#[godot_api]
impl IRefCounted for SplatSorterNode {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            base,
            driver: None,
            count: 0,
            pending: None,
        }
    }
}

#[cfg(not(test))]
#[godot_api]
impl SplatSorterNode {
    /// Start a worker for `positions` (xyz a splat, the volume's local frame).
    /// Replaces any earlier set; the next `update` sorts at once.
    #[func]
    fn set_positions(&mut self, positions: PackedFloat32Array) {
        self.count = positions.len() / 3;
        self.driver = Some(SplatDriver::new(SplatSorter::new(Arc::new(
            positions.to_vec(),
        ))));
        self.pending = None;
    }

    /// Splats the sorter holds.
    #[func]
    fn splat_count(&self) -> i64 {
        self.count as i64
    }

    /// Call every frame with the head pose in world space and the volume's
    /// content-to-world transform. Moves the pose into the splat frame, asks for
    /// a sort when the 5 cm / 2 degree / 30 Hz rule says so, and collects a
    /// finished one. Returns true when a new order is ready for
    /// `take_index_bytes`. Never blocks.
    #[func]
    fn update(
        &mut self,
        head_pos: Vector3,
        head_forward: Vector3,
        content_to_world: Transform3D,
        now_sec: f64,
    ) -> bool {
        let Some(driver) = self.driver.as_mut() else {
            return false;
        };
        let inv = content_to_world.affine_inverse();
        let local_pos = inv * head_pos;
        let fwd = (inv.basis * head_forward)
            .try_normalized()
            .unwrap_or(Vector3::new(0.0, 0.0, -1.0));
        let scale = content_to_world.basis.col_a().length();
        let pose = Pose {
            pos: [local_pos.x, local_pos.y, local_pos.z],
            fwd: [fwd.x, fwd.y, fwd.z],
        };
        if let Some(o) = driver.update(&pose, scale, now_sec as f32) {
            self.pending = Some(o);
        }
        self.pending.is_some()
    }

    /// The `FORMAT_RF` index texture bytes of the order `update` announced
    /// (4 bytes a splat, rows of 1024 texels), or empty when none is waiting.
    #[func]
    fn take_index_bytes(&mut self) -> PackedByteArray {
        match self.pending.take() {
            Some(o) => PackedByteArray::from(o.texels.as_slice()),
            None => PackedByteArray::new(),
        }
    }

    /// Index texture height in rows for the current splat set.
    #[func]
    fn index_height(&self) -> i64 {
        index_texture_height(self.count) as i64
    }

    /// {accepted, skipped_busy, dropped_late, last_ms, p95_ms}.
    #[func]
    fn stats(&self) -> Dictionary {
        let s = self.driver.as_ref().map(|d| d.stats()).unwrap_or_default();
        let mut d = Dictionary::new();
        d.set("accepted", s.accepted as i64);
        d.set("skipped_busy", s.skipped_busy as i64);
        d.set("dropped_late", s.dropped_late as i64);
        d.set("last_ms", s.last_ms as f64);
        d.set("p95_ms", s.p95_ms as f64);
        d
    }

    /// Stop the worker and forget the set.
    #[func]
    fn clear(&mut self) {
        self.driver = None;
        self.count = 0;
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny deterministic generator (xorshift32); the tests need no `rand`.
    struct Rng(u32);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            (self.0 >> 8) as f32 / (1u32 << 24) as f32
        }
    }

    fn cloud(n: usize, seed: u32, extent: f32) -> Vec<f32> {
        let mut r = Rng(seed);
        (0..n * 3).map(|_| (r.next() - 0.5) * extent).collect()
    }

    const FWD: [f32; 3] = [0.0, 0.0, -1.0];

    fn depth(p: &[f32], i: u32, eye: [f32; 3], fwd: [f32; 3]) -> f32 {
        depth_of(&p[i as usize * 3..i as usize * 3 + 3], eye, fwd)
    }

    fn sorted(p: &[f32], eye: [f32; 3], fwd: [f32; 3]) -> Vec<u32> {
        let (mut o, mut s) = (Vec::new(), SortScratch::default());
        sort_back_to_front(p, eye, fwd, &mut o, &mut s);
        o
    }

    #[test]
    fn it_orders_well_separated_splats_far_to_near_exactly() {
        // five splats on the view axis at known distances, shuffled
        let zs = [-2.0f32, -7.0, -0.5, -4.0, -1.0];
        let p: Vec<f32> = zs.iter().flat_map(|&z| [0.0, 0.0, z]).collect();
        assert_eq!(
            sorted(&p, [0.0; 3], FWD),
            vec![1, 3, 0, 4, 2],
            "farthest (-7) first"
        );
    }

    #[test]
    fn it_returns_a_permutation_in_non_increasing_depth_up_to_the_quantum() {
        let p = cloud(20_000, 7, 10.0);
        let eye = [0.3, -0.2, 4.0];
        let fwd = [0.6, 0.0, -0.8];
        let o = sorted(&p, eye, fwd);
        assert_eq!(o.len(), 20_000);
        let mut seen = vec![false; o.len()];
        for &i in &o {
            assert!(!seen[i as usize], "{i} appears twice");
            seen[i as usize] = true;
        }
        let ds: Vec<f32> = (0..o.len() as u32)
            .map(|i| depth(&p, i, eye, fwd))
            .collect();
        let (lo, hi) = ds
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &d| (a.min(d), b.max(d)));
        let quantum = (hi - lo) / 65535.0;
        for w in o.windows(2) {
            assert!(
                ds[w[0] as usize] >= ds[w[1] as usize] - quantum * 1.001,
                "{} then {} is not far to near",
                ds[w[0] as usize],
                ds[w[1] as usize]
            );
        }
        assert!(ds[o[0] as usize] > ds[o[o.len() - 1] as usize]);
    }

    #[test]
    fn equal_depths_keep_index_order_and_the_sort_is_deterministic() {
        // every splat on one plane perpendicular to the view axis
        let p: Vec<f32> = (0..50).flat_map(|i| [i as f32, 0.0, -3.0]).collect();
        assert_eq!(sorted(&p, [0.0; 3], FWD), (0..50).collect::<Vec<u32>>());
        let q = cloud(5_000, 3, 8.0);
        assert_eq!(
            sorted(&q, [1.0, 1.0, 1.0], FWD),
            sorted(&q, [1.0, 1.0, 1.0], FWD)
        );
    }

    #[test]
    fn it_handles_empty_single_and_non_finite_input() {
        assert!(sorted(&[], [0.0; 3], FWD).is_empty());
        assert_eq!(sorted(&[1.0, 2.0, 3.0], [0.0; 3], FWD), vec![0]);
        // a NaN position sorts as the farthest; the finite ones keep their order
        let p = [0.0, 0.0, -1.0, f32::NAN, 0.0, 0.0, 0.0, 0.0, -5.0];
        assert_eq!(sorted(&p, [0.0; 3], FWD), vec![1, 2, 0]);
    }

    #[test]
    fn the_order_flips_when_the_head_turns_to_face_the_other_way() {
        let p: Vec<f32> = [-4.0f32, -1.0, 2.0, 6.0]
            .iter()
            .flat_map(|&z| [0.0, 0.0, z])
            .collect();
        assert_eq!(
            sorted(&p, [0.0; 3], [0.0, 0.0, -1.0]),
            vec![0, 1, 2, 3],
            "facing -Z, z = -4 is farthest"
        );
        assert_eq!(
            sorted(&p, [0.0; 3], [0.0, 0.0, 1.0]),
            vec![3, 2, 1, 0],
            "facing +Z, z = 6 is farthest"
        );
    }

    #[test]
    fn the_index_texture_is_four_bytes_a_splat_padded_to_whole_rows() {
        let order: Vec<u32> = vec![3, 1, 2, 0];
        let b = index_texture_bytes(&order);
        assert_eq!(b.len(), INDEX_TEX_WIDTH * 4, "one padded row");
        for (i, &o) in order.iter().enumerate() {
            let v = f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
            assert_eq!(v, o as f32);
        }
        assert!(b[16..].iter().all(|&x| x == 0), "padding is zero");
        let n = 100_000;
        let big: Vec<u32> = (0..n as u32).collect();
        assert_eq!(
            index_texture_bytes(&big).len(),
            index_texture_height(n) * INDEX_TEX_WIDTH * 4
        );
        assert_eq!(index_texture_height(n), 98);
        assert!(index_texture_bytes(&big).len() >= n * 4);
        assert_eq!(index_texture_bytes(&[]).len(), INDEX_TEX_WIDTH * 4);
    }

    // ── cadence ──

    /// Run the trigger over a scripted head path sampled at `hz` for `secs`,
    /// asking for a sort whenever it says to; returns the times (s) of the sorts.
    fn script(hz: f32, secs: f32, scale: f32, pose_at: impl Fn(f32) -> Pose) -> Vec<f32> {
        let mut t = PoseTrigger::new();
        let mut sorts = Vec::new();
        for f in 0..(secs * hz) as usize {
            let now = f as f32 / hz;
            let pose = pose_at(now);
            if t.should_sort(&pose, scale, now) {
                t.mark(&pose, now);
                sorts.push(now);
            }
        }
        sorts
    }

    fn walking(speed: f32) -> impl Fn(f32) -> Pose {
        move |t| Pose {
            pos: [speed * t, 1.6, 0.0],
            fwd: [0.0, 0.0, -1.0],
        }
    }

    fn turning(deg_per_s: f32) -> impl Fn(f32) -> Pose {
        move |t| {
            let a = (deg_per_s * t).to_radians();
            Pose {
                pos: [0.0, 1.6, 0.0],
                fwd: [a.sin(), 0.0, -a.cos()],
            }
        }
    }

    fn rate(sorts: &[f32], secs: f32) -> f32 {
        // the first sort is the initial one, not motion
        (sorts.len() - 1) as f32 / secs
    }

    #[test]
    fn the_first_pose_always_sorts_and_a_still_head_never_sorts_again() {
        let s = script(90.0, 10.0, 1.0, |_| Pose {
            pos: [0.0, 1.6, 0.0],
            fwd: FWD,
        });
        assert_eq!(s, vec![0.0]);
    }

    #[test]
    fn five_centimetres_of_travel_triggers_a_sort() {
        // 0.5 m/s: 5 cm every 0.1 s = 10 sorts a second
        let s = script(90.0, 10.0, 1.0, walking(0.5));
        assert!(
            (rate(&s, 10.0) - 10.0).abs() < 1.5,
            "walking 0.5 m/s: {} sorts/s",
            rate(&s, 10.0)
        );
        // 4 cm never triggers
        let s = script(90.0, 10.0, 1.0, |t| Pose {
            pos: [0.04 * (t * 6.0).sin().abs(), 1.6, 0.0],
            fwd: FWD,
        });
        assert_eq!(s.len(), 1, "an oscillation inside 5 cm sorts once");
        // slow drift accumulates against the last sort and triggers at 5 cm
        let s = script(90.0, 10.0, 1.0, walking(0.01));
        assert_eq!(
            s.len(),
            1 + 1,
            "1 cm/s over 10 s crosses 5 cm once, at 5 s: {s:?}"
        );
        assert!((s[1] - 5.0).abs() < 0.05);
    }

    #[test]
    fn two_degrees_of_turn_triggers_a_sort() {
        // 20 deg/s: 2 degrees every 0.1 s
        let s = script(90.0, 10.0, 1.0, turning(20.0));
        assert!(
            (rate(&s, 10.0) - 10.0).abs() < 1.5,
            "turning 20 deg/s: {} sorts/s",
            rate(&s, 10.0)
        );
        // 1.5 degrees of wobble never does
        let s = script(90.0, 10.0, 1.0, |t| {
            let a = (1.5 * (t * 5.0).sin()).to_radians();
            Pose {
                pos: [0.0, 1.6, 0.0],
                fwd: [a.sin(), 0.0, -a.cos()],
            }
        });
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn sorts_are_capped_at_thirty_a_second_however_fast_the_head_moves() {
        for (label, sorts) in [
            ("sprint 4 m/s", script(90.0, 10.0, 1.0, walking(4.0))),
            ("whip 300 deg/s", script(90.0, 10.0, 1.0, turning(300.0))),
        ] {
            let r = rate(&sorts, 10.0);
            assert!(r <= MAX_SORT_HZ + 0.2, "{label}: {r} sorts/s");
            assert!(
                r >= MAX_SORT_HZ - 1.0,
                "{label}: the cap is reached, not undershot ({r})"
            );
            for w in sorts.windows(2) {
                assert!(
                    w[1] - w[0] >= 1.0 / MAX_SORT_HZ - 1.0e-4,
                    "{label}: {} then {}",
                    w[0],
                    w[1]
                );
            }
        }
        // a 144 Hz frame clock does not change the cap
        let r = rate(&script(144.0, 10.0, 1.0, walking(4.0)), 10.0);
        assert!(r <= MAX_SORT_HZ + 0.2, "144 Hz: {r}");
    }

    #[test]
    fn the_distance_threshold_is_measured_in_world_metres() {
        // a volume fitted at 0.25: 5 cm of world is 0.2 local units, and a head
        // moving 1.0 local unit/s is moving 0.25 m/s in the world
        let s = script(90.0, 5.0, 0.25, |t| Pose {
            pos: [t, 0.0, 0.0],
            fwd: FWD,
        });
        // 5 cm every 0.2 s = 5 a second
        assert!(
            (rate(&s, 5.0) - 5.0).abs() < 0.8,
            "{} sorts/s",
            rate(&s, 5.0)
        );
    }

    #[test]
    fn a_reset_makes_the_next_pose_sort_at_once() {
        let mut t = PoseTrigger::new();
        let p = Pose {
            pos: [0.0; 3],
            fwd: FWD,
        };
        t.mark(&p, 0.0);
        assert!(!t.should_sort(&p, 1.0, 5.0));
        t.reset();
        assert!(t.should_sort(&p, 1.0, 5.0));
    }

    // ── worker ──

    fn wait_for(s: &SplatSorter, limit: Duration) -> Option<SortedOrder> {
        let t0 = Instant::now();
        while t0.elapsed() < limit {
            if let Some(o) = s.poll() {
                return Some(o);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    #[test]
    fn the_worker_sorts_off_the_calling_thread_and_matches_the_pure_sort() {
        let p = Arc::new(cloud(30_000, 11, 6.0));
        let s = SplatSorter::new(p.clone());
        let pose = Pose {
            pos: [0.5, 1.0, 3.0],
            fwd: [0.0, 0.0, -1.0],
        };
        assert!(s.request(pose));
        let o = wait_for(&s, Duration::from_secs(5)).expect("a sort arrives");
        assert_eq!(o.order, sorted(&p, pose.pos, pose.fwd));
        assert_eq!(o.texels, index_texture_bytes(&o.order));
        assert_eq!(o.seq, 1);
        assert_eq!(s.stats().accepted, 1);
        assert!(!s.is_busy());
    }

    #[test]
    fn a_request_while_the_worker_is_busy_is_dropped_not_queued() {
        let p = Arc::new(cloud(1_000, 5, 4.0));
        let s = SplatSorter::with_options(p, Duration::from_secs(5), Duration::from_millis(150));
        let pose = Pose {
            pos: [0.0; 3],
            fwd: FWD,
        };
        assert!(s.request(pose));
        assert!(!s.request(pose), "busy: refused");
        assert!(!s.request(pose));
        assert_eq!(s.stats().skipped_busy, 2);
        let first = wait_for(&s, Duration::from_secs(5)).expect("the first request finishes");
        assert_eq!(first.seq, 1);
        // Nothing was queued behind it: no second result ever arrives.
        std::thread::sleep(Duration::from_millis(400));
        assert!(s.poll().is_none(), "dropped requests leave nothing queued");
        assert_eq!(s.stats().accepted, 1);
        // and the worker takes a new request once idle
        assert!(s.request(pose));
    }

    #[test]
    fn a_late_sort_is_dropped_and_the_previous_order_stands() {
        let p = Arc::new(cloud(1_000, 5, 4.0));
        // each sort takes 120 ms against a 30 ms bound
        let s = SplatSorter::with_options(p, Duration::from_millis(30), Duration::from_millis(120));
        let pose = Pose {
            pos: [0.0; 3],
            fwd: FWD,
        };
        assert!(s.request(pose));
        std::thread::sleep(Duration::from_millis(400));
        assert!(s.poll().is_none(), "a late result is never handed over");
        let st = s.stats();
        assert_eq!((st.accepted, st.dropped_late), (0, 1));
        assert!(!s.is_busy(), "a dropped sort frees the worker");
        // a prompt one is delivered
        let q = Arc::new(cloud(1_000, 5, 4.0));
        let s = SplatSorter::with_options(q, Duration::from_secs(5), Duration::ZERO);
        assert!(s.request(pose));
        assert!(wait_for(&s, Duration::from_secs(5)).is_some());
        assert_eq!(s.stats().dropped_late, 0);
    }

    #[test]
    fn a_dropped_late_sort_is_asked_for_again_so_a_still_head_is_not_left_stale() {
        // Every sort is late (120 ms of work against a 30 ms bound). The head
        // never moves after the first request. Without the repair the trigger
        // reference sits at the dropped request and nothing is ever asked again.
        let p = Arc::new(cloud(500, 5, 4.0));
        let sorter =
            SplatSorter::with_options(p, Duration::from_millis(30), Duration::from_millis(120));
        let mut d = SplatDriver::new(sorter);
        let pose = Pose {
            pos: [0.0; 3],
            fwd: FWD,
        };
        let t0 = Instant::now();
        let mut got = None;
        while t0.elapsed() < Duration::from_millis(900) {
            let now = t0.elapsed().as_secs_f32();
            if let Some(o) = d.update(&pose, 1.0, now) {
                got = Some(o);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(got.is_none(), "every one of them was late");
        let st = d.stats();
        assert!(
            st.dropped_late >= 3,
            "late sorts are asked for again: {st:?}"
        );
        // The same still head with a prompt worker asks exactly once.
        let p = Arc::new(cloud(500, 5, 4.0));
        let mut d = SplatDriver::new(SplatSorter::new(p));
        let t0 = Instant::now();
        let mut orders = 0;
        while t0.elapsed() < Duration::from_millis(300) {
            if d.update(&pose, 1.0, t0.elapsed().as_secs_f32()).is_some() {
                orders += 1;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(orders, 1, "a still head sorts once");
    }

    #[test]
    fn the_worker_thread_is_not_the_calling_thread_and_joins_on_drop() {
        let p = Arc::new(cloud(100, 1, 1.0));
        let s = SplatSorter::new(p);
        assert!(s
            .handle
            .as_ref()
            .is_some_and(|h| h.thread().id() != std::thread::current().id()));
        assert_eq!(
            s.handle.as_ref().unwrap().thread().name(),
            Some("splat-sort")
        );
        drop(s); // joins; a hang here fails the test by timeout
    }

    #[test]
    fn scripted_head_motion_drives_the_worker_at_the_capped_cadence() {
        // The whole path: trigger on the render side, request, pick up, with the
        // frame clock scripted at 90 Hz for 4 s of a 3 m/s walk.
        let p = Arc::new(cloud(20_000, 9, 8.0));
        let s = SplatSorter::new(p);
        let mut trig = PoseTrigger::new();
        let (mut asked, mut got) = (0u32, 0u32);
        let t0 = Instant::now();
        let hz = 90.0;
        for f in 0..(4.0 * hz) as usize {
            let now = f as f32 / hz;
            let pose = Pose {
                pos: [3.0 * now, 1.6, 0.0],
                fwd: FWD,
            };
            if trig.should_sort(&pose, 1.0, now) && s.request(pose) {
                trig.mark(&pose, now);
                asked += 1;
            }
            if s.poll().is_some() {
                got += 1;
            }
            // pace the loop in real time so the worker keeps up as it would
            let due = Duration::from_secs_f32((f as f32 + 1.0) / hz);
            if let Some(wait) = due.checked_sub(t0.elapsed()) {
                std::thread::sleep(wait);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        got += u32::from(s.poll().is_some());
        assert!(
            asked as f32 / 4.0 <= MAX_SORT_HZ + 0.5,
            "{asked} sorts asked in 4 s"
        );
        assert!(asked >= 100, "{asked}: the cap is reached");
        assert!(got + 2 >= asked, "asked {asked}, received {got}");
    }

    /// The sort of 100k splats (ADR-2170 acceptance: <= 4 ms p95 on the worker).
    /// A smoke test on whatever CPU runs it: it prints the numbers (run with
    /// `--nocapture`) and asserts only a loose ceiling, because the acceptance
    /// figure is the Quest's, not this machine's.
    #[test]
    fn a_hundred_thousand_splat_sort_timing_smoke() {
        let p = cloud(100_000, 21, 12.0);
        let (mut o, mut s) = (Vec::new(), SortScratch::default());
        let mut ms = Vec::new();
        for i in 0..60 {
            let a = (i as f32 * 3.0).to_radians();
            let t0 = Instant::now();
            sort_back_to_front(
                &p,
                [0.0, 1.6, 0.0],
                [a.sin(), 0.0, -a.cos()],
                &mut o,
                &mut s,
            );
            ms.push(t0.elapsed().as_secs_f32() * 1000.0);
        }
        ms.sort_by(|a, b| a.total_cmp(b));
        let p95 = ms[(ms.len() as f32 * 0.95) as usize];
        println!(
            "splat sort 100k (this CPU, not the HP or a Quest): min {:.2} ms, median {:.2} ms, p95 {:.2} ms, max {:.2} ms over {} runs",
            ms[0], ms[ms.len() / 2], p95, ms[ms.len() - 1], ms.len()
        );
        assert_eq!(o.len(), 100_000);
        assert!(p95 < 50.0, "p95 {p95} ms is far beyond any plausible CPU");
    }
}
