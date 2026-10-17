//! Codescape domain model.
//!
//! This crate is the contract every other Codescape crate compiles against. It
//! is deliberately I/O-free: no filesystem, no network, no GPU, no graphicle.
//! `codescape-ingest` maps graphicle's output *into* these types;
//! `codescape-layout` positions them; `codescape-collab` tracks who is working
//! on them; `codescape-server` ships them over the wire.
//!
//! # Invariants (ADR-2143)
//!
//! * One graphicle node key maps to exactly one 26-bit wire id per epoch, and an
//!   id is never reused inside an epoch ([`ids::IdInterner`]).
//! * The V3 position record is the frozen 52-byte layout of ADR-2018; this
//!   crate includes the dependency-free fixture source from
//!   `visionclaw-protocol` by `#[path]`, so the bytes cannot drift ([`wire`]).
//! * Community labels are stable-matched across recomputes so colours do not
//!   flicker ([`community::stable_match`]).
//! * LOD aggregates get their own interned ids, never a member's ([`lod`]).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod community;
pub mod delta;
pub mod events;
pub mod graph;
pub mod ids;
pub mod lod;
pub mod wire;

pub use community::{stable_match, StableMatch};
pub use delta::GraphDelta;
pub use events::{CaptionEvent, FloorState, PresenceEvent, PresenceKind, QueryResult, ServerEvent};
pub use graph::{
    CodeEdge, CodeNode, Confidence, GraphSnapshot, Lang, NodeKind, Relation, Span, Tier,
};
pub use ids::{IdInterner, InternError, NODE_ID_MASK};
pub use wire::{record_for, NodeRecord, AGENT_FLAG, NODE_RECORD_BYTES, PROTOCOL_V3, PROTOCOL_V5};
