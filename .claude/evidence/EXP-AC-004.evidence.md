---
expectation_id: EXP-AC-004
git_sha: 4a9a3e0682bdc695a8ebf904e0453271443daea8
produced_by: agent:claude-opus
produced_at: 2026-09-14T15:31:46Z
audited_by:
---

# Evidence — EXP-AC-004 (ElevationActor TTL, boot reconciliation, expiry receipt)

**Scope note.** EXP-AC-004 spans three codebases. This evidence covers **only**
PRD FR4.5 — the VisionClaw `ElevationActor` clause. The receipts endpoint
(FR4.1), the agentbox broker-bridge receipt post (FR4.2), the relay ageing cron
(FR4.3) and the authority-deny journal (FR4.4) belong to nostr-rust-forum and
agentbox and are **NOT** evidenced here. Those clauses remain open.

## The defect

`Handler<Decision>` opened with
`let Some(case) = self.pending.remove(&d.case_id) else { return; }`
(`src/actors/elevation_actor.rs`). `pending` is an in-process `HashMap` built
only by `RunCycle` as it opens new cases, so after a restart it was empty: a
signed kind-31403 for a case opened before the restart hit that miss and returned
early. The human's decision was dropped silently and the durable row sat
`pending` for ever. This is the EXP-AC-004 counter-example "a post-restart 31403
returning early at the in-memory map miss".

## The change

`OPEN_CASE_TTL = 14 days` — the same constant
`decision_elevation_actor.rs` uses, so sibling panels do not age cases on
different clocks. A `Reconcile` message is sent once the ACSP client connects
and **before** `RunCycle`, so the case-window budget counts recovered cases.

Its handler reads durable `pending` rows, rehydrates the ones belonging to this
panel, and applies the pure policy:

- `recovered_case(&StoredProposal) -> Option<RecoveredCase>` — rejects a case id
  outside `CASE_PREFIX` (another panel's row), a terminal status, or a body
  carrying no draft/target path. The last is deliberate: an `approve` commits
  the stored draft, and fabricating a replacement would commit text no agent
  authored and no human reviewed.
- `plan_elevation_reconciliation(cases, now_s, ttl_s)` — resume inside the TTL,
  expire outside it. The boundary is **exclusive**: a case exactly at the TTL is
  still answerable.
- `split_reconciliation(plan)` — the working set to re-arm, and the cases to
  expire.

Expiry mirrors `decision_elevation_actor::spawn_expiry`: a kind-31404
`elevation_expired` receipt (best-effort) followed by the terminal `expired`
durable status, which is **not** conditional on the receipt publishing — a relay
that is down must not leave a case permanently un-aged.

## Test run

```
$ cargo test --lib --no-default-features --features ontology,persistence-oxigraph,solid-pod-embed elevation_actor
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 1378 filtered out
```

Eight of those are new:

| Test | Asserts |
|---|---|
| `ttl_is_fourteen_days_matching_the_decision_elevation_actor` | `OPEN_CASE_TTL == 14 * 86400` |
| `a_pending_row_rehydrates_into_the_working_set` | draft, path and label recovered from the durable row |
| `a_row_from_another_panel_is_not_ours_to_recover` | a `vc-decelev-` row is ignored |
| `a_row_with_no_draft_cannot_be_rehydrated` | no draft ⇒ no recovery, never a fabricated one |
| `reconciliation_resumes_a_case_inside_the_ttl_and_expires_one_outside_it` | both branches |
| `the_ttl_boundary_is_exclusive` | exactly-at-TTL still answerable |
| `terminal_rows_are_never_reconciled` | five terminal statuses all skipped |
| `a_decision_arriving_after_a_restart_finds_its_case` | **the restart-then-decide scenario** |

The last is the regression itself: a cold map drops the case id, the same case
is reconciled from its durable row, and the identical lookup then resolves and
yields the recovered draft path.

## Security gate

`deepsec-gate.sh --diff main` → `deepsec-gate: PASS`, exit **0**, receipt
`.deepsec-gate/reports/20260914T152023Z/receipt.json` (0 at/above HIGH). One
MEDIUM (`acl-check`) was raised against `elevation_actor.rs:L1184-L1245`: the
approve path trusts relay-side admission of a kind-31403 and holds no in-process
admin allowlist before spending a GitHub write token. This is **pre-existing**
(the identical finding stands against `decision_elevation_actor.rs`), is not
introduced or widened by this change, and is recorded as follow-on 2 on ADR-2110
rather than fixed here.

## Counter-examples re-checked

| Counter-example | Status |
|---|---|
| A post-restart 31403 returning early at the in-memory map miss | Fixed — `a_decision_arriving_after_a_restart_finds_its_case` |
| Two `escalated-on-age` receipts for one case | Not applicable to FR4.5 (relay cron clause, not evidenced) |
| A decision projected `projection-committed` for ever with no receipt | Not applicable to FR4.5 (receipts-endpoint clause, not evidenced) |

## Not covered

- Everything outside FR4.5, as set out in the scope note.
- Expiry idempotency across repeated boots is structural, not tested: the
  `expired` status write removes the row from the `pending` scan, so a second
  boot cannot re-expire it. No test exercises two boots.
- No live relay: the 31404 receipt publish is unit-untested. The durable status
  write beside it is the part that must not depend on the relay, and it does not.
