---
title: Identity & Authority Chain
doc_id: VC-IDENTITY
version: 0.1.2
status: draft-for-ratification
verified_commit: 
changelog:
  - "0.1.2 (2026-09-06): Remediation — 2026-09-05 section: Wave 3 ADRs (2094–2101, 2061, 2071, 2085; proposed 2102–2105) and the ledger/diagram re-verification landed in 2cf222406 — re-verified at "
  - "0.1.1 — corrected three off-by-a-few-lines citations (auth.rs dev-bypass, rbac_gate.rs Viewer-write test, http_handler.rs token block/query-param)"
sources:
  - src/utils/nip98.rs
  - src/utils/auth.rs
  - src/middleware/rbac_gate.rs
  - src/services/role_store.rs
  - src/services/nostr_service.rs
  - src/services/nostr_bridge.rs
  - src/handlers/socket_flow_handler/http_handler.rs
  - src/main.rs
date: 2026-08-31
---

# Identity & Authority Chain

## Purpose

Defines who a caller *is* (Nostr pubkey via NIP-98) and what they may *do* (the
RBAC lattice enforced at the `/api` scope). This is the single ground-truth map
of every path a request can gain authority, and every gap in that map.

## Current State

### Identity: did:nostr

Every identity is a Nostr public key (secp256k1 Schnorr, 64-char hex). There is
no username/password realm and no server-minted identity — the DID *is* the
pubkey. Roles are keyed on the lowercase-hex canonical form
(`role_store.rs:68` `canonicalise_pubkey`), applied on every read and write path
so mixed-case input always resolves one row.

### Request signing: NIP-98 (kind 27235)

`src/utils/nip98.rs` is the sole verifier. `validate_nip98_token`
(`nip98.rs:330`) enforces, in order: base64/UTF-8/JSON decode → kind ==
`HTTP_AUTH_KIND` 27235 (const `:20`, checked `:348`) → **symmetric ±60 s
freshness window** (`TOKEN_MAX_AGE_SECONDS`, `:169`; past-side `:362`,
future-side `:367`) → `u`/`method` tag extraction and match (`:376`–`:389`,
host-checked `urls_match` at `:524`) → optional payload SHA-256
(`compute_payload_hash`, `:132`, applied `:413`) → Schnorr signature verify
(`:426`) → **single-use replay claim** (`:435`).

The replay cache is new as of this rebuild and closes the gap the ±60 s window
left open. `claim_event_id` (`nip98.rs:234`) records a spent event id under a
`Mutex<HashMap>` (`REPLAY_CACHE`, `:215`, initialised via `replay_cache()` at
`:217`); a second presentation of the same id within `REPLAY_CACHE_TTL`
(= `2 * TOKEN_MAX_AGE_SECONDS`, `:178`, compared at `:255`) returns
`Nip98ValidationError::TokenReplayed`. Check-insert-prune runs under one lock, so
two concurrent validations of the same token cannot both win (no TOCTOU). The
claim happens *after* every other check passes, so a forged token cannot burn a
legitimate id. Covered by `test_replay_same_token_rejected` (`:797`).

`verify_nip98_auth` (`nostr_service.rs:648`) wraps the verifier, reconstructing
the signed URL from `X-Forwarded-Proto`/`X-Forwarded-Host` behind the TLS proxy,
then materialises/updates the `NostrUser` and its `is_power_user` flag.

### Session tokens (NostrService)

**Legacy opaque sessions are off by default (ADR-2009 amendment, 2026-10-19).** A login mints
a session token only when `VISIONCLAW_LEGACY_SESSIONS` is `1` or `true`, read once at service
construction (`legacy_sessions_enabled`, `nostr_service.rs:115`, field `:125`, captured `:174`);
otherwise the login response carries `session_token: None` (`:439`, `:732`), `validate_session`
(`:510`), `get_session` (`:611`) and `refresh_session` (`:527`) all refuse, and the REST
legacy-header branch below answers 401 "Invalid or expired session". The paragraph that follows
describes the behaviour with the flag on.

With the flag on, after a NIP-98 login `NostrService` issues an opaque random session token
(`Uuid::new_v4`, `nostr_service.rs:428`) with TTL from `AUTH_TOKEN_EXPIRY`
(default 3600, `:140`). `get_session` (`:611`) resolves a token back to its
`NostrUser` and `validate_session` (`:510`) checks a token against a known
pubkey. Both reject an empty token before any lookup and both enforce expiry
through one shared rule, `session_is_fresh(last_seen, now, token_expiry)`
(`:634`), so the WebSocket and REST realms cannot drift apart; a `last_seen` in
the future (a clock stepped backwards) is treated as stale rather than as an
unbounded lease. Session tokens are the WebSocket credential (below); the REST
`/api` scope re-verifies NIP-98 on each call rather than trusting the session
token.

Until ADR-2044 (2026-09-05) `get_session` enforced **no** expiry at all while
`validate_session` did, so the same credential was bounded on REST and valid
until logout on a socket. Every WebSocket upgrade now resolves its token through
the session realm and fails closed — an unknown token, an expired token, an
absent token and an absent `NostrService` all produce 401, because presence of a
token is not authentication.

### Authorization: the RBAC lattice (legacy ADR-142)

Roles, highest-first: **Owner > Admin > Editor > Viewer**
(`role_store.rs`, `CHECK` constraint `:42`). They map onto the `AccessLevel`
permission lattice in `src/utils/auth.rs:31` (`has_permission`, `:41`): a
required `WriteGraph` is satisfied by Editor→Authenticated/Admin but **not** by
a Viewer→ReadOnly — this is what actually denies Viewer writes
(`rbac_gate.rs:372`–`:373` test assertions in
`writes_are_gated_at_write_graph_not_mere_authenticated`, `:359`).

`effective_role` (`role_store.rs:194`) resolves precedence: explicit row →
`Admin` for a legacy power user (`POWER_USER_PUBKEYS`, `nostr_service.rs:134`) →
else `UserRole::default_authenticated()` = **Editor**. It **fails closed to
Viewer** on any lookup/parse error (`:204`). Role writes go through
`assign_checked`/`revoke_checked` (`:243`, `:310`), which enforce the `can_assign`
lattice and the last-Owner invariant atomically inside one transaction.

### The gate: RbacGate middleware

`src/middleware/rbac_gate.rs` applies `required_level` (`:133`) centrally across
the whole `/api` scope — closing the "15+ endpoints missing auth" gap in one
place. Matching is **whole-segment** (`segments`/`has_segment_prefix`, `:61`),
so `/api/administrator` does not inherit `/api/admin`. Policy:

- Public (no auth): `/api/auth/*`, `/api/client-logs`, `/api/health[z]`,
  `/api/readyz` (`PUBLIC_SEGMENT_PREFIXES`, `:52`).
- `/api/admin/*` → `Admin` for every method (`:147`).
- Safe methods (GET/HEAD/OPTIONS) → public when `RBAC_PUBLIC_READS` on, else
  `ReadOnly` (`:151`).
- Mutations → `WriteSettings` under `/api/settings`, else `WriteGraph` (`:164`).

On a gated route the middleware pulls `NostrService` from app data and calls
`verify_access` (`rbac_gate.rs:286` → `auth.rs:191`), then injects the
authenticated pubkey into request extensions for downstream handlers. `verify_access` accepts
two request realms in order: NIP-98 (`Authorization: Nostr …`, primary) and, only while legacy
sessions are enabled, the `X-Nostr-Pubkey` + `X-Nostr-Token` pair (`auth.rs:329-365`,
`validate_session`); a request with neither gets 401, an identified caller below the level 403
(ADR-2009).

### Shipped posture: open by default

The live compose sets `RBAC_PUBLIC_READS=1` and `RBAC_ALLOW_OWNERLESS=1`.
Consequences, by code:

- **Anonymous reads pass.** `public_reads_enabled` (`rbac_gate.rs:121`) *fails
  closed* structurally (absent flag ⇒ auth required) but the deployment sets it
  explicitly ⇒ every `/api` GET is public; startup logs a `warn!` (`:188`).
- **Any authenticated pubkey is an Editor.** An unassigned pubkey that passes
  NIP-98 becomes Editor — `UserRole::default_authenticated()`
  (`src/models/rbac.rs:70`), reached from `parse_default_role` for both unset and
  empty `RBAC_DEFAULT_ROLE` (`role_store.rs:197-198`) — i.e. can write the graph.
- **Ownerless boot is allowed.** `RBAC_ALLOW_OWNERLESS=1` downgrades the
  no-Owner condition from a fail-closed startup error to a warning
  (`main.rs:717`–`737`).

These are deliberate single-operator compatibility trade-offs, not bugs, but
they need a named security profile before multi-tenant use.

### Dev-auth triple gate

`Bearer dev-session-token` (arbitrary `X-Nostr-Pubkey`) is accepted **only**
when all three hold (`auth.rs:221-260`, `dev_bypass_permitted_for_addr` `:139`,
thin wrapper `dev_bypass_permitted` `:155`): compile-time
`debug_assertions`/`dev-auth` feature, runtime `DEV_AUTH_LOOPBACK=1`, **and** a
loopback peer address. A remote attacker on a debug build cannot reach it. It is
compiled out of release entirely.

`RBAC_GATE_MODE=report` (log-only, no enforcement) similarly refuses to activate
in release unless `RBAC_REPORT_MODE_ACK` equals today's UTC date, and logs at
`error` on every request it waves through (`rbac_gate.rs:80`).

### WebSocket / XR client auth

The `/wss` upgrade requires an Origin header (`http_handler.rs:124`) then a
session token, read from `Authorization: Bearer …` **or** `?token=` query
param (`:139`–`:150`), validated via `NostrService::get_session`. The XR/Godot
client holds a per-client secret (`XR_NOSTR_SECRET`, client-side; no server
reference) and signs NIP-98 to obtain a session, then presents the session token
on the socket. On validation failure the socket is rejected in release; the
insecure-allow branch is `#[cfg(debug_assertions/dev-auth)]` only.

### Trust-chain taxonomy (reviewer request)

| Class | Meaning | Exists today? |
|-------|---------|---------------|
| **User-signed** | Request Schnorr-signed by the acting user's own key (NIP-98) | **Yes** — the primary path (`auth.rs`, `nip98.rs`). |
| **Server-attested** | Server issues a bearer/session token after a prior signed login | **Conditional** — `NostrService` session tokens for `/wss` and legacy REST header validation exist but are issued and accepted only when `VISIONCLAW_LEGACY_SESSIONS=1/true` (default off; `auth.rs:329-365`, `nostr_service.rs:115`, `:510`). |
| **Service-signed** | A service/bot signs under its *own* key, not the user's | **Yes** — `nostr_bridge.rs:169` re-signs forum events under the bridge key. |
| **Delegated-agent-signed** | Agent signs *on behalf of* a user with a verifiable delegation (NIP-26) | **No — NOT IMPLEMENTED.** |

## Known divergences & open items

- **NIP-26 delegation is NOT wired.** `nostr_bridge.rs` re-signs under the
  bridge key (service-signed), which is *not* delegation — the original user
  authority is not carried. Fail-closed NIP-26 is deferred to the unbuilt Phase
  5. Key custody/rotation (legacy ADR-081) and delegated admin (legacy ADR-094)
  are frozen (2026-07-03). Pod signing can fall back unsigned (legacy agentbox
  ADR-026). Until this lands, no request can be attributed to a user *through*
  an agent.
- **`?token=` accepted on `/wss`** (`http_handler.rs:155`) contradicts legacy
  ADR-011's header-only stance. Medium severity, log-hygiene (tokens leak into
  proxy/access logs). The header path exists and is preferred. Deprecated for one
  release by ADR-2044 (2026-09-05) rather than removed, because XR and native
  clients cannot set headers on an upgrade; retirement is that record's review
  trigger. The pattern is systemic, not one route — it also appears in
  `client_messages_handler.rs:127`, `mcp_relay_handler.rs:461`,
  `multi_mcp_websocket_handler.rs:798`, `fastwebsockets_handler.rs:238` and
  `socket_flow_handler/filter_auth.rs:138` (WS message body).
- **WebSocket upgrades that accept any non-empty token** — partially resolved.
  `client_messages_handler` checked only that a token was present, never that it
  named a live session: **Resolved — ADR-2044 (2026-09-05)**. The same defect
  remains in `mcp_relay_handler.rs` and `multi_mcp_websocket_handler.rs`, which
  reference `NostrService` nowhere at all; routed to the estate lead, so ADR-2044
  is `implementation_status: partial` until those land. `speech_socket_handler`
  is **not** affected — it performs full NIP-98 verification (`:230`).
- **Open-by-default posture** (RBAC_PUBLIC_READS, RBAC_ALLOW_OWNERLESS,
  Editor-default) is intentional but unnamed. Needs a ratified security profile
  distinguishing single-operator from multi-tenant.
- **OIDC parked.** Legacy ADR-040 (OIDC) is superseded-in-part by ADR-142; the
  public-key identity foundation supports both signed requests and legacy session credentials (ADR-2009). Enterprise federation remains deferred for VisionClaw (ADR-2013).
- **agentbox AoE :9095 token auth** was `--auth none` on loopback with tokenless
  direct routes; token auth is staged for the next image rebuild (landing
  2026-08-31, not yet in the running image).
- **PUBKEY_VISIBILITY_FILTER** default flipped ON this rebuild (privacy encoder
  existed but was inert). Landing 2026-08-31.

## Invariants (must not silently change)

1. NIP-98 verification order is fixed: freshness → tag match (host-checked) →
   signature → **replay claim last**. The claim must never precede signature
   verification, or a forged token can burn a legitimate id.
2. The replay cache TTL must stay ≥ 2× the freshness window, else a token still
   inside its window becomes replayable after its cache entry prunes.
3. `effective_role` and `verify_access` fail **closed** (to Viewer / 401). No
   error path may widen access.
4. `RBAC_PUBLIC_READS` absence means auth-required (`unwrap_or(false)`); the flag
   may only *widen* reads when explicitly set.
5. The dev bypass and `RBAC_GATE_MODE=report` must remain unreachable in a
   release build without an explicit, dated, loopback-scoped acknowledgement.
6. Role escalation must be impossible through any settings-write endpoint: the
   `user_roles` table is isolated from the user-writable settings layer.
7. Last-Owner invariant: the sole Owner cannot be demoted or revoked.

## Change process

This is a living document. Amendments require: (1) the code change landed with
file:line evidence, (2) `verified_commit` bumped to the new HEAD, (3) any
invariant change called out explicitly in review. Legacy ADRs (011, 040, 081,
094, 142, agentbox ADR-026) are cited as evidence only — where they contradict
the code above, the code wins and the divergence is recorded here.

## RBAC closeout qualification — 2026-09-04

ADR-2010 is partial against broad atomic-authority claims: target checks transact, caller role is passed from an earlier resolution. Explicit-role removal restores fallback authority and is not necessarily access revocation. ADR-2011's central gate includes public-prefix and report-mode branches; its dated acknowledgement is checked at construction, not continuously. [Source evidence and acceptance](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md) require concurrency, composed-route, audit and lifecycle receipts before complete-system claims.

## Request-credential review — 2026-09-04

ADR-2009's browser migration trigger is reached in current source: the ordinary API interceptor signs NIP-98. Server legacy header acceptance remains in code but is conditional on `VISIONCLAW_LEGACY_SESSIONS` and off by default (ADR-2009 amendment, 2026-10-19); no in-tree client sends `X-Nostr-Token`. Eleven mocked interceptor tests cover header construction, not full-route authentication. Session age is relative to mutable last_seen. ADR-2013's enterprise/delegation deferral does not imply a single request-credential realm or govern other repositories' verifiers. See the [estate review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#request-realms-and-deferred-delegation) for consumer retirement, session recovery and delegated-authority acceptance.

## Remediation — 2026-09-05

- **ADR-2044** — session credentials fail closed and expire identically on every
  transport. `get_session` and `validate_session` now share one freshness rule
  (`session_is_fresh`, `nostr_service.rs:634`) enforcing `AUTH_TOKEN_EXPIRY`;
  `get_session` previously enforced no expiry at all, so a WS token outlived its
  REST equivalent indefinitely. `/ws/client-messages` now resolves its token
  through the session realm instead of checking non-emptiness. The `?token=`
  query path is deprecated for one release rather than removed. Partial:
  `mcp_relay_handler` and `multi_mcp_websocket_handler` are routed to the estate
  lead. Diagrams VC-03.2, VC-03.5, VC-05.3.
- **ADR-2070** (vc-knowledge) — the NIP-98 citations in "Request signing" had
  drifted by roughly 60 lines. Every `file:line` in that section was re-verified
  and corrected: `validate_nip98_token` `:270`→`:330`, kind check `:288`→`:348`,
  freshness window `:168`→`:169` with past/future arms `:302`/`:307`→`:362`/`:367`,
  tag match `:328`-`:349`→`:376`-`:389`, `urls_match` `:463`→`:524`, payload hash
  →`:132`/`:413`, signature verify `:365`→`:426`, replay claim `:374`→`:435`,
  `claim_event_id` `:199`→`:234`, `REPLAY_CACHE` `:187`→`:215`, and the constant
  named `REPLAY_CACHE_TTL_SECONDS` `:177` corrected to the real `REPLAY_CACHE_TTL`
  at `:178`.
- **ADR-2094** — `MANAGEMENT_API_KEY` has one validator again. `AgentMonitorActor::new` (`src/actors/agent_monitor_actor.rs`) read the variable itself and fell back to an empty key behind a `warn!` while `AppState` boot failed closed on the same variable through `validate_security_env_vars` (`src/app_state.rs:78`); the actor now calls that validator, and a missing, insecure-default or under-16-character key is a boot error. The only relaxation is the existing compile-gated `ALLOW_INSECURE_DEFAULTS` (debug/`dev-auth` builds), which *disables* the Management API client loudly rather than weakening the credential — no path yields an empty-string bearer token.
