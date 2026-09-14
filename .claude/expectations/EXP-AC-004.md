---
id: EXP-AC-004
parent_spec: PRD-augmentation-conditions FR4
linked_adrs: [ADR-2010]
priority: critical
regression_critical: true
evidence_category: executable
status: accepted
authored_by: pair
---

## Expectation: A human learns whether their decision applied, a stalled case ages visibly, and a restarted actor recovers its cases

`POST /api/governance/receipts/{response_event_id}/application` (NIP-98) advances the receipt to `consumer-received`, then to exactly one of `applied | not-applied | applied-manually`; a regression (e.g. `applied` then `consumer-received`) returns HTTP 409 and leaves the row unchanged; `applied-manually` from a non-admin returns 403. agentbox `broker-bridge` posts both stages after `ApplicationReceiptStore.begin/finish`; a failed post writes an `authority.receipt-post-failed` journal record. The relay cron marks any `Open`/`UnderReview` case older than the panel's `max_pending_hours` (default 72) with an `escalated-on-age` receipt exactly once. Authority-gate denials append `authority.deny {stage, reason}` to the execution journal and appear at `/v1/agent-events`. VisionClaw `ElevationActor` expires pending cases at 14 days with an `expired` receipt and reconciles `pending` rows at boot so a decision arriving after restart is applied rather than dropped.

### In scope
- Endpoint auth (registered agent for consumer-received/applied/not-applied; admin for applied-manually)
- Monotonic stage machine with 409 on regression
- Cron idempotency (one receipt per case)
- ElevationActor TTL, boot reconciliation, post-restart decision handling

### Out of scope (intentionally)
- Multi-relay receipt federation

### Counter-examples (must NOT happen)
- A decision projected `projection-committed` forever while the mutation failed, with no receipt
- Two `escalated-on-age` receipts for one case
- A post-restart 31403 returning early at the in-memory map miss
