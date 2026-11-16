---
id: ADR-2103
title: Give Oxigraph a point-in-time backup with a declared RPO and RTO
date: 2026-09-05
decision_status: proposed
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 5a9b6fc3801c17c4e45ae71b974ea8d609553936
verified_paths: [crates/visionclaw-backup/src/run.rs, crates/visionclaw-backup/src/checkpoint.rs, crates/visionclaw-backup/tests/restore_roundtrip.rs, src/services/ontology_checkpoint.rs, src/services/data_reconciliation.rs, docs/how-to/store-backup-restore.md]
owner: jjohare
review_trigger: an Oxigraph/RocksDB data-loss incident, a restore drill, or any change to scripts/backup-sqlite.sh's membership contract
repo: visionclaw
domain: DATA-authority-erasure
lineage: extends ADR-2017 (SQLite-only online backup, no PITR for Oxigraph); ADR-2069 (required/optional backup membership contract); DATA-authority-erasure "Backup coverage is partial — Oxigraph only"
---

# ADR-2103 — Give Oxigraph a point-in-time backup with a declared RPO and RTO

## Context

Diagram **VC-22.7** (`22-data-authority-provenance-erasure.md:329`) records that Oxigraph
has no point-in-time backup, no cross-store consistent restore and no declared RPO or
RTO. ADR-2017 established the backup posture for the four SQLite databases via the SQLite
online-backup API, and ADR-2069 hardened its membership contract, but both are explicitly
SQLite-only: `scripts/backup-sqlite.sh` is the tree's only backup script. ADR-2017's own
Consequences state Oxigraph recovery is "re-sync from GitHub", which reconstructs the
disposable `:assert` projection but **not** the derived and provenance graphs. Since
`GRAPH_PROVENANCE` is append-only and never re-derivable from the corpus (ADR-2016), a
RocksDB loss is an unrecoverable loss of the estate's only audit trail.

## Decision

**Proposed.** Oxigraph is brought under the same backup contract as SQLite, with numbers
attached. When taken:

1. **Backup exists and is scheduled.** A backup routine takes a consistent point-in-time
   snapshot of the Oxigraph/RocksDB store — via a RocksDB checkpoint (hard-linked, so
   near-instant and low-space) or a full store export, whichever the deployed binding
   supports — writing to the same destination discipline as `backup-sqlite.sh`, with a
   `MANIFEST.txt` carrying a sha256 per artefact.
2. **RPO and RTO are declared numbers, not aspirations.** The backup interval sets the
   RPO; a measured restore drill sets the RTO. Both are recorded in
   `docs/DATA-authority-erasure.md` and are re-measured whenever the store's size class
   changes. An undeclared RPO/RTO is treated as a failing backup.
3. **Membership is a contract, following ADR-2069.** The Oxigraph store is a **required**
   member: a failed or skipped Oxigraph snapshot aborts the run and publishes no
   manifest, exactly as a missing `REQUIRED_DBS` member does today.
4. **Graph classes are restored differently, and the difference is documented.**
   `:assert` is disposable and may be re-derived from GitHub instead of restored;
   derived/inferred graphs follow whichever is faster; `GRAPH_PROVENANCE` is
   restore-only, because it cannot be re-derived. The restore procedure states this per
   graph rather than treating the store as one opaque blob.
5. **Cross-store consistency remains explicitly out of scope.** ADR-2017 forecloses 2PC.
   This ADR does not claim a cross-store consistent restore; it claims a per-store
   point-in-time one, and the skew window between the SQLite and Oxigraph snapshots is
   stated as a known quantity (bounded by the interval between the two snapshot steps in
   one backup run).

## Consequences

- The provenance graph stops being a single-copy, unrecoverable artefact — the largest
  durability hole after the erasure gaps in ADR-2102.
- Cost: a new script and schedule, storage for RocksDB checkpoints (cheap while
  hard-linked, expensive once the store churns), and a restore drill that must actually
  be run rather than assumed, since the RTO is defined by measurement.
- ADR-2017 moves closer to `complete` but is **not** superseded: its per-class
  write-master decision and its no-2PC stance are unchanged, and this record extends only
  its backup half.
- A stated skew window is a weaker guarantee than cross-store consistency, and any
  consumer relying on a consistent restore must be corrected rather than accommodated.

## Verification

`implementation_status: none`. Verified at `b0bc275f6501aae7751b85a72ce15fe1e730e7e8`:
`scripts/backup-sqlite.sh` is the only backup script in the tree, its `REQUIRED_DBS`
(`:75`) names three SQLite databases and no triple store, and `docs/DATA-authority-erasure.md`
states "**Oxigraph has NO point-in-time backup.** There is no cross-store consistent
restore and no declared RPO/RTO."

**Acceptance test for the landing change.**

1. Run the backup with a live writer mutating the store. The run completes, publishes a
   `MANIFEST.txt` with a sha256 per artefact, and the snapshot is readable.
2. Restore the snapshot into an empty store. A SPARQL count over `GRAPH_PROVENANCE`
   matches the count at snapshot time, and the `append_only_verified` property still
   holds on the restored store.
3. Time step 2 from a cold start; the measured figure is the declared RTO and appears in
   `docs/DATA-authority-erasure.md` alongside the declared RPO (the schedule interval).
4. Force the Oxigraph snapshot step to fail. The whole run aborts and publishes **no**
   manifest — the ADR-2069 required-member semantics, not a skip-and-continue.
5. `docs/DATA-authority-erasure.md` no longer contains the sentence "Oxigraph has NO
   point-in-time backup", and states the per-graph restore policy from Decision item 4.

## Disposition — 2026-10-02

- **Suitability:** fits, needs revision
- **Priority:** P2 — next cycle (TODO row G-7: reopens once a real deployment exists to set a backup policy for, i.e. after the CY-C fresh-host bring-up)
- **Why:** `implementation_status: none` is out of date. An opt-in checkpoint taken from the open writer now exists: `ONTOLOGY_BACKUP_DIR` at `src/main.rs:728-750` calls `checkpoint_oxigraph` (`src/services/data_reconciliation.rs:284-297`, last touched in `1ad881cab`). It runs once at boot, aborts boot on failure, and writes no `MANIFEST.txt`. It is not called from `scripts/backup-sqlite.sh`, so decision items 1 to 3 (schedule, declared RPO/RTO, required membership) are still unmet. G-7 records the checkpoint and source-loss restore fixture passing, and the deployment-specific policy as unverified. This is the host record. Planning-cycle §10's "ADR-2103 P21 gate" refers to agentbox ADR-2103, a different record.
- **Next:** Restate the verification against the boot-time checkpoint, then bind it into the backup run with a manifest and one timed restore drill on the deployed host.

## Implementation on branch feat/store-backup-and-restore — 2026-11-16

**Built and tested against synthetic stores. Not merged, not scheduled, not run
against the live stores.** Owner decision G-7 (16 November): a nightly dump of
Oxigraph and the SQLite stores to the NAS, plus one recorded restore test.

**What now exists.** `crates/visionclaw-backup` (binary `visionclaw-backup`, with
`backup`, `verify` and `restore`). Following the code as it is, it departs from the
design above in three ways:

1. **Dump, not checkpoint.** The artefact is an N-Quads dump of every graph from one
   read snapshot (portable across Oxigraph versions), not a RocksDB checkpoint.
   A checkpoint (`Store::backup`) is the *source* when the server is running.
2. **A separate process cannot read a live store.** Oxigraph 0.4.11 has no secondary
   or shared-read mode; `Store::open` takes the RocksDB lock and `open_read_only`
   beside a live writer is documented as undefined behaviour. The tool opens the store
   read-write and fails by name when a server holds it. Decision item 1 ("backup
   exists and is scheduled") therefore still needs the owner to pick a live trigger:
   a periodic in-server checkpoint, an authenticated admin route, or a stop window
   (resolved on 2026-11-16: the owner chose the periodic in-server checkpoint; see the next section).
3. **Manifest is `manifest.json`, not `MANIFEST.txt`,** with size and SHA-256 per
   artefact, quad count or per-table row counts, and a SQLite content digest.

Decision item 3 (required membership) is met: Oxigraph and `settings`, `enrichment`,
`kpi` and `shares` are required; `liveness` is optional. Any failure deletes the
partial run (built under a `.partial` name, renamed when complete) and exits
non-zero. Retention keeps the newest 14 complete runs by default.

**Acceptance status.** Item 4 (forced failure publishes no run) is covered by
`missing_required_store_fails_and_publishes_nothing` and the held-store test. Items 1
and 2 are covered on synthetic stores only (round trip equal quads and a SPARQL count
over a named graph); the live-writer variant of item 1 is **not** met, for the reason
in point 2. Item 3 (a timed cold restore on the deployed host, and a declared RPO and
RTO) is **not done**. Item 5 is partly done in `docs/DATA-authority-erasure.md`; the
per-graph restore policy of Decision item 4 is not yet written.

**Evidence.** `docs/estate-closeout/2026-11-16/store-backup-restore-test.md`
(`cargo test -p visionclaw-backup`: 9 passed plus 1 doc-test, synthetic stores in
temporary directories). **Operator guide.** `docs/how-to/store-backup-restore.md`.

**Status.** `decision_status` stays `proposed` (the owner has chosen the direction in
G-7 but has not accepted this record) and `activation_status` stays `inactive`.

## Owner ruling and checkpoint implementation — 2026-11-16

**Owner ruling (16 November 2026).** The server writes a periodic Oxigraph checkpoint
itself, and the backup tool backs that checkpoint up. The backup destination is
`/mnt/dell` (an NFS mount, visible in the agentbox container). This resolves the open
choice of live trigger recorded above (periodic checkpoint, admin route or stop window):
the periodic checkpoint is chosen; the admin route and the stop window are not built.

**What is built on the branch (and nowhere else).**

- `src/services/ontology_checkpoint.rs`: a background task, started from `src/main.rs`,
  that runs `checkpoint_oxigraph` at start and then every
  `ONTOLOGY_CHECKPOINT_INTERVAL_SECS` (default 21600; `0` disables, including the first
  run) into `ONTOLOGY_CHECKPOINT_DIR` (falling back to `ONTOLOGY_BACKUP_DIR`; with
  neither set it does not run, because a default location outside the data volume
  would be an owner choice). Each run is on `spawn_blocking`, logs one line on success
  and a warning on failure, and shutdown waits for a run in progress.
- `crates/visionclaw-backup/src/checkpoint.rs`: the one checkpoint implementation.
  `checkpoint_oxigraph` now delegates to it. It calls `Store::backup` (a RocksDB
  checkpoint of the open transactional database: point-in-time, no writer pause),
  builds under a `.partial` name, counts quads from the checkpoint itself, writes the
  `checkpoint.json` sidecar (taken time, quad count, format) last, renames, and keeps
  the newest two. A folder without a parsable sidecar is never complete.
- `visionclaw-backup backup --checkpoint-dir` dumps the latest complete checkpoint
  read-only, refuses one older than `--max-checkpoint-age-secs` (default 24 hours),
  checks the dump's quad count against the sidecar, and records the checkpoint name,
  time and count in `manifest.json` (`oxigraph_checkpoint`). A direct dump of an unheld
  store still works.

**Behaviour change.** The boot-time `ONTOLOGY_BACKUP_DIR` checkpoint used to abort
start-up on failure and write once; it is now the task's first run, runs after start-up
completes its other work, and a failure is a warning retried at the next interval.

**Tests.** `cargo test -p visionclaw-backup`: 6 unit and 13 integration tests and one
doc-test (complete checkpoint, atomic replace, keep-two, interrupted write never
visible, stale refusal, fresh backup, restore from a checkpoint-sourced run); in the
server, `services::ontology_checkpoint` and `services::data_reconciliation`
(8 tests). All on synthetic stores.

**Status, kept honest.** `decision_status` stays `proposed` (the owner has ruled on the
trigger and destination but has not accepted this record). `implementation_status`
stays `partial`: built and tested on a branch; not merged. `activation_status` stays
`inactive`: nothing is scheduled, no checkpoint directory is configured, the task is not
running, nothing has been written to `/mnt/dell`, and no restore has been timed on live
data (acceptance items 1 live-writer variant and 3 remain unmet). Steps to switch it on
are in `docs/how-to/store-backup-restore.md`.
