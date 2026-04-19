---
title: VisionClaw Ontology Pipeline
description: End-to-end guide to VisionClaw's OWL 2 ontology processing pipeline — from GitHub Markdown ingestion through Neo4j storage to Whelk-rs EL++ reasoning and GPU constraint application
category: explanation
tags: [ontology, owl, whelk, reasoning, pipeline, neo4j, knowledge-graph]
updated-date: 2026-04-19
---

# VisionClaw Ontology Pipeline

End-to-end guide to VisionClaw's OWL 2 ontology processing pipeline — from GitHub Markdown ingestion through Neo4j storage to Whelk-rs EL++ reasoning and GPU constraint application.

---

## 1. Pipeline Overview

The ontology pipeline is a 5-stage process that converts Logseq-formatted Markdown files from a GitHub repository into GPU-enforced semantic physics constraints rendered in the 3D graph.

```mermaid
flowchart LR
    A[GitHub Repo\nMarkdown Files] --> B[OntologyParser\nRust]
    B --> C[Neo4j\nGraph Store]
    C --> D[Whelk-rs\nEL++ Reasoner]
    D --> E[GPU\nConstraints]
    E --> F[Client\n3D Graph]

    style A fill:#4A90D9,color:#fff
    style B fill:#4A90D9,color:#fff
    style C fill:#27AE60,color:#fff
    style D fill:#E67E22,color:#fff
    style E fill:#8E44AD,color:#fff
    style F fill:#2C3E50,color:#fff
```

**End-to-end timing** (production, warm cache):
- GitHub sync → Neo4j save: ~10 ms per file
- Reasoning (cache hit): < 1 ms
- Reasoning (cache miss, 50 classes): ~50 ms
- Constraint generation: ~1 ms per 100 axioms
- GPU upload: ~10–50 ms for 1,000 constraints
- **Total pipeline**: ~65–600 ms depending on ontology size and cache state

---

## 2. Stage 1: GitHub Markdown Ingestion

### Entry point

`GitHubSyncService::sync_graphs()` in `src/services/github-sync-service.rs` is the synchronization entry point. It fetches Markdown files from the configured GitHub repository (default path: `mainKnowledgeGraph/pages/`) in batches of 50 files.

### Incremental filtering

Sync uses SHA1 hashing to skip unchanged files. Set `FORCE_FULL_SYNC=1` to bypass SHA1 filtering and reprocess all files. Reset to `0` after a forced sync.

### File type detection

`detect_file_type()` classifies each file. Files tagged `public:: true` become `KnowledgeGraph` page nodes. Files lacking this tag are still scanned for `### OntologyBlock` sections (see below).

### OntologyBlock extraction

**Only `public:: true` files** produce KG page nodes and `[[wikilink]]` → `linked_page` conversions.

**All files regardless of `public:: true` status** are scanned for `### OntologyBlock` sections. These sections contain OWL class definitions, axioms, and properties in a structured Logseq property format.

```
### OntologyBlock
- term-id:: CL:0000540
- preferred-term:: Neuron
- owl:class:: Cell
- is-subclass-of:: [[Cell]]
- owl-axioms:: DisjointClasses(Neuron, Astrocyte)
```

After detecting an OntologyBlock, `save_ontology_data()` triggers the full downstream pipeline.

### Data flow in this stage

```
sync_graphs()
  └── process_single_file(page.md)
        ├── detect_file_type() → KnowledgeGraph | Ontology
        ├── KnowledgeGraphParser::parse() → page/linked_page nodes
        └── OntologyParser::parse() → OntologyData
              └── save_ontology_data()
                    ├── UnifiedOntologyRepository::save_ontology()
                    └── OntologyPipelineService::on_ontology_modified() [async task]
```

---

## 3. Stage 2: OWL Parsing

### Enhanced parser architecture

The enhanced OntologyParser (`src/services/parsers/ontology_parser.rs`, version 2.0.0) extracts the complete metadata set from OntologyBlock sections. It compiles regex patterns once at startup using `Lazy<Regex>` for zero-overhead parsing.

Key regex patterns:
- Property extraction: `^\s*-\s*([a-zA-Z0-9_:-]+)::\s*(.+)$`
- Wiki links: `\[\[([^\]]+)\]\]` — converted to `linked_page` nodes
- OWL axioms in code blocks: ` ```clojure ... ``` `
- Cross-domain bridges: `^\s*-\s*(bridges-(?:to|from))::\s*\[\[([^\]]+)\]\]\s*via\s+(\w+)`

### Three-tier property validation

Properties are divided into three tiers:

| Tier | Requirement | Examples |
|------|-------------|---------|
| 1 | Required | `term-id`, `preferred-term`, `owl:class`, `is-subclass-of` |
| 2 | Recommended | `alt-terms`, `quality-score`, `source`, `has-part`, `depends-on` |
| 3 | Optional | `bridges-to`, `bridges-from`, `owl-axioms`, domain extensions |

### OWL 2 EL profile support

The parser recognises the OWL 2 EL subset: SubClassOf, EquivalentClasses, DisjointClasses, ObjectSomeValuesFrom, ObjectIntersectionOf, transitive and reflexive properties. Universal quantification, negation, cardinality restrictions, and disjunction are outside EL++ scope and will produce "missing inferred axioms" warnings if used.

### Node types created

| Node Type | Source |
|-----------|--------|
| `page` | File with `public:: true` |
| `linked_page` | `[[wikilink]]` targets |
| `owl_class` | OntologyBlock with `owl:class` |
| `owl_property` | OntologyBlock with `owl:role` |
| `owl_individual` | OntologyBlock individual declarations |

---

## 4. Stage 3: Neo4j Storage

### Neo4j as primary graph store

Neo4j (v5.15 community) is the **primary and sole graph store** (migration from SQLite completed November 2025). All nodes and relationships are stored as Neo4j `KGNode` nodes with `EDGE` relationships.

### Node schema

```cypher
(:KGNode {
  id: Integer,              // Sequential u32 from NEXT_NODE_ID atomic counter
  metadata_id: String,      // File path or IRI
  label: String,
  owl_class_iri: String,    // OWL ontology class (populated for owl_* nodes)
  node_type: String,        // "page", "linked_page", "owl_class", etc.
  x, y, z: Float,          // Physics positions
  vx, vy, vz: Float        // Velocities
})
```

```cypher
[:EDGE {
  weight: Float,
  relation_type: String,   // "SUBCLASS_OF", "LINKS_TO", "HAS_PROPERTY", etc.
  owl_property_iri: String
}]
```

### Indexes and constraints

```cypher
CREATE CONSTRAINT kg_node_id IF NOT EXISTS FOR (n:KGNode) REQUIRE n.id IS UNIQUE
CREATE INDEX kg_node_metadata_id IF NOT EXISTS FOR (n:KGNode) ON (n.metadata_id)
CREATE INDEX kg_node_owl_class IF NOT EXISTS FOR (n:KGNode) ON (n.owl_class_iri)
```

### Edge types created by the ontology pipeline

| Relationship | Meaning |
|--------------|---------|
| `SUBCLASS_OF` | OWL SubClassOf axiom |
| `LINKS_TO` | `[[wikilink]]` or `relates-to` |
| `HAS_PROPERTY` | `owl:role` declaration |
| `EQUIVALENT_TO` | EquivalentClasses axiom |
| `DISJOINT_WITH` | DisjointClasses axiom |
| `HAS_PART` | `has-part` property |
| Namespace edges | Generated from `--` prefix convention |

### The 623 SUBCLASS_OF relationships

There are 623 `SUBCLASS_OF` relationships originating from `OwlClass` nodes in Neo4j. These require **label matching** to link `OwlClass` nodes to the corresponding `KGNode` entries. Without this mapping, the client graph receives isolated OwlClass nodes — see the **Ontology Edge Gap** section below.

### Namespace edge generation

Nodes whose `metadata_id` contains a `--` prefix (e.g., `ai--machine-learning`) automatically generate namespace grouping edges during `GraphStateActor` startup, placing them into the correct sub-graph cluster.

---

## 5. Stage 4: Whelk-rs Reasoning (EL++)

### What is Whelk-rs?

Whelk-rs is a high-performance OWL 2 EL reasoner written in Rust. It offers 10–100× speedup over traditional Java-based reasoners (Pellet, HermiT). Integration uses `horned-owl` (a Rust OWL parser) as the parsing layer.

```toml
horned-owl = { version = "1.2.0", features = ["remote"], optional = true }
whelk = { path = "./whelk-rs", optional = true }
```

### EL++ reasoning capabilities

| Construct | Inference |
|-----------|-----------|
| SubClassOf transitivity | A ⊑ B, B ⊑ C ⇒ A ⊑ C |
| DisjointClasses propagation | A ⊥ B, C ⊑ A ⇒ C ⊥ B |
| EquivalentClasses | Symmetric and transitive |
| Transitive properties | part-of-part propagates |
| FunctionalProperty constraints | Cardinality enforcement |

### Reasoning algorithms in CustomReasoner

The `CustomReasoner` (`src/reasoning/custom-reasoner.rs`, 466 lines) implements three algorithms:

1. **infer_transitive_subclass()** — computes transitive closure of SubClassOf, using `transitive_cache: HashMap<String, HashSet<String>>`. Worst-case O(n³), average O(n²).
2. **infer_disjoint()** — propagates disjointness to subclasses. Example: `Neuron ⊥ Astrocyte` → `PyramidalNeuron ⊥ Astrocyte`.
3. **infer_equivalent()** — symmetric and transitive equivalence closure.

All inferred axioms receive `confidence: 1.0` (deductive reasoning is certain).

### Inference caching

Results are cached with a Blake3 hash key:

```rust
let cache_key = blake3::hash(
    format!("{}:{}:{}", ontology_id, cache_type, ontology_hash).as_bytes()
).to_hex();
```

The in-memory `RwLock<HashMap<String, InferenceCacheEntry>>` provides sub-millisecond cache hits. A persistent SQLite `inference_cache` table exists for cross-session persistence (optional, database-backed caching).

Cache invalidation triggers when the ontology content hash changes or on explicit `InvalidateCache` message.

### Sequence diagram

```mermaid
sequenceDiagram
    participant Sync as GitHubSyncActor
    participant Parser as OntologyParser
    participant Neo4j as Neo4j
    participant Whelk as Whelk-rs / CustomReasoner
    participant GPU as SemanticForcesActor

    Sync->>Parser: parse_files(markdown_batch)
    Parser->>Parser: Extract OntologyBlocks
    Parser->>Neo4j: Create/update OWL nodes
    Neo4j-->>Parser: Node IDs

    Parser->>Whelk: TriggerReasoning(ontology_id)
    Whelk->>Whelk: Check Blake3 cache
    alt Cache miss
        Whelk->>Neo4j: get_classes() + get_axioms()
        Whelk->>Whelk: EL++ completion (~50ms for 50 classes)
        Whelk->>Neo4j: Store inferred_edges (is_inferred=true)
        Whelk->>Whelk: Store in-memory cache
    else Cache hit
        Whelk->>Whelk: Return cached axioms (<1ms)
    end
    Whelk->>GPU: UpdateSemanticConstraints(class_hierarchy)
    GPU-->>Whelk: ACK
```

### Performance benchmarks

| Operation | 10 classes | 50 classes | 100+ classes |
|-----------|-----------|-----------|--------------|
| Cold reasoning | ~15 ms | ~50 ms | ~150 ms |
| Cached retrieval | < 1 ms | < 1 ms | < 1 ms |
| Cache hit rate | > 90% in production |

| Operation | 1,000 classes | 5,000 classes | 10,000 classes |
|-----------|---------------|---------------|----------------|
| First inference | ~500 ms | ~2,000 ms | ~5,000 ms |
| Cached retrieval | < 10 ms | < 15 ms | < 20 ms |

---

## 6. Stage 5: GPU Constraint Application

### Axiom-to-constraint translation

`OntologyPipelineService::generate_constraints_from_axioms()` converts inferred axioms to typed physics constraints:

| Axiom Type | Constraint Kind | Default Strength | Effect |
|------------|----------------|-----------------|--------|
| `SubClassOf(A, B)` | Clustering / HierarchicalAttraction | 1.0× | Child nodes cluster near parent |
| `EquivalentClass(A, B)` | Alignment / Colocation | 1.5× | Nodes align strongly |
| `DisjointWith(A, B)` | Separation | 2.0× | Nodes repel (strong force) |

The `constraint_strength` multiplier in `SemanticPhysicsConfig` scales all constraint forces:

```rust
pub struct SemanticPhysicsConfig {
    pub auto_trigger_reasoning: bool,      // default: true
    pub auto_generate_constraints: bool,    // default: true
    pub constraint_strength: f32,           // default: 1.0 (range 0–10)
    pub use_gpu_constraints: bool,          // default: true
    pub max_reasoning_depth: usize,         // default: 10
    pub cache_inferences: bool,             // default: true
}
```

### Priority blending

When multiple constraints affect the same nodes, priorities resolve conflicts using exponential decay:

```
weight(priority) = 10^(-(priority-1)/9)

Priority 1: User-defined  → weight = 1.000 (100%)
Priority 5: Asserted axioms → weight = 0.359 (36%)
Priority 7: Inferred axioms → weight = 0.215 (22%)
Priority 10: Lowest        → weight = 0.100 (10%)
```

Inferred constraints receive priority 7; asserted constraints priority 5; user-defined priority 1.

### GPU upload path

1. `OntologyPipelineService::upload_constraints_to_gpu()` sends `ApplyOntologyConstraints` to `OntologyConstraintActor`
2. `OntologyConstraintActor` converts `Constraint` structs to `ConstraintData` (GPU format, 64-byte aligned) via `OntologyConstraintTranslator`
3. `upload_constraints_to_gpu()` calls `unified_compute.upload_constraints(&self.constraint_buffer)`
4. `ForceComputeActor` receives `UpdateOntologyConstraintBuffer` and caches the buffer
5. Each physics frame: `apply_ontology_forces()` re-uploads cached buffer before main force pass

### CUDA kernel invocations

Constraint kind `SEMANTIC = 10` in `ontology_constraints.cu`:
- `apply_disjoint_classes_kernel` — block size 256, target ~0.8 ms / 10K nodes
- `apply_subclass_hierarchy_kernel` — block size 256, target ~0.6 ms / 10K nodes
- `apply_sameas_colocate_kernel` — block size 256, target ~0.3 ms / 10K nodes
- **Total ontology constraint overhead**: ~2 ms / frame for 10K nodes

GPU data structures (64-byte aligned):

```cuda
struct OntologyNode {
    uint32_t graph_id;         // 4 bytes
    uint32_t node_id;          // 4 bytes
    uint32_t ontology_type;    // 4 bytes
    uint32_t constraint_flags; // 4 bytes
    float3 position;           // 12 bytes
    float3 velocity;           // 12 bytes
    float mass;                // 4 bytes
    float radius;              // 4 bytes
    uint32_t parent_class;     // 4 bytes
    uint32_t property_count;   // 4 bytes
    uint32_t padding[6];       // 24 bytes — TOTAL: 64 bytes
};

struct OntologyConstraint {
    uint32_t type;             // DisjointClasses=1, SubClassOf=2
    uint32_t source_id;        // 4 bytes
    uint32_t target_id;        // 4 bytes
    uint32_t graph_id;         // 4 bytes
    float strength;            // 4 bytes
    float distance;            // 4 bytes
    float padding[10];         // 40 bytes — TOTAL: 64 bytes
};
```

Memory footprint: 10K nodes = 640 KB, 1K constraints = 64 KB.

---

## 7. Ontology Query Interface

### Cypher traversal patterns (Neo4j)

```cypher
-- Find all subclasses of a given class (including inferred)
MATCH (n:KGNode {owl_class_iri: $iri})<-[:SUBCLASS_OF*]-(child)
RETURN child.label, child.id

-- Find nodes by OWL class
MATCH (n:KGNode {owl_class_iri: $iri})
RETURN n.id, n.label, n.metadata

-- Multi-hop ontology path
MATCH (n:KGNode {id: $start})-[:SUBCLASS_OF|:LINKS_TO*1..5]-(m:KGNode)
RETURN DISTINCT m.id, m.label
```

### REST API (via CypherQueryHandler)

**POST** `/api/query/cypher` — Execute Cypher with safety limits (max 300s timeout, max 10,000 results, read-only enforcement).

**POST** `/api/ontology-physics/enable` — Enable ontology force constraints:
```json
{
  "ontologyId": "university-ontology",
  "mergeMode": "replace",
  "strength": 0.8
}
```

**GET** `/api/ontology-physics/constraints` — List active constraints with GPU stats.

**PUT** `/api/ontology-physics/weights` — Adjust constraint strength at runtime.

### MCP Ontology Tools (7 tools)

The system exposes 7 MCP tools for ontology operations:
1. `infer_axioms` — trigger EL++ reasoning for an ontology ID
2. `get_class_hierarchy` — retrieve complete class tree
3. `get_disjoint_classes` — list disjoint class pairs
4. `invalidate_cache` — force cache invalidation
5. `get_cache_stats` — reasoning cache statistics
6. `query_cypher` — execute Cypher on Neo4j
7. `get_constraints` — list active GPU constraints

### OntologyReasoningService API

```rust
// Core service methods
pub async fn infer_axioms(&self, ontology_id: &str) -> Result<Vec<InferredAxiom>>
pub async fn get_class_hierarchy(&self, ontology_id: &str) -> Result<ClassHierarchy>
pub async fn get_disjoint_classes(&self, ontology_id: &str) -> Result<Vec<DisjointClassPair>>

// Data structures
pub struct InferredAxiom {
    pub axiom_type: String,       // "SubClassOf", "DisjointWith", etc.
    pub subject_iri: String,
    pub object_iri: Option<String>,
    pub confidence: f32,          // 1.0 for deductive, 0.7-0.9 for inferred
    pub inference_path: Vec<String>,
    pub user_defined: bool,
}

pub struct ClassHierarchy {
    pub root_classes: Vec<String>,
    pub hierarchy: HashMap<String, ClassNode>,
}

pub struct ClassNode {
    pub iri: String,
    pub label: String,
    pub parent_iri: Option<String>,
    pub children_iris: Vec<String>,
    pub node_count: usize,  // Descendant count
    pub depth: usize,
}
```

---

## 8. The Ontology Edge Gap Problem

This is a known architectural debt item affecting the current production system.

### Symptom

62% of `OwlClass` nodes in the client graph are isolated — they have no edges connecting them to other nodes, even though 623 `SUBCLASS_OF` relationships exist in Neo4j.

### Root cause

`OwlClass` nodes in Neo4j have a different label format than `KGNode` entries. The 623 `SUBCLASS_OF` relationships originate from `OwlClass` source nodes, but the client-side `KGNode` entries use a different ID scheme. The mapping between `OwlClass` nodes and `KGNode` entries requires label-based matching that is not currently implemented.

### Impact

- 62% of ontology nodes are visually isolated in the 3D graph
- Ontology hierarchy is not visually represented
- SemanticForcesActor receives incomplete constraint data

### Proposed fix

Map `OwlClass` → `KGNode` via `owl_class_iri` field matching:

```cypher
MATCH (oc:OwlClass)-[:SUBCLASS_OF]->(parent:OwlClass)
MATCH (gn_child:KGNode {owl_class_iri: oc.iri})
MATCH (gn_parent:KGNode {owl_class_iri: parent.iri})
CREATE (gn_child)-[:SUBCLASS_OF]->(gn_parent)
```

This is tracked as a P1 architectural debt item.

---

## 9. Environment Variables and Configuration

```bash
# Reasoning configuration
REASONING_CACHE_TTL=3600          # Cache lifetime (seconds)
REASONING_TIMEOUT=30000           # Max reasoning time (ms)
REASONING_MAX_AXIOMS=100000       # Axiom limit

# Sync configuration
FORCE_FULL_SYNC=0                 # Set to 1 to bypass SHA1 filtering
GITHUB_BASE_PATH=mainKnowledgeGraph/pages/

# Neo4j connection
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=<secret>
NEO4J_ENABLED=true
```

```toml
[features]
ontology_validation = true
reasoning_cache = true
ontology = ["horned-owl", "whelk", "walkdir", "clap"]
```

---

## 10. Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| "Reasoning timeout" | Large ontology / complex axioms | Increase `REASONING_TIMEOUT` or reduce ontology size |
| "Cache invalidation loop" | Ontology hash changes on every read | Ensure consistent serialisation; normalise whitespace |
| "Missing inferred axioms" | Axiom uses OWL 2 construct outside EL++ | Verify no universal quantification, negation, or cardinality restrictions |
| Constraints not applied | `auto_generate_constraints` disabled | Check pipeline config |
| GPU upload failures | Constraint actor not initialized / OOM | Check CUDA logs; CPU fallback activates automatically |
| OntologyBlock not detected | Missing `### OntologyBlock` header | Verify exact header format in Markdown |

**Debug log patterns to watch:**
```
🔄 Triggering ontology reasoning pipeline after ontology save
✅ Reasoning complete: 67 inferred axioms
🔧 Generating constraints from 67 axioms
📤 Uploading 67 constraints to GPU
✅ Constraints uploaded to GPU successfully
🎉 Ontology pipeline complete: 67 axioms inferred, 67 constraints generated, GPU upload: true
```

---

## Related Documentation

- [Physics & GPU Engine](physics-gpu-engine.md) — how constraints affect node positioning
- `docs/explanation/ontology-pipeline.md` — actor wire-up analysis
- `docs/reference/neo4j-schema-unified.md` — full storage schema
- `docs/explanation/ontology-pipeline.md` — detailed sequence diagrams including error and backpressure paths
- `docs/reference/neo4j-schema-unified.md` — Neo4j configuration and Cypher reference
