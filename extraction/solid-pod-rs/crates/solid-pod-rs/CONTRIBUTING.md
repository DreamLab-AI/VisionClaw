# Contributing to solid-pod-rs

## Scope

solid-pod-rs implements the Solid Protocol server-side: WAC, LDP,
WebID, Notifications. Keep the crate platform-independent — no
Cloudflare Workers, no actix-web dependencies in the library crate,
no CF-specific code (R2, KV, D1, `worker::*`). HTTP framework
integration lives in examples or downstream crates.

## Backends

New storage backends must implement `storage::Storage` in full and
pass the `tests/storage_trait.rs` conformance suite on both Memory
and themselves. Concurrency-safety is required.

## Parity checklist

Any feature port from the JavaScriptSolidServer reference (under
`references/javascript-solid-server/`) must update
`PARITY-CHECKLIST.md` to reflect its new status.

## Testing

```bash
cargo test -p solid-pod-rs
cargo clippy -p solid-pod-rs --all-targets
```

## Licence on contribution

solid-pod-rs is licensed **AGPL-3.0-only**, inherited from the
JavaScriptSolidServer (JSS) ecosystem covenant. By submitting a
contribution (patch, PR, issue patch, documentation change) you agree
that your contribution is licensed under the same AGPL-3.0-only terms
that cover the rest of the crate, and that you have the right to
release it under those terms. There is no separate CLA; the licence
on the file you edit is the licence on your change.

Practical implications:

- Contributions under a permissive licence (MIT / Apache-2.0 / BSD)
  are accepted — the AGPL is compatible with permissive upstream code
  being relicensed on inbound, and the crate itself remains AGPL on
  outbound.
- Contributions under an incompatible copyleft licence (e.g. GPL-2.0-
  only, SSPL, BUSL) cannot be merged.
- If your employer requires a separate DCO / CLA process, please sign
  your commits with `git commit -s` and flag the PR so we can record
  the trail.

## Attribution

This crate originated as an extraction from
`community-forum-rs/crates/pod-worker` and inherits its AGPL-3.0-only
licence from the JavaScriptSolidServer ecosystem covenant (see
`NOTICE` for the full provenance chain). Keep attribution intact in
`NOTICE` when making derivative changes.
