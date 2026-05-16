# TEST-PLAN-11 — Port Parity Harness for Persistence Migration

## 1. Purpose

This document describes the **port parity test harness** at
`tests/adapter_parity/`. The harness is the runtime enforcement of
PRD-11 §A2:

> **A2. Port parity.** The current `OntologyRepository`,
> `GraphRepository`, and `SettingsRepository` trait surfaces (a
> combined 108+ methods) are implemented unchanged by the new adapters.
> No upstream caller — actor, handler, or service — changes by a
> single line as a consequence of the persistence swap.

Concretely: any future adapter that implements one of the three
persistence ports either **passes the harness** or **violates the trait
contract**. The harness has no opinion on the backend (Neo4j, Oxigraph,
SQLite, any plausible future swap to Jena/Stardog/etc.) — it asserts
behaviour at the trait boundary only.

The harness is **not** a benchmark, fault-injection suite, or
correctness checker for SPARQL/Cypher translation. Those live elsewhere
(PRD-11 §6 — acceptance evidence — covers the parity-count, latency,
and snapshot-size analytics paths separately).

## 2. How it works

### 2.1 Generic parity functions

For each port the harness exposes a set of generic async functions of
the shape:

```rust
pub async fn parity_<scenario><R: SomeRepository>(repo: R) { ... }
```

Each function:

- **Owns its data lifecycle.** It creates the inputs it needs, runs
  the assertions, and leaves no required cleanup state behind.
- **Asserts behaviour at the trait surface only.** It does not look
  at adapter internals. If the adapter routes through SPARQL or
  through Cypher or through `serde_json::to_string` it is the
  adapter's business; the harness sees `repo.add_owl_class(&c)` and
  expects `repo.get_owl_class(&c.iri)` to round-trip.
- **Is independently failable.** A regression in one scenario
  produces one failing test, not a cascade.

### 2.2 Concrete runners

A *runner* binds a battery of parity functions to a real adapter:

| Runner                               | Backend                              | Feature gate            |
|--------------------------------------|--------------------------------------|-------------------------|
| `runner_neo4j.rs`                    | Neo4j Bolt (`neo4j_*_repository.rs`) | `test-neo4j`            |
| `runner_oxigraph.rs`                 | Oxigraph + SQLite (scaffolded)       | `persistence-oxigraph`  |

Both runners use `#[tokio::test]` with `#[ignore]` (so default
`cargo test` doesn't fail in environments without the backend). They
share their parity functions via the generic functions in the
`adapter_parity` module — adding a third backend (say, an in-memory
mock for fast CI feedback) means writing only the runner.

The Neo4j runner is what gives us confidence that **the harness is
correct**. If a parity scenario passes against the existing in-
production adapter, that scenario is a faithful expression of the
trait contract.

### 2.3 Discovery layout

```
tests/
├── adapter_parity.rs               # top-level cargo-test binary (3 lines)
└── adapter_parity/
    ├── mod.rs                      # module declarations + builders
    ├── ontology_parity.rs          # 10 scenarios
    ├── graph_parity.rs             # 10 scenarios
    ├── settings_parity.rs          # 15 scenarios (covers 17 methods)
    ├── named_graph_invariants.rs   # 5 cross-port invariants
    ├── runner_neo4j.rs             # concrete runner (Neo4j)
    └── runner_oxigraph.rs          # concrete runner (Oxigraph + SQLite)
```

`tests/adapter_parity.rs` is the entry. Rust's integration-test
discovery only auto-picks up `.rs` files directly in `tests/`; the
`#[path]` declaration in `adapter_parity.rs` brings the
`adapter_parity/` directory into scope as a module tree.

## 3. Coverage

### 3.1 Ontology port — `tests/adapter_parity/ontology_parity.rs`

Ten scenarios covering all asserted behaviour on `OntologyRepository`:

| # | Scenario                                       | Methods exercised                                                  |
|---|------------------------------------------------|--------------------------------------------------------------------|
| 1 | `parity_owl_class_roundtrip`                   | `add_owl_class`, `get_owl_class`, `list_owl_classes`               |
| 2 | `parity_owl_class_idempotent_add`              | `add_owl_class` (twice), `list_owl_classes`                        |
| 3 | `parity_owl_class_remove`                      | `add_owl_class`, `remove_owl_class`, `get_owl_class`               |
| 4 | `parity_owl_property_roundtrip`                | `add_owl_property`, `get_owl_property`, `list_owl_properties`      |
| 5 | `parity_axiom_query`                           | `add_axiom`, `get_class_axioms` (subject and object positions)     |
| 6 | `parity_axiom_remove`                          | `add_axiom`, `remove_axiom`, `get_class_axioms`                    |
| 7 | `parity_inference_results`                     | `store_inference_results`, `get_inference_results`                 |
| 8 | `parity_validate_ontology`                     | `validate_ontology` (report shape, not validity)                   |
| 9 | `parity_query_ontology_returns_vec`            | `query_ontology` (trait shape only)                                |
|10 | `parity_class_vs_axiom_segregation`            | `add_owl_class`, `add_axiom`, `list_owl_classes`, `get_axioms`     |

### 3.2 Graph port — `tests/adapter_parity/graph_parity.rs`

Ten scenarios over `GraphRepository`:

| # | Scenario                                       | Methods exercised                                                  |
|---|------------------------------------------------|--------------------------------------------------------------------|
| 1 | `parity_add_nodes_empty`                       | `add_nodes` (degenerate input)                                     |
| 2 | `parity_add_nodes_returns_ids`                 | `add_nodes`, `get_node_map`                                        |
| 3 | `parity_add_edges`                             | `add_nodes`, `add_edges`, `get_graph`                              |
| 4 | `parity_update_positions`                      | `update_positions`, `get_node_positions`                           |
| 5 | `parity_get_graph_membership`                  | `add_nodes`, `get_graph`                                           |
| 6 | `parity_get_node_map`                          | `add_nodes`, `get_node_map`                                        |
| 7 | `parity_dirty_nodes_lifecycle`                 | `update_positions`, `get_dirty_nodes`, `clear_dirty_nodes`         |
| 8 | `parity_physics_state_query`                   | `get_physics_state`, `get_equilibrium_status`, `get_constraints`, `get_auto_balance_notifications` |
| 9 | `parity_knowledge_vs_agent_isolation`          | `add_nodes`, `get_graph`, `get_bots_graph`                         |
|10 | `parity_shortest_path_smoke`                   | `compute_shortest_paths`                                           |

### 3.3 Settings port — `tests/adapter_parity/settings_parity.rs`

Coverage for **all 17 methods** of `SettingsRepository`. Two methods
get composite coverage (per-user resolution and schema_version
round-trip):

| Method                       | Scenario                                          |
|------------------------------|---------------------------------------------------|
| `get_setting`                | `parity_get_setting_missing`, `parity_set_get_all_variants` |
| `set_setting`                | `parity_set_get_all_variants`                     |
| `delete_setting`             | `parity_delete_setting`, `parity_delete_idempotent` |
| `has_setting`                | `parity_has_setting`                              |
| `get_settings_batch`         | `parity_batch_set_then_get`, `parity_batch_get_partial` |
| `set_settings_batch`         | `parity_batch_set_then_get`                       |
| `list_settings`              | `parity_list_settings_prefix`                     |
| `load_all_settings`          | `parity_load_save_all_settings`, `parity_schema_version_roundtrip` |
| `save_all_settings`          | `parity_load_save_all_settings`, `parity_schema_version_roundtrip` |
| `get_physics_settings`       | `parity_physics_profile_lifecycle`, `parity_physics_missing_profile` |
| `save_physics_settings`      | `parity_physics_profile_lifecycle`                |
| `list_physics_profiles`      | `parity_physics_profile_lifecycle`                |
| `delete_physics_profile`     | `parity_physics_profile_lifecycle`                |
| `export_settings`            | `parity_export_import_roundtrip`                  |
| `import_settings`            | `parity_export_import_roundtrip`                  |
| `clear_cache`                | `parity_clear_cache` (asserts data survives)      |
| `health_check`               | `parity_health_check`                             |

Plus:

- `parity_schema_version_roundtrip` (composite) — exercises ADR-11
  §D5's non-conflation rule between the *document* version field
  (`AppFullSettings.version`) and the SQLite-side `schema_migrations`
  table.
- `parity_per_user_isolation_with_context` (composite, opt-in) — the
  runner provides three closures (one per principal: Alice, Bob,
  global) and the harness sequences the writes. The runner asserts
  the resolution rule (ADR-11 §D5, PRD-11 §A5):

    - Read for `(K, U)` returns `(K, U)` if present, else `(K, NULL)`,
      else `NotFound`.
    - Write by `U` is invisible to other `U`s and to NULL.
    - Cross-user reads do not see each other.

  Adapters that have not yet implemented per-user resolution skip
  this composite scenario; they still must pass the global portion of
  every other settings scenario.

### 3.4 Cross-port named-graph invariants — `named_graph_invariants.rs`

Five invariants. Each takes one or two ports as arguments and asserts
a structural property that maps to ADR-11 §D2's named-graph layout:

| # | Invariant                                                 | Ports involved                          |
|---|-----------------------------------------------------------|-----------------------------------------|
|I1 | OntologyClass write does NOT pollute the knowledge graph  | `OntologyRepository`, `GraphRepository` |
|I2 | KGNode write does NOT pollute the ontology class list     | `OntologyRepository`, `GraphRepository` |
|I3 | Knowledge graph and agent graph are isolated              | `GraphRepository` (both views)          |
|I4 | Inferred axiom rewrite does NOT touch the asserted graph  | `OntologyRepository`                    |
|I5 | Removing an OntologyClass does NOT cascade to a same-IRI KGNode | `OntologyRepository`, `GraphRepository` |

I5 is the structural fix for the migration-0045 orphan-stub class of
bug (PRD-11 §2, ADR-11 §D2). The Neo4j adapter handles it via label
discipline; the Oxigraph adapter handles it via named-graph
segregation. The harness asserts the **observable behaviour** is the
same.

## 4. How to run

### 4.1 Default: harness compiles, tests are `#[ignore]`d without a backend

```bash
cargo test --test adapter_parity
```

Outputs nothing executable. This is the build gate: a refactor of
`src/ports/*.rs` that breaks the trait surface will fail compilation
here long before any test runs.

### 4.2 Against the Neo4j adapter (proves harness correctness)

```bash
docker compose up -d neo4j
export NEO4J_PASSWORD=test ALLOW_INSECURE_DEFAULTS=true
cargo test --features test-neo4j --test adapter_parity -- --include-ignored
```

Expected: every scenario in `runner_neo4j.rs` passes. If a scenario
fails here, fix the harness — the existing Neo4j adapter is the
truth.

### 4.3 Against the Oxigraph adapter (once scaffolded)

```bash
cargo test --features persistence-oxigraph --test adapter_parity -- --include-ignored
```

While the adapters are still being scaffolded the runner emits a
`compile_error!` pointing at the wiring procedure. Once the adapters
land:

1. Delete the `compile_error!` line in `runner_oxigraph.rs`.
2. Uncomment the import / factory / test bodies.
3. Drop the `#[ignore]` attributes once CI provisions the feature.

## 5. What is intentionally out of scope

The harness is **a behavioural contract test**. It is deliberately
not:

- **A performance benchmark.** SPARQL p50/p99 measurement lives in
  `tests/performance/` and the migration tool's parity-table emitter.
  The harness measures correctness, not speed.
- **A fault-injection suite.** Network partition, disk full, RocksDB
  WAL corruption, SQLite WAL on NFS — all of these are deployment
  concerns mitigated by ADR-11 §R3/§R5/§R6 and tested elsewhere.
- **A SPARQL ↔ Cypher translation checker.** Every Cypher migration
  has a paired SPARQL Update file (ADR-11 §D7) and a per-predicate
  parity test in the migration tool. The trait harness doesn't see
  the query language.
- **A schema-migration test.** The `schema_migrations` table is
  exercised by the migration tool's own integration tests under
  `tools/migrate-neo4j-to-oxigraph/tests/`.
- **A whelk-inference correctness test.** `whelk-rs` has its own
  test suite. The harness asserts only that `store_inference_results`
  → `get_inference_results` round-trips the result set the inference
  engine produces, not that the result set is logically correct.
- **A `SettingsRepository` semantic test.** Whether
  `physics.gravity_strength = -1.0` is a valid value is owned by
  Section 5 (Settings & Control Panel) and tested in
  `tests/settings/`. The persistence harness only asserts that
  whatever value is stored is what comes back.

## 6. Adding new parity scenarios

When a new method appears on one of the three ports:

1. Add a `parity_<scenario>` function in the appropriate file
   (`ontology_parity.rs`, `graph_parity.rs`, `settings_parity.rs`).
   Keep it generic over `R: SomeRepository`.
2. Append it to the `run_all` aggregator in the same file.
3. Add a row to §3 of this document.

If the new behaviour spans two ports (e.g. a write in one port must
be observable in the other), put it in `named_graph_invariants.rs`
and add a row to §3.4.

When a new adapter backend appears:

1. Add a `runner_<name>.rs` next to the existing runners.
2. Expose adapter factories under a feature gate.
3. Bind the existing parity functions to those factories — DO NOT
   write new test cases. The parity contract is shared across all
   backends.
4. Add a row to the §2.2 runner table.

## 7. Failure-mode interpretation

| Failure surface                                  | First thing to check                                |
|--------------------------------------------------|-----------------------------------------------------|
| Neo4j runner fails a scenario                    | The harness is wrong. Fix the scenario.             |
| Oxigraph runner fails a scenario Neo4j passes    | The Oxigraph adapter violates the trait contract.   |
| Both runners fail the same scenario              | The scenario hit an under-specified part of the trait. Pin the contract first, then make both pass. |
| `compile_error!` in `runner_oxigraph.rs`         | Oxigraph adapters not yet scaffolded. See the file header. |
| `#[ignore]`d tests in default `cargo test` run   | Expected — the runners need their backend.          |
| Trait change breaks the harness compilation      | Working as intended. The harness is a build gate.   |

## 8. References

- PRD-11 — `docs/migration-sprint/11-persistence-migration/PRD-11.md`
- ADR-11 — `docs/migration-sprint/11-persistence-migration/ADR-11.md`
- DDD-11 — `docs/migration-sprint/11-persistence-migration/DDD-11.md`
- `src/ports/ontology_repository.rs` — `OntologyRepository` trait
- `src/ports/graph_repository.rs`    — `GraphRepository` trait
- `src/ports/settings_repository.rs` — `SettingsRepository` trait
