---
id: ADR-2173
title: The force pass reads a per-node constraint index, not every constraint
date: 2026-11-01
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: d5c46cdd86cda5cf376fb95ee0901158be0f97e2
relates: [ADR-2169]
verified_paths: [src/utils/unified_gpu_compute/constraint_index.rs, src/utils/unified_gpu_compute/memory.rs, crates/visionclaw-gpu/src/cuda_sources/visionclaw_unified.cu, src/utils/unified_gpu_compute/execution.rs, src/actors/gpu/force_compute_actor.rs]
owner: jjohare
review_trigger: the constraint count or node count grows 10x, or a kernel signature changes
repo: visionclaw
evidence_class: test
acceptance_boundary: Measured on a synthetic graph of the live shape on an RTX 6000 Ada (GPU 1); establishes bit-identical positions and a 3.0x faster step there. It does not establish the live RTX A6000 rate, which is measured only after redeploy.
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
- The two index pointers reach the kernels as `__device__` globals (`g_constraint_node_offsets`, `g_constraint_node_ids`), written with `get_global`, as `c_params` is. No kernel signature and no `c_params` field changes, so the I7 arity guard still holds.
- Either global null means unbound: the kernel scans every constraint, as before. Every failure path (upload error, PTX without the symbol) unbinds and logs; physics stays correct and only slower.

## Consequences
- Output is bit-identical: positions after 60 steps match the pre-change run in 29,064 of 29,064 floats, and a GPU test compares indexed against unbound full forces under both kernels.
- Held step 29.0 to 9.77 ms and idle 27.9 to 9.16 ms on the synthetic live shape (about 3.0x; repeats within 1%).
- A host copy of the constraints (6 MB at 96,742) is kept to rebuild the index on a node-capacity change, and the index is rebuilt on every constraint upload (host cost only).
- **Cost now follows the longest per-node list.** A node's thread walks its own list serially and the step ends when the slowest warp does. On the synthetic set (longest list 3,539, p99 327) the step is flat at about 9.8 ms up to a longest list of about 14k; then 12.1 ms at 18.5k, 15.4 ms at 23.5k, 40.3 ms at 33.5k and 66.9 ms at 63.5k (GPU 1, `TICK_HUB`). The live maximum is not known; every upload logs `constraint index: ... longest node list N (p99 M)`. A top-level class with tens of thousands of children would need a hub-splitting follow-up.
- The remaining 9.5 ms is repulsion (7 to 9 ms) and springs (about 2.4 ms). Reducing it bit-exactly means a cell-ordered position gather in `force_pass_kernel`; that is a separate decision.

## Verification
- Built `visionclaw_unified.ptx`: `force_pass_kernel` has 30 `.param` entries (the I7 arity), and `.global .align 8 .u64 g_constraint_node_offsets` / `g_constraint_node_ids` are present.
- `cargo test --lib` at the stamped commit: 1774 passed, 0 failed, exit 0.
- `cargo test --lib constraint_index` (host equivalence to the old scan) and `adr_2029_constraint_gpu_tests::the_constraint_index_changes_no_bit_of_the_result` (device globals read back as bound/unbound, results equal).
- `execution.rs` `step_stage_timings_at_the_live_graph_size` (`--ignored --nocapture`, spare GPU): the live-shape graph (9,688 nodes, 147,048 edges, 96,742 constraints), held and idle ms per step, readback, and the per-stage breakdown. `TICK_HUB=n` adds n constraints on node 0, `TICK_CONSTRAINTS=0` removes them, `TICK_MIN_HZ` asserts a floor. The stage breakdown is `StepTiming`'s kernel sub-phases (`VISIONCLAW_STEP_STAGES=1` adds it to the live "step timing" log line); there is no second profiler.
- Re-run after folding the profiler into `StepTiming`: held 9.76 ms, idle 9.18 to 9.35 ms with constraints, identical to the standalone bench it replaced (9.20 idle) on the same GPU.

## Relation to ADR-2169
ADR-2169 recorded the live step at 9 to 10 Hz with 84 to 94 ms in the force kernel and left the cause "not investigated further". This record is that investigation: the cost was every thread scanning every ontology constraint (about 0.94 billion involvement checks per step), not the node count or the card sharing. ADR-2169's step-rate question is answered by this record; the per-client stream rate stays ack-paced (ADR-2141) and is not changed here.
