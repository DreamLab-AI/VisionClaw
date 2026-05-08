# PRD-005: VisionClaw v2 Ontology Pipeline Refactor

**Status:** Draft
**Author:** Architecture Agent
**Date:** 2026-04-26
**Priority:** P0 -- blocks ontology correctness for all downstream consumers
**Companion ADRs:** [ADR-014](adr/ADR-014-semantic-pipeline-unification.md)
**DDD Context:** [Semantic Pipeline](explanation/ddd-semantic-pipeline.md)

---

## 1. Problem Statement

The VisionFlow Rust backend parses Logseq markdown from `jjohare/logseq` and imports it into Neo4j. The parser (`ontology_parser.rs`) expects **v4 format** pages wrapped in a `- ### OntologyBlock` section with short-code domain prefixes (`AI-`, `BC-`, `mv:`, etc.) and `term-id` as the primary identifier.

The ontology has been migrated to **VisionClaw v2 IRI-first format** (`PAGE-FORMAT.md`). v2 pages:

- Place properties **flat at page level** (no `### OntologyBlock` wrapper).
- Use `iri::` (line 1) as canonical HTTP IRI and `uri::` (line 2) as operational URN.
- Require `rdf-type::`, `same-as::`, and `content-hash::` properties.
- Use **full-word domain values** (`artificial-intelligence`, not `ai`).
- Use full-word `owl-class::` prefixes (`artificial-intelligence:AIAgentSystem`).
- Annotate `bridges-to::` with `(domain: <target-domain>)`.

The current parser returns `Err("No OntologyBlock found in file")` for every v2 page. The knowledge graph parser (`knowledge_graph_parser.rs`) imports v2 pages as plain KG nodes but discards all ontology-specific properties (IRI, URN, rdf-type, domain, content-hash, quality-score, bridges-to annotations).

Additionally, the sync service only imports from a single `pages/` directory. v2 splits content across `mainKnowledgeGraph/pages/` (ontology) and `workingGraph/pages/` (working notes), both of which must be ingested.

---

## 2. Goals

| # | Goal | Success Metric |
|---|------|----------------|
| G1 | Parse v2 IRI-first flat properties | All 6 domain namespaces import with IRI, URN, rdf-type, same-as, content-hash |
| G2 | Backward-compatible v4 parsing | Existing `### OntologyBlock` pages still parse without regression |
| G3 | Neo4j schema carries v2 fields | `iri`, `uri`, `rdf_type`, `same_as`, `content_hash`, `quality_score`, `domain` indexed on `:OntologyNode` |
| G4 | Dual-graph import | Single sync run ingests both `mainKnowledgeGraph/pages/` and `workingGraph/pages/` |
| G5 | Content-hash change detection | Skip re-import when `content-hash` matches stored value (complement to SHA1) |
| G6 | JSON-LD context integration | API can emit JSON-LD for any node using the v2 context document |
| G7 | Domain/quality/bridge filtering | API supports `?domain=`, `?min_quality=`, `?bridges_only=` query parameters |

---

## 3. Non-Goals

- OWL reasoner changes (Whelk). The reasoner consumes `OwlClass`/`OwlAxiom`; its input contract does not change.
- Binary protocol changes. The `BinaryNodeData` wire format is unchanged; new fields are metadata-only.
- Solid Pod integration changes. The `IngestSaga` pod-first path is unaffected.
- CUDA kernel changes. `SemanticEdgeType` discriminants are stable.

---

## 4. Architecture Overview

```
GitHub (jjohare/logseq)
  |
  +-- mainKnowledgeGraph/pages/*.md   (ontology, v2 format)
  +-- workingGraph/pages/*.md         (notes, plain Logseq)
  |
  v
EnhancedContentAPI  (fetch from both paths)
  |
  v
GitHubSyncService   (dual-graph orchestration)
  |
  +-- detect_file_type()              (v2-aware classification)
  |
  +-- OntologyParser                  (v2 flat + v4 OntologyBlock)
  |     |
  |     +-- parse_v2_flat()           [NEW]
  |     +-- parse_enhanced()          [EXISTING, v4 path]
  |     +-- OntologyBlock             (extended with v2 fields)
  |
  +-- KnowledgeGraphParser            (extract v2 page-level props into Node metadata)
  |
  v
Neo4j  (:OntologyNode with v2 properties, :KGNode with enrichment)
  |
  v
REST API  (JSON-LD emission, domain/quality/bridge filters)
```

---

## 5. Detailed Design

### 5.1 Parser Refactoring

#### 5.1.1 v2 Format Detection

Add a detection function that identifies v2 pages by checking whether the first non-empty line starts with `iri::`. This replaces the `### OntologyBlock` gate for v2 content.

```
fn is_v2_format(content: &str) -> bool
    // true if the first property line is `iri:: http://...`
    // also true if `context:: https://visionclaw.dreamlab-ai.systems/ns/v2` is present
```

#### 5.1.2 OntologyBlock struct extensions

Add these fields to `OntologyBlock`:

| Field | Type | Source Property |
|-------|------|-----------------|
| `iri` | `Option<String>` | `iri::` |
| `uri` | `Option<String>` | `uri::` |
| `rdf_type` | `Option<String>` | `rdf-type::` |
| `same_as` | `Option<String>` | `same-as::` |
| `context_url` | `Option<String>` | `context::` |
| `content_hash` | `Option<String>` | `content-hash::` |
| `legacy_term_id` | `Option<String>` | `legacy-term-id::` |
| `author_did` | `Option<String>` | `author-did::` |
| `signature` | `Option<String>` | `signature::` |
| `created` | `Option<String>` | `created::` |
| `modified` | `Option<String>` | `modified::` |
| `format_version` | `FormatVersion` | Detected (`V2` or `V4`) |

The existing `source_domain` field maps to `domain::` (v2) or `source-domain::` (v4). The existing `owl_class` field maps to `owl-class::` (v2 kebab-case) or `owl:class::` (v4 colon-separated).

#### 5.1.3 Domain prefix mapping update

Replace `DOMAIN_PREFIXES` (v4 short codes) with a bidirectional map:

```
v1/v4 short code  <->  v2 full-word domain
AI-  / ai         <->  artificial-intelligence
BC-  / bc         <->  blockchain
MV-  / mv         <->  spatial-computing
RB-  / rb         <->  robotics
TC-  / tc         <->  distributed-collaboration
DT-  / dt         <->  infrastructure
```

The `get_full_iri()` method must accept both `ai:Thing` and `artificial-intelligence:Thing` and resolve to the same namespace.

#### 5.1.4 New parse_v2_flat method

```rust
fn parse_v2_flat(&self, content: &str, filename: &str) -> Result<OntologyBlock, String>
```

This method:
1. Reads all top-level `key:: value` lines (lines not indented under a `- ###` header).
2. Reads section headers (`- ### Definition`, `- ### Semantic Classification`, `- ### Relationships`, `- ### Content`, `- ### Provenance`) and their indented properties.
3. Populates OntologyBlock with both v2-specific fields and the shared fields that already exist.
4. Parses `bridges-to:: [[Target]] (domain: robotics)` into a structured representation capturing both the target and the annotated domain.

#### 5.1.5 Unified entry point

```rust
pub fn parse_enhanced(&self, content: &str, filename: &str) -> Result<OntologyBlock, String> {
    if is_v2_format(content) {
        self.parse_v2_flat(content, filename)
    } else {
        self.parse_v4_ontology_block(content, filename)  // renamed from current parse_enhanced
    }
}
```

#### 5.1.6 bridges-to structured representation

Currently `bridges_to: Vec<String>` stores raw wiki-link text. Extend to:

```rust
pub struct BridgeRef {
    pub target: String,           // wiki-link target name
    pub target_domain: Option<String>,  // annotated domain (full-word)
}

pub bridges_to: Vec<BridgeRef>,  // replaces Vec<String>
```

### 5.2 Neo4j Schema Changes

#### 5.2.1 New node properties on `:OntologyNode`

| Property | Neo4j Type | Index | Description |
|----------|-----------|-------|-------------|
| `iri` | STRING | UNIQUE | Canonical HTTP IRI (`@id`) |
| `uri` | STRING | INDEX | Operational URN |
| `rdf_type` | STRING | INDEX | Always `owl:Class` for concept pages |
| `same_as` | STRING | -- | owl:sameAs link (URN) |
| `content_hash` | STRING | INDEX | `sha256-12-<hex>` for change detection |
| `domain` | STRING | INDEX | Full-word domain value |
| `quality_score` | FLOAT | INDEX | 0.00-1.00 |
| `authority_score` | FLOAT | -- | 0.00-1.00 |
| `context_url` | STRING | -- | JSON-LD context URL |
| `legacy_term_id` | STRING | INDEX | Old `AI-0600` style ID |
| `format_version` | STRING | -- | `v2` or `v4` |
| `author_did` | STRING | -- | `did:nostr:...` |
| `created` | DATETIME | -- | ISO-8601 |
| `modified` | DATETIME | INDEX | ISO-8601 |

#### 5.2.2 New relationship property

`BRIDGES_TO` relationships gain a `target_domain` property:

```cypher
(a:OntologyNode)-[:BRIDGES_TO {target_domain: "robotics"}]->(b:OntologyNode)
```

#### 5.2.3 New label for graph origin

```cypher
:MainGraph   -- nodes from mainKnowledgeGraph/pages/
:WorkingGraph -- nodes from workingGraph/pages/
```

Applied as additional labels alongside existing `:KGNode` or `:OntologyNode`.

#### 5.2.4 Migration Cypher (run once)

```cypher
// Add format_version to existing nodes
MATCH (n:OntologyNode) WHERE n.format_version IS NULL
SET n.format_version = 'v4';

// Backfill domain from term_id prefix for existing v4 nodes
MATCH (n:OntologyNode) WHERE n.domain IS NULL AND n.term_id IS NOT NULL
SET n.domain = CASE
  WHEN n.term_id STARTS WITH 'AI-' THEN 'artificial-intelligence'
  WHEN n.term_id STARTS WITH 'BC-' THEN 'blockchain'
  WHEN n.term_id STARTS WITH 'MV-' THEN 'spatial-computing'
  WHEN n.term_id STARTS WITH 'RB-' THEN 'robotics'
  WHEN n.term_id STARTS WITH 'TC-' THEN 'distributed-collaboration'
  WHEN n.term_id STARTS WITH 'DT-' THEN 'infrastructure'
  ELSE NULL
END;

// Create indexes
CREATE INDEX idx_ontology_iri IF NOT EXISTS FOR (n:OntologyNode) ON (n.iri);
CREATE INDEX idx_ontology_uri IF NOT EXISTS FOR (n:OntologyNode) ON (n.uri);
CREATE INDEX idx_ontology_domain IF NOT EXISTS FOR (n:OntologyNode) ON (n.domain);
CREATE INDEX idx_ontology_quality IF NOT EXISTS FOR (n:OntologyNode) ON (n.quality_score);
CREATE INDEX idx_ontology_content_hash IF NOT EXISTS FOR (n:OntologyNode) ON (n.content_hash);
CREATE INDEX idx_ontology_legacy_id IF NOT EXISTS FOR (n:OntologyNode) ON (n.legacy_term_id);
CREATE INDEX idx_ontology_modified IF NOT EXISTS FOR (n:OntologyNode) ON (n.modified);
CREATE CONSTRAINT uniq_ontology_iri IF NOT EXISTS FOR (n:OntologyNode) REQUIRE n.iri IS UNIQUE;
```

### 5.3 Dual-Graph Support

#### 5.3.1 GitHub content fetch changes

The `EnhancedContentAPI` currently lists files from a single `pages/` path. Change to:

```rust
pub struct GraphSource {
    pub path: String,           // "mainKnowledgeGraph/pages" or "workingGraph/pages"
    pub label: GraphOrigin,     // MainGraph or WorkingGraph
}

pub enum GraphOrigin {
    MainGraph,
    WorkingGraph,
}
```

The sync service iterates over both sources in a single run, tagging each file with its origin.

#### 5.3.2 Sync orchestration

```
for source in [mainKnowledgeGraph, workingGraph]:
    files = content_api.list_files(source.path)
    for batch in files.chunks(BATCH_SIZE):
        process_batch(batch, source.label)
```

Each node receives an additional Neo4j label (`:MainGraph` or `:WorkingGraph`) and a `graph_origin` metadata property.

### 5.4 JSON-LD Integration

#### 5.4.1 Context storage

Store the JSON-LD context URL on each node (`context_url` property). The actual context document (`visionclaw-ontology-schema-v2.json`) is served statically or fetched from the URL.

#### 5.4.2 API JSON-LD emission

New endpoint: `GET /api/v1/nodes/{id}/jsonld`

Response shape:

```json
{
  "@context": "https://visionclaw.dreamlab-ai.systems/ns/v2",
  "@id": "http://narrativegoldmine.com/artificial-intelligence#AIAgentSystem",
  "@type": "owl:Class",
  "uri": "urn:visionclaw:concept:artificial-intelligence:ai-agent-system",
  "preferred-term": "AI Agent System",
  "domain": "artificial-intelligence",
  "is-subclass-of": ["http://narrativegoldmine.com/artificial-intelligence#AutonomousSystem"],
  "bridges-to": [{"@id": "http://narrativegoldmine.com/robotics#Robot"}],
  "quality-score": 0.92
}
```

### 5.5 Domain Model Changes (Node struct)

The `Node` struct (`models/node.rs`) already has `owl_class_iri: Option<String>` and a `metadata: HashMap<String, String>` bag. v2 fields will be stored in `metadata` during parsing and promoted to first-class Neo4j properties during the Cypher projection. No new fields on `Node` itself -- this keeps the binary protocol stable.

The `Metadata` struct (`models/metadata.rs`) gains:

| Field | Type |
|-------|------|
| `iri` | `Option<String>` |
| `uri` | `Option<String>` |
| `rdf_type` | `Option<String>` |
| `same_as` | `Option<String>` |
| `content_hash` | `Option<String>` |
| `domain` | `Option<String>` |
| `legacy_term_id` | `Option<String>` |
| `format_version` | `Option<String>` |

### 5.6 Incremental Sync (Content-Hash)

The existing sync uses GitHub SHA1 blob hashes to skip unchanged files. v2 adds a semantic content-hash (`sha256-12-<hex>`) computed from definition + relationships (not whitespace or cosmetic edits).

Change detection flow:

```
1. GitHub SHA1 differs?  NO  -> skip entirely (file untouched)
                          YES -> fetch content
2. Parse content-hash:: from page
3. content-hash matches Neo4j stored value?  YES -> skip re-import (cosmetic edit only)
                                              NO  -> full re-import
```

This avoids re-running the ontology pipeline for whitespace-only or metadata-only edits that do not change the semantic content.

### 5.7 API Changes

#### 5.7.1 New query parameters on `GET /api/v1/graph`

| Parameter | Type | Description |
|-----------|------|-------------|
| `domain` | STRING | Filter nodes by full-word domain |
| `min_quality` | FLOAT | Minimum quality_score threshold |
| `max_quality` | FLOAT | Maximum quality_score threshold |
| `bridges_only` | BOOL | Return only nodes that have BRIDGES_TO relationships |
| `graph_origin` | STRING | `main` or `working` |
| `format_version` | STRING | `v2` or `v4` |

#### 5.7.2 New endpoint

`GET /api/v1/domains` -- returns domain summary:

```json
[
  {"domain": "artificial-intelligence", "count": 142, "avg_quality": 0.87},
  {"domain": "blockchain", "count": 98, "avg_quality": 0.81}
]
```

### 5.8 Migration Path

#### Phase 1: Parser (this PRD)
1. Extend `OntologyBlock` with v2 fields.
2. Add `parse_v2_flat()` behind the `is_v2_format()` gate.
3. Update `detect_file_type()` to recognize v2 pages (have `iri::` at page level, no `### OntologyBlock`).
4. Add dual-graph path listing to `EnhancedContentAPI`.
5. Run migration Cypher to backfill `format_version` and `domain` on existing v4 nodes.

#### Phase 2: Neo4j + API
6. Add new indexes and constraint.
7. Implement content-hash change detection.
8. Add JSON-LD emission endpoint.
9. Add domain/quality/bridge query parameters.

#### Phase 3: Cleanup
10. Deprecate `term-id` as primary identifier (keep as `legacy_term_id`).
11. Remove `DOMAIN_PREFIXES` short-code-only map once all pages migrated.
12. Update OWL export pipeline to use `iri::` as the class IRI.

---

## 6. File-by-File Implementation Plan

### 6.1 Parser Layer

| # | File | Change |
|---|------|--------|
| 1 | `src/services/parsers/ontology_parser.rs` | Add `FormatVersion` enum (`V2`, `V4`). Add `BridgeRef` struct. Extend `OntologyBlock` with v2 fields (`iri`, `uri`, `rdf_type`, `same_as`, `context_url`, `content_hash`, `legacy_term_id`, `author_did`, `signature`, `created`, `modified`, `format_version`). Change `bridges_to` from `Vec<String>` to `Vec<BridgeRef>`. Add `is_v2_format()` function. Add `parse_v2_flat()` method. Rename current `parse_enhanced` body to `parse_v4_ontology_block`. Update `parse_enhanced` to dispatch between v2 and v4. Update `DOMAIN_PREFIXES` to a bidirectional map supporting both short codes and full-word domains. Update `get_full_iri()` to accept `artificial-intelligence:Foo` in addition to `ai:Foo`. Update `validate()` to use v2 required-field rules when `format_version == V2` (require `iri`, `rdf_type`, `domain` instead of `term-id`, `source-domain`, `owl:physicality`). |
| 2 | `src/services/parsers/knowledge_graph_parser.rs` | In `extract_page_meta` / `create_page_node`: extract v2 page-level properties (`iri::`, `uri::`, `domain::`, `content-hash::`, `quality-score::`, `rdf-type::`) and insert them into `Node.metadata`. This ensures the KG parser path also captures v2 ontology metadata for nodes that take the `FileType::KnowledgeGraph` branch. |
| 3 | `src/services/parsers/mod.rs` | Re-export `BridgeRef`, `FormatVersion`. |

### 6.2 Model Layer

| # | File | Change |
|---|------|--------|
| 4 | `src/models/metadata.rs` | Add fields: `iri`, `uri`, `rdf_type`, `same_as`, `content_hash`, `domain`, `legacy_term_id`, `format_version` (all `Option<String>`). These are populated during KG parsing for downstream Cypher projection. |
| 5 | `src/models/edge.rs` | No struct changes. `SemanticEdgeType::Bridge` already exists. Add a `target_domain` field to the `Edge` struct's metadata map when the source is a v2 `bridges-to` with domain annotation. |
| 6 | `src/models/node.rs` | No struct changes. v2 fields flow through `metadata: HashMap<String, String>`. |

### 6.3 Sync Layer

| # | File | Change |
|---|------|--------|
| 7 | `src/services/github_sync_service.rs` | (a) Update `detect_file_type()`: if `is_v2_format(content)` is true, return `FileType::KnowledgeGraph` (v2 pages always have `public:: true` or are treated as public by convention; ontology extraction happens inside the KG branch). (b) Add `GraphOrigin` enum and `GraphSource` struct. (c) Update `sync_graphs()` to iterate over both `mainKnowledgeGraph/pages` and `workingGraph/pages`. (d) Tag each processed node with `graph_origin` in its metadata. (e) Add content-hash comparison: after fetching content and parsing, compare `content_hash` against stored value in Neo4j; skip re-import if identical. |
| 8 | `src/services/github/content_enhanced.rs` | Add method `list_files_at_path(path: &str)` that accepts a sub-directory path instead of hardcoding `pages/`. The existing `list_files()` becomes `list_files_at_path("pages")` for backward compat. |
| 9 | `src/services/github/types.rs` | Add `graph_origin: Option<String>` to `GitHubFileBasicMetadata`. |

### 6.4 Neo4j Adapter Layer

| # | File | Change |
|---|------|--------|
| 10 | `src/adapters/neo4j_ontology_repository.rs` | (a) Update MERGE/CREATE Cypher for `:OntologyNode` to include new v2 properties (`iri`, `uri`, `rdf_type`, `same_as`, `content_hash`, `domain`, `quality_score`, `authority_score`, `context_url`, `legacy_term_id`, `format_version`, `author_did`, `created`, `modified`). (b) Update BRIDGES_TO relationship creation to include `target_domain` property. (c) Add query methods: `find_by_iri()`, `find_by_content_hash()`, `list_domains()`. |
| 11 | `src/adapters/` (KG adapter) | Update the KG node Cypher projection to write v2 metadata fields (`iri`, `uri`, `domain`, `content_hash`, `quality_score`) as first-class properties on `:KGNode` when present. Add `:MainGraph` or `:WorkingGraph` label based on `graph_origin`. |

### 6.5 Enrichment Layer

| # | File | Change |
|---|------|--------|
| 12 | `src/services/ontology_enrichment_service.rs` | Update enrichment lookup to match on `iri` (v2) or `owl_class` IRI (v4). When enriching a KG node, prefer `iri` match if present. |
| 13 | `src/services/ontology_converter.rs` | Update OWL/TTL export to use `iri::` as the class IRI for v2 nodes (instead of constructing from owl_class prefix). |

### 6.6 API Layer

| # | File | Change |
|---|------|--------|
| 14 | `src/routes/graph_routes.rs` (or equivalent) | Add query parameters: `domain`, `min_quality`, `max_quality`, `bridges_only`, `graph_origin`, `format_version`. Pass through to repository query. |
| 15 | `src/routes/graph_routes.rs` | Add `GET /api/v1/nodes/{id}/jsonld` endpoint. Fetches node from Neo4j, maps properties to JSON-LD using the v2 context keys, returns with `Content-Type: application/ld+json`. |
| 16 | `src/routes/graph_routes.rs` | Add `GET /api/v1/domains` endpoint. Aggregation query: `MATCH (n:OntologyNode) RETURN n.domain, count(n), avg(n.quality_score)`. |

### 6.7 Migration Scripts

| # | File | Change |
|---|------|--------|
| 17 | `scripts/migrations/v2_schema_migration.cypher` | [NEW] Contains the migration Cypher from section 5.2.4: backfill `format_version`, `domain`; create indexes and constraint. |

### 6.8 Tests

| # | File | Change |
|---|------|--------|
| 18 | `tests/parsers/test_ontology_parser_v2.rs` | [NEW] Unit tests for `parse_v2_flat()` with sample v2 markdown. Test: IRI extraction, domain mapping, bridges-to with domain annotation, content-hash extraction, backward compat (v4 pages still parse). |
| 19 | `tests/parsers/test_kg_parser_v2_metadata.rs` | [NEW] Verify that v2 page-level properties appear in `Node.metadata` after KG parsing. |
| 20 | `tests/services/test_dual_graph_sync.rs` | [NEW] Integration test mocking `EnhancedContentAPI` to return files from both graph paths. Verify both `:MainGraph` and `:WorkingGraph` labels applied. |

---

## 7. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| v4 regression -- existing ontology pages break | Medium | High | `is_v2_format()` gate ensures v4 pages take the unchanged code path. Comprehensive v4 regression test suite. |
| IRI collision between v2 and v4 representations of the same concept | Low | High | `legacy_term_id` on v2 nodes enables dedup. Migration script links v4 `term_id` to v2 `legacy_term_id`. MERGE on `iri` (v2) or `term_id` (v4), never both simultaneously. |
| Large sync time with dual-graph | Medium | Medium | Content-hash skip reduces re-imports. Batch size remains 50. Both graphs process in sequence within one run. |
| JSON-LD context drift | Low | Low | Context URL stored per-node. API reads it dynamically. Version bumps are explicit (`/ns/v3` etc.). |
| `bridges-to` parsing regression | Medium | Medium | `BridgeRef` struct is backward-compatible: `BridgeRef { target, target_domain: None }` for v4 pages that lack the `(domain: ...)` annotation. |

---

## 8. Dependencies

- VisionClaw v2 JSON-LD context document served at `https://visionclaw.dreamlab-ai.systems/ns/v2` (currently in-repo at `docs/schema/visionclaw-ontology-schema-v2.json`).
- All mainKnowledgeGraph pages migrated to v2 format (migration script in Logseq repo, separate from this PRD).
- Neo4j 5.x (current) supports all required index types.

---

## 9. Open Questions

1. **workingGraph visibility** -- Should `workingGraph/pages/` files default to `private` visibility even if `public:: true` is present? Current assumption: respect the `public::` property regardless of graph origin.
2. **IRI dereferenceability** -- The v2 spec says `iri::` must be dereferenceable. Is `http://narrativegoldmine.com/artificial-intelligence#AIAgentSystem` currently resolvable, or is this aspirational? If aspirational, the API JSON-LD endpoint could serve as the dereference target.
3. **content-hash recomputation** -- Should the Rust backend verify the `content-hash::` value in the page against its own recomputation, or trust the value written by the Logseq save hook?
4. **OWL axiom code blocks** -- v2 pages do not include ```` ```clojure ```` axiom blocks. Should the OWL export pipeline generate axioms purely from structured properties for v2 nodes?
