# ADR-062: QE Graph Scaffolding — PRD / ADR / DDD Traceability via URN Identity

**Status:** Accepted
**Date:** 2026-05-01
**Author:** VisionClaw platform team
**Related:**
- PRD-QE-001 (integration quality engineering)
- PRD-006 (URI federation)
- ADR-013 (canonical URI grammar — agentbox)
- ADR-058 (MAD-to-agentbox migration)
- DDD BC20 (`ddd-agentbox-integration-context.md`)
- `src/uri/kinds.rs` (visionclaw URN kinds)
- `agentbox/management-api/lib/uris.js` (agentbox URN kinds, including `adr`, `prd`, `ddd`)

## Context

VisionClaw maintains 43 ADRs, 12 PRDs, and 10 DDD bounded-context documents.
These artefacts cross-reference each other in prose (e.g. ADR-061 cites PRD-007
and `ddd-binary-protocol-context.md`; PRD-QE-001 cites PRD-006, ADR-013, and
DDD BC20). The references are ad-hoc Markdown links — no machine-readable graph
connects them. This creates three concrete problems:

1. **Traceability gap.** When a QE agent validates a code change, it cannot
   programmatically determine which PRD requirement, ADR decision, and DDD
   bounded context the change implements. The agent falls back to grep-based
   heuristics that miss indirect chains (PRD requires ADR requires DDD context
   requires URN kind requires test).

2. **Orphan detection failure.** There is no way to discover that an ADR exists
   without a parent PRD, or that a DDD context references a URN kind that has no
   test coverage. The 2026-04-27 PRD-QE-001 audit found 10+ surfaces with zero
   or disabled tests — a symptom of untraceable requirements.

3. **Dual-namespace blind spot.** Two URI namespaces coexist
   (`urn:visionclaw:*` with 6 kinds in `src/uri/kinds.rs`;
   `urn:agentbox:*` with 18 kinds in `agentbox/management-api/lib/uris.js`).
   Agentbox already defines `adr`, `prd`, and `ddd` as first-class URI kinds
   (ADR-013, `KINDS` object in `uris.js`), but VisionClaw has never minted URNs
   for its own governance artefacts. The existing kinds sit unused.

The 19-agent QE fleet (PACT principles: Proactive, Autonomous, Collaborative,
Targeted) operates on code and test artefacts but lacks a structural graph of
the governance artefacts those tests are supposed to validate.

## Decision

### D1 — Assign URN identity to every governance artefact

Each PRD, ADR, and DDD bounded-context document receives a URN minted through
the agentbox URI grammar (ADR-013, R3 stable-on-identity rule):

```
urn:agentbox:prd:001                   → PRD-001-pipeline-alignment.md
urn:agentbox:prd:qe-001               → PRD-QE-001-integration-quality-engineering.md
urn:agentbox:adr:062                   → this document
urn:agentbox:ddd:bc20                  → ddd-agentbox-integration-context.md
urn:agentbox:ddd:bc10                  → ddd-binary-protocol-context.md
```

These are not content-addressed (the `KINDS` table in `uris.js` already declares
`adr`, `prd`, `ddd` as `contentAddressed: false`). The `<local>` segment is the
artefact's canonical short identifier (e.g. `062`, `qe-001`, `bc20`). No
`<scope>` segment — these are project-wide, not owner-scoped
(`ownerScope: false`).

### D2 — Define typed edge relations stored in ruvector memory

Three edge types capture the governance graph:

| Edge Type | Source Kind | Target Kind | Semantics |
|---|---|---|---|
| `REQUIRES_ADR` | `prd` | `adr` | PRD requirement is realised by this ADR decision |
| `IMPLEMENTS_CONTEXT` | `adr` | `ddd` | ADR decision operates within this bounded context |
| `USES_URN_KIND` | `ddd` | URN kind slug | DDD context defines or consumes this URN kind |

Edges are stored in ruvector memory under the `qe/edges` namespace as JSON
entries. Each edge key encodes the relationship:

```
Namespace: qe/edges
Key:       REQUIRES_ADR:prd:007→adr:061
Value:     {"source":"urn:agentbox:prd:007","target":"urn:agentbox:adr:061",
            "type":"REQUIRES_ADR","confidence":1.0,
            "evidence":"ADR-061 §Related cites PRD-007"}
```

Node metadata is stored under `qe/nodes`:

```
Namespace: qe/nodes
Key:       adr:062
Value:     {"urn":"urn:agentbox:adr:062","title":"QE Graph Scaffolding",
            "status":"accepted","date":"2026-05-01",
            "file":"docs/adr/ADR-062-qe-prd-adr-ddd-graph-scaffolding.md",
            "kind":"adr"}
```

### D3 — Bridge to urn:visionclaw:concept for concept-level tracing

VisionClaw's `urn:visionclaw:concept:<domain>:<slug>` kind (R3, stable-on-identity)
represents domain concepts in the knowledge graph. Governance artefacts that
define or constrain a domain concept receive a fourth edge type:

| Edge Type | Source Kind | Target Kind | Semantics |
|---|---|---|---|
| `DEFINES_CONCEPT` | `adr` or `ddd` | `concept` | Governance artefact defines this domain concept |

Example:

```
DEFINES_CONCEPT:adr:061→concept:binary-protocol:wire-frame
```

This bridges the governance graph into VisionClaw's existing concept namespace
without requiring changes to `src/uri/kinds.rs`. The `concept` kind already
exists. The BC20 anti-corruption layer (when implemented per
`ddd-agentbox-integration-context.md`) maps `urn:agentbox:adr:*` to the
VisionClaw-side representation at the federation boundary.

### D4 — QE fleet integration via gate queries

QE agents query the graph through ruvector memory search to enforce traceability
gates:

1. **`qe-requirements-validator`** — Given a changed file, traces upward:
   file path -> DDD context -> ADR -> PRD. Fails the gate if any link in the
   chain is missing.

2. **`qe-coverage-analyzer`** — For each `USES_URN_KIND` edge, asserts that
   the referenced URN kind has at least one test in `tests/contract/` or
   `tests/unit/`. This directly addresses the PRD-QE-001 finding that URI
   grammar tests exist only on the agentbox side.

3. **`qe-api-contract-validator`** — For each `IMPLEMENTS_CONTEXT` edge,
   validates that the DDD context's aggregate invariants are covered by contract
   tests.

4. **`qe-fleet-commander`** — Orchestrates the above three agents in a
   hierarchical coordination pattern:
   `fleet-commander -> [requirements-validator, coverage-analyzer, contract-validator] -> quality-gate`.

Gate queries use ruvector memory search with namespace filtering:

```javascript
// Find all ADRs required by a PRD
mcp__claude-flow__memory_search({
  query: "REQUIRES_ADR prd:006",
  namespace: "qe/edges",
  limit: 50
})

// Find all URN kinds used by a DDD context
mcp__claude-flow__memory_search({
  query: "USES_URN_KIND ddd:bc20",
  namespace: "qe/edges",
  limit: 20
})
```

### D5 — Neo4j-compatible node schema for optional visualisation

Graph nodes stored under `qe/nodes` follow a schema compatible with VisionClaw's
Neo4j graph model, enabling optional projection into the knowledge graph:

```cypher
// Node projection (if desired)
CREATE (a:GovernanceArtefact {
  urn: "urn:agentbox:adr:062",
  title: "QE Graph Scaffolding",
  kind: "adr",
  status: "accepted",
  date: date("2026-05-01"),
  file_path: "docs/adr/ADR-062-qe-prd-adr-ddd-graph-scaffolding.md"
})

// Edge projection
MATCH (p:GovernanceArtefact {urn: "urn:agentbox:prd:007"})
MATCH (a:GovernanceArtefact {urn: "urn:agentbox:adr:061"})
CREATE (p)-[:REQUIRES_ADR {confidence: 1.0, evidence: "..."}]->(a)
```

Node `kind` values (`adr`, `prd`, `ddd`) map directly to the agentbox URI kind
slugs. The Neo4j projection is optional — the authoritative store is ruvector
memory. Projection is a one-way sync performed by a scheduled job, not a
live mirror.

### D6 — Graph population is incremental and frontmatter-driven

The initial graph is populated by scanning existing artefact files for their
`Related:`, `Pairs with:`, and `Supersedes:` header fields. Future artefacts
include a structured frontmatter block:

```yaml
---
urn: urn:agentbox:adr:062
requires_prd: [prd:qe-001, prd:006]
implements_context: [ddd:bc20]
uses_urn_kind: [adr, prd, ddd, concept]
defines_concept: [concept:governance:qe-graph]
---
```

A `scripts/populate-qe-graph.sh` scanner extracts these fields and writes edges
to ruvector memory. The scanner is idempotent — re-running it updates existing
entries without duplication.

## Consequences

### Positive

- Every QE gate query can trace from code change to PRD requirement in a single
  ruvector memory search, eliminating grep-based heuristics.
- The 19-agent QE fleet gains a structural backbone: `qe-requirements-validator`
  becomes a graph traversal rather than a filename pattern match.
- Orphan ADRs (decisions without a parent PRD) and orphan DDD contexts (contexts
  with no implementing ADR) become discoverable via graph completeness checks.
- URN kinds that lack test coverage are surfaced by `qe-coverage-analyzer`
  automatically, directly addressing the PRD-QE-001 audit findings.
- No new URI kinds are introduced — `adr`, `prd`, and `ddd` already exist in
  the agentbox `KINDS` table. This decision activates infrastructure that was
  already built and paid for.
- The `urn:visionclaw:concept:*` bridge enables governance artefacts to appear
  alongside domain concepts in the knowledge graph, providing architectural
  visibility to stakeholders who use the graph viewer.
- Neo4j projection is optional and one-way, so the governance graph does not
  create a hard dependency on the graph database.

### Negative

- Frontmatter must be added to existing artefacts over time. The initial
  population scanner handles the backfill, but newly authored documents must
  include the frontmatter to remain traceable. This is a process burden.
- The `qe/edges` and `qe/nodes` namespaces add to ruvector memory usage.
  At the current scale (43 ADRs + 12 PRDs + 10 DDD contexts + edges), this is
  approximately 200-300 entries — negligible against the 1.17M+ row
  `memory_entries` table.
- QE gate failures on missing traceability links may initially produce noise
  until the backfill is complete. A 30-day grace period with warnings (not
  failures) is recommended.

### Neutral

- The decision does not change the existing agentbox or visionclaw URI minting
  code. Both `uris.js` and `src/uri/kinds.rs` are unchanged.
- The BC20 anti-corruption layer mapping between the two namespaces is planned
  but not yet implemented. This ADR does not depend on BC20 completion — the
  `DEFINES_CONCEPT` edge type works as a soft cross-namespace reference until
  BC20 lands.

## Options Considered

### Option 1: Neo4j-native governance graph (rejected)

Store governance nodes and edges directly in Neo4j as first-class graph entities.

- **Pros**: Native Cypher queries, immediate visualisation, existing Neo4j
  infrastructure.
- **Cons**: Creates a hard dependency on Neo4j for QE gates. QE agents run in
  CI where Neo4j may not be available. Mixes governance metadata with runtime
  graph data, complicating backup/restore. Violates the principle that
  ruvector memory is the coordination substrate for agent work (CLAUDE.md).

### Option 2: Standalone SQLite graph (rejected)

Ship a `qe-graph.sqlite` file with nodes/edges tables.

- **Pros**: Zero infrastructure dependency, portable, queryable with standard
  SQL.
- **Cons**: Not accessible to the 19-agent QE fleet without a new MCP adapter.
  Duplicates the graph-storage problem that ruvector memory already solves.
  No semantic search capability. Diverges from the project's established
  memory-first architecture.

### Option 3: Ruvector memory with URN identity (accepted)

Use existing agentbox URI kinds and ruvector memory namespaces.

- **Pros**: Zero new infrastructure. Leverages `adr`/`prd`/`ddd` kinds that
  already exist in `uris.js`. Accessible to all QE agents via
  `mcp__claude-flow__memory_*` tools. Semantic search enables fuzzy
  discovery. Neo4j projection is optional.
- **Cons**: Graph queries are namespace-scoped memory searches, not native
  graph traversals. Adequate at current scale; may need migration to a
  dedicated graph store if governance artefacts exceed ~1000 nodes.

## Memory Namespace Layout

```
qe/nodes                    — Governance artefact metadata
  adr:061                   → {urn, title, status, date, file, kind}
  prd:007                   → {urn, title, status, date, file, kind}
  ddd:bc10                  → {urn, title, date, file, kind}

qe/edges                    — Typed relationships
  REQUIRES_ADR:prd:007→adr:061
  IMPLEMENTS_CONTEXT:adr:061→ddd:bc10
  USES_URN_KIND:ddd:bc10→binary-protocol
  DEFINES_CONCEPT:adr:061→concept:binary-protocol:wire-frame

qe/gates                    — QE gate results (written by fleet agents)
  gate:coverage:adr:061     → {pass: true, tested_kinds: ["binary-protocol"], timestamp}
  gate:traceability:prd:007 → {pass: true, chain_depth: 3, timestamp}

qe/stats                    — Aggregate graph statistics
  summary                   → {nodes: 65, edges: 120, orphan_adrs: 2, untested_kinds: 1}
```

## References

- Agentbox URI grammar: `agentbox/management-api/lib/uris.js` lines 71-90
  (KINDS table with `adr`, `prd`, `ddd` definitions)
- VisionClaw URI kinds: `src/uri/kinds.rs` (6 kinds including `Concept`)
- QE fleet specification: `.claude/skills/agentic-quality-engineering/SKILL.md`
  (19 agents, PACT principles)
- PRD-QE-001: Quality Engineering for VisionClaw/Agentbox Integration
  (the audit that motivated this decision)
- DDD bounded contexts: `docs/explanation/ddd-bounded-contexts.md`
  (10 contexts: BC1-BC10)
- Ruvector memory: 1.17M+ entries in `memory_entries` table
  (`ruvector-postgres:5432`)
