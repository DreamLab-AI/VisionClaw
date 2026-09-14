---
expectation_id: EXP-AC-005
git_sha: 4a9a3e0682bdc695a8ebf904e0453271443daea8
produced_by: agent:claude-opus
produced_at: 2026-09-14T15:29:17Z
audited_by:
---

# Evidence — EXP-AC-005 (intent persistence, intent_match, HITL Precision)

Implementation commit `4a9a3e0682bdc695a8ebf904e0453271443daea8`; the same
commit sha is recorded on ADR-2110 as `verified_commit`.

## Build/test configuration

The default feature set includes CUDA `gpu`, which cannot link in this
container, so every command below runs the non-GPU feature set the brief names.
**No GPU-gated code is touched by this change.**

## 1. `intent` is persisted verbatim; absence stays NULL

Column added by `migrations/sqlite/0006_kpi_agent_event_intent.sql` and by the
embedded self-bootstrapping schema
(`src/adapters/sqlite_kpi_repository.rs` `CREATE_SCHEMA` +
`apply_additive_migrations`). The hub tap writes `env.intent.clone()`
(`src/services/kpi_compute.rs`, `run_agent_event_tap`) — no fallback, no
derivation from `action_type_name`.

```
$ cargo test --lib --no-default-features --features ontology,persistence-oxigraph,solid-pod-embed sqlite_kpi_repository
running 3 tests
test adapters::sqlite_kpi_repository::tests::agent_event_volume_window_count ... ok
test adapters::sqlite_kpi_repository::tests::declared_intent_round_trips_verbatim_and_absence_stays_null ... ok
test adapters::sqlite_kpi_repository::tests::snapshot_persists_with_queryable_lineage ... ok
test result: ok. 3 passed; 0 failed
```

**Verdict: PASS.** `intent` round-trips byte-for-byte; an envelope with no
intent stores `NULL`.

## 2. `/api/trace` returns `intent` and a three-valued `intent_match`

`TraceRecord` gains `intent` and `intent_match`
(`src/services/provenance_trace.rs`); the matcher is the pure
`src/services/intent_match.rs`.

```
$ cargo test --lib --no-default-features --features ontology,persistence-oxigraph,solid-pod-embed intent_match
running 9 tests
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 1402 filtered out

$ cargo test --lib --no-default-features --features ontology,persistence-oxigraph,solid-pod-embed provenance_trace
test result: ok. 7 passed; 0 failed
```

Table tests cover: no intent → `None`; blank intent → `None`; unparseable
intent → `None`; operation+target both found → `Some(true)`; differing target →
`Some(false)`; differing operation → `Some(false)`; declared component with
nothing recorded → `Some(false)`; case-insensitivity; the structured
`op=…`/`target=…` form and the positional/prose form.

**Divergence recorded honestly.** The brief summarised the rule as "`Some(true)`
if *both* a declared operation token and the declared target are found". The
binding expectation text is narrower — "`false` when a declared target differs
from the recorded one" — so the implemented rule is: *every component the agent
actually declared must be found, and at least one must have been declared*. An
operation-only intent therefore matches on the operation alone rather than
failing for a claim it never made. Documented in `intent_match.rs` module docs
and in ADR-2110 §Decision.

**Verdict: PASS.**

## 3. HITL Precision computed, denominator reported, no zero-denominator value

```
$ cargo test --lib --no-default-features --features ontology,persistence-oxigraph,solid-pod-embed kpi_compute
running 22 tests
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 1392 filtered out
```

Covered: empty window → `value: None, warranted: 0, decided: 0`; outcome equal to
request → not warranted; outcome differing → warranted; `approved` answering
`approve` → NOT an override (outcome families compared, not spellings);
`amend`/`delegate` → always warranted; `intent_match == false` → warranted;
warranted ÷ decided arithmetic (1/4 = 0.25).

The `awaiting_data_source` branch for `hitl_precision` is deleted; the tile is
`computed` with numerator/denominator/sample_count and a persisted snapshot with
lineage, or `no_decided_cases` with **no value** when `decided == 0`.

**Verdict: PASS.**

## 4. Whelk-gate rejections are not human decisions

`src/actors/elevation_actor.rs` — the synthetic gate rejection now carries
`responder_pubkey: SYSTEM_WHELK_GATE` ("system:whelk-gate") instead of the
human responder's pubkey, which `decision_record` persists as `broker_pubkey`
(read back under its domain name `decided_by`) with `attributed = false` and
`owner_did = None` — a reserved non-DID actor, exactly as the PRD specifies.

`kpi_compute::human_decision_outcomes` drops those rows from the Trust Variance
series and from its lineage; `hitl_precision` drops them from BOTH terms, so a
window of gate rejections only reports `value: None` rather than a precision no
human earned.

```
test services::kpi_compute::tests::whelk_gate_rejections_are_not_human_decisions ... ok
test services::kpi_compute::tests::a_gate_only_window_reports_no_value_rather_than_zero ... ok
test services::kpi_compute::tests::trust_variance_human_series_excludes_the_whelk_gate ... ok
test services::kpi_compute::tests::is_system_actor_recognises_only_the_reserved_gate_identity ... ok
```

**Verdict: PASS.**

## 5. Security gate

`deepsec-gate.sh --diff main` raised a BUG against the first version of
`trajectory_names_case`: the case↔trajectory correlation used a plain substring
over agent-controlled URNs, so an agent could attach a mismatched intent to
another agent's case. Fixed before commit — `urn_names_case` now requires whole
delimited segments, with two regression tests
(`a_case_is_correlated_only_on_whole_urn_segments`,
`an_intent_mismatch_on_a_neighbouring_case_does_not_bleed_across`).

## Counter-examples re-checked

| Counter-example | Status |
|---|---|
| `intent` populated from `action_type_name` after the fact | Absent — the tap writes `env.intent` only; no fallback exists in the code path |
| HITL precision reported as a value with `decided == 0` | Prevented — `value: Option<f64>`, `None` at zero; test `hitl_precision_with_no_decided_cases_is_a_value_of_none` |
| A Whelk rejection counted as a human override | Prevented — excluded from both HITL terms and the Trust Variance series |

## Not covered

No live-traffic run: the elevation actor needs `FORUM_RELAY_URL` plus a relay,
neither available here. Every claim above is unit-level. `/api/trace` was not
exercised over HTTP; the handler is an unmodified pass-through of
`ProvenanceTraceService::query`, whose join is tested directly.
