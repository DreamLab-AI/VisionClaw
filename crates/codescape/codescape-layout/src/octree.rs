//! Barnes-Hut octree for `k / (d² + ε)` repulsion between unit-mass bodies.

/// Bodies per leaf before a cell splits.
const LEAF_SIZE: usize = 8;
/// Depth limit, so coincident bodies cannot recurse forever.
const MAX_DEPTH: u32 = 24;
const NO_CHILD: u32 = u32::MAX;
/// Pairs closer than this (squared) exert no force, as in `force_pass_kernel`.
pub(crate) const MIN_DIST_SQ: f32 = 1e-6;

#[derive(Debug, Clone, Copy)]
struct Cell {
    center: [f32; 3],
    half: f32,
    com: [f32; 3],
    mass: f32,
    /// Index of the first of eight consecutive children, or [`NO_CHILD`] for a leaf.
    child: u32,
    start: u32,
    end: u32,
}

/// An octree over a point set, rebuilt every step.
#[derive(Debug, Default)]
pub(crate) struct Octree {
    cells: Vec<Cell>,
    perm: Vec<u32>,
    scratch: Vec<u32>,
}

/// Parameters of the repulsion law.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Repulsion {
    /// `k` in `k / (d² + ε)`.
    pub k: f32,
    /// Softening `ε`.
    pub eps: f32,
    /// Per-pair cap on the magnitude.
    pub max_pair: f32,
    /// Squared opening angle.
    pub theta2: f32,
}

impl Octree {
    /// Rebuild over `points`, reusing allocations.
    pub fn rebuild(&mut self, points: &[[f32; 3]]) {
        self.cells.clear();
        self.perm.clear();
        self.perm.extend(0..points.len() as u32);
        self.scratch.resize(points.len(), 0);
        if points.is_empty() {
            return;
        }
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in points {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let center = [
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
        ];
        let half = ((hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]) * 0.5).max(1e-3) * 1.001;
        self.cells.push(Cell {
            center,
            half,
            com: [0.0; 3],
            mass: 0.0,
            child: NO_CHILD,
            start: 0,
            end: points.len() as u32,
        });
        self.build(points, 0, 0);
    }

    fn build(&mut self, points: &[[f32; 3]], cell: usize, depth: u32) {
        let Cell {
            center,
            half,
            start,
            end,
            ..
        } = self.cells[cell];
        let (s, e) = (start as usize, end as usize);
        if e - s <= LEAF_SIZE || depth >= MAX_DEPTH {
            let mut com = [0.0f64; 3];
            for &i in &self.perm[s..e] {
                for k in 0..3 {
                    com[k] += f64::from(points[i as usize][k]);
                }
            }
            let m = (e - s) as f64;
            let c = &mut self.cells[cell];
            c.mass = m as f32;
            c.com = if m > 0.0 {
                [
                    (com[0] / m) as f32,
                    (com[1] / m) as f32,
                    (com[2] / m) as f32,
                ]
            } else {
                center
            };
            return;
        }
        // Counting sort of the cell's range into octants.
        let octant = |p: &[f32; 3]| -> usize {
            usize::from(p[0] >= center[0])
                | usize::from(p[1] >= center[1]) << 1
                | usize::from(p[2] >= center[2]) << 2
        };
        let mut counts = [0usize; 8];
        for &i in &self.perm[s..e] {
            counts[octant(&points[i as usize])] += 1;
        }
        let mut offs = [0usize; 9];
        for o in 0..8 {
            offs[o + 1] = offs[o] + counts[o];
        }
        let mut cursor = offs;
        for k in s..e {
            let i = self.perm[k];
            let o = octant(&points[i as usize]);
            self.scratch[s + cursor[o]] = i;
            cursor[o] += 1;
        }
        self.perm[s..e].copy_from_slice(&self.scratch[s..e]);

        let first = self.cells.len() as u32;
        let h = half * 0.5;
        for o in 0..8 {
            let c = [
                center[0] + if o & 1 != 0 { h } else { -h },
                center[1] + if o & 2 != 0 { h } else { -h },
                center[2] + if o & 4 != 0 { h } else { -h },
            ];
            self.cells.push(Cell {
                center: c,
                half: h,
                com: c,
                mass: 0.0,
                child: NO_CHILD,
                start: (s + offs[o]) as u32,
                end: (s + offs[o + 1]) as u32,
            });
        }
        self.cells[cell].child = first;
        for o in 0..8 {
            let ci = first as usize + o;
            if self.cells[ci].end > self.cells[ci].start {
                self.build(points, ci, depth + 1);
            }
        }
        let mut com = [0.0f64; 3];
        let mut mass = 0.0f64;
        for o in 0..8 {
            let c = &self.cells[first as usize + o];
            let m = f64::from(c.mass);
            mass += m;
            for k in 0..3 {
                com[k] += f64::from(c.com[k]) * m;
            }
        }
        let c = &mut self.cells[cell];
        c.mass = mass as f32;
        if mass > 0.0 {
            c.com = [
                (com[0] / mass) as f32,
                (com[1] / mass) as f32,
                (com[2] / mass) as f32,
            ];
        }
    }

    /// Repulsive force on body `i` at `p` from every other body.
    pub fn force_on(&self, i: u32, p: [f32; 3], points: &[[f32; 3]], law: Repulsion) -> [f32; 3] {
        let mut f = [0.0f32; 3];
        if self.cells.is_empty() {
            return f;
        }
        let mut stack = [0u32; 8 * MAX_DEPTH as usize + 8];
        let mut top = 1usize;
        stack[0] = 0;
        while top > 0 {
            top -= 1;
            let cell = &self.cells[stack[top] as usize];
            if cell.mass == 0.0 {
                continue;
            }
            if cell.child == NO_CHILD {
                for &j in &self.perm[cell.start as usize..cell.end as usize] {
                    if j != i {
                        add_pair(&mut f, p, points[j as usize], 1.0, law);
                    }
                }
                continue;
            }
            let inside = (0..3).all(|k| (p[k] - cell.center[k]).abs() <= cell.half);
            let d = [p[0] - cell.com[0], p[1] - cell.com[1], p[2] - cell.com[2]];
            let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            let width = 2.0 * cell.half;
            if !inside && width * width < law.theta2 * d2 {
                add_pair(&mut f, p, cell.com, cell.mass, law);
            } else {
                for o in 0..8 {
                    stack[top] = cell.child + o;
                    top += 1;
                }
            }
        }
        f
    }
}

/// Accumulate the force from `mass` units at `q` onto a body at `p`.
#[inline]
fn add_pair(f: &mut [f32; 3], p: [f32; 3], q: [f32; 3], mass: f32, law: Repulsion) {
    let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if d2 <= MIN_DIST_SQ {
        return;
    }
    let mag = (law.k / (d2 + law.eps)).min(law.max_pair) * mass;
    let s = mag / d2.sqrt();
    f[0] += d[0] * s;
    f[1] += d[1] * s;
    f[2] += d[2] * s;
}

/// Exact O(n²) repulsion on body `i`, the reference for the octree.
#[cfg(test)]
pub(crate) fn brute_force_on(i: usize, points: &[[f32; 3]], law: Repulsion) -> [f32; 3] {
    let mut f = [0.0f32; 3];
    for (j, q) in points.iter().enumerate() {
        if j != i {
            add_pair(&mut f, points[i], *q, 1.0, law);
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    fn cloud(n: usize, seed: u64) -> Vec<[f32; 3]> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        (0..n)
            .map(|_| {
                [
                    rng.gen_range(-100.0..100.0),
                    rng.gen_range(-100.0..100.0),
                    rng.gen_range(-100.0..100.0),
                ]
            })
            .collect()
    }

    fn norm(v: [f32; 3]) -> f32 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }

    #[test]
    fn theta_zero_is_exact() {
        let pts = cloud(500, 1);
        let mut t = Octree::default();
        t.rebuild(&pts);
        let law = Repulsion {
            k: 100.0,
            eps: 1.0,
            max_pair: 1e9,
            theta2: 0.0,
        };
        for i in [0usize, 17, 250, 499] {
            let a = t.force_on(i as u32, pts[i], &pts, law);
            let b = brute_force_on(i, &pts, law);
            for k in 0..3 {
                assert!(
                    (a[k] - b[k]).abs() <= 1e-3 * norm(b).max(1.0),
                    "{a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn approximation_error_is_small_at_default_theta() {
        let pts = cloud(4000, 2);
        let mut t = Octree::default();
        t.rebuild(&pts);
        let law = Repulsion {
            k: 100.0,
            eps: 1.0,
            max_pair: 1e9,
            theta2: 0.8 * 0.8,
        };
        let mut worst = 0.0f32;
        for i in (0..pts.len()).step_by(97) {
            let a = t.force_on(i as u32, pts[i], &pts, law);
            let b = brute_force_on(i, &pts, law);
            let err = norm([a[0] - b[0], a[1] - b[1], a[2] - b[2]]) / norm(b).max(1e-6);
            worst = worst.max(err);
        }
        assert!(worst < 0.1, "relative Barnes-Hut error {worst}");
    }

    #[test]
    fn coincident_points_terminate() {
        let pts = vec![[1.0, 1.0, 1.0]; 100];
        let mut t = Octree::default();
        t.rebuild(&pts);
        let law = Repulsion {
            k: 1.0,
            eps: 1.0,
            max_pair: 1.0,
            theta2: 0.64,
        };
        assert_eq!(t.force_on(0, pts[0], &pts, law), [0.0; 3]);
    }
}
