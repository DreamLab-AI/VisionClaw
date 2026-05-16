# WORKTREE-PLAN — Phase 3: Binary Protocol & Broadcast

Branch  : `impl/phase-3-binary-protocol`
Baseline: `radical-rollback @ d260a6158`
Depends : Phase 1 (Persistence ports — GraphStateActor reads), Phase 2.5 (auth compile-gates, `--allow-skip-auth` flag)
Author  : anthropic@xrsystems.uk
Date    : 2026-05-16

---

## 1. Phase 3 Task Breakdown

Each task maps to one or more PRD-02 acceptance criteria, carries an effort
estimate (S = ½d, M = 1d, L = 2-3d, XL = 4-5d), and lists hard dependencies.

---

### T-01  Remove V4 delta protocol code

**Complexity**: S
**Files affected**:
- `src/utils/binary_protocol.rs` — delete `DeltaNodeData`, `DELTA_*` constants,
  `PROTOCOL_V4` const, all `encode_delta_frame` / `decode_delta_frame` functions
- `src/handlers/socket_flow_handler/position_updates.rs` — remove delta-branch
  dispatch logic (lines that check `PROTOCOL_V4` or route to delta encoder)
- `src/utils/delta_encoding.rs` — delete file entirely (referenced only from
  position_updates.rs)

**Acceptance criteria**: A1 (V3 only). After deletion `grep -r 'V4\|DeltaNodeData\|delta_encoding' src/` returns zero hits outside test tombstone comments.

**Dependencies**: none — standalone deletion task.

---

### T-02  Remove BroadcastOptimizer

**Complexity**: M
**Files affected**:
- `src/gpu/broadcast_optimizer.rs` — delete file
- `src/gpu/mod.rs` lines 65-67 — remove `pub mod broadcast_optimizer` and `pub use broadcast_optimizer::{...}`
- `src/gpu/backpressure.rs` — remove the BroadcastOptimizer reference in the module doc (line 9)
- `src/actors/gpu/force_compute_actor.rs` — remove `use crate::gpu::broadcast_optimizer::*` import (line 18), remove `broadcast_optimizer: BroadcastOptimizer` field (line 107), remove construction (line 208), and all call sites of `broadcast_optimizer.process_frame()` and `broadcast_optimizer.reset_delta_state()` (lines 719, 1407, 1437, 1441, 1480, 1562)

**Acceptance criteria**: A1 + ADR-02 D5. After deletion the `ForceComputeActor` struct contains no `broadcast_optimizer` field. `cargo build` must succeed.

**Dependencies**: T-01 (cleaner diff context); can be done in parallel with T-01.

---

### T-03  Remove BinaryFrameCoalescer

**Complexity**: S
**Files affected**: Locate and delete any `BinaryFrameCoalescer` struct and its
module file (referenced in ADR-02 D6 via commits `348d23c62` and `17c0f913a`).
Search: `grep -r 'BinaryFrameCoalescer\|frame_coalescer' src/`.
Remove all call sites that drain or push to the coalescer.

**Acceptance criteria**: ADR-02 D6. `grep -r 'Coalescer\|coalesce' src/` returns zero hits.

**Dependencies**: none.

---

### T-04  Purge `broadcast_interval` fields from `ClientCoordinatorActor`

**Complexity**: S
**Files affected**:
- `src/actors/client_coordinator_actor.rs` — remove three fields `broadcast_interval: Duration` (line 455), `active_broadcast_interval: Duration` (line 458), `stable_broadcast_interval: Duration` (line 461) and their initialisations (lines 523-525); remove `update_broadcast_interval()` method (lines 748-768); remove all reads of `self.broadcast_interval` in the broadcast-gate predicate (line 767) and telemetry reporting (lines 1094, 1106, 1120, 1148-1157)

**Acceptance criteria**: PRD-02 §7 smell list. `grep -n 'broadcast_interval' src/actors/client_coordinator_actor.rs` returns zero hits.

**Dependencies**: T-09 (broadcast actor replaces the cadence logic these fields implement; do T-04 last among the removal tasks or coordinate carefully).

---

### T-05  Add `current_snapshot()` to `GraphStateActor`

**Complexity**: M
**Files affected**:
- `src/actors/graph_state_actor.rs` — add `current_snapshot() -> PositionFrameSnapshot` method that reads `self.graph_data` positions and returns a plain struct; add the message `GetPositionFrameSnapshot` and its handler
- `src/actors/messages/graph_messages.rs` — add `GetPositionFrameSnapshot` message type and `PositionFrameSnapshot` response type (`node_id: u32, x: f32, y: f32, z: f32, vx: f32, vy: f32, vz: f32` per node, plus `node_count: u32`)
- `src/actors/messages/mod.rs` — re-export

**Acceptance criteria**: ADR-02 D4. Unit test: `GraphStateActor` seeded with 3 nodes responds to `GetPositionFrameSnapshot` with correct per-node data. The method does not consult `ForceComputeActor` directly.

**Dependencies**: Phase 1 (persistence port must be in place so `GraphStateActor` has canonical position data to read).

---

### T-06  Redirect `GET /api/graph/positions` to read from `GraphStateActor`

**Complexity**: M
**Files affected**:
- `src/handlers/api_handler/graph/mod.rs` — rewrite `get_graph_positions()` (lines 497-545): replace the `ForceComputeActor::GetCurrentPositions` call path with `GraphStateActor::GetPositionFrameSnapshot`; encode the result in the same V3 binary format using the new encoder from T-07 (or JSON if the endpoint currently returns JSON — keep existing wire type, switch source)
- `src/app_state.rs` — expose `graph_state_actor_addr` for use by the handler (or retrieve via the supervisor `GetGraphStateActor` message already wired at line 575)

**Acceptance criteria**: ADR-02 D4 + A5. Integration test: POST a known position set to `GraphStateActor`, then `GET /api/graph/positions` and assert positions match. The endpoint must not query GPU.

**Dependencies**: T-05.

---

### T-07  Implement `BinaryV3Frame` encoder and decoder

**Complexity**: M
**Files affected**:
- `src/utils/binary_protocol.rs` — add the `BinaryV3Frame` struct (see §6 below for exact shape); add `BinaryV3Frame::encode(nodes: &[NodePositionRecord]) -> Vec<u8>` and `BinaryV3Frame::decode(bytes: &[u8]) -> Result<BinaryV3Frame>`, with monotonic `frame_id` tracked per-call-site (passed in, not global); write the header, per-node payload (28 bytes: `u32 node_id, f32 x, f32 y, f32 z, f32 vx, f32 vy, f32 vz`), and trailer `u32 node_count`

**Acceptance criteria**: A1. Round-trip test: encode 5 000 nodes, decode, assert every field matches. Size test: 5 000-node frame == 140 012 bytes (8 header + 5000×28 + 4 trailer).

**Dependencies**: T-01 (V4 code removed so no versioning ambiguity).

---

### T-08  Remove `LayoutHeartbeat` event from physics

**Complexity**: S
**Files affected**:
- `docs/migration-sprint/01-gpu-physics/DDD-01.md` — remove `LayoutHeartbeat` bullet from Domain events section (per T3 resolution)
- `src/actors/gpu/force_compute_actor.rs` — remove `iters_since_full >= 300` branches (lines 1448-1480, 1528-1562) that emit iteration-counted broadcasts; remove any `LayoutHeartbeat` variant from the physics event enum
- `src/actors/physics_orchestrator_actor.rs` — remove 60 Hz broadcast throttle logic (lines 538-590, 1100-1178 per T3 resolution)
- Any physics event enum definition — remove `LayoutHeartbeat` variant

**Acceptance criteria**: ADR-01 D9 (T3 resolution). `grep -r 'LayoutHeartbeat' src/` returns zero hits. Physics domain event enum has exactly four variants: `LayoutStarted`, `LayoutSettled`, `LayoutDestabilised`, `PhysicsClamped`.

**Dependencies**: T-09 (the broadcast actor must own the heartbeat before removing iteration-based triggers, to avoid a window with no heartbeat at all).

---

### T-09  Implement `BroadcastActor` with ACTIVE / SETTLED / SHUTDOWN state machine

**Complexity**: XL
**Files affected**:
- `src/actors/broadcast_actor.rs` — create new file; full implementation (see §2 state machine below)
- `src/actors/mod.rs` — add `pub mod broadcast_actor; pub use broadcast_actor::BroadcastActor`
- `src/actors/messages/broadcast_messages.rs` — add messages: `RegisterBroadcastClient { client_id, addr }`, `UnregisterBroadcastClient { client_id }`, `OnLayoutStarted`, `OnLayoutSettled`, `OnLayoutDestabilised`, `OnPhysicsClamped`, `TriggerHeartbeat` (internal, timer-driven), `BroadcastTick` (internal)
- `src/actors/messages/mod.rs` — re-export new message types
- `src/app_state.rs` — spawn `BroadcastActor` on startup; wire to `GraphStateActor` and `SocketFlowServer`
- `src/main.rs` — subscribe `BroadcastActor` to physics event stream (emitting `OnLayoutStarted`, etc. to `BroadcastActor` when ForceComputeActor emits them)

**Acceptance criteria**: A2 (settlement-gated cadence), A3 (backpressure drop), A6 (≤50ms p99 tick-to-wire), A8 (wall-clock heartbeat). See §2 state machine for transition table.

**Dependencies**: T-05 (snapshot source), T-07 (encoder), T-08 (clean physics event set — coordinate so the LayoutHeartbeat removal and BroadcastActor creation happen in the same PR to avoid a gap).

---

### T-10  Wire `BroadcastActor` to `SocketFlowServer` client registry

**Complexity**: M
**Files affected**:
- `src/handlers/socket_flow_handler/mod.rs` — on `RegisterClient` message, forward `RegisterBroadcastClient` to `BroadcastActor`; on `UnregisterClient`, forward `UnregisterBroadcastClient`
- `src/actors/client_coordinator_actor.rs` — remove the `BroadcastPositions` message handler that currently drives position sends; delegate broadcast path to `BroadcastActor`; retain `ClientCoordinatorActor` for connection state management and auth only
- `src/handlers/socket_flow_handler/http_handler.rs` — on new WebSocket upgrade, propagate client addr to `BroadcastActor` via `RegisterBroadcastClient`

**Acceptance criteria**: A2 + A3. A cold-connecting client is registered within the same request lifecycle as the WebSocket handshake.

**Dependencies**: T-09.

---

### T-11  Implement per-client `frame_id` counter and backpressure check

**Complexity**: M
**Files affected**:
- `src/actors/broadcast_actor.rs` — add `frame_ids: HashMap<ClientId, u32>` to actor state; increment on each successful send; reset to 0 on `OnLayoutStarted`; check `buffered_amount` before each send and drop frame for that client if `> 64 * 1024` bytes; increment a `frames_dropped_total` counter metric

**Acceptance criteria**: A3 (drop semantics), A7 (ADR-02 D7 drop metric). Test: saturate one mock client's buffer, assert drop counter increments, assert next frame is delivered (buffer cleared by test).

**Dependencies**: T-09, T-10.

---

### T-12  `BroadcastActor` sends position frame immediately on new client connect

**Complexity**: S
**Files affected**:
- `src/actors/broadcast_actor.rs` — in `handle(RegisterBroadcastClient)`: immediately call `GraphStateActor::current_snapshot()` and send the encoded V3 frame to the newly registered client, regardless of current broadcast-actor state (ACTIVE or SETTLED)

**Acceptance criteria**: PRD-02 §3 "Client connecting cold" (within 500ms of handshake). Test: register a mock client, assert it receives exactly one V3 frame within 100ms of registration.

**Dependencies**: T-09, T-10.

---

### T-13  Auth gating on WebSocket upgrade (`/wss`)

**Complexity**: M
**Files affected**:
- `src/handlers/socket_flow_handler/http_handler.rs` — integrate the compile-time `#[cfg(any(debug_assertions, feature = "dev-auth"))]` gate from Phase 2.5: when the feature is absent, require `?token=<nostr_jwt>`; when the flag is present and `--allow-skip-auth` is active, allow upgrade without token; reject with 403 otherwise

**Acceptance criteria**: A7 (ADR-02 D8). Two integration tests: (1) production-mode upgrade without token → 403, (2) dev-mode upgrade without token with `--allow-skip-auth` → 101 Switching Protocols.

**Dependencies**: Phase 2.5 (compile-time gate must exist).

---

### T-14  CI route-enumeration drift check

**Complexity**: S
**Files affected**:
- `scripts/ci/check-ws-route-enumeration.sh` — create script that parses `src/main.rs` route registrations and asserts every `.route("/ws...")` and `.route("/wss")` appears in the T4-resolved canonical table in `ADR-06 §D11`; fails on any drift

**Acceptance criteria**: T4 resolution verification requirement. Script exits 0 on a clean tree, exits non-zero if a new route is added without updating the table.

**Dependencies**: T-13 (the `/wss` route registration is the primary one Phase 3 touches).

---

### T-15  Node-flag round-trip unit tests (`TC-1` resolution)

**Complexity**: S
**Files affected**:
- `src/utils/binary_protocol.rs` — rename `ONTOLOGY_INDIVIDUAL_FLAG` → `LINKED_PAGE_FLAG`; add `AXIOM_FLAG = 0x0C000000`; update `NodeType` enum and `get_node_type()` function (lines 183-197); add mask-coverage assertion test and round-trip tests (lines 1038-1051)

**Acceptance criteria**: TC-1 resolution. Tests assert: every assigned ontology subtype lies inside `ONTOLOGY_TYPE_MASK`; round-trip `set_axiom_flag(set_sequence(1234))` recovers sequence 1234 and `is_axiom() == true`.

**Dependencies**: T-07 (encoder uses same constants).

---

## 2. State Machine — `BroadcastActor`

```
                    OnLayoutStarted
  +---------+   ─────────────────────►   +--------+
  | SETTLED |                            | ACTIVE |
  +---------+   ◄─────────────────────   +--------+
    │   ▲          OnLayoutSettled          │   ▲
    │   │                                   │   │
    │   │  heartbeat_interval tick          │   │  BroadcastTick (≤100ms poll)
    │   └───────── TriggerHeartbeat ────────┘   │
    │                                           │
    │ Shutdown msg                  Shutdown msg│
    ▼                                           ▼
  +----------+                       +----------+
  | SHUTDOWN |                       | SHUTDOWN |
  +----------+                       +----------+
```

### States

| State    | Entry action                                                         | Ongoing behaviour                                               | Exit trigger                                |
|----------|----------------------------------------------------------------------|-----------------------------------------------------------------|---------------------------------------------|
| ACTIVE   | Reset `frame_id = 0` for all clients; emit immediate snapshot        | Emit full V3 frame to all clients at up to 10 Hz (100ms poll via `ctx.run_interval`) | `OnLayoutSettled` → SETTLED; `Shutdown` → SHUTDOWN |
| SETTLED  | Cancel ACTIVE interval; start `tokio::time::interval(heartbeat_secs)` | Each `TriggerHeartbeat` tick: read `current_snapshot()`, encode V3, send to all registered clients with backpressure check | `OnLayoutDestabilised` or `OnLayoutStarted` → ACTIVE; `Shutdown` → SHUTDOWN |
| SHUTDOWN | Cancel heartbeat interval; cancel ACTIVE interval                    | Drop all messages; log shutdown                                  | Terminal — no exit                          |

### Events and transitions

| Event               | From state | To state | Side effect                                                                 |
|---------------------|------------|----------|-----------------------------------------------------------------------------|
| `OnLayoutStarted`   | any        | ACTIVE   | Reset `frame_ids` map to 0 for all clients; emit immediate position snapshot |
| `OnLayoutSettled`   | ACTIVE     | SETTLED  | Cancel 100ms poll; start wall-clock interval (`broadcast_heartbeat_secs`)  |
| `OnLayoutDestabilised` | SETTLED | ACTIVE   | Cancel heartbeat interval; start 100ms poll                                 |
| `PhysicsClamped`    | any        | (same)   | Log `warn!`; no state change; no protocol change                           |
| `TriggerHeartbeat`  | SETTLED    | SETTLED  | Read `GraphStateActor::current_snapshot()`, encode, broadcast with per-client drop check |
| `BroadcastTick`     | ACTIVE     | ACTIVE   | Same as TriggerHeartbeat but at ≤10 Hz                                     |
| `RegisterBroadcastClient` | any | (same)   | Add client to registry; immediately send one snapshot regardless of state   |
| `UnregisterBroadcastClient` | any | (same) | Remove from registry; remove `frame_ids` entry                             |
| `Shutdown`          | any        | SHUTDOWN | Cancel timers; drain in-flight send futures; log                            |

### Heartbeat lifecycle

- Started: when state transitions ACTIVE → SETTLED. The actor calls
  `ctx.run_interval(Duration::from_secs(broadcast_heartbeat_secs), |act, _ctx| { act.trigger_heartbeat(); })` and stores the returned `SpawnHandle`.
- Cancelled: when state transitions SETTLED → ACTIVE (via `OnLayoutDestabilised` or `OnLayoutStarted`), the actor calls `ctx.cancel_future(heartbeat_handle)`.
- Also cancelled: on `Shutdown`, before the actor stops.
- The heartbeat timer is owned exclusively by `BroadcastActor`. `ForceComputeActor` and `PhysicsOrchestratorActor` emit no time-based events.

---

## 3. Removal List

The following must be deleted or fully stripped. Nothing is deprecated-in-place.

| Item | Location | Reason |
|------|----------|--------|
| `DeltaNodeData` struct | `src/utils/binary_protocol.rs:62-73` | V4 delta encoding rejected (ADR-02 D1) |
| `DELTA_*` constants | `src/utils/binary_protocol.rs:76-78` | V4 encoding |
| `PROTOCOL_V4` const | `src/utils/binary_protocol.rs:13` | V4 encoding |
| `WireNodeDataItemV3` analytics extension fields (`sssp_distance`, `sssp_parent`, `cluster_id`, `anomaly_score`, `community_id`) | `src/utils/binary_protocol.rs:40-49` | PRD-02 §6 defines the 28-byte V3 frame (`u32 node_id, f32*6`); the 48-byte analytics extension is not part of Phase 3's V3 |
| `delta_encoding.rs` module | `src/utils/delta_encoding.rs` | V4 artefact |
| `BroadcastOptimizer` struct | `src/gpu/broadcast_optimizer.rs` (entire file) | ADR-02 D5 |
| `pub mod broadcast_optimizer` | `src/gpu/mod.rs:65-67` | D5 |
| `broadcast_optimizer` field | `ForceComputeActor` struct (line 107) | D5 |
| `BinaryFrameCoalescer` | wherever located (search `grep -r Coalescer src/`) | ADR-02 D6 |
| `broadcast_interval: Duration` | `ClientCoordinatorActor` (line 455) | Vestigial under D2 (PRD-02 §7) |
| `active_broadcast_interval: Duration` | `ClientCoordinatorActor` (line 458) | Vestigial |
| `stable_broadcast_interval: Duration` | `ClientCoordinatorActor` (line 461) | Vestigial |
| `update_broadcast_interval()` method | `ClientCoordinatorActor` (lines 748-768) | Vestigial |
| `LayoutHeartbeat` event variant | Physics event enum | T3 resolution: physics emits no heartbeat |
| Iteration-count broadcast branches (`iters_since_full >= 300`) | `force_compute_actor.rs:1448-1480, 1528-1562` | T3 resolution |
| 60 Hz broadcast throttle | `physics_orchestrator_actor.rs:538-590, 1100-1178` | T3 resolution |

---

## 4. Single-Source-of-Truth Wiring

All position data flows through a single read path: `GraphStateActor::current_snapshot()`.

```
ForceComputeActor (GPU)
        │
        │  UpdateNodePositions {positions: Vec<(u32,f32,f32,f32,f32,f32,f32)>}
        │  (pushed after each physics tick that produces meaningful change)
        ▼
GraphStateActor
  ├── current_snapshot() → PositionFrameSnapshot
  │         │
  │         │ (called by BroadcastActor on every ACTIVE tick and SETTLED heartbeat)
  │         ▼
  │   BroadcastActor
  │         │
  │         │  BinaryV3Frame::encode(snapshot) → Vec<u8>
  │         │  ctx.binary(frame_bytes) per registered client (with backpressure check)
  │         ▼
  │   SocketFlowServer (WebSocket /wss)
  │
  └── (also called by REST handler)
        │
        │  GetPositionFrameSnapshot message
        ▼
  GET /api/graph/positions handler
        │
        │  returns JSON array OR binary, same snapshot data
        ▼
  HTTP response
```

### Call sites for `GraphStateActor::current_snapshot()`

| Call site | File | When called |
|-----------|------|-------------|
| `BroadcastActor::broadcast_tick()` | `src/actors/broadcast_actor.rs` | Every ACTIVE tick (≤10 Hz) |
| `BroadcastActor::trigger_heartbeat()` | `src/actors/broadcast_actor.rs` | Every SETTLED heartbeat (≤0.2 Hz) |
| `BroadcastActor::handle(RegisterBroadcastClient)` | `src/actors/broadcast_actor.rs` | Once per new client |
| `get_graph_positions()` handler | `src/handlers/api_handler/graph/mod.rs` | On `GET /api/graph/positions` |

There are no other callers of position data from the GPU. The `ForceComputeActor` no longer broadcasts directly; it only writes positions to `GraphStateActor`.

---

## 5. WebSocket Endpoint Registration

The baseline `src/main.rs` registers `/wss` as the primary graph-sync endpoint (line 694):

```rust
.route("/wss", web::get().to(socket_flow_handler))
```

This matches ADR-06 D11 (T4 resolution) and ADR-02 D8. No rename is needed. The following corrections are documentary only (already resolved in TENSIONS-RESOLVED T4):

| ADR reference | Stale path cited | Correct path | Action |
|---------------|-----------------|--------------|--------|
| ADR-06 D5 CSP rationale | `wss://<host>/api/ws/...` | `/wss` | Doc edit only (T4) |
| ADR-12 D9 | `wss://<host>/ws` (graph data) | `/wss` | Doc edit only (T4) |

Canonical endpoint table (T4 resolution, 7 baseline + 3 sprint additions, partial):

| Path | Owner section | Auth posture |
|------|---------------|--------------|
| `/wss` | Section 02 (this phase) | Nostr JWT or `--allow-skip-auth` in dev build |
| `/ws/speech` | Section 09 | Nostr JWT |
| `/ws/mcp-relay` | Section 09 | Nostr JWT |
| `/ws/client-messages` | Section 03 | Nostr JWT |
| `/ws/xr-presence` | Section 12 | Nostr-signed JWT at upgrade only |
| `/ws/agent-telemetry` | Section 07 | Nostr JWT |
| `/ws/enterprise-events` | Section 09 | Nostr JWT |

Full 11-row table lives in `ADR-06 §D11`; Phase 3 only touches `/wss`.

---

## 6. Frame Format Implementation Plan

### `BinaryV3Frame` Rust struct

```
// Conceptual layout — plan only, do not implement here.

struct BinaryV3Frame {
    // Header — 8 bytes
    magic: u32,      // 0xV3F0 (= 0x56334630)
    frame_id: u32,   // monotonic per connection, wraps at u32::MAX

    // Body — node_count × 28 bytes each
    nodes: Vec<NodePositionRecord>,

    // Trailer — 4 bytes
    node_count: u32, // must equal nodes.len()
}

struct NodePositionRecord {
    node_id: u32,  // with class flag bits (see src/utils/binary_protocol.rs constants)
    pos_x:  f32,
    pos_y:  f32,
    pos_z:  f32,
    vel_x:  f32,
    vel_y:  f32,
    vel_z:  f32,
}
```

Total per-node: 28 bytes. Frame for 5 000 nodes: 8 + (5000 × 28) + 4 = 140 012 bytes.

### Encoder algorithm

1. Allocate `Vec<u8>` with capacity `8 + node_count * 28 + 4`.
2. Write magic `0x56334630u32` as little-endian bytes (4 bytes).
3. Write `frame_id: u32` (caller-supplied, little-endian).
4. For each `NodePositionRecord`: write `node_id` then six `f32` values, all little-endian.
5. Write `node_count: u32` (little-endian) as trailer.
6. Return buffer. No heap reallocation if capacity was correctly computed.

### Decoder algorithm

1. Validate minimum length (≥ 12 bytes for empty frame).
2. Read magic bytes; reject if `!= 0x56334630`.
3. Read `frame_id`.
4. Read trailer `node_count` from `bytes[len-4..]`.
5. Validate body length: `len - 12 == node_count * 28`.
6. Parse each 28-byte block into `NodePositionRecord`.
7. Return `BinaryV3Frame { magic, frame_id, nodes, node_count }`.

### `frame_id` counter

`frame_id` is per-connection, not global. `BroadcastActor` stores `frame_ids: HashMap<ClientId, u32>`. On `OnLayoutStarted`, all entries reset to 0. On each successful send to a client, its entry increments (wrapping at `u32::MAX`). The `frame_id` passed to the encoder is read from this map before encoding and is per-client (each client's copy of the frame may carry a different `frame_id` if frames were dropped for that client). This means encoding happens once per client where frame_ids diverge — acceptable at ≤10 Hz.

---

## 7. Spawn Plan

Three agents work in sequence then overlap. Phase 3 begins after Phase 2.5 merges.

### Agent A — backend-dev (broadcast actor + framing)

**Tasks**: T-01, T-02, T-03, T-04, T-07, T-05, T-06, T-08, T-09, T-10, T-11, T-12, T-15
**Order**:
1. T-01, T-02, T-03 in parallel (pure deletions, no dependencies between them)
2. T-07 (encoder needed before broadcast actor)
3. T-05, T-15 in parallel (GraphStateActor snapshot method, flag constants)
4. T-06 (REST endpoint redirected — needs T-05)
5. T-08, T-09 coordinated in one PR (remove LayoutHeartbeat + add BroadcastActor atomically)
6. T-04 in the same PR as T-09 (remove broadcast_interval fields when BroadcastActor takes over)
7. T-10, T-11, T-12 after T-09

### Agent B — tester (state-machine tests + heartbeat-while-physics-paused)

**Starts after**: T-09 merges (broadcast actor exists)
**Tasks**:
- BDD-1 (T3 verification): start `BroadcastActor` in SETTLED state, pause physics (no `LayoutDestabilised` or `OnLayoutStarted` fired), assert `TriggerHeartbeat` fires and a V3 frame arrives at a mock client within 5.5 s
- BDD-2 (T3 verification): verify 5 s cadence at simulated 200 Hz physics (physics events suppressed; heartbeat fires on wall clock, not on GPU tick)
- BDD-3 (T3 verification): emit `OnLayoutDestabilised` while in SETTLED state; assert heartbeat interval is cancelled within 100 ms
- State-machine unit tests: `ACTIVE → SETTLED → ACTIVE` round-trip; `Shutdown` from both states; `RegisterBroadcastClient` in ACTIVE sends immediate frame; `RegisterBroadcastClient` in SETTLED sends immediate frame
- Drop-counter test (T-11 verification): saturate one mock client buffer, assert `frames_dropped_total` increments, next frame delivered
- Round-trip frame test (T-07 verification): 5 000-node encode/decode, size assertion, field equality
- `GetPositionFrameSnapshot` unit test (T-05 verification): seeded `GraphStateActor`, message round-trip

### Agent C — perf-validator (1.4 MB/s peak check)

**Starts after**: T-09, T-10 merge (broadcast path wired end-to-end)
**Tasks**:
- Construct 5 000-node `PositionFrameSnapshot` in-process; encode with `BinaryV3Frame::encode()`; measure encode time for a single frame (target: <1 ms)
- Drive `BroadcastActor` at 10 Hz with 5 000 nodes and 1 mock client; measure bytes/s over a 10-second window; assert peak ≥ 1.0 MB/s and ≤ 1.5 MB/s (expected: 1.4 MB/s)
- Assert SETTLED heartbeat at 5 s ± 200 ms (wall-clock accuracy under load)
- Assert p99 tick-to-wire latency ≤ 50 ms (A6): timestamp at `TriggerHeartbeat` entry and at `ctx.binary()` return; collect 100 samples

---

## Summary

**Plan file**: `/home/devuser/workspace/visionflow-worktrees/phase-3-binary-protocol/docs/migration-sprint/02-binary-protocol/WORKTREE-PLAN.md`

**Total tasks**: 15 (T-01 through T-15)

**Total complexity**:
- S × 6 (T-01, T-03, T-04, T-08, T-12, T-14, T-15) — but T-08 counted once, note T-15 is S
- M × 6 (T-02, T-05, T-06, T-07, T-10, T-11, T-13)
- L × 0
- XL × 1 (T-09)

Rough estimate: 6×0.5 + 6×1 + 1×4.5 = 3 + 6 + 4.5 = **~13.5 dev-days** for a single developer working sequentially; with the three-agent spawn plan above, the critical path compresses to approximately 6 dev-days (Agent A drives the blocking spine; B and C validate in parallel after T-09).

**Top 3 hardest tasks**:

1. **T-09 — Implement `BroadcastActor` with state machine** (XL). Introduces a new actor that must be correct from day one: event subscription to physics, two independent timer strategies (100ms poll vs wall-clock interval), per-client frame_id tracking, backpressure logic, and an atomically safe transition from the existing `ClientCoordinatorActor` broadcast path. Getting the heartbeat cancellation and restart sequence wrong produces the exact starvation bug the sprint exists to fix.

2. **T-08 + T-09 coordination — Remove `LayoutHeartbeat` and activate `BroadcastActor` atomically**. The removal of iteration-count broadcast triggers in `force_compute_actor.rs` and the 60 Hz throttle in `physics_orchestrator_actor.rs` must land in the same commit as the `BroadcastActor`'s SETTLED heartbeat becoming active. Any window where both are absent leaves clients with no positions after convergence — the original freeze.

3. **T-05 — Add `current_snapshot()` to `GraphStateActor`**. This is the single-source-of-truth bottleneck: `GraphStateActor` at the baseline reads from Neo4j on startup and then is written by `UpdateNodePositions`. The snapshot method must reflect the latest GPU-pushed positions with no read-write race under actix's single-threaded actor model, return data in the exact shape the encoder expects, and be the only read path for both the broadcast actor and the REST endpoint. Getting the data model wrong here propagates incorrectness to all downstream paths simultaneously.
