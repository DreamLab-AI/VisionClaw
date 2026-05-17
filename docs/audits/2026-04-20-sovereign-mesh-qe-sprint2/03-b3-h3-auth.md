# 03 — B3 + H3 Auth Hardening (ADR-028-ext §body-binding + §prod-gate)

**Status: CLOSED (both).**

## B3 — NIP-98 body-hash binding

### Primary verifier (`src/utils/auth.rs`)

Sprint 1 sig: `verify_nip98_auth(auth_value, &url, method, None)` at the
pre-B3 call site; token usable against any body.

Sprint 2:

- New entry-point `verify_access_with_body(req, nostr, required, body: Option<&[u8]>)`
  at :127-396. The legacy `verify_access` at :116-122 delegates with
  `body=None` for middleware-layer call sites where the body is not yet
  buffered — documented in-line (:112-115) as an explicit design
  trade-off.
- Body bytes are lossily converted to UTF-8 (:248-249), yielding an
  `Option<&str>` that is handed to `NostrService::verify_nip98_auth` at
  :251-253. The verifier takes a SHA-256 of the canonical bytes the
  client signed; the comment block at :236-247 spells out why lossy
  UTF-8 is safe: tampered bodies still produce a different hash (the
  attack surface is exact-byte replay, not charset edge cases).

### Handler-layer call site (`src/handlers/solid_proxy_handler.rs`)

- `get_user_from_request(req, nostr, body: Option<&[u8]>)` at :1559-1616
  now takes a body parameter. The only production call site that
  receives a buffered body (POST / PUT / PATCH on the Pod proxy) sits at
  :1325 and passes `Some(body.as_ref())`. Two auxiliary lifecycle
  endpoints at :1383 and :1448 intentionally pass `None` — they are
  bodyless HEAD-like pod-init triggers (the docstring at :1556-1558
  makes the contract explicit).
- Body conversion at :1595-1596 mirrors the primary verifier for
  consistency; the call to `verify_nip98_auth` at :1597-1599 now
  includes `body_ref`.

### Replay test (`tests/auth_hardening.rs:82-139`)

| Test | Asserts |
|------|---------|
| `b3a_tampered_body_is_rejected` | Sign body A, replay against body B ⇒ 401. |
| `b3b_matching_body_is_accepted` | Matching body ⇒ 200 with echoed pubkey. |
| `b3c_empty_body_post_is_accepted` | Bodyless POST signed with empty payload hash ⇒ 200. |

Together these pin the contract both ways: tampered ⇒ reject, matching
⇒ accept, empty ⇒ accept-iff-client-included-empty-hash.

## H3 — APP_ENV fail-closed default

### `is_production()` at `src/utils/auth.rs:27-41`

```
match std::env::var("APP_ENV") {
    Ok(v) => v == "production",
    Err(_) => {
        WARN_ONCE.call_once(|| {
            warn!("APP_ENV unset — defaulting to production for safety; ...");
        });
        true
    }
}
```

The inversion vs Sprint 1: missing env var now returns **`true`** (treat
as production), so any dev-mode bypass is disabled by default. The
`WARN_ONCE` emits a single startup log line for operator visibility — a
deployment that genuinely wants dev-mode must `APP_ENV=development`
explicitly; an operator who simply forgot the variable now sees a
one-shot warning instead of a silent security regression.

### Dev-bypass gating (:177-205)

The `Bearer dev-session-token` branch at :177-205 is wrapped in
`if !is_production()` (:184). A missing `APP_ENV` returns `true` from
`is_production()`, so the guard evaluates to `false` and the branch is
skipped; the request falls through to NIP-98 verification or the legacy
`X-Nostr-Pubkey` path, both of which also gate on the same probe.

### Legacy path rejection (:294-310)

`if is_production() { return Err(HttpResponse::Unauthorized()...) }`
makes the legacy unsigned `X-Nostr-Pubkey + X-Nostr-Token` flow
**unavailable** in production regardless of which flag combinations an
operator sets. The ADR-055 H3 comment at :300-302 points back to this
file to anchor the decision for future review.

### Tests (`tests/auth_hardening.rs:202-309`)

| Test | Asserts |
|------|---------|
| `h3a_unset_app_env_is_production_mode` | Remove `APP_ENV`, legacy auth rejected with 401. |
| `h3b_development_accepts_legacy_path` | `APP_ENV=development` ⇒ 200 on legacy bearer+pubkey. |
| `h3c_production_rejects_legacy_path` | `APP_ENV=production` ⇒ 401. |

## Verdict

Both findings closed. The body-binding plumbing reaches every NIP-98
verifier call site that handles a mutating request, and the `is_production()`
probe fails closed with a single central definition. One minor note on
defence in depth: the `body` parameter is carried as `Option<&[u8]>` —
for call sites where the body is intentionally missing (bodyless
triggers at :1383 and :1448 of `solid_proxy_handler.rs`), a token
replayed from a body-bearing endpoint still fails because the NIP-98
`payload` tag's hash won't match the empty-body hash the verifier
computes. Safe in practice; noted for future hardening (report §6 R5
covers the `APP_ENV` poisoning angle).
