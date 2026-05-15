# PRD-QE-001: Quality Engineering for VisionClaw ↔ Agentbox Integration

**Status:** Draft
**Author:** QE / Architecture Agent
**Date:** 2026-04-27
**Priority:** P0 — paired with PRD-006; ships as gate, not as follow-up
**Pairs with:** PRD-006 (URI federation), PRD-005 (v2 pipeline), DDD-BC20 (anti-corruption layer)
**Companion ADR:** [ADR-062](adr/ADR-062-qe-prd-adr-ddd-graph-scaffolding.md)
**DDD Context:** [QE Traceability Graph (BC21)](ddd-qe-traceability-graph-context.md)

---

## 1. Problem Statement

The parallel-swarm audit on 2026-04-27 found that the surfaces under PRD-006's scope are **largely untested or have disabled tests**:

| Surface | State | Severity |
|---|---|---|
| `ontology_parser` | All tests disabled (`tests/ontology_parser_test.rs:1-35`) | P0 |
| `knowledge_graph_parser` | Zero tests | P1 |
| `github_sync_service` | Critical regression test commented out (`tests/CRITICAL_github_sync_regression_test.rs:31-77`) | P0 |
| `neo4j_adapter` | Tests reference removed module (`tests/neo4j_settings_repository_tests.rs:1-18`); 36 `#[ignore]` | P0 |
| `force_compute_actor` | Zero dedicated tests | P1 |
| `broadcast_optimizer` | Zero tests | P1 |
| `binary_protocol` | 10 tests covering bit29 + opacification only — **no v2-field survival** | P1 |
| `bots_handler` | Zero tests | P1 |
| `automation_orchestrator_actor` | Zero tests across 900+ LoC | P1 |
| URI/JSON-LD on VC side | Zero tests; agentbox has 7 grammar tests in isolation | P0 |
| Cross-substrate (BC20) | Zero tests; agentbox contract specs are JS-only and never boot a VC adapter | P0 |
| GPU→Binary v2-field determinism | Zero tests | P0 |
| CI gates | `.github/workflows/` runs no `cargo test`, no agentbox tests, no integration | P1 |

The integration cannot ship without QE coverage that exercises the **actual data flows** end-to-end. This PRD specifies the test surface required to ship PRD-006 with confidence.

---

## 2. Goals

| # | Goal | Success Metric |
|---|------|----------------|
| Q1 | All disabled tests are restored OR explicitly deleted with ADR | Zero `#[ignore]`-without-justification, zero commented-out test bodies |
| Q2 | Every v2 field has a unit test for parse + persist + load | `iri/uri/rdf_type/same_as/content_hash/quality_score/authority_score` × parser/Neo4j matrix is green |
| Q3 | End-to-end determinism: a fixture page round-trips through all stages | Single `e2e_v2_field_survival.rs` test boots Neo4j fixture, ingests a fixture page, restarts the graph actor, fetches via `/api/v1/uri/<urn>`, asserts every field value |
| Q4 | URI grammar contract test on VC side mirrors agentbox's | `tests/contract/uri_grammar.rs` covers mint determinism, parse round-trip, validation rejections, R1/R2/R3 invariants |
| Q5 | BC20 ACL contract harness | Each of six ACL modules has a `tests/contract/acl_<slot>.rs` exercising `to_visionclaw` and `to_agentbox` round-trips against canonical fixtures shared with agentbox |
| Q6 | Cross-substrate integration test boots both substrates | `tests/integration/bc20_federation.rs` brings up an agentbox sibling via testcontainers, runs the `/v1/meta` handshake, spawns an agent, asserts the event arrives via WS |
| Q7 | Live stdio bridge soak test | A 30-min synthetic load (1000 spawns, 50 concurrent) shows zero deadlocks, no stdio buffer overrun, p95 event latency <250ms |
| Q8 | CI gates the work | `.github/workflows/integration-ci.yml` runs cargo test (with Neo4j fixture), agentbox JS contract tests, and the cross-substrate suite — and blocks merge on red |
| Q9 | Coverage targets | Line coverage ≥75% for `src/uri/`, `src/bc20/`, `src/handlers/{uri_resolver,agent_events,jsonld}_handler.rs`; ≥85% for the ACL modules |
| Q10 | Mutation testing baseline | `cargo mutants` on `src/uri/` and `src/bc20/acl/` survives ≤10% of mutants |

---

## 3. Non-Goals

- E2E browser tests for the linked-object viewer (S12). Out of scope; covered separately when S12 lands.
- Performance regression tests for the GPU pipeline. Existing `physics_data_flow_test.rs` is sufficient.
- Property tests across the entire codebase. Property tests are scoped to the URI grammar and content-hash determinism only.
- Replacing the existing `bit29_on_wire.rs` opacification tests. Those stay.
- Test infrastructure for the unrelated MCP/MAD migration (covered in PRD-004).

---

## 4. Test Surface Inventory

### 4.1 Unit / Property tests

| Test | Location | Purpose | Type |
|---|---|---|---|
| `uri_mint_determinism.rs` | `tests/unit/uri/` | same payload → same URN, every kind | Property (proptest) |
| `uri_parse_roundtrip.rs` | `tests/unit/uri/` | `parse(mint(x)) == x` | Property |
| `uri_owner_scope_required.rs` | `tests/unit/uri/` | owner-scoped kinds reject empty pubkey | Unit |
| `uri_pubkey_normalisation.rs` | `tests/unit/uri/` | hex / `did:nostr:` / `npub1...` all normalise to bare hex | Unit |
| `uri_content_hash_format.rs` | `tests/unit/uri/` | hash form `sha256-12-<12 lowercase hex>`; rejects 11/13/uppercase | Property |
| `uri_clippy_lint.rs` | `tests/unit/uri/` | confirms forbidden `format!("urn:visionclaw:...")` outside `src/uri/` fails compile | Compile-fail |
| `ontology_parser_v2.rs` | `tests/unit/parsers/` (RESTORE) | every v2 field extracted from fixture markdown | Unit |
| `kg_parser_v2.rs` | `tests/unit/parsers/` (NEW) | `iri/uri/content_hash` survive `KnowledgeGraphParser::parse` | Unit |
| `neo4j_v2_roundtrip.rs` | `tests/integration/neo4j/` | write Node with all v2 fields, read back, assert identity | Integration (testcontainer) |
| `kgnode_ontology_merge.rs` | `tests/integration/neo4j/` | a v2 page produces both `:KGNode` and `:OntologyClass` linked by `[:HAS_ONTOLOGY]` | Integration |
| `did_nostr_wrap_at_boundary.rs` | `tests/contract/identity/` | every API/WS handler wraps `owner_pubkey` as `did:nostr:` | Snapshot + contract |
| `uri_resolver_307.rs` | `tests/contract/resolver/` | `GET /api/v1/uri/<urn>` returns 307 with correct Location | Contract |
| `uri_resolver_404.rs` | `tests/contract/resolver/` | unknown well-formed URN returns 404 with grammar hint | Contract |
| `uri_resolver_400.rs` | `tests/contract/resolver/` | malformed URN returns 400 | Contract |
| `jsonld_endpoint_shape.rs` | `tests/contract/resolver/` | `/api/v1/nodes/{id}/jsonld` matches PRD-005 §5.4.2 fixture | Contract (golden file) |
| `events_acl_translation.rs` | `tests/contract/acl/` | every agentbox event JSONL line maps to a VC `AgentEvent` or raises `UnmappedAgentboxPayload` | Contract |
| `uris_acl_translation.rs` | `tests/contract/acl/` | `urn:agentbox:thing:visionclaw:*` ↔ `urn:visionclaw:concept:*` round-trips | Contract |
| `beads_acl_translation.rs` | `tests/contract/acl/` | bead command shapes ↔ `BeadProvenance` aggregate | Contract |
| `pods_acl_translation.rs` | `tests/contract/acl/` | LDP container ↔ pod artefact URI | Contract |
| `memory_acl_translation.rs` | `tests/contract/acl/` | generic vector → `personal-context|project-state|patterns` namespace | Contract |
| `orchestrator_acl_translation.rs` | `tests/contract/acl/` | stdio spawn ↔ actor spawn | Contract |
| `agentbox_compat_versions.rs` | `tests/contract/handshake/` | every `adapter_contract_versions` value in agentbox `/v1/meta` intersects VC's compat range | Contract |

### 4.2 Integration / cross-substrate

| Test | Location | Purpose |
|---|---|---|
| `e2e_v2_field_survival.rs` | `tests/e2e/` | spin Neo4j; ingest fixture page; restart graph actor; fetch `/api/v1/uri/<urn>`; assert all v2 fields. Forms the **gate** for PRD-006 P1 |
| `bc20_federation_session.rs` | `tests/e2e/` | bring up agentbox via testcontainers; run `/v1/meta` handshake; assert `FederationSessionStarted` event + bindings |
| `local_fallback_probe.rs` | `tests/e2e/` | start agentbox in `client` mode pointing at a mock that fails Ed25519 sig; assert `LocalFallbackProbeFailed` event + session quarantine |
| `agent_spawn_event_stream.rs` | `tests/e2e/` | client POSTs `/bots/spawn-agent-hybrid`; assert `Spawned`, `ToolUsed`, `Completed` events arrive via WS within 1s of agentbox emission; assert ordering |
| `stdio_bridge_soak.rs` | `tests/e2e/` (`#[ignore = "soak; nightly"]`) | 1000 spawns / 50 concurrent / 30 min; zero deadlocks, p95 <250ms event latency, no stdio buffer pressure |
| `cross_substrate_uri_resolve.rs` | `tests/e2e/` | mint a `urn:visionclaw:concept:*`; resolve via agentbox `/v1/uri/urn:agentbox:thing:visionclaw:*`; assert federation-hop 307 chain terminates at VC `/api/v1/uri/...` |
| `jsonld_round_trip.rs` | `tests/e2e/` | JSON-LD `expand → compact` is identity for every emitted node, against the pinned visionclaw-v2 context |

### 4.3 Determinism / GPU pipeline

| Test | Location | Purpose |
|---|---|---|
| `gpu_metadata_passthrough.rs` | `tests/e2e/` | physics tick on fixture; assert v2 fields unchanged on the `Node` after force_compute (negative test — GPU must not stomp metadata) |
| `binary_protocol_v2_fields_absent.rs` | `tests/contract/binary_protocol/` | confirms binary frame is unchanged size; v2 fields are NOT in the binary stream (they ride JSON sidechannel only) — guards against accidental binary-protocol bloat |

### 4.4 Mutation testing

`cargo mutants --in-diff` runs per-PR on `src/uri/` and `src/bc20/acl/`. Threshold: ≤10% mutant survival before merge.

### 4.5 Restored / deleted tests

| Existing test | Action |
|---|---|
| `tests/ontology_parser_test.rs:1-35` | RESTORE; rewrite against current `OntologyParser` API |
| `tests/CRITICAL_github_sync_regression_test.rs:31-77` | RESTORE; replace stale mock types with real `Neo4jOntologyRepository` testcontainer fixture |
| `tests/neo4j_settings_repository_tests.rs:1-18` | DELETE; module was removed in ADR-001 restructuring |
| `tests/neo4j_settings_integration_tests.rs` | DELETE; same reason |
| 36 `#[ignore]` tests across the suite | TRIAGE: each gets RESTORE / DELETE decision recorded in `tests/IGNORE_TRIAGE.md`; no `#[ignore]` survives without a written reason |

---

## 5. Test Infrastructure

### 5.1 Neo4j fixture

`tests/fixtures/neo4j.rs` brings up `neo4j:5-community` via `testcontainers-rs` with:
- pre-applied indexes (`kg_node_canonical_iri`, `kg_node_owl_class`, `kg_node_domain`, `kg_node_quality_score`)
- 10 fixture markdown pages spanning v2 ontology + plain knowledge-graph + working-graph

### 5.2 Agentbox sibling fixture

`tests/fixtures/agentbox.rs` brings up the agentbox container in `client` federation mode:
- mounted shared docker network `visionclaw_network`
- `/v1/meta` returns deterministic `image_hash` from build digest
- adapter slots wired to mock VC `AdapterEndpoint`s with registered Ed25519 keys
- p95 boot ≤30s on CI workers

### 5.3 Shared canonical fixtures

`tests/fixtures/canonical/` holds JSON-LD payloads, agentbox event JSONL lines, and bead commands consumed by **both** VC contract tests AND agentbox JS contract tests. The fixtures are the single source of truth for the wire shape; either side adding a new field requires touching the fixture before tests pass.

### 5.4 Visionclaw context document

`docs/schema/visionclaw-ontology-schema-v2.jsonld` is committed to the repo; tests against pinned context resolve to this file directly (no network), matching the agentbox FOD-everything pattern.

### 5.5 Property test corpus

`tests/property/corpus/` holds the seed corpus for proptest URI fuzzing — 200 hand-crafted edge cases covering: empty string, max-length slug, Unicode boundary, hex/non-hex pubkey, prefix collisions.

---

## 6. CI Gates

### 6.1 New workflow `.github/workflows/integration-ci.yml`

```yaml
on: [pull_request, push]
jobs:
  rust-unit:
    runs-on: ubuntu-latest
    steps:
      - cargo test --lib --bins
  rust-integration:
    services: { neo4j: { image: neo4j:5-community, ports: [7687] } }
    steps:
      - cargo test --test '*' -- --include-ignored --skip soak
  agentbox-contract:
    runs-on: ubuntu-latest
    steps:
      - cd agentbox && npm test -- tests/contract/
  cross-substrate:
    needs: [rust-unit, agentbox-contract]
    services: { neo4j: ..., agentbox-sibling: ... }
    steps:
      - cargo test --test 'e2e_*' --test 'bc20_*' --test 'cross_substrate_*'
  mutation:
    if: github.event_name == 'pull_request'
    steps:
      - cargo mutants --in-diff --baseline auto --error-on-survival
```

### 6.2 Gate matrix

| Stage | Required for merge |
|---|---|
| `rust-unit` | ✅ |
| `rust-integration` | ✅ |
| `agentbox-contract` | ✅ |
| `cross-substrate` | ✅ |
| `mutation` | ✅ on `src/uri/` + `src/bc20/acl/` (advisory elsewhere) |
| `stdio_bridge_soak.rs` | nightly only; failure pages on-call but does not block PRs |

---

## 7. Coverage & Mutation Targets

| Module | Line coverage | Branch coverage | Mutation survival |
|---|---|---|---|
| `src/uri/` | ≥85% | ≥80% | ≤10% |
| `src/bc20/acl/` | ≥85% | ≥80% | ≤10% |
| `src/bc20/federation_*.rs` | ≥80% | ≥75% | ≤15% |
| `src/handlers/uri_resolver_handler.rs` | ≥80% | — | — |
| `src/handlers/agent_events_handler.rs` | ≥75% | — | — |
| `src/handlers/jsonld_handler.rs` | ≥80% | — | — |
| `src/adapters/neo4j_*` | ≥70% (existing baseline) | — | — |

`cargo tarpaulin` reports per-PR; deltas are surfaced as a PR comment.

---

## 8. Phased Rollout (paired with PRD-006)

| Phase | Tests that gate | Pairs with PRD-006 phase |
|---|---|---|
| **P1** | restore `ontology_parser_test`, `CRITICAL_github_sync_regression_test`, neo4j fixture, `neo4j_v2_roundtrip`, `kgnode_ontology_merge`, `e2e_v2_field_survival` | P1 (Plumbing) |
| **P2** | `uri_mint_determinism`, `uri_parse_roundtrip`, `uri_resolver_*`, `did_nostr_wrap_at_boundary`, `jsonld_endpoint_shape`, `uri_clippy_lint` | P2 (Mint+Resolver) |
| **P3** | every `tests/contract/acl/*`, `bc20_federation_session`, `local_fallback_probe`, `agentbox_compat_versions` | P3 (BC20 ACL) |
| **P4** | `agent_spawn_event_stream`, `stdio_bridge_soak` (nightly), mutation gate | P4 (Live observability) |
| **P5** | `cross_substrate_uri_resolve`, `jsonld_round_trip`, agentbox-side context-pin tests | P5 (Context federation) |

---

## 9. Risks

| Risk | Mitigation |
|---|---|
| Neo4j testcontainer startup time bloats CI | parallelise; share a single Neo4j across the integration job; reset DB per test via Cypher `MATCH (n) DETACH DELETE n` |
| Agentbox sibling boot is fragile under CI | pin to a known-good `image_hash`; cache the docker image in CI; allow up to 60s for `/v1/meta` to be reachable |
| Mutation testing ratchets are noisy | start advisory for 30 days; flip to gate after baseline stabilises |
| Stdio bridge soak test produces false positives | run only nightly; alerts go to oncall, not PR authors |
| Property test corpus drifts from real data | derive proptest seeds from the canonical fixture corpus; weekly job replays prod sample (sanitised) |

---

## 10. Open Questions

1. **Mocked vs real agentbox in CI?** Real agentbox sibling is more accurate; mocked sibling is faster. Recommendation: real for `bc20_federation_session` and `agent_spawn_event_stream`; mocked for unit-level ACL tests.
2. **Where does `tests/fixtures/canonical/` live — VC repo, agentbox repo, or a third shared repo?** Recommendation: VC repo (this repo is the integration substrate); agentbox imports via git submodule pinned to a tag.
3. **Should we adopt `cargo nextest`?** Faster, better failure UX, supports test partitioning. Recommendation: yes for all integration suites; unit tests can stay on `cargo test`.
4. **JSON-LD canonicalisation testing**: agentbox uses JCS (RFC 8785) for signed credentials. VC currently doesn't sign anything. Defer JCS round-trip tests to PRD-006 P5.

---

## 11. References

- Audit memo: `project_integration_audit_2026_04_27.md` (RuVector memory)
- PRD-006: [`docs/PRD-006-visionclaw-agentbox-uri-federation.md`](PRD-006-visionclaw-agentbox-uri-federation.md)
- Agentbox URI tests: [`agentbox/tests/contract/linked-data/uris.contract.spec.js`](../agentbox/tests/contract/linked-data/uris.contract.spec.js)
- Existing gap analyses: [`docs/gap-analysis-mad-vs-agentbox.md`](gap-analysis-mad-vs-agentbox.md)
