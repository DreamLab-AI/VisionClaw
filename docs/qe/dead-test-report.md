# Dead & Flaky Test Report

**Date**: 2026-05-09
**Scope**: All 5 DreamLab substrates
**Total tests scanned**: ~3,478 Rust tests + 48 JS/TS test files

---

## 1. Dead Tests -- Tests That Test Nothing

### 1.1 Empty Test Bodies (VisionClaw Rust)

These tests compile and "pass" but contain zero logic, zero assertions.
They inflate test counts while verifying nothing.

| File | Line | Function | Verdict |
|------|------|----------|---------|
| `src/actors/event_coordination.rs` | 148 | `test_event_coordinator_creation` | **DELETE** -- empty async fn body |
| `src/utils/network/connection_pool.rs` | 636 | `test_pooled_connection_validation` | **DELETE** -- empty async fn body |
| `src/adapters/actor_graph_repository.rs` | 249 | `test_repository_construction` | **DELETE** -- empty fn body |

### 1.2 Placeholder Integration Tests (VisionClaw Rust)

These are comment-only stubs marked `#[ignore]`. They were never implemented
and have no code path -- only comments describing what they "would" test.

| File | Line | Function | Verdict |
|------|------|----------|---------|
| `src/adapters/tests/neo4j_tests.rs` | 2002 | `test_neo4j_adapter_integration` | **DELETE** -- only comments, no code |
| `src/adapters/tests/neo4j_tests.rs` | 2022 | `test_neo4j_graph_repository_integration` | **DELETE** -- single-line comment |
| `src/adapters/tests/neo4j_tests.rs` | 2029 | `test_neo4j_settings_repository_integration` | **DELETE** -- single-line comment |
| `src/adapters/tests/neo4j_tests.rs` | 2036 | `test_neo4j_ontology_repository_integration` | **DELETE** -- single-line comment |

### 1.3 Compile-Only / Construction-Only Tests

These tests verify only that a type can be constructed (no `assert!`). The
contract "construction does not panic" has marginal value when the type
derives `Default`.

| Substrate | File | Line | Function | Verdict |
|-----------|------|------|----------|---------|
| nostr-rust-forum | `crates/nostr-bbs-relay-worker/tests/moderation_tests.rs` | 62 | `mod_cache_default_is_empty` | **KEEP with caveat** -- documents panic-freedom contract; add `assert!` on `.len()` or similar |
| nostr-rust-forum | `crates/nostr-bbs-relay-worker/tests/moderation_tests.rs` | 68 | `mod_cache_new_returns_empty_cache` | **MERGE** into above test -- duplicates default() vs new() for same assertion-free pattern |
| solid-pod-rs | `crates/solid-pod-rs-activitypub/tests/sprint12_ap_features.rs` | 71 | `handle_outbox_post_exists` | **KEEP** -- intentional compile-gate test (`let _ = handle_outbox_post`), prevents silent API removal. Well-documented intent. |

**Total dead tests: 10** (7 DELETE, 1 MERGE, 2 KEEP-with-improvement)

---

## 2. Stale #[ignore] Tests

### 2.1 Neo4j Integration Tests (VisionClaw) -- 15 tests

All require a live Neo4j instance. The project runs Neo4j in Docker (`docker compose`).

| File | Count | Reason | Assessment |
|------|-------|--------|------------|
| `src/adapters/neo4j_settings_repository.rs` | 4 | "Requires Neo4j instance" | **CONVERT to CI integration test** -- Neo4j is available in Docker; add a `--features integration` gate or `cargo test -- --ignored` CI step |
| `src/adapters/tests/neo4j_settings_repository_tests.rs` | 7 | "Requires live Neo4j instance" | Same as above |
| `src/adapters/tests/neo4j_tests.rs` | 4 | "Requires live Neo4j instance" | 4 of these are empty placeholders (see Section 1.2) -- DELETE those; the others need CI |

**Recommendation**: Create a CI job that starts Neo4j via `docker compose`, runs `cargo test -- --ignored`, tears down. This converts 11 real integration tests from permanently-ignored to actually-running. Effort: ~2h.

### 2.2 CUDA/GPU Tests (VisionClaw) -- 6 tests

| File | Count | Reason | Assessment |
|------|-------|--------|------------|
| `src/gpu/memory_manager.rs` | 6 | "Requires CUDA device" | **KEEP ignored** -- legitimate hardware dependency. These can only run on GPU nodes. Consider a `gpu` CI runner label if GPU CI becomes available. |

### 2.3 Environment Variable Tests (VisionClaw) -- 5 tests

| File | Count | Reason | Assessment |
|------|-------|--------|------------|
| `src/services/server_identity.rs` | 4 | Process-global env var mutation races | **KEEP ignored** -- legitimate `--test-threads=1` requirement. Document the exact run command in CI comments. |
| `src/services/nostr_bead_publisher.rs` | 1 | Same env-var race issue (PRD-010 F1) | **KEEP ignored** -- same category |

### 2.4 solid-pod-rs -- 1 test

| File | Function | Reason | Assessment |
|------|----------|--------|------------|
| `crates/solid-pod-rs-didkey/tests/upstream_vectors/all_fixtures.rs` | `did_doc_emitter_matches_canonical_shape` | "Phase 2" ADR-074 D2 | **KEEP** -- explicitly deferred to Phase 2 of the mega-sprint. Review when Phase 2 completes. |

**Summary**: 27 ignored tests total. 11 can be converted to CI integration tests. 12 are legitimately ignored (hardware/env). 4 are dead placeholders (delete).

---

## 3. Orphaned Test Fixtures

### 3.1 VisionClaw `tests/fixtures/ontology/`

| File | Referenced? | Verdict |
|------|------------|---------|
| `tests/fixtures/ontology/test_graph.json` | 0 references in `src/` or `tests/` | **DELETE** -- `sample_graph.json` is the active fixture |
| `tests/fixtures/ontology/test_ontology.ttl` | 0 references | **DELETE** -- `sample.ttl` is the active fixture |
| `tests/fixtures/ontology/test_ontology.rdf` | 0 references | **DELETE** -- no RDF parser test loads this |
| `tests/fixtures/ontology/test_constraints.toml` | 0 references | **DELETE** -- `test_mapping.toml` is used instead |

### 3.2 VisionClaw `docs/specs/fixtures/`

All 13 JSON fixture files are referenced in test code. **No orphans.**

### 3.3 VisionClaw Client

| File | Issue | Verdict |
|------|-------|---------|
| `client/src/position_diff_test.cjs` | Node.js diagnostic script in `src/`, not a vitest test | **MOVE** to `client/scripts/` or `client/tools/` |
| `client/src/proxy_probe.cjs` | Node.js diagnostic script in `src/`, not a vitest test | **MOVE** to `client/scripts/` or `client/tools/` |

### 3.4 solid-pod-rs

All fixture files in `crates/solid-pod-rs/tests/fixtures/` are referenced. **No orphans.**

**Total orphaned fixtures: 4 files to delete, 2 files to relocate.**

---

## 4. Duplicate Tests

### 4.1 CRITICAL: Triplicated `cosine_similarity` Function + Tests

Three separate files define an identical `pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32`
with identical test suites:

| File | Function | Tests |
|------|----------|-------|
| `src/handlers/discovery_handler.rs:178` | `cosine_similarity` | 4 tests (identical, opposite, orthogonal, zero_vector) |
| `src/services/kge_trainer.rs:535` | `cosine_similarity` | 4 tests (identical, opposite, orthogonal) -- 3 of 4 |
| `src/services/embedding_service.rs:534` | `cosine_similarity` | 4 tests (identical, opposite, orthogonal, zero_vector) |

**This is not just duplicate tests -- the production function itself is triplicated.**

**Recommendation**: Extract `cosine_similarity` to a shared `src/utils/math.rs` module. Delete
the copies. Move one set of tests to the canonical location. Effort: ~1h. Removes ~12 duplicate tests
and ~30 lines of duplicate production code.

### 4.2 Neo4j Settings Repository: Inline vs External Test Overlap

| Inline (`neo4j_settings_repository.rs`) | External (`neo4j_settings_repository_tests.rs`) | Overlap |
|----------------------------------------|------------------------------------------------|---------|
| `test_neo4j_settings_repository` (ignored) | `test_neo4j_settings_repository_connection` + `_health_check` (ignored) | Partial -- inline test covers CRUD, external splits connection/health |
| `test_user_management` (ignored) | `test_neo4j_user_management` (ignored) | **High** -- both test user create/get |
| `test_user_filter` (ignored) | `test_neo4j_user_filter_operations` (ignored) | **High** -- both test filter CRUD |

The external test file has 43 tests (36 unit + 7 integration). The inline module has 4 tests (all integration).

**Recommendation**: Delete the 4 inline integration tests from `neo4j_settings_repository.rs`.
The external test file is more comprehensive and already covers the same scenarios. Effort: ~15min.

### 4.3 Same-Named Tests in Different Modules (VisionClaw)

These are NOT exact duplicates -- they test different structs/implementations with the same test function
name. No action needed, but listed for awareness:

- `test_actor_creation` -- 2 copies (OntologyConstraintActor vs TaskOrchestratorActor) -- **OK, different subjects**
- `test_custom_config` -- 3 copies (LshConfig vs LODConfig vs TranslationConfig) -- **OK, different subjects**
- `test_default_config` -- 2 copies (SemanticPhysicsConfig vs NHopConfig) -- **OK, different subjects**
- `test_disjoint_classes_translation` -- 3 copies (OntologyConstraintTranslator vs AxiomMapper vs SemanticAxiomTranslator) -- **OK, different subjects** but review whether these modules should share a test trait

### 4.4 Cross-Substrate Helper Duplication

`test_keypair()` / `test_signing_key()` / `test_key()` helper functions are duplicated
across nostr-rust-forum (5 copies) and solid-pod-rs (3 copies). These are private helper
functions within test modules, not tests themselves. The ADR-082 fixture-sharing initiative
should eventually consolidate these into shared test crate fixtures.

**No immediate action** -- this is tracked in the mega-sprint Phase 4+ roadmap.

---

## 5. Test File Naming & Location Issues

### 5.1 Misplaced Files

| File | Issue | Recommendation |
|------|-------|----------------|
| `client/src/position_diff_test.cjs` | Node.js script with `_test` suffix but no vitest harness; lives in `src/` | Move to `client/scripts/position_diff_test.cjs` |
| `client/src/proxy_probe.cjs` | Node.js diagnostic script in `src/` | Move to `client/scripts/proxy_probe.cjs` |

### 5.2 Assertion-Free Playwright Test

| File | Issue | Recommendation |
|------|-------|----------------|
| `dreamlab-ai-website/tests/login-deep.spec.ts` | 2 test blocks, 0 `expect()` assertions. Uses `console.log()` output and screenshots as the only "verification". | **Add assertions**: check `page.url()` post-login, verify authenticated state element exists, check for error absence. Current form is a manual debugging script masquerading as an automated test. |

---

## 6. Recommended Cleanup Actions

| Priority | Action | Files | Effort | Impact |
|----------|--------|-------|--------|--------|
| P1 | Delete 3 empty-body tests | 3 files | 10 min | Removes false test count inflation |
| P1 | Delete 4 placeholder neo4j tests | 1 file | 5 min | Removes dead code |
| P1 | Delete 4 orphaned ontology fixtures | 4 files | 5 min | Reduces confusion |
| P2 | Extract `cosine_similarity` to shared module, deduplicate | 3 files | 1h | Removes ~12 duplicate tests + ~30 LOC duplicate prod code |
| P2 | Delete 4 inline neo4j integration tests (covered by external file) | 1 file | 15 min | Removes duplication |
| P2 | Add assertions to `login-deep.spec.ts` | 1 file | 30 min | Converts debug script to real test |
| P2 | Move 2 `.cjs` diagnostic scripts out of `client/src/` | 2 files | 10 min | Correct directory placement |
| P3 | Create Neo4j CI integration test job | CI config | 2h | Un-ignores 11 integration tests |
| P3 | Add assertions to nostr-rust-forum ModCache tests | 1 file | 15 min | Strengthens construction tests |
| P3 | Document `--test-threads=1` env-var test run command | 2 files | 10 min | Aids future developers |

**Total effort**: ~4.5 hours for all actions.
**Quick wins (P1)**: ~20 minutes to delete 7 dead tests + 4 orphaned fixtures.

---

## 7. Per-Substrate Summary

| Substrate | Tests | Dead | Ignored | Orphan Fixtures | Duplicates | Score |
|-----------|-------|------|---------|-----------------|------------|-------|
| VisionClaw Rust | 1,374 | 7 | 27 | 4 | 16 | Needs cleanup |
| VisionClaw Client | 42 files | 0 | 0 | 2 misplaced | 0 | Good (naming issues only) |
| nostr-rust-forum | 986 | 2 | 0 | 0 | 0 | Clean |
| solid-pod-rs | 1,118 | 1 | 1 | 0 | 0 | Clean |
| dreamlab-ai-website | 6 files | 0 | 0 | 0 | 0 | Needs assertions in login test |

**Overall reliability**: The vast majority of tests across all substrates are well-structured with
proper assertions. The issues are concentrated in VisionClaw Rust, specifically in the Neo4j adapter
layer, GPU tests, and a triplicated utility function. solid-pod-rs and nostr-rust-forum are clean.
