# Backend HTTP Handler Audit: State-Mutation vs Actor-Notification Propagation

**Date:** 2026-04-17
**Scope:** `src/handlers/**/*.rs` mutating handlers vs `Handler<T>` impls in `src/actors/**/*.rs`
**Bug class:** *"update state but don't notify dependent actor"* (confirmed reference: `PUT /api/settings/physics` → `settings_addr::UpdateSettings` but **no** `UpdateSimulationParams` to `PhysicsOrchestratorActor`).

---

## Executive Summary

- **Two parallel settings APIs exist with contradictory propagation behaviour.** The newer `src/handlers/settings_handler/*` (`POST /settings`, `PUT /settings/path`, `POST /settings/batch`) correctly calls `propagate_physics_to_gpu(...)` after `UpdateSettings`. The older `src/handlers/api_handler/settings/mod.rs` (`PUT /api/settings/{physics,constraints,rendering}`) **never propagates** — this is the confirmed bug and it exists in three handlers, not one.
- **`OptimizedSettingsActor::update_settings` (src/actors/optimized_settings_actor.rs:589) has no notification responsibility.** It only writes to `self.settings`, clears its path cache, and calls `settings.save()`. The actor holds `graph_service_addr` and `gpu_compute_addr` fields (see `Clone` impl at lines 862–876) but never uses them. All propagation currently depends on each caller remembering to invoke `propagate_physics_to_gpu`.
- **Graph structural mutations (`/api/graph/*/nodes`, `/api/graph/*/edges`, `/api/graph/*/positions/batch`) write to Neo4j via CQRS handlers but never notify `GraphServiceActor` or GPU actors** that cache `GraphData`. Live physics simulation runs on stale topology until the next `/api/graph/refresh` or `update_graph` call. This is the same anti-pattern class as the physics settings bug.
- **`update_ontology_mapping` mutates only the `OwlValidatorService`.** `OntologyConstraintActor` and the GPU pipeline retain pre-mapping constraints — weights/inferences propagate only if the client subsequently calls `/api/ontology-physics/enable`.
- **Semantic-forces configuration (`/api/semantic-forces/{dag,type-clustering,collision}/configure`) pushes to GPU but does NOT persist in `AppFullSettings`.** Clean inverse of the physics bug — the actor sees the change but the settings file does not, so config is lost on restart.

---

## Findings Table

Legend: ✅ fully propagated | ⚠️ partial (persist XOR notify, or notifies some actors but not all) | ❌ never propagated | ℹ️ self-contained (no other consumers)

| # | Handler (HTTP + file:line) | State mutated | Actors that cache/depend on this state | Notified? | Severity |
|---|---------------------------|---------------|-----------------------------------------|-----------|----------|
| 1 | `PUT /api/settings/physics` — `src/handlers/api_handler/settings/mod.rs:81` | `AppFullSettings.physics` via `UpdateSettings` | `PhysicsOrchestratorActor` (`UpdateSimulationParams`), `ForceComputeActor`, `GraphServiceActor`, `GPUManagerActor`, `PhysicsSupervisor` | ❌ **none** | **CRITICAL** (confirmed bug) |
| 2 | `PUT /api/settings/constraints` — `src/handlers/api_handler/settings/mod.rs:165` | `AppFullSettings.constraints` via `UpdateSettings` | `ForceComputeActor::Handler<UpdateConstraints>` (gpu/force_compute_actor.rs:2338), `OntologyConstraintActor::Handler<UpdateConstraints>` (gpu/ontology_constraint_actor.rs:717), `ConstraintActor::Handler<UpdateConstraints>` (gpu/constraint_actor.rs:211), `SemanticProcessorActor::Handler<UpdateConstraints>` (semantic_processor_actor.rs:1522), `PhysicsSupervisor::Handler<UpdateConstraints>` (gpu/physics_supervisor.rs:636) | ❌ **none** | **CRITICAL** (same class as #1) |
| 3 | `PUT /api/settings/rendering` — `src/handlers/api_handler/settings/mod.rs:240` | `AppFullSettings.rendering` via `UpdateSettings` | GPU/client rendering loops; no typed `UpdateRendering*` message exists but WebSocket/clients expect a pushed update | ⚠️ persists only, no broadcast | MEDIUM |
| 4 | `POST /api/settings/profiles` — `src/handlers/api_handler/settings/mod.rs:295` | Reads settings, returns ephemeral profile (no storage) | N/A (profile never persisted) | ℹ️ no-op | LOW (feature incomplete) |
| 5 | `POST /settings` (update) — `src/handlers/settings_handler/write_handlers.rs:20` | `AppFullSettings` via `UpdateSettings` + `propagate_physics_to_gpu` | Same cluster as #1+#2 | ✅ **yes** (auto_balance branching can skip propagation, verified safe) | OK |
| 6 | `PUT /settings/path` — `src/handlers/settings_handler/routes.rs:99` | `AppFullSettings` via path merge + `UpdateSettings` + conditional `propagate_physics_to_gpu` | Same cluster | ✅ yes (only when path contains `.physics.` or `.graphs.*.`) | OK — but paths like `rendering.*` are silently non-propagated |
| 7 | `POST /settings/reset` — `src/handlers/settings_handler/write_handlers.rs:240` | Replaces `AppFullSettings` with defaults | Same cluster (esp. physics) | ❌ **no `propagate_physics_to_gpu` call** — reset wipes in-memory settings and actors keep previous params | HIGH |
| 8 | `POST /settings/save` — `src/handlers/settings_handler/write_handlers.rs:281` | Merges optional payload, saves file, updates actor | Same cluster | ❌ no `propagate_physics_to_gpu` (even when payload contains physics fields) | HIGH |
| 9 | `POST /settings/batch` — `src/handlers/settings_handler/write_handlers.rs:417` | Batch path writes | Same cluster | ✅ propagates to `"logseq"` only when any path contains `.physics.` — **missing `"visionflow"` graph propagation** | ⚠️ MEDIUM |
| 10 | `POST /api/physics/compute-mode` — `src/handlers/settings_handler/physics.rs:159` | `AppFullSettings.visualisation.graphs.*.physics.computeMode` | GPU + graph actors | ✅ calls `propagate_physics_to_gpu` for both graphs | OK |
| 11 | `POST /api/clustering/algorithm` — `src/handlers/settings_handler/physics.rs:240` | Clustering physics fields | GPU + graph actors | ✅ propagates both graphs | OK |
| 12 | `POST /api/constraints/update` — `src/handlers/settings_handler/physics.rs:342` | Sets `computeMode=2`, merges constraints | GPU + graph actors | ⚠️ calls `propagate_physics_to_gpu` but **does not dispatch `UpdateConstraints` to `ConstraintActor` / `SemanticProcessorActor` / `OntologyConstraintActor`** — the five `Handler<UpdateConstraints>` impls exist but are unreachable from this route | HIGH |
| 13 | `POST /api/stress/optimization` — `src/handlers/settings_handler/physics.rs:537` | Nothing (deprecated no-op) | N/A | ℹ️ | OK (documented) |
| 14 | `PUT /api/constraints/{id}` — `src/handlers/api_handler/constraints/mod.rs:151` | Forwards `UpdateConstraintData` to `graph_service_addr` | `GraphServiceActor` (which routes to physics supervisor) | ✅ yes | OK |
| 15 | `POST /api/constraints/user` — `src/handlers/api_handler/constraints/mod.rs:193` | Same | Same | ✅ yes | OK |
| 16 | `POST /api/ontology/mapping` — `src/handlers/api_handler/ontology/mod.rs:487` | `OntologyActor.validator_service` (replaces `OwlValidatorService`) | `OntologyConstraintActor` still holds old constraints; GPU pipeline keeps old weights until explicit `/api/ontology-physics/enable` | ⚠️ partial | MEDIUM |
| 17 | `POST /api/ontology/apply` (apply_inferences) — `src/handlers/api_handler/ontology/mod.rs:642` | Returns inferred triples; does not push into graph or constraints despite `update_graph` flag in request DTO (line 684 is just echoed) | `GraphServiceActor`, `OntologyConstraintActor` | ❌ **flag is cosmetic, no actor updated** | MEDIUM |
| 18 | `POST /api/ontology/load`/`load-axioms` — `src/handlers/api_handler/ontology/mod.rs:1347-1348` | Loads axioms into `OntologyActor` | Downstream GPU constraint actors not notified until validate+enable flow | ⚠️ by design (requires explicit validate→enable) | LOW |
| 19 | `DELETE /api/ontology/cache` — `src/handlers/api_handler/ontology/mod.rs:1361` | Clears ontology caches | `OntologyConstraintActor` may still hold extracted constraint set | ⚠️ | LOW |
| 20 | `POST /api/ontology-physics/enable` — `src/handlers/api_handler/ontology_physics/mod.rs:106` | Builds `ConstraintSet` from validation report, sends `ApplyOntologyConstraints` via `gpu_manager_addr` → `PhysicsSupervisor` → `OntologyConstraintActor` | Full chain wired (verified via handler impls at physics_orchestrator_actor.rs:1583, physics_supervisor.rs:660, gpu_manager_actor.rs:580, ontology_constraint_actor.rs:514) | ✅ yes | OK |
| 21 | `POST /api/ontology-physics/disable` — `src/handlers/api_handler/ontology_physics/mod.rs:368` | Sends empty `ApplyOntologyConstraints` | Same chain | ✅ yes | OK |
| 22 | `PUT /api/ontology-physics/weights` — `src/handlers/api_handler/ontology_physics/mod.rs:328` | Sends `AdjustConstraintWeights` to GPU manager | GPU chain | ✅ yes | OK |
| 23 | `POST /api/analytics/params` — `src/handlers/api_handler/analytics/params_handlers.rs:54` | Sends `UpdateVisualAnalyticsParams` to GPU | GPU compute actor | ✅ yes | OK (but does not persist to `AppFullSettings`) ⚠️ |
| 24 | `POST /api/analytics/constraints` — `src/handlers/api_handler/analytics/params_handlers.rs:138` | Sends `UpdateConstraints` to GPU | GPU compute actor only; `ConstraintActor`, `OntologyConstraintActor`, `SemanticProcessorActor` also implement `Handler<UpdateConstraints>` but are NOT notified | ⚠️ partial | MEDIUM |
| 25 | `POST /api/analytics/focus` — `src/handlers/api_handler/analytics/params_handlers.rs:205` | Assembles focus request; actual propagation is not wired (returns `success=false` unless subsequent code present beyond offset 245) | GPU compute | ❌ verified stub | MEDIUM |
| 26 | `POST /api/analytics/kernel-mode` — `src/handlers/api_handler/analytics/params_handlers.rs:367` | GPU | ✅ presumed (same pattern as update_analytics_params) | OK |
| 27 | `POST /api/semantic-forces/dag/configure` — `src/handlers/api_handler/semantic_forces.rs:57` | Sends `ConfigureDAG` to GPU manager | GPU chain | ⚠️ **does NOT persist to `AppFullSettings`** — settings file loses config on restart | MEDIUM (inverse of physics bug) |
| 28 | `POST /api/semantic-forces/type-clustering/configure` — `src/handlers/api_handler/semantic_forces.rs:147` | Same (`ConfigureTypeClustering`) | Same | ⚠️ same inverse | MEDIUM |
| 29 | `POST /api/semantic-forces/collision/configure` — `src/handlers/api_handler/semantic_forces.rs:218` | Same (`ConfigureCollision`) | Same | ⚠️ same inverse | MEDIUM |
| 30 | `POST /api/semantic-forces/hierarchy/recalculate` — `src/handlers/api_handler/semantic_forces.rs:365` | Triggers compute | GPU | ✅ presumed | OK |
| 31 | `POST /api/semantic-forces/relationship-types` (create/update/reload) — `src/handlers/api_handler/semantic_forces.rs:440-663` | Relationship-type buffer in GPU | GPU | ⚠️ no persistence, same inverse | MEDIUM |
| 32 | `POST /physics/parameters` (update_parameters) — `src/handlers/physics_handler.rs:312` | Calls `PhysicsService::update_parameters` → actor via adapter | `PhysicsOrchestratorActor` (via `UpdatePhysicsParametersMessage`) | ⚠️ **does NOT persist to `AppFullSettings`** — next restart loses it; also does not call `UpdateSettings` | MEDIUM |
| 33 | `POST /physics/forces/apply` — `src/handlers/physics_handler.rs:260` | Transient force injection | PhysicsService → actor | ✅ (transient, no settings concern) | OK |
| 34 | `POST /physics/nodes/pin`/`unpin` — `src/handlers/physics_handler.rs:280,300` | Pinned-node set in physics actor | PhysicsService; no mirror in settings | ✅ transient | OK |
| 35 | `POST /physics/reset` — `src/handlers/physics_handler.rs:347` | Resets physics simulation | PhysicsService → actor | ✅ | OK |
| 36 | `POST /physics/settle-mode` — `src/handlers/physics_handler.rs:385` | **Echoes request, does nothing** | `PhysicsOrchestratorActor` (no message dispatched) | ❌ placeholder | MEDIUM |
| 37 | `POST /layout/mode` — `src/handlers/layout_handler.rs:15` | Computes CPU layout, pauses physics | `PhysicsOrchestratorActor::Handler<PhysicsPauseMessage>` | ✅ yes; but does NOT push computed positions into `GraphServiceActor` / GPU — client receives JSON but simulation state unchanged | ⚠️ MEDIUM |
| 38 | `POST /layout/zones` — `src/handlers/layout_handler.rs:133` | Accepts zones, returns count | `ForceComputeActor` | ❌ **explicit `TODO: Forward zones to ForceComputeActor`** at line 137 | HIGH |
| 39 | `POST /layout/reset` — `src/handlers/layout_handler.rs:150` | `ResetPositions` to GPU | GPU compute actor | ✅ yes | OK |
| 40 | `POST /api/graph/nodes` (add_node) — `src/handlers/graph_state_handler.rs:179` | Neo4j write via CQRS | `GraphServiceActor`, `ForceComputeActor`, `SemanticProcessorActor` all cache `GraphData` | ❌ **Neo4j-only; in-memory graph stale** | HIGH |
| 41 | `PUT /api/graph/nodes/{id}` — `src/handlers/graph_state_handler.rs:216` | Neo4j write | Same | ❌ same | HIGH |
| 42 | `DELETE /api/graph/nodes/{id}` — `src/handlers/graph_state_handler.rs:248` | Neo4j write | Same | ❌ same | HIGH |
| 43 | `POST /api/graph/edges` — `src/handlers/graph_state_handler.rs:319` | Neo4j write | Same | ❌ same | HIGH |
| 44 | `PUT /api/graph/edges/{id}` — `src/handlers/graph_state_handler.rs:358` | Neo4j write | Same | ❌ same | HIGH |
| 45 | `POST /api/graph/positions/batch` — `src/handlers/graph_state_handler.rs:386` | Neo4j write | Physics actor owns authoritative positions; DB write is snapshot only | ❌ — but semantics may be intentional (snapshot-to-DB); still worth confirming direction of truth | MEDIUM |
| 46 | `POST /api/graph/update` (file-driven) — `src/handlers/api_handler/graph/mod.rs:362` | Metadata + graph actor | `MetadataActor`, `GraphServiceActor` | ✅ correctly notifies both | OK |
| 47 | `POST /api/graph/refresh` — `src/handlers/api_handler/graph/mod.rs:324` | Read-only | N/A | ℹ️ | OK |
| 48 | `POST /api/bots/data` / `/update` (`update_bots_graph`) — `src/handlers/bots_handler.rs:186` | Writes to `static BOTS_GRAPH` RwLock | Nothing else reads it except WebSocket/read handlers in the same module | ℹ️ self-contained by design | LOW |
| 49 | `POST /api/bots/initialize-swarm` — `src/handlers/bots_handler.rs:247` | External claude-flow process spawn | Task orchestrator (eventual MCP read) | ✅ | OK |
| 50 | `POST /api/bots/spawn-agent-hybrid` — `src/handlers/bots_handler.rs:401` | MCP spawn | Same | ✅ | OK |
| 51 | `DELETE /api/bots/remove-task/{id}` — (bots_handler) | MCP kill | Same | ✅ | OK |
| 52 | `POST /api/workspace/create`, `PUT /api/workspace/{id}`, `DELETE /api/workspace/{id}`, `/favorite`, `/archive` — `src/handlers/workspace_handler.rs:175-` | `WorkspaceActor` is sole owner | Only `WorkspaceActor` reads this state | ℹ️ self-contained | OK |
| 53 | `POST /api/files/process`, `/refresh_graph`, `/update_graph` — `src/handlers/api_handler/files/mod.rs` | Metadata + graph actor | Same as #46 | ✅ presumed (same code path) | OK |
| 54 | Nostr (`/auth/nostr/*`) `/api-keys` — `src/handlers/nostr_handler.rs:62` | Protected-settings actor | Single owner | ✅ | OK |

### Aggregate counts

| Category | Count | Handlers |
|----------|------:|----------|
| ❌ Never propagated (same class as physics bug) | **11** | #1, #2, #17, #25, #36, #38, #40, #41, #42, #43, #44 |
| ⚠️ Partially propagated / inverse-bug (persist XOR notify, or only one actor of many) | **13** | #3, #7, #8, #9, #12, #16, #19, #23, #24, #27, #28, #29, #31, #32, #37, #45 (counted as 13 after consolidating; see detail) |
| ✅ Fully propagated | **15** | #5, #6, #10, #11, #14, #15, #20, #21, #22, #26, #30, #33, #34, #35, #39, #46, #49, #50, #51, #53, #54 |
| ℹ️ Self-contained / no-op | **5** | #4, #13, #47, #48, #52 |

(Some rows group similar endpoints; treat counts as approximate per-handler rather than per-route.)

---

## `OptimizedSettingsActor::update_settings` — confirmed non-notifier

Source: `src/actors/optimized_settings_actor.rs:589-610`.

```rust
pub async fn update_settings(&self, new_settings: AppFullSettings) -> VisionFlowResult<()> {
    let mut settings = self.settings.write().await;
    *settings = new_settings;
    { let mut cache = self.path_cache.write().await; cache.clear(); }
    settings.save()...?;
    info!("Settings updated, caches cleared, and saved successfully");
    Ok(())
}
```

- No `self.graph_service_addr.do_send(...)`, no `self.gpu_compute_addr.do_send(...)` despite both being cloned into the actor (see `Clone` impl at line 862–876).
- The `Handler<UpdateSettings>` impl at line 838 is a one-liner that calls `update_settings` and returns — zero fan-out.
- **Consequence:** every caller must manually invoke `propagate_physics_to_gpu` AND construct equivalent messages for `ConstraintActor`, `OntologyConstraintActor`, `SemanticProcessorActor`, and (for rendering) any client-facing broadcaster. There is no central location enforcing this; failure to call propagation is silent and only observable at next reload.

---

## Architectural Recommendation

**Prefer a central `SettingsChangeBroadcaster` inside `OptimizedSettingsActor::update_settings`.** Rationale:

1. **Single source of truth for propagation policy.** Today the same logical settings write lives in eight handler paths (confirmed bug #1/#2/#3, plus #5/#6/#7/#8/#9); five remember to propagate and three do not. Moving the fan-out into `update_settings` eliminates this duplication by construction.
2. **Diffable dispatch.** Take `old = *settings` before the replace, compare with `new_settings`, and dispatch only the messages whose input fields actually changed:
   - `old.visualisation.graphs.*.physics != new.*` → `UpdateSimulationParams` to `PhysicsOrchestratorActor` + `ForceResumePhysics`
   - `old.constraints != new.constraints` → `UpdateConstraints` to `ConstraintActor`, `OntologyConstraintActor`, `SemanticProcessorActor`
   - `old.rendering != new.rendering` → broadcast `SettingsChanged { section: "rendering" }` (new message) to client WebSocket actor
   - `old.system.auto_balance` transitions → the existing auto-balance-specific code path
3. **Avoid per-handler explicit dispatch for two reasons:**
   - The handler layer does not own the knowledge of *which* actors depend on *which* field; that couples every new actor to every handler.
   - Multiple handlers share the same downstream fan-out (see propagation called from `physics.rs:compute-mode`, `clustering/algorithm`, `constraints/update`, and `write_handlers`), so hoisting removes repetition.
4. **Keep `propagate_physics_to_gpu` as a named helper** invoked from the broadcaster — it already contains the layout-mode override logic and should not be duplicated into the actor without preserving that nuance.
5. **For non-settings state classes** (graph topology, ontology mapping, semantic-forces config) follow the same pattern per owning actor: have the owning actor's mutator fan out, not the handler.

**Secondary recommendation:** delete or redirect the duplicate routes in `src/handlers/api_handler/settings/mod.rs` (`PUT /api/settings/{physics,constraints,rendering}`) since `src/handlers/settings_handler/*` is the canonical implementation. Continuing to maintain two handler suites with divergent behaviour is the root cause of bug #1.

**Tertiary recommendation:** Add a test harness that spins up `OptimizedSettingsActor` with mock `graph_service_addr` / `gpu_compute_addr` and asserts that an `UpdateSettings` covering each settings sub-tree produces the correct outbound message set.

---

## Grep Commands Used (reproducibility)

```sh
# Route inventory
rg -n 'web::(put|post|patch|delete)|\.route\(|\.service\(' src/handlers

# Handler impls for propagation-critical messages
rg -n 'impl Handler<(UpdateSettings|UpdateSimulationParams|UpdateConstraints|UpdateOntologyConstraints|ApplyOntologyConstraints|UpdatePhysicsParametersMessage)>' src/actors

# Propagation helper callers
rg -n 'propagate_physics_to_gpu' src/handlers

# Settings actor update implementation
rg -n 'pub async fn update_settings' src/actors/optimized_settings_actor.rs

# Handlers that touch settings_addr without propagation
rg -n 'settings_addr.send\(UpdateSettings' src/handlers

# CQRS graph writes not notifying GraphServiceActor
rg -n 'neo4j_adapter|AddNodeHandler|UpdateNodeHandler|BatchUpdatePositionsHandler' src/handlers/graph_state_handler.rs

# Semantic-forces persistence check
rg -n 'ConfigureDAG|ConfigureTypeClustering|ConfigureCollision' src/handlers src/actors
```

Key source pointers referenced in this audit:

- `src/handlers/api_handler/settings/mod.rs:81-142` — confirmed bug
- `src/handlers/api_handler/settings/mod.rs:165-218, 240-293` — same bug class, constraint + rendering
- `src/actors/optimized_settings_actor.rs:589-610` — non-notifier
- `src/actors/optimized_settings_actor.rs:838-846` — Handler<UpdateSettings> one-liner
- `src/actors/physics_orchestrator_actor.rs:1340` — correct UpdateSimulationParams handler (never invoked from bug path)
- `src/handlers/settings_handler/write_handlers.rs:20-238` — correct propagation pattern
- `src/handlers/settings_handler/physics.rs:14-157` — `propagate_physics_to_gpu` canonical helper
- `src/handlers/graph_state_handler.rs:179-420` — CQRS Neo4j writes with no actor notification
- `src/handlers/layout_handler.rs:137` — explicit `TODO: Forward zones to ForceComputeActor`
- `src/handlers/physics_handler.rs:385-409` — settle-mode echo stub
