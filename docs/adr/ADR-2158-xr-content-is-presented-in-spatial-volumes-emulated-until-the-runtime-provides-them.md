---
id: ADR-2158
title: XR content is presented in spatial volumes, emulated until the runtime provides them
date: 2026-10-30
decision_status: proposed
implementation_status: none
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: e74bcddc772d47a45b1971156a2bab540eaa027c
verified_paths: []
owner: jjohare
review_trigger: Godot 4.8 reaching stable; SteamVR, Monado or Quest advertising XR_EXT_spatial_container; the operator's decision on cross-volume edges; the first headset session of trial E3; a fifth volume or a second client platform joining the arrangement
repo: visionclaw
domain: XR-client
depends_on: [ADR-2032, ADR-2135, ADR-2143, ADR-2159]
amends: [ADR-2135]
relates: [ADR-2141, ADR-2157]
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

## Decision
The XR client presents graph content in **spatial volumes** behind a GDScript contract shaped like `XR_EXT_spatial_container`, and runs emulated volumes everywhere until a runtime provides native ones.

1. **Contract.** `SpatialVolume` exposes bounds, bounds mode (`bounded` or `immersive`), visible, interactable and closed. Changes are requests, answered by granted or denied signals, and the runtime can change bounds at any time through a `bounds_changed` event. Payloads carry an app-level id in place of Godot's `RID`.
2. **Implementations.** `EmulatedVolume` (a `Node3D` with frame, clip and resize handles) works on every runtime and is the only arm built for trial E3. `NativeMainVolume` adapts Godot's `OpenXRSpatialContainerExtension` and is used only when Godot is 4.8 or later *and* the runtime advertises the extension. Godot exposes one main container per application, so at most one volume is native. The adapter is GDScript, not gdext, because gdext has no 4.8 API binding.
3. **A space is not a volume.** A volume is how one client presents content on its own device. A space may describe the volumes it shows; each client lays them out locally. **Frames.** A descriptor's frame is the home's own frame. Each consuming node applies its own placement transform, and no cross-node frame is shared in v1.
4. **Trial E3.** Three emulated volumes: (a) the knowledge graph and (b) the ontology graph, split by `NodeKind` from the one VisionClaw stream; (c) the Codescape estate graph, from a second connection (ADR-2159). The memory cloud stays world furniture.
5. **Fit rules.** A volume scales uniformly by `min_i(bounds_i / content_i)` over the axes, ignoring any axis whose content extent is below 1e-6, so anisotropic content fits on every axis. It centres at the volume origin, restores 1:1 in immersive mode, and un-latches and eases on `bounds_changed` (a resize can fire every frame). Expectations E7 to E9 in plan §6 are the acceptance tests: no drawn node centre outside its volume's box (one frame of excursion allowed during a resize); 90 Hz on the VIVE with the three volumes visible (about 4.4k nodes); Codescape `nodeDrag` refused locally and never sent.
6. **Cross-volume edges are optional and switchable (operator, 2026-10-30).** Knowledge-to-ontology bridge edges (about 56k links, ADR-2135), and links between local and federated data-space volumes (ADR-2161), are drawn as world-space inter-volume links only when `xr.cross_volume_links` is on. It defaults to **off**, can be toggled at runtime from the XR menu without reconnecting, and turning it off removes the link geometry immediately (it costs nothing in the frame budget while off). Links are drawn from the client's existing data; the switch never changes what the server sends.
7. **Amends ADR-2135, for the XR client only.** The tri-layout triangle becomes the default *arrangement* of volumes rather than a per-node fold inside one origin. A user's move of a volume overrides it. The `visionclaw-tri-layout` crate, its fixture, the desktop client and the server's display projection are unchanged. Trial E3 itself fires ADR-2135's review trigger ("a fourth body joining the separated layout"): Codescape joins knowledge, ontology and the memory cloud. This ADR is the answer to that trigger.
8. **Later (P6), not part of E3.** Native arm N1 on Godot 4.8 stable with Android XR; per-device persistence of the arrangement keyed by room (`XR_EXT_spatial_persistence`); co-located alignment to a shared marker (`XR_EXT_spatial_marker_tracking`). No vendor anchor-sharing extension is adopted.

Rejected alternatives:
- **Wait for native containers.** Nothing we own provides them, so the trial would never run.
- **Build native first on Godot 4.8.** Reopens Invariant 1 (the Compatibility multiview re-test) on a beta engine, for hardware we do not have.
- **Treat each space as a volume.** Placement would then be a server decision about a device-local presentation, and it cannot follow `bounds_changed`.
- **Adopt vendor anchor sharing** (`XR_META_*`, `XR_ANDROID_*`, `XR_BD_*`) for colocation. Each is single-vendor; the marker route is portable.

## Consequences
- **Easy:** the same contract runs headless, so the state machine and fit maths are testable in CI; the native arm later changes one adapter, not the scene.
- **Hard:** each volume is a separate fit target, so `_fit_graph_to_view` is generalised to `fit_into(volume)`. The frame budget is shared across visible volumes (ADR-2159).
- **Foreclosed for now:** more than one native volume (a Godot limit); native volumes on the VIVE.
- **Not decided here, and the plan does not settle them (they need an owner before E3 starts):**
  - *Units.* The triangle constants are graph units (`SEPARATION` derives from `LIVE_GRAPH_RADIUS` 152, `crates/visionclaw-tri-layout/src/lib.rs:84-106`). The plan says metres. A metre arrangement needs a per-volume fit step to map one to the other.
  - *Fourth body.* E3 has three volumes and the memory cloud, which currently sits on the 180 degree vertex (`lib.rs:128`). Which body takes which vertex is unspecified.
  - *Yaw.* The server's display projection still yaws each population by its vertex angle. Per-volume recentring absorbs translation, not that yaw, so the XR client must either request the unprojected positions or undo the yaw per volume.
- **Revisit** if a runtime on our hardware advertises the extension, if Godot's container path fails under `gl_compatibility` (unverified; the Android guide assumes the Mobile renderer), or if the operator drops inter-volume links.
- **Follow-on:** `docs/XR-client.md` carries a proposed-section entry. ADR-2135's text is not edited; this record carries the `amends` edge.

## Activation prerequisites
`depends_on` lists code dependencies only. ADR-2158 depends on ADR-2159 (its Codescape volume needs the second connection), and ADR-2159 does not depend on this ADR (`AUDIT-DECISIONS.md` C9). The volume work can proceed once ADR-2159's two-client test has passed. Trial E3 needs nothing from ADR-2157, 2160 or 2161.

## Verification
`implementation_status: none`. At `e74bcddc772d47a45b1971156a2bab540eaa027c` the preconditions were read:
- Single scene, single backend, fixed fit: `xr-client/scripts/xr_boot.gd:54-56` loads one `GraphScene`; `graph_scene.gd:166` (`_graph_ws_url`), `:1920-1951`, `:45-46`.
- Triangle constants: `crates/visionclaw-tri-layout/src/lib.rs:84` (`LIVE_GRAPH_RADIUS`), `:106` (`SEPARATION`), `:110` and `:115` (memory scale and distance), `:128` (`VERTEX_ANGLES_DEG`), `:132` (`Vertex`), `:226` (`TriangleFrame::separated`).
- No `SpatialVolume`, `EmulatedVolume` or OpenXR spatial-container use exists in `xr-client/`.
- Runtime support claims (SteamVR string check dated 2026-10-30, Monado merge request search, Godot PR #123124 in milestone 4.8) are from `research/04-openxr-spatial.md`; they were not re-run for this ADR and age quickly.
