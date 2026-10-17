//! k-nearest-neighbour overlap, the GPU-vs-CPU conformance metric.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::PositionSet;

/// Above this many shared ids the neighbour search uses a uniform grid instead of brute force.
const BRUTE_FORCE_MAX: usize = 20_000;

/// Mean Jaccard similarity of each node's `k` nearest neighbours in `a` and in `b`.
///
/// Only ids present in both sets take part, and neighbours are drawn from the
/// same shared set. Ties in distance break by id, so the result is
/// deterministic. Returns 1.0 for identical layouts, and 0.0 when the sets
/// share no ids or `k` is 0. When `k` is at least the number of shared ids
/// minus one, every node's neighbour set is all the others, so the result is 1.0.
///
/// ```
/// use codescape_layout::{knn_overlap, PositionSet};
///
/// let a = PositionSet {
///     ids: vec![1, 2, 3, 4],
///     pos: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [10.0, 0.0, 0.0], [11.0, 0.0, 0.0]],
///     vel: vec![[0.0; 3]; 4],
/// };
/// assert_eq!(knn_overlap(&a, &a, 1), 1.0);
/// ```
pub fn knn_overlap(a: &PositionSet, b: &PositionSet, k: usize) -> f32 {
    let b_index: HashMap<u32, usize> = b.ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut shared: Vec<(u32, [f32; 3], [f32; 3])> = a
        .ids
        .iter()
        .enumerate()
        .filter_map(|(i, id)| b_index.get(id).map(|&j| (*id, a.pos[i], b.pos[j])))
        .collect();
    shared.sort_unstable_by_key(|s| s.0);
    shared.dedup_by_key(|s| s.0);
    let n = shared.len();
    if n == 0 || k == 0 {
        return 0.0;
    }
    if k + 1 >= n {
        return 1.0;
    }
    let pa: Vec<[f32; 3]> = shared.iter().map(|s| s.1).collect();
    let pb: Vec<[f32; 3]> = shared.iter().map(|s| s.2).collect();
    let na = neighbours(&pa, k);
    let nb = neighbours(&pb, k);
    let total: f64 = na
        .par_iter()
        .zip(nb.par_iter())
        .map(|(x, y)| {
            let inter = x.iter().filter(|i| y.contains(i)).count();
            let union = x.len() + y.len() - inter;
            if union == 0 {
                1.0
            } else {
                inter as f64 / union as f64
            }
        })
        .sum();
    (total / n as f64) as f32
}

fn d2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// For each point, the indices of its `k` nearest other points (sorted by distance, then index).
fn neighbours(points: &[[f32; 3]], k: usize) -> Vec<Vec<u32>> {
    if points.len() <= BRUTE_FORCE_MAX {
        brute(points, k)
    } else {
        Grid::new(points, k).all(points, k)
    }
}

fn brute(points: &[[f32; 3]], k: usize) -> Vec<Vec<u32>> {
    points
        .par_iter()
        .enumerate()
        .map(|(i, &p)| {
            let mut c: Vec<(f32, u32)> = points
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(j, &q)| (d2(p, q), j as u32))
                .collect();
            take_k(&mut c, k)
        })
        .collect()
}

fn take_k(c: &mut Vec<(f32, u32)>, k: usize) -> Vec<u32> {
    let cmp = |x: &(f32, u32), y: &(f32, u32)| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1));
    if c.len() > k {
        c.select_nth_unstable_by(k - 1, cmp);
        c.truncate(k);
    }
    c.sort_unstable_by(cmp);
    c.iter().map(|x| x.1).collect()
}

struct Grid {
    lo: [f32; 3],
    cell: f32,
    dims: [i64; 3],
    /// Point indices sorted by cell key, with `start[key]..start[key + 1]` per cell.
    order: Vec<u32>,
    start: Vec<u32>,
}

impl Grid {
    fn new(points: &[[f32; 3]], k: usize) -> Self {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in points {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let ext: Vec<f32> = (0..3).map(|a| (hi[a] - lo[a]).max(1e-3)).collect();
        // About k points per cell on average, capped at 2^21 cells.
        let cells_wanted = (points.len() as f64 / k.max(1) as f64).clamp(1.0, 2_097_152.0);
        let vol = f64::from(ext[0]) * f64::from(ext[1]) * f64::from(ext[2]);
        let mut cell = (vol / cells_wanted).cbrt() as f32;
        if !cell.is_finite() || cell <= 0.0 {
            cell = 1.0;
        }
        let dims = [0, 1, 2].map(|a| ((ext[a] / cell).ceil() as i64).clamp(1, 1 << 20));
        let mut g = Grid {
            lo,
            cell,
            dims,
            order: Vec::new(),
            start: Vec::new(),
        };
        let ncells = (dims[0] * dims[1] * dims[2]) as usize;
        let keys: Vec<usize> = points.iter().map(|&p| g.key(g.coord(p))).collect();
        let mut counts = vec![0u32; ncells + 1];
        for &key in &keys {
            counts[key + 1] += 1;
        }
        for c in 0..ncells {
            counts[c + 1] += counts[c];
        }
        g.start = counts.clone();
        let mut cursor = counts;
        g.order = vec![0; points.len()];
        for (i, &key) in keys.iter().enumerate() {
            g.order[cursor[key] as usize] = i as u32;
            cursor[key] += 1;
        }
        g
    }

    fn coord(&self, p: [f32; 3]) -> [i64; 3] {
        [0, 1, 2].map(|a| (((p[a] - self.lo[a]) / self.cell) as i64).clamp(0, self.dims[a] - 1))
    }

    fn key(&self, c: [i64; 3]) -> usize {
        ((c[2] * self.dims[1] + c[1]) * self.dims[0] + c[0]) as usize
    }

    fn all(&self, points: &[[f32; 3]], k: usize) -> Vec<Vec<u32>> {
        (0..points.len())
            .into_par_iter()
            .map(|i| self.query(points, i, k))
            .collect()
    }

    /// Visit rings of cells at Chebyshev radius 0, 1, 2… Every point outside
    /// ring `r` is at least `r × cell` away, so stop once the k-th best is that close.
    fn query(&self, points: &[[f32; 3]], i: usize, k: usize) -> Vec<u32> {
        let p = points[i];
        let c = self.coord(p);
        let max_r = self.dims.iter().copied().max().unwrap_or(1);
        let mut cand: Vec<(f32, u32)> = Vec::new();
        for r in 0..=max_r {
            for z in c[2] - r..=c[2] + r {
                for y in c[1] - r..=c[1] + r {
                    for x in c[0] - r..=c[0] + r {
                        let on_shell =
                            (x - c[0]).abs() == r || (y - c[1]).abs() == r || (z - c[2]).abs() == r;
                        if !on_shell
                            || x < 0
                            || y < 0
                            || z < 0
                            || x >= self.dims[0]
                            || y >= self.dims[1]
                            || z >= self.dims[2]
                        {
                            continue;
                        }
                        let key = self.key([x, y, z]);
                        for &j in
                            &self.order[self.start[key] as usize..self.start[key + 1] as usize]
                        {
                            if j as usize != i {
                                cand.push((d2(p, points[j as usize]), j));
                            }
                        }
                    }
                }
            }
            if cand.len() >= k {
                let mut sorted = cand.clone();
                let kth = {
                    let cmp = |x: &(f32, u32), y: &(f32, u32)| x.0.total_cmp(&y.0);
                    sorted.select_nth_unstable_by(k - 1, cmp);
                    sorted[k - 1].0
                };
                let reach = r as f32 * self.cell;
                if kth < reach * reach {
                    break;
                }
            }
        }
        take_k(&mut cand, k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    fn set(points: Vec<[f32; 3]>) -> PositionSet {
        let n = points.len();
        PositionSet {
            ids: (1..=n as u32).collect(),
            pos: points,
            vel: vec![[0.0; 3]; n],
        }
    }

    fn cloud(n: usize, seed: u64) -> Vec<[f32; 3]> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        (0..n)
            .map(|_| {
                [
                    rng.gen_range(-50.0..50.0),
                    rng.gen_range(-50.0..50.0),
                    rng.gen_range(-50.0..50.0),
                ]
            })
            .collect()
    }

    #[test]
    fn identical_and_rigidly_moved_layouts_agree() {
        let a = set(cloud(300, 1));
        assert_eq!(knn_overlap(&a, &a, 8), 1.0);
        let moved = set(a
            .pos
            .iter()
            .map(|p| [p[1] + 100.0, -p[0], p[2] * 1.0])
            .collect());
        assert!(
            (knn_overlap(&a, &moved, 8) - 1.0).abs() < 1e-6,
            "rotation + translation preserves neighbourhoods"
        );
    }

    #[test]
    fn unrelated_layouts_disagree() {
        let a = set(cloud(2000, 1));
        let b = set(cloud(2000, 2));
        assert!(knn_overlap(&a, &b, 10) < 0.05);
    }

    #[test]
    fn grid_matches_brute_force() {
        let pts = cloud(3000, 5);
        let g = Grid::new(&pts, 10);
        let fast = g.all(&pts, 10);
        let slow = brute(&pts, 10);
        assert_eq!(fast, slow);
    }

    #[test]
    fn only_shared_ids_count() {
        let a = set(vec![[0.0; 3], [1.0, 0.0, 0.0], [5.0, 0.0, 0.0]]);
        let mut b = a.clone();
        b.ids = vec![7, 8, 9];
        assert_eq!(knn_overlap(&a, &b, 1), 0.0, "no shared ids");
        assert_eq!(knn_overlap(&a, &a, 0), 0.0);
        assert_eq!(knn_overlap(&a, &a, 5), 1.0, "k covers every other node");
    }
}
