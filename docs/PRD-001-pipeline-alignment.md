# PRD-001: VisionFlow Data Pipeline Alignment

> **Wire-format clauses superseded by [ADR-061](adr/ADR-061-binary-protocol-unification.md) /
> [PRD-007](PRD-007-binary-protocol-unification.md) (2026-04-30)** — references to V3/V5
> wire formats and the legacy versioning vocabulary below reflect the historical state.
> The current single binary protocol (no versioning) is documented at
> [docs/binary-protocol.md](binary-protocol.md). Other consolidation work in this PRD
> (node classification, settings handlers, graph loading) remains as historical record.

**Status**: Draft (wire-format sections superseded 2026-04-30)
**Author**: Tech Debt Research Fleet
**Date**: 2026-04-12
**Priority**: P0 — Blocks correct rendering for all users
**Companion ADRs:** [ADR-037](adr/superseded/ADR-037-binary-protocol-consolidation.md) (superseded), [ADR-038](adr/ADR-038-position-flow-consolidation.md), [ADR-061](adr/ADR-061-binary-protocol-unification.md)
**DDD Context:** [Binary Protocol](ddd-binary-protocol-context.md)

---

## Problem Statement

VisionFlow's data pipeline from Neo4j through Rust server to TypeScript client has accumulated 49 parallel implementations across 5 subsystems. These redundancies cause:

1. **Silent data loss**: Ontology nodes classified as knowledge nodes due to case mismatch (`"OwlClass"` vs `"owl_class"`)
2. **Phantom protocols**: Client `BinaryWebSocketProtocol.ts` describes V4 as current; server always sends V3/V5
3. **Duplicate delivery**: Push (V5) and poll (V3) paths both active on same WebSocket session
4. **Settings black hole**: Two handlers mounted for `PUT /api/settings/physics` — one drops GPU propagation
5. **Zero positions on connect**: REST `/api/graph/data` returns GraphStateActor positions (Neo4j zeros) before GPU writes back

## Goals

- **Single node classification function** on server, single matching function on client
- **One binary wire format** (with optional sequence wrapper for backpressure) — *historical: superseded by ADR-061's single binary protocol*
- **Two position delivery paths**: REST snapshot (new client) + WebSocket push (real-time)
- **One physics settings struct chain**: Config → SimParams → GPU, with one API handler
- **One graph loading path** per trigger type, with correct node_type propagation

## Non-Goals

- Changing the CUDA kernel interface or GPU buffer layout
- Redesigning the actor system topology
- Adding new features (ontology dual-label, position persistence)
- Changing the REST API surface (keep endpoints, fix implementations)

---

## Scope

### Area 1: Node Type System (11 → 2)

**Current state**: 11 different places set, read, or classify node type using inconsistent string literals.

**Target**:
- One `classify_node_population(node_type: &str) -> NodePopulation` function in `src/models/graph_types.rs`
- One `getNodePopulation(typeStr: string): NodePopulation` in `client/src/types/nodeTypes.ts`
- Canonical string constants table shared between both

**Changes required**:
| File | Change |
|------|--------|
| `src/models/graph_types.rs` | Add `classify_node_population()` with exhaustive match including `"OwlClass"` |
| `src/actors/gpu/force_compute_actor.rs:487-511` | Replace inline match with `classify_node_population()` |
| `src/actors/graph_state_actor.rs:190-218` | Replace inline match with `classify_node_population()` |
| `src/handlers/api_handler/graph/mod.rs:162-176` | Replace inline match with `classify_node_population()` |
| `src/handlers/bots_visualization_handler.rs:377` | Replace inline match with `classify_node_population()` |
| `src/adapters/neo4j_adapter.rs:573` | Normalize `"OwlClass"` → `"owl_class"` at source |
| `src/services/ontology_enrichment_service.rs:240` | Use constant from `graph_types.rs` |
| `client/src/types/binaryProtocol.ts:96-103` | Import from shared `nodeTypes.ts` |
| `client/src/features/graph/hooks/useGraphVisualState.ts:146-163` | Use `getNodePopulation()` |
| `client/src/store/websocket/textMessageHandler.ts:95` | Normalize field name on receipt |

### Area 2: Binary Protocol (14 → 3)

**Historical state (pre-ADR-061)**: 14 encoding paths, 4 named wire-format variants defined, server always sent the canonical 48-byte node payload.

**Target**:
- `encode_positions_v3(nodes, type_ids, analytics) -> Vec<u8>` — the one encoder
- `wrap_v5(data, sequence) -> Vec<u8>` — optional backpressure wrapper
- Remove: V2 encoding, V4 delta encoding, `encode_node_data()`, `encode_node_data_with_flags()`, `encode_node_data_with_types()`, `encode_node_data_with_analytics()`, `encode_node_data_with_all()`
- Client: Remove `BinaryWebSocketProtocol.ts` MessageType-based protocol (it's a phantom — server never sends those frames)

**Changes required**:
| File | Change |
|------|--------|
| `src/utils/binary_protocol.rs` | Consolidate to `encode_positions_v3()` + `wrap_v5()`. Remove 6 wrapper functions |
| `src/utils/delta_encoding.rs` | Keep but disable; add `#[allow(dead_code)]` for future re-enable |
| `src/actors/client_coordinator_actor.rs` | Use single `encode_positions_v3()` in both `serialize_positions()` methods |
| `src/handlers/socket_flow_handler/position_updates.rs` | Remove subscription-based polling encode path (lines ~560-590) |
| `client/src/types/binaryProtocol.ts` | Remove V2/V5 constants and parsers. Keep V3 + V4 (for future delta) |
| `client/src/services/BinaryWebSocketProtocol.ts` | Remove or gut — its MessageType protocol is not used by server |
| `client/src/store/websocket/binaryProtocol.ts` | Remove legacy dispatch; parse V3 only, accept V5 as V3-with-sequence |

### Area 3: Position Data Flow (7 → 2)

**Current state**: Push path (ForceComputeActor → Supervisor → Orchestrator → CCA → WebSocket V5) AND poll path (timer → GraphStateActor → encode V3 → WebSocket) both active simultaneously.

**Target**:
- **WebSocket push** (real-time): ForceComputeActor → GraphServiceSupervisor → PhysicsOrchestratorActor → ClientCoordinatorActor → WebSocket
- **REST snapshot** (new client connect): `GET /api/graph/positions` reads from ForceComputeActor directly
- **Remove**: Subscription-based polling timer in `position_updates.rs` that reads from GraphStateActor

**Changes required**:
| File | Change |
|------|--------|
| `src/handlers/socket_flow_handler/position_updates.rs` | Remove `start_subscription_position_updates()` polling timer |
| `src/actors/graph_state_actor.rs:788` | Keep `UpdateNodePositions` handler (stores positions for REST reads) |
| `src/actors/physics_orchestrator_actor.rs` | Keep as-is (the push broadcast relay) |
| `src/handlers/api_handler/graph/mod.rs:497` | Keep `/api/graph/positions` reading from ForceComputeActor |
| `src/handlers/api_handler/graph/mod.rs:99` | Fix `/api/graph/data` to merge GPU positions if available |

### Area 4: Settings/Physics (7 → 3)

**Current state**: `PhysicsSettings`, `SimParams`, `SimulationParams`, `PhysicsSettingsDTO`, `UpdatePhysicsRequest`, plus two client interfaces with the same name.

**Target**:
- `PhysicsSettings` (config/persistence) — the source of truth
- `SimParams` (GPU wire format) — keep #[repr(C)], 172 bytes
- One client `PhysicsConfig` interface (full field set)
- One `PUT /api/settings/physics` handler that propagates to GPU

**Changes required**:
| File | Change |
|------|--------|
| `src/handlers/api_handler/settings/mod.rs` | Remove duplicate handler; route to `settings_routes.rs` handler |
| `src/handlers/settings_handler/types.rs:332` | Remove `PhysicsSettingsDTO`; use `PhysicsSettings` directly |
| `client/src/api/settingsApi.ts:50` | Replace 11-field `PhysicsSettings` with full `PhysicsConfig` |
| `client/src/features/settings/config/settings.ts:46` | Rename to `PhysicsConfig`, make canonical |

### Area 5: Graph Loading (10 → 4)

**Current state**: Two Neo4j adapters, multiple CQRS handlers, `AddNodesFromMetadata` doesn't set `node_type`.

**Target**:
- **File→Neo4j**: `load_graph_from_files_into_neo4j()` (keep as-is)
- **Neo4j→GraphState**: `neo4j_graph_repository.load_graph()` — single adapter
- **GraphState→GPU**: Single `UpdateGPUGraphData` fan-out through `GraphServiceSupervisor`
- **Incremental**: `AddNodesFromMetadata` populates `node_type` from metadata

**Changes required**:
| File | Change |
|------|--------|
| `src/adapters/neo4j_adapter.rs` | Delegate to `neo4j_graph_repository` or normalize type strings |
| `src/actors/graph_state_actor.rs` | In `AddNodesFromMetadata` handler, extract `node_type` from metadata map |
| `src/actors/graph_service_supervisor.rs` | Ensure `AnalyticsSupervisor` receives graph data on all reload paths |

---

## Success Criteria

1. All node types visible in client match their Neo4j `node_type` field
2. Only V3 binary frames received by client (no V2/V4/V5 legacy)
3. No duplicate position frames per physics tick
4. Physics slider changes immediately affect GPU simulation
5. Zero `NodeType.Unknown` nodes in client when `node_type` is set in Neo4j

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breaking existing WebSocket connections | Feature flag: keep legacy parsers but don't encode legacy |
| GPU buffer layout change | Out of scope — SimParams stays 172 bytes |
| Client-side caching of stale positions | REST `/api/graph/positions` always reads from GPU |
| Delta encoding needed later | Keep `delta_encoding.rs` intact but unused |

## Implementation Order

1. **Phase 1** (Node types + Graph loading): Fix data correctness first
2. **Phase 2** (Binary protocol + Position flow): Fix delivery paths
3. **Phase 3** (Settings): Fix control path
4. **Phase 4** (Cleanup): Remove dead code, update tests

---

## Appendix: Complete Redundancy Map

### Encoding Function Call Graph (Server)

```mermaid
graph TD
    A["encode_node_data()"] --> B["encode_node_data_with_types(nodes, &[], &[])<br/><i>no type flags</i>"]
    B --> C["encode_node_data_extended(nodes, agents, knowledge, &[], &[], &[])"]
    C --> D["encode_node_data_extended_with_sssp(... None, None)<br/><b>THE ENCODER</b>"]

    E["encode_node_data_with_flags(nodes, agents)"] --> F["encode_node_data_with_types(nodes, agents, &[])<br/><i>no ontology flags</i>"]
    F --> C

    G["encode_node_data_with_analytics(nodes, analytics)"] --> H["encode_node_data_with_all(nodes, agents, knowledge, analytics)"]
    H --> D

    I["encode_node_data_with_live_analytics(nodes, analytics)"] --> J["encode_node_data_extended_with_sssp(nodes, &[], &[], &[], &[], &[], None, analytics)<br/><i>ALL TYPE ARRAYS EMPTY - all nodes Unknown on client</i>"]
```

**Target**: One function: `encode_positions_v3(nodes, type_classification, analytics)`
