---
title: VisionClaw Actor System Hierarchy
description: Complete reference for VisionClaw's 23-actor Actix-Web supervision tree, covering actor roles, message protocols, and supervision strategies
category: explanation
tags: [actors, actix, concurrency, supervision, rust]
updated-date: 2026-06-03
---

# VisionClaw Actor System Hierarchy

> **Superseded for the actor set (2026-09-05).** Commit 346fff7af trimmed the actor tree and ADR-2045/2046/2053/2097 removed the generic `SupervisorActor`, `ActorLifecycleManager`, the dead `SettingsActor`, the standalone analytics actors and `RefreshMetadata`. The authoritative picture is `docs/BASELINE-architecture.md` (actors section) and the sequence diagrams in `docs/diagrams/visionclaw/02-actor-supervision.md` in the VisionFlow estate tree (see `docs/diagrams/README.md`); this explainer is kept as narrative history until it is rewritten against them.

> **Updated 2026-06-03.** Wire format corrected from 34 B (V1) to 52 B V3
> (ADR-031). `ForceComputeActor` profile updated to describe display-only disc
> projection. Settings change pipeline diagram updated to the verified single
> debounced path (T2 resolved). Position broadcast pipeline diagram updated to
> the verified FC → GSS → CCA route with disc projection. See
> `docs/architecture/diagrams/` for the authoritative verified diagrams.

VisionClaw's Rust backend coordinates 23 specialised Actix actors under a hierarchical supervision tree. Actors handle all concurrent concerns — WebSocket client management, GPU physics coordination, graph state, semantic processing, GitHub ingestion, agent monitoring, and task orchestration — without shared mutable state. Communication is exclusively via typed asynchronous messages.

> **Note**: `GraphServiceActor` no longer exists as an active component. It was decomposed into the current 23-actor hierarchy and replaced by `GraphServiceSupervisor` (which supervises `GraphStateActor` and `GraphUpdateActor`) backed by the embedded Oxigraph store (ADR-2004). Any reference to `GraphServiceActor` in older documentation is historical.

---

## 1. Overview

The actor system is built on Actix-Web's arbiter model. Each actor owns private state and processes one message at a time from its mailbox. Concurrency arises from many actors running in parallel across the Tokio thread pool. The supervision tree ensures failures are isolated: a CUDA OOM in `ForceComputeActor` restarts only the physics sub-actors, not the graph state or WebSocket connections.

Actors serve as the **Presentation Layer** of the hexagonal architecture. They receive external inputs (HTTP requests, WebSocket messages, timer ticks, GPU callbacks) and translate them into domain operations by dispatching commands and queries through the CQRS bus. Actors never access the graph store or the GPU directly — they go through port trait adapters.

---

## 2. Actor Supervision Tree

```mermaid
graph TD
    Arbiter["Actix Arbiter<br/>(Tokio runtime)"]

    Arbiter --> AppRoot["app_state.rs bootstrap<br/>(actors started directly with .start();<br/>generic SupervisorActor available for parenting,<br/>src/actors/supervisor.rs)"]

    AppRoot --> GSS["GraphServiceSupervisor<br/>OneForOne strategy"]
    AppRoot --> PhysicsOrch["PhysicsOrchestratorActor"]
    AppRoot --> ClientCoord["ClientCoordinatorActor"]
    AppRoot --> GitHubSync["GitHubSyncActor"]
    AppRoot --> SemanticProc["SemanticProcessorActor"]
    AppRoot --> OptSettings["OptimizedSettingsActor"]
    AppRoot --> Workspace["WorkspaceActor"]
    AppRoot --> Ontology["OntologyActor"]

    GSS --> GraphState["GraphStateActor"]
    GSS --> GraphUpdate["GraphUpdateActor"]

    PhysicsOrch --> GPUMgr["GPUManagerActor<br/>(GPU coordinator)"]

    GPUMgr --> ResourceSup["ResourceSupervisor"]
    GPUMgr --> PhysicsSup["PhysicsSupervisor<br/>AllForOne strategy"]
    GPUMgr --> AnalyticsSup["AnalyticsSupervisor<br/>OneForOne strategy"]
    GPUMgr --> GraphAnalSup["GraphAnalyticsSupervisor<br/>OneForOne strategy"]

    ResourceSup --> GPUResource["GPUResourceActor"]

    PhysicsSup --> ForceCompute["ForceComputeActor"]
    PhysicsSup --> StressMaj["StressMajorizationActor"]
    PhysicsSup --> ConstraintA["ConstraintActor"]
    PhysicsSup --> OntConst["OntologyConstraintActor"]
    PhysicsSup --> SemForces["SemanticForcesActor"]

    AnalyticsSup --> Clustering["ClusteringActor"]
    AnalyticsSup --> AnomalyDet["AnomalyDetectionActor"]
    AnalyticsSup --> PageRank["PageRankActor"]

    GraphAnalSup --> ShortPath["ShortestPathActor"]
    GraphAnalSup --> ConnComp["ConnectedComponentsActor"]

    ClientCoord --> WSSessions["WebSocketSession × N<br/>(one per client)"]
```

The 23 named actors (excluding the ephemeral `WebSocketSession × N` instances):

1. `GraphStateActor`
2. `GraphUpdateActor`
3. `PhysicsOrchestratorActor`
4. `GPUManagerActor`
5. `GPUResourceActor`
6. `ForceComputeActor`
7. `StressMajorizationActor`
8. `ConstraintActor`
9. `OntologyConstraintActor`
10. `SemanticForcesActor`
11. `ClusteringActor`
12. `AnomalyDetectionActor`
13. `PageRankActor`
14. `ShortestPathActor`
15. `ConnectedComponentsActor`
16. `ClientCoordinatorActor`
17. `SemanticProcessorActor`
18. `OptimizedSettingsActor`
19. `WorkspaceActor`
20. `OntologyActor`
21. `GitHubSyncActor`
22. `AgentMonitorActor`
23. `TaskOrchestratorActor`

---

## 3. Actor Profiles

### GraphStateActor

| Property | Value |
|---|---|
| Supervisor | `GraphServiceSupervisor` |
| Mailbox | Bounded (capacity 1000) |
| Lines of code | ~797 |

**State machine**: `Uninitialized → Initializing → Loading → Ready ↔ Updating ↔ Simulating → Error → Recovering`

Key messages handled:
- `GetGraphData` → `Arc<GraphData>` (request-response)
- `AddNode { node: Node }` → `u32`
- `RemoveNode { id: u32 }` → `()`
- `UpdatePositions { updates: Vec<(u32, Vec3)> }` → `()`
- `GetNode { id: u32 }` → `Option<Node>`

Key messages sent:
- Publishes `GraphUpdateEvent` to `SemanticProcessorActor` subscribers
- Sends `BroadcastPositions` to `ClientCoordinatorActor` after position commits

Critical state: in-memory graph cache, metadata-ID-to-node-ID map, checkpoint timestamps.

---

### GraphUpdateActor

| Property | Value |
|---|---|
| Supervisor | `GraphServiceSupervisor` |
| Mailbox | Unbounded |

Handles incremental graph diffs from the GitHub sync pipeline, coalescing batched node/edge updates before forwarding to `GraphStateActor`. Prevents `GraphStateActor` from processing individual file-level mutations during large syncs.

Key messages handled: `ApplyGraphDiff { diff: GraphDiff }`, `FlushPendingUpdates`.

---

### PhysicsOrchestratorActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Bounded (capacity 128) |

Coordinates the full GPU physics pipeline per simulation tick. Fans out to `GPUManagerActor`, awaits results, then broadcasts positions to clients. Uses `join!` for parallel force computation paths.

Orchestration sequence per tick:
1. `GPUManagerActor` ← `SimulationStep`
2. `GPUResourceActor` ← `AllocateStream`
3. `PhysicsSupervisor/ForceComputeActor` ← `ComputeForces(stream)`
4. `PhysicsSupervisor/SemanticForcesActor` ← `ComputeSemanticForces(stream)` (parallel with step 3)
5. Merge force vectors
6. `PhysicsSupervisor/ConstraintActor` ← `ApplyConstraints(combined)`
7. `PhysicsSupervisor/OntologyConstraintActor` ← `ApplyOntologyConstraints`
8. `ForceComputeActor` ← `UpdatePositions`
9. `ClientCoordinatorActor` ← `BroadcastPositions(binary)`

---

### GPUManagerActor

| Property | Value |
|---|---|
| Supervisor | `PhysicsOrchestratorActor` |
| Mailbox | Bounded (capacity 64) |

Routes messages to the four GPU sub-supervisors. Tracks GPU memory budget across supervisors. Aggregates health status for diagnostics. Coordinates cascading restart on critical GPU failures.

Key messages handled: `SimulationStep`, `RunAnalytics`, `AllocateMemory`, `GetGpuHealth`.

---

### GPUResourceActor

| Property | Value |
|---|---|
| Supervisor | `ResourceSupervisor` (under `GPUManagerActor`) |
| Mailbox | Bounded |

Manages the CUDA stream pool and device memory allocations. All GPU actors request streams through this actor before launching kernels.

Key messages: `AllocateStream` → `StreamHandle`, `ReleaseStream`, `GetMemoryStats`.

---

### ForceComputeActor

| Property | Value |
|---|---|
| Supervisor | `PhysicsSupervisor` (AllForOne) |
| Mailbox | Bounded (capacity 256) |
| CUDA kernels | See `crates/visionclaw-gpu/src/cuda_sources/` (source count varies by build) |
| Typical latency | 4ms/step |

Barnes-Hut O(n log n) force calculation with Verlet integration. Owns the primary node position CUDA buffers. Runs a warmup window (1800 steps stability-bypass) before the GPU stability kernel is allowed to suppress steps. Emits periodic full-broadcast (every 300 iterations) to prevent client position starvation after convergence.

**Dual-graph disc projection (display-only, 2026-06-03):** After each GPU readback, `ForceComputeActor` applies a display-only facing-disc transformation before broadcasting: Knowledge nodes are flattened to a disc at `z = −graph_separation_x`, Ontology nodes at `z = +graph_separation_x`, Agents remain at `z = 0`. The transformation is applied to the broadcast buffer only and undone from `last_good_positions` before the next physics step — the GPU state never sees the disc offsets. The rim clamp radius (`DISC_RIM_RADIUS = 2600.0`) is decoupled from the separation so close discs remain full-size. The same projection runs independently in the `ForceFullBroadcast` handler. See `docs/architecture/diagrams/06-gpu-physics.md`.

Key messages: `ComputeForces`, `UpdateNodePositions`, `UpdateSimulationParams { params: SimulationParams }`, `ForceFullBroadcast`, `ResetPositions`.

On CUDA OOM: calls `ctx.stop()` to trigger `PhysicsSupervisor` AllForOne restart.

---

### StressMajorizationActor

| Property | Value |
|---|---|
| Supervisor | `PhysicsSupervisor` (AllForOne) |
| CUDA kernels | 4 |
| Typical latency | 8ms/step |

Global layout optimisation via energy minimisation. Invoked optionally by `PhysicsOrchestratorActor` when layout quality metric drops below threshold.

---

### ConstraintActor

| Property | Value |
|---|---|
| Supervisor | `PhysicsSupervisor` (AllForOne) |
| CUDA kernels | 5 |
| Typical latency | 1ms/step |

Hard spatial constraints — collision detection, boundary enforcement, user-pinned node positions.

---

### OntologyConstraintActor

| Property | Value |
|---|---|
| Supervisor | `PhysicsSupervisor` (AllForOne) |
| CUDA kernels | 5 |
| Typical latency | 2.3ms/step |

Receives `ConstraintsGeneratedEvent` from the ontology pipeline and uploads OWL-derived physics forces to GPU. `SubClassOf` axioms become attraction forces; `DisjointWith` axioms become repulsion forces.

---

### SemanticForcesActor

| Property | Value |
|---|---|
| Supervisor | `PhysicsSupervisor` (AllForOne) |
| CUDA kernels | 15 |
| Typical latency | 3ms/step |

AI-driven semantic clustering forces. Uses content embeddings (256-dim) generated by `SemanticProcessorActor` to pull semantically similar nodes together.

---

### ClusteringActor

| Property | Value |
|---|---|
| Supervisor | `AnalyticsSupervisor` (OneForOne) |
| CUDA kernels | 12 |
| Typical latency | 20ms |

K-means, Louvain, and label propagation community detection. On-demand; not part of the per-tick physics loop.

---

### AnomalyDetectionActor

| Property | Value |
|---|---|
| Supervisor | `AnalyticsSupervisor` (OneForOne) |
| CUDA kernels | 4 |
| Typical latency | 10ms |

LOF and Z-score outlier identification. Used for surface-level diagnostics and anomaly highlighting in the visualisation.

---

### PageRankActor

| Property | Value |
|---|---|
| Supervisor | `AnalyticsSupervisor` (OneForOne) |
| CUDA kernels | 5 |
| Typical latency | 5ms |

GPU PageRank centrality scoring. Results feed node sizing and colouring in the client.

---

### ShortestPathActor

| Property | Value |
|---|---|
| Supervisor | `GraphAnalyticsSupervisor` (OneForOne) |
| CUDA kernels | 4 |
| Typical latency | 15ms |

SSSP and APSP via GPU Dijkstra and BFS. Supports landmark-based approximate APSP for large graphs.

---

### ConnectedComponentsActor

| Property | Value |
|---|---|
| Supervisor | `GraphAnalyticsSupervisor` (OneForOne) |
| CUDA kernels | 3 |
| Typical latency | 3ms |

Union-Find connected component labelling. Used by the client to visualise disconnected subgraphs.

---

### ClientCoordinatorActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Unbounded |
| Lines of code | ~1,593 |

Central WebSocket multiplexer. Manages the registry of active `WebSocketSession` instances. Serialises node positions into the **52-byte V3 binary wire protocol** (ADR-031, little-endian; analytics fields cluster_id@36, anomaly_score@40, community_id@44, centrality@48) and broadcasts at adaptive rates: 60 FPS when physics is active, 5 Hz when converged.

> **Updated 2026-06-03:** The previously cited "34-byte" record was the
> pre-ADR-031 V1 format. The live protocol is 52 B V3. See
> `docs/architecture/diagrams/05-wire-analytics-types.md` for the full layout.

Key messages handled:
- `RegisterClient { id: ClientId, addr: Addr<WebSocketSession> }` → `()`
- `DeregisterClient { id: ClientId }` → `()`
- `BroadcastPositions { binary: Bytes }` → `()` (fire-and-forget from PhysicsOrchestratorActor)
- `ForwardPositions { binary: Bytes }` → `()` (fan-out to all sessions)

Key state: `HashMap<ClientId, Addr<WebSocketSession>>`, bandwidth throttle counters per client.

---

### SemanticProcessorActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Bounded (capacity 64) |

AI and ML feature extraction. Generates 256-dim content embeddings, topic classifications, importance scores, and similarity-based constraints that feed into `SemanticForcesActor`.

Subscribed to `GraphUpdateEvent` from `GraphStateActor`; invalidates embedding cache for changed nodes.

---

### OptimizedSettingsActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Unbounded |

Thin actor wrapper over the `SettingsRepository` port. Provides actor-addressable settings access for components that cannot use `async_trait` directly. Uses `SqliteSettingsRepository` as the backing adapter with LRU TTL caching (default 300s, ~90× speedup for cache hits).

---

### WorkspaceActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Unbounded |

Multi-tenant workspace management. Routes graph operations to the correct per-workspace repository instance. Enforces workspace-level access control.

---

### OntologyActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Bounded (capacity 32) |

Orchestrates the OWL reasoning pipeline: triggers `WhelkInferenceEngine`, caches inferred axioms, and emits `ReasoningCompleteEvent` to the pipeline service. Receives `OntologyModifiedEvent` from `GitHubSyncActor`.

---

### GitHubSyncActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Bounded (capacity 8) |

Wraps the `GitHubSyncService` domain service. Invoked at startup (after the embedded Oxigraph store opens, before the HTTP server starts) and on-demand via the admin API. Emits `OntologyModifiedEvent` on successful file ingestion to trigger the downstream reasoning pipeline.

Startup sequence:
1. `GitHubSyncService::sync_graphs()` — fetch changed `.md` files (SHA1 dedup)
2. Route each file to `KnowledgeGraphParser` (trigger: `public:: true`) or `OntologyParser` (trigger: `### OntologyBlock`)
3. Persist parsed data to the embedded Oxigraph store via port adapters
4. Emit `OntologyModifiedEvent` → `OntologyActor`

---

### AgentMonitorActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Bounded (capacity 64) |

Monitors the health and lifecycle of all spawned agents. Tracks agent heartbeats, detects stalled or unresponsive agents, and reports health status via the `/api/health` diagnostic endpoint. Receives periodic health pings from active agents and maintains a registry of agent states.

---

### TaskOrchestratorActor

| Property | Value |
|---|---|
| Supervisor | `AppSupervisor` |
| Mailbox | Bounded (capacity 128) |

Manages concurrent task scheduling and execution across agents. Enforces the `MAX_CONCURRENT_TASKS` capacity limit (default 20, configurable via environment variable). Routes incoming task requests to available agents, tracks task progress, handles timeouts, and supports task cancellation. Provides task status via the metrics endpoint.

---

## 4. Inter-Actor Message Flow

### Position Broadcast Pipeline

> **Updated 2026-06-03.** Verified path: FC applies display-only disc projection
> to the broadcast buffer, sends to `GraphServiceSupervisor`, which routes to
> `ClientCoordinatorActor`. The projection is undone after broadcast.
> See `docs/architecture/diagrams/06-gpu-physics.md` for the full sequence.

```mermaid
sequenceDiagram
    participant FC as ForceComputeActor
    participant GSS as GraphServiceSupervisor
    participant CC as ClientCoordinatorActor
    participant WS as WebSocket clients

    loop Physics tick (≥ 2 ms interval, orchestrated by PhysicsOrchestratorActor)
        FC->>FC: CUDA kernel: compute forces + integrate positions
        FC->>FC: Divergence guard (NaN/Inf/OOB/KE check)
        FC->>FC: DISC PROJECTION (display-only):<br/>population_centroids_xy() → project_node_xy() per node<br/>Knowledge z=−sep, Ontology z=+sep, Agent z=0
        FC->>GSS: UpdateNodePositions (projected positions, 52 B V3 per node)
        GSS->>CC: BroadcastPositions
        CC->>WS: binary frame fan-out to all sessions
        FC->>FC: RESTORE from last_good_positions<br/>(GPU state unchanged, next step sees pristine physics)
    end

    note over FC,GSS: After FastSettle plateau: PhysicsOrchestratorActor<br/>sends ForceFullBroadcast → pure GPU snapshot, projected.<br/>Periodic full broadcast every 300 iters prevents starvation.
```

### Graph Update Pipeline (GitHub → Client)

```mermaid
sequenceDiagram
    participant GS as GitHubSyncActor
    participant OA as OntologyActor
    participant WI as WhelkInferenceEngine
    participant OCA as OntologyConstraintActor
    participant FC as ForceComputeActor
    participant GSA as GraphStateActor
    participant CC as ClientCoordinatorActor

    GS->>GSA: ApplyGraphDiff(nodes, edges)
    GS->>OA: OntologyModifiedEvent
    OA->>WI: infer_axioms(ontology)
    WI-->>OA: InferredAxioms
    OA->>OCA: ConstraintsGeneratedEvent
    OCA->>FC: GPUUploadCompleteEvent (new forces active)
    GSA->>CC: BroadcastGraphStructure (on topology change)
```

### Settings Change Pipeline

> **Updated 2026-06-03 (T2 resolved).** The diagram below shows the verified
> single-path flow. The previous diagram showed an incorrect CQRS/CommandBus
> path and `SA→FC` direct send; both have been removed. See
> `docs/architecture/diagrams/01-settings-flow.md` for the full sequence.

```mermaid
sequenceDiagram
    participant UI as React UI
    participant ASM as autoSaveManager<br/>(500 ms debounce)
    participant SApi as settingsApi
    participant HTTP as PUT /api/settings/physics<br/>settings_routes.rs
    participant OSA as OptimizedSettingsActor
    participant GSS as GraphServiceSupervisor
    participant Orch as PhysicsOrchestratorActor
    participant FC as ForceComputeActor
    participant SQLite as SQLite settings.sqlite3

    UI->>ASM: queueChange(path, value)
    ASM->>SApi: updateSettingsByPaths (single PUT, no duplicate)
    SApi->>HTTP: PUT /api/settings/physics
    HTTP->>OSA: UpdateSettings (in-memory + YAML)
    HTTP->>GSS: UpdateSimulationParams
    GSS->>Orch: UpdateSimulationParams (forwarded)
    Orch->>FC: UpdateSimulationParams (forwarded)
    HTTP->>SQLite: set_setting("physics")
    HTTP->>GSS: ForceResumePhysics
    note over FC: reheat_factor set, stability_warmup_remaining=1800<br/>warmup bypasses GPU stability suppression for 30 s
```

---

## 5. Supervision Strategies

| Supervisor | Strategy | Rationale |
|---|---|---|
| `AppSupervisor` | OneForOne | Top-level actors are independent; a settings failure must not restart the physics pipeline |
| `GraphServiceSupervisor` | OneForOne | `GraphStateActor` and `GraphUpdateActor` are independent |
| `PhysicsOrchestratorActor` | AllForOne (for GPU children) | GPU actors share CUDA device state; a CUDA OOM in one actor corrupts shared buffers |
| `ResourceSupervisor` | Restart with backoff | GPU init failures are retried before escalation |
| `PhysicsSupervisor` | AllForOne | Physics state is interdependent across force actors; restarts all 5 children when any fails since they share `SharedGPUContext` (upgraded from OneForOne in ADR-031 sprint) |
| `AnalyticsSupervisor` | OneForOne | Analytics algorithms are independent; clustering failure must not restart PageRank |
| `GraphAnalyticsSupervisor` | OneForOne | Path algorithms are independent |

### Restart Policy

All supervisors use exponential backoff with:

```rust
SupervisorStrategy::OneForOne {
    max_restarts: 3,
    within: Duration::from_secs(10),
    backoff: BackoffStrategy::Exponential {
        initial_interval: Duration::from_millis(500),
        max_interval: Duration::from_secs(5),
        multiplier: 2.0,
    }
}
```

If an actor exceeds 3 restarts in 10 seconds, the failure escalates to its parent supervisor.

### Actor Respawning via ActorFactory

`SupervisorActor` uses a type-erased `ActorFactory` trait to respawn child actors on failure. Each supervisor stores a `Box<dyn ActorFactory>` per child, enabling real actor recreation (with full `started()` lifecycle) rather than simple address re-registration. This ensures GPU actors re-execute their self-init sequence on respawn.

### Graceful Shutdown

Actors support the `InitiateGracefulShutdown` message, which triggers a drain-before-stop sequence: the actor finishes processing its current mailbox, flushes pending state (e.g., position checkpoints, cache writes), and then calls `ctx.stop()`. This prevents data loss during planned restarts or rolling deployments.

### Escalation Wiring

`GraphServiceSupervisor` sends `SetParentSupervisor` to its children during `started()`, wiring the escalation chain so that child actors can report unrecoverable failures to their parent supervisor. This message carries the parent's `Addr<SupervisorActor>`, enabling children to escalate without requiring a global actor registry.

---

## 6. Actor Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Created
    Created --> Starting : Actix spawns actor
    Starting --> Running : started() hook completes
    Running --> Processing : Message dequeued
    Processing --> Running : Handler returns
    Processing --> Error : Handler panics / returns Err
    Error --> Restarting : Supervisor decides restart
    Restarting --> Starting : New instance created
    Running --> Stopping : ctx.stop() or supervisor stop
    Stopping --> Stopped : stopping() + stopped() hooks
    Stopped --> [*]
```

**Lifecycle hooks**:

- `started()` — initialise resources, start timer intervals (e.g., `GraphStateActor` starts 60-second checkpointing interval here)
- `stopping()` — begin cleanup, flush pending state
- `stopped()` — release CUDA buffers, close DB connections, deregister from pub/sub

**Warm-up window** (`ForceComputeActor`): 600 frames (~10 seconds at 60 FPS). Physics is considered settled only after this window. The periodic full broadcast (every 300 iterations) ensures clients that connect after warm-up and convergence still receive positions.

---

## 7. GPU Supervisor Hierarchy

The GPU subsystem uses a 4-supervisor pattern under `GPUManagerActor`, providing fault isolation zones that prevent analytics failures from affecting physics.

```mermaid
graph TB
    subgraph "GPU Manager"
        GM["GPUManagerActor<br/>Routes messages<br/>Manages memory budget<br/>Aggregates health"]
    end

    subgraph "Zone 1 — Resources (critical)"
        RS["ResourceSupervisor"]
        GRA["GPUResourceActor<br/>Stream pool<br/>Memory allocation"]
        RS --> GRA
    end

    subgraph "Zone 2 — Physics (AllForOne)"
        PS["PhysicsSupervisor"]
        FCA["ForceComputeActor<br/>CUDA kernel dispatch"]
        SMA["StressMajorizationActor<br/>4 kernels, 8ms/step"]
        CA["ConstraintActor<br/>5 kernels, 1ms/step"]
        OCA["OntologyConstraintActor<br/>5 kernels, 2.3ms/step"]
        SFA["SemanticForcesActor<br/>15 kernels, 3ms/step"]
        PS --> FCA
        PS --> SMA
        PS --> CA
        PS --> OCA
        PS --> SFA
    end

    subgraph "Zone 3 — Analytics (OneForOne)"
        AS["AnalyticsSupervisor"]
        CLA["ClusteringActor<br/>12 kernels, 20ms"]
        ADA["AnomalyDetectionActor<br/>4 kernels, 10ms"]
        PRA["PageRankActor<br/>5 kernels, 5ms"]
        AS --> CLA
        AS --> ADA
        AS --> PRA
    end

    subgraph "Zone 4 — Graph Analytics (OneForOne)"
        GAS["GraphAnalyticsSupervisor"]
        SPA["ShortestPathActor<br/>4 kernels, 15ms"]
        CCA["ConnectedComponentsActor<br/>3 kernels, 3ms"]
        GAS --> SPA
        GAS --> CCA
    end

    GM --> RS
    GM --> PS
    GM --> AS
    GM --> GAS
```

### Fault Isolation by Zone

| Zone | Failure Type | Impact | Escalation |
|---|---|---|---|
| Zone 1 (Resources) | OOM, CUDA init | All GPU ops blocked | Full GPU restart |
| Zone 2 (Physics) | CUDA errors, NaN divergence | Physics paused, CPU fallback | Degraded mode |
| Zone 3 (Analytics) | Algorithm errors | Analytics unavailable | Feature disabled |
| Zone 4 (Graph Analytics) | Path query errors | Path queries fail | Feature disabled |

### GPU Self-Init (`ForceComputeActor`)

`ForceComputeActor` initialises CUDA independently on `started()`:

1. Detect available CUDA device (driver API)
2. Allocate node position and velocity CUDA buffers
3. Compile/load PTX kernels (build.rs downgrades `.version` from 9.2 to 9.0 for driver compatibility)
4. Request stream handle from `GPUResourceActor`
5. Signal `PhysicsOrchestratorActor` readiness via `GpuInitComplete`

On CUDA OOM mid-simulation, `ForceComputeActor` emits `ctx.stop()`. The `PhysicsSupervisor` AllForOne strategy restarts all five physics actors, which re-executes the self-init sequence and re-uploads the current constraint set from `OntologyConstraintActor`.

### Kernel Inventory

| Supervisor | Actors | Total Kernels | Frame Budget |
|---|---|---|---|
| ResourceSupervisor | 1 | 0 | N/A |
| PhysicsSupervisor | 5 | 66 | 8ms |
| AnalyticsSupervisor | 3 | 21 | 4ms (on-demand) |
| GraphAnalyticsSupervisor | 2 | 7 | 2ms (on-demand) |
| **Total** | **11** | **94** | **14ms** |

---

## 8. Message Patterns

### Request-Response

Used when the caller requires the result before proceeding (e.g., API handler fetching graph data):

```rust
let graph_data: Arc<GraphData> = graph_actor.send(GetGraphData).await?;
```

### Fire-and-Forget

Used for position broadcasts where confirmation is not needed:

```rust
client_coordinator.do_send(BroadcastPositions { binary });
```

### Pub/Sub

`GraphStateActor` maintains a subscriber list and notifies on topology changes:

```rust
impl GraphStateActor {
    fn notify_graph_update(&self, event: GraphUpdateEvent) {
        for subscriber in &self.subscribers {
            subscriber.do_send(event.clone());
        }
    }
}
```

### Coordination (Join)

`PhysicsOrchestratorActor` runs force computation in parallel, then sequences constraints:

```rust
let forces_future = self.force_compute.send(ComputeForces);
let semantic_future = self.semantic_forces.send(ComputeSemanticForces);
let (forces, semantic) = join!(forces_future, semantic_future);
let combined = self.merge_forces(forces?, semantic?);
let constrained = self.constraint_actor.send(ApplyConstraints(combined)).await?;
```

---

## 9. Performance Characteristics

| Message Pattern | P50 | P95 | P99 |
|---|---|---|---|
| Local actor (same thread) | 50 µs | 100 µs | 200 µs |
| GPU actor (CUDA kernel) | 2 ms | 5 ms | 10 ms |
| WebSocket broadcast | 10 ms | 30 ms | 100 ms |

| Actor | Message | Throughput |
|---|---|---|
| `GraphStateActor` | `GetGraphData` | ~20,000/s |
| `GraphStateActor` | `AddNode` | ~5,000/s |
| `PhysicsOrchestratorActor` | `SimulationStep` | 60/s |
| `ClientCoordinatorActor` | `BroadcastPositions` | ~20/s |

---

## See Also

- [Backend CQRS Pattern](backend-cqrs-pattern.md) — hexagonal architecture, 9 ports, 12 adapters, 114 handlers
- [Physics & GPU Engine](physics-gpu-engine.md) — GPU supervisor and CUDA force computation
- [System Overview](system-overview.md) — complete hexagonal architecture migration overview
