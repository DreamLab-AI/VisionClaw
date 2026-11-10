---
id: ADR-2192
title: The reasoner's layout constraints, the sync pipeline's reasoning and materialisation, and node thinning are stored operator switches
date: 2026-11-10
decision_status: proposed
implementation_status: complete
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit: 77305d3811a0e14d8d33d60c6b6d2178a94b9805
verified_paths: [crates/visionclaw-domain/src/types/physics_config.rs, src/actors/gpu/ontology_constraint_actor.rs, src/actors/gpu/force_compute_actor.rs, src/actors/gpu/physics_supervisor.rs, src/actors/messages/physics_messages.rs, src/settings/models.rs, src/settings/api/settings_routes.rs, src/services/github_sync_service.rs, src/handlers/api_handler/ontology_physics/mod.rs, src/actors/client_filter.rs, src/actors/client_coordinator_actor.rs, src/handlers/api_handler/graph/mod.rs, client/src/features/control-center/registry/groups/quality.ts, client/src/features/control-center/registry/groups/motion.ts, client/src/features/ontology/components/OntologyForcesPanel.tsx]
relates: [ADR-2140, ADR-2183, ADR-2189, ADR-2173, ADR-2071, ADR-098]
owner: jjohare
review_trigger: "The operator accepts or rejects the switches after using them on the dev stack; a request to thin the GPU simulation itself; any new sync-time producer of nodes or edges."
repo: visionclaw
domain: BASELINE-architecture
evidence_class: source+test
acceptance_boundary: "Local runs only: not built into or deployed on the dev stack, no GPU used. Server behaviour is shown by unit tests of the pure decisions (constraint sync action, live buffer, switch parsing, domain-root plan, inferred-edge purge set, node cap, REST/stream parity) and one actix test of the constraint actor without a GPU context. The device clear has a GPU test (constraint_schedule_tests::a_set_replaced_by_the_empty_set_leaves_the_engine_and_stays_off) that skips without a device and was not run. No sync was run, so the 110 s saving and the purges are not observed. The client is shown by Vitest and tsc, not a browser."
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
   - They are not `SimParams` fields. `SetOntologyConstraintPolicy` carries them to the constraint actor, sent from three places: `apply_physics_patch` (only when the policy changed, because each application re-uploads the set and restarts its ramp), the layout reset, and every physics-supervisor (re)start (`restore_saved_physics`).
   - A layout reset keeps `ontologyConstraintsEnabled`, as it keeps `layoutMode`: the switch is an operator choice, not tuning. An operator who switched the constraints off and then reset a collapsed layout does not get them back. The strength is tuning, so it resets to 0.6.
   - Off empties the live buffer and keeps the unscaled set, so on re-applies it without a sync. `ForceComputeActor` clears the device when a writer replaces its cache with the empty set (`constraint_sync_action`: an empty, pending cache over a non-empty engine is `Clear`). That one clear removes everything the engine holds, including any set `ConstraintActor` wrote to it directly. An empty cache nobody asked for never clears.
2. **The sync pipeline has three stored quality gates**, read key by key from the global settings layer each time a sync, or the start-up reasoning load, runs. They take effect at the next sync or restart, not live. Each defaults to `true`, which is what the sync did, and a missing key reads as on.
   - `qualityGates.ontologyReasoning` gates the start-up Whelk load and post-sync reasoning.
   - `qualityGates.materialiseDomainRoots`: when off, `plan_domain_roots` has no live domain, so the stored roots and every edge on them are purged.
   - `qualityGates.materialiseInferredEdges`: when off, or when reasoning is off, no inferred edges are written and the stored `inferred:true` edges are removed.
3. **What degrades when reasoning is off:**
   - The ontology-agent routes (`OntologyQueryService`, through the shared engine) hold no inferred closure, so they answer from asserted axioms only.
   - No inference results are persisted.
   - No inferred edges are written.
   - The layout constraints come from asserted axioms only (subClassOf, disjointWith, equivalentClass as written), dispatched without Whelk. With the constraint switch on, the layout keeps those forces and loses only the inferred ones.
4. **The retired copies:**
   - `qualityGates.ontologyPhysics` and `ontologyStrength` are retired, read and dropped, following the ADR-2189 `gnnPhysics` precedent. A stored `ontologyPhysics: false` was never in force, so giving it meaning would have switched the constraints off on deploy.
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
   - After the recompute, the coordinator answers `filter_update` with `filter_update_success { visible_nodes, total_nodes }`.
   - `GET /api/graph/data?apply_node_filter=true` applies the stored filter's removal through the same function, and the desktop client sends the parameter while removal is on.
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
At `77305d381`, with no GPU:
- `cargo test -p visionclaw-server --lib` exercises the new tests:
  - `constraint_sync_action_tests` (2) and `ontology_constraint_actor::tests` (live buffer; the actix policy off/on test);
  - `ontology_physics::tests` (2) and `sync_switch_tests` (3);
  - `domain_root_plan_tests::switched_off_domain_roots_are_purged_with_their_spokes`;
  - `client_filter` cap tests (2) and `rest_removal_matches_the_stream_filter_and_is_opt_in`;
  - the settings-model round trips (`sync_switches_default_on_for_old_documents_and_round_trip`, `quality_gates_with_retired_keys_still_deserialise`, `node_filter_max_nodes_round_trips_and_defaults_to_no_cap`).
- `cargo test -p visionclaw-domain` runs `ontology_constraint_keys_default_round_trip_and_load_from_rows_without_them`.
- The stored row captured from the live system (`src/actors/gpu/test_fixtures/live_physics_settings.json`, 56 keys) lacks both new keys and still deserialises (`live_physics_row_without_the_policy_keys_loads_todays_policy`).
- Client: `vitest run` (207 files, 1,916 tests) and `tsc --noEmit` are clean. That covers registry counts and placement (`(c9)`), the filter-sync `maxNodes` send, the count store and readout, and the Ontology panel writing the stored keys.
- The GPU test named in the acceptance boundary is left for a gate run.
