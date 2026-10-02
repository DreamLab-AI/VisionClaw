---
id: ADR-2105
title: Carry one authoring correlation id through validation, PR, approval, merge and served corpus — VisionClaw echoes it
date: 2026-09-05
decision_status: rejected
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: b0bc275f6501aae7751b85a72ce15fe1e730e7e8
verified_paths: []
owner: jjohare
review_trigger: any change to the ontology proposal/approval/merge path, agentbox ADR-2022's Remaining item being taken up, or a governed-write audit requiring end-to-end traceability
repo: visionclaw
domain: IDENTIFIER-taxonomy
lineage: agentbox ADR-2022 governed ontology writes — its own "Remaining" section names this gap and is its ORIGIN, referenced with `see`, superseded by nothing here; agentbox ADR-2054 (every authoring caller crosses the gate); IDENTIFIER-taxonomy (the authority for identifiers that must survive a repo crossing)
---

# ADR-2105 — Carry one authoring correlation id through the full promotion chain

## Context

Diagram **AB-25.5** (`agentbox/25-ontology-tools-and-governed-writes.md:259`) records that
the correlated promotion chain is not demonstrated end to end. The agentbox side is built:
`mcp/servers/lib/ontology-authoring-authority.js:77-79` freezes the chain as
`['validation','proposal','approval','merge','served-corpus']`, the gate mints a
96-bit-suffixed id at stage 1 (`:387`) and stamps it into the authored artefact's
frontmatter as `ontology-authoring-correlation` / `-mode` / `-stage` (`:97-99,411-426`).
Stages 2 to 5 are VisionClaw's and GitHub's. VisionClaw has a `correlation_id` concept in
its event and telemetry layers (`src/events/types.rs`, `src/telemetry/agent_telemetry.rs`)
but **not** in the governed-write path: `src/services/ontology_mutation_service.rs` and
`src/handlers/enrichment_proposals_handler.rs` contain no `correlation_id`. An id minted
at validation therefore dies at the repo boundary, and the audit trail the agentbox gate
exists to produce is four-fifths missing. agentbox ADR-2022's own "Remaining" section
names this — it is the **origin** of the gap, not its resolution.

## Decision

**Proposed.** The authoring correlation id is a durable cross-repo identifier, and
VisionClaw is a participant in the chain rather than its terminus. When taken:

1. **The id is an identifier, governed here.** `docs/IDENTIFIER-taxonomy.md` records
   `ontology-authoring-correlation` as a cross-repo correlation identifier: minted only
   by the agentbox gate, opaque to VisionClaw, never re-minted, never synthesised on the
   VisionClaw side when absent. Its grammar and mint site are agentbox's; its obligation
   to survive the crossing is this document's.
2. **Every downstream stage accepts and echoes it.** The proposal handler
   (`enrichment_proposals_handler.rs`), the mutation service
   (`ontology_mutation_service.rs`) and the PR-creation path (`github_pr_service.rs`)
   accept an optional correlation id, persist it with the proposal record, and echo it in
   every response and emitted event for stages `proposal`, `approval`, `merge` and
   `served-corpus`.
3. **Absence is recorded, never invented.** A request arriving without an id is processed
   and marked `correlation: unlinked`. Minting a VisionClaw-side substitute is prohibited:
   a synthetic id would make an unaudited write look audited, which is worse than an
   honest gap. This mirrors the existing `cross_from_agentbox` rule that unmapped kinds
   record the raw string plus an unmapped marker rather than a synthetic ID.
4. **The id reaches the served corpus.** The merged artefact's frontmatter retains the
   stamped keys through sync, so a class in the served corpus can be traced back to the
   validation that admitted it by grepping one id.
5. **Authority split is explicit.** agentbox ADR-2022 keeps the gate and the stage
   vocabulary; this record holds the VisionClaw-side obligation and the identifier's place
   in the taxonomy. Neither supersedes the other.

## Consequences

- A governed ontology write becomes auditable end to end: one id answers "what validated
  this, who approved it, which merge landed it, is it serving".
- Cost: a schema/column addition on the proposal record, a field on three request/response
  shapes, and event-payload changes — small individually, but cross-repo, so it needs both
  lanes in one change window.
- The `unlinked` marker makes the size of the unaudited surface visible, which will
  initially look worse than the current silence. That is the point.
- Frontmatter keys must survive the corpus sync path unmodified; any sync step that
  rewrites or strips frontmatter breaks stage 5 and must be found before this lands.
- No change to `urn:visionclaw:*` grammar: the correlation id is a *linking* identifier
  and is never a subject, never minted by `src/uri/mod.rs`, and never persisted in a URN
  position.

## Verification

`implementation_status: none`. Verified at `b0bc275f6501aae7751b85a72ce15fe1e730e7e8`
(VisionClaw) and `e070514d808b218574403377fb75e0e1a0a256b3` (agentbox):

- agentbox side present: `AUTHORING_CHAIN_STAGES` (`ontology-authoring-authority.js:77-79`),
  `mintCorrelationId` (`:228`), grant carries `correlation_id`/`chain_stages`/`stage`
  (`:387-389`), frontmatter stamping (`:401-426`), and the intent comment at
  `mcp/servers/ontology-bridge.js:85` naming the full chain.
- VisionClaw side absent: `grep -rn 'correlation_id\|correlationId' src/services/ontology_mutation_service.rs
  src/handlers/enrichment_proposals_handler.rs` returns nothing. Hits elsewhere
  (`src/events/`, `src/telemetry/`) are an unrelated event-tracing field.

**Acceptance test for the landing change.** Author one class through the agentbox gate and
capture the id it mints:

1. The artefact's frontmatter carries `ontology-authoring-correlation: <id>` at stage
   `validation`.
2. The proposal created in VisionClaw returns that same `<id>`, and the persisted proposal
   record stores it.
3. The approval response and the merge event both carry `<id>`; the four downstream stage
   values (`proposal`, `approval`, `merge`, `served-corpus`) each appear exactly once for
   that id.
4. After sync, a SPARQL or corpus query for the merged class resolves to an artefact whose
   frontmatter still carries `<id>` — five stages, one id, no gaps.
5. Submit a proposal with **no** id: it succeeds and is recorded `correlation: unlinked`,
   and no id is minted on the VisionClaw side (`grep` for a mint call in the VisionClaw
   path returns nothing).
6. Submit a proposal with a malformed id: it is rejected or recorded `unlinked`, never
   normalised into a plausible-looking one.

## Disposition — 2026-10-02

- **Suitability:** discordant
- **Priority:** withdrawn
- **Why:** The premise no longer holds at either end of the chain. The mint site this record depends on, agentbox `mcp/servers/lib/ontology-authoring-authority.js`, was deleted in agentbox `873d9e0a1`, and agentbox ADR-2022, the origin record, is superseded by agentbox ADR-2108. The PR, approval and merge stages were removed by accepted host ADR-2116: `/api/ontology-agent/propose` answers 410, and proposals are forum kind-31402 events decided by signed kind-31403 events. No `ontology-authoring-correlation` reference survives in host `src/`, `crates/` or agentbox (grep, 2 Oct). Traceability now comes from the signed event ids. Whether apply receipts cite the deciding 31403 id is unverified.
- **Next:** None. `decision_status` set to `rejected`; the other fields are unchanged. If end-to-end traceability is wanted for the ADR-2116 path, raise it there, against the event ids.
