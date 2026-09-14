---
id: EXP-AC-002
parent_spec: PRD-augmentation-conditions FR2
linked_adrs: [ADR-2010]
priority: critical
regression_critical: true
evidence_category: executable
status: accepted
authored_by: pair
---

## Expectation: A 31403 records the human's own rationale, formed with the proposal in view, and never a fabricated one

On the forum decision card and the VisionClaw case queue, the proposed change (`ActionRequest.fields` / proposal payload), `context_url` and the agent's reasoning are rendered in full and the agent's declared tier and confidence appear below the Approve/Reject controls. A text input collects the human's rationale. When the effective tier is `high` or `critical`, the publish action is disabled until the rationale has at least 20 characters. The published 31403 `reasoning` (and VisionClaw's `CaseDecision.reasoning`) equals the typed text byte-for-byte. The literal `"Human {action} via governance UI"` and any equivalent template no longer exist in either codebase. VisionClaw's `AgentContext.confidence` is `None` unless a real value was produced, and the UI renders nothing for `None`.

### In scope
- Forum `ActionRow`, VisionClaw `AcspCaseQueue`
- Tier-gated mandatory rationale (high, critical); optional on low/medium
- Byte-equality of typed text and published content
- `fields` of arbitrary JSON shape rendered (pretty-printed) without truncation

### Out of scope (intentionally)
- Rationale quality scoring
- `Amend` UI (DDD §9.3)

### Counter-examples (must NOT happen)
- Approve button enabled on a `critical` case with an empty rationale
- Tier/confidence rendered above the controls
- `reasoning` containing text the human did not type
- `confidence: 0.5` appearing for a case where no model produced a confidence
