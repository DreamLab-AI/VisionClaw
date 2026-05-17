# PRD-005 Implementation Benefit Estimates

**Status:** Draft v1 — analyst's projections, sanity-checked against existing telemetry where present
**Date:** 2026-05-01
**Author:** VisionClaw platform team
**Companion to:** [PRD-005](PRD-005-graph-cognition-platform.md), [ADR-064](adr/ADR-064-typed-graph-schema.md)–[ADR-070](adr/ADR-070-cuda-integration-hardening.md), [DDD Graph Cognition](ddd-graph-cognition-context.md)

> **What this document is.** A pre-implementation benefit projection. Estimates are bounded with confidence intervals where data exists and clearly tagged "speculative" where it doesn't. After Phase 1 ships, this doc is revisited and projections updated against measured outcomes.
>
> **What this document is not.** A business case requiring sign-off. The user (sovereign individual) is the implementer; the benefit framing is operator-centric: capability-per-week-of-effort, risk-adjusted, with explicit costs.

---

## 1. Executive summary

Implementation across 19 weeks (4 phases, ~6 dev-weeks per phase including the QE-mandated additions) yields **eight measurable capability gains** and **three strategic-positioning gains**. The work is mostly *connector engineering* — the GPU substrate, identity layer, and federation layer are already mature; PRD-005 wires them together via a new typed schema, a Rust code-analysis pipeline, a block-level Logseq parser, and six 3D dashboard features.

Headline projected benefits, ranked by leverage:

| # | Benefit | Time-to-impact | Confidence |
|---|---------|---------------|-----------|
| 1 | Code repositories become navigable 3D graphs (today: not at all) | End of Phase 1 (week 9) | High |
| 2 | Logseq vault block-level fidelity (today: page-level) | End of Phase 1 (week 9) | High |
| 3 | Path-find on 100k-node graphs at p99 ≤8ms via existing GPU APSP (today: not exposed) | Phase 3 (week 14) | High |
| 4 | Persona-graded views (today: 0 views) | Phase 3 (week 16) | Medium |
| 5 | Solid-pod-federated graphs across `did:nostr` peers (today: 0 federation) | Phase 2 (week 13) | Medium |
| 6 | Reduced LLM cost via incremental update + fingerprint (vs naive every-commit re-analyze) | Phase 4 (week 19) | Medium |
| 7 | OWL/SHACL reasoning federation via ontobricks bridge (today: VC has OWL constraint kernel; no SHACL/SWRL) | Phase 3 (week 17) | Lower (opt-in, depends on user Databricks access) |
| 8 | 3D ontology constraint visualisation across 5 OWL kinds (DisjointWith, SubClassOf, SameAs, InverseOf, Functional) | Already exists; PRD-005 lights it up via UA's typed schema | High |

Strategic positioning:

| # | Strategic gain | Lever |
|---|---------------|-------|
| S1 | Sovereign identity meets graph cognition: every typed node has a `did:nostr`-bound URN, content-addressed in a Solid pod | Combination of existing URN/pod infra + new typed schema |
| S2 | Federated cognition pattern: VC ↔ ontobricks via MCP; reasoning *and* visualization both speak MCP — generalises to other reasoning servers | Foundational; opens future PRDs |
| S3 | Block-level retrieval-grade fidelity: Logseq vault becomes RAG-ready at block granularity rather than chunked-page noise; aligns with Karpathy LLM-OS thesis | Opens private-AI-agent use cases not previously addressable |

---

## 2. Capability gains (measurable)

### 2.1 — Code analysis: zero → working

**Today:** No `vc analyze` capability. Foreign repository understanding requires reading every file or trusting the README.

**After Phase 1 (week 9):**
- `vc analyze <repo>` produces a typed graph for any repo containing tree-sitter-supported languages (Rust, TypeScript, Python, Go, Java in P1; six more in P2).
- ≥95% file coverage on the reference repo (AC-B.5).
- Graph appears in 3D dashboard within 2 seconds of analysis completion (UC-02).
- LLM-augmented summaries, complexity, tags per node.

**Quantified value (analyst, single-user lens):**
- Estimated time-to-orientation on a foreign 250-file repo:
  - Today: 4–8 hours (sampled file reading + Grep).
  - Post-PRD-005: 5 minutes wall-clock + interactive exploration.
  - **~50× faster orientation** for the use case where it works.
- LLM cost per analysis (median): ~$0.50–$2.00 against Anthropic Sonnet, $0 against local Ollama.
- Bug-escape during onboarding (subjective): expected to drop because architecture is visible rather than guessed.

**Confidence:** **High**. The pipeline replicates UA's proven approach in Rust; UA has demonstrated the value at ≥1k stars and active enterprise use.

### 2.2 — Logseq vault: page → block fidelity

**Today:** A Logseq page becomes a single graph node. Wikilinks become page-page edges. Block-level structure (parent, left, properties, refs, drawers, scheduled, deadline) is invisible.

**After Phase 1 (week 9):**
- Block-level parse per ADR-068: every bullet becomes a `Block` node with `parent_id`, `left_id`, `path-refs`, properties, task status, scheduled/deadline, repeater.
- Existing 998-file vault: ~30k blocks added as containers under existing 998 page nodes.
- Block-level search returns ≥10× more useful hits than page-level search on the user's reference vault (M-11).

**Quantified value:**
- Search-hit count on `#research` query against the user's vault:
  - Today (page-level): ~24 hits (pages tagged `#research`).
  - Post-PRD-005 (block-level w/ path-refs inheritance): ~280–600 hits (blocks descending from a `#research` ancestor).
  - **~10–25× increase** in retrievable knowledge units, with structure to navigate.
- RAG quality: `clean_text` per block is property-stripped, drawer-stripped, ready for embedding without pollution. Embedding-quality improvement non-trivial; expect noticeable downstream LLM-agent performance gains (subjective).

**Confidence:** **High** for the parser; **Medium** for the downstream RAG quality gains until measured.

### 2.3 — Path-find on large graphs: WASM BFS → GPU APSP

**Today:** No public path-find. Internally, GPU APSP exists (`gpu_landmark_apsp.cu`, `relaxation_step_kernel`) but is not wired to UI.

**After Phase 3 (week 14):**
- Path-find p99 ≤ 8ms on 100k-node graphs (M-17).
- Path-find p99 ≤ 40ms on 1M-node graphs (aspirational, supported by existing kernel).
- Camera animates along path in 3D; visually distinguishable from background edges.

**Quantified value:**
- Original PRD plan (WASM BFS): p99 ≤50ms on 10k nodes; ceiling at ~50k nodes before browser stalls.
- GPU-APSP-backed implementation: ≥6× better p99 on 100k nodes than the WASM plan would deliver, at scale where WASM doesn't run at all.
- This unblocks "big graph" use cases (federated multi-owner) that the original PRD effectively forbade by perf-budget.

**Confidence:** **High**. The GPU APSP kernel already exists, runs in production for our analytics; wiring to UI is well-scoped.

### 2.4 — Persona-graded views: 0 → 3 personas

**Today:** Single uniform view of every graph.

**After Phase 3 (week 16):**
- Three personas (`non-technical | junior | experienced`) hide/show node kinds.
- Persona-graded summaries precomputed at analysis time (one LLM call per node × 3 personas) — zero new LLM calls on persona switch.
- Persona switch updates render within 32ms (M-13 edge-crossing improvement, separate metric).

**Quantified value:**
- Stakeholder-explainability: a non-technical PM can navigate a senior engineer's typed graph and get plain-English summaries; today they would not even open the dashboard.
- Cost: 3× LLM cost at analysis time; mitigated by R-10 (lazy generation if storage tight; precompute trade-off).

**Confidence:** **Medium** — persona switching is mechanically straightforward, but the *quality* of register-graded summaries depends on LLM faithfulness. Quarterly user survey (M-10 stakeholder NPS ≥+30) is the signal.

### 2.5 — Solid-pod federation across `did:nostr`

**Today:** No graph federation. Logseq vaults are user-local.

**After Phase 2 (week 13):**
- Every produced graph published as `urn:agentbox:bead:<owner-hex>:<sha256-12>` in the user's Solid pod.
- Solid ACL grants per-bead read access; recipient's BC20 layer mounts as separate, owner-tagged subgraph.
- Cross-owner edges visualised distinctly (per Q-05 outcome).
- ≥10 federated graph fetches in Phase 2's first month (M-03).

**Quantified value:**
- Multi-machine: same user pulls their own graph from another machine; no manual sync.
- Multi-user (sovereign): peer publishes a small subgraph; user mounts it; richer context without giving up sovereignty.
- Strategic: this is a wedge into the "sovereign-AI" thesis where users own their cognitive substrate. M-15 (≥30% of Databricks-access users adopt ontobricks bridge) is a separate adoption signal.

**Confidence:** **Medium** — pod write/read mechanics are proven (existing infra), but *federation* across `did:nostr` peers is a new social pattern. Adoption depends on at least one peer existing.

### 2.6 — Incremental update: re-analyze every commit → fingerprint-gated

**Today:** No incremental update for code-analysis. Wiki-side: `FORCE_FULL_SYNC=1` is a manual override.

**After Phase 4 (week 19):**
- Cosmetic-only changes (whitespace, comments) consume zero LLM tokens (AC-F.1).
- Structural-only changes re-analyze ≤2 files (AC-F.2).
- Architectural changes re-run layer detection but not full analysis (AC-F.3).

**Quantified value:**
- Estimated LLM cost reduction over a 100-commit replay on a real repo: linear in structural-change count (not in commit count). At median ~30% of commits being structural, **expected ~70% LLM-cost reduction** vs naive every-commit reanalysis.
- Time-to-fresh-graph after a commit: ≤2 seconds for cosmetic; ≤30 seconds for structural; ≤2 minutes for architectural.

**Confidence:** **Medium** — fingerprint stability across tree-sitter grammar updates (R-08) is a known fragility; mitigated by `grammar_version` inclusion.

### 2.7 — Ontobricks federated reasoning: 0 → opt-in

**Today:** VC has OWL constraint kernels (DisjointWith, SubClassOf, SameAs, InverseOf, Functional). No SHACL, no SWRL, no domain-registry materialization.

**After Phase 3 (week 17), opt-in users only:**
- OWL 2 RL forward-chaining via ontobricks → inferred triples in quarantine.
- SWRL Horn-clause rules compiled to SQL/Cypher → conditional inferences.
- SHACL shapes → render-time visual annotations (red badges) on offending nodes.
- R2RML mapping → import Databricks-table-derived triples into the same graph.

**Quantified value:**
- For a user with Databricks access: a reasoning surface 5–10× richer than VC's standalone OWL kernels.
- For users without Databricks access: zero benefit (gated by config; no degradation either).

**Confidence:** **Lower (opt-in)** — the *capability* is high-confidence; *adoption* is conditional on the small subset of users who have Databricks access AND want to combine it with VC.

### 2.8 — 3D ontology constraint visualisation lit up

**Today:** OWL constraint kernels exist; visualization only displays the spring-style force result (nodes attract/repel) without any user-facing badge or constraint inspector.

**After Phase 3 (week 17):**
- Per-axiom constraint visualisation: hover any node to see its incoming OWL axioms.
- SHACL violation badges (red) on offending nodes.
- Inferred-triple quarantine pane lets user accept/reject per axiom.

**Confidence:** **High** for the kernel side; **Medium** for the UX. The kernels deliver constraint forces today; surfacing them as user-comprehensible affordances is a UX project.

---

## 3. Strategic positioning gains

### S1 — Sovereign cognition

PRD-005 closes the loop: every cognitive artifact (graph, inference, persona view) has a sovereign owner via `did:nostr`, a stable URN, a signed bead, a Solid-pod home, and a federated readability path. This is a unique combination in 2026's market — graph databases lack identity; identity systems lack graph cognition; few have GPU rendering at all.

The PRD does not invent these primitives; it consolidates them into a single user-facing capability ("vc analyze and federate"). The strategic value is *reduced friction* — what was a research project becomes a usable tool.

### S2 — Federated reasoning pattern (MCP-symmetric)

VC already exposes an MCP server. Ontobricks exposes its own (`mcp-ontobricks`). Both store data in pods (or pod-equivalents). Both speak typed cognitive content over a JSON-RPC-style transport. The PRD-005 implementation makes this symmetry concrete: any future reasoning service that exposes MCP can federate similarly.

This positions VC as a node in a *cognition mesh*, not a monolith. The pattern is testable in PRD-005's first ontobricks integration and applicable to future integrations (BlazeGraph, Stardog, Apache Jena Fuseki, etc.) without architectural change.

### S3 — Block-level RAG-ready substrate

Most knowledge-management RAG systems chunk pages by character/token windows. Logseq's value is that it's *already* structured at block granularity with parent-child semantics. By preserving that granularity (ADR-068), VC produces a substrate that LLM agents can navigate as if it were a filesystem (Karpathy's "LLM OS" framing).

This is non-trivially valuable for the user's own future agentic workflows: agents querying the typed graph via MCP get block-grade hits with provenance, not chunked-prose-grade noise.

---

## 4. Costs

### 4.1 — Engineering effort

19 weeks across 4 phases, ~6 dev-weeks per phase. Single-developer (the user) implementation pace; extends ~5 months. Buffers for QE-mandated additions and Phase 3's late-stage scope (ontobricks, 2D fallback) baked into the 19-week estimate.

### 4.2 — Compute / LLM cost

| Workload | One-time | Per-analysis | Notes |
|---------|---------|-------------|-------|
| Persona-graded summaries | $1–$2 per analysis × 3 personas | $0.10–$0.30 incremental | Lower cost on Ollama-local |
| Tree-sitter extraction | $0 | $0 | CPU-bound, no LLM |
| LLM enrichment (entities, claims) | — | $0.50–$2.00 | Cap per session |
| Ontobricks reasoning (opt-in) | — | Variable, Databricks DBU | Gated by per-session $ budget |
| Pod write | — | <$0.01 | Local-pod default |

Token-budget kill-switch caps single-session cost at user-configured ceiling.

### 4.3 — Storage cost

| Store | Today | Post-PRD-005 | Delta |
|------|------|--------------|-------|
| Neo4j (graph) | ~3 GB user vault | +30k blocks ≈ +30 MB / vault | +1% |
| AgentDB (analysis sessions) | minimal | ~50 MB per active session | trivial |
| Solid pod (beads) | minimal | ~5 MB / page-graph or ~150 MB / block-graph | new line item |
| RuVector PG (HNSW) | per existing config | +30k embeddings × 384-d × 4 B = +46 MB / vault | +1% |

Pod storage is the dominant new cost; user-controlled.

### 4.4 — Operational complexity

PRD-005 adds:
- 6 new Rust crates.
- 7 new ADRs.
- 1 new DDD context map.
- 6 incoming Epics with feature flags.
- 7 new actors in the supervision tree.
- ≥30 new acceptance criteria, ≥30 risk-register entries.

This raises the operational surface materially. The QE review's recommendation to split out **PRD-005a (Federation Hardening), PRD-005b (Operations Runbooks), PRD-005c (Schema Versioning)** as sibling PRDs absorbs some of this complexity by deferring it.

### 4.5 — Risk-adjusted cost

The QE fleet identified 30+ risks across 4 reviewers. Of these, **R-19 (NaN broadcast), R-23 (force explosion), R-22 (URN forgery), R-28 (LLM secret leak)** are critical — partial mitigations live in ADR-066/067/069/070; full closure requires implementation.

Realistic scenario: ~15% of effort goes to QE-mandated additions and risk closure; baked into the 19-week estimate.

---

## 5. Quantified outcomes by phase

### Phase 0 (weeks 1–4): Foundations

**Deliverables:**
- Typed schema lands in Neo4j.
- Existing 998-file vault re-labelled with new kinds.
- 5-language extractors landed.

**Measurable benefit:**
- M-04 (crash budget) baseline established.
- AC-A.1 through AC-A.4 green.

**User-perceived value:** Low — internal infrastructure work. Existing graph still renders identically.

**Risk:** Migration corruption (R-06). Mitigated by dry-run + snapshot.

### Phase 1 (weeks 5–9): Code analyzer + Logseq blocks

**Deliverables:**
- `vc analyze <repo>` works end-to-end.
- Block-level Logseq parser produces ~30× node count for user's vault.

**Measurable benefit:**
- M-01 (≥50% internal users run `vc analyze` first month) — meaningful for the user's own workflow.
- M-11 (block-level search ≥10× hits on `#research`) — directly observable.

**User-perceived value:** **High** — first new capability shippable end-to-end. Foreign repo navigation works; vault search dramatically richer.

**Risk:** R-15 (block explosion vs GPU/pod budget). Mitigated by ADR-068 D3 caps + ADR-069 D7 stability tuning.

### Phase 2 (weeks 10–13): Pod federation + Force presets

**Deliverables:**
- Pod publish + federate working.
- 5 force presets land with calibration.

**Measurable benefit:**
- M-03 (≥10 federated fetches first month) — adoption signal.
- M-14 (edge-crossing improvement ≥30% on `logseq_large` preset) — visual quality.

**User-perceived value:** **Medium-High** — federation is novel; force presets visually-noticeable.

**Risk:** R-23 (force explosion from matryca constants). Mitigated by ADR-069 D3 calibration.

### Phase 3 (weeks 14–17): Dashboard + Ontobricks

**Deliverables:**
- 6 dashboard features (PathFinder, TourMode, DiffOverlay, PersonaSelector, EdgeCategoryFilter, CodeViewer) in 3D + 2D.
- Ontobricks bridge (opt-in).

**Measurable benefit:**
- M-09 (time-to-first-tour ≤6 min) — onboarding.
- M-15 (≥30% Databricks-access users adopt bridge) — opt-in adoption.
- M-17 (path-find p99 ≤8ms on 100k) — performance.

**User-perceived value:** **High** — most dashboard polish lands here.

**Risk:** R-19 (NaN broadcast), R-25 (stale ontology), R-27 (inference poisoning). Mitigated by ADR-066/067/070.

### Phase 4 (weeks 18–19): Incremental + polish

**Deliverables:**
- Post-commit hook + change classifier.
- Production rollout.

**Measurable benefit:**
- M-08 (token spend within ±20% of projection) — cost control.
- ~70% LLM-cost reduction on commit-driven re-analysis (vs naive every-commit) — projected.

**User-perceived value:** **Medium** — invisible-but-important; reduces ongoing operating cost.

**Risk:** R-08 (fingerprint instability). Mitigated by `grammar_version` salt.

---

## 6. Sensitivity analysis

| Scenario | Effect on benefits |
|---------|-------------------|
| Phase 1 slips by 2 weeks | M-01 first-month adoption window narrows; downstream phases compress. **Acceptable.** |
| LLM cost doubles due to provider price changes | M-08 ±20% projection breached; pivot to Ollama-local default. **Mitigation in PRD §3.1 default.** |
| Block-level explosion produces 100k+ nodes on user's vault | Pod write SLA breached; hard cap activates; user runs in `--allow-large-vault` mode at degraded perf. **Pre-mitigated by ADR-068 D3.** |
| Ontobricks adoption is zero | Phase 3 deliverable G is wasted. Acceptable: G is opt-in and parallel-Phase, not critical-path. **Strategic loss.** |
| Federated peer count stays at 0 | Phase 2 pod-federation deliverable has no consumers. Acceptable: federation is enabled but unused. **Strategic loss.** |
| GPU-NaN guard overhead exceeds 2% budget | ADR-070 D1.2 needs profile-and-tune; may relax frequency from every-32-iters to every-128-iters. **Minor.** |
| Migration corrupts 1.17M memory entries | **Catastrophic.** Mitigation: pre-migration snapshot + rehearsed rollback runbook. PRD §F-DoD requires rehearsed rollback. **Accepted as residual risk.** |

---

## 7. Comparable alternatives — what if we did less?

| Alternative | Effort | Benefits captured | Benefits lost |
|------------|-------|------------------|---------------|
| **Schema only** (Epic A): just adopt UA's typed taxonomy in Neo4j. | 4 weeks | Richer queries, edge category filter | All capability gains 2–8 lost |
| **Code analyzer only** (Epics A+B+C): no Logseq block upgrade, no federation, no ontobricks. | 9 weeks | Capability gains 1, 6 | Logseq fidelity (2), federation (5), ontobricks (7), persona (4) lost |
| **Block parser only** (Epics A+H): better Logseq fidelity. | 5 weeks | Capability gain 2 | Code analysis (1), federation (5) lost |
| **Full PRD-005** | 19 weeks | All 8 + 3 strategic gains | — |

**Recommendation:** the *full* PRD is justifiable only if the user values capability gains 1, 2, AND at least one of (5, 4, 7). For a user with no Databricks access who cares only about local code-cognition, a reduced scope ending at Phase 1 captures ~80% of the personally-relevant benefit at ~50% of the effort. For a user who values sovereignty + federation + multi-machine workflows, the full scope is justified.

---

## 8. Confidence breakdown

| Estimate | Confidence | Basis |
|---------|-----------|-------|
| 19-week schedule | Medium | Single-developer pace assumption; +/- 4 weeks plausible |
| 50× faster repo orientation | Medium-High | Subjective comparison vs. Grep/file-reading; UA's existing user reports support |
| 10× block-level search hits on `#research` | High | Mechanical: path-refs inheritance is deterministic on user's vault structure |
| Path-find p99 ≤8ms on 100k nodes | High | GPU APSP kernel already runs at this scale |
| 70% LLM-cost reduction on incremental | Medium | Depends on commit-pattern (cosmetic vs structural ratio); user-specific |
| Pod federation adoption ≥10/month | Lower | Dependent on at least one peer existing; uncertain |
| Stakeholder NPS ≥+30 | Lower | Subjective; depends on UX polish in Phase 3 |
| ≥99% UUID parity with matryca | Medium-High | Mechanical fixture-driven test |
| 0 NaN escapes after ADR-070 D1.2 lands | High | Mechanical guarantee from per-iter scan |

---

## 9. Net assessment

**For the user (sovereign individual operator):**

- **Personal-workflow value:** **High.** Code-cognition (G1) and Logseq fidelity (G2) directly improve the user's daily friction. Path-find (G3) and persona (G4) are quality-of-life improvements once the substrate is in place.
- **Strategic value:** **High.** S1–S3 position VC as more than a renderer — a sovereign cognition substrate. This unlocks future PRDs (multi-user collaboration, agentic workflows, persistent memory layer for AI agents).
- **Execution risk:** **Medium.** The QE fleet identified 30+ risks; mitigations are in ADRs but require disciplined implementation. The user is the entire engineering org; estimation slips are likely. Phase 4 polish gates are appropriate.
- **Cost-benefit:** **Strongly positive** if the user values strategic gains S1–S3 in addition to operational gains G1–G6. Strongly positive at reduced scope (Phase 1 only) if the user values G1 + G2 alone.

**Recommendation:** Proceed. Start at Phase 0 after resolving the four P0 open questions (Q-09 RuVector vs AgentDB, Q-02 standalone pod, Q-12 block-level default, Q-15 federation preset). Expect Phase 1 to deliver disproportionate value; Phases 2–4 are completion-of-substrate.

---

## 10. Revision protocol

This document is updated:

- **At the end of Phase 1 (week 9):** measure G1, G2 actuals against projections; update confidence.
- **At the end of Phase 2 (week 13):** measure G3, G5; update.
- **At the end of Phase 3 (week 17):** measure G4, G7; update.
- **At the end of Phase 4 (week 19):** measure G6, S1–S3; final retrospective.

Each revision appends a §11.N retro subsection: "what beat projection", "what missed", "what to revise in PRD-005a/b/c follow-ups".
