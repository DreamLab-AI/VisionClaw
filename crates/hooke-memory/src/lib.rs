//! Physics-structured latent memory for a frozen LLM.
//!
//! A Rust port of Memorilla's memory module (Apache-2.0, snap-stanford) in which the memory
//! slots occupy a relational field built from each collection's typed triples. See
//! `PORTING.md` for the port boundary and `PREREGISTRATION.md` for the frozen test.

pub mod data;
pub mod decoder;
pub mod field;
pub mod memory;
pub mod prompt;
pub mod store;
