---
title: Contributor Studio — Integration Validation Report
description: Cross-document consistency audit across all 12 sprint artifacts. Run 2026-04-20 after DR1-DR8 reconciliation. Read-only; no source files were modified.
category: design
tags: [contributor-studio, validation, qe, 2026-04-20]
updated-date: 2026-04-20
---

# Integration Validation Report

## 1. Scope

Artifacts checked (12 total):

- `docs/PRD-003-contributor-ai-support-stratum.md`
- `docs/adr/ADR-057-contributor-enablement-platform.md`
- `docs/explanation/ddd-contributor-enablement-context.md`
- `docs/explanation/contributor-support-stratum.md`
- `docs/design/2026-04-20-contributor-studio/00-master.md`
- `docs/design/2026-04-20-contributor-studio/01-contributor-studio-surface.md`
- `docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md`
- `docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md`
- `docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature`
- `docs/design/2026-04-20-contributor-studio/evidence-annex.md`
- `docs/design/2026-04-20-contributor-studio/99-sprint-close.md` (present; checked)
- `docs/design/2026-04-20-contributor-studio/diagrams.md` (NOT present at audit time — parallel agent still writing; not blocking)

Validations executed: V1-V10 per sprint-close plan `contributor-nexus-sprint-close-plan-2026-04-20`.

## 2. Summary

| # | Validation | Result | Issues |
|---|------------|--------|--------|
| V1 | Canonical BrokerCase category (`contributor_mesh_share`) | PARTIAL | 1 residual (legacy `share_review` in narrative) |
| V2 | Canonical pod paths | PARTIAL | 3 residuals (`/shared/team/…` drift in master + PRD-003) |
| V3 | Canonical route paths | FAIL | Significant PRD-003 route drift from design-01 canonical |
| V4 | Canonical KPI names | PARTIAL | KPI event-name vocabulary drifts across three docs |
| V5 | BC18/BC19 aggregate names | PASS | No alternate spellings found |
| V6 | MCP tool names | PASS | All tools use canonical names consistently |
| V7 | ShareState transition rules | PARTIAL | `Private → Mesh` described (ADR-057) vs. "monotonic only" (DDD) |
| V8 | Companion ADR coverage | PASS | All required ADRs present in both PRD-003 and ADR-057 |
| V9 | Evidence-annex C1-C10 citation coverage | PASS | All 10 claims cited outside the annex |
| V10 | Acceptance-test coverage | PARTIAL | R3 marked "partial"; R11-R14 not covered |

## 3. Detailed findings

### V1 Canonical BrokerCase category

Canonical: `contributor_mesh_share` with `subject_kind` discriminator. Found in:

- `docs/adr/ADR-057-contributor-enablement-platform.md`
- `docs/design/2026-04-20-contributor-studio/00-master.md`
- `docs/explanation/ddd-contributor-enablement-context.md`
- `docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md`

Legacy tokens searched: `mesh_skill_review`, `share_review`, `MeshSkillCandidate`.

Allowed occurrences (documentation context):
- `docs/design/2026-04-20-contributor-studio/00-master.md:292` — DR4 resolution row documents the rename (allowed per spec).

Residual issues:
- `docs/explanation/contributor-support-stratum.md:325` — narrative prose uses the old token: *"The broker sees the intent in their inbox as a `share_review` case later that day."* This is the only non-documentation occurrence of a legacy name.

### V2 Canonical pod paths

Canonical pattern for team containers per `docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md:111-113`: `/shared/skills/{team}/`, `/shared/workspaces/{team}/`, `/shared/memory/{team}/`.

Acceptance tests (`04-acceptance-tests.feature`) consistently use `/shared/team-research/skills/…` which is a valid instantiation of the `{team}` slug pattern.

Residual issues (team-prefix inversion):
- `docs/design/2026-04-20-contributor-studio/00-master.md:317` — review-checklist row lists `/shared/team/skills/`, `/shared/team/workspaces/` — the `team/skills` ordering inverts the canonical `skills/{team}` ordering.
- `docs/design/2026-04-20-contributor-studio/00-master.md:166` — Q1 references `/shared/team/` as a container (informal; ambiguous).
- `docs/PRD-003-contributor-ai-support-stratum.md:470, 502, 611` — table rows and retirement prose reference `/shared/team/skills/*`, `/shared/team/workspaces/*`, `/shared/team/skills/` using the inverted ordering.
- `docs/PRD-003-contributor-ai-support-stratum.md:448` — text lists `/shared/team/` (rather than `/shared/skills/`, `/shared/workspaces/`, `/shared/memory/`).

Historical/allowed: `/private/archived-skills/` appears only in 00-master:186 (Q2 historical option) and 00-master:291 (DR3 resolution). No other occurrences — compliant.

### V3 Canonical route paths

Canonical per `docs/design/2026-04-20-contributor-studio/01-contributor-studio-surface.md:81-91`:
```
/studio, /studio/new, /studio/:workspaceId,
/studio/:workspaceId/artifacts/:aid, /studio/:workspaceId/skills,
/studio/:workspaceId/skills/dojo, /studio/:workspaceId/sensei,
/studio/automations, /studio/automations/new, /studio/automations/:id,
/studio/inbox
```

Non-canonical routes found in PRD-003:
- `docs/PRD-003-contributor-ai-support-stratum.md:251, 302, 334, 611` — `/studio/skills` (top-level, not nested under `:workspaceId`).
- `docs/PRD-003-contributor-ai-support-stratum.md:269, 303, 611` — `/studio/skills/:skillId` (top-level).
- `docs/PRD-003-contributor-ai-support-stratum.md:275, 306, 644` and `docs/design/2026-04-20-contributor-studio/00-master.md:155` — `/studio/share/new` (entire route absent from canonical list).

Minor param-name variance (not a route drift, but inconsistent identifier):
- `docs/design/2026-04-20-contributor-studio/01-contributor-studio-surface.md:131, 232` — uses `/studio/:id/sensei` vs. canonical `/studio/:workspaceId/sensei` (same file).

Deep-link pattern in PRD-003 (`artifacts/:aid`) is not enumerated in PRD-003 §8 but appears in design-01 — not technically a drift, but PRD-003 §8 route table is incomplete relative to design-01.

### V4 Canonical KPI names

The validation brief lists canonical snake-case event names: `contributor_activated`, `skill_reused`, `share_to_mesh_converted`, `sensei_hit`, `sensei_dismissed`, `redundant_skill_retired`. Only `contributor_activated` literally appears anywhere:

- `docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md:1153` — appears in the content-free KPI event table.

None of `skill_reused`, `share_to_mesh_converted`, `sensei_hit`, `sensei_dismissed`, `redundant_skill_retired` appear in any artifact under those spellings.

Actual emitted-event vocabularies, which differ across three documents:
- PRD-003 §12 uses dot-notation: `contributor.studio.opened`, `sensei.suggestion.offered`, `skill.installed`, `share.intent.created`, `broker.case.decided { category: share_to_mesh }`, `skill.retired`.
- Design 03 §12 uses snake_case (partial): `contributor_activated`, `share_intent_created`, `sensei_suggestion_outcome`, `inbox_item_triage`, `skill_distribution_update`.
- Sprint-close §9 mixes both and lists K1-K6 with human-readable KPI names plus snake_case emitter tokens.

Reverse-KPIs (R11-R14) are dot-notation and consistent across PRD-003 §14 and 99-sprint-close.md §9:
- `sensei.budget.exhausted` (PRD-003:633, 99-sprint-close:233).
- `scanner.queue.depth` (99-sprint-close:234).
- `inbox.quota.approached` (PRD-003:635, 99-sprint-close:235).
- `ws.session.over` (99-sprint-close:236).

### V5 Canonical BC18/BC19 aggregate names

All canonical aggregates appear consistently across 11 of 12 files (397 total occurrences). No variant spellings detected for `SkillPackage`, `SkillVersion`, `SkillEvalSuite`, `SkillBenchmark`, `SkillDistribution`, `ContributorWorkspace`, `GuidanceSession`, `WorkArtifact`, `ShareIntent`, `ContributorProfile`.

Value objects: `ShareState`, `SkillLifecycleState` are consistent. `WorkspaceFocus`, `GuidanceSuggestion`, `ArtifactLineage`, `PartnerBinding` appear where expected (DDD BC18/BC19 sections).

Read-model names (`SkillIndex`, `SkillRegistry`, `SkillIndexProjector`, `SkillRegistrySupervisor`) are services/actors, not aggregates — appear consistently.

### V6 MCP tool names

All canonical MCP tools appear with consistent spelling:
- `skill_publish`, `skill_install`, `skill_eval_run` (not `skill_evals_run`), `studio_context_assemble`, `sensei_nudge`, `share_intent_create`, `automation_schedule`, `inbox_ack`, `studio_run_skill`.

ADR-057 Open Question 1 (line 598-601) explicitly flags MCP tool-naming finalisation as a pre-Phase-1 decision; current usage is consistent pending that decision.

### V7 ShareState transition rules

- `docs/explanation/ddd-contributor-enablement-context.md:292-293` states: *"ShareIntent transitions are monotonic: Private → Team → Mesh. No downward transitions except via explicit ContributorRevocation, which emits…"*
- `docs/adr/ADR-057-contributor-enablement-platform.md:447-448` states: *"A ShareIntent cannot skip a state. Private → Mesh transitions still create the intermediate Mesh-candidate BrokerCase."*
- `docs/explanation/ddd-contributor-enablement-context.md:847` scenario-diagram line renders *"Raise ShareIntent (Private → Mesh)"* as a single step.
- Acceptance test `docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature:332` explicitly asserts *"Contributor cannot skip Private -> Mesh directly"*.

Tension: DDD forbids state-skipping. ADR-057 and DDD Scenario C describe a `Private → Mesh` *intent* that the orchestrator still routes through Mesh-candidate. The acceptance test treats the skip as forbidden. The three documents can be reconciled (the `Private → Mesh` intent is a UX affordance; the orchestrator internally creates the staged intermediate), but the language in ADR-057:447 is ambiguous and reads as legitimising the skip.

`BrokerRevocation` was named in the validation brief but does not appear in any artifact; `ContributorRevocation` is the only revocation path named (DDD §BC18 + §Events at line 521). Not a contradiction, but a gap vs. the brief's canonical list.

### V8 Companion ADR coverage

- `docs/PRD-003-contributor-ai-support-stratum.md:7-8` "Depends On" lists ADR-027, 029, 030, 040, 041, 042, 045, 046, 049, 052; "Companion ADRs" lists ADR-057. Required by brief: ADR-040, 041, 045, 046, 049, 052, 057 — all present.
- `docs/adr/ADR-057-contributor-enablement-platform.md:579-594` "Related ADRs" lists ADR-026, 027, 029, 030, 034, 040, 041, 042, 043, 045, 046, 048, 049, 052 — all 14 required by brief present.

### V9 Evidence-annex C1-C10 citation coverage

Citations outside the annex:
- C1: 99-sprint-close.md:160, 299.
- C2: 99-sprint-close.md:161, 300; 04-acceptance-tests.feature:124, 133.
- C3: 99-sprint-close.md:162, 301; 04-acceptance-tests.feature:cross-ref table.
- C4: 99-sprint-close.md:44, 163, 302; 04-acceptance-tests.feature:46, 56, 807.
- C5: 99-sprint-close.md:44, 59, 164, 302; 02-skill-dojo-and-evals.md:28, 877, 935; 04-acceptance-tests.feature:190, 198, 808.
- C6: 99-sprint-close.md:44, 165, 302; 04-acceptance-tests.feature:697, 809.
- C7: 99-sprint-close.md:44, 166, 302; 04-acceptance-tests.feature:46, 56, 810.
- C8: 99-sprint-close.md:44, 167, 301; 04-acceptance-tests.feature:124, 133, 811.
- C9: 99-sprint-close.md:48, 61, 168, 303; 02-skill-dojo-and-evals.md:31, 936; 04-acceptance-tests.feature:190, 198, 259, 267, 812.
- C10: 99-sprint-close.md:48, 61, 62, 169, 303; 02-skill-dojo-and-evals.md:937; 04-acceptance-tests.feature:19, 813.

All ten claims cited in at least one non-annex file. PRD-003 itself does not contain inline `C1`-`C10` citations — it cites `evidence-annex.md` as a reference rather than inline identifiers (evidence-annex:224 proposes inline text for PRD-003 §12 but that insertion has not landed in the PRD).

### V10 Acceptance-test coverage

The acceptance-test coverage map (`04-acceptance-tests.feature:700-833`) is comprehensive. Analysis:

- **Four pillars**: mapped via tags `@contributor-studio` (Pillar 6.1), `@ontology-sensei` (6.3), `@skill-dojo` (6.2), `@automations` (6.4) — headers at lines 13-18. Pass.
- **Six KPIs**: `@kpi-12.1..@kpi-12.6` tags applied at Features 1, 2, 3, 5, 7, 9. Pass.
- **Risks R1-R10**: R1-R2, R4-R10 mapped. R3 "Pod-to-Neo4j cache incoherence" marked "partial" in the map at line 765 (only Feature 1 offline degradation touches it). Partial.
- **Risks R11-R14** (promoted from DR5-DR8): no `@risk-R11`, `@risk-R12`, `@risk-R13`, `@risk-R14` tags found anywhere in the feature file. Gap.
- **Share-state transitions**: Feature 5 (Share-to-Mesh) covers Private→Team, Team→Mesh, skip-rejection, broker revocation, PII block, and rate limit. Pass.
- **STRIDE invariants** (per design-03 §8 STRIDE): Feature 8 `@security @cross-phase @stride` covers STRIDE classes; explicit STRIDE-class mapping in the feature-file header at lines 498-500. Pass.

## 4. Residual issues

Concrete file:line citations requiring action:

1. `docs/explanation/contributor-support-stratum.md:325` — legacy `share_review` in narrative.
2. `docs/design/2026-04-20-contributor-studio/00-master.md:317` — `/shared/team/skills/`, `/shared/team/workspaces/` (team-prefix inversion).
3. `docs/design/2026-04-20-contributor-studio/00-master.md:166` — `/shared/team/` informal reference in Q1.
4. `docs/PRD-003-contributor-ai-support-stratum.md:448` — `/shared/team/` (informal).
5. `docs/PRD-003-contributor-ai-support-stratum.md:470` — `/shared/team/skills/*`, `/shared/team/workspaces/*` (team-prefix inversion).
6. `docs/PRD-003-contributor-ai-support-stratum.md:502` — `/shared/team/skills/X` (un-share description).
7. `docs/PRD-003-contributor-ai-support-stratum.md:611` — Phase-2 roadmap row uses `/shared/team/skills/`.
8. `docs/PRD-003-contributor-ai-support-stratum.md:251,269,302,303` — routes `/studio/skills` and `/studio/skills/:skillId` drift from design-01 canonical `/studio/:workspaceId/skills` and `/studio/:workspaceId/skills/:skillId`.
9. `docs/PRD-003-contributor-ai-support-stratum.md:275,306` — route `/studio/share/new` absent from design-01 canonical route list.
10. `docs/PRD-003-contributor-ai-support-stratum.md:527-567` (§12) and `docs/design/2026-04-20-contributor-studio/03-pod-context-memory-and-sharing.md:1149-1160` (§12) — KPI event-name vocabulary diverges (dot-notation vs. snake_case). No single canonical mapping table.
11. `docs/adr/ADR-057-contributor-enablement-platform.md:447-448` — ambiguous wording about `Private → Mesh` that reads as legitimising the skip rather than framing it as a UX affordance internally staged.
12. `docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature` — no scenarios tagged `@risk-R11`, `@risk-R12`, `@risk-R13`, `@risk-R14` for the four DR-promoted risks.
13. `docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature:765` — R3 coverage self-declared "partial"; no dedicated scenario for pod-to-Neo4j cache incoherence.
14. `docs/design/2026-04-20-contributor-studio/diagrams.md` — not present at audit time; in-flight parallel work.

## 5. Recommendations

**Blocking (must fix before Phase 1 kickoff):**

- (8), (9): PRD-003 §8 route table must be reconciled with design-01 canonical — either PRD-003 adopts the `/:workspaceId`-nested routes, or design-01 documents top-level `/studio/skills` and `/studio/share/new` as intentional exceptions. PRD-003 is the contract the router lives against; drift here breaks Phase-2 implementation scope.
- (2), (5), (6), (7): team-path ordering (`/shared/skills/{team}/` vs. `/shared/team/skills/`) must be resolved identically across PRD-003 and 00-master. Design-02 and design-03 are canonical (team slug is the leaf). Touches Phase-2 pod-container creation.
- (11): ADR-057 §Share-State invariants wording needs a one-line clarification that `Private → Mesh` is a UX affordance, not a state skip.

**Cosmetic (fix in next touch):**

- (1): swap `share_review` for `contributor_mesh_share` in contributor-support-stratum.md narrative.
- (3), (4): disambiguate Q1 and §9.2 `/shared/team/` references.

**Follow-up tasks:**

- (10): add a canonical KPI emitter-name table to either PRD-003 §12 or ADR-057 §KPIs, reconciling dot-notation (PRD) vs. snake_case (design-03). Track as ADR-057 addendum §D3 per 99-sprint-close §11.
- (12): add R11-R14 scenarios to `04-acceptance-tests.feature` (Sensei budget exhaustion, Scanner back-pressure, Inbox quota, WS session cap). Track in Phase 4 instrumentation work per 99-sprint-close §9.
- (13): convert R3 coverage from "partial" to "full" by adding a dedicated Feature 11 or Feature 12 scenario for pod-write-to-Neo4j eventual-consistency.
- (14): note in 99-sprint-close §3 that `diagrams.md` render is a parallel work item; this report will be re-run once present.

**Intentional and documentable:**

- V5 aggregate consistency; V6 MCP tool names; V8 ADR coverage; V9 evidence-annex coverage: no action.

## 6. Sign-off

- [ ] All V1-V10 pass → sprint artifacts internally consistent (currently 5/10 pass, 4/10 partial, 1/10 fail; **NOT YET** fully consistent).
- [ ] All residual issues have follow-up tasks or ADR amendments (14 residuals enumerated; blocking vs. cosmetic split identified).
