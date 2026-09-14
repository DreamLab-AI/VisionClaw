---
id: EXP-AC-005
parent_spec: PRD-augmentation-conditions FR5
linked_adrs: [ADR-2010]
priority: high
regression_critical: true
evidence_category: executable
status: accepted
authored_by: pair
---

## Expectation: Declared intent is persisted verbatim, compared with the act, and feeds a HITL Precision with a real denominator

`kpi_agent_events.intent` stores the envelope's `intent` exactly; a row for an envelope with no intent has `NULL`, never a synthesised string. `/api/trace` returns `intent` and `intent_match` (`true` when the declared operation and target both appear in the recorded `action_type_name`/`target_urn`, `false` when a declared target differs from the recorded one, `null` when no intent). `kpi_compute::hitl_precision` returns `{value, warranted, decided}` where `warranted` counts decided cases whose outcome ≠ the request's action, or `Amend`/`Delegate`, or `intent_match == false`; the `awaiting_data_source` branch is deleted. Rows decided by `system:whelk-gate` are excluded from the Trust Variance human series and from `decided`.

### In scope
- Migration adding the column; backward-compatible read of old rows
- Matching rule with three-valued result
- KPI denominator reported

### Out of scope (intentionally)
- Semantic similarity matching of intent text (exact token containment only)

### Counter-examples (must NOT happen)
- `intent` populated from `action_type_name` after the fact
- HITL precision reported as a value with `decided == 0`
- A Whelk rejection counted as a human override
