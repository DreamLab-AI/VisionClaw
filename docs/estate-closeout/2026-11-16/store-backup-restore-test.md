# Store backup and restore test, 16 November 2026 (register row G-7, ADR-2103)

**Scope, stated plainly:** this was run against **synthetic stores in temporary
directories**. It was **NOT** run against the live Oxigraph store, the live SQLite
databases, the `visionclaw_container` container or the `visionclaw-data` volume.
No live restore has been performed and no RTO has been measured.

Code under test: `crates/visionclaw-backup` on branch
`feat/store-backup-and-restore` (base `dreamlab-github/main`; the commit that adds
this record is the code state tested). Oxigraph 0.4.11,
rusqlite 0.31.0 (bundled SQLite).

Command: `cargo test --offline -p visionclaw-backup`

```
Running tests/restore_roundtrip.rs
test destination_inside_the_data_directory_is_refused ... ok
test missing_required_store_fails_and_publishes_nothing ... ok
test missing_optional_store_is_recorded_and_does_not_fail ... ok
test oxigraph_held_by_a_running_server_fails_by_name ... ok
test verify_detects_an_altered_artefact ... ok
test restore_refuses_a_non_empty_target_and_leaves_it_alone ... ok
test a_checkpoint_directory_can_be_backed_up_while_the_store_stays_open ... ok
test backup_then_restore_reproduces_every_store ... ok
test retention_keeps_the_newest_runs_and_ignores_partial_folders ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.88s
Doc-tests visionclaw_backup: 1 passed; 0 failed
```

## What the round-trip test proves

`backup_then_restore_reproduces_every_store` builds, in a temporary directory, an
Oxigraph store (one default-graph quad and six quads in a named provenance graph, among them
a blank-node subject with two quads, as OWL restrictions have, and one literal
containing a newline, quotes and a non-ASCII letter) and five SQLite
databases in WAL mode with open writer connections, so committed rows may sit in
the WAL. It backs them up, verifies the run, restores the newest run into a second
empty directory, then asserts:

- the restored Oxigraph holds the same quads (7, every graph). Blank-node labels are re-minted on load, so
  the comparison is by isomorphism: each blank node is matched by the statements it
  takes part in;
- a SPARQL `COUNT` over the provenance graph is 3 in both stores;
- for each SQLite database the content digest (schema plus every row, ordered)
  and the row count of `items` equal the original's.

## What it does not prove

- Not a live run; the live store is held by the running server, and the tool
  refuses such a store by name (`oxigraph_held_by_a_running_server_fails_by_name`).
  A live nightly run needs an Oxigraph checkpoint source or a stop window (see
  `docs/how-to/store-backup-restore.md`).
- Store sizes here are a few rows. Dump time, restore time and disk use at the live
  store's size are unmeasured, so ADR-2103's RTO is still undeclared.
- Nothing is scheduled; the systemd files in `scripts/systemd/` are examples.

## Addendum: server checkpoints (owner ruling, 16 November 2026)

Same scope: synthetic stores in temporary directories, nothing live, nothing written to
`/mnt/dell`. After the periodic checkpoint was added:

- `cargo test -p visionclaw-backup`: 6 unit tests passed (`checkpoint::tests`), 13
  integration tests passed (adds a fresh checkpoint backed up and restored to the same
  quads with the checkpoint recorded in the manifest, a stale checkpoint refused with
  nothing published, an empty checkpoint directory refused), 1 doc-test passed.
- `cargo test --lib -p visionclaw-server -- services::ontology_checkpoint services::data_reconciliation`:
  8 passed (the task writes at start, refreshes each interval and stops cleanly; the
  existing source-loss restore of `checkpoint_oxigraph` still passes).
- `cargo clippy -p visionclaw-backup --all-targets -- -D warnings`, `cargo fmt --check` and
  `cargo doc -p visionclaw-backup --no-deps`: clean.

The existing restore round-trip test passes unchanged in behaviour; the field
`oxigraph_dir` became `oxigraph: OxigraphSource`.
