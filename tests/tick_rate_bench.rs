//! Physics tick-rate benchmark on a synthetic graph with the live dev shape
//! (9,688 nodes, 147,048 undirected edges, Barabasi-Albert degree distribution).
//!
//! GPU-only and slow, so it is `#[ignore]`d. Run it on a spare device:
//!
//! ```text
//! CUDA_VISIBLE_DEVICES=1 CARGO_TARGET_DIR=... \
//!   cargo test --release --test tick_rate_bench -- --ignored --nocapture
//! ```
//!
//! Reports ms/step and Hz for the whole `execute_physics_step` (stability bypass off = idle,
//! on = a held node), then a per-stage breakdown from the opt-in stage profiler, which
//! synchronises at every stage boundary and so is reported separately from the plain timing.
//! `TICK_MIN_HZ` (default 0 = report only) turns it into an assertion.

use std::time::Instant;
use visionclaw_server::models::constraints::ConstraintData;
use visionclaw_server::models::simulation_params::SimulationParams;
use visionclaw_server::utils::unified_gpu_compute::{
    set_stage_profile, take_stage_profile_us, UnifiedGPUCompute, STAGE_NAMES,
};

const NODES: usize = 9_688;
const EDGES: usize = 147_048;
/// Live constraint count (OWL mapper: 96,742 = 20,000 sibling-alignment + the rest
/// DISTANCE / SEPARATION). Override with `TICK_CONSTRAINTS` (0 disables).
const CONSTRAINTS: usize = 96_742;
/// ASSUMPTION: the ontology nodes the constraints touch are a skewed subset of the graph.
const CONSTRAINT_NODE_POOL: usize = 3_000;

/// xorshift64*: deterministic, so before/after runs see the same graph.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Barabasi-Albert preferential attachment, then trimmed/padded to exactly `EDGES` edges.
fn synthetic_edges(rng: &mut Rng) -> Vec<(usize, usize)> {
    let m = EDGES / NODES; // 15
    let mut targets: Vec<usize> = (0..=m).collect(); // repeated-endpoint urn
    let mut edges = Vec::with_capacity(EDGES + NODES);
    for a in 0..=m {
        for b in (a + 1)..=m {
            edges.push((a, b));
        }
    }
    for v in (m + 1)..NODES {
        let mut picked = Vec::with_capacity(m);
        while picked.len() < m {
            let t = targets[(rng.next() as usize) % targets.len()];
            if t != v && !picked.contains(&t) {
                picked.push(t);
            }
        }
        for t in picked {
            edges.push((v, t));
            targets.push(v);
            targets.push(t);
        }
    }
    while edges.len() < EDGES {
        let a = (rng.next() as usize) % NODES;
        let b = (rng.next() as usize) % NODES;
        if a != b {
            edges.push((a, b));
        }
    }
    edges.truncate(EDGES);
    edges
}

/// Live-sized constraint set: 20,000 ALIGNMENT (7, params[0] = axis), then alternating
/// DISTANCE (0) / SEPARATION (6). Endpoints are drawn from a squared-uniform skew over the
/// first `CONSTRAINT_NODE_POOL` nodes, so a few class nodes are hubs.
fn synthetic_constraints(rng: &mut Rng, n: usize) -> Vec<ConstraintData> {
    let pick = |rng: &mut Rng| {
        let u = rng.unit();
        ((u * u * CONSTRAINT_NODE_POOL as f32) as i32).min(CONSTRAINT_NODE_POOL as i32 - 1)
    };
    (0..n)
        .map(|i| {
            let (a, mut b) = (pick(rng), pick(rng));
            if a == b {
                b = (a + 1) % CONSTRAINT_NODE_POOL as i32;
            }
            let mut c = ConstraintData {
                count: 2,
                node_idx: [a, b, 0, 0],
                weight: 0.5,
                ..ConstraintData::default()
            };
            if i < 20_000 {
                c.kind = 7;
                c.params[0] = (i % 3) as f32;
            } else if i % 2 == 0 {
                c.kind = 0;
                c.params[0] = 30.0;
            } else {
                c.kind = 6;
                c.params[0] = 60.0;
            }
            c
        })
        .collect()
}

fn build() -> UnifiedGPUCompute {
    cust::init(cust::CudaFlags::empty()).expect("CUDA driver init");
    let ptx = visionclaw_server::utils::ptx::load_ptx_sync().expect("unified PTX");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let edges = synthetic_edges(&mut rng);

    let mut adj: Vec<Vec<(i32, f32)>> = vec![Vec::new(); NODES];
    for &(a, b) in &edges {
        adj[a].push((b as i32, 1.0));
        adj[b].push((a as i32, 1.0));
    }
    let mut row = vec![0i32; NODES + 1];
    let (mut col, mut w) = (Vec::new(), Vec::new());
    for (i, l) in adj.iter().enumerate() {
        for &(t, wt) in l {
            col.push(t);
            w.push(wt);
        }
        row[i + 1] = col.len() as i32;
    }

    // Random points in a ball: the same start the live graph is seeded from.
    let (mut px, mut py, mut pz) = (vec![0f32; NODES], vec![0f32; NODES], vec![0f32; NODES]);
    for i in 0..NODES {
        loop {
            let (x, y, z) = (
                rng.unit() * 2.0 - 1.0,
                rng.unit() * 2.0 - 1.0,
                rng.unit() * 2.0 - 1.0,
            );
            if x * x + y * y + z * z <= 1.0 {
                (px[i], py[i], pz[i]) = (x * 400.0, y * 400.0, z * 400.0);
                break;
            }
        }
    }

    let mut gpu = UnifiedGPUCompute::new(NODES, col.len(), &ptx).expect("GPU compute on device 0");
    gpu.initialize_graph(&row, &col, &w, [&px, &py, &pz], NODES, col.len())
        .expect("graph upload");
    // Same degree weighting the actor uploads (log(1+deg), median-normalised).
    let dw: Vec<f32> = (0..NODES)
        .map(|i| (1.0 + (row[i + 1] - row[i]) as f32).ln())
        .collect();
    let mut sorted: Vec<f32> = dw.iter().copied().filter(|&x| x > 1e-6).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let norm = 1.0 / sorted[sorted.len() / 2];
    let dw: Vec<f32> = dw.iter().map(|&x| x * norm).collect();
    gpu.upload_degree_weights(&dw).expect("degree weights");
    gpu
}

/// The settings the live backend logs on its first step.
fn live_params() -> SimulationParams {
    let mut p = SimulationParams::new();
    p.dt = 0.016;
    p.damping = 0.9;
    p.repel_k = 343.98535;
    p.spring_k = 12.0;
    p.center_gravity_k = 0.2;
    // Attribution knobs for the remaining force-pass cost: TICK_REPEL_K=0 / TICK_SPRING_K=0.
    if let Some(v) = std::env::var("TICK_REPEL_K").ok().and_then(|v| v.parse().ok()) {
        p.repel_k = v;
    }
    if let Some(v) = std::env::var("TICK_SPRING_K").ok().and_then(|v| v.parse().ok()) {
        p.spring_k = v;
    }
    p
}

fn run(gpu: &mut UnifiedGPUCompute, p: &SimulationParams, bypass: bool, steps: usize) -> f64 {
    let t = Instant::now();
    for _ in 0..steps {
        gpu.execute_physics_step_with_bypass(p, bypass).expect("step");
    }
    t.elapsed().as_secs_f64() * 1000.0 / steps as f64
}

fn rep(gpu: &mut UnifiedGPUCompute, p: &SimulationParams, bypass: bool, label: &str) -> f64 {
    let v: Vec<f64> = (0..3).map(|_| run(gpu, p, bypass, 150)).collect();
    let (lo, hi) = (
        v.iter().cloned().fold(f64::MAX, f64::min),
        v.iter().cloned().fold(f64::MIN, f64::max),
    );
    let mean = v.iter().sum::<f64>() / 3.0;
    println!(
        "{label:<34} {mean:7.2} ms/step = {:6.1} Hz   (3 reps x150, min {lo:.2} max {hi:.2})",
        1000.0 / mean
    );
    mean
}

/// Positions after a fixed number of steps from the fixed start, for before/after comparison.
fn golden(constraints: usize) -> Vec<f32> {
    let mut gpu = build();
    let mut p = live_params();
    p.cluster_strength = 0.0;
    if constraints > 0 {
        gpu.set_constraints(synthetic_constraints(&mut Rng(7), constraints)).unwrap();
    }
    run(&mut gpu, &p, true, 60);
    let (x, y, z) = gpu.get_node_positions().unwrap();
    x.iter().chain(y.iter()).chain(z.iter()).copied().collect()
}

#[test]
#[ignore = "needs a CUDA device; run with CUDA_VISIBLE_DEVICES=<spare gpu> --ignored"]
fn physics_tick_rate_on_live_shape_graph() {
    let n_constraints: usize = std::env::var("TICK_CONSTRAINTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CONSTRAINTS);
    let min_hz: f64 = std::env::var("TICK_MIN_HZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);

    // Determinism and golden file (before/after comparison of the physics result).
    let g1 = golden(n_constraints);
    let g2 = golden(n_constraints);
    let run_to_run = g1.iter().zip(&g2).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    println!("GOLDEN run-to-run max |dx| over 60 steps: {run_to_run:e}");
    if let Ok(path) = std::env::var("TICK_GOLDEN_OUT") {
        let bytes: Vec<u8> = g1.iter().flat_map(|f| f.to_le_bytes()).collect();
        std::fs::write(&path, bytes).unwrap();
        println!("GOLDEN written to {path}");
    }
    if let Ok(path) = std::env::var("TICK_GOLDEN_CMP") {
        let raw = std::fs::read(&path).unwrap();
        let base: Vec<f32> = raw.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        assert_eq!(base.len(), g1.len());
        let max = base.iter().zip(&g1).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        let exact = base.iter().zip(&g1).filter(|(a, b)| a.to_bits() == b.to_bits()).count();
        println!("GOLDEN vs {path}: max |dx| {max:e}, bit-identical {exact}/{}", g1.len());
    }

    let mut gpu = build();
    let mut p = live_params();
    p.cluster_strength = 0.0; // keep the Louvain refresh out of the steady-state numbers
    println!("nodes={NODES} undirected_edges={EDGES} constraints={n_constraints}");

    // Let the layout leave its random-ball start so the grid density is realistic.
    run(&mut gpu, &p, true, 400);
    println!("-- WITHOUT constraints");
    let h0 = rep(&mut gpu, &p, true, "HELD (stability bypass)");
    let i0 = rep(&mut gpu, &p, false, "IDLE (stability gate)");

    let (mut held, mut idle) = (h0, i0);
    if n_constraints > 0 {
        gpu.set_constraints(synthetic_constraints(&mut Rng(7), n_constraints)).unwrap();
        run(&mut gpu, &p, true, 100);
        println!("-- WITH {n_constraints} constraints");
        held = rep(&mut gpu, &p, true, "HELD (stability bypass)");
        idle = rep(&mut gpu, &p, false, "IDLE (stability gate)");
    }

    // Idle must actually integrate: a stability skip would return early and flatter the number.
    let before = gpu.get_node_positions().unwrap().0;
    run(&mut gpu, &p, false, 5);
    let after = gpu.get_node_positions().unwrap().0;
    let moved = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    println!("IDLE integrates: {moved}/{} nodes moved in 5 steps", before.len());

    let t = Instant::now();
    for _ in 0..50 {
        gpu.get_node_positions().unwrap();
        gpu.get_node_velocities().unwrap();
    }
    println!("READBACK pos+vel: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 50.0);

    set_stage_profile(true);
    let _ = take_stage_profile_us();
    let prof_steps = 100;
    let prof = run(&mut gpu, &p, true, prof_steps);
    set_stage_profile(false);
    let us = take_stage_profile_us();
    println!("STAGES (held, synchronised at each boundary; total {prof:.2} ms/step):");
    for (name, u) in STAGE_NAMES.iter().zip(us.iter()) {
        println!("  {:<46} {:8.3} ms", name, *u as f64 / 1000.0 / prof_steps as f64);
    }

    if min_hz > 0.0 {
        assert!(
            1000.0 / held >= min_hz && 1000.0 / idle >= min_hz,
            "tick rate below {min_hz} Hz: held {:.1} Hz, idle {:.1} Hz",
            1000.0 / held,
            1000.0 / idle
        );
    }
}
