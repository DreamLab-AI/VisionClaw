# V2 — Requirements Validation: PRD-010 + PRD-011 vs ADR-073..ADR-083

| Field | Value |
|-------|-------|
| Status | QE Validation, 2026-05-07 |
| Subjects | PRD-010 (mesh federation), PRD-011 (forum kit extraction) |
| ADRs verified | ADR-073, 074, 075, 076, 077, 078, 079, 080, 081, 082, 083 |
| Method | INVEST + SMART + traceability + BDD + dependency-graph audit |

---

## Aggregate Verdict — **GO-WITH-CAVEATS**

The requirement set is testable and largely traceable. ADR coverage is real, not paper. Phasing and dependency graph are coherent with one circular hazard between ADR-082 fixture-sharing and PRD-010 P0 gating.

Five caveats to clear before P0 implementation:

1. **PRD-010 F12 has no explicit per-substrate ownership** — manifest schema specified, but the patch-site list omits VisionClaw `Settings.toml`/env shape (it shows up as "VisionClaw's `Settings.toml`/env exposes a `[mesh]` config section" in ADR-073 D6 prose with no file path). Risk: ambiguous test surface.
2. **PRD-011 G6 ("Sprint v9-v11 work captured") has no objective acceptance test.** No diff-coverage gate, no LOC-counted carry-over assertion. Reviewer is the only verifier.
3. **PRD-011 F8 (DreamLab consumer package) is gated on a kit GA that has no objective release criterion.** "Phase X6 — v3.0.0 GA (~0.5 sprints)" lists no exit conditions; ADR-083 D11 supplies a checklist but PRD-011 §7 does not cite it.
4. **ADR-082 D4 / sync-fixtures.sh is referenced by ADR-083 D13 as a precondition but ADR-082 D4 itself depends on substrates already having `tests/fixtures/` directories** — chicken-and-egg if treated as a bootstrapping step. Sequencing comment present but not enforced.
5. **Goal G6 in PRD-010 ("Discovery without prior configuration") and G2 (M2 in §11) measure 95% success rate** — but the test population is "100 random hex pubkeys from each substrate's roster", which is undefined for substrates that haven't yet onboarded any actors. Bootstrap measurement gap.

Recommended actions before approving cut-merge of P0:

- Add `[mesh]` schema documentation to PRD-010 §6 / ADR-073 with all three substrate file locations cited (VisionClaw `Settings.toml`, agentbox `agentbox.toml`, forum `wrangler.toml` `[vars]` env).
- Convert PRD-011 G6 into a measurable: per-Sprint LOC-mapping table in PRD-011 §4 carried into kit's tests/regression suite.
- Lift ADR-083 D11 checklist into PRD-011 Phase X6 as the literal exit criteria.
- Add ADR-082 D4 bootstrapping step ordering: master fixtures are created first, substrates bootstrap their fixtures directories second.
- Define M2 test population concretely: ≥30 known pubkeys per substrate seeded at staging boot; supplement with synthetic pubkeys for cold-start measurement.

---

## Section 1 — Requirements Traceability Matrix

**Verdict: GO** — every F<N> in PRD-010 + PRD-011 maps to at least one ADR. 10 PARTIAL items lack a concrete test surface or ownership assignment; 2 ORPHAN-class items rely on ADR text that does not yet specify validation.

### PRD-010 F1–F30

| # | Requirement | Implementing ADR(s) | Test surface (per ADR-077) | Verdict |
|---|---|---|---|---|
| F1 | VisionClaw identity unification (`SERVER_NOSTR_PRIVKEY` + `VISIONCLAW_NOSTR_PRIVKEY`) | ADR-074 D6, ADR-081 D2 | L1 unit; `tests/identity_unification.rs` (PRD-010 §12 lists) | TRACED |
| F2 | DID Document publication (3 substrates) | ADR-074 D2, D5; ADR-082 D13 | L1 + L2 contract via `did-doc-conformance.json` | TRACED |
| F3 | Tier-3 service entries (`#solid-pod`, `#nostr-relay`, `#webid`, `#mesh`) | ADR-074 D2, D9 | `did-doc-conformance.json` D2 schema | TRACED |
| F4 | `verificationMethod.type` standardised | ADR-074 D1, D13; ADR-077 P8.6 | CI emitter assertion (P8.6) | TRACED |
| F5 | NIP-19 npub correctness in agentbox | ADR-074 §Implementation; ADR-078 A1; ADR-081 D3 | L1 vector test (BIP-340); migration smoke | TRACED |
| F6 | NIP-44 v2 conv-key correctness | ADR-076 D1 (delete `nip44.rs`); ADR-082 D2 (paulmillr vectors) | L1 paulmillr vectors; mutation P4 ≥90% | TRACED |
| F7 | Universal NIP-42 AUTH gate | ADR-073 D3; ADR-074 D8 | `mesh-federation.json` AUTH session shape | TRACED |
| F8 | NIP-26 delegation verifier wiring (3 sites) | ADR-074 D8; ADR-076 cross-ref | `nip26-delegation.json` per substrate L1 | TRACED |
| F9 | Bridge re-signing replacement | ADR-074 §Implementation forward(); PRD-010 §Implementation | property test in §12; no fixture file named | PARTIAL — fixture file not enumerated in ADR-082 D1 |
| F10 | `urn:visionclaw:context` references in envelopes | ADR-075 D8 | `is-envelope-v1.json` D13 | TRACED |
| F11 | Federated kind allowlist per relay | ADR-073 D2, D6, D9 | `mesh-federation.json` D14 fan-out test | TRACED |
| F12 | Mesh deployment switches `[mesh]` block | ADR-073 D6; ADR-080 D1-D5 | TOML validator (PRD-011 F3.3) | PARTIAL — VC config-file path not enumerated |
| F13 | Linked-Data S2/S4 surface coherence | ADR-075 D10 | LDN AS2 mapping conformance | TRACED |
| F14 | DID-via-relay resolution path | ADR-074 D5; ADR-078 S6 | L1 in solid-pod-rs 0.5; integration smoke | TRACED |
| F15 | VisionClaw resolver routes registered | ADR-074 D2 | unit + integration; routes listed in PRD-010 §12 | TRACED |
| F16 | RelayConsumer wired into management-api boot | ADR-073 D10 (sidecar); PRD-010 §6 patch | smoke test of full bridge boot | TRACED |
| F17 | Agentbox external relay reachability | ADR-073 D7, D10; ADR-080 D2 | `tests/topologies/federated/` smoke | TRACED |
| F18 | Operator pubkey auto-allowlisted on agentbox relay | ADR-073 D6; ADR-074 D1 | unit (flake.nix) + integration AUTH | TRACED |
| F19 | Pod-inbox payloads as LDN | ADR-075 D10 | `is-envelope-v1.json` AS2 mapping | TRACED |
| F20 | Replay store federation across forum workers | ADR-077 P8 (TOCTOU mention); ADR-078 S3 | TOCTOU race property test | PARTIAL — TOCTOU race fix needed; no explicit cross-worker test |
| F21 | Cross-system replay via canonical event id | ADR-073 D9 (LRU); ADR-075 D12 | `mesh-federation.json` LRU dedup vector | TRACED |
| F22 | Bead-relay coupling via subscription | ADR-073 D10 (VC MeshBridge) | integration test in PRD-010 §12 | TRACED |
| F23 | Anti-drift CI gates | ADR-077 P3, P8; ADR-074 D13; ADR-076 cleanup | per-repo CI workflows | TRACED |
| F24 | Substrate-emitted bead republishing | ADR-073 D2 (federated_pubkeys) | `mesh-federation.json` per-pubkey gate | TRACED |
| F25 | `nostr-core` upstream absorption | ADR-076 D1-D8 | per-PR vector test + WASM build | TRACED |
| F26 | WASM/CF Workers compatibility validation spike | ADR-076 D5 | spike outcome ≥ acceptance criteria | TRACED |
| F27 | Reference test vectors | ADR-076 D8; ADR-082 D1, D2 | per-substrate L1 | TRACED |
| F28 | Per-PR behaviour-preserving migration | ADR-076 D6, D9 | per-PR CI gate | TRACED |
| F29 | Cargo workspace alignment | ADR-076 D9; ADR-078 V2 | unit (Cargo.toml shape) | TRACED |
| F30 | Public surface stability | ADR-076 §Consequences neutral | API-diff CI tool (not specified) | PARTIAL — no fixture or tool named for stability check |

### PRD-010 G1–G10 (Goals)

| # | Goal | Test surface | Verdict |
|---|---|---|---|
| G1 | Single canonical did:nostr resolution path | M2 (95%); ADR-074 D2 | TRACED |
| G2 | Unified inter-system message envelope | M1; ADR-075 D15 | TRACED |
| G3 | Three relay topologies, one wire protocol | ADR-073 D1, D6 | TRACED |
| G4 | NIP-42 AUTH as universal write gate | ADR-073 D3 | TRACED |
| G5 | NIP-26 as universal trust pivot | M3 (100%); ADR-074 D8 | TRACED |
| G6 | Discovery without prior configuration | M2 (95%); ADR-074 D5, D9 | PARTIAL — bootstrap measurement undefined for empty rosters |
| G7 | Deployment options | ADR-073 D6; ADR-080 | TRACED |
| G8 | Cryptographic correctness before scale | C1-C3 fixed in P0; ADR-076 + ADR-082 D2 | TRACED |
| G9 | Established Nostr protocol library, not hand-roll | ADR-076 entire | TRACED |
| G10 | (PRD-010 §2 has 9 Gs explicitly; G10 is in PRD-011) | n/a | n/a |

### PRD-011 F1–F11

| # | Requirement | Implementing ADR(s) | Test surface | Verdict |
|---|---|---|---|---|
| F1 | Repository setup (clone, branch, import, tag) | ADR-083 §D1 phasing; PRD-011 §7 X0 | manual checklist; CI tag-creation | TRACED |
| F2 | De-branding pass (anti-drift) | ADR-077 P3 (anti-drift CI); ADR-080 D7 | grep-based CI + manual sweep | TRACED |
| F3 | TOML schema + validator | ADR-079 D5 (config builder); ADR-080 D1-D8 examples | unit + property test | TRACED |
| F4 | Mesh federation native | ADR-073 (whole); ADR-074 D9 | inherited from PRD-010 G3 | TRACED |
| F5 | Library convergence (kit applies ADR-076 + 078 from inception) | ADR-076 + ADR-078 D1-D5 | inherited L1 vector tests | TRACED |
| F6 | QE policy compliance | ADR-077 (whole); ADR-082 (fixtures) | per-policy gates | TRACED |
| F7 | Forum-setup skill | ADR-079 D1-D13 | property test (D9); provider contract test | TRACED |
| F8 | DreamLab consumer package | ADR-080 D7; ADR-083 D7 | manual T₂ review + parity tests | PARTIAL — kit GA criteria not lifted into PRD-011 §7 X6 |
| F9 | Documentation | (no explicit ADR) | reviewer-only | PARTIAL — no test surface for completeness |
| F10 | Release versioning | (no explicit ADR; PRD-011 only) | semver lint + crates.io publish smoke | PARTIAL — no formalised acceptance |
| F11 | Per-project ecosystem cross-referencing (CLAUDE.md) | ADR-078 §Implementation Doc alignment | reviewer-only | ORPHAN — no test surface |

### PRD-011 G1–G10

| # | Goal | Test surface | Verdict |
|---|---|---|---|
| G1 | Reusable forum substrate | ADR-080 D1; PRD-011 M4 | TRACED |
| G2 | Federation-native | ADR-073 default-standalone | TRACED |
| G3 | DreamLab as one consumer among many | ADR-080 D7; ADR-083 §Context | TRACED |
| G4 | TOML-driven configuration | ADR-080 D1; PRD-011 §5.2 schema | TRACED |
| G5 | AI-assisted configurator | ADR-079 (whole) | TRACED |
| G6 | Sprint v9-v11 work captured | (no carry-over diff fixture) | PARTIAL — reviewer-only |
| G7 | De-branding completeness | M1 (zero `dreamlab` substring); ADR-077 P3 | TRACED |
| G8 | Re-import path (`dreamlab-ai-website` consumer cutover) | ADR-083 (whole) | TRACED |
| G9 | Quality engineering parity | ADR-077; ADR-082 | TRACED |
| G10 | Library convergence | ADR-076 + 078 | TRACED |

**Section 1 verdict**: 41 of 51 requirements TRACED, 9 PARTIAL, 1 ORPHAN. No UNTESTABLE.

---

## Section 2 — Goal-to-Success-Metric Traceability

**Verdict: GO-WITH-CAVEATS** — every G<N> has a corresponding M<N> in §11/§10, but two metrics have undefined test populations and one threshold is non-load-bearing (M5 "≤1% user-visible regression rate" without baseline).

### PRD-010

| Goal | Metric | Threshold | Measurable? | Verifying surface |
|---|---|---|---|---|
| G1 (canonical resolution) | M2 | ≥95% success rate over 100 random pubkeys per substrate | YES | Phase 1 deliverable; integration test |
| G2 (unified envelope) | M1 | E2E DM round-trip ≤5s | YES | `tests/mesh_e2e.rs` Phase 4 |
| G3 (three topologies) | (no dedicated M; covered by M1) | implicit | PARTIAL | covered only by M1 |
| G4 (NIP-42 universal) | M1 + M5 (latency budget) | depends on M1 ≤5s | YES | Phase 4 |
| G5 (NIP-26 delegation) | M3 | 100% verify rate on emitted delegated events | YES | Phase 2 deliverable |
| G6 (discovery without config) | M2 | ≥95% (same as G1) | PARTIAL — measurement population undefined for cold-start substrates | Phase 1 |
| G7 (deployment options) | (no dedicated M) | implicit in Phase 3 | PARTIAL — deployment-mode coverage not measured | Phase 3 |
| G8 (cryptographic correctness) | M4 | anti-drift CI passing; zero open warnings | YES | per-PR CI |
| G9 (established library) | (no dedicated M; covered by ADR-076 D5 spike acceptance) | bundle ≤+200 KiB; cold-start ≤+50 ms | YES | spike at F26 |

### PRD-011

| Goal | Metric | Threshold | Measurable? | Verifying surface |
|---|---|---|---|---|
| G1 (reusable substrate) | M2 + M4 | TOML round-trip + skill <15min | YES | integration smoke + user testing |
| G2 (federation-native) | M3 | participates in 3-substrate mesh | YES | ADR-077 P2 L3 |
| G3 (DL as one consumer) | (no dedicated M; covered by M5 cutover safety) | zero data loss + ≤1% regression | PARTIAL — M5 ≤1% lacks baseline definition (regression-against-what?) | post-cutover audit |
| G4 (TOML-driven) | M2 + M4 | (same as G1) | YES | same |
| G5 (AI configurator) | M4 | <15 minutes for first-time operator | YES | user testing |
| G6 (Sprint v9-v11 captured) | (no dedicated M) | reviewer-only | NO | NONE |
| G7 (de-branding) | M1 | zero `dreamlab` substrings | YES | anti-drift CI |
| G8 (re-import) | M5 | zero data loss / zero session loss | YES | post-cutover audit |
| G9 (QE parity) | M6 | line ≥80%, branch ≥70%, mutation ≥80% | YES | ADR-077 P6 + P4 |
| G10 (library convergence) | (no dedicated M) | implicit via M6 | PARTIAL | via M6 |

**Caveats**:
- M5 "≤1% user-visible regression rate" needs baseline (regression vs what? old stack at T₀?). ADR-083 D6 partially addresses with "behaviour-divergent" classification but the 1% threshold is asserted without measurement plan in PRD-011.
- G6 (PRD-011) and G7 (PRD-010) both lack dedicated metrics. G6 is the more material gap; PRD-011 §10 should add an M for "all Sprint v9-v11 NIPs/migrations/cohort fixes verified by carry-over fixture suite".

---

## Section 3 — Cutover Acceptance Criteria Audit (ADR-083)

**Verdict: GO-WITH-CAVEATS** — D11 pre-flight checklist is concrete and machine-checkable; D9 rollback triggers are partially measurable; D5 session continuity has a clear test surface but is gated on storage-sharing (D3) which has hidden risk.

### D11 Pre-flight checklist (15 items)

| Check | Concrete? | Measurable? | Owner | Notes |
|---|---|---|---|---|
| Forum kit v3.0.0 GA tagged | YES (git tag) | YES | kit maintainer | depends on PRD-011 §7 X6 exit criteria, which is missing — see §1 caveat |
| Cross-substrate fixture sync verified | YES | YES | VisionClaw integration | per-fixture CHECKSUM.txt |
| L2 contract tests passing for ≥7 nights | YES | YES | VisionClaw nightly | clear stop criterion |
| Federation smoke tests passing for ≥7 nights | YES | YES | VisionClaw nightly | ADR-077 P2 L3 |
| forum-config/ package built + tested | YES | YES | DreamLab ops | smoke test specified D7 |
| dreamlab.toml manually reviewed | YES | NO (manual) | DL admin team | sign-off captured? not in checklist |
| Schema parity confirmed | YES | YES | DL ops | D4 invariant tests |
| Session continuity tested in staging | YES | YES (≥10 users) | DL ops | sample size cited but coverage criteria not |
| Rollback drill completed | YES | YES (per-path elapsed) | DL ops | drill at T₂ |
| On-call coverage scheduled | YES | YES | DL operations | calendar artefact |
| PagerDuty alerts configured | YES | YES | DL DevOps | enumerated alerts in D9 |
| parity-check.sh CI passes | YES | YES | DL ops + nightly | endpoints enumerated |
| router-worker deployed | YES | YES | DL ops | deployment artefact |
| OldRouter URL preserved | YES | YES | DL ops | DNS check |
| NewRouter URL preserved | YES | YES | DL ops | DNS check |
| Stakeholder briefing complete | YES | NO (acknowledgement) | DL leadership | weak — no sign-off captured |

**Caveats**:
- "manually reviewed" + "stakeholder briefing complete" lack documented sign-off artefact. Suggest each checklist item produces a stamped record in `docs/operations/cutover-log.md` D12.

### D9 Rollback triggers (7 scenarios)

| Trigger | Detection method | Threshold | Measurable? |
|---|---|---|---|
| 5xx spike | Prometheus `errors_total{stack="new"}` | >1% | YES |
| Session loss >0.1% | `session_drop_total{stack="new"}` | >0.1% | YES |
| Schema corruption | manual + data-integrity tests | "suspected" | NO — operator judgement |
| WebAuthn breakage | `passkey_login_failure_rate` alert | (no threshold given) | PARTIAL |
| Pod ACL drift | nightly ACL parity scanner | (no threshold given) | PARTIAL |
| Subtle behaviour regression | nightly parity-check.sh | >1% rate | YES |
| Catastrophic data loss | "irrecoverable" — D3 dual-deploy mitigates | n/a | n/a (mitigation, not trigger) |

**Caveat**: WebAuthn and pod ACL drift triggers lack concrete thresholds. Suggest baseline values for first deployment, ratchet over time.

### D5 Session continuity (testable)

ADR-083 D5 makes **four crisp claims**:
1. SESSIONS KV identical between stacks → testable by writing `pubkey1 → session1` to KV via old worker, reading via new worker.
2. WebAuthn rp_id identical (`dreamlab-ai.com`) → unit test against forum-config/dreamlab.toml + community-forum-rs config.
3. PRF derivation info string identical (`"nostr-secp256k1-v1"`) → grep + assertion across both codebases.
4. Cookie / Authorization header parsing identical → contract test feeding sample cookies + headers through both routers, asserting decoded session matches.

All four are testable now without infrastructure investment. Verdict: **TRACED**.

---

## Section 4 — BDD Scenarios for 5 Highest-Risk Requirements

### Scenario A — F6 NIP-44 v2 conversation key correctness (PRD-010)

```gherkin
Feature: NIP-44 v2 conversation key derivation matches paulmillr reference vectors
  C1-class CRITICAL: prevents forum DM interop with reference Nostr clients

  Background:
    Given the kit's nostr-core post-ADR-076 absorption (nip44.rs DELETED)
    And the upstream `nostr` crate at version 0.44 with feature "nip44" enabled
    And paulmillr/nip44 reference vectors loaded from tests/fixtures/nip44-v2.json

  Scenario: Round-trip with a known good vector
    Given a vector "nip44_v2_basic" with sk, pk, plaintext, expected_payload, expected_conv_key
    When the upstream `nip44::ConversationKey::derive(sk, pk)` is computed
    Then the result MUST byte-equal expected_conv_key
    And `nip44::v2::encrypt(conv_key, plaintext)` MUST produce ciphertext equal to expected_payload (modulo nonce)
    And `nip44::v2::decrypt(conv_key, expected_payload)` MUST equal plaintext

  Scenario: All 40 paulmillr vectors pass
    Given all paulmillr/nip44 reference vectors
    When each is fed through nostr_core::nip44::round_trip()
    Then 100% MUST pass
    And 0% MUST exhibit HKDF-Expand vs Extract drift (the C1 signature)

  Scenario: Cross-substrate L2 contract validates
    Given a kind-1059 wrap built by forum nostr-bbs-core
    When agentbox `RelayConsumer._processEvent` decrypts via nostr-tools
    Then plaintext MUST byte-equal forum's input

  Scenario: Mutation-testing harness rejects hand-roll regression
    Given a synthetic mutation that replaces `Hkdf::new().extract()` with `Hkdf::new().expand(&[])`
    When `cargo mutants` runs against the post-absorption shim
    Then mutation kill rate MUST be ≥90% (per ADR-077 P4)
    And the specific HKDF-Expand-vs-Extract mutation MUST be killed
```

### Scenario B — F25 nostr-core absorption (PRD-010, ADR-076)

```gherkin
Feature: Forum nostr-core absorbed into upstream `nostr` crate as thin shim
  Drives PRD-010 G9 + ADR-076 D1-D8

  Background:
    Given the F26 WASM/CF Workers compatibility spike has succeeded
    And bundle delta ≤+200 KiB per worker
    And cold-start delta ≤+50 ms

  Scenario: Module deletion preserves public API surface
    Given the previous `crates/nostr-core/src/{nip04,nip19,nip26,nip44,nip90}.rs` modules deleted
    When `cargo build -p nostr-core --target wasm32-unknown-unknown` runs
    Then the build MUST succeed
    And forum-client + 5 CF Workers MUST also build via that nostr-core
    And `pub use nostr::Event as NostrEvent` exists in lib.rs (F30)

  Scenario: Per-PR migration order
    Given migration follows ADR-076 D6 ordering (nip04 → nip19 → nip44 → ...)
    When PR #N lands a single module deletion
    Then existing unit tests MUST pass
    And new upstream-vector tests for that module MUST pass
    And `forum-client` integration tests MUST show no behaviour delta
    And the next PR cannot land until the prior is merged

  Scenario: Final cleanup drops direct deps
    Given all D6-listed modules have been deleted/refactored
    When the final cleanup PR drops chacha20poly1305/hmac/aes/cbc/bech32/k256
    Then `Cargo.lock` MUST shrink (verifiable by line-count delta)
    And `cargo deny check licenses` MUST pass (per ADR-077 P8.3)

  Scenario: Project-specific shim retains canonical kinds
    Given the post-absorption shim crate
    When a downstream consumer imports `nostr_bbs_core::kinds::KIND_BAN`
    Then KIND_BAN MUST equal 30910 (per ADR-076 D7)
    And builders for moderation events MUST delegate to `nostr::EventBuilder`

  Scenario: Spike failure rolls back
    Given the F26 spike fails on bundle size or cold-start budget
    When the spike outcome is recorded
    Then PRD-010 P0 MUST revert to Shape C (in-place patch nip44.rs:122-128)
    And ADR-076 status MUST move to Rejected
    And no per-module deletion PR MUST be merged
```

### Scenario C — PRD-011 F1 clean repo setup

```gherkin
Feature: Forum kit extraction creates clean canonical home at nostr-rust-forum
  Drives PRD-011 G1, G7, F1.1-F1.5

  Background:
    Given a local clone of DreamLab-AI/nostr-rust-forum at /home/devuser/workspace/nostr-rust-forum/
    And a working tree on branch `import/v3-from-dreamlab-ai-website`

  Scenario: De-branded import commit
    Given the de-branding extraction script is run over community-forum-rs/
    When the import commit is applied
    Then `git grep -i dreamlab` MUST return zero hits across `crates/`, `docs/`, `examples/`
    And README.md MUST contain "nostr-bbs-rs" (the public product name)
    And README.md MUST NOT contain "VisionFlow" (DreamLab internal brand) anywhere public

  Scenario: PR merged + v3.0-rc1 tag
    Given the import commit is reviewed and merged
    When `git tag v3.0-rc1` is applied
    Then the tag points at the merge commit
    And the tag is pushed to origin

  Scenario: Cohort/admin/zone defaults match kit shape (G7-allowed)
    Given the post-import workspace
    When `nostr-bbs-config` defaults are loaded
    Then default zone names MUST be "public", "members", "private" (kit-default)
    And admin mode default MUST be "first-user" or operator-explicit "static"
    And no "lobby"/"trusted" cohort name appears as a default

  Scenario: Anti-drift CI rejects regression
    Given a PR that adds `dreamlab` substring to crates/nostr-bbs-core/src/lib.rs
    When CI runs on the PR
    Then the anti-drift lint MUST fail
    And the PR MUST be blocked from merge
```

### Scenario D — ADR-083 D5 session continuity

```gherkin
Feature: User sessions survive cutover without re-login
  Drives ADR-083 M5 (zero session loss); critical for PRD-011 R5 mitigation

  Background:
    Given old stack at https://old-router.dreamlab-ai.com running community-forum-rs/
    And new stack at https://new-router.dreamlab-ai.com running forum-config/ + nostr-bbs-* crates
    And both stacks share KV namespace SESSIONS, KV NIP98_REPLAY, R2 dreamlab-pods, D1 dreamlab-relay
    And WebAuthn rp_id is "dreamlab-ai.com" on both
    And PRF info string is "nostr-secp256k1-v1" on both

  Scenario: Identical session token validates on both stacks
    Given user U with passkey on dreamlab-ai.com signs in via old-router
    And session token T is issued and stored in SESSIONS KV
    When U makes a request to new-router with token T
    Then new-router MUST recognise the session
    And U MUST NOT be prompted to re-authenticate
    And the response MUST include `X-Forum-Stack: new`

  Scenario: PRF derivation is byte-identical across stacks
    Given user U with WebAuthn passkey + PRF salt S
    When U signs in via old-router and PRF derives nsec_old
    And U signs in via new-router and PRF derives nsec_new
    Then `hex(nsec_old) == hex(nsec_new)` MUST hold
    And the corresponding `did:nostr:<hex>` MUST be byte-equal

  Scenario: NIP-98 token built by old worker is rejected (replay) by new worker
    Given a NIP-98 token bound to URL+method M issued by old-worker, recorded in NIP98_REPLAY
    When the same token is presented to new-worker for M
    Then new-worker MUST reject with replay-detected error
    And NIP98_REPLAY shared-namespace MUST contain the token id

  Scenario: Mid-thread post on old stack is visible on new stack
    Given U posts kind-1 K on old-router
    When U refreshes thread on new-router (consistent-hash cohort flips)
    Then K MUST be visible at the same thread position
    And mod state on K MUST match between stacks

  Scenario: 10-user staging session-continuity test
    Given 10 test users with seeded passkeys
    When each user is rotated old→new and new→old via ROUTING_MODE pubkey-cohort flips
    Then 0 of 10 MUST require re-login
    And 0 of 10 MUST experience session corruption
```

### Scenario E — ADR-081 D6 federation key rotation

```gherkin
Feature: Federation key rotation preserves mesh continuity
  Drives PRD-010 R7, ADR-081 D6 protocol

  Background:
    Given a federated mesh with kit + agentbox + VisionClaw
    And kit operator holds federation_key K_old in tier-2 (CF Workers Secret)
    And cadence dictates rotation due in <=24 hours

  Scenario: Standard rotation with overlap window
    Given operator runs `nostr-bbs-admin rotate federation`
    When new key K_new is generated and stored in CF Workers Secret v2
    And NIP-26 delegation δ_old→new is signed by K_old (kind=22242, created_at<now+24h)
    And kind-30033 mesh service-list is published advertising K_new under #nostr-relay service
    And kind-30050 mesh_ping with body.kind="rotation_announcement" is federated
    Then peers MUST observe both K_old and K_new as valid for the overlap window
    And `mesh_peer_failover_total{from=old,to=new}` MUST NOT increment (no failover; planned rotation)
    And health endpoint /health/keys MUST report transition_active: true for federation role

  Scenario: Receiver-cache TTL respected
    Given peer cache TTL is 600s for kind-30033
    When K_new appears in mesh service-list
    Then peers MUST refresh within 600s
    And events signed by K_new MUST be accepted within 600s of publish
    And events signed by K_old MUST continue to be accepted for the configured 24h overlap

  Scenario: Decommission after overlap
    Given the 24h overlap has elapsed
    When operator runs `nostr-bbs-admin rotate federation --decommission-old`
    Then a kind-30033 with K_old removed from #nostr-relay service is published
    And K_old MUST be wiped from the cloud secret store (Tier-2 expiry)
    And events signed by K_old MUST start being rejected at peer ingest

  Scenario: Emergency revocation collapses overlap
    Given operator suspects K_old compromise
    When `nostr-bbs-admin emergency-revoke federation` is run
    Then a kind-30033 with K_old REMOVED and K_new ADVERTISED is published immediately
    And kind-30050 with body.kind="revocation" is federated
    And peers' replay-store gates begin rejecting K_old events at ingest
    And no overlap window applies

  Scenario: Anti-collision check refuses role conflict
    Given a deployment misconfigures federation_key == operator_key
    When `nostr-bbs-admin custody verify` runs
    Then the verifier MUST report a HIGH severity collision finding
    And the deployment MUST refuse to boot until corrected (per D8)

  Scenario: Cross-substrate contract test verifies rotation
    Given kit federation key rotated
    When agentbox + VisionClaw ingest events
    Then both substrates MUST observe the kind-30033 update
    And both substrates MUST accept new federation events from K_new within overlap window
```

---

## Section 5 — Open Questions Audit

**Verdict: GO-WITH-CAVEATS** — most open questions are resolved by subsequent ADRs; 4 remain explicitly deferred (acceptable); 0 are silently dropped.

### PRD-010 §10 Q1–Q8

| Q | Status | Resolved by | Notes |
|---|---|---|---|
| Q1 — Mesh routing protocol pull vs push | RESOLVED (ADR-073 D2) | push fan-out chosen; deferred-from-PRD note intact | TRACED |
| Q2 — Per-relay vs mesh-wide moderation | RESOLVED (ADR-073 D6 `honor_remote_moderation`) | opt-in per relay; recommendation aligned | TRACED |
| Q3 — `#mesh` service URL list refresh | RESOLVED (ADR-074 D9 + D5) | hybrid kind-30033 + TTL cache fallback | TRACED |
| Q4 — Multi-agent identity per agentbox container | DEFERRED (PRD-011 §10 Q follow-up; ADR-074 D6 explicitly punts to P5) | acceptable deferral | DEFERRED |
| Q5 — Relay discovery for unknown actors | RESOLVED (ADR-074 D5 + ADR-080 D10) | parallel-race + cache | TRACED |
| Q6 — solid-pod-rs upgrade timing | DEFERRED (ADR-078 D2 B5/S1; PRD-010 §6 R5) | gating B2 absorption; acceptable | DEFERRED |
| Q7 — BC20 anti-corruption layer in this PRD? | RESOLVED (ADR-075 D8 + PRD-006 §5.5) | parallel sprint to PRD-010 P3 | TRACED |
| Q8 — `.well-known/mesh.json` for human discovery | DEFERRED (ADR-073 §References; recommendation made) | documented deferral | DEFERRED |

### PRD-011 §9 Q1–Q8

| Q | Status | Resolved by | Notes |
|---|---|---|---|
| Q1 — License: MIT vs Apache-2.0 vs MIT/Apache dual | DEFERRED (PRD-011 §9 to kit ADR-001) | Phase X1 | DEFERRED |
| Q2 — Crates.io organisation | RESOLVED (PRD-011 §9 recommendation) | TRACED |
| Q3 — Skill discovery | RESOLVED (ADR-079 D10 multi-channel) | TRACED |
| Q4 — `community-forum-rs` retirement | RESOLVED (ADR-083 D12) | one-month observation | TRACED |
| Q5 — Backwards-compatible TOML schema migrations | RESOLVED (PRD-011 §9 + `nostr-bbs-config migrate` CLI committed to) | TRACED |
| Q6 — Per-deployment localisation | DEFERRED (out-of-scope for v3.0; v3.1+ feature) | DEFERRED |
| Q7 — Federation key custody | RESOLVED (ADR-081 D1-D3) | TRACED |
| Q8 — Cross-deployment user identity | RESOLVED (PRD-011 §9 + docs/deployment/identity-portability.md committed to) | TRACED |

**Section 5 verdict**: 11 of 16 questions RESOLVED; 5 DEFERRED (none silently). No blocking questions remain.

---

## Section 6 — Phase Dependency Graph Validity

**Verdict: GO-WITH-CAVEATS** — phases align across PRD-010 P0-P5, PRD-011 X0-X6, and ADR-077 P0-P5. One circular hazard between ADR-082 D1 (master fixtures created) and PRD-010 P0 (which consumes those fixtures). Critical-path estimate is plausible.

### Cross-PRD/ADR phase mapping

| Phase | PRD-010 P# | PRD-011 X# | ADR-077 P# | ADR-080 phase | ADR-082 phase | ADR-083 timeframe |
|---|---|---|---|---|---|---|
| Crypto correctness gating | P0 (~2 sprints) | n/a (kit inherits from inception) | P0: vectors + anti-drift + CSPRNG | n/a | Phase 0 deliverable | gating dependency D13 |
| Identity / DID-Doc | P1 (~1 sprint) | X1 (~1 sprint) workspace restructure | P1-2 coverage ratchet | D6 custody | Phase 1 substrate fixtures | D11 pre-flight items |
| AUTH + delegation | P2 (~1 sprint) | X2 (~1.5 sprints) library convergence | P3 cross-substrate L2 | n/a | Phase 2 L2 cross-substrate | n/a |
| Bridge wiring + fan-out | P3 (~1.5 sprints) | X3 (~1 sprint) QE compliance | P3 | n/a | Phase 3 property tests | n/a |
| Envelope contract | P4 (~1 sprint) | X4 (~1 sprint) skill | P4-5 mutation | D9 migration | n/a | T₁ staging deploy |
| Consolidation | P5 (~0.5 sprint) | X5 + X6 cutover + GA (~1.5 sprints) | P4-5 | D9 migration | n/a | T₃-T₈ |

### Circular dependency hazard

- **ADR-082 Phase 0 deliverable**: "Vendor paulmillr/nip44 vectors as nip44-v2.json" must be done in VisionClaw monorepo.
- **PRD-010 P0 Phase 0**: "F27 — Reference test vectors landed alongside the modules they validate" — required before each absorption PR.
- **ADR-083 D13**: "Cannot proceed [cutover] until: ADR-082 fixture-sharing protocol active in CI (≥7 days clean)" — but this assumes fixtures already authored.
- **Risk**: if ADR-082 D1 is in flight while PRD-010 P0 absorption PRs are landing, fixture file checksums will churn; substrate CHECKSUM.txt updates need synchronised PR handling that is documented in ADR-082 D10 but not enforced by CI.

**Mitigation**: ADR-082 Phase 0 deliverable list is a hard prerequisite for PRD-010 P0; sequencing should be: (1) author VisionClaw fixtures + UPSTREAM_PINS.md; (2) bootstrap substrate `tests/fixtures/` directories via sync-fixtures.sh (Phase 1 deliverable); (3) THEN start absorption PRs.

### Critical-path estimate validity

- PRD-010: 6 sprints (1 engineer) / 4 sprints (2 engineers parallel). Plausible given F26 spike at P0 gates the rest, and ADR-076 D6 is module-by-module.
- PRD-011: 5-6 sprints (1 engineer) / 3-4 sprints (2 engineers). Plausible — X0 + X1 are setup-heavy, X2-X4 can run parallel, X5 (cutover) is sequential.
- ADR-083: 14 days T₃ → T₆ + 7 days observation + 7 days audit = 28 days. Plausible.

Combined timeline: PRD-010 P0 ≥ ADR-076 absorption ≥ ADR-082 fixtures ≥ ADR-083 cutover. Conservative total: 8-10 sprints. Aggressive (parallel substrate engineers): 4-5 sprints.

---

## Section 7 — Done-Definition Completeness

**Verdict: GO-WITH-CAVEATS** — PRD-010 P0 has objectively verifiable completion (vectors + spike acceptance + per-PR CI). PRD-011 X0 has reviewer-only completion ("import commit replaces crates/ content with de-branded extraction").

### PRD-010 P0 Exit Criteria

PRD-010 §7 Phase 0:

```
- F26 — WASM/CF Workers compat spike passes acceptance criteria
- F4 — verificationMethod.type standardised on SchnorrSecp256k1VerificationKey2019 (3 patch sites)
- F5 — agentbox NIP-19 npub fix
- F25 — Per-module nostr-core absorption PRs land
- F27 — Reference test vectors land alongside each module
- F28 — Per-PR CI gating in place from PR 1
- F29 — VisionClaw workspace nostr pin aligned
- F30 — Public surface stability verified at each PR
- §15.H11 — Cross-language HKDF info string match
```

**Cited?** YES — PRD-010 §7 + ADR-076 §D5/D6/D8 enumerates each exit. **Objectively verifiable?** YES — every item has a CI test or unit test (F26 spike acceptance criteria are quantitative).

### PRD-011 X0 Exit Criteria

PRD-011 §7 Phase X0:

```
- Clone nostr-rust-forum locally
- Create import/v3-from-dreamlab-ai-website branch
- Run de-branding extraction script over community-forum-rs/
- Single import commit + PR + merge + v3.0-rc1 tag
```

**Cited?** PARTIAL — script exists but no acceptance test specified.
**Objectively verifiable?** PARTIAL:
- Branch exists / tag exists → mechanical check.
- "De-branding extraction script" → script must exist; script's correctness is ad hoc unless ADR-077 P3 anti-drift CI is active.

**Caveat**: ADR-077 P3 anti-drift lint should ALREADY be enabled before X0 lands its import commit, otherwise X0 cannot self-verify. Lift this dependency into PRD-011 §7 X0 explicitly:
- "X0 prerequisite: ADR-077 P3 anti-drift CI active in nostr-rust-forum repo."

Beyond that, "Single import commit replaces crates/ content with de-branded extraction" needs an explicit "MUST pass anti-drift CI" assertion to be objectively verifiable.

### Objectively verifiable subset (per phase)

| Phase | Objectively verifiable? | Cited? | Verdict |
|---|---|---|---|
| PRD-010 P0 | YES | YES | DONE-DEF OK |
| PRD-010 P1 | YES (handlers + identity unification = code lands; F15 routes registered = unit test) | YES | DONE-DEF OK |
| PRD-010 P2 | YES (AUTH + delegation = unit + integration tests) | YES | DONE-DEF OK |
| PRD-010 P3 | YES (RelayConsumer wired = boot smoke test; mesh-bridge generalised = integration) | YES | DONE-DEF OK |
| PRD-010 P4 | YES (M1 = E2E DM 5s) | YES | DONE-DEF OK |
| PRD-010 P5 | YES (anti-drift active + bead republishing) | YES | DONE-DEF OK |
| PRD-011 X0 | PARTIAL (no anti-drift gate cited) | PARTIAL | DONE-DEF GAP |
| PRD-011 X1 | YES (workspace restructure → cargo build) | YES | DONE-DEF OK |
| PRD-011 X2 | YES (library convergence per ADR-076 D6) | YES | DONE-DEF OK |
| PRD-011 X3 | YES (QE policy compliance) | YES | DONE-DEF OK |
| PRD-011 X4 | PARTIAL (skill conversation flow → property test, but kit GA criteria not lifted) | PARTIAL | DONE-DEF GAP |
| PRD-011 X5 | YES (cutover ADR-083 D11 checklist) | YES (reference) | DONE-DEF OK |
| PRD-011 X6 | NO (no exit criteria; "ship to crates.io" is implicit) | NO | DONE-DEF MAJOR GAP |

**Section 7 verdict**: 11 of 13 phases have done-definition. PRD-011 X0 + X4 + X6 have gaps; X6 most critical because ADR-083 D11 explicitly depends on "Forum kit v3.0.0 GA tagged" without naming what makes it GA-eligible.

---

## Aggregate Summary

| Section | Verdict | Critical issues |
|---|---|---|
| 1 — Traceability | GO | 9 PARTIAL (test surface gaps), 1 ORPHAN (F11 docs) |
| 2 — Goal-to-Metric | GO-WITH-CAVEATS | M5 baseline; G6/G7 missing dedicated metrics |
| 3 — Cutover criteria | GO-WITH-CAVEATS | sign-off artefacts; 2 thresholds undefined |
| 4 — BDD scenarios | GO | 5 scenarios completed |
| 5 — Open questions | GO-WITH-CAVEATS | 5 DEFERRED items (all explicit) |
| 6 — Phase graph | GO-WITH-CAVEATS | sequencing hazard between ADR-082 D1 and PRD-010 P0 |
| 7 — Done-definition | GO-WITH-CAVEATS | PRD-011 X0 + X4 + X6 gaps |

**Final verdict: GO-WITH-CAVEATS.**

Six recommended actions (all bounded; ≤1 sprint of editorial work):

1. Add `[mesh]` schema documentation locations to PRD-010 §6 (VC `Settings.toml`, agentbox `agentbox.toml`, forum `wrangler.toml`).
2. Convert PRD-011 G6 to a measurable Sprint v9-v11 carry-over fixture suite.
3. Lift ADR-083 D11 checklist into PRD-011 §7 X6 as literal exit criteria (close DONE-DEF major gap).
4. Add ADR-082 D1 → D4 sequencing as enforced ordering in implementation notes.
5. Define M2 test population concretely (≥30 known + synthetic for cold-start).
6. Pin baseline thresholds for ADR-083 D9 WebAuthn breakage and pod-ACL drift triggers.

After these actions, the requirement set reaches GO. Implementation can begin on PRD-010 P0 once F26 spike outcomes are recorded.

---

## References

- PRD-010 — `/home/devuser/workspace/project/docs/PRD-010-did-nostr-mesh-federation.md`
- PRD-011 — `/home/devuser/workspace/project/docs/PRD-011-visionflow-forum-kit-extraction.md`
- ADR-073 — `/home/devuser/workspace/project/docs/adr/ADR-073-private-nostr-relay-mesh-topology.md`
- ADR-074 — `/home/devuser/workspace/project/docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md`
- ADR-075 — `/home/devuser/workspace/project/docs/adr/ADR-075-is-envelope-message-contract.md`
- ADR-076 — `/home/devuser/workspace/project/docs/adr/ADR-076-nostr-core-absorption-into-upstream.md`
- ADR-077 — `/home/devuser/workspace/project/docs/adr/ADR-077-ecosystem-qe-policy.md`
- ADR-078 — `/home/devuser/workspace/project/docs/adr/ADR-078-cross-substrate-library-convergence.md`
- ADR-079 — `/home/devuser/workspace/project/docs/adr/ADR-079-forum-setup-skill-provider-abstraction.md`
- ADR-080 — `/home/devuser/workspace/project/docs/adr/ADR-080-forum-kit-deployment-topology-patterns.md`
- ADR-081 — `/home/devuser/workspace/project/docs/adr/ADR-081-federation-key-custody-rotation.md`
- ADR-082 — `/home/devuser/workspace/project/docs/adr/ADR-082-cross-substrate-test-fixture-sharing.md`
- ADR-083 — `/home/devuser/workspace/project/docs/adr/ADR-083-dreamlab-ai-website-cutover-migration.md`
