---
id: ADR-2008
title: The development image recompiles the Rust backend on container start
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
verified_paths: [scripts/dev-entrypoint.sh, docker-compose.unified.yml]
owner: jjohare
review_trigger: a dev-loop turnaround that makes on-start compilation intolerable, or a move to pre-baked dev binaries by default
repo: visionclaw
domain: BASELINE-architecture
lineage: No dedicated legacy ADR; distils the dev/prod build-path divergence (project_dev_prod_build_sync.md).
---

# ADR-2008 — The development image recompiles the Rust backend on container start

## Context

The project mount is a host bind-mount: developers edit source on the host and
expect changes to take effect without an image rebuild. A pre-baked binary in
the image would silently run stale code against edited source. The dev and prod
build paths therefore diverge deliberately. Lineage: the dev/prod build-path
divergence note (`project_dev_prod_build_sync.md`).

## Decision

The original individual-service development branch rebuilds with
`cargo build --release --features gpu,dev-auth`. The normal supervisor path now uses a timestamp-gated wrapper instead; see the
closeout extension for its input coverage and failure handling. The pre-baked binary is opt-in via
`SKIP_RUST_REBUILD=true`. Production pre-compiles in the image build and does not
recompile on start.

## Consequences

- Edit-on-host, restart-container is the dev loop; no `docker build` per change.
- Container start pays a full `--release` compile (minutes) unless
  `SKIP_RUST_REBUILD=true` — the cost of never running stale code by accident.
- A broken tree fails the container loudly at boot instead of masking the error
  behind an old binary.

## Verification

`scripts/dev-entrypoint.sh` guards the rebuild on `SKIP_RUST_REBUILD` (~:103),
runs `cargo build --release --features gpu,dev-auth` (~:108) and `exit 1`s on
failure (~:113). `docker-compose.unified.yml` sets `target: development` (:52)
and `BUILD_TARGET: ${BUILD_TARGET:-development}` (:55), with the production stage
distinct (`BUILD_TARGET: production`, :157). Verified at `e0f8cd896`.

### Re-verification 2026-09-05 (ADR-2086)

Verification ran on the **uncommitted working tree** above SHA
`b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`; `verified_paths` is emptied because
the tree is uncommitted and the staleness gate cannot bind paths to a commit that
does not yet contain them. Both must be restored at the landing commit.

All claims above still hold, with one line-number correction:

- `scripts/dev-entrypoint.sh:103` guards on `SKIP_RUST_REBUILD` (`!= "true"`),
  `:108` runs `cargo build --release --features gpu,dev-auth`, `:113` `exit 1`s on
  failure, `:119` logs the skip path. Unchanged.
- `docker-compose.unified.yml:52` `target: development` and `:55`
  `BUILD_TARGET: ${BUILD_TARGET:-development}`. Unchanged.
- The production stage's `BUILD_TARGET: production` has moved from `:157` to
  **`:178`**.
- The closeout's note that the normal supervisor path goes through
  `scripts/rust-backend-wrapper.sh` is confirmed: it builds with
  `cargo build --release --features "$BUILD_FEATURES"` at `:70` and `:79`, i.e.
  the feature set is a variable rather than the literal in `dev-entrypoint.sh`.

**Security consequence, now gated.** `dev-entrypoint.sh:108` produces a
`--release` binary carrying `dev-auth`, which is precisely the hazard ADR-2037
names: `src/main.rs:169` compiles `enforce_release_env_hygiene()` to a no-op stub
under `#[cfg(any(debug_assertions, feature = "dev-auth"))]`, so such a binary
ships the boot-abort disabled. This ADR's dev-loop decision is unchanged and
remains correct for the dev image; ADR-2086 adds the CI assertion that stops a
`dev-auth`-featured binary being built for production.

## Closeout extension — 2026-09-04

CP-01/06/08. Owner remains jjohare with development/build/GPU maintainers. Implementation is partial; historical live activation is retained. The normal supervisor path invokes rust-backend-wrapper.sh, whose timestamp heuristic can skip Cargo without SKIP_RUST_REBUILD. Its build uses gpu,ontology,dev-auth and visionclaw-server. Three extracted decision fixtures show crate Rust edits trigger rebuilding, while crate CUDA and crate-manifest edits are missed. The older individual-service branch is not the default supervisor path.

**Acceptance condition:** Define authoritative startup/build-input handling; verify every source/config/toolchain/feature input, skip mode, failure/retry and selected-binary identity in the actual image. Reconcile the old path and inspect process readiness separately from compilation success. Reopen on wrapper, supervisor, mounts, build-input or toolchain changes. See [development review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/configuration-projection.md#development-restart-and-build-input-coverage), [probe](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-build-input-probe.json) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/dev-docs-closeout.json). No build, clean, container or server operation ran.

## Acceptance progress — 2026-09-05

**Implemented.** The rebuild decision is extracted from
`scripts/rust-backend-wrapper.sh` into `scripts/lib/build-inputs.sh`, the single
authoritative build-input inventory, and the wrapper now sources it. Both
reproduced misses are closed: the old heuristic globbed `*.rs` under
`/app/src` + `/app/crates` but stat'd only the **root** `Cargo.toml`/`Cargo.lock`/
`build.rs`, and globbed `*.cu` under `/app/src` only, so a crate manifest, a
crate build script and a crate CUDA kernel all left a stale binary running.

The inventory covers `*.rs`, `*.cu`, `*.cuh`, `*.ptx`, every `Cargo.toml`,
`Cargo.lock`, `rust-toolchain*` and `.cargo/config.toml` under both roots, with
`target/`, `node_modules/`, `.git/` pruned. It also covers the build inputs no
timestamp can see: `build_env_signature` records the cargo feature set and the
`rerun-if-env-changed` variables both build scripts declare — notably
`CUDA_ARCH`, which the wrapper itself recomputes from `nvidia-smi` on every
start, so moving the image to a different GPU changes a real build input with no
file touched. The signature is written to a stamp after a successful build and
compared on the next start; the stamp is removed on a failed build. Rebuild is
the safe default for every unevaluable branch (missing binary, empty tree,
missing stamp, changed environment), and the mtime comparison is `<=` so a
same-second edit rebuilds.

**Tests.** `cargo test -p visionclaw-integration-tests --test dev_build_inputs`
— 18 passed, 0 failed. The three probe fixtures are executable rows: crate Rust
(rebuilds), crate CUDA (**was skipped**, now rebuilds), crate manifest (**was
skipped**, now rebuilds), plus crate build script, PTX, root inputs, the
CUDA_ARCH swap with no file edit, unset-vs-empty variables, missing stamp,
missing binary, empty tree, same-second edit, and negative cases proving
`target/`, `node_modules/` and docs are not build inputs.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2008-dev-build-inputs.txt`.

**Remains open.** No build, container or supervisor operation ran (CLAUDE.md
forbids docker builds in this container), so selected-binary identity in the
actual image, skip-mode behaviour under the real supervisor, failure/retry, and
the older individual-service branch remain unverified. Process readiness is
still not inspected separately from compilation success.


## Source closeout verification — 2026-09-07

Compose now forwards explicit profile intent and a default-off session compatibility flag. Source mounts and the recompilation entrypoint are unchanged. A release/dev-auth entrypoint is now refused by the boot guard; development must use a debug build as recorded in SECURITY-profiles.md. This is a required migration, not a claim that hosted dev images have been rebuilt.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.
