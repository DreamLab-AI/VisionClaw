# Physics Parameter Analysis & Remediation Plan

## Graph Topology (the input)

| Metric | Value |
|--------|-------|
| Nodes | 2,242 |
| Edges | 4,531 |
| Mean degree | 3.8 |
| Isolated nodes (degree 0) | 468 (21%) |
| Leaf nodes (degree 1) | 477 (21%) |
| Connected components | 492 |
| Largest component | 1,722 nodes (77%) |
| Singleton components | 468 |
| Hub (deg > 50) count | 5 |
| Max degree | 149 ("Artificial Intelligence") |

**Key insight**: 77% of nodes are in one giant component. 21% are isolated singletons. The graph is sparse (mean degree 3.8) with a power-law degree distribution and 5 mega-hubs.

## Current State (SPREAD OUT config): What's Wrong

With `restLength=100`, the **median edge length is 103** — springs are working correctly. But:

- **Edge length / node spread ratio = 1.35** — connected nodes are as far apart as random pairs
- **All 2,242 nodes fit in a 256x270x233 cube** — the graph is a uniform ball with no visible community structure
- **Hubs are NOT central** — "Artificial Intelligence" (deg 149) is at distance 120, same as random nodes
- **Isolated nodes occupy the same space as connected nodes** — no separation

The force-directed layout has found a **local minimum** where repulsion balances springs uniformly, producing a featureless sphere instead of revealing community structure.

## Parameter Space Exploration Results

| Config | DistMean | DistStd | P50 | Extreme% | Moderate% | Quality |
|--------|:--------:|:-------:|:---:|:--------:|:---------:|:-------:|
| BASELINE (extreme params) | 584 | 535 | 461 | 27.7% | 5.7% | POOR |
| BALANCED | 254 | 247 | 188 | 0% | 21.9% | POOR |
| TIGHT CLUSTER | 178 | 177 | 126 | 0% | 30.9% | POOR |
| SPREAD OUT | 122 | 66 | 104 | 0% | 90.0% | GOOD* |
| GRAPH-THEORETIC | 66 | 26 | 64 | 0% | 71.6% | GOOD* |
| BOUNDED | 27 | 18 | 30 | 0% | 10.0% | FAIR |

*GOOD statistically but visually featureless — no community separation visible.

## Root Causes

### 1. Uniform repulsion prevents structure
All node pairs repel equally. In a good force-directed layout, repulsion should be **distance-scaled** so nearby nodes repel strongly but distant nodes are ignored (Barnes-Hut approximation). The current `repulsionCutoff` helps but isn't enough — the cutoff should adapt to local density.

### 2. No multi-scale force hierarchy
The physics has one repulsion scale and one spring scale. For 2,242 nodes with power-law degree distribution, you need:
- **Local**: short-range repulsion between nearby nodes (prevent overlap)
- **Cluster**: medium-range attraction within communities (springs)
- **Global**: weak gravity to keep components together
- **Inter-cluster**: repulsion between clusters (currently missing)

### 3. Fast-settle converges to nearest equilibrium, not best
The 167ms fast-settle with exponential reheat decay (1.0 → 0.06 in 40ms) finds the nearest local minimum — always a uniform sphere. Proper force-directed layout needs:
- **Simulated annealing**: slow cooling from high temperature
- **Multi-resolution**: coarsen graph → layout → refine → layout
- **Or**: Run thousands of iterations at moderate damping, not 10 at high damping

### 4. 468 isolated nodes pollute the layout
21% of nodes have zero edges. They experience only repulsion + gravity, so they fill available space uniformly. They should be placed in a separate region or filtered.

### 5. restLength doesn't scale with graph density
`restLength=100` with 4,531 edges means the total spring network wants to fill a cube of side ~100. With 2,242 nodes in 3D, the optimal packing density for visible structure is much smaller.

## Recommended Fix: Optimal Default Parameters

For a 2,242-node graph with mean degree 3.8:

```
Ideal restLength ≈ (V / N)^(1/3) where V is desired volume
For a 500-unit display cube: restLength ≈ (500^3 / 2242)^(1/3) ≈ 37
```

| Parameter | Current | Recommended | Rationale |
|-----------|:-------:|:-----------:|-----------|
| restLength | 100 | 30-40 | Scale to graph size for visible structure |
| repelK | 2000 | 500 | Moderate — enough to prevent overlap |
| springK | 5 | 30 | Stronger — pull communities together |
| centerGravityK | 0.1 | 0.3 | Moderate — keep main component centered |
| damping | 0.85 | 0.92 | Higher — prevent oscillation |
| maxVelocity | 200 | 50 | Lower — smoother settling |
| maxForce | 1000 | 200 | Lower — prevent explosion |
| repulsionCutoff | 1000 | 150 | Lower — localize repulsion |
| maxRepulsionDist | 5000 | 300 | Lower — long-range repulsion hurts clusters |
| iterations | 50 | 200 | More — allow proper settling |
| temperature | 1.0 | 0.5 | Lower — less random energy |
| coolingRate | 0.001 | 0.005 | Faster cooling → stable layout |

### Critical algorithmic fixes needed:

1. **Separate isolated nodes**: Nodes with degree 0 should be placed in a ring around the main graph, not mixed in
2. **Degree-scaled gravity**: Hubs should be more central (`gravity_force *= log(1 + degree)`)
3. **Multi-pass settle**: Instead of 10 iterations at decaying reheat, run 200+ iterations at moderate damping
4. **Cluster-aware repulsion**: After initial layout, detect clusters (Louvain) and add inter-cluster repulsion

## Immediate Action Items

### Phase 1: Better defaults (code change)
- Update `data/settings.yaml` or default PhysicsSettings with the recommended values above
- Increase fast-settle `max_settle_iterations` from 2000 to 5000
- Change reheat decay from 0.7x per step to 0.95x per step (slower decay = more exploration)
- Set `warmupIterations` to 200 (currently 100)

### Phase 2: Isolated node handling (code change)
- In `force_compute_actor.rs`, detect degree-0 nodes after graph upload
- Place them in a spherical shell around the main component (radius = 1.5 * main_component_radius)
- Exclude them from physics simulation (pin in place)

### Phase 3: Degree-weighted forces (GPU kernel change)
- In `visionflow_unified.cu`, scale gravity by `log(1 + degree[node_idx])`
- Upload degree array to GPU alongside positions
- This pulls hubs to center naturally

### Phase 4: Multi-resolution layout (architecture change)
- Coarsen graph (merge clusters into super-nodes)
- Layout coarse graph (fast, ~50 nodes)
- Expand back and refine with springs
- This is the gold standard for large graph layout (OpenOrd, ForceAtlas2)
