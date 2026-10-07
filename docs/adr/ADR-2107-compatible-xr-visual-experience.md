---
id: ADR-2107
title: A shared XR visual language with reversible comfort and rendering budgets
date: 2026-09-07
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 944cba88cc1472319adcabeff337f1ea37a0ce08
verified_paths: [xr-client/scenes/GraphScene.tscn, xr-client/scenes/HUD.tscn, xr-client/scripts/spatial_environment.gd, xr-client/scripts/xr_theme.gd, xr-client/scripts/hud.gd, xr-client/scripts/radial_menu.gd, xr-client/scripts/dwell_reticle.gd, xr-client/scripts/agent_avatar.gd, xr-client/materials/spatial_floor.gdshader, xr-client/materials/edge_flow.gdshader, xr-client/tests/spatial_visual_fixture.gd, xr-client/tests/unit/test_xr_visual_accessibility.gd]
owner: jjohare
review_trigger: Headset acceptance, a renderer change, or a change to graph instance channels and world-radius compensation.
repo: visionclaw
domain: BASELINE-architecture
lineage: Extends ADR-2004 client-runtime baseline without changing the desktop validation or Quest shipping target.
---

# ADR-2107 — A shared XR visual language with reversible comfort and rendering budgets

## Context

The user authorised a substantial visual upgrade alongside the estate closeout. The existing client has real graph instancing, world-space panels, proximity labels and interaction semantics that must survive a visual revision. Compatibility rendering remains the established desktop OpenXR path. A desktop screenshot cannot establish binocular comfort, controller usability or mobile frame budgets.

## Decision

Use one navy/cyan instrument palette across the seven-tab HUD, radial menu and dwell indicator. Shared opaque panel surfaces, larger typography, consistent spacing and explicit hover/focus outlines provide contrast without bloom. Status text and existing interaction semantics remain visible; colour does not replace them. The radial backdrop is stationary and non-interactive.

Give the graph a restrained procedural sky and shadow-free cool key/warm fill. Preserve the opaque/faded MultiMesh split, per-instance colours, centrality/fold/query channels and four edge relation styles. Reduce fixed-purple emission and oversized node halos. Antialias semantic dashes with shader derivatives, correct the reversed smoothstep and dephase optional edge pulses by instance. Do not add screen-space effects, shadow maps, external textures or a new renderer dependency.

A stationary, metre-spaced grid supplies scale independently of GraphRoot. It is transparent, writes no depth, and has maximum opacity 0.35 so translating the graph beneath the ground does not hide it behind an opaque floor. Low-cost mode removes it. A single pooled, depth-tested billboard brackets the hovered or grabbed node at its actual world position; it expires after 350 ms without a refreshed target. Its diameter follows node-size preference rather than GraphRoot scale because production node buffers already compensate that scale to preserve world radius. It does not move the camera or scan every node each frame.

Reduced motion defaults on. It stops query/edge pulsing and agent bobbing while preserving status colours and badges. Balanced quality uses 2× MSAA on desktop viewports and the restrained halo pass. MSAA stays off whenever the viewport is an XR viewport (`Viewport.use_xr`): under the Compat renderer the OpenXR multiview swapchain cannot be multisampled, and forcing it leaves both eye framebuffers incomplete (`GL_INVALID_FRAMEBUFFER_OPERATION` every frame, black headset). Verified on VIVE Pro / SteamVR 2.16.7 with Godot 4.6.1 and 4.7.2 on 2026-09-08; this keeps the `project.godot` `msaa_3d=0` invariant recorded in XR-client.md. Low-cost quality disables MSAA, node halos, edge pulses and the ground grid. Help-page controls apply these choices immediately through the scene-local environment; material duplication makes toggling reversible without modifying shared resources. `XR_REDUCED_MOTION=0` opts into motion; `XR_VISUAL_QUALITY=low` seeds low-cost mode. Preferences currently last for the running scene; environment settings provide launch defaults.

## Source and verification

The implementation is in `xr-client/scenes/{GraphScene,HUD}.tscn`, `xr-client/scripts/{spatial_environment,xr_theme,hud,radial_menu,radial_backdrop,dwell_reticle,agent_avatar}.gd` and the node/edge/floor/focus materials. `graph_scene.gd` continues to own interaction and radius compensation. The verification commit records the implemented source revision validated below.

On 2026-09-07 the current native GDExtension was rebuilt with `cargo build --locked --offline --lib`. The previously copied September 5 binary exposed an obsolete three-argument `connect_to_url`; current source and GDScript both use two arguments. Validation uses the rebuilt library, not that stale artefact.

The integrated suite passed **83 tests and 314 assertions, with no risky tests**, under Godot 4.3, GUT 9.3.1 and actual GL compatibility rendering on the GUI sidecar's X display. Six stale test node paths were corrected to the current GraphRoot spawners and reparented HUD. The deterministic production-material fixture rendered under Godot 4.6.1, exercising 48 opaque nodes, one faded node, 60 edges and all four relation styles; its comfort assertions check reversible halo/MSAA/grid changes and stopped pulses. The HUD gallery renders all seven tabs plus radial controls.

Godot 4.6.1 conflicts with GUT 9.3.1's `Logger` class; the pinned 4.3 engine therefore runs GUT. Headless font metrics differ from actual GL and are not accepted as panel-fit evidence. A 4.6.1 headless editor import crashed in the engine; actual GL rendering succeeded. Initial receipts contained unset ViewportTexture and two texture-release diagnostics at shutdown. Commit `6dd349544` removed premature serialised texture references and binds the actual viewport textures in `_ready`; the final integrated suite and gallery verify removal of the unset-texture error. The final combined software-GL suite still reports two 349,524-byte texture-release diagnostics at process shutdown despite all assertions passing. Their cause remains unresolved; this record does not claim an error-free renderer log. The earlier diagnostics remain in historical receipts.

The XR workflow retains its existing GUT job identifier and pinned Godot 4.3/GUT 9.3.1 versions. It imports global script classes first, then runs the suite on Xvfb with software GL and dummy audio so the panel-fit gate uses rendered font metrics. Android export remains a separate advisory job.

## Acceptance boundary

Implementation is partial and activation staged until a fresh headset session checks stereo compositing, near/far text readability, translated/scaled graph focus, both comfort modes and controller/dwell operation against measured frame times. Quest packaging remains subject to the existing Android toolchain and device acceptance; this revision does not certify it. No live authenticated graph workload or headset was used for the new visual captures. Desktop software-display throughput is not a headset performance measurement.

## Re-verification — 2026-09-21 at 997440cd0717d4c5f9341369571fc69fcf5a38d6

**Governed change since `3eb2ffae5`:** `xr-client/scripts/hud.gd` +1/-4 — the
Agent Demo button's tooltip was reworded and `_mk_swarm_row` no longer prefixes
a synthetic agent's name with `[demo]`. Both are ADR-2109 D5 ("the synthetic
agents play as real agents") landing in the HUD.

**Decision unaffected.** Nothing in the palette, typography, spacing,
hover/focus outlines, MSAA-off-under-XR rule, reduced-motion default, ground
grid or focus bracket changed; the edit is two lines of row text inside the
Swarm page. The acceptance boundary (implementation partial, activation staged
until a fresh headset session) is unchanged and is *not* re-asserted here.
`verified_commit` moved to the CI-repair commit.

## Re-verification — 2026-10-07 at b6fbe772d (XR beat clock, memory bursts, attention heat; ADR-2134)

**Governed change:**
- `xr-client/materials/edge_flow.gdshader` (and `node_halo.gdshader`) gain a `beat_pulse` uniform: an alpha/emission swell on the beat, set per frame on the scene-local material duplicates by `beat_pulse.gd`.
- `xr-client/scripts/hud.gd` gains a Session-page Beat row (status, Tap, Mic, Bursts), all through `_press_fire` and the `xr_theme` styles, plus a header MIC badge and Key-tab rows for beam actions and burst verbs.

**Decision holds.** There is no screen-space effect, shadow map, texture or renderer dependency, and the swell is emission only. Reduced motion stops the beat swell entirely, as it stops the travelling edge and query pulses: the uniform is held at exactly 0, and only the HUD Beat readout shows the tempo (Rust `reduced_motion_stops_the_pulse_entirely`, GUT `test_relayed_beat_clock_drives_the_shared_shader_uniforms`). An interim 0.25 cap was withdrawn before merge. Low-cost mode removes the halo pass, and with it the halo swell. Panel fit is covered by GUT `test_no_page_overflows_its_host` and `test_session_beat_row_fires_on_press_and_fits` (123 pass on HP, headless metrics; the rendered-font check stays with the Xvfb CI job).

## Re-verification — 2026-10-07 at 39f580e93 (merged XR parity tree)

The merge of `feat/xr-cloud` (1c03ffb2a; it carries `feat/xr-graph`) brings these into one tree with the ADR-2134 work:
- xr-cloud's memory cloud and route layers (WP6/7);
- xr-graph's palette, settings sync, hulls and node LOD (WP1/2/4).

Those branches did not move this record's `verified_commit`, so the combined state is re-verified here. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 105 integration tests, and GUT on HP Godot 4.6.1 (`--xr-mode off`) passes 149 tests.

`hud.gd` adds xr-graph's domain and hull key rows and xr-cloud's Memory/Cloud toggles. No `Button.new()`/`CheckButton.new()` sits outside `_press_fire`. The new materials (`memory_route.gdshader`, cluster hull, impostor) are additive or unshaded geometry: no `SCREEN_TEXTURE`, depth texture or post-process ("glow" appears only as a uniform name and in comments). The page-fit tests pass. **Decision unaffected.** Reduced motion holds the beat swell at 0 (see above).

## Re-verification — 2026-10-07 at 944cba88c (xr-graph halo quads and edge LOD merged)

The merge of `feat/xr-graph` (944cba88c) brings in xr-graph's halo quad layer (`NodesHaloMulti`, `node_halo_quad.gdshader`), its edge LOD (near cylinders plus far camera-facing ribbons sharing `edge_flow_common.gdshaderinc`) and the avatar quaternion slerp. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 111 integration tests; GUT on HP Godot 4.6.1 (`--xr-mode off`) runs 161 tests, 158 passing and 3 GL-only tests pending headless.

The halo is now an unshaded, additive camera-facing quad and the far edges are ribbons, with no screen-space effect, texture or renderer dependency. Under reduced motion `spatial_environment.gd` zeroes `query_pulse_depth` on the halo material and `pulse_energy` on the cylinders. `NodeLod.sync_edge_params` copies the cylinder parameters to the ribbons every frame, so the ribbons stop pulsing too. The beat swell lives in `edge_flow_common.gdshaderinc` and `node_halo_quad.gdshader` and is held at 0 under reduced motion. **Decision unaffected.**
