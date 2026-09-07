---
id: ADR-2007
title: GPUManagerActor is a coordinator over four subsystem supervisors on a context bus
date: 2026-08-31
decision_status: accepted
implementation_status: partial
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 1ad881cab5ed786fc112f6e50db03fd587e23ec0
verified_paths: [src/actors/gpu/gpu_manager_actor.rs, src/actors/gpu/mod.rs, src/actors/gpu/context_bus.rs, docs/GPU-wire-abi.md]
owner: jjohare
review_trigger: a new GPU subsystem that does not fit the four-supervisor split, or a change to SharedGPUContext distribution
repo: visionclaw
domain: BASELINE-architecture
lineage: "No dedicated legacy ADR; distils the in-code 'Phase 7: God Actor Decomposition' refactor, promoted into the BASELINE actor-topology map."
---

# ADR-2007 — GPUManagerActor is a coordinator over four subsystem supervisors on a context bus

## Context

The GPU subsystem was a single God Actor: one failure took down force
computation, analytics and graph analytics together, and every actor held a
central GPU handle. Failures did not isolate and restart policy was uniform.
Lineage: the in-code "Phase 7: God Actor Decomposition" refactor, promoted here
into the BASELINE actor-topology map (no separate legacy ADR number).

## Decision

`GPUManagerActor` is a lightweight coordinator that spawns four subsystem
supervisors — **Resource, Physics, Analytics, GraphAnalytics** — each isolating
its own failures with exponential-backoff restart and reporting health
independently. `SharedGPUContext` is distributed by ResourceSupervisor through direct messages
to registered supervisors, with additional GPUContextBus publication. Shared
context ownership and recovery still couple subsystem behaviour.

## Consequences

- Restart policies are organised per supervisor. Isolation from shared-context
  failure requires runtime evidence; it is not guaranteed by topology alone.
- The analytics kernels the AnalyticsSupervisor carries are code-fixed but not
  all output-validated — legacy ADR-031's "known-broken" list is stale in the
  good direction (Louvain D1 fix, PageRank D8 fix; Landmark-APSP remains
  compile-quarantined). Per-kernel evidence: `docs/GPU-wire-abi.md`
  §"Analytics kernel trust status". Residual work is output validation
  (benchmarks), not disablement; the topology is sound independent of any
  kernel's output quality.
- One more indirection layer (bus + supervisors) to trace for a GPU call versus
  the old single actor.

## Verification

`src/actors/gpu/gpu_manager_actor.rs` spawns `ResourceSupervisor`,
`PhysicsSupervisor`, `AnalyticsSupervisor` and `GraphAnalyticsSupervisor`.
`src/actors/gpu/mod.rs` documents the "Phase 7: God Actor Decomposition"
topology and the `SharedGPUContext via GPUContextBus broadcast` distribution.
`src/actors/gpu/context_bus.rs` defines `GPUContextBus` with `publish(...)` over
`Arc<SharedGPUContext>`. Verified at `e0f8cd896`.

## Closeout extension — 2026-09-04

CP-01/06/08. Owner remains jjohare with GPU/actor/operations maintainers. Implementation is partial against broadcast-only context distribution and guaranteed isolation; historical live activation is retained. The manager creates four supervisors and registers their addresses with ResourceSupervisor. That supervisor sends context directly, then publishes to the bus. Several send results are discarded before pending graph data is cleared. Physics restarts its sibling group around shared state; address replacement alone does not prove termination or clean device recovery.

**Acceptance condition:** Bind context/graph generations to acknowledged child readiness; reconcile failed sends and late/restarted subscribers. Inject child/supervisor/mailbox/device failures and verify termination, current-state recovery, bounded backoff and the declared isolation boundary. Reopen on context ownership, restart policy or new subsystems. See [supervision review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#gpu-supervision-and-context-delivery) and [source receipt](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/evidence/crate-supervision-snapshot.json). No actor crash or GPU recovery was exercised.

## Re-verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**Governed changes since `eac011303`:** `src/actors/gpu/gpu_manager_actor.rs`
(+74) and `docs/GPU-wire-abi.md` (+148/-38). `src/actors/gpu/mod.rs` and
`src/actors/gpu/context_bus.rs` are **unchanged**.

**The four-supervisor topology is intact.** `src/actors/gpu/gpu_manager_actor.rs`
still spawns all four: `PhysicsSupervisor::new().start()` (`:91`),
`AnalyticsSupervisor::new().start()` (`:94`),
`GraphAnalyticsSupervisor::new().start()` (`:97`) and
`ResourceSupervisor::new().start()` (`:101`, spawned last and then configured
with the others' addresses via `SetSubsystemSupervisors`, imported at `:31`).
The coordinator holds the four `Addr<…>` at `:44-47`, and the module header
documents the split at `:8-11`. `src/actors/gpu/mod.rs:3` still carries the
"Phase 7: God Actor Decomposition" heading and `:33` the "Receives
SharedGPUContext via GPUContextBus broadcast" line. `context_bus.rs` still
defines `GPUContextBus` (`:66`) with `publish(&self, context:
Arc<SharedGPUContext>) -> usize` (`:92`) and `publish_with_device` (`:97`).

**The change strengthens this record rather than eroding it.** The +74 lines are
ADR-2053: `GPUManagerActor` gained a `Handler<SetNodeSSSP>` that forwards to
`GraphAnalyticsSupervisor`, plus a `forward_to_graph_analytics!` macro routing
the `/api/analytics/*` pathfinding messages to the supervised children. The
reason matters for this ADR's central claim: `src/app_state.rs` had been starting
a **second, standalone** `ShortestPathActor`/`ConnectedComponentsActor` pair
outside the supervisor tree, and because `ResourceSupervisor` distributes
`SharedGPUContext` only to registered subsystem supervisors, that pair never
received a context and always took the GPU-absent branch — every analytics
pathfinding route addressed a GPU-blind actor. Removing it means the coordinator
is now genuinely the sole entry point to the GPU subsystem, which is what this
record asserts. That bypass existed at the previous `verified_commit`; it does
not now.

**Consequences cross-reference still resolves.** `docs/GPU-wire-abi.md` grew, but
§"Analytics kernel trust status" is still present at `:158`, with Louvain FIXED
(`:164`), PageRank FIXED (`:166`), the outstanding output-validation caveat at
`:177` and the summary invariant at `:204`. The Consequences bullet's claim —
kernels code-fixed but not output-validated — is unchanged.

**Status stays `partial`:** failure isolation is still argued from topology, not
demonstrated by a runtime fault-injection run, and no GPU executed in this pass.

**Commands run:** `git diff --stat eac011303..HEAD -- <verified_paths>`;
`git diff` on `gpu_manager_actor.rs` (full patch read); `grep -n` over
`gpu_manager_actor.rs` for the four supervisor types, over `mod.rs` for
`Phase 7|GPUContextBus|SharedGPUContext`, over `context_bus.rs` for
`GPUContextBus|publish`, and over `docs/GPU-wire-abi.md` for the kernel-trust
section; `git diff` on `src/app_state.rs` for the removed standalone pair.

## Landing re-verification — 2026-09-06 (2cf222406)

Governed paths changed in the Wave 3 landing commit: docs/GPU-wire-abi.md: the kernel trust table became a measured per-kernel table (ADR-2061: PageRank, DBSCAN, Louvain trusted; LOF broken, cause localised); gpu_manager_actor.rs untouched. Decision unaffected; `verified_commit` moved to the landing commit. Gates at that commit: cargo check --workspace --all-targets exit 0, 827 crate + 1600 root + 309 xr-client tests, vitest 809, fmt and lint clean.

## Landing re-verification — 2026-09-06 (c9734a524)

Governed paths changed in the doc-sync commit: docs/GPU-wire-abi.md — frontmatter `version`/`verified_commit` bump and changelog entry for the Remediation — 2026-09-05 section, plus narrative corrections; no code or citation this record depends on changed. `verified_commit` moved to the doc-sync commit.


## Source closeout verification — 2026-09-07

The governed ABI document now reports the corrected LOF fixture. Coordinator/supervisor source is unchanged by this commit; the existing coordinator ownership decision is unaffected by the analytics numerical fix.

Verified implementation: `1ad881cab5ed786fc112f6e50db03fd587e23ec0`. Evidence: [VisionClaw execution report](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/closeout/2026-09-07-execution-visionclaw.md). The embedded-pod library suite passed 1,364 tests (six ignored); a subsequent focused three-test handshake suite also passes. Source verification does not assert deployment activation. Earlier dated observations remain historical.
