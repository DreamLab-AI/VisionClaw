# ADR-068: Logseq Block-Level Fidelity (Matryca-Heritage Parser)

**Status:** Implementing
**Date:** 2026-05-01
**Implementation:** `crates/graph-cognition-extract/src/logseq/` — LogseqBlockParser O(N) stack-machine, BlockNode, 15 tests passing
**Deciders:** jjohare, VisionClaw platform team
**Supersedes:** None
**Extends:** Existing `src/services/parsers/knowledge_graph_parser.rs` (1,349 lines, page-level)
**Implements:** PRD-005 §6 Epic H
**Threat-modelled:** PRD-005 §19 (R-15 30× node explosion, R-16 UUID collisions, F-01 cyclic block-ref, F-02 UUID collision attack, F-15 1M-block OOM, T-5 mutation amplification)

## Context

VC's existing `KnowledgeGraphParser` parses Logseq vaults at **page granularity** — one file becomes one node with extracted wikilinks. This loses the block-level structure that gives a Logseq vault its value: parent/child indentation, properties, block-refs `((uuid))`, drawer metadata, scheduled/deadline dates.

The matryca parser project (`logseq-matryca-parser`) demonstrates a deterministic O(N) stack-machine FSM for Logseq AST that preserves this structure faithfully. Its data model maps cleanly to Logseq's internal Datascript schema: `:block/uuid`, `:block/parent`, `:block/left`, `:block/refs`, `:block/path-refs`, `:block/journal-day`, `:block/scheduled`, `:block/deadline`, `:block/marker`.

QE chaos review flagged five concerns:

- **R-15 / F-15** — Block-level explosion (998 files × ~30 blocks ≈ 30k nodes; pathological 1M); GPU and pod budgets assumed ~5k.
- **R-16 / F-02** — UUID collision attacks: explicit `id::` properties from external content can collide with existing user URNs.
- **F-01** — Cyclic block-ref `((uuid-X))` whose target embeds `((uuid-X))`; parser stack overflow.
- **T-5** — Mutation amplification: crafted CLOCK entries explode temporal edges.

QE quality-analyzer flagged a critical migration gap:

- The existing 998-file Logseq vault is already in Neo4j as page-level nodes with URNs. Block-level upgrade is not a clean replacement — it must preserve existing URNs while adding block-level children.

## Decision

**Implement a Rust stack-machine block parser. Always namespace block URNs by `did:nostr:<owner-pubkey>` regardless of explicit `id::`. Cap recursion and edge counts. Migrate page-level nodes to "container" status, not delete.**

### D1 — Stack-machine FSM in Rust

`crates/graph-cognition-extract::logseq` implements `LogseqBlockParser` as an O(N) deterministic stack machine matching matryca's design. Per matryca's verified failure modes:

- Indentation alone arbitrates parent-child hierarchy (heading markers `#` are *content*, not structure).
- Soft-break continuation: lines without bullet prefix merge into current block's `content` field.
- Property line detection: `key:: value` on its own line at block terminus → property; mid-sentence `key:: value` → literal prose.
- Drawer parsing: `:LOGBOOK:`/`:PROPERTIES:`/`:END:` regions consumed as opaque metadata; CLOCK entries parsed as `(start_ts, end_ts, duration_min)` tuples.
- Empty drawer (`:LOGBOOK:` immediately followed by `:END:`) handled gracefully (matryca regression fix #3823).
- Indentation fracture (0→4-space jump): no phantom intermediate nodes; baseline adjusts and accepts as new delta.
- Cloze/LaTeX nesting (`{{c1 $\mathrm{K}$}}`) handled with sub-state machine.

Per-block output: `BlockNode { uuid, content, clean_text, parent_id, left_id, indent_level, properties, refs, task_status, scheduled, deadline, repeater, created_at, journal_day }`.

### D2 — UUID precedence and namespacing

Order of UUID assignment for a block:

1. **Explicit `id::` property present** → adopt **but salted with owner**: `mint_typed_concept(owner, NodeKind::Block, uuid_str)` produces `urn:visionclaw:concept:<owner-hex>:block:<uuid-str>`. The bare uuid is preserved as a property (`logseq_uuid`) but the **graph identity** is the URN.
2. **No explicit id** → deterministic v3: `Uuid::v3(NAMESPACE_DNS, format!("{owner_hex}:{rel_path}:{block_index}:{content_hash}"))`. Owner is included in the namespace string so re-ingestion under a different identity produces a distinct URN.

This closes R-16 / F-02 by making block URNs cryptographically owner-scoped. An attacker cannot publish a block whose URN collides with the victim's, regardless of what `id::` they put in their vault.

### D3 — Recursion and capacity caps

- Block-ref embed-resolution depth ≤ 16 (deeper → mark `cyclic_embed=true`, render as opaque ref without expansion).
- Per-block refs cap = 256; CLOCK entries cap = 64. Over-cap blocks marked `truncated=true` and surfaced in dashboard. Closes T-5.
- Per-vault hard cap (default 50,000 blocks; configurable up to 200,000 with `--allow-large-vault` flag). Above cap: refuse-and-fall-back-to-2D-mode. Closes R-15 / F-15.
- Recursion-depth cap on cloze/LaTeX nested parsing (32 levels).
- Coordinate parsing for PDF asset annotations uses checked arithmetic (`i64::checked_mul`, `i32::try_from`); panic-free.

### D4 — Migration: existing page-level Neo4j → block-level

The 998 Logseq files already in Neo4j as page-level nodes are **preserved as containers**, not deleted:

- Page-level nodes retain their URNs (`urn:visionclaw:concept:<owner-hex>:page:<file-stem>`).
- New block-level nodes mint URNs per D2 and attach via `BLOCK_PARENT` edges to their containing page.
- Existing page-page wikilink edges are augmented (not replaced) by block-block wikilink edges where the source/target are now blocks rather than pages.
- The user's existing federated graph references (any pod beads citing page URNs) continue to resolve.

Migration script `migrations/2026-05_logseq-blocks.cypher`:
1. For each existing `:Page` node, run the new parser.
2. Insert child `(:Block)` nodes with `:BLOCK_PARENT` edges to the page.
3. Re-resolve wikilinks: prefer block-level target if available, else page-level (legacy behaviour preserved).
4. Compute `path-refs` inheritance and store on each block.

Idempotent: re-running on unchanged vault produces zero mutations (D6 below).

### D5 — Path-refs inheritance

Each block's `path-refs` is the union of:
- Its own `refs` (wikilinks + tags + block-refs).
- All ancestor blocks' `refs` up to the page root.

Stored as a flattened set on the block for query-time efficiency. Search for `#research` returns nested blocks whose only ancestor with `#research` is the page itself — meaningful UX upgrade.

### D6 — Idempotency invariant

Block UUID determinism (D2) plus content-hash inclusion in the deterministic-fallback namespace ensures re-parsing an unchanged vault produces zero mutations. Re-parsing a vault with one changed block produces exactly one node mutation and at most O(1+|refs|) edge mutations.

CI test: parse → commit → re-parse → assert zero new beads, zero new edges.

### D7 — Cross-implementation parity test (matryca golden corpus)

A 50-page fixture vault is committed under `tests/fixtures/logseq-matryca-parity/`. The test runs both:

- Our Rust parser → emits `parity-rust.json`.
- The matryca Python parser → emits `parity-matryca.json` (Python invocation is in **test infra only**, not runtime; this is consistent with PRD-005 Non-goal #1 which scopes "no Python in runtime").

Asserts:
- ≥99% identical block UUIDs (allowed 1% drift documented as: explicit-`id::`-bearing blocks where matryca's namespace differs from our owner-scoped namespace).
- ≥99% identical parent/left edges.
- 100% identical `path-refs` sets per block.

CI runs nightly. Drift triggers a failing build; if upstream matryca changes UUID strategy, we pin matryca's commit hash explicitly in the fixture build script.

### D8 — Streaming parser, bounded memory

For large vaults (>10k blocks):

- Streaming parser reads file-at-a-time and emits a bounded `mpsc::channel<BlockBatch>` (capacity = 10,000 blocks).
- Downstream commit actor batches into transactional Neo4j writes of 1,000 blocks each.
- Backpressure when downstream is slow.
- RSS budget 2 GB enforced via cgroup; CI test on synthetic 1M-block vault asserts RSS < 2 GB throughout.

Closes F-15.

## Consequences

### Positive

- High-fidelity Logseq ingest: every block becomes a first-class graph node with provenance.
- Block-level search produces ≥10× more useful hits than page-level on the user's reference vault.
- Existing page-level URNs preserved; federated peers' citations continue to resolve.
- Recursion/capacity caps eliminate parser-DoS attack surface.
- Owner-scoped UUID namespace closes the cross-vault collision attack.

### Negative

- Block explosion (~30× node count for a typical vault) propagates to downstream sizing. Pod write SLA (§7.1) needs to be revised per-MB or with size buckets — handled in PRD §19.7 follow-up PRD-005c.
- Migration is irreversible after dual-existence cleanup (~1 release later); rollback before that point is reversible.
- 1% allowed UUID drift between matryca and our parser is a permanent escape hatch; characterise it precisely in the test report.

### Risks

- Logseq's `:journal/page-title-format` config drift mid-vault produces multi-format journal axes. Mitigated by multi-pass date resolution (matryca-style) but ordering precedence must be specified.
- Asset-reference resolution (`hls://`, `file://`) has its own threat surface — handled in PRD-005's Epic E.6 / ADR-Epic-E security AC.

## References

- PRD-005 §6 Epic H, §19 (R-15, R-16, T-5, F-01, F-02, F-15)
- Matryca: `logseq-matryca-parser/docs/ARCHITECTURE.md`, `logseq_ast_primer.md`, `LOGSEQ_DATASCRIPT_MAPPING.md`, `LOGSEQ_TEMPORAL_ONTOLOGY.md`
- Existing: `src/services/parsers/knowledge_graph_parser.rs` (page-level baseline)
- ADR-064 (Typed Graph Schema)
- ADR-066 (Pod-Federated Graph Storage)
