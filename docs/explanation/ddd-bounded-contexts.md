---
title: DDD Bounded Contexts
description: Domain-Driven Design context map for VisionClaw — aggregate boundaries, context relationships, and anti-corruption layers between Authentication, Graph Data, Physics Compute, and Ontology domains
category: explanation
tags: [ddd, bounded-contexts, domain, architecture, context-map]
updated-date: 2026-04-09
---

# DDD Bounded Contexts - VisionClaw

> **ADR-2004 update**: Wherever this map names "Neo4j" as a persistence target, the live
> backing store is the **embedded Oxigraph** RDF triple store (in-process, RocksDB-backed;
> settings in SQLite). The repository **ports** are unchanged — only the adapter
> implementations differ (`OxigraphGraphRepository`, `SqliteSettingsRepository`).
>
> **Correction (2026-07-22 doc-drift audit): scope widened.** The ADR-2004
> update above applies to this **entire document**, not only the literal
> string "Neo4j" — every bounded-context entry below (BC1-BC10, both Mermaid
> diagrams, and any persistence/port description) should be read as
> Oxigraph+SQLite-backed unless a context is explicitly named otherwise. No
> BC in this map has a live Neo4j dependency on `main`.

## Context Map

```mermaid
graph LR
    Auth["Authentication"] --- GD["Graph Data"] --- PC["Physics Compute"]
    Auth --- Set["Settings"]
    GD --- WS["WebSocket Comm"]
    PC --- GPU["GPU Compute"]
    Set --- WS --- GPU
    Set --- An["Analytics"]
    WS --- Ont["Ontology"]
    GPU --- AB["Agent/Bot Mgmt"]
    An --- Ont --- AB
    An --- Ren["Rendering"]
    Ont --- BP["Binary Protocol"]
    Ren --- BP
```

*Context map showing the ten bounded contexts and their upstream/downstream relationships across Authentication, Data, Physics, and Presentation layers.*

%%{init: {'theme': 'base', 'themeVariables': {'primaryColor': '#4A90D9', 'primaryTextColor': '#fff', 'lineColor': '#2C3E50'}}}%%
```mermaid
graph LR
    subgraph "Core Domain"
        GD[Graph Data\nBC2]
        Phys[Physics Compute\nBC3]
        Onto[Ontology & Reasoning\nBC7]
    end
    subgraph "Supporting Domain"
        Auth[Authentication &\nAuthorization BC1]
        WS[WebSocket Comm\nBC4]
        Cfg[Settings &\nConfiguration BC5]
        BP[Binary Protocol\nBC10]
    end
    subgraph "Generic Domain"
        Ana[Analytics &\nMonitoring BC6]
        Agents[Agent/Bot\nManagement BC8]
        Render[Client Rendering\nBC9]
    end

    Auth -->|upstream - guards| GD
    Auth -->|upstream - guards| WS
    GD -->|feeds positions| Phys
    GD -->|feeds axioms| Onto
    Onto -->|constraints| Phys
    Phys -->|settled positions| BP
    BP -->|binary frames| WS
    WS -->|delivers| Render
    GD -->|node/edge metadata| Render
    Cfg -->|tune params - upstream| Phys
    Cfg -->|feature flags| Auth
    Ana -->|cluster/anomaly scores| BP
    Agents -->|agent nodes| GD
    Agents -->|telemetry| Ana
```

## BC1: Authentication & Authorization
**Aggregate Root**: `NostrSession`
**Entities**: `UserSession`, `ApiKey`, `FeatureAccess`
**Services**: `NostrService`, `AuthExtractor`
**Ports**: `AuthenticationPort`, `SessionStore`
**Key Files**: `nostr_service.rs`, `auth_extractor.rs`, `nostr_handler.rs`,
`nostrAuthService.ts`
**Invariants**:
- Private keys never persisted to browser storage
- All mutation endpoints require valid session
- Session tokens compared constant-time

## BC2: Graph Data Management
**Aggregate Root**: `GraphData`
**Entities**: `Node`, `Edge`, `GraphMetadata`
**Services**: `GraphStateActor`, `GraphDataManager`, `FileService`
**Ports**: `GraphRepository` (embedded Oxigraph), `GraphPersistence`
**Key Files**: `graph_state_actor.rs`, `adapters/graph_repository.rs` (OxigraphGraphRepository), `graphDataManager.ts`,
`graph.worker.ts`
**Invariants**:
- Single position source of truth (no data.x vs x duplication)
- Persist-first-then-mutate for critical operations
- Single ingestion pipeline for all data sources
- Node IDs coerced to String everywhere

## BC3: Physics Compute
**Aggregate Root**: `PhysicsSimulation`
**Entities**: `SimulationParams`, `ForceConfig`, `ConstraintData`
**Services**: `PhysicsOrchestratorActor`, `ForceComputeActor`
**Key Files**: `physics_orchestrator_actor.rs`, `force_compute_actor.rs`,
`stress_majorization.rs`, `simd_forces.rs`
**Invariants**:
- Step completion tracked by state machine with IDs
- Mass values always > epsilon before SIMD division
- Sparse representation for large graph distances

## BC4: WebSocket Communication
**Aggregate Root**: `WebSocketConnection`
**Entities**: `ConnectionState`, `MessageQueue`, `BinaryFrame`
**Services**: `ConnectionLifecycle`, `BinaryProtocol`, `MessageQueue`
**Key Files**: `websocketStore.ts`, `socket_flow_handler/`, `binary_protocol.rs`
**Invariants**:
- Auth before any protocol activity
- Binary payloads capped before allocation
- Update intervals clamped server-side
- No module-level mutable state

## BC5: Settings & Configuration
**Aggregate Root**: `AppSettings`
**Entities**: `PhysicsSettings`, `VisualSettings`, `FeatureFlags`
**Services**: `SettingsStore`, `SettingsApi`
**Key Files**: `settingsStore.ts`, `settingsApi.ts`, `app_settings.rs`,
`feature_access.rs`
**Invariants**:
- One authoritative source per setting category
- No .env file mutation at runtime
- Feature flags persisted in database, not process-local statics

## BC6: Analytics & Monitoring
**Aggregate Root**: `AnalyticsState`
**Entities**: `ClusteringTask`, `AnomalyState`, `GpuMetrics`
**Services**: `AnalyticsStore`, health handlers
**Key Files**: `analytics/state.rs`, `analyticsStore.ts`, `consolidated_health_handler.rs`
**Invariants**:
- GPU metrics are real or explicitly marked unavailable
- Feature flag changes atomic with backend state
- Health checks non-blocking (cached background results)

## BC7: Ontology & Reasoning
**Aggregate Root**: `Ontology`
**Entities**: `OntologyAxiom`, `InferredRelation`, `OntologyClass`
**Services**: `OntologyPipelineService` (reasoning runs through `WhelkInferenceEngine`)
**Key Files**: `ontology_pipeline_service.rs`, `ontology_actor.rs`
**Invariants**:
- Depth/descendant computation has cycle detection
- Single API surface (remove duplicate ontology handlers)
- Inferred axioms deduplicated

## BC8: Agent/Bot Management
**Aggregate Root**: `AgentSwarm`
**Entities**: `Agent`, `SwarmStatus`, `TelemetryMessage`
**Services**: `AgentPollingService`, `BotsDataContext`
**Key Files**: `BotsControlPanel.tsx`, `AgentPollingService.ts`, `BotsVisualization.tsx`
**Invariants**:
- Polling stop only when subscriber count = 0
- WebSocket URLs absolute (ws://)
- No remote script injection
- Per-frame rendering zero-allocation

## BC9: Client Rendering
**Aggregate Root**: `GraphScene`
**Entities**: `GemNode`, `GlassEdge`, `ClusterHull`, `SceneEffect`
**Services**: `GraphManager`, `GraphWorkerProxy`, `WasmSceneEffects`
**Key Files**: `GraphManager.tsx`, `GemNodes.tsx`, `GlassEdges.tsx`,
`scene-effects-bridge.ts`
**Invariants**:
- No allocations in useFrame
- InstancedMesh capacity handles overflow
- useMemo never used for side effects
- Three.js resources disposed in useEffect cleanup

## BC10: Binary Protocol
**Aggregate Root**: `BinaryMessage`
**Entities**: `NodeUpdate`, `DeltaFrame`, `ProtocolVersion`
**Services**: `BinaryProtocolService`, `DeltaEncoding`
**Key Files**: `binary_protocol.rs`, `delta_encoding.rs`,
`BinaryWebSocketProtocol.ts`
**Invariants**:
- Max payload size enforced before allocation
- One canonical wire format spec (generated from code)
- Version byte validated before processing
