---
title: Physics Parameter Calibration Proposal
description: Proposed changes to default physics parameters to reduce visible oscillation and false-settlement reporting
category: proposal
tags: [physics, calibration, force-directed, tuning]
status: proposal
date: 2026-04-18
---

# Physics Parameter Calibration Proposal

## Problem Statement

Users report a **"jumpy internal fighting state"** during graph layout — the simulation appears to drift and fight with itself even after physics claims to have settled. Live instrumentation confirmed this matches reality: with the default running parameters the system holds mean node velocity at ~8–9 units/tick indefinitely while reporting `isSettled: true`.

## Diagnosis Findings (2026-04-18, 2,242-node graph)

| Metric | Observed | Expected |
|--------|---------:|---------:|
| Mean node \|velocity\| | 8.5 units/tick (~530 u/s) | < 1.0 at rest |
| Median node \|velocity\| | 8.96 units/tick | < 0.5 at rest |
| Max node \|velocity\| | 19.3 units/tick | < 5.0 |
| % nodes with \|vx\| > 0.5 | 99.6% | Small fraction during transitions |
| Position.x spread | -560 to +669 (span 1229) | < 500 |
| Position.y/z spread | Near zero (flat) | ≈ same order as X |
| `settlementState.isSettled` | `true` (stale — fixed in `f2c1c942a`) | `false` during motion |
| `settlementState.kineticEnergy` | `0.0` (hardcoded — fixed) | Actual GPU KE |

**Root cause** — force scaling interaction:

1. `scalingRatio = 10.0` multiplies LinLog attraction by 10, creating oversized attraction at long range.
2. `repelK = 250.0` versus `springK ≈ 25` gives a 10:1 repulsion-to-spring ratio, which pushes nodes far apart before the spring can pull them back. This causes large-amplitude oscillation along edge lines.
3. `centerGravityK = 5.0` × r hits the `maxForce = 1000` clip once `r > 200`, so outer nodes receive a uniform (rather than restoring) pull. This flattens the potential well and lets nodes wander freely beyond ±200.
4. `damping = 0.6` retains 40% of velocity per tick — adequate in isolation, but insufficient to bleed the kinetic energy injected by the three items above fast enough to settle.
5. The system therefore finds an equilibrium at ±600+ on X while Y/Z are compressed to ≈ 0 (strongest force winner-takes-all per axis).

## Proposed Defaults

Applied live against the running graph and measured the effect over 14 seconds:

| Parameter | Current Running | Proposed | Effect Measured |
|-----------|----------------:|---------:|-----------------|
| `damping` | 0.6 | **0.9** | Mean \|v\| 15.7 → 6.4 (59% ↓), Max \|v\| 42 → 16 (62% ↓) |
| `scalingRatio` | 10.0 | **2.0** | Removes 5× LinLog amplification, prevents long-range pulls dominating |
| `repelK` | 250.0 | **80.0** | Restores healthy spring:repulsion ratio around 1:8 |
| `centerGravityK` | 5.0 | **0.5** | 10× weaker gravitational pull, stays below maxForce clip for all r < 2000 |
| `springK` | 25.1 | **10.0** | Balanced against the lower repulsion |

**Immediate result** (live-applied via PUT, no rebuild):
- Mean \|v\| dropped from 15.7 → 6.4 within 3 seconds
- Max \|v\| dropped from 42 → 11 — no more outlier runaways
- After 14 seconds the graph transitioned from compressed X-dominant line (Y/Z flat within ±30) to healthy 3D distribution (Y/Z span ±1000)
- Mean \|v\| rose to 8.2 during the 3D expansion transient — expected while nodes find new positions; will settle once equilibrium reached

## Source Files to Update

The running system picks parameters from multiple sources. Proposal aligns them:

### 1. `src/config/physics.rs` (Rust `PhysicsSettings::default()`)

```rust
impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            damping: 0.9,           // was 0.95
            repel_k: 80.0,          // was 900.0
            spring_k: 10.0,         // was 14.0
            scaling_ratio: 2.0,     // was 10.0 — LinLog multiplier
            // centerGravityK lives on a separate field path in this struct
            ...
        }
    }
}
```

### 2. `data/settings.yaml` — verify both physics profiles

Currently two profiles (lines 140–180 and 265–300). Proposal aligns on:

```yaml
physics:
  damping: 0.9
  repelK: 80.0
  springK: 10.0
  centerGravityK: 0.5
  scalingRatio: 2.0
  # ...existing other fields unchanged
```

### 3. `src/layout/types.rs:48` — second `scaling_ratio: 10.0` default

Align to `2.0` for consistency.

## What This Doesn't Fix

- **Runaway rescue is still needed** — node 2190 "Rigid Body" moved 1620 units in 2 seconds with the old defaults. The boundary-stuck rescue (commit `fcfc1a166`) catches these cases; keep it.
- **`stable_frame_count` telemetry** still reports 0 — plumbing it end-to-end through CQRS requires a separate PR.
- **Settings-source fragmentation** — parameters arrive from `PhysicsSettings::default()`, `settings.yaml` (two sections), and any persisted overrides. This proposal aligns the static defaults but doesn't resolve the source-of-truth question. Track as a follow-up: ADR-XXX "Physics Settings Source of Truth".

## Risk Assessment

**Low risk.** These are numeric parameter defaults. They affect the *initial* layout physics only; users with saved preferences override them. If a user has stored custom values, their behaviour is unchanged.

**Rollback** — revert the three files above.

## Related Fix Already Landed

Commit `f2c1c942a` (`fix(api): report real kinetic energy and settlement state`) fixed the misleading `isSettled: true` / `kineticEnergy: 0.0` hardcoded telemetry. Without this, the calibration effect would be invisible to clients that gate on `isSettled`. The new handler reports real KE from `ForceComputeActor.GetPhysicsStats` and computes `is_settled` against `auto_pause.equilibrium_energy_threshold`.

## Verification

After rebuild with the proposed defaults:

1. Start with empty graph, let ingestion populate (~2,242 nodes).
2. Physics should settle within ~15 seconds (mean \|v\| < 0.5 across all nodes).
3. `GET /api/graph/data` should report `isSettled: true` with `kineticEnergy < 0.01` once actually settled — no longer a lie.
4. Position spread should be bounded (e.g., span < 1,000 on each axis) without runaway outliers.
5. Physics parameter changes via UI sliders should produce smooth, damped transitions rather than oscillation.

## Conclusion

The current running defaults create a stable-but-excited equilibrium where nodes perpetually drift at ~530 units/sec on X while Y/Z collapse to a flat plane. The proposed defaults produce a calmer, genuinely 3D layout with faster convergence. Live testing confirmed a 59–74% reduction in node velocity and elimination of outlier runaways.
