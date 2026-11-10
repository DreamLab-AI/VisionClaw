//! Golden baseline of the force kernels before the CUDA rework.
//!
//! Every test drives the compiled PTX directly (`visionclaw_unified.cu` and
//! `semantic_forces.cu`) through the same launch sequence the server uses,
//! over four fixture graphs, and checks three things:
//!
//! 1. the per-node forces match a plain-Rust CPU reference of the law **as the
//!    kernel implements it today** (`reference.rs`, bound documented there);
//! 2. the forces match the committed golden for that scenario (`golden.rs`);
//! 3. separately, repeated runs are compared for determinism.
//!
//! Where the kernel departs from its own stated law, the as-implemented test
//! passes (it pins today's behaviour) and a second test named
//! `intended_law_*` asserts the stated law and is marked as an expected
//! failure with `#[should_panic(expected = "INTENDED-LAW DEPARTURE")]`. When
//! the rework fixes the kernel, that test starts failing; flip it to a plain
//! test and regenerate the affected goldens in the same change.
//!
//! Everything is `#[ignore]`d so a CPU-only `cargo test` stays green. Run on
//! a CUDA host as described in `README.md` in this directory. A host with no
//! CUDA driver or device skips the as-implemented tests with a `SKIP` line;
//! any other failure (including a PTX load or JIT failure) fails the test.

#![allow(clippy::too_many_arguments)]

mod fixtures;
mod golden;
mod harness;
mod reference;

use fixtures::Fixture;
use golden::Golden;
use harness::{
    flags, gpu, kind, Centroids, ConstraintData, ForceInputs, ForceKernel, Forces, Gpu,
    Relationship, SemanticConfig, SemanticKernel, SimParams, TYPE_SLOTS,
};

// ===========================================================================
// Parameters
// ===========================================================================

/// Neutral parameters: every term off, caps far above any force the fixtures
/// produce, so a scenario switches on exactly the terms it names.
fn base() -> SimParams {
    SimParams {
        dt: 0.2,
        damping: 0.9,
        spring_k: 0.05,
        rest_length: 8.0,
        repel_k: 50.0,
        repulsion_cutoff: 25.0,
        repulsion_softening_epsilon: 0.5,
        max_force: 1.0e6,
        max_velocity: 100.0,
        grid_cell_size: 25.0,
        alignment_strength: 0.8,
        sssp_alpha: 0.5,
        constraint_max_force_per_node: 1.0e6,
        norm_delta_cap: 10.0,
        position_constraint_attraction: 0.5,
        scaling_ratio: 2.0,
        dag_level_distance: 15.0,
        plane_spacing: 12.0,
        layer_spacing: 10.0,
        ..SimParams::default()
    }
}

/// One constraint of every live kind, including a mid-ramp and a zero-ramp
/// constraint, plus (on the large fixture) a spread of random-ish pairs.
fn constraints_for(fx: &Fixture) -> Vec<ConstraintData> {
    let n = fx.n() as i32;
    let last = n - 1;
    let dist = |a: i32, b: i32| {
        let (p, q) = (fx.pos[a as usize], fx.pos[b as usize]);
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
    };
    let c = |kind: i32, count: i32, nodes: [i32; 2], params: [f32; 3], weight: f32, frame: i32| {
        ConstraintData {
            kind,
            count,
            node_idx: [nodes[0], nodes[1], -1, -1],
            params: [params[0], params[1], params[2], 0.0, 0.0, 0.0, 0.0, 0.0],
            weight,
            activation_frame: frame,
        }
    };
    let mut out = vec![
        c(kind::DISTANCE, 2, [0, 1], [7.0, 0.0, 0.0], 0.3, -100),
        c(kind::POSITION, 1, [last, -1], [1.0, 2.0, 3.0], 0.5, 2),
        c(
            kind::SEPARATION,
            2,
            [0, last],
            [dist(0, last) + 3.0, 0.0, 0.0],
            0.4,
            -100,
        ),
        c(kind::ALIGNMENT, 2, [1, last], [1.0, 0.0, 0.0], 0.6, 5),
    ];
    if n > 16 {
        for k in 0..32 {
            let (a, b) = ((k * 7 + 3) % n, (k * 13 + 11) % n);
            if a == b {
                continue;
            }
            let kd = [kind::DISTANCE, kind::SEPARATION, kind::ALIGNMENT][k as usize % 3];
            let p0 = match kd {
                kind::DISTANCE => 12.0,
                kind::SEPARATION => dist(a, b) + 2.5,
                _ => (k % 3) as f32,
            };
            out.push(c(kd, 2, [a, b], [p0, 0.0, 0.0], 0.25, -100));
        }
    }
    out
}

// ===========================================================================
// Unified force-pass scenarios
// ===========================================================================

type Setup = fn(&Fixture, &Gpu) -> (Fixture, SimParams, ForceInputs);

fn run_unified(name: &str, which: ForceKernel, setup: Setup) {
    let Some(g) = gpu(name) else { return };
    let mut golden = Golden::new(name, &g.device);
    for fx in fixtures::all() {
        let (eval, params, inputs) = setup(&fx, &g);
        let got = g.force_pass(&eval, &params, &inputs, which);
        let r = reference::force_pass(&eval, &params, &inputs, which);
        let ratio = reference::assert_matches(&format!("{name}/{}", fx.name), &got, &r);
        println!("  {name}/{}: reference worst |Δ|/bound {ratio:.3}", fx.name);
        golden.add(&eval, &got);
    }
    golden.finish();
}

macro_rules! unified {
    ($(#[doc = $d:literal])* $name:ident, $which:expr, $setup:expr) => {
        $(#[doc = $d])*
        #[test]
        #[ignore = "needs a CUDA device; see README.md"]
        fn $name() {
            run_unified(stringify!($name), $which, $setup);
        }
    };
}

fn repulsion_only() -> SimParams {
    SimParams {
        feature_flags: flags::REPULSION,
        ..base()
    }
}

unified!(
    /// Classic repulsion `repel_k / (d^2 + eps)` alone, no class data.
    repulsion_classic,
    ForceKernel::Main,
    |fx, _| (fx.clone(), repulsion_only(), ForceInputs::default())
);

unified!(
    /// Classic repulsion with domain modulation (same class x0.1, different
    /// x3) and, for class 0 nodes, the charge product.
    repulsion_class_modulated,
    ForceKernel::Main,
    |fx, _| {
        let inputs = ForceInputs {
            class_id: Some(fx.class_id.clone()),
            class_charge: Some(fx.class_charge.clone()),
            ..ForceInputs::default()
        };
        (fx.clone(), repulsion_only(), inputs)
    }
);

unified!(
    /// ForceAtlas2 degree-scaled repulsion (LinLog mode with node degrees).
    repulsion_fa2,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            feature_flags: flags::REPULSION,
            lin_log_mode: 1,
            ..base()
        };
        let inputs = ForceInputs {
            node_degrees: Some(fx.degrees()),
            ..ForceInputs::default()
        };
        (fx.clone(), p, inputs)
    }
);

unified!(
    /// Classic repulsion with `max_force = 0.5`, so close pairs hit the cap.
    repulsion_capped,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            feature_flags: flags::REPULSION,
            max_force: 0.5,
            ..base()
        };
        (fx.clone(), p, ForceInputs::default())
    }
);

unified!(
    /// Short-range hard separation inside `separation_radius`, on top of
    /// classic repulsion.
    repulsion_separation_radius,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            feature_flags: flags::REPULSION,
            separation_radius: 4.5,
            max_force: 20.0,
            ..base()
        };
        (fx.clone(), p, ForceInputs::default())
    }
);

fn springs(fx: &Fixture, lin_log: bool) -> (Fixture, SimParams, ForceInputs) {
    let p = SimParams {
        feature_flags: flags::SPRINGS,
        lin_log_mode: u32::from(lin_log),
        ..base()
    };
    (fx.clone(), p, ForceInputs::default())
}

unified!(
    /// LinLog attraction `log(1 + d) * w`.
    springs_linlog,
    ForceKernel::Main,
    |fx, _| springs(fx, true)
);

unified!(
    /// LinLog attraction with the per-node spring multiplier.
    springs_linlog_scaled,
    ForceKernel::Main,
    |fx, _| {
        let (f, p, mut i) = springs(fx, true);
        i.spring_scale = Some(fx.spring_scale.clone());
        (f, p, i)
    }
);

unified!(
    /// Hooke springs `spring_k * (d - rest_length) * w`; also the "hold
    /// overlay off" case (no rest snapshot bound).
    springs_hooke,
    ForceKernel::Main,
    |fx, _| springs(fx, false)
);

unified!(
    /// Hooke springs with the SSSP-adjusted rest length.
    springs_hooke_sssp,
    ForceKernel::Main,
    |fx, _| {
        let (f, mut p, mut i) = springs(fx, false);
        p.feature_flags |= flags::SSSP_SPRING_ADJUST;
        i.sssp = Some(fx.sssp.clone());
        (f, p, i)
    }
);

fn overlay(fx: &Fixture, g: &Gpu, lin_log: bool) -> (Fixture, SimParams, ForceInputs) {
    let rest = g.snapshot_edge_rest(fx);
    let (_, p, mut i) = springs(fx, lin_log);
    i.edge_rest = Some(rest);
    (fx.displaced(), p, i)
}

unified!(
    /// Hold overlay on, Hooke mode: rest lengths snapshot at the original
    /// positions by `snapshot_edge_rest_kernel`, forces at displaced ones.
    hold_overlay_hooke,
    ForceKernel::Main,
    |fx, g| overlay(fx, g, false)
);

unified!(
    /// Hold overlay on, LinLog mode (the production default).
    hold_overlay_linlog,
    ForceKernel::Main,
    |fx, g| overlay(fx, g, true)
);

unified!(
    /// Uniform centre gravity (`ENABLE_CENTERING`).
    centering,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            feature_flags: flags::CENTERING,
            center_gravity_k: 0.03,
            ..base()
        };
        (fx.clone(), p, ForceInputs::default())
    }
);

unified!(
    /// The ungated `gravity` centre pull.
    gravity,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            gravity: 0.02,
            ..base()
        };
        (fx.clone(), p, ForceInputs::default())
    }
);

unified!(
    /// DAG radial shell bias around an off-origin centre that coincides with
    /// a ranked node of the `pair` fixture (exercises the deterministic nudge).
    dag_radial,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            dag_bias_k: 0.1,
            radial_center_x: 3.0,
            radial_center_y: 4.0,
            radial_center_z: 0.0,
            ..base()
        };
        let i = ForceInputs {
            node_rank: Some(fx.rank.clone()),
            ..ForceInputs::default()
        };
        (fx.clone(), p, i)
    }
);

unified!(
    /// Stratified planes: Z spring toward `plane * plane_spacing`.
    plane_bias,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            plane_bias_k: 0.2,
            ..base()
        };
        let i = ForceInputs {
            node_plane: Some(fx.plane.clone()),
            ..ForceInputs::default()
        };
        (fx.clone(), p, i)
    }
);

unified!(
    /// Sugiyama layers: Y spring toward `rank * layer_spacing`.
    layer_bias,
    ForceKernel::Main,
    |fx, _| {
        let p = SimParams {
            layer_bias_k: 0.15,
            ..base()
        };
        let i = ForceInputs {
            node_rank: Some(fx.rank.clone()),
            ..ForceInputs::default()
        };
        (fx.clone(), p, i)
    }
);

unified!(
    /// Distance, position, separation and alignment constraints with the
    /// progressive ramp mid-way (iteration 5 of 10).
    constraints,
    ForceKernel::Main,
    |fx, _| constraints_setup(fx)
);

fn constraints_setup(fx: &Fixture) -> (Fixture, SimParams, ForceInputs) {
    let p = SimParams {
        feature_flags: flags::CONSTRAINTS,
        constraint_ramp_frames: 10,
        iteration: 5,
        ..base()
    };
    let i = ForceInputs {
        constraints: constraints_for(fx),
        ..ForceInputs::default()
    };
    (fx.clone(), p, i)
}

/// Every term at once, production-style (LinLog, FA2 degrees, overlay on).
fn combined_setup(fx: &Fixture, g: &Gpu) -> (Fixture, SimParams, ForceInputs) {
    let rest = g.snapshot_edge_rest(fx);
    let p = SimParams {
        feature_flags: flags::REPULSION | flags::SPRINGS | flags::CENTERING | flags::CONSTRAINTS,
        lin_log_mode: 1,
        center_gravity_k: 0.03,
        gravity: 0.02,
        separation_radius: 2.0,
        max_force: 40.0,
        dag_bias_k: 0.1,
        plane_bias_k: 0.2,
        layer_bias_k: 0.15,
        constraint_ramp_frames: 10,
        iteration: 5,
        ..base()
    };
    let i = ForceInputs {
        node_degrees: Some(fx.degrees()),
        class_id: Some(fx.class_id.clone()),
        class_charge: Some(fx.class_charge.clone()),
        spring_scale: Some(fx.spring_scale.clone()),
        node_rank: Some(fx.rank.clone()),
        node_plane: Some(fx.plane.clone()),
        edge_rest: Some(rest),
        constraints: constraints_for(fx),
        ..ForceInputs::default()
    };
    (fx.displaced(), p, i)
}

/// Classic repulsion, Hooke+SSSP springs, centring, gravity and
/// constraints: the non-LinLog production path.
fn classic_setup(fx: &Fixture, _: &Gpu) -> (Fixture, SimParams, ForceInputs) {
    let p = SimParams {
        feature_flags: flags::REPULSION
            | flags::SPRINGS
            | flags::CENTERING
            | flags::CONSTRAINTS
            | flags::SSSP_SPRING_ADJUST,
        center_gravity_k: 0.03,
        gravity: 0.02,
        constraint_ramp_frames: 10,
        iteration: 5,
        ..base()
    };
    let i = ForceInputs {
        class_id: Some(fx.class_id.clone()),
        class_charge: Some(fx.class_charge.clone()),
        sssp: Some(fx.sssp.clone()),
        constraints: constraints_for(fx),
        ..ForceInputs::default()
    };
    (fx.clone(), p, i)
}

unified!(
    /// All terms together through `force_pass_kernel`, LinLog path.
    combined_linlog,
    ForceKernel::Main,
    combined_setup
);
unified!(
    /// All terms together through `force_pass_kernel`, classic path.
    combined_classic,
    ForceKernel::Main,
    classic_setup
);
unified!(
    /// The LinLog combination through `force_pass_with_stability_kernel` with
    /// the gate open, as implemented (no gravity, no class modulation).
    stability_combined_linlog,
    ForceKernel::Stability,
    combined_setup
);
unified!(
    /// The classic combination through `force_pass_with_stability_kernel`.
    stability_combined_classic,
    ForceKernel::Stability,
    classic_setup
);

// ===========================================================================
// Small unified kernels
// ===========================================================================

#[test]
#[ignore = "needs a CUDA device; see README.md"]
fn snapshot_edge_rest() {
    let name = "snapshot_edge_rest";
    let Some(g) = gpu(name) else { return };
    let mut golden = Golden::new(name, &g.device);
    for fx in fixtures::all() {
        let got: Forces = g
            .snapshot_edge_rest(&fx)
            .into_iter()
            .map(|d| [d, 0.0, 0.0])
            .collect();
        let r = reference::edge_rest(&fx);
        reference::assert_matches(&format!("{name}/{}", fx.name), &got, &r);
        golden.add(&fx, &got);
    }
    golden.finish();
}

fn degree_weight(fx: &Fixture) -> Vec<f32> {
    let csr = fx.csr();
    (0..fx.n())
        .map(|i| ((csr.offsets[i + 1] - csr.offsets[i]) as f32).ln_1p())
        .collect()
}

#[test]
#[ignore = "needs a CUDA device; see README.md"]
fn degree_weighted_gravity() {
    let name = "degree_weighted_gravity";
    let Some(g) = gpu(name) else { return };
    let mut golden = Golden::new(name, &g.device);
    for fx in fixtures::all() {
        let p = SimParams {
            feature_flags: flags::CENTERING,
            center_gravity_k: 0.03,
            ..base()
        };
        let base_f = g.force_pass(&fx, &p, &ForceInputs::default(), ForceKernel::Main);
        let dw = degree_weight(&fx);
        let got = g.degree_weighted_gravity(&fx, &base_f, &dw, 0.03, 80.0, 0.05);
        let r = reference::degree_weighted_gravity(&fx, &base_f, &dw, 0.03, 80.0, 0.05);
        reference::assert_matches(&format!("{name}/{}", fx.name), &got, &r);
        golden.add(&fx, &got);
    }
    golden.finish();
}

#[test]
#[ignore = "needs a CUDA device; see README.md"]
fn cluster_cohesion() {
    let name = "cluster_cohesion";
    let Some(g) = gpu(name) else { return };
    let mut golden = Golden::new(name, &g.device);
    for fx in fixtures::all() {
        let cents = reference::centroids(&fx, &fx.hierarchy_level, 4);
        let got = g.cluster_cohesion(&fx, &cents, &fx.hierarchy_level, 0.01);
        let r = reference::cluster_cohesion(&fx, &cents, &fx.hierarchy_level, 0.01);
        reference::assert_matches(&format!("{name}/{}", fx.name), &got, &r);
        golden.add(&fx, &got);
    }
    golden.finish();
}

// ===========================================================================
// Semantic scenarios
// ===========================================================================

/// Every semantic force switched on with distinct, hand-picked values.
fn semantic_all() -> SemanticConfig {
    SemanticConfig {
        dag: [20.0, 15.0, 0.1, 0.5],
        dag_enabled: true,
        type_cluster: [0.05, 12.0, 30.0],
        type_cluster_enabled: true,
        collision: [1.5, 0.8, 2.0],
        collision_enabled: true,
        attribute_spring: [0.04, 0.5, 4.0, 12.0],
        attribute_spring_enabled: true,
        physicality: [0.05, 10.0, 20.0],
        physicality_enabled: true,
        role: [0.04, 14.0, 25.0],
        role_enabled: true,
        maturity: [0.0, 0.07, 30.0],
        maturity_enabled: true,
        cross_domain: [0.1, 0.25, 2.0, 50.0],
        cross_domain_enabled: true,
    }
}

/// Only the named force enabled (the dynamic table has its own switch).
fn semantic_only(which: SemanticKernel) -> SemanticConfig {
    let all = semantic_all();
    let mut c = SemanticConfig {
        dag: all.dag,
        type_cluster: all.type_cluster,
        collision: all.collision,
        attribute_spring: all.attribute_spring,
        physicality: all.physicality,
        role: all.role,
        maturity: all.maturity,
        cross_domain: all.cross_domain,
        ..SemanticConfig::default()
    };
    match which {
        SemanticKernel::Dag => c.dag_enabled = true,
        SemanticKernel::TypeCluster => c.type_cluster_enabled = true,
        SemanticKernel::Collision => c.collision_enabled = true,
        SemanticKernel::AttributeSpring => c.attribute_spring_enabled = true,
        SemanticKernel::Physicality => c.physicality_enabled = true,
        SemanticKernel::Role => c.role_enabled = true,
        SemanticKernel::Maturity => c.maturity_enabled = true,
        SemanticKernel::Dynamic => c.cross_domain_enabled = true,
        SemanticKernel::Production => c = all,
    }
    c
}

/// The relationship table: one entry per force type, plus a zero-strength
/// entry the kernel must skip. Edge type 7 in `degenerate` is outside it.
fn relationship_table() -> Vec<Relationship> {
    let r = |strength, rest_length, directional, force_type| Relationship {
        strength,
        rest_length,
        directional,
        force_type,
    };
    vec![
        r(0.08, 10.0, false, 0),
        r(0.05, 6.0, true, 1),
        r(0.03, 40.0, false, 2),
        r(-0.3, 30.0, false, 3),
        r(0.0, 10.0, false, 0),
    ]
}

fn centroids_for(fx: &Fixture) -> Centroids {
    Centroids {
        types: reference::centroids(fx, &fx.node_type, TYPE_SLOTS),
        physicality: reference::centroids(fx, &fx.physicality, TYPE_SLOTS),
        role: reference::centroids(fx, &fx.role, TYPE_SLOTS),
    }
}

fn run_semantic(name: &str, which: SemanticKernel) {
    let Some(g) = gpu(name) else { return };
    let cfg = semantic_only(which);
    let table = relationship_table();
    let dynamic = matches!(which, SemanticKernel::Dynamic | SemanticKernel::Production);
    let mut golden = Golden::new(name, &g.device);
    for fx in fixtures::all() {
        let cents = centroids_for(&fx);
        let got = g.semantic(&fx, &cfg, &table, dynamic, &cents, which);
        if which != SemanticKernel::Production {
            let r = reference::semantic(&fx, &cfg, &table, dynamic, &cents, which);
            let ratio = reference::assert_matches(&format!("{name}/{}", fx.name), &got, &r);
            println!("  {name}/{}: reference worst |Δ|/bound {ratio:.3}", fx.name);
        }
        golden.add(&fx, &got);
    }
    golden.finish();
}

macro_rules! semantic {
    ($(#[doc = $d:literal])* $name:ident, $which:expr) => {
        $(#[doc = $d])*
        #[test]
        #[ignore = "needs a CUDA device; see README.md"]
        fn $name() {
            run_semantic(stringify!($name), $which);
        }
    };
}

semantic!(
    /// `apply_dag_force`: level spring on Y plus horizontal sibling repulsion.
    semantic_dag,
    SemanticKernel::Dag
);
semantic!(
    /// `apply_type_cluster_force` with known centroids.
    semantic_type_cluster,
    SemanticKernel::TypeCluster
);
semantic!(
    /// `apply_collision_force` with the default radius (null radii, as served).
    semantic_collision,
    SemanticKernel::Collision
);
semantic!(
    /// `apply_attribute_spring_force` as implemented (rest length unclamped).
    semantic_attribute_spring,
    SemanticKernel::AttributeSpring
);
semantic!(
    /// `apply_physicality_cluster_force` with known centroids.
    semantic_physicality,
    SemanticKernel::Physicality
);
semantic!(
    /// `apply_role_cluster_force` with known centroids.
    semantic_role,
    SemanticKernel::Role
);
semantic!(
    /// `apply_maturity_layout_force`: Z spring toward the stage plane.
    semantic_maturity,
    SemanticKernel::Maturity
);
semantic!(
    /// `apply_dynamic_relationship_force` over the relationship table, as
    /// implemented (including the double division by distance).
    semantic_dynamic,
    SemanticKernel::Dynamic
);
semantic!(
    /// The server's whole `SemanticGpu::apply` sequence with every force on.
    /// Golden only: its centroids are float-atomic sums (see README).
    semantic_production,
    SemanticKernel::Production
);

#[test]
#[ignore = "needs a CUDA device; see README.md"]
fn semantic_config_reads_back_through_the_device_struct() {
    let Some(g) = gpu("semantic_config_probe") else {
        return;
    };
    let fx = fixtures::pair();
    let cfg = semantic_all();
    // Any run uploads the config and table; then read them back.
    g.semantic(
        &fx,
        &cfg,
        &relationship_table(),
        true,
        &centroids_for(&fx),
        SemanticKernel::Maturity,
    );
    let v = g.semantic_probe();
    let want = [
        140.0, 20.0, 1.0, 12.0, 2.0, 12.0, 20.0, 30.0, 50.0, 1.0, 4108.0, 5.0, 6.0, 1.0, 1.0,
    ];
    assert_eq!(
        v, want,
        "device view of c_semantic_config / c_dynamic_relationships"
    );
}

// ===========================================================================
// Determinism
// ===========================================================================

/// Each kernel ten times on the large fixture; report which differ run to run
/// and by how much. Kernels without float atomics on their output must be
/// bitwise identical; the float-atomic ones are reported and bounded by the
/// golden tolerance.
#[test]
#[ignore = "needs a CUDA device; see README.md"]
fn determinism() {
    const RUNS: usize = 10;
    let Some(gpu_ctx) = gpu("determinism") else {
        return;
    };
    let g = &gpu_ctx;
    let fx = &fixtures::mixed256();
    let table = &relationship_table();
    let cents = &centroids_for(fx);
    type Run<'a> = Box<dyn Fn() -> Forces + 'a>;
    let unified = |which: ForceKernel, setup: Setup| -> Run<'_> {
        let (eval, p, i) = setup(fx, g);
        Box::new(move || g.force_pass(&eval, &p, &i, which))
    };
    let semantic = |which: SemanticKernel| -> Run<'_> {
        let cfg = semantic_only(which);
        let dynamic = matches!(which, SemanticKernel::Dynamic | SemanticKernel::Production);
        Box::new(move || g.semantic(fx, &cfg, table, dynamic, cents, which))
    };
    // (name, has float atomics on its output, runner)
    let cases: Vec<(&str, bool, Run<'_>)> = vec![
        (
            "force_pass_kernel (combined LinLog)",
            false,
            unified(ForceKernel::Main, combined_setup),
        ),
        (
            "force_pass_kernel (combined classic)",
            false,
            unified(ForceKernel::Main, classic_setup),
        ),
        (
            "force_pass_with_stability_kernel (combined LinLog)",
            false,
            unified(ForceKernel::Stability, combined_setup),
        ),
        (
            "snapshot_edge_rest_kernel",
            false,
            Box::new(|| {
                g.snapshot_edge_rest(fx)
                    .into_iter()
                    .map(|d| [d, 0.0, 0.0])
                    .collect()
            }),
        ),
        (
            "cluster_cohesion_kernel",
            false,
            Box::new(|| {
                let c = reference::centroids(fx, &fx.hierarchy_level, 4);
                g.cluster_cohesion(fx, &c, &fx.hierarchy_level, 0.01)
            }),
        ),
        ("apply_dag_force", false, semantic(SemanticKernel::Dag)),
        (
            "apply_type_cluster_force",
            false,
            semantic(SemanticKernel::TypeCluster),
        ),
        (
            "apply_collision_force",
            false,
            semantic(SemanticKernel::Collision),
        ),
        (
            "apply_physicality_cluster_force",
            false,
            semantic(SemanticKernel::Physicality),
        ),
        (
            "apply_role_cluster_force",
            false,
            semantic(SemanticKernel::Role),
        ),
        (
            "apply_maturity_layout_force",
            false,
            semantic(SemanticKernel::Maturity),
        ),
        (
            "apply_attribute_spring_force",
            true,
            semantic(SemanticKernel::AttributeSpring),
        ),
        (
            "apply_dynamic_relationship_force",
            true,
            semantic(SemanticKernel::Dynamic),
        ),
        (
            "SemanticGpu::apply sequence",
            true,
            semantic(SemanticKernel::Production),
        ),
    ];
    println!(
        "determinism over {RUNS} runs on mixed256 ({} nodes):",
        fx.n()
    );
    let mut failures = Vec::new();
    for (name, atomics, run) in &cases {
        let first = run();
        let scale = first
            .iter()
            .flat_map(|f| f.iter())
            .fold(0f32, |a, v| a.max(v.abs()));
        let (mut differing, mut max_abs, mut max_rel, mut worst) = (0usize, 0f32, 0f32, 0f64);
        for _ in 1..RUNS {
            let again = run();
            for (a, b) in first.iter().flatten().zip(again.iter().flatten()) {
                if a.to_bits() != b.to_bits() {
                    differing += 1;
                    let d = (a - b).abs();
                    max_abs = max_abs.max(d);
                    max_rel = max_rel.max(d / a.abs().max(f32::MIN_POSITIVE));
                    // The golden bound: 1e-5 * |value| + 1e-6 * field max.
                    let bound = 1e-5 * a.abs() as f64 + 1e-6 * scale as f64;
                    worst = worst.max(d as f64 / bound.max(f64::MIN_POSITIVE));
                }
            }
        }
        println!(
            "  {name:52} atomics={atomics:<5} differing components {differing:5} / {:5} \
             max|Δ| {max_abs:.3e} (max relative {max_rel:.3e}, field max {scale:.3e})",
            first.len() * 3 * (RUNS - 1)
        );
        if !atomics && differing > 0 {
            failures.push(*name);
        }
        assert!(
            worst <= 1.0,
            "{name}: run-to-run spread reaches {worst:.2}x the golden tolerance"
        );
    }
    assert!(
        failures.is_empty(),
        "kernels without float atomics were not bitwise reproducible: {failures:?}"
    );
}

// ===========================================================================
// Expected failures: the kernel departs from its own stated law
// ===========================================================================

/// Two nodes `d` apart along a Pythagorean direction, one edge of `kind`.
fn two_nodes(a: [f32; 3], b: [f32; 3], weight: f32, kind: i32) -> Fixture {
    let mut fx = fixtures::pair();
    fx.pos = vec![a, b];
    fx.edges = vec![fixtures::Edge {
        src: 0,
        dst: 1,
        weight,
        kind,
    }];
    fx
}

fn require_gpu(name: &str) -> Gpu {
    gpu(name).unwrap_or_else(|| {
        panic!("{name}: no CUDA device, so this expected-failure test cannot run")
    })
}

fn dynamic_only(fx: &Fixture, g: &Gpu, rel: Relationship) -> Forces {
    let cfg = semantic_only(SemanticKernel::Dynamic);
    g.semantic(
        fx,
        &cfg,
        &[rel],
        true,
        &centroids_for(fx),
        SemanticKernel::Dynamic,
    )
}

/// EXPECTED FAILURE. `semantic_forces.cu` lines 560-563 state Hooke's law
/// `F = -k (x - x0)`, but compute `strength * (d - rest) / d` and then
/// multiply by the unit vector, dividing by distance twice. Case: k = 1,
/// rest 5, d = 10: Hooke gives |F| = 5 on each end; the kernel gives 0.5.
#[test]
#[ignore = "needs a CUDA device; see README.md"]
#[should_panic(expected = "INTENDED-LAW DEPARTURE")]
fn intended_law_dynamic_spring_is_hookean() {
    let g = require_gpu("intended_law_dynamic_spring_is_hookean");
    let fx = two_nodes([0.0, 0.0, 0.0], [6.0, 8.0, 0.0], 1.0, 0);
    let rel = Relationship {
        strength: 1.0,
        rest_length: 5.0,
        directional: false,
        force_type: 0,
    };
    let f = dynamic_only(&fx, &g, rel);
    let got = (f[0][0].powi(2) + f[0][1].powi(2) + f[0][2].powi(2)).sqrt();
    let want = 5.0f32;
    println!("dynamic spring k=1 rest=5 d=10: kernel |F| = {got}, Hooke |F| = {want}");
    assert!(
        (got - want).abs() <= 1e-4 * want,
        "INTENDED-LAW DEPARTURE: dynamic spring |F| = {got}, Hooke's law gives {want} \
         (extra 1/d factor, semantic_forces.cu:561-563)"
    );
}

/// EXPECTED FAILURE. `force_type 3` is documented as repulsion ("Negative
/// strength creates repulsion. Only apply when distance is less than
/// rest_length"; the registry ships `strength: -0.3 // Repulsive`). Inside
/// the rest length `d - rest < 0`, and a negative strength flips it back
/// positive, so the source is pulled TOWARD the target. Case: k = -0.3,
/// rest 150, d = 50: intended a push apart of |k| (rest - d) = 30; the kernel
/// gives an attraction of 0.6.
#[test]
#[ignore = "needs a CUDA device; see README.md"]
#[should_panic(expected = "INTENDED-LAW DEPARTURE")]
fn intended_law_dynamic_repulsion_type_pushes_apart() {
    let g = require_gpu("intended_law_dynamic_repulsion_type_pushes_apart");
    let fx = two_nodes([0.0, 0.0, 0.0], [30.0, 40.0, 0.0], 1.0, 0);
    let rel = Relationship {
        strength: -0.3,
        rest_length: 150.0,
        directional: false,
        force_type: 3,
    };
    let f = dynamic_only(&fx, &g, rel);
    // Component of the source's force along source -> target.
    let along = (f[0][0] * 30.0 + f[0][1] * 40.0) / 50.0;
    println!(
        "repulsion type k=-0.3 rest=150 d=50: kernel force on source along source->target = \
         {along} (positive = attraction); intended -30"
    );
    assert!(
        (along + 30.0).abs() <= 1e-4 * 30.0,
        "INTENDED-LAW DEPARTURE: force_type 3 gives {along} along source->target (attraction); \
         the stated law is a repulsion of 30"
    );
}

/// EXPECTED FAILURE. `apply_attribute_spring_force` documents
/// `rest_length_min` / `rest_length_max` as the minimum and maximum rest
/// length, but `rest = max - w (max - min)` is unclamped, so a weight above
/// 1 drives it below the minimum (negative here). Case: min 10, max 50,
/// w = 2, d = 20, k = 1: rest becomes -30 and |F| = 50; with the rest length
/// held to its documented bound (10) |F| = 10.
#[test]
#[ignore = "needs a CUDA device; see README.md"]
#[should_panic(expected = "INTENDED-LAW DEPARTURE")]
fn intended_law_attribute_spring_rest_length_stays_in_bounds() {
    let g = require_gpu("intended_law_attribute_spring_rest_length_stays_in_bounds");
    let fx = two_nodes([0.0, 0.0, 0.0], [12.0, 16.0, 0.0], 2.0, 0);
    let cfg = SemanticConfig {
        attribute_spring: [1.0, 0.0, 10.0, 50.0],
        attribute_spring_enabled: true,
        ..SemanticConfig::default()
    };
    let f = g.semantic(
        &fx,
        &cfg,
        &[],
        false,
        &centroids_for(&fx),
        SemanticKernel::AttributeSpring,
    );
    let got = (f[0][0].powi(2) + f[0][1].powi(2)).sqrt();
    println!("attribute spring w=2 min=10 max=50 d=20: kernel |F| = {got}, in-bounds law 10");
    assert!(
        (got - 10.0).abs() <= 1e-4 * 10.0,
        "INTENDED-LAW DEPARTURE: attribute spring |F| = {got}; with rest held to [min, max] it \
         is 10 (rest = max - w(max - min) = -30 for w = 2)"
    );
}

fn stability_vs_main(name: &str, setup: Setup) {
    let g = require_gpu(name);
    let fx = fixtures::mixed256();
    let (eval, p, i) = setup(&fx, &g);
    let main = g.force_pass(&eval, &p, &i, ForceKernel::Main);
    let stab = g.force_pass(&eval, &p, &i, ForceKernel::Stability);
    let r = reference::force_pass(&eval, &p, &i, ForceKernel::Main);
    let (mut worst, mut at) = (0f32, 0usize);
    for (k, (a, b)) in main.iter().zip(&stab).enumerate() {
        let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        if d > worst {
            worst = d;
            at = k;
        }
    }
    println!(
        "{name}: largest per-node difference {worst} at node {at} (main {:?}, stability {:?})",
        main[at], stab[at]
    );
    let tol = r[at].tolerance() as f32 * 2.0;
    assert!(
        worst <= tol.max(1e-6),
        "INTENDED-LAW DEPARTURE: force_pass_with_stability_kernel differs from force_pass_kernel \
         by {worst} at node {at}; it is documented as mirroring it"
    );
}

/// EXPECTED FAILURE. `force_pass_with_stability_kernel` is documented as
/// mirroring `force_pass_kernel` and the server switches to it whenever
/// `stability_threshold > 0`, but it has no `gravity` term: the settings'
/// centre pull silently disappears on that path. Case: `gravity = 0.02`
/// alone on `mixed256`: the main kernel pulls each node by `0.02 * |p|`
/// (up to about 2 on this fixture); the stability kernel returns 0.
#[test]
#[ignore = "needs a CUDA device; see README.md"]
#[should_panic(expected = "INTENDED-LAW DEPARTURE")]
fn intended_law_stability_kernel_applies_gravity() {
    stability_vs_main("intended_law_stability_kernel_applies_gravity", |fx, _| {
        let p = SimParams {
            gravity: 0.02,
            ..base()
        };
        (fx.clone(), p, ForceInputs::default())
    });
}

/// EXPECTED FAILURE. The same mirror claim, for class-aware repulsion: the
/// stability kernel takes no class buffers, so same-domain pairs repel at
/// full strength instead of 0.1x and cross-domain pairs at 1x instead of 3x.
#[test]
#[ignore = "needs a CUDA device; see README.md"]
#[should_panic(expected = "INTENDED-LAW DEPARTURE")]
fn intended_law_stability_kernel_applies_class_repulsion() {
    stability_vs_main(
        "intended_law_stability_kernel_applies_class_repulsion",
        |fx, _| {
            let p = SimParams {
                feature_flags: flags::REPULSION,
                ..base()
            };
            let i = ForceInputs {
                class_id: Some(fx.class_id.clone()),
                class_charge: Some(fx.class_charge.clone()),
                ..ForceInputs::default()
            };
            (fx.clone(), p, i)
        },
    );
}
