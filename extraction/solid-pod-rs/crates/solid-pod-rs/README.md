# solid-pod-rs

Framework-agnostic Rust library for serving [Solid Protocol 0.11]
pods: LDP resources and containers, Web Access Control, WebID,
Solid Notifications 0.2, Solid-OIDC 0.1, and NIP-98 HTTP auth.

The library has no opinions about the HTTP runtime; wire it into
actix-web, axum, hyper, or any other server. For a turnkey binary
use the sibling crate [`solid-pod-rs-server`](../solid-pod-rs-server/).

```toml
[dependencies]
solid-pod-rs = "0.4.0-alpha.1"
```

```rust
use solid_pod_rs::{storage::FsStorage, wac, ldp};
use std::path::PathBuf;

let storage = FsStorage::new(PathBuf::from("./pod-root"));
// Compose with your framework; see examples/embed_in_actix.rs.
```

## Feature flags

| Flag                    | Default | Purpose                                       |
|-------------------------|:-------:|-----------------------------------------------|
| `fs-backend`            | on      | POSIX filesystem storage.                     |
| `memory-backend`        | on      | In-process `HashMap` storage (tests/demos).   |
| `s3-backend`            | off     | AWS S3 / S3-compatible object stores.         |
| `oidc`                  | off     | Solid-OIDC 0.1 + DPoP.                        |
| `dpop-replay-cache`     | off     | DPoP `jti` replay cache (pulls `oidc`).       |
| `nip98-schnorr`         | off     | BIP-340 signature verification for NIP-98.    |
| `acl-origin`            | off     | WAC `acl:origin` enforcement.                 |
| `security-primitives`   | off     | SSRF guard + dotfile allowlist.               |
| `legacy-notifications`  | off     | `solid-0.1` WebSocket adapter (SolidOS).      |
| `config-loader`         | off     | Layered config loader with `JSS_*` env vars.  |

## Modules

| Module          | Responsibility                                               |
|-----------------|--------------------------------------------------------------|
| `storage`       | `Storage` trait + FS / Memory / S3 backends.                 |
| `ldp`           | Resources, containers, content negotiation, PATCH, `Prefer`. |
| `wac`           | Access control evaluator (JSON-LD + Turtle ACL).             |
| `webid`         | WebID profile documents.                                     |
| `auth`          | NIP-98 HTTP authentication.                                  |
| `oidc`          | Solid-OIDC 0.1 + DPoP + replay cache.                        |
| `notifications` | WebSocketChannel2023, WebhookChannel2023, legacy adapter.    |
| `security`      | SSRF guard + dotfile allowlist.                              |
| `config`        | Layered configuration schema.                                |
| `interop`       | `.well-known/solid`, WebFinger JRD.                          |
| `provision`     | Pod bootstrap (WebID + containers + ACL).                    |

## Documentation

- Workspace README: [`../../README.md`](../../README.md)
- Diátaxis docs: [`docs/`](docs/)
- Parity vs JSS: [`PARITY-CHECKLIST.md`](PARITY-CHECKLIST.md)
- Gap analysis: [`GAP-ANALYSIS.md`](GAP-ANALYSIS.md)
- Changelog: [`CHANGELOG.md`](CHANGELOG.md)

## Licence

AGPL-3.0-only — see [`../../LICENSE`](../../LICENSE) and
[`NOTICE`](NOTICE).

[Solid Protocol 0.11]: https://solidproject.org/TR/protocol
