---
title: Logseq / Ontology Corpus & Pipeline Audit
description: Six-agent parallel audit of the hybrid Logseq/OWL knowledge pipeline in VisionClaw — corpus quality, ingestion loss, ontology bridge gaps, and namespace design issues
date: 2026-04-18
status: findings
audience: project owner
---

# Logseq / Ontology Audit — Master Report

## Context

Six specialist agents audited the full pipeline from Markdown on disk through VisionClaw's ingestion into the live Neo4j graph. The goal was to characterise data quality, ingestion fidelity, ontology design, and assumption gaps before retuning physics defaults.

Sources audited:

| Corpus | Location | Size |
|---|---|---|
| mainKnowledgeGraph | `/home/devuser/workspace/logseq/mainKnowledgeGraph/pages` | 2,865 .md files, 37 MB |
| workingGraph | `/home/devuser/workspace/logseq/workingGraph/pages` | 465 .md files, 6.2 MB |
| journals | `/home/devuser/workspace/logseq/*/journals` | 1,028 entries |
| VisionClaw parser | `/home/devuser/workspace/project/src/services/{parsers,ontology_*,github_sync_service}.rs` | — |
| Live Neo4j graph | `visionflow-neo4j` | 2,242 GraphNodes, 3,812 edges |

---

## The Big Picture: a 33% ingestion cliff

3,330 source pages yield **2,242 GraphNodes** in Neo4j. That's a 32.7% loss with two distinct failure modes:

1. **462 pages (13.9%)** never indexed at all — didn't even reach `FileMetadata` stage.
2. **626 pages (18.8%)** indexed as `FileMetadata` but never converted to `GraphNode`. `FileMetadata` (2,868 rows) and `GraphNode` (2,242 rows) exist as **disconnected node categories with zero cross-linking edges**.

Additionally:
- **1,028 journal entries** are silently excluded by design.
- **workingGraph**: only 193/465 are `public:: true`; yet 444 of 465 (95.5%) are **also duplicated** in mainKnowledgeGraph. The working→main promotion appears to be bulk filesystem mirroring, not a gated pipeline.

---

## What's Actually In the Data

### mainKnowledgeGraph (research corpus)

| Metric | Value |
|---|---|
| Pages | 2,865 |
| With `### OntologyBlock` | 2,275 (78.6%) |
| With `public-access::` declared | 2,090 (72.3%) |
| With `owl:class::` | 2,257 (78.0%) |
| With `subClassOf` axiom | 977 (33.8%) |
| With `disjointWith` axiom | **0 (0%)** |
| With Clojure-encoded OWL | 1,195 (41.3%) |
| With formal `Declaration(Class ...)` | 413 (14.3%) |
| Stub pages (<5 lines) | 106 (3.7%) |
| Archived in `.deleted/` | 28 |
| Unique term-id collisions | 22 |

**Domain distribution (`source-domain::`):**

| Domain | Count | % |
|---|---:|---:|
| mv (Metaverse) | 1,189 | 41% |
| ai | 345 | 12% |
| bc (Blockchain) | 332 | 11% |
| rb (Robotics) | 252 | 9% |
| tc (Telecom) | 58 | 2% |
| ngm | 44 | 2% |
| — (null / fallback to mv) | ~640 | 22% |

**Filename prefix families:** `BC-xxxx` (199), `AI-xxxx` or `ai-xxxx` (154), `rb-xxxx`/`RB-xxxx` (95), `TELE-xxxx` (28), `TC-xxxx` (4), long-tail natural-language titles (~1,600).

**Internal format heterogeneity** — three distinct writing styles coexist:
- Format A: minimal stubs ("Further enrichment pending")
- Format B: property-heavy structured OntologyBlocks
- Format C: Clojure + narrative hybrid

### workingGraph (private notes)

| Metric | Value |
|---|---:|
| Pages | 465 |
| `public:: true` | 193 (41.5%) |
| `public:: false` | 0 |
| Also in mainKnowledgeGraph | 444 (95.5%) |
| Journal entries (never ingested) | 1,028 |

**Public gating is effectively the only governance lever.** Metadata is otherwise sparse — most pages have only the `public::` flag. No `domain::`, `tags::`, or `type::` differentiation between public and private pages.

---

## What VisionClaw Actually Ingests

### Parser contract (github_sync_service.rs:765-793)

The ingestion filter is **binary**:

| Page content | Treatment |
|---|---|
| `public:: true` OR `public-access:: true` | `FileType::KnowledgeGraph` → creates GraphNode |
| `### OntologyBlock` only (no public flag) | `FileType::Ontology` → creates OwlClass only, no GraphNode |
| Neither | `FileType::Skip` — discarded |

### Properties honoured

Three tiers defined in `ontology_parser.rs:448-525`:
- **Tier 1** (11 props): `term-id`, `preferred-term`, `source-domain`, `status`, `public-access`, `last-updated`, `definition`, `owl:class`, `owl:physicality`, `owl:role`, `is-subclass-of`
- **Tier 2** (10 props): `alt-terms`, `version`, `quality-score`, `maturity`, `source`, `authority-score`, `scope-note`, `belongsToDomain`, plus relationship props
- **Tier 3** (optional): domain extensions

**Everything outside these tiers is silently discarded.**

### Critical parser bug

`extract_metadata_store()` at `knowledge_graph_parser.rs:251-271` collects all parsed properties into a HashMap but **returns an empty `MetadataStore`**. Every `key:: value` pair the parser recognises is parsed, then thrown away before reaching the Neo4j write. This explains why **`source_file` and `source_domain` are NULL on 100% of the 2,242 GraphNodes**.

### Axiom end-to-end coverage

| OWL construct | Parsed | Stored on OwlClass | Whelk reasons on | Produces GPU force |
|---|:---:|:---:|:---:|:---:|
| `SubClassOf` | ✓ | ✓ | ✓ | ✓ attraction 0.5× |
| `DisjointWith` | ✓ | ✓ | ✗ (drops from reasoner) | ✓ repulsion 2.0× |
| `EquivalentTo` | ✓ | ✓ | ✓ (CustomReasoner) | ✓ colocation 1.5× |
| `has-part`, `is-part-of`, `requires`, `depends-on`, `enables`, `relates-to`, `bridges-to`, `bridges-from` | ✓ | ✓ | ✗ | ✗ dead code |
| `ObjectPropertyAssertion` | ✓ | ✓ | ✗ "not EL Tbox" | ✗ |
| `FunctionalProperty`, `InverseFunctionalProperty`, `TransitiveProperty`, `SymmetricProperty` | ✓ | ✓ | ✗ | ✗ |

The corpus has **zero `disjointWith` axioms** anyway, so the strongest semantic force (2.0× repulsion) is unused in practice.

### IRI → node lookup failure

`ontology_pipeline_service.rs:259-265` looks up `owl_class_iri` on GraphNodes to apply semantic constraints. If no matching node exists, **the axiom is silently skipped with `continue`**. Result:

- 1,539 of 2,242 (68.7%) GraphNodes are `ontology_node` type with IRIs
- 703 (31.3%) are `page` type — these have **no IRI alignment**, so no ontology-derived force ever applies to them
- An estimated **30-50% of parsed axioms produce zero GPU constraints** because their subject/object IRIs don't match any GraphNode

### Node ID scheme

`knowledge_graph_parser.rs:293-305` uses `std::collections::hash_map::DefaultHasher` (NOT the FNV-1a referenced in earlier docs) to hash page names to u32 IDs. Collisions silently merge — two pages with colliding hashes become the same node in Neo4j. No detection, no logging.

---

## Assumption Gaps

### Code expects vs. data reality

| Code assumes | Data actually has |
|---|---|
| 8 domains (`domain_to_color` at `neo4j_adapter.rs:297-308`) | 6 domains in meaningful counts; `uk-regional` and `dt` have ≤2 pages each |
| Uppercase domain strings | Data stores lowercase (`ai`, `bc`, `rb`) |
| Canonical IRI scheme | Ad-hoc per-file IRIs (`http://example.org/...`, `mv:Agent`, bare names) |
| `node_type` string match (case-insensitive but exact) | Strings include "owl_class" vs "OWLClass" vs "ontology_node"; ≥15% of ontology nodes will default to Knowledge classification |
| Whelk returns disjointness and equivalence inferences | Whelk only returns `named_subsumptions()` — disjointness/equivalence come from parsed structures only |
| Well-formed wikilinks resolve to existing pages | 15–30% of wikilinks are unresolved in the sample |
| FileMetadata links to GraphNode | Zero cross-linking edges — they're parallel registries |
| `extract_metadata_store` returns populated properties | Returns empty — properties discarded |

### Invisible failures

The pipeline has **no validation warnings**. Pages with missing Tier 1 fields save silently. Axioms with unresolved IRIs are skipped silently. Stubs become nodes silently. Collision-merged node IDs are silent. This makes the pipeline appear to work while losing 33% of the data and 30-50% of the axioms.

---

## Actionable Suggestions (Ranked by Impact/Effort Ratio)

### Tier A — high impact, low effort (ship first)

**A1. Fix the empty MetadataStore bug** — `knowledge_graph_parser.rs:251-271`. One function that currently returns an empty store despite parsing every property. Fixing this populates `source_file`, `source_domain`, and all Tier 1/2 properties onto GraphNodes, immediately unlocking domain filtering, source attribution, and freshness gating. Cost: ~15 lines.

**A2. Add ingestion-loss instrumentation** — log the reason every source page fails to become a GraphNode. Currently 626 files reach FileMetadata but never become nodes with zero visibility. Add `warn!` logs at each rejection point (empty body, parser error, filter skip). Cost: ~30 lines across github_sync_service + parsers.

**A3. Normalize domain case at ingestion** — downcase `source-domain::` values on read and align `domain_to_color` to lowercase. Fixes the silent domain filter mismatch. Cost: ~5 lines.

**A4. Validate & log wikilink resolution failures** — during parse, check each `[[Target]]` against known page names; log warnings for unresolved links. Doesn't need to block ingestion but gives a clean report of broken references. Cost: ~40 lines, one-off script runs in CI.

**A5. FileMetadata↔GraphNode link** — create a `MENTIONED_IN` or `EXTRACTED_FROM` edge at ingestion time. Enables source traceability and correct incremental-sync cleanup. Cost: ~20 lines in github_sync_service.

### Tier B — high impact, medium effort

**B1. Enable Whelk disjointness and equivalence inference** — currently only `named_subsumptions()` is extracted. Add wrappers that query `assert_result` for DisjointWith and EquivalentTo inferences. This has zero cost in corpus changes but unlocks stronger semantic forces for future ontologies that use them. Cost: ~80 lines in whelk_inference_engine.rs.

**B2. Design and publish a canonical IRI scheme** — one rule, e.g. `https://visionclaw.dev/{domain}/{snake_case_id}` where `domain ∈ {ai, bc, rb, mv, tc, ngm}` and the ID is derived from filename. Add a `build_iri()` helper in one place and use it everywhere. This makes cross-graph equivalence possible. Cost: ~50 lines + migration script.

**B3. Map the 8 orphaned relationship types to GPU forces** — `has-part`, `is-part-of`, `requires`, `depends-on`, `enables`, `relates-to`, `bridges-to`, `bridges-from` are parsed and stored but produce no forces. Either (a) wire them through to ontology_pipeline_service with tunable magnitudes, or (b) document them as knowledge-only metadata and remove from parser to reduce ambiguity. Cost: ~150 lines, needs design decision.

**B4. Three-population classification hardening** — `classify_node_population()` should log at every fallback to Knowledge. Add telemetry for how many nodes per type fall through. Also fail loudly on empty `node_type`. Cost: ~20 lines.

**B5. Runtime dashboard — corpus health page** — a simple HTML page or API endpoint showing: total pages, ingested count, rejected-with-reason breakdown, stub count, orphan-edge count, Whelk axiom count. This becomes the canonical "is my ingestion healthy?" view. Cost: one handler + template, ~200 lines.

### Tier C — corpus-side hygiene (owner-only work)

**C1. Deduplicate the 22 term-id collisions** — automated grep + manual review. Cost: half a day.

**C2. Normalize filename prefixes** — pick one case convention per family (e.g. uppercase `AI-`, `BC-`, `RB-`, `TELE-`, drop `TC-`). Rename via script. Cost: half a day including PR review.

**C3. Resolve the `Agent.md` / `Agents.md` collision** — single page, canonical plural or singular, use aliases for the other. Cost: 30 minutes.

**C4. Stub consolidation** — 106 stub pages (<5 lines) should either be enriched, merged, or explicitly tagged `status:: stub` so downstream can filter. Cost: staged; can be prioritised by domain.

**C5. Seed the corpus with `disjointWith` axioms** — currently 0% use disjointness. A few strategic axioms (e.g. `Agent disjointWith Location`, `AIDomain disjointWith BlockchainDomain`) would unlock strong cluster repulsion in the GPU layout without changing defaults. Cost: 1 day of targeted axiom authoring.

**C6. Publish an index / root ontology file** — `ONTOLOGY_ROOT.md` with explicit `owl:Ontology` declaration, imports, namespace prefixes. Acts as bootstrap for new pages. Cost: 2 hours.

### Tier D — architectural (larger reshape)

**D1. Decouple working/main promotion** — currently workingGraph is 95% copy-mirrored to mainKnowledgeGraph regardless of `public::`. Consider an explicit promotion pipeline: working (private, thinking-in-progress) → gated (needs review) → mainKG (published). Cost: workflow design + small CI check.

**D2. Ontology-driven color mapping** — drop `domain_to_color` in favour of reading a YAML palette (domain → color) at startup. Adding a new domain doesn't require recompiling. Cost: ~40 lines.

**D3. Separate "page" from "ontology node"** — page nodes currently have no OWL grounding (703 of 2,242 = 31%). Consider auto-generating lightweight ontology classes for every page so every node participates in the semantic graph. Cost: ~100 lines of generation logic.

---

## Implications for Physics Calibration

The physics retune conversation we paused was based on the symptom ("nodes jumpy-fighting on X axis, flat on Y/Z"). The audit reveals the root cause is upstream:

1. **31% of nodes have no ontology grounding** — they're page-type with no owl_class_iri. They participate in force-directed layout but receive zero ontology-derived forces (no attraction to domain cluster, no repulsion from disjoint classes). They drift to whatever the raw spring/repulsion equilibrium is.
2. **`source_domain` is NULL on 100% of nodes** (parser bug A1). The `graphSeparationX` dual-graph split uses `node_population` not `source_domain`, but any future "domain-cluster-by-color" force would have no signal.
3. **Zero `disjointWith` axioms in the corpus** — the strongest GPU repulsion force (2.0×) is never activated. There is no cross-cluster separation pressure other than generic node-node repulsion.
4. **Only ~50-70% of axioms produce constraints** because IRI matching fails. The GPU physics is therefore layout-dominated, not ontology-dominated.

**Recommendation:** defer the physics calibration defaults PR until at least A1 lands. With `source_file`, `source_domain`, and properties actually persisted, the graph gains signal for domain-aware forces. Retuning defaults now would optimise for a data-blind layout and have to be redone.

---

## Next Steps — Proposed Order

1. Land A1 (empty MetadataStore fix) — unlocks almost everything downstream.
2. Rebuild + re-ingest to populate source_file / source_domain on all nodes.
3. Run A4 (wikilink validation report) to quantify broken references.
4. Owner decides on C1–C3 (one afternoon of corpus cleanup).
5. Ship A2, A3, A5, B4 as a single "ingestion observability" PR.
6. Ship B1 when someone has a day for Whelk internals.
7. **Then** retune physics defaults with the calibrated corpus.

This sequence makes the bottleneck the fastest-moving thing: ingestion fidelity first, data hygiene second, physics third.

---

## Appendix — Six contributing audit reports

1. [Main Knowledge Graph Corpus Audit](01-mainkg-corpus-audit.md) (to be written — see agent transcripts)
2. [Working Graph & Journals Audit](02-workinggraph-audit.md)
3. [Parser Analysis](03-parser-analysis.md)
4. [Ontology Bridge Analysis](04-ontology-bridge-analysis.md)
5. [Namespace & Taxonomy Design Analysis](05-namespace-analysis.md)
6. [Live Neo4j Post-Ingest Audit](06-postingest-audit.md)

All six ran as parallel Explore agents on 2026-04-18. Raw transcripts retained in the agent task outputs.
