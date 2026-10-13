# Actor bypass inventory (TODO-unified X-2, inventory half)

Status: inventory and recommendations only. **The decisions below are recommendations, and none of the deletions or re-routings has been done.** No source file was edited to produce this document. The owner decides; the dead half is deleted in a later, separately reviewed change (the X-2 row calls for GPU-feature builds and the handler tests before any deletion).

Read at: VisionClaw `main` HEAD `93f38c6`. Every `file:line` below was resolved from `git show HEAD:<path>`, not from the working tree (which carries other people's uncommitted edits).

## How the inventory was built

The exact search, run against HEAD:

```
git grep -n "execute_in_thread" HEAD -- src
```

That returns 46 lines, which matches the X-2 row. They split as follows.

| Kind | Lines | Count |
|------|-------|-------|
| Definition | `src/handlers/utils.rs:1` | 1 |
| Imports | `api_handler/graph/mod.rs:20`, `api_handler/mod.rs:21`, `graph_state_handler.rs:4`, `ontology_handler.rs:4`, `ontology_handler.rs:1115` | 5 |
| Doc comment | `ontology_handler.rs:1111` | 1 |
| Test calls (inside the `#[cfg(test)]` module) | `ontology_handler.rs:1139`, `:1147`, `:1154` | 3 |
| **Production call sites** | listed in table A | **36** |

The X-2 row says "35 production call sites" but its own per-file breakdown (ontology 20, graph_state 9, api_handler/graph 6, api_handler/mod 1) sums to 36, and HEAD agrees with 36.

Second sweep, for handlers that touch a store without going through `execute_in_thread`:

```
git grep -n -E "state\.(graph_adapter|ontology_repository|sqlite_settings_repository|settings_repository)|app_state\.(graph_adapter|ontology_repository|sqlite_settings_repository|settings_repository)|\.sparql_select_json|\.read_inferred_graph|oxigraph::" HEAD -- src/handlers
git grep -n -i -E "sqlite|\.store\(\)|rusqlite" HEAD -- src/handlers src/settings
```

Those results are table B. Table B is a sweep of `src/handlers` plus `src/settings/api`, not a proof that no other direct reader exists elsewhere in `src/`.

### What `execute_in_thread` is

`src/handlers/utils.rs:1` is a thin `tokio::task::spawn_blocking` wrapper. Every call site builds a hexser CQRS handler (`QueryHandler` / `DirectiveHandler`) over a repository and runs its synchronous `handle()` on a blocking thread. The handlers bridge back to async with `Handle::current().block_on(...)` (for example `src/application/knowledge_graph/directives.rs:47,96,142,193,243`). So the shim is: blocking thread, `block_on`, async repository.

### Which store each handler is bound to

- `state.graph_adapter` is an `Arc<OxigraphGraphRepository>` (`src/app_state.rs:315`). It is the persistent store itself.
- `state.ontology_repository` is an `Arc<OxigraphOntologyRepository>` (`src/app_state.rs:318`). Also the store itself.
- `state.graph_query_handlers.*` are built over `ActorGraphRepository` (`src/app_state.rs:852-865`), which sends messages to `GraphStateActor` (`src/adapters/actor_graph_repository.rs:35-170`). These are **already routed through the actor**.
- `state.settings_repository` is the SQLite settings repository (`src/app_state.rs:285,441`).

`GraphStateActor` (`src/actors/graph_state_actor.rs`) holds the live graph in memory and persists write-through to the same `OxigraphGraphRepository`, fire-and-forget (`graph_state_actor.rs:359-364`, `389-394`, `424-430`, `450-456`). It loads from the store only at start-up and on `ReloadGraphFromDatabase` (`graph_state_actor.rs:1217`). Consequence: a write that goes to the store directly (table A, `graph_state_handler.rs`) never reaches the actor's in-memory graph, the physics loop or the wire until a reload.

`OntologyActor` (`src/actors/ontology_actor.rs`) is a validation and reasoning job runner (handlers at `:696-:932`: `LoadOntologyAxioms`, `ValidateOntology`, `ApplyInferences`, `TriggerReasoning`, caches). It does not own the ontology store and has no class, property or axiom CRUD messages. The `GraphServiceSupervisor` tree is not involved in any ontology handler.

## Summary by decision (production `execute_in_thread` sites, table A)

| Decision | Sites | Where |
|----------|-------|-------|
| route-through-actor | **13** | `graph_state_handler.rs` 9; `api_handler/graph/mod.rs` 4 |
| direct-store-is-correct (remove decorative supervision) | **18** | `ontology_handler.rs` 17; `api_handler/mod.rs` 1 |
| needs-owner-call | **5** | `ontology_handler.rs` 3; `api_handler/graph/mod.rs` 2 |
| **Total** | **36** | |

Read these three notes before the table:

1. **Reachability caveat on `graph_state_handler.rs` (UNVERIFIED at runtime).** `api_handler::config` registers `graph::config` (`api_handler/mod.rs:112`) before `graph_state_handler::config` (`api_handler/mod.rs:113`). Both declare `web::scope("/graph")`. The comment at `api_handler/graph/mod.rs:1487-1489` states that actix claims the prefix for the first scope and later same-prefix scopes return 404. If that holds, all nine `graph_state_handler` routes (`/api/graph/state`, `/statistics`, `/nodes`, `/edges`, `/positions/batch`) are unreachable. I found no client caller (`git grep` over `client/src`) and the only test, `tests/graph_state_integration.rs`, is commented out. If the owner confirms a 404 with `curl -i http://localhost:4000/api/graph/statistics`, the better decision for those nine rows is to delete the module, not re-route it. I record `route-through-actor` because that is the right shape if the endpoints are kept.
2. **`ActorGraphRepository` returns stubs for physics.** `get_physics_state`, `get_constraints`, `get_auto_balance_notifications` and `get_equilibrium_status` return defaults (`actor_graph_repository.rs:101,127,134,141`, each with the comment "GraphStateActor doesn't handle physics operations"). Two table-A rows (`graph/mod.rs:207` and `:579`) therefore go through the actor and still return placeholder data.
3. **"Direct-store-is-correct" here means the supervision layer is absent, not the shim.** For ontology handlers the only decorative layer is the CQRS handler plus `spawn_blocking` plus `block_on`. The follow-up is to await the async repository method directly. That is a refactor of the call shape, not a data-path change.

## Table A: production `execute_in_thread` call sites (36)

Columns: site, function, operation, store, whether an actor message exists for the same operation, recommended decision with reason.

### A1. `src/handlers/graph_state_handler.rs` (9 sites, store: Oxigraph via `state.graph_adapter`, bypassing `GraphStateActor`)

| Site | Function | Reads or writes | Actor message for the same operation | Recommended decision | Reason |
|------|----------|-----------------|--------------------------------------|----------------------|--------|
| `graph_state_handler.rs:71` | `get_graph_state` | reads whole graph (`LoadGraph`) | Yes: `GetGraphData`, handled at `graph_state_actor.rs:942` | route-through-actor | The actor holds the live positions; the store holds the last persisted ones. |
| `graph_state_handler.rs:131` | `get_graph_statistics` | reads counts (`GetGraphStatistics`) | No dedicated message; derivable from `GetGraphData` | route-through-actor | Compute from the actor's graph so counts match what clients see. |
| `graph_state_handler.rs:172` | `add_node` | writes a node | Yes: `AddNode`, `graph_state_actor.rs:951` | route-through-actor | The actor already persists write-through; a direct write leaves its memory stale. |
| `graph_state_handler.rs:204` | `update_node` | writes a node | No (`UpdateNodeFromMetadata`, `:1016`, is a different operation) | route-through-actor | Needs a new `UpdateNode` message; otherwise the store and actor diverge. |
| `graph_state_handler.rs:235` | `remove_node` | deletes a node | Yes: `RemoveNode`, `graph_state_actor.rs:960` | route-through-actor | As `add_node`. |
| `graph_state_handler.rs:263` | `get_node` | reads one node | Partly: `GetNodeMap`, `graph_state_actor.rs:987` | route-through-actor | Serve from the actor's node map. |
| `graph_state_handler.rs:314` | `add_edge` | writes an edge | Yes: `AddEdge`, `graph_state_actor.rs:969` | route-through-actor | As `add_node`. |
| `graph_state_handler.rs:346` | `update_edge` | writes an edge | No | route-through-actor | Needs a new `UpdateEdge` message. |
| `graph_state_handler.rs:379` | `batch_update_positions` | writes positions | Yes, nearest: `UpdateNodePositions`, `graph_state_actor.rs:862` (carries `correlation_id`, `graph_messages.rs:58-62`) | route-through-actor | Positions are owned by the physics loop; a store-only write is overwritten or ignored. |

See reachability caveat 1 above: these nine may be dead code.

### A2. `src/handlers/api_handler/graph/mod.rs` (6 sites, via `state.graph_query_handlers`, already actor-backed)

| Site | Function | Reads or writes | Actor message for the same operation | Recommended decision | Reason |
|------|----------|-----------------|--------------------------------------|----------------------|--------|
| `graph/mod.rs:203` | `get_graph_data` (`:188`) | reads graph (`GetGraphData`) | Yes: `GetGraphData` (`graph_state_actor.rs:942`), already used via `ActorGraphRepository` | route-through-actor | Already routed; only the `spawn_blocking`/`block_on` shim remains. |
| `graph/mod.rs:205` | `get_graph_data` | reads node map (`GetNodeMap`) | Yes: `GetNodeMap` (`graph_state_actor.rs:987`) | route-through-actor | Already routed. The result is bound to `_node_map` and discarded (`graph/mod.rs:221`), so this is wasted work; remove the call. |
| `graph/mod.rs:207` | `get_graph_data` | reads physics state (`GetPhysicsState`) | Not on `GraphStateActor`; `ActorGraphRepository::get_physics_state` returns `PhysicsState::default()` (`actor_graph_repository.rs:101-106`). `PhysicsOrchestratorActor` has `GetPhysicsStatus` (`physics_orchestrator_actor.rs:1482`) but it is not wired here | needs-owner-call | Either wire the physics actor in or delete the query; today it returns a placeholder. The handler separately fetches live settlement via `fetch_settlement` (`graph/mod.rs:152`). |
| `graph/mod.rs:348` | `get_paginated_graph_data` (`:329`) | reads graph | Yes: `GetGraphData` | route-through-actor | Already routed. |
| `graph/mod.rs:437` | `refresh_graph` (`:432`) | reads graph | Yes: `GetGraphData` | route-through-actor | Already routed. |
| `graph/mod.rs:579` | `get_auto_balance_notifications` (`:565`) | reads notifications | A `GetAutoBalanceNotifications` message type exists (`actors/messages/graph_messages.rs:30`) but I found no `Handler<GetAutoBalanceNotifications>` impl in `src/`; the repository returns an empty list (`actor_graph_repository.rs:134-139`) | needs-owner-call | The endpoint always returns empty. Implement the handler or delete the endpoint. |

### A3. `src/handlers/api_handler/mod.rs` (1 site)

| Site | Function | Reads or writes | Actor message for the same operation | Recommended decision | Reason |
|------|----------|-----------------|--------------------------------------|----------------------|--------|
| `api_handler/mod.rs:44` | `get_app_config` | reads settings (`LoadAllSettings`) via `state.settings_repository` (SQLite) | Yes: `GetSettings`, handled at `optimized_settings_actor.rs:762` | direct-store-is-correct | Read-only feature-presence flags; the persisted record is authoritative. Caveat: I verified that physics writes persist to SQLite (`settings/api/settings_routes.rs:684`), not that every settings write does. |

### A4. `src/handlers/ontology_handler.rs` (20 sites, store: Oxigraph via `state.ontology_repository`, no actor owns this state)

All twenty route through `execute_in_thread(... handler.handle(...))` over `OxigraphOntologyRepository`. Routes are mounted by `api_handler/ontology/mod.rs:1632-1697` (the old `ontology_handler::config` was collapsed into it, per `api_handler/mod.rs:114-118`).

| Site | Function | Reads or writes | Actor message for the same operation | Recommended decision | Reason |
|------|----------|-----------------|--------------------------------------|----------------------|--------|
| `ontology_handler.rs:85` | `get_ontology_graph` | reads ontology graph | None (`OntologyActor` has no graph read) | direct-store-is-correct | No actor owns the store; supervision tree not involved. |
| `ontology_handler.rs:113` | `save_ontology_graph` | writes ontology graph | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:148` | `get_owl_class` | reads a class | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:182` | `list_owl_classes` | reads classes | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:228` | `get_class_hierarchy` | reads classes to build a tree | None | direct-store-is-correct | The function (`:203`) is not mounted anywhere (`git grep get_class_hierarchy HEAD -- src` finds only its definition and a comment). It is dead: delete the handler. |
| `ontology_handler.rs:340` | `add_owl_class` | writes a class | None | direct-store-is-correct | No actor owns the store. |
| `ontology_handler.rs:372` | `update_owl_class` | writes a class | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:402` | `remove_owl_class` | deletes a class | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:441` | `get_owl_property` | reads a property | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:476` | `list_owl_properties` | reads properties | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:512` | `add_owl_property` | writes a property | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:554` | `update_owl_property` | writes a property | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:588` | `get_class_axioms` | reads axioms of a class | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:633` | `add_axiom` | writes an axiom | None (`LoadOntologyAxioms`, `ontology_actor.rs:696`, is bulk ingest into the reasoner, not a CRUD write) | direct-store-is-correct | Different operation from the actor's. |
| `ontology_handler.rs:671` | `remove_axiom` | deletes an axiom | None | direct-store-is-correct | As above. |
| `ontology_handler.rs:710` | `get_inference_results` | reads stored inference results | Related: `ApplyInferences` (`ontology_actor.rs:796`), `TriggerReasoning` (`:909`) | needs-owner-call | The actor runs and applies inference; this reads the persisted table. Whether they should share one path is a design call I could not settle from the code. |
| `ontology_handler.rs:745` | `store_inference_results` | writes inference results | Related: as `:710` | needs-owner-call | As `:710`. |
| `ontology_handler.rs:786` | `validate_ontology` (GET) | repository-level validation (`ValidateOntology` query) | Same name, different operation: `ValidateOntology` in `actors/messages/ontology_messages.rs:37` is a job-based validation (`ontology_actor.rs:738`). POST `/ontology/validate` already uses the actor | needs-owner-call | Two validators share a name; the owner should say which one GET `/ontology/validate` is meant to be. |
| `ontology_handler.rs:980` | `query_ontology` | reads, SPARQL (`QueryOntology`) | None | direct-store-is-correct | `sparql_query` (`:1040`) reaches the same store directly with no shim; one of the two routes is redundant. |
| `ontology_handler.rs:1076` | `get_ontology_metrics` | reads metrics | None | direct-store-is-correct | `ontology_class_count_handler.rs:41` reads the same metrics directly. |

## Table B: handlers that read or write a store directly, without `execute_in_thread`

These are not counted in the 36. "Actor message" means an actor message for the same operation.

| Site | Function | Reads or writes | Store | Actor message for the same operation | Recommended decision | Reason |
|------|----------|-----------------|-------|--------------------------------------|----------------------|--------|
| `ontology_handler.rs:1040` | `sparql_query` | reads (SPARQL select) | Oxigraph (`state.ontology_repository`) | None | direct-store-is-correct | Read-only; already the target shape. |
| `ontology_handler.rs:1059` | `get_inferred_graph` | reads inferred graph | Oxigraph | None | direct-store-is-correct | As above. |
| `api_handler/ontology/mod.rs:353` | `extract_property_graph` | reads ontology graph | Oxigraph | None | direct-store-is-correct | As above. |
| `api_handler/ontology/mod.rs:1065` | `get_hierarchy` | reads classes | Oxigraph | None | direct-store-is-correct | As above. |
| `api_handler/ontology/mod.rs:1540` | `state_at` | reads (SPARQL, bi-temporal) | Oxigraph | None | direct-store-is-correct | As above. |
| `api_handler/ontology_physics/mod.rs:444` | `get_trust_status` | reads (raw `Store` handle) | Oxigraph (`ontology_repository.store()`) | None | direct-store-is-correct | Liveness canary reading the store is the point. |
| `decision_handler.rs:249` | `trace_decision` | reads (bounded SPARQL frontier, raw `Store`) | Oxigraph | None | direct-store-is-correct | Read-only provenance query. |
| `ontology_class_count_handler.rs:41` | `class_count` | reads metrics | Oxigraph | None | direct-store-is-correct | Read-only. |
| `ontology_derived_handler.rs:98` | `write_derived` | writes derived quads (`append_derived_quads`) | Oxigraph | None | direct-store-is-correct | Single-writer fenced append in the repository (`DERIVED_FENCE`), not an actor concern. |
| `ontology_derived_handler.rs:174` | `read_derived` | reads derived graph | Oxigraph | None | direct-store-is-correct | Read-only. |
| `enrichment_proposals_handler.rs:469` | `decide_as_operator` | reads and writes a case | SQLite (`sqlite_enrichment_repository`) | None (it does message `ClientCoordinatorActor` for the broker round-trip) | direct-store-is-correct | SQLite is the system of record for decisions; the actor is a notification channel. |
| `insight_loop_handler.rs:55` | `traces` | reads loop traces | SQLite | None | direct-store-is-correct | Read-only. |
| `insight_loop_handler.rs:71` | `trace_by_case` | reads one case trace | SQLite | None | direct-store-is-correct | Read-only. |
| `trace_handler.rs:40` | `unified_trace` | reads a join of enrichment and KPI tables | SQLite (`sqlite_enrichment_repository`, `sqlite_kpi_repository`) | None | direct-store-is-correct | Read-time join, no live state involved. |
| `socket_flow_handler/filter_auth.rs:360` | `handle_filter_update` continuation | writes the user's filter | SQLite (`sqlite_settings_repository`) | Yes, already used first: `ClientCoordinatorActor` applies the filter in memory, then SQLite persists it (comment at `filter_auth.rs:352-358`) | direct-store-is-correct | Actor-first then persist is the intended order. |
| `settings/api/settings_routes.rs` (14 handlers take `web::Data<Arc<SqliteSettingsRepository>>`, for example `:485`, `:521`, `:725`, `:754`, `:935`) | settings routes | reads and writes settings | SQLite | Yes: `OptimizedSettingsActor` (`GetSettings`, `GetSettingsByPaths`) | needs-owner-call | Grouped, not listed one by one. Outside `src/handlers`, and I did not audit which of the 14 update the actor as well as the store. |
| `bots_handler.rs:89,203,243` | `BOTS_GRAPH` static used by `update_bots_graph` (`:190`) and `get_bots_data` (`:216`) | `update_bots_graph` writes only to a process-static `GraphData`; `get_bots_data` prefers the actor's copy and falls back to the static | in-memory static, no store | Yes: `UpdateBotsGraph` (`graph_state_actor.rs:1069`, forwarded at `graph_service_supervisor.rs:1994`) | route-through-actor | `POST /api/bots/update` never reaches the actor, so the write is invisible to `GET /api/bots/data` whenever the actor has nodes. Delete the static. |

Noted but not inventoried (they message the actor, so they are not bypasses): `AddNodesFromMetadata` sent straight to the graph actor from `api_handler/files/mod.rs:84,257` and `api_handler/graph/mod.rs:523` (the X-2 annotation lists these).

## What I could not resolve

- Whether the nine `graph_state_handler` routes return 404 at runtime (reachability caveat 1). It needs one `curl` against a running server.
- Whether `schema_handler`, `semantic_handler` and `physics_handler` extract `Addr<GraphStateActor>` or `Arc<RwLock<GraphData>>` that `main.rs` never registers (the X-2 annotation says "needs a runtime check"). I did not examine them.
- The 14 `settings_routes.rs` handlers, individually.
- Whether the three `needs-owner-call` ontology rows (`:710`, `:745`, `:786`) have equivalent semantics in `OntologyActor`; I read only the handler signatures, not the reasoner paths.
