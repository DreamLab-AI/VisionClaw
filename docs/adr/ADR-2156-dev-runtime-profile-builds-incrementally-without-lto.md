---
id: ADR-2156
title: The dev-runtime profile builds incrementally without cross-crate LTO
date: 2026-10-29
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: b5200feb4c0d89bc5544d78884d9d79fcdfc67b1
verified_paths: [Cargo.toml, scripts/rust-backend-wrapper.sh, build.rs]
owner: jjohare
review_trigger: a runtime-performance regression traced to the dev-runtime profile, or a move to pre-baked dev binaries (ADR-2008)
repo: visionclaw
domain: BASELINE-architecture
lineage: Amends ADR-2008 (on-start recompilation), whose review trigger, "a dev-loop turnaround that makes on-start compilation intolerable", fired.
---

# ADR-2156 — The dev-runtime profile builds incrementally without cross-crate LTO

## Context
ADR-2008 recompiles the backend on container start. Its diff detection is correct: the 2026-10-29 start rebuilt only `visionclaw-server` and `visionclaw-jev`. But `[profile.dev-runtime]` inherited release's `lto = true` and `codegen-units = 1`. The root crate therefore ran a fat-LTO pass over every rlib on a single core, for more than 10 minutes, on a 72-core host. Meanwhile nginx answered `/api/*` with 502. The wrapper's failure path also ran a full `cargo clean`, so a plain compile error wiped every cached dependency. Supervisor's autorestart then repeated that cold build on each retry. The root `build.rs` declared `rerun-if-changed=.git/HEAD`. `.git` is not mounted in the container, and cargo treats a missing path as always changed.

## Decision
- `[profile.dev-runtime]` keeps `inherits = "release"`, `opt-level = 3`, `debug-assertions` and `overflow-checks`. It overrides `lto = "off"`, `codegen-units = 256` and `incremental = true`. `[profile.release]` is unchanged, so production artefacts still use fat LTO.
- On a build failure, `rust-backend-wrapper.sh` runs only `cargo clean --profile dev-runtime -p visionclaw-server` before its single retry. It never runs a full `cargo clean`.
- The root `build.rs` declares `.git/HEAD` as a rerun input only when the file exists.

## Consequences
- An edit to the root crate is compiled in parallel and incrementally, and the slow single-core LTO link is gone. Runtime speed stays close to release, because cross-crate inlining is the only optimisation given up. Benchmarks taken on the dev container are therefore no longer release-equivalent, so measure performance on a release build.
- Changing the profile invalidates the `dev-runtime` cache once. The first start after this change rebuilds every dependency.
- Incremental artefacts make `target/` larger. The `cargo-target-cache` volume holds them.

## Verification
- `python3 scripts/tests/test_dev_launcher.py` passes: profile identity, skip and failure paths.
- `bash scripts/tests/test-dev-inputs.sh` passes: 86 checks.
- `cargo metadata --no-deps` accepts the manifest.
- Activation is staged until a `./scripts/launch.sh redeploy dev` has timed a single-file rebuild.

Re-verified 2026-10-30 at 17e3cdda0: code committed at dd5be4d9e; `Cargo.toml:332-334` (`lto = "off"`, `codegen-units = 256`, `incremental = true`), `build.rs:38` and `scripts/rust-backend-wrapper.sh:124` match the decision.

## Close-out 2026-10-30

`staged → live`. `verified_commit` moves to `772039bd172027af034d64befacca1ebcf0bbc04`; the governed paths are unchanged.

**Profile at this commit.** `Cargo.toml:332-334` holds `lto = "off"`, `codegen-units = 256` and
`incremental = true`. `[profile.release]` keeps `lto = true` and `codegen-units = 1` (`:318-319`).

**The timed rebuild.** The running backend is `/app/target/dev-runtime/visionclaw-server`, with build stamp
`/app/target/.visionclaw-dev-runtime-build-stamp` dated 19:02. Its wrapper log reads:
`[RUST-WRAPPER][2026-10-30 19:01:06] Building from published inputs: source_sha=772039bd172027af034d64befacca1ebcf0bbc04 source_dirty=0`, then
`Rebuilding: build input newer than binary`, `Compiling visionclaw-xr-presence`, `visionclaw-adapters`,
`visionclaw-server`, and finally `` Finished `dev-runtime` profile [optimized] target(s) in 1m 00s ``. That was a three-crate
incremental rebuild after a merge, not the single-file edit the Verification section names. It still ends the
condition the record was written against: a fat-LTO single-core pass of more than 10 minutes.
No command was typed: the container's wrapper started this rebuild on boot, so the log above is the whole invocation
record. The single-file timing in Verification has not been run; `live` here means the profile is the one serving, not
that the measurement passed (qualified after the 2026-10-30 cross-vendor audit).

## Scoped re-verification — 2026-11-01, ADR-2168

The manifest adds only the shared compiler dependency/default feature. Dev-runtime/release profile tables, wrapper retry behaviour and build.rs are unchanged. No runtime rebuild timing or deployment was repeated.

All governed paths were byte-identical between the previous verification pin and
the pre-change baseline `5e1bd90a4`. The reviewed delta is the shared-layout change
through `974daf5d22d2d0ea709140777eae58d60ddb2c36`. Existing activation and acceptance limits
remain in force; this stamp does not repeat historical runtime measurements.

## Scoped patch re-verification — 2026-11-01

Only the Ontomark compiler minimum and registry lock move from 0.6.2 to 0.6.3
in this record's governed paths through `efbd7f6f080df06ecb580254bddabcee59d4fbf8`.
The manifest feature graph and consumer implementation are unchanged. Registry
vault/core tests, strict Clippy, the actual local-module tests and the server
library compile check pass; prior runtime and activation limits remain.

## Re-verification — 2026-11-04 (`5c94c3cbd`)

Re-verified after merging feat/splats (ADR-2170, Gaussian splat scenes). The governed-path changes since `efbd7f6f0` are additive: `Cargo.toml`: a workspace member and path dependency for `crates/visionclaw-splat`; no profile, feature or other dependency changed. This record's own claims are unchanged. Evidence on the merged tree: `cargo test --lib` 1823 passed, 1 failed (`nip98_proxy_tests::prod_lan_headset_wss_authenticate_validates_through_nginx`, which reads only `nginx.*.conf` and `auth.rs`, neither touched; it fails identically at `d74b25143`); `xr-client/rust` 699 passed; XR GUT 595 of 598 passing (3 pending pixel tests); vitest 1593 passed. Re-run: `git diff efbd7f6f0..5c94c3cbd -- <governed path>`.

Governed-path change (server consolidation, 2026-11-06, `923feef34..b5200feb4`): `Cargo.toml` drops the fastwebsockets/postcard/hyper/hyper-util/http-body-util/http dependencies of the deleted `FastWebSocketServer`. The `dev-runtime` profile is untouched. **Decision unaffected.**
