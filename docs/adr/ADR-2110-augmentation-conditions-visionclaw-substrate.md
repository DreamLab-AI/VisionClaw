---
id: ADR-2110
title: Instrument the VisionClaw judgment surfaces against the augmentation conditions
date: 2026-09-14
decision_status: accepted
implementation_status: complete
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4a9a3e0682bdc695a8ebf904e0453271443daea8
verified_paths: [src/services/intent_match.rs, src/services/kpi_compute.rs, src/actors/elevation_actor.rs, src/adapters/sqlite_kpi_repository.rs, src/adapters/sqlite_enrichment_repository.rs, src/handlers/broker_inbox_handler.rs, client/src/features/control-center/governance/brokerCaseQueue.ts, client/src/features/control-center/governance/AcspCaseQueue.tsx]
owner: jjohare
review_trigger: The forum half of EXP-AC-002/004/006 landing, or the first live case queue with real decided cases
repo: visionclaw
---

# ADR-2110 — Instrument the VisionClaw judgment surfaces against the augmentation conditions

## Context

The broker loop is cryptographically complete and semantically hollow
(PRD-augmentation-conditions). On the VisionClaw substrate specifically: the case
card showed a two-line clipped preview and published
`operator ${outcome} via control centre` as the human's reasoning; `AgentContext`
had a non-optional `confidence` so the elevation paths passed `0.5`, which the UI
then rendered as the agent's own judgement; the `ElevationActor` held open cases
only in a process-local `HashMap`, so a kind-31403 arriving after a restart was
dropped at the map miss; declared `intent` rode the wire but was never persisted,
so no comparison with the act was possible; HITL Precision was a hardcoded
`awaiting_data_source` stub; and the EL++ consistency gate's own rejections were
attributed to the human who opened the case.

## Decision

VisionClaw owns four things in the Augmentation Conditions context (DDD §8), and
implements them as follows.

**Intent persistence and matching (FR5.1–5.2).** `kpi_agent_events.intent` stores
the envelope's `intent` verbatim; `None` persists as `NULL` and is never
synthesised from `action_type_name`. `/api/trace` reports `intent` and a
three-valued `intent_match` from the pure `services::intent_match`. The rule is
exact, case-insensitive token containment over an operation and a target: **every
component the agent actually declared must be found, and at least one must have
been declared**. `null` means "no claim was made" and is never conflated with
`false`, "the claim did not hold". *(The brief summarised this as requiring both
components; the expectation text — "`false` when a declared target differs from
the recorded one" — is narrower, and the narrower reading is implemented, so an
operation-only intent is not failed for a claim it never made.)*

**HITL Precision (FR5.3).** Warranted ÷ decided over human-decided cases, one row
per case (its terminal decision), with the denominator always reported. Warranted
= outcome family ≠ requested-action family, or `amend`/`delegate`, or
`intent_match == false` on a trajectory the case owns. `value` is
`Option<f64>` and is `None` when `decided == 0` — a ratio over an empty
denominator is undefined and the dashboard says so. The `awaiting_data_source`
branch for this KPI is deleted.

**System actors are not humans (FR5.4, DDD invariant 8).** The gate's synthetic
rejection carries `system:whelk-gate` as its decider. Such rows are excluded from
the Trust Variance human-outcome series **and from both terms** of HITL
Precision, so a window of gate rejections reports no value rather than a
precision no human earned.

**Case recovery (FR4.5).** `OPEN_CASE_TTL` is 14 days, matching
`decision_elevation_actor`. Boot reconciliation runs before the first cycle:
durable `pending` rows rehydrate into the working set, and anything past the TTL
is closed with a kind-31404 `elevation_expired` receipt plus a terminal durable
status that is not conditional on the receipt publishing. A row with no draft is
never rehydrated — fabricating one would commit text no agent authored.

**Non-vacuous decision surface (FR2.3–2.4, FR6.5).** The case card renders the
full proposal payload pretty-printed and unclipped, with proposal URN, reasoning
summary, reasoning hash and generation provenance each shown only when present.
The human types their own rationale and that text is published byte-for-byte;
nothing is published when they typed nothing. On `high`/`critical` the decision
is disabled below 20 trimmed characters. The agent's self-assessment renders
BELOW the controls, where it cannot anchor the reviewer. `AgentContext.confidence`
is `Option<f32>`, so absence is representable and renders as absence. The queue
sorts oldest first and badges each case's age.

## Consequences

- HITL Precision and Trust Variance become comparable across windows for the
  first time, because the series they read is now human-only.
- Three status values now appear on the KPI summary: `computed`,
  `awaiting_data_source` (Mesh Velocity only) and `no_decided_cases`. A dashboard
  that switches on `status == "computed"` handles the third correctly by
  rendering no value, which is the intent.
- `AgentContext.confidence` is a breaking change for any out-of-tree constructor;
  all three in-tree ones are updated and the single reader prints
  "not reported" for absence.
- A reviewer on a `high` case cannot decide without typing 20 characters. The
  gate is client-side only — the WS-9 operator route does not yet enforce it.
- `decisions_since` now returns `KpiDecisionRow` rather than a tuple. One caller.
- Correlating a trajectory to a case still rests on the case id appearing in
  agent-supplied URNs. The match is now segment-delimited, closing the
  cross-case attribution deepsec raised, but an explicit `case_id` column on the
  trajectory row would be better.

### Follow-on work (open, not closed by this ADR)

1. The rationale gate is client-side. The WS-9 decide route should reject a
   `high`/`critical` decision carrying no rationale.
2. `elevation_actor`'s approve path trusts relay-side admission of a kind-31403
   and holds no in-process admin allowlist before spending a GitHub write token
   (deepsec `acl-check`, MEDIUM). Pre-existing, identical to the finding standing
   against `decision_elevation_actor`; not widened here.
3. Expiry idempotency across repeated boots is structural (the `expired` status
   removes the row from the scan), not tested.
4. The forum halves of EXP-AC-002, EXP-AC-004 and EXP-AC-006 are open. This ADR
   closes only the VisionClaw clauses.

## Verification

At `verified_commit`, on the non-GPU feature set — the default set includes CUDA
`gpu`, which cannot link in this container; **no GPU-gated code is touched**:

```
$ cargo test --lib --no-default-features \
    --features ontology,persistence-oxigraph,solid-pod-embed
test result: ok. 1408 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out

$ cargo clippy --lib --no-default-features \
    --features ontology,persistence-oxigraph,solid-pod-embed
exit 0 — warning count identical to the `main` baseline (770), i.e. no new lints

$ cd client && ./node_modules/.bin/vitest run src/features/control-center/governance/
 Test Files  2 passed (2)
      Tests  23 passed (23)

$ ./node_modules/.bin/eslint src/features/control-center/governance --ext ts,tsx
(clean, exit 0)

$ ./node_modules/.bin/tsc --noEmit -p tsconfig.json
0 errors

$ .claude/skills/build-with-quality/scripts/deepsec-gate.sh --diff main
deepsec-gate: PASS — 7 finding(s) {CRITICAL:0, HIGH:0, MEDIUM:5, HIGH_BUG:1, BUG:1, LOW:0},
0 at/above HIGH; exit 0
receipt .deepsec-gate/reports/20260914T152023Z/receipt.json
```

The run's one finding in new code — an unbounded substring correlation between
cases and trajectories over agent-controlled URNs — was fixed before commit
(`urn_names_case`, segment-delimited, two regression tests). The rest are
pre-existing or out of scope, itemised in the evidence files.

Per-expectation evidence with commands, trimmed raw output and per-clause
verdicts: `.claude/evidence/EXP-AC-{002,004,005,006}.evidence.md`. Each carries a
scope note naming the clauses it does **not** cover.

`activation_status: inactive` — nothing here is deployed. No launch, build or
docker command was run; every claim above is unit-level, from a working tree.
