# 00 — Master Synthesis + Go/No-Go (Sprint 2 close)

Re-audit of the six findings raised on 2026-04-19
(`docs/audits/2026-04-19-sovereign-mesh-qe/00-master.md`) against Sprint 2
commits landed on `main` (HEAD `df1946eba`). Read-only; no production code
modified. One sanity `cargo check --lib` was attempted but the worktree lacks
the `whelk-rs` submodule (`whelk-rs/Cargo.toml` missing); build-time
regression is covered by the CI pipeline cited in ADR-055.

## Verdict

**PROD-READY (conditional).** All six blocker/high-priority findings are
closed with tests and call-site plumbing. Phase 2 (solid-pod-rs) and ADR-054
(URN-Solid alignment) are scope-complete behind flags. Conditions: (i)
deploy flags set per ADR-055 rollback matrix, (ii) 72-hour staging
shadow-test (ADR-055 compliance criterion #5) runs clean.

## Finding status

| ID | Status | Evidence |
|----|--------|----------|
| **B1** publish/unpublish saga | **closed** | `src/sovereign/visibility.rs:89-593` — full module; `flip_to_public` / `flip_to_private` / `write_tombstone`; `mark_saga_pending` on Neo4j failure; 7 tests in `tests/visibility_transitions.rs:183-433`. |
| **B2** BRIDGE_TO ↔ kind-30100 | **closed** | `src/services/bridge_edge.rs:426-472` dispatches `SignBridgePromotion` **after** the Cypher MERGE at :391-395; best-effort (never fails promote); counters `bridge_kind30100_signed_total` + `_errors_total` at `src/services/metrics.rs:110-114,256-266`; 5 tests in `tests/bridge_signing_fanout.rs:108-325`. |
| **B3** NIP-98 body-hash binding | **closed** | Primary verifier `src/utils/auth.rs:248-249` builds `body_ref` from buffered bytes and passes it to `verify_nip98_auth` at :251-253; proxy counterpart `src/handlers/solid_proxy_handler.rs:1591-1599` identical; replay test `tests/auth_hardening.rs:82-139` (b3a/b3b/b3c). |
| **H1** opacify cross-user private | **closed** | `opacify_for_caller` at `src/handlers/api_handler/graph/mod.rs:170-207`; graph handler at :299-334 routes non-owner private nodes through opacification instead of dropping; 3 tests in `tests/bit29_on_wire.rs:93-187` (h1a/h1b/h1c). |
| **H2** bit 29 on the wire | **closed** | V3 encoder `src/utils/binary_protocol.rs:448-458`; V4 delta helper `src/utils/delta_encoding.rs:34-46` and :214-216, :229-231; 20+ call sites (`fastwebsockets_handler`, `socket_flow_handler/position_updates.rs`, `client_coordinator_actor`) pass a real `HashSet<u32>`; byte-level assertion `tests/bit29_on_wire.rs:265-272`. |
| **H3** APP_ENV fail-closed | **closed** | `is_production()` at `src/utils/auth.rs:27-41` — missing var returns `true`; dev-bypass gated at :184 and legacy-path rejected at :303-310; 3 tests `tests/auth_hardening.rs:202-309` (h3a/h3b/h3c). |

## Phase 2 + ADR-054 scope survey

- solid-pod-rs parity: 61 `present`, 8 `partial`, 11 `missing`
  (`crates/solid-pod-rs/PARITY-CHECKLIST.md`). Delta vs Sprint-1 tail
  (present 48←27, partial 8←7, missing 11←25). LDP (1,384 LoC), OIDC
  feature-gated (672 LoC, `#![cfg(feature = "oidc")]` in `src/oidc.rs:22`;
  Cargo `features` entry at :53-58), Notifications (624 LoC with both
  WebSocket and Webhook channels), `tests/wac_inheritance.rs` 735 LoC /
  31 tests, `tests/interop_jss.rs` 705 LoC / 30 tests.
- ADR-054: `docs/reference/urn-solid-mapping.md` carries 51 mapping rows;
  `IngestSaga::regenerate_corpus_jsonl` at `src/services/ingest_saga.rs:669`
  wired into publish and unpublish via
  `VisibilityTransitionService::maybe_regenerate_corpus`
  (`src/sovereign/visibility.rs:360-379`, :458, :569). Schema + manifest
  rendered in `src/handlers/solid_proxy_handler.rs:163-255`. The
  `kg-node.schema.json` document is **not** persisted under `schema/`; it
  is rendered at request time and PUT to the Pod. No leak risk; inline in
  report §5.

## New risks (see 06-new-risks.md)

| # | Risk | Rating |
|---|------|--------|
| R1 | B2 best-effort: Neo4j `:BRIDGE_TO` promotions without a kind-30100 audit event are visible only via error counter | medium |
| R2 | `:PodTombstone` nodes accumulate unboundedly; no retention policy | low |
| R3 | `corpus.jsonl` regenerated in full on every publish/unpublish — O(n) per transition | medium |
| R4 | JSON-LD content negotiation in solid-pod-rs relies on `Accept` parsing only; profile parameters not honoured | low |
| R5 | `APP_ENV=development` env-var poisoning in shared-host deployments | low |

None blocker. R1 and R3 warrant operational mitigations before production
soak-test (queue length alerts, nightly corpus-bytes-written histogram).

## Sprint-2 delivery summary

Sprint 2 landed exactly what ADR-055 committed to: every finding from the
Sprint-1 audit is plumbed through to its caller; the wire format now
carries bit 29; the saga enforces Pod-first ordering with crash-recovery
markers; the audit trail for bridge promotions ships over Nostr. The
implementation fidelity is high — no scope creep, no band-aid
work-arounds, and the feature flags from ADR-055 §Rollback remain the
single-switch recovery path. The ecosystem surface (ADR-054) ships behind
`URN_SOLID_ALIGNMENT=true` and has additive side-effects only — safe to
enable incrementally.
