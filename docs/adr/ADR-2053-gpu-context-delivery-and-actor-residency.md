---
id: ADR-2053
title: Make direct context delivery authoritative and give every GPU analytics actor one residency
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: b00c28a0d766c8cf46cd00b100dab60ef2dd74a4
verified_paths: []
owner: jjohare
review_trigger: Any new GPU subsystem supervisor, or any second instance of a GPU actor being spawned outside its supervisor
repo: visionclaw
---

# ADR-2053 — Make direct context delivery authoritative and give every GPU analytics actor one residency

## Context

ADR-2007 split the GPU God Actor into a coordinator plus four subsystem supervisors and
described a decentralised `GPUContextBus` broadcast for `SharedGPUContext`.
`docs/BASELINE-architecture.md` repeated that as "not a central handle". The code does the
opposite: `ResourceSupervisor::distribute_context_to_supervisors` sends
`SetSharedGPUContext` point-to-point to the three subsystem supervisors first and only then
calls `context_bus.publish()`, under a comment reading "Also publish to event bus for any
additional subscribers". The bus has no in-tree subscriber. Every send was
`let _ = addr.try_send(...)` followed unconditionally by `info!("Context sent")`, so a full
mailbox dropped the context **and logged success** (diagram VC-10.3).
Separately, `src/app_state.rs:969-971` spawns standalone `ShortestPathActor` /
`ConnectedComponentsActor` that every `/pathfinding/*` route targets, while
`GraphAnalyticsSupervisor` spawns its own pair. Only the supervisor's pair receives a
context, so the HTTP pathfinding routes have been running permanently GPU-blind
(diagrams VC-15.6, VC-15.8).

## Decision

Direct point-to-point delivery from `ResourceSupervisor` to the three subsystem supervisors
is the authoritative mechanism for `SharedGPUContext`. The `GPUContextBus` is a
supplementary broadcast for observers that are not subsystem supervisors; a zero receiver
count is normal, not an error. ADR-2007 and BASELINE are corrected to say this rather than
the reverse.

Context delivery is not fire-and-forget. Each `try_send` result is inspected; a failure is
logged at `error!` naming the subsystem and the consequence, recorded in
`ResourceSupervisor::context_delivery_failures`, and reflected in `get_health` — a
supervisor that holds a context but failed to hand it on reports `Degraded`, not `Healthy`,
and its `last_error` names the failed targets. A silently GPU-blind subsystem is not an
acceptable state.

Every GPU analytics actor has exactly one residency: the instance its supervisor spawns.
Actors are not spawned a second time outside their supervisor. The `/pathfinding/*` routes
target the supervised instances through `GPUManagerActor` → `GraphAnalyticsSupervisor`,
whose `Handler<ComputeShortestPaths>` and `Handler<ComputeConnectedComponents>` therefore
become the live entry points and are retained.

GPU-absence handling stays per-actor and explicit rather than being unified: an actor either
degrades to a documented CPU path or refuses with a named error. What is forbidden is the
third behaviour — appearing to work while silently producing nothing.

## Consequences

The pathfinding routes begin actually using the GPU, which changes their latency and their
failure modes: they can now fail with GPU errors where previously they always took the
CPU-absent arm. That is the intended correction, but it is a behavioural change for any
consumer that had adapted to the degraded results.

Removing the standalone spawns requires an edit to `src/app_state.rs`, which belongs to the
vc-core lead; it was routed with the exact change and a sequencing constraint so the routes
are retargeted before the addresses disappear. That sequencing held: vc-core verified the
retarget had not landed, declined the deletion at the point this record's author declared a
green light, and completed both halves after ownership transferred — see the addendum below.

The `SetNodeSSSP` send that follows the standalone spawn (ADR-031 D2b, feeding wire slot 28)
could not simply be deleted with it: the message must reach whichever `ShortestPathActor`
actually holds a `SharedGPUContext`, and only `GraphAnalyticsSupervisor` can address its own
child. `impl Handler<SetNodeSSSP> for GraphAnalyticsSupervisor` was therefore added on this
side, forwarding to the supervised instance. It spawns the child first if the map arrives
before `Actor::started` has run, and logs at `error!` if the forward fails or the child is
absent — dropping the map silently would be precisely the failure class this ADR exists to
remove. vc-core deletes the standalone pair and the orphaned send in one edit once this
forward is green.

`ConnectedComponentsActor`'s CPU fallback is reachable but degenerate for a separate reason
recorded in ADR-2054: `UpdateComponentEdges` has no senders, so `cached_edges` is always
empty and the fallback would return all-singleton components. Wiring the GPU path is
therefore the fix that matters; the dead message is removed rather than wired.

## Verification

`cargo check -p visionclaw-server` — **exit 0, zero errors**, with every Phase 2 change in the
tree. (An earlier run in this phase was blocked by concurrent breakage in files owned by other
leads; those were fixed by their owners and the check was re-run clean.)

The bus-is-secondary finding was established by reading
`src/actors/gpu/resource_supervisor.rs` `distribute_context_to_supervisors` in full: three
`try_send` calls precede the single `context_bus.publish()`, whose own comment names it
supplementary.

The duplicate-residency finding was established by `grep -rn "ShortestPathActor::new\|ConnectedComponentsActor::new"`
showing a spawn in `src/app_state.rs` in addition to the supervisor's, cross-checked against
`distribute_context_to_supervisors`, which sends to supervisors only.

Verification ran on the uncommitted working tree above the recorded SHA and must be re-run
at the landing commit; `verified_paths` is empty for that reason.

### Landed by vc-core — 2026-09-05

`implementation_status` moves `partial` → **complete**, `activation_status`
`staged` → **live**. vc-gpu-wire became unreachable with two pieces outstanding;
the queen transferred `src/handlers/api_handler/analytics/pathfinding.rs` and
`src/actors/gpu/gpu_manager_actor.rs` to vc-core, who completed it.

**The message set did not match, which is why the retarget was not a rename.**
This record assumed `GraphAnalyticsSupervisor` already handled what the routes
send. It handled `ComputeShortestPaths` and `ComputeConnectedComponents`, but the
HTTP handlers send `ComputeSSP`, `ComputeAPSP`, `GetShortestPathStats` and
`GetConnectedComponentsStats` directly to the child actors. Retargeting only the
two supported messages would have split the routes across two actors — worse than
either end state — so the missing forwards were added first:

- `GraphAnalyticsSupervisor` gained `Handler<ComputeSSP>` and `Handler<ComputeAPSP>`,
  each mirroring the existing `ComputeShortestPaths` guard (forward only to a child
  that exists *and* is running, else a plain error).
- The two stats reads needed new messages, `GetSupervisedShortestPathStats` and
  `GetSupervisedComponentsStats`, rather than forwards of the existing ones.
  `GetShortestPathStats` declares a bare `#[rtype(result = "ShortestPathStats")]`,
  so a supervisor forward would have had to invent a value when the child is
  absent — and neither stats struct derives `Default`, so "no actor" would have
  been indistinguishable from "genuinely all zeroes". The wrappers return
  `Result`, keeping absence explicit.
- `GPUManagerActor` gained `Handler<SetNodeSSSP>` (the hop this record identified
  as missing) plus forwards for the four messages above, via a local macro since
  all five share one shape. `AppState` holds only the manager's address, never the
  supervisor's, so without the `SetNodeSSSP` hop the map could not have reached
  the supervised child at all.

**`pathfinding.rs`** now sends every one of those through `data.gpu_manager_addr`:
`compute_sssp`, `compute_apsp`, `compute_connected_components`, both stats routes,
and the `PathAlgorithm::Sssp` fall-through inside the point-to-point handler — a
fifth site this record did not list.

**`src/app_state.rs`** collapsed: the standalone `ShortestPathActor` /
`ConnectedComponentsActor` spawns are gone, `SetNodeSSSP` now goes to
`GPUManagerActor`, and the `AppState.shortest_path_actor` /
`.connected_components_actor` fields are removed. The four-slot tuple became a
plain `let gpu_manager_addr = { ... }`, because a third slot was also dead:
`stress_majorization_addr` was already a literal `None` with a comment claiming
the actor "will be available after GPU initialization" — nothing ever assigned it.
It is retained as an always-`None` field only because the readiness inventory
reports on it. `AppState.graph_ops` is now `GraphSubsystem::default()` with a
comment saying so: the children are supervisor-owned, and
`GraphAnalyticsSupervisor`'s `GetSubsystemHealth` is the live health source.

**Verification** (vc-core, on the uncommitted working tree above
`b00c28a0d766c8cf46cd00b100dab60ef2dd74a4`; must be re-run at the landing commit):

```
cargo check -p visionclaw-server --lib            0 errors
cargo check -p visionclaw-server --lib --release  0 errors
```

The release check matters here beyond habit: it is the arm in which the
`#[cfg(any(debug_assertions, feature = "dev-auth"))]` gates added under ADR-2044
and ADR-2058 disappear, so it type-checks the code path a shipped binary actually
takes.

## Addendum 2026-10-12: the `ForceComputeActor` self-initialisation fallback (TODO-unified S-4 item 4)

This record names `ResourceSupervisor` as the authoritative path for GPU context
delivery. It did not mention that `ForceComputeActor::initialize_own_gpu_context`
(`src/actors/gpu/force_compute_actor.rs`) can build a `SharedGPUContext` itself: it
opens a CUDA device, loads the PTX modules and constructs the context directly, outside
the supervisor chain. That is deliberate and is recorded here as such.

- **When it runs.** Only if `shared_context` is still `None`; a context delivered by
  `SetSharedGPUContext` always wins and the method returns at once. It is also attempted
  lazily on the first physics step if no context has arrived.
- **Why it exists.** The supervisor chain (`GPUResourceActor` to `GPUManagerActor` to
  `ResourceSupervisor` to `PhysicsSupervisor` to `ForceComputeActor`) is subject to message
  races at boot; the fallback keeps GPU physics from staying disabled behind a lost message.
- **Bounded.** A retry budget with exponential backoff; when it is exhausted the actor stops
  trying and reports through `notify_gpu_init_failed`.
- **Cost.** A context built this way is invisible to `ResourceSupervisor`'s accounting and
  health view, so memory limits and `GetSubsystemHealth` do not cover it. Treat it as a
  degraded path, not a second authority: a boot log line `GPU context self-initialized
  successfully` means the supervisor path failed.
- **Review trigger.** Remove the fallback once context delivery has been boot-reliable
  across a release, or fold it into the supervisor so the context is registered there.
