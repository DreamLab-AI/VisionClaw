# How-to: back up and restore the VisionClaw stores (nightly dump, verify, restore)

`visionclaw-backup` (crate `crates/visionclaw-backup`) backs up the Oxigraph
store and the SQLite databases in one run, checks a run against its manifest,
and rebuilds the stores from a run into an empty directory. It implements
ADR-2103 and extends ADR-2017; the owner's decision is register row G-7.

**Status (2026-11-16): merged and deployed. A real stopped-server backup has
verified and restored successfully into an empty scratch directory.** Host
checkpoint/schedule activation evidence is recorded in
[the deployment receipt](../estate-closeout/2026-11-16/deployment-rollout.md).

Deployment integration (2026-11-16): compose now mounts a dedicated
`visionclaw-checkpoints` volume outside `/app/data`. The dev Solid root
`/data/solid` also receives a named `visionclaw-solid` volume: existing
container-layer data must be copied and verified while the backend is stopped
before the first recreation. Keep the original container and an independent
private recovery copy until that migration is verified. This Solid copy is
not part of the store-backup tool's coverage.

## The stores

| Store | Path under the data directory | Compose volume | Irreplaceable? |
|---|---|---|---|
| Oxigraph (RocksDB), all graphs | `oxigraph/` (`src/app_state.rs:407`) | `visionclaw-data` at `/app/data` | **Partly.** `:assert` is rebuilt from GitHub; the provenance graph is append-only and cannot be re-derived (ADR-2016). Treat the store as irreplaceable. |
| `settings.sqlite3` (also the RBAC role store) | `app_state.rs:417` | same | **Yes**: authored configuration |
| `enrichment.sqlite3` | `app_state.rs:430` | same | **Yes**: proposal and decision ledger |
| `kpi.sqlite3` | `app_state.rs:475` | same | **Yes**: KPI lineage series |
| `shares.sqlite3` | `app_state.rs:484` | same | **Yes**: shared graph records |
| `liveness.sqlite3` | `app_state.rs:445` | same | No: canary telemetry, recreated on boot (optional member) |

Not covered by this tool (listed so the gap is visible): the payment and
exchange ledger (`cfg.ledger_dir`), Solid pod data (`/app/data/solid`),
`/app/data/metadata` and `/app/data/markdown`. The corpus is sourced from GitHub
and the vault; the ledger and pod data are not rebuildable and have no recurring
backup from this tool. The November deployment retains a separate one-off
Solid migration archive; that does not close recurring backup coverage.

## What a run contains

`<destination>/<UTC stamp>/` holds `oxigraph.nq` (N-Quads, every named graph),
one snapshot per SQLite database (SQLite online backup API, integrity-checked,
single file) and `manifest.json` (per artefact: size, SHA-256, quad count or
per-table row counts and a content digest). The run is built as
`.<stamp>.partial` and renamed only when complete; a failed run is deleted.
Required members are Oxigraph and `settings`, `enrichment`, `kpi`, `shares`; a
missing one fails the whole run (same contract as `scripts/backup-sqlite.sh`,
ADR-2069). A missing `liveness` is recorded in the manifest and skipped.
The newest `VISIONCLAW_BACKUP_KEEP` complete runs are kept (default 14).

## Settings

| Setting | Flag | Default | Meaning |
|---|---|---|---|
| `VISIONCLAW_BACKUP_DEST` | `--dest` | none, **required** | Folder that receives the dated runs (`/mnt/dell`, an NFS mount). Refused if inside the data directory. |
| `DATA_DIR` | `--data-dir` | `./data` | The server's data directory (same variable the server reads). |
| `VISIONCLAW_BACKUP_CHECKPOINT_DIR` | `--checkpoint-dir` | none | The server's `ONTOLOGY_CHECKPOINT_DIR`. When set, Oxigraph is dumped from the latest complete checkpoint there (the route for a running server). Conflicts with `--oxigraph-dir`. |
| `VISIONCLAW_BACKUP_MAX_CHECKPOINT_AGE_SECS` | `--max-checkpoint-age-secs` | 86400 (24 h) | A checkpoint older than this is refused with an error naming it. |
| `VISIONCLAW_BACKUP_OXIGRAPH_DIR` | `--oxigraph-dir` | `<DATA_DIR>/oxigraph` | A store (stopped, or a bare `Store::backup` directory) to dump directly. A store held by a server is refused. |
| `VISIONCLAW_BACKUP_KEEP` | `--keep` | 14 | Complete runs to keep. |

## Commands

```bash
cargo build --release -p visionclaw-backup      # binary: target/release/visionclaw-backup
visionclaw-backup backup  --dest /mnt/dell/visionclaw-backups --checkpoint-dir <ONTOLOGY_CHECKPOINT_DIR>   # server running
visionclaw-backup backup  --dest /mnt/dell/visionclaw-backups                                            # server stopped: dumps <DATA_DIR>/oxigraph
visionclaw-backup verify  --run  /mnt/dell/visionclaw-backups            # newest run, or a run folder
visionclaw-backup restore --run  /mnt/dell/visionclaw-backups --target /srv/restore-test   # must be empty
```

Every command exits non-zero on failure. `restore` verifies the run first,
refuses a non-empty target, and checks each restored store against the manifest
(quad count; SQLite integrity, row counts and content digest). Restored
`oxigraph/` is a normal Oxigraph store (blank-node labels are re-minted on load, which RDF treats as the same graph; named graphs and every other term are preserved); the SQLite files drop in under their own names.

## Oxigraph and a running server

Oxigraph on RocksDB admits one read-write process, and its documentation calls a
read-only open beside a live writer undefined behaviour (0.4.11 has no shared-read
mode). A second process therefore cannot dump the store while the server holds it,
and the tool, opening it read-write, **fails by name** rather than reading unsafely.

So the server checkpoints itself (owner decision, 16 November 2026). A background task
in the server calls `Store::backup`, which asks RocksDB for a checkpoint of the open
transactional database: a point-in-time image (immutable SST files hard-linked when on
the same filesystem), consistent without pausing writers. It runs off the async
worker threads, once at start and then every interval, and logs one line per run.

| Setting (server) | Default | Meaning |
|---|---|---|
| `ONTOLOGY_CHECKPOINT_DIR` | none (falls back to the older `ONTOLOGY_BACKUP_DIR`) | Where checkpoints are written. Must be outside the data directory. With neither variable set the task does not run and logs why. |
| `ONTOLOGY_CHECKPOINT_INTERVAL_SECS` | 21600 (6 h) | Gap between checkpoints. `0` disables the task, including its first run. |

Each checkpoint is a folder `checkpoint-<UTC stamp>/` holding `store/` (the RocksDB
checkpoint) and `checkpoint.json` (when it was taken, the quad count, the format). It
is built as `.checkpoint-<stamp>.partial` and renamed only when both are on disk, so a
reader never sees a half-written one; a folder without a parsable sidecar is not a
checkpoint. The newest two complete checkpoints are kept. Old `ontology-<uuid>`
folders from the previous boot-only checkpoint are not touched; delete them by hand.

`visionclaw-backup backup --checkpoint-dir <dir>` dumps the latest complete checkpoint
(opened read-only, so the checkpoint itself is not altered), checks its quad count
against the sidecar, refuses one older than the maximum age (default 24 hours, so a
stalled checkpoint task fails the nightly run loudly instead of backing up stale data),
and records in `manifest.json` under `oxigraph_checkpoint` the checkpoint's name, when it
was taken and its quad count. The SQLite databases are safe to snapshot live.

Disk: `link(2)` does not cross mount points, so a checkpoint directory on a separate
Docker volume gets copied SSTs, not hard links; size that volume for about two full
copies of the store (the newest two checkpoints are kept).

Consequence for the RPO: Oxigraph is as old as the latest checkpoint (up to the
checkpoint interval plus the backup interval), the SQLite files as old as the run.

## Where the backups go: `/mnt/dell`

The owner chose `/mnt/dell`; the default destination in the example unit and
`env.example` is `/mnt/dell/visionclaw-backups`. That path is an NFS mount visible in
the agentbox container. It must also be mounted on the host or container where
`visionclaw-backup` runs (the example unit's `RequiresMountsFor` makes systemd wait for
it), and the tool must be able to read the server's `ONTOLOGY_CHECKPOINT_DIR` and
`DATA_DIR` there. The checkpoint directory should be a mounted volume of the server, or
the host will not see it.

## Schedule

The repository schedules nothing itself (no cron, supervisor or compose job; the
cron and supervisord snippets in `sqlite-backup-restore.md` are examples). An
example systemd service and timer are in `scripts/systemd/`
(`*.example`; nothing installs them). They run the tool nightly at 03:15 and mark
the unit failed on a non-zero exit.

## Deployment checklist

For another installation, explicitly provision and verify each step:

1. Merge `feat/store-backup-and-restore` and rebuild the server image (the checkpoint
   task is in the server binary). On the production image, add a volume for the
   checkpoint directory.
2. Set `ONTOLOGY_CHECKPOINT_DIR` for the server to that directory (outside `/app/data`).
   The interval needs no setting unless 6 hours is wrong.
3. Build `visionclaw-backup` on the host and copy it to `/usr/local/bin`; the production image ships only `visionclaw-server`.
4. Mount `/mnt/dell` on the host, copy and edit the two example units (set
   `VISIONCLAW_BACKUP_CHECKPOINT_DIR` to the checkpoint directory as the host sees it,
   and `DATA_DIR`), then `systemctl enable --now visionclaw-backup.timer`.
5. Check the server log for `ontology checkpoint: wrote`, run `visionclaw-backup verify` on the first run, then a `restore` into an empty directory on the live data and time it: that figure is the RTO ADR-2103 asks for.

Evidence that the tool works: `docs/estate-closeout/2026-11-16/store-backup-restore-test.md`
(synthetic fixtures), plus the linked deployment receipt (real stores).

The deployed host uses a user-systemd Docker one-shot, not direct traversal of
Docker's private volume directories: data and checkpoints are read-only mounts,
network is disabled, the executable is mounted read-only, output is the private
NAS directory, and `UMask=0077` applies inside and outside the container. The
example host-binary unit remains a template, not an installer. Both approaches
must refuse a missing NAS mount. Keep the one-shot's image and executable when
reclaiming old runtime images.
