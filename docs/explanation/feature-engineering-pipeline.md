---
title: Feature Engineering Pipeline
description: How VisionClaw transforms raw ontology data into ML-ready numeric representations for discovery, similarity search, and physics-informed layout.
category: explanation
tags:
  - machine-learning
  - embeddings
  - kge
  - transe
  - feature-engineering
  - autordf2gml
updated-date: 2026-05-05
adr-references: [ADR-072]
prd-references: [PRD-009]
ddd-references: [ddd-feature-engineering-context]
---

# Feature Engineering Pipeline

## Why Feature Engineering?

VisionClaw ingests rich ontology data — OWL classes with definitions, relationships, metadata — but raw text and graph edges aren't directly usable for ML-powered features like similarity search or gap detection. The feature engineering pipeline transforms these raw inputs into fixed-dimension numeric vectors that enable:

- **Similarity search**: "find concepts related to X" without exact label matching
- **Gap detection**: automatically discover missing relationships between semantically similar concepts
- **Layout differentiation**: different relationship types behave differently in the physics simulation

This approach is inspired by [AutoRDF2GML](https://dl.acm.org/doi/10.1007/978-3-031-77847-6_10) (ISWC 2024), which demonstrated that combining content-based and topology-based features via weighted addition achieves F1 0.940 on large-scale knowledge graphs — significantly outperforming either signal alone.

## Two Embedding Spaces

### Content Embeddings (384-dim)

Each ontology node's text fields (preferred term, definition, scope note) are concatenated and encoded by MiniLM-L6-v2 into a 384-dimensional vector. This captures *what a concept means* regardless of its position in the graph.

Text construction: `"{preferred_term}: {definition}. {scope_note}"`

Nodes with similar definitions cluster together even if they live in completely different parts of the ontology hierarchy. For example, "Machine Learning" and "Statistical Learning" would be near-neighbours in content space despite potentially being in different sub-trees.

The embedding service calls an external MiniLM endpoint in batches of 64, storing results as a `content_embedding_384` property on each Neo4j node.

### Topology Embeddings (128-dim)

TransE (Bordes et al., 2013) trains 128-dimensional entity embeddings from the graph's edge set. For each triple (head, relation, tail), TransE enforces `head + relation ≈ tail` in embedding space, learning representations that encode *structural role*.

Nodes that occupy similar positions in the graph topology (same depth, similar connectivity pattern, analogous relationship types) cluster together in topology space — even if their text is completely different.

Training parameters:
- 100 epochs, batch size 1024
- Margin-based loss with margin 1.0
- 5 negative samples per positive triple (corrupted head or tail)
- Learning rate 0.01 with L2 normalisation

Results are stored as `kge_embedding_128` on Neo4j nodes.

## Combined Scoring

Discovery queries combine both signals:

```
combined_score = content_weight × cosine(query_content, node_content)
              + topology_weight × cosine(query_topology, node_topology)
```

Default weights: 60% content, 40% topology. Users can adjust per-query to shift emphasis.

This combination finds results that pure text search misses (structurally similar but differently named) and results that pure graph analysis misses (semantically related but distantly connected).

## N-Hop Edge Materialisation

Direct edges only capture 1-hop relationships. Concepts connected via 2–3 intermediate steps have no spring attraction in the physics layout, causing hierarchy fragments to scatter.

The materialiser precomputes transitive connections:

| Hop Distance | Neo4j Relationship | Default Weight | Physics Effect |
|---|---|---|---|
| 2-hop | `MATERIALIZED_2HOP` | 0.05 | Very weak attraction — loose grouping |
| 3-hop | `MATERIALIZED_3HOP` | 0.02 | Barely perceptible — prevents scattering |

Rules:
- Traverses across relationship types (SUBCLASS_OF → RELATES → SUBCLASS_OF)
- Caps materialised edges per source node to prevent quadratic blowup
- Will not duplicate existing direct edges
- Feature-gated via `NHOP_MATERIALIZATION_ENABLED` environment variable

## Per-Edge-Type Physics

The `SemanticEdgeType` enum maps each relationship type to distinct force parameters uploaded to GPU constant memory:

| Edge Type | Rest Length | Spring Strength | Behaviour |
|-----------|-------------|-----------------|-----------|
| Hierarchical (SUBCLASS_OF) | 60 | 0.9 | Tight directional pull — clear parent/child |
| Structural | 80 | 0.7 | Orbit clustering |
| Dependency | 90 | 0.6 | Directional spring |
| ExplicitLink | 100 | 0.5 | Standard spring |
| Associative | 120 | 0.3 | Loose association |
| Namespace | 150 | 0.2 | Very loose grouping |
| Bridge (cross-domain) | 200 | 0.15 | Long-range cross-domain |
| Materialised 2-hop | 250 | 0.08 | Ghost connection |
| Materialised 3-hop | 350 | 0.03 | Barely visible |

This maps directly to the existing `DynamicForceConfigGPU` buffer in `semantic_forces.cu` — no CUDA kernel changes required, only different parameter values per edge type discriminant.

## Pipeline Execution

The pipeline runs as three independent batch operations, each triggered via admin endpoint:

1. **Index** (`POST /api/discovery/index`) — Embeds all ontology node text via MiniLM
2. **Train** (`POST /api/discovery/train`) — Trains TransE on the full edge set
3. **Materialise** (`POST /api/discovery/materialize`) — Creates N-hop transitive edges

These are designed to run post-sync (after new ontology data is ingested from GitHub). They are idempotent — re-running overwrites previous results.

## Bounded Context

The feature engineering pipeline sits downstream of Ontology Ingest (reads nodes and edges) and upstream of both GPU Physics (materialised edges, force config) and Client Discovery (similarity search API). See [DDD Feature Engineering Context](../ddd-feature-engineering-context.md) for the full domain model.
