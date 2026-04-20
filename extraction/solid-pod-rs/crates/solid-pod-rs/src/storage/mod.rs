//! Storage abstraction for Solid pods.
//!
//! The `Storage` trait is the sole interface between the Solid
//! protocol layer and concrete persistence backends. Implementations
//! must be `Send + Sync + 'static` and safe for concurrent access.
//!
//! Two backends ship with the crate:
//!
//! - `memory::MemoryBackend` — in-memory, ideal for tests.
//! - `fs::FsBackend` — filesystem-rooted, uses `tokio::fs` and
//!   `notify` for change events.

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::error::PodError;

#[cfg(feature = "fs-backend")]
pub mod fs;

#[cfg(feature = "memory-backend")]
pub mod memory;

/// Metadata describing a resource stored in a pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMeta {
    /// Strong ETag (typically hex-encoded SHA-256).
    pub etag: String,
    /// Last modification time, UTC.
    pub modified: chrono::DateTime<chrono::Utc>,
    /// Size of the body in bytes.
    pub size: u64,
    /// MIME type, e.g. `"application/ld+json"`.
    pub content_type: String,
    /// `Link` header values.
    ///
    /// Each entry is a single `Link` value (no outer commas), e.g.
    /// `<http://www.w3.org/ns/ldp#Resource>; rel="type"`.
    pub links: Vec<String>,
}

impl ResourceMeta {
    /// Construct a default `ResourceMeta` with the current UTC time.
    pub fn new(etag: impl Into<String>, size: u64, content_type: impl Into<String>) -> Self {
        ResourceMeta {
            etag: etag.into(),
            modified: chrono::Utc::now(),
            size,
            content_type: content_type.into(),
            links: Vec::new(),
        }
    }
}

/// Change events emitted by storage watchers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageEvent {
    /// A resource was created at the given path.
    Created(String),
    /// A resource was updated at the given path.
    Updated(String),
    /// A resource was deleted at the given path.
    Deleted(String),
}

/// The storage abstraction. Implementations back a Solid pod with
/// arbitrary persistence.
///
/// All paths use forward slashes and are rooted at `/`. Container
/// paths end with `/`.
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    /// Fetch a resource body + metadata.
    async fn get(&self, path: &str) -> Result<(Bytes, ResourceMeta), PodError>;

    /// Write (create-or-replace) a resource.
    ///
    /// Returns the new metadata including the computed ETag.
    async fn put(
        &self,
        path: &str,
        body: Bytes,
        content_type: &str,
    ) -> Result<ResourceMeta, PodError>;

    /// Delete a resource.
    async fn delete(&self, path: &str) -> Result<(), PodError>;

    /// List direct children of a container.
    ///
    /// Returned paths are relative to the container. A trailing `/`
    /// indicates a sub-container.
    async fn list(&self, container: &str) -> Result<Vec<String>, PodError>;

    /// Fetch metadata without the body.
    async fn head(&self, path: &str) -> Result<ResourceMeta, PodError>;

    /// Return whether a resource exists.
    async fn exists(&self, path: &str) -> Result<bool, PodError>;

    /// Register a watcher for a resource or container.
    ///
    /// The returned channel receives `StorageEvent` messages for
    /// changes under `path`. Closing the receiver detaches the
    /// watcher.
    async fn watch(
        &self,
        path: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<StorageEvent>, PodError>;
}
