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

Any feature port from the JavaScript Solid Server reference (under
`references/community-solid-server/`) must update
`PARITY-CHECKLIST.md` to reflect its new status.

## Testing

```bash
cargo test -p solid-pod-rs
cargo clippy -p solid-pod-rs --all-targets
```

## Attribution

This crate originated as an extraction from
`community-forum-rs/crates/pod-worker`. Keep attribution intact in
NOTICE when making derivative changes.
