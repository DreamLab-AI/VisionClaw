#!/bin/bash
# VisionFlow Unified Launch Script - Simple, unified launcher for all environments
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Script configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
COMPOSE_FILE="$PROJECT_ROOT/docker-compose.unified.yml"
CONTAINER_NAME="visionflow_container"
AGENT_CONTAINER="agentic-workstation"
AGENT_COMPOSE_FILE="$PROJECT_ROOT/multi-agent-docker/docker-compose.unified.yml"

# Default values
COMMAND="${1:-up}"
ENVIRONMENT="${2:-dev}"
WITH_AGENT=false

# Check for --with-agent flag in any position
for arg in "$@"; do
    if [[ "$arg" == "--with-agent" ]]; then
        WITH_AGENT=true
    fi
done

# Adjust ENVIRONMENT if it was set to --with-agent
if [[ "$ENVIRONMENT" == "--with-agent" ]]; then
    ENVIRONMENT="dev"
fi

# Logging functions
log() {
    echo -e "${CYAN}[$(date '+%H:%M:%S')]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

# Show help
show_help() {
    cat << EOF
${GREEN}╔════════════════════════════════════════════════════════════╗
║         VisionFlow Unified Launch Script                   ║
╚════════════════════════════════════════════════════════════╝${NC}

${YELLOW}Usage:${NC}
    ./launch.sh [COMMAND] [ENVIRONMENT]

${YELLOW}Commands:${NC}
    ${GREEN}up${NC}             Start the environment (auto-detects changes, fast)
    ${GREEN}down${NC}           Stop and remove containers
    ${GREEN}build${NC}          Build containers (with layer cache)
    ${GREEN}rebuild${NC}        Full rebuild (no cache, cleans all cargo volumes)
    ${GREEN}rebuild-agent${NC}  Rebuild agentic-workstation (full GPU/ComfyUI/CachyOS validation)
                     Options: --skip-comfyui, --comfyui-full, --skip-cachyos
    ${GREEN}logs${NC}           Show container logs (follow mode)
    ${GREEN}shell${NC}          Open interactive shell in container
    ${GREEN}restart${NC}        Restart the environment
    ${GREEN}restart-agent${NC}  Restart the agentic-workstation container
    ${GREEN}status${NC}         Show container status and URLs
    ${GREEN}clean${NC}          Clean all containers, volumes, and images

${YELLOW}Environments:${NC}
    ${GREEN}dev${NC}        Development environment (default)
                - BUILD_TARGET=development
                - Verbose logging enabled
                - Hot reload enabled
                - No restart policy

    ${GREEN}prod${NC}       Production environment
                - BUILD_TARGET=production
                - Minimal logging
                - Restart policy: unless-stopped
                - Optimized builds

${YELLOW}Flags:${NC}
    ${GREEN}--with-agent${NC}   Also restart the agentic-workstation container

${YELLOW}Examples:${NC}
    ./launch.sh                    ${CYAN}# Start dev environment${NC}
    ./launch.sh up dev             ${CYAN}# Start dev environment${NC}
    ./launch.sh up dev --with-agent ${CYAN}# Start dev + restart agent${NC}
    ./launch.sh build prod         ${CYAN}# Build production${NC}
    ./launch.sh rebuild prod       ${CYAN}# Rebuild production (no cache)${NC}
    ./launch.sh logs dev           ${CYAN}# View dev logs${NC}
    ./launch.sh shell prod         ${CYAN}# Open prod shell${NC}
    ./launch.sh restart dev        ${CYAN}# Restart dev${NC}
    ./launch.sh restart-agent      ${CYAN}# Restart agentic-workstation${NC}
    ./launch.sh rebuild-agent      ${CYAN}# Full rebuild with GPU/ComfyUI/CachyOS${NC}
    ./launch.sh rebuild-agent --skip-comfyui  ${CYAN}# Skip ComfyUI check${NC}
    ./launch.sh rebuild-agent --comfyui-full  ${CYAN}# Build full open3d (30-60 min)${NC}
    ./launch.sh clean              ${CYAN}# Clean everything${NC}

${YELLOW}Environment Files:${NC}
    .env.dev       Development configuration
    .env.prod      Production configuration

${YELLOW}GPU Support:${NC}
    Automatically detected via nvidia-smi
    Enabled in containers when GPU is available

EOF
}

# Validate command
validate_command() {
    case "$COMMAND" in
        up|down|build|rebuild|rebuild-agent|logs|shell|restart|restart-agent|status|clean|help|-h|--help)
            if [[ "$COMMAND" == "help" ]] || [[ "$COMMAND" == "-h" ]] || [[ "$COMMAND" == "--help" ]]; then
                show_help
                exit 0
            fi
            ;;
        *)
            error "Invalid command: $COMMAND"
            echo ""
            show_help
            exit 1
            ;;
    esac
}

# Validate environment
validate_environment() {
    case "$ENVIRONMENT" in
        dev|prod)
            ;;
        *)
            error "Invalid environment: $ENVIRONMENT"
            echo "Valid environments: dev, prod"
            exit 1
            ;;
    esac
}

# Load environment-specific configuration
load_env_config() {
    local env_file="$PROJECT_ROOT/.env.$ENVIRONMENT"

    if [[ -f "$env_file" ]]; then
        success "Loading environment config: .env.$ENVIRONMENT"
        set -a
        source "$env_file"
        set +a
    else
        warning "Environment file not found: $env_file"
        if [[ -f "$PROJECT_ROOT/.env" ]]; then
            info "Using default .env file"
            set -a
            source "$PROJECT_ROOT/.env"
            set +a
        else
            error "No .env file found. Please create .env.$ENVIRONMENT or .env"
            exit 1
        fi
    fi
}

# Set environment-specific variables
set_environment_vars() {
    case "$ENVIRONMENT" in
        dev)
            export BUILD_TARGET="development"
            export COMPOSE_PROFILES="dev"
            export LOG_LEVEL="debug"
            export RESTART_POLICY="no"
            info "Environment: Development"
            info "  - Verbose logging enabled"
            info "  - Hot reload enabled"
            info "  - No restart policy"
            ;;
        prod)
            export BUILD_TARGET="production"
            export COMPOSE_PROFILES="prod"
            export LOG_LEVEL="info"
            export RESTART_POLICY="unless-stopped"
            info "Environment: Production"
            info "  - Minimal logging"
            info "  - Optimized builds"
            info "  - Restart policy: unless-stopped"
            ;;
    esac
}

# Check prerequisites
check_prerequisites() {
    log "Checking prerequisites..."

    # Check Docker
    if ! command -v docker &> /dev/null; then
        error "Docker is not installed. Please install Docker first."
        exit 1
    fi
    success "Docker: $(docker --version)"

    # Check Docker Compose
    if ! docker compose version &> /dev/null; then
        error "Docker Compose is not installed. Please install Docker Compose first."
        exit 1
    fi
    success "Docker Compose: $(docker compose version)"

    # Check compose file
    if [[ ! -f "$COMPOSE_FILE" ]]; then
        error "Docker Compose file not found: $COMPOSE_FILE"
        exit 1
    fi
    success "Compose file: docker-compose.unified.yml"

    success "Prerequisites check complete"
}

# Detect Docker-in-Docker and compute host paths for bind mounts
detect_dind() {
    if [ -f /.dockerenv ] || grep -qsm1 'docker\|containerd' /proc/1/cgroup 2>/dev/null; then
        info "Docker-in-Docker detected — resolving host paths for bind mounts"
        # Find our container's name by matching hostname or known container name
        local my_container="${HOSTNAME:-}"
        if [ -z "$my_container" ] || ! docker inspect "$my_container" &>/dev/null; then
            # Fallback: find container whose workspace mounts match our path
            my_container=$(docker ps --format '{{.Names}}' | while read -r name; do
                docker inspect "$name" --format '{{range .Mounts}}{{if eq .Destination "/home/devuser/workspace"}}{{$.Name}}{{end}}{{end}}' 2>/dev/null
            done | head -1)
        fi
        if [ -n "$my_container" ]; then
            # Check if the project directory has its own separate mount (e.g. mldata)
            local host_project
            host_project=$(docker inspect "$my_container" --format '{{range .Mounts}}{{if eq .Destination "/home/devuser/workspace/project"}}{{.Source}}{{end}}{{end}}' 2>/dev/null)
            if [ -n "$host_project" ]; then
                export HOST_PROJECT_ROOT="$host_project"
                success "Host project root (dedicated mount): $HOST_PROJECT_ROOT"
                return 0
            fi
            # Fallback: project is inside the workspace volume
            local host_workspace
            host_workspace=$(docker inspect "$my_container" --format '{{range .Mounts}}{{if eq .Destination "/home/devuser/workspace"}}{{.Source}}{{end}}{{end}}' 2>/dev/null)
            if [ -n "$host_workspace" ]; then
                export HOST_PROJECT_ROOT="${host_workspace}/project"
                success "Host project root (workspace volume): $HOST_PROJECT_ROOT"
                return 0
            fi
        fi
        warning "Could not determine host path — bind mounts may fail"
    fi
    # Not DinD or detection failed: use relative paths (compose default)
    export HOST_PROJECT_ROOT="."
}

# Detect and validate GPU
detect_gpu() {
    log "Detecting GPU..."

    if command -v nvidia-smi &> /dev/null; then
        GPU_INFO=$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -n1 || true)
        if [[ -n "$GPU_INFO" ]]; then
            success "GPU detected: $GPU_INFO"
            export GPU_AVAILABLE="true"
            export NVIDIA_RUNTIME="nvidia"

            # Check NVIDIA Docker runtime
            if docker info 2>/dev/null | grep -q nvidia; then
                success "NVIDIA Docker runtime: Available"
            else
                warning "NVIDIA Docker runtime not found in Docker info"
                info "Install nvidia-container-toolkit for GPU passthrough"
            fi
        else
            warning "NVIDIA GPU not detected"
            export GPU_AVAILABLE="false"
        fi
    else
        warning "nvidia-smi not found - GPU support disabled"
        export GPU_AVAILABLE="false"
    fi
}

# Docker Compose wrapper
docker_compose() {
    cd "$PROJECT_ROOT"
    docker compose -f "$COMPOSE_FILE" --profile "$COMPOSE_PROFILES" "$@"
}

# Clean up conflicting containers and resources
cleanup_conflicts() {
    log "Checking for conflicting containers and resources..."

    # Stop and remove any containers with conflicting names
    local conflicting_containers=(
        "visionflow-neo4j"
        "visionflow_container"
        "visionflow-backend"
        "visionflow-frontend"
        "visionflow-cloudflared"
    )

    for container in "${conflicting_containers[@]}"; do
        if docker ps -a --format '{{.Names}}' | grep -q "^${container}$"; then
            warning "Removing conflicting container: $container"
            docker rm -f "$container" 2>/dev/null || true
        fi
    done

    # Remove orphan containers from previous runs
    cd "$PROJECT_ROOT"
    docker compose -f "$COMPOSE_FILE" down --remove-orphans 2>/dev/null || true

    success "Conflict cleanup complete"
}

# Build containers
build_containers() {
    log "Building containers for $ENVIRONMENT environment..."

    local build_args=()

    if [[ "$COMMAND" == "rebuild" ]]; then
        info "Rebuild mode: Using --no-cache"
        build_args+=("--no-cache")
    fi

    # Only bust cache on explicit rebuild. Normal builds rely on Docker's content-
    # addressable layer cache. Without this guard, CACHE_BUST invalidates the
    # runtime stage's pacman/CUDA layers (~2.2 GB download) on every single build.
    if [[ "$COMMAND" == "rebuild" ]]; then
        build_args+=("--build-arg" "CACHE_BUST=$(date +%s)")
    fi

    # Enable GPU and ontology features by default for both dev and prod
    # These are the core VisionFlow features required for full functionality.
    #
    # ADR-06 §D1 + resolution T2: `dev-auth` Cargo feature is added ONLY for
    # dev environment. Release/production builds MUST NOT include it — the
    # presence of `dev-auth` causes auth-bypass code to be compiled into the
    # binary (Bearer dev-session-token acceptance, ALLOW_INSECURE_DEFAULTS
    # honour, --allow-skip-auth argv flag).
    local feature_list=""
    if [[ "${GPU_AVAILABLE:-false}" == "true" ]]; then
        feature_list="gpu,ontology"
    else
        feature_list="ontology"
    fi
    if [[ "$ENVIRONMENT" == "dev" ]]; then
        feature_list="${feature_list},dev-auth"
        info "Building with features: ${feature_list} (dev-auth ENABLED — bypass code present)"
    else
        info "Building with features: ${feature_list} (dev-auth DISABLED — bypass code absent from binary)"
    fi
    build_args+=("--build-arg" "FEATURES=${feature_list}")

    docker_compose build "${build_args[@]}"
    success "Build complete for $ENVIRONMENT environment"
}

# Cleanup handler for dev environment
cleanup_dev() {
    echo ""
    warning "Caught interrupt signal - cleaning up dev environment..."
    log "Stopping and removing dev containers..."
    docker_compose down --remove-orphans
    success "Dev environment cleaned up"
    exit 0
}

# Derive the Docker Compose image name for the visionflow service
get_image_name() {
    # Docker Compose names images as <project>-<service>
    # Project name defaults to the directory name of the compose file
    local project_dir
    project_dir="$(basename "$PROJECT_ROOT")"
    # Also check COMPOSE_PROJECT_NAME override
    local project_name="${COMPOSE_PROJECT_NAME:-$project_dir}"
    echo "${project_name}-visionflow"
}

# Remove stale cargo TARGET cache only — preserves registry/git downloads
clean_cargo_target() {
    log "Removing stale cargo target cache volume..."
    docker volume rm "${CARGO_TARGET_CACHE_VOLUME:-visionflow-cargo-target-cache}" 2>/dev/null || true
    success "Cargo target cache cleaned (registry/git downloads preserved)"
}

# Remove ALL cargo cache volumes (for full rebuild only)
clean_cargo_volumes() {
    log "Removing all cargo cache volumes..."
    docker volume rm "${CARGO_TARGET_CACHE_VOLUME:-visionflow-cargo-target-cache}" 2>/dev/null || true
    docker volume rm "${CARGO_CACHE_VOLUME:-visionflow-cargo-cache}" 2>/dev/null || true
    docker volume rm "${CARGO_GIT_CACHE_VOLUME:-visionflow-cargo-git-cache}" 2>/dev/null || true
    success "All cargo cache volumes cleaned"
}

# Check if Docker IMAGE rebuild is needed (Dockerfile/dependency changes)
# Source-only changes DON'T need image rebuild — source is volume-mounted in dev
needs_image_rebuild() {
    local image_name
    image_name="$(get_image_name)"

    # No image at all — must build
    if ! docker images --format "{{.Repository}}" | grep -q "^${image_name}$"; then
        echo "true"
        return 0
    fi

    # Get image creation time
    local image_created=$(docker images --format "{{.CreatedAt}}" "$image_name" 2>/dev/null | head -1)
    if [[ -z "$image_created" ]]; then
        echo "true"
        return 0
    fi

    local image_epoch=$(date -d "$image_created" +%s 2>/dev/null || echo 0)

    # Only check files that affect the IMAGE (not source — that's volume-mounted)
    local image_files=(
        "$PROJECT_ROOT/Dockerfile.unified"
        "$PROJECT_ROOT/Dockerfile.production"
        "$PROJECT_ROOT/Dockerfile.dev"
        "$PROJECT_ROOT/Cargo.toml"
        "$PROJECT_ROOT/Cargo.lock"
        "$PROJECT_ROOT/client/package.json"
        "$PROJECT_ROOT/client/package-lock.json"
        "$PROJECT_ROOT/supervisord.dev.conf"
        "$PROJECT_ROOT/nginx.dev.conf"
        "$PROJECT_ROOT/nginx.production.conf"
        "$PROJECT_ROOT/scripts/dev-entrypoint.sh"
        "$PROJECT_ROOT/scripts/rust-backend-wrapper.sh"
        "$PROJECT_ROOT/scripts/production-startup.sh"
    )

    for file in "${image_files[@]}"; do
        if [[ -f "$file" ]]; then
            local file_epoch=$(stat -c %Y "$file" 2>/dev/null || echo 0)
            if [[ $file_epoch -gt $image_epoch ]]; then
                echo "true"
                return 0
            fi
        fi
    done

    echo "false"
    return 1
}

# Check if source code changed (needs container restart to trigger recompile)
needs_recompile() {
    local container_name="$1"

    # If container isn't running, recompile is implicit on startup
    if ! docker ps --format '{{.Names}}' | grep -q "^${container_name}$"; then
        echo "false"
        return 1
    fi

    # Get container start time
    local container_started=$(docker inspect --format='{{.State.StartedAt}}' "$container_name" 2>/dev/null)
    if [[ -z "$container_started" ]]; then
        echo "true"
        return 0
    fi
    local container_epoch=$(date -d "$container_started" +%s 2>/dev/null || echo 0)

    # Check Rust source files
    local latest_rs=$(find "$PROJECT_ROOT/src" -name "*.rs" -printf '%T@\n' 2>/dev/null | sort -n | tail -1 | cut -d. -f1)
    latest_rs=${latest_rs:-0}

    # Check client source files
    local latest_ts=$(find "$PROJECT_ROOT/client/src" \( -name "*.ts" -o -name "*.tsx" \) -printf '%T@\n' 2>/dev/null | sort -n | tail -1 | cut -d. -f1)
    latest_ts=${latest_ts:-0}

    # Check build.rs
    local build_rs_epoch=$(stat -c %Y "$PROJECT_ROOT/build.rs" 2>/dev/null || echo 0)

    local latest_source=$latest_rs
    [[ $latest_ts -gt $latest_source ]] && latest_source=$latest_ts
    [[ $build_rs_epoch -gt $latest_source ]] && latest_source=$build_rs_epoch

    if [[ $latest_source -gt $container_epoch ]]; then
        echo "true"
        return 0
    fi

    echo "false"
    return 1
}

# Check if container is already running and healthy
is_container_running() {
    local container_name="$1"
    if docker ps --format '{{.Names}}' | grep -q "^${container_name}$"; then
        # Check if container is healthy (or has no health check)
        local health=$(docker inspect --format='{{.State.Health.Status}}' "$container_name" 2>/dev/null || echo "none")
        if [[ "$health" == "healthy" ]] || [[ "$health" == "none" ]]; then
            return 0
        fi
    fi
    return 1
}

# Start environment
start_environment() {
    log "Starting $ENVIRONMENT environment..."

    # Check if main container is already running and healthy
    if is_container_running "$CONTAINER_NAME"; then
        local source_changed=$(needs_recompile "$CONTAINER_NAME")
        if [[ "$source_changed" == "true" ]]; then
            warning "Source code changes detected — restarting container to recompile..."
            # Source is volume-mounted, so just restart. The wrapper rebuilds on startup.
            # Clean target cache to avoid stale incremental artifacts.
            docker_compose stop visionflow
            clean_cargo_target
            docker_compose start visionflow
            sleep 3
        else
            success "Container $CONTAINER_NAME is already running and healthy (no source changes)"
        fi

        echo ""
        show_service_urls
        echo ""
        info "Following logs... (Press Ctrl+C to exit)"
        echo ""

        # Set up cleanup trap for dev environment
        if [[ "$ENVIRONMENT" == "dev" ]]; then
            trap cleanup_dev INT TERM
        fi

        docker_compose logs -f
        return 0
    fi

    # Clean up any conflicting containers first
    cleanup_conflicts

    # Check if IMAGE rebuild is needed (Dockerfile/deps changed)
    local image_rebuild=$(needs_image_rebuild)

    if [[ "$image_rebuild" == "true" ]]; then
        warning "Image-level changes detected (Dockerfile/deps) — rebuilding image..."
        # Only clean target cache, preserve registry downloads for speed
        clean_cargo_target
        build_containers
    elif ! docker images | grep -q "visionflow"; then
        info "Container images not found. Building first..."
        build_containers
    else
        success "Using existing container image (source is volume-mounted, no image rebuild needed)"
    fi

    # Conditionally start cloudflared based on environment
    if [[ "$ENVIRONMENT" == "dev" ]]; then
        info "Development mode: Skipping cloudflared tunnel (local access only)"
        docker_compose up -d --remove-orphans --scale cloudflared=0

        # Wait for containers to be ready
        sleep 3

        success "Environment started in background"
        echo ""
        show_service_urls
        echo ""
        info "Following logs... (Press Ctrl+C to stop and cleanup)"
        echo ""

        # Set up cleanup trap for dev environment
        trap cleanup_dev INT TERM

        # Show logs and keep running
        docker_compose logs -f
    else
        info "Production mode: Starting cloudflared tunnel"
        docker_compose up -d --remove-orphans

        # Wait for containers to be ready
        sleep 3

        success "Environment started in background"
        echo ""
        show_service_urls
        echo ""
        info "View logs with: ${GREEN}./launch.sh logs $ENVIRONMENT${NC}"
        info "Stop with: ${GREEN}./launch.sh down $ENVIRONMENT${NC}"
    fi
}

# Stop environment
stop_environment() {
    log "Stopping $ENVIRONMENT environment..."
    docker_compose down --remove-orphans
    success "Environment stopped"
}

# Restart environment
restart_environment() {
    log "Restarting $ENVIRONMENT environment..."
    stop_environment
    sleep 2
    start_environment
}

# Restart agent container (agentic-workstation)
restart_agent_container() {
    log "Restarting agentic-workstation container..."

    # Check if agent compose file exists
    if [[ ! -f "$AGENT_COMPOSE_FILE" ]]; then
        error "Agent compose file not found: $AGENT_COMPOSE_FILE"
        exit 1
    fi

    # Check if container is running
    if docker ps --format '{{.Names}}' | grep -q "^${AGENT_CONTAINER}$"; then
        info "Stopping $AGENT_CONTAINER..."
        docker stop "$AGENT_CONTAINER"
        docker rm "$AGENT_CONTAINER"
    else
        warning "Container $AGENT_CONTAINER is not running"
    fi

    # Ensure comfyui container is reachable from docker_ragflow network
    # (comfyui may live on a different network; connect it at runtime without build changes)
    local RAGFLOW_NET="${EXTERNAL_NETWORK:-docker_ragflow}"
    if docker ps --format '{{.Names}}' | grep -q "^comfyui$"; then
        if ! docker inspect comfyui --format '{{range $k,$v := .NetworkSettings.Networks}}{{$k}} {{end}}' 2>/dev/null | grep -q "$RAGFLOW_NET"; then
            info "Connecting comfyui to $RAGFLOW_NET for agentic-workstation access..."
            docker network connect "$RAGFLOW_NET" comfyui && \
                success "comfyui connected to $RAGFLOW_NET (reachable at comfyui:3000 enhanced API, comfyui:8188 standard)" || \
                warning "Could not connect comfyui to $RAGFLOW_NET (container will use host.docker.internal fallback)"
        else
            success "comfyui already on $RAGFLOW_NET"
        fi
    else
        warning "comfyui container not running - ComfyUI integration will be unavailable"
    fi

    # Start the agent container
    info "Starting $AGENT_CONTAINER..."
    cd "$PROJECT_ROOT/multi-agent-docker"

    # Load .env from multi-agent-docker
    if [[ -f ".env" ]]; then
        set -a
        source .env
        set +a
    fi

    docker compose -f docker-compose.unified.yml up -d

    # Wait for container to start
    sleep 3

    # Check if container started successfully
    if docker ps --format '{{.Names}}' | grep -q "^${AGENT_CONTAINER}$"; then
        success "Container $AGENT_CONTAINER restarted successfully"
        echo ""
        info "Services available:"
        echo "  ${GREEN}SSH:${NC}            ssh devuser@localhost -p 2222"
        echo "  ${GREEN}VNC:${NC}            localhost:5901"
        echo "  ${GREEN}code-server:${NC}    http://localhost:8080"
        echo "  ${GREEN}Management API:${NC} http://localhost:9090"
        echo "  ${GREEN}ComfyUI API:${NC}    http://comfyui:3000 (enhanced) | http://comfyui:8188 (standard)"
        echo ""
        info "View logs with: docker logs -f $AGENT_CONTAINER"
    else
        error "Failed to start $AGENT_CONTAINER"
        docker compose -f docker-compose.unified.yml logs --tail=50
        exit 1
    fi
}

# Rebuild agent container (agentic-workstation) with no cache
# Canonical build with GPU verification, ComfyUI, CachyOS builds, and skills validation
rebuild_agent_container() {
    local SKIP_COMFYUI=false
    local BUILD_COMFYUI_FULL=false
    local SKIP_CACHYOS=false

    # Parse additional flags
    for arg in "$@"; do
        case "$arg" in
            --skip-comfyui) SKIP_COMFYUI=true ;;
            --comfyui-full) BUILD_COMFYUI_FULL=true ;;
            --skip-cachyos) SKIP_CACHYOS=true ;;
        esac
    done

    echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║   AGENTIC WORKSTATION v3.0.0 - Canonical Build System            ║${NC}"
    echo -e "${GREEN}║   Claude Flow V3 | 62+ Skills | Multi-Agent Orchestration        ║${NC}"
    echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════╝${NC}"
    echo ""

    # Check if agent compose file exists
    if [[ ! -f "$AGENT_COMPOSE_FILE" ]]; then
        error "Agent compose file not found: $AGENT_COMPOSE_FILE"
        exit 1
    fi

    # Change to multi-agent-docker directory
    cd "$PROJECT_ROOT/multi-agent-docker"

    # Check for .env file
    if [[ ! -f .env ]]; then
        warning ".env file not found"
        if [[ -f .env.example ]]; then
            info "Creating from .env.example..."
            cp .env.example .env
            success "Created .env from template"
            warning "IMPORTANT: Edit .env and add your API keys before continuing!"
            read -p "Press Enter to continue (or Ctrl+C to exit and edit .env)..."
        else
            error ".env.example not found"
            exit 1
        fi
    fi

    # Load .env
    set -a
    source .env
    set +a

    # Verify skills exist
    log "Verifying skills..."
    local skills_dir="skills"
    if [[ -d "$skills_dir/docker-manager" ]]; then
        success "Docker Manager skill found"
    else
        warning "Docker Manager skill not found"
    fi
    if [[ -d "$skills_dir/chrome-devtools" ]]; then
        success "Chrome DevTools skill found"
    else
        warning "Chrome DevTools skill not found"
    fi
    echo ""

    # Check/create ragflow network
    log "Checking docker_ragflow network..."
    if ! docker network inspect docker_ragflow >/dev/null 2>&1; then
        info "Creating docker_ragflow network..."
        docker network create docker_ragflow
        success "Network created"
    else
        success "Network exists"
    fi
    echo ""

    # Stop and remove existing container
    if docker ps -a --format '{{.Names}}' | grep -q "^${AGENT_CONTAINER}$"; then
        info "Stopping and removing $AGENT_CONTAINER..."
        docker stop "$AGENT_CONTAINER" 2>/dev/null || true
        docker rm "$AGENT_CONTAINER" 2>/dev/null || true
    fi

    # Build with no cache
    log "[1/4] Building Agentic Workstation Docker image..."
    export DOCKER_BUILDKIT=1
    export COMPOSE_DOCKER_CLI_BUILD=1
    docker compose -f docker-compose.unified.yml build --no-cache

    # Start the container
    log "[2/4] Launching Agentic Workstation..."
    docker compose -f docker-compose.unified.yml up -d

    log "[3/4] Waiting for services to start..."
    sleep 10

    # Check services
    echo ""
    log "Service Status:"
    docker exec "$AGENT_CONTAINER" /opt/venv/bin/supervisorctl status 2>/dev/null || warning "Could not get service status"

    echo ""
    echo "========================================"
    echo "  GPU VERIFICATION"
    echo "========================================"
    echo ""

    # Test GPU access
    log "Testing NVIDIA GPU access..."
    docker exec "$AGENT_CONTAINER" nvidia-smi --query-gpu=name,memory.total --format=csv,noheader 2>/dev/null || \
        warning "GPU not accessible - check NVIDIA runtime configuration"

    echo ""
    log "Testing PyTorch CUDA..."
    docker exec "$AGENT_CONTAINER" /opt/venv/bin/python3 -c "
import torch
print(f'PyTorch version: {torch.__version__}')
print(f'CUDA available: {torch.cuda.is_available()}')
if torch.cuda.is_available():
    print(f'CUDA version: {torch.version.cuda}')
    print(f'GPU count: {torch.cuda.device_count()}')
    for i in range(torch.cuda.device_count()):
        print(f'  GPU {i}: {torch.cuda.get_device_name(i)}')
else:
    print('WARNING: PyTorch cannot access CUDA')
    print('   Image generation will be CPU-only and very slow')
" 2>/dev/null || warning "PyTorch test failed"

    echo ""
    log "Testing ComfyUI installation..."
    if docker exec "$AGENT_CONTAINER" test -d /home/devuser/ComfyUI 2>/dev/null; then
        success "ComfyUI installed at /home/devuser/ComfyUI"
        if docker exec "$AGENT_CONTAINER" test -f /home/devuser/ComfyUI/models/checkpoints/flux1-schnell-fp8.safetensors 2>/dev/null; then
            success "FLUX model downloaded"
        else
            warning "FLUX model not found - will download on first use"
        fi
    else
        warning "ComfyUI not installed"
    fi

    # ComfyUI deployment
    echo ""
    echo "========================================"
    echo "  COMFYUI STANDALONE DEPLOYMENT"
    echo "========================================"
    echo ""

    if [[ "$SKIP_COMFYUI" == "true" ]]; then
        info "Skipping standalone ComfyUI deployment (--skip-comfyui flag)"
    elif [[ "$BUILD_COMFYUI_FULL" == "true" ]]; then
        log "[4/4] Building ComfyUI with full open3d support..."
        warning "This will take 30-60 minutes!"
        if [[ -f "comfyui/build-comfyui.sh" ]]; then
            cd comfyui && ./build-comfyui.sh && cd ..
        else
            warning "comfyui/build-comfyui.sh not found, skipping"
        fi
    else
        log "[4/4] Checking ComfyUI standalone container..."
        if docker ps -a | grep -q "^comfyui"; then
            if docker exec comfyui python3 -c "import open3d; print(open3d.__version__)" 2>/dev/null | grep -q "stub"; then
                success "ComfyUI already running with open3d stub"
                info "To rebuild with full open3d: ./scripts/launch.sh rebuild-agent --comfyui-full"
            else
                info "ComfyUI running (open3d status unknown)"
            fi
        else
            info "Standalone ComfyUI container not found"
            info "To deploy: cd multi-agent-docker/comfyui && ./build-comfyui.sh"
        fi
    fi

    # Build VisionFlow CachyOS containers
    if [[ "$SKIP_CACHYOS" != "true" ]]; then
        echo ""
        echo "========================================"
        echo "  VISIONFLOW CACHYOS BUILD"
        echo "========================================"
        echo ""
        log "Building CachyOS-aligned VisionFlow containers..."

        if [[ -f "comfyui/Dockerfile.cachyos" ]]; then
            log "[VF-1/2] Building ComfyUI (CachyOS aligned)..."
            docker build -f comfyui/Dockerfile.cachyos -t comfyui-cachyos:latest . 2>/dev/null || warning "ComfyUI CachyOS build failed"
        else
            warning "comfyui/Dockerfile.cachyos not found, skipping"
        fi

        if [[ -f "claude-zai/Dockerfile.cachyos" ]]; then
            log "[VF-2/2] Building Claude-ZAI (CachyOS aligned)..."
            docker build -f claude-zai/Dockerfile.cachyos -t claude-zai-cachyos:latest . 2>/dev/null || warning "Claude-ZAI CachyOS build failed"
        else
            warning "claude-zai/Dockerfile.cachyos not found, skipping"
        fi

        success "VisionFlow CachyOS builds complete"
        echo ""
        info "CUDA Compatibility (all containers):"
        echo "  - CUDA Path: /opt/cuda"
        echo "  - CUDA Version: 13.1"
        echo "  - PTX Binary: /opt/cuda/bin/ptxas"
        echo "  - Libraries: /opt/cuda/lib64"
    fi

    echo ""
    echo "========================================"
    echo "  DEPLOYMENT COMPLETE"
    echo "========================================"
    echo ""

    # Verify skills installation in container
    log "Verifying skills installation..."
    if docker exec "$AGENT_CONTAINER" test -f /home/devuser/.claude/skills/docker-manager/SKILL.md 2>/dev/null; then
        success "Docker Manager skill installed"
    else
        warning "Docker Manager skill not found in container"
    fi
    if docker exec "$AGENT_CONTAINER" test -f /home/devuser/.claude/skills/chrome-devtools/SKILL.md 2>/dev/null; then
        success "Chrome DevTools skill installed"
    else
        warning "Chrome DevTools skill not found in container"
    fi

    # Verify Docker socket
    if docker exec "$AGENT_CONTAINER" test -S /var/run/docker.sock 2>/dev/null; then
        success "Docker socket mounted"
    else
        warning "Docker socket not found - Docker Manager will not work"
    fi

    echo ""
    echo "========================================"
    echo "  ACCESS INFORMATION"
    echo "========================================"
    echo ""
    echo -e "${GREEN}Agentic Workstation:${NC}"
    echo "  SSH:         ssh -p 2222 devuser@localhost  (password: turboflow)"
    echo "  VNC:         vnc://localhost:5901           (password: turboflow)"
    echo "  code-server: http://localhost:8080"
    echo "  API:         http://localhost:9090/health"
    echo "  Swagger:     http://localhost:9090/documentation"
    echo ""

    # Check ComfyUI standalone status
    if docker ps | grep -q "comfyui"; then
        echo -e "${GREEN}ComfyUI Standalone:${NC}"
        echo "  Web UI:      http://localhost:8188"
        echo "  Container:   comfyui"
        local open3d_ver=$(docker exec comfyui python3 -c "import open3d; print(open3d.__version__)" 2>/dev/null || echo "not installed")
        echo "  open3d:      $open3d_ver"
        echo ""
    fi

    echo -e "${GREEN}Management Commands:${NC}"
    echo "  View logs:   docker compose -f multi-agent-docker/docker-compose.unified.yml logs -f"
    echo "  Stop:        docker compose -f multi-agent-docker/docker-compose.unified.yml down"
    echo "  Shell:       docker exec -it $AGENT_CONTAINER zsh"
    echo ""
    echo -e "${GREEN}Build Options:${NC}"
    echo "  ./scripts/launch.sh rebuild-agent              # Standard build"
    echo "  ./scripts/launch.sh rebuild-agent --skip-comfyui  # Skip ComfyUI check"
    echo "  ./scripts/launch.sh rebuild-agent --comfyui-full  # Full open3d (30-60 min)"
    echo "  ./scripts/launch.sh rebuild-agent --skip-cachyos  # Skip CachyOS builds"
    echo ""
    echo "All containers use CachyOS v3 base for binary compatibility"
    echo "  CUDA: /opt/cuda (v13.1) | PTX: /opt/cuda/bin/ptxas"
    echo ""
}

# Show logs
show_logs() {
    log "Showing logs for $ENVIRONMENT environment..."
    info "Press Ctrl+C to exit log view"
    echo ""
    docker_compose logs -f
}

# Open shell
open_shell() {
    log "Opening interactive shell in $ENVIRONMENT container..."

    if ! docker ps | grep -q "$CONTAINER_NAME"; then
        error "Container is not running. Start it first with: ./launch.sh up $ENVIRONMENT"
        exit 1
    fi

    info "Entering container shell..."
    docker exec -it "$CONTAINER_NAME" /bin/bash || docker exec -it "$CONTAINER_NAME" /bin/sh
}

# Show status
show_status() {
    log "Container status for $ENVIRONMENT environment:"
    echo ""
    docker_compose ps
    echo ""

    if docker ps -q -f name="$CONTAINER_NAME" &> /dev/null; then
        show_service_urls

        # Show resource usage
        echo ""
        log "Resource usage:"
        docker stats --no-stream --format "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}" | grep "$CONTAINER_NAME" || true
    else
        warning "Container is not running"
        info "Start with: ${GREEN}./launch.sh up $ENVIRONMENT${NC}"
    fi
}

# Show service URLs
show_service_urls() {
    log "Service URLs:"

    if [[ "$ENVIRONMENT" == "dev" ]]; then
        echo "  ${GREEN}Vite Dev:${NC}      http://localhost:3001"
        echo "  ${GREEN}Web UI:${NC}        http://localhost:4000 (→ 3001)"
    else
        echo "  ${GREEN}Web UI:${NC}        http://localhost:4000"
    fi

    echo "  ${GREEN}WebSocket:${NC}     ws://localhost:4000/ws"
    echo "  ${GREEN}Claude Flow:${NC}   tcp://localhost:9500"

    # Check for cloudflared tunnel
    if docker ps 2>/dev/null | grep -q cloudflared-tunnel; then
        echo ""
        success "Cloudflared tunnel: Active"
        echo "  ${GREEN}Public URL:${NC}    https://www.visionflow.info"
    fi
}

# Clean everything
clean_all() {
    warning "This will remove ALL VisionFlow containers, volumes, and images"
    echo ""
    read -p "Are you sure you want to continue? (yes/no): " -r
    echo

    if [[ "$REPLY" == "yes" ]]; then
        log "Cleaning all VisionFlow resources..."

        # Stop and remove conflicting containers
        cleanup_conflicts

        # Stop all containers for both environments
        for env in dev prod; do
            export COMPOSE_PROFILES="$env"
            log "Stopping $env environment..."
            docker_compose down -v --remove-orphans 2>/dev/null || true
        done

        # Remove VisionFlow volumes (including those from different project names)
        log "Removing VisionFlow volumes..."
        docker volume ls --format '{{.Name}}' | grep -E '(visionflow|ar-ai-knowledge-graph)' | xargs -r docker volume rm -f 2>/dev/null || true

        # Remove images
        log "Removing VisionFlow images..."
        docker images | grep -E '(visionflow|ar-ai-knowledge-graph)' | awk '{print $3}' | xargs -r docker rmi -f || true

        # Clean build cache
        log "Cleaning build cache..."
        docker builder prune -f

        success "Cleanup complete - all VisionFlow resources removed"
    else
        info "Cleanup cancelled"
    fi
}

# Show banner
show_banner() {
    echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║         VisionFlow Unified Launcher                        ║${NC}"
    echo -e "${GREEN}║  Command:     ${CYAN}$(printf '%-42s' "$COMMAND")${GREEN}║${NC}"
    echo -e "${GREEN}║  Environment: ${YELLOW}$(printf '%-42s' "$ENVIRONMENT")${GREEN}║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
    echo ""
}

# Main execution
main() {
    # Validate inputs
    validate_command
    validate_environment

    # Show banner
    show_banner

    # Load configuration
    load_env_config
    set_environment_vars

    # Execute command
    case "$COMMAND" in
        up)
            check_prerequisites
            detect_dind
            detect_gpu
            if [[ "$WITH_AGENT" == "true" ]]; then
                info "Starting with --with-agent flag"
                restart_agent_container
                echo ""
            fi
            start_environment
            ;;
        down)
            stop_environment
            ;;
        build)
            check_prerequisites
            detect_dind
            detect_gpu
            build_containers
            ;;
        rebuild)
            check_prerequisites
            detect_dind
            detect_gpu
            # Explicit rebuild: clean all cargo caches and --no-cache image build
            clean_cargo_volumes
            build_containers
            ;;
        logs)
            show_logs
            ;;
        shell)
            open_shell
            ;;
        restart)
            check_prerequisites
            detect_dind
            detect_gpu
            if [[ "$WITH_AGENT" == "true" ]]; then
                info "Restarting with --with-agent flag"
                restart_agent_container
                echo ""
            fi
            restart_environment
            ;;
        restart-agent)
            check_prerequisites
            restart_agent_container
            ;;
        rebuild-agent)
            check_prerequisites
            detect_gpu
            rebuild_agent_container "$@"
            ;;
        status)
            show_status
            ;;
        clean)
            clean_all
            ;;
    esac
}

# Run main function
main
