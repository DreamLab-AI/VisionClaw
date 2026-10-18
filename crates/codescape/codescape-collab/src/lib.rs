//! Codescape collaboration state, with no I/O.
//!
//! This crate tracks who is working on the code graph, and turns what they say
//! or type into a typed [`Command`]. It does no I/O. The server feeds it
//! timestamps, events and utterances, and acts on what comes back.
//!
//! | Module | Role |
//! |---|---|
//! | [`actors`] | [`ActorRegistry`]: who is present, what they last touched, per-node activity heat |
//! | [`heat`] | Exponentially decaying per-node heat ([`heat::HeatField`]), shared by actors and the timeline |
//! | [`paths`] | [`PathMap`] prefix rewriting and [`resolve_paths`] from hook paths to file ids |
//! | [`floor`] | [`FloorArbiter`]: the single push-to-talk floor over a shared single-stream ASR |
//! | [`grammar`] | [`normalise_spoken`] and the deterministic [`parse_command`] grammar |
//! | [`llm`] | [`command_schema`] and [`command_from_llm`] for the language-model fallback |
//! | [`fuzzy`] | [`fuzzy_rank`]: identifier-aware trigram ranking of spoken names against labels |
//! | [`timeline`] | [`TimelineLog`]: a bounded log of presence and revision markers, for time scrubbing |
//!
//! # Invariants
//!
//! * A [`Command`] can only be built through [`parse_command`] or
//!   [`command_from_llm`] if it passes [`Command::validate`]. Language-model
//!   output that fails validation is rejected, never executed.
//! * The floor never has more than one holder, and nobody holds it while the
//!   shared ASR is busy.
//! * Heat is never above `1.0` and halves every [`heat::HALF_LIFE_MS`].
//!
//! # Example
//!
//! ```
//! use codescape_collab::{parse_command, Command, Target, FloorArbiter};
//! use codescape_model::FloorState;
//!
//! let cmd = parse_command("Um, who calls graph underscore service, two levels deep?").unwrap();
//! assert_eq!(cmd, Command::Callers { target: Target::Name("graph_service".into()), depth: 2 });
//!
//! let mut floor = FloorArbiter::new(30_000);
//! floor.request(7, 0);
//! assert_eq!(floor.request(8, 10), FloorState::Held { holder: 7, queue: vec![8] });
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod actors;
pub mod floor;
pub mod fuzzy;
pub mod grammar;
pub mod heat;
pub mod llm;
pub mod paths;
pub mod timeline;

pub use actors::{Actor, ActorKind, ActorRegistry};
pub use floor::{FloorArbiter, DEFAULT_HOLD_CAP_MS};
pub use fuzzy::{fuzzy_rank, split_identifier};
pub use grammar::{normalise_spoken, parse_command, Command, ParseError, Target};
pub use llm::{command_from_llm, command_schema};
pub use paths::{resolve_paths, PathMap, PathMapError};
pub use timeline::{Entry, TimelineEntry, TimelineLog};

// Keep the README examples compiling (they are what crates.io shows).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
