---
title: Security Profiles & Flag Matrix
doc_id: VC-SECURITY
version: 0.1.1
status: draft-for-ratification
verified_commit: 
sources:
  - src/middleware/rbac_gate.rs
  - src/services/role_store.rs
  - src/models/rbac.rs
  - src/handlers/socket_flow_handler/position_updates.rs
  - crates/visionclaw-domain/src/utils/visibility_filter.rs
  - src/utils/nip98.rs
  - src/handlers/socket_flow_handler/http_handler.rs
  - src/main.rs
  - docker-compose.unified.yml
  - .env
date: 2026-08-31
---

# Security Profiles & Flag Matrix

## Purpose

Single source of truth for every security-relevant runtime flag, the three
validated deployment profiles built from them, and the illegal combinations the
operator must never ship. Ground-truth order: live code > audit facts > legacy ADR prose.

## Current State

### Flag inventory (from code)

Every flag below is read directly from process env. The **code default** is what
the binary does with the var *absent*; the **compose default** is what
`docker-compose.unified.yml` injects. Where they differ, the compose value is the
shipped posture — that gap is called out in Known divergences.

| Flag | Purpose | Code default | Source (file:line) |
|------|---------|--------------|--------------------|
| `RBAC_PUBLIC_READS` | Allow anonymous safe-method (`GET`/`HEAD`/`OPTIONS`) reads across `/api` | **OFF** (fail-closed: absence requires auth) | `src/middleware/rbac_gate.rs:121-128` |
| `RBAC_ALLOW_OWNERLESS` | Permit boot with no Owner assigned in the role store | **OFF** (fail-closed: refuses to start, exit on no Owner) | `src/main.rs:717-739`, `src/services/role_store.rs:33` |
| `PUBKEY_VISIBILITY_FILTER` | ADR-060 drop-set: strip private nodes not owned by the session pubkey from the wire frame | **ON** (secure-by-default; only explicit `0`/`false`/`off`/`no` disables) | `src/handlers/socket_flow_handler/position_updates.rs:34-43` |
| `RBAC_GATE_MODE` | `report` downgrades `/api` denials to logs instead of 401/403 | **enforce** (`report` refuses to activate in release without dated `RBAC_REPORT_MODE_ACK`) | `src/middleware/rbac_gate.rs:80-113` |
| `SETTINGS_AUTH_BYPASS` | Legacy dev auth bypass | Codepath `#[cfg]`-stripped in release; **presence hard-fails boot** (exit 2) | `src/main.rs:117-155` |
| `VISIONCLAW_DEV_MODE` | **ADR-2039** LAN-local full auth bypass: in dev/`dev-auth` builds, `=1`/`true` grants dev-admin (`dev-mode-local-admin`) to **every** request — no NIP-98/token/peer check — across REST (`verify_access`), settings extractor, and WS handshake. Peer-agnostic by design (Docker SNAT hides origin). | **OFF** (dev builds); codepath `#[cfg]`-stripped in release + **presence hard-fails boot** (exit 2). Loud boot banner when armed. | `src/utils/auth.rs` (`dev_full_bypass_active`), `src/main.rs:120-133` |
| `ALLOW_INSECURE_DEFAULTS` / `--allow-skip-auth` | Dev auth relaxations | Same release refusal as above | `src/main.rs:120-133` |
| `APP_ENV` | Production signal; drives strict env validation & CORS lockdown | unset ⇒ non-production (permissive) | `src/main.rs:85`, `src/main.rs:887` |
| `RBAC_OWNER_PUBKEY` | Bootstrap the Owner role at startup | unset ⇒ no owner bootstrapped | `src/main.rs:709`, `src/services/role_store.rs` (`bootstrap_owner_from_env`) |
| `POWER_USER_PUBKEYS` | Legacy pubkeys mapped to `Admin` when unassigned | unset ⇒ no power users | `src/services/role_store.rs:194-203` |
| `VISIONCLAW_DEV_TOKEN` | Dev session token (`.env:197`) | dev builds only | `.env:197` |

`POD_DEFAULT_PRIVATE` and a distinct `NIP98_OPTIONAL_AUTH` env var are **not
present in this repo's code** at this commit — Pod-privacy defaults and NIP-98
optionality are governed structurally, not by these named vars (see divergences).

### Default-role & RBAC lattice

- Effective role precedence: explicit assignment → `Admin` if power-user → else
  the configured unassigned-signer default: **`RBAC_DEFAULT_ROLE`**
  (`editor`|`viewer`, parsed fail-closed in `src/services/role_store.rs`
  `parse_default_role`), defaulting to **`Editor`** for pre-RBAC compatibility.
  Under `viewer`, an *unassigned but authenticated* pubkey reads only until an
  Admin grants a role. Unrecognised values (including `admin`/`owner`) fail
  closed to `viewer` — the env var can narrow access, never widen it.
- Any error in role lookup **fails closed to `Viewer`**, never up
  (`src/services/role_store.rs:204-208`).
- `/api` gate policy (`rbac_gate.rs:133-169`): public allowlist
  (`/api/auth/*`, `/api/client-logs`, health probes); `/api/admin/*` needs `Admin`
  for every method; safe reads public iff `RBAC_PUBLIC_READS`; mutations need
  `WriteSettings` under `/api/settings`, else `WriteGraph` (denies Viewer writes).

### NIP-98 replay resistance (landing 2026-08-31)

Two-layer scheme (`src/utils/nip98.rs`): (1) a 60s freshness window
(`TOKEN_MAX_AGE_SECONDS`), and (2) a **single-use replay cache** — after
validation the event id is atomically claimed under a mutex
(`claim_event_id`) with a TTL of `2 × 60s`, closing the
previously-open replay-within-window gap. Concurrent presentations of the same id
cannot both win (single lock acquisition). The cache keys on a **monotonic
`Instant`** so a backward clock step cannot extend entry lifetimes, and enforces
a hard `REPLAY_CACHE_MAX_ENTRIES` ceiling (100 000): once full, new auth is
rejected fail-closed with `ReplayCacheFull` (mapped to 503) rather than evicting
a live id — evict-oldest would let a signature flooder purge a genuine id and
re-enable replay.

**Process-local scope:** the cache lives in one process's memory. Replay
protection does **not** span replicas; horizontal scaling requires shared
storage (e.g. Redis) or sticky routing so every presentation of a token lands on
the same process.

### Pubkey visibility filter (landing 2026-08-31)

Default flipped **ON** in the handler gate (`position_updates.rs:34-43`). The
pure drop-set logic (`visibility_filter.rs`) is fail-closed: a missing session
pubkey drops every private node, yielding a public-only graph. Public nodes are
unaffected, so default-on is behaviour-neutral for all-public deployments.

### Named validated profiles

Each profile is an **exact** flag set. Anything not listed takes its code default.

| Flag | `demo-open` | `single-tenant` | `multi-user-locked` |
|------|-------------|-----------------|---------------------|
| `RBAC_PUBLIC_READS` | `1` | `0` | `0` |
| `RBAC_ALLOW_OWNERLESS` | `1` | `1` | `0` |
| `RBAC_OWNER_PUBKEY` | unset | set (64-hex) | set (64-hex) |
| `RBAC_DEFAULT_ROLE` | `editor` | `editor` | `viewer` |
| `PUBKEY_VISIBILITY_FILTER` | `1` | `1` | `1` |
| `RBAC_GATE_MODE` | enforce | enforce | enforce |
| `APP_ENV` | `production` | `production` | `production` |
| `SETTINGS_AUTH_BYPASS` etc. | unset | unset | unset |

- **`demo-open`** — public read-only kiosk. Anonymous reads on, no owner required,
  visibility filter still on so only `public::true` nodes reach the wire. Writes
  still require an Editor+ session. This is the closest profile to today's shipped
  compose posture.
- **`single-tenant`** — one operator, private graph. Reads require auth; an Owner
  is bootstrapped; ownerless boot tolerated for the legacy `POWER_USER_PUBKEYS`
  path. The unassigned-⇒-Editor default is acceptable because the operator
  controls who can authenticate.
- **`multi-user-locked`** — hardened multi-tenant. Owner mandatory
  (`RBAC_ALLOW_OWNERLESS=0` ⇒ hard-fail without an Owner), no anonymous reads,
  visibility filter on, and `RBAC_DEFAULT_ROLE=viewer` so an unknown-but-valid
  NIP-98 signer is read-only until an Admin grants a role (least-privilege
  admission; closes the former open item where any valid signer was an Editor).

### Illegal combinations

| Combination | Why it is illegal | Enforcement |
|-------------|-------------------|-------------|
| `SETTINGS_AUTH_BYPASS` / `VISIONCLAW_DEV_MODE` / `ALLOW_INSECURE_DEFAULTS` set in a release build | Promotes dev auth relaxation to production | **Hard-fail at boot**, `src/main.rs:117-155` |
| `--allow-skip-auth` argv in release | Same | Hard-fail, `src/main.rs:120-126` |
| `NODE_ENV=development` + `DOCKER_ENV` both set | Dev config in a container image | Hard-fail, `src/main.rs:141-147` |
| `RBAC_GATE_MODE=report` in release without `RBAC_REPORT_MODE_ACK=<today UTC>` | Would silently disable `/api` auth | Refuses to activate, falls back to enforce, `rbac_gate.rs:88-102` |
| `RBAC_ALLOW_OWNERLESS=0` with no `RBAC_OWNER_PUBKEY` and no prior Owner | Permanent lockout / unmanageable RBAC | Refuses to start (`PermissionDenied`), `src/main.rs:729-739` |
| `RBAC_PUBLIC_READS=1` with `PUBKEY_VISIBILITY_FILTER=0` | Anonymous reads *and* private nodes on the wire = full data disclosure | **Machine-enforced — ADR-2043** (2026-09-05): an unconditional rule in `evaluate_effective_profile` (`src/config/security_profile.rs:422`) keyed on the pair itself, so it fires whether or not a profile is declared. Before ADR-2043 this pair was caught only *indirectly* — it matches no named profile, so it raised `ProfileDrift` when a profile was declared but merely classified as `Unnamed` when none was, and `Unnamed` is not itself fatal |
| `APP_ENV` unset in a public deployment | Skips strict env validation and CORS lockdown (`main.rs:85,887`) | Not enforced; profiles pin `APP_ENV=production` |

## Known divergences & open items

- **Shipped compose posture is open-by-default, and inverts two code defaults.**
  `docker-compose.unified.yml:93-94` sets `RBAC_PUBLIC_READS: "${RBAC_PUBLIC_READS:-1}"`
  and `RBAC_ALLOW_OWNERLESS: "${RBAC_ALLOW_OWNERLESS:-1}"` — both are code-default
  **OFF/fail-closed** (`rbac_gate.rs:122-128`, `public_reads_enabled()` ending
  `.unwrap_or(false)`; `main.rs:732-752`, which refuses to start without an Owner
  unless the flag is set). The image therefore boots owner-less with anonymous
  reads unless an operator overrides the `.env`. `PUBKEY_VISIBILITY_FILTER=1`
  (`docker-compose.unified.yml:107`) matches the code default
  (`position_updates.rs:34-58`, `parse_visibility_flag` defaults ON). Net shipped
  posture ≈ `demo-open`. This is a deliberate compatibility trade-off (matches
  legacy ADR-142 open-by-default) and **stays**: the profile it realises is now
  named and ratified by ADR-2027, so the earlier "needs a named profile"
  condition is met. What remains open is *machine* selection — nothing at boot
  asserts the running env matches a named profile (ADR-2038, `proposed`).
  The code default and the compose default are two distinct facts and are cited
  separately here per ADR-2087.
- **Unassigned authenticated pubkey ⇒ Editor** (`rbac.rs:68`,
  `role_store.rs:194-203`). Write-capable by default, which contradicts
  least-privilege — but this is now a *configuration* choice, not a missing
  capability. Resolved — ADR-2087 (2026-09-05): the selector exists as
  `RBAC_DEFAULT_ROLE` (`role_store.rs:41` `RBAC_DEFAULT_ROLE_ENV`, parsed by
  `parse_default_role` at `:195`, failing closed to `viewer` on an unrecognised
  value at `:204`). The `multi-user-locked` profile sets it to `viewer`; the
  shipped compose sets `editor` (`docker-compose.unified.yml:101`) for
  pre-RBAC compatibility. The earlier claim that no flag existed and a code
  change was required is retired.
- **`?token=` accepted on `/wss`** (`http_handler.rs:139-152`, the query-parameter
  lookup at `:148`) — contradicts legacy
  ADR-011 (header-only). Session token in the query string is a log-hygiene /
  referrer-leak risk (medium). Header path exists; query path should be removed or
  gated.
- **`SETTINGS_AUTH_BYPASS=false` and `APP_ENV=development` sit in `.env`**
  (`.env:36,39`). Harmless in a dev build; in a release image the mere presence of
  `SETTINGS_AUTH_BYPASS` hard-fails boot — the `.env` must be scrubbed for release.
- **SOPS never executed** (legacy ADR-109, Accepted 2026-05-09): `.env` is
  plaintext today, no SOPS artifacts in-tree. Secrets management is an open item.
- **agentbox AoE :9095 token auth** — staged for the next image rebuild (was
  `--auth none` on loopback with tokenless direct routes). Not yet in the running image.
- **Key custody / rotation frozen** (legacy ADR-081) and **delegated admin frozen**
  (legacy ADR-094), both 2026-07-03. NIP-26 delegation not wired — the Nostr bridge
  re-signs under the bridge key (fail-closed NIP-26 deferred to unbuilt Phase 5).
- **`POD_DEFAULT_PRIVATE` / `NIP98_OPTIONAL_AUTH` absent** as named env vars — the
  brief's expected flags do not exist at this commit; Pod default-privacy and NIP-98
  optionality are structural, not flag-driven. Recorded so a future reader does not
  hunt for a non-existent switch.

## Invariants (must not silently change)

1. Absence of any security flag must **widen nothing** — every flag fails closed
   except `PUBKEY_VISIBILITY_FILTER`, which fails *safe* (filter on) with the same effect.
2. `default_authenticated()` and the `Editor` mapping are load-bearing: changing
   the default role silently re-grants or revokes write access estate-wide.
3. Release builds must retain the boot-time refusal of dev-auth env/argv
   (`main.rs:117-155`) and the report-mode acknowledgement gate.
4. NIP-98 must keep both layers — window **and** single-use cache. Removing the
   cache re-opens replay-within-60s. The cache is **process-local** by design:
   replay protection does not span replicas, so any multi-replica deployment must
   add shared-state (Redis) or sticky routing before it can rely on this
   invariant. The hard capacity ceiling must fail closed (reject, `ReplayCacheFull`
   → 503), never evict a live id.
5. `RBAC_PUBLIC_READS=1` and `PUBKEY_VISIBILITY_FILTER=0` must never coexist in a
   deployed profile (full-disclosure combination). Enforced at boot by ADR-2043
   as an unconditional rule, independent of whether a profile is declared.
6. The security profile is asserted **before the listener binds**
   (`assert_effective_profile_or_exit`, `src/main.rs:873`, called from the block
   at `:868-878`, ahead of `HttpServer::new` at `:893` and `.bind()` at `:1177`).
   A production artefact with any finding exits 2 rather than serving a request
   (ADR-2038).

## Change process

Any new security-relevant env var, or any change to a default in this matrix,
requires: (1) updating this table with the new file:line; (2) confirming the
fail-closed invariant; (3) adding it to the illegal-combinations table if it can
interact badly; (4) bumping `version` and re-recording `verified_commit`. Compose
default changes require an accompanying named-profile update. Legacy ADR prose is
evidence, not authority — cite it, do not defer to it.

## Replay closeout qualification — 2026-09-04

ADR-2002's bounded process-local cache remains implemented in the inspected source. [Isolated helper evidence](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/runtime-ingress.md#visionclaw-replay-and-operation-boundaries) confirms capacity and TTL semantics without certifying full authentication. Acceptance still needs combined clock boundaries, restart/replicas, route-specific payload binding and fresh-token/idempotent retries after downstream failure. Single-use authentication does not itself establish exactly-once mutation.

## Visibility closeout qualification — 2026-09-04

ADR-2003's default-on parser and both inspected initial/position output filters are present. Six domain filter tests pass. [Output coverage and acceptance](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#visibility-defaults-and-output-coverage) still require metadata write authority, canonical owner identity, alternate output inventory and client-state behaviour after visibility/owner changes. A flag default is not proof of every private-data boundary.

## Development/release acceptance qualification — 2026-09-04

ADR-2037/2038 remain proposed controls, not established release-image/profile assertions. Nine [extracted-helper build cases](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#development-bypass-and-release-identity) confirm that non-debug plus dev-auth still allows the full bypass, whereas non-debug without it rejects any present VISIONCLAW_DEV_MODE value. ADR-2039's dependency wording is corrected. Shipped feature identity, pre-listener profile validation, report-mode interaction and actual REST/WS/headset behaviour require separate receipts.

## Profile dependency reconciliation — 2026-09-04

ADR-2012 is partial: report mode is reachable with acknowledgement in a non-debug build and does not expire automatically in an existing middleware instance. ADR-2026's hygiene evidence applies only without debug/dev-auth; ADR-2027 remains a partial four-setting profile model. [Effective-policy requirements](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#profile-claims-and-effective-policy) add build features, full bypass, report mode, public prefixes and power-user fallback to release acceptance. Prior helper source hashes still match; no live profile was certified.

## Remediation — 2026-09-05

- **ADR-2087** — Correct the deployment-profile documentation to match the code
  defaults. Retires the stale "no flag exists to select the default role" bullet
  (`RBAC_DEFAULT_ROLE_ENV` is `role_store.rs:41`, parsed at `:195`, fail-closed at
  `:204`); re-cites the `?token=` WS path from `http_handler.rs:342-354` to the
  actual `:139-152` (query lookup at `:148`); re-cites the open-by-default bullet
  to `docker-compose.unified.yml:93-94` / `rbac_gate.rs:122-128` /
  `main.rs:732-752`; and records that ADR-2027 satisfies the "needs a named,
  ratified profile" condition while machine selection remains ADR-2038's open
  item. Compose defaults are unchanged — demo-open is ADR-2027's decision.
- **ADR-2086** — Assert in CI that release images exclude the `dev-auth` cargo
  feature, implementing ADR-2037's decision as an actual gate in
  `.github/workflows/ci.yml`. `src/main.rs:169` remains the no-op hygiene stub
  under `#[cfg(any(debug_assertions, feature = "dev-auth"))]`, so the gate is what
  prevents such a binary reaching production.
- **ADR-2038 landed (vc-core, 2026-09-05)** — the profile matrix above is now
  **machine-asserted at boot**, not advisory prose.
  `src/config/security_profile.rs` resolves the profile from
  `VISIONCLAW_SECURITY_PROFILE` (`:55`) or classifies the observed flags when it
  is unset, and `assert_effective_profile_or_exit` (`:528`) exits 2 on any
  finding in a production artefact. Note the implementation tracks **six**
  flags (`PROFILE_FLAGS`, `:68`), not the four ADR-2027 originally named — it
  adds `RBAC_OWNER_PUBKEY` and `RBAC_GATE_MODE`, both of which the table above
  has always carried. ADR-2027 is corrected. The 2026-09-07 closeout now
  rejects both a missing declaration and an unsupported combination in every
  non-debug build, including a release build carrying `dev-auth`.

- **ADR-2043 (vc-core, 2026-09-05)** — makes the full-disclosure pair a
  first-class unconditional rule rather than an indirect profile mismatch. See
  the "Illegal combinations" row and Invariant 5.

## Required migration for 2026-09-07 profile admission

Before starting an updated release binary, explicitly set
`VISIONCLAW_SECURITY_PROFILE` to one of the three names in the table above and
set all six matching flags. Compose forwards this variable without choosing a
profile. The existing Compose defaults match `demo-open` only when no Owner key
is configured; selecting that public-read posture must be intentional. For a
private deployment, use the complete `single-tenant` or `multi-user-locked`
column, including an operator-controlled Owner key. Missing/blank intent and
unnamed combinations now fail before listener binding; they are not silently
converted into a hardened profile.

Release binaries built with `dev-auth` are refused even on an otherwise valid
profile. Development sessions requiring that bypass must use a debug build;
promoted release images must be rebuilt without `dev-auth`. Existing source
checks alone do not certify every published image's feature configuration.


### Signed browser upgrades and session migration (2026-09-07)

Opaque session issuance, lookup and refresh default to disabled. An operator can
explicitly set `VISIONCLAW_LEGACY_SESSIONS=1` (or `true`) during migration; this
only restores validation of issued, unexpired sessions. It does not enable
`dev-session-token`, unsigned identities or the query-token release bypass.
The flag has the same strict parsing in debug and release and is captured when
the service is constructed. Remove it after migrating remaining MCP/session clients.

The graph browser signs each HTTP GET upgrade URL through its active NIP-07 or
passkey signer. It offers `visionclaw` and `nostr.<base64url event>` subprotocols;
the server verifies the latter before upgrading and negotiates only `visionclaw`
(or the existing compatibility protocol), never the signed event. Signed events
include a fresh nonce so requests within one second remain distinct. Missing or
declined browser signers abort connection. The browser signs the public HTTP(S)
URL corresponding to its WS(S) URL, including its query. The edge must strip
untrusted forwarding headers and supply the public host/scheme consistently with
REST verification. Do not log the authentication subprotocol header.

Synthetic signing/replay tests do not prove a deployed proxy's forwarding policy
or reconnect flow. Remaining session-based MCP clients require migration or the
explicit compatibility setting; this change does not claim every transport has
completed sunset.

### Storage publication and recovery controls (2026-09-07)

Ontology pulls now stage a complete immutable generation and activate it through
an fsynced pointer replacement. The canonical index exposes `visionflow:generation`;
readers pin subsequent resources under `/public/ontology/@<generation>/...`.
The browser schema parser uses this pin for both JSON-LD and Turtle. Existing
canonical URLs remain valid but separate unpinned requests can straddle activation.
Pinned resources use the canonical resource's ACL. Private generation paths are
not accessible through the Solid handler. Old generations are retained; capacity
planning and a reader-safe retention policy remain required.

Setting `ONTOLOGY_BACKUP_DIR` enables an Oxigraph checkpoint from the open writer
at startup. Its destination must be outside the active data tree. A failed
configured checkpoint fails startup. This is opt-in and does not schedule backups
or certify an off-device recovery copy. The durable reconciliation journal records
explicit erase/restore operation IDs, selected store membership and per-store
receipts; destructive production adapters and subject selection remain outstanding.
