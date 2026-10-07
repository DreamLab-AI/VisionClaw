//! Distance-bucket LOD policy. Thresholds are a verbatim port of
//! `client/src/immersive/hooks/useVRConnectionsLOD.ts` so visual fidelity
//! matches the deprecated browser path. Recompute cadence is every 2 frames
//! per `xr-godot-system-architecture.md` §4.

#[cfg(not(test))]
use godot::prelude::*;

pub const HIGH_DISTANCE_M: f32 = 5.0;
pub const MEDIUM_DISTANCE_M: f32 = 15.0;
pub const LOW_DISTANCE_M: f32 = 30.0;
pub const RECOMPUTE_INTERVAL_FRAMES: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodLevel {
    High,
    Medium,
    Low,
    Culled,
}

impl LodLevel {
    pub fn as_i32(self) -> i32 {
        match self {
            LodLevel::High => 0,
            LodLevel::Medium => 1,
            LodLevel::Low => 2,
            LodLevel::Culled => 3,
        }
    }

    /// Inverse of [`as_i32`]. Out-of-range values clamp to `Culled` (the safe,
    /// least-work bucket) rather than panic, so a stray GDScript int can never
    /// crash the render path.
    pub fn from_i32(v: i32) -> LodLevel {
        match v {
            0 => LodLevel::High,
            1 => LodLevel::Medium,
            2 => LodLevel::Low,
            _ => LodLevel::Culled,
        }
    }
}

// --- Agent-avatar feature LOD -------------------------------------------------
//
// The copresence brief (§1) is explicit about the drop order for the geometric
// agent embodiment: "LOD-trivial: drop badge/cone first, billboard the core at
// distance". These bits let GDScript read one integer per level instead of
// re-deriving the policy scene-side. The DID badge is the cheapest cue to lose,
// the gaze cone next; the core survives longest, switching from a lit mesh to a
// camera-facing billboard before it culls entirely.

/// The DID/name badge (a pooled `Label3D`).
pub const AGENT_FEAT_BADGE: i32 = 1 << 0;
/// The translucent gaze cone.
pub const AGENT_FEAT_CONE: i32 = 1 << 1;
/// The geometric core rendered as a full lit mesh.
pub const AGENT_FEAT_CORE_MESH: i32 = 1 << 2;
/// The geometric core rendered as a cheap camera-facing billboard.
pub const AGENT_FEAT_CORE_BILLBOARD: i32 = 1 << 3;

/// Which agent-avatar features are visible at `level`. Badge drops at Medium,
/// cone drops at Low (where the core also degrades to a billboard), and Culled
/// shows nothing. `AGENT_FEAT_CORE_MESH` and `AGENT_FEAT_CORE_BILLBOARD` are
/// mutually exclusive by construction.
pub fn agent_feature_mask(level: LodLevel) -> i32 {
    match level {
        LodLevel::High => AGENT_FEAT_BADGE | AGENT_FEAT_CONE | AGENT_FEAT_CORE_MESH,
        LodLevel::Medium => AGENT_FEAT_CONE | AGENT_FEAT_CORE_MESH,
        LodLevel::Low => AGENT_FEAT_CORE_BILLBOARD,
        LodLevel::Culled => 0,
    }
}

pub fn classify(distance_m: f32) -> LodLevel {
    if distance_m < HIGH_DISTANCE_M {
        LodLevel::High
    } else if distance_m < MEDIUM_DISTANCE_M {
        LodLevel::Medium
    } else if distance_m < LOW_DISTANCE_M {
        LodLevel::Low
    } else {
        LodLevel::Culled
    }
}

pub fn classify_squared(distance_sq_m2: f32) -> LodLevel {
    let high_sq = HIGH_DISTANCE_M * HIGH_DISTANCE_M;
    let med_sq = MEDIUM_DISTANCE_M * MEDIUM_DISTANCE_M;
    let low_sq = LOW_DISTANCE_M * LOW_DISTANCE_M;
    if distance_sq_m2 < high_sq {
        LodLevel::High
    } else if distance_sq_m2 < med_sq {
        LodLevel::Medium
    } else if distance_sq_m2 < low_sq {
        LodLevel::Low
    } else {
        LodLevel::Culled
    }
}

pub fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

pub struct LodPolicyState {
    frame_counter: u32,
    last_levels: Vec<LodLevel>,
}

impl LodPolicyState {
    pub fn new() -> Self {
        Self {
            frame_counter: 0,
            last_levels: Vec::new(),
        }
    }

    pub fn tick(&mut self) -> bool {
        self.frame_counter = self.frame_counter.wrapping_add(1);
        self.frame_counter.is_multiple_of(RECOMPUTE_INTERVAL_FRAMES)
    }

    pub fn classify_avatars(&mut self, camera: [f32; 3], avatars: &[[f32; 3]]) -> &[LodLevel] {
        self.last_levels.clear();
        for pos in avatars {
            let d_sq = distance_squared(camera, *pos);
            self.last_levels.push(classify_squared(d_sq));
        }
        &self.last_levels
    }
}

impl Default for LodPolicyState {
    fn default() -> Self {
        Self::new()
    }
}

/// Pick the indices of the `cap` most important nodes by centrality. Used to
/// bound the Quest MultiMesh instance count on large graphs: when the node
/// count exceeds the cap, only the structurally significant nodes render.
/// Returns ALL indices (identity order) when `count <= cap`. Selection is
/// O(n log n); ties keep the lower index for determinism.
pub fn select_top_by_centrality(centrality: &[f32], cap: usize) -> Vec<u32> {
    if centrality.len() <= cap {
        return (0..centrality.len() as u32).collect();
    }
    let mut idx: Vec<u32> = (0..centrality.len() as u32).collect();
    idx.sort_by(|&a, &b| {
        let ca = centrality[a as usize];
        let cb = centrality[b as usize];
        cb.partial_cmp(&ca)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    idx.truncate(cap);
    idx.sort_unstable(); // restore stable scene order for the renderer
    idx
}

// --- Node-mesh LOD (PRD-008 §6: ≤ 100k triangles, ≤ 50 draw calls) ---------
//
// The full gem node was a 16×8 sphere drawn twice (the halo `next_pass`): 576
// triangles per node on HP (Godot 4.6.1, opengl3, 2026-10-07), so 1 000 gem
// nodes alone were 576k triangles and 13k ≈ 7.5M. The halo is now a quad layer
// (290 per gem node), and two tiers bound the rest without touching the near
// look: the nearest `near_cap` nodes inside `near_max_dist` keep the gem, and
// every other drawn node is a 2-triangle camera-facing impostor
// (`materials/node_impostor.gdshader`) in one extra MultiMesh — one more draw
// call, whatever the node count.

/// 16×8 SphereMesh (GraphScene `SphereMesh_node`), measured.
pub const SPHERE_TRIS: usize = 288;
/// The halo is one camera-facing quad per gem node (`NodesHaloMulti`,
/// `node_halo_quad.gdshader`), not a second sphere pass: the old `next_pass`
/// shell doubled the gem to 576 triangles.
pub const HALO_TRIS_PER_NODE: usize = 2;
/// Triangles per gem-tier node: one sphere pass plus its halo quad.
pub const GEM_TRIS_PER_NODE: usize = SPHERE_TRIS + HALO_TRIS_PER_NODE;
/// One quad per impostor.
pub const IMPOSTOR_TRIS_PER_NODE: usize = 2;
/// Near-field triangle ceiling the cap is sized for.
pub const NEAR_TRI_BUDGET: usize = 60_000;
/// Default gem cap: 80 · 290 = 23 200 triangles. Sized with the edge tier and
/// hulls so the whole scene stays ≥ 3 % under 100k at 13 164 nodes / 20 000
/// edges (see `scene_triangle_estimate`).
pub const DEFAULT_NEAR_CAP: usize = 80;
/// Default near radius in world metres (the graph is fitted to ~2.4 m, a node
/// is ~3 cm): past ~1 m a node subtends under 2°, where the impostor's shaded
/// disc and the sphere are indistinguishable.
pub const DEFAULT_NEAR_RADIUS_M: f32 = 1.0;
/// A node that was in the gem tier last build competes with its squared
/// distance scaled by this, so boundary nodes do not flip tiers every frame.
pub const NEAR_HYSTERESIS_SQ: f32 = 0.81; // ≈ 10 % in distance

/// Worst-case triangles for nodes plus edges with both near tiers full (hulls
/// excluded — add `hulls::DEFAULT_MAX_HULLS * hulls::MAX_TRIS_PER_HULL`).
pub fn scene_triangle_estimate(nodes: usize, edges: usize, near_cap: usize, near_edge_cap: usize) -> usize {
    let near_edges = edges.min(near_edge_cap);
    node_triangle_estimate(nodes, near_cap)
        + near_edges * CYLINDER_TRIS_PER_EDGE
        + (edges - near_edges) * RIBBON_TRIS_PER_EDGE
}

/// Worst-case node triangles for `nodes` drawn with a full gem cap.
pub fn node_triangle_estimate(nodes: usize, near_cap: usize) -> usize {
    let near = nodes.min(near_cap);
    near * GEM_TRIS_PER_NODE + (nodes - near) * IMPOSTOR_TRIS_PER_NODE
}

/// Uncapped 8-sided edge cylinder (GraphScene `CylinderMesh_edge`, caps off —
/// the ends sit inside the node spheres): measured 32 triangles (48 with caps).
pub const CYLINDER_TRIS_PER_EDGE: usize = 32;
/// One camera-facing ribbon quad per far edge.
pub const RIBBON_TRIS_PER_EDGE: usize = 2;
/// Default near-field cylinder cap: 96 · 32 = 3 072 triangles.
pub const DEFAULT_NEAR_EDGE_CAP: usize = 96;

/// Split a packed instance buffer (`stride` floats per instance, row-major 3×4
/// transform first, origin at floats 3/7/11 — node centre or edge midpoint) into
/// a near tier and a far tier, writing into caller-owned buffers so a steady
/// frame allocates nothing.
///
/// `prev_near[i]` (missing ⇒ false) says instance i was in the near tier last
/// build; such an instance competes with its squared distance scaled by
/// [`NEAR_HYSTERESIS_SQ`]. The near tier is the `near_cap` instances nearest
/// `cam` within `near_max_dist`; both outputs keep scene order and every float of
/// an instance travels with it. `near_flags` receives the per-instance result.
/// O(n): a partial select, not a sort. A truncated trailing instance is dropped.
#[allow(clippy::too_many_arguments)]
pub fn split_tiers_into(
    buf: &[f32],
    stride: usize,
    prev_near: &[bool],
    cam: [f32; 3],
    near_cap: usize,
    near_max_dist: f32,
    near: &mut Vec<f32>,
    far: &mut Vec<f32>,
    near_flags: &mut Vec<bool>,
    cand: &mut Vec<(f32, usize)>,
) {
    near.clear();
    far.clear();
    near_flags.clear();
    cand.clear();
    if stride < 12 {
        far.extend_from_slice(buf);
        return;
    }
    let n = buf.len() / stride;
    let max_sq = if near_max_dist.is_finite() { near_max_dist * near_max_dist } else { f32::INFINITY };
    for i in 0..n {
        let o = i * stride;
        let d2 = distance_squared(cam, [buf[o + 3], buf[o + 7], buf[o + 11]]);
        if d2.is_nan() || d2 > max_sq {
            continue; // beyond the radius, or NaN
        }
        let was_near = prev_near.get(i).copied().unwrap_or(false);
        cand.push((if was_near { d2 * NEAR_HYSTERESIS_SQ } else { d2 }, i));
    }
    let take = near_cap.min(cand.len());
    near_flags.resize(n, false);
    if take > 0 {
        if take < cand.len() {
            cand.select_nth_unstable_by(take - 1, |a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        }
        for &(_, i) in &cand[..take] {
            near_flags[i] = true;
        }
    }
    for (i, &flag) in near_flags.iter().enumerate() {
        let chunk = &buf[i * stride..(i + 1) * stride];
        if flag {
            near.extend_from_slice(chunk);
        } else {
            far.extend_from_slice(chunk);
        }
    }
}

/// [`split_tiers_into`] keyed by stable instance identities (node id, endpoint
/// pair) with hysteresis against the previous near set; allocating convenience
/// form for callers without a pack plan.
#[allow(clippy::type_complexity)]
pub fn split_tiers<K: Copy + Eq + std::hash::Hash>(
    buf: &[f32],
    stride: usize,
    keys: &[K],
    cam: [f32; 3],
    near_cap: usize,
    near_max_dist: f32,
    prev_near: &std::collections::HashSet<K>,
) -> (Vec<f32>, Vec<f32>, std::collections::HashSet<K>) {
    let prev: Vec<bool> = keys.iter().map(|k| prev_near.contains(k)).collect();
    let (mut near, mut far, mut flags, mut cand) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    split_tiers_into(buf, stride, &prev, cam, near_cap, near_max_dist, &mut near, &mut far, &mut flags, &mut cand);
    let near_keys = flags
        .iter()
        .enumerate()
        .filter(|(_, &f)| f)
        .filter_map(|(i, _)| keys.get(i).copied())
        .collect();
    (near, far, near_keys)
}

/// [`split_tiers`] for the 20-float node buffer keyed by node id.
pub fn split_node_tiers(
    buf: &[f32],
    ids: &[u32],
    cam: [f32; 3],
    near_cap: usize,
    near_max_dist: f32,
    prev_near: &std::collections::HashSet<u32>,
) -> (Vec<f32>, Vec<f32>, std::collections::HashSet<u32>) {
    split_tiers(buf, 20, ids, cam, near_cap, near_max_dist, prev_near)
}

#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct LodPolicy {
    state: LodPolicyState,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl LodPolicy {
    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            state: LodPolicyState::new(),
            base,
        })
    }

    #[func]
    fn should_recompute(&mut self) -> bool {
        self.state.tick()
    }

    #[func]
    fn classify_distance(&self, distance_m: f32) -> i32 {
        classify(distance_m).as_i32()
    }

    /// Feature-visibility bitmask for an agent avatar at LOD `level` (0 High ..
    /// 3 Culled). Bits: 1 badge, 2 cone, 4 core-mesh, 8 core-billboard. The
    /// scene reads this to drop the badge, then the cone, then billboard the
    /// core as distance grows — the brief's LOD drop order in one integer.
    #[func]
    fn agent_feature_mask(&self, level: i32) -> i32 {
        agent_feature_mask(LodLevel::from_i32(level))
    }

    /// Indices of the `cap` highest-centrality nodes (all indices when the
    /// node count is within the cap). Drives the Quest node-instance budget.
    #[func]
    fn visible_subset(&self, centrality: PackedFloat32Array, cap: i64) -> PackedInt32Array {
        let cap = cap.max(0) as usize;
        let picked = select_top_by_centrality(centrality.as_slice(), cap);
        PackedInt32Array::from(
            picked
                .into_iter()
                .map(|i| i as i32)
                .collect::<Vec<i32>>()
                .as_slice(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_under_5m() {
        assert_eq!(classify(0.0), LodLevel::High);
        assert_eq!(classify(4.99), LodLevel::High);
    }

    #[test]
    fn medium_5_to_15m() {
        assert_eq!(classify(5.0), LodLevel::Medium);
        assert_eq!(classify(14.99), LodLevel::Medium);
    }

    #[test]
    fn low_15_to_30m() {
        assert_eq!(classify(15.0), LodLevel::Low);
        assert_eq!(classify(29.99), LodLevel::Low);
    }

    #[test]
    fn culled_above_30m() {
        assert_eq!(classify(30.0), LodLevel::Culled);
        assert_eq!(classify(1000.0), LodLevel::Culled);
    }

    #[test]
    fn squared_classify_matches_linear() {
        for d in [0.5_f32, 4.9, 5.1, 14.5, 15.5, 29.5, 30.5, 100.0] {
            assert_eq!(classify(d), classify_squared(d * d), "mismatch at {d}");
        }
    }

    #[test]
    fn tick_returns_true_every_two_frames() {
        let mut s = LodPolicyState::new();
        assert!(!s.tick(), "frame 1 should not recompute");
        assert!(s.tick(), "frame 2 should recompute");
        assert!(!s.tick(), "frame 3 should not recompute");
        assert!(s.tick(), "frame 4 should recompute");
    }

    #[test]
    fn classify_avatars_respects_camera_position() {
        let mut s = LodPolicyState::new();
        let cam = [0.0, 0.0, 0.0];
        let avatars = vec![
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 10.0],
            [0.0, 0.0, 25.0],
            [0.0, 0.0, 50.0],
        ];
        let levels = s.classify_avatars(cam, &avatars).to_vec();
        assert_eq!(
            levels,
            vec![
                LodLevel::High,
                LodLevel::Medium,
                LodLevel::Low,
                LodLevel::Culled
            ]
        );
    }

    #[test]
    fn distance_squared_identity() {
        assert_eq!(distance_squared([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn distance_squared_known_vector() {
        let d = distance_squared([0.0, 0.0, 0.0], [3.0, 4.0, 0.0]);
        assert!((d - 25.0).abs() < f32::EPSILON);
    }

    #[test]
    fn negative_distance_handled() {
        // A negative distance value is nonsensical but classify should not
        // panic. Since -2.0 < 5.0 the branch returns High.
        assert_eq!(classify(-2.0), LodLevel::High);
    }

    #[test]
    fn tick_fires_on_interval_cadence() {
        let mut s = LodPolicyState::new();
        let ticks = 10_000u32;
        let mut fired = 0u32;
        for i in 1..=ticks {
            let recompute = s.tick();
            assert_eq!(
                recompute,
                i.is_multiple_of(RECOMPUTE_INTERVAL_FRAMES),
                "tick {i} cadence mismatch"
            );
            fired += recompute as u32;
        }
        // Exactly one recompute per interval, and wrapping_add never panics.
        assert_eq!(fired, ticks / RECOMPUTE_INTERVAL_FRAMES);
    }

    #[test]
    fn classify_avatars_empty_returns_empty() {
        let mut s = LodPolicyState::new();
        let levels = s.classify_avatars([0.0, 0.0, 0.0], &[]).to_vec();
        assert!(levels.is_empty());
    }

    #[test]
    fn top_by_centrality_identity_when_under_cap() {
        assert_eq!(
            select_top_by_centrality(&[0.1, 0.9, 0.5], 3),
            vec![0, 1, 2]
        );
        assert_eq!(select_top_by_centrality(&[], 10), Vec::<u32>::new());
    }

    #[test]
    fn top_by_centrality_picks_most_important() {
        // cap 2 of 4: highest are index 1 (0.9) and 3 (0.7); output in index order.
        assert_eq!(
            select_top_by_centrality(&[0.1, 0.9, 0.3, 0.7], 2),
            vec![1, 3]
        );
    }

    #[test]
    fn top_by_centrality_ties_keep_lower_index() {
        assert_eq!(
            select_top_by_centrality(&[0.5, 0.5, 0.5, 0.5], 2),
            vec![0, 1]
        );
    }

    #[test]
    fn top_by_centrality_handles_nan_without_panic() {
        let picked = select_top_by_centrality(&[f32::NAN, 0.9, 0.1], 2);
        assert_eq!(picked.len(), 2);
        assert!(picked.contains(&1), "real maximum must always survive NaNs");
    }

    #[test]
    fn from_i32_round_trips_every_level() {
        for lvl in [
            LodLevel::High,
            LodLevel::Medium,
            LodLevel::Low,
            LodLevel::Culled,
        ] {
            assert_eq!(LodLevel::from_i32(lvl.as_i32()), lvl);
        }
    }

    #[test]
    fn from_i32_clamps_out_of_range_to_culled() {
        assert_eq!(LodLevel::from_i32(-1), LodLevel::Culled);
        assert_eq!(LodLevel::from_i32(4), LodLevel::Culled);
        assert_eq!(LodLevel::from_i32(9999), LodLevel::Culled);
    }

    #[test]
    fn agent_features_drop_badge_first_then_cone() {
        // High shows everything (badge + cone + core mesh).
        let high = agent_feature_mask(LodLevel::High);
        assert!(high & AGENT_FEAT_BADGE != 0, "badge visible at High");
        assert!(high & AGENT_FEAT_CONE != 0, "cone visible at High");
        assert!(high & AGENT_FEAT_CORE_MESH != 0, "core mesh at High");

        // Medium drops the badge but keeps the cone and the core.
        let med = agent_feature_mask(LodLevel::Medium);
        assert!(med & AGENT_FEAT_BADGE == 0, "badge drops first at Medium");
        assert!(med & AGENT_FEAT_CONE != 0, "cone survives Medium");
        assert!(med & AGENT_FEAT_CORE_MESH != 0, "core mesh survives Medium");

        // Low drops the cone and billboards the core.
        let low = agent_feature_mask(LodLevel::Low);
        assert!(low & AGENT_FEAT_CONE == 0, "cone drops at Low");
        assert!(low & AGENT_FEAT_CORE_MESH == 0, "core is not a full mesh at Low");
        assert!(low & AGENT_FEAT_CORE_BILLBOARD != 0, "core billboards at Low");

        // Culled shows nothing.
        assert_eq!(agent_feature_mask(LodLevel::Culled), 0);
    }

    #[test]
    fn agent_core_mesh_and_billboard_never_coexist() {
        for lvl in [
            LodLevel::High,
            LodLevel::Medium,
            LodLevel::Low,
            LodLevel::Culled,
        ] {
            let m = agent_feature_mask(lvl);
            let both = (m & AGENT_FEAT_CORE_MESH != 0) && (m & AGENT_FEAT_CORE_BILLBOARD != 0);
            assert!(!both, "mesh and billboard are mutually exclusive at {lvl:?}");
        }
    }

    #[test]
    fn agent_feature_visibility_is_monotone_non_increasing() {
        // As the level index grows (further away) the visible-feature count must
        // never increase — LOD only ever sheds detail.
        let counts: Vec<u32> = [
            LodLevel::High,
            LodLevel::Medium,
            LodLevel::Low,
            LodLevel::Culled,
        ]
        .iter()
        .map(|l| agent_feature_mask(*l).count_ones())
        .collect();
        for w in counts.windows(2) {
            assert!(w[1] <= w[0], "feature count must not grow with distance: {counts:?}");
        }
    }
}
