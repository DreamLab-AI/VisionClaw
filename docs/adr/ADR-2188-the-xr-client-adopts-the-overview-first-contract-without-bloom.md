---
id: ADR-2188
title: The XR client adopts the overview-first contract — sized points, screen-width edges, label budget and highlight mask — without bloom
date: 2026-11-06
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 4adab22f02a09dfddc29b59b7d1f0213d5b3e948
verified_paths: []
relates: [ADR-2179, ADR-2181, ADR-2182, ADR-2183, ADR-2185, ADR-2186]
owner: jjohare
review_trigger: "A headset frame-time receipt below 90 fps with the overview path on; a Godot upgrade changing MultiMesh or shader point-size behaviour; a request to bring bloom to XR."
repo: visionclaw
domain: XR-client
evidence_class: source
acceptance_boundary: "Accepted on operator instruction 2026-11-06 ('fully implement the upgrades to the visual system for desktop and XR including the menus'). XR source (xr-client/scripts/graph_scene.gd, hud.gd, radial_menu.gd, query_builder.gd) at main 4adab22f0 was not yet read in full when this record was written; headset frame rate and visuals cannot be verified from the agentbox container."
---

# ADR-2188 — The XR client adopts the overview-first contract without bloom

## Context
- ADR-2179 to ADR-2187 were scoped to the desktop client. On 2026-11-06 the operator asked for the same upgrades on XR, menus included.
- The XR client (Godot 4.6, `xr-client/`) already renders 13,164 nodes and 145,692 edges at 90 fps (`docs/XR-client.md`). Its rendering invariants stand: glow and bloom off, Linear tonemap, a 90 fps floor.
- The desktop and XR clients read the same server graph and the same wire, so the same node table and highlight semantics can apply to both.

## Decision
1. **P1–P4 of ADR-2179 apply to XR.** At the XR default view, the knowledge body is legible, edges carry structure, interactive filters grey out rather than remove, and nothing is truncated silently.
2. **Representation.**
   - Overview nodes: a `MultiMesh` of camera-facing quads, sized in screen pixels (clamped in metres at close range). Gems or meshes are the near level of detail only.
   - Overview edges: screen-width lines that fade by projected length, like ADR-2181. Capacity is sized from the data, and dropped edges are counted and shown in the HUD.
3. **No bloom.** Emphasis in XR is by colour, size and outline, never glow.
4. **Highlight mask.** The XR client keeps a per-node highlight byte, using the ADR-2183 semantics (selected, hovered, search hit, filtered-out greyed). Filters set in the XR menus write it.
5. **Label budget.** ADR-2186's priority-ranked budget applies, with a smaller default budget suited to headset text legibility.
   - *Set at implementation (accepted by the lead, 2026-11-06):* the default is 8 ranked labels plus up to 4 pinned ones (held, selected, hovered), so the label pool stays at 12 anchors (`xr-client/scripts/label_budget.gd`). The reason is draw calls more than legibility: each visible `Label3D` is its own draw call, and the scene measures 31–34 of PRD-008's 50. Ranked labels draw a title only and pinned ones title plus detail, so the worst case is 8 + 4 × 2 = 16 calls, down from 12 × 2 = 24 before. The desktop default is 40 + 40 + 100 (ADR-2186 D1). The operator can lower the budget in Encode › Labels (0–8).
6. **Menus.** The XR HUD and radial menu expose the same information architecture as the desktop menus, as set by the menu-audit record (ADR-2189). Controls are reachable within two levels.

## Consequences
- XR and desktop converge on one visual grammar, which halves the design surface.
- Headset verification needs a person with a headset. Headless GUT tests cover logic, budgets and capacity, but not visual quality.

## Verification
None yet. Required:
- **D1–D2:** GUT tests for edge-capacity sizing, the dropped-edge counter, and LOD switching thresholds.
- **D4:** a GUT test showing that a filter greys a node out rather than removing it.
- **D5:** a GUT test of the label-budget ranking.
- **Headset:** a 90 fps receipt on the VIVE Pro or Quest with the overview path on (operator-run).
