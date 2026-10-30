---
id: ADR-2101
title: Decision-elevation cases have a durable case-state authority
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: b0bc275f6501aae7751b85a72ce15fe1e730e7e8
verified_paths: []
owner: jjohare
review_trigger: a second consumer of decision-elevation case state, a change to the enrichment store schema or its category tagging, or evidence of duplicate PRs from resumed approvals; activates on the first live decision-elevation case decided over the relay that survives a real backend restart
repo: visionclaw
domain: BASELINE-architecture
---

# ADR-2101 — Decision-elevation cases have a durable case-state authority

## Context

`DecisionElevationActor` (ADR-050 write half) held every open governance case in
two in-process `HashMap`s: `pending` awaiting a human decision, `elevating`
awaiting a terminal git state. Nothing was written down. A crash — in
particular between the kind-31404 terminal publish and any local bookkeeping —
silently lost an open governance decision, and on restart nothing could tell
that a case had ever existed. The sibling `ElevationActor` already persisted
through `SqliteEnrichmentRepository`; the decision path had no repository field
at all. ADR-2006's closeout named this exact gap twice ("current elevation
processing owns pending state"; "the durable case-state authority is still not
named") and stayed `partial` on it. Diagram VC-24.6 carried it as a DIVERGENCE.

## Decision

The durable case-state authority for decision elevation is **the same
`data/enrichment.sqlite3` store the `ElevationActor` uses**, reached through a
typed facade, `DecisionElevationStore`
(`src/adapters/decision_elevation_store.rs`). Cases are `enrichment_proposals`
rows tagged `category = "decision-elevation"`; broker decisions are
`enrichment_decisions` rows written by `SqliteEnrichmentRepository::record_decision`,
so the ADR-2006 signed-event correlation columns (`decision_event_id`,
`decision_created_at_s`) and the re-delivery suppression they guard apply to
this path identically. No new table, no new database file, no new backup unit.

Every lifecycle transition is durable, and the in-memory maps are a cache of the
store, never the authority:

```
pending ──approve──▶ approved ──PR opened──▶ elevating ──merged──▶ published
   │                     │                       └──closed unmerged──▶ abandoned
   ├──reject──▶ rejected      ├──amend/delegate──▶ reviewed
   ├──31402 publish failed──▶ publish_failed
   └──unanswered past TTL──▶ expired (with a kind-31404 receipt)
```

Three orderings are policy, not incident:

1. **Durable-first on open.** The case row is written *before* the kind-31402
   publish, so no crash window can lose an open case. A publish that then fails
   closes the row out as `publish_failed` — nobody can answer a case the forum
   never saw, and leaving it open would strand it.
2. **One future per decision.** `record_decision` (→ `approved`) and
   `mark_elevating` (→ `elevating`) are sequenced inside a single future, so the
   slower network write can never land first and undo the faster one.
3. **Durable before in-memory on the PR.** The PR url is stamped into the store
   before the tracking-map insert, and the GOV-2 poll only drops a case from the
   map once its terminal status is durably written; a persist failure retries on
   the next poll instead of vanishing. The 31404 publish and the durable write
   still fail independently, mirroring the `ElevationActor`.

At boot the actor reloads every non-terminal case and applies
`plan_reconciliation` — a pure function, so every branch including the TTL
boundary is testable without a relay, a GitHub token or an actor system:

- `elevating` with a PR url → re-arm the GOV-2 merge poll.
- `approved` (or a half-written `elevating`) with no PR url → re-open the corpus
  PR; the draft is durable, so it need not be re-derived.
- `pending` within `OPEN_CASE_TTL` (14 days) → re-arm the case so a late
  kind-31403 is still matched.
- `pending` / `approved` past the TTL → expire with a kind-31404 receipt plus a
  terminal durable status.

A case tracking an open PR is **never** expired: a long-lived PR is legitimate
work the merge poll owns, and expiring it would fabricate a terminal state git
disagrees with.

## Consequences

- ADR-2006's unnamed authority is named for this consumer, and the diagram
  divergence at VC-24.6 is closed. The identical claim for the `ElevationActor`
  path was already true; both elevation paths now share one store and one
  decision-row shape.
- The actor's message contract is unchanged. `ActorElevationSink::elevate` and
  `DecisionElevationActor::new()` keep their signatures, so `main.rs` and
  `DecisionService` are untouched; `with_store` is an optional injection seam and
  the actor otherwise opens its own connection to the same file at boot (WAL,
  writes serialised by `tokio-rusqlite`).
- Failure stays fail-open. If the store cannot be opened the actor logs loudly
  and runs with the old in-process behaviour rather than refusing decisions; a
  decision-persist failure leaves the case non-terminal so reconciliation
  retries it.
- The panel state gains a `durable` flag, so an operator can see from the forum
  whether restart recovery is armed.
- **Cost — duplicate PR risk.** Resuming an `approved` case re-runs
  `create_ontology_pr`. If the previous process actually created the PR but died
  before `mark_elevating` persisted, the resume opens a second PR for the same
  decision. Losing the approval entirely is worse, so the resume is the right
  default, but the window is real and unclosed (see follow-on).

## Follow-on work (precise, not stubbed)

1. **Idempotent PR resume.** Before re-opening a PR for a resumed `approved`
   case, query GitHub for an open PR on the deterministic branch name that
   `create_ontology_pr` derives from `agent_id`, and adopt it instead of opening
   a second one. Needs a `GitHubPRService::find_pr_for_branch` that does not yet
   exist; that is why it is not in this change.
2. **Expiry of a stuck `elevating` case.** A PR that is neither merged nor closed
   is polled for ever. A separate, longer PR-age budget with an operator receipt
   belongs here, but it needs a policy decision on what "abandoned by the human"
   means for an open PR.
3. **`amend` / `delegate` semantics.** Both currently land as `reviewed` and stop.
   ADR-2006 lists defining them (and supersession) as still open; this record
   does not close that.
4. **Live proof.** No live relay, broker approval or GitHub PR operation ran in
   this change — see Verification.

## Verification

Implementation verified against the working tree at
`b0bc275f6501aae7751b85a72ce15fe1e730e7e8` (the tree carried uncommitted sprint
edits from parallel agents; this record's claims cover only the files listed
below).

Code:

- `src/adapters/decision_elevation_store.rs` (new, 481 lines; ~163 lines of
  non-comment code before the test module) — the facade, the status vocabulary
  and `is_terminal`, the `DecisionCase` ⇄ row mapping.
- `src/actors/decision_elevation_actor.rs` — `store` field, `with_store`,
  boot store attach + `Reconcile`, `plan_reconciliation`, `apply_reconciliation`,
  `spawn_decision_outcome` (which replaced the inline approve branch),
  `spawn_expiry`, `decision_record`, `terminal_case_status`, and durable writes
  in the `ElevateDecision`, `Decision` and `PollPrs` handlers.
- `src/adapters/mod.rs` — module registration and two re-exports (4 lines).

Commands, all run at the end of the change:

- `cargo check --workspace --all-targets` — exit 0 (also green as a baseline
  before the change, so the exit code is attributable).
- `cargo test -p visionclaw-server --lib decision` — **79 passed, 0 failed**,
  1260 filtered out. 19 of those are new: 6 store tests (field round trip,
  foreign-category rows ignored, pending→elevating→published transitions,
  `record_decision` moving the case status atomically plus ADR-2006 re-delivery
  returning the same row id, open cases surviving a real close/reopen of a temp
  SQLite file, unknown-case no-ops) and 13 actor tests (terminal-status mapping,
  `pending_from`, six `plan_reconciliation` branches including "a tracked PR is
  never expired however old", two `decision_record` correlation tests, and two
  integration-style tests over a temp SQLite file that close the process handle
  and then plan reconciliation from what survived).
- `cargo fmt --all --check` — exit 0.
- `cargo clippy -p visionclaw-server --lib --all-targets` — no findings in the
  touched files.

What the tests caught, recorded because it corrected a claim rather than the
code: the first draft asserted that the decision's `activity_urn` *contains*
`decelev-decide:<case>:<event_id>`. It does not — `uri::execution` content-
addresses the correlation string (`urn:visionclaw:execution:sha256-12-…`). The
assertions now test the property that actually matters: same signed event id ⇒
same URN (a re-delivery is recognisable), different event id ⇒ different URN (an
admin answering the same case twice the same way no longer collides).

**Not proven.** No live relay connection, no broker approval, no GitHub PR
creation, and no crash-and-restart of the real process ran. Restart recovery is
demonstrated at the store and planner boundary — a real SQLite file, all handles
dropped, reopened, reconciled — not against a killed server. The duplicate-PR
window in Consequences is reasoned, not measured.
