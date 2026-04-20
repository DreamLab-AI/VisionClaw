# HTTP endpoint reference

solid-pod-rs is framework-agnostic: it does not ship an HTTP server.
This page describes the endpoint surface your integration should
expose, derived from the example in
[`examples/standalone.rs`](../../examples/standalone.rs) and the LDP
specification.

## Method matrix

| Path kind                    | GET | HEAD | PUT | POST | DELETE | PATCH |
|------------------------------|-----|------|-----|------|--------|-------|
| Pod root `/`                 | ✓ container | ✓ | ✗ 405 | ✓ | ✗ 405 | ✗ 405 |
| Container `/c/`              | ✓ container | ✓ | ✗ 405 | ✓ | ✓ if empty | ✗ 405 |
| Resource `/c/r`              | ✓ body | ✓ | ✓ | ✗ 405 | ✓ | ✓ N3 or SPARQL |
| ACL sidecar `/c/r.acl`       | ✓ | ✓ | ✓ (acl:Control required) | ✗ | ✓ | ✗ |
| Meta sidecar `/c/r.meta`     | ✓ | ✓ | server-managed | ✗ | ✗ | ✗ |
| `.well-known/openid-configuration` | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| `.notifications`             | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| `.notifications/websocket`   | ✗ | ✗ | ✗ | ✓ (subscribe) | ✗ | ✗ |
| `.notifications/webhook`     | ✗ | ✗ | ✗ | ✓ (subscribe) | ✗ | ✗ |
| `/.well-known/solid`         | partial (OIDC discovery only) | | | | | |

## Response status codes

| Code | Use |
|---|---|
| 200 OK | `GET` / `HEAD` success |
| 201 Created | `PUT` (new resource) or `POST` (Slug → child) |
| 204 No Content | `DELETE`, successful `PATCH` |
| 400 Bad Request | Malformed body, invalid path, base64/hex/JSON decode error |
| 401 Unauthorized | Missing or invalid NIP-98 / OIDC token (send `WWW-Authenticate`) |
| 403 Forbidden | ACL evaluator denied |
| 404 Not Found | `PodError::NotFound` |
| 405 Method Not Allowed | See matrix |
| 409 Conflict | `PodError::AlreadyExists` — usually only for idempotent-create semantics |
| 412 Precondition Failed | N3 PATCH `where` clause missed, or `If-Match` mismatch |
| 415 Unsupported Media Type | Unknown `Content-Type` for resource kind or PATCH |
| 500 Internal Server Error | I/O, backend, or `PodError::Backend` |

See [reference/error-codes.md](error-codes.md) for the `PodError` →
status mapping.

## Response headers (per method / path kind)

### All non-error responses

- `Link` — see [reference/link-headers.md](link-headers.md).
- `WAC-Allow` — derived via `wac::wac_allow_header`. Shape
  `user="…", public="…"`.
- `ETag` — strong validator on resource bodies (SHA-256 hex).
- `Accept-Post` — on containers: `text/turtle, application/ld+json,
  application/n-triples` (`ldp::ACCEPT_POST`).
- `Content-Type` — passed through from `ResourceMeta.content_type` for
  resources; `application/ld+json` or the negotiated RDF format for
  containers.

### `401`

```
WWW-Authenticate: Nostr
WWW-Authenticate: DPoP algs="ES256 RS256"
```

## Request headers the server honours

| Header | Effect |
|---|---|
| `Authorization: Nostr <b64>` | NIP-98 authentication — `auth::nip98::verify`. |
| `Authorization: DPoP <token>` | Solid-OIDC access token (feature `oidc`). |
| `DPoP` | DPoP proof — `oidc::verify_dpop_proof`. |
| `Content-Type` | Stored verbatim on `PUT`; selects PATCH dialect (`text/n3` or `application/sparql-update`). |
| `Accept` | Drives RDF format for container GET — `ldp::negotiate_format`. |
| `Prefer` | Controls container representation — `ldp::PreferHeader::parse`. See [reference/prefer-headers.md](prefer-headers.md). |
| `Slug` | On `POST` to container: UTF-8 child name. Rejected if contains `/` or `..`. |
| `If-Match` / `If-None-Match` | P2 item — the storage layer returns canonical ETags; middleware enforces. |

## Path conventions

- Container paths end with `/`; resources do not.
- ACL sidecar for a resource `/c/r` lives at `/c/r.acl`.
- ACL sidecar for a container `/c/` lives at `/c/.acl`.
- Meta sidecar for `/c/r` lives at `/c/r.meta`.
- Pod root ACL lives at `/.acl`.
- Root path `/` is always a container.
- Paths are resolved case-sensitive.
- Backends MUST reject paths containing `..` or `\0`.

## Slug semantics on POST

```rust
pub fn resolve_slug(container: &str, slug: Option<&str>) -> String;
```

- If `slug` is `Some(s)` and `s` is non-empty, contains no `/`, and
  contains no `..`: append `s` to `container`.
- Otherwise: append a fresh UUID v4.

## Discovery endpoints

### `GET /.well-known/openid-configuration`

Feature `oidc` required. Build with `oidc::discovery_for(issuer)`.

### `GET /.notifications`

Returns the subscription-discovery JSON-LD document. Build with
`notifications::discovery_document(pod_base)`.

```json
{
  "@context": ["https://www.w3.org/ns/solid/notifications-context/v1"],
  "id":            "https://pod.example/.notifications",
  "channelTypes": [
    { "id": "WebSocketChannel2023", "endpoint": ".../websocket", "features": ["as:Create","as:Update","as:Delete"] },
    { "id": "WebhookChannel2023",   "endpoint": ".../webhook",   "features": ["as:Create","as:Update","as:Delete"] }
  ]
}
```

## See also

- [reference/api.md](api.md) — the Rust API backing each endpoint.
- [reference/link-headers.md](link-headers.md)
- [reference/prefer-headers.md](prefer-headers.md)
- [reference/content-types.md](content-types.md)
- [reference/patch-semantics.md](patch-semantics.md)
