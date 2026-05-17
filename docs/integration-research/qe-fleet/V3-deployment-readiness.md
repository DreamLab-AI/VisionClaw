# V3 — Deployment Readiness Assessment

| Field | Value |
|-------|-------|
| Status | Final (2026-05-07) |
| Author | QE Validator V3 — Deployment Readiness Agent |
| Scope | PRD-010 P0–P5 + PRD-011 X0–X6 mega-sprint go/no-go |
| Inputs | PRD-010, PRD-011, ADR-073..083 (11 ADRs), DDD mesh federation, Q1-Q5 audit (~6,070 lines), 01-06 surface research (~5,708 lines) |
| Companion validators | V1 (quality-gate-audit) — NOT YET WRITTEN; V2 (requirements-validation) — NOT YET WRITTEN |
| Verdict | **GO-WITH-CAVEATS** — see §D9 |

---

## Executive verdict

**GO-WITH-CAVEATS.** The specification corpus (PRDs, ADRs, DDD, audits) is internally coherent, file:line-precise, dependency-ordered, and individually implementable. The mega-sprint can launch IF AND ONLY IF four pre-sprint must-do items complete first: (1) `docs/specs/fixtures/` corpus seeded with at minimum the paulmillr/nip44 vectors (defends C1 by regression at sprint start, not sprint end); (2) the F26 WASM/CF Workers compatibility canary spike runs and produces a Pass/Fail verdict — its outcome decides whether ADR-076 absorption is on the critical path or rolled back to Shape C patch-in-place; (3) workspace `Cargo.toml` `nostr-sdk` pin upgrade from 0.43.0 to 0.44.x to eliminate the workspace/forum skew (PRD-010 F29 prerequisite); (4) `nostr-rust-forum` repo cloned locally and v3 import branch created (PRD-011 F1.1 prerequisite, currently zero LOC on local disk). Beyond those four, the sprint shape is constrained by 5 substrate × 11 sprint deliverable cross-product producing 47 fixture-pair contract tests and ~10 distinct CLI tools to be implemented, so phasing must be enforced strictly: PRD-010 P0 (gating) before everything; PRD-011 X0–X3 in parallel with PRD-010 P1–P3 once P0 lands; ADR-083 cutover absolutely last and gated on 7 consecutive nights of clean L2/L3 contract tests. A naive "all 15 agents in parallel" deployment will create cross-substrate diamond-problem failures that the L2 contract harness (the very tool needed to triage them) does not yet exist to detect.

---

## D1 — Critical-path readiness

PRD-010 Phase 0 is the single gating spec. Below: actionability per item.

| Item | Spec maturity | File:line precision | Blocked-on | Verdict |
|------|---------------|---------------------|------------|---------|
| **C1 — NIP-44 v2 conv-key (forum)** | Full | `dreamlab-ai-website/community-forum-rs/crates/nostr-core/src/nip44.rs:122-128` exact bug + spec citation; ADR-076 D1 specifies fix-by-deletion via upstream `nostr` crate | F26 spike | **READY** if F26 PASSES; **FALLBACK READY** (Shape C patch in place) if F26 FAILS |
| **C2 — agentbox bech32 npub** | Full | `agentbox/scripts/sovereign-bootstrap.py:90-91, 133-134, 192` exact lines; PRD-010 F5 + ADR-078 A1 specify replacement with `bech32` PyPI + BIP-340 x-only | None — Python-side, independent of forum absorption | **READY** |
| **C3 — verificationMethod.type drift** | Full | 3 patch sites: `solid-pod-rs/crates/solid-pod-rs-nostr/src/did.rs:98, 154` (`NostrSchnorrKey2024`), `agentbox/scripts/sovereign-bootstrap.py:192` (`SchnorrSecp256k1VerificationKey2022`); forum already aligned | None | **READY** |
| **C4 — agentbox 4th type drift** | Full | `agentbox/management-api/middleware/linked-data/surfaces/s04-did.js:71` (`SchnorrSecp256k1VerificationKey2025`); ADR-078 A4 specifies fix | None — same PR as C3 agentbox-side | **READY** |
| **C5 — NIP-42 challenge CSPRNG** | Full | `dreamlab-ai-website/community-forum-rs/crates/relay-worker/src/relay_do/session.rs:284-292`; ADR-077 P8 mandates CSPRNG audit lint | None | **READY** |
| **F26 — WASM/CF Workers spike** | Full | ADR-076 D5: spike at `crates/nostr-upstream-canary/`; acceptance criteria explicit (5 workers compile, +200KiB/+50ms/+500KiB-WASM budgets, paulmillr vectors pass) | **Operator must execute spike before any P0 deletions** | **NOT YET STARTED** — gating |
| **F1 — VisionClaw identity unification** | Full | `src/services/server_identity.rs:64-128` + `nostr_bridge.rs:62-94` + `nostr_bead_publisher.rs:46-70` | None | **READY** |
| **F2/F15 — DID Document handlers + dangling 307s** | Full | 4 routes specified at `src/handlers/uri_resolver_handler.rs:148-173` | None | **READY** |
| **F16 — RelayConsumer wired into agentbox boot** | Full | 10–15 line patch given verbatim in PRD-010 F16 | None | **READY** |
| **F18 — agentbox operator pubkey allowlist** | Full | `agentbox/flake.nix:732-736` exact fix | None | **READY** |
| **F25 — `nostr-core` per-module deletion** | Full | ADR-076 D6 specifies 12-step ordering; F28 specifies CI gating | F26 PASS | **READY** if F26 PASSES |

**Critical-path summary.** Every Phase 0 item has file:line precision. The single uncertainty is F26 (the spike), which is gating because its outcome bifurcates the rest of P0 into two distinct workstreams (Shape A absorption vs Shape C patch-in-place). The spike itself is fully specified and resourced (3–5 days, one engineer); its "not yet started" status is the only true blocker before sprint kickoff.

---

## D2 — Deployment risk inventory

| ADR | Subject | Risk | Mitigation documented? | Testable? | Rollback-able? |
|-----|---------|------|------------------------|-----------|----------------|
| ADR-073 | Mesh topology / fan-out | **MEDIUM** | YES — `mesh.federated_kinds` allowlist + LRU + tag-injection loop avoidance; per-relay rate limits | YES — federation smoke (P2 L3), bilateral admission test | YES — flip `mesh.mode = standalone` per substrate |
| ADR-074 | DID:Nostr canonicalisation | **LOW** | YES — single canonical `verificationMethod.type` + `@context`; CI assertion | YES — DID-doc-conformance fixture (ADR-082 D2) | YES — emitter rollback per substrate |
| ADR-075 | IS-Envelope v1 | **LOW** | YES — versioned `v: 1`; 7 explicit kinds; JCS canonical | YES — IS-envelope conformance fixture | YES — version field allows v2 alongside |
| ADR-076 | nostr-core absorption | **HIGH** | YES — F26 gating spike, D6 per-module ordering, D8 reference vectors, D10 fallback to Shape C | YES — paulmillr vectors + per-PR CI gates | YES IF caught at PR (per-module); HARD to roll back once cross-substrate flows depend on upstream types |
| ADR-077 | Ecosystem QE policy | **LOW** | YES — phased per PRD-010 §7; ~75 e-day programme | YES — each policy is a CI gate | YES — policies are configuration |
| ADR-078 | Cross-substrate library convergence | **MEDIUM** | YES — registry at `docs/ecosystem-libraries.md`; per-batch ADR | YES — per-batch reference vectors | YES — git revert per batch |
| ADR-079 | Forum-setup skill provider abstraction | **LOW** | YES — single Provider trait, 5 impls, fallback chain | YES — provider contract test | YES — provider switch via CLI flag |
| ADR-080 | Forum kit deployment topology | **MEDIUM** | YES — 9 named patterns (D1-D9), each TOML-shaped | PARTIAL — air-gapped + multi-tenant lack smoke fixtures | YES — TOML edit + redeploy |
| ADR-081 | Federation key custody & rotation | **HIGH** | YES — 3 tiers, per-role cadence, D7 emergency revocation, runbook templates | YES — D11 reference vectors + property tests | PARTIAL — emergency revoke is documented but unimplemented; key proliferation is operator-error-prone |
| ADR-082 | Cross-substrate fixture sharing | **MEDIUM** | YES — single source of truth at VisionClaw; copy-with-CI-check; SHA-256 verify | YES — D11 fixture freshness alerts | YES — refresh PR per upstream |
| ADR-083 | dreamlab-ai-website cutover | **EXTREME** | YES — 14-day phased ramp, dual-deploy, D9 rollback matrix, D11 19-item checklist | YES — D6 parity-check.sh + automated cohort-based comparison | YES — `wrangler secret put ROUTING_MODE old-only` reverts in <60s |

**Specific high-risk items per the brief:**

- **ADR-076 absorption (could break forum).** Risk class HIGH because failure mode is "all five CF Workers stop building" or "forum-client WASM bundle blows the 1 MiB CF Worker ceiling". Mitigation strength: F26 spike is a hard gate; per-PR CI runs `cargo build --target wasm32-unknown-unknown -p {auth,pod,relay,search,preview}-worker`; per-PR CI runs paulmillr vectors. Diagnosis: well-mitigated but only IF the spike runs first. Go condition: F26 produces a verdict before any per-module deletion PR opens.
- **ADR-083 cutover.** Risk class EXTREME because this is the only ADR touching live production. Mitigation strength: dual-deploy + D3 share-storage means the cutover is purely deployment-layer (no data migration); D5 session continuity by passkey-PRF + identical rp_id; D9 rollback matrix gives <60s reversion path; D11 pre-flight checklist 19 items gates traffic split. Diagnosis: well-mitigated, but the prerequisites (D13) are extensive — cutover cannot proceed until ADR-082 fixture sharing is active in CI for ≥7 days clean AND PRD-010 P0 has shipped. **The sprint MUST NOT attempt cutover and core implementation in parallel; cutover is the post-sprint demonstration.**
- **ADR-081 key rotation tooling (operator-facing).** Risk class HIGH because 10+ key roles + 3 custody tiers × N substrates create operator surface area where collapse-to-shared-key (Q3 §I12 warning) is the predictable failure mode. Mitigation strength: D8 anti-collision lint (`custody verify`) catches role-key reuse; D9 commits 15 new CLI commands across 3 substrates; D10 `/health/keys` Prometheus exposure. Diagnosis: spec is complete; risk lives in operator adoption. **The sprint should ship the CLI tools but defer cadence enforcement (D6 weekly federation rotation) until operator runbooks land in §D5.**
- **PRD-011 G7 de-branding completeness.** Risk class HIGH per PRD-011 R1 ("DreamLab-specific strings hide in unexpected places"). Mitigation strength: F2.6 anti-drift CI lint; M1 zero-substring assertion. Diagnosis: easy to mis-scope. **Sprint must apply the anti-drift CI lint at F2.6 BEFORE any bulk import commit lands at F1.3** — otherwise the import commit itself reintroduces strings the lint would have caught.
- **F18 agentbox port 7777 exposure (security-critical).** Risk class HIGH because exposing `127.0.0.1:7777` to `0.0.0.0:7777` over a public CF Worker URL creates a publicly-reachable Nostr relay defended only by NIP-42 AUTH + `pubkey_whitelist`. Mitigation strength: F18 fixes the auto-allowlist drift; F17 documents the trade-off; ADR-073 R4 makes it explicit. Diagnosis: acceptable trade-off but operator runbook MUST land at the same time as the port-flip — sprint deliverable.

---

## D3 — Operational dependencies

Each cross-cutting dependency is real, has an owner (or doesn't), and is a potential blocker.

| Dependency | Identified? | Owner | Blocked? |
|------------|-------------|-------|----------|
| Cloudflare Workers deployment access (forum kit) | YES — ADR-080 D2 | Forum kit operator | **OPEN** — must be provisioned for X0 deployment smoke |
| Cloudflare Workers deployment access (router-worker for ADR-083) | YES — ADR-083 D2 | DreamLab ops | **OPEN** — needed at T₂ |
| GitHub repo access — `DreamLab-AI/nostr-rust-forum` (write) | YES — PRD-011 F1.1, F1.4 | DreamLab-AI org | UNCLEAR — repo exists publicly, write access not validated |
| GitHub repo access — `DreamLab-AI/dreamlab-ai-website` (write) | YES | DreamLab-AI org | OK — repo present locally |
| GitHub repo access — `DreamLab-AI/agentbox` (write, submodule) | YES | DreamLab-AI org | OK — submodule wired |
| GitHub repo access — `DreamLab-AI/solid-pod-rs` (write, workspace) | YES | DreamLab-AI org | OK — workspace present locally |
| GitHub repo access — `DreamLab-AI/VisionClaw` (write, integrator) | YES | DreamLab-AI org | OK — this repo |
| KV namespace provisioning (`NIP98_REPLAY` shared) | YES — PRD-010 F20 | DreamLab CF account | **OPEN** — provisioning placeholder per Sprint v9 |
| Secret store provisioning per ADR-081 D2 tier-2 custody | YES — ADR-081 D2 | Per substrate operator | **OPEN** — no Tier-2 secrets exist; CF Worker secrets need names |
| nostr-rust-forum LOCAL clone | YES — PRD-011 F1.1: `/home/devuser/workspace/nostr-rust-forum/` | Sprint kickoff | **MISSING** — directory does not exist locally |
| `nostr-sdk` 0.43 → 0.44 workspace bump | YES — PRD-010 F29 | VisionClaw maintainer | **MISSING** — workspace at 0.43.0; forum at 0.44 (skew) |
| `agentic-workstation` (port 9090) availability for sprint orchestration | YES — `multi-agent-docker/CLAUDE.md` | Sprint operator | OK — running |
| `agentbox` v2 (ports 9190/8180/etc.) for federation peer testing | YES | Sprint operator | OK — running per project CLAUDE.md |
| RuVector PostgreSQL for swarm memory | YES | Container | OK — `ruvector-postgres:5432` |
| Codebase Memory MCP indexing | YES — `home-devuser-workspace-project` | Sprint operator | OK — 48,159 nodes indexed |
| paulmillr/nip44 GitHub access (for fixture vendoring) | YES — ADR-082 D2 | Public GitHub | OK |
| nostr-protocol/nips reference vector access | YES | Public GitHub | OK |

**Critical operational blockers (must clear before sprint kickoff):**
1. **Local clone of nostr-rust-forum** at `/home/devuser/workspace/nostr-rust-forum/` (PRD-011 F1.1).
2. **Workspace `nostr-sdk` 0.43→0.44 bump** (PRD-010 F29; resolves duplicate-compile note in current Cargo.toml).
3. **CF Worker secrets** named per ADR-081 D2 template (`mesh/{role}/v{version}`).
4. **F26 spike crate** at `community-forum-rs/crates/nostr-upstream-canary/` provisioned with CF Worker deployment slot.

---

## D4 — Skills/tooling/agent inventory

The mega-sprint will not execute without a substantial CLI/tooling stack. Each item below has a spec; none are implemented.

| Tool / skill / agent | Spec exists? | Substrate | Priority | Skill-builder applicable? |
|----------------------|:------------:|-----------|:--------:|:------------------------:|
| `forum-setup` skill | YES — ADR-079 | nostr-rust-forum + agentbox + Claude Code | P1 | YES — D10 lists 3 entry points (agentbox/skills, ~/.claude/skills, cargo install) |
| `nostr-bbs-admin` CLI | YES — PRD-011 F8 + ADR-081 D9 | nostr-rust-forum | P1 | partial — Rust CLI; not a skill per se |
| `vc-cli identity` + `vc-cli mesh` | YES — ADR-081 D9 | VisionClaw | P0 (operator key gen) | NO — Rust CLI |
| `nostr-bbs-migrate` CLI | YES — ADR-083 D8 | nostr-rust-forum | P3 (cutover-only) | NO |
| `agentbox sovereign federation-keygen` subcommand | YES — ADR-081 D3 | agentbox | P1 | NO |
| `sync-fixtures.sh` per substrate | YES — ADR-082 D5 (full bash listing) | All 5 substrates | P0 (gating) | NO — shell script |
| Cross-substrate L2 contract test harness | YES — ADR-082 D7 + Q5 T7 | VisionClaw monorepo | P3 (post-P2 reference vectors) | partial — `agentic-qe`/`build-with-quality` skills cover |
| `router-worker` CF Worker | YES — ADR-083 D2 | dreamlab-ai-website | P3 (cutover-only) | NO |
| `forum-config/` package | YES — PRD-011 F8 + ADR-080 D7 | dreamlab-ai-website | P3 | NO |
| Anti-drift CI lint (clippy `urn-visionclaw-format`) | YES — PRD-010 F23 | VisionClaw | P0 | partial — covered by `code-review-quality` skill |
| ESLint `no-ad-hoc-urn` for agentbox | YES — PRD-010 F23 | agentbox | P0 | partial — covered by skill |
| Federation smoke harness (P2 L3) | YES — ADR-077 P2 + Q5 T8 | VisionClaw integrator | P3 | partial — `chaos-engineering-resilience` + `e2e-flow-verifier` skills compose |
| Bundle-size CI gate | YES — ADR-077 P7 | nostr-rust-forum CI | P1 | NO — wrangler-tooled |
| Mutation testing config | YES — ADR-077 P4, tools pinned | All 5 substrates | P3 | YES — `mutation-testing` skill applicable |
| Property test harness per DDD invariant | YES — ADR-077 P5; F-Inv-01..07, A-Inv-01..09, V-Inv-01..07, S-Inv-01..04 | All 4 substrates | P3 | YES — `qe-test-generation` skill covers |
| Reference-vector consumer harnesses | YES — ADR-082 D6 (full Rust + JS code listings) | All 4 substrates | P0 (gating) | YES — `qe-test-generation` |

**Tooling readiness summary.** Of the 16 tools/harnesses required: ZERO are implemented today; 16 have file:line specifications. Five are P0 (gating) or P1 (early sprint), and these MUST be the first agents' deliverables. The Cross-substrate L2 contract test harness alone is ~12 engineer-days per Q4 G14 estimates. **The sprint cannot use these tools to validate itself; the early agents must build them, then later agents use them.** This phasing constraint is non-negotiable.

---

## D5 — Documentation gaps

Documents referenced by ADRs that do not yet exist on disk. Severity: HIGH = blocks sprint exit, MEDIUM = blocks P5 consolidation, LOW = nice-to-have.

| Document | Referenced by | Exists? | Severity |
|----------|---------------|:-------:|:--------:|
| `docs/identity-contracts.md` | ADR-074 D2 | NO | **HIGH** — load-bearing per the brief; specifies cross-substrate canonical DID Document contract |
| `docs/ecosystem-libraries.md` | ADR-078 D1 | NO | MEDIUM — registry of upstream library pins |
| `docs/specs/fixtures/` directory | ADR-082 D1 | NO | **HIGH** — entire test-surface foundation |
| `docs/specs/fixtures/UPSTREAM_PINS.md` | ADR-082 D2 | NO | **HIGH** — gating per ADR-082 Phase 0 |
| `docs/specs/fixtures/COVERAGE_MATRIX.md` | ADR-082 D3 | NO | **HIGH** — gating |
| `docs/specs/fixtures/error-class-mapping.md` | ADR-082 D9 | NO | MEDIUM |
| `docs/operations/triage-l2-failure.md` | ADR-082 D8 + ADR-077 P9 | NO | MEDIUM (Phase 3) |
| `docs/operations/cutover-log.md` | ADR-083 D12 | NO | LOW (filled at cutover time) |
| `docs/operations/key-rotation-log.md` | ADR-081 implementation note | NO | LOW (filled at first rotation) |
| `docs/deployment/runbooks/rotate-{role}.md` (multiple) | ADR-081 implementation note | NO | MEDIUM |
| `docs/deployment/key-rotation.md` (per substrate) | ADR-081 D9 negative | NO | MEDIUM |
| `agentbox/docs/user/mesh-deployment.md` | PRD-010 F17 | NO (in agentbox repo) | MEDIUM |
| Per-substrate CLAUDE.md `Ecosystem & Federation` sections | PRD-011 F11 | PARTIAL — VisionClaw has, others don't | MEDIUM |
| Kit-side ADR-001 onward | PRD-011 F9.3 | NO (kit repo not cloned) | LOW (Phase X1 deliverable) |
| `docs/operations/triage-l2-failure.md` | ADR-082 D8 | NO | MEDIUM |
| `/health/qe`, `/health/mesh`, `/health/keys` endpoints | ADR-077 P10, ADR-073 D11, ADR-081 D10 | NO (handlers unimplemented) | MEDIUM |

**Most-urgent document.** `docs/identity-contracts.md` (ADR-074 D2). It is referenced as the load-bearing canonical contract for cross-substrate DID Document interoperability. Without it, F4 (verificationMethod canonicalisation) is operating from inline ADR text rather than a citable artefact, and any disagreement between substrates over DID Document shape has no triage authority. **Recommendation: write this document as part of Pre-sprint must-do list (§Pre-sprint).**

---

## D6 — Test surface readiness

The gap between "test surface specified" and "test surface gating the sprint" is enormous.

| Category | % complete | What's missing |
|----------|:----------:|---------------|
| Unit test count (within-substrate) | **~100%** | ~3,937 protocol-relevant assertions exist (Q4 G1.5); this is the strongest layer and is the only thing approaching adequacy |
| Reference vectors (P1, ADR-077) | **0%** | All 13 fixtures named in ADR-082 D1 are absent. paulmillr/nip44 alone is the load-bearing C1 regression guard — it does not exist anywhere. |
| L1 within-substrate contract tests (P2) | **~30%** | Forum has proptest fixtures (NIP-19, NIP-04 round-trips); agentbox has linked-data contract tests at `tests/contract/linked-data/`; VisionClaw has no equivalent for crypto/protocol surface; solid-pod-rs's `wac_inheritance.rs` is the gold standard but isn't replicated cross-substrate |
| L2 cross-substrate contracts (P2) | **0%** | Q4 G3 finding: zero contract tests exist anywhere in the ecosystem. ADR-082 D7 specifies the harness; spec exists; not built. |
| L3 federation smoke (P2) | **0%** | Q4 G4 finding: no test boots ≥2 substrates. Q5 T7 specifies the docker-compose-shape harness; not built. |
| Mutation testing (P4) | **0%** | Tools pinned in ADR-077 (cargo-mutants, stryker, mutmut) but no cargo-mutants config in any tree. Per-substrate kill-rate targets specified; no baseline measured. |
| Fuzz targets (P5) | **~5%** | One fuzz target exists at `crates/visionclaw-xr-presence/fuzz` (not protocol-relevant). Q5 T10 specifies 4 fuzz targets including the differential JCS fuzzer; not built. |
| Property tests for DDD invariants (P5) | **~10%** | Forum has 35 proptest assertions on NIP-19/NIP-04. ADR-077 P5 mandates F-Inv-01..07, A-Inv-01..09, V-Inv-01..07, S-Inv-01..04 = 27 invariants. ~3 covered. |
| Coverage thresholds (P6) | **untracked** | No coverage data published for any substrate. Q4 baseline implies ≥75% line for VisionClaw; not measured per-PR. |
| Bundle-size CI gates (P7) | **0%** | No bundle-size measurement in any CF Worker; ADR-077 P7 specifies 1MiB ceiling + 200KiB delta budget |
| Security CI gates (P8) | **partial** | Q2 mentions clippy lints active; gitleaks/cargo deny/eslint security NOT installed across all substrates |
| `/health/qe` dashboard (P10) | **0%** | Per-substrate handlers not implemented; aggregator not built |
| **Aggregate test surface readiness** | **~15%** | The substrate-local test corpus is healthy; the cross-substrate test surface — which is what the mega-sprint output needs to validate against — is functionally absent. |

**Specific concrete numbers.** ADR-082 references **13 fixture files** total. Currently on disk: **0** (`docs/specs/fixtures/` does not exist). The single most leveraged Phase 0 deliverable is `nip44-v2.json` (paulmillr vectors); writing it is hours of work and prevents C1 regression for the entire sprint duration.

ADR-077 P2 L2 specifies that **cross-substrate contracts run nightly** — meaning the harness MUST exist before nightly runs become meaningful. If sprint kickoff is Day 1, the L2 harness MUST land before Day ~14 or no L2 runs accumulate before sprint exit. Current ETA per Q4 G14: 12 engineer-days. **This is the single highest-leverage early-sprint deliverable.**

---

## D7 — Multi-substrate coordination

Five substrates, each with its own GitHub repo, CI pipeline, deployment topology.

```
DreamLab-AI/VisionClaw       (this repo; integrator; fixture host; L2 harness)
DreamLab-AI/nostr-rust-forum (kit; canonical home of nostr-bbs-rs)
DreamLab-AI/agentbox         (submodule; sovereign agents + relay)
DreamLab-AI/solid-pod-rs     (workspace; foundation library)
DreamLab-AI/dreamlab-ai-website (downstream consumer; pre-cutover holds community-forum-rs/)
```

**Cross-repo dependency sequencing (load-bearing for sprint phasing):**

```
Phase 0 (gating)
  ├── F26 spike (forum/community-forum-rs)
  ├── docs/specs/fixtures/ seeded (VisionClaw)
  ├── C1-C5 + F1/F4/F5 fixes (3 repos parallel)
  └── nostr-sdk 0.43→0.44 (VisionClaw workspace)

Phase 1 (DID/resolver convergence)         Phase X1 (kit workspace restructure)
  ├── F2/F15 routes (VisionClaw)             ├── nostr-bbs-config crate
  ├── DID emitter alignment (3 repos)        ├── nostr-bbs-mesh crate
  └── F3 service entries                     └── anti-drift CI

Phase 2 (AUTH+delegation)                  Phase X2 (kit library convergence)
  ├── F7/F8 (forum + agentbox + VC)          ├── ADR-076 absorption (kit)
  ├── F9 bridge re-signing                   └── ADR-078 B3 webauthn-rs

Phase 3 (bridge/fan-out)                   Phase X3 (kit QE compliance)
  ├── F11/F12/F16/F17/F22                    ├── reference vectors landed
  ├── L2 harness landed (VisionClaw)         ├── mutation baseline
  └── ADR-082 sync-fixtures.sh active        └── Coverage gates

Phase 4 (envelope contract)                Phase X4 (forum-setup skill)
  ├── F10/F14/F20/F21                        └── ADR-079 5 providers
  └── ADR-075 conformance tests

Phase 5 (consolidation)                    Phase X5 (cutover) ← ADR-083 GATE
  ├── F13/F23/F24                            └── 14-day phased ramp; cutover only after
  ├── /health/qe deployed                        - PRD-010 P0 shipped
  └── Operator runbook                           - L2 harness running ≥7 nights clean
                                                  - L3 federation smoke ≥7 nights clean
```

**Single coordination point.** Per ADR-082 D1 + ADR-077 P9, **VisionClaw is the integration substrate**. The L2 contract test harness lives in `<VisionClaw>/tests/cross_substrate/`, the master fixture corpus at `<VisionClaw>/docs/specs/fixtures/`, and the federation smoke harness at `<VisionClaw>/.github/workflows/federation-smoke.yml`. This is the right call — having a single integrator avoids N×N coordination overhead — but it makes VisionClaw a critical-path bottleneck. **VisionClaw repo CI MUST be set up first.** Q4 G1.3 finding: "There is no Rust CI workflow for the VisionClaw substrate itself." This is the single largest one-step ROI fix in the ecosystem and must be the first sprint deliverable.

**Cross-repo PR sequencing rule.** ADR-082 D10 mandates synchronised PRs across substrates when fixtures change. The sprint produces ≥30 PRs across 5 repos; without explicit sequencing, fixture-refresh PRs in VisionClaw will land before consumer-substrate sync PRs land, causing CHECKSUM.txt drift and CI failures. **Recommend: a single agent with `github-multi-repo` skill owns the cross-repo sequencing at all times during the sprint.**

**Coalition vs monolith.** The right model is one coordinator agent maintaining the cross-repo PR queue (a "release-train" pattern), with per-substrate worker agents sitting in worktrees within their substrate's repo. This matches the topology recommendation in §D8.

---

## D8 — Sprint-ability assessment

Per VisionClaw's `CLAUDE.md` and `multi-agent-docker/CLAUDE.md`:

| Setting | Recommendation | Justification |
|---------|----------------|---------------|
| Topology | `hierarchical-mesh` | One coordinator + per-substrate sub-coordinators + worker agents; matches the §D7 release-train pattern |
| Max agents | **15** (the project ceiling) | 1 coordinator + 5 substrate-leads + 8 specialist agents (crypto, fixtures, CI, docs, cutover, QE, tooling, integration) + 1 sentinel |
| Strategy | `specialized` | Each substrate-lead owns their substrate; specialists drop in for their phase |
| Consensus | `raft` (f<n/2) | At 15 agents, raft tolerates 7 failures; matches the dependency-ordered phase structure |
| Memory | `hybrid` (RuVector backed) | Per CLAUDE.md; persists patterns across phase transitions |
| HNSW indexing | enabled | Cross-substrate semantic recall over PRDs/ADRs/Q-audits |
| Neural | enabled | Per-task model recommendation hints |
| Skill stack (must-load) | `agentic-qe`, `build-with-quality`, `codebase-memory`, `sparc-methodology`, `mutation-testing`, `qe-test-generation`, `swarm-advanced`, `github-multi-repo`, `swarm-orchestration` | These cover the contract-testing, multi-repo coordination, and QE-policy enforcement workflow |
| Skill stack (phase-specific) | `agentic-jujutsu` (Phase 0 git discipline), `chaos-engineering-resilience` (Phase 3 federation smoke), `cicd-pipeline-qe-orchestrator` (Phase 5 release-train), `defense-security` (Phase 0 C5 CSPRNG audit), `verification-quality` (Phase 0 fixture vendoring) | Loaded per phase boundary |
| Topology decision | **HIERARCHICAL-MESH not flat MESH** | A flat 15-agent mesh would create cross-substrate diamond-problem (§D2 ADR-082 D8) on every phase boundary. Hierarchical-mesh routes substrate-internal decisions to substrate-leads and only escalates cross-substrate decisions to the coordinator. |
| Single mega-sprint vs phased rollout? | **Phased mega-sprint** with hard phase gates | Phase 0 must complete before Phase 1; Phase X5 (cutover) cannot execute in parallel with Phase 0–4 |

**Recommended `swarm init` shape (per `claude-flow swarm init`):**

```bash
claude-flow swarm init \
  --topology hierarchical-mesh \
  --max-agents 15 \
  --strategy specialized \
  --consensus raft \
  --memory hybrid \
  --hnsw-indexing \
  --neural-enabled \
  --skill-stack 'agentic-qe,build-with-quality,codebase-memory,sparc-methodology,mutation-testing,qe-test-generation,swarm-advanced,github-multi-repo,swarm-orchestration' \
  --phase-gates 'P0-gating,P1-2-parallel,P3-bridge,P4-envelope,P5-consolidation,X5-cutover-locked' \
  --memory-namespace 'aqe/deployment/prd-010-011' \
  --checkpoint-cadence '4h' \
  --task-id 'mesh-mega-sprint-2026-05'
```

**Worktree model.** Per `multi-agent-docker/CLAUDE.md` and `lazy-fetch` blueprint conventions: each substrate-lead opens a git worktree inside its target repo; specialist agents (e.g. crypto specialist) move between worktrees as phases dictate. The coordinator agent itself sits in a non-worktree shell because it spans repos.

---

## D9 — Risk-weighted go/no-go decision

**VERDICT: GO-WITH-CAVEATS.**

The decision is enabled by:
- **Specification completeness.** PRD-010 + PRD-011 + 11 ADRs + DDD + 5 surface researches + 5 audits = ~36,400 lines of internally consistent, file:line-precise specification. Zero spec gaps that would block agent execution.
- **Risk inventory completeness.** Every HIGH/EXTREME risk (§D2) has a documented mitigation; every mitigation is testable.
- **Fallback path defined.** F26 spike outcome bifurcates Phase 0 cleanly into Shape A (absorption) or Shape C (patch-in-place); both paths fully specified.
- **Critical-path tooling has specs.** §D4 inventory of 16 tools, all spec'd; phase ordering ensures early agents build the harnesses later agents need.
- **Cross-repo sequencing model defined.** ADR-082 + ADR-083 establish single integrator (VisionClaw) + release-train PR pattern.
- **Cutover carefully isolated.** ADR-083 D13 explicitly gates cutover on PRD-010 P0 + ADR-082 + ADR-081 + 7-night clean L2/L3 — no risk of cutover happening mid-sprint.

The decision is gated by:
1. **F26 WASM/CF Workers spike has not started.** This is the single largest unknown. Until the spike returns, ADR-076 absorption status is "Proposed" — sprint cannot commit to it as the implementation strategy.
2. **`docs/specs/fixtures/` is empty.** Without paulmillr/nip44 vectors landed before any code change, every Phase 0 PR ships without its regression guard.
3. **`nostr-rust-forum` not cloned locally.** PRD-011 F1.1 prerequisite missing.
4. **Workspace `nostr-sdk` skew (0.43.0 vs forum 0.44).** PRD-010 F29 prerequisite missing; not blocking but creates confusion.
5. **Companion validators V1, V2 not yet written.** This V3 is producing in advance of the cohesion (V1) and traceability (V2) outputs; cohesion/traceability findings could surface gaps not covered here. **Recommendation: do not launch sprint until V1 and V2 land and align with V3.**
6. **VisionClaw has no Rust CI workflow.** Q4 G1.3 finding. Sprint deliverable can land it but until it does, CI gates from ADR-077 are not enforceable in the integrator repo.

**If all 6 caveats clear: proceed to launch.**
**If F26 spike fails:** sprint launches under Shape C plan; ADR-076 status moves to Rejected; Phase 0 collapses to ~1 sprint instead of ~2; total scope shrinks by ~1 sprint of net work; sprint shape unchanged otherwise.
**If V1 or V2 surface contradictions with V3:** re-validate before launch.

---

## D10 — Recommended sprint shape

**Sprint scope.** PRD-010 P0–P5 + PRD-011 X0–X4 in single mega-sprint. **PRD-011 X5 (dreamlab-ai-website cutover) explicitly DEFERRED to a post-sprint operations workstream.** Per ADR-083 D13, cutover is gated on the sprint's deliverables landing first; bundling cutover with sprint creates an EXTREME-risk release.

**Topology.**
```yaml
topology: hierarchical-mesh
coordinator: 1 agent (Sprint Coordinator)
substrate_leads: 5 agents
  - VisionClaw lead (this repo)
  - nostr-rust-forum lead (kit)
  - agentbox lead (submodule)
  - solid-pod-rs lead (workspace)
  - dreamlab-ai-website lead (consumer; X5 deferred so this lead is light)
specialists: 8 agents
  - Crypto specialist (C1-C5, F4-F6, F25)
  - Fixtures specialist (ADR-082 D1 corpus + sync-fixtures.sh)
  - CI specialist (ADR-077 P1-P9 gates per substrate)
  - Identity/DID specialist (F1-F4, F14, F15, ADR-074)
  - Mesh/federation specialist (F7-F12, F16-F18, ADR-073)
  - Tooling specialist (CLI tools per ADR-081 D9 + ADR-079)
  - Docs specialist (D5 documentation gaps, ADR-074 D2)
  - QE harness specialist (L2/L3 contracts, mutation, fuzz)
sentinel: 1 agent (cross-cutting safety, drift detection, V1/V2 alignment monitor)
TOTAL: 15 agents
```

**Phase gating (hard gates):**
1. **G0 — Pre-sprint must-do (§Pre-sprint) clears** before Day 1.
2. **G1 — Phase 0 lands** (C1-C5 fixed, F26 spike verdict, F1/F4/F5 fixed, fixture corpus seeded, paulmillr vectors verified) before Phase 1 starts.
3. **G2 — Phase 1+2 gate** (DID/resolver/AUTH/delegation done across 3 substrates) before Phase 3.
4. **G3 — L2 harness operational** (cross-substrate fixture sharing CI green ≥3 nights) before Phase 4.
5. **G4 — Phase 4 envelope contract** before Phase 5.
6. **G5 — Sprint exit** = §Post-sprint criteria below.

**Quorum/consensus.** Raft at f<n/2 means up to 7 agent failures tolerated (over 15 agents). For phase-gate decisions, require 2/3 agreement among coordinator + relevant substrate leads.

**Checkpoint cadence.** Every 4h: each agent commits progress to `aqe/deployment/checkpoint/<agent-id>` namespace; coordinator aggregates into `aqe/deployment/sprint-status` summary; sentinel runs drift-detection on cross-substrate PRs. End-of-day: phase-gate check.

**Memory namespace plan:**
```
aqe/deployment/decision/<task_id>      → GO/NO-GO per phase
aqe/deployment/risk-score/<phase>      → per-phase risk delta tracking
aqe/deployment/confidence/<phase>      → release-confidence metric per phase exit
aqe/deployment/checklist/<phase>       → phase-gate item completion
aqe/deployment/rollback-plan/<adr>     → per-ADR rollback procedures
aqe/learning/patterns/deployment/*     → patterns fed to subsequent sprints
```

---

## Pre-sprint must-do list

Time-estimated, ordered for parallel execution where possible.

| # | Item | Owner | Effort | Blocks |
|---|------|-------|--------|--------|
| 1 | Clone `DreamLab-AI/nostr-rust-forum` to `/home/devuser/workspace/nostr-rust-forum/` (PRD-011 F1.1) | DreamLab ops | 5 min | All PRD-011 work |
| 2 | Bump VisionClaw workspace `nostr-sdk = "0.43.0"` → `"0.44"` and add `nostr = "0.44"` (PRD-010 F29) | VisionClaw lead | 30 min + cargo build verify ~10 min | All F25/F29 cross-type-flow work |
| 3 | Create `docs/specs/fixtures/` skeleton with `README.md`, `UPSTREAM_PINS.md`, `COVERAGE_MATRIX.md` (ADR-082 Phase 0) | Fixtures specialist | 2 hours | Every reference-vector test in Phase 0 |
| 4 | Vendor paulmillr/nip44 vectors as `docs/specs/fixtures/nip44-v2.json` (ADR-082 Phase 0) | Fixtures specialist | 1 hour | C1 regression guard for ADR-076 PRs |
| 5 | Vendor BIP-340 + RFC 8785 JCS vectors (ADR-082 Phase 0) | Fixtures specialist | 2 hours | C5 conformance, IS-Envelope canonicalisation |
| 6 | Author `did-doc-conformance.json` + `is-envelope-v1.json` DreamLab-internal contracts (ADR-082 D13/D2) | Crypto + Identity specialists | 4 hours | C3/C4 verification, ADR-075 conformance |
| 7 | Write `docs/identity-contracts.md` referenced by ADR-074 D2 | Docs specialist | 4 hours | F4 emitter compliance citation |
| 8 | Provision F26 spike crate at `community-forum-rs/crates/nostr-upstream-canary/` and execute spike per ADR-076 D5 (bundle-size + cold-start + paulmillr vectors) | Forum lead + Crypto specialist | 3-5 days | ADR-076 absorption decision; entire P0 strategy |
| 9 | Set up VisionClaw Rust CI workflow at `.github/workflows/rust-ci.yml` (Q4 G1.3) | CI specialist | 4 hours | Every CI gate downstream |
| 10 | Provision Cloudflare Worker secret namespaces per ADR-081 D2 template (`mesh/{role}/v{version}`) | DreamLab ops | 1 hour | Tier-2 custody flows |
| 11 | Create `forum-setup` skill scaffold per ADR-079 D2 (5 provider stubs, conversation flow registered) | Tooling specialist | 1 day | Phase X4 work |
| 12 | Write `docs/operations/triage-l2-failure.md` runbook per ADR-082 D8 | Docs + QE specialists | 3 hours | L2 harness usability |
| 13 | Confirm V1 (quality-gate-audit) and V2 (requirements-validation) ≡ V3 findings | Sentinel agent | 1 day after V1+V2 land | Sprint kickoff alignment |

**Total pre-sprint effort: ~5-6 engineer-days, assuming items run in parallel and the F26 spike is the longest pole.**

---

## Post-sprint validation criteria

Sprint exits when ALL of the following are true. These ARE the success metrics; each maps to an ADR or PRD success metric.

### Spec-level exits
- [ ] PRD-010 G1 — single canonical `did:nostr` resolution path operational across 3 substrates
- [ ] PRD-010 G2 — IS-Envelope v1 reference impl in `nostr-core`; 7 kinds round-trip
- [ ] PRD-010 G3 — 3 relay topologies serve identical NIP-11 capability set; bilateral admission tested
- [ ] PRD-010 G4 — All-write NIP-42 AUTH; client AUTH-RESP code paths active in forum-client + RelayConsumer + nostr_bridge
- [ ] PRD-010 G5 — NIP-26 delegation verifier wired in 3 event-ingest paths
- [ ] PRD-010 G6 — Forum user can DM agentbox agent over mesh; round-trip ≤ 5s (M1)
- [ ] PRD-010 G7 — Each substrate's `[mesh]` block in manifest; `mesh.mode` switch verified
- [ ] PRD-010 G8 — C1, C2, C3, C4, C5 all closed; reference vectors guard against regression
- [ ] PRD-010 G9 — Forum nostr-core ≤1,000 LOC (post-absorption); upstream `nostr` integrated
- [ ] PRD-010 G10 — Library convergence registry at `docs/ecosystem-libraries.md`
- [ ] PRD-011 G1 — Kit deployable from single TOML; `forum-setup wizard` produces valid TOML <15min (M4)
- [ ] PRD-011 G2 — Federation native in kit; `[mesh].mode` switch operational
- [ ] PRD-011 G3 — DreamLab consumer pattern validated via `forum-config/` package skeleton (cutover deferred)
- [ ] PRD-011 G7 — Anti-drift CI lint zero violations on `nostr-rust-forum/`
- [ ] PRD-011 G9 — ADR-077 P1-P10 gates active; CI green
- [ ] PRD-011 G10 — Kit `nostr-bbs-core` ≤700 LOC; ADR-076 absorption from inception

### Test-surface exits
- [ ] All 13 fixtures in `docs/specs/fixtures/` exist + have JSON Schemas
- [ ] L1 reference vector tests passing per substrate per fixture (47 substrate × fixture pairs)
- [ ] L2 cross-substrate contract tests running nightly ≥7 nights clean before Day-of-exit
- [ ] L3 federation smoke test running nightly ≥7 nights clean before Day-of-exit
- [ ] Mutation testing baseline established; per-substrate kill-rates measured at ≥75% of ADR-077 P4 targets
- [ ] Coverage thresholds met per ADR-077 P6 baselines
- [ ] Anti-drift CI lints active in 4 substrates; zero open violations
- [ ] Bundle-size CI gates active on 5 CF Workers; all under 1MiB; per-PR delta budget enforced
- [ ] CSPRNG audit lint catches `Math.random()` in any auth path (test: red-team PR)
- [ ] DID Document type-string assertion CI green

### Tooling exits
- [ ] All 16 tools in §D4 implemented + smoke-tested
- [ ] `vc-cli identity` + `vc-cli mesh` + `nostr-bbs-admin` + `agentbox sovereign federation-keygen` all produce valid output for `keygen --role=<role>` + `custody verify`
- [ ] `forum-setup` skill answers structured questions via 5 providers; produces valid TOML
- [ ] `sync-fixtures.sh --verify` green in 5 substrates

### Documentation exits
- [ ] `docs/identity-contracts.md` written (was Pre-sprint must-do #7)
- [ ] `docs/ecosystem-libraries.md` written (PRD-011 / ADR-078)
- [ ] `docs/operations/triage-l2-failure.md` written
- [ ] `docs/deployment/runbooks/rotate-{role}.md` for each rotation cadence in ADR-081 D6
- [ ] `agentbox/docs/user/mesh-deployment.md` written
- [ ] Per-substrate `Ecosystem & Federation` section in CLAUDE.md (5 repos)
- [ ] Kit-side `docs/adr/ADR-001..004` per PRD-011 F9.3
- [ ] Per-substrate `/health/qe`, `/health/mesh`, `/health/keys` endpoints exposing JSON

### Operational exits
- [ ] CF Worker secrets named + populated per ADR-081 D2 template
- [ ] Each substrate's CI workflow green for ≥3 consecutive PRs
- [ ] Cross-repo PR sequencing log shows zero CHECKSUM.txt drift incidents
- [ ] M2 — 100 random hex pubkeys × 2 substrates = ≥95% Tier-3 DID resolution
- [ ] M3 — 100% NIP-26 delegation verification rate on cross-substrate events
- [ ] M5 — Federation overhead ≤30ms median per peer relay; ≤5% volume amplification

### Governance exits
- [ ] All ADRs (073-083) status moves from "Proposed" to "Accepted" (or "Rejected" with documented rationale)
- [ ] PRD-010 §11 success metrics M1-M5 measured and documented
- [ ] PRD-011 §10 success metrics M1-M6 measured (M5 dropped for deferred cutover)
- [ ] Sprint retrospective in `docs/operations/sprint-mesh-mega-2026-05-retro.md` with what worked / what did not / patterns extracted to RuVector `aqe/learning/patterns/deployment/*`

### Cutover exit (ADR-083 — separate post-sprint trigger)
ADR-083 cutover begins ONLY when the above sprint exits AND:
- [ ] L2 + L3 nightly tests have been clean for ≥7 consecutive nights post-sprint
- [ ] ADR-081 federation key custody decisions made for DreamLab production deployment
- [ ] ADR-083 D11 19-item pre-flight checklist green
- [ ] On-call coverage scheduled for 14-day cutover window

If any post-sprint exit fails: cutover slips; sprint output remains valid; remediation sub-sprint scoped to specific failures.

---

## Closing note

This V3 produces a deployment-readiness assessment in advance of V1 (cohesion) and V2 (traceability) outputs. **The GO-WITH-CAVEATS verdict is provisional on V1 and V2 alignment.** If V1 surfaces internal cohesion gaps (e.g. ADR-074 D9 cross-references something not in ADR-073 D2) or V2 surfaces dangling requirements (e.g. PRD-010 G5 not traced to any ADR), this V3 must be re-evaluated and the verdict possibly downgraded to NO-GO.

The single highest-leverage action between this V3 and sprint kickoff is item #4 in Pre-sprint must-do: vendoring paulmillr/nip44 vectors as `docs/specs/fixtures/nip44-v2.json`. That single 1-hour task converts C1 from "shipped bug we know about" to "regression-guarded forever". It defends every sprint deliverable that touches NIP-44 (ADR-076 absorption, F25 module deletion, F27 reference vectors) and is the operational definition of the lesson C1 taught us: _hand-roll + no vectors + no contract = drift_. If only one item from this V3 happens before sprint kickoff, it should be that one.
