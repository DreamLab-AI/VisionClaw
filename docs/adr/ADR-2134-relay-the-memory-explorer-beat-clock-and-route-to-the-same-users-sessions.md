---
id: ADR-2134
title: The graph socket relays the memory explorer's beat clock and route to the same user's other sessions
date: 2026-10-07
decision_status: proposed
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: aa01c5536efd6d65c72dcc644e21472bfd2e223c
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

## Re-verification — 2026-10-07 (089f196d6)

The position-stream fix reformatted `relay_text_to_pubkey` with rustfmt only: its behaviour, its pubkey scoping, the sender exclusion and `relay_reaches_only_the_same_pubkeys_other_sessions` are unchanged. The decision holds.

## Amendment — 2026-10-07 (35bd7c6bc): optional sidecar agreement counts on `memoryRoute`

The merge of `feat/xr-cloud-parity` (48e1bf327) widens the `memoryRoute` wire contract. The Decision's field list above is left as accepted; this amendment adds to it.

**Fields.** `memoryRoute` may carry `sidecarTotal` (every sidecar hit, sampled or not) and `sidecarAgree` (the sampled hits that are also in the desktop's local top-k). The desktop takes both from the panel's `sidecarAgreement` (`xrRelay.ts:113-117`). It sends them only for a finished route with a non-empty path, and only when the agreement's `inSample` equals the `sidecar` rows actually sent, so the counts never describe rows cut by the 64-row cap (`:115`). Without a sidecar response both are omitted. The sidecar's method, timings and hit objects stay in the HTTP query response and are never relayed (`xrRelay.ts:18-24`).

**Validation.** The counts decorate the frame and never decide whether it relays. `validate_memory_route` (`session_relay.rs:158`) reads each count with the same whole-number rule as a row (`as_row`, `:186`). It re-serialises the counts only as a pair satisfying `sidecar.len() ≤ sidecarTotal` and `sidecarAgree ≤ sidecar.len()`, measured on the filtered, capped `sidecar` (`:187-192`, `:203-206`). Otherwise both counts are dropped and the frame still relays. That covers one count alone, a negative, fractional or string value, total < sampled, or agree > sampled. The headset applies the identical rule in `SidecarStats::from_wire` (`memory_route.rs:249-252`). Four new cases in `memory_route_cases.json` hold the server and the headset to the same verdict. The largest valid route with both counts at `u32::MAX` still fits the 16 KiB frame cap.

**Backward compatibility.** Both counts are optional and additive:
- An older desktop sends no counts. The headset then shows only the sampled marks.
- An older server rebuilds frames from its validated fields, so it drops the counts. The headset again shows only the sampled marks.
- An older headset deserialises into a `WireRoute` without `deny_unknown_fields` (`git show 089f196d6:xr-client/rust/src/memory_route.rs:156`), so it ignores the counts.

No frame that was valid before is rejected now. `memory_route::WIRE_FIELDS` (`memory_route.rs:117-118`) lists the nine field names, and a Rust test checks them against `xrRelay.ts` `MemoryRouteFrame` (`:1763-1765`). PROTOCOL-registry 0.1.6 records the fields.

**Verification at 35bd7c6bc.**
- `session_relay.rs` tests: the 8 validator tests pass, including `memory_route_matches_the_headset_parser_rules` (pair cases) and `relay_agrees_with_the_headset_parser_on_the_shared_cases`. They were compiled in isolation (lines 1–267 plus the test module, `serde_json` only), because the root crate's `visionclaw-gpu` build script fails in this container: no `cuda_runtime.h`, and the fallback `semantic_forces` PTX has no `.entry`.
- `relay_reaches_only_the_same_pubkeys_other_sessions` (`client_coordinator_actor.rs`, unchanged) was not re-run.
- `xr-client/rust` `cargo test --offline` passes 384 library + 118 integration tests.
- `vitest run …/memoryCloud/__tests__/xrRelay.test.ts` passes 7.

Same-pubkey scoping, the throttle, rebasing and the frame cap are unchanged. `activation_status` stays `staged`.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `crates/visionclaw-protocol/src/socket_flow_messages.rs`: rustfmt only; `src/actors/client_coordinator_actor.rs`: the V3 encoder call passes its five class-id sets as one `NodeClassIds` (same sets, same bytes); struct-literal `ClientFilter` in tests; `src/handlers/socket_flow_handler/session_relay.rs`: rustfmt only; `xr-client/rust/src/beat.rs`: the NaN-rejecting `!(x > 0.0)` guards are written as explicit `is_nan() ||` checks (same truth table); `xr-client/rust/src/pulse.rs`: a module-level allow for gdext's generated `CallError` closures. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record); `cargo test --workspace` in `xr-client/rust`: 495 passed, 0 failed, before and after. **Still holds.**

## Re-verification — 2026-10-07 (aa01c5536)

This stamp covers the merge of `chore/clippy-sweep` (299aa35bc) with `feat/xr-cloud-parity`, and the lint follow-up aa01c5536. Each branch re-verified this record against its own changes (sections above). The merge itself kept both sides; the only code it combined was test code in `client_coordinator_actor.rs`. aa01c5536 is mechanical: rustfmt, an `async-trait` patch bump, `as_chunks`, and test checks made `const`. No wire format, tag byte, settings key, pose owner, crate boundary or relay rule changed. Decision holds. Verified with `cargo test --workspace --tests` (3,244 passed), xr-client `cargo test --workspace` (516 passed) and clippy `-D warnings` clean in both.
