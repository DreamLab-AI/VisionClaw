---
id: ADR-2010
title: Four-tier pubkey-bound RBAC lattice (Editor default, fail-closed to Viewer) with atomic role mutations
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []                   # legacy ADR-142/ADR-094 distilled — not in this tree; see lineage
superseded_by: []
verified_commit: c16b257741b980d4599122ab77aa2f5980f94f31
verified_paths: [src/models/rbac.rs, src/services/role_store.rs, src/utils/auth.rs]
owner: jjohare
review_trigger: adoption of a multi-user-locked deployment, or any change to default_authenticated() or the last-Owner guard
repo: visionclaw
domain: IDENTITY-authority-chain
lineage: Distils legacy ADR-142 (multi-user RBAC bound to NIP-98 pubkeys), ADR-131 §3 (branch-only enterprise_auth rejected), ADR-094 (admin-pubkey privilege intent); hardened per Codex round-2 (three checks folded into one transaction).
---

# ADR-2010 — Four-tier pubkey-bound RBAC lattice (Editor default, fail-closed to Viewer) with atomic role mutations

## Context

Authority must attach to the NIP-98 pubkey (ADR-142), not to a branch build flag (ADR-131 §3
rejected `enterprise_auth`). Pre-RBAC `main` granted every authenticated user read+write-graph;
demoting them silently on RBAC rollout would break existing users. Naive check-then-write role
mutations carry TOCTOU risk: two concurrent grants could bypass the `can_assign` privilege check
or the last-Owner guard, permitting escalation or sole-Owner lockout.

## Decision

Persisted per-pubkey roles form an `Owner(4) > Admin(3) > Editor(2) > Viewer(1)` lattice.
An authenticated-but-unassigned pubkey resolves to the **configured default role**:
`RBAC_DEFAULT_ROLE` = `editor` (default, preserving the pre-RBAC grant) or `viewer`
(least-privilege admission for multi-user-locked deployments); unrecognised values — including
`admin`/`owner` — parse fail-closed to `Viewer`, so the env var can narrow access but never
widen it. Any lookup error — including an unparseable stored role — **fails closed to
`Viewer`**, never up. `assign_checked`/`revoke_checked` fold the current-role read, the
`can_assign` privilege check, and the last-Owner guard into a single SQLite transaction, protecting target-role and last-Owner checks from interleaving; caller-role
freshness remains outside this transaction (see the dated closeout extension). This forecloses
role logic scattered across handlers and any code path that reads then writes a role non-atomically.

## Consequences

- Under the shipped `editor` default, new authenticated pubkeys can write the graph immediately;
  the multi-user-locked profile sets `RBAC_DEFAULT_ROLE=viewer` so unknown signers are read-only
  until granted a role (closes the former open item — SECURITY ADR-2027 profile table).
- A role-store outage narrows to Viewer; this does not deny every read operation.
- Role mutations run inside a transaction, so a busy store can contend; the guarantee is atomicity,
  not throughput.
- Also absorbs the SECURITY-profiles Editor-default facet — cross-ref SECURITY ADR-2027.

## Verification

Re-checked at `e0f8cd896`, default-role facet updated 2026-08-31 (see commit for this change):
`src/models/rbac.rs` `default_authenticated()` returns `Editor` with the rationale comment;
`src/services/role_store.rs` `parse_default_role` maps `RBAC_DEFAULT_ROLE` to
`editor`/`viewer` and fails closed to `Viewer` on any other value (unit-tested:
`default_role_parse_lattice`, `unassigned_default_is_store_configured_not_hardcoded`);
`effective_role` returns the constructed default on an empty row and `Viewer` on `Err`;
`assign_checked` (`:243`) enforces `can_assign` at `:266`/`:272` and `LastOwner` at `:287`;
`revoke_checked` (`:310`) enforces `can_assign` `:330` and `LastOwner` `:342`, all inside one
transaction folded through `TxOutcome`. `src/utils/auth.rs:60` `resolve_access_level` maps roles
to access levels. Governing doc: `docs/IDENTITY-authority-chain.md`.

## Closeout extension — 2026-09-04

CP-01/04/05/08. Owner remains jjohare with identity/runtime maintainers. The transaction reads the target role and Owner count, but consumes a caller role resolved earlier by the handler. Caller demotion is not rechecked in the transaction. Implementation is partial against the broad TOCTOU-free authority claim. Removing an assignment restores default or legacy power-user authority; it does not necessarily revoke access. Lookup failure narrows to Viewer rather than denying every read.

**Acceptance condition:** Verify caller/target authority under concurrent changes, removal versus effective revocation, immutable/recoverable audit outcomes and response-loss retries. Exercise the composed route policy, public-prefix methods, whoami, missing identity services and report-mode lifecycle. Bind accepted behaviour to the release profile and effective process settings. Reopen on role fallback, transaction, prefix, middleware ordering or report-mode changes. See the [authority review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/rbac-snapshot.json). This pass is source-only: no role mutation, HTTP request or race test ran.

## Acceptance progress — 2026-09-05

**Implemented.** `src/services/role_store.rs`, `src/handlers/admin_rbac_handler.rs`.

*Caller-role freshness inside the mutation transaction.* The reproduced defect —
the transaction read the target role and Owner count but consumed a caller role
the handler had resolved earlier — is closed. `CallerAuthority { pubkey,
is_power_user, admission_role }` replaces the bare `UserRole` parameter;
`resolve_caller_role_in_tx` re-resolves the caller's effective role **inside**
the same transaction using the same precedence as `effective_role` (explicit
row, then Admin for a legacy power user, then the configured default, with an
unparseable row failing closed to Viewer). `effective_mutation_authority` then
applies the rule: a demotion aborts with `RoleStoreError::CallerAuthorityChanged`
(HTTP 409, distinct from a plain 403 so the client knows re-authentication
helps); a promotion does **not** escalate — the mutation stays bound by the
admission role, so winning a race cannot grant a privilege the request was not
admitted for.

*Removal versus revocation.* `revoke_checked` is replaced by `remove_checked`,
returning `RemovalOutcome { had_explicit_role, previous_role, effective_after,
authority_reduced }` plus `revocation_requires_explicit_viewer()`. Removing a
Viewer row *raises* authority to the unassigned default; removing a power user's
row restores Admin. The handler now reports `authority_reduced` /
`access_revoked` and warns explicitly when removal did not revoke anything,
naming the explicit-Viewer assignment as the real revocation. `RoleStore::new_with_default`
lets the configured default be set programmatically so both postures are testable.

**Tests.** `cargo test --lib --no-default-features role_store` — 29 passed,
0 failed (13 new): concurrent caller demotion on assign and on remove, caller
row deleted mid-request, corrupt caller row failing closed, promotion not
escalating, unchanged authority proceeding, demotion reported distinctly from
denial, and six removal-semantics cases including the Viewer, Admin, power-user,
no-op and Viewer-default variants.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2010-2011-rbac-caller-freshness.txt`.

**Remains open.** No HTTP request or real race ran — the concurrency is
simulated by mutating the store between admission and the transaction, which
exercises the same code path but is not a live race. Immutable/recoverable audit
outcomes, response-loss retries, whoami, missing identity services and
report-mode lifecycle through real routes remain. Binding accepted behaviour to
the release profile is partly addressed by the ADR-2038 boot assertion.

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

**Governed changes since `f326a3b11`:** `src/services/role_store.rs` only
(+596/-38 — the caller-freshness and removal-semantics work recorded above).
`src/models/rbac.rs` and `src/utils/auth.rs` are unchanged.

**Lattice and defaults intact.** `Owner(4) > Admin(3) > Editor(2) > Viewer(1)` at
`src/models/rbac.rs:52-55`; `default_authenticated()` still returns `Editor` with
its rationale comment at `:62-69`. `parse_default_role` (`role_store.rs:195-211`)
still maps only `editor`/`viewer` and logs-then-fails-closed to `Viewer` on any
other value at `:202-208`; unset and empty both take the `Editor` default
(`:197-198`). `effective_role` (`:359-374`) keeps the precedence explicit
row → Admin-for-power-user → configured default, with `Err(e) => UserRole::Viewer`
at `:369-372` — the fail-closed-never-up rule.
`src/utils/auth.rs:60` `resolve_access_level` is unmoved.

**Atomicity holds; the API changed shape.** `assign_checked` is now at `:408`
(was `:243`), enforcing `can_assign` at `:444` and `:450` and the last-Owner guard
at `:465`, all inside one transaction folded through `TxOutcome` (`:775-798`).
`revoke_checked` **no longer exists** — the citation `(:310)`/`:330`/`:342` in the
Verification block above is historical and describes a removed function. Its
replacement is `remove_checked` at `:500-577`, enforcing `can_assign` at `:538`
and last-Owner at `:550`, returning `RemovalOutcome` (`:129-160`) so a caller can
tell removal from revocation. Caller freshness is now *inside* the transaction:
`CallerAuthority` (`:95`), `resolve_caller_role_in_tx` (`:717`) and
`effective_mutation_authority` (`:750`), with a mid-request demotion surfacing as
`RoleStoreError::CallerAuthorityChanged` (`:81`, `:798`).

**Status unchanged at `partial`, and correctly so.** The 2026-09-04 acceptance
condition is only part-discharged: no HTTP request or real race has run, and
audit/idempotency/whoami/report-mode lifecycle remain unexercised.

**Commands run:** `git diff --stat f326a3b11..HEAD -- src/models/rbac.rs
src/services/role_store.rs src/utils/auth.rs`; `grep -n` over `role_store.rs` for
`assign_checked|remove_checked|revoke_checked|CallerAuthority|LastOwner|can_assign|
parse_default_role|effective_role`; `awk` dumps of `role_store.rs:195-215` and
`:355-395`; `cargo test --lib --no-default-features role_store` → **29 passed,
0 failed** (1238 filtered out).

## Re-verification — 2026-10-02 at e7e6b61d8 (headset NIP-98 behind the prod nginx)

**Governed changes:** `src/utils/auth.rs` gains `nip98_request_url` (`:153`), which is the URL reconstruction that `verify_access` previously did inline: `X-Forwarded-Proto`/`X-Forwarded-Host` first, then `connection_info`. Empty forwarded headers now fall through to `connection_info`. The settings extractor and the `/wss` and `/ws/speech` authenticate URLs share it, and `nip98_proxy_tests` (`:544`) were added. The change exists so that the headset's NIP-98 writes validate behind the prod nginx (owner decision 2026-10-02, Q3). **Decision unaffected.** The lattice (`AccessLevel::has_permission`, `:41`) and role resolution (`resolve_access_level`, `:60`) are untouched. `verified_commit` moved to the landing commit. Source reading, plus the unit tests named in that commit.

## Re-verification — 2026-10-07 at ed5644d03 (live memory cloud, ADR-2133)

**Governed change:** `src/utils/auth.rs` adds `effective_access_level(pubkey, nostr)` after `resolve_access_level` (+17 lines after line 75; later citations such as `dev_bypass_permitted_for_addr`, cited at `:122`, now sit at `:139`). It resolves the role of a pubkey the gate has *already* verified, using the same `resolve_access_level` path as `verify_access`, so a handler can demand a higher level without re-verifying a single-use NIP-98 token. It maps the dev-mode sentinel to `Admin` only when `dev_full_bypass_active()` is true, which is compile-gated to dev builds. **Decision unaffected.** The lattice, the `RBAC_DEFAULT_ROLE` default and fail-closed-to-Viewer all apply unchanged, because resolution goes through the same `resolve_access_level`. `verified_commit` moved to `ed5644d03`. Source reading of the diff (`git diff 20499efc6..ed5644d03` on the governed paths) plus `cargo check --lib --bins` and `cargo test --lib -- auth rbac memory_cloud` (62 + 5 pass) at the landing commit.

## Re-verification — 2026-10-07 at c16b25774 (NIP-98 single verification per request)

**Governed change:** `verify_access` (`src/utils/auth.rs:191`) no longer verifies a NIP-98 token a second time in one request. When the request carries `Authorization: Nostr …` and an outer layer (`RbacGate`, an enclosing `RequireAuth`) already left an `AuthenticatedUser` in the request extensions, it reuses that identity and checks only the required level via `effective_access_level`; extensions are server-side and cannot be populated from headers. Before this, every `RequireAuth` scope under `/api` answered NIP-98 callers 401 "Token replayed" (proved by `tests/rbac_gate_require_auth_stacking_test.rs`, now green). The lattice is untouched: the reused identity is held to the same `has_permission` check at the inner level, and a lower role still gets 403. Line citations into `src/utils/auth.rs` after line 265 shift by +25 (the NIP-98 branch of `verify_access` gains the reuse block; e.g. `nip98_request_url(req)` in that branch moves from `:266` to `:291`); earlier lines are unchanged. The decision holds.
