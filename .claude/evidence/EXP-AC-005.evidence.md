---
expectation_id: EXP-AC-005
git_sha: 4a9a3e0682bdc695a8ebf904e0453271443daea8
produced_by: agent:claude-opus
produced_at: 2026-09-14T15:29:17Z
audited_by: agent:claude-sonnet-5 (degraded: same family as producer; codex GPT-6 Astra unavailable — bwrap sandbox refused in container)
audited_at: 2026-09-14T20:20:00Z
auditor_verdict: fail
auditor_counter_examples_attempted: 5
auditor_counter_examples_found: 1
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

## Auditor adversarial probes

All probes added as throwaway `#[test]` cases (to `intent_match.rs`,
`sqlite_kpi_repository.rs`), run, then reverted — `git status --porcelain`
confirmed clean on each file afterward. Nothing committed except this
evidence file.

**Probe 1 — CONFIRMED DEFECT: a declared target that is a strict prefix of a
DIFFERENT recorded `target_urn` false-positives as a match.**
```
$ cargo test --lib ... intent_match::tests::auditor -- --nocapture
PROBE prefix/superstring target result = Some(true)
thread '...auditor_probe_target_superstring_prefix_false_positive' panicked:
assertion `left == right` failed: node-7 vs node-70 are DIFFERENT targets; containment falsely matches them
  left: Some(true)
 right: Some(false)
```
`intent_match(Some("update urn:kg:node-7"), Some("graph_update"), Some("urn:kg:node-70"))`
returns `Some(true)`. `src/services/intent_match.rs:150`
(`token_matches`) does plain substring containment in either direction
(`d.contains(&r) || r.contains(&d)`) with **no segment-delimiter boundary
check** — unlike its sibling `urn_names_case` in `kpi_compute.rs:241-248`,
whose doc comment explicitly explains *why* a plain `contains` is unsafe for
URNs ("would let `vc-elev-foo` match a URN naming `vc-elev-foo-bar`"). The
same reasoning applies here and was not applied: an agent that declared
intent to touch `urn:kg:node-7` and instead recorded an act against
`urn:kg:node-70` (a different node) is reported `intent_match: Some(true)` —
the exact false agreement the module's own docs say semantic/fuzzy matching
must never manufacture ("Matching is exact token containment... a fuzzy
matcher would manufacture agreement, which is precisely the failure mode this
context exists to remove", `intent_match.rs:12-14`). This directly weakens
`DecidedCase::is_warranted()` (`kpi_compute.rs:181-193`): `intent_mismatch`
would be `false` for a genuinely diverged act, so `hitl_precision`'s
`warranted` count is silently deflated whenever the mismatched target happens
to share a numeric/alphanumeric prefix with the correct one — an increasingly
likely collision on sequentially-minted URNs like `node-7`/`node-70`,
`node-7`/`node-71`...`node-79`, or content-addressed URNs sharing a hash
prefix.

**File:line: `src/services/intent_match.rs:144-151` (`token_matches`).**

**Probe 2 — operation-only intent, nothing recorded at all.**
```
PROBE operation-only, nothing recorded = Some(false)
```
Matches the documented "declared but absent from the record → `Some(false)`"
rule. No counter-example.

**Probe 3 — whitespace-only intent.**
```
PROBE whitespace-only intent = None
```
Matches documented `None` (no claim to check). No counter-example.

**Probe 4 — HITL Precision zero-decided and whelk-gate-only windows.** Not
independently re-probed with new test code; re-read the producer's existing
`hitl_precision_with_no_decided_cases_is_a_value_of_none` and
`a_gate_only_window_reports_no_value_rather_than_zero` tests
(`kpi_compute.rs:796, 853`) and confirmed by code inspection that
`hitl_precision` filters `is_system_actor` BEFORE computing `decided`
(`kpi_compute.rs:213-230`), so a whelk-gate-only window correctly yields
`decided: 0, value: None`. No counter-example found on inspection.

**Probe 5 — a legacy `kpi_agent_events` row inserted before the `intent`
column existed, read back after `apply_additive_migrations` runs.**
```
PROBE legacy row read back: [AgentTrajectoryRow { event_id: 99, ... intent: None, ... }]
test ...auditor_probe_legacy_row_predating_intent_column_reads_back_as_null ... ok
```
Built a genuinely pre-migration schema by hand (table with no `intent`
column), inserted a row directly via raw SQL, then opened it through
`SqliteKpiRepository::open` (which runs `apply_additive_migrations`) and read
it back via `trajectories_since`. The row survives, `intent` reads `None`
(never a synthesised value), other columns intact. **No counter-example** —
the additive-migration path is genuinely backward-compatible, not merely
tested against a same-session-created DB as the producer's own
`declared_intent_round_trips_verbatim_and_absence_stays_null` test does.

**Verdict: FAIL on Probe 1.** The producer's evidence documents `intent_match`
as PASS against all stated counter-examples, and its own table tests never
exercise a target that is a *proper prefix/superstring* of a *different*
recorded target — only exact matches and fully-differing ones
(`"urn:kg:node-7"` vs `"urn:kg:node-99"`, `intent_match.rs:220-229`, no shared
prefix collision). The mandate's specific adversarial prompt ("a target URN
that is a prefix/superstring of the recorded one — segment-delimited?")
surfaces a real gap: it is NOT segment-delimited, and the module's sibling
correlation function in the same codebase (`urn_names_case`) demonstrates the
producer/codebase already knows the correct fix pattern but did not apply it
here.

## Not covered

No live-traffic run: the elevation actor needs `FORUM_RELAY_URL` plus a relay,
neither available here. Every claim above is unit-level. `/api/trace` was not
exercised over HTTP; the handler is an unmodified pass-through of
`ProvenanceTraceService::query`, whose join is tested directly.

## Iteration after audit

Commit: `b2baa2d16b58bf9d030a41c7b0880df14eb0d803`
Closes the auditor's **Probe 1 FAIL** above: `intent_match`'s declared-target
comparison was plain substring containment, so a declared `urn:kg:node-7`
matched a recorded `urn:kg:node-70`.

The auditor's diagnosis was exact — the correct pattern already existed in the
same codebase (`kpi_compute::urn_names_case`) and had simply not been applied
here. Rather than copy it, the rule is lifted into one shared helper,
`intent_match::urn_names_segment`, which `urn_names_case` now delegates to; the
`/api/trace` intent verdict and the KPI case correlation are now structurally
incapable of drifting apart.

`token_matches` is split rather than tightened, because the two declared
components warrant different rules and a uniform segment rule would have broken
operation matching: `update` is a genuine part of `graph_update`, and `_` is not
a URN delimiter. So `operation_matches` keeps containment; `target_matches`
requires whole `:`/`/`-delimited segments in either direction.

### Failing first

```
$ cargo test --lib --no-default-features \
    --features ontology,persistence-oxigraph,solid-pod-embed intent_match
thread '...::a_declared_target_matches_on_whole_segments_not_substrings' panicked
  at src/services/intent_match.rs:285:9:
assertion `left == right` failed
  left: Some(true)
 right: Some(false)
test result: FAILED. 9 passed; 1 failed; 0 ignored; 0 measured; 1414 filtered out
```

`left: Some(true)` is the defect itself: the declared `urn:kg:node-7` against a
recorded `urn:kg:node-70`, reported as a match.

### After the fix

```
$ cargo test --lib --no-default-features \
    --features ontology,persistence-oxigraph,solid-pod-embed intent_match
test services::intent_match::tests::a_declared_target_matches_on_whole_segments_not_substrings ... ok
test services::intent_match::tests::a_declared_operation_that_differs_is_a_mismatch ... ok
test services::intent_match::tests::a_declared_component_with_nothing_recorded_cannot_match ... ok
test services::intent_match::tests::an_operation_only_intent_matches_on_the_operation_alone ... ok
test services::intent_match::tests::a_declared_target_that_differs_is_a_mismatch ... ok
test services::intent_match::tests::an_unparseable_intent_declares_nothing_and_cannot_be_verified ... ok
test services::intent_match::tests::declared_operation_and_target_both_found_is_a_match ... ok
test services::intent_match::tests::matching_is_case_insensitive_and_tolerates_affixes ... ok
test services::intent_match::tests::no_intent_is_unknown_never_a_verdict ... ok
test services::intent_match::tests::parse_splits_operation_from_target ... ok
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 1414 filtered out

# the shared helper does not regress the correlation it was lifted from
$ cargo test --lib --no-default-features \
    --features ontology,persistence-oxigraph,solid-pod-embed \
    a_case_is_correlated_only_on_whole_urn_segments
test services::kpi_compute::tests::a_case_is_correlated_only_on_whole_urn_segments ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1423 filtered out

$ cargo test --lib --no-default-features \
    --features ontology,persistence-oxigraph,solid-pod-embed
test result: ok. 1418 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out
```

The new test asserts all four cases the auditor's mandate named, including the
two the producer's original table never exercised:

| declared | recorded | verdict | why |
|---|---|---|---|
| `urn:kg:node-7` | `urn:kg:node-70` | `Some(false)` | the auditor's counter-example: a superstring is a different node |
| `urn:kg:node-7` | `urn:kg:node-7` | `Some(true)` | the same declaration still holds against the node named |
| `urn:kg` | `urn:kg:node-7` | `Some(true)` | a coarser claim ON a segment boundary is a real claim the record bears out |
| `urn:kg:node` | `urn:kg:node-7` | `Some(false)` | a prefix stopping mid-segment names nothing (`-` is not a delimiter) |

### Still not covered

Unchanged from the scope note above: no live-traffic run, no HTTP exercise of
`/api/trace`. `urn_names_segment` is byte-exact and case-insensitivity is
applied by the caller, so a URN differing only by percent-encoding or Unicode
normalisation still reads as a mismatch — correct for the ids these surfaces
actually mint, but not a general URN equivalence.
