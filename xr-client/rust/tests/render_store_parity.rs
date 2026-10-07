//! WP1/WP2/WP4 integration through the public `RenderStore` API: domain colour
//! in the node buffer, the received node filter on the draw domain, and hulls
//! built from what is actually drawn.

use visionclaw_xr_gdext::domain_palette::{domain_node_color, hex_to_rgb, ColorMode};
use visionclaw_xr_gdext::hulls::{HullKey, HullParams, HullSource};
use visionclaw_xr_gdext::render_store::{
    community_color, query_var_color, RenderStore, KIND_AGENT, KIND_KNOWLEDGE, NODE_STRIDE,
};
use visionclaw_xr_gdext::settings_sync::{FilterInputs, NodeFilter};

fn colour_of(buf: &[f32], i: usize) -> [f32; 4] {
    let o = i * NODE_STRIDE + 12;
    [buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    (0..4).all(|k| (a[k] - b[k]).abs() < 1e-5)
}

fn ids(n: &[u32]) -> Vec<i32> {
    n.iter().map(|&x| x as i32).collect()
}

#[test]
fn domain_is_the_default_colour_mode_and_uses_the_palette() {
    let mut s = RenderStore::new();
    assert_eq!(s.color_mode(), ColorMode::Domain);
    s.upsert(1, [0.0; 3], 7, 0.0, 0.0);
    s.set_node_domain(1, "robotics");
    s.upsert(2, [1.0, 0.0, 0.0], 7, 0.0, 0.0); // no domain → fallback grey
    let buf = s.build_node_buffer(&ids(&[1, 2]), 1.0, 0.7, 1.9);
    let rb = hex_to_rgb("#FFB74D").unwrap();
    assert!(close(colour_of(&buf, 0), [rb[0], rb[1], rb[2], 1.0]));
    let grey = hex_to_rgb("#90A4AE").unwrap();
    assert!(close(colour_of(&buf, 1), [grey[0], grey[1], grey[2], 1.0]));
}

#[test]
fn domain_arriving_before_the_position_still_applies() {
    let mut s = RenderStore::new();
    s.set_node_domain(5, "AI");
    s.upsert(5, [0.0; 3], 0, 0.0, 0.0);
    let buf = s.build_node_buffer(&ids(&[5]), 1.0, 0.7, 1.9);
    assert!(close(colour_of(&buf, 0), domain_node_color(Some("AI"), 0)));
}

#[test]
fn community_mode_restores_the_golden_ratio_hue() {
    let mut s = RenderStore::new();
    s.upsert(1, [0.0; 3], 7, 0.0, 0.0);
    s.set_node_domain(1, "robotics");
    s.set_color_mode(ColorMode::Community);
    let buf = s.build_node_buffer(&ids(&[1]), 1.0, 0.7, 1.9);
    assert!(close(colour_of(&buf, 0), community_color(7, 0.0, 1)));
    s.set_color_mode(ColorMode::Domain);
    let buf = s.build_node_buffer(&ids(&[1]), 1.0, 0.7, 1.9);
    assert!(close(
        colour_of(&buf, 0),
        domain_node_color(Some("robotics"), 0)
    ));
}

#[test]
fn hub_lift_follows_degree_changes() {
    let mut s = RenderStore::new();
    s.upsert(1, [0.0; 3], 0, 0.0, 0.0);
    s.upsert(2, [1.0, 0.0, 0.0], 0, 0.0, 0.0);
    s.set_node_domain(1, "AI");
    let before = colour_of(&s.build_node_buffer(&ids(&[1]), 1.0, 0.7, 1.9), 0);
    s.compute_degrees(&[1, 2, 1, 2, 1, 2]);
    let after = colour_of(&s.build_node_buffer(&ids(&[1]), 1.0, 0.7, 1.9), 0);
    assert!(
        close(after, domain_node_color(Some("AI"), 3)),
        "degree 3 lift applied"
    );
    assert!(!close(before, after), "cache invalidated on degree change");
}

#[test]
fn query_marks_agents_and_anomaly_still_take_precedence_in_domain_mode() {
    let mut s = RenderStore::new();
    s.upsert(1, [0.0; 3], 0, 0.0, 0.0);
    s.upsert(2, [1.0, 0.0, 0.0], 0, 1.0, 0.0); // fully anomalous
    s.upsert(3, [2.0, 0.0, 0.0], 0, 0.0, 0.0);
    for id in [1, 2, 3] {
        s.set_node_domain(id, "blockchain");
    }
    s.set_query_var(1, 2);
    s.set_agent_state(3, "working", "");
    let buf = s.build_node_buffer(&ids(&[1, 2, 3]), 1.0, 0.7, 1.9);
    assert!(
        close(colour_of(&buf, 0), query_var_color(2)),
        "query mark wins"
    );
    let bc = domain_node_color(Some("blockchain"), 0);
    let anomalous = colour_of(&buf, 1);
    assert!(
        anomalous[0] > bc[0] && anomalous[1] < bc[1],
        "anomaly blends toward warning red"
    );
    assert!(!close(colour_of(&buf, 2), bc), "agent status colour wins");
}

#[test]
fn node_filter_hides_nodes_and_clearing_restores_them() {
    let mut s = RenderStore::new();
    for id in 1..=4u32 {
        s.upsert(id, [id as f32, 0.0, 0.0], 0, 0.0, 0.0);
        s.set_node_kind(id, KIND_KNOWLEDGE);
    }
    s.set_node_kind(4, KIND_AGENT);
    s.set_filter_inputs(
        1,
        FilterInputs {
            quality: Some(0.9),
            ..Default::default()
        },
    );
    s.set_filter_inputs(
        2,
        FilterInputs {
            quality: Some(0.2),
            ..Default::default()
        },
    );
    s.set_filter_inputs(
        3,
        FilterInputs {
            quality: Some(0.95),
            linked_page: true,
            ..Default::default()
        },
    );
    s.set_filter_inputs(
        4,
        FilterInputs {
            quality: Some(0.0),
            ..Default::default()
        },
    );
    let all = ids(&[1, 2, 3, 4]);
    assert_eq!(
        s.build_node_buffer(&all, 1.0, 0.7, 1.9).len() / NODE_STRIDE,
        4,
        "no filter received → all drawn"
    );

    s.set_node_filter(Some(NodeFilter {
        enabled: true,
        mode_and: true,
        ..NodeFilter::default()
    }));
    s.build_node_buffer(&all, 1.0, 0.7, 1.9);
    assert_eq!(
        s.render_ids(),
        &[1, 4],
        "low quality and linked_page hidden; agents exempt"
    );
    assert_eq!(s.filter_hidden_count(), 2);

    // A node that arrives after the filter is subject to it too.
    s.upsert(9, [9.0, 0.0, 0.0], 0, 0.0, 0.0);
    s.set_filter_inputs(
        9,
        FilterInputs {
            quality: Some(0.1),
            ..Default::default()
        },
    );
    s.build_node_buffer(&ids(&[1, 4, 9]), 1.0, 0.7, 1.9);
    assert_eq!(s.render_ids(), &[1, 4]);

    s.set_node_filter(None);
    s.build_node_buffer(&ids(&[1, 2, 3, 4, 9]), 1.0, 0.7, 1.9);
    assert_eq!(s.render_ids().len(), 5, "clearing restores every node");
}

#[test]
fn hidden_by_filter_nodes_drop_their_edges_and_search_hits() {
    let mut s = RenderStore::new();
    s.upsert(1, [0.0; 3], 0, 0.0, 0.0);
    s.upsert(2, [0.0, 3.0, 0.0], 0, 0.0, 0.0);
    s.set_meta(
        2,
        String::new(),
        "Hidden page".into(),
        String::new(),
        String::new(),
    );
    s.set_filter_inputs(
        2,
        FilterInputs {
            linked_page: true,
            ..Default::default()
        },
    );
    s.set_node_filter(Some(NodeFilter::default())); // disabled, but linked_page gate on
    s.build_node_buffer(&ids(&[1, 2]), 1.0, 0.7, 1.9);
    assert!(
        s.build_edge_buffer(&[1, 2], 1.0).is_empty(),
        "edge to a hidden node is not drawn"
    );
    assert!(
        s.search_labels("hidden", 5).is_empty(),
        "hidden nodes are not search hits"
    );
}

#[test]
fn hull_mesh_groups_drawn_nodes_by_cluster() {
    let mut s = RenderStore::new();
    let mut drawn = Vec::new();
    for i in 0..12u32 {
        let id = i + 1;
        let a = i as f32;
        s.upsert(
            id,
            [a.cos() * 50.0, (a * 1.3).sin() * 50.0, a * 7.0],
            0,
            0.0,
            0.0,
        );
        s.set_cluster(id, if i < 6 { 3 } else { 4 });
        drawn.push(id);
    }
    // Node 12 is not drawn → cluster 4 has 5 drawn members, still ≥ 4.
    s.build_node_buffer(&ids(&drawn[..11]), 1.0, 0.7, 1.9);
    let mesh = s.hull_mesh(HullSource::Clusters, HullParams::default());
    assert_eq!(mesh.keys, vec![HullKey::Cluster(3), HullKey::Cluster(4)]);
    assert!(mesh.triangle_count() >= 8);
    assert!(s
        .hull_mesh(HullSource::Off, HullParams::default())
        .keys
        .is_empty());
    // Signature is stable until something moves or the source changes.
    let a = s.hull_signature(HullSource::Clusters, HullParams::default());
    assert_eq!(
        a,
        s.hull_signature(HullSource::Clusters, HullParams::default())
    );
    assert_ne!(
        a,
        s.hull_signature(HullSource::Communities, HullParams::default())
    );
}

#[test]
fn attention_heat_keeps_animating_while_the_pack_plan_is_reused() {
    // Heat decays every frame while nothing else changes — exactly when the pack
    // plan is reused. The tint must follow the clock, not the cached colour.
    let mut s = RenderStore::new();
    s.upsert(5, [0.0; 3], 0, 0.0, 0.0); // agent
    s.upsert(20, [0.0, 4.0, 0.0], 3, 0.0, 0.0);
    s.upsert(21, [1.0, 4.0, 0.0], 3, 0.0, 0.0);
    s.set_clock_ms(1_000.0);
    assert!(s.record_agent_action(5, 0x4000_0000 | 20, 1, 100, ""));
    let col = |b: &[f32], i: usize| {
        [
            b[i * NODE_STRIDE + 12],
            b[i * NODE_STRIDE + 13],
            b[i * NODE_STRIDE + 14],
        ]
    };
    let hot = s.build_node_buffer(&ids(&[20, 21]), 1.0, 0.7, 1.9);
    assert_ne!(col(&hot, 0), col(&hot, 1), "touched node is brighter");
    // Five half-lives later, positions only: the plan is reused, the heat is gone.
    s.set_clock_ms(1_000.0 + 5.0 * visionclaw_xr_gdext::attention::DEFAULT_HEAT_HALF_LIFE_MS);
    let cool = s.build_node_buffer(&ids(&[20, 21]), 1.0, 0.7, 1.9);
    let (a, b) = (col(&cool, 0), col(&cool, 1));
    for k in 0..3 {
        assert!(
            (a[k] - b[k]).abs() < 0.03,
            "heat must decay on the plan path: {a:?} vs {b:?}"
        );
    }
    // Turning heat off drops the tint at once, also on the plan path.
    s.set_clock_ms(1_000.0);
    s.set_heat_enabled(false);
    let off = s.build_node_buffer(&ids(&[20, 21]), 1.0, 0.7, 1.9);
    assert_eq!(col(&off, 0), col(&off, 1), "heat off: no tint");
    // The LOD path too.
    s.set_heat_enabled(true);
    let near = s
        .build_node_buffer_lod(&ids(&[20, 21]), 1.0, 0.7, 1.9, [0.0; 3], 80, f32::INFINITY)
        .to_vec();
    assert_eq!(near.len() / NODE_STRIDE, 2);
}
