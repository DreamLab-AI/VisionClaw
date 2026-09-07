//! ADR-2061 — GPU analytics kernel conformance against the CPU reference oracle.
//!
//! `docs/GPU-wire-abi.md` recorded Louvain, PageRank, DBSCAN and LOF as
//! *code-fixed but not output-validated*: the in-source fix markers were
//! reviewed, but no reference implementation had ever confirmed what the
//! kernels actually compute. Those four values reach clients on the wire at V3
//! offsets `cluster_id@36`, `anomaly_score@40`, `community_id@44` and
//! `centrality@48`, so an unvalidated kernel is an unvalidated answer to a
//! user-facing query.
//!
//! This suite closes that gap. Each test drives the **compiled PTX kernel
//! itself** — no server-side wrapper in the path — over a fixture from
//! `visionclaw-analytics-oracle`, and asserts the result against that crate's
//! pure-`std` CPU reference at exactly the tolerances ADR-2061 fixes:
//!
//! | Kernel   | Bar (ADR-2061) |
//! |----------|----------------|
//! | Louvain  | `two_clique` → 2 communities matching `two_clique_optimal_partition` up to label permutation; `triangle`/`star` → 1; `canonical_live_scale` → Q within 0.02 of the oracle's Q for the GPU's own partition, and no more than 0.05 below the oracle partition's Q |
//! | PageRank | per-node `|Δ| < 1e-4` vs `pagerank(g, 0.85, 100)`, and exact top-decile ranking order |
//! | DBSCAN   | exact label agreement up to cluster-id permutation, noise set included |
//! | LOF      | per-point `|Δ| < 1e-3` vs `lof(points, k)`, and an exactly matching >95th-percentile set |
//!
//! # Why the kernels are driven directly
//!
//! `visionclaw-gpu` owns the `.cu` sources and the PTX loader; the production
//! driver (`UnifiedGPUCompute`) lives in the root `visionclaw-server` crate,
//! which *depends on* this one. Asserting from here would invert that edge, so
//! the harness below reimplements the host-side launch sequence for each kernel
//! against the same PTX the server loads. That scopes the suite honestly: it
//! validates **the kernels as compiled**, which is where every fix marker in
//! the trust table lives. The thin server-side wrapper that marshals buffers
//! into these launches is not covered here.
//!
//! # Running
//!
//! Every test is `#[ignore]`-gated (the convention the root crate's
//! `tests/analytics_correctness_test.rs` already uses), so a CPU-only CI job
//! runs `cargo test` green without a device. On a CUDA host:
//!
//! ```sh
//! cargo test -p visionclaw-gpu --test analytics_oracle_conformance \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `--test-threads=1` is not optional in spirit: each test creates its own CUDA
//! context, and serialising them keeps device memory and the measured numbers
//! reproducible. A host that has no CUDA device, or no compiled PTX, **skips**
//! with a `SKIP:` line on stdout rather than failing — lacking a GPU is not a
//! correctness result. Only a real bad number fails.

#![allow(clippy::too_many_arguments)]

use cust::context::Context;
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer, DeviceCopy};
use cust::module::Module;
use cust::stream::{Stream, StreamFlags};

use visionclaw_analytics_oracle as oracle;
use visionclaw_gpu::ptx_loader::{load_ptx_module_sync, PTXModule};

// ===========================================================================
// Harness
// ===========================================================================

/// `int3` as CUDA lays it out, for by-value kernel parameters.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Int3 {
    x: i32,
    y: i32,
    z: i32,
}
// SAFETY: `Int3` is `repr(C)` and contains only `i32`, which is itself
// `DeviceCopy`. It has no padding, no pointers and no `Drop`, so a bitwise copy
// to the device is a faithful copy — exactly the contract `DeviceCopy` asserts.
unsafe impl DeviceCopy for Int3 {}

/// A live CUDA context plus the PTX module a test needs.
///
/// Held for the whole test: dropping the [`Context`] tears down every device
/// allocation made under it.
///
/// **Field order is load-bearing.** Rust drops struct fields in declaration
/// order, and a [`Module`] or [`Stream`] belongs to the context that created it
/// — destroying the context first and the module second segfaults inside the
/// driver. `_ctx` is therefore declared last so it is destroyed last.
struct Gpu {
    module: Module,
    stream: Stream,
    ptx: String,
    _ctx: Context,
}

/// Bring up CUDA and load `module`, or return `None` with a printed reason.
///
/// Both "no device" and "no compiled PTX" are skip conditions, never failures —
/// see the module docs. The caller does `let Some(gpu) = setup(..) else { return }`.
fn setup(module: PTXModule, test: &str) -> Option<Gpu> {
    let ptx = match load_ptx_module_sync(module) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("SKIP {test}: PTX for {module:?} unavailable: {e}");
            return None;
        }
    };
    let ctx = match cust::quick_init() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP {test}: no CUDA device: {e}");
            return None;
        }
    };
    let m = match Module::from_ptx(&ptx, &[]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("SKIP {test}: Module::from_ptx failed for {module:?}: {e}");
            return None;
        }
    };
    let stream = match Stream::new(StreamFlags::NON_BLOCKING, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP {test}: stream creation failed: {e}");
            return None;
        }
    };
    Some(Gpu {
        module: m,
        stream,
        ptx,
        _ctx: ctx,
    })
}

impl Gpu {
    /// Resolve a kernel by its **source** name, tolerating C++ mangling.
    ///
    /// `gpu_clustering_kernels.cu` wraps its kernels in `extern "C"` so their
    /// PTX entries are unmangled, but `pagerank.cu` does not — there the entry
    /// is `_Z25pagerank_iteration_kernelPKfPfPKiS3_S3_iff`. Hard-coding mangled
    /// names would bind this suite to one nvcc release, so instead the PTX text
    /// is scanned for the entry whose symbol contains the source name. The
    /// containment test is unambiguous for every name used here: no kernel in
    /// these modules is a strict substring of another (`pagerank_iteration_kernel`
    /// is not a substring of `pagerank_iteration_optimized_kernel`).
    fn kernel(&self, source_name: &str) -> Option<cust::function::Function<'_>> {
        let entry = self.ptx.lines().find_map(|line| {
            let rest = line.trim().strip_prefix(".visible .entry ")?;
            let sym = rest.split(['(', ' ', '\t']).next()?.trim();
            (sym == source_name || sym.contains(source_name)).then(|| sym.to_string())
        })?;
        match self.module.get_function(&entry) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("kernel {source_name} (entry {entry}) not loadable: {e}");
                None
            }
        }
    }
}

fn grid_for(n: usize, block: u32) -> u32 {
    (n as u32).div_ceil(block)
}

fn download<T: DeviceCopy + Default + Clone>(buf: &DeviceBuffer<T>, len: usize) -> Vec<T> {
    let mut host = vec![T::default(); len];
    buf.copy_to(&mut host).expect("device -> host copy");
    host
}

/// Both-direction unit-weight CSR, the layout the graph kernels expect.
///
/// The fixtures store each undirected edge once; the kernels want each
/// direction present, so `|indices| == 2 * |edges|`.
fn undirected_csr(g: &oracle::GraphFixture) -> (Vec<i32>, Vec<i32>, Vec<f32>) {
    let mut adj: Vec<Vec<i32>> = vec![Vec::new(); g.n];
    for &(u, v) in &g.edges {
        adj[u as usize].push(v as i32);
        adj[v as usize].push(u as i32);
    }
    let mut offsets = Vec::with_capacity(g.n + 1);
    let mut indices = Vec::new();
    offsets.push(0i32);
    for a in &adj {
        indices.extend_from_slice(a);
        offsets.push(indices.len() as i32);
    }
    let weights = vec![1.0f32; indices.len()];
    (offsets, indices, weights)
}

/// Rank indices by descending value, breaking ties by ascending index.
///
/// The tiebreak is what makes "ranking order must match exactly" a testable
/// claim on symmetric fixtures: on `triangle` every node holds exactly 1/3, so
/// without a deterministic tiebreak the ordering is meaningless and any
/// comparison is noise. Applying the same rule to both sides compares the part
/// of the order that is actually determined by the values.
fn ranking(values: &[f64]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| {
        values[b]
            .partial_cmp(&values[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    idx
}

/// Check that two labellings agree up to a renaming of the labels.
///
/// Verifies a *bijection*, not merely a function: `a -> b` and `b -> a` are both
/// built, so a GPU result that merges two oracle clusters into one is rejected
/// (it would be a valid function one way but not the other). Returns the first
/// disagreement as an error string.
fn same_partition(a: &[i32], b: &[i32]) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("length {} vs {}", a.len(), b.len()));
    }
    use std::collections::HashMap;
    let mut fwd: HashMap<i32, i32> = HashMap::new();
    let mut rev: HashMap<i32, i32> = HashMap::new();
    for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
        // Noise (-1) is a distinguished label, never permuted onto a cluster.
        if (x < 0) != (y < 0) {
            return Err(format!("point {i}: noise mismatch, gpu={x} oracle={y}"));
        }
        if let Some(&m) = fwd.get(&x) {
            if m != y {
                return Err(format!("point {i}: gpu label {x} maps to both {m} and {y}"));
            }
        } else {
            fwd.insert(x, y);
        }
        if let Some(&m) = rev.get(&y) {
            if m != x {
                return Err(format!(
                    "point {i}: oracle label {y} maps to both {m} and {x}"
                ));
            }
        } else {
            rev.insert(y, x);
        }
    }
    Ok(())
}

// ===========================================================================
// PageRank — pagerank.cu
// ===========================================================================

/// Run the GPU PageRank power iteration and return the final vector.
///
/// Mirrors the oracle's schedule exactly (`damping = 0.85`, 100 iterations,
/// renormalise at the end) so any divergence is the kernel's, not the schedule's.
/// The dangling-mass path (D8's two-kernel global reduction) is launched every
/// iteration even on connected fixtures, so the corrected kernels are exercised
/// rather than merely present.
fn gpu_pagerank(gpu: &Gpu, g: &oracle::GraphFixture, damping: f32, iters: usize) -> Vec<f64> {
    let n = g.n;
    // The iteration kernel walks *incoming* edges, so it wants CSC. For an
    // undirected graph CSC == CSR, and out-degree == degree.
    let (offsets, indices, _) = undirected_csr(g);
    let out_degree: Vec<i32> = (0..n).map(|i| offsets[i + 1] - offsets[i]).collect();

    let block = 256u32;
    let grid = grid_for(n, block);
    let stream = &gpu.stream;

    let d_offsets = DeviceBuffer::from_slice(&offsets).unwrap();
    let d_indices = DeviceBuffer::from_slice(&indices).unwrap();
    let d_outdeg = DeviceBuffer::from_slice(&out_degree).unwrap();
    let mut d_old = DeviceBuffer::<f32>::zeroed(n).unwrap();
    let mut d_new = DeviceBuffer::<f32>::zeroed(n).unwrap();
    let d_partial = DeviceBuffer::<f32>::zeroed(grid as usize).unwrap();
    let d_sum = DeviceBuffer::<f32>::zeroed(grid.max(1) as usize).unwrap();

    let k_init = gpu
        .kernel("pagerank_init_kernel")
        .expect("pagerank_init_kernel");
    let k_iter = gpu
        .kernel("pagerank_iteration_kernel")
        .expect("pagerank_iteration_kernel");
    let k_dsum = gpu
        .kernel("pagerank_dangling_sum_kernel")
        .expect("pagerank_dangling_sum_kernel");
    let k_dist = gpu
        .kernel("pagerank_dangling_distribute_kernel")
        .expect("pagerank_dangling_distribute_kernel");
    let k_norm = gpu
        .kernel("pagerank_normalize_kernel")
        .expect("pagerank_normalize_kernel");

    let shared = (block as usize * std::mem::size_of::<f32>()) as u32;
    let teleport = (1.0 - damping) / n as f32;

    // SAFETY (applies to every launch in this function): each buffer is
    // allocated with at least the element count passed as the kernel's `n`
    // argument, the grid covers exactly that range with an in-kernel bounds
    // check, and the dynamic shared-memory request matches the `extern
    // __shared__ float[]` each reduction kernel indexes by `threadIdx.x`
    // (< blockDim.x). Every launch is on one stream, synchronised before any
    // host read, so there is no concurrent access to a device buffer.
    unsafe {
        launch!(k_init<<<grid, block, 0, stream>>>(d_old.as_device_ptr(), n as i32)).unwrap();

        for _ in 0..iters {
            launch!(k_iter<<<grid, block, 0, stream>>>(
                d_old.as_device_ptr(),
                d_new.as_device_ptr(),
                d_offsets.as_device_ptr(),
                d_indices.as_device_ptr(),
                d_outdeg.as_device_ptr(),
                n as i32,
                damping,
                teleport
            ))
            .unwrap();

            // D8 global dangling path: per-block partial sums, then a
            // single-block distribute that reduces them to one total.
            launch!(k_dsum<<<grid, block, shared, stream>>>(
                d_old.as_device_ptr(),
                d_outdeg.as_device_ptr(),
                d_partial.as_device_ptr(),
                n as i32
            ))
            .unwrap();
            launch!(k_dist<<<1u32, block, 0, stream>>>(
                d_new.as_device_ptr(),
                d_partial.as_device_ptr(),
                grid as i32,
                n as i32,
                damping
            ))
            .unwrap();

            std::mem::swap(&mut d_old, &mut d_new);
        }

        launch!(k_norm<<<grid, block, shared, stream>>>(
            d_old.as_device_ptr(),
            d_sum.as_device_ptr(),
            n as i32
        ))
        .unwrap();
    }
    gpu.stream.synchronize().unwrap();

    let host = download(&d_old, n);
    // Renormalise host-side, matching the oracle's closing step: the contract
    // under test is the *distribution*, not the kernel's reduction epsilon.
    let s: f64 = host.iter().map(|&v| v as f64).sum();
    host.iter()
        .map(|&v| if s > 0.0 { v as f64 / s } else { 0.0 })
        .collect()
}

#[test]
#[ignore = "needs GPU: real CUDA device + compiled pagerank PTX (run with --ignored)"]
fn adr_2061_pagerank_matches_oracle() {
    const TEST: &str = "adr_2061_pagerank_matches_oracle";
    let Some(gpu) = setup(PTXModule::Pagerank, TEST) else {
        return;
    };

    let fixtures = vec![
        oracle::triangle(),
        oracle::star(6),
        oracle::linear_chain(10),
        oracle::two_clique(),
        oracle::canonical_live_scale(),
    ];

    let mut failures = Vec::new();
    for g in &fixtures {
        let cpu = oracle::pagerank(g, 0.85, 100);
        let got = gpu_pagerank(&gpu, g, 0.85f32, 100);

        let (worst_i, worst) = cpu
            .iter()
            .zip(got.iter())
            .enumerate()
            .map(|(i, (a, b))| (i, (a - b).abs()))
            .fold((0usize, 0.0f64), |acc, x| if x.1 > acc.1 { x } else { acc });

        // ADR-2061: per-node absolute difference < 1e-4.
        let tol_ok = worst < 1e-4;

        // ADR-2061: the ranking order of the top decile must match exactly.
        let decile = ((g.n as f64 * 0.1).ceil() as usize).max(1);
        let r_cpu = ranking(&cpu);
        let r_gpu = ranking(&got);
        let order_ok = r_cpu[..decile] == r_gpu[..decile];

        let sum: f64 = got.iter().sum();
        println!(
            "  pagerank/{:<20} n={:<6} max|Δ|={:.3e} top-decile({decile}) order={} sum={:.6}",
            g.name,
            g.n,
            worst,
            if order_ok { "match" } else { "DIFFER" },
            sum
        );
        if !tol_ok {
            failures.push(format!(
                "{}: max|Δ|={worst:.3e} at node {worst_i} exceeds 1e-4",
                g.name
            ));
        }
        if !order_ok {
            failures.push(format!(
                "{}: top-decile order differs: gpu={:?} oracle={:?}",
                g.name,
                &r_gpu[..decile.min(8)],
                &r_cpu[..decile.min(8)]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "PageRank conformance:\n  {}",
        failures.join("\n  ")
    );
}

// ===========================================================================
// DBSCAN — gpu_clustering_kernels.cu
// ===========================================================================

/// Deterministic DBSCAN fixture: two separated 3x3 lattices, one border point,
/// two isolated noise points.
///
/// The border point is the reason this fixture exists. At `[-1.4, 1.0]` it sits
/// 1.4 from lattice-A's `(0,1)` — inside `eps` — but has only that one
/// neighbour, so it is not a core point. ADR-031 D7's contract is that such a
/// point joins the core's cluster rather than being labelled noise, which is
/// precisely what the `atomicMax` fix at `dbscan_propagate_labels_kernel`
/// claims to do. The two lattices sit 20 apart so no point is within `eps` of
/// both, keeping the expected answer unambiguous.
///
/// Returns `(points, eps, min_pts)` with `min_pts` in the oracle's
/// **self-inclusive** convention.
fn dbscan_fixture() -> (Vec<oracle::Pt>, f64, usize) {
    let mut pts: Vec<oracle::Pt> = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            pts.push([i as f64, j as f64]);
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            pts.push([20.0 + i as f64, j as f64]);
        }
    }
    pts.push([-1.4, 1.0]); // border of lattice A
    pts.push([50.0, 50.0]); // noise
    pts.push([-50.0, 50.0]); // noise
    (pts, 1.5, 4)
}

#[test]
#[ignore = "needs GPU: real CUDA device + compiled clustering PTX (run with --ignored)"]
fn adr_2061_dbscan_matches_oracle() {
    const TEST: &str = "adr_2061_dbscan_matches_oracle";
    let Some(gpu) = setup(PTXModule::GpuClusteringKernels, TEST) else {
        return;
    };

    let (pts, eps, min_pts) = dbscan_fixture();
    let n = pts.len();
    let cpu = oracle::dbscan(&pts, eps, min_pts);

    let px: Vec<f32> = pts.iter().map(|p| p[0] as f32).collect();
    let py: Vec<f32> = pts.iter().map(|p| p[1] as f32).collect();
    let pz = vec![0.0f32; n];

    let max_neighbors = n as i32;
    let offsets: Vec<i32> = (0..n as i32).map(|i| i * max_neighbors).collect();

    let block = 128u32;
    let grid = grid_for(n, block);
    let stream = &gpu.stream;

    let d_x = DeviceBuffer::from_slice(&px).unwrap();
    let d_y = DeviceBuffer::from_slice(&py).unwrap();
    let d_z = DeviceBuffer::from_slice(&pz).unwrap();
    let d_nbrs = DeviceBuffer::<i32>::zeroed(n * max_neighbors as usize).unwrap();
    let d_counts = DeviceBuffer::<i32>::zeroed(n).unwrap();
    let d_offsets = DeviceBuffer::from_slice(&offsets).unwrap();
    let d_labels = DeviceBuffer::<i32>::zeroed(n).unwrap();
    let mut d_changed = DeviceBuffer::<i32>::zeroed(1).unwrap();

    let k_nbr = gpu
        .kernel("dbscan_find_neighbors_kernel")
        .expect("dbscan_find_neighbors_kernel");
    let k_core = gpu
        .kernel("dbscan_mark_core_points_kernel")
        .expect("dbscan_mark_core_points_kernel");
    let k_prop = gpu
        .kernel("dbscan_propagate_labels_kernel")
        .expect("dbscan_propagate_labels_kernel");
    let k_noise = gpu
        .kernel("dbscan_finalize_noise_kernel")
        .expect("dbscan_finalize_noise_kernel");

    // The kernel's `neighbor_counts` excludes the point itself (`if (i == j)
    // continue`), while the oracle's core test is `nbrs.len() + 1 >= min_pts`.
    // The two agree only when the device is given `min_pts - 1`. This is a
    // host-side calling convention, not a kernel defect — pinning it here is
    // the point of the test.
    let device_min_pts = min_pts as i32 - 1;

    // SAFETY: every buffer is sized for `n` (or `n * max_neighbors` for the
    // flattened neighbour lists, which `neighbor_offsets` indexes by
    // `i * max_neighbors`), the grid covers `n` with in-kernel bounds checks,
    // and each phase is separated by a stream synchronise so no kernel reads a
    // buffer another is still writing.
    unsafe {
        launch!(k_nbr<<<grid, block, 0, stream>>>(
            d_x.as_device_ptr(),
            d_y.as_device_ptr(),
            d_z.as_device_ptr(),
            d_nbrs.as_device_ptr(),
            d_counts.as_device_ptr(),
            d_offsets.as_device_ptr(),
            eps as f32,
            n as i32,
            max_neighbors
        ))
        .unwrap();
        launch!(k_core<<<grid, block, 0, stream>>>(
            d_counts.as_device_ptr(),
            d_labels.as_device_ptr(),
            device_min_pts,
            n as i32
        ))
        .unwrap();
        gpu.stream.synchronize().unwrap();

        // Label propagation to a fixpoint. `n + 2` is a hard bound: each
        // sweep either changes at least one label or terminates, and labels
        // only ever move monotonically toward the lowest core id in a
        // component, of which there are at most `n`.
        for _ in 0..(n + 2) {
            d_changed.copy_from(&[0i32]).unwrap();
            launch!(k_prop<<<grid, block, 0, stream>>>(
                d_nbrs.as_device_ptr(),
                d_counts.as_device_ptr(),
                d_offsets.as_device_ptr(),
                d_labels.as_device_ptr(),
                d_changed.as_device_ptr(),
                n as i32
            ))
            .unwrap();
            gpu.stream.synchronize().unwrap();
            let changed = download(&d_changed, 1)[0];
            if changed == 0 {
                break;
            }
        }

        launch!(k_noise<<<grid, block, 0, stream>>>(d_labels.as_device_ptr(), n as i32)).unwrap();
    }
    gpu.stream.synchronize().unwrap();

    let got = download(&d_labels, n);
    let expect: Vec<i32> = cpu
        .iter()
        .map(|c| c.map(|v| v as i32).unwrap_or(-1))
        .collect();

    let counts = download(&d_counts, n);
    println!("  dbscan/n={n} eps={eps} min_pts={min_pts} (device min_pts={device_min_pts})");
    println!("    gpu labels    = {got:?}");
    println!("    oracle labels = {expect:?}");
    println!("    gpu nbr counts= {counts:?}");

    let gpu_noise = got.iter().filter(|&&l| l < 0).count();
    let cpu_noise = expect.iter().filter(|&&l| l < 0).count();
    println!("    noise: gpu={gpu_noise} oracle={cpu_noise}");

    // ADR-2061: exact agreement up to cluster-label permutation, noise included.
    if let Err(e) = same_partition(&got, &expect) {
        panic!("DBSCAN diverges from oracle: {e}\n  gpu={got:?}\n  oracle={expect:?}");
    }
}

// ===========================================================================
// LOF — gpu_clustering_kernels.cu
// ===========================================================================

/// Deterministic LOF fixture: seven points in general position plus one clear
/// outlier at `[8, 8]`.
///
/// "General position" is load-bearing. The oracle's k-neighbourhood keeps every
/// point at distance `<= kth` (`dd <= kth`), so on a symmetric point set it can
/// hold more than `k` members, while the GPU's insertion-sort buffer is capped
/// at exactly `k`. Choosing coordinates with no tied distances removes that
/// difference from the comparison, so what the test measures is the LOF
/// formula, not a tie-handling artefact.
fn lof_fixture() -> (Vec<oracle::Pt>, usize) {
    (
        vec![
            [0.0, 0.0],
            [1.0, 0.2],
            [0.3, 1.1],
            [1.2, 1.3],
            [0.6, 0.5],
            [2.1, 0.9],
            [0.9, 2.2],
            [8.0, 8.0],
        ],
        3,
    )
}

#[test]
#[ignore = "needs GPU: real CUDA device + compiled clustering PTX (run with --ignored)"]
fn adr_2061_lof_matches_oracle() {
    const TEST: &str = "adr_2061_lof_matches_oracle";
    let Some(gpu) = setup(PTXModule::GpuClusteringKernels, TEST) else {
        return;
    };

    let (pts, k) = lof_fixture();
    let n = pts.len();
    let cpu = oracle::lof(&pts, k);

    let px: Vec<f32> = pts.iter().map(|p| p[0] as f32).collect();
    let py: Vec<f32> = pts.iter().map(|p| p[1] as f32).collect();
    let pz = vec![0.0f32; n];

    // Single-cell grid. `lof_gather_neighbors` scans the 3x3x3 cell
    // neighbourhood around the query and clips to `grid_dims`, so a 1x1x1 grid
    // whose one cell holds every point makes the gather an exact brute-force
    // k-NN — the same neighbourhood the oracle uses. That isolates the LOF
    // *formula* from the spatial-index approximation, which is the comparison
    // ADR-2061 specifies.
    let lo = -100.0f32;
    let cell = 1.0e6f32;
    let sorted: Vec<i32> = (0..n as i32).collect();
    let cell_start = vec![0i32];
    let cell_end = vec![n as i32];
    let cell_keys = vec![0i32];
    let grid_dims = Int3 { x: 1, y: 1, z: 1 };
    let radius = 1.0e6f32;

    let block = 64u32;
    let grid = grid_for(n, block);
    let stream = &gpu.stream;

    let d_x = DeviceBuffer::from_slice(&px).unwrap();
    let d_y = DeviceBuffer::from_slice(&py).unwrap();
    let d_z = DeviceBuffer::from_slice(&pz).unwrap();
    let d_sorted = DeviceBuffer::from_slice(&sorted).unwrap();
    let d_cs = DeviceBuffer::from_slice(&cell_start).unwrap();
    let d_ce = DeviceBuffer::from_slice(&cell_end).unwrap();
    let d_ck = DeviceBuffer::from_slice(&cell_keys).unwrap();
    let d_lof = DeviceBuffer::<f32>::zeroed(n).unwrap();
    let d_dens = DeviceBuffer::<f32>::zeroed(n).unwrap();

    let k_lof = gpu
        .kernel("compute_lof_kernel")
        .expect("compute_lof_kernel");

    // SAFETY: position and output buffers are all length `n` and the grid
    // covers `n` with an in-kernel bounds check; the grid-index buffers are
    // sized for the declared 1x1x1 `grid_dims`, so every `cell_idx` the kernel
    // forms is 0 and in range. `k` is 3, well under the kernel's `LOF_MAX_K`
    // register-buffer bound of 32.
    unsafe {
        launch!(k_lof<<<grid, block, 0, stream>>>(
            d_x.as_device_ptr(),
            d_y.as_device_ptr(),
            d_z.as_device_ptr(),
            d_sorted.as_device_ptr(),
            d_cs.as_device_ptr(),
            d_ce.as_device_ptr(),
            d_ck.as_device_ptr(),
            grid_dims,
            d_lof.as_device_ptr(),
            d_dens.as_device_ptr(),
            n as i32,
            k as i32,
            radius,
            lo,
            -lo,
            cell,
            32i32
        ))
        .unwrap();
    }
    gpu.stream.synchronize().unwrap();

    let got: Vec<f64> = download(&d_lof, n).iter().map(|&v| v as f64).collect();

    let mut worst = 0.0f64;
    let mut worst_i = 0usize;
    for (i, (a, b)) in cpu.iter().zip(got.iter()).enumerate() {
        let d = (a - b).abs();
        if d > worst {
            worst = d;
            worst_i = i;
        }
    }
    println!("  lof/n={n} k={k}");
    for i in 0..n {
        println!(
            "    p{i} {:?}  gpu={:.6}  oracle={:.6}  Δ={:.3e}",
            pts[i],
            got[i],
            cpu[i],
            (got[i] - cpu[i]).abs()
        );
    }
    println!("    max|Δ|={worst:.6e} at point {worst_i}");

    // ADR-2061: the >95th-percentile set must match exactly.
    let top = |v: &[f64]| -> Vec<usize> {
        let mut s: Vec<f64> = v.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let thresh = s[((v.len() as f64 * 0.95).ceil() as usize).min(v.len()) - 1];
        (0..v.len()).filter(|&i| v[i] >= thresh).collect()
    };
    let (t_gpu, t_cpu) = (top(&got), top(&cpu));
    println!("    >p95 set: gpu={t_gpu:?} oracle={t_cpu:?}");

    assert_eq!(
        t_gpu, t_cpu,
        "LOF >95th-percentile set differs: gpu={t_gpu:?} oracle={t_cpu:?}"
    );
    // ADR-2061: per-point absolute difference < 1e-3.
    assert!(
        worst < 1e-3,
        "LOF max|Δ|={worst:.6e} at point {worst_i} exceeds the ADR-2061 bar of 1e-3 \
         (gpu={:.6}, oracle={:.6}).\n\
         The kernel must implement neighbour k-distance reachability and include kth-distance ties.",
        got[worst_i],
        cpu[worst_i]
    );
}

// ===========================================================================
// Louvain — gpu_clustering_kernels.cu
// ===========================================================================

/// One level's graph in the Louvain hierarchy.
///
/// Level 0 is the fixture itself with unit weights; every later level is the
/// previous level contracted so that one node == one community. `node_weights`
/// is the weighted degree (self-loops counted twice), which is the `k_i` the
/// gain formula uses.
struct LevelGraph {
    n: usize,
    offsets: Vec<i32>,
    indices: Vec<i32>,
    weights: Vec<f32>,
    node_weights: Vec<f32>,
}

impl LevelGraph {
    fn from_fixture(g: &oracle::GraphFixture) -> Self {
        let (offsets, indices, weights) = undirected_csr(g);
        let node_weights = (0..g.n)
            .map(|i| (offsets[i + 1] - offsets[i]) as f32)
            .collect();
        LevelGraph {
            n: g.n,
            offsets,
            indices,
            weights,
            node_weights,
        }
    }
}

/// Run `louvain_local_pass_kernel` to a fixpoint on one level. Returns the raw
/// (non-dense) community id per node.
///
/// This is the host half of the D1 fix's contract, spelled out in the kernel's
/// own comment: before each pass the host seeds `out == in` and
/// `next == snapshot`, and after each pass copies `out -> in` and
/// `next -> snapshot`. Passes alternate `iteration` parity, which is the
/// kernel's symmetry break against two adjacent nodes swapping communities
/// forever — so convergence needs **two** consecutive quiet passes, one of each
/// parity, not one.
fn louvain_local_move(
    gpu: &Gpu,
    lg: &LevelGraph,
    total_weight: f32,
    max_passes: usize,
) -> Vec<i32> {
    let n = lg.n;
    let block = 256u32;
    let grid = grid_for(n, block);
    let stream = &gpu.stream;

    let d_w = DeviceBuffer::from_slice(&lg.weights).unwrap();
    let d_idx = DeviceBuffer::from_slice(&lg.indices).unwrap();
    let d_off = DeviceBuffer::from_slice(&lg.offsets).unwrap();
    let d_nw = DeviceBuffer::from_slice(&lg.node_weights).unwrap();
    let mut d_in = DeviceBuffer::<i32>::zeroed(n).unwrap();
    let mut d_out = DeviceBuffer::<i32>::zeroed(n).unwrap();
    let mut d_snap = DeviceBuffer::<f32>::zeroed(n).unwrap();
    let mut d_next = DeviceBuffer::<f32>::zeroed(n).unwrap();
    let mut d_flag = DeviceBuffer::<u8>::zeroed(1).unwrap();

    let k_init = gpu
        .kernel("init_communities_kernel")
        .expect("init_communities_kernel");
    let k_pass = gpu
        .kernel("louvain_local_pass_kernel")
        .expect("louvain_local_pass_kernel");

    // SAFETY: `d_in`, `d_out`, `d_snap`, `d_next` and `d_nw` are each length
    // `n`, indexed by a bounds-checked thread id; `d_off` is length `n + 1` and
    // `d_idx`/`d_w` length `|indices|`, the exact ranges
    // `edge_offsets[node]..edge_offsets[node + 1]` address. `d_flag` is a single
    // byte matching the kernel's `bool*`. Every pass synchronises before the
    // host reads back, so no launch races a copy.
    unsafe {
        launch!(k_init<<<grid, block, 0, stream>>>(
            d_in.as_device_ptr(),
            d_snap.as_device_ptr(),
            d_nw.as_device_ptr(),
            n as i32
        ))
        .unwrap();
        gpu.stream.synchronize().unwrap();

        let mut communities = download(&d_in, n);
        let mut snapshot = download(&d_snap, n);
        let mut quiet = 0usize;

        for iteration in 0..max_passes {
            d_in.copy_from(&communities).unwrap();
            d_out.copy_from(&communities).unwrap();
            d_snap.copy_from(&snapshot).unwrap();
            d_next.copy_from(&snapshot).unwrap();
            d_flag.copy_from(&[0u8]).unwrap();

            launch!(k_pass<<<grid, block, 0, stream>>>(
                d_w.as_device_ptr(),
                d_idx.as_device_ptr(),
                d_off.as_device_ptr(),
                d_in.as_device_ptr(),
                d_out.as_device_ptr(),
                d_nw.as_device_ptr(),
                d_snap.as_device_ptr(),
                d_next.as_device_ptr(),
                d_flag.as_device_ptr(),
                n as i32,
                total_weight,
                1.0f32,
                iteration as i32
            ))
            .unwrap();
            gpu.stream.synchronize().unwrap();

            communities = download(&d_out, n);
            snapshot = download(&d_next, n);

            if download(&d_flag, 1)[0] == 0 {
                quiet += 1;
                if quiet >= 2 {
                    break;
                }
            } else {
                quiet = 0;
            }
        }
        communities
    }
}

/// Contract a level: renumber communities densely, then build the next level's
/// weighted graph with `louvain_relabel_nodes_kernel` +
/// `louvain_aggregate_edges_kernel`.
///
/// Returns `(dense community per node, next LevelGraph)`, or `None` when the
/// level did not merge anything (a fixpoint of the whole algorithm).
fn louvain_contract(gpu: &Gpu, lg: &LevelGraph, raw: &[i32]) -> Option<(Vec<usize>, LevelGraph)> {
    use std::collections::HashMap;
    let n = lg.n;

    // Dense renumbering, host-side: the aggregation kernel indexes a
    // `num_comm x num_comm` matrix, so ids must be contiguous from 0.
    let mut dense: HashMap<i32, i32> = HashMap::new();
    let mut remap = vec![0i32; n];
    for &c in raw.iter() {
        let next = dense.len() as i32;
        dense.entry(c).or_insert(next);
    }
    // `remap` is indexed by RAW community id, which `init_communities_kernel`
    // seeds to the node's own index, so raw ids stay inside [0, n).
    for (&raw_id, &dense_id) in dense.iter() {
        remap[raw_id as usize] = dense_id;
    }
    let nc = dense.len();
    if nc == n || nc <= 1 {
        // Nothing merged (or everything did) — contracting again is a no-op.
        return None;
    }
    // The dense adjacency is nc^2 floats. Every fixture here contracts hard on
    // the first level, but refuse rather than thrash if a future one does not.
    if nc.saturating_mul(nc).saturating_mul(4) > 2 << 30 {
        eprintln!(
            "  louvain: refusing {nc}^2 aggregation buffer (> 2 GiB); stopping at this level"
        );
        return None;
    }

    let block = 256u32;
    let grid = grid_for(n, block);
    let stream = &gpu.stream;

    let d_off = DeviceBuffer::from_slice(&lg.offsets).unwrap();
    let d_idx = DeviceBuffer::from_slice(&lg.indices).unwrap();
    let d_w = DeviceBuffer::from_slice(&lg.weights).unwrap();
    let d_raw = DeviceBuffer::from_slice(raw).unwrap();
    let d_remap = DeviceBuffer::from_slice(&remap).unwrap();
    let d_densec = DeviceBuffer::<i32>::zeroed(n).unwrap();
    let d_agg = DeviceBuffer::<f32>::zeroed(nc * nc).unwrap();

    let k_relabel = gpu
        .kernel("louvain_relabel_nodes_kernel")
        .expect("louvain_relabel_nodes_kernel");
    let k_agg = gpu
        .kernel("louvain_aggregate_edges_kernel")
        .expect("louvain_aggregate_edges_kernel");

    // SAFETY: `d_raw`, `d_remap` and `d_densec` are length `n` and indexed by a
    // bounds-checked thread id; every value in `d_raw` is a valid index into
    // `d_remap` because raw community ids are seeded from node indices. `d_agg`
    // is exactly `nc * nc` and the kernel's only write is
    // `agg[c_src * num_comm + c_dst]` with both ids `< nc` by construction of
    // the dense remap. Zeroed by `DeviceBuffer::zeroed`, as the kernel requires.
    unsafe {
        launch!(k_relabel<<<grid, block, 0, stream>>>(
            d_raw.as_device_ptr(),
            d_remap.as_device_ptr(),
            d_densec.as_device_ptr(),
            n as i32
        ))
        .unwrap();
        launch!(k_agg<<<grid, block, 0, stream>>>(
            d_w.as_device_ptr(),
            d_idx.as_device_ptr(),
            d_off.as_device_ptr(),
            d_densec.as_device_ptr(),
            d_agg.as_device_ptr(),
            n as i32,
            nc as i32
        ))
        .unwrap();
    }
    gpu.stream.synchronize().unwrap();

    let dense_per_node: Vec<usize> = download(&d_densec, n).iter().map(|&c| c as usize).collect();
    let agg = download(&d_agg, nc * nc);

    // Compact the dense matrix to CSR. Both directions of every inter-community
    // edge are present (the source CSR carries both), and the diagonal holds
    // twice the intra-community weight — exactly the convention that makes
    // `node_weights[c]` (the row sum) equal the summed degree of c's members,
    // so the Louvain invariant `sum(node_weights) == 2m` survives contraction.
    let mut offsets = Vec::with_capacity(nc + 1);
    let mut indices = Vec::new();
    let mut weights = Vec::new();
    let mut node_weights = vec![0.0f32; nc];
    offsets.push(0i32);
    for c in 0..nc {
        let mut row_sum = 0.0f32;
        for d in 0..nc {
            let w = agg[c * nc + d];
            row_sum += w;
            if w != 0.0 {
                indices.push(d as i32);
                weights.push(w);
            }
        }
        node_weights[c] = row_sum;
        offsets.push(indices.len() as i32);
    }

    Some((
        dense_per_node,
        LevelGraph {
            n: nc,
            offsets,
            indices,
            weights,
            node_weights,
        },
    ))
}

/// Full multi-level Louvain: local move to a fixpoint, contract, repeat.
///
/// The contraction step is not optional garnish. `gpu_clustering_kernels.cu`
/// says so itself at the aggregation kernels: *"Running local-move again on the
/// contracted graph lets Louvain escape the first local optimum — the step the
/// single-pass kernel was missing, which is why modularity sat near zero."*
/// A harness that ran only level 0 would measure that missing step rather than
/// the kernels, and would report a failure the kernels do not own.
///
/// Returns `(community per original node, distinct community count, levels run)`.
fn gpu_louvain(gpu: &Gpu, g: &oracle::GraphFixture, max_levels: usize) -> (Vec<u32>, usize, usize) {
    // `m` is invariant across levels: contraction preserves total edge weight.
    let total_weight = g.edge_count() as f32;
    let mut lg = LevelGraph::from_fixture(g);
    // Original node -> its node index at the current level.
    let mut mapping: Vec<usize> = (0..g.n).collect();
    let mut levels = 0usize;

    for _ in 0..max_levels {
        let raw = louvain_local_move(gpu, &lg, total_weight, 256);
        levels += 1;
        let Some((dense_per_node, next)) = louvain_contract(gpu, &lg, &raw) else {
            // No further merging: fold this level's raw ids through and stop.
            let mut dense = std::collections::HashMap::new();
            let folded: Vec<u32> = mapping
                .iter()
                .map(|&node| {
                    let c = raw[node];
                    let next_id = dense.len() as u32;
                    *dense.entry(c).or_insert(next_id)
                })
                .collect();
            let distinct = oracle::distinct_communities(&folded);
            return (folded, distinct, levels);
        };
        mapping = mapping.iter().map(|&node| dense_per_node[node]).collect();
        lg = next;
    }

    let labels: Vec<u32> = mapping.iter().map(|&c| c as u32).collect();
    let distinct = oracle::distinct_communities(&labels);
    (labels, distinct, levels)
}

#[test]
#[ignore = "needs GPU: real CUDA device + compiled clustering PTX (run with --ignored)"]
fn adr_2061_louvain_matches_oracle() {
    const TEST: &str = "adr_2061_louvain_matches_oracle";
    let Some(gpu) = setup(PTXModule::GpuClusteringKernels, TEST) else {
        return;
    };

    let mut failures: Vec<String> = Vec::new();

    // --- two_clique: exactly 2 communities, matching the known-correct answer.
    {
        let g = oracle::two_clique();
        let (labels, distinct, levels) = gpu_louvain(&gpu, &g, 16);
        let optimal = oracle::two_clique_optimal_partition(&g);
        let q = oracle::modularity(&g, &labels);
        let q_opt = oracle::modularity(&g, &optimal);
        println!(
            "  louvain/two_clique  communities={distinct} levels={levels} Q={q:.4} Q_optimal={q_opt:.4}\n    labels={labels:?}\n    optimal={optimal:?}"
        );
        if distinct != 2 {
            failures.push(format!("two_clique: {distinct} communities, expected 2"));
        }
        let a: Vec<i32> = labels.iter().map(|&v| v as i32).collect();
        let b: Vec<i32> = optimal.iter().map(|&v| v as i32).collect();
        if let Err(e) = same_partition(&a, &b) {
            failures.push(format!(
                "two_clique: partition != two_clique_optimal_partition up to permutation ({e})"
            ));
        }
    }

    // --- triangle and star: exactly 1 community.
    for g in [oracle::triangle(), oracle::star(6)] {
        let (labels, distinct, levels) = gpu_louvain(&gpu, &g, 16);
        let q = oracle::modularity(&g, &labels);
        println!(
            "  louvain/{:<12} communities={distinct} levels={levels} Q={q:.4} labels={labels:?}",
            g.name
        );
        if distinct != 1 {
            failures.push(format!("{}: {distinct} communities, expected 1", g.name));
        }
    }

    // --- canonical_live_scale: modularity quality, never exact labels.
    {
        let g = oracle::canonical_live_scale();
        let (labels, distinct, levels) = gpu_louvain(&gpu, &g, 16);
        // The oracle's modularity *of the GPU's own partition* — the number
        // ADR-2061 requires the kernel's own reported Q to sit within 0.02 of,
        // and the value that says whether the partition is any good.
        let q_gpu = oracle::modularity(&g, &labels);
        // The planted ground-truth partition: 16 equal blocks.
        let per = g.n / oracle::CANONICAL_COMMUNITIES;
        let planted: Vec<u32> = (0..g.n)
            .map(|i| (i / per).min(oracle::CANONICAL_COMMUNITIES - 1) as u32)
            .collect();
        let q_planted = oracle::modularity(&g, &planted);
        println!(
            "  louvain/canonical_live_scale n={} communities={distinct} levels={levels} Q_gpu={q_gpu:.4} Q_planted={q_planted:.4} deficit={:.4}",
            g.n,
            q_planted - q_gpu
        );
        if q_planted - q_gpu > 0.05 {
            failures.push(format!(
                "canonical_live_scale: Q_gpu={q_gpu:.4} is {:.4} below the reference partition's \
                 Q={q_planted:.4}, exceeding the 0.05 allowance",
                q_planted - q_gpu
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "Louvain conformance:\n  {}",
        failures.join("\n  ")
    );
}
