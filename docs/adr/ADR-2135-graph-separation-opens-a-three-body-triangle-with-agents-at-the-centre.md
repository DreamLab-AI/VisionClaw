---
id: ADR-2135
title: Graph Separation opens a ground-plane triangle of knowledge, ontology and memory, with agents drifting at its centre
date: 2026-10-07
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 96d9c426d4b94b42840b95d6644df97ee9afd617
verified_paths: [crates/visionclaw-tri-layout/src, crates/visionclaw-tri-layout/fixtures, src/actors/gpu/display_projection.rs, src/handlers/memory_flash_handler.rs, client/src/features/graph/triLayout.ts, client/src/features/graph/agentDrift.ts, client/src/features/bots/agentDriftFeed.ts, client/src/features/graph/utils/agentNudge.ts, client/src/features/visualisation/memoryCloud/cloudFrame.ts, client/src/features/graph/utils/sceneFitBounds.ts, xr-client/rust/src/cloud_frame.rs]
owner: jjohare
review_trigger: a change of the default camera direction; a fourth body joining the separated layout; agent nodes leaving the GPU graph; the XR work-layer pose rules (ADR-2109) changing
repo: visionclaw
domain: XR-client
---

# ADR-2135 — Graph Separation opens a ground-plane triangle of knowledge, ontology and memory, with agents drifting at its centre

## Context
The Motion › Layout Forces › Graph Separation slider (`graphSeparationX`, 0–400) only acted
with `enableDualDiscLayout` on, and then pushed two populations apart along Z (knowledge −sep,
ontology +sep, agents at 0) as facing X-Y discs. The memory cloud was framed on the merged
graph client-side (`cloudFrame.ts`, `cloud_frame.rs`) and ignored the slider. The operator
asked for the slider to separate knowledge, ontology *and* memory as a triangle along the
ground plane, with the centre left to the agents, which should move between the three graphs
as they work. The projection is display-only and undone before each physics step: feeding it
back lets ~56k knowledge↔ontology cross-links (rest length ~30) pull the populations together.

## Decision
- **One geometry, defined once.** `crates/visionclaw-tri-layout` (MIT, no dependencies) owns the
  triangle and the agent drift rule. The server projects with it, the XR client links it
  directly, and the desktop port `client/src/features/graph/triLayout.ts` is held to it by a
  committed fixture (`fixtures/tri_layout_fixture.json`, checked by the crate's
  `fixture_is_current` test, the vitest `triLayout.test.ts` and `cloud_frame.rs` tests).
- **Triangle.** Y is up, so the ground plane is X–Z. Vertex yaw θ is measured about +Y from +Z
  towards +X: **knowledge −60° (front-left), ontology +60° (front-right), memory 180°
  (back)**. The default camera sits on +Z looking at −Z, so neither graph hides the other, the
  cloud is the backdrop and the centre stays visible. Circumradius `R = separation × 2/√3`,
  so adjacent vertices sit `2 × separation` apart, the old knowledge↔ontology disc gap; saved
  values keep their meaning. Centroid = scene origin (the server re-centres every population
  on its median).
- **Orientation.** Each graph is yawed by `θ × s`, where `s` is a smoothstep from 0 at
  separation 0 to 1 at separation 100. At full strength a graph's disc normal (local +Z) lies on
  the line through the centroid. At separation 0 every vertex is the origin and every yaw is 0.
- **The slider alone opens the triangle.** The projection runs when `graphSeparationX > 0` **or**
  `enableDualDiscLayout` is on. Dual-disc now means "shape each graph as a disc facing the
  centre": it re-centres in-plane and rim-clamps (2600) at every separation. Without it the
  in-plane re-centre eases in with `s`; Z is re-centred with `s` in both modes. Separation 0
  with dual-disc off is the identity (or the plain Z-scale when `axisCompressionZ < 1`), and
  separation 0 with dual-disc on is the old coplanar pair, so the merged path is bit-for-bit
  what it was (`legacy_parity_at_separation_zero_in_every_mode`). No settings migration.
- **Memory cloud.** The client measures the graph on positions folded back into each graph's
  frame (nearest of the knowledge and ontology vertices), so the cloud keeps one graph's size,
  and places it at that centre plus the memory vertex. It glides there at the existing rate
  (0.8 s), snaps under reduced motion, and re-reads at once when the slider moves. The route,
  beads, bursts and camera rig live in the cloud's groups and follow it.
- **Agent drift is one rule, run where agents are drawn.** Every agent body is positioned
  client-side. Desktop demo agents are injected into the client graph, live swarm agents come
  from the separate bots graph, and XR avatars are placed by `agent_choreography.gd`. None of
  them passes through the GPU projection, so the server keeps GPU-graph agent nodes at the
  centroid and leaves the drift to the clients. Both clients run `DriftField` from the shared
  crate: XR links it in the render store; the desktop uses `agentDrift.ts`, which replays the
  crate's scripted run in the fixture. A `0x23` action credits the vertex of its target's
  population (desktop: the server's `Node::population` mirrored in `agentDriftFeed.ts`; XR: the
  node's wire-flag class). A `memory_flash` credits memory: it goes to the agent named by the
  new optional `agentId` (accepted by `POST /api/memory-flash` and relayed in the frame), and
  is otherwise shared equally by every agent present. Credit halves every 6 s. The target
  `0.6 × Σ wᵥ·vertexᵥ / (1 + Σ wᵥ)` stays inside the triangle, short of the graphs, and the
  shown offset follows it through a first-order lag (τ 1.5 s): no overshoot or oscillation,
  and a calm glide home when idle. Drift is stored as vertex coefficients, so slider moves
  carry agents with the triangle. Agents spawn at the centroid. On the desktop the offset is
  added to the agent's position in `GemNodes`. In XR it moves the slot a work agent rests at
  (centroid plus drift, on an 0.18 m ring), and a new avatar materialises at the centroid.
  The choreography stays the only pose writer (Invariant 8).
- **Desktop nudge.** In the separated layout a done or idle agent's client-side nudge offset
  relaxes back to its server position instead of drifting outward (`agentNudge.ts`). Working
  agents still converge on their target node, so they visit the graph they act on.
- **Camera.** On the desktop the auto-fit frames the sphere enclosing all three bodies whenever
  separation is above 0: each graph is one graph's folded extent at its vertex, and the cloud is
  where and as large as `cloudPlacement` puts it (only while it is on and loaded). At separation
  0 the fit is the old `robustBounds` fit. The camera refits once, after the slider has been still
  for 900 ms, never on every tick of a drag (`sceneFitBounds.ts`). XR has no camera auto-fit.
- **Display-only stays the rule.** The main loop, the bad-frame fallback and the immediate
  snapshot share `project_display`; the physics buffer is restored before every step.

## Consequences
- With dual-disc on, a non-zero separation no longer means ±Z: the discs sit on the triangle.
  Anyone who relied on the depth pair must use separation 0 with dual-disc on.
- The XR work layer keeps its single pose writer (Invariant 8). Avatars travel to their target
  nodes, which now sit on the separated graphs, and park at centroid + drift instead of the
  front-arc rim while separated (the rim returns at separation 0).
- The two clients run the drift independently from the same events and rule, so they agree up
  to event arrival timing. There is no drift wire frame.
- Memory activity is attributed per agent only when producers send `agentId`. Until then a
  flash pulls the whole swarm towards the cloud a little.

## Verification
At 96d9c426d, on main f95dc554f (code commits f275173a3, 08a3e2a41 and 96d9c426d):

- `cargo test -p visionclaw-tri-layout`: 22 passed, plus 2 doc tests. `fixture_is_current` holds the
  committed fixture (geometry plus a scripted drift run) to the code.
- Server `cargo test --lib`: 1,565 passed, 0 failed, 6 ignored. The `display_projection` tests
  check parity with the pre-ADR code at separation 0 in all eight dual-disc × compression cases.
  They also check that populations sit on their vertices, discs face the centroid, the projection
  is continuous in the slider, and agent nodes stay at the centroid. A handler test covers the
  `agentId` relay.
- xr-client `cargo test --workspace`: 527 passed. `cloud_frame` is checked against the fixture and
  parses `cloudFrame.ts` and `EmbeddingCloudLayer.tsx`. The render-store drift tests cover the
  target class, memory attribution and the return home.
- Client vitest: 112 files, 1,264 tests. `agentDrift.test.ts` replays the crate's drift run.
  `sceneFitBounds.test.ts` checks the camera fit: it is the old fit at separation 0, contains both
  graphs and the cloud once separated, and the settle trigger fires once per settle, never during
  a drag. `triLayout`, `cloudFrame`, `agentDriftFeed`, `agentNudge` and `registry` are covered too.
- `tsc --noEmit`, `vite build`, `cargo fmt --check` and clippy `-D warnings` are clean (server lib
  and tests, the new crate, the xr-client workspace).

A browser check of this design (before the camera fix) against the live backend (still on main's server) confirmed
the cloud group's world position at separation 0, 200 and 400 (z −45, −446, −908) and the
desktop agents' drift towards the ontology vertex during the agent demo. GUT tests were added
(`test_memory_cloud_layer.gd`, `test_graph_parity.gd`, `test_beat_pulse.gd`,
`test_tri_layout_rest.gd`) but not run: there is no Godot in the build container. Activation stays
`staged` until the operator has seen the triangle live on a backend built from this change.
