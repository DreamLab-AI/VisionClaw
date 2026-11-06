---
id: ADR-2187
title: A cosmos.gl 2D overview fed with server positions is built as a time-boxed reference spike, and the Cosmograph product library is not used
date: 2026-11-05
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4adab22f02a09dfddc29b59b7d1f0213d5b3e948
verified_paths: []
relates: [ADR-2135, ADR-2179, ADR-2181, ADR-2182, ADR-2184]
owner: jjohare
review_trigger: "The spike's exit review (keep as a route, or delete); a cosmos.gl release adding WebGPU or 3D to the MIT engine; any change to Cosmograph's licence."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source
acceptance_boundary: "cosmos.gl 3.6.0 source and package metadata read on 2026-11-05 (MIT; luma.gl 9.3 on WebGL2; enableSimulation false + setPointPositions supported). @cosmograph/cosmograph 2.5.1 licence read from npm (CC-BY-NC-4.0). Cosmograph's 3D mode is vendor-documented (cosmograph.app/docs-lib/features/3d) and does not appear in the MIT engine's release notes. Nothing was built or rendered with VisionClaw data."
---

# ADR-2187 — A cosmos.gl 2D overview fed with server positions is built as a time-boxed reference spike, and the Cosmograph product library is not used

## Context
- **What the reference looks like.** The owner's reference for the target look is Cosmograph.
- **Licences.** Its open engine, cosmos.gl (`@cosmos.gl/graph` 3.6.0), is MIT. The product library (`@cosmograph/cosmograph` 2.5.1) is CC BY-NC 4.0, which excludes commercial use without a separate licence.
- **cosmos.gl can render positions it is given.** It can draw externally supplied positions without running its own simulation (`enableSimulation: false`, `setPointPositions`), so VisionClaw's CUDA layout can stay authoritative.
- **It cannot share our renderer.** cosmos.gl runs on luma.gl over WebGL2. It cannot share the R3F/WebGPU context the desktop uses, so it would be a separate canvas or route.
- **The cost of the alternative.** ADR-2181 to ADR-2186 port Cosmograph's techniques into our own renderer, which is weeks of work. Seeing our own graph drawn the Cosmograph way first would ground that work in a direct comparison.

## Decision
1. **A spike, not a product feature.** Build a separate `/overview` route that renders the knowledge graph with cosmos.gl in 2D.
   - Point positions come from the server stream, projected to 2D on the client by dropping the axis with the least spread, or by the display projection's ground plane.
   - The cosmos.gl simulation is disabled.
   - Colours come from the same domain palette.
   - It is time-boxed to two weeks. The route is behind a settings flag and off by default.
2. **Only the MIT engine.** The spike uses `@cosmos.gl/graph` alone. `@cosmograph/cosmograph` and the hosted Cosmograph app are not dependencies of any VisionClaw build. Loading VisionClaw data into the hosted app is an owner decision, outside this record.
3. **What the spike must produce:**
   - side-by-side screenshots against the 3D view at the same data;
   - a receipt row on the same host as ADR-2171 Decision 3;
   - a written list of the cosmos.gl rendering parameters that made the difference (link width and fade, point size, blending).
   Those parameters become the starting defaults for ADR-2181 and ADR-2182.
4. **Exit.** At the review the owner chooses one of three outcomes, each recorded by amending this record:
   - **(a)** delete the route;
   - **(b)** keep it as a permanent 2D overview mode, which is then a new subsystem and gets its own record;
   - **(c)** keep it as a test fixture only.

## Consequences
- **What it settles.** A cheap, direct answer to "would our graph look like Cosmograph with their drawing choices?", before the larger port.
- **Cost.** Adds a WebGL2 dependency (luma.gl via cosmos.gl) to the client bundle for one flagged route. It is code-split, so other routes do not load it.
- **It is not a substitute.** A 2D route has no XR parity and no gems. ADR-2179's suite stays the main path.

## Verification
None yet. Each decision has its own check:
- **D1:** the route renders the live knowledge graph with server positions; the screenshot is recorded.
- **D2:** `npm ls @cosmograph/cosmograph` is empty in `client/`.
- **D3:** the side-by-side pair, the receipt row and the parameter list are committed under `docs/evidence/`.
