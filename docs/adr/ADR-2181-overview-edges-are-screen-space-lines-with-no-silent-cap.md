---
id: ADR-2181
title: Overview edges are screen-space lines that fade by on-screen length, with capacity sized from the data and never silently capped
date: 2026-11-05
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4adab22f02a09dfddc29b59b7d1f0213d5b3e948
verified_paths: []
relates: [ADR-2171, ADR-2179, ADR-2182, ADR-2183]
owner: jjohare
review_trigger: "ADR-2171 accepted or rejected (Decision 4 is this record's data source); the first receipt row with screen-space edges; three changing Line2NodeMaterial or admitting sized line primitives on WebGPU."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source+runtime
acceptance_boundary: "Edge path read in GlassEdges.tsx, GlassEdgeMaterial.ts and InferredEdges.tsx at main 4adab22f0; three 0.187.0 source read for PointsNodeMaterial.js:19-27 and Line2NodeMaterial.js:19-22. Runtime: the live edge InstancedMesh held count = capacity = 65,536 against 147,048 graph edges; how many edges survive upstream filtering was not measured. Edges were invisible at fit and appeared as aliased dotted hairlines only at heavy zoom. cosmos.gl link behaviour taken from its config.ts defaults (linkVisibilityDistanceRange [50,150], linkVisibilityMinTransparency 0.25)."
---

# ADR-2181 — Overview edges are screen-space lines that fade by on-screen length, with capacity sized from the data and never silently capped

## Context
- **What an edge is today.** Each edge is a lit 10-vertex glass tube in an `InstancedMesh`. Its radius is world-space (`baseWidth` 0.04 in the live settings), its opacity is 0.3 and it uses `MeshPhysicalMaterial` (`GlassEdgeMaterial.ts`). At fit zoom the tube is far below one pixel wide, so the edges vanish; close up they alias into dotted lines.
- **The cap.** Capacity grows by doubling up to `DEFAULT_EDGE_CEILING = 65536` (`GlassEdges.tsx:39-40`, `:429`). The live graph has 147,048 edges, and the mesh sat at exactly 65,536. The node quality filter is on by default (`src/settings/models.rs:147-158`, threshold 0.9), so some edges are likely dropped before they reach the renderer; how many was not measured. The overflow message is a log line (`GlassEdges.tsx:414-422`), and no edge-capacity message appeared in the console at any level.
- **Per-frame CPU cost.** Matrices are rebuilt on the CPU every frame (`computeInstanceMatrices`, `GlassEdges.tsx:126-176`). ADR-2171 Decision 4 already proposes moving edge transforms to the GPU on the WebGPU path, and leaves WebGL unchanged.
- **WebGPU has no wide line primitives.** Line primitives are 1 px. Wider lines need instanced quads, which three provides as `Line2NodeMaterial` (`three/src/materials/nodes/Line2NodeMaterial.js:19-22`).
- **cosmos.gl's approach.** It draws 1 px instanced link strips, with alpha fading by on-screen length. That is the property that makes its overview read as structure.

## Decision
1. **Overview edges are screen-space.**
   - On the WebGPU path, overview edges are instanced quads whose width is set in pixels (`Line2NodeMaterial` or an equivalent TSL quad), with a floor of 1 device pixel. They are unlit and alpha-blended. Endpoint positions come from the GPU node-position source ADR-2171 Decision 4 establishes, so there is no per-frame CPU edge loop.
   - On the WebGL fallback, overview edges use the addons `LineSegments2` / `LineMaterial`, which are instanced screen-space quads, so width in pixels works there too. Plain `LineSegments` is not used: `linewidth` above 1 is ignored on ANGLE and most platforms. Positions are re-uploaded only when the topology changes or while the layout is moving.
2. **Length fade.** Overview edge alpha falls with on-screen length between two pixel thresholds. The defaults are the cosmos.gl ones: 50 px and 150 px, with a minimum opacity of 0.25. Edge colour keeps today's `colorByType`. Weight sets the width, from 1 px up to a small multiple.
3. **Glass tubes become close-zoom LOD.** The glass-tube mesh is drawn only for edges incident to selected or hovered nodes, plus edges whose on-screen length passes a close-zoom threshold, within a fixed tube budget. It stops being the overview.
4. **Capacity comes from the data.**
   - Overview edge capacity is set from the edge count of the current graph, rounded up to a power of two. The `maxEdgesCeiling` setting becomes a GPU memory budget rather than a fixed clip.
   - When the budget is exhausted, the client draws the highest-weight edges first and shows a visible notice in the canvas ("N of M edges drawn"). The browser console is not enough.
   - The same rule applies to the worker position buffer. `syncToSharedBuffer` truncates silently today (`client/src/features/graph/workers/lib/buffer-manager.ts:131-132`, `MAX_NODES = 50_000` at `graphWorkerProxy.ts:108`), so a truncation there raises the same notice.
5. **If ADR-2171 is rejected.** Decision 1's WebGPU path sources endpoints from the existing node transform texture that `GemNodes` already writes (`GemNodes.tsx:918-962`), not from CPU arrays.

## Consequences
- **What becomes visible.** All edges are visible at fit (P1/P2 of ADR-2179), and the edge-ceiling truncation stops (P4).
- **Cost.** Quads cost 4 to 6 vertices per edge, against 10 for tubes, and an unlit blended pass replaces `MeshPhysicalMaterial`. The net GPU effect still has to be measured.
- **Look.** Glass is kept as the close-zoom look. The overview loses the refractive look by design.
- **Inferred edges.** These (`InferredEdges.tsx`, a CPU-copied `LineSegments`) move onto the same overview path, with a dash or colour distinction.
- **WebGL is not a 1 px fallback.** Both renderer paths draw quads, so the WebGL path reaches the same look.

## Verification
None yet. Each decision has its own check:
- **D1/D2:** a screenshot at default fit on the live graph shows edge structure, and a receipt row confirms no per-frame CPU edge loop (main-thread profile with no `computeInstanceMatrices` while settled).
- **D4:** a fixture graph with more edges than the budget shows the in-canvas notice, and a unit test covers the buffer-manager notice.
- **D3:** selecting a node draws glass tubes for its incident edges only.

## Implementation notes
- **WebGL edge material (accepted deviation from D1, 2026-11-06).** The WebGL path does not use the stock addons `LineSegments2` / `LineMaterial`. That material has one `linewidth` uniform: no per-edge width, no per-edge alpha and no fade by on-screen length. The path instead uses a custom GLSL3 `ShaderMaterial` (`client/src/rendering/materials/OverviewEdgeMaterial.ts`) built the same way, as instanced screen-space quads extruded by half the edge width in CSS pixels. Weight sets the width, the D2 fade is computed in the vertex shader, and an edge with a greyed endpoint drops to `greyoutOpacity`. The WebGPU path uses the same maths in TSL. Neither path is a 1 px `LineSegments` or `Points`.
- **Endpoints.** Both paths fetch endpoints from one node position texture, `client/src/features/graph/overview/nodePositionTexture.ts`. Each texel holds xyz plus the highlight level, indexed by render index. The texture is uploaded only when the drawn positions or the highlight mask change. There is no per-edge CPU loop for overview edges.
- **Worker buffer (D4).** `graphWorkerProxy.setGraphTopology` grows the position SAB to the delivered node count (`sharedCapacityFor`). Any shortfall it cannot cover goes to `publishNodeBufferReport` and appears in the same in-canvas notice as edges (`OverviewEdgeNotice`).
