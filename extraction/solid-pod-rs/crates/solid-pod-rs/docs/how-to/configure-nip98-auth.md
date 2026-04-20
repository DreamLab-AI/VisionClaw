# How to configure NIP-98 authentication

**Goal:** verify a NIP-98 `Authorization: Nostr <base64>` header on
every request to your pod.

## When to use this

You want lightweight request-bound authentication with no OIDC
infrastructure. NIP-98 binds a signed Nostr event to a single HTTP
request; replay windows are ≤ 60 s.

See [explanation/security-model.md](../explanation/security-model.md)
for the threat model.

## Step 1 — Extract the token

Pull the header and pass its value verbatim to `verify` (async) or
`verify_at` (deterministic, for tests).

```rust
use solid_pod_rs::auth::nip98;

async fn extract_pubkey(req: &HttpRequest) -> Option<String> {
    let header = req.headers().get("authorization")?.to_str().ok()?;
    let url = format!("http://{}{}", req.connection_info().host(), req.uri().path());
    nip98::verify(header, &url, req.method().as_str(), /* body */ None)
        .await
        .ok()
}
```

Rules the verifier enforces (all in
[`auth::nip98::verify_at`](../reference/api.md#authnip98)):

- Header begins with `Nostr ` (note the trailing space).
- Base64 token decodes to JSON ≤ 64 KB.
- Event `kind == 27235`.
- `pubkey` is 64 hex chars.
- `created_at` within 60 s of now.
- `u` tag matches the canonical URL (trailing slashes ignored).
- `method` tag matches the HTTP method (case-insensitive).
- If a body is passed, the `payload` tag must be SHA-256(body).

## Step 2 — Bind the body hash

For `PUT`, `POST`, `PATCH` — anything with a body — pass the body to
`verify`. Otherwise a token minted for an innocuous payload can be
replayed against a malicious one.

```rust
let pubkey = nip98::verify(header, &url, method, Some(&body_bytes)).await?;
```

The verifier rejects requests where:

- A `payload` tag is present but doesn't match `sha256(body)`.
- A body is present but the token has no `payload` tag.

## Step 3 — Map pubkey to WebID

NIP-98 produces a Nostr pubkey, not a WebID. Map it consistently:

```rust
let agent_uri = format!("did:nostr:{pubkey}");
```

Pass `agent_uri` to `wac::evaluate_access` and other WAC APIs. This
convention matches the stock example server.

## Step 4 — Handle errors

```rust
match nip98::verify(header, &url, method, body.as_deref()).await {
    Ok(pk)  => { /* authenticated */ }
    Err(PodError::Nip98(msg)) => return HttpResponse::Unauthorized()
        .insert_header(("www-authenticate", "Nostr"))
        .body(msg),
    Err(_)  => return HttpResponse::InternalServerError().finish(),
}
```

Always return `401` with `WWW-Authenticate: Nostr` — the standard
challenge header. Never echo the token back to the client.

## Step 5 — Advanced: dev-mode bypass

solid-pod-rs intentionally does **not** ship a dev-mode bypass; see
[PARITY-CHECKLIST.md](../../PARITY-CHECKLIST.md#authentication). If
your testbed needs one, put it in a wrapper middleware that skips
`verify` on a dev host, not in the library.

## See also

- [reference/api.md §auth::nip98](../reference/api.md#authnip98)
- [explanation/security-model.md](../explanation/security-model.md)
- Tutorial: [Adding access control](../tutorials/03-adding-access-control.md)
