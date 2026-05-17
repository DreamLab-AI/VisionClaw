# 00 — Master Synthesis + Go/No-Go

Aggregation of the six perspective reports for the sovereign-mesh sprint (13 commits on `main` ending `48396ea12`, 7 new ADRs). Verdict at the end.

## Critical blockers (must fix before merge-to-prod)

| # | Finding | Source |
|---|---|---|
| **B1** | **Publish/unpublish saga (ADR-051 §publish saga) not implemented.** No `src/sovereign/visibility.rs` or equivalent. MOVE between `./private/kg/` and `./public/kg/` containers is missing. The double-gate supports *creation* of public content but not the symmetric *transition* that ADR-051 requires. **No code path emits HTTP 410 Gone on unpublish.** | Integration §I5 |
| **B2** | **BRIDGE_TO + kind-30100 coordination missing.** `BridgeEdgeService::promote` writes the monotonic edge; `ServerNostrActor::SignBridgePromotion` signs the event. Nothing fans these out together. The intended "promotion ⇒ audit trail" invariant is not enforced. | Integration §I4 |
| **B3** | **NIP-98 body not hash-bound on primary verifier.** `verify_nip98_auth` called with `body=None` at `src/utils/auth.rs:197` and `solid_proxy_handler:368`. A token can be paired with an arbitrary body and pass. ADR-028-ext compliance criterion "body hash binding" is false in practice. | Security §S1 |

## High-priority findings (should fix before MVP ship)

| # | Finding | Source |
|---|---|---|
| H1 | **Opacified-stub emission missing.** `visibility_allows` drops cross-user private nodes rather than emitting them as bit-29-flagged opaque stubs. ADR-028-ext §three-tier contract unmet. | Privacy §P1 |
| H2 | **Binary V5 privacy flag never fires.** `encode_positions_v3_with_privacy` is defined but every call site passes `None`. Bit 29 is code but not wire. Privacy-by-wire-opacity-signal intent of ADR-050 is unrealised. | Privacy §P2 / Integration §I6 |
| H3 | **`APP_ENV` default is fail-open.** `std::env::var("APP_ENV").map(...).unwrap_or(false)` treats a missing env var as non-production, activating dev-mode bypasses and the legacy X-Nostr-Pubkey path. | Security §S2 |
| H4 | **Bootstrap audit event missing (ADR-030-ext).** `vc-cli bootstrap-power-user` writes the Pod resource but does not emit the kind-30301 audit event. | Integration §I1 |
| H5 | **Saga→Neo4j batch-failure amplification.** A single row-level Neo4j error pends the entire batch. Recovery storm risk on large batches. | Architecture §A1 / Performance §P4 |

## Medium priority

| # | Finding | Source |
|---|---|---|
| M1 | `body_marks_public` in `solid_proxy_handler` is laxer than the parser's `classify_visibility` — a body-level bullet `- public:: true` would falsely satisfy the source gate. Align with the line-anchored rule. | Security §S3 |
| M2 | If `OPAQUE_ID_SALT_SEED` leaks, all daily salts are recomputable; longitudinal unlinkability is lost historically. Rotation mitigates forward only. Document trade-off. | Security §S4 |
| M3 | solid-pod-rs WAC lacks an ACL-resolution cache — each request traverses the path tree via `storage.get` per segment. | Performance §P5 |
| M4 | Saga resumption is fixed 60 s interval; adaptive polling would shorten recovery under large pending queues. | Performance §P4 |
| M5 | `stub_source_wikilink` metadata preserves the authored wikilink text on private stubs. Rename to `_internal_*` or strip at read. | Privacy §P3 |
| M6 | GitHub-sync integration with `GITHUB_CREDS_IN_POD` and with the saga not verified in this audit. | Integration §I2 |
| M7 | `VISIBILITY_CLASSIFICATION` flag usage not verified as a live gate inside `parse_bundle`. | Integration §I3 |

## Low priority / backlog

| # | Finding | Source |
|---|---|---|
| L1 | WAC `agent_matches` does not implement explicit deny — Solid-conformant but warrants ADR-level documentation. | Security §S5 |
| L2 | `bridge_edge_enabled()` re-checks env on every call; cache at construction. | Architecture §A2 |
| L3 | `ServerNostrActor` message types lack serde derives; add if persistence-of-in-flight becomes a requirement. | Architecture §A4 |
| L4 | `corpus.jsonl` (ADR-054) not yet implemented. Scope gap, no leak risk. | Privacy §P4 |
| L5 | PARITY-CHECKLIST.md for solid-pod-rs presence confirmed; population not audited. | Architecture §A3 |
| L6 | `nodes.iter().position(...)` in saga is O(n²) in batch size. | Performance §P3 |
| L7 | `sovereign_schema_enabled()` inside the encoder loop; hoist outside. | Performance §P2 |

## Test coverage gaps (top 5 highest-value additions)

1. Body-binding replay test (catches B3).
2. Opacified-stub emission contract test (catches H1).
3. Binary V5 integration test with populated `private_opaque_ids` (catches H2).
4. `APP_ENV` unset default test (catches H3).
5. Saga crash-recovery end-to-end test (catches H5 + validates resumption).

See `04-coverage.md` for the full priority list.

## Go / No-Go verdict

**BLOCK merge-to-prod. Ship-with-mitigations is viable for staging/MVP-soft-launch.**

Rationale:
- The **cryptographic primitives are sound** (NIP-98 Schnorr, HMAC opaque_id, server identity handling). Tested, correct, and the test surface is reasonable.
- **Pod-first saga ordering is correctly enforced** — the architectural headline of the sprint lands.
- **Default-private Pod provisioning is correctly implemented** — ADR-052 compliance on the ACL shape is solid.
- The **blocker set is narrow and fixable**: B1 (publish saga) and B2 (BRIDGE_TO coordinator) are implementation gaps against explicit ADR contracts, not design flaws. B3 (body binding) is a one-line-per-callsite fix plus tests.
- The **high-priority set** is mostly "plumb the already-defined primitive through to its caller" — e.g. `is_opaque_to` exists, `encode_positions_v3_with_privacy` exists, `SignAuditRecord`/`SignBridgePromotion` exist. The sprint built the tools; the wiring is what needs to ship next.

Recommended sequence:
1. Land B3 (body binding) + a replay-resistance test in a single PR — cheapest, highest security payoff.
2. Land H3 (APP_ENV fail-closed default) + a test — one-line fix, prevents a silent prod misconfiguration.
3. Land the publish saga (B1) as the first post-sprint workstream with crash-recovery tests (H5 + coverage §5).
4. Land BRIDGE_TO+kind-30100 fan-out (B2) coordinator with a saga-style pending-marker for the sign step.
5. Land binary-V5 privacy-flag wiring (H2) + opacified-stub emission (H1) together — they share the caller-context plumbing.
6. Medium/low items on a rolling backlog.

With those six steps the sovereign-mesh sprint is prod-ready. As landed today it is a strong staging deliverable and a coherent set of primitives; it is not yet a coherent end-to-end feature.
