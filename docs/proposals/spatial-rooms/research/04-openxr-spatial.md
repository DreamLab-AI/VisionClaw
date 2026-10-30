# 04 — OpenXR spatial containers in Godot, and a VisionClaw trial

Worker `openxr-spatial`, swarm-1793348706856-n5upgb. Research only. Date 2026-10-30.

## Headline findings (read these first)

1. **The extension is `XR_EXT_spatial_container` (ext #811), with `XR_EXT_spatial_container_self_rendering` (#814).** Both are ratified multi-vendor EXT extensions, spec revision 1, last modified 2026-08-06, and first published in OpenXR SDK 1.1.63 (2026-09-01). They are not part of the `XR_EXT_spatial_entity` family. They are a separate "OpenXR Multi-App" windowing model.
2. **Godot gives an app exactly one container.** The spec lets an app create many (`maxSpatialContainerCount`). Godot's integration (PR #123124, milestone **4.8**, merged 2026-09-08) creates a single *main* spatial container from project settings. The class is a singleton, there is one `bounds` setting and one `bounds_mode` setting, and Godot swaps its play space for the container space. Android's Godot guide says: "Your entire active 3D scene is placed inside a single primary spatial container." **Three graphs in three native containers is not possible in Godot today.** The trial therefore has two arms. **N1** is one native container holding the whole scene, and runs on Android XR only. **E3** is three emulated containers in the app, and runs on any runtime.
3. **Our hardware cannot run it natively.** SteamVR on HP has no support: its Linux runtime carries no `XR_EXT_spatial_*` strings at all, with the build from 2026-10-25 checked today. There is no Monado merge request. Quest 3 / Horizon OS has made no commitment. Android XR (Spatial API level 4+) is the only committed runtime. Even if SteamVR adds the extension, a VR-only compositor would most likely report immersive-only (see §2). **On the VIVE, bounded volumes will only ever come from emulation.**
4. **The engine requirement is Godot 4.8-dev**: master's `version.py` reads 4.8 "beta", and the latest stable is 4.7.2 (2026-08-18). We run 4.6.1-stable. The jump is two minor versions to a beta. That re-opens Invariant 1 (the Compat multiview re-test on SteamVR/Linux/NVIDIA). It also pulls in PR #116424, which breaks `XRInterface` extensions, and the vendors plugin moves to 5.2 (rc1, 2026-10-21).
5. **Spatial entities, anchors, persistence, planes and markers are already in Godot 4.6**, via PR #107391 (merged 2025-09-28). They work on our current engine, with or without containers. Sharing anchors between users has no EXT extension; it is vendor-only.
6. **Most of the trial's code cost is unrelated to containers.** Showing Codescape *alongside* the knowledge graph needs two concurrent backend connections. Today `_switch_backend` (`xr-client/scripts/graph_scene.gd:909`) is one-at-a-time, and caps, presence and the graph WebSocket are all global. The KG + ontology split is cheap: both already arrive on one stream with per-node kind flags.

---

## 1. What a spatial container is

**Semantics** (spec §12.50.1, registry 1.1): spatial containers "are analogous to the concept of 'windows' in desktop 2D windowing systems… Spatial containers are like windows extended to 3D. The user is able to see multiple spatial containers simultaneously and provides input to a subset of them. The visible spatial containers are able to belong to different apps and runtimes manage their size and placement on behalf of the user." It is shared-space multitasking: the runtime owns placement and size, and the app owns the content.

**Session model change** (§12.50.2 note, §12.50.11). Enabling containers means chaining `XrSessionCreateInfoSpatialContainersEXT` into `xrCreateSession`. The session then "remains in `XR_SESSION_STATE_IDLE` for its active lifetime" and is effectively headless. Visibility and focus move to each container. `XrEventDataSpatialContainerInteractableChangedEXT` "replaces `XR_SESSION_STATE_FOCUSED`".

**Lifecycle** (§12.50.2):
1. `xrCreateSpatialContainerEXT`. A new container is "not visible, not interactable, and in bounds mode BOUNDED".
2. Set the initial state, such as the bounds mode.
3. `xrRequestSpatialContainerVisibleEXT(true)`.
4. Wait for `VisibleChanged(visible=true)`. The runtime may defer this, and may show the container up to about 0.5 s later.
5. Render.

Teardown runs the same steps in reverse: hide, wait, end rendering, destroy. The runtime can queue `XrEventDataSpatialContainerClosedEXT` at any time, for example when the user presses "close" or at `LOSS_PENDING`. The app must then stop and destroy the container.

**State** (`XrSpatialContainerStateEXT`) has three fields:
- `visible`: the app requests it; the runtime may deny the request (`…VisibleRequestDenied`) or change it at will once the app has asked once.
- `interactable`: **the runtime alone controls it**. "An app cannot request a spatial container be interactable." Several containers may be interactable at once.
- `boundsMode`.

**Bounds modes** (`XrSpatialContainerBoundsModeEXT`):
- `BOUNDED = 1`: "a finite box. The spatial container will only appear in this region."
- `IMMERSIVE = 2`: "infinite bounds… displayed over the entire screen". The runtime must treat it as a normal 1.x full-screen app, and may hide other containers while it is visible.

The app cannot set the box size. `suggestedBounds` at creation is a hint the runtime may ignore. After that, "an app cannot request changes to bounds other than changing between bounded and immersive". The runtime may resize at any time. While the user drags the container, the runtime either queues bounds-changed events at frame rate or a single event at the end, and it coalesces unpolled events. A resize can move the container's local origin together with the bounds. Mode changes are asynchronous and can take "many seconds", because the runtime may show a safety prompt.

**Container space** (§12.50.4) has its origin at the centre of the bounds, with +X right, +Y up and +Z towards the front face. It is "a non-world locked origin". The runtime may re-pose it at any time, for example when the user moves the window. For a world-locked origin in immersive mode, the spec suggests using `LOCAL`. Godot PR #123736 (merged 2026-09-24) does exactly that: in immersive mode it switches back to the normal play space.

**System properties** (`XrSystemSpatialContainerPropertiesEXT`): `maxSpatialContainerCount`, `supportsBounded`, `supportsImmersive`. A runtime must support at least one of the two modes. The spec also says: "highly constrained devices may support only one spatial container being displayed at a time".

**Self-rendering vs other presentations.** `XR_EXT_spatial_container` deliberately defines no rendering method. `XR_EXT_spatial_container_self_rendering` is the **only presentation type defined so far** (`XR_SPATIAL_CONTAINER_GRAPHICS_PRESENTATION_SELF_RENDERING_EXT`). The app renders its own pixels into ordinary swapchains and composition layers:
- `xrBeginSpatialContainerRenderingEXT` / `xrEndSpatialContainerRenderingEXT` replace `xrBegin/EndSession` and add the container to the active render set.
- Each frame, `xrLocateSpatialContainerViewsEXT` returns views plus `shouldSubmitLayers` and `recommendedImageExtent`.
- `xrEndFrame` chains `XrSpatialContainerLayerFrameEndInfoEXT`, which carries per-container layers and `retainPreviousSubmission`, so an idle container can keep its last frame and skip re-rendering.
- `XrSpatialContainerLayerVolumeClippingEXT` selects `NONE` or `STRICT` clipping to the bounding volume.
- The spec says this "is intended to offer a superset of… core composition layer rendering", and that **1.x runtimes with graphics must set `supportsImmersive`**.

No runtime-rendered or scene-graph presentation (visionOS/RealityKit style) is defined yet.

**Godot surface** (master; docs at `docs.godotengine.org/en/latest`, which is the unstable channel):
- `OpenXRSpatialContainerExtension` (inherits `OpenXRExtensionWrapper`) has `is_enabled`, `is_spatial_container_active`, `get_supported_bounds_modes`, `get_spatial_container_state`, `get_spatial_container_bounds`, `request_spatial_container_bounds_mode`, `request_spatial_container_visible`.
- Its signals are `spatial_container_bounds_changed(rid, infinite, mode, Vector3 bounds)`, `…bounds_mode_request_denied`, `…closed`, `…interactable_changed`, `…visible_changed` and `…visible_request_denied`.
- `OpenXRSpatialContainerState` (RefCounted) has `get_bounds_mode`, `is_interactable`, `is_visible`, and `BoundsMode { BOUNDED=0, IMMERSIVE=1 }`. Note that Godot's enum values are not the spec's values, which are 1 and 2.
- `OpenXRSpatialContainerSelfRenderingExtension` only has `is_enabled`.
- Project settings are `xr/openxr/extensions/spatial_container/{enabled=false, bounds_mode=0, bounds=Vector3(0.5,0.5,0.5)}`. The self-rendering extension is gated under the same flag (`register_types.cpp:236-240`).
- `OpenXRSpatialComponentPolygon2DList` is **not** a container class. It belongs to the spatial-entity query API (`OpenXRSpatialEntityExtension.query_snapshot`), marked experimental, and returns plane boundary polygons.

Follow-ups: PR #123418 fixed a camera offset (2026-09-17). PR #124382 (2026-10-14) fixed window positioning and a first-frame `xrEndFrame` "Mismatch between active containers and submitted containers" error.

**Naming trap:** `XR_FB_spatial_entity_container` (#200) is unrelated. It is Meta's grouping of scene anchors, not a window.

## 2. Runtime availability matrix

| Runtime / device | `XR_EXT_spatial_container` | Evidence / note |
|---|---|---|
| **Android XR** (Galaxy XR etc.) | **Committed; documented** | Godot guide (updated 2026-10-28) requires Godot 4.8-dev, vendors plugin ≥ 5.2, a device with Spatial API Level 4, **Full Space Mode only** (`XR_ACTIVITY_START_MODE_FULL_SPACE_MANAGED`), alpha-blend environment and a transparent viewport background. Google is the spec lead (Jared Finder). The guide does not mention emulator support. |
| **Meta Quest 3 / Horizon OS** | **No commitment** | Meta engineers are spec contributors, but UploadVR (2026-10-26) says Meta "did not explicitly commit". Godot's editor gained a `horizonos` flavour of `GodotSpatialContainerGame` in PR #123124, which is plumbing, not a runtime. |
| **SteamVR (Linux, HP / VIVE)** | **Absent** | **Primary check (2026-10-30):** on HP, `SteamVR/bin/linux64/vrclient.so` (build 1792185270, mtime 2026-10-25) contains `XR_EXT_hand_tracking` but no `XR_EXT_spatial_container`, `XR_EXT_spatial_entity` or `XR_EXT_spatial_anchor` string. A secondary report (pyopenxr issue #148) says the Windows SteamVR beta also lacks it. Valve is a spec contributor, which is not an implementation commitment. |
| **Monado** | **No merge request found** | The freedesktop GitLab MR search for "spatial_container" returns 0 results (2026-10-30). Source-tree confirmation was not possible (API 401/404), so this is "not found", not "confirmed absent". |
| **Pico / ByteDance** | **No commitment** | ByteDance engineers are contributors. Pico OS 6 has a similar but proprietary concept. Godot added a `picoos` flavour as plumbing. |
| **Meta desktop runtime** | Absent (secondary, pyopenxr #148) | — |

**Why the VIVE will not get bounded mode even later.** The self-rendering spec requires conventional 1.x graphical runtimes to set `supportsImmersive`. The bounds section notes that any runtime can implement plain 1.x as "implicitly creating a spatial container, setting the container's bounds mode to immersive". A VR-only compositor such as SteamVR, with no shared space of other apps, would most plausibly ship immersive-only support. That gives us nothing we lack today. Bounded multitasking is a mixed-reality, shared-space feature.

**Fallback / emulation path (recommended): an app-level `SpatialVolume` contract.** It has the same shape as the extension, so the native implementation can be swapped in later:
- **State**: `bounds: Vector3`, `infinite: bool`, `mode: {BOUNDED, IMMERSIVE}`, `visible`, `interactable`, and a `Transform3D` pose in the play space.
- **Signals**: the same names as Godot's (`bounds_changed(id, infinite, mode, bounds)`, `visible_changed`, `interactable_changed`, `closed`, `*_request_denied`). Payloads carry an app-level `id`, standing in for the `RID`.
- **Requests**: `request_visible(bool)` and `request_bounds_mode(mode)`, both asynchronous. In the emulated version, a "volume manager" grants them, playing the runtime's role. Resizing and moving are done by the user (grab the corner or frame handles), never by the content. That matches the spec's rule that the runtime, not the app, sets the box size.
- **Backends**:
  - `EmulatedVolume`: a Node3D with a frame, clip and resize handles. Used on SteamVR, Monado and Quest.
  - `NativeMainVolume`: wraps `OpenXRSpatialContainerExtension`. Used on Android XR + Godot 4.8 when `is_enabled()` is true, for **one** volume only.
- **Degradation**: when the extension is absent, `NativeMainVolume` reports `is_enabled()==false` and the manager falls back to emulated volumes. Content code never branches on which backend is in use.

## 3. Mapping onto VisionClaw concepts

- **A graph is a container's content, not the container.** A container is a runtime-placed window on one device. The emulated `SpatialVolume` is the right unit for "one graph, one volume": knowledge, ontology and Codescape estate.
- **A NIP-29 room/space is not a container.** A room is shared session state: membership, presence and the signed event stream (`crates/visionclaw-xr-presence/src/room.rs`). A container is how *one client* presents a room or a graph on its own device. Container placement is deliberately device-local and runtime-owned, because the spec lets the runtime re-pose it at any time. It therefore cannot carry shared layout on its own. One reasonable mapping: a room *describes* a set of volumes (graph id, preferred bounds, relative arrangement), and each client lays them out locally.
- **Sharing a room layout across co-present users needs anchors, not containers.**
  - Available now in Godot 4.6 (our engine), from PR #107391: `XR_EXT_spatial_entity`, `XR_EXT_spatial_anchor`, `XR_EXT_spatial_persistence` (+ `_operations`), `XR_EXT_spatial_plane_tracking`, `XR_EXT_spatial_marker_tracking`. Godot has built-in anchor, plane and marker trackers (the `xr/openxr/extensions/spatial_entity/*` settings: `enable_builtin_anchor_detection`, `enable_persistent_anchors`, `marker_tracking/*`).
  - **There is no EXT for sharing anchors between users.** The options are vendor extensions: `XR_META_colocation_discovery` (#572), `XR_META_spatial_entity_sharing` (#291) and `_group_sharing` (#573) on Quest; `XR_ANDROID_anchor_sharing_export` (#702); `XR_BD_spatial_anchor_sharing` (#392).
  - The portable route is **alignment to a shared marker**: each device detects the same ArUco or AprilTag with `XR_EXT_spatial_marker_tracking` and publishes room-layout offsets relative to it over the NIP-29 room.
  - On the VIVE, SteamVR exposes none of these (the strings check above), so co-location there means manual alignment or Lighthouse-shared play-space calibration.
- **Persistence**: `XR_EXT_spatial_persistence` would let one device restore its own volume arrangement between sessions, keyed by room. That is single-user persistence, not sharing.

## 4. Trial design — "spatial containers for the dual KG + Codescape"

### How a graph scene is bounded today
- One `GraphRoot` holds everything. `_fit_graph_to_view` (`xr-client/scripts/graph_scene.gd:1920-1950`) takes a robust 5–95 % AABB from Rust (`render_aabb`). It maps the longest axis to `GRAPH_TARGET_SPAN = 2.4 m`, centres the result at `GRAPH_ANCHOR = (0, 1.3, −1.6)` (`graph_scene.gd:45-47`), refits every 15 frames with ease 0.06, and latches after `GRAPH_REFIT_SETTLE_SEC = 6`. A two-hand gesture sets `_manual_transform`, which takes permanent authority.
- Knowledge and ontology arrive on **one** binary stream and are told apart per node by flags (`xr-client/rust/src/binary_protocol.rs:43-94`: `KNOWLEDGE_NODE_FLAG 0x4000_0000`, `ONTOLOGY_*`; `NodeKind` → 0 knowledge, 1 ontology). The ADR-2135 triangle comes from the shared `visionclaw-tri-layout` crate: `TriangleFrame::separated()`, vertices at −60° (knowledge), +60° (ontology) and 180° (memory), `SEPARATION` from `LIVE_GRAPH_RADIUS 152` × `CLEARANCE 1.25` (`crates/visionclaw-tri-layout/src/lib.rs:84-128`). The cloud folds each node into its nearest graph's frame (`cloud_frame.rs`, `docs/XR-client.md:390-398`).
- Codescape is a **separate backend** reached through a mode switch: `XR_BACKEND_MODE=codescape`, port 3001 `/codescape/` (`docs/XR-client.md:937-990`). `_switch_backend` replaces both WebSocket URLs and reconnects (`graph_scene.gd:909-927`). `BackendCaps.for_mode` gates actions globally (`scripts/backend_caps.gd:98`). Codescape drops `nodeDrag`.
- Avatars, effects, HUD and labels sit on unit-scale roots outside `GraphRoot` (Invariant 8, ADR-2109).

### Arms
- **E3 (main arm, runs on VIVE / SteamVR, Monado, Quest):** three emulated `SpatialVolume`s, each about 1 × 1 × 1 m by default and user-resizable. (a) The knowledge graph and (b) the ontology graph are both fed from the one VisionClaw stream and split by `NodeKind`. (c) The Codescape estate (~4.4 k nodes) is fed from a second concurrent connection. The memory cloud stays outside all volumes as world furniture, or becomes an optional fourth volume.
- **N1 (native, Android XR only, later):** the main container holds the whole E3 arrangement (or one graph). This tests the runtime-driven `bounds_changed` → re-fit path and the bounded↔immersive switch. It needs Godot 4.8-dev, vendors plugin 5.2 and an Android XR device. Do not start it until 4.8 is stable, or until someone deliberately takes on a beta branch.

### Code changes (E3)

**Cheap — KG / ontology split:**
- `xr-client/rust/src/render_store.rs` / `render_store_pack.rs`: per-kind packing. Emit separate MultiMesh buffers, or index ranges, for kind 0 and kind 1, plus `render_aabb_kind(kind, lo, hi, exclude)`. Edges that cross kinds (knowledge ↔ ontology bridges) need a rule: draw them between volumes in world space as "inter-volume links", or drop them in E3. This is an operator decision.
- New `xr-client/scripts/spatial_volume.gd` (contract + `EmulatedVolume`) and `volume_manager.gd`, which grants visibility and mode requests, tracks interactable state, and owns the arrangement.
- `graph_scene.gd`:
  - Split `GraphRoot` into a `GraphRoot` per volume (`NodesMulti`, `EdgesMulti`, halo and LOD impostors each).
  - Generalise `_fit_graph_to_view` into `fit_into(volume)`: scale by the **smallest** bounds dimension, as the Android guide advises; origin at the volume centre; restore 1:1 in immersive. Un-latch on `bounds_changed`, and ease rather than snap, because a resize can fire at frame rate.
  - Clamp `_manual_transform` to the volume, or reinterpret it as moving the volume rather than the content.
- `TriangleFrame` gains a role: the default *arrangement* of the volumes (the same −60° / +60° / 180° placement in metres), not a per-node fold. A user's move of a volume overrides it.
- Label pool (`_update_proximity_labels`, `graph_scene.gd:678`) and selection rays: route hits to the volume the ray enters first. Labels are world-space, so they need per-volume clipping or culling.

**Expensive — Codescape alongside:**
- A second `BinaryProtocolClient.create()` (`graph_scene.gd:479` builds a single one) with its own WebSocket, acknowledgement sequence (ADR-2141) and render store. Check that the gdext statics and the tokio runtime (`rust/src/runtime.rs`, `transport.rs`) allow two instances.
- Capabilities per volume: change `backend_supports()` / `_gate()` (`graph_scene.gd:858-864`) to `volume.backend_caps`, so drag works in the KG volume and is refused in the Codescape volume.
- Presence: either one presence socket (the VisionClaw room) with Codescape presence ignored, or two. Decide which before building. Identity is NIP-98 to both (Invariant 6).
- Palette per volume: `apply_backend_palette` is global today (Codescape colour mode 2, `rust/src/domain_palette.rs`).
- HUD: the Graph, Forces and Pins tabs must target the *interactable* (focused) volume.

**Native hook (N1 only):** `native_main_volume.gd` adapts `OpenXRSpatialContainerExtension` to the contract. Do it in GDScript, not gdext (see Risks).

### Success metrics
| Metric | Target | How |
|---|---|---|
| Bounds respected | 0 drawn node centres outside their volume's box; ≤ 1 frame excursion during resize | Per-frame assert in a debug pass; GUT on the fit maths |
| Frame rate | 90 Hz on VIVE with three volumes: KG + ontology (live) + Codescape (~4.4 k nodes) | Compare against the `docs/XR-client.md` frame-budget tables (§"Node, halo and edge LOD", the 13 k / 32-hull baselines); triangle budget unchanged or explicitly re-budgeted |
| Interaction routing | 100 % of ray hits, grabs, drags and HUD actions reach the volume under the ray; Codescape `nodeDrag` refused locally with the HUD notice, never sent | Scripted ray fixtures (GUT) + VIVE session log |
| Re-layout on bounds change | New fit converges ≤ 0.8 s after a resize ends; no jitter while a resize streams events at frame rate | Synthetic `bounds_changed` sequences (headless) + manual corner drag on VIVE |
| Lifecycle | closed → teardown with no leaked MultiMesh or socket; hidden volume stops packing (CPU drops) | GUT + `perf/` pack timing |
| (N1) Native parity | The same contract tests pass against `NativeMainVolume` on Android XR; bounded↔immersive round trip | Android XR device only |

### What can be validated where
- **Headless (CI, `cargo test -p visionclaw-xr-gdext`, GUT `--headless`):** the contract state machine (request, grant or deny, deferred events, coalesced bounds), the fit maths (smallest-axis scale, origin, immersive 1:1), the per-kind AABB and packing split, routing ray fixtures, and two `BinaryProtocolClient`s against the captured Codescape frames (`tests/codescape_wire.rs`) plus VisionClaw frames.
- **Monado simulated HMD on Linux:** E3 in a real OpenXR session — stereo submission under Compatibility, frame time, input routing with the simulated controllers. It does **not** exercise native containers, since Monado lacks the extension.
- **VIVE / SteamVR (HP):** the 90 Hz target, comfort, resize ergonomics with real controllers, and legibility at volume scale (a 1 m volume is about 2.4× smaller than today's 2.4 m fit; `NODE_WORLD_RADIUS` holds node size, but density rises).
- **Android XR hardware only:** N1. Whether the Android XR emulator supports Spatial API level 4 is unverified; check before relying on it.

## 5. Risks

1. **API instability, already visible.** The Android guide (2026-10-28) uses names that do not match Godot master: `spatial_container_bounds_mode_changed`, `…visibility_changed`, `…interactability_changed`, `request_main_spatial_container_bounds_mode`, and `OpenXRSpatialContainerState.IMMERSIVE`. Master has `bounds_changed` (with the mode as a parameter), `visible_changed`, `interactable_changed`, `request_spatial_container_bounds_mode` and `BOUNDS_MODE_IMMERSIVE`. Expect more renames before 4.8 stable. The docs are the `latest` (unstable) channel. Mitigation: confine all native calls to one adapter file.
2. **How script reaches the extension is unconfirmed.** The guide calls `OpenXRSpatialContainerExtension.is_enabled()` as if it were a global singleton. `register_types.cpp` only `GDREGISTER_CLASS`es it, and the C++ singleton is internal; no `Engine.add_singleton` was seen. Verify on a 4.8-dev build before writing the adapter.
3. **gdext bindings.** We pin `godot = "0.2"` (lock: 0.2.4). The latest gdext is v0.5.5 (2026-08-09), and its `api-4-*` features stop at **4-7**. Binding 4.8 classes from Rust would need a 0.2 → 0.5 migration *and* `api-custom` codegen from a 4.8 `extension_api.json`. **Recommendation: keep the container adapter in GDScript.** The emulated contract can live in Rust/GDScript as normal.
4. **Engine bump 4.6.1 → 4.8 (beta).**
   - Invariant 1 requires the Compatibility multiview re-test on SteamVR/Linux/NVIDIA 580 before any renderer-adjacent change.
   - PR #116424 (multi-layer projection camera, 4.8) changes XR camera timing and deprecates `get_transform_for_view` / `get_projection_for_view`, which breaks `XRInterface` extensions.
   - The vendors plugin moves to 5.2 (rc), GUT moves line (9.6.1 targets 4.6), and `config/features` must be re-pinned in the editor (still "4.3").
   - **Unverified whether Godot's container path has been exercised under `gl_compatibility`.** The Android XR guide assumes the Mobile renderer. Flag this for N1.
5. **Session-state assumptions.** In container mode the session stays IDLE and there is no FOCUSED; interactable is per container. Godot has run `xrSyncActions` only while focused since PR #107619. Our scripts do not hook session-state signals directly (no hits in `xr_boot.gd` / `graph_scene.gd`), but input loss in N1 is plausible if Godot's focus gating does not map interactable onto it. Test it first.
6. **Clipping cuts through our overlays.** In bounded mode the runtime clips geometry outside the box (strict clipping). The HUD, labels, avatars and memory cloud sit on unit-scale roots in the same scene and would be clipped in N1. They need to move inside or be redesigned as in-volume UI.
7. **Alpha-blend requirement.** Android XR containers require `environment_blend_mode = Alpha Blend` and a transparent background. We run `openxr/environment_blend_mode=0` (opaque), and our passthrough probe warns when passthrough is missing (`xr_boot.gd:29`). This would be a per-platform settings fork.
8. **Scope creep.** The largest piece of E3 (dual backend) is useful even without containers, but it touches Invariant 6 (NIP-98) and ADR-2141 (acknowledgements). It should get its own ADR, separate from the container contract.

## Sources (accessed 2026-10-30)
- Godot class ref (latest/unstable): https://docs.godotengine.org/en/latest/classes/class_openxrspatialcontainerextension.html
- Godot PR #123124 "OpenXR Spatial Container integration" (m4gr3d, merged 2026-09-08, milestone 4.8): https://github.com/godotengine/godot/pull/123124
- Godot PR #123418 camera offset fix (2026-09-17): https://github.com/godotengine/godot/pull/123418
- Godot PR #123736 normal play space in immersive (dsnopek, 2026-09-24): https://github.com/godotengine/godot/pull/123736
- Godot PR #124382 bug fixes and polish (2026-10-14): https://github.com/godotengine/godot/pull/124382
- Godot PR #116424 multi-layer projection camera (2026-09-10, 4.8): https://github.com/godotengine/godot/pull/116424
- Godot PR #107391 spatial entities (2025-09-28, 4.6): https://github.com/godotengine/godot/pull/107391
- Godot PR #107619 xrSyncActions only on focus: https://github.com/godotengine/godot/pull/107619
- Godot source: `modules/openxr/extensions/spatial_container/*.cpp`, `modules/openxr/register_types.cpp`, `doc/classes/ProjectSettings.xml`, `version.py` (master, 4.8 beta)
- OpenXR spec 1.1, §12.50 `XR_EXT_spatial_container` and §12.51 `…_self_rendering` (rev 1, last modified 2026-08-06): https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html#XR_EXT_spatial_container
- Registry XML: https://raw.githubusercontent.com/KhronosGroup/OpenXR-SDK-Source/release-1.1.65/specification/registry/xr.xml
- OpenXR SDK 1.1.63 (2026-09-01, extensions ratified) and 1.1.65 (2026-10-16, hello_xr container mode): https://github.com/KhronosGroup/OpenXR-SDK-Source/releases
- Khronos SIGGRAPH 2026 BOF slides: https://www.khronos.org/assets/uploads/developers/presentations/OpenXR_BOF_v2_SIGG_2026_-_Slides.pdf
- Android XR — Use Spatial Containers in Godot (updated 2026-10-28): https://developer.android.com/develop/xr/godot/spatial-containers
- Android XR — Build with Spatial Containers (OpenXR): https://developer.android.com/develop/xr/openxr/spatial-containers
- GodotVR/godot_openxr_vendors 5.2.0-rc1 (2026-10-21, "Add support for exporting spatial container applications on Android XR #536"): https://github.com/GodotVR/godot_openxr_vendors/releases
- UploadVR, "OpenXR Spatial Container Extensions Standardize Volumetric App Multitasking" (2026-10-26): https://www.uploadvr.com/openxr-spatial-container-extensions-standardize-volumetric-app-multitasking/
- Mixed News coverage: https://mixed-news.com/en/openxr-spatial-containers-multiple-xr-apps-android-xr-godot/
- pyopenxr issue #148 (secondary: SteamVR beta and Meta desktop runtime lack the extension): https://github.com/cmbruns/pyopenxr/issues/148
- gdext releases (v0.5.5, 2026-08-09; features up to api-4-7): https://github.com/godot-rust/gdext
- Monado MR search (0 results for "spatial_container"): https://gitlab.freedesktop.org/monado/monado/-/merge_requests?search=spatial_container
- Primary check on HP (2026-10-30): `strings`-style grep of `~/.local/share/Steam/steamapps/common/SteamVR/bin/linux64/vrclient.so`, build 1792185270.
- Local: `xr-client/project.godot`, `xr-client/rust/Cargo.toml` / `Cargo.lock`, `xr-client/scripts/graph_scene.gd`, `xr-client/scripts/backend_caps.gd`, `xr-client/rust/src/binary_protocol.rs`, `crates/visionclaw-tri-layout/src/lib.rs`, `docs/XR-client.md`.
