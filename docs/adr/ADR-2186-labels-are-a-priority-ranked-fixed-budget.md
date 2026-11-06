---
id: ADR-2186
title: Labels are a priority-ranked fixed budget, drawn the same way on both renderer paths
date: 2026-11-05
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: c7c78c88d6fbeaa91d873dc80e20d8ed163faffb
verified_paths: []
relates: [ADR-2171, ADR-2179, ADR-2183, ADR-2184]
owner: jjohare
review_trigger: "A receipt row for the label pass; three adding a WebGPU text path that changes the glyph-quad trade-off; ADR-2184 adding a centrality or rank column to the table."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source
acceptance_boundary: "Label path read in client/src/features/graph/components/InstancedLabels.tsx at main 4adab22f0 (MAX_GLYPHS 32768 at :113, distance threshold default 1200 at :377, WebGPU branch to an HTML fallback at :302-304). The live renderer was WebGPU (console warning from THREE.WebGPURenderer). Cosmograph label limits from cosmograph.app/docs-lib/features/labels. No label pass was profiled."
---

# ADR-2186 — Labels are a priority-ranked fixed budget, drawn the same way on both renderer paths

## Context
- **The WebGL path.** Labels are instanced glyph quads, pre-allocated for `MAX_GLYPHS = 32768` (`InstancedLabels.tsx:113`). They are chosen closest-first within a distance threshold (default 1200, `:377`).
- **The WebGPU path,** which is the live default, branches to a DOM `<Html>` fallback (`InstancedLabels.tsx:302-304`). DOM labels scale poorly with count and composite outside the canvas.
- **Distance is not importance.** Closest-first selection means a zoomed-out view labels whatever happens to be near the camera, not what matters.
- **Cosmograph's approach.** It shows a small weighted set: the top 30, about 30 dynamic, and 100 selected. Cluster labels outrank point labels.

## Decision
1. **A fixed budget.** The label budget defaults to:
   - 40 overview labels;
   - 40 dynamic labels around the cursor;
   - up to 100 for the current selection and its neighbourhood.
   Each tier can be changed in settings. The glyph buffer is sized from the budget, not from `MAX_GLYPHS`.
2. **Priority, not proximity.**
   - The overview tier ranks nodes by a priority column. The default is the wire centrality, replaced by the ADR-2184 encoding column when one is chosen.
   - It then removes screen-space collisions, using a coarse grid over projected positions, before drawing.
   - Cluster labels (from the server cluster id) outrank node labels at overview zoom.
3. **Greyed-out nodes have no labels.** This follows ADR-2183.
4. **One drawing path.** WebGPU draws labels as glyph quads in the canvas, as WebGL does: an SDF or MSDF atlas on instanced quads through TSL. The DOM `<Html>` fallback is kept only for the node detail tooltip.
5. **Cadence.** Label selection runs at a bounded rate (about 4 Hz, matching today's frustum refresh at `GraphManager.tsx:399-404`) and on camera stop. It does not run per frame.

## Consequences
- **What labels mean.** Labels at fit name the important nodes and clusters, not the nearest ones.
- **One code path.** Both renderers share it, which removes the DOM-count scaling risk on WebGPU.
- **New work.** Requires a TSL glyph material, and a priority column that is cheap on the wire (centrality) or comes from ADR-2184.

## Verification
None yet. Each decision has its own check:
- **D1/D2:** at fit on the live graph, the label count is within the budget and labels name the top-centrality nodes (screenshot plus the ranked list).
- **D4:** on WebGPU, the DOM label element count is 0 outside the tooltip.
- **D5:** a main-thread profile shows label selection no more than about 4 Hz while the camera moves.
