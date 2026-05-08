# PRD: Agent Orchestration Improvements (ADR-031)

**Status**: Accepted
**Source**: Multica multi-agent analysis (`multica-ai/multica`)
**Priority**: P1 — reliability and throughput under load
**Companion ADRs:** [ADR-031](adr/ADR-031-layout-mode-system.md)
**Affects**: `agent_monitor_actor`, `task_orchestrator_actor`, `websocket_heartbeat`,
`client_coordinator_actor`, `events/bus`, `supervisor`

---

## 1. Problem Statement

A review of the Multica open-source multi-agent daemon surfaced seven coordination
patterns that address known weaknesses in VisionFlow's agent orchestration layer:

| # | Gap | Symptom |
|---|-----|---------|
| 1 | No round-robin fairness in agent polling | First agent in the list always gets the apex 3-D spiral position; polling priority never rotates |
| 2 | Task creation retries when already at capacity | HTTP retries waste time and API budget when the orchestrator is fully loaded |
| 3 | Agent status lags up to 3 s behind reality | Status UI shows stale idle/working state until the next poll cycle |
| 4 | Heartbeat is liveness-only | No mechanism to push server-side directives (e.g. force-sync, config-reload) to clients |
| 5 | Slow/dead WebSocket clients never evicted | `do_send` silently drops frames; full mailboxes go undetected, memory grows |
| 6 | A panicking event handler crashes the publish call | One bad handler can cascade to stop all event processing for that event type |
| 7 | No graceful drain on shutdown | Actors stop abruptly; in-flight tasks are abandoned without a wait window |

---

## 2. Goals

- Eliminate wasted HTTP retries when the orchestrator is at capacity.
- Reduce agent status staleness from ≤3 s to near-zero on task state transitions.
- Detect and evict slow/dead WebSocket clients during broadcast.
- Isolate panicking event handlers so other handlers in the same publish call still run.
- Provide a configurable drain window before actor shutdown.
- Rotate spiral position allocation fairly across agents.
- Enable heartbeat-carried server directives for operational control.

## 3. Non-Goals

- Per-agent GPU physics differentiation.
- Replacing the Management API with a push-based alternative.
- Changing the WebSocket binary protocol.

---

## 4. Detailed Requirements

### 4.1 Round-Robin Poll Offset (Item 1)

**File**: `src/actors/agent_monitor_actor.rs`

Add `poll_offset: usize` field to `AgentMonitorActor`. On each successful poll cycle,
increment `poll_offset` (wrapping). In the golden-angle spiral position computation,
use `spiral_i = (i + poll_offset) % agent_count` as the spiral index instead of `i`.

*Acceptance criteria*: With 5 agents and `poll_offset=1`, agent 0 occupies spiral
position 1 instead of 0.

---

### 4.2 Capacity-Aware Task Claiming (Item 2)

**File**: `src/actors/task_orchestrator_actor.rs`

Add `max_concurrent_tasks: usize` field (env `MAX_CONCURRENT_TASKS`, default 20).
In `CreateTask` handler, count tasks with `status == Running` and return
`Err("At capacity: N/M tasks running")` before issuing any HTTP calls if the
count is at the ceiling.

*Acceptance criteria*: With `max_concurrent_tasks=1` and one running task, a second
`CreateTask` returns `Err` without contacting the Management API.

---

### 4.3 Observational Status Inference (Item 3)

**Files**: `src/actors/messages/agent_messages.rs`,
`src/actors/task_orchestrator_actor.rs`,
`src/actors/agent_monitor_actor.rs`

Add `TaskStatusChanged { agent_type, running_task_count }` message.
Add `SetAgentMonitorAddr { addr }` message.

After a `CreateTask` succeeds, `TaskOrchestratorActor` sends `TaskStatusChanged`
to the registered `AgentMonitorActor` address. The monitor handles this by calling
`poll_agent_statuses` immediately, bypassing the 3-second interval.

*Acceptance criteria*: A `TaskStatusChanged` message causes an immediate re-poll
without waiting for the next interval tick.

---

### 4.4 Heartbeat Directive Carrying (Item 4)

**File**: `src/utils/websocket_heartbeat.rs`

Add `HeartbeatDirective` enum with variants:
- `ReloadConfig`
- `ForceFullSync`
- `UpdateAvailable { version: String }`

Add `directives: Vec<HeartbeatDirective>` field to `CommonWebSocketMessage::Pong`
(skip-serialising when empty). Add default-empty `get_pending_directives()` method
to `WebSocketHeartbeat` trait. Update `send_pong` to include directives.

*Acceptance criteria*: `Pong` with one directive serialises to JSON including the
directive type. Callers that do not override `get_pending_directives` receive an
empty list.

---

### 4.5 WebSocket Backpressure Detection and Client Eviction (Item 5)

**File**: `src/actors/client_coordinator_actor.rs`

Add `BroadcastResult { sent: usize, slow_clients: Vec<usize> }` struct.
Change `broadcast_to_all` and `broadcast_with_filter` to use `Addr::try_send`
instead of `do_send`. Collect `Full` and `Closed` error client IDs into
`slow_clients`. Update all call sites to evict slow clients under a write lock
after releasing the read lock.

*Acceptance criteria*: `BroadcastResult::slow_clients` is non-empty when a client
actor mailbox is full or closed. Evicted clients are removed from
`ClientManager::clients`.

---

### 4.6 Panic Isolation in Event Handlers (Item 6)

**File**: `src/events/bus.rs`

In `execute_handler_concurrent`, wrap the retry loop in `tokio::task::spawn`.
Catch `JoinError` (handler panic) and convert to `EventError::Handler(...)`.
This ensures a panicking handler is reported as a partial failure rather than
unwinding the publish call.

*Acceptance criteria*: A handler that calls `panic!()` does not prevent sibling
handlers from executing. The `publish` call returns a partial-failure `Ok(())`
(or `Err` only when ALL handlers fail).

---

### 4.7 Graceful Task Drain on Shutdown (Item 7)

**Files**: `src/actors/task_orchestrator_actor.rs`, `src/actors/supervisor.rs`

#### TaskOrchestratorActor

Add `accepting_tasks: bool` field (default `true`) and
`DrainTasksBeforeShutdown { timeout_secs }` message.
Handler sets `accepting_tasks = false`, then polls every second until running
task count reaches 0 or the timeout expires, then calls `ctx.stop()`.

`CreateTask` checks `accepting_tasks` first and returns an immediate error if
draining.

#### SupervisorActor

Add `draining: bool` field and `InitiateGracefulShutdown { timeout_secs }` message.
Handler sets `draining = true`, schedules `ctx.stop()` after the timeout.
`RegisterActor` returns an error when `draining == true`.

*Acceptance criteria*: After `DrainTasksBeforeShutdown`, `CreateTask` returns
`Err("draining")`. After `InitiateGracefulShutdown`, `RegisterActor` returns an
error.

---

## 5. Implementation Plan

| Phase | Work | Files |
|-------|------|-------|
| P0 | Add messages, mod re-exports | `agent_messages.rs`, `mod.rs` |
| P1 | Items 2 & 7a (task actor) | `task_orchestrator_actor.rs` |
| P1 | Items 1 & 3 (monitor actor) | `agent_monitor_actor.rs` |
| P1 | Item 4 (heartbeat) | `websocket_heartbeat.rs` |
| P1 | Item 5 (backpressure) | `client_coordinator_actor.rs` |
| P1 | Item 6 (panic isolation) | `events/bus.rs` |
| P1 | Item 7b (supervisor drain) | `supervisor.rs` |
| P2 | Tests | `tests/actors/orchestration_improvements_test.rs` |

---

## 6. Testing Strategy

- **Unit**: Round-robin offset arithmetic, capacity ceiling logic, heartbeat
  directive serialisation, `BroadcastResult` structure.
- **Integration (actix::test)**: Drain mode rejection, supervisor drain flag,
  `TaskStatusChanged` triggering re-poll.
- **Panic isolation**: Deploy a panicking handler alongside a counting handler;
  verify counting handler still fires.

---

## 7. Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| `try_send` raises false evictions under momentary load | Log at `warn`, not `error`; eviction is observable |
| `tokio::spawn` lifetime constraints on `EventHandler` | `Arc<dyn EventHandler + 'static>` + owned event clone satisfies `'static` |
| Drain interval fires before test assertions | Use `timeout_secs: 30` in tests to prevent premature stop |

---

*References*: `multica-ai/multica` — daemon orchestration source, heartbeat implementation,
event bus panic isolation pattern.
