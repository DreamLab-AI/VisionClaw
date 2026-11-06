---
id: ADR-2179
title: The desktop knowledge view is overview-first — the whole graph legible at a glance, then filtered by highlight
date: 2026-11-05
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4adab22f02a09dfddc29b59b7d1f0213d5b3e948
verified_paths: []
relates: [ADR-2135, ADR-2170, ADR-2171, ADR-2180, ADR-2181, ADR-2182, ADR-2183, ADR-2184, ADR-2185, ADR-2186, ADR-2187]
owner: jjohare
review_trigger: "Any record ADR-2180 to ADR-2187 is accepted or rejected; ADR-2171 is rejected (its Decision 3 receipt is this suite's measuring stick); the first desktop receipt row at the live graph's size."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source+runtime
acceptance_boundary: "Context comes from a 2026-11-05 gap study against Cosmograph / cosmos.gl 3.6.0 (source read from github.com/cosmosgl/graph). Runtime: one browser-sidecar session against the live dev container (9,688 nodes, 147,048 edges, WebGPU renderer, memory cloud ON from persisted settings); scene introspection through window.__scene. Sidecar FPS was invalid (cosmos.gl's own demo also ran at 8–11 fps there) and is not used. Cloud-off framing, the /api/graph/data payload and desktop frame time were not measured; the container was stopped externally mid-study."
---

# ADR-2179 — The desktop knowledge view is overview-first — the whole graph legible at a glance, then filtered by highlight

## Context
- **The knowledge graph was barely visible in the live session.** The desktop web client was checked live on 2026-11-05 with the memory cloud on. The 9,604-node knowledge graph showed as a ~60 px grey ball. Hiding its node and edge meshes removed only that ball, which proves the larger glowing bodies on screen are the 30,000-point memory cloud.
- **Edges were not visible at fit zoom.** They only appeared, as aliased dotted hairlines, after a heavy zoom-in. The edge instance mesh held exactly its 65,536 default capacity, against 147,048 edges in the graph.
- **The Cosmograph comparison.** Cosmograph (cosmos.gl, MIT) reads as a knowledge map at the same scale because of four things:
  - 1 px screen-space links that fade by on-screen length;
  - small point sprites;
  - filtering that greys out instead of removing;
  - a columnar data layer driving encoding and linked filters.
  Its GPU force simulation is *not* the difference: VisionClaw's CUDA layout is stronger, and Cosmograph has shipped a 3D mode since August 2026.
- **Desktop frame time is unmeasured.** ADR-2171 Decision 3 proposes the receipt that would measure it.

## Decision
1. **The four properties.** The desktop knowledge view is held to:
   - **(P1) Legible overview.** At the default fit, the graph you came to see fills the frame, and its edge structure is visible without zooming.
   - **(P2) Structure from edges.** Edges carry the overview: thin, screen-space and length-faded. Rich geometry is a close-zoom level of detail (LOD), not the overview.
   - **(P3) Filter by highlight.** Interactive filters and selection grey out what is not selected. They do not remove it. Removal is reserved for access control and server-side population scoping.
   - **(P4) No silent truncation.** Any render-side capacity limit is either sized from the data or raised to the user as a visible notice. It never clips quietly.
2. **The suite.** The upgrade is delivered as separately decidable records:
   - ADR-2180 framing;
   - ADR-2181 edges;
   - ADR-2182 nodes and glow;
   - ADR-2183 highlight mask;
   - ADR-2184 node table and encoding;
   - ADR-2185 linked filter widgets;
   - ADR-2186 label budget;
   - ADR-2187 cosmos.gl 2D overview spike.
   Each one cites the property it serves. Rejecting one does not block the others, except where a record names a dependency: ADR-2185 depends on ADR-2183 and ADR-2184.
3. **Performance is measured, not asserted.** Every performance claim in the suite carries a receipt row from ADR-2171 Decision 3, on a named host with a real GPU. The browser sidecar is not a benchmark host. If ADR-2171 is rejected, this suite adopts its Decision 3 measurement method as its own precondition.
4. **What the suite leaves alone:**
   - the CUDA layout and the 52-byte wire record (ADR-2018);
   - tri-layout separation (ADR-2135);
   - the XR client;
   - the gem and glass look, which stays as close-zoom detail.

## Consequences
- **Easier.** A reviewer can test any desktop graph change against P1–P4 by looking at the screen.
- **Harder.** Two deliberate earlier choices give ground in the overview:
  - the all-bodies camera fit of ADR-2135 (see ADR-2180);
  - glass tubes as the only edge form.
- **Follow-on work.** Each record lists its own. The suite's order is:
  1. ADR-2180 and the ADR-2181 cap notice, which are cheap;
  2. ADR-2181, ADR-2182 and ADR-2183;
  3. ADR-2184, ADR-2185 and ADR-2186.
  ADR-2187 can run in parallel as a reference.

## Verification
None yet (`implementation_status: none`). The acceptance scenarios run on the live graph (≥ 9.5k nodes, ≥ 140k edges), on a named host with a real GPU, with screenshots and an ADR-2171 receipt:
- **A1 (P1, P2):** with the default settings, the knowledge body fills at least 50% of the shorter screen axis at fit, and edge structure is visible in the screenshot without zooming.
- **A2 (P3):** applying any interactive filter leaves the node count of the drawn instance meshes unchanged and changes only the highlight mask.
- **A3 (P4):** with the edge count above the current capacity, every edge is drawn, or a visible notice states how many are not.
- **A4:** GPU p95 frame time is no worse than the ADR-2171 Decision 3 baseline.
