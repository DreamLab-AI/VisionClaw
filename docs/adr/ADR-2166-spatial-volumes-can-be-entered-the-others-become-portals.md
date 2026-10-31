---
id: ADR-2166
title: Spatial volumes can be entered; the others become portals
date: 2026-10-31
decision_status: accepted
implementation_status: partial
activation_status: inactive
supersedes: []
superseded_by: []
amends: [ADR-2158]
verified_commit: e49db18363bd3822016e9dde1e9eafe73322a5b8
verified_paths: [xr-client/scripts/space_portals.gd, xr-client/scripts/spatial_volume.gd, xr-client/scripts/emulated_volume.gd, xr-client/scripts/volume_arrangement.gd, xr-client/scripts/graph_scene.gd, xr-client/scripts/hud.gd, xr-client/scripts/xr_prefs.gd, xr-client/materials/portal_membrane.gdshader]
owner: jjohare
review_trigger: the first headset session with Enter used (stereo membranes, 90 Hz with an entered 6 m room, comfort of the dim); or XE merging FrameBudgetPass.set_volume_weights
repo: visionclaw
domain: XR-client
depends_on: [ADR-2158]
relates: [ADR-2159, ADR-2164]
evidence_class: source+test
acceptance_boundary: the layout, state, refusal, comfort and wiring logic is covered by unit tests; whether the membranes read as portals in stereo, whether a 6 m room holds 90 Hz on the VIVE, and whether the dim is comfortable are not established and are the acceptance gates for activation_status staged
---

# ADR-2166 — Spatial volumes can be entered; the others become portals

## Context
- The operator's request of 2026-10-31: "inspect and improve the visionclaw spatial containers to add the portal effects such that a user works in one space and can see into and move into the others".
- ADR-2158 puts three 1 m volumes on a ring about the user (E3). The user sees them all from outside; there is no way to work *inside* one and look at the others.
- `IMMERSIVE` is 1:1 scale (graph units as metres, hundreds of metres wide) and the native shim maps it straight onto the container's mode, so it cannot mean "entered".
- Compatibility multiview has a history of a blank second eye, so render-to-texture portals are off the table.

## Decision
1. **"Entered" is a new arrangement state, not `IMMERSIVE`.** `SpacePortals` (`xr-client/scripts/space_portals.gd`) enters volume X: X stays `BOUNDED`, gets `ROOM_BOUNDS = (6, 3, 6)` m through `set_runtime_bounds` (past the emulated 4 m clamp), is centred on the user's floor position with the box bottom at the floor, and its frame and handles are hidden (`portal_room`). The room cannot be moved or resized. E7 holds for it. `IMMERSIVE` and the shim's mapping are untouched.
2. **Portals are the other volumes' real boxes.** Every other visible, open volume moves onto a ring about the user, facing the user at eye height. The ring radius is the room's XZ half-diagonal + the largest portal's XZ half-diagonal + 0.3 m, so no portal box can meet the room box (a unit test checks this with an independent separating-axis test, including a resized 4 m portal and swaps). "Seeing in" is looking at the portal's real content through a membrane. There is no `SubViewport`, second camera, stencil, or screen or depth sampling.
3. **Membrane.** `materials/portal_membrane.gdshader` is unshaded, `blend_add`, `depth_draw_never`, `cull_disabled`, depth test on. It draws a rim and soft edge glow from the quad's UV edge distance plus a fresnel term, so the static look (reduced motion, the default) reads as a portal. The rim pulses only when the `motion` uniform is 1 (reduced motion off). All membranes are one `MultiMesh`: one draw call. Per-instance colour carries the tint (knowledge, ontology, Codescape) and, in alpha, the focus highlight.
4. **Moving in.** Trigger, grip and two-hand bindings are unchanged. Entering is an explicit action on the focused volume: HUD Session **Enter** (`volume_enter:<id>`), which reads **Leave** (`volume_leave`) while that volume is entered. The radial menu is generic and has no volume section, so it gets nothing. Entering another volume while inside swaps (the room becomes a portal, the target the room). Walk-through is optional and off by default (`xr.portal_walkthrough` in `xr_prefs`): the head inside a portal box for 0.25 s enters it, and it re-arms only after the head has been 0.3 m outside every portal.
5. **Leaving restores the ring exactly.** The first enter snapshots the arrangement (its own transform, and per volume the transform, bounds, mode, slot and moved flag). Leave restores it. While entered, `bring_to_front` is refused for every caller, box grips are ignored, and Reset Layout leaves first.
6. **Transitions.** Reduced motion on (the comfort default): one dim over the whole view (a black quad 0.2 m in front of the camera, 0.12 s), an instant snap while dark (the room's content fit is snapped too, so it does not grow in), then the dim back. Nothing around the user moves or scales while visible. Reduced motion off: the boxes ease over 0.5 s through `VolumeArrangement.ease_volume_to` (smoothstep). An arrangement carrying a two-hand transform takes the dim path in both directions, because easing it would visibly rescale the world. The flag is read from `visual_comfort_changed` via `_refresh_reduced_motion`.
7. **Refusals** return false and change nothing: the native container arm active or chosen but not started; the target unknown, closed, hidden or offline; the target already entered; a transition in flight. A room that is hidden, closed or offline while entered ends the space at once.
8. **The ring stays the default.** Portal mode is opt-in per action. E3's E7–E9 and the 90 Hz three-volume baseline are defined on the ring. E9 is unchanged: a Codescape `nodeDrag` is still refused locally (`claim_blocks_drag`, `BackendCaps.DRAG`), in portal mode too.
9. **Budget hook (stream XE).** On enter `graph_scene.gd` calls `FrameBudgetPass.set_volume_weights({entered: 1.0, others: 0.15})`; on leave all weights are 1.0. The call is guarded by `has_method`, so the branch works before and after the method exists.

## Consequences
- A user can work in one volume at room scale and see the others as real boxes on a ring. The ring layout, the shim, and the trigger/grip/two-hand bindings are unchanged.
- Portals are small: at 1 m boxes the ring is about 5.3 m out, so their detail reads at that distance only through the fit's content scale. Scaling a portal up is a follow-up, not decided here.
- Two-hand moves of the arrangement while entered move room and portals together, and leaving discards them (the snapshot restores the arrangement transform).
- The shader has not been compiled by a real renderer in this change, and the 6 m room's draw cost (the fit grows content to fill it, so the near caps matter) is unmeasured. XE's weights are what keep the portals from taking budget.
- `spatial_volume.gd` gains `portal_room`, `set_portal_room` and `snap_fit`; `volume_arrangement.gd` gains `snapshot`, `restore`, `ease_volume_to` and `layout_locked`; `xr_prefs.gd` gains `portal_walkthrough`.

## Verification
- Evidence class: `source+test`, and the tests are headless GUT only (Godot 4.6.1 and 4.8-dev5, dummy renderer): no stereo render, no headset run, no fill-rate measurement, and the shader has not been compiled on a real renderer. `tests/unit/test_space_portals.gd` (36 tests) covers enter, leave and swap state; exact restore of slots, moved flags, bounds, modes and the arrangement transform; the ring invariant against an independent oblique-box separating-axis test; E7 for the room on every frame; each refusal (native, closed, hidden, offline, unknown, already entered, mid-transition) asserting nothing changed; walk-through dwell and hysteresis; no visible motion in reduced motion and an eased ≤ 0.5 s move otherwise; one MultiMesh and one draw call; the shader free of screen and depth sampling; the budget hook; Codescape drag refused while Codescape is entered.
- Not verified: stereo rendering, the headset, fill rate, shader compilation on a real renderer, and the comfort of the dim. `implementation_status` stays `partial` and `activation_status` `inactive` until a headset session.
- `verified_commit` is the last code commit on the branch `xr/portals` (`e49db1836`); after the merge, re-stamp it to the merge commit if the governed paths moved.
