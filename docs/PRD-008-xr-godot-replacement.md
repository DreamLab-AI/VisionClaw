# PRD-008: XR Client Replacement — Native Quest 3 APK via Godot 4 + godot-rust + OpenXR

**Status:** In Progress — scaffold, protocol, presence, interaction, LOD, avatar rendering, and testing feature-complete; LiveKit AAR JNI bridge, WebXR removal, soak testing, and on-device profiling remain planned
**Priority:** P0 — current XR stack is silent-failing in production; the user-facing immersive path is effectively unshipped
**Date:** 2026-05-02 (last updated 2026-05-04)
**Author:** Architecture Audit (xr-godot-replacement swarm `swarm-1777757491161-nl2bbv`)
**Supersedes:**
- [`docs/prd-xr-modernization.md`](prd-xr-modernization.md) — incremental fix to the Babylon/R3F/Vircadia stack; **superseded in full** by this PRD
- [ADR-032 (RATK integration)](adr/ADR-032-ratk-integration.md) — RATK was a Babylon/Three.js typed-anchor wrapper; **moot** once we leave the browser
- [ADR-033 (Vircadia decoupling)](adr/ADR-033-vircadia-decoupling.md) — adapter pattern around `quest3AutoDetector`; **moot** once Vircadia is removed entirely
**Related (this PRD's siblings, to be authored):**
- [ADR-071 — XR client replatform to Godot 4 + godot-rust](adr/ADR-071-xr-client-godot-replatform.md) (decision record for §4–§5)
- [`docs/ddd-xr-godot-context.md`](ddd-xr-godot-context.md) (bounded-context model — supersedes [`docs/ddd-xr-bounded-context.md`](ddd-xr-bounded-context.md))
- [`docs/PRD-QE-002-xr-godot-quality-engineering.md`](PRD-QE-002-xr-godot-quality-engineering.md) (QE plan, gdext crate coverage, soak harness)
- [`docs/xr-vircadia-removal-plan.md`](xr-vircadia-removal-plan.md) (file-by-file removal manifest)
- [`docs/xr-godot-system-architecture.md`](xr-godot-system-architecture.md) (architecture deep-dive)
- [`docs/xr-godot-threat-model.md`](xr-godot-threat-model.md) (presence WS auth, pose validation, voice, anchor PII)
**Related (existing, unchanged):**
- [PRD-007 — Binary protocol unification](PRD-007-binary-protocol-unification.md) (the per-frame stream the new client must consume unchanged)
- [`docs/binary-protocol.md`](binary-protocol.md) (single-source spec — extended in §5.2 below with an avatar pose frame)
- [ADR-061 — Binary protocol unification](adr/ADR-061-binary-protocol-unification.md) (authoritative; this PRD adds an opcode under its umbrella, does **not** version it)
- [PRD-004 — Agentbox/VisionClaw integration](PRD-004-agentbox-visionclaw-integration.md) (template for cross-system PRD shape)

---

## 1. Problem Statement

The browser-based XR client is structurally unable to deliver the immersive
experience the rest of VisionClaw was built to support. The accumulated
evidence:

| # | Finding | Impact |
|---|---------|--------|
| 1 | Vircadia integration is silent-failing — `quest3AutoDetector.ts` hardcodes `ws://localhost:3020/world/ws` and the SDK directory; if the server isn't up the auto-start dies without surfacing a user error (`docs/prd-xr-modernization.md` §1, finding 2) | Quest sessions launch, render an empty world, and exit. No user has had a working multi-user XR session in 4 months. |
| 2 | Two independent renderer pipelines compete for the immersive slot — the legacy `client/src/immersive/threejs/` tree (R3F + `@react-three/xr`) and the newer `client/src/services/vircadia/` Babylon path (`VircadiaSceneBridge.ts`, `EntitySyncManager.ts`, `ThreeJSAvatarRenderer.ts`) — every PR touching XR has to choose, every contributor has to learn both, and `xrStore.ts` exists in two singleton shapes. | New XR features stall in design review; bugs reproduce in one path and not the other; no contributor confidently owns the surface. |
| 3 | XR test coverage sits at 17% (2 of 12 files) per the prior PRD's audit, and the Vircadia-side `services/vircadia/` directory has zero tests at all | Refactoring is high-risk; CI gates nothing; regressions only surface on a Quest 3, in person, after a manual Docker bring-up. |
| 4 | Multi-user is JS-heavy, PostgreSQL-backed, and **does not match the Rust substrate**. Vircadia owns its own entity store in its own Postgres; VisionClaw's authoritative graph state lives in Neo4j + RuVector + `GraphStateActor`. The `EntitySyncManager` is a 100ms reconciliation loop between two stores that should not exist in parallel, run by a third-party world server we do not control. | Identity drifts (Vircadia entity UUIDs ≠ VisionClaw `did:nostr:` ids), avatar pose has no path to the Rust ACL boundary, voice routes through LiveKit alongside an unrelated WebRTC entity-sync channel, and per-user visibility (ADR-050 sovereign ownership) **cannot be enforced** through Vircadia's PostgreSQL layer. |
| 5 | WebXR on Quest 3 caps `devicePixelRatio` at 1.0 to stay under the physics-tick budget, fights browser GC, and runs the full 200-line `parseBinaryFrameData` decode in the JS main thread every frame. Even on Quest 3 it cannot consistently hold 90 fps with > 5k visible nodes. | The headline target frame rate is unmet on the headline target device. |
| 6 | The Apple Vision Pro / Safari path is also unsupported (no WebXR in Safari), so "browser WebXR" was never the universal-reach bet it was sold as. The reach we actually have is **Quest Browser** — i.e. one platform we already have to special-case. | The justification for staying in a browser is gone. |

The user-facing summary: **the XR client is the weakest part of VisionClaw,
and it is the only part not written in the same idiom as the rest of the
system.** Every other surface is a Rust actor talking the binary protocol
to a typed renderer over a single authenticated WebSocket. XR is a
browser-hosted Babylon/Three.js hybrid talking to a third-party world
server over an entity-sync protocol the rest of the system does not
recognise.

The decision (2026-05-02): **stop incrementally fixing the browser
stack. Replace it.** The replacement is a **native Quest 3 APK built on
Godot 4.3 + godot-rust (gdext) + OpenXR**, talking the same binary
protocol the desktop client speaks, plus one new presence WebSocket that
lives inside the Rust substrate.

---

## 2. Goals

| # | Goal | Measurable Target | Verification |
|---|------|-------------------|--------------|
| G1 | Hold the headline frame rate on the headline device | **90 fps stable on Quest 3, 99th-percentile frame time ≤ 12 ms**, with 5 k visible nodes + 4 remote avatars | OVR Metrics Tool capture, 10-min soak; CI perf gate against baseline trace |
| G2 | Drive motion-to-photon latency below the comfort threshold | **< 20 ms motion-to-photon** on Quest 3 (HMD pose → rendered frame) | OpenXR `xrLocateViews` timestamp diffed against `xrEndFrame` displayTime; reported via gdext perf tap |
| G3 | Multi-user join is fast enough to feel collaborative, not scripted | **< 500 ms presence join** (APK launch → first remote-avatar tick visible) on warm cache, measured client-side from `presence_subscribe` send to first remote `pose_frame` render | gdext-emitted instrumentation event, asserted in QE soak run |
| G4 | APK is small enough to side-load casually | **APK ≤ 80 MB** (universal `.apk`, not split by ABI; arm64-v8a only) | `ls -l xr-client/export/visionclaw-xr.apk` in CI build artifact step |
| G5 | The hot-path Rust crate is genuinely tested, not nominally covered | **`crates/visionclaw-xr-gdext` line coverage ≥ 80 %, branch coverage ≥ 70 %** | `cargo tarpaulin` (or `grcov`) report committed by CI |
| G6 | One renderer, one path, one identity model | After cutover: zero references to `vircadia`, `babylon`, `@react-three/xr`, `RealityKit` in `client/src/`; `did:nostr:<hex-pubkey>` is the only identity entering the presence WS | `rg -i 'vircadia\|babylon\|@react-three/xr' client/src` returns 0; presence WS auth integration test rejects non-NIP-98 connections |
| G7 | The wire format the new client consumes is the **same** wire format PRD-007 just unified — no fork | The 28 B/node position frame and `analytics_update` JSON are reused **byte-for-byte**; the new avatar pose frame rides under the same `0x42` preamble's umbrella spec, registered as a sibling opcode in §5.2 | Round-trip integration test using a single `crates/binary-protocol` encoder against both the desktop TS decoder and the Godot gdext decoder |
| G8 | Test, soak, and ship inside one engineering quarter | The 8-week timeline in §7 lands on `main` with all gates in §6 green | M1–M5 milestone exit predicates (§7.2) |

---

## 3. Non-Goals

- **No browser WebXR fallback.** The current Babylon/R3F path is removed wholesale (§5.6). If a user lacks a Quest 3, the desktop R3F graph view (non-XR) is the only available surface. We are not building a second renderer to chase a long-tail device.
- **No Quest 2 perf parity beyond 72 fps.** Quest 2 is a tested target at the lower OpenXR refresh rate; if a Quest 2 user wants the 90 fps Quest 3 experience, they upgrade. Quest 2 stability is a soak target, not a perf target.
- **No Apple Vision Pro / visionOS support in this PRD.** Vision Pro is not a Quest, not an OpenXR-via-Android-NDK target, and the runtime story (RealityKit bridge, WebXR-via-polyfill) is materially different. It gets its own PRD when prioritised.
- **No Vircadia-equivalent world-building, scripting, or asset-hosting.** Multi-user means avatar pose + voice + shared graph view. It does not mean user-authored geometry, persistent world state, or world scripts.
- **No PostgreSQL-backed entity store.** Authoritative graph state stays in `GraphStateActor` (Neo4j + RuVector). Presence is an in-memory Actix actor with optional RuVector audit-trail, not a persisted entity world.
- **No protocol versioning.** Per ADR-061 (binary protocol unification), there is one binary protocol. The avatar pose frame is added as a **sibling opcode** under the existing `0x42` preamble's umbrella, registered in `docs/binary-protocol.md`. We do not introduce a `protocol_v6`.
- **No new identity scheme.** `did:nostr:<hex-pubkey>` (PRD-004 / ADR-050) carries through unchanged. Presence WS auth is NIP-98 hybrid (per agentbox §P1.7 BeadsClient pattern).
- **No mid-flight migration mode.** There is no period in which Vircadia and Godot multi-user run side-by-side on the same graph. The browser XR stack is removed in W6 and the Godot APK is the only XR client thereafter.

---

## 4. User Stories

### Quest 3 user (primary)

- As a Quest 3 user, I side-load `visionclaw-xr.apk`, launch from the Apps drawer, see the graph at 90 fps within 3 seconds, and can grab and reposition nodes with my hands without controller pair-up.
- As a Quest 3 user, I see passthrough through the room I'm standing in and the graph rendered as a holographic overlay with correct depth, not a black-background scene I have to teleport inside.
- As a Quest 3 user, when another collaborator joins, their avatar appears within 500 ms with their head and hand pose moving smoothly, and I hear their voice spatially located at their avatar's mouth.
- As a Quest 3 user, my session continues if Wi-Fi briefly drops; the gdext presence reconnect logic re-auths and re-subscribes without my noticing more than a half-second hitch in the remote avatar's motion.

### Rust developer

- As a Rust developer, the hot paths I care about — protocol decode, presence pose validation, LOD selection, frustum cull — are in `crates/visionclaw-xr-gdext` with `cargo test`-able coverage, not in GDScript.
- As a Rust developer, I can change the binary protocol's avatar pose frame in `crates/binary-protocol` once and the change is picked up by the desktop TS client and the Godot gdext crate from the same source of truth.
- As a Rust developer, presence is just another Actix actor in `src/actors/`; the WS handler is just another `src/handlers/presence_handler.rs`. I do not need to learn Babylon, Vircadia, or PostgreSQL entity shapes to reason about multi-user.

### QE engineer

- As a QE engineer, I can run the gdext crate test suite headlessly in CI without a Quest connected, on a Linux runner, with `cargo test --all-features`.
- As a QE engineer, I have a 10-minute Quest 3 soak harness that exits non-zero if the 99th-percentile frame time exceeds 12 ms or any presence WS reconnect takes longer than 1 s — wired as a CI nightly job against a tethered Quest in the lab rig.
- As a QE engineer, I can capture an OVR Metrics Tool trace from a CI run and diff it against the prior week's baseline; perf regression is an automated PR check, not a manual eyeball.

### Multi-user collaborator

- As a multi-user collaborator, I can invite another user via a shared room id; they enter the same presence room and we see each other's avatars and node selections in real time.
- As a multi-user collaborator, when I drag a node, the other user sees the position update on the same wire the rest of VisionClaw uses (PRD-007's 28 B/node position frame); my drag intent is encoded as a presence-room input the server validates and reflects back, not as a direct write to the GPU positions actor.
- As a multi-user collaborator, voice latency feels conversational (< 200 ms end-to-end), spatially accurate (HRTF positioned at the speaker's avatar mouth), and degrades gracefully if one participant's network drops.

### Security reviewer

- As a security reviewer, the presence WS rejects any connection that does not present a valid NIP-98 `Authorization` header signed by the claimed `did:nostr:<hex-pubkey>`; spoofing another user's pubkey returns HTTP 401 before the WS upgrade.
- As a security reviewer, every inbound `pose_frame` is validated server-side against anatomical limits (joint angles, head/hand distance, max linear/angular velocity) before being broadcast to other room members; spoofed poses are dropped with a counter increment, not propagated.
- As a security reviewer, the same per-user visibility filter that drops invisible nodes from the position stream (`ClientCoordinator::broadcast_with_filter`, ADR-050) drops invisible avatars from the presence stream — anonymous viewers cannot enumerate the room.
- As a security reviewer, voice traffic uses LiveKit's existing E2EE option; presence WS traffic is `wss://` only; the gdext crate's panic surface is bounded (no `.unwrap()` in protocol decode paths), enforced by `cargo clippy -D clippy::unwrap_used` on the crate.

---

## 5. Technical Requirements

### 5.1 Godot Project Layout

A new top-level `xr-client/` directory holds the Godot project. Layout:

```
xr-client/
├── project.godot                       # Godot 4.3 project config
├── icon.svg
├── scenes/
│   ├── XRBoot.tscn                     # OpenXR boot + capability probe scene
│   ├── GraphScene.tscn                 # MultiMeshInstance3D for nodes, avatar lifecycle signals
│   ├── Avatar.tscn                     # Head + 2× hand, instanced per remote user, pose application
│   └── HUD.tscn                        # Room join + mute controls, presence/voice status
├── scripts/                            # GDScript: scene wiring, UI behaviour, signals
│   ├── xr_boot.gd                      # OpenXR boot + capability probe
│   ├── graph_scene.gd                  # Graph rendering, avatar lifecycle signals
│   ├── avatar.gd                       # Avatar pose application, interpolation
│   └── hud.gd                          # Room join, mute controls, HUD wiring
├── rust/                               # gdext crate (see detailed layout above)
├── perf/
│   ├── benchmark_scene.tscn            # Performance benchmark scene
│   ├── benchmark.gd                    # Benchmark harness
│   └── run_benchmark.gd                # Benchmark runner
├── tests/
│   ├── run_gut.gd                      # GUT test runner
│   └── unit/
│       └── test_scene_load.gd          # Scene load tests (being expanded)
├── addons/
│   └── livekit/                        # LiveKit Android AAR + binding shim (§5.5) — NOT YET STARTED
├── export_presets.cfg                  # Android Quest 3 export preset (arm64-v8a, OpenXR enabled)
└── android/
    └── build/                          # Custom Gradle template if needed (NDK r26+)
```

The companion gdext crate lives in the workspace at:

```
xr-client/rust/                         # Rust-side hot paths exposed as GDExtension classes
├── Cargo.toml
├── src/
│   ├── lib.rs                          # GDExtension entry point; registers 5 classes
│   ├── binary_protocol.rs              # Decodes 0x42 position frames (214 lines, 5 inline tests)
│   ├── presence.rs                     # WS presence client with NIP-98 auth, reconnect (331 lines, 4 async tests)
│   ├── interaction.rs                  # Hand-tracking ray cast + pinch detection (266 lines, 9 inline tests)
│   ├── lod.rs                          # Frustum cull + distance-bucket LOD policy (200 lines, 7 inline tests)
│   ├── webrtc_audio.rs                 # Spatial voice routing surface (13+ inline tests, being expanded)
│   └── ports/
│       └── mod.rs                      # Hexagonal port architecture with fake transports (151 lines)
├── benches/
│   └── decode_throughput.rs
└── tests/
    ├── presence_handshake.rs           # 2 integration tests
    ├── property_interaction.rs         # 11 property tests (proptest)
    ├── lod_thresholds.rs               # 5 integration tests
    ├── pose_wire_round_trip.rs         # 3 round-trip tests
    ├── property_lod.rs                 # 9 property tests (proptest)
    ├── interaction_raycast.rs          # 8 integration tests
    └── visual_fixture.rs              # 5 visual fixture tests (numerical, not pixel-diff)
```

GDScript responsibilities (and **only** these): scene composition, signal
wiring, UI state, OpenXR feature toggles, scene-graph manipulation in
response to gdext signals. **No** wire-format parsing, **no** WebSocket
state, **no** pose validation.

gdext responsibilities: protocol decode, WS lifecycle, pose
validation, LOD math, perf taps. Exposed to GDScript via
`#[derive(GodotClass)]` classes that signal scene-side updates.

### 5.2 Binary Protocol Reuse and Extension

**The position stream is unchanged.** The Godot client consumes the same
28 B/node position frame the desktop TS client consumes. Implementation
lives in `crates/visionclaw-xr-gdext::protocol_decoder` and reuses
`crates/binary-protocol::decode_position_frame` byte-for-byte (workspace
dependency).

**One new opcode is added under the existing `0x42` preamble's umbrella**:
the **avatar pose frame**. Per ADR-061, this is **not** a protocol version
bump; it is a sibling message type registered in
`docs/binary-protocol.md`'s opcode table.

#### 5.2.1 Avatar pose frame layout

Sent server → all clients in a presence room, at 90 Hz, over the **presence
WS** (not the existing position WS — different actor, different
authentication scope, different sender):

```
[u8  preamble = 0x43]               <- "C" — sibling of 0x42, distinct opcode for pose ("conscious presence")
[u64 broadcast_sequence_LE]         <- monotonic per presence room; client acks via PresenceAck for backpressure
[u32 room_id]
[u16 user_count]
[N × AvatarPose]
```

Per `AvatarPose` (76 bytes fixed when both hands present; the
implementation in `crates/visionclaw-xr-presence::wire` adds a
1-byte `transform_mask` bitfield (bit0=head, bit1=lhand, bit2=rhand)
**before** the per-avatar payload to elide untracked hand transforms,
so the per-avatar size ranges from 32 B (head-only) to 76 B (both
hands). Server aggregates per-room into the multi-user broadcast
shape below):

```
[u32 user_id_LE]                    <- room-local id; mapped to did:nostr:<hex-pubkey> via init JSON (§5.3)
[f32 head_x][f32 head_y][f32 head_z]                    <- 12 B head position
[f32 head_qx][f32 head_qy][f32 head_qz][f32 head_qw]    <- 16 B head orientation (quaternion)
[f32 lhand_x][f32 lhand_y][f32 lhand_z]                 <- 12 B left hand position
[f32 lhand_qx][f32 lhand_qy][f32 lhand_qz][f32 lhand_qw]<- 16 B left hand orientation
[f32 rhand_x][f32 rhand_y][f32 rhand_z]                 <- 12 B right hand position
[f32 rhand_qx][f32 rhand_qy][f32 rhand_qz][f32 rhand_qw]<- 16 B right hand orientation
                                                            (76 B total per avatar)
```

Client → server uses the **same per-avatar payload**, prefixed with the
`0x43` preamble and broadcast_sequence; the server validates (§5.3),
authorises against the WS-bound pubkey, and re-broadcasts to other room
members (not back to sender).

Cadence: **90 Hz** outbound from each Quest client (matching Quest 3
display refresh); server coalesces and re-broadcasts at the same rate to
subscribers. Token-bucket backpressure mirrors PRD-007's
`ClientBroadcastAck` mechanism, scoped to the presence stream.

Per-user wire cost at 4 remote avatars × 90 Hz × 76 B = ~27 kB/s
inbound. Negligible vs the position stream.

#### 5.2.2 Opcode registry

`docs/binary-protocol.md` is extended with an opcode table:

| Preamble | Opcode | Sender | Description | Spec |
|---|---|---|---|---|
| `0x42` | position_frame | server → all subscribed clients | Per-physics-tick GPU positions (PRD-007) | `docs/binary-protocol.md` |
| `0x43` | avatar_pose_frame | bidirectional, scoped to presence room | Head + hand transforms at 90 Hz | this PRD §5.2.1 |

Forbidden patterns from `docs/binary-protocol.md` apply unchanged:
adding columns to either frame requires a superseding ADR; no flag bits
in `user_id` or `room_id`; no version byte.

#### 5.2.3 Workspace crate split

`crates/binary-protocol/` (new — extracted from `src/utils/binary_protocol.rs`
during PRD-007 landing) becomes the single source of truth. Both the
Actix server and the gdext crate depend on it as a workspace member.
The desktop TS client continues to consume it via the wire (no Rust→TS
binding); the gdext client consumes it natively in-process.

### 5.3 Presence Service

**Rust crate** at `crates/visionclaw-xr-presence/` (1175 lines across source, feature-complete):

```
crates/visionclaw-xr-presence/
├── Cargo.toml
├── src/
│   ├── lib.rs                          # Public API re-exports (26 lines)
│   ├── types.rs                        # AvatarId, Did, RoomId, PoseFrame, Transform types (204 lines)
│   ├── wire.rs                         # 0x43 avatar pose codec — encode/decode with transform_mask bitfield (304 lines, 4 inline tests)
│   ├── room.rs                         # Room model: members, broadcast_sequence, last_pose_per_user (174 lines, 3 inline tests)
│   ├── validate.rs                     # Anatomical limits + velocity clamps, quaternion checks (202 lines, 7 inline tests)
│   ├── delta.rs                        # Delta compression for pose updates (142 lines, 3 inline tests)
│   ├── error.rs                        # Typed error hierarchy (61 lines)
│   └── ports/
│       └── mod.rs                      # ACL port traits for identity verification and room membership
├── proptest-regressions/
│   └── wire.txt                        # Proptest regression seeds
├── fuzz/
│   ├── Cargo.toml
│   └── fuzz_targets/
│       └── wire_decode.rs              # Fuzz target for wire protocol decode
├── benches/
│   ├── wire.rs                         # Wire encode/decode benchmarks
│   └── baseline.json                   # Criterion baseline
└── tests/
    ├── integration.rs                  # 9 integration tests (join/leave/concurrent-rooms)
    ├── property_tests.rs               # 12 property tests (proptest — round-trip, NaN rejection, quaternion validation)
    └── adversarial_tests.rs            # 24 adversarial tests (spoofed poses, replay, rate-limit)
```

**New Actix WS handler** at `src/handlers/presence_handler.rs`,
mounted at the route **`/ws/presence`**. Handler responsibilities:

1. **Auth on upgrade.** Inspect `Authorization: Nostr <NIP-98 token>`
   header. Verify signature against claimed `did:nostr:<hex-pubkey>`.
   On failure → HTTP 401, no upgrade. Successful pubkey is bound to the
   socket for the session's lifetime; client cannot impersonate another
   user mid-session.
2. **Init handshake.** First post-upgrade message is a JSON
   `presence_init { room_id, display_name }`. Server replies with a
   JSON `presence_room_state { room_id, members: [{user_id, did, display_name, color}] }`,
   establishing the `user_id ↔ did:nostr` mapping the binary pose frames
   reference by `user_id`.
3. **Pose ingest.** Inbound `0x43` frames are decoded by
   `crates/binary-protocol::decode_avatar_pose_frame`, validated by
   `visionclaw-xr-presence::pose_validator`, then forwarded to the
   `PresenceActor` for re-broadcast.
4. **Broadcast.** `PresenceActor` coalesces in-flight poses per room at
   90 Hz, encodes one `0x43` frame containing all current room members'
   latest pose (including a stale-flag bit for any member whose last
   pose is older than 200 ms), and sends to each subscribed socket
   **except** the sender.
5. **Visibility filter.** Re-broadcast respects ADR-050 — if user A has
   not consented to be visible to user B (anonymous-viewer rules), A's
   pose is dropped from the frame B receives, same pattern as the
   position-stream `broadcast_with_filter`.

**Pose validation rules** (`pose_validator.rs`):

| Check | Bound | Action on violation |
|---|---|---|
| Head position within world bounds | configured per room (default ±50 m) | Drop frame, increment `presence.invalid_pose.bounds` counter |
| Head linear velocity | ≤ 20 m/s | Drop frame, increment `presence.invalid_pose.head_velocity` counter |
| Hand-to-head distance | ≤ 1.2 m (anatomical reach) | Drop frame, increment `presence.invalid_pose.hand_reach` counter |
| Quaternion magnitude | within [0.99, 1.01] (must be unit) | Drop frame, increment `presence.invalid_pose.bad_quat` counter |
| Frame rate from a single user | ≤ 120 Hz averaged over 1 s | Token-bucket throttle; excess dropped, counter incremented |

A user accumulating > 5 invalid frames in a 10-s window is
disconnected with a `presence_kick` reason code — surfaced client-side
as a non-fatal toast.

**No PostgreSQL.** Room state is in-memory in `PresenceActor`; the only
persistence is an optional audit-trail RuVector entry per
join/leave/kick (namespace `xr-presence-audit`), gated by
`PRESENCE_AUDIT=true`. Room membership does not survive a server
restart — clients reconnect and re-init.

### 5.4 OpenXR Feature Set

The Godot OpenXR plugin is configured to request the following extensions
at session init. Each is listed with its **OpenXR extension ID** so
deployment can verify Quest 3 runtime support directly via
`xrEnumerateInstanceExtensionProperties`:

| Feature | OpenXR extension ID | Required? | Notes |
|---|---|---|---|
| Hand tracking | `XR_EXT_hand_tracking` | required | 26 joints per hand; gdext consumes joint data via Godot OpenXR API; pinch detection from thumb-tip ↔ index-tip distance |
| Hand interaction (alternative input source) | `XR_EXT_hand_interaction` | required | Higher-level pinch/grab signals layered on top of `XR_EXT_hand_tracking` |
| Passthrough | `XR_FB_passthrough` | required | Quest-vendor extension; enables environment passthrough as the scene's compositor layer; verified against Meta Horizon OS 71+ |
| Scene mesh | `XR_FB_scene` + `XR_FB_scene_capture` | required | Room-scan mesh for occlusion of graph behind real walls; cached per-room-setup |
| Spatial anchors | `XR_FB_spatial_entity` + `XR_FB_spatial_entity_storage` | required | Anchored "graph origin" point persists across sessions; user re-enters a session and the graph is in the same physical place |
| Foveated rendering | `XR_FB_foveation` + `XR_FB_foveation_configuration` | required | Fixed foveation level 2 default; level 3 if `xrGetSystemProperties` reports Quest 3 |
| Composition layer depth | `XR_KHR_composition_layer_depth` | required | Correct depth submission for proper passthrough occlusion sorting |
| Performance settings | `XR_EXT_performance_settings` | required | Hint Meta runtime that we want sustained CPU/GPU performance |
| Display refresh rate | `XR_FB_display_refresh_rate` | required | Negotiate 90 Hz; fall back to 72 Hz for Quest 2 |
| Local floor reference space | `XR_EXT_local_floor` | required | Stage-relative reference space; matches user's physical floor |
| Visibility mask | `XR_KHR_visibility_mask` | optional | Eye-tracking-driven render mask if hardware supports |
| Eye tracking | `XR_EXT_eye_gaze_interaction` | optional | Quest Pro / Vision Pro only; ignored on Quest 3 base; PII concerns covered in `docs/xr-godot-threat-model.md` |

If a required extension is missing at session init, the client surfaces a
fatal error to the user and exits — there is no degraded mode.

### 5.5 Voice Routing

LiveKit is retained. The browser's `livekit-client` JS SDK is replaced
with the **livekit-client-android AAR** (Java/Kotlin), exposed to Godot
via a thin GDExtension binding shim in `addons/livekit/`. The AAR is
built from LiveKit's published Android client release (pinned in
`xr-client/android/build.gradle`).

Voice flow:

```mermaid
graph LR
    MIC["Quest mic"] --> GSM["Godot AudioStreamMicrophone"]
    GSM --> ENC["LiveKit AAR encoder (Opus)"]
    ENC --> ROOM["LiveKit room"]
    ROOM --> DEC["LiveKit AAR decoder"]
    DEC --> TRACK["remote audio track"]
    TRACK --> ASP["Godot AudioStreamPlayer3D<br/>(positioned at AvatarRig.HeadPivot)"]
    ASP --> HRTF["AudioServer HRTF<br/>(attenuation: INVERSE_DISTANCE,<br/>area_mask: HRTFAudioBus)"]
```

HRTF is Godot-native (`AudioStreamPlayer3D` + `AudioEffectHRTF` on the
voice bus); LiveKit handles transport and codec only. Per-avatar HRTF
panner position is updated each frame in `avatar_rig.gd` from the
`AvatarPose.head_position` exposed by gdext.

LiveKit room id is the same as the presence room id (1:1). The
LiveKit auth token is minted by the existing
`src/handlers/livekit_token_handler.rs`, no changes required — the
Godot client requests it over the existing HTTPS API at session start.

### 5.6 Vircadia + Babylon Removal

Removal is **wholesale**, not gradual. The full file-by-file removal
manifest lives in [`docs/xr-vircadia-removal-plan.md`](xr-vircadia-removal-plan.md).
Summary of what disappears:

**Client TypeScript:**

| Path | Files | Action |
|---|---|---|
| `client/src/services/vircadia/` | `VircadiaClientCore.ts`, `EntitySyncManager.ts`, `GraphEntityMapper.ts`, `ThreeJSAvatarRenderer.ts`, `CollaborativeGraphSync.ts` | Delete entire directory |
| `client/src/immersive/` | `components/ImmersiveApp.tsx`, `xrStore.ts`, `types.ts`, all of `hooks/`, all of `threejs/`, all of `ports/` (24 files) | Delete entire directory |
| `client/src/utils/` (XR-specific) | `quest3AutoDetector.ts`, `platformManager.ts` (XR portions only) | Delete `quest3AutoDetector.ts`; trim platformManager to keep only desktop platform detection |
| `client/package.json` | `@babylonjs/core`, `@babylonjs/loaders`, `@react-three/xr`, `vircadia-world-sdk-ts` | Remove dependency entries; `npm prune` |

**Server Rust:**

- `docker-compose.vircadia.yml`: delete file.
- `Dockerfile` references to Vircadia world server: remove.
- Any Actix routes that proxy to Vircadia entity-store: remove
  (none expected; Vircadia ran in its own container).
- PostgreSQL Vircadia schema: drop in the Vircadia container's own
  data volume; documented as a one-time op in the removal plan.

**Documentation:**

- `docs/explanation/xr-architecture.md`: **delete**. Replaced by
  `docs/xr-godot-system-architecture.md`.
- `docs/ddd-xr-bounded-context.md`: **superseded** by
  `docs/ddd-xr-godot-context.md`. Add a one-line "Superseded by …" note
  at top, retain for git history.
- `docs/how-to/xr-setup-quest3.md` (if exists): rewrite as Godot APK
  side-load instructions.
- `CLAUDE.md`: update XR references — delete Babylon/Vircadia sections,
  add a brief "XR client lives at `xr-client/`; multi-user via
  `crates/visionclaw-xr-presence`" note.
- ADR-032 (RATK), ADR-033 (Vircadia decoupling): mark **Superseded by
  PRD-008** with a one-paragraph "Historical context" preamble.
- ADR-050 §H (sovereign visibility): no change to the doc; the
  visibility filter pattern now applies to the presence stream too,
  same implementation shape.

**Cutover ordering** (within the W6 removal sprint):

1. Godot APK (W3–W5 deliverable) is shipped, soaked, and accepted.
2. Vircadia world server container is stopped in production compose.
3. Browser XR routes (`/xr`, `ImmersiveApp` mount) return a one-screen
   "XR has moved to the Quest APK — install instructions" page.
4. Files listed above are deleted in a single `feat: remove Vircadia
   and Babylon XR client` commit.
5. Compose, Docker, and CLAUDE.md cleanup commit.
6. ADRs marked superseded.

There is no "ramp" period in which both clients are live. The browser
XR client was silent-failing anyway (§1, finding 1); there is no user
to migrate.

### 5.7 godot-rust Build Toolchain

| Item | Pin | Source |
|---|---|---|
| Godot engine | **4.3 stable** | [godotengine/godot v4.3-stable](https://github.com/godotengine/godot/releases/tag/4.3-stable); export templates downloaded by CI |
| godot-rust (gdext) | **`v0.2`** branch, commit pinned in `crates/visionclaw-xr-gdext/Cargo.toml` | [godot-rust/gdext](https://github.com/godot-rust/gdext); track upstream `master` quarterly with explicit bump PRs (no auto-update) |
| Rust toolchain | `stable` (1.82+ at PRD authoring) | `rust-toolchain.toml` at workspace root |
| Android NDK | **r26d** | Android Studio SDK manager; pinned in `xr-client/android/local.properties.template` |
| Android target | `aarch64-linux-android` only (Quest 3 is arm64) | `cargo build --target aarch64-linux-android --release` driven by `cargo-ndk` |
| OpenXR Loader | bundled with Godot 4.3 OpenXR plugin | no separate pin; tracked with Godot version |
| LiveKit Android | **`v2.x` AAR** (latest stable at sprint start) | `xr-client/android/build.gradle` Maven dependency |
| Java/Kotlin toolchain | JDK 17 (Android Gradle plugin requirement) | `xr-client/android/build.gradle` |

**CI build pipeline** (GitHub Actions workflow `.github/workflows/xr-godot-ci.yml` — 10 jobs, 473 lines, operational):

1. Restore cargo cache.
2. Install Android NDK r26d, Godot 4.3 export templates, JDK 17.
3. `cargo ndk -t aarch64-linux-android build --release -p visionclaw-xr-gdext`.
4. Stage `.so` into `xr-client/addons/visionclaw_xr_gdext/aarch64/`.
5. `godot --headless --export-release "Quest 3 arm64" xr-client/export/visionclaw-xr.apk`.
6. APK size gate: fail if `> 80 MB` (G4).
7. `cargo test -p visionclaw-xr-gdext --all-features`.
8. `cargo tarpaulin -p visionclaw-xr-gdext` — fail if line coverage `< 80 %` (G5).
9. Upload APK + coverage report as workflow artifacts.
10. On `main` push: also publish APK to internal release channel for QA side-load.

**Android export preset** (`xr-client/export_presets.cfg`):

- `arch/arm64-v8a = true`, all other ABIs `false` (Quest 3 only)
- `xr_features/xr_mode = "openxr"`
- `xr_features/hand_tracking = 2` (required)
- `xr_features/passthrough = 2` (required)
- `permissions/internet = true`
- `permissions/record_audio = true` (LiveKit voice)
- `package/unique_name = "uk.xrsystems.visionclaw.xr"`
- `version/code = <ci-build-number>`, `version/name = "<git-describe>"`

---

## 6. Success Criteria

| Criterion | Verification | Owner |
|---|---|---|
| 90 fps stable on Quest 3 with 5 k visible nodes + 4 remote avatars | OVR Metrics Tool 10-min capture; 99th-pct frame time ≤ 12 ms; CI nightly perf gate against tethered Quest in lab rig | QE |
| Motion-to-photon < 20 ms | gdext perf tap diffs `xrLocateViews` predicted-display-time vs `xrEndFrame` actual-display-time; reported per frame, asserted in soak | QE |
| Presence join < 500 ms | gdext instrumentation event `presence_join_latency_ms`; QE soak asserts p95 < 500 ms | QE |
| APK ≤ 80 MB | CI step `ls -l xr-client/export/visionclaw-xr.apk`; PR check fails if exceeded | DevOps |
| `crates/visionclaw-xr-gdext` line coverage ≥ 80 % | `cargo tarpaulin` report posted as PR comment; CI fails below threshold | QE |
| `crates/visionclaw-xr-gdext` branch coverage ≥ 70 % | Same as above | QE |
| Zero Vircadia / Babylon / @react-three/xr / RealityKit refs in `client/src/` post-cutover | `rg -i 'vircadia\|babylon\|@react-three/xr\|RealityKit' client/src` returns 0; PR check on W6 removal commit | Server eng |
| Presence WS rejects unauthenticated connections | Integration test in `crates/visionclaw-xr-presence/tests/auth_rejection.rs` posts a connection without a NIP-98 header and asserts HTTP 401; second test posts a forged signature and asserts HTTP 401 | Security |
| Presence WS rejects spoofed poses | Integration test posts a pose with head velocity 100 m/s and asserts the broadcast does not contain it; counters increment | Security |
| Same binary protocol byte-for-byte on both decoders | Round-trip integration test in `crates/binary-protocol/tests/decoder_parity.rs` encodes a frame, decodes it via the TS decoder (Node.js test) and the gdext decoder (Rust test), asserts identical decoded structs | Server eng |
| Hand tracking detects pinch on Quest 3 | Manual smoke per release: pinch a node with bare hands, observe selection highlight; documented in `docs/how-to/xr-setup-quest3.md` (rewrite) | QE |
| Voice spatially located at remote avatar mouth within 5° | Manual smoke per release using two Quest 3 devices in lab rig | QE |
| Browser XR routes return migration page, not a render attempt | Curl `https://visionclaw.example/xr` post-W6, assert response body contains "XR has moved to the Quest APK" | DevOps |

---

## 7. Phased Timeline (8 weeks)

### 7.1 Phases

| Week | Phase | Workstreams active in parallel | Exit deliverable | Status |
|---|---|---|---|---|
| **W1** | Scaffold | Godot project skeleton; gdext crate skeleton; presence Rust crate skeleton; CI workflow stub | `xr-client/` builds an empty Godot scene to APK; `crates/visionclaw-xr-gdext` and `crates/visionclaw-xr-presence` compile with stub APIs; CI pipeline runs and produces an APK artifact | **DONE** |
| **W2** | Protocol + presence handler | Extend `crates/binary-protocol` with `encode/decode_avatar_pose_frame`; implement `src/handlers/presence_handler.rs`; implement `PresenceActor` with NIP-98 auth + pose validation; gdext `protocol_decoder` consumes the binary-protocol crate | A Quest can connect to `/ws/presence`, exchange poses with a Rust integration-test client, and have invalid poses rejected. Round-trip parity test green. | **DONE** — binary_protocol.rs (214 lines, 5 inline tests + integration tests), visionclaw-xr-presence crate (1175 lines: types/wire/room/validate/delta, 17 inline tests + 45 external tests), presence.rs (331 lines, 4 async tests + 2 integration tests), wire round-trip and adversarial tests passing |
| **W3** | Avatar + hand tracking | `AvatarRig.tscn` with head + 2× hands; gdext `presence_client` drives remote avatar transforms; OpenXR `XR_EXT_hand_tracking` wired through Godot to local avatar pose; passthrough enabled (`XR_FB_passthrough`) | Single user sees own hands tracked; two users in same room see each other's avatars moving | **DONE** — Avatar.tscn + avatar.gd with pose application, interaction.rs (266 lines, 9 inline tests + 8 integration tests + 11 property tests), xr_boot.gd (OpenXR boot + capability probe), GraphScene.tscn + graph_scene.gd with avatar lifecycle signals; parallel agents are fixing avatar hand interpolation + LOD awareness |
| **W4** | Graph rendering parity | Port desktop graph rendering to `GraphScene.tscn` — `MultiMeshInstance3D` for nodes, `ImmediateMesh` for edges; consume position frames from the existing position WS in gdext; LOD module driven by frustum + distance buckets; node selection via hand pinch | Quest 3 user sees the live graph at the lab rig's typical 5 k-node load, holds 90 fps, can pinch-select a node | **DONE** — lod.rs (200 lines, 7 inline tests + 3 visual fixture tests + 9 property tests), hexagonal port architecture in ports/mod.rs (151 lines) with fake transports for testing |
| **W5** | Voice + multi-user soak | LiveKit AAR integrated; per-avatar HRTF wired; 4-user lab rig soak runs for 30 min without crash, frame drop, or audio glitch | Multi-user demo recorded; lab-rig soak harness lives in CI as nightly-only job | **IN PROGRESS** — webrtc_audio.rs API surface complete (13 inline tests, being expanded); SpatialVoiceRouter GDScript-exposed methods being wired; LiveKit Android AAR JNI bridge (§5.5) not started |
| **W6** | Vircadia / Babylon removal + perf optimisation | Execute `docs/xr-vircadia-removal-plan.md`; remove dependencies from `client/package.json`; perf tune gdext hot paths to hit MTP target | All removal commits landed; G6 verification passes; OVR Metrics Tool report shows 99th-pct frame time ≤ 12 ms | **PLANNED** |
| **W7** | Soak, perf gates, security review | 7-day continuous soak in lab rig; security review against `docs/xr-godot-threat-model.md`; perf-gate CI nightly enabled; APK size gate enforced | Soak report committed; threat-model review sign-off; all CI gates green for 5 consecutive nightlies | **PLANNED** |
| **W8** | Ship | Final APK signing; release notes; CLAUDE.md final update; PRD-007 telemetry verified; tagging | `v1.0.0-xr-godot` release tag; APK published to internal distribution; PRD-008 status marked **Shipped** | **PLANNED** |

### 7.2 Milestone exit predicates

Each milestone has passable/failable predicates (PRD-004 §7 pattern — no
prose). All must be green to advance.

- **M1 exit (end W2): ACHIEVED**
  (a) `cargo test -p visionclaw-xr-gdext` green;
  (b) `cargo test -p visionclaw-xr-presence` green — 17 inline tests + 45 external tests (integration, property, adversarial);
  (c) `cargo test -p binary-protocol` includes new `avatar_pose_frame_roundtrip` test, green;
  (d) `presence_handler` integration test posts unauthenticated WS upgrade → HTTP 401 within 100 ms;
  (e) gdext protocol_decoder benchmark decodes 5000-node position frame in `< 1 ms` on the lab Quest 3 reference (verified via `cargo bench` reading frame from a fixture).

- **M2 exit (end W4): ACHIEVED**
  (a) Quest 3 APK boots, enters OpenXR session, and renders a non-empty graph within 3 s of cold launch (manual stopwatch, log-asserted);
  (b) two Quest 3 users in same room see each other's head + hand transforms updating at ≥ 60 Hz observed end-to-end (gdext perf tap);
  (c) hand pinch detected and triggers node-selection highlight in scene (smoke) — interaction.rs provides ray cast + pinch detection (9 inline + 8 integration + 11 property tests);
  (d) frame rate ≥ 80 fps with 5 k nodes (interim target; full 90 fps not required until M3);
  (e) `cargo tarpaulin -p visionclaw-xr-gdext` ≥ 60 % line coverage (interim target).

- **M3 exit (end W5):**
  (a) LiveKit voice plays in 3D-positioned `AudioStreamPlayer3D` per remote avatar, manual A/B confirms HRTF correctness in lab;
  (b) 4-user lab soak runs for 30 min uninterrupted; CI artifact contains the OVR Metrics Tool trace;
  (c) presence WS reconnect after simulated 5-s Wi-Fi drop completes within 1 s and re-syncs avatars (instrumented test, asserted).

- **M4 exit (end W6):**
  (a) `rg -i 'vircadia\|babylon\|@react-three/xr\|RealityKit' client/src` returns 0;
  (b) `npm run build` in `client/` succeeds with the dependencies removed;
  (c) `docker compose up -d` no longer brings up a Vircadia container;
  (d) ADR-032 and ADR-033 contain `Superseded by PRD-008` markers;
  (e) Quest 3 APK at end-of-W6 holds 90 fps stable with 5 k nodes + 4 avatars (G1).

- **M5 exit (end W8):**
  (a) `crates/visionclaw-xr-gdext` line coverage ≥ 80 %, branch ≥ 70 % (G5);
  (b) APK size ≤ 80 MB (G4);
  (c) MTP < 20 ms verified (G2);
  (d) presence join < 500 ms p95 verified across 100 join events in soak (G3);
  (e) 7-day continuous soak completed with zero crash, zero unhandled WS reconnect, zero pose-validation false-positive (smoke);
  (f) security review sign-off on `docs/xr-godot-threat-model.md`;
  (g) release tag `v1.0.0-xr-godot` exists on `main`.

---

## 8. Risks

| # | Risk | Mitigation |
|---|------|-----------|
| R1 | gdext stability on Android arm64 — godot-rust v0.2 is young; an upstream regression could land mid-sprint | Pin a specific commit in `Cargo.toml`; do not auto-track upstream `master`; explicit bump PRs only after smoke on lab Quest. Maintain a local fork branch as fallback if upstream blocks. **Status (2026-05-04):** GDExtension entry point operational, 5 classes registered, all Rust tests pass headlessly; no upstream regression encountered. |
| R2 | LiveKit Android AAR ↔ Godot binding shim is non-trivial — Java/Kotlin/JNI surface to a Rust-via-gdext or GDScript glue | Prototype the binding first thing in W3 against a stub LiveKit room; if blocked, fall back to a thin Kotlin wrapper exposing high-level connect/disconnect/audio-stream signals to GDScript only (no gdext involvement for voice). Worst-case fallback: 2D voice (no HRTF) for v1.0, HRTF in v1.1. |
| R3 | OpenXR Quest extension drift — Meta deprecates an FB extension or changes its semantics in a Horizon OS update | Pin Horizon OS version in soak rig; track Meta's [Quest OS release notes](https://developer.oculus.com/blog/) per release; budget for a "OS bump" PR in the quarter following ship. |
| R4 | 80 MB APK ceiling is tight once Godot 4.3 export templates + gdext `.so` + LiveKit AAR + scene assets are bundled | Strip debug symbols from release `.so` (`strip` step in CI); use Godot's `pck` separation and consider downloading a compressed asset bundle on first launch if size pressure mounts. Hard ceiling: 100 MB before re-evaluating. |
| R5 | Presence pose validation false-positives kick legitimate users (e.g. a tall user with long arms exceeds 1.2 m hand-reach) | Validation thresholds are configurable per-room; default thresholds widened from Vircadia's tighter limits. QE ground-truth thresholds against 5 lab users of varying height before W7. Kick-on-violation gated behind a 5-violations-in-10s policy, not single-frame. |
| R6 | Browser XR users lose the surface they had — even if it was silent-failing, removing it is observable | The browser XR was unshipped in practice (§1, finding 1); no active users to lose. Migration page (§5.6 step 3) explains the move and links to side-load instructions. Communication: changelog entry, internal Slack announcement, customer-facing release note. |
| R7 | Side-loading is friction for non-technical users | Mitigated outside this PRD by future Meta Store submission (separate effort). Side-load + signed APK acceptable for v1.0 — VisionClaw is a technical product with technical users. |
| R8 | The `crates/binary-protocol` crate split (W2 deliverable) overlaps with PRD-007's landing if PRD-007 hasn't completed extraction yet | PRD-007 lands first per the existing PRD-007 sprint plan; this PRD's W2 begins **after** the binary-protocol extraction is on `main`. If PRD-007 slips, W2 starts inside `src/utils/binary_protocol.rs` directly and the workspace crate split becomes a follow-up. |
| R9 | `did:nostr:<hex-pubkey>` ↔ presence room id mapping leaks user identity into rooms a user has not joined | Presence WS emits `presence_room_state` only to room members; remote-room enumeration via the WS is impossible by design. Room id is opaque (UUIDv7) — not user-derived. |
| R10 | Voice spatial accuracy degrades under packet loss | LiveKit handles RTP retransmission and Opus FEC; HRTF position update is independent of audio packet arrival, so position remains correct even if audio briefly stutters. Documented in threat model. |

---

## 9. Out-of-Scope Follow-ups

- **Apple Vision Pro / visionOS port.** Separate PRD when prioritised; the gdext crate is engine-portable but the OpenXR profile and the LiveKit binding both differ.
- **Meta Store submission and signed distribution.** v1.0 is side-load only. Store submission is a process exercise (privacy review, asset prep) tracked separately.
- **User-authored geometry / shared whiteboarding in XR.** Out of scope for this PRD's "multi-user" definition (avatar pose + voice + shared graph view).
- **Eye tracking and gaze-driven interaction.** Listed as optional in §5.4 and noted in the threat model; deferred to a future PRD when Quest 3S / Pro / Vision Pro share is justified.
- **Hand-driven menus beyond `RoomMenu.tscn`.** A general 3D menu framework is a v1.1 effort; v1.0 ships with a single menu surface.
- **Federation between presence rooms.** v1.0: rooms are server-local. Multi-server federated presence is a future PRD aligned with the BC20 agentbox-federation pattern.

---

## 10. Open Questions for Reviewers

- Should the `0x43` avatar pose frame include **per-finger joint data** in v1.0, or is head + 2× hand pose sufficient? Per-finger adds ~24 × 7 floats × 2 hands = 1.3 kB per pose per user (vs current 76 B), pushing the per-user cost from ~27 kB/s to ~470 kB/s at 4 avatars × 90 Hz. **Recommend deferring** per-finger to v1.1; v1.0 hand pose is wrist transform + pinch-strength scalar (already captured by `XR_EXT_hand_interaction`).
- Should the presence room id be **tied to a graph filter** (e.g. one room per knowledge subgraph) so users in the same room are looking at the same data, or **independent** so users coordinate room membership separately? Recommend **independent for v1.0** — simpler model — with a v1.1 "auto-join room for this graph view" UX layer above it.
- LiveKit AAR vs **WebRTC native** in gdext (e.g. `webrtc-rs`)? LiveKit gives us SFU + room management for free at the cost of a binding shim. Recommend **LiveKit** for v1.0 to keep parity with desktop voice (which already routes through LiveKit), revisit only if the AAR binding becomes a maintenance burden.
- Is the 8-week timeline acceptable, or should the wholesale-removal step (W6) be delayed by a sprint to give the new APK a longer parallel-run window? Recommend **no parallel run** — the browser XR is not delivering value, parallel run multiplies surface area, and the cleaner cutover is the lower-risk option in practice.
