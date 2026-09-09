---
id: ADR-2107
title: A shared XR visual language with reversible comfort and rendering budgets
date: 2026-09-07
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 82cbaf88b594f881def9f1f6e84bacf31274efa9
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
