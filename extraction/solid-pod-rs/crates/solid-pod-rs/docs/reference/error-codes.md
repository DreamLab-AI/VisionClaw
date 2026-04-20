# Error code reference

Every `solid_pod_rs` API returns `Result<T, PodError>`. This page
documents how each `PodError` variant maps to an HTTP status code and
response body, and what typically triggers it.

The library itself does not produce HTTP responses. Your HTTP
framework maps errors. The mapping below matches the convention used
in `examples/standalone.rs` and is the recommended default.

## `PodError` → HTTP mapping

| `PodError` variant                  | HTTP status         | `WWW-Authenticate` | Body suggestion |
|-------------------------------------|---------------------|--------------------|-----------------|
| `NotFound(path)`                    | 404                 | —                  | `"resource not found"` |
| `AlreadyExists(path)`               | 409                 | —                  | `"resource already exists"` |
| `Forbidden`                         | 403                 | —                  | `""` (avoid leaking ACL info) |
| `Unauthenticated`                   | 401                 | `Nostr` (+ `DPoP` if OIDC) | `""` |
| `InvalidPath(path)`                 | 400                 | —                  | `"invalid path"` |
| `InvalidContentType(ct)`            | 415                 | —                  | echo the CT |
| `Json(_)`                           | 400                 | —                  | `"malformed JSON"` |
| `UrlParse(_)`                       | 400                 | —                  | `"malformed URL"` |
| `Base64(_)`                         | 400                 | —                  | `"malformed base64"` |
| `Hex(_)`                            | 400                 | —                  | `"malformed hex"` |
| `AclParse(msg)`                     | 500                 | —                  | server error, log `msg` |
| `Nip98(msg)`                        | 401                 | `Nostr`            | log `msg`, don't echo |
| `Io(err)`                           | 500                 | —                  | log `err` |
| `Watch(msg)`                        | 500                 | —                  | log `msg` |
| `Backend(msg)`                      | 500                 | —                  | log `msg` |
| `PreconditionFailed(msg)`           | 412                 | —                  | echo `msg` (safe: no secret info) |
| `Unsupported(msg)`                  | 400 or 415 (context-dependent) | —       | echo `msg` |

## When each variant fires

### `NotFound`

- `Storage::get`, `head`, `delete` on a missing path.
- `StorageAclResolver::find_effective_acl` does **not** produce
  `NotFound` — it returns `Ok(None)` when no ACL is found.

### `AlreadyExists`

Reserved for backends with true insert-only semantics. Neither
`MemoryBackend` nor `FsBackend` produce this — `put` is create-or-
replace. Custom backends may choose to enforce it.

### `Forbidden` / `Unauthenticated`

Produced by the HTTP layer, not by library APIs. Use when
`wac::evaluate_access` returns `false` (`403`) or when authentication
failed (`401`).

### `InvalidPath`

- `FsBackend::normalize` when the path contains `..` or `\0`.
- Any custom backend rejecting paths that would escape its root.

### `InvalidContentType`

Reserve for PATCH dispatch and similar content-type-sensitive
operations. `patch_dialect_from_mime` returning `None` does not
produce an error itself; wrap it:

```rust
patch_dialect_from_mime(ct).ok_or_else(|| PodError::InvalidContentType(ct.into()))?
```

### `Json`, `UrlParse`, `Base64`, `Hex`

Auto-converted from their upstream error types via `#[from]`. They
bubble up from:

- `Json`: `AclDocument` deserialisation, OIDC claim parsing.
- `UrlParse`: explicit `url::Url::parse` calls (not currently in the
  crate hot path but available).
- `Base64`: NIP-98 token decode; DPoP JWT part decode.
- `Hex`: NIP-98 pubkey hex validation.

### `AclParse`

Produced on explicit ACL parsing failures where the JSON is valid but
the document shape is wrong. `StorageAclResolver` currently swallows
ACL parse failures (treats them as "no ACL found") — callers that
want strict mode should fetch `/.acl` manually and
`serde_json::from_slice` into `AclDocument`.

### `Nip98`

Every structural failure in `auth::nip98::verify_at`:

| Cause | Message prefix |
|---|---|
| Missing `Nostr ` prefix | `missing 'Nostr ' prefix` |
| Token too large | `token too large` / `decoded token too large` |
| Wrong kind | `wrong kind: expected 27235, got …` |
| Invalid pubkey | `invalid pubkey` |
| Timestamp out of window | `timestamp outside tolerance: event=…, now=…` |
| Missing `u` tag | `missing 'u' tag` |
| URL mismatch | `URL mismatch: token=…, expected=…` |
| Missing `method` tag | `missing 'method' tag` |
| Method mismatch | `method mismatch: …` |
| Body provided but no payload tag | `body provided but no payload tag` |
| Payload hash mismatch | `payload hash mismatch` |

DPoP/OIDC paths reuse `PodError::Nip98` as a generic "auth failure"
channel. Inspect the message for a more specific cause.

### `Io`

Any `std::io::Error` — FS backend reads/writes, `tokio::fs` failures.

### `Watch`

Wraps `notify::Error`. Produced by `FsBackend::watch` on subscription
setup failures.

### `Backend`

Reserve for custom backends to signal backend-specific failures that
don't fit elsewhere. Neither of the built-in backends produces this.

### `PreconditionFailed`

- `apply_n3_patch` when a WHERE clause triple is missing from the
  target graph.
- Reserved for `If-Match` enforcement at the HTTP layer (P2 item).

### `Unsupported`

Emitted when:

- `Graph::parse_ntriples` hits an unparseable line.
- `apply_sparql_patch` encounters a SPARQL op other than
  `INSERT DATA` / `DELETE DATA` / `DELETE/INSERT WHERE`.
- `Jwk::thumbprint` on an unknown `kty`.

## Mapping in code

Reference implementation:

```rust
use solid_pod_rs::error::PodError;

fn to_http(err: &PodError) -> http::StatusCode {
    use http::StatusCode as S;
    match err {
        PodError::NotFound(_)           => S::NOT_FOUND,
        PodError::AlreadyExists(_)      => S::CONFLICT,
        PodError::Forbidden             => S::FORBIDDEN,
        PodError::Unauthenticated
        | PodError::Nip98(_)            => S::UNAUTHORIZED,
        PodError::InvalidPath(_)
        | PodError::Json(_)
        | PodError::UrlParse(_)
        | PodError::Base64(_)
        | PodError::Hex(_)              => S::BAD_REQUEST,
        PodError::InvalidContentType(_) => S::UNSUPPORTED_MEDIA_TYPE,
        PodError::PreconditionFailed(_) => S::PRECONDITION_FAILED,
        PodError::Unsupported(_)        => S::BAD_REQUEST,
        _                               => S::INTERNAL_SERVER_ERROR,
    }
}
```

## Security note — what **not** to leak

- Never echo the NIP-98 token, the DPoP proof, or the access token
  back to the client.
- Never surface whether an ACL is missing vs denying — both cases
  return 403 (or 401 if unauthenticated), identical body.
- Never echo server filesystem paths on `InvalidPath` — sanitise or
  replace with the request path.

## See also

- [reference/api.md §PodError](api.md#error)
- [reference/http-endpoints.md](http-endpoints.md)
- [explanation/security-model.md](../explanation/security-model.md)
