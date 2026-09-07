---
id: ADR-2009
title: Two request-auth realms coexist — NIP-98 signatures and login-derived session bearers
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
verified_paths: [src/utils/auth.rs, src/services/nostr_service.rs, src/middleware/rbac_gate.rs, client/src/services/api/authInterceptor.ts]
owner: jjohare
review_trigger: React client migrating to per-request NIP-98 signing, or any multi-tenant deployment where session-bearer mutations are unacceptable
repo: visionclaw
domain: IDENTITY-authority-chain
lineage: "legacy ADR-011, ADR-142; corrects the aspiration that NIP-98 is the sole request-auth realm"
---

# ADR-2009 — Two request-auth realms coexist — NIP-98 signatures and login-derived session bearers

## Context

The original draft of this record claimed NIP-98 was the *sole* request-auth
realm. Adversarial verification refuted it: `verify_access`
(`src/utils/auth.rs`), which `RbacGate` consults for **every** `/api` route
including `WriteGraph` mutations (`rbac_gate.rs:270`), carries an unconditional
legacy fallback authenticating `X-Nostr-Pubkey` + `X-Nostr-Token` headers via
`validate_session` (`src/services/nostr_service.rs:478`). The original verification found React client dependence on this path. The current
interceptor has migrated to per-request signing; see the dated closeout review
below before relying on that historical retirement constraint.

## Decision

Both realms are accepted as live, with an explicit quality ordering. **NIP-98**
(Schnorr per-request signatures + freshness window + single-use replay cache,
ADR-2002) is the primary realm and the only one the XR client and agents use.
**Legacy session bearers** (UUID minted at Schnorr-verified login, expiring
`token_expiry` after `last_seen`, plain-equality check) remain accepted on REST
for the browser client's benefit. This forecloses pretending the estate has
signature-grade non-replayability on the REST surface: a captured session
header pair is replayable until expiry, unlike a NIP-98 token.

## Consequences

- The RBAC lattice binds to the pubkey regardless of realm, so role enforcement
  is uniform; only the *transport credential* strength differs.
- ADR-2002's replay guarantees apply to the NIP-98 realm only — documentation
  and security reviews must not claim them for the whole REST surface.
- Retiring the legacy realm requires the React client to sign per-request
  (NIP-98 or equivalent) first; that migration is the recorded exit path (see
  review_trigger).
- `docs/IDENTITY-authority-chain.md` must present both realms (it does).

## Verification

`src/utils/auth.rs` ~218-260 legacy branch confirmed unconditional and
non-cfg-gated at `e0f8cd896`; `validate_session` expiry window confirmed at
`nostr_service.rs:478-488`; client dependence confirmed across five
`client/src` files (authInterceptor, restClient, endpoints, ldpClient,
contextLoader). Original sole-realm draft deleted by the adversarial verify
pass of wf_0d0794b9-02c; this record replaces it stating the dual-realm truth.

## Closeout extension — 2026-09-04

CP-01/04/05/08. Owner remains jjohare with identity/client/release maintainers. Complete/live is retained for server coexistence. The browser migration review trigger is reached: current API interceptor, settings and LDP source sign NIP-98, and eleven mocked interceptor tests pass. The old verification remains historical; it does not describe current interceptor dependence. Legacy server acceptance remains active. Session age uses mutable last_seen, not an immutable issuance timestamp.

**Acceptance condition:** Inventory every deployed credential consumer, including sockets and external clients; choose a dated compatibility/retirement contract. Verify realm precedence, body binding, effective roles, refresh/logout/revocation, persisted-session restart and rollback through real routes. Reopen on consumer migration, session lifetime/refresh changes or deployment profile changes. See [request realm review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#request-realms-and-deferred-delegation) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/auth-realms-snapshot.json). Client mocks establish header construction only; no session or deployment was changed.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed changes since `f326a3b11`:** `client/src/services/api/authInterceptor.ts`
(+75/-27, the NIP-98 signing migration), `src/services/nostr_service.rs` (+79/-9,
shared session-freshness helper) and `src/middleware/rbac_gate.rs` (+12/-11, the
ADR-2012 report-mode de-duplication). `src/utils/auth.rs` is **unchanged** since
`f326a3b11` (`git diff --stat` empty), so the legacy realm is untouched.

**The dual-realm decision still holds — and is still the uncomfortable truth.**
`verify_access` (`src/utils/auth.rs:142`) still carries the unconditional,
non-`cfg`-gated legacy branch: `X-Nostr-Pubkey` at `:277`, `X-Nostr-Token` at
`:289`, accepted via `nostr_service.validate_session(&pubkey, &token)` at `:310`.
`RbacGate` still routes **every** non-public `/api` request through it —
`verify_access(req.request(), &nostr_service, level.clone())` at
`src/middleware/rbac_gate.rs:270` (was `:265`; the +3-line `security_profile`
import shifted it — citation corrected in Context above).

`validate_session` is still at `src/services/nostr_service.rs:478`, so that
citation was already right. Its expiry check has been refactored, not relaxed:
it now delegates to `Self::session_is_fresh(user.last_seen, now,
self.token_expiry)` at `:486`, defined at `:597-599` as
`now.saturating_sub(last_seen).abs() <= token_expiry`. The doc comment at
`:593-595` states the reason — the WebSocket realm (`:585`) now shares the same
helper, so the two cannot drift apart again. Session age is still measured from
the mutable `last_seen` (`:505`), not an immutable issuance timestamp, exactly as
the 2026-09-04 closeout recorded. The bearer remains replayable until expiry;
ADR-2002's guarantees still apply to the NIP-98 realm only.

**Client migration unchanged in substance:** the interceptor signs per request,
but the server-side legacy acceptance is still live, so the review trigger
(retiring the legacy realm) is *not* yet discharged.

**Commands run:** `git diff --stat f326a3b11..HEAD -- src/utils/auth.rs
src/services/nostr_service.rs src/middleware/rbac_gate.rs
client/src/services/api/authInterceptor.ts`; `grep -n` over `auth.rs` for
`X-Nostr-*`/`validate_session`/`verify_access` and over `nostr_service.rs` for
`validate_session`/`last_seen`/`token_expiry`; `awk` line dumps of
`rbac_gate.rs:259-274` and `auth.rs:270-315`.


## Source closeout verification — 2026-09-07

The React graph upgrade now signs its HTTP GET URL and fails before connection if signing is unavailable or declined. The server verifies the NIP-98 carrier before upgrading and never negotiates the credential subprotocol. REST already signed requests. Opaque session issuance, validation and refresh now default off; VISIONCLAW_LEGACY_SESSIONS=1/true explicitly restores compatibility for real unexpired tokens. The dual realm is conditional, superseding earlier claims of unconditional session acceptance. Remaining MCP clients and deployed proxy/reconnect acceptance remain outstanding.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.
