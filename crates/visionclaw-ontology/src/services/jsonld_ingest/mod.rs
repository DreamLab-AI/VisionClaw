//! What survives of the JSON-LD ingest pipeline once the corpus became
//! frontmatter-only (ADR-2112) and ingest moved to `vault_core`:
//!
//! - [`expander`] — the canonical namespace constants (`OWL_NS`, `RDFS_NS`, …)
//!   the vocabulary registry mints IRIs against, and the term expander;
//! - [`shacl_gate`] — the process-wide SHACL gate mode the server configures
//!   at boot and the ontology-physics API reports;
//! - [`errors`] — the error type both share.
//!
//! The fence extractor, validator, triple emitter, import resolver and the
//! `ingest_page` pipeline that chained them had no production caller left and
//! were deleted.

pub mod errors;
pub mod expander;
pub mod shacl_gate;

pub use errors::{JsonLdIngestError, Result};
