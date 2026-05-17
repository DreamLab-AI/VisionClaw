# ADR-038: Position Data Flow Consolidation

> **Wire-format clauses updated by [ADR-061](ADR-061-binary-protocol-unification.md) (2026-04-30)** —
> the push-path consolidation decision in this ADR remains in effect. The wire-format
> language below (legacy V3/V5 frame names and versioning vocabulary) is historical;
> the current spec is one binary protocol with no versioning. See
> [docs/binary-protocol.md](../binary-protocol.md).

## Status

Implemented 2026-04-20 (dead poll block removed from position_updates.rs; push path canonical).
Wire-format clauses superseded by ADR-061 (2026-04-30); push-path consolidation still active.

## Date

2026-04-14

## Context (historical, pre-ADR-061)

VisionFlow had 7 server-side position delivery paths. The overlap caused duplicate
frames on the same WebSocket connection with conflicting wire-format encodings:

1. **Push path**: `ForceComputeActor` -> `GraphServiceSupervisor` ->
   `PhysicsOrchestratorActor` -> `ClientCoordinatorActor` -> WebSocket V5 frames.
2. **Poll path**: Subscription timer in `position_updates.rs` reads `GraphStateActor`,
   encodes V3 frames, sends to the same WebSocket session. Reads stale actor state.
3. **REST `/api/graph/data`**: Returns positions from `GraphStateActor` (often Neo4j
   zeros because GPU results are not always propagated back).
4. **REST `/api/graph/positions`**: Reads live GPU positions. Not used by default client.

Push and poll run simultaneously. Clients receive interleaved V5 and V3 frames with
different position values and incompatible binary layouts. The poll path is a legacy
fallback from before the GPU push pipeline and was never removed.

## Decision Drivers

- Clients must receive a single authoritative position stream per connection.
- REST endpoints must return consistent position data.
- The GPU push path is the source of truth for live positions.

## Considered Options

### Option 1: Keep push, remove poll, fix REST (chosen)
- **Pros**: Eliminates duplicate frames, single encoding path (V5), removes ~200 LOC.
- **Cons**: No fallback if push stalls (mitigated by watchdog).

### Option 2: Keep both, unify to V5
- **Pros**: Redundancy. **Cons**: Still duplicates with different values; two code paths.

### Option 3: REST-only polling
- **Pros**: Simple. **Cons**: Incompatible with 60fps physics visualisation.

## Decision

**Option 1: Keep push path, remove poll path, fix REST merge.**

1. **Remove poll timer**: Delete subscription timer and V3 encoding in
   `position_updates.rs`. Remove the `GraphStateActor` read path it uses.
2. **Push is sole real-time channel**: No changes to the existing push path.
3. **Fix `/api/graph/data`**: Merge live GPU positions when physics pipeline is
   active; fall back to `GraphStateActor` when GPU is unavailable. Add
   `x-position-source: gpu|state-actor` response header.
4. **Retain `/api/graph/positions`**: Lightweight JSON snapshot for client bootstrap.
5. **Remove V3 encoding** if no other consumers exist.

## Consequences

### Positive
- One position frame per physics tick per connection; no protocol conflicts.
- `/api/graph/data` consistent with the WebSocket stream.
- ~200 lines of dead poll infrastructure removed.

### Negative
- Push stall means no updates until recovery. Mitigation: watchdog in
  `ClientCoordinatorActor` logs warning after 2s silence.
- Undiscovered V3 consumers will break. Grep before removal.

### Neutral
- WebSocket lifecycle, authentication, and GPU compute pipeline unchanged.

## Related Decisions
- ADR-012: WebSocket Store Decomposition
- ADR-013: Render Performance

## References
- `src/handlers/socket_flow_handler/position_updates.rs`
- `src/actors/gpu/force_compute_actor.rs`
- `src/actors/physics_orchestrator_actor.rs`
- `src/handlers/socket_flow_handler/http_handler.rs`
- `src/utils/binary_protocol.rs`
