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
//!   crate re-exports it from [`visionclaw-wire`](https://docs.rs/visionclaw-wire),
//!   the same crate the VisionClaw server and XR client use, so the bytes
//!   cannot drift ([`wire`]).
//! * Community labels are stable-matched across recomputes so colours do not
//!   flicker ([`community::stable_match`]).
//! * LOD aggregates get their own interned ids, never a member's ([`lod`]).
//!
//! # Example
//!
//! Intern two symbols, build a validated snapshot with an edge between them,
//! then encode the callee's position as a V5 frame a VisionClaw client decodes.
//!
//! ```
//! use std::sync::Arc;
//! use codescape_model::{
//!     record_for, CodeEdge, CodeNode, Confidence, GraphSnapshot, IdInterner, Lang, NodeKind,
//!     Relation, Tier, NODE_RECORD_BYTES, PROTOCOL_V5,
//! };
//!
//! let mut ids = IdInterner::new(1);
//! let node = |id: u32, key: &str, label: &str| CodeNode {
//!     id,
//!     key: Arc::from(key),
//!     kind: NodeKind::Function,
//!     lang: Lang::Rust,
//!     label: Arc::from(label),
//!     path: Some(Arc::from("src/main.rs")),
//!     span: None,
//!     parent: 0,
//!     community: 1,
//!     cluster: 1,
//!     importance: 0.5,
//!     tier: Tier::L2,
//!     size: 0,
//! };
//! let main = ids.intern("src/main.rs::main").unwrap();
//! let run = ids.intern("src/main.rs::run").unwrap();
//! assert_eq!((main, run), (1, 2), "ids are handed out in order, never hashed");
//!
//! let snapshot = GraphSnapshot {
//!     repo: Arc::from("demo"),
//!     epoch: ids.epoch(),
//!     revision: 1,
//!     nodes: vec![node(main, "src/main.rs::main", "main"), node(run, "src/main.rs::run", "run")],
//!     edges: vec![CodeEdge {
//!         source: main,
//!         target: run,
//!         relation: Relation::Calls,
//!         weight: 1.0,
//!         confidence: Confidence::Extracted,
//!     }],
//!     ..Default::default()
//! };
//! snapshot.validate().unwrap();
//!
//! let record = record_for(&snapshot.nodes[1], [1.0, 2.0, 3.0], [0.0; 3], (1.0, main as i32), 0.0);
//! let frame = codescape_model::wire::v5_frame(7, &[record]);
//! assert_eq!(frame[0], PROTOCOL_V5);
//! assert_eq!(frame.len(), 1 + 8 + NODE_RECORD_BYTES);
//! ```

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

// Keep the README examples compiling (they are what crates.io shows).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
