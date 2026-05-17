# DDD Analysis: Binary Protocol Bounded Context

> **Related**: [PRD-007](PRD-007-binary-protocol-unification.md) · [ADR-061](adr/ADR-061-binary-protocol-unification.md) · [ADR-031](adr/ADR-031-broadcast-backpressure.md) · [ADR-050 §H2](adr/ADR-050-sovereign-ownership.md#h2-opacification) · [ADR-037 (Superseded)](adr/ADR-037-binary-position-protocol.md)

## 1. Bounded Context Map

```mermaid
graph TB
    subgraph "Core Domain: Position Stream"
        FCA[ForceComputeActor]
        PFA[Position Frame Assembler]
        BS[Broadcast Sequencer]
        CC[ClientCoordinator]
    end

    subgraph "Supporting: Analytics Stream"
        CLA[ClusteringActor]
        ADA[AnomalyDetectionActor]
        SSA[SsspActor]
        AUE[AnalyticsUpdate Emitter]
    end

    subgraph "Wire (WebSocket)"
        WS[SocketFlowServer]
    end

    subgraph "Client (Browser)"
        DEC[binaryProtocol decoder]
        ANS[useAnalyticsStore]
        TWN[Tween / Render Loop]
    end

    subgraph "Init Channel (HTTP)"
        API[/api/graph/data]
        SS[Side-table: nodeType, visibility]
    end

    FCA -->|positions per tick| PFA
    PFA --> BS
    BS --> CC
    CC -->|28 B/node frame| WS

    CLA -->|on recompute| AUE
    ADA -->|on recompute| AUE
    SSA -->|on recompute| AUE
    AUE -->|analytics_update msg| WS

    API -->|once at init| SS

    WS -->|28 B/node| DEC
    WS -->|analytics_update| ANS

    DEC -->|pos+vel| TWN
    ANS -->|cluster/anomaly/sssp| TWN
    SS -->|nodeType| TWN
```

## 2. Strategic Patterns

### 2.1 Two streams, one transport

The bounded context is split along **cadence**, not transport. Both
streams ride the same WebSocket connection but obey different timing
contracts:

| Stream | Producer | Cadence | Carrier | Volume |
|---|---|---|---|---|
| **Position Stream** | `ForceComputeActor` (GPU physics kernel) | every physics tick (16-50 ms) | binary frames, 28 B/node | high (MB/s) |
| **Analytics Stream** | `ClusteringActor`, `AnomalyDetectionActor`, `SsspActor` (GPU compute kernels) | on recompute completion (seconds) | JSON or compact binary `analytics_update` messages | low (KB/s) |

The mistake the prior design made was **conflating cadence with carrier**:
because the analytics actors produced their outputs on the GPU, the
implementation routed those outputs through the same per-tick path as
positions. This ADR-061 split corrects that.

### 2.2 Init vs steady-state

A third channel — the JSON `/api/graph/data` HTTP endpoint — carries
session-invariant per-node state (node_type, visibility, owner_pubkey,
canonical_iri). This is **send-once at init**, never repeated. The
client's `currentNodeTypeMap` is populated from this and consulted
without needing to re-read it per frame.

### 2.3 Anti-corruption: visibility filtering

ADR-050 H2 (anonymous-viewer privacy) was implemented by setting bit-29
on every private node id in every binary frame. After the unification,
the wire carries no such bit. Visibility is enforced at the boundary
(`ClientCoordinator::broadcast_with_filter`) by simply not including
positions for nodes the caller cannot see. The bounded-context boundary
is moved one level outward; the wire format becomes ignorant of privacy
state.

## 3. Aggregates

### Aggregate: `PositionFrame`

**Aggregate root**: `PositionFrame`

**Invariants:**
- Frame is a single physics tick's snapshot — never accumulates state.
- Every node entry is exactly 28 bytes; no variable-length fields.
- `broadcast_sequence` is monotonic per `ClientCoordinator` instance.
- Frame carries position + velocity for *every visible node* of the caller — no partial frames.

**Entities:**
- `PositionFrame { preamble: u8, broadcast_sequence: u64, entries: Vec<NodeEntry> }`
- `NodeEntry { id: u32, x: f32, y: f32, z: f32, vx: f32, vy: f32, vz: f32 }`

**Allowed operations:**
- `encode_position_frame(positions, broadcast_sequence) -> Vec<u8>` (server)
- `decode_position_frame(bytes) -> PositionFrame` (client)

**Forbidden:**
- Adding fields without an explicit ADR superseding ADR-061.
- Versioning. The preamble is a sanity byte, not a version dispatch.

### Aggregate: `AnalyticsUpdate`

**Aggregate root**: `AnalyticsUpdate`

**Invariants:**
- `source` is one of {clustering, community, anomaly, sssp}. Each source
  is a single producer.
- `generation` is monotonic per source. Out-of-order updates are dropped
  client-side (last-wins by generation).
- `entries` may be partial — only nodes whose value changed since the
  prior generation need to be present.
- Server-side rate cap: `max 1/sec` per source, coalescing by generation.

**Entities:**
- `AnalyticsUpdate { source: AnalyticsSource, generation: u64, entries: Vec<AnalyticsEntry> }`
- `AnalyticsEntry { id: u32, cluster_id?: u32, community_id?: u32, anomaly_score?: f32, sssp_distance?: f32, sssp_parent?: i32 }`

**Allowed operations:**
- Producer actors emit `BroadcastAnalyticsUpdate { update }` to `ClientCoordinator`.
- Client `useAnalyticsStore.merge(update)` — last-wins by generation.

**Forbidden:**
- Per-frame emission. Producer actors must emit only on actual recompute
  completion. The coalescing rate cap is the second line of defence.
- Cross-source mixing. Each `AnalyticsUpdate` carries exactly one source.

### Aggregate: `NodeInitDescriptor`

**Aggregate root**: `NodeWithPosition` (already exists in
`api_handler/graph/mod.rs:34-79`)

**Invariants:**
- Sent once per session via `/api/graph/data`.
- Carries `node_type`, `visibility`, `owner_pubkey`, `pod_url`,
  `canonical_iri`, plus all display metadata (color, size, group).
- Position/velocity are best-effort; the binary stream is authoritative.

**Forbidden:**
- Per-frame retransmission of these fields. They are session-invariant
  by definition.

## 4. Domain Events

| Event | Producer | Consumer | Carrier |
|---|---|---|---|
| `PositionTickAvailable` | `ForceComputeActor` | `ClientCoordinator` → wire | `BroadcastPositions` actor message → 28 B/node frame |
| `ClusteringRecomputeCompleted` | `ClusteringActor` | `ClientCoordinator` → wire | `BroadcastAnalyticsUpdate { source: clustering }` → JSON/binary message |
| `CommunityRecomputeCompleted` | `ClusteringActor` (Louvain branch) | `ClientCoordinator` → wire | `BroadcastAnalyticsUpdate { source: community }` |
| `AnomalyDetectionCompleted` | `AnomalyDetectionActor` | `ClientCoordinator` → wire | `BroadcastAnalyticsUpdate { source: anomaly }` |
| `SsspComputeCompleted` | `SsspActor` | `ClientCoordinator` → wire | `BroadcastAnalyticsUpdate { source: sssp }` |
| `ClientSubscribed` | `SocketFlowServer` | `ClientCoordinator` | `RegisterClient` actor message; triggers GraphData JSON push |
| `ClientAcked` | `SocketFlowServer` | `ForceComputeActor` (token-bucket replenish) | `ClientBroadcastAck` actor message |

## 5. Invariants (cross-aggregate)

| # | Invariant | Why |
|---|---|---|
| I01 | Position stream cadence ≤ physics tick cadence ≤ 60 Hz | Wire spend is bounded by physics, not by analytics. |
| I02 | Analytics stream cadence ≤ 1 Hz per source | Sticky data must not steal physics bandwidth. |
| I03 | Total per-node binary payload = exactly 28 B | The stream's contract. Any field requiring per-frame transmission needs an ADR superseding ADR-061. |
| I04 | `broadcast_sequence` strictly increases per `ClientCoordinator` | Backpressure ack via `ClientBroadcastAck` (ADR-031) requires monotonic sequencing. |
| I05 | Analytics `generation` strictly increases per source | Last-wins merge is correct only if generations don't repeat. |
| I06 | Anonymous viewer wire frames contain only public-visible nodes | ADR-050 §H2 outcome preserved; mechanism changed from bit-29 to per-client filter. |
| I07 | `nodeType` and `visibility` are present in `/api/graph/data` JSON for every node | Client renderer correctness — never reads these from per-frame data. |
| I08 | No `V3`/`V4`/`V5` literal in `src/`, `client/src/`, `docs/`, `tests/` | Versioning vocabulary deprecation; verified in CI grep. |

## 6. Ubiquitous Language

| Term | Definition |
|---|---|
| **Binary protocol** | The single per-physics-tick wire format: 28 B/node, preamble + broadcast_sequence + entries. There are no "versions" of the binary protocol. |
| **Position stream** | The continuous per-tick wire path from ForceComputeActor to subscribed clients. |
| **Analytics stream** | The on-recompute message path from ClusteringActor / AnomalyDetectionActor / SsspActor to subscribed clients. |
| **Init descriptor** | The session-invariant per-node JSON sent once via `/api/graph/data`. |
| **Recompute completion** | The discrete event that gates analytics-stream emission. Producer actors are forbidden from emitting on any other trigger (especially: not on physics tick). |
| **Generation** | Monotonic counter per analytics source. Used for last-wins merge. |
| **Side-table** | Client-side `useAnalyticsStore` keyed by node id; populated from analytics_update messages. |

## 7. Anti-Patterns Forbidden

The following patterns are **explicitly forbidden** to prevent regression
back into the pre-ADR-061 state:

1. **Adding columns to per-frame binary** without an ADR superseding
   ADR-061. The 28 B/node payload is the contract.
2. **Encoding session-static state in id flag bits.** Bits 26-31 are
   gone. If session-static state needs to ride the wire, it goes via
   JSON init.
3. **Emitting `analytics_update` from a per-frame loop.** Producer
   actors emit only on their compute kernel's completion event.
4. **Versioning the binary protocol.** Any future evolution gets a new
   endpoint name (`/wss-v2/positions`, etc.) and a separate ADR. The
   preamble byte 0x42 is fixed forever.
5. **Coalescing analytics_update messages across sources.** Each
   `AnalyticsUpdate` carries exactly one source; clients depend on this
   for selective merge.
6. **Re-introducing wire-side bit-29 opacification.** Visibility filter
   at `broadcast_with_filter` is the canonical mechanism per ADR-061
   §D3.

## 8. Test Strategy

The bounded-context tests follow the aggregate boundaries:

| Test | Scope | What it pins |
|---|---|---|
| `binary_protocol_roundtrip_test.rs` | Server unit | encode → decode preserves position + velocity |
| `analytics_update_serde_test.rs` | Server unit | `BroadcastAnalyticsUpdate` JSON round-trip per source |
| `position_stream_e2e_test.rs` | Server integration | ForceComputeActor → wire → mock client receives 28 B/node |
| `analytics_stream_cadence_test.rs` | Server integration | Producer firing per-tick is rate-capped to 1/sec; coalescing works |
| `binaryProtocol.decoder.test.ts` | Client unit | Decoder produces `Map<id, {x,y,z,vx,vy,vz}>`; rejects non-0x42 preamble |
| `analyticsStore.merge.test.ts` | Client unit | Last-wins by generation per source; out-of-order drops |
| `clusterHulls.render.test.tsx` | Client integration | ClusterHulls reads from store, not from per-frame data |
| `binary_protocol_e2e_smoke.test.ts` | E2E | Browser receives full session: JSON init → position frames → analytics_update → renderers update |

## 9. Open Questions

- Should `analytics_update` be JSON or binary? JSON is simpler; binary is
  ~3-5× smaller. For a 25 k-node graph emitting cluster_id at 0.1 Hz,
  JSON ≈ 3 MB/event, binary ≈ 1 MB/event. Recommend binary if
  recompute frequency ever exceeds 1 Hz; JSON otherwise.
- Should `generation` be per-source or globally monotonic? Per-source is
  sufficient for last-wins merge and avoids cross-source serialisation
  contention.
- Is there a future case where `community_id` needs per-frame transmission
  (e.g. live community-detection animation)? If so, the right answer is
  **a separate fast-cadence channel**, not adding the column back to the
  position stream.
