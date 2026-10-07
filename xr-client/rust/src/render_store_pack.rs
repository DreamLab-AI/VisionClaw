//! Per-frame pack plans for the node and edge MultiMesh buffers (CPU budget).
//!
//! At production density (13 164 nodes, 20 000 edges) the full packs cost
//! 5–8 ms p99 on HP's desktop CPU, about a dozen hash lookups per node and per
//! edge, every frame, although only positions change from one frame to the next.
//! Quest's XR2 Gen 2 is roughly 3–4× slower per core, against an 8 ms CPU budget.
//!
//! A *plan* records, once, everything about a drawn instance except its
//! position: its store slot, packed colour and custom channels and its size at
//! unit scale (nodes), or its endpoint slots and style (edges). While the plan
//! is valid, a frame rewrites transforms from the current positions and nothing
//! else — no hashing, no allocation. The plan is derived from the output of the
//! full pack (`full_node_pack` / `RenderStore::pack_edges`), which remains the
//! single source of truth for how an instance looks.
//!
//! Validity: every mutation that can change an instance's look or the drawn set
//! bumps `RenderStore::visual_epoch` (`touch()`); per-frame feeds that usually
//! repeat (`upsert`'s analytics, `set_node_kind`, agent expiry) bump only on an
//! actual change. A plan is reused only when the epoch, the requested id/pair
//! list and the size/radius parameters are unchanged and no fold animation is
//! running. The existing render-store tests mutate and rebuild constantly, so a
//! missing `touch()` shows up there as a stale-buffer failure.
//!
//! Animated per-frame tints that must not invalidate the plan go through
//! [`RenderStore::live_tint`], applied on both the plan and the full path.

use std::collections::HashSet;

use super::{edge_transform12, RenderStore, EDGE_STRIDE_TYPED, NODE_STRIDE};

#[derive(Clone, Copy, Debug)]
pub(super) struct NodePlanEntry {
    id: u32,
    slot: u32,
    unit_size: f32,
    col: [f32; 4],
    custom: [f32; 4],
    faded: bool,
}

#[derive(Default)]
pub(super) struct NodePlan {
    valid: bool,
    epoch: u64,
    ids: Vec<i32>,
    size_lo: f32,
    size_hi: f32,
    entries: Vec<NodePlanEntry>,
    /// Per entry: in the gem tier at the last LOD build (hysteresis input).
    near: Vec<bool>,
    /// Bumped on every full node pack; the edge plan keys on it (drawn set).
    generation: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EdgePlanEntry {
    s: u32,
    t: u32,
    style: f32,
}

#[derive(Default)]
pub(super) struct EdgePlan {
    valid: bool,
    epoch: u64,
    node_generation: u64,
    radius: f32,
    pairs: Vec<i32>,
    entries: Vec<EdgePlanEntry>,
    near: Vec<bool>,
}

/// Reused per-frame buffers: the pack never allocates in steady state.
#[derive(Default)]
pub(super) struct PackScratch {
    pub(super) node_buf: Vec<f32>,
    pub(super) near_buf: Vec<f32>,
    pub(super) edge_near_buf: Vec<f32>,
    cand: Vec<(f32, usize)>,
    inst_prev: Vec<bool>,
    inst_near: Vec<bool>,
    inst_entry: Vec<u32>,
    /// Plan entry of each ribbon instance (valid between far-tier rebuilds).
    ribbon_entry: Vec<u32>,
    /// A ribbon collapsed during an in-place refresh: rebuild the far tier.
    ribbon_stale: bool,
}

#[inline]
fn push_node(buf: &mut Vec<f32>, size: f32, p: [f32; 3], col: &[f32; 4], custom: &[f32; 4]) {
    buf.extend_from_slice(&[size, 0.0, 0.0, p[0], 0.0, size, 0.0, p[1], 0.0, 0.0, size, p[2]]);
    buf.extend_from_slice(col);
    buf.extend_from_slice(custom);
}

impl RenderStore {
    /// Mark every cached plan stale. Called by each mutation that can change an
    /// instance's look or the drawn set.
    #[inline]
    pub(super) fn touch(&mut self) {
        self.visual_epoch = self.visual_epoch.wrapping_add(1);
    }

    /// Per-frame colour modulation that must not invalidate the plan (e.g. a
    /// time-decaying highlight). Applied identically on the plan and full paths.
    #[inline]
    fn live_tint(&self, id: u32, _slot: usize, col: &mut [f32; 4]) {
        // Attention heat (attention.rs, desktop attentionHeat.ts) brightens —
        // never recolours — a node agents are touching, and decays on the store
        // clock every frame.
        if self.heat.enabled() {
            self.heat.brighten(id, self.clock_ms, &mut col[..3]);
        }
    }

    fn fold_animating(&self) -> bool {
        !self.folding.is_empty() || !self.unfolding.is_empty()
    }

    /// Pack the node buffers for `ids` into `scratch.node_buf` (opaque) and
    /// `faded_buf` (labelled), via the plan when it is valid.
    pub(super) fn pack_nodes(&mut self, ids: &[i32], scale_comp: f32, size_lo: f32, size_hi: f32) {
        self.step_label_fades();
        self.refresh_filter();
        let reuse = self.node_plan.valid
            && self.node_plan.epoch == self.visual_epoch
            && self.node_plan.size_lo == size_lo
            && self.node_plan.size_hi == size_hi
            && scale_comp > 0.0
            && !self.fold_animating()
            && self.node_plan.ids.as_slice() == ids;
        if reuse {
            self.pack_nodes_from_plan(scale_comp);
        } else {
            self.pack_nodes_full(ids, scale_comp, size_lo, size_hi);
        }
    }

    fn pack_nodes_from_plan(&mut self, scale_comp: f32) {
        let mut buf = std::mem::take(&mut self.scratch.node_buf);
        let mut faded = std::mem::take(&mut self.faded_buf);
        buf.clear();
        faded.clear();
        self.render_positions.clear();
        for e in &self.node_plan.entries {
            let slot = e.slot as usize;
            let pos = self.positions[slot];
            let mut col = e.col;
            let target = if e.faded {
                col[3] = self.label_alpha.get(&e.id).copied().unwrap_or(1.0);
                &mut faded
            } else {
                &mut buf
            };
            self.live_tint(e.id, slot, &mut col);
            push_node(target, e.unit_size * scale_comp, pos, &col, &e.custom);
            self.render_positions.push(pos);
        }
        self.scratch.node_buf = buf;
        self.faded_buf = faded;
    }

    fn pack_nodes_full(&mut self, ids: &[i32], scale_comp: f32, size_lo: f32, size_hi: f32) {
        // Gem-tier ids of the outgoing plan, to carry hysteresis across a rebuild.
        let prev_near: HashSet<u32> = self
            .node_plan
            .entries
            .iter()
            .zip(self.node_plan.near.iter())
            .filter(|(_, &n)| n)
            .map(|(e, _)| e.id)
            .collect();
        let mut buf = self.full_node_pack(ids, scale_comp, size_lo, size_hi);
        let plan = &mut self.node_plan;
        plan.entries.clear();
        plan.near.clear();
        plan.ids.clear();
        plan.ids.extend_from_slice(ids);
        plan.size_lo = size_lo;
        plan.size_hi = size_hi;
        plan.epoch = self.visual_epoch;
        plan.generation = plan.generation.wrapping_add(1);
        plan.valid = scale_comp > 0.0 && !(!self.folding.is_empty() || !self.unfolding.is_empty());
        let (mut km, mut kf) = (0usize, 0usize);
        for &id in &self.render_ids {
            let Some(&slot) = self.id_index.get(&id) else {
                plan.valid = false;
                continue;
            };
            let faded = self.label_alpha.contains_key(&id);
            let src = if faded {
                let o = kf * NODE_STRIDE;
                kf += 1;
                &self.faded_buf[o..o + NODE_STRIDE]
            } else {
                let o = km * NODE_STRIDE;
                km += 1;
                &buf[o..o + NODE_STRIDE]
            };
            let unit_size = if scale_comp > 0.0 { src[0] / scale_comp } else { 0.0 };
            plan.entries.push(NodePlanEntry {
                id,
                slot: slot as u32,
                unit_size,
                col: [src[12], src[13], src[14], src[15]],
                custom: [src[16], src[17], src[18], src[19]],
                faded,
            });
            plan.near.push(prev_near.contains(&id));
        }
        // Live tint on the full path too, so both paths emit the same colours.
        let (mut km, mut kf) = (0usize, 0usize);
        for i in 0..self.node_plan.entries.len() {
            let e = self.node_plan.entries[i];
            let o = if e.faded { kf += 1; (kf - 1) * NODE_STRIDE } else { km += 1; (km - 1) * NODE_STRIDE };
            let src: &[f32] = if e.faded { &self.faded_buf } else { &buf };
            let mut col = [src[o + 12], src[o + 13], src[o + 14], src[o + 15]];
            self.live_tint(e.id, e.slot as usize, &mut col);
            let dst: &mut [f32] = if e.faded { &mut self.faded_buf } else { &mut buf };
            dst[o + 12..o + 16].copy_from_slice(&col);
        }
        self.scratch.node_buf = buf;
    }

    #[allow(clippy::too_many_arguments)]
    /// Node pack split into LOD tiers (see `lod::split_tiers_into`); results in
    /// `scratch.near_buf` (gem tier) and `impostor_buf`.
    pub(super) fn pack_nodes_lod(
        &mut self,
        ids: &[i32],
        scale_comp: f32,
        size_lo: f32,
        size_hi: f32,
        cam: [f32; 3],
        near_cap: usize,
        near_max_dist: f32,
    ) {
        self.pack_nodes(ids, scale_comp, size_lo, size_hi);
        let sc = &mut self.scratch;
        sc.inst_prev.clear();
        sc.inst_entry.clear();
        let mut faded = 0usize;
        for (i, e) in self.node_plan.entries.iter().enumerate() {
            if e.faded {
                faded += 1;
            } else {
                sc.inst_prev.push(self.node_plan.near.get(i).copied().unwrap_or(false));
                sc.inst_entry.push(i as u32);
            }
        }
        let cap = near_cap.saturating_sub(faded);
        crate::lod::split_tiers_into(
            &sc.node_buf,
            NODE_STRIDE,
            &sc.inst_prev,
            cam,
            cap,
            near_max_dist,
            &mut sc.near_buf,
            &mut self.impostor_buf,
            &mut sc.inst_near,
            &mut sc.cand,
        );
        for n in self.node_plan.near.iter_mut() {
            *n = false;
        }
        for (k, &flag) in sc.inst_near.iter().enumerate() {
            if flag {
                if let Some(n) = self.node_plan.near.get_mut(sc.inst_entry[k] as usize) {
                    *n = true;
                }
            }
        }
    }

    fn rebuild_edge_plan(&mut self, pairs: &[i32], radius_comp: f32, fold_active: bool) {
        let mut entries = std::mem::take(&mut self.edge_plan.entries);
        let old_near: HashSet<(u32, u32)> = entries
            .iter()
            .zip(self.edge_plan.near.iter())
            .filter(|(_, &n)| n)
            .map(|(e, _)| (e.s, e.t))
            .collect();
        entries.clear();
        let mut near = std::mem::take(&mut self.edge_plan.near);
        near.clear();
        if !fold_active {
            let n = pairs.len() / 2;
            for i in 0..n {
                let s = pairs[i * 2] as u32;
                let t = pairs[i * 2 + 1] as u32;
                if s == t || !self.drawn.contains(&s) || !self.drawn.contains(&t) {
                    continue;
                }
                let (Some(&ss), Some(&ts)) = (self.id_index.get(&s), self.id_index.get(&t)) else {
                    continue;
                };
                let e = EdgePlanEntry { s: ss as u32, t: ts as u32, style: self.edge_style_of(s, t) as f32 };
                near.push(old_near.contains(&(e.s, e.t)));
                entries.push(e);
            }
        }
        let plan = &mut self.edge_plan;
        plan.entries = entries;
        plan.near = near;
        plan.pairs.clear();
        plan.pairs.extend_from_slice(pairs);
        plan.radius = radius_comp;
        plan.epoch = self.visual_epoch;
        plan.node_generation = self.node_plan.generation;
        plan.valid = !fold_active;
    }

    /// Edge pack split into LOD tiers; results in `scratch.edge_near_buf`
    /// (cylinders) and `ribbon_buf`.
    ///
    /// With a valid plan the tier choice uses only edge midpoints (cheap), the
    /// near cylinders are transformed every frame, and the far ribbons — about
    /// 19 900 of 20 000 at production density — are re-transformed half per
    /// frame (alternating instance parity, in place), so each is at most one frame
    /// old and the frame cost is flat. Any change of tier membership rebuilds the
    /// far tier at once, so the tiers always partition the drawn set (no edge
    /// drawn twice or dropped).
    pub(super) fn pack_edges_lod(&mut self, pairs: &[i32], radius_comp: f32, cam: [f32; 3], near_cap: usize, near_max_dist: f32) {
        let fold_active = !self.fold_remap.is_empty() || self.fold_animating();
        let reuse = self.edge_plan.valid
            && self.edge_plan.epoch == self.visual_epoch
            && self.edge_plan.node_generation == self.node_plan.generation
            && self.edge_plan.radius == radius_comp
            && !fold_active
            && self.edge_plan.pairs.as_slice() == pairs;
        let mut fresh = false;
        if !reuse {
            self.rebuild_edge_plan(pairs, radius_comp, fold_active);
            fresh = true;
        }
        if !self.edge_plan.valid {
            self.pack_edges_lod_full(pairs, radius_comp, cam, near_cap, near_max_dist);
            return;
        }
        // Tier choice from midpoints.
        let max_sq = if near_max_dist.is_finite() { near_max_dist * near_max_dist } else { f32::INFINITY };
        let sc = &mut self.scratch;
        sc.cand.clear();
        for (i, e) in self.edge_plan.entries.iter().enumerate() {
            let a = self.positions[e.s as usize];
            let b = self.positions[e.t as usize];
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5];
            let d2 = crate::lod::distance_squared(cam, mid);
            if d2.is_nan() || d2 > max_sq {
                continue; // beyond the radius, or NaN
            }
            let was = self.edge_plan.near.get(i).copied().unwrap_or(false);
            sc.cand.push((if was { d2 * crate::lod::NEAR_HYSTERESIS_SQ } else { d2 }, i));
        }
        let take = near_cap.min(sc.cand.len());
        sc.inst_near.clear();
        sc.inst_near.resize(self.edge_plan.entries.len(), false);
        if take > 0 {
            if take < sc.cand.len() {
                sc.cand.select_nth_unstable_by(take - 1, |a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            }
            for &(_, i) in &sc.cand[..take] {
                sc.inst_near[i] = true;
            }
        }
        let changed = sc.inst_near != self.edge_plan.near;
        self.edge_frame = self.edge_frame.wrapping_add(1);
        let rebuild_far = fresh || changed || sc.ribbon_stale;
        sc.ribbon_stale = false;
        sc.edge_near_buf.clear();
        if rebuild_far {
            self.ribbon_buf.clear();
            sc.ribbon_entry.clear();
        }
        // Near cylinders every frame; on a rebuild, the whole far tier too.
        for (i, e) in self.edge_plan.entries.iter().enumerate() {
            let near = sc.inst_near[i];
            if !near && !rebuild_far {
                continue;
            }
            let Some(tf) = edge_transform12(self.positions[e.s as usize], self.positions[e.t as usize], radius_comp) else {
                continue;
            };
            if near {
                sc.edge_near_buf.extend_from_slice(&tf);
                sc.edge_near_buf.extend_from_slice(&[0.0, 0.0, 0.0, e.style]);
            } else {
                self.ribbon_buf.extend_from_slice(&tf);
                self.ribbon_buf.extend_from_slice(&[0.0, 0.0, 0.0, e.style]);
                sc.ribbon_entry.push(i as u32);
            }
        }
        if !rebuild_far {
            // Interleaved: this frame's parity of ribbon instances, in place, so
            // every ribbon is at most one frame old and the cost is flat.
            let parity = (self.edge_frame % 2) as usize;
            let mut j = parity;
            while j < sc.ribbon_entry.len() {
                let e = self.edge_plan.entries[sc.ribbon_entry[j] as usize];
                match edge_transform12(self.positions[e.s as usize], self.positions[e.t as usize], radius_comp) {
                    Some(tf) => self.ribbon_buf[j * EDGE_STRIDE_TYPED..j * EDGE_STRIDE_TYPED + 12].copy_from_slice(&tf),
                    // Collapsed this frame: keep the old transform, rebuild next frame.
                    None => sc.ribbon_stale = true,
                }
                j += 2;
            }
        }
        self.edge_plan.near.clone_from(&sc.inst_near);
    }

    /// Fold active: full pack (dedup + remap) and a per-frame split.
    fn pack_edges_lod_full(&mut self, pairs: &[i32], radius_comp: f32, cam: [f32; 3], near_cap: usize, near_max_dist: f32) {
        let full = self.pack_edges(pairs, radius_comp, None);
        let sc = &mut self.scratch;
        sc.inst_prev.clear();
        crate::lod::split_tiers_into(
            &full,
            EDGE_STRIDE_TYPED,
            &sc.inst_prev,
            cam,
            near_cap,
            near_max_dist,
            &mut sc.edge_near_buf,
            &mut self.ribbon_buf,
            &mut sc.inst_near,
            &mut sc.cand,
        );
    }
}
