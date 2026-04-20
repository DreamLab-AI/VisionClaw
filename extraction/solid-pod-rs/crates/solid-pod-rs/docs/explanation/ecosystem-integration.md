# Ecosystem integration

solid-pod-rs is one component in a larger family of Solid-on-Rust
crates. This page explains how the pieces fit, who owns what, and
the integration points between them.

## The stack

```
  ┌──────────────────────────────────────────────────────────┐
  │                       Solid-Apps                         │
  │  (end-user apps: Nostrum Forum, clients, dashboards)     │
  └──────────────────────────────────────────────────────────┘
                          ▲
                          │ HTTP (LDP + WAC + Notifications)
                          ▼
  ┌──────────────────────────────────────────────────────────┐
  │                       URN-Solid                          │
  │   Rust HTTP service wiring solid-pod-rs into a runtime   │
  │   (actix/axum), adds provisioning, WebFinger, NIP-05     │
  └──────────────────────────────────────────────────────────┘
                          │
                          │ uses (Storage trait)
                          ▼
  ┌──────────────────────────────────────────────────────────┐
  │                     solid-pod-rs (this crate)            │
  │   Library: WAC, LDP, Notifications, NIP-98, Solid-OIDC   │
  └──────────────────────────────────────────────────────────┘
                          ▲
                          │ schema + vocabularies
                          │
  ┌──────────────────────────────────────────────────────────┐
  │                     solid-schema                         │
  │   Shared Rust types for Solid vocabularies (acl, ldp,    │
  │   solid, pim, dcterms, foaf)                             │
  └──────────────────────────────────────────────────────────┘
```

## Who owns what

### solid-pod-rs (this crate)

- The `Storage` trait.
- LDP semantics (containers, Link headers, PATCH).
- WAC evaluator.
- NIP-98 + Solid-OIDC verification.
- In-memory, filesystem, (planned) S3 backends.
- Solid Notifications 0.2 channel managers.
- Platform-independent. No actix, no worker, no hosting concerns.

**Does not** own: HTTP routing, TLS, provisioning, account lifecycle,
WebFinger, NIP-05, quota, billing.

### URN-Solid

URN-Solid is the consumer crate (a downstream workspace member) that
wraps solid-pod-rs in an actix/axum HTTP service with production
concerns layered on top:

- Request routing and middleware.
- Provisioning (`.provision` endpoint, WebID bootstrap).
- Tenancy (per-user pod paths).
- WebFinger / NIP-05 discovery.
- Quota enforcement.
- Metrics export.
- Admin API.

URN-Solid depends on solid-pod-rs as a library and never modifies its
API surface. When URN-Solid needs a feature from solid-pod-rs (e.g.
new PATCH dialect), it lands here first, with a
[parity-checklist](../../PARITY-CHECKLIST.md) update.

### solid-schema

Shared Rust types for Solid vocabularies. When solid-pod-rs defines a
struct that represents an ACL or LDP concept, that struct may later
graduate to solid-schema so both solid-pod-rs and URN-Solid share
the same canonical type.

Currently, solid-pod-rs defines its own lightweight types
(`AclAuthorization`, `ContainerMember`, etc.) to stay self-contained.
The migration path is:

1. Library is stable on in-crate types.
2. solid-schema publishes a type compatible with the in-crate shape.
3. solid-pod-rs re-exports or adopts the schema type in a minor-bump
   release.

### Solid-Apps

Applications that talk to a pod over HTTP. Do not link to
solid-pod-rs — they are HTTP clients.

Examples in the VisionClaw workspace:

- **Nostrum Forum** — a threaded discussion app. Reads + writes
  Solid resources for posts, uses NIP-98 for auth.
- **Pod Dashboard** — administrative UI for a pod operator.

The integration contract is *the Solid Protocol itself*. solid-pod-rs
is chosen because it supports the protocol, not because it exposes
a particular Rust API.

## Boundary contracts

### solid-pod-rs ↔ URN-Solid

URN-Solid depends on these public API surfaces:

- `storage::Storage` trait.
- `ldp::*` helpers (link_headers, PreferHeader, content negotiation,
  PATCH).
- `wac::evaluate_access*`, `wac_allow_header`, `StorageAclResolver`.
- `auth::nip98::verify`.
- `oidc::*` (feature `oidc`).
- `notifications::{WebSocketChannelManager, WebhookChannelManager,
  discovery_document}`.
- `PodError` variants for HTTP status mapping.

Anything else is an implementation detail and may change between
minor versions.

### solid-pod-rs ↔ solid-schema

Currently no hard dependency. Future boundary will be:

- `solid_schema::acl::Authorization` → `solid_pod_rs::wac::AclAuthorization`.
- `solid_schema::ldp::iri::*` → `solid_pod_rs::ldp::iri::*`.
- `solid_schema::as_ns::*` → `solid_pod_rs::notifications::as_ns::*`.

When solid-schema ships, solid-pod-rs will re-export the schema types
and deprecate the in-crate equivalents for one minor version.

## Integration patterns

### Pattern 1 — URN-Solid wraps solid-pod-rs

```rust
use solid_pod_rs::{storage::fs::FsBackend, Storage};
use urn_solid::Server;

let storage: Arc<dyn Storage> = Arc::new(FsBackend::new("/var/lib/pods").await?);
let server = Server::builder()
    .storage(storage)
    .bind("0.0.0.0:8080")
    .build();
server.run().await?;
```

URN-Solid composes the HTTP routing, auth middleware, provisioning,
and metrics.

### Pattern 2 — direct embedding

A service that isn't a pod but wants to expose a subset of Solid
semantics (e.g. a forum that hosts user avatars as LDP resources)
links to solid-pod-rs directly. The service chooses which endpoints
to expose — for an avatar subsystem, maybe only `GET` and `PUT` on a
single path prefix.

### Pattern 3 — storage-only reuse

A consumer that needs just a storage abstraction with strong ETags +
change events could use solid-pod-rs's storage module alone, ignoring
LDP / WAC / auth. Nothing in the crate forces the full Solid stack
on you.

## Cross-repo versioning

- solid-pod-rs uses semantic versioning. Breaking changes to public
  APIs cause a major-version bump.
- URN-Solid pins `solid-pod-rs = "0.2"` in Cargo.toml. Minor / patch
  updates are automatic.
- solid-schema will publish `1.x` once stabilised; solid-pod-rs will
  track its semver separately.

## Contribution flow

When adding a feature that spans the ecosystem:

1. Land library API in solid-pod-rs with tests. Update
   `PARITY-CHECKLIST.md`.
2. Expose it in URN-Solid via new endpoints or middleware.
3. Update solid-schema if the feature introduces new vocabulary
   types.
4. Document in the appropriate Diátaxis quadrant (this
   documentation site for the library; URN-Solid has its own site).

## See also

- [PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md) — current
  feature status.
- [README.md](../../README.md) — crate overview.
- [explanation/architecture-decisions.md](architecture-decisions.md) —
  why the library is framework-agnostic.
- [explanation/storage-abstraction.md](storage-abstraction.md) — the
  trait shape consumers build on.
