# Changelog

All notable changes to this crate are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha.1] — 2026-04-19

### Added
- Initial crate scaffold as a VisionClaw workspace member.
- `Storage` trait with associated `ResourceMeta` and `StorageEvent`
  types.
- `MemoryBackend` — in-memory backend for tests, backed by an
  `Arc<RwLock<HashMap<...>>>` with a broadcast channel for change
  events.
- `FsBackend` — filesystem backend rooted at a configurable directory,
  with SHA-256 ETags, `.meta` sidecar files for Link headers, and a
  `notify`-backed file watcher.
- `wac` module — JSON-LD ACL evaluator supporting `acl:agent`,
  `acl:agentClass`, `acl:mode`, `acl:accessTo`, `acl:default`,
  container inheritance, and the WAC-Allow response header.
- `ldp` module — container/resource distinction, Link header
  generation, slug resolution for POST-to-container.
- `webid` module — WebID profile document generation and validation.
- `auth::nip98` module — structural NIP-98 token verification (kind,
  tags, URL/method/payload matching, timestamp tolerance).
- `error::PodError` — crate-wide error type.
- Conformance test suite (`tests/storage_trait.rs`) covering Memory
  and FS backends.
- WAC smoke tests (`tests/wac_basic.rs`).
- `examples/standalone.rs` — minimal actix-web Solid pod server.

### Notes
- The Phase 1 NIP-98 module implements all structural checks. Schnorr
  signature verification is deferred to Phase 2, behind a feature flag
  that will gate the `k256` dependency.
- Notifications module (`src/notifications.rs`) ships with trait
  signatures and in-memory stubs. Full Solid Notifications Protocol
  (WebSocket, Webhook) is the Phase 2 deliverable.

[0.1.0-alpha.1]: https://github.com/DreamLab-AI/VisionClaw
