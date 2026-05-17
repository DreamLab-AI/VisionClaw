---
title: Physics Parameters Reference
description: UI slider names, settings keys, ranges, and backend parameter mapping for the VisionClaw physics simulation
category: reference
difficulty-level: intermediate
tags: [physics, simulation, settings, GPU, CUDA]
updated-date: 2026-04-18
---

# Physics Parameters Reference

All physics sliders live under the **Physics** tab in the Control Panel. Changes are
debounced and sent via `PUT /api/settings/physics` to the backend.

---

## Core Physics Sliders

| UI Label | Settings Key | Min | Max | Step | Backend Field | Effect |
|---|---|---|---|---|---|---|
| Attraction | `attractionK` | 0 | 10 | 0.1 | `physics.attractionK` | Edge attraction strength — fully effective at ~10 |
| Spring Strength | `springK` | 0.1 | 100 | 0.5 | `physics.springK` | Edge spring constant; 8–20 recommended for 2K+ node graphs |
| Repulsion | `repelK` | 0 | 3000 | 10 | `physics.repelK` | Node–node repulsion; balance with gravity (800–1500 recommended) |
| Cluster Tightness | `centerGravityK` | 0 | 10 | 0.1 | `physics.centerGravityK` | Pull toward centre; higher values tightly cluster the graph |
| Damping | `damping` | 0 | 1 | 0.01 | `physics.damping` | Velocity damping — lower = more energy, higher = faster settle |
| Node Spacing | `restLength` | 1 | 200 | 1 | `physics.restLength` | Spring rest length — small = dense, large = spread |
| Max Velocity | `maxVelocity` | 0.1 | 500 | 5 | `physics.maxVelocity` | Maximum node speed per step |
| Dual Graph Separation | `graphSeparationX` | 0 | 500 | 5 | `physics.graphSeparationX` | X-axis distance between knowledge and ontology graph planes — 0 = merged |
| Flatten to Planes | `zDamping` | 0 | 0.1 | 0.002 | `physics.zDamping` | Squash Z-axis — 0 = full 3D, 0.1 = fully flat YZ planes |

## Advanced Dynamics (visible in Advanced mode)

| UI Label | Settings Key | Min | Max | Step | Backend Field | Effect |
|---|---|---|---|---|---|---|
| Time Step | `dt` | 0.001 | 0.1 | 0.001 | `physics.dt` | Simulation time step per iteration |
| Iterations | `iterations` | 1 | 5000 | 50 | `physics.iterations` | Solver iterations per frame |
| Warmup Iterations | `warmupIterations` | 0 | 500 | 10 | `physics.warmupIterations` | Initial stabilisation iterations before convergence is checked |
| Cooling Rate | `coolingRate` | 0.00001 | 0.01 | 0.0001 | `physics.coolingRate` | Simulated annealing decay rate |
| Temperature | `temperature` | 0.001 | 100 | 0.5 | `physics.temperature` | Simulation energy — higher = more node movement |
| Max Force | `maxForce` | 1 | 1000 | 5 | `physics.maxForce` | Force cap per node per step |
| Separation Radius | `separationRadius` | 0.01 | 200 | 0.5 | `physics.separationRadius` | Minimum enforced node separation |
| Min Distance | `minDistance` | 0.05 | 20 | 0.1 | `physics.minDistance` | Minimum repulsion distance |
| Max Repulsion Dist | `maxRepulsionDist` | 10 | 2000 | 10 | `physics.maxRepulsionDist` | Range beyond which repulsion is ignored |
| Repulsion Cutoff | `repulsionCutoff` | 1 | 2000 | 10 | `physics.repulsionCutoff` | Hard cutoff for repulsion force application |

---

## Pipeline: slider → CUDA kernel

```
User drags slider
  → React settings store (Zustand)
    → debounced PUT /api/settings/physics (Axum handler)
      → PhysicsOrchestratorActor (receives UpdateSimulationParams message)
        → resets fast-settle counters, triggers reheat
          → ForceComputeActor (dispatches ComputeForces to GPU)
            → visionflow_unified.cu CUDA kernel
              → updated node positions broadcast via WebSocket V4 binary
```

---

## Effective Ranges

The slider maxima are conservative limits chosen to prevent degenerate layouts:

- **`attractionK` > 10**: Edges pull so strongly that nodes collapse into a hairball; at 10 the force is saturated for typical graph densities.
- **`graphSeparationX` > 500**: Creates visually unusable gaps between the knowledge and ontology planes; the planes become unnavigable in 3D space.
- **`zDamping` > 0.1**: Fully eliminates Z variation, collapsing the graph into a flat 2D plane; values above 0.1 provide no additional flattening.
- **`repelK` > 3000**: Nodes explode beyond the bounding box before the physics loop can compensate.
- **`springK` > 100**: Springs become rigid rods; the layout oscillates rather than settling.

Recommended starting values for a ~2000-node graph are documented in
[`docs/physics-parameter-analysis.md`](../physics-parameter-analysis.md).

---

## FastSettle vs Continuous Mode

Physics runs in one of two modes set by `SettleMode` in `SimulationParams`:

**FastSettle** (`SettleMode::FastSettle { max_settle_iterations, energy_threshold, damping_override }`)
- Fires GPU steps as fast as the GPU can process (0 ms sleep between steps).
- Stops when kinetic energy drops below `energy_threshold` **or** `max_settle_iterations` is reached — whichever comes first.
- Convergence is not checked during the warmup window (`warmupIterations`) to avoid false early termination from residual reheat energy.
- A slider change during FastSettle resets `fast_settle_iteration_count` to zero and triggers a reheat cycle, restarting the settle.

**Continuous** (`SettleMode::Continuous`)
- Fires at ~60 fps indefinitely.
- Used when real-time responsiveness matters more than convergence (e.g. live graph edits).
- Slider changes take effect on the next tick with no reheat.

---

## See Also

- [Physics/GPU Engine](../explanation/physics-gpu-engine.md) — architecture of the simulation pipeline
- [Physics Parameter Analysis](../physics-parameter-analysis.md) — empirical tuning results for 2,242-node graphs
- [REST API — PUT /api/settings/physics](./rest-api.md) — request schema and auth requirements
