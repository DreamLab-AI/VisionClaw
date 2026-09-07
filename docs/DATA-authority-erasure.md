---
title: Data Authority & Erasure
doc_id: VC-DATA
version: 0.2.1
status: draft-for-ratification
verified_commit: 
changelog:
  - "0.2.1 (2026-09-06): Remediation — 2026-09-05 section: Wave 3 ADRs (2094–2101, 2061, 2071, 2085; proposed 2102–2105) and the ledger/diagram re-verification landed in 2cf222406 — re-verified at "
  - "0.2.0 — Authored-content owner restated as the Obsidian vault gate (frontmatter `public: true` / `owl-class`, ADR-2040); citations marked `:~` pending re-verification against the vault reader."
  - "0.1.1 — fix GRAPH_PROVENANCE citation 53→55; widen named-graph range 50-55→46-55; add RBAC_ALLOW_OWNERLESS file:line citations"
sources:
  - crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs
  - src/services/role_store.rs
  - src/middleware/rbac_gate.rs
  - crates/visionclaw-domain/src/utils/visibility_filter.rs
  - src/handlers/socket_flow_handler/position_updates.rs
  - scripts/backup-sqlite.sh
  - src/bin/sync_github.rs
  - src/services/file_service.rs
date: 2026-08-31
---

# Data Authority & Erasure

## Purpose

Defines the single authoritative owner for each data class in the estate, the
store that actually wins today, and how dual-writes linearise. Erasure and
backup are covered honestly, including where they do not yet exist.

## Current State

### Ownership matrix

Each data class has exactly one authoritative store. "Wins today" is what the
running code reads back as truth, regardless of what legacy ADR prose claims.

| Data class | Authoritative owner (today) | Store / mechanism | Linearisation point |
|---|---|---|---|
| Authored content | Vault `public: true` / `owl-class` markdown (GitHub-synced) | `src/bin/sync_github.rs:~`, `src/services/file_service.rs:~` — gate per [ADR-2040](adr/ADR-2040-obsidian-vault-frontmatter-gate.md) / [`VAULT-corpus-format.md`](VAULT-corpus-format.md) §V4 | Sync run commit SHA; last-writer = the GitHub push |
| Visibility intent | Node `visibility` column + owner pubkey | SQLite (settings/node metadata), projected by `visibility_filter.rs` | Write to the node metadata row |
| Graph projection | Oxigraph named graphs | `oxigraph_ontology_repository.rs` (`GRAPH_KNOWLEDGE`) | Oxigraph transaction commit |
| Ontology (asserted + inferred) | Oxigraph | `GRAPH_ONTOLOGY` / `GRAPH_ONTOLOGY_INFERRED` | Oxigraph transaction commit; inferred is derived, never primary |
| Operational state | SQLite (WAL) | `data/{kpi,enrichment,settings,liveness}.sqlite3` | SQLite commit |
| Vector / agent memory | RuVector (external Postgres) | `mcp__claude-flow__memory_*` → `ruvector-postgres:5432` | Embedding-pipeline insert |
| Event journal / provenance | Oxigraph append-only | `GRAPH_PROVENANCE` (PROV-O reification), `oxigraph_ontology_repository.rs:55,641,2568` | Append; never mutated in place |
| Audit evidence (RBAC/auth) | SQLite role store | `src/services/role_store.rs` | Role-store write |
| Credentials | `.env` plaintext | filesystem | N/A — see divergences (SOPS never executed) |

### Store of record

The triple store is embedded Oxigraph (RocksDB backend, SPARQL 1.1); Neo4j is
100% removed (legacy ADR-132, no `neo4rs` in tree). Named-graph layout is fixed
in code: `GRAPH_ONTOLOGY`, `GRAPH_ONTOLOGY_INFERRED`, `GRAPH_KNOWLEDGE`,
`GRAPH_AGENT`, `GRAPH_SHAPES`, `GRAPH_PROVENANCE`
(`oxigraph_ontology_repository.rs:46-55`). Non-triple state lives in four
SQLite databases in WAL mode inside the `visionclaw-data` Docker volume at
`/app/data` (`scripts/backup-sqlite.sh:10-16`).

### Conflict resolution per class

- **Authored content vs graph projection.** GitHub is upstream; the Oxigraph
  `GRAPH_KNOWLEDGE` projection is derived from a sync run and is regenerated,
  never hand-edited. On conflict the sync overwrites the projection.
- **Ontology asserted vs inferred.** `:assert` is authored; `:inferred` is
  whelk-derived and rebuilt on each reasoner run — never a source of truth.
  The `:derived` graphs (`:summary`, `:observed`) are the ONLY writeback path
  through `/api/ontology/derived`; `:assert`/`:inferred` are never writable
  there (`oxigraph_ontology_repository.rs:56-65`).
- **Visibility intent.** The SQLite metadata row wins. The wire encoder applies
  it fail-closed: anonymous or unknown-owner callers are dropped to public-only
  (`visibility_filter.rs:11-14,74`).
- **Audit/RBAC.** Role store resolves the effective role with precedence
  explicit-assignment → power-user-Admin → default `Editor`, failing closed to
  `Viewer` on any error (`role_store.rs:188-199`).

### Linearisation of dual-writes

There is no cross-store two-phase commit. Each class linearises at its own
store's commit point (table above). Where a write fans out to two stores
(content sync → Oxigraph projection; agent action → RuVector + provenance), the
authoritative store commits first and the secondary is best-effort. There is no
distributed transaction and no compensating-action framework in code.

## Known divergences & open items

- **`deleteAgentMemory` tombstone gap.** Deleting agent memory in the Pod path
  emits NO reverse tombstone to RuVector. A delete does not revoke the vector
  memory: the embedding row persists and remains semantically searchable. RuVector
  is an external Postgres store reached only via MCP, with no delete-propagation
  hook. This is the single largest erasure hole.
- **No estate-wide erasure design.** There is no subject-erasure orchestration
  that spans GitHub content, Oxigraph graphs, SQLite state, RuVector vectors and
  the append-only provenance graph. `GRAPH_PROVENANCE` is append-only by design
  (`oxigraph_ontology_repository.rs:55`), so a compliant erasure story must define
  crypto-shredding or redaction for it — neither exists today.
- **Sources-of-truth conflict (legacy).** Legacy ADRs assign primacy to
  Oxigraph (132), Pod write-master (050/052), GitHub `public::true` (051),
  RuVector for agent memory (030) and provenance trails (033/034/124/128). Code
  resolves this as the matrix above; the legacy prose is not reconciled and must
  not be treated as authority.
- **Backup coverage is partial — Oxigraph only.** `scripts/backup-sqlite.sh` (2026-08-31) takes
  lock-consistent online snapshots of the SQLite DBs via the SQLite backup
  API. Off-volume placement depends on configuration: the host default is
  `./data/backups`. Membership is a contract, not a best-effort sweep: a missing
  member of `REQUIRED_DBS` (`settings.sqlite3 enrichment.sqlite3 kpi.sqlite3`,
  `backup-sqlite.sh:75`) aborts the run and publishes no manifest
  (`:242-245`), while a missing member of `OPTIONAL_DBS` (`liveness.sqlite3`,
  `:76`) is logged and the run continues (`:229`) — ADR-2069. **Oxigraph has NO
  point-in-time backup.** There is no cross-store consistent restore and no
  declared RPO/RTO.
- **Credentials.** SOPS (legacy ADR-109, VisionClaw) was accepted 2026-05-09 but
  NEVER EXECUTED — `.env` is plaintext, no SOPS artifacts in tree. Open.
- **Open-by-default posture is a compose choice, not a code default.** The code
  fails closed: `public_reads_enabled()` ends `.unwrap_or(false)`
  (`rbac_gate.rs:122-129`), whose doc comment states that "the absence of a
  security flag must never widen access", and `main.rs:727-732` refuses to start
  on an owner-less store unless `RBAC_ALLOW_OWNERLESS` is set explicitly
  (env const `role_store.rs:33`). The shipped compose inverts both:
  `RBAC_PUBLIC_READS: "${RBAC_PUBLIC_READS:-1}"`
  (`docker-compose.unified.yml:93`) and
  `RBAC_ALLOW_OWNERLESS: "${RBAC_ALLOW_OWNERLESS:-1}"` (`:94`). An unassigned
  authenticated pubkey then resolves to `Editor` via `effective_role`
  (`role_store.rs:359`). The open posture is ADR-2027's deliberate demo default
  and stays; corrected here (ADR-2069, ADR-2070 pass; raised by estate ADR-2087)
  because this document previously described the *code* default as open, which
  contradicted `docs/SECURITY-profiles.md`.
- **Landing 2026-08-31.** `PUBKEY_VISIBILITY_FILTER` now defaults ON
  (`position_updates.rs:35-42`, `unwrap_or(true)`) — the privacy encoder was
  previously inert. NIP-98 single-use replay cache added (`src/utils/nip98.rs`).
  These are reflected as current state above.
- **Identifier grammars unreconciled.** `vc:{domain}/{slug}` (legacy ADR-100),
  `urn:visionclaw:*` (105), `visionclaw:owner:{npub}/kg/...` (050), agentbox
  hex-canonical/npub-display (053) and minted-URNs-may-return-null (063) coexist.
  Code mints `urn:ngm:*` IRIs (`oxigraph_ontology_repository.rs:20-24`). A single
  identifier grammar is not yet agreed — cross-class joins rely on convention.

## Invariants

These must not silently change:

1. **One authoritative owner per class.** No class may acquire a second
   write-master without an explicit conflict-resolution rule in this table.
2. **`:assert` / `:inferred` / `:provenance` are never writable through the
   derived-writeback path.** Only `:summary` / `:observed` accept writeback.
3. **`GRAPH_PROVENANCE` is append-only.** Erasure of provenance must be by an
   explicit, documented redaction mechanism — never in-place mutation.
4. **Visibility filtering stays fail-closed.** Unknown-owner or anonymous
   callers get public-only; the default must remain ON.
5. **Role resolution fails closed to `Viewer`.** Errors never escalate.
6. **Secondary-store fan-out is best-effort but must be observable.** A dropped
   secondary write (e.g. RuVector tombstone) must be logged, not swallowed —
   the current silent gap is a defect, not the invariant.

## Change process

This is a living document. Amend it in the same commit that changes any
authoritative-owner assignment, conflict rule, or backup/erasure behaviour.
Every load-bearing claim carries a `file:line` citation; update the citation
when the code moves. Bump `version` (semver: patch for wording, minor for a
new class or rule, major for an owner reassignment) and refresh
`verified_commit`. Ratification requires closing the open items above or
explicitly accepting each as a signed-off trade-off with a named owner.

## Complete-system closeout qualification — 2026-09-04

ADR-2015/2016/2017 now carry CP-02/04/05/08 acceptance conditions backed by
[current source and a temporary backup probe](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/visionclaw-data-runtime.md).
The derived-writeback method is fenced; that does not exclude other governed
asserted-graph writers. Full sync drops knowledge and agent graphs before batch
processing and separately rebuilds asserted ontology, so no corpus-wide atomic
activation is established. Runtime assertions not yet in the corpus can be lost
on that rebuild even while their separate provenance survives.

Provenance emission is insert-only. The earlier record-atomicity gap is now closed locally: `reify_activity` validates its quad set and `commit_quads_with` commits it in one Oxigraph transaction (`crates/visionclaw-adapters/src/provenance_emitter.rs:282-310`). ADR-2016 remains partial for its wider integration acceptance; local record atomicity does not establish cross-store atomicity. SQLite online backup recovers WAL
data in the tested fixture, but required-member coverage, failure-domain
separation and coordinated application restore remain open. Existing statements
of immutable history or off-volume durability must be read with these limits.

## Remediation — 2026-09-05

- **ADR-2069** — ratifies the required/optional backup membership contract and corrects the
  "missing databases are skipped" sentence: a missing `REQUIRED_DBS` member aborts the run and
  publishes no manifest. Oxigraph point-in-time backup remains open.
- **ADR-2070** — corrects the "Open-by-default posture" bullet: the code fails closed
  (`rbac_gate.rs:122-129`, `main.rs:727-732`); the open posture comes from compose
  (`docker-compose.unified.yml:93,94`) and is ADR-2027's deliberate choice. Raised by estate ADR-2087,
  which found this document contradicting `docs/SECURITY-profiles.md`.
- **ADR-2066** — the unwired `/api/inference/*` stack was removed; it is no longer a store consumer.

- **ADR-2102** (proposed) — estate-wide subject-erasure orchestration across GitHub content,
  Oxigraph, SQLite, RuVector and the append-only provenance graph: one durable erasure record,
  five acknowledgements, partial erasure recorded and retryable. Forces the open
  `GRAPH_PROVENANCE` question — crypto-shredding the per-subject key, or provenance declared out
  of erasure scope — as an explicit choice. agentbox ADR-2060 is the RuVector-side half and is
  referenced (`see`), not superseded.
- **ADR-2103** (proposed) — Oxigraph point-in-time backup with a declared RPO (schedule) and RTO
  (measured restore drill), as a **required** backup member under ADR-2069 semantics. Extends
  ADR-2017's backup posture; `GRAPH_PROVENANCE` is restore-only because it cannot be re-derived.
- **ADR-2104** (proposed) — forces a choice on legacy ADR-109: execute the SOPS rollout or
  formally withdraw the 2026-05-09 acceptance. The tree shows a rollout that started and stopped
  (a 43 MB gitignored `scripts/sops` binary dated to the acceptance day, none of the four
  deliverables). Plaintext `.env` is documented as the interim state under either branch.
- **ADR-2097** — `MetadataActor::refresh_metadata` is deleted, not implemented. It logged one line and returned `Ok(())`, and `RefreshMetadata` had no senders anywhere in the workspace. It could not be implemented honestly either: `MetadataStore` is a `HashMap` type alias, the actor is constructed empty, and `metadata.json` is owned by `FileService`, which pushes rebuilt stores in via `UpdateMetadata`. Single-writer ownership is now documented on the actor so a future reload requirement lands on `FileService` rather than resurrecting the stub.

## Estate audit — 2026-09-07

The ownership table describes canonical stores, not a distributed transaction. The optional `redis` feature adds Nostr-session persistence (`src/services/nostr_service.rs:149-187,245-299`) beyond the default embedded graph and per-writer SQLite stores. Include it in erasure/restore membership when enabled; do not infer that it is deployed from this source path.

Ontology pull publication is not generation-atomic: `src/services/ontology_pull.rs:345-373` writes resources sequentially and writes the manifest last. Storage failure can expose a mixed generation; an ACL existence error is currently treated as absence. ADR-2106 now records the limitation and failure-injection acceptance work. Neither manifest presence nor HTTP success substitutes for cross-store erasure, restore or generation acceptance.
