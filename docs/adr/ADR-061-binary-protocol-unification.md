# ADR-061: Binary Protocol Unification — Single Wire, No Versioning

**Status:** Accepted (2026-04-30)
**Date:** 2026-04-30
**Author:** VisionClaw platform team
**Supersedes:** ADR-037 (binary position protocol consolidation), ADR-038 §wire-format clauses
**Related:**
- PRD-007 (binary protocol unification — this ADR is the decision instance)
- ADR-031 (broadcast backpressure / `ClientBroadcastAck`)
- ADR-050 §H2 (sovereign-model privacy — replaces wire bit-29 with per-client filter)
- DDD `ddd-binary-protocol-context.md` (bounded-context model)

## TL;DR

The WS binary channel was scope-creeping at 48 bytes/node (24 of which were
sticky labels riding the 60 Hz wire). It is now **28 bytes/node, fixed,
forever**, and the term "binary protocol" replaces all `V3`/`V4`/`V5` mentions
in code, tests, and docs. Sticky GPU outputs (cluster_id, community_id,
anomaly_score, sssp_distance, sssp_parent) move to a separate
`analytics_update` message emitted on recompute completion — same producer
(GPU actors), different cadence (≈0.1–1 Hz instead of 60 Hz). Session-static
type/visibility flag bits move to the JSON init payload at
`/api/graph/data` and are no longer transmitted per frame.

## Context

ADR-037 ("literal-only, full snapshot of every node's absolute position +
velocity") was implemented as a 28 B/node wire. Over ~12 months of
incremental changes, the per-node payload doubled to 48 B as analytics
columns and type-discriminator flag bits accumulated. ADR-038 retired V4
delta encoding and consolidated on a V5 envelope wrapping V3 nodes with an
8 B broadcast sequence. The broadcast cadence is 60 Hz; the analytics
recompute cadence is seconds-to-minutes. A back-of-envelope on a 25 k-node
graph at 30 Hz steady state shows 20 redundant bytes × 25 000 × 30 Hz =
**≈18 MB/s** of wire spend on values that did not change since the
previous frame. Browser-side, every frame the decoder ran the analytics
parse loop and re-stored the same values into the same side-table indices —
wasted main-thread cycles 30 times a second.

The user directive (2026-04-30) is to stop framing this as a "versioning"
problem. **There will be one binary protocol**, and a parallel
`analytics_update` channel for GPU-produced sticky outputs.

## Decision

### D1 — Per-frame wire, fixed at 28 B/node

```
Frame:
  [u8  preamble = 0x42]            ← fixed sanity byte; not a version dispatch
  [u64 broadcast_sequence_LE]      ← unchanged from prior envelope
  [N × Node]

Node (28 bytes):
  [u32 id_LE]
  [f32 x_LE]
  [f32 y_LE]
  [f32 z_LE]
  [f32 vx_LE]
  [f32 vy_LE]
  [f32 vz_LE]
```

Implementation: single function
`encode_position_frame(positions: &[(u32, BinaryNodeData)], broadcast_sequence: u64) -> Vec<u8>`
in `src/utils/binary_protocol.rs`. Old `encode_node_data_*`,
`encode_node_data_with_*`, `encode_node_data_extended_*`,
`encode_node_data_with_live_analytics_*`, etc. are deleted.

### D2 — `analytics_update` side message

A new WebSocket message type carries sticky GPU outputs at the cadence
their producer emits them, not at the per-frame cadence:

```jsonc
{
  "type": "analytics_update",
  "source": "clustering" | "community" | "anomaly" | "sssp",
  "generation": <u64 monotonic>,
  "entries": [
    { "id": <u32>,
      "cluster_id"?: <u32>,
      "community_id"?: <u32>,
      "anomaly_score"?: <f32>,
      "sssp_distance"?: <f32>,
      "sssp_parent"?: <i32>
    }, …
  ]
}
```

Emitters: `ClusteringActor` (k-means + Louvain), `AnomalyDetectionActor`
(LOF), `SsspActor` (BFS / Bellman-Ford). Each fires only on compute-kernel
completion. Server-side rate-cap: `max 1/sec` per source, coalescing by
`generation`.

Client merges entries into `useAnalyticsStore` (Zustand). Renderers
(`ClusterHulls`, `AnomalyOverlay`, SSSP gradient mesh) read from the store
keyed by node id.

### D3 — Type/visibility move to JSON init

Bits 26-31 of the prior `id_with_flags` (agent / knowledge / private /
ontology-subtype) are removed. Node type, visibility, and ontology subtype
are session-invariant for the lifetime of the node and ride the existing
JSON `/api/graph/data` response (`NodeWithPosition.node_type`, etc.) once
at init. The client's `currentNodeTypeMap` is populated from JSON, not
from per-frame flag-bit decode.

ADR-050 §H2 (anonymous viewers) prose updates: opacification is no longer
implemented by setting bit-29 on the wire; instead,
`ClientCoordinator::broadcast_with_filter` drops positions for nodes the
caller may not see. Same end-state (anonymous viewer learns nothing
about private nodes); simpler implementation.

### D4 — Versioning vocabulary removed

**The binary protocol** is the wire's name in code, tests, and docs.
Every match for `PROTOCOL_V3`, `PROTOCOL_V4`, `PROTOCOL_V5`,
`BINARY_NODE_SIZE_V3`, `binary_protocol_v3`, "V3 frame", "V5 envelope",
etc. is removed from `src/`, `client/src/`, `docs/`, `tests/`. The
preamble byte 0x42 is a permanent sanity check — not a version field.
If the protocol ever needs to evolve, it does so via a new endpoint, not
a version byte.

### D5 — Subscription-confirmed payload trimmed

`min_interval_ms` and `requests_per_minute` (audit confirmed: zero client
consumers) are removed from the `subscription_confirmed` JSON message.

## Consequences

**Positive:**
- Wire savings: ~18 MB/s at 25 k nodes / 30 Hz steady state.
- Decoder paths: client `binaryProtocol.ts` collapses from 7 functions to 1.
- ADR-037 retired; one less ADR to track.
- Versioning matrix gone — every contributor reads one doc.
- `currentNodeTypeMap` becomes static at init, not rebuilt per frame.

**Negative:**
- Client-side renderer migration is mandatory in lockstep — no fallback
  layer means a missed renderer ships with broken visualisation.
- `analytics_update` introduces a new message type to maintain.
- Anonymous-viewer behaviour changes implementation: still hidden from
  position frames, but via filter rather than wire opacification.

**Migration risk** is the lockstep renderer migration. Mitigation: a
sprint coordinator gates merge on the renderer integration tests passing.
A pre-merge browser smoke confirms cluster hulls / anomaly overlay /
SSSP gradients all render correctly against the new store-driven path.

## Alternatives considered

1. **Keep V5, just compress sparse fields.** Rejected — adds decoder
   complexity without removing the conceptual debt of "this column rides
   the wire 60×/s and means nothing 99% of the time."
2. **Per-frame delta encoding.** Rejected — was V4, retired in ADR-037.
   Reintroducing it now solves the same problem twice (the analytics
   columns are sticky, not delta-able to zero on most frames).
3. **Promote analytics to its own protocol with version negotiation.**
   Rejected — version negotiation is precisely what we're removing. A
   simple JSON-or-binary message type on the existing WS connection
   covers the use case.
4. **Phased migration (separate PRs).** Originally proposed in PRD-007;
   superseded by the 2026-04-30 velocity directive: single sprint, atomic
   landing.

## Implementation references

- Server: `src/utils/binary_protocol.rs`,
  `src/actors/client_coordinator_actor.rs::serialize_positions`,
  `src/actors/gpu/{clustering_actor,anomaly_detection_actor,sssp_actor}.rs`,
  `src/handlers/socket_flow_handler/position_updates.rs::handle_subscribe_position_updates`.
- Client: `client/src/store/websocket/binaryProtocol.ts`,
  `client/src/types/binaryProtocol.ts`,
  `client/src/store/websocket/index.ts` (message dispatch),
  `client/src/store/analyticsStore.ts` (new),
  `client/src/features/graph/components/{ClusterHulls,AnomalyOverlay,InstancedLabels}.tsx`.
- Docs: `docs/binary-protocol.md` (new, single-source spec), this ADR,
  PRD-007, DDD `ddd-binary-protocol-context.md`.

## Telemetry / observability

A new counter `binary_protocol.bytes_per_frame_per_client` exposed via
`/metrics` allows post-merge verification. Expected steady-state value:
`9 + 28 * <visible_node_count_for_client>`. Expected delta vs the prior
release: 50% reduction.
