# How to swap storage backends

**Goal:** replace `FsBackend` with `MemoryBackend` (or your own
backend) without touching any LDP/WAC/auth code.

## The pattern

Every LDP / WAC / Notifications API in solid-pod-rs takes a
`Storage` trait object. Keep your app code generic over `Arc<dyn
Storage>` and you can swap implementations without rebuilding.

```rust
use std::sync::Arc;
use solid_pod_rs::storage::Storage;

#[derive(Clone)]
struct AppState {
    storage: Arc<dyn Storage>,
}
```

## Option 1 — In-memory (for tests, ephemeral pods)

```rust
use solid_pod_rs::storage::memory::MemoryBackend;

let state = AppState { storage: Arc::new(MemoryBackend::new()) };
```

Drop this into any test and you get a real pod with watchers and
everything else — no filesystem required.

## Option 2 — Filesystem

```rust
use solid_pod_rs::storage::fs::FsBackend;

let backend = FsBackend::new("/var/lib/my-pod").await?;
let state = AppState { storage: Arc::new(backend) };
```

`FsBackend::new` creates the directory if it doesn't exist. Path
traversal is blocked at the boundary (`..` and `\0` both reject with
`InvalidPath`).

## Option 3 — S3

See [how-to/scale-with-s3-backend.md](scale-with-s3-backend.md) for
the full guide.

```rust
// Cargo.toml
// solid-pod-rs = { version = "0.2", features = ["s3-backend"] }
```

## Option 4 — Custom backend

Implement `Storage` on your type. Minimum surface:

```rust
use async_trait::async_trait;
use bytes::Bytes;
use solid_pod_rs::{
    error::PodError,
    storage::{ResourceMeta, Storage, StorageEvent},
};

pub struct MyBackend { /* ... */ }

#[async_trait]
impl Storage for MyBackend {
    async fn get(&self, path: &str) -> Result<(Bytes, ResourceMeta), PodError> { ... }
    async fn put(&self, path: &str, body: Bytes, ct: &str)  -> Result<ResourceMeta, PodError> { ... }
    async fn delete(&self, path: &str) -> Result<(), PodError> { ... }
    async fn list(&self, container: &str) -> Result<Vec<String>, PodError> { ... }
    async fn head(&self, path: &str) -> Result<ResourceMeta, PodError> { ... }
    async fn exists(&self, path: &str) -> Result<bool, PodError> { ... }
    async fn watch(&self, path: &str)
        -> Result<tokio::sync::mpsc::Receiver<StorageEvent>, PodError> { ... }
}
```

### Concurrency contract

- All methods are `async` and may be called from multiple tasks.
- The implementation must be `Send + Sync + 'static`.
- `put` must be atomic from an observer's standpoint — either
  `get` returns the old state or the new state, never a half-written
  body. The stock `MemoryBackend` uses `RwLock`; `FsBackend` writes
  to a temp file and renames.

### ETag contract

`ResourceMeta::etag` must be a strong validator — identical bodies
produce identical etags. Both shipped backends use hex-encoded
SHA-256. You can use anything deterministic.

### Path contract

- All paths are `/`-prefixed.
- Container paths end with `/`.
- `list(container)` returns *direct* children only. Sub-containers
  include their trailing `/`. See
  [reference/api.md §Storage::list](../reference/api.md#storagelist).

## Conformance tests

Every backend should pass the shared suite:

```bash
cargo test -p solid-pod-rs --test storage_trait
```

The suite exercises both `MemoryBackend` and `FsBackend` through the
same scenarios. Copy `tests/storage_trait.rs` and instantiate your
backend in place of one of the built-ins to verify parity.

## Common pitfalls

- **Forgot to emit `StorageEvent::Updated`** when `put` replaces an
  existing resource. Notification subscribers rely on this.
- **`list("/")` excludes the pod root itself.** Only children, by
  contract.
- **Blocking I/O in async methods** — wrap with
  `tokio::task::spawn_blocking` if your backend has synchronous APIs.

## See also

- [reference/api.md §Storage trait](../reference/api.md#storage-trait)
- [explanation/storage-abstraction.md](../explanation/storage-abstraction.md)
- [how-to/scale-with-s3-backend.md](scale-with-s3-backend.md)
