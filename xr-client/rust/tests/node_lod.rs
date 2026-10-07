//! Node-mesh LOD (PRD-008 §6 budget: ≤ 100k triangles, ≤ 50 draw calls).
//!
//! The nearest `near_cap` nodes within `near_max_dist` keep the full gem mesh
//! (sphere + halo pass); every other drawn node is a 2-triangle impostor in a
//! second MultiMesh. These tests pin the split, its hysteresis and the budget
//! arithmetic for the 1k benchmark fixture and the 13k production density.

use std::collections::HashSet;

use visionclaw_xr_gdext::lod::{
    node_triangle_estimate, split_node_tiers, DEFAULT_NEAR_CAP, GEM_TRIS_PER_NODE,
    IMPOSTOR_TRIS_PER_NODE, NEAR_TRI_BUDGET,
};
use visionclaw_xr_gdext::render_store::{RenderStore, NODE_STRIDE};

/// A packed node buffer (row-major 3×4, origin at 3/7/11) for points on +x.
fn packed(xs: &[f32]) -> Vec<f32> {
    let mut b = Vec::new();
    for (i, &x) in xs.iter().enumerate() {
        b.extend_from_slice(&[1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        b.extend_from_slice(&[0.5, 0.5, 0.5, 1.0]);
        b.extend_from_slice(&[i as f32, 0.0, 0.0, 1.0]); // custom.r tags the instance
    }
    b
}

fn origins_x(buf: &[f32]) -> Vec<f32> {
    buf.chunks_exact(NODE_STRIDE).map(|c| c[3]).collect()
}

#[test]
fn nearest_n_keep_the_gem_and_the_rest_become_impostors() {
    let xs = [9.0, 1.0, 5.0, 2.0, 7.0, 3.0];
    let ids: Vec<u32> = (10..16).collect();
    let buf = packed(&xs);
    let (near, far, near_ids) = split_node_tiers(&buf, &ids, [0.0; 3], 3, f32::INFINITY, &HashSet::new());
    assert_eq!(origins_x(&near), vec![1.0, 2.0, 3.0], "nearest three, scene order kept");
    assert_eq!(origins_x(&far), vec![9.0, 5.0, 7.0], "the rest, scene order kept");
    assert_eq!(near_ids, HashSet::from([11, 13, 15]));
    assert_eq!(near.len() + far.len(), buf.len(), "no instance lost or duplicated");
    // Colour and custom channels travel with their instance.
    assert_eq!(near[NODE_STRIDE + 16], 3.0, "custom.r of the x=2 instance (index 3)");
}

#[test]
fn near_radius_excludes_far_nodes_even_under_the_cap() {
    let buf = packed(&[0.5, 1.5, 3.0]);
    let ids = [1, 2, 3];
    let (near, far, _) = split_node_tiers(&buf, &ids, [0.0; 3], 10, 1.0, &HashSet::new());
    assert_eq!(origins_x(&near), vec![0.5]);
    assert_eq!(origins_x(&far), vec![1.5, 3.0]);
}

#[test]
fn previously_near_nodes_win_a_close_call() {
    // id 2 is marginally farther than id 1 but was near last frame: with a cap
    // of one it keeps the gem rather than the pair flipping every frame.
    let buf = packed(&[1.00, 1.05]);
    let ids = [1, 2];
    let prev = HashSet::from([2]);
    let (near, _, near_ids) = split_node_tiers(&buf, &ids, [0.0; 3], 1, f32::INFINITY, &prev);
    assert_eq!(origins_x(&near), vec![1.05]);
    assert_eq!(near_ids, HashSet::from([2]));
    // A clearly nearer newcomer still takes the slot.
    let buf = packed(&[0.5, 1.05]);
    let (near, _, _) = split_node_tiers(&buf, &ids, [0.0; 3], 1, f32::INFINITY, &prev);
    assert_eq!(origins_x(&near), vec![0.5]);
}

#[test]
fn degenerate_inputs_are_safe() {
    let (n, f, ids) = split_node_tiers(&[], &[], [0.0; 3], 96, 1.0, &HashSet::new());
    assert!(n.is_empty() && f.is_empty() && ids.is_empty());
    let buf = packed(&[1.0, 2.0]);
    let (n, f, _) = split_node_tiers(&buf, &[1, 2], [0.0; 3], 0, f32::INFINITY, &HashSet::new());
    assert!(n.is_empty());
    assert_eq!(f.len(), buf.len(), "cap 0 → everything is an impostor");
    // A truncated trailing instance is dropped, never read out of bounds.
    let mut ragged = buf.clone();
    ragged.truncate(buf.len() - 3);
    let (n, f, _) = split_node_tiers(&ragged, &[1, 2], [0.0; 3], 5, f32::INFINITY, &HashSet::new());
    assert_eq!(n.len() + f.len(), NODE_STRIDE);
    // Mismatched id list: ids past the end are ignored, missing ids never panic.
    let (n, f, _) = split_node_tiers(&buf, &[1], [0.0; 3], 5, f32::INFINITY, &HashSet::new());
    assert_eq!(n.len() + f.len(), buf.len());
}

#[test]
fn budget_arithmetic_holds_for_the_fixture_and_production_density() {
    assert!(DEFAULT_NEAR_CAP * GEM_TRIS_PER_NODE <= NEAR_TRI_BUDGET, "near field ≤ ~60k");
    assert_eq!(IMPOSTOR_TRIS_PER_NODE, 2);
    // Worst case: the cap is full.
    let fixture = node_triangle_estimate(1_000, DEFAULT_NEAR_CAP);
    let production = node_triangle_estimate(13_164, DEFAULT_NEAR_CAP);
    assert!(fixture < 100_000, "1k fixture: {fixture}");
    assert!(production < 100_000, "13k production: {production}");
    assert_eq!(node_triangle_estimate(50, DEFAULT_NEAR_CAP), 50 * GEM_TRIS_PER_NODE, "small graphs are all gem");
}

#[test]
fn render_store_lod_build_counts_labelled_nodes_against_the_cap() {
    let mut s = RenderStore::new();
    let mut ids = Vec::new();
    for i in 0..200u32 {
        s.upsert(i + 1, [i as f32, 0.0, 0.0], 0, 0.0, 0.0);
        ids.push((i + 1) as i32);
    }
    // Two labelled nodes ride the faded (full-mesh) pass and use two cap slots.
    s.set_labelled(&[1, 2]);
    let near = s.build_node_buffer_lod(&ids, 1.0, 0.7, 1.9, [0.0; 3], 10, f32::INFINITY).to_vec();
    let far = s.impostor_node_buffer().to_vec();
    let faded = s.faded_node_buffer().len() / NODE_STRIDE;
    assert_eq!(faded, 2);
    assert_eq!(near.len() / NODE_STRIDE, 8, "cap 10 minus 2 labelled");
    assert_eq!(far.len() / NODE_STRIDE, 190);
    assert_eq!(s.render_ids().len(), 200, "every tier stays pickable by the ray");
    // The near tier is the nodes nearest the camera (ids 3..=10 at x = 2..9).
    let xs = origins_x(&near);
    assert!(xs.iter().all(|&x| (2.0..=9.0).contains(&x)), "{xs:?}");
}

// --- edge LOD ------------------------------------------------------------------

use visionclaw_xr_gdext::lod::{split_tiers, CYLINDER_TRIS_PER_EDGE, DEFAULT_NEAR_EDGE_CAP, RIBBON_TRIS_PER_EDGE};
use visionclaw_xr_gdext::render_store::EDGE_STRIDE_TYPED;

/// Packed 16-float edge instances centred at x (midpoint origin at 3/7/11),
/// INSTANCE_CUSTOM.a = style code i % 4.
fn packed_edges(xs: &[f32]) -> Vec<f32> {
    let mut b = Vec::new();
    for (i, &x) in xs.iter().enumerate() {
        b.extend_from_slice(&[0.03, 0.0, 0.0, x, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.03, 0.0]);
        b.extend_from_slice(&[0.0, 0.0, 0.0, (i % 4) as f32]);
    }
    b
}

#[test]
fn edge_split_keeps_the_16_float_stride_and_the_style_channel() {
    assert_eq!(EDGE_STRIDE_TYPED, 16, "Invariant 3");
    let buf = packed_edges(&[8.0, 1.0, 6.0, 2.0]);
    let keys: Vec<u64> = vec![10, 11, 12, 13];
    let (near, far, near_keys) = split_tiers(&buf, EDGE_STRIDE_TYPED, &keys, [0.0; 3], 2, f32::INFINITY, &HashSet::new());
    assert_eq!(near.len(), 2 * 16);
    assert_eq!(far.len(), 2 * 16);
    assert_eq!(near_keys, HashSet::from([11, 13]));
    let mids: Vec<f32> = near.chunks_exact(16).map(|c| c[3]).collect();
    assert_eq!(mids, vec![1.0, 2.0], "nearest midpoints, scene order");
    let far_styles: Vec<f32> = far.chunks_exact(16).map(|c| c[15]).collect();
    assert_eq!(far_styles, vec![0.0, 2.0], "style code travels with the ribbon");
}

#[test]
fn render_store_edge_lod_splits_cylinders_from_ribbons() {
    let mut s = RenderStore::new();
    let mut ids = Vec::new();
    for i in 0..60u32 {
        s.upsert(i + 1, [i as f32 * 2.0, 0.0, 0.0], 0, 0.0, 0.0);
        ids.push((i + 1) as i32);
    }
    s.build_node_buffer(&ids, 1.0, 0.7, 1.9);
    // 50 edges between neighbours: midpoints at x = 1, 3, 5, …
    let mut pairs = Vec::new();
    for i in 0..50i32 {
        pairs.push(i + 1);
        pairs.push(i + 2);
    }
    let all = s.build_edge_buffer(&pairs, 1.0).len() / 16;
    assert_eq!(all, 50);
    let near = s.build_edge_buffer_lod(&pairs, 1.0, [0.0; 3], 5, f32::INFINITY).to_vec();
    let ribbons = s.ribbon_edge_buffer().len() / 16;
    assert_eq!(near.len() / 16, 5);
    assert_eq!(ribbons, 45);
    let mids: Vec<f32> = near.chunks_exact(16).map(|c| c[3]).collect();
    assert_eq!(mids, vec![1.0, 3.0, 5.0, 7.0, 9.0], "the five edges nearest the eye stay cylinders");
    // Radius bound.
    let near = s.build_edge_buffer_lod(&pairs, 1.0, [0.0; 3], 96, 4.5).to_vec();
    assert_eq!(near.len() / 16, 2, "midpoints 1 and 3 inside 4.5");
    // Hysteresis: with cap 1 the eye at x = 9 holds the edge at midpoint 9. Moving
    // to 10.05 makes midpoint 11 nearer (0.95 vs 1.05), but within the 10 %
    // margin, so the incumbent keeps its cylinder; a clear win still switches.
    let _ = s.build_edge_buffer_lod(&pairs, 1.0, [9.0, 0.0, 0.0], 1, f32::INFINITY);
    let near = s.build_edge_buffer_lod(&pairs, 1.0, [10.05, 0.0, 0.0], 1, f32::INFINITY).to_vec();
    assert_eq!(near[3], 9.0, "incumbent keeps the cylinder in a close call");
    let near = s.build_edge_buffer_lod(&pairs, 1.0, [10.8, 0.0, 0.0], 1, f32::INFINITY).to_vec();
    assert_eq!(near[3], 11.0, "a clearly nearer edge takes over");
}

#[test]
fn edge_budget_at_the_safety_ceiling() {
    assert_eq!(CYLINDER_TRIS_PER_EDGE, 32, "uncapped 8-sided cylinder (measured)");
    assert_eq!(RIBBON_TRIS_PER_EDGE, 2);
    let edges = 20_000usize;
    let tris = DEFAULT_NEAR_EDGE_CAP * CYLINDER_TRIS_PER_EDGE + (edges - DEFAULT_NEAR_EDGE_CAP) * RIBBON_TRIS_PER_EDGE;
    assert!(tris < 45_000, "{tris}");
}

// --- whole-scene budget (nodes + halo quads + edges + hulls) --------------------

use visionclaw_xr_gdext::hulls::{DEFAULT_MAX_HULLS, MAX_TRIS_PER_HULL};
use visionclaw_xr_gdext::lod::{scene_triangle_estimate, HALO_TRIS_PER_NODE, SPHERE_TRIS};

#[test]
fn gem_is_one_sphere_pass_plus_a_halo_quad() {
    assert_eq!(SPHERE_TRIS, 288, "16x8 SphereMesh, measured");
    assert_eq!(HALO_TRIS_PER_NODE, 2, "halo is a camera-facing quad, not a second sphere pass");
    assert_eq!(GEM_TRIS_PER_NODE, SPHERE_TRIS + HALO_TRIS_PER_NODE);
}

#[test]
fn whole_scene_worst_case_stays_under_100k_at_production_density() {
    let hull_max = DEFAULT_MAX_HULLS * MAX_TRIS_PER_HULL;
    for (nodes, edges) in [(1_000usize, 1_500usize), (13_164, 20_000)] {
        let t = scene_triangle_estimate(nodes, edges, DEFAULT_NEAR_CAP, DEFAULT_NEAR_EDGE_CAP) + hull_max;
        assert!(t <= 97_000, "{nodes} nodes / {edges} edges: {t} (keep ≥ 3 % headroom under 100k)");
    }
}

// --- far edge tier at half rate (CPU budget) -------------------------------------

#[test]
fn ribbons_refresh_every_other_frame_and_immediately_on_tier_change() {
    use visionclaw_xr_gdext::perf_fixture::production_store;
    let mut f = production_store(400, 900, 3);
    let cam = [0.0f32; 3];
    let total = |s: &mut RenderStore, pairs: &[i32]| s.build_edge_buffer(pairs, 1.0).len() / 16;
    // Per ribbon instance: changed this frame?
    let mut prev: Vec<f32> = Vec::new();
    let mut history: Vec<Vec<bool>> = Vec::new();
    for frame in 0..8u32 {
        f.advance(frame);
        f.store.build_node_buffer(&f.ids, 1.0, 0.7, 1.9);
        let near = f.store.build_edge_buffer_lod(&f.pairs, 1.0, cam, 24, f32::INFINITY).len() / 16;
        let ribbons = f.store.ribbon_edge_buffer().to_vec();
        // Never an edge drawn twice or dropped: the tiers always partition the drawn set.
        assert_eq!(near + ribbons.len() / 16, total(&mut f.store, &f.pairs), "frame {frame}");
        if prev.len() == ribbons.len() {
            history.push(ribbons.chunks_exact(16).zip(prev.chunks_exact(16)).map(|(a, b)| a != b).collect());
        }
        prev = ribbons;
    }
    assert!(history.len() >= 6, "tier membership stays stable while only positions move");
    for (k, h) in history.iter().enumerate() {
        let n = h.iter().filter(|&&c| c).count();
        assert!(n * 10 <= h.len() * 6, "frame {k}: about half the far tier repacked per frame, got {n}/{}", h.len());
    }
    for w in history.windows(2) {
        for i in 0..w[0].len() {
            assert!(w[0][i] || w[1][i], "ribbon {i} stale for two frames running");
        }
    }
    // Moving the eye across the graph changes tier membership: the far tier is
    // rebuilt at once and still partitions the drawn set.
    let near = f.store.build_edge_buffer_lod(&f.pairs, 1.0, [300.0, 300.0, 300.0], 24, f32::INFINITY).len() / 16;
    let c = f.store.ribbon_edge_buffer().len() / 16;
    assert_eq!(near + c, total(&mut f.store, &f.pairs), "membership change repacks the far tier at once");
}
