---
expectation_id: EXP-AC-002
git_sha: 4a9a3e0682bdc695a8ebf904e0453271443daea8
produced_by: agent:claude-opus
produced_at: 2026-09-14T15:29:56Z
audited_by:
---

# Evidence — EXP-AC-002 (non-vacuous decision surface, VisionClaw half)

**Scope note.** EXP-AC-002 spans two codebases. This evidence covers the
**VisionClaw** clauses only (PRD FR2.3, FR2.4). The forum `ActionRow` clauses
(FR2.1, FR2.2) belong to nostr-rust-forum and are NOT evidenced here.

## 1. The full proposal payload is rendered, unclipped

`client/src/features/control-center/governance/AcspCaseQueue.tsx` — the payload
renders in a `<pre data-testid="acsp-case-payload">` as
`JSON.stringify(proposal, null, 2)`, with `whitespace-pre-wrap` +
`overflow-x-auto`. The former `line-clamp-2` preview is gone; a test asserts the
element's class list contains no `line-clamp`.

Proposal URN, reasoning summary, reasoning hash and generation provenance each
render in their own testid'd element, **only when present**.

## 2. Provenance and confidence render only when the agent produced them

Backend: `CaseMetadata` gains `risk_tier`, `confidence`, `provenance`
(`src/handlers/broker_inbox_handler.rs`), projected from the proposal body by
`EnrichmentProposal::from_stored` — `confidence` is `j.get("confidence")`, with
no default. Client: `toCaseView` maps them through `num()` /`toProvenance()`,
which return `undefined` for a missing or non-finite value, and an all-empty
provenance bag collapses to `undefined`.

```
$ cargo test --lib --no-default-features --features ontology,persistence-oxigraph,solid-pod-embed broker_inbox
test handlers::broker_inbox_handler::tests::a_proposal_with_no_self_assessment_projects_absence_not_a_default ... ok
test handlers::broker_inbox_handler::tests::inbox_envelope_carries_cases_and_total ... ok
test handlers::broker_inbox_handler::tests::projects_proposal_into_bridge_case_shape ... ok
test handlers::broker_inbox_handler::tests::self_assessment_and_provenance_pass_through_when_the_agent_produced_them ... ok
test result: ok. 4 passed; 0 failed
```

## 3. `AgentContext.confidence` is `Option<f32>`, defaulting to absent

`crates/visionclaw-domain/src/types/ontology_tools.rs` and
`crates/visionclaw-ontology/src/types/ontology_tools.rs`:
`pub confidence: Option<f32>` with `#[serde(default)]`.

The three hardcoded `confidence: 0.5` sites on the elevation paths are now
`confidence: None`:

```
$ grep -rn "confidence: 0.5" --include=*.rs src/ crates/
src/services/natural_language_query_service.rs:263:                        confidence: 0.5,
```

The one remaining hit is a different type (`QueryTranslation`, an SPARQL
translation score) and is out of scope for this expectation.

The one reader, `ontology_mutation_service`'s PR body, now prints
`"not reported"` rather than a number for an absent confidence.

## 4. Rationale gate, and the human's words published verbatim

`brokerCaseQueue.ts`: `MIN_RATIONALE_CHARS = 20`, `rationaleRequired(tier)`
(true for `high`/`critical` only), `canPublishDecision(tier, rationale)`
(trimmed length ≥ 20). The card's Approve/Reject are `disabled={busy ||
!canPublish}`.

The gate trims; the **publish does not** — `onDecide` passes `rationale`
(untrimmed, as typed) and `undefined` when the human typed nothing, so the
decision carries the human's text byte-for-byte or carries none at all.

## 5. The fabricated rationale is deleted

The template `operator {outcome} via control centre` at `AcspCaseQueue.tsx:26`
is gone. Three further occurrences of the forum's sibling template
(`"Human approve via governance UI"`) survived as **test fixtures** in
`src/services/acsp/{client,events}.rs`, encoding the fabricated string as the
exemplar of a human rationale; they now carry a sentence a human could actually
have typed.

```
$ grep -rn "via control centre|via governance UI" client/src src/ \
    --include=*.ts --include=*.tsx --include=*.rs
client/src/features/control-center/governance/AcspCaseQueue.tsx:20://     `operator ${outcome} via control centre` template is deleted: a rationale
```

The single remaining hit is the comment recording the deletion. No producer or
fixture emits either template.

## 6. Self-assessment renders BELOW the controls

The card's DOM order is payload → provenance → rationale input → controls →
self-assessment. The test asserts index order between `acsp-case-controls` and
`acsp-case-self-assessment` within the card, not mere presence.

## Test run

```
$ ./node_modules/.bin/vitest run src/features/control-center/governance/
 ✓ src/features/control-center/governance/__tests__/brokerCaseQueue.test.ts (16 tests)
 ✓ src/features/control-center/governance/__tests__/AcspCaseQueue.test.tsx (7 tests)
 Test Files  2 passed (2)
      Tests  23 passed (23)

$ ./node_modules/.bin/eslint src/features/control-center/governance --ext ts,tsx --report-unused-disable-directives
(clean, exit 0)

$ ./node_modules/.bin/tsc --noEmit -p tsconfig.json
0 errors
```

## Counter-examples re-checked

| Counter-example | Status |
|---|---|
| Approve enabled on a `critical` case with an empty rationale | Prevented — test `disables a critical decision until 20 characters of rationale are typed` |
| Tier/confidence rendered above the controls | Prevented — DOM-order test |
| `reasoning` containing text the human did not type | Prevented — `body.reasoning` asserted byte-equal to the typed string; `undefined` when untyped |
| `confidence: 0.5` for a case where no model produced one | Prevented — `Option<f32>`/`undefined` end to end |

## Not covered

- The forum `ActionRow` half of this expectation (different repository).
- No browser run: assertions are jsdom-level via Testing Library.
- `decide()` still posts to the WS-9 operator route, which does not yet enforce
  the rationale server-side. The gate is client-side only; recorded as follow-on
  1 on ADR-2110, not closed.
