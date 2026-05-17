# 04 — Test Coverage

## Inventory (by ADR)

| ADR | Test file | Lines | Notes |
|---|---|---|---|
| ADR-028-ext (optional auth) | `tests/auth_sovereign_mesh.rs` | 417 | Three-tier matrix: anonymous → public, signed → own private, cross-user → opacified. |
| ADR-050 (schema) | `tests/schema_sovereign_fields.rs` | 346 | `Visibility` enum, `owner_pubkey`, `opaque_id`, `pod_url` fields, `is_opaque_to`. |
| ADR-050 (opaque_id) | `src/utils/opaque_id.rs` (inline) | ~90 lines of tests | HMAC determinism, salt rotation, dictionary resistance, day-quantised derivation. |
| ADR-050 (canonical IRI) | `src/utils/canonical_iri.rs` (inline) | ~70 lines of tests | npub encoding, SHA-256 determinism, rename-produces-new-IRI. |
| ADR-051 (two-pass parser) | `tests/parser_sovereign.rs` | 301 | Visibility classification, stub synthesis, wikilink resolution. |
| ADR-051 (ingest saga) | `tests/ingest_saga.rs` | 338 | Pod-first ordering, pending markers, batch behaviour. |
| ADR-051 (bridge edge) | `tests/bridge_edge_test.rs` | 304 | Signal scoring, sigmoid, surface/promote/expiry. |
| ADR-052 (Pod provisioning) | `tests/pod_provisioning_sovereign.rs` | 439 | 3+1 container layout, root-ACL sovereign check, migration idempotence. |
| ADR-053 (solid-pod-rs WAC) | `crates/solid-pod-rs/src/wac.rs` inline + `crates/solid-pod-rs/tests/wac_basic.rs` | ~200 | ACL evaluation, inheritance, method→mode mapping, allow-header shape. |
| ADR-053 (Storage trait) | `crates/solid-pod-rs/tests/storage_trait.rs` | — | Backend contract (Memory, FS). |
| Metrics | `src/services/metrics.rs` inline | ~50 lines | Registry construction + render text + enabled flag. |

## What's missing

1. **Opacified-stub contract test (ADR-028-ext §three-tier)**: no test asserts that a cross-user private node appears as a **bit-29-flagged stub** in the JSON response. Today's handler drops it silently (see 02-privacy §P1). High priority.
2. **Body-binding replay test (NIP-98)**: no test feeds a valid NIP-98 token for body A against request body B. Critical given finding S1.
3. **`APP_ENV` unset default test**: no test pins the fail-open behaviour of `APP_ENV` defaults. Critical given finding S2.
4. **Binary V5 privacy flag integration test**: `encode_positions_v3_with_privacy` has inline unit tests that exercise the flag but **no integration test** verifies that the broadcast path populates `private_opaque_ids` from the request's caller pubkey. Would catch finding P2.
5. **Saga crash-recovery test**: ADR-051 compliance criterion "process is killed mid-saga; recovery completes on restart". No such test under `tests/`. `ingest_saga.rs` tests validate batch outcomes but do not exercise the resumption loop end-to-end against a real Neo4j.
6. **Orphan retraction integration test**: no test under `tests/` for the 15-min sweep with real edges. Only unit tests for env parsing.
7. **Double-gate race test**: two concurrent PUTs to the same `./public/kg/foo.ttl` with different bodies — does the second one overwrite the first atomically, or does the ETag-optimistic path race?
8. **Bridge expiry monotonic violation test**: synthesise a BRIDGE_TO with confidence 0.95, re-score with 0.40, assert stored confidence is still 0.95. Covered by sigmoid math tests but not by an end-to-end promote-twice test on a real Neo4j.
9. **WAC deny inheritance edge case**: parent grants `foaf:Agent Read`; child `.acl` omits the grant. Does evaluation at the child level deny? (It should — `find_effective_acl` returns the child's ACL first.)
10. **Pod PATCH MOVE atomicity test**: ADR-051 §atomicity model asserts atomic MOVE; no test exercises the MOVE code path against a Pod that simulates partial failure.

## Suggested priority additions (up to 10)

| Pri | Test |
|---|---|
| P0 | Body-binding replay: send NIP-98 token for body-A with body-B, assert 401/403. |
| P0 | Opacified-stub emission: signed caller sees their private, cross-user sees opacified-stub (not dropped). |
| P0 | `APP_ENV` unset legacy-auth test: pin production fail-closed default. |
| P1 | Binary V5 broadcast with populated `private_opaque_ids`: assert bit 29 is set on the wire id. |
| P1 | Saga crash recovery: crash between Phase 1 Pod write and Phase 2 commit, assert resumption completes. |
| P1 | Orphan retraction e2e: seed two stale WikilinkRef edges + a private stub, run `run_once()`, assert count. |
| P2 | Double-gate concurrent PUTs: ETag-guarded idempotence. |
| P2 | WAC inheritance: parent grants public Read, child is restricted, assert child access. |
| P2 | Pod MOVE atomicity (publish saga), both happy and mid-failure paths. |
| P3 | `corpus.jsonl` generation + schema validation round-trip (ADR-054). |

## `cargo check --lib` status

Not executed per audit protocol constraints (read-only audit). The sprint lands substantial new modules (`ingest_saga`, `bridge_edge`, `orphan_retraction`, `pod_client`, `server_identity`, `server_nostr_actor`, `metrics`, `solid_proxy_migration`, `canonical_iri`, `opaque_id`, `parsers::visibility`, the `solid-pod-rs` crate). Recommend a gating `cargo check --lib -p webxr && cargo check -p solid-pod-rs` in the staging pre-merge pipeline before cutting the MVP release — the compilation surface expanded materially and `cargo check` is the cheapest way to catch a drift.
