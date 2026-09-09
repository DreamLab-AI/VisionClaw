# VisionClaw XR — Godot OpenXR client

Godot + godot-rust (gdext) + OpenXR client per
[PRD-008](../docs/archive/prd/PRD-008-xr-godot-replacement.md) and
[ADR-071](../docs/archive/adr/ADR-071-godot-rust-xr-replacement.md).

**Two run targets, one codebase:**
- **Quest 3 native APK** — the sole *ship* target. Cross-build is currently
  **frozen** (no Android NDK provisioned in this environment; see below).
- **Desktop OpenXR (SteamVR / VIVE Pro)** — the close-out *validation* target
  per [ADR-136](../docs/archive/adr/ADR-136-desktop-openxr-vive-validation-target.md).
  This is the path that has **actually rendered on a headset** — see
  "VIVE Pro desktop-OpenXR bring-up (2026-08-22)" below.

> **Version note.** The project was authored against Godot 4.3; the first
> working on-headset render (2026-08-22) was achieved on **Godot 4.6.1-stable**
> using the **Compatibility (OpenGL 3) renderer**. Where this README still says
> "4.3", read it as "the pinned editor of the day"; the *runtime* facts that
> matter for a headset session are in the VIVE bring-up section.

## Visual experience and comfort

The shared instrument theme, spatial grid, restrained graph materials and reversible
comfort controls are documented in [ADR-2107](../docs/adr/ADR-2107-compatible-xr-visual-experience.md).
Reduced motion is on by default. Use the Help tab to change motion and low-cost
rendering, or set `XR_REDUCED_MOTION=0` / `XR_VISUAL_QUALITY=low` before launch.
These settings apply to the running scene; the desktop captures do not certify
headset comfort or Quest frame budgets.

Run the offline production-material fixture with
`godot --path xr-client --rendering-method gl_compatibility --xr-mode off res://tests/spatial_visual_fixture.tscn`.
Set `XR_VISUAL_CAPTURE` to choose its PNG output. Build the current native extension
first; an old copied library can expose incompatible method signatures.

## Layout

```
xr-client/
├── project.godot                       Godot 4.3 project root
├── export_presets.cfg                  Quest 3 arm64 Android preset
├── visionclaw_xr_gdext.gdextension     Manifest binding the Rust .so
├── icon.svg                            Project icon
├── android-export-template-config.txt  OpenXR loader notes
├── permissions-required.md             Android permission justification
├── scenes/
│   ├── XRBoot.tscn                     Boot: OpenXR init + capability probe
│   ├── GraphScene.tscn                 Graph rendering + AvatarSpawner
│   ├── HUD.tscn                        Settings / room controls / debug
│   └── Avatar.tscn                     Per-remote-presence template
├── scripts/                            GDScript wiring (no business logic)
├── materials/                          gem / crystal_orb / agent_capsule
├── addons/                             Godot OpenXR Vendors (CI-installed)
└── rust/                               gdext crate (see ./rust/Cargo.toml)
```

## Build prerequisites

| Tool | Version | Notes |
|---|---|---|
| Godot 4.3 stable | 4.3 | Both editor and Android export templates |
| Godot OpenXR Vendors plugin | 3.0.x | Install via AssetLib at first project open |
| Rust toolchain | stable 1.82+ | `rust-toolchain.toml` not pinned in this dir; uses workspace default |
| Android target | `aarch64-linux-android` | `rustup target add aarch64-linux-android` |
| Android NDK | r26d | Pinned in `xr-client/android/local.properties.template` (created in W2) |
| `cargo-ndk` | latest | `cargo install cargo-ndk` |
| JDK | 17 | Android Gradle plugin requirement |

## Build steps (manual)

```bash
# 1. Build the gdext .so for the host OS (for editor preview)
cargo build -p visionclaw-xr-gdext --release

# 2. Build for Quest 3 (arm64 Android)
cd xr-client/rust
cargo ndk -t aarch64-linux-android -o ../addons/visionclaw_xr_gdext build --release
cd ../..

# 3. Open Godot, import project, install OpenXR Vendors via AssetLib

# 4. Export the APK
godot --headless --export-release "Quest 3 arm64" xr-client/export/visionclaw-xr.apk

# 5. Side-load to Quest 3 (developer mode + USB debugging)
adb install -r xr-client/export/visionclaw-xr.apk
adb shell am start -n uk.xrsystems.visionclaw.xr/com.godot.game.GodotApp
```

> **Quest APK cross-build is FROZEN.** Steps 2 and 4–5 above are the design
> path but do **not** run in the current environment: there is no Android NDK,
> no `aarch64-linux-android` Rust std component, and no `cargo-ndk`. All headset
> validation to date is via the **desktop OpenXR** path (see below); the arm64
> APK build is pending a provisioned Android toolchain.

## Desktop OpenXR run (VIVE Pro / SteamVR — the working headset path)

The canonical, verified way to get the client onto a real headset today is
**not** the APK — it is the Godot Compatibility renderer on native X11 driving
SteamVR:

```bash
# Prereqs on the render host (HP-Desktop, CachyOS):
#   - Godot 4.6.1-stable (Compatibility build)
#   - SteamVR running, VIVE Pro tracked (lighthouses up)
#   - NVIDIA 580 open driver (nvidia-580xx-open-dkms, PINNED — see constraints)
#   - ~/.config/openxr/1/active_runtime.json → SteamVR

XR_BACKEND_WS=ws://<backend-host>:4000 \
XR_NOSTR_SECRET=<hex-secret> \
godot --path xr-client \
      --rendering-driver opengl3 \
      --display-driver x11 \
      res://scenes/XRBoot.tscn
```

- `XR_BACKEND_WS` — presence/graph WebSocket. Point at the dev backend
  (`ws://192.168.2.132:4000` on the LAN). **Use `:4000` directly**; nginx `:3001`
  does not proxy `/ws/presence`.
- `XR_NOSTR_SECRET` — hex Nostr secret key. **Required** — besides the presence
  challenge/response handshake it signs the NIP-98 `authenticate` on the graph
  socket that gates node drag/pin (server-authoritative drag). Without it, presence
  won't join and drags are rejected. Any BIP-340 key works in dev
  (`graph_scene.gd:288` → `NostrAuth.create(OS.get_environment("XR_NOSTR_SECRET"))`;
  `transport.rs:75`).

### Render constraints (learned the hard way, 2026-08-22)

These are not preferences — each one is a workaround for a concrete failure
observed bringing up the VIVE Pro:

| Constraint | Why |
|---|---|
| **Compatibility (OpenGL 3) renderer is mandatory** (`--rendering-driver opengl3`) | The RenderingDevice / Vulkan **multiview tonemapper is broken on SteamVR + Linux + NVIDIA** — the stereo submission fails. Compatibility is the only renderer that submits both eyes. |
| **Glow OFF** | Godot's glow post-process breaks Compatibility XR multiview submission (blank/black second eye). Disabled in `WorldEnvironment`. |
| **NVIDIA 580, not 610** (`nvidia-580xx-open-dkms`, pinned) | The 610 driver fails to render the GL multiview **second eye**; 580 open renders both eyes correctly. Pin the driver. |
| **Native X11** (`--display-driver x11`) | SteamVR's Linux compositor path; Wayland was not brought up. |

## Test the gdext crate (headless)

```bash
cargo test -p visionclaw-xr-gdext
```

The headless suite — 9 integration files under `rust/tests/`
(`binary_protocol_edge`, `interaction_raycast`, `lod_thresholds`,
`pose_wire_round_trip`, `presence_handshake`, `property_binary_protocol`,
`property_interaction`, `property_lod`, `visual_fixture`) plus 100+ per-module
unit tests — runs in <1 s on a workstation. No Quest, no Godot runtime, no network
required. (PRD-019 tracks the canonical cross-crate total: 141 headless tests green.)

## gdext-registered classes

Read by `scripts/graph_scene.gd`; the registration lives in
`rust/src/lib.rs`. Each class is a `RefCounted` constructed via
`ClassName.create()` from GDScript:

| Class | Module | Signals |
|---|---|---|
| `BinaryProtocolClient` | `binary_protocol.rs` (+ `render_store.rs`) | `position_updated(node_id: u32, position: Vector3, velocity: Vector3)` — also fronts the Rust **`RenderStore`** render offload via `#[func]` adapters (`build_node_buffer`, `faded_node_buffer`, `set_labelled`, `build_edge_buffer`, `hunt`, `nodes_near`, `upsert`, `set_meta`); decodes Protocol V3 **and** the V5 wrapper (`0x05` + 8-byte broadcast seq) |
| `PresenceClientNode` | `presence.rs` | `avatar_joined(did, display_name, avatar_id)`, `avatar_left(avatar_id)`, `avatar_pose_updated(avatar_id, head_pos, head_rot, has_left, has_right)`, `presence_kicked(reason)` |
| `XrInteraction` | `interaction.rs` | `node_targeted(node_id, distance)`, `node_grabbed(node_id, position)`, `haptic_pulse(controller, intensity)` |
| `LodPolicy` | `lod.rs` | (no signals; `should_recompute()` + `classify_distance()` getters) |
| `SpatialVoiceRouter` | `webrtc_audio.rs` | (no signals; `attach_track`, `detach_track`, `update_listener`) |
| `AgentAvatarNode` | `avatar_state.rs` | `activity_changed` — copresence activity state machine + gaze-attention |
| `ProxemicsSolver` | `proxemics.rs` | (no signals; Hall's-zones arc placement) |
| `GazeTracker` | `gaze.rs` | (no signals; unified head/eye gaze ray + one-euro smoothing) |
| `SelectionArbiterNode` | `selection.rs` | `selection_made(node_id: u32, did_nostr: GString, resolver: i32)` — controller-ray / pinch / gaze-dwell arbiter |
| `NostrAuth` | `signer.rs` | (no signals; BIP-340 presence challenge/response signer) |

All classes are `#[class(no_init, base = RefCounted)]` — constructed from
GDScript via `ClassName.create()`.

> **Interaction note (2026-08-22 bring-up).** `XrInteraction` grab now does a
> literal ray/sphere intersection with `TARGET_RADIUS` tightened `1.0 → 0.05`
> and a best-aligned pick, casting in **world space** (previously it tested a
> world-space ray against server coordinates, so grabs missed). Locomotion is
> trackpad-driven; the HUD is a world-anchored, wand-grabbable panel (grip to
> move). Controllers bind via `openxr_action_map.tres` to
> `htc/vive_controller` with `pose="aim"`.

## VIVE Pro desktop-OpenXR bring-up (2026-08-22)

First working **on-headset render** of the Godot XR client in project history —
branch `xr-vive-runtime`. Stack: **Godot 4.6.1-stable Compatibility renderer +
NVIDIA 580 open + native X11 + SteamVR** on HP-Desktop, VIVE Pro.

**What rendered / worked in the headset:**
- **Both eyes** — stereo submission via the Compatibility renderer (the RD/Vulkan
  multiview tonemapper is broken on SteamVR/Linux/NVIDIA; glow off; NVIDIA 580
  for the GL multiview second eye).
- **Dual-wand grab** — world-space raycast, literal ray/sphere pick
  (`TARGET_RADIUS` 1.0→0.05). **Right wand confirmed**; left wand still WIP.
- **Trackpad locomotion**.
- **Movable HUD** — world-anchored, grip-to-move on either wand.
- **Graph render** — top-5% node cap (**640 nodes**), per-node hue fallback,
  **adaptive fit** (GraphRoot scales/recentres the streamed graph into a
  room-sized volume, re-measured **each frame** so it tracks physics spread).
- **Client-side optimistic position hunting** — `_node_targets` eased into
  `_node_positions`, matching the desktop "GPU settles fast, clients hunt" model.
- **Edges** — the backend `initialGraphLoad` node limit was raised **200 → 3000**
  so edges actually reach the client (edge count **49 → 7558**); client renders
  edges only between displayed nodes, with robust src→tgt orientation.

**Backend changes that made the graph live:**
- **Continuous settle** default (was a FastSettle latch that froze the graph
  once converged → positions stopped streaming).
- **`PinNodePositions` GPU injection** on grab — a dragged node perturbs its
  neighbours' springs; grab re-arm on release.
- `wire.rs` decode-panic guard on truncated frames.
- Physics is live-tunable via dev-auth `PUT /api/settings/physics`
  (`Authorization: Bearer dev-session-token`).

**Still pending / WIP (honest):**
- **Canary fires NOT done** — `CANARY-VC-M1-HUD`, `CANARY-VC-M4-RAY`,
  `CANARY-VC-COM18-INTERV` remain **armed, unfired**. On-device render was
  achieved; the canary predicates have not yet been observed firing, so the
  P2-M* tiers are **not** promoted to `integrated`.
- **Edge tracking** — edges render but per-frame tracking of moving endpoints
  needs follow-up.
- **Adaptive-fit-on-grab** — fit interaction during an active grab is unfinished.
- **Left wand** — only the right wand grab is confirmed.
- **Glow parity** — glow is off (Compat XR constraint); visual parity with the
  desktop client is outstanding.
- Debug prints retained in the client (WIP branch).

## VIVE render hardening (2026-08-30, branch `xr-vive-hardening`)

The 2026-08-22 section above rendered a **capped subset** (640 nodes) with per-frame
GDScript hot loops that collapsed at density. This branch made it full-density and
production-shaped — decision record in
[ADR-137](../docs/archive/adr/ADR-137-xr-render-offload-and-runtime-quality-dials.md).

**Architecture — render offload.** The per-frame position-hunt and MultiMesh buffer
packing moved from GDScript into a pure Rust `RenderStore` (`rust/src/render_store.rs`),
fronted by `BinaryProtocolClient`. GDScript now calls `build_node_buffer(ids, comp,
0.7, 1.9)` / `build_edge_buffer(pairs, radius)` / `hunt(...)` and hands the returned
`PackedFloat32Array` straight to the MultiMesh (layout: 12 transform + 4 colour + 4
custom floats/node, matching `GraphScene.tscn`'s `use_colors`/`use_custom_data` flags).
Node colour and the edge `scaled_local` basis are computed Rust-side. **Result:**
13,164 nodes / 145,692 edges at **90 fps** (was ~11 fps in GDScript; GDScript held
90 fps only to ~3k).

**Budgets are topology-derived.** Node/edge draw budgets come from the received
topology (bounded by a safety ceiling), not the old hardcoded 640/3000 gates
(`graph_scene.gd::_recompute_instance_budgets`). Initial-load quality is the
`initialNodeLimit` settings dial (`PUT /api/settings/physics`; default 3000, ceiling
100 000), and the WS receive cap is 256 MiB so a full graph loads intact.

**Layout is full-3D by default.** `axisCompressionZ` removed; dual-disc flatten is
opt-in (`enableDualDiscLayout` default OFF). The sanitiser hard-rejects out-of-bounds
records instead of clamping (no more edge-fan-on-clamp).

**Controls (VR).**
- **Wand-ray pointer** — the first working VR button input; a laser from the wand
  drives the world-space HUD buttons (`RAY_IDLE_COLOR` cyan when tracking).
- **Operator control panel** on the HUD: Reset Layout, Spread ±, Edges ±, NodeSize ±
  (`hud.gd` `ControlsGrid`), live status line. A **Key** tab holds the colour
  swatch legend (community hue, anomaly, query marks, agent status, edge tints,
  wand/panel states). Server-routed buttons (View 3D/Flat, Hierarchy, Radial,
  Layout Mode, Reset) need the backend armed with `VISIONCLAW_DEV_MODE=1` (the
  dev compose default since ADR-2108) or a power-user `XR_NOSTR_SECRET`; a
  rejected write now flashes its reason + remedy in the HUD bottom strip.
- **Grab / drag** — ray/sphere pick, keep-distance; server-authoritative (NIP-98
  signed, needs `XR_NOSTR_SECRET`).
- **Double-click a node** → narrativegoldmine page card in the HUD document panel
  (`hud.gd`, `NG_PAGE_BASE`; two grabs of the same node within `DOUBLE_CLICK_SEC`).
- **Proximity node labels** — pooled `Label3D`, 4 Hz, distance-faded, shown only for
  the nodes nearest the camera (`LABEL_*` constants; `nodes_near` query).

**XR-safe eye candy** (no post-process — Compat/multiview constraint): fresnel-halo
`next_pass` (`materials/node_halo.gdshader`), TIME-driven additive edge-flow shader
(`materials/edge_flow.gdshader`), centrality-driven node size tells.

**Still WIP:** copresence canaries unfired; Quest APK frozen; per-frame edge-endpoint
tracking of moving nodes and adaptive-fit-during-grab are follow-ups; glow parity
still sacrificed on Compat.

## Immersive interaction — Graph2VR-class controls (2026-08-30)

Wave-1 (commit `92b2d9588`), the pinch-manipulation commit (`f7113226a`), the GPU
pin/DAG/force-channel commit (`82abb6776`, [ADR-138](../docs/archive/adr/ADR-138-gpu-force-channel-registry.md)),
and the flagship working tree together add a full linked-data interaction
vocabulary. Provenance and licence governance for the mined features:
[ADR-139](../docs/archive/adr/ADR-139-immersive-interaction-adoption-programme.md).

**Controls (VR).**
- **A / X face button → node radial menu.** Opens `RadialMenu`
  (`scripts/radial_menu.gd`, `scenes/RadialMenu.tscn`) at the targeted node's
  render position — arc-wedge items with an overflow slider when there are more
  than a windowful, latch-safe close. Items are context-dependent (expand by
  predicate, mark as variable, use as anchor, join, clear). Grip stays HUD-grab;
  trigger stays grab/click — the face button was the free input.
- **Both-grips pinch manipulation.** Holding **both grips** scales / rotates /
  translates the whole graph about its bounding-sphere centroid
  (`graph_scene.gd::_update_two_hand_manip`) — a Graph2VR-style two-hand transform.
  A reference-frame reset avoids rotational drift. (This is why the fold-ladder
  gesture is *not* both-grips — see the fold PRD §5.)
- **Drag pins a node; Unpin All clears them.** A wand grab-drag now pins the node
  **server-authoritatively** on release (`fx/fy/fz` held in the GPU integrator;
  the node still exerts forces). Drag-timeout and disconnect send an explicit
  `nodeUnpin` so no stale anchors linger. The HUD **Unpin All** button clears every
  pin in the view. Pin bookkeeping is gated on the server's `nodeUnpinAck`.
- **HUD Hierarchy / Shells± / 3D-Flat buttons** (`hud.gd` `ControlsGrid2`,
  staged commit-on-2xx, NIP-98 auth with dev fallback):
  - **Hierarchy** toggles the DAG radial rank-bias layout (`dagBiasK` / `dagLevelDistance`).
  - **Shells ±** steps the layout's shell separation.
  - **3D-Flat** toggles the dual-disc flatten (`enableDualDiscLayout`); `1.0 = fully 3D`.
- **Search** — `search_labels` (cached-lowercase, bounded top-k) locates nodes by
  label from the HUD.
- **Variable marking `?vN`** (flagship query builder, `scripts/query_builder.gd`):
  the radial *Mark as variable* action recolours a node and badges it `?v1`,
  `?v2`, … (cyan/magenta query palette, shader rim glow). The marked pattern is a
  graph query executed server-side; see the visual-query-builder PRD.
- **Fold ladder `[Fold +]/[Fold −]`** (in build) — steps the density ladder
  ∅→L1→L2→L3; see the fold-ladder PRD.

> **Drag routing fix (shipped in `92b2d9588`).** Drag/pin was silently dead:
> the client emitted **snake_case** drag messages while the server routes
> **camelCase**. The client now emits `nodeDragStart` / `nodeDragUpdate` /
> `nodeDragEnd` (with a routing-literal test), so server-authoritative drag/pin
> actually works.

**Pinning semantics.** A pin is an operator assertion that survives layout
resettles: the integrator early-outs the pinned node's *position* update but keeps
it in the force calculation, so its neighbours still respond to it. Pins are set on
drag-end, released on explicit `nodeUnpin` (button, drag-timeout, or disconnect),
and are **per-view** on the wire but server-authoritative in effect. The fold
ladder honours pins — a pinned node is never folded away; it is promoted to its
group's representative.

**New backend endpoints the client speaks** (all under `/api/graph`, camelCase,
`NODE_ID_MASK` on wire ids, `RateLimit::per_minute(120)` on the POSTs):

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/graph/node/{id}/relations` | GET | Predicate counts for a node — populates the expand radial before fan-out |
| `/api/graph/node/{id}/expand` | POST | Bounded top-k neighbourhood expansion along a chosen predicate/direction |
| `/api/graph/fold?level=<0..3>` | GET | Fold plan (hidden ids + collapse groups) for the density ladder; optional `graph_type` / `pinned` |
| `/api/graph/query/pattern` | POST | Visual-query pattern match over the live typed graph; `countOnly` for the live preview |

## Cross-references

- **Mining provenance & governance**: [ADR-139](../docs/archive/adr/ADR-139-immersive-interaction-adoption-programme.md)
- **Fold ladder**: [PRD-fold-ladder-hierarchical-density](../docs/archive/prd/PRD-fold-ladder-hierarchical-density.md)
- **Visual query builder**: [prd-visual-query-builder-semantic-planes](../docs/archive/prd/prd-visual-query-builder-semantic-planes.md)
- **Wire format**: [`crates/visionclaw-xr-presence/src/wire.rs`](../crates/visionclaw-xr-presence/src/wire.rs) — opcode 0x43 single source of truth
- **Bounded context**: [`docs/ddd-xr-godot-context.md`](../docs/archive/ddd/ddd-xr-godot-context.md) BC22
- **Threat model**: [`docs/XR-client.md`](../docs/XR-client.md) (governing doc; the retired threat-model note is on the `archive/visionclaw-docs/` shelf)
- **Architecture**: [`docs/explanation/xr-architecture.md`](../docs/explanation/xr-architecture.md)
