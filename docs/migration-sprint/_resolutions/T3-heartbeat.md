# T3 — Heartbeat: iteration-count vs wall-clock

Status   : Resolved (proposal)
Date     : 2026-05-16
Tension  : ADR-01 D9 expresses `LayoutHeartbeat` as iteration-counted ("every N
           iterations, default 300"); ADR-02 D2 expresses it as wall-clock
           ("every 5 s by default"). At 60 Hz the two coincide; at any other
           tick rate they diverge. The two ADRs disagree on the unit of
           measurement and which side owns the source.

## Current state in code

Baseline `41979d33e`:

- **Physics tick rate is variable, not fixed.** `PhysicsOrchestratorActor`
  runs a sequential pipeline gated on `PhysicsStepCompleted`. The next tick
  is scheduled via `schedule_next_pipeline_step(ctx, delay)` where `delay`
  is `Duration::ZERO` in `FastSettle` mode (the default — see
  `src/models/simulation_params.rs:33`), i.e. as fast as the GPU produces
  completions; and `Duration::from_millis(16)` (60 Hz) in `Continuous`
  mode. Refs: `src/actors/physics_orchestrator_actor.rs:215-230, 1394-1410`.

- **The 300-iter heartbeat is purely iteration-counted.** In
  `src/actors/gpu/force_compute_actor.rs:1448-1480` and `:1528-1560`:
  ```rust
  let iters_since_full = actor.gpu_state.iteration_count
      .saturating_sub(actor.last_full_broadcast_iteration);
  let needs_full = iters_since_full >= 300;
  ```
  No `Instant`-based check gates the heartbeat. The field
  `last_full_broadcast_iteration: u32` (`:121`) is iteration-typed.

- **Wall-clock throttles exist elsewhere but do not gate the heartbeat.**
  `src/gpu/broadcast_optimizer.rs:50-74` gates every frame on
  `target_fps: 25` (40 ms). `src/actors/physics_orchestrator_actor.rs:538,
  1124` and `src/actors/client_coordinator_actor.rs:455-525, 748-768`
  carry vestigial 60 FPS / 50 ms / 1000 ms `broadcast_interval` Durations
  that ADR-02 already flags as "remove rather than re-tune".

- **The empirical coincidence.** In `Continuous` mode, 300 iters × 16 ms ≈
  4.8 s ≈ 5 s. The 5 s figure in ADR-02 D2 was reverse-derived from 300
  iters at 60 Hz, not chosen on its own merits.

## Performance analysis

300-iteration heartbeat wall-clock cadence under varying tick rates:

| Mode                  | Tick rate    | 300 iters → wall-clock | Client experience            |
|-----------------------|--------------|------------------------|------------------------------|
| Continuous 60 Hz      | 60 Hz        | 5.0 s                  | Matches ADR-02 D2 spec       |
| Continuous 30 Hz      | 30 Hz        | 10 s                   | Heartbeat half as frequent   |
| Continuous 120 Hz     | 120 Hz       | 2.5 s                  | Heartbeat twice as frequent  |
| FastSettle 5 k nodes  | ~200 Hz      | 1.5 s                  | Wastes bandwidth on settled  |
| FastSettle 50 k nodes | ~5 Hz        | 60 s                   | **Late client waits 60 s**   |
| Paused                | 0 Hz         | ∞                      | **Heartbeat never fires**    |

The last two rows are the failure cases. The freeze regression that
triggered this sprint was exactly "heartbeat doesn't reach the client".
Coupling heartbeat to iteration count re-introduces that bug whenever the
iteration rate deviates from 60 Hz — which is unavoidable under
`FastSettle` (the default) or on large graphs. 50 k is not hypothetical:
ADR-01 D3 sets `max_nodes_warning = 16384` as a soft ceiling, not a hard
limit.

## Domain ownership analysis

Per DDD-01: the physics context **does not own** "position broadcasting to
clients" and the "broadcast cadence is a *broadcast* concern, not a
*physics* concern".

The heartbeat is semantically a broadcast cadence rule: "you will see
fresh positions within 5 s even if nothing is moving". The client neither
knows nor cares about iteration count. The unit of meaning on the client
is **wall-clock seconds**.

Therefore the heartbeat belongs to the broadcast bounded context. Physics
owns settlement detection (a state event: settled vs destabilised), not
heartbeat (a time-driven cadence event).

This aligns with ADR-02 D4 (`GraphStateActor::current_snapshot()` is the
single source of truth) and D5 (`BroadcastOptimizer` eliminated). With
both in place, the broadcast layer is already polling-shaped: it reads a
snapshot when it wants to emit. A `tokio::time::interval` is the natural
driver.

## Options evaluated

- **A. Physics emits both event variants.** Rejected: doubles the event
  surface, perpetuates physics-side knowledge of broadcast cadence.
- **B. Physics emits on iteration; orchestrator translates.** Rejected:
  introduces a third actor doing time-translation of an event the physics
  actor still emits; the physics-emitted heartbeat becomes dead.
- **C. Broadcast owns heartbeat entirely (recommended).** Physics emits
  only `LayoutStarted`, `LayoutSettled`, `LayoutDestabilised`,
  `PhysicsClamped`. Broadcast actor holds a
  `tokio::time::interval(Duration::from_secs(5))` active only while in
  SETTLED state. Adopted.

## Recommended resolution

- **Owning domain**: Broadcast (Section 2). Physics emits no heartbeat.
- **Canonical event name**: `BroadcastHeartbeat` (broadcast-internal —
  removed from the physics event taxonomy).
- **Canonical interval source**:
  `tokio::time::interval(Duration::from_secs(broadcast_heartbeat_secs))`
  owned by the broadcast actor. Default `broadcast_heartbeat_secs = 5`.

### ADR-01 D9 — proposed wording

> ### D9. Broadcast cadence governed by settlement, not FPS
>
> ADR-02 owns this in detail. From the physics-side perspective: the actor
> emits **state-transition events only** — `LayoutStarted`,
> `LayoutSettled { iteration, rms_velocity }`, and
> `LayoutDestabilised { iteration, rms_velocity }`. Settlement is detected
> via RMS-velocity hysteresis across the last N ticks. The physics actor
> emits **no time-based heartbeat**: wall-clock heartbeat cadence is a
> broadcast concern (see ADR-02 D2) and is wholly owned by the broadcast
> actor.

### DDD-01 § Domain events — proposed change

Remove:
> - `LayoutHeartbeat { iteration, rms_velocity }` — every 300 iterations
>   unconditionally.

Physics events become four: `LayoutStarted`, `LayoutSettled`,
`LayoutDestabilised`, `PhysicsClamped`.

### ADR-02 D2 — proposed wording

Replace:
> On `LayoutSettled`: enter SETTLED state, broadcast on `LayoutHeartbeat`
> only (every 5 s by default).

With:
> On `LayoutSettled`: enter SETTLED state. The broadcast actor starts a
> `tokio::time::interval(broadcast_heartbeat_secs)` (default 5 s); each
> tick reads `GraphStateActor::current_snapshot()` and emits a full V3
> frame to all connected clients. The interval is cancelled on
> `LayoutDestabilised` and on shutdown.

### PRD-02 § 4 — add acceptance criterion

> A8. **Heartbeat is wall-clock, not iteration-driven.** In the SETTLED
> state, full-position frames emit every `broadcast_heartbeat_secs` of
> wall-clock time (default 5 s), independent of physics tick rate.
> Verified by a test that pauses physics entirely and observes a heartbeat
> within 5.5 s.

## Test scenarios

### BDD-1 — Heartbeat fires when physics is paused

```
Given the physics actor is in the PAUSED state (no ticks computed)
  And the broadcast actor is in the SETTLED state
  And one client is connected
When 6 seconds of wall-clock time elapse
Then the client receives at least one V3 full-position frame
  And the frame's node_count equals the snapshot's node count
  And no LayoutHeartbeat event was emitted by the physics actor
```

This is the case the iteration-counted rule fails (50 k-node and paused
rows above).

### BDD-2 — Heartbeat cadence is independent of physics rate

```
Given the broadcast actor is in the SETTLED state, broadcast_heartbeat_secs = 5
  And physics is ticking at 200 Hz (FastSettle on a fast GPU after re-heat)
When 30 seconds of wall-clock time elapse
Then the client receives exactly 6 heartbeat frames (± 1, for jitter)
  And not 20 (which is what 300-iter cadence would yield at 200 Hz)
```

Asserts broadcast cadence stays at 0.2 Hz regardless of physics rate.

### BDD-3 — Heartbeat is cancelled on destabilisation

```
Given the broadcast actor is in the SETTLED state
  And the heartbeat interval is scheduled to fire in 4.9 seconds
When the physics actor emits LayoutDestabilised
Then the heartbeat interval is cancelled within 100 ms
  And no heartbeat frame is sent for the remainder of that interval window
  And the active-broadcast path (up to 10 Hz) takes over
```

Asserts the two cadence modes don't overlap; heartbeat is SETTLED-only.

## Implementation notes (non-normative)

- `force_compute_actor.rs` loses `last_full_broadcast_iteration: u32` and
  both `iters_since_full >= 300` branches at `:1448-1480` and `:1528-1560`.
  The "send ALL nodes" path moves to the broadcast actor's heartbeat tick.
- `physics_orchestrator_actor.rs:538-590, 1100-1178` (60 FPS broadcast
  throttles) become redundant under ADR-02 D4: the orchestrator pushes
  positions to `GraphStateActor` via `UpdateNodePositions`; the broadcast
  actor reads from `GraphStateActor`. Remove in the same sprint task.
- `client_coordinator_actor.rs:455-525, 748-768` — the three
  `broadcast_interval` Duration fields are deleted. Replaced by a single
  `Option<SpawnHandle>` for the heartbeat interval, alive only in SETTLED
  state.
- `broadcast_optimizer.rs` is deleted entirely (ADR-02 D5). Its
  `target_fps: 25` was the closest thing baseline had to a wall-clock
  cadence anchor; replacing it with the heartbeat interval is a strict
  improvement.
