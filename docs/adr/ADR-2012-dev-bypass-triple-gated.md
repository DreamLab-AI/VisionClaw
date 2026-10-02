---
id: ADR-2012
title: Development token and RBAC report mode have distinct activation gates
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []                   # legacy ADR-011 dev-bypass clause distilled — not in this tree; see lineage
superseded_by: []
verified_commit: e7e6b61d82fac8232a88229d6edadb198c64ce29
verified_paths: [src/utils/auth.rs, src/middleware/rbac_gate.rs]
owner: jjohare
review_trigger: any change to the dev-auth feature gate, DEV_AUTH_LOOPBACK handling, or the report-mode ack check
repo: visionclaw
domain: IDENTITY-authority-chain
lineage: Distils legacy ADR-011's dev-bypass exception clause; a Codex HIGH finding fenced the literal token behind three compiled-out gates.
---

# ADR-2012 — Development token and RBAC report mode have distinct activation gates

## Context

Local development needs an auth shortcut, but ADR-011's dev-bypass clause was dangerous: a Codex
HIGH finding showed `Bearer dev-session-token` with an arbitrary `X-Nostr-Pubkey` previously
satisfied the whole lattice — including Admin — on any request. Separately, a stray
`RBAC_GATE_MODE=report` env var could silently downgrade all auth denials to logs in production.
Both are foot-guns that must be structurally unreachable in a release build.

## Decision

The `Bearer dev-session-token` bypass (with arbitrary `X-Nostr-Pubkey`) fires only under **all
three** of: a debug or dev-auth build, runtime `DEV_AUTH_LOOPBACK=1`, and a loopback peer
address. RBAC report-mode refuses to disable enforcement unless it is acknowledged — a debug
build, or `RBAC_REPORT_MODE_ACK` equal to today's UTC date — otherwise it logs the refusal and
stays in `Enforce`. The token is absent only when neither debug assertions nor dev-auth is enabled;
report mode can be deliberately activated in a non-debug build with its dated ack.

## Consequences

- Developers must set `DEV_AUTH_LOOPBACK=1` and hit the server from loopback for the shortcut;
  a remote peer or unset flag is rejected with a warning.
- Report-mode checks acknowledgement at construction; it does not automatically
  expire in an already-running middleware instance at UTC midnight.
- The bypass code still ships in release binaries but is fenced (feature + runtime + peer), so it
  is dead unless all gates are opened — complementary to the SECURITY release-env boot-abort (ADR-2026).
- Absorbs the SECURITY-profiles report-mode dated-ack facet — cross-ref SECURITY ADR-2027.

## Verification

Re-checked at `e0f8cd896`: `src/utils/auth.rs:82` `dev_bypass_permitted_for_addr` requires
`DEV_AUTH_LOOPBACK=1` (`:83`) and a loopback peer; the acceptance site `:130-144` gates on it and
rejects with a warning otherwise. `src/middleware/rbac_gate.rs:80` `GateMode::from_env` returns
`Report` only when `report_acknowledged()` (`:105-110`: debug build or `RBAC_REPORT_MODE_ACK` =
today's UTC date) holds, else logs the refusal and returns `Enforce` (`:96-101`).
Governing doc: `docs/IDENTITY-authority-chain.md`.

## Closeout extension — 2026-09-04

CP-01/04/08. Owner remains jjohare with authentication/release maintainers. Implementation is partial against the original release-unreachability and midnight-expiry claims. The literal token is gated by debug-or-dev-auth compilation, explicit runtime opt-in and the observed peer being loopback. Report mode is different: a non-debug build may enable it with a current-date acknowledgement and retain it for that middleware instance. The full dev-mode bypass in ADR-2039 is also separate and peer-agnostic.

**Acceptance condition:** Define the effective profile across feature set, all bypass/report controls, role fallbacks, peer/proxy handling and public route exceptions. Test missing versus zero-valued variables, report-mode construction/date rollover/restart, ownerless/error paths and production artefact promotion. Bind configuration and binary identity to the same pre-listener acceptance receipt. Reopen on any security-relevant setting, build feature, route exception or fallback. See the [profile review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#profile-claims-and-effective-policy) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/security-profile-snapshot.json). The prior nine helper cases retain matching source hashes; no new live profile or network test ran.

## Acceptance progress — 2026-09-05

**Implemented.** New module `src/config/security_profile.rs`, called from
`src/main.rs` immediately before `HttpServer::new`/`bind`.

The reproduced gap — the literal dev token is triple-gated, but **report mode is
not**: a non-debug build can disable RBAC enforcement with a current-date
acknowledgement and retain it for that middleware instance — is closed at boot.
`evaluate_effective_profile(env, build, today)` is a pure function over an
injected environment snapshot, the build identity and the UTC date, so the whole
acceptance matrix runs as unit tests with no global state.

Findings, each fatal in a production artefact (no `debug_assertions`, no
`dev-auth`) and reported-only in a development build:
`ForbiddenDevVariable` (presence, not truthiness — `SETTINGS_AUTH_BYPASS=0` is a
rejection, and `DEV_AUTH_LOOPBACK` is now in the set, which ADR-2026's
`SUSPECT_ENVS` did not cover), `DevelopmentNodeEnvInContainer`,
`AllowSkipAuthArgv`, `DevAuthFeatureInArtefact`, `ReportModeRequested`
(acknowledged or not) and `ProfileDrift` / `UnknownDeclaredProfile`.

**Tests.** `cargo test --lib --no-default-features security_profile` — 29
passed, 0 failed. The missing-versus-zero-valued matrix is explicit: `VAR=0`,
`VAR=""` and `VAR=1` all reject, a missing variable does not. Report mode is
covered at construction, across the date rollover (yesterday's acknowledgement
no longer acknowledges; a future date does not pre-authorise), and on restart
(the same environment evaluated on two dates gives two answers).

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2012-2038-security-profile.txt`.

**Remains open.** Binary identity is asserted from `cfg!` flags, not from an
image digest — binding configuration and artefact identity to one pre-listener
receipt still needs a build-side step. Peer/proxy handling and the full REST +
WebSocket paths are unexercised. No live profile or network test ran.

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

**Governed changes since `f326a3b11`:** `src/middleware/rbac_gate.rs` (+12/-11)
only; `src/utils/auth.rs` is unchanged. The change is this record's own
acceptance work landing: the dated-acknowledgement rule now has exactly one
implementation.

**Gate 1 — the dev token is still triple-gated.**
`dev_bypass_permitted_for_addr` at `src/utils/auth.rs:122` (cited `:82` above —
that citation predates the file's growth and is corrected here) requires
`DEV_AUTH_LOOPBACK` at `:123` **and** a loopback peer; `dev_bypass_permitted`
wraps it at `:139` using `req.peer_addr()`. The acceptance site is
`:188-206` (cited `:130-144`): `Bearer dev-session-token` at `:188` is accepted
only when `dev_bypass_permitted(req)` holds (`:189`), else it warns and falls
through (`:200-205`). The whole block is inside the `#[cfg(any(debug_assertions,
feature = "dev-auth"))]` fence, so it is compiled out of a production artefact.

**Gate 2 — report mode no longer has its own copy of the rule.**
`GateMode::from_env` at `src/middleware/rbac_gate.rs:83-103` (cited `:80`) now
calls `report_mode_requested(&EnvSnapshot::from_process())` at `:85` and returns
`Enforce` immediately when unset (`:86`); the unacknowledged path still logs the
refusal and returns `Enforce` at `:95-101`. `report_acknowledged` at `:111-118`
(cited `:105-110`) delegates to
`config::security_profile::report_mode_acknowledged(env, BuildIdentity::current(),
&today)`. The construction-time capture is unchanged — the consequence about
non-expiry at UTC midnight still stands — but reaching it in a production
artefact is now impossible: `assert_effective_profile_or_exit`
(`src/main.rs:876`) rejects `RBAC_GATE_MODE=report` before the listener binds.

**Status stays `partial`:** binary identity is still asserted from `cfg!` flags
rather than an image digest, and no live profile or network test has run.

**Commands run:** `git diff f326a3b11..HEAD -- src/utils/auth.rs
src/middleware/rbac_gate.rs`; `grep -n` over `auth.rs` for
`dev_bypass_permitted_for_addr|DEV_AUTH_LOOPBACK|dev-session-token`; `awk` dumps
of `auth.rs:185-210` and `rbac_gate.rs:75-124`; `grep -n
assert_effective_profile_or_exit src/main.rs`; `cargo test --lib
--no-default-features security_profile` → **37 passed, 0 failed** (1230 filtered
out).

## Re-verification — 2026-10-02 at e7e6b61d8 (headset NIP-98 behind the prod nginx)

**Governed changes:** `src/utils/auth.rs` gains `nip98_request_url` (`:153`), which is the URL reconstruction that `verify_access` previously did inline: `X-Forwarded-Proto`/`X-Forwarded-Host` first, then `connection_info`. Empty forwarded headers now fall through to `connection_info`. The settings extractor and the `/wss` and `/ws/speech` authenticate URLs share it, and `nip98_proxy_tests` (`:544`) were added. The change exists so that the headset's NIP-98 writes validate behind the prod nginx (owner decision 2026-10-02, Q3). **Decision unaffected.** The dev-session-token branch and `dev_bypass_permitted` are byte-identical. Their lines moved from `:185-210` to `:217-242`; `:82` and `:122` are unchanged. `verified_commit` moved to the landing commit. Source reading, plus the unit tests named in that commit.
