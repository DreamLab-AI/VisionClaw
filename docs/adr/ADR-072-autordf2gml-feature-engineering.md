# ADR-072: AutoRDF2GML-Inspired Feature Engineering Pipeline

**Status:** Accepted
**Date:** 2026-05-05
**Author:** Architecture Agent (SPARC)
**Related:** ADR-048 (ontology edge weights), ADR-070 (CUDA integration), semantic_forces.cu, SemanticTypeRegistry

## Context

VisionClaw's ontology pipeline extracts OWL classes with rich metadata (definition, scope_note, preferred_term, quality_score, authority_score) and connects them via typed relationships (SUBCLASS_OF, RELATES with subtypes). The GPU physics simulation applies force-directed layout with ontology-specific constraints. However:

1. **No content-based node features**: The semantic_processor_actor uses a 256-dim hash-based bag-of-characters "embedding" — effectively random noise with no semantic meaning.
2. **No structural embeddings**: Node positions encode topology implicitly but there's no fixed-dimension representation of structural role for ML tasks.
3. **No transitive connectivity**: Only 1-hop edges are loaded. Nodes connected via 2-3 hops have no spring attraction, causing hierarchy fragments to scatter.
4. **Uniform spring physics**: Although `DynamicRelationshipBuffer` and `SemanticEdgeType` infrastructure exists, edges are effectively treated uniformly in the main spring kernel.
5. **No combined similarity search**: Users cannot query "find nodes similar to X" using both content and structure.

AutoRDF2GML (ISWC 2024, Farber/Lamprecht/Susanti) demonstrates that combining content-based features (text embeddings of RDF literals) with topology-based features (KGE embeddings from graph structure) via weighted addition achieves F1 0.940 on large-scale academic knowledge graphs — significantly outperforming either alone.

## Decision

Implement a five-component feature engineering pipeline inspired by AutoRDF2GML's dual-mode architecture, adapted for VisionClaw's Rust/CUDA substrate:

### Component 1: Sentence Embeddings (Content Features)

**Module**: `src/services/embedding_service.rs`

Uses MiniLM-L6-v2 (already deployed for AgentDB at ruvector-postgres) to produce 384-dim vectors from ontology node text fields. Stored as `content_embedding_384` on Neo4j nodes.

Text construction: `"{preferred_term}: {definition}. {scope_note}"`

Batch processing in groups of 64 via HTTP POST to the embedding service endpoint.

### Component 2: N-Hop Edge Materialization

**Module**: `src/services/nhop_materializer.rs`

Precomputes transitive 2-hop and 3-hop connections as Neo4j relationships (`MATERIALIZED_2HOP`, `MATERIALIZED_3HOP`) with configurable weights (default: 0.05, 0.02). These are loaded alongside direct edges and fed to GPU as very weak springs.

Supports cross-type traversal (SUBCLASS_OF → RELATES → SUBCLASS_OF) and caps materialized edges per source node to prevent quadratic blowup.

### Component 3: KGE Training (Topology Features)

**Module**: `src/services/kge_trainer.rs`

Pure-Rust TransE implementation. Trains 128-dim entity embeddings on the full edge set (all relationship types). Training is CPU-only (rayon-parallelized negative sampling), runs as a scheduled batch job post-sync.

Parameters: 500 epochs, batch size 1024, margin 1.0, 5 negative samples per positive.

Stored as `kge_embedding_128` on Neo4j nodes.

### Component 4: Combined Discovery Endpoint

**Module**: `src/handlers/discovery_handler.rs`

REST API: `GET /api/discovery/search?q=...&content_weight=0.6&topology_weight=0.4`

Combines content similarity (cosine distance in 384-dim space) with topology similarity (cosine distance in 128-dim KGE space) via weighted addition. Also exposes gap detection (high content similarity but no edge between nodes).

### Component 5: Per-Edge-Type Physics Differentiation

**Module**: `src/services/edge_type_physics.rs`

Extends `SemanticEdgeType` with `Materialized2Hop` and `Materialized3Hop` variants. Provides per-type `EdgeTypeForceParams` (rest_length, spring_strength, force_type, directionality) that maps directly to the existing `DynamicForceConfigGPU` buffer uploaded to the semantic_forces.cu kernel.

Edge type differentiation:
| Type | Rest Length | Strength | Force Model |
|------|-------------|----------|-------------|
| Hierarchical | 60 | 0.9 | Directional spring |
| Structural | 80 | 0.7 | Orbit clustering |
| Dependency | 90 | 0.6 | Directional spring |
| ExplicitLink | 100 | 0.5 | Standard spring |
| Associative | 120 | 0.3 | Standard spring |
| Namespace | 150 | 0.2 | Standard spring |
| Bridge | 200 | 0.15 | Cross-domain |
| Materialized2Hop | 250 | 0.08 | Standard spring |
| Materialized3Hop | 350 | 0.03 | Standard spring |

## Consequences

### Positive

1. **ML-ready node features**: 384-dim content + 128-dim topology embeddings enable downstream tasks (link prediction, node classification, anomaly detection) without manual feature engineering.
2. **Improved layout coherence**: N-hop materialization prevents hierarchy fragments from scattering; per-edge physics makes relationship types visually distinguishable.
3. **Discovery UX**: Combined similarity search surfaces both "means similar things" (content) and "occupies similar structural role" (topology) — strictly better than either alone per AutoRDF2GML benchmarks.
4. **Gap detection**: Ontology gaps (missing edges between semantically similar nodes) become automatically discoverable.
5. **Incremental adoption**: Each component is independently valuable and deployable.

### Negative

1. **Storage overhead**: Two embedding vectors per node (~2KB) doubles per-node storage in Neo4j. Acceptable for <100k nodes.
2. **Training latency**: KGE training takes minutes for large graphs. Must run as background batch job, not in request path.
3. **Embedding staleness**: Content/topology embeddings become stale after graph mutations. Requires re-embedding on sync.

### Neutral

1. **No GPU for training**: TransE training is CPU-only. GPU acceleration would help for >1M triples but is unnecessary at current scale.
2. **Existing DynamicRelationshipBuffer unchanged**: Per-edge-type physics reuses the existing GPU constant memory infrastructure — no CUDA kernel changes required, only different parameter values.

## Alternatives Considered

### A1: Full AutoRDF2GML Python pipeline as sidecar
Rejected. Adds Python dependency, inter-process serialization overhead, and doesn't leverage existing Neo4j/Rust infrastructure.

### A2: GPU-accelerated KGE training via cudarc
Deferred. Current graph size (<50k triples) doesn't justify GPU training complexity. CPU training completes in <60s. Revisit when graph exceeds 500k triples.

### A3: Use existing force-directed positions as topology embedding
Considered. Positions are 3D (too low dimensional for ML) and change with physics parameters. Fixed 128-dim KGE embeddings are parameter-independent and ML-compatible.

## References

- Lamprecht, D., Susanti, Y., Farber, M. (2024). "AutoRDF2GML: Facilitating RDF Integration in Graph Machine Learning." ISWC 2024.
- Bordes, A. et al. (2013). "Translating Embeddings for Modeling Multi-relational Data." NeurIPS.
- VisionClaw `src/utils/semantic_forces.cu` — DynamicRelationshipBuffer infrastructure
- VisionClaw `src/services/semantic_type_registry.rs` — Runtime type registration
