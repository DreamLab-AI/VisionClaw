//! Plain-Rust CPU references for each force law **as the kernels implement
//! it today**, transcribed from the `.cu` sources (line references are to
//! `visionclaw_unified.cu` and `semantic_forces.cu` at the baseline commit).
//!
//! # Arithmetic
//!
//! Inputs are the same `f32` values the device receives. Forces are
//! accumulated in `f64`, so the reference is the exact law up to `f64`
//! rounding. Branch predicates (cutoff, the `1e-6` floors, the `max_force`
//! caps, the semantic radii) are evaluated on the reference value and every
//! one passes through [`clear`], which **panics if the value lies within a
//! relative 1e-5 of its threshold**. A fixture therefore cannot place a pair
//! where `f32` rounding or `--use_fast_math` could send the device down the
//! other branch; the deliberate degenerate cases (coincident nodes, distance
//! 1/2048) sit orders of magnitude away from every threshold.
//!
//! # Tolerance
//!
//! The comparison bound per node and component is
//!
//! ```text
//! |gpu - ref| <= EPS_F32 * (16 + terms) * scale
//! ```
//!
//! where `scale` sums, over every term the kernel added for that node, the
//! term's coefficient times the absolute values of the operands of its
//! subtraction (for `k (d - rest)` that is `k (d + rest)`; for a term with
//! no subtraction, its magnitude), and `terms` is their count. Operands
//! rather than the difference, because a term like `target - dist` loses
//! accuracy relative to `dist` when the two nearly cancel, and the kernel
//! computes `dist` with an approximate square root. Justification: the kernels are
//! compiled with `--use_fast_math`, so each term carries approximate square
//! root, reciprocal and division (`sqrt.approx`, `div.approx`, about 2 ulp
//! each), a `log1pf` (about 2 ulp) and a handful of multiplies and FMAs: 16
//! ulp of the term's own magnitude bounds that. Summing `terms` values in
//! `f32` adds at most `(terms - 1) * EPS_F32 * scale` (the standard
//! recursive-summation bound). The bound is relative to the sum of term
//! magnitudes, not to the net force, because a node whose terms cancel has a
//! tiny net force carrying the absolute error of its large terms. A node with
//! no terms must come back exactly zero.

use crate::fixtures::Fixture;
use crate::harness::{
    flags, kind, ForceInputs, ForceKernel, Relationship, SemanticConfig, SimParams,
};

/// `f32` machine epsilon.
pub const EPS_F32: f64 = f32::EPSILON as f64;

/// Per-node reference: the force, the summed term magnitudes and the count.
#[derive(Clone, Copy, Debug, Default)]
pub struct RefForce {
    pub f: [f64; 3],
    pub scale: f64,
    pub terms: usize,
}

impl RefForce {
    fn add(&mut self, v: [f64; 3], magnitude: f64) {
        for (a, b) in self.f.iter_mut().zip(v) {
            *a += b;
        }
        self.scale += magnitude.abs();
        self.terms += 1;
    }
    fn add_scaled(&mut self, dir: [f64; 3], s: f64, magnitude: f64) {
        self.add([dir[0] * s, dir[1] * s, dir[2] * s], magnitude);
    }
    /// The comparison bound for this node.
    pub fn tolerance(&self) -> f64 {
        EPS_F32 * (16 + self.terms) as f64 * self.scale
    }
}

/// Assert a predicate value is not within a relative 1e-5 of its threshold.
fn clear(what: &str, value: f64, threshold: f64) {
    let gap = (value - threshold).abs();
    assert!(
        gap > 1e-5 * threshold.abs().max(1e-30),
        "fixture places {what} = {value} within 1e-5 (relative) of its threshold {threshold}; \
         the device could branch either way. Move the fixture."
    );
}

fn p64(p: [f32; 3]) -> [f64; 3] {
    [p[0] as f64, p[1] as f64, p[2] as f64]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

// ---------------------------------------------------------------------------
// Unified force pass (force_pass_kernel / force_pass_with_stability_kernel)
// ---------------------------------------------------------------------------

/// Reference for one force pass.
///
/// `which == Stability` reproduces `force_pass_with_stability_kernel`
/// (lines 2228-2555) as written: no `gravity` term and no class-based
/// repulsion modulation, everything else as the main kernel (lines 460-867).
pub fn force_pass(
    fx: &Fixture,
    p: &SimParams,
    inp: &ForceInputs,
    which: ForceKernel,
) -> Vec<RefForce> {
    let n = fx.n();
    let csr = fx.csr();
    let main = which == ForceKernel::Main;
    let mut out = vec![RefForce::default(); n];
    for (i, acc) in out.iter_mut().enumerate() {
        let me = p64(fx.pos[i]);

        // Repulsion, lines 518-616 (main) / 2306-2373 (stability). The grid is
        // built with cell >= cutoff, so the 3x3x3 stencil sees every pair
        // inside the cutoff; the reference walks all pairs.
        if p.feature_flags & flags::REPULSION != 0 {
            for j in 0..n {
                if j == i {
                    continue;
                }
                let diff = sub(me, p64(fx.pos[j]));
                let dsq = diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2];
                let cut2 = (p.repulsion_cutoff * p.repulsion_cutoff) as f64;
                clear("dist_sq vs cutoff^2", dsq, cut2);
                if dsq >= 1e-12 {
                    clear("dist_sq vs 1e-6", dsq, 1e-6);
                }
                if !(dsq < cut2 && dsq > 1e-6) {
                    continue;
                }
                let d = dsq.sqrt();
                let sep = p.separation_radius as f64;
                let max_f = p.max_force as f64;
                if sep > 0.0 {
                    clear("dist vs separation_radius", d, sep);
                    if d < sep {
                        let pen = (sep - d) / sep;
                        let sf = (max_f * pen).min(max_f);
                        acc.add_scaled(diff, sf / d, max_f * (sep + d) / sep);
                    }
                }
                let mut rep;
                if let (true, Some(deg)) = (p.lin_log_mode != 0, &inp.node_degrees) {
                    let di = deg[i] as f64 + 1.0;
                    let dj = deg[j] as f64 + 1.0;
                    rep = p.scaling_ratio as f64 * di * dj
                        / (d + p.repulsion_softening_epsilon as f64);
                } else {
                    rep = p.repel_k as f64 / (dsq + p.repulsion_softening_epsilon as f64);
                    if main {
                        match &inp.class_id {
                            Some(cid) if cid[i] != 0 && cid[j] != 0 => {
                                rep *= if cid[i] == cid[j] { 0.1 } else { 3.0 };
                            }
                            _ => {
                                let ch = |k: usize| {
                                    inp.class_charge.as_ref().map_or(1.0, |c| c[k] as f64)
                                };
                                rep *= ch(i) * ch(j);
                            }
                        }
                    }
                }
                clear("repulsion vs max_force", rep, max_f);
                rep = rep.min(max_f);
                acc.add_scaled(diff, rep / d, rep);
            }
        }

        // Springs, lines 618-652 and edge_spring_scale (lines 422-458).
        if p.feature_flags & flags::SPRINGS != 0 {
            let use_sssp = inp.sssp.is_some() && p.feature_flags & flags::SSSP_SPRING_ADJUST != 0;
            let s = inp.spring_scale.as_ref().map_or(1.0, |v| v[i] as f64);
            for slot in csr.offsets[i] as usize..csr.offsets[i + 1] as usize {
                let j = csr.cols[slot] as usize;
                let w = csr.weights[slot] as f64;
                let diff = sub(p64(fx.pos[j]), me);
                let d = norm(diff);
                if d > 1e-9 {
                    clear("spring dist vs 1e-6", d, 1e-6);
                }
                if d <= 1e-6 {
                    continue;
                }
                let mut mag;
                if p.lin_log_mode != 0 {
                    mag = d.ln_1p() * w * s;
                } else {
                    let mut ideal = p.rest_length as f64;
                    if let (true, Some(sd)) = (use_sssp, &inp.sssp) {
                        let (du, dv) = (sd[i] as f64, sd[j] as f64);
                        if du.is_finite() && dv.is_finite() {
                            let nd = (du - dv).abs().min(p.norm_delta_cap as f64);
                            ideal = p.sssp_alpha as f64 * nd + p.rest_length as f64;
                        }
                    }
                    mag = p.spring_k as f64 * (d - ideal) * w * s;
                }
                let k_ws = (p.spring_k as f64 * w * s).abs();
                let mut magnitude = if p.lin_log_mode != 0 {
                    mag.abs()
                } else {
                    k_ws * (d
                        + p.rest_length as f64
                        + p.sssp_alpha as f64 * p.norm_delta_cap as f64)
                };
                if let Some(rest) = &inp.edge_rest {
                    let overlay = p.spring_k as f64 * (d - rest[slot] as f64) * w * s;
                    mag += overlay;
                    magnitude += k_ws * (d + rest[slot] as f64);
                }
                acc.add_scaled(diff, mag / d, magnitude);
            }
        }

        // Centring, lines 654-656, and gravity, lines 659-663 (main only).
        if p.feature_flags & flags::CENTERING != 0 {
            let k = p.center_gravity_k as f64;
            acc.add_scaled(me, -k, k * norm(me));
        }
        if main && p.gravity != 0.0 {
            let g = p.gravity as f64;
            acc.add_scaled(me, -g, g * norm(me));
        }

        // Constraints, lines 667-843 / 2417-2529.
        if p.feature_flags & flags::CONSTRAINTS != 0 {
            constraints(fx, p, inp, i, acc);
        }

        // DAG radial bias (lines 231-260), stratified planes (269-283),
        // Sugiyama layers (291-304).
        if let Some(rank) = &inp.node_rank {
            let k = p.dag_bias_k as f64;
            if k > 0.0 && rank[i] >= 0.0 {
                let c = [
                    p.radial_center_x as f64,
                    p.radial_center_y as f64,
                    p.radial_center_z as f64,
                ];
                let delta = sub(me, c);
                let target = rank[i] as f64 * p.dag_level_distance as f64;
                let dist = norm(delta);
                if dist > 1e-9 {
                    clear("radial dist vs 1e-6", dist, 1e-6);
                }
                if dist > 1e-6 {
                    let err = target - dist;
                    acc.add_scaled(delta, err * k / dist, k * (target + dist));
                } else if target > 0.0 {
                    acc.add([target * k, 0.0, 0.0], target * k);
                }
            }
            let lk = p.layer_bias_k as f64;
            if lk > 0.0 && p.layer_spacing > 0.0 && rank[i] >= 0.0 {
                let target = rank[i] as f64 * p.layer_spacing as f64;
                let fy = (target - me[1]) * lk;
                acc.add([0.0, fy, 0.0], lk * (target.abs() + me[1].abs()));
            }
        }
        if let Some(plane) = &inp.node_plane {
            let k = p.plane_bias_k as f64;
            if k > 0.0 && p.plane_spacing > 0.0 && plane[i].is_finite() {
                let target = plane[i] as f64 * p.plane_spacing as f64;
                let fz = (target - me[2]) * k;
                acc.add([0.0, 0.0, fz], k * (target.abs() + me[2].abs()));
            }
        }
    }
    out
}

fn ramp(p: &SimParams, c: &crate::harness::ConstraintData) -> f64 {
    if p.constraint_ramp_frames > 0 {
        let since = p.iteration - c.activation_frame;
        if since >= 0 && (since as u32) < p.constraint_ramp_frames {
            return (since as f64 / p.constraint_ramp_frames as f64).min(1.0);
        }
    }
    1.0
}

fn constraints(fx: &Fixture, p: &SimParams, inp: &ForceInputs, i: usize, acc: &mut RefForce) {
    let n = fx.n();
    let me = p64(fx.pos[i]);
    let maxc = p.constraint_max_force_per_node as f64;
    for c in &inp.constraints {
        let count = c.count.clamp(0, 4) as usize;
        let Some(role) = c.node_idx[..count].iter().position(|&x| x == i as i32) else {
            continue;
        };
        let other = if role == 0 {
            c.node_idx[1]
        } else {
            c.node_idx[0]
        };
        let w = c.weight as f64 * ramp(p, c);
        let mut f = [0.0; 3];
        let mut operands = None;
        match c.kind {
            kind::DISTANCE if c.count >= 2 && other >= 0 && (other as usize) < n => {
                let diff = sub(me, p64(fx.pos[other as usize]));
                let cur = norm(diff);
                let target = c.params[0] as f64;
                if cur > 1e-6 && target > 0.0 {
                    let fm = (-w * (cur - target)).clamp(-maxc, maxc);
                    f = diff.map(|v| v * fm / cur);
                    operands = Some(w.abs() * (cur + target));
                }
            }
            kind::POSITION if c.count >= 1 => {
                let t = [c.params[0] as f64, c.params[1] as f64, c.params[2] as f64];
                let diff = sub(t, me);
                let dist = norm(diff);
                if dist > 1e-6 {
                    let fm = (w * dist * p.position_constraint_attraction as f64).min(maxc);
                    f = diff.map(|v| v * fm / dist);
                }
            }
            kind::SEPARATION if c.count >= 2 && other >= 0 && (other as usize) < n => {
                let diff = sub(me, p64(fx.pos[other as usize]));
                let cur = norm(diff);
                let min_d = c.params[0] as f64;
                clear("separation constraint dist", cur, min_d);
                if cur > 1e-6 && cur < min_d {
                    let fm = (w * (min_d - cur)).min(maxc);
                    f = diff.map(|v| v * fm / cur);
                    operands = Some(w.abs() * (cur + min_d));
                }
            }
            kind::ALIGNMENT if c.count >= 2 && other >= 0 && (other as usize) < n => {
                let axis = c.params[0] as i32;
                let a = |q: [f64; 3]| match axis {
                    0 => q[0],
                    1 => q[1],
                    _ => q[2],
                };
                let err = (a(p64(fx.pos[other as usize])) - a(me)) * 0.5;
                let fm = (err * w * p.alignment_strength as f64).clamp(-maxc, maxc);
                f[match axis {
                    0 => 0,
                    1 => 1,
                    _ => 2,
                }] = fm;
            }
            _ => continue,
        }
        acc.add(f, operands.unwrap_or_else(|| norm(f)));
    }
}

/// `snapshot_edge_rest_kernel` (lines 955-972): per-CSR-slot edge lengths.
pub fn edge_rest(fx: &Fixture) -> Vec<RefForce> {
    let csr = fx.csr();
    let mut out = Vec::with_capacity(csr.cols.len());
    for i in 0..fx.n() {
        for slot in csr.offsets[i] as usize..csr.offsets[i + 1] as usize {
            let d = norm(sub(p64(fx.pos[csr.cols[slot] as usize]), p64(fx.pos[i])));
            let mut r = RefForce::default();
            r.add([d, 0.0, 0.0], d);
            out.push(r);
        }
    }
    out
}

/// `degree_weighted_gravity_kernel` (lines 2701-2750) on top of `base`.
pub fn degree_weighted_gravity(
    fx: &Fixture,
    base: &[[f32; 3]],
    dw: &[f32],
    cgk: f32,
    peripheral_radius: f32,
    isolated_k: f32,
) -> Vec<RefForce> {
    (0..fx.n())
        .map(|i| {
            let mut r = RefForce::default();
            let b = p64(base[i]);
            r.add(b, norm(b));
            let me = p64(fx.pos[i]);
            let k = cgk as f64;
            if (dw[i] as f64) < 1e-6 {
                r.add_scaled(me, k, k * norm(me));
                let dist = norm(me);
                if dist > 1e-4 {
                    let shell = -(isolated_k as f64) * (dist - peripheral_radius as f64);
                    let operands = isolated_k as f64 * (dist + peripheral_radius as f64);
                    r.add_scaled(me, shell / dist, operands);
                }
            } else {
                let corr = k * (1.0 - dw[i] as f64);
                r.add_scaled(me, corr, corr * norm(me));
            }
            r
        })
        .collect()
}

/// `cluster_cohesion_kernel` (lines 915-945) from zero forces.
pub fn cluster_cohesion(
    fx: &Fixture,
    centroids: &[[f32; 3]],
    assignment: &[i32],
    strength: f32,
) -> Vec<RefForce> {
    (0..fx.n())
        .map(|i| {
            let mut r = RefForce::default();
            let c = assignment[i];
            if c >= 0 && (c as usize) < centroids.len() {
                let d = sub(p64(centroids[c as usize]), p64(fx.pos[i]));
                let s = strength as f64;
                r.add_scaled(d, s, s * norm(d));
            }
            r
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Semantic module
// ---------------------------------------------------------------------------

/// Which semantic law to evaluate; `Production` has no CPU reference (see
/// the README: its centroids are built with float atomics).
pub use crate::harness::SemanticKernel;

/// Unit vector or zero, as `normalize` (line 191) with its 1e-6 floor.
fn unit(v: [f64; 3]) -> [f64; 3] {
    let l = norm(v);
    if l > 1e-6 {
        v.map(|x| x / l)
    } else {
        [0.0; 3]
    }
}

/// Reference for one semantic kernel run in isolation from zero forces, with
/// the given centroid tables (slot-indexed by the family value).
pub fn semantic(
    fx: &Fixture,
    cfg: &SemanticConfig,
    table: &[Relationship],
    dynamic_enabled: bool,
    centroids: &crate::harness::Centroids,
    which: SemanticKernel,
) -> Vec<RefForce> {
    let n = fx.n();
    let mut out = vec![RefForce::default(); n];
    let pos: Vec<[f64; 3]> = fx.pos.iter().map(|p| p64(*p)).collect();
    match which {
        SemanticKernel::Dag if cfg.dag_enabled => {
            let [vs, hs, la, sr] = cfg.dag.map(|v| v as f64);
            for i in 0..n {
                let level = fx.hierarchy_level[i];
                if level < 0 {
                    continue;
                }
                let target = level as f64 * vs;
                let dy = target - pos[i][1];
                out[i].add([0.0, dy * la, 0.0], la * (target.abs() + pos[i][1].abs()));
                for j in 0..n {
                    if j == i || fx.hierarchy_level[j] != level {
                        continue;
                    }
                    let mut delta = sub(pos[i], pos[j]);
                    delta[1] = 0.0;
                    let d = norm(delta);
                    clear("dag sibling dist vs horizontal_spacing", d, hs);
                    if d < hs && d > 1e-6 {
                        let fm = sr * (hs - d) / d;
                        out[i].add_scaled(delta, fm, sr * (hs + d));
                    }
                }
            }
        }
        SemanticKernel::TypeCluster if cfg.type_cluster_enabled => {
            let [ca, radius, icr] = cfg.type_cluster.map(|v| v as f64);
            cluster(
                &pos,
                &fx.node_type,
                &centroids.types,
                (0, 15),
                false,
                [ca, radius, icr],
                &mut out,
            );
        }
        SemanticKernel::Physicality if cfg.physicality_enabled => {
            let c = cfg.physicality.map(|v| v as f64);
            cluster(
                &pos,
                &fx.physicality,
                &centroids.physicality,
                (1, 3),
                true,
                c,
                &mut out,
            );
        }
        SemanticKernel::Role if cfg.role_enabled => {
            let c = cfg.role.map(|v| v as f64);
            cluster(&pos, &fx.role, &centroids.role, (1, 4), true, c, &mut out);
        }
        SemanticKernel::Collision if cfg.collision_enabled => {
            let [md, cs, r] = cfg.collision.map(|v| v as f64);
            let min_d = r + r + md;
            for i in 0..n {
                for j in 0..n {
                    if j == i {
                        continue;
                    }
                    let delta = sub(pos[i], pos[j]);
                    let d = norm(delta);
                    clear("collision dist vs min distance", d, min_d);
                    if d < min_d && d > 1e-6 {
                        let fm = cs * (min_d - d);
                        out[i].add_scaled(unit(delta), fm, cs * (min_d + d));
                    }
                }
            }
        }
        SemanticKernel::Maturity if cfg.maturity_enabled => {
            let [_, la, ss] = cfg.maturity.map(|v| v as f64);
            for i in 0..n {
                let target = match fx.maturity[i] {
                    1 => -ss,
                    2 => 0.0,
                    3 => ss,
                    _ => continue,
                };
                let fz = (target - pos[i][2]) * la;
                out[i].add([0.0, 0.0, fz], la * (target.abs() + pos[i][2].abs()));
            }
        }
        SemanticKernel::AttributeSpring if cfg.attribute_spring_enabled => {
            let [bk, wm, rmin, rmax] = cfg.attribute_spring.map(|v| v as f64);
            for e in &fx.edges {
                let (s, t) = (e.src as usize, e.dst as usize);
                let delta = sub(pos[t], pos[s]);
                let d = norm(delta);
                if d < 1e-6 {
                    continue;
                }
                let w = e.weight as f64;
                let k = bk * (1.0 + w * wm);
                let rest = rmax - w * (rmax - rmin);
                let fm = k * (d - rest);
                let operands = k.abs() * (d + rest.abs() + rmax.abs() + (w * (rmax - rmin)).abs());
                let u = unit(delta);
                out[s].add_scaled(u, fm, operands);
                out[t].add_scaled(u, -fm, operands);
            }
        }
        SemanticKernel::Dynamic if dynamic_enabled => {
            for e in &fx.edges {
                let Some(r) = usize::try_from(e.kind).ok().and_then(|k| table.get(k)) else {
                    continue;
                };
                if (r.strength.abs() as f64) < 1e-6 {
                    continue;
                }
                let (s, t) = (e.src as usize, e.dst as usize);
                let delta = sub(pos[t], pos[s]);
                let d = norm(delta);
                if d < 1e-6 {
                    continue;
                }
                let mut strength = r.strength as f64;
                let rest = r.rest_length as f64;
                match r.force_type {
                    2 => {
                        let avg = (fx.cross_domain[s] + fx.cross_domain[t]) as f64 * 0.5;
                        let boost = (1.0 + avg * cfg.cross_domain[1] as f64)
                            .min(cfg.cross_domain[2] as f64);
                        strength *= boost;
                    }
                    3 => {
                        clear("dynamic repulsion dist vs rest_length", d, rest);
                        if d >= rest {
                            continue;
                        }
                    }
                    _ => {}
                }
                // Lines 561-563 as written: divided by distance, then normalised.
                let fm = strength * (d - rest) / d;
                let operands = strength.abs() * (d + rest) / d;
                let u = unit(delta);
                out[s].add_scaled(u, fm, operands);
                if !r.directional {
                    out[t].add_scaled(u, -fm, operands);
                }
            }
        }
        _ => {}
    }
    out
}

/// Shared law of the type, physicality and role cluster kernels: attraction
/// to the family centroid beyond `radius`, inverse-square repulsion from
/// nodes of a different family inside `2 * radius`. `skip_zero` excludes
/// family 0 neighbours (physicality and role do; type does not).
fn cluster(
    pos: &[[f64; 3]],
    family: &[i32],
    centroids: &[[f32; 3]],
    valid: (i32, i32),
    skip_zero: bool,
    [attraction, radius, repulsion]: [f64; 3],
    out: &mut [RefForce],
) {
    for i in 0..pos.len() {
        let t = family[i];
        if t < valid.0 || t > valid.1 {
            continue;
        }
        let c = centroids.get(t as usize).copied().unwrap_or([0.0; 3]);
        let to_c = sub(p64(c), pos[i]);
        let dc = norm(to_c);
        clear("distance to centroid vs cluster_radius", dc, radius);
        if dc > radius {
            let fm = attraction * (dc - radius);
            out[i].add_scaled(unit(to_c), fm, attraction * (dc + radius));
        }
        for j in 0..pos.len() {
            if j == i || family[j] == t || (skip_zero && family[j] == 0) {
                continue;
            }
            let delta = sub(pos[i], pos[j]);
            let d = norm(delta);
            clear("cluster repulsion dist vs 2 * radius", d, 2.0 * radius);
            if d < 2.0 * radius && d > 1e-6 {
                let fm = repulsion / (d * d);
                out[i].add_scaled(unit(delta), fm, fm);
            }
        }
    }
}

/// Mean position per family slot, rounded to `f32` (what an isolated cluster
/// kernel is handed as its centroid table).
pub fn centroids(fx: &Fixture, family: &[i32], slots: usize) -> Vec<[f32; 3]> {
    let mut sum = vec![[0f64; 3]; slots];
    let mut count = vec![0usize; slots];
    for (i, &f) in family.iter().enumerate() {
        if f >= 0 && (f as usize) < slots {
            for (acc, v) in sum[f as usize].iter_mut().zip(fx.pos[i]) {
                *acc += v as f64;
            }
            count[f as usize] += 1;
        }
    }
    (0..slots)
        .map(|s| {
            if count[s] == 0 {
                [0.0; 3]
            } else {
                sum[s].map(|v| (v / count[s] as f64) as f32)
            }
        })
        .collect()
}

/// Compare device forces with a reference under the documented bound.
/// Returns the worst `|delta| / tolerance` ratio; panics past 1.
pub fn assert_matches(label: &str, gpu: &[[f32; 3]], reference: &[RefForce]) -> f64 {
    assert_eq!(gpu.len(), reference.len(), "{label}: node count");
    let mut worst = (0.0f64, 0usize, 0usize);
    for (i, (g, r)) in gpu.iter().zip(reference).enumerate() {
        let tol = r.tolerance();
        for (a, (&ga, &ra)) in g.iter().zip(&r.f).enumerate() {
            let delta = (ga as f64 - ra).abs();
            assert!(
                ga.is_finite(),
                "{label}: node {i} axis {a}: device produced {ga} (reference {ra})"
            );
            if tol == 0.0 {
                assert!(
                    ga == 0.0,
                    "{label}: node {i} axis {a}: no terms, so the force must be exactly 0; got {ga}"
                );
                continue;
            }
            let ratio = delta / tol;
            if ratio > worst.0 {
                worst = (ratio, i, a);
            }
        }
    }
    let (ratio, i, a) = worst;
    assert!(
        ratio <= 1.0,
        "{label}: node {i} axis {a}: device {} vs reference {} (|Δ| = {:.3e}, bound {:.3e}, \
         {} terms, scale {:.3e})",
        gpu[i][a],
        reference[i].f[a],
        (gpu[i][a] as f64 - reference[i].f[a]).abs(),
        reference[i].tolerance(),
        reference[i].terms,
        reference[i].scale
    );
    ratio
}
