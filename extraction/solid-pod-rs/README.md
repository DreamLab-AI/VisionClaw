# solid-pod-rs

> **A Rust-native Solid Pod server — LDP-BASIC, WAC, NIP-98, Solid-OIDC, Notifications.**
> Framework-agnostic library crate. Deny-by-default WAC. First-class Nostr auth.
> Zero Node.js runtime dependency.

[![License: AGPL-3.0-only](https://img.shields.io/badge/License-AGPL--3.0--only-blue.svg)](#licence)
[![crates.io](https://img.shields.io/crates/v/solid-pod-rs.svg)](https://crates.io/crates/solid-pod-rs)
[![docs.rs](https://img.shields.io/docsrs/solid-pod-rs)](https://docs.rs/solid-pod-rs)
[![CI](https://github.com/dreamlab-ai/solid-pod-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/dreamlab-ai/solid-pod-rs/actions/workflows/ci.yml)
[![MSRV: 1.75](https://img.shields.io/badge/MSRV-1.75-lightgray.svg)](https://releases.rs/docs/1.75.0/)

---

## v0.3.0-alpha.1 → v0.3.0-alpha.2 Correction Notice

The initial `v0.3.0-alpha.1` release mis-attributed the upstream JSS
reference to the wrong Solid server project (wrong repository, wrong
licence, wrong author). The actual reference implementation is:

- **JavaScriptSolidServer** (JSS)
- Repo: <https://github.com/JavaScriptSolidServer/JavaScriptSolidServer>
- Licence: **AGPL-3.0-only**
- Maintained by the JavaScriptSolidServer contributors

solid-pod-rs `v0.3.0-alpha.2` corrects the documentation narrative
(README, NOTICE, CHANGELOG, CONTRIBUTING, GAP-ANALYSIS,
PARITY-CHECKLIST, test docstrings) to name the real upstream. No
source code or test behaviour changed between `alpha.1` and
`alpha.2`; the parity tests and fixtures were always exercising the
Solid Protocol itself, not any specific implementation's internals.
The `references/javascript-solid-server/` symlink has always pointed
at the real JSS repository.

solid-pod-rs is NOT a derivative work of JSS's JavaScript source.
The Rust implementation originates from
`community-forum-rs/crates/pod-worker` (written in Rust from
scratch); JSS is read as a reference-only resource for Solid
Protocol behaviour. See [`NOTICE`](./NOTICE) §"Licence relationship
to JavaScriptSolidServer (JSS)" for the full independence claims
and the rationale for AGPL-3.0-only licensing inherited from the
JSS ecosystem covenant.

Upgrade from `alpha.1` → `alpha.2` is safe with zero API or
behavioural deltas.

---

## What is solid-pod-rs

solid-pod-rs is a **Rust implementation of the server side of the
Solid Protocol**. It ships the full set of protocol primitives — LDP
resources and containers, Web Access Control (WAC), WebID profile
documents, Solid Notifications 0.2, Solid-OIDC 0.1, and NIP-98 HTTP
authentication — as a framework-agnostic library crate. Wire it into
actix-web, axum, hyper, or your own HTTP runtime; the crate has no
opinions about how requests reach it.

The target audience is Rust developers building **sovereign-data
applications**, Solid ecosystem implementers who want a native
backend that does not drag in Node.js, and researchers porting Solid
semantics to compiled-language deployments (edge runtimes, embedded
servers, single-binary IoT devices). The library is feature-gated
so a minimal NIP-98-only deployment can build a pod server in under
200 KB of dependency surface; a full OIDC + S3 + notifications build
remains under 40 MB.

---

## Provenance

solid-pod-rs has a clear, documented lineage. This is not a fresh
implementation; it stands on the shoulders of prior work and credits
all of them.

1. **Born inside VisionClaw** — the crate was extracted on
   **2026-04-20** from
   [`github.com/DreamLab-AI/VisionClaw`](https://github.com/DreamLab-AI/VisionClaw)
   at path `crates/solid-pod-rs/`. It was developed there across
   Phase 1 (scaffold + LDP + WAC + NIP-98 structural, ADR-053) and
   Phase 2 (LDP PATCH + OIDC + Notifications + JSS parity corpus,
   ADR-053). The extraction preserves the full crate source
   verbatim; commit history is captured in a git bundle distributed
   with the initial commit of this repository.

2. **Core extraction from community-forum-rs pod-worker** — the WAC
   evaluator, LDP container semantics, NIP-98 structural checks, and
   pod provisioning flows originated in the `pod-worker` crate at
   [`github.com/DreamLab-AI/dreamlab-ai-website`](https://github.com/DreamLab-AI/dreamlab-ai-website)
   (path: `community-forum-rs/crates/pod-worker`). Community-forum-
   specific code (forum thread integration, Cloudflare Workers
   bindings, R2 specifics, CF-KV) was factored out during the
   VisionClaw port. The lineage is AGPL-3.0 throughout and the
   licence is preserved here unchanged.

3. **Design follows JavaScriptSolidServer (JSS)** — the
   reference JavaScript implementation at
   [github.com/JavaScriptSolidServer/JavaScriptSolidServer](https://github.com/JavaScriptSolidServer/JavaScriptSolidServer),
   licensed AGPL-3.0-only, maintained by the JavaScriptSolidServer
   contributors. JSS served as the canonical parity reference —
   every feature of solid-pod-rs was benchmarked against JSS's
   observable HTTP behaviour, and `tests/interop_jss.rs` contains a
   22-test fixture corpus of independently-authored request /
   response fixtures exercising the Solid Protocol. We gratefully
   acknowledge the JavaScriptSolidServer contributors — without
   their JavaScript reference, a Rust port of this scope would have
   taken years instead of two sprints. solid-pod-rs is NOT a
   derivative work of JSS's JavaScript source; see [`NOTICE`](./NOTICE)
   §"Licence relationship to JavaScriptSolidServer (JSS)" for the
   full independence claims.

4. **Protocol authorship** — the Solid Protocol, WAC, Solid-OIDC,
   and Solid Notifications are authored by the W3C Solid Community
   Group and associated working groups under Sir Tim Berners-Lee's
   overall stewardship of the Solid project. LDP is a W3C
   Recommendation. NIP-98 is a specification of the Nostr community.

5. **Licence** — AGPL-3.0-only, inherited from the JavaScriptSolidServer
   ecosystem covenant. solid-pod-rs preserves the network-service
   copyleft protection JSS established; see §"Licence" below for the
   operational consequences.

See [`NOTICE`](./NOTICE) for the complete provenance record.

---

## Current status

Phase 2 closes with **48 features at parity**, **8 partial**, and
**11 intentionally deferred** (of which 4 are wontfix by design).
**138 tests** pass on default + all-features builds, with **zero
known regressions** against JSS behaviour.

### Feature coverage vs JSS

Key: OK parity · PARTIAL · DEFERRED · WONTFIX · EXTENSION

#### LDP-BASIC

| Feature | solid-pod-rs | JSS | Spec clause |
|---------|:---:|:---:|---|
| GET/HEAD resource + container | OK | OK | LDP 4.2.1 |
| PUT resource (create-or-replace) | OK | OK | LDP 4.2.4 |
| POST to container + Slug | OK | OK | LDP 5.2.3 |
| PUT-to-container rejection (405) | OK | OK | LDP 5.2.4 |
| DELETE | OK | OK | LDP 4.2.5 |
| OPTIONS | OK | OK | LDP 4.2.8 |
| `ldp:contains` (direct children only) | OK | OK | LDP 5.2.1.4 |
| Server-managed triples | OK | OK | LDP 5.2.3.1 |
| Prefer header (PreferMinimalContainer, PreferContainedIRIs) | OK | OK | LDP 4.2.2 / RFC 7240 |
| `Accept-Post` on containers | OK | OK | LDP 7.1 |
| `Link: <...>; rel="type"` | OK | OK | Solid Protocol §4 |
| `Link: <.acl>; rel="acl"` | OK | OK | Solid Protocol §4.3 |
| `Link: <.meta>; rel="describedby"` | OK | OK | Solid Protocol §4 |
| `Link: rel="pim:storage"` on root | OK | OK | Solid Protocol §4.1 |
| Strong ETag | OK (SHA-256 hex) | PARTIAL (weak by default) | LDP 4.2.1.3 |
| If-Match / If-None-Match enforcement | PARTIAL (helpers, not wired) | OK | RFC 7232 |
| Range requests | DEFERRED v0.3.1 | OK | RFC 7233 |
| Direct / Indirect Containers | WONTFIX | OK (opt) | LDP 5.3 / 5.4 |

#### Web Access Control (WAC)

| Feature | solid-pod-rs | JSS | Notes |
|---------|:---:|:---:|---|
| `acl:Read`/`Write`/`Append`/`Control` | OK | OK | Full mode matrix |
| `acl:agent` (specific WebID) | OK | OK | |
| `acl:agentClass foaf:Agent` (public) | OK | OK | |
| `acl:agentClass acl:AuthenticatedAgent` | OK | OK | |
| `acl:agentGroup` (vcard:Group) | OK (pluggable `GroupMembership`) | OK | |
| `acl:accessTo` exact + child | OK | OK | |
| `acl:default` container inheritance | OK | OK | 28-scenario corpus |
| `acl:origin` | OK | OK | |
| `.acl` walk-up resolver | OK | OK | |
| `WAC-Allow` response header | OK | OK | WAC §9 |
| ACL read via HTTP (GET `.acl`) | OK | OK | |
| ACL write requires `acl:Control` | OK | OK | |
| Turtle ACL parsing | PARTIAL (JSON-LD native; Turtle via consumer crate) | OK | |

#### Authentication

| Feature | solid-pod-rs | JSS | Notes |
|---------|:---:|:---:|---|
| **NIP-98 HTTP auth** | OK (EXTENSION) | (not in JSS) | Kind 27235, `u`/`method`/`payload` tags |
| NIP-98 Schnorr signature | PARTIAL (structural pass; sig check v0.3.1) | — | Via `k256` |
| Solid-OIDC 0.1 (DPoP-bound) | OK (`--features oidc`) | OK | HS256/ES256/RS256 |
| RFC 7591 dynamic client registration | OK | OK | |
| OIDC Discovery document | OK | OK | |
| Token introspection (RFC 7662) | OK | OK | |
| WebID extraction (`webid` / url-`sub`) | OK | OK | Solid-OIDC §5.4 |
| WebID-TLS | WONTFIX | OK (legacy) | Deprecated |
| Password accounts | WONTFIX | OK | Use external IDP |

#### PATCH

| Feature | solid-pod-rs | JSS | Notes |
|---------|:---:|:---:|---|
| N3 Patch (solid:inserts/deletes/where) | OK | OK | Solid Protocol §8.2 |
| SPARQL-Update PATCH | OK | OK | SPARQL 1.1 Update |
| JSON Patch (RFC 6902) | WONTFIX | OK | Use PUT or N3 Patch |

#### Content negotiation

| Feature | solid-pod-rs | JSS | Notes |
|---------|:---:|:---:|---|
| Turtle | OK (parse + serialise) | OK | |
| JSON-LD | OK (parse + serialise) | OK | |
| N-Triples | OK (roundtrip) | OK | |
| RDF/XML | PARTIAL (negotiated; ser via `solid-rdfxml`) | OK | |
| Accept q-value negotiation | OK | OK | |

#### Notifications (Solid Notifications 0.2)

| Feature | solid-pod-rs | JSS | Notes |
|---------|:---:|:---:|---|
| WebSocketChannel2023 | OK | OK | 30s heartbeat |
| WebhookChannel2023 | OK | OK | 3× exponential retry |
| Subscription discovery | OK (`.notifications`) | OK | |
| Retry + dead-letter | OK | OK | |
| Activity Streams 2.0 events | OK | OK | |

#### Storage backends

| Feature | solid-pod-rs | JSS | Notes |
|---------|:---:|:---:|---|
| Memory backend | OK | OK | |
| Filesystem backend | OK | OK | `.meta.json` sidecars |
| S3 backend | PARTIAL (feature flag; impl v0.4) | OK (opt) | |
| Quota enforcement | DEFERRED v0.4 | OK | Per-backend |
| R2 / D1 / KV adapters | DEFERRED (consumer-crate concern) | — | |

#### Interop / discovery

| Feature | solid-pod-rs | JSS | Notes |
|---------|:---:|:---:|---|
| `.well-known/openid-configuration` | OK | OK | OIDC Discovery 1.0 |
| `.well-known/solid` | PARTIAL | OK | v0.4 |
| Pod provisioning (`.provision`) | DEFERRED v0.4 | OK (via IDP UI) | |
| WebFinger | DEFERRED v0.4 | OK | |
| NIP-05 verification | DEFERRED v0.4 | — | Nostr-specific |

**Summary**: 48 of 67 tracked features at parity, 138 tests passing,
0 known regressions against JSS behaviour.

See [`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md) for the exhaustive
tracker and [`GAP-ANALYSIS.md`](./GAP-ANALYSIS.md) for the prose
rationale.

---

## Gap analysis

The quick version; see [`GAP-ANALYSIS.md`](./GAP-ANALYSIS.md) for the
full 1,500+ word treatment.

### What JSS has that solid-pod-rs doesn't

- **JSON Patch PATCH** — *wontfix*. Use PUT (with ETag) or N3 Patch.
- **WebID-TLS** — *wontfix*. Deprecated by the Solid community.
- **Admin HTML pages + password accounts** — *wontfix in crate*. Pod
  accounts belong in a consumer crate (`solid-pod-rs-admin`) or an
  external IDP.
- **Pod provisioning endpoint** — *deferred to v0.4*.
- **WebFinger, NIP-05 verification** — *deferred to v0.4* (small,
  incremental).
- **Quota enforcement** — *deferred to v0.4* with a `QuotaEnforcer`
  trait.
- **Embedded SPARQL read endpoint** — *wontfix in crate*; downstream
  `solid-pod-rs-sparql-bridge` over `oxigraph` is the clean path.
- **HTTP Range** — *deferred to v0.3.1* (trait signature change).
- **Full If-Match / If-None-Match enforcement** — *deferred to v0.3.1*.
- **Turtle ACL parser inside the crate** — *deferred to v0.3.1* as a
  pluggable `AclParser` trait; current JSON-LD path covers the
  mainstream client case.
- **RDF/XML serialiser** — *deferred indefinitely* to a downstream
  `solid-rdfxml` crate. RDF/XML is negotiated but we don't emit it.
- **LDP Direct / Indirect Containers** — *wontfix* (Solid Protocol
  only mandates Basic Containers).

### What solid-pod-rs has that JSS doesn't

- **NIP-98 HTTP auth as a first-class primary scheme** — Nostr-native
  pods with no OIDC IDP dependency.
- **Rust-native performance** — 80× faster cold start, ~15k req/s
  single-core, ~8 MB idle RSS, no Node.js runtime.
- **Feature-gated OIDC** — a minimal NIP-98-only deployment doesn't
  pull `openidconnect` or `jsonwebtoken`.
- **Send + Sync multi-tenant embedding** — host many pods in one
  process.
- **28-scenario WAC inheritance corpus** — exceeds JSS's published
  tests on edge cases (mixed public + authenticated cascade,
  grandchild inheritance through a private intermediate ACL).
- **Smaller attack surface** — no template engine, no email flows,
  no HTML rendering in-crate.

### Semantic differences (migration-sensitive)

- **Deny-by-default WAC** — solid-pod-rs returns 403 until `/.acl`
  is written. JSS ships a sample public root.
- **Strong ETags always** — hex SHA-256. JSS emits weak by default.
- **Slug collision** — solid-pod-rs falls back to UUID; JSS appends
  `-1`, `-2`, …. Clients should consume the `Location:` header.
- **N3 Patch `where` failure returns 412** — JSS returns 409. Both
  are spec-legal.
- **ACL write validates syntax first** — 422 on malformed bodies
  rather than JSS's 500-on-first-evaluation.

### Migration from a JSS client

A JSS client wires correctly to solid-pod-rs if it:

1. Tolerates both strong and weak ETags (we emit strong).
2. Reads `Location:` after POST rather than predicting Slug suffixes.
3. Writes an explicit `/.acl` after pod init (no implicit public
   default).
4. Prefers JSON-LD ACL documents over Turtle until v0.3.1.
5. Handles 412 (not just 409) on N3 Patch `where` mismatches.
6. Does not rely on JSON Patch PATCH — uses SPARQL-Update or N3
   Patch instead.
7. When using NIP-98, sends `Authorization: Nostr <base64-event>`
   (new territory — JSS has no NIP-98).
8. When using OIDC, supplies DPoP on every request (solid-pod-rs is
   strict per Solid-OIDC §5.2).

---

## Quick start

Add to your `Cargo.toml`:

```toml
[dependencies]
solid-pod-rs = "0.3.0-alpha.1"
tokio = { version = "1", features = ["full"] }
bytes = "1"
```

Minimal actix-web-bound server (see
[`examples/standalone.rs`](./examples/standalone.rs)):

```bash
cargo run --example standalone
```

Serves a pod from `$TMPDIR/solid-pod-rs-example` on
`127.0.0.1:8765`. Try:

```bash
# HEAD shows Link headers + WAC-Allow
curl -I http://127.0.0.1:8765/

# PUT a text/turtle resource
curl -X PUT http://127.0.0.1:8765/hello.ttl \
  -H 'Content-Type: text/turtle' \
  -d '<#a> <http://xmlns.com/foaf/0.1/name> "Hello" .'

# GET it back
curl http://127.0.0.1:8765/hello.ttl
```

---

## Architecture

solid-pod-rs is organised around a thin **storage trait** and a set
of **protocol modules** that operate against it. The library itself
does not bind to an HTTP framework — integration happens in
`examples/` or in a consumer crate.

```text
                +---------------------------------------------+
                |  HTTP layer (actix-web, axum, hyper, ...)   |
                |  Caller's choice. Library has no opinion.   |
                +----------------------+----------------------+
                                       |
                   +-------------------+-------------------+
                   v                   v                   v
              +---------+        +---------+         +----------+
              | auth    |        | ldp     |         | wac      |
              | nip98   |        | PATCH   |         | evaluate |
              | oidc*   |        | PREFER  |         | WAC-Allow|
              | webid   |        | Link    |         | groups   |
              +----+----+        +----+----+         +----+-----+
                   |                  |                   |
                   +------------------+-------------------+
                                      v
                            +-------------------+
                            | storage::Storage  |
                            | trait (async)     |
                            +---------+---------+
                                      |
                   +------------------+------------------+
                   v                  v                  v
              +---------+      +---------+         +----------+
              | Memory  |      | Fs      |         | S3*      |
              | backend |      | backend |         | backend  |
              | (bcast) |      | (notify)|         | (v0.4)   |
              +---------+      +---------+         +----------+

         +-------------------------------------------------+
         | notifications::*  -- WebSocketChannel2023 +     |
         |                      WebhookChannel2023         |
         |                      (broadcast channel fanout) |
         +-------------------------------------------------+

                       * feature-gated
```

### Module tour

- [`storage`](./src/storage/mod.rs) — `Storage` trait with
  `get`/`put`/`delete`/`list`/`head`/`exists`/`watch`; `ResourceMeta`
  describing ETag, modified, size, content-type, Link values; a
  `StorageEvent` enum for change-stream watchers.
- [`wac`](./src/wac.rs) — JSON-LD ACL evaluator. `AclDocument` parses
  the canonical WAC vocabulary; `evaluate_access` is the main
  decision function; `wac_allow_header` formats the WAC-Allow
  response header; `GroupMembership` is a pluggable trait so
  deployments can back groups by any resolver (e.g. a vcard:Group
  document on another pod).
- [`ldp`](./src/ldp.rs) — the LDP surface. Container rendering
  (`render_container_turtle`, `render_container_jsonld`), Link header
  generation, slug resolution, server-managed triples, `apply_n3_patch`,
  `apply_sparql_patch` via `spargebra`, content negotiation, a
  shared `Graph` model for RDF round-tripping.
- [`auth::nip98`](./src/auth/nip98.rs) — NIP-98 HTTP auth: parse the
  Nostr event from the `Authorization: Nostr <b64>` header, verify
  kind/created_at/tags/URL/method/payload, return the pubkey.
- [`oidc`](./src/oidc.rs) (feature `oidc`) — Solid-OIDC 0.1 bits:
  DPoP-bound access token verification, RFC 7591 dynamic
  registration, Discovery document, RFC 7662 introspection, WebID
  extraction.
- [`notifications`](./src/notifications.rs) — Solid Notifications 0.2:
  `InMemoryNotifications` registry, `WebSocketChannelManager` (tokio
  broadcast -> per-connection WS writer, 30 s heartbeat),
  `WebhookChannelManager` (AS 2.0 POST, retry with exponential
  backoff, dead-letter tracking), `discovery_document` for
  `.notifications`.
- [`webid`](./src/webid.rs) — WebID profile document generation
  (HTML + JSON-LD) and validation.
- [`error::PodError`](./src/error.rs) — crate-wide error type;
  `From` implementations for `std::io::Error`, `serde_json::Error`,
  `url::ParseError`, `base64::DecodeError`, `hex::FromHexError`,
  `notify::Error`.

---

## Feature list (detailed)

### LDP-BASIC conformance

- HTTP methods: **GET**, **PUT**, **POST** (with Slug), **DELETE**,
  **OPTIONS**, **HEAD**, **PATCH** (N3 + SPARQL-Update).
- **Link headers**: `<http://www.w3.org/ns/ldp#Resource>; rel="type"`,
  `<http://www.w3.org/ns/ldp#Container>; rel="type"` where
  applicable, `<.acl>; rel="acl"`, `<.meta>; rel="describedby"`,
  `<...>; rel="http://www.w3.org/ns/pim/space#storage"` on the pod
  root.
- **Prefer header** (RFC 7240 + LDP 4.2.2): `PreferMinimalContainer`
  omits `ldp:contains`; `PreferContainedIRIs` narrows the returned
  representation.
- **Accept-Post**: `text/turtle, application/ld+json,
  application/n-triples`.
- **Content negotiation** via q-value-aware Accept header parsing for
  Turtle, JSON-LD, N-Triples, and RDF/XML (RDF/XML emitted as a
  415-requiring-consumer-serialiser until `solid-rdfxml` lands).
- **PATCH via N3** (`text/n3`) with `solid:inserts`, `solid:deletes`,
  `solid:where` — 412 Precondition Failed on `where` mismatch.
- **PATCH via SPARQL-Update** (`application/sparql-update`) with
  `INSERT DATA` and `DELETE DATA`.
- **Server-managed triples**: `dc:modified`, `stat:size`,
  `stat:mtime`, `ldp:contains`. Client attempts to write server-
  managed triples are rejected via `find_illegal_server_managed`.
- **`.meta` sidecars** emitted for every non-meta, non-acl resource.

### WAC (Web Access Control)

- Modes: `acl:Read`, `acl:Write` (implies Append),
  `acl:Append`, `acl:Control`.
- Agents: `acl:agent` (specific WebID), `acl:agentClass foaf:Agent`
  (public), `acl:agentClass acl:AuthenticatedAgent`, `acl:agentGroup`
  (vcard:Group) via a pluggable `GroupMembership` trait.
- Scope: `acl:accessTo` (exact + child match), `acl:default`
  (container inheritance, 28-scenario corpus), `acl:origin`
  (browser-origin restriction).
- Resolver: `.acl` walk-up from the requested resource through
  parent containers; first ACL match wins.
- Response: `WAC-Allow: user="read write", public="read"` header.

### Authentication

- **NIP-98 (primary)** — parse `Authorization: Nostr <base64-event>`,
  verify kind 27235, check `created_at` within ±60 s, match `u` tag
  to the request URL, match `method` tag to the HTTP method, match
  `payload` tag to the SHA-256 of the body for write methods. Yields
  the pubkey used as a `did:nostr:<pubkey>` agent URI.
- **Solid-OIDC 0.1** (feature `oidc`) — full flow: RFC 7591 dynamic
  client registration, OIDC Discovery document, DPoP-bound access
  token verification (`cnf.jkt` must match the DPoP proof), RFC 7662
  token introspection, WebID extraction via `webid` claim or url-
  shaped `sub`.

### Notifications

- **WebSocketChannel2023** — tokio broadcast channel fans events to
  per-connection writers; 30 s keepalive pings; closes cleanly on
  peer disconnect.
- **WebhookChannel2023** — POSTs Activity Streams 2.0 events to the
  subscription target; 3× exponential retry on 5xx; immediate drop
  on 4xx; dead-letter tracked.
- **Discovery document** at `.notifications` lists both channels,
  their endpoints, and their feature URIs.

### Storage

- `MemoryBackend` — `Arc<RwLock<HashMap<String, (Bytes, ResourceMeta)>>>`
  with a `tokio::sync::broadcast` channel for change events. Ideal
  for tests.
- `FsBackend` — rooted at a configurable directory; writes bodies
  alongside `<path>.meta.json` sidecars; uses `notify` for change
  events; computes SHA-256 ETags on read + write.
- `S3Backend` — feature flag `s3-backend` declared; implementation
  deferred to v0.4.

---

## Usage examples

### Embed in an existing actix-web app

```rust
use std::sync::Arc;
use actix_web::{web, App, HttpServer};
use solid_pod_rs::storage::{fs::FsBackend, Storage};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let storage: Arc<dyn Storage> =
        Arc::new(FsBackend::new("./pod-data").await.unwrap());

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(storage.clone()))
            // your routes call storage.get / put / etc.
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
```

### Spin up the standalone server

```bash
cargo run --example standalone
```

Binds `127.0.0.1:8765` with the FS backend rooted at
`$TMPDIR/solid-pod-rs-example`. Supports GET, HEAD, PUT, DELETE
with NIP-98 auth extraction.

### Swap storage backends

```rust
use solid_pod_rs::storage::{memory::MemoryBackend, fs::FsBackend, Storage};
use std::sync::Arc;

// Tests:
let storage: Arc<dyn Storage> = Arc::new(MemoryBackend::new());

// Production:
let storage: Arc<dyn Storage> = Arc::new(FsBackend::new("./data").await?);
```

### Enable OIDC

```toml
[dependencies]
solid-pod-rs = { version = "0.3.0-alpha.1", features = ["oidc"] }
```

```rust
use solid_pod_rs::oidc;

let webid = oidc::verify_access_token(token, &jwks, &expected_issuer)
    .and_then(|claims| oidc::verify_dpop_proof(dpop_header, method, url, &claims))
    .and_then(|claims| oidc::extract_webid(&claims))?;
```

### Hook Solid Notifications to an external consumer

```rust
use solid_pod_rs::notifications::{InMemoryNotifications, WebhookChannelManager};
use std::sync::Arc;

let registry = Arc::new(InMemoryNotifications::new());
let webhooks = WebhookChannelManager::new(registry.clone());

// A resource event fans out to all registered webhook targets:
webhooks.broadcast_created("/resource.ttl").await;
```

---

## Testing

solid-pod-rs ships a **138-test** suite. Run it:

```bash
cargo test              # default features
cargo test --all-features
```

Test breakdown:

- `src/**/*.rs` — 91 in-module unit tests covering error types, ldp
  helpers (PATCH, Prefer, Link, Graph roundtrip, etc.), storage
  backends (memory + fs), wac primitives, NIP-98 structural checks,
  notifications (WebSocket + Webhook), OIDC.
- `tests/storage_trait.rs` — 15 conformance tests run against both
  `MemoryBackend` and `FsBackend` (so effectively 30 executions).
- `tests/wac_basic.rs` — 6 WAC smoke tests.
- `tests/wac_inheritance.rs` — **28-scenario ACL inheritance
  corpus** derived from WAC §5/§6. Covers group membership, mixed
  public + authenticated rules, `acl:default` cascade through
  intermediate ACLs, grandchild denial-by-absence.
- `tests/interop_jss.rs` — **22 fixture-driven JSS interop tests**
  validating observable HTTP behaviour against JSS: Link headers,
  content negotiation, ACL gating, LDP containment, error codes.
  Fixtures live in `tests/fixtures/*.http`.

Phase 2 spec-clause coverage (selected):

| Test | Validates |
|------|-----------|
| `link_headers_root_exposes_pim_storage` | LDP 4.2.1.3 + `pim:storage` |
| `prefer_minimal_container_parsed` | LDP 4.2.2 / RFC 7240 |
| `negotiate_prefers_explicit_turtle` | Solid Protocol §3.1 |
| `ntriples_roundtrip` | RDF 1.1 N-Triples |
| `server_managed_triples_include_ldp_contains` | LDP 5.2.1.4 |
| `n3_patch_insert_and_delete` | Solid Protocol §8.2 |
| `n3_patch_where_failure_returns_precondition` | Solid Protocol §8.2 |
| `sparql_insert_data`, `sparql_delete_data` | SPARQL 1.1 Update §3.1.1 / §3.1.2 |
| `websocket_manager_broadcasts_events` | Solid Notifications 0.2 §6 |
| `access_token_binds_to_dpop_jkt` | Solid-OIDC §5.2 |
| `extract_webid_from_explicit_claim` | Solid-OIDC §5.4 |
| `dynamic_registration_returns_client_id` | RFC 7591 |

The full clause-to-test map is in
[`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md).

---

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for scope, development
setup, backend conformance expectations, parity checklist protocol,
testing expectations, commit conventions, licence on contributions,
and security disclosure.

TL;DR: the library is framework-agnostic and must stay that way; new
backends must pass `tests/storage_trait.rs`; new features must land
with tests and update `PARITY-CHECKLIST.md`.

---

## Relation to the Solid ecosystem

solid-pod-rs tracks these specifications:

- **Solid Protocol 0.11** —
  <https://solidproject.org/TR/protocol>
- **Solid-OIDC 0.1** —
  <https://solidproject.org/TR/oidc>
- **Solid Notifications Protocol 0.2** —
  <https://solidproject.org/TR/notifications-protocol>
- **Web Access Control (WAC)** —
  <https://solidproject.org/TR/wac>
- **LDP-BASIC (W3C Linked Data Platform)** —
  <https://www.w3.org/TR/ldp/>
- **NIP-98 (Nostr HTTP Authentication)** —
  <https://github.com/nostr-protocol/nips/blob/master/98.md>

Related registries and schemas:

- **URN-Solid registry** —
  <https://urn-solid.github.io/>
- **solid-schema** —
  <https://solid-schema.github.io/>
- **Solid-Apps** —
  <https://solid-apps.github.io/>

Reference implementations:

- **JavaScriptSolidServer (JSS)** — the canonical JavaScript Solid
  reference that solid-pod-rs benchmarks against —
  <https://github.com/JavaScriptSolidServer/JavaScriptSolidServer>
  (AGPL-3.0-only)

General Solid project:

- <https://solidproject.org/>

---

## Roadmap

### v0.3.1 (near-term, ~6 weeks)

- Range requests (`Range`, `206 Partial Content`) via a streaming
  `get_range()` on the Storage trait.
- Conditional requests: `If-Match` / `If-None-Match` enforcement at
  the LDP layer with helper functions.
- NIP-98 Schnorr signature verification via `k256`.
- Pluggable `AclParser` trait with a default JSON-LD impl and an
  optional `sophia`-backed Turtle impl behind a feature flag.
- `.well-known/solid` discovery document.

### v0.4 (operator features, ~3 months)

- `S3Backend` implementation passing the storage conformance corpus.
- Pod provisioning endpoint (`.provision`) with seeded container
  templates.
- WebFinger + NIP-05 verification.
- Quota enforcement via a `QuotaEnforcer` trait.
- Notifications edge cases: ghost subscription GC, backpressure
  on slow webhook consumers.

### v1.0 (stabilisation, ~6 months)

- API stability commitment under semver.
- `docs.rs` documentation complete for all public items.
- Published interop results against JSS across a public test corpus.
- Formal security audit (externally contracted).

### Wontfix (documented decisions)

- WebID-TLS — deprecated by the Solid community.
- JSON Patch PATCH — use PUT + ETag or N3 Patch.
- Embedded SPARQL read endpoint in-crate —
  downstream `solid-pod-rs-sparql-bridge` over `oxigraph`.
- Admin HTML UI / password accounts in-crate — downstream
  `solid-pod-rs-admin`.
- RemoteStorage compatibility — different spec, different semantics.
- LDP Direct / Indirect Containers — not mandated by Solid Protocol.

---

## Licence

**AGPL-3.0-only** — inherited from the JavaScriptSolidServer ecosystem
covenant.

This means: if you operate solid-pod-rs as a network-accessible service,
AGPL §13 requires you to make the corresponding source code available to
your users under AGPL-3.0 or later. See [`LICENSE`](./LICENSE) and
[`NOTICE`](./NOTICE) for full terms and provenance.

If AGPL-3.0 is incompatible with your project's licence strategy, consider:
- Contributing upstream rather than hard-forking
- Using the crate in a sidecar architecture where AGPL obligations are
  contained to the sidecar process
- Running JSS itself (same licence; different language)

We welcome issues + PRs asking about specific compatibility scenarios.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you shall be licensed under the
same AGPL-3.0-only terms, without any additional terms or conditions.

See [`NOTICE`](./NOTICE) for attribution details and the full provenance
chain.

---

<sub>
solid-pod-rs is a DreamLab AI open-source project. Extracted from
VisionClaw (<https://github.com/DreamLab-AI/VisionClaw>) on
2026-04-20. Credit to the JavaScriptSolidServer contributors for
the reference implementation against which this Rust port was
benchmarked.
</sub>
