---
id: ADR-2011
title: One central RbacGate covers the whole /api scope; writes gate at WriteGraph, not mere Authenticated
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []                   # legacy ADR-011/ADR-142 distilled — not in this tree; see lineage
superseded_by: []
verified_commit: e7e6b61d82fac8232a88229d6edadb198c64ce29
verified_paths: [src/middleware/rbac_gate.rs, src/utils/auth.rs]
owner: jjohare
review_trigger: addition of an /api sub-scope with a distinct auth requirement, or any change to the public-prefix allowlist
repo: visionclaw
domain: IDENTITY-authority-chain
lineage: Distils legacy ADR-011 (enforce at scope config not handler) + ADR-142 ('15+ endpoints missing auth' gap).
---

# ADR-2011 — One central RbacGate covers the whole /api scope; writes gate at WriteGraph, not mere Authenticated

## Context

Per-handler auth checks left an audited gap of 15+ `/api` endpoints with no enforcement (ADR-142).
Prefix matching on raw string segments risks `/api/administrator` inheriting `/api/admin`'s
policy. Gating writes at `Authenticated` alone would let a `Viewer` mutate the graph, because an
authenticated Viewer is still authenticated. ADR-011 required enforcement at scope config, not
scattered across handlers.

## Decision

A single middleware (`RbacGate`) computes `required_level` per `(method, whole-segment path)` across
the entire `/api` scope. Matching is whole-`/`-segment, so `/api/administrator` does not inherit
`/api/admin`. The admin surface requires `Admin` for every method; mutations require `WriteGraph`
— satisfied by Editor(→Authenticated)/Admin but refused to a Viewer(→ReadOnly). This forecloses
per-handler auth as the enforcement point and any write path gated only at `Authenticated`.

## Consequences

- New `/api` routes are covered by default; forgetting a per-handler guard no longer opens a hole.
- A public route must be added to the whole-segment allowlist explicitly, or it is gated.
- The gate enforces the level; the role model it maps to lives in ADR-2010 — the two must stay
  consistent (WriteGraph ⇔ Editor+).
- Whole-segment matching means near-miss paths (`/api/admin-x`) are treated as distinct scopes,
  which is the intended behaviour, not a bug.

## Verification

Re-checked at `e0f8cd896`: `src/middleware/rbac_gate.rs:133` `required_level` uses
`has_segment_prefix` over `segments(path)` (`:60-66`); the public allowlist is whole-segment
(`:50`); `has_segment_prefix(&segs, &["api","admin"])` → `Admin` at `:146-147`; mutating methods
→ `Some(AccessLevel::WriteGraph)` at `:159-167`. `src/utils/auth.rs:41` `has_permission`
implements the lattice comparison that refuses WriteGraph to a ReadOnly (Viewer) level.
Governing doc: `docs/IDENTITY-authority-chain.md`.

## Closeout extension — 2026-09-04

CP-01/04/05/08. Owner remains jjohare with identity/runtime maintainers. The central gate is installed on /api, with explicit public-prefix exceptions and a report mode that forwards denials. The complete/live declaration remains scoped to that implementation, not unconditional enforcement. Release builds can activate report mode using the current-date acknowledgement; construction captures the mode without per-request expiry. The whoami handler inherits the stricter central admin-prefix requirement.

**Acceptance condition:** Verify caller/target authority under concurrent changes, removal versus effective revocation, immutable/recoverable audit outcomes and response-loss retries. Exercise the composed route policy, public-prefix methods, whoami, missing identity services and report-mode lifecycle. Bind accepted behaviour to the release profile and effective process settings. Reopen on role fallback, transaction, prefix, middleware ordering or report-mode changes. See the [authority review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/rbac-snapshot.json). This pass is source-only: no role mutation, HTTP request or race test ran.

## Acceptance progress — 2026-09-05

**Implemented.** The shared half of the ADR-2010/2011 acceptance.
`src/middleware/rbac_gate.rs` no longer carries its own copy of the report-mode
dated-acknowledgement rule: `GateMode::from_env` and `report_acknowledged` now
call `config::security_profile::{report_mode_requested, report_mode_acknowledged}`,
so the gate and the new boot-time assertion cannot drift apart. The
construction-time capture is unchanged, but reaching it in a production artefact
is now impossible: the ADR-2038 assertion rejects `RBAC_GATE_MODE=report` before
the listener binds, acknowledged or not.

The caller-authority and removal-versus-revocation work is recorded under
ADR-2010; the central gate is the surface that composes it.

**Tests.** `cargo test --lib --no-default-features rbac` — 13 passed, 0 failed
(existing gate tests, unchanged behaviour). `cargo test --lib --no-default-features
security_profile` — 29 passed, 0 failed, covering the shared acknowledgement
rule including the date rollover.

**Receipts.** `adr-2010-2011-rbac-caller-freshness.txt`,
`adr-2012-2038-security-profile.txt`.

**Remains open.** The composed route policy, public-prefix methods, whoami and
missing identity services are still not exercised through real routes.

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

**Governed changes since `f326a3b11`:** `src/middleware/rbac_gate.rs` only
(+12/-11). The whole delta is the ADR-2012 de-duplication — `GateMode::from_env`
and `report_acknowledged` now call `config::security_profile::{report_mode_requested,
report_mode_acknowledged}` instead of reading the env vars themselves. **No routing,
matching or level-mapping logic changed.** `src/utils/auth.rs` is unchanged.

**Every invariant re-confirmed, line numbers refreshed (the +3-line
`security_profile` import shifted the file down):**

- `required_level` at `:138` (was cited `:133`), still a pure function over
  `(method, path, public_reads)`.
- Whole-segment matching: `segments` at `:64-67`, `has_segment_prefix` at
  `:69-72` (was cited `:60-66`); `required_level` uses them at `:143-147`.
- Public allowlist `PUBLIC_SEGMENT_PREFIXES` at `:55-61` (was cited `:50`), still
  a list of whole-segment slices — `api/auth`, `api/client-logs`, `api/health`,
  `api/healthz`, `api/readyz`.
- Admin surface: `has_segment_prefix(&segs, &["api", "admin"])` → `Admin` at
  `:152-153` (was cited `:146-147`), ahead of the safe-method branch, so admin
  **reads** stay Admin even with `RBAC_PUBLIC_READS=1`.
- Mutations gate at `WriteGraph`, not `Authenticated`: `:169-173` (was cited
  `:159-167`), with the rationale comment at `:163-168`. `src/utils/auth.rs:41`
  `has_permission` still implements the lattice comparison, and the unit test at
  `rbac_gate.rs:377-378` still asserts `ReadOnly` fails `WriteGraph` while
  `Authenticated` passes it.
- `/api/administrator` still does not inherit `/api/admin` — test at `:331-335`.

**Commands run:** `git diff f326a3b11..HEAD -- src/middleware/rbac_gate.rs`
(full patch read); `awk` dumps of `rbac_gate.rs:40-76`, `:75-124`, `:134-176`;
`grep -n has_permission src/utils/auth.rs`; `cargo test --lib
--no-default-features rbac` → **14 passed, 0 failed** (1253 filtered out).

## Re-verification — 2026-10-02 at e7e6b61d8 (headset NIP-98 behind the prod nginx)

**Governed changes:** `src/utils/auth.rs` gains `nip98_request_url` (`:153`), which is the URL reconstruction that `verify_access` previously did inline: `X-Forwarded-Proto`/`X-Forwarded-Host` first, then `connection_info`. Empty forwarded headers now fall through to `connection_info`. The settings extractor and the `/wss` and `/ws/speech` authenticate URLs share it, and `nip98_proxy_tests` (`:544`) were added. The change exists so that the headset's NIP-98 writes validate behind the prod nginx (owner decision 2026-10-02, Q3). **Decision unaffected.** `RbacGate` still delegates to `verify_access`, and `required_level` is unchanged. The new helper sits on the NIP-98 branch only. `verified_commit` moved to the landing commit. Source reading, plus the unit tests named in that commit.
