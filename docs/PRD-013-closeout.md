# PRD-013 Close-Out: Solid Pod Git Ingest Surface

**Date:** 2026-05-08
**Author:** Dr John O'Hare / Architecture Agent
**Status:** G1-G7 + R1-R5 Implemented — only Phase 7 (GitHub REST deprecation) remains
**Parent:** PRD-013-solid-git-ingest-surface.md

---

## Delivery Summary

All seven PRD-013 goals are implemented, QE-hardened, committed, and pushed.

| Goal | Component | Impl Location | Status |
|------|-----------|---------------|--------|
| G1 | Git Ingest Adapter | `src/services/git_ingest/mod.rs` | Done |
| G2 | DID-Gated Remote Registry | `src/services/git_ingest/remote_registry.rs` | Done |
| G3 | Provenance Commit Encoder | `src/services/git_ingest/provenance.rs` | Done |
| G4 | Write-Back Saga | `src/services/git_ingest/writeback_saga.rs` | Done |
| G5 | Agentbox Pod Bridge | `agentbox/management-api/routes/git-bridge.js` | Done |
| G6 | Broker Review Surface | `agentbox/management-api/routes/broker-bridge.js` | Done |
| G7 | Nostr Control Plane | `src/actors/server_nostr_actor.rs`, `broker_actor.rs` | Done |

### QE Hardening Applied

Rust: 13 items (C1, C2, H1, H2, H3, H4, H5, H6, H7, H8, M2, M4, M6, M7, L1, L2).
JS: 9 items (agent DID binding, webhook HMAC, CORS, SSE cap, env filtering, 5xx scrub, .git block, arg injection, URL suppression).

---

## Remaining Work

Five structural items and one full deprecation phase remain. None block production use of G1-G7.

### R1: Broker WebSocket Broadcast Wiring

**What:** `BrokerActor.client_coordinator` is `None` in production. All `broadcast()` calls (new_case, case_claimed, case_decided, case_updated) silently drop.

**Why it's unwired:** `ClientCoordinatorActor` is created inside `GraphServiceSupervisor::started()` and is not exposed as a standalone address. Wiring it requires either:
- (a) Extract the address from the supervisor via a new query message, or
- (b) Create `ClientCoordinatorActor` externally in main.rs and inject it into both the supervisor and the BrokerActor.

**Impact:** Broker review pane (G6) works via REST polling but does not receive real-time push events. Agentbox SSE bridge in `broker-bridge.js` can poll but cannot stream.

**Effort:** ~2h. Option (b) is cleaner — main.rs already creates other actors externally.

**Files:**
- `src/main.rs` (~735): add `.with_client_coordinator(client_coordinator_addr)`
- `src/actors/graph_service_supervisor.rs` (~276-290): use externally-provided path

### R2: Legacy BrokerRepository Type Unification

**What:** Two incompatible `BrokerCase` types exist:
- `models::enterprise::BrokerCase` — used by `BrokerRepository` port, `Neo4jBrokerRepository` adapter, `ShareOrchestrator`, `KpiComputationService`
- `domain::broker::BrokerCase` — used by `BrokerActor`, `DecisionOrchestrator`, enrichment handler

The `BrokerActor` has no persistence — its inbox cache is session-scoped. Cases and decisions are lost on restart.

**Why it's unwired:** The two types have different field structures (priority enum vs u8, CaseStatus vs CaseState, evidence vec vs history vec, String timestamps vs DateTime). A projection layer is needed.

**Impact:** Broker cases survive only in memory. Low impact for enrichment flow (write-back saga has its own audit trail in Neo4j), but means broker inbox is empty after restart.

**Effort:** ~4h. Write `impl From<domain::broker::BrokerCase> for models::enterprise::BrokerCase>` projection, wire `app_state.broker_repository` into BrokerActor, persist on submit/decide.

**Files:**
- New: `src/adapters/broker_case_projection.rs` (~50 lines)
- `src/actors/broker_actor.rs`: re-add repository field, persist in handlers
- `src/main.rs`: add `.with_repository(app_state.broker_repository.clone())`

### R3: Precedent-Based Auto-Approval

**What:** `DecisionOutcome::Precedent { scope }` exists in the domain model but has no automation. Today it simply marks a case as precedent — no matching logic exists to auto-approve future similar cases.

**Design (from PRD-013 §Open Questions #4):** After N approvals of the same enrichment type for the same entity class, the broker can flag as Precedent. Future cases matching that scope skip human review. Target: 40% of routine embedding updates auto-approved.

**Effort:** ~6h. Precedent registry (in-memory or Neo4j), match function in `DecisionOrchestrator`, auto-decide path in `SubmitBrokerCase` handler.

**Files:**
- `src/domain/broker/broker_decision.rs`: precedent matching logic
- `src/actors/broker_actor.rs`: auto-approve check on submit
- New: precedent registry (or extend existing Neo4j adapter)

### R4: Conflict Resolution on Write-Back Push

**What:** If the source remote has diverged since our last fetch, `WriteBackSaga::execute()` will fail at the push phase with a non-fast-forward error. The error is logged and the saga returns `Err`, but there is no structured notification or retry path.

**Design (from PRD-013 §Open Questions #1):** Fail-and-notify on conflict. The broker re-reviews after manual resolution. Auto-merge is dangerous for knowledge bases.

**Effort:** ~3h. Add conflict detection in push error path, emit a dedicated `broker:push_conflict` event with the case id and remote id, optionally create a new BrokerCase for re-review.

**Files:**
- `src/services/git_ingest/writeback_saga.rs`: classify push errors
- `src/actors/broker_actor.rs`: handle conflict notification

### R5: NIP-17 Human-Agent Messaging Surface (DONE)

**Implemented.** `SignSealedDM` (kind 14) on `ServerNostrActor` supports broker-to-agent sealed DMs with optional subject tags.

### R6: Agent Control Surface Protocol Integration (DONE)

**What:** `ServerNostrActor` now signs and publishes two Agent Control Surface Protocol event types:
- `PublishGovernancePanel` (kind 31400): NIP-33 parameterized replaceable event that registers/updates governance control panels on the Nostr relay. `BrokerActor` sends this to define panel schemas (ActionInbox, Dashboard, ConfigForm, StatusBoard, ChatBridge) with field definitions, actions, layout hints, and capabilities.
- `PublishActionRequest` (kind 31402): NIP-33 event submitted when a case needs human review, carrying case fields, category, priority, and agent reasoning.

Both kinds are added to `SUPPORTED_KINDS` in `server_identity.rs` and tracked by `NostrKind::K31400`/`K31402` Prometheus counters in `metrics.rs`. Wire format is compatible with `nostr-bbs-core`'s `PanelDefinition` and `ActionRequest` schemas.

---

## Phase 7: GitHub REST API Deprecation

The largest remaining workstream. Not blocked by R1-R5.

### Scope

Remove the legacy GitHub REST API ingest path and require all knowledge sources to use the git-over-HTTP ingest surface (G1/G2).

### Files to Remove

| File | Lines | Purpose |
|------|-------|---------|
| `src/services/github/api.rs` | ~509 | GitHub REST API client |
| `src/services/github/config.rs` | ~221 | GITHUB_* env var parsing |
| `src/services/github/content_enhanced.rs` | ~588 | EnhancedContentAPI (tree endpoint) |
| `src/services/github/types.rs` | ~222 | GitHub-specific response types |
| `src/services/github/pr.rs` | ~185 | PR creation (write-back via PR, replaced by git push) |
| `src/services/github/mod.rs` | ~22 | Module re-exports |
| **Total** | **~1,747** | |

### Files to Modify

| File | Change |
|------|--------|
| `src/services/github_sync_service.rs` (~1,266 lines) | Rewrite to use `GitIngestService` instead of `EnhancedContentAPI`. The sync loop, SHA1 delta logic, and `SyncStatistics` remain — only the fetch backend changes. |
| `src/services/file_service.rs` (~1,282 lines) | Remove `GitHubClient` dependency. File processing pipeline stays; input source changes from GitHub API response to local git worktree files. |
| `src/main.rs` | Remove `GITHUB_*` env var reads, remove `github_sync_service` construction from `EnhancedContentAPI`, update data orchestration to use git-ingest. |
| `src/app_state.rs` | Remove `GitHubConfig` field if present. |
| `.env.example` | Remove `GITHUB_TOKEN`, `GITHUB_OWNER`, `GITHUB_REPO`, `GITHUB_BASE_PATH`. Add `GIT_REMOTES` JSON example. |

### Migration Path

1. Legacy `GITHUB_*` env vars already auto-register as a PAT remote (`id = "legacy-github"`) via `git_ingest_registry.legacy_github_shim()` in main.rs.
2. Operators switch to `GIT_REMOTES` JSON or the REST API (`POST /api/ingest/remotes`).
3. After migration window (1 sprint), remove legacy shim and `src/services/github/`.
4. `EnhancedContentAPI` tree endpoint can be retained as an optimisation path (single API call vs full clone) behind a feature flag if operators need it for large repos.

### Effort

~2-3 days. The git-ingest surface already handles everything GitHub REST did. The work is mostly plumbing replacement and test migration.

---

## Implementation Status

| # | Item | Status | Commit |
|---|------|--------|--------|
| R1 | Broadcast wiring | **Done** | `with_client_coordinator()` wired in main.rs |
| R2 | Type unification + persistence | **Done** | `broker_case_projection.rs` + `with_repository()` wired |
| R3 | Precedent auto-approval | **Done** | `PrecedentRegistry` in domain, auto-approve in BrokerActor |
| R4 | Push conflict notification | **Done** | `is_conflict()` + classified errors in writeback_saga |
| R5 | NIP-17 messaging | **Done** | `SignSealedDM` (kind 14) on ServerNostrActor |
| R6 | Agent Control Surface Protocol | **Done** | `PublishGovernancePanel` (kind 31400), `PublishActionRequest` (kind 31402) on ServerNostrActor |
| Phase 7 | GitHub REST deprecation | **Planned** | ~2-3 days, not blocked by R1-R6 |

---

## Open Questions Carried Forward

From PRD-013 §Open Questions, updated with current status:

| # | Question | Status |
|---|----------|--------|
| 1 | Conflict resolution on push | Addressed by R4 above. Design: fail-and-notify. |
| 2 | Enrichment file format standardisation | Deferred. VisionClaw-specific schema is acceptable for MVP. Revisit when third-party agents submit enrichments. |
| 3 | Multi-remote write-back | Deferred. Current design: first `writeback_enabled` remote wins. No operator has requested multi-remote yet. |
| 4 | Precedent-based auto-approval | Addressed by R3 above. |

---

## Acceptance Criteria for Full Close-Out

PRD-013 is fully closed when:

- [x] R1: Broker broadcasts reach clients in real-time
- [x] R2: Broker cases persist across restarts
- [x] R3: Precedent auto-approval for qualifying enrichments
- [x] R4: Push conflicts produce structured notifications
- [x] R5: NIP-17 sealed DM capability for broker ↔ agent dialogue
- [ ] Phase 7: `src/services/github/` directory deleted, all remotes use git-over-HTTP
- [ ] All success metrics from PRD-013 §Success Metrics are measurable (dashboards or log queries exist)
