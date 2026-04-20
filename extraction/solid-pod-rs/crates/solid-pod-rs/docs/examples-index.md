# solid-pod-rs — examples index

Every example lives in `crates/solid-pod-rs/examples/` and runs with
`cargo run --example <name> -p solid-pod-rs`. The block at the top of
each source file repeats the invocation, the expected output, and any
prerequisites.

## Server-side

| Example | Purpose |
|---------|---------|
| `standalone` | Minimal actix-web server wiring `FsBackend`, NIP-98 auth, LDP headers, and WAC-Allow. The quickest way to see a working pod. |
| `embed_in_actix` | Same pod, but mounted under `/pod/*` inside a larger actix-web app that also serves `/health` and `/api/whoami`. Shows how to share `SharedState` (storage + auth) between the host app and the pod sub-scope. |
| `custom_storage` | Implements the `Storage` trait for a `BTreeMap`-backed backend to demonstrate the extension point. Useful if you want to back a pod with Redis, SQLite, S3, IPFS, etc. |
| `webhook_receiver` | Minimal Axum server that receives `WebhookChannel2023` POSTs from a pod and logs them. Good template for building downstream consumers. |

## Client-side

| Example | Purpose |
|---------|---------|
| `nip98_client` | Builds a NIP-98 `Authorization: Nostr <b64>` header (with payload hash), PUTs a Turtle resource against a running pod, reads it back. |
| `notifications_consumer` | Connects to a pod's `WebSocketChannel2023` endpoint, subscribes to a topic, and prints every `ChangeNotification`. |
| `oidc_client` | Feature-gated (`--features oidc`). Walks the discovery → dynamic client registration → DPoP proof → access-token verification cycle hermetically. |

## Administration

| Example | Purpose |
|---------|---------|
| `wac_admin` | CLI that reads and writes `.acl` sidecars directly against an `FsBackend` root. Supports `grant`, `show`, and `check` subcommands. |

## Compilation check

```bash
cargo check -p solid-pod-rs --examples                 # without oidc
cargo check -p solid-pod-rs --examples --features oidc # with oidc
```

Both must succeed. If the `oidc` feature is disabled, the
`oidc_client` example compiles to a stub that prints a hint and exits
— it is still registered as an example so the name is discoverable.
