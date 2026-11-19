---
id: ADR-2205
title: A browser binds an ephemeral session key with one real signature and signs requests with it
date: 2026-11-19
decision_status: proposed
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: [src/services/session_keys.rs, src/services/nostr_service.rs, src/utils/auth.rs, src/handlers/nostr_handler.rs, client/src/services/nostrAuthService.ts]
owner: jjohare
review_trigger: any change to the NIP-98 verifier, the deny list, or the in-memory binding table (Redis persistence, multi-instance deployment)
repo: visionclaw
---

# ADR-2205 — A browser binds an ephemeral session key with one real signature and signs requests with it

Amends [ADR-2009](ADR-2009-dual-request-auth-realms.md).

## Context

The browser signs every REST call and WebSocket upgrade with NIP-98 through the
user's NIP-07 extension or passkey, so a NIP-07 signer such as Podkey is asked
once per request. Legacy opaque session bearers would remove the prompts but are
deliberately off (`VISIONCLAW_LEGACY_SESSIONS`; ADR-2009, ADR-2044): a captured
bearer header is replayable until expiry. The owner approved "one Podkey
signature per session instead of one per request", without reviving bearers.

## Decision

1. **Bind.** After sign-in the client creates an ephemeral secp256k1 key in
   memory only (never `localStorage`/`sessionStorage`) and calls
   `POST /api/auth/nostr/session-key` with `{"sessionPubkey": "<64 hex>",
   "expiresIn": <secs>}`, NIP-98-signed by the user's real signer. The token's
   `payload` tag is required and checked (`BodyBinding::Required`), so a captured
   bind token cannot bind a different key. The server records
   `session_pubkey -> {principal, expires_at, scope}` in memory (a restart forces
   a re-bind; Redis persistence is not wired). The lifetime is absolute:
   `min(requested, SESSION_KEY_MAX_TTL_SECS, 3600)`, default 3600, never renewed.
2. **Use.** Later requests are NIP-98-signed by the session key and carry a
   signed `["session", "<principal pubkey>"]` tag. The verifier is unchanged
   (freshness window, single-use replay cache). `NostrService::verify_nip98_identity`
   then resolves the identity once, for every caller:
   a live binding naming that principal authenticates as the **principal**
   (RBAC, power-user, owner filters, share creators, owner stamps all see the
   principal), with a `via_session_key` marker; a marked token with no live
   binding is `401 {"code":"session_key_expired"}`; a mismatched principal, or a
   bound key without the tag, is a plain 401. An unmarked, unbound key is
   ordinary NIP-98 as itself (XR client, agents), and is never given a principal.
   The marker is what turns a restart into a re-bind instead of a silent
   switch to a fresh user, and the principal in the tag stops a squatter who
   pre-binds someone else's session pubkey from being acted for.
3. **Scope.** A session key may not bind another key, manage API keys
   (`/api/auth/nostr/api-keys`), or administer RBAC (any non-GET under
   `/api/admin/rbac`): `403 {"code":"session_key_scope"}`, enforced in
   `verify_access` (`session_key_denied`), in the outer-identity reuse branch too.
   The Solid proxy, pod management and the payment boundary refuse session-key
   tokens (the Solid proxy forwards the token upstream, to be verified as the
   signer), and the client signs `/solid`, `/api/solid` and `/api/pay` with the
   real signer. Everything else the principal can do, it can do.
4. **Client.** `nostrAuth.signRequest` binds once (single flight), signs REST and
   WebSocket upgrades with the session key, re-binds ahead of expiry (margin
   90 s, beyond the 60 s freshness window), re-binds once and retries once on
   `session_key_expired`, falls back to per-request signing if the bind is
   refused, and zeroes the key on logout. The loopback dev-token path, the
   passkey local-key path and the XR client's own signing are unchanged.

## Consequences

- A NIP-07 signer is asked once per session (plus deny-list and Solid requests).
- **Accepted risk:** script running in the page (XSS) can use the in-memory key
  until it expires, as it can call `window.nostr.signEvent` today; the exposure
  is bounded by the TTL and the deny list, and the key never leaves memory.
- The binding table is per process and in memory: restarts and multiple
  instances without sticky routing force re-binds (a distinct 401, handled).
- No server-side revocation endpoint: a binding ends at its TTL or a restart.
  The maximum TTL is read from `SESSION_KEY_MAX_TTL_SECS`, not a stored setting.
- Redis is not used: production builds with `--features gpu` only.
- `ragflow_handler` still authenticates by `X-Nostr-Pubkey` plus a legacy
  bearer, so it is unreachable while legacy sessions are off.

## Verification

Route-level tests in `tests/adr2205_session_key.rs` (11) and store unit tests in
`src/services/session_keys.rs`; client tests in
`client/src/services/__tests__/sessionKey.test.ts` (11), including zero
`signEvent` calls after the bind. `implementation_status` stays `partial` and
`activation_status` `inactive` until a deployed build is observed.
