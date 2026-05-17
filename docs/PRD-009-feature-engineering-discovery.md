# PRD-009: AutoRDF2GML-Inspired Feature Engineering & Discovery

**Status:** Implemented (endpoints verified 2026-05-05)
**Priority:** P1
**Author:** Architecture Agent
**Date:** 2026-05-05
**ADR:** ADR-072
**DDD Context:** ddd-feature-engineering-context.md

## Problem Statement

VisionClaw ingests rich ontology data (OWL classes with definitions, relationships, metadata) but offers no ML-powered discovery. Users can only navigate the graph visually or search by exact label match. There is no way to:
- Find semantically similar concepts across domains
- Discover missing relationships (ontology gaps)
- Leverage graph structure for recommendations
- Differentiate relationship types visually in the physics layout

## Solution Overview

Implement a five-component feature engineering pipeline inspired by AutoRDF2GML (ISWC 2024), adapted for VisionClaw's Rust/CUDA architecture:

1. **Sentence Embeddings** — MiniLM-L6 vectors from node text
2. **N-Hop Edge Materialization** — Transitive weak springs
3. **KGE Training** — TransE structural embeddings
4. **Combined Discovery API** — Content + topology similarity search
5. **Per-Edge-Type Physics** — Differentiated spring parameters

## User Stories

### US-1: Semantic Similarity Search
> As a knowledge engineer, I want to search for concepts by meaning (not just label) so I can find related terms across different naming conventions.

**Acceptance criteria:**
- `GET /api/discovery/search?q=machine+learning` returns nodes ranked by semantic similarity
- Results include both content and structural similarity scores
- Response time <200ms for 10k node graphs

### US-2: Ontology Gap Detection
> As an ontology curator, I want to automatically find pairs of concepts that are semantically similar but have no direct relationship, so I can identify missing edges.

**Acceptance criteria:**
- `GET /api/discovery/gaps?domain=ai&min_score=0.3` returns candidate missing relationships
- Each gap includes the similarity score and suggested relationship type
- False positive rate <30% (validated by domain expert)

### US-3: Related Node Exploration
> As a graph explorer, I want to see related concepts when I hover a node, combining both meaning-based and structure-based similarity.

**Acceptance criteria:**
- `GET /api/discovery/related/{iri}?top_k=5` returns top 5 similar nodes
- Results include relationship type ("content_similar", "structurally_similar", "combined")
- UI renders related nodes with visual connection indicators

### US-4: Visual Relationship Differentiation
> As a graph user, I want different relationship types to have visually distinct physics behavior so I can understand the graph structure at a glance.

**Acceptance criteria:**
- Hierarchy edges (SUBCLASS_OF) pull tighter than associative edges
- Bridge edges (cross-domain) have visible longer rest lengths
- Materialized N-hop edges are barely perceptible (ghost connections)
- No increase in GPU frame time >0.5ms

### US-5: Improved Layout Coherence
> As a visualization user, I want transitive relationships to contribute to layout so that distant relatives still cluster loosely.

**Acceptance criteria:**
- 2-hop materialized edges visible as loose grouping in layout
- 3-hop materialized edges prevent complete scattering of deep hierarchies
- Total materialized edge count ≤10x direct edge count

## Technical Architecture

### Data Flow

```
Neo4j (OntologyClass nodes + typed edges)
         │
         ├──► EmbeddingService (MiniLM-L6, 384-dim)
         │        └──► content_embedding_384 property on Neo4j nodes
         │
         ├──► KGETrainer (TransE, 128-dim)  
         │        └──► kge_embedding_128 property on Neo4j nodes
         │
         ├──► NHopMaterializer (2-hop, 3-hop)
         │        └──► MATERIALIZED_2HOP/3HOP relationships in Neo4j
         │
         └──► EdgeTypePhysicsConfig
                  └──► DynamicForceConfigGPU buffer on GPU
```

### API Surface

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/discovery/search` | GET | Combined similarity search |
| `/api/discovery/related/{iri}` | GET | Related nodes for hover/expand |
| `/api/discovery/gaps` | GET | Ontology gap detection |
| `/api/discovery/batch` | POST | Batch similarity for multiple nodes |
| `/api/discovery/index` | POST | Trigger re-indexing (admin) |
| `/api/discovery/train` | POST | Trigger KGE retraining (admin) |
| `/api/discovery/materialize` | POST | Trigger N-hop materialization (admin) |

### Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Embedding indexing (10k nodes) | <5 min | Batch timer |
| KGE training (50k triples) | <60s | Training duration |
| Discovery search latency | <200ms | p95 response time |
| GPU frame time impact | <0.5ms | CUDA timer delta |
| N-hop materialization | <30s | Batch timer |

## Dependencies

- MiniLM-L6-v2 embedding service (ruvector-postgres:8080/embed)
- Neo4j 5.x (existing)
- CUDA 12.x runtime (existing)
- `semantic_forces.cu` DynamicRelationshipBuffer (existing)
- `SemanticTypeRegistry` (existing)

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Embedding service unavailable | No content search | Graceful fallback to topology-only |
| KGE training divergence | Bad embeddings | Early stopping on loss plateau |
| N-hop edge explosion | Memory/perf | Cap per-node materialized edges |
| Stale embeddings after graph change | Incorrect results | Re-index on sync completion event |

## Success Metrics

- Discovery search click-through rate >40% (users find what they expected)
- Gap detection precision >70% (validated suggestions)
- Layout coherence score (visual clustering of related concepts) improves by >25%
- No GPU frame time regression beyond 0.5ms
