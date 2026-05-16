# WORKTREE-PLAN — Phase 5: GPU Physics & Force Engines

Branch  : `impl/phase-5-gpu-physics`
Base    : `radical-rollback` @ `d260a6158`
Depends : Phase 3 (broadcast event bus live), Phase 2 §D4 (`GraphStateActor::current_snapshot()` available)
Authors : worktree-planner

---

## 1. Phase 5 Task Breakdown

Tasks are labelled T1..T8 and map to PRD-01 acceptance criteria A1..A6 and
ADR-01 decisions D1..D9.

| Task | Label | PRD A# | ADR D# | Files touched | Est. hours |
|------|-------|--------|--------|---------------|------------|
| Consolidate GPU buffers into `PhysicsGpuBuffers` | T1 | A3, A4 | D1, D3 | `src/gpu/buffers.rs` (new), `src/utils/unified_gpu_compute/construction.rs`, `memory.rs`, `src/actors/gpu/force_compute_actor.rs` | 10 |
| Actor-owned GPU context + D2 discipline | T2 | A4 | D2 | `src/actors/gpu/force_compute_actor.rs`, `src/actors/gpu/physics_supervisor.rs`, `src/actors/gpu/shared.rs` | 6 |
| Supervisor restart pattern, remove `catch_unwind` | T3 | A4 | D4 | `src/actors/supervisor.rs`, `src/actors/gpu/physics_supervisor.rs` | 4 |
| `LayoutEngine` trait + 5 engine impls | T4 | A2, A6 | D5 | `src/physics/engines/mod.rs` (new), `src/physics/engines/{force_directed,stress_majorization,hierarchical,circular,geographic}.rs` (new), `src/actors/gpu/force_compute_actor.rs`, `src/layout/types.rs` | 14 |
| Log-mass derivation + gravity policy | T5 | A1 | D6, D7 | `src/actors/gpu/force_compute_actor.rs` (upload path), `src/utils/unified_gpu_compute/memory.rs`, `src/utils/visionflow_unified.cu` | 5 |
| `numerical_safety` kernel (NaN/Inf sentinel) | T6 | A3 | D8 | `src/utils/visionflow_unified.cu`, `src/utils/unified_gpu_compute/execution.rs`, `src/gpu/buffers.rs` | 6 |
| Event emission only — remove physics heartbeat | T7 | A5 | D9 | `src/actors/gpu/force_compute_actor.rs`, `src/actors/physics_orchestrator_actor.rs`, `src/actors/messages/physics_messages.rs` | 4 |
| Tests: NaN injection, panic recovery, engine switch | T8 | A2, A3, A4, A6 | D4, D5, D8 | `tests/physics_gpu_integration.rs` (new), `tests/engine_switch.rs` (new) | 12 |

**Sequencing**: T1 → T2 → T3 (buffer struct must exist before actor discipline is
enforced, supervisor strategy after actor is stable). T4 depends on T1 (engines
share `PhysicsGpuBuffers`). T5, T6, T7 are independent once T1 is complete and
can run in parallel. T8 is written in parallel with T4–T7 and gates merge.

---

## 2. `PhysicsGpuBuffers` Consolidation Plan (ADR-01 D1)

### New struct: `src/gpu/buffers.rs`

```rust
pub struct PhysicsGpuBuffers {
    // --- positions (ping-pong pair) ---
    pub pos_in_x:  DeviceBuffer<f32>,
    pub pos_in_y:  DeviceBuffer<f32>,
    pub pos_in_z:  DeviceBuffer<f32>,
    pub pos_out_x: DeviceBuffer<f32>,
    pub pos_out_y: DeviceBuffer<f32>,
    pub pos_out_z: DeviceBuffer<f32>,
    // --- velocities ---
    pub vel_in_x:  DeviceBuffer<f32>,
    pub vel_in_y:  DeviceBuffer<f32>,
    pub vel_in_z:  DeviceBuffer<f32>,
    pub vel_out_x: DeviceBuffer<f32>,
    pub vel_out_y: DeviceBuffer<f32>,
    pub vel_out_z: DeviceBuffer<f32>,
    // --- forces ---
    pub force_x:   DeviceBuffer<f32>,
    pub force_y:   DeviceBuffer<f32>,
    pub force_z:   DeviceBuffer<f32>,
    // --- FA2 LinLog adaptive speed ---
    pub prev_force_x: DeviceBuffer<f32>,
    pub prev_force_y: DeviceBuffer<f32>,
    pub prev_force_z: DeviceBuffer<f32>,
    // --- mass model ---
    pub masses:      DeviceBuffer<f32>,   // log-mass per node
    pub class_ids:   DeviceBuffer<i32>,
    pub class_masses: DeviceBuffer<f32>,
    // --- AABB reduction workspace ---
    pub aabb_block_results:     DeviceBuffer<AABB>,
    pub partial_kinetic_energy: DeviceBuffer<f32>,
    pub zero_buffer:            DeviceBuffer<u8>,
    // --- bookkeeping ---
    pub node_count:  usize,
    pub capacity:    usize,
}

impl PhysicsGpuBuffers {
    pub fn new(initial_capacity: usize) -> Result<Self>;
    pub fn resize(&mut self, new_node_count: usize) -> Result<()>;
}
```

`resize()` is the **only** code path permitted to modify capacity. It is
transactional: all `DeviceBuffer` replacements succeed or the method returns
`Err` with the previous capacity intact. The caller (the physics actor) treats
`Err` as a fatal GPU error that triggers a supervisor restart, not a retry.

Capacity growth policy (ADR-01 D3):

```
new_capacity = max(node_count, min(current_capacity * 2, 16384))
```

Values above 16384 are permitted but logged at `warn!` via
`PhysicsConfig::max_nodes_warning`.

### Callsites in the current baseline that must be migrated

The current implementation does not have a `PhysicsGpuBuffers` struct. Buffer
fields are distributed across `UnifiedGPUCompute`
(`src/utils/unified_gpu_compute/construction.rs`) and resized in
`memory.rs::resize_buffers()`. Every site that currently mutates an individual
buffer must be routed through `PhysicsGpuBuffers::resize()` after T1.

| Callsite | File | Lines | Migration action |
|----------|------|-------|-----------------|
| `resize_buffers()` — resizes pos/vel/force/class/prev_force independently | `src/utils/unified_gpu_compute/memory.rs` | 346–468 | Replace with `PhysicsGpuBuffers::resize()`; strip out per-buffer replacement logic |
| `aabb_block_results` resized separately from positions | `src/utils/unified_gpu_compute/memory.rs` | 445–446 | Moved into `PhysicsGpuBuffers::resize()` |
| `partial_kinetic_energy` resized separately | `src/utils/unified_gpu_compute/memory.rs` | 454 | Moved into `PhysicsGpuBuffers::resize()` |
| `prev_force_x/y/z` reset to zero on resize | `src/utils/unified_gpu_compute/memory.rs` | 439–441 | Moved into `PhysicsGpuBuffers::resize()` |
| `class_id/charge/mass` resized separately | `src/utils/unified_gpu_compute/memory.rs` | 430–432 | Moved into `PhysicsGpuBuffers::resize()` |
| Direct `compute.class_mass = new_mass` field assign | `src/actors/gpu/force_compute_actor.rs` | 702 | Replaced by mass upload via `PhysicsGpuBuffers::upload_masses()` |
| `DeviceBuffer::from_slice(&mass_weights)` standalone | `src/actors/gpu/force_compute_actor.rs` | 701 | Removed; mass derivation happens inside T5 upload path |
| `upload_class_metadata()` validates per-field sizes | `src/utils/unified_gpu_compute/memory.rs` | 44–80 | Unified into a single `PhysicsGpuBuffers::upload_class_metadata()` |
| `upload_positions()` pads independently | `src/utils/unified_gpu_compute/memory.rs` | 12–41 | Unified into `PhysicsGpuBuffers::upload_positions()` |
| Cell buffer resizes (`resize_cell_buffers`) | `src/utils/unified_gpu_compute/memory.rs` | 240–344 | Grid/cell buffers are spatial-hash concerns, not owned by `PhysicsGpuBuffers`; remain in `UnifiedGPUCompute` |

Clustering analytics buffers (`centroids_*`, `lof_scores`, etc.) are not part
of the physics simulation loop and are not moved into `PhysicsGpuBuffers`.

---

## 3. `LayoutEngine` Trait + 5 Engines (ADR-01 D5)

### Trait definition — `src/physics/engines/mod.rs`

```rust
pub trait LayoutEngine: Send + Sync {
    /// Execute one integration step.
    fn step(&self, buffers: &mut PhysicsGpuBuffers, params: &SimParams) -> Result<()>;

    /// True if this engine supports 3-D positions; false for 2-D planar engines.
    fn supports_3d(&self) -> bool;

    /// Return a scalar convergence metric (typically RMS velocity).
    fn convergence_metric(&self, buffers: &PhysicsGpuBuffers) -> f32;

    /// Engine name for logging and metrics.
    fn name(&self) -> &'static str;
}
```

All engines share the `PhysicsGpuBuffers` owned by `ForceComputeActor`. No
engine allocates its own device memory.

### Engine registry

| Engine struct | `LayoutMode` variant | 3D? | CUDA kernel | Notes |
|--------------|---------------------|-----|-------------|-------|
| `ForceDirectedEngine` | `ForceDirected` | yes | `force_pass_kernel`, `integrate_pass_kernel` (FA2 LinLog) | Default engine |
| `StressMajorizationEngine` | `StressMajorization` | yes | wraps existing `StressMajorizationSolver` in `src/physics/stress_majorization.rs` | |
| `HierarchicalEngine` | `Hierarchical` | no | CPU-side Sugiyama layer assignment → uploads positions once per step | Positions updated in CPU, GPU reads |
| `CircularEngine` | `Circular` | no | CPU-side radial placement | Fixed layout; `convergence_metric` returns 0.0 immediately |
| `GeographicEngine` | `Geographic` | no | CPU-side coordinate mapping | Fixed layout; `convergence_metric` returns 0.0 immediately |

The active engine is held as `engine: Box<dyn LayoutEngine>` inside
`ForceComputeActor`. The current `if/else` dispatch chain in `ForceComputeActor`
(present at baseline via `src/layout/engines.rs`) is replaced by
`self.engine.step(buffers, params)?`.

### `SetLayoutMode` message handling

```rust
// In src/actors/messages/physics_messages.rs
#[derive(Message, Debug, Clone)]
#[rtype(result = "Result<()>")]
pub struct SetLayoutMode {
    pub mode: LayoutMode,
}
```

Handler in `ForceComputeActor`:
1. Construct new engine from the registry (infallible; all engines are stateless
   at construction time).
2. Call `self.buffers.reset_velocities_to_zero()` to prevent ghost forces.
3. Replace `self.engine`.
4. Reset `ConvergenceState` so the hysteresis window starts fresh.
5. Emit `LayoutDestabilised` to broadcast layer so clients enter ACTIVE mode.

The handler does **not** touch positions. Nodes remain where they are; the new
force model takes over from there, satisfying PRD-01 A2 (no teardown, no ghost
positions).

---

## 4. Supervisor Restart Pattern (ADR-01 D4)

### Target policy

```
ForceComputeActor supervised under PhysicsSupervisor
  strategy : OneForOne
  max_restarts : 3
  within_secs : 60
  on_exceed : stop actor, surface SubsystemHealth::Failed alarm
```

### What changes

**Remove** the `catch_unwind` wrapper in `src/actors/supervisor.rs` lines 444–471.
The `RestartAttempt` handler calls a factory closure wrapped in
`std::panic::catch_unwind`. This must become a plain factory call; if the factory
panics the panic propagates to the actix runtime, which is the correct behaviour
for a top-level actor thread.

`PhysicsSupervisor` currently tracks restarts per actor in
`SupervisedActorState::failure_count`. It needs a time-window check:

```rust
fn should_allow_restart(&self, state: &SupervisedActorState) -> bool {
    let window = Duration::from_secs(60);
    let recent = state.last_restart
        .map(|t| t.elapsed() < window)
        .unwrap_or(false);
    if recent && state.failure_count >= 3 {
        return false;
    }
    true
}
```

When `should_allow_restart` returns false, `PhysicsSupervisor` sends
`GetSubsystemHealth` upward and stops issuing `RestartAttempt` messages.

### GPU context cold re-init on restart

When `ForceComputeActor` starts after a supervisor restart, `shared_context` is
`None` and `gpu_self_init_attempts` is reset to 0 (new actor instance). The
existing `initialize_own_gpu_context()` path already handles this correctly. No
new code is required beyond resetting the atom at construction.

The restart is slower than `catch_unwind` (~100–500ms for CUDA context init vs
~1ms for unwind). This is acceptable per ADR-01 D4's rationale: the expected
crash rate is near-zero in steady state; the restart is a safety valve, not a
hot path.

### Test harness integration

T8 introduces a deliberate CUDA panic via an invalid kernel launch (null device
pointer on a trivial kernel). The test verifies:
- Actor delivers `PhysicsClamped` events before the panic.
- Supervisor logs `error!` with backtrace.
- Actor is alive again within 2s.
- Supervisor does not surface `SubsystemHealth::Failed` on first crash.

---

## 5. Mass Model + Gravity Policy (ADR-01 D6, D7)

### Log-mass derivation

Mass is computed once at graph load inside `ForceComputeActor`'s graph upload
handler. Source: degree of each node derived from the CSR edge list.

```rust
fn derive_masses(degrees: &[u32]) -> Vec<f32> {
    degrees.iter()
        .map(|&d| 1.0_f32 + (1 + d as u32).ilog2() as f32)
        .collect()
}
```

This matches ADR-01 D6: `mass = 1.0 + log2(1 + degree)`.

Mass is uploaded into `PhysicsGpuBuffers::masses` (replacing the current
`class_mass` overloading in `force_compute_actor.rs` lines 694–702).

**Re-upload trigger**: only on `UpdateTopology` message (edge add/remove changes
degrees). Not per-frame, not per settings change.

### Gravity policy

The CUDA integrate kernel currently has a `center_gravity_k` term with per-node
degree-weight scaling (`degree_weight` buffer, populated in
`force_compute_actor.rs` lines 694–718). This degree-weighted path is removed.
The kernel receives a single `center_gravity_k` scalar applied uniformly,
scaled by the node's log-mass. High-mass hubs already resist repositioning
through inertia; additional gravity weighting is the source of the
unit-cancellation bug documented in ADR-01's context section.

Files to edit:
- `src/utils/visionflow_unified.cu` — remove `degree_weight` buffer parameter
  from the integration kernel signature; replace with `mass * center_gravity_k`.
- `src/utils/unified_gpu_compute/construction.rs` — remove `degree_weight`
  `DeviceBuffer<f32>` field.
- `src/utils/unified_gpu_compute/memory.rs` — remove `degree_weight` resize
  and `degree_weights_available` flag.
- `src/actors/gpu/force_compute_actor.rs` lines 657–718 — remove the degree
  weight upload block entirely.

---

## 6. NaN/Inf Sentinel Kernel (ADR-01 D8)

### Kernel specification

A new `numerical_safety` kernel is appended to `src/utils/visionflow_unified.cu`
and dispatched as the **last** step of every physics tick, after force
integration and before `copy_to` download.

```c
__global__ void numerical_safety_kernel(
    float* __restrict__ pos_x,
    float* __restrict__ pos_y,
    float* __restrict__ pos_z,
    float* __restrict__ vel_x,
    float* __restrict__ vel_y,
    float* __restrict__ vel_z,
    const float centroid_x,
    const float centroid_y,
    const float centroid_z,
    int*   __restrict__ clamp_count_nan,
    int*   __restrict__ clamp_count_inf,
    int*   __restrict__ clamp_count_vel,
    const float max_velocity,
    const int   num_nodes
);
```

Per thread:
1. If any position component is `!isfinite()`, replace with the corresponding
   centroid coordinate and atomically increment `clamp_count_nan` or
   `clamp_count_inf`.
2. Compute `|v|²`. If greater than `max_velocity² (= 10000.0)`, scale velocity
   to `max_velocity` and atomically increment `clamp_count_vel`.

Centroid is computed by the existing AABB kernel; the host reads
`aabb_block_results` to compute the mean once per tick and passes it as a
scalar argument. This avoids a second reduction pass.

### Rust-side integration

In `src/utils/unified_gpu_compute/execution.rs`, after the existing integrate
kernel dispatch, add:

```rust
let (cx, cy, cz) = self.compute_centroid_from_aabb()?;
unsafe {
    numerical_safety_kernel.launch(&config, (
        pos_x, pos_y, pos_z,
        vel_x, vel_y, vel_z,
        cx, cy, cz,
        clamp_nan_ptr, clamp_inf_ptr, clamp_vel_ptr,
        MAX_VELOCITY, self.num_nodes as i32,
    ))?;
}
```

The three clamp counters are aggregated after synchronisation and emitted as a
`PhysicsClamped { kind, count }` domain event if non-zero. At most one event per
kind per tick.

### Metrics surface

`/metrics/physics_clamp_count` (Prometheus counter) is incremented by the
`PhysicsClamped` event handler in `PhysicsOrchestratorActor`. This replaces the
ad-hoc per-kernel safety scattered across `visionflow_unified.cu` (line 356 and
similar).

---

## 7. Event Emission Only (ADR-01 D9 / TENSIONS-RESOLVED T3)

### Events physics emits — complete list

```rust
pub enum PhysicsEvent {
    LayoutStarted      { node_count: usize, engine_name: &'static str, params_hash: u64 },
    LayoutSettled      { iteration: u64, rms_velocity: f32 },
    LayoutDestabilised { iteration: u64, rms_velocity: f32 },
    PhysicsClamped     { kind: ClampKind, count: u32 },
}

pub enum ClampKind { NaN, Inf, VelocityCap }
```

These four events are the complete physics-side output. No others are permitted.

### What is removed from `ForceComputeActor`

- `last_full_broadcast_iteration` field and the 300-iteration periodic broadcast
  trigger (lines 1407–1480 of `force_compute_actor.rs`). Broadcast cadence is
  owned by the broadcast actor per ADR-02 D2 and T3 resolution.
- `broadcast_optimizer: BroadcastOptimizer` field and all `process_frame()` /
  `reset_delta_state()` calls. `BroadcastOptimizer` is eliminated per ADR-02 D5.
- `suppress_intermediate_broadcasts` and `force_full_broadcast` flags.
- `backpressure: NetworkBackpressure` field and token bucket logic. Backpressure
  is a broadcast concern per ADR-02 D3.
- `graph_service_addr` (physics no longer pushes to `GraphServiceSupervisor`
  directly for broadcast; `UpdateNodePositions` to `GraphStateActor` is retained
  as the snapshot write path per ADR-02 D4).

### What is retained

`ForceComputeActor` still sends `UpdateNodePositions` to `GraphStateActor` after
every tick (or every N ticks at the orchestrator's discretion). This is the
snapshot write path; it is not a broadcast. The broadcast actor reads from
`GraphStateActor::current_snapshot()` on its own timer.

Settlement detection (RMS velocity hysteresis over last N ticks) remains in
`ForceComputeActor` as the source of `LayoutSettled` / `LayoutDestabilised`.
The hysteresis window size is `PhysicsConfig::settlement_window` (default 10
ticks). Crossing the threshold downward emits `LayoutSettled`; crossing it
upward emits `LayoutDestabilised`.

---

## 8. Spawn Plan

Two specialised agents run in parallel once T1 is merged to the branch.

### Agent A — `backend-dev` (Rust + CUDA)

Responsibilities:
- T1: Implement `PhysicsGpuBuffers` struct in `src/gpu/buffers.rs`. Migrate all
  callsites listed in §2.
- T2: Enforce actor-owned GPU context; remove `shared_context` injection path.
- T3: Remove `catch_unwind` from supervisor; implement time-window restart guard.
- T4: Define `LayoutEngine` trait; implement 5 engine structs; wire `SetLayoutMode`.
- T5: Log-mass derivation; remove degree-weighted gravity from CUDA kernel and
  Rust upload path.
- T6: Write `numerical_safety_kernel` in CUDA; wire Rust dispatch; emit
  `PhysicsClamped` events.
- T7: Strip broadcast plumbing from `ForceComputeActor`; define `PhysicsEvent`
  enum; wire `LayoutStarted/Settled/Destabilised/PhysicsClamped` emission.

Worktree isolation: works directly on `impl/phase-5-gpu-physics`. One commit per
task, rebased onto the branch tip before PR.

### Agent B — `tester`

Responsibilities:
- T8a: NaN injection test — construct a graph with two coincident nodes, verify
  `PhysicsClamped { kind: NaN, count: > 0 }` is emitted and downstream
  positions are finite.
- T8b: Panic recovery test — trigger invalid kernel launch, verify actor survives
  and emits `LayoutStarted` after restart.
- T8c: Engine switch test — switch from `ForceDirected` to `StressMajorization`
  mid-run, verify no ghost positions and `LayoutDestabilised` + `LayoutStarted`
  pair emitted.
- T8d: Heartbeat BDD tests from T3 resolution: BDD-1 (paused physics emits
  broadcast heartbeat within 5.5s via broadcast actor), BDD-2 (5-s cadence at
  200-Hz physics tick), BDD-3 (interval cancelled within 100ms of
  destabilisation).
- T8e: Buffer resize atomicity test — inject an OOM error mid-resize via
  mock; verify `node_count` and `capacity` are unchanged.

Agent B does not touch implementation files. All tests go to `tests/` and use
the headless CUDA test harness present in `tests/physics_gpu_integration.rs`.
Agent B opens a draft PR against the branch; Agent A reviews before merge.

---

## 9. Output Summary

**Plan file**: `/home/devuser/workspace/visionflow-worktrees/phase-5-gpu-physics/docs/migration-sprint/01-gpu-physics/WORKTREE-PLAN.md`

**Tasks**: 8 (T1–T8), spanning ~61 estimated hours. T1 is gating; T4–T7 are
parallel after T1; T8 runs in parallel with T4–T7.

**Complexity**: High. The buffer consolidation (T1) touches the deepest GPU
abstraction layer and every callsite above it. Kernel edits (T5, T6) carry
non-zero risk of silent mis-launch. Engine switching (T4) introduces a trait
vtable dispatch on the hot path.

**Top 3 risks**

R1. **Buffer consolidation breaks compilation across many callsites** (T1).
    `UnifiedGPUCompute` has 30+ `pub` buffer fields referenced across
    `execution.rs`, `memory.rs`, `force_compute_actor.rs`, and analytics actors.
    Mitigation: T1 is a pure refactor — no semantic change — and is gated behind
    a feature flag (`physics-v2`) until it compiles cleanly. Analytics buffer
    fields (`centroids_*`, `lof_scores`, etc.) are explicitly out of scope for
    `PhysicsGpuBuffers` and remain on `UnifiedGPUCompute`; the migration touches
    only the 10 physics-simulation buffers enumerated in §2.

R2. **Supervisor restart time exceeds 500ms under load** (T3, ADR-01 D4 R2).
    A cold CUDA context init on an actively streaming GPU may block. Mitigation:
    measure in T8b. If restart latency exceeds 500ms on the test harness, add a
    warm context pool of size 1 (a single pre-initialised `CudaDevice` held by
    `PhysicsSupervisor`, donated to the new actor on restart rather than built
    from scratch).

R3. **Gravity kernel change causes layout regression on the knowledge graph** (T5).
    Removing degree-weighted gravity changes the equilibrium positions for dense
    subgraphs. The 5k-node convergence acceptance criterion (PRD-01 A1, RMS < 0.01
    within 600 frames) may not hold with the log-mass model on all graph shapes.
    Mitigation: expose `mass_function = { log | linear | sqrt }` in
    `PhysicsConfig` as ADR-01 R3 recommends; run the benchmark fixture on all
    three before merging T5; document the winning default.
