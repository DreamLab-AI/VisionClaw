# Security model

This page describes the threat model solid-pod-rs is built to defend
against, the controls the crate provides, and what remains the
integrator's responsibility.

## Scope

We defend against:

- Unauthenticated access to non-public resources.
- Authenticated-but-unauthorised access (WAC).
- Token replay (both NIP-98 and DPoP-bound OIDC).
- Path traversal / escape in storage backends.
- Request tampering on bodies (both dialects bind the body hash).

We **do not** (as a library) defend against:

- TLS termination weaknesses — that's the reverse proxy's job.
- Application-layer rate limiting.
- Denial of service from oversized requests — use your HTTP
  framework's body-size limits.
- OS-level integrity — if an attacker can write to `$POD_FS_ROOT`
  directly, they own the pod regardless of what the library does.

## Auth layering

Two layers, independent, stackable:

### Layer 1 — Request authentication

Either NIP-98 or Solid-OIDC DPoP. Both produce:

- A verified identity token (pubkey or WebID).
- A bound URL + method + optional body hash.

Characteristics:

| Property                        | NIP-98                          | Solid-OIDC DPoP                 |
|---------------------------------|---------------------------------|---------------------------------|
| Transport                       | HTTP `Authorization: Nostr …`   | HTTP `Authorization: DPoP …` + `DPoP: …` |
| Event / token format            | Nostr kind 27235 event, base64  | JWT access token + DPoP proof JWT |
| Signature algorithm             | Schnorr over secp256k1 (P1: structural only; P2: full) | ES256 / RS256 (access token + DPoP proof) |
| Binds URL                       | `u` tag                         | `htu` claim (DPoP proof)        |
| Binds method                    | `method` tag                    | `htm` claim                     |
| Binds body                      | `payload` tag = `SHA-256(body)` | Access-token handling; proof's `ath` if applicable |
| Timestamp tolerance             | ±60 s                           | Configurable `skew` (default 60 s) |
| Key-to-identity                 | pubkey → `did:nostr:{pubkey}`   | DPoP thumbprint bound via `cnf.jkt` |
| Anti-replay                     | Per-request timestamp window    | `jti` nonce cache (consumer-crate concern) |

Both layers produce an `agent_uri` string that feeds the WAC evaluator.

### Layer 2 — WAC authorisation

Once authenticated, every request is filtered by
`wac::evaluate_access`. The WAC evaluator:

- Walks up the tree looking for `.acl` sidecars.
- Parses JSON-LD authorizations.
- Checks agent matchers (`acl:agent`, `acl:agentClass`,
  `acl:agentGroup`).
- Checks mode (`acl:Read`, `Write`, `Append`, `Control`) with the
  single implication rule: `Write ⇒ Append`.
- Returns a boolean.

Deny-by-default: no ACL, no access.

## NIP-98 threat cases

### Token replay at a different URL

Mitigated. The `u` tag must match the canonical URL (trailing-slash
normalised). A token minted for `/profile/card` is rejected at
`/public/secrets`.

### Token replay at a different method

Mitigated. The `method` tag must match.

### Token replay with a modified body

Mitigated. If the body is non-empty, the `payload` tag must equal
`SHA-256(body)`. A token with no `payload` tag is rejected if a body
is provided.

### Token replay in the future

The 60 s window bounds replay. Clocks must be synced within that
window (NTP). We do **not** implement jti-cache — the timestamp
window is the only bound.

### Enlarged token to exhaust the server

Mitigated by `MAX_EVENT_SIZE = 64 KB`. Tokens larger than this (pre-
or post-base64-decode) are rejected without parsing.

### Non-standard kind

Mitigated. `kind != 27235` → rejected.

### Invalid pubkey format

Mitigated. 64 hex chars required; non-hex rejected.

## Solid-OIDC threat cases

### Bearer-token theft

Mitigated *at the protocol level* by DPoP: the client must prove
possession of the keypair whose thumbprint appears in the access
token's `cnf.jkt`. A stolen access token without the DPoP key is
useless.

### DPoP proof replay at a different URL / method

Mitigated. `htu` and `htm` claims are checked against the actual
request.

### DPoP proof replay in time

Mitigated by `iat` skew. We do not implement a jti nonce cache —
consumers that need stronger replay protection should add one in
middleware.

### Access-token substitution

Mitigated by issuer validation — `verify_access_token` enforces
`iss == expected_issuer`.

### WebID impersonation

Mitigated. `extract_webid` only accepts URL-shaped WebIDs from either
the `webid` claim (explicit) or the `sub` claim (fallback). Non-URL
`sub` values are rejected.

## WAC threat cases

### Modifying `.acl` without authorisation

`.acl` is a resource like any other, gated by the ACL effective for
**its own path**. Convention: granting `acl:Control` on a resource
permits writing its `.acl` sidecar. Our example server does not
special-case `.acl` writes — the HTTP layer must check
`AccessMode::Control` when the request URI ends in `.acl`.

### Walking up past the pod root

The resolver walks from the resource path up to `/`. It terminates at
`/`. There is no way to escape to the host filesystem.

### ACL document injection

`StorageAclResolver::find_effective_acl` silently ignores
deserialisation failures (treats them as "no ACL found"). This is the
safer default — a corrupted ACL must not accidentally grant access.
A noisier mode that raises `AclParse` would invite
denial-of-service via deliberately broken ACL documents.

## Storage threat cases

### Path traversal via `..`

Both built-in backends reject paths containing `..` or `\0` in
`normalize`. Custom backends **must** do the same.

### Path traversal via URL encoding

URL decoding happens in the HTTP framework, before solid-pod-rs sees
the path. Ensure the framework's URL-decoder is correct (actix-web,
axum, and hyper all handle this).

### Symlink escape (FS backend)

The `FsBackend::resolve` path check (`full.starts_with(root)`) is
best-effort — on filesystems that follow symlinks, a malicious
symlink inside the root could redirect writes. Run the pod as a user
with write access only to the pod root and nothing else.

### Concurrent mutation

Both backends are `Send + Sync` and use appropriate synchronisation.
Custom backends must preserve "either old or new state, never mid-
write" semantics for `put`.

## What integrators must add

### HTTP-layer hardening

- TLS termination with a strong cipher suite (TLS 1.3 only when
  possible).
- Body-size limits on every method (both NIP-98 limit ≤ 64 KB for
  the *token*; the *body* itself should be bounded realistically).
- Rate limiting per identity (not per IP — authenticated identity is
  the natural axis).
- Short request timeouts (PATCH blocks can pathologically evaluate).

### DPoP jti cache

Solid-OIDC DPoP nominally requires a short-TTL cache of seen `jti`
values to make replay protection strict. solid-pod-rs doesn't ship
one — it's a deployment concern (you choose Redis, in-memory, local
LRU).

### WebID-OIDC issuer trust

If you accept arbitrary OIDC issuers (not a single one), implement an
issuer allow-list. The crate's `verify_access_token` takes a single
`expected_issuer` argument — the caller decides which issuers are
accepted.

### Audit logging

Log every 401 / 403 with the identity that was rejected and the
resource path. Attackers probing for access leave patterns.

### ACL review pipeline

Treat `.acl` files as code. Review every change. A misplaced
`foaf:Agent` grants the world.

## Defence-in-depth recommendations

1. TLS everywhere. NIP-98 and DPoP both trust the transport.
2. Strong body caps (e.g., 10 MB per resource at the proxy).
3. Non-root pod process, with write access limited to the pod root.
4. No `public` network access on the pod port — traffic should come
   exclusively through the reverse proxy.
5. Rotate OIDC HS256 secrets if used (production should use ES256 /
   RS256 + JWKS, so rotation happens via the OP's JWKS endpoint).
6. Keep `RUST_LOG=solid_pod_rs=info` or stricter in production —
   `debug` may log token metadata in verbose contexts.

## See also

- [how-to/configure-nip98-auth.md](../how-to/configure-nip98-auth.md)
- [how-to/enable-solid-oidc.md](../how-to/enable-solid-oidc.md)
- [how-to/debug-acl-denials.md](../how-to/debug-acl-denials.md)
- [reference/wac-modes.md](../reference/wac-modes.md)
- [RFC 9449 DPoP](https://datatracker.ietf.org/doc/html/rfc9449)
- [NIP-98](https://github.com/nostr-protocol/nips/blob/master/98.md)
