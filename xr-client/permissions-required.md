# Android Permissions — VisionClaw XR

The Quest 3 APK requests the following permissions. Each is justified by the
feature in `docs/PRD-008-xr-godot-replacement.md` it supports; removal of any
breaks the corresponding feature.

## Meta Quest specific

| Permission | Why | PRD-008 ref |
|---|---|---|
| `com.oculus.permission.HAND_TRACKING` | `XR_EXT_hand_tracking` extension required to drive `XrInteraction` ray cast and pinch detection from bare hands | §5.4 hand tracking row |
| `com.oculus.permission.USE_SCENE` | `XR_FB_scene` + `XR_FB_scene_capture` for room-mesh occlusion of graph behind real walls | §5.4 scene mesh row |
| `com.oculus.permission.USE_ANCHOR_API` | `XR_FB_spatial_entity` + `XR_FB_spatial_entity_storage` for graph-origin anchor persistence across sessions | §5.4 spatial anchors row |

## Standard Android

| Permission | Why | PRD-008 ref |
|---|---|---|
| `android.permission.INTERNET` | TLS WebSocket to `/wss` (graph) + `/ws/presence` and HTTPS to LiveKit token endpoint | §5.2 (binary protocol) + §5.5 (voice) |
| `android.permission.RECORD_AUDIO` | (1) LiveKit microphone capture for spatial voice. (2) **Beat sync, opt-in** (ADR-2134 / WP8): when the wearer turns on the Session-tab Mic toggle, the headset listens for the music's tempo so the graph pulses on the beat. Requested at that press, never at start-up | §5.5 voice routing; ADR-2134 |
| `android.permission.MODIFY_AUDIO_SETTINGS` | LiveKit AAR sets the audio mode for VoIP routing | §5.5 |
| `android.permission.ACCESS_NETWORK_STATE` | Reconnect logic must observe network transitions to avoid burning battery on doomed retries | §11.1 |
| `android.permission.ACCESS_WIFI_STATE` | Same as above | §11.1 |
| `android.permission.WAKE_LOCK` | Maintain CPU during XR session — Horizon OS otherwise idles the CPU during periods where only the GPU is busy | G1 (90 fps) |
| `android.permission.VIBRATE` | Haptic pulses on hand pinch / grab confirmation | §5.4 hand interaction row |

## Notes

- **Microphone for beat sync (ADR-2134).**
  - It is off by default. It needs a deliberate press of the Session-tab Mic toggle, and the runtime permission is requested only then. A denial leaves it off and says so on the HUD.
  - While the mic is listening, a red `● MIC` badge shows in the HUD header on every tab.
  - Captured audio goes to a muted capture bus, so it is never played back. It is held only as a rolling 8 s analysis window in memory, reduced to an onset envelope and a tempo estimate, and discarded. It is never recorded, written to storage or transmitted. Only the derived tempo (bpm, phase, confidence) drives the local pulse, and the mic clock is not relayed to other sessions.
  - A tempo may drive the pulse only after two consecutive estimates agree at high confidence (≥ 0.6), so the wearer's speech does not lock a false beat.
  - Turning the toggle off drops the buffered samples at once.

- **No camera permission.** Passthrough is rendered by the OpenXR runtime; the
  app does not see camera frames.
- **No location permission.** Spatial anchors are device-local and do not
  derive from GPS.
- **Scene mesh data stays on device** — see `docs/xr-godot-threat-model.md` A7.
