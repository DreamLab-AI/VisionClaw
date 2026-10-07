---
id: ADR-2127
title: Ontology agent answers state their basis and never report "not asserted" as "false"
date: 2026-10-05
decision_status: accepted
implementation_status: partial
activation_status: staged
supersedes: []
superseded_by: []
verified_commit: f6a502e47a7b7f651c7376cb62559b75aa50b46b
verified_paths: [crates/visionclaw-ontology/src/open_world.rs, crates/visionclaw-ontology/src/lib.rs, crates/visionclaw-ontology/src/types/ontology_tools.rs, src/services/ontology_query_service.rs, src/handlers/ontology_agent_handler.rs, tests/ontology_agent_integration_test.rs]
owner: jjohare
review_trigger: the first entailed-false result (ADR-2125 disjointness landing); a change to the ontology-agent response types; an agent decision traced to an empty ontology result
repo: visionclaw
domain: BASELINE-architecture
---

# ADR-2127 — Ontology agent answers distinguish "not asserted" from "false"

## Context

The ontology agent surface (`src/handlers/ontology_agent_handler.rs:366-378`: discover, read, query, traverse, validate, status) returns rows and summaries (`crates/visionclaw-ontology/src/types/ontology_tools.rs:29-38,83-89,117-121`). Some types flag inference (`whelk_inferred`, `is_inferred`), but `QueryResult` and the traversal outputs do not, and no response says what an *empty* result means. The graph is open-world: no triple means the corpus does not say, not that the relationship is false. A closed-world reader — an agent, or a Cypher-trained developer — turns "no row" into "no", which the store cannot license (PRD-029 §4). Today no answer can be *entailed* false, because nothing is disjoint (ADR-2125). Once disjointness lands, three outcomes exist and the API can only express two.

## Decision

1. **Every fact carries a basis**: `asserted` (authored), `inferred` (Whelk closure), or `provenance` (named graph), on every row or edge the surface returns, including `QueryResult` rows.
2. **Membership and relation checks are tri-valued**: `entailed`, `entailed_false` (contradicts a disjointness or definition), or `not_asserted`. A boolean field is not used for these.
3. **Empty results are labelled.** An empty result carries `closure: "open"` and the generation it was computed against (ADR-2128 `versionIRI`), so the caller can tell "the corpus is silent at this generation" from "the answer is no".
4. **The response types are generated** (`cargo run --bin generate_types`); the client and agent consumers read the new fields rather than inferring them.

## Consequences

- Agents get the information needed to ask, defer or propose rather than conclude.
- It is a breaking shape change to agent-facing types; consumers and `tests/ontology_agent_integration_test.rs` move together.
- `entailed_false` stays unreachable until ADR-2125; shipping the field first is deliberate, so the shape does not change twice.

## Verification

Not implemented. Evidence of the current state at `c4570c081`: `ontology_tools.rs:117-121` (`QueryResult` has no basis or closure field). Complete when the integration test asserts an empty query returns `closure: "open"` with a generation id, and a fixture with one disjointness returns `entailed_false`.

### Implementation evidence (2026-10-05, uncommitted on `feat/prd-029-reasoning`)

- `CARGO_TARGET_DIR=/home/devuser/workspace/.cargo-target-prd029 cargo test -p visionclaw-ontology --lib open_world`: red first (the rules replaced by a closed-world-naive baseline: 3 passed, 10 failed), then the implementation: `cargo test -p visionclaw-ontology --lib` **139 passed, 0 failed** (126 baseline + 13 new `open_world::tests`).
- Root crate (`tests/ontology_agent_integration_test.rs`, 7 new ADR-2127 tests): `cargo test --test ontology_agent_integration_test` **cannot build in-container** (`visionclaw-gpu` build.rs: `PTX validation failed for semantic_forces`). Run on the host. In-container, the service and handler files were compiled verbatim (`#[path]`) in a scratch harness crate against the real `visionclaw-domain`, `-adapters` and `-ontology`, with the ADR-2127 test section extracted verbatim: red against the HEAD service (did not compile: `missing field basis` ×4), then **7 passed**, including `test_empty_discover_is_labelled_open_with_its_generation`, `test_membership_in_a_disjoint_class_is_entailed_false` and the HTTP test over `configure_ontology_agent_routes`. The pre-existing discovery/read/Cypher tests passed in the same harness (**9 passed**).
- Not met: decision 4. `cargo run --bin generate_types` only writes the hand-maintained settings interfaces (`client/src/types/generated/settings.ts`), and no ontology-agent type is generated, so running it would change nothing relevant. Generating these types needs a generator change. No client code consumes these types today.
- Breaking change: `QueryResult.rows` is now `Vec<QueryRow { values, basis }>` instead of bare maps (no producer existed). Every other change is additive with serde defaults (`basis` on `DiscoveryResult`, `RelationshipSummary`, `InferredAxiomSummary`, `RelatedNote` and `TraversalEdge`; `scope` on `TraversalResult` and `QueryResult` and in the discover/query JSON; new `POST /ontology-agent/check`).

- Review fixes (2026-10-05): (a) generation is derived per loaded ontology and refreshed on reload: `OntologyQueryService` caches its index keyed by an order-free fingerprint of the asserted axioms, class IRIs and Whelk closure, and on change resolves the generation from the bundle `ONTOLOGY_BUNDLE_DIR` pins, else the one `$VAULT_ROOT/vault.toml` names (`[build] out` + `[build.artifacts] ontology`/`generation`), else a non-null store digest `urn:visionclaw:generation:store:sha256-12-…` of what was loaded (caveat: a vault.toml bundle older than the loaded pages still reports its own versionIRI); `answer_scope()` is now async; (b) tri-valued relation check `open_world::RelationIndex` (asserted edge; inferred via subject/object subsumption and `SubPropertyOf`; `entailed_false` only from a declared domain/range the subject/object is entailed-disjoint with), exposed as `POST /ontology-agent/check` with optional `property` (`object` accepted for `class`) and `ontology_check_relation` in `/status`; (c) `version_iri_from_turtle` reads only the `owl:versionIRI` of a subject typed `owl:Ontology`, with a tokenizer that skips literals, comments and other subjects; (d) `read_note` uses `SubsumptionIndex::subject_view` (ancestors and descendants walked once) on the cached index and `traverse` lists classes once per walk. `cargo test -p visionclaw-ontology --lib` in place: red first (version-IRI test failed; new API did not compile), then 148 passed (139 + 9 new). Root crate via the scratch harness compiling the service and handler verbatim (`#[path]`): the new tests were red against reconstructed pre-fix copies (`generation is null` for production construction; missing `check_relation`/`with_bundle_location`), then 12 ADR-2127 tests passed (7 prior + 5 new) and the 9 pre-existing discovery/read/Cypher tests passed. Host run still owed: `cargo test --test ontology_agent_integration_test`.

- Defect fix 7 (2026-10-05): fixed `clippy::redundant_closure` at `open_world.rs:478` (`.and_then(expand)`). `cargo clippy -p visionclaw-ontology --no-deps --all-targets -- -D warnings 2>&1 | grep -c open_world` now prints 0. The remaining errors are pre-existing debt in other modules. `cargo test -p visionclaw-ontology --lib`: 148 passed.
- Defect fix 8, a cheap cache check (2026-10-05): the repository exposes no sound store revision. The Oxigraph `Store` is shared through `store()` with direct writers (GitHub sync, the decision handler, the mutation service), so a repository-side counter would serve a stale generation. `loaded()` therefore still reads the axioms, but it no longer materialises every class. The new `OntologyRepository::class_iris` (a default method, overridden in Oxigraph with a `SELECT DISTINCT ?s` probe) supplies the class set. The Whelk closure is keyed on the new `WhelkInferenceEngine::generation` counter, which is bumped on every write to the cached closure, instead of being cloned and hashed. The hierarchy is read only on a rebuild, under the same guard as the generation it is keyed by. In place, `cargo test -p visionclaw-adapters --lib` gives 78 passed (the 2 new tests were red first). The root service was checked in a /dev/shm scratch harness compiling `loaded`, `fingerprint_of`, `loaded_from` and `loaded_for` verbatim over an in-memory Oxigraph store. It shows no rebuild when nothing changed (`Arc::ptr_eq`), and a rebuild on a new axiom, a new class or a Whelk reload, with the answers following the change. On 9,000 classes with 2 KB bodies, the cache check per request fell from 378–413 ms to 135 ms. Host run still owed: `cargo test --test ontology_agent_integration_test`.

## Re-verification — 2026-10-07 (clippy sweep)

At f6a502e47, the `chore/clippy-sweep` branch (194ea20f0..f6a502e47) changes the governed paths for lint only: `crates/visionclaw-ontology/src/lib.rs`: crate-doc list indentation; `src/handlers/ontology_agent_handler.rs`: `split(..).last()` becomes `next_back()` (same element); `src/services/ontology_query_service.rs`: Levenshtein matrix initialised with iterator loops (same cells); `tests/ontology_agent_integration_test.rs`: rustfmt only. No decision-relevant behaviour changed. `cargo test --workspace --tests`: 3239 passed, 0 failed, 83 ignored (3260 at 194ea20f0; the 21 removed tests covered deleted dead modules outside this record). **Still holds.**
