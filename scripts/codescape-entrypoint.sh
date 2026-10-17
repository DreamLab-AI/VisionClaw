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

if [[ "${CODESCAPE_SKIP_BUILD:-0}" != "1" ]]; then
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
