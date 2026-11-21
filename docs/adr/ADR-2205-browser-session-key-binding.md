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
   a re-bind; Redis persistence is not wired). Each bind has an absolute
   lifetime, `min(requested, SESSION_KEY_MAX_TTL_SECS, 3600)`, default 3600;
   using a key never extends it (no sliding renewal). The same principal may
   bind again, which needs a fresh real signature and restarts the clock.
   **Proof of possession:** the body carries `proof`, a NIP-01 event of kind
   27236 signed *by the session key*, tagged with the principal, the bind URL,
   the requested `expires` and a `nonce`, created within 60 s. The server
   verifies it before binding, so nobody can bind a key they do not hold (a
   victim's long-term key would otherwise be locked out of its own account).
   **Admission:** only established principals may bind, meaning a configured
   power user (`POWER_USER_PUBKEYS`) or the holder of an explicit RBAC role
   row; anyone else gets `403 {"code":"session_key_not_admitted"}`, no user is
   created and nothing is stored, and the client signs per request for the
   rest of that session. (`APPROVED_PUBKEYS` is no signal: every
   authenticating key is added to it.) Without this, unlimited fresh keypairs
   could each become a principal and fill the table. The principal's user is
   created before the binding is inserted, so a creation failure cannot leave
   an orphan binding. A key that is already an account (has authenticated as itself, or is a
   configured power user) is refused as a session key. Per principal: at most 8
   live keys (oldest evicted), 10 binds a minute, and 120 successful binds a
   minute across all principals (`429`). Expired entries
   are removed on the bind path (the principal's own, and the table once a
   minute); the auth lookup takes a read lock and never scans.
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


> Note 2026-11-21 (open policy decision, W3-02): `GET /api/auth/nostr/api-keys` returns the caller's stored provider keys in plaintext, to the authenticated owner only, with `Cache-Control: no-store` (`src/handlers/nostr_handler.rs` `get_api_keys`; asserted by `post_without_pubkey_segment_with_valid_session_succeeds`). A session key is already refused on this route by the scope rule above. Options: (a) keep plaintext read-back to the owner, as now; (b) return presence booleans only (`{"openai": true, ...}`), so a stored secret is never sent to any client, and add a write-only "replace" flow in the client. The client does not call this route today, so (b) breaks nothing; the choice is the operator's. Related fix the same day: `POST` now patches instead of replacing (`NostrService::update_user_api_keys`), so saving one provider no longer erases the others.

## Consequences

- A NIP-07 signer is asked once per session (plus deny-list and Solid requests).
- **Accepted risk:** script running in the page (XSS) can use the in-memory key
  until it expires, as it can call `window.nostr.signEvent` today. It can also
  **export** the key and sign requests off-browser until expiry; there is no
  automatic revocation on compromise. The exposure is bounded by the TTL (at
  most 3600 s), the deny list, and the per-principal caps. Revocation exists but
  is deliberate: `DELETE /api/auth/nostr/session-key`, signed by the session key
  (drops that key) or by the principal (drops all its keys), which the client
  calls on logout.
- The binding table is per process and in memory: restarts and multiple
  instances without sticky routing force re-binds (a distinct 401, handled).
- Members without a role row or power-user status keep per-request signing
  (one prompt each) until an admin assigns a role.
- A binding otherwise ends at its TTL or a restart. The maximum TTL is read from `SESSION_KEY_MAX_TTL_SECS`, not a stored setting.
- Redis is not used: production builds with `--features gpu` only.
- `ragflow_handler` still authenticates by `X-Nostr-Pubkey` plus a legacy
  bearer, so it is unreachable while legacy sessions are off.

## Verification

Route-level tests in `tests/adr2205_session_key.rs` (23, plus `adr2205_admission_role.rs`, including admission, proof of
possession, the victim-lockout case, caps, rate limit and revocation) and store unit tests in
`src/services/session_keys.rs`; client tests in
`client/src/services/__tests__/sessionKey.test.ts` (18), including zero
`signEvent` calls after the bind. `implementation_status` stays `partial` and
`activation_status` `inactive` until a deployed build is observed.
