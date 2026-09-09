---
title: XR Client Architecture
doc_id: VC-XR
version: 0.1.2
status: draft-for-ratification
verified_commit: 
changelog:
  - "0.1.2 (2026-09-06): Remediation — 2026-09-05 section: Wave 3 ADRs (2094–2101, 2061, 2071, 2085; proposed 2102–2105) and the ledger/diagram re-verification landed in 2cf222406 — re-verified at "
  - "0.1.1: flag self-contradictory docstring on is_directed_hierarchy_relation (excludes vs accepts 'hierarchical')"
sources:
  - xr-client/project.godot
  - xr-client/scripts/xr_boot.gd
  - xr-client/scripts/hud.gd
  - xr-client/scripts/graph_scene.gd
  - xr-client/rust/src/render_store.rs
  - xr-client/rust/src/webrtc_audio.rs
  - xr-client/README.md
  - src/handlers/layout_handler.rs
  - src/actors/gpu/force_compute_actor.rs
  - docs/gap-close-evidence/P2-vive-closeout-2026-08-20.md
date: 2026-08-31
---

# XR Client Architecture (VC-XR)

## Purpose
The immersive client that renders the VisionClaw knowledge graph in a headset: a
Godot + OpenXR front-end whose hot path is a godot-rust (gdext) crate. This
document is the living ground truth for how it renders, connects, authenticates
and lays out — code wins over legacy ADR/PRD prose.

## Current State

### Engine, renderer, and the multiview constraint (INVARIANT)
The project is authored against Godot 4.3 (`xr-client/project.godot:12`,
`config/features=PackedStringArray("4.3", "Forward Mobile")`) but the only
build that has ever rendered on a headset is **Godot 4.6.1-stable running the
Compatibility (OpenGL 3) renderer** (README "Version note" and VIVE bring-up
2026-08-22). Do not read the 4.3 string as the runtime.

`renderer/rendering_method="gl_compatibility"` is set for desktop
(`project.godot:48`) and this is **load-bearing, not a preference**: the
RenderingDevice / Vulkan multiview tonemapper is broken on SteamVR + Linux +
NVIDIA and fails stereo submission; Compatibility is the only renderer that
submits both eyes (README render-constraints table, line 119). Consequences that
travel with it and must not be silently "upgraded":
- **Glow / bloom OFF** in `WorldEnvironment` — glow post-process blanks the
  second eye under Compat multiview (README line 120). MSAA is off
  (`project.godot:51 anti_aliasing/quality/msaa_3d=0`), `hdr_2d=false`.
- **NVIDIA 580 open driver pinned** (`nvidia-580xx-open-dkms`); the 610 driver
  fails to render the GL multiview second eye (README line 121).
- **Native X11** (`--display-driver x11`); Wayland was never brought up.
Physics tick is pinned to 90 Hz (`project.godot:43`). OpenXR is enabled with the
HTC Vive action map (`project.godot:56-57`, `openxr_action_map.tres`).

### Scene graph and boot
`XRBoot.tscn` → `xr_boot.gd` initialises the OpenXR interface, sets
`get_viewport().use_xr = true`, probes capabilities, then defers the scene swap
to idle (`xr_boot.gd:53-61`) — a synchronous `change_scene_to_packed()` trips
"Parent node is busy adding/removing children" while the OpenXR vendor addon is
still adding XR nodes. Capability probing is defensive: eye-gaze is only
*queried*, never blindly bound, because enabling the
`XR_EXT_eye_gaze_interaction` action-map binding on a device that lacks it trips
the action-map error (`xr_boot.gd:40-50`). Quest 3 returns false there;
head-gaze stays primary. `GraphScene.tscn` → `graph_scene.gd` (3326 lines) is the
runtime: it holds the `GraphRoot/NodesMulti`, `GraphRoot/EdgesMulti` and
`GraphRoot/AgentMulti` MultiMeshInstance3D nodes (`graph_scene.gd:381-385`).

### Agent embodiment (ADR-2109)
Two layers, deliberately separate (ADR-140 D5). The **work layer** is every id
in the Rust agent registry (fed by `0x23 AGENT_ACTION` frames): embodied at
~4 Hz by `_reconcile_embodiment` with the `AgentAvatar` template under the
unit-scale `AgentsRoot` (never under `GraphRoot`, so avatars keep physical size
while the graph is fitted). Exactly one writer owns a work-layer avatar's
position and alpha: `scripts/agent_choreography.gd` (materialise at a rim slot →
travel at ~0.32 m/s → work 0.32 m off the node → explicit complete → park to the
rim at 0.3 alpha → rest → re-task). Work cues — the beam (`AgentMulti`, origin
synchronised to the body via `set_agent_anchors`), a pulsing node ring and a
completion burst (`scripts/agent_effects.gd` under `AgentEffectsRoot`) — follow
the registry. The **conversation layer** (`spawn_agent`, did:nostr keyed) is the
only thing the proxemics arc places. **Demo mode** is `scripts/agent_demo_director.gd`
alone: it produces real `0x23` frames into `ingest()` (wire ids
`0x80000000|0xD001..`, payload `"demo":true`), reports completion via
`apply_agent_state`, and `retire_agents` on Stop; the scene has no demo branch and
labels demo rows "[demo]". Reduced motion (comfort default) turns travel into
fade/relocate/fade.

### HUD structure (hud.gd)
The HUD is a tabbed panel built **programmatically** under `HudControl` into a
SubViewport shown on a world-space, wand-grabbable quad — one source of truth so
every control fits its page (`hud.gd:1-28`). Tab order:
`graph, layout, query, pins, swarm, key, session, help` (`hud.gd` `TAB_ORDER`;
`key` is the colour swatch legend, 2026-09-08). Stable node
paths are documented in-file (`hud.gd:20-28`) for rebases.

Two overflow lessons are baked in as INVARIANTS:
- **532px page host.** The constrained-layout controls were split out of the
  Graph tab onto a dedicated Layout tab (commit 2358cda4a) because the combined
  page needed ~1050px in a 532px host. The Layout page uses tightened separation
  `3` not the usual `8` (`hud.gd:340-345`): four groups land at 564px with
  default separation — 32px past the host — and the tighter separation buys
  ~35px. A dev-only overflow guard warns once per tab if a page's min-height
  exceeds the host (`hud.gd:756-766`).
- **ACTION_MODE_BUTTON_PRESS everywhere.** Every action button, tab button and
  type-toggle fires on *press*, not release (`hud.gd:252`, `637`, `647`):
  pulling the Vive trigger jolts the ray 20–30px, so a release-mode button often
  sees the release land outside itself and silently cancels the click (observed
  live 2026-08-31, commit ae9c6ac60).
The HUD owns no decision logic — it emits `control_pressed(action)` intents and
GraphScene owns every effect (`hud.gd:76-88`). Overlay panels (intervention,
document card) raise an overlay shield that hides the tab root so a stray ray
can't click a control behind them (`hud.gd:744-749`).

### Transport
Backend resolution is env-overridable (`graph_scene.gd:65-71`,
`_connect_from_env` at 1123): `XR_BACKEND_WS` (default `ws://localhost:4000`) is
the base; two well-known paths are appended — graph stream `/wss`, presence
`/ws/presence`. On the working desktop path the client runs on HP-Desktop and
points at the LAN backend `ws://192.168.2.132:4000` **directly** — nginx `:3001`
does not proxy `/ws/presence` (README run notes). "localhost:4000" is reached
over a reverse SSH tunnel from HP-Desktop to the backend host. The Rust
`BinaryProtocolClient` owns the wire; GDScript only supplies URLs/credentials and
pumps the inbox each frame (`graph_scene.gd:65-66`). The graph socket connects to
the **plain URL** and authenticates solely with the NIP-98 `authenticate` frame
minted over that same URL — `connect_to_url(url, nostr_secret_hex)` takes no token
and `XR_GRAPH_TOKEN` no longer exists (ADR-2076). It decodes **Protocol V3**
and the **V5 wrapper** (`0x05` + 8-byte broadcast seq) — see VC protocol doc and
README gdext-class table. HTTP origin for writes is derived by swapping the ws
scheme (`_http_base`, `graph_scene.gd:1141-1152`) or `XR_BACKEND_HTTP`.

### Deploy ceremony (desktop OpenXR)
The verified way onto a headset today is not the APK — it is the Compatibility
renderer on native X11 driving SteamVR (README line 81). HP-Desktop is reached
via `ssh john@10.10.10.1` (the 25G rail; gap-close evidence line 14). Launch is a
foreground process:
```
XR_BACKEND_WS=ws://192.168.2.132:4000 XR_NOSTR_SECRET=<hex> \
  godot --path xr-client --rendering-driver opengl3 --display-driver x11 \
  res://scenes/XRBoot.tscn
```
Redeploy is a **separate kill then launch** over two ssh calls (a single chained
call races the compositor), and the launch call must carry `XAUTHORITY` so Godot
can open the X11 display for the SteamVR compositor — the process has no
inherited session. SteamVR must be running with the VIVE Pro tracked and
`~/.config/openxr/1/active_runtime.json` pointed at SteamVR.

### Identity and signing (NIP-98)
`NostrAuth.create(OS.get_environment("XR_NOSTR_SECRET"))` always returns a signer
— ephemeral if the secret is empty (`graph_scene.gd:425-430`). `XR_NOSTR_SECRET`
is a hex BIP-340 key and is **required** in practice: besides the presence
challenge/response handshake it signs the NIP-98 `authenticate` on the graph
socket that gates server-authoritative node drag/pin (README, `transport.rs:75`).
Physics/layout HTTP writes attach a per-request NIP-98 `Authorization: Nostr
<b64>` header minted for the exact URL+method (`_auth_headers`,
`graph_scene.gd:1055-1073`); the HUD decide POST uses the same path
(`hud.gd:1176-1188`). A legacy dev bearer (+ `X-Nostr-Pubkey`) is the fallback
only when no real secret is present; `_nostr_secret_present` gates this and the
dev bearer 401s in release builds (`graph_scene.gd:85-90`).

### Rendering offload and edge stride (INVARIANT 16)
Rust `RenderStore` packs the whole instance buffer; GDScript does a single buffer
assignment per MultiMesh. The edge MultiMesh has `use_custom_data=true`, so the
resource stride is **16 floats/instance** — 12 transform + 4 INSTANCE_CUSTOM
(relation-style code in `.a`), `EDGE_STRIDE_TYPED = 16` in
`render_store.rs:105`. `_update_edge_multimesh` divides the packed buffer by 16
(`graph_scene.gd:1807-1811`); a `/12` divisor mis-sizes `instance_count`, so
`set_buffer` rejects every frame and edges vanish (fixed in commit 63d9bb9b8).
The work-beam MultiMesh (agent→target links, ADR-140 Pillar 2) uses the same
stride 16 (`graph_scene.gd:1818-1830`, `render_store.rs:1362`).

### Constrained layouts
The Layout tab drives the backend layout engine. Six modes cycle through the
`LAYOUT_MODES` enum, POSTed to `/api/layout/mode`
(`graph_scene.gd:213-215`; server enumerates the same list at
`layout_handler.rs:10`): `forceDirected, hierarchical, radial, spectral,
temporal, clustered`. Radial shells POST `/api/layout/radial` with a mode of
`dagRank | typeTier | ego` (`graph_scene.gd:734-742`, `_post_radial` at 983;
server at `layout_handler.rs:135-166`). The Hierarchy toggle PUTs `dagBiasK`
(0.6 on / 0.0 off) and Shells ± nudge `dagLevelDistance` (`graph_scene.gd:888-910`).

**DAG ranks are derived from "hierarchical" edge labels.** As of commit
73540faa0, `is_directed_hierarchy_relation` in `force_compute_actor.rs:580`
matches `is_subclass_of | subclass_of | SUBCLASS_OF | hierarchical |
HIERARCHICAL`. This deployment's ingest writes the collapsed label
`hierarchical`; before the fix the DAG ranks stayed all-unranked
(`compute_dag_ranks`, line 590) so `SetRadialLayout{DagRank}` and the Hierarchy
toggle were silently inert — reported in-headset as the Radial Shells buttons
"appearing disconnected". Caveat for maintainers: the function's own doc-comment
(`force_compute_actor.rs:576-578`) still asserts the generic `"hierarchical"`
string is *excluded* ("accepting it would fabricate ranks from non-subclass
structure"), directly contradicting the `matches!` set three lines below it at
line 586 which accepts it. The inline comment at 581-583 is the authoritative
intent; the stale exclusion paragraph above it should be read as superseded.

## Known divergences & open items
- **project.godot vs runtime.** File says Godot 4.3 / Forward Mobile
  (`project.godot:12`); the working build is 4.6.1-stable Compatibility. Still
  open, and deliberately so: `config/features` is editor-managed metadata, Godot
  is **not installed in this environment**, and hand-editing it cannot be
  verified — the editor rewrites that array on save. Re-pinning is a task for the
  next session on a machine with the 4.6.1 editor, not a text edit. Documented in
  `xr-client/README.md:15-18` ("read 4.3 as the pinned editor of the day") as
  well as here. Assessed 2026-09-05 (ADR-2079 scope review).
- **Quest 3 is unmeasured.** Quest 3 is the sole *ship* target
  (`project.godot:2`, README) but the APK is **unbuilt** and the cross-build is
  frozen — no Android NDK is provisioned in this environment (README line 6).
  The 90 fps figure (13,164 nodes / 145,692 edges) was validated only on VIVE Pro
  + dual-RTX-6000 desktop OpenXR (README line 226). No Quest performance number
  exists. Legacy PRD-008 treats Quest as the primary; that is aspirational.
- **LiveKit / spatial voice incomplete.** `SpatialVoiceRouter`
  (`webrtc_audio.rs`) owns only the routing maths and per-avatar position map;
  the livekit-android AAR media transport (PRD-008 §5.5) that would consume it is
  not wired on any built target (`webrtc_audio.rs:1-5`). Voice is design-complete,
  transport-absent.
- **Query Execute is implemented; runtime acceptance remains open.**
  `query_builder.gd:EXECUTE_ENABLED` is true; `graph_scene.gd` posts to
  `/api/graph/query/pattern` and builds result planes. Server correctness and
  user-visible denied/error states require verification.
- **`?token=` — Resolved — ADR-2076 (2026-09-05).** The XR client no longer sends
  a query token: `with_token`, the `token` parameter of `spawn_graph_stream` /
  `graph_pump` / `connect_to_url`, and the `XR_GRAPH_TOKEN` plumbing in
  `graph_scene.gd` are deleted. `XR_NOSTR_SECRET` and the NIP-98 `authenticate`
  frame are the only graph-socket credential. The *server* still accepts the
  query form for other clients — that remains an open divergence owned by the
  wire/core domains (`docs/BASELINE-architecture.md:217`).
- **Dev bearer fallback.** Still open as a code path. `_auth_headers`
  (`graph_scene.gd`) falls back to `PHYSICS_BEARER` + `X-Nostr-Pubkey` when no
  real secret is present; the server refuses it for any non-loopback peer (the
  HP is never loopback), so on the headset every server-routed HUD write
  (View 3D/Flat, Hierarchy, Shells, Spread, Planes, Radial, Layout Mode, Reset)
  401s unless the backend is armed with `VISIONCLAW_DEV_MODE=1`. **ADR-2108
  (2026-09-08)** arms it by default in the dev compose profile; the client now
  also flashes the rejection and its remedy in the HUD bottom strip
  (`hud.flash_notice`, `graph_scene._describe_write_failure`) and appends it to
  the Graph-tab status line, instead of a log-only `push_warning`.
- **HUD Key tab (2026-09-08).** A colour swatch key (`hud.gd::_build_key_page`)
  mirrors the live palette: community hue / anomaly / query marks
  (`render_store.rs`), agent status halo + Swarm dot (`SWARM_STATUS_COLORS`),
  edge tints (`edge_flow.gdshader`), wand ray + panel states, avatar states.
  The swatch constants are duplicated from their sources by design (same
  posture as `SWARM_STATUS_COLORS`); a palette change must update both.
- **Legacy ADR status.** ADR-071 (Godot-rust replacement), ADR-136 (VIVE
  validation target), ADR-140 (swarm pillars), ADR-141 (constrained layout) are
  cited as evidence; treat this document as authority where they conflict.

## Invariants (must not silently change)
1. `gl_compatibility` / Compatibility (OpenGL 3) renderer — re-testing on Vulkan
   multiview (SteamVR/Linux/NVIDIA) is required before any change.
2. Glow/bloom stay OFF; NVIDIA 580 open pinned; native X11.
3. Edge and beam MultiMesh stride = 16 floats/instance (matches
   `EDGE_STRIDE_TYPED`); never divide packed buffers by 12.
4. HUD buttons use `ACTION_MODE_BUTTON_PRESS`.
5. Layout-tab page content must fit the 532px host (overflow guard is the tripwire).
6. `XR_NOSTR_SECRET` required for drag/pin/presence; NIP-98 header URL must be the
   exact request URL incl. query.
7. DAG-rank detection must accept the ingest's collapsed `hierarchical` label.
8. Work-layer embodiments have ONE pose writer (`agent_choreography.gd`); the
   proxemics arc, nudges or drifts must never write their transforms. Avatars
   and effects sit under unit-scale roots, never under `GraphRoot` (ADR-2109).
9. Demo mode enters only through the real registry doors (`ingest`,
   `apply_agent_state`, `retire_agents`) from `agent_demo_director.gd`; no
   scene-side demo rendering branch, no synthetic position frames (ADR-2109).

## Change process
Edit the affected `.gd`/`.rs` file, run `cargo test -p visionclaw-xr-gdext`
(226 headless tests as of 2026-09-05, <1 s, no headset/Godot/network needed; the
README's "141" is stale — ADR-2076). Any change
to a render-constraint invariant (renderer, glow, driver, display) requires a
fresh on-headset bring-up on the VIVE Pro before merge and a note here. Bump
`version` on ratified change; record new divergences honestly rather than
deleting them.

## Estate closeout qualification — 2026-09-04

The [rendered-state review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md) records 218 passing Rust library tests and their limits: Godot-facing runtime classes are excluded by `cfg(test)`, and no headset/scene/shader test ran. Hover motion is implemented. Beam targets are fold-remapped and drawn-gated, while agent endpoints use local positions directly. This 2026-09-04 observation is superseded by the current freshness implementation: `xr-client/rust/src/render_store.rs:469,738-740,794-796,816-822` compares timestamps, rejects stale evidence and expires actions. Source-level precedence and expiry exist. Visible stale/error handling and authenticated action-to-render behaviour still require scene and headset evidence on each intended target (rechecked 2026-09-07).

## Renderer, HUD and hierarchy closeout — 2026-09-04

ADR-2032 retains its scoped desktop configuration; current headset/export/mobile acceptance needs separate receipts — that item stands. The other two are closed:
ADR-2033 stays **partial**, but for a different reason than the closeout gave — Corrected — ADR-2079 (2026-09-05). The source-inventory half now holds: `hud.gd` routes every ray-driven control through the single `_press_fire` helper (`hud.gd:262-264`), all eleven `Button`/`CheckButton` sites call it and no raw constructor remains, so the defect to grep for is a `Button.new()` not wrapped in `_press_fire`. What is still open is the **behavioural** half — press-to-dispatch, disabled controls, drag-off, controller jitter and duplicate actions have never been exercised on the target runtime (Godot is not installed in this environment). The receipt to fill is `docs/estate-closeout/2026-09-05/xr-export-runtime-revision-matrix.md` Column C.
ADR-2035 retains label acceptance and its predicate test agrees — Resolved — ADR-2079 (2026-09-05). `directed_hierarchy_accepts_subsumption_and_the_collapsed_label` (`force_compute_actor.rs:4562`) asserts the accept for `is_subclass_of | subclass_of | SUBCLASS_OF | hierarchical | HIERARCHICAL`; the earlier test contradicted both the implementation and the ratified decision. The residual cost stands and is ADR-2035's `review_trigger`: the collapsed label is lossy, so a producer reusing it for domain membership contributes edges ranked as subsumption.
Still required for the ADR-2032 and ADR-2033 items: actual scene/headset evidence. See [estate XR review](https://github.com/DreamLab-AI/VisionFlow/blob/main/docs/estate-review/rendered-state.md#xr-control-coverage-and-hierarchy-semantics); source and helper results do not certify Godot execution.

## Remediation — 2026-09-05

- **ADR-2081** — the dead browser XR-mode surface is removed; the immersive client is the Godot app alone. `@react-three/xr` (declared, never imported) and every `isXRMode` / `xrSessionState` flag and setter are deleted; the WebXR *capability probe* is retained for telemetry.
- **ADR-2075** — `/ws/speech` authenticates with a post-upgrade NIP-98 `authenticate` frame, refuses every command-bearing frame until it arrives, closes after a 30 s deadline, and no longer reads a `?token=` query parameter or an unverified bearer. The browser voice client now sends that frame.
- **ADR-2076** — the XR graph socket drops query-token auth entirely (`with_token`, the `token` parameters, and `XR_GRAPH_TOKEN` deleted); `XR_NOSTR_SECRET` plus the NIP-98 frame is the only credential.
- **ADR-2077** — dead browser-client surfaces deleted: `interactionApi.ts`, the never-emitted `message:graph` bus event, the uncalled `WebSocketRegistry.closeAll()`, and the empty `contributor-studio/` and `workspace/` feature directories.
- **ADR-2079** — closeout narrative corrected: no HUD constructor omits press-mode any more (`_press_fire` centralises it across all eleven controls) and ADR-2035's predicate test agrees with its implementation. ADR-2033 stays `partial` because its behavioural half is unverified, and the ADR-2032 headset/export/mobile receipt item stands.
- **ADR-2100** — the browser client's two independent WebSockets to `VITE_JSS_WS_URL` are consolidated onto the single `podNotificationManager` (registry name `solid-pod`); the store's `solid-store` client is now a thin adapter over it, `state.solidSubscriptions` is a bookkeeping mirror rather than a second dispatch path, and one shared constant pair (`SOLID_MAX_RECONNECT_ATTEMPTS = 5`, `SOLID_RECONNECT_DELAY_MS = 1000`) replaces the divergent 10-vs-5 retry ladders.
