//! The CUDA engine: a thin cust 0.3.2 driver over `visionclaw_unified` PTX.
//!
//! One step:
//!
//! 1. Host: containment, aggregate edges and cohesion from the shared model at
//!    the current positions, plus the far field. The far field is computed in
//!    the background from the previous step's positions: each step queues the
//!    job for the next one, which joins it, so the result is deterministic.
//!    All of it is staged as per-body cohesion targets `pos + F_host`.
//! 2. Host, side by side with step 1: the uniform grid, in exactly the encoding
//!    `build_grid_kernel` → stable sort → `compute_cell_bounds_kernel`
//!    produces (a device test checks it bit for bit). It replaces two kernels,
//!    two memsets and a thrust sort. That sort synchronises and allocates on
//!    every call, and each synchronisation can wait a whole time slice on a
//!    shared GPU.
//! 3. Asynchronous upload of targets and grid from page-locked staging.
//! 4. `force_pass_kernel` (grid repulsion, CSR springs, gravity; all ontology
//!    inputs null; `compute_mask` under a focus), then `cluster_cohesion_kernel`
//!    (one "cluster" per body, target `pos + F_host`, strength 1, which adds
//!    `F_host` exactly), then `integrate_pass_kernel` (`pinned_mask` freezes
//!    bodies outside the focus).
//! 5. Asynchronous download of positions and velocities into page-locked
//!    staging, then an event.
//!
//! [`LayoutEngine::step`] returns as soon as its last step is queued. The next
//! step waits on that event before it computes host terms, so the physics is
//! exact (no stale positions), and the caller's own work and sleep overlap
//! the device. [`LayoutEngine::positions`] waits for the queued step only when
//! it is called.

use std::cell::OnceCell;
use std::time::Instant;

use codescape_model::{GraphDelta, GraphSnapshot};
use cust::context::{Context, CurrentContext};
use cust::device::Device;
use cust::event::{Event, EventFlags};
use cust::launch;
use cust::memory::{
    AsyncCopyDestination, CopyDestination, DeviceBuffer, DeviceCopy, DevicePointer, LockedBuffer,
};
use cust::module::Module;
use cust::stream::{Stream, StreamFlags};
use visionclaw_gpu::ptx_loader::{downgrade_ptx_isa_if_needed, load_ptx_module_sync, PTXModule};

use crate::abi::{check_provenance, check_ptx_abi, ManifestEntry, UNIFIED_MODULE};
use crate::cpu::{check_delta, state_after_delta, State, REPULSION_SOFTENING};
use crate::model::{far_field, FarField};
use crate::octree::Repulsion;
use crate::{LayoutEngine, LayoutError, LayoutParams, PositionSet, StepStats};

/// Repulsion cut-off as a fraction of [`LayoutParams::shell_radius`]. The CPU
/// engine's Barnes-Hut repulsion has unlimited range; the uniform grid needs a
/// cut-off, and the host adds per-file monopoles for everything beyond it.
/// On a 2k-symbol repository graph, k-NN overlap against the CPU oracle was
/// 0.35 (symbols) / 0.89 (files) at `shell_radius / 6` and 0.75 / 0.97 at
/// `shell_radius / 3` (both measured before the two engines shared an
/// integrator). With the shared integrator and θ = 0.5 on the CPU,
/// `shell_radius / 3` gives 0.87 / 1.00, above the CPU engine against itself
/// with a 10% smaller time step (0.73 / 0.98).
pub const CUTOFF_PER_SHELL_RADIUS: f32 = 1.0 / 3.0;
const BLOCK: u32 = 256;
/// Grid cells allowed per body, with a floor of [`MIN_GRID_CELLS`]. When the
/// layout's bounding box would need more, the cell grows past the cut-off.
/// That costs extra candidate pairs but never misses one, and it stops a
/// single far-out body from inflating every upload.
const GRID_CELLS_PER_BODY: usize = 2;
const MIN_GRID_CELLS: usize = 4096;
/// Pins the device ordinal for [`CudaEngine::new`]. When unset, the engine
/// picks the visible device with the most free memory, so a Codescape
/// container that can see several cards lands on an idle one instead of
/// sharing device 0 with the VisionClaw server.
pub const DEVICE_ENV: &str = "CODESCAPE_CUDA_DEVICE";

/// The ordinal with the most free memory; ties go to the lower ordinal.
/// Pure so it can be tested without a device.
pub(crate) fn best_by_free(free_by_ordinal: &[(u32, usize)]) -> Option<u32> {
    free_by_ordinal
        .iter()
        .copied()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(ordinal, _)| ordinal)
}

const ENABLE_REPULSION: u32 = 1 << 0;
const ENABLE_SPRINGS: u32 = 1 << 1;

/// Mirror of the device `SimParams` (`visionclaw_unified.cu:22`), 53 four-byte fields.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, cust::DeviceCopy)]
struct SimParams {
    dt: f32,
    damping: f32,
    warmup_iterations: u32,
    cooling_rate: f32,
    spring_k: f32,
    rest_length: f32,
    repel_k: f32,
    repulsion_cutoff: f32,
    repulsion_softening_epsilon: f32,
    center_gravity_k: f32,
    max_force: f32,
    max_velocity: f32,
    grid_cell_size: f32,
    feature_flags: u32,
    seed: u32,
    iteration: i32,
    separation_radius: f32,
    cluster_strength: f32,
    alignment_strength: f32,
    temperature: f32,
    viewport_bounds: f32,
    sssp_alpha: f32,
    boundary_damping: f32,
    constraint_ramp_frames: u32,
    constraint_max_force_per_node: f32,
    stability_threshold: f32,
    min_velocity_threshold: f32,
    world_bounds_min: f32,
    world_bounds_max: f32,
    cell_size_lod: f32,
    k_neighbors_max: u32,
    anomaly_detection_radius: f32,
    learning_rate_default: f32,
    norm_delta_cap: f32,
    position_constraint_attraction: f32,
    lof_score_min: f32,
    lof_score_max: f32,
    weight_precision_multiplier: f32,
    gravity: f32,
    lin_log_mode: u32,
    scaling_ratio: f32,
    adaptive_speed: u32,
    global_speed: f32,
    dag_bias_k: f32,
    dag_level_distance: f32,
    layout_mode: u32,
    plane_bias_k: f32,
    plane_spacing: f32,
    radial_center_x: f32,
    radial_center_y: f32,
    radial_center_z: f32,
    layer_bias_k: f32,
    layer_spacing: f32,
}

const _: () = assert!(std::mem::size_of::<SimParams>() == crate::abi::SIM_PARAMS_BYTES);

impl SimParams {
    /// Every term Codescape does not use is off: no warm-up ramp, no
    /// temperature, no viewport bounds, no stability gate (so the plain
    /// `force_pass_kernel` contract applies), linear springs with zero rest length.
    fn for_layout(p: &LayoutParams, cutoff: f32) -> Self {
        SimParams {
            dt: p.dt,
            damping: p.damping,
            spring_k: p.spring_k,
            repel_k: p.repulsion,
            repulsion_cutoff: cutoff,
            repulsion_softening_epsilon: REPULSION_SOFTENING,
            max_force: p.max_force(),
            max_velocity: p.max_velocity,
            grid_cell_size: cutoff,
            feature_flags: ENABLE_REPULSION | ENABLE_SPRINGS,
            seed: p.seed as u32,
            boundary_damping: 1.0,
            gravity: p.gravity,
            ..Default::default()
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, cust::DeviceCopy)]
struct Int3 {
    x: i32,
    y: i32,
    z: i32,
}

fn dev<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> LayoutError + '_ {
    move |e| LayoutError::Device(format!("{what}: {e}"))
}

fn upload<T: DeviceCopy>(data: &[T], fill: T) -> Result<DeviceBuffer<T>, LayoutError> {
    if data.is_empty() {
        DeviceBuffer::from_slice(&[fill]).map_err(dev("alloc"))
    } else {
        DeviceBuffer::from_slice(data).map_err(dev("alloc"))
    }
}

fn locked<T: DeviceCopy + Default>(n: usize) -> Result<LockedBuffer<T>, LayoutError> {
    LockedBuffer::new(&T::default(), n.max(1)).map_err(dev("alloc page-locked"))
}

/// Geometry of one step's uniform grid.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GridGeom {
    min: [f32; 3],
    cell: f32,
    dims: [i32; 3],
    cells: usize,
}

impl GridGeom {
    /// The bounding box of `pos`, padded by one cut-off, in cells of the
    /// cut-off. The cell grows by 1.25× until there are at most `max_cells`.
    fn around(pos: &[[f32; 3]], cutoff: f32, max_cells: usize) -> Self {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        for k in 0..3 {
            lo[k] -= cutoff;
            hi[k] += cutoff;
        }
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let dims_for = |cell: f32| ext.map(|e| ((e / cell).ceil() as i64).clamp(1, 1 << 20));
        let mut cell = cutoff;
        let mut d = dims_for(cell);
        while (d[0] * d[1] * d[2]) as usize > max_cells.max(1) {
            cell *= 1.25;
            d = dims_for(cell);
        }
        GridGeom {
            min: lo,
            cell,
            dims: d.map(|x| x as i32),
            cells: (d[0] * d[1] * d[2]) as usize,
        }
    }

    fn dims(&self) -> Int3 {
        Int3 {
            x: self.dims[0],
            y: self.dims[1],
            z: self.dims[2],
        }
    }

    /// The key `build_grid_kernel` computes for a body at `p`.
    fn key(&self, p: [f32; 3]) -> i32 {
        let inv = 1.0 / self.cell;
        let c: [i32; 3] =
            [0, 1, 2].map(|k| (((p[k] - self.min[k]) * inv) as i32).clamp(0, self.dims[k] - 1));
        (c[2] * self.dims[1] * self.dims[0] + c[1] * self.dims[0] + c[0])
            .clamp(0, self.cells as i32 - 1)
    }
}

/// The grid arrays the device pipeline would build: the key of every body;
/// the bodies sorted by key, stably (ascending index within a cell); and the
/// `start..end` slice of that order for each cell, with `0..0` for an empty
/// cell (what `compute_cell_bounds_kernel` leaves after its memsets).
fn build_grid(
    pos: &[[f32; 3]],
    g: &GridGeom,
    keys: &mut [i32],
    sorted: &mut [i32],
    start: &mut [i32],
    end: &mut [i32],
) {
    let (start, end) = (&mut start[..g.cells], &mut end[..g.cells]);
    end.fill(0);
    for (i, &p) in pos.iter().enumerate() {
        let k = g.key(p);
        keys[i] = k;
        end[k as usize] += 1;
    }
    let mut acc = 0i32;
    for (s, e) in start.iter_mut().zip(end.iter_mut()) {
        *s = acc;
        acc += *e;
        *e = *s; // now a placement cursor
    }
    for (i, &k) in keys[..pos.len()].iter().enumerate() {
        let slot = &mut end[k as usize];
        sorted[*slot as usize] = i as i32;
        *slot += 1;
    }
    for (s, e) in start.iter_mut().zip(end.iter_mut()) {
        if *s == *e {
            (*s, *e) = (0, 0);
        }
    }
}

/// Device-resident copy of one loaded model, with its page-locked staging.
struct Buffers {
    n: usize,
    cell_cap: usize,
    pos: [[DeviceBuffer<f32>; 3]; 2],
    vel: [[DeviceBuffer<f32>; 3]; 2],
    /// Which of the two `pos`/`vel` sets is current.
    cur: usize,
    force: [DeviceBuffer<f32>; 3],
    target: [DeviceBuffer<f32>; 3],
    identity: DeviceBuffer<i32>,
    cell_keys: DeviceBuffer<i32>,
    sorted_idx: DeviceBuffer<i32>,
    cell_start: DeviceBuffer<i32>,
    cell_end: DeviceBuffer<i32>,
    row: DeviceBuffer<i32>,
    col: DeviceBuffer<i32>,
    weight: DeviceBuffer<f32>,
    mask: DeviceBuffer<i32>,
    mask_len: usize,
    pinned: DeviceBuffer<i32>,
    up_target: [LockedBuffer<f32>; 3],
    up_keys: LockedBuffer<i32>,
    up_sorted: LockedBuffer<i32>,
    up_start: LockedBuffer<i32>,
    up_end: LockedBuffer<i32>,
    down_pos: [LockedBuffer<f32>; 3],
    down_vel: [LockedBuffer<f32>; 3],
    /// Ordinary host memory the grid and targets are built in. Reading and
    /// updating page-locked memory element by element measured about 10×
    /// slower here, so staging only ever sees whole-slice copies.
    scratch_f: Vec<f32>,
    scratch_keys: Vec<i32>,
    scratch_sorted: Vec<i32>,
    scratch_start: Vec<i32>,
    scratch_end: Vec<i32>,
}

fn split(v: &[[f32; 3]], axis: usize) -> Vec<f32> {
    v.iter().map(|p| p[axis]).collect()
}

impl Buffers {
    fn new(s: &State) -> Result<Self, LayoutError> {
        let n = s.model.n_bodies();
        let cell_cap = (GRID_CELLS_PER_BODY * n).max(MIN_GRID_CELLS);
        let axes = |v: &[[f32; 3]]| -> Result<[DeviceBuffer<f32>; 3], LayoutError> {
            Ok([
                upload(&split(v, 0), 0.0)?,
                upload(&split(v, 1), 0.0)?,
                upload(&split(v, 2), 0.0)?,
            ])
        };
        let zeros = vec![[0.0f32; 3]; n];
        let ints = |len: usize| upload(&vec![0i32; len], 0);
        let row: Vec<i32> = s.model.row.iter().map(|&r| r as i32).collect();
        let col: Vec<i32> = s.model.col.iter().map(|&c| c as i32).collect();
        let mut b = Buffers {
            n,
            cell_cap,
            pos: [axes(&s.pos)?, axes(&s.pos)?],
            vel: [axes(&s.vel)?, axes(&s.vel)?],
            cur: 0,
            force: axes(&zeros)?,
            target: axes(&zeros)?,
            identity: upload(&(0..n as i32).collect::<Vec<_>>(), 0)?,
            cell_keys: ints(n)?,
            sorted_idx: ints(n)?,
            cell_start: ints(cell_cap)?,
            cell_end: ints(cell_cap)?,
            row: upload(&row, 0)?,
            col: upload(&col, 0)?,
            weight: upload(&s.model.weight, 0.0)?,
            mask: ints(n)?,
            mask_len: 0,
            pinned: ints(n)?,
            up_target: [locked(n)?, locked(n)?, locked(n)?],
            up_keys: locked(n)?,
            up_sorted: locked(n)?,
            up_start: locked(cell_cap)?,
            up_end: locked(cell_cap)?,
            down_pos: [locked(n)?, locked(n)?, locked(n)?],
            down_vel: [locked(n)?, locked(n)?, locked(n)?],
            scratch_f: vec![0.0; n],
            scratch_keys: vec![0; n],
            scratch_sorted: vec![0; n],
            scratch_start: vec![0; cell_cap],
            scratch_end: vec![0; cell_cap],
        };
        b.set_focus(s)?;
        Ok(b)
    }

    /// Upload the focus masks. The stream must be idle: these copies are synchronous.
    fn set_focus(&mut self, s: &State) -> Result<(), LayoutError> {
        match &s.active {
            None => self.mask_len = 0,
            Some(active) => {
                let mut pinned = vec![1i32; self.n];
                let mut mask: Vec<i32> = active.iter().map(|&b| b as i32).collect();
                for &b in active {
                    pinned[b as usize] = 0;
                }
                self.mask_len = mask.len();
                mask.resize(self.n.max(1), 0);
                pinned.resize(self.n.max(1), 1);
                self.mask.copy_from(&mask[..]).map_err(dev("upload mask"))?;
                self.pinned
                    .copy_from(&pinned[..])
                    .map_err(dev("upload pinned"))?;
            }
        }
        Ok(())
    }
}

/// Force layout on a CUDA device, driving VisionClaw's unified kernels.
///
/// Construction fails unless the PTX passes [`crate::abi::check_ptx_abi`] and
/// its build manifest says `provenance=compiled`. [`crate::select_engine`]
/// falls back to the CPU engine in that case.
///
/// Steps are pipelined (see [`StepStats`]): `energy` and [`LayoutEngine::settled`]
/// describe the steps that have finished, which lag the queued work by one
/// step. [`LayoutEngine::positions`] always reflects every queued step.
pub struct CudaEngine {
    params: LayoutParams,
    cutoff: f32,
    state: State,
    bufs: Option<Buffers>,
    cache: OnceCell<PositionSet>,
    device_name: String,
    artefact: ManifestEntry,
    /// The far field this step uses.
    far: FarField,
    /// The far field for the next step, being computed in the background from
    /// this step's input positions (see [`CudaEngine::launch_step`]).
    far_job: Option<std::sync::mpsc::Receiver<FarField>>,
    /// Around the queued step: before uploads, after uploads, after kernels, after downloads.
    events: [Event; 4],
    /// A step is queued whose results have not been ingested into `state`.
    in_flight: bool,
    last_energy: f32,
    // Field order is drop order: every device allocation above goes before the
    // module, the stream and finally the context that owns them. `Drop` drains
    // the stream first, so no queued kernel outlives its buffers.
    module: Module,
    stream: Stream,
    context: Context,
}

impl std::fmt::Debug for CudaEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CudaEngine")
            .field("device", &self.device_name)
            .field("bodies", &self.state.model.n_bodies())
            .field("in_flight", &self.in_flight)
            .finish_non_exhaustive()
    }
}

impl Drop for CudaEngine {
    fn drop(&mut self) {
        if self.bind().is_ok() {
            let _ = self.stream.synchronize();
        }
    }
}

/// Load the unified PTX and prove it is the fresh build this crate expects.
fn verified_ptx() -> Result<(String, ManifestEntry), LayoutError> {
    let module = PTXModule::VisionflowUnified;
    let artefact_path = module.compiled_ptx_path().ok_or_else(|| {
        LayoutError::AbiMismatch(
            "visionclaw-gpu baked no build-time PTX path: provenance cannot be proven".into(),
        )
    })?;
    let manifest_path =
        std::path::Path::new(artefact_path).with_file_name("ptx-build-manifest.txt");
    let manifest = std::fs::read_to_string(&manifest_path).ok();
    let entry = check_provenance(manifest.as_deref(), UNIFIED_MODULE)?;
    let on_disk = std::fs::read_to_string(artefact_path)
        .map_err(|e| LayoutError::PtxUnavailable(format!("{artefact_path}: {e}")))?;
    if visionclaw_gpu::ptx_policy::content_tag(on_disk.as_bytes()) != entry.rewritten {
        return Err(LayoutError::AbiMismatch(format!(
            "{artefact_path} does not match its build manifest tag {:016x}",
            entry.rewritten
        )));
    }
    let loaded = load_ptx_module_sync(module).map_err(LayoutError::PtxUnavailable)?;
    if loaded != downgrade_ptx_isa_if_needed(on_disk) {
        return Err(LayoutError::AbiMismatch("the PTX loader returned something other than the build artefact (a source-tree or runtime-compiled copy)".into()));
    }
    check_ptx_abi(&loaded)?;
    Ok((loaded, entry))
}

impl CudaEngine {
    /// Initialise CUDA on device `$CODESCAPE_CUDA_DEVICE` (default 0), verify
    /// and load the PTX. Nothing is loaded into the layout yet.
    pub fn new(params: LayoutParams) -> Result<Self, LayoutError> {
        let (ptx, artefact) = verified_ptx()?;
        cust::init(cust::CudaFlags::empty()).map_err(dev("cuInit"))?;
        let count = Device::num_devices().map_err(dev("device count"))?;
        if count == 0 {
            return Err(LayoutError::Device("no CUDA device visible".into()));
        }
        let pinned: Option<u32> = std::env::var(DEVICE_ENV).ok().and_then(|v| v.parse().ok());
        let ordinal: u32 = match pinned {
            Some(o) => o,
            None => {
                // Probe free memory on every visible device. Each probe needs a
                // context; it is dropped before the chosen one is created.
                let mut free_by_ordinal: Vec<(u32, usize)> = Vec::with_capacity(count as usize);
                for o in 0..count {
                    let d = Device::get_device(o).map_err(dev("get device"))?;
                    let probe = Context::new(d).map_err(dev("probe context"))?;
                    CurrentContext::set_current(&probe).map_err(dev("probe set context"))?;
                    let (free, _total) =
                        cust::memory::mem_get_info().map_err(dev("mem_get_info"))?;
                    free_by_ordinal.push((o, free));
                    drop(probe);
                }
                let chosen = best_by_free(&free_by_ordinal).unwrap_or(0);
                tracing::info!(
                    target: "codescape_layout",
                    ?free_by_ordinal,
                    chosen,
                    "cuda device chosen by free memory (pin with {DEVICE_ENV})"
                );
                chosen
            }
        };
        if ordinal >= count {
            return Err(LayoutError::Device(format!(
                "device {ordinal} requested, {count} present"
            )));
        }
        let device = Device::get_device(ordinal).map_err(dev("get device"))?;
        let device_name = device.name().map_err(dev("device name"))?;
        let context = Context::new(device).map_err(dev("context"))?;
        CurrentContext::set_current(&context).map_err(dev("set context"))?;
        let module = Module::from_ptx(&ptx, &[]).map_err(dev("load PTX module"))?;
        let stream = Stream::new(StreamFlags::NON_BLOCKING, None).map_err(dev("stream"))?;
        let event = || Event::new(EventFlags::DEFAULT).map_err(dev("event"));
        let cutoff = (params.shell_radius * CUTOFF_PER_SHELL_RADIUS).max(1.0);
        let engine = CudaEngine {
            params,
            cutoff,
            state: State::default(),
            bufs: None,
            cache: OnceCell::new(),
            device_name,
            artefact,
            far: FarField::default(),
            far_job: None,
            events: [event()?, event()?, event()?, event()?],
            in_flight: false,
            last_energy: 0.0,
            module,
            stream,
            context,
        };
        engine.upload_params()?;
        Ok(engine)
    }

    /// Device name, PTX artefact and repulsion cut-off, for health reports.
    pub fn describe(&self) -> String {
        format!(
            "{} ({} PTX isa {} tag {:016x}, provenance {}), repulsion cut-off {:.1}",
            self.device_name,
            self.artefact.module,
            self.artefact.isa,
            self.artefact.rewritten,
            self.artefact.provenance,
            self.cutoff
        )
    }

    /// Repulsion cut-off radius of the uniform grid.
    pub fn repulsion_cutoff(&self) -> f32 {
        self.cutoff
    }

    /// Change the repulsion cut-off radius (and grid cell size) of the uniform grid.
    pub fn set_repulsion_cutoff(&mut self, cutoff: f32) -> Result<(), LayoutError> {
        self.drain()?;
        self.cutoff = cutoff.max(1.0);
        self.far = FarField::default();
        self.far_job = None;
        self.upload_params()
    }

    fn bind(&self) -> Result<(), LayoutError> {
        CurrentContext::set_current(&self.context).map_err(dev("set context"))
    }

    fn upload_params(&self) -> Result<(), LayoutError> {
        self.bind()?;
        let mut sym = self
            .module
            .get_global::<SimParams>(c"c_params")
            .map_err(dev("c_params symbol"))?;
        sym.copy_from(&SimParams::for_layout(&self.params, self.cutoff))
            .map_err(dev("upload c_params"))
    }

    /// Finish and ingest the queued step, if any. Needed before anything that
    /// changes buffers, masks or parameters the queued work reads.
    fn drain(&mut self) -> Result<(), LayoutError> {
        self.bind()?;
        let mut sink = StepStats::default();
        self.complete(&mut sink).map(|_| ())
    }

    fn reupload(&mut self) -> Result<(), LayoutError> {
        self.bind()?;
        self.far = FarField::default();
        self.far_job = None;
        self.bufs = if self.state.model.n_bodies() == 0 {
            None
        } else {
            Some(Buffers::new(&self.state)?)
        };
        self.cache = OnceCell::new();
        Ok(())
    }

    /// Queue one step at the current host positions. Requires no step in flight.
    fn launch_step(&mut self, t: &mut StepStats) -> Result<(), LayoutError> {
        debug_assert!(!self.in_flight, "launch_step with a step in flight");
        let host = Instant::now();
        let p = &self.params;
        let law = Repulsion {
            k: p.repulsion,
            eps: REPULSION_SOFTENING,
            max_pair: p.max_force(),
            theta2: 0.0,
        };
        let cutoff = self.cutoff;
        // The far field is one step old: the job the previous step queued on
        // its own input positions. It ran while the device worked and the
        // caller slept, so it is usually ready. The first step after a load
        // computes its own. Joining here keeps the result deterministic.
        let sources = self
            .state
            .model
            .far_sources(&self.state.pos, &self.state.node_pos);
        match self.far_job.take().and_then(|job| job.recv().ok()) {
            Some(field) => self.far = field,
            None => self.far = far_field(&sources, cutoff, law),
        }
        let bufs = self.bufs.as_mut().expect("launch_step needs buffers");
        let State {
            model,
            pos,
            node_pos,
            host: forces,
            ..
        } = &mut self.state;
        let (model, pos, node_pos, far): (&_, &Vec<[f32; 3]>, &_, _) =
            (model, pos, node_pos, &self.far);
        // The host terms and the grid read the same positions and nothing
        // else in common, so they run side by side.
        let geom = rayon::join(
            || {
                model.host_forces(pos, node_pos, forces);
                model.apply_far_field(far, forces);
            },
            || {
                let geom = GridGeom::around(pos, cutoff, bufs.cell_cap);
                build_grid(
                    pos,
                    &geom,
                    &mut bufs.scratch_keys,
                    &mut bufs.scratch_sorted,
                    &mut bufs.scratch_start,
                    &mut bufs.scratch_end,
                );
                bufs.up_keys.copy_from_slice(&bufs.scratch_keys);
                bufs.up_sorted.copy_from_slice(&bufs.scratch_sorted);
                bufs.up_start[..geom.cells].copy_from_slice(&bufs.scratch_start[..geom.cells]);
                bufs.up_end[..geom.cells].copy_from_slice(&bufs.scratch_end[..geom.cells]);
                geom
            },
        )
        .1;
        for k in 0..3 {
            for (t, (p, f)) in bufs.scratch_f.iter_mut().zip(pos.iter().zip(forces.iter())) {
                *t = p[k] + f[k];
            }
            bufs.up_target[k].copy_from_slice(&bufs.scratch_f);
        }
        t.host_ms += host.elapsed().as_secs_f32() * 1000.0;

        let n = bufs.n as i32;
        let cells = geom.cells;
        let dims = geom.dims();
        let grid = (bufs.n as u32).div_ceil(BLOCK);
        let (cur, nxt) = (bufs.cur, 1 - bufs.cur);
        let stream = &self.stream;
        let module = &self.module;
        let [e_up, e_kernels, e_down, e_done] = &self.events;
        let null_f = DevicePointer::<f32>::null();
        let null_i = DevicePointer::<i32>::null();
        let (mask_ptr, mask_len, force_grid, pinned_ptr) = if self.state.active.is_some() {
            (
                bufs.mask.as_device_ptr(),
                bufs.mask_len as i32,
                (bufs.mask_len as u32).div_ceil(BLOCK).max(1),
                bufs.pinned.as_device_ptr(),
            )
        } else {
            (null_i, 0, grid, null_i)
        };

        // SAFETY (whole block): every pointer is a live DeviceBuffer owned by
        // `bufs`, sized for `n` bodies (cell buffers for `cell_cap >= cells`, CSR
        // for its own lengths); the argument lists match the kernel signatures
        // checked by `abi::check_ptx_abi`. The async copies read and write
        // page-locked staging that nothing touches until `e_done` completes:
        // `complete` waits on it before the next `launch_step` rewrites the
        // staging, and `positions` waits on it before reading.
        let queued: Result<(), LayoutError> = (|| unsafe {
            e_up.record(stream).map_err(dev("record event"))?;
            for k in 0..3 {
                bufs.target[k]
                    .async_copy_from(&bufs.up_target[k][..], stream)
                    .map_err(dev("upload targets"))?;
            }
            bufs.cell_keys
                .async_copy_from(&bufs.up_keys[..], stream)
                .map_err(dev("upload cell keys"))?;
            bufs.sorted_idx
                .async_copy_from(&bufs.up_sorted[..], stream)
                .map_err(dev("upload sorted index"))?;
            bufs.cell_start
                .index(..cells)
                .async_copy_from(&bufs.up_start[..cells], stream)
                .map_err(dev("upload cell starts"))?;
            bufs.cell_end
                .index(..cells)
                .async_copy_from(&bufs.up_end[..cells], stream)
                .map_err(dev("upload cell ends"))?;
            e_kernels.record(stream).map_err(dev("record event"))?;

            let [px, py, pz] = &bufs.pos[cur];
            let [vx, vy, vz] = &bufs.vel[cur];
            let [ox, oy, oz] = &bufs.pos[nxt];
            let [wx, wy, wz] = &bufs.vel[nxt];
            let [fx, fy, fz] = &bufs.force;
            let [tx, ty, tz] = &bufs.target;
            launch!(module.force_pass_kernel<<<force_grid, BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                fx.as_device_ptr(), fy.as_device_ptr(), fz.as_device_ptr(),
                bufs.cell_start.as_device_ptr(), bufs.cell_end.as_device_ptr(),
                bufs.sorted_idx.as_device_ptr(), bufs.cell_keys.as_device_ptr(), dims,
                bufs.row.as_device_ptr(), bufs.col.as_device_ptr(), bufs.weight.as_device_ptr(),
                n,
                null_f,                          // d_sssp_dist
                DevicePointer::<u8>::null(),     // constraints
                0i32,                            // num_constraints
                null_f, null_f, null_f,          // constraint telemetry
                null_i, null_f, null_f,          // class_id, class_charge, class_mass
                null_f,                          // node_degrees
                null_f,                          // spring_scale
                mask_ptr, mask_len,
                null_f,                          // node_rank
                null_f                           // node_plane
            ))
            .map_err(dev("force_pass_kernel"))?;
            launch!(module.cluster_cohesion_kernel<<<grid, BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                fx.as_device_ptr(), fy.as_device_ptr(), fz.as_device_ptr(),
                tx.as_device_ptr(), ty.as_device_ptr(), tz.as_device_ptr(),
                bufs.identity.as_device_ptr(), n, n, 1.0f32
            ))
            .map_err(dev("cluster_cohesion_kernel"))?;
            launch!(module.integrate_pass_kernel<<<grid, BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                vx.as_device_ptr(), vy.as_device_ptr(), vz.as_device_ptr(),
                fx.as_device_ptr(), fy.as_device_ptr(), fz.as_device_ptr(),
                null_f,                          // mass
                ox.as_device_ptr(), oy.as_device_ptr(), oz.as_device_ptr(),
                wx.as_device_ptr(), wy.as_device_ptr(), wz.as_device_ptr(),
                n,
                null_i, null_f, null_f,          // class_id, class_charge, class_mass
                null_f, null_f, null_f,          // prev_force (adaptive speed off)
                pinned_ptr
            ))
            .map_err(dev("integrate_pass_kernel"))?;
            e_down.record(stream).map_err(dev("record event"))?;
            for k in 0..3 {
                bufs.pos[nxt][k]
                    .async_copy_to(&mut bufs.down_pos[k][..], stream)
                    .map_err(dev("download positions"))?;
                bufs.vel[nxt][k]
                    .async_copy_to(&mut bufs.down_vel[k][..], stream)
                    .map_err(dev("download velocities"))?;
            }
            e_done.record(stream).map_err(dev("record event"))?;
            Ok(())
        })();
        if let Err(e) = queued {
            // Part of the step may be queued against the staging; let it finish.
            let _ = stream.synchronize();
            return Err(e);
        }
        bufs.cur = nxt;
        self.in_flight = true;
        let (tx, rx) = std::sync::mpsc::channel();
        rayon::spawn(move || {
            // The receiver is gone if the model changed meanwhile; drop the result.
            let _ = tx.send(far_field(&sources, cutoff, law));
        });
        self.far_job = Some(rx);
        Ok(())
    }

    /// Wait for the queued step and ingest it: positions, velocities, energy,
    /// aggregates and device times. `Ok(false)` if nothing was queued.
    fn complete(&mut self, t: &mut StepStats) -> Result<bool, LayoutError> {
        if !self.in_flight {
            return Ok(false);
        }
        let wait = Instant::now();
        let [e_up, e_kernels, e_down, e_done] = &self.events;
        let waited = e_done.synchronize().map_err(dev("wait for step"));
        self.in_flight = false;
        waited?;
        t.wait_ms += wait.elapsed().as_secs_f32() * 1000.0;
        let span = |b: &Event, a: &Event| b.elapsed_time_f32(a).map_err(dev("event time"));
        t.upload_ms += span(e_kernels, e_up)?;
        t.kernel_ms += span(e_down, e_kernels)?;
        t.download_ms += span(e_done, e_down)?;

        let host = Instant::now();
        let bufs = self.bufs.as_ref().expect("a queued step has buffers");
        let s = &mut self.state;
        let axis = |b: &LockedBuffer<f32>| {
            let mut v = vec![0.0f32; b.len()];
            v.copy_from_slice(b);
            v
        };
        let [px, py, pz] = [
            axis(&bufs.down_pos[0]),
            axis(&bufs.down_pos[1]),
            axis(&bufs.down_pos[2]),
        ];
        let [vx, vy, vz] = [
            axis(&bufs.down_vel[0]),
            axis(&bufs.down_vel[1]),
            axis(&bufs.down_vel[2]),
        ];
        let (mut energy, mut max_disp) = (0.0f64, 0.0f32);
        for (b, (p, v)) in s.pos.iter_mut().zip(s.vel.iter_mut()).enumerate() {
            let q = [px[b], py[b], pz[b]];
            let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            *p = q;
            *v = [vx[b], vy[b], vz[b]];
            energy += f64::from(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
            max_disp = max_disp.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
        }
        s.model.aggregate(&s.pos, &mut s.node_pos);
        s.record_energy(energy as f32, &self.params);
        self.last_energy = energy as f32;
        t.max_displacement = t.max_displacement.max(max_disp);
        t.host_ms += host.elapsed().as_secs_f32() * 1000.0;
        self.cache = OnceCell::new();
        Ok(true)
    }

    /// The layout including the queued step, read from the download staging.
    fn queued_positions(&self) -> Option<PositionSet> {
        let bufs = self.bufs.as_ref()?;
        if !self.in_flight || self.bind().is_err() || self.events[3].synchronize().is_err() {
            return None;
        }
        let m = &self.state.model;
        let body_pos: Vec<[f32; 3]> = (0..bufs.n)
            .map(|b| {
                [
                    bufs.down_pos[0][b],
                    bufs.down_pos[1][b],
                    bufs.down_pos[2][b],
                ]
            })
            .collect();
        let mut pos = self.state.node_pos.clone();
        m.aggregate(&body_pos, &mut pos);
        let mut vel = vec![[0.0f32; 3]; m.n_nodes()];
        for (b, &ni) in m.body_node.iter().enumerate() {
            vel[ni as usize] = [
                bufs.down_vel[0][b],
                bufs.down_vel[1][b],
                bufs.down_vel[2][b],
            ];
        }
        Some(PositionSet {
            ids: m.ids.clone(),
            pos,
            vel,
        })
    }
}

impl LayoutEngine for CudaEngine {
    fn name(&self) -> &'static str {
        "cuda"
    }

    fn load(
        &mut self,
        snapshot: &GraphSnapshot,
        warm: Option<&PositionSet>,
    ) -> Result<(), LayoutError> {
        snapshot.validate().map_err(LayoutError::InvalidSnapshot)?;
        self.drain()?;
        let (pos, vel) = match warm {
            Some(w) => (
                w.ids.iter().copied().zip(w.pos.iter().copied()).collect(),
                w.ids.iter().copied().zip(w.vel.iter().copied()).collect(),
            ),
            None => Default::default(),
        };
        let focus = self.state.focus_ids.take();
        self.state = State::new(snapshot, &self.params, &pos, &vel, false);
        self.state.focus_ids = focus;
        self.state.refocus();
        self.reupload()
    }

    fn apply_delta(
        &mut self,
        snapshot_after: &GraphSnapshot,
        delta: &GraphDelta,
    ) -> Result<(), LayoutError> {
        check_delta(self.state.loaded, snapshot_after, delta)?;
        self.drain()?;
        self.state = state_after_delta(&self.state, snapshot_after, delta, &self.params);
        self.reupload()
    }

    fn set_focus_mask(&mut self, ids: Option<&[u32]>) {
        if let Err(e) = self.drain() {
            tracing::error!("codescape-layout: CUDA step failed while changing focus: {e}");
        }
        self.state.focus_ids = ids.map(<[u32]>::to_vec);
        self.state.refocus();
        self.cache = OnceCell::new();
        if let Some(b) = self.bufs.as_mut() {
            if let Err(e) = b.set_focus(&self.state) {
                tracing::warn!(
                    "codescape-layout: focus upload failed, integrating every body: {e}"
                );
                self.state.active = None;
                b.mask_len = 0;
            }
        }
    }

    fn step(&mut self, n: u32) -> StepStats {
        let t0 = Instant::now();
        let mut st = StepStats::default();
        if self.bufs.is_none() {
            if self.state.model.n_bodies() == 0 {
                st.steps = n;
            }
        } else if let Err(e) = self.bind() {
            tracing::error!("codescape-layout: {e}");
        } else {
            for _ in 0..n {
                if let Err(e) = self
                    .complete(&mut st)
                    .and_then(|_| self.launch_step(&mut st))
                {
                    tracing::error!("codescape-layout: CUDA step failed: {e}");
                    break;
                }
                st.steps += 1;
            }
            self.cache = OnceCell::new();
        }
        st.energy = self.last_energy;
        st.millis = t0.elapsed().as_secs_f32() * 1000.0;
        st
    }

    fn positions(&self) -> &PositionSet {
        self.cache.get_or_init(|| {
            self.queued_positions()
                .unwrap_or_else(|| self.state.position_set())
        })
    }

    fn settled(&self) -> bool {
        self.state.settled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_graphs::{repo_graph, Repo};
    use crate::{knn_overlap, CpuEngine};
    use codescape_model::Tier;

    #[test]
    fn device_picker_prefers_most_free_memory_then_lowest_ordinal() {
        // The estate shape on 2026-10-17: device 0 busy, two idle cards.
        assert_eq!(
            best_by_free(&[(0, 20 << 30), (1, 21 << 30), (2, 45 << 30)]),
            Some(2)
        );
        assert_eq!(
            best_by_free(&[(0, 10), (1, 10)]),
            Some(0),
            "tie goes to the lower ordinal"
        );
        assert_eq!(best_by_free(&[(3, 1)]), Some(3));
        assert_eq!(best_by_free(&[]), None);
    }

    #[test]
    fn sim_params_mirror_is_212_bytes_of_four_byte_fields() {
        assert_eq!(std::mem::size_of::<SimParams>(), 212);
        assert_eq!(std::mem::size_of::<SimParams>() / 4, 53);
        assert_eq!(std::mem::align_of::<SimParams>(), 4);
        let p = SimParams::for_layout(&LayoutParams::default(), 100.0);
        assert!(
            p.max_force > 0.0 && p.damping > 0.0,
            "a zero max_force or damping freezes everything"
        );
        assert_eq!(
            (
                p.warmup_iterations,
                p.temperature,
                p.viewport_bounds,
                p.stability_threshold,
                p.lin_log_mode
            ),
            (0, 0.0, 0.0, 0.0, 0)
        );
        assert!(p.grid_cell_size >= p.repulsion_cutoff);
    }

    /// Device tests run only with `CODESCAPE_GPU_TESTS=1` and `--ignored`.
    fn gpu_or_skip() -> Option<CudaEngine> {
        if std::env::var("CODESCAPE_GPU_TESTS").as_deref() != Ok("1") {
            eprintln!("SKIPPED: set CODESCAPE_GPU_TESTS=1 to run CUDA device tests");
            return None;
        }
        Some(CudaEngine::new(LayoutParams::default()).expect("CUDA engine with verified PTX"))
    }

    fn graph(scale: usize) -> GraphSnapshot {
        graph_seeded(scale, 17)
    }

    fn graph_seeded(scale: usize, seed: u64) -> GraphSnapshot {
        repo_graph(&Repo {
            dirs: 2 * scale,
            files_per_dir: 10,
            symbols_per_file: 25,
            calls_per_symbol: 2,
            seed,
        })
    }

    #[test]
    #[ignore = "CUDA device: CODESCAPE_GPU_TESTS=1 cargo test --features cuda -- --ignored"]
    fn select_engine_prefers_verified_cuda() {
        if gpu_or_skip().is_none() {
            return;
        }
        let (engine, reason) = crate::select_engine(LayoutParams::default(), true);
        assert_eq!(engine.name(), "cuda", "{reason}");
        assert!(
            reason.starts_with("cuda: ") && reason.contains("provenance compiled"),
            "{reason}"
        );
        let (engine, reason) = crate::select_engine(LayoutParams::default(), false);
        assert_eq!(
            (engine.name(), reason.as_str()),
            ("cpu", "cpu: CUDA not requested")
        );
    }

    #[test]
    #[ignore = "CUDA device: CODESCAPE_GPU_TESTS=1 cargo test --features cuda -- --ignored"]
    fn gpu_steps_and_settles() {
        let Some(mut e) = gpu_or_skip() else { return };
        eprintln!("{}", e.describe());
        let g = graph(4);
        e.load(&g, None).unwrap();
        let first = e.step(1);
        assert_eq!(first.steps, 1);
        let mut steps = 1;
        while !e.settled() && steps < 4000 {
            let s = e.step(50);
            assert_eq!(s.steps, 50, "a CUDA step failed");
            steps += 50;
        }
        assert!(e.positions().pos.iter().flatten().all(|v| v.is_finite()));
        assert!(e.settled(), "GPU layout not settled after {steps} steps");
    }

    #[test]
    #[ignore = "CUDA device: CODESCAPE_GPU_TESTS=1 cargo test --features cuda -- --ignored"]
    fn gpu_focus_mask_freezes_exactly() {
        let Some(mut e) = gpu_or_skip() else { return };
        let g = graph(1);
        e.load(&g, None).unwrap();
        e.step(10);
        let before = e.positions().clone();
        let target = g.nodes.iter().find(|n| n.tier == Tier::L2).unwrap().id;
        e.set_focus_mask(Some(&[target]));
        let active = e.state.active.clone().unwrap();
        e.step(20);
        let after = e.positions();
        for (b, &ni) in e.state.model.body_node.iter().enumerate() {
            let i = ni as usize;
            if active.contains(&(b as u32)) {
                continue;
            }
            assert_eq!(
                after.pos[i], before.pos[i],
                "frozen body {} moved on the GPU",
                after.ids[i]
            );
        }
    }

    /// The ADR-2061 oracle check. Different numerics alone (Barnes-Hut vs
    /// grid + monopoles) reshuffle symbols inside
    /// a file, so the symbol threshold is calibrated against the CPU engine run
    /// against itself with a 10% smaller time step. Files must agree at ≥ 0.9.
    #[test]
    #[ignore = "CUDA device: CODESCAPE_GPU_TESTS=1 cargo test --features cuda -- --ignored"]
    fn gpu_conforms_to_cpu_oracle() {
        let Some(mut gpu) = gpu_or_skip() else { return };
        let g = graph(4);
        let tier = |p: &PositionSet, t: Tier| {
            let keep: Vec<usize> = g
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.tier == t)
                .map(|(i, _)| i)
                .collect();
            PositionSet {
                ids: keep.iter().map(|&i| p.ids[i]).collect(),
                pos: keep.iter().map(|&i| p.pos[i]).collect(),
                vel: vec![[0.0; 3]; keep.len()],
            }
        };
        let p = LayoutParams::default();
        let mut cpu = CpuEngine::new(p);
        let mut cpu_alt = CpuEngine::new(LayoutParams {
            dt: p.dt * 0.9,
            ..p
        });
        cpu.load(&g, None).unwrap();
        cpu_alt.load(&g, None).unwrap();
        gpu.load(&g, None).unwrap();
        let k = 10;
        assert_eq!(
            knn_overlap(cpu.positions(), gpu.positions(), k),
            1.0,
            "both engines start from the same seed"
        );
        let (mut ec, mut eg) = (Vec::new(), Vec::new());
        for _ in 0..8 {
            ec.push(cpu.step(50).energy);
            eg.push(gpu.step(50).energy);
        }
        cpu_alt.step(400);
        let sym = |a: &PositionSet, b: &PositionSet| {
            knn_overlap(&tier(a, Tier::L2), &tier(b, Tier::L2), k)
        };
        let file = |a: &PositionSet, b: &PositionSet| {
            knn_overlap(&tier(a, Tier::L1), &tier(b, Tier::L1), k)
        };
        let (base_sym, base_file) = (
            sym(cpu.positions(), cpu_alt.positions()),
            file(cpu.positions(), cpu_alt.positions()),
        );
        let (gpu_sym, gpu_file) = (
            sym(cpu.positions(), gpu.positions()),
            file(cpu.positions(), gpu.positions()),
        );
        eprintln!("energy cpu {ec:?}\nenergy gpu {eg:?}");
        eprintln!("k-NN overlap (k={k}) after 400 steps: GPU vs CPU symbols {gpu_sym:.3} files {gpu_file:.3}; CPU vs CPU(0.9 dt) symbols {base_sym:.3} files {base_file:.3}");
        assert!(
            ec.last() < ec.first() && eg.last() < eg.first(),
            "both energies fall"
        );
        assert!(
            gpu_file >= 0.9,
            "file neighbourhoods agree {gpu_file:.3} < 0.9"
        );
        assert!(
            gpu_sym >= base_sym - 0.1,
            "symbol neighbourhoods {gpu_sym:.3} vs the CPU's own numerics baseline {base_sym:.3}"
        );
    }

    /// The host-built grid is bit-identical to `build_grid_kernel` → thrust
    /// stable sort → `compute_cell_bounds_kernel` on the same positions.
    /// The cell is a power of two, so the device's fast-math reciprocal is exact.
    #[test]
    #[ignore = "CUDA device: CODESCAPE_GPU_TESTS=1 cargo test --features cuda -- --ignored"]
    fn host_grid_matches_device_grid() {
        use rand::{Rng, SeedableRng};
        use std::ffi::{c_int, c_void};

        #[repr(C)]
        #[derive(Debug, Clone, Copy, Default, cust::DeviceCopy)]
        struct Aabb {
            min: [f32; 3],
            max: [f32; 3],
        }
        extern "C" {
            fn thrust_sort_key_value(
                keys_in: *const c_void,
                keys_out: *mut c_void,
                values_in: *const c_void,
                values_out: *mut c_void,
                num_items: c_int,
                stream: *mut c_void,
            );
        }

        let Some(e) = gpu_or_skip() else { return };
        let ptx = verified_ptx().unwrap().0;
        assert_eq!(crate::abi::entry_arity(&ptx, "build_grid_kernel"), Some(9));
        assert_eq!(
            crate::abi::entry_arity(&ptx, "compute_cell_bounds_kernel"),
            Some(5)
        );
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        let n = 20_000usize;
        let mut pos: Vec<[f32; 3]> = (0..n)
            .map(|_| {
                [
                    rng.gen_range(-900.0..900.0),
                    rng.gen_range(-500.0..500.0),
                    rng.gen_range(-700.0..700.0),
                ]
            })
            .collect();
        pos[17] = [5_000.0, 0.0, 0.0]; // one far-out body: forces cell growth
        let mut g = GridGeom::around(&pos, 256.0, 4096);
        g.cell = 2f32.powi(g.cell.log2().ceil() as i32); // power of two, still covers the box
        let (mut keys, mut sorted, mut start, mut end) =
            (vec![0; n], vec![0; n], vec![0; g.cells], vec![0; g.cells]);
        build_grid(&pos, &g, &mut keys, &mut sorted, &mut start, &mut end);

        e.bind().unwrap();
        let s = &e.stream;
        let module = &e.module;
        let axis = |k: usize| DeviceBuffer::from_slice(&split(&pos, k)).unwrap();
        let (dx, dy, dz) = (axis(0), axis(1), axis(2));
        let d_keys = DeviceBuffer::<i32>::zeroed(n).unwrap();
        let d_sorted_keys = DeviceBuffer::<i32>::zeroed(n).unwrap();
        let d_sorted = DeviceBuffer::<i32>::zeroed(n).unwrap();
        let d_start = DeviceBuffer::<i32>::zeroed(g.cells).unwrap();
        let d_end = DeviceBuffer::<i32>::zeroed(g.cells).unwrap();
        let max = [0, 1, 2].map(|k| g.min[k] + g.cell * g.dims[k] as f32);
        let blocks = (n as u32).div_ceil(BLOCK);
        // SAFETY: buffers are live and sized as the kernels expect; the arities
        // were checked above; the stream is synchronised before any read.
        unsafe {
            launch!(module.build_grid_kernel<<<blocks, BLOCK, 0, s>>>(
                dx.as_device_ptr(), dy.as_device_ptr(), dz.as_device_ptr(), d_keys.as_device_ptr(),
                Aabb { min: g.min, max }, g.dims(), g.cell, n as i32, g.cells as i32
            ))
            .unwrap();
            thrust_sort_key_value(
                d_keys.as_device_ptr().as_raw() as *const c_void,
                d_sorted_keys.as_device_ptr().as_raw() as *mut c_void,
                std::ptr::null(),
                d_sorted.as_device_ptr().as_raw() as *mut c_void,
                n as c_int,
                s.as_inner() as *mut c_void,
            );
            launch!(module.compute_cell_bounds_kernel<<<blocks, BLOCK, 0, s>>>(
                d_sorted_keys.as_device_ptr(), d_start.as_device_ptr(), d_end.as_device_ptr(), n as i32, g.cells as i32
            ))
            .unwrap();
        }
        s.synchronize().unwrap();
        let back = |b: &DeviceBuffer<i32>| {
            let mut v = vec![0; b.len()];
            b.copy_to(&mut v[..]).unwrap();
            v
        };
        assert!(g.cells <= 4096 && g.cells > 1, "{g:?}");
        assert_eq!(back(&d_keys), keys, "cell keys");
        assert_eq!(back(&d_sorted), sorted, "sorted body order");
        assert_eq!(back(&d_start), start, "cell starts");
        assert_eq!(back(&d_end), end, "cell ends");
    }

    /// Steps until the mean squared speed's 32-step moving average first drops
    /// below `threshold`, one `step(1)` per call as the server drives it.
    fn steep_steps(e: &mut dyn LayoutEngine, g: &GraphSnapshot, threshold: f32) -> u32 {
        e.load(g, None).unwrap();
        let n = g.tier_counts()[2].max(1) as f32;
        let mut avg = f32::MAX;
        for k in 1..=20_000u32 {
            let m = e.step(1).energy / n;
            avg = if avg == f32::MAX {
                m
            } else {
                avg + (m - avg) / 32.0
            };
            if k > 32 && avg < threshold {
                return k;
            }
        }
        panic!("{} never dropped below {threshold}", e.name());
    }

    /// Steps until `settled()`, one `step(1)` per call.
    fn settle_steps(e: &mut dyn LayoutEngine, g: &GraphSnapshot) -> u32 {
        e.load(g, None).unwrap();
        for k in 1..=20_000u32 {
            assert_eq!(e.step(1).steps, 1);
            if e.settled() {
                return k;
            }
        }
        panic!("{} did not settle", e.name());
    }

    fn median(mut v: Vec<u32>) -> f64 {
        v.sort_unstable();
        let m = v.len() / 2;
        if v.len().is_multiple_of(2) {
            f64::from(v[m - 1] + v[m]) / 2.0
        } else {
            f64::from(v[m])
        }
    }

    /// Mean squared speed at which the decay is still steep, so the crossing
    /// step is well conditioned. Full settle sits on a slow plateau where whole
    /// files keep rearranging; there the same engine with another layout seed
    /// settles up to 1.5× apart, so its step count is reported, not asserted.
    const STEEP_ENERGY: f32 = 0.2;

    /// Step-count parity, CPU vs CUDA, on a 2k and a 20k graph over four
    /// graph-and-layout seeds: medians of the steep-decay crossing within 10%.
    /// The queen approved this metric in place of full-settle step count,
    /// because full settle is noise-dominated: the CPU engine against itself,
    /// changing only the layout seed, settled up to ×1.49 apart.
    #[test]
    #[ignore = "CUDA device: CODESCAPE_GPU_TESTS=1 cargo test --release --features cuda -- --ignored --nocapture gpu_settle_step_parity"]
    fn gpu_settle_step_parity() {
        if gpu_or_skip().is_none() {
            return;
        }
        let full = std::env::var("CODESCAPE_PARITY_FULL_SETTLE").as_deref() == Ok("1");
        for scale in [4usize, 40] {
            let (mut cs, mut gs, mut cf, mut gf) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for seed in 1..=4u64 {
                let g = graph_seeded(scale, seed);
                let p = LayoutParams {
                    seed,
                    ..LayoutParams::default()
                };
                let (mut cpu, mut gpu) = (CpuEngine::new(p), CudaEngine::new(p).unwrap());
                cs.push(steep_steps(&mut cpu, &g, STEEP_ENERGY));
                gs.push(steep_steps(&mut gpu, &g, STEEP_ENERGY));
                if full {
                    cf.push(settle_steps(&mut cpu, &g));
                    gf.push(settle_steps(&mut gpu, &g));
                }
            }
            let ratio = median(gs.clone()) / median(cs.clone());
            println!("parity {} symbols, steep (avg < {STEEP_ENERGY}): cpu {cs:?} gpu {gs:?}, median gpu/cpu {ratio:.3}", scale * 500);
            if full {
                println!(
                    "parity {} symbols, full settle: cpu {cf:?} gpu {gf:?}, median gpu/cpu {:.3}",
                    scale * 500,
                    median(gf.clone()) / median(cf.clone())
                );
            }
            assert!(
                (0.9..=1.1).contains(&ratio),
                "GPU needs {ratio:.3}× the CPU's steps at {} symbols",
                scale * 500
            );
        }
    }

    /// Server-shaped cost: one `step(1)` per call on a ~28k-symbol graph,
    /// with a 240 Hz pacing sleep like the server's layout loop.
    #[test]
    #[ignore = "CUDA device benchmark: CODESCAPE_GPU_TESTS=1 cargo test --release --features cuda -- --ignored --nocapture bench_gpu_server_shaped"]
    fn bench_gpu_server_shaped() {
        let Some(mut e) = gpu_or_skip() else { return };
        eprintln!("{}", e.describe());
        let g = graph(56);
        e.load(&g, None).unwrap();
        for _ in 0..50 {
            e.step(1);
        }
        let period = std::time::Duration::from_secs_f64(1.0 / 240.0);
        for paced in [false, true] {
            let calls = 300u32;
            let mut sum = StepStats::default();
            let t = Instant::now();
            for _ in 0..calls {
                let s = e.step(1);
                assert_eq!(s.steps, 1);
                sum.millis += s.millis;
                sum.host_ms += s.host_ms;
                sum.upload_ms += s.upload_ms;
                sum.kernel_ms += s.kernel_ms;
                sum.download_ms += s.download_ms;
                sum.wait_ms += s.wait_ms;
                if paced && s.millis < period.as_secs_f32() * 1000.0 {
                    std::thread::sleep(
                        period - std::time::Duration::from_secs_f32(s.millis / 1000.0),
                    );
                }
            }
            let per = |x: f32| x / calls as f32;
            println!(
                "server-shaped {} symbols{}: {:.2} ms/step wall, {:.2} ms in step(); host {:.2}, wait {:.2}; device upload {:.2}, kernels {:.2}, download {:.2}",
                g.tier_counts()[2],
                if paced { " paced at 240 Hz" } else { ", back to back" },
                t.elapsed().as_secs_f32() * 1000.0 / calls as f32,
                per(sum.millis), per(sum.host_ms), per(sum.wait_ms), per(sum.upload_ms), per(sum.kernel_ms), per(sum.download_ms)
            );
        }
    }

    /// Cold seed → settled, and `apply_delta` latency, at 100k symbols.
    #[test]
    #[ignore = "CUDA device benchmark: CODESCAPE_GPU_TESTS=1 cargo test --release --features cuda -- --ignored --nocapture bench_gpu_settle"]
    fn bench_gpu_settle_and_delta_100k() {
        let Some(mut e) = gpu_or_skip() else { return };
        eprintln!("{}", e.describe());
        let g = repo_graph(&Repo {
            dirs: 50,
            files_per_dir: 40,
            symbols_per_file: 50,
            calls_per_symbol: 3,
            seed: 21,
        });
        let t = Instant::now();
        e.load(&g, None).unwrap();
        let load_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let mut steps = 0u32;
        while !e.settled() && steps < 4000 {
            assert_eq!(e.step(25).steps, 25);
            steps += 25;
        }
        let settle_s = t.elapsed().as_secs_f64();
        let settled = e.settled();
        let (after, delta) = crate::model::test_graphs::one_symbol_delta(&g);
        let t = Instant::now();
        e.apply_delta(&after, &delta).unwrap();
        let delta_ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("cuda 100k: load {load_ms:.0} ms; settled={settled} after {steps} steps in {settle_s:.1} s; apply_delta {delta_ms:.0} ms");
    }

    /// Back-to-back step time at 10k, 50k and 100k symbols.
    #[test]
    #[ignore = "CUDA device benchmark: CODESCAPE_GPU_TESTS=1 cargo test --release --features cuda -- --ignored --nocapture bench_gpu_step"]
    fn bench_gpu_step() {
        let Some(mut e) = gpu_or_skip() else { return };
        for (dirs, files, syms) in [(10, 20, 50), (25, 40, 50), (50, 40, 50)] {
            let g = repo_graph(&Repo {
                dirs,
                files_per_dir: files,
                symbols_per_file: syms,
                calls_per_symbol: 3,
                seed: 21,
            });
            e.load(&g, None).unwrap();
            e.step(3);
            let s = e.step(20);
            assert_eq!(s.steps, 20);
            println!(
                "cuda step: {:>6} bodies, {:>6} edges: {:>6.2} ms/step (host {:.2}, wait {:.2}, device upload {:.2} kernels {:.2} download {:.2})",
                e.state.model.n_bodies(), g.edges.len(), s.millis / 20.0, s.host_ms / 20.0, s.wait_ms / 20.0, s.upload_ms / 20.0, s.kernel_ms / 20.0, s.download_ms / 20.0
            );
        }
    }
}
