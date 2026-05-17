---
title: "PRD: Insight Migration Loop (MVP)"
description: Scopes the first shippable version of VisionClaw's tacit-to-explicit knowledge migration loop — Logseq KG notes promoted to OWL ontology classes through a physics-visible broker review, with GitHub-backed provenance and rollback.
date: 2026-04-18
status: draft
audience: project owner, engineering, pilot customers
owner: VisionClaw core
companion-adrs:
  - adr/ADR-049-insight-migration-broker-workflow.md
ddd-context: explanation/ddd-insight-migration-context.md
relates-to:
  - docs/design/2026-04-18-insight-migration-loop/01-prior-art.md
  - docs/design/2026-04-18-insight-migration-loop/02-bridge-theory.md
  - docs/design/2026-04-18-unified-knowledge-pipeline.md
  - docs/architecture-self-review.md
  - docs/explanation/ddd-enterprise-contexts.md
  - docs/audits/2026-04-18-logseq-ontology-audit/00-master.md
---

# PRD: Insight Migration Loop (MVP)

## 1. Problem statement

Organisations that think for a living accumulate their real intellectual capital in notes, not in systems. The taxonomy that would make that capital reusable almost never gets built — the people who know what the terms mean are too busy using them to formalise them.

**Rosa, research lead at a 30-person AI policy institute.** She maintains 2,000+ Logseq pages across half a dozen threads. Her juniors keep asking "do we already have a position on X?" and she keeps answering from memory. Onboarding a new analyst costs six weeks of re-discovering the frame; every "concept map" attempt has been stale within a month.

**Idris, principal at a boutique digital-transformation consultancy.** His team produces 20 client deliverables a year in bespoke language that partly reuses firm IP. There is no retrieval across engagements — when a client asks "have you seen this before?" he answers on vibes. A failed taxonomy project in 2024 produced a spreadsheet nobody opens.

**Chen, regulated-industry SME at a medical-device manufacturer.** She writes design-history evidence against ISO 14971 and IEC 62304; auditors require every clinical-risk claim to trace to a controlled vocabulary term. Her team's vocabulary lives in a stale SharePoint glossary and the heads of four senior engineers. Each audit costs a fortnight of retrospective reconciliation.

All three fail the same way: **tacit structure exists in their notes, but formalising it as a project is too expensive**, and no instrument makes the translation continuous, visible, and low-ceremony.

## 2. Goal

**Make ontology promotion a two-minute broker gesture grounded in a live physics view of fit, so governed vocabulary grows as a by-product of normal note-taking.**

Secondary objectives, measurable:

1. **Time-to-promote ≤ 2 minutes median** from broker opening a candidate to PR opened (measured in telemetry).
2. **≥ 80% of promoted terms still present in the ontology 90 days later** (no silent rollbacks / churn).
3. **≥ 50% of proposed candidates surfaced by the system are eventually promoted** (the system proposes well, not noisily).
4. **Zero ontology-layer silent failures**: every promotion produces a visible PR, a traceable provenance chain, and a rollback path.
5. **Broker cognitive load ≤ 10 minutes/week** to clear the candidate queue at a 2,000-page corpus scale.

## 3. Non-goals (explicit cuts)

- **Not a Notion replacement.** No block editor, no WYSIWYG, no rich-text note CRUD. Logseq stays authoritative for authorship.
- **Not Palantir.** No bespoke data-integration connectors, no ontology-driven ETL, no analytical application surface. The ontology serves discovery and governance, not operations.
- **Not a chatbot.** Candidates surface through structural signal, not NL querying. Agent tools propose — they do not answer.
- **Not an Obsidian plugin.** Logseq is the only supported editor for MVP; integration happens via GitHub sync.
- **Not multi-broker consensus.** One broker decides per candidate (voting, quorum, delegation deferred to v2).
- **Not federated.** Single-tenant, single-repository.
- **Not graph-database-agnostic.** Neo4j + OWL only.

## 4. Users and permissions

Roles map onto the existing `EnterpriseRole` enum (`Broker`, `Admin`, `Auditor`, `Contributor`).

| Role | Can view | Can do |
|---|---|---|
| **Contributor** (Rosa's analysts, Idris's consultants) | KG notes they authored + public KG + current ontology | Author Logseq pages (`public:: true`). See a candidate only if they authored its source. Cannot approve. |
| **Broker** (Rosa, Idris, Chen) | Full KG, full ontology, full candidate inbox, full provenance, physics view | Approve / defer / reject candidates. Trigger PRs. Revert promotions. Tune scoring thresholds. |
| **Admin** | Everything a Broker sees + user management + domain registry (`config/domains.yaml`) | Assign roles. Configure candidate pipeline. Configure GitHub PR target. Cannot approve (separation of duties). |
| **Auditor** | Read-only: full candidate log, full decision log, provenance graph, PR history | Cannot author, approve, or configure. Can export audit reports. |
| **Observer** (optional) | Public KG + promoted ontology, no candidate inbox | Purely read. For pilot stakeholders who want to watch the instrument without operating it. |

Permissions are enforced at API boundary. Role resolution comes from BC14 (Enterprise Identity); authorisation from BC17 (Policy Engine). Escalations go to BC11 (Judgment Broker inbox), where this whole feature lives.

## 5. Capability catalogue (MVP surface)

### 5.1 Split graph view (KG-vs-ontology physics layout)

- **Story**: "As a broker I see the KG and the ontology as one graph, visually distinguished, laid out by physics so fit is spatial."
- **Signal**: Two-population rendering — KG nodes (`public:: true`) and ontology nodes (`OwlClass` with `public-access:: true`). Inter-population edges render as bridges. Camera defaults to whole-corpus view; population toggle hides either layer.
- **Out of scope**: VR/XR mode (existing), time-slider over ontology history, ontology-only focus mode.

### 5.2 Broker candidate inbox

- **Story**: "As a broker I can open a queue of auto-generated promotion candidates ranked by readiness."
- **Signal**: List view showing candidate term, source KG page, score (section 6), top three neighbours, agent rationale. Sorts by score descending. Clicking opens a Decision Canvas with the physics view centred on that candidate.
- **Out of scope**: Multi-broker assignment, SLA timers, free-text search.

### 5.3 Approve / defer / reject

- **Story**: "As a broker I take one of three actions per candidate and the system handles the rest."
- **Signal**:
  - **Approve**: opens a GitHub PR amending the ontology TTL plus the source Logseq page (adds `owl:class::` if absent); state → `PRAssigned`; notifies on merge.
  - **Defer**: state → `Deferred` with optional reason; returns to inbox after configurable cooldown (default 14 days).
  - **Reject**: state → `Rejected` with required reason; never re-surfaced unless the source page's score changes materially.
- **Out of scope**: Draft-editing the OWL block before PR (edit happens in the PR itself); bulk approve.

### 5.4 Provenance chain

- **Story**: "As an auditor I can trace any promoted term back to the exact note, decision, PR, and commit that introduced it."
- **Signal**: Click a promoted class → source page + permalink, candidate score at promotion, broker identity, decision timestamp, PR URL, merge commit SHA, re-ingest timestamp — rendered as a navigable vertical timeline.
- **Out of scope**: Cryptographic signing (uses BC1 bead provenance if available; otherwise v2); W3C PROV-O export.

### 5.5 Revert (rollback)

- **Story**: "As a broker I can undo a promotion and have the ontology return to its prior state with an audit-visible reason."
- **Signal**: A **Revert** action on any promoted class opens a revert PR removing the axioms introduced by the original PR. On merge, the candidate enters state `Rollback` with the reason attached. The source page is left unchanged.
- **Out of scope**: Partial revert; cascade revert of dependent axioms.

### 5.6 Agent `ontology_propose` integration

- **Story**: "As a broker I want agents to actively nominate candidates, not only see what scoring surfaces structurally."
- **Signal**: Agents call `ontology_propose` (`src/handlers/ontology_agent_handler.rs`). Each call produces a candidate with `source: AgentProposal`, `agent_rationale`, and `agent_confidence` as a scoring input. Agent candidates merge into the same inbox, provenance tagged.
- **Out of scope**: Autonomous agent promotion without broker review — explicitly forbidden.

### 5.7 Orphan KG note identification

- **Story**: "As a broker I can see KG notes with zero formal grounding — no `owl:class::`, no `subclass-of`, no edges to any OWL class."
- **Signal**: Graph renders orphans with a dimmed ring marker. An **Orphans** inbox tab lists them ranked by in-degree. "Propose grounding" generates a draft candidate.
- **Out of scope**: Auto-grounding without broker review.

### 5.8 Ungrounded hot-spot detector

- **Story**: "As a broker I want tacit concepts — notes the team links to heavily but which lack a formal anchor — surfaced automatically."
- **Signal**: Heuristic `(in-degree > 10) ∧ (out-degree > 5) ∧ (no owl:class::) ∧ (mentioned in ≥ 3 source files)`. Rendered as haloed "hot spots" in the graph and listed in a dedicated inbox tab by composite density.
- **Out of scope**: Hot-spot detection across deleted pages.

### 5.9 Stale frontier detection

- **Story**: "As a broker I want to know which highly-central ontology terms have aged without review."
- **Signal**: Per class, `PageRank × days_since_last_update`; top N (default 10) surface in a **Stale Frontier** tab.
- **Out of scope**: Auto-refresh or agent-written updates.

### 5.10 Physics re-settle after merge

- **Story**: "As a broker I see the corpus physically re-balance after a promotion."
- **Signal**: PR merge → re-ingest → ontology forces update → 3–5 s re-settle animation. Toast announces completion with a before/after centrality delta.
- **Out of scope**: Per-frame delta comparison; undo via physics gesture.

## 6. Candidate scoring algorithm

Candidates carry a scalar confidence score in `[0, 1]` computed by the discovery engine (BC13) from eight signals: WikilinkToOntology (S1), SemanticCooccurrence (S2), ExplicitOwlDeclaration (S3), AgentProposal (S4), MaturityMarker (S5), CentralityInKG/PageRank (S6), AuthoringRecency (S7), and AuthorityScore (S8). Each signal is normalised before combining; the linear weighted sum is passed through a sigmoid (`sigmoid(12·(raw − 0.42))`) to produce the final confidence value. The surface threshold is **≥ 0.60**; the high-priority badge fires at **≥ 0.75**; the dismissed floor is **< 0.35** (below this a candidate is never re-surfaced, preventing broker fatigue). Agent confidence contributes as one signal (S4, weight 0.20) and does not bypass the threshold regardless of its value (D2). Weights and thresholds live in `config/insight-migration.yaml`, static per deployment in MVP; per-tenant learning is v2.

Canonical formula and worked examples: see [05-candidate-scoring.md](design/2026-04-18-insight-migration-loop/05-candidate-scoring.md). This PRD's earlier formula has been superseded per master doc §11 D2.

## 7. Promotion lifecycle state machine

```mermaid
stateDiagram-v2
    [*] --> Candidate : Scoring sweep >= 0.60 or ontology_propose call

    Candidate --> UnderReview : Broker opens (dwell > 5s)
    Candidate --> [*] : score < 0.35 (dropped, never re-surfaced)

    UnderReview --> Approved : Broker clicks Approve
    UnderReview --> Deferred : Broker clicks Defer
    UnderReview --> Rejected : Broker clicks Reject + reason
    Deferred --> Candidate : After cooldown re-surface

    Approved --> PRAssigned : PR opens automatically

    PRAssigned --> PRMerged : GitHub PR merge webhook
    PRAssigned --> PRClosed : PR closed without merge
    PRClosed --> Candidate : Returns on next scan

    PRMerged --> Promoted : Re-ingest completes, OWL class in graph

    Promoted --> Deprecated : Rollback BrokerCase approved + revert PR merged + re-ingest
```

Revocation (rollback of a Promoted class) is handled as a new BrokerCase with `category: migration_rollback` — see ADR-049 §Lifecycle. It is NOT a rewind of the original state machine.

Transition rules:

| From | To | Trigger | Side-effects |
|---|---|---|---|
| (none) | Candidate | Scoring sweep ≥ 0.60 or `ontology_propose` call | Inbox entry created; candidate record persisted |
| Candidate | UnderReview | Broker opens the candidate (any dwell > 5 s) | Telemetry event; lock held for 30 min to prevent double-work |
| UnderReview | Approved | Broker clicks **Approve** | PR opens via GitHub App; candidate state = Approved; PR URL stored |
| UnderReview | Deferred | Broker clicks **Defer** | Re-surfaces after per-role cooldown (D3); reason stored |
| UnderReview | Rejected | Broker clicks **Reject** + reason | Candidate permanently closed for this page version |
| Approved | PRAssigned | PR webhook confirms PR opened | State machine advances; waits on PR |
| PRAssigned | PRMerged | GitHub PR merge webhook | Triggers re-ingest job |
| PRAssigned | PRClosed | GitHub PR closed without merge | Candidate returns to Candidate state, eligible for re-surface |
| PRMerged | Promoted | Re-ingest completes + OWL class appears in graph | Provenance record finalised; physics re-settles |
| Promoted | Deprecated | Rollback BrokerCase approved + revert PR merged + re-ingest completes | OWL class removed; provenance record closed with rollback reason |

Each transition emits a domain event consumed by BC15 (KPI Observability).

## 8. Success metrics (mesh KPIs extension)

Existing README KPIs remain (Mesh Velocity, Augmentation Ratio, Trust Variance, HITL Precision). MVP adds:

| KPI | Definition | MVP target |
|---|---|---:|
| **Promotion latency** | `merged_at − candidate_detected_at`, p50 across last 30 days | ≤ 48 h |
| **Broker inbox clearance** | (candidates closed / candidates entered) over trailing 7 days | ≥ 80% |
| **Rollback rate** | promotions reverted within 90 days ÷ promotions in window | ≤ 10% |
| **Ontology coherence** | `unsatisfiable_classes / total_classes` (from Whelk reasoner) | ≤ 1% |
| **Candidate surface precision** | promoted ÷ (promoted + rejected) over last 90 days | ≥ 50% |
| **Orphan coverage** | fraction of orphan KG notes for which a candidate has been surfaced at least once | ≥ 70% after 30 days |
| **Rate-limited overflow** | Count of candidates suppressed by 10/broker/24h rate limit | < 20/week steady state |

All metrics computed by BC15 from domain events. Dashboard surfaces them on the enterprise homepage with 30-/7-/1-day trend deltas.

## 9. MVP exit criteria

Ships to first pilot when all hold:

1. Split physics view renders full mainKnowledgeGraph (2,800+ pages + 2,200+ OWL classes) at ≥ 30 fps on reference hardware.
2. ≥ 20 candidates surface from the existing corpus at default thresholds without manual priming.
3. End-to-end round-trip (Candidate → Approve → PR → Merge → Promoted → visible in graph) completes in ≤ 5 min wall-clock, re-settle included.
4. Revert round-trip completes in the same 5-min envelope.
5. Provenance chain renders all seven steps, every link navigable.
6. Zero silent failures: every rejection or skip emits a structured log line surfaced in the Corpus Health panel.
7. RBAC enforced: contributor cannot see inboxes; auditor and admin cannot Approve.
8. KPI dashboard shows all six new KPIs with real pilot data.

## 10. Beyond MVP — v2 roadmap

Explicitly deferred:

- **Federation**: multi-repo, multi-tenant, cross-organisation ontology sharing.
- **Multi-broker consensus**: voting, quorum, delegation chains.
- **Ontology diff visualisation**: temporal graph, scrub slider, delta highlighting.
- **Ontology forking**: sandbox branches for what-if exploration.
- **Multi-language terms**: locale-scoped `rdfs:label` and alias resolution.
- **Bulk approve / reject**: one action, N candidates, one PR.
- **Learned scoring weights**: broker actions tune per-tenant weights online or in batch.
- **Auto-grounding agent**: autonomous proposal *and* PR generation under policy.
- **Cryptographic provenance**: broker-signed decisions, verifiable chain.

## 11. Risks and mitigations

| # | Risk | Mitigation |
|---|---|---|
| R1 | **Identity mismatch**: KG page IRI ≠ OWL class IRI produces ghost promotions (PR merges but node never links) | Enforce canonical IRI scheme (D1); fail re-ingest on drift; block promotion on ad-hoc IRI |
| R2 | **Broker fatigue** from noisy inbox | Surface ≥ 0.55, dismissed floor 0.35, weekly "top 5" digest; ≤ 10 min/week target |
| R3 | **Concept drift** post-promotion — term meaning diverges from OWL class | Stale frontier surfaces aging high-centrality classes; auditor report flags edits to promoted classes |
| R4 | **Ontology churn**: frequent promote-revert cycles | Rollback KPI ≤ 10%; deferral encourages waiting; Whelk runs on each PR and blocks merge on new unsatisfiable classes |
| R5 | **Bad promotion** passes review but violates domain policy (Chen's case especially) | Policy Engine (BC17) runs on every PR; auditor has read-only veto via decision log |
| R6 | **Agent hallucination** in `ontology_propose` | `agent_confidence ≥ 0.7` to surface; distinct provenance tag; agent rationale shown italic; precision tracked separately |
| R7 | **Scale** at 10k+ pages breaks scoring/physics/inbox | Incremental re-scoring on touched pages only; GPU physics (existing); inbox paginated (50) |
| R8 | **Permission escalation**: contributor acquires broker actions via API abuse | All transitions mediated by BC17 policy checks; RBAC tests in CI; auditor log catches misattributed actions |
| R9 | **GitHub outage** blocks promotion | PRs queued; exponential backoff; UI shows reconnect banner; fall-back read-only mode |
| R10 | **Ingestion regression** (the 33% cliff documented in the audit) | Corpus Health panel green as hard pre-promotion gate; unified pipeline D1–D5 is a prerequisite |

## 12. Open questions for owner review

1. **Scoring weight scope** — MVP ships global defaults; is per-tenant configurability required before pilot?
2. ~~**Agent gating** — should `agent_confidence ≥ 0.9` bypass the 0.55 surface threshold, or share the same floor as structural candidates?~~ **RESOLVED (D2)**: No bypass. Agent confidence contributes through S4 (weight 0.20); the sigmoid decides. Threshold is 0.60.
3. ~~**Defer cooldown** — is 14 days right for regulated pace (Chen) and research pace (Rosa), or should it be per-role?~~ **RESOLVED (D3)**: Per-role cooldowns via ADR-045 policy engine. Defaults: Broker 72 h, Admin 24 h, Auditor 168 h.
4. ~~**Rollback of depended-on classes** — if a reverted class has subclasses or inbound references, MVP blocks the revert. Acceptable or should we cascade?~~ **RESOLVED (D4)**: Cascade via one compensating PR covering all dependents. Orphaning dependents is forbidden; `reversible: true` is required on the original promotion.
5. ~~**PR reviewer policy** — does in-app broker approval self-merge, or require a separate GitHub review? MVP proposes self-merge with audit trail; regulated tenants may require a second reviewer.~~ **RESOLVED (D5)**: Schema-configurable per deployment (`mode: self_merge` or `mode: second_reviewer`) in `.visionclaw/broker-policy.yaml`. Policy engine (ADR-045) enforces at merge-time. Agents never satisfy the second-reviewer role.
6. **Self-approval** — can a broker approve a candidate sourced from a page they authored? MVP says yes (researchers typically author their own concepts); an opt-in "no self-approval" policy flag is trivial.
7. **Stub vs orphan** — orphan detection will surface stubs already scheduled for deletion. Hide pages with `status:: stub`, or surface with a distinct marker?
8. **Re-ingest affordance** — the merge → re-ingest gap is 30–90 s on a 3k-page corpus. Spinner, toast, or the graph itself animating the new axiom coming into existence?
9. ~~**Pilot choice** — Rosa's corpus is the best-characterised (audit baseline), Chen's has the sharpest compliance pull, Idris's is the most commercially persuasive. Which first?~~ **RESOLVED (D7)**: Idris (consultancy principal) is the pilot persona. Rosa and Chen follow in v2.
10. ~~**Pipeline dependency** — D1–D5 in `docs/design/2026-04-18-unified-knowledge-pipeline.md` (canonical IRIs, frontmatter schema v2, five-stage pipeline, every-page-is-an-OWL-class) are prerequisites for R1, R10, 5.7, and 5.8. Accept the dependency, or take a simpler bridge path against the current pipeline as-is?~~ **RESOLVED (D8)**: Bridge as-is. The Insight Migration Loop does not gate on Unified Pipeline decisions. Sprint T1 (audit A1 — MetadataStore fix) is the only hard prerequisite.
