---
id: ADR-2171
title: The desktop client catches up on WebGPURenderer's own facilities and does not adopt the meshlet drawing pipeline
date: 2026-11-01
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: The first GPU-timestamp receipt row for the desktop graph; a three.js release that moves the drawing pipelines into core or admits transparent materials; or the appearance of a dense opaque mesh volume (ADR-2158).
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source
acceptance_boundary: The three 0.187.0 npm source (MeshletDrawingPipeline.js, Renderer.js and WebGPUBackend.js) was read in the agentbox container on 2026-11-01, together with the client source on client/three-0187 @ b81918ec7, the main production bundle (dist/assets, built 2026-10-01) and the r184-r187 release notes. Nothing was rendered and no GPU was used. The perf figures cited are commit-message receipts (d15ad013e, c32af1085) from earlier code. The current desktop frame time is unmeasured, and the acceptance numbers below are targets.
lineage: Builds on ADR-2170 (splats; the three 0.187 bump is its decision 2). Leaves ADR-2032, ADR-2158, ADR-2166 and ADR-2167 (the headset in gl_compatibility) unchanged. Defers headset visual-extras work for one sprint.
---

# ADR-2171 — The desktop client catches up on WebGPURenderer's own facilities and does not adopt the meshlet drawing pipeline

## Context
- **What the meshlet pipeline is.** three r187 (PR #35191) added an experimental `Renderer.drawingPipeline` hook and the addons `MeshletDrawingPipeline` and `VisibilityBufferDrawingPipeline`. They do GPU-driven culling and LOD over 64-triangle clusters, and need the WebGPU backend.
- **What it accepts.** Opaque or alpha-tested `FrontSide` meshes only (`MeshletDrawingPipeline.js:274-278`). No `ArrayCamera` (`:3544`), and no GPU-written instance data (`:572`).
- **Our graph doesn't qualify.** The gems are `transparent` (`GemNodeMaterial.ts:56`). The edges are `transparent` and `DoubleSide` (`GlassEdgeMaterial.ts:56,58`). Our instances are 8 to 80 triangles.
- **The bottlenecks were not vertex throughput.** The recorded bottlenecks were CPU and fill (d15ad013e; c32af1085: `fillEdgeFrame` 492 → 10.7 ms at 144,674 edges).
- **Edges are still computed on the CPU.** Edge matrices are built per frame (`GlassEdges.tsx:126-176`), about 4.2 MB uploaded per frame at the 65,536-edge ceiling (`GlassEdges.tsx:40`) while the graph moves.
- **The fallback gate is broken in production.** The WebGPU→WebGL gate tests `backend.constructor.name === 'WebGLBackend'` (`rendererFactory.ts:143-144`). Terser renames that class (`class _A … isWebGLBackend=!0` in the production bundle), so the gate never fires in production. **Fixed on `client/three-0187` at `23cb01eaf`:** `backendIsWebGPU()` reads the backend's `isWebGPUBackend`/`isWebGLBackend` flags, and a test covers a renamed class.
- **The desktop has lagged the headset.** Since 2026-09-01 the headset has had 174 commits and the desktop client 92.

## Decision
1. **Meshlets are not adopted.** The desktop does not set `renderer.drawingPipeline` to a meshlet or visibility-buffer pipeline. This is revisited only when a dense opaque mesh volume exists, or when three admits transparent materials.
2. **The fallback gate uses three's flags.** Backend identity is read from `backend.isWebGPUBackend` and `backend.isWebGLBackend`, never from a class name.
3. **Desktop perf is measured on the GPU.** `trackTimestamp` is set at renderer construction when perf is requested at load (`?perf=1` or `localStorage.perfProbe`), because the backend fixes it at init (`WebGPUBackend.js:298`). PerfProbe then reads `resolveTimestampsAsync`. Desktop perf claims carry a receipt row in `docs/reference/performance-benchmarks.md` that records build mode and backend.
4. **Edge transforms move to the GPU on the WebGPU path.** They use a TSL `positionNode` or a compute pass over node positions. The CPU uploads edge data per topology change, not per frame. The WebGL path is unchanged.
5. **Workarounds are retested.** The `renderObject` try/catch wrapper and the DataTexture instance-transform indirection are retested on r187. Each is removed if its original crash no longer reproduces.
6. **Headset visual-extras work is deferred** for one sprint, after in-flight ADR-2168 and ADR-2169 fixes reach a safe point.

## Consequences
- The desktop gets its first measured frame. Every later choice is made from receipts, including whether `BatchedMesh` or `DirectRenderPipeline` is worth trying.
- The GPU edge path and the meshlet pipeline exclude each other, by the gate at `:572`. This ADR chooses the edge path.
- A failed WebGPU adapter reaches a clean `WebGLRenderer` in production again.
- Headset polish slips by about two weeks.
- ADR-2170 needs a re-stamp: `GaussianSplat` is an addon, not core, and its Context still says 0.183.

## Verification
None yet (`implementation_status: none`). Each item has its own check:
- **Decision 2:** a unit test with a renamed backend class passes, and a production build with `requestAdapter` returning `null` lands on `WebGLRenderer`.
- **Decision 3:** a receipt row on the real graph, on a named host.
- **Decision 4:** `fillEdgeFrame` plus `computeInstanceMatrices` take under 1 ms per frame while settling, and GPU p95 is no worse than the Decision 3 baseline.
- **Decision 5:** the repro outcome is recorded before anything is deleted.
