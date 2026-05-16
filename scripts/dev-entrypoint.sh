#!/bin/bash
set -e # Exit immediately if a command exits with a non-zero status.

# Set Docker environment variable to ensure PTX compilation at runtime
export DOCKER_ENV=1

# Add Docker group if socket is mounted
if [ -S /var/run/docker.sock ]; then
    DOCKER_GID=$(stat -c '%g' /var/run/docker.sock)
    groupadd -g $DOCKER_GID docker 2>/dev/null || true
    usermod -aG docker root 2>/dev/null || true
fi

# Configure log paths
LOGS_DIR="/app/logs"
RUST_LOG_FILE="${LOGS_DIR}/rust.log"
VITE_LOG_FILE="${LOGS_DIR}/vite.log"
NGINX_ACCESS_LOG="/var/log/nginx/access.log"
NGINX_ERROR_LOG="/var/log/nginx/error.log"
MAX_LOG_SIZE_MB=10 # Increased size for dev logs
MAX_LOG_FILES=3

# Create directories if they don't exist
mkdir -p "${LOGS_DIR}"
mkdir -p /var/log/nginx # Ensure nginx log dir exists (also done in Dockerfile)
touch ${NGINX_ACCESS_LOG} ${NGINX_ERROR_LOG} # Ensure log files exist

# Function to log messages with timestamps
log() {
    echo "[ENTRYPOINT][$(date '+%Y-%m-%d %H:%M:%S')] $1"
}

# Function to rotate logs (simplified for entrypoint)
rotate_log_file() {
    local log_file=$1
    if [ -f "${log_file}" ]; then
        local size_bytes=$(stat -c %s "${log_file}" 2>/dev/null || echo 0)
        local size_mb=$((size_bytes / 1024 / 1024))

        if [ "${size_mb}" -ge "${MAX_LOG_SIZE_MB}" ]; then
            log "Rotating log: ${log_file} (current size: ${size_mb}MB)"
            # Simple rotation: just move current to .1 and clear current
            mv "${log_file}" "${log_file}.1" 2>/dev/null || true
            touch "${log_file}"
        fi
    fi
}

# Cleanup function
cleanup() {
    log "Shutting down services..."
    # Send TERM signal to child processes
    if [ -n "${NGINX_PID:-}" ]; then kill -TERM $NGINX_PID 2>/dev/null || true; fi
    if [ -n "${RUST_PID:-}" ]; then kill -TERM $RUST_PID 2>/dev/null || true; fi
    if [ -n "${VITE_PID:-}" ]; then kill -TERM $VITE_PID 2>/dev/null || true; fi
    # Wait briefly for processes to terminate gracefully
    sleep 2
    # Force kill if still running
    if [ -n "${NGINX_PID:-}" ]; then kill -KILL $NGINX_PID 2>/dev/null || true; fi
    if [ -n "${RUST_PID:-}" ]; then kill -KILL $RUST_PID 2>/dev/null || true; fi
    if [ -n "${VITE_PID:-}" ]; then kill -KILL $VITE_PID 2>/dev/null || true; fi
    log "Cleanup complete."
    # exit 0 # Removed to allow script to continue for indefinite running
}

# Set up trap for cleanup on receiving signals
trap cleanup SIGTERM SIGINT SIGQUIT

# Start the Rust server in the background
start_rust_server() {
    log "Starting Rust server (logging to ${RUST_LOG_FILE})..."
    TARGET_PORT=${SYSTEM_NETWORK_PORT:-4000} # Use SYSTEM_NETWORK_PORT, default to 4000 if not set
    log "Attempting to free port ${TARGET_PORT} if in use..."
    log "Checking for processes on TCP port ${TARGET_PORT}..."
    if command -v lsof &> /dev/null; then
        log "lsof is available. Checking port ${TARGET_PORT} with lsof..."
        # Try to get PIDs, allow failure if no process is found
        LSOF_PIDS=$(lsof -t -i:${TARGET_PORT} || true)
        if [ -n "$LSOF_PIDS" ]; then
            # Replace newlines with spaces if multiple PIDs are found
            LSOF_PIDS_CLEANED=$(echo $LSOF_PIDS | tr '\n' ' ')
            log "Process(es) $LSOF_PIDS_CLEANED found on port ${TARGET_PORT} by lsof. Attempting to kill..."
            # Kill the PIDs, allow failure if they already exited or kill fails
            kill -9 $LSOF_PIDS_CLEANED || log "kill -9 $LSOF_PIDS_CLEANED (from lsof) failed. This might be okay."
            sleep 1 # Give a moment for the port to be released
        else
            log "No process found on port ${TARGET_PORT} with lsof."
        fi
    else
        log "lsof command not found. Skipping lsof check."
    fi

    if command -v fuser &> /dev/null; then
        log "fuser is available. Attempting to free port ${TARGET_PORT} with fuser..."
        fuser -k -TERM ${TARGET_PORT}/tcp || log "fuser -TERM ${TARGET_PORT}/tcp failed or no process found. This is okay."
        sleep 0.5 # Shorter sleep
        fuser -k -KILL ${TARGET_PORT}/tcp || log "fuser -KILL ${TARGET_PORT}/tcp failed or no process found. This is okay."
    else
        log "fuser command not found. Skipping fuser check. Port ${TARGET_PORT} might still be in use if Rust server fails to start."
    fi
    # ALWAYS rebuild Rust backend in dev mode to ensure code changes are applied
    # This is critical for development workflow
    if [ "${SKIP_RUST_REBUILD:-false}" != "true" ]; then
        log "Rebuilding Rust backend with GPU support (set SKIP_RUST_REBUILD=true to skip)..."
        cd /app
        
        # Build with GPU features - this ensures all code changes are compiled
        if cargo build --release --features gpu,dev-auth 2>&1 | tee -a "${RUST_LOG_FILE}"; then
            log "✓ Rust backend rebuilt successfully with latest code changes"
        else
            log "ERROR: Failed to rebuild Rust backend"
            log "Check ${RUST_LOG_FILE} for build errors"
            exit 1
        fi
        
        # The binary is built at target/release/webxr
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

    # Rotate log before starting
    rotate_log_file "${RUST_LOG_FILE}"
    # Start Rust server, redirect stdout/stderr to its log file
    ${RUST_BINARY} --gpu-debug > "${RUST_LOG_FILE}" 2>&1 &
    RUST_PID=$!
    log "Rust server started (PID: $RUST_PID)"
    # Basic check if process started
    sleep 2
    if ! kill -0 $RUST_PID 2>/dev/null; then
        log "ERROR: Rust server failed to start. Check ${RUST_LOG_FILE}."
        # exit 1 # Allow container to stay up for debugging even if Rust server fails
    fi
}

# Start Vite dev server in the background
start_vite() {
    cd /app/client
    log "Starting Vite dev server (logging to ${VITE_LOG_FILE})..."
    # Rotate log before starting
    rotate_log_file "${VITE_LOG_FILE}"
    # Start Vite, redirect stdout/stderr to its log file
    # Use --host 0.0.0.0 to ensure it's accessible within the container network
    FORCE_COLOR=1 npm run dev -- --host 0.0.0.0 --port 5173 > "${VITE_LOG_FILE}" 2>&1 &
    VITE_PID=$!
    log "Vite dev server started (PID: $VITE_PID)"
    # Basic check if process started
    sleep 5 # Vite can take a bit longer to spin up
    if ! kill -0 $VITE_PID 2>/dev/null; then
        log "ERROR: Vite server failed to start. Check ${VITE_LOG_FILE}."
        exit 1
    fi
    cd /app # Go back to app root
}

# Start Nginx in the foreground
start_nginx() {
    log "Starting Nginx..."
    # Ensure Nginx config is valid before starting
    nginx -t
    # Start Nginx in foreground mode
    nginx -g 'daemon off;' &
    NGINX_PID=$!
    log "Nginx started (PID: $NGINX_PID)"
    log "Development environment accessible at http://localhost:3001"
}

# --- Main Execution ---
log "Starting development environment services..."

# Check if we should use supervisord
if [ -f /etc/supervisor/conf.d/services.conf ] || [ -f /app/supervisord.dev.conf ]; then
    log "Starting services with supervisord..."

    # Create necessary directories
    mkdir -p /app/logs /tmp

    # Use custom config if available
    if [ -f /app/supervisord.dev.conf ]; then
        exec supervisord -c /app/supervisord.dev.conf
    else
        exec supervisord -n
    fi
else
    # Fallback to original startup method
    log "Starting services individually..."
    start_rust_server
    start_vite
    start_nginx

    # Wait for Nginx (foreground process) to exit
    wait $NGINX_PID

    # If Nginx exits, trigger cleanup
    log "Nginx process ended. Initiating cleanup..."
    cleanup

    log "Entrypoint script will now sleep indefinitely to keep container alive for debugging."
    sleep infinity
fi
