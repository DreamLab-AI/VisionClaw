---
id: ADR-2054
title: Delete the dead GPU, analytics and WebSocket code paths
date: 2026-09-05
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: 3b3ee7779f37c5cc9a7ec30b7928428e78e8f87a
verified_paths: [src/gpu/mod.rs, src/actors/gpu/connected_components_actor.rs, src/handlers/socket_flow_handler/mod.rs, src/handlers/socket_flow_handler/actor_messages.rs, src/handlers/socket_flow_handler/types.rs, tests/gpu_safety_tests.rs]
owner: jjohare
review_trigger: Any reintroduction of a message type, module or kernel with no caller at merge time
repo: visionclaw
---

# ADR-2054 — Delete the dead GPU, analytics and WebSocket code paths

## Context

The Phase 1 diagram sweep verified reachability for every actor, message and module in the
GPU and wire domain, and found several that could not execute. Dead code in this subsystem is
worse than clutter: it appears in the supervision diagrams, it is cited by governing docs, and
it makes a reader believe a capability exists. Each item below was confirmed unreachable by
grep before deletion, not merely assumed.

- `StreamingPipeline` (`src/gpu/streaming_pipeline.rs`): exported from `gpu/mod.rs`, never
  instantiated anywhere (VC-13).
- `UpdateComponentEdges`: zero senders tree-wide — only the type definition, its re-export and
  the handler existed. It was the sole writer of `ConnectedComponentsActor::cached_edges`, so
  the CPU fallback that reads that field could only ever return all-singleton components
  (VC-15.8).
- `VisualAnalyticsGPU`, `VisualAnalyticsEngine`, `TSNode`, `TSEdge`: re-exported from
  `gpu/mod.rs`, no caller of `new()` anywhere in `src` (VC-15.12).
- `PushDirective` / `HeartbeatDirective` queueing (ADR-031 item 4): directives were queued into
  `pending_directives`, but `send_pong` / `get_pending_directives` were never called on the live
  ping/pong path — dead on arrival (VC-13.5).
- The `approximate_apsp_kernel` body in `gpu_landmark_apsp.cu`: `#if 0`-guarded and explicitly
  labelled "quarantined (NFR-7)" (VC-15.6).
- `UnifiedGPUCompute::apsp_module`: declared, built from PTX at construction and stored, with
  **zero read sites** — no kernel was ever resolved from it. Found while removing the kernel
  above, not by the Phase 1 sweep.

## Decision

Dead code is deleted, not stubbed, not feature-gated, not left with a "future use" comment.
Each deletion site carries a one-line `REMOVED (ADR-2054):` comment recording what was there
and why it went, so the next reader does not rediscover the same absence as a gap.

Two things are deliberately **kept**, and the distinction is the substance of this decision:

The `ComputeAPSP` handler is retained even though its kernel is gone. It returns an explicit
NFR-7 refusal naming the O(n²) memory reason. An endpoint that refuses clearly is not dead
code — deleting it would turn a documented, intelligible error into a 404 and lose the
explanation.

The `ComputeShortestPaths` and `ComputeConnectedComponents` handlers on `GPUManagerActor` and
`GraphAnalyticsSupervisor` are retained. Phase 1 assessed them as unreachable, and in the
tree as found they were — but ADR-2053 makes them the live entry point for the
`/pathfinding/*` routes once those are retargeted off the standalone actors. Deleting code
that is about to become load-bearing was caught during implementation and reversed.

## Consequences

`ConnectedComponentsActor`'s CPU fallback no longer has a `cached_edges` source. That is
correct rather than a regression: with `UpdateComponentEdges` having no senders, the fallback
was already guaranteed to produce all-singleton components, so it was returning a confidently
wrong answer. ADR-2053's wiring of the GPU path is what makes connected components work.

Removing `StreamingPipeline` narrows `gpu/mod.rs`'s public surface. Nothing consumed it, so no
consumer breaks; if a streaming pipeline is wanted later it should be designed against the
current broadcast path (which, per BROADCAST-001, is a full-snapshot rate-limiter and
visibility cull, not a delta stream) rather than resurrected from this code.

The APSP `.ptx` artefacts are **retained**: the same module also carries
`select_landmarks_kernel` and `stress_majorization_barneshut_kernel`, so it is not an artefact
for the removed kernel alone.

Removing the `apsp_module` load deletes a startup PTX parse and, more importantly, an error
path that logged "GPU DEGRADED: GPU APSP is DISABLED for this process" — alarming operators
about the loss of a capability NFR-7 forbids permanently and that no code could have used. The
`apsp_ptx` parameter is retained in the constructor signature and explicitly ignored, so
callers keep their shape and a future APSP revival has an obvious seam.

## Verification

`grep -rn "REMOVED (ADR-2054)" src/ crates/` — six deletion sites, each with a recorded reason:
`connected_components_actor.rs:276` (`Handler<UpdateComponentEdges>`),
`socket_flow_handler/mod.rs:14` and `actor_messages.rs:230` (`PushDirective` + handler),
`socket_flow_handler/types.rs:219` (`queue_directive`) and `:603`
(`get_pending_directives` + the `pending_directives` field),
`gpu/mod.rs:34` (`TSNode`, `TSEdge`, `VisualAnalyticsGPU`, `VisualAnalyticsEngine`).

`ls src/gpu/streaming_pipeline.rs` — absent.
`grep -rn "UpdateComponentEdges" src/ --include=*.rs` — only the removal comments remain.
`grep -rn "#if 0" crates/visionclaw-gpu/src/cuda_sources/gpu_landmark_apsp.cu` — the guarded
kernel body is gone; only the explanatory header comment remains.

Each item was re-grepped for senders/callers immediately before deletion rather than trusting
the Phase 1 assessment — which is how the `ComputeShortestPaths` reversal above was caught.

`cargo check -p visionclaw-server` — **exit 0, zero errors**, with every Phase 2 change in the
tree. (An earlier run in this phase was blocked by concurrent breakage in three files owned by
other leads; those were fixed by their owners and the check was re-run clean.)

Verification ran on the uncommitted working tree above the recorded SHA; `verified_paths` is
empty for that reason.

## Verification — 2026-09-05 at b0bc275f6501aae7751b85a72ce15fe1e730e7e8


**Range note.** `bed6b617d..b0bc275f6` is `cargo fmt --all` plus the test-side
fixes that made `--all-targets` build; **no production logic changed**. Verified,
not assumed: comparing every changed file with all whitespace stripped leaves
only rustfmt artefacts — struct-literal reflow, import/module reordering and
added trailing commas. The largest single case,
`src/models/simulation_params.rs` (+303/-70 raw), is the `SIMPARAMS_MANIFEST`
literal reflowed one-field-per-line: its field names and byte offsets hash
identically on both sides. Citations below are
therefore re-derived line numbers over unchanged code, not new findings.

**The `complete` status above was premature, and the reason is instructive.**
The Verification block's greps were scoped to `src/` and `crates/`, and its build
evidence was `cargo check -p visionclaw-server` — which compiles the library and
binary but **not** the test targets. The deletions therefore left three root-crate
test targets uncompilable, and nothing in this record's own verification could
have detected it.

Concretely, at the recorded `b00c28a0d` and still at `bed6b617d`,
`tests/gpu_safety_tests.rs` opened with
`use visionclaw_server::gpu::streaming_pipeline::{…}` (`:9`) and imported
`TSEdge, TSNode, … VisualAnalyticsGPU` (`:13`) — every one of them deleted by this
ADR — and exercised `TSNode::new()` at `:525`, `:538`, `:543`, `:548`. A dead type
has no callers *in `src/`* precisely because its remaining callers are tests; the
grep that proves the deletion is safe is the same grep that hides the breakage.

**Now closed at `b0bc275f6`** (*fix(tests): make the sprint build with
--all-targets; cargo fmt the workspace*), which trimmed
`tests/gpu_safety_tests.rs` by 199 lines: the import block and the
`TSNode`/`TSEdge` cases were removed with `REMOVED (ADR-2054)` markers matching
the convention used in `src/` (`:9-10`, `:337-339`, `:410-411`), and
`test_render_data_validation` was reparented into a module renamed to match what
it actually exercises (`RenderData`, `:347`). The marker set is now consistent
across both trees:
`grep -rn "REMOVED (ADR-2054)" src/ crates/ tests/` → the six original `src/`
sites plus a seventh in `src/utils/unified_gpu_compute/construction.rs` (the APSP
module load) and three in `tests/gpu_safety_tests.rs`.

**The build evidence for this record is now `cargo check --workspace
--all-targets` → exit 0** (5m32s at this commit, warnings only). That command,
not `cargo check -p <crate>`, is the standing requirement for any ADR whose
implementation is a deletion: the `review_trigger` above ("any reintroduction of
a message type, module or kernel with no caller at merge time") is only
answerable if test targets are compiled too.

`verified_paths` is populated for the first time — the record was
`verified_paths: []` because the original verification ran on an uncommitted
tree. It now names the deletion sites and, deliberately,
`tests/gpu_safety_tests.rs`, so that a future change re-coupling a test to a
deleted type re-stales this record instead of passing silently.

`implementation_status` stays `complete`: the deletions themselves were correct
and are unchanged — what was incomplete was the *verification*, which is what
this section repairs.

## Re-verification — 2026-10-07 at b6fbe772d (XR beat clock, memory bursts, attention heat; ADR-2134)

**Governed change:** in `src/handlers/socket_flow_handler/`, `mod.rs` registers `session_relay`, and `types.rs` adds the relay throttles, a one-shot unauthenticated-relay flag and `serverTime` on the pong. In `actor_messages.rs`, `SetClientId` now forwards an upgrade-time pubkey to the coordinator. **Decision unaffected.** No code removed by ADR-2054 comes back, and every `REMOVED (ADR-2054):` marker is intact (grep). The new fields are read on the live relay path, with unit and probe-actor tests. Verified with `cargo check --lib --tests` and the 11 relay/coordinator tests.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `src/gpu/mod.rs`: the deprecated, unused `dynamic_buffer_manager` module deleted; `src/handlers/socket_flow_handler/actor_messages.rs`: the V3 encoder call passes `NodeClassIds::default()` for the five empty class sets; `src/handlers/socket_flow_handler/types.rs`: the never-called change-detection method, its two maps and deadband fields deleted; `get_client_id`'s identical branches merged; `tests/gpu_safety_tests.rs`: a never-read accumulator and index loops tidied. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**

## Re-verification — 2026-10-07 at 3b3ee7779 (clippy sweep, round 2)

`chore/clippy-sweep` (merged with main at c025c1694) changes this record's governed paths only as follows:

- `src/handlers/socket_flow_handler/mod.rs`: stops re-exporting the deleted `PreReadSocketSettings`.
- `src/handlers/socket_flow_handler/types.rs`: deletes `PreReadSocketSettings` and eight never-read rate/motion fields; `dev_bypass_ok` is now compiled only in dev/`dev-auth` builds, matching its only reader.

None of these changes touches the decision this record makes. Every deletion had no caller in any build (debug, release, `--features redis`). `cargo clippy --workspace --all-targets -- -D warnings` is clean in debug and release; `cargo test --workspace --tests` on the merged tree: 3242 passed, 0 failed, 83 ignored. **Still holds.**
