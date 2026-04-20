# solid-pod-rs-server

Binary distribution of [`solid-pod-rs`](../solid-pod-rs/) — a drop-in
JSS replacement that runs as a single static-ish Rust binary.

## Install

Once published to crates.io (target: v0.4.0):

```bash
cargo install solid-pod-rs-server
solid-pod-rs-server --config config.json
```

Until then, build from source:

```bash
cargo build --release -p solid-pod-rs-server
./target/release/solid-pod-rs-server --help
```

## Architecture

This crate is a thin binary shell over [`solid-pod-rs`](../solid-pod-rs/).
Per ADR-056 §D3 (F7 library-server split):

- [`solid-pod-rs`](../solid-pod-rs/) — pure library. No `#[tokio::main]`,
  no `actix-web::HttpServer`. Framework-agnostic.
- `solid-pod-rs-server` (this crate) — owns the actix-web HTTP server,
  the tokio runtime, clap CLI, the F6 layered config loader, and signal
  handling. Depends on the library and wires its `PodService`-style
  primitives into concrete HTTP routes.

## Configuration

Configuration is loaded by [`solid_pod_rs::config::ConfigLoader`]
(F6, PRD §F6). Precedence (later overrides earlier):

```text
Defaults  <  File  <  EnvVars  <  CLI flags
```

See [`crates/solid-pod-rs/src/config/sources.rs`](../solid-pod-rs/src/config/sources.rs)
for the full `JSS_*` environment variable table.

## Feature flags

This binary enables the following `solid-pod-rs` features by default:

| Feature | Purpose |
|---|---|
| `fs-backend` | Filesystem storage (JSS default) |
| `memory-backend` | In-memory storage (test / dev) |
| `config-loader` | F6 layered config loader |
| `legacy-notifications` | F3 `solid-0.1` WS notifications adapter |

Other feature flags (`oidc`, `dpop-replay-cache`, `nip98-schnorr`,
`s3-backend`) can be opted into by the operator via a custom build.

## Licence

**AGPL-3.0-only**. See [`LICENSE`](./LICENSE). Operating this binary as a
network service triggers AGPL §13 source-disclosure obligations.

## v0.5.0 sibling crates

The following sibling crates are reserved under the workspace for
v0.5.0 extensions; they are empty placeholders in v0.4.0:

- [`solid-pod-rs-activitypub`](../solid-pod-rs-activitypub/) — ActivityPub federation
- [`solid-pod-rs-git`](../solid-pod-rs-git/) — Git HTTP backend
- [`solid-pod-rs-idp`](../solid-pod-rs-idp/) — OAuth / OIDC IDP
- [`solid-pod-rs-nostr`](../solid-pod-rs-nostr/) — DID:nostr + embedded Nostr relay
