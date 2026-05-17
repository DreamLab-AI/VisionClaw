# ADR-055: Sovereign Debt Payoff + Phase 2 Sprint

## Status

Ratified 2026-04-19 (end of Sprint 1)

## Context

The 2026-04-19 sovereign-mesh audit (`docs/audits/2026-04-19-sovereign-mesh-qe/00-master.md`)
returned verdict **BLOCK merge-to-prod, staging-viable** with 3 blockers (B1-B3)
and 3 high-priority findings (H1-H3). The implementation gaps are narrow:
every finding is a call-site or wiring issue, not a design flaw. The
cryptographic primitives are sound; the ADR contracts are sound; what's
missing is the plumbing that couples them.

This ADR declares Sprint 2 scope and ordering to close the debt, land
solid-pod-rs Phase 2, and ship the ecosystem-alignment surface from
ADR-054 behind the `URN_SOLID_ALIGNMENT` flag.

## Decision

### Stream A — QE debt payoff (P0, must complete before prod)

| ID | Scope | Files | ADR gap |
|----|-------|-------|---------|
| B1 | Publish/unpublish saga with MOVE + `410 Gone` | `src/sovereign/visibility.rs` (NEW) + saga wiring | ADR-051 §transitions |
| B2 | `BridgeEdgeService::promote` fans out kind-30100 via `ServerNostrActor` | `src/services/bridge_edge.rs`, caller hook | ADR-050 §server-identity + ADR-051 §audit |
| B3 | NIP-98 body-hash binding at call sites | `src/utils/auth.rs:197`, `src/handlers/solid_proxy_handler.rs:368` | ADR-028-ext §body-binding |
| H1 | `visibility_allows` emits opaque stubs instead of dropping cross-user private | `src/services/graph_serialization.rs` (or wherever filter lives) | ADR-050 §bit-29 |
| H2 | `encode_positions_v3_with_privacy` call sites pass real `private_opaque_ids` | wire-encode call sites | ADR-050 §bit-29 |
| H3 | `APP_ENV` default fail-closed (production unless explicitly dev) | `src/utils/auth.rs` dev-bypass + legacy path | ADR-028-ext §prod-gate |

### Stream B — solid-pod-rs Phase 2 (P1, JSS parity path)

| Scope | Target |
|-------|--------|
| LDP conformance: Link headers (rel=type, rel=acl, rel=describedby), Prefer headers, Accept-Post, PATCH via SPARQL-Update | `crates/solid-pod-rs/src/ldp.rs` |
| Solid-OIDC flow (feature-flagged `OIDC_ENABLED`) | `crates/solid-pod-rs/src/oidc.rs` (NEW) |
| Solid Notifications complete: WebSocketChannel2023 + WebhookChannel2023 | `crates/solid-pod-rs/src/notifications.rs` |
| ACL inheritance edge-case corpus ported from JSS | `crates/solid-pod-rs/tests/wac_inheritance.rs` (NEW) |
| Parity gate: JSS test corpus adapted | `crates/solid-pod-rs/tests/interop_jss.rs` (NEW) |

### Stream C — ADR-054 implementation (P2, ecosystem)

| Scope | Target |
|-------|--------|
| `urn-solid-mapping.md` with ~50 initial vocab mappings | `docs/reference/urn-solid-mapping.md` (NEW) |
| `./public/kg/corpus.jsonl` per-user generator in saga | `src/services/ingest_saga.rs` addition |
| JSON-LD content negotiation in solid-pod-rs | `crates/solid-pod-rs/src/ldp.rs` + accept/content-type branches |
| `urn:solid:KGNode` schema + type manifest writer | `src/services/pod_client.rs` (schema push) |

## Sprint sequencing

**Wave 1 (P0 Stream A, parallel-safe):**
- W1-a: B3+H3 combined → `src/utils/auth.rs` (same file, single commit)
- W1-b: B1 new module → `src/sovereign/visibility.rs` (zero-conflict new file)

**Wave 2 (P0 Stream A, after Wave 1 merges):**
- W2-a: B2 bridge-to-signing fan-out → `src/services/bridge_edge.rs`
- W2-b: H1+H2 bit 29 on wire → `src/services/graph_serialization.rs` + encoder call sites

**Wave 3 (P1/P2, post-debt):**
- W3-a: solid-pod-rs LDP + Notifications completion
- W3-b: ADR-054 implementation (corpus.jsonl + mapping + JSON-LD)
- W3-c: JSS parity test corpus port

**Wave 4 (gate):**
- QE re-audit confirming B1-B3 + H1-H3 resolved
- Shadow-test in staging via `config/staging/docker-compose.staging.yml`
- Cutover flag switch `SOLID_IMPL=native` (eventually)

## Consequences

- Closes the sprint-1 blocker set without re-architecting
- Puts solid-pod-rs on the parity path, unblocking the JSS cut-over
- URN-Solid + Solid-Apps + solid-schema interop ships behind a flag,
  enabling external Solid apps to read VisionClaw KGs with zero
  custom code
- Two re-audits (mid-sprint after W2, end-of-sprint after W4) provide
  the merge-to-prod green light

## Compliance Criteria

- [ ] All 6 QE findings (B1-B3, H1-H3) resolved and re-audit verifies
- [ ] `cargo check --lib` clean after each merge (no build, no test
      execution — inherits Sprint 1 discipline)
- [ ] solid-pod-rs passes adapted JSS test corpus
- [ ] `URN_SOLID_ALIGNMENT=true` feature works end-to-end in staging
- [ ] Staging shadow-test green for 72 hours before prod cutover

## Rollback

Per-feature flags (inherited from Sprint 1):
- `NIP98_OPTIONAL_AUTH`
- `POD_DEFAULT_PRIVATE`
- `VISIBILITY_CLASSIFICATION`
- `POD_SAGA_ENABLED`
- `SOVEREIGN_SCHEMA`
- `BRIDGE_EDGE_ENABLED`
- `METRICS_ENABLED`
- `URN_SOLID_ALIGNMENT`

Each is independent; any single regression falls back to pre-Sprint-1
behaviour without disturbing others.

## Related Documents

- `docs/audits/2026-04-19-sovereign-mesh-qe/00-master.md` — the audit driving this
- ADR-028-ext, ADR-050, ADR-051, ADR-052, ADR-053, ADR-054
