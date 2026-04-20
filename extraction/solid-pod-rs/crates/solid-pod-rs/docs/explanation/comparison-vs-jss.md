# Comparison vs JSS

JSS — [JavaScriptSolidServer](https://github.com/JavaScriptSolidServer/JavaScriptSolidServer)
— is a JavaScript implementation of the Solid Protocol, licensed
AGPL-3.0-only and maintained by the JavaScriptSolidServer
contributors. This page compares solid-pod-rs to JSS across runtime,
protocol support, operations, and extensibility. It is meant to set
expectations for operators evaluating a migration and to help
contributors understand where the two implementations diverge.

See also [PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md) for a
feature-level status table.

## At a glance

|                                | JSS 0.0.x             | solid-pod-rs 0.2.0-alpha |
|--------------------------------|-----------------------|---------------------------|
| Language                       | JavaScript (Node 18+) | Rust 2021 / 1.74+         |
| Binary distribution            | `npm install -g javascript-solid-server` → `jss` | Build from source; drop-in crate |
| Licence                        | AGPL-3.0-only         | AGPL-3.0-only (inherited) |
| HTTP framework                 | Fastify               | Agnostic; actix/axum/hyper|
| Configuration                  | `JSS_*` env vars + optional `config.json` | Rust code (no runtime config loader) |
| Memory footprint (idle)        | ~80 MB                | ~8 MB                     |
| Startup time                   | ~1 s                  | < 50 ms                   |

## Protocol coverage

| Feature                                 | JSS       | solid-pod-rs |
|-----------------------------------------|-----------|---------------|
| LDP RDF + non-RDF GET/PUT/DELETE        | full      | full          |
| LDP container GET                       | full      | full          |
| LDP Slug→child via POST                 | full      | full          |
| `Prefer` header parsing                 | full      | partial (return=representation / include / omit) |
| Content negotiation (Turtle/JSON-LD/N-Triples/RDF-XML) | full | Turtle/JSON-LD/N-Triples full; RDF-XML partial (negotiated, serialisation deferred) |
| `Link`: type, acl, describedby, storage | full      | full          |
| Strong ETag (SHA-256)                   | optional  | always        |
| If-Match enforcement                    | full      | full          |
| Range requests                          | full      | full          |
| `WAC-Allow` header                      | full      | full          |
| WAC agent / agentClass / agentGroup     | full      | full          |
| `.acl` walk-up resolution               | full      | full          |
| JSON Patch (RFC 6902)                   | full      | full          |
| N3 PATCH (solid-protocol)               | full      | full          |
| SPARQL-Update PATCH                     | full      | subset (INSERT DATA, DELETE DATA, DELETE/INSERT WHERE with ground templates) |
| Notifications 0.2 WebSocketChannel2023  | full      | full          |
| Notifications 0.2 WebhookChannel2023    | full      | full (3× retry, exp. backoff) |
| Solid-OIDC DPoP                         | full      | full (feature `oidc`)         |
| OIDC dynamic client registration        | full      | full          |
| OIDC discovery doc                      | full      | full          |
| Token introspection (RFC 7662)          | full      | full          |
| WebID extraction                        | full      | full          |
| WebID-TLS                               | feature-flagged (`webidTls`) | not supported (legacy) |
| NIP-98                                  | via Nostr relay feature | full (structural) |
| `.provision` / account scaffold         | full (IdP + multiuser) | full (`provision_pod`) |
| `.well-known/solid`                     | full      | full          |
| WebFinger / NIP-05                      | full (via ActivityPub + Nostr features) | full |

## Defaults that differ (gotchas for JSS migrants)

### ACL posture

- **JSS default:** multiuser-enabled; pods created through the IdP
  signup flow receive a scaffolded `/.acl`. A `public: true` option
  disables WAC entirely for trusted single-user deployments.
- **solid-pod-rs:** deny-by-default, always. You cannot read a pod
  without an `.acl` being in effect somewhere up the tree. No runtime
  switch to disable WAC — it is a library invariant.

Mitigation: commit `/.acl` as the first write to any new pod. See
[tutorial 3](../tutorials/03-adding-access-control.md).

### Content-type storage

- **JSS:** stores the body as the client `PUT`s it, with a sidecar
  describing content type. Content-type negotiation at read time is
  opt-in via the `conneg` flag.
- **solid-pod-rs:** stores the body verbatim with its original
  `Content-Type`. No on-the-fly transcoding. If a client `PUT`s
  Turtle and another `GET`s with `Accept: application/ld+json`, we
  serve the stored Turtle with a 200 (client ignores Accept) —
  *unless* you wire up transcoding in your HTTP layer using the
  `Graph` primitives.

### PATCH dialect support

- **JSS:** supports JSON Patch, N3, and SPARQL Update.
- **solid-pod-rs:** all three dialects supported; SPARQL-Update is a
  documented subset (INSERT DATA, DELETE DATA, DELETE/INSERT WHERE
  with ground templates only).

## Operational comparison

### Configuration

- **JSS:** `JSS_*` environment variables (e.g. `JSS_PORT`, `JSS_HOST`,
  `JSS_ROOT`) overlaid on an optional `config.json` file, overlaid on
  CLI arguments. Precedence: CLI > env > file > defaults.
- **solid-pod-rs:** configured in Rust. You write a ~50-line `main.rs`
  that picks a storage backend, wires auth, and starts your HTTP
  framework. No DI container, no runtime config loader.

Pros and cons:

- JSS gives you runtime configurability; you can change the storage
  root without rebuilding. solid-pod-rs requires a rebuild to swap
  backends — but the build takes <2 s incremental and your whole
  server is a static binary.
- Rust configuration is type-checked. Misconfigurations show up at
  compile time, not on startup.

### Observability

- **JSS:** logs via Fastify's pino-based logger.
- **solid-pod-rs:** `tracing`. Structured JSON logs + spans.

### Monitoring

- **JSS:** emits logs; no built-in metrics exporter.
- **solid-pod-rs:** library does not export metrics itself; you
  instrument at the HTTP framework layer.

### Backup

- **JSS:** filesystem backup of `JSS_ROOT` (default `./data`) works.
- **solid-pod-rs:** filesystem backup of `$POD_FS_ROOT` works; include
  the `.meta.json` sidecars.

## Extensibility

### Writing a custom storage backend

- **JSS:** the data layer is monolithic (filesystem + optional sql.js
  for accounts); swapping it requires a fork.
- **solid-pod-rs:** implement the `Storage` trait (7 async methods).
  Pass `tests/storage_trait.rs` and you're done.

### Writing custom auth

- **JSS:** fastify plugin architecture; add middleware or a custom
  auth hook via Fastify's hook API.
- **solid-pod-rs:** write middleware in your HTTP framework; call
  `auth::nip98::verify` or `oidc::verify_access_token` (or your own
  logic) and populate request-scoped state.

### Writing a notification backend

- **JSS:** `@fastify/websocket` plugin; extension requires a fork.
- **solid-pod-rs:** implement `Notifications` trait (3 async methods)
  and feed it from `Storage::watch()`.

## What you give up moving to solid-pod-rs

- IdP integration with dynamic account signup (JSS's
  `oidc-provider`-based flow is out of scope; bring your own IdP).
- ActivityPub federation (JSS has built-in support; not in
  solid-pod-rs).
- Some Prefer-header nuances (handling=strict vs handling=lenient —
  we always parse leniently).

## What you gain moving to solid-pod-rs

- A single static binary, <20 MB.
- ~10× smaller memory footprint.
- Strong typing at every boundary (`AccessMode`, `PatchDialect`,
  `StorageEvent`, `RdfFormat`).
- First-class NIP-98 authentication (useful for Nostr ecosystems).
- Sub-ms startup time; viable as a serverless function.
- `cargo test` runs the whole conformance suite in a couple of
  seconds.
- Linear cost increase per resource (Rust + tokio scales further per
  core than V8).
- AGPL-3.0-only licensing inherited from the JSS ecosystem covenant — same
  network-service copyleft protection, different runtime — fewer compliance
  concerns when embedding into proprietary or non-AGPL services.

## When to stay on JSS

- You need the built-in IdP (OIDC provider with account signup).
- You want ActivityPub federation out of the box.
- You rely on JSS-specific features (`mashlib`, `solidosUi`, Git HTTP
  backend, invite-only registration).
- You need WebID-TLS.
- AGPL-3.0 licensing is acceptable or desired for your project.

## When to pick solid-pod-rs

- You are building a NIP-98-authenticated app and want Solid on top.
- You need a tiny, embeddable pod for IoT / edge / serverless.
- You want to add solid-pod-rs as a library inside an existing Rust
  service (VisionClaw, federated forum, etc.).
- You need strict deny-by-default WAC.
- Your deployment demands static binaries (k8s, distroless, single-
  container deployments).
- You cannot take an AGPL-3.0 dependency into your service tree.

## See also

- [PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md)
- [how-to/migrate-from-jss.md](../how-to/migrate-from-jss.md)
- [explanation/architecture-decisions.md](architecture-decisions.md)
