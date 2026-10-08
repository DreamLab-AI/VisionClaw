---
title: XR Client Architecture
doc_id: VC-XR
version: 0.1.17
status: draft-for-ratification
verified_commit: 
changelog:
  - "0.1.17 (2026-10-08): always separate, memory ×10, typed search (ADR-2135 amendment, ADR-2136). The separation slider is gone from the HUD Layout page and the desktop (separation_control.gd deleted); the triangle is permanent at the fixed SEPARATION 190 derived from the live graph radii. The memory cloud is MEMORY_BODY_SCALE 10 graphs wide on the memory vertex's ray, clear of both graphs (TriangleFrame::memory_centre); route tubes, beads and rings grow with it (cloud-local), guide dots grow to half the answer ring at the far end, the hover label, its lift and its reach ×10. Memory Search gains a press-fire on-screen keyboard (onscreen_keyboard.gd, in place of the lists, ≤ 532 px); a typed query searches globally. The query response carries query.position (the snapshot's PCA basis), and the headset route runs query point → sidecar top-k in rank order with the answer ring on the top hit. Triangle budget unchanged (scale is free). No invariant changed."
  - "0.1.16 (2026-10-07): HUD Graph Separation control (ADR-2135 in the headset) — Sep −/slider/value/Sep + share the Layout Mode row (page stays 529 px); separation_control.gd writes graphSeparationX through the physics PUT at ≤ 4 Hz while dragging plus a final write on release; read-back moves the slider. Memory search from the headset — Query page Graph Query / Memory Search modes; presets POST /api/memory-cloud/query (NIP-98) and draw a sidecar top-k route (sampled hits in rank order, labelled as such) through the same MemoryRoute gate; a hit press retargets the guide cue; shared query-response fixture pins the headset parser to the server's wire types; benchmark route_source=query. No invariant changed."
  - "0.1.15 (2026-10-07): ADR-2135 separated layout — Graph Separation opens a ground-plane triangle (knowledge −60°, ontology +60°, memory 180°; R = 2/√3 × separation) from the shared visionclaw-tri-layout crate; the cloud folds the graph bounds and takes the memory vertex (graph_robust_bounds(separation), CloudFrame.set_separation, physics read-back of graphSeparationX); work agents rest at the centroid plus their activity drift (render-store DriftField fed by 0x23 and memory_flash agentId; the choreography stays the single pose writer). No invariant changed."
  - "0.1.14 (2026-10-07): intermittent CPU gate root-caused and fixed. The cause was cross-L3-domain migration of the main thread on HP's multi-L3 CPU (~8x on-CPU spikes for two frames), not the first build. The benchmark pins its main thread to its L3 domain, and the first plan build is gated on its own limits (pack 12 ms, LOD 33 ms) instead of being dropped silently. No invariant changed."
  - "0.1.13 (2026-10-07): held things above the route — wand aim rays at HELD_RENDER_PRIORITY 15 in the transparent pass (depth test kept), radial menu with the HUD at 20; ADR review finding (conflicting depth cue). No invariant changed."
  - "0.1.12 (2026-10-07): desktop memory-explorer parity — cloud framed on the live graph (cloud_frame.rs port of cloudFrame.ts/robustBounds.ts, 1 Hz read, 0.8 s glide); route drawn without depth test below the HUD; framing cue instead of a camera move (ADR-2107); honest sidecar agreement line in the HUD Memory row (sidecarTotal/sidecarAgree on memoryRoute); 10 s route repeats no longer replay the trace; TUBE_R/RING_R re-synced. No invariant changed."
  - "0.1.11 (2026-10-07): pack timing gate on thread CPU time (wall reported beside it; holds at load average 38); GUT 9.6.1 (the Godot 4.6 line) vendored with a CI guard against parse errors and skipped scripts; live FrameBudget pass on the final interface (burst pool, 5 % reserve, 2 s peak of measured other_tris)."
  - "0.1.10 (2026-10-07): per-frame pack plans (13k/20k pack 0.5 ms, zero steady-state allocations, far ribbons half per frame), benchmark asserts pack_ms/lod_build_ms p99; live GraphScene runs the FrameBudget pass (measured other_tris) and both packs every frame; attention heat moved to live_tint."
  - "0.1.9 (2026-10-07): FrameBudget allocator (rust/src/frame_budget.rs) shared by the graph LOD tiers, memory cloud, route and burst pool; one-triangle sprites and bead discs; 5 % variance reserve; row emphasis for memory_flash on cloud sprites"
  - "0.1.8 (2026-10-07): halo next_pass replaced by a quad layer, edge LOD (near cylinders, far ribbons), gem cap 80; all benchmark runs incl. 20k edges under budget; instance-colour divergence corrected by measurement; avatar rotation drift fixed. No invariant changed."
  - "0.1.7 (2026-10-07): beat clock (relayed desktop clock, tap tempo, opt-in mic), memory_flash bursts, attention heat and desktop beam action encoding (ADR-2134); Swarm-roster teleport routed; Invariant 10 (mic opt-in, never recorded)"
  - "0.1.6 (2026-10-07): live memory cloud and relayed query route in the headset (ADR-2133 client side, XR WP6/WP7); memoryRoute text frame; memory layers held to the 16k triangle headroom the node LOD leaves"
  - "0.1.5 (2026-10-07): node-mesh LOD (gem tier capped at 96, 2-triangle impostors beyond) brings the benchmark under the PRD-008 triangle budget at 1k and 13k nodes; dev profile optimised because the editor/headset run loads target/debug; GUT 9.7.1 vendored, CI on Godot 4.6.1. No invariant changed."
  - "0.1.4 (2026-10-07): desktop parity — domain palette (default) with community toggle, inbound settingsUpdated/filter/graphUpdated sync incl. physics read-back, cluster hulls as one ArrayMesh; project.godot comment corrected; benchmark triangle-budget divergence recorded. No invariant changed."
  - "0.1.3 (2026-10-02): DAG ranks keyed on subClassOf provenance, not the hierarchical label; domain-root spokes relabelled domain_member (ADR-2035 amendment, N-14)"
  - "0.1.2 (2026-09-06): Remediation — 2026-09-05 section: Wave 3 ADRs (2094–2101, 2061, 2071, 2085; proposed 2102–2105) and the ledger/diagram re-verification landed in 2cf222406 — re-verified at "
  - "0.1.1: flag self-contradictory docstring on is_directed_hierarchy_relation (excludes vs accepts 'hierarchical')"
sources:
  - xr-client/project.godot
  - xr-client/scripts/xr_boot.gd
  - xr-client/scripts/hud.gd
  - xr-client/scripts/onscreen_keyboard.gd
  - xr-client/scripts/memory_search.gd
  - xr-client/rust/src/memory_query.rs
  - xr-client/scripts/graph_scene.gd
  - xr-client/rust/src/render_store.rs
  - xr-client/rust/src/domain_palette.rs
  - xr-client/rust/src/settings_sync.rs
  - xr-client/rust/src/hulls.rs
  - xr-client/scripts/graph_parity.gd
  - xr-client/materials/cluster_hull.gdshader
  - xr-client/rust/src/lod.rs
  - xr-client/scripts/node_lod.gd
  - xr-client/materials/node_impostor.gdshader
  - xr-client/materials/node_halo_quad.gdshader
  - xr-client/materials/edge_ribbon.gdshader
  - xr-client/materials/edge_flow_common.gdshaderinc
  - xr-client/perf/benchmark.gd
  - xr-client/rust/src/render_store_pack.rs
  - xr-client/scripts/frame_budget_pass.gd
  - xr-client/rust/src/beat.rs
  - xr-client/rust/src/semantic.rs
  - xr-client/rust/src/attention.rs
  - xr-client/scripts/beat_pulse.gd
  - xr-client/scripts/memory_bursts.gd
  - xr-client/rust/src/webrtc_audio.rs
  - xr-client/rust/src/memory_cloud.rs
  - xr-client/rust/src/memory_route.rs
  - xr-client/rust/src/cloud_frame.rs
  - crates/visionclaw-tri-layout/src/lib.rs
  - crates/visionclaw-tri-layout/src/drift.rs
  - src/actors/gpu/display_projection.rs
  - xr-client/scripts/beat_pulse.gd
  - xr-client/scripts/memory_cloud_layer.gd
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
rim at 0.3 alpha → rest → re-task). The body is a faceted 0.14 m core inside a
procedural role frame (`scripts/agent_role.gd`: Architect cage, Analyst ring,
Coder chevrons, Reviewer diamond, Tester fins, Optimizer hoops, generic hoop;
role inferred from name then task) with a 0.09 m pointer cone aimed at the
target and a world-size badge "AR  Name" whose task caption shows only while
announcing or selected. Work cues — the beam (`AgentMulti`, origin synchronised
to the body via `set_agent_anchors`), a pulsing node ring, hand-off packet beads
along the real edge, an arrival flash and a completion burst
(`scripts/agent_effects.gd` under `AgentEffectsRoot`) — follow the registry. The **conversation layer** (`spawn_agent`, did:nostr keyed) is the
only thing the proxemics arc places. **Demo mode** is `scripts/agent_demo_director.gd`
alone: it produces real `0x23` frames into `ingest()` (wire ids
`0x80000000|0xD001..`), reports completion via `apply_agent_state`, and
`retire_agents` on Stop; the scene has no demo branch, and the synthetic agents
play as real ones — no name, roster or payload marker; the HUD's Start/Stop
Agent Demo button is the only visible sign. Reduced motion (comfort default)
turns travel into fade/relocate/fade.

### Memory activity, attention heat and the beat clock (ADR-2134)
These are ports of desktop behaviour, pinned to it by
`xr-client/rust/tests/fixtures/desktop_parity.json`. The desktop's own functions write that file
(`client/.../__tests__/xrParityFixtures.test.ts`), and the Rust tests read the same file. They
also parse the TS source tables and the beam shader uniforms.
- **Beam action encoding** (`semantic.rs`): work beams take the desktop's per-action colour and
  taper (`semanticEncoding.ts`). `INSTANCE_CUSTOM` holds r = action code, g/b = target/agent
  radius and a = status, so the stride stays 16. Blocked beams are pulled toward amber, slowed
  and dimmed.
- **memory_flash bursts** (`memory_bursts.gd` under `AgentEffectsRoot`): colour, size,
  lifetime, implode motion and ring count follow the verb, with the namespace hue jitter
  computed in three.js linear space. The triangle budget decides how they are drawn:
  - **Memory cloud shown and loaded.** The scene is at ~99.6k of 100k triangles (xr-cloud,
    13k nodes), so a flash adds **no geometry**. `resolve_flash` names the cloud rows (desktop
    rule; an unmatched flash draws nothing), and `set_row_emphasis` restyles those sprites
    with the desktop tint, a brightness envelope (1–2.5×) and a size envelope (1–2×, held at
    1 under reduced motion). At most 64 rows are live at once.
  - **No cloud on screen.** One pooled ring MultiMesh of ≤ 64 slots draws the rings (≤ 4,096
    triangles, one draw call), recycling the oldest slot, on a hashed 0.45 m shell around the
    graph centre. Reduced motion holds the ring size and only fades it.
- **Attention heat** (`attention.rs`, render store): every applied `0x23` action touches its
  target. The heat has a 20 s half-life, saturation 1.5 and 512 entries, and brightens the node
  colour in place without recolouring it. It never touches the edge buffer.
- **Beat clock** (`beat.rs`, `pulse.rs`, `beat_pulse.gd`): the arbiter chooses a mic lock first,
  then a tap until the desktop's clock changes, then the relayed desktop `beatClock` (fresh
  within 6.5 s), then the last tap.
  - The server offset comes from the JSON ping/pong round trip every 2 s (the minimum-RTT
    sample of eight).
  - One `beat_pulse` uniform per frame drives the live materials of the node-halo
    quads (`NodesHaloMulti`), the edge cylinders and far ribbons (both through
    `edge_flow_common.gdshaderinc`) and the burst opacity. It is an emission swell, not a post-process (Invariant 2), and
    under reduced motion (the comfort default) it is held at exactly 0, so
    halos, edges and bursts keep steady brightness and only the HUD Beat
    readout shows the tempo (ADR-2107).
  - Tap tempo uses **B/Y**, or a click of the **left trackpad/stick centre** (inside the
    locomotion dead zone). Neither is bound elsewhere; Vive wands have no B/Y.
- **Microphone** (WP8, off by default): the Session-tab Mic toggle starts an
  `AudioStreamMicrophone` on a muted `BeatMic` capture bus. On Android it asks for
  `RECORD_AUDIO` on that press, never at start-up. A red `● MIC` header badge shows while it is
  listening. Audio is analysed in memory over an 8 s window (port of `beat.ts`
  `onsetEnvelope`/`estimateTempo`) and discarded. The mic may drive the pulse only after two
  consecutive estimates agree at confidence ≥ 0.6; noise scores ≈ 0.23.
- **HUD**: the Session page has one Beat row (status · Tap · Mic · Bursts) inside the 532 px
  host. The Key tab lists beam actions and burst verbs.

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
- **No separation control (ADR-2135 amendment, 2026-10-08).** The knowledge
  graph, ontology and memory cloud are always apart, so the Layout page's Layout
  Mode row holds only the layout-mode button; the 2026-10-07 slider row and
  `separation_control.gd` are gone. The page stays 529 of 532 px.
- **Query page: Graph Query / Memory Search (2026-10-07).** A mode row (press
  fire, styled like the tab bar) replaces the 39 px header: graph mode is the
  desktop query builder (522 px); memory mode is a one-line caption, a "Type a
  query…" button, a preset list (120 px) and the top hits, each list in a
  fixed-height scroll region (≤ 532 px with both full). The keyboard (ADR-2136,
  2026-10-08) opens in place of the lists: an entry line, four rows of ten
  press-fire keys (digits, QWERTY, `' - . ?`) and Cancel · Space · Delete ·
  Search →, about 430 px (only glyphs in the HUD font, `test_hud_batching`); `onscreen_keyboard.gd` holds the buffer (120 characters)
  and Search emits `memory_typed:<text>`. See *Memory search from the headset*
  below.
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

### Memory cloud and query route (ADR-2133, WP6/WP7)
The headset draws the same live RuVector sample as the desktop explorer
(`EmbeddingCloudLayer.tsx`), owned end to end by `scripts/memory_cloud_layer.gd`
with the hot path in Rust (`memory_cloud.rs`, `memory_route.rs`).
- **Load.** `GET /api/memory-cloud` only (positions + metadata). The vectors blob
  is never fetched and the headset never runs HNSW. The request is signed by the
  scene's `_auth_headers` for the exact URL fetched (Invariant 6). 401/403 hides
  the cloud quietly ("Memory: Locked" on the button, no toast, no polling); 503
  reloads at `Retry-After`, else 5 s doubling to 60 s; 409 reloads once at once;
  a malformed or short body is rejected in Rust and the previous snapshot stays.
- **Placement (desktop `cloudFrame.ts`, `rust/src/cloud_frame.rs`).** The layer
  sits under `GraphRoot` (server space). `CloudRoot` (outer) sits at the graph's
  robust centre — the 5th–95th percentile box of every node position,
  `BinaryProtocolClient.graph_robust_bounds()`, the TS `robustBounds` order
  statistics exactly (folded per graph, see below) — and scales the cloud's robust radius to `MEMORY_BODY_SCALE` (10) times the graph's times
  `cloud_scale / 5` (the layer's `cloud_scale`, default 5, linear from there); `CloudCore` (inner) shifts the cloud by minus its own robust
  centre, so rotation turns the core in place. The graph extent is re-read once a
  second and the outer node glides by `min(1, dt / 0.8)` per frame (snapping on
  the first placement, on a new snapshot and under reduced motion), so physics
  never jitters it. Without a graph the scale is `cloud_scale` × 10 and the
  cloud clears a live-sized graph. A Rust
  test parses `cloudFrame.ts`, `robustBounds.ts` and `EmbeddingCloudLayer.tsx`
  for the constants and formulas. Sprite size follows `cloudPointSize` (constant
  in cloud-local units above its 0.5 floor).
- **Memory search from the headset (2026-10-07, `memory_search.gd`,
  `rust/src/memory_query.rs`).** The relay above needs the desktop and the
  headset on the same key; this path does not. The Query page's Memory mode
  lists presets — the last four queries run on this headset (persisted to
  `user://memory_search_recent.json`), five curated questions (each searched
  within its estate namespace when the snapshot has it, else globally), and
  "what does <namespace> hold" for the eight namespaces with the most sampled
  rows (at least two, and no whitespace in the name: the live store holds
  stray sentence-length values there) — as shortcuts beside the on-screen
  keyboard (ADR-2136), whose typed query is searched globally. A press or a
  typed Search POSTs `/api/memory-cloud/query {text, k: 50, namespace?}`
  signed by
  `_auth_headers` for the exact URL (ADR-2076, Invariant 6), one query in
  flight; 401/403 (power user or dev mode needed, ADR-2133), 429, 503, 400 and
  transport failures are spelled out on the caption line. The hits list shows
  rank · key · namespace · score and ● (sampled, has a point) or — (not
  sampled) for the top eight. k is the server's ceiling because the cloud
  samples a few thousand rows of a much larger store: measured live on
  2026-10-07, a namespace-scoped top-10 had 0–3 sampled hits and a top-50 had
  2–11, while a global query (dominated by the thinly sampled `ruvnet-kb`) had
  none. **The route is query point → sidecar top-k, not a search path:** the
  response carries no traversal, and the headset holds neither the vectors nor
  the PCA basis, so the server places the query (ADR-2136): `query.position`
  is the query vector projected with the snapshot's own basis and scale (a
  sampled row's vector lands exactly on its row). The route starts at that
  point and runs through the sampled hits in rank order; the root ring marks
  the query point and the answer ring the top hit (`RouteSamples::answer`);
  every sampled hit gets a gold mark. One sampled hit is enough. From a server
  without `position` the old line through the hits alone, top hit last, is
  drawn (two hits needed). `RouteSource::SidecarTopK` makes both the caption
  ("route: query point → sidecar top-k (not a search path)") and the
  Memory-row line ("Route: query point → sidecar top-k · n of k sidecar hits
  are in the sample") say so; there is no local top-k, so no agreement figure
  is shown. `MemoryRoute.offer_query_response`
  feeds the same `RouteGate` as a relay (Unix-ms `sentAt`, so a later desktop
  frame replaces it; a response naming another snapshot reloads the cloud
  once). Pressing a sampled hit
  replays the guide cue towards that point (`ActiveRoute::focus_row`; the
  answer ring is not highlighted while the cue points elsewhere); an unsampled
  hit flashes a notice instead. Running a query turns the cloud on. There is
  no voice entry: the headset's only microphone path is the beat analyser,
  whose audio never leaves the device (Invariant 10). Drift:
  `rust/tests/fixtures/memory_query_response.json` is parsed by the headset
  (`tests/memory_query_parity.rs`) and round-tripped by the server's
  `MemoryCloudQueryResponse` (`crates/visionclaw-memory-cloud/src/wire.rs`);
  the request limits are pinned to `validate.rs`. `perf/run_benchmark.gd
  route_source=query` measures a route built this way, and
  `tests/visual/live_memory_search_capture.gd` runs one preset against a live
  backend through the HUD intent path.
- **Separated layout (ADR-2135; always on since 2026-10-08).** The cloud folds
  each node position into its nearest graph's frame before the bounds are
  taken (`TriangleFrame::separated`, the shared `visionclaw-tri-layout` crate),
  so the bounds are one graph's, and `CloudRoot` sits at
  `TriangleFrame::memory_centre`: on the memory vertex's ray (180°: behind the
  graphs, in front of a user facing them), at least `CLEARANCE` 1.25 × the
  summed radii from each graph. At live scale (graph radius ~93, scale ~0.005
  m/unit) the cloud is about 4.5 m in radius with its near side about 3 m from
  the user: past the near clip and the HUD, and inside the 12 m × 10 hover
  reach. The guide cue still runs from the wand to the answer. The server
  places the knowledge (−60°) and ontology (+60°) graphs on the other vertices
  and keeps agent nodes at the centroid. Work-layer avatars still have one pose
  writer (Invariant 8) and travel to target nodes on the separated graphs.
  `GraphScene._apply_drift_rest_slots` (4 Hz) moves the slot each avatar parks
  at to the centroid plus its activity drift. That is
  `BinaryProtocolClient.agent_drift_offset`, from the shared `DriftField` in the
  render store: applied `0x23` actions credit their target's class vertex, and
  `memory_flash` frames credit memory through `record_memory_flash`
  (`beat_pulse.gd`, the frame's optional `agentId`, else every agent). New
  avatars materialise at the centroid.
- **Look.** One MultiMesh of camera-facing sprites, each
  a single triangle circumscribing the disc (`SPRITE_TRIANGLE_UV`; the shader's
  round mask discards the corners), billboarded on the main camera so both eyes
  agree (`memory_point.gdshader`;
  stride 16 = 12 transform + 4 colour). Sprite diameter is the desktop
  size-attenuated point converted to world units (`7.5 · tan(37.5°) / 5`
  cloud-local). Colours come from the `cloudData.ts` tables (namespace / source
  type / age); a Rust test parses the TS source so they cannot drift. Level of
  detail: at most 8 000 sprites (8 000 triangles), namespace-stratified, route
  and sidecar rows always kept. The cloud turns slowly only when reduced motion
  is off and no route is shown.
- **HUD.** Graph tab, Layers grid: `Memory: Off/<count>/Locked/Waiting/Error`
  and `Cloud: Namespace/Source/Age` (both `_press_fire`). The pointer ray picks
  a sprite at 15 Hz and a world-size `Label3D` (top-level, not fit-scaled) shows
  its key and namespace / source type.
- **Flashes.** `resolve_flash(key, ns)` follows `resolveFlashTargets` (exact
  `namespace:key`, bare key, up to three namespace rows, else none) and
  `world_point(row)` gives the burst position, so `memory_flash` bursts land on
  real rows.
- **Route.** A `memoryRoute` text frame (`{type, snapshotId, seq, sentAt,
  path[], sidecar[], query}`; rows root → answer; `path: []` clears) relayed from
  the desktop is gated in Rust by `(sentAt, seq)`. A frame naming another
  snapshot reloads the cloud once and is then applied or dropped. The route is
  sampled as the desktop space view does (quadratic Bézier, quarter-back,
  0.12·length lift) and drawn in three draw calls: one additive surface
  holding five tubes (outer/inner sheath, root #6f9bff → white → tip #ff7a3d
  body, white core, comet tail; `memory_route.gdshader` reveals the trace and
  tapers the 18 % tail on the GPU), beads + comet head/glow, and billboard
  rings (root, answer, pulse, sidecar gold). Off-route sprites dim by 0.75.
  Like the desktop overlay (`depthTest = false`), the three route materials draw
  with `depth_test_disabled` at `render_priority` 10 (`ROUTE_RENDER_PRIORITY`,
  after edges 0 and halos/beams 1), so the opaque glass nodes cannot hide the
  route; the HUD panel, the radial menu and the hover label sit above it at 20
  (`OVERLAY_RENDER_PRIORITY`, pinned in `hud.gd` and `radial_menu.gd` by a Rust
  test). What the user holds sits between them: the wand aim rays
  (`graph_scene.make_aim_ray`) draw at `HELD_RENDER_PRIORITY` 15, because the
  route painted over the user's own ray is a conflicting depth cue. They are
  alpha-transparent at alpha 1 so the priority applies (it only orders the
  transparent pass, which runs after the opaque one), and they keep their depth
  test: the route writes no depth (`depth_draw_never`), so order alone puts the
  ray over it, while the far end of the 5 m beam stays hidden by nodes in front
  of it. This client draws no controller models or hand meshes for the local
  user; avatar hands are remote peers' and stay ordinary world geometry. GUT
  asserts the 10 < 15 < 20 ordering on the built materials. Depth test and
  order are pipeline state of the one multiview draw, so both eyes agree; no
  call or triangle is added.
- **Route framing cue.** The desktop flies the camera to a new route; the
  headset never moves the user's head (comfort, ADR-2107). Instead, for 4.5 s
  after a *new* route (`CUE_SECONDS`, longer than the 2.8 s trace), 12 guide
  dots run from the right controller (else 0.3 m ahead of and 0.25 m below the
  camera) to the answer, white to the answer's orange, growing 6 mm → 18 mm, with
  a brightness wave travelling towards the answer; the answer ring brightens
  ×(1 + 1.2h) and grows ×(1 + 0.45h). Under reduced motion the dots hold still
  and the ring only brightens (opacity fades remain). The dots ride the bead
  MultiMesh (`bead_instances` counts them, hidden at zero size when idle): 12
  triangles, no draw call. The desktop's 10 s late-joiner repeat of the same
  route is a `refresh` (marks, query and stats only): it no longer replays the
  trace or re-fires the cue.
- **Sidecar agreement.** `memoryRoute` carries optional `sidecarTotal` /
  `sidecarAgree` (`xrRelay.ts`, from the panel's `sidecarAgreement`); the
  server relays them only as a consistent pair (`sidecar.len() ≤ total`,
  `agree ≤ sidecar.len()`), else drops both, as the headset parser does. The HUD
  Graph page shows one line under the Memory buttons in the desktop's wording —
  "Route: n of k sidecar hits are in the sample · a of n sampled agree with the
  local top-k", unsampled hits outside the denominator — or, from a relay
  without the counts, only the sampled marks. 20 px font in a 2 px box: the page
  measures 530 px (≤ 532). The sidecar's `method`, `tookMs` and hit objects
  stay in the HTTP query response, never in the frame (Rust test reads
  `xrRelay.ts` `MemoryRouteFrame` against `memory_route::WIRE_FIELDS`).
  The answer ring pulses to xr-pulse's beat clock when it is locked. Under
  reduced motion the route is shown converged, with no comet, pulse ring or
  rotation. Glow is emissive/additive geometry only (Invariant 2).
- **Budget: one allocator for every layer.** `rust/src/frame_budget.rs`
  (`FrameBudget.allocate`) divides the frame between the graph's LOD tiers, the
  cloud, the route and the memory_flash ring pool. It hands out 95 000
  triangles and 48 draw calls: 5 % of each budget is held back for
  frame-to-frame variance. Minimums, in priority order: `other_tris` (HUD,
  controllers, avatars, measured by the scene on the root viewport), the
  graph's far tiers (an impostor quad per node, a ribbon quad per edge,
  labelled nodes on the full mesh), the route at one sample per hop, 2 000
  cloud sprites, 16 gem nodes. Growth to demand in the same order: route curve
  detail (up to 121 centreline samples), cloud (up to 8 000 one-triangle
  sprites), the ring pool (64 × 64 triangles, one call, only while no cloud is
  shown), hulls, gem nodes (to 80), cylinder edges (to 96). Costs are imported
  from `lod.rs`, `hulls.rs` and `memory_*.rs`, never copied; if the minimums
  alone overrun, they are returned with `over_budget` set. Route and sidecar
  rows always draw, even past the cloud cap (at most 128, under the 2 000
  floor).
- **Flashes on the cloud.** With the cloud shown, xr-pulse's bursts call
  `set_row_emphasis(rows, tints, gains, scales)` each frame (replace-all, gain
  1–2.5, scale 1–2, at most 64 rows, empty arrays clear). The layer writes the
  existing sprite buffer: per-instance custom floats carry (tint, gain), which
  `memory_point.gdshader` blends and brightens, and the basis carries the
  scale. No geometry is added; the cloud MultiMesh stride is 20 (12 transform
  + 4 colour + 4 custom).
- **Measured worst case.** The pass check is the renderer's global per-frame
  maximum over the whole run, offscreen passes included (the HUD SubViewport).
  The scene carries the HUD, two controller aim rays and two avatars, and
  `other_tris` / their draw calls are calibrated from the HUD's *dirty* frame:
  a forced re-render of every page, the costliest kept (24 calls, 2 150
  triangles), with that page left open for the run. HP, GL window, Godot 4.6.1,
  2026-10-07, 13 164 nodes / 20 000 edges / 32 hulls:

  | Cloud | Route nodes / sidecar | Flash load | Draw calls | Triangles | p99 | Gems / cylinders / hulls |
  |---|---|---|---|---|---|---|
  | — | — | 64 rings | 31 | 94 554 | 5.56 ms | 67 / 6 / 32 |
  | 6 000 | 13 / 5 | 64 rows | 34 | 94 546 | 6.67 ms | 35 / 8 / 32 |
  | 20 000 | 13 / 5 | 64 rows | 34 | 94 560 | 6.06 ms | 28 / 9 / 32 |
  | 20 000 | 64 / 64 | 64 rows | 33 | 94 546 | 5.88 ms | 40 / 0 / 32 |
  | same, HUD re-rendered every frame | | | 33 | 94 546 | 6.06 ms | 40 / 0 / 32 |

  Re-measured 2026-10-07 (xr-parity: cloud framed on the bench graph, route
  without depth test, 12 cue dots; same rig and scene): graph only 31 / 94 554 /
  2.47 ms; 6 000 + 13/5 34 / 94 558 / 2.95 ms; 20 000 + 13/5 34 / 94 542 /
  2.78 ms; **20 000 + 64/64 33 / 94 558 / 2.48 ms** (gems 40, cylinders 0, hulls
  32, 8 000 sprites, route 4 056 triangles); HUD re-rendered every frame 33 /
  94 558 / 2.78 ms. All pass; pack CPU p99 0.57 ms, lod_build CPU p99 1.11 ms.

  Re-measured 2026-10-07 with the benchmark's controllers built by
  `make_aim_ray` (held priority 15, transparent pass): graph only 31 / 94 554 /
  2.78 ms; 6 000 + 13/5 34 / 94 558 / 2.54 ms; 20 000 + 13/5 34 / 94 542 /
  2.78 ms; **20 000 + 64/64 33 / 94 558 / 2.78 ms**; HUD re-rendered every frame
  33 / 94 558 / 3.03 ms. All pass, with calls and triangles identical to the run
  above. The sweep before it failed its cold first row on the CPU sub-budgets
  only (pack CPU p99 2.62 ms against 2.0, lod_build CPU p99 3.34 ms against 3.0;
  frame p99 3.70 ms). That run used the old ray, and the rerun did not repeat it.
  Root cause and fix: see "CPU gate: L3 pinning and the warm-up limit" below.

  Before the HUD fixes the same combined row read 103 236 triangles / 80 calls
  (idle-HUD reserve), then 95 004 / 81 with the dirty frame reserved.
- **HUD canvas cost.** A HUD re-render cost 59–73 draw calls and up to 8 242
  primitives; now 6–12 calls and 318–1 430 primitives per page
  (`perf/hud_draw_calls.gd`). Causes, measured one at a time and fixed:
  StyleBoxFlat boxes are anti-aliased polygons, a call each, splitting the text
  batch (`scripts/hud_batching.gd` draws every box as a nine-patch from one
  atlas in a z −1 background layer; swatches and CheckButton switches join
  it); glyphs missing from Godot's default font came from fallback fonts with
  their own textures (`fonts/HudSans-SemiBold.ttf`, Open Sans SemiBold plus
  the HUD's 15 symbols, built by `fonts/build_hud_font.py`); the page host's
  clip split every batch (removed; the GUT fit tests hold pages inside it).
  The SubViewport renders only when a control redraws
  (`scripts/hud_render_on_demand.gd`), and the FPS header updates at most every
  2 s, only when the integer changes. Avatar heads went from default 64 × 32
  spheres (4 224 triangles) to 16 × 8 (288).
- **Live scene.** The live GraphScene
  runs the same allocator through `scripts/frame_budget_pass.gd` at 4 Hz: demand
  is what was actually drawn (instance counts per graph layer, the hull mesh, the
  memory layer's `frame_demand()`), and `other_tris` is measured as the
  renderer's frame total minus the budgeted layers (`lod::graph_layer_triangles`
  plus the memory layer's `budget()`). The caps feed the gem and cylinder tiers of
  `build_*_buffer_lod`, the hull rebuild (`GraphParity.max_hulls`) and the memory
  layer (`apply_frame_caps`); caps only move the tier split, so they never
  invalidate the pack plans.

### Constrained layouts
The Layout tab drives the backend layout engine. Six modes cycle through the
`LAYOUT_MODES` enum, POSTed to `/api/layout/mode`
(`graph_scene.gd:213-215`; server enumerates the same list at
`layout_handler.rs:10`): `forceDirected, hierarchical, radial, spectral,
temporal, clustered`. Radial shells POST `/api/layout/radial` with a mode of
`dagRank | typeTier | ego` (`graph_scene.gd:734-742`, `_post_radial` at 983;
server at `layout_handler.rs:135-166`). The Hierarchy toggle PUTs `dagBiasK`
(0.6 on / 0.0 off) and Shells ± nudge `dagLevelDistance` (`graph_scene.gd:888-910`).

**DAG ranks are derived from subclass provenance, not from the edge label**
(ADR-2035, amended 2026-10-02). `edge_type` is a force category: the ingest
folds `rdfs:subClassOf`, `owl:equivalentClass`, `owl:sameAs`,
`rdfs:subPropertyOf` and instance-of all into `hierarchical`, and keeps the
predicate it folded in `owl_property_iri`. The ranker
(`ForceComputeActor::hierarchy_pairs`) layers only edges for which
`Edge::asserts_subsumption` holds: an explicit `subclass_of`-family label, or
`hierarchical` whose `owl_property_iri` is `rdfs:subClassOf`. Domain-root
spokes are labelled `domain_member` and never rank. Before the amendment the
bare label was accepted, so from `7b6330608` each domain root ranked as the
child of its own members (live census: 6400 membership edges in a 16196-edge
rank set; every root at rank 1 below 36-533 of its members).

### Node, halo and edge LOD — the triangle budget (2026-10-07)
PRD-008 budgets ≤ 100k triangles and ≤ 50 draw calls (`perf/README.md`). Before
this work the node gem was a 16×8 sphere (288 triangles) drawn **twice** — the
halo was a `next_pass` shell — and every drawn edge was a capped 8-sided cylinder
(48): 1 000 nodes measured 576 000 triangles, and 13 164 nodes with 20 000 edges
1.04 M once edges were drawn. Three changes, none touching the near look:
- **Halo as a quad layer.** `gem.tres` / `gem_faded.tres` are single-pass; the halo
  is `GraphRoot/NodesHaloMulti` (`materials/node_halo_quad.gdshader`), one additive
  camera-facing quad per full-mesh node (gem tier + faded), fed the same 20-float
  buffer. It reproduces the shell analytically: radius 0.5 + `halo_width`
  (+ badge / query widths), fresnel `(1 − sqrt(1 − (ρ/R)²))^2.5`, centrality,
  badge and query-pulse modifiers, label-fade alpha. Close-up before/after on HP
  (same three nodes, production materials): 0.59 % of pixels differ by > 8/255,
  at ring edges. Low cost hides the layer and reduced motion stops its query pulse
  (`spatial_environment.gd`); `crystal_orb.tres` (avatar cores) keeps the old pass.
- **Node tiers.** The nearest `NEAR_CAP = 80` drawn nodes within `NEAR_RADIUS_M =
  1.0` m of the eye keep the gem (80 × 290 = 23 200 triangles); every other drawn
  node is a 2-triangle impostor (`materials/node_impostor.gdshader`: analytic lit
  sphere, main-camera billboard, opaque with discard) in `NodesImpostorMulti`.
  Labelled nodes stay on the full mesh and count against the cap.
- **Edge tiers.** The `NEAR_EDGE_CAP = 96` edges whose midpoint is nearest the eye
  within 1 m keep the cylinder, now uncapped (32 triangles; the ends sit inside the
  node spheres); every other drawn edge is a 2-triangle ribbon turned about its axis
  toward the main camera (`materials/edge_ribbon.gdshader`) in `EdgesRibbonMulti`,
  16 floats per instance (Invariant 3). Both edge tiers shade through
  `edge_flow_common.gdshaderinc` (pulse, relation grammar, palette), the ribbon
  composites as the cylinder's two alpha layers, and its material mirrors the live
  cylinder uniforms so comfort settings reach it.

Selection is Rust `lod::split_tiers` (O(n) partial select, 10 % distance
hysteresis keyed by node id / endpoint pair), via `build_node_buffer_lod` and
`build_edge_buffer_lod`; camera and radius are converted into GraphRoot space, so
fit scale and two-hand manipulation are respected. Every drawn node keeps its
render position, so ray picking, labels, edges and hulls ignore the tiers.
`lod::scene_triangle_estimate` pins the worst case (both near tiers full, 32
hulls at their bound) at ≤ 97k for 13 164 nodes / 20 000 edges.

`perf/benchmark_scene.tscn` measures this production path every frame — fixture
or `XR_BENCH_NODES` synthetic nodes through `ingest`, edges at production density
(the fixture's 1 500; `XR_BENCH_EDGES`, default 20 000 = `EDGE_SAFETY_CEILING`),
both near tiers always full, caps from the FrameBudget pass — and reports
`node_lod`, `edges`, `hull_layer`, `frame_budget`, and the pack timings in two
clocks: `pack_cpu_ms` / `lod_build_cpu_ms` (thread CPU, `CLOCK_THREAD_CPUTIME_ID`,
`rust/src/thread_cpu.rs`) and `pack_ms` / `lod_build_ms` (wall). The gate is on
CPU time — `pack_cpu_ms` p99 ≤ 2.0 ms and `lod_build_cpu_ms` p99 ≤ 3.0 ms — so a
loaded host's preemption cannot fail it; wall time is reported beside it so
preemption stays visible. Measured on HP (Godot 4.6.1, opengl3, `--xr-mode off`,
dev-profile library, 2026-10-07; `-- extras=0`, graph rows `memory_rows=0`,
combined row `memory_rows=20000 route_hops=63 route_sidecar=64`; caps from the
FrameBudget with its 5 % reserve):

| Run | Draw calls | Triangles | Frame p50 / p99 | pack CPU / wall p99 | lod_build CPU / wall p99 | Caps gem / cyl / hulls / sprites |
|---|---|---|---|---|---|---|
| 1 000 nodes, 1 500 edges, 32 hulls | 7 | 35 942 | 0.43 / 0.93 ms | 0.05 / 0.06 ms | 0.12 / 0.14 ms | 80 / 96 / 32 / — |
| 1 000 nodes, 1 500 edges, no hulls | 6 | 35 016 | 0.44 / 0.93 ms | 0.05 / 0.06 ms | 0.12 / 0.13 ms | 80 / 96 / 0 / — |
| 13 164 nodes, 20 000 edges, 32 hulls | 7 | 94 992 | 2.04 / 2.78 ms | 0.62 / 0.71 ms | 1.11 / 1.29 ms | 75 / 1 / 32 / — |
| 13 164 nodes, 20 000 edges, no hulls | 6 | 94 994 | 2.11 / 2.47 ms | 0.65 / 0.71 ms | 1.15 / 1.30 ms | 80 / 51 / 0 / — |
| combined: + 20k-row cloud + 64-node route (64 sidecar) | 10 | 94 996 | 2.30 / 3.03 ms | 0.65 / 0.73 ms | 1.15 / 1.31 ms | 47 / 5 / 32 / 8 000 |
| 13 164 / 20 000 / 32 hulls **under load** (64 busy loops, load average 38) | 7 | 94 992 | 8.08 / 14.6 ms | **0.91 / 2.52 ms** | **1.64 / 4.47 ms** | 75 / 1 / 32 / — |

**CPU gate: L3 pinning and the warm-up limit (2026-10-07).** The gate failed
intermittently on HP: one cold sweep read pack CPU p99 2.62 ms and lod_build
3.34 ms. Per-frame series from 10 fresh cold runs showed the cause.
- **Not the first build.** Sample 0 (pack 4.9–5.9 ms, LOD 10.7–16.2 ms) is
  already in the skipped warm-up window.
- **What it was.** Pairs of adjacent frames, 3–25 per run, where the
  unchanged pack ran about 8× slower on-CPU (0.50 → 4.2–4.6 ms). Thread CPU ≈
  wall on those frames, so the thread was running, not preempted.
- **The experiment.** Pinned to one L3 domain (`taskset`, 12 CPUs), or to one
  core: 0 spikes in 10 runs, worst sample 0.87 ms, same 0.50 ms median.

The Threadripper 7965WX has four 6-core L3 domains. When the scheduler migrates
the main thread across them, the pack's working set goes cold, and how often
that happens depends on host load. Quest is a single cluster, and the budget is
for the pack itself. So the benchmark pins its main thread to the L3 domain it
starts on: `BinaryProtocolClient.pin_thread_to_l3` → `thread_cpu.rs`
`pin_current_thread_to_l3`, which intersects the domain with the existing cpuset
and reports the result as `cpu_affinity`. The live client is not pinned.

The 90-sample warm-up window used to be dropped silently, so nothing limited a
real first-build hitch. Its worst sample is now gated:
`warmup_pack_cpu_ms_max` ≤ 12 ms and `warmup_lod_build_cpu_ms_max` ≤ 33 ms
(about 2× the worst measured first build; 33 ms is three 90 Hz frames). This is
`cpu_gate()` in `perf/benchmark.gd`, and `test_benchmark_cpu_gate.gd` covers a
normal build, a hitch, and a sustained regression.

Self-pinned and unpinned by the runner, 10 fresh cold runs gave:
- 0 pack samples over 2 ms (worst 1.12 ms);
- p99 pack 0.60–0.64 ms and LOD 1.05–1.11 ms;
- warm-up 4.9–5.8 / 10.9–13.2 ms.

In the full sweep every row passes every gate:
- 31–34 calls, 94 542–94 558 triangles, frame p99 2.24–2.78 ms;
- pack p99 ≤ 0.63 ms, LOD p99 ≤ 1.10 ms, worst warm-up 6.2 / 16.4 ms.

The loaded row is the gate's point: wall pack p99 reads 2.52 ms (it would have
failed a wall-clock gate) while the pack's own CPU is 0.91 ms; both CPU gates
pass. The run as a whole fails, correctly, on frame time — a saturated host
really misses 90 fps. Rows with the HUD, controllers and avatars (`extras`)
measure the same scene totals (root viewport) but exceed the global draw-call
and triangle budgets on the HUD canvas's ~1 Hz dirty frames (see the HUD
canvas open item). Reference points on the same rig: all-gem nodes with the
halo pass, 1k nodes = 576 000 triangles; edges as capped cylinders, 1k = 130 030
and 13k = 1 044 370; the per-frame pack before the plans, 5–8 ms p99.

**CPU: per-frame pack plans** (`rust/src/render_store_pack.rs`). Only positions
change from one frame to the next, so the store records once per drawn instance
everything else — slot, packed colour and custom channels, unit-scale size (nodes),
endpoint slots and style (edges) — derived from the full pack's output, and
replays it while a `visual_epoch` is unchanged. Every mutation that can change an
instance's look or the drawn set bumps the epoch (`touch()`); per-frame feeds bump
only on a real change (`upsert` analytics, node kind, agent expiry, label-fade
membership). Edge tiers are chosen from midpoints; near cylinders are transformed
every frame and far ribbons half per frame (alternating parity, in place), so each
ribbon is at most one frame old and any tier change rebuilds the far tier at once.
Per-frame tints that must not invalidate a plan (attention heat) go through
`RenderStore::live_tint`. Steady state allocates nothing (`tests/pack_alloc.rs`,
counting allocator: 79 allocations per frame before, 0 after). GraphScene now runs
both packs every frame; the 45 Hz node/edge alternation (from when GDScript looped
over instances) is gone, so edge ends no longer trail their nodes by a frame.
`examples/lod_pack_profile.rs` measures the dev profile the headset loads: on HP
0.50 ms p50 / 0.52 ms p99 at 13 164 nodes / 20 000 edges.

**The headset runs the debug library.** `visionclaw_xr_gdext.gdextension` maps
the editor (`linux.debug.x86_64`) to `target/debug`, and the desktop-OpenXR launch
is the editor binary. Unoptimised, the 13k node pack alone took 18–20 ms per frame;
the crate's `[profile.dev]` is `opt-level = 2` (debug assertions and overflow
checks kept).

### Desktop parity: domain colour, settings sync, cluster hulls (2026-10-07)
Three desktop behaviours ported under the existing invariants (audit WP1, WP2,
WP4). Rust owns every rule; `scripts/graph_parity.gd` is the only scene-side
owner and GraphScene forwards it two hooks (`handle_control`, `route_text`).

- **Domain palette (default).** `domain_palette.rs` is a deliberate copy of
  `client/src/features/graph/utils/domainColors.ts` (`DOMAIN_COLORS`,
  `getDomainColor` alias rules, fallback `#90A4AE`) plus the `GemNodes.tsx` hub
  lift (saturation `+min(cc/30, 0.1)` ≤ 0.95, lightness `+min(cc/40, 0.06)` ≤ 0.8;
  no authority term — the XR wire has none). The lift runs in three.js's
  *linear* HSL and is returned sRGB-encoded, matching what the desktop displays;
  ground-truth hexes come from the worktree's three.js 0.183.0.
  `tests/domain_palette_parity.rs` parses the TS file **and** `hud.gd`'s
  `KEY_DOMAIN_SWATCHES` / `KEY_HULL_SWATCHES`, so the table, its desktop source
  and the Key tab cannot drift apart. Domain comes from `initialGraphLoad`
  `metadata.domain ?? metadata.source_domain`. Precedence is unchanged: query
  mark, then agent status, then the base colour; the anomaly red blend applies
  in both modes (`render_store::anomaly_blend`). The Graph tab's
  **Colour: Domain / Community** button switches the base colour.
- **Inbound settings and filter sync.** `settings_sync.rs` mirrors
  `textMessageHandler.ts`: `settingsUpdated` (ADR-2047) is dropped when its
  timestamp is ≤ the last applied for that category, or when `updatedBy` is this
  session's pubkey (`set_own_pubkey`); `nodeFilter` is applied to the render
  store's draw domain with the desktop `useGraphFiltering` predicate (linked_page
  gate, quality `quality_score ?? quality ?? qualityScore` else `min(1, degree/10)`,
  authority else 1.0, AND/OR — note the desktop/server OR mode admits everything
  when only one check is on); agents are never filtered; filter-hidden nodes also
  drop their edges and search hits. `physics` triggers a `GET
  /api/settings/physics` whose body updates the HUD's tracked values (repelK,
  restLength, Hierarchy, shells, planes, 3D/Flat) — this closes the old one-way
  write, where a peer's change left stale button faces. The read waits while a
  local write is in flight. `graphUpdated` is coalesced by `RefetchGate` into
  `requestInitialData`, held to the server's 30 s per-connection cooldown
  (`position_updates.rs:382`) so the last change is never dropped. Receipt never
  writes back; writes stay on the HUD-press NIP-98 path (Invariant 6).
- **Cluster hulls.** `hulls.rs` follows `ClusterHulls.tsx`: group by
  `cluster_id` (V3 offset 36) when any node has one, otherwise — only in
  *Communities* mode, the desktop's opt-in `communityFallback` — by Louvain
  community; drop clusters < 4, keep the 32 largest; pad 15 % from the centroid;
  extrude flat clusters ±35 along the thinnest axis; desktop palettes
  (`GPU_CLUSTER_COLORS`, community HSL in linear space). Each cluster is reduced
  to its extreme points along 64 fixed directions, so a hull is ≤ 124 triangles
  and the layer ≤ 3 968; all hulls are one `ArrayMesh` surface under `GraphRoot`
  (**one draw call**), drawn by `materials/cluster_hull.gdshader` (unshaded,
  `blend_mix`, no depth write, double-sided, opacity 0.08 plus a fresnel edge —
  no post-process, no screen texture). Only drawn nodes are hulled. The scene
  polls at 2 Hz and Rust skips the build unless a drawn position moved ≥ 1
  server unit. HUD **Hulls: Off / Clusters / Communities** cycles the source;
  without a server clustering run, *Clusters* honestly shows nothing (ADR-031 D6).
  Measured on HP (Godot 4.6.1, opengl3, `perf/benchmark_scene.tscn` at the 32-hull
  cap): draw calls 6 → 7, triangles +926.

## Known divergences & open items
- **project.godot vs runtime.** `config/features` still says Godot 4.3 / Forward
  Mobile; the working build is 4.6.1-stable Compatibility. The header comment was
  corrected in text on 2026-10-07 (it now states the verified runtime). The array
  itself stays open: it is editor-managed metadata (4.6.1 `--import` on HP leaves
  the file byte-identical), and its renderer tag is the Quest renderer decision
  (`rendering_method.mobile="mobile"` vs the Compatibility runtime that works),
  which belongs to audit WP0 with an ADR and a headset receipt (Invariant 1). A
  hand-edit would pre-empt that decision. Documented in `xr-client/README.md:15-18`
  as well as here.
- **Benchmark triangle budget — Resolved 2026-10-07.** It failed at baseline
  (576 000 triangles for 1 000 gem nodes; 1.04 M at 13k nodes once edges were
  drawn). The halo quad layer and the node and edge LOD above bring every
  benchmark run under 100k with ≤ 6 draw calls.
- **Instance colour convention — Corrected 2026-10-07.** Version 0.1.4 said the
  gem material showed instance colours lighter than their sRGB swatch. That was
  inferred from the StandardMaterial flags, not measured, and it is wrong: on HP
  (Godot 4.6.1, Compatibility/opengl3) unlit `gem.tres` renders `#646b9f` as exactly
  `#646b9f` on both a SubViewport and the root window, with `vertex_color_is_srgb`
  off *or* on. Instance colours pass through unchanged, so the palette hexes, the
  Key swatches and the desktop's displayed colours already agree. The convention is
  "COLOR is used raw"; `tests/unit/test_instance_color_space.gd` renders the gem,
  the impostor and the hull shader under GL and compares pixels with the hex
  (pending under the headless dummy renderer, enforced in CI's Xvfb job). The OpenXR
  swapchain path is not measured here and needs a headset check.
- **GUT on Godot 4.6 — Resolved 2026-10-07.** GUT 9.3.x does not compile on
  Godot ≥ 4.5 (its `Logger` shadows the new native class), and GUT 9.7.x is the
  Godot 4.7 line (`godot_4_7` branch): on 4.6.1 it logs two Parse Errors
  (`godot_singletons.gd:5` names the 4.7-only `AccessibilityServer`;
  `stub_params.gd:16` returns null as `StringName`). GUT **9.6.1** — upstream tag
  `v9.6.1`, commit `c80954f4`, the Godot 4.6 line on `main` — is vendored in
  `xr-client/addons/gut/` (provenance in `addons/README.md`) and logs none. GUT
  skips a script that fails to parse and still exits 0 (shown on HP: a broken
  test file left GUT at exit 0), so `tests/gut_guard.sh` fails CI on any
  Parse/Compile Error in the import or GUT logs, or when GUT's `Scripts N` differs
  from the `test_*.gd` files on disk. On HP: 26/26 scripts, 182/182 under GL.
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
  posture as `SWARM_STATUS_COLORS`); a palette change must update both. Since
  2026-10-07 the Key also lists the domain palette (default colour mode) and the
  hull palette, and those two tables are parity-tested from Rust.
- **Memory cloud — not yet seen in a headset.** The layers are exercised by
  GUT on Godot 4.6.1 (HP) and by screenshots in a desktop GL window
  (`tests/visual/memory_cloud_capture.gd`). Stereo agreement of the main-camera
  billboards, sprite legibility at fit scale, label reach and the route's
  additive brightness need a VIVE Pro session. The server relay of `memoryRoute`
  and the desktop sender are owned by the beat-clock relay work (WP5).
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
7. DAG-rank detection ranks class subsumption only, decided by `owl_property_iri`
   provenance (`Edge::asserts_subsumption`), never by the `hierarchical` force label.
8. Work-layer embodiments have ONE pose writer (`agent_choreography.gd`); the
   proxemics arc, nudges or drifts must never write their transforms. Avatars
   and effects sit under unit-scale roots, never under `GraphRoot` (ADR-2109).
9. Demo mode enters only through the real registry doors (`ingest`,
   `apply_agent_state`, `retire_agents`) from `agent_demo_director.gd`; no
   scene-side demo rendering branch, no synthetic position frames (ADR-2109).
10. Microphone capture is opt-in per session (HUD Mic toggle, off by default),
    shows the `● MIC` header badge while active, and its audio is only analysed
    in memory: never recorded, stored or transmitted (ADR-2134,
    `permissions-required.md`).

## Change process
Edit the affected `.gd`/`.rs` file, run `cargo test -p visionclaw-xr-gdext`
(479 headless tests — 361 library + 118 integration — as of 2026-10-07, no
headset/Godot/network needed; peer parity tests read `client/src`). GUT
(`tests/unit`, vendored 9.6.1; check with `bash tests/gut_guard.sh <gut.log>`) needs the
4.6.1 editor, a `--headless --import` pass and the native library built for the
host (`cargo build -p visionclaw-xr-gdext`); pass `--xr-mode off` (as CI does),
because the project enables OpenXR and a headless run otherwise probes the
installed runtime and crashes on HP when SteamVR is active (182 tests on HP,
2026-10-07: 182 pass under GL; headless 179 pass, 3 GL-only tests pending). Any change
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
