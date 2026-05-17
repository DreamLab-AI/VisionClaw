# PRD-007: Binary Protocol Unification

**Status:** Draft
**Author:** Architecture Audit (codex consult + qe-code-reviewer agent + cadence-reframing pass)
**Date:** 2026-04-30
**Priority:** P0 — fixes per-frame waste of ~36 MB/s on a 25k-node graph; client init hangs traced to JSON bloat have a parallel cause on the WS side
**Related:** ADR-037 (binary position protocol — to be retired), ADR-038 (push-path consolidation), ADR-050 (sovereign ownership / privacy bits), ADR-031 (broadcast backpressure), PRD-005 (v2 ontology pipeline). Supersedes ADR-037's per-node payload schema.
**Companion ADR:** [ADR-061](adr/ADR-061-binary-protocol-unification.md)
**DDD Context:** [Binary Protocol Bounded Context](ddd-binary-protocol-context.md)

---

## 1. Problem Statement

The WebSocket binary channel was designed for one purpose: stream **per-physics-tick position and velocity** from the GPU to subscribed clients at 60 Hz, identified only by node index. The on-disk justification (`src/utils/binary_protocol.rs:11-31`) reads:

> ADR-037: literal-only, full snapshot of every node's absolute position + velocity.

Today the same channel ships **48 bytes per node**, exactly **double** the original intent. The added 24 bytes are not extra physics — they are sticky labels and session-static type discriminators riding the 60 Hz wire:

| Current field | Bytes | Producer | Recompute cadence | Per-frame transmission justified? |
|---|---|---|---|---|
| `position` (3×f32) | 12 | `ForceComputeActor` GPU kernel | every physics tick | **YES** — original intent |
| `velocity` (3×f32) | 12 | `ForceComputeActor` GPU kernel | every physics tick | **YES** — needed for client tween between sparse broadcasts |
| `sssp_distance` (f32) | 4 | `SsspActor` GPU kernel | only when SSSP source/topology changes | **NO** — zero/Infinity 99% of the time |
| `sssp_parent` (i32) | 4 | `SsspActor` GPU kernel | same | **NO** — sticky-per-N-frames |
| `cluster_id` (u32) | 4 | `ClusteringActor` (k-means on GPU) | recompute interval ≈ seconds | **NO** — same value rides the wire 60×/s between recomputes |
| `community_id` (u32) | 4 | `ClusteringActor` (Louvain on GPU) | recompute interval ≈ seconds | **NO** — same |
| `anomaly_score` (f32) | 4 | `AnomalyDetectionActor` GPU kernel | LOF recompute interval (slow loop) | **NO** — same |
| `id_with_flags` (u32) | 4 | encoder | id is per-frame; flag bits 26-31 are session-invariant | **partial** — keep id, demote 6 flag bits |

Plus an envelope:
- `[u8 version=5][u64 broadcast_sequence]` (9 bytes per frame — keep; backpressure replenishment relies on it via `ClientBroadcastAck`).

### 1.1 Quantified waste

On a 25 k-node graph at 30 Hz steady-state:
- 20 redundant bytes/node × 25 000 × 30 Hz = **≈ 18 MB/s** of wire spend on values that did not change.
- Browser-side, every frame the decoder runs through the analytics offsets in `parseBinaryFrameData` for fields it then re-stores at the same index of the same side-table — wasted main-thread cycles 30 times a second.

### 1.2 Versioning has accumulated, not converged

The `binary_protocol.rs` codebase carries:
- `PROTOCOL_V3` — the original 48-byte node format
- `PROTOCOL_V4` — delta encoding (retired 2026-04-20 per ADR-037)
- `PROTOCOL_V5` — V3 nodes wrapped with an 8-byte broadcast sequence (current default)
- legacy / "extended" / "with-sssp" / "with-privacy" flavours of `encode_node_data_*`

The client decoder mirrors this with branches at `binaryProtocol.ts:421-457`. Version proliferation has become a soft cost: every new contributor reads the matrix of code paths, every change has to choose a flavour, every test fixture has to declare which one it speaks.

The user directive (2026-04-30) is to stop framing this as a "versioning" problem. **There will be one binary protocol.** It is the GPU's per-frame stream. New analytics outputs go on a different channel. Code, ADRs, tests, and docs unify on that single name.

---

## 2. Goals

| # | Goal | Success Metric |
|---|------|----------------|
| G1 | Reduce per-node wire payload to its original intent | Per-node binary frame is **28 bytes** (id + position + velocity) |
| G2 | Move sticky GPU outputs to an on-change side stream — same producer, different cadence | `ClusteringActor`, `AnomalyDetectionActor`, `SsspActor` push their results only on recompute completion. No analytics field appears in the per-frame stream. |
| G3 | Remove version numbering from the protocol surface | No `V3`/`V4`/`V5` tokens in `src/utils/binary_protocol.rs`, in client decoder, in test names, or in current ADRs. The wire format byte is removed (or fixed and undocumented). One name: "binary protocol." |
| G4 | Demote session-invariant type/visibility discriminators to JSON init | Bits 26-31 of id-with-flags removed from the wire. Node type / privacy state ride `/api/graph/data`'s JSON response once at init; client side-table caches them. |
| G5 | Preserve all current observable behaviour | No regression in: physics live updates, drag pinning, settings-driven reheat, ADR-050 anonymous-viewer opacification, client-side cluster overlays, SSSP overlays, anomaly highlights. |
| G6 | Wire-cost telemetry is in place | A counter exposes `bytes_per_frame_per_client` and `analytics_updates_per_minute` so we can confirm the saving in production. |

---

## 3. Non-Goals

- **No** rewrite of GPU kernels. Producers stay where they are.
- **No** change to the broadcast-sequence/ack mechanism. The envelope's 9-byte header is kept.
- **No** introduction of delta/diff encoding. The original "literal-only, full snapshot" property is preserved for the per-frame stream.
- **No** change to the polling REST path (`/api/graph/data`). It already gets the slow-path JSON treatment.
- **No** new client framework. The existing `binaryProtocol.ts` decoder is simplified, not replaced.

---

## 4. The Single Protocol

### 4.1 Per-frame stream — `binary_protocol`

```
Frame layout:
  [u8  preamble        = 0x42]   ← "B" — fixed, used only for sanity, not version dispatch
  [u64 broadcast_sequence_LE]    ← preserved from prior envelope; backpressure ack key
  [N × Node]

Per-Node layout (28 bytes, fixed):
  [u32 id_LE]          ← raw node id; no flag bits
  [f32 x_LE]
  [f32 y_LE]
  [f32 z_LE]
  [f32 vx_LE]
  [f32 vy_LE]
  [f32 vz_LE]
```

Notes:
- The preamble byte stays at 0x42 forever. It is **not** a version. If the protocol ever needs to evolve it does so via a new endpoint, not a version byte.
- The broadcast-sequence is the only acked unit. `ClientBroadcastAck` semantics are unchanged.
- ADR-050 privacy: the wire id is the raw id. Anonymous viewers receive **no** position frames for nodes they cannot see (server-side filter in `ClientCoordinator::broadcast_with_filter`). Bit-29 opacification is no longer needed in the per-frame stream because per-node visibility was always known at init time and is enforced by the per-client filter.

### 4.2 On-change stream — `analytics_update`

A separate WebSocket message type, sent by the GPU-side actors only when their compute kernel completes:

```
Message layout (text or binary; recommend binary for size at scale):
  type: "analytics_update"
  source: "clustering" | "community" | "anomaly" | "sssp"
  generation: u64                ← monotonic, for client merge-ordering
  entries: [
    { id: u32,
      cluster_id?: u32,           ← only when source == "clustering"
      community_id?: u32,         ← only when source == "community"
      anomaly_score?: f32,        ← only when source == "anomaly"
      sssp_distance?: f32,        ← only when source == "sssp"
      sssp_parent?: i32           ← only when source == "sssp"
    }, …
  ]
```

Cadence: 0.1 – 1 Hz for clustering/community, on demand for SSSP, slow loop for anomaly. **Never** per physics tick.

Client behaviour: maintain a side-table `analyticsByNodeId: Map<u32, AnalyticsRow>` populated from `analytics_update` messages. Renderers (cluster hulls, anomaly overlay, SSSP gradients) read from the side-table, not from per-frame data.

### 4.3 What is no longer on the wire per frame

| Field | New home | Cadence |
|---|---|---|
| `sssp_distance`, `sssp_parent` | `analytics_update` source=`sssp` | on SSSP recompute |
| `cluster_id` | `analytics_update` source=`clustering` | on k-means recompute |
| `community_id` | `analytics_update` source=`community` | on Louvain recompute |
| `anomaly_score` | `analytics_update` source=`anomaly` | on LOF recompute |
| Type flags (agent / knowledge / private / ontology-subtype) bits 26-31 | `/api/graph/data` JSON `node.node_type` (already present) | once, at init |

---

## 5. Code Touch List

### 5.1 Server (Rust)

| File | Change |
|---|---|
| `src/utils/binary_protocol.rs` | Replace `PROTOCOL_V3`/`V4`/`V5` constants, the `encode_node_data_*` family, and the flag-bit constants. Single `encode_position_frame(positions: &[(u32, BinaryNodeData)], broadcast_sequence: u64) -> Vec<u8>`. 28 B/node fixed. No flags. |
| `src/actors/client_coordinator_actor.rs::serialize_positions` (~line 472) | Drop the `private_opaque_ids`, `analytics_data`, and `node_type_arrays` parameters from the per-frame path. Per-client visibility enforcement moves up one level — the client filter already drops invisible nodes from `positions` before serialise. |
| `src/actors/gpu/clustering_actor.rs` | On k-means / Louvain completion, emit `analytics_update{source:"clustering"|"community"}` via a new `BroadcastAnalyticsUpdate` message that `ClientCoordinator` proxies to all subscribed sockets as a JSON or compact binary frame. **Stop writing `cluster_id`/`community_id` into the per-node binary frame state.** |
| `src/actors/gpu/anomaly_detection_actor.rs` | Same shape, source=`anomaly`. |
| `src/actors/gpu/sssp_actor.rs` (or wherever SSSP completes) | Same shape, source=`sssp`. Triggered by the SSSP-mode toggle, not by physics ticks. |
| `src/handlers/socket_flow_handler/position_updates.rs::handle_subscribe_position_updates` (~line 474) | Remove `min_interval_ms` and `requests_per_minute` from the `subscription_confirmed` payload (zero client consumers per audit). |
| `src/handlers/socket_flow_handler/types.rs` | Subscription state no longer needs `subscribed_node_types` filter — node types are session-static and resolved at JSON init. |
| `src/utils/binary_protocol.rs` tests | Replace V3/V5 fixture tests with a single "binary_protocol" round-trip test. |
| `tests/integration/binary_protocol_*.rs` | Rename + simplify; one happy path, one "anonymous viewer sees no private positions" path. |

### 5.2 Client (TypeScript)

| File | Change |
|---|---|
| `client/src/store/websocket/binaryProtocol.ts` | Delete `PROTOCOL_V3`/`V5` branches. `processBinaryData` becomes one path: validate preamble byte, read `broadcast_sequence`, iterate 28 B/node. The flag-bit decode loop and `currentNodeTypeMap` builder go away. |
| `client/src/types/binaryProtocol.ts` | Same — collapse the type matrix to one node shape. |
| `client/src/store/websocket/index.ts` (message dispatch) | Add `analytics_update` text-message handler that merges into a `useAnalyticsStore` Zustand slice. |
| `client/src/features/graph/components/ClusterHulls.tsx` and similar | Read `cluster_id` / `community_id` / `anomaly_score` from the analytics store, not from per-node parsed binary fields. |
| `client/src/features/graph/managers/graphDataManager.ts` | Per-frame decoded data simplifies to `Map<u32, {x,y,z,vx,vy,vz}>`. Drop the analytics-side-effect path. |
| `client/src/features/graph/components/InstancedLabels.tsx` | Read `nodeType` from the JSON-init side-table by id, not from per-frame flag bits. |

### 5.3 Documentation

| Document | Change |
|---|---|
| `docs/adr/ADR-037-binary-position-protocol.md` | Mark **Superseded by PRD-007**. Move historical content to a "Historical context" section. |
| `docs/adr/ADR-038-physics-push-path.md` (if exists) | Refresh to reference `binary_protocol` (no version), drop V5 mentions. |
| `docs/adr/ADR-050-sovereign-ownership.md` | Section H2: replace "bit-29 opacification on the wire" with "filtered out of the per-frame stream by `ClientCoordinator::broadcast_with_filter`." Same outcome, simpler implementation. |
| `CLAUDE.md`, `multi-agent-docker/CLAUDE.md`, `client/.../README.md` | Replace any `V3`/`V4`/`V5` references with **the binary protocol**. Single name throughout. |
| `client/src/store/websocket/binaryProtocol.ts` JSDoc | Update the doc-comment that currently lists three protocol versions. |
| `agentbox/docs/...` references | Audit; any cross-link that says "VisionFlow V5 wire" gets the same treatment. |

---

## 6. Single-Sprint Execution

Per the velocity directive (2026-04-30), all changes ship in ONE sprint, ONE
landing. No phasing, no fallback layer. The risk of an analytics renderer
not migrating cleanly is mitigated by the parallel-stream agent topology
described below — every renderer gets migrated in lockstep with the wire
trim. If a renderer can't read from the analytics store, the sprint doesn't
land.

### 6.1 Workstreams (executed in parallel)

| Stream | Owner | Deliverable |
|---|---|---|
| **A. Rust wire** | server agent | `binary_protocol.rs` collapsed to single `encode_position_frame(positions, broadcast_sequence) -> Vec<u8>`; flag-bit constants removed; legacy `encode_node_data_*` family deleted; new `BroadcastAnalyticsUpdate` actor message + serialiser |
| **B. Rust analytics emitters** | server agent | `ClusteringActor` / `AnomalyDetectionActor` / `SsspActor` emit `BroadcastAnalyticsUpdate` on recompute completion only; remove their writes to per-node binary columns |
| **C. TypeScript client** | client agent | `binaryProtocol.ts` decoder reduced to one path (28 B/node); `analytics_update` text-message handler; new `useAnalyticsStore` Zustand slice; renderers (`ClusterHulls`, anomaly overlay, SSSP gradients, `InstancedLabels` for `nodeType`) read from store/JSON-init not per-frame |
| **D. Tests** | qe agent | Replace V3/V5 fixtures with one `binary_protocol_roundtrip_test`; client decoder unit tests; renderer integration tests verifying side-table reads; one E2E smoke proving cluster hulls + anomaly overlay still render after wire trim |
| **E. Doc consolidation** | doc agent | Mark ADR-037 **Superseded by ADR-061**; remove every `V3`/`V4`/`V5` token from `src/`, `client/src/`, `docs/`, `tests/`; new `docs/binary-protocol.md` single-source spec; CLAUDE.md / README updates; JSDoc cleanup |

### 6.2 Landing order within the sprint

The streams are parallel but the merge has one ordering constraint:
analytics emitters (B) must reach `main` before — or with — the wire trim
(A), so that even a brief in-flight build never has a renderer reading
analytics columns that the wire no longer provides. The sprint coordinator
gates merge on (A+B+C+D+E green) as a single atomic landing. Branch is
`feat/binary-protocol-unification`.

### 6.3 Verification gates

Before the sprint can claim success:

1. `cargo build --lib && cargo test` green; the binary protocol roundtrip
   test exercises the new 28 B/node format end-to-end.
2. `cargo grep -r "PROTOCOL_V3\|PROTOCOL_V4\|PROTOCOL_V5\|encode_node_data_with_"` returns
   zero matches in `src/`.
3. `tsc --noEmit && jest` green; `grep -rE "PROTOCOL_V[345]|BINARY_NODE_SIZE_V" client/src` returns
   zero matches.
4. Browser smoke: hard-refresh against a freshly-rebuilt backend.
   Per-node wire payload measured at exactly 28 bytes via DevTools
   network inspection of the WS frames; cluster hulls / anomaly overlay /
   SSSP overlay all render correctly; settings-driven reheat still works.
5. Telemetry counter `bytes_per_frame_per_client` reports ≈ `9 + 28*N`
   on a connected client.

### 6.4 Telemetry verification (post-merge)

Confirm production shows predicted wire-cost reduction (≈18 MB/s saved at
25 k nodes / 30 Hz). If the saving is materially less than predicted,
follow-up ADR investigates.

---

## 7. Risks

| # | Risk | Mitigation |
|---|------|-----------|
| R1 | Renderers that silently relied on per-frame analytics columns break when those columns disappear | Phase 1 ships the side stream first; Phase 2 only proceeds after renderers are confirmed to read from the store. Add a Phase-1 deprecation warning in client logs whenever a renderer reads analytics from per-frame data. |
| R2 | The `broadcast_sequence` envelope removal accidentally lands as part of the cleanup | Explicit non-goal in §3. Tests should pin the envelope shape. |
| R3 | Anonymous-viewer behaviour shifts — ADR-050 H2 currently relies on bit-29 opacification at the encoder | The new model enforces visibility at the per-client filter (drop the position from the frame entirely for non-owners). Outcome is the same: anonymous viewer sees nothing for private nodes. Update ADR-050 H2 prose. |
| R4 | The `analytics_update` cadence is mis-tuned and floods clients | Add a per-source rate limit (`max 1/sec` for clustering, `max 1/min` for anomaly) and a server-side coalesce. |
| R5 | Existing serialised binary fixtures or snapshot tests break en masse | Phase 2 PR rewrites them in lockstep; CI must go green before merge. |
| R6 | Old client (out of date) reconnects expecting V5 envelope | The preamble byte (0x42) differs from V3 (3) and V5 (5). Old clients see "invalid version" warning and reconnect attempts fail closed. Acceptable — clients are pinned by deployment. |

---

## 8. Success Criteria

1. **Wire**: per-node payload is 28 bytes, every frame, every client. No exceptions.
2. **Code**: zero matches for `PROTOCOL_V3`, `PROTOCOL_V4`, `PROTOCOL_V5`, `binary_protocol_v` in `src/`, `client/src/`, `docs/`, `tests/`.
3. **Docs**: every reference to "V3" / "V5" wire format in CLAUDE.md / README.md / ADRs has been removed or relegated to a "Historical context" subsection.
4. **Behaviour**: at 25 k nodes / 30 Hz, the browser shows physics, tweening, cluster hulls, anomaly overlays, and SSSP overlays correctly; client init does not hang; settings-driven reheat propagates as before.
5. **Telemetry**: `bytes_per_frame_per_client` averages around `9 + 28*N` (where N is the visible node count for that client). `analytics_updates_per_minute` is bounded.

---

## 9. Out-of-Scope Follow-ups

- Delta encoding of the per-frame stream (revisit only if profiling shows wire is still the bottleneck).
- Promotion of the analytics-update channel to its own protocol with its own ADR.
- A binary serialisation for the JSON-init payload (current 3 MB at 22 k nodes is not a problem post the metadata-allowlist fix).
- Migrating `/api/bots/*` polling to the analytics-update channel (ADR-038 ground; tracked separately).

---

## 10. Open Questions for Reviewers

- Is there any current consumer of `cluster_id` / `community_id` per-frame that requires per-frame freshness rather than per-recompute? If yes, name it; the rate-limit on the side stream may need raising.
- Should the analytics-update message use binary or JSON? JSON is simpler; binary is ~3-5× smaller. Recommend binary if the analytics store updates are expected at >1 Hz.
- Is the preamble byte `0x42` worth keeping at all, or do we just declare the WS subprotocol and let the absence of versioning be load-bearing? (Recommend keeping it as a one-byte sanity check for malformed frames.)
