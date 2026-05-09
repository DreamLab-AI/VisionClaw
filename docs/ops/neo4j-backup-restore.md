# Neo4j Backup & Restore Runbook

## Quick Reference

| Item | Value |
|------|-------|
| Container | `visionflow-neo4j` |
| Image | `neo4j:5.13.0` (Community Edition) |
| Data volume | `neo4j-data` |
| Backup dir (default) | `/app/data/backups/neo4j` |
| Backup script | `scripts/neo4j-backup.sh` |
| Restore script | `scripts/neo4j-restore.sh` |
| RPO | 24 hours (daily cron) |
| RTO | ~30 minutes |
| Retention | 7 days (configurable via `BACKUP_RETENTION_DAYS`) |

## Manual Backup

```bash
# From the project root
./scripts/neo4j-backup.sh

# Custom backup directory
./scripts/neo4j-backup.sh /mnt/backups/neo4j

# Override container name or password
NEO4J_CONTAINER=my-neo4j NEO4J_PASSWORD=secret ./scripts/neo4j-backup.sh
```

The script tries three strategies in order:
1. `neo4j-admin database dump` (binary, most reliable)
2. APOC `apoc.export.cypher.all` (Cypher statements, works online)
3. Raw `cypher-shell` node/relationship export (always available, slowest)

Output is gzip-compressed automatically.

## Automated Daily Backups (Cron)

Add to the host crontab (the machine running Docker, not inside the container):

```bash
# Edit crontab
crontab -e

# Add this line — runs at 02:00 daily
0 2 * * * /mnt/mldata/githubs/AR-AI-Knowledge-Graph/scripts/neo4j-backup.sh /mnt/backups/neo4j >> /var/log/neo4j-backup.log 2>&1
```

Adjust the path to match your host project location. The script uses the `$NEO4J_PASSWORD` env var; if your cron environment does not inherit it, set it inline:

```bash
0 2 * * * NEO4J_PASSWORD=your-password /path/to/scripts/neo4j-backup.sh /mnt/backups/neo4j >> /var/log/neo4j-backup.log 2>&1
```

## Restore

```bash
# Interactive (prompts for confirmation)
./scripts/neo4j-restore.sh /app/data/backups/neo4j/neo4j-backup-20260509_020000.dump.gz

# Non-interactive
./scripts/neo4j-restore.sh --yes /app/data/backups/neo4j/neo4j-backup-20260509_020000.cypher.gz

# Override credentials
NEO4J_PASSWORD=secret ./scripts/neo4j-restore.sh backup.dump.gz
```

Supported formats:
- `.dump` / `.dump.gz` — binary dump (requires brief database stop)
- `.cypher` / `.cypher.gz` — Cypher statement replay
- `.json` / `.json.gz` — APOC JSON import

The restore script will:
1. Check the backup file exists and is readable
2. Show current node count
3. Prompt for confirmation (unless `--yes`)
4. Clear existing data (for Cypher/JSON formats) or overwrite (for dump)
5. Import the backup
6. Verify the result (node + relationship counts)

## Verification Procedure

After any restore, verify the database is healthy:

```bash
# Check node count
docker exec visionflow-neo4j cypher-shell -u neo4j -p "${NEO4J_PASSWORD:-changeme-dev}" \
    "MATCH (n) RETURN count(n) AS nodes"

# Check relationship count
docker exec visionflow-neo4j cypher-shell -u neo4j -p "${NEO4J_PASSWORD:-changeme-dev}" \
    "MATCH ()-[r]->() RETURN count(r) AS rels"

# Check label distribution
docker exec visionflow-neo4j cypher-shell -u neo4j -p "${NEO4J_PASSWORD:-changeme-dev}" \
    "CALL db.labels() YIELD label RETURN label, count{(n) WHERE label IN labels(n)} AS count ORDER BY count DESC"

# VisionClaw health endpoint (checks Neo4j connectivity)
curl -s http://localhost:3001/health | jq .
```

### Test Restore (Recommended Monthly)

Spin up a throwaway Neo4j container and restore into it to validate the backup without affecting production:

```bash
# Start a temporary Neo4j instance
docker run -d --name neo4j-test \
    -e NEO4J_AUTH=neo4j/test-password \
    -p 17474:7474 -p 17687:7687 \
    neo4j:5.13.0

# Wait for it to start
sleep 15

# Restore into it
NEO4J_CONTAINER=neo4j-test NEO4J_PASSWORD=test-password \
    ./scripts/neo4j-restore.sh --yes /app/data/backups/neo4j/latest-backup.dump.gz

# Verify
docker exec neo4j-test cypher-shell -u neo4j -p test-password \
    "MATCH (n) RETURN count(n) AS nodes"

# Clean up
docker rm -f neo4j-test
```

## Retention Policy

Default: 7 days. Backups older than `BACKUP_RETENTION_DAYS` are deleted automatically after each backup run.

Override:

```bash
BACKUP_RETENTION_DAYS=30 ./scripts/neo4j-backup.sh
```

## Backup Storage

| Location | Purpose |
|----------|---------|
| `/app/data/backups/neo4j` | Default, inside the visionflow data volume |
| `/mnt/backups/neo4j` | Recommended for production (separate disk/mount) |

For off-site backup, sync the backup directory to remote storage after the cron job:

```bash
# Example: rsync to a remote server
0 3 * * * rsync -az /mnt/backups/neo4j/ backup-server:/backups/neo4j/

# Example: upload to S3-compatible storage
0 3 * * * aws s3 sync /mnt/backups/neo4j/ s3://your-bucket/neo4j-backups/ --delete
```

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `NEO4J_CONTAINER` | `visionflow-neo4j` | Docker container name |
| `NEO4J_USER` | `neo4j` | Database username |
| `NEO4J_PASSWORD` | `changeme-dev` | Database password |
| `BACKUP_RETENTION_DAYS` | `7` | Days to keep old backups |

## Troubleshooting

**"neo4j-admin dump failed"** — Community Edition cannot do online dumps. The script falls back to APOC export automatically. If APOC is also unavailable, it uses raw cypher-shell output.

**"Container not found"** — Check `docker ps` and ensure the container name matches. Override with `NEO4J_CONTAINER=name`.

**"Cypher replay failed"** — The backup may contain syntax incompatible with the current Neo4j version. Check `docker logs visionflow-neo4j` for details.

**Empty restore (0 nodes)** — The backup file may be corrupt or from an empty database. Check the backup file size and inspect its contents (`zcat backup.cypher.gz | head -50`).

**Restore takes too long** — Large Cypher replays are slow. Binary dump/load is much faster for large databases. If only Cypher backups are available, consider increasing the container memory limits.
