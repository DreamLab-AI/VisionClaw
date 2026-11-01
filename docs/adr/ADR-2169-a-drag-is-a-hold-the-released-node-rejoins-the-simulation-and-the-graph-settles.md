---
id: ADR-2169
title: A drag is a hold: the released node rejoins the simulation and the graph answers the hand within a few ticks
date: 2026-11-01
decision_status: accepted
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
amends: [ADR-2140]
relates: [ADR-2141, ADR-2168]
verified_commit: 37d1226b10c36c44ba854a3fccfe8dff3785aec5
verified_paths: [src/actors/gpu/hold_registry.rs, src/actors/gpu/force_compute_actor.rs, src/actors/physics_orchestrator_actor.rs, src/handlers/socket_flow_handler/position_updates.rs, src/handlers/socket_flow_handler/types.rs, src/handlers/layout_handler.rs, scripts/drag-physics-acceptance.mjs]
owner: jjohare
review_trigger: the first headset session after the server carrying this change is deployed (does the graph answer the hand, does it rest about two seconds after release), or any change to the FA2 integrator or the stored `globalSpeed`.
repo: visionclaw
domain: PROTOCOL-registry
evidence_class: source+test
acceptance_boundary: registry, actor, orchestrator and handler rules are covered by unit tests, and the response rate against `global_speed` and the Verlet-mode damping by tests on an RTX A6000. Not established: the step and broadcast rate with a hold on the live graph, the time to rest on the live graph, the second-connection view against a running backend (the acceptance script exists and has only been run against a local mock), and why the live settings PUTs register as changes.
---

# ADR-2169 — A drag is a hold: the released node rejoins the simulation and the graph answers the hand within a few ticks

## Context
- Owner requirement, 2026-11-01: up to two nodes held at once (one per controller), the whole graph responds as fast as practical on the server and every client, and a released node rejoins the physics instead of staying pinned.
- Before this change `nodeDragEnd` left the node pinned on the GPU until an explicit `nodeUnpin`. One global pin map served drags and pins alike, so a second client's release freed a node the first still held. A connection could hold five nodes.
- While a node was held nothing forced the GPU to step: the stability kernel and the orchestrator's equilibrium check could skip a still graph.
- The settings reheat adds uniform random velocity of up to twice its factor to every node on every step while it lasts (`inject_velocity_perturbation`, 0.997 decay per step to a 0.02 floor, about 1,300 steps from 1.0). Any write the GPU actor judged a change re-armed it. The orchestrator's FastSettle damping override and restore are such writes. The live log shows 22–1,680 consecutive reheat steps after each "params CHANGED", and the per-step "Reheating physics" line is that one reheat logged once per step, not a stream of senders.
- Measured step: about 9–10 Hz on the live graph (9,688 nodes), 84–94 ms of it the force kernel. At the stored `globalSpeed` 0.4 a node held 300 units out moves its neighbours 4.5 units in 120 steps.

## Decision
1. **A drag is a hold, owned by a connection.** `nodeDragStart` and `nodeDragUpdate` send `HoldNodes{holder, holds}`; `nodeDragEnd`, the 2 s drag timeout and a disconnect send `ReleaseNodes` / `ReleaseHolder`. The node leaves the pinned set and rejoins the simulation. The last write wins when two connections hold one node, and a release counts only from the node's current owner. A connection holds at most two nodes (one per hand). A rate-limited update is coalesced and applied by a trailing timer or at drag end, not dropped.
2. **Explicit pins are separate.** `nodePin` (new, `{nodeId, position}`, ack `nodePinAck`) and `nodeUnpin` keep their meaning. A pin made before or during a hold survives the release; the node stays anchored where it was dropped.
3. **The GPU steps every tick while anything is held, by any client, and for 2 s after the last release.** The stability bypass is `warmup > 0 || any_held || settling`; the orchestrator removes the 16 ms inter-step pause while a node is held and never auto-pauses then.
4. **The settings reheat is withheld during a hold and the settle window**, and a grab clears any standing one. A damping-only `UpdateSimulationParams` is applied without re-arming the reheat or the 30 s warmup. Pin events use a one-shot impulse (0.05) instead of a decaying reheat.
5. **`global_speed` floors at 2.0 while held and through the settle window.** Under the live FA2 integrator velocity is set from `force * global_speed`, and the measured response is 4.5 / 10.5 / 19 / 33 / 55 units at 0.4 / 1 / 2 / 4 / 8. `damping`, `max_force` and `max_velocity` have no effect there. The 0.6 settle damping applies in Verlet mode only (about 50× less residual energy after 20 steps there).
6. **Amends ADR-2140 item 2 for the span of a hold.** ADR-2140 forbids any multiplier, clamp or floor between the store and the GPU. Items 3 and 5 are such an override: while a node is held or settling, the step runs with `global_speed = max(stored, 2.0)` and, in Verlet mode, `damping = min(stored, 0.6)`; the stored document is untouched and the stored values apply again the moment the window closes. This is the same kind of transient override the FastSettle damping already is. If the owner wants the 2.0 and the 0.6 to be stored, user-tunable values (compliant with ADR-2140 item 1), that is a schema, validator and client change and is open follow-up.
7. **Every client sees it.** The GPU's per-step snapshot is published to the client coordinator as before; nothing in the drag path broadcasts or paces. `/api/layout/status` now reports the GPU actor's iterations, settled flag and kinetic energy instead of fixed zeros.

## Consequences
- A client that relied on the drag-end pin must send `nodePin`. The XR client's own pin set (`fx/fy/fz`) is a client follow-up (ADR-2168 stops recording drag releases as pins).
- Time to rest after a release is bounded by the FA2 integrator, not by damping: at `globalSpeed` 2.0 the neighbourhood moves about four times faster, and rest within about 2 s on the live graph is expected, not measured.
- Rates are GPU-bound. With the step at 84–94 ms, removing the 16 ms pause is the only cadence headroom; no higher broadcast rate is available without a cheaper kernel.
- Why the live settings PUTs (about every 3 s, `dev-mode-local-admin`) register as changes while the visible fields are equal is not established; the gate returns "unchanged" for the live document in a unit test, so a hidden field differs between PUTs.

## Verification
`cargo test --lib` and `cargo test -p visionclaw-domain` at the verified commit (see the stream report); the GPU tests in `execution.rs` (`grid_gpu_tests`) on an RTX A6000; `scripts/drag-physics-acceptance.mjs` against a local mock only. Each rule above was also broken in turn and its test shown to fail.
