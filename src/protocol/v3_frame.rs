//! V3 Binary Frame — Phase 3 wire format.
//!
//! Per ADR-02 D1 and PRD-02 §6, the protocol carries exactly one frame type:
//!
//! ```text
//! Frame header (8 bytes):
//!     u32 magic       = 0x56334630 ("V3F0", little-endian)
//!     u32 frame_id    (monotonic per connection, wraps at u32::MAX)
//!
//! Per node (28 bytes):
//!     u32 node_id     (full id including class flag bits, ADR-08 §D6)
//!     f32 pos_x, pos_y, pos_z
//!     f32 vel_x, vel_y, vel_z
//!
//! Frame trailer (4 bytes):
//!     u32 node_count  (count of nodes in this frame)
//! ```
//!
//! Total size: `12 + 28 * N` bytes.
//!
//! This module is the single source of encode/decode logic. The legacy
//! `binary_protocol.rs` V3 encoders (48-byte analytics extension) are
//! retained for non-broadcast paths; the broadcast path uses **this** 28-byte
//! `BinaryV3Frame` exclusively.

use bytemuck::{Pod, Zeroable};

/// Wire-format magic value: ASCII "V3F0" stored little-endian.
/// Reading the four bytes back yields `0x56334630` on x86_64.
pub const V3_MAGIC: u32 = 0x5633_4630;

/// Fixed header size (magic + frame_id).
pub const V3_HEADER_BYTES: usize = 8;

/// Fixed trailer size (node_count).
pub const V3_TRAILER_BYTES: usize = 4;

/// Per-node payload size.
pub const V3_NODE_BYTES: usize = 28;

/// Compile-time invariant: `NodeRow` matches the wire payload size.
const _: () = {
    assert!(std::mem::size_of::<NodeRow>() == V3_NODE_BYTES);
};

/// Single node entry in a V3 frame. Layout matches the wire format exactly
/// so a `&[NodeRow]` can be cast to a byte slice with `bytemuck::cast_slice`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct NodeRow {
    pub node_id: u32,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
}

impl NodeRow {
    #[inline]
    pub fn new(node_id: u32, pos: [f32; 3], vel: [f32; 3]) -> Self {
        Self { node_id, pos, vel }
    }
}

/// Errors produced by [`BinaryV3Frame::decode`].
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum V3DecodeError {
    #[error("frame too short: {0} bytes (minimum {minimum})", minimum = V3_HEADER_BYTES + V3_TRAILER_BYTES)]
    TooShort(usize),
    #[error("bad magic: 0x{actual:08X} (expected 0x{expected:08X})", expected = V3_MAGIC)]
    BadMagic { actual: u32 },
    #[error("length mismatch: header says {node_count} nodes ({expected_bytes} bytes), got {actual_bytes}")]
    LengthMismatch {
        node_count: u32,
        expected_bytes: usize,
        actual_bytes: usize,
    },
}

/// A decoded V3 frame.
///
/// On the encode side, callers may build a `BinaryV3Frame` then call
/// [`BinaryV3Frame::encode`] to serialise into a `Vec<u8>` for transport.
///
/// On the decode side, [`BinaryV3Frame::decode`] validates magic + length
/// invariants and returns an owned frame.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryV3Frame {
    pub magic: u32,
    pub frame_id: u32,
    pub nodes: Vec<NodeRow>,
    pub node_count: u32,
}

impl BinaryV3Frame {
    /// Construct a new frame. `node_count` is derived from `nodes.len()`.
    pub fn new(frame_id: u32, nodes: Vec<NodeRow>) -> Self {
        let node_count = nodes.len() as u32;
        Self {
            magic: V3_MAGIC,
            frame_id,
            nodes,
            node_count,
        }
    }

    /// Total wire size in bytes.
    #[inline]
    pub fn wire_size(&self) -> usize {
        V3_HEADER_BYTES + (self.nodes.len() * V3_NODE_BYTES) + V3_TRAILER_BYTES
    }

    /// Encode this frame into `buf`. Clears `buf` first.
    /// Uses zero-copy `bytemuck::cast_slice` for the node payload.
    pub fn encode_into(&self, buf: &mut Vec<u8>) {
        buf.clear();
        buf.reserve_exact(self.wire_size());
        buf.extend_from_slice(&V3_MAGIC.to_le_bytes());
        buf.extend_from_slice(&self.frame_id.to_le_bytes());
        // Cast Vec<NodeRow> -> &[u8] in one step (NodeRow is Pod, repr(C), 28-byte).
        buf.extend_from_slice(bytemuck::cast_slice::<NodeRow, u8>(&self.nodes));
        buf.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
    }

    /// Allocate and return a wire-format byte buffer.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.wire_size());
        self.encode_into(&mut buf);
        buf
    }

    /// Encode directly from a slice of `NodeRow` without owning a `Vec`.
    /// Hot path for `BroadcastActor`: snapshot → bytes with one allocation.
    pub fn encode_slice(frame_id: u32, nodes: &[NodeRow], buf: &mut Vec<u8>) {
        let total = V3_HEADER_BYTES + nodes.len() * V3_NODE_BYTES + V3_TRAILER_BYTES;
        buf.clear();
        buf.reserve_exact(total);
        buf.extend_from_slice(&V3_MAGIC.to_le_bytes());
        buf.extend_from_slice(&frame_id.to_le_bytes());
        buf.extend_from_slice(bytemuck::cast_slice::<NodeRow, u8>(nodes));
        buf.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
    }

    /// Decode a V3 frame from a byte slice. Validates magic and length.
    pub fn decode(bytes: &[u8]) -> Result<Self, V3DecodeError> {
        let min = V3_HEADER_BYTES + V3_TRAILER_BYTES;
        if bytes.len() < min {
            return Err(V3DecodeError::TooShort(bytes.len()));
        }

        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != V3_MAGIC {
            return Err(V3DecodeError::BadMagic { actual: magic });
        }
        let frame_id = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

        // Trailer is the last 4 bytes.
        let trailer_off = bytes.len() - V3_TRAILER_BYTES;
        let node_count = u32::from_le_bytes([
            bytes[trailer_off],
            bytes[trailer_off + 1],
            bytes[trailer_off + 2],
            bytes[trailer_off + 3],
        ]);

        let expected_body = (node_count as usize) * V3_NODE_BYTES;
        let actual_body = bytes.len() - V3_HEADER_BYTES - V3_TRAILER_BYTES;
        if expected_body != actual_body {
            return Err(V3DecodeError::LengthMismatch {
                node_count,
                expected_bytes: V3_HEADER_BYTES + expected_body + V3_TRAILER_BYTES,
                actual_bytes: bytes.len(),
            });
        }

        let body = &bytes[V3_HEADER_BYTES..trailer_off];
        // Safe cast: body length is an exact multiple of V3_NODE_BYTES (== sizeof NodeRow)
        // and NodeRow is Pod. `try_cast_slice` returns Err on alignment or length issues.
        let rows: &[NodeRow] = bytemuck::try_cast_slice(body).map_err(|_| {
            V3DecodeError::LengthMismatch {
                node_count,
                expected_bytes: V3_HEADER_BYTES + expected_body + V3_TRAILER_BYTES,
                actual_bytes: bytes.len(),
            }
        })?;

        Ok(BinaryV3Frame {
            magic,
            frame_id,
            nodes: rows.to_vec(),
            node_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rows(n: usize) -> Vec<NodeRow> {
        (0..n)
            .map(|i| {
                let f = i as f32;
                NodeRow::new(
                    i as u32 + 1,
                    [f, f + 0.5, f + 1.0],
                    [f * 0.01, f * 0.02, f * 0.03],
                )
            })
            .collect()
    }

    #[test]
    fn v3_frame_roundtrip_empty() {
        let frame = BinaryV3Frame::new(0, vec![]);
        let bytes = frame.encode();
        assert_eq!(bytes.len(), V3_HEADER_BYTES + V3_TRAILER_BYTES);
        let decoded = BinaryV3Frame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn v3_frame_roundtrip_small() {
        let frame = BinaryV3Frame::new(42, sample_rows(7));
        let bytes = frame.encode();
        // 8 header + 7*28 + 4 trailer = 208
        assert_eq!(bytes.len(), 8 + 7 * 28 + 4);
        let decoded = BinaryV3Frame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn v3_frame_size_5000_nodes() {
        let frame = BinaryV3Frame::new(123, sample_rows(5000));
        let bytes = frame.encode();
        // PRD-02 §6 reference figure: 140,012 bytes
        assert_eq!(bytes.len(), 140_012);
        let decoded = BinaryV3Frame::decode(&bytes).unwrap();
        assert_eq!(decoded.node_count, 5000);
        assert_eq!(decoded.nodes.len(), 5000);
        assert_eq!(decoded.nodes[0], frame.nodes[0]);
        assert_eq!(decoded.nodes[4999], frame.nodes[4999]);
    }

    #[test]
    fn v3_frame_magic_value() {
        // Spec literal in the docs is "0xV3F0" — the ASCII bytes V3F0 in LE order.
        // V=0x56, 3=0x33, F=0x46, 0=0x30 → little-endian u32 = 0x56334630
        assert_eq!(V3_MAGIC, 0x5633_4630);
        let frame = BinaryV3Frame::new(0, vec![]);
        let bytes = frame.encode();
        assert_eq!(&bytes[0..4], b"V3F0");
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut bytes = BinaryV3Frame::new(0, sample_rows(1)).encode();
        bytes[0] = b'X';
        let err = BinaryV3Frame::decode(&bytes).unwrap_err();
        assert!(matches!(err, V3DecodeError::BadMagic { .. }));
    }

    #[test]
    fn decode_rejects_short_buffer() {
        let err = BinaryV3Frame::decode(&[0u8; 4]).unwrap_err();
        assert!(matches!(err, V3DecodeError::TooShort(4)));
    }

    #[test]
    fn decode_rejects_length_mismatch() {
        let mut bytes = BinaryV3Frame::new(0, sample_rows(3)).encode();
        // Tamper with trailer to claim 4 nodes instead of 3.
        let len = bytes.len();
        bytes[len - 4..].copy_from_slice(&4u32.to_le_bytes());
        let err = BinaryV3Frame::decode(&bytes).unwrap_err();
        assert!(matches!(err, V3DecodeError::LengthMismatch { .. }));
    }

    #[test]
    fn encode_slice_matches_encode() {
        let rows = sample_rows(10);
        let frame = BinaryV3Frame::new(99, rows.clone());
        let v1 = frame.encode();
        let mut v2 = Vec::new();
        BinaryV3Frame::encode_slice(99, &rows, &mut v2);
        assert_eq!(v1, v2);
    }

    #[test]
    fn frame_id_preserved() {
        let frame = BinaryV3Frame::new(u32::MAX, sample_rows(2));
        let bytes = frame.encode();
        let decoded = BinaryV3Frame::decode(&bytes).unwrap();
        assert_eq!(decoded.frame_id, u32::MAX);
    }
}
