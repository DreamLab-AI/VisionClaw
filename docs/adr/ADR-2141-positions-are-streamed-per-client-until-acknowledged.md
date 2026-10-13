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
8. **Three stored, validated settings replace the two broadcast rates**: `physics.streamChunkNodes` (100–20000, default 2500), `physics.streamMoveThreshold` (0–100, default 0.25), `physics.streamAckTimeoutMs` (20–10000, default 500). Defaults are measured: settled per-node motion is ~0.008 units per frame and the graph spans ~1,400 units, so 0.25 is a third of a pixel at full-graph zoom.
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
