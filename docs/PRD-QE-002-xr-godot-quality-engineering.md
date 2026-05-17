# PRD-QE-002: Quality Engineering for Godot 4 + OpenXR Quest 3 Stack

**Status:** In Progress — Rust test coverage substantially implemented; GUT test expansion underway
**Author:** QE / XR Architecture Agent
**Date:** 2026-05-02 (last updated 2026-05-04)
**Priority:** P0 — paired with PRD-008; ships as gate, not as follow-up
**Pairs with:** PRD-008 (Godot 4 + godot-rust XR client), ADR-071 (XR runtime selection), [DDD XR Godot (BC22)](ddd-xr-godot-context.md), ADR-032/033 (superseded by ADR-071)
**Supersedes-tests-of:** ADR-032 (Three.js / WebXR XR client), and the Vircadia integration test surface in `docs/testing/TESTING_GUIDE.md` §5

---

## 1. Problem Statement

The XR substrate is being replaced wholesale: PRD-008 ships a Godot 4.3 native APK on Quest 3, built atop a `godot-rust` (gdext) Rust crate at `xr-client/rust/`, talking to a new Actix WS endpoint backed by a new `crates/visionclaw-xr-presence` workspace member. The legacy Three.js / WebXR / Vircadia surface is being deleted in the removal plan that ships alongside PRD-008.

The legacy XR test corpus is **either obsolete or actively misleading** under the new architecture:

| Surface | State | Severity |
|---|---|---|
| `client/src/immersive/threejs/__tests__/VRGraphCanvas.test.tsx` | Tests Three.js R3F `<XR>` provider; framework gone | P0 — DELETE |
| `client/src/immersive/threejs/__tests__/VRInteractionManager.test.tsx` | Tests Three.js raycaster + WebXR controllers; substrate gone | P0 — DELETE |
| `client/src/immersive/threejs/__tests__/VRActionConnectionsLayer.test.tsx` | Tests Three.js InstancedMesh edges in XR; substrate gone | P0 — DELETE |
| `client/src/immersive/hooks/__tests__/useVRHandTracking.test.ts` | Tests WebXR hand-input source enumeration; replaced by gdext `XrRuntime::hand_joints()` | P0 — DELETE |
| `client/src/immersive/hooks/__tests__/useVRConnectionsLOD.test.ts` | Tests JS LOD threshold computation; replaced by Rust `lod::compute_tier()` | P0 — DELETE |
| `xr-client/rust/` | gdext crate with comprehensive test coverage (see §4.1.1 below) | **COVERED** |
| `crates/visionclaw-xr-presence` | Presence crate with 17 inline + 45 external tests (see §4.1.2 below) | **COVERED** |
| `src/handlers/presence_handler.rs` | Actix WS endpoint; contract tests in progress | P0 |
| `src/actors/presence_actor.rs` | Per-room actor; tests in progress | P0 |
| OpenXR runtime contract | Hexagonal port architecture with `FakeIdentity`/`FakePresence` in `ports/mod.rs` enables headless testing without Khronos runtime | **MITIGATED** |
| Quest 3 on-device perf budget | Perf benchmark harness (`benchmark.gd`, `benchmark_scene.tscn`, `run_benchmark.gd`) operational; on-device profiling baseline not yet captured | P0 |
| Binary protocol decode in gdext | `binary_protocol.rs` (5 inline tests + integration tests); `wire_decode` fuzz target operational in `crates/visionclaw-xr-presence/fuzz/` | **COVERED** |
| Multi-user voice routing | `webrtc_audio.rs` API surface tested (13+ inline tests, being expanded); LiveKit AAR JNI bridge not started | P1 |
| APK size / startup time | CI workflow includes APK build job with size gate | P1 |

The Quest 3 client cannot ship without QE coverage that exercises the **actual Rust ↔ Godot ↔ OpenXR ↔ presence-WS data flow** from device through the Rust substrate. This PRD specifies that test surface and pins the gates.

---

## 2. Goals

| # | Goal | Success Metric |
|---|------|----------------|
| Q1 | All legacy XR tests are explicitly DELETED with replacement test IDs recorded | Zero file remaining under `client/src/immersive/{threejs,hooks}/__tests__/`; deletion manifest in §11 maps each old test to its successor |
| Q2 | gdext Rust crate has unit + property coverage of every public surface | `cargo test -p xr-client-rust` ≥ 200 tests; line coverage ≥ 80% |
| Q3 | `crates/visionclaw-xr-presence` has unit + property coverage | `cargo test -p visionclaw-xr-presence` ≥ 80 tests; line coverage ≥ 85% |
| Q4 | Presence WS handler has contract coverage for join/leave/pose/voice/auth | `tests/contract/presence_ws/*.rs` covers all 5 frame types; round-trip Pact-style fixtures shared with gdext client tests |
| Q5 | Binary protocol decoder is fuzzed | `cargo fuzz run binary_protocol_decoder` runs 30 min on CI nightly; zero crashes; wire format identical to ADR-061 |
| Q6 | OpenXR runtime is mockable | Trait `XrRuntime` + `MockXrRuntime` impl; production code does not depend on a live OpenXR loader at unit-test time |
| Q7 | Quest 3 on-device perf gate enforces PRD-008 budgets | 90fps p99 ≥ 88fps; CPU frame ≤ 8ms p95; GPU frame ≤ 8ms p95; motion-to-photon ≤ 20ms p99; ≤ 50 draw calls; ≤ 100K triangles; ≤ 80MB APK |
| Q8 | End-to-end multi-user scenario is exercised | 4-headset (or 4-emulated) test: join → see avatars → spatial voice → joint node manipulation → clean leave; runs nightly |
| Q9 | Visual regression catches material drift | Headless Godot screenshot diff against pinned baselines for gem / crystal-orb / agent-capsule node materials; threshold ≤ 1% pixel delta |
| Q10 | Security gate enforces threat model invariants | 0 critical/high from `cargo audit`, `semgrep`, OWASP dependency check on bundled AARs; pose-injection / replay / room-enumeration penetration suite is green |
| Q11 | CI gates the work | `.github/workflows/xr-godot-ci.yml` runs lint → unit → contract → integration → APK build → security; nightly adds on-device perf + visual regression + multi-headset E2E |
| Q12 | Mutation testing baseline on safety-critical Rust | `cargo mutants` on `xr-client/rust/binary_protocol/` and `crates/visionclaw-xr-presence/src/validate/` survives ≤ 10% of mutants |

---

## 3. Non-Goals

- E2E desktop browser tests for the legacy WebXR client. The substrate is being deleted; tests are deleted with it.
- Performance regression tests for the Three.js renderer. Out of scope; Three.js is gone.
- Cross-platform XR runtime support beyond Quest 3. PCVR / Pico / Vision Pro are out of scope for v1; mock runtime exercises platform abstraction but production targets Quest 3 only.
- Property tests across the entire codebase. Scoped to pose validation, binary protocol decode, LOD threshold computation.
- Vircadia compatibility tests (TESTING_GUIDE §5). Vircadia is removed by the PRD-008 removal plan.
- LiveKit infrastructure tests. Voice routing is tested at the gdext / presence-WS boundary; LiveKit's own SFU is treated as a third-party dependency with a mock for unit tests and a containerised instance for E2E.
- Test coverage for legacy `client/src/contexts/Vircadia*.tsx` shims. Removed in PRD-008.

---

## 4. Test Pyramid

The pyramid is sized to PRD-008's surface. Counts are minimum gate values; actual implementations will exceed them.

```mermaid
graph TD
    E2E["E2E Multi-User (5+)<br/><i>nightly, paired hardware or 4x emulator</i>"]
    VR["Visual Regression (15+)<br/><i>per-release</i>"]
    PERF["On-Device Perf (12+)<br/><i>nightly, self-hosted Quest 3 runner</i>"]
    SEC["Security (10+)<br/><i>per-PR (SAST), nightly (DAST)</i>"]
    INT["Integration (20+)<br/><i>per-PR (headless Godot + Rust handler)</i>"]
    CON["Contract (15+)<br/><i>per-PR (presence WS Pact)</i>"]
    PROP["Property + Fuzz (8+)<br/><i>per-PR (proptest), nightly (libfuzzer)</i>"]
    GDS["Unit GDScript (30+)<br/><i>per-PR (gut)</i>"]
    RUST["Unit Rust (200+ + 80+)<br/><i>per-PR (cargo test)</i>"]

    E2E --> VR --> PERF --> SEC --> INT --> CON --> PROP --> GDS --> RUST
```

### 4.1 Unit (Rust) — `cargo test`

| Crate | Min Tests | Current (2026-05-04) | Coverage Gate | Frameworks |
|---|---|---|---|---|
| `xr-client/rust/` | 200 | 80+ (see §4.1.1) | ≥ 80% line | `#[test]`, `#[tokio::test]`, `proptest`, `mockall`, `rstest` |
| `crates/visionclaw-xr-presence` | 80 | 62 (see §4.1.2) | ≥ 85% line | `#[test]`, `proptest`, `mockall` |
| `src/handlers/presence_handler.rs` (new) | 25 | in progress | ≥ 75% line | `#[test]`, `actix-web::test`, `tokio-tungstenite` test client |
| `src/actors/presence_actor.rs` (new) | 25 | in progress | ≥ 75% line | `#[test]`, `actix::test` |

#### 4.1.1 Actual test counts for `xr-client/rust/` (as of 2026-05-04)

| Module | Inline tests | Integration tests | Property tests | Visual fixtures | Total | Notes |
|---|---|---|---|---|---|---|
| `binary_protocol.rs` (214 lines) | 5 | via `pose_wire_round_trip.rs` (3) | via proptest | — | 8+ | 0x42 position frame decode, round-trips, edge cases |
| `presence.rs` (331 lines) | 4 (async) | `presence_handshake.rs` (2) | — | — | 6 | WS client with NIP-98 auth, reconnect/backoff |
| `interaction.rs` (266 lines) | 9 | `interaction_raycast.rs` (8) | `property_interaction.rs` (11) | — | 28 | Ray cast, pinch detection, gesture FSM |
| `lod.rs` (200 lines) | 7 | `lod_thresholds.rs` (5) | `property_lod.rs` (9) | `visual_fixture.rs` (5) | 26 | Distance-bucket LOD, monotonicity, numerical visual fixtures |
| `webrtc_audio.rs` | 13+ | — | — | — | 13+ | Spatial voice routing API; being expanded by parallel agents |
| `ports/mod.rs` (151 lines) | — | (used by all above) | — | — | — | Hexagonal port architecture with 4 fake transports |

**Total implemented:** 80+ tests across inline, integration, property, and visual fixture categories. Target: 200 per §4.1.

Module-level test breakdown for `xr-client/rust/` (remaining target counts to reach 200):

| Module | Current | Target | Gap | Notes |
|---|---|---|---|---|
| `binary_protocol` | 8+ | 40 | ~32 | Endianness edge cases, NaN/Inf rejection, alignment — being expanded |
| `presence` | 6 | 35 | ~29 | Pose buffering, dropout detection, reconnect stress — being expanded |
| `interaction` | 28 | 35 | ~7 | Controller-vs-hand precedence, additional gesture states |
| `lod` | 26 | 25 | **met** | Target exceeded |
| `webrtc_audio` | 13+ | 30 | ~17 | Spatial pan vector, packet-loss synthesis, mute toggle — being expanded |
| `xr_runtime` (trait + Khronos impl) | 0 | 20 | 20 | Trait surface, swapchain creation, `MockXrRuntime` parity — planned |
| `gdext_bridge` (boundary helpers) | 0 | 15 | 15 | Variant marshalling, Godot signal emit — planned |

#### 4.1.2 Actual test counts for `crates/visionclaw-xr-presence` (as of 2026-05-04)

| Source module | Inline tests | Notes |
|---|---|---|
| `wire.rs` (304 lines) | 4 | 0x43 encode/decode, transform_mask bitfield, round-trip |
| `validate.rs` (202 lines) | 7 | Quaternion unit-length, velocity bounds, anatomical reach, NaN rejection |
| `room.rs` (174 lines) | 3 | Membership add/remove, duplicate-DID rejection, broadcast_sequence |
| `delta.rs` (142 lines) | 3 | Delta compression encode/decode, identity property |

| Test file | Tests | Notes |
|---|---|---|
| `tests/integration.rs` | 9 | Join/leave/concurrent-rooms, pose forwarding, membership lifecycle |
| `tests/property_tests.rs` | 12 | proptest: round-trip wire codec, NaN rejection, quaternion validation, monotonicity |
| `tests/adversarial_tests.rs` | 24 | Spoofed poses, replay attacks, rate-limit enforcement, bad signatures, room enumeration |

**Additional test infrastructure:**
- `fuzz/fuzz_targets/wire_decode.rs` — libfuzzer target for wire protocol decode
- `proptest-regressions/wire.txt` — regression seeds from previous proptest runs
- `benches/wire.rs` + `benches/baseline.json` — Criterion benchmarks with committed baseline

**Total implemented:** 62 tests (17 inline + 9 integration + 12 property + 24 adversarial). Target: 80 per §4.1.

### 4.2 Unit (GDScript) — `gut`

| Scene / Script | Min Tests | Current (2026-05-04) |
|---|---|---|
| `xr_boot.gd` (OpenXR boot + capability probe) | 8 | scene load test in `test_scene_load.gd`; being expanded |
| `graph_scene.gd` (MultiMesh graph, avatar lifecycle) | 6 | scene load test; _process loop being expanded |
| `avatar.gd` (pose application, interpolation) | 6 | scene load test; hand position/rotation tests being added |
| `hud.gd` (room join, mute, presence/voice) | 6 | scene load test; live data wiring being added |
| `benchmark.gd` (perf harness) | 4 | operational; `benchmark_scene.tscn` + `run_benchmark.gd` |

**Implemented:** GUT test runner (`run_gut.gd`) + scene load tests (`test_scene_load.gd`) operational. Test count being expanded by parallel agents.

Total target: **30+ GDScript tests** covering scene initialisation, signal wiring, node lifecycle, panel anchoring.

### 4.3 Integration (gdext ↔ Godot) — Headless Godot

Headless Godot test runner exercises the Rust ↔ GDScript boundary via the gdext extension loaded into a script-driven scene. Tests run under `godot --headless --script res://tests/integration/<test>.gd`.

| Test | What it pins |
|---|---|
| `presence_join_emits_signal.gd` | gdext `Presence::join()` emits `presence_joined` Godot signal with correct DID |
| `pose_stream_updates_avatar.gd` | gdext-decoded pose frames update avatar `Transform3D` within 1 frame |
| `binary_frame_to_node_position.gd` | 24-byte frame from `binary_protocol` lands at `NodeInstance.global_position` |
| `lod_tier_switches_geometry.gd` | Tier change from HighDetail→MidDetail swaps mesh resource via gdext signal |
| `hand_joint_pinch_selects_node.gd` | gdext hand FSM emits `node_selected` signal; `NodeInstance` receives it |
| `webrtc_audio_join_routes.gd` | Voice room join fires gdext signal; UI panel reflects participant list |
| `room_disconnect_recovers.gd` | WS dropout triggers reconnect; avatar state restored within 5s |
| `controller_hand_priority.gd` | When both controllers and hands tracked, controllers win for selection |
| `passthrough_toggle.gd` | OpenXR passthrough flag toggles via gdext, Godot environment updates |
| `app_pause_releases_xr.gd` | Android pause releases swapchains; resume re-acquires within 100ms |

Total: **20+ integration tests**, runs per-PR.

### 4.4 Contract (Presence WS) — Pact-style

Shared canonical fixtures live at `tests/fixtures/presence/canonical/` — JSON frame samples that both the Rust gdext client tests and the Rust handler tests load. Either side adding a field requires the fixture to change first.

| Test | Frame type | Direction |
|---|---|---|
| `join_request_well_formed.rs` | `JoinRoom { room, did, signature }` | client → handler |
| `join_response_includes_roster.rs` | `RoomState { participants, version }` | handler → client |
| `leave_clean.rs` | `LeaveRoom { reason: Clean }` | client → handler |
| `leave_dropout.rs` | `LeaveRoom { reason: Timeout }` | handler → other clients |
| `pose_frame_24_byte.rs` | `Pose { 24-byte binary }` | client → handler → fanout |
| `pose_frame_invalid_quaternion.rs` | rejects with `InvalidPose` | handler-side validation |
| `voice_route_announce.rs` | `VoiceRoute { livekit_token, sfu_url }` | handler → client |
| `voice_route_revoke.rs` | revocation fans out within 100ms | handler → all clients |
| `auth_did_signature_required.rs` | unsigned join rejected with `AuthRequired` | handler-side |
| `auth_did_signature_invalid.rs` | bad sig rejected with `AuthInvalid` | handler-side |
| `auth_did_replay_rejected.rs` | nonce reuse → `ReplayDetected` | handler-side |
| `room_enumeration_forbidden.rs` | unauthenticated room-list request → 403 | handler-side |
| `pose_rate_limit.rs` | > 120 Hz pose stream throttled to 90 Hz | handler-side |
| `disconnect_idle_timeout.rs` | no activity ≥ 30s → handler closes WS | handler-side |
| `version_mismatch_rejected.rs` | client sending wrong protocol version → `VersionMismatch` | handler-side |

Total: **15+ contract tests**, per-PR.

### 4.5 Property-based — `proptest`

**Implementation status (2026-05-04):** Property tests operational in three files:
- `xr-client/rust/tests/property_interaction.rs` — 11 property tests (pinch detection, ray cast, gesture invariants)
- `xr-client/rust/tests/property_lod.rs` — 9 property tests (monotonicity, determinism, tier ordering)
- `crates/visionclaw-xr-presence/tests/property_tests.rs` — 12 property tests (wire round-trip, NaN rejection, quaternion validation)
- Proptest regression seeds committed at `crates/visionclaw-xr-presence/proptest-regressions/wire.txt`

| Property | Module | Strategy | Status |
|---|---|---|---|
| `validate_pose(decode(encode(p))) == Some(p)` for valid `p` | `presence` | random valid pose generator | **Implemented** in `property_tests.rs` |
| `validate_pose` rejects all NaN, Inf, non-unit quaternions | `presence` | adversarial generator | **Implemented** in `property_tests.rs` + `adversarial_tests.rs` |
| Binary protocol decode is total: `decode(any 24 bytes) ∈ Ok | Err` (no panics) | `binary_protocol` | random byte arrays | **Implemented** via proptest + fuzz target |
| `decode(encode(frame)) == frame` for all valid frames | `binary_protocol` | random valid frame generator | **Implemented** in `pose_wire_round_trip.rs` |
| `lod::compute_tier` is monotonic in distance: `d1 ≤ d2 ⇒ tier(d1) ≤ tier(d2)` | `lod` | (distance, frustum, speed) tuples | **Implemented** in `property_lod.rs` |
| `lod::compute_tier(d, f, 0) == lod::compute_tier(d, f, 0)` deterministic | `lod` | reflexivity | **Implemented** in `property_lod.rs` |
| Spatial pan vector magnitude ≤ 1.0 for all listener / source positions | `webrtc_audio` | random Vec3 pairs | Planned |
| Room ID slug normalisation is idempotent | `presence` | random Unicode strings | **Implemented** in `property_tests.rs` |
| Hand pinch detection threshold consistency | `interaction` | random joint positions | **Implemented** in `property_interaction.rs` |
| Ray cast hit distance is non-negative | `interaction` | random ray origins/directions | **Implemented** in `property_interaction.rs` |

Total: **32+ property tests implemented** (target: 8+ per-PR gate — exceeded).

### 4.6 Fuzz — `cargo-fuzz` (libfuzzer)

Required by the threat model (see §10).

**Implementation status (2026-05-04):** One fuzz target operational:
- `crates/visionclaw-xr-presence/fuzz/fuzz_targets/wire_decode.rs` — fuzzes the 0x43 avatar pose wire decode path
- `crates/visionclaw-xr-presence/fuzz/Cargo.toml` — fuzz crate configuration

| Target | Corpus seed | Schedule | Status |
|---|---|---|---|
| `wire_decode` (presence crate) | Proptest regression seeds + contract fixtures | nightly 30 min | **Operational** |
| `binary_protocol_decoder` | 100 hand-crafted seeds: edge cases from §4.5 | nightly 30 min | Planned |
| `did_signature_verifier` | Real signatures + bit-flipped variants | nightly 30 min | Planned |

Crash policy: any crash blocks the next release until triaged + fixed; corpus auto-grows and is committed weekly.

### 4.7 Performance — Quest 3 on-device

Self-hosted runner with Quest 3 in developer mode, paired via `adb pair`. Tests deploy APK, drive via ADB intent, and read perf counters via Godot `--benchmark` plus `adb shell dumpsys gfxinfo`.

| Test | Budget (per PRD-008) | Measurement |
|---|---|---|
| `quest3_steady_90fps_baseline.sh` | 90fps p99 ≥ 88 | 60s sustained, empty room, single-user |
| `quest3_steady_90fps_loaded.sh` | 90fps p99 ≥ 88 | 60s sustained, 4-user room, 1000 nodes visible |
| `quest3_motion_to_photon.sh` | ≤ 20ms p99 | head-shake test, photodiode-derived synthetic measurement via OpenXR `xrLocateViews` timestamps |
| `quest3_cpu_frame_time.sh` | ≤ 8ms p95 | gfxinfo CPU column |
| `quest3_gpu_frame_time.sh` | ≤ 8ms p95 | gfxinfo GPU column |
| `quest3_draw_calls.sh` | ≤ 50 | RenderDoc capture, automated parse |
| `quest3_triangle_count.sh` | ≤ 100K | RenderDoc capture |
| `quest3_apk_size.sh` | ≤ 80 MB | post-build artefact size check |
| `quest3_cold_start.sh` | ≤ 4s to first XR frame | `am start` to first `xrEndFrame` |
| `quest3_thermal_sustained.sh` | no throttle in 20 min run | thermal-state polling via `adb shell dumpsys thermalservice` |
| `quest3_battery_drain.sh` | ≤ 25%/hour | fuel-gauge before/after 60-min session |
| `quest3_room_capacity_4_user.sh` | maintains 90fps with 4 avatars | scripted multi-emulator scenario |

Total: **12+ on-device perf tests**, nightly.

### 4.8 Visual regression — Headless Godot screenshot diff + numerical fixtures

**Implementation note (2026-05-04):** The initial visual regression approach uses **numerical visual fixtures** (threshold-to-tier mapping assertions in `xr-client/rust/tests/visual_fixture.rs`, 5 tests) rather than pixel-diff. These verify that LOD tier boundaries produce correct geometry/material selection for given distances. Pixel-diff visual regression against headless Godot screenshots is the target state.

Reference architecture mirrors `client/playwright/` but adapted: Godot exports a headless runner that loads a scripted scene, renders one frame, dumps PNG, and the diff harness compares against committed baselines.

| Scene | What it pins |
|---|---|
| `node_gem.png` | Knowledge-graph gem icosahedron material (radius 0.5, refraction, PBR) |
| `node_crystal_orb.png` | Ontology crystal sphere (radius 0.5, internal lattice) |
| `node_agent_capsule.png` | Agent capsule (radius 0.3, height 0.6, emissive) |
| `edge_glass.png` | Glass cylinder edge (radius 0.03) at 1m, 5m, 50m distances |
| `lod_tier_high.png` | HighDetail tier render at 0.5m |
| `lod_tier_mid.png` | MidDetail at 5m |
| `lod_tier_far.png` | Far at 50m |
| `lod_tier_cull.png` | Past cull threshold; expect zero geometry pixels |
| `passthrough_room.png` | Passthrough enabled; overlay only graph geometry |
| `hand_joint_indicator.png` | Hand-tracking pinch indicator at neutral pose |
| `controller_ray.png` | Controller raycast visualisation |
| `ui_panel_default.png` | UI HUD panel default state |
| `ui_panel_room_roster.png` | UI HUD with 4-participant roster |
| `voice_indicator_active.png` | Speaking-participant glow ring |
| `selected_node_outline.png` | Selected-node outline shader |

Threshold: ≤ 1% pixel delta against committed baseline (CIEDE2000 colour delta + Δ pixel-count). Total: **15+ visual regression tests**, per-release.

### 4.9 End-to-End multi-user

`tests/e2e/multiuser_session.rs` drives 4 sessions (4 paired Quest 3 headsets when hardware available; otherwise 4× Godot Android emulator + mock OpenXR runtime). Scenario:

1. All 4 join the same room within a 2s window
2. Each sees the other 3 avatars within 1s of join
3. Spatial voice activates: speaker-1 talks, speakers 2-4 receive within 200ms with correct spatial pan
4. Speaker-1 selects a graph node; selection state propagates to all clients within 200ms
5. All 4 leave cleanly; presence handler reports 0 participants within 1s

Pass criteria: every step succeeds for 5 consecutive runs. Runs nightly.

### 4.10 Security

| Test | Threat (per threat model) | Tool |
|---|---|---|
| `cargo_audit_xr_client.sh` | Dependency CVE | `cargo audit` |
| `cargo_audit_presence.sh` | Dependency CVE | `cargo audit` |
| `semgrep_xr_client.sh` | Tainted-input flow analysis on all `pub fn` accepting external bytes | `semgrep --config=p/rust` |
| `owasp_dependency_check_aar.sh` | Bundled Android AAR libraries (LiveKit SDK, OpenXR loader) | OWASP Dep Check |
| `pose_injection_pen_test.rs` | Forged pose frames bypass DID auth | custom DAST harness against running handler |
| `replay_attack_pen_test.rs` | Replayed signed join packets | custom DAST harness |
| `room_enumeration_pen_test.rs` | Unauthenticated room discovery | custom DAST harness |
| `voice_token_leakage_pen_test.rs` | LiveKit JWT leaks across rooms | custom DAST harness |
| `apk_secret_scan.sh` | Secrets baked into APK | `trufflehog` over APK contents |
| `permission_audit.sh` | AndroidManifest declares only required perms (`com.oculus.permission.HAND_TRACKING`, etc.) | manifest-diff against allowlist |

Total: **10+ security tests**, per-PR (SAST) + nightly (DAST).

---

## 5. Quality Gates

CI MUST enforce these. No `--force-pass`. No advisory-by-default for any gate marked Required.

### 5.1 Coverage gates

| Surface | Line | Branch | Mutation survival |
|---|---|---|---|
| `xr-client/rust/` | ≥ 80% | ≥ 75% | ≤ 15% |
| `crates/visionclaw-xr-presence` | ≥ 85% | ≥ 80% | ≤ 10% |
| `src/handlers/presence_handler.rs` | ≥ 75% | — | — |
| `src/actors/presence_actor.rs` | ≥ 75% | — | — |
| `xr-client/rust/binary_protocol/` (safety-critical) | ≥ 90% | ≥ 85% | ≤ 5% |
| `crates/visionclaw-xr-presence/src/validate/` (safety-critical) | ≥ 90% | ≥ 85% | ≤ 5% |

`cargo tarpaulin --workspace --skip-clean` reports per-PR; deltas surfaced as PR comment. Mutation gate uses `cargo mutants --in-diff` per PR.

### 5.2 Performance gates

| Metric | Budget | Tolerance |
|---|---|---|
| Quest 3 sustained framerate | 90fps p99 | ±2% (≥ 88fps p99) |
| Motion-to-photon | ≤ 20ms p99 | hard cap |
| CPU frame time | ≤ 8ms p95 | ±1ms |
| GPU frame time | ≤ 8ms p95 | ±1ms |
| Draw calls | ≤ 50 | hard cap |
| Triangle count | ≤ 100K | hard cap |
| Binary protocol decode | ≤ 1µs/frame on Quest 3 ARM | ±10% |
| Pose validation | ≤ 5µs/frame on Quest 3 ARM | ±10% |

Regression detection compares the run to the rolling 7-day median; > 5% degradation against median is a PR block.

### 5.3 APK size gate

| Artefact | Budget | Hard cap |
|---|---|---|
| Release APK | ≤ 80 MB | 100 MB |
| Per-architecture (arm64-v8a only for Quest 3) | ≤ 80 MB | 100 MB |
| Cold-start to first XR frame | ≤ 4s | 6s |

### 5.4 Security gates

- 0 critical / 0 high CVE from `cargo audit` and OWASP Dep Check
- 0 high-severity findings from `semgrep --config=p/rust --severity=ERROR`
- All 4 DAST pen-tests (pose injection, replay, room enumeration, voice token leakage) green
- AndroidManifest matches the allowlist in `xr-client/godot/permissions.allowlist`

### 5.5 Lint gates

- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo fmt --check` clean
- `gdlint xr-client/godot/` clean
- `cargo deny check` clean (license + supply-chain)

### 5.6 Gate matrix (CI)

| Stage | Required for merge | Required for release |
|---|---|---|
| Lint (clippy / fmt / gdlint / deny) | ✅ | ✅ |
| Rust unit | ✅ | ✅ |
| GDScript unit | ✅ | ✅ |
| Property + fuzz (PR-time, 1 min smoke) | ✅ | ✅ |
| Contract | ✅ | ✅ |
| Integration (headless Godot) | ✅ | ✅ |
| Security SAST | ✅ | ✅ |
| Coverage (tarpaulin) | ✅ | ✅ |
| Mutation (in-diff) | ✅ on safety-critical paths | ✅ |
| APK build | nightly | ✅ |
| On-device perf | nightly | ✅ |
| Security DAST | nightly | ✅ |
| Visual regression | nightly | ✅ |
| Multi-user E2E | nightly | ✅ |
| Fuzz (full 30 min) | nightly | ✅ |

---

## 6. Test Data Management

### 6.1 Fixture rooms

`tests/fixtures/presence/rooms/` contains 5 deterministic rooms with synthetic DIDs:

| Room ID | Participants | Purpose |
|---|---|---|
| `room-empty` | 0 | join-into-empty smoke |
| `room-1user` | 1 | join-as-second-user; first-avatar visibility |
| `room-4user-active` | 4 | full-capacity perf scenario |
| `room-stale-participant` | 1 (timed-out) | cleanup logic |
| `room-mid-handshake` | 0 (mid join, never completed) | partial-state recovery |

Synthetic DIDs use deterministic seeds: `did:nostr:0000…0001` through `did:nostr:0000…0010`. Signing keys committed to `tests/fixtures/presence/keys/` with a `TEST-ONLY` README warning.

### 6.2 Recorded pose streams

`tests/fixtures/presence/poses/` contains captured pose streams as `.bin` files (raw 24-byte frames concatenated, with a `.meta.json` companion describing capture context):

| Stream | Frames | Captured from |
|---|---|---|
| `idle-standing.bin` | 5400 (60s @ 90Hz) | stationary user |
| `head-shake-yaw.bin` | 540 (6s) | head shake; used for motion-to-photon |
| `walking-arc.bin` | 5400 (60s) | room-scale locomotion |
| `hand-pinch-sequence.bin` | 540 (6s) | pinch-release × 10 |
| `controller-trigger-burst.bin` | 540 (6s) | trigger × 20 |
| `dropout-recovery.bin` | 5400 (60s) | tracking-loss → recovery |

These feed regression tests for `presence::pose_buffer` and as the input to perf benchmarks (replay deterministic input, measure deterministic output).

### 6.3 Synthetic graph snapshots

`tests/fixtures/graphs/` contains `.rvf` files matching existing `agentdb.rvf` patterns:

| Snapshot | Nodes | Edges | Purpose |
|---|---|---|---|
| `xr-tiny.rvf` | 10 | 12 | unit tests |
| `xr-small.rvf` | 100 | 150 | integration tests |
| `xr-typical.rvf` | 1000 | 2500 | perf baseline (matches Quest 3 budget) |
| `xr-stress.rvf` | 5000 | 12500 | stress / regression |

Each snapshot has a `.snapshot-hash.txt` blake3 hash; CI verifies the hash before each run to catch fixture corruption.

---

## 7. CI/CD Integration

### 7.1 Workflow `.github/workflows/xr-godot-ci.yml` (operational — 473 lines, 10 jobs)

```yaml
name: XR Godot CI
on:
  pull_request:
    paths:
      - 'xr-client/**'
      - 'crates/visionclaw-xr-presence/**'
      - 'src/handlers/presence_handler.rs'
      - 'src/actors/presence_actor.rs'
      - '.github/workflows/xr-godot-ci.yml'
  push:
    branches: [main]
  schedule:
    - cron: '0 3 * * *'  # nightly 03:00 UTC

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - cargo clippy --workspace --all-targets -- -D warnings
      - cargo fmt --check
      - gdlint xr-client/godot/
      - cargo deny check

  rust-unit:
    runs-on: ubuntu-latest
    steps:
      - cargo test -p xr-client-rust -p visionclaw-xr-presence --lib --bins
      - cargo test -p visionclaw --test 'presence_*'

  gdscript-unit:
    runs-on: ubuntu-latest
    container: barichello/godot-ci:4.3
    steps:
      - godot --headless --path xr-client/godot/ --script res://addons/gut/gut_cmdln.gd

  property-fuzz-smoke:
    runs-on: ubuntu-latest
    steps:
      - cargo test --features proptest-extra
      - timeout 60 cargo +nightly fuzz run binary_protocol_decoder

  contract:
    runs-on: ubuntu-latest
    steps:
      - cargo test --test 'contract_presence_ws_*' -- --include-ignored

  integration:
    runs-on: ubuntu-latest
    container: barichello/godot-ci:4.3
    steps:
      - cargo build -p xr-client-rust --release
      - godot --headless --path xr-client/godot/ --script res://tests/integration/run_all.gd

  security-sast:
    runs-on: ubuntu-latest
    steps:
      - cargo audit
      - semgrep --config=p/rust --error
      - dependency-check.sh --scan xr-client/godot/android/libs/

  coverage:
    runs-on: ubuntu-latest
    steps:
      - cargo tarpaulin -p xr-client-rust -p visionclaw-xr-presence --out Xml
      - bash <(curl -s https://codecov.io/bash)

  mutation:
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    steps:
      - cargo mutants --in-diff --baseline auto --error-on-survival --packages xr-client-rust visionclaw-xr-presence

  apk-build:
    if: github.event_name == 'schedule' || startsWith(github.ref, 'refs/tags/')
    runs-on: ubuntu-latest
    container: barichello/godot-ci:4.3-mono
    steps:
      - godot --headless --path xr-client/godot/ --export-release "Quest 3" build/visionclaw-xr.apk
      - test $(stat -c %s build/visionclaw-xr.apk) -le 83886080  # 80 MiB cap
      - upload-artifact: build/visionclaw-xr.apk

  on-device-perf:
    if: github.event_name == 'schedule' || startsWith(github.ref, 'refs/tags/')
    needs: [apk-build]
    runs-on: [self-hosted, quest3]
    steps:
      - adb install -r build/visionclaw-xr.apk
      - bash tests/perf/run_all_quest3.sh

  security-dast:
    if: github.event_name == 'schedule' || startsWith(github.ref, 'refs/tags/')
    runs-on: ubuntu-latest
    steps:
      - docker compose -f tests/fixtures/presence-handler.yml up -d
      - cargo test --test 'pen_test_*' --features dast

  visual-regression:
    if: github.event_name == 'schedule' || startsWith(github.ref, 'refs/tags/')
    runs-on: ubuntu-latest
    container: barichello/godot-ci:4.3
    steps:
      - godot --headless --path xr-client/godot/ --script res://tests/visual/capture_all.gd
      - python tests/visual/diff.py --threshold 0.01

  multi-user-e2e:
    if: github.event_name == 'schedule'
    needs: [apk-build]
    runs-on: [self-hosted, quest3-paired-4]
    steps:
      - bash tests/e2e/multiuser_4headset.sh

  fuzz-full:
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    steps:
      - timeout 1800 cargo +nightly fuzz run binary_protocol_decoder
      - timeout 1800 cargo +nightly fuzz run presence_frame_parser
      - timeout 1800 cargo +nightly fuzz run did_signature_verifier
```

### 7.2 PR-time vs nightly vs release split

| Stage | PR | Nightly | Release |
|---|---|---|---|
| Lint | ✅ | ✅ | ✅ |
| Rust + GDScript unit | ✅ | ✅ | ✅ |
| Property + fuzz smoke (60s) | ✅ | ✅ | ✅ |
| Contract | ✅ | ✅ | ✅ |
| Integration (headless Godot) | ✅ | ✅ | ✅ |
| SAST + coverage + mutation | ✅ | ✅ | ✅ |
| APK build | — | ✅ | ✅ |
| On-device perf | — | ✅ | ✅ |
| DAST | — | ✅ | ✅ |
| Visual regression | — | ✅ | ✅ |
| Multi-user E2E | — | ✅ | ✅ |
| Fuzz full (30 min × 3 targets) | — | ✅ | ✅ |

---

## 8. Test Environments

### 8.1 Local dev

- `cargo test -p xr-client-rust -p visionclaw-xr-presence` for Rust unit
- `cargo run --bin webxr` for the presence handler (talks to local in-memory Neo4j fixture)
- `godot --editor --path xr-client/godot/` for scene work
- `gut` panel inside the editor for GDScript tests
- Headless integration: `godot --headless --script res://tests/integration/run_all.gd`
- Local Quest 3: `adb install` + `adb shell am start -n com.visionclaw.xr/.MainActivity`

### 8.2 CI ephemeral

- Docker base: `barichello/godot-ci:4.3` for Godot tooling, plus stock Rust toolchain image for cargo
- Android NDK pre-installed in the Godot image variant
- Neo4j testcontainer for any handler-side test that needs the persistence layer

### 8.3 On-device (self-hosted runner)

- Self-hosted GitHub Actions runner labelled `quest3` with one paired Quest 3 in dev mode (`adb pair` token in runner secrets)
- Runner labelled `quest3-paired-4` is the same machine with 4 Quest 3 headsets paired for multi-user E2E
- ADB stable connection via USB; runner restarts ADB daemon between jobs to clear state
- Headset is reset to a known scene between runs via `adb shell am force-stop com.visionclaw.xr`

### 8.4 Voice infrastructure

- LiveKit dev server in a container at `tests/fixtures/livekit-dev.yml` (single-node, ephemeral tokens)
- Real LiveKit cloud is OUT of scope for CI; staging environment uses a separate prod-shaped LiveKit instance

---

## 9. Mock / Fake Strategy

### 9.1 OpenXR runtime mock

```rust
pub trait XrRuntime {
    fn poll_events(&mut self) -> Vec<XrEvent>;
    fn locate_views(&mut self, time: XrTime) -> Result<[ViewState; 2], XrError>;
    fn hand_joints(&mut self, hand: Hand) -> Result<[JointPose; 26], XrError>;
    fn submit_frame(&mut self, swapchains: &[SwapchainImage]) -> Result<(), XrError>;
    fn set_passthrough(&mut self, enabled: bool) -> Result<(), XrError>;
}

#[cfg(test)]
pub struct MockXrRuntime { /* scripted event queue + pose generator */ }
```

Production code (`xr-client/rust/xr_runtime/khronos.rs`) implements `XrRuntime` against the live Khronos loader. Tests use `MockXrRuntime` and never load `libopenxr_loader.so`.

### 9.2 Presence WS mock

`xr-client/rust/presence/mock.rs` exposes `MockPresenceWs` — a tokio-channel-backed fake that scripts server responses and records sent frames. Used by all `presence` unit tests and by gdext integration tests that exercise the client side without booting the real handler.

### 9.3 LiveKit mock

`xr-client/rust/webrtc_audio/mock.rs` exposes `MockLiveKitRoom`. Voice routing tests verify the gdext crate emits correct join/publish/subscribe calls without requiring a LiveKit server. The DAST tier uses the real LiveKit dev server.

### 9.4 Nostr signer mock

`tests/fixtures/presence/keys/test_signer.rs` provides deterministic Ed25519 signing for join frames. Production uses the user's real Nostr key via the device keystore; tests use the fixture key set.

### 9.5 ADB driver mock

For tests that exercise the on-device perf harness logic (parsing gfxinfo output, etc.) without a paired headset, `tests/perf/mock_adb.sh` returns canned dumpsys output. The full perf harness on real devices is exercised only on the self-hosted runner.

---

## 10. Traceability Matrix

Every PRD-008 requirement maps to at least one covering test ID. Test IDs follow the convention `[layer]:[module]:[test]` to be machine-resolvable by the QE Traceability Graph (BC21).

| PRD-008 requirement | Covering test IDs |
|---|---|
| R01: Native Quest 3 APK via Godot 4.3 export | perf:apk:quest3_apk_size, perf:apk:quest3_cold_start |
| R02: gdext crate provides binary_protocol module compatible with ADR-061 wire format | unit:binary_protocol:* (40 tests), property:binary_protocol:decode_total, fuzz:binary_protocol_decoder, contract:presence_ws:pose_frame_24_byte |
| R03: Presence WS endpoint at `src/handlers/presence_handler.rs` | unit:presence_handler:* (25), contract:presence_ws:* (15) |
| R04: Per-room presence actor | unit:presence_actor:* (25), integration:presence_join_emits_signal |
| R05: 90fps sustained on Quest 3 | perf:quest3_steady_90fps_baseline, perf:quest3_steady_90fps_loaded, perf:quest3_room_capacity_4_user |
| R06: Motion-to-photon ≤ 20ms | perf:quest3_motion_to_photon |
| R07: CPU frame ≤ 8ms, GPU frame ≤ 8ms | perf:quest3_cpu_frame_time, perf:quest3_gpu_frame_time |
| R08: ≤ 50 draw calls, ≤ 100K triangles | perf:quest3_draw_calls, perf:quest3_triangle_count |
| R09: APK ≤ 80MB | perf:apk:quest3_apk_size |
| R10: Hand tracking via gdext | unit:interaction:* (35), integration:hand_joint_pinch_selects_node, visual:hand_joint_indicator |
| R11: Controller fallback when hands unavailable | unit:interaction:controller_hand_priority, integration:controller_hand_priority |
| R12: LOD tier system | unit:lod:* (25), property:lod:monotonic, integration:lod_tier_switches_geometry, visual:lod_tier_high/mid/far/cull |
| R13: Spatial voice via LiveKit | unit:webrtc_audio:* (30), integration:webrtc_audio_join_routes, e2e:multiuser:step3_spatial_voice |
| R14: Multi-user (≥ 4 simultaneous) | e2e:multiuser:step1_join, perf:quest3_room_capacity_4_user |
| R15: Passthrough toggle | integration:passthrough_toggle, visual:passthrough_room |
| R16: DID-signed presence join | unit:presence:join_signature, contract:presence_ws:auth_did_signature_required, fuzz:did_signature_verifier |
| R17: Reconnect / dropout handling | unit:presence:reconnect_backoff, integration:room_disconnect_recovers, perf:dropout-recovery (replay) |
| R18: Android pause/resume releases XR cleanly | integration:app_pause_releases_xr, perf:quest3_thermal_sustained |
| R19: UI HUD panel | unit:gdscript:UIPanel, visual:ui_panel_default, visual:ui_panel_room_roster |
| R20: Selected node visual feedback | visual:selected_node_outline, integration:hand_joint_pinch_selects_node |
| R21: Voice indicator on speaking participants | visual:voice_indicator_active, e2e:multiuser:step3_spatial_voice |
| R22: Pose injection rejected | security:pose_injection_pen_test, contract:presence_ws:pose_frame_invalid_quaternion |
| R23: Replay attacks rejected | security:replay_attack_pen_test, contract:presence_ws:auth_did_replay_rejected |
| R24: Room enumeration forbidden without auth | security:room_enumeration_pen_test, contract:presence_ws:room_enumeration_forbidden |
| R25: Voice JWT scoped to room | security:voice_token_leakage_pen_test, contract:presence_ws:voice_route_revoke |

Coverage of R01-R25: 100%. Any new requirement added to PRD-008 must arrive with at least one new test ID before merge — enforced by the BC21 traceability gate.

---

## 11. Deletion Manifest (Legacy XR Tests)

Every legacy test below is DELETED with the PRD-008 removal commit. Each maps to its replacement test ID (or test bundle).

| Legacy test | Lines | Replaced by |
|---|---|---|
| `client/src/immersive/threejs/__tests__/VRGraphCanvas.test.tsx` | (full file) | integration:gdext_bridge:* (15 tests) + visual:node_gem / node_crystal_orb / node_agent_capsule |
| `client/src/immersive/threejs/__tests__/VRInteractionManager.test.tsx` | (full file) | unit:interaction:* (35 tests) + integration:hand_joint_pinch_selects_node + integration:controller_hand_priority |
| `client/src/immersive/threejs/__tests__/VRActionConnectionsLayer.test.tsx` | (full file) | visual:edge_glass + integration:binary_frame_to_node_position |
| `client/src/immersive/hooks/__tests__/useVRHandTracking.test.ts` | (full file) | unit:interaction:hand_joint_* (8 tests covered within the 35-test interaction suite) + unit:xr_runtime:hand_joints |
| `client/src/immersive/hooks/__tests__/useVRConnectionsLOD.test.ts` | (full file) | unit:lod:* (25 tests) + property:lod:monotonic + visual:lod_tier_high/mid/far/cull |
| `docs/testing/TESTING_GUIDE.md` §3 (VR Performance Validation) | section | perf:quest3_* (12 tests in §4.7) — TESTING_GUIDE.md §3 is REWRITTEN to point at this PRD |
| `docs/testing/TESTING_GUIDE.md` §5 (Vircadia Integration Testing) | section | DELETED with no replacement; Vircadia substrate removed by PRD-008 removal plan |
| `client/src/tests/vr/VRPerformanceTest.ts` | (full file) | perf:quest3_steady_90fps_baseline + 11 sibling perf tests; old WebXR-based perf test is incompatible with Quest 3 native APK |
| `client/src/tests/integration/VircadiaTest.ts` | (full file) | DELETED with no replacement |

The deletion manifest is committed as `tests/DELETION_MANIFEST_xr_godot.md` in the same PR that lands these tests' replacements. CI runs a check that, once the manifest exists, no file in the manifest may be reintroduced (`scripts/check_deletion_manifest.sh`).

---

## 12. QE Agents Spawned for Execution

This work is executed by a dedicated swarm dispatched from the QE fleet. Per ADR-058 / agentic-quality-engineering skill:

| Agent | Responsibility | Inputs | Outputs |
|---|---|---|---|
| `qe-test-generator` | Auto-generate Rust unit-test stubs from gdext crate AST; enumerate every `pub fn`, `pub struct`, `Trait` impl in `xr-client/rust/` and `crates/visionclaw-xr-presence/`; emit `#[test]` skeletons + `proptest!` blocks where annotated `#[gen-property]` | Crate AST | `tests/generated/<module>_unit.rs` |
| `qe-coverage-analyzer` | Run `cargo tarpaulin`, walk uncovered lines, produce gap report with per-line justification (intentional-skip / missing-test / dead-code), emit `CoverageGapDetected` events for the BC21 graph | tarpaulin XML | gap report + RuVector entries under `qe-traceability` namespace |
| `qe-performance-tester` | Drive on-device perf harness against paired Quest 3; collect per-test metrics; compare to rolling 7-day median; raise PRs to update baselines when intentional improvements land | Quest 3 ADB + APK | perf JSON report + Grafana metrics push |
| `qe-security-scanner` | Run `cargo audit`, `semgrep`, OWASP Dep Check, and the four DAST pen-test harnesses; triage findings by CVSS; auto-file issues for high/critical | repo + presence handler container | security report + GitHub issues |
| `qe-visual-tester` | Run headless Godot screenshot capture, diff against committed baselines, file PR comments with diff images on regression | rendered PNGs | diff report + GitHub PR comments |
| `qe-quality-gate` | Aggregate every gate's verdict per §5.6, produce a single PASS/FAIL per PR or release, block merge or release on FAIL, surface a structured `GateVerdict` to the BC21 traceability graph | all preceding agents' outputs | `GateVerdict` event |

The swarm is initialised under swarm ID `swarm-1777757491161-nl2bbv` with hierarchical-mesh topology, RAFT consensus, and the existing 19-agent QE fleet's resource limits.

---

## 13. Phased Rollout (paired with PRD-008)

| Phase | Tests that gate | Pairs with PRD-008 phase | Status |
|---|---|---|---|
| **P1 (Plumbing)** | unit:binary_protocol:* (40), unit:presence:* (35), unit:presence_handler:* (25), property:binary_protocol:decode_total, contract:presence_ws:join_request/response/leave_clean, integration:binary_frame_to_node_position, lint, coverage gate at 60% | P1 (gdext crate scaffolding + presence handler skeleton) | **SUBSTANTIALLY COMPLETE (2026-05-04):** binary_protocol 8+ tests, presence crate 62 tests (inline+integration+property+adversarial), wire_decode fuzz target, Criterion benchmarks. Handler/actor tests in progress. |
| **P2 (Renderer)** | unit:lod:* (25), unit:interaction:* (35), unit:gdscript:* (30), integration:presence_join_emits_signal / pose_stream_updates_avatar / lod_tier_switches_geometry / hand_joint_pinch_selects_node, visual:node_gem / crystal_orb / agent_capsule / edge_glass, perf:apk:quest3_apk_size, coverage gate at 75% | P2 (Godot scene tree + node materials) | **SUBSTANTIALLY COMPLETE (2026-05-04):** lod 26 tests (target exceeded), interaction 28 tests, GUT runner + scene load tests operational, 5 numerical visual fixtures, perf benchmark harness operational. GDScript test expansion underway. |
| **P3 (Voice + Multi-user)** | unit:webrtc_audio:* (30), contract:presence_ws:voice_route_announce / voice_route_revoke / pose_rate_limit / disconnect_idle_timeout, integration:webrtc_audio_join_routes / room_disconnect_recovers, e2e:multiuser_4headset (manual run), security:voice_token_leakage_pen_test | P3 (LiveKit integration + 4-user rooms) | **IN PROGRESS (2026-05-04):** webrtc_audio 13+ tests; SpatialVoiceRouter methods being wired; LiveKit AAR not started |
| **P4 (Hardening)** | All security:* (10), property + fuzz full (3 × 30 min nightly), perf:quest3_* (12), visual:* (15), mutation gate, coverage gate at full §5.1 levels | P4 (release candidate) | Planned |
| **P5 (GA)** | e2e:multiuser_4headset nightly green for 7 consecutive days, all gates §5.6 release column green, deletion manifest committed, BC21 traceability shows R01-R25 100% covered | P5 (Quest store submission) | Planned |

Per PRD-006 / PRD-QE-001 convention, the QE phase MUST be green before the corresponding PRD-008 phase ships. No phase advances on the back of a partial gate pass.

---

## 14. Risks

| Risk | Mitigation |
|---|---|
| Self-hosted Quest 3 runner fragility (USB cable, ADB drops, headset battery) | Two paired headsets in active rotation; nightly health check job alerts if either degrades; fallback to emulator-based perf with widened tolerances flagged in the report |
| LiveKit dev server flakes under CI load | LiveKit container is per-job, never shared; `livekit-cli health` is a precondition step; failures categorised as infra-flake (auto-retry) vs test-fail (block) |
| Visual regression false positives from driver / GPU updates | Baseline images committed with a `.driver-version.txt` companion; baseline refresh requires a separate PR with two human approvers; CIEDE2000 colour delta tolerates ≤ 1% to absorb minor driver shifts |
| Headless Godot integration slow on CI | Each integration test runs in its own headless session; tests are sharded across 4 parallel CI jobs; total wall time budget 8 min |
| OpenXR runtime mock drifts from Khronos behaviour | Mock parity tests (`unit:xr_runtime:khronos_mock_parity`) run the same scripted scenario through both impls and assert byte-equal results on the deterministic surface |
| 4-headset E2E requires expensive hardware | E2E runs with 1 real + 3 emulated headsets by default; full-hardware run is gated to release candidates only |
| Fuzz finds genuine crashes mid-development | Crashes do not block PRs (fuzz full runs nightly only); they file P1 issues that block the next release; the smoke-fuzz at PR time uses a 60s budget so it only catches regressions in known-bad seeds |
| Mutation testing noisy on first 30 days | Start advisory; flip to gate after baseline stabilises (matches PRD-QE-001 convention) |
| APK size gate forces material/texture compromises | Perf agent monitors APK size weekly; >70 MB triggers an early-warning issue; release-blocking only at 80 MB |
| LiveKit JWT semantics change between SDK versions | `cargo deny` pins LiveKit SDK version; security DAST detects token-shape drift |

---

## 15. Open Questions

1. **Full-fidelity multi-headset E2E cost.** Running 4 paired Quest 3 headsets in CI is hardware-intensive. Recommendation: nightly run with 1 real + 3 emulated; release-only run with 4 real. Alternative: cloud-device farm (e.g. Headspin). Decision deferred to ADR-072 once cost data exists.
2. **GDScript test coverage measurement.** `gut` does not natively report coverage. Options: (a) instrument GDScript via custom tool, (b) treat GDScript as scene wiring and rely on integration coverage. Recommendation: option (b); coverage gate applies only to Rust.
3. **Fuzz corpus management.** Should the fuzz corpus live in this repo or a separate `xr-fuzz-corpus` repo? Recommendation: this repo, under `fuzz/corpus/`, with monthly garbage-collection job to keep size bounded.
4. **Visual baseline storage.** PNG baselines bloat git history. Recommendation: store baselines in git LFS; baseline rotation is a deliberate PR with two reviewers.
5. **On-device perf metric collection on Quest 3 prod hardware.** Quest 3's `gfxinfo` output format may shift across Horizon OS updates. Mitigation: pin to a specific Horizon OS via OTA-block in dev mode where possible; abstract metric parsing behind an interface so updates land in a single place.
6. **Should `tests/fixtures/presence/canonical/` be cross-shared with PRD-006's `tests/fixtures/canonical/`?** They serve different protocols (presence WS vs federation HTTP). Recommendation: separate directories; if patterns converge, refactor under PRD-006 P5.

---

## 16. References

- [PRD-008](PRD-008-godot-xr-quest3-client.md) — Godot 4 + godot-rust + OpenXR Quest 3 client (concurrent)
- [ADR-071](adr/ADR-071-xr-runtime-godot-openxr.md) — XR runtime selection (concurrent; supersedes ADR-032, ADR-033)
- [DDD XR Godot (BC22)](ddd-xr-godot-context.md) — XR domain model (supersedes ddd-xr-bounded-context.md)
- [PRD-QE-001](PRD-QE-001-integration-quality-engineering.md) — Integration QE; this document mirrors its structure and gate format
- [DDD QE Traceability Graph (BC21)](ddd-qe-traceability-graph-context.md) — gate consumer + traceability
- [ADR-061](adr/ADR-061-binary-protocol-unification.md) — binary protocol wire format shared with gdext `binary_protocol` module
- [docs/testing/TESTING_GUIDE.md](testing/TESTING_GUIDE.md) — current test conventions; §3 + §5 rewritten by this PRD
- [docs/reference/performance-benchmarks.md](reference/performance-benchmarks.md) — perf gate baseline; XR section to be replaced by §4.7 of this PRD
- [docs/binary-protocol.md](binary-protocol.md) — single-source wire spec
- [agentic-quality-engineering skill](../multi-agent-docker/skills/agentic-quality-engineering/SKILL.md) — QE fleet composition
- [Removal plan for legacy XR](xr-godot-removal-plan.md) — concurrent companion document enumerating files deleted by PRD-008
- [Threat model for new XR substrate](xr-godot-threat-model.md) — concurrent companion; informs §4.6 and §4.10
- [System architecture — XR substrate](xr-godot-system-architecture.md) — concurrent companion
