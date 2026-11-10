//! CPU replica of the live force form (B0) and the learned-parameter physics run (RF-sim).
//!
//! This replicates the *form* of VisionClaw's force pass, not the CUDA kernel: Hooke springs
//! along edges, inverse-square repulsion inside a cutoff found through a uniform grid, centre
//! gravity, damping and force/velocity clamps, with the values from `data/settings.yaml`. The
//! only thing that differs between B0 and RF-sim is the spring list; repulsion, gravity and
//! integration are identical, so any difference in the metrics comes from the spring law.

use crate::corpus::Triple;
use crate::embed::{Pos, RelParams};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct SimCfg {
    pub spring_k: f32,
    pub rest_length: f32,
    pub repel_k: f32,
    pub center_k: f32,
    pub damping: f32,
    pub dt: f32,
    pub max_velocity: f32,
    pub max_force: f32,
    pub repel_cutoff: f32,
    pub bounds: f32,
    pub iterations: usize,
}

/// `data/settings.yaml` physics block, plus `repulsion_cutoff` from `data/dev_config.toml`.
pub const LIVE: SimCfg = SimCfg {
    spring_k: 12.0,
    rest_length: 50.0,
    repel_k: 120.0,
    center_k: 0.2,
    damping: 0.9,
    dt: 0.016,
    max_velocity: 100.0,
    max_force: 150.0,
    repel_cutoff: 150.0,
    bounds: 400.0,
    iterations: 2500,
};

/// One spring as seen from node `other`'s neighbour: stiffness, rest length, and an optional
/// axial target `dz = z_other − z_self` with its weight.
#[derive(Clone, Copy)]
pub struct Spring {
    other: u32,
    k: f32,
    rest: f32,
    axial_w: f32,
    axial_dz: f32,
}

/// B0: one uniform spring per unordered training pair, as the live graph has one edge per pair.
pub fn uniform_springs(n: usize, train: &[Triple], cfg: &SimCfg) -> Vec<Vec<Spring>> {
    let pairs: HashSet<(u32, u32)> = train
        .iter()
        .map(|t| crate::split::pair(t.head, t.tail))
        .collect();
    let mut adj = vec![Vec::new(); n];
    let mut pairs: Vec<_> = pairs.into_iter().collect();
    pairs.sort_unstable();
    for (a, b) in pairs {
        let s = Spring {
            other: b,
            k: cfg.spring_k,
            rest: cfg.rest_length,
            axial_w: 0.0,
            axial_dz: 0.0,
        };
        adj[a as usize].push(s);
        adj[b as usize].push(Spring { other: a, ..s });
    }
    adj
}

/// How the learned parameters were mapped into world units, reported with the results.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Mapping {
    pub world_scale: f32,
    pub mean_kappa: f32,
    pub per_relation: Vec<(String, f32, f32, f32, f32)>, // name, k, rest, axial_w, axial_dz
}

/// RF-sim: one spring per training triple with its relation's learned law, mapped into world
/// units so the triple-weighted median rest length equals the live `rest_length` and the
/// triple-weighted mean stiffness equals the live `spring_k`.
pub fn learned_springs(
    n: usize,
    train: &[Triple],
    rels: &[RelParams],
    names: &[String],
    cfg: &SimCfg,
) -> (Vec<Vec<Spring>>, Mapping) {
    let mut count: HashMap<u16, usize> = HashMap::new();
    train
        .iter()
        .for_each(|t| *count.entry(t.rel).or_default() += 1);
    let mut rests: Vec<f32> = train.iter().map(|t| rels[t.rel as usize].rest()).collect();
    rests.sort_by(|a, b| a.total_cmp(b));
    let median_rest = rests[rests.len() / 2].max(1e-6);
    let mean_kappa = train
        .iter()
        .map(|t| rels[t.rel as usize].kappa())
        .sum::<f32>()
        / train.len() as f32;
    let scale = cfg.rest_length / median_rest;
    let law = |r: &RelParams| {
        // Energy κ(d−L)² in model units ⇒ Hooke stiffness ∝ κ; scaled to the live spring_k.
        let k = cfg.spring_k * r.kappa() / mean_kappa;
        let aw = cfg.spring_k * r.mu() / mean_kappa;
        (k, r.rest() * scale, aw, r.h * scale)
    };
    let mut adj = vec![Vec::new(); n];
    for t in train {
        let (k, rest, aw, dz) = law(&rels[t.rel as usize]);
        // From the head's side the axial target is z_tail − z_head = dz; from the tail's, −dz.
        adj[t.head as usize].push(Spring {
            other: t.tail,
            k,
            rest,
            axial_w: aw,
            axial_dz: dz,
        });
        adj[t.tail as usize].push(Spring {
            other: t.head,
            k,
            rest,
            axial_w: aw,
            axial_dz: -dz,
        });
    }
    let per_relation = rels
        .iter()
        .enumerate()
        .filter(|(i, _)| count.contains_key(&(*i as u16)))
        .map(|(i, r)| {
            let (k, rest, aw, dz) = law(r);
            (names[i].clone(), k, rest, aw, dz)
        })
        .collect();
    (
        adj,
        Mapping {
            world_scale: scale,
            mean_kappa,
            per_relation,
        },
    )
}

fn clamp_len(v: [f32; 3], max: f32) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > max {
        [v[0] * max / l, v[1] * max / l, v[2] * max / l]
    } else {
        v
    }
}

/// Run the force simulation from a seeded random start and return final positions.
pub fn simulate(adj: &[Vec<Spring>], cfg: &SimCfg, seed: u64) -> Pos {
    let n = adj.len();
    let mut rng = StdRng::seed_from_u64(seed.wrapping_add(303));
    let half = cfg.bounds / 2.0;
    let mut pos: Pos = (0..n)
        .map(|_| {
            [
                rng.gen_range(-half..half),
                rng.gen_range(-half..half),
                rng.gen_range(-half..half),
            ]
        })
        .collect();
    let mut vel: Pos = vec![[0.0; 3]; n];
    let cell = cfg.repel_cutoff;
    let cut2 = cfg.repel_cutoff * cfg.repel_cutoff;
    let key = |p: &[f32; 3]| {
        (
            (p[0] / cell).floor() as i32,
            (p[1] / cell).floor() as i32,
            (p[2] / cell).floor() as i32,
        )
    };

    for _ in 0..cfg.iterations {
        let mut grid: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
        for (i, p) in pos.iter().enumerate() {
            grid.entry(key(p)).or_default().push(i as u32);
        }
        let forces: Vec<[f32; 3]> = (0..n)
            .into_par_iter()
            .map(|i| {
                let xi = pos[i];
                let mut f = [
                    -cfg.center_k * xi[0],
                    -cfg.center_k * xi[1],
                    -cfg.center_k * xi[2],
                ];
                for s in &adj[i] {
                    let xj = pos[s.other as usize];
                    let d = [xj[0] - xi[0], xj[1] - xi[1], xj[2] - xi[2]];
                    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-4);
                    let mag = s.k * (len - s.rest) / len;
                    f[0] += mag * d[0];
                    f[1] += mag * d[1];
                    f[2] += mag * d[2];
                    if s.axial_w > 0.0 {
                        // Pull z_other − z_self toward the learned offset.
                        f[2] += s.axial_w * (d[2] - s.axial_dz);
                    }
                }
                let (cx, cy, cz) = key(&xi);
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            if let Some(bucket) = grid.get(&(cx + dx, cy + dy, cz + dz)) {
                                for &j in bucket {
                                    if j as usize == i {
                                        continue;
                                    }
                                    let xj = pos[j as usize];
                                    let d = [xi[0] - xj[0], xi[1] - xj[1], xi[2] - xj[2]];
                                    let r2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
                                    if r2 < cut2 {
                                        let r = r2.sqrt().max(1e-2);
                                        let mag = cfg.repel_k / (r2 + 1e-4) / r;
                                        f[0] += mag * d[0];
                                        f[1] += mag * d[1];
                                        f[2] += mag * d[2];
                                    }
                                }
                            }
                        }
                    }
                }
                clamp_len(f, cfg.max_force)
            })
            .collect();
        for i in 0..n {
            let mut v = vel[i];
            for a in 0..3 {
                v[a] = (v[a] + forces[i][a] * cfg.dt) * cfg.damping;
            }
            v = clamp_len(v, cfg.max_velocity);
            for a in 0..3 {
                pos[i][a] = (pos[i][a] + v[a] * cfg.dt * 60.0).clamp(-cfg.bounds, cfg.bounds);
            }
            vel[i] = v;
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_spring_settles_near_its_rest_length() {
        let ts = vec![Triple {
            head: 0,
            rel: 0,
            tail: 1,
        }];
        let cfg = SimCfg {
            repel_k: 0.0,
            center_k: 0.0,
            iterations: 3000,
            ..LIVE
        };
        let adj = uniform_springs(2, &ts, &cfg);
        let p = simulate(&adj, &cfg, 1);
        let d = ((p[0][0] - p[1][0]).powi(2)
            + (p[0][1] - p[1][1]).powi(2)
            + (p[0][2] - p[1][2]).powi(2))
        .sqrt();
        assert!((d - cfg.rest_length).abs() < 2.0, "settled at {d}");
    }
}
