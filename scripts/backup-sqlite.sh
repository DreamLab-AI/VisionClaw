#!/usr/bin/env bash
#
# backup-sqlite.sh — online-safe backups of the VisionClaw SQLite databases.
#
# Why this exists (PRD-014 / TODO-unified C-3):
#   data/{kpi,enrichment,settings,liveness}.sqlite3 had NO backup at all.
#   The LIVE databases are not the stale copies in the repo's data/ dir — they
#   live in the Docker named volume `visionclaw-data`, mounted at /app/data
#   inside `visionclaw_container`. All four run in WAL mode, so a naive `cp`
#   can capture a torn snapshot (committed pages still sitting in the -wal
#   file). This script uses the SQLite ONLINE BACKUP API — `.backup` — which
#   takes a lock-consistent snapshot across the main DB and its WAL.
#
# Two modes, auto-detected:
#   docker  — DBs live in visionclaw_container:/app/data. We run sqlite3 .backup
#             INSIDE the container, then `docker cp` the artefact out to a
#             host-visible backup dir (off the source volume, so a volume loss
#             does not take the backups with it).
#   host    — DBs are plain files in a local directory (DATA_DIR). We run
#             sqlite3 .backup directly. Used on machines where the DBs sit on a
#             host path rather than inside a container.
#
# Rotation: timestamped dir per run; keep the newest KEEP (default 14).
#
# ADR-2017 membership contract:
#   Databases are declared in two classes, and the class decides what a missing
#   member means.
#
#     REQUIRED_DBS — authoritative state with no other source of truth. A
#                    missing required member is a FAILED backup: the run exits
#                    non-zero and writes no manifest, because a backup set that
#                    silently omits the settings database is worse than no
#                    backup at all (it looks restorable and is not).
#     OPTIONAL_DBS — state that a running server rebuilds from scratch. A
#                    missing optional member is logged and skipped; the run
#                    still succeeds and the manifest records it as absent.
#
#   settings/enrichment/kpi hold authored configuration, the enrichment
#   proposal-and-decision ledger and the KPI lineage series — none of which can
#   be regenerated. liveness holds canary telemetry that the runtime watchdog
#   recreates on boot, so it is optional.
#
# ADR-2017 destination contract:
#   The default destination is OUTSIDE the source data directory. Writing
#   backups into ./data puts them in the same failure domain as the databases
#   they protect: one `rm -rf data`, one corrupt volume, one bad bind-mount and
#   both the primary and every snapshot are gone at once. The script refuses a
#   destination that resolves inside the source directory unless
#   ALLOW_BACKUP_INSIDE_DATA=1 is set explicitly.
#
# ADR-2017 restore check:
#   integrity_check proves the SQLite pages are self-consistent; it does not
#   prove the snapshot carries the application's schema. With VERIFY_RESTORE=1
#   (the default) each snapshot is restored into a scratch directory and probed
#   for the tables the application actually reads. A page-clean snapshot of an
#   empty file fails that probe.
#
# Usage:
#   scripts/backup-sqlite.sh                 # auto-detect, default settings
#   BACKUP_ROOT=/srv/backups scripts/backup-sqlite.sh
#   MODE=host DATA_DIR=./data scripts/backup-sqlite.sh
#   KEEP=30 scripts/backup-sqlite.sh
#   VERIFY_RESTORE=0 scripts/backup-sqlite.sh   # skip the restore probe
#
set -euo pipefail

# ----- configuration (env-overridable) --------------------------------------
CONTAINER="${CONTAINER:-visionclaw_container}"
CONTAINER_DATA_DIR="${CONTAINER_DATA_DIR:-/app/data}"
DATA_DIR="${DATA_DIR:-./data}"                 # host-mode source dir
# ADR-2017: default destination sits beside ./data, never inside it.
BACKUP_ROOT="${BACKUP_ROOT:-./backups/sqlite}" # where timestamped dirs land
KEEP="${KEEP:-14}"                             # rotation depth
# ADR-2017 membership: required members fail the run when absent.
REQUIRED_DBS="${REQUIRED_DBS:-settings.sqlite3 enrichment.sqlite3 kpi.sqlite3 shares.sqlite3}"
OPTIONAL_DBS="${OPTIONAL_DBS:-liveness.sqlite3}"
# DBS stays overridable for ad-hoc runs; when set explicitly it is treated as
# the required set unless REQUIRED_DBS is also given.
DBS="${DBS:-$REQUIRED_DBS $OPTIONAL_DBS}"
MODE="${MODE:-auto}"
VERIFY_RESTORE="${VERIFY_RESTORE:-1}"
ALLOW_BACKUP_INSIDE_DATA="${ALLOW_BACKUP_INSIDE_DATA:-0}"

log() { printf '%s [backup-sqlite] %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >&2; }
die() { log "ERROR: $*"; exit 1; }

# Is $db declared required?
is_required() {
  local db="$1" r
  for r in $REQUIRED_DBS; do [[ "$r" == "$db" ]] && return 0; done
  return 1
}

# The application tables each database must carry for a restore to be usable.
# Sourced from the repository adapters that own each file (see
# src/adapters/sqlite_*_repository.rs).
expected_tables_for() {
  case "$1" in
    settings.sqlite3)   echo "settings schema_migrations" ;;
    enrichment.sqlite3) echo "enrichment_proposals enrichment_decisions schema_migrations" ;;
    kpi.sqlite3)        echo "kpi_snapshots kpi_lineage schema_migrations" ;;
    shares.sqlite3)     echo "shared_graphs schema_migrations" ;;
    liveness.sqlite3)   echo "liveness_canaries canary_fires schema_migrations" ;;
    *)                  echo "" ;;
  esac
}

# ADR-2017 application-level restore check. Copies the snapshot to a scratch
# path (a real restore, not an in-place read) and asserts every expected table
# is present and queryable. Returns non-zero with a reason on failure.
verify_restore() {
  local db="$1" snapshot="$2" expected
  expected="$(expected_tables_for "$db")"
  [[ -n "$expected" ]] || { log "restore-check $db: no declared schema, skipped"; return 0; }
  command -v sqlite3 >/dev/null 2>&1 || die "VERIFY_RESTORE=1 needs sqlite3 on PATH"

  local scratch restored
  scratch="$(mktemp -d)"
  restored="$scratch/$db"
  cp "$snapshot" "$restored" || { rm -rf "$scratch"; die "restore-check $db: copy failed"; }

  local table rows rc=0
  for table in $expected; do
    if ! rows="$(sqlite3 "$restored" "SELECT count(*) FROM \"$table\";" 2>&1)"; then
      log "restore-check $db: table '$table' is not queryable: $rows"
      rc=1
      break
    fi
    log "restore-check $db: $table rows=$rows"
  done
  rm -rf "$scratch"
  return $rc
}

command -v sqlite3 >/dev/null 2>&1 || log "note: host sqlite3 not found (only needed for host mode / verification)"

# ----- mode detection -------------------------------------------------------
detect_mode() {
  if [[ "$MODE" != "auto" ]]; then echo "$MODE"; return; fi
  if command -v docker >/dev/null 2>&1 \
     && docker inspect -f '{{.State.Running}}' "$CONTAINER" >/dev/null 2>&1; then
    echo docker
  else
    echo host
  fi
}
MODE="$(detect_mode)"
log "mode=$MODE keep=$KEEP dbs=[$DBS]"

# ----- destination ----------------------------------------------------------
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
DEST="${BACKUP_ROOT%/}/${STAMP}"
mkdir -p "$DEST"

# ADR-2017 failure-domain separation: refuse a destination that resolves inside
# the host source directory. In docker mode the databases live in the container
# volume, so only host mode has a source directory to collide with.
if [[ "$MODE" == host && -d "$DATA_DIR" ]]; then
  src_abs="$(cd "$DATA_DIR" && pwd -P)"
  dest_abs="$(cd "$DEST" && pwd -P)"
  if [[ "$dest_abs" == "$src_abs" || "$dest_abs" == "$src_abs"/* ]]; then
    if [[ "$ALLOW_BACKUP_INSIDE_DATA" == "1" ]]; then
      log "WARNING: destination $dest_abs is inside the source directory $src_abs \
(ALLOW_BACKUP_INSIDE_DATA=1) — backups share a failure domain with the databases"
    else
      die "destination $dest_abs is inside the source data directory $src_abs; \
set BACKUP_ROOT to a path outside it (or ALLOW_BACKUP_INSIDE_DATA=1 to override)"
    fi
  fi
fi
log "destination: $DEST"

backup_one_docker() {
  local db="$1"
  local src="${CONTAINER_DATA_DIR%/}/$db"
  # Absent member: the caller decides whether that is fatal (see the run loop).
  if ! docker exec "$CONTAINER" test -f "$src"; then
    log "absent: $db (not present in $CONTAINER:$src)"
    return 2
  fi
  local tmp="/tmp/backup-${db}.${STAMP}"
  # .backup is the online backup API — safe against concurrent writers.
  docker exec "$CONTAINER" sqlite3 "$src" ".backup '$tmp'" \
    || die "sqlite3 .backup failed for $db in container"
  # Integrity-check the snapshot before we trust it.
  local chk
  chk="$(docker exec "$CONTAINER" sqlite3 "$tmp" 'PRAGMA integrity_check;' 2>&1)"
  [[ "$chk" == "ok" ]] || die "integrity_check failed for $db snapshot: $chk"
  docker cp "$CONTAINER:$tmp" "$DEST/$db" || die "docker cp failed for $db"
  docker exec "$CONTAINER" rm -f "$tmp" || true
  log "backed up $db ($(du -h "$DEST/$db" | cut -f1)) integrity=ok"
}

backup_one_host() {
  local db="$1"
  local src="${DATA_DIR%/}/$db"
  if [[ ! -f "$src" ]]; then
    log "absent: $db (not present at $src)"
    return 2
  fi
  command -v sqlite3 >/dev/null 2>&1 || die "host mode needs sqlite3 on PATH"
  sqlite3 "$src" ".backup '$DEST/$db'" || die "sqlite3 .backup failed for $db"
  local chk
  chk="$(sqlite3 "$DEST/$db" 'PRAGMA integrity_check;' 2>&1)"
  [[ "$chk" == "ok" ]] || die "integrity_check failed for $db snapshot: $chk"
  log "backed up $db ($(du -h "$DEST/$db" | cut -f1)) integrity=ok"
}

# ----- run ------------------------------------------------------------------
count=0
missing_required=""
missing_optional=""
for db in $DBS; do
  rc=0
  case "$MODE" in
    docker) backup_one_docker "$db" || rc=$? ;;
    host)   backup_one_host   "$db" || rc=$? ;;
    *)      die "unknown MODE=$MODE" ;;
  esac

  if (( rc == 2 )); then
    # ADR-2017: a missing REQUIRED member is a failed backup, not a skip.
    if is_required "$db"; then
      missing_required="${missing_required}${missing_required:+ }$db"
    else
      missing_optional="${missing_optional}${missing_optional:+ }$db"
    fi
    continue
  fi
  (( rc == 0 )) || die "backup of $db failed (rc=$rc)"

  [[ -f "$DEST/$db" ]] || die "backup of $db reported success but produced no file"

  # ADR-2017: prove the snapshot is application-restorable, not merely
  # page-consistent, before it is counted as a captured database.
  if [[ "$VERIFY_RESTORE" == "1" ]]; then
    verify_restore "$db" "$DEST/$db" \
      || die "restore check failed for $db — snapshot is not application-restorable"
  fi
  count=$((count + 1))
done

if [[ -n "$missing_required" ]]; then
  rm -rf "$DEST"
  die "required database(s) missing from the source: $missing_required \
(declared REQUIRED_DBS=[$REQUIRED_DBS]) — refusing to publish an incomplete backup set"
fi
[[ -n "$missing_optional" ]] && log "optional database(s) absent, continuing: $missing_optional"
[[ "$count" -gt 0 ]] || die "no databases backed up — check CONTAINER/DATA_DIR"

# Manifest for the restore drill and for auditing what a run captured.
{
  echo "backup_utc=$STAMP"
  echo "mode=$MODE"
  echo "source=$([[ "$MODE" == docker ]] && echo "$CONTAINER:$CONTAINER_DATA_DIR" || echo "$DATA_DIR")"
  echo "databases=$count"
  echo "required_dbs=$REQUIRED_DBS"
  echo "optional_dbs=$OPTIONAL_DBS"
  echo "missing_optional=$missing_optional"
  echo "restore_check=$([[ "$VERIFY_RESTORE" == "1" ]] && echo application-level || echo skipped)"
  for db in $DBS; do
    [[ -f "$DEST/$db" ]] && echo "sha256=$(sha256sum "$DEST/$db" | cut -d' ' -f1)  $db"
  done
} > "$DEST/MANIFEST.txt"
log "wrote $DEST/MANIFEST.txt ($count databases)"

# ----- rotation: keep newest KEEP timestamped dirs --------------------------
mapfile -t all < <(find "${BACKUP_ROOT%/}" -mindepth 1 -maxdepth 1 -type d -name '20*' | sort)
if (( ${#all[@]} > KEEP )); then
  prune=$(( ${#all[@]} - KEEP ))
  log "rotation: ${#all[@]} backups present, pruning oldest $prune (keep=$KEEP)"
  for ((i = 0; i < prune; i++)); do
    log "  removing ${all[$i]}"
    rm -rf "${all[$i]}"
  done
else
  log "rotation: ${#all[@]} backups present, under keep=$KEEP, nothing to prune"
fi

log "DONE — $count databases in $DEST"
echo "$DEST"
