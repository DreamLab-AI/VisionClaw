#!/bin/bash
# Wrapper script for rust-backend that ensures rebuild on startup
# This is used by supervisord in development mode

set -e

# Set Docker environment variable to ensure PTX compilation at runtime
export DOCKER_ENV=1

log() {
    echo "[RUST-WRAPPER][$(date '+%Y-%m-%d %H:%M:%S')] $1"
}

# Auto-detect GPU compute capability at runtime (GPU is accessible in container).
# ALWAYS prefer runtime detection over .env/compose values — the .env may contain
# a stale arch from a different GPU (e.g. sm_89 when the actual GPU is sm_86).
DETECTED_ARCH=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader --id=0 2>/dev/null | head -1 | tr -d '.' | tr -d '[:space:]')
if [ -n "$DETECTED_ARCH" ] && [ "$DETECTED_ARCH" != "" ]; then
    if [ -n "${CUDA_ARCH:-}" ] && [ "$CUDA_ARCH" != "$DETECTED_ARCH" ]; then
        log "WARNING: .env CUDA_ARCH=${CUDA_ARCH} does not match GPU (sm_${DETECTED_ARCH}). Overriding to sm_${DETECTED_ARCH}"
    fi
    export CUDA_ARCH="$DETECTED_ARCH"
    log "GPU compute capability: sm_${CUDA_ARCH} (runtime-detected)"
else
    if [ -n "${CUDA_ARCH:-}" ]; then
        log "WARNING: nvidia-smi failed, using .env CUDA_ARCH=${CUDA_ARCH}"
    else
        export CUDA_ARCH="75"
        log "WARNING: nvidia-smi failed, no .env CUDA_ARCH, falling back to sm_75"
    fi
fi

# Truncate stale error log from previous runs
> /app/logs/rust-error.log

# Always rebuild in dev mode unless explicitly skipped
if [ "${SKIP_RUST_REBUILD:-false}" != "true" ]; then
    log "Rebuilding Rust backend with GPU support to apply code changes..."
    cd /app

    # Clean stale incremental cache if fingerprints look corrupt
    if [ -d "/app/target/release/.fingerprint" ]; then
        FINGERPRINT_AGE=$(find /app/target/release/.fingerprint -maxdepth 1 -type d -mmin +1440 2>/dev/null | head -1)
        if [ -n "$FINGERPRINT_AGE" ]; then
            log "Stale fingerprints detected (>24h old), cleaning incremental cache..."
            cargo clean 2>/dev/null || true
        fi
    fi

    # Build release with GPU features (matches dev-entrypoint.sh)
    if cargo build --release --features gpu,dev-auth 2>&1; then
        log "✓ Rust backend rebuilt successfully (release build with GPU)"
    else
        log "ERROR: Failed to rebuild Rust backend"
        log "Attempting clean build..."
        cargo clean 2>/dev/null || true
        if cargo build --release --features gpu,dev-auth 2>&1; then
            log "✓ Clean rebuild succeeded"
        else
            log "FATAL: Clean rebuild also failed"
            exit 1
        fi
    fi

    RUST_BINARY="/app/target/release/webxr"
else
    log "Skipping Rust rebuild (SKIP_RUST_REBUILD=true)"
    RUST_BINARY="/app/webxr"
fi

# Verify binary exists
if [ ! -f "${RUST_BINARY}" ]; then
    log "ERROR: Rust binary not found at ${RUST_BINARY}"
    exit 1
fi

log "Starting Rust backend from ${RUST_BINARY}..."
exec ${RUST_BINARY}