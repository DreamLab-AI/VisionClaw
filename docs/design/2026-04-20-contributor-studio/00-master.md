---
title: Contributor Studio — Design Sprint Master
description: Master index for the 2026-04-20 Contributor Studio design sprint implementing PRD-003 and ADR-057. Links evidence annex, surface spec, skill dojo, pod memory, acceptance tests.
category: design
tags: [contributor-studio, design, master, 2026-04-20]
updated-date: 2026-04-20
---

# Contributor Studio — Design Sprint Master (2026-04-20)

## 1. Scope

This sprint designs the product surface for the **Contributor AI Support Stratum**
ratified by PRD-003 and ADR-057: a new architectural layer that sits above the
VisionClaw substrate (BC1–BC10, BC14, BC30) and below the management mesh
(BC11–BC17), hosting two new bounded contexts — BC18 Contributor Enablement
(core) and BC19 Skill Lifecycle (supporting). The sprint produces the
specifications required to build Phase 1–4 of PRD-003 §13: the Contributor
Studio shell with four lanes (graph / work / AI partner / Sensei), the Mesh
Dojo skill lifecycle surface with Anthropic v2-discipline evals and
benchmarks, the pod-first share funnel (Private → Team → Mesh) with Broker
handoff, the Pod-Native Automations inbox, and the acceptance tests that
prove all of the above.

Out of scope for this sprint (per PRD-003 §3 and ADR-057 Decision):
Notion/Obsidian-style block editing; chat-first UX; Broker Workbench
replacement; federated multi-tenant mesh; mobile; swarm orchestration console;
write-side offline-first; a Studio-only auth surface; an in-browser skill
authoring IDE (deferred per design 02 §2); and automatic SKILL.md harvesting
from raw transcripts. Phase 0 enterprise-mesh auth/wire remediation from
`docs/qe-enterprise-audit-report.md` is a **hard dependency** owned outside
this sprint.

## 2. Sprint artifacts

| # | File | Purpose | Status |
|---|------|---------|--------|
| 00 | `00-master.md` (this) | Index, decisions log, open questions | Active |
| 01 | `01-contributor-studio-surface.md` | React surface, router, palette, panes | Drafted (peer) |
| 02 | `02-skill-dojo-and-evals.md` | Skill lifecycle, Dojo, evals, benchmarks | Drafted |
| 03 | `03-pod-context-memory-and-sharing.md` | Pod layout, memory, share funnel, WAC | Drafted (peer) |
| 04 | `04-acceptance-tests.feature` | Gherkin scenarios | Drafted (peer) |
| — | `evidence-annex.md` | Industry research (PwC, McKinsey, a16z, Ramp, Anthropic) | Complete |

## 3. Upstream strategy documents

| Doc | Role |
|-----|------|
| `docs/PRD-003-contributor-ai-support-stratum.md` | Product requirements |
| `docs/adr/ADR-057-contributor-enablement-platform.md` | Architecture decision |
| `docs/explanation/ddd-contributor-enablement-context.md` | BC18/BC19 domain model |
| `docs/explanation/contributor-support-stratum.md` | Architectural narrative |

## 4. Dependencies

### 4.1 Hard dependencies (Phase 0 prerequisites)

- Close enterprise mesh gaps in `docs/qe-enterprise-audit-report.md`:
  auth middleware on all enterprise endpoints, server-side policy evaluation,
  persistence/wire fixes. Studio endpoints inherit this middleware; Phase 1
  will not ship on unauthenticated endpoints.
- ADR-046 router + sidebar + command-registry extensions already merged.
- ADR-052 Pod Default WAC (private-by-default + `/public/` container) enforced.
- ADR-040 NIP-07 flow and OIDC-to-ephemeral-Nostr dual-tier identity stable.
- ADR-027 Pod-Backed Graph Views cache-coherence mechanism available for
  new containers (`/public/skills/`, `/shared/skills/`, `/inbox/`, etc.).
- ADR-029 Type Index discovery available as the skill registration primitive.
- ADR-030 Agent Memory Pods `/agents/{id}/memory/` pattern available.
- Policy Engine (ADR-045) rule-set extension available; BC17 can accept the
  six new rules enumerated in ADR-057 §Share-State Transition Rules.

### 4.2 Soft dependencies (Phase 1+ integration)

- ADR-041 Broker Workbench — Mesh-candidate skill reviews route here via
  the canonical `contributor_mesh_share` case category (with `subject_kind` discriminator).
- ADR-042 WorkflowProposal — skill promotion producing a cross-role workflow
  pattern emits a `WorkflowProposal`.
- ADR-043 KPI Lineage — six new contributor KPIs extend the BC15 metric
  catalogue (see PRD-003 §12 and `contributor-support-stratum.md` §Measurement).
- ADR-045 Policy Engine — every share-state transition is a policy evaluation.
- ADR-048 Dual-Tier Identity Model — artefact promotion preserves the
  KG-vs-ontology tier distinction in `BRIDGE_TO` edges.
- ADR-049 Insight Migration Broker Workflow — ontology-implicating shares
  fold into the existing MigrationPayload path.
- ADR-026 3-Tier Model Routing — Sensei nudges land on Tier 2 Haiku;
  eval suite drafting, mesh-gate benchmarks and semantic grading on Tier 3
  Sonnet; deterministic assertions and index queries on Tier 1 Agent Booster.
- ADR-047 WASM visualisation components — the Broker Decision Canvas "Skill
  Preview" widget uses this substrate (design 02 §11).
- ADR-044 Connector Governance — `do-not-publish` tool flags block mesh
  promotion (design 02 §14).

## 5. Cross-cutting design principles

1. **Workspace, not chat window.** Split-pane cockpit with persistent layout
   per contributor — Ramp Glass C7 precedent. Design 01 §2 enforces this;
   chat is a lane inside the workspace, never the frame around it.
2. **Pod-first, backend-derived.** The contributor's Solid Pod is the
   write-master for profile, workspaces, skills, evals, automations,
   and inbox items. Neo4j and backend actors index for performance only.
   ADR-052 WAC + ADR-027 cache-coherence extend to every new container;
   design 03 inherits this end-to-end.
3. **Progressive disclosure.** A new contributor hits a working four-lane
   Studio with Sensei active and a seeded inbox on day 1 (PRD-003 §10
   §15 acceptance gate; Ramp Glass C4, C7). Power-user depth (eval
   composer, benchmark history, retirement queue) is behind additional
   routes — never on the critical path of first activation.
4. **Keyboard-first.** The ADR-046 `CommandRegistry` gets 10+ new commands
   (PRD-003 §8.3, design 01 §4 and §11). No primary action in the Studio
   requires a pointer.
5. **Observable by default.** Every significant event — workspace open,
   suggestion offered/accepted/dismissed, share intent created, skill
   installed, automation run, broker decision — emits to BC15 with
   lineage back to the `GuidanceSession` that produced it. All six
   contributor KPIs in PRD-003 §12 are wired before their phase ships.
6. **Governed sharing, monotonic.** Three states Private → Team → Mesh,
   strictly forward. Every transition is an explicit `ShareIntent`,
   evaluated by BC17, and (for Mesh) a `BrokerCase`. Skip-ahead is
   forbidden; rollback is a new, audit-visible `ShareIntent`.
7. **Model-routed by tier.** Every AI call declares its ADR-026 tier.
   Sensei nudges Tier 2; SKILL.md draft Tier 2; eval suite draft Tier 3;
   semantic grader Tier 3; deterministic assertions and index queries
   Tier 1 Agent Booster. No "LLM-only" path.
8. **Sovereign by default.** Pod owns truth; a contributor walking away
   still has their `/public/` and `/private/` containers intact. No
   contributor artefact is born on the backend.

## 6. Design decisions log

| Date | Decision | Doc | Rationale |
|------|----------|-----|-----------|
| 2026-04-20 | `/studio` is a peer route of `/broker`, not a palette on `/graph` | ADR-057 Opt 4 rejection; design 01 | Produces-vs-reviews role separation; canvas cannot carry a workspace (ADR-046 precedent) |
| 2026-04-20 | Two new bounded contexts BC18 (core) + BC19 (supporting) rather than extending BC11 | ADR-057 Opt 2 rejection; DDD doc | Broker alert fatigue; SRP; contributor and broker have different latency and permission envelopes |
| 2026-04-20 | Server-side actor topology required; not a pure client-side shell | ADR-057 Opt 3 rejection | Share-state invariants, KPI emission, sandboxed eval runs, notification subscriptions all need server-side residency |
| 2026-04-20 | Skills are pod-backed `SKILL.md` directories with Type Index registration | Design 02 §3–§5 | Sovereign-first; ADR-029 discovery; Ramp Glass C5 compounding pattern |
| 2026-04-20 | Three share states strictly monotonic (Private → Team → Mesh; no skip) | ADR-057 §Share-State Transition Rules; PRD-003 §10 | Prevents unreviewed mesh publication; matches BC17 policy rules and ADR-052 WAC containers |
| 2026-04-20 | Eval suites mandatory for Team promotion; benchmarks mandatory for Mesh promotion | Design 02 §8.3; PRD-003 §11 | Anthropic v2 discipline; C9/C10 evidence |
| 2026-04-20 | Retirement is a first-class lifecycle state, not deletion | Design 02 §10; ADR-057 state machine | Audit + rollback + installed-copies keep working; `BaseModelAbsorbed` signal (C10) |
| 2026-04-20 | Contributor metrics extend BC15 KPI lineage (ADR-043); not a separate pipeline | PRD-003 §12; `contributor-support-stratum.md` §Measurement | Governance consistency; six named KPIs with source-event traceability |
| 2026-04-20 | Sensei rate-limited (≤1 suggestion per 20 s per pane, mute per context) | PRD-003 §6.3, §14 R6; DDD `NudgeEnvelope` | Ramp Glass "not nagware"; mute ratio is a reverse indicator |
| 2026-04-20 | Automations live in `/private/automations/`; output only to `/inbox/` | PRD-003 §6.4, §9.1; ADR-057 §Pod Layout | No autonomous mesh writes; contributor reviews before any promotion |
| 2026-04-20 | Share-to-mesh produces a `BrokerCase` of category `contributor_mesh_share` with a `subject_kind` discriminator (`skill` \| `work_artifact` \| `ontology_term` \| `workflow` \| `graph_view`); `subject_kind=ontology_term` further delegates to ADR-049 `migration_candidate` downstream on approve | ADR-057 Integration Points | Reuses ADR-041 case-category pattern; single canonical category simplifies broker routing + KPI queries |
| 2026-04-20 | New MCP tools scoped under `skill_*`, `studio_*`, `sensei_*`, `share_intent_*`, `automation_*`, `inbox_*` | ADR-057 §MCP Tool Additions; design 02 §5.1 | Namespacing agreed pending Open Q1 |
| 2026-04-20 | `SkillVersion` immutable once Benchmarked; fixes ship as new versions | Design 02 §4 inv. 1; DDD BC19 inv. 1 | Promoted patterns reference a version that cannot silently change |
| 2026-04-20 | Install uses linked reference + version pin, not silent auto-update | Design 02 §7; PRD-003 §7.7 | Reproducibility; updates are explicit `skill_install` with higher pinned version |
| 2026-04-20 | Pod MOVE (not COPY) is the transport for Private↔Team share transitions | PRD-003 §10.1; ADR-057 §Share-State table | Enforces "single artefact, single state" invariant (BC18 inv. 1) |

## 7. Phased delivery mapping

| Phase | Scope | Design sections delivering it |
|-------|-------|-------------------------------|
| 0 (prereq) | Enterprise mesh auth/wire gap closure | `docs/qe-enterprise-audit-report.md` (external to this sprint) |
| 1 (2 wks) | Studio MVP shell + pod context + ontology rail + AI partner lane | 01 §2 (shell), §3 (router), §4 (palette), §6 (Sensei rail), §7 (partner lane); 03 §2 (pod containers), §3 (profile + layout), §4 (WAC bootstrap); 04 Phase-1 scenarios |
| 2 (3 wks) | Skill Dojo + evals (Personal/Team) | 02 §3–§8 end-to-end; 01 §7 (Dojo UI route); 03 §7 (Team share state + `/shared/skills/`); 04 Phase-2 scenarios |
| 3 (3 wks) | Share-to-mesh funnel + Broker integration | 03 §7 (Mesh transitions + WAC MOVE); 02 §11 (broker integration + Skill Preview widget); 01 §9 (share-funnel composer at `/studio/share/new`); 04 Phase-3 scenarios |
| 4 (3 wks) | Automations + proactive Sensei + retirement discipline | 03 §5 (automations + `/inbox/`), §6 (24h memory synthesis); 01 §6 (Sensei proactive mode + mute ratio surfacing); 02 §10 (retirement), §8.4 (compatibility scanner); 04 Phase-4 scenarios |

## 8. Open questions (consolidated)

Aggregated from PRD-003 §16, ADR-057 Open Questions, DDD `Open Questions`,
evidence-annex `Gaps`, and design 02 §16. Numbering keeps the originating
document visible so that resolution can flow back to the right owner.

### 8.1 Security & WAC

- **Q1** — Do `/shared/skills/{team}/` Type Index entries on a contributor's profile
  leak team membership to peers who can read the profile? Resolution:
  ADR-057 §D4; may require access-gated Type Index or a named-group-only
  Type Index (PRD-003 §16.1).
- **Q2** — WAC misconfiguration escalation: does the share funnel's
  triple-gate (contributor confirm + Policy Engine + Broker) fully cover
  ADR-052 double-gate failure modes, or do we need a Studio-side pre-flight
  redaction preview? (PRD-003 §14 R5.)
- **Q3** — Inbox retention and GDPR: `/inbox/` accumulates automation
  outputs that may contain third-party personal data. Retention policy
  owner — BC18 or BC16 redaction pipeline? ADR-052 (contributor-owned) and
  ADR-041 (audit append-only) conflict for inbox-held PII. (DDD §Open 4.)
- **Q4** — Skill-level PII: a local calibration run in
  `/private/skills/.../local-evals.jsonl` may reveal sensitive prompts
  even when only pass-rate is projected. Dojo-side metrics opt-in, or
  leak-only-pass-rate acceptable? (Design 02 §16.6.)

### 8.2 Skill lifecycle

- **Q5** — Retirement preserves audit: does retirement MOVE to
  `/private/archived-skills/` (authors retain ownership, read access gated)
  or leave in place with a flag? (PRD-003 §16.2; design 02 §10 assumes
  MOVE to `archive/` — reconcile.)
- **Q6** — Skill version retirement with in-flight `WorkflowPattern`s (BC12)
  that still reference the version: grace window + pattern refresh, or
  immediate pattern-level retirement propagation? `successor_ref` exists
  but propagation semantics to BC12 are unspecified. (DDD §Open 3.)
- **Q7** — Forward-referencing tools: may a skill declare a tool the target
  instance has not yet installed? Publish with warning or refuse? Current
  assumption is permit + install-time prerequisite. (Design 02 §16.1.)
- **Q8** — Skill dependencies: formal `depends_on` field vs embedded
  dependency copy. Proposed compromise: declare at publish, resolve at
  install with pinned versions. Revisit after Phase 4. (Design 02 §16.2.)
- **Q9** — Preventing eval gaming: is a third-party meta-eval required
  for mesh promotion to validate assertion diversity and trigger-accuracy
  split (Anthropic v2 description-optimisation loop)? (Design 02 §16.3.)
- **Q10** — Benchmark cost attribution: publisher's, installer's, or a
  platform pool? Connects to rate-limit policy and admin cost visibility.
  Related: `SkillCompatibilityScanner` re-runs — per-skill rate limit or
  fire on every upstream change? (ADR-057 §Open 4; design 02 §16.5.)
- **Q11** — Benchmark baseline staleness on tier crossings (Haiku → Sonnet):
  auto-rerun or maintainer-owned? (DDD §Open 6.)
- **Q12** — Cross-tenant trust: at what point do we require NIP-05 /
  NIP-39 domain verification before a skill is eligible for public mesh
  promotion? Nostr keys alone are anonymous. (Design 02 §16.4.)

### 8.3 Share funnel

- **Q13** — Broker ontology expertise for skill-share cases: not every
  Broker is ontology-competent; skills introducing new OWL terms need a
  routing rule (inherited from ADR-049 Open 3). (PRD-003 §16.5.)
- **Q14** — Cross-team skill discovery: does a Team-A contributor see
  **all** Mesh skills or only those a published-language policy permits?
  (PRD-003 §16.6; ADR-057 §D8.)
- **Q15** — Mesh retirement automation: drift detector → auto-retire after
  N firings, auto-open a `BrokerCase` for retirement review, or only
  surface a Sensei nudge? Current model is ambiguous. (ADR-057 §Open 5.)
- **Q16** — Suggestion-acceptance as graph mutation: emit ADR-048
  `BRIDGE_TO` candidate edge immediately on `SuggestionAccepted`, or only
  on Team/Mesh share? Early emission pollutes with private intent; late
  emission delays discovery. (DDD §Open 2.)

### 8.4 Automation

- **Q17** — Offline automation auth: scheduled automation runs while the
  contributor is offline and the ephemeral Nostr delegation (ADR-040) has
  expired. Longer-lived refresh delegation, service-principal alternate
  key, or queue-until-reauth? (PRD-003 §16.3.)
- **Q18** — Delivery guarantees: at-most-once, at-least-once, or
  exactly-once delivery of automation briefs to `/inbox/`? Exactly-once
  needs idempotency keys on Pod writes. (ADR-057 §Open 2.)
- **Q19** — Per-contributor storage quotas on `/private/skill-evals/`,
  `/private/workspaces/`, `/inbox/` — cap size and apply polling/notification
  health checks per container? (ADR-057 §Open 6.)
- **Q20** — Budget-cap gaming: default cap is per-contributor-rolling-window
  aggregated across all automations under one pubkey. Is that aggregation
  sufficient, or do we also need per-routine caps to catch adversarial
  splitting? (PRD-003 §14 R8.)

### 8.5 Observability

- **Q21** — Sensei tuning feedback loop: per-contributor BC18-local
  relevance model, or shared BC19-supporting model? Per-contributor is
  easier; shared compounds faster at the cost of a training pipeline.
  (ADR-057 §Open 3.)
- **Q22** — Sensei fatigue back-off scope: global back-off on repeated
  dismissal, or per-focus-class? Global risks starving new-focus
  suggestions; focus-class risks overload on well-trodden contexts.
  (DDD §Open 7; PRD-003 §14 R6.)
- **Q23** — No external benchmark for the six contributor KPIs (contributor
  activation, TTFR, skill reuse, share-to-mesh conversion, guidance hit
  rate, retirement rate). Should PRD-003 §2 targets be labelled "targets
  to establish" rather than "targets to hit" until 90 days of in-production
  data exists? (Evidence annex §Gaps 4; PRD-003 §2.)
- **Q24** — Read-side caching policy for mesh/team skills when the
  authoring pod is offline: TTL and invalidation model. (PRD-003 §16.4;
  ADR-057 §D7.)

### 8.6 Integration

- **Q25** — MCP tool naming convention and ownership: `studio_*` vs
  `skills_*` vs a mix; ratify with MCP conventions owner before Phase 1
  lands. (ADR-057 §Open 1.)
- **Q26** — `ContributorEnablementActor` vs reuse of existing BC14
  EnterpriseUser actor infrastructure for role-gated endpoints.
  (PRD-003 §16 additional; ADR-057 §E.)
- **Q27** — Workspace identity across devices: one `ContributorWorkspace`
  or two when a contributor opens Studio concurrently on laptop and phone?
  Concurrency conflict model needed. (DDD §Open 1.)
- **Q28** — Multi-partner orchestration invariants: a `GuidanceSession`
  binding both an AI partner and a human collaborator — whose lineage
  root does the resulting artefact claim when contributions are material?
  (DDD §Open 5.)
- **Q29** — Design-spec-01 owns the pane-split geometry at ≤1280px;
  design-spec-02 owns the concrete `SKILL.md` frontmatter schema
  (Anthropic v2 shape fixed, exact allowed keys TBC); DDD owns
  `ShareIntent` concurrency invariants (MVP says "no simultaneous
  in-flight intents per artefact", confirm). (PRD-003 §16 additional.)

## 9. Risks not covered by PRD-003 §14

| # | Risk | Source | Status / Resolution |
|---|------|--------|---------------------|
| DR1 | **Spec-02 vs PRD-003 skill namespace nomenclature drift.** PRD-003 uses `SkillPackage / SkillVersion / SkillEvalSuite / SkillBenchmark / SkillDistribution` (five aggregates). Design 02 §15 also surfaces a "preference vs capability" split from Anthropic v2 (evidence C9) that BC19 does not model. | Comparison of PRD-003 §11 + design 02 §15 + DDD BC19 | **Resolved 2026-04-20.** Design 02 §4 updated: the `capability.*` / `preference.*` prefix in the `category` enum is descriptive metadata on the `SkillPackage` aggregate, **not** a separate BC19 aggregate dimension or invariant. All skills go through `SkillBenchmark`. |
| DR2 | **Distribution scope mismatch.** Design 02 §9 defines **four** scopes — Personal / Team / Company / Public-Mesh. PRD-003, ADR-057, and DDD BC18 ValueObjects all define **three** share states — Private / Team / Mesh. | Cross-check PRD-003 §10.1 vs design 02 §9 | **Resolved 2026-04-20.** Design 02 §9 updated: the four distribution scopes are pod-layout refinements of the three canonical BC18 `ShareState` values. `Personal` → `Private`; `Team` and `Company` both → `Team` (differing only in WAC group membership breadth); `Public (Mesh)` → `Mesh`. ShareState remains canonical at the aggregate level. |
| DR3 | **Retirement archive path inconsistency.** Design 02 §10 writes retirement to `/public/skills/{slug}/archive/`. PRD-003 §16.2 proposes `/private/archived-skills/` as an option. DDD §BC19 entities does not specify a path. | PRD-003 §16.2 vs design 02 §10 | **Resolved 2026-04-20.** PRD-003 §16 Q2 closed: retirement MOVEs skill to `/public/skills/{slug}/archive/` per design 02 §10 (preserves historical read access + removes Type Index entry). ADR-057 §D5 closed; Q2 now informational. |
| DR4 | **Broker case category naming inconsistency.** ADR-057 named case `mesh_skill_review`; design 02 §11 `skill_promotion`; DDD ACL 7 `share_review`. Same intent, three names — will fragment broker routing and KPI queries. | ADR-057 §Share-State, design 02 §11, DDD ACL 7 | **Resolved 2026-04-20.** Canonical category is **`contributor_mesh_share`** with `subject_kind` discriminator (`skill` \| `work_artifact` \| `ontology_term` \| `workflow` \| `graph_view`). ADR-057, DDD, and design 03 updated. Design 02 line 756 `case_type: skill_promotion` is a local field inside the broker payload, not the BC11 category — left as-is. `subject_kind=ontology_term` further delegates to ADR-049 `migration_candidate` downstream on approve. |
| DR5 | **Sensei model cost.** Phase-4 proactive Sensei runs periodically over `/private/agent-memory/episodic/` for every active contributor. If every five seconds of idle time triggers a Tier 2 Haiku call, cost grows linearly with contributor count and attention span. | ADR-057 Consequences (Nagware), PRD-003 §14 R6 | **Promoted 2026-04-20.** Added to PRD-003 §14 as **R11** with per-contributor nudge budget cap (default 40 Tier-2 calls/day), Tier-1 degradation when budget exhausted, and a `sensei.budget.exhausted` reverse-KPI. |
| DR6 | **Skill Compatibility Scanner back-pressure.** Design 02 §8.4 fires on every ADR-026 tier mapping change or MCP tool version bump. A single tier change can queue hundreds of benchmark re-runs instantaneously. | Design 02 §8.4 + §16.5 | **Promoted 2026-04-20.** Added to PRD-003 §14 as **R12** with bounded-concurrency queue (8 parallel benchmarks), feature-flagged rollout schedule, per-contributor re-benchmark quota (10 skills/hour), queue-depth surfaced in Admin dashboard. |
| DR7 | **Inbox size / long-tail.** `/inbox/` writes from automations have owner-only visibility but no specified retention or archival policy. | PRD-003 §9.1, ADR-057 Pod layout, DDD Q4 | **Promoted 2026-04-20.** Added to PRD-003 §14 as **R13** with hard per-namespace retention (500 items or 30 days), overflow triggers `inbox.quota.approached`, dropped items land in `/inbox/.dlq/`, pod-provider quota surfaced in Workspace bar. |
| DR8 | **WebSocket channel fan-out on Mesh publish.** A publish spike fan-outs to O(contributor × skill) deliveries. | PRD-003 §8.4, design 02 §12 | **Promoted 2026-04-20.** Added to PRD-003 §14 as **R14** with per-WebID connection cap (4 concurrent `/api/ws/studio` sessions), topic-scoped broadcasts (`workspace_id`, not WebID), separate rate limits per channel, 429 + retry-after on excess. |

## 10. Review checklist (for sign-off)

- [ ] All four design docs (01, 02, 03, 04) complete and internally consistent
- [x] DR1–DR4 resolved 2026-04-20 via inline edits to PRD-003, ADR-057, DDD, design 02, design 03
- [x] DR5–DR8 promoted to PRD-003 §14 risk register as R11–R14 on 2026-04-20
- [ ] §8 open questions routed to owners; Q1/Q13/Q25 resolved before Phase 1
      gate (auth/discovery/naming block the MVP surface)
- [ ] Acceptance test coverage matrix in `04-acceptance-tests.feature` maps
      ≥90 % of PRD-003 §7 capabilities and all six §12 KPIs
- [ ] Every new MCP tool listed in ADR-057 has a call-site in design 01, 02,
      or 03 (including `studio_context_assemble`, `sensei_nudge`,
      `share_intent_create`, `skill_publish`, `skill_install`,
      `skill_eval_run`, `automation_schedule`, `inbox_ack`)
- [ ] Every BC18/BC19 domain event in the DDD has an emit-site referenced
      in a design doc
- [ ] Every new pod container (`/private/contributor-profile/`,
      `/private/automations/`, `/private/skills/`,
      `/private/skill-evals/`, `/private/workspaces/`,
      `/public/skills/`, `/public/workspaces/`, `/shared/skills/`,
      `/shared/skills/{team}/`, `/shared/workspaces/{team}/`, `/inbox/`)
      has an ACL template in design 03
- [ ] Phase 0 prerequisites signed off by the enterprise-audit owner
      (auth middleware on all enterprise endpoints, Security score ≥ 80)
- [ ] Security review of WAC transitions complete (design 03 STRIDE)
- [ ] KPI lineage diagram in PRD-003 §12.7 reconciled with BC15 model
      (ADR-043) — `Contributor` dimension slicer present on the Mesh KPI
      Dashboard
- [ ] Policy Engine rules `share_private_to_team`, `share_team_to_mesh`,
      `skill_retirement_request`, `automation_budget_total`,
      `automation_inbox_write`, `automation_public_write`,
      `partner_binding_scope`, `share_skill_widening` all defined in
      BC17 rule-set `contributor-stratum-v1`
- [x] Mesh-promotion `BrokerCase` category collapsed to canonical `contributor_mesh_share` with `subject_kind` discriminator (DR4 resolved)
- [x] Scope vocabulary reconciled: four distribution scopes are pod-layout refinements of three canonical BC18 ShareStates (DR2 resolved)

## 11. References

### Internal (this sprint)

- `docs/PRD-003-contributor-ai-support-stratum.md`
- `docs/adr/ADR-057-contributor-enablement-platform.md`
- `docs/explanation/ddd-contributor-enablement-context.md`
- `docs/explanation/contributor-support-stratum.md`
- `docs/design/2026-04-20-contributor-studio/01-contributor-studio-surface.md`
- `docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md`
- `docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md`
- `docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature`
- `docs/design/2026-04-20-contributor-studio/evidence-annex.md`

### Internal (upstream)

- `docs/PRD-002-enterprise-ui.md` (PRD-003 extends with sixth surface family)
- `docs/prd-insight-migration-loop.md` (personas continued)
- `docs/explanation/ddd-enterprise-contexts.md` (BC1–BC17 context map)
- `docs/qe-enterprise-audit-report.md` (Phase 0 prerequisites)

### ADRs cited by the sprint

ADR-026 (3-Tier Model Routing), ADR-027 (Pod-Backed Graph Views),
ADR-029 (Type Index Discovery), ADR-030 (Agent Memory Pods),
ADR-034 (Needle Bead Provenance), ADR-040 (Enterprise Identity),
ADR-041 (Judgment Broker Workbench), ADR-042 (Workflow Proposal),
ADR-043 (KPI Lineage), ADR-044 (Connector Governance),
ADR-045 (Policy Engine), ADR-046 (Enterprise UI Architecture),
ADR-047 (WASM Visualisation Components), ADR-048 (Dual-Tier Identity),
ADR-049 (Insight Migration Broker Workflow),
ADR-052 (Pod Default WAC + Public Container),
ADR-057 (Contributor Enablement Platform — this sprint).

### External sources (full fidelity notes in `evidence-annex.md`)

- PwC 2026 AI Performance Study (C1) — `presentation/2026-04-20-best-companies-ai/01-pwc-2026-ai-performance-study.md`
- McKinsey AI Transformation Manifesto (C2) — `…/02-mckinsey-ai-transformation-manifesto.md`
- a16z *Institutional AI vs Individual AI* (C3, C8) — `…/03-a16z-institutional-vs-individual-ai.md`
- Ramp Glass (C4–C7) — `…/04-ramp-glass-seb-goddijn.md` + `…/04b-ramp-glass-shane-buchan-how-we-built-it.md`
- Anthropic Skill Creator v2 (C9, C10) — `…/90-anthropic-skill-creator-v2.md`
