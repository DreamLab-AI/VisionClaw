//! Device bring-up and launch sequences for the force kernels.
//!
//! Every launcher here reproduces the host-side sequence the server runs
//! (`src/utils/unified_gpu_compute/execution.rs` for the unified force pass,
//! `src/utils/unified_gpu_compute/semantic.rs` for the semantic module)
//! against the PTX this crate compiles, so a result is a statement about the
//! kernels as compiled, not about a wrapper.
//!
//! One deliberate difference: the server sorts the grid cell keys with the
//! `thrust_sort_key_value` FFI wrapper (a `stable_sort_by_key` over the
//! identity permutation). This harness performs the same stable sort on the
//! host, so the thrust call itself is outside what these tests cover.

use std::ffi::CStr;

use cust::context::Context;
use cust::device::{Device, DeviceAttribute};
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer, DeviceCopy, DevicePointer};
use cust::module::Module;
use cust::stream::{Stream, StreamFlags};
use cust::CudaFlags;

use visionclaw_gpu::ptx_loader::{load_ptx_module_sync, PTXModule};

use crate::fixtures::Fixture;

/// Threads per block, as the server launches them.
pub const BLOCK: u32 = 256;

/// Centroid table slots the server allocates for the semantic module
/// (`TYPE_SLOTS` in `semantic.rs`).
pub const TYPE_SLOTS: usize = 16;

// ---------------------------------------------------------------------------
// Device-side structure mirrors
// ---------------------------------------------------------------------------

/// `SimParams` from `visionclaw_unified.cu`, field for field (53 four-byte
/// words, 212 bytes; the kernel `static_assert`s the same size).
///
/// Transcribed from the `.cu` struct rather than imported, because the
/// server's mirror lives in the root crate, which depends on this one.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SimParams {
    pub dt: f32,
    pub damping: f32,
    pub warmup_iterations: u32,
    pub cooling_rate: f32,
    pub spring_k: f32,
    pub rest_length: f32,
    pub repel_k: f32,
    pub repulsion_cutoff: f32,
    pub repulsion_softening_epsilon: f32,
    pub center_gravity_k: f32,
    pub max_force: f32,
    pub max_velocity: f32,
    pub grid_cell_size: f32,
    pub feature_flags: u32,
    pub seed: u32,
    pub iteration: i32,
    pub separation_radius: f32,
    pub cluster_strength: f32,
    pub alignment_strength: f32,
    pub temperature: f32,
    pub viewport_bounds: f32,
    pub sssp_alpha: f32,
    pub boundary_damping: f32,
    pub constraint_ramp_frames: u32,
    pub constraint_max_force_per_node: f32,
    pub stability_threshold: f32,
    pub min_velocity_threshold: f32,
    pub world_bounds_min: f32,
    pub world_bounds_max: f32,
    pub cell_size_lod: f32,
    pub k_neighbors_max: u32,
    pub anomaly_detection_radius: f32,
    pub learning_rate_default: f32,
    pub norm_delta_cap: f32,
    pub position_constraint_attraction: f32,
    pub lof_score_min: f32,
    pub lof_score_max: f32,
    pub weight_precision_multiplier: f32,
    pub gravity: f32,
    pub lin_log_mode: u32,
    pub scaling_ratio: f32,
    pub adaptive_speed: u32,
    pub global_speed: f32,
    pub dag_bias_k: f32,
    pub dag_level_distance: f32,
    pub layout_mode: u32,
    pub plane_bias_k: f32,
    pub plane_spacing: f32,
    pub radial_center_x: f32,
    pub radial_center_y: f32,
    pub radial_center_z: f32,
    pub layer_bias_k: f32,
    pub layer_spacing: f32,
}
const _: () = assert!(std::mem::size_of::<SimParams>() == 212);
// SAFETY: `SimParams` is `repr(C)` and made only of `f32`/`u32`/`i32`, so it
// has no padding, pointers or `Drop`; a bitwise copy into `c_params` is exact.
unsafe impl DeviceCopy for SimParams {}

/// `FeatureFlags` bits from `visionclaw_unified.cu`.
pub mod flags {
    pub const REPULSION: u32 = 1 << 0;
    pub const SPRINGS: u32 = 1 << 1;
    pub const CENTERING: u32 = 1 << 2;
    pub const CONSTRAINTS: u32 = 1 << 4;
    pub const SSSP_SPRING_ADJUST: u32 = 1 << 6;
}

/// `ConstraintData` from `visionclaw_unified.cu` (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstraintData {
    pub kind: i32,
    pub count: i32,
    pub node_idx: [i32; 4],
    pub params: [f32; 8],
    pub weight: f32,
    pub activation_frame: i32,
}
const _: () = assert!(std::mem::size_of::<ConstraintData>() == 64);
// SAFETY: `repr(C)`, only `i32`/`f32` fields and arrays of them, no padding.
unsafe impl DeviceCopy for ConstraintData {}

/// `ConstraintKind` numbering of the live kernel.
pub mod kind {
    pub const DISTANCE: i32 = 0;
    pub const POSITION: i32 = 1;
    pub const SEPARATION: i32 = 6;
    pub const ALIGNMENT: i32 = 7;
}

/// `int3` by value.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Int3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
// SAFETY: `repr(C)` of three `i32`, no padding.
unsafe impl DeviceCopy for Int3 {}

/// `AABB { float3 min; float3 max; }` by value.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}
// SAFETY: `repr(C)` of six `f32`, no padding.
unsafe impl DeviceCopy for Aabb {}

/// `ConstraintIndexView { const int* offsets; const int* ids; }`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct ConstraintIndexView {
    offsets: u64,
    ids: u64,
}
// SAFETY: two raw device addresses as `u64`; the device struct is two pointers.
unsafe impl DeviceCopy for ConstraintIndexView {}

/// `SemanticConfig` from `semantic_forces.cu`, in the server's field order.
///
/// Uploaded as the 35-word image `semantic.rs::pack_config` builds: every
/// `bool` occupies a whole word (1 or 0) and the device reads its first byte.
#[derive(Clone, Copy, Debug, Default)]
pub struct SemanticConfig {
    pub dag: [f32; 4],
    pub dag_enabled: bool,
    pub type_cluster: [f32; 3],
    pub type_cluster_enabled: bool,
    pub collision: [f32; 3],
    pub collision_enabled: bool,
    pub attribute_spring: [f32; 4],
    pub attribute_spring_enabled: bool,
    pub physicality: [f32; 3],
    pub physicality_enabled: bool,
    pub role: [f32; 3],
    pub role_enabled: bool,
    pub maturity: [f32; 3],
    pub maturity_enabled: bool,
    pub cross_domain: [f32; 4],
    pub cross_domain_enabled: bool,
}

impl SemanticConfig {
    fn image(&self) -> [u32; 35] {
        let mut w = Vec::with_capacity(35);
        let mut push = |vals: &[f32], on: bool| {
            w.extend(vals.iter().map(|v| v.to_bits()));
            w.push(u32::from(on));
        };
        push(&self.dag, self.dag_enabled);
        push(&self.type_cluster, self.type_cluster_enabled);
        push(&self.collision, self.collision_enabled);
        push(&self.attribute_spring, self.attribute_spring_enabled);
        push(&self.physicality, self.physicality_enabled);
        push(&self.role, self.role_enabled);
        push(&self.maturity, self.maturity_enabled);
        push(&self.cross_domain, self.cross_domain_enabled);
        w.try_into().expect("SemanticConfig image is 35 words")
    }
}

/// One `DynamicForceConfig` entry of the relationship table.
#[derive(Clone, Copy, Debug)]
pub struct Relationship {
    pub strength: f32,
    pub rest_length: f32,
    pub directional: bool,
    pub force_type: u32,
}

/// Words in `DynamicRelationshipBuffer` (256 * 4 + 3).
const DYNAMIC_WORDS: usize = 256 * 4 + 3;

/// `semantic.rs::pack_dynamic`: unused slots stay zero, then the count,
/// version and enable words.
fn dynamic_image(table: &[Relationship], enabled: bool) -> [u32; DYNAMIC_WORDS] {
    let mut w = [0u32; DYNAMIC_WORDS];
    for (i, r) in table.iter().take(256).enumerate() {
        w[i * 4] = r.strength.to_bits();
        w[i * 4 + 1] = r.rest_length.to_bits();
        w[i * 4 + 2] = u32::from(r.directional);
        w[i * 4 + 3] = r.force_type;
    }
    w[1024] = table.len().min(256) as u32;
    w[1025] = 1;
    w[1026] = u32::from(enabled);
    w
}

// ---------------------------------------------------------------------------
// Device bring-up
// ---------------------------------------------------------------------------

/// A CUDA context with both force modules loaded.
///
/// Field order is load-bearing: modules and the stream belong to the context
/// and must drop before it, so `_ctx` is declared last.
pub struct Gpu {
    pub unified: Module,
    pub semantic: Module,
    pub stream: Stream,
    pub device: String,
    _ctx: Context,
}

/// Bring up the first visible device, or return `None` when this host has no
/// CUDA driver or no device (the only skip conditions). Any later failure,
/// including a PTX load or JIT failure, panics: a kernel that does not load
/// is a failure, not a skip.
///
/// Set `VISIONCLAW_EXPECT_PCI_BUS` (decimal or `0x` hex) to make the harness
/// refuse to run on any other card.
pub fn gpu(test: &str) -> Option<Gpu> {
    if let Err(e) = cust::init(CudaFlags::empty()) {
        println!("SKIP {test}: CUDA driver unavailable: {e}");
        return None;
    }
    match Device::num_devices() {
        Ok(n) if n > 0 => {}
        Ok(_) => {
            println!("SKIP {test}: no CUDA device visible");
            return None;
        }
        Err(e) => {
            println!("SKIP {test}: cannot count CUDA devices: {e}");
            return None;
        }
    }
    let device = Device::get_device(0).expect("device 0");
    let name = device.name().expect("device name");
    let bus = device
        .get_attribute(DeviceAttribute::PciBusId)
        .expect("PCI bus id");
    let label = format!("{name} (PCI bus 0x{bus:02X})");
    if let Ok(want) = std::env::var("VISIONCLAW_EXPECT_PCI_BUS") {
        let want = want.trim();
        let parsed = match want.strip_prefix("0x").or_else(|| want.strip_prefix("0X")) {
            Some(hex) => i32::from_str_radix(hex, 16),
            None => want.parse(),
        }
        .expect("VISIONCLAW_EXPECT_PCI_BUS must be an integer");
        assert_eq!(
            parsed, bus,
            "{test}: refusing to run on {label}; VISIONCLAW_EXPECT_PCI_BUS={want}. \
             Set CUDA_DEVICE_ORDER=PCI_BUS_ID so CUDA_VISIBLE_DEVICES matches nvidia-smi."
        );
    }
    println!("{test}: device {label}");
    let ctx = Context::new(device).expect("CUDA context");
    let load = |m: PTXModule| -> Module {
        let ptx = load_ptx_module_sync(m).unwrap_or_else(|e| panic!("PTX for {m:?}: {e}"));
        Module::from_ptx(&ptx, &[]).unwrap_or_else(|e| {
            panic!(
                "{test}: loading {m:?} failed on {label}: {e}. If this is a JIT failure, check \
                 CUDA_ARCH matches the selected card and CUDA_DEVICE_ORDER=PCI_BUS_ID is set."
            )
        })
    };
    let unified = load(PTXModule::VisionflowUnified);
    let semantic = load(PTXModule::SemanticForces);
    let stream = Stream::new(StreamFlags::NON_BLOCKING, None).expect("stream");
    Some(Gpu {
        unified,
        semantic,
        stream,
        device: label,
        _ctx: ctx,
    })
}

fn grid(n: usize) -> u32 {
    (n as u32).div_ceil(BLOCK).max(1)
}

/// Upload a slice; cust rejects zero-length buffers, so an empty slice gets
/// one placeholder element that no kernel reads.
fn up<T: DeviceCopy + Default + Clone>(v: &[T]) -> DeviceBuffer<T> {
    if v.is_empty() {
        DeviceBuffer::from_slice(&[T::default()]).expect("upload")
    } else {
        DeviceBuffer::from_slice(v).expect("upload")
    }
}

fn down<T: DeviceCopy + Default + Clone>(b: &DeviceBuffer<T>, n: usize) -> Vec<T> {
    let mut v = vec![T::default(); b.len()];
    b.copy_to(&mut v).expect("download");
    v.truncate(n);
    v
}

fn opt_ptr<T: DeviceCopy>(b: &Option<DeviceBuffer<T>>) -> DevicePointer<T> {
    b.as_ref()
        .map(|b| b.as_device_ptr())
        .unwrap_or_else(DevicePointer::null)
}

// ---------------------------------------------------------------------------
// Unified force pass
// ---------------------------------------------------------------------------

/// Which of the two production force kernels to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceKernel {
    /// `force_pass_kernel`, chosen when `stability_threshold <= 0`.
    Main,
    /// `force_pass_with_stability_kernel`, chosen when `stability_threshold > 0`.
    /// Run with the gate open: all velocities 1, skip flag 0.
    Stability,
}

/// The optional per-node and per-edge buffers of the force pass. `None` is
/// the null pointer the server passes when a feature is absent.
#[derive(Clone, Debug, Default)]
pub struct ForceInputs {
    pub node_degrees: Option<Vec<f32>>,
    pub class_id: Option<Vec<i32>>,
    pub class_charge: Option<Vec<f32>>,
    pub spring_scale: Option<Vec<f32>>,
    pub sssp: Option<Vec<f32>>,
    pub node_rank: Option<Vec<f32>>,
    pub node_plane: Option<Vec<f32>>,
    /// Per-CSR-slot rest lengths for the hold overlay (`g_edge_rest_snapshot`).
    pub edge_rest: Option<Vec<f32>>,
    pub constraints: Vec<ConstraintData>,
}

/// Forces per node, as three parallel arrays.
pub type Forces = Vec<[f32; 3]>;

/// Production grid geometry for a fixture: cell size at least the cutoff,
/// AABB padded by one cell on every side, `ceil(extent / cell)` cells per axis.
pub fn grid_geometry(fx: &Fixture, params: &SimParams) -> (Aabb, Int3, f32) {
    let cell = params.grid_cell_size.max(params.repulsion_cutoff).max(1.0);
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &fx.pos {
        for a in 0..3 {
            min[a] = min[a].min(p[a]);
            max[a] = max[a].max(p[a]);
        }
    }
    for a in 0..3 {
        min[a] -= cell;
        max[a] += cell;
    }
    let dim = |a: usize| ((max[a] - min[a]) / cell).ceil().max(1.0) as i32;
    (
        Aabb { min, max },
        Int3 {
            x: dim(0),
            y: dim(1),
            z: dim(2),
        },
        cell,
    )
}

impl Gpu {
    fn set_params(&self, params: &SimParams) {
        let mut sym = self
            .unified
            .get_global::<SimParams>(c"c_params")
            .expect("c_params symbol");
        sym.copy_from(params).expect("upload c_params");
    }

    fn set_u64(&self, name: &CStr, value: u64) {
        let mut sym = self.unified.get_global::<u64>(name).expect("device global");
        sym.copy_from(&value).expect("upload device global");
    }

    /// Run one force pass over `fx` and return the per-node forces.
    pub fn force_pass(
        &self,
        fx: &Fixture,
        params: &SimParams,
        inputs: &ForceInputs,
        which: ForceKernel,
    ) -> Forces {
        let n = fx.n();
        let stream = &self.stream;
        let mut params = *params;
        if which == ForceKernel::Stability {
            params.min_velocity_threshold = 0.0;
        }
        self.set_params(&params);

        let px = up(&fx.pos.iter().map(|p| p[0]).collect::<Vec<_>>());
        let py = up(&fx.pos.iter().map(|p| p[1]).collect::<Vec<_>>());
        let pz = up(&fx.pos.iter().map(|p| p[2]).collect::<Vec<_>>());
        let csr = fx.csr();
        let row = up(&csr.offsets);
        let col = up(&csr.cols);
        let wts = up(&csr.weights);

        // Spatial grid: device key build, host stable sort, device cell bounds.
        let (aabb, dims, cell) = grid_geometry(fx, &params);
        let cells = (dims.x * dims.y * dims.z) as usize;
        let keys = up(&vec![0i32; n]);
        let build = self
            .unified
            .get_function("build_grid_kernel")
            .expect("kernel");
        // SAFETY: position buffers hold n floats, `keys` holds n ints, and the
        // kernel clamps every key into [0, cells).
        unsafe {
            launch!(build<<<grid(n), BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                keys.as_device_ptr(), aabb, dims, cell, n as i32, cells as i32
            ))
            .expect("build_grid_kernel");
        }
        stream.synchronize().expect("sync");
        let key_host = down(&keys, n);
        let mut order: Vec<i32> = (0..n as i32).collect();
        order.sort_by_key(|&i| key_host[i as usize]); // stable, as thrust::stable_sort_by_key
        let sorted_keys: Vec<i32> = order.iter().map(|&i| key_host[i as usize]).collect();
        let sorted_keys = up(&sorted_keys);
        let sorted_idx = up(&order);
        let cell_start = up(&vec![0i32; cells]);
        let cell_end = up(&vec![0i32; cells]);
        let bounds = self
            .unified
            .get_function("compute_cell_bounds_kernel")
            .expect("kernel");
        // SAFETY: sorted keys hold n ints in [0, cells); cell buffers hold `cells`.
        unsafe {
            launch!(bounds<<<grid(n), BLOCK, 0, stream>>>(
                sorted_keys.as_device_ptr(), cell_start.as_device_ptr(),
                cell_end.as_device_ptr(), n as i32, cells as i32
            ))
            .expect("compute_cell_bounds_kernel");
        }

        let degrees = inputs.node_degrees.as_deref().map(up);
        let class_id = inputs.class_id.as_deref().map(up);
        let class_charge = inputs.class_charge.as_deref().map(up);
        let spring_scale = inputs.spring_scale.as_deref().map(up);
        let sssp = inputs.sssp.as_deref().map(up);
        let rank = inputs.node_rank.as_deref().map(up);
        let plane = inputs.node_plane.as_deref().map(up);
        let rest = inputs.edge_rest.as_deref().map(up);
        let cons = up(&inputs.constraints);

        // Per-node constraint CSR, ids ascending (constraint_index.rs).
        let mut per_node: Vec<Vec<i32>> = vec![Vec::new(); n];
        for (c, cd) in inputs.constraints.iter().enumerate() {
            for &ni in cd.node_idx.iter().take(cd.count.clamp(0, 4) as usize) {
                if ni >= 0 && (ni as usize) < n && !per_node[ni as usize].contains(&(c as i32)) {
                    per_node[ni as usize].push(c as i32);
                }
            }
        }
        let mut c_off = vec![0i32];
        let mut c_ids = Vec::new();
        for ids in &per_node {
            c_ids.extend_from_slice(ids);
            c_off.push(c_ids.len() as i32);
        }
        let c_off = up(&c_off);
        let c_ids = up(&c_ids);
        let view = if inputs.constraints.is_empty() {
            ConstraintIndexView::default()
        } else {
            ConstraintIndexView {
                offsets: c_off.as_device_ptr().as_raw(),
                ids: c_ids.as_device_ptr().as_raw(),
            }
        };
        let mut sym = self
            .unified
            .get_global::<ConstraintIndexView>(c"g_constraint_index")
            .expect("g_constraint_index");
        sym.copy_from(&view).expect("bind constraint index");
        self.set_u64(
            c"g_edge_rest_snapshot",
            rest.as_ref().map_or(0, |b| b.as_device_ptr().as_raw()),
        );

        let fxb = up(&vec![0f32; n]);
        let fyb = up(&vec![0f32; n]);
        let fzb = up(&vec![0f32; n]);
        let node_cf = up(&vec![0f32; n]);
        let null_f = DevicePointer::<f32>::null();
        let null_i = DevicePointer::<i32>::null();

        match which {
            ForceKernel::Main => {
                let k = self
                    .unified
                    .get_function("force_pass_kernel")
                    .expect("kernel");
                // SAFETY: every non-null pointer addresses a buffer of the length
                // the kernel indexes (n nodes, CSR slots, constraints); the grid
                // buffers were built for `dims` above.
                unsafe {
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                        fxb.as_device_ptr(), fyb.as_device_ptr(), fzb.as_device_ptr(),
                        cell_start.as_device_ptr(), cell_end.as_device_ptr(),
                        sorted_idx.as_device_ptr(), keys.as_device_ptr(), dims,
                        row.as_device_ptr(), col.as_device_ptr(), wts.as_device_ptr(),
                        n as i32, opt_ptr(&sssp), cons.as_device_ptr(),
                        inputs.constraints.len() as i32,
                        null_f, null_f, node_cf.as_device_ptr(),
                        opt_ptr(&class_id), opt_ptr(&class_charge), null_f,
                        opt_ptr(&degrees), opt_ptr(&spring_scale),
                        null_i, 0i32, opt_ptr(&rank), opt_ptr(&plane)
                    ))
                    .expect("force_pass_kernel");
                }
            }
            ForceKernel::Stability => {
                let ones = up(&vec![1f32; n]);
                let skip = up(&[0i32]);
                let k = self
                    .unified
                    .get_function("force_pass_with_stability_kernel")
                    .expect("kernel");
                // SAFETY: as for the main kernel; velocity buffers hold n floats
                // and the skip flag is one int.
                unsafe {
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                        ones.as_device_ptr(), ones.as_device_ptr(), ones.as_device_ptr(),
                        fxb.as_device_ptr(), fyb.as_device_ptr(), fzb.as_device_ptr(),
                        cell_start.as_device_ptr(), cell_end.as_device_ptr(),
                        sorted_idx.as_device_ptr(), keys.as_device_ptr(), dims,
                        row.as_device_ptr(), col.as_device_ptr(), wts.as_device_ptr(),
                        n as i32, opt_ptr(&sssp), cons.as_device_ptr(),
                        inputs.constraints.len() as i32, skip.as_device_ptr(),
                        opt_ptr(&degrees), opt_ptr(&spring_scale), node_cf.as_device_ptr(),
                        null_i, 0i32, opt_ptr(&rank), opt_ptr(&plane)
                    ))
                    .expect("force_pass_with_stability_kernel");
                }
            }
        }
        stream.synchronize().expect("force pass");
        // Unbind the device globals so no later launch reads freed buffers.
        self.set_u64(c"g_edge_rest_snapshot", 0);
        sym.copy_from(&ConstraintIndexView::default())
            .expect("unbind constraint index");
        let (x, y, z) = (down(&fxb, n), down(&fyb, n), down(&fzb, n));
        (0..n).map(|i| [x[i], y[i], z[i]]).collect()
    }

    /// `snapshot_edge_rest_kernel`: the per-CSR-slot edge lengths the hold
    /// overlay is armed with.
    pub fn snapshot_edge_rest(&self, fx: &Fixture) -> Vec<f32> {
        let n = fx.n();
        let stream = &self.stream;
        let csr = fx.csr();
        let px = up(&fx.pos.iter().map(|p| p[0]).collect::<Vec<_>>());
        let py = up(&fx.pos.iter().map(|p| p[1]).collect::<Vec<_>>());
        let pz = up(&fx.pos.iter().map(|p| p[2]).collect::<Vec<_>>());
        let row = up(&csr.offsets);
        let col = up(&csr.cols);
        let out = up(&vec![0f32; csr.cols.len()]);
        let k = self
            .unified
            .get_function("snapshot_edge_rest_kernel")
            .expect("kernel");
        // SAFETY: CSR rows bound every write into `out`, which holds one float per slot.
        unsafe {
            launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                row.as_device_ptr(), col.as_device_ptr(), out.as_device_ptr(), n as i32
            ))
            .expect("snapshot_edge_rest_kernel");
        }
        stream.synchronize().expect("sync");
        down(&out, csr.cols.len())
    }

    /// `degree_weighted_gravity_kernel` applied on top of `base` forces.
    pub fn degree_weighted_gravity(
        &self,
        fx: &Fixture,
        base: &Forces,
        degree_weight: &[f32],
        center_gravity_k: f32,
        peripheral_radius: f32,
        isolated_spring_k: f32,
    ) -> Forces {
        let n = fx.n();
        let stream = &self.stream;
        let px = up(&fx.pos.iter().map(|p| p[0]).collect::<Vec<_>>());
        let py = up(&fx.pos.iter().map(|p| p[1]).collect::<Vec<_>>());
        let pz = up(&fx.pos.iter().map(|p| p[2]).collect::<Vec<_>>());
        let fxb = up(&base.iter().map(|f| f[0]).collect::<Vec<_>>());
        let fyb = up(&base.iter().map(|f| f[1]).collect::<Vec<_>>());
        let fzb = up(&base.iter().map(|f| f[2]).collect::<Vec<_>>());
        let dw = up(degree_weight);
        let k = self
            .unified
            .get_function("degree_weighted_gravity_kernel")
            .expect("kernel");
        // SAFETY: all buffers hold n entries.
        unsafe {
            launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                fxb.as_device_ptr(), fyb.as_device_ptr(), fzb.as_device_ptr(),
                dw.as_device_ptr(), n as i32, center_gravity_k, peripheral_radius,
                isolated_spring_k
            ))
            .expect("degree_weighted_gravity_kernel");
        }
        stream.synchronize().expect("sync");
        let (x, y, z) = (down(&fxb, n), down(&fyb, n), down(&fzb, n));
        (0..n).map(|i| [x[i], y[i], z[i]]).collect()
    }

    /// `cluster_cohesion_kernel` added onto zero forces.
    pub fn cluster_cohesion(
        &self,
        fx: &Fixture,
        centroids: &[[f32; 3]],
        assignment: &[i32],
        strength: f32,
    ) -> Forces {
        let n = fx.n();
        let stream = &self.stream;
        let px = up(&fx.pos.iter().map(|p| p[0]).collect::<Vec<_>>());
        let py = up(&fx.pos.iter().map(|p| p[1]).collect::<Vec<_>>());
        let pz = up(&fx.pos.iter().map(|p| p[2]).collect::<Vec<_>>());
        let fxb = up(&vec![0f32; n]);
        let fyb = up(&vec![0f32; n]);
        let fzb = up(&vec![0f32; n]);
        let cx = up(&centroids.iter().map(|c| c[0]).collect::<Vec<_>>());
        let cy = up(&centroids.iter().map(|c| c[1]).collect::<Vec<_>>());
        let cz = up(&centroids.iter().map(|c| c[2]).collect::<Vec<_>>());
        let asg = up(assignment);
        let k = self
            .unified
            .get_function("cluster_cohesion_kernel")
            .expect("kernel");
        // SAFETY: node buffers hold n entries; centroid buffers hold
        // `centroids.len()` and the kernel range-checks the assignment.
        unsafe {
            launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(),
                fxb.as_device_ptr(), fyb.as_device_ptr(), fzb.as_device_ptr(),
                cx.as_device_ptr(), cy.as_device_ptr(), cz.as_device_ptr(),
                asg.as_device_ptr(), n as i32, centroids.len() as i32, strength
            ))
            .expect("cluster_cohesion_kernel");
        }
        stream.synchronize().expect("sync");
        let (x, y, z) = (down(&fxb, n), down(&fyb, n), down(&fzb, n));
        (0..n).map(|i| [x[i], y[i], z[i]]).collect()
    }
}

// ---------------------------------------------------------------------------
// Semantic module
// ---------------------------------------------------------------------------

/// One semantic force kernel, or the server's whole `SemanticGpu::apply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticKernel {
    Dag,
    TypeCluster,
    Collision,
    AttributeSpring,
    Physicality,
    Role,
    Maturity,
    Dynamic,
    /// gather, zero, three centroid tables, all eight kernels, scatter-add.
    Production,
}

/// Centroid tables supplied to a cluster kernel run in isolation, so its law
/// is tested against known centroids rather than the atomics that build them.
#[derive(Clone, Debug, Default)]
pub struct Centroids {
    pub types: Vec<[f32; 3]>,
    pub physicality: Vec<[f32; 3]>,
    pub role: Vec<[f32; 3]>,
}

fn flat3(v: &[[f32; 3]], slots: usize) -> Vec<f32> {
    let mut out = vec![0f32; slots * 3];
    for (i, c) in v.iter().take(slots).enumerate() {
        out[i * 3..i * 3 + 3].copy_from_slice(c);
    }
    out
}

impl Gpu {
    fn set_semantic(&self, cfg: &SemanticConfig, table: &[Relationship], enabled: bool) {
        let mut c = self
            .semantic
            .get_global::<[u32; 35]>(c"c_semantic_config")
            .expect("c_semantic_config");
        c.copy_from(&cfg.image()).expect("upload semantic config");
        let mut d = self
            .semantic
            .get_global::<[u32; DYNAMIC_WORDS]>(c"c_dynamic_relationships")
            .expect("c_dynamic_relationships");
        d.copy_from(&dynamic_image(table, enabled))
            .expect("upload relationship table");
    }

    /// Read the configuration back through the device's own struct view
    /// (`sf_config_probe`), so a layout mismatch fails loudly.
    pub fn semantic_probe(&self) -> [f32; 15] {
        let out = up(&[0f32; 15]);
        let k = self
            .semantic
            .get_function("sf_config_probe")
            .expect("kernel");
        let stream = &self.stream;
        // SAFETY: one thread writes 15 floats into a 15-float buffer.
        unsafe {
            launch!(k<<<1, 1, 0, stream>>>(out.as_device_ptr())).expect("sf_config_probe");
        }
        stream.synchronize().expect("sync");
        let v = down(&out, 15);
        v.try_into().expect("15 floats")
    }

    /// Run a semantic kernel (or the production sequence) over `fx` from zero
    /// forces and return what it adds per node.
    pub fn semantic(
        &self,
        fx: &Fixture,
        cfg: &SemanticConfig,
        table: &[Relationship],
        dynamic_enabled: bool,
        centroids: &Centroids,
        which: SemanticKernel,
    ) -> Forces {
        let n = fx.n();
        let e = fx.edges.len();
        let stream = &self.stream;
        self.set_semantic(cfg, table, dynamic_enabled);
        let m = &self.semantic;

        let pos = up(&flat3(&fx.pos, n));
        let forces = up(&vec![0f32; n * 3]);
        let types = up(&fx.node_type);
        let levels = up(&fx.hierarchy_level);
        let phys = up(&fx.physicality);
        let role = up(&fx.role);
        let maturity = up(&fx.maturity);
        let cross = up(&fx.cross_domain);
        let src = up(&fx.edges.iter().map(|e| e.src as i32).collect::<Vec<_>>());
        let dst = up(&fx.edges.iter().map(|e| e.dst as i32).collect::<Vec<_>>());
        let ety = up(&fx.edges.iter().map(|e| e.kind).collect::<Vec<_>>());
        let ewt = up(&fx.edges.iter().map(|e| e.weight).collect::<Vec<_>>());
        let type_c = up(&flat3(&centroids.types, TYPE_SLOTS));
        let phys_c = up(&flat3(&centroids.physicality, TYPE_SLOTS));
        let role_c = up(&flat3(&centroids.role, TYPE_SLOTS));
        let ni = n as i32;
        let ei = e as i32;
        let (p, f) = (pos.as_device_ptr().cast::<f32>(), forces.as_device_ptr());
        let k = |name: &str| m.get_function(name).expect("semantic kernel");

        // SAFETY (all launches below): node buffers hold n entries, edge buffers
        // e entries, centroid tables TYPE_SLOTS float3s, positions and forces
        // 3n floats; kernels only read positions and add into forces.
        unsafe {
            match which {
                SemanticKernel::Dag => {
                    let k = k("apply_dag_force");
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        levels.as_device_ptr(), types.as_device_ptr(), p, f, ni))
                    .expect("apply_dag_force");
                }
                SemanticKernel::TypeCluster => {
                    let k = k("apply_type_cluster_force");
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        types.as_device_ptr(), type_c.as_device_ptr(), p, f, ni,
                        TYPE_SLOTS as i32))
                    .expect("apply_type_cluster_force");
                }
                SemanticKernel::Collision => {
                    let k = k("apply_collision_force");
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        DevicePointer::<f32>::null(), p, f, ni))
                    .expect("apply_collision_force");
                }
                SemanticKernel::AttributeSpring => {
                    if e > 0 {
                        let k = k("apply_attribute_spring_force");
                        launch!(k<<<grid(e), BLOCK, 0, stream>>>(
                            src.as_device_ptr(), dst.as_device_ptr(), ewt.as_device_ptr(),
                            ety.as_device_ptr(), p, f, ei))
                        .expect("apply_attribute_spring_force");
                    }
                }
                SemanticKernel::Physicality => {
                    let k = k("apply_physicality_cluster_force");
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        phys.as_device_ptr(), phys_c.as_device_ptr(), p, f, ni))
                    .expect("apply_physicality_cluster_force");
                }
                SemanticKernel::Role => {
                    let k = k("apply_role_cluster_force");
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        role.as_device_ptr(), role_c.as_device_ptr(), p, f, ni))
                    .expect("apply_role_cluster_force");
                }
                SemanticKernel::Maturity => {
                    let k = k("apply_maturity_layout_force");
                    launch!(k<<<grid(n), BLOCK, 0, stream>>>(
                        maturity.as_device_ptr(), p, f, ni))
                    .expect("apply_maturity_layout_force");
                }
                SemanticKernel::Dynamic => {
                    if e > 0 {
                        let k = k("apply_dynamic_relationship_force");
                        launch!(k<<<grid(e), BLOCK, 0, stream>>>(
                            src.as_device_ptr(), dst.as_device_ptr(), ety.as_device_ptr(),
                            cross.as_device_ptr(), p, f, ei))
                        .expect("apply_dynamic_relationship_force");
                    }
                }
                SemanticKernel::Production => {
                    return self.production_sequence(fx, dynamic_enabled);
                }
            }
        }
        stream.synchronize().expect("semantic kernel");
        let v = down(&forces, n * 3);
        (0..n)
            .map(|i| [v[i * 3], v[i * 3 + 1], v[i * 3 + 2]])
            .collect()
    }

    /// `SemanticGpu::apply` step for step: gather SoA positions into float3,
    /// zero the scratch forces, rebuild the three centroid tables (zeroed,
    /// atomically summed, divided), run the eight kernels in server order,
    /// then scatter-add into SoA force buffers that start at zero.
    fn production_sequence(&self, fx: &Fixture, dynamic_enabled: bool) -> Forces {
        let n = fx.n();
        let e = fx.edges.len();
        let ni = n as i32;
        let stream = &self.stream;
        let m = &self.semantic;
        let k = |name: &str| m.get_function(name).expect("semantic kernel");

        let px = up(&fx.pos.iter().map(|p| p[0]).collect::<Vec<_>>());
        let py = up(&fx.pos.iter().map(|p| p[1]).collect::<Vec<_>>());
        let pz = up(&fx.pos.iter().map(|p| p[2]).collect::<Vec<_>>());
        let types = up(&fx.node_type);
        let levels = up(&fx.hierarchy_level);
        let phys = up(&fx.physicality);
        let role = up(&fx.role);
        let maturity = up(&fx.maturity);
        let cross = up(&fx.cross_domain);
        let src = up(&fx.edges.iter().map(|e| e.src as i32).collect::<Vec<_>>());
        let dst = up(&fx.edges.iter().map(|e| e.dst as i32).collect::<Vec<_>>());
        let ety = up(&fx.edges.iter().map(|e| e.kind).collect::<Vec<_>>());
        let ewt = up(&fx.edges.iter().map(|e| e.weight).collect::<Vec<_>>());
        let tables: Vec<(DeviceBuffer<f32>, DeviceBuffer<i32>)> = (0..3)
            .map(|_| (up(&[1f32; TYPE_SLOTS * 3]), up(&[1i32; TYPE_SLOTS])))
            .collect();
        // The float3 scratch arrays, deliberately allocated non-zero so a
        // missing gather or zero step would show in the result.
        let pos = up(&vec![f32::NAN; n * 3]);
        let forces = up(&vec![f32::NAN; n * 3]);
        let out = [up(&vec![0f32; n]), up(&vec![0f32; n]), up(&vec![0f32; n])];
        let p = pos.as_device_ptr();
        let pf3 = p.cast::<f32>();
        let f = forces.as_device_ptr();

        // SAFETY (whole block): as in `semantic`; the centroid tables hold
        // TYPE_SLOTS entries and are zeroed by `sf_zero_words` before use (they
        // are allocated non-zero so a missing reset would show).
        unsafe {
            let gather = k("sf_gather_positions");
            launch!(gather<<<grid(n), BLOCK, 0, stream>>>(
                px.as_device_ptr(), py.as_device_ptr(), pz.as_device_ptr(), p, ni))
            .expect("gather");
            let zero_f = k("sf_zero_forces");
            launch!(zero_f<<<grid(n), BLOCK, 0, stream>>>(f, ni)).expect("zero forces");
            let zero_w = k("sf_zero_words");
            let calc = k("calculate_type_centroids");
            let fin = k("finalize_type_centroids");
            for (family, (cent, count)) in [&types, &phys, &role].iter().zip(&tables) {
                launch!(zero_w<<<grid(TYPE_SLOTS * 3), BLOCK, 0, stream>>>(
                    cent.as_device_ptr().cast::<u32>(), (TYPE_SLOTS * 3) as i32))
                .expect("zero centroids");
                launch!(zero_w<<<grid(TYPE_SLOTS), BLOCK, 0, stream>>>(
                    count.as_device_ptr().cast::<u32>(), TYPE_SLOTS as i32))
                .expect("zero counts");
                launch!(calc<<<grid(n), BLOCK, 0, stream>>>(
                    family.as_device_ptr(), pf3, cent.as_device_ptr(),
                    count.as_device_ptr(), ni, TYPE_SLOTS as i32))
                .expect("centroids");
                launch!(fin<<<grid(TYPE_SLOTS), BLOCK, 0, stream>>>(
                    cent.as_device_ptr(), count.as_device_ptr(), TYPE_SLOTS as i32))
                .expect("finalise centroids");
            }
            let k_dag = k("apply_dag_force");
            launch!(k_dag<<<grid(n), BLOCK, 0, stream>>>(
                levels.as_device_ptr(), types.as_device_ptr(), pf3, f, ni))
            .expect("dag");
            let k_type = k("apply_type_cluster_force");
            launch!(k_type<<<grid(n), BLOCK, 0, stream>>>(
                types.as_device_ptr(), tables[0].0.as_device_ptr(), pf3, f, ni,
                TYPE_SLOTS as i32))
            .expect("type");
            let k_col = k("apply_collision_force");
            launch!(k_col<<<grid(n), BLOCK, 0, stream>>>(
                DevicePointer::<f32>::null(), pf3, f, ni))
            .expect("collision");
            let k_phys = k("apply_physicality_cluster_force");
            launch!(k_phys<<<grid(n), BLOCK, 0, stream>>>(
                phys.as_device_ptr(), tables[1].0.as_device_ptr(), pf3, f, ni))
            .expect("physicality");
            let k_role = k("apply_role_cluster_force");
            launch!(k_role<<<grid(n), BLOCK, 0, stream>>>(
                role.as_device_ptr(), tables[2].0.as_device_ptr(), pf3, f, ni))
            .expect("role");
            let k_mat = k("apply_maturity_layout_force");
            launch!(k_mat<<<grid(n), BLOCK, 0, stream>>>(
                maturity.as_device_ptr(), pf3, f, ni))
            .expect("maturity");
            if e > 0 {
                let ei = e as i32;
                let k_spring = k("apply_attribute_spring_force");
                launch!(k_spring<<<grid(e), BLOCK, 0, stream>>>(
                    src.as_device_ptr(), dst.as_device_ptr(), ewt.as_device_ptr(),
                    ety.as_device_ptr(), pf3, f, ei))
                .expect("attribute spring");
                if dynamic_enabled {
                    let k_dyn = k("apply_dynamic_relationship_force");
                    launch!(k_dyn<<<grid(e), BLOCK, 0, stream>>>(
                        src.as_device_ptr(), dst.as_device_ptr(), ety.as_device_ptr(),
                        cross.as_device_ptr(), pf3, f, ei))
                    .expect("dynamic");
                }
            }
            let scatter = k("sf_scatter_add_forces");
            launch!(scatter<<<grid(n), BLOCK, 0, stream>>>(
                f, out[0].as_device_ptr(), out[1].as_device_ptr(), out[2].as_device_ptr(), ni))
            .expect("scatter");
        }
        stream.synchronize().expect("production sequence");
        let (x, y, z) = (down(&out[0], n), down(&out[1], n), down(&out[2], n));
        (0..n).map(|i| [x[i], y[i], z[i]]).collect()
    }
}
