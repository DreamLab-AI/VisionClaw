# PRD: VisionClaw Agentic Mesh Integration & Completion

**Status:** Proposed
**Priority:** P0 strategic, P1 platform
**Document purpose:** Turn VisionClaw from a strong semantic/agentic substrate into a complete, enterprise-usable agentic mesh control plane aligned with the organisational model in [`presentation/the-coordination-collapse.md`](the-coordination-collapse.md).
**Primary gap statement:** VisionClaw is already strong at ontology, orchestration, provenance, and visualisation, but incomplete at the human operating layer, the broker workflow layer, enterprise identity, and organisational KPI instrumentation.

## 1. Executive Summary

VisionClaw should be developed into the first production-ready platform for a **governed agentic organisation**.

Today, the platform already proves several of the thesis’s hardest technical claims:

- ontology-grounded orchestration,
- formal reasoning-backed knowledge structures,
- graph-native visibility of agent activity,
- provenance-rich event capture,
- signed identity and user-owned memory,
- real-time spatial visualisation of coordination state.

However, the current system is still missing the key layers required to operationalise the thesis in real enterprises:

- a **Judgment Broker Workbench**,
- a true **Insight Ingestion Loop**,
- the four proposed organisational KPIs,
- enterprise identity and policy integration,
- workflow discovery connectors,
- platform coherence across node typing, binary protocol, settings, and position flow.

This PRD defines the work required to close that gap.

### Strategic outcome

At the end of this initiative, VisionClaw will support a complete operating loop:

1. discover hidden workflows and decision patterns,
2. codify them into reusable structured workflows,
3. route ambiguous or high-risk cases to human judgment brokers,
4. enforce policy and provenance continuously,
5. measure organisational learning speed and trust quality,
6. propagate validated patterns across the organisation.

### Product outcome

VisionClaw will evolve from a **semantic graph platform with agent features** into a **governed agentic coordination platform** for knowledge-dense, compliance-sensitive organisations.

---

## 2. Source Basis for This PRD

This PRD is grounded in the organisational model and current platform reality documented in:

- [`presentation/the-coordination-collapse.md`](the-coordination-collapse.md)
- [`presentation/google-analysis.md`](google-analysis.md)
- [`presentation/report/chapters/08-new-kpis.tex`](report/chapters/08-new-kpis.tex)
- [`presentation/report/chapters/09-governance.tex`](report/chapters/09-governance.tex)
- [`presentation/report/chapters/13-technical-substrate.tex`](report/chapters/13-technical-substrate.tex)
- [`presentation/report/chapters/14-implementation.tex`](report/chapters/14-implementation.tex)
- [`presentation/report/chapters/15-open-questions.tex`](report/chapters/15-open-questions.tex)
- [`docs/README.md`](../docs/README.md)
- [`docs/KNOWN_ISSUES.md`](../docs/KNOWN_ISSUES.md)
- [`docs/how-to/agent-orchestration.md`](../docs/how-to/agent-orchestration.md)
- [`docs/explanation/ontology-pipeline.md`](../docs/explanation/ontology-pipeline.md)
- [`docs/explanation/security-model.md`](../docs/explanation/security-model.md)
- [`docs/PRD-001-pipeline-alignment.md`](../docs/PRD-001-pipeline-alignment.md)
- [`docs/adr/ADR-036-node-type-consolidation.md`](../docs/adr/ADR-036-node-type-consolidation.md)
- [`docs/adr/ADR-037-binary-protocol-consolidation.md`](../docs/adr/ADR-037-binary-protocol-consolidation.md)
- [`docs/adr/ADR-038-position-flow-consolidation.md`](../docs/adr/ADR-038-position-flow-consolidation.md)
- [`docs/adr/ADR-039-settings-consolidation.md`](../docs/adr/ADR-039-settings-consolidation.md)
- [`docs/adr/ADR-027-pod-backed-graph-views.md`](../docs/adr/ADR-027-pod-backed-graph-views.md)
- [`docs/adr/ADR-029-type-index-discovery.md`](../docs/adr/ADR-029-type-index-discovery.md)
- [`docs/adr/ADR-030-agent-memory-pods.md`](../docs/adr/ADR-030-agent-memory-pods.md)
- [`docs/adr/ADR-034-needle-bead-provenance.md`](../docs/adr/ADR-034-needle-bead-provenance.md)

---

## 3. Problem Statement

VisionClaw currently has a **thesis-product gap**.

### 3.1 What is already true

VisionClaw already provides:

- an ontology-backed semantic substrate,
- a graph-native coordination model,
- real-time visualisation,
- provenance infrastructure,
- decentralised identity and user-owned data patterns,
- multi-agent orchestration surfaces.

### 3.2 What is not yet true

VisionClaw does not yet provide:

- a first-class human interface for the judgment broker role,
- a productised discovery-to-codification workflow engine,
- enterprise-ready identity and access integration,
- a measurable compounding loop,
- unified internal platform coherence across key runtime paths,
- deployment evidence in regulated or non-technical enterprise contexts.

### 3.3 Why this matters now

If DreamLab positions VisionClaw as the platform expression of the Dynamic Agentic Mesh before closing these gaps, buyers will see:

- strong infrastructure,
- strong vision,
- incomplete operational readiness.

This is fixable, but only if product development prioritises the human and governance control plane over further peripheral novelty.

---

## 4. Product Vision

### Vision statement

VisionClaw will become the operating system for governed agentic organisations: a platform where agents coordinate through shared semantics, humans intervene only where judgment is irreplaceable, and organisational learning compounds through a measurable, auditable loop.

### Positioning statement

For organisations facing uncontrolled AI adoption, coordination overload, and governance risk, VisionClaw is the platform that converts hidden AI work into governed organisational capability through ontology-grounded orchestration, judgment-broker workflows, and provenance-backed policy enforcement.

---

## 5. Goals

### 5.1 Primary goals

1. **Operationalise the Judgment Broker role** as a real product workflow, not just a thesis concept.
2. **Build the Insight Ingestion Loop** from discovery to propagation.
3. **Instrument the four mesh KPIs** so the thesis can be measured inside the product.
4. **Make VisionClaw enterprise-adoptable** through enterprise identity and policy integration.
5. **Eliminate internal coordination debt** in the platform core that undermines the thesis.
6. **Produce pilot-ready workflows** for one or two regulated, knowledge-dense sectors.

### 5.2 Secondary goals

1. Reframe the 3D layer as a coordination digital twin.
2. Turn provenance infrastructure into a reusable organisational learning ledger.
3. Position Solid plus user-owned memory as the sanctioned answer to shadow AI.

---

## 6. Non-Goals

The following are explicitly out of scope for this PRD:

1. Replacing all existing enterprise workflow systems.
2. Building a generic consumer knowledge graph platform.
3. Solving every XR or rendering roadmap item before broker workflows exist.
4. Creating a universal no-code BPM suite.
5. Supporting every identity provider or every enterprise connector in the first release.
6. Proving universal applicability across all industries in one release cycle.

---

## 7. Target Users and Jobs to Be Done

### 7.1 Judgment Broker

**Who:** Middle manager, programme lead, transformation lead, risk lead, architecture lead.
**Primary jobs:**

- Review edge cases the mesh cannot safely resolve.
- Validate or reject discovered workflows.
- Resolve cross-functional conflicts.
- Maintain trust in the automated layer.
- Curate organisational learning into approved capability.

### 7.2 Functional Leader / COO / Transformation Sponsor

**Who:** COO, VP Operations, Chief Transformation Officer, CIO, Head of Change.
**Primary jobs:**

- Understand where work is flowing and where it is blocked.
- Measure whether AI adoption is compounding or fragmenting the organisation.
- See whether judgment intervention is rising or falling.
- Decide which workflows should be institutionalised.

### 7.3 AI Platform Administrator

**Who:** Enterprise platform team, architecture team, AI enablement team.
**Primary jobs:**

- Integrate identity, policy, and connectors.
- Maintain semantic models and routing quality.
- Manage agent permissions and workflow deployment.
- Monitor runtime health and drift.

### 7.4 Individual Contributor with Personal AI Workflows

**Who:** Analyst, researcher, engineer, operator, domain specialist.
**Primary jobs:**

- Capture useful local workflows safely.
- Suggest improvements without fear of punishment.
- Reuse approved patterns from other teams.
- Keep personal augmentation while staying inside sanctioned governance.

### 7.5 Auditor / Compliance / Risk Reviewer

**Who:** Internal audit, model risk, compliance, governance office.
**Primary jobs:**

- Inspect why a workflow was approved.
- See which agent, user, policy, and data produced an outcome.
- Verify access boundaries and provenance.
- Export defensible records for review.

---

## 8. Current-State Diagnosis

### 8.1 Strengths to preserve

1. **Ontology and reasoning substrate** are a true differentiator.
2. **Provenance model** is stronger than most agentic platforms.
3. **User-owned memory and views** are strategically distinctive.
4. **Real-time graph and XR visualisation** can become a high-value decision layer.
5. **Multi-agent orchestration** is already meaningfully integrated.

### 8.2 Critical gaps to close

1. No judgment-broker control plane.
2. No first-class organisational workflow discovery pipeline.
3. No native measurement of Mesh Velocity, Augmentation Ratio, Trust Variance, or HITL Precision.
4. No enterprise SSO.
5. No broad enterprise system connectors.
6. Core runtime still has duplicated paths in node typing, position flow, settings, and protocol encoding.
7. Known ontology visualisation gaps weaken human trust in the semantic layer.

### 8.3 Strategic interpretation

VisionClaw is **closer to a semantic operating kernel than a finished operating system**. The fastest path to strategic coherence is not more visual complexity; it is a stronger broker, governance, and workflow layer built on top of what already exists.

---

## 9. Product Scope

This initiative is divided into seven workstreams.

### Workstream 1: Platform Coherence Foundation

This workstream resolves internal architecture fragmentation that directly undermines trust and maintainability.

**Scope includes:**

- single node type system,
- single binary encoding path,
- single position delivery path,
- single settings object chain,
- ontology edge-gap repair,
- graph loading correctness.

**Source basis:** [`docs/PRD-001-pipeline-alignment.md`](../docs/PRD-001-pipeline-alignment.md), [`docs/adr/ADR-036-node-type-consolidation.md`](../docs/adr/ADR-036-node-type-consolidation.md), [`docs/adr/ADR-037-binary-protocol-consolidation.md`](../docs/adr/ADR-037-binary-protocol-consolidation.md), [`docs/adr/ADR-038-position-flow-consolidation.md`](../docs/adr/ADR-038-position-flow-consolidation.md), [`docs/adr/ADR-039-settings-consolidation.md`](../docs/adr/ADR-039-settings-consolidation.md), [`docs/KNOWN_ISSUES.md`](../docs/KNOWN_ISSUES.md).

**Outcome:** the substrate becomes coherent enough to support enterprise-facing workflow and governance features.

---

### Workstream 2: Enterprise Identity & Access

This workstream adds enterprise identity compatibility without abandoning Nostr-native strengths.

**Must-have capabilities:**

- OIDC support for enterprise users,
- SAML compatibility via direct support or proxy layer,
- SCIM-compatible user/group sync roadmap,
- role mapping for Judgment Brokers, admins, contributors, auditors,
- policy-based access controls for workflow actions,
- compatibility between enterprise identity and Nostr-backed provenance.

**Required decisions:**

- whether to run dual identity paths in parallel,
- whether Nostr remains internal provenance identity while OIDC handles user access,
- how group membership maps to policy and workflow scope.

**Outcome:** a regulated enterprise can authenticate users without forcing browser extensions or nonstandard identity workflows.

---

### Workstream 3: Judgment Broker Workbench

This is the core product layer of the thesis and the highest-priority new user surface.

#### Main product surfaces

1. **Broker Inbox**
   - Escalated cases
   - Workflow proposals awaiting review
   - Policy exceptions
   - Trust drift alerts
   - Cross-functional conflicts

2. **Decision Canvas**
   - Full provenance of the case
   - Underlying graph context
   - Source data and agent chain
   - Suggested decisions and confidence
   - Applicable governance policies
   - Related past decisions and patterns

3. **Decision Actions**
   - approve,
   - reject,
   - amend,
   - request more evidence,
   - delegate to another broker,
   - promote as reusable workflow,
   - mark as policy precedent.

4. **Broker Timeline**
   - History of adjudications,
   - decision quality outcomes,
   - override frequency,
   - trust drift correlation,
   - workload distribution.

#### Core product principles

- The workbench must keep humans cognitively engaged, not just approving defaults.
- It must present ambiguity clearly rather than hiding it.
- It must reduce alert fatigue by prioritising only meaningful escalations.
- It must make the graph and provenance model explorable in context.

**Outcome:** the central human role in the thesis becomes a real, repeatable operational practice.

---

### Workstream 4: Insight Ingestion Loop

This workstream productises the compounding loop.

#### Pipeline stages

1. **Discovery**
   - Ingest candidate signals from human actions, agent actions, and connected systems.
   - Detect repeated workarounds, repeated prompts, repeated graph traversals, repeated decision patterns.

2. **Codification**
   - Convert discovered patterns into structured workflow proposals.
   - Represent each proposal as a typed graph object with provenance and scope.

3. **Validation**
   - Route proposals to the appropriate broker or governance role.
   - Run policy checks and semantic consistency checks.

4. **Integration**
   - Publish approved workflows into live orchestration.
   - Version them.
   - Attach SLAs, ownership, and rollback rules.

5. **Amplification**
   - Recommend approved workflows to other teams,
   - attach training/context assets,
   - measure reuse and adaptation.

#### Product requirements

- Workflow proposals must be diffable and versioned.
- Every proposal must show origin, source evidence, affected functions, and expected benefit.
- Every approved workflow must be rollbackable.
- Every rejected proposal must preserve reasoning for future learning.

**Outcome:** VisionClaw becomes a platform that institutionalises useful local AI work instead of letting it remain invisible.

---

### Workstream 5: Organisational KPI & Observability Layer

This workstream instruments the thesis.

#### KPI definitions

1. **Mesh Velocity**
   - Time from first discovery signal to approved reusable workflow.

2. **Augmentation Ratio**
   - Proportion of decision volume or workflow volume resolved without escalation.

3. **Trust Variance**
   - Rolling variance in decision quality, override rates, or policy exceptions across workflows and agents.

4. **HITL Precision**
   - Percentage of escalations where human intervention materially changed or improved the outcome.

#### Requirements

- Each KPI must have a documented definition, data source, update frequency, and confidence band.
- KPI dashboards must be explorable by team, function, workflow type, agent type, and time window.
- Metrics must be exportable for auditors and transformation leaders.
- KPI lineage must be auditable: users can click from a metric to the underlying decision events.

#### Dashboard views

- Executive dashboard,
- broker dashboard,
- platform ops dashboard,
- pilot/customer dashboard.

**Outcome:** the thesis becomes measurable and therefore governable.

---

### Workstream 6: Discovery Connectors

This workstream gives the Discovery Engine actual enterprise input.

#### Connector priorities

**Tier 1**

- Slack,
- Microsoft Teams,
- Jira,
- Confluence,
- Notion,
- Google Workspace docs/comments,
- GitHub issues/PR/review events.

**Tier 2**

- ServiceNow,
- Salesforce,
- Linear,
- Zendesk,
- meeting transcript systems,
- incident management systems.

#### Discovery outputs

Connectors should surface:

- recurring manual coordination loops,
- repeated requests for the same context,
- approval bottlenecks,
- shadow AI workflow patterns,
- cross-functional handoff failures,
- local automations worth institutionalising.

#### Privacy requirements

- discovery must support scoped ingestion,
- redaction and minimisation must be configurable,
- legal and works-council constraints must be respected,
- monitoring must be transparent and role-governed.

**Outcome:** the Discovery Engine becomes operational in real enterprises rather than remaining limited to knowledge repositories and technical tools.

---

### Workstream 7: Provenance, Policy, and Digital Twin Reframing

This workstream generalises existing strong primitives and repositions the visual layer.

#### Provenance generalisation

Extend the bead and learning system beyond brief/debrief provenance to cover:

- workflow proposal lifecycle,
- broker decisions,
- policy override events,
- rollout events,
- workflow reuse and propagation,
- trust incidents and recovery actions.

#### Policy model

Introduce a reusable policy model for:

- escalation rules,
- domain ownership,
- confidence thresholds,
- regulatory workflows,
- separation-of-duty rules,
- workflow deployment permissions.

#### Digital twin reframing

Reframe graph and XR surfaces as:

- live map of coordination flows,
- escalation topology,
- trust drift zones,
- workflow adoption clusters,
- cross-functional bottlenecks,
- decision provenance explorer.

**Outcome:** existing graph strengths are reinterpreted as operational decision surfaces for the thesis, not just knowledge visualisation.

---

## 10. Detailed Functional Requirements

### FR1 — Broker Inbox

The system must provide a single inbox where a broker can see all open escalations, proposals, trust alerts, and policy exceptions.

**Acceptance criteria:**

- Items sortable by severity, business impact, age, and domain.
- Each item displays confidence, source, owner, and required action.
- Broker can complete action without leaving the workbench.

### FR2 — Decision Provenance View

Every reviewed item must expose complete provenance.

**Acceptance criteria:**

- Show originating signals,
- participating agents,
- graph entities involved,
- policies evaluated,
- prior similar decisions,
- affected users or teams.

### FR3 — Workflow Proposal Model

The system must represent a workflow proposal as a first-class object.

**Acceptance criteria:**

- Proposal has unique ID, source, status, owner, history, risk score, expected benefit.
- Proposal can be versioned and diffed.
- Proposal supports comments, amendments, and final decision.

### FR4 — Promotion and Propagation

Approved workflows must move into production orchestration with rollout controls.

**Acceptance criteria:**

- Support pilot rollout to one team,
- staged rollout to multiple teams,
- rollback,
- change log,
- adoption measurement.

### FR5 — KPI Engine

The system must calculate the four mesh KPIs continuously from event data.

**Acceptance criteria:**

- Metrics update within a defined freshness window.
- Every KPI is explainable from its source events.
- Metrics can be sliced by workflow, team, function, agent type, and time.

### FR6 — Enterprise Identity

The system must support enterprise sign-in and role mapping.

**Acceptance criteria:**

- OIDC login works for all primary roles.
- Role mapping supports broker, admin, auditor, contributor.
- Audit logs preserve both enterprise user identity and signed action provenance.

### FR7 — Discovery Signal Ingestion

The system must ingest structured events from enterprise collaboration and work systems.

**Acceptance criteria:**

- Connector configuration is admin-controlled.
- Discovery rules are configurable.
- Signals can be sampled, redacted, and scoped.

### FR8 — Organisational Digital Twin Views

The graph layer must provide operational views beyond entity exploration.

**Acceptance criteria:**

- Show workflow bottlenecks,
- escalation hotspots,
- trust variance clusters,
- workflow reuse patterns,
- broker workload distribution.

### FR9 — Audit and Compliance Export

The system must support exportable evidence for governance and review.

**Acceptance criteria:**

- Export workflow histories,
- decision records,
- policy evaluations,
- KPI lineage,
- broker decisions,
- provenance chains.

### FR10 — Resilience and Fallback

The system must fail safely.

**Acceptance criteria:**

- If connector ingestion fails, workflow operations continue.
- If semantic services degrade, broker views remain accessible with reduced automation.
- If enterprise identity is unavailable, approved local emergency access patterns exist.

---

## 11. Non-Functional Requirements

1. **Auditability:** every broker decision and workflow promotion must be reconstructable.
2. **Security:** enterprise auth, role-based controls, and signed provenance must coexist.
3. **Performance:** broker inbox load under 2 seconds; decision view under 3 seconds for normal cases.
4. **Reliability:** no single duplicated runtime path for node typing, position flow, settings, or binary protocol.
5. **Scalability:** support at least 500 active users and 50+ active agents in pilot environments.
6. **Privacy:** connector ingestion supports minimisation and legal review.
7. **Explainability:** every metric and decision is explorable from source to output.
8. **Adoption:** the primary broker workflows must be learnable within 1–2 weeks of onboarding.

---

## 12. Proposed Data Model Additions

### New primary graph entities

1. **Insight**
   - raw detected pattern or human-submitted discovery

2. **WorkflowProposal**
   - structured candidate workflow awaiting review

3. **BrokerDecision**
   - approval, rejection, amendment, escalation, policy exception

4. **WorkflowPattern**
   - approved reusable workflow template

5. **EscalationCase**
   - runtime issue requiring broker review

6. **OrganisationalMetricSnapshot**
   - KPI materialisation over time windows

7. **ConnectorSource**
   - external system and sync state

### Required relationships

- Insight discovered_from ConnectorSource
- Insight codified_as WorkflowProposal
- WorkflowProposal reviewed_by User
- WorkflowProposal promoted_to WorkflowPattern
- EscalationCase linked_to WorkflowPattern
- BrokerDecision attached_to EscalationCase or WorkflowProposal
- OrganisationalMetricSnapshot derived_from BrokerDecision and workflow events

### Storage strategy

- Neo4j remains primary for graph objects, workflow objects, and metric lineage.
- Solid remains primary for user-owned personal overlays, broker preferences, and optional private notes.
- Provenance entries remain event-driven and append-only where practical.

---

## 13. API and Service Changes

### New service domains

1. Broker Service
2. Workflow Proposal Service
3. Insight Discovery Service
4. KPI Computation Service
5. Connector Ingestion Service
6. Policy Evaluation Service

### New endpoint groups

- /api/broker/inbox
- /api/broker/cases
- /api/workflows/proposals
- /api/workflows/patterns
- /api/insights
- /api/mesh-metrics
- /api/connectors
- /api/policy

### Existing service reuse

- ontology discovery / read / query / propose / validate tools
- bead provenance lifecycle
- settings and Solid services
- graph and analytics services
- WebSocket / graph visualisation pipeline

---

## 14. UX Requirements

### Primary surfaces

1. Broker Home
2. Escalation Case Detail
3. Workflow Proposal Review
4. Organisational KPI Dashboard
5. Discovery Review Queue
6. Policy Console
7. Coordination Digital Twin View
8. Connector Administration

### UX principles

- ambiguous cases must be understandable in under 60 seconds,
- provenance must be explorable without switching systems,
- approval should require understanding, not just clicking,
- alerts must be sparse and meaningful,
- all dashboards must answer “what needs judgment now?”

---

## 15. Delivery Plan

## Phase 0 — Foundation and truth alignment (Weeks 1–6)

**Objective:** make the platform internally coherent and externally honest.

**Deliverables:**

- platform consolidation plan execution begins,
- ontology edge gap fixed,
- single position path confirmed,
- single node typing path confirmed,
- settings path unified,
- external-facing truth pack for platform claims.

**Exit criteria:** no major contradictions between platform story and platform behaviour.

## Phase 1 — Identity and broker MVP (Weeks 7–14)

**Objective:** make the platform usable in a real enterprise login context and provide the first broker surface.

**Deliverables:**

- OIDC support,
- role model,
- Broker Inbox MVP,
- Decision Canvas MVP,
- first provenance-linked broker actions.

**Exit criteria:** a broker can log in, review, and decide on real cases.

## Phase 2 — Insight Ingestion Loop MVP (Weeks 15–24)

**Objective:** move from isolated decisions to discovery-to-workflow flow.

**Deliverables:**

- Insight objects,
- WorkflowProposal objects,
- promotion/rollback flow,
- first two enterprise connectors,
- reusable workflow publication.

**Exit criteria:** one discovered pattern can become an approved live workflow.

## Phase 3 — KPI and governance release (Weeks 25–32)

**Objective:** measure the mesh and enforce policy coherently.

**Deliverables:**

- Mesh Velocity,
- Augmentation Ratio,
- Trust Variance,
- HITL Precision,
- policy console,
- exportable audit reports.

**Exit criteria:** thesis KPIs measurable from production data.

## Phase 4 — Pilot release (Weeks 33–44)

**Objective:** validate in one or two target domains.

**Deliverables:**

- regulated pilot package,
- connector hardening,
- customer-specific ontology and workflow starter kit,
- success reporting framework.

**Exit criteria:** at least one paid or design-partner pilot running.

---

## 16. Success Metrics

### Product success

1. 80% of broker actions completed within a single workbench session.
2. 90%+ of escalations judged by brokers as genuinely needing human intervention.
3. Median proposal-to-production workflow promotion under 14 days in pilot.
4. At least 30% of approved workflows reused by a second team within 90 days.

### Platform success

1. No duplicate core path remains for node typing, settings, binary protocol, or position flow.
2. Enterprise sign-in working for all pilot users.
3. Ontology-derived visual and workflow state are consistent and trustworthy.
4. Provenance events available for 100% of broker decisions and workflow promotions.

### Strategic success

1. One design partner or paying pilot in a regulated or knowledge-dense sector.
2. One externally credible case study showing measurable reduction in hidden AI work or coordination latency.
3. VisionClaw narrative shifts from “graph platform” to “agentic coordination control plane.”

---

## 17. Risks and Mitigations

### Risk 1 — Enterprise identity dilutes decentralised differentiation

**Mitigation:** preserve Nostr as signed provenance layer while using OIDC for user access.

### Risk 2 — Ontology burden slows adoption

**Mitigation:** ship narrow starter ontologies and workflow kits by vertical; sell ontology construction as guided onboarding, not prerequisite perfection.

### Risk 3 — Connector privacy concerns trigger organisational resistance

**Mitigation:** build transparent, scoped, minimised discovery with legal review modes and auditability.

### Risk 4 — Broker role becomes a new bottleneck

**Mitigation:** instrument HITL Precision early, optimise escalation quality before scaling volume.

### Risk 5 — Platform coherence debt blocks new feature reliability

**Mitigation:** Workstream 1 is mandatory and precedes major broker rollout.

### Risk 6 — XR and visual layers distract roadmap focus

**Mitigation:** freeze major discretionary XR expansion until broker and KPI layers exist, except for fixes directly tied to operational views.

---

## 18. Dependencies and Required Decisions

### Blocking product decisions

1. Enterprise identity direction: dual-stack or proxy model.
2. Broker role model and permission set.
3. Final KPI formula definitions and data confidence model.
4. Workflow proposal schema and versioning strategy.
5. Discovery connector privacy and legal operating model.
6. Policy language: embedded rules only or reusable DSL.

### Recommended ADRs

New ADRs should be created for:

- enterprise identity strategy,
- judgment broker workbench architecture,
- workflow proposal object model,
- KPI lineage model,
- connector governance and privacy boundaries,
- policy engine approach.

---

## 19. Recommended Initial Pilot Wedge

### Preferred pilot sectors

1. Pharma / biotech R&D
2. Financial risk / compliance
3. Advanced manufacturing programme coordination

### Pilot criteria

Choose a pilot where:

- work is knowledge-dense,
- provenance matters,
- policy matters,
- cross-functional coordination is expensive,
- hidden local AI work is likely already happening,
- a broker-like human role already exists informally.

### Pilot workflow examples

- cross-functional regulatory review workflow,
- risk assessment and approval workflow,
- technical change-intake and governance workflow,
- recurring research synthesis and proposal workflow.

---

## 20. Final Recommendation

DreamLab should treat this initiative as the product programme that makes the Dynamic Agentic Mesh thesis commercially real.

The right order is:

1. **stabilise and simplify the substrate**,
2. **ship enterprise identity**,
3. **build the Judgment Broker Workbench**,
4. **productise the Insight Ingestion Loop**,
5. **instrument the thesis KPIs**,
6. **validate in a serious pilot environment**.

If executed in that order, VisionClaw will move from being a powerful and unusual graph/agentic platform to being the first credible control plane for governed organisational AI.

This PRD is the correct bridge between the thesis and the product, and it should become the governing document for the next major delivery cycle.

2. The "Tarpits" (Risks & Blind Spots to watch out for)
While the strategy is sound, there are three specific technical/product execution risks hiding in this PRD that will need careful handling:
A. The "Discovery Connectors" Tarpit (Workstream 6)
The PRD lists Slack, Teams, Jira, Confluence, and Notion as "Tier 1" connectors. This is a massive trap. Building reliable, enterprise-compliant, rate-limit-respecting integrations for these platforms is notoriously difficult and could easily consume your entire engineering bandwidth.
Recommendation: Do not try to build a universal ingestion engine in Phase 2. Pick exactly one high-signal system where shadow workflows live (e.g., Jira or GitHub Issues) and build the complete Insight Ingestion Loop around that single source first.
B. The "Cold Start" Problem for the Judgment Broker
The PRD assumes the Discovery Engine will surface insights into the Broker's Inbox. But what happens on Day 1? The engine won't have enough data to confidently propose workflows, meaning the Broker Inbox will be empty, and the user won't see the value of the platform.
Recommendation: You need a manual "Submit Workflow Proposal" primitive right away. Allow a human worker to manually describe a shadow workflow they use, attaching the relevant agents. Let the Judgment Broker review manual submissions first to train the system, then introduce automated discovery later.
C. The OIDC ↔ Nostr Identity Collision (Workstream 2)
The PRD requires OIDC for enterprise sign-in while preserving Nostr for signed provenance. This is going to be a severe UX and architectural friction point. How does an enterprise user logging in via Microsoft Entra ID (OIDC) get a secp256k1 keypair to sign NIP-98 events without forcing them to install a browser extension (which IT will block)?
Recommendation: This needs to be the very first ADR written. The most likely path is a secure, server-side enclave (or the user's Solid Pod) that holds a delegated, ephemeral Nostr keypair mapped to their OIDC session token, shielding the user from the cryptographic complexity entirely.
3. Immediate Next Steps
To turn this PRD into executable sprints, I recommend the following sequence:
Draft the Enterprise Identity ADR immediately. This is the biggest technical unknown. You cannot build the Broker Workbench if you don't know how the Broker logs in.
Wireframe the Judgment Broker Workbench. Do not write code for this yet. Draw the Inbox, the Decision Canvas, and the Provenance View. Getting this UI right will dictate exactly what data the backend needs to aggregate.
Execute Workstream 1 (Platform Coherence) right now. Since it's pure refactoring and consolidation, the backend team can start executing this immediately while product/design figures out the Identity and Workbench layers.
Define the exact Cypher schema for the new entities. Map out exactly what WorkflowProposal and BrokerDecision look like in Neo4j, and how they link to existing OwlClass and Bead (provenance) node