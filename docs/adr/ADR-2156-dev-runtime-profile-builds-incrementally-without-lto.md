---
id: ADR-2156
title: The dev-runtime profile builds incrementally without cross-crate LTO
date: 2026-10-29
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 17e3cdda0c51ad84d942c469c77814d915753313
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
