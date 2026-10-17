//! The frozen 52-byte V3 position record (ADR-2018) and Codescape's mapping onto it.
//!
//! The record layout is included *by source* from `visionclaw-protocol`'s
//! dependency-free fixtures module, the same way the Godot XR client does, so
//! Codescape cannot drift from the bytes the clients decode.
//!
//! Field mapping for a code graph (ADR-2143):
//!
//! | V3 field | meaning |
//! |---|---|
//! | `community_id` | Leiden community, stable-matched |
//! | `cluster_id` | structural cluster (crate / top-level directory) |
//! | `centrality` | importance 0..1 |
//! | `anomaly` | activity heat 0..1 (recent agent/human touches) |
//! | `sssp_distance`/`sssp_parent` | hop distance and parent from the current focus |

#[allow(dead_code, missing_docs)]
#[path = "../../../visionclaw-protocol/src/wire_fixtures.rs"]
mod fixtures;

pub use fixtures::{
    v3_frame, v5_frame, v5_sequence_of, NodeRecord, BROADCAST_ACK_BYTES, NODE_RECORD_BYTES,
    OPCODE_AGENT_ACTION, OPCODE_BROADCAST_ACK, PROTOCOL_V3, PROTOCOL_V5, V5_SEQ_BYTES,
};

use crate::graph::CodeNode;

/// Flag bit the XR client reads as "this node is an agent" (`docs/PROTOCOL-registry.md`).
pub const AGENT_FLAG: u32 = 0x8000_0000;

/// Build the wire record for one code node. Code nodes carry no flag bits.
pub fn record_for(
    node: &CodeNode,
    position: [f32; 3],
    velocity: [f32; 3],
    sssp: (f32, i32),
    heat: f32,
) -> NodeRecord {
    NodeRecord {
        wire_id: node.id & crate::ids::NODE_ID_MASK,
        position,
        velocity,
        sssp_distance: sssp.0,
        sssp_parent: sssp.1,
        cluster_id: node.cluster,
        anomaly: heat.clamp(0.0, 1.0),
        community_id: node.community,
        centrality: node.importance.clamp(0.0, 1.0),
    }
}

/// Build the wire record for an actor (agent or human) shown as an agent node.
pub fn actor_record(actor_id: u32, position: [f32; 3], heat: f32) -> NodeRecord {
    let mut r = NodeRecord::plain((actor_id & crate::ids::NODE_ID_MASK) | AGENT_FLAG, position);
    r.anomaly = heat.clamp(0.0, 1.0);
    r
}

/// Decode one 52-byte record (the inverse of [`NodeRecord::to_bytes`]), used by tests
/// and by the server's ack handling.
pub fn decode_record(bytes: &[u8]) -> Option<NodeRecord> {
    if bytes.len() < NODE_RECORD_BYTES {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    Some(NodeRecord {
        wire_id: u32_at(0),
        position: [f32_at(4), f32_at(8), f32_at(12)],
        velocity: [f32_at(16), f32_at(20), f32_at(24)],
        sssp_distance: f32_at(28),
        sssp_parent: i32::from_le_bytes(bytes[32..36].try_into().unwrap()),
        cluster_id: u32_at(36),
        anomaly: f32_at(40),
        community_id: u32_at(44),
        centrality: f32_at(48),
    })
}

/// Encode a client → server acknowledgement (`0x34`, 21 bytes): opcode, `u64` sequence,
/// `u32` node count, `u64` client timestamp. Decoded by [`decode_ack`].
pub fn encode_ack(sequence: u64, nodes: u32, timestamp_ms: u64) -> [u8; BROADCAST_ACK_BYTES] {
    let mut out = [0u8; BROADCAST_ACK_BYTES];
    out[0] = OPCODE_BROADCAST_ACK;
    out[1..9].copy_from_slice(&sequence.to_le_bytes());
    out[9..13].copy_from_slice(&nodes.to_le_bytes());
    out[13..21].copy_from_slice(&timestamp_ms.to_le_bytes());
    out
}

/// Decode an acknowledgement frame into `(sequence, nodes, timestamp_ms)`.
pub fn decode_ack(bytes: &[u8]) -> Option<(u64, u32, u64)> {
    if bytes.len() != BROADCAST_ACK_BYTES || bytes[0] != OPCODE_BROADCAST_ACK {
        return None;
    }
    Some((
        u64::from_le_bytes(bytes[1..9].try_into().unwrap()),
        u32::from_le_bytes(bytes[9..13].try_into().unwrap()),
        u64::from_le_bytes(bytes[13..21].try_into().unwrap()),
    ))
}

/// Agent action types on the `0x23` frame (`src/utils/binary_protocol.rs` `AgentActionType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AgentActionType {
    /// Read / Grep / Glob.
    Query = 0,
    /// Edit of an existing file.
    Update = 1,
    /// Write of a new file.
    Create = 2,
    /// A file removed in a delta.
    Delete = 3,
}

/// Encode an agent-action batch frame: `[0x23][u16 count]([u16 len][body])*`, body =
/// `[u32 agent][u32 node][u8 type][u32 ts][u16 duration_ms]`.
pub fn agent_action_frame(events: &[(u32, u32, AgentActionType, u32, u16)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + events.len() * 17);
    out.push(OPCODE_AGENT_ACTION);
    out.extend_from_slice(&(events.len() as u16).to_le_bytes());
    for &(agent, node, ty, ts, dur) in events {
        out.extend_from_slice(&15u16.to_le_bytes());
        out.extend_from_slice(&(agent | AGENT_FLAG).to_le_bytes());
        out.extend_from_slice(&node.to_le_bytes());
        out.push(ty as u8);
        out.extend_from_slice(&ts.to_le_bytes());
        out.extend_from_slice(&dur.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::graph::{Lang, NodeKind, Tier};

    #[test]
    fn record_matches_frozen_fixture_bytes() {
        let n = CodeNode {
            id: 42,
            key: Arc::from("k"),
            kind: NodeKind::Function,
            lang: Lang::Rust,
            label: Arc::from("f"),
            path: None,
            span: None,
            parent: 0,
            community: 7,
            cluster: 3,
            importance: 0.5,
            tier: Tier::L2,
            size: 0,
        };
        let r = record_for(&n, [1.0, 2.0, 3.0], [0.0, 0.0, 0.0], (2.0, 9), 0.25);
        let bytes = r.to_bytes();
        assert_eq!(bytes.len(), 52);
        assert_eq!(&bytes[0..4], &42u32.to_le_bytes());
        assert_eq!(&bytes[36..40], &3u32.to_le_bytes(), "cluster_id @36");
        assert_eq!(&bytes[40..44], &0.25f32.to_le_bytes(), "anomaly @40");
        assert_eq!(&bytes[44..48], &7u32.to_le_bytes(), "community_id @44");
        assert_eq!(&bytes[48..52], &0.5f32.to_le_bytes(), "centrality @48");
        assert_eq!(decode_record(&bytes), Some(r));
        let frame = v5_frame(99, &[r]);
        assert_eq!(frame[0], PROTOCOL_V5);
        assert_eq!(v5_sequence_of(&frame), Some(99));
        assert_eq!(frame.len(), 1 + 8 + 52);
    }

    #[test]
    fn actors_carry_the_agent_flag() {
        let r = actor_record(5, [0.0; 3], 2.0);
        assert_eq!(r.wire_id, 5 | AGENT_FLAG);
        assert_eq!(r.anomaly, 1.0);
        // An actor id with stray high bits is masked before the flag is set.
        let r = actor_record(5 | 0x4000_0000, [0.0; 3], 0.0);
        assert_eq!(r.wire_id, 5 | AGENT_FLAG);
    }

    #[test]
    fn code_records_never_carry_flag_bits() {
        // A node whose id somehow carries flag bits must still go out plain:
        // the XR client would otherwise render a code symbol as an agent.
        let mut n = CodeNode {
            id: 7 | AGENT_FLAG | 0x4000_0000,
            key: Arc::from("k"),
            kind: NodeKind::Function,
            lang: Lang::Rust,
            label: Arc::from("f"),
            path: None,
            span: None,
            parent: 0,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier: Tier::L2,
            size: 0,
        };
        let r = record_for(&n, [0.0; 3], [0.0; 3], (0.0, -1), 0.0);
        assert_eq!(r.wire_id, 7);
        n.id = crate::ids::NODE_ID_MASK;
        let r = record_for(&n, [0.0; 3], [0.0; 3], (0.0, -1), 0.0);
        assert_eq!(r.wire_id & AGENT_FLAG, 0);
        assert_eq!(r.wire_id, crate::ids::NODE_ID_MASK);
    }

    #[test]
    fn ack_round_trips_and_matches_fixture_shape() {
        let a = encode_ack(7, 1000, 123_456);
        assert_eq!(decode_ack(&a), Some((7, 1000, 123_456)));
        assert_eq!(decode_ack(&a[..20]), None);
        let fixture = fixtures::broadcast_ack_frame(7, 1000, 123_456);
        assert_eq!(
            &a[..],
            &fixture[..],
            "ack bytes must match the shared fixture"
        );
    }

    #[test]
    fn agent_action_frame_shape() {
        let f = agent_action_frame(&[(1, 2, AgentActionType::Update, 3, 4)]);
        assert_eq!(f[0], OPCODE_AGENT_ACTION);
        assert_eq!(u16::from_le_bytes([f[1], f[2]]), 1);
        assert_eq!(u16::from_le_bytes([f[3], f[4]]), 15);
        assert_eq!(f.len(), 3 + 2 + 15);
        assert_eq!(f[13], 1u8);
    }
}
