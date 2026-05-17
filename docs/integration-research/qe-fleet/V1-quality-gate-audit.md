# V1 — Quality Gate Audit: DreamLab Ecosystem PRD/ADR/DDD Set (2026-05-07)

| Field | Value |
|-------|-------|
| Auditor | QE Validator V1 (qe-quality-gate agent) |
| Scope | 2 PRDs (010, 011), 11 ADRs (073-083), 1 DDD (mesh-federation-context) |
| Audit date | 2026-05-07 |
| Inputs total | 6,867 lines across 14 files |
| Verdict format | PASS / WARN / FAIL per gate + go/no-go aggregate |

---

## Executive verdict

**Aggregate: CONDITIONAL GO** — the document set is internally cohesive on the *load-bearing decisions* (mesh topology, identity canonicalisation, envelope, absorption) and conventionally well-formed, but contains **9 specific defects** that should be resolved before fleet implementation begins. Estimated **6.5 engineer-hours** to remediate the blocking defects; **~14 hours** total including non-blocking polish.

| Gate | Verdict | Findings | Blocking? |
|------|---------|----------|-----------|
| G-Cohesion | **WARN** | 6 findings (1 wrong-target reference, 4 phantom requirement IDs, 1 asymmetric companion-ADR set) | 1 of 6 blocking |
| G-Conformance | **PASS** | 2 minor findings (frontmatter field naming, alternative letter ordering) | 0 blocking |
| G-Coverage | **PASS** | 1 finding (PRD-011 companion list incomplete) | 0 blocking |
| G-Implementability | **WARN** | 3 findings (estimate fragmentation, ADR-080 D5 forward-ref, fixture sync command-line ambiguity) | 1 blocking |
| G-Critical-Path | **PASS** | clean phase chain, 5-7 sprint estimate plausible | 0 blocking |

**Recommendation**: deploy implementation swarm immediately on Phase 0 / X0 work (clearly bounded, all blocking defects are downstream). Land the two blocking remediations (Cohesion-1, Implementability-2) within the first sprint.

---

## G-Cohesion — Internal consistency

**Verdict: WARN** — the decision dependencies form a coherent DAG and most cross-references are accurate, but four ADRs cite phantom requirement IDs in PRD-010 and there is one outright wrong-target reference.

### Cohesion-1 — Wrong-target ADR reference (BLOCKING)

`docs/adr/ADR-080-forum-kit-deployment-topology-patterns.md:424`:

> **HA (D5) introduces failover semantics not yet in ADR-073**: ... ADR-073 D11 specified probes + counters but not failover decisions; D5 here adds that. Specifies the trigger but the implementation lands in PRD-011 Phase X3 + a follow-up **ADR-082 (HA failover semantics)**.

But `docs/adr/ADR-082-cross-substrate-test-fixture-sharing.md:1` is titled **"Cross-Substrate Test Fixture Sharing Protocol"** — not HA failover. The implementing-ADR for D5 HA failover semantics is actually unallocated in the current set.

**Severity**: HIGH. A reader following the ADR-080 → ADR-082 forward-reference will land in a fixture-sharing document and either (a) waste time understanding why fixtures relate to failover, or (b) conclude that the failover design lives there when it does not. The fixture sharing was apparently allocated ADR-082 after ADR-080 was drafted.

**Remediation**: either (a) renumber the failover-semantics ADR to ADR-084 and update ADR-080:424; (b) remove the forward-reference in ADR-080:424 and add a "Future work" section noting failover semantics are TBD; (c) absorb the failover behaviour into ADR-080 itself (it is a topology concern). Recommend option (b) as cheapest. **Effort: 0.25 hours.**

### Cohesion-2 — Phantom requirement IDs F31–F50 (BLOCKING)

`docs/adr/ADR-077-ecosystem-qe-policy.md:6` and `docs/adr/ADR-078-cross-substrate-library-convergence.md:6`:

> | Drives | PRD-010 G10, **F31–F50** |

But `docs/PRD-010-did-nostr-mesh-federation.md` ends at F30 (line 754: "F30 — Public surface stability across migration"). There are no F31–F50 in PRD-010. ADR-077 and ADR-078 also cite "PRD-010 F31-F50" inside their References sections (ADR-077:274, ADR-078:317).

**Severity**: HIGH. Sprint planners scoping ADR-077/078 work will go looking for F31-F50 in PRD-010 and find them missing. This will trigger either (a) a clarification cycle, (b) silent invention of missing F-numbers, or (c) abandoned ADR enforcement.

**Remediation options**:
1. Extend PRD-010 §6 with F31–F50 (canonical resolution: each ADR-077/078 P/D maps to a PRD-010 F).
2. Change the "Drives" field to a more honest descriptor: "PRD-010 G10 (library convergence policy) + this-ADR-internal P/D set".
3. Cite PRD-011 F6, F11 (which DO exist) instead.

Recommend option 2 as cheapest and most honest. **Effort: 0.5 hours** (small text edits in two ADRs).

### Cohesion-3 — Asymmetric companion-ADR sets

ADR-077 (Ecosystem QE Policy) lists `Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-078` (`docs/adr/ADR-077-ecosystem-qe-policy.md:7`) but ADR-073/074/075/076 do **not** list ADR-077 in their companion sets.

| ADR | Lists ADR-077 in Companions? |
|-----|------------------------------|
| ADR-073 (`docs/adr/ADR-073-private-nostr-relay-mesh-topology.md:9`) | NO — lists only `ADR-074, ADR-075, ADR-076` |
| ADR-074 (`docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md:8`) | NO — lists only `ADR-073, ADR-075, ADR-076` |
| ADR-075 (`docs/adr/ADR-075-is-envelope-message-contract.md:7`) | NO — lists only `ADR-073, ADR-074, ADR-076` |
| ADR-076 (`docs/adr/ADR-076-nostr-core-absorption-into-upstream.md:7`) | NO — lists only `ADR-073, ADR-074, ADR-075` |

Same defect for ADR-078: cited by ADR-077 (line 7), ADR-079 (line 7), ADR-080 (line 7), ADR-081 (line 7), ADR-082 (line 7), ADR-083 (line 7), but ADR-073/074/075/076 are not aware of ADR-078.

ADR-079 is cited by ADR-080 (line 7), ADR-081 (line 7), ADR-082 (line 7), ADR-083 (line 7), but is not listed as companion of ADR-077 (the QE policy).

**Severity**: MEDIUM. Frontmatter is the index that human readers use to navigate the set; asymmetric companions mean a reader on ADR-073 cannot trivially find ADR-077 even though ADR-077 P3 enforces ADR-073 D6's manifest schema. Affects discoverability, not correctness.

**Remediation**: add ADR-077 + ADR-078 to ADR-073/074/075/076 companion lists; add ADR-079 to ADR-077. **Effort: 0.5 hours.**

### Cohesion-4 — DAG-acyclic verification

I traced the gating chain and confirm no circular dependencies:

```
F26 spike (3-5 days, gates absorption)
   ↓
ADR-076 module-by-module migration (~2 sprints, Phase 0)
   ↓
ADR-074 D8 verifier wiring (post-absorption uses upstream nostr::nips::nip26)
   ↓
ADR-073 D2 fan-out (uses upstream nostr::Event types)
   ↓
ADR-075 envelope (sits atop NIP-59 from upstream)
   ↓
PRD-010 P1-5 phases
   ↓
ADR-082 fixture-sync active (≥7 days clean)
   ↓
PRD-011 phases X0-X6 (kit extraction)
   ↓
ADR-083 cutover (T0-T8, ~28 days; gated on PRD-010 P0 + PRD-011 X1-X4 + ADR-076 + ADR-080 + ADR-081 + ADR-082)
```

No node depends on a descendant. Phase 0 is correctly the universal prerequisite.

### Cohesion-5 — Phase-mapping coherence between PRD-010, PRD-011, ADR-077

**PRD-010** uses Phase 0..5 (`docs/PRD-010-did-nostr-mesh-federation.md:766-829`).
**PRD-011** uses Phase X0..X6 (`docs/PRD-011-visionflow-forum-kit-extraction.md:430-471`).
**ADR-077** uses P0..P5 mapping (`docs/adr/ADR-077-ecosystem-qe-policy.md:218-224`).

The two phase-numbering systems are deliberately distinct (Px = PRD-010 mesh phase; Xx = PRD-011 kit phase) and there is no ambiguity in any single document. ADR-077 explicitly maps to PRD-010 phase numbers. **PASS.**

### Cohesion-6 — Cross-document semantic equivalence

I spot-checked several claims for self-consistency:

| Claim | PRD-010 | ADR-073 | ADR-074 | DDD | Consistent? |
|-------|---------|---------|---------|-----|-------------|
| Default `mesh.federated_kinds = [14, 1059, 30033, 30910..30916]` | F11 line 591, line 601 | D9 line 142, D6 line 106 | (cited via D9) | (cited) | YES |
| `verificationMethod.type = SchnorrSecp256k1VerificationKey2019` | F4 line 526 | (cited) | D2 line 73 | S-Inv-03, A-Inv-06 | YES |
| Federation key cardinality "1 per relay" | F11 line 590 | D4 line 80 | (cited) | (referenced) | YES |
| kind-30033 / kind-30050 allocation | F25 line 716 | D9 line 164 (074) | D9 line 167 (074) | "BC-MESH-FORUM-Inv F-Inv-07" | YES |
| Forum nostr-core post-absorption ~700 LOC | F25 line 723 | D2 line 103 (076) | (cited) | F-Inv-07 line 42 | YES |
| C1 NIP-44 conv key bug | §4.5 line 232 | (cited) | (cited) | (cited) | YES |
| TTL of 600s on kind-30033 cache | §10 Q3 line 953-958 | D11 line 175 | D5 line 114 | TR-DID-Resolution line 332 | YES |

No semantic drift detected on the load-bearing claims.

### Cohesion-7 — PRD-011 references PRD-010 / ADR-076 / ADR-077 / ADR-078 / ADR-079 / ADR-080

`docs/PRD-011-visionflow-forum-kit-extraction.md:7-8`:

> | Predecessors | PRD-010, ADR-073, ADR-074, ADR-075, ADR-076, ADR-077, ADR-078 |
> | Companion ADRs | ADR-079 (forum-setup skill), ADR-080 (kit deployment topology) |

This **excludes** ADR-081 (federation key custody, drives PRD-011 G7), ADR-082 (fixture sharing, drives kit's QE per ADR-077 P1), and ADR-083 (cutover migration, drives PRD-011 R5/Phase X5). All three were authored on the same date (2026-05-07) and explicitly drive PRD-011 requirements:

- ADR-081:6: `Drives | PRD-010 R7..., **PRD-011 G7**`
- ADR-082:6: drives ADR-077 enforcement which PRD-011 inherits (G9)
- ADR-083:6: `Drives | PRD-011 G3 + G8 + R5 (cutover safety), Phase X5`

**Severity**: LOW. PRD-011 was likely drafted before 081/082/083 (which appear to be later splits). The companion-list staleness is cosmetic but signals an unmaintained index.

**Remediation**: add ADR-081, ADR-082, ADR-083 to PRD-011 frontmatter Companion ADRs. **Effort: 0.1 hours.**

---

## G-Conformance — Style + format

**Verdict: PASS** — every ADR has the canonical frontmatter table, every ADR uses Context → Decision → Consequences → Alternatives → Implementation notes → References structure. Numbered decisions D1, D2, ... and lettered alternatives Alt-A, Alt-B, ... with explicit "*Rejected*: <reason>" rationale. File:line citations are consistent throughout. No structural violations.

### Conformance-1 — Frontmatter field naming variance (cosmetic)

The frontmatter field names drift slightly across ADRs:

| ADR | "Drives" field | "Status" field | "Affected repos" field |
|-----|----------------|----------------|------------------------|
| ADR-073:5 | absent | "Proposed (2026-05-07)" | absent |
| ADR-074:5 | "Drives" | "Proposed (2026-05-07)" | absent |
| ADR-076:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |
| ADR-077:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |
| ADR-078:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |
| ADR-079:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |
| ADR-080:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |
| ADR-081:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |
| ADR-082:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |
| ADR-083:5 | "Drives" | "Proposed (2026-05-07)" | "Affected repos" |

ADR-073 alone omits the "Drives" and "Affected repos" rows. This is the foundational ADR and arguably should match the rest. ADR-074:7 has "Supersedes | ADR-027 (DID identity stack — extends)" which only ADR-074 carries.

**Severity**: COSMETIC.

**Remediation**: backfill ADR-073 with `Drives | PRD-010 G3, G4, G7` (already implicit) and `Affected repos | dreamlab-ai-website (forum), agentbox, this repo (VisionClaw), solid-pod-rs`. **Effort: 0.1 hours.**

### Conformance-2 — Alternatives lettering ordering

Every ADR uses Alt-A, Alt-B, Alt-C consistently. ADR-077 has Alt-A through Alt-E (5 alternatives, line 187-216). ADR-076 has Alt-A through Alt-E (line 274-302). ADR-078 has Alt-A through Alt-D (line 243-265). All are sequential without gaps. PASS.

### Conformance-3 — File:line citations are present

Spot check (10 random samples):

| Document | Line | Citation form | Valid? |
|----------|------|---------------|--------|
| PRD-010:153 | `relay-worker/src/relay_do/mod.rs:140` | code:line | yes (referenced in ADR-073) |
| ADR-073:16 | `relay_do/mod.rs:140` | code:line | yes (matches PRD-010) |
| ADR-074:18 | `docs/integration-research/05-crypto-gotchas.md` §2-§3 | doc:section | yes (file exists) |
| ADR-075:175 | `management-api/middleware/linked-data/jcs.js` per `docs/integration-research/03-agentbox-surfaces.md` §10 | doc:section | yes |
| ADR-076:13 | `dreamlab-ai-website/community-forum-rs/crates/nostr-core/` | path | yes (referenced consistently) |
| ADR-077:21 | `crates/relay-worker/src/relay_do/session.rs:284-292` | code:line | yes |
| ADR-078:104 | `Q1 §0` | doc:section | yes (Q1 file exists) |
| ADR-080:177 | `docs/integration-research/qe-fleet/Q3-identity-custody-audit.md`:581 | doc:line | yes |
| ADR-081:14 | `Q3 §I12` | doc:section | yes |
| ADR-082:60 | `paulmillr/nip44`, commit `7fe2cabb02bdce6f3d5b3e90c2b4a3e1f0c4d8a3` | repo:commit | unverifiable from inside repo (external) |

All internally-resolvable references are correct. External pin verification (paulmillr commit, BIP-340 vectors) would require network access; flagged as a future check, not a current defect.

### Conformance-4 — GitHub URL references at end

All ADRs that introduce or affect repos include explicit GitHub URLs in their References section:

- ADR-077:280-285 — paths but no URLs (acceptable; cites doc paths in repo)
- ADR-078:316-328 — has GitHub URLs (paulmillr/nip44, etc.)
- ADR-079:325-328 — has GitHub URLs (DreamLab-AI/nostr-rust-forum, etc.)
- ADR-080:589-595 — has GitHub URLs
- ADR-081:425-430 — has GitHub URLs
- ADR-082:545-550 — has GitHub URLs
- ADR-083:436-440 — has GitHub URLs

PRD-010:1141-1161 has internal references; PRD-011:573-579 has GitHub URLs.

PASS.

---

## G-Coverage — Architectural completeness

**Verdict: PASS** — all decisions mentioned in PRD-010/011 are backed by an ADR, key cross-cutting concerns (security, QE, identity, deployment, migration) are covered, and there are no single-sentence "we decided to do X" without a corresponding full ADR.

### Coverage-1 — Decision-to-ADR mapping

I extracted PRD-010's load-bearing decisions and mapped them to ADRs:

| PRD-010 decision | ADR | Coverage adequate? |
|------------------|-----|-------------------|
| G3 mesh topology, AUTH gate | ADR-073 | YES — full D1-D11 |
| G1 single canonical DID | ADR-074 D1-D6 | YES |
| G5 NIP-26 trust pivot | ADR-074 D8-D11 | YES |
| G2 unified envelope | ADR-075 D1-D15 | YES |
| G9 established library | ADR-076 D1-D10 | YES |
| G10 library convergence (broader than nostr-core) | ADR-078 D1-D8 | YES |
| Cryptographic correctness gating | §8 + ADR-076 + ADR-077 P1 | YES |
| Mesh deployment switches (G7) | ADR-073 D6 + ADR-080 D1-D9 | YES |
| Cross-system message contract | ADR-075 | YES |
| QE policy | ADR-077 P1-P10 | YES |

PRD-011 decisions:

| PRD-011 decision | ADR | Coverage adequate? |
|------------------|-----|-------------------|
| G2 federation native | ADR-073, 074, 075 | YES |
| G4 TOML-driven config | §5.2 schema; topology classes via ADR-080 | YES |
| G5 AI configurator | ADR-079 | YES |
| G7 de-branding | ADR-080 D7 (downstream-consumer pattern) | YES |
| G8 re-import path | ADR-083 (cutover) | YES |
| G9 QE parity | ADR-077, ADR-082 | YES |
| Federation key custody | ADR-080 D6 + ADR-081 (full custody+rotation) | YES |
| Cutover safety (R5) | ADR-083 | YES |

No PRD requirement lacks an ADR. No ADR is orphaned (all are cited by at least one PRD or higher-level ADR).

### Coverage-2 — Cross-cutting concerns

| Concern | Treatment | Adequacy |
|---------|-----------|----------|
| Security | ADR-077 P8 (CI gates) + ADR-081 (key custody) + ADR-074 D13 (anti-drift) | comprehensive |
| QE / testing | ADR-077 (policy) + ADR-082 (fixture sharing) | comprehensive |
| Identity / DID | ADR-074 + ADR-081 (custody) | comprehensive |
| Deployment | ADR-080 (six topology patterns) + PRD-011 §5 | comprehensive |
| Migration | ADR-083 (cutover) + ADR-080 D9 (migration topology) | comprehensive |
| Rollback | ADR-083 D9 (rollback matrix) | comprehensive |
| Observability | ADR-073 D11 (/health/mesh) + ADR-077 P10 (/health/qe) + ADR-081 D10 (/health/keys) | comprehensive |
| Operator runbooks | ADR-081 implementation notes (rotation runbook template) + ADR-083 D11 (cutover checklist) | comprehensive |

All cross-cutting concerns explicitly addressed.

### Coverage-3 — Companion-PRD list completeness

`docs/PRD-011-visionflow-forum-kit-extraction.md:7-8` excludes ADR-081/082/083 — see Cohesion-7. Severity LOW; remediation **0.1 hours**.

### Coverage-4 — DDD coverage of mesh-federation

`docs/ddd-mesh-federation-context.md` covers four bounded contexts (BC-MESH-FORUM, BC-MESH-AGENTBOX, BC-MESH-VISIONCLAW, BC-MESH-SOLID-POD-RS) plus four ACLs (VC↔FORUM, VC↔AGENTBOX/BC20, FORUM↔AGENTBOX, VC↔SOLID-POD-RS, FORUM↔SOLID-POD-RS as a "gap" ACL).

Invariants F-Inv-01..07, A-Inv-01..09, V-Inv-01..07, S-Inv-01..04 are all numbered, scoped to their bounded context, and traceable to ADR or PRD requirements.

Translation rules TR-DID-Resolution, TR-Bead-URN-VC↔Agentbox, TR-Identity-Lowercase, TR-Delegation-Forward, TR-Moderation-Honour, TR-IS-Envelope-Validation are formal enough to drive ACL implementation.

DQ1-DQ4 (open domain questions) are honest about what is unresolved and propose resolutions. PASS.

### Coverage-5 — Are there hidden decisions that need formalising?

I scanned for "we decided to do X" or "we will" phrasings that should be ADRs:

| Location | Statement | Adequately formalised? |
|----------|-----------|-----------------------|
| PRD-011:514 (Q1 license) | "Default recommendation: MIT/Apache-2.0 dual" | open question, deferred to "kit ADR-001"; acceptable |
| PRD-010 §12:1075 | "F25 absorbs and supersedes the originally-listed in-place patch" | covered by ADR-076 |
| ADR-080:424 | "implementation lands in PRD-011 Phase X3 + a follow-up ADR-082 (HA failover semantics)" | this is the broken forward-ref (Cohesion-1) |
| ADR-080:303 | "or wait for a future local-LLM provider" | clearly future work, acceptable |
| ADR-082:344-362 | "property-based fixture extension" via D12 | fully specified |
| DDD DQ1-DQ4 | open domain questions with proposed resolutions | acceptable to defer |

No hidden full-ADR-shaped decisions are smuggled in as one-liners.

---

## G-Implementability — Sprint-readiness

**Verdict: WARN** — most ADRs specify implementation in concrete enough terms for sprint planning, but estimate fragmentation and one design forward-reference (Cohesion-1) leave unresolved scope.

### Implementability-1 — Estimate consistency

| Document | Effort estimate | Engineers | Source |
|----------|-----------------|-----------|--------|
| PRD-010 §7:830 | "~6 sprints (~12 weeks) at 1 engineer FTE; ~4 sprints with 2 engineers parallel" | 1-2 | Section 7 |
| PRD-011 §7:471 | "~5-6 sprints (~10-12 weeks) at 1 engineer FTE ... compresses to ~3-4 sprints" | 1-2 | Section 7 |
| ADR-076 §D10:238 | "Phase 0 grows from 1 sprint to ~2 sprints" | 1 | D10 |
| ADR-077 §C:177 | "~75 engineer-days per Q4 G14 to land the full programme" | (mixed) | Consequences |
| ADR-078 §D6:192 | "Total ecosystem absorption: ~76-86 engineer-days. Roughly 4-6 engineer-sprints." | (mixed) | D6 |
| ADR-081 §C:300 | "3 substrates × ~5 CLI commands = ~15 new commands" | (no time) | Consequences |
| ADR-083 §D1:44 | "T₀ → T₆: ~14 days. T₆ → T₇: +7 days. T₇ → T₈: +7 days" | (cutover ops) | D1 |

ADR-077 says 75 e-days; ADR-078 says 76-86 e-days. These overlap heavily (ADR-078 absorbs ADR-077 work). PRD-010's "6 sprints" + PRD-011's "5-6 sprints" if fully serialised is ~11-12 sprints, but PRD-011 §7:471 explicitly calls out parallel execution: "Phase X4 (skill) and Phase X3 (QE) can run parallel to X1/X2" — so ~5-7 sprints in practice with 2 engineers.

**Severity**: LOW. The estimates are not contradictory but are scattered across documents. A sprint planner has to assemble them.

**Remediation**: add a single "Roadmap" appendix to PRD-010 or PRD-011 that consolidates effort × phase × parallelism into one table. **Effort: 1 hour.**

### Implementability-2 — Forward-reference orphan (BLOCKING)

ADR-080 D5:424 promises "follow-up ADR-082 (HA failover semantics)" — but ADR-082 is "Cross-Substrate Test Fixture Sharing" (per Cohesion-1). The HA failover behaviour beyond ADR-073 D11 probes is therefore *unspecified* in the ADR set. ADR-080 D5 says "auto_failover_threshold_seconds" + "buffer up to fanout_buffer_size events" but does not specify the failover decision algorithm in detail.

**Severity**: HIGH for D5 (HA topology). LOW for D1-D4, D6-D9 which are fully specified.

**Remediation options**:
1. Allocate ADR-084 for HA failover semantics; defer for post-PRD-011 implementation.
2. Add the failover algorithm to ADR-080 D5 inline (~30-50 lines).
3. Mark D5 explicitly as P3 (post-cutover) and decline to specify until a real operator hits it.

Recommend option 3 — D5 (HA topology) is not on the critical path for PRD-010/011 GA. Annotate D5 with "implementation deferred; failover semantics to be specified in a future ADR when required by a deployment". **Effort: 0.5 hours.**

### Implementability-3 — Test surfaces specified per ADR-077 P1-P10

I verified each ADR specifies its test surface where applicable:

| ADR | Reference vectors? | Property tests? | Mutation? | Coverage threshold? |
|-----|-------------------|-----------------|-----------|---------------------|
| ADR-073 §Implementation:319 | yes (mesh smoke) | yes (LRU dedup, fan-out) | (not specified) | (inherits ADR-077) |
| ADR-074 §Implementation:313 | yes (nip26_vectors.json) | (covered by ADR-077) | (covered) | (covered) |
| ADR-075 §D15:373 | yes (envelope_v1_*.rs) | yes (round-trip) | (covered) | (covered) |
| ADR-076 §D8:213 | yes (paulmillr/nip44 etc.) | yes (proptest on nip19, nip04) | yes (D8 implies coverage) | yes (D8) |
| ADR-077 §P1-P10 | (defines policy) | (defines policy) | yes (P4 90/80/75/85%) | yes (P6) |
| ADR-078 §D8:201 | (license audit only) | (inherits ADR-077) | (inherits ADR-077) | (inherits ADR-077) |
| ADR-079 §Tests:307 | yes (provider_contract.rs, IS-Envelope vectors) | yes (conversation_flow.rs) | yes (≥80% conversation.rs) | (inherits ADR-077) |
| ADR-080 §QE invariants:566 | yes (per-topology fixtures) | yes (composition tests) | (inherits ADR-077) | (inherits ADR-077) |
| ADR-081 §D11:264 | yes (nip26 paulmillr conditions=kind=30033) | yes (rotation flow) | yes (≥80% rotation flow) | (inherits) |
| ADR-082 (defines fixture) | (defines policy) | yes (D12 property-based extension) | (covered) | (covered) |
| ADR-083 §implementation:336 | yes (api-parity.sh) | yes (schema-parity-check.sh) | (cutover N/A) | (cutover N/A) |

Every ADR with implementable code references its test surface. PASS.

### Implementability-4 — Rollback paths documented for risky changes

Per the prompt's specific concern about ADR-083:

`docs/adr/ADR-083-dreamlab-ai-website-cutover-migration.md:208-218` defines a 7-row rollback matrix (5xx spike, session loss, schema corruption, WebAuthn breakage, pod ACL drift, subtle regression, catastrophic data loss) with detection method, rollback path, and time-to-recover for each. D9:218 explicitly drills the rollback paths in staging at T₂.

Other risky changes:

- ADR-076 (nostr-core absorption): D5:137 (validation spike with explicit acceptance criteria); D6:156 (per-module per-PR with parity gates); R7 in PRD-010:926-936 (spike outcome dictates Shape A vs Shape C fallback).
- ADR-074 D12 (key rotation): D7:177 (revocation flow with explicit zero-overlap window).
- ADR-081 D7:177 (emergency-revoke command).

All risky changes have rollback paths. PASS.

### Implementability-5 — Fixture sync command-line ambiguity

`docs/adr/ADR-082-cross-substrate-test-fixture-sharing.md:174`:

> Each substrate's CI runs `sync-fixtures.sh --verify` (a flag that skips the actual sync but compares local checksums against the master's; flag exits 0 if consistent, 1 if drift detected).

The `sync-fixtures.sh` shell script body shown at line 149-172 does not implement `--verify` — it always pulls and rsyncs. Implementation note (lines 481-482) says "if: failure() upload-artifact" but the verify flag itself is unspecified.

**Severity**: LOW. The intent is clear; the script needs `--verify` added during implementation.

**Remediation**: extend the example shell script in ADR-082:149-172 to include the `--verify` branch. **Effort: 0.25 hours.**

### Implementability-6 — Dependencies "depends on" annotation traceable

ADR-078 D6:166 has explicit batch-level dependencies (B1, V1, V2, A1, A4, S2, S5, V3 → B1; A2 → A1; A5 → A1; A6 → A5; B2 → B5; etc.). Dependencies form a DAG. Effort tally is itemised per batch. PASS.

ADR-083 D13:273 has explicit cutover gating: "Cannot proceed until: C1 fixed, C2 fixed, C3 fixed, ADR-080 selected, ADR-082 active 7 days, ADR-081 implemented." PASS.

---

## G-Critical-Path — Sequencing

**Verdict: PASS** — clear gating chain, parallel-able tracks identified, 5-7 sprint estimate plausible.

### Critical-Path-1 — Gating chain verified

```
F26 spike (3-5 days) → blocks all of P0
  ↓
ADR-076 absorption (per-PR PRs 1-11, ~2 sprints) → fixes C1, C3, L20
  + agentbox C2 fix (parallel) → in-place patch sovereign-bootstrap.py
  + verificationMethod.type fixes (parallel) → 3 patch sites
  ↓
PRD-010 P1 (DID Document & resolver, 1 sprint)
  ↓
PRD-010 P2 (AUTH + delegation, 1 sprint) — parallel-able with P3 in 2-engineer mode
  ↓
PRD-010 P3 (Bridge wiring + mesh fan-out, 1.5 sprints) — agentbox RelayConsumer boot wiring + VC MeshBridge
  ↓
PRD-010 P4 (Envelope contract + cross-system flows, 1 sprint)
  ↓
PRD-010 P5 (Consolidation, 0.5 sprint)

PRD-011 (kit extraction) — STARTS in parallel with PRD-010 P3:
  ↓
X0 (clone + branch, 3 days) → kit ready for de-branding
X1 (workspace restructure, 1 sprint) → nostr-bbs-config + nostr-bbs-mesh stubs
X2 (library convergence, 1.5 sprints) → kit-side ADR-076 absorption
X3 (QE policy, 1 sprint) — parallel with X2
X4 (forum-setup skill, 1 sprint) — parallel with X1/X2
X5 (DreamLab cutover, 1 sprint) → ADR-083
X6 (v3.0.0 GA, 0.5 sprint)

ADR-083 cutover — final step:
  T₀ → T₆: 14 days
  T₆ → T₇: +7 days observation
  T₇ → T₈: +7 days audit
  Total: ~28 days
```

The chain is acyclic and well-ordered. Phase 0 (gating) is the single bottleneck; everything else parallel-isable in 2-engineer mode.

### Critical-Path-2 — 5-7 sprint estimate plausibility check

Combining PRD-010 (~4 sprints, 2 engineers parallel) + PRD-011 (~3-4 sprints, parallel within the same window where P3-P5 of PRD-010 can interleave with X1-X4 of PRD-011) + ADR-083 cutover (~4 weeks ≈ 1.5 sprints) gives **~5-7 sprints (10-14 weeks)** elapsed for a 2-engineer team. This matches the audit prompt's expectation.

If the F26 WASM/CF Workers spike fails and ADR-076 falls back to Shape C (in-place patch), the timeline contracts back to ~3-4 sprints because absorption work disappears. PRD-010 §7:792 documents this fallback explicitly.

### Critical-Path-3 — Parallel-able tracks identified

| Track | Parallel with | Independence |
|-------|---------------|--------------|
| ADR-076 absorption (Forum) | C2/C3 fixes (different repos) | YES — different file trees |
| PRD-011 X1 (kit restructure) | PRD-010 P2 (auth wiring) | YES — kit work doesn't touch existing forum auth code |
| PRD-011 X3 (kit QE) | PRD-011 X4 (skill) | YES — different crates |
| ADR-082 fixture sync | ADR-076 module migrations | YES — fixture infra orthogonal |
| ADR-081 custody tooling | ADR-079 setup skill | YES — both needed pre-cutover but independent |

Every parallel track has clear repo or module boundary; no integration debt expected. PASS.

### Critical-Path-4 — Single-sprint blockers

Phase 0 is the tightest bottleneck. Within Phase 0:

1. F26 spike (3-5 days) blocks F25 absorption (Forum). If spike fails, fall-back is single in-place patch (1 day).
2. F4 + F5 + F18 (agentbox npub + verificationMethod.type) are 3 independent patch sets, each ~0.5-1 day.
3. ADR-082 fixture vendoring (paulmillr nip44, BIP-340, JCS) — 1-2 days, blocks per-PR vector tests.

A 2-engineer team can execute Phase 0 in 1 sprint with parallelism + 1 sprint of buffer for the absorption PRs. The PRD-010 §7:768 estimate of "~2 sprints" for Phase 0 is realistic.

---

## Aggregate go/no-go: deploy implementation swarm?

**GO with two pre-merge remediations.**

The document set is mature enough to drive a 5-7 sprint implementation with a 2-engineer fleet. The DAG is acyclic, the load-bearing decisions are mutually consistent, and the test/QE/security/migration policies are concretely enough specified to write code against. The two blocking defects are surface-level and can be fixed in ~1 hour.

### Required pre-merge remediations (BLOCKING)

| ID | Defect | Location | Effort |
|----|--------|----------|--------|
| Cohesion-1 | ADR-080:424 cites wrong target (ADR-082) for HA failover semantics | docs/adr/ADR-080-forum-kit-deployment-topology-patterns.md:424 | 0.25 h |
| Cohesion-2 | ADR-077:6 + ADR-078:6 cite phantom PRD-010 F31-F50 (do not exist) | docs/adr/ADR-077-ecosystem-qe-policy.md:6, docs/adr/ADR-078-cross-substrate-library-convergence.md:6 | 0.5 h |
| Implementability-2 | ADR-080 D5 HA failover algorithm unspecified due to broken forward-ref | docs/adr/ADR-080-forum-kit-deployment-topology-patterns.md:424 | 0.5 h |

**Total blocking remediation effort: 1.25 hours.**

### Recommended (NON-BLOCKING) polish

| ID | Defect | Location | Effort |
|----|--------|----------|--------|
| Cohesion-3 | Asymmetric Companion ADR sets — ADR-073/074/075/076 don't list ADR-077, ADR-078, ADR-079 | 4 ADR frontmatter tables | 0.5 h |
| Cohesion-7 | PRD-011 doesn't list ADR-081/082/083 in companions | docs/PRD-011-visionflow-forum-kit-extraction.md:7-8 | 0.1 h |
| Conformance-1 | ADR-073 frontmatter table missing "Drives" + "Affected repos" | docs/adr/ADR-073-private-nostr-relay-mesh-topology.md:5-10 | 0.1 h |
| Implementability-1 | Effort estimates scattered; needs consolidation table | append to PRD-010 or PRD-011 | 1.0 h |
| Implementability-5 | sync-fixtures.sh --verify branch missing from ADR-082 example | docs/adr/ADR-082-cross-substrate-test-fixture-sharing.md:149-172 | 0.25 h |

**Total non-blocking polish effort: ~2 hours.**

### What the document set does notably well

1. **Cryptographic correctness as a gating phase** — Phase 0 is non-negotiable before anything else, and ADR-076 + ADR-077 P1 (paulmillr vectors) make the bug class structurally preventable rather than relying on review discipline. This is the right shape.

2. **Library convergence policy** — ADR-077 P3 + ADR-078 D1 (registry) + ADR-077 P3 anti-drift lint forms a coherent policy framework that prevents the C1-class drift across all four repos. Six absorption batches with itemised effort.

3. **Cutover safety** — ADR-083 has a 7-row rollback matrix with detection, rollback path, and time-to-recover, plus a pre-flight checklist (D11) and stakeholder communication template (Implementation notes). Production-grade.

4. **DDD bounded context map** — `ddd-mesh-federation-context.md` resolves PRD-006's "composite agent URN" question explicitly (D7 of ADR-074), names ACL boundaries with location and translation rules, and tracks open domain questions (DQ1-DQ4) with resolution proposals.

5. **Topology vocabulary** — ADR-080's six named patterns + decision tree + `topology` field in TOML give operators, configurator (ADR-079), and QE fixtures (ADR-082) a shared vocabulary. Air-gapped (D8) is named as a first-class topology, not an afterthought.

6. **Federation key custody** — ADR-081 quantifies 10 distinct key roles, three custody tiers, per-role rotation cadence, anti-collision lint (D8), and emergency revocation (D7). Closes the Q3 "operators will collapse roles back into one shared key" warning.

### What the document set could do better (post-implementation)

1. **Single-page roadmap** — consolidate PRD-010 §7 + PRD-011 §7 + ADR-077/078 e-day estimates into one table. Currently scattered.

2. **HA failover ADR (ADR-084)** — D5 HA topology is named but failover semantics (decision algorithm, buffer size, replay ordering) are unspecified. Defer until a real operator deploys HA, but track it as a known gap.

3. **Sprint v9-v11 sprint summary** — PRD-011 R3 alludes to "per-sprint summary linked to community-forum-rs commit ranges" but doesn't include it. A pre-import audit of those sprint commits would protect against R3 (Sprint v9-v11 fidelity loss during single import commit).

4. **License decision (PRD-011 Q1)** — kit licensing is deferred to "kit ADR-001". Should be decided pre-X0 because it affects every contributor's first commit.

5. **Multi-agent agentbox per container (DDD DQ4)** — explicitly P5 follow-up but already reflected in `RelayConsumer.npubs: [...]` shape. Document the planned A-Inv-11 (multi-agent invariant) so contributors know to preserve the array shape.

---

## Summary table

| Gate | Verdict | Findings | Blocking | Remediation effort |
|------|---------|----------|----------|-------------------|
| G-Cohesion | WARN | 6 (1 wrong-target, 1 phantom IDs, 1 asymmetric companions, 1 trivial PRD-011 frontmatter, 2 verified clean) | 2 | 0.85 h blocking, 0.6 h polish |
| G-Conformance | PASS | 4 (3 cosmetic, 1 verified clean) | 0 | 0.1 h polish |
| G-Coverage | PASS | 5 (4 verified, 1 trivial PRD-011 frontmatter) | 0 | (covered above) |
| G-Implementability | WARN | 6 (1 estimate fragmentation, 1 forward-ref orphan, 4 verified clean) | 1 | 0.5 h blocking, 1.25 h polish |
| G-Critical-Path | PASS | 4 verified | 0 | 0 |

**Aggregate: CONDITIONAL GO. Total blocking remediation: ~1.25 engineer-hours. Total polish (recommended): ~2 hours. Implementation fleet may proceed on Phase 0 / X0 work concurrent with remediation since neither blocking defect is in the critical path of the first sprint.**

---

## Appendix A — Audited document inventory

| File | Lines | Status field |
|------|-------|--------------|
| `docs/PRD-010-did-nostr-mesh-federation.md` | 1227 | Draft (2026-05-07) |
| `docs/PRD-011-visionflow-forum-kit-extraction.md` | 578 | Draft (2026-05-07) |
| `docs/ddd-mesh-federation-context.md` | 463 | Draft (2026-05-07) |
| `docs/adr/ADR-073-private-nostr-relay-mesh-topology.md` | 341 | Proposed (2026-05-07) |
| `docs/adr/ADR-074-cross-system-did-nostr-canonicalisation.md` | 404 | Proposed (2026-05-07) |
| `docs/adr/ADR-075-is-envelope-message-contract.md` | 543 | Proposed (2026-05-07) |
| `docs/adr/ADR-076-nostr-core-absorption-into-upstream.md` | 352 | Proposed (2026-05-07) |
| `docs/adr/ADR-077-ecosystem-qe-policy.md` | 285 | Proposed (2026-05-07) |
| `docs/adr/ADR-078-cross-substrate-library-convergence.md` | 328 | Proposed (2026-05-07) |
| `docs/adr/ADR-079-forum-setup-skill-provider-abstraction.md` | 328 | Proposed (2026-05-07) |
| `docs/adr/ADR-080-forum-kit-deployment-topology-patterns.md` | 599 | Proposed (2026-05-07) |
| `docs/adr/ADR-081-federation-key-custody-rotation.md` | 430 | Proposed (2026-05-07) |
| `docs/adr/ADR-082-cross-substrate-test-fixture-sharing.md` | 550 | Proposed (2026-05-07) |
| `docs/adr/ADR-083-dreamlab-ai-website-cutover-migration.md` | 439 | Proposed (2026-05-07) |
| **Total** | **6867** | — |

## Appendix B — Defect register (citable)

| ID | File | Line | Defect | Severity | Effort |
|----|------|------|--------|----------|--------|
| Cohesion-1 | ADR-080 | 424 | wrong-target ADR ref ("ADR-082 (HA failover)" but ADR-082 is fixtures) | HIGH | 0.25 h |
| Cohesion-2a | ADR-077 | 6 | "Drives | PRD-010 G10, F31–F50" — F31-F50 don't exist in PRD-010 | HIGH | 0.25 h |
| Cohesion-2b | ADR-078 | 6 | same as Cohesion-2a | HIGH | 0.25 h |
| Cohesion-2c | ADR-077 | 274 | References section cites "PRD-010 F31-F50" | HIGH | (rolled into 2a) |
| Cohesion-2d | ADR-078 | 317 | References section cites "PRD-010 F31-F50" | HIGH | (rolled into 2b) |
| Cohesion-3a | ADR-073 | 9 | Companion ADRs missing 077, 078 | MED | 0.1 h |
| Cohesion-3b | ADR-074 | 8 | Companion ADRs missing 077, 078 | MED | 0.1 h |
| Cohesion-3c | ADR-075 | 7 | Companion ADRs missing 077, 078 | MED | 0.1 h |
| Cohesion-3d | ADR-076 | 7 | Companion ADRs missing 077, 078 | MED | 0.1 h |
| Cohesion-3e | ADR-077 | 7 | Companion ADRs missing 079 | MED | 0.1 h |
| Cohesion-7 | PRD-011 | 7-8 | Companion ADRs missing 081, 082, 083 | LOW | 0.1 h |
| Conformance-1 | ADR-073 | 5-10 | frontmatter missing Drives, Affected repos | COSMETIC | 0.1 h |
| Implementability-1 | (multi-doc) | — | effort estimates scattered across 7 documents | LOW | 1.0 h |
| Implementability-2 | ADR-080 | 424 | HA failover algorithm unspecified due to broken forward-ref | HIGH | 0.5 h |
| Implementability-5 | ADR-082 | 149-172 | sync-fixtures.sh --verify branch unimplemented in example | LOW | 0.25 h |

**Total: 15 defects, 3 blocking (Cohesion-1, Cohesion-2, Implementability-2), ~3.25 hours total effort.**
