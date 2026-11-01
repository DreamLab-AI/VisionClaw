---
id: ADR-2173
title: The force pass reads a per-node constraint index, not every constraint
date: 2026-11-01
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 8f796ad8645a6dca5303150273b36de677e1ab70
relates: [ADR-2169]
verified_paths: [src/utils/unified_gpu_compute/constraint_index.rs, src/utils/unified_gpu_compute/memory.rs, crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu, src/utils/unified_gpu_compute/execution.rs, src/actors/gpu/force_compute_actor.rs]
owner: jjohare
review_trigger: the constraint count or node count grows 10x, or a kernel signature changes
repo: visionclaw
evidence_class: test
acceptance_boundary: Measured on a synthetic graph of the live shape on an RTX 6000 Ada (GPU 1); establishes bit-identical positions and a 3.0x faster step there. It does not establish the live RTX A6000 rate, which is measured only after redeploy, and the hub slope (flat to about 14k entries, then steeper) was measured on the Ada's 96 MB L2: the A6000 has 6 MB, so its knee is expected to be lower.
---

# ADR-2173 — The force pass reads a per-node constraint index, not every constraint

## Context
- The live dev backend stepped physics at 8 to 12 Hz. Its own step-timing log put 84 of 95 ms in `execute`; readback was 0.3 ms and the tick interval is 16 ms (0 while a node is held).
- `force_pass_kernel` and `force_pass_with_stability_kernel` found a node's constraints by having every thread scan all `num_constraints` and test membership: 9,688 nodes x 96,742 constraints, about 0.94 billion checks per tick.
- On a synthetic graph of the live shape (9,688 nodes, 147,048 edges, 96,742 constraints) that scan is 19 of 29 ms per step; without constraints the step is 10 ms.
- Codescape consumes this PTX and refuses it unless the kernel arities match (ADR-2143 I7: `force_pass_kernel` is 30).

## Decision
- The host builds a CSR from node to constraint ids (`constraint_index.rs`) on every `set_constraints`, `clear_constraints` and node-capacity `resize_buffers`. A node lists a constraint exactly when the old test matched (`node_idx[k] == n`, `k < min(count, 4)`), once per constraint, ascending, which is the old accumulation order.
- Both force kernels loop over the node's own list. The loop body is untouched.
- The index reaches the kernels as one `__device__` global, `g_constraint_index`, a `{offsets, ids}` pair written with `get_global` in a single 16-byte copy, as `c_params` is. The two pointers are bound together or not at all. No kernel signature and no `c_params` field changes, so the I7 arity guard still holds.
- A null `offsets` means no constraints: the loop runs zero times. The kernels contain no unindexed walk, and the unlaunched `blend_semantic_physics_forces` kernel, a second flat walk, is deleted. A failed index upload, or a PTX without the symbol, is an error returned to the caller (fail loud), and `set_constraints` stages the new constraint buffer and index, writes the global, and only then swaps them in, so a failure leaves the previous constraints, index and binding consistent. A failed rebuild after a node-capacity resize drops the constraints (none counted, index unbound) and returns the error.

## Consequences
- Output is bit-identical: positions after 60 steps match the pre-change run in 29,064 of 29,064 floats, and a GPU test compares the production index against an all-constraints index (every node lists every constraint, which reproduces the old flat scan exactly and exists only in the test) under both kernels, with every node in motion and annealing jitter on so the stability kernel reaches its constraint loop, and reads the per-node constraint force back to prove it did. A second test shows an unbound index with constraints resident acts as no constraints.
- Held step 29.0 to 9.77 ms and idle 27.9 to 9.16 ms on the synthetic live shape (about 3.0x; repeats within 1%).
- **The hub knee is an upper bound for the live card.** The ~14k flat-to-knee figure and the slope table below were measured on an RTX 6000 Ada with a 96 MB L2. The live RTX A6000 has a 6 MB L2, so each list entry costs more there and its knee is expected to be lower.
- A host copy of the constraints (6 MB at 96,742) is kept to rebuild the index on a node-capacity change, and the index is rebuilt on every constraint upload (host cost only).
- **Cost now follows the longest per-node list.** A node's thread walks its own list serially and the step ends when the slowest warp does. On the synthetic set (longest list 3,539, p99 327) the step is flat at about 9.8 ms up to a longest list of about 14k; then 12.1 ms at 18.5k, 15.4 ms at 23.5k, 40.3 ms at 33.5k and 66.9 ms at 63.5k (GPU 1, `TICK_HUB`). The live maximum is not known; every upload logs `constraint index: ... longest node list N (p99 M)`. A top-level class with tens of thousands of children would need a hub-splitting follow-up.
- The remaining 9.5 ms is repulsion (7 to 9 ms) and springs (about 2.4 ms). Reducing it bit-exactly means a cell-ordered position gather in `force_pass_kernel`; that is a separate decision.

## Verification
- Built `visionclaw_unified.ptx`: `force_pass_kernel` has 30 `.param` entries (the I7 arity), `force_pass_with_stability_kernel` 29, `c_params[212]`, and `g_constraint_index` is present (`codescape_abi_tests`, which reads the PTX).
- `cargo test --lib` at the stamped commit: 1791 passed, 0 failed, 11 ignored, exit 0.
- `cargo test --lib constraint_index` (host equivalence to the old scan) and `adr_2029_constraint_gpu_tests::the_constraint_index_changes_no_bit_of_the_result`, `an_unbound_index_means_no_constraints` and `the_index_view_is_bound_whole_and_follows_the_buffers`.
- `execution.rs` `step_stage_timings_at_the_live_graph_size` (`--ignored --nocapture`, spare GPU): the live-shape graph (9,688 nodes, 147,048 edges, 96,742 constraints), held and idle ms per step, readback, and the per-stage breakdown. `TICK_HUB=n` adds n constraints on node 0, `TICK_CONSTRAINTS=0` removes them, `TICK_MIN_HZ` asserts a floor. The stage breakdown is `StepTiming`'s kernel sub-phases (`VISIONCLAW_STEP_STAGES=1` adds it to the live "step timing" log line); there is no second profiler.
- Re-run after folding the profiler into `StepTiming`: held 9.76 ms, idle 9.18 to 9.35 ms with constraints, identical to the standalone bench it replaced (9.20 idle) on the same GPU.

## Relation to ADR-2169
ADR-2169 recorded the live step at 9 to 10 Hz with 84 to 94 ms in the force kernel and left the cause "not investigated further". This record is that investigation: the cost was every thread scanning every ontology constraint (about 0.94 billion involvement checks per step), not the node count or the card sharing. ADR-2169's step-rate question is answered by this record; the per-client stream rate stays ack-paced (ADR-2141) and is not changed here.

## Re-stamp — 2026-11-01 (integration of `phys/tick-rate` and `phys/hold-springs`, `8f796ad86`)

Re-verified after the integration with `phys/hold-springs` and the removal of the unindexed walk: the PTX has `force_pass_kernel` at 30 parameters, `force_pass_with_stability_kernel` at 29, `c_params[212]`, `g_constraint_index`, and no `blend_semantic_physics_forces`. `cargo test --lib`: 1791 passed, 0 failed, 11 ignored (exit 0). The live-shape timing test (GPU 1): held 9.80 ms, idle 9.18 ms (a second run: idle 9.20 ms), longest list 3,539, p99 327, unchanged from the stamped numbers. Re-run: `git diff --stat 531df5ef7 8f796ad86 -- src/utils/unified_gpu_compute/constraint_index.rs src/utils/unified_gpu_compute/memory.rs crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu src/utils/unified_gpu_compute/execution.rs src/actors/gpu/force_compute_actor.rs` → `5 files changed, 2071 insertions(+), 187 deletions(-)`.
