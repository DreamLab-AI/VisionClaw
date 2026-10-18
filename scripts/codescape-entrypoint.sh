#!/usr/bin/env bash
# Codescape service entrypoint (ADR-2143).
#
# Runs inside the development image but NOT under supervisord: one process,
# its own Cargo workspace, its own target volume, no /vault, no RuVector.
# Builds the server (release, CUDA when nvcc + a device exist) and the client
# bundle on first start or when sources changed, then execs the server.
set -euo pipefail

CS_ROOT=/app/crates/codescape
CS_TARGET=${CODESCAPE_TARGET_DIR:-/codescape-target}
CS_DIST=${CODESCAPE_STATIC_DIR:-$CS_TARGET/codescape-dist}
CS_DATA=${CODESCAPE_DATA_DIR:-/data/codescape}
CS_FEATURES=${CODESCAPE_FEATURES:-auto}

log() { printf '[codescape-entrypoint] %s\n' "$*" >&2; }

# Isolation guard: refuse a RuVector connection string outright (the server
# also checks, but failing here keeps the log unambiguous).
if [[ -n "${RUVECTOR_PG_CONNINFO:-}" ]]; then
  log "RUVECTOR_PG_CONNINFO is set; Codescape must not see the memory store (ADR-2143 §6)"
  exit 2
fi

mkdir -p "$CS_TARGET" "$CS_DATA"

features=()
if [[ "$CS_FEATURES" == "auto" ]]; then
  if command -v nvcc >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1; then
    features=(--features cuda)
    log "CUDA toolchain and device present: building with --features cuda"
  else
    log "no nvcc or no device: CPU engine only"
  fi
elif [[ "$CS_FEATURES" == "cuda" ]]; then
  features=(--features cuda)
fi

# CUDA PTX as a runtime artefact (ADR-2143 A5). The layout engine loads
# `visionclaw_unified.ptx` + `ptx-build-manifest.txt` from CODESCAPE_PTX_DIR and
# refuses anything without `provenance=compiled` and a matching content tag.
# The pair is produced by visionclaw-gpu's OWN build script (never written by
# hand here), through a throwaway wrapper package on the writable target
# volume, because the crate sources are mounted read-only and cargo would
# otherwise try to write a lockfile beside them. The output is copied to one
# fixed directory: the hashed build `out` dirs are never globbed, since a stale
# pair is internally consistent and would pass the checks.
export CODESCAPE_PTX_DIR="${CODESCAPE_PTX_DIR:-$CS_TARGET/ptx}"
build_ptx() {
  local gpu=/app/crates/visionclaw-gpu wrap="$CS_TARGET/ptx-wrapper" out="$CODESCAPE_PTX_DIR"
  if [[ ! -f "$gpu/Cargo.toml" ]]; then
    log "visionclaw-gpu sources not mounted; CUDA engine will fall back to CPU"
    return 0
  fi
  mkdir -p "$wrap/src"
  cat > "$wrap/Cargo.toml" <<EOF_WRAP
[package]
name = "codescape-ptx-wrapper"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
visionclaw-gpu = { path = "$gpu", default-features = false, features = ["gpu"] }

[workspace]
EOF_WRAP
  echo "" > "$wrap/src/lib.rs"
  local out_dir
  out_dir=$(cd "$wrap" && CARGO_TARGET_DIR="$CS_TARGET/ptx-build" \
    cargo build --release --message-format=json 2>/dev/null \
    | python3 -I -c '
import json, sys
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") == "build-script-executed" and "visionclaw-gpu" in m.get("package_id", ""):
        print(m["out_dir"])
' | tail -1)
  if [[ -z "$out_dir" || ! -f "$out_dir/visionclaw_unified.ptx" || ! -f "$out_dir/ptx-build-manifest.txt" ]]; then
    log "visionclaw-gpu build produced no PTX pair; CUDA engine will fall back to CPU"
    return 0
  fi
  local stage="$out.tmp.$$"
  rm -rf "$stage" && mkdir -p "$stage"
  cp "$out_dir/visionclaw_unified.ptx" "$out_dir/ptx-build-manifest.txt" "$stage/"
  chmod 0444 "$stage"/*
  rm -rf "$out" && mv "$stage" "$out"
  log "PTX pair staged at $out: $(grep -m1 '^visionclaw_unified ' "$out/ptx-build-manifest.txt")"
}

if [[ "${CODESCAPE_SKIP_BUILD:-0}" != "1" ]]; then
  if [[ ${#features[@]} -gt 0 ]]; then
    log "building the CUDA PTX artefact into $CODESCAPE_PTX_DIR"
    build_ptx
  fi
  log "building codescape-server (release) into $CS_TARGET"
  (cd "$CS_ROOT" && CARGO_TARGET_DIR="$CS_TARGET" cargo build --release --locked -p codescape-server "${features[@]}")
  if [[ -f /app/client/src/codescape/vite.config.ts && -d /app/client/node_modules ]]; then
    log "building the Codescape client bundle into $CS_DIST"
    (cd /app/client && HOME=/tmp CODESCAPE_DIST="$CS_DIST" node_modules/.bin/vite build --config src/codescape/vite.config.ts >/dev/null)
  else
    log "client sources or node_modules missing; serving without a bundle"
  fi
fi

repos=()
IFS=',' read -ra repo_specs <<< "${CODESCAPE_REPO:-visionclaw=/repos/visionclaw}"
for spec in "${repo_specs[@]}"; do
  [[ -n "$spec" ]] && repos+=(--repo "$spec")
done
excludes=(--exclude "${CODESCAPE_EXCLUDES:-target/**,node_modules/**,agentbox/**,data/**,logs/**,.dev-inputs/**,archive/**,vircadia-world/**}")

token_args=()
if [[ -n "${CODESCAPE_INGEST_TOKEN_FILE:-}" ]]; then
  token_args=(--ingest-token-file "$CODESCAPE_INGEST_TOKEN_FILE")
fi

log "starting codescape-server on :${CODESCAPE_PORT:-4100} (ingest :${CODESCAPE_INGEST_PORT:-4101})"
exec "$CS_TARGET/release/codescape-server" \
  "${repos[@]}" \
  "${excludes[@]}" \
  --data-dir "$CS_DATA" \
  --static-dir "$CS_DIST" \
  --port "${CODESCAPE_PORT:-4100}" \
  --ingest-port "${CODESCAPE_INGEST_PORT:-4101}" \
  --bind "${CODESCAPE_BIND:-0.0.0.0}" \
  --prefer-cuda "${CODESCAPE_PREFER_CUDA:-1}" \
  "${token_args[@]}" \
  "$@"
