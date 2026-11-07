//! Learned arms: the relation-typed field (RF), the untyped distance embedding (B1) and
//! TransE-3D (B2).
//!
//! RF scores a directed triple `(i, r, j)` as
//!
//! ```text
//! s_r(i, j) = b_r − κ_r (d_ij − L_r)² − μ_r (Δz_ij − h_r)²,   Δz_ij = z_j − z_i
//! ```
//!
//! and maximises the logistic likelihood of training triples against corrupted ones. The
//! negative gradient of the edge term with respect to a position is a spring force toward rest
//! length `L_r` with stiffness `κ_r`, plus an axial force toward offset `h_r`: the learned model
//! *is* a typed spring network, which is what lets `sim` run its parameters as physics.
//!
//! B1 is the same trainer with types collapsed, `L` pinned to 0 and the axial term off, which is
//! the standard LINE-style `σ(b − κ d²)` distance embedding.

use crate::corpus::Triple;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

pub type Pos = Vec<[f32; 3]>;

/// Per-relation parameters in their unconstrained form.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RelParams {
    pub k_raw: f32, // κ = softplus(k_raw)
    pub l_log: f32, // L = exp(l_log)
    pub h: f32,
    pub m_raw: f32, // μ = softplus(m_raw)
    pub b: f32,
}

impl RelParams {
    pub fn kappa(&self) -> f32 { softplus(self.k_raw) }
    pub fn rest(&self) -> f32 { self.l_log.exp() }
    pub fn mu(&self) -> f32 { softplus(self.m_raw) }
}

fn softplus(x: f32) -> f32 { if x > 20.0 { x } else { x.exp().ln_1p() } }
fn sigmoid(x: f32) -> f32 { 1.0 / (1.0 + (-x).exp()) }

#[derive(Clone, Copy, Debug)]
pub struct FieldCfg {
    pub typed: bool,
    pub learn_rest: bool,
    pub axial: bool,
    pub epochs: usize,
    pub negatives: usize,
    pub lr_pos: f32,
    pub lr_par: f32,
}

/// Fixed in advance and shared by every seed (PREREGISTRATION.md: no tuning on validation).
pub const RF: FieldCfg = FieldCfg { typed: true, learn_rest: true, axial: true, epochs: 60, negatives: 5, lr_pos: 0.05, lr_par: 0.01 };
pub const B1: FieldCfg = FieldCfg { typed: false, learn_rest: false, axial: false, ..RF };

pub struct Field {
    pub pos: Pos,
    pub rels: Vec<RelParams>,
    pub best_epoch: usize,
}

fn init_pos(n: usize, rng: &mut StdRng, scale: f32) -> Pos {
    (0..n)
        .map(|_| {
            // Box–Muller for a standard normal per axis.
            let mut p = [0f32; 3];
            for v in &mut p {
                let (u1, u2): (f32, f32) = (rng.gen_range(1e-7..1.0), rng.gen());
                *v = scale * (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos();
            }
            p
        })
        .collect()
}

fn corrupt(t: &Triple, n: u32, rng: &mut StdRng) -> Triple {
    let mut c = *t;
    loop {
        let x = rng.gen_range(0..n);
        if rng.gen_bool(0.5) { c.tail = x } else { c.head = x }
        if c.head != c.tail { return c; }
    }
}

struct Eval { s: f32, d: f32, dz: f32, diff: [f32; 3] }

fn score(cfg: &FieldCfg, p: &RelParams, xi: &[f32; 3], xj: &[f32; 3]) -> Eval {
    let diff = [xi[0] - xj[0], xi[1] - xj[1], xi[2] - xj[2]];
    let d = (diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2]).sqrt().max(1e-6);
    let dz = xj[2] - xi[2];
    let l = if cfg.learn_rest { p.rest() } else { 0.0 };
    let mut s = p.b - p.kappa() * (d - l) * (d - l);
    if cfg.axial {
        s -= p.mu() * (dz - p.h) * (dz - p.h);
    }
    Eval { s, d, dz, diff }
}

fn rel_of(cfg: &FieldCfg, t: &Triple) -> usize { if cfg.typed { t.rel as usize } else { 0 } }

/// Mean logistic loss over `ts` against seeded corruptions; used only for early stopping.
fn loss(cfg: &FieldCfg, pos: &Pos, rels: &[RelParams], ts: &[Triple], seed: u64) -> f64 {
    let mut rng = StdRng::seed_from_u64(seed);
    let n = pos.len() as u32;
    let mut total = 0f64;
    for t in ts {
        let p = &rels[rel_of(cfg, t)];
        let e = score(cfg, p, &pos[t.head as usize], &pos[t.tail as usize]);
        total -= (sigmoid(e.s).max(1e-7) as f64).ln();
        for _ in 0..cfg.negatives {
            let c = corrupt(t, n, &mut rng);
            let e = score(cfg, p, &pos[c.head as usize], &pos[c.tail as usize]);
            total -= ((1.0 - sigmoid(e.s)).max(1e-7) as f64).ln();
        }
    }
    total / ts.len().max(1) as f64
}

/// Train RF (or B1 with `B1`). Returns the positions and parameters at the best validation epoch.
pub fn train_field(cfg: FieldCfg, n: usize, n_rel: usize, train: &[Triple], val: &[Triple], seed: u64) -> Field {
    train_field_from(cfg, None, n, n_rel, train, val, seed)
}

/// As `train_field`, optionally starting from given positions instead of a random draw.
pub fn train_field_from(cfg: FieldCfg, init: Option<Pos>, n: usize, n_rel: usize, train: &[Triple], val: &[Triple], seed: u64) -> Field {
    let mut rng = StdRng::seed_from_u64(seed.wrapping_add(101));
    let drawn = init_pos(n, &mut rng, 1.0); // always drawn, so the RNG stream is unchanged
    let mut pos = init.unwrap_or(drawn);
    let n_rel = if cfg.typed { n_rel } else { 1 };
    let mut rels = vec![RelParams { k_raw: 0.0, l_log: 0.0, h: 0.0, m_raw: -3.0, b: 3.0 }; n_rel];
    let mut order: Vec<usize> = (0..train.len()).collect();
    let mut best = (f64::INFINITY, pos.clone(), rels.clone(), 0usize);
    let clip = |v: f32| v.clamp(-5.0, 5.0);

    for epoch in 0..cfg.epochs {
        let frac = 1.0 - epoch as f32 / cfg.epochs as f32;
        let (lr_x, lr_p) = (cfg.lr_pos * frac.max(0.1), cfg.lr_par * frac.max(0.1));
        rand::seq::SliceRandom::shuffle(order.as_mut_slice(), &mut rng);
        for &k in &order {
            let t = train[k];
            let r = rel_of(&cfg, &t);
            for s_idx in 0..=cfg.negatives {
                let (c, y) = if s_idx == 0 { (t, 1.0) } else { (corrupt(&t, n as u32, &mut rng), 0.0) };
                let (i, j) = (c.head as usize, c.tail as usize);
                let p = rels[r].clone();
                let e = score(&cfg, &p, &pos[i], &pos[j]);
                let g = sigmoid(e.s) - y; // ∂loss/∂s
                let (kappa, l, mu) = (p.kappa(), if cfg.learn_rest { p.rest() } else { 0.0 }, p.mu());

                // Parameter gradients (chain rule through softplus / exp).
                let rp = &mut rels[r];
                rp.b -= lr_p * clip(g);
                rp.k_raw -= lr_p * clip(g * -(e.d - l) * (e.d - l) * sigmoid(p.k_raw));
                if cfg.learn_rest {
                    rp.l_log -= lr_p * clip(g * 2.0 * kappa * (e.d - l) * l);
                }
                if cfg.axial {
                    rp.h -= lr_p * clip(g * 2.0 * mu * (e.dz - p.h));
                    rp.m_raw -= lr_p * clip(g * -(e.dz - p.h) * (e.dz - p.h) * sigmoid(p.m_raw));
                }

                // Position gradients: ∂s/∂x_i = −2κ(d−L)·diff/d (+ axial on z).
                let radial = -2.0 * kappa * (e.d - l) / e.d;
                for a in 0..3 {
                    let mut gi = radial * e.diff[a];
                    let mut gj = -gi;
                    if cfg.axial && a == 2 {
                        let ax = 2.0 * mu * (e.dz - p.h);
                        gi += ax;
                        gj -= ax;
                    }
                    pos[i][a] -= lr_x * clip(g * gi);
                    pos[j][a] -= lr_x * clip(g * gj);
                }
            }
        }
        let v = loss(&cfg, &pos, &rels, val, seed ^ 0xa11ce);
        if v < best.0 {
            best = (v, pos.clone(), rels.clone(), epoch);
        }
    }
    Field { pos: best.1, rels: best.2, best_epoch: best.3 }
}

/// TransE (Bordes et al. 2013) in three dimensions: `‖x_h + t_r − x_t‖₂`, margin 1, entities
/// renormalised to the unit sphere after every update, as in the original method.
pub fn train_transe(n: usize, n_rel: usize, train: &[Triple], val: &[Triple], seed: u64) -> (Pos, usize) {
    const EPOCHS: usize = 60;
    const MARGIN: f32 = 1.0;
    const LR: f32 = 0.01;
    let mut rng = StdRng::seed_from_u64(seed.wrapping_add(202));
    let normalise = |p: &mut [f32; 3]| {
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1e-6);
        p.iter_mut().for_each(|v| *v /= l);
    };
    let mut ent = init_pos(n, &mut rng, 1.0);
    ent.iter_mut().for_each(normalise);
    let mut rel = init_pos(n_rel, &mut rng, 0.3);
    let dist = |e: &Pos, r: &Pos, t: &Triple| -> ([f32; 3], f32) {
        let (h, x, tl) = (&e[t.head as usize], &r[t.rel as usize], &e[t.tail as usize]);
        let v = [h[0] + x[0] - tl[0], h[1] + x[1] - tl[1], h[2] + x[2] - tl[2]];
        (v, (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6))
    };
    let val_loss = |e: &Pos, r: &Pos| -> f64 {
        let mut rng = StdRng::seed_from_u64(seed ^ 0xa11ce);
        val.iter().map(|t| {
            let c = corrupt(t, n as u32, &mut rng);
            (MARGIN + dist(e, r, t).1 - dist(e, r, &c).1).max(0.0) as f64
        }).sum::<f64>() / val.len().max(1) as f64
    };
    let mut order: Vec<usize> = (0..train.len()).collect();
    let mut best = (f64::INFINITY, ent.clone(), 0usize);
    for epoch in 0..EPOCHS {
        rand::seq::SliceRandom::shuffle(order.as_mut_slice(), &mut rng);
        for &k in &order {
            let t = train[k];
            let c = corrupt(&t, n as u32, &mut rng);
            let ((vp, dp), (vn, dn)) = (dist(&ent, &rel, &t), dist(&ent, &rel, &c));
            if MARGIN + dp - dn <= 0.0 {
                continue;
            }
            for a in 0..3 {
                let (gp, gn) = (vp[a] / dp, vn[a] / dn);
                ent[t.head as usize][a] -= LR * gp;
                ent[t.tail as usize][a] += LR * gp;
                rel[t.rel as usize][a] -= LR * (gp - gn);
                ent[c.head as usize][a] += LR * gn;
                ent[c.tail as usize][a] -= LR * gn;
            }
            for idx in [t.head, t.tail, c.head, c.tail] {
                normalise(&mut ent[idx as usize]);
            }
        }
        let v = val_loss(&ent, &rel);
        if v < best.0 {
            best = (v, ent.clone(), epoch);
        }
    }
    (best.1, best.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two relation types with different true geometry: RF should learn different rest lengths.
    #[test]
    fn learns_distinct_rest_lengths_per_type() {
        let n = 200;
        let mut ts = Vec::new();
        for i in 0..100u32 {
            ts.push(Triple { head: i, rel: 0, tail: (i + 1) % 100 }); // dense ring
            ts.push(Triple { head: i, rel: 1, tail: 100 + i });       // matched pair
        }
        let f = train_field(FieldCfg { epochs: 40, ..RF }, n, 2, &ts, &ts[..20], 7);
        assert!(f.rels.iter().all(|r| r.kappa().is_finite() && r.rest().is_finite()));
        let (l0, l1) = (f.rels[0].rest(), f.rels[1].rest());
        assert!((l0 - l1).abs() > 1e-3, "rest lengths did not separate: {l0} vs {l1}");
    }

    #[test]
    fn transe_keeps_entities_on_unit_sphere() {
        let ts: Vec<_> = (0..50u32).map(|i| Triple { head: i, rel: 0, tail: (i + 1) % 50 }).collect();
        let (pos, _) = train_transe(50, 1, &ts, &ts[..5], 1);
        for p in pos {
            let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-3);
        }
    }
}
