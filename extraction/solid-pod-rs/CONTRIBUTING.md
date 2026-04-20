# Contributing to solid-pod-rs

Thanks for your interest. solid-pod-rs is a framework-agnostic Rust
library for hosting a Solid Pod. Contributions that broaden protocol
coverage, harden tests, or improve performance are all welcome.

## Scope

solid-pod-rs implements the **server side** of the Solid Protocol:

- LDP-BASIC resource and container semantics
- Web Access Control (WAC)
- WebID profile documents
- NIP-98 HTTP authentication (primary)
- Solid-OIDC 0.1 (feature-gated, `--features oidc`)
- Solid Notifications 0.2 (WebSocket + Webhook channels)

Keep the library crate **platform-independent and framework-agnostic**:

- No Cloudflare Workers types (`worker::*`, R2, KV, D1) in the main crate
- No `actix-web` / `axum` / `hyper` dependencies in the library (they
  belong in examples or consumer crates)
- No runtime assumptions beyond tokio + `Send + Sync + 'static`

HTTP framework bindings live in `examples/` or downstream consumer
crates.

## Development setup

```bash
git clone https://github.com/dreamlab-ai/solid-pod-rs
cd solid-pod-rs
cargo test
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Run the example server:

```bash
cargo run --example standalone
```

Serves a pod from `$TMPDIR/solid-pod-rs-example` on `127.0.0.1:8765`.

## Backends

New storage backends must implement `storage::Storage` in full and
pass the `tests/storage_trait.rs` conformance suite on both the new
backend and the Memory backend. Concurrency safety is required.

Backends should:

- Preserve strong ETags (SHA-256 of the body, hex-encoded)
- Emit `StorageEvent` via a `tokio::sync::mpsc` sender for `watch()`
- Persist content-type and Link values through `ResourceMeta`
- Reject invalid paths (directory traversal, absolute paths) via
  `PodError::InvalidPath`

## Parity checklist

Any feature port from the JavaScriptSolidServer (JSS) reference
implementation must update `PARITY-CHECKLIST.md` to reflect its new
status. The checklist is the contract between this crate and the
wider Solid ecosystem — it is not a formality.

See `GAP-ANALYSIS.md` for deferred features and the rationale for
deferrals.

## Testing expectations

- Every new feature lands with **at least one** unit test and one
  integration test where applicable.
- New WAC behaviour extends `tests/wac_inheritance.rs` with a scenario.
- New LDP behaviour extends `tests/interop_jss.rs` with a fixture.
- Feature-gated code (`oidc`, `s3-backend`) carries feature-gated tests.
- Do not merge regressions against the JSS behaviour corpus without a
  documented rationale.

## Commit conventions

- Conventional Commits style preferred: `feat:`, `fix:`, `docs:`,
  `test:`, `chore:`
- Reference spec clauses in commit bodies where relevant (e.g.
  "implements LDP 4.2.2 Prefer handling").
- Sign-off via `git commit -s` if you want a DCO trail.

## Licence on contributions

solid-pod-rs is licensed **AGPL-3.0-only**, inherited from the
JavaScriptSolidServer (JSS) ecosystem covenant. By submitting a
contribution (patch, PR, documentation change) you agree that your
contribution is licensed under the same AGPL-3.0-only terms, and that
you have the right to release it under those terms. There is no
separate CLA; the licence on the file you edit is the licence on your
change.

Practical implications:

- Contributions under a permissive licence (MIT / Apache-2.0 / BSD)
  are accepted — permissive upstream code can be relicensed on inbound,
  and the crate remains AGPL on outbound.
- Contributions under an incompatible copyleft licence (e.g. GPL-2.0-
  only, SSPL, BUSL) cannot be merged.
- If your employer requires a DCO / CLA process, please sign your
  commits with `git commit -s` and flag the PR so we can record the
  trail.

## Security

Security-sensitive issues (WAC bypass, NIP-98 forgery, OIDC token
handling) should not be filed in public issues first. Email
security@dreamlab.ai with a minimal reproducer; a public issue and
CVE will follow coordinated disclosure.

## Attribution

This crate originated as an extraction from
`github.com/DreamLab-AI/VisionClaw:crates/solid-pod-rs/`, which in
turn derived from `community-forum-rs/crates/pod-worker`. Keep
attribution intact in `NOTICE` when making derivative changes.
