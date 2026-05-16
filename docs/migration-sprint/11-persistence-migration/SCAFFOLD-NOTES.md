# SCAFFOLD-NOTES.md — Phase 1 Persistence Scaffolding

Status: complete (Phase 1 of ADR-11 §"Implementation order").
Branch: `impl/phase-1-persistence` (off `radical-rollback` @ `d260a6158`).
Author: backend-api-developer agent.
Date: 2026-05-16.

## What this is

Phase 1 of the persistence migration lands **empty `oxigraph_*` and
`sqlite_*` adapters** under `src/adapters/`, each implementing the
existing port trait surface (`OntologyRepository`, `GraphRepository`,
`SettingsRepository`) with method bodies that are either:

- a `todo!(...)` macro whose message carries the SPARQL or SQL fragment
  that Phase 2 must finish, **or**
- a trivially-correct default (e.g. `Ok(Vec::new())`, `Ok(false)`,
  `PhysicsState::default()`) where the semantic answer is "this adapter
  has no concept of X" (e.g. dirty-node tracking, equilibrium status).

Per PRD-11 A2, **the trait surface itself is frozen** — no
port files were modified. Per ADR-11 §"Implementation order" step 1,
"CI must compile" with these adapters in place; the new module is
behind a Cargo feature (`persistence-oxigraph`) so the radical-rollback
baseline still compiles without pulling in Oxigraph or rusqlite.

The Neo4j adapters remain present and unchanged; cutover (their
deletion + `mod.rs` rewiring + Cargo dep removal) is ADR-11
§"Implementation order" step 8, not Phase 1.

## Files added

| File                                                                  | Lines | Purpose                                              |
|-----------------------------------------------------------------------|------:|------------------------------------------------------|
| `src/adapters/oxigraph_ontology_repository.rs`                        |   502 | `OntologyRepository` over Oxigraph (ADR-11 §D2/§D9)  |
| `src/adapters/oxigraph_graph_repository.rs`                           |   278 | `GraphRepository` over Oxigraph (ADR-11 §D2/§D4)     |
| `src/adapters/sqlite_settings_repository.rs`                          |   402 | `SettingsRepository` over SQLite (ADR-11 §D5)        |
| `migrations/sqlite/0001_initial.sql`                                  |   130 | Canonical schema (ADR-11 §D5 verbatim)               |
| `docs/migration-sprint/11-persistence-migration/SCAFFOLD-NOTES.md`    |  this | Index of `todo!` sites and Phase-2 prioritisation    |

## Files modified

| File                       | Change                                                                  |
|----------------------------|-------------------------------------------------------------------------|
| `Cargo.toml`               | Added optional deps: `oxigraph = "0.4"`, `rusqlite = "0.31"` (bundled), `tokio-rusqlite = "0.5"`. Added `persistence-oxigraph` feature. |
| `src/adapters/mod.rs`      | Added 3 new module declarations + re-exports, all gated by `#[cfg(feature = "persistence-oxigraph")]`. |

## Method inventory by trait

### `OntologyRepository` (24 methods present; 26 `todo!` sites)

The two extra `todo!` sites come from the `pub async fn open(...)`
constructor and an internal `query_ontology` SPARQL pass-through. Methods
whose body is `Ok(...)` (no `todo!`) are not listed — they are the
trait-defaulted no-ops we explicitly retain (none in this trait;
override-or-todo is the discipline).

| #   | Method                              | Line | Phase-1 disposition |
|----:|-------------------------------------|-----:|---------------------|
|  1  | `open(data_dir)` constructor        |   89 | `todo!` — RocksDB open |
|  2  | `load_ontology_graph`               |  113 | `todo!` — **complex (5/5)** |
|  3  | `save_ontology_graph`               |  131 | `todo!` — CLEAR + INSERT DATA |
|  4  | `save_ontology`                     |  149 | `todo!` — composite INSERT DATA |
|  5  | `add_owl_class`                     |  168 | `todo!` — **complex (5/5)** |
|  6  | `get_owl_class`                     |  201 | `todo!` — SELECT ?p ?o |
|  7  | `list_owl_classes`                  |  213 | `todo!` — **complex (5/5)** |
|  8  | `add_owl_property`                  |  234 | `todo!` — ASK + INSERT DATA |
|  9  | `get_owl_property`                  |  252 | `todo!` — SELECT + type-discriminate |
| 10  | `list_owl_properties`               |  262 | `todo!` — SELECT with FILTER IN |
| 11  | `get_classes`                       |  281 | `todo!` — delegate to list_owl_classes |
| 12  | `get_axioms`                        |  287 | `todo!` — SELECT axiom triples |
| 13  | `add_axiom`                         |  305 | `todo!` — sha256-derived IRI + INSERT |
| 14  | `get_class_axioms`                  |  321 | `todo!` — SELECT by vc:subject |
| 15  | `store_inference_results`           |  341 | `todo!` — DELETE+INSERT in :inferred (ADR-11 §D9) |
| 16  | `get_inference_results`             |  359 | `todo!` — SELECT FROM :inferred |
| 17  | `validate_ontology`                 |  372 | `todo!` — ASK battery for ADR-11 §D6 constraints |
| 18  | `query_ontology`                    |  389 | `todo!` — caller-supplied SELECT pass-through |
| 19  | `remove_owl_class`                  |  403 | `todo!` — DELETE WHERE + cascade |
| 20  | `remove_axiom`                      |  413 | `todo!` — DELETE by id |
| 21  | `get_metrics`                       |  430 | `todo!` — **complex (5/5)** |
| 22  | `cache_sssp_result`                 |  455 | `todo!` — INSERT into cache graph |
| 23  | `get_cached_sssp`                   |  465 | `todo!` — SELECT from cache graph |
| 24  | `cache_apsp_result`                 |  472 | `todo!` — CLEAR + INSERT matrix |
| 25  | `get_cached_apsp`                   |  482 | `todo!` — SELECT matrix |
| 26  | `invalidate_pathfinding_caches`     |  489 | `todo!` — CLEAR GRAPH × 2 |

### `GraphRepository` (16 methods present; 7 `todo!` sites)

The other 9 methods are intentionally non-`todo!`: they return
trivially-correct defaults because the GraphRepository contract has
out-of-band concepts (dirty-node tracking, equilibrium status, runtime
constraints, auto-balance notifications, in-flight physics state) that
have no on-disk representation in the new architecture (ADR-11 §D4).
These methods exist on the trait surface and are implemented for parity
but the canonical answer at the persistence layer is "empty / default".

| #  | Method                              | Line | Phase-1 disposition |
|---:|-------------------------------------|-----:|---------------------|
|  1 | `add_nodes`                         |   86 | `todo!` — INSERT DATA KGNode triples |
|  2 | `add_edges`                         |  103 | `todo!` — INSERT DATA reified vc:edge/<sha256-12> |
|  3 | `update_positions`                  |  120 | `todo!` — **complex (5/5)** — DELETE+INSERT vc:hasX/Y/Z per ADR-11 §D4 |
|  4 | `clear_dirty_nodes`                 |  152 | `Ok(())` — adapter has no dirtiness |
|  5 | `get_graph`                         |  165 | `todo!` — SELECT nodes + edges, fold to GraphData |
|  6 | `get_node_map`                      |  188 | `todo!` — SELECT by ?id, group |
|  7 | `get_physics_state`                 |  202 | `Ok(default)` — volatile, no on-disk shape |
|  8 | `get_node_positions`                |  211 | `todo!` — SELECT cold-start positions |
|  9 | `get_bots_graph`                    |  229 | `todo!` — SELECT FROM :agent named graph |
| 10 | `get_constraints`                   |  240 | `Ok(default)` — owned by constraint-set actor |
| 11 | `get_auto_balance_notifications`    |  248 | `Ok(vec![])` — volatile |
| 12 | `get_equilibrium_status`            |  255 | `Ok(false)` — owned by PhysicsOrchestratorActor |
| 13 | `compute_shortest_paths`            |  261 | `Err(NotImplemented)` — SPARQL property paths viable only at small scale |
| 14 | `get_dirty_nodes`                   |  273 | `Ok(empty)` — no dirty concept |

### `SettingsRepository` (17 methods present; 17 + 1 `todo!` sites)

The 17 trait methods are all `todo!`-stubbed plus 1 in the `open()`
constructor, except `clear_cache` which is `Ok(())` (the adapter is
cache-free per ADR-11 §D5 anti-cache stance — SQLite's page cache is
sufficient). Total trait methods: 17 (matches PRD-11 A2 / TC-5 trait-surface SHA).

| #  | Method                          | Line | Phase-1 disposition |
|---:|---------------------------------|-----:|---------------------|
|  0 | `open(db_path)` constructor     |  145 | `todo!` — open + execute_batch(CREATE_SCHEMA) |
|  1 | `get_setting`                   |  167 | `todo!` — per-user layered SELECT |
|  2 | `set_setting`                   |  183 | `todo!` — UPSERT |
|  3 | `delete_setting`                |  203 | `todo!` — DELETE |
|  4 | `has_setting`                   |  214 | `todo!` — SELECT 1 |
|  5 | `get_settings_batch`            |  226 | `todo!` — IN clause + fold |
|  6 | `set_settings_batch`            |  240 | `todo!` — txn + bulk UPSERT |
|  7 | `list_settings`                 |  253 | `todo!` — SELECT DISTINCT key |
|  8 | `load_all_settings`             |  265 | `todo!` — composite document load via path-accessor |
|  9 | `save_all_settings`             |  281 | `todo!` — flatten + txn DELETE+INSERT |
| 10 | `get_physics_settings`          |  295 | `todo!` — profile lookup |
| 11 | `save_physics_settings`         |  310 | `todo!` — profile UPSERT |
| 12 | `list_physics_profiles`         |  327 | `todo!` — SELECT DISTINCT |
| 13 | `delete_physics_profile`        |  338 | `todo!` — DELETE |
| 14 | `export_settings`               |  349 | `todo!` — group by owner_pubkey |
| 15 | `import_settings`               |  364 | `todo!` — inverse of export |
| 16 | `clear_cache`                   |  377 | `Ok(())` — adapter is cache-free (ADR-11 §D5) |
| 17 | `health_check`                  |  388 | `todo!` — SELECT 1 + optional PRAGMA integrity_check |

**Trait-surface SHA test (TC-5)** must wrap exactly these 17 methods.
Phase 2 will compute SHA256 over the sorted method signatures of
`SettingsRepository` and pin it as a constant. Any trait edit forces
an explicit version bump and an ADR amendment.

## `todo!` totals

- **Total `todo!` macro invocations**: **50**
  (26 in `oxigraph_ontology_repository.rs` +
   7 in `oxigraph_graph_repository.rs` +
   17 in `sqlite_settings_repository.rs`)
- Of these, 26 are *trait method bodies*; 24 are *constructor + helper
  fragments*; 5 are flagged as "complex" below.

## Five most complex methods needing explicit Phase-2 work

These five are the long-tail of the Section-11 work plan. The other 45
`todo!` sites are line-for-line translations of an idiom Phase 2 will
do in bulk; these five require explicit design.

### 1. `OntologyRepository::load_ontology_graph` (oxigraph_ontology_repository.rs:125)

**SPARQL**:
```sparql
SELECT ?s ?p ?o
FROM <urn:visionflow:graph:ontology>
WHERE { ?s ?p ?o }
```

**Why complex**: not the query — the *projection back into `GraphData`*. The
RDF triple set must be folded into the node/edge shape that
`GraphData` expects. This involves:

- Identifying which `?s` values are class nodes (`?s a vc:OntologyClass`),
  which are property nodes (`?s a owl:ObjectProperty | owl:DatatypeProperty | owl:AnnotationProperty`),
  and which are axiom nodes (`?s a vc:Axiom`).
- Folding all 40+ OwlClass V2 metadata fields (`belongs_to_domain`,
  `bridges_to_domain`, `quality_score`, `authority_score`, ten
  semantic-relationship vectors, etc.) back into `OwlClass` from
  property-bag rows.
- Constructing `GraphData.edges` from the multi-predicate fan
  (`vc:bridgeTo`, `vc:hasPart`, `vc:requires`, `vc:enables`, `vc:relatesTo`,
  `rdfs:subClassOf`) — each predicate maps to a different edge
  classification on the destination side.

**Phase-2 effort estimate**: 1 day; needs a dedicated `triple_serde.rs`
helper module (called out in PRD-11 §8 bullet 1).

### 2. `OntologyRepository::add_owl_class` (oxigraph_ontology_repository.rs:195)

**SPARQL**: `ASK` + `INSERT DATA` against `<urn:visionflow:graph:ontology>`.

**Why complex**: `OwlClass` has 40+ optional fields (term_id,
preferred_term, label, description, parent_classes, source_domain,
version, class_type, status, maturity, quality_score, authority_score,
public_access, content_status, owl_physicality, owl_role,
belongs_to_domain, bridges_to_domain, source_file, file_sha1,
markdown_content, last_synced, has_part[], is_part_of[], requires[],
depends_on[], enables[], relates_to[], bridges_to[], bridges_from[],
other_relationships{}, properties{}, additional_metadata). Each maps
to one or more triples with appropriate xsd-datatype literals or IRI
references. The INSERT DATA statement is dynamically built from the
struct, with Option<T> fields emitting a triple only when `Some`, and
Vec<T> fields emitting one triple per element.

Additionally: the pre-write `ASK` uniqueness constraint from ADR-11 §D6
must run atomically with the INSERT or be repeated inside the
transaction.

**Phase-2 effort estimate**: 1.5 days; needs the `triple_serde.rs`
helper plus property-mapping table (probably a derive macro on `OwlClass`
in Phase 3).

### 3. `OntologyRepository::list_owl_classes` (oxigraph_ontology_repository.rs:223)

**SPARQL**:
```sparql
SELECT ?s ?p ?o
FROM <urn:visionflow:graph:ontology>
WHERE { ?s a vc:OntologyClass ; ?p ?o }
ORDER BY ?s
```

**Why complex**: this is the **hot read path** used by `OntologyActor` on
startup and on every cache invalidation. It dominates SPARQL p99 in the
perf budget (PRD-11 §7 — target p99 ≤ 50 ms). The query itself is
trivial; the *fold* is non-trivial because rows arrive sorted by
subject, but each subject yields a variable number of rows (40+
property-bag rows per class). Phase 2 must implement a streaming
group-by-subject reducer that holds at most one partial `OwlClass` in
memory at a time, and must avoid the obvious O(n²) of running
`add_to_owl_class(&mut OwlClass, ?p, ?o)` per row.

**Phase-2 effort estimate**: 1 day; this is also the place to add the
benchmarking harness called out in PRD-11 §6 ("SPARQL benchmark: each
of the 32 hot Cypher queries translated and run").

### 4. `OntologyRepository::get_metrics` (oxigraph_ontology_repository.rs:443)

**SPARQL**: a battery of COUNT queries + a recursive subClassOf+
traversal for `max_depth`. The depth pattern:
```sparql
SELECT (MAX(?depth) AS ?d)
FROM <urn:visionflow:graph:ontology>
WHERE {
  ?leaf rdfs:subClassOf+ ?root .
  # ?depth computation requires explicit per-subject path enumeration
  # (SPARQL has no built-in depth aggregate)
}
```

**Why complex**: SPARQL property paths (`+`) give you reachability but
not depth. Computing depth requires either:
- An iterative ASK loop in Rust (1-hop reachability, 2-hop, ...) until
  fixpoint — slow but trivially correct, **or**
- Direct walk of the subClassOf graph via `Store::quads_for_pattern`
  outside the SPARQL surface — faster but couples the adapter to
  Oxigraph's lower-level API.

`average_branching_factor` is straightforward (COUNT children grouped
by parent, averaged).

**Phase-2 effort estimate**: 0.5 day to pick the depth strategy + 0.5
day to implement + 0.5 day to benchmark against PRD-11 §7 budget.

### 5. `GraphRepository::update_positions` (oxigraph_graph_repository.rs:120)

**SPARQL** (per ADR-11 §D4 position-triple snapshot):

```sparql
DELETE { GRAPH <urn:visionflow:graph:knowledge> {
           ?n vc:hasX ?x ; vc:hasY ?y ; vc:hasZ ?z ;
              vc:hasVX ?vx ; vc:hasVY ?vy ; vc:hasVZ ?vz . } }
WHERE  { GRAPH <urn:visionflow:graph:knowledge> {
           ?n a vc:KGNode ; vc:nodeId ?id .
           FILTER(?id IN (<u32-list>))
           ?n vc:hasX ?x ; ... } } ;
INSERT DATA { GRAPH <urn:visionflow:graph:knowledge> {
           <vc:kg/<slug-of-id>>
               vc:hasX "<x>"^^xsd:float ;
               vc:hasY "<y>"^^xsd:float ;
               vc:hasZ "<z>"^^xsd:float ;
               vc:hasVX "<vx>"^^xsd:float ;
               vc:hasVY "<vy>"^^xsd:float ;
               vc:hasVZ "<vz>"^^xsd:float .
           # ...one block per update...
} }
```

**Why complex**: this is the position-snapshot hot path. At ~5k nodes
with a 60 s cadence the update set is ~5k subjects each writing 6
triples (= ~30k triples per snapshot). The DELETE-then-INSERT pattern
batched into a single SPARQL Update string is the canonical way to
express atomicity, but the string itself is ~1 MB for 5k nodes and
forces oxigraph to re-parse the entire batch on every snapshot.

Phase-2 decision will likely be to bypass SPARQL Update entirely for
this method and use Oxigraph's lower-level `Store::insert` /
`Store::remove` API within a single transaction — same on-disk effect,
order-of-magnitude lower CPU. The trade-off is that the adapter then
has two different write paths (declarative SPARQL for everything else,
imperative for positions) which is a clarity cost.

**Phase-2 effort estimate**: 1 day to benchmark the SPARQL-Update path
first (it may be fast enough at our scale), 0.5 day to add the
lower-level path if not.

## What was deliberately deferred

- The migration tool at `tools/migrate-neo4j-to-oxigraph/` (ADR-11 §D8).
  Phase 6 work; needs the adapters working end-to-end first.
- The four SPARQL migration files at `queries/migrations/sparql/` (ADR-11
  §D7). Phase 7 work; needs the adapter write-path proven.
- The `OxigraphAgentTelemetryRepository` — Section 7 territory (PRD-11
  §10 calls this out as a separate cross-section dependency).
- The audit-log adapter co-located with `SqliteSettingsRepository`
  (ADR-11 §D5 last bullet). It needs the rest of ADR-06 §D6 landed
  first.
- Trait-surface SHA test (TC-5). The test belongs with the first real
  adapter PR, not the scaffolding.
- BDD scenarios from T5 (domain-only change, storage-only change,
  cross-cutting change). Phase 2 deliverable.

## Verification performed

- File diff vs port traits: every method on
  `OntologyRepository`, `GraphRepository`, `SettingsRepository` is
  present in the corresponding adapter with the exact signature copied
  from the port file. No signature divergence.
- Feature-gate: all three new modules + their re-exports are behind
  `#[cfg(feature = "persistence-oxigraph")]`; without the feature,
  `Cargo.toml` does not pull `oxigraph` / `rusqlite` / `tokio-rusqlite`.
- Schema parity: `migrations/sqlite/0001_initial.sql` matches the
  embedded `CREATE_SCHEMA` constant in `sqlite_settings_repository.rs`
  (a Phase-2 byte-equality test will assert this).
- `cargo check --features persistence-oxigraph --no-default-features`
  was attempted but failed on `cust_raw v0.11.3` (unconditional CUDA
  dep — pre-existing project issue unrelated to this PR; see the
  baseline build setup in `build.rs` and `Cargo.toml` lines 82-84
  which pull `cust`/`cudarc` unconditionally rather than under the
  `gpu` feature). The new adapter source has been manually code-reviewed
  against the trait surface; type signatures and import paths are
  consistent with the existing `neo4j_*` adapter prior art.

## Definition of done for Phase 1 (per ADR-11)

> "1. Adapter scaffolding: empty `oxigraph_*` adapters under
>    `src/adapters/`, each implementing the existing trait with
>    `todo!()` bodies. CI must compile."

This deliverable. Phase 2 begins with read-path implementations against
a Turtle fixture per ADR-11 step 2.
