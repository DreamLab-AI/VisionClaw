# ADR-070: CUDA Integration Hardening

**Status:** Implementing
**Date:** 2026-05-01
**Implementation:** P0 items D1.1-D1.5 implemented — SimParams::validate_for_gpu(), NaN guard kernel, per-kernel timing header, constraint buffer cap (50k), build.rs PTX regex fix + version mismatch detection
**Deciders:** jjohare, VisionClaw GPU team
**Supersedes:** None
**Extends:** ADR-061 (Binary Protocol), ADR-031 (Network Backpressure), ADR-069 (Force Preset System)
**Implements:** PRD-005 §13 (GPU Implementation Notes), §19 R-19/R-23
**Threat-modelled:** PRD-005 §19 (R-19 NaN, R-23 force explosion, F-29 mask oscillation, F-30 mid-tick edge race, F-09 GPU NaN broadcast)

## Context

A focused review of VC's CUDA integration (10 kernels, ~200 KB CUDA source, full Rust actor glue) identified the substrate as **mature for the typed-graph and ontology workloads of PRD-005** — but with concrete hardening gaps. Specifically:

**Strengths confirmed:**
- Spatial grid + CSR for O(N) n-body — handles 30k–100k nodes.
- Ontology constraints with 5 OWL kernels (DisjointWith, SubClassOf, SameAs, InverseOf, Functional).
- `DynamicRelationshipBuffer` + `SemanticTypeRegistry` enable runtime type registration without kernel recompilation.
- Stability gating (kinetic-energy threshold) skips force computation when the system is at rest, saving ~60% GPU at idle.
- Persistent buffers in stability detection avoid 300 cudaMalloc/free per second.
- Token-bucket backpressure (5 Hz refill, ~1.9 MB/s for 13k nodes × 28 B).
- Periodic full broadcast every 300 iters (existing fix for late-connecting clients).

**Gaps requiring hardening:**

1. **R-19 / F-09 — No NaN output detection.** Forces and positions can become non-finite (rest_length=0, divide-by-zero in normalize). Bad positions broadcast to all clients; client GPU may NaN-cascade through edge geometry; possible browser tab crash on Apple GPUs.
2. **No SimParams validation in Rust before GPU upload.** Negative `dt`, `damping` ∉ (0,1), `spring_k < 0`, `gravity` magnitude > 100 all accepted silently.
3. **No per-kernel timing telemetry.** Only `last_step_duration_ms` for the entire tick. Cannot diagnose which kernel is slow.
4. **No sparse compute mask.** All nodes always evaluated — Epic E.4 (persona masking) currently render-only because GPU has no path to skip computation for hidden nodes. A 30k-block vault under non-technical persona still costs 30k-block compute even though only ~1k file-level nodes need rendering.
5. **Semantic config updates not atomic w.r.t. force kernel.** Mid-tick relationship-buffer rebuild can cause kernel to read partial buffer state.
6. **No coordinate-scale calibration step.** Matryca's vis-network constants applied verbatim would explode forces by 10²× in VC's metre-scale world (R-23 / F-28).
7. **No SHACL kernel** (Epic G need; OWL kernels only).
8. **Z-damping is misnamed** — the field actually dampens the X-axis for dual-graph offset; PRD I.6 temporal-Z proposal needs clean naming.
9. **PTX version downgrade regex is panic-fragile** in build.rs (`pos + 13.min(ptx_text.len() - pos)` may panic if PTX file ends early).
10. **No CUDA version mismatch detection** (nvcc vs. driver).
11. **No constraint-force component in stability detection** — system can be KE-stable but still pinned by semantic constraints; convergence detector reports stable when it isn't.
12. **No memory-pressure adaptation on consumer GPUs** (RTX 3060 12 GB shared with 3D rendering).

## Decision

**Adopt a four-tier hardening plan: P0 launch-safety quick wins, P1 observability, P2 sparse-compute and atomic config, P3 big-projects deferred to follow-up.**

### D1 — P0: Launch-safety quick wins (≤1 week each, MUST land before Phase 0)

These are blockers for any new GPU-touching Epic.

#### D1.1 — `SimParams::validate()` (Rust)

Implemented in `crates/graph-cognition-physics-presets`. Called by `ForceComputeActor` before every `cudaMemcpyToSymbol(c_params, ...)`. Rejects:

- `dt ∉ [0.001, 0.1]`
- `damping ∉ (0.0, 1.0)`
- `spring_k < 0`, `repel_k < 0`
- `max_force ≤ 0`, `max_velocity ≤ 0`
- Any field non-finite (NaN / ±Inf).
- `gravity` magnitude > 100 (sanity ceiling).
- For each registered edge kind: `rest_length > 0`, `spring_k ≥ 0`.

On rejection: GPU upload is skipped, previous valid SimParams stay in place, audit event `physics.invalid_simparams` fired, UI banner shown to user. **Closes R-19 partial.**

#### D1.2 — Per-iteration NaN guard kernel

Every 32 iterations, a small reduction kernel scans GPU output for any non-finite position or velocity. On detection:

- Halt physics integration.
- Restore last-known-good positions from a 2-frame ring buffer (cheap; positions are 12 B/node × 30k = 360 KB).
- Fire metric `physics_nan_detected{iter, nodes_affected}`.
- Emit user banner: "Physics encountered numerical instability; layout reverted."
- Force preset → `Default` until user re-selects.

**Closes R-19 fully.**

#### D1.3 — Per-kernel CUDA event timing

Wrap each kernel launch with `cudaEventRecord(start)` and `cudaEventRecord(stop)`; query elapsed via `cudaEventElapsedTime`. Export via `PhysicsStats` telemetry into existing observability pipeline.

Resulting metrics:
- `physics_kernel_duration_seconds_bucket{kernel="force_pass|integration|grid_build|stability_check|constraint_apply|..."}` (histogram).
- `physics_kernel_invocations_total{kernel}` (counter).
- `physics_kernel_failures_total{kernel, cause}` (counter).

Overhead: ~1 µs per event, negligible at 60 Hz.

#### D1.4 — Constraint buffer size assertion

In `OntologyConstraintActor`:
- Hard cap of 50,000 constraints; configurable via setting `physics.max_ontology_constraints`.
- Reject construction if cap exceeded; mark surplus axioms `truncated=true` and surface in dashboard.

Prevents OOM kernel launch.

#### D1.5 — Build.rs robustness

- Replace fragile PTX-version regex substring with safe `re.replace_all`.
- On missing `CUDA_PATH`/`CUDA_HOME` and `/opt/cuda` absent: fail with clear message rather than silent default.
- Add `nvcc --version` vs. `nvidia-smi --query-gpu=driver_version` mismatch detection at build time; warn loudly if PTX may not JIT.

### D2 — P1: Observability and atomicity (1–4 weeks each, before Phase 1)

#### D2.1 — Hot-reload versioning for semantic config

`SemanticTypeRegistry` increments a `buffer_version: u64` on every update. The relationship-buffer constant memory carries this version. Force kernel reads version at launch; if mid-launch the version changed, kernel re-reads buffer (or, more conservatively, the actor delays kernel launch until current version is fully committed).

Implementation choice: **delay-launch** is simpler and avoids GPU-side spinwait. Actor uses a `Mutex<RegistryUpdate>` that physics tick acquires before launch.

**Closes F-30 (mid-tick edge insertion race) at the registry layer.**

#### D2.2 — Stability detection 3rd criterion

Existing stability check uses kinetic energy + active-node count. Extend to include **constraint-force magnitude**: if any constraint's force on its target node exceeds a per-preset epsilon, the system is **not** stable regardless of KE.

This prevents false-stable detection for systems pinned by hierarchy or disjoint constraints.

#### D2.3 — NaN guard for input edges

`OntologyConstraintActor` rejects constraint upload if any constraint references a node with non-finite position. Prevents NaN entering the kernel from upstream actors.

#### D2.4 — Coordinate-scale calibration step

Per ADR-069 D3: every preset undergoes a calibration test against a canonical 100-node graph fixture. Auto-scaling factor recorded in the preset TOML. CI fails if a new preset's post-1000-iter AABB exceeds ±1,000 or peak KE exceeds 100× initial.

**Closes R-23 / F-28 (force explosion).**

### D3 — P2: Sparse compute and config-update atomicity (Phase 2 deliverable)

#### D3.1 — Sparse compute mask kernel

A new compaction kernel produces a `compute_mask: Vec<u32>` (indices of nodes to evaluate this tick). Force-pass kernel reads only these indices. Persona switching, edge-category filter, and large-vault block-explosion all benefit.

Cost: ~100 lines CUDA + Rust glue. Expected speedup for narrow personas: 5–10×.

**Closes Epic E.4 implementation gap (`filtered_indices` referenced but not present).**

#### D3.2 — Atomic semantic-config swap

Builds on D2.1 but adds GPU-side: relationship buffer is double-buffered. `SemanticTypeRegistry::commit_update` writes new buffer to the inactive slot then flips a single u32 atomic `active_buffer_idx`. Kernel reads `active_buffer_idx` once at launch, uses that buffer for the entire tick.

True atomic swap, zero stalls.

#### D3.3 — z-damping rename

`SimParams::z_damping` is renamed to `x_axis_damping_for_dual_graph`. The field semantics (dampens X for dual-population offset) is documented in the rustdoc. New `z_axis_temporal_pin_strength` field added per ADR-069 D11 for the temporal Z-axis feature; the two are orthogonal.

### D4 — P3: Big projects (deferred to follow-up PRDs)

The following are **not in PRD-005 scope** but recorded here for visibility and future ADR splitting.

| Item | Effort | Follow-up PRD |
|------|--------|---------------|
| GPU-native bidirectional APSP for huge graphs | 1 quarter | TBD (sub of E.1 acceleration) |
| SHACL constraint kernel (graph-pattern matching on GPU) | 1 quarter | extension of Epic G |
| Hierarchical LOD clustering for >100k nodes | 1 quarter | future scaling PRD |
| Multi-GPU collective communication (NCCL or custom) | 1 quarter | future scaling PRD |
| WebGPU compute shader fallback for browser | 6 weeks | future client PRD |
| Apple Silicon Metal Compute backend | 6 weeks | future portability PRD |
| Memory-pressure adaptation (eviction policy) on consumer GPUs | 4 weeks | future scaling PRD |

### D5 — Test infrastructure (cross-cutting)

- **GPU-determinism CI gate** (per PRD §19.3 Quality Gate addition): a fixture graph runs through the full pipeline on the reference RTX A6000; output position vector hashed; bit-stable across runs at fixed seed and SimParams. Passes if hash matches checked-in golden value.
- **Multi-GPU correctness gate**: same fixture runs on RTX A6000 and Quadro RTX 6000 (different SM count); positions converge to within tolerance ε.
- **Kernel fuzz**: `cargo-fuzz` corpus of synthesised GraphData; each input runs ≤30s; no crash, no NaN escape, no OOM.

### D6 — Documentation deliverables

- `docs/cuda-integration-overview.md`: kernel inventory, ABI specifications, buffer-layout invariants, coordinate-system conventions.
- `docs/cuda-debugging-runbook.md`: how to read `compute-sanitizer` output, common kernel-failure causes, NaN-trace procedure.
- Inline rustdoc on every `SimParams` field clarifying units (m vs. px), expected ranges, and downstream kernel consumers.

## Consequences

### Positive

- Launch-safety closures: NaN cannot escape to clients; bad SimParams cannot reach GPU.
- Per-kernel observability unblocks performance tuning for Epic I (force-preset calibration) and Epic H (block explosion).
- Sparse compute mask makes Epic E.4 actually performant (currently render-only).
- Coordinate-scale calibration prevents the most damaging Epic I failure mode.
- Atomic config swap eliminates a known race-condition class.

### Negative

- ~32-iter NaN scan adds ~1-2% GPU overhead; acceptable.
- Calibration step adds CI time per preset (≈30 s each); acceptable at one-time cost per preset.
- z-damping rename is a breaking change for any external code reading SimParams; project-internal only, low blast.

### Risks

- Sparse compute mask interacts non-trivially with force coherence: removing a node from compute changes the force field experienced by *visible* nodes. Mitigation: always include 1-hop neighbors of each visible node in the compute mask, even if hidden.
- D3.2 double-buffered atomic swap doubles relationship-buffer GPU memory cost (~16 KB per buffer × 256 entries = trivial).
- Multi-GPU correctness gate may not be portable to user RTX 4080 single-GPU dev workstations; CI-only.

## References

- PRD-005 §13 (GPU Implementation Notes), §19 (R-19, R-23, F-09, F-29, F-30)
- ADR-061 (Binary Protocol — preserved invariants), ADR-031 (Network Backpressure), ADR-069 (Force Preset System)
- Source files cited: `src/utils/visionflow_unified.cu`, `src/utils/visionflow_unified_stability.cu`, `src/utils/semantic_forces.cu`, `src/utils/ontology_constraints.cu`, `src/actors/gpu/force_compute_actor.rs`, `src/actors/gpu/semantic_forces_actor.rs`, `src/actors/gpu/ontology_constraint_actor.rs`, `src/services/semantic_type_registry.rs`, `src/models/simulation_params.rs`, `build.rs`
