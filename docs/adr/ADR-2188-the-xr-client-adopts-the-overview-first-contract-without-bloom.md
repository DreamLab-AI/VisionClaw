---
id: ADR-2188
title: The XR client adopts the overview-first contract — sized points, screen-width edges, label budget and highlight mask — without bloom
date: 2026-11-06
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: c7c78c88d6fbeaa91d873dc80e20d8ed163faffb
verified_paths: []
relates: [ADR-2179, ADR-2181, ADR-2182, ADR-2183, ADR-2185, ADR-2186]
owner: jjohare
review_trigger: "A headset frame-time receipt below 90 fps with the overview path on; a Godot upgrade changing MultiMesh or shader point-size behaviour; a request to bring bloom to XR."
repo: visionclaw
domain: XR-client
evidence_class: source+test+runtime
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
All satisfied (2026-11-06):
- **D1–D2:** `xr-client/tests/unit/test_overview_lod.gd` — edge-limit sizing, the dropped counter and HUD badge, LOD thresholds, pixel floor and fade.
- **D4:** `xr-client/tests/unit/test_xr_filter_behaviour.gd` — a received filter greys a node out rather than removing it, with precedence.
- **D5:** `xr-client/tests/unit/test_label_budget.gd` — ranking, pinning, collisions.
- **Headset:** the receipt in the status paragraph below (HP, 88–90 fps, three volumes, zero errors over 15 min; `docs/XR-client.md` 0.1.35).

## Status at integration (2026-11-06)

Complete in code and under headless GUT (660 tests, 0 failures) and the Rust suite. Headset receipt, 2026-11-06 on the HP (Godot 4.8-beta2, snapshot vc-809a859aa, backend main 809a859aa): OpenXR session focused, 88-90 fps (11.1 ms/frame) with all three volumes (knowledge 84 nodes, ontology 9,604, Codescape 4,817), zero script, parse, shader, heartbeat or reconnect errors over 15 minutes. Visual legibility of the rail and pages in the headset is still a human check. Activation is active.

## Correction — 2026-11-07: `activation_status` vocabulary

`a950f4d80` set `activation_status: active`, which is not a value the ledger accepts (`inactive | staged | live`, `scripts/adr-index-gen.js:37`). As a result `adr-index-gen.js --check` failed and docs-ci went red. The status paragraph above records the transition it meant: staged → in service after the HP headset receipt. In this vocabulary that is `live`, so the field now reads `live`. `evidence_class` becomes `source+test+runtime`: the headless GUT suites are listed under Verification, and the headset receipt is a runtime observation. That receipt was made on the HP on 2026-11-06 and was not repeated for this correction. `acceptance_boundary` stands as written, and the visual-legibility check is still a human one.
