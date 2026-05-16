# ADR-11 — Persistence Strategy Migration: Neo4j → Oxigraph + SQLite

Status      : Proposed
Date        : 2026-05-16
Supersedes  : the implicit Neo4j-as-backing-store decision baked into
              every `neo4j_*` adapter under `src/adapters/`
Related     : ADR-08 (Ontology & Graph Data — depends on this ADR's
              named-graph layout), ADR-05 (Settings & Control Panel —
              depends on this ADR's SQLite schema), PRD-11

## Context

VisionFlow durable state is currently held in a Neo4j 5.x Community
container reached over Bolt from three adapters totalling **4,977
lines** of Rust:

| File                                  | Lines | Owns                              |
|---------------------------------------|-------|-----------------------------------|
| `src/adapters/neo4j_adapter.rs`       | 1,484 | Connection pool, BoltType maps    |
| `src/adapters/neo4j_graph_repository.rs` | 756  | `GraphRepository` impl            |
| `src/adapters/neo4j_ontology_repository.rs` | 1,600 | `OntologyRepository` impl       |
| `src/adapters/neo4j_settings_repository.rs` | 1,137 | `SettingsRepository` impl       |

The cost of this choice is documented in PRD-11 §2 (operations,
licensing, schema drift, test isolation, reasoning round-trip,
query expressiveness, settings/graph coupling, per-user settings,
backup).

The 371-commit delta between baseline `41979d33e` and `main@HEAD`
includes four Cypher migrations (0042–0045) which between them:

- Introduce the dual-tier identity model (KGNode vs OntologyClass)
- Add render-time properties to ontology nodes for the GPU layout
- Place the ontology tier on a Fibonacci-sphere shell with 10× mass
- Reconcile a `:OwlClass` ↔ `:OntologyClass` label rename that
  produced silent orphan stubs in production data

Each of these is a graph-data-model decision that we must carry
forward, because they encode capability (PRD-08 §3). None of them
is dependent on Neo4j as an *engine*; they are dependent on having
*a triple- or quad-store with type membership and named relationships*.
That is precisely what Oxigraph provides.

## Decision

### D1. Two stores, one binary

VisionFlow embeds **Oxigraph** (W3C SPARQL 1.1 + Update, RocksDB
backend, Rust-native, MIT licensed) for ontology and graph data, and
**SQLite** (via `rusqlite` with the `bundled` feature) for settings.
Both stores are opened in-process by `webxr` at startup, against a
data directory passed by `--data-dir` or the `VISIONFLOW_DATA_DIR`
environment variable.

```
<data-dir>/
├── oxigraph/                # Oxigraph dataset (RocksDB column families)
│   ├── CURRENT
│   ├── MANIFEST-000001
│   ├── *.sst
│   └── …
└── settings.sqlite3         # SQLite database (WAL + SHM siblings)
```

There is **no separate Neo4j container** in the destination
architecture, no Bolt port exposed by the deployment, and no JVM
in any image VisionFlow produces.

### D2. Named-graph segregation for the triple store

Oxigraph supports **named graphs** natively (it is a quad-store). We
segregate VisionFlow's three logical graphs by IRI:

| Named graph                                       | Contents                                |
|---------------------------------------------------|-----------------------------------------|
| `<urn:visionflow:graph:knowledge>`                | KGNode entities + KG edges              |
| `<urn:visionflow:graph:ontology:assert>`           | Asserted OntologyClass / OwlProperty / OwlAxiom |
| `<urn:visionflow:graph:ontology:inferred>`        | Whelk-derived inferred axioms           |
| `<urn:visionflow:graph:agent>`                    | Agent telemetry (Section 7)             |
| default graph                                     | Cross-graph `BRIDGE_TO` quads + schema  |

This replaces the current Neo4j approach of node-label-as-type
(`:KGNode`, `:OntologyClass`) with named-graph-as-type. The benefits:

- **No label drift.** RDF has no labels; the named graph IS the
  domain. The `:OwlClass` ↔ `:OntologyClass` rename bug (migration
  0045) cannot recur because there is no `class label` concept to
  forget to write.
- **Per-graph invalidation.** `CLEAR GRAPH
  <urn:visionflow:graph:ontology:inferred>` is one statement.
  Asserted ontology and inferred ontology are physically separated.
- **Default-graph queries see the union.** A SPARQL query with no
  `FROM`/`FROM NAMED` clause queries the union — i.e. inferred and
  asserted axioms are visible together to clients with no
  per-query annotation cost.

### D3. IRI minting

Every node and edge has a canonical IRI minted from a small set of
schemes:

```
vc:kg/<slug>                   # KGNode (knowledge tier)
vc:onto/<slug>                 # OntologyClass (ontology tier)
vc:agent/<pubkey-hex>/<agent>  # Agent telemetry node
vc:edge/<sha256-12>            # Edge (sha256 of source||predicate||target)
vc:bridge/<sha256-12>          # BRIDGE_TO edge
```

The `vc:` prefix expands to `https://visionflow.dreamlab/ns/` in
emitted RDF. Slug derivation is the same algorithm as today (NFKC
normalisation, lowercase, non-alnum to dash, collapse, trim). For
content addressing of edges, sha256 of the canonical
`subject||predicate||object` string ensures idempotent reapplication
of the same edge set.

### D4. Position storage decoupling

Live physics positions live in `GraphStateActor` RAM (Section 1).
They are NOT round-tripped through Oxigraph on every tick. Periodic
snapshots — currently every 60 seconds of wall-clock — write
`:hasX/:hasY/:hasZ` literal triples per node atomically as part of a
single SPARQL Update transaction. The previous Neo4j integration
suffered the `cbac7532a` bug because positions were being clobbered
on incremental graph upload; the new flow is one-way (RAM → Oxigraph
snapshot, never Oxigraph → RAM on the hot loop). Cold start does
read positions from Oxigraph so that layout doesn't restart from
random on every binary restart.

### D5. SQLite settings schema

This ADR is the sole authority for every table in `settings.sqlite3`,
including the audit log catalogued from Section 6. ADR-05 (Settings &
Control Panel) and ADR-06 (Auth & Security) defer to this section for
all storage and operational concerns; those sections own only their
domain types (Section 5: `AppFullSettings`, `PhysicsSettings`;
Section 6: audit event semantics) persisted via the unchanged
`SettingsRepository` trait surface (17 methods).

A single table covers all 17 SettingsRepository methods:

```sql
CREATE TABLE IF NOT EXISTS settings (
    key            TEXT    NOT NULL,
    owner_pubkey   TEXT,                       -- NULL = global
    value          TEXT    NOT NULL,           -- JSON-encoded SettingValue
    description    TEXT,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (key, owner_pubkey)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS settings_owner_idx
    ON settings(owner_pubkey, key);

CREATE TABLE IF NOT EXISTS physics_profiles (
    profile_name   TEXT    NOT NULL,
    owner_pubkey   TEXT,
    settings_json  TEXT    NOT NULL,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (profile_name, owner_pubkey)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS schema_migrations (
    id          TEXT PRIMARY KEY,
    applied_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Audit log (semantics owned by ADR-06 §D6; storage owned here).
CREATE TABLE IF NOT EXISTS audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at     INTEGER NOT NULL DEFAULT (unixepoch()),
    actor_pubkey    TEXT,                      -- NULL = anonymous / system
    request_method  TEXT    NOT NULL,
    request_path    TEXT    NOT NULL,
    status_code     INTEGER NOT NULL,
    detail_json     TEXT
);

CREATE INDEX IF NOT EXISTS audit_log_occurred_idx
    ON audit_log(occurred_at);
CREATE INDEX IF NOT EXISTS audit_log_actor_idx
    ON audit_log(actor_pubkey, occurred_at);

-- Monthly archive tables (`audit_log_archive_yyyymm`) are created on
-- rotation by the audit-log adapter and follow the same column layout
-- as `audit_log`. See ADR-06 §D6 for audit event semantics and the
-- retention policy that drives rotation.
```

- `WITHOUT ROWID` keeps the table B-tree-organised by primary key.
- `value` is a JSON-encoded `SettingValue` (the existing serde tag
  shape). SQLite's JSON1 extension allows ad-hoc inspection without
  schema rework.
- The `schema_migrations` table tracks which SQL migrations the
  database has applied. It is independent of the per-document
  `AppFullSettings::schema_version` field owned by ADR-05; the table
  answers "which migrations did this DB receive?", while the embedded
  field answers "which shape is this row in?". The two counters are
  distinct by name and tested for non-conflation.
- Per-user resolution is layered in the adapter: a read for key K
  by user U returns the row with `(K, U)` if present, else
  `(K, NULL)`. Writes always specify the pubkey explicitly.
- The owner pubkey is sourced from the per-request auth context
  (NIP-98 verified), not from a new method parameter. The
  SettingsRepository trait surface (17 methods) is unchanged. The
  adapter holds a thread-local or task-local `current_owner_pubkey`
  set by middleware.
- The audit-log adapter co-locates with the settings adapter so all
  tables share one connection pool, one pragma set, and one backup
  procedure (D10). See ADR-06 §D6 for audit event semantics and the
  retention policy.

### D6. Constraints expressed in code, not in the store

Neo4j's `CREATE CONSTRAINT … REQUIRE … IS UNIQUE` is expressed in
the adapter as pre-write validation:

- IRI uniqueness for `OntologyClass` and `KGNode` is enforced by a
  SPARQL `ASK` against the named graph before the insert. Failure
  raises `OntologyRepositoryError::InvalidData`.
- Settings primary key is enforced by SQLite directly (it has
  proper constraints).
- We deliberately do **not** adopt SHACL validation in-process for
  this sprint. SHACL has a non-trivial runtime cost and adds a
  dependency (`shacl-rs` is immature; Jena is JVM). Pre-write
  validation in the adapter is sufficient for the constraint set
  Neo4j currently enforces (5 UNIQUE constraints across the
  migrations).

### D7. Cypher → SPARQL Update translation

Each of the 4 migrations (0042–0045) gets a one-to-one SPARQL Update
file under `queries/migrations/sparql/`. The destination file names
keep the migration ID for traceability:

```
queries/migrations/sparql/
├── 0042_bridge_to.rq
├── 0043_render_ontology_tier.rq
├── 0044_ontology_tier_layout.rq
└── 0045_owlclass_to_ontologyclass.rq
```

Translation principles:

1. **Cypher `MERGE` → SPARQL `INSERT { … } WHERE { FILTER NOT
   EXISTS { … } }`.** SPARQL has no native upsert; the FNE pattern
   is the standard idiom and is performant on indexed quads.
2. **Cypher labels → `rdf:type` triples.** `(k:KGNode)` becomes
   `?k a vc:KGNode .` in the knowledge named graph.
3. **Cypher properties → datatype triples.** `k.iri` becomes
   `?k vc:iri "iri-value" .` (literal or IRI depending on the
   property; IRI for `iri`, literal for `label`, `confidence`,
   etc.).
4. **Cypher relationship types → predicate IRIs.** `[:BRIDGE_TO]`
   becomes the predicate `vc:bridgeTo`. Relationship properties
   become triples on a reified edge IRI (`vc:edge/<sha256-12>`).
5. **`datetime()` → `xsd:dateTime` literal.** Generated at
   migration-apply time, embedded in the file by a templating step
   (we do not use SPARQL's `NOW()` because some migrations want
   deterministic timestamps for replay tests).
6. **Cypher `CREATE INDEX` → no-op.** Oxigraph maintains SPO, POS,
   OSP, SPOG, POSG indexes automatically; explicit index DDL is
   neither necessary nor available.
7. **Cypher `CREATE CONSTRAINT` → adapter-side ASK validation.**
   See D6. Constraint statements are absent from the SPARQL
   migration files; they live in the adapter test suite as
   invariant assertions instead.

Detailed translation for each migration is in the DDD document
(DDD-11 §SPARQL translation walkthrough). The intent here is that
the migration mechanism survives unchanged: a numbered, idempotent
script registered in `schema_migrations` (now an SQLite table
rather than a Neo4j `:SchemaMigration` node).

### D8. One-shot migration tool, no live dual-write

A standalone binary lives at
`tools/migrate-neo4j-to-oxigraph/src/main.rs` (a cargo workspace
member, not a feature of `webxr`). Its inputs are a Neo4j Bolt URL
plus credentials and an Oxigraph data directory. Its single mode
of operation:

1. Open the Neo4j connection.
2. Stream all nodes via `MATCH (n) RETURN n, labels(n)`.
3. Translate each node to a set of triples according to D2/D3 and
   write into Oxigraph in 10k-quad batches.
4. Stream all relationships via `MATCH ()-[r]->() RETURN r,
   startNode(r), endNode(r), type(r)`.
5. Translate each relationship to triples (reified edge IRI + one
   predicate triple) and write.
6. Apply the four SPARQL migrations from
   `queries/migrations/sparql/` in numeric order.
7. Print a triple-count parity table grouped by named graph and
   predicate.
8. Exit.

Re-running the tool against a non-empty directory wipes the
Oxigraph dataset (after a confirmation flag) and starts fresh.
The tool **never opens Oxigraph from inside the `webxr` binary**;
the binary is expected to be stopped during migration. We
deliberately do not support live dual-write or hot cutover. The
window for this migration is short (one Neo4j export takes
minutes at our scale), and dual-write has a long tail of
consistency problems we are not willing to fund.

### D9. Whelk inference materialisation

`WhelkInferenceEngine` (already a trait, already a port at
`src/ports/inference_engine.rs`) emits inferred axioms as
`OwlAxiom` values. The new Oxigraph adapter writes them as triples
into `<urn:visionflow:graph:ontology:inferred>` in one batch:

```
DELETE { GRAPH <urn:visionflow:graph:ontology:inferred> { ?s ?p ?o } }
WHERE  { GRAPH <urn:visionflow:graph:ontology:inferred> { ?s ?p ?o } } ;
INSERT DATA {
    GRAPH <urn:visionflow:graph:ontology:inferred> {
        <vc:onto/foo> rdfs:subClassOf <vc:onto/bar> .
        <vc:onto/foo> rdfs:subClassOf <vc:onto/baz> .
        …
    }
}
```

The two statements run as a single SPARQL Update request which
Oxigraph treats atomically. Clients querying without a `FROM` clause
see the union of asserted and inferred (D2). Clients wanting only
authored data add `FROM <urn:visionflow:graph:ontology:assert>`.

Inference is **not triggered on the write path**. The
`WhelkInferenceEngine` runs on demand from the
`OntologyInferenceActor` (per Section 8). The persistence layer
does not own the inference schedule.

### D10. Backup is a directory copy

Backup procedure (documented in `docs/operations/backup-restore.md`,
authored as part of Section 9):

```bash
# Stop the writer (or accept point-in-time snapshot from RocksDB)
systemctl stop webxr

# Oxigraph: tar the directory
tar -czf oxigraph-$(date +%Y%m%d-%H%M%S).tar.gz \
        --directory=<data-dir> oxigraph/

# SQLite: VACUUM INTO produces a clean snapshot
sqlite3 <data-dir>/settings.sqlite3 \
        "VACUUM INTO 'settings-$(date +%Y%m%d-%H%M%S).sqlite3'"

# Resume
systemctl start webxr
```

Restore is the inverse: `tar -xzf` and replace the SQLite file
while the writer is stopped. The `webxr` binary verifies on
startup that the Oxigraph dataset opens cleanly and the SQLite
file passes `PRAGMA integrity_check`; failure logs `error!` and
exits with code 2 (data corruption — operator intervention).

## Options considered

### O1. Stay on Neo4j (current)

Rejected. PRD-11 §2 enumerates nine concrete costs and the
license posture is wrong for in-process deployment.

### O2. Apache Jena Fuseki (Java, SPARQL 1.1)

Rejected. Wider feature set (full reasoner, SHACL, GeoSPARQL) and
deeper community, but it is JVM. The whole point of D1 is to remove
the JVM from our deployment. Fuseki would be a sideways move.

### O3. Stardog (Java, SPARQL 1.1, ICV)

Rejected. Commercial license; non-starter for our deployment posture.

### O4. GraphDB (Java, SPARQL 1.1, OWL reasoner)

Rejected. Commercial license at the scale-out tiers; Free edition
caps don't match our future needs.

### O5. PostgreSQL + rdf2 / postgres-rdf extension

Rejected. We do already run PostgreSQL (RuVector). Putting our
ontology in the same database is appealing on operator count
grounds. But:

- `rdf2`-style extensions are unmaintained or alpha-quality.
- We do not want to couple VisionFlow's graph store to the RuVector
  cluster (different operational tiers; different backup cadences;
  RuVector is "if it's gone we re-embed" while the ontology is
  authored data).
- PostgreSQL's general-purpose query planner is poor for triple-
  pattern joins at any scale.

### O6. Oxigraph + SQLite (this ADR)

Adopted. Native Rust, embeddable, W3C-standard, MIT license,
single-binary deployment, line-for-line Cypher → SPARQL
translatability, schema rename bug structurally impossible.

### O7. Oxigraph for everything (settings included)

Rejected. Settings are flat key-value documents with no graph
shape. Modelling them as RDF buys nothing and costs query
clarity ("get all settings for user X" is one SQL statement
versus a SPARQL pattern that has to model user/setting/value as
triples). Two stores is the right cost for the right shape.

### O8. SQLite for everything (graph included)

Rejected. We tried — there is a long history of "graph in
SQL" projects (recursive CTEs, closure tables) and they all
fall over on the queries we run today (variable-length path
patterns, transitive subClassOf+, BRIDGE_TO traversal across
named graphs). Oxigraph's S/P/O indexes are designed for
exactly these queries.

## Risks

- **R1: Oxigraph maturity.** Oxigraph is at version ~0.4 and used
  in production by relatively few high-profile deployments. Risk:
  unknown bugs at our usage corner. Mitigation: pin a specific
  version, contribute fixes upstream, fall back to Apache Jena
  TDB via the same SPARQL surface if necessary (the adapter
  isolation makes this swappable in <500 LOC of new adapter).
- **R2: SPARQL translation completeness.** 32 hot Cypher queries
  need translation. Risk: edge cases (Cypher's `coalesce`,
  `apoc.*` calls, native procedures). Mitigation: catalogue
  every Cypher query in `src/adapters/neo4j_*.rs`, translate
  with paired tests, reject any query that depends on
  Neo4j-specific procedures (we use none today).
- **R3: Migration tool correctness.** The triple-count parity test
  (A4) is the primary safeguard. Risk: subtle data loss
  (timestamps, list-valued properties, null vs missing). Mitigation:
  the migration tool emits a per-predicate parity table; CI fails
  the migration if any predicate drops by more than 0 between
  Neo4j-counted and Oxigraph-counted.
- **R4: Per-user settings task-local context.** The pubkey is
  threaded through the adapter via task-local storage to keep the
  trait signature unchanged (D5). Risk: a code path that calls
  the settings adapter outside a request context (e.g. startup
  defaults loading) loses the pubkey and writes to global by
  accident. Mitigation: a debug-build `assert!(pubkey.is_some()
  || allow_global)` guard and an explicit `as_global()` adapter
  view for the legitimate startup path.
- **R5: SQLite WAL on shared volumes.** If the data directory is a
  network filesystem (NFS), SQLite WAL mode can corrupt. Mitigation:
  document that `<data-dir>` must be on a local filesystem and add
  a startup check that the volume is not NFS-mounted.
- **R6: Audit archive table growth.** ADR-06 §D6 creates monthly
  archive tables `audit_log_archive_yyyymm` that accumulate
  indefinitely. SQLite has no `OFFLINE PARTITION` story; the only
  retention mechanism is `DROP TABLE`. Mitigation: operator runbook
  at `docs/operations/audit-log-retention.md` covers
  `DROP TABLE audit_log_archive_yyyymm` for retired months
  (default retention 24 months) and integrates with the backup
  procedure so a dropped archive is captured on the next snapshot
  before deletion.

## Rejected from main as buggy / unjustified

- Migration 0045's "redirect orphan BRIDGE_TO edges from stub
  OntologyClass nodes to real OwlClass nodes" — entire problem
  class doesn't exist in RDF (D2). The SPARQL translation of 0045
  is a one-liner that adds the `vc:OntologyClass` type triple
  where missing, with no orphan-stub branch.
- The `neo4j_adapter.rs` connection pool, retry/backoff loop, and
  BoltType wrangler (≈400 LOC) — entirely replaced by Oxigraph's
  in-process API. There is no network, no pool, no retry.
- The `:SchemaMigration` node convention — replaced by an SQLite
  `schema_migrations` table (D5) keyed by migration ID. Migrations
  are now a single-row idempotent INSERT, not a MERGE on a
  graph node.
- Neo4j Browser as an ops surface — out, with no replacement at
  this layer. Oxigraph ships a SPARQL query UI as a separate
  optional binary which we do not deploy in production.

## Bugs and smells at the reset point (41979d33e)

To flag for migration awareness:

- The baseline already exhibits the orphan-stub bug — migration
  0045 was written *because of it*. The destination architecture
  (D2 + D3) makes this bug-class impossible.
- `neo4j_settings_repository.rs` at baseline includes 130+ lines
  of cache management. With SQLite in WAL mode at our scale, a
  read-through cache is unnecessary; the adapter should be
  cache-free and trust SQLite's page cache.
- Position persistence to Neo4j was already a known smell (the
  `cbac7532a` fix). D4's RAM-primary, periodic-snapshot model
  resolves it by design.
- The baseline does not yet have `whelk-rs` inference round-trip
  to Neo4j (it's a TODO). D9 specifies the round-trip; the
  inference engine port is unchanged.

## Implementation order (within Section 11 work)

1. **Adapter scaffolding**: empty `oxigraph_*` adapters under
   `src/adapters/`, each implementing the existing trait with
   `todo!()` bodies. CI must compile.
2. **Read-path implementations first** (`load_ontology_graph`,
   `get_owl_class`, `get_graph`, `get_setting`). Tested against
   a small bundled fixture loaded from Turtle.
3. **Write-path implementations** with adapter-level constraint
   ASK (`add_owl_class`, `add_axiom`, `add_nodes`, `add_edges`,
   `set_setting`).
4. **Position snapshot path** (`update_positions` writing
   `vc:hasX/Y/Z` literal triples in a single Update statement).
5. **Whelk materialisation** (`store_inference_results`).
6. **One-shot migration tool**: read Neo4j, write Oxigraph, emit
   parity table.
7. **SPARQL migration files** for 0042–0045. Apply post-import.
8. **Cutover**: delete `src/adapters/neo4j_*.rs`. Update
   `src/adapters/mod.rs`. Remove `neo4rs` from `Cargo.toml`.
9. **Backup-restore documentation** (Section 9).
