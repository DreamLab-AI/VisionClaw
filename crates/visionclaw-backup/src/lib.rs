//! Backup, verify and restore for the VisionClaw persistent stores.
//!
//! One run writes a dated folder holding an N-Quads dump of the Oxigraph store
//! (all named graphs, so the provenance graph survives), one SQLite snapshot
//! per database taken with SQLite's online backup API, and a `manifest.json`
//! listing every artefact with its size and SHA-256. A run is built under a
//! dot-prefixed `.partial` name and renamed only once complete, so an
//! interrupted run never looks like a backup.
//!
//! # Oxigraph and a running server
//!
//! Oxigraph 0.4 on RocksDB allows one read-write process, and opening it
//! read-only beside a live writer is documented as undefined behaviour. This
//! crate therefore opens the store read-write: a store held by a running server
//! fails with a named error rather than being read unsafely. Point it at a
//! stopped store or at a directory produced by `Store::backup` (the server's
//! `ONTOLOGY_BACKUP_DIR` checkpoint).
//!
//! # Examples
//!
//! ```no_run
//! use std::path::PathBuf;
//! use visionclaw_backup::{backup, BackupRequest};
//!
//! # fn main() -> anyhow::Result<()> {
//! let request = BackupRequest {
//!     data_dir: PathBuf::from("/app/data"),
//!     oxigraph_dir: None,
//!     dest_root: PathBuf::from("/mnt/backups/visionclaw"),
//!     keep: 14,
//! };
//! let outcome = backup(&request)?;
//! println!("wrote {}", outcome.run_dir.display());
//! # Ok(())
//! # }
//! ```

mod manifest;
mod oxigraph_dump;
mod run;
mod sqlite_snapshot;

pub use manifest::{Artefact, ArtefactKind, Manifest, MANIFEST_FILE};
pub use run::{
    backup, latest_run, restore, verify, BackupOutcome, BackupRequest, OPTIONAL_SQLITE,
    REQUIRED_SQLITE,
};
pub use sqlite_snapshot::sqlite_content_digest;
