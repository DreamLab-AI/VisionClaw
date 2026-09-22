//! The single error type every `vault-core` fallible operation returns.

use std::path::PathBuf;

/// Anything that can go wrong reading, parsing or validating the corpus.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// The file could not be read or written.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// The file the operation was attempted on.
        path: PathBuf,
        /// The underlying operating-system error.
        #[source]
        source: std::io::Error,
    },

    /// A page's YAML frontmatter block did not parse.
    #[error("{path}: malformed frontmatter: {source}")]
    Frontmatter {
        /// The offending page.
        path: PathBuf,
        /// The YAML parser's complaint.
        #[source]
        source: serde_yaml::Error,
    },

    /// `ontology/vocabulary.yaml` did not parse or is internally inconsistent.
    #[error("vocabulary {path}: {message}")]
    Vocabulary {
        /// The vocabulary file.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },

    /// A vault root does not have the expected layout.
    #[error("{root} is not a vault: {message}")]
    NotAVault {
        /// The directory that was offered as a vault root.
        root: PathBuf,
        /// Why it was rejected.
        message: String,
    },

    /// A json-ld fence could not be decoded during migration.
    #[error("{path}: malformed json-ld fence: {source}")]
    Fence {
        /// The offending page.
        path: PathBuf,
        /// The JSON parser's complaint.
        #[source]
        source: serde_json::Error,
    },
}

/// `Result` specialised to [`VaultError`].
pub type Result<T> = std::result::Result<T, VaultError>;

impl VaultError {
    /// Wrap a [`std::io::Error`] with the path it happened on.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
