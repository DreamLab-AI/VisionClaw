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

## 6. The rationale gate is enforced SERVER-SIDE too

The client gate is a courtesy: any caller holding the credential could POST a
tiered decision carrying no rationale, and the record would hold a signed
judgement in nobody's words. The same rule therefore runs on
`apply_decision` — the one shared core both the service route
(`POST /api/enrichment-proposals/{id}/decide`, `X-Agent-Key`) and the operator
route (`POST /api/broker/cases/{id}/decide`, power-user session) funnel through.

A `high`/`critical` case whose decision carries no rationale, or under
`MIN_RATIONALE_CHARS` (20) after trimming, is refused with **HTTP 422** and a
structured body (`code: "rationale_required"`, `tier`, `min_chars`,
`received_chars`). The refusal happens **before** anything is minted, stubbed or
persisted, so a gated request leaves no partial state.

`check_rationale` is a predicate returning `Result<(), RationaleRejection>`: it
yields permission and nothing else, so it is structurally incapable of supplying
the text it demands. The 422 body deliberately carries no `reasoning` field — a
test asserts its absence.

```
\$ cargo test --lib --no-default-features --features ontology,persistence-oxigraph,solid-pod-embed enrichment_proposals
test handlers::enrichment_proposals_handler::tests::min_rationale_chars_matches_the_client_gate ... ok
test handlers::enrichment_proposals_handler::tests::an_untiered_or_low_case_may_be_decided_without_a_rationale ... ok
test handlers::enrichment_proposals_handler::tests::a_high_or_critical_case_is_rejected_without_a_rationale ... ok
test handlers::enrichment_proposals_handler::tests::a_short_or_whitespace_rationale_does_not_satisfy_the_gate ... ok
test handlers::enrichment_proposals_handler::tests::a_real_rationale_passes_and_is_never_rewritten_by_the_gate ... ok
test handlers::enrichment_proposals_handler::tests::every_human_outcome_family_is_gated ... ok
test handlers::enrichment_proposals_handler::tests::a_non_human_outcome_is_not_gated ... ok
test handlers::enrichment_proposals_handler::tests::the_declared_tier_is_read_from_the_proposal_body ... ok
test handlers::enrichment_proposals_handler::tests::the_rejection_serialises_as_a_structured_422_body ... ok
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 1404 filtered out
```

Covered: the 20-char threshold shared with the client; `low`/`medium`/untiered
stay optional; `high`/`critical` refused at 0 and at 19 characters and on
whitespace-only input (the count is of TRIMMED characters); accepted at exactly
20; all four human outcome families gated (`approve`, `reject`, `amend`,
`delegate`, and their spellings); non-human outcomes (`expired`, `precedent`) NOT
gated, so a system-produced terminal state is never asked for a judgement it did
not make; tier read from `risk_tier` or `tier` on the proposal body, with a blank
value treated as absent.

**Scope limit, recorded rather than hidden.** The tier consulted is the tier
RECORDED ON THE CASE, which today is the proposing agent's *declared*
`risk_tier`. PRD FR3's `effective_tier` — operator task properties bounding the
agent's declaration from below — is a nostr-rust-forum clause and has not landed,
so an agent that under-declares its own tier still escapes the gate. When FR3
lands, this gate should read the effective tier from the case row; it will
tighten, never loosen. Reading the declared tier meanwhile is strictly better
than reading nothing. A case with no stored row carries no tier and is not gated:
a first-contact decision from the bridge is not something this route can tier,
and inventing one would be as dishonest as inventing the rationale.

The Whelk-gate path is unaffected: `elevation_actor` writes its synthetic
rejection through `repo.record_decision` directly, not through `apply_decision`,
so `system:whelk-gate` is never asked for a human rationale.

**Three entry points, one gate.** The deepsec re-run established that
`apply_decision` has *three* callers, not the two named in its own doc comment:
the service route, the operator route, and `/api/ingest/writeback`. Placing the
gate inside the shared core rather than on the two handlers means all three are
covered, including the one neither the brief nor the module docs mentioned.

### Security gate on this increment

`deepsec-gate.sh --diff main` re-run after the change →
`deepsec-gate: PASS`, exit **0**, receipt
`.claude/evidence/deepsec-20260914T154635Z.receipt.json` (20 findings, **0 at or
above HIGH**). The wider diff surfaced two MEDIUMs against this handler, both
**pre-existing and not introduced or widened here**:

- `other-provenance-forgery` — `record_decision` accepts `broker_pubkey` from the
  request body on a syntactic hex check alone, with no signature binding the
  caller to that key, and that pubkey becomes `owner_did` and drives an
  owner-scoped Oxigraph write. Untouched by this change.
- `acl-check` — the three entry points into `apply_decision` carry divergent
  authorisation (`X-Agent-Key`, `power_user()`, and `/api/ingest/writeback` at
  the `RbacGate` default). The rationale gate now applies uniformly across all
  three; the *authorisation* asymmetry is a separate, older problem.

Both are recorded as follow-ons on ADR-2110 rather than fixed here.

## 7. Self-assessment renders BELOW the controls

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
| `reasoning` containing text the human did not type | Prevented — `body.reasoning` asserted byte-equal to the typed string; `undefined` when untyped; the server gate **refuses** rather than filling in |
| `confidence: 0.5` for a case where no model produced one | Prevented — `Option<f32>`/`undefined` end to end |

## Not covered

- The forum `ActionRow` half of this expectation (different repository).
- No browser run: assertions are jsdom-level via Testing Library.
- The server gate reads the case's DECLARED tier, not PRD FR3's effective tier,
  which has not landed. An agent that under-declares its own tier escapes it.
- No HTTP-level test: `apply_decision` needs a full `AppState` (Oxigraph, the
  client-coordinator actor), which no test in this handler constructs. The gate
  is tested as the pure predicate it is, and its single call site is the first
  statement after the case read in the shared core. The route wiring itself is
  unverified by test.
