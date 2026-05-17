# ADR-048: Dual-Tier Identity Model — Bridging KG Notes and Ontology Classes

## Status

Implemented 2026-04-20

## Date

2026-04-18 (Proposed) · 2026-04-20 (Implemented)

## Implementation

- Service: `src/services/bridge_edge.rs` — scoring sigmoid, `surface`, `promote`
  (monotonic), `auto_expire` (3-day below 0.35).
- Hygiene: `src/services/orphan_retraction.rs` — 15-min sweep, stale
  `WikilinkRef` deletion and orphan private-stub cleanup.
- Migration: `queries/migrations/0042_bridge_to.cypher` — constraints, indexes,
  colocation backfill.
- Tests: `tests/bridge_edge_test.rs` — weights-sum-to-one, sigmoid shape,
  monotonic-invariant logic (`#[ignore]`-gated Neo4j integration for the
  on-wire confidence check).
- Model: `src/models/node.rs` (visibility/owner sovereign fields) and
  `src/models/graph_types.rs::classify_node_population` recognise both
  `KGNode` and `OntologyClass` populations.

### Signal weights (per ADR-049 migration-broker)

S1 wikilink 0.20 · S2 cooccurrence 0.15 · S3 OWL declaration 0.15 ·
S4 agent proposal 0.20 · S5 maturity 0.10 · S6 centrality 0.10 ·
S7 recency 0.05 · S8 authority 0.05  (Σ = 1.00). Sigmoid `k=12, bias=0.42`.
Surface threshold 0.60; expiry threshold 0.35 over 3 days.

## Related Documents

- Commit `b501942b1` — parser fix separating `public::` from `public-access::`
- `docs/design/2026-04-18-insight-migration-loop/02-bridge-theory.md`
- `docs/design/unified-pipeline-design.md`
- `config/domains.yaml`, `docs/reference/neo4j-schema-unified.md`

## Context

Until commit `b501942b1`, the Logseq parser conflated two orthogonal flags. The
fix now treats them as independent signals:

- **`public:: true`** — *user publication intent*. A human author asserts a note
  belongs to the public corpus. ~389 pages carry this flag.
- **`public-access:: true`** — an *OWL axiom property* authored inside
  OntologyBlocks. It declares an ontology term part of the shared vocabulary.
  ~2,058 auto-stubbed pages carry this flag with no `public:: true` — machine-
  generated skeletons with OWL metadata but no publication intent.

KG pages are *narrative nodes* in a personal/team graph; ontology classes are
*vocabulary nodes* in a shared OWL T-Box. Collapsing them caused ghost nodes,
invalid wikilink resolution, and bridge-agnostic physics.

### Three scenarios the model must handle

1. **KG note with outbound `[[Ontology Class Name]]` wikilink.** The author
   references a formal class from prose. The note is narrative; the class is
   vocabulary; the link is a candidate bridge.
2. **Ontology class label matches a KG note filename.** An ontology stub
   `Systems Thinking` co-exists with a user-published `Systems Thinking.md`.
   Both resolve to the same canonical IRI. Neither begat the other.
3. **KG note is promoted, then the author edits the markdown.** The narrative
   keeps evolving after an ontology class has been distilled from it. Editing
   the note body must not silently mutate promoted OWL axioms.

### Why a single node type fails

A unified `Concept` node with a status field breaks on three axes: promotion is
derivation not mutation, so shared state hides lineage (ADR-034); KG writes are
hourly while ontology change is PR-gated, forcing the slow tier to absorb the
fast tier's churn; and the two populations need different layout forces.

## Decision Drivers

- **Provenance**: every promotion records which KG note begat which class,
  when, by whom, with which agent signature (ADR-034).
- **Reversibility**: a wrongly promoted class must roll back without
  invalidating dependent axioms.
- **Scale tension**: KG flow is fast; ontology change is slow and PR-gated.
- **Agent safety**: agents *propose*, never *directly write* ontology.
- **Physics**: both tiers participate in layout; bridge edges must attract
  gravitationally.
- **Canonical addressing**: one IRI scheme across tiers so external consumers
  never branch on node class.

## Considered Options

### Option 1 — Single node type with lifecycle states

One `Concept` label; promotion mutates a `status` field in place.

- **Pros**: simplest schema; no bridge edges.
- **Cons**: loses provenance on mutation; collapses write-rate tiers; single
  physics population; reversion is destructive.

### Option 2 — Dual-tier with bridge edges (chosen)

`KGNode` and `OntologyClass` are distinct Neo4j labels. A `BRIDGE_TO` edge
carries the relationship and its state.

- **Pros**: provenance preserved as edge properties; native per-tier physics;
  reversion is an edge-label flip; promotion never mutates identity.
- **Cons**: two labels; Cypher must be bridge-aware; agents must check tier
  before proposing edits.

### Option 3 — Ontology as tag overlay

KG is the base graph; ontology is tag metadata on KG nodes.

- **Pros**: no new node type.
- **Cons**: the 2,058 auto-stubs without a KG counterpart have nowhere to live;
  OWL axioms need a node target; scenario 2 collapses; promotion workflow
  disappears entirely.

## Decision

**Option 2: dual-tier nodes with state-bearing bridge edges.**

### Node classes

| Field | `KGNode` | `OntologyClass` |
|---|---|---|
| Neo4j label | `KGNode` | `OntologyClass` |
| IRI scheme | `vc:{domain}/{slug}` | `vc:{domain}/{slug}` or external `bc:SomeClass` |
| `node_type` values | `page`, `knowledge_node`, `agent` | `ontology_node`, `owl_class`, `owl_individual`, `owl_property` |
| Physics population | `Knowledge` | `Ontology` |
| Source gating | `public:: true` | `public-access:: true` inside OntologyBlock |
| Write rate | High (markdown edits) | Low (PR + broker approval) |
| Agent write access | Direct (via file sync) | Forbidden — proposals only |

### Canonical IRI

Both tiers share one scheme: `vc:{domain}/{slug}` where `domain` comes from
`config/domains.yaml` and `slug` is deterministic from the filename. Frontmatter
`canonical-iri::` overrides the computed value. External imports (e.g. `bc:`)
keep their source IRI.

### Bridge edges

`BRIDGE_TO` is a directed edge from `KGNode` to `OntologyClass`:

```
(k:KGNode)-[b:BRIDGE_TO {
  kind: "candidate" | "promoted" | "revoked" | "colocated" | "rejected",
  confidence: f32,                  // 0.0 – 1.0
  created_at: DateTime,
  created_by: NostrPubkey,          // agent or human identity (ADR-034)
  provenance_bead_id: NostrEventId  // links to the audit bead
}]->(o:OntologyClass)
```

### Promotion is an edge state transition

A `KGNode` is never rewritten into an `OntologyClass`. The edge `kind` advances
through the state machine below; the `OntologyClass` exists independently and
becomes physics-anchored beside its KG origin once the edge flips to `promoted`.

### State machine for `BRIDGE_TO.kind`

```text
  "none" --- auto-detect OR agent-propose ---> "candidate"

  "candidate" --- broker approval + PR merged ---> "promoted"
  "candidate" --- broker rejection             ---> "rejected"

  "promoted"  --- broker rollback (ADR-049)    ---> "revoked"

  (system-generated, not broker-managed)
  "none"      --- IRI collision detected       ---> "colocated"
```

### Collision resolution

If both tiers resolve to the same canonical IRI, ingest auto-creates a
`BRIDGE_TO {kind: "colocated", confidence: 1.0, created_by: "system"}`.
Colocated edges are declarative co-existence markers, not broker proposals.

### Neo4j schema additions

```cypher
CREATE CONSTRAINT ontology_class_iri IF NOT EXISTS
  FOR (o:OntologyClass) REQUIRE o.canonical_iri IS UNIQUE;

CREATE CONSTRAINT kg_node_iri IF NOT EXISTS
  FOR (k:KGNode) REQUIRE k.canonical_iri IS UNIQUE;

CREATE INDEX bridge_to_kind IF NOT EXISTS
  FOR ()-[r:BRIDGE_TO]-() ON (r.kind);

CREATE INDEX bridge_to_created_at IF NOT EXISTS
  FOR ()-[r:BRIDGE_TO]-() ON (r.created_at);
```

### Rust model changes

Add `OntologyClass` as an aggregate root in the ontology context and `Bridge` as
a value object. `OntologyClass` carries `{canonical_iri, labels, owl_kind,
source: OntologySource (AutoStub | Imported | Promoted { from }), public_access}`.
`Bridge` carries `{from: KGNodeId, to: OntologyClassId, kind: BridgeKind,
confidence: f32, created_at, created_by: NostrPubkey, provenance_bead_id:
Option<NostrEventId>}`. The graph context holds read-models of both.

## Compliance with adjacent ADRs

- **ADR-027**: graph views filter by label to project single-tier or bridged
  subgraphs; pod queries unchanged.
- **ADR-028**: SPARQL PATCH targets `OntologyClass` only; `KGNode` PATCH is
  rejected at the adapter.
- **ADR-030**: agents write proposals to their pod; the broker promotes a
  proposal to `BRIDGE_TO(candidate)`, never directly mutating `OntologyClass`.
- **ADR-034**: every bridge state transition emits a bead;
  `Bridge.provenance_bead_id` is the cryptographic anchor.
- **ADR-049**: defines `promoted → revoked` and dependent-axiom impact analysis.
  Out of scope here.

## Consequences

### Positive

- Each node stays in its native tier; provenance survives promotion because
  nodes are never rewritten.
- Physics renders both populations with tier-specific forces; bridge edges
  create visible attraction between narrative and distilled vocabulary.
- Promotion and reversion are cheap edge-label flips, not destructive ops.
- Agent writes stay confined to KG and proposal pods; the ontology tier is
  workflow-protected.
- External consumers resolve `vc:{domain}/{slug}` identically across tiers.

### Negative

- Two Neo4j labels; "concept" queries must `MATCH (n) WHERE n:KGNode OR
  n:OntologyClass`. Mitigated by a view layer.
- Agents must check tier before proposing edits; tooling must surface this.
- GPU physics gains a new bridge-attraction force.

### Neutral

- SHA1 delta sync unchanged (content-hash based).
- `FileMetadata` unchanged.
- Nostr event schema additively gains a `bridge_id` tag.

## Migration plan

1. Re-ingest the Logseq corpus with the `b501942b1` parser fix; drop nodes
   previously gated by the conflated flag.
2. Classify remaining ontology-only pages (no `public:: true`, with
   `public-access:: true`) as `OntologyClass`.
3. For every `(KGNode, OntologyClass)` pair sharing a canonical IRI, create
   `BRIDGE_TO {kind: "colocated"}`.
4. For every `KGNode` with an outbound wikilink resolving to an `OntologyClass`
   label, create `BRIDGE_TO {kind: "candidate", confidence: 0.7, created_by:
   "system"}`. Broker reviews the backlog.
5. Emit one bead per auto-created bridge (ADR-034).
6. Backfill the Rust domain model and update the physics pipeline.

## Open questions

1. Should broker-confirmed `colocated` edges receive a stronger marker, or is
   explicit confirmation implicit in long-lived co-existence?
2. What confidence threshold auto-queues a candidate for broker review vs
   silently recording it? Proposal: `>= 0.85` auto-queues.
3. Schema allows multiple `KGNode → OntologyClass` bridges fan-in from different
   notes. Do we need uniqueness on `(from, to)` or allow multiple edges keyed on
   `created_by`?
4. When a KG note is deleted post-promotion, the `OntologyClass` survives but
   the edge is orphaned. New kind `"orphaned-source"`, or tombstone property?
5. Is `BridgeKind` surfaced in the broker workbench as a first-class object or
   only in the proposal detail view? Ties to ADR-041.

## References

- `src/graph/ingest.rs`, `src/graph/bridge.rs` (new), `src/ontology/class.rs`
- `config/domains.yaml`, `docs/reference/neo4j-schema-unified.md`
- Commit `b501942b1` — parser fix separating `public::` and `public-access::`
