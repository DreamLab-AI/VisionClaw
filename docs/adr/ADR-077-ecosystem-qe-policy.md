# ADR-077 — Ecosystem Quality Engineering Policy

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-010 G10 + F25–F30 implementation safety; PRD-011 G9 |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-078 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |
| Affected repos | All four — VisionClaw, dreamlab forum, agentbox, solid-pod-rs |

## Context

The QE fleet audit (`docs/integration-research/qe-fleet/Q1..Q5-*.md`, ~6,070 lines) found that the ecosystem has accumulated significant quality debt that **caused** the C1/C2/C3 critical drifts found in the original `05-crypto-gotchas.md`:

- **No paulmillr/NIP-44 reference vectors anywhere** (Q4 G2). C1 shipped because round-trips with the same buggy code passed.
- **No cross-substrate contract tests** (Q4 G3). Same NIP implemented 4 times with no equality assertion.
- **No cross-system smoke tests** (Q4 G4). `nostr-rs-relay` declared in `agentbox/flake.nix` but never `docker run` in any workflow.
- **No mutation testing** in any tree (Q4 G5).
- **VisionClaw has NO Rust CI workflow** (Q4 G1) — 1,353 inline tests with zero gating. Single largest one-step ROI fix in the ecosystem.
- **Bundle size budget (ADR-076 D5) has no CI gate**. ADR specifies +200 KiB per worker, +500 KiB forum-client; no enforcement.
- **C5 (NEW)**: NIP-42 AUTH challenge is generated via `js_sys::Math::random()` (`crates/relay-worker/src/relay_do/session.rs:284-292`) — not CSPRNG. Challenge unpredictability **is** the entire NIP-42 security property.
- **C4 (NEW)**: agentbox emits a **fourth** distinct `verificationMethod.type` (`SchnorrSecp256k1VerificationKey2025` at `s04-did.js:71`) — and is internally inconsistent (Python sovereign-bootstrap and JS S4 surface emit different DID Documents for the same agent).
- **NIP-98 quadruple duplication = 2,408 LOC** across forum + solid-pod-rs + VisionClaw + agentbox (Q1 §A2).
- **Forum WebAuthn 904 LOC hand-roll** while `passkey-types = "0.3"` is declared in workspace `Cargo.toml:35` but never imported (Q1 F2.10).
- **`KvReplayStore::seen_or_record` TOCTOU race** in all 4 CF Workers — CF KV is eventually consistent; no CAS (Q2 S-HIGH-3).
- **`solid-pod-rs-server` actix wrapper passes `body_hash = None`** to `nip98::verify`, dropping payload-tag verification entirely (Q2 S-HIGH-5).

The pattern is consistent: hand-rolled crypto/protocol code + no upstream reference vectors + no cross-substrate equality contracts = repeated drift. PRD-010 P0 + ADR-076 close the *first* class of drift (forum's nostr-core). This ADR establishes the **policy framework** that prevents the *whole class* of drift from recurring across the ecosystem.

## Decision

The DreamLab ecosystem adopts the following ten quality-engineering policies, applicable across all four repositories. Each policy is enforced via CI gates; failures block merge.

### P1 — Reference vectors are mandatory for every protocol primitive

Every implementation of a Nostr NIP, Solid spec, RFC, or W3C standard MUST be regression-guarded by **upstream-pinned reference test vectors**. Vectors live in `tests/fixtures/<spec-id>.json` per substrate, sourced from canonical repositories (paulmillr/nip44, nostr-protocol/nips, BIP-340 test vectors, RFC 8785 IETF test suite, w3c/did-test-suite, paulmillr/scure-bip32, etc.). UPSTREAM_PINS.md tracks commit hashes per source.

**CI gate**: `cargo test --test upstream_vectors -p <crate>` runs on every PR. Failures block merge.

**No new protocol-implementing code without reference vectors.** A PR adding a NIP implementation without vectors is rejected at review.

### P2 — Cross-substrate contract tests at three levels

Three levels of contract testing per Q5 T5:

- **Level 1 (within-substrate)**: every PR — unit + property + reference vector + library equality (e.g. `forum.compute_event_id(unsigned) == nostr_sdk.event_id(unsigned)`).
- **Level 2 (cross-substrate)**: nightly + per release — same input through ≥2 implementations, byte-equal output asserted. NIP-98 token built by forum, validated by VisionClaw + agentbox + solid-pod-rs. IS-Envelope encoded by one substrate, decoded by every other.
- **Level 3 (federation smoke)**: staging deploy — end-to-end DM round-trip across all 3 relay endpoints, NIP-26 delegation propagation, moderation cache invalidation.

**CI gates**: Level 1 = block-on-fail per PR; Level 2 = warn-and-page nightly; Level 3 = block-on-fail per staging release.

### P3 — Library convergence: established libraries first, hand-roll never

Per ADR-076 (forum nostr-core absorption) and ADR-078 (cross-substrate library convergence), every protocol primitive uses the established community library where one exists:

| Primitive | Established library |
|-----------|---------------------|
| Nostr NIPs (Rust) | `nostr` crate (rust-nostr.org) |
| Nostr NIPs (JS) | `nostr-tools` (paulmillr) |
| Nostr NIPs (Python) | `pynostr` (where appropriate) |
| Bech32 (Rust) | `bech32` crate |
| Bech32 (Python) | `bech32` PyPI package |
| WebAuthn (Rust) | `webauthn-rs = "0.5"` + `passkey-types = "0.3"` |
| HTTP Signatures (Rust) | `http-signature-rs` |
| RFC 8785 JCS (JS) | `canonicalize` (npm) |
| RFC 8785 JCS (Rust) | `serde_jcs` |
| Solid LDP / WAC / WebID | `solid-pod-rs` (DreamLab-owned, federated upstream) |
| Schnorr secp256k1 BIP-340 | `secp256k1` or `k256::schnorr` (RustCrypto) |
| ChaCha20-Poly1305 | `chacha20poly1305` (RustCrypto) |
| HKDF / HMAC / SHA-256 | RustCrypto org crates |

Hand-rolled protocol code requires explicit ADR citation justifying why no upstream library suffices. Bare assertion ("CF Workers compatibility") is insufficient — must include validation spike outcome.

**CI gate**: anti-drift lint rejects new modules under `crates/*/src/` whose name matches `nip\d+|bech32|did_doc|webauthn|http_sig|jcs` unless they are thin shims (≤100 LOC, ≥80% direct delegation to upstream).

### P4 — Mutation testing with kill-rate gates

Every protocol-implementing module is mutation-tested. Tools:
- Rust: `cargo-mutants`
- JS: `stryker`
- Python: `mutmut`

Kill-rate targets:
- Forum nostr-core (post-ADR-076 shim) ≥ **90%**
- Forum pod-worker WAC enforcement ≥ **80%**
- VisionClaw substrate auth paths ≥ **75%**
- Agentbox NIP-98 verifier + RelayConsumer dedup ≥ **80%**
- solid-pod-rs auth/wac/security modules ≥ **85%** (foundational)

**CI gate**: full mutation on weekly cron (block-and-page); sampled mutation per PR (warn-only).

### P5 — Property and fuzz testing per DDD invariant

Every invariant in the DDD bounded-context map (`docs/ddd-mesh-federation-context.md`) gets a property test. F-Inv-01..07 (forum), A-Inv-01..09 (agentbox), V-Inv-01..07 (VisionClaw), S-Inv-01..04 (solid-pod-rs). Per Q5 T8.

Fuzz targets (per Q5 T10):
- Forum nostr-core envelope parser (cargo-fuzz)
- Agentbox `RelayConsumer._verifyEvent` fallback parser (jest-fuzz)
- VisionClaw URI parser `src/uri/parse.rs` (cargo-fuzz)
- IS-Envelope JCS canonicalisation **differential fuzzing** across Rust/JS/Python implementations

Runtime budget: 5min per PR (smoke), 4hr nightly (deep).

### P6 — Coverage thresholds by substrate

Per Q4 G15 baseline:
- Forum nostr-core (post-ADR-076): line ≥ **95%**, branch ≥ **90%** (small surface area)
- Forum CF Workers: line ≥ **80%**, branch ≥ **70%**
- VisionClaw substrate: line ≥ **75%**, branch ≥ **65%**
- Agentbox JS: line ≥ **80%**, branch ≥ **70%**
- solid-pod-rs core: line ≥ **85%**, branch ≥ **75%** (foundational)

**CI gate**: PR fails if coverage drops by >2% vs main without justification annotation.

### P7 — Bundle size and cold-start budgets (ADR-076 D5)

CF Workers (forum) hard limits:
- Per-worker bundle ≤ **1 MiB** (CF free tier ceiling).
- Per-PR delta ≤ **+200 KiB** unless ADR cites the reason.
- Cold-start latency p50 ≤ **+50 ms** vs main.

Forum-client WASM:
- Bundle delta ≤ **+500 KiB** per PR.
- LCP regression ≤ **+200 ms** vs main.

**CI gate**: bundle size measurement + comparison; block on >150% of budget, warn on >100%.

### P8 — Security CI gates

Every PR runs:
1. **Secret scanning** — `gitleaks` or `trufflehog`. Block on positive finding.
2. **Dep audit** — `cargo deny check advisories` (Rust) + `npm audit --omit dev` (JS) + `pip-audit` (Python). Block on HIGH/CRITICAL CVE in production deps.
3. **License audit** — `cargo deny check licenses`. Block on incompatible AGPL/GPL in non-AGPL projects.
4. **SAST** — language-appropriate static analyser (clippy `--all-targets` + `--deny warnings` for Rust; eslint security plugins for JS; bandit for Python).
5. **Anti-drift** — anti-drift lint enforcing canonical mint chokepoints (P3).
6. **DID Document type-string assertion** — emitter test verifies `verificationMethod[0].type == "SchnorrSecp256k1VerificationKey2019"` and `@context` includes `https://w3id.org/security/suites/secp256k1-2019/v1`.
7. **CSPRNG audit** — grep-based check rejects `Math.random()` / `js_sys::Math::random()` / `random.random()` in any auth/crypto/session module. Forces use of `crypto.getRandomValues()` / `getrandom::getrandom()` / `secrets.token_bytes()`.

### P9 — Test ownership and triage

Per Q5 T15:
- **Reference vectors** owned by the substrate that consumes the protocol primitive. Updates require synchronised PRs across consumers when upstream evolves (e.g. NIP-44 v3).
- **Cross-substrate contracts** owned by VisionClaw (the integration-substrate). PRs that change contract behaviour require sign-off from all affected substrate maintainers.
- **Federation smoke** owned by the staging-deploy operator. Failures page the on-call.
- **Diamond-problem triage** (when L2 contract test fails because two substrates disagree): re-run upstream conformance on both → compare with third reference impl → default to last-touched-by from `git blame` → escalate to ADR if architectural.

### P10 — Quality dashboard

Each substrate exposes `/health/qe` returning:

```jsonc
{
  "coverage":     { "line": 0.84, "branch": 0.71, "trend_7d": "+0.3%" },
  "mutation_kill_rate": { "current": 0.83, "target": 0.80 },
  "reference_vectors":  { "passing": 247, "total": 250, "stale": 3 },
  "anti_drift_lint":    { "violations": 0 },
  "bundle_size_budget": { "used_pct": 0.71, "trend_7d": "+1.2%" },
  "security_audit":     { "high_cves": 0, "med_cves": 2, "last_scan": "..." }
}
```

Aggregated dashboard at `https://qe.dreamlab-ai.com/ecosystem` (or local equivalent) joins all four substrate signals.

## Consequences

### Positive

- **Closes the C1-class bug pattern by design**: hand-roll + no vectors + no contract = drift; this policy makes hand-roll require ADR justification, mandates vectors, mandates contracts.
- **Quantifiable quality posture**: every substrate has measurable coverage / mutation / security gates rather than ad-hoc judgment.
- **Cross-substrate symmetry**: the same PR-quality bar applies to all four projects; contributors can move between codebases with consistent expectations.
- **Faster review cycles**: reviewers don't manually re-derive whether a PR has adequate testing — CI tells them.
- **Operator confidence**: `/health/qe` gives operators a real-time signal of mesh integrity.

### Negative

- **CI runtime cost**: full mutation testing on weekly cron + per-PR sampled mutation + L1 contract + reference vectors adds ~20-30 minutes per PR (mostly mutation if sampled). Acceptable for protocol-critical repos; tunable.
- **One-time setup cost**: ~75 engineer-days per Q4 G14 to land the full programme. Phased per PRD-010 §7 (P0-P5).
- **Friction for prototype work**: contributors who want to spike a new NIP face the "where are the vectors?" gate. Mitigation: explicit `experimental/` dirs that opt out of P1; reviewer-approved exceptions for time-bounded spikes.
- **Bundle size budgets constrain feature velocity** when dep bumps push past the +200 KiB/PR limit. Mitigation: explicit budget exemptions tied to ADR.

### Neutral

- **CI workflow files multiply**: each substrate gains 5-8 new workflow files. Acceptable; workflows are configuration, not code.

## Alternatives Considered

### Alt-A — Voluntary best-practice + reviewer enforcement

No CI gates; rely on reviewer culture.

*Rejected*: this is the status quo. C1, C2, C3 happened under it. Cultural enforcement is necessary but insufficient when the failure mode is subtle (HKDF-Expand vs Extract).

### Alt-B — Per-substrate independent QE policies

Each project sets its own thresholds.

*Rejected*: the failure mode is **cross-substrate drift**. Independent policies cannot close it. The mesh is one system; QE is one system.

### Alt-C — Outsource QE to a single dedicated team

Hire / contract a QE team that owns every substrate's gates.

*Rejected*: bottlenecks contributor velocity; QE that sits outside the implementing teams calcifies. The right model is contributor-owned tests with framework-enforced rigor.

### Alt-D — Coverage gates only, no mutation testing

Skip P4 mutation testing.

*Rejected*: coverage measures executed lines, not the oracle quality of the assertions. C1 had 100% line coverage on `nip44.rs:122-128` — every encryption test executed those lines and passed. Mutation kill-rate would have detected the bug (mutating `expand` to `extract` produces different output that no test catches).

### Alt-E — Reference vectors imported into each repo separately

Each substrate maintains its own copy of paulmillr/nip44 vectors.

*Rejected*: this is how drift starts. Single source of truth at `docs/specs/fixtures/` in VisionClaw monorepo, copied with CI hash check, ensures all substrates test against byte-identical vectors.

## Implementation notes

### Phasing into PRD-010

- **P0 gating prerequisites** (Phase 0): P1 reference vectors for NIP-04/19/26/44/59/98 + WebAuthn + bech32 + JCS. P3 anti-drift lint. P8 CSPRNG audit + DID Document assertion.
- **P1-2 (Phase 1-2)**: P2 Level 1 contract tests within each substrate. P6 coverage threshold ratchet from current to target over 4 sprints.
- **P3 (Phase 3)**: P2 Level 2 cross-substrate contract tests. P5 property tests for DDD invariants.
- **P4-5 (Phase 4-5)**: P2 Level 3 federation smoke. P4 mutation testing baseline + ratchet. P10 quality dashboard.

### CI infrastructure

Each substrate gains:
- `.github/workflows/qe-policy.yml` — runs P1-P9 gates on every PR.
- `.github/workflows/qe-mutation.yml` — runs P4 mutation on weekly cron.
- `.github/workflows/cross-substrate-contracts.yml` (only in VisionClaw, the integration repo) — runs P2 L2 nightly.
- `.github/workflows/federation-smoke.yml` (only in VisionClaw) — runs P2 L3 on staging deploy webhook.

### Tooling pins

```toml
# Rust ecosystem
cargo-mutants = "25.0"     # mutation testing
cargo-tarpaulin = "0.31"   # coverage
cargo-deny = "0.16"        # license + advisory audit
cargo-fuzz = "0.12"        # fuzz harness

# JS ecosystem
@stryker-mutator/core = "8.7"
vitest = "2.1"
jest = "29.x"
gitleaks = "8.x"

# Python ecosystem
mutmut = "3.x"
pip-audit = "2.x"
bandit = "1.x"
```

### Per-policy enforcement matrix

| Policy | Forum | Agentbox | VisionClaw | solid-pod-rs |
|--------|-------|----------|------------|--------------|
| P1 vectors | ✓ all NIPs + WAC | ✓ NIP-98 + JCS | ✓ NIP-* + URN grammar | ✓ NIP-* + WAC + DID |
| P2 L1 | ✓ | ✓ | ✓ | ✓ |
| P2 L2 | run by VisionClaw | run by VisionClaw | runs | run by VisionClaw |
| P2 L3 | participates | participates | runs | n/a |
| P3 anti-drift | ✓ | ✓ | ✓ | ✓ |
| P4 mutation | ≥90% nostr-core, ≥80% pod-worker | ≥80% NIP-98+RelayConsumer | ≥75% auth | ≥85% auth/wac |
| P5 property | F-Inv-01..07 | A-Inv-01..09 | V-Inv-01..07 | S-Inv-01..04 |
| P6 coverage | 95/90 + 80/70 | 80/70 | 75/65 | 85/75 |
| P7 bundle | ✓ CF Workers | ✓ pod-server WASM | n/a | n/a |
| P8 security | ✓ | ✓ | ✓ | ✓ |
| P9 ownership | forum maintainers | agentbox maintainers | VC maintainers | solid-pod-rs maintainers |
| P10 dashboard | exposes signal | exposes signal | hosts dashboard | exposes signal |

## References

- PRD-010 — DID:Nostr Mesh Federation, G10 + F25-F30 (absorption requirements that this QE policy gates)
- ADR-073 — Mesh topology
- ADR-074 — DID:Nostr canonicalisation
- ADR-075 — IS-Envelope
- ADR-076 — Forum nostr-core absorption
- ADR-078 — Cross-substrate library convergence (parallel ADR; broadens P3 to all libraries)
- `docs/integration-research/qe-fleet/Q1-crypto-protocol-audit.md` — 1,342 lines, 49 findings
- `docs/integration-research/qe-fleet/Q2-security-primitive-audit.md` — 1,487 lines, S1-S14 categories
- `docs/integration-research/qe-fleet/Q3-identity-custody-audit.md` — 1,044 lines, I1-I13 sections
- `docs/integration-research/qe-fleet/Q4-coverage-gap-audit.md` — 821 lines, 75 e-day programme
- `docs/integration-research/qe-fleet/Q5-test-fixture-design.md` — 1,378 lines, fixture taxonomy + L1/L2/L3 contracts
- `docs/integration-research/05-crypto-gotchas.md` — original C1-C3 findings
