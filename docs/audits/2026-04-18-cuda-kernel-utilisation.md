---
title: CUDA Kernel Utilisation Audit
description: Inventory of all 91 CUDA kernels in src/utils/*.cu and identification of unused and under-leveraged compute surfaces, especially the three semantic-force kernels that would consume the metadata we just wired up
date: 2026-04-18
status: findings
---

# CUDA Kernel Utilisation Audit

## Summary

**91 CUDA kernels across 11 files. At least 24 are unused.** Several unused kernels implement major capabilities (PageRank, SSSP, Barnes-Hut stress majorisation). Additionally, three `semantic_forces.cu` kernels read exactly the OWL metadata fields that were NULL on every GraphNode until today's parser fix — they now have data to operate on but aren't yet invoked from the actor pipeline.

## Kernel inventory by file

| File | Kernels | Wired | Unused |
|---|---:|---:|---:|
| `visionflow_unified.cu` | 30 | ~28 | 2 (stability variants) |
| `gpu_clustering_kernels.cu` | 22 | 18 | 4 (DBSCAN variants, Louvain init) |
| `semantic_forces.cu` | 15 | **8** | **7** (physicality, role, maturity + centroids) |
| `pagerank.cu` | 8 | **0** | **8** (entire pipeline orphaned) |
| `ontology_constraints.cu` | 5 | 5 | 0 |
| `visionflow_unified_stability.cu` | 4 | 3 | 1 (`check_stability_kernel`) |
| `gpu_landmark_apsp.cu` | 3 | 1 | 2 (landmark selection, compact frontier) |
| `gpu_connected_components.cu` | 3 | 2 | 1 (`count_components_kernel`) |
| `sssp_compact.cu` | 2 | **0** | **2** (plus 3 more via supplementary files) |
| `gpu_aabb_reduction.cu` | 1 | 1 | 0 |
| `dynamic_grid.cu` | 0 (helpers only) | — | — |
| **Total** | **91** | **~67** | **~24** |

## High-value unused kernels

### Semantic forces driven by OWL metadata (highest-value unlock)

These kernels were written against the same metadata schema that, until today, was never populated on GraphNodes. My earlier fix in `knowledge_graph_parser.rs` now persists these fields — wiring the kernels is a small step with high payoff.

| Kernel | Metadata it consumes | What it does |
|---|---|---|
| `apply_physicality_cluster_force` | `owl:physicality::` (abstract / concrete / virtual / spatial) | Clusters nodes of the same physicality around a computed centroid; distinct physicalities repel |
| `apply_role_cluster_force` | `owl:role::` (concept / process / object / actor / relation) | Clusters nodes by semantic role — "all processes" form one cluster, "all actors" another |
| `apply_maturity_layout_force` | `maturity::` / `status::` (draft / enriched / authoritative) | Layered layout by maturity — authoritative nodes at stable core, drafts at periphery |
| `calculate_physicality_centroids` + `finalize_physicality_centroids` | — | Helper centroid computation for the above |
| `calculate_role_centroids` + `finalize_role_centroids` | — | Helper centroid computation |

**All three force kernels exist, compile, and are dormant.** They just need:
1. A Rust FFI binding (probably in `src/actors/gpu/semantic_forces_actor.rs` which already has matching config structs)
2. Upload path for the per-node metadata flag buffer (physicality code, role code, maturity level)
3. Dispatch wiring in `ForceComputeActor` or `SemanticForcesActor`

### PageRank (entire pipeline orphaned)

Eight kernels — full distributed PageRank with dangling node handling and optimised variants. Referenced only from `PageRankActor` which is spawned but never invoked from a request handler (per earlier audit).

- `pagerank_init_kernel`
- `pagerank_iteration_kernel`
- `pagerank_iteration_optimized_kernel`
- `pagerank_dangling_kernel`
- `pagerank_dangling_sum_kernel`
- `pagerank_dangling_distribute_kernel`
- `pagerank_normalize_kernel`
- `pagerank_convergence_kernel`

Wiring PageRank through to an API endpoint and surfacing the score on the `analytics_update` JSON channel (post-[ADR-061](../adr/ADR-061-binary-protocol-unification.md): analytics no longer ride the per-frame binary protocol) would give users a centrality metric in the visualisation — direct visual value.

### Single-source shortest paths (orphaned)

Five kernels across `sssp_compact.cu` and supplementary files:
- `sssp_init_distances_kernel`
- `sssp_frontier_relax_kernel`
- `sssp_relax_edges_kernel`
- `sssp_detect_negative_cycle_kernel`
- `select_landmarks_kernel`

The existing UI has an SSSP overlay (per `InstancedLabels.tsx:143-154` which renders `Distance: X.XX` under nodes when an SSSP result is active), but the GPU kernels are bypassed in favour of a CPU path. Wiring the GPU SSSP would make the overlay live-responsive to source changes.

### Barnes-Hut stress majorisation

`stress_majorization_barneshut_kernel` exists but the slower O(n²) `compute_stress_kernel` is used instead. The Barnes-Hut version is ~10× faster on 1000+ node graphs.

### Efficient KE reduction

`reduce_kinetic_energy_kernel` is a proper parallel reduction for computing total KE. The current path computes KE on the host after downloading all velocities (slow). Wiring this would make settlement-state telemetry cheap at 60 Hz.

## Wiring proposal — semantic forces (concrete plan)

### Stage 1 — metadata upload

Extend the existing per-node flag buffer that already transfers `node_type` flags to GPU. Add three new 8-bit columns:

```c
// GPU per-node metadata (one byte each)
uint8_t physicality_code;   // 0=abstract, 1=concrete, 2=virtual, 3=spatial, 255=unknown
uint8_t role_code;          // 0=concept, 1=process, 2=object, 3=actor, 4=relation, 255=unknown
uint8_t maturity_level;     // 0=draft, 1=enriched, 2=authoritative, 255=unknown
```

Rust-side mapping lives next to `classify_node_population`. Unknown values (255) are exempted from the corresponding force.

### Stage 2 — kernel dispatch order

Inside the force-computation tick loop (physics_orchestrator → ForceCompute):

1. Existing: `force_pass_kernel` (spring/repulsion/gravity)
2. Existing: `apply_subclass_hierarchy_kernel`, `apply_disjoint_classes_kernel`, `apply_sameas_colocate_kernel`
3. **NEW**: `calculate_physicality_centroids` → `finalize_physicality_centroids` → `apply_physicality_cluster_force`
4. **NEW**: `calculate_role_centroids` → `finalize_role_centroids` → `apply_role_cluster_force`
5. **NEW**: `apply_maturity_layout_force`
6. Existing: `integrate_pass_kernel`

Each new force has its own strength parameter in the physics config, tunable per-domain in `config/domains.yaml` (from the unified pipeline design doc).

### Stage 3 — strength parameters

Default starting magnitudes (owner tunable):

```yaml
physics:
  physicality-cluster-strength: 0.4    # moderate — separates abstract from concrete
  role-cluster-strength: 0.3           # weak — keeps roles tethered without dominating
  maturity-layout-strength: 0.15       # very weak — subtle drift, authoritative inward
```

### Stage 4 — visual feedback

Once wired, these forces produce:
- **Abstract vs concrete separation** — concepts like `Policy`, `Strategy` cluster together; concrete entities like `RobotArm`, `SensorUnit` cluster separately
- **Role-based bands** — all processes form one zone, all actors another, all objects another
- **Maturity gradient** — newly-drafted nodes migrate outward, authoritative pinned near ontology hubs

This is the exact "sensible metadata-reflective physics" the owner asked for.

## Actionable ranking

**P1 — High impact, low effort, code already exists:**

1. Wire `apply_physicality_cluster_force` + centroids — Rust FFI + dispatch in semantic_forces_actor
2. Wire `apply_role_cluster_force` + centroids
3. Wire `apply_maturity_layout_force`

**P2 — Medium effort, high impact:**

4. Wire PageRank through an API handler (`GET /api/analytics/pagerank`) + add to V3 binary analytics fields
5. Wire GPU SSSP (replace CPU path already used by the UI overlay)
6. Switch stress majorisation to Barnes-Hut kernel (performance win on >500 nodes)
7. Replace host-side KE computation with `reduce_kinetic_energy_kernel`

**P3 — Clean-up:**

8. Delete or document `check_stability_kernel`, `count_components_kernel`, unused DBSCAN variants, `compact_frontier_atomic_kernel`
9. Add `#[cfg(unused)]` guards around dead kernel loading code to stop eating PTX cache slots

## Estimate

- P1 (all three semantic forces wired): ~2 days work
- P2 (four items): ~1 week
- P3 (cleanup): half a day

Total: ~1.5 weeks to reach full kernel utilisation.

## Relation to the unified pipeline design

This audit aligns with and extends the earlier design doc `docs/design/2026-04-18-unified-knowledge-pipeline.md`. The parser fix committed today (`b501942b1`) unlocks the metadata that these kernels consume. Wiring the three semantic forces is the natural next PR and will produce the first visible "ontology-shaped layout" the owner described.
