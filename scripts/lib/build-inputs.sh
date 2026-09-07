#!/usr/bin/env bash
# build-inputs.sh — the authoritative build-input inventory for the dev-image
# rebuild decision (ADR-2008).
#
# `rust-backend-wrapper.sh` skips cargo entirely when the development binary looks
# newer than the sources, which turns a no-change restart from ~30s into ~1s.
# The estate-review probe reproduced two holes in the original heuristic:
#
#   * it globbed `*.rs` under /app/src and /app/crates but only looked at the
#     ROOT Cargo.toml/Cargo.lock/build.rs, so editing a CRATE manifest
#     (crates/*/Cargo.toml) or a crate build script left a stale binary running;
#   * it globbed `*.cu` under /app/src only, so editing a CRATE CUDA kernel
#     (crates/visionclaw-gpu/src/cuda_sources/*.cu) was likewise missed.
#
# It also ignored the non-file build inputs entirely. `build.rs` and
# `crates/visionclaw-gpu/build.rs` both declare `rerun-if-env-changed` for
# CUDA_ARCH, DOCKER_ENV and CARGO_FEATURE_GPU — and the wrapper *itself*
# recomputes CUDA_ARCH from `nvidia-smi` on every start. Moving the image to a
# different GPU therefore changes a real build input without touching a single
# file, which no timestamp comparison can ever see.
#
# This file is the single place that answers "what is a build input?". Source
# it; do not duplicate its globs.
#
#   source scripts/lib/build-inputs.sh
#   if needs_rebuild "$RUST_BINARY" /app /app/target/.build-stamp; then ... fi
#
# Every function is pure with respect to the filesystem except
# `write_build_stamp`, which is what makes the whole decision testable against
# fixture trees.

# Directories that never contain build inputs, however deep the tree goes.
BUILD_INPUT_PRUNE_DIRS="${BUILD_INPUT_PRUNE_DIRS:-target node_modules .git .venv dist}"

# File names/extensions that ARE build inputs, relative to the source roots.
#   *.rs            — Rust sources, including every crate's build.rs
#   *.cu *.cuh      — CUDA kernels and their headers (compiled by build.rs)
#   *.ptx           — pre-compiled PTX shipped alongside the kernels
#   Cargo.toml      — root AND crate manifests: features, deps, members
#   Cargo.lock      — the resolved dependency graph
#   rust-toolchain*  and .cargo/config.toml — toolchain and target selection
BUILD_INPUT_NAME_GLOBS="${BUILD_INPUT_NAME_GLOBS:-*.rs *.cu *.cuh *.ptx Cargo.toml Cargo.lock rust-toolchain rust-toolchain.toml config.toml}"

# Environment variables the build scripts declare as `rerun-if-env-changed`.
# A change to any of these invalidates the binary without touching a file.
BUILD_INPUT_ENV_VARS="${BUILD_INPUT_ENV_VARS:-CUDA_ARCH CUDA_PATH DOCKER_ENV CARGO_BUILD_FEATURES CARGO_PROFILE_DEV_RUNTIME_DEBUG_ASSERTIONS}"

# Emit every build-input path under $1 (default: the current directory).
# Used directly by the fixture tests to assert coverage.
list_build_inputs() {
    local root="${1:-.}"
    local prune_expr=() name_expr=() prune_dirs=() name_globs=() d g first=1
    # Split the configured words without expanding *.rs against the caller's
    # cwd (the project root contains build.rs, which otherwise hides src/*.rs).
    read -r -a prune_dirs <<< "$BUILD_INPUT_PRUNE_DIRS"
    read -r -a name_globs <<< "$BUILD_INPUT_NAME_GLOBS"

    for d in "${prune_dirs[@]}"; do
        prune_expr+=(-name "$d" -o)
    done
    unset 'prune_expr[${#prune_expr[@]}-1]'   # drop the trailing -o

    for g in "${name_globs[@]}"; do
        if (( first )); then
            name_expr+=(-name "$g"); first=0
        else
            name_expr+=(-o -name "$g")
        fi
    done

    find "$root" \
        \( -type d \( "${prune_expr[@]}" \) -prune \) -o \
        \( -type f \( "${name_expr[@]}" \) -print \) 2>/dev/null
}

# Echo the newest build-input mtime under $1 as whole epoch seconds, or 0 when
# the tree holds no build inputs at all.
latest_build_input_mtime() {
    local root="${1:-.}" newest
    newest=$(list_build_inputs "$root" \
        | tr '\n' '\0' \
        | xargs -0 -r stat -c %Y 2>/dev/null \
        | sort -n | tail -1)
    echo "${newest:-0}"
}

# Echo a stable one-line signature of the non-file build inputs: the cargo
# feature set the wrapper will pass, plus every `rerun-if-env-changed`
# variable. Unset variables are rendered explicitly so "unset" and "empty" are
# distinguishable, which is what makes a GPU swap (CUDA_ARCH 75 -> 89) or a
# feature-set edit visible to the decision.
build_env_signature() {
    local features="${1:-}" var out
    out="features=${features}"
    for var in $BUILD_INPUT_ENV_VARS; do
        if [[ -n "${!var+x}" ]]; then
            out="${out};${var}=${!var}"
        else
            out="${out};${var}=<unset>"
        fi
    done
    echo "$out"
}

# Write the stamp recording the environment signature the binary was built
# with. Called only after a successful build.
write_build_stamp() {
    local stamp="$1" features="${2:-}"
    mkdir -p "$(dirname "$stamp")" 2>/dev/null || true
    build_env_signature "$features" > "$stamp"
}

# Decide whether cargo must run.
#
#   needs_rebuild <binary> <source-root> <stamp-file> [features]
#
# Exits 0 (build needed) or 1 (skip), echoing a one-line human reason either
# way. Rebuild is the safe default: any condition we cannot evaluate — missing
# binary, empty source tree, unreadable stamp — builds.
needs_rebuild() {
    local binary="$1" root="$2" stamp="$3" features="${4:-}"

    if [[ ! -f "$binary" ]]; then
        echo "no binary at $binary"
        return 0
    fi

    local bin_mtime latest
    bin_mtime=$(stat -c %Y "$binary" 2>/dev/null || echo 0)
    latest=$(latest_build_input_mtime "$root")

    if [[ "$latest" -eq 0 ]]; then
        echo "no build inputs found under $root (refusing to trust the binary)"
        return 0
    fi
    if [[ "$bin_mtime" -le "$latest" ]]; then
        echo "build input newer than binary (input=$latest binary=$bin_mtime)"
        return 0
    fi

    # File timestamps agree; check the inputs no timestamp can see.
    local want have
    want="$(build_env_signature "$features")"
    if [[ ! -f "$stamp" ]]; then
        echo "no build stamp at $stamp (build environment unverifiable)"
        return 0
    fi
    have="$(cat "$stamp" 2>/dev/null || true)"
    if [[ "$want" != "$have" ]]; then
        echo "build environment changed (was [$have], now [$want])"
        return 0
    fi

    echo "binary is up to date (newest input=$latest binary=$bin_mtime, environment unchanged)"
    return 1
}
