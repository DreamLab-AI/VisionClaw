//! The frozen VisionClaw position wire, as byte-exact constructors.
//!
//! VisionClaw streams graph-node positions to its web, Godot XR and Codescape
//! clients over one binary WebSocket. This crate is the single source of truth
//! for those bytes: every producer's encoder is tested for byte equality
//! against the functions here, and every consumer's decoder is tested against
//! the frames they build. It has no dependencies and is `no_std` + `alloc`
//! with the default `std` feature switched off.
//!
//! # Frame layouts
//!
//! All integers and floats are little-endian.
//!
//! | Frame | Layout | Size |
//! |---|---|---|
//! | V3 position | `[0x03]` (node record)* | `1 + 52n` |
//! | V5 position | `[0x05][u64 broadcast_seq]` (node record)* | `9 + 52n` |
//! | `0x34` acknowledgement | `[0x34][u64 sequence][u32 nodes_received][u64 timestamp_ms]` | 21 |
//! | `0x23` agent actions | `[0x23][u16 count]([u16 event_len][event bytes])*` | variable |
//!
//! One V3 node record ([`NodeRecord`], [`NODE_RECORD_BYTES`] = 52):
//!
//! ```text
//! [u32 wire_id][f32 x][f32 y][f32 z][f32 vx][f32 vy][f32 vz]
//! [f32 sssp_distance][i32 sssp_parent]
//! [u32 cluster_id][f32 anomaly][u32 community_id][f32 centrality]
//! ```
//!
//! One agent-action event body ([`ActionEvent`], 15-byte header + payload):
//!
//! ```text
//! [u32 source_agent_id][u32 target_node_id][u8 action_type]
//! [u32 timestamp][u16 duration_ms][payload bytes…]
//! ```
//!
//! # Invariants
//!
//! - The node record is frozen at 52 bytes. Changing it invalidates every
//!   consumer's alignment check at once, so it never changes in a 0.x release.
//! - The V5 envelope is purely additive: strip the version byte and the eight
//!   sequence bytes and the rest is byte-for-byte the V3 body.
//! - A position frame is all-or-nothing: a body that is not a whole multiple of
//!   52 bytes, an unknown version byte, or a V5 frame cut inside its sequence
//!   is rejected whole. A `0x23` batch is a tolerant prefix parse: complete
//!   events before a truncation are kept.
//! - The low 26 bits of `wire_id` ([`NODE_ID_MASK`]) are the node identifier;
//!   bits 26..=31 are node-class flags.
//! - The `0x34` acknowledgement and `0x23` batch carry no generic header.
//!
//! # Example
//!
//! ```
//! use visionclaw_wire::{v3_frame, v5_frame, v5_sequence_of, NodeRecord, V5_SEQ_BYTES};
//!
//! let records = [NodeRecord::plain(7, [1.0, 2.0, 3.0])];
//! let v5 = v5_frame(1234, &records);
//! assert_eq!(v5.len(), 1 + V5_SEQ_BYTES + 52);
//! assert_eq!(v5_sequence_of(&v5), Some(1234));
//! assert_eq!(&v5[1 + V5_SEQ_BYTES..], &v3_frame(&records)[1..]);
//! ```
//!
//! The malformed-frame constructors ([`v3_frame_misaligned`],
//! [`v5_frame_truncated_sequence`], [`agent_action_frame_truncated`] and
//! friends) exist so decoders can be tested against the documented rejection
//! and tolerance policies with the same bytes on every side.

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

/// Position-frame protocol version byte for the frozen 52-byte V3 record.
pub const PROTOCOL_V3: u8 = 0x03;

/// Position-frame protocol version byte for the V5 envelope, which prefixes a
/// V3 body with an 8-byte broadcast sequence number.
pub const PROTOCOL_V5: u8 = 0x05;

/// Size of one V3 node record. Frozen by ADR-2018; every fixture here asserts it.
pub const NODE_RECORD_BYTES: usize = 52;

/// Bytes the V5 envelope inserts between the version byte and the V3 body.
pub const V5_SEQ_BYTES: usize = 8;

/// Message-type byte prefixing a binary agent-action batch frame.
pub const OPCODE_AGENT_ACTION: u8 = 0x23;

/// Opcode of the client -> server position acknowledgement (ADR-2141).
pub const OPCODE_BROADCAST_ACK: u8 = 0x34;

/// Size of an acknowledgement frame: opcode + `u64` sequence + `u32` nodes + `u64` timestamp.
pub const BROADCAST_ACK_BYTES: usize = 21;

/// Fixed header size of a single agent-action event body, excluding payload.
pub const AGENT_ACTION_HEADER_BYTES: usize = 15;

/// Low 26 bits: the ephemeral wire node-id field (ADR-2024). Flag bits 26..=31
/// carry node class and are *not* part of the identifier.
pub const NODE_ID_MASK: u32 = 0x03FF_FFFF;

/// One node's worth of V3 wire state. Field order mirrors the wire exactly, so
/// [`NodeRecord::to_bytes`] is a straight-line serialisation with no reordering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeRecord {
    /// Wire id *including* any class flag bits already applied by the producer.
    pub wire_id: u32,
    /// World-space position `[x, y, z]`.
    pub position: [f32; 3],
    /// Velocity `[vx, vy, vz]`.
    pub velocity: [f32; 3],
    /// Shortest-path distance from the current source; `INFINITY` when unreached.
    pub sssp_distance: f32,
    /// Wire id of the shortest-path parent; `-1` when there is none.
    pub sssp_parent: i32,
    /// Structural cluster id.
    pub cluster_id: u32,
    /// Anomaly (or, for Codescape, activity heat) score.
    pub anomaly: f32,
    /// Community id.
    pub community_id: u32,
    /// Centrality / importance score.
    pub centrality: f32,
}

impl NodeRecord {
    /// A record with the given wire id and position, and the same neutral
    /// analytics tail the server writes when no analytics/SSSP entry exists:
    /// `(INFINITY, -1)` for SSSP and `Default::default()` for analytics.
    pub fn plain(wire_id: u32, position: [f32; 3]) -> Self {
        Self {
            wire_id,
            position,
            velocity: [0.0, 0.0, 0.0],
            sssp_distance: f32::INFINITY,
            sssp_parent: -1,
            cluster_id: 0,
            anomaly: 0.0,
            community_id: 0,
            centrality: 0.0,
        }
    }

    /// Serialise this record as its 52 little-endian wire bytes.
    pub fn to_bytes(self) -> [u8; NODE_RECORD_BYTES] {
        let mut out = [0u8; NODE_RECORD_BYTES];
        let mut off = 0usize;
        let mut put = |src: &[u8], off: &mut usize| {
            out[*off..*off + src.len()].copy_from_slice(src);
            *off += src.len();
        };
        put(&self.wire_id.to_le_bytes(), &mut off);
        for c in self.position {
            put(&c.to_le_bytes(), &mut off);
        }
        for c in self.velocity {
            put(&c.to_le_bytes(), &mut off);
        }
        put(&self.sssp_distance.to_le_bytes(), &mut off);
        put(&self.sssp_parent.to_le_bytes(), &mut off);
        put(&self.cluster_id.to_le_bytes(), &mut off);
        put(&self.anomaly.to_le_bytes(), &mut off);
        put(&self.community_id.to_le_bytes(), &mut off);
        put(&self.centrality.to_le_bytes(), &mut off);
        debug_assert_eq!(off, NODE_RECORD_BYTES);
        out
    }
}

/// A well-formed V3 position frame: `[0x03]` then the records back to back.
pub fn v3_frame(records: &[NodeRecord]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + records.len() * NODE_RECORD_BYTES);
    out.push(PROTOCOL_V3);
    for r in records {
        out.extend_from_slice(&r.to_bytes());
    }
    out
}

/// A well-formed V5 position frame carrying `broadcast_seq`.
pub fn v5_frame(broadcast_seq: u64, records: &[NodeRecord]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + V5_SEQ_BYTES + records.len() * NODE_RECORD_BYTES);
    out.push(PROTOCOL_V5);
    out.extend_from_slice(&broadcast_seq.to_le_bytes());
    for r in records {
        out.extend_from_slice(&r.to_bytes());
    }
    out
}

/// Read the broadcast sequence out of a V5 frame, or `None` if `bytes` is not a
/// long-enough V5 frame. Consumers use this to drive their freshness watermark.
pub fn v5_sequence_of(bytes: &[u8]) -> Option<u64> {
    if bytes.first().copied() != Some(PROTOCOL_V5) || bytes.len() < 1 + V5_SEQ_BYTES {
        return None;
    }
    let mut seq = [0u8; V5_SEQ_BYTES];
    seq.copy_from_slice(&bytes[1..1 + V5_SEQ_BYTES]);
    Some(u64::from_le_bytes(seq))
}

// ── Malformed / truncated position-frame fixtures (ADR-2019) ────────────────

/// A frame whose version byte is not a supported position version. Both
/// decoders must reject it as an unknown version rather than parse the body.
pub fn frame_with_unknown_version(version: u8, records: &[NodeRecord]) -> Vec<u8> {
    let mut out = v3_frame(records);
    out[0] = version;
    out
}

/// A V3 frame whose body is not a whole multiple of [`NODE_RECORD_BYTES`].
/// Both decoders must reject the whole frame as misaligned — a position frame
/// is all-or-nothing, unlike the tolerant `0x23` batch parse.
pub fn v3_frame_misaligned(records: &[NodeRecord], drop_tail_bytes: usize) -> Vec<u8> {
    let mut out = v3_frame(records);
    let keep = out.len().saturating_sub(drop_tail_bytes);
    out.truncate(keep.max(1));
    out
}

/// A V5 frame cut off inside its 8-byte sequence field. The sequence is part of
/// the envelope header, so this must be rejected as too short — never parsed as
/// a zero-sequence frame.
pub fn v5_frame_truncated_sequence(kept_seq_bytes: usize) -> Vec<u8> {
    let mut out = vec![PROTOCOL_V5];
    let seq = 42u64.to_le_bytes();
    out.extend_from_slice(&seq[..kept_seq_bytes.min(V5_SEQ_BYTES)]);
    out
}

/// An empty frame: no version byte at all.
pub fn empty_frame() -> Vec<u8> {
    Vec::new()
}

// ── Agent-action (0x23) fixtures ────────────────────────────────────────────

/// One agent→node action, matching the server's `AgentActionEvent` wire body.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionEvent {
    /// Wire id of the acting agent.
    pub source_agent_id: u32,
    /// Wire id of the node acted upon.
    pub target_node_id: u32,
    /// Action kind discriminant.
    pub action_type: u8,
    /// Event timestamp as the producer writes it.
    pub timestamp: u32,
    /// How long a client should render the action, in milliseconds.
    pub duration_ms: u16,
    /// Opaque payload appended after the 15-byte header.
    pub payload: Vec<u8>,
}

impl ActionEvent {
    /// A payload-free action event.
    pub fn plain(source_agent_id: u32, target_node_id: u32, timestamp: u32) -> Self {
        Self {
            source_agent_id,
            target_node_id,
            action_type: 0,
            timestamp,
            duration_ms: 250,
            payload: Vec::new(),
        }
    }

    /// The event body as it appears after its `u16` length prefix — i.e. the
    /// server's `AgentActionEvent::encode()` output with the message-type byte
    /// stripped, which is exactly what `encode_agent_actions` length-prefixes.
    pub fn to_body(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(AGENT_ACTION_HEADER_BYTES + self.payload.len());
        out.extend_from_slice(&self.source_agent_id.to_le_bytes());
        out.extend_from_slice(&self.target_node_id.to_le_bytes());
        out.push(self.action_type);
        out.extend_from_slice(&self.timestamp.to_le_bytes());
        out.extend_from_slice(&self.duration_ms.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// A well-formed `0x23` agent-action batch.
pub fn agent_action_frame(events: &[ActionEvent]) -> Vec<u8> {
    let mut out = vec![OPCODE_AGENT_ACTION];
    out.extend_from_slice(&(events.len() as u16).to_le_bytes());
    for e in events {
        let body = e.to_body();
        out.extend_from_slice(&(body.len() as u16).to_le_bytes());
        out.extend_from_slice(&body);
    }
    out
}

/// A `0x23` batch cut off `drop_tail_bytes` from the end. ADR-2019 records this
/// codec's policy as a **tolerant prefix parse**: complete events preceding the
/// truncation are accepted and returned; the incomplete tail is dropped. This
/// is deliberately different from the position frame's all-or-nothing rejection.
pub fn agent_action_frame_truncated(events: &[ActionEvent], drop_tail_bytes: usize) -> Vec<u8> {
    let mut out = agent_action_frame(events);
    let keep = out.len().saturating_sub(drop_tail_bytes);
    out.truncate(keep.max(3));
    out
}

/// A `0x23` batch whose declared count exceeds the events actually present.
/// The tolerant parse must return the events that are really there and must not
/// read past the buffer.
pub fn agent_action_frame_overstated_count(events: &[ActionEvent], claimed: u16) -> Vec<u8> {
    let mut out = agent_action_frame(events);
    out[1..3].copy_from_slice(&claimed.to_le_bytes());
    out
}

/// A `0x23` batch header with a count but no event bodies at all.
pub fn agent_action_frame_header_only(claimed: u16) -> Vec<u8> {
    let mut out = vec![OPCODE_AGENT_ACTION];
    out.extend_from_slice(&claimed.to_le_bytes());
    out
}

/// The client -> server acknowledgement of one position chunk (ADR-2141), exactly as the server
/// decodes it (`BinaryProtocol::decode_message`, opcode `0x34`): NO generic six-byte header,
///
/// ```text
/// [0x34][u64 sequence_LE][u32 nodes_received_LE][u64 timestamp_ms_LE]
/// ```
///
/// `sequence` is the V5 sequence of the chunk being acknowledged. The server marks the nodes of
/// that chunk as held by this client and builds its next chunk.
pub fn broadcast_ack_frame(sequence: u64, nodes_received: u32, timestamp_ms: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(BROADCAST_ACK_BYTES);
    v.push(OPCODE_BROADCAST_ACK);
    v.extend_from_slice(&sequence.to_le_bytes());
    v.extend_from_slice(&nodes_received.to_le_bytes());
    v.extend_from_slice(&timestamp_ms.to_le_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;

    #[test]
    fn node_record_is_exactly_the_frozen_52_bytes() {
        // ADR-2018's frozen record. If this ever changes, every consumer's
        // alignment check and every stored fixture is invalidated at once.
        assert_eq!(NodeRecord::plain(1, [0.0; 3]).to_bytes().len(), 52);
    }

    #[test]
    fn v3_and_v5_bodies_are_identical_after_their_headers() {
        // The V5 envelope must be purely additive: strip the version byte and
        // the 8 sequence bytes and the remainder is byte-for-byte the V3 body.
        let recs = [
            NodeRecord::plain(7, [1.0, 2.0, 3.0]),
            NodeRecord::plain(9, [4.0, 5.0, 6.0]),
        ];
        let v3 = v3_frame(&recs);
        let v5 = v5_frame(1234, &recs);
        assert_eq!(&v3[1..], &v5[1 + V5_SEQ_BYTES..]);
        assert_eq!(v5_sequence_of(&v5), Some(1234));
        assert_eq!(v5_sequence_of(&v3), None);
    }

    #[test]
    fn misaligned_fixture_is_not_a_record_multiple() {
        let recs = [NodeRecord::plain(1, [0.0; 3])];
        let bad = v3_frame_misaligned(&recs, 3);
        assert_ne!((bad.len() - 1) % NODE_RECORD_BYTES, 0);
    }

    #[test]
    fn truncated_v5_sequence_is_not_readable() {
        for kept in 0..V5_SEQ_BYTES {
            assert_eq!(v5_sequence_of(&v5_frame_truncated_sequence(kept)), None);
        }
    }

    #[test]
    fn action_event_body_is_header_plus_payload() {
        let mut e = ActionEvent::plain(1, 2, 3);
        assert_eq!(e.to_body().len(), AGENT_ACTION_HEADER_BYTES);
        e.payload = b"task".to_vec();
        assert_eq!(e.to_body().len(), AGENT_ACTION_HEADER_BYTES + 4);
    }

    #[test]
    fn overstated_count_keeps_the_real_bodies() {
        let events = [ActionEvent::plain(1, 2, 3)];
        let frame = agent_action_frame_overstated_count(&events, 9);
        assert_eq!(frame[0], OPCODE_AGENT_ACTION);
        assert_eq!(u16::from_le_bytes([frame[1], frame[2]]), 9);
        // Only one real body is present despite the claimed count of nine.
        assert_eq!(frame.len(), 3 + 2 + AGENT_ACTION_HEADER_BYTES);
    }

    #[test]
    fn broadcast_ack_frame_is_21_bytes_with_no_generic_header() {
        let f = broadcast_ack_frame(0x0102_0304_0506_0708, 9470, 1_700_000_000_123);
        assert_eq!(f.len(), BROADCAST_ACK_BYTES);
        assert_eq!(f[0], OPCODE_BROADCAST_ACK);
        assert_eq!(&f[1..9], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(&f[9..13], &9470u32.to_le_bytes());
        assert_eq!(&f[13..21], &1_700_000_000_123u64.to_le_bytes());
    }

    /// The same bytes are asserted in the web client's test (`BinaryWebSocketProtocol.ack.test.ts`):
    /// three encoders, one golden frame.
    #[test]
    fn broadcast_ack_golden_bytes() {
        let hex: String = broadcast_ack_frame(48_113, 2500, 1_700_000_000_123)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, "34f1bb000000000000c40900007b68e5cf8b010000");
    }
}

/// Compiles and runs the README's example under `cargo test --doc`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
