---
id: ADR-2182
title: Overview nodes are screen-sized sprites, gems are the close-zoom level of detail, and glow is reserved for emphasis
date: 2026-11-05
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: c7c78c88d6fbeaa91d873dc80e20d8ed163faffb
verified_paths: []
relates: [ADR-2171, ADR-2179, ADR-2181, ADR-2183]
owner: jjohare
review_trigger: "The first receipt row comparing sprite and gem passes; three changing PointsNodeMaterial sprite behaviour; a request to restore bloom on the overview."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source+runtime
acceptance_boundary: "Node path read in GemNodes.tsx, GemNodeMaterial.ts and GemPostProcessing.tsx at main 4adab22f0; three 0.187.0 PointsNodeMaterial.js:19-27 read. Runtime: the live knowledge InstancedMesh used a 240-vertex sphere geometry with MeshStandardMaterial (9,604 visible). The bloom-saturated blob seen live was the memory cloud, not the knowledge graph; bloom's effect on the knowledge graph was not measured."
---

# ADR-2182 — Overview nodes are screen-sized sprites, gems are the close-zoom level of detail, and glow is reserved for emphasis

## Context
- **What a node is today.** Each knowledge node is an instance of a 240-vertex lit sphere: `MeshStandardMaterial` on WebGL (`GemNodeMaterial.ts:46`), and a TSL transform texture on WebGPU (`GemNodes.tsx:918-962`). At 9,604 nodes that is about 2.3 M vertices for what reads at fit as single-pixel dots.
- **Per-frame CPU cost.** A CPU loop writes x, y, z and scale for every visible node every frame on both paths. WebGL writes `setMatrixAt`; WebGPU writes the transform texture.
- **WebGPU points are 1 px only.** Sized points need sprites (`PointsNodeMaterial` on `Sprite`, or instanced quads), per `three/src/materials/nodes/PointsNodeMaterial.js:19-27`. cosmos.gl's `gl_PointSize` approach does not carry over to WebGPU.
- **Bloom.** Bloom runs over the whole scene (`GemPostProcessing.tsx`: `UnrealBloomPass` on WebGL, `BloomNode` on WebGPU; default threshold 0.3, strength 0.3 at `:100-102`). The saturated glow seen live belonged to the memory cloud. Its effect on dense knowledge clusters is unmeasured.

## Decision
1. **Overview nodes are sprites.**
   - Overview nodes are camera-facing instanced sprites whose size is set in screen pixels, clamped between a minimum and a maximum. They have unlit flat or soft-disc shading and a thin rim for contrast on dark backgrounds.
   - On WebGPU they read positions from the same GPU source as ADR-2181's edges. On WebGL they use `THREE.Points` with a sized `PointsMaterial`; `gl_PointSize` is honoured there.
2. **Gems are close-zoom LOD.** A gem mesh is drawn for a node once its projected size passes a threshold, or when it is selected or hovered, within a fixed gem budget. The gem look survives as close-zoom detail.
3. **Size and colour encodings carry over.** Node size keeps today's sizing scheme until ADR-2184 makes it column-driven. Colour keeps `getDomainColor` until the same point.
4. **Glow is for emphasis.**
   - Bloom applies only to emphasised elements: selected, hovered and search hits. It is drawn through a selective-bloom layer or an emissive mask, not through a global luminance threshold over the whole scene.
   - The memory cloud keeps its own additive look, but no longer feeds the knowledge view's bloom.
   - Whether global bloom saturates dense knowledge clusters is measured before and after. It is not assumed.
5. **The CPU per-node loop.** It runs only while positions change: an idle-settled graph does no per-node CPU work per frame.

## Consequences
- **Vertex count.** Vertex work at overview falls by more than an order of magnitude, about 4 vertices per node instead of 240. This is arithmetic, not a measurement.
- **Look.** The overview gains the small-point look that makes dense structure legible. The "field of gems" look moves to close zoom.
- **Selective bloom.** This adds a render layer or mask, which is new code in both renderer paths.

## Verification
None yet. Each decision has its own check:
- **D1/D2:** a screenshot at fit shows sprites, a screenshot at close zoom shows gems, and a scene inventory shows the gem mesh count within its budget.
- **D4:** a receipt row and screenshot pair with global bloom against selective bloom on the live graph, with the cloud off.
- **D5:** a main-thread profile of a settled graph shows no per-node loop.

## Implementation notes
- **Scope of the sprite LOD (2026-11-06).** Only the knowledge population becomes sprites at overview zoom with gems as the close LOD (`GemNodes` gem LOD, `client/src/features/graph/overview/gemLod.ts`). The ontology and agent populations keep full gems at every zoom: they are small, and their gem shape carries type meaning the sprites do not. Revisit if either grows to knowledge-graph size.
- **One renderer (2026-11-06, operator decision).** The desktop runs only three's `WebGPURenderer`, which falls back to its own WebGL2 backend where WebGPU is missing (`client/src/rendering/rendererFactory.ts`; the `visionclaw-force-webgl` override maps to `forceWebGL: true`). Every material is TSL, compiled to WGSL or GLSL by the backend, so there is one dialect of each material and no `WebGLRenderer` path. D1's WebGL clause (`THREE.Points` with a sized `PointsMaterial`) no longer applies: the sprites are one TSL material (`OverviewNodeMaterial.ts`), and the GLSL3 twin is deleted.
- **No silent loss of the graph.** Gems beyond `gemLodDistance` are hidden only while the sprite layer is mounted with a built material and has not faulted (`overview/overviewLive.ts`). A fault is any of these (`rendering/renderFaults.ts`):
  - the material fails to build (`useOverviewMaterial` → `buildFault`);
  - a `renderObject` throw on 3 separate frames;
  - a pipeline that fails on the WebGPU backend (three logs it and skips the draw without throwing; routed by the material id in the pipeline label);
  - a GLSL program that fails to link on the WebGL2 backend (`renderer.debug.onShaderError`, routed to the material its program was created for, `installShaderErrorWatch`).

  Then every gem draws again and `OverviewEdgeNotice` names the failed layer. The WebGLRenderer-only program check (`webglProgramFailure`) is deleted.
- **No hidden graph before the positions (2026-11-06).** A layer is also not live until the node position texture has received drawn positions (`setOverviewPositions`, from OverviewLayer's first upload). Before that, at startup or with no position stream, the texture is all zeros. The layer used to count as live then: the sprites collapsed onto the origin, a grey dot, while the gems they replace were hidden. Now the gems draw and the overview meshes stay hidden (`overviewGlue.test.ts`).
- **Grey-out blend space.** WebGPURenderer blends in linear space on both backends. Re-measured 2026-11-06 in the browser sidecar on the WebGL2 backend with no post-processing: a 0.8 grey at α 0.1 over #101018 reads 82/255. The sRGB case (37/255) existed only for `WebGLRenderer` drawing straight to the canvas. Its correction (`srgbBlendAlpha`, `rendering/blendSpace.ts`) is deleted, and `explore.greyoutOpacity` is used as is.
- **Bloom.** `GemPostProcessing` is the node pipeline only (`RenderPipeline` + `BloomNode`) with the `emphasis`, `scene` and `off` scopes. The `EffectComposer` / `UnrealBloomPass` path is deleted. Seen in the sidecar on the WebGL2 backend: the emphasis halos bloom under `emphasis`, and the whole scene blooms under `scene`.

## Status at integration (2026-11-06)

Partial: sprites, gem LOD, encoding-driven colour, the fault fallback and emphasis-only bloom are built. Ontology and agent populations keep full gems by scope decision (see the implementation notes). Emphasis bloom was seen on the WebGL2 backend (2026-11-06) with emphasised halos. The real WebGPU backend is unverified because the sidecar has no adapter.
