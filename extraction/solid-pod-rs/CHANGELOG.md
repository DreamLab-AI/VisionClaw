# Changelog

All notable changes to this crate are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.3.0-alpha.3 — 2026-04-20

### Licence migration (BREAKING)

- Licence changed from dual `MIT OR Apache-2.0` to `AGPL-3.0-only`.
- Inherited from the JavaScriptSolidServer (JSS) ecosystem covenant; JSS
  is AGPL-3.0, and solid-pod-rs preserves the network-service copyleft
  protection rather than weakening it with a permissive relicence.
- `LICENSE-MIT` and `LICENSE-APACHE` removed; `LICENSE` added with full
  AGPL-3.0 text.
- `Cargo.toml` `license` field updated to `"AGPL-3.0-only"`.
- `NOTICE` rewritten to document AGPL covenant + full provenance chain.
- `deny.toml` added with AGPL-3.0 on the allowlist for the dependency
  graph; the denylist posture remains deny-by-default for anything not
  on the allowlist.
- Consumers: if you're operating solid-pod-rs as a network service, AGPL
  §13 requires you to distribute corresponding source to your users.

## [0.3.0-alpha.2] — 2026-04-20

### Fixed
- **Provenance correction (documentation only).** The `0.3.0-alpha.1`
  release mis-attributed the upstream JSS reference to the wrong
  Solid server project (wrong repository, wrong licence, wrong
  author). The actual reference implementation is
  **JavaScriptSolidServer** (JSS), at
  <https://github.com/JavaScriptSolidServer/JavaScriptSolidServer>,
  maintained by the JavaScriptSolidServer contributors and licensed
  **AGPL-3.0-only**. No parity code or behaviour was affected — only
  the documentation narrative was wrong.
- `README.md`, `NOTICE`, `GAP-ANALYSIS.md`, `CONTRIBUTING.md`,
  `PARITY-CHECKLIST.md`, `Cargo.toml` corrected across every mention.
- `NOTICE` now carries an explicit "Licence relationship to
  JavaScriptSolidServer (JSS)" section documenting that solid-pod-rs
  is NOT a derivative work of JSS's JavaScript source, and that
  MIT/Apache-2.0 dual licensing is deliberate despite JSS being AGPL.
- The `references/` directory pointer now reads
  `javascript-solid-server/` across all prose; the vendored symlink
  has always resolved to the real JSS repo.

### Notes
- Zero code behaviour changes in this release.
- Consumers who already installed `v0.3.0-alpha.1` can upgrade to
  `v0.3.0-alpha.2` safely with no API or behavioural deltas.

## [0.3.0-alpha.1] — 2026-04-20

### Changed
- **Repository extraction**: solid-pod-rs is now a standalone
  public repository at <https://github.com/dreamlab-ai/solid-pod-rs>.
  It was previously a workspace member of VisionClaw
  (<https://github.com/DreamLab-AI/VisionClaw>, path
  `crates/solid-pod-rs/`). All source is verbatim from the VisionClaw
  snapshot taken on 2026-04-20. Commit history is preserved via
  `solid-pod-rs-history.bundle` (two commits corresponding to the
  Phase 1 and Phase 2 landings).
- `Cargo.toml` updated with the new repository URL, explicit
  `homepage` and `documentation`, `rust-version = "1.75"`, and
  `docs.rs` all-features metadata for generated documentation.

### Added
- `GAP-ANALYSIS.md` — exhaustive feature-by-feature comparison with
  the JavaScriptSolidServer (JSS) reference implementation, with
  deferred-to milestones for every gap.
- GitHub Actions CI workflow (`.github/workflows/ci.yml`) running
  `cargo check`, `cargo test` (default + all-features), `cargo clippy`,
  and `cargo fmt --check` on stable Rust.
- Standalone `NOTICE` with the full provenance chain: VisionClaw →
  community-forum-rs pod-worker → TypeScript pod-api, with credit to
  the JavaScriptSolidServer contributors.

### Notes
- No source behaviour has changed between `0.2.0-alpha.1` (the last
  in-VisionClaw snapshot) and this release. The version bump reflects
  the repository move, updated metadata, and standalone CI.
- The `solid-pod-rs-history.bundle` distributed alongside the initial
  commit contains the crate's subtree-split history. Consumers who
  want the pre-extraction history can `git fetch ./solid-pod-rs-history.bundle`
  into a clone.

## [0.2.0-alpha.1] — 2026-04-19 (as VisionClaw workspace member)

### Added (Phase 2)
- Full **LDP-BASIC PATCH** support:
  - N3 Patch (`text/n3`) via `ldp::apply_n3_patch` handling
    `solid:inserts`, `solid:deletes`, and `solid:where` clauses.
  - SPARQL-Update (`application/sparql-update`) via `ldp::apply_sparql_patch`
    parsed with `spargebra`; supports `INSERT DATA` and `DELETE DATA`.
- **Content negotiation** for Turtle, JSON-LD, N-Triples, and RDF/XML
  (RDF/XML negotiated, serialisation deferred to consumer crates):
  `ldp::negotiate_format` with q-value awareness.
- **Prefer header** handling (RFC 7240 + LDP 4.2.2):
  `PreferMinimalContainer`, `PreferContainedIRIs` via `PreferHeader::parse`.
- **Server-managed triples**: `dc:modified`, `stat:size`, `stat:mtime`,
  `ldp:contains` emitted; client attempts to write them blocked via
  `find_illegal_server_managed`.
- **Solid Notifications 0.2**:
  - `WebSocketChannelManager` — AS 2.0 events, 30s heartbeat, per-connection
    writers fed from a tokio broadcast channel.
  - `WebhookChannelManager` — AS 2.0 POSTs, 3× exponential retry on
    5xx, immediate drop on 4xx, dead-letter tracking.
  - Subscription discovery document at `.notifications`.
- **Solid-OIDC 0.1** (feature `oidc`):
  - `openidconnect 4.x` + `jsonwebtoken 9.x` dependencies.
  - DPoP-bound access token verification (`oidc::verify_access_token`
    + `oidc::verify_dpop_proof`), HS256/ES256/RS256.
  - RFC 7591 dynamic client registration (`oidc::register_client`).
  - OIDC Discovery document (`oidc::discovery_for`).
  - RFC 7662 token introspection (`oidc::IntrospectionResponse`).
  - WebID extraction from `webid` claim or url-shaped `sub`.
- **ACL inheritance corpus** (`tests/wac_inheritance.rs`) — 28 scenarios
  covering `acl:default`, agent-group membership, mixed public +
  authenticated rules, cascading denials.
- **JSS interop corpus** (`tests/interop_jss.rs`) — 22 fixture-driven
  tests covering Link headers, content-neg, ACL gating, LDP containment,
  error codes.
- **WAC extensions**: `acl:agentGroup` (vcard:Group) resolution via
  a pluggable `GroupMembership` trait, `acl:accessTo` exact + child
  match, ACL read via HTTP with `acl:Control` gating.
- **LDP extensions**: `Accept-Post` constant, `link_headers` emits all
  three media types, `<...>; rel="describedby"` for `.meta` sidecars,
  `<...>; rel="http://www.w3.org/ns/pim/space#storage"` for pod root.

### Changed
- `ldp` module expanded from ~400 lines to ~1,400 lines to cover the
  Phase 2 surface; `notifications` from stubs to full impl; `oidc`
  added as a feature-gated module (~670 lines).
- Internal `Graph` model added to back Turtle ⇄ JSON-LD ⇄ N-Triples
  conversion.

### Notes
- RDF/XML is negotiated but not serialised in the crate; that sits in
  a downstream crate (`solid-rdfxml`) to keep the dependency graph
  light.
- S3 backend is feature-flagged but implementation is deferred to
  the consumer crate that actually needs it (S3 adapter quality is
  orthogonal to protocol correctness).

## [0.1.0-alpha.1] — 2026-04-19

### Added
- Initial crate scaffold as a VisionClaw workspace member.
- `Storage` trait with associated `ResourceMeta` and `StorageEvent`
  types.
- `MemoryBackend` — in-memory backend for tests, backed by an
  `Arc<RwLock<HashMap<...>>>` with a broadcast channel for change
  events.
- `FsBackend` — filesystem backend rooted at a configurable directory,
  with SHA-256 ETags, `.meta.json` sidecar files for content type and
  Link values, and a `notify`-backed file watcher.
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

[0.3.0-alpha.2]: https://github.com/dreamlab-ai/solid-pod-rs/releases/tag/v0.3.0-alpha.2
[0.3.0-alpha.1]: https://github.com/dreamlab-ai/solid-pod-rs/releases/tag/v0.3.0-alpha.1
[0.2.0-alpha.1]: https://github.com/DreamLab-AI/VisionClaw
[0.1.0-alpha.1]: https://github.com/DreamLab-AI/VisionClaw
