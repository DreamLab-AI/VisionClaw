---
id: ADR-2049
title: The production image's dependency-warming stage must be able to fail
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: b00c28a0d766c8cf46cd00b100dab60ef2dd74a4
verified_paths: []
owner: jjohare
review_trigger: a change to the Dockerfile.production stage layout, or adoption of cargo-chef for dependency caching; activates on the first successful host docker build of Dockerfile.production at a commit carrying this change
repo: visionclaw
domain: BASELINE-architecture
lineage: ADR-2008 governs the dev rebuild decision; this is its production-image counterpart.
---

# ADR-2049 — The production image's dependency-warming stage must be able to fail

## Context

`Dockerfile.production` warms the dependency cache in a dedicated stage using a
stub `build.rs`, so the ~200 dependencies are compiled into a layer that is
cached until `Cargo.toml` changes. Phase 1 (diagram VC-08.11) found the stage
could not fail for any reason:

```dockerfile
RUN cargo build --release 2>&1 || true && \
    cargo build --release --lib 2>&1 || true
```

Shell precedence makes this `((A || true) && B) || true`: the second build always
runs and the whole `RUN` always exits 0. A broken `Cargo.lock`, an unreachable
registry, or a dependency that no longer compiles all produced a green layer, and
the failure surfaced later in the real build stage (`:164`) with a poisoned cache
behind it.

The crate build in this stage is **expected** to fail — the real `build.rs` and
the CUDA `.cu` files arrive in later stages — so the tolerance itself was not the
bug. The bug was that nothing in the stage was checked at all.

## Decision

The dependency-warming stage separates the two concerns. Resolving and fetching
the dependency graph **must succeed**, and is gated by `cargo fetch --locked`.
Compiling the crate against the stub `build.rs` **may** fail, and says so in
plain words rather than through an opaque `|| true`. The redundant second
`cargo build --release --lib` is removed: it built the same dependency graph a
second time and could not fail either.

`--locked` is deliberate. `Cargo.lock` is tracked in the repository and copied
into the image at `Dockerfile.production:64`, so a production image is built from
the resolved graph the repository declares; a lockfile that no longer matches
`Cargo.toml` fails the build rather than silently resolving to something else.

## Consequences

- A dependency-resolution failure now fails the image build at the stage that
  caused it, with the cache un-poisoned.
- A stale `Cargo.lock` becomes a build failure. That is the intent for a
  production artefact, and it is a behaviour change for anyone who was relying on
  implicit re-resolution.
- The authoritative build at `Dockerfile.production:164` was already correct —
  `rm -rf … && cargo build --release && cp …` is `&&`-chained, so it fails
  properly. It is unchanged.
- This does not adopt `cargo-chef`, which would express the same intent more
  directly. That is a larger change to the stage layout and is the review trigger
  for this record.

## Verification

`Dockerfile.production:89-103` now reads: the stub `build.rs` write, then
`RUN cargo fetch --locked`, then the crate build with an explanatory fallback
message instead of `|| true`.

Confirmed at this commit:
- `git ls-files --error-unmatch Cargo.lock` → tracked, so `--locked` has an input.
- `Dockerfile.production:64` → `COPY Cargo.toml Cargo.lock ./`, so the lockfile is
  present in the stage before `cargo fetch` runs.
- Shell precedence of the old line re-read directly from the file before editing;
  there is no `SHELL […] -o pipefail` directive in this Dockerfile, which is why
  the original `|| true` chain swallowed both exit codes.

**Not executed here.** `cargo fetch --locked` could not be run to completion in
this container: the shared cargo registry at
`/home/devuser/workspace/.cargo/registry` is not writable by this session
(`failed to remove file … protobuf-2.28.0/regenerate.sh: Permission denied`). The
command got far enough to be unpacking sources, which shows the lockfile resolves,
but the change is text-verified rather than executed and **must be exercised by an
actual `docker build` of `Dockerfile.production` before this record moves to
`activation_status: live`.** That is why it is `staged`.

**Verification ran on the uncommitted working tree above
`b00c28a0d766c8cf46cd00b100dab60ef2dd74a4` and must be re-run at the landing
commit, which sets `verified_paths`.**
