//! `vault` — the single door onto the sovereign corpus.
//!
//! One binary replaces the Python pipeline (`build.py` and its twelve
//! siblings), `vault-migrate`, the `ontology-bridge` MCP server and
//! `ontology-propose` (PRD-sovereign-corpus §3.2, ADR-2113). Agents reach the
//! corpus through this CLI and nothing else; humans use Obsidian.
//!
//! | subcommand | what it does |
//! |---|---|
//! | `validate` | OKF v0.2 conformance, vocabulary agreement, link integrity, the public gate, and the three migration-residue checks |
//! | `find` / `retrieve` / `tree` | the graph over frontmatter links, with per-edge-type expansion depths |
//! | `edit --expect` | guarded mutation; refused without a declared blast radius |
//! | `propose` | a contract-C4 `PatchProposal`, Whelk and `conflicts` as blockers, posted as a forum 31402 |
//! | `gate` / `conflicts` | the autonomous continuation gate and the semantic conflict detector |
//! | `build` | pages to one generation: asserted and inferred TTL, the scaffold / prose / search indexes, the page API, the OKF bundle, the JSON-LD context and the generation stamp |
//! | `migrate` | the one-shot fence-to-properties conversion, deleted after its run |
//!
//! # Layers
//!
//! Parsing, the vocabulary model, the OKF types and the promotion state
//! machine live in [`vault_core`], which `VisionClaw`'s ingest also links, so
//! the corpus is parsed by one implementation. This crate adds the
//! projections: [`model`] turns pages into ontology records, [`projection`]
//! applies the public build boundary, [`closure`] and [`whelk`] compute the two
//! closures, and [`build`] emits the bundle.
//!
//! # Example
//!
//! ```no_run
//! use vault::build;
//! use vault_core::vocabulary::Vocabulary;
//!
//! # fn main() -> anyhow::Result<()> {
//! let (_, vocab) = Vocabulary::discover("/srv/visionGraph")?;
//! let options = build::Options {
//!     vault_root: "/srv/visionGraph/knowledge".into(),
//!     out: "/srv/visionGraph/www".into(),
//!     repo_root: "/srv/visionGraph".into(),
//!     with_rvdb: false,
//!     with_markdown_mirror: false,
//!     // Also stage `working/`'s `public: true` pages under `publish/`.
//!     with_working_publish: true,
//!     // `None` keeps `publish/` inside the bundle.
//!     publish_out: None,
//!     embed_endpoint: build::rvdb::DEFAULT_ENDPOINT.into(),
//!     stale_after: None,
//! };
//! let report = build::run(&options, &vocab)?;
//! println!("{} classes in {}", report.class_count, report.generation.id);
//! for (vault, counts) in &report.published {
//!     println!("published {vault}: {}", counts.published);
//! }
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod build;
pub mod closure;
pub mod conflicts;
pub mod create;
pub mod edit;
pub mod gate;
pub mod migrate;
pub mod model;
pub mod nostr;
pub mod projection;
pub mod propose;
pub mod repair;
pub mod validate;
pub mod whelk;

pub use model::{ClassRecord, Corpus};
