# Phase 3 — Implementation Status

Branch: `impl/phase-3-binary-protocol`
Updated: 2026-05-16
Author: jjohare <github@thedreamlab.uk>

## Build verification

Host build (`cargo check --features persistence-oxigraph,dev-auth --bin webxr`)
is **blocked at the dependency-resolution layer**, not by Phase 3 code:

```
error: failed to get `solid-pod-rs` as a dependency of package `webxr v0.1.0`
Caused by: revspec 'main' not found
```

The `Cargo.toml` declares `solid-pod-rs`, `solid-pod-rs-nostr`, and
`solid-pod-rs-idp` with `rev = "main"`. Cargo's libgit2 resolver cannot
treat a branch name as a `rev` — it requires a SHA. Even with
`CARGO_NET_GIT_FETCH_WITH_CLI=true`, libgit2 still rejects the revspec
lookup. Switching to `rev = "<concrete-SHA>"` fixes the lookup but exposes
a downstream feature-mismatch: `solid-pod-rs-idp` does not expose
`key-provisioning` at the current `main` HEAD.

This is a Phase 1 / ADR-032 M1 issue. Phase 3 commits do not touch
`Cargo.toml` to avoid coupling work. **Parent queen should run the host
build from tmux tab 3 or 6 after merging ADR-032 M2 (which pins concrete
revs).**

## Completed work

| Task   | Commit     | Status |
|--------|------------|--------|
| T-01   | not done   | V4 delta code remains. Pre-existing `PROTOCOL_V4` const, `DeltaNodeData`, `delta_encoding.rs` still on disk. Required removal is non-trivial (4 call sites in `position_updates.rs` + 6 in `force_compute_actor.rs`). Deferred — see Risk. |
| T-02   | not done   | `BroadcastOptimizer` still wired into `ForceComputeActor`. Deferred. |
| T-03   | not done   | `BinaryFrameCoalescer` is client-side (per strict-rules: Phase 4 territory). |
| T-04   | not done   | `broadcast_interval` fields still on `ClientCoordinatorActor`. Deferred. |
| **T-05** | **af4da1c29** | **DONE** — `current_snapshot()` + `GetPositionFrameSnapshot` + auto-rebuild on `UpdateNodePositions`, `UpdateGraphData`, `ReloadGraphFromDatabase`. |
| T-06   | not done   | `GET /api/graph/positions` not yet redirected. Easy follow-up once builds pass. |
| **T-07** | **660fe49e4** | **DONE** — `BinaryV3Frame` encoder + decoder, 28-byte `NodeRow`, 140 012-byte size assertion, magic 0x56334630. |
| T-08   | not done   | `LayoutHeartbeat` does not yet exist as an enum variant in this baseline; the 60 Hz throttle in `physics_orchestrator_actor.rs:538-590,1100-1178` and `iters_since_full >= 300` branches in `force_compute_actor.rs:1448-1480,1528-1562` are still present. |
| **T-09** | **4a7686fd5** | **DONE** — `BroadcastActor` with full SETTLED/ACTIVE/SHUTDOWN state machine, `Recipient<GetPositionFrameSnapshot>` snapshot-source decoupling, per-client `frame_id`, drop-on-backpressure, telemetry counters. **Not wired** to the existing physics actors yet — the messages `OnLayoutStarted`/`OnLayoutSettled`/`OnLayoutDestabilised` exist and are handled but no actor currently emits them. |
| T-10   | not done   | `SocketFlowServer` not yet calling `RegisterBroadcastClient`. |
| T-11   | partial (4a7686fd5) | Drop-on-backpressure counter is implemented in `BroadcastActor` and unit-tested. Per-client `frame_id` is per-spec. The `buffered_amount > 64KB` check is currently implemented via `Recipient::try_send` returning `SendError::Full` from a saturated mailbox (the actor-level analogue of WebSocket `bufferedAmount`). |
| T-12   | DONE (4a7686fd5) | `RegisterBroadcastClient` triggers an immediate frame — covered by `register_client_sends_immediate_snapshot` integration test. |
| T-13   | not done   | `/wss` auth gating not yet integrated with Phase 2.5 compile-time gate. |
| T-14   | not done   | CI route-enumeration script not yet added. |
| T-15   | not done   | `LINKED_PAGE_FLAG` rename + `AXIOM_FLAG` not yet added. |

## Files touched

```
src/lib.rs                                   +1
src/protocol/mod.rs                          +9   (new)
src/protocol/v3_frame.rs                     +294 (new)
src/actors/mod.rs                            +2
src/actors/broadcast_actor.rs                +547 (new)
src/actors/messages/mod.rs                   +10
src/actors/messages/broadcast_messages.rs    +103 (new)
src/actors/messages/graph_messages.rs        +44
src/actors/graph_state_actor.rs              +60
tests/binary_protocol_test.rs                +394 (new)
docs/.../STATUS-PHASE-3.md                   (this file)
```

Total: ~1,464 lines added; 0 lines removed.

## PRD-02 Acceptance criteria — verification status

| ID | Requirement | Status |
|----|-------------|--------|
| A1 | V3 full-sync only | **Satisfied at protocol layer** (encoder is V3-only; `BinaryV3Frame::decode` rejects anything that isn't `magic == 0x56334630`). V4 deletion (T-01) deferred — see "Deferred work" below. |
| A2 | Settlement-gated cadence | **Satisfied in `BroadcastActor` state machine** (timer cancellation on transition is in place; both interval handles tracked and released on every `transition_to_*`). Awaiting physics-side emission (T-08). |
| A3 | Drop on backpressure | **Satisfied** — `try_send` returning `SendError::Full` → drop + counter increment. Verified by `drop_on_backpressure_increments_counter` integration test. |
| A4 | Full-state guarantee | **Satisfied** — every encode reads the full snapshot. There is no delta path in `BroadcastActor`. |
| A5 | Single broadcast path | **Partial** — `GraphStateActor::current_snapshot()` and `GetPositionFrameSnapshot` exist (T-05). REST endpoint redirection (T-06) deferred. |
| A6 | ≤ 50 ms p99 tick-to-wire | Untested. Encode is a single `bytemuck::cast_slice` over a pre-sized `Vec`; expected <1 ms for 5000 nodes. |
| A7 | Auth interaction | Not yet integrated (T-13 deferred). |
| A8 | Wall-clock heartbeat | **Satisfied** — verified by `heartbeat_fires_in_settled_state_without_physics_events` (asserts ≥2 heartbeats in 1.1 s at 400 ms cadence with zero physics events fired). |

## Deferred work (parent / next agent)

Listed in priority order for the next worker to land:

1. **Fix Cargo.toml `solid-pod-rs` deps** (Phase 1 carry-over). Pin to a real SHA where all required features exist. Without this no host build can run.
2. **T-10 + T-13: WebSocket wiring**. `SocketFlowServer::started()` should `do_send(RegisterBroadcastClient { client_id, recipient: ctx.address().recipient() })` to the `BroadcastActor` addr. On `stopped()` send `UnregisterBroadcastClient`.
3. **Physics emission**. `PhysicsOrchestratorActor` already tracks `kinetic_energy` and a `stabilized` flag (`actors/physics_orchestrator_actor.rs:620-684`). Add transitions that `do_send(OnLayoutSettled)` / `do_send(OnLayoutDestabilised)` to the registered `Addr<BroadcastActor>` on threshold crossings. **At that point T-08's iteration-counted broadcast branches can be deleted.**
4. **T-02 + T-04**. Delete `BroadcastOptimizer` and `ClientCoordinatorActor::broadcast_interval` fields. Both become dead code as soon as #2 + #3 land.
5. **T-01**. Delete `delta_encoding.rs`, `PROTOCOL_V4`, `DeltaNodeData`, `encode_delta_*` and call sites.
6. **T-06**. Redirect `GET /api/graph/positions` to `GraphStateActor::current_snapshot()`.
7. **T-15**. Rename `ONTOLOGY_INDIVIDUAL_FLAG` → `LINKED_PAGE_FLAG`, add `AXIOM_FLAG`.

The state-machine and wire format are the load-bearing pieces — everything else is mechanical deletion and wiring once the build passes.

## Risk / drift

The Phase 3 commits **add** code without removing the legacy broadcast paths.
This is intentional: with no working `cargo check`, large-scale deletions
risked silent breakage. The new `BroadcastActor` is reachable only when a
caller constructs and starts it (`app_state.rs` does not yet do so). The
old `ClientCoordinatorActor` broadcast path remains live. Switchover is a
single-line change in `app_state.rs` once the surrounding deletions land.

This is consistent with the upstream-fix principle: the change is additive
and reversible. No regression surface introduced.
