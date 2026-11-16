# How-to: SQLite backup & restore (VisionClaw)

Covers the operational SQLite databases: `kpi`, `enrichment`, `settings`,
`shares`, `liveness`. The Rust tool that also dumps Oxigraph is described in
[store-backup-restore.md](store-backup-restore.md). Closes TODO-unified **C-3** (exposed by the PRD-014 correction:
these databases previously had **no backup at all**).

## Where the live databases actually are

The `data/*.sqlite3` files in the repo working tree are **stale dev copies**
(e.g. `data/kpi.sqlite3` is ~45 KB from July). Do **not** treat them as live.

The live databases sit in the Docker **named volume `visionclaw-data`**
(`/mnt/nvme/docker/volumes/visionclaw-data/_data` on the host), mounted at
`/app/data` inside `visionclaw_container`. As of this writing the live
`kpi.sqlite3` is ~19 MB with 205,577 rows in `kpi_agent_events`.

All four run in **WAL mode**. That is why we never `cp` them: a copy taken
mid-write can miss committed pages still in the `-wal` file. Both scripts use
the SQLite **online backup API** (`.backup` / `.restore`), which snapshots the
main DB and its WAL under a lock — safe against a live writer.

## Scripts

| Script | Purpose |
|--------|---------|
| `scripts/backup-sqlite.sh`  | Snapshot every DB to a timestamped dir, integrity-check each, rotate. |
| `scripts/restore-sqlite.sh` | Restore one DB from a backup; destructive targets gated behind `--yes`. |

Both auto-detect **docker mode** (DBs inside `visionclaw_container`, default)
vs **host mode** (DBs are plain local files). `sqlite3` is present both on this
host and inside the container.

## Back up

```bash
# Default: auto-detect, snapshot every database, keep newest 14, write to ./backups/sqlite/
./scripts/backup-sqlite.sh

# Common overrides
BACKUP_ROOT=/srv/backups ./scripts/backup-sqlite.sh   # off-repo destination (recommended for prod)
KEEP=30 ./scripts/backup-sqlite.sh                    # deeper rotation
MODE=host DATA_DIR=./data ./scripts/backup-sqlite.sh  # force host mode
```

Each run creates `<BACKUP_ROOT>/<UTC-STAMP>/` (default `./backups/sqlite/`) containing one file per database
plus `MANIFEST.txt` (mode, source, per-DB sha256). Every snapshot is
`PRAGMA integrity_check`-ed before it is trusted; the run fails loudly if any
check is not `ok`.

In docker mode the snapshot is produced **inside** the container (in the
persistent volume) and then `docker cp`-ed out to `BACKUP_ROOT` on the host —
so the backups live **off** the source volume and survive a volume loss.

## Rotation policy

Keep the newest **`KEEP`** timestamped dirs (default **14**). Older dirs are
pruned at the end of each successful run. With a daily schedule that is a
two-week window. Raise `KEEP` for a longer retention tail.

**Physical-disk separation (already satisfied by the default).** The default
`BACKUP_ROOT=./backups/sqlite` (never inside `./data`: the script refuses that) is not the same disk as the live data on the reference host: the checkout and
the live `visionclaw-data` volume sit on different physical disks. So a default-configured backup already lands on a *different
physical disk* from the source — a single-disk failure cannot take both. Point
`BACKUP_ROOT` at a third host/disk (or off-box entirely) for defence against
whole-host loss.

## Schedule it

**cron** (daily 03:15, off-repo destination):

```cron
15 3 * * * cd /home/devuser/workspace/project && BACKUP_ROOT=/srv/backups KEEP=14 ./scripts/backup-sqlite.sh >> /var/log/sqlite-backup.log 2>&1
```

**supervisord** (run once daily via a wrapping sleep loop, matching this
repo's supervisor conventions):

```ini
[program:sqlite-backup]
command=/bin/bash -c 'while true; do cd /home/devuser/workspace/project && BACKUP_ROOT=/srv/backups ./scripts/backup-sqlite.sh; sleep 86400; done'
user=devuser
autostart=true
autorestart=true
stdout_logfile=/var/log/sqlite-backup.log
redirect_stderr=true
```

## Restore

By default `restore-sqlite.sh` restores to a **temp path** so you can inspect
first — safe, no `--yes` needed. Overwriting a live DB or an existing host file
requires `--yes` (destructive, irreversible). The backup is integrity-checked
before restore and the result is integrity-checked after.

```bash
# Safe: restore to a scratch file and inspect
./scripts/restore-sqlite.sh \
  --backup backups/sqlite/<STAMP> --db kpi.sqlite3 \
  --to /tmp/kpi.restored.sqlite3

# Destructive: restore INTO the live container DB (stop writers first if possible)
./scripts/restore-sqlite.sh \
  --backup backups/sqlite/<STAMP> --db kpi.sqlite3 \
  --into-container --yes

# Destructive: restore to a host path that already exists
./scripts/restore-sqlite.sh \
  --backup backups/sqlite/<STAMP> --db settings.sqlite3 \
  --to ./data/settings.sqlite3 --yes
```

## Restore drill (run this quarterly)

Prove the backups are restorable without touching live data:

```bash
BK=$(ls -d backups/sqlite/20*/ | tail -1)

# 1. Live row count (source of truth)
docker exec visionclaw_container sqlite3 /app/data/kpi.sqlite3 \
  "SELECT count(*) FROM kpi_agent_events;"

# 2. Restore latest backup to a temp path
./scripts/restore-sqlite.sh --backup "${BK%/}" --db kpi.sqlite3 \
  --to /tmp/kpi.restored.sqlite3

# 3. Integrity + row count on the restored copy — compare to step 1
sqlite3 /tmp/kpi.restored.sqlite3 "PRAGMA integrity_check;"
sqlite3 /tmp/kpi.restored.sqlite3 "SELECT count(*) FROM kpi_agent_events;"
```

Last drill: **2026-08-31** — `integrity_check` = `ok`, live and restored
`kpi_agent_events` both **205,577**. Evidence in TODO-unified C-3.
