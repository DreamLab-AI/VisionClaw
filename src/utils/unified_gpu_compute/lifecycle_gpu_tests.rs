//! GPU tests for the engine's buffer lifecycle and for SSSP correctness (rework stage S1a).
//!
//! These drive a real `UnifiedGPUCompute` the way production does: the engine is built at
//! 1000 nodes x 1000 edges (`gpu_resource_actor.rs`, `force_compute_actor.rs`) and every
//! graph after that arrives through `initialize_graph`, which resizes. Each test states the
//! behaviour the engine must have; the module history records what each one did before the
//! S1a fix (see the commit that added this file).
//!
//! Run them on a GPU host, one CUDA context per test:
//!
//! ```sh
//! CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=<gpu> CUDA_ARCH=<sm> \
//!   cargo test --lib lifecycle_gpu_tests -- --ignored --test-threads=1
//! ```
//!
//! A host without a CUDA driver or device prints `SKIP` and passes. Out-of-bounds device
//! accesses are not always visible to an ordinary run (a short read inside the same
//! allocation granule returns garbage rather than faulting), so the gate script also runs
//! this module under `compute-sanitizer --tool memcheck`.

use super::construction::UnifiedGPUCompute;
use crate::models::simulation_params::SimulationParams;
use cust::device::{Device, DeviceAttribute};
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use visionclaw_domain::types::physics_config::PhysicsSettings;

/// The size production builds its engines at before the first graph arrives.
const PRODUCTION_CAPACITY: usize = 1000;

// ---------------------------------------------------------------------------
// Device and PTX
// ---------------------------------------------------------------------------

/// Context guard plus the PTX this tree built. `None` means SKIP (no driver or device).
struct Gpu {
    _ctx: cust::context::Context,
    ptx: String,
}

fn gpu(test: &str) -> Option<Gpu> {
    let ctx = match cust::quick_init() {
        Ok(c) => c,
        Err(e) => {
            println!("SKIP {test}: CUDA unavailable: {e}");
            return None;
        }
    };
    let device = Device::get_device(0).expect("device 0");
    let name = device.name().expect("device name");
    let bus = device
        .get_attribute(DeviceAttribute::PciBusId)
        .expect("PCI bus id");
    if let Ok(want) = std::env::var("VISIONCLAW_EXPECT_PCI_BUS") {
        let want = want.trim();
        let parsed = match want.strip_prefix("0x").or_else(|| want.strip_prefix("0X")) {
            Some(hex) => i32::from_str_radix(hex, 16),
            None => want.parse(),
        }
        .expect("VISIONCLAW_EXPECT_PCI_BUS must be an integer");
        assert_eq!(
            parsed, bus,
            "{test}: refusing to run on {name} (PCI bus 0x{bus:02X}); \
             VISIONCLAW_EXPECT_PCI_BUS={want}. Set CUDA_DEVICE_ORDER=PCI_BUS_ID."
        );
    }
    // The loader refuses a baked artefact whose bytes differ from the build's recorded tag,
    // so a PTX that loads here is the one compiled from this tree's `.cu` sources.
    let module = visionclaw_gpu::ptx_loader::PTXModule::VisionflowUnified;
    let ptx = visionclaw_gpu::ptx_loader::load_ptx_module_sync(module)
        .unwrap_or_else(|e| panic!("{test}: unified PTX: {e}"));
    println!(
        "{test}: device {name} (PCI bus 0x{bus:02X}), unified PTX tag {}",
        module.expected_content_tag().unwrap_or("unknown")
    );
    Some(Gpu { _ctx: ctx, ptx })
}

fn production_engine(g: &Gpu) -> UnifiedGPUCompute {
    UnifiedGPUCompute::new(PRODUCTION_CAPACITY, PRODUCTION_CAPACITY, &g.ptx).expect("engine")
}

// ---------------------------------------------------------------------------
// Fixtures and the CPU oracle
// ---------------------------------------------------------------------------

/// A symmetric CSR built the way the server builds it (both directions of every edge).
struct Graph {
    n: usize,
    offsets: Vec<i32>,
    cols: Vec<i32>,
    weights: Vec<f32>,
    /// Component id of each node, as generated.
    component: Vec<usize>,
}

struct Lcg(u64);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next_f32() * n as f32) as usize).min(n - 1)
    }
}

/// `components` disjoint connected pieces of (nearly) equal size, each a random spanning tree
/// plus `extra_per_node` random chords. Weights are uniform in `[w_lo, w_hi]`, strictly positive.
fn graph(
    n: usize,
    components: usize,
    extra_per_node: f32,
    w_lo: f32,
    w_hi: f32,
    seed: u64,
) -> Graph {
    let mut rng = Lcg(seed);
    let component: Vec<usize> = (0..n).map(|i| i * components / n).collect();
    let mut edges: Vec<(usize, usize, f32)> = Vec::new();
    let weight = |rng: &mut Lcg| w_lo + (w_hi - w_lo) * rng.next_f32();
    for c in 0..components {
        let members: Vec<usize> = (0..n).filter(|&i| component[i] == c).collect();
        for k in 1..members.len() {
            let parent = members[rng.below(k)];
            let w = weight(&mut rng);
            edges.push((members[k], parent, w));
        }
        let extra = (members.len() as f32 * extra_per_node) as usize;
        for _ in 0..extra {
            let a = members[rng.below(members.len())];
            let b = members[rng.below(members.len())];
            if a != b {
                let w = weight(&mut rng);
                edges.push((a, b, w));
            }
        }
    }
    let mut adjacency: Vec<Vec<(i32, f32)>> = vec![Vec::new(); n];
    for &(a, b, w) in &edges {
        adjacency[a].push((b as i32, w));
        adjacency[b].push((a as i32, w));
    }
    let mut offsets = vec![0i32];
    let mut cols = Vec::new();
    let mut weights = Vec::new();
    for row in &adjacency {
        for &(c, w) in row {
            cols.push(c);
            weights.push(w);
        }
        offsets.push(cols.len() as i32);
    }
    Graph {
        n,
        offsets,
        cols,
        weights,
        component,
    }
}

fn positions(n: usize, seed: u64) -> [Vec<f32>; 3] {
    let mut rng = Lcg(seed);
    let mut axis = || (0..n).map(|_| (rng.next_f32() - 0.5) * 400.0).collect();
    [axis(), axis(), axis()]
}

fn load(engine: &mut UnifiedGPUCompute, g: &Graph) {
    let [x, y, z] = positions(g.n, 7);
    engine
        .initialize_graph(
            &g.offsets,
            &g.cols,
            &g.weights,
            [&x, &y, &z],
            g.n,
            g.cols.len(),
        )
        .expect("initialize_graph");
}

#[derive(PartialEq)]
struct Entry(f64, usize);
impl Eq for Entry {}
impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Plain Dijkstra in f64, the oracle every GPU distance is compared with.
fn dijkstra(g: &Graph, source: usize) -> Vec<f64> {
    let mut dist = vec![f64::INFINITY; g.n];
    let mut heap = BinaryHeap::new();
    dist[source] = 0.0;
    heap.push(Entry(0.0, source));
    while let Some(Entry(d, u)) = heap.pop() {
        if d > dist[u] {
            continue;
        }
        for e in g.offsets[u] as usize..g.offsets[u + 1] as usize {
            let v = g.cols[e] as usize;
            let nd = d + g.weights[e] as f64;
            if nd < dist[v] {
                dist[v] = nd;
                heap.push(Entry(nd, v));
            }
        }
    }
    dist
}

/// Compares GPU distances with the oracle. f32 path sums carry at most one rounding per hop,
/// so the tolerance is relative to the distance. Returns the number of mismatches, after
/// printing the first few.
fn compare_distances(label: &str, gpu: &[f32], oracle: &[f64]) -> usize {
    assert_eq!(gpu.len(), oracle.len(), "{label}: length");
    let mut bad = 0usize;
    for (i, (&d, &o)) in gpu.iter().zip(oracle).enumerate() {
        let ok = if o.is_infinite() {
            d.is_infinite()
        } else {
            d.is_finite() && ((d as f64) - o).abs() <= 1e-4 * o.max(1.0)
        };
        if !ok {
            if bad < 5 {
                println!("{label}: node {i}: gpu {d}, oracle {o}");
            }
            bad += 1;
        }
    }
    let reachable = oracle.iter().filter(|d| d.is_finite()).count();
    println!(
        "{label}: {} nodes, {reachable} reachable, {bad} mismatches",
        gpu.len()
    );
    bad
}

fn panic_message(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "<non-string panic>".into())
}

// ---------------------------------------------------------------------------
// L1: per-node buffers after a resize
// ---------------------------------------------------------------------------

/// SSSP runs through the shared `Arc<Mutex<_>>` exactly as `ShortestPathActor` does. It must
/// return the oracle's distances and leave the mutex usable, for a graph larger and a graph
/// smaller than the construction size (the shrink takes `resize_buffers`' early return).
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn l1_sssp_after_the_graph_changes_size() {
    let Some(g) = gpu("l1_sssp_after_the_graph_changes_size") else {
        return;
    };
    for &n in &[1500usize, 500] {
        let fixture = graph(n, 1, 1.0, 0.5, 3.0, 11 + n as u64);
        let mut engine = production_engine(&g);
        load(&mut engine, &fixture);
        let shared = Arc::new(Mutex::new(engine));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            shared.lock().expect("lock").run_sssp(0, None)
        }));
        let distances = match outcome {
            Ok(Ok(d)) => d,
            Ok(Err(e)) => panic!("n={n}: run_sssp returned an error: {e:#}"),
            Err(p) => panic!(
                "n={n}: run_sssp panicked ({}); shared engine mutex poisoned: {}",
                panic_message(p.as_ref()),
                shared.is_poisoned()
            ),
        };
        assert!(!shared.is_poisoned(), "n={n}: mutex poisoned");
        let bad = compare_distances(&format!("n={n}"), &distances, &dijkstra(&fixture, 0));
        assert_eq!(bad, 0, "n={n}: SSSP disagrees with Dijkstra");
    }
}

/// Label propagation on a graph larger than the construction size: it must finish, return
/// one label per node, keep components apart, report labels inside `0..num_communities`, and
/// find real structure (six disjoint components of about 250 nodes).
///
/// A physics step runs first, as in production: the vote weight `weight_precision_multiplier`
/// lives in the `c_params` device constant, which only `execute` uploads (finding L13). On a
/// fresh engine every vote is zero and label propagation returns 1500 singletons, "converged"
/// after one iteration; the structure assertions below would catch that.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn l1_label_propagation_after_growth() {
    let Some(g) = gpu("l1_label_propagation_after_growth") else {
        return;
    };
    let fixture = graph(1500, 6, 2.0, 1.0, 1.0, 21);
    let mut engine = production_engine(&g);
    load(&mut engine, &fixture);
    engine
        .execute_physics_step_with_bypass(
            &SimulationParams::from(&PhysicsSettings::default()),
            true,
        )
        .expect("physics step");
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        engine.run_community_detection(50, 1234)
    }));
    let (labels, communities, modularity, iterations, sizes, converged) = match outcome {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => panic!("label propagation returned an error: {e:#}"),
        Err(p) => panic!("label propagation panicked: {}", panic_message(p.as_ref())),
    };
    println!(
        "labels {} communities {communities} modularity {modularity:.3} iterations {iterations} converged {converged}",
        labels.len()
    );
    assert_eq!(labels.len(), fixture.n, "one label per node");
    assert!(communities >= 6, "six disjoint components need six labels");
    assert!(
        communities <= fixture.n / 10,
        "{communities} communities for six components of ~250 nodes: no labels propagated"
    );
    assert!(
        modularity > 0.3,
        "modularity {modularity} shows no community structure"
    );
    assert_eq!(
        sizes.iter().sum::<i32>() as usize,
        fixture.n,
        "sizes cover every node"
    );
    for (i, &l) in labels.iter().enumerate() {
        assert!(
            l >= 0 && (l as usize) < communities,
            "node {i}: label {l} outside 0..{communities}"
        );
    }
    let mut owner = vec![usize::MAX; communities];
    for (i, &l) in labels.iter().enumerate() {
        let c = fixture.component[i];
        let slot = &mut owner[l as usize];
        assert!(
            *slot == usize::MAX || *slot == c,
            "label {l} spans components {} and {c}",
            *slot
        );
        *slot = c;
    }
}

// ---------------------------------------------------------------------------
// F1: delta-stepping against the CPU oracle
// ---------------------------------------------------------------------------

/// Delta-stepping and the plain frontier relaxation must both match Dijkstra. The engine is
/// built at exactly the graph's size so no resize is involved and only the algorithm is under
/// test. Cases: weights below delta, weights above delta (so whole buckets are empty), and a
/// graph in several components (unreachable nodes must stay infinite).
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn f1_sssp_matches_dijkstra() {
    let Some(g) = gpu("f1_sssp_matches_dijkstra") else {
        return;
    };
    struct Case {
        name: &'static str,
        n: usize,
        components: usize,
        w: (f32, f32),
        delta: f32,
    }
    let cases = [
        Case {
            name: "light-64",
            n: 64,
            components: 1,
            w: (0.5, 1.5),
            delta: 4.0,
        },
        Case {
            name: "heavy-64",
            n: 64,
            components: 1,
            w: (5.0, 9.0),
            delta: 1.0,
        },
        Case {
            name: "split-64",
            n: 64,
            components: 3,
            w: (0.5, 3.0),
            delta: 2.0,
        },
        Case {
            name: "light-1500",
            n: 1500,
            components: 1,
            w: (0.5, 1.5),
            delta: 4.0,
        },
        Case {
            name: "heavy-1500",
            n: 1500,
            components: 1,
            w: (5.0, 9.0),
            delta: 1.0,
        },
        Case {
            name: "split-1500",
            n: 1500,
            components: 4,
            w: (0.5, 3.0),
            delta: 2.0,
        },
    ];
    let mut failures = Vec::new();
    for (k, case) in cases.iter().enumerate() {
        let fixture = graph(
            case.n,
            case.components,
            1.5,
            case.w.0,
            case.w.1,
            100 + k as u64,
        );
        let mut engine =
            UnifiedGPUCompute::new(fixture.n, fixture.cols.len(), &g.ptx).expect("engine");
        load(&mut engine, &fixture);
        let source = fixture.n / 3;
        let oracle = dijkstra(&fixture, source);
        for delta in [None, Some(case.delta)] {
            let label = format!("{} delta={delta:?}", case.name);
            // Repeated runs: the stream-ordering race (F6) shows up as run-to-run variation.
            for run in 0..3 {
                let d = engine.run_sssp(source, delta).expect("run_sssp");
                if compare_distances(&format!("{label} run {run}"), &d, &oracle) != 0 {
                    failures.push(format!("{label} run {run}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "SSSP disagrees with Dijkstra: {failures:?}"
    );
}

// ---------------------------------------------------------------------------
// C1b: SSSP distances seen by the force pass after the graph changes
// ---------------------------------------------------------------------------

/// SSSP on the production-sized engine, then the graph grows and the next physics step runs
/// through the production wrapper with SSSP spring adjustment on. Distances computed for the
/// old graph must not reach the force pass for the new one (they are indexed by the old node
/// order and are one entry per old node), and the step must produce finite positions.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn c1b_stale_sssp_distances_do_not_reach_the_force_pass() {
    let Some(g) = gpu("c1b_stale_sssp_distances_do_not_reach_the_force_pass") else {
        return;
    };
    let mut engine = production_engine(&g);
    // Exactly the construction size: the one node count at which SSSP worked before S1a.
    let before = graph(PRODUCTION_CAPACITY, 1, 1.0, 0.5, 3.0, 31);
    load(&mut engine, &before);
    engine
        .run_sssp(0, None)
        .expect("SSSP on the 1000-node graph");
    assert!(engine.sssp_available, "SSSP reported available");

    let after = graph(1500, 1, 1.0, 0.5, 3.0, 32);
    load(&mut engine, &after);
    let stale = engine
        .sssp_device_distances
        .as_ref()
        .map(|b| b.len())
        .filter(|&len| len < engine.num_nodes);
    println!(
        "after growth: num_nodes {}, sssp_available {}, retained distances {:?}",
        engine.num_nodes,
        engine.sssp_available,
        engine.sssp_device_distances.as_ref().map(|b| b.len())
    );

    let physics = PhysicsSettings {
        sssp_enabled: true,
        lin_log_mode: false, // Hooke: both du and dv are read
        ..PhysicsSettings::default()
    };
    let mut params = SimulationParams::from(&physics);
    params.warmup_iterations = 0; // the ramp scales every force by iteration / warmup
    engine
        .execute_physics_step_with_bypass(&params, true)
        .expect("physics step");
    let (x, y, z) = engine.get_node_positions().expect("positions");
    assert!(
        x.iter()
            .chain(&y)
            .chain(&z)
            .take(3 * after.n)
            .all(|v| v.is_finite()),
        "non-finite positions after the step"
    );
    assert_eq!(
        stale,
        None,
        "the engine still offers {}-entry SSSP distances to a {}-node force pass",
        stale.unwrap_or(0),
        engine.num_nodes
    );
    assert!(
        !engine.sssp_available,
        "SSSP availability survived a graph change"
    );
}

// ---------------------------------------------------------------------------
// L6: the relabel kernel's unchecked `label_mapping[old_label]`
// ---------------------------------------------------------------------------

/// Labels are node indices, so a valid label is below `num_nodes`. The relabel kernel must not
/// index `label_mapping` with anything else: an out-of-range label becomes -1 (unassigned),
/// which every consumer already skips.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn l6_relabel_rejects_an_out_of_range_label() {
    let Some(g) = gpu("l6_relabel_rejects_an_out_of_range_label") else {
        return;
    };
    let engine = production_engine(&g);
    let n = 4usize;
    let labels = DeviceBuffer::from_slice(&[0i32, 3, -1, 1_000_000]).expect("labels");
    let mapping = DeviceBuffer::from_slice(&[7i32, -1, -1, 8]).expect("mapping");
    let relabel = engine
        ._module
        .get_function("relabel_communities_kernel")
        .expect("relabel_communities_kernel");
    let stream = &engine.stream;
    // SAFETY: both buffers hold `n` elements and the kernel guards `idx < n`; the out-of-range
    // label is the input under test.
    unsafe {
        launch!(relabel<<<1, 32, 0, stream>>>(
            labels.as_device_ptr(),
            mapping.as_device_ptr(),
            n as i32
        ))
        .expect("launch");
    }
    stream.synchronize().expect("sync");
    let mut out = vec![0i32; n];
    labels.copy_to(&mut out).expect("read labels");
    println!("relabelled: {out:?}");
    assert_eq!(out, vec![7, 8, -1, -1]);
}

/// The path the C1b crash masked, now reachable: SSSP on a graph that is not the construction
/// size succeeds, and its distances (one per live node) reach the force pass. Two identical
/// engines take one Hooke step from the same state, one with SSSP spring adjustment on and one
/// off; the SSSP rest-length term must change the result. The term's law itself is checked
/// against the CPU reference by the `springs_hooke_sssp` golden in the kernel harness.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn c1b_fresh_sssp_distances_reach_the_force_pass() {
    let Some(g) = gpu("c1b_fresh_sssp_distances_reach_the_force_pass") else {
        return;
    };
    let fixture = graph(1500, 1, 1.0, 0.5, 3.0, 41);
    let step = |sssp_on: bool| -> Vec<f32> {
        let mut engine = production_engine(&g);
        load(&mut engine, &fixture);
        let distances = engine.run_sssp(0, Some(2.0)).expect("SSSP after growth");
        assert_eq!(distances.len(), fixture.n);
        assert!(!engine.sssp_distances_for_force_pass().is_null());
        let physics = PhysicsSettings {
            sssp_enabled: sssp_on,
            lin_log_mode: false,
            sssp_alpha: 5.0,
            ..PhysicsSettings::default()
        };
        let mut params = SimulationParams::from(&physics);
        params.warmup_iterations = 0; // the ramp scales every force by iteration / warmup
        engine
            .execute_physics_step_with_bypass(&params, true)
            .expect("physics step");
        let (x, y, z) = engine.get_node_positions().expect("positions");
        let n = fixture.n;
        let out: Vec<f32> = x[..n]
            .iter()
            .chain(&y[..n])
            .chain(&z[..n])
            .copied()
            .collect();
        assert!(out.iter().all(|v| v.is_finite()), "non-finite positions");
        let [ix, iy, iz] = positions(n, 7);
        let start: Vec<f32> = ix.iter().chain(&iy).chain(&iz).copied().collect();
        let stepped = out
            .iter()
            .zip(&start)
            .filter(|(a, b)| (*a - *b).abs() > 1e-6)
            .count();
        assert!(stepped > 0, "the step moved no node (sssp_on={sssp_on})");
        out
    };
    let with = step(true);
    let without = step(false);
    let moved = with
        .iter()
        .zip(&without)
        .filter(|(a, b)| (*a - *b).abs() > 1e-6)
        .count();
    println!(
        "coordinates changed by the SSSP rest-length term: {moved} of {}",
        with.len()
    );
    assert!(moved > 0, "SSSP distances did not reach the force pass");
}

/// A negative weight is a negative cycle in the symmetric CSR, so the relaxation never
/// settles. Before S1a the round cap ended the loop and the garbage distances were returned
/// as a result (and kept for the force pass); now it is an error and nothing is kept.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn sssp_with_a_negative_weight_is_an_error() {
    let Some(g) = gpu("sssp_with_a_negative_weight_is_an_error") else {
        return;
    };
    let mut fixture = graph(64, 1, 1.0, 0.5, 3.0, 51);
    fixture.weights[0] = -1.0;
    // Keep the CSR symmetric: give the reverse slot the same weight.
    let (a, b) = (0usize, fixture.cols[0] as usize);
    let back = (fixture.offsets[b] as usize..fixture.offsets[b + 1] as usize)
        .find(|&e| fixture.cols[e] as usize == a)
        .expect("reverse slot");
    fixture.weights[back] = -1.0;
    let mut engine = production_engine(&g);
    load(&mut engine, &fixture);
    for delta in [None, Some(2.0)] {
        let outcome = engine.run_sssp(0, delta);
        assert!(
            outcome.is_err(),
            "delta={delta:?}: negative cycle returned distances"
        );
        println!("delta={delta:?}: {:#}", outcome.unwrap_err());
        assert!(!engine.sssp_available);
        assert!(engine.sssp_distances_for_force_pass().is_null());
    }
}

// ---------------------------------------------------------------------------
// Review round 1: SSSP delta policy, resize atomicity, graph-change invalidation
// ---------------------------------------------------------------------------

/// A path 0 - 1 - ... - (n-1) with edge `i` weighing `weight(i)`.
fn chain(n: usize, weight: impl Fn(usize) -> f32) -> Graph {
    let mut adjacency: Vec<Vec<(i32, f32)>> = vec![Vec::new(); n];
    for i in 0..n - 1 {
        let w = weight(i);
        adjacency[i].push((i as i32 + 1, w));
        adjacency[i + 1].push((i as i32, w));
    }
    let mut offsets = vec![0i32];
    let mut cols = Vec::new();
    let mut weights = Vec::new();
    for row in &adjacency {
        for &(c, w) in row {
            cols.push(c);
            weights.push(w);
        }
        offsets.push(cols.len() as i32);
    }
    Graph {
        n,
        offsets,
        cols,
        weights,
        component: vec![0; n],
    }
}

/// The same graph with node `i` renamed `n - 1 - i`: equal counts, different CSR.
fn reversed(g: &Graph) -> Graph {
    let n = g.n;
    let mut offsets = vec![0i32];
    let mut cols = Vec::new();
    let mut weights = Vec::new();
    for new_row in 0..n {
        let old = n - 1 - new_row;
        for e in g.offsets[old] as usize..g.offsets[old + 1] as usize {
            cols.push((n - 1) as i32 - g.cols[e]);
            weights.push(g.weights[e]);
        }
        offsets.push(cols.len() as i32);
    }
    Graph {
        n,
        offsets,
        cols,
        weights,
        component: g.component.iter().rev().copied().collect(),
    }
}

/// The review's measured hangs: a bucket width below half the f32 spacing at the nearest
/// unsettled distance stopped the bound advancing, and the bucket loop never ended (while
/// holding the engine lock). The loop itself must now terminate with Dijkstra's distances
/// for any width (`relax_from` bypasses `run_sssp`'s validation on purpose), and `run_sssp`
/// must refuse those widths as invalid.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn sssp_tiny_delta_terminates() {
    let Some(g) = gpu("sssp_tiny_delta_terminates") else {
        return;
    };
    // (delta, chain length, edge weight): distances reach 2.0+, 100+ and 1000+ respectively.
    for &(delta, n, w) in &[
        (1e-7f32, 64usize, 2.0f32),
        (1e-6, 64, 2.0),
        (1e-5, 600, 2.0),
    ] {
        let fixture = chain(n, |_| w);
        let mut engine = UnifiedGPUCompute::new(n, fixture.cols.len(), &g.ptx).expect("engine");
        load(&mut engine, &fixture);
        let started = std::time::Instant::now();
        let (d, _) = engine.relax_from(0, Some(delta)).expect("relax_from");
        let elapsed = started.elapsed();
        let label = format!("delta={delta:e} n={n}");
        assert_eq!(
            compare_distances(&label, &d, &dijkstra(&fixture, 0)),
            0,
            "{label}"
        );
        assert!(elapsed.as_secs() < 30, "{label}: took {elapsed:?}");
        let refused = engine
            .run_sssp(0, Some(delta))
            .expect_err("tiny delta accepted");
        println!("{label}: {elapsed:?}; run_sssp: {refused:#}");
        assert!(refused
            .to_string()
            .starts_with(super::sssp::INVALID_SSSP_DELTA));
    }
}

/// Non-finite, zero and negative widths are refused before anything runs, and a refusal
/// leaves the distances kept from the last good run in place.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn sssp_rejects_invalid_delta_and_keeps_prior_result() {
    let Some(g) = gpu("sssp_rejects_invalid_delta_and_keeps_prior_result") else {
        return;
    };
    let fixture = graph(200, 1, 1.0, 0.5, 3.0, 61);
    let mut engine = production_engine(&g);
    load(&mut engine, &fixture);
    engine.run_sssp(0, None).expect("baseline SSSP");
    for delta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -0.0, -1.0] {
        let e = engine
            .run_sssp(0, Some(delta))
            .expect_err("invalid delta accepted");
        println!("delta={delta}: {e:#}");
        assert!(e.to_string().starts_with(super::sssp::INVALID_SSSP_DELTA));
        assert!(
            engine.sssp_available,
            "a refused request discarded the kept distances"
        );
    }
}

/// Valid but awkward widths on awkward graphs, all against Dijkstra: the smallest accepted
/// width on a disconnected graph (exhausts the bucket budget, then falls back), zero-weight
/// edges (all zero, and alternating zero and one), and very large widths (one bucket).
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn sssp_delta_edge_cases_match_dijkstra() {
    let Some(g) = gpu("sssp_delta_edge_cases_match_dijkstra") else {
        return;
    };
    let split = graph(1500, 3, 1.5, 0.5, 3.0, 71);
    let max_w = split.weights.iter().copied().fold(0.0f32, f32::max);
    let floor = max_w * super::sssp::SSSP_MIN_DELTA_FRACTION;
    let cases: Vec<(&str, Graph, Vec<Option<f32>>)> = vec![
        ("split at the minimum delta", split, vec![Some(floor)]),
        (
            "all zero weights",
            chain(300, |_| 0.0),
            vec![None, Some(1e-3), Some(1.0)],
        ),
        (
            "alternating zero and one",
            chain(300, |i| (i % 2) as f32),
            vec![None, Some(1.0 / 4096.0), Some(0.5), Some(3.0)],
        ),
        (
            "very large delta",
            graph(1500, 1, 1.5, 0.5, 3.0, 72),
            vec![Some(1e30), Some(f32::MAX)],
        ),
    ];
    for (name, fixture, deltas) in cases {
        let mut engine =
            UnifiedGPUCompute::new(fixture.n, fixture.cols.len(), &g.ptx).expect("engine");
        load(&mut engine, &fixture);
        let oracle = dijkstra(&fixture, 0);
        for delta in deltas {
            let label = format!("{name} delta={delta:?}");
            let d = engine.run_sssp(0, delta).expect(&label);
            assert_eq!(compare_distances(&label, &d, &oracle), 0, "{label}");
        }
    }
}

/// A growth that fails after the fresh buffers are allocated (injected while carrying
/// positions) leaves counts, capacities, every registered buffer and the positions as they
/// were, and the next growth succeeds.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn failed_resize_leaves_the_engine_intact() {
    let Some(g) = gpu("failed_resize_leaves_the_engine_intact") else {
        return;
    };
    let mut engine = production_engine(&g);
    let before = graph(PRODUCTION_CAPACITY, 1, 1.0, 0.5, 3.0, 81);
    load(&mut engine, &before);
    let lengths = engine.graph_sized_lengths();
    let counts = (engine.num_nodes, engine.num_edges);
    let capacity = (engine.allocated_nodes, engine.allocated_edges);
    let positions = engine.get_node_positions().expect("positions");

    let after = graph(1500, 1, 1.0, 0.5, 3.0, 82);
    engine.inject_resize_carry_failure = true;
    let [x, y, z] = positions_of(&after);
    let err = engine
        .initialize_graph(
            &after.offsets,
            &after.cols,
            &after.weights,
            [&x, &y, &z],
            after.n,
            after.cols.len(),
        )
        .expect_err("injected failure ignored");
    println!("failed growth: {err:#}");
    assert_eq!(
        (engine.num_nodes, engine.num_edges),
        counts,
        "counts changed"
    );
    assert_eq!((engine.allocated_nodes, engine.allocated_edges), capacity);
    assert_eq!(
        engine.graph_sized_lengths(),
        lengths,
        "a registered buffer was replaced"
    );
    assert_eq!(engine.get_node_positions().expect("positions"), positions);

    load(&mut engine, &after);
    assert_eq!(engine.num_nodes, after.n);
    let d = engine
        .run_sssp(0, None)
        .expect("SSSP after the retried growth");
    assert_eq!(compare_distances("retry", &d, &dijkstra(&after, 0)), 0);
}

fn positions_of(g: &Graph) -> [Vec<f32>; 3] {
    positions(g.n, 7)
}

/// A graph change that keeps the counts (here the same graph with nodes renamed) or shrinks
/// them discards every node-indexed piece of state; re-uploading the identical graph (two
/// actors share the engine) keeps it.
#[test]
#[ignore = "needs GPU: run with --ignored --test-threads=1"]
fn graph_changes_discard_node_indexed_state() {
    let Some(g) = gpu("graph_changes_discard_node_indexed_state") else {
        return;
    };
    let a = graph(PRODUCTION_CAPACITY, 1, 1.0, 0.5, 3.0, 91);
    let b = reversed(&a);
    let mut engine = production_engine(&g);
    let arm = |engine: &mut UnifiedGPUCompute| {
        engine
            .upload_degree_weights(&vec![2.0; engine.num_nodes])
            .expect("degree weights");
        engine.set_compute_mask(&[0, 1, 2]).expect("mask");
        engine.run_sssp(0, None).expect("SSSP");
        engine.community_count_active = 3;
        assert!(engine.degree_weights_available && engine.compute_mask_active());
    };
    let armed = |engine: &UnifiedGPUCompute| {
        [
            engine.degree_weights_available,
            engine.compute_mask_active(),
            engine.sssp_available,
            engine.community_count_active > 0,
        ]
    };

    load(&mut engine, &a);
    arm(&mut engine);
    load(&mut engine, &a);
    assert_eq!(
        armed(&engine),
        [true, true, false, true],
        "identical re-upload"
    );

    arm(&mut engine);
    load(&mut engine, &b);
    assert_eq!(engine.num_nodes, a.n);
    assert_eq!(armed(&engine), [false; 4], "same counts, renamed nodes");

    arm(&mut engine);
    load(&mut engine, &graph(400, 1, 1.0, 0.5, 3.0, 92));
    assert_eq!(armed(&engine), [false; 4], "shrink");
}
