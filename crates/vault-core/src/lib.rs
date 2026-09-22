//! `vault-core` — the one implementation that reads the sovereign corpus.
//!
//! The estate used to have four doors that disagreed about the same vault: the
//! raw disk, a Node MCP bridge, Loom's served bundle and `VisionClaw`'s own
//! json-ld parse. This crate is the answer to that: a single parser, a single
//! vocabulary model, a single OKF projection and a single promotion state
//! machine, consumed by both the `vault` CLI and `VisionClaw`'s
//! `CorpusSource::LocalDirectory` ingest (PRD-sovereign-corpus Q11,
//! ADR-2113).
//!
//! # The model in one page
//!
//! * [`page::Page`] is a markdown file: YAML [`frontmatter::Frontmatter`] plus
//!   a body. Its **id** is its path under `pages/` without `.md`.
//! * [`vocabulary::Vocabulary`] (`ontology/vocabulary.yaml`) declares every
//!   legal frontmatter key and what it means in OWL. An undeclared key in
//!   `knowledge/` is a validation error; `working/` tolerates them.
//! * [`okf::OkfBlock`] projects the OKF v0.2 lifecycle and trust keys —
//!   `status`, `stale_after`, `generated`, `verified`, `sources`.
//! * [`graph::VaultGraph`] indexes the frontmatter links so retrieval can
//!   declare its blast radius per edge type.
//! * [`promotion`] enforces that only a human promotes and that a machine-found
//!   blocker cannot be waved through.
//! * [`proposal::PatchProposal`] is the contract-C4 payload of a forum 31402.
//!
//! # Reading a vault
//!
//! ```no_run
//! use vault_core::{page::load_vault, vocabulary::Vocabulary, graph::VaultGraph};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let (_, vocab) = Vocabulary::discover("/srv/visionGraph")?;
//! let vault = load_vault("/srv/visionGraph/knowledge")?;
//! let graph = VaultGraph::build(&vault, &vocab);
//!
//! for hit in graph.find("knowledge graph", Some("Class"), 5, false) {
//!     println!("{:.2} {}", hit.score, hit.id);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Parsing one page
//!
//! ```
//! use vault_core::page::Page;
//!
//! let page = Page::parse(
//!     "/v/pages/Knowledge Graph.md",
//!     "pages/Knowledge Graph.md",
//!     "Knowledge Graph",
//!     "---\ntype: Class\npublic: true\nis-a: [\"[[Content and Assets]]\"]\n---\nbody\n",
//! ).unwrap();
//!
//! assert_eq!(page.title(), "Knowledge Graph");
//! assert_eq!(page.resource("urn:ngm:class:"), "urn:ngm:class:knowledge-graph");
//! assert_eq!(page.frontmatter.wikilinks("is-a")[0].target, "Content and Assets");
//! ```
//!
//! # Feature flags
//!
//! * `migrate` — enables [`fences`], the json-ld fence reader used only by
//!   `vault migrate --fences-to-properties`. Delete the feature with the
//!   one-shot.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod code;
pub mod error;
#[cfg(feature = "migrate")]
pub mod fences;
pub mod frontmatter;
pub mod graph;
pub mod json;
pub mod okf;
pub mod page;
pub mod promotion;
pub mod proposal;
pub mod secrets;
pub mod slug;
pub mod vocabulary;

pub use code::CodeMap;
pub use error::{Result, VaultError};
pub use frontmatter::{Frontmatter, Wikilink};
pub use graph::VaultGraph;
pub use okf::{Actor, OkfBlock, Source, Stamp, Status};
pub use page::{load_vault, parse_page, Page, Vault, VaultKind};
pub use promotion::{Blocker, PromotionState, Transition};
pub use proposal::{Level, PatchProposal};
pub use vocabulary::Vocabulary;
