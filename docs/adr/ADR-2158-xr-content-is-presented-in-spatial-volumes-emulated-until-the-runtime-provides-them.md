---
id: ADR-2158
title: XR content is presented in spatial volumes, emulated until the runtime provides them
date: 2026-10-30
decision_status: accepted
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 00669b7a2b8bca15702b8173df1dab268144c3ec
verified_paths: []
owner: jjohare
review_trigger: Godot 4.8 reaching stable or renaming the spatial-container API; SteamVR, Monado or Quest advertising XR_EXT_spatial_container; the first native-arm run on an Android XR device; the first headset session of trial E3; a fifth volume or a second client platform joining the arrangement
repo: visionclaw
domain: XR-client
depends_on: [ADR-2032, ADR-2135, ADR-2143, ADR-2159]
amends: [ADR-2135]
relates: [ADR-2141, ADR-2157, ADR-2164]
---

# ADR-2158 — XR content is presented in spatial volumes, emulated until the runtime provides them

Plan: `docs/proposals/spatial-rooms/PLAN.md` §4 (VisionClaw), §5, §6; evidence in `research/04-openxr-spatial.md`. Forum ADRs named in the plan belong to the nostr-rust-forum repository.

## Context
- One `GraphScene` holds one origin and one backend; knowledge, ontology and memory are three bodies placed by one layout inside it (ADR-2135). `_fit_graph_to_view` maps the longest axis to 2.4 m and centres on a fixed anchor (`xr-client/scripts/graph_scene.gd:1920-1951`, constants `:45-46`).
- The operator asked to trial three graphs, each in its own spatial container: knowledge, ontology and the Codescape estate.
- OpenXR ratified `XR_EXT_spatial_container` (#811, SDK 1.1.63, 2026-09-01). Godot integrates it from 4.8 as a single *main* container. We run 4.6.1 under Compatibility (Invariant 1 of `docs/XR-client.md`).
- Our runtime cannot provide it: SteamVR's `vrclient.so` on the HP has no `XR_EXT_spatial_*` string; no Monado merge request exists; Quest has made no commitment. Android XR is the only committed runtime.
- gdext has no 4.8 binding (we pin `godot = "0.2"`), and the Android guide's names already differ from Godot master.
- Anchor sharing between users has no EXT extension, only vendor ones.
- ADR-2135's review trigger names "a fourth body joining the separated layout".
- **Acceptance (operator, 2026-10-30):** "do the XR updates / Godot upgrades and bring in the new spatial containers". Godot 4.8 (beta) becomes the target engine; the upgrade and its Invariant-1 re-test are a separate record (ADR-2165, being drafted by the engine-gate agent), not this one.

## Decision
The XR client presents graph content in **spatial volumes** behind a GDScript contract shaped like `XR_EXT_spatial_container`, and runs emulated volumes everywhere until a runtime provides native ones.

1. **Contract.** `SpatialVolume` exposes bounds, bounds mode (`bounded` or `immersive`), visible, interactable and closed. Changes are requests, answered by granted or denied signals, and the runtime can change bounds at any time through a `bounds_changed` event. Payloads carry an app-level id in place of Godot's `RID`.
2. **Implementations.** `EmulatedVolume` (a `Node3D` with frame, clip and resize handles) works on every runtime and is the arm trial E3 runs on. Every graph volume is always emulated.
   - **Native container arm (decided at acceptance, 2026-10-30).** Godot 4.8 exposes **one main** spatial container per application, configured by project settings (`xr/openxr/extensions/spatial_container/{enabled, bounds_mode, bounds}`). The native arm puts the **whole scene** in that one container: the arrangement of emulated volumes becomes its child content, and the container's bounds and mode drive the arrangement's fit, in the same way a volume's bounds drive its content's fit. No graph volume is ever itself native.
   - **Switched by a VisionClaw setting.** The arm is controlled by the project setting `visionclaw/xr/native_spatial_container` (bool). It is **off by default**, honoured only on an Android export (`OS.has_feature("android")`), and **always off in desktop exports**, whatever the setting says. Godot reads its own spatial-container setting when OpenXR initialises, so this is a build-time choice, not a runtime toggle. An export that turns the arm on sets both settings through a feature-tag override.
   - **It never leaves the session idle.** The emulated arrangement is built first, every time. The native shim only re-hosts it. The shim falls back to plain emulated volumes, and records why, whenever any of these holds: the setting is off; the export is not Android; the engine is older than 4.8; `OpenXRSpatialContainerExtension` is not registered or not enabled; the runtime lacks `XR_EXT_spatial_container`; the container is not active within 3 s of the XR session starting; or the runtime later reports it closed. Fallback is immediate and needs no reconnect.
   - **Ships unverified.** No runtime we own supports it today. SteamVR on the HP has no `XR_EXT_spatial_*` string, Monado has no merge request and Quest has made no commitment. Android XR is the only committed runtime, and we have no Android XR device. The arm is built and covered by headless tests against a fake extension, but it has never run against a real container.
   - **Adapter.** It is a GDScript shim, not gdext, because gdext has no 4.8 binding. It reaches the extension by name (`ClassDB`, `Engine.get_singleton`), never by a static reference, so it still parses on 4.6. All Godot spatial-container names and enum values are confined to that one file: Godot's `BOUNDED = 0` and `IMMERSIVE = 1` differ from the spec's 1 and 2, and the Android guide's names differ from Godot master.
3. **A space is not a volume.** A volume is how one client presents content on its own device. A space may describe the volumes it shows; each client lays them out locally. **Frames.** A descriptor's frame is the home's own frame. Each consuming node applies its own placement transform, and no cross-node frame is shared in v1.
4. **Trial E3.** Three emulated volumes: (a) the knowledge graph and (b) the ontology graph, split by `NodeKind` from the one VisionClaw stream; (c) the Codescape estate graph, from a second connection (ADR-2159). The memory cloud stays world furniture.
5. **Fit rules.** A volume scales uniformly by `min_i(bounds_i / content_i)` over the axes, ignoring any axis whose content extent is below 1e-6, so anisotropic content fits on every axis. It centres at the volume origin, restores 1:1 in immersive mode, and un-latches and eases on `bounds_changed` (a resize can fire every frame). Expectations E7 to E9 in plan §6 are the acceptance tests: no drawn node centre outside its volume's box (one frame of excursion allowed during a resize); 90 Hz on the VIVE with the three volumes visible (about 4.4k nodes); Codescape `nodeDrag` refused locally and never sent.
6. **Cross-volume edges are optional and switchable (operator, 2026-10-30).** These are drawn as world-space inter-volume links only when `xr.cross_volume_links` is on:
   - knowledge-to-ontology bridge edges (about 56k links, ADR-2135);
   - code-to-concept links (ADR-2164);
   - and, later, links between local and federated data-space volumes (ADR-2161).

   **Defaults: on in dev builds, off in release.** "Dev" means `OS.is_debug_build()`, which covers the editor and debug exports; a release export is off. The XR menu toggles the switch at runtime without reconnecting. The operator's choice is stored per device in `user://xr_prefs.cfg`, section `xr`, under a key **per build type**: `cross_volume_links_debug` or `cross_volume_links_release`. A debug and a release install of one package share `user://`, so a choice made in a dev build never turns links on in a release build. The build default applies while the key for the running build type is absent. Turning the switch off removes the link geometry immediately, and while off it costs nothing in the frame budget. Links are drawn from data the client already holds, so the switch never changes what any server sends.
7. **Amends ADR-2135, for the XR client only.** The tri-layout triangle becomes the default *arrangement* of volumes rather than a per-node fold inside one origin. A user's move of a volume overrides it. The `visionclaw-tri-layout` crate, its fixture, the desktop client and the server's display projection are unchanged. Trial E3 itself fires ADR-2135's review trigger ("a fourth body joining the separated layout"): Codescape joins knowledge, ontology and the memory cloud. This ADR is the answer to that trigger.
8. **Later (P6), not part of E3:**
   - the native arm's first *verified* run (N1, on an Android XR device);
   - per-device persistence of the arrangement keyed by room (`XR_EXT_spatial_persistence`);
   - co-located alignment to a shared marker (`XR_EXT_spatial_marker_tracking`).

   The native arm is built now (decision 2) but is not part of E3's acceptance. No vendor anchor-sharing extension is adopted.
9. **E3 open items, settled at acceptance as defaults.** Each is a default the operator may revise without a new record; the phase-1 contract (`docs/proposals/spatial-rooms/XR-PHASE1-CONTRACT.md`) carries the numbers.
   - *Units:* a volume's content is fitted from graph units into the volume's metres by decision 5. No unit conversion happens anywhere else.
   - *Fourth body:* knowledge sits at −60°, ontology at +60° (the ADR-2135 vertices) and Codescape at 0° (front centre), all on one ring in front of the user. The memory cloud keeps 180° as world furniture.
   - *Yaw:* if the stream's positions carry the server's per-population vertex yaw, the volume that is fed the population cancels it on its content root. Whether `/wss` positions carry that yaw is checked first; it was not re-verified here.

Rejected alternatives:
- **Wait for native containers.** Nothing we own provides them, so the trial would never run.
- **Build native first on Godot 4.8.** Making native the primary arm would hinge E3 on hardware we do not have. Rejected at the proposal and still rejected. The operator has since approved 4.8 as the engine (ADR-2165 owns that re-test), so the native arm is now built, but only as an off-by-default wrapper around the emulated arrangement (decision 2), never in place of it.
- **One native container per graph.** Not possible: Godot creates a single main container per application.
- **Treat each space as a volume.** Placement would then be a server decision about a device-local presentation, and it cannot follow `bounds_changed`.
- **Adopt vendor anchor sharing** (`XR_META_*`, `XR_ANDROID_*`, `XR_BD_*`) for colocation. Each is single-vendor; the marker route is portable.

## Consequences
- **Easy.** The same contract runs headless, so the state machine, the fit maths and the native shim's fallback are all testable in CI. The native arm touches one shim file and the arrangement root, never a volume or the scene graph beneath it.
- **Hard.** Each volume is its own fit target, so `_fit_graph_to_view` generalises to the volume's `fit_into`. All visible volumes share one frame budget (ADR-2159). E7 forbids any drawn node centre outside its box, so a volume fits its content's **full** extent, not the 5th to 95th percentile box the single-scene fit uses. A grabbed node is exempt while it is held.
- **Foreclosed for now:**
  - more than one native container (a Godot limit);
  - a native container on the VIVE (SteamVR lacks the extension);
  - a runtime toggle for the native arm (Godot reads its setting when OpenXR initialises).
- **Settled at acceptance (decision 9).** These were open at the proposal:
  - *units:* the triangle constants are graph units (`crates/visionclaw-tri-layout/src/lib.rs:84-106`), and the volume fit maps them to metres;
  - *fourth body:* the memory cloud keeps the 180° vertex (`lib.rs:128`) and Codescape takes 0°;
  - *yaw:* the volume cancels it, after the check decision 9 names.
- **Unverified and shipping anyway:** the native arm (decision 2), and whether Godot's container path works under `gl_compatibility` (the Android guide assumes the Mobile renderer). Both are tested only against a fake extension until an Android XR device exists.
- **Revisit** if any of these happens:
  - a runtime on our hardware advertises the extension;
  - Godot renames the container API before 4.8 stable (only the shim changes);
  - the native path fails under `gl_compatibility`;
  - the operator drops inter-volume links.
- **Follow-on.** `docs/XR-client.md` moves this record from Proposed to being implemented. ADR-2135's text is not edited, and this record carries the `amends` edge. The phase-1 file ownership and APIs are in `docs/proposals/spatial-rooms/XR-PHASE1-CONTRACT.md`.

## Activation prerequisites
`depends_on` lists code dependencies only. ADR-2158 depends on ADR-2159, because its Codescape volume needs the second connection. ADR-2159 does not depend on this ADR (`AUDIT-DECISIONS.md` C9).
- Volume code may be written and tested headless in parallel with ADR-2159. It is wired into `graph_scene.gd` only after ADR-2159's two-client preflight has passed.
- Activation means trial E3 passing E7, E8 and E9. It needs nothing from ADR-2157, 2160 or 2161, and nothing from the native arm, which stays off by default and is not an activation criterion.

## Verification
`implementation_status: none`, `activation_status: inactive`. Accepted on 2026-10-30. Nothing is built.

**Implementation note, 2026-10-30 (spatial-volume UX, branch `xr/ux-wire` on `xr/spatial` `246008ff8`).** This adds interaction to the volumes, built on decision 1's contract. It does not change any decision.
- Trigger on a box frame focuses it. Trigger on a node in any visible box selects it through `SpatialVolume.select` and focuses the box. Trigger on a drawn link follows it to the far end and brings that box to the front.
- One-hand grip moves a box (decision 7: "a user's move of a volume overrides" the default arrangement), or resizes it from a corner handle through `handle_at` / `begin_resize` (decision 5's un-latch and ease apply). The two-hand gesture still moves the whole arrangement.
- `VolumeArrangement.bring_to_front` swaps ring slots, which are always a permutation of the default yaws, and eases over 0.35 s at unit scale. Reset Layout restores `reset_default(true)`. The native shim's fallback still calls `reset_default()` and keeps bounds.
- Drag stays knowledge-only, so E9 is unchanged: a press that picks a link, a frame or a node in another box keeps the Rust drag off that hand.
- The interactions table is in `docs/XR-client.md` (volumes section). The tests are `tests/unit/test_volume_ux.gd`, `test_volume_arrangement.gd` and `test_hud_tabs.gd`, green headless on the HP on 4.6.1 and 4.8-dev5. Not yet run in the headset, and the lead deploys. `implementation_status` and `activation_status` are left for the E3 session to set.

**Re-read at `00669b7a2b8bca15702b8173df1dab268144c3ec` for acceptance.** The single-scene, single-backend preconditions still hold:
- `xr-client/scripts/graph_scene.gd:45-46` (fit constants), `:166` (`_graph_ws_url`), `:479-480` (one `BinaryProtocolClient`, one `PresenceClientNode`), `:909-926` (`_switch_backend`), `:1515` (`_connect_from_env`) and `:1920-1951` (`_fit_graph_to_view`, which fits the 5th to 95th percentile box from `render_aabb(0.05, 0.95, …)`).
- `xr-client/project.godot:56` (`gl_compatibility`) and `:64` (OpenXR on).
- There is no `visionclaw/xr/*` setting, `SpatialVolume`, `EmulatedVolume` or spatial-container use in `xr-client/`.
- The only build-mode check in the scripts is `hud.gd:1594` (`OS.is_debug_build()`), and no `user://` preferences file exists besides memory search's.
- Triangle constants are unchanged at `crates/visionclaw-tri-layout/src/lib.rs:84`, `:106` and `:128`.

**Not re-run:** the runtime support claims, from `research/04-openxr-spatial.md`:
- the SteamVR string check dated 2026-10-30;
- the Monado merge-request search;
- Godot PR #123124 in milestone 4.8;
- the settings `xr/openxr/extensions/spatial_container/*`.

These claims age quickly, and the shim's first task on the ADR-2165 engine is to confirm the class and setting names.

**First read at `e74bcddc772d47a45b1971156a2bab540eaa027c`** (proposal), for history: the same citations, plus `xr_boot.gd:54-56` and tri-layout `lib.rs:110`, `:115`, `:132` and `:226`.
