---
id: ADR-2139
title: Position frames are full snapshots of the entitled set — never deltas, never partial
date: 2026-10-13
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: [ADR-2141]
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: any change to a position encoder, the broadcast path, a client frame decoder, or the visibility filter
repo: visionclaw
---

# ADR-2139 — Position frames are full snapshots of the entitled set — never deltas, never partial

> **Superseded by [ADR-2141](ADR-2141-positions-are-streamed-per-client-until-acknowledged.md)** for decisions 1, 5 and 9: delivery is now per-client chunks of what each client has not yet acknowledged, not periodic full snapshots. The no-delta rule, entitlement, fail-closed policy and class flags below still stand.

## Context
- BROADCAST-001 / PRD-007 §3 / ADR-061 prohibit delta position updates, but the prohibition lived in a comment while the machinery stayed reachable: the web client decoded V4 delta frames and added them to target positions; the XR client carried a Full/Delta frame classifier; the server kept a spatial-culling subset path and a `delta_threshold` wire field.
- Measured on the live stream (2026-10-13, 9,478 nodes): steady state is V5 full snapshots at ~8 Hz, but the connect-time sync sent a 3,000-node frame (quality top-N reused from the topology limit) and a 9,470-node frame, and the V5 stream sent all 9,478 nodes to a session the connect sync had withheld 8 private nodes from.
- The live V5 frames carried no class-flag bits because the coordinator's node classification was never delivered (`UpdateNodeTypeArrays` had no sender).

## Decision
1. **Every position frame is a complete snapshot of every node the receiving session is entitled to see.** The set is decided by (a) entitlement — the ADR-060 visibility drop-set, applied per session — and (b) a filter the client explicitly requested. It is never decided by whether a node moved, by camera position, by a quality cut the client did not ask for, or by recency.
2. **There is no delta encoding.** Tag `0x04` (the retired V4 delta frame) stays reserved in `PROTOCOL-registry.md`. No encoder exists; every decoder (web, XR) refuses it explicitly and applies nothing.
3. **The connect-time position frame is the same set as the live stream.** `initialNodeLimit` bounds only the JSON topology payload.
4. **Entitlement is applied identically on every path** — connect sync, snapshot request and the live broadcast — through the one domain function `visibility_filter::compute_private_opaque_ids`.
5. **The broadcast layer decides only *when*** (`broadcastFps`), never *which nodes*: spatial culling, `delta_threshold` and bandwidth-reduction statistics are removed.
6. **A non-finite position is a physics fault.** The snapshot builder counts omitted nodes and the actor logs an error naming the shortfall; an incomplete frame is never passed off as complete.
7. **Class-flag bits are set on every V5 frame**: the classification is pushed to the coordinator at wiring, at GPU initialisation and, immediately (not after the debounce), on every graph update.
8. **Entitlement fails closed.** With the visibility filter on, no frame is broadcast until a real classification snapshot (`NodeTypeArrays.loaded`) has arrived: "the policy has not arrived" is never read as "nothing is private". A cross-vendor review (Codex) found that the first version failed open.

9. **Interaction goes through the GPU stream, nowhere else.** `nodeDragStart/Update/End` do exactly one thing: pin (move) the node on the GPU and wake the loop. The handlers used to also run CPU `SimulationStep`s on the graph-state copy and broadcast all of it as bare V3 to every session (measured: ~190 un-entitled 490 KB frames in 4 s, private nodes included, fighting the GPU physics) plus single-node frames. Both are removed. While pins are being updated the stream runs at the stored `interactionBroadcastFps` (default 30 Hz; the idle `broadcastFps` is 8), held for two idle intervals after the last pin update; a grab start sends the next frame immediately.

## Consequences
- Clients may treat any position frame as "replace what I have for these nodes"; the web worker and bots context lost their add-delta branches and the XR gate lost its resync-awaiting-a-delta verdict.
- Bandwidth is exactly `broadcastFps × 52 B × entitled nodes`; reducing it means lowering the rate or the entitled set, never sending less per frame.
- Anonymous sessions no longer receive private nodes' positions on the live stream (a privacy gap closed).
- Removed APIs: `UpdateCameraFrustum`, `ConfigureBroadcastOptimization.{delta_threshold,enable_spatial_culling}`, the bandwidth statistics, V4 decoding on the web, `FrameKind::Delta` on XR.

## Known limits
- Entitlement is a deny-list keyed to the last classification snapshot. A node that turns private is withheld from other sessions as soon as the update is pushed; frames already queued to a socket are not revoked.
- Shared layout: private nodes take part in the force layout, so the motion of public nodes is influenced by them. Record-level filtering does not hide that.

## Verification
- Server: `complete_snapshot_tests`, `live_broadcast_withholds_private_nodes_from_sessions_that_may_not_see_them`, `with_no_private_nodes_every_session_gets_the_whole_snapshot`, `the_optimizer_gates_whole_snapshots_and_never_selects_nodes`.
- Web: `never decodes a delta frame` in `binaryProtocol.framePolicy.test.ts`.
- XR: `a_retired_v4_delta_frame_is_refused_not_applied`, `the_first_sequenced_frame_after_a_reconnect_rebaselines_and_nothing_is_awaiting_a_delta`.
- Fail closed: `nothing_is_broadcast_until_the_visibility_policy_has_arrived`.
- Live (after redeploy): the per-frame probe must show only `0x05` frames, each with the session's full entitled node count, and non-zero class-flag bits.
