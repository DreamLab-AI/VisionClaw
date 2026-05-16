//! Wire-level protocol modules.
//!
//! Phase 3 introduces [`v3_frame::BinaryV3Frame`], the single wire format
//! shared by the `/wss` WebSocket broadcast and the `GET /api/graph/positions`
//! REST endpoint (see ADR-02 D1, D4).

pub mod v3_frame;

pub use v3_frame::{BinaryV3Frame, NodeRow, V3DecodeError, V3_MAGIC, V3_NODE_BYTES};
