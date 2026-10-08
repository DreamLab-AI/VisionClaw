---
id: ADR-2038
title: "Boot-time deployment-profile assertion with illegal-combination abort"
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: bb76e75bdf2c2f86cf0ce2c13ea4296f42924582
verified_paths: [src/config/security_profile.rs, src/main.rs]
owner: jjohare
review_trigger: adoption of a production deployment, or any change to the profile env vars (RBAC_PUBLIC_READS, PUBKEY_VISIBILITY_FILTER, RBAC_DEFAULT_ROLE)
repo: visionclaw
domain: SECURITY-profiles
lineage: ADR-2027 (three named profiles, documented-prose gap), ADR-2003 (visibility-filter illegal combination), ADR-2010 (editor-default first-contact write), ADR-2026 (fail-closed posture the assertion enforces)
---

# ADR-2038 — Boot-time deployment-profile assertion with illegal-combination abort

## Context

ADR-2027 ratified three profiles but admits they are "documented prose, **not**
machine-selected — nothing at boot asserts the running env matches a named
profile," so flag drift is possible. The shipped `docker-compose.unified.yml`
realises demo-open (`RBAC_PUBLIC_READS:-1`). ADR-2003 records
`RBAC_PUBLIC_READS=1` + `PUBKEY_VISIBILITY_FILTER=0` as an illegal
full-disclosure combination with no runtime rejection. ADR-2010's shipped editor
default grants first-contact write. The `RBAC_DEFAULT_ROLE=viewer` lever already
landed (commit 8e78a9d19); the missing piece is the boot assertion that binds
these flags to a named, validated posture — not another lever.

## Decision

At boot the backend MUST resolve the active security profile from env — exactly
one of **demo-open**, **single-tenant**, **multi-user-locked** — log the resolved
profile name and its effective flag set, and ABORT with a non-zero exit when the
env is not a supported posture. Abort is mandatory in two cases: (1) the recorded
illegal full-disclosure combination `RBAC_PUBLIC_READS=1` +
`PUBKEY_VISIBILITY_FILTER=0` (ADR-2003), which no profile permits; and (2) any
env whose flags do not match the named invariants of the resolved profile in
`docs/SECURITY-profiles.md`. A production selector (any deployment not explicitly
opting into demo-open or single-tenant) MUST default to the **multi-user-locked**
profile. The assertion runs before the server binds a listener, so an
unsupported or leaky configuration never serves a request. Cross-refs: ADR-2003
(illegal combination), ADR-2010 (editor default the locked profile overrides),
ADR-2026 (fail-closed defaults the assertion enforces), ADR-2027 (the profiles
this asserts).

## Consequences

- An unsupported or leaky configuration fails fast at boot instead of running and
  silently serving over-disclosed data; the illegal full-disclosure combination
  becomes unbootable rather than merely undocumented.
- Operators gain a logged, machine-asserted statement of which profile is live —
  provable in incident review, closing the ADR-2027 drift gap.
- The shipped demo-open compose must now declare its profile explicitly; a bare
  or ambiguous production env resolves to multi-user-locked and will abort if its
  flags contradict that posture, so demo deployments must opt in on purpose.
- Follow-on work: a profile resolver + validator module, a boot-sequence hook
  ahead of listener bind, and a table of per-profile invariant checks kept in
  lockstep with `docs/SECURITY-profiles.md`.

## Verification

None yet — this record is `proposed` / `implementation_status: none` /
`activation_status: inactive`. No boot-time profile assertion exists (grep for a
resolver/abort path returns nothing). On implementation, verification MUST record:
the resolver source path and line, an abort test proving
`RBAC_PUBLIC_READS=1` + `PUBKEY_VISIBILITY_FILTER=0` yields a non-zero exit before
listener bind, and a production-default test proving an unspecified env resolves
to multi-user-locked. Set `verified_commit` and `verified_paths` at that point.

## Closeout extension — 2026-09-04

CP-01/04/06/08. Owner remains jjohare with release/authentication maintainers. Proposed/none/inactive is retained for the complete profile assertion. Existing development-variable hygiene and individual role/filter parsers do not jointly assert a named deployment profile. Extend the acceptance matrix to include RBAC report mode, whose construction-time acknowledgement can enable non-enforcement.

**Acceptance condition:** Bind image digest, source, feature closure and effective profile to a receipt. Test production rejection before listener bind, including forbidden variables set to zero, and prevent promotion of a dev-auth artefact. Exercise full REST and WebSocket paths, report-mode interaction, network reachability and sentinel attribution separately from helper parsing. Preserve the distinction between the peer-agnostic full bypass and loopback dev-token mechanism. Reopen on build features, boot sequencing, bypass branches or profile policy. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#development-bypass-and-release-identity), [reproducer](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-auth-probe.py) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-auth-probe.json). No full image, listener, HTTP or headset execution ran.

### Implementation status update — 2026-09-05 (ADR-2038)

`implementation_status` moves `none` → **complete** and `activation_status`
`inactive` → **staged**. `decision_status` deliberately stays `proposed`: the
implementation exists but has never landed in a commit, and promoting the
decision on the strength of untracked code would make this record assert
something a clean checkout cannot reproduce.

**The implementation.** `src/config/security_profile.rs` (1038 lines) is a pure
evaluator over `(EnvSnapshot, BuildIdentity, today)`:
`assert_effective_profile_or_exit` (`:528`) is called from `src/main.rs:873`,
inside the block at `:868-878`, **before** `HttpServer::new` (`:893`) and `.bind()`
(`:1146`). A production artefact with any finding prints each one and
`std::process::exit(2)`; a development build logs and continues
(`may_bind_listener`, `:358`). The boot receipt — `summary()` plus
`observed_flags` — is logged at `src/main.rs:879-883`.

`DeploymentProfile` (`:79`) implements ADR-2027's three profiles with
`expected_flags()` (`:120-136`) reproducing the table in
`docs/SECURITY-profiles.md` exactly, and `VISIONCLAW_SECURITY_PROFILE` (`:55`) is
the selector. Findings cover the forbidden dev variables (`FORBIDDEN_DEV_VARS`,
`:60-66` — a superset of ADR-2026's `SUSPECT_ENVS`, adding `DEV_AUTH_LOOPBACK`),
the `NODE_ENV=development` + `DOCKER_ENV` fingerprint, `--allow-skip-auth` in
argv, a `dev-auth` artefact (ADR-2037), report mode (ADR-2012), declared-profile
drift (ADR-2027), an unknown declared profile, and — added by **ADR-2043** — the
ADR-2003 full-disclosure flag pair.

**Why `staged` and not `live`.** Both the module and its call site are
**uncommitted**: `git status` reports `src/config/security_profile.rs` as
untracked (`??`), and `git show HEAD:src/main.rs | grep -c
assert_effective_profile_or_exit` returns `0`. A clean checkout of
`b00c28a0d766c8cf46cd00b100dab60ef2dd74a4` therefore contains neither, and
re-verifying this record from that SHA will not find the assertion. `verified_commit`
is set to that SHA per the Phase 2 convention that verification ran on the
uncommitted working tree above it; **both `verified_commit` and `verified_paths`
must be restored at the landing commit**, at which point `decision_status` can
move to `accepted` and `activation_status` to `live`.

**Not implemented.** The Decision text says a production selector defaults to
`multi-user-locked`. The code implements no implicit profile: an undeclared
deployment whose flags match no ratified profile is classified `Unnamed` and
still binds. ADR-2043 closes the highest-risk case (the full-disclosure pair)
unconditionally, but the general gap stays open and is recorded in
`docs/SECURITY-profiles.md` and the estate lead's ES-10.7.

**Provenance.** The implementation predates this remediation pass — the file's
mtime is 2026-09-05 13:10 and it has never been committed — and its author is not
established. It was found during Phase 1 diagram work (VC-09.4, VC-09.5, VC-09.6)
rather than written for it.

## Acceptance progress — 2026-09-05

**Implemented.** This record's subject now exists:
`src/config/security_profile.rs`, called from `src/main.rs` before
`HttpServer::new`/`bind`.

The gap the closeout named — "existing development-variable hygiene and
individual role/filter parsers do not jointly assert a named deployment profile"
— is closed by a single pure function,
`evaluate_effective_profile(&EnvSnapshot, BuildIdentity, today) ->
EffectiveProfile`, which joins: forbidden development variables (by presence,
including `DEV_AUTH_LOOPBACK`), the `NODE_ENV=development` + `DOCKER_ENV`
container fingerprint, `--allow-skip-auth` in argv, the dev-auth artefact
identity, **RBAC report mode** (the extension this record asked for — its
construction-time acknowledgement can otherwise enable non-enforcement), and
drift from the declared ADR-2027 profile. `EffectiveProfile` carries the
observed value of every profile flag as the boot receipt, and
`may_bind_listener()` is the decision: a production artefact must have no
findings; a development build always may.

Because the function takes an injected environment snapshot and date rather than
reading the process environment and clock, the entire matrix runs as ordinary
unit tests with no global state and no ordering hazard.

**Tests.** `cargo test --lib --no-default-features security_profile` — 29
passed, 0 failed. Missing versus zero-valued variables, report-mode construction
and date rollover, restart re-evaluation, artefact identity, argv, profile
drift, classification, and the binding decision are each covered.
The gate shares this module's acknowledgement rule (ADR-2011), so the two cannot
drift: `cargo test --lib --no-default-features rbac` — 13 passed, 0 failed.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2012-2038-security-profile.txt`.

**Remains open.** Image digest and feature closure are not bound to the receipt.
`assert_effective_profile_or_exit` is not exercised end to end (it calls
`process::exit`); only the pure evaluator it wraps is tested. Status left
`none`/`inactive`: the assertion exists but no deployment has run it.

## Verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8

**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; no production logic changed (verified by
whitespace-normalised comparison of every changed file — only rustfmt artefacts
remain). Line numbers below are re-derived positions over unchanged code.

**The reason this record was held at `proposed` no longer holds.** That posture
was chosen for one stated reason, quoted from the section above: the module and
its call site were *uncommitted*, so "a clean checkout … contains neither, and
re-verifying this record from that SHA will not find the assertion." That is
false at this commit:

- `git ls-tree HEAD src/config/security_profile.rs` returns the blob, and
  `git log --oneline -1 -- src/config/security_profile.rs` → **`ac3e12dd1`**
  (*feat(security): explicit runtime security profile + RBAC gate updates*).
- `git show HEAD:src/main.rs | grep -c assert_effective_profile_or_exit` → **2**
  (the import and the call), where the earlier check returned `0`.

`decision_status` therefore moves `proposed` → **`accepted`**, and
`verified_commit`/`verified_paths` are restored exactly as that section required.

**Citations re-derived at this commit** (the file has grown to 1263 lines since
the 1038 recorded above, and every line number in the previous section has
moved):

| Symbol | Cited above | Current |
|---|---|---|
| `assert_effective_profile_or_exit` | `:528` | **`security_profile.rs:612`** |
| call site in `main.rs` | `:873` | **`main.rs:876`** (block `:871-881`, import `:873`) |
| `HttpServer::new` | `:893` | **`main.rs:896`** |
| `.bind()` | `:1146` | **`main.rs:1177`** |
| boot receipt logged | `:879-883` | **`main.rs:882-886`** |
| `DeploymentProfile` | `:79` | **`:84`** |
| `expected_flags()` | `:120-136` | **`:125`** |
| `SECURITY_PROFILE_ENV` | `:55` | **`:55`** (unchanged) |
| `FORBIDDEN_DEV_VARS` | `:60-66` | **`:60`** |
| `may_bind_listener` | `:358` | **`:383-385`** |
| `evaluate_effective_profile` | — | **`:480`** |

The assertion runs **before the listener binds**, unconditionally, in a committed
build: `:876` precedes `HttpServer::new` (`:896`) and `.bind()` (`:1177`).
`activation_status` moves `staged` → **`live`** on that basis.

**`implementation_status` moves `complete` → `partial`, which is a downgrade, and
deliberate.** The Decision states: *"A production selector (any deployment not
explicitly opting into demo-open or single-tenant) MUST default to the
**multi-user-locked** profile."* That is **not implemented**, and the code says so
plainly at HEAD: `EffectiveProfile::classified` is an `Option<DeploymentProfile>`
rendered `<unnamed>` in the boot receipt (`:388-395`) when no ratified profile
matches, and an unnamed classification **raises no finding**. Since
`may_bind_listener()` is `!is_production_artefact() || findings.is_empty()`
(`:383-385`), a production deployment that declares nothing and lands on an
unnamed flag combination still binds. `grep -n "unwrap_or(DeploymentProfile"` →
no hit: there is no implicit profile anywhere.

The highest-risk case *is* closed, but by a different rule and a different
record: ADR-2043's unconditional full-disclosure pair check at `:544-552`, whose
comment at `:531-536` names exactly this asymmetry — "a deployment that DECLARED
one got `ProfileDrift` while a deployment that declared nothing was merely
classified `Unnamed` and bound anyway. That is backwards relative to the risk."
The general default remains open.

This also reconciles an internal contradiction: the "Acceptance progress" section
ends *"Status left `none`/`inactive`"* while the frontmatter read
`complete`/`staged`. The frontmatter is now `partial`/`live`/`accepted` and is
authoritative; the earlier prose is superseded by this section.

**Tests at this commit.** `cargo test --lib --no-default-features
security_profile` → **37 passed, 0 failed** (1230 filtered out) — the sections
above record 29; the suite has grown since. The shared acknowledgement rule with
ADR-2011's gate still holds: `cargo test --lib --no-default-features rbac` →
**14 passed, 0 failed** (recorded as 13 above).

**Still open, unchanged.** Image digest and feature closure are not bound to the
boot receipt (`BuildIdentity` is derived from `cfg!` flags, not an artefact
digest). `assert_effective_profile_or_exit` is still not exercised end to end
because it calls `process::exit`; only the pure evaluator it wraps is tested. No
deployment has run the assertion, and the shipped `docker-compose.unified.yml`
still declares no `VISIONCLAW_SECURITY_PROFILE` (ADR-2027).


## Source closeout verification — 2026-09-07

Missing intent, unnamed effective flags and release/dev-auth findings now refuse listener binding. Debug remains report-only. The default release artefact passes two pre-start exit-2 probes; its hash is in the execution evidence. Partial status remains because the original Decision requires implicit multi-user-locked selection whereas implemented migration requires explicit intent; this divergence requires owner ratification, and no deployment image/reconnect acceptance is claimed.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.

## 2026-09-07 development launcher compatibility

The host restart loop confirmed that a release artefact containing `dev-auth`
is rejected before listener binding. ADR-2008 now uses an optimised
`dev-runtime` profile with debug assertions enabled for local development.
The release rejection, environment checks and production posture are unchanged.
Host runtime acceptance remains pending the operator's rebuild/restart.

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed change since `1ad881cab`:** `src/main.rs` only, +11/-5 — the
panic-hook payload downcast. `src/config/security_profile.rs` is unchanged.

**Decision unaffected.** The assertion still runs before the listener binds:
`assert_effective_profile_or_exit(...)` is at `src/main.rs:918`, inside the
ADR-2038 block opened at `:904`, and `.bind(&bind_address)?` is at `:1219`.

**Citation note.** This record's `main.rs` citations predate several intervening
commits as well as the six-line shift from the panic-hook edit; the anchors
above are re-derived at HEAD and supersede the `873 / 876 / 879-883 / 882-886 /
896 / 1177` line numbers in the older sections. `verified_commit` moved to the
CI-repair commit.

## Re-verification — 2026-09-22 at a32abac57f3a7cfe66ab68ea1b0faca013c0d6b2

**Governed changes since `997440cd0`:** `src/main.rs` changed only at the sync-service construction (ADR-2114 `CorpusSource`).

**Decision unaffected.** The boot-time profile assertion and its illegal-combination abort are not on the changed lines. `verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-10-03 at 780eb3edb788c9cb568d5756689a3c8455db79b7

`780eb3edb` (W8v, VisionClaw's own K_broker) inserts eight lines at `src/main.rs:195-202`: `visionclaw-server mint-nostr-key --out <path>` is dispatched first and exits. That path loads no `.env`, reads no environment, binds no listener, and rejects every argument other than `--out`/`--help` with exit 2, so `--allow-skip-auth` cannot ride along. Every path that serves still runs `enforce_release_env_hygiene()` (now `:209`) and `assert_effective_profile_or_exit` (now `:931`) before `HttpServer::new` (`:951`) and `.bind()` (`:1232`). Every `main.rs` citation after `:195` in this record moves down by eight lines. The assertion is still unconditional on every path that binds. The mint path never reaches a bind, so it needs no profile. `security_profile.rs` is unchanged. The decision holds.

## Re-verification — 2026-10-07 at ed5644d03 (live memory cloud, ADR-2133)

**Governed change:** `src/main.rs` builds one shared `MemoryCloudService` before `HttpServer::new` (+7 lines after line 884), adds it as app data (+1 near line 1067) and registers `configure_memory_cloud_routes` under the `/api` scope (+4 after line 1191); later line citations shift by up to +12. No boot assertion, env-hygiene check, profile evaluation, feature gate or bind call changes. **Decision unaffected.** The boot-time profile assertion and its position before the listener binds are unchanged. `verified_commit` moved to `ed5644d03`. Source reading of the diff (`git diff 20499efc6..ed5644d03` on the governed paths) plus `cargo check --lib --bins` and `cargo test --lib -- auth rbac memory_cloud` (62 + 5 pass) at the landing commit.

## Re-verification — 2026-10-07 at b43a2a1e6 (memory-cloud security review)

**Governed change:** `src/main.rs` gains a shared `memory_cloud_query_limit` (+4 lines after line 890) and passes it to `configure_memory_cloud_routes(..)` (+2 lines near line 1206); later citations shift by up to +6. Current anchors: `assert_effective_profile_or_exit` call `:942` (block `:937-947`), `HttpServer::new` `:962`, `.bind()` `:1250`. The profile assertion still runs before `HttpServer::new` and `.bind()`; SECURITY-profiles invariant 6 now cites the current lines. The decision holds.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `src/main.rs`: `io::Error::new(Other, ..)` becomes `io::Error::other(..)`; a `let app = ..; app` binding inlined. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `src/main.rs`: removes the unread `PreReadSocketSettings` construction and its `app_data` registration, and drops dead constructor arguments to `OntologyQueryService` / `OntologyMutationService` (plus earlier lint-only edits since the recorded commit).

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**

## Re-verification — 2026-10-07 at 6e89f6adb (fix/broadcast-timer merge)

`src/main.rs` gains one line, the `on_connect` hook (`:1234`), and it comes after the assertion. `assert_effective_profile_or_exit` still runs at `:927`, before `HttpServer::new` (`:947`) and `.bind()` (`:1235`), so an unsupported posture still aborts before any listener binds. `src/config/security_profile.rs` is unchanged. **Still holds.** Checked by reading `git diff <previous verified_commit> 6e89f6adb` over this record's governed paths; the test suites were not re-run for this stamp.

## Re-verification — 2026-10-08 at bb76e75bd (fix/startup-errors)

`src/main.rs` changes in two hunks only: the RAGFlow start-up match (`:425-446`; an unset `RAGFLOW_API_KEY` is `RAGFlowError::NotConfigured`, logged as one WARN at `:431`, and the service stays `None`) and Step 1 of the data orchestration (`:605-627`; `FileService::initialize_local_storage` takes the corpus source and skips the GitHub markdown mirror for a local corpus, `:606`). Nothing touches release-build dev-auth exclusion (`enforce_release_env_hygiene`, `:117-169`, called at `:209` before any other start-up work; dev-auth cfg gate at `:293`), the boot profile assertion (`assert_effective_profile_or_exit`, `:934-944`, still before the listener binds) or fail-closed behaviour: no line added or removed in `main.rs` reads a security flag, and an absent RAGFlow key leaves chat off, which widens nothing. The assertion still runs after all services start and before `HttpServer::new`, and its abort path is unchanged. Decision holds.
