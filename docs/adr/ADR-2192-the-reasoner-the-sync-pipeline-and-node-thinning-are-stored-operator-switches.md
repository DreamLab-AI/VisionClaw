---
id: ADR-2192
title: The reasoner's layout constraints, the sync pipeline's reasoning and materialisation, and node thinning are stored operator switches
date: 2026-11-10
decision_status: proposed
implementation_status: complete
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 01196ca74206359e715febfed56e77fc6d1affc2
verified_paths: [crates/visionclaw-domain/src/types/physics_config.rs, src/actors/gpu/ontology_constraint_actor.rs, src/actors/gpu/force_compute_actor.rs, src/actors/gpu/physics_supervisor.rs, src/actors/messages/physics_messages.rs, src/settings/models.rs, src/settings/api/settings_routes.rs, src/services/github_sync_service.rs, src/handlers/api_handler/ontology_physics/mod.rs, src/actors/client_filter.rs, src/actors/client_coordinator_actor.rs, src/handlers/api_handler/graph/mod.rs, client/src/features/control-center/registry/groups/quality.ts, client/src/features/control-center/registry/groups/motion.ts, client/src/features/ontology/components/OntologyForcesPanel.tsx]
relates: [ADR-2140, ADR-2183, ADR-2189, ADR-2173, ADR-2071, ADR-098]
owner: jjohare
review_trigger: "The operator accepts or rejects the switches after using them on the dev stack; a request to thin the GPU simulation itself; any new sync-time producer of nodes or edges."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source+test
acceptance_boundary: "Local runs only: not built into or deployed on the dev stack. Server behaviour is shown by unit tests of the pure decisions (constraint sync action, live buffer, switch parsing, domain-root plan, inferred-edge purge set, node cap, REST/stream parity) and one actix test of the constraint actor without a GPU context. The source split has a GPU test (constraint_schedule_tests::switching_the_ontology_source_off_and_on_leaves_the_user_set_on_the_device, #[ignore] without a device), run on one RTX 6000 Ada (CUDA_VISIBLE_DEVICES=2, CUDA_ARCH=89). No sync was run, so the 110 s saving and the purges are not observed. The client is shown by Vitest and tsc, not a browser."
---

# ADR-2192 — The reasoner, the sync pipeline and node thinning are stored operator switches

## Context
- On the dev stack (9,688 nodes, 147,048 edges), start-up Whelk produced 81,049 inferred axioms, post-sync reasoning took 110 s, 104,489 materialised axioms went to the GPU constraint mapper, 9 domain roots were materialised and 0 inferred edges were written. Every node was streamed and simulated. The operator found the graph overloaded and the reasoner's contribution too strong. None of this could be switched off.
- The flags that seemed to govern it (`SemanticPhysicsConfig`: `auto_trigger_reasoning`, `constraint_strength: 1.0`, `materialise_inferred_edges: false`) belonged to `OntologyPipelineService`, whose entry point `on_ontology_modified` had no caller. The live path is `GitHubSyncService::sync_graphs` → `materialise_domain_roots` → `run_post_sync_reasoning` (which always wrote inferred edges) → `OntologyConstraintActor`, at a compiled-in strength of 0.6.
- Three more copies of the constraint switch existed and none was in force: `qualityGates.ontologyPhysics` (stored, default false), `qualityGates.ontologyStrength` (0.5, pushed only when `ontologyPhysics` was true), and the Ontology panel's local state (0.6), which posted to `/api/ontology-physics/{enable,disable,weights}`. `/disable` cleared the device, but `ForceComputeActor` still cached the full set and re-uploaded it on the next step, because the resident count differed from the cache.
- The node filter parsed and persisted `max_nodes` but never applied it. The client listened for counts that the server never sent. A REST reload (`/api/graph/data`) bypassed server-side removal.

## Decision
1. **The reasoner-derived constraints are two stored physics keys** (ADR-2140 decision 1):
   - `physics.ontologyConstraintsEnabled` (default `true`) and `physics.ontologyConstraintStrength` (default `0.6`, bound 0..=1 in `physics_bounds`). Both carry serde defaults, so a stored row without them loads today's behaviour.
   - They are not `SimParams` fields. `SetOntologyConstraintPolicy` carries them to the constraint actor, sent from three places: every `apply_physics_patch` (before the `SimParams` send, so a refusal there cannot leave the policy unapplied), the layout reset, and every physics-supervisor (re)start (`restore_saved_physics`).
   - The store cannot say whether an application succeeded, because it is written first. So the constraint actor holds the policy it last handed to the device writer (`applied_policy`) and answers an identical one without re-uploading. Anything else is applied, including the first policy after a restart. A refused or failed push leaves `applied_policy` unset, so the next write or supervisor start retries. A write that names a policy key gets a 500 saying it was stored but not applied.
   - A layout reset keeps `ontologyConstraintsEnabled`, as it keeps `layoutMode`: the switch is an operator choice, not tuning. An operator who switched the constraints off and then reset a collapsed layout does not get them back. The strength is tuning, so it resets to 0.6.
   - Off empties the live buffer and keeps the unscaled set, so on re-applies it without a sync.
   - **The switch governs the ontology-derived set only.** `ForceComputeActor` is the one device writer and keeps two sources on the host: the ontology set (`UpdateOntologyConstraintBuffer`) and the user set (`UpdateConstraints` from the constraints and analytics routes, and `UploadConstraintsToGPU` from the orchestrator). It uploads their union. Off removes only the ontology set, and an empty `UpdateConstraints` clears only the user set. The device is cleared only when both are empty (`constraint_sync_action`). `OntologyConstraintActor` no longer writes the engine directly, because a direct write replaced the user set too.
   - `GET /api/ontology-physics/constraints` reports the ontology count the device writer holds (`activeConstraints`), with `userConstraintsOnDevice` and `deviceSynced`. `deviceSynced` is false while an upload is pending for the next step.
   - **Restart.** The physics supervisor restarts all its actors together (all-for-one). Both sources are then empty and the reasoned set is gone, so "on" has nothing to re-apply until the next sync (`POST /api/admin/sync`) re-dispatches the constraints. User constraints return when their writers resend them. The stored policy itself is re-applied at the restart. Rebuilding the ontology set at actor start would need the ontology and graph stores inside the supervisor, so it is not built. The control's help text says this.
2. **The sync pipeline has three stored quality gates**, read key by key from the global settings layer each time a sync, or the start-up reasoning load, runs. They take effect at the next sync or restart, not live. Each defaults to `true`, which is what the sync did, and a missing key reads as on.
   - `qualityGates.ontologyReasoning` gates the start-up Whelk load and post-sync reasoning. When a sync runs with it off, the sync also forgets earlier inference: it clears the shared Whelk closure and the stored inferred graph, which holds the persisted inference results and their run id, so nothing keeps serving the last run.
   - `qualityGates.materialiseDomainRoots`: when off, `plan_domain_roots` has no live domain, so the stored roots and every edge on them are purged.
   - `qualityGates.materialiseInferredEdges`: when off, or when reasoning is off, no inferred edges are written and the stored `inferred:true` edges are removed.
3. **What degrades when reasoning is off:**
   - The ontology-agent routes (`OntologyQueryService`, through the shared engine) hold no inferred closure, so they answer from asserted axioms only.
   - No inference results are persisted, and earlier ones are cleared.
   - The gated per-view reasoner (`ontology_view_reasoner`, ADR-2184) is not switched separately. It never reasons while the switch is off: the hierarchy reads skip it when the shared closure is empty, and the `/inference/results` and `/inferred` reads now check for a stored run first, answering "none" or the empty graph without a per-view Whelk run.
   - No inferred edges are written.
   - The layout constraints come from asserted axioms only (subClassOf, disjointWith, equivalentClass as written), dispatched without Whelk. With the constraint switch on, the layout keeps those forces and loses only the inferred ones.
4. **The retired copies:**
   - `qualityGates.ontologyPhysics` and `ontologyStrength` are retired, read and dropped, following the ADR-2189 `gnnPhysics` precedent. A stored `ontologyPhysics: false` was never in force, so giving it meaning would have switched the constraints off on deploy.
   - **Migration.** The old strength was applied, on each quality-gate save, while `ontologyPhysics` was true. So under exactly that condition, a physics row without `ontologyConstraintStrength` takes `qualityGates.ontologyStrength` (when it is within 0..=1), at boot and on every saved-physics restore (`migrate_ontology_strength`). Before the migrated row is written back, the quality-gate row is archived unchanged under `quality_gates.archive.adr-2192`. The archive is written once and never overwritten. If it cannot be written, nothing from that boot's migration is persisted.
   - `/api/ontology-physics/{enable,disable,weights}` are thin writers of the two physics keys through `apply_physics_patch`. Out-of-range strength is a 400, not a clamp.
   - The Ontology panel reads and writes the same keys.
   - `OntologyPipelineService` (636 lines) and its orphan test are deleted.
5. **Not in the semantic `FIELDS` table.** These five values follow the stored-settings path instead, for four reasons:
   - `semantic_config.rs` defines that group as the mirror of the device struct `SemanticConfig` ("adding a field here is a change to both").
   - Any change to `semantic` re-uploads constant memory and reheats through the GPU change gate.
   - The packer (`unified_gpu_compute/semantic.rs`) and the device struct sit in directories reserved by another open change.
   - None of the five is a constant-memory force: two are a constraint-buffer policy, and three are sync-time switches.
6. **Node thinning reuses the ADR-2183 filter:**
   - `nodeFilter.maxNodes` (default 0, no cap; ceiling 100,000) is applied in `recompute_filtered_nodes` under the same opt-in as predicate removal (`enabled && serverRemove`). The nodes that pass are ranked by the predicate's own quality signal (unscored nodes after scored ones), then by degree, then by id.
   - **Who it reaches.** The stream applies each client's own filter: every desktop session sends its `filter_update`, `maxNodes` included, and thins its own stream. The value is the `nodeFilter` document, saved through `PUT /api/settings/node-filter`. Each write is broadcast to every connected session as `settingsUpdated`, and that payload is now the serialised stored document. Before, it listed fields by hand, left out `maxNodes`, and wiped the cap on every other desktop. So a cap set in one desktop session reaches the other open desktop sessions, and each then thins its own view. The headset ignores `maxNodes`. It sends `filter_update` only from its quality toggle, with `server_remove: false` and no `max_nodes`, so it never removes and never writes the stored row.
   - After the recompute, the coordinator answers `filter_update` with `filter_update_success { visible_nodes, total_nodes }`. M counts the nodes in scope, so with nothing removed the panel says "all". On every graph change (`UpdateNodeTypeArrays`, sent after a load or sync) the coordinator recomputes every client's filter and resends its counts. When the graph cannot be read, it answers `filter_update_error { error }`, and the panel shows the reason rather than a stale count.
   - `GET /api/graph/data?node_filter=<json>` takes the caller's own filter snapshot, the same body as its `filter_update`, built by one client function (`filterWireBody`). It is parsed with the socket's parser and mapped with the coordinator's function (`apply_filter_update`), then thinned by the stream's `recompute_filtered_nodes_admitting`. One order applies to both: visibility, scope, the request's `graph_type` population, predicates, then the cap. With no `graph_type`, REST keeps exactly the stream's nodes. With one, the cap keeps the best N of that population.
   - **This thins what a client receives and draws, not the GPU simulation**, which is shared by every viewer and the headset. Thinning the simulation would need a server-global population scope applied where the graph is uploaded to `ForceComputeActor`, with the constraint mapper resolving against the thinned set. That is not built.

## Consequences
- Defaults reproduce today's behaviour, so a deploy changes nothing until the operator flips a switch.
  - The constraint switch and strength act live. The pipeline switches act at the next sync (`POST /api/admin/sync`) or restart.
  - The 110 s saving and the purges need a sync to be seen.
- Conflict with the brief: it asked for the inferred-edge flag to stay off by default, but the live sync always wrote inferred edges, so this ADR defaults it **on**. It is a one-line flip (`default_on` → false on `materialise_inferred_edges` in `src/settings/models.rs`, and the client default). On this dataset the two choices look the same: 0 inferred edges were produced.
- Why every node is shown: the filter and `serverRemove` are off by default, and nothing else thins the desktop graph. The cheapest honest reduction is filter on, "Also remove on server" on, and Max Nodes set. The panel's "Showing N of M nodes" line reports the server's actual count.
- `SetOntologyConstraintPolicy` replaces `AdjustConstraintWeights`.
- `filter_update` no longer gets an immediate acknowledgement. Its one reply comes after the recompute.
- The `tests/node-filter-parity.json` default case does not list `maxNodes`: it is a server-removal setting, and the XR client never removes.
- Size: 24 server files and 31 client and IA files, plus this record and 16 amended or re-stamped ADRs. The stored-settings path cost model, validator, route, message, actor, client types, defaults, registry and test edits for each key, against the one-row-plus-regenerate the `FIELDS` table offers its own group.

## Verification
At `01196ca74` (the merge with main that includes ADR-2191), stamped by the following docs-only commit:
- `scripts/gpu-physics-gate.sh` passed in 988 s on an RTX 6000 Ada, PCI bus 0xAF (`CUDA_VISIBLE_DEVICES=2`, `CUDA_ARCH=89`). That covers:
  - fmt, and the CPU crates (build, clippy, test);
  - the server lib suite: 2,001 passed, 0 failed, 28 ignored;
  - 12 GPU buffer-lifecycle and SSSP tests under compute-sanitizer memcheck;
  - the 40-test force-kernel golden harness;
  - server clippy: 32 warnings, against main's baseline of 33;
  - the ADR index: no errors beyond the seven records stale on main.
- `switching_the_ontology_source_off_and_on_leaves_the_user_set_on_the_device` (`#[ignore]` without a device) passed on the same card, with the same environment. The module's three other device tests also passed with `VISIONCLAW_GPU_TESTS_REQUIRED=1`.
- `cargo test -p visionclaw-server --test corpus_local_sync`: 10 passed, 1 ignored. This includes the sync-switch test: reasoning and roots off, the purge and restore of roots, and reasoning off forgetting the closure and the stored results.
- Client: `vitest run` gives 207 files and 1,921 tests passed; `tsc --noEmit` exits 0.
- Not run: anything against the live stack.
