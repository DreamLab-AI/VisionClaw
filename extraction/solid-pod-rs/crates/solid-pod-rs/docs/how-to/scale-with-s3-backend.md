# How to scale with an S3 backend

**Goal:** replace the filesystem backend with S3 (or an S3-compatible
store — R2, MinIO, Tigris) for horizontal scale.

> **Status:** the `s3-backend` feature is declared in Cargo.toml and
> pulls in `aws-sdk-s3`, but the implementation is a P2 item (see
> [PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md#storage-backends)).
> This guide documents the planned shape; the trait contract is
> stable.

## Enable the feature

```toml
[dependencies]
solid-pod-rs = { version = "0.2", features = ["s3-backend"] }
aws-config   = "1"
aws-sdk-s3   = "1"
```

## Configure the client

```rust
use solid_pod_rs::storage::Storage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let sdk_config = aws_config::load_from_env().await;
    let s3_client  = aws_sdk_s3::Client::new(&sdk_config);

    let backend = solid_pod_rs::storage::s3::S3Backend::builder(s3_client)
        .bucket("my-pod-bucket")
        .prefix("pods/mypod")
        .build()
        .await?;

    let storage: std::sync::Arc<dyn Storage> = std::sync::Arc::new(backend);
    // Use `storage` exactly as with FsBackend.
    Ok(())
}
```

## Object layout

| Pod path | S3 key |
|---|---|
| `/profile/card`       | `pods/mypod/profile/card` |
| `/profile/card.meta`  | `pods/mypod/profile/card.meta` |
| `/profile/card.acl`   | `pods/mypod/profile/card.acl` |

Metadata (content-type, Link header list) is stored as S3 user-defined
metadata on the object itself — no sidecar.

## ETag

Use S3's native `ETag` response header directly. For
`multipart_upload` objects the ETag is not a strict SHA-256 — if you
need deterministic etags, put a `x-amz-meta-sha256` user header and
read that in `ResourceMeta::etag`.

## Watchers

S3 does not expose a filesystem-style `inotify` stream. Use one of:

1. **Event Bridge → SQS**: configure bucket notifications on
   `s3:ObjectCreated:*` and `s3:ObjectRemoved:*`, push to SQS. The
   backend's `watch()` consumes from SQS and re-emits `StorageEvent`.
2. **Polling**: `watch()` polls `list_objects_v2` at an interval. OK
   for low-traffic or development.

Both strategies keep the trait contract intact — callers only see
`StorageEvent`.

## Cost considerations

- Every `GET` incurs a billed `GetObject`. Consider a short-TTL cache
  in front of the backend for frequently-read resources like
  `/profile/card` and common `.acl` files.
- `list()` does one `ListObjectsV2` per call; combine with a cache to
  avoid re-listing every time an LDP container is rendered.
- The `.meta` and `.acl` sidecars double the GET count for most
  request paths. Use S3 object metadata + in-process caching to halve
  it.

## Production checklist

- [ ] Bucket versioning enabled (rollback cover).
- [ ] Server-side encryption (SSE-S3 minimum, KMS for regulated data).
- [ ] IAM: pod processes have only `s3:GetObject`, `PutObject`,
      `DeleteObject`, `ListBucket` on the prefix.
- [ ] Lifecycle policy for `.meta` objects (never expire).
- [ ] Event notifications forwarded to SQS or Kinesis.
- [ ] CloudWatch metrics on 5xx rate.

## Testing

Use [`s3s`](https://crates.io/crates/s3s) or MinIO for integration
tests. Or use the `MemoryBackend` for anything that doesn't need
S3-specific behaviour — the `Storage` trait is backend-agnostic by
construction.

## See also

- [how-to/swap-storage-backends.md](swap-storage-backends.md)
- [explanation/storage-abstraction.md](../explanation/storage-abstraction.md)
- [reference/api.md §Storage trait](../reference/api.md#storage-trait)
