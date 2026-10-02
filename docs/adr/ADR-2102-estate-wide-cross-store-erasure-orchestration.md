---
id: ADR-2102
title: Orchestrate subject erasure across all five stores, and give the append-only provenance graph a redaction mechanism
date: 2026-09-05
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: b0bc275f6501aae7751b85a72ce15fe1e730e7e8
verified_paths: []
owner: jjohare
review_trigger: any right-to-erasure request reaching the estate, a subject-deletion API being designed, agentbox ADR-2060 landing its RuVector-side tombstone, or any change to the GRAPH_PROVENANCE append-only invariant
repo: visionclaw
domain: DATA-authority-erasure
lineage: DATA-authority-erasure "No estate-wide erasure design" and "deleteAgentMemory tombstone gap"; ADR-2016 (append-only provenance); ADR-2017 (per-class write-master, no cross-store 2PC); agentbox ADR-2060 (the RuVector-side half, `see` only)
---

# ADR-2102 — Orchestrate subject erasure across all five stores, and give the append-only provenance graph a redaction mechanism

## Context

Diagram **VC-22.10** (`22-data-authority-provenance-erasure.md:422`) records that no
erasure or corpus-consistency orchestration spans the estate's five stores: GitHub
content, Oxigraph graphs, SQLite state, RuVector vectors and the append-only provenance
graph. **VC-22.4** (`:230`) records the sharper half — `GRAPH_PROVENANCE` is append-only
by design (`provenance_emitter.rs:32-33`, test `append_only_verified` at `:917`;
retraction only *adds* `dl:validTo`, `provenance_writer.rs:218,478-502`), so no deletion
mechanism exists for it at all. ADR-2017 forecloses cross-store 2PC and states there are
no tombstones. agentbox ADR-2060 designs the RuVector half only, and is the **origin**
statement of that gap rather than its closure. Today a "delete my data" request cannot be
honoured completely, and cannot be shown to have been honoured at all.

## Decision

**Proposed.** Subject erasure is a first-class, orchestrated estate operation, not a
per-store courtesy. When taken, the decision is:

1. **One initiator, one erasure record, five acknowledgements.** An erasure names a
   *subject identifier* (a `did:nostr`, or a `urn:visionclaw:*` owner scope) and is
   recorded durably before any store is touched. Each of the five stores acknowledges
   independently. The erasure is complete only when all five have acknowledged; anything
   less is a recorded, retryable **partial erasure**, never a reported success.
2. **The record is durable and replayable.** A store that is down at request time applies
   the erasure on recovery. Best-effort in-memory fan-out is explicitly insufficient:
   the failure being designed against is a deletion that *appears* to succeed.
3. **`GRAPH_PROVENANCE` gets redaction, not deletion.** The append-only invariant
   (ADR-2016) is retained. Erasure of provenance is satisfied by **crypto-shredding**:
   subject-identifying literals in provenance quads are written encrypted under a
   per-subject key held outside the graph, and erasure destroys the key, leaving the
   quad structure and its hash chain intact and verifiable while the plaintext becomes
   unrecoverable. Redaction-by-overwrite is rejected — it breaks the invariant and the
   chain. **This is the open design question this ADR exists to force**: the alternative
   is to declare provenance out of erasure scope and say so in the privacy surface, and
   that alternative must be chosen explicitly, not by omission.
4. **Derived stores are re-derived, not erased.** Oxigraph `:assert` is disposable
   (ADR-2017) and is rebuilt from the GitHub upstream after the upstream content is
   erased; erasing it directly without erasing upstream is a no-op that the orchestrator
   must refuse rather than acknowledge.
5. **Authority stays here.** `docs/DATA-authority-erasure.md` holds the invariant.
   agentbox ADR-2060 is the RuVector-side implementation record and is referenced with
   `see`, not superseded — it names this gap and does not close it.

## Consequences

- A subject-erasure surface becomes possible to build and, more importantly, possible to
  *audit*: the erasure record is the evidence.
- Cost: a durable orchestration store, five idempotent consumers, per-subject key custody
  for crypto-shredding (which interacts with ADR-2104's secret-custody posture), and a
  cross-repo dependency on agentbox ADR-2060 for the RuVector consumer.
- Crypto-shredding changes the provenance *write* path: subject-identifying literals must
  be encrypted at emission, so this cannot be retrofitted to quads already written. Quads
  written before the mechanism lands are permanently un-erasable, and that boundary date
  must be recorded.
- Interim honesty (binding now, before any of this lands): any erasure surface shipped
  before this ADR is implemented must state that deletion covers the Pod copy only, does
  not revoke RuVector-held vectors, and does not touch provenance history.
- Choosing the alternative in item 3 (provenance declared out of scope) is a legitimate
  outcome of this ADR, but it is then a stated limitation of the privacy posture, not an
  absence.

## Verification

`implementation_status: none` — this records a decision and a plan. Verified at
`b0bc275f6501aae7751b85a72ce15fe1e730e7e8` that the gap is real: `docs/DATA-authority-erasure.md`
"Known divergences & open items" carries both "No estate-wide erasure design" and the
`deleteAgentMemory` tombstone gap; `grep -rn 'tombstone\|2pc\|two-phase' src/ crates/`
returns no orchestration; `provenance_emitter.rs:32-33` states the append-only invariant
and no DELETE/DROP/CLEAR is issued against `GRAPH_PROVENANCE`.

**Acceptance test for the landing change.** Issue one erasure for a subject that holds
data in all five stores, with the RuVector consumer stopped:

1. The call returns **partial**, naming RuVector as unacknowledged — not success.
2. Restart the consumer; the erasure replays from the durable record without re-issuing
   the request, and the erasure record transitions to complete with five acknowledgements.
3. `memory_retrieve` and `memory_search` for the subject's keys return nothing.
4. A SPARQL read of `GRAPH_PROVENANCE` for the subject returns quads whose structure and
   count are **unchanged**, whose subject-identifying literals are unreadable, and whose
   append-only test (`append_only_verified`) still passes.
5. Re-running the same erasure is idempotent: five acknowledgements, no new mutations.
6. Erasing a subject present only in the derived Oxigraph `:assert` graph without an
   upstream deletion is **refused** with a typed error, not acknowledged.

## Disposition — 2026-10-02

- **Suitability:** fits, needs revision
- **Priority:** P2 — next cycle (planning-cycle §3: "Erasure pair (agentbox ADR-2060, host ADR-2102) — first candidate after this cycle alongside ADR-2078")
- **Why:** A left-behind residential node will one day receive a deletion request, and nothing in the estate can honour one in full, so the intent stands. The store list is out of date. "GitHub content" is no longer the corpus upstream: ingest is a `CorpusSource` (ADR-2114) and GitHub sync is optional and off (ADR-2115). Decision item 4's "rebuilt from the GitHub upstream" therefore needs restating against the local vault. agentbox ADR-2060 is still `proposed`/`none`.
- **Next:** Reopen at the start of the next cycle together with agentbox ADR-2060. First restate the five stores against ADR-2114/2115, then make the item-3 choice (crypto-shredding, or provenance declared out of scope).
