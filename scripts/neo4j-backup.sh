#!/usr/bin/env bash
# Neo4j backup script — runs against the neo4j container via docker exec
# Usage: ./scripts/neo4j-backup.sh [backup_dir]
# Schedule: add to crontab: 0 2 * * * /path/to/scripts/neo4j-backup.sh
set -euo pipefail

BACKUP_DIR="${1:-/app/data/backups/neo4j}"
CONTAINER_NAME="${NEO4J_CONTAINER:-visionflow-neo4j}"
NEO4J_USER="${NEO4J_USER:-neo4j}"
NEO4J_PASS="${NEO4J_PASSWORD:-changeme-dev}"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
BACKUP_FILE="neo4j-backup-${TIMESTAMP}"
RETENTION_DAYS="${BACKUP_RETENTION_DAYS:-7}"

log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"; }

die() { log "ERROR: $*" >&2; exit 1; }

# --- pre-flight checks ---

command -v docker &>/dev/null || die "docker not found in PATH"

docker inspect "$CONTAINER_NAME" &>/dev/null || die "Container '$CONTAINER_NAME' not found. Is it running?"

container_status="$(docker inspect -f '{{.State.Status}}' "$CONTAINER_NAME")"
[ "$container_status" = "running" ] || die "Container '$CONTAINER_NAME' is ${container_status}, not running"

mkdir -p "$BACKUP_DIR"

# --- attempt backup strategies in order of preference ---

backup_succeeded=false

# Strategy 1: neo4j-admin database dump (Neo4j 5.x)
# Works on Community Edition but requires the database to be stopped first.
# We try it anyway because some deployments allow online dump.
log "Strategy 1: neo4j-admin database dump..."
if docker exec "$CONTAINER_NAME" neo4j-admin database dump neo4j --to-path=/tmp/ 2>/dev/null; then
    # The dump command produces /tmp/neo4j.dump
    if docker cp "${CONTAINER_NAME}:/tmp/neo4j.dump" "${BACKUP_DIR}/${BACKUP_FILE}.dump"; then
        docker exec "$CONTAINER_NAME" rm -f /tmp/neo4j.dump 2>/dev/null || true
        backup_succeeded=true
        log "Strategy 1 succeeded: ${BACKUP_DIR}/${BACKUP_FILE}.dump"
    fi
fi

# Strategy 2: APOC full Cypher export (works online, needs APOC plugin)
if [ "$backup_succeeded" = false ]; then
    log "Strategy 2: APOC Cypher export..."
    export_path="/tmp/${BACKUP_FILE}.cypher"
    apoc_cmd="CALL apoc.export.cypher.all('${export_path}', {format:'plain', useOptimizations:{type:'UNWIND_BATCH',unwindBatchSize:100}})"

    if docker exec "$CONTAINER_NAME" cypher-shell \
        -u "$NEO4J_USER" -p "$NEO4J_PASS" \
        --format plain \
        "$apoc_cmd" 2>/dev/null; then
        if docker cp "${CONTAINER_NAME}:${export_path}" "${BACKUP_DIR}/${BACKUP_FILE}.cypher"; then
            docker exec "$CONTAINER_NAME" rm -f "${export_path}" 2>/dev/null || true
            backup_succeeded=true
            log "Strategy 2 succeeded: ${BACKUP_DIR}/${BACKUP_FILE}.cypher"
        fi
    fi
fi

# Strategy 3: cypher-shell node/relationship dump (always available, slower)
if [ "$backup_succeeded" = false ]; then
    log "Strategy 3: cypher-shell node + relationship export..."

    cypher_opts=(-u "$NEO4J_USER" -p "$NEO4J_PASS" --format plain)

    # Export node count first as a sanity check
    node_count="$(docker exec "$CONTAINER_NAME" cypher-shell "${cypher_opts[@]}" \
        "MATCH (n) RETURN count(n) AS cnt" 2>/dev/null | tail -1)"
    log "Node count: ${node_count:-unknown}"

    # Export nodes with labels and properties
    docker exec "$CONTAINER_NAME" cypher-shell "${cypher_opts[@]}" \
        "CALL apoc.export.json.all('/tmp/${BACKUP_FILE}-nodes.json', {useTypes:true})" 2>/dev/null \
    && docker cp "${CONTAINER_NAME}:/tmp/${BACKUP_FILE}-nodes.json" "${BACKUP_DIR}/${BACKUP_FILE}-nodes.json" 2>/dev/null

    # If APOC JSON export failed, fall back to raw Cypher output
    if [ ! -f "${BACKUP_DIR}/${BACKUP_FILE}-nodes.json" ]; then
        log "APOC JSON unavailable, falling back to raw Cypher output..."

        docker exec "$CONTAINER_NAME" cypher-shell "${cypher_opts[@]}" \
            "MATCH (n) RETURN labels(n) AS labels, properties(n) AS props" \
            > "${BACKUP_DIR}/${BACKUP_FILE}-nodes.cypher" 2>/dev/null || true

        docker exec "$CONTAINER_NAME" cypher-shell "${cypher_opts[@]}" \
            "MATCH (a)-[r]->(b) RETURN id(a) AS src, id(b) AS tgt, type(r) AS type, properties(r) AS props" \
            > "${BACKUP_DIR}/${BACKUP_FILE}-rels.cypher" 2>/dev/null || true

        if [ -s "${BACKUP_DIR}/${BACKUP_FILE}-nodes.cypher" ]; then
            backup_succeeded=true
            log "Strategy 3 succeeded (raw Cypher): ${BACKUP_DIR}/${BACKUP_FILE}-{nodes,rels}.cypher"
        else
            die "All backup strategies failed. Check container logs: docker logs $CONTAINER_NAME"
        fi
    else
        docker exec "$CONTAINER_NAME" rm -f "/tmp/${BACKUP_FILE}-nodes.json" 2>/dev/null || true
        backup_succeeded=true
        log "Strategy 3 succeeded (APOC JSON): ${BACKUP_DIR}/${BACKUP_FILE}-nodes.json"
    fi
fi

# --- compress ---
if command -v gzip &>/dev/null; then
    log "Compressing backup files..."
    for f in "${BACKUP_DIR}/${BACKUP_FILE}"*; do
        [ -f "$f" ] && gzip "$f" 2>/dev/null && log "Compressed: ${f}.gz"
    done
fi

# --- retention ---
deleted_count=0
while IFS= read -r old_backup; do
    rm -f "$old_backup"
    deleted_count=$((deleted_count + 1))
done < <(find "$BACKUP_DIR" -name "neo4j-backup-*" -mtime "+${RETENTION_DAYS}" -type f 2>/dev/null)
[ "$deleted_count" -gt 0 ] && log "Pruned ${deleted_count} backups older than ${RETENTION_DAYS} days"

# --- summary ---
log "Backup complete."
log "Contents of ${BACKUP_DIR}:"
ls -lh "${BACKUP_DIR}/${BACKUP_FILE}"* 2>/dev/null || ls -lh "${BACKUP_DIR}/" 2>/dev/null
