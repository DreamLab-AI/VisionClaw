---
id: ADR-2026
title: Fail-closed security posture — flag absence widens nothing, release aborts on promoted dev-auth
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
verified_paths: [src/middleware/rbac_gate.rs, src/main.rs, src/services/role_store.rs]
owner: jjohare
review_trigger: any new security-relevant env flag, or a request to soften the release boot-abort to a warning
repo: visionclaw
domain: SECURITY-profiles
lineage: legacy ADR-011 (no log-and-allow, fail closed), ADR-142 (fail-closed flag defaults), T2-auth-gating V3 resolution
---

# ADR-2026 — Fail-closed security posture — flag absence widens nothing, release aborts on promoted dev-auth

## Context

Two failure modes leak access. First, a security-relevant env flag that defaults
open means a forgotten var silently widens the surface. Second, dev-auth bypass
knobs (`SETTINGS_AUTH_BYPASS`, `ALLOW_INSECURE_DEFAULTS`, `VISIONCLAW_DEV_MODE`,
`--allow-skip-auth`) are `#[cfg]`-stripped from release builds, so they cannot
grant bypass — but their presence at deploy time signals a dev config was
promoted to production and must not pass silently. Legacy ADR-011 forbade
log-and-allow; ADR-142 set the flag lattice; the T2-auth-gating V3 resolution
made env promotion a hard boot failure. This merges those two facets.

## Decision

The absence of any security-relevant flag grants nothing more. `RBAC_PUBLIC_READS`
and `RBAC_ALLOW_OWNERLESS` both `unwrap_or(false)`; an owner-less store aborts
boot with `PermissionDenied` unless `RBAC_ALLOW_OWNERLESS=1` is set explicitly;
every role-lookup `Err` resolves down to `Viewer`, never up. In non-debug builds without dev-auth,
`enforce_release_env_hygiene()` aborts at boot: `--allow-skip-auth` in argv exits
`1`; any of the three suspect env vars, or `NODE_ENV=development`+`DOCKER_ENV`
together, exits `2`. This forecloses defaulting any auth gate open and forecloses
booting a release binary that carries dev-config fingerprints.

## Consequences

- A misconfigured or empty environment is safe by construction; there is no
  "we forgot the flag, so it opened up" path.
- Operators who genuinely run single-operator/owner-less must opt in visibly
  (`RBAC_PUBLIC_READS=1`, `RBAC_ALLOW_OWNERLESS=1`) — friction is the point.
- A non-debug image without dev-auth that inherits both `NODE_ENV=development`
  and `DOCKER_ENV` will refuse to
  boot even though it is not actually vulnerable; the fix is to clean the env,
  not the binary. Exit codes `1`/`2` must be understood by orchestration.
- Role-lookup errors degrade to `Viewer`, which overlaps the RBAC record
  ADR-2010 (cross-ref) rather than being re-litigated here.

## Verification

Re-checked at `e0f8cd896`: `public_reads_enabled` `unwrap_or(false)` at
`src/middleware/rbac_gate.rs:121-128`; owner-less `PermissionDenied` return at
`src/main.rs:729-739` (with `RBAC_ALLOW_OWNERLESS` opt-out at 717-719); role
`Err(e) => UserRole::Viewer` at `src/services/role_store.rs:204-208`;
`enforce_release_env_hygiene` argv `exit(1)` and `SUSPECT_ENVS`/`NODE_ENV+DOCKER_ENV`
`exit(2)` at `src/main.rs:117-163`, gated `#[cfg(not(any(debug_assertions,
feature = "dev-auth")))]` with a no-op dev stub. All four constructs are live.

## Closeout extension — 2026-09-04

CP-01/04/08. Owner remains jjohare with authentication/release maintainers. The scoped default parsers, ownerless boot refusal and non-debug/no-dev-auth hygiene are present. Historical complete/live declarations are retained for those mechanisms, not universal production posture. The release hygiene function is a stub when dev-auth is compiled, role errors resolve to Viewer rather than denying all reads, and the unassigned-signer default remains Editor unless narrowed.

**Acceptance condition:** Define the effective profile across feature set, all bypass/report controls, role fallbacks, peer/proxy handling and public route exceptions. Test missing versus zero-valued variables, report-mode construction/date rollover/restart, ownerless/error paths and production artefact promotion. Bind configuration and binary identity to the same pre-listener acceptance receipt. Reopen on any security-relevant setting, build feature, route exception or fallback. See the [profile review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#profile-claims-and-effective-policy) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/security-profile-snapshot.json). The prior nine helper cases retain matching source hashes; no new live profile or network test ran.

## Acceptance progress — 2026-09-05

**Implemented.** The fail-closed posture gains a boot-time enforcement point.
`enforce_release_env_hygiene()` covered three suspect variables plus the argv
flag; `src/config/security_profile.rs` (see ADR-2038) extends that to the whole
effective profile and runs it **before the listener binds**.

Two gaps in the original hygiene check are closed: `DEV_AUTH_LOOPBACK` — the
ADR-2012 dev-token runtime opt-in — is now in the forbidden set, and
`RBAC_GATE_MODE=report` is now a rejection in a production artefact, which the
env-hygiene list did not cover at all. The presence-not-truthiness rule the ADR
already relied on is made explicit and tested: `SETTINGS_AUTH_BYPASS=0` is a
rejection, not a disabled feature.

**Tests.** `cargo test --lib --no-default-features security_profile` — 29
passed, 0 failed.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2012-2038-security-profile.txt`.

**Remains open.** The boot abort's exit code and message are asserted only in
the pure evaluator, not by running a binary; `assert_effective_profile_or_exit`
itself is not exercised end to end.

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

**Governed changes since `f326a3b11`:** `src/main.rs`, `src/middleware/rbac_gate.rs`
(+12/-11) and `src/services/role_store.rs` (+596/-38). None of them softens a
fail-closed default; the main.rs change *adds* the ADR-2038 boot assertion.

**All four constructs still live, line numbers refreshed:**

- `public_reads_enabled()` at `src/middleware/rbac_gate.rs:122-129`, still ending
  `.unwrap_or(false)` at `:128` (cited `:121-128` — off by one after the import
  block grew).
- Owner-less boot refusal at `src/main.rs:732-752`: `RBAC_ALLOW_OWNERLESS_ENV`
  read at `:732`, the explicit opt-in logged at `:739`, and the
  `PermissionDenied` abort at `:747-752` (cited `:729-739`; the block moved down
  and grew).
- `enforce_release_env_hygiene` at `src/main.rs:117-163` — **citation still
  exact**. Argv `--allow-skip-auth` → `exit(1)` at `:120-126`; `SUSPECT_ENVS`
  (`SETTINGS_AUTH_BYPASS`, `ALLOW_INSECURE_DEFAULTS`, `VISIONCLAW_DEV_MODE`) at
  `:129-133`; the `NODE_ENV=development` + `DOCKER_ENV` pair at `:141-147`;
  `exit(2)` at `:161`. Still gated `#[cfg(not(any(debug_assertions, feature =
  "dev-auth")))]` at `:117` with the no-op dev stub at `:167-169`.
- Role-lookup `Err` → `Viewer` has **moved**: it is now `effective_role`'s
  `Err(e) => UserRole::Viewer` at `src/services/role_store.rs:369-372`, not
  `:204-208` as cited above. Lines `:202-208` today hold `parse_default_role`'s
  *other* fail-closed path (an unrecognised `RBAC_DEFAULT_ROLE` value logs and
  resolves to `Viewer`) — so the invariant is now enforced in two places, and the
  original citation points at the newer of them.

**Strengthened, not weakened.** The posture now has a second enforcement point:
`src/config/security_profile.rs` (landed `ac3e12dd1`) runs
`assert_effective_profile_or_exit` at `src/main.rs:873`, before
`HttpServer::new` (`:893`) and `.bind()` (`:1174`), extending presence-not-
truthiness rejection to `DEV_AUTH_LOOPBACK` and `RBAC_GATE_MODE=report`, which
`SUSPECT_ENVS` never covered. See ADR-2038.

**Commands run:** `git diff --stat f326a3b11..HEAD -- src/middleware/rbac_gate.rs
src/main.rs src/services/role_store.rs`; `awk` dumps of `main.rs:115-170` and
`:856-896`, `role_store.rs:195-215` and `:355-395`; `grep -n -A10
'fn public_reads_enabled' src/middleware/rbac_gate.rs`; `cargo test --lib
--no-default-features security_profile` → **37 passed, 0 failed**.


## Source closeout verification — 2026-09-07

The pre-bind assertion now rejects every finding for non-debug builds, including release/dev-auth. main.rs additionally logs selected storage membership and supports opt-in writer checkpointing; neither bypasses the assertion. Actual default release artefact negative probes returned exit 2 for forbidden variables set to zero. The exact artefact receipt predates later storage implementation; no rebuilt deployment image is claimed.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.
