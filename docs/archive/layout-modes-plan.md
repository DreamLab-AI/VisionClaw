# Layout Modes Implementation Plan

## Context

VisionFlow is a collaborative AR application for humans and AI agents to explore
knowledge graphs in 3D. The physics engine runs on GPU (CUDA, RTX A6000) with
real-time WebSocket position streaming. This plan defines the layout mode system
based on research into graph visualization for knowledge discovery.

## Current State

The existing force model is basic Fruchterman-Reingold (linear springs, uniform
repulsion). This produces a featureless ball — communities don't separate because
linear attraction doesn't penalize long edges enough.

## Priority 1: ForceAtlas2 with LinLog Mode

**Why**: LinLog mode replaces linear springs with `log(1 + distance)` attraction.
This makes the energy function equivalent to Newman modularity — energy minimization
directly maximizes community separation. No separate clustering step needed.

**Key differences from current model**:

| Property | Current (FR) | ForceAtlas2 LinLog |
|----------|:------------:|:------------------:|
| Repulsion | `repelK / d^2` | `(deg_i + 1)(deg_j + 1) / d` |
| Attraction | `springK * (d - restLength)` | `log(1 + d)` per edge |
| Hub handling | Equal for all | Degree-scaled repulsion |
| Convergence | Global temperature | Per-node adaptive speed |
| Complexity | O(n^2) or grid | O(n log n) Barnes-Hut octree |

**Implementation**: Modify `visionflow_unified.cu` repulsion and spring kernels.
The octree (Barnes-Hut) is the main new code. Reference: govertb/GPUGraphLayout.

**Parameters**:
```
scalingRatio:     10.0      // overall spread
gravity:          1.0       // prevent disconnected drift  
linLogMode:       true      // CRITICAL for community separation
dissuadeHubs:     true      // prevent mega-hubs dominating
barnesHutTheta:   0.5       // accuracy (lower = more exact)
barnesHut3D:      true      // octree for 3D
```

**Performance**: ~0.5ms per iteration at 2,812 nodes on A6000. Convergence in
300 iterations from random init, 100 from spectral init.

## Priority 2: Spectral Initialization

Compute eigenvectors 2, 3, 4 of the normalized graph Laplacian. Use as initial
positions for ForceAtlas2. The Fiedler vector (eigenvector 2) gives the optimal
graph bisection — initial positions already capture global structure.

**Implementation**: cuSPARSE Laplacian + cuSolver LOBPCG. ~50ms for 2,812 nodes.

## Priority 3: Layout Mode Switcher

| Mode | When to Use | Implementation |
|------|-------------|---------------|
| **Force-directed (FA2 LinLog)** | Default exploration | GPU Barnes-Hut octree |
| **Hierarchical (Sugiyama)** | Ontology class taxonomy | CPU acceptable at 2.8K |
| **Radial (centrality rings)** | Hub analysis | GPU PageRank + ring placement |
| **Side-by-side** | Knowledge vs ontology comparison | X-offset per graph type |
| **Temporal (Z = time)** | Knowledge evolution | Z-axis by timestamp |
| **Metanode overview** | Cluster navigation | Louvain → super-nodes |

Transitions between modes: LERP positions over 0.5-1.0 seconds.

## Priority 4: Constraint Zones

Soft zone forces overlay on the main layout:

```
Zone A (Y > 0):     Ontology classes — attracts OwlClass nodes upward
Zone B (Y < 0):     Knowledge instances — attracts page nodes downward  
Zone C (center):    Hub nodes (degree > mean + 2σ)
Zone D (periphery): Isolated nodes on Fibonacci sphere shell
```

Zone force: `strength * (distance_to_zone_center - zone_radius)` when outside radius.
Strength: 0.01-0.05 × standard repulsion.

## Priority 5: Cluster Visualization Upgrades

| Feature | Current | Upgrade |
|---------|---------|---------|
| Boundaries | Convex hull | Alpha shape (concave, tight-fitting) |
| Colors | HSL golden angle | Glasbey perceptual maximization |
| Navigation | None | Click hull → zoom into cluster |
| Overview | Full graph always | Metanode collapse/expand |
| Edge bundling | None | FDEB for inter-cluster edges |

## Priority 6: Interaction Techniques

**Tier 1 (implement now)**:
- Ray-cast selection + hover info
- Semantic zoom (distance-based LOD)
- Grab and drag (node repositioning with physics)
- Cluster collapse/expand (metanodes)

**Tier 2 (next sprint)**:
- Focus+context (iSphere magnification)
- Path highlighting (GPU BFS shortest path)
- Progressive disclosure (start with top-N centrality nodes)

**Tier 3 (AR differentiators)**:
- AI agent spatial presence (avatar + attention ray)
- Shared spatial annotations
- Multi-user conflict resolution
- Layout mode switching via gesture/voice

## Dual-Graph Display Modes

| Mode | Description | graphSeparationX |
|------|-------------|:----------------:|
| Merged | All nodes in one layout (default) | 0 |
| Side-by-side | Knowledge at -X, ontology at +X | 500-1000 |
| Layered 2.5D | Ontology above (Y+), instances below (Y-) | 0 |
| Schema-constrained | Classes pinned, instances cluster around them | 0 |

Agent nodes always at origin (bridging both populations).

## Key References

- ForceAtlas2: PMC4051631 (Jacomy et al. 2014)
- LinLog = modularity: Noack 2009
- GPU Barnes-Hut: Burtscher & Pingali 2011
- GPUGraphLayout: github.com/govertb/GPUGraphLayout
- H3 Hyperbolic: Munzner 1997
- Spectral layout: Koren 2005
