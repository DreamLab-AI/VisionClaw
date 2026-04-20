# Changelog

All notable changes to this crate are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.4.0-alpha.1 — 2026-04-20

### Added

- SSRF guard with IP classification plus allow/deny lists
  (`src/security/ssrf.rs`). Gated behind `security-primitives`.
- Dotfile allowlist (default `.acl`, `.meta`) enforced at the storage
  boundary (`src/security/dotfile.rs`).
- Legacy `solid-0.1` WebSocket notifications adapter for SolidOS
  data-browser compatibility (`src/notifications/legacy.rs`). Gated
  behind `legacy-notifications`.
- WAC `acl:origin` enforcement per the Web Access Control spec §4.3
  (`src/wac/origin.rs`). Gated behind `acl-origin`.
- DPoP `jti` replay cache per Solid-OIDC §5.2 and RFC 9449 §11.1
  (`src/oidc/replay.rs`). Gated behind `dpop-replay-cache`.
- Layered configuration loader (defaults → file → environment) with
  `JSS_*` environment variable mapping for drop-in operational
  parity (`src/config/`). Gated behind `config-loader`.
- Workspace split into `solid-pod-rs` (library) and
  `solid-pod-rs-server` (binary). The binary is a thin actix-web
  shell; the library no longer mounts HTTP routes.
- Reserved sibling crates for the v0.5.0 surface:
  `solid-pod-rs-activitypub`, `solid-pod-rs-git`,
  `solid-pod-rs-idp`, `solid-pod-rs-nostr`.
- Fresh gap analysis against the real JavaScriptSolidServer
  (97 rows, 76 % strict parity).
- ~30 integration tests and four criterion benchmarks covering the
  new surface.

### Changed

- The library crate no longer constructs `actix-web::HttpServer`;
  transport lives in `solid-pod-rs-server`.
- `verify_dpop_proof` now accepts an optional replay cache handle.
- `evaluate_access` now accepts an optional request origin.
- `NOTICE` restructured for clarity.

### Fixed

- Deduplicated residual `jss-v04` feature keys in `Cargo.toml`.

### Security

- Closes five of the six audit findings from the previous release
  cycle. The remaining finding — library-server coupling — is also
  resolved by the workspace split.

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
- `deny.toml` allowlist flipped to permit AGPL-3.0 in dependency graph.
- Consumers: if you're operating solid-pod-rs as a network service, AGPL
  §13 requires you to distribute corresponding source to your users.

## [0.3.0-alpha.1] — 2026-04-20

### Added — Sprint 3 parity close (ADR-053 §"JSS parity gate")

Every remaining `partial` or `missing` row in the JSS parity checklist
is resolved in this release. 67/67 rows are now either `present` (62)
or `explicitly-deferred` (5) with ADR-053 rationale. No `partial` and
no `missing` rows remain.

**WAC / ACL.**
- `wac::parse_turtle_acl` — Turtle ACL parser (accepts `@prefix`,
  `a`-shorthand, `;`-separated predicate lists, `,`-separated object
  lists). The `StorageAclResolver` now falls back to Turtle when the
  JSON-LD parse fails. Covers Solid's Turtle-authored `.acl` documents.
- `wac::serialize_turtle_acl` — canonical Turtle output round-trip.
- `AclDocument`, `AclAuthorization`, `IdOrIds`, `IdRef` now derive
  `Clone + Serialize` (required for `ProvisionPlan` and round-tripping).

**LDP.**
- `ldp::evaluate_preconditions` — RFC 7232 If-Match / If-None-Match,
  including wildcard (`*`) and comma-separated ETag lists. Returns a
  typed `ConditionalOutcome::{Proceed, PreconditionFailed, NotModified}`
  so callers can map to 412 / 304 without repeating logic.
- `ldp::parse_range_header` + `ldp::slice_range` — RFC 7233 byte
  ranges for binary resources. Supports `start-end`, open-ended
  (`start-`), and suffix-length (`-n`) forms; multi-range is rejected
  by design.
- `ldp::options_for` + `ACCEPT_PATCH` — OPTIONS response builder with
  correct `Allow` set per container/resource, `Accept-Post`,
  `Accept-Patch` (n3 / sparql-update / json-patch), and
  `Accept-Ranges: bytes`.
- `ldp::apply_json_patch` — RFC 6902 (`add`, `remove`, `replace`,
  `test`, `copy`, `move`) with JSON Pointer `-` append semantics.
- `PatchDialect::JsonPatch` + `patch_dialect_from_mime` now recognises
  `application/json-patch+json`.
- `PreferHeader::parse` now tolerates multi-IRI `include=` lists
  (`PreferMinimalContainer` + `PreferContainedIRIs` in one directive).

**WebID.**
- `webid::generate_webid_html_with_issuer` — emits `solid:oidcIssuer`
  for Solid-OIDC follow-your-nose discovery.
- `webid::extract_oidc_issuer` — pulls the issuer claim back out of a
  WebID HTML document (accepts string + `{@id:…}` object forms).

**Auth.**
- `auth::nip98::verify_schnorr_signature` — BIP-340 Schnorr signature
  verification over the canonical NIP-01 event hash. Gated behind the
  new `nip98-schnorr` feature (adds `k256` dep). Structural checks
  remain active in both configurations; verifier is invoked
  automatically from `verify_at` when the feature is on.
- `auth::nip98::compute_event_id` — canonical event-id hash per
  NIP-01, reused by the Schnorr verifier.

**Interop / discovery.**
- New `interop` module with:
  - `well_known_solid` — Solid Protocol §4.1.2 discovery document.
  - `webfinger_response` — RFC 7033 JRD with `acct:` and `https://`
    subjects; advertises OIDC issuer + WebID + pim:storage links.
  - `verify_nip05` — NIP-05 identifier verification, `_` wildcard
    fallback for root-of-domain names.
  - `dev_session` / `DevSession` — typed dev-mode bypass; the type is
    constructable only through this helper so callers can gate it
    behind their own env checks without exposing a header-based path.

**Provisioning.**
- New `provision` module with:
  - `ProvisionPlan` + `provision_pod` — declarative pod bootstrap:
    seeded containers (idempotent), WebID profile, optional root ACL,
    optional quota.
  - `QuotaTracker` — atomic reserve/release with `PreconditionFailed`
    on overrun; `None` quota means unlimited.
  - `check_admin_override` — constant-time shared-secret comparison
    that upgrades requests to the new `AdminOverride` marker type.

**Tests.**
- `tests/parity_close.rs` — 20 Sprint 3 integration tests exercising
  every newly-landed feature.
- `tests/interop_jss.rs` grew from 23 to 42 tests (+19 covering Turtle
  ACL, conditional requests, ranges, JSON Patch, OPTIONS response,
  WebID-OIDC, `.well-known/solid`, WebFinger, NIP-05, provisioning,
  quota, admin override, multi-include Prefer, dev session, JSON Patch
  dialect detection, and the `.meta` Link-rel invariant).
- `tests/schnorr_nip98.rs` — 2 Schnorr tests (feature-gated behind
  `nip98-schnorr`).

**Crate metadata.**
- Version bumped `0.2.0-alpha.1` → `0.3.0-alpha.1`.
- New `nip98-schnorr` feature in `Cargo.toml`.

### Explicitly-deferred (with rationale)

These five rows retain the `explicitly-deferred` status with an ADR-053
pointer so the parity checklist is never a moving target:

- **WebID-TLS** — legacy, superseded by Solid-OIDC + DPoP.
- **RDF/XML serialisation** — format negotiated; serialiser is a
  consumer-crate concern (avoids pulling in sophia/oxigraph).
- **S3 backend** — feature flag + `aws-sdk-s3` optional dep retained;
  concrete impl lives in VisionClaw pod-worker (backend boundary).
- **R2 / D1 / KV adapters** — Cloudflare-specific; consumer-crate.
- **RemoteStorage compatibility** — not on Solid Protocol path.

## [0.2.0-alpha.1] — 2026-04-19 (Phase 2 close)

### Added

- Full Solid Notifications Protocol 0.2 (WebSocket + Webhook channel
  managers, discovery document, exponential retry + fatal-drop).
- Solid-OIDC 0.1: DPoP proof verification, dynamic client registration,
  discovery, token introspection, WebID extraction (feature-gated
  under `oidc`).
- LDP PATCH: N3 (`solid:inserts`/`deletes`/`where`) and SPARQL-Update
  (`INSERT DATA` / `DELETE DATA` / `DELETE WHERE`).
- Prefer header parser + server-managed triple enforcement.
- ACL inheritance corpus (31 tests) + JSS interop corpus (23 tests).
- Count rolled from 27/67 to 48/67 present.

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

[0.1.0-alpha.1]: https://github.com/DreamLab-AI/VisionClaw
