# How-to: back up and restore the VisionClaw stores (nightly dump, verify, restore)

`visionclaw-backup` (crate `crates/visionclaw-backup`) backs up the Oxigraph
store and the SQLite databases in one run, checks a run against its manifest,
and rebuilds the stores from a run into an empty directory. It implements
ADR-2103 and extends ADR-2017; the owner's decision is register row G-7.

**Status: built and tested against synthetic stores. It is not scheduled and has
not been run against the live stores.** See [What is not yet switched on](#what-is-not-yet-switched-on).

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
and the vault; the ledger and pod data are not rebuildable and have no backup.

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
| `VISIONCLAW_BACKUP_DEST` | `--dest` | none, **required** | Folder that receives the dated runs (the NAS mount). Refused if inside the data directory. |
| `DATA_DIR` | `--data-dir` | `./data` | The server's data directory (same variable the server reads). |
| `VISIONCLAW_BACKUP_OXIGRAPH_DIR` | `--oxigraph-dir` | `<DATA_DIR>/oxigraph` | Store or checkpoint directory to dump. |
| `VISIONCLAW_BACKUP_KEEP` | `--keep` | 14 | Complete runs to keep. |

## Commands

```bash
cargo build --release -p visionclaw-backup      # binary: target/release/visionclaw-backup
visionclaw-backup backup  --dest /mnt/nas/visionclaw-backups
visionclaw-backup verify  --run  /mnt/nas/visionclaw-backups            # newest run, or a run folder
visionclaw-backup restore --run  /mnt/nas/visionclaw-backups --target /srv/restore-test   # must be empty
```

Every command exits non-zero on failure. `restore` verifies the run first,
refuses a non-empty target, and checks each restored store against the manifest
(quad count; SQLite integrity, row counts and content digest). Restored
`oxigraph/` is a normal Oxigraph store (blank-node labels are re-minted on load, which RDF treats as the same graph; named graphs and every other term are preserved); the SQLite files drop in under their own names.

## Oxigraph and a running server

Oxigraph on RocksDB admits one read-write process, and its documentation calls a
read-only open beside a live writer undefined behaviour. The tool opens the
store read-write, so against a running server it **fails by name** instead of
reading unsafely. The server writes a consistent checkpoint (`Store::backup`) only **once, at boot**,
when `ONTOLOGY_BACKUP_DIR` is set (`src/main.rs`), into a new `ontology-<uuid>`
directory that is never pruned. The tool can dump such a directory while the server
stays up, but a nightly run pointed at it would dump the same boot-time state every
night. The SQLite databases are safe to snapshot live.

So the nightly run needs one of these, which is the owner's choice:

1. a periodic in-server checkpoint (a small change to the server, not made here);
2. an authenticated admin route that writes a checkpoint (also a server change);
3. a short stop window around the run (`ExecStartPre`/`ExecStopPost` in the example unit). The tool opens the stopped store read-write, which writes new RocksDB files (MANIFEST, OPTIONS, LOG); run it as the user that owns the volume (the server runs as `appuser`), or the server may be unable to open the store afterwards.

## Schedule

The repository schedules nothing itself (no cron, supervisor or compose job; the
cron and supervisord snippets in `sqlite-backup-restore.md` are examples). An
example systemd service and timer are in `scripts/systemd/`
(`*.example`; nothing installs them). They run the tool nightly at 03:15 and mark
the unit failed on a non-zero exit.

## What is not yet switched on

1. Set `VISIONCLAW_BACKUP_DEST` to the NAS folder (the one required line).
2. Choose the Oxigraph trigger above and set `VISIONCLAW_BACKUP_OXIGRAPH_DIR` (or the stop window).
3. Build the binary on the host and copy it to `/usr/local/bin`; the production image ships only `visionclaw-server`.
4. Copy and edit the two example units, then `systemctl enable --now visionclaw-backup.timer`.
5. Run `verify` on the first run, then a `restore` into an empty directory on the live data and time it: that figure is the RTO ADR-2103 asks for. The schedule interval is the RPO (24 hours).

Evidence that the tool works: `docs/estate-closeout/2026-11-16/store-backup-restore-test.md`
(synthetic stores only).
