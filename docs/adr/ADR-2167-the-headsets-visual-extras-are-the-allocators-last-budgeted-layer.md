---
id: ADR-2167
title: The headset's visual extras are the allocator's last budgeted layer, tiered by measured GPU time
date: 2026-10-31
decision_status: proposed
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 8e92c5d5c552164a9190424ef0cf637edc455455
verified_paths: [xr-client/rust/src/frame_budget.rs, xr-client/scripts/frame_budget_pass.gd, xr-client/scripts/eye_candy.gd, xr-client/scripts/emulated_volume.gd, xr-client/scripts/spatial_environment.gd, xr-client/perf/benchmark.gd, xr-client/perf/gpu_time_probe.gd]
owner: jjohare
review_trigger: A headset session that measures both eyes, or any change to the renderer, the frame budget or the comfort switches.
repo: visionclaw
domain: BASELINE-architecture
lineage: Extends ADR-2107 (shared visual language, reversible comfort) and the frame-budget allocator of docs/XR-client.md; renderer rule unchanged from ADR-2032.
---

# ADR-2167 — The headset's visual extras are the allocator's last budgeted layer, tiered by measured GPU time

## Context

The operator asked for the headset's visual polish to use whatever GPU budget remains. PRD-008 allows 100 000 triangles and 50 draw calls; the allocator uses 95 000 and 48 after its 5 % reserve, and the graph's LOD spends every spare triangle, so benchmarks sit near 94.5k triangles and 31–34 calls. The free resources are draw calls and per-pixel work, not triangles. The renderer is `gl_compatibility` with every post-process off (glow blanks the second eye); reduced motion is the default; low-cost mode must remove every extra. A steady 90 FPS is vsync-locked and says nothing about spare GPU time.

## Decision

1. **One controller, one budget layer.** The extras (static sky, ribbon volume frames, ambient motes) live behind `eye_candy.gd`, a child of `SpatialEnvironment`. `FrameBudget.allocate_with_extras` prices them as the **last** claim, after the graph's cylinder edges: graph, cloud, route and burst caps equal `allocate`'s whatever is asked, the grant is the highest tier whose triangles fit in the leftover and whose draw calls keep the frame at or under 48, and when the minimums overrun only the zero-cost sky remains. `allocate` is `allocate_with_extras` with tier 0.
2. **Tiers by triangle cost:** 0 off; 1 static sky (0 triangles, 0 calls); 2 ribbon frames (24 triangles and one call a frame); 3 and 4 a 128- or 512-mote MultiMesh (one call). The controller draws only the grant and reports what it drew, and the pass subtracts it from the measured `other_tris`.
3. **Tier steps on measured headroom.** The controller probes `viewport_get_measured_render_time_gpu` at start-up and steps on GPU ms (drop above 8 ms p95 at once, climb below 5.5 ms after 8 s, hold-off doubling after a reversal); if the timer reads zero it uses frame-time p95 with a ceiling of tier 2 and a 20 s hold.
4. **Comfort.** The extras look right with reduced motion on (static sky, static ribbons, still motes); motion is added only when it is off. `XR_VISUAL_QUALITY=low` turns every extra off. The Environment is copied on write.
5. **Portal weights.** `FrameBudgetPass.set_volume_weights` weights the near-cap split by `drawn × weight`; empty or equal weights give the unweighted split exactly, and the `GEM_MIN` floor holds.

## Consequences

The extras cost graph LOD only in the triangles they draw (584 at tier 4 with three volumes), and none at all at tiers 0–1. At production density the leftover is under one near-detail step, so only the sky shows (measured: tier 1 at 13 164 nodes, tier 4 on the 1k fixture); tier 4 there would cost about two of 35 gem nodes, a trade left to the operator. The sky is the one extra that costs pixels rather than geometry. Volume frames are one call each, not one for all volumes. The frames' calls were never counted before (`overhead_draw_calls` is not read by the budget pass); tier 2 counts them. Stereo behaviour of the sky shader, the existence of the GPU timer on the XR viewport and fill rate in both eyes are unverified until a headset session.

## Verification

GPU timer: `perf/gpu_time_probe.gd` on the HP, windowed, XR off, Godot 4.6.1 and 4.8-beta2: nonzero and load-following (0.019 → 0.43 ms). Allocator: 25 `frame_budget` Rust tests, 10 of them new (`cargo test --lib frame_budget`). Scene: GUT 441 passing on 4.6.1 and 4.8-beta2 with `gut_guard` 47/47. Benchmark (`XR_BENCH_CANDY`): see `docs/XR-client.md` "Visual extras and the budget layer" and the implementer's report; 13 164 nodes stays at or under 48 calls and 100k triangles at every tier. Mono desktop only.
