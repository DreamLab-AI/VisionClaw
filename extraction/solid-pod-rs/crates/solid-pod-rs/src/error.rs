//! Crate-wide error type.
//!
//! All public APIs return `Result<T, PodError>`.

use thiserror::Error;

/// Errors emitted by solid-pod-rs.
#[derive(Debug, Error)]
pub enum PodError {
    #[error("resource not found: {0}")]
    NotFound(String),

    #[error("resource already exists: {0}")]
    AlreadyExists(String),

    #[error("access forbidden")]
    Forbidden,

    #[error("authentication required")]
    Unauthenticated,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("invalid content type: {0}")]
    InvalidContentType(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("ACL parse error: {0}")]
    AclParse(String),

    #[error("NIP-98: {0}")]
    Nip98(String),

    #[error("watch subsystem error: {0}")]
    Watch(String),

    #[error("backend error: {0}")]
    Backend(String),

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("unsupported: {0}")]
    Unsupported(String),
}

impl From<notify::Error> for PodError {
    fn from(e: notify::Error) -> Self {
        PodError::Watch(e.to_string())
    }
}
