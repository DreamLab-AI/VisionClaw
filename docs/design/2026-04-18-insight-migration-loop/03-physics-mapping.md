---
title: Physics Mapping — Five-Force Pipeline for the Insight Migration Loop
description: Concrete CUDA + Rust specification for wiring the three dormant semantic-force kernels and authoring two new migration-physics kernels (bridging attraction, orphan repulsion) that together make VisionClaw's KG-to-ontology migration visible as physics
date: 2026-04-18
status: spec
series: insight-migration-loop
part: 03
precedes: implementation PR (Phase A), ADR-048 implementation (Phase B)
related: docs/audits/2026-04-18-cuda-kernel-utilisation.md, docs/design/2026-04-18-unified-knowledge-pipeline.md, docs/physics-calibration-proposal.md
---

# Physics Mapping — Five-Force Pipeline

This document is a full engineering specification. A Rust engineer should be able
to open this file, cross-reference the kernel source at
`src/utils/semantic_forces.cu` and the actor at
`src/actors/gpu/semantic_forces_actor.rs`, and begin wiring without additional
clarification questions. All open questions are listed explicitly in Section 10.

---

## 1. Force Tier Table

| # | Force | Phase | Source data | Purpose | Default magnitude | Tunable per |
|---|-------|-------|-------------|---------|------------------|-------------|
| 1 | Physicality cluster | A | `node_physicality[i]` (u8: 0=None, 1=Virtual, 2=Physical, 3=Conceptual) | Groups abstract/virtual/physical/conceptual nodes into distinct spatial bands; nodes of different physicality repel | cluster_attraction = 0.40; inter_physicality_repulsion = 0.20; cluster_radius = 80.0 | `config/domains.yaml physics.physicality-cluster-strength`; per-node override: `physics-weight::` frontmatter |
| 2 | Role cluster | A | `node_role[i]` (u8: 0=None, 1=Process, 2=Agent, 3=Resource, 4=Concept) | Groups nodes by semantic role — process band, agent band, concept band, resource band; separate role repulsion keeps bands distinct | cluster_attraction = 0.30; inter_role_repulsion = 0.15; cluster_radius = 80.0 | `config/domains.yaml physics.role-cluster-strength`; per-node override |
| 3 | Maturity layout | A | `node_maturity[i]` (u8: 0=None, 1=emerging/draft, 2=mature/enriched, 3=declining/authoritative) | Drives authoritative nodes toward z=0 (the stable core), draft nodes outward to z = -stage_separation | level_attraction = 0.15; stage_separation = 150.0 | `config/domains.yaml physics.maturity-layout-strength`; no per-edge override (layout force only) |
| 4 | Bridging attraction | B | `bridge_edges[]` array: (source_node_id u32, target_node_id u32, bridge_kind u8, confidence f32) populated from Neo4j `BRIDGE_TO` edges per ADR-048 | Pulls each KGNode toward its candidate or promoted OntologyClass across the BRIDGE_TO edge; confidence-weighted so fully promoted nodes are more strongly attracted than candidates | bridge_strength_per_kind[4]: none=0.0, candidate=0.35, promoted=0.75, colocated=1.20 (revoked=0.0 always) | Per-edge override via `confidence` field; `config/domains.yaml physics.bridge-strength`; bridge_kind overrides per-kind table |
| 5 | Orphan repulsion | B | `bridge_outbound_counts[i]` (u32 per KGNode); zero = no ontology anchor | Pushes unanchored KGNodes into the orphan zone (configurable world-space direction vector), making them spatially distinct and visually prominent as migration candidates | orphan_strength = 0.25; orphan_zone_vector = (1.5, 0.0, 0.0) world units/tick² | `config/domains.yaml physics.orphan-strength`; orphan_zone_vector is a runtime param, not per-node |

All magnitudes are starting proposals at the 2,500-node test scale. See Section 6 for the rationale and Section 7 for the 10 k-node envelope.

---

## 2. Rust Type Additions

### 2.1 Enum types (new, in `src/actors/gpu/semantic_forces_actor.rs` or a companion `migration_physics.rs`)

```rust
/// Canonical physicality codes matching semantic_forces.cu node_physicality values.
/// u8 to fit in GPU per-node metadata byte.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicalityCode {
    None        = 0,   // Excluded from physicality force
    Virtual     = 1,   // VirtualEntity — software, digital artefact
    Physical    = 2,   // PhysicalEntity — robot, sensor, hardware
    Conceptual  = 3,   // ConceptualEntity — policy, strategy, abstract idea
}

impl PhysicalityCode {
    pub fn from_owl_tag(tag: &str) -> Self {
        match tag {
            "virtual" | "VirtualEntity"    => Self::Virtual,
            "physical" | "PhysicalEntity"  => Self::Physical,
            "conceptual" | "ConceptualEntity" => Self::Conceptual,
            _                              => Self::None,
        }
    }
}

/// Canonical role codes matching semantic_forces.cu node_role values.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleCode {
    None     = 0,   // Excluded from role force
    Process  = 1,   // Process — workflows, pipelines, events
    Agent    = 2,   // Agent — actors, agents, systems with agency
    Resource = 3,   // Resource — data, documents, tools
    Concept  = 4,   // Concept — abstract ideas, theories
}

impl RoleCode {
    pub fn from_owl_tag(tag: &str) -> Self {
        match tag {
            "process" | "Process"   => Self::Process,
            "agent"   | "Agent"     => Self::Agent,
            "resource"| "Resource"  => Self::Resource,
            "concept" | "Concept"   => Self::Concept,
            _                       => Self::None,
        }
    }
}

/// Canonical maturity levels matching semantic_forces.cu node_maturity values.
/// Maps the quality/status frontmatter field.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaturityLevel {
    None          = 0,   // Excluded from maturity force
    Emerging      = 1,   // draft / candidate — outward z
    Mature        = 2,   // enriched — z = 0
    Authoritative = 3,   // authoritative / stable — inward z
}

impl MaturityLevel {
    pub fn from_quality_tag(tag: &str) -> Self {
        match tag {
            "draft" | "candidate" | "emerging"          => Self::Emerging,
            "enriched" | "review"                       => Self::Mature,
            "authoritative" | "stable" | "authoritative"=> Self::Authoritative,
            _                                           => Self::None,
        }
    }
}

/// Bridge edge kind for bridging attraction force.
/// Matches bridge_kind in BridgeEdge and bridge_strength_per_kind[] indexing.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeKind {
    None      = 0,   // No active force
    Candidate = 1,   // KGNode is a promotion candidate for this OntologyClass
    Promoted  = 2,   // KGNode has been promoted to this OntologyClass
    Colocated = 3,   // KGNode and OntologyClass are the same concept (post-merge)
    Revoked   = 4,   // Previously promoted but reverted; zero force applied
}
```

### 2.2 `SemanticMetadataBuffers` — device-memory struct

This struct is the Rust-side owner of all GPU device buffers that the five new forces read from. It lives alongside `SemanticForcesActor`'s existing node/edge cache vectors.

```rust
/// Per-node semantic metadata uploaded once per ingestion cycle (or on hotpath
/// changes). Matches the three-byte GPU per-node metadata layout described in
/// docs/audits/2026-04-18-cuda-kernel-utilisation.md §Stage 1.
pub struct SemanticMetadataBuffers {
    // --- per-node (len = num_nodes) ---
    pub physicality_codes: Vec<i32>,  // PhysicalityCode as i32 (CUDA uses int*)
    pub role_codes:        Vec<i32>,  // RoleCode as i32
    pub maturity_levels:   Vec<i32>,  // MaturityLevel as i32

    // --- per-physicality-type centroid scratch (len = 4, index 0 unused) ---
    pub physicality_centroids: Vec<Float3>,  // [4] — written by calculate_physicality_centroids
    pub physicality_counts:    Vec<i32>,     // [4]

    // --- per-role-type centroid scratch (len = 5, index 0 unused) ---
    pub role_centroids: Vec<Float3>,  // [5] — written by calculate_role_centroids
    pub role_counts:    Vec<i32>,     // [5]

    // --- Phase B: bridge edges (len = num_bridge_edges) ---
    pub bridge_source_ids:  Vec<u32>,   // KGNode graph index
    pub bridge_target_ids:  Vec<u32>,   // OntologyClass graph index
    pub bridge_kinds:       Vec<u8>,    // BridgeKind discriminant
    pub bridge_confidences: Vec<f32>,   // 0.0–1.0 from Neo4j BRIDGE_TO.confidence

    // --- Phase B: per-node bridge outbound count (len = num_nodes) ---
    pub bridge_outbound_counts: Vec<u32>,  // 0 = orphan
}

impl SemanticMetadataBuffers {
    pub fn new(num_nodes: usize) -> Self {
        Self {
            physicality_codes: vec![0i32; num_nodes],
            role_codes:        vec![0i32; num_nodes],
            maturity_levels:   vec![0i32; num_nodes],
            physicality_centroids: vec![Float3 { x: 0.0, y: 0.0, z: 0.0 }; 4],
            physicality_counts:    vec![0i32; 4],
            role_centroids: vec![Float3 { x: 0.0, y: 0.0, z: 0.0 }; 5],
            role_counts:    vec![0i32; 5],
            // Phase B fields — empty until ADR-048 bridge edges exist
            bridge_source_ids:  Vec::new(),
            bridge_target_ids:  Vec::new(),
            bridge_kinds:       Vec::new(),
            bridge_confidences: Vec::new(),
            bridge_outbound_counts: vec![0u32; num_nodes],
        }
    }

    /// Zero the centroid accumulators before each tick.
    pub fn zero_centroids(&mut self) {
        for c in &mut self.physicality_centroids { *c = Float3 { x: 0.0, y: 0.0, z: 0.0 }; }
        for c in &mut self.physicality_counts    { *c = 0; }
        for c in &mut self.role_centroids        { *c = Float3 { x: 0.0, y: 0.0, z: 0.0 }; }
        for c in &mut self.role_counts           { *c = 0; }
    }
}
```

### 2.3 `MigrationPhysicsParams` — per-tick parameter struct

```rust
/// Per-tick tunable parameters for the five migration forces.
/// Populated from config/domains.yaml at startup; hot-reloadable via
/// the existing DynamicRelationshipBuffer reload path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPhysicsParams {
    // --- Phase A ---
    pub physicality_strength:      f32,  // cluster_attraction multiplier; default 0.40
    pub physicality_cluster_radius: f32, // default 80.0
    pub physicality_inter_repulsion: f32,// default 0.20
    pub role_strength:             f32,  // cluster_attraction multiplier; default 0.30
    pub role_cluster_radius:       f32,  // default 80.0
    pub role_inter_repulsion:      f32,  // default 0.15
    pub maturity_vertical_spacing: f32,  // stage_separation in world units; default 150.0
    pub maturity_attraction:       f32,  // level_attraction; default 0.15
    // --- Phase B ---
    pub bridge_strength_candidate: f32,  // per BridgeKind::Candidate; default 0.35
    pub bridge_strength_promoted:  f32,  // per BridgeKind::Promoted; default 0.75
    pub bridge_strength_colocated: f32,  // per BridgeKind::Colocated; default 1.20
    pub orphan_strength:           f32,  // per-tick acceleration into orphan zone; default 0.25
    pub orphan_zone_direction:     [f32; 3], // world-space unit vector; default [1.0, 0.0, 0.0]
}

impl Default for MigrationPhysicsParams {
    fn default() -> Self {
        Self {
            physicality_strength:       0.40,
            physicality_cluster_radius: 80.0,
            physicality_inter_repulsion: 0.20,
            role_strength:              0.30,
            role_cluster_radius:        80.0,
            role_inter_repulsion:       0.15,
            maturity_vertical_spacing:  150.0,
            maturity_attraction:        0.15,
            bridge_strength_candidate:  0.35,
            bridge_strength_promoted:   0.75,
            bridge_strength_colocated:  1.20,
            orphan_strength:            0.25,
            orphan_zone_direction:      [1.0, 0.0, 0.0],
        }
    }
}
```

`MigrationPhysicsParams` is passed by value to the actor's tick method and is not a GPU constant-memory struct. The existing `SemanticConfigGPU` already carries the physicality/role/maturity sub-configs; the `config_to_gpu()` method in `semantic_forces_actor.rs` must be updated to read from `MigrationPhysicsParams` rather than the hardcoded defaults currently present (lines 563–594 of the actor).

---

## 3. Dispatch Order for One Tick

The following sequence replaces the comment block in `ForceComputeActor` (or wherever the semantic force tick loop lives). Steps 4–5 and 11 are existing; all others are new or newly enabled. The order is non-negotiable: centroid calculation must precede the forces that consume centroids.

```
1.  meta.zero_centroids()
    forces_buffer.fill(Float3::ZERO)          // zero force accumulator

2.  KERNEL: calculate_physicality_centroids(
        node_physicality, positions,
        physicality_centroids, physicality_counts, num_nodes
    )
    KERNEL: finalize_physicality_centroids(
        physicality_centroids, physicality_counts   // grid: 4 threads
    )

3.  KERNEL: calculate_role_centroids(
        node_role, positions,
        role_centroids, role_counts, num_nodes
    )
    KERNEL: finalize_role_centroids(
        role_centroids, role_counts                 // grid: 5 threads
    )

4.  KERNEL: force_pass_kernel(
        positions, velocities, edges, edge_weights,
        repel_k, spring_k, center_gravity_k, num_nodes, num_edges
    )                                               // existing spring/repulsion/gravity

5.  KERNEL: apply_subclass_hierarchy_kernel(...)    // existing
    KERNEL: apply_disjoint_classes_kernel(...)      // existing
    KERNEL: apply_sameas_colocate_kernel(...)        // existing

6.  KERNEL: apply_physicality_cluster_force(
        node_physicality, physicality_centroids,
        positions, forces, num_nodes
    )                                               // Phase A — DORMANT, now wired

7.  KERNEL: apply_role_cluster_force(
        node_role, role_centroids,
        positions, forces, num_nodes
    )                                               // Phase A — DORMANT, now wired

8.  KERNEL: apply_maturity_layout_force(
        node_maturity,
        positions, forces, num_nodes
    )                                               // Phase A — DORMANT, now wired

9.  KERNEL: apply_bridging_attraction_kernel(
        bridge_edges, num_bridge_edges,
        positions, forces,
        bridge_strength_per_kind[4]
    )                                               // Phase B — NEW, authored post-ADR-048

10. KERNEL: apply_orphan_repulsion_kernel(
        bridge_outbound_counts,
        positions, forces, num_nodes,
        orphan_strength, orphan_zone_vector
    )                                               // Phase B — NEW

11. KERNEL: integrate_pass_kernel(
        positions, velocities, forces,
        damping, dt, num_nodes
    )                                               // existing Verlet/Euler integrator
```

All CUDA kernel launches within a single tick are issued to the same stream (the existing physics stream). No explicit `cudaDeviceSynchronize()` is needed between steps 2 and 3 because they write to different device buffers. A single `cudaStreamSynchronize()` before step 11 ensures all force accumulations are complete before integration.

Centroid finalization (steps 2b, 3b) uses a grid of 4 and 5 threads respectively — these are instantaneous kernel launches; their overhead is negligible.

---

## 4. Two New Kernel Specifications

### 4.1 `apply_bridging_attraction_kernel`

**CUDA signature:**

```c
__global__ void apply_bridging_attraction_kernel(
    const unsigned int* bridge_source_ids,   // KGNode graph index for each bridge edge
    const unsigned int* bridge_target_ids,   // OntologyClass graph index for each bridge edge
    const unsigned char* bridge_kinds,       // BridgeKind discriminant (0–4)
    const float* bridge_confidences,         // 0.0–1.0 confidence for each edge
    const float3* positions,                 // all node positions [num_nodes]
    float3* forces,                          // force accumulator [num_nodes]
    const float bridge_strength_per_kind[4], // indexed by BridgeKind (none=0 unused, candidate=1, promoted=2, colocated=3)
    const int num_bridge_edges
);
```

**Semantics:**

- Grid: `ceil(num_bridge_edges / BLOCK_SIZE)` blocks, `BLOCK_SIZE = 128` threads
- Thread `idx` handles bridge edge `idx`
- If `idx >= num_bridge_edges`: return immediately
- `kind = bridge_kinds[idx]`
- If `kind == 0 || kind == 4` (None or Revoked): return immediately — zero force
- `strength_base = bridge_strength_per_kind[kind]`  (kind in 1–3)
- `confidence = bridge_confidences[idx]`
- `effective_strength = strength_base * confidence`
- `src = bridge_source_ids[idx]`, `tgt = bridge_target_ids[idx]`
- `delta = positions[tgt] - positions[src]`  (direction: source toward target)
- `dist = length(delta)` — if `< 1e-6f`: return (coincident nodes need no force)
- `force_vec = normalize(delta) * effective_strength * dist`  (pure attraction, no rest length; scale proportional to distance so distant nodes feel stronger pull)
- `atomicAdd(&forces[src].{x,y,z}, force_vec.{x,y,z})`  — only source node is pulled; target (OntologyClass) position is determined by its own ontology hierarchy forces and should not be disturbed by KG-side candidates

**Why no rest length:** A bridged KGNode should merge toward its anchor as the bridge becomes more authoritative. A Hooke spring with a rest length > 0 would repel at close range; pure `strength * dist` attraction provides a true potential well at the anchor position.

**Pseudocode:**

```
FOR EACH bridge edge idx IN PARALLEL:
    IF idx >= num_bridge_edges: RETURN
    kind = bridge_kinds[idx]
    IF kind IN {0, 4}: RETURN
    src = bridge_source_ids[idx]
    tgt = bridge_target_ids[idx]
    delta = positions[tgt] - positions[src]
    dist = |delta|
    IF dist < 1e-6: RETURN
    effective_strength = bridge_strength_per_kind[kind] * bridge_confidences[idx]
    force = normalize(delta) * effective_strength * dist
    atomicAdd(forces[src], force)
```

**Atomic contention analysis:** Each thread writes to a unique `src` node. In graphs where many KGNodes bridge to the same OntologyClass, multiple threads will write to the same `forces[src]` only if there are multi-hop bridges from the same source node — unlikely in practice. Contention is low. `atomicAdd` on `float3` requires three separate `atomicAdd(float*)` calls; no warp-level reduction is needed for correctness.

### 4.2 `apply_orphan_repulsion_kernel`

**CUDA signature:**

```c
__global__ void apply_orphan_repulsion_kernel(
    const unsigned int* bridge_outbound_counts,  // per-node count of BRIDGE_TO edges [num_nodes]
    const float3* positions,                     // node positions [num_nodes] (unused — force is constant-direction)
    float3* forces,                              // force accumulator [num_nodes]
    const int num_nodes,
    const float orphan_strength,                 // scalar magnitude per tick
    const float3 orphan_zone_vector              // world-space direction (need not be unit; magnitude encodes zone distance)
);
```

**Semantics:**

- Grid: `ceil(num_nodes / BLOCK_SIZE)` blocks, `BLOCK_SIZE = 128` threads
- Thread `idx` handles node `idx`
- If `idx >= num_nodes`: return
- If `bridge_outbound_counts[idx] > 0`: return — node has at least one ontology anchor, skip
- `force_vec = orphan_zone_vector * orphan_strength`
- `atomicAdd(&forces[idx].{x,y,z}, force_vec.{x,y,z})`

**Design note:** The orphan force is constant-direction regardless of current position. This is intentional: it is a directional bias, not an attractor. The node's own repulsion from other nodes and the base spring forces prevent it from flying to infinity; the net result is that orphans drift into a spatial region that is distinct from the anchored cluster. If position-dependent behaviour is desired in a future revision, replace `orphan_zone_vector` with an attractor-point and compute `normalize(attractor - positions[idx]) * orphan_strength * dist`.

**Pseudocode:**

```
FOR EACH node idx IN PARALLEL:
    IF idx >= num_nodes: RETURN
    IF bridge_outbound_counts[idx] > 0: RETURN
    force = orphan_zone_vector * orphan_strength
    atomicAdd(forces[idx], force)
```

**Atomic contention:** Zero — each thread writes to a unique `forces[idx]`. No contention.

---

## 5. Visual Semantics

When all five forces are active, the physics produces a layered, readable layout that encodes the migration state of every node. On the horizontal plane, physicality clustering separates abstract (Conceptual) nodes from Virtual and Physical entities into three loose neighborhoods, each with a visible spatial boundary where inter-physicality repulsion keeps them distinct rather than interleaved. Superimposed on that partition, role clustering forms curved bands — a process band, an agent band, a concept/resource band — whose gentle attraction keeps semantically similar roles proximate without overriding the physicality separation. The z-axis becomes a quality gradient: authoritative nodes sit in the stable centre plane (z ≈ 0), enriched nodes at a middle band, and draft or candidate nodes pushed outward toward the poles; the effect is that the ontology core appears visually heavier and denser than the periphery of emerging work. Across this landscape, BRIDGE_TO edges act as tendons: KGNodes that have been proposed as candidates for an OntologyClass are pulled measurably toward that class's position, the pull strengthening as confidence and bridge kind rise from candidate through promoted to colocated, so a user can literally watch a node migrate across the screen as a promotion is approved. Finally, KGNodes with no ontology anchor at all drift in the direction of the orphan zone vector — a dedicated region of world space, rendered in an accent colour (suggested: amber at 60% opacity) that visually declares "these nodes are migration candidates and nobody has claimed them yet." The combined effect is that the physics view becomes a live dashboard of ontology health: the core is ordered, the bridges are taut, the orphan cloud identifies exactly where the next enrichment effort should go.

---

## 6. Tuning Profile for a 2,500-Node Test Graph

The physics calibration proposal (`docs/physics-calibration-proposal.md`) established that the base system requires `damping = 0.9`, `repelK = 80.0`, `springK = 10.0`, `scalingRatio = 2.0`, `centerGravityK = 0.5`. The five migration forces layer on top of this baseline. The primary conflict is between the physicality/role cluster forces and the base repulsion: both push nodes apart, and at insufficient damping they will produce oscillation rather than convergence.

**Recommended starting values for 2,500-node test graph:**

```yaml
physics:
  # Base calibration (from physics-calibration-proposal.md — do not change)
  damping: 0.9
  repelK: 80.0
  springK: 10.0
  scalingRatio: 2.0
  centerGravityK: 0.5

  # Phase A — migration forces (proposed; tune against actual corpus distribution)
  physicality-cluster-strength: 0.40
  physicality-cluster-radius: 80.0
  physicality-inter-repulsion: 0.20

  role-cluster-strength: 0.30
  role-cluster-radius: 80.0
  role-inter-repulsion: 0.15

  maturity-layout-strength: 0.15
  maturity-stage-separation: 150.0

  # Phase B — migration forces
  bridge-strength-candidate: 0.35
  bridge-strength-promoted: 0.75
  bridge-strength-colocated: 1.20
  orphan-strength: 0.25
  orphan-zone-direction: [1.5, 0.0, 0.0]
```

**Conflict analysis:** The physicality inter-repulsion (0.20) adds to the base repulsion between heterogeneous-physicality pairs. At `repelK = 80.0` the base repulsion already dominates at short range; 0.20 additional repulsion is about 0.25% of the base and will not cause oscillation. The role cluster attraction at 0.30 is weaker than the base spring constant of 10.0, so it acts as a soft bias rather than a stiff constraint. The maturity layout at 0.15 is deliberately weak; it is a layout preference, not a hard constraint, and should never override the subclass hierarchy forces already provided by `apply_subclass_hierarchy_kernel`.

**Damping requirement for Phase A:** `damping = 0.9` as calibrated is sufficient. The three dormant forces add forces in the 0.15–0.40 range, well below the magnitude that caused oscillation with the old `damping = 0.6` baseline. No additional damping increase is needed for Phase A.

**Damping requirement for Phase B:** The bridging attraction at `bridge_strength_promoted = 0.75` creates a net force that is 7.5% of the base repulsion. If the graph has a high fraction of promoted nodes (>50%), the cumulative bridging pull could create slow drift loops at the default damping. Monitor mean |v| during Phase B testing; if it does not reach < 1.0 units/tick within 20 seconds on the test graph, increase damping to 0.92.

---

## 7. Performance Envelope for 10,000 Nodes

**Additional GPU memory required per tick:**

| Buffer | Size | Bytes |
|--------|------|-------|
| `node_physicality` (i32 × 10k) | 40 KB | 40,960 |
| `node_role` (i32 × 10k) | 40 KB | 40,960 |
| `node_maturity` (i32 × 10k) | 40 KB | 40,960 |
| `physicality_centroids` (float3 × 4) | 48 B | 48 |
| `physicality_counts` (i32 × 4) | 16 B | 16 |
| `role_centroids` (float3 × 5) | 60 B | 60 |
| `role_counts` (i32 × 5) | 20 B | 20 |
| Phase B: `bridge_source_ids` (u32 × ~5k edges est.) | 20 KB | 20,480 |
| Phase B: `bridge_target_ids` (u32 × ~5k) | 20 KB | 20,480 |
| Phase B: `bridge_kinds` (u8 × ~5k) | 5 KB | 5,120 |
| Phase B: `bridge_confidences` (f32 × ~5k) | 20 KB | 20,480 |
| Phase B: `bridge_outbound_counts` (u32 × 10k) | 40 KB | 40,960 |

**Phase A total:** ~122 KB. **Phase A + B total:** ~230 KB. Both are negligible against the ~30 MB already consumed by positions, velocities, and edge adjacency at 10 k nodes.

**Additional kernel launches per tick (7 new):**

| Kernel | Grid (10k nodes / 128) | Estimated time |
|--------|------------------------|----------------|
| `calculate_physicality_centroids` | 79 blocks | ~10 µs |
| `finalize_physicality_centroids` | 1 block (4 threads) | < 1 µs |
| `calculate_role_centroids` | 79 blocks | ~10 µs |
| `finalize_role_centroids` | 1 block (5 threads) | < 1 µs |
| `apply_physicality_cluster_force` | 79 blocks | ~15 µs |
| `apply_role_cluster_force` | 79 blocks | ~15 µs |
| `apply_maturity_layout_force` | 79 blocks | ~8 µs |
| Phase B: `apply_bridging_attraction_kernel` | ~40 blocks (5k edges) | ~12 µs |
| Phase B: `apply_orphan_repulsion_kernel` | 79 blocks | ~8 µs |

**Estimated total added time per tick:** ~79 µs for Phase A; ~99 µs for Phase A + B. The existing force pass + integration at 10 k nodes runs in the 300–600 µs range; the migration forces add approximately 15–25% to the per-tick compute budget. At a target 30 Hz physics tick rate (33 ms per tick), this addition is invisible to the user.

**Atomic contention risks:**

- `calculate_physicality_centroids` and `calculate_role_centroids` both perform `atomicAdd` into 4 and 5 slots respectively. At 10 k nodes, ~2,500 threads will contend on each physicality slot on average. This is a known pattern (scatter to a small array); modern GPU hardware serialises atomics to the same cache line but the total wall-clock cost is bounded by the atomic throughput of the SM, not by the number of collisions. Empirically this pattern costs ~10–15 µs at 10 k nodes — measured against the existing `calculate_type_centroids` which uses the identical idiom.
- `apply_physicality_cluster_force` and `apply_role_cluster_force` both contain an O(n²) inner loop (lines 600–617 and 654–671 in `semantic_forces.cu`) that iterates over all nodes to compute inter-cluster repulsion. At 10 k nodes this is 10^8 operations per kernel — approximately 200 ms on an RTX 3080, which is unacceptable at 30 Hz. **This is the single serious performance risk.** The fix, deferred to the Phase A follow-on PR, is to replace the O(n²) inner loop with a lookup against the precomputed centroids (inter-cluster repulsion directed away from the foreign centroid, not away from each foreign node individually). The centroid-based approximation is already used in `apply_type_cluster_force` in the same file; apply the same pattern to physicality and role kernels before enabling them at scale. For the initial 2,500-node test graph (6.25×10^6 operations per kernel), the O(n²) loop is acceptable and will run in ~12 ms — barely acceptable at 30 Hz. Wire at 2,500 nodes; fix before 5,000.

---

## 8. Testing Strategy

### Force 1: Physicality cluster (`apply_physicality_cluster_force`)

- **Unit test (Rust, 4-node fixture):** Two nodes physicality=Virtual at positions (0,0,0) and (200,0,0); two nodes physicality=Physical at (0,100,0) and (200,100,0). Centroid for Virtual should be (100,0,0); centroid for Physical should be (100,100,0). After one force pass, both Virtual nodes should move toward (100,0,0) and both Physical nodes toward (100,100,0). Assert `forces[0].y == 0.0` (no vertical component from horizontal clustering). Mirrors `tests/gpu_semantic_forces_test.rs` fixture pattern.
- **GPU smoke test:** Extend `tests/gpu_semantic_forces_test.rs`: construct a 16-node graph with 4 nodes per physicality type, assert centroid positions after `finalize_physicality_centroids`, assert force sign and magnitude direction.
- **Integration test (20-node corpus):** Load the 20-node ontology fixture from `tests/fixtures/minimal_kg.json`, set physicality codes from the `owl:physicality::` tags, run 100 physics ticks, assert that the mean pairwise distance between same-physicality nodes is less than the mean pairwise distance between different-physicality nodes.
- **Regression harness:** Add a `physics_migration_forces_regression.rs` test (parallel to `tests/settings_physics_propagation_regression.rs`) that records centroid positions and force magnitudes at tick 1, 10, 50. Golden file stored in `tests/fixtures/migration_physics_regression.json`. CI fails if any recorded value drifts by more than 1% across builds.

### Force 2: Role cluster (`apply_role_cluster_force`)

- **Unit test:** 4 nodes: two Process (role=1) at (0,0,0) and (100,0,0); two Agent (role=2) at (0,200,0) and (100,200,0). Assert after one pass that Process nodes have forces directed toward (50,0,0) and Agent nodes toward (50,200,0). Assert that inter-role repulsion produces a force component pointing away from the foreign centroid.
- **GPU smoke test:** 20 nodes, 5 per role type, assert centroid accuracy and force direction.
- **Integration and regression:** Same pattern as physicality; include in `physics_migration_forces_regression.rs`.

### Force 3: Maturity layout (`apply_maturity_layout_force`)

- **Unit test:** 3 nodes: maturity=Emerging at z=0, maturity=Mature at z=100, maturity=Authoritative at z=300. Target z for Emerging is `-stage_separation = -150`; for Mature is 0; for Authoritative is `+stage_separation = 150`. Assert force direction and non-zero magnitude for each.
- **GPU smoke test:** Assert that after 50 ticks starting from random z positions, the three maturity groups have mean z in the correct ordering (Authoritative < Mature < Emerging).
- **Regression:** Reference `tests/physics_orchestrator_settle_regression.rs` — add a maturity-layout settlement variant that asserts convergence within 30 ticks for a 20-node graph.

### Force 4: Bridging attraction (`apply_bridging_attraction_kernel`) — Phase B

- **Unit test:** 2 nodes: KGNode at (500,0,0), OntologyClass at (0,0,0). One BRIDGE_TO edge with kind=Promoted (2), confidence=0.8. Expected force on KGNode: direction toward (0,0,0), magnitude = `0.75 * 0.8 * 500 = 300.0`. Assert `forces[0].x ≈ -300.0`, `forces[0].y == 0.0`, `forces[0].z == 0.0`. Assert `forces[1]` unchanged (OntologyClass not pulled).
- **GPU smoke test:** 10 KGNodes each bridged at varying confidence to one OntologyClass; assert that higher-confidence nodes have proportionally stronger forces.
- **Revoked edge test:** Set kind=Revoked (4) on an edge; assert zero force is applied.
- **Integration and regression:** After ADR-048 bridge edges are populated in Neo4j, run against the 20-node corpus with synthetic BRIDGE_TO edges and assert visible position convergence.

### Force 5: Orphan repulsion (`apply_orphan_repulsion_kernel`) — Phase B

- **Unit test:** 4 nodes: nodes 0–2 have `bridge_outbound_counts > 0`, node 3 has `bridge_outbound_counts == 0`. `orphan_zone_vector = (1.0, 0.0, 0.0)`, `orphan_strength = 0.5`. Assert `forces[3].x ≈ 0.5`, `forces[3].y == 0.0`, `forces[3].z == 0.0`. Assert `forces[0..2]` unchanged.
- **GPU smoke test:** 100 nodes, 20 orphans; assert that after 100 ticks the orphans have mean x position at least 200 world units greater than the non-orphan mean x position.
- **Regression:** Add to `physics_migration_forces_regression.rs`.

---

## 9. Phased Activation

### Phase A — Activate three dormant kernels (this sprint)

**Scope:** Wire `apply_physicality_cluster_force`, `apply_role_cluster_force`, and `apply_maturity_layout_force` plus their centroid helpers into `SemanticForcesActor` and the tick dispatch loop.

**No corpus changes required.** The metadata fields (`physicality_code`, `role_code`, `maturity_level`) are already parsed from the `owl:physicality::`, `owl:role::`, and `quality::` frontmatter fields per the parser fix in commit `b501942b1`. The only work is:

1. Add `SemanticMetadataBuffers` (Section 2.2) to `SemanticForcesActor`.
2. Add `MigrationPhysicsParams` (Section 2.3) to actor state; populate from `config/domains.yaml`.
3. Update `config_to_gpu()` (actor lines 517–595) to copy `MigrationPhysicsParams` values into `PhysicalityClusterConfigGPU`, `RoleClusterConfigGPU`, and `MaturityLayoutConfigGPU` and set `enabled = true`.
4. Add the FFI declarations for the six new kernel entry points (three force kernels + three centroid helpers) to the `extern "C"` block in the actor (after line 333).
5. Wire the dispatch sequence from Section 3 into the tick loop.
6. Gate all three forces behind a `migration_physics_enabled: bool` flag in `MigrationPhysicsParams` defaulting to `false` so the feature can ship dark and be enabled via runtime config.

**Phase A is independently shippable.** It has zero dependency on Neo4j schema changes, ADR-048, or any bridge edge infrastructure.

### Phase B — Author and activate two new kernels (follow-on sprint, post-ADR-048)

**Scope:** Author `apply_bridging_attraction_kernel` and `apply_orphan_repulsion_kernel` in `src/utils/semantic_forces.cu`, add FFI bindings, populate `bridge_edges` from Neo4j BRIDGE_TO edges, wire dispatch.

**Requires:**
- ADR-048 BRIDGE_TO edge schema committed to Neo4j
- Neo4j ingestion pipeline writing `confidence`, `bridge_kind`, and `revoked_at` fields on BRIDGE_TO edges
- `SemanticMetadataBuffers.bridge_*` fields populated by the graph loader

**Phase B cannot ship without ADR-048.** Do not partially wire it; gate the entire Phase B block behind a compile-time feature flag `migration-bridge-physics` to prevent accidental activation.

---

## 10. Open Engineering Questions

**Q1 — O(n²) inner loop in physicality and role kernels.** The existing kernels iterate over all other nodes to compute inter-cluster repulsion (semantic_forces.cu lines 600–617, 654–671). At the 2,500-node test threshold this is marginal; at 5,000 nodes it will break the 30 Hz budget. Before Phase A reaches production, the inner loop must be replaced with a centroid-based approximation: repulsion is directed away from the foreign cluster centroid, not away from each individual foreign node. Decision needed: accept O(n²) for the initial 2,500-node Phase A rollout with a committed follow-on PR, or block Phase A on the centroid-based fix?

**Q2 — Centroid stability across tick boundaries.** The physicality and role centroids are recomputed every tick from the current positions. If a large cluster shifts rapidly (e.g., during initial layout convergence), the centroid can jump, causing nodes to receive a reversed force direction for one tick. This is usually harmless but can manifest as a one-tick "snap." The existing `apply_type_cluster_force` has the same behaviour. Should the centroid be exponentially smoothed across ticks (centroid_t = alpha * centroid_{t-1} + (1-alpha) * centroid_t)? This adds one float3 buffer per cluster type but eliminates snaps. Recommendation: yes, use alpha=0.9 — but this is an enhancement, not a blocker, and can be added in Phase A follow-on.

**Q3 — Bridge edge ownership and deduplication.** When an OntologyClass is promoted from a KGNode, both a BRIDGE_TO edge (kind=Promoted) and a SUBCLASS_OF edge may exist in Neo4j. The bridging attraction kernel must not double-count the pull. Clarification needed: does the ingestion pipeline guarantee that BRIDGE_TO and SUBCLASS_OF are mutually exclusive per node pair, or must the kernel skip BRIDGE_TO edges that also have a SUBCLASS_OF edge? This affects the bridge edge loader, not the kernel itself.

**Q4 — Orphan zone direction as a user-configurable vector.** The `orphan_zone_direction` vector determines where orphans drift spatially. The current proposal uses `[1.5, 0.0, 0.0]` (positive X axis), which places the orphan cloud to the right of the main graph in default camera orientation. If the user rotates the camera or uses a different default orientation, the orphan zone may not be intuitively "off to the side." Should `orphan_zone_direction` be expressed in a camera-relative coordinate system, or remain in world space with a user-facing UI control to set the orphan zone position? World space is simpler to implement; camera-relative requires passing the camera transform into the kernel or computing the direction on the Rust side per tick.

**Q5 — Phase B activation gate and ADR-048 coupling.** The bridge edge loader (which will populate `SemanticMetadataBuffers.bridge_*`) depends on Neo4j schema changes that are not yet committed. If ADR-048 slips, Phase B code that is compiled but not activated must not regress Phase A stability. The compile-time feature flag `migration-bridge-physics` proposed in Section 9 handles this, but the feature flag must be absent from the default `Cargo.toml` profile and must not be activated by `cargo test` unless explicitly set. Confirm the feature gating convention with the Rust lead before Phase B code is authored.
