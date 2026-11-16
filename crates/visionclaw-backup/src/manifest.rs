//! The manifest written beside every run.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// File name of the manifest inside a run folder.
pub const MANIFEST_FILE: &str = "manifest.json";

/// What an artefact holds and how to check it after a restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtefactKind {
    /// An N-Quads dump of the whole Oxigraph dataset.
    OxigraphNquads {
        /// Number of quads in the dump.
        quads: u64,
    },
    /// A SQLite online-backup snapshot.
    Sqlite {
        /// Row count per user table at snapshot time.
        tables: BTreeMap<String, u64>,
        /// Digest over every row of every table (see [`crate::sqlite_content_digest`]).
        content_sha256: String,
    },
}

/// One file in a run, with the size and digest `verify` checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artefact {
    /// File name inside the run folder.
    pub name: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Lower-case hex SHA-256 of the file.
    pub sha256: String,
    /// Whether a missing store of this name fails the run.
    pub required: bool,
    /// Content facts used to check a restore.
    #[serde(flatten)]
    pub kind: ArtefactKind,
}

/// Everything a reader needs to check or restore a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest schema version.
    pub format: u32,
    /// UTC time the run started, RFC 3339.
    pub created_utc: String,
    /// Artefacts in the run, in name order.
    pub artefacts: Vec<Artefact>,
    /// Optional stores that were not present and so were skipped.
    pub absent_optional: Vec<String>,
}

impl Manifest {
    /// Current manifest schema version.
    pub const FORMAT: u32 = 1;
}
