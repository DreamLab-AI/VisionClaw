---
title: XR Client Architecture
doc_id: VC-XR
version: 0.1.7
status: draft-for-ratification
verified_commit: 
changelog:
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
  - xr-client/perf/benchmark.gd
  - xr-client/rust/src/beat.rs
  - xr-client/rust/src/semantic.rs
  - xr-client/rust/src/attention.rs
  - xr-client/scripts/beat_pulse.gd
  - xr-client/scripts/memory_bursts.gd
  - xr-client/rust/src/webrtc_audio.rs
  - xr-client/rust/src/memory_cloud.rs
  - xr-client/rust/src/memory_route.rs
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
- **memory_flash bursts** (`memory_bursts.gd` under `AgentEffectsRoot`): one pooled ring
  MultiMesh of ≤ 64 slots, recycling the oldest, in one draw call. Colour, scale, lifetime,
  implode motion and ring count follow the verb, with a namespace hue jitter computed in
  three.js linear space. A burst lands on the memory cloud's point when the
  `xr_memory_cloud` layer resolves it, else on a hashed 0.45 m shell around the graph centre.
  Reduced motion holds the ring size and only fades it.
- **Attention heat** (`attention.rs`, render store): every applied `0x23` action touches its
  target. The heat has a 20 s half-life, saturation 1.5 and 512 entries, and brightens the node
  colour in place without recolouring it. It never touches the edge buffer.
- **Beat clock** (`beat.rs`, `pulse.rs`, `beat_pulse.gd`): the arbiter chooses a mic lock first,
  then a tap until the desktop's clock changes, then the relayed desktop `beatClock` (fresh
  within 6.5 s), then the last tap.
  - The server offset comes from the JSON ping/pong round trip every 2 s (the minimum-RTT
    sample of eight).
  - One `beat_pulse` uniform per frame drives the shared `node_halo` and `edge_flow` materials
    and the burst opacity. It is an emission swell, not a post-process (Invariant 2), and
    reduced motion scales it to ≤ 0.25.
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
- **Placement and look.** The layer sits under `GraphRoot` at the server origin
  and `CloudRoot` carries the desktop `cloudScale` (5), so the cloud surrounds
  the graph as it does on desktop. One MultiMesh of camera-facing quads
  (`memory_point.gdshader`, billboarded on the main camera so both eyes agree;
  stride 16 = 12 transform + 4 colour). Sprite diameter is the desktop
  size-attenuated point converted to world units (`7.5 · tan(37.5°) / 5`
  cloud-local). Colours come from the `cloudData.ts` tables (namespace / source
  type / age); a Rust test parses the TS source so they cannot drift. Level of
  detail: at most 12 000 sprites (24 000 triangles), namespace-stratified, route
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
  The answer ring pulses to xr-pulse's beat clock when it is locked. Under
  reduced motion the route is shown converged, with no comet, pulse ring or
  rotation. Glow is emissive/additive geometry only (Invariant 2).
- **Budget.** Measured on HP (desktop GL window, 1k-node benchmark): +4 draw
  calls (6 → 10). The cloud adds 12 000 triangles at 6 000 rows and 24 000 at
  20 000 rows (capped). A 12-hop route adds 13 776 triangles; a 64-hop route
  is bounded at about 30 000 (tube ≤ 19 200 at 320 samples, beads 80 per sphere).
  The graph-only baseline of that benchmark is already 576 000 triangles (288 per
  node sphere × 2 passes for the halo `next_pass`), far above the 100k budget;
  see the open item below.

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

### Node-mesh LOD and the triangle budget (2026-10-07)
The gem node (16×8 sphere plus the halo `next_pass`) costs 576 triangles, so the
PRD-008 budget (≤ 100k triangles, ≤ 50 draw calls, `perf/README.md`) failed at
1 000 nodes (576 000) and the 13k production graph would be ≈ 7.5 M. Two tiers fix
it without touching the near look:
- **Gem tier**: the nearest `NEAR_CAP = 96` drawn nodes within `NEAR_RADIUS_M =
  1.0` m of the eye keep the full gem material — 96 × 576 = 55 296 triangles, under
  a ~60k near-field ceiling. Labelled (faded) nodes stay on the full mesh and count
  against the cap. Selection is Rust `lod::split_node_tiers` (O(n) partial select,
  10 % distance hysteresis so boundary nodes do not flicker), called through
  `BinaryProtocolClient.build_node_buffer_lod`; camera and radius are converted
  into GraphRoot space, so the fit scale and two-hand manipulation are respected.
- **Impostor tier**: every other drawn node is one camera-facing quad
  (`materials/node_impostor.gdshader`: analytic lit sphere with specular, rim and
  the centrality/query rim tells; opaque with discard; billboarded with the main
  camera basis as Godot's own billboard mode does, so both eyes agree) in a single
  `NodesImpostorMulti` under GraphRoot (`scripts/node_lod.gd`) — +1 draw call.
  Same 20-float instance layout, and every node keeps its render position, so ray
  picking, edges, labels and hulls ignore the tier.

`perf/benchmark_scene.tscn` now measures this production path (fixture → V3 frame
→ `ingest` → LOD split every frame, worst case: gem cap always full) and reports
`node_lod`, `lod_build_ms_p50/p99` and `hull_layer`; `XR_BENCH_NODES=13164` runs
production density, `XR_BENCH_HULLS=0` drops the hull layer. Measured on HP
(Godot 4.6.1, opengl3, `--xr-mode off`, dev-profile library, 2026-10-07):

| Run | Draw calls | Triangles | Frame p50 / p99 | LOD pack p99 | Result |
|---|---|---|---|---|---|
| 1 000 nodes + 32 hulls | 4 | 58 030 | 0.31 / 0.93 ms | 0.23 ms | pass |
| 1 000 nodes, no hulls | 3 | 57 104 | 0.31 / 0.93 ms | 0.16 ms | pass |
| 13 164 nodes + 32 hulls | 4 | 84 370 | 2.02 / 2.22 ms | 2.17 ms | pass |
| 13 164 nodes, no hulls | 3 | 81 432 | 2.02 / 2.47 ms | 2.13 ms | pass |

Before the LOD the 1 000-node scene measured 576 000 triangles. The benchmark
renders nodes (and hulls) only: edges are not in it. The live scene's edge
cylinders (8-sided, capped, up to `EDGE_SAFETY_CEILING`) are an open item below.

**The headset runs the debug library.** `visionclaw_xr_gdext.gdextension` maps
the editor (`linux.debug.x86_64`) to `target/debug`, and the desktop-OpenXR launch
is the editor binary. Unoptimised, the 13k node pack took 18–20 ms per frame
(p99 20.3 ms, failing 90 fps); the crate's `[profile.dev]` is now `opt-level = 2`
(debug assertions and overflow checks kept), which gives the numbers above.

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
  (576 000 triangles for 1 000 gem nodes); the node-mesh LOD above brings 1k to
  58 030 and 13k to 84 370 with ≤ 4 draw calls. **Still open: edges.** The live
  scene draws up to 20 000 edge cylinders (8 radial segments plus caps ≈ 32
  triangles each, ≈ 640k at the ceiling), which the benchmark does not render;
  an edge LOD (uncapped or ribbon impostors beyond the near field) is the next
  budget item.
- **Instance colour is treated as linear.** `gem.tres` uses
  `vertex_color_use_as_albedo` without `vertex_color_is_srgb`, so every node palette
  (community, query, agent, and now domain) shows lighter than its sRGB swatch and
  than the desktop hex. Domain colours and hulls keep that convention, so a hull
  and its nodes read as one hue. Flipping the flag changes every palette at once
  and needs a headset look review.
- **GUT on Godot 4.6 — Resolved 2026-10-07.** GUT 9.3.x does not compile on
  Godot ≥ 4.5 (its `Logger` shadows the new native class). GUT 9.7.1 (upstream tag
  `v9.7.1`, commit `aeb5d4f3`) is now vendored in `xr-client/addons/gut/`, and CI
  runs the suite on Godot 4.6.1 with the vendored copy. Both documented invocations
  (`-gdir=res://tests/unit -ginclude_subdirs -gexit` and
  `-gconfig=res://.gutconfig.json`) pass on HP (121/121).
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
(449 headless tests — 344 library + 105 integration — as of 2026-10-07, no
headset/Godot/network needed). GUT (`tests/unit`, vendored 9.7.1) needs the
4.6.1 editor, a `--headless --import` pass and the native library built for the
host (`cargo build -p visionclaw-xr-gdext`); run it with `--xr-mode off` so a
live SteamVR session is never grabbed (149 tests pass on HP, 2026-10-07). Any change
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
