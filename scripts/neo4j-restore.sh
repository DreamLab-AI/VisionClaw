#!/usr/bin/env bash
# Neo4j restore script — restores a backup into the neo4j container
# Usage: ./scripts/neo4j-restore.sh <backup_file>
#
# Supported formats:
#   *.dump / *.dump.gz    — neo4j-admin database load (full binary restore)
#   *.cypher / *.cypher.gz — cypher-shell replay (APOC or raw export)
#   *.json / *.json.gz    — APOC JSON import
#
# The script will:
#   1. Verify the backup file exists and is readable
#   2. Prompt for confirmation (unless --yes is passed)
#   3. Stop the database if needed (dump restore)
#   4. Restore data
#   5. Restart and verify
set -euo pipefail

CONTAINER_NAME="${NEO4J_CONTAINER:-visionflow-neo4j}"
NEO4J_USER="${NEO4J_USER:-neo4j}"
NEO4J_PASS="${NEO4J_PASSWORD:-changeme-dev}"

log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"; }

die() { log "ERROR: $*" >&2; exit 1; }

usage() {
    echo "Usage: $0 [--yes] <backup_file>"
    echo ""
    echo "Options:"
    echo "  --yes    Skip confirmation prompt"
    echo ""
    echo "Supported backup formats:"
    echo "  *.dump(.gz)     neo4j-admin binary dump"
    echo "  *.cypher(.gz)   Cypher statements (APOC export or raw)"
    echo "  *.json(.gz)     APOC JSON export"
    exit 1
}

wait_for_neo4j() {
    local max_wait=60
    local waited=0
    log "Waiting for Neo4j to become available (max ${max_wait}s)..."
    while [ $waited -lt $max_wait ]; do
        if docker exec "$CONTAINER_NAME" cypher-shell \
            -u "$NEO4J_USER" -p "$NEO4J_PASS" \
            "RETURN 1" &>/dev/null; then
            log "Neo4j is ready."
            return 0
        fi
        sleep 2
        waited=$((waited + 2))
    done
    die "Neo4j did not become available within ${max_wait}s"
}

verify_restore() {
    log "Verifying restore..."
    local node_count
    node_count="$(docker exec "$CONTAINER_NAME" cypher-shell \
        -u "$NEO4J_USER" -p "$NEO4J_PASS" \
        --format plain \
        "MATCH (n) RETURN count(n) AS cnt" 2>/dev/null | tail -1)"
    local rel_count
    rel_count="$(docker exec "$CONTAINER_NAME" cypher-shell \
        -u "$NEO4J_USER" -p "$NEO4J_PASS" \
        --format plain \
        "MATCH ()-[r]->() RETURN count(r) AS cnt" 2>/dev/null | tail -1)"
    log "Restored database contains: ${node_count} nodes, ${rel_count} relationships"
    if [ "${node_count:-0}" = "0" ]; then
        log "WARNING: Node count is 0. The restore may have failed or the backup was empty."
        return 1
    fi
    return 0
}

# --- parse args ---

auto_confirm=false
backup_file=""

while [ $# -gt 0 ]; do
    case "$1" in
        --yes|-y) auto_confirm=true; shift ;;
        --help|-h) usage ;;
        -*) die "Unknown option: $1" ;;
        *) backup_file="$1"; shift ;;
    esac
done

[ -n "$backup_file" ] || usage
[ -f "$backup_file" ] || die "Backup file not found: $backup_file"
[ -r "$backup_file" ] || die "Backup file not readable: $backup_file"

# --- pre-flight ---

command -v docker &>/dev/null || die "docker not found in PATH"
docker inspect "$CONTAINER_NAME" &>/dev/null || die "Container '$CONTAINER_NAME' not found"

container_status="$(docker inspect -f '{{.State.Status}}' "$CONTAINER_NAME")"
[ "$container_status" = "running" ] || die "Container '$CONTAINER_NAME' is ${container_status}, not running"

# --- decompress if needed ---

restore_file="$backup_file"
tmp_decompressed=""

if [[ "$backup_file" == *.gz ]]; then
    log "Decompressing ${backup_file}..."
    tmp_decompressed="/tmp/neo4j-restore-$(date +%s)-$(basename "${backup_file%.gz}")"
    gunzip -c "$backup_file" > "$tmp_decompressed"
    restore_file="$tmp_decompressed"
fi

# Determine format from the (decompressed) filename
case "$restore_file" in
    *.dump)   restore_type="dump" ;;
    *.cypher) restore_type="cypher" ;;
    *.json)   restore_type="json" ;;
    *)        die "Unrecognized backup format: $restore_file (expected .dump, .cypher, or .json)" ;;
esac

# --- confirmation ---

log "Restore plan:"
log "  Backup file:  $backup_file"
log "  Format:       $restore_type"
log "  Container:    $CONTAINER_NAME"
log ""
log "WARNING: This will overwrite the current Neo4j database."

if [ "$auto_confirm" = false ]; then
    printf "Type 'yes' to proceed: "
    read -r confirm
    [ "$confirm" = "yes" ] || die "Aborted by user"
fi

# --- get current state for comparison ---

log "Current database state:"
pre_count="$(docker exec "$CONTAINER_NAME" cypher-shell \
    -u "$NEO4J_USER" -p "$NEO4J_PASS" \
    --format plain \
    "MATCH (n) RETURN count(n) AS cnt" 2>/dev/null | tail -1 || echo "unknown")"
log "  Nodes before restore: ${pre_count}"

# --- restore ---

case "$restore_type" in
    dump)
        log "Restoring from binary dump..."

        # Copy dump into container
        docker cp "$restore_file" "${CONTAINER_NAME}:/tmp/neo4j-restore.dump"

        # neo4j-admin database load requires the database to be stopped
        log "Stopping neo4j database inside container..."
        docker exec "$CONTAINER_NAME" neo4j-admin database stop neo4j 2>/dev/null || true

        # Load the dump — --from-path for 5.x, --from-stdin not used here
        # --overwrite-destination=true replaces existing data
        if ! docker exec "$CONTAINER_NAME" neo4j-admin database load neo4j \
            --from-path=/tmp/ --overwrite-destination=true 2>/dev/null; then
            # Try the alternate 5.x syntax
            docker exec "$CONTAINER_NAME" neo4j-admin database load \
                --from-path=/tmp/neo4j-restore.dump \
                --overwrite-destination=true \
                neo4j 2>&1 || die "neo4j-admin database load failed"
        fi

        # Clean up and restart
        docker exec "$CONTAINER_NAME" rm -f /tmp/neo4j-restore.dump 2>/dev/null || true

        log "Restarting neo4j database..."
        docker exec "$CONTAINER_NAME" neo4j-admin database start neo4j 2>/dev/null || {
            log "neo4j-admin start failed, restarting container..."
            docker restart "$CONTAINER_NAME"
        }

        wait_for_neo4j
        ;;

    cypher)
        log "Restoring from Cypher statements..."

        # Clear existing data first
        log "Clearing existing data..."
        docker exec "$CONTAINER_NAME" cypher-shell \
            -u "$NEO4J_USER" -p "$NEO4J_PASS" \
            "CALL apoc.periodic.iterate('MATCH (n) RETURN n', 'DETACH DELETE n', {batchSize:10000})" 2>/dev/null \
        || docker exec "$CONTAINER_NAME" cypher-shell \
            -u "$NEO4J_USER" -p "$NEO4J_PASS" \
            "MATCH (n) DETACH DELETE n" 2>/dev/null \
        || log "WARNING: Could not clear existing data. Restore will merge."

        # Copy file into container and replay
        docker cp "$restore_file" "${CONTAINER_NAME}:/tmp/restore.cypher"
        log "Replaying Cypher statements (this may take a while)..."
        docker exec "$CONTAINER_NAME" cypher-shell \
            -u "$NEO4J_USER" -p "$NEO4J_PASS" \
            --file /tmp/restore.cypher 2>&1 || die "Cypher replay failed"
        docker exec "$CONTAINER_NAME" rm -f /tmp/restore.cypher 2>/dev/null || true
        ;;

    json)
        log "Restoring from APOC JSON export..."

        # Clear existing data first
        log "Clearing existing data..."
        docker exec "$CONTAINER_NAME" cypher-shell \
            -u "$NEO4J_USER" -p "$NEO4J_PASS" \
            "CALL apoc.periodic.iterate('MATCH (n) RETURN n', 'DETACH DELETE n', {batchSize:10000})" 2>/dev/null \
        || docker exec "$CONTAINER_NAME" cypher-shell \
            -u "$NEO4J_USER" -p "$NEO4J_PASS" \
            "MATCH (n) DETACH DELETE n" 2>/dev/null \
        || log "WARNING: Could not clear existing data. Restore will merge."

        # Copy file and import
        docker cp "$restore_file" "${CONTAINER_NAME}:/tmp/restore.json"
        log "Importing APOC JSON (this may take a while)..."
        docker exec "$CONTAINER_NAME" cypher-shell \
            -u "$NEO4J_USER" -p "$NEO4J_PASS" \
            "CALL apoc.import.json('/tmp/restore.json')" 2>&1 || die "APOC JSON import failed"
        docker exec "$CONTAINER_NAME" rm -f /tmp/restore.json 2>/dev/null || true
        ;;
esac

# --- cleanup temp file ---

[ -n "$tmp_decompressed" ] && rm -f "$tmp_decompressed"

# --- verify ---

verify_restore

log "Restore complete."
