---
id: ADR-2138
title: Directed cycles are one server analysis that every client consumes
date: 2026-10-13
decision_status: proposed
implementation_status: none        # code is in the working tree, uncommitted; set to partial and pin verified_commit when it lands
activation_status: inactive
supersedes: []
superseded_by: []
verified_commit:
verified_paths: []
owner: jjohare
review_trigger: Before any cycle datum is added to the 52-byte position frame, or when the XR Shapes tab is first run on a headset
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
