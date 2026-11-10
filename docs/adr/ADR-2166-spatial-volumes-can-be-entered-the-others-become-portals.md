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
verified_commit: eba81532aced55cd53edc37655571a49e3846c04
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

## External review note, 2026-11-02

The scale-relative rearm finding remains open at `e78210c4d`:
`space_portals.gd:617` compares world distance to fixed REARM_DISTANCE 0.3.
The [reconciliation](../reference/external-review-2026-11-02.md) distinguishes this source
finding from the unchanged stereo, comfort and headset activation gates.

**Update, 2026-11-05 (`6b811f7a1`):** the rearm finding is closed in source. `distance_to_box` now returns world metres (the per-axis scale of the box is applied to the local overshoot) and `update_walkthrough` compares it with `REARM_DISTANCE` scaled by the arrangement's largest axis scale, so a scaled room or ring no longer rearms at the wrong distance. The first paragraph is kept as the record at `e78210c4d`. Stereo, 90 Hz and comfort gates are unchanged and still open; `test_space_portals.gd` carries the new cases.

## Context
- The operator's request of 2026-10-31: "inspect and improve the visionclaw spatial containers to add the portal effects such that a user works in one space and can see into and move into the others".
- ADR-2158 puts three 1 m volumes on a ring about the user (E3). The user sees them all from outside; there is no way to work *inside* one and look at the others.
- `IMMERSIVE` is 1:1 scale (graph units as metres, hundreds of metres wide) and the native shim maps it straight onto the container's mode, so it cannot mean "entered".
- Compatibility multiview has a history of a blank second eye, so render-to-texture portals are off the table.

## Decision
1. **"Entered" is a new arrangement state, not `IMMERSIVE`.** `SpacePortals` (`xr-client/scripts/space_portals.gd`) enters volume X: X stays `BOUNDED`, gets `ROOM_BOUNDS = (6, 3, 6)` m through `set_runtime_bounds` (past the emulated 4 m clamp), is centred on the user's floor position with the box bottom at the floor, and its frame and handles are hidden (`portal_room`). The room cannot be moved or resized. E7 holds for it. `IMMERSIVE` and the shim's mapping are untouched.
2. **Portals are the other volumes' real boxes.** Every other visible, open volume moves onto a ring about the user, facing the user at eye height. The ring radius is the room's XZ half-diagonal + the largest portal's XZ half-diagonal + 0.3 m, so no portal box can meet the room box (a unit test checks this with a separating-axis test, `boxes_intersect`, itself checked against hand-worked cases, including a resized 4 m portal and swaps; portals are also checked against each other at the maximum count, because the step between neighbours shrinks below 50° from 8 portals on so the ring never wraps). "Seeing in" is looking at the portal's real content through a membrane. There is no `SubViewport`, second camera, stencil, or screen or depth sampling.
3. **Membrane.** `materials/portal_membrane.gdshader` is unshaded, `blend_add`, `depth_draw_never`, `cull_disabled`, depth test on. It draws a rim and soft edge glow from the quad's UV edge distance plus a fresnel term, so the static look (reduced motion, the default) reads as a portal. The rim pulses only when the `motion` uniform is 1 (reduced motion off). All membranes are one `MultiMesh`: one draw call. Per-instance colour carries the tint (knowledge, ontology, Codescape) and, in alpha, the focus highlight.
4. **Moving in.** Trigger, grip and two-hand bindings are unchanged. Entering is an explicit action on the focused volume: HUD Session **Enter** (`volume_enter:<id>`), which reads **Leave** (`volume_leave`) while that volume is entered. The radial menu is generic and has no volume section, so it gets nothing. Entering another volume while inside swaps (the room becomes a portal, the target the room). Walk-through is optional and off by default (`xr.portal_walkthrough` in `xr_prefs`): the head inside a portal box for 0.25 s enters it, and it re-arms only after the head has been 0.3 m outside every portal.
5. **Leaving restores the ring exactly.** The first enter snapshots the arrangement (its own transform, and per volume the transform, bounds, mode, slot and moved flag). Leave restores it. While entered, `bring_to_front` is refused for every caller, box grips are ignored, and Reset Layout leaves first.
6. **Transitions.** Reduced motion on (the comfort default): one dim over the whole view (a black quad 0.2 m in front of the camera, 0.12 s), an instant snap while dark (the room's content fit is snapped too, so it does not grow in), then the dim back. Nothing around the user moves or scales while visible. That includes a portal being hidden, shown or resized while entered (the ring is re-laid in the dark) and the room going offline, hidden or closed (the space ends in the dark, from any phase of a dim in progress). Reduced motion off: Enter from the ring and a portal re-layout ease the boxes over 0.5 s through `VolumeArrangement.ease_volume_to` (smoothstep). **Leave and swap always take the dim path, in either setting**, because they shrink a room's content and E7 forbids an eased shrink (a shrink is immediate); an eased Leave would have cut the content to a sixth in one frame with the 1 m frame back around the user. An arrangement carrying a two-hand transform takes the dim path in both directions, because easing it would visibly rescale the world. Only Reset Layout leaves instantly. The flag is read from `visual_comfort_changed` via `_refresh_reduced_motion`.
7. **Refusals** return false and change nothing: the native container arm active or chosen but not started; the target unknown, closed, hidden or offline; the target already entered; a transition in flight. A room that is hidden, closed or offline while entered ends the space through the dim.
8. **The ring stays the default.** Portal mode is opt-in per action. E3's E7–E9 and the 90 Hz three-volume baseline are defined on the ring. E9 is unchanged: a Codescape `nodeDrag` is still refused locally (`claim_blocks_drag`, `BackendCaps.DRAG`), in portal mode too.
9. **Budget hook (stream XE).** On enter `graph_scene.gd` calls `FrameBudgetPass.set_volume_weights({entered: 1.0, others: 0.15})`; on leave all weights are 1.0. The call is guarded by `has_method`, so the branch works before and after the method exists.

## Consequences
- A user can work in one volume at room scale and see the others as real boxes on a ring. The ring layout, the shim, and the trigger/grip/two-hand bindings are unchanged.
- Portals are small: at 1 m boxes the ring is about 5.3 m out, so their detail reads at that distance only through the fit's content scale. Scaling a portal up is a follow-up, not decided here.
- Two-hand moves of the arrangement while entered move room and portals together, and leaving discards them (the snapshot restores the arrangement transform).
- The shader has not been compiled by a real renderer in this change, and the 6 m room's draw cost (the fit grows content to fill it, so the near caps matter) is unmeasured. XE's weights are what keep the portals from taking budget.
- `spatial_volume.gd` gains `portal_room`, `set_portal_room` and `snap_fit`; `volume_arrangement.gd` gains `snapshot`, `restore`, `ease_volume_to` and `layout_locked`; `xr_prefs.gd` gains `portal_walkthrough`.

## Verification
- Evidence class: `source+test`, and the tests are headless GUT only (Godot 4.6.1 and 4.8-dev5, dummy renderer): no stereo render, no headset run, no fill-rate measurement, and the shader has not been compiled on a real renderer. `tests/unit/test_space_portals.gd` (48 tests) covers enter, leave and swap state; exact restore of slots, moved flags, bounds, modes and the arrangement transform; the ring invariant against an oblique-box separating-axis test (`boxes_intersect`, checked against hand-worked cases and by mutation) for portal against room and portal against portal at the maximum count; E7 for the room on every frame; each refusal (native, closed, hidden, offline, unknown, already entered, mid-transition) asserting nothing changed; walk-through dwell and hysteresis; no visible motion in reduced motion, including room loss and portal re-layout, and an eased ≤ 0.5 s move otherwise, with Leave and swap dimming in both; a Front ease in flight restored to its target; walk-through using the origin's floor; the grip guard during the dim back; one MultiMesh and one draw call; the shader free of screen and depth sampling; the budget hook; Codescape drag refused while Codescape is entered.
- Not verified: stereo rendering, the headset, fill rate, shader compilation on a real renderer, and the comfort of the dim. `implementation_status` stays `partial` and `activation_status` `inactive` until a headset session.
- `verified_commit` is the last code commit on the branch `xr/portals` (`f1edf742d`, after the R10 review fixes; first stamped at `e49db1836`); after the merge, re-stamp it to the merge commit if the governed paths moved.

Re-stamped to `8e92c5d5c` (2026-10-31, ADR-2167, the visual extras; merge of `main` at `dd09f66e7` into `xr/eye-candy`). Re-run: `git diff --stat f1edf742d8db725b610212e5e7097587f3c4dd4a 8e92c5d5c552164a9190424ef0cf637edc455455 -- xr-client/scripts/space_portals.gd xr-client/scripts/spatial_volume.gd xr-client/scripts/emulated_volume.gd xr-client/scripts/volume_arrangement.gd xr-client/scripts/graph_scene.gd xr-client/scripts/hud.gd xr-client/scripts/xr_prefs.gd xr-client/materials/portal_membrane.gdshader` gives only `xr-client/scripts/emulated_volume.gd | 106 ++++` (+102/-4). The change is the frame's build and material, not this record's decision: the frame can be a ribbon quad mesh instead of 1 px lines (`EmulatedVolume.set_ribbon_frames`, one call a frame either way), and `set_frame_hidden(bool)` is a second hide hook. `_on_state_changed` keeps `portal_room` in both `frame.visible` and `live`: `frame.visible = visible_state and bounded and not portal_room and not _frame_hidden`, so an entered room still shows no frame and no handles. **The review trigger "XE merging `FrameBudgetPass.set_volume_weights`" is met:** the method, its Rust split (`split_near_caps_weighted`, empty or equal weights identical to the unweighted split, `GEM_MIN` floor kept, NaN and negatives weigh 0) and their tests are in this merge (ADR-2167); the `has_method` guard in `graph_scene.gd` now takes the weighted path. GUT on the merged tree on the HP: `test_space_portals.gd` and every other script pass, 489 of 492 (3 pending), 48/48 scripts, no parse errors, on 4.6.1 and 4.8-beta2. Headless only for the portal tests, as before; stereo, fill rate and the headset are not re-asserted. The decision holds.

Re-stamped to `a4c07caed` (2026-10-31, ADR-2167 review fixes R11; first stamped at `8e92c5d5c`). Re-run: `git diff --stat 8e92c5d5c552164a9190424ef0cf637edc455455 a4c07caed3f05bc170a0096fdde878ceebbd19c1 -- xr-client/scripts/space_portals.gd xr-client/scripts/spatial_volume.gd xr-client/scripts/emulated_volume.gd xr-client/scripts/volume_arrangement.gd xr-client/scripts/graph_scene.gd xr-client/scripts/hud.gd xr-client/scripts/xr_prefs.gd xr-client/materials/portal_membrane.gdshader` gives only `xr-client/scripts/graph_scene.gd | 4 ++++`. The four lines are in `_tick_frame_budget`: for every volume the scene now adds `v.overhead_draw_calls()` (the volume frame's own call and the handle markers') to the draw calls it hands the frame-budget pass, so the volume frames are counted at every extras tier. It changes no portal, entry or weight behaviour (`budget_weights`, `_on_portal_entered_changed` and `set_volume_weights` are as before; the doc line on `set_volume_weights` says 1.0 is the neutral weight). The decision holds. Note: at production density the portal weights are mostly inert (open owner decision recorded in ADR-2167), so this record's line 46, "XE's weights are what keep the portals from taking budget", holds at the 1k fixture only.

Note (2026-10-31, owner decision on ADR-2167 R11 F5): the portal weighting is left as it is. The `GEM_MIN` floor and the `drawn x weight` split are unchanged, so at production density (gem cap 35) the weights remain nearly inert, and the line in Consequences that XE's weights keep the portals from taking budget holds at the 1k fixture only. The extras allowance (ADR-2167, amended) takes at most two gems at production density and does not touch the split. No code in this record's `verified_paths` changed, so `verified_commit` stays.

## Re-verification — 2026-10-31 (`ecd78b6cb`)

Re-verified 2026-11-01 at `ecd78b6cb`: the only change to the governed path is `xr-client/scripts/hud.gd`, where `KEY_EDGE_FLOW` (the Key page swatch mirroring the graph's flow-edge colour) moves from cyan to gold and its label gains "breathes slowly". It changes no HUD panel palette, no tab, and nothing this record decides. Evidence: `git diff 8e92c5d5c..ecd78b6cb -- xr-client/scripts/hud.gd` (2 lines).

Re-verified 2026-11-01 at `484d9392f` (xr/grab-client, ADR-2172). Changes since `a4c07caed`: `graph_scene.gd` now lets a node be grabbed in the entered volume and in the ring, never in a portal, releases held nodes when a space is entered, swapped or left, and refuses a grab during a transition (ADR-2172, which amends this record); `hud.gd` has a two-line edge-key edit. Enter, Leave, the ring, the membranes and the transitions are unchanged. The decision holds; its line 8 (E9) still holds for Codescape.

Re-verified 2026-11-01 at `232130a64` (xr/grab-client, ADR-2172 review fixes). The changes to `graph_scene.gd` since `484d9392f` extend the portal rules without changing them: `_grab_locked` also holds while an arrangement ease runs (the first entry from the ring eases with no dim, R15 F4), and while a node is held the HUD Front action, a link press and a box grip on the holding volume move no volume (F8). `space_portals.gd` and the room rules are untouched. The decision holds.

## Re-verification — 2026-10-31 (`f1fa1862c`)

Re-verified 2026-11-01 after merging xr/grab-server (ADR-2169), xr/grab-client (ADR-2172), client/three-0187 and docs/splat-adr, each Opus-reviewed (R15/R16/R17). The governed-path changes are those merges: drag holds and release in the GPU actor and socket handler, the held-only global_speed floor, two-hand grab and the edge breathe in the XR client, and the ADR 2168->2172 comment renames. This record's own claims are unchanged. Evidence on the merged tree: `cargo test --lib` 1768 passed, 0 failed (GPU 1); vitest 142 files / 1468 (installed three 0.183; the 0.187 run is on client/three-0187); XR GUT 522 passing on 4.6.1 and 4.8 at the grab-client head. Re-run: `git diff 2257ee7fa..f1fa1862c -- <governed path>`.

Re-verified 2026-11-01 at `03ddc61e1` (viz/edge-tracking). The only change to `xr-client/scripts/graph_scene.gd` is one comment in `_physics_process` (the far ribbon tier is now re-transformed every frame, not half per frame). Nothing this record decides changed. Evidence: `git diff 531df5ef7..03ddc61e1 -- xr-client/scripts/graph_scene.gd` (2 lines).

## Re-verification — 2026-11-01 (`0df96bc52`)

`xr-client/scripts/graph_scene.gd` changes one comment above `_tick_links`. The enter/portal behaviour this record governs is untouched.

## Re-verification — 2026-11-01 (`e99c16d31`)

`xr-client/scripts/spatial_volume.gd` gained `content_to_world() -> Transform3D`, and `world_from_content(p)` now returns `content_to_world() * p`. A pure refactor: both run `_ensure_built()` and `_content_world()` exactly as before, so entering a volume and the portals are unchanged. The link layer reads the transform once per volume per frame. Evidence: the full GUT suite, 49 scripts, 536 tests, 0 failing; `tests/gut_guard.sh` exit 0.

## Re-verification — 2026-11-01 at ef021a41d438d930559d9724006de56500388260

`ef021a41d` adds the pure static `SpatialVolume.edge_radius(box, world_scale)` to `spatial_volume.gd` (half the box diagonal, used as the scene edge for a finished agent, ADR-2109 / ADR-2135) and, in `graph_scene.gd`, two helpers that read the knowledge volume's `bounds` and global scale and one call that passes them to the choreography. Nothing in entering a volume, the portal ring, `fit_scale`, `fit_into` or the fit ease changes; the helpers only read. The decision holds unchanged. Evidence: the full GUT suite, 50 scripts, 552 tests, 542 passing, 7 failing, the same seven `test_memory_search` failures as the parent commit in the same harness; `tests/gut_guard.sh` exit 0.

## Re-verification — 2026-10-31 (`633423357`)

Re-verified 2026-11-01 after the inscribed-sphere edge change: `git diff df3e2aca5..6334233576941af20879a2b3d3d00b30ecb3c231 -- xr-client/scripts/spatial_volume.gd` replaces half the box diagonal with half its smallest extent (owner decision); this record's claims about volume bounds and agent edge placement hold.

## Re-verification — 2026-11-01 (`abcf36db1`, branch `viz/edge-tracking`)

Re-verified after the R23 fixes. `xr-client/scripts/graph_scene.gd` changes one doc comment (`_overlay_box`). Portals and volume entry are untouched. Every claim in this record holds. Evidence: `cargo test` in `xr-client/rust` (623 passed), server `cargo test --lib` (1781 passed, `CUDA_VISIBLE_DEVICES=1`), client vitest (1540 passed), GUT on a fresh `.so` (553 passed, 0 failed, `gut_guard` ok). Not run: the headset.

Re-verified again at `69b16867d`: A later commit adds only a `slots` argument to the `_tick_links` call in `graph_scene.gd`. Portals and volume entry are untouched. GUT on a fresh `.so`: 554 passed, 0 failed; `xr-client/rust` `cargo test`: 624 passed.

## Re-verification — 2026-11-04 (`5c94c3cbd`)

Re-verified after merging feat/splats (ADR-2170, Gaussian splat scenes). The governed-path changes since `69b16867d` are additive: `xr-client/scripts/graph_scene.gd`: the splat volume, built only when `XrPrefs.splat_scene()` is on (off by default) and kept out of VOLUME_BINDINGS, VOLUME_ORDER and GraphRoot; `xr-client/scripts/hud.gd`: a credits line shown only when a splat scene sets one, and an Enter/Leave Splat Scene button hidden until one is loaded; `xr-client/scripts/xr_prefs.gd`: the `xr.splat_scene` pref and `XR_SPLATS` env switch. This record's own claims are unchanged. Evidence on the merged tree: `cargo test --lib` 1823 passed, 1 failed (`nip98_proxy_tests::prod_lan_headset_wss_authenticate_validates_through_nginx`, which reads only `nginx.*.conf` and `auth.rs`, neither touched; it fails identically at `d74b25143`); `xr-client/rust` 699 passed; XR GUT 595 of 598 passing (3 pending pixel tests); vitest 1593 passed. Re-run: `git diff 69b16867d..5c94c3cbd -- <governed path>`.

Re-verified 2026-11-05 at `858cf136f` for ADR-2178: ADR-2178 adds a second entered state (`enter_merged`, `set_merged_scale`, `_apply_merged`) beside `enter`; `enter` is refused while merged, `portal_ids` is empty while merged, and leave restores the ring from the same snapshot and shows the volumes the merge hid. Also reviewed `ac0d68913` on these paths: the HUD Splat toggle leaves an entered splat space before hiding it (`leave_volume_space`), consistent with this record. GUT on the HP rig at this commit: 609/609 (`test_space_portals.gd` includes the merged round trip).

## Re-verification — 2026-11-05 (`81e711267`)

Governed-path changes since the last stamp, both `6b811f7a1`: `space_portals.gd` (`distance_to_box` returns world metres, rearm margin arrangement-scaled) and `graph_scene.gd` (topology generation change releases active grabs). The first resolves the open rearm-scale finding in the note above; the second does not touch portal layout, state or refusal logic. **Claims otherwise unchanged; text amended above.** Headless only; no stereo or headset evidence.

Re-run: `git diff 858cf136f..81e711267 -- xr-client/scripts/space_portals.gd xr-client/scripts/spatial_volume.gd xr-client/scripts/emulated_volume.gd xr-client/scripts/volume_arrangement.gd xr-client/scripts/graph_scene.gd xr-client/scripts/hud.gd xr-client/scripts/xr_prefs.gd xr-client/materials/portal_membrane.gdshader`. Read from the diff and the repair record; the repair's own test results are in that record and were not repeated here.

## Re-verification — 2026-11-07 (`7bd25427e`)

Governed-path changes since `81e711267`:
- `graph_scene.gd` and `hud.gd`: the ADR-2188 overview path, the ADR-2189 seven-section HUD, and the headset quality filter.
- `spatial_volume.gd`: `refit()` (`:156`) for the HUD Fit button.
- `xr_prefs.gd` (+72): numeric and flag keys for the label budget, text scale, dwell, haptics, comfort and colour mode.

The portal mechanics are untouched.
- The HUD **Enter**/**Leave** button is still on the Session page and still emits `volume_enter:<id>` / `volume_leave` (`hud.gd:1625-1636`, page `_build_session_page` `:1565`).
- The scene dispatches those actions at `graph_scene.gd:2305-2307`.
- The budget hook still calls `set_volume_weights(_portals.budget_weights())` behind `has_method` (`graph_scene.gd:1590-1591`), and `OTHER_WEIGHT := 0.15` is unchanged (`space_portals.gd:75`).
- `xr.portal_walkthrough` is still `KEY_WALKTHROUGH` (`xr_prefs.gd:25`, getter `:102`).

**Decision unaffected; status stays `partial`/`inactive`.** Re-run: `git diff 81e711267..7bd25427e -- <verified_paths>`. Checked by reading that diff; GUT was not re-run for this stamp.

## Re-verification — 2026-11-10 (`eba81532a`)

Governed-path changes since `7bd25427e` (slice 1 of the XR upgrade):
- `graph_scene.gd`: the dead edge re-rank is deleted, resume redials a down connection, a socket swap releases grabs (`_connect_graph` `:3022`), the quality toggle greys locally, and the build stamp is shown (`_show_build_stamp` `:2235`).
- `hud.gd`: the Session header row carries the build stamp (`:1568`).

The portal mechanics are untouched.
- The HUD **Enter**/**Leave** button is still on the Session page and still emits `volume_enter:<id>` / `volume_leave` (`hud.gd:1649`).
- The scene dispatches those actions at `graph_scene.gd:2317-2319`.
- The budget hook still calls `set_volume_weights(_portals.budget_weights())` behind `has_method` (`graph_scene.gd:1564-1565`).

**Decision unaffected; status stays `partial`/`inactive`.** Re-run: `git diff 7bd25427e..eba81532a -- <verified_paths>`. Checked by reading that diff; `scripts/xr-gate.sh` passed at `eba81532a` (both XR crates' fmt, clippy and tests; headless Godot 4.8-beta2 parse check and GUT, 676 of 679 passing, 3 pending for want of a GL renderer).
