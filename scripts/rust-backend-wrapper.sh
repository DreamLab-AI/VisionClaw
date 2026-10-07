#!/bin/bash
# Wrapper script for rust-backend used by supervisord in development mode.
# Skips cargo entirely when binary is already up-to-date — restarts with no
# source changes take ~1s instead of ~30s.
#
# ADR-2008: the "up-to-date" decision lives in scripts/lib/build-inputs.sh, the
# single authoritative inventory of what counts as a build input. It covers
# every Rust source, CUDA kernel/header/PTX and Cargo manifest under BOTH
# /app/src and /app/crates (the original inline heuristic missed crate
# manifests and crate CUDA entirely), plus the toolchain files and the
# `rerun-if-env-changed` variables the build scripts declare — including the
# CUDA_ARCH this very script recomputes from nvidia-smi on each start.

set -e

export DOCKER_ENV=1

log() {
    echo "[RUST-WRAPPER][$(date '+%Y-%m-%d %H:%M:%S')] $1"
}

# Auto-detect GPU compute capability at runtime.
# ALWAYS prefer runtime detection over .env/compose values — .env may be stale.
DETECTED_ARCH=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader --id=0 2>/dev/null | head -1 | tr -d '.' | tr -d '[:space:]')
if [ -n "$DETECTED_ARCH" ]; then
    if [ -n "${CUDA_ARCH:-}" ] && [ "$CUDA_ARCH" != "$DETECTED_ARCH" ]; then
        log "WARNING: .env CUDA_ARCH=${CUDA_ARCH} != GPU sm_${DETECTED_ARCH}. Overriding."
    fi
    export CUDA_ARCH="$DETECTED_ARCH"
    log "GPU compute capability: sm_${CUDA_ARCH} (runtime-detected)"
else
    export CUDA_ARCH="${CUDA_ARCH:-75}"
    log "WARNING: nvidia-smi failed, using sm_${CUDA_ARCH}"
fi

# Supervisor owns stderr rotation; retain fatal diagnostics across retries.

APP_ROOT="${APP_ROOT:-/app}"
RUST_BINARY="${RUST_BINARY:-$APP_ROOT/target/dev-runtime/visionclaw-server}"
# The feature set is part of the binary's identity: a change here must rebuild
# even when no file changed, so it feeds the stamp signature.
BUILD_FEATURES="${BUILD_FEATURES:-gpu,ontology,dev-auth}"
# This is the development launcher. Keep optimisation without misidentifying
# a dev-auth artefact as production (ADR-2038). Pin the assertion override too.
export CARGO_PROFILE_DEV_RUNTIME_DEBUG_ASSERTIONS=true
BUILD_STAMP="${BUILD_STAMP:-$APP_ROOT/target/.visionclaw-dev-runtime-build-stamp}"

# ADR-2008: the authoritative build-input inventory.
WRAPPER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/build-inputs.sh
. "$WRAPPER_DIR/lib/build-inputs.sh"
# ADR-2008 (2026-10-07): root files (Cargo.toml, Cargo.lock, build.rs) reach /app
# by copy from /app/.dev-inputs, which the host publishes and stamps; never by a
# single-file bind mount, which pins a stale inode after a git operation.
# shellcheck source=lib/dev-inputs.sh
. "$WRAPPER_DIR/lib/dev-inputs.sh"

# Build against the committed lock. The container never resolves dependencies
# on its own: a lock it wrote would land in /app only, which the host never sees
# (the hazard the old rw Cargo.lock bind mount had). A stale host lock fails
# loudly here instead, with the remedy, and without wiping the target cache.
cargo_build() {
    local output rc
    output="$(mktemp)"
    set +e
    cargo build --locked --profile dev-runtime --features "$BUILD_FEATURES" 2>&1 | tee "$output"
    rc=${PIPESTATUS[0]}
    set -e
    if [ "$rc" -ne 0 ] && grep -q -- "--locked was passed" "$output"; then
        rm -f "$output"
        log "FATAL: Cargo.lock does not match Cargo.toml. The container builds --locked against the host's committed lock."
        log "       Refresh it on the host (e.g. \`cargo metadata --format-version 1 >/dev/null\` in the checkout), commit it, then run ./scripts/launch.sh redeploy dev."
        exit 1
    fi
    rm -f "$output"
    return "$rc"
}

NEEDS_BUILD=true
NEEDS_BUILD_REASON=""

if [ "${SKIP_RUST_REBUILD:-false}" != "true" ]; then
    cd "$APP_ROOT"

    # Install the published root files before deciding anything, and say which
    # commit they came from. No stamp, or a stamp that does not match the files,
    # means no trustworthy manifest: refuse rather than build something stale.
    # The container cannot see the host's HEAD (.git is not mounted: its config
    # can hold credentials), so the stamp's source_sha in this log is the
    # operator's check, and `launch.sh redeploy dev` always republishes first.
    if ! dev_inputs_install; then
        log "FATAL: refusing to build: root build files are not verified (see the DEV-INPUTS line above)"
        exit 1
    fi
    log "Building from published inputs: $(dev_inputs_describe)"

    # ADR-2008: ask the shared build-input inventory whether cargo must run.
    # Rebuild is the safe default — every branch that cannot be evaluated
    # (missing binary, empty tree, missing stamp, changed environment) builds.
    if NEEDS_BUILD_REASON=$(needs_rebuild "$RUST_BINARY" "$APP_ROOT" "$BUILD_STAMP" "$BUILD_FEATURES"); then
        NEEDS_BUILD=true
    else
        NEEDS_BUILD=false
    fi

    if [ "$NEEDS_BUILD" = "false" ]; then
        log "Skipping cargo: $NEEDS_BUILD_REASON"
    else
        log "Rebuilding: $NEEDS_BUILD_REASON"

        if cargo_build; then
            log "✓ Build succeeded"
            write_build_stamp "$BUILD_STAMP" "$BUILD_FEATURES"
        else
            log "ERROR: Build failed. Attempting clean rebuild..."
            # A failed build leaves the stamp describing an environment no
            # binary was produced for; drop it so the next start rebuilds.
            rm -f "$BUILD_STAMP"
            cargo clean 2>/dev/null || true
            if cargo_build; then
                log "✓ Clean rebuild succeeded"
                write_build_stamp "$BUILD_STAMP" "$BUILD_FEATURES"
            else
                log "FATAL: Clean rebuild also failed"
                exit 1
            fi
        fi
    fi
else
    log "Skipping Rust rebuild (SKIP_RUST_REBUILD=true)"
    RUST_BINARY="$APP_ROOT/visionclaw-server"
fi

if [ ! -f "${RUST_BINARY}" ]; then
    log "ERROR: Rust binary not found at ${RUST_BINARY}"
    exit 1
fi

log "Starting Rust backend from ${RUST_BINARY}..."
exec "${RUST_BINARY}"
