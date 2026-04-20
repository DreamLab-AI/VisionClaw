# Environment variables reference

solid-pod-rs is a **library** — it does not read environment variables
itself. This page documents the conventional environment variables
the example server and recommended integrations honour, plus those
honoured by upstream dependencies that consumers typically surface.

If you want a specific env var to be authoritative, wire it into your
pod binary yourself. Nothing below is read by `solid_pod_rs` code.

## Recommended conventional env vars

Use these when building a pod binary on top of the crate. The stock
example (`examples/standalone.rs`) uses hard-coded defaults; production
wrappers add the indirection.

| Variable | Type / default | Consumed by | Purpose |
|---|---|---|---|
| `POD_BIND`               | `host:port` string, default `127.0.0.1:8765` | your HTTP framework | Listen address. |
| `POD_BASE_URL`           | URL, e.g. `https://pod.example` | `notifications::ChangeNotification::from_storage_event`, `oidc::discovery_for` | Canonical public URL of the pod. |
| `POD_STORAGE_BACKEND`    | `fs`, `memory`, `s3` | your wiring | Selects the `Storage` implementation to construct. |
| `POD_FS_ROOT`            | path | `FsBackend::new` | Root directory for the FS backend. |
| `POD_S3_BUCKET`          | bucket name | S3 backend builder | S3 bucket (when using the S3 backend). |
| `POD_S3_PREFIX`          | key prefix | S3 backend builder | Key-prefix isolation for multi-tenant buckets. |
| `POD_NIP98_TOLERANCE`    | seconds, default 60 | your NIP-98 middleware | Override the timestamp window. Don't exceed 300. |
| `POD_OIDC_ISSUER`        | URL | `oidc::discovery_for` | OIDC issuer identity. Ignored when `oidc` feature is off. |
| `POD_OIDC_HS256_SECRET`  | bytes (UTF-8 OK) | `oidc::verify_access_token` | HS256 secret for test-path token verification. Production deployments use ES256/RS256 and a JWKS instead. |
| `POD_WEBHOOK_RETRY_BASE` | ms, default 500 | `WebhookChannelManager::retry_base` | Base backoff for webhook retries. |
| `POD_WEBHOOK_MAX_RETRIES`| integer, default 3 | `WebhookChannelManager::max_retries` | Max retries on 5xx. |
| `POD_WS_HEARTBEAT`       | seconds, default 30 | `WebSocketChannelManager::with_heartbeat` | WebSocket ping interval. |

None of the above are parsed by the library. This table is a suggested
vocabulary so multi-pod deployments can share config conventions.

## Tracing / logging

solid-pod-rs uses the `tracing` crate.

| Variable | Consumer | Effect |
|---|---|---|
| `RUST_LOG` | `tracing_subscriber::EnvFilter` | Filter spec. `solid_pod_rs=info`, `solid_pod_rs=debug` for ACL resolver traces, etc. |
| `RUST_LOG_STYLE` | `tracing_subscriber::fmt` | `auto`, `always`, `never`. |

Example production settings:

```
RUST_LOG=solid_pod_rs=info,tower_http=info,actix_web=warn
```

## AWS S3 backend (when feature `s3-backend` enabled)

Standard AWS env vars are honoured by the `aws-config` default
loader:

| Variable | Purpose |
|---|---|
| `AWS_REGION`                    | AWS region. |
| `AWS_ACCESS_KEY_ID`             | Static access key. |
| `AWS_SECRET_ACCESS_KEY`         | Static secret key. |
| `AWS_SESSION_TOKEN`             | STS session token. |
| `AWS_PROFILE`                   | Named profile in `~/.aws/credentials`. |
| `AWS_ENDPOINT_URL_S3`           | Custom endpoint (R2, MinIO, Tigris). |
| `AWS_USE_PATH_STYLE_ADDRESSING` | Path-style URLs (some S3-compatible stores). |

These are read by the AWS SDK, not by solid-pod-rs.

## `notify` filesystem watcher

`FsBackend` uses `notify` internally. There is no exposed env var.
The watcher inherits platform defaults (inotify on Linux, FSEvents
on macOS, ReadDirectoryChangesW on Windows).

Very large Pods may hit platform `inotify` limits:

```bash
sudo sysctl -w fs.inotify.max_user_watches=524288
```

## Compile-time features (not env vars)

These are feature flags in `Cargo.toml`, set at build time:

| Feature        | Default | Effect |
|----------------|---------|--------|
| `fs-backend`   | yes     | Compiles `FsBackend`. |
| `memory-backend` | yes   | Compiles `MemoryBackend`. |
| `s3-backend`   | no      | Compiles `S3Backend` (requires `aws-sdk-s3`). |
| `oidc`         | no      | Compiles the `oidc` module. |

## Testing

For tests, use `tempfile` to create a throwaway `POD_FS_ROOT`:

```rust
use tempfile::TempDir;
let tmp = TempDir::new().unwrap();
let backend = FsBackend::new(tmp.path()).await?;
```

No env var needed.

## See also

- [how-to/deploy-to-production.md](../how-to/deploy-to-production.md)
- [how-to/migrate-from-jss.md](../how-to/migrate-from-jss.md) — includes a
  JSS → solid-pod-rs env-var mapping table.
- [reference/api.md](api.md)
