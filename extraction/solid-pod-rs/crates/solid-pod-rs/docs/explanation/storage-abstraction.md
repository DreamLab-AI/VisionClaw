# Storage abstraction

This page explains the `Storage` trait — why it has the shape it
does, which invariants implementors must preserve, and how the rest
of the crate composes on top of it.

## The trait

```rust
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn get   (&self, path: &str)                             -> Result<(Bytes, ResourceMeta), PodError>;
    async fn put   (&self, path: &str, body: Bytes, ct: &str)      -> Result<ResourceMeta, PodError>;
    async fn delete(&self, path: &str)                             -> Result<(), PodError>;
    async fn list  (&self, container: &str)                        -> Result<Vec<String>, PodError>;
    async fn head  (&self, path: &str)                             -> Result<ResourceMeta, PodError>;
    async fn exists(&self, path: &str)                             -> Result<bool, PodError>;
    async fn watch (&self, path: &str)
        -> Result<tokio::sync::mpsc::Receiver<StorageEvent>, PodError>;
}
```

Seven methods. No surface on top of these — every LDP, WAC,
Notifications, and auth concern in the crate composes on them.

## Why seven methods, not five, not ten

We deliberated on each method. The final shape is:

- **`get` / `put` / `delete`** — the core CRUD. No alternative.
- **`list`** — needed to render containers.
- **`head`** — separate because metadata reads are ~10× cheaper than
  bodies on most backends. Conflating with `get` would make every
  `HEAD` request transfer bodies from S3.
- **`exists`** — cheaper still. `wac::StorageAclResolver` probes
  multiple paths looking for an ACL; `exists` keeps the walk cheap.
- **`watch`** — Notifications are a first-class feature in Solid 2024+.
  The trait surface carries this concern rather than leaving it as
  an addon, so every backend must answer the same question: "how do
  I notice a change?"

Not included:

- **`copy`** — HTTP doesn't have a `COPY` verb. Client does GET+PUT.
- **`move`** — same.
- **`lock`** — WebDAV-style locks are out of scope; `If-Match` + strong
  ETags give us optimistic concurrency without a lock service.
- **`batch`** — would conflate trait contract with performance
  optimisation. Backends that want batching expose it separately.

## Invariants every backend must preserve

### Path canonicalisation

- All paths start with `/`.
- Container paths end with `/`.
- No `..` or `\0` may leak into the backend's underlying identifier.

### `put` atomicity

From an outside observer, `put` is atomic. Either `get` returns the
old state or the new state. No half-written bodies, no mismatched
`ResourceMeta`.

- `MemoryBackend`: achieved via `RwLock` write guard.
- `FsBackend`: achieved via `tempfile + rename(2)`.
- Custom: use whatever the underlying store offers (S3 `PutObject`
  is atomic; etcd has transactions; SQL has…).

### `put` event emission

On a brand-new path, emit `StorageEvent::Created(path)`. On an
existing path, emit `Updated(path)`. Never emit both. Never emit
neither.

### `delete` emits `Deleted`

Even if subsequent `get` / `exists` is already racing with other
callers.

### `list` returns direct children only

Nested descendants are **not** returned by `list("/c/")`. Sub-
containers carry a trailing `/`. Order is not specified; both
backends return lexicographic, but callers must not rely on it.

### `watch` delivers events for `path` and its descendants

A watcher registered on `/c/` sees events for `/c/a`, `/c/sub/b`,
etc. A watcher on a resource path (e.g. `/c/a`) sees only events for
that exact resource.

### Concurrency

All methods may be called from multiple tasks simultaneously. The
trait bounds (`Send + Sync + 'static`) enforce the shape; the
implementation must be thread-safe in practice.

## ETag contract

`ResourceMeta.etag` is a **strong validator** — identical bodies
produce identical etags. Both shipped backends use hex-encoded
SHA-256. Custom backends may use anything deterministic.

Why strong ETags?

- `If-Match` is meaningful only with strong validators
  ([RFC 7232 §3.1](https://datatracker.ietf.org/doc/html/rfc7232#section-3.1)).
- Notifications deduplication (a feature downstream consumers often
  want) relies on the hash property.

Why hex, not base64?

- Fits cleanly in HTTP headers without padding ambiguity.
- Trivially inspectable in logs.

## `ResourceMeta` design

```rust
pub struct ResourceMeta {
    pub etag:         String,
    pub modified:     chrono::DateTime<chrono::Utc>,
    pub size:         u64,
    pub content_type: String,
    pub links:        Vec<String>,  // pre-composed
}
```

The `links` field is intentional. Some backends (FS with JSON
sidecars) pre-compose Link values at write time. Others (memory,
S3) compute them on demand via `ldp::link_headers`. The trait
accepts both: callers should prefer `ldp::link_headers(path)` and
fall back to `meta.links` when the backend has richer information.

## Why `Bytes`, not `Vec<u8>` or `impl AsyncRead`

`bytes::Bytes` is cheap to clone (reference-counted), integrates with
every HTTP framework, and represents "owned, immutable bytes" —
exactly the right abstraction for resource bodies.

We considered `impl AsyncRead` for streaming uploads. Rejected:

- The ETag must be computed at write time. Streaming etag computation
  works but complicates the trait.
- LDP PATCH operations are not streaming; they load the whole body.
- Pods are for small-to-medium documents. For file storage with GB-
  scale bodies, you're in S3 territory, and the S3 backend can
  multipart under the hood without changing the trait.

## Why `watch` returns `mpsc`, not `broadcast`

`mpsc` gives the caller sole ownership of the receiver — events they
miss don't count against anyone else. `broadcast` would force every
watcher to share a bus and deal with `Lagged` errors.

The shipped backends internally multiplex many `mpsc` receivers from
one broadcast / notify stream — the complexity lives on the backend
side. Trait consumers always get the simple handle.

## Composition on top

| Concern                               | Composition |
|---------------------------------------|-------------|
| `wac::StorageAclResolver`             | walk up with `get` until an `.acl` is found |
| `ldp::LdpContainerOps::container_representation` | `list` + `render_container` |
| `notifications::WebSocketChannelManager::pump_from_storage` | `watch` + convert to AS2.0 |
| `notifications::WebhookChannelManager::pump_from_storage`   | same |
| HTTP handler for `PUT`                | `put`, compose with `meta.etag` + Link headers |
| HTTP handler for `PATCH`              | `get` → `Graph::parse_ntriples` → `apply_*_patch` → `put` |

## Testing

The shared conformance suite `tests/storage_trait.rs` instantiates
both `MemoryBackend` and `FsBackend` against the same scenarios:

- Round-trip `put` / `get` with binary and RDF bodies.
- `list` returns direct children only.
- `delete` emits `Deleted`.
- `watch` sees `Created` + `Updated` + `Deleted` in order.
- `exists` matches `head` results.
- `put` on existing path emits `Updated`, not `Created`.
- Invalid paths rejected consistently.

Custom backends should copy `tests/storage_trait.rs` and run it
against their own type. Anything that fails indicates a contract
divergence.

## When to write your own backend

- You want S3 / R2 / IPFS / Azure Blob.
- You want content-addressed storage (CAS) with deduplication.
- You have an existing key-value store you want to re-use.
- You need encryption-at-rest with a backend-specific key schema.

When **not** to write your own:

- You want to add a cache in front of an existing backend — do that
  at the HTTP layer, not inside the trait.
- You want custom ACL semantics — that belongs in the WAC layer, not
  the storage layer.

## See also

- [how-to/swap-storage-backends.md](../how-to/swap-storage-backends.md)
- [how-to/scale-with-s3-backend.md](../how-to/scale-with-s3-backend.md)
- [reference/api.md §Storage trait](../reference/api.md#storage-trait)
- [explanation/architecture-decisions.md](architecture-decisions.md#why-a-single-storage-trait)
