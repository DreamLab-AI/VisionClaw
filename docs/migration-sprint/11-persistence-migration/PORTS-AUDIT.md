# PORTS-AUDIT — Phase 1 Persistence

Source-of-truth alignment between `src/ports/*.rs`, `src/adapters/neo4j_*.rs`,
`DDD-11.md`, and `ADR-11.md`. Authored from worktree
`visionflow-worktrees/phase-1-persistence` on top of
`impl/phase-1-persistence` (off `radical-rollback @ d260a6158`).

Header counts as verified from source on 2026-05-16:

| Port                       | Trait methods | Adapter file (Neo4j)                        | LOC   |
|----------------------------|---------------|---------------------------------------------|-------|
| `OntologyRepository`       | **27**        | `src/adapters/neo4j_ontology_repository.rs` | 1,600 |
| `GraphRepository`          | **15**        | `src/adapters/neo4j_graph_repository.rs` *  | 756   |
| `SettingsRepository`       | **17**        | `src/adapters/neo4j_settings_repository.rs` | 1,137 |

\* The hot-path `GraphRepository` impl in the current code is
`src/adapters/actor_graph_repository.rs` (delegates to `GraphStateActor`); the
Neo4j file is the durable-tier impl. ADR-11 §D4 keeps the RAM-canonical /
periodic-snapshot split, so both impls remain after cutover but `neo4j_*` is
replaced by `oxigraph_*`.

**The "40+ / 24+ / 44+" annotations in DDD-11 §"Commands accepted" are stale.**
The real trait surfaces are smaller than DDD-11 advertises. The audit
proceeds against the verified counts above. DDD-11 must be corrected to
`27 / 15 / 17`.

---

## 1. Trait surface enumeration

### 1.1 `OntologyRepository` (27 methods)

| # | Method | Cypher today (`neo4j_ontology_repository.rs`) | SPARQL / SQLite target (ADR-11) |
|---|--------|-----------------------------------------------|---------------------------------|
| 1 | `load_ontology_graph()` | `MATCH (c:OwlClass)…` + edge sweep, lines 1107–1153 | `CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <urn:visionflow:graph:ontology:assert> { ?s ?p ?o } UNION GRAPH <…:inferred> { ?s ?p ?o } }` |
| 2 | `save_ontology_graph(graph)` | Bulk MERGE over OwlClass + relationships, 1154–1189 | Batched `INSERT DATA { GRAPH <…:assert> { … } }` per type, idempotent via FNE |
| 3 | `save_ontology(classes, properties, axioms)` | Multi-pass MERGE, 1190–1217 | One transaction: 3 batched `INSERT DATA` blocks in `<…:assert>` |
| 4 | `add_owl_class(class)` | `MERGE (c:OwlClass {iri:$iri}) ON CREATE/ON MATCH SET …`, 288–488 | ADR-11 D6: `ASK { GRAPH <…:assert> { <iri> a vc:OntologyClass } }` then `INSERT DATA { GRAPH <…:assert> { <iri> a vc:OntologyClass ; vc:label …; … } }` |
| 5 | `get_owl_class(iri)` | `MATCH (c:OwlClass {iri:$iri}) RETURN c`, 489–529 | `SELECT ?p ?o WHERE { GRAPH <…:assert> { <iri> ?p ?o } }`, pivot in adapter into `OwlClass` |
| 6 | `list_owl_classes()` | `MATCH (c:OwlClass) RETURN c`, 530–571 | `SELECT ?s ?p ?o WHERE { GRAPH <…:assert> { ?s a vc:OntologyClass ; ?p ?o } }` |
| 7 | `add_owl_property(property)` | `MERGE (p:OwlProperty {iri:$iri}) …`, 622–673 | `INSERT DATA { GRAPH <…:assert> { <iri> a vc:OwlProperty ; vc:propertyType "ObjectProperty" ; … } }` after ASK |
| 8 | `get_owl_property(iri)` | `MATCH (p:OwlProperty {iri:$iri}) RETURN p`, 674–741 | `SELECT … WHERE { GRAPH <…:assert> { <iri> ?p ?o } }` |
| 9 | `list_owl_properties()` | `MATCH (p:OwlProperty) RETURN p`, 742–812 | `SELECT ?s ?p ?o WHERE { GRAPH <…:assert> { ?s a vc:OwlProperty ; ?p ?o } }` |
| 10 | `get_classes()` | Alias of `list_owl_classes`, 1218–1222 | Same as #6 |
| 11 | `get_axioms()` | `MATCH (a:OwlAxiom) RETURN a`, 861–928 | `SELECT ?id ?type ?s ?o ?ann WHERE { GRAPH <…:assert> { ?ax a vc:OwlAxiom ; vc:axiomType ?type ; vc:subject ?s ; vc:object ?o ; vc:annotations ?ann ; vc:axiomId ?id } }` |
| 12 | `add_axiom(axiom)` | `MERGE (a:OwlAxiom {id:$id}) …`, 813–858 | `INSERT DATA { GRAPH <…:assert> { <vc:axiom/<id>> a vc:OwlAxiom ; vc:axiomType …; vc:subject <s> ; vc:object <o> ; vc:annotations "{json}" } }` |
| 13 | `get_class_axioms(class_iri)` | `MATCH (a:OwlAxiom) WHERE a.subject=$iri OR a.object=$iri`, 1223–1276 | `SELECT … WHERE { GRAPH <…:assert> { ?ax a vc:OwlAxiom ; (vc:subject\|vc:object) <iri> ; ?p ?o } }` |
| 14 | `store_inference_results(results)` | Loops `add_axiom` per inferred, 929–937 (no atomicity) | **D9 atomic:** `DELETE WHERE { GRAPH <…:inferred> { ?s ?p ?o } } ; INSERT DATA { GRAPH <…:inferred> { … } }` in one `Store::update` call |
| 15 | `get_inference_results()` | Default `Ok(None)` (trait default; Neo4j adapter does not override) | `SELECT … WHERE { GRAPH <…:inferred> { ?s ?p ?o } }` plus a metadata triple `<urn:visionflow:meta:lastInference> vc:timestamp ?t` in default graph |
| 16 | `validate_ontology()` | `MATCH (c:OwlClass) WHERE c.iri IS NULL …` plus duplicate-IRI ASK, 1028–1062 | SPARQL `ASK` guards + adapter-side `ValidationReport`; mirrors D6 pre-write checks |
| 17 | `query_ontology(query)` | Default `Ok(vec![])`; Neo4j adapter does not override | `Store::query(SparqlQuery::parse(query))` — pass-through SELECT with row→HashMap pivot |
| 18 | `remove_owl_class(iri)` | `MATCH (c:OwlClass {iri:$iri}) DETACH DELETE c`, 572–594 | `DELETE WHERE { GRAPH <…:assert> { <iri> ?p ?o . ?s ?p2 <iri> } }` (drops incoming refs too — matches DETACH DELETE) |
| 19 | `remove_axiom(axiom_id)` | `MATCH (a:OwlAxiom {id:$id}) DETACH DELETE a`, 595–621 | `DELETE WHERE { GRAPH <…:assert> { <vc:axiom/<id>> ?p ?o } }` |
| 20 | `get_metrics()` | 4× `COUNT(c)` sweeps, 944–1027 | 4× `SELECT (COUNT(*) AS ?n) WHERE { GRAPH <…:assert> { ?s a vc:OntologyClass } }` etc. — one query per metric |
| 21 | `cache_sssp_result(entry)` | Default `Ok(())` — explicitly no-op in Neo4j adapter 1063 | SQLite `pathfinding_cache` table is **not** in ADR-11 §D5. **Trait gap** (see §3). Default no-op stays in `OxigraphOntologyRepository`. |
| 22 | `get_cached_sssp(node_id)` | Default `Ok(None)` 1075 | Same gap as #21 |
| 23 | `cache_apsp_result(matrix)` | Default `Ok(())` 1082 | Same gap |
| 24 | `get_cached_apsp()` | Default `Ok(None)` 1090 | Same gap |
| 25 | `invalidate_pathfinding_caches()` | Default `Ok(())` 1097 | Same gap |
| 26 | `(implicit trait default) get_inference_results()` | Counted at #15 |   |
| 27 | `(implicit trait default) validate_ontology()` | Counted at #16 |   |

Rows 26–27 collapse onto 15/16; the trait body lists 27 distinct `async fn`
declarations but 5 of them (`store_inference_results`, `get_inference_results`,
`validate_ontology`, `query_ontology`, and the five `*_cache_*` methods) ship
trait-default bodies. The verified non-defaulted surface is **17 methods**
(rows 1–13 + 16 + 18–20). All cache methods (#21–25) are dead-code in the
Neo4j adapter — the trait defaults run.

### 1.2 `GraphRepository` (15 methods)

| # | Method | Cypher today (`neo4j_graph_repository.rs`) | SPARQL / SQLite target (ADR-11) |
|---|--------|--------------------------------------------|---------------------------------|
| 1 | `add_nodes(nodes)` | `UNWIND range… MERGE (n:GraphNode {id:$ids[i]}) ON CREATE/ON MATCH SET …`, 426–563 | `INSERT DATA { GRAPH <urn:visionflow:graph:knowledge> { <vc:kg/{slug}> a vc:KGNode ; vc:id "…"; vc:label "…"; vc:metadataId "…" ; vc:mass "…"^^xsd:decimal ; … } }` (one large batch per call) |
| 2 | `add_edges(edges)` | `UNWIND … MATCH … MERGE (s)-[r:EDGE]->(t) SET r.weight=…`, 565–625 | Reified edges: `INSERT DATA { GRAPH <…:knowledge> { <vc:edge/{sha256-12}> a vc:Edge ; vc:from <s> ; vc:to <t> ; vc:weight "…"^^xsd:decimal ; vc:edgeType "default" } . <s> vc:linksTo <t> }` |
| 3 | `update_positions(updates)` | `UNWIND … MATCH (n) SET n.sim_x=…`, 651–708 | **D4 atomic:** one Update: `DELETE WHERE { GRAPH <…:knowledge> { ?n vc:hasX ?_ ; vc:hasY ?_ ; vc:hasZ ?_ ; vc:hasVx ?_ ; vc:hasVy ?_ ; vc:hasVz ?_ . FILTER(?n IN (…)) } } ; INSERT DATA { GRAPH <…:knowledge> { <n1> vc:hasX "x"^^xsd:float ; vc:hasY … . … } }` |
| 4 | `clear_dirty_nodes()` | `MATCH (n:GraphNode {dirty:true}) SET n.dirty=false`, 709–714 | **Not modelled in RDF.** Dirty tracking is a RAM concern (GraphStateActor). Oxigraph adapter forwards to RAM only; trait-default no-op acceptable. **Trait gap candidate** (§3). |
| 5 | `get_graph()` | Returns cached snapshot loaded by `load_graph()`, 626–637 | `CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <…:knowledge> { ?s ?p ?o } }` |
| 6 | `get_node_map()` | Projection of #5, 638–645 | Same SPARQL CONSTRUCT, pivot into `HashMap<u32, Node>` |
| 7 | `get_physics_state()` | Returns `PhysicsState::default()` — Neo4j has no live physics, 646–650 | RAM-only (D4). Adapter returns `PhysicsState::default()`; signature kept for trait stability |
| 8 | `get_node_positions()` | `MATCH (n:GraphNode) RETURN n.sim_x, n.sim_y, n.sim_z`, 731–744 | **D4 cold-start path:** `SELECT ?n ?x ?y ?z WHERE { GRAPH <…:knowledge> { ?n vc:hasX ?x ; vc:hasY ?y ; vc:hasZ ?z } }` |
| 9 | `get_bots_graph()` | `MATCH (a:Agent) RETURN a` + edge sweep, 745–750 | `CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <urn:visionflow:graph:agent> { ?s ?p ?o } }` |
| 10 | `get_constraints()` | Returns `ConstraintSet::default()` (not stored in Neo4j), 719–722 | Same; stored in SQLite as a single setting `physics.constraints`. Adapter delegates to `SettingsRepository::get_setting`. **Cross-port read**. |
| 11 | `get_auto_balance_notifications()` | Returns `Vec::new()` (not stored), 715–718 | Same; RAM-only |
| 12 | `get_equilibrium_status()` | Returns `false` (not stored), 751–755 | Same; RAM-only |
| 13 | `compute_shortest_paths(params)` | Not implemented for Neo4j — returns `NotImplemented`, 723–726 | **Hard:** SPARQL has no native shortest-path. Oxigraph adapter must implement BFS in Rust over `SELECT ?src ?tgt WHERE { GRAPH <…:knowledge> { ?src vc:linksTo ?tgt } }`. Top-3 risk (§5). |
| 14 | `get_dirty_nodes()` | `MATCH (n:GraphNode {dirty:true})`, 727–730 | RAM-only; same as #4 |
| 15 | `(implicit graph load)` | `pub async fn load_graph()` 135–183 — not on the trait | Internal helper; absorbed by `get_graph()` |

### 1.3 `SettingsRepository` (17 methods — verified)

All 17 map to one SQLite table per ADR-11 §D5 (`settings` for global + per-user,
`physics_profiles` for per-profile physics, plus the `current_owner_pubkey()`
task-local from Section 6). See §5 below.

---

## 2. DDD-11 conformance check

### 2.1 Aggregate root: `OxigraphDataset`

| DDD-11 invariant / op | Trait method(s) | Status |
|------|-----|--------|
| Single-writer | n/a (process-level) | OK — enforced at `Store::open()` |
| IRI uniqueness in typed graph | `add_owl_class`, `add_owl_property`, `add_axiom`, `add_nodes`, `add_edges` | OK — D6 ASK guards land in each |
| Foreign-IRI integrity for BRIDGE_TO | `add_edges` (when source is KGNode + target is OntologyClass) | **Missing dedicated method.** `BRIDGE_TO` edges in default graph are added via the generic `add_edges` path. The integrity check must execute in the OxigraphGraphRepository adapter when an edge's source/target span graphs. Documented in DDD-11 invariants but not surfaced as a distinct port method — relies on adapter discipline. |
| Inferred axioms derivable | `store_inference_results` (clears + writes inferred graph atomically) | OK |
| Position snapshot atomicity | `update_positions` (one Update with N×3 inserts) | OK |
| `load_ontology_graph()` op | #1 in §1.1 | OK |
| `save_ontology_graph(graph)` op | #2 | OK |
| `add_owl_class(class)` op | #4 | OK |
| `get_owl_class(iri)` op | #5 | OK |
| `add_axiom(axiom)` op | #12 | OK |
| `store_inference_results(results)` op | #14 | OK |
| `update_positions(updates)` op | #3 in §1.2 | OK |
| `add_nodes(nodes)`, `add_edges(edges)` ops | #1, #2 in §1.2 | OK |

### 2.2 Aggregate root: `SqliteSettingsStore`

| DDD-11 invariant / op | Trait method(s) | Status |
|------|-----|--------|
| Primary key on `(key, owner_pubkey)` | `set_setting`, `set_settings_batch`, `delete_setting` | OK — SQLite-enforced |
| Owner pubkey 64-char hex | `set_setting`, `set_settings_batch` | OK — adapter-side validation; not on trait |
| Resolution order pubkey-specific → global | `get_setting`, `get_settings_batch`, `load_all_settings` | OK — ORDER BY trick in DDD-11 |
| WAL mode | n/a (adapter ctor) | OK |
| JSON-valued column parse | `get_setting`, `load_all_settings`, `export_settings` | OK |
| Per-key CRUD | `get_setting`, `set_setting`, `delete_setting`, `has_setting` | OK |
| Batch | `get_settings_batch`, `set_settings_batch` | OK |
| Prefix listing | `list_settings(prefix)` | OK |
| Full load/save | `load_all_settings`, `save_all_settings` | OK |
| Physics profile CRUD | `get_physics_settings`, `save_physics_settings`, `list_physics_profiles`, `delete_physics_profile` | OK |
| Export/import | `export_settings`, `import_settings` | OK |

### 2.3 Domain events

DDD-11 explicitly states "Persistence is below the level at which domain
events are emitted" — Section 11 emits **none** and consumes **none**. No port
method maps to a domain event because there are none. Section 8 owns the
event vocabulary (`OwlClassAdded`, `OntologyInferred`, etc.) and calls
`add_owl_class` / `store_inference_results` from event handlers.

**Conformance: complete.** No DDD-11 domain event is unrepresented in a port
method, because the bounded context has zero events by design.

### 2.4 ACL surfaces

| DDD-11 ACL | Port surface | Status |
|------|-----|--------|
| → Section 8 (Ontology) | `OntologyRepository` trait (entirety) | OK |
| → Section 5 (Settings) | `SettingsRepository` trait (entirety) | OK |
| → Section 1 (Physics) | `GraphRepository::update_positions`, `::get_node_positions` | OK |
| → Section 6 (Auth) | `auth_context::current_owner_pubkey()` task-local | **Not on port trait.** Adapter consumes; trait surface stable per TC-5. Documented in DDD-11 R4 + §D5. Conformant. |
| → `whelk-rs` | `OntologyRepository::store_inference_results` consuming `InferenceResults` | OK |

---

## 3. Trait gaps

### 3.1 DDD-11 concepts with no port method

| DDD-11 concept | Issue | Resolution |
|------|-----|-----|
| `IriMinter` (DDD-11 ubiquitous language; aggregate field on `OxigraphDataset`) | No port method exposes IRI minting. Callers pass strings; the adapter mints on insert. | **Acceptable** — IRI minting is an internal aggregate concern (DDD-11 §"Bounded context" lists it as sovereign). Section 8 does not need it on the port. |
| `BRIDGE_TO` foreign-IRI integrity invariant | Discharged inside `add_edges`. No dedicated `add_bridge_edge(kgnode_iri, ontology_iri, kind, confidence)` method exists. | **Soft gap.** ADR-11 §D2 places BRIDGE_TO in the default graph. The current trait shape forces the adapter to *detect* a BRIDGE_TO from generic Edge input. Recommend adding `add_bridge_edges(edges)` in a follow-up ADR but do **not** widen the port surface during Phase 1 — TC-5 freezes the trait. |
| Backup snapshot semantics (DDD-11 §"Backup snapshot model") | `Snapshotter::snapshot_to(dir)` is mentioned but lives outside the three ports. | **Acceptable** — Section 9 owns the operator surface. New trait not part of Phase 1. |

### 3.2 Port methods with no DDD-11 justification (deletion candidates)

| Method | Why suspect | Recommendation |
|------|-----|------|
| `OntologyRepository::cache_sssp_result` (#21) | DDD-11 doesn't model SSSP cache; ADR-11 §D5 schema has no `pathfinding_cache` table. All five cache methods are trait-default no-ops in the Neo4j adapter today. | **Keep as trait defaults, do not implement.** Surface remains stable; semantics absent. |
| `OntologyRepository::get_cached_sssp` (#22) | Same | Same |
| `OntologyRepository::cache_apsp_result` (#23) | Same | Same |
| `OntologyRepository::get_cached_apsp` (#24) | Same | Same |
| `OntologyRepository::invalidate_pathfinding_caches` (#25) | Same | Same |
| `OntologyRepository::query_ontology` (#17) | Generic "run a Cypher/SPARQL string" escape hatch. DDD-11 §Bounded-context says persistence "speaks triples, not concepts" — but exposing raw SPARQL to upstream callers leaks the storage idiom. Currently a trait default returning empty. | **Keep as trait default.** Implement only if Section 8 actually needs it; do not promote to non-default. |
| `GraphRepository::get_constraints` (#10) | DDD-11 places constraints in `SettingsRepository`, not in the graph. Method exists because legacy callers expected it on the graph trait. | **Cross-port redirect:** OxigraphGraphRepository constructor takes `Arc<dyn SettingsRepository>`; method delegates. Documented in §1.2 row 10. Keep trait surface, change impl. |
| `GraphRepository::get_auto_balance_notifications` (#11) | RAM-only; unrelated to persistence. | **Keep on trait** for actor-adapter use; Oxigraph adapter returns empty `Vec`. |
| `GraphRepository::clear_dirty_nodes`, `get_dirty_nodes` (#4, #14) | RAM-only. Currently the Neo4j adapter writes `n.dirty=true` on update — this is a misuse that should be retired. | **Keep on trait** (actor adapter uses it). Oxigraph impl returns empty `HashSet` and is a no-op. |
| `GraphRepository::get_physics_state` (#7) | Returns default; never populated by Neo4j. | **Keep.** Oxigraph impl identical (returns default). |
| `GraphRepository::get_equilibrium_status` (#12) | Returns false; never set. | **Keep.** Oxigraph impl identical (returns false). |

**Total port methods flagged for "implement as no-op / default": 11.**
**Total port methods flagged for "delete and reduce surface": 0.** TC-5 freezes
the `SettingsRepository` shape and DDD-11 §"Commands accepted" reiterates that
no method is added or removed during the Phase 1 cutover.

### 3.3 Trait gap headline number

**0 hard gaps. 1 soft gap (BRIDGE_TO method).** All 27 + 15 + 17 = **59 trait
methods** are reachable from DDD-11's aggregate operations and ACL contracts.
The five SSSP/APSP cache methods, plus the four no-op graph methods
(`clear_dirty_nodes`, `get_dirty_nodes`, `get_physics_state`,
`get_equilibrium_status`), are kept on the trait for caller stability but ship
as no-ops or defaults in `OxigraphOntologyRepository` / `OxigraphGraphRepository`.

---

## 4. Named-graph IRI catalogue

Confirmed canonical IRIs (and **discrepancy with ADR-11 §D2**):

| Canonical IRI (this audit, per Queen brief) | ADR-11 §D2 spelling | Used by port methods |
|------|-----|-----|
| `<urn:visionflow:graph:knowledge>` | identical | **W**: `add_nodes`, `add_edges`, `update_positions`. **R**: `get_graph`, `get_node_map`, `get_node_positions`. |
| `<urn:visionflow:graph:ontology:assert>` | **ADR-11 has `<urn:visionflow:graph:ontology>` (no `:assert` suffix).** DDD-11 inherits the bare form. | **W**: `add_owl_class`, `add_owl_property`, `add_axiom`, `save_ontology`, `save_ontology_graph`. **R**: `get_owl_class`, `get_owl_property`, `list_owl_classes`, `list_owl_properties`, `get_classes`, `get_axioms`, `get_class_axioms`, `load_ontology_graph`, `validate_ontology`, `get_metrics`. |
| `<urn:visionflow:graph:ontology:inferred>` | identical | **W**: `store_inference_results` (DELETE + INSERT in one Update). **R**: `get_inference_results`, also union-readable from `load_ontology_graph`. |
| `<urn:visionflow:graph:agent>` | identical | **R**: `get_bots_graph`. **W**: not in Phase 1; ADR-11 reserves the IRI for Section 7 agent telemetry. |
| Default graph (unnamed) | identical | **W**: BRIDGE_TO quads from `add_edges` (when the edge spans knowledge↔ontology). **R**: union queries see it implicitly. |

**Action required:** Either ADR-11 §D2 must rename `<urn:visionflow:graph:ontology>`
to `<urn:visionflow:graph:ontology:assert>` for parallel structure with
`:inferred`, **or** the Queen brief is amended to drop `:assert`. Recommendation:
**rename to `:assert`** — it makes the asserted/inferred dichotomy symmetric in
the IRI namespace and matches how operators discuss the data ("the assert graph
and the inferred graph"). Estimated impact: 6 occurrences in ADR-11, 12 in
DDD-11, 0 in source (no `oxigraph_*` adapter exists yet). This audit assumes
the `:assert` form for all SPARQL templates above.

---

## 5. `SettingsRepository` — all 17 methods with SQLite query templates

ADR-11 §D5 schema:

```sql
settings(key TEXT, owner_pubkey TEXT, value TEXT, description TEXT, updated_at INTEGER, PRIMARY KEY (key, owner_pubkey)) WITHOUT ROWID
physics_profiles(profile_name TEXT, owner_pubkey TEXT, settings_json TEXT, updated_at INTEGER, PRIMARY KEY (profile_name, owner_pubkey)) WITHOUT ROWID
schema_migrations(id TEXT PRIMARY KEY, applied_at INTEGER)
audit_log(id INTEGER PRIMARY KEY AUTOINCREMENT, occurred_at INTEGER, actor_pubkey TEXT, request_method TEXT, request_path TEXT, status_code INTEGER, detail_json TEXT)
```

Owner-pubkey resolution: `?U = current_owner_pubkey()` (task-local, may be `NULL`).

| # | Method | SQLite query template |
|---|--------|---------------------|
| 1 | `get_setting(key)` | `SELECT value FROM settings WHERE key = ?K AND (owner_pubkey = ?U OR owner_pubkey IS NULL) ORDER BY owner_pubkey IS NULL ASC LIMIT 1` — parse `value` as JSON `SettingValue` |
| 2 | `set_setting(key, value, description)` | `INSERT INTO settings (key, owner_pubkey, value, description, updated_at) VALUES (?K, ?U, ?V, ?D, unixepoch()) ON CONFLICT(key, owner_pubkey) DO UPDATE SET value = excluded.value, description = excluded.description, updated_at = excluded.updated_at` |
| 3 | `delete_setting(key)` | `DELETE FROM settings WHERE key = ?K AND owner_pubkey IS ?U` (NULL-safe via `IS`) |
| 4 | `has_setting(key)` | `SELECT 1 FROM settings WHERE key = ?K AND (owner_pubkey = ?U OR owner_pubkey IS NULL) LIMIT 1` — boolean from row presence |
| 5 | `get_settings_batch(keys)` | One transaction with prepared stmt looped, or `SELECT key, value FROM settings WHERE key IN (rarray(?)) AND (owner_pubkey = ?U OR owner_pubkey IS NULL) ORDER BY key, owner_pubkey IS NULL ASC` collapsed in Rust |
| 6 | `set_settings_batch(updates)` | `BEGIN; … INSERT … ON CONFLICT … (prepared, looped) … ; COMMIT` — single transaction wraps the loop |
| 7 | `list_settings(prefix)` | `SELECT DISTINCT key FROM settings WHERE (?P IS NULL OR key LIKE ?P || '%') AND (owner_pubkey = ?U OR owner_pubkey IS NULL) ORDER BY key` |
| 8 | `load_all_settings()` | `SELECT key, value FROM settings WHERE (owner_pubkey = ?U OR owner_pubkey IS NULL) ORDER BY key, owner_pubkey IS NULL ASC` — pivot into `AppFullSettings` via serde |
| 9 | `save_all_settings(settings)` | `BEGIN; DELETE FROM settings WHERE owner_pubkey IS ?U ; INSERT … (looped over flattened keys) ; COMMIT` (full replace at the per-user scope; global rows untouched) |
| 10 | `get_physics_settings(profile_name)` | `SELECT settings_json FROM physics_profiles WHERE profile_name = ?P AND (owner_pubkey = ?U OR owner_pubkey IS NULL) ORDER BY owner_pubkey IS NULL ASC LIMIT 1` — deserialize as `PhysicsSettings` |
| 11 | `save_physics_settings(profile_name, settings)` | `INSERT INTO physics_profiles (profile_name, owner_pubkey, settings_json, updated_at) VALUES (?P, ?U, ?J, unixepoch()) ON CONFLICT(profile_name, owner_pubkey) DO UPDATE SET settings_json = excluded.settings_json, updated_at = excluded.updated_at` |
| 12 | `list_physics_profiles()` | `SELECT DISTINCT profile_name FROM physics_profiles WHERE (owner_pubkey = ?U OR owner_pubkey IS NULL) ORDER BY profile_name` |
| 13 | `delete_physics_profile(profile_name)` | `DELETE FROM physics_profiles WHERE profile_name = ?P AND owner_pubkey IS ?U` |
| 14 | `export_settings()` | `SELECT key, owner_pubkey, value, description FROM settings UNION ALL SELECT 'physics:' \|\| profile_name, owner_pubkey, settings_json, NULL FROM physics_profiles` — emit as JSON object keyed by `(scope, key)` |
| 15 | `import_settings(json)` | `BEGIN; (per row) INSERT … ON CONFLICT … DO UPDATE … ; COMMIT` — accepts JSON in the shape produced by #14 |
| 16 | `clear_cache()` | `Ok(())` — SQLite WAL page cache is fine; the adapter has no application-level read-through cache per ADR-11 §"baseline cache management is unnecessary" |
| 17 | `health_check()` | `SELECT 1` plus `PRAGMA integrity_check` (full check is heavy; route to startup only, run `SELECT 1` here) |

Per-user write discipline: methods 2, 6, 9, 11, 15 must reject when
`current_owner_pubkey()` is `None` unless `allow_global` is true (ADR-11 R4).
Reads never reject — they fall through to global.

---

## Summary for the Queen

- **Trait gaps found: 0 hard, 1 soft (BRIDGE_TO has no dedicated method;
  flows through generic `add_edges`).**
- **DDD-11 concepts with no port method:** none. All aggregate roots,
  invariants, and ACL contracts map to existing trait methods. `IriMinter`,
  `Snapshotter`, and `current_owner_pubkey()` are deliberately
  adapter-internal or cross-bounded-context, per DDD-11 §"What this context
  does not contain".
- **DDD-11 documentation drift:** §"Commands accepted" claims 40+/24+/44+
  methods. Actual surfaces are 27/15/17. Fix DDD-11 to match verified counts.
- **Named-graph IRI drift:** the Queen brief specifies
  `<urn:visionflow:graph:ontology:assert>` but ADR-11 §D2 writes
  `<urn:visionflow:graph:ontology>`. Recommend renaming ADR-11 to use the
  `:assert` suffix for symmetry with `:inferred`. Scope: 18 doc occurrences,
  0 source occurrences.
- **Top-3 highest-risk methods (most complex SPARQL translation):**
  1. **`GraphRepository::compute_shortest_paths`** — SPARQL 1.1 has no native
     shortest-path; requires Rust-side BFS over a `CONSTRUCT`-ed edge set.
     Currently `NotImplemented` for Neo4j; first true implementation lives
     in the Oxigraph adapter. Risk: O(V+E) memory pressure on large graphs;
     test on production-scale ontology before cutover.
  2. **`OntologyRepository::store_inference_results`** — must execute
     `DELETE` + `INSERT DATA` as a **single** SPARQL Update for atomicity
     (DDD-11 invariant; Neo4j adapter currently loops `add_axiom` per
     inferred axiom, which is non-atomic and would re-create the
     "asserted graph mutated by inference" bug from baseline). Risk:
     getting the Update string syntax exactly right; Oxigraph parses
     SPARQL 1.1 strictly and a stray newline between statements breaks
     atomicity guarantees.
  3. **`GraphRepository::update_positions`** — atomic per-node DELETE +
     INSERT for `vc:hasX/Y/Z` (plus velocities) over potentially 10k+
     nodes in one Update. Risk: SPARQL Update string size limits in
     Oxigraph (none documented but practical RocksDB write-batch ceilings
     apply); the implementation must chunk above N≈5000 nodes and emit
     multiple Updates, breaking the "all positions land or none do"
     DDD-11 invariant unless wrapped in a `Store::transaction`.

This audit blocks no scaffolding work. Implementation can begin against
the verified trait counts and the SPARQL templates above. Recommend the
Queen amend ADR-11 §D2 to `:assert` before any `oxigraph_*` adapter
writes the named-graph IRI as a string literal.
