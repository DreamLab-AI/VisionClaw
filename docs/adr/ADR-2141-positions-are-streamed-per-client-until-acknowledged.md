---
id: ADR-2141
title: Positions are streamed to each client in chunks until it has acknowledged them
date: 2026-10-13
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: [ADR-2139]
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: any change to how positions are encoded, chunked, acknowledged or entitled; any client frame decoder; any setting under physics.stream*
repo: visionclaw
---

# ADR-2141 — Positions are streamed to each client in chunks until it has acknowledged them

## Supersedes
ADR-2139 decision 1 ("every position frame is a complete snapshot of every node the session is entitled to"), 5 ("the broadcast layer decides only *when*") and 9 (the interaction broadcast rate). Everything else in ADR-2139 stands: **no deltas**, entitlement applied identically on every path, fail-closed visibility policy, class flags on every frame, and the rule that a non-finite position is a counted fault.

## Context
- Measured on the live ~9.5k-node graph: positions reached clients through a chain of rate limiters (physics-step limiter with an idle and an interaction rate, a global token bucket refilled *by time*, a 60 FPS orchestrator relay, an adaptive coordinator interval, a 1 MB/s bandwidth cap, a periodic cache flush, force and initial broadcasts, and a connect-time snapshot). Result: 8.4 Hz when nothing moved, 10.9 Hz during a drag, ~490 KB to every client whether or not anything had changed.
- The acknowledgement the design relied on never worked. The web client wrapped its ack in the six-byte generic message header the server does not parse, so the server read garbage for the sequence number; XR sent no acks at all. The token bucket refilled by time, which hid this.
- A client that has the latest positions has nothing to be sent, and a client that is behind should be sent exactly what it is missing, at its own pace.

## Decision
1. **Delivery is per client and acknowledgement-driven.** For each client the server remembers what that client last *acknowledged* for every node: its position and the analytics on its record (cluster, community, anomaly, centrality).
2. **A node is a candidate** for a client when it is entitled to that client and (a) it has never been acknowledged, or (b) its position is more than `streamMoveThreshold` from the acknowledged one, or (c) its analytics differ. Comparing with the *acknowledged* state, not the previous frame, means slow drift accumulates until it crosses the threshold and is sent.
3. **A chunk** is at most `streamChunkNodes` candidates, never-sent first (in id order), then by displacement descending, then analytics-only changes. It carries absolute positions, is built fresh from current state with a new sequence number, and is never resent as old bytes. There are no deltas.
4. **One chunk is in flight per client.** The next is built when the client acknowledges the chunk's sequence, or after `streamAckTimeoutMs` without an acknowledgement. Delivery therefore runs at each client's own round trip, and stops entirely once every client is current. A new or reconnecting client has acknowledged nothing, so its initial sync is the same mechanism; there is no connect-time position frame.
5. **The acknowledgement is `[0x34][u64 sequence][u32 nodes][u64 timestamp_ms]`, little-endian, 21 bytes, no generic header**, sent by a client **after it has applied** the chunk. A chunk that was not applied (stale, duplicate, worker failure) is not acknowledged: the server rebuilds it after the timeout rather than marking its nodes as held. The wire fixture (`crates/visionclaw-protocol/src/wire_fixtures.rs`) defines the bytes; the server, the web client and the XR client each assert the same golden frame.
6. **Clients apply chunks in arrival order and drop none.** The web client's newest-wins coalescing (store dispatcher and worker proxy) is replaced by an ordered queue; the XR client's resync-awaiting-a-delta state is gone. A V4 delta frame is refused by every decoder.
7. **Liveness is ping/pong only.** Binary silence is the normal state of a current client; the web client's binary-silence watchdog (which re-sent a subscribe and recycled the connection) is removed.
8. **Three stored, validated settings replace the two broadcast rates**: `physics.streamChunkNodes` (100–20000, default 2500), `physics.streamMoveThreshold` (0–100, default 0.05), `physics.streamAckTimeoutMs` (20–10000, default 500). Defaults are measured with an acknowledging client (per-node update interval p50/p95): threshold 0.25 gave 318/1017 ms (visible stepping), 0.05 gave 81/204 ms at ~4.7 MB/s per client, 0.01 gave 69/80 ms at ~7 MB/s. The first default (0.25) was reasoned rather than measured and was wrong.
9. **Interaction needs no special rate.** A grab pins the node on the GPU; the springs move the neighbours; those positions move beyond the threshold and are streamed as fast as each client acknowledges. There is no idle/interaction mode.

## Removed (no compatibility shims)
`BroadcastOptimizer`, `NetworkBackpressure`/`TokenBucket`, `PositionBroadcastAck`, `ConfigureBroadcastOptimization`, `GetBroadcastStats`, `ForceFullBroadcast`, the orchestrator's position relay and its second pin store, `BroadcastPositions`, `BroadcastNodePositions`, `ForcePositionBroadcast`, `InitialClientSync`, `SetGpuComputeAddress`, `SetBandwidthLimit`, `SendPositionUpdate`, `BroadcastPositionUpdate`; the JSON messages `subscribe_position_updates`, `requestPositionUpdates`, `request_full_snapshot`, `requestBotsGraph`, `requestBotsPositions`, `enableRandomization`, `update_physics_params`; the settings `broadcastFps` and `interactionBroadcastFps`.

## Consequences
- Idle cost falls to the periodic re-delivery of slowly drifting nodes (~140 KB/s per client on the current layout) and to zero when the layout is still.
- A slow client cannot slow another: its in-flight chunk and its acknowledged state are its own.
- A client that never acknowledges costs the server one chunk per `streamAckTimeoutMs`, and is closed by the existing stalled-client rule once its mailbox stays full.
- Memory: per client, one map entry per entitled node (~9.5k).
- **Deploy order**: clients that acknowledge (XR, then web) before the backend that waits for acknowledgements.

## Verification
- `position_stream` unit tests (initial sync in chunks, quiet when current, threshold against acknowledged state, analytics-only changes, ordering, timeout, late ack, entitlement, reconnect).
- Coordinator protocol tests: chunks follow acknowledgements, timeout resend with a larger sequence, acknowledgements are per client, entitlement, fail closed, full mailbox.
- Wire: the shared ack fixture decodes on the server; the web and XR encoders produce the same golden bytes.
- Clients: web `positionAck.test.ts` (acknowledged by own sequence, after apply, not on failure), ordered-queue tests, `binary-processor.test.ts` (apply replaces, idempotent); XR `admit_and_ack` tests and the rewritten freshness tests.
- Integration: `ws_stalled_client_test` (a never-reading client is closed in under 3 s).

## Amendment — 2026-10-19

First verification, at `b4a175412`. Every removal, wire byte, setting and test named above was opened and found as described, with three behaviours added after the record was written (commit `f82a92c48`, "Codex review of ADR-2141") that decisions 3 and 4 do not state. Governing doc updated in the same change: `docs/PROTOCOL-registry.md` invariant 6 and `docs/reference/physics-parameters.md` (stream settings table).

**Added behaviour (the record's ordering and timing rules are now qualified):**
1. **Fair share in a chunk (decision 3).** When there are more candidates than `streamChunkNodes`, a quarter of the chunk (`FAIR_SHARE_DIVISOR = 4`, `src/actors/position_stream.rs:66`) is reserved for the longest-waiting candidates, the rest goes to the most displaced (`plan`, `:125-182`). Without it, small movers starve behind a constantly moving set. Never-sent nodes remain first (infinite displacement, age key 0).
2. **Late acknowledgements are honoured (decision 4).** The last eight sent chunks stay on an in-flight list (`IN_FLIGHT_HISTORY = 8`, `:64`); an acknowledgement for a replaced chunk still commits it and never moves a node's held state backwards (`acknowledge`, `:225-242`). Without this a client slower than the timeout never advances.
3. **Bounded backoff (decision 4).** Each consecutive timeout doubles the wait, up to 8x (`MAX_BACKOFF_DOUBLINGS = 3`, `:68`, `effective_timeout` `:99`); any acknowledgement resets it. So `streamAckTimeoutMs` is the base wait, not a fixed one.

**Confirmed against code:** one chunk in flight per client (`is_waiting`, `:104`; `pump_one` `client_coordinator_actor.rs:442-454`); candidates are compared with acknowledged state (`position_stream.rs:139-159`); ack decode `[0x34]` + 20 bytes, no generic header (`src/utils/binary_protocol.rs:1853`, `:1866`; `crates/visionclaw-wire/src/lib.rs:92-95`, `:335`); web ack encoder `client/src/services/BinaryWebSocketProtocol.ts:245`; XR `OPCODE_BROADCAST_ACK` and `admit_and_ack` (`xr-client/rust/src/binary_protocol.rs:510`, `:701`); defaults 2500 / 0.05 / 500 (`crates/visionclaw-domain/src/types/physics_config.rs:24`, `:37`, `:46`) with the range for the chunk size at `src/actors/gpu/physics_bounds.rs:96`; every item under "Removed" is absent from the tree (no `BroadcastOptimizer`, `TokenBucket`, `InitialClientSync`, `request_full_snapshot` and so on; `broadcastFps` survives only as a legacy key in an old persisted blob, which still deserialises and is ignored, test at `physics_config.rs:688-715`). Tests exist: `position_stream` unit tests, the coordinator tests (`client_coordinator_actor.rs:2294`, `:2347`, `:2399`), `positionAck.test.ts`, `binary-processor.test.ts`, `tests/ws_stalled_client_test.rs`.

**Two residues, recorded rather than hidden.**
- Decision 6 says clients "drop none". The web dispatcher keeps arrival order but bounds its queue at 256 frames and drops the oldest on overflow (`client/src/store/websocket/binaryFrameDispatcher.ts:28`, `:72-76`); a dropped chunk is unacknowledged, so the server rebuilds it after the timeout. That is consistent with decision 5 but is not "none".
- The `enableRandomization` JSON message listed under "Removed" survives as an unused variant of the client-facing `Message` enum (`crates/visionclaw-protocol/src/socket_flow_messages.rs:165`). No handler or sender references it. Delete the variant.
