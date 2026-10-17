//! The physical model both engines simulate: bodies, aggregates, springs and
//! the host-side force terms (containment, aggregate edges, community cohesion).
//!
//! The CPU and CUDA engines differ only in repulsion and integration. Every
//! term computed here is shared code, so on those terms the two engines agree
//! bit for bit.

use std::collections::HashMap;

use codescape_model::{GraphSnapshot, Relation, Tier};
use rayon::prelude::*;

use crate::LayoutParams;

/// Community cohesion stiffness per unit of [`LayoutParams::gravity`]. Each body
/// is pulled towards its community's centroid with stiffness
/// `gravity × COMMUNITY_COHESION_PER_GRAVITY`.
pub const COMMUNITY_COHESION_PER_GRAVITY: f32 = 4.0;

pub(crate) const NONE: u32 = u32::MAX;
/// Minimum bodies per rayon task in the O(n) host passes. Finer splitting on a
/// loaded 56-thread host cost 2.4 ms where one thread took 0.66 ms (28k bodies).
pub(crate) const BODY_CHUNK: usize = 8192;
/// Minimum far-field receivers per rayon task (each costs O(sources)).
#[cfg(feature = "cuda")]
const FAR_FIELD_CHUNK: usize = 16;
/// Accumulator lanes in the far-field inner loop.
#[cfg(feature = "cuda")]
const LANES: usize = 8;
/// Bound on parent-chain walks, so a malformed cycle cannot hang a step.
const MAX_CHAIN: usize = 32;

/// Far-field sources from [`Model::far_sources`]: files first, then orphans.
#[cfg(feature = "cuda")]
#[derive(Debug, Clone, Default)]
pub(crate) struct FarSources {
    points: Vec<[f32; 3]>,
    masses: Vec<f32>,
    n_files: usize,
}

/// The host far field for the CUDA engine; see [`far_field`].
#[cfg(feature = "cuda")]
#[derive(Debug, Clone, Default)]
pub(crate) struct FarField {
    /// Field at each file's centroid, indexed like the model's file groups.
    files: Vec<[f32; 3]>,
    /// Field on each orphan body, indexed like the model's orphans.
    orphans: Vec<[f32; 3]>,
}

/// The long-range repulsion a cut-off grid misses, for the CUDA engine.
///
/// The orphans are binned into cut-off-sized cells, and each occupied cell
/// acts as one source at its orphans' centroid with their count. On one
/// VisionClaw ingest this cuts 4,143 orphans to about 990 cells beside 1,558
/// files. A source whose centre is at least `cutoff` from a receiver's centre
/// pushes it with `law(d) × mass`; closer pairs are left to the device grid.
/// Each file receives the field at its centroid and passes it to every member;
/// an orphan receives its cell's field. Cost: (files + cells)² pairs.
///
/// On the 2k-symbol conformance graph (CPU at θ = 0.8), this hard centroid test
/// agreed with the CPU oracle better (k-NN 0.83 symbols / 0.99 files) than
/// ramps that fade a file in over its radius (0.78 / 0.98 with RMS radii,
/// 0.57 / 0.95 with max radii). On a 20k-symbol graph the early energy decay
/// was within 3% of exact repulsion. A Barnes-Hut walk with the cut-off
/// excluded was slower than this brute force: every cell straddling the
/// cut-off sphere has to be opened.
#[cfg(feature = "cuda")]
pub(crate) fn far_field(src: &FarSources, cutoff: f32, law: crate::octree::Repulsion) -> FarField {
    let nf = src.n_files.min(src.points.len());
    // Orphan cells, numbered in order of first appearance (deterministic).
    let inv = 1.0 / cutoff.max(1e-3);
    let mut cell_of: HashMap<[i32; 3], u32> = HashMap::new();
    let mut orphan_cell = Vec::with_capacity(src.points.len() - nf);
    let mut cells: Vec<([f64; 3], f64)> = Vec::new();
    for (p, &m) in src.points[nf..].iter().zip(&src.masses[nf..]) {
        let key = p.map(|x| (x * inv).floor() as i32);
        let c = *cell_of.entry(key).or_insert_with(|| {
            cells.push(([0.0; 3], 0.0));
            (cells.len() - 1) as u32
        });
        let e = &mut cells[c as usize];
        for k in 0..3 {
            e.0[k] += f64::from(p[k]) * f64::from(m);
        }
        e.1 += f64::from(m);
        orphan_cell.push(c);
    }

    // Structure of arrays, padded to whole lanes with zero-mass sources, so
    // the inner loop is a fixed-width body. Each lane keeps its own float
    // accumulator, since sums are not reassociated without fast-math.
    let n_src = nf + cells.len();
    let padded = n_src.div_ceil(LANES) * LANES;
    let (mut xs, mut ys, mut zs, mut ms) = (
        Vec::with_capacity(padded),
        Vec::with_capacity(padded),
        Vec::with_capacity(padded),
        Vec::with_capacity(padded),
    );
    for (p, &m) in src.points[..nf].iter().zip(&src.masses[..nf]) {
        xs.push(p[0]);
        ys.push(p[1]);
        zs.push(p[2]);
        ms.push(m);
    }
    for (sum, m) in &cells {
        xs.push((sum[0] / m) as f32);
        ys.push((sum[1] / m) as f32);
        zs.push((sum[2] / m) as f32);
        ms.push(*m as f32);
    }
    for v in [&mut xs, &mut ys, &mut zs, &mut ms] {
        v.resize(padded, 0.0);
    }
    let cut2 = cutoff * cutoff;
    // Beyond the cut-off the per-pair cap cannot bind once k / (cutoff² + ε) ≤
    // cap, so the force is k·m / ((d² + ε)·d): one division and one square root
    // per pair. Otherwise apply the cap.
    let (k, eps, cap) = (law.k, law.eps, law.max_pair);
    let capped = k / (cut2 + eps) > cap;
    let one = |me: usize| -> [f32; 3] {
        let (px, py, pz) = (xs[me], ys[me], zs[me]);
        let (mut fx, mut fy, mut fz) = ([0.0f32; LANES], [0.0f32; LANES], [0.0f32; LANES]);
        for c in (0..padded).step_by(LANES) {
            let (x, y, z, m) = (
                &xs[c..c + LANES],
                &ys[c..c + LANES],
                &zs[c..c + LANES],
                &ms[c..c + LANES],
            );
            for l in 0..LANES {
                let (dx, dy, dz) = (px - x[l], py - y[l], pz - z[l]);
                let d2 = dx * dx + dy * dy + dz * dz;
                // Sources inside the cut-off, `me` among them, weigh 0.
                let far = if d2 >= cut2 && d2 > 1e-6 { m[l] } else { 0.0 };
                let d = d2.max(1e-6).sqrt();
                let s = if capped {
                    (k / (d2 + eps)).min(cap) * far / d
                } else {
                    k * far / ((d2 + eps) * d)
                };
                fx[l] += dx * s;
                fy[l] += dy * s;
                fz[l] += dz * s;
            }
        }
        [fx.iter().sum(), fy.iter().sum(), fz.iter().sum()]
    };
    let all: Vec<[f32; 3]> = (0..n_src)
        .into_par_iter()
        .with_min_len(FAR_FIELD_CHUNK)
        .map(one)
        .collect();
    FarField {
        files: all[..nf].to_vec(),
        orphans: orphan_cell.iter().map(|&c| all[nf + c as usize]).collect(),
    }
}

/// One end of an edge that touches an aggregate.
#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    Body(u32),
    Agg(u32),
}

#[derive(Debug, Clone, Copy)]
struct AggEdge {
    u: End,
    v: End,
    /// Full stiffness, `spring_k × weight × multiplier`.
    k: f32,
}

/// A loaded graph, compiled into the index arrays the engines iterate.
#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// Node ids in snapshot order.
    pub ids: Vec<u32>,
    /// Node id → snapshot index.
    pub index: HashMap<u32, usize>,
    /// Body → node index.
    pub body_node: Vec<u32>,
    /// Node index → body, or [`NONE`].
    pub node_body: Vec<u32>,
    /// Node index → aggregate slot, or [`NONE`].
    node_agg: Vec<u32>,
    /// Aggregate slot → node index.
    agg_node: Vec<u32>,
    /// `(child node, parent aggregate)` for L2 children, then for L1 children:
    /// the two passes of `lod::aggregate_positions`.
    pass_l2: Vec<(u32, u32)>,
    pass_l1: Vec<(u32, u32)>,
    /// Body → its parent aggregate, or [`NONE`].
    pub body_group: Vec<u32>,
    /// Aggregate → its parent aggregate, or [`NONE`].
    agg_parent: Vec<u32>,
    /// Aggregates ordered ancestors first.
    agg_order: Vec<u32>,
    /// Aggregate → number of member bodies at any depth.
    agg_desc: Vec<u32>,
    /// Symmetric CSR over bodies. Weights exclude `spring_k`.
    pub row: Vec<u32>,
    pub col: Vec<u32>,
    pub weight: Vec<f32>,
    agg_edges: Vec<AggEdge>,
    /// Body → dense community index, or -1.
    pub body_comm: Vec<i32>,
    pub n_comm: usize,
    /// Aggregates that are some body's immediate parent, and their member counts.
    #[cfg(feature = "cuda")]
    groups: Vec<u32>,
    #[cfg(feature = "cuda")]
    group_size: Vec<u32>,
    /// Bodies with no aggregate parent.
    #[cfg(feature = "cuda")]
    orphans: Vec<u32>,
    /// Aggregate → its index in `groups`, or [`NONE`].
    #[cfg(feature = "cuda")]
    group_slot: Vec<u32>,
    k_contain: f32,
    k_comm: f32,
}

/// The spring multiplier for an edge, before `spring_k`.
pub(crate) fn edge_multiplier(params: &LayoutParams, relation: &Relation, cu: u32, cv: u32) -> f32 {
    if relation.is_structural() {
        params.structural_weight
    } else if cu != 0 && cu == cv {
        params.intra_community_weight
    } else {
        params.inter_community_weight
    }
}

fn sanitize_weight(w: f32) -> f32 {
    if w.is_finite() {
        w.max(0.0)
    } else {
        1.0
    }
}

impl Model {
    /// Compile `snapshot`. The snapshot must already pass `validate()`.
    pub fn build(snapshot: &GraphSnapshot, params: &LayoutParams) -> Self {
        let n = snapshot.nodes.len();
        let index = snapshot.index();
        let parent_idx: Vec<u32> = snapshot
            .nodes
            .iter()
            .enumerate()
            .map(|(i, nd)| match index.get(&nd.parent) {
                Some(&p) if nd.parent != 0 && p != i => p as u32,
                _ => NONE,
            })
            .collect();

        // An aggregate is a non-L2 node that lod::aggregate_positions overwrites:
        // the parent of some L2 or L1 node.
        let mut is_agg = vec![false; n];
        for (i, nd) in snapshot.nodes.iter().enumerate() {
            let p = parent_idx[i];
            if p != NONE
                && matches!(nd.tier, Tier::L2 | Tier::L1)
                && snapshot.nodes[p as usize].tier != Tier::L2
            {
                is_agg[p as usize] = true;
            }
        }
        let mut node_body = vec![NONE; n];
        let mut node_agg = vec![NONE; n];
        let mut body_node = Vec::new();
        let mut agg_node = Vec::new();
        for i in 0..n {
            if is_agg[i] {
                node_agg[i] = agg_node.len() as u32;
                agg_node.push(i as u32);
            } else {
                node_body[i] = body_node.len() as u32;
                body_node.push(i as u32);
            }
        }

        let mut pass_l2 = Vec::new();
        let mut pass_l1 = Vec::new();
        for (i, nd) in snapshot.nodes.iter().enumerate() {
            let p = parent_idx[i];
            if p == NONE || node_agg[p as usize] == NONE {
                continue;
            }
            match nd.tier {
                Tier::L2 => pass_l2.push((i as u32, node_agg[p as usize])),
                Tier::L1 => pass_l1.push((i as u32, node_agg[p as usize])),
                Tier::L0 => {}
            }
        }

        let agg_parent: Vec<u32> = agg_node
            .iter()
            .map(|&ni| {
                let p = parent_idx[ni as usize];
                if p == NONE {
                    NONE
                } else {
                    node_agg[p as usize]
                }
            })
            .collect();
        let body_group: Vec<u32> = body_node
            .iter()
            .map(|&ni| {
                let p = parent_idx[ni as usize];
                if p == NONE {
                    NONE
                } else {
                    node_agg[p as usize]
                }
            })
            .collect();

        let chain = |start: u32| -> Vec<u32> {
            let mut out = Vec::new();
            let mut a = start;
            while a != NONE && out.len() < MAX_CHAIN && !out.contains(&a) {
                out.push(a);
                a = agg_parent[a as usize];
            }
            out
        };

        let mut agg_desc = vec![0u32; agg_node.len()];
        for &g in &body_group {
            for a in chain(g) {
                agg_desc[a as usize] += 1;
            }
        }
        let mut agg_depth: Vec<(usize, u32)> = (0..agg_node.len() as u32)
            .map(|a| (chain(a).len(), a))
            .collect();
        agg_depth.sort_unstable();
        let agg_order = agg_depth.into_iter().map(|(_, a)| a).collect();

        // Communities: dense, in ascending label order for determinism.
        let mut labels: Vec<u32> = body_node
            .iter()
            .map(|&ni| snapshot.nodes[ni as usize].community)
            .filter(|&c| c != 0)
            .collect();
        labels.sort_unstable();
        labels.dedup();
        let dense: HashMap<u32, i32> = labels
            .iter()
            .enumerate()
            .map(|(k, &c)| (c, k as i32))
            .collect();
        let body_comm = body_node
            .iter()
            .map(|&ni| {
                dense
                    .get(&snapshot.nodes[ni as usize].community)
                    .copied()
                    .unwrap_or(-1)
            })
            .collect();

        // Springs.
        let mut pairs: Vec<(u32, u32, f32)> = Vec::with_capacity(snapshot.edges.len() * 2);
        let mut agg_edges = Vec::new();
        for e in &snapshot.edges {
            let (Some(&u), Some(&v)) = (index.get(&e.source), index.get(&e.target)) else {
                continue;
            };
            if u == v {
                continue;
            }
            let cu = snapshot.nodes[u].community;
            let cv = snapshot.nodes[v].community;
            let w = sanitize_weight(e.weight) * edge_multiplier(params, &e.relation, cu, cv);
            if w <= 0.0 {
                continue;
            }
            let end = |i: usize| {
                if node_body[i] != NONE {
                    End::Body(node_body[i])
                } else {
                    End::Agg(node_agg[i])
                }
            };
            match (end(u), end(v)) {
                (End::Body(a), End::Body(b)) => {
                    pairs.push((a, b, w));
                    pairs.push((b, a, w));
                }
                (End::Body(b), End::Agg(a)) | (End::Agg(a), End::Body(b)) => {
                    // Body ↔ one of its own containers: the containment term covers it.
                    if chain(body_group[b as usize]).contains(&a) {
                        continue;
                    }
                    agg_edges.push(AggEdge {
                        u: end(u),
                        v: end(v),
                        k: params.spring_k * w,
                    });
                }
                (eu, ev) => agg_edges.push(AggEdge {
                    u: eu,
                    v: ev,
                    k: params.spring_k * w,
                }),
            }
        }
        pairs.sort_unstable_by_key(|&(a, b, _)| (a, b));
        let nb = body_node.len();
        let mut row = vec![0u32; nb + 1];
        let mut col = Vec::with_capacity(pairs.len());
        let mut weight: Vec<f32> = Vec::with_capacity(pairs.len());
        let mut last: Option<(u32, u32)> = None;
        for (a, b, w) in pairs {
            if last == Some((a, b)) {
                *weight.last_mut().expect("merged pair has a predecessor") += w;
                continue;
            }
            last = Some((a, b));
            row[a as usize + 1] += 1;
            col.push(b);
            weight.push(w);
        }
        for i in 0..nb {
            row[i + 1] += row[i];
        }

        #[cfg(feature = "cuda")]
        let (groups, group_size, orphans, group_slot) = {
            let mut size_of_agg = vec![0u32; agg_node.len()];
            for &g in &body_group {
                if g != NONE {
                    size_of_agg[g as usize] += 1;
                }
            }
            let groups: Vec<u32> = (0..agg_node.len() as u32)
                .filter(|&a| size_of_agg[a as usize] > 0)
                .collect();
            let group_size = groups.iter().map(|&a| size_of_agg[a as usize]).collect();
            let orphans = body_group
                .iter()
                .enumerate()
                .filter(|(_, &g)| g == NONE)
                .map(|(b, _)| b as u32)
                .collect();
            let mut group_slot = vec![NONE; agg_node.len()];
            for (gi, &a) in groups.iter().enumerate() {
                group_slot[a as usize] = gi as u32;
            }
            (groups, group_size, orphans, group_slot)
        };

        Model {
            ids: snapshot.nodes.iter().map(|n| n.id).collect(),
            index,
            body_node,
            node_body,
            node_agg,
            agg_node,
            pass_l2,
            pass_l1,
            body_group,
            agg_parent,
            agg_order,
            agg_desc,
            row,
            col,
            weight,
            agg_edges,
            body_comm,
            n_comm: labels.len(),
            #[cfg(feature = "cuda")]
            groups,
            #[cfg(feature = "cuda")]
            group_size,
            #[cfg(feature = "cuda")]
            orphans,
            #[cfg(feature = "cuda")]
            group_slot,
            k_contain: params.spring_k * params.structural_weight,
            k_comm: params.gravity * COMMUNITY_COHESION_PER_GRAVITY,
        }
    }

    /// Number of bodies.
    pub fn n_bodies(&self) -> usize {
        self.body_node.len()
    }

    /// Number of aggregates that directly hold bodies.
    #[cfg(test)]
    pub fn groups_len(&self) -> usize {
        self.agg_desc.iter().filter(|&&d| d > 0).count()
    }

    /// Number of nodes.
    pub fn n_nodes(&self) -> usize {
        self.ids.len()
    }

    /// Write body positions into `node_pos` and place every aggregate at the
    /// centroid of its members (L2 pass, then L1 pass), exactly as
    /// `lod::aggregate_positions` does except that a body is never overwritten.
    pub fn aggregate(&self, body_pos: &[[f32; 3]], node_pos: &mut [[f32; 3]]) {
        for (b, &ni) in self.body_node.iter().enumerate() {
            node_pos[ni as usize] = body_pos[b];
        }
        let mut sum = vec![([0.0f64; 3], 0u32); self.agg_node.len()];
        for pass in [&self.pass_l2, &self.pass_l1] {
            sum.iter_mut().for_each(|s| *s = ([0.0; 3], 0));
            for &(child, a) in pass.iter() {
                let p = node_pos[child as usize];
                let s = &mut sum[a as usize];
                for k in 0..3 {
                    s.0[k] += f64::from(p[k]);
                }
                s.1 += 1;
            }
            for (a, s) in sum.iter().enumerate() {
                if s.1 > 0 {
                    let c = f64::from(s.1);
                    node_pos[self.agg_node[a] as usize] = [
                        (s.0[0] / c) as f32,
                        (s.0[1] / c) as f32,
                        (s.0[2] / c) as f32,
                    ];
                }
            }
        }
    }

    /// Centroid of each community over all bodies.
    fn community_centroids(&self, body_pos: &[[f32; 3]]) -> Vec<[f32; 3]> {
        let mut sum = vec![([0.0f64; 3], 0u32); self.n_comm];
        for (b, &c) in self.body_comm.iter().enumerate() {
            if c >= 0 {
                let s = &mut sum[c as usize];
                for k in 0..3 {
                    s.0[k] += f64::from(body_pos[b][k]);
                }
                s.1 += 1;
            }
        }
        sum.into_iter()
            .map(|(s, c)| {
                let c = f64::from(c.max(1));
                [(s[0] / c) as f32, (s[1] / c) as f32, (s[2] / c) as f32]
            })
            .collect()
    }

    /// The host-side force on every body: containment, aggregate edges and
    /// community cohesion. `node_pos` must come from [`Model::aggregate`] on
    /// the same `body_pos`. `out` has one entry per body.
    pub fn host_forces(&self, body_pos: &[[f32; 3]], node_pos: &[[f32; 3]], out: &mut [[f32; 3]]) {
        let centroids = self.community_centroids(body_pos);
        let pos_of = |e: End| match e {
            End::Body(b) => body_pos[b as usize],
            End::Agg(a) => node_pos[self.agg_node[a as usize] as usize],
        };
        // Dense, not a map: a real repository has tens of thousands of
        // body ↔ aggregate edges, and hashing them dominated this pass.
        let mut direct = vec![
            [0.0f32; 3];
            if self.agg_edges.is_empty() {
                0
            } else {
                self.n_bodies()
            }
        ];
        let mut agg_force = vec![[0.0f32; 3]; self.agg_node.len()];
        for e in &self.agg_edges {
            let (pu, pv) = (pos_of(e.u), pos_of(e.v));
            let f = [
                e.k * (pv[0] - pu[0]),
                e.k * (pv[1] - pu[1]),
                e.k * (pv[2] - pu[2]),
            ];
            for (end, sign) in [(e.u, 1.0f32), (e.v, -1.0f32)] {
                let slot = match end {
                    End::Body(b) => &mut direct[b as usize],
                    End::Agg(a) => &mut agg_force[a as usize],
                };
                for k in 0..3 {
                    slot[k] += sign * f[k];
                }
            }
        }
        // An aggregate's force is shared by its member bodies; a body feels the
        // shares of every container above it.
        let mut offset = vec![[0.0f32; 3]; self.agg_node.len()];
        for &a in &self.agg_order {
            let ai = a as usize;
            let share = 1.0 / self.agg_desc[ai].max(1) as f32;
            let up = match self.agg_parent[ai] {
                NONE => [0.0; 3],
                p => offset[p as usize],
            };
            offset[ai] = [
                agg_force[ai][0] * share + up[0],
                agg_force[ai][1] * share + up[1],
                agg_force[ai][2] * share + up[2],
            ];
        }

        out.par_iter_mut()
            .enumerate()
            .with_min_len(BODY_CHUNK)
            .for_each(|(b, f)| {
                let p = body_pos[b];
                let mut acc = [0.0f32; 3];
                let g = self.body_group[b];
                if g != NONE {
                    let c = node_pos[self.agg_node[g as usize] as usize];
                    let o = offset[g as usize];
                    for k in 0..3 {
                        acc[k] += self.k_contain * (c[k] - p[k]) + o[k];
                    }
                }
                let c = self.body_comm[b];
                if c >= 0 {
                    let m = centroids[c as usize];
                    for k in 0..3 {
                        acc[k] += self.k_comm * (m[k] - p[k]);
                    }
                }
                if let Some(d) = direct.get(b) {
                    for k in 0..3 {
                        acc[k] += d[k];
                    }
                }
                *f = acc;
            });
    }

    /// The far-field sources at these positions: each file (any aggregate
    /// that directly holds bodies) at its centroid with its member count, then
    /// each orphan body (no aggregate parent) with mass 1. See [`far_field`].
    #[cfg(feature = "cuda")]
    pub fn far_sources(&self, body_pos: &[[f32; 3]], node_pos: &[[f32; 3]]) -> FarSources {
        let mut points = Vec::with_capacity(self.groups.len() + self.orphans.len());
        let mut masses = Vec::with_capacity(points.capacity());
        for (&a, &m) in self.groups.iter().zip(&self.group_size) {
            points.push(node_pos[self.agg_node[a as usize] as usize]);
            masses.push(m as f32);
        }
        points.extend(self.orphans.iter().map(|&b| body_pos[b as usize]));
        masses.resize(points.len(), 1.0);
        FarSources {
            points,
            masses,
            n_files: self.groups.len(),
        }
    }

    /// Add a field from [`far_field`] to every body.
    #[cfg(feature = "cuda")]
    pub fn apply_far_field(&self, field: &FarField, out: &mut [[f32; 3]]) {
        if field.files.len() != self.groups.len() || field.orphans.len() != self.orphans.len() {
            return;
        }
        for (b, o) in out.iter_mut().enumerate() {
            let g = self.body_group[b];
            if g != NONE {
                let f = field.files[self.group_slot[g as usize] as usize];
                for k in 0..3 {
                    o[k] += f[k];
                }
            }
        }
        for (&b, f) in self.orphans.iter().zip(&field.orphans) {
            for k in 0..3 {
                out[b as usize][k] += f[k];
            }
        }
    }

    /// Bodies to integrate under a focus on `ids`: each body id, every member
    /// body of each aggregate id, and their 1-hop spring neighbours. Sorted.
    pub fn focus_bodies(&self, ids: &[u32]) -> Vec<u32> {
        let mut seed = vec![false; self.n_bodies()];
        let mut aggs = vec![false; self.agg_node.len()];
        for id in ids {
            let Some(&ni) = self.index.get(id) else {
                continue;
            };
            if self.node_body[ni] != NONE {
                seed[self.node_body[ni] as usize] = true;
            } else {
                aggs[self.node_agg[ni] as usize] = true;
            }
        }
        if aggs.iter().any(|&a| a) {
            for (b, &g) in self.body_group.iter().enumerate() {
                let mut a = g;
                let mut hops = 0;
                while a != NONE && hops < MAX_CHAIN {
                    if aggs[a as usize] {
                        seed[b] = true;
                        break;
                    }
                    a = self.agg_parent[a as usize];
                    hops += 1;
                }
            }
        }
        let mut active = seed.clone();
        for (b, &s) in seed.iter().enumerate() {
            if s {
                for k in self.row[b]..self.row[b + 1] {
                    active[self.col[k as usize] as usize] = true;
                }
            }
        }
        active
            .iter()
            .enumerate()
            .filter(|(_, &a)| a)
            .map(|(b, _)| b as u32)
            .collect()
    }
}

/// Synthetic graphs for tests and benchmarks.
#[cfg(test)]
pub(crate) mod test_graphs {
    use std::sync::Arc;

    use codescape_model::lod::build_tiers;
    use codescape_model::{
        CodeEdge, CodeNode, Confidence, GraphSnapshot, IdInterner, Lang, NodeKind, Relation, Tier,
    };
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    /// Shape of a synthetic repository.
    pub struct Repo {
        pub dirs: usize,
        pub files_per_dir: usize,
        pub symbols_per_file: usize,
        /// Random `calls` edges per symbol: 70% stay inside the directory.
        pub calls_per_symbol: usize,
        pub seed: u64,
    }

    pub fn node(id: u32, key: &str, kind: NodeKind, path: Option<&str>) -> CodeNode {
        CodeNode {
            id,
            key: Arc::from(key),
            kind,
            lang: Lang::Rust,
            label: Arc::from(key),
            path: path.map(Arc::from),
            span: None,
            parent: 0,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier: Tier::L2,
            size: 0,
        }
    }

    pub fn edge(s: u32, t: u32, relation: Relation) -> CodeEdge {
        CodeEdge {
            source: s,
            target: t,
            relation,
            weight: 1.0,
            confidence: Confidence::Extracted,
        }
    }

    /// A tiered repository: directories → files → symbols, with contains edges
    /// from `build_tiers`, one community per directory and random calls.
    pub fn repo_graph(r: &Repo) -> GraphSnapshot {
        let mut it = IdInterner::new(1);
        let mut g = GraphSnapshot {
            revision: 1,
            ..Default::default()
        };
        let mut rng = ChaCha8Rng::seed_from_u64(r.seed);
        let mut symbols_by_dir: Vec<Vec<u32>> = vec![Vec::new(); r.dirs];
        for d in 0..r.dirs {
            for f in 0..r.files_per_dir {
                let path = format!("d{d}/f{f}.rs");
                let fid = it.intern(&path).unwrap();
                g.nodes.push(node(fid, &path, NodeKind::File, Some(&path)));
                for s in 0..r.symbols_per_file {
                    let key = format!("{path}::s{s}");
                    let sid = it.intern(&key).unwrap();
                    let mut n = node(sid, &key, NodeKind::Function, Some(&path));
                    n.community = d as u32 + 1;
                    g.nodes.push(n);
                    symbols_by_dir[d].push(sid);
                }
            }
        }
        build_tiers(&mut g, &mut it).unwrap();
        let all: Vec<u32> = symbols_by_dir.iter().flatten().copied().collect();
        for (d, syms) in symbols_by_dir.iter().enumerate() {
            for &s in syms {
                for _ in 0..r.calls_per_symbol {
                    let t = if rng.gen_bool(0.7) {
                        syms[rng.gen_range(0..syms.len())]
                    } else {
                        all[rng.gen_range(0..all.len())]
                    };
                    if t != s {
                        g.edges.push(edge(s, t, Relation::Calls));
                    }
                }
                let _ = d;
            }
        }
        g.validate().unwrap();
        g
    }

    /// `g` plus one new symbol in its first file, as the post-delta snapshot and the delta.
    pub fn one_symbol_delta(g: &GraphSnapshot) -> (GraphSnapshot, codescape_model::GraphDelta) {
        let mut after = g.clone();
        after.revision += 1;
        let file = after
            .nodes
            .iter()
            .find(|n| n.tier == Tier::L1)
            .expect("a file")
            .clone();
        let id = after.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let mut n = node(
            id,
            "delta::new_symbol",
            NodeKind::Function,
            file.path.as_deref(),
        );
        n.parent = file.id;
        after.nodes.push(n);
        after.edges.push(edge(file.id, id, Relation::Contains));
        let delta = codescape_model::GraphDelta::diff(g, &after);
        (after, delta)
    }

    /// Two dense cliques of `size` symbols each (communities 1 and 2), joined by one edge.
    pub fn two_cliques(size: u32) -> GraphSnapshot {
        let mut g = GraphSnapshot {
            revision: 1,
            ..Default::default()
        };
        for c in 0..2u32 {
            for i in 0..size {
                let id = 1 + c * size + i;
                let mut n = node(id, &format!("c{c}s{i}"), NodeKind::Function, None);
                n.community = c + 1;
                g.nodes.push(n);
            }
            for i in 0..size {
                for j in i + 1..size {
                    g.edges
                        .push(edge(1 + c * size + i, 1 + c * size + j, Relation::Calls));
                }
            }
        }
        g.edges.push(edge(1, 1 + size, Relation::Calls));
        g.validate().unwrap();
        g
    }

    /// A hub symbol with `leaves` leaf symbols, all in one community.
    pub fn star(leaves: u32) -> GraphSnapshot {
        let mut g = GraphSnapshot {
            revision: 1,
            ..Default::default()
        };
        g.nodes.push(node(1, "hub", NodeKind::Function, None));
        for i in 0..leaves {
            g.nodes
                .push(node(2 + i, &format!("leaf{i}"), NodeKind::Function, None));
            g.edges.push(edge(1, 2 + i, Relation::Calls));
        }
        g.validate().unwrap();
        g
    }
}

#[cfg(test)]
mod tests {
    use super::test_graphs::*;
    use super::*;
    use codescape_model::lod::aggregate_positions;
    use codescape_model::NodeKind;

    #[test]
    fn bodies_and_aggregates_are_classified() {
        let mut g = repo_graph(&Repo {
            dirs: 2,
            files_per_dir: 2,
            symbols_per_file: 2,
            calls_per_symbol: 0,
            seed: 1,
        });
        // A file with no symbols is a body, not an aggregate.
        let empty = node(9_000, "d0/empty.rs", NodeKind::File, Some("d0/empty.rs"));
        let dir = g.nodes.iter().find(|n| n.tier == Tier::L0).unwrap().id;
        g.nodes.push(codescape_model::CodeNode {
            parent: dir,
            tier: Tier::L1,
            ..empty
        });
        let m = Model::build(&g, &LayoutParams::default());
        assert_eq!(m.n_bodies(), 8 + 1, "8 symbols and the empty file");
        assert_eq!(
            m.agg_node.len(),
            4 + 2,
            "4 files with symbols and 2 directories"
        );
        let empty_idx = m.index[&9_000];
        assert_ne!(m.node_body[empty_idx], NONE);
    }

    #[test]
    fn aggregate_matches_lod_aggregate_positions() {
        let g = repo_graph(&Repo {
            dirs: 3,
            files_per_dir: 4,
            symbols_per_file: 5,
            calls_per_symbol: 2,
            seed: 7,
        });
        let m = Model::build(&g, &LayoutParams::default());
        let seeded = crate::hierarchical_seed(&g, &LayoutParams::default());
        let body_pos: Vec<[f32; 3]> = m
            .body_node
            .iter()
            .map(|&ni| seeded.pos[ni as usize])
            .collect();
        let mut ours = seeded.pos.clone();
        m.aggregate(&body_pos, &mut ours);
        let mut theirs = seeded.pos.clone();
        aggregate_positions(&g, &mut theirs);
        for (a, b) in ours.iter().zip(&theirs) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-3, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn csr_is_symmetric_merged_and_weighted() {
        let mut g = two_cliques(3);
        g.edges.push(edge(1, 2, Relation::Calls)); // duplicate of a clique edge
        g.edges.push(edge(2, 3, Relation::Contains)); // structural on top
        let p = LayoutParams::default();
        let m = Model::build(&g, &p);
        let w = |a: u32, b: u32| {
            let (a, b) = (m.node_body[m.index[&a]], m.node_body[m.index[&b]]);
            (m.row[a as usize]..m.row[a as usize + 1])
                .find(|&k| m.col[k as usize] == b)
                .map(|k| m.weight[k as usize])
        };
        assert_eq!(
            w(1, 2),
            Some(2.0 * p.intra_community_weight),
            "duplicates merge"
        );
        assert_eq!(w(2, 1), w(1, 2), "symmetric");
        assert_eq!(
            w(2, 3),
            Some(p.intra_community_weight + p.structural_weight)
        );
        assert_eq!(
            w(1, 4),
            Some(p.inter_community_weight),
            "the bridge is inter-community"
        );
        assert_eq!(w(1, 5), None);
    }

    #[test]
    fn host_forces_pull_members_to_their_file_and_balance() {
        let g = repo_graph(&Repo {
            dirs: 2,
            files_per_dir: 3,
            symbols_per_file: 4,
            calls_per_symbol: 0,
            seed: 2,
        });
        let p = LayoutParams {
            gravity: 0.0,
            ..LayoutParams::default()
        };
        let m = Model::build(&g, &p);
        let seeded = crate::hierarchical_seed(&g, &p);
        let body_pos: Vec<[f32; 3]> = m
            .body_node
            .iter()
            .map(|&ni| seeded.pos[ni as usize])
            .collect();
        let mut node_pos = seeded.pos.clone();
        m.aggregate(&body_pos, &mut node_pos);
        let mut f = vec![[0.0; 3]; m.n_bodies()];
        m.host_forces(&body_pos, &node_pos, &mut f);
        // Containment, aggregate springs and (zero) cohesion are all internal
        // forces, so they sum to zero over the bodies.
        let mut total = [0.0f64; 3];
        for v in &f {
            for k in 0..3 {
                total[k] += f64::from(v[k]);
            }
        }
        for t in total {
            assert!(t.abs() < 1e-2, "net host force {total:?}");
        }
        // Inside one file every member shares the aggregate-edge offset, so a
        // member's deviation from the file's mean force is the containment pull.
        let mut mean: HashMap<u32, ([f32; 3], f32)> = HashMap::new();
        for (b, v) in f.iter().enumerate() {
            let e = mean.entry(m.body_group[b]).or_insert(([0.0; 3], 0.0));
            for k in 0..3 {
                e.0[k] += v[k];
            }
            e.1 += 1.0;
        }
        for (b, v) in f.iter().enumerate() {
            let g = m.body_group[b];
            let (sum, cnt) = mean[&g];
            let c = node_pos[m.agg_node[g as usize] as usize];
            for k in 0..3 {
                let want = m.k_contain * (c[k] - body_pos[b][k]);
                let got = v[k] - sum[k] / cnt;
                assert!(
                    (want - got).abs() < 1e-3,
                    "body {b}: containment {got} vs {want}"
                );
            }
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn far_field_is_the_monopole_sum_beyond_the_cutoff() {
        use crate::octree::Repulsion;
        let g = repo_graph(&Repo {
            dirs: 1,
            files_per_dir: 2,
            symbols_per_file: 3,
            calls_per_symbol: 0,
            seed: 3,
        });
        let m = Model::build(&g, &LayoutParams::default());
        // Put each file's symbols in a tight cluster, the two files 100 apart.
        let body_pos: Vec<[f32; 3]> = (0..m.n_bodies())
            .map(|b| {
                let file = m.body_group[b];
                let x = if file == m.body_group[0] { 0.0 } else { 100.0 };
                [x, b as f32 * 0.01, 0.0]
            })
            .collect();
        let mut node_pos = vec![[0.0; 3]; m.n_nodes()];
        m.aggregate(&body_pos, &mut node_pos);
        let law = Repulsion {
            k: 50.0,
            eps: 1.0,
            max_pair: 1e9,
            theta2: 0.0,
        };
        let mut out = vec![[0.0; 3]; m.n_bodies()];
        let field = far_field(&m.far_sources(&body_pos, &node_pos), 10.0, law);
        m.apply_far_field(&field, &mut out);
        // Three unit masses 100 away: about 3 × 50 / (100² + 1), pointing away.
        let want = 3.0 * 50.0 / (100.0f32 * 100.0 + 1.0);
        for (b, f) in out.iter().enumerate() {
            let sign = if body_pos[b][0] < 50.0 { -1.0 } else { 1.0 };
            assert!((f[0] - sign * want).abs() < 1e-4, "body {b}: {f:?}");
        }
        // Within the cut-off the device grid owns repulsion: no far field.
        let mut none = vec![[0.0; 3]; m.n_bodies()];
        let field = far_field(&m.far_sources(&body_pos, &node_pos), 1000.0, law);
        m.apply_far_field(&field, &mut none);
        assert!(none.iter().all(|f| *f == [0.0; 3]));
    }

    #[test]
    #[ignore = "benchmark: run in release with --ignored --nocapture"]
    fn bench_host_terms() {
        for (dirs, files, syms, calls) in [(112usize, 10usize, 25usize, 2usize), (50, 40, 50, 3)] {
            let g = repo_graph(&Repo {
                dirs,
                files_per_dir: files,
                symbols_per_file: syms,
                calls_per_symbol: calls,
                seed: 21,
            });
            let p = LayoutParams::default();
            let m = Model::build(&g, &p);
            let seeded = crate::hierarchical_seed(&g, &p);
            let body_pos: Vec<[f32; 3]> = m
                .body_node
                .iter()
                .map(|&ni| seeded.pos[ni as usize])
                .collect();
            let mut node_pos = seeded.pos.clone();
            let mut f = vec![[0.0; 3]; m.n_bodies()];
            let time = |mut work: Box<dyn FnMut() + '_>| {
                let t = std::time::Instant::now();
                for _ in 0..50 {
                    work();
                }
                t.elapsed().as_secs_f64() * 20.0
            };
            let agg = time(Box::new(|| m.aggregate(&body_pos, &mut node_pos)));
            let host = time(Box::new(|| m.host_forces(&body_pos, &node_pos, &mut f)));
            #[cfg(feature = "cuda")]
            let far = {
                let law = crate::octree::Repulsion {
                    k: p.repulsion,
                    eps: 1.0,
                    max_pair: p.max_force(),
                    theta2: 0.0,
                };
                time(Box::new(|| {
                    drop(far_field(&m.far_sources(&body_pos, &node_pos), 200.0, law))
                }))
            };
            #[cfg(not(feature = "cuda"))]
            let far = f64::NAN;
            println!(
                "host terms at {} bodies / {} files: aggregate {agg:.2} ms, host_forces {host:.2} ms, far_field {far:.2} ms",
                m.n_bodies(),
                m.groups_len()
            );
        }
    }

    #[test]
    fn focus_expands_aggregates_and_one_hop() {
        let g = star(4);
        let m = Model::build(&g, &LayoutParams::default());
        let leaf = m.node_body[m.index[&3]];
        let hub = m.node_body[m.index[&1]];
        let mut want = vec![leaf, hub];
        want.sort_unstable();
        assert_eq!(m.focus_bodies(&[3]), want);

        let g = repo_graph(&Repo {
            dirs: 2,
            files_per_dir: 2,
            symbols_per_file: 3,
            calls_per_symbol: 0,
            seed: 4,
        });
        let m = Model::build(&g, &LayoutParams::default());
        let file = g.nodes.iter().find(|n| n.tier == Tier::L1).unwrap();
        let members: Vec<u32> = g
            .nodes
            .iter()
            .filter(|n| n.parent == file.id)
            .map(|n| m.node_body[m.index[&n.id]])
            .collect();
        let got = m.focus_bodies(&[file.id]);
        assert_eq!(got.len(), members.len());
        assert!(members.iter().all(|b| got.contains(b)));
    }
}
