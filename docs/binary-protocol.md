---
title: The Binary Protocol
description: Single-source spec for the GPU position stream wire format
category: reference
tags: [websocket, binary, protocol, real-time]
updated-date: 2026-04-30
---

# The Binary Protocol

Single per-physics-tick wire format used by the GPU position stream. There
is one binary protocol; there are no versions.

> Authoritative spec: [ADR-061](adr/ADR-061-binary-protocol-unification.md).
> Domain model: [ddd-binary-protocol-context.md](ddd-binary-protocol-context.md).
> Decision instance: [PRD-007](PRD-007-binary-protocol-unification.md).

## Frame layout

```
[u8  preamble = 0x42]            <- fixed sanity byte, NOT a version dispatch
[u64 broadcast_sequence_LE]
[N × Node]
```

## Node layout (28 bytes, fixed)

```
[u32 id_LE]
[f32 x_LE][f32 y_LE][f32 z_LE]
[f32 vx_LE][f32 vy_LE][f32 vz_LE]
```

## Cadence

- **Position stream**: every physics tick (≤60 Hz). Carries pos+vel only.
- **Analytics stream**: separate `analytics_update` JSON message at recompute
  cadence (~0.1–1 Hz). Carries `cluster_id`, `community_id`, `anomaly_score`,
  `sssp_distance`, `sssp_parent`.

## Backpressure

Server emits a monotonic `broadcast_sequence`; client acks via
`ClientBroadcastAck` to replenish a token bucket on the
`ForceComputeActor`. See [ADR-031](adr/ADR-031-broadcast-backpressure.md).

## Privacy

Visibility is enforced at the broadcast boundary
(`ClientCoordinator::broadcast_with_filter`); positions for nodes the
caller cannot see are dropped from the frame. The wire id is the raw
u32 — no flag bits. See ADR-050 §H2 (post-ADR-061 prose).

## Forbidden patterns (regression guards)

- Adding columns to per-frame binary without an ADR superseding ADR-061.
- Encoding session-static state in id flag bits.
- Emitting `analytics_update` from a per-frame loop.
- Versioning the binary protocol — any future evolution gets a new endpoint.

## Implementation

- **Server**: `src/utils/binary_protocol.rs::encode_position_frame`
- **Client**: `client/src/store/websocket/binaryProtocol.ts::parsePositionFrame`
