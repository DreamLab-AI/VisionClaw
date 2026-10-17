//! Codescape (ADR-2143) speaks the XR client's wire unchanged.
//!
//! The three fixtures under `tests/fixtures/codescape/` were captured from a
//! running `codescape-server` (`/wss`, 2026-10-17, VisionClaw repository on
//! the CUDA engine) with a plain WebSocket client: the `initialGraphLoad` text
//! frame, the first V5 keyframe and a `0x23` agent-action frame produced by an
//! activity-sink hook event. This test decodes them with the exact functions
//! the Godot client uses, so a Codescape server pointed at by `XR_BACKEND_WS`
//! renders in XR without a client change.

use std::collections::HashMap;

use visionclaw_xr_gdext::binary_protocol::{
    decode_agent_action_frame, decode_position_frame_with_sequence, parse_initial_graph, NodeKind,
    NODE_ID_MASK, PROTOCOL_V5,
};

const INITIAL: &str = include_str!("fixtures/codescape/initial_graph_load.json");
const KEYFRAME: &[u8] = include_bytes!("fixtures/codescape/v5_keyframe.bin");
/// The first V5 frame after the hook event: the join keyframe predates the
/// actor, so the actor record first appears here (one record larger).
const AFTER_ACTION: &[u8] = include_bytes!("fixtures/codescape/v5_after_action.bin");
const AGENT_ACTION: &[u8] = include_bytes!("fixtures/codescape/agent_action_0x23.bin");

#[test]
fn initial_graph_load_parses_with_the_xr_parser() {
    let (edges, nodes) = parse_initial_graph(INITIAL).expect("initialGraphLoad parses");
    assert!(
        nodes.len() >= 1000,
        "file-tier projection carries the files: {}",
        nodes.len()
    );
    assert!(!edges.is_empty());
    // Every node carries the fields the XR store reads for label, type and size.
    for n in &nodes {
        assert!(!n.label.is_empty(), "node {} has a label", n.id);
        assert!(!n.node_type.is_empty(), "node {} has a node_type", n.id);
        assert_eq!(n.id & !NODE_ID_MASK, 0, "code node ids carry no flag bits");
    }
    // Code-graph node types are the Codescape kinds, not knowledge-graph ones.
    let types: std::collections::BTreeSet<&str> =
        nodes.iter().map(|n| n.node_type.as_str()).collect();
    assert!(types.contains("file"), "types seen: {types:?}");
    assert!(types.contains("directory"), "types seen: {types:?}");
    // Edge endpoints all resolve to announced nodes.
    let ids: std::collections::HashSet<u32> = nodes.iter().map(|n| n.id).collect();
    for e in &edges {
        assert!(
            ids.contains(&e.source) && ids.contains(&e.target),
            "edge {}->{} resolves",
            e.source,
            e.target
        );
        assert!(e.edge_type.is_some(), "edge carries its relation");
    }
}

#[test]
fn v5_keyframe_decodes_and_maps_fields_as_adr_2143_says() {
    assert_eq!(KEYFRAME[0], PROTOCOL_V5);
    let (seq, updates) = decode_position_frame_with_sequence(KEYFRAME).expect("V5 frame decodes");
    assert!(seq.is_some(), "V5 carries a sequence");
    assert_eq!((KEYFRAME.len() - 9) % 52, 0, "whole 52-byte records");

    let (_, nodes) = parse_initial_graph(INITIAL).unwrap();
    let announced: HashMap<u32, _> = nodes.iter().map(|n| (n.id, n)).collect();

    let mut agents = 0usize;
    let mut plain = 0usize;
    for u in &updates {
        assert!(
            u.position.iter().all(|v| v.is_finite()),
            "finite position for {}",
            u.node_id
        );
        match u.kind {
            NodeKind::Agent => agents += 1,
            NodeKind::Plain => {
                plain += 1;
                // Code records carry no flag bits and are all announced nodes.
                assert!(
                    announced.contains_key(&u.node_id),
                    "record {} was announced",
                    u.node_id
                );
                assert!(
                    (0.0..=1.0).contains(&u.centrality),
                    "centrality is importance 0..1"
                );
                assert!(
                    (0.0..=1.0).contains(&u.anomaly),
                    "anomaly is activity heat 0..1"
                );
            }
            other => panic!("unexpected kind {other:?} for {}", u.node_id),
        }
    }
    assert_eq!(
        plain,
        announced.len(),
        "the keyframe carries every announced node exactly once"
    );
    assert_eq!(
        agents, 0,
        "no actor existed when this join keyframe was sent"
    );

    // Once the hook event has created the actor, it rides the same frame as
    // an agent record while every code record stays plain.
    let (seq2, after) = decode_position_frame_with_sequence(AFTER_ACTION).expect("frame decodes");
    assert!(seq2 > seq, "sequence strictly increases");
    let agents_after: Vec<&_> = after.iter().filter(|u| u.kind == NodeKind::Agent).collect();
    assert_eq!(
        agents_after.len(),
        1,
        "exactly one actor in the frame after the action"
    );
    assert!(agents_after[0].position.iter().all(|v| v.is_finite()));
    assert!(
        agents_after[0].anomaly > 0.0,
        "the actor carries its activity heat"
    );
    assert!(after
        .iter()
        .filter(|u| u.kind == NodeKind::Plain)
        .all(|u| announced.contains_key(&u.node_id)));

    // Field mapping: community_id on the wire equals metadata.community in the
    // text frame for the same node (Leiden community, stable-matched).
    let raw: serde_json::Value = serde_json::from_str(INITIAL).unwrap();
    let json_community: HashMap<u32, u64> = raw["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| {
            Some((
                n["id"].as_u64()? as u32,
                n["metadata"]["community"].as_u64()?,
            ))
        })
        .collect();
    let mut checked = 0usize;
    for u in updates.iter().filter(|u| u.kind == NodeKind::Plain) {
        if let Some(&c) = json_community.get(&u.node_id) {
            assert_eq!(
                u64::from(u.community_id),
                c,
                "community mapping for {}",
                u.node_id
            );
            checked += 1;
        }
    }
    assert!(
        checked > 100,
        "community mapping checked on {checked} nodes"
    );
}

#[test]
fn agent_action_frame_targets_an_announced_node() {
    let actions = decode_agent_action_frame(AGENT_ACTION).expect("0x23 frame decodes");
    assert_eq!(actions.len(), 1);
    let a = &actions[0];
    // The acting agent carries the agent flag the store masks off (0x8000_0000).
    assert_ne!(
        a.source_agent_id & 0x8000_0000,
        0,
        "source carries the agent flag"
    );
    assert_eq!(
        a.target_node_id & !NODE_ID_MASK,
        0,
        "target is a plain node id"
    );
    assert!(a.action_type <= 3, "Query/Update/Create/Delete");
    let (_, nodes) = parse_initial_graph(INITIAL).unwrap();
    // The hook event touched src/main.rs; the target must be a file in the projection.
    let target = nodes
        .iter()
        .find(|n| n.id == a.target_node_id)
        .expect("target announced");
    assert_eq!(target.node_type, "file");
    // The acting agent is the actor record in the frame that followed the action,
    // so the XR work beam (render_store, 0x23 source lookup) finds its position.
    let (_, updates) = decode_position_frame_with_sequence(AFTER_ACTION).unwrap();
    let agent_ids: Vec<u32> = updates
        .iter()
        .filter(|u| u.kind == NodeKind::Agent)
        .map(|u| u.node_id)
        .collect();
    assert!(
        agent_ids.contains(&(a.source_agent_id & NODE_ID_MASK)),
        "agent {} is in the frame's agents {agent_ids:?}",
        a.source_agent_id & NODE_ID_MASK
    );
}
