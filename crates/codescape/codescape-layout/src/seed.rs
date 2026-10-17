//! Deterministic initial placement.
//!
//! Every random draw comes from a ChaCha stream keyed by `(params.seed, node id)`.
//! A node's seed position therefore depends only on its own id, its ancestors
//! and how many siblings it has. It does not depend on snapshot order, and
//! adding an unrelated node does not move it.

use std::collections::HashMap;

use codescape_model::{GraphSnapshot, Tier};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{LayoutParams, PositionSet};

/// Golden angle in radians, `π (3 − √5)`.
const GOLDEN_ANGLE: f32 = 2.399_963_1;
/// Ball radius around a directory, per cube root of its file count, as a fraction of `shell_radius`.
const DIR_BALL: f32 = 0.06;
/// Ball radius around a file (or class), per cube root of its member count, as a fraction of `shell_radius`.
const FILE_BALL: f32 = 0.015;
/// Parentless non-directory nodes go on a shell this much further out than the directories.
const OUTER_SHELL: f32 = 1.25;
/// Jitter radius for a node added under an existing directory, as a fraction of `shell_radius`.
const NEW_UNDER_DIR: f32 = 0.03;
/// Jitter radius for a node added under an existing file or symbol, as a fraction of `shell_radius`.
const NEW_UNDER_FILE: f32 = 0.006;

/// Place every node of `snapshot`. Velocities are zero.
///
/// * Top-level directories (L0 with no parent) sit on a sphere of radius
///   `shell_radius`, spread by the golden-angle spiral in `(cluster, id)` order.
/// * Every other parentless node sits on an outer shell, at `1.25 × shell_radius`,
///   in a seeded random direction.
/// * A node with a parent lands uniformly in a ball around the parent. The
///   ball's radius grows with the cube root of the parent's child count. It is
///   wide around a directory and small around a file or symbol.
///
/// ```
/// use codescape_layout::{hierarchical_seed, LayoutParams};
/// use codescape_model::GraphSnapshot;
///
/// let p = hierarchical_seed(&GraphSnapshot::default(), &LayoutParams::default());
/// assert!(p.is_empty());
/// ```
pub fn hierarchical_seed(snapshot: &GraphSnapshot, params: &LayoutParams) -> PositionSet {
    let known = HashMap::new();
    let pos = place(snapshot, params, &known, false);
    PositionSet {
        ids: snapshot.nodes.iter().map(|n| n.id).collect(),
        pos,
        vel: vec![[0.0; 3]; snapshot.nodes.len()],
    }
}

/// Positions for every node of `snapshot`, in snapshot order.
///
/// Nodes in `known` keep their position. If `incremental` is set, a missing
/// node whose parent is placed starts at the parent plus a small jitter (the
/// [`crate::LayoutEngine::apply_delta`] rule). Otherwise it is seeded
/// hierarchically, relative to the parent's position. Missing parentless nodes
/// always get their [`hierarchical_seed`] position.
pub(crate) fn place(
    snapshot: &GraphSnapshot,
    params: &LayoutParams,
    known: &HashMap<u32, [f32; 3]>,
    incremental: bool,
) -> Vec<[f32; 3]> {
    let n = snapshot.nodes.len();
    let idx = snapshot.index();
    let parent_of = |i: usize| -> Option<usize> {
        let p = snapshot.nodes[i].parent;
        if p == 0 {
            None
        } else {
            idx.get(&p).copied().filter(|&pi| pi != i)
        }
    };

    // Depth via the parent chain, with a guard so a malformed cycle cannot hang.
    let mut depth = vec![u32::MAX; n];
    for i in 0..n {
        if depth[i] != u32::MAX {
            continue;
        }
        let mut chain = Vec::new();
        let mut cur = i;
        let base = loop {
            if depth[cur] != u32::MAX {
                break depth[cur] + 1;
            }
            if chain.len() > 64 || chain.contains(&cur) {
                break 0; // cycle: treat the start of the chain as a root
            }
            chain.push(cur);
            match parent_of(cur) {
                Some(p) => cur = p,
                None => break 0,
            }
        };
        // `chain` runs child → … → topmost; assign from the top down.
        for (k, &c) in chain.iter().rev().enumerate() {
            if depth[c] == u32::MAX {
                depth[c] = base + k as u32;
            }
        }
    }

    let mut children = vec![0u32; n];
    for i in 0..n {
        if let Some(p) = parent_of(i) {
            children[p] += 1;
        }
    }

    // Top-level directories in a stable (cluster, id) order for the spiral.
    let mut roots: Vec<usize> = (0..n)
        .filter(|&i| snapshot.nodes[i].tier == Tier::L0 && parent_of(i).is_none())
        .collect();
    roots.sort_by_key(|&i| (snapshot.nodes[i].cluster, snapshot.nodes[i].id));
    let root_rank: HashMap<usize, usize> = roots.iter().enumerate().map(|(k, &i)| (i, k)).collect();

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (depth[i], snapshot.nodes[i].id));

    let r = params.shell_radius.max(1.0);
    let mut pos = vec![[0.0f32; 3]; n];
    let mut placed = vec![false; n];
    for i in order {
        let node = &snapshot.nodes[i];
        if let Some(&p) = known.get(&node.id) {
            pos[i] = p;
            placed[i] = true;
            continue;
        }
        let mut rng = node_rng(params.seed, node.id);
        let parent = parent_of(i).filter(|&p| placed[p]);
        pos[i] = match parent {
            Some(p) => {
                let around = pos[p];
                let ball = if incremental {
                    if snapshot.nodes[p].tier == Tier::L0 {
                        NEW_UNDER_DIR * r
                    } else {
                        NEW_UNDER_FILE * r
                    }
                } else {
                    let per = if snapshot.nodes[p].tier == Tier::L0 {
                        DIR_BALL
                    } else {
                        FILE_BALL
                    };
                    per * r * (children[p].max(1) as f32).cbrt()
                };
                add(around, in_ball(&mut rng, ball))
            }
            None => match root_rank.get(&i) {
                Some(&k) => scale(fibonacci_sphere(k, roots.len()), r),
                None => scale(unit_vector(&mut rng), r * OUTER_SHELL),
            },
        };
        placed[i] = true;
    }
    pos
}

/// The `k`-th of `m` points of a golden-angle (Fibonacci) spiral on the unit sphere.
pub(crate) fn fibonacci_sphere(k: usize, m: usize) -> [f32; 3] {
    if m <= 1 {
        return [1.0, 0.0, 0.0];
    }
    let y = 1.0 - 2.0 * (k as f32 + 0.5) / m as f32;
    let ring = (1.0 - y * y).max(0.0).sqrt();
    let phi = GOLDEN_ANGLE * k as f32;
    [ring * phi.cos(), y, ring * phi.sin()]
}

/// A ChaCha stream unique to `(seed, id)`.
pub(crate) fn node_rng(seed: u64, id: u32) -> ChaCha8Rng {
    // SplitMix64 finaliser so neighbouring ids give unrelated streams.
    let mut z = seed ^ (u64::from(id)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    ChaCha8Rng::seed_from_u64(z)
}

fn unit_vector(rng: &mut ChaCha8Rng) -> [f32; 3] {
    let z: f32 = rng.gen_range(-1.0..=1.0);
    let phi: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
    let ring = (1.0 - z * z).max(0.0).sqrt();
    [ring * phi.cos(), ring * phi.sin(), z]
}

fn in_ball(rng: &mut ChaCha8Rng, radius: f32) -> [f32; 3] {
    let u: f32 = rng.gen_range(0.0..1.0);
    scale(unit_vector(rng), radius * u.cbrt())
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_graphs::{repo_graph, Repo};

    fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn seed_is_deterministic_and_order_independent() {
        let g = repo_graph(&Repo {
            dirs: 4,
            files_per_dir: 5,
            symbols_per_file: 6,
            calls_per_symbol: 1,
            seed: 3,
        });
        let p = LayoutParams::default();
        let a = hierarchical_seed(&g, &p);
        let b = hierarchical_seed(&g, &p);
        assert_eq!(a, b, "same seed, same positions");

        let mut shuffled = g.clone();
        shuffled.nodes.reverse();
        let c = hierarchical_seed(&shuffled, &p);
        for (i, id) in c.ids.iter().enumerate() {
            assert_eq!(
                Some(c.pos[i]),
                a.position_of(*id),
                "position of {id} must not depend on node order"
            );
        }

        let other = hierarchical_seed(&g, &LayoutParams { seed: 99, ..p });
        assert_ne!(a.pos, other.pos, "a different seed must move nodes");
    }

    #[test]
    fn hierarchy_is_nested() {
        let g = repo_graph(&Repo {
            dirs: 6,
            files_per_dir: 8,
            symbols_per_file: 5,
            calls_per_symbol: 0,
            seed: 1,
        });
        let p = LayoutParams::default();
        let s = hierarchical_seed(&g, &p);
        let idx = g.index();
        for (i, n) in g.nodes.iter().enumerate() {
            match n.tier {
                Tier::L0 => {
                    let r = dist(s.pos[i], [0.0; 3]);
                    assert!(
                        (r - p.shell_radius).abs() < 1e-2,
                        "directory {} at radius {r}",
                        n.id
                    );
                }
                Tier::L1 => {
                    let d = dist(s.pos[i], s.pos[idx[&n.parent]]);
                    assert!(
                        d <= DIR_BALL * p.shell_radius * 2.0 + 1e-3,
                        "file {} is {d} from its directory",
                        n.id
                    );
                }
                Tier::L2 => {
                    let d = dist(s.pos[i], s.pos[idx[&n.parent]]);
                    assert!(
                        d <= FILE_BALL * p.shell_radius * 5f32.cbrt() + 1e-3,
                        "symbol {} is {d} from its file",
                        n.id
                    );
                }
            }
        }
    }

    #[test]
    fn fibonacci_points_are_unit_and_spread() {
        let m = 50;
        let pts: Vec<_> = (0..m).map(|k| fibonacci_sphere(k, m)).collect();
        for p in &pts {
            assert!((dist(*p, [0.0; 3]) - 1.0).abs() < 1e-5);
        }
        let mut min = f32::MAX;
        for i in 0..m {
            for j in i + 1..m {
                min = min.min(dist(pts[i], pts[j]));
            }
        }
        // Ideal spacing for 50 points is about sqrt(4π/50) ≈ 0.5.
        assert!(min > 0.3, "points bunch up: min distance {min}");
    }

    #[test]
    fn known_positions_are_kept_and_children_follow_parents() {
        let g = repo_graph(&Repo {
            dirs: 2,
            files_per_dir: 2,
            symbols_per_file: 3,
            calls_per_symbol: 0,
            seed: 5,
        });
        let p = LayoutParams::default();
        let file = g.nodes.iter().find(|n| n.tier == Tier::L1).unwrap();
        let mut known = HashMap::new();
        known.insert(file.id, [5000.0, 0.0, 0.0]);
        let pos = place(&g, &p, &known, true);
        let idx = g.index();
        assert_eq!(pos[idx[&file.id]], [5000.0, 0.0, 0.0]);
        for n in g.nodes.iter().filter(|n| n.parent == file.id) {
            let d = dist(pos[idx[&n.id]], [5000.0, 0.0, 0.0]);
            assert!(
                d <= NEW_UNDER_FILE * p.shell_radius + 1e-3,
                "new symbol {d} from its moved file"
            );
        }
    }
}
