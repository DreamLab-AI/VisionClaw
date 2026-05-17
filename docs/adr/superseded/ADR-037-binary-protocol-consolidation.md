# ADR-037: Binary Protocol Consolidation

> **Status: Superseded by [ADR-061](ADR-061-binary-protocol-unification.md) (2026-04-30)** —
> the V3/V5 wire format documented here was replaced by the unified binary protocol with
> a 24-byte/node payload and no versioning vocabulary. The historical content below
> remains for archaeological reference. Do not implement against this ADR.
>
> The current single-source wire-format spec is [docs/binary-protocol.md](../binary-protocol.md).

## Status

**Superseded by ADR-061** (2026-04-30). Originally implemented (2026-04-20),
relitigated (2026-04-21).

## Historical context

## Lock-in (2026-04-21)

**DO NOT re-introduce delta-encoded position protocols (V4 or any successor).**

Multiple agents have attempted to "optimise bandwidth" by wiring delta encoding
back into the broadcast path. Every attempt has regressed position updates.
This is wrong for our workload and the reasons are physics-fundamental, not
about implementation quality:

1. The graph is a force-directed spring network. Under continuous simulation
   every node moves every tick. Our "deltas" always contain every node — there
   is nothing to compress.
2. Delta encoding trades a tiny (near-zero for us) bandwidth saving for real
   correctness risks: stale-position drift on reconnect, silent drop of user
   pin signals when the threshold filters them out, and parallel decode paths
   that double the bug surface.
3. The real bandwidth lever is **broadcast cadence**, not payload encoding.
   `ForceComputeActor` now drives broadcasts via **network backpressure** (a
   token-bucket gate — we only emit as fast as the client pipeline drains).
   No FPS timer. No delta filter.

**Enforcement:**

- Server `src/utils/binary_protocol.rs` carries an ARCHITECTURE LOCK comment
  at the top of the file; V4 constants/decoders are gone.
- Client `client/src/types/binaryProtocol.ts` carries the same lock; any V4
  frame received triggers a loud error log and is dropped (regression detector).
- Server `src/gpu/broadcast_optimizer.rs` retains `DeltaCompressor` only as
  dead code for a future pure deletion PR; its `filter_delta_updates` must
  never be re-wired. Header comment enforces this.
- Server `src/actors/gpu/force_compute_actor.rs` broadcast path is
  **literal-only + backpressure-driven**. The delta branch has been removed.

Any PR that adds `PROTOCOL_V4`, a `type: 'delta'` flag on the wire, a
`filter_delta_*` call from the broadcast path, or a "bandwidth-saving"
per-node movement threshold is **rejected on sight**.

## Context

VisionFlow's binary WebSocket protocol has accumulated 14 encoding paths across
4 protocol versions (V2-V5) through incremental feature additions. The current
state creates maintenance burden, type-classification bugs, and dead code that
confuses contributors.

**Server-side encoding functions (binary_protocol.rs):**

| Function | Delegates to | Used by |
|----------|-------------|---------|
| `encode_node_data()` | `encode_node_data_with_types(&[], &[])` | Nowhere meaningful |
| `encode_node_data_with_flags()` | `encode_node_data_with_types(agents, &[])` | Legacy callers |
| `encode_node_data_with_types()` | `encode_node_data_extended()` | Thin wrapper |
| `encode_node_data_extended()` | `encode_node_data_extended_with_sssp(None, None)` | Thin wrapper |
| `encode_node_data_with_analytics()` | `encode_node_data_with_all()` | Thin wrapper |
| `encode_node_data_with_all()` | Duplicates `encode_node_data_extended_with_sssp` body | 0 external callers |
| `encode_node_data_extended_with_sssp()` | Terminal encoder (V3, 48 bytes/node) | All paths converge here |
| `encode_node_data_with_live_analytics()` | `encode_node_data_extended_with_sssp(&[], &[], ...)` | Polling path (broken) |

All seven wrappers eventually call `encode_node_data_extended_with_sssp()` which
always emits V3 frames (48 bytes per node).

**Critical bug:** `encode_node_data_with_live_analytics()` passes empty type
arrays (`&[], &[]`) for agent/knowledge IDs. Every node sent through this path
(position_updates.rs polling, fastwebsockets_handler.rs) arrives on the client
classified as `Unknown`, breaking type-based rendering.

**V4 delta encoding (delta_encoding.rs):** Fully implemented but permanently
disabled. The delta encoder is called with `frame=0`, which always triggers the
full-state V3 resync branch, so no V4 frame is ever emitted.

**V5 backpressure wrapper (client_coordinator_actor.rs):** A thin 9-byte prefix
(`[version=5][8-byte sequence LE]`) prepended to V3 node data. Used by the
`ClientCoordinatorManager::serialize_positions()` broadcast path only.

**Two serialize_positions() methods on ClientCoordinator:** The manager has a V5
variant (with sequence number and analytics). The inner `ClientCoordinator` has a
V3-only variant (no analytics, no sequence). Both convert `BinaryNodeDataClient`
to `(u32, BinaryNodeData)` then call different protocol functions.

**Client-side dead code:**
- `BinaryWebSocketProtocol.ts` implements a `MessageType`-based protocol
  (`GRAPH_UPDATE`, `NODE_POSITIONS`, etc.) that the server never sends.
- `binaryProtocol.ts` contains a V2 parser that is unreachable since the server
  only emits V3/V5.

## Decision

Consolidate to three encoding paths:

1. **`encode_positions_v3()`** -- Single V3 encoder that accepts node data, type
   classification arrays, optional SSSP data, and optional analytics data. This
   replaces all seven existing wrappers plus `encode_node_data_with_all()`.

2. **`wrap_v5(v3_frame, sequence)`** -- Stateless function that strips the V3
   version byte and prepends the V5 header. Used only by the broadcast path that
   needs backpressure sequence correlation.

3. **Keep `delta_encoding.rs` dormant** -- Do not delete. The V4 delta encoder
   is correct and tested. Gate it behind a feature flag (`delta-encoding`) so it
   can be re-enabled when the client V4 parser is validated end-to-end.

Additionally:

- **Fix the type-classification bug:** All callers must pass node type arrays.
  Remove `encode_node_data_with_live_analytics()` and require callers to supply
  type arrays explicitly. The polling paths in `position_updates.rs` and
  `fastwebsockets_handler.rs` must obtain `NodeTypeArrays` from `AppState`.

- **Unify `serialize_positions()`:** Collapse the two methods into one on
  `ClientCoordinatorManager` that always produces V5 frames.

- **Delete client dead code:** Remove `BinaryWebSocketProtocol.ts` (phantom
  protocol) and the V2 parser from `binaryProtocol.ts`.

## Consequences

### Positive

- Eliminates 6 wrapper functions and ~120 lines of delegation boilerplate
- Fixes the Unknown-node-type bug on all polling paths
- Single encoding path simplifies protocol version upgrades
- Client bundle size decreases by removing phantom protocol code
- `wrap_v5()` makes the V3-to-V5 relationship explicit and testable

### Negative

- Breaking change for any out-of-tree code calling removed wrapper functions
- Polling path callers need refactoring to thread `NodeTypeArrays` through
- Feature-flagging delta encoding adds a cargo feature to manage

### Neutral

- Wire format does not change; V3 and V5 frames remain byte-compatible
- No client parser changes needed beyond dead code removal

## Options Considered

### Option 1: Status Quo

- **Pros**: No migration work
- **Cons**: Type-classification bug persists, 7 wrappers remain, contributors
  cannot determine the canonical encoding path

### Option 2: Consolidate to V3 + V5 wrapper (chosen)

- **Pros**: Minimal API surface, fixes type bug, preserves V4 for future use
- **Cons**: One-time refactor cost across ~10 call sites

### Option 3: Full V5 migration (drop V3)

- **Pros**: Single protocol version everywhere
- **Cons**: Sequence numbers add overhead for paths that do not need backpressure;
  forces all clients to implement V5 parsing immediately

## Related Decisions

- ADR-013: Render Performance (node type rendering depends on correct flags)
- ADR-012: WebSocket Store Decomposition (client protocol layer)

## References

- `src/utils/binary_protocol.rs` -- all encoding functions
- `src/utils/delta_encoding.rs` -- V4 delta encoder
- `src/actors/client_coordinator_actor.rs` -- two `serialize_positions()` methods
- `src/handlers/socket_flow_handler/position_updates.rs` -- polling path (bug)
- `src/handlers/fastwebsockets_handler.rs` -- fastws polling path (bug)
- `client/src/services/BinaryWebSocketProtocol.ts` -- phantom protocol (delete)
- `client/src/types/binaryProtocol.ts` -- V2 dead parser (delete)
