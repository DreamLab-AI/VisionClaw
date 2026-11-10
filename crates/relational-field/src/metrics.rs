//! Geometry-only metrics M1–M5 for the static H1 test (`PREREGISTRATION.md`, "Metrics", with the
//! details pinned in deviation D4).
//!
//! Every function here reads final node positions and nothing else from an arm, so every arm is
//! scored the same way and the score reflects what a viewer could see.

use crate::corpus::Triple;
use crate::embed::Pos;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::HashSet;

/// Variance floor added to every per-feature Gaussian variance in M3, in m-normalised units.
pub const VAR_FLOOR: f64 = 1e-3;

/// Minimum training triples a relation type needs before M3 fits a Gaussian for it.
pub const MIN_TYPE_TRAIN: usize = 10;

/// Neighbourhood size for M4 and M5.
pub const K: usize = 10;

/// Euclidean distance between nodes `i` and `j`.
pub fn dist(p: &Pos, i: u32, j: u32) -> f64 {
    let (a, b) = (p[i as usize], p[j as usize]);
    let d: f64 = (0..3)
        .map(|k| (f64::from(a[k]) - f64::from(b[k])).powi(2))
        .sum();
    d.sqrt()
}

/// Mann–Whitney AUC: the probability that a random positive outscores a random negative, ties
/// counting one half. Returns 0.5 when either side is empty.
pub fn auc(pos: &[f64], neg: &[f64]) -> f64 {
    if pos.is_empty() || neg.is_empty() {
        return 0.5;
    }
    let mut all: Vec<(f64, bool)> = pos
        .iter()
        .map(|&s| (s, true))
        .chain(neg.iter().map(|&s| (s, false)))
        .collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Average ranks over tied runs (1-based), then the rank-sum form of U.
    let mut rank_sum_pos = 0f64;
    let mut i = 0;
    while i < all.len() {
        let mut j = i;
        while j + 1 < all.len() && all[j + 1].0.total_cmp(&all[i].0) == Ordering::Equal {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        rank_sum_pos += all[i..=j].iter().filter(|x| x.1).count() as f64 * avg;
        i = j + 1;
    }
    let (np, nn) = (pos.len() as f64, neg.len() as f64);
    (rank_sum_pos - np * (np + 1.0) / 2.0) / (np * nn)
}

/// Link AUC scoring each pair by `−d` (M1 with random negatives, M2 with hard negatives).
pub fn link_auc(p: &Pos, positives: &[(u32, u32)], negatives: &[(u32, u32)]) -> f64 {
    let s = |v: &[(u32, u32)]| v.iter().map(|&(i, j)| -dist(p, i, j)).collect::<Vec<_>>();
    auc(&s(positives), &s(negatives))
}

/// Median, taking the mean of the two middle values for an even count; 0 for an empty slice.
pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// The arm's median training-triple length `m`, floored so features stay finite.
pub fn median_edge_length(p: &Pos, train: &[Triple]) -> f64 {
    let d: Vec<f64> = train.iter().map(|t| dist(p, t.head, t.tail)).collect();
    median(&d).max(1e-12)
}

/// M3 features `(d / m, Δz / m)` for a triple in its recorded direction, `Δz = z_tail − z_head`.
pub fn features(p: &Pos, t: &Triple, m: f64) -> [f64; 2] {
    let dz = f64::from(p[t.tail as usize][2]) - f64::from(p[t.head as usize][2]);
    [dist(p, t.head, t.tail) / m, dz / m]
}

/// One fitted per-type Gaussian.
#[derive(Clone, Debug)]
struct TypeGaussian {
    rel: u16,
    log_prior: f64,
    mean: [f64; 2],
    var: [f64; 2],
}

/// Gaussian naive Bayes over relation types with diagonal covariance and a training-frequency
/// prior, fitted on one arm's training triples (M3).
#[derive(Clone, Debug)]
pub struct TypeClassifier {
    m: f64,
    classes: Vec<TypeGaussian>,
}

impl TypeClassifier {
    /// Fit on `train` with geometry `p`. Types with fewer than [`MIN_TYPE_TRAIN`] triples get no
    /// Gaussian and are never predicted.
    pub fn fit(p: &Pos, train: &[Triple], n_rel: usize) -> Self {
        let m = median_edge_length(p, train);
        let mut by_rel: Vec<Vec<[f64; 2]>> = vec![Vec::new(); n_rel];
        for t in train {
            by_rel[t.rel as usize].push(features(p, t, m));
        }
        let total = train.len() as f64;
        let classes = by_rel
            .iter()
            .enumerate()
            .filter(|(_, xs)| xs.len() >= MIN_TYPE_TRAIN)
            .map(|(r, xs)| {
                let n = xs.len() as f64;
                let mut mean = [0f64; 2];
                let mut var = [0f64; 2];
                for a in 0..2 {
                    mean[a] = xs.iter().map(|x| x[a]).sum::<f64>() / n;
                    var[a] = xs.iter().map(|x| (x[a] - mean[a]).powi(2)).sum::<f64>() / (n - 1.0)
                        + VAR_FLOOR;
                }
                TypeGaussian {
                    rel: r as u16,
                    log_prior: (n / total).ln(),
                    mean,
                    var,
                }
            })
            .collect();
        Self { m, classes }
    }

    /// The median training-edge length the features are normalised by.
    pub fn scale(&self) -> f64 {
        self.m
    }

    /// Most probable type for features `x`; an exact tie goes to the lowest relation id.
    pub fn predict(&self, x: [f64; 2]) -> Option<u16> {
        let mut best: Option<(f64, u16)> = None;
        for c in &self.classes {
            let ll: f64 = c.log_prior
                + (0..2)
                    .map(|a| {
                        -0.5 * (std::f64::consts::TAU * c.var[a]).ln()
                            - (x[a] - c.mean[a]).powi(2) / (2.0 * c.var[a])
                    })
                    .sum::<f64>();
            // Classes are in ascending id order, so a strict `>` keeps the lowest id on a tie.
            if best.is_none_or(|(b, _)| ll > b) {
                best = Some((ll, c.rel));
            }
        }
        best.map(|b| b.1)
    }

    /// Fraction of `test` triples whose predicted type is their own; 0 for an empty test set.
    pub fn accuracy(&self, p: &Pos, test: &[Triple]) -> f64 {
        if test.is_empty() {
            return 0.0;
        }
        let hits = test
            .iter()
            .filter(|t| self.predict(features(p, t, self.m)) == Some(t.rel))
            .count();
        hits as f64 / test.len() as f64
    }
}

/// The most frequent training type (lowest id on a tie), or `None` for an empty training set.
pub fn majority_type(train: &[Triple], n_rel: usize) -> Option<u16> {
    let mut c = vec![0usize; n_rel];
    train.iter().for_each(|t| c[t.rel as usize] += 1);
    let max = *c.iter().max()?;
    (max > 0).then(|| c.iter().position(|&x| x == max).expect("max is present") as u16)
}

/// M3 chance: test accuracy of always predicting the most frequent training type.
pub fn chance(train: &[Triple], test: &[Triple], n_rel: usize) -> f64 {
    match majority_type(train, n_rel) {
        Some(r) if !test.is_empty() => {
            test.iter().filter(|t| t.rel == r).count() as f64 / test.len() as f64
        }
        _ => 0.0,
    }
}

/// The `k` nearest other nodes of every node, nearest first, distance ties to the lower id.
pub fn knn(p: &Pos, k: usize) -> Vec<Vec<u32>> {
    let n = p.len();
    (0..n)
        .into_par_iter()
        .map(|i| {
            let mut c: Vec<(f64, u32)> = (0..n)
                .filter(|&j| j != i)
                .map(|j| (dist(p, i as u32, j as u32), j as u32))
                .collect();
            let kk = k.min(c.len());
            let cmp = |a: &(f64, u32), b: &(f64, u32)| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1));
            if kk < c.len() {
                c.select_nth_unstable_by(kk, cmp);
                c.truncate(kk);
            }
            c.sort_by(cmp);
            c.into_iter().map(|x| x.1).collect()
        })
        .collect()
}

/// M4: leave-one-out kNN majority vote on `domain` over `nodes` (each must have a domain). A
/// vote tie goes to the tied domain whose nearest member comes first in the neighbour list.
/// Neighbours without a domain do not vote.
pub fn domain_knn_accuracy(nn: &[Vec<u32>], domain: &[Option<u16>], nodes: &[u32]) -> f64 {
    if nodes.is_empty() {
        return 0.0;
    }
    let hits = nodes
        .iter()
        .filter(|&&i| {
            let mut votes: Vec<(u16, usize, usize)> = Vec::new(); // domain, count, first position
            for (pos, &j) in nn[i as usize].iter().enumerate() {
                let Some(d) = domain[j as usize] else {
                    continue;
                };
                match votes.iter_mut().find(|v| v.0 == d) {
                    Some(v) => v.1 += 1,
                    None => votes.push((d, 1, pos)),
                }
            }
            let winner = votes
                .iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
                .map(|v| v.0);
            winner.is_some() && winner == domain[i as usize]
        })
        .count();
    hits as f64 / nodes.len() as f64
}

/// Distinct training neighbours of every node, either direction.
pub fn neighbours(n: usize, train: &[Triple]) -> Vec<HashSet<u32>> {
    let mut nb = vec![HashSet::new(); n];
    for t in train {
        nb[t.head as usize].insert(t.tail);
        nb[t.tail as usize].insert(t.head);
    }
    nb
}

/// M5: mean over nodes with at least one training neighbour of the fraction of those neighbours
/// found among the node's `k` nearest nodes (not capped at `k / degree`).
pub fn neighbour_recall(nn: &[Vec<u32>], nb: &[HashSet<u32>]) -> f64 {
    let rs: Vec<f64> = nb
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, s)| nn[i].iter().filter(|j| s.contains(j)).count() as f64 / s.len() as f64)
        .collect();
    if rs.is_empty() {
        0.0
    } else {
        rs.iter().sum::<f64>() / rs.len() as f64
    }
}

/// Mean and sample standard deviation (n − 1; 0 for fewer than two values).
pub fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    if v.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let mean = v.iter().sum::<f64>() / n;
    let sd = if v.len() < 2 {
        0.0
    } else {
        (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
    };
    (mean, sd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    #[test]
    fn auc_hand_cases() {
        assert_eq!(auc(&[3.0, 4.0], &[1.0, 2.0]), 1.0);
        assert_eq!(auc(&[1.0], &[2.0]), 0.0);
        assert_eq!(auc(&[2.0, 2.0], &[2.0]), 0.5);
        // pos {3, 1} vs neg {2, 2}: 3 beats both, 1 beats neither → 2 / 4.
        assert_eq!(auc(&[3.0, 1.0], &[2.0, 2.0]), 0.5);
        // pos {2} vs neg {2, 1}: one tie (½) and one win → 1.5 / 2.
        assert_eq!(auc(&[2.0], &[2.0, 1.0]), 0.75);
        assert_eq!(auc(&[], &[1.0]), 0.5);
    }

    #[test]
    fn link_auc_prefers_shorter_positive_pairs() {
        let p: Pos = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [9.0, 0.0, 0.0],
        ];
        assert_eq!(link_auc(&p, &[(0, 1)], &[(0, 2), (1, 3)]), 1.0);
        assert_eq!(link_auc(&p, &[(0, 3)], &[(0, 1)]), 0.0);
    }

    #[test]
    fn median_odd_even_empty() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn mean_sd_uses_n_minus_one() {
        let (m, s) = mean_sd(&[1.0, 2.0, 3.0]);
        assert_eq!(m, 2.0);
        assert!((s - 1.0).abs() < 1e-12);
        assert_eq!(mean_sd(&[5.0]), (5.0, 0.0));
    }

    /// Build `n_pairs` edges per type: type 0 short and pointing up, type 1 long and flat. In the
    /// typed geometry each edge's tail sits at its type's offset; in the uniform one every edge
    /// gets the same random-direction unit offset whatever its type.
    fn planted(typed: bool, n_pairs: u32, seed: u64) -> (Pos, Vec<Triple>) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut p: Pos = Vec::new();
        let mut ts = Vec::new();
        for k in 0..2 * n_pairs {
            let rel = (k % 2) as u16;
            let base = [
                rng.gen_range(-100.0..100.0),
                rng.gen_range(-100.0..100.0),
                rng.gen_range(-100.0..100.0),
            ];
            let jitter = |r: &mut StdRng| r.gen_range(-0.05..0.05f32);
            let off = if typed {
                if rel == 0 {
                    [jitter(&mut rng), jitter(&mut rng), 1.0 + jitter(&mut rng)]
                } else {
                    [3.0 + jitter(&mut rng), jitter(&mut rng), jitter(&mut rng)]
                }
            } else {
                let v = [
                    rng.gen_range(-1.0..1.0f32),
                    rng.gen_range(-1.0..1.0f32),
                    rng.gen_range(-1.0..1.0f32),
                ];
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-3);
                [v[0] / l, v[1] / l, v[2] / l]
            };
            let h = p.len() as u32;
            p.push(base);
            p.push([base[0] + off[0], base[1] + off[1], base[2] + off[2]]);
            ts.push(Triple {
                head: h,
                rel,
                tail: h + 1,
            });
        }
        (p, ts)
    }

    #[test]
    fn planted_typed_geometry_beats_uniform_on_m3() {
        let (pt, tt) = planted(true, 200, 1);
        let (pu, tu) = planted(false, 200, 1);
        let (train_t, test_t) = tt.split_at(300);
        let (train_u, test_u) = tu.split_at(300);
        let acc_t = TypeClassifier::fit(&pt, train_t, 2).accuracy(&pt, test_t);
        let acc_u = TypeClassifier::fit(&pu, train_u, 2).accuracy(&pu, test_u);
        let ch = chance(train_u, test_u, 2);
        assert!(
            acc_t > 0.99,
            "typed geometry should separate the types: {acc_t}"
        );
        assert!(
            acc_u < ch + 0.1,
            "uniform geometry should sit near chance: {acc_u} vs {ch}"
        );
        assert!(acc_t - acc_u > 0.3);
    }

    #[test]
    fn null_labels_do_not_beat_chance_on_m3() {
        // Typed geometry, but labels reassigned at random with a 70/30 split: geometry carries no
        // information about the label, so accuracy must stay near the majority rate.
        let (p, mut ts) = planted(true, 300, 2);
        let mut rng = StdRng::seed_from_u64(9);
        ts.iter_mut()
            .for_each(|t| t.rel = u16::from(rng.gen_bool(0.3)));
        let (train, test) = ts.split_at(400);
        let acc = TypeClassifier::fit(&p, train, 2).accuracy(&p, test);
        let ch = chance(train, test, 2);
        assert!(acc <= ch + 0.05, "null case beat chance: {acc} vs {ch}");
    }

    #[test]
    fn rare_types_are_never_predicted_but_count_as_errors() {
        let (p, mut ts) = planted(true, 50, 3);
        // Relabel five type-1 edges as type 2: too few to fit, so they can only be errors.
        let mut moved = 0;
        for t in ts
            .iter_mut()
            .filter(|t| t.rel == 1)
            .take(MIN_TYPE_TRAIN - 5)
        {
            t.rel = 2;
            moved += 1;
        }
        let c = TypeClassifier::fit(&p, &ts, 3);
        assert!(c.classes.iter().all(|g| g.rel != 2));
        let acc = c.accuracy(&p, &ts);
        assert!(acc <= 1.0 - moved as f64 / ts.len() as f64 + 1e-12);
    }

    #[test]
    fn exact_tie_goes_to_lowest_id() {
        let c = TypeClassifier {
            m: 1.0,
            classes: vec![
                TypeGaussian {
                    rel: 1,
                    log_prior: 0.0,
                    mean: [0.0; 2],
                    var: [1.0; 2],
                },
                TypeGaussian {
                    rel: 4,
                    log_prior: 0.0,
                    mean: [0.0; 2],
                    var: [1.0; 2],
                },
            ],
        };
        assert_eq!(c.predict([0.3, 0.2]), Some(1));
    }

    #[test]
    fn chance_is_training_majority_on_test() {
        let t = |rel| Triple {
            head: 0,
            rel,
            tail: 1,
        };
        let train = [t(0), t(1), t(1)];
        let test = [t(1), t(0), t(0), t(0)];
        assert_eq!(chance(&train, &test, 2), 0.25);
    }

    #[test]
    fn knn_on_a_line_with_ties_to_lower_id() {
        let p: Pos = (0..5).map(|i| [i as f32, 0.0, 0.0]).collect();
        let nn = knn(&p, 2);
        assert_eq!(nn[0], vec![1, 2]);
        assert_eq!(nn[2], vec![1, 3]); // 1 and 3 tie at distance 1; lower id first
        assert_eq!(nn[4], vec![3, 2]);
    }

    #[test]
    fn domain_vote_and_tie_rule() {
        // Node 0's neighbours in order: 1 (A), 2 (B), 3 (B), 4 (A). Tie 2-2; A's nearest is first.
        let nn = vec![vec![1, 2, 3, 4], vec![0], vec![0], vec![0], vec![0]];
        let dom = vec![Some(0), Some(0), Some(1), Some(1), Some(0)];
        assert_eq!(domain_knn_accuracy(&nn, &dom, &[0]), 1.0);
        let dom_b = vec![Some(1), Some(0), Some(1), Some(1), Some(0)];
        assert_eq!(domain_knn_accuracy(&nn, &dom_b, &[0]), 0.0);
    }

    #[test]
    fn neighbour_recall_hand_case() {
        let t = |h, tl| Triple {
            head: h,
            rel: 0,
            tail: tl,
        };
        let nb = neighbours(4, &[t(0, 1), t(0, 2), t(2, 1)]);
        // 0: {1,2} with kNN {1,3} → ½; 1: {0,2} with {0,2} → 1; 2: {0,1} with {3,0} → ½; 3 skipped.
        let nn = vec![vec![1, 3], vec![0, 2], vec![3, 0], vec![0, 1]];
        assert!((neighbour_recall(&nn, &nb) - 2.0 / 3.0).abs() < 1e-12);
    }
}
