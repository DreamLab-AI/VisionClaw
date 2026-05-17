# DDD Bounded Context: Feature Engineering Pipeline

**Context Map Position:** Downstream of BC-Ontology (OntologyClass nodes, SUBCLASS_OF edges), upstream of BC-Visualization (client rendering, discovery UX).

## Domain Overview

The Feature Engineering bounded context transforms raw ontology data (text properties, graph structure) into ML-ready numeric representations that power discovery, similarity search, and physics-informed layout.

## Ubiquitous Language

| Term | Definition |
|------|-----------|
| **Content Embedding** | 384-dim MiniLM-L6 vector encoding semantic meaning of a node's text fields |
| **Topology Embedding** | 128-dim TransE vector encoding structural role from graph connectivity |
| **Combined Score** | Weighted sum of content and topology similarity (default 0.6/0.4) |
| **Materialized Edge** | Transitive N-hop connection precomputed and stored as a weak-spring relationship |
| **Edge Type Physics** | Per-relationship-type force parameters (rest_length, strength, force_model) |
| **Discovery Gap** | Pair of nodes with high content similarity but no direct graph edge (potential missing relationship) |
| **KGE Triple** | (head, relation, tail) fact from the knowledge graph used for TransE training |
| **Negative Sample** | Corrupted triple (random head/tail replacement) used as contrastive training signal |

## Entities

### EmbeddingService (Service)
- Stateless transformer: text → 384-dim float vector
- Calls external MiniLM endpoint in batches of 64
- Stores results as `content_embedding_384` property on Neo4j OntologyClass nodes

### KGETrainer (Domain Service)
- Trains TransE model on (entity, relation, entity) triples extracted from Neo4j
- Produces 128-dim entity embeddings and relation embeddings
- Stores results as `kge_embedding_128` property on Neo4j nodes
- Training is idempotent: re-running overwrites previous embeddings

### NHopMaterializer (Domain Service)
- Computes transitive closure up to N hops for configured relationship types
- Creates `MATERIALIZED_2HOP` and `MATERIALIZED_3HOP` Neo4j relationships
- Configurable weight decay (2-hop: 0.05, 3-hop: 0.02)
- Deduplication: will not create duplicate materialized edges

### EdgeTypePhysicsConfig (Value Object)
- Maps SemanticEdgeType variants to GPU force parameters
- Immutable configuration uploaded to GPU constant memory
- Per-type: rest_length, spring_strength, force_type, directionality

## Value Objects

```
ContentEmbedding(Vec<f32>)  // len == 384, L2-normalized
TopologyEmbedding(Vec<f32>) // len == 128, L2-normalized  
CombinedScore(f32)          // 0.0 - 1.0, weighted sum
MaterializationStats { two_hop: usize, three_hop: usize, duration_ms: u64 }
TrainingStats { entities: usize, relations: usize, triples: usize, loss: f32 }
SimilarityResult { iri: String, label: String, score: f32 }
```

## Aggregates

### FeaturePipeline (Aggregate Root)
Orchestrates the full pipeline:
1. EmbeddingService indexes all ontology nodes (content features)
2. KGETrainer trains on full graph (topology features)
3. NHopMaterializer creates transitive edges
4. EdgeTypePhysicsConfig updates GPU force buffer

Invariants:
- Content embeddings must be 384-dim
- Topology embeddings must be 128-dim
- Materialized edges must not duplicate existing direct edges
- Combined score weights must sum to 1.0

## Domain Events

| Event | Trigger | Consumers |
|-------|---------|-----------|
| `ContentEmbeddingsIndexed { count, duration_ms }` | After batch embedding completes | Discovery endpoint cache invalidation |
| `TopologyEmbeddingsTrained { entities, loss }` | After KGE training completes | GPU physics actor (uploads new vectors) |
| `EdgesMateriazed { two_hop, three_hop }` | After N-hop materialization | GraphStateActor (reload edges) |
| `PhysicsConfigUpdated { types_count }` | After edge type config change | SemanticForcesActor (re-upload buffer) |

## Context Map (Anti-Corruption Layers)

```
┌─────────────────────────┐
│  BC: Ontology Ingest    │
│  (OntologyParser,       │
│   Neo4jOntologyRepo)    │
└────────────┬────────────┘
             │ OntologyClass nodes + SUBCLASS_OF/RELATES edges
             ▼
┌─────────────────────────┐
│  BC: Feature Eng.       │◄── THIS CONTEXT
│  (EmbeddingService,     │
│   KGETrainer,           │
│   NHopMaterializer,     │
│   EdgeTypePhysics)      │
└──────┬──────────┬───────┘
       │          │
       │          │ Materialized edges + embedding vectors
       │          ▼
       │   ┌─────────────────────────┐
       │   │  BC: GPU Physics        │
       │   │  (ForceComputeActor,    │
       │   │   SemanticForcesActor)  │
       │   └─────────────────────────┘
       │
       │ Combined similarity scores
       ▼
┌─────────────────────────┐
│  BC: Client Discovery   │
│  (DiscoveryHandler,     │
│   REST API)             │
└─────────────────────────┘
```

## Integration Points

### Upstream: Ontology Ingest
- Reads: OntologyClass nodes (iri, preferred_term, definition, scope_note)
- Reads: SUBCLASS_OF, RELATES edges (for KGE training triples)
- Contract: Nodes must have `iri` as unique key

### Downstream: GPU Physics
- Writes: EdgeTypePhysicsConfig → DynamicForceConfigGPU buffer
- Writes: Materialized edges loaded alongside direct edges
- Contract: Edge type u8 discriminant matches SemanticEdgeType enum

### Downstream: Client Discovery
- Exposes: REST API at `/api/discovery/search`, `/api/discovery/related`, `/api/discovery/gaps`
- Contract: JSON response with scored results sorted by combined similarity

## Quality Constraints

- Embedding batch indexing must complete within 5 minutes for 10k nodes
- KGE training must converge (loss < 0.5) within 500 epochs for 50k triples
- N-hop materialization must not create >10x the original edge count
- Discovery search must respond within 200ms for <10k candidate nodes
- Per-edge-type physics differentiation must not increase GPU frame time by >0.5ms
