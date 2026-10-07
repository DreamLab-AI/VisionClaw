---
id: ADR-2134
title: The graph socket relays the memory explorer's beat clock and route to the same user's other sessions
date: 2026-10-07
decision_status: proposed
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: b6fbe772d332f2afd9f4d6817eb7655d031eeb91
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
- **Two client text frames, relayed same-user only.** `beatClock` `{bpm, phaseAt, confidence, source, sentAt?}` and `memoryRoute` `{snapshotId, nodeIds, positions?}` sent on `/wss` are delivered to every *other* session whose coordinator pubkey equals the sender's, and to no other session (`ClientManager::relay_text_to_pubkey`). A sender with no authenticated pubkey gets one `error` frame and nothing is relayed.
- **Validated and re-serialised.** `bpm` ∈ [40, 220], `confidence` ∈ [0, 1], `phaseAt` ≥ 0 and finite, `source` ∈ {off, file, tap, spotify}; `snapshotId` and each node id 1–128 chars, at most 512 nodes, `positions` exactly 3·n finite values with |v| ≤ 10⁴, raw frame ≤ 96 KiB. The relayed frame is rebuilt from the validated fields only and stamped with `serverTime`; unknown keys never cross to another session.
- **Server clock.** When `sentAt` (the sender's clock) is present the relay rebases `phaseAt` onto the server clock (`phaseAt − sentAt + serverNow`). The JSON pong carries `serverTime`, so a receiver sets its offset from the round trip as `serverTime − (sent + rtt/2)`. The XR client keeps the minimum-RTT sample of the last eight.
- **Rate.** Per session and per kind, at most one relay every 250 ms (4 Hz). The newest held frame is flushed at the end of the interval, so the final state always lands. The desktop sends beat changes plus a 2 s heartbeat while on, a single `off` frame when it stops, and the live route every 10 s for a late-joining headset. The headset drops a relayed clock after 6.5 s of silence.
- **Handshake identity reaches the coordinator.** `SetClientId` forwards an upgrade-time pubkey as `AuthenticateClient`.

## Consequences
- The headset pulses node halos, edge flow and memory bursts to the desktop's beat, scaled to ≤ 0.25 under reduced motion (the comfort default). It can draw the desktop's route once the XR memory-cloud layer consumes `memoryRoute`; the hook is `on_memory_route` on the `xr_memory_cloud` group.
- Under `VISIONCLAW_DEV_MODE` every authenticated session shares the dev pubkey, so the relay reaches every dev session. That is accepted for LAN dev and impossible in release, which compiles the bypass out.
- The relay carries timing and memory-entry ids, never audio. Microphone beat detection (WP8) runs only on the headset, is opt-in and is never sent.
- Cost: one validation and one coordinator message per relayed frame, at ≤ 4 Hz per session.

## Verification
At `verified_commit`:
- `cargo test --lib -- session_relay relay_reaches_only client_coordinator`: 11 passed, covering validation bounds, rebasing, the frame cap, the 4 Hz trailing throttle, and same-pubkey-only delivery with sender exclusion through probe actors.
- `vitest run …/memoryCloud/__tests__/xrRelay.test.ts`: 6 passed (off silence, heartbeat, single off frame, throttle, route ids/positions, clear, cap).
- `cargo test -p visionclaw-xr-gdext`: beat, ping/pong and offset parsing under asymmetric round trips; heartbeat continuity; staleness.
- GUT `test_beat_pulse.gd` on HP Godot 4.6.1 drives frames through `_on_graph_text`.

Still unverified: a desktop and headset filmed side by side (phase error < 30 ms target), so `activation_status` stays `staged`.
