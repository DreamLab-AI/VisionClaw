---
expectation_id: EXP-AC-006
git_sha: 4a9a3e0682bdc695a8ebf904e0453271443daea8
produced_by: agent:claude-opus
produced_at: 2026-09-14T15:31:46Z
audited_by: agent:claude-sonnet-5 (degraded: same family as producer; codex GPT-6 Astra unavailable — bwrap sandbox refused in container)
audited_at: 2026-09-14T20:20:00Z
auditor_verdict: pass
auditor_counter_examples_attempted: 2
auditor_counter_examples_found: 0
---

# Evidence — EXP-AC-006 (case ageing and ordering, VisionClaw clause)

**Scope note.** EXP-AC-006 is mostly a nostr-rust-forum expectation. This
evidence covers **only** PRD FR6.5 — `CaseView` gains `createdAt`, and the queue
sorts oldest first and shows age. The reviewer-telemetry endpoint (FR6.1),
delegation admission (FR6.2), calibration sampling (FR6.3), seeded probes
(FR6.4) and the dream-cycle ledger columns (FR6.6) are **NOT** evidenced here and
remain open.

## The change

`InboxCase` gains `created_at_ms` (already served at the top level of each
`/api/broker/inbox` case by `broker_inbox_handler.rs`; the client simply never
read it). `CaseView.createdAt` carries it through `num()`, so a row without a
timestamp stays `undefined` rather than defaulting to now — an unknown age is
not an age of zero.

`sortOldestFirst(views)` returns a **new** array, oldest first, with undated
cases last. `caseAgeLabel(createdAt, now)` renders the largest whole unit
(`4d`, `3h`, `5m`, `just now`) and `undefined` for an undated case, so the badge
is simply absent rather than fabricated.

`AcspCaseQueue` applies `sortOldestFirst` to the pending list under `useMemo`,
and each card badges its age when it has one.

## Test run

```
$ ./node_modules/.bin/vitest run src/features/control-center/governance/
 ✓ brokerCaseQueue.test.ts (16 tests)
 ✓ AcspCaseQueue.test.tsx (7 tests)
 Test Files  2 passed (2)
      Tests  23 passed (23)
```

FR6.5-specific:

| Test | Asserts |
|---|---|
| `sorts oldest first, with undated cases last` | full ordering over four cases |
| `does not mutate the input array` | the sort is non-destructive |
| `labels case age in the largest whole unit` | the four labels, and `undefined` for undated |
| `carries the full proposal payload, URNs, reasoning and provenance` | `createdAt` populated from `created_at_ms` |
| `renders absence as absence` | `createdAt` `undefined` when the row has none |
| `shows the oldest case first and labels its age` | rendered order + a `4d` badge |

Backend: `a_proposal_with_no_self_assessment_projects_absence_not_a_default`
asserts `created_at_ms` survives the projection.

```
$ ./node_modules/.bin/eslint src/features/control-center/governance --ext ts,tsx
(clean, exit 0)
$ ./node_modules/.bin/tsc --noEmit -p tsconfig.json
0 errors
```

## Counter-examples re-checked

| Counter-example | Status |
|---|---|
| A reviewer deciding a case not delegated to them | Not applicable (FR6.2, forum clause, not evidenced) |
| Probe tag visible on a pending card | Not applicable (FR6.4, forum clause, not evidenced) |
| Sampling that depends on wall-clock time | Not applicable (FR6.3, forum clause, not evidenced) |

## Auditor adversarial probes

Both probes added as throwaway vitest cases to `brokerCaseQueue.test.ts`, run,
and reverted (`git status --porcelain` confirmed clean afterward). Nothing
committed except this evidence file.

**Probe 1 — clock-skew: a `createdAt` in the FUTURE relative to `now`.**
```
PROBE future createdAt label = just now
```
`caseAgeLabel` clamps `now - createdAt` to `Math.max(0, ...)`, so a clock-skewed
or malformed future timestamp renders `"just now"` rather than a negative or
garbage age. **No counter-example.**

**Probe 2 — two cases sharing an identical `createdAt` — stable relative
order?**
```
PROBE equal-createdAt order = [ 'a', 'b' ]
```
`Array.prototype.sort` is stable in the JS engines this ships to (ES2019+
guarantee); confirmed empirically rather than assumed. **No counter-example.**

**Verdict: PASS for the scope evidenced (FR6.5 only).** Note: a broader probe
of the client-side `MIN_RATIONALE_CHARS` counting (astral-plane Unicode
divergence from the server) surfaced during this audit is an EXP-AC-002
finding, not FR6.5 — recorded there, not duplicated here.

## Not covered

- Every clause but FR6.5, as set out in the scope note.
- `now` is read once per render (`Date.now()` in the component body), so age
  labels refresh on the next render rather than ticking. Acceptable for a queue
  that refreshes on every broker event; noted rather than hidden.
