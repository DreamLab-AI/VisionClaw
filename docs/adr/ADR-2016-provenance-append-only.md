---
id: ADR-2016
title: GRAPH_PROVENANCE is append-only (INSERT DATA only)
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
verified_paths: [src/services/data_reconciliation.rs, crates/visionclaw-adapters/src/provenance_emitter.rs, crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs, src/services/ontology_mutation_service.rs]
owner: jjohare
review_trigger: a GDPR/right-to-erasure obligation landing on provenance-recorded subjects, or introduction of a redaction/crypto-shred mechanism
repo: visionclaw
domain: DATA-authority-erasure
lineage: legacy ADR-033 (git-bead provenance), ADR-034 (needle-bead), ADR-124/ADR-128 (web-contract / gitmark blocktrails)
---

# ADR-2016 — GRAPH_PROVENANCE is append-only (INSERT DATA only)

## Context

Provenance must be a tamper-evident record: a governed event that could be
silently rewritten or deleted afterwards is not provenance. The PROV-O triad
(Entity/Activity/Agent) reifies the same identifiers used on the decision
paths, so the RDF is a queryable projection, never a fork. The constraint is
in tension with data-erasure duties: an append-only log cannot honour a delete.

## Decision

Every governed event writes a full PROV-O `prov:Entity` / `prov:Activity` /
`prov:Agent` triad (plus `wasGeneratedBy` / `wasAttributedTo`) into
`GRAPH_PROVENANCE` via **insert-only quad writes**. `DELETE`, `DROP`, and
`CLEAR` are never issued against this graph. All emission funnels through the
single `reify_activity` primitive (called via the async `emit_activity` /
`emit_activity_nonfatal` wrappers), whether invoked through
`OxigraphOntologyRepository::emit_provenance` or directly by a caller such as
`ontology_mutation_service.rs`. This forecloses in-place mutation and
destructive compaction: any future erasure of a provenance-recorded subject
must be an explicit redaction or crypto-shred mechanism — which does not yet
exist.

## Consequences

- The provenance graph grows monotonically; there is no compaction and no
  built-in retention trim. Storage cost is unbounded over time.
- There is a real erasure gap: a right-to-be-forgotten request against a
  subject recorded here cannot be satisfied today. This interacts with the
  backup / write-master posture in ADR-2017 (no PITR, no consistent restore).
- Successful complete records support agent-scoped SPARQL (`?a a prov:Agent`).
  Insert-only emission does not guarantee record completeness after failure or
  independently prove tamper-evidence against other privileged writers.

## Verification

Re-checked at `e0f8cd896`: `provenance_emitter.rs` header states the
append-only rationale ("only `INSERT DATA` is permitted. No `DELETE`, `DROP`,
or `CLEAR`"); `reify_activity` (line 97) issues only `store.insert(QuadRef::new(...))`
calls for the Activity/Agent/Entity triad, and a grep of the file for
`remove`/`clear`/`DELETE`/`DROP`/`CLEAR` returns nothing but the header comment.
Two call sites reach `reify_activity`: `oxigraph_ontology_repository.rs:646`
(`emit_provenance`, wrapping `emit_activity`) and
`src/services/ontology_mutation_service.rs:127` (`emit_activity_nonfatal`,
called directly, bypassing `emit_provenance`) — both resolve to the same
insert-only primitive, so the append-only property holds across both paths
even though `emit_provenance` is not the sole entry point.
Re-verified at `542d63d1d` after the ADR-141 formatting sweep (test-only line
wrapping in `ontology_mutation_service.rs` `provenance_wiring_tests`) — the
append-only invariant is unchanged.

## Closeout extension — 2026-09-04

**Work package:** CP-04 / CP-05 / CP-08. **Owner:** existing owner above. Dependencies are
CP-01 revision/ownership mapping and the relevant corpus or authority contract.

**Current evidence:** reify_activity inserts individual quads without an enclosing transaction. The activity type is inserted before the agent IRI is validated; later invalid input or storage failure can leave a partial record. This is source evidence, not a production fault reproduction.

See [runtime analysis](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/visionclaw-data-runtime.md),
[source hashes](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/visionclaw-data-snapshot.json)
and [backup receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/visionclaw-backup-probe.json).
Source was inspected at `b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`. Earlier verification at `9a2c8087385bf6db08b1aeb91004e1a60203965b`
remains historical evidence; this annex does not claim a new deployed activation
or complete verification of every older assertion.

**Acceptance still required:** Validate all terms before writing and commit a complete record atomically, or detect and repair partial records. Test a late invalid IRI and injected storage failure, correlate completed provenance with mutation receipts, and distinguish insert-only code from tamper-detection and retention guarantees.

## Consumer closeout extension — 2026-09-05

The [joined-trace review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/joined-provenance-trace.md) traces the separate SQLite-backed `GET /api/trace` consumer. It does not query this ADR's Oxigraph provenance graph. Shared-identity grouping and a two-source canary are insufficient evidence of a causally complete action. CP-01/04/08 acceptance must establish original-record correlation, resource-authorised reads, capture health, bounded time/volume and any actual pod integration. This source-only extension preserves the append-only decision and its earlier storage obligations; no endpoint or deployment acceptance ran.

## Acceptance progress — 2026-09-05

**Implemented.** `crates/visionclaw-adapters/src/provenance_emitter.rs`. The
reproduced defect — `reify_activity` inserted quads one at a time with no
enclosing transaction, and inserted the activity type *before* validating the
agent IRI, so a late invalid input or a storage failure left a partial record —
is closed by a two-phase write.

* `build_activity_quads(record)` validates **every** term first (including the
  optional `generated` and `informed_by`, which the interleaved version reached
  only after writing three quads) and returns the full quad set. It touches no
  store, so callers can validate a record without writing it.
* `commit_quads_with(store, quads, guard)` commits the whole set inside one
  `Store::transaction`. The guard is called with each quad's index immediately
  before insertion — the seam that makes the atomicity contract testable, since
  a guard failing at index *n* aborts after *n* inserts have been issued, which
  is exactly the shape of a mid-record storage failure.
* `find_incomplete_activities(store)` is the repair half: a SPARQL query for
  subjects typed `prov:Activity` missing any of `MANDATORY_ACTIVITY_PREDICATES`,
  for enumerating damaged records written by the pre-ADR-2016 emitter in a
  deployed store.

`ProvenanceError` gains `From<StorageError>` so it can be the transaction
closure's error type.

**Tests.** `cargo test -p visionclaw-adapters --lib provenance` — 20 passed,
0 failed (9 new): late invalid `generated` IRI writes nothing; late invalid
`informed_by` writes nothing; invalid `used` writes nothing; pure validation
without a store; **injected storage failure at index 4 rolls the record back**
and the same quads then commit cleanly, proving the rollback did not poison the
store; failure at index 0 is a clean no-op; atomically written records are never
reported incomplete; a simulated legacy partial record *is* detected; and every
mandatory predicate is actually written, so the constant and the emitter cannot
drift.

**Receipts.** `docs/estate-closeout/2026-09-05/adr-2016-provenance-atomicity.txt`,
`adr-2015-2016-adapters.txt`.

**Remains open.** Correlating completed provenance with mutation receipts is
only partly advanced (ADR-2006 now preserves the signed event id on the decision
side). Tamper detection and retention guarantees remain distinct from
insert-only code and are not implemented. The separate SQLite-backed
`GET /api/trace` consumer is untouched.

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

**Governed changes since `9a2c80873`:** `crates/visionclaw-adapters/src/provenance_emitter.rs`
(+588, effectively a rewrite), `crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs`
and `src/services/ontology_mutation_service.rs`, landed by `b47db377c` and
`1b513295a`. The rewrite is this record's own acceptance work — the two-phase
atomic write — not a change of decision.

**Append-only still holds, and is now provably so.** The module header at
`provenance_emitter.rs:32-33` still states the rule verbatim ("only `INSERT DATA`
is permitted. No `DELETE`, `DROP`, or `CLEAR`"), and
`grep -cE 'DELETE |DROP |CLEAR ' crates/visionclaw-adapters/src/provenance_emitter.rs`
returns **0** at HEAD. Emission still funnels through one primitive:
`reify_activity` (`:307`), reached by `emit_activity` (`:433`) and
`emit_activity_nonfatal` (`:446`).

**Two stale citations in the Verification block, corrected here:**

| Cited | Correct at HEAD |
|---|---|
| `reify_activity` (line 97) | **`provenance_emitter.rs:307`** |
| `src/services/ontology_mutation_service.rs:127` | **`:223`** |
| `oxigraph_ontology_repository.rs:646` | `:646` — **still exact** (`emit_provenance` wraps `emit_activity` at `:650`) |

The `reify_activity` move is large because the file more than doubled: the
partial-write defect the 2026-09-04 closeout recorded (quads inserted one at a
time, activity type written *before* the agent IRI was validated, so a late
invalid IRI or storage failure left a torn record) is closed by splitting
validation from commit — `build_activity_quads` (`:137`) validates every term
with no store access, `commit_quads_with` (`:283`) writes them in a single
`Store::transaction`, `MANDATORY_ACTIVITY_PREDICATES` (`:117`) defines
completeness, and `find_incomplete_activities` (`:332`) is the SPARQL repair
query for legacy partial records. `From<StorageError>` at `:105-109` keeps a
storage fault a typed error rather than a panic.

The two-call-site asymmetry the Verification block records is unchanged and still
worth knowing: `ontology_mutation_service.rs:223` calls
`emit_activity_nonfatal` **directly**, bypassing the repository's
`emit_provenance` wrapper.

**Commands run:** `git diff --stat 9a2c80873..HEAD -- <verified_paths>`;
`grep -n` over `provenance_emitter.rs` for `build_activity_quads|commit_quads_with|
reify_activity|find_incomplete_activities|MANDATORY_ACTIVITY_PREDICATES|emit_activity`;
`sed -n` dumps of `:32-33`, `:105-123`, `oxigraph_ontology_repository.rs:646-650`
and `ontology_mutation_service.rs:223`;
`grep -cE 'DELETE |DROP |CLEAR '` → 0;
`cargo test -p visionclaw-adapters --lib provenance` → **20 passed, 0 failed**
(56 filtered out); `cargo test -p visionclaw-adapters` → **76 passed, 0 failed**.

**Still open:** correlating completed provenance with mutation receipts
(ADR-2006) and the separate SQLite-backed `GET /api/trace` consumer were not
re-checked — they lie outside `verified_paths`.

## Landing re-verification — 2026-09-06 (2cf222406)

Governed paths changed in the Wave 3 landing commit: crates/visionclaw-adapters/src/oxigraph_ontology_repository.rs: three `urn:ngm:class` mints routed through the typed constructor with byte-identical output (ADR-2095); the append-only provenance path is untouched. Decision unaffected; `verified_commit` moved to the landing commit. Gates at that commit: cargo check --workspace --all-targets exit 0, 827 crate + 1600 root + 309 xr-client tests, vitest 809, fmt and lint clean.


## Source closeout verification — 2026-09-07

The new durable reconciliation journal supplies operation IDs, selected membership and per-store receipts. No production provenance erase adapter is wired: ADR-2016 forbids DELETE/DROP/CLEAR against the append-only provenance graph, so a concrete redaction/crypto-shred authority and subject mapping must be agreed before such an adapter can truthfully implement erasure. Synthetic adapter tests establish retry/idempotency boundaries only; they do not satisfy provenance erasure.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.
