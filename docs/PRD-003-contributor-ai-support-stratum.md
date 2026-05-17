# PRD-003: Contributor AI Support Stratum

**Status**: Proposed
**Author**: Architecture Agent
**Date**: 2026-04-20
**Priority**: P1 -- Required to convert individual AI productivity into institutional intelligence
**Depends On**: ADR-027 (Pod-Backed Graph Views), ADR-029 (Type Index Discovery), ADR-030 (Agent Memory Pods), ADR-040 (Enterprise Identity), ADR-041 (Broker Workbench), ADR-042 (Workflow Proposals), ADR-045 (Policy Engine), ADR-046 (Enterprise UI Architecture), ADR-049 (Insight Migration Broker Workflow), ADR-052 (Pod Default WAC + Public Container)
**Companion ADRs**: ADR-057 (Contributor Enablement Platform)
**Supersedes / Extends**: extends PRD-002 (Enterprise Control Plane UI) with a sixth surface family for contributor-facing work

---

## 1. Problem Statement

VisionClaw has a strong substrate (graph, ontology, Solid Pods, agents, GPU physics) and a strong management mesh (BC11 Broker Workbench, BC12 Workflow Lifecycle, BC15 KPI Observability, BC16 Connector Ingestion, BC17 Policy Engine, all surfaced via the five enterprise UI surfaces in PRD-002). What it does not have is a **daily cockpit for knowledge providers** -- the contributors whose notes, prompts, skills, and quick experiments are the raw material the mesh depends on.

Today those contributors fragment their AI work across Logseq, a command palette, the MCP WebSocket bridge, the 3D graph, and their Solid Pods. Each tool is individually capable. None compound. The result is a failure mode documented across the industry: lots of individual AI activity, no institutional compounding. A contributor's Tuesday-afternoon breakthrough stays in their pod; a teammate rediscovers the same prompt on Friday; a third contributor retires a skill the company still quietly depends on.

The Broker Workbench (ADR-041) is where judgment happens. The Insight Migration Loop (PRD -- Insight Migration) is where tacit structure becomes ontology. The KPI Dashboard (PRD-002 §8) is where the mesh proves it is working. None of these are where the **everyday knowledge work** happens, and none of them build the funnel that feeds them. Without a contributor-facing surface, the Broker's inbox depends on automated discovery plus manual submissions against a form designed for escalations, not creative nomination. The mesh stays half-built.

Industry evidence converges on the same diagnosis. See the evidence annex at `docs/design/2026-04-20-contributor-studio/evidence-annex.md` for the primary sources; a representative selection: PwC's 2026 leadership survey ("leaders build AI foundations, not tools"); the McKinsey "AI Manifesto" argument for enduring capabilities, data productisation, and agentic engineering over one-off automations; the a16z Institutional AI thesis that coordination matters more than individual productivity and that unprompted discovery is the hard problem; Ramp's public "Glass" post-mortem on why harness beats models, why memory must be default, and why one person's breakthrough must become the team's baseline; and Anthropic's Skills v2 discipline for evaluations, benchmarks, and retirement as first-class lifecycle stages.

This PRD specifies the **Contributor AI Support Stratum** -- a new architectural layer sitting above the substrate and below the management mesh -- together with its primary product surface, the **Contributor Studio**, and three companion capabilities: the Mesh Dojo (decentralised skill sharing), the Ontology Sensei (proactive guidance), and Pod-Native Automations (headless contributor agents). It does so without replacing Logseq, the graph, the Broker Workbench, or any PRD-002 surface. It is the missing floor, not a rebuild.

---

## 2. Goals

Measurable outcomes within 120 days of Phase 4 completion, each tracked as a named KPI with source events defined in §12:

1. **Contributor activation rate** -- percentage of invited contributors who open the Studio, attach a pod, and complete one Guidance Session within 7 days. Target: 80%.
2. **Time-to-first-result (TTFR)** -- median elapsed wall-clock from first Studio login to first durable artefact (workspace saved, skill installed, automation enabled, inbox item reviewed). Target: <= 30 minutes.
3. **Skill reuse rate** -- cross-contributor installs ÷ distinct published skills over trailing 30 days. Target: >= 2.5.
4. **Share-to-mesh conversion rate** -- (MigrationPayload or WorkflowProposal submissions sourced from Studio share actions) ÷ (Studio-authored artefacts promoted to Team state) over trailing 30 days. Target: >= 15%.
5. **Ontology guidance hit rate** -- Sensei suggestions that the contributor accepts or edits toward (ignored = miss, dismissed with reason = signal). Target: >= 40% accept-or-edit.
6. **Redundant skill retirement rate** -- skills marked deprecated/retired ÷ skills in registry per quarter. Target: >= 8% (evidence the library self-prunes rather than accretes).

All six KPIs extend the KPI Lineage Model (ADR-043) and are reported on the Mesh KPI Dashboard (PRD-002 §8) as an additional dimension group "Contributor".

Secondary, non-metric goals:

- Every Studio-authored artefact carries provenance sufficient for a Broker to review it without a second round of context gathering.
- Studio never hides the CLI, MCP, or pod layer -- power users retain direct access; Studio is an accelerant, not a wall.
- A contributor whose pod is offline still has read-only Studio affordances (recent graph state, inbox digests, skill metadata) cached locally.

---

## 3. Non-Goals (explicit cuts)

This PRD is deliberately narrow. The following are out of scope for MVP and the four phases in §13:

- **Not a Notion or Obsidian replacement.** Studio does not include a block editor, a WYSIWYG markdown composer, backlinks-as-graph, or slash-command authoring. Logseq stays authoritative for long-form authorship. Studio's "editor lane" is an AI partner lane, not an authoring tool.
- **Not a chatbot-only surface.** The AI partner lane is a lane, not the product. Studio is a workspace with AI affordances woven in, not a chat window with files attached. The a16z and Ramp Glass critiques of chat-first UX apply directly.
- **Not a replacement for the Broker Workbench.** Share-to-mesh hand-off produces artefacts the Broker Workbench (ADR-041) and Migration Broker Workflow (ADR-049) then adjudicate. Studio does not approve, reject, or promote. It prepares, funnels, and hands over.
- **Not federated multi-tenant in MVP.** Single organisation per deployment. Cross-org skill sharing, federated skill registries, and multi-tenant pod discovery are v2.
- **Not mobile.** Desktop-first, tablet-tolerable, phone-unsupported. Enterprise power users work at desks; the information density we need is incompatible with phone viewports.
- **Not a swarm orchestration console.** Complex multi-agent coordination stays with the hive-mind surfaces (not-in-this-PRD). Studio's automations are single-purpose scheduled contributors, not fleets.
- **Not an offline-first application.** Studio assumes a live mesh connection for writes. Local caches cover read-only degraded modes; write queues are explicitly out of scope until v2.
- **No Studio-only auth.** Studio reuses ADR-040 enterprise identity and Nostr NIP-07; no new login surface, no local accounts.

---

## 4. Users and Roles

All roles map to the existing `EnterpriseRole` enum (`Broker`, `Admin`, `Auditor`, `Contributor`) from ADR-040. Studio introduces no new roles; it introduces progressive affordances within the existing `Contributor` role, distinguished by usage level rather than authorisation level.

| Role | Can see in Studio | Can do in Studio | Notes |
|------|------------------|------------------|-------|
| **Contributor** | Their own workspaces, their own skills, team-visible skills, public mesh skills, their own inbox, their own automations, graph read views consistent with their pod ACL | Author workspaces, install/evaluate skills, run AI partner sessions, schedule automations in their own pod, submit share-to-mesh intents | Default role; all new users. |
| **Power Contributor** | Everything a Contributor sees + skill eval leaderboards + share-to-mesh funnel analytics for their own submissions | Publish skills to Mesh state, author SkillEvalSuite fixtures, benchmark skills, propose retirements | Unlocked by meeting telemetry thresholds (e.g. >=3 skills published, >= 1 mesh promotion); tracked in `personal-context` memory. Not an RBAC escalation -- same `Contributor` role, exposed affordances. |
| **Team Lead** | Everything a Contributor sees for their reports + aggregated team activity digest | Grant `shared/team/` group-WAC entries, pin team-scoped skills, view team inbox digest | Derived from GroupMembership (BC14) claims, not from EnterpriseRole. |
| **Admin** | Everything, cross-team | Configure Studio defaults, manage the skill retirement queue, view redaction-blocked share-to-mesh attempts | Existing `Admin` role. |
| **Auditor** | Read-only view of all contributor activity, share-to-mesh provenance chains, skill eval history | No write, no claim, no share | Existing `Auditor` role. |

### 4.1 Personas

Continuing the personas introduced in the Insight Migration PRD (Rosa, Idris, Chen), with contributor-specific elaboration:

**Rosa, research lead at a 30-person AI policy institute.** Has 2,000+ Logseq pages. She opens Studio because she wants an AI partner that knows her corpus, not a chatbot she has to re-brief every session. Her Studio cockpit combines the graph view of her pod's knowledge graph, the AI partner lane pre-loaded with her pod's `/private/agent-memory/` episodic history, the Ontology Sensei rail suggesting three already-existing terms adjacent to her current note, and her inbox where automations have left synthesised briefs. When she has a reusable prompt, she publishes it to the Mesh Dojo at team scope. Her juniors install it the same day. The Sensei's job is to stop her re-inventing vocabulary she already has.

**Idris, principal at a boutique digital-transformation consultancy.** Runs 20 client engagements a year. Studio is his firm's common floor: a consultant wraps up an engagement, hits "Share to Team", the deliverable template + prompt set + eval harness land in `/shared/skills/{team}/` and `/shared/workspaces/{team}/` on their pod and surface to the next consultant who opens Studio with a similar client domain. Idris himself uses Pod-Native Automations to schedule weekly synthesis across his consultants' `/public/kg/` subtrees; the synthesis lands in his inbox Monday morning. Studio has to make "package an engagement" a two-minute gesture, or his team stays in their personal Logseqs and the firm compounds nothing.

**Chen, regulated-industry SME at a medical-device manufacturer.** ISO 14971 and IEC 62304 land on her desk. She needs every clinical-risk claim to trace to a controlled vocabulary. Studio's Ontology Sensei rail warns her when a draft note invents a term that collides with an existing OwlClass; the AI partner lane refuses to continue a draft that cites a retired skill; the share-to-mesh funnel routes her proposed vocabulary additions through the Policy Engine (ADR-045) before they ever reach the Broker. For Chen, Studio is the compliance harness that prevents casual authoring from creating casual vocabulary.

---

## 5. Technology Context

| Layer | Technology | Version | Notes |
|-------|-----------|---------|-------|
| Framework | React | 19 | Reuses PRD-002 concurrent features; no new framework dependency |
| Build | Vite | latest | Additional lazy-loaded chunk `/studio`; existing bundle strategy unchanged |
| Language | TypeScript | strict mode | Pod resource types generated from ADR-052 RDF shapes; skill manifests typed via `@anthropic/skills-v2` shape |
| Design System | Radix UI | v3 (`@radix-ui/themes` 3.2.1) | Reuses PRD-002 components; adds `SplitPane`, `GuidanceRail`, `SkillCard`, `ShareStateBadge`, `InboxRow` |
| Styling | Tailwind CSS | v4.1.7 | HSL tokens unchanged; Studio inherits cosmic/crystalline palette |
| State | Zustand + Immer | 5.x / 11.1.3 | Per-feature stores following PRD-002 pattern; no cross-store imports |
| Routing | react-router-dom | v7 | Extends PRD-002 router; five new routes under `/studio` (see §8) |
| Pod Client | Solid Pod client | from ADR-027 infrastructure | Read/write to `/private/`, `/public/`, `/shared/`, `/inbox/` per ADR-052 layout |
| Agent Bridge | MCP WebSocket | existing | Reuses current bridge; Studio adds a per-session channel for partner lane |
| Auth | NIP-07 + ADR-040 dual-tier | -- | Identical to PRD-002; Studio surfaces no new auth UI |
| 3D | React Three Fiber + Three.js | 9.5.0 / 0.183.0 | Embedded graph context panel in Studio shell; same constrained-viewport approach as PRD-002 §6.3 Decision Canvas |
| Animation | Framer Motion | 12.23.26 | Existing presets from `design-system/animations.ts` |
| Icons | Lucide React | 0.562.0 | New icons wired: `Sparkles` (AI partner), `Compass` (Sensei), `Package` (skill), `Inbox`, `Clock` (automation) |

No new backend technology is introduced. All new domain logic lives in the two new bounded contexts (BC18 Contributor Enablement, BC19 Skill Lifecycle) on the existing Rust + Actix + Neo4j stack and the existing Solid Pod infrastructure.

---

## 6. Four Pillars

The Contributor AI Support Stratum is structured as four pillars. Each pillar has its own user stories, capabilities, observable signals, explicit out-of-scope cuts, and measurement hooks. The pillars compose into the Contributor Studio surface family (§7) but are conceptually separable -- Admin can disable a pillar without collapsing the rest.

### 6.1 Sovereign Workspace (Contributor Studio)

**Intent**: Give a knowledge provider one multi-pane cockpit that is simultaneously connected to their pod (source of truth), the graph (shared world model), the AI partner lane (tactical reasoning), and the ontology (shared vocabulary).

**User stories**:
- *Rosa*: "When I open Studio I see my graph, the note I'm working on, the AI partner that already knows the thread, and suggestions that stop me inventing a word I already defined last year."
- *Idris*: "I need to hand off Studio to a new consultant on their first day and have them productive by lunch. One screen, not six tools."
- *Chen*: "Studio is where I do my regulated work; it has to prove its pedigree for every keystroke that touches the ontology."

**Capabilities**:
- Split-pane shell with resizable lanes: graph context, work lane, AI partner lane, ontology guide rail (details in §7.1).
- NIP-07 auth transparently propagates to the Solid Pod client, the MCP bridge, and the graph websocket; contributors never re-log.
- Deep-linking between graph selection and AI partner context: selecting a node populates partner context with that node's neighbourhood.
- Pod context chip showing current WAC policy (private / team / public) and unresolved mesh writes.
- Graceful offline: last-known graph state, inbox digest, skill metadata all render from local cache when pod or mesh is unavailable.

**Signals emitted**:
- `contributor.studio.opened`, `contributor.studio.workspace.saved`, `contributor.studio.pod.attached`, `contributor.studio.graph.deeplinked`.

**Out of scope**:
- Not a block editor. Not a WYSIWYG. Not a real-time collaborative editor. Not a terminal replacement.

**Measurement**: Activation rate, TTFR, session duration distribution, pane-usage histogram.

### 6.2 Mesh Dojo (decentralised skill sharing)

**Intent**: Make Anthropic Skills v2 discipline (create / install / eval / benchmark / share / retire) a first-class mesh operation, with skills living in contributor pods and discoverable through the Solid Type Index (ADR-029) rather than a central registry.

**User stories**:
- *Rosa*: "I want to publish a prompt-and-rubric pair for 'policy memo first draft' to my team without waiting on an admin."
- *Idris*: "I want my consultants to discover each other's engagement skills automatically, scoped to the client's sector."
- *Chen*: "I want to see exactly which skill produced the output that ended up in our design history, with eval evidence."

**Capabilities**:
- `SkillPackage` is a first-class aggregate in BC19 (Skill Lifecycle) with three share states: Private, Team, Mesh (details in §10).
- Skills live in `/public/skills/` (mesh), `/shared/skills/{team}/` (team), `/private/skills/` (private) in the pod; discoverable via Type Index entries per ADR-029.
- Install pulls a skill's manifest and fixtures into the installing contributor's `/private/skills/` as a linked copy with provenance back to the origin.
- Every skill has a required `SkillEvalSuite` before it can transition from Private to Team, and a `SkillBenchmark` record before Team to Mesh (details in §11).
- Retirement is a first-class state that does not delete the skill but prevents new installs and surfaces a migration banner to existing users.

**Signals emitted**:
- `skill.package.created`, `skill.version.published`, `skill.installed`, `skill.eval.run`, `skill.benchmark.completed`, `skill.shared`, `skill.retired`.

**Out of scope**:
- Not a paid marketplace. No ratings, no stars, no social proof UI in MVP (benchmark outcomes are the proof).
- Not cross-org federation. Skills cross org boundaries only via explicit Admin export in MVP.

**Measurement**: Skill reuse rate, skill retirement rate, time-from-publish-to-first-install, eval-run density per skill.

### 6.3 Ontology Sensei (proactive guidance)

**Intent**: Surface relevant ontology terms, past skills, and neighbour contributors' patterns at the moment a contributor is drafting, rather than requiring them to search. The mesh reaches out; the contributor does not reach in.

**User stories**:
- *Rosa*: "While I draft, show me the three existing terms that are conceptually adjacent to my current paragraph, with one-click snap-to."
- *Idris*: "When one of my consultants drafts a recommendation, tell them if the firm has already written this recommendation for a different client."
- *Chen*: "When I write 'clinical risk control', warn me if my phrasing conflicts with a ratified OWL class or a retired one."

**Capabilities**:
- Background synthesis over the contributor's pod `/private/agent-memory/episodic/` (ADR-030) plus recent graph state, producing a rolling context summary.
- `ontology_discover` MCP tool call on every significant edit boundary (configurable, default 5s idle), returning up to 3 ranked suggestions: OWL term, neighbour contributor's similar draft, or matching skill.
- Guidance rail renders suggestions inline; contributor accepts (snap-to), edits (diverge), dismisses (miss, optional reason), or mutes per context.
- Acceptance and edits feed the hit-rate KPI (§12.5); dismissal reasons feed skill retirement and ontology pruning.

**Signals emitted**:
- `sensei.suggestion.offered`, `sensei.suggestion.accepted`, `sensei.suggestion.edited`, `sensei.suggestion.dismissed`, `sensei.suggestion.muted`.

**Out of scope**:
- Not autonomous insertion. Sensei never edits the contributor's draft without an explicit snap-to action.
- Not a chat. Sensei suggestions are typed objects with known provenance, not generated text.
- No nagging. Suggestion rate is capped (default: <= 1 suggestion per 20s per pane, user-configurable).

**Measurement**: Hit rate (§12.5), suggestion latency p95, per-contributor mute ratio (leading indicator of nagware drift).

### 6.4 Pod-Native Automations (headless contributor agents)

**Intent**: Let contributors schedule agents that run inside the pod's own scope -- producing synthesised briefs, eval runs, digest emails -- that land in `/inbox/` for human review rather than silently committing to shared state.

**User stories**:
- *Rosa*: "Every Sunday night, synthesise the week's new public pages in my pod into a one-page brief."
- *Idris*: "Every Monday morning, cross-check last week's consultant deliverables against the firm's active skills and flag any that could have been reused."
- *Chen*: "Every overnight, re-run the eval suite for any skill that touches the regulated ontology, and tell me at 07:00 if any regressed."

**Capabilities**:
- Automation definitions in `/private/automations/` (JSON), scheduled via an extended TaskOrchestratorActor in BC18.
- Each automation runs as a scoped agent using the contributor's delegated Nostr keypair (ADR-040), with an explicit capability list (read X, write Y, call Z).
- Output lands in `/inbox/` (ADR-052 pod structure addition) and never in `/public/` without human review.
- Contributor reviews inbox item and either accepts (routes to Broker Workbench or applies locally), defers, or dismisses.

**Signals emitted**:
- `automation.defined`, `automation.scheduled`, `automation.run.started`, `automation.run.completed`, `automation.output.reviewed`, `automation.output.promoted`.

**Out of scope**:
- Not autonomous workflow execution. Automations produce briefs for humans, not commits.
- Not cross-pod. An automation reads and writes only inside the contributor's own pod (and reads team/public as the WAC allows).
- Not infinite. Automations have budget caps (time, tokens, $) enforced by the policy engine (ADR-045).

**Measurement**: Automation definition count, inbox review latency p95, promotion-from-inbox rate, budget-cap-hit rate.

---

## 7. Capability Catalogue (MVP surfaces)

The pillars (§6) compose into concrete surfaces. Subsections mirror the Insight Migration Loop PRD §5 style: story, signal, out-of-scope per capability.

### 7.1 Studio shell

- **Story**: "As a contributor I open Studio once and have all four lanes (graph, work, AI partner, Sensei) laid out with my last session state restored."
- **Signal**: Four-lane split-pane using `SplitPane` (new design system component). Lane states persisted to `/private/contributor-profile/studio-layout.json`. Lane pinning, collapse, and resize. Pod-context chip in the header.
- **Out of scope**: Arbitrary pane arrangements; only the four canonical lanes in MVP.

### 7.2 Ontology guide rail

- **Story**: "As a contributor I see up to three relevant ontology terms, skills, or neighbour patterns in a right-hand rail that updates as I move through my work."
- **Signal**: `GuidanceRail` component (new). Renders a max-3 suggestion stack; each suggestion has provenance (source OwlClass node, skill manifest, or neighbour pod pointer), an accept action, an edit action, and a dismiss-with-reason action. Rate-limited per §6.3.
- **Out of scope**: Free-text suggestions; suggestions are always typed objects.

### 7.3 AI partner lane

- **Story**: "As a contributor I have an MCP-backed conversational partner pre-loaded with the context of what I'm currently doing -- selected graph node, open note, recently reviewed inbox items."
- **Signal**: Chat-like UI built on existing MCP WebSocket infrastructure; system prompt assembled server-side by `ContextAssembly` service (BC18). Session transcripts stored in `/private/agent-memory/episodic/studio-partner/`.
- **Out of scope**: Not the product's centre of gravity; partner lane is collapsible and explicitly subordinate to the graph + Sensei + work lanes.

### 7.4 Graph deep-link

- **Story**: "As a contributor clicking a node in the graph pane sets the AI partner context, the Sensei suggestions, and the work-lane header to that node."
- **Signal**: Shared selection state in the `studioStore` (Zustand); all four lanes subscribe with fine-grained selectors. Round-trip latency target <100ms for local selection; <300ms when the selection triggers a pod fetch.
- **Out of scope**: Bidirectional selection from arbitrary external tools (v2).

### 7.5 Command palette extensions

- **Story**: "As a power user I reach every Studio capability from the existing `Cmd+K` palette."
- **Signal**: New command registrations via the ADR-046 `CommandRegistry` pattern: `Go to Studio`, `New Workspace`, `Install Skill`, `Publish Skill`, `Run Skill Eval`, `Open Inbox`, `Schedule Automation`, `Search Skills`, `Search Neighbours`.
- **Out of scope**: Custom per-user palette extensions (v2).

### 7.6 Skill discovery

- **Story**: "As a contributor I browse available skills by scope (team, mesh), tag, domain, and benchmark outcome."
- **Signal**: `/studio/:workspaceId/skills` route renders a filtered skill catalogue pulled from pod Type Index (ADR-029) + BC19 SkillRegistry service. `SkillCard` component displays manifest summary, share state, benchmark badge.
- **Out of scope**: Commercial discovery (ratings, marketing copy).

### 7.7 Skill install

- **Story**: "As a contributor I install a discovered skill and it becomes immediately available to my AI partner lane and my automations."
- **Signal**: One-click install writes a linked reference under `/private/skills/` with provenance pointing back to the origin pod + version. `SkillCompatibilityScanner` (BC19) verifies skill's required MCP tools and pod permissions are granted; surfaces a pre-install diff.
- **Out of scope**: Forking. Skills are referenced, not copied. Fork is a deliberate second action that creates a new SkillPackage in the contributor's pod.

### 7.8 Skill share

- **Story**: "As a power contributor I publish a skill from Private to Team, or from Team to Mesh, through a checklist-driven flow."
- **Signal**: Share button on skill detail surfaces the funnel (§10); each transition requires specific artefacts (eval suite for Team; benchmark + eval suite for Mesh). Share intent is recorded in BC18 as a `ShareIntent` aggregate; on Mesh, the intent becomes a Broker Workbench case.
- **Out of scope**: Direct pod-to-pod share without the funnel. Every transition is audited.

### 7.9 Skill eval

- **Story**: "As a skill author I define a `SkillEvalSuite` with fixtures and expected outcomes, and the mesh runs it."
- **Signal**: `/studio/skills/:skillId?tab=evals` renders the eval composer. Eval suites are versioned and pod-resident under `/private/skill-evals/`. Runs can execute locally (contributor pod) or mesh-side (SkillEvaluation service in BC19).
- **Out of scope**: Adversarial eval (red-teaming) as a separate capability in MVP; single harness for pass/fail eval.

### 7.10 Share-to-mesh funnel

- **Story**: "As a contributor I submit a workspace, skill, or insight to the mesh through an explicit gate that tells me exactly what the Policy Engine, Broker, and Migration Workflow need from me."
- **Signal**: Share funnel UI at `/studio/:workspaceId/share/new`; validates completeness (eval suite present, redaction check passed, confidence above threshold), surfaces the checks visibly, and on submit produces the appropriate artefact: a `MigrationPayload` (ADR-049), a `WorkflowProposal` (ADR-042), or a `ShareIntent` for Team scope.
- **Out of scope**: Implicit / silent promotion. Every mesh crossing is an explicit, audited action.

### 7.11 Inbox review

- **Story**: "As a contributor I open my inbox and review synthesised briefs, eval runs, and automation outputs delivered by my pod-native agents."
- **Signal**: `/studio/inbox` route reads `/inbox/` container from the pod; each item is an `InboxRow` with reviewed/deferred/dismissed actions. Promoted items route to the Broker Workbench or apply locally depending on type.
- **Out of scope**: Real-time inbox collaboration; inbox is single-contributor.

### 7.12 Automation scheduler

- **Story**: "As a contributor I define a scheduled automation with an explicit capability list and budget cap."
- **Signal**: `/studio/automations` renders existing definitions (from `/private/automations/`) and a composer for new ones. Each automation shows last run, next run, success rate, token spend. Budget-cap breach fires a policy event that the policy engine (ADR-045) can block.
- **Out of scope**: Visual workflow DAG authoring; automations are single-definition scheduled agents in MVP.

---

## 8. Navigation & Integration

Studio slots into the ADR-046 router as a sixth top-level surface. Existing surfaces (Graph, Broker, Workflows, KPIs, Connectors, Policy) are unchanged.

### 8.1 Routes

| Route | Purpose |
|-------|---------|
| `/studio` | Studio shell index; restores last workspace or prompts to create one |
| `/studio/:workspaceId` | Named workspace (saved split-pane layout + pinned graph selection + pinned pod context) |
| `/studio/:workspaceId/skills` | Skill catalogue (discovery + filters + installed list) |
| `/studio/:workspaceId/skills/:skillId` | Skill detail (manifest, version history, eval results, share controls) |
| `/studio/automations` | Automation list + composer |
| `/studio/inbox` | Pod inbox review queue |
| `/studio/:workspaceId/share/new` | Share-to-mesh funnel composer |

All routes are lazy-loaded via `React.lazy()` following the PRD-002 §14 pattern. The Studio chunk target is <60KB gzipped (skill catalogue and automations composer are sub-chunks).

### 8.2 Sidebar entry

Studio registers a new `SidebarNav` entry via the `registerNavEntry()` pattern (ADR-046). Positioned between Graph and Broker to reflect its "below the management mesh" architectural role:

| Section | Icon | Route | Badge |
|---------|------|-------|-------|
| Graph | `Network` | `/` | -- |
| **Studio** | **`Sparkles`** | **`/studio`** | **Inbox unread count (live)** |
| Broker | `Scale` | `/broker` | Open case count (live) |
| Workflows | `GitBranch` | `/workflows` | Pending review count |
| KPIs | `BarChart3` | `/kpi` | -- |
| Connectors | `Plug` | `/connectors` | Error count (if any) |
| Policy | `Shield` | `/policy` | -- |

Badge value driven by a lightweight `useInboxUnreadCount` selector over the `studioStore`.

### 8.3 Command palette

New commands registered by the Studio module's command hook (ADR-046 pattern):

| Command | Action |
|---------|--------|
| `Go to Studio` | Navigate to `/studio` |
| `New Workspace` | Open workspace creation dialog |
| `Install Skill` | Navigate to `/studio/:workspaceId/skills` with install filter |
| `Publish Skill` | Open skill share wizard for selected skill |
| `Run Skill Eval` | Execute current skill's eval suite |
| `Open Inbox` | Navigate to `/studio/inbox` |
| `Schedule Automation` | Navigate to `/studio/automations` with composer open |
| `Search Skills {query}` | Fuzzy skill search across accessible scopes |
| `Search Neighbours {query}` | Pod-graph neighbour pattern search |
| `Snap to term {term}` | Sensei-originated suggestion accept |

### 8.4 WebSocket channels

Studio subscribes to three new channels on the existing WebSocket infrastructure:

- `studio:sensei:{pubkey}` -- Sensei suggestions targeting the current contributor
- `studio:inbox:{pubkey}` -- new inbox items (from automations or mesh delivery)
- `studio:skills:events` -- skill publish/retire broadcasts for the current contributor's accessible scopes

Subscriptions follow the PRD-002 §12.3 pattern: subscribe on mount, unsubscribe on unmount, messages dispatched to `studioStore` via a channel-to-store router.

---

## 9. Data & Pod Model

The Contributor AI Support Stratum respects the ADR-052 WAC-gated pod layout and extends it with contributor-specific containers. Neo4j continues to hold the graph performance layer; pods hold sovereign truth; MCP/WebSocket state is transient.

### 9.1 Pod additions (extending ADR-052)

```
/
├── ...existing ADR-052 layout (profile/, private/, public/, shared/)...
│
├── private/
│   ├── contributor-profile/      (new, BC18)
│   │   ├── .acl                  owner-only
│   │   ├── identity.ttl          role, goals, collaborators, preferences
│   │   ├── studio-layout.json    last split-pane state per workspace
│   │   └── guidance-settings.ttl Sensei rate/mute/channel configuration
│   │
│   ├── automations/              (new, BC18)
│   │   ├── .acl                  owner-only
│   │   └── *.json                scheduled automation definitions
│   │
│   ├── skill-evals/              (new, BC19)
│   │   ├── .acl                  owner-only
│   │   └── {skillId}/
│   │       ├── runs/             per-run eval output with timestamps
│   │       └── benchmarks/       benchmark records
│   │
│   ├── skills/                   (new, BC19 -- private-scope skills)
│   │   ├── .acl                  owner-only
│   │   └── {skillId}/
│   │       ├── SKILL.md          manifest (Anthropic Skills v2 shape)
│   │       ├── fixtures/
│   │       └── versions/
│   │
│   └── agent-memory/episodic/studio-partner/   (BC18 -- consumes ADR-030 layout)
│       └── session-{timestamp}.ttl
│
├── shared/
│   ├── team/                     (ADR-052 placeholder; now populated)
│   │   ├── skills/               (BC19 -- team-scope skills)
│   │   └── workspaces/           (BC18 -- team-pinned workspaces)
│   └── [named-group ACLs per ADR-052 Wave 2]
│
├── public/
│   ├── skills/                   (new, BC19 -- mesh-scope skills)
│   │   ├── .acl                  foaf:Agent Read + owner Write/Control (inherited)
│   │   └── {skillId}/            identical shape to private, mesh-visible
│   │
│   ├── workspaces/               (new, BC18 -- mesh-visible workspace templates, opt-in)
│   │   └── *.ttl
│   │
│   └── [existing public/kg/ etc.]
│
└── inbox/                        (new, BC18)
    ├── .acl                      owner-only write/read; others Append via ADR-052 named groups
    └── *.ttl                     per-item inbox entries with source + status
```

Type Index entries (ADR-029) for the new containers are seeded at Studio first-run so that mesh peers can discover a contributor's public skill and workspace surfaces without knowing the pod layout up front.

### 9.2 Neo4j performance-layer additions

BC18 and BC19 write projections into Neo4j for graph-speed queries, never for source-of-truth storage. All projections carry a `pod_uri` back-reference that resolves to the pod-resident truth.

New node types:

| Node Type | Label | BC | Properties (summary) |
|-----------|-------|----|----------------------|
| ContributorWorkspace | `:ContributorWorkspace` | BC18 | id, name, contributor_pubkey, share_state, pinned_node_ids, pod_uri |
| GuidanceSession | `:GuidanceSession` | BC18 | id, contributor_pubkey, started_at, ended_at, suggestion_count, accept_count |
| ShareIntent | `:ShareIntent` | BC18 | id, source_type, source_id, from_state, to_state, status, created_at |
| WorkArtifact | `:WorkArtifact` | BC18 | id, artifact_type, contributor_pubkey, pod_uri, share_state |
| SkillPackage | `:SkillPackage` | BC19 | id, name, latest_version, share_state, author_pubkey, pod_uri |
| SkillVersion | `:SkillVersion` | BC19 | id, package_id, semver, created_at, retired (bool) |
| SkillEvalSuite | `:SkillEvalSuite` | BC19 | id, package_id, version_range, pass_count, fail_count |
| SkillBenchmark | `:SkillBenchmark` | BC19 | id, package_id, run_at, metrics_json |
| SkillDistribution | `:SkillDistribution` | BC19 | id, package_id, installed_by_pubkey, installed_at |

New relationships:

| Relationship | From | To |
|-------------|------|----|
| `OWNS_WORKSPACE` | EnterpriseUser | ContributorWorkspace |
| `PRODUCED` | ContributorWorkspace | WorkArtifact |
| `OFFERED_SUGGESTION` | GuidanceSession | OwlClass / SkillPackage / ContributorWorkspace |
| `SHARED_AS` | WorkArtifact / SkillPackage | ShareIntent |
| `PROMOTED_TO` | ShareIntent | BrokerCase / MigrationPayload / WorkflowProposal |
| `PUBLISHED` | EnterpriseUser | SkillPackage |
| `HAS_VERSION` | SkillPackage | SkillVersion |
| `BENCHMARKED_BY` | SkillVersion | SkillBenchmark |
| `EVALUATED_BY` | SkillVersion | SkillEvalSuite |
| `INSTALLED_BY` | SkillPackage | SkillDistribution |

All graph projections carry a `pod_uri` back-reference so the pod remains authoritative (ADR-052 §Consequences). Invalidation on pod writes uses the same Solid Notifications subscription pattern ADR-052 established for `/public/kg/`, extended to `/public/skills/`, `/public/workspaces/`, `/shared/skills/{team}/`, `/shared/workspaces/{team}/`, and `/inbox/`.

### 9.3 Transient state

| State | Location | Lifetime |
|-------|----------|----------|
| Current partner lane MCP session | Server memory + WebSocket | Until pane closes or idle timeout (30 min) |
| Current Sensei suggestion window | `studioStore` (client) | Until pane closes or route change |
| Selected graph node in Studio | `studioStore` | Until pane closes or selection changes |
| Studio layout hover/drag state | `studioStore` | Frame-by-frame; never persisted |

---

## 10. Share-to-Mesh Funnel

The share-to-mesh funnel is the bridge from the Contributor AI Support Stratum to the management mesh. It operationalises the three share states already referenced in §6 and §9.

### 10.1 Three share states

| State | Pod location | Visibility | Transition cost |
|-------|-------------|------------|-----------------|
| **Private** | `/private/skills/*`, `/private/automations/*`, session memory | Contributor only | None (default) |
| **Team** | `/shared/skills/{team}/*`, `/shared/workspaces/{team}/*` | Team (WAC named group, ADR-052 §shared) | `SkillEvalSuite` present + redaction check passed |
| **Mesh** | `/public/skills/*`, `/public/workspaces/*` | Open mesh | `SkillBenchmark` recorded + Policy Engine check passed + Broker approval (ADR-041 case) |

A single artefact never exists in two states at once. Transitions are pod MOVE operations (per ADR-052 symmetric publish/unpublish), co-ordinated by the `ShareOrchestrator` service (BC18) which records a `ShareIntent` aggregate as provenance.

### 10.2 Transition boundaries

**Private -> Team**:
- Eval suite present and passing against at least 1 fixture.
- Redaction pipeline (reuses BC16 infrastructure) scans for PII + corporate-private tokens.
- Team selector resolves to a `GroupMembership` the contributor is a member of (no cross-team writes from Studio; that is Admin's path).
- Policy Engine (ADR-045) evaluates under the policy rule `share_private_to_team` with the contributor's role, target team, and artefact type.

**Team -> Mesh**:
- `SkillBenchmark` record with pass/fail metrics.
- Eval suite passes deterministically across at least 3 fixtures.
- Policy Engine evaluates under `share_team_to_mesh`; failure routes to Broker as a policy exception.
- On Policy pass, `ShareOrchestrator` creates a `BrokerCase` with `category: "contributor_mesh_share"` (with `subject_kind` discriminator) and the artefact's payload. The Broker applies the same claim/decide machinery (ADR-041). For skill shares whose approval implies an ontology change, the Broker's decision additionally emits a `MigrationPayload` (ADR-049) into the existing migration workflow. For skill shares that imply a new reusable workflow pattern, the Broker's decision emits a `WorkflowProposal` (ADR-042).
- Only after Broker approval does the `SharePolicy` write to `/public/skills/*` (the pod MOVE).

### 10.3 Policy Engine hooks

The funnel defines three new policy rule types to be implemented in BC17 (policy rule surface defined in ADR-057 §C2, not here):

- `share_private_to_team` -- minimum eval coverage, redaction rule, role gate.
- `share_team_to_mesh` -- benchmark threshold, minimum eval count, role gate.
- `skill_retirement_request` -- gate on who can retire a Mesh-state skill (contributor self, Admin, or active-consumer threshold).

All three route Deny results to Broker as standard policy exceptions (ADR-045 §Escalate). No silent suppressions.

### 10.4 Rollback / un-share

Un-sharing is symmetric with sharing: a pod MOVE from `/public/skills/X` back to `/shared/skills/{team}/X` with a `ShareIntent` of direction `mesh_to_team`. Existing installs continue to resolve (pod redirects serve the moved resource from its new location; the Type Index entry is updated). A banner flags "this skill has been demoted" on any `SkillCard` showing an installed skill whose source share state has dropped.

---

## 11. Skill Lifecycle Discipline

This section is a pointer, not a spec. Full skill lifecycle specification lives in `docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md`. This section fixes the vocabulary.

Adopting Anthropic Skills v2 discipline:

1. **Create** -- Contributor authors a `SkillPackage` with a `SKILL.md` manifest and optional fixtures. State: Private.
2. **Install** -- Another contributor (or the same) links the skill into their `/private/skills/`. Install is idempotent and provenance-tagged.
3. **Eval** -- Before Team share, a `SkillEvalSuite` must exist and pass. Runs are pod-resident under `/private/skill-evals/{skillId}/runs/`.
4. **Benchmark** -- Before Mesh share, a `SkillBenchmark` record captures pass/fail across the eval suite under realistic-size workloads and records cost metrics.
5. **Share** -- State transitions governed by §10 funnel.
6. **Retire** -- A first-class state. Retired skills remain installed for existing consumers but prevent new installs. A retirement includes a migration note (what to use instead). Retirement is a `ShareIntent` with direction `retire`.

Both `SkillEvalSuite` and `SkillBenchmark` are first-class aggregates in BC19 (pod-resident, Neo4j-projected), not opaque blobs. Eval failure gates Team share; benchmark failure gates Mesh share. Retirement is reversible only by the original author or Admin.

---

## 12. KPIs and Observability

Extends the KPI Lineage Model (ADR-043). Each of the six KPIs declared in §2 is specified here with source events, calculation, target, and lineage.

### 12.1 Contributor Activation Rate

- **Source events**: `contributor.studio.opened`, `contributor.studio.pod.attached`, `sensei.suggestion.offered`, `sensei.suggestion.accepted|edited|dismissed`.
- **Calculation**: Over a rolling 7-day window per invited contributor, = 1 if the contributor (a) opened Studio, (b) attached a pod, (c) completed at least one GuidanceSession (offered + any action). Otherwise 0. Aggregate = mean across invited contributors.
- **Target**: 80% by end of Phase 4.
- **Lineage**: BC18 `GuidanceSession` aggregate + BC14 EnterpriseUser invitation records.

### 12.2 Time-to-First-Result (TTFR)

- **Source events**: `contributor.studio.opened` (first), and the earliest of {`workspace.saved`, `skill.installed`, `automation.defined`, `inbox.item.reviewed`}.
- **Calculation**: Per contributor, delta between first-open and first-artefact event. Aggregate = median across contributors whose first-open was in the reporting window.
- **Target**: <= 30 minutes.
- **Lineage**: BC18 `WorkArtifact` + `ContributorWorkspace` + `InboxReview` + BC19 `SkillDistribution`.

### 12.3 Skill Reuse Rate

- **Source events**: `skill.version.published`, `skill.installed`.
- **Calculation**: Over rolling 30 days: distinct `installed_by_pubkey` values ÷ distinct `SkillPackage.id` values where at least one version was published in the window (and author != installer).
- **Target**: >= 2.5.
- **Lineage**: BC19 `SkillDistribution` + `SkillPackage` projections; pod-resident truth in `/public/skills/*` Type Index entries.

### 12.4 Share-to-Mesh Conversion Rate

- **Source events**: `share.intent.created` (direction `team_to_mesh`), `share.intent.approved`, `share.intent.rejected`, `broker.case.decided { category: contributor_mesh_share }`.
- **Calculation**: Over rolling 30 days: approved mesh ShareIntents ÷ all mesh ShareIntents (approved + rejected + expired).
- **Target**: >= 15% conversion.
- **Lineage**: BC18 `ShareIntent` + BC11 `BrokerCase` + (for ontology-implicating shares) ADR-049 `MigrationPayload`.

### 12.5 Ontology Guidance Hit Rate

- **Source events**: `sensei.suggestion.offered`, `sensei.suggestion.accepted`, `sensei.suggestion.edited`, `sensei.suggestion.dismissed`, `sensei.suggestion.muted`.
- **Calculation**: Over rolling 7 days: (accepted + edited) ÷ (offered - muted). Muted suggestions do not count against the denominator because the contributor has explicitly opted out of that channel.
- **Target**: >= 40% accept-or-edit.
- **Lineage**: BC18 `GuidanceSession` aggregate; suggestion provenance recorded as `OFFERED_SUGGESTION` edges.

### 12.6 Redundant Skill Retirement Rate

- **Source events**: `skill.retired`, `skill.package.created` (via `SkillPackage` lifecycle).
- **Calculation**: Over a calendar quarter: retired skills ÷ active skills at start of quarter. A skill is "active" if it has non-zero installs and is in state Team or Mesh.
- **Target**: >= 8%/quarter.
- **Lineage**: BC19 `SkillPackage` + `SkillDistribution`. Retirement reason (migration note) is pod-resident under the retired skill's `SKILL.md`.

### 12.7 Lineage diagram

```mermaid
graph LR
    E1["contributor.studio.opened"] --> GS["GuidanceSession (BC18)"]
    E2["contributor.studio.pod.attached"] --> GS
    E3["sensei.suggestion.*"] --> GS
    GS --> KPI1["Activation Rate (12.1)"]
    GS --> KPI5["Hit Rate (12.5)"]

    E4["contributor.studio.opened (first)"] --> WA["WorkArtifact (BC18)"]
    E5["workspace.saved / skill.installed"] --> WA
    E6["automation.defined / inbox.reviewed"] --> WA
    WA --> KPI2["TTFR (12.2)"]

    E7["skill.installed"] --> SD["SkillDistribution (BC19)"]
    E8["skill.version.published"] --> SD
    SD --> KPI3["Reuse Rate (12.3)"]

    E9["share.intent.*"] --> SI["ShareIntent (BC18)"]
    E10["broker.case.decided"] --> SI
    SI --> KPI4["Conversion Rate (12.4)"]

    E11["skill.retired"] --> SP["SkillPackage (BC19)"]
    E12["skill.package.created"] --> SP
    SP --> KPI6["Retirement Rate (12.6)"]
```

All six KPIs render on the Mesh KPI Dashboard (PRD-002 §8) with a new dimension slicer `Contributor` populated from BC14 EnterpriseUser records; individual contributor drill-down is gated behind `Auditor` or `Admin` roles.

---

## 13. Phased Delivery

| Phase | Weeks | Scope | Success criterion |
|-------|-------|-------|-------------------|
| 0 | prereq | Close enterprise mesh wire/auth gaps identified in `docs/qe-enterprise-audit-report.md` §1 (5 HIGH security findings, auth middleware on all enterprise endpoints, server-side policy evaluation). Phase 0 is **not owned by this PRD** but is a hard dependency -- no Studio capability ships on unauthenticated endpoints. | qe-enterprise-audit-report Security score >= 80/100; all enterprise endpoints require `Broker`/`Contributor`/`Admin` role as appropriate. |
| 1 | 2 wks | Contributor Studio MVP shell + pod context + ontology rail + AI partner lane (read-only Sensei). New routes `/studio`, `/studio/:workspaceId`. BC18 aggregates `ContributorWorkspace` and `GuidanceSession`. Pod container creation for `/private/contributor-profile/` and `/inbox/`. Sidebar entry + 4 commands. | Rosa persona flow: open Studio, attach pod, receive >=1 Sensei suggestion, save a workspace. Activation Rate KPI instrumented and reporting. |
| 2 | 3 wks | Skill Dojo MVP + evals. Routes `/studio/:workspaceId/skills`, `/studio/:workspaceId/skills/:skillId`. BC19 aggregates `SkillPackage`, `SkillVersion`, `SkillEvalSuite`. Pod containers `/private/skills/`, `/private/skill-evals/`, `/shared/skills/{team}/`. Install, publish-to-team, eval composer + runner. | Idris persona flow: one consultant publishes a skill to team; a second consultant discovers, installs, evaluates it. Skill Reuse Rate KPI instrumented. |
| 3 | 3 wks | Share-to-mesh funnel (§10). Route `/studio/:workspaceId/share/new`. `ShareOrchestrator` service (BC18). `SkillBenchmark` aggregate (BC19). Policy Engine rules `share_private_to_team`, `share_team_to_mesh`, `skill_retirement_request`. Integration with ADR-041 BrokerCase with `category: contributor_mesh_share`. Integration with ADR-049 MigrationPayload and ADR-042 WorkflowProposal as optional outputs of Broker approval. | Chen persona flow: submit a skill that touches ontology from Team to Mesh; it lands on Broker as a case; Broker approves; MigrationPayload flows into Migration Broker Workflow; outcome visible in contributor's inbox. Share-to-Mesh Conversion Rate KPI instrumented. |
| 4 | 3 wks | Pod-Native Automations + proactive Sensei. Routes `/studio/automations`, `/studio/inbox`. BC18 `Automation` aggregate; TaskOrchestratorActor extension for scheduling; budget caps enforced by Policy Engine. Sensei switches from reactive (on-edit) to proactive (periodic synthesis). Rate-limit and mute infrastructure. | Rosa Sunday-night synthesis and Idris Monday-morning cross-check both land in inbox at the scheduled time, under budget, with provenance. Hit Rate + Retirement Rate KPIs reporting. |

Total: 2 + 3 + 3 + 3 = 11 weeks of Studio-scoped work, with Phase 0 gated separately on the QE audit remediation. Phase boundaries are acceptance gates (see §15).

---

## 14. Risks & Mitigations

| # | Risk | Mitigation |
|---|------|-----------|
| R1 | **Broker intake overload**. The share-to-mesh funnel creates a new case source for BC11; high-volume contributor publishing could flood the Broker Inbox. | Rate-limit per contributor (default 3 mesh share intents/24h, configurable); Policy Engine pre-screens (§10.3); failed pre-screen never reaches Broker. Monitor Broker Inbox size as a leading indicator; raise rate limit only after observed backlog stays low for 2 weeks. |
| R2 | **Skill drift**. A skill publishes, gets installed, then its author modifies it without eval re-run; consumers are on an unsafe version. | SkillVersion is immutable per semver; new versions are new versions. Install pins a specific version. Eval suite must pass against the installed version; consumers see a warning if their pinned version fails newer eval runs. |
| R3 | **Pod-to-Neo4j cache incoherence**. Contributor writes to pod directly (third-party Solid client) without going through Studio; Neo4j projection lags. | Reuses ADR-052 Solid Notifications subscription mechanism on the new containers (`/public/skills/`, `/public/workspaces/`, `/shared/skills/{team}/`, `/shared/workspaces/{team}/`, `/inbox/`). 60 s polling fallback per ADR-052. Admin dashboard exposes per-user notification health. |
| R4 | **Eval gaming**. An author games eval fixtures to secure a green badge without real quality. | Benchmark runs mesh-side (SkillEvaluation service in BC19) against a held-out fixture set per skill domain; mesh-side benchmark results are signed by the service, not by the author. Broker sees both author-asserted eval results and mesh-side benchmark results side-by-side. |
| R5 | **WAC misconfiguration leaks**. A contributor accidentally marks private content as `public:: true` and it lands in `/public/skills/`. | Double-gate already enforced by ADR-052 (page flag AND container path). Share-to-mesh funnel adds a third gate: explicit Contributor confirmation + Policy Engine check + (for Mesh) Broker approval. Any single gate failure blocks publication. |
| R6 | **Sensei becomes nagware**. Excessive suggestions erode contributor trust; mute ratio rises; activation plateaus. | Per-pane rate limit (default <= 1 suggestion/20s); per-context mute; global "quiet hours" in guidance settings. Mute ratio tracked as a reverse-indicator KPI; if it exceeds 30% org-wide, Sensei suggestion rate auto-halves until it drops. Default mute remembered per context type (graph-focused, prose-focused, skill-focused). |
| R7 | **CLI power-users bypass Studio**. Power contributors write directly to their pod and the MCP bridge, leaving Studio's activation and reuse metrics permanently low. | Studio never hides CLI/MCP; both write paths emit the same signal events (`skill.version.published`, `automation.run.started`, etc.) as the Studio UI. Activation Rate measures pod attach + at least one GuidanceSession, not Studio pane-hours, so CLI-preferring users still count if they have ever reached the shell. |
| R8 | **Budget-cap gaming**. A contributor splits a large automation into many small ones to evade Policy Engine budget caps. | Budget caps are enforced per-contributor-rolling-window, not per-automation. Policy rule `automation_budget_total` aggregates across all automations for a given pubkey. Surface breaches on the inbox with a throttling notice. |
| R9 | **Retirement abandonment**. No one retires skills; the library accretes; the Retirement Rate KPI drifts below target. | Admin retirement queue surfaces skills with zero installs in 90 days and benchmark age > 180 days; retirement is one click from the queue. Positive KPI reporting on retirement rate as a cultural signal ("we prune well"). |
| R10 | **Offline pod failure silently hides work**. Contributor's pod provider outage; Studio runs on cached reads; writes fail silently. | Studio surfaces pod-connection status in the header chip; writes require live pod and fail loudly with a retry queue. Inbox review items written while pod is offline stay in a local pending queue flushed on reconnect; UI shows a "pending pod write" badge on each affected row. |
| R11 | **Sensei sustained background cost**. Nudge frequency at scale blows the Tier-2 inference budget; `sensei_nudge` composition becomes the dominant token spend. | Per-contributor nudge budget cap (default 40 Tier-2 calls/day); Sensei degrades to Tier-1 ontology-distance heuristics when budget exhausted; cap and actual spend surface in the Admin Studio settings pane; `sensei.budget.exhausted` is a reverse-KPI. |
| R12 | **Compatibility Scanner fan-out back-pressure**. On a model-routing config change (§8.4 trigger 1), the Scanner re-benchmarks every installed Personal + Team skill across every affected contributor's pod; concurrency blows past the MCP rate limit and the mesh benchmark runner stalls. | Scanner runs in bounded-concurrency queue (default 8 parallel benchmarks); config changes ship behind a feature flag with a planned-rollout schedule; per-contributor re-benchmark quota caps 10 skills/hour; Admin dashboard surfaces queue depth and ETA. |
| R13 | **Inbox retention and quota explosion**. Automations, Sensei, and Broker notifications all write to `/inbox/`; a power user accumulates thousands of unread items; pod storage quota trips; new automation output is silently dropped. | Hard per-namespace retention (default 500 items or 30 days, whichever first); overflow triggers `inbox.quota.approached` event and an inbox-cleanup wizard; dropped items land in `/inbox/.dlq/` not silent delete; pod-provider quota surfaced in Workspace bar. |
| R14 | **WebSocket publish fan-out at scale**. One contributor with many open Studio tabs + Sensei + inbox + automation subscriptions creates multiple concurrent `/api/ws/studio` sessions; org-wide fan-out from `context_updated` broadcasts saturates the relay. | Per-WebID WebSocket connection cap (default 4 concurrent `/api/ws/studio` sessions); broadcast topics are scoped to `workspace_id` (not WebID); rate-limit chat and nudge channels separately; session excess returns 429 with retry-after. |

---

## 15. Acceptance Criteria

Phase 1 (Studio MVP shell) ships when all of the following are demonstrably true. Detailed Gherkin-style scenarios in `docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature`.

- [ ] A new Contributor can navigate to `/studio`, attach a pod, and see a split-pane shell with graph / work / partner / Sensei lanes laid out and persisted to `/private/contributor-profile/studio-layout.json` (§7.1).
- [ ] `SidebarNav` shows a Studio entry between Graph and Broker with live inbox-unread badge (§8.2).
- [ ] At least 10 commands are registered to the command palette (§8.3).
- [ ] The Ontology Sensei rail offers at least one suggestion per session for a corpus of >= 50 pod-resident pages (§7.2 + §6.3).
- [ ] Selecting a graph node populates the AI partner context within <= 300 ms (§7.4).
- [ ] Pod containers `/private/contributor-profile/`, `/private/automations/`, `/private/skills/`, `/private/skill-evals/`, `/inbox/` are created at first Studio login with ACLs matching §9.1 and inherit-ACL behaviour per ADR-052.
- [ ] Neo4j projections `ContributorWorkspace` and `GuidanceSession` exist, carry `pod_uri` back-references, and invalidate on pod Solid Notifications.
- [ ] Activation Rate, TTFR, and Hit Rate KPIs emit events and appear on the Mesh KPI Dashboard with a `Contributor` dimension slicer.
- [ ] Auth, authorisation, and role gating are identical to PRD-002 and ADR-040 (no new auth surface).
- [ ] Offline-degraded mode: disconnecting from the pod shows a visible warning chip; the shell continues to render last-known graph state and inbox digest.
- [ ] `feature_acceptance_studio_phase1.spec.ts` passes end-to-end in CI.

Phase 2, 3, 4 acceptance gates are specified in the design docs `02-skill-dojo-and-evals.md`, `03-pod-context-memory-and-sharing.md`, and `04-acceptance-tests.feature` respectively.

---

## 16. Open Questions

Flagged for resolution by companion artefacts (ADR-057, BC18/BC19 DDD doc, design specs 00-03):

1. **Team-scoped skill discovery without doxxing membership**. If a skill under `/shared/skills/{team}/` is listed on a contributor's Type Index, does that leak team membership to peers who can read the profile? **Open**: ADR-057 §D4 to define whether `/shared/skills/{team}/` Type Index entries are access-gated or live in a separate named-group-only Type Index.
2. **How does a retired skill still serve history**. *Resolved 2026-04-20*: retirement MOVEs the skill from `/public/skills/{slug}/` to `/public/skills/{slug}/archive/` per design spec 02 §10. This preserves historical read access for audit + past installers (archive inherits the parent `/public/` WAC `foaf:Agent` Read) while removing the skill's `urn:solid:AgentSkill` entry from the public Type Index so it is no longer discoverable for new installs. A `retirement.jsonld` sidecar records who retired, when, why, and the replacement pointer.
3. **Offline automation auth**. A scheduled automation runs while the contributor is offline and their ephemeral Nostr delegation (ADR-040) has expired. **Open**: ADR-057 §D6 to define whether automations use a longer-lived refresh delegation, a service-principal alternate key, or simply queue until the contributor reauthenticates.
4. **What happens when a contributor's pod is down**. Writes queue locally (per R10) but reads of other contributors' pods may also fail (e.g. resolving a Team-scope skill whose authoring pod is offline). **Open**: ADR-057 §D7 to define read-side caching policy and the TTL for cached Mesh / Team skills.
5. **Broker ontology expertise for skill-share cases**. Not every Broker has the ontology chops to approve a skill that introduces new OWL terms (same open question as ADR-049 §Open 3, inherited here). Routing rule needed.
6. **Cross-team skill discovery**. A contributor in Team A wants to discover a Mesh-state skill authored by Team B; the skill's `/public/skills/` entry is mesh-visible but does the contributor see *all* mesh skills or only those their Group claims permit? **Open**: ADR-057 §D8 to decide between "all mesh skills visible, filter by contributor preference" and "mesh skills gated by a published-language policy rule".

Additional questions for parallel artefact authors to resolve in their own documents:

- **Design spec 01 (Studio surface)**: Exact pane-split geometry at <=1280px viewport widths. This PRD assumes desktop; design spec owns the breakpoints.
- **Design spec 02 (Skill Dojo)**: Concrete `SKILL.md` frontmatter schema. This PRD commits to Anthropic Skills v2 shape but the exact allowed keys are a design-spec-02 deliverable.
- **DDD doc (BC18/BC19)**: Cardinality and invariants for `ShareIntent` (can a single artefact have multiple simultaneous in-flight intents? MVP says no, DDD doc confirms).
- **ADR-057 §E**: Runtime allocation between a new `ContributorEnablementActor` and reuse of existing BC14 EnterpriseUser infrastructure.

---

## 17. References

Internal ADRs:
- ADR-027: Pod-Backed Graph Views
- ADR-029: Type Index Discovery
- ADR-030: Agent Memory Pods
- ADR-040: Enterprise Identity Strategy
- ADR-041: Judgment Broker Workbench Architecture
- ADR-042: Workflow Proposal Object Model
- ADR-043: KPI Lineage Model
- ADR-045: Policy Engine Approach
- ADR-046: Enterprise UI Architecture
- ADR-048: Dual-Tier Identity Model
- ADR-049: Insight Migration Broker Workflow
- ADR-052: Pod Default WAC + Public Container
- ADR-057: Contributor Enablement Platform (companion, authored in parallel to this PRD)

Internal PRDs:
- PRD-001: VisionFlow Data Pipeline Alignment
- PRD-002: Enterprise Control Plane UI
- PRD: Insight Migration Loop (MVP)

Internal explanation docs:
- `docs/explanation/ddd-enterprise-contexts.md` (BC11-BC17; BC18 and BC19 extended in the companion DDD doc `ddd-contributor-enablement-context.md`)
- `docs/explanation/contributor-support-stratum.md` (architectural overview of the new stratum)

Design specs (all dated 2026-04-20):
- `docs/design/2026-04-20-contributor-studio/00-master.md`
- `docs/design/2026-04-20-contributor-studio/01-contributor-studio-surface.md`
- `docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md`
- `docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md`
- `docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature`
- `docs/design/2026-04-20-contributor-studio/evidence-annex.md` (industry evidence citations)

QE:
- `docs/qe-enterprise-audit-report.md` (Phase 0 prereqs)

External:
- Anthropic Skills v2 lifecycle discipline (create / install / eval / benchmark / share / retire) -- cited in `evidence-annex.md`
- PwC 2026 leadership survey, McKinsey AI Manifesto, a16z Institutional AI thesis, Ramp Glass post-mortem -- cited in `evidence-annex.md`
