---
id: ADR-2004
title: Embedded Oxigraph plus per-writer SQLite owns canonical graph and local state
date: 2026-08-31
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: bb76e75bdf2c2f86cf0ce2c13ea4296f42924582
verified_paths: [Cargo.toml, src/app_state.rs]
owner: jjohare
review_trigger: a scale requirement that exceeds a single-node embedded store, or any proposal to reintroduce a networked graph database
repo: visionclaw
domain: BASELINE-architecture
lineage: Distils legacy ADR-132 (Neo4j removal, Oxigraph+SQLite adoption; cutover 2026-05-20) and its ADR-101 versioning regime / ADR-098-100 IRI-provenance migrations.
---

# ADR-2004 — Embedded Oxigraph plus per-writer SQLite owns canonical graph and local state

## Context

The graph/ontology data needs SPARQL 1.1 and durable triples; the non-triple
state (settings, enrichment lifecycle, liveness, KPI) needs a single-writer
transactional store. A networked graph DB (Neo4j) added an operational
dependency, a second query dialect, and a cross-process consistency problem the
deployment does not need. Prior state carried both. Lineage: ADR-132 cutover
(2026-05-20), ADR-101 versioning, ADR-098-100 IRI-provenance migrations.

## Decision

The canonical graph/ontology store is **embedded Oxigraph** (RocksDB-backed,
SPARQL 1.1), opened exactly once at `data/oxigraph` and shared: the graph
repository is derived `from_store(...)` off the same handle the ontology
repository opens. The settings, enrichment, liveness and KPI state lives in **per-writer SQLite files** under
`DATA_DIR` (`settings`, `enrichment`, `liveness`, `kpi`.sqlite3), one file per
single-writer to keep migration and lock posture isolated. Neo4j and any
external or networked graph database are forbidden.

## Consequences

- The canonical graph and named local state stores are process-local and back up as files. This does not cover every optional session persistence path.
- The store is bound to one node: horizontal scale-out is foreclosed without a
  new ADR. Oxigraph/RocksDB has no PITR (see ADR-2017 for backup posture).
- Sharing one handle means a corrupt or locked store takes down both
  repositories together — accepted for a single-operator deployment.

## Verification

`Cargo.toml:80` pins `oxigraph = { version = "0.4" }`; no `neo4rs` dependency
anywhere (the only match is a comment asserting its absence). `src/app_state.rs`
opens `OxigraphOntologyRepository::open(&oxigraph_path)` (~:451) and derives
`OxigraphGraphRepository::from_store(...)` (~:456). The four SQLite files are
opened under `data_dir` at ~:459 (settings), ~:472 (enrichment), ~:487
(liveness), plus the KPI store. Verified at `e0f8cd896`.

## Closeout extension — 2026-09-04

**Work package:** CP-01 / CP-06 / CP-08. **Owner:** existing owner above. Dependencies are
CP-01 revision/ownership mapping and the relevant corpus or authority contract.

**Current evidence:** Source confirms AppState opens one Oxigraph handle and shares it between ontology and graph repositories, plus separate SQLite files. This verifies storage composition, not a cross-store transaction or consistent serving generation.

See [runtime analysis](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/visionclaw-data-runtime.md),
[source hashes](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/visionclaw-data-snapshot.json)
and [backup receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/visionclaw-backup-probe.json).
Source was inspected at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`. Earlier verification at `4fed5663dfbc0940c6b19a175dfcc8a9c67f2ab8`
remains historical evidence; this annex does not claim a new deployed activation
or complete verification of every older assertion.

**Acceptance still required:** Record per-store commit points, actor reload/generation identity and a restore manifest. Test startup failures and restart against separately restored stores; do not infer consistency from the shared Oxigraph handle.

### Re-verification 2026-09-05 (ADR-2004)

Re-checked at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4` after `Cargo.toml` changed
since the previous `verified_commit` (`4fed5663`). `verified_paths` is emptied and
`verified_commit` set to current HEAD for this pass; **both must be restored at the
landing commit** (`verified_paths: [Cargo.toml, src/app_state.rs]` plus that
commit's SHA) so the staleness check regains its teeth.

Claim-by-claim against the Verification section above:

- **Oxigraph pin — line drift, claim holds.** The dependency is at
  **`Cargo.toml:82`**, not `:80`: `oxigraph = { version = "0.4" }`. The version
  constraint is unchanged.
- **No Neo4j — holds.** `grep -n "neo4rs" Cargo.toml` returns nothing; the only
  matches anywhere in the tree remain comments asserting its absence.
- **One shared Oxigraph handle — holds, with exact lines.** `src/app_state.rs:448`
  reads `DATA_DIR` (default `./data`), `:449` joins `oxigraph`, `:451` opens
  `OxigraphOntologyRepository::open(&oxigraph_path)`, and `:456` derives
  `OxigraphGraphRepository::from_store(oxigraph_store)` off that same handle. The
  Decision's "opened exactly once and shared" is exact at this commit.
- **Four per-writer SQLite files — holds; the KPI line is now recorded.** Under
  `data_dir`: `settings.sqlite3` path `:459`, `SqliteSettingsRepository::open`
  `:461`; `enrichment.sqlite3` `:472`, `SqliteEnrichmentRepository::open` `:474`;
  `liveness.sqlite3` `:487`, `SqliteCanaryRepository::open` `:489`; and the KPI
  store, previously cited only as "plus the KPI store", is `kpi.sqlite3` at
  **`:517`** with `SqliteKpiRepository::open` at **`:519`**.
- **New observation, relevant to the storage decision.** `Cargo.toml:250` sets
  `default = ["gpu", "ontology", "persistence-oxigraph", "solid-pod-embed"]`, and
  `persistence-oxigraph` (`:266`) is an empty marker feature — it gates nothing at
  this commit. The Oxigraph dependency and the open calls above are unconditional,
  so the feature name currently documents intent rather than enforcing a choice.
  A future alternative-substrate ADR must either wire this feature or delete it.

The 2026-09-04 closeout extension's limits are unaffected and are retained in full:
this re-verification confirms **storage composition only** — it establishes no
cross-store transaction, no actor reload/generation identity and no restore
correctness, and consistency must not be inferred from the shared handle.

**Re-confirmed after the 2026-09-05 remediation edits to `Cargo.toml`.** ADR-2066
removed the `quinn`/`rustls`/`rcgen` dependencies along with the unwired QUIC
transport server. Every citation above was re-read afterwards and all still land
on the claimed line: `Cargo.toml:82` `oxigraph = { version = "0.4" }`, `:250`
`default = ["gpu", "ontology", "persistence-oxigraph", "solid-pod-embed"]`, `:266`
`persistence-oxigraph = []`, and `src/app_state.rs:448/449/451/456` (`DATA_DIR`
read, `oxigraph` path join, `OxigraphOntologyRepository::open`,
`OxigraphGraphRepository::from_store`). `grep -n "neo4rs" Cargo.toml` remains
empty. The removed dependencies sat below the storage block, so no line shifted.
`node scripts/adr-index-gen.js docs/adr --check` → `ok: 72 ADR(s) valid`.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed changes since `b00c28a0d`:** `Cargo.toml` (-24/+14) and
`src/app_state.rs` (-50/+38). **Neither touches the persistence substrate.** The
`Cargo.toml` delta drops `quinn`/`rustls`/`rcgen` (direct deps only of the dead
`QuicTransportServer` removed under ADR-2066) and retires the `physics-v2`
feature (ADR-2055); the `app_state.rs` delta removes the standalone
`ShortestPathActor`/`ConnectedComponentsActor` pair under ADR-2053. The
persistence substrate is untouched by both.

**Substrate confirmed at HEAD, with the `~` citations made exact:**

| Claim | Cited | Actual at HEAD |
|---|---|---|
| `oxigraph = { version = "0.4" }` | `Cargo.toml:80` | **`Cargo.toml:82`** |
| Store opened once | `~:451` | `OxigraphOntologyRepository::open(&oxigraph_path)` at `src/app_state.rs:449`, path built at `:447` |
| Graph repo derived off the same handle | `~:456` | `OxigraphGraphRepository::from_store(oxigraph_store)` at `:454` |
| settings.sqlite3 | `~:459` | `:457` |
| enrichment.sqlite3 | `~:472` | `:470` (isolation rationale at `:469`) |
| liveness.sqlite3 | `~:487` | `:485` |
| kpi.sqlite3 | "plus the KPI store" | `:515` |

The one-handle-shared-by-both-repositories property — and therefore the accepted
consequence that a corrupt store takes both down together — is intact: `:454`
still derives the graph repository `from_store` the ontology repository opened at
`:449`.

**Neo4j still absent.** `grep -rn neo4rs --include=*.toml --include=*.rs`
(excluding `target/`) returns exactly one hit, and it is the negative assertion
in a comment: `crates/visionclaw-contracts/Cargo.toml:19` — "this crate … pulls
no actix, no neo4rs". No dependency, no client, no connection string.

**Commands run:** `git diff --stat b00c28a0d..HEAD -- Cargo.toml src/app_state.rs`
plus the full patches; `grep -n 'oxigraph|neo4rs|neo4j' Cargo.toml`; `grep -n`
over `app_state.rs` for the repository constructors and the four `.sqlite3`
paths; `grep -rn neo4rs` across the tree.

## Landing re-verification — 2026-09-06 (2cf222406)

Governed paths changed in the Wave 3 landing commit: src/app_state.rs: `validate_security_env_vars` made `pub(crate)` so AgentMonitorActor reuses it (ADR-2094); no persistence, Oxigraph or SQLite path changed. Decision unaffected; `verified_commit` moved to the landing commit. Gates at that commit: cargo check --workspace --all-targets exit 0, 827 crate + 1600 root + 309 xr-client tests, vitest 809, fmt and lint clean.

## Estate audit — 2026-09-07

The default graph/local-state decision remains accepted. `Cargo.toml:253` exposes the non-default `redis` feature, and `src/services/nostr_service.rs:149-187,245-299` configures Redis, restores sessions and persists them with SETEX when that feature is enabled. Redis is not a networked graph database, so its presence does not undo the Oxigraph decision. It does invalidate the universal claim that all non-triple persistence is SQLite. No Redis deployment was observed in this audit. Backup, erasure and release-profile inventories must include session Redis if enabled. See [VC-A11](../../../VisionFlow/docs/estate-review/2026-09-07-visionclaw-audit.md).

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed changes since `2cf222406`:** `Cargo.toml` gained a
`[profile.dev-runtime]` (inherits `release`, `debug-assertions` and
`overflow-checks` on) and `src/app_state.rs` changed two doc comments
(Kokoro → PocketTts in the local speech-stack description).

**Decision unaffected.** Neither touches persistence. At HEAD `Cargo.toml` still
declares `oxigraph = "0.4"`, `rusqlite = "0.31"` (bundled) and
`tokio-rusqlite = "0.5"`, and `persistence-oxigraph` is still in the default
feature set; the `app_state.rs` edits are comment-only and the store wiring they
sit beside is unchanged. `verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-09-22 at a32abac57f3a7cfe66ab68ea1b0faca013c0d6b2

**Governed changes since `997440cd0`:** `Cargo.toml` swapped the workspace member `crates/vault-migrate` for `crates/vault-core` + `crates/vault` and renamed the `sync_local`/`sync_github` bins to one `sync_corpus`; `src/app_state.rs` now builds the sync service over a `CorpusSource` (ADR-2114) instead of the GitHub API directly.

**Decision unaffected.** Neither touches persistence: `oxigraph = "0.4"`, `rusqlite = "0.31"` (bundled) and the `persistence-oxigraph` default feature are unchanged, and the `app_state.rs` edit is upstream of the store (which corpus is read), not the store itself. `verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-09-22 at 853c4a069 (Sovereign Corpus landing)

**Governed changes since `a32abac57`:** `Cargo.toml` adds the root dependency `vault-core = { path = "crates/vault-core" }` (ADR-2113: ingest parses pages and the relation vocabulary through the same crate `vault` builds from). **Decision unaffected.** No persistence dependency, feature or store path changed; Oxigraph remains the canonical store. `verified_commit` moved to the landing commit. Gates at that commit: vault 294 + vault-core 111 + golden parity 15/15; server lib 1,444; corpus_local_sync 4, vault_gate_test 18, jsonld_validator_test 3; client tsc clean; fmt and clippy -D warnings clean on the crates.

## Re-verification — 2026-10-03 at fdcbc9120fda25fd93fdaee744d68d2c00713d6b

`1d3e14a30` and `fdcbc9120` change `Cargo.toml`: solid-pod-rs and its siblings move to `=0.5.0-alpha.12` with feature `mrc20`, and `nostr-bbs-core` is patched to nostr-rust-forum `b73ec8c` (ADR-2111, S4 amendment). No oxigraph, rusqlite or persistence feature changed, and `src/app_state.rs` is untouched. The decision holds unchanged.

## Re-verification — 2026-10-03 at 780eb3edb788c9cb568d5756689a3c8455db79b7

`1e55daebb` drops the `[patch.crates-io]` git override for `nostr-bbs-core` from `Cargo.toml` in favour of the crates.io `=1.0.0-beta.13` pin. That changes no storage dependency. In `780eb3edb`, `src/app_state.rs` changes only the decision-projection client's key lookup (`:1362-1368`, now `load_panel_secret`). Oxigraph and the per-writer SQLite wiring are untouched. The decision holds.

## Re-verification — 2026-10-07 at ed5644d03 (live memory cloud, ADR-2133)

**Governed change:** `Cargo.toml` adds the workspace member `crates/visionclaw-memory-cloud` (pure logic, no server-layer dependency), root dependencies `tokio-postgres 0.7.18`, `deadpool-postgres 0.14.2` and the new crate, and a `[profile.dev.package.visionclaw-memory-cloud] opt-level = 3` override. Line citations into `Cargo.toml` after line 15 shift by +1, after line 128 by +8 and after line 310 by +14. **Decision unaffected.** Oxigraph, SQLite and their features are untouched; the new Postgres client is a read-only consumer of the external RuVector store, not a graph or local-state store. `verified_commit` moved to `ed5644d03`. Source reading of the diff (`git diff 20499efc6..ed5644d03` on the governed paths) plus `cargo check --lib --bins` and `cargo test --lib -- auth rbac memory_cloud` (62 + 5 pass) at the landing commit.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `src/app_state.rs`: three field-wise `Default` impls become derives; the SSSP map uses `SharedNodeSssp`; `format!` without arguments becomes `to_string()`; `fetch_update` is renamed `try_update`. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `Cargo.toml`: adds `subtle = "2.6.1"` as a direct dependency for the constant-time `X-Agent-Key` comparison (`utils::agent_key`).

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**

## Re-verification — 2026-10-07 at 6e89f6adb (fix/broadcast-timer merge)

`src/app_state.rs` changes only how the saved physics settings reach the GPU: boot sends `SetPhysicsSettingsSource` carrying the existing SQLite settings repository (`:1022`) and pushes them through `physics_restore::push_physics` (`:1078`); `get_gpu_compute_addr` refreshes a cached `ForceComputeActor` address that a supervisor restart left stale. Oxigraph is still opened once at `data/oxigraph` (`:425-427`), and the graph repository is still derived `from_store` off the same handle (`:431-432`). No store was added and no SQLite file was split or merged. **Still holds.** Checked by reading `git diff <previous verified_commit> 6e89f6adb` over this record's governed paths; the test suites were not re-run for this stamp.

## Re-verification — 2026-10-07 (ADR-2135: f275173a3, 08a3e2a41, 96d9c426d)

ADR-2135 adds the path dependency `visionclaw-tri-layout` to the root `Cargo.toml`. No store, persistence or SQLite/Oxigraph code changed. Decision holds. Verified at 96d9c426d on f95dc554f: server `cargo test --lib` 1,565 passed, 0 failed, 6 ignored; `cargo test -p visionclaw-tri-layout` 22 + 2 doc; xr-client `cargo test --workspace` 527 passed; client vitest 1,264; clippy `-D warnings` and fmt clean on the server lib, the new crate and the xr-client workspace.

## Re-verification — 2026-10-08 at 31bc3d3a1 (ADR-2135 amendment, ADR-2136)

`src/app_state.rs` changes only log text and comments: the boot physics logs name `axis_compression_z` instead of the retired `graph_separation_x`. Persistence is unchanged. Decision holds. Citations at 31bc3d3a1: `src/app_state.rs:715`, `:725` (boot log lines).

## Re-verification — 2026-10-08 at bb76e75bd (fix/startup-errors)

`src/app_state.rs` changes one log line: the ACSP client now says its relay is set rather than claiming a connection (`:1343`). Persistence is unchanged. Decision holds.
