---
id: ADR-2191
title: Graph-sized GPU buffers are declared once and resized together; SSSP results never outlive their graph; SSSP spring adjustment is off by default
date: 2026-11-10
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
amends: [ADR-2140]
relates: [ADR-2028, ADR-2029, ADR-2143, ADR-2169, ADR-2173]
verified_commit: fd7525907eb3f1eddc8db1d9e32b29a5c5021564
verified_paths: [src/adapters/sqlite_settings_repository.rs, src/handlers/api_handler/analytics/pathfinding.rs, scripts/gpu-physics-gate.sh, crates/visionclaw-gpu/tests/force_kernel_baseline/harness.rs, src/utils/unified_gpu_compute/buffers.rs, src/utils/unified_gpu_compute/construction.rs, src/utils/unified_gpu_compute/memory.rs, src/utils/unified_gpu_compute/sssp.rs, src/utils/unified_gpu_compute/community.rs, src/utils/unified_gpu_compute/execution.rs, src/utils/unified_gpu_compute/lifecycle_gpu_tests.rs, crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu, crates/visionclaw-domain/src/types/physics_config.rs, src/handlers/settings_handler/types.rs, src/handlers/api_handler/analytics/sssp_handlers.rs]
owner: jjohare
review_trigger: a new device buffer on UnifiedGPUCompute; stage S3 of the physics rework (separate path-length and stiffness buffers), which decides whether SSSP spring adjustment comes back on; any change to relaxation_step_kernel or the SSSP host loop
repo: visionclaw
domain: GPU-wire-abi
evidence_class: test
acceptance_boundary: GPU tests in the development container on one RTX 6000 Ada (sm_89, PCI bus 0xAF), driven through UnifiedGPUCompute exactly as the actors call it, plus compute-sanitizer memcheck over those tests. Not established by this record - a product build, a host smoke run, a deployed server, the actor message path (ShortestPathActor, ForceComputeActor) end to end, or the settings migration against a real production database (tested on in-memory and temporary-file databases).
---

# ADR-2191 — Graph-sized GPU buffers are declared once; SSSP results never outlive their graph; SSSP spring adjustment is off by default

Stage S1a of the physics rework (operator-approved plan, 10 November 2026).

## Context
- Production builds its engines at 1000 nodes x 1000 edges and resizes on the first graph. `resize_buffers` reassigned about 60 buffers one by one and missed eight: the SSSP working set (`dist`, `current_frontier`, `next_frontier_flags`) and the label-propagation set (`labels_current`, `labels_next`, `label_counts`, `label_mapping`, `rand_states`, plus `max_labels`).
- Reproduced on the GPU before the fix: SSSP panicked in cust's equal-length assert for any graph that was not exactly 1000 nodes and poisoned the shared engine mutex; label propagation on 1500 nodes wrote 124 times past `rand_states` (memcheck) and returned 1000 labels; after SSSP at 1000 nodes and growth to 1500, the force pass read the retained 1000-entry distances out of bounds (64 reads, memcheck; clean with the adjustment off); delta-stepping returned infinity for every node at distance >= delta; an out-of-range label made `relabel_communities_kernel` fault.
- SSSP and the force pass share one engine: every actor holds the same `Arc<Mutex<UnifiedGPUCompute>>` from the resource supervisor, so SSSP distances do reach the force pass.
- The L1 panic masked the SSSP faults. Fixing it alone would have made the stale-distance read fire on every graph growth.

## Decision
1. **One table.** Every device buffer whose length follows the node or edge count is declared once in `graph_sized_buffers!` (`buffers.rs`) with its extent (nodes, nodes + 1, edges, 256-thread node blocks) and default fill. The same allocator serves `new` and `resize_buffers`; a growth replaces the whole set, allocated in full before anything is replaced. Buffers that do not follow the graph are listed in `FIXED_EXTENT` with the reason. A CPU test reads the engine struct and fails if a `DeviceBuffer` field is in neither list.
2. **Bounds come from the registry.** SSSP and label propagation check every buffer they index against its registered extent before launching and return an error instead of launching out of bounds. `max_labels` is derived from the registered label capacity.
3. **Derived results die with their graph.** Every graph change (any `resize_buffers` call, including a shrink that keeps the allocation, and every CSR upload) discards the SSSP distances and availability. The force pass receives `d_sssp` only from `sssp_distances_for_force_pass`: current, available and at least `num_nodes` long, otherwise null. There is no fallback to the SSSP working buffer. No kernel signature changes (ADR-2143 I7).
4. **SSSP delta-stepping is correct, not disabled.** The relaxation kernel records every improvement and queues only those inside the bucket; the host moves the bound to the bucket of the nearest unsettled node, skipping empty buckets. All SSSP copies, memsets and launches are ordered on the SSSP stream. A run that reaches the round cap (a negative weight is a negative cycle in the symmetric CSR) is an error, and nothing is kept.
5. **The relabel kernel never indexes with an out-of-range label.** A label >= `num_nodes` becomes -1 (unassigned).
6. **SSSP spring adjustment is off by default** (`SSSP_SPRING_ADJUST_DEFAULT = false`, the one definition, read by both serde defaults and the `/sssp/status` fallback). SSSP reads the springs' stiffness (`edge_weights`) as path length, so a stiffer relation counts as a longer path and receives a longer rest length. The default stays off until stage S3 gives SSSP its own length buffer. A stored `ssspEnabled: true` is honoured. In LinLog mode, the live default, the term has no effect because LinLog attraction has no rest length.

## Consequences
- A new graph-sized buffer must be added to the table or `FIXED_EXTENT`; the registry test fails otherwise.
- SSSP works at every node count, and its distances reach the HTTP response and the force pass correctly.
- A growth briefly holds the old and new buffer sets together (about twice the graph-sized memory).
- A document without `ssspEnabled` now loads with the adjustment off (it loaded on before). Rows already holding `true` are unchanged until the boot-time settings migration (a later stage).
- **Unmasked by this fix (decision needed from the operator).** Before it, a stored `ssspEnabled: true` had no effect at any node count other than exactly 1000, because `run_sssp` panicked before producing distances. Now, once any SSSP request succeeds (`/pathfinding/sssp` or a `ComputeShortestPaths` forward; nothing runs SSSP per frame), the rest-length term is live for every following physics step until the next graph change. It moves nodes only in Hooke mode, which runs when a stored row sets `linLogMode: false` (no layout mode forces it). So the configurations whose layout changes are: stored `ssspEnabled: true` with `linLogMode: false`, after an SSSP request. The live settings fixture (`ssspEnabled: true`, `linLogMode: true`) is not affected. Stored values are applied as stored (ADR-2140 decision 2), so this change does not override them; migrating stored `true` rows to the new default is for the operator to decide (the boot-time settings migration stage).
- Not addressed here: `c_params` is uploaded only by a physics step, so label propagation on an engine that has never stepped casts zero votes and returns singletons (finding L13; production always steps first); context binding for `run_sssp` on a fresh thread (L16); the per-block buffers assume 256-thread blocks while `VISIONCLAW_BLOCK_SIZE` may lower the block size.

## Verification
At `verified_commit` (the fix is `212465a0d`; `9af9b0912` keeps the re-seeded frontier alive until its upload completes), on the RTX 6000 Ada (bus 0xAF) with `CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=2 CUDA_ARCH=89`:
- `cargo test --lib lifecycle_gpu_tests -- --ignored --test-threads=1` under `compute-sanitizer --tool memcheck`: 7 passed, 0 errors. The tests: SSSP after growth and shrink through the shared mutex; label propagation after growth (1500 labels, 7 communities, modularity 0.803 on six components); SSSP against CPU Dijkstra (light, heavy and split graphs at 64 and 1500 nodes, delta and plain, three repeats each); no stale distances after growth; fresh distances change 4279 of 4500 coordinates through the Hooke rest-length term; out-of-range relabel; negative weight is an error.
- The five reproduction tests on `c35d718ba` (the commit that added them) fail as listed in Context; that commit's message summarises the logs. `c1b_fresh_sssp_distances_reach_the_force_pass` and `sssp_with_a_negative_weight_is_an_error` were added with the fix.
- `cargo test --lib`: all pass, including `buffers::registry_tests`. The force-kernel harness: 40 passed, unchanged.
- `scripts/gpu-physics-gate.sh` runs all of the above plus the CI CPU crates, clippy, fmt and this index check.

## Review round 1 (2026-11-10)

An independent review of the first implementation found one regression and five gaps. Decisions added or changed:

7. **`delta` is validated at both ends, and the bucket loop provably terminates.** The regression: with `delta` below half the f32 spacing at the nearest unsettled distance, `nearest + delta == nearest`, the next bucket was empty and the loop spun forever while holding the engine mutex, freezing physics (reproduced: `run_sssp(0, Some(1e-7))` on a 64-node chain of weight 2 did not return in 60 s). Now:
   - HTTP: `delta` that is not finite and greater than zero is a 400 before any graph or GPU is consulted; the engine's weight-relative refusal also answers 400.
   - Engine: `delta` must be finite, positive and at least `SSSP_MIN_DELTA_FRACTION` (1/4096) of the largest edge weight, recorded at CSR upload; negative weights are refused before launching. Refusals keep the distances from the last good run.
   - Loop: every bucketed pass strictly raises the bound or the run falls back to the plain relaxation, seeded with every tentatively labelled unsettled node, so distances are unchanged. At most `SSSP_MAX_DELTA_BUCKETS` (64) buckets run, each costing one n-float device-to-host copy and host scan (at most 2.5 MB at 10,000 nodes). A pass cap and a per-drain round cap (n + 1) turn any violation into an error, never a spin.
8. **A failed growth leaves the engine intact.** Positions and velocities are carried into the fresh buffer set before it is installed.
9. **Graph change clears all node-indexed state, not only SSSP.** Every `resize_buffers` call, and every `initialize_graph` whose counts or CSR topology fingerprint changed, clears the compute mask, community labels and count, degree weights, the hold overlay's rest lengths, SSSP results and `SemanticGpu`. The actor re-uploads each of these after `initialize_graph`. An identical re-upload keeps them, because GraphResourceActor and ForceComputeActor share the engine and both upload each graph.
10. **Stored `ssspEnabled: true` is migrated once** (operator decision, 10 November 2026). `SqliteSettingsRepository::open` runs migration `0003_sssp_spring_adjust_off` in one transaction before any reader. It copies each stored `physics` row (every owner layer) that has `ssspEnabled` (or `sssp_enabled`) true verbatim into `settings_archive`, sets that key alone to false, and logs a warning. The `schema_migrations` marker makes it run once, so turning the setting back on afterwards sticks. No other key, row or table is touched; `physics_profiles` are not migrated. This supersedes decision 6's "a stored `true` is honoured" for rows stored before the migration ran.
11. **The registry test parses structs.** It strips comments, splits fields at top-level commas, resolves crate-wide aliases of device memory, and finds nested holders by fixpoint. Engine fields holding device memory through another type must be listed in `NESTED_DEVICE_STATE`.
12. **The gate cannot pass without the GPU.** With `VISIONCLAW_EXPECT_PCI_BUS` set, a missing driver or device fails the harness and lifecycle tests instead of skipping. The gate fails on any `SKIP` line, on fewer than 12 lifecycle or 40 harness tests passed, or on fewer device lines on the expected bus than that.

**Outside the table, stated plainly:**
- `semantic` (`SemanticGpu`, sized from its own inputs, dropped on graph change, rebuilt by `refresh_semantic`);
- the 15 `FIXED_EXTENT` buffers: grid cells, k-means, flags and scalars, constraint data and index, SSSP's kept distances.

**Still open:**
- The constraint index keeps node ids across a graph change. Constraints belong to the constraint actors, which re-send them on their own schedule.
- A relabelled graph with an identical CSR is indistinguishable from the old one by fingerprint.
- `set_compute_mask` failed for every sparse mask; it now fills the buffer's prefix, but it still has no production caller.
- L13, L16, and `VISIONCLAW_BLOCK_SIZE` below 256, as listed above.

**Verification, round 1:**
- Five more lifecycle GPU tests pass, 12 in all under memcheck in the gate: `sssp_tiny_delta_terminates` (the three measured cases now finish in 1-9 ms with exact distances, and `run_sssp` refuses them), `sssp_rejects_invalid_delta_and_keeps_prior_result`, `sssp_delta_edge_cases_match_dijkstra` (minimum delta on a three-component graph, all-zero and alternating zero/one weights, `1e30` and `f32::MAX`), `failed_resize_leaves_the_engine_intact`, and `graph_changes_discard_node_indexed_state`.
- CPU tests pass: three handler tests (`sssp_delta_validation_tests`), two migration tests, and the scanner fixture.
- On the pre-round code, the same-count rename and the shrink both kept degree weights and community labels.
- `cargo test --lib`: 1980 passed, 0 failed.
