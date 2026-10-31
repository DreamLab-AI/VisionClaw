---
id: ADR-2167
title: The headset's visual extras are the allocator's last budgeted layer, tiered by measured GPU time
date: 2026-10-31
decision_status: proposed
implementation_status: partial
activation_status: staged
evidence_class: source+test
acceptance_boundary: mono desktop and headless on HP (Godot 4.6.1 and 4.8-beta2); not stereo, headset or both-eye fill rate
supersedes: []
superseded_by: []
verified_commit: 3f22dc33906eaae155ef88d129eba16c538fa855
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

1. **One controller, one budget layer, a bounded allowance.** The extras (static sky, ribbon volume frames, ambient motes) live behind `eye_candy.gd`, a child of `SpatialEnvironment`. By the owner's decision of 2026-10-31, `FrameBudget.allocate_with_extras` holds back an **allowance** ahead of the graph's near detail, in this order: the hard minimums (far tiers, floors); the route, cloud, burst pool and hulls; the allowance; the gem then cylinder share. The allowance is the cost of the *requested* tier capped at tier 4, `extras_cost(min(demand, 4), frames)`, never a higher tier; the granted tier is the highest at or below it whose triangles fit and whose draw calls keep the frame at or under 48, and a tier the guard refuses reserves nothing, so a pinned lower tier or a refused tier returns its triangles to the near detail in the same allocation. The cloud, route, burst and hull caps equal `allocate`'s, and the graph loses at most `ceil(allowance / GEM_STEP)` gems (GEM_STEP 288). The allowance is 0 when the minimums overrun, or when the route, cloud, burst pool or hulls get less than they asked for; the extras are then the last claim, buying only what the graph left (the zero-cost sky when nothing is left). `XR_VISUAL_QUALITY=low` and `XR_EYE_CANDY=0` request tier 0 and reserve nothing; reduced motion leaves the motes drawn (still), so it changes neither tier nor cost. `allocate` is `allocate_with_extras` with tier 0. This replaces the earlier rule that graph caps are exactly `allocate`'s.
2. **Tiers by triangle cost:** 0 off; 1 static sky (0 triangles, 0 calls); 2 ribbon frames (24 triangles a frame; no call of their own, because the ribbon replaces the 1 px line in the frame's existing call, which the scene counts); 3 and 4 a 128- or 512-mote MultiMesh (one call). The scene counts every volume frame's call (`GraphScene._tick_frame_budget` adds `overhead_draw_calls()` per volume), so the extras never count one twice. The controller draws only the grant and reports what it drew, and the pass subtracts it from the measured `other_tris`.
3. **Tier steps on measured headroom.** The controller probes `viewport_get_measured_render_time_gpu` at start-up and steps on GPU ms (drop above 8 ms p95 at once, climb below 5.5 ms after 8 s, hold-off doubling after a reversal); the governor never requests below tier 1, because the sky costs nothing and swapping it flips the background in both eyes. GPU time is chosen only if most probe frames read nonzero, and in GPU mode about 2 s of consecutive zero readings fall back to frame-time p95 (ceiling tier 2, 20 s hold).
4. **Comfort.** The extras look right with reduced motion on (static sky, static ribbons, still motes); motion is added only when it is off. `XR_VISUAL_QUALITY=low` turns every extra off. The Environment is copied on write.
5. **Portal weights.** `FrameBudgetPass.set_volume_weights` weights the near-cap split by `drawn × weight`; empty or equal weights give the unweighted split exactly, and the `GEM_MIN` floor holds.

## Consequences

The extras cost graph LOD only in the triangles they draw (72 at tier 2, 200 at tier 3, 584 at tier 4 with three volumes), and none at all at tiers 0–1. Before the allowance the leftover at production density was under one near-detail step, so only the sky showed (measured: tier 1 at 13 164 nodes, tier 4 on the 1k fixture). With it, the allocator at the benchmark's production shape (13 164 nodes, `other_tris` 2 182, 6 000 cloud rows, a 13-row route) grants the requested tier: gems 35, 35, 35, 35, 33 and cylinders 7, 7, 4, 0, 6 for tiers 0–4, so tier 4 costs two gems (the allocator's output, not re-measured in the renderer). The cylinder cap is the remainder of the gem step and may move either way. The sky is the one extra that costs pixels rather than geometry. Volume frames are one call each, not one for all volumes. The frames' calls were never counted before the review fixes (`overhead_draw_calls` was read by no production code); the scene now counts them at every tier, and tier 2 adds none. At tiers 0–1 that makes the counted calls match the renderer's (two more than before on the benchmark's two frames). Stereo behaviour of the sky shader, the existence of the GPU timer on the XR viewport and fill rate in both eyes are unverified until a headset session.

## Verification

GPU timer: `perf/gpu_time_probe.gd` on the HP, windowed, XR off, Godot 4.6.1 and 4.8-beta2: nonzero and load-following (0.019 → 0.43 ms). Allocator: 28 `frame_budget` Rust tests (`cargo test --lib frame_budget`), including the allowance's bound swept over 32 node counts, the production-shape grant, the call-guard return and the zero-under-pressure cases. Scene: GUT on the review-fix commit, 4.6.1 and 4.8-beta2 each 499 of 502 passing (3 pending, pre-existing), 0 failing, `gut_guard` 48/48. Benchmark (`XR_BENCH_CANDY`): see `docs/XR-client.md` "Visual extras and the budget layer" and the implementer's report; 13 164 nodes stays at or under 48 calls and 100k triangles at every tier. Mono desktop only.

## Review follow-up (R11, 2026-10-31)

Re-stamped to `a4c07caed`. Re-run: `git diff --stat 3fffb8a20bd767e0ca033dfb58436516de913889 a4c07caed3f05bc170a0096fdde878ceebbd19c1 -- xr-client/rust/src/frame_budget.rs xr-client/scripts/frame_budget_pass.gd xr-client/scripts/eye_candy.gd xr-client/scripts/emulated_volume.gd xr-client/scripts/spatial_environment.gd xr-client/perf/benchmark.gd xr-client/perf/gpu_time_probe.gd` gives `benchmark.gd | 6`, `frame_budget.rs | 22`, `eye_candy.gd | 101`, `frame_budget_pass.gd | 15` (97 insertions, 47 deletions) and nothing else. Fixed: the governor floor is tier 1 (F1); the tier-cost test measures the built scene and the fallback table has a parity test (F2); the GPU-mode governor leaves a dead timer for frame time (F3); the volume frames' draw calls are counted by the scene and `extras_cost` no longer charges them (F4); one cached cost table, the dead `_extras_calls` removed, the demand clamped to the top tier (F6, F7a, F7c). **Benchmark numbers are not re-measured:** the allocator-calls column in `docs/XR-client.md` predates the hook.

**Owner decision (F5, 2026-10-31): left as is.** The portal weights are almost inert at production density. The `GEM_MIN` floor (16) is dealt to every visible volume before any weighting, and the gem cap there is 35 (7 cylinders): with two visible volumes only 3 gems are weighted (about 19 against 16 for weights 1.0 and 0.15), and with three visible volumes the floors (48) exceed the cap, so the split is unweighted and the weights do nothing; the cylinder cap is below any floor. ADR-2166's "XE's weights are what keep the portals from taking budget" holds at the 1k fixture (gem cap 80) only. The owner chose on 2026-10-31 to leave the weighting and the floors unchanged (the two options considered were: the floor applies only to volumes with weight 1.0, the others flooring at `GEM_MIN/4`; or the portals are left out of the near split while one is entered). The same decision approved the extras allowance, which supersedes the earlier "extras never change the other caps" rule; see Decision 1 and the amendment below.

## Amendment: bounded extras allowance (owner decision, 2026-10-31)

The owner approved reserving up to tier 4 (ribbons plus 512 motes) ahead of the graph's near detail, so the extras show at production density, and left the portal weighting alone (R11 F5). Decision 1 is rewritten above; the rest of the record stands. `FrameCaps` gains `extras_allowance` (also in the GDScript caps dictionary and the pass's defaults), the triangles held back; it is 0 under pressure. The benchmark calls the Rust allocator itself, so its allocator-against-measured parity needs no change; the renderer columns of `docs/XR-client.md` are not re-measured and say so.

Re-stamped to `3f22dc339` (2026-10-31, this amendment; previous stamp `a4c07caed`). Re-run: `git diff --stat a4c07caed3f05bc170a0096fdde878ceebbd19c1 3f22dc33906eaae155ef88d129eba16c538fa855 -- xr-client/rust/src/frame_budget.rs xr-client/scripts/frame_budget_pass.gd xr-client/scripts/eye_candy.gd xr-client/scripts/emulated_volume.gd xr-client/scripts/spatial_environment.gd xr-client/perf/benchmark.gd xr-client/perf/gpu_time_probe.gd` gives `eye_candy.gd | 6`, `frame_budget.rs | 264`, `frame_budget_pass.gd | 10` (231 insertions, 49 deletions) and nothing else. `evidence_class` and `acceptance_boundary` are unchanged: mono desktop and headless only.
