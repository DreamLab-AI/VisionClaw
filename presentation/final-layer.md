Strategic gap analysis complete.

# Strategic assessment

The project currently has two strong strata and one weak one:

1. **A strong substrate** — graph, ontology, GPU physics, agents, Solid Pods, identity, policy, and migration primitives are all substantially designed in [`docs/README.md`](docs/README.md), [`docs/explanation/system-overview.md`](docs/explanation/system-overview.md), [`docs/explanation/ontology-pipeline.md`](docs/explanation/ontology-pipeline.md), [`docs/how-to/integration/solid-integration.md`](docs/how-to/integration/solid-integration.md), and [`docs/reference/neo4j-schema-unified.md`](docs/reference/neo4j-schema-unified.md).
2. **A strong management mesh** — broker, workflow, KPI, connector, and policy surfaces are defined in [`docs/PRD-002-enterprise-ui.md`](docs/PRD-002-enterprise-ui.md), [`docs/adr/ADR-041-judgment-broker-workbench.md`](docs/adr/ADR-041-judgment-broker-workbench.md), [`docs/adr/ADR-042-workflow-proposal-object-model.md`](docs/adr/ADR-042-workflow-proposal-object-model.md), [`docs/adr/ADR-043-kpi-lineage-model.md`](docs/adr/ADR-043-kpi-lineage-model.md), and [`docs/adr/ADR-045-policy-engine-approach.md`](docs/adr/ADR-045-policy-engine-approach.md).
3. **A missing contributor harness** — the platform does not yet provide a first-class operating environment for knowledge providers who are not managers, but who do daily work with AI, skills, ontology guidance, private Pod-scoped context, and controlled sharing into the mesh.

That missing middle layer is the final piece.

The strategic conclusion is simple:

**VisionClaw currently has the governors and the machine, but not the daily cockpit for the people who generate institutional intelligence.**

It can govern contributions once they become visible. It can reason over ontology once it exists. It can store personal state in sovereign Pods. It can run specialist agents and MCP tools. But it does **not** yet combine those capabilities into a contributor-facing product surface that raises the floor for non-manager knowledge workers.

Without that stratum, the system risks reproducing exactly the failure mode described in the external research: lots of individual AI activity, little institutional compounding.

---

# Evidence from the current corpus

## 1. The enterprise layer is management-heavy, not contributor-heavy

The planned enterprise UI in [`docs/PRD-002-enterprise-ui.md`](docs/PRD-002-enterprise-ui.md) is composed of five surfaces: Broker Workbench, Workflow Studio, KPI Dashboard, Connector Management, and Policy Console. These are governance and management surfaces.

The contributor role does exist in the PRDs and ADRs, but only thinly:

- In [`docs/prd-insight-migration-loop.md`](docs/prd-insight-migration-loop.md), the Contributor is defined mainly as someone who authors notes and submits workflow proposals.
- In [`docs/adr/ADR-040-enterprise-identity-strategy.md`](docs/adr/ADR-040-enterprise-identity-strategy.md), Contributor is a role in the identity model, but there is no contributor-native experience designed around it.
- In [`docs/explanation/ddd-enterprise-contexts.md`](docs/explanation/ddd-enterprise-contexts.md), BC11–BC17 cover brokering, workflows, discovery, identity, KPIs, connectors, and policy — but nothing like a contributor enablement or contributor augmentation context exists.

So the contributor is recognized as an actor, but not yet served as a first-class operating persona.

## 2. The building blocks for a contributor stratum already exist, but are fragmented

The project already has many of the raw ingredients:

- Personal workspaces in [`docs/how-to/features/workspace.md`](docs/how-to/features/workspace.md)
- Command palette and keyboard-first invocation in [`docs/how-to/features/command-palette.md`](docs/how-to/features/command-palette.md)
- Pod-backed graph views in [`docs/adr/ADR-027-pod-backed-graph-views.md`](docs/adr/ADR-027-pod-backed-graph-views.md)
- Type Index discovery in [`docs/adr/ADR-029-type-index-discovery.md`](docs/adr/ADR-029-type-index-discovery.md)
- Agent memory in Pods in [`docs/adr/ADR-030-agent-memory-pods.md`](docs/adr/ADR-030-agent-memory-pods.md)
- Per-user GitHub credentials in Pods in [`docs/adr/ADR-030-ext-github-creds-in-pod.md`](docs/adr/ADR-030-ext-github-creds-in-pod.md)
- Pod-backed identity and WAC patterns in [`docs/adr/ADR-052-pod-default-wac-public-container.md`](docs/adr/ADR-052-pod-default-wac-public-container.md)
- A large agent/skill surface in [`docs/how-to/agent-orchestration.md`](docs/how-to/agent-orchestration.md) and [`docs/reference/agents-catalog.md`](docs/reference/agents-catalog.md)

But these capabilities are presented as infrastructure, APIs, and low-level features — **not as one coherent contributor experience**.

## 3. Ontology wisdom is available to agents, not yet productized for contributors

The ontology toolchain is rich:

- Discovery, read, query, traverse, propose, validate, status in [`docs/how-to/agent-orchestration.md`](docs/how-to/agent-orchestration.md)
- Migration/broker loop in [`docs/adr/ADR-049-insight-migration-broker-workflow.md`](docs/adr/ADR-049-insight-migration-broker-workflow.md)
- The full migration explanation in [`docs/explanation/insight-migration-loop.md`](docs/explanation/insight-migration-loop.md)

But the access pattern is still primarily one of:

- backend API,
- MCP tools,
- broker review,
- power-user / admin orchestration.

What is missing is a contributor-native “ontology guide” experience that says:

- what the ontology currently knows,
- which terms are canonical,
- what precedents exist,
- which rules matter for this role,
- what the next best contribution is,
- how to safely share work into the mesh.

## 4. Even the management layer still has integration debt

[`docs/qe-enterprise-audit-report.md`](docs/qe-enterprise-audit-report.md) makes an important strategic point: even management-facing enterprise surfaces still have wire gaps, adapter gaps, schema gaps, and auth gaps.

That matters because the contributor stratum cannot just “submit upward” into broker, workflow, and policy surfaces unless those paths are reliable, persisted, and authorised.

This means the missing contributor stratum is not only a UX/product gap. It is also an orchestration and completion gap across the current management mesh.

---

# The actual gap

The gap is **not** “we need a few more tools.”

The gap is:

**There is no institutional AI harness for contributors.**

In practical terms, that means there is no first-class surface where a knowledge provider can:

- enter work with full role-aware context,
- see relevant ontology guidance as they work,
- invoke AI partners and shared skills without setup pain,
- accumulate private memory and personal ambitions in their own Pod,
- turn successful workflows into reusable team capability,
- share work into the governed mesh without becoming a broker,
- be proactively supported by the system rather than forced to prompt everything manually.

Today, the contributor journey is fragmented across:

- the graph,
- the command palette,
- external skills in the multi-agent environment,
- raw Solid Pod APIs,
- broker workflows,
- and manual GitHub or note authoring patterns.

That fragmentation is exactly what the industry research warns against.

---

# Strategic lessons from the industry material

## PwC: leaders capture value by building AI foundations, not just using tools

[`presentation/2026-04-20-best-companies-ai/01-pwc-2026-ai-performance-study.md`](presentation/2026-04-20-best-companies-ai/01-pwc-2026-ai-performance-study.md) shows that leading companies capture disproportionate value because they combine AI use with AI foundations: governance, data, talent, process, and role-specific training.

**Implication for VisionClaw:** the contributor layer must be treated as a strategic capability platform, not a convenience feature. It needs onboarding, role-specific guidance, reusable workflows, and trust scaffolding.

## McKinsey: enduring capabilities, data productization, speed, trust, and agentic engineering

[`presentation/2026-04-20-best-companies-ai/02-mckinsey-ai-transformation-manifesto.md`](presentation/2026-04-20-best-companies-ai/02-mckinsey-ai-transformation-manifesto.md) argues that winning firms build enduring capabilities, focus on economic leverage points, treat platforms as strategic assets, make data easy to consume, design for adoption, and master agentic engineering.

**Implication for VisionClaw:** the missing stratum must:

- be a platform,
- sit on top of productized ontology and context,
- increase organisational metabolic rate,
- and be explicitly designed for adoption and scale.

## a16z: institutional AI is different from individual AI

[`presentation/2026-04-20-best-companies-ai/03-a16z-institutional-vs-individual-ai.md`](presentation/2026-04-20-best-companies-ai/03-a16z-institutional-vs-individual-ai.md) is especially relevant. It says the missing move is not more personal productivity, but institutional coordination, signal extraction, bias checking, enablement, and unprompted discovery.

**Implication for VisionClaw:** the contributor stratum must not be just “chat with the ontology.” It must coordinate AI and human partners, reduce slop, provide auditable and deterministic assistive flows, and proactively surface relevant work.

## Ramp Glass: the models are good enough, the harness is not

[`presentation/2026-04-20-best-companies-ai/04-ramp-glass-seb-goddijn.md`](presentation/2026-04-20-best-companies-ai/04-ramp-glass-seb-goddijn.md) and [`presentation/2026-04-20-best-companies-ai/04b-ramp-glass-shane-buchan-how-we-built-it.md`](presentation/2026-04-20-best-companies-ai/04b-ramp-glass-shane-buchan-how-we-built-it.md) are the clearest product precedent.

The critical lessons are:

- do not lower the ceiling,
- make complexity invisible rather than removing capability,
- make one person’s breakthrough everyone’s baseline,
- make the product itself the enablement mechanism,
- connect everything on day one,
- provide memory by default,
- support scheduled/headless work,
- make the interface a workspace, not just a chat window,
- and keep the codebase coherent with design systems, docs, and quality gates.

**Implication for VisionClaw:** the missing layer should look much more like an institutional AI workspace than like another admin dashboard.

## Anthropic Skill Creator 2.0: skills need lifecycle, evals, and retirement

[`presentation/2026-04-20-best-companies-ai/90-anthropic-skill-creator-v2.md`](presentation/2026-04-20-best-companies-ai/90-anthropic-skill-creator-v2.md) adds the missing discipline: skills are not prompts; they are software assets.

They need:

- creation,
- evaluation,
- benchmarking,
- improvement,
- triggering optimisation,
- and retirement when the base model catches up.

**Implication for VisionClaw:** any contributor skill layer must include evaluation and governance, not just markdown distribution.

---

# Proposed target state

## Name of the missing layer

I recommend explicitly naming the missing layer the **Contributor AI Support Stratum**.

This stratum sits:

- **above** the graph / ontology / agent / Pod substrate,
- **below** the broker / workflow / KPI / policy management mesh,
- and **around** everyday knowledge work.

Its purpose is to make contributors institutionally effective with AI.

## The main product surface

The primary user-facing manifestation of this stratum should be a new surface:

**Contributor Studio**

Not a dashboard. Not a chat page. A workspace.

It should be the place where contributors:

- open work,
- see ontology context,
- work with AI and human partners,
- use shared skills,
- store private context in their Pods,
- and decide what to share into the mesh.

---

# What Contributor Studio must do

## 1. Ontology-guided work

Contributors need continuous access to the “wisdom of the ontology,” not just a browseable hierarchy.

That means:

- term disambiguation while writing,
- canonical term suggestions,
- nearby concepts and prior examples,
- relevant policies and precedents,
- “house view” explanations,
- warnings when work conflicts with ontology norms,
- suggestions for where a note fits in the current conceptual structure,
- and gentle cues about whether something should stay private, be shared, or be submitted upward.

This is not the broker’s Decision Canvas. It is a live guidance layer for everyday work.

## 2. Private-by-default pod-backed context

The contributor should enter the system through a personal, sovereign workspace backed by their Pod.

This private layer should hold:

- current projects,
- active collaborators,
- personal ambitions and learning goals,
- work-in-progress artifacts,
- installed skills,
- preferred AI partners,
- session memory,
- reusable personal workflows,
- and private notes or experimental ontology drafts.

Current Pod primitives in [`docs/how-to/integration/solid-integration.md`](docs/how-to/integration/solid-integration.md), [`docs/adr/ADR-027-pod-backed-graph-views.md`](docs/adr/ADR-027-pod-backed-graph-views.md), [`docs/adr/ADR-029-type-index-discovery.md`](docs/adr/ADR-029-type-index-discovery.md), and [`docs/adr/ADR-030-agent-memory-pods.md`](docs/adr/ADR-030-agent-memory-pods.md) already make this feasible. What is missing is the integrated product layer and sharing semantics.

## 3. Skills as reusable organisational capability

VisionClaw already has an enormous skill and MCP tool surface in [`docs/how-to/agent-orchestration.md`](docs/how-to/agent-orchestration.md) and [`docs/reference/agents-catalog.md`](docs/reference/agents-catalog.md), but contributors do not yet have a productized way to:

- discover skills relevant to their role and tools,
- install them into their personal workspace,
- share them with a team,
- benchmark them,
- evaluate them,
- and retire or improve them.

This needs a first-class **Skill Dojo** inside Contributor Studio.

## 4. AI/human partner orchestration

Contributors should be able to work with:

- private AI coworkers,
- team-scoped AI partners,
- human collaborators,
- and the broker mesh,

from one place.

The system should make it obvious:

- which actions are private,
- which are team-shared,
- which create broker-visible artifacts,
- and which escalate into governed ontology or workflow changes.

## 5. Share-to-mesh workflow

The share path is currently underdeveloped.

There must be an explicit, ergonomic funnel from:

- private work in Pod,
- to team sharing,
- to formal proposal,
- to broker review,
- to promoted mesh asset.

That funnel should work for:

- ontology candidates,
- workflow proposals,
- reusable skills,
- graph views,
- structured insights,
- and AI-generated artifacts.

## 6. Proactive and unprompted support

The stratum should not wait for contributors to ask perfect questions.

It should proactively surface:

- suggested skills,
- relevant ontology neighbors,
- stale or under-grounded work,
- candidate notes that could be shared upward,
- broken connections,
- role-relevant precedents,
- and automation opportunities.

This is exactly the “unprompted” property from the a16z piece, implemented in a governed way.

---

# Strategic gap matrix

## Critical gaps

### G1. No contributor-first workspace

**Current state:** There is no contributor-native equivalent to the broker workbench. The closest pieces are generic workspaces and the command palette in [`docs/how-to/features/workspace.md`](docs/how-to/features/workspace.md) and [`docs/how-to/features/command-palette.md`](docs/how-to/features/command-palette.md).

**Why it matters:** contributors have substrate features but no coherent harness.

**Recommendation:** build Contributor Studio as a first-class surface before expanding management UI breadth further.

### G2. No integrated ontology guidance layer

**Current state:** ontology wisdom is available through APIs and MCP tools, but not productized into daily contributor work.

**Why it matters:** contributors cannot reliably align their work to the ontology while producing it.

**Recommendation:** add an Ontology Guide rail in Contributor Studio with contextual suggestions, canonical terms, rule hints, and precedent traces.

### G3. No skill operating model for contributors

**Current state:** skills exist operationally in the multi-agent environment, but not as a governed contributor capability layer.

**Why it matters:** breakthroughs do not compound across the org.

**Recommendation:** introduce Skill Dojo with discovery, install, share, eval, benchmark, and retirement.

### G4. No private-to-shared-to-mesh funnel

**Current state:** Pods, graph views, agent memory, and type index primitives exist, but there is no explicit “share into mesh” workflow for contributors.

**Why it matters:** personal work stays personal or must jump abruptly into broker/governance surfaces.

**Recommendation:** define share states and a Share Orchestrator.

## High gaps

### G5. No contributor memory and ambition model

**Current state:** Pods can store memory, but there is no product model for contributor goals, role context, collaborators, ambitions, or recurring work.

**Why it matters:** the system cannot personalize support or proactively recommend next actions.

**Recommendation:** create a Pod-backed contributor context profile and daily synthesis service.

### G6. No “everything connected on day one” integration experience

**Current state:** MCP tools and integrations are powerful but operational. Contributors still face setup and discoverability friction.

**Why it matters:** only power users get full value.

**Recommendation:** preconfigured tool connections, role-aware defaults, and one-click setup flows inside Contributor Studio.

### G7. No eval discipline for shared skills

**Current state:** no built-in Anthropic-style skill eval or benchmark loop.

**Why it matters:** skill quality drifts, redundant skills accumulate, trust falls.

**Recommendation:** require eval suites and benchmark baselines for any skill shared beyond personal scope.

### G8. Management layer still has delivery debt

**Current state:** [`docs/qe-enterprise-audit-report.md`](docs/qe-enterprise-audit-report.md) shows persistent backend and adapter gaps in the enterprise control plane.

**Why it matters:** contributor outputs cannot reliably flow upward until broker/workflow/policy paths are stable.

**Recommendation:** treat management mesh hardening as Phase 0 dependency.

## Medium gaps

### G9. No contributor metrics

**Current state:** KPI work focuses on broker/workflow/governance outcomes.

**Why it matters:** no way to measure whether the floor is rising.

**Recommendation:** add contributor activation, skill reuse, share-to-mesh, and context hit-rate KPIs.

### G10. No institutional training-by-doing loop

**Current state:** onboarding and command palette exist, but not role-specific enablement that improves through work.

**Why it matters:** learning remains separate from production.

**Recommendation:** make the product itself the trainer via contextual nudges, recommended skills, and workspace hints.

---

# Proposed target operating model

## Layering

### Layer 1 — Substrate (already strong)

- graph,
- ontology,
- Solid Pods,
- MCP tools,
- agent mesh,
- identity,
- policy,
- GPU physics.

### Layer 2 — Contributor AI Support Stratum (missing)

This new layer should own:

- Contributor Studio,
- Ontology Guide,
- Skill Dojo,
- contributor Pod context,
- AI/human partner orchestration,
- share-to-mesh workflows,
- proactive support.

### Layer 3 — Management Mesh (already designed)

- broker,
- workflows,
- KPI observability,
- connectors,
- policy,
- governance.

The missing stratum is the layer that converts day-to-day work into governed assets.

## Day-in-the-life flow

1. Contributor opens Contributor Studio.
2. Studio loads Pod-scoped context: projects, collaborators, active work, installed skills, personal goals.
3. Studio assembles ontology guidance relevant to the task.
4. Contributor works in a multi-pane workspace with AI partners and docs/tools side-by-side.
5. The system recommends skills, terms, prior cases, and automations.
6. Outputs stay private by default in the Pod.
7. Contributor can explicitly share to team or submit upward into the mesh.
8. Shared artifacts become workflow proposals, ontology candidates, reusable skills, or graph assets.
9. Broker/policy/governance handles escalation.
10. Accepted patterns feed back down as shared capability for all contributors.

That is the compounding loop.

---

# Proposed bounded-context changes

## New core context: Contributor Enablement

I recommend adding a new bounded context focused on contributor work.

### Proposed aggregate roots

- **ContributorWorkspace** — the live multi-pane work surface and its state
- **GuidanceSession** — one ontology-guided work episode with AI/human partner context
- **ShareIntent** — explicit publication intent from private Pod state to team or mesh
- **WorkArtifact** — a reusable output: note, skill, workflow seed, graph view, analysis

### Proposed services

- **ContextAssemblyService** — builds role/project/tool/person context from Pod + graph + recent work
- **OntologyGuidanceService** — surfaces canonical terms, precedents, rules, and recommended neighbors
- **PartnerOrchestrationService** — manages private AI coworkers, team agents, and human collaborators in one session
- **ShareOrchestrator** — moves artifacts from private → shared → governed states

## New supporting context: Skill Lifecycle

### Proposed aggregate roots

- **SkillPackage** — a contributor-visible skill asset
- **SkillVersion** — versioned skill definition
- **SkillEvalSuite** — benchmark prompts and assertions
- **SkillBenchmark** — baseline vs current performance results
- **SkillDistribution** — scope: personal, team, company, public

### Proposed services

- **SkillRegistryService** — discover, install, publish, retire
- **SkillEvaluationService** — run evals and comparisons
- **SkillRecommendationService** — role/tool/work-context aware suggestions
- **SkillCompatibilityScanner** — detect broken or obsolete skills as models and platforms change

## Existing contexts to extend

- **BC14 Enterprise Identity** should gain a richer contributor profile and delegation model for Contributor Studio.
- **BC13 Insight Discovery** should emit contributor-facing support signals, not only broker-facing surfacing events.
- **BC11 Judgment Broker** should accept structured upward shares from Contributor Studio, not only migration or escalation cases.
- **BC15 KPI Observability** should track contributor-level enablement and reuse, not just governance throughput.

---

# Data ownership model

## Private in the user’s Pod

The following should live primarily in the contributor’s Pod:

- contributor workspace layouts,
- personal context profile,
- session memory,
- installed personal skills,
- private work artifacts,
- ambition/goal records,
- scheduled automations,
- private drafts and notes.

## Shared but still user-controlled

- team-scoped skills,
- shared graph views,
- shared workspaces,
- shared contextual packs,
- reusable work artifacts.

These should be discoverable via Type Index patterns from [`docs/adr/ADR-029-type-index-discovery.md`](docs/adr/ADR-029-type-index-discovery.md).

## Canonical in Neo4j / governance layer

- promoted ontology classes,
- workflow patterns,
- broker decisions,
- contributor contribution index metadata,
- KPIs and lineage,
- policy evaluations.

The rule should be:

**private work is sovereign and pod-first; institutional assets are graph-first and governed.**

---

# Prioritized roadmap

## Phase 0 — Hardening prerequisites

Before building the new stratum:

1. close the enterprise wire/auth/persistence gaps called out in [`docs/qe-enterprise-audit-report.md`](docs/qe-enterprise-audit-report.md),
2. ensure workspace reliability and reload behaviour,
3. keep Pod/sovereign flows stable,
4. keep ingestion fidelity and ontology consistency moving in the right direction.

This phase is about making the destination reliable.

## Phase 1 — Contributor Studio MVP

Ship:

- multi-pane workspace shell,
- Pod-backed personal context,
- ontology guide rail,
- basic AI partner lane,
- explicit private/share states,
- role-aware command palette and onboarding.

Success criterion: a contributor can complete one meaningful task entirely inside VisionClaw without external setup.

## Phase 2 — Skill Dojo and connections

Ship:

- discover/install/share skills,
- role-aware recommendations,
- one-click integrations,
- personal/team/company skill scopes,
- skill metadata and provenance.

Success criterion: one contributor’s useful workflow can become a team baseline in under a day.

## Phase 3 — Share-to-mesh

Ship:

- explicit share funnel,
- upward submission into workflow/migration/broker channels,
- contributor-originated reusable artifacts,
- broker intake integration.

Success criterion: contributors can intentionally move work upward without leaving Contributor Studio.

## Phase 4 — Evals, benchmarks, and proactive support

Ship:

- skill eval suites,
- A/B benchmarking,
- trigger optimisation,
- proactive recommendations,
- unprompted “next best action” support.

Success criterion: the system measurably raises the floor and prunes redundant skills.

---

# KPIs for the new stratum

I recommend adding these metrics to BC15 once the stratum exists:

- **Contributor activation rate** — % of target contributors using Contributor Studio weekly
- **Time to first useful result** — median minutes from first login to first successful AI-assisted outcome
- **Skill reuse rate** — average installs/reuses per published skill
- **Shared breakthrough rate** — % of personally created skills/artifacts later shared to team scope
- **Share-to-mesh conversion** — % of upward shares becoming broker cases, proposals, or promoted assets
- **Ontology guidance hit rate** — % of sessions where contributors accept a system-suggested canonical term, precedent, or rule hint
- **Private-to-public confidence** — % of contributors who report they understand where their work is private, shared, or mesh-visible
- **Redundant skill retirement rate** — number of shared skills retired because base models caught up
- **Contributor satisfaction / trust** — survey-backed confidence in AI support and governance fairness

These are the “raise the floor” metrics.

---

# Risks if this stratum is not built

1. **Broker bottleneck intensifies** — only the governance layer is empowered, so everything interesting waits for management.
2. **Power-user islands form** — only people comfortable with MCP, skills, CLI tooling, and ontology structure gain compounding benefits.
3. **Ontology growth starves** — candidate production becomes accidental rather than systematic.
4. **Pod sovereignty under-delivers** — powerful storage exists, but contributors do not experience it as useful autonomy.
5. **Skills fragment** — the multi-agent environment accumulates capabilities that never become institutional capability.
6. **The platform looks more advanced than it feels** — a classic “great substrate, weak harness” failure.

That is the strategic danger.

---

# Recommended document and artifact package

I recommend creating the following next, in this order:

1. [`docs/PRD-003-contributor-ai-support-stratum.md`](docs/PRD-003-contributor-ai-support-stratum.md)
2. [`docs/adr/ADR-057-contributor-enablement-platform.md`](docs/adr/ADR-057-contributor-enablement-platform.md)
3. [`docs/explanation/contributor-support-stratum.md`](docs/explanation/contributor-support-stratum.md)
4. [`docs/explanation/ddd-contributor-enablement-context.md`](docs/explanation/ddd-contributor-enablement-context.md)
5. [`docs/design/2026-04-20-contributor-studio/00-master.md`](docs/design/2026-04-20-contributor-studio/00-master.md)
6. [`docs/design/2026-04-20-contributor-studio/01-contributor-studio-surface.md`](docs/design/2026-04-20-contributor-studio/01-contributor-studio-surface.md)
7. [`docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md`](docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md)
8. [`docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md`](docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md)
9. [`docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature`](docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature)

That artifact set would turn the current conceptual gap into a shippable program.

---

# Final strategic statement

The missing final piece is a **contributor operating system**.

VisionClaw already knows how to:

- reason,
- govern,
- visualise,
- remember,
- and share.

What it does **not** yet know how to do is make those capabilities natural, persistent, role-aware, and compounding for the everyday knowledge provider.

The Contributor AI Support Stratum should therefore be treated as a top-priority strategic layer, not a feature add-on.

If built well, it becomes the place where:

- ontology wisdom is made usable,
- Pod sovereignty becomes lived autonomy,
- skills become institutional capability,
- AI partners become coordinated coworkers,
- and private work becomes mesh intelligence through explicit, trusted, governed sharing.

That is the layer that turns VisionClaw from a strong governed graph platform into a genuinely AI-native institution-building system.