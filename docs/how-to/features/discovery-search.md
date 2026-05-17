---
title: Discovery & Similarity Search
description: Find semantically similar concepts, detect ontology gaps, and explore related nodes using combined content and structural analysis.
category: how-to
tags:
  - discovery
  - search
  - similarity
  - embeddings
  - feature-engineering
updated-date: 2026-05-05
difficulty-level: intermediate
adr-references: [ADR-072]
prd-references: [PRD-009]
---

# Discovery & Similarity Search

## Overview

VisionClaw's discovery system combines two complementary signals to find related concepts:

- **Content similarity** — what nodes *mean* (based on their text: labels, definitions, scope notes)
- **Topology similarity** — what nodes *connect to* (based on their position in the graph structure)

A weighted combination (default 60% content, 40% topology) surfaces results that neither signal alone would find.

## Searching for Similar Concepts

Find nodes semantically related to a free-text query:

```bash
GET /api/discovery/search?q=machine+learning&top_k=10
```

Optional parameters:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `q` | (required) | Search query text |
| `top_k` | 10 | Maximum results to return |
| `content_weight` | 0.6 | Weight for text-meaning similarity (0.0–1.0) |
| `topology_weight` | 0.4 | Weight for graph-structure similarity (0.0–1.0) |

Response:

```json
{
  "results": [
    {
      "iri": "urn:visionclaw:concept:abc123",
      "label": "Deep Learning",
      "score": 0.87,
      "content_score": 0.92,
      "topology_score": 0.79
    }
  ],
  "query": "machine learning",
  "total": 5
}
```

Adjust the weights to explore different angles:
- `content_weight=1.0&topology_weight=0.0` — pure text search (ignores graph structure)
- `content_weight=0.0&topology_weight=1.0` — pure structural role (ignores labels)

## Exploring Related Nodes

Given a specific node, find its nearest neighbours:

```bash
GET /api/discovery/related/{iri}?top_k=5
```

Returns nodes related to the given IRI by both content and structural similarity. Useful for "more like this" exploration from a known starting point.

## Detecting Ontology Gaps

Find pairs of concepts that are semantically similar but have no direct edge between them — potential missing relationships:

```bash
GET /api/discovery/gaps?min_score=0.3&limit=20
```

| Parameter | Default | Description |
|-----------|---------|-------------|
| `min_score` | 0.3 | Minimum combined similarity to report as a gap |
| `limit` | 20 | Maximum gaps to return |

Each result includes the pair of nodes, their similarity score, and a suggested relationship type.

## Batch Similarity

Compute pairwise similarity for a set of IRIs in a single request:

```bash
POST /api/discovery/batch
Content-Type: application/json

{
  "iris": ["urn:visionclaw:concept:a", "urn:visionclaw:concept:b", "urn:visionclaw:concept:c"],
  "top_k": 3
}
```

## When Results Are Unavailable

The discovery system depends on pre-computed embeddings. If embeddings haven't been generated yet, search falls back to topology-only results with a warning in the response. An administrator must run the indexing pipeline first (see [Pipeline Admin API](../operations/pipeline-admin-api.md)).

## How It Works

1. **Content embeddings** (384-dim) encode each node's text into a vector using MiniLM-L6-v2
2. **Topology embeddings** (128-dim) encode each node's structural role using TransE knowledge graph embeddings
3. **Search** computes cosine similarity in both spaces, combines with configurable weights
4. **Gap detection** identifies high-similarity pairs with no direct edge

For the underlying architecture, see [Feature Engineering Pipeline](../../explanation/feature-engineering-pipeline.md).
