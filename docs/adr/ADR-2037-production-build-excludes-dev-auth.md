---
id: ADR-2037
title: "Production release images are built without the dev-auth cargo feature, asserted in CI"
date: 2026-08-31
decision_status: proposed
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 81929f1f3c3688d08f0b2311ec19a7dbe158686f
verified_paths: [src/config/security_profile.rs, src/main.rs, .github/workflows/ci.yml, Dockerfile.production]
owner: jjohare
review_trigger: any change to the production Dockerfile build line, the dev-auth feature gates, or enforce_release_env_hygiene
repo: visionclaw
domain: SECURITY-profiles
---

# ADR-2037 — Production release images are built without the dev-auth cargo feature, asserted in CI

## Context

`src/main.rs:169` `enforce_release_env_hygiene()` is a no-op stub under
`#[cfg(any(debug_assertions, feature="dev-auth"))]`; the real boot-abort only compiles when
neither holds. ADR-2012's `Bearer dev-session-token` fence is likewise dev-auth-gated. ADR-2008
records the DEV image building `cargo build --release --features gpu,dev-auth` — a `--release`
binary that nonetheless carries the hygiene abort stubbed out AND the bypass fence's compile-time
gate open. No record currently forbids promoting such a dev-auth-featured release binary to
production, so a mis-targeted pipeline could ship stubbed hardening silently.

## Decision

Production and release images MUST be compiled without the `dev-auth` cargo feature; the release
build line carries only production features (e.g. `--features gpu`), never `dev-auth`. A CI or
image-build assertion MUST verify the shipped binary contains neither the
`enforce_release_env_hygiene` no-op stub nor the dev-session-token bypass fence — via a build-time
symbol/feature check on the emitted binary, or a boot self-test proving the hygiene abort is the
real (non-stub) implementation. A release artefact that fails this assertion fails the build; it is
never published. The dev image is exempt and retains `dev-auth` for local work.

## Consequences

- The dev image stays as-is; local development keeps the auth shortcut and the stubbed hygiene.
- The release pipeline gains a gate: a mis-built binary (dev-auth leaked into a release image)
  fails CI rather than silently shipping with the boot-abort stubbed and the bypass gate open.
- Closes the promotion gap between ADR-2008 (dev image recompiles) and ADR-2012 / ADR-2026
  (fences and boot-abort that only bite when dev-auth is absent) — those defences now have a
  build-time guarantee that release binaries actually compile them in.
- Cost: the release pipeline must add and maintain the assertion; a feature-set change to the
  production build line now requires updating this gate in lockstep.

## Verification

None yet — this record is `proposed` / `implementation_status: none`. Implementation lands the CI
or image-build assertion (symbol/feature check or boot self-test) and, once green, updates
`verified_commit` and `verified_paths` to the build config and check that were inspected.
Cross-ref ADR-2008 (dev image recompiles), ADR-2012 (dev bypass triple-gated), ADR-2026
(fail-closed boot-abort). Governing doc: `docs/SECURITY-profiles.md`.

## Closeout extension — 2026-09-04

CP-01/04/06/08. Owner remains jjohare with release/authentication maintainers. Proposed/none/inactive remains appropriate for the shipped-image assertion. Current production build commands/default features omit dev-auth, but that is not a verified emitted-binary guarantee. The isolated matrix confirms that a non-debug build with dev-auth retains the bypass and no-op hygiene.

**Acceptance condition:** Bind image digest, source, feature closure and effective profile to a receipt. Test production rejection before listener bind, including forbidden variables set to zero, and prevent promotion of a dev-auth artefact. Exercise full REST and WebSocket paths, report-mode interaction, network reachability and sentinel attribution separately from helper parsing. Preserve the distinction between the peer-agnostic full bypass and loopback dev-token mechanism. Reopen on build features, boot sequencing, bypass branches or profile policy. See the [review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/role-authority.md#development-bypass-and-release-identity), [reproducer](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-auth-probe.py) and [receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-auth-probe.json). No full image, listener, HTTP or headset execution ran.

## Acceptance progress — 2026-09-05

**Implemented.** `src/config/security_profile.rs` + the call site in
`src/main.rs`. The acceptance item "test production rejection **before listener
bind**, including forbidden variables set to zero, and prevent promotion of a
dev-auth artefact" is implemented as far as it can be without a build pipeline.

`BuildIdentity::current()` captures `cfg!(debug_assertions)` and
`cfg!(feature = "dev-auth")`; `is_production_artefact()` is true only for a
release build without dev-auth. A release build carrying dev-auth raises
`DevAuthFeatureInArtefact` — such a binary retains the loopback dev-token branch
and must never be promoted. `assert_effective_profile_or_exit` runs immediately
before `HttpServer::new`/`bind` and exits 2 on any finding in a production
artefact, so a mis-promoted image never accepts a request. The peer-agnostic
full bypass (ADR-2039) and the loopback dev-token mechanism remain distinct: the
forbidden-variable set names `VISIONCLAW_DEV_MODE` and `DEV_AUTH_LOOPBACK`
separately.

**Tests.** `cargo test --lib --no-default-features security_profile` — 29
passed, 0 failed, including: a release dev-auth artefact reported; a debug
dev-auth build *not* reported (it is not a promotion candidate); a production
artefact with findings refusing to bind; a development build with the same
findings binding; and the forbidden-variable matrix at `0`, `""` and `1`.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2012-2038-security-profile.txt`.

**Remains open.** Image digest, source and feature closure are still not bound
to a receipt — that needs a CI step this pass cannot add. Full REST and
WebSocket paths, network reachability and sentinel attribution are unexercised.
No image or listener ran. `implementation_status` is left `none`/`inactive`:
the boot check exists, but the shipped-image assertion this record is about is
not evidenced.


## Executable implementation progress — 2026-09-07

Decision remains proposed; this audit does not ratify image-promotion policy.
Implementation is now partial/staged rather than none: the non-debug pre-bind
profile guard rejects release/dev-auth findings, and the existing CI feature gate
checks production build posture. A default-feature release artefact was actually
built; its hash and two exit-2 forbidden-variable probes are recorded in the
[execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md).
Both probes were repeated with a fully matching explicit profile, so profile drift
is not their refusal reason. The tested artefact predates later storage work.
No claim is made that every produced Docker image has the tested feature closure
or that a deployed image has passed authenticated negative-route probes.
