---
id: ADR-2138
title: Directed cycles are one server analysis that every client consumes
date: 2026-10-13
decision_status: accepted
implementation_status: complete
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: 772039bd172027af034d64befacca1ebcf0bbc04
verified_paths: [src/layout/scc.rs, src/handlers/api_handler/analytics/cycles_handlers.rs, src/settings/api/settings_routes.rs, client/src/api/layoutApi.ts, client/src/features/graph/hooks/useCyclicNodeIds.ts, xr-client/rust/src/render_store.rs, xr-client/scripts/hud.gd]
owner: jjohare
review_trigger: "Activation (staged → live): the XR Shapes tab first run on a headset against a live graph. Review: before any cycle datum is added to the 52-byte position frame"
repo: visionclaw
---

# ADR-2138 — Directed cycles are one server analysis that every client consumes

## Context

Users had no way to see which nodes sit on a directed cycle (a subclass loop is a modelling error), and the desktop and XR layout-mode vocabularies had drifted: the server whitelist rejected `spectral` and `clustered`, which the engine enum and the XR cycle button already used. A desktop-only answer would run Tarjan in the browser, which cannot serve the XR client or agents. Separately, the CPU `hierarchical_layout` fallback ranked by BFS from in-degree-zero roots, so a rootless cycle fell onto a trailing "unreachable" layer. Production `Hierarchical` does not run that code: it is GPU-resident (ADR-141 P4), ranked by `compute_dag_ranks` (BFS shortest-depth over `Edge::asserts_subsumption` edges, terminating on cycles, pinned by its unit tests).

## Decision

1. `src/layout/scc.rs` is the only directed-cycle implementation: iterative Tarjan, condensation longest-path ranks, `directed_adjacency` and `cycle_report`. The CPU `hierarchical_layout` fallback ranks with it. The GPU `compute_dag_ranks` is **unchanged**: its shortest-depth semantics are deliberate and tested.
2. `GET /api/analytics/cycles[?edgeType=<type>|all]` returns `{nodeCount, edgeCount, edgeFilter, cyclicNodeIds, components}`. It reads the graph through `GetGraphData` and is a public read like `/api/graph/data`. Its default edge set is `Edge::asserts_subsumption`, the predicate the GPU DAG ranker already uses, because the collapsed `hierarchical` label also carries equivalence and `sameAs`, and mutual links and `co_citation` edges are symmetric: each would read as a two-node cycle. REST only: the 52-byte position frame and the analytics buffer are not touched.
3. Clients consume that endpoint and carry no analysis of their own. Desktop: colour scheme `cycles` via `useCyclicNodeIds`. XR: a dedicated `RenderStore` cycle overlay (`set_cycle_overlay` / `clear_cycle_overlay`, separate from the query-variable channel; a query mark outranks it).
4. The server layout-mode whitelist, `layoutApi.ts` and the registry accept the full engine vocabulary, adding `spectral` and `clustered`.
5. XR HUD gains a ninth **Shapes** tab (Planes, Radial Shells, Cycles). Layout keeps Hierarchy & View and Layout Mode at the default separation.

## Consequences

- One cycle analysis for the overlay and agents, over the same edges the GPU layout ranks. The corpus-level `SUBCLASS_CYCLE` check (`vault conflicts`, `ontology_conflict_gate`) stays separate because it reads the disk, not the live graph; the agentbox `ontology-curator` agent documents both.
- Layout and overlay share the edge predicate and the Tarjan code only where they meet (CPU fallback); the GPU ranker does not call `layout::scc`. Moving it onto longest-path ranks would change shell placement and is out of scope.
- The report is recomputed per request (O(V+E)); the desktop refetches when node or edge counts change, XR on toggle or Refresh. No caching until measured.
- Nine tabs in a 1280 px viewport is tighter than eight; unmeasured on a headset.
- Not done, by design: edge bundling, density cloud, colormaps, layout recipe/provenance (needs a wire entry), seeding the GPU warm start (touches the `SimParams` ABI).

## Verification

Not yet pinned to a commit, so the ledger reads `proposed` / `none` until the owner accepts it and the change lands with a `verified_commit`. Evidence at time of writing: `cargo test --lib -- settings layout analytics` 40 pass (includes scc, `hierarchical_layout` on cyclic and rootless-cycle input, and the `cycles_handlers` edge selector); `xr-client/rust` `cargo test --lib cycle_overlay` 1 pass; client `tsc --noEmit` clean and 58 vitest tests pass across `src/api` and `src/features/graph/hooks` (the wider graph, api and control-centre run was 355 before the last additions), including `getCycles` against the real `StandardResponse` envelope; agentbox `tests/config/agent-reconcile.test.sh` 45 pass. Not verified: the GDScript (no Godot binary or gdlint in the container or on HP's PATH), the endpoint against a live graph (the VisionClaw container was not running), the real edge mix (7548 `hierarchical` edges were reported on one deployment; how many carry subClassOf provenance is unmeasured), and the Shapes tab on a headset.

## Close-out 2026-10-30

`proposed / none / inactive → accepted / complete / staged` under close-out rule 3a. The change landed on main in
`4306c65e9` ("feat(layout): directed-cycle analysis on the server, SCC-ranked hierarchy, cycles colour scheme").
Verified at `772039bd172027af034d64befacca1ebcf0bbc04`.

**Each decision against the code.**
1. `src/layout/scc.rs` exists. The GPU `compute_dag_ranks` (`src/actors/gpu/force_compute_actor.rs:438`) is unchanged: `git show 4306c65e9 --stat` does not list that file, and the commit names the function only in this record's text.
2. `GET /api/analytics/cycles` is routed at `src/handlers/api_handler/analytics/mod.rs:255` and handled in
   `cycles_handlers.rs`. On the running server (provenance below) it answered
   `{"nodeCount":9688,"edgeCount":10958,"edgeFilter":"subsumption","cyclicNodeIds":[],"components":[]}`.
3. The desktop hook is `client/src/features/graph/hooks/useCyclicNodeIds.ts`, used by `GemNodes.tsx`. The XR overlay
   is `RenderStore::set_cycle_overlay` (`xr-client/rust/src/render_store.rs:1273`).
4. `spectral` and `clustered` are in the settings whitelist (`src/settings/api/settings_routes.rs:413-414`) and in
   `client/src/api/layoutApi.ts:58`, `:76`. The separate advertising list `LIVE_LAYOUT_MODES` in
   `src/handlers/layout_handler.rs:13-28` still withholds `clustered`, by a deliberate comment dating from `ac3e12dd1`
   (`Clustered` is indistinguishable from `ForceDirected` in the actor). That is the advertised menu, not the whitelist
   this decision names.
5. The Shapes tab carries the Planes, Radial Shells and Cycles groups (`xr-client/scripts/hud.gd:582`, `:711`).

Runtime provenance: `visionclaw_container` runs `/app/target/dev-runtime/visionclaw-server` (build stamp `features=gpu,ontology,dev-auth`, i.e. the dev profile with the dev-auth bypass compiled in); its wrapper log (`/app/logs/rust.log`) reads `[RUST-WRAPPER][2026-10-30 19:01:06] Building from published inputs: source_sha=772039bd172027af034d64befacca1ebcf0bbc04 source_dirty=0`, so the responses below come from this commit.

Activation is `staged`, not `live`. The server half answers, but the Shapes tab has not been run on a headset, and
this record's own trigger names that run.
