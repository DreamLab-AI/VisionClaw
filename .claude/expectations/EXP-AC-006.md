---
id: EXP-AC-006
parent_spec: PRD-augmentation-conditions FR6
linked_adrs: [ADR-2010]
priority: high
regression_critical: true
evidence_category: executable
status: accepted
authored_by: pair
---

## Expectation: Reviewers are measured, juniors can be delegated real cases, and reviewers keep exposure to routine and known-bad agent output

`GET /api/governance/reviewers` (admin NIP-98) returns for each deciding pubkey: `decisions`, `median_ttd_ms`, `p90_ttd_ms`, `override_rate`, `superseded`, `calibration_shown`, `calibration_decided`, `probes_seen`, `probes_caught`. A `reviewer`-role pubkey's 31403 is rejected (403) unless an admin has published `Delegate{to: that pubkey}` on that case, after which exactly that case admits it and the chain shows both events. With `calibration_sample_rate = 0.1`, over 1,000 `Low`/`Reversible` requests between 80 and 120 are marked `calibration_sample` and none are member-suppressed. A 31402 tagged `probe=<sha>` from the panel's probe agent renders without any probe indication until its 31403 exists. VisionClaw `CaseView.createdAt` is present and the queue sorts oldest first. The dream-cycle ledger row schema includes `Reviewer` and `Review-minutes`.

### In scope
- Telemetry read model and its auth
- Delegation admission gate and chain integrity
- Deterministic sampling bounds
- Probe blindness

### Out of scope (intentionally)
- Reviewer scoring or ranking UI

### Counter-examples (must NOT happen)
- A reviewer deciding a case not delegated to them
- Probe tag visible on a pending card
- Sampling that depends on wall-clock time (must be a hash of the request id)
