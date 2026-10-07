---
id: ADR-2134
title: The graph socket relays the memory explorer's beat clock and route to the same user's other sessions
date: 2026-10-07
decision_status: proposed
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 697df7350f474b3dfbba4eed1b3bd976dd35b061
verified_paths: [src/handlers/socket_flow_handler/session_relay.rs, src/handlers/socket_flow_handler/message_routing.rs, src/actors/client_coordinator_actor.rs, crates/visionclaw-protocol/src/socket_flow_messages.rs, client/src/features/visualisation/memoryCloud/xrRelay.ts, xr-client/rust/src/beat.rs, xr-client/rust/src/pulse.rs, xr-client/scripts/beat_pulse.gd]
owner: jjohare
review_trigger: a second consumer of beatClock or memoryRoute; any request to relay across users or rooms; a headset receipt showing desktop/headset phase error above 30 ms; a change to RECORD_AUDIO policy
repo: visionclaw
domain: PROTOCOL-registry
---

# ADR-2134 — The graph socket relays the memory explorer's beat clock and route to the same user's other sessions

## Context
The desktop memory explorer (ADR-2133) keeps a beat clock `{bpm, phaseAt, confidence, source}` (`memoryCloud/beatClock.ts`) and a query route through the memory cloud. The Godot XR client should pulse to the same beat and draw the same route while the user wears the headset. Both clients already hold an authenticated `/wss` graph socket. Before this record there was no client→client path on it, and no way for a client to learn the server clock: the JSON pong only echoed the client's own timestamp. A session authenticated at the upgrade (NIP-98 header) also never reported its pubkey to the `ClientCoordinatorActor`, so the coordinator could not scope anything to it.

## Decision
- **Two client text frames, relayed same-user only.** `beatClock` `{bpm, phaseAt, confidence, source, sentAt?}` and `memoryRoute` `{snapshotId, seq, sentAt, path, sidecar, query}` sent on `/wss` are delivered to every *other* session whose coordinator pubkey equals the sender's, and to no other session (`ClientManager::relay_text_to_pubkey`). A sender with no authenticated pubkey gets one `error` frame and nothing is relayed.
- **Validated and re-serialised.** `bpm` ∈ [40, 220], `confidence` ∈ [0, 1], `phaseAt` ≥ 0 and finite, `source` ∈ {off, file, tap, spotify}; For `memoryRoute` the rules are those of the headset parser (`xr-client/rust/src/memory_route.rs`, xr-cloud): a non-blank `snapshotId` (≤ 128 chars); `path` with ≤ 64 snapshot row indices, root → answer, where an empty path clears the route and any bad entry rejects the frame; `sidecar` with bad entries dropped and ≤ 64 kept; `query` cut to 120 chars; `seq` and `sentAt` finite and ≥ 0. Rows rather than ids or geometry keep the frame small, and the headset resolves them against its own copy of the snapshot. Both suites read `xr-client/rust/tests/fixtures/memory_route_cases.json`. The raw frame is at most 16 KiB. The relayed frame is rebuilt from the validated fields only and stamped with `serverTime`; unknown keys never cross to another session.
- **Server clock.** When `sentAt` (the sender's clock) is present the relay rebases `phaseAt` onto the server clock (`phaseAt − sentAt + serverNow`). The JSON pong carries `serverTime`, so a receiver sets its offset from the round trip as `serverTime − (sent + rtt/2)`. The XR client keeps the minimum-RTT sample of the last eight.
- **Rate.** Per session and per kind, at most one relay every 250 ms (4 Hz). The newest held frame is flushed at the end of the interval, so the final state always lands. The desktop sends beat changes plus a 2 s heartbeat while on, a single `off` frame when it stops, and the live route every 10 s for a late-joining headset. Every route frame carries a fresh `seq` and `sentAt`, because the headset orders by (`sentAt`, `seq`). The headset drops a relayed clock after 6.5 s of silence.
- **Handshake identity reaches the coordinator.** `SetClientId` forwards an upgrade-time pubkey as `AuthenticateClient`.

## Consequences
- The headset pulses node halos, edge flow and memory bursts to the desktop's beat. Under reduced motion (the comfort default) nothing pulses (ADR-2107): halos, edges and bursts keep steady brightness and only the HUD Beat readout shows the tempo. xr-cloud's memory-cloud layer draws the relayed route and reads the beat from `BeatPulse` (`is_locked`, `beat_phase`, `beat_pulse`) through `beat_source`. While the cloud is shown, a `memory_flash` restyles the cloud sprites it names through `set_row_emphasis` and adds no geometry, because the scene is at the 100k-triangle budget. When no cloud is shown, it draws pooled rings on a stand-in point around the graph.
- Under `VISIONCLAW_DEV_MODE` every authenticated session shares the dev pubkey, so the relay reaches every dev session. That is accepted for LAN dev and impossible in release, which compiles the bypass out.
- The relay carries timing and memory-entry ids, never audio. Microphone beat detection (WP8) runs only on the headset, is opt-in and is never sent.
- Cost: one validation and one coordinator message per relayed frame, at ≤ 4 Hz per session.

## Verification
At `verified_commit`:
- `cargo test --lib -- session_relay relay_reaches_only`: 9 passed, covering validation bounds, rebasing, the frame cap, the 4 Hz trailing throttle, agreement with the headset parser on the shared cases, and same-pubkey-only delivery with sender exclusion through probe actors.
- `cargo test -p visionclaw-xr-gdext --test memory_route_relay_parity`: the headset half of the shared cases.
- `vitest run …/memoryCloud/__tests__/xrRelay.test.ts`: 6 passed (off silence, heartbeat, single off frame, throttle, route rows/sidecar/query with seq and sentAt, repeat, clear, cap).
- `cargo test -p visionclaw-xr-gdext`: beat, ping/pong and offset parsing under asymmetric round trips; heartbeat continuity; staleness.
- GUT `test_beat_pulse.gd` on HP Godot 4.6.1 drives frames through `_on_graph_text`.

Still unverified: a desktop and headset filmed side by side (phase error < 30 ms target), so `activation_status` stays `staged`.

## Re-verification — 2026-10-07 at 944cba88c (xr-graph halo quads and edge LOD merged)

The merge of `feat/xr-graph` (944cba88c) brings in xr-graph's halo quad layer (`NodesHaloMulti`, `node_halo_quad.gdshader`), its edge LOD (near cylinders plus far camera-facing ribbons sharing `edge_flow_common.gdshaderinc`) and the avatar quaternion slerp. Suite on the merged tree: `cargo test -p visionclaw-xr-gdext` passes 344 library + 111 integration tests; GUT on HP Godot 4.6.1 (`--xr-mode off`) runs 161 tests, 158 passing and 3 GL-only tests pending headless.

`beat_pulse.gd` now writes `beat_pulse` to the live `NodesHaloMulti`, `EdgesMulti` and `EdgesRibbonMulti` materials, plus any remaining sphere-shell `next_pass`. The GUT test asserts that each live layer declares the uniform and carries the pulse, and it was red before this change. **Decision unaffected.**
