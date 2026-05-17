# PRD-014 Addendum: QE Fleet Validation — 80% → 90%+ Readiness

**Date:** 2026-05-09
**Author:** Dr John O'Hare / QE Fleet (6-agent validation audit)
**Status:** Draft
**Parent:** PRD-014 (Ecosystem Productionisation)
**Scope:** All 5 substrates — VisionClaw, agentbox, nostr-rust-forum, dreamlab-ai-website, solid-pod-rs

---

## 1. Audit Methodology

A 6-agent QE fleet was deployed to validate PRD-014 findings and identify gaps the original 5-agent research swarm missed. Each agent performed deep, file-level verification with evidence.

| Agent | Focus | Duration | Tool Uses |
|-------|-------|----------|-----------|
| qe-security-deep | Security audit: injection, SSRF, path traversal, secrets, deps | ~8.5 min | 96 |
| qe-client-coverage | Client test coverage, bundle, error boundaries, a11y, dead code | ~2 min | 21 |
| qe-cross-substrate | IS-Envelope, NIP-26, NIP-98, DID, fixture sync, health schemas | ~3.5 min | 42 |
| qe-rust-quality | Unwrap/panic audit, complexity, error handling, concurrency | ~5.5 min | 24 |
| qe-ci-ops | CI pipelines, Docker, deployment, monitoring, backup, docs | ~3.3 min | 49 |
| qe-dependency-audit | cargo-audit, npm audit, licence compliance, supply chain | ~2.9 min | 31 |

---

## 2. PRD-014 Validation Summary

### Findings Confirmed (PRD-014 was correct)

| PRD-014 ID | Finding | Verified By |
|------------|---------|-------------|
| S01 | WebAuthn assertion signature never verified | qe-security-deep (V1) |
| S02 | Auth bypass via DOCKER_ENV | qe-security-deep (V2) |
| S03 | 23+ unauthenticated handler groups | qe-security-deep (V3) — expanded to include git ingest CRUD |
| S05 | CSP commented out | qe-security-deep (V4) |
| S06 | ALLOW_INSECURE_DEFAULTS soft-fail | qe-security-deep (V5) |
| S10 | NIP-98 replay — no event ID tracking | qe-security-deep (V6) |
| D03 | No CI for nostr-rust-forum | qe-ci-ops — RED across all 6 dimensions |
| T01 | Client low test coverage | qe-client-coverage — 9.4% (39/417), not 8.4% |
| T02 | Cross-substrate fixtures never synced | qe-cross-substrate (N5) — dirs empty, L1 tests silently skip |
| H02 | Rate limiting only on /api/settings | qe-security-deep (N4) |
| H05 | panic!() in actor/event code | qe-rust-quality — 19 production panic paths found |
| O01 | Circuit breaker stats empty | qe-ci-ops |

### Findings Corrected (PRD-014 was wrong or imprecise)

| PRD-014 ID | Original Claim | Correction |
|------------|---------------|------------|
| C02 | XR room scenes in production (4.5MB) | **FALSE.** Vite config at `vite.config.ts:51-54` correctly externals `@iwer/*` in production. No room scene files exist in `client/src/`. Remove from remediation scope. |
| S03 | "23 handler groups" | **Undercounted.** Git ingest CRUD endpoints (`POST/DELETE /api/ingest/remotes`, `POST /api/ingest/sync`, `POST /api/ingest/writeback`) are also unauthenticated — these are the most dangerous as they allow registering arbitrary git remotes and triggering syncs. |
| T01 | "8.4% test coverage" | **Slightly better:** 9.4% (39 test files / 417 source files). Still critically low. |
| C01 | "Single error boundary" | **Two exist:** top-level ErrorBoundary + CanvasErrorBoundary in GraphCanvasWrapper. But 21/23 feature directories still lack any boundary. |
| Risk: "License compliance (AGPL solid-pod-rs consumed by MPL VisionClaw)" | Listed as risk only | **CRITICAL blocker.** VisionClaw is MIT, not MPL. AGPL solid-pod-rs is compiled into the MIT binary as a direct Cargo dependency. This is a hard licence violation that blocks any public release. |

---

## 3. New Findings (Missed by Research Swarm)

### P0 — CRITICAL (3 new)

| ID | Finding | Substrate | File | Detail |
|----|---------|-----------|------|--------|
| **NEW-S1** | Live API keys on disk | VisionClaw | `.env:152,161` | `OPENAI_API_KEY=sk-proj-5H1QNQ...` and `DEEPSEEK_API_KEY=sk-d76e012d...` in plaintext. Any agent/subprocess/container escape exposes them. Rotate immediately. |
| ~~NEW-S2~~ | ~~AGPL→MIT licence~~ | ~~solid-pod-rs → VisionClaw~~ | — | **ACCEPTED.** DreamLab owns both repos. Internal licence choice, not a compliance issue. No action needed. |
| **NEW-R1** | Poisoning-hazard global RwLock | VisionClaw | `src/connectors/mod.rs:45,79,94,107` | `lazy_static! RwLock<Vec<ConnectorEntry>>` — if any thread panics while holding the lock, ALL subsequent connector API calls panic permanently. Cascading failure vector. |

### P1 — HIGH (15 new)

| ID | Finding | Substrate | File | Detail |
|----|---------|-----------|------|--------|
| **NEW-S3** | Cypher injection via axiom subject | VisionClaw | `handlers/ontology_agent_handler.rs:217` | `format!("MATCH (n:{}) ...", axiom.subject)` — user-controlled POST body interpolated into Cypher label. Injection: `KGNode) RETURN n UNION MATCH (m`. |
| **NEW-S4** | Git ingest CRUD unauthenticated | VisionClaw | `services/git_ingest/mod.rs:576-594` | Attacker can register arbitrary remotes, trigger syncs from malicious repos, writeback to knowledge graph. |
| **NEW-S5** | No JSON payload size limits | VisionClaw | `main.rs:826-984` | No `JsonConfig::limit()` or `PayloadConfig::limit()`. Combined with no rate limiting, enables memory exhaustion. |
| **NEW-R2** | Unbounded channel OOM vector | VisionClaw | `app_state.rs:1041` | `mpsc::unbounded_channel::<ClientMessage>()` for all WebSocket clients. No backpressure. Under load/slow consumer → OOM. |
| **NEW-R3** | Attacker-triggerable panic | solid-pod-rs | `webid.rs:387` | `panic!("embedded JSON-LD failed to parse")` — malformed WebID profile from attacker-controlled URL crashes the server. |
| **NEW-R4** | 9 todo!() panics in setup-skill | nostr-rust-forum | `setup-skill/lib.rs` | SelfHost, Cloudflare, FlyDotIo, Turnkey, Kubernetes provider impls all `todo!()`. Runtime call = process crash. |
| **NEW-R5** | unreachable!() on user input | nostr-rust-forum | `relay-worker/moderation.rs:307` | User-controlled `resolution` string hits `unreachable!()`. New resolution types → server panic. |
| **NEW-I1** | Nostr crate version divergence | VisionClaw + forum | `Cargo.toml` | VisionClaw: `nostr-sdk` 0.43. Forum: `nostr` 0.44. Different `Kind`, `Event`, `Timestamp` types. Cross-substrate serialization breaks at the type boundary. |
| **NEW-I2** | Event kind triple-split | ALL Rust | Multiple | `kind` is u16 (VisionClaw), u32 (forum EventId::compute), u64 (forum NostrEvent). Truncation at `event.rs:289` silently wraps kinds > 65535. |
| **NEW-I3** | Kind 30023 collision | VisionClaw + forum | Multiple | VisionClaw uses kind 30023 for migration approvals; forum uses it for NIP-23 long-form content. Same relay -> indistinguishable events. Note: VisionClaw now also publishes kinds 31400 (governance panel) and 31402 (action request) which are Agent Control Surface Protocol events consumed by `nostr-bbs-core` -- these do not collide as they are in the 31xxx range reserved for governance. |
| **NEW-I4** | No shared type crate or IDL | ALL | — | Zero shared Rust crate for DID, Event, Envelope types. Every substrate independently defines these. Root cause of NEW-I1/I2/I3. |
| **NEW-I5** | Fixture dirs empty, L1 tests inert | forum, solid-pod-rs, agentbox | `tests/fixtures/` | ADR-082 fixture tests soft-skip when fixtures absent. CI passes green with zero fixture validation. Defeats entire test fixture strategy. |
| **NEW-D1** | h2 0.3.27 HTTP/2 Rapid Reset | VisionClaw | `Cargo.lock` | CVE-2023-44487 / RUSTSEC-2024-0332. RST_STREAM flood → excessive CPU. Needs h2 >= 0.4.4 or nginx mitigation. |
| **NEW-D2** | axios + lodash HIGH vulns in client | client | `package-lock.json` | GHSA-pf86 (axios prototype pollution), GHSA-r5fr (lodash code injection). No upstream fix available; replace with native alternatives. |
| **NEW-D3** | EOL rustls 0.21.x in solid-pod-rs | solid-pod-rs | `Cargo.lock` | End-of-life branch, receives no security patches. Upgrade to 0.23.x. |

### P2 — MEDIUM (14 new)

| ID | Finding | Substrate | Detail |
|----|---------|-----------|--------|
| **NEW-S6** | Path traversal URL-encoding bypass potential | VisionClaw | `solid_pod_handler.rs:356-364` — dot check may not handle `%2e%2e` |
| **NEW-S7** | 92 unsafe blocks in CUDA FFI | VisionClaw | Expected for GPU, but unauthenticated clustering endpoints feed GPU buffers |
| **NEW-S8** | Cookie Secure flag conditional on NODE_ENV | VisionClaw | `idp/provider.js:113` — missing NODE_ENV → insecure cookies |
| **NEW-R6** | GPU subsystem: 42 lock sites, no ordering | VisionClaw | 4 files (`force_compute_actor`, `shared`, `gpu_safety`, `backpressure`) — deadlock risk |
| **NEW-R7** | speech_service.rs nesting depth 29 | VisionClaw | Most complex function in ecosystem; 1,414 lines, 110 match arms |
| **NEW-R8** | neo4rs 0.9.0-rc.8 (release candidate) | VisionClaw | Pre-stable API, no semver guarantees |
| **NEW-R9** | 126 #[allow(dead_code)] suppressions | VisionClaw | Indicates incomplete CQRS event system and partial supervision impl |
| **NEW-C1** | 23 dead components + 6 dead hooks | client | ~2,000+ lines of dead weight in bundle |
| **NEW-C2** | No lazy loading for main features | client | Graph, settings, vis, ontology, bots, physics all load eagerly |
| **NEW-C3** | Feature flags entirely dead | client | `fetchFeatureFlags()` returns hardcoded defaults. 4 gated components permanently hidden |
| **NEW-I6** | Secp256k1 library divergence | ALL Rust | 3 libraries: `secp256k1` 0.29 (VC), internal (forum via nostr), `k256` 0.13 (solid-pod-rs) |
| **NEW-I7** | Health endpoint schema divergence | ALL | No shared health response format. Mesh liveness checks cannot parse uniformly |
| **NEW-I8** | NIP-98 timestamp tolerance mismatch | VC + forum | VisionClaw: 300s. Forum: 60s. Cross-substrate NIP-98 auth intermittently fails |
| **NEW-D4** | Zero cargo-deny across all substrates | ALL Rust | No licence, advisory, or source auditing in CI |

---

## 4. Revised Gap Count

| Severity | PRD-014 Original | Confirmed | Corrected | New | **Revised Total** |
|----------|-----------------|-----------|-----------|-----|-------------------|
| CRITICAL | 7 (2 accepted) | 5 | 0 | 3 (1 accepted) | **8** (3 accepted) |
| HIGH | 11 (1 accepted) | 6 | 1 expanded | 15 | **22** (1 accepted) |
| MEDIUM | ~30 | ~25 | 1 removed (C02) | 14 | **~39** |
| LOW | ~22 | ~20 | — | 2 | **~22** |
| **Total** | **~70** | | | **34 new** | **~91** |

The QE fleet found **34 new gaps** that the research swarm missed, including 3 CRITICAL and 15 HIGH.

---

## 5. Revised Workstreams (60% → 90%+)

### WS-0: Security Blockers (P0) — EXPANDED

**Original estimate: 5 days → Revised: 8 days**

| ID | Gap | Effort | Change |
|----|-----|--------|--------|
| S01 | WebAuthn sig verification | 2d | Unchanged |
| S02 | DOCKER_ENV auth bypass | 4h | Unchanged |
| S03+NEW-S4 | Unauthenticated endpoints (expanded) | 3.5d | +0.5d for git ingest CRUD |
| S05 | CSP headers | 1h | Unchanged |
| NEW-S1 | Rotate exposed API keys | 1h | **NEW** |
| ~~NEW-S2~~ | ~~Licence resolution~~ | — | **ACCEPTED** — DreamLab owns both repos, internal licence choice |
| NEW-S3 | Cypher injection fix | 4h | **NEW** — regex validate axiom.subject |
| NEW-S5 | JSON payload size limits | 1h | **NEW** |

### WS-1: Data Safety & CI Pipeline (P1) — EXPANDED

**Original estimate: 4 days → Revised: 6 days**

| ID | Gap | Effort | Change |
|----|-----|--------|--------|
| D01 | Neo4j backup automation | 1d | Unchanged |
| D02 | Neo4j schema migration framework | 2d | Unchanged |
| D03 | nostr-rust-forum CI | 1d | Unchanged |
| D04 | cargo-audit in VisionClaw CI | 2h | Unchanged |
| D05+NEW-D4 | cargo-deny for all substrates | 1d | Expanded: 3 deny.toml files, wired to CI |
| D06 | Clippy -D warnings | 2h | Unchanged |
| D07 | Workspace licence field | 15m | Unchanged |
| NEW-D1 | h2 upgrade (CVE mitigation) | 4h | **NEW** |
| NEW-D3 | solid-pod-rs rustls upgrade | 4h | **NEW** |

### WS-2: Runtime Hardening (P2) — EXPANDED

**Original estimate: 5 days → Revised: 9 days**

| ID | Gap | Effort | Change |
|----|-----|--------|--------|
| H01 | Security headers middleware | 1h | Unchanged |
| H02+NEW-S5 | Rate limiting + payload limits | 1.5d | Expanded |
| H03 | Swagger UI gated | 2h | Unchanged |
| H04 | CORS restrictive default | 4h | Unchanged |
| H05 | panic!() → Result conversions | 6h | Unchanged |
| H06-H13 | agentbox + solid-pod-rs hardening | 1d | Unchanged |
| NEW-R1 | Replace global RwLock with DashMap | 4h | **NEW** |
| NEW-R2 | Bounded channel for client messages | 4h | **NEW** |
| NEW-R3 | solid-pod-rs webid.rs panic → Result | 2h | **NEW** |
| NEW-R4 | Forum todo!() → Err(NotImplemented) | 4h | **NEW** |
| NEW-R5 | Forum unreachable!() → error return | 1h | **NEW** |
| NEW-R6 | GPU lock ordering documentation | 1d | **NEW** |
| NEW-I8 | NIP-98 timestamp tolerance alignment | 2h | **NEW** |

### WS-3: Client Production Readiness (P2) — REVISED

**Original estimate: 5 days → Revised: 6 days**

| ID | Gap | Effort | Change |
|----|-----|--------|--------|
| C01 | Error boundaries (21 features need them) | 1d | Expanded: 21 features, not just 5 |
| ~~C02~~ | ~~XR room scenes~~ | — | **REMOVED** — properly externalized already |
| C03 | JSON.parse try-catch | 15m | Unchanged |
| C04 | window.open noopener | 30m | Unchanged |
| C05 | skipAuth URL param | 1h | Unchanged |
| C06 | sourceMap explicit disable | 15m | Unchanged |
| C07 | WebSocket reconnect | 2h | Unchanged |
| C08 | Offline detection | 2h | Unchanged |
| C09 | COOP/COEP documentation | 1h | Unchanged |
| C10 | Web vitals monitoring | 3h | Unchanged |
| NEW-C1 | Remove 23 dead components + 6 hooks | 4h | **NEW** |
| NEW-C2 | Lazy loading for main features | 1d | **NEW** |
| NEW-D2 | Replace axios with fetch, lodash with native | 1d | **NEW** |

### WS-4: Testing & Quality Gates (P2) — EXPANDED

**Original estimate: 8 days → Revised: 10 days**

| ID | Gap | Effort | Change |
|----|-----|--------|--------|
| T01 | Client test coverage → 25%+ | 5d | Unchanged; SolidPodService (1,670 lines) added to critical list |
| T02+NEW-I5 | Fixture sync + populate dirs + un-skip L1 | 1.5d | Expanded: must verify tests actually run |
| T03 | agentbox HTTP integration tests | 3d | Unchanged |
| T04 | solid-pod-rs CI path triggers | 1h | Unchanged |
| T05 | Property/fuzz tests | 3d | Unchanged |

### WS-5: Observability & Operations (P3) — UNCHANGED

**Estimate: 5 days** (no new findings here)

### WS-6: Cross-Substrate Interoperability (NEW — P2)

**Estimate: 8 days** — Required for 90%+, deferred from PRD-015 scope.

| ID | Gap | Effort | Detail |
|----|-----|--------|--------|
| NEW-I1 | Nostr crate version alignment | 2d | Align VisionClaw to nostr-sdk 0.44 or forum to 0.43. Requires API migration. |
| NEW-I2+I3 | Event kind width unification + registry | 1d | Standardize on u64 everywhere. Document kind registry to prevent collisions. |
| NEW-I4 | Shared ecosystem-types crate | 3d | `ecosystem-types` crate with DID, NIP-98, Event, Envelope shared types. Published to private registry or workspace dep. |
| NEW-I7 | Health endpoint schema contract | 1d | Shared JSON schema for `/health` responses. Implement in all substrates. |
| NEW-I6 | Document secp256k1 library strategy | 1d | ADR documenting why 3 libraries coexist (or converge to one). Ensure BIP-340 test vectors pass identically across all 3. |

---

## 6. Revised Effort Summary

| Workstream | PRD-014 Est. | Revised Est. | Delta |
|------------|-------------|-------------|-------|
| WS-0: Security Blockers | 5d | 8d | +3d |
| WS-1: Data Safety & CI | 4d | 6d | +2d |
| WS-2: Runtime Hardening | 5d | 9d | +4d |
| WS-3: Client Production | 5d | 6d | +1d |
| WS-4: Testing & Quality | 8d | 10d | +2d |
| WS-5: Observability & Ops | 5d | 5d | — |
| WS-6: Cross-Substrate Interop | — | 8d | **NEW** |
| **Total** | **32d** | **52d** | **+20d** |

At 1 FTE: ~10-11 weeks. With AI-assisted development (10-15x multiplier): ~4-5 weeks wall-clock.

---

## 7. Revised Phasing (4 phases → 5 phases)

### Phase A — Security Gate (WS-0): Sprint 1, Week 1-2
**Exit criterion:** Zero CRITICAL security gaps. API keys rotated. Licence resolved. All mutating endpoints (including git ingest) require auth. WebAuthn signature verified. CSP active. Cypher injection blocked.

### Phase B — Foundation (WS-1 + WS-2 partial): Sprint 1, Week 2-4
**Exit criterion:** All 5 substrates have CI with cargo-audit/deny. Neo4j backup running. h2/rustls upgraded. RwLock/unbounded channel fixed. panic→Result conversions done.

### Phase C — Quality (WS-3 + WS-4): Sprint 2, Week 1-3
**Exit criterion:** Client test coverage ≥25%. Error boundaries on all features. Dead code removed. Lazy loading active. Cross-substrate fixtures synced and L1 tests running. axios/lodash replaced.

### Phase D — Interoperability (WS-6): Sprint 2, Week 2-4
**Exit criterion:** Nostr crate versions aligned. Event kind width standardized to u64. ecosystem-types crate exists. Health schema contract shared. Kind registry documented.

### Phase E — Operations (WS-5 + WS-2 remainder): Sprint 3, Week 1-2
**Exit criterion:** Runbooks for all 5 substrates. DR procedures with RTO/RPO. Health dashboard live. GPU lock ordering documented.

---

## 8. Revised Success Metrics (60% → 90%+)

| Metric | Current (60%) | PRD-014 Target (80%) | Revised Target (90%+) |
|--------|--------------|---------------------|----------------------|
| CRITICAL security gaps | 8 (2 accepted) | 0 | 0 |
| HIGH security gaps | 22 (1 accepted) | ≤3 | 0 |
| MEDIUM gaps | ~39 | ~20 | ≤10 |
| Substrates with CI+audit+deny | 1/5 | 5/5 | 5/5 with -D warnings |
| Client test coverage | 9.4% | 25%+ | 30%+ on critical paths |
| Cross-substrate fixture sync | 0/3 consumers | 3/3 | 3/3 with L1 tests green |
| Shared type crate | None | N/A (deferred) | ecosystem-types v0.1 |
| Nostr crate version alignment | Diverged (0.43/0.44) | N/A | Aligned |
| Event kind width | u16/u32/u64 | N/A | u64 everywhere |
| Client dead code | 23 components + 6 hooks | N/A | 0 |
| Lazy loading | 0 main features | N/A | All features |
| Licence compliance | AGPL violation | "Legal review" | Resolved |
| Dependency vulnerabilities | 0 CRITICAL, 5 HIGH | N/A | 0 CRITICAL, 0 HIGH |
| Production panic paths | 19 | ≤5 | 0 |
| Unbounded channels | 2 | N/A | 0 |
| Operational runbooks | 1 | 5 | 5 |
| Health schema contract | Diverged | N/A | Shared JSON schema |

---

## 9. What Remains After 90% (PRD-015 Scope)

The final 10% to reach full production readiness:

1. **IS-Envelope v1 runtime** — shared crate + JCS canonicalisation + per-substrate integration (2-3 sprints)
2. **Nostr relay mesh** — NIP-42 AUTH, kind-30033 anchors, peer discovery (3-5 sprints)
3. **NIP-26 cross-substrate unification** — converge on forum's implementation (2-3 sprints)
4. **OpenTelemetry distributed tracing** — OTEL exporter per substrate (2-3 sprints)
5. **WCAG 2.1 Level AA** — accessible graph alternative, keyboard nav (1-2 sprints)
6. **Cross-substrate integration tests** — docker-compose smoke suite (2-3 sprints)
7. **Coordinated release process** — compatibility matrix, versioning policy (1 sprint)
8. **Secret manager integration** — Vault/KMS for production key management (1 sprint)

---

## 10. Appendix: Agent Evidence Summary

### qe-security-deep (510s, 96 tool uses)
- 7 verified findings, 10 new findings (1 CRITICAL, 4 HIGH, 4 MEDIUM, 1 LOW)
- Key new: live API keys on disk, Cypher injection, unauthenticated git ingest, no payload limits
- Verified: WebAuthn sig missing, DOCKER_ENV bypass, NIP-98 replay

### qe-client-coverage (118s, 21 tool uses)
- Corrected XR room scene finding (properly externalized)
- Found 23 dead components, 6 dead hooks, 3 entirely dead feature flags
- GraphManager grown to 1,506 lines (43% increase since MEMORY.md snapshot)
- 21/23 features lack error boundaries
- Zero a11y testing, zero alt attributes

### qe-cross-substrate (215s, 42 tool uses)
- 3 verified findings, 10 new (0 CRITICAL, 7 HIGH, 7 MEDIUM)
- Key new: Nostr crate divergence, event kind triple-split, kind 30023 collision, empty fixture dirs
- Root cause identified: no shared type crate (NEW-I4) causes all type mismatches

### qe-rust-quality (329s, 24 tool uses)
- Found 19 production panic paths, 2 unbounded channels, poisoning-hazard RwLock
- 42 lock acquisition sites in GPU subsystem with no ordering protocol
- speech_service.rs nesting depth 29 (ecosystem complexity outlier)
- neo4rs pinned to 0.9.0-rc.8 (release candidate)
- 126 dead code suppressions in VisionClaw

### qe-ci-ops (197s, 49 tool uses)
- Overall verdict: **NO-GO** for ecosystem production deployment
- nostr-rust-forum: RED on all 6 dimensions
- solid-pod-rs, dreamlab-ai-website: 4 RED each
- Zero backup automation across entire ecosystem
- Zero cross-substrate integration tests

### qe-dependency-audit (171s, 31 tool uses)
- CRITICAL: AGPL solid-pod-rs compiled into MIT VisionClaw binary
- 3,934 total dependencies across ecosystem
- 5 HIGH JS vulnerabilities (axios, lodash — no upstream fixes)
- 3 unmaintained Rust crates, EOL rustls 0.21.x
- Zero cargo-deny configuration in any substrate
