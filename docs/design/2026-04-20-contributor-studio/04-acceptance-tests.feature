# File: docs/design/2026-04-20-contributor-studio/04-acceptance-tests.feature
# Companion to: PRD-003 (Contributor AI Support Stratum), ADR-057 (Contributor
#               Enablement Platform), design specs 00-master / 01-contributor-
#               studio-surface / 02-skill-dojo-and-evals / 03-pod-context-memory-
#               and-sharing, and the evidence annex claims C1..C10.
# Date: 2026-04-20
# Style: Gherkin (Cucumber / pytest-bdd compatible). Scenarios are intended to be
#        independently runnable. Tags map to PRD capabilities, KPIs, and risks
#        (see coverage matrix at the bottom of this file).
#
# Tagging convention:
#   @phase-1..@phase-4           — maps to PRD-003 §13 phased delivery
#   @contributor-studio          — primary Studio shell scenarios (Pillar 6.1)
#   @ontology-sensei             — Ontology Sensei scenarios (Pillar 6.3)
#   @skill-dojo                  — Skill Dojo discovery/install/publish (Pillar 6.2, BC19)
#   @skill-evals                 — Eval suite / benchmark gating (design-02 §8, BC19)
#   @share-to-mesh               — Share funnel (PRD-003 §10, ADR-057 share-state rules)
#   @automations                 — Pod-Native Automations (Pillar 6.4)
#   @skill-retirement            — Retirement discipline (C10, design-02 §10)
#   @security                    — STRIDE-style invariants across the stratum
#   @kpis                        — BC15 event emission / observability
#   @accessibility               — WCAG 2.2 AA coverage
#   @ramp-glass-parity           — evidence-annex C4/C6/C7 parity
#   @ramp-glass-baseline         — evidence-annex C5 baseline-raising
#   @anthropic-v2                — evidence-annex C9/C10
#   @a16z-unprompted             — evidence-annex C8 (pillar-7 unprompted)
#   @governance                  — funnel + policy engine enforcement
#   @pod-native                  — Pod-resident truth / ADR-052 WAC
#   @stride                      — STRIDE threat-class coverage
#   @cross-phase                 — cuts across all phases
#   @risk-R1..@risk-R10          — maps to PRD-003 §14 risks
#   @kpi-12.1..@kpi-12.6         — maps to PRD-003 §12 KPI definitions
#   @wcag22                      — WCAG 2.2 AA normative criterion
#
# Conventions used in steps:
#   WebID form:   https://<contributor>.pods.visionclaw.org/profile/card
#   Nostr form:   npub1<contributor>
#   Pod paths:    absolute POSIX-style under the contributor's Solid Pod root.
#                 Any `.acl` reference is the Solid WAC resource sibling per
#                 ADR-052.
#   Event names:  dotted-lowercase per PRD-003 §12 (e.g. "skill.version.published").
#   Policy rules: lowercase_snake_case per ADR-057 §D2 rule names.

# --- Feature 1 --------------------------------------------------------------
# Mapping: PRD-003 §6.1 Sovereign Workspace · §7.1 Studio shell · §15 Acceptance
# Criteria (Phase 1). Evidence: C4 (harness beats model), C7 (workspace not chat).
# KPIs driven: §12.1 Activation Rate, §12.2 TTFR.
# Risks touched: R10 (offline pod), R7 (CLI bypass).

@contributor-studio @phase-1 @ramp-glass-parity
Feature: Contributor Studio shell opens with assembled context
  As a contributor with a provisioned Solid Pod and a Nostr identity
  I want the Studio to load with my role, goals, and current focus already
  assembled without configuration friction
  So that my first session ends with a durable artefact, not a setup tour.
  Satisfies PRD-003 §7.1, §15 Phase 1. Evidence: C4, C7.

  Background:
    Given the enterprise deployment has feature flag "STUDIO_ENABLED" set to true
    And the Phase 0 prerequisites from qe-enterprise-audit-report.md §1 are closed
    And contributor "Rosa" holds role "Contributor" per ADR-040
    And Rosa's WebID is "https://rosa.pods.visionclaw.org/profile/card#me"
    And Rosa has authenticated via NIP-07 with a valid Nostr signature
    And the WebSocket bridge, the Solid Pod client, and the MCP session all share the same Nostr delegation

  @kpi-12.1 @kpi-12.2
  Scenario: First-time Studio visit auto-provisions the pod and renders the four lanes
    Given Rosa has never opened the route "/studio"
    And Rosa's pod lacks the containers "/private/contributor-profile/", "/private/automations/", "/private/skills/", "/private/skill-evals/", and "/inbox/"
    When Rosa navigates to "/studio"
    Then a provisioning wizard appears
    And the pod containers "/private/contributor-profile/", "/private/automations/", "/private/skills/", "/private/skill-evals/", and "/inbox/" are created
    And each new container has an "owner-only" ACL inherited per ADR-052
    And Type Index entries for "urn:solid:AgentSkill" and "urn:solid:ContributorWorkspace" are seeded in "/profile/publicTypeIndex.jsonld"
    And the four lanes "graph", "work", "partner", "sensei" render within 2 seconds
    And the event "contributor.studio.opened" is emitted with Rosa's WebID
    And the event "contributor.studio.pod.attached" is emitted within the same session

  Scenario: Returning contributor sees last-known layout restored from the pod
    Given Rosa's pod contains "/private/contributor-profile/studio-layout.json" with pane widths {graph: 40%, work: 30%, partner: 20%, sensei: 10%}
    And Rosa had previously pinned node "vc:core/DistributedSystems" as the graph selection
    When Rosa navigates to "/studio"
    Then the four lanes restore to pane widths {40%, 30%, 20%, 10%}
    And node "vc:core/DistributedSystems" is re-selected in the graph lane
    And the AI partner lane context includes "vc:core/DistributedSystems" and its one-hop ontology neighbours

  @risk-R10
  Scenario: Pod offline degrades gracefully to a read-only cached view
    Given Rosa's pod provider returns HTTP 503 for every request
    And Rosa has a cached copy of her last GuidanceSession from the previous login
    When Rosa navigates to "/studio"
    Then a "pod offline" status chip appears in the header
    And the graph lane renders the last-known graph state from the local cache
    And the inbox lane renders the last synced inbox digest marked "stale"
    And any attempt to save a WorkArtifact surfaces a visible "pending pod write" badge
    And the event "contributor.studio.pod.attached" is NOT emitted

  @kpi-12.2
  Scenario: Graph deep-link populates all four lanes within 300 ms
    Given Rosa is already in "/studio" with an empty selection
    When Rosa clicks node "vc:policy/AIRiskControl" in the graph lane
    Then within 300 ms the work lane header updates to "vc:policy/AIRiskControl"
    And the AI partner lane system prompt includes "vc:policy/AIRiskControl" and its direct neighbours
    And the Sensei rail requests suggestions for focus "vc:policy/AIRiskControl"
    And the event "contributor.studio.graph.deeplinked" is emitted once

  @risk-R7
  Scenario: CLI-only contributor still counts toward activation
    Given Rosa has never opened the "/studio" route
    And Rosa has published a skill via the CLI tool "vf skill publish" which emits "skill.version.published"
    And Rosa has completed at least one GuidanceSession through the MCP WebSocket bridge
    When the Activation Rate calculation runs for Rosa's invitation window
    Then Rosa is counted as activated under §12.1
    And no "contributor.studio.opened" event is required for activation

  Scenario: Sidebar badge shows live inbox unread count
    Given Rosa has 3 unreviewed items under "/inbox/"
    When Rosa opens any route that renders the sidebar
    Then the sidebar entry "Studio" shows a badge with value "3"
    And the badge updates within 2 seconds when a fourth inbox item arrives over "studio:inbox:{pubkey}"

# --- Feature 2 --------------------------------------------------------------
# Mapping: PRD-003 §6.3 Ontology Sensei · §7.2 Ontology guide rail. Evidence:
# C2, C8. KPIs: §12.5 Guidance hit rate. Risks: R6 (nagware).

@ontology-sensei @phase-1 @a16z-unprompted
Feature: Ontology Sensei proactively offers relevant, typed suggestions
  As a contributor drafting a note inside Studio
  I want the Sensei rail to surface at most three typed suggestions without
  interrupting my flow
  So that I reuse existing ontology terms, skills, and peer precedents instead
  of silently re-inventing them.
  Satisfies PRD-003 §6.3, §7.2, §12.5. Evidence: C2, C8.

  Background:
    Given contributor "Rosa" is in "/studio/my-policy-workspace"
    And Rosa's pod contains at least 50 pages under "/private/kg/"
    And the "OntologyGuidanceActor" is running and healthy
    And the Sensei rate limit is configured to "<= 1 suggestion per 20s per pane"

  @kpi-12.5
  Scenario: A canonical-term suggestion is offered with provenance after an idle boundary
    Given Rosa has just typed the paragraph "our framework depends on distributed consensus"
    And 5 seconds of edit-idle have elapsed
    When the Sensei computes suggestions for the current focus
    Then the Sensei rail shows at most 3 ranked suggestions
    And at least one suggestion has kind "canonical_term" pointing at "vc:core/DistributedConsensus"
    And each suggestion carries a provenance pointer (OwlClass IRI, skill manifest URI, or peer pod WebID)
    And the event "sensei.suggestion.offered" is emitted with the envelope id

  @kpi-12.5
  Scenario: Accepted suggestion snaps the term into the draft and records the hit
    Given a Sensei NudgeEnvelope with 3 suggestions is visible
    When Rosa clicks "Snap to term" on the "vc:core/DistributedConsensus" suggestion
    Then the text "distributed consensus" in the work lane is linked to "vc:core/DistributedConsensus"
    And the event "sensei.suggestion.accepted" is emitted with the suggestion id
    And the §12.5 numerator for the 7-day rolling window is incremented

  Scenario: Dismissing a suggestion with a reason records a signal, not a miss silently
    Given a Sensei NudgeEnvelope contains a precedent-ref suggestion to peer "Chen"'s draft
    When Rosa dismisses the suggestion and selects reason "different-domain"
    Then the event "sensei.suggestion.dismissed" is emitted with reason "different-domain"
    And the denominator for §12.5 includes this suggestion
    And the suggestion does not reappear for the same focus within the current GuidanceSession

  @risk-R6
  Scenario: Rate limit prevents more than one suggestion per 20 seconds per pane
    Given Rosa has received a Sensei suggestion in the work lane 5 seconds ago
    When a new idle boundary triggers an OntologyGuidance compute inside the 20s window
    Then no new suggestion is pushed over "studio:sensei:{pubkey}"
    And the next permitted push is scheduled for no earlier than 20 seconds after the last

  @risk-R6
  Scenario: Global mute ratio exceeding 30% auto-halves the suggestion rate
    Given the org-wide Sensei mute ratio over the last 7 days is 0.34
    When the daily Sensei configuration job runs
    Then the per-pane rate limit doubles from "1/20s" to "1/40s"
    And an admin alert "sensei.nagware.autoshed" is raised
    And the mute ratio remains a reverse-indicator KPI on the BC15 dashboard

  Scenario: Sensei never auto-inserts text without explicit snap-to
    Given a Sensei suggestion of kind "canonical_term" is visible
    And Rosa has not clicked "Snap to term"
    When 60 seconds elapse
    Then the work lane draft content is byte-identical to before the suggestion arrived
    And no "WorkArtifactUpdated" event references the Sensei as author

# --- Feature 3 --------------------------------------------------------------
# Mapping: PRD-003 §6.2 Mesh Dojo · §7.7 Skill install · design-02 §5, §6, §7.
# Evidence: C5, C9. BC19 aggregates: SkillPackage, SkillVersion, SkillDistribution.

@skill-dojo @phase-2 @ramp-glass-baseline
Feature: Publishing a personal skill to the Dojo
  As a power contributor
  I want to publish a curated tool-sequence as a signed SkillPackage
  So that my teammates can discover and install it in minutes without exchanging
  URLs or using a central store.
  Satisfies PRD-003 §6.2, §7.8; design-02 §5. Evidence: C5, C9.

  Background:
    Given contributor "Alice" has WebID "https://alice.pods.visionclaw.org/profile/card#me"
    And Alice has authored a draft skill "market-analysis-brief" version "1.2.0" in "/private/skills/market-analysis-brief/"
    And the draft SKILL.md contains a valid YAML frontmatter with fields name, version, author, description, category, tools, min_model_tier, signature
    And an eval suite "skill.evals.jsonl" with at least 3 test cases exists in the same directory
    And Alice is a member of the team group "team-research"

  Scenario: Successful team publication writes SKILL.md and updates the Type Index
    When Alice invokes MCP tool "skill_publish" with {distribution: "team", team_slug: "team-research"}
    Then the files "SKILL.md", "skill.jsonld", "skill.evals.jsonl" are written atomically under "/shared/team-research/skills/market-analysis-brief/"
    And the skill manifest is signed with NIP-98 algorithm "nip98-ed25519" by "npub1alice..."
    And the Type Index entry of type "urn:solid:AgentSkill" is upserted for the new URI
    And the event "skill.version.published" is emitted with version "1.2.0" and distribution "team"
    And a baseline SkillBenchmark record is created in "/public/skills/market-analysis-brief/benchmarks/" within 60 seconds

  @security @risk-R5
  Scenario: Publication blocked when SKILL.md fails PII scan
    Given the SKILL.md contains the literal string "api_key=sk-prod-12345"
    When Alice invokes "skill_publish" with distribution "team"
    Then the Policy Engine rule "share_private_to_team" returns Deny with reason "pii_detected"
    And no files are written under "/shared/team-research/skills/"
    And a remediation message is delivered to Alice's "/inbox/" describing the offending line number
    And the event "skill.version.published" is NOT emitted

  @skill-evals
  Scenario: Publication blocked when the mandatory eval suite is missing
    Given the file "skill.evals.jsonl" has been deleted from "/private/skills/market-analysis-brief/"
    When Alice invokes "skill_publish" with distribution "team"
    Then the publish pre-flight fails with error "missing_eval_suite"
    And no Type Index entry is written
    And Alice is prompted to generate or author an eval suite before retrying

  @security
  Scenario: Signature verification on install rejects a tampered skill
    Given contributor "Bob" discovers Alice's skill at "https://alice.pods.visionclaw.org/shared/team-research/skills/market-analysis-brief/"
    And an on-path attacker has modified SKILL.md so its SHA-256 no longer matches "signature.content_hash"
    When Bob invokes MCP tool "skill_install" with that skill URI
    Then the NIP-98 signature verification fails
    And the event "skill.signature.mismatch" is emitted with the skill URI
    And nothing is written to Bob's "/private/skills/market-analysis-brief/"
    And a Policy Engine alert "SkillSignatureMismatch" is forwarded to the SOC

  @pod-native
  Scenario: Discovering peer skills respects WAC — no private pod leakage
    Given contributor "Chen" is NOT a member of team group "team-research"
    When the DojoDiscoveryActor crawls Alice's Type Index on Chen's behalf
    Then Chen receives HTTP 403 when resolving "/shared/team-research/skills/"
    And Alice's team-scope skill "market-analysis-brief" does not appear in Chen's SkillIndex
    And Chen's Studio Dojo pane lists only mesh-scope and Chen's own skills

  Scenario: Re-publishing the same version is rejected
    Given Alice has already published "market-analysis-brief" version "1.2.0" to team scope
    When Alice invokes "skill_publish" again with version "1.2.0"
    Then the publish fails with error "version_not_strictly_greater"
    And SKILL.md on the pod is unchanged
    And no duplicate Type Index entry is written

# --- Feature 4 --------------------------------------------------------------
# Mapping: design-02 §8 (eval discipline), §8.3 (mandatory eval gates). Evidence:
# C9. BC19 aggregates: SkillEvalSuite, SkillBenchmark.

@skill-evals @phase-2 @ramp-glass-baseline @anthropic-v2
Feature: Skill eval suites gate team and mesh promotion
  As a contributor
  I want promotion of a skill across share states to require passing evals and
  benchmarks against specific tier and recency thresholds
  So that compounding does not come at the cost of quality drift.
  Satisfies design-02 §8. Evidence: C9.

  Background:
    Given skill "market-analysis-brief" version "1.2.0" is in state "Personal"
    And the skill's min_model_tier is 3
    And the "SkillEvaluationActor" is healthy

  Scenario: Team promotion requires at least one passing baseline benchmark on tier 3
    Given no benchmark exists for "market-analysis-brief" version "1.2.0"
    When Alice requests transition Personal -> Team
    Then the transition is blocked with reason "no_passing_baseline_benchmark"
    When Alice invokes "skill_eval_run" with mode "baseline" and tier 3
    And the run records pass_rate >= 0.8 on at least 3 assertions
    Then a SkillBenchmark record is written with mode "baseline" and tier 3
    And the subsequent Personal -> Team transition succeeds
    And the event "skill.benchmark.completed" is emitted

  Scenario: Mesh candidacy requires at least 3 passing benchmarks across 30 days
    Given skill "market-analysis-brief" has 2 passing benchmarks in the trailing 30 days
    When Alice requests transition Team -> Mesh-candidate
    Then the transition is blocked with reason "insufficient_benchmark_history"
    And the UI shows "2 of 3 benchmarks met; next eligible after next passing run"

  Scenario: Broker Decision Canvas renders the skill preview and benchmark sparkline
    Given skill "market-analysis-brief" has a MeshCandidate BrokerCase "case-42" of category "skill_promotion"
    When a Broker with role "Broker" opens "case-42" in the Decision Canvas
    Then the canvas renders a Skill Preview widget with SKILL.md body
    And a sparkline shows the pass_rate trend across the last 5 benchmarks
    And a tool-chain diagram highlights any tool flagged as "high_trust" such as "ontology_propose"
    And an adoption heatmap shows distinct installer WebIDs in the last 14 days

  @security @risk-R4
  Scenario: Eval gaming detection flags prompt-memorised eval outputs
    Given Alice has submitted a skill whose eval assertions match the training prompts verbatim
    When the mesh-side SkillEvaluationActor runs against the held-out benchmark set
    Then the recorded benchmark carries a "gaming_suspected" flag
    And the broker case surfaces both author-asserted pass_rate and mesh-side pass_rate side-by-side
    And if the delta between the two exceeds 0.25 the case routes to the audit queue

  Scenario: Regression below threshold flags the skill without disabling it
    Given the previous benchmark for "market-analysis-brief" had pass_rate 0.95
    When a new benchmark records pass_rate 0.88 on the same model and tier
    Then the event "skill.compatibility.drift" is emitted with regression_vs_previous = -0.07
    And a "regression" badge renders on the Dojo SkillCard
    And installed copies remain usable and version-pinned

# --- Feature 5 --------------------------------------------------------------
# Mapping: PRD-003 §10 Share-to-Mesh Funnel · ADR-057 share-state rules. KPI:
# §12.4. Risks: R1, R5. Evidence: C5, governance.

@share-to-mesh @phase-3 @governance
Feature: Share-to-Mesh funnel enforces Private -> Team -> Mesh progression
  As the Contributor Enablement platform
  I want every artefact crossing a share-state boundary to pass through the
  funnel with Policy Engine + optional Broker review
  So that mesh publication is always explicit, audited, and governable.
  Satisfies PRD-003 §10, §12.4. Risks: R1, R5.

  Background:
    Given a WorkArtifact "draft-skill-quality-memo" exists in "/private/skills/quality-memo/"
    And its current ShareState is "Private"
    And contributor "Alice" is the author
    And the Policy Engine rule set "contributor-stratum-v1" is loaded

  @risk-R5
  Scenario: Contributor cannot skip Private -> Mesh directly
    When Alice invokes "share_intent_create" with target_scope "mesh"
    Then the ShareOrchestrator rejects with reason "must_progress_through_team"
    And no BrokerCase is opened
    And no WAC mutation occurs under "/public/skills/"

  Scenario: Team share requires WAC group membership
    Given Alice is NOT a member of team group "team-quality"
    When Alice invokes "share_intent_create" with target_scope "team:team-quality"
    Then the Policy Engine rule "share_private_to_team_scope" returns Deny with reason "not_in_team_group"
    And ShareState remains "Private"

  Scenario: Mesh share opens a BrokerCase via ShareOrchestrator
    Given "draft-skill-quality-memo" is currently in ShareState "Team" with a valid benchmark
    And Alice's pubkey is not a Broker
    When Alice invokes "share_intent_create" with target_scope "mesh"
    Then the Policy Engine rule "share_team_to_mesh" evaluates to Escalate
    And a BrokerCase is opened with category "share_to_mesh"
    And the event "share.intent.created" is emitted with direction "team_to_mesh"
    And no WAC change occurs under "/public/skills/" until Broker approval

  @security @risk-R5
  Scenario: Policy Engine blocks mesh share when PII is detected
    Given the artefact body contains the string "SSN: 000-00-0000"
    And "draft-skill-quality-memo" is currently in ShareState "Team"
    When Alice invokes "share_intent_create" with target_scope "mesh"
    Then the Policy Engine rule "share_team_to_mesh" returns Deny with reason "pii_detected"
    And no BrokerCase is opened
    And the event "policy.violation" is emitted to both the SOC and Alice's "/inbox/"

  Scenario: Broker revocation cascades WAC tightening and notifies installers
    Given skill "quality-memo" was previously promoted to Mesh and installed by 5 peers
    When a Broker opens a "share_rollback" case and approves the rollback
    Then the ShareOrchestrator MOVEs the skill from "/public/skills/quality-memo/" back to "/shared/team-quality/skills/quality-memo/"
    And the foaf:Agent Read ACL on the original path is removed
    And the Type Index entry for the mesh URI is deleted
    And each of the 5 installers receives an "/inbox/" item tagged "skill_demoted"
    And the SkillCard for installed copies renders a "demoted" badge

  @risk-R1
  Scenario: Contributor exceeds the mesh share rate limit of 3 per 24h
    Given Alice has created 3 mesh ShareIntents in the last 24 hours
    When Alice invokes "share_intent_create" with target_scope "mesh" a fourth time
    Then the request is denied with reason "rate_limit_exceeded"
    And no BrokerCase is opened
    And the next eligible window is surfaced in the UI

  Scenario: Audit log records every ShareIntent with signed provenance
    When Alice successfully creates a ShareIntent "intent-7"
    Then a Nostr-signed provenance event is written referencing "intent-7", Alice's WebID, from_state, to_state, and the policy_eval_id
    And the provenance event is append-only and readable by any "Auditor" role

# --- Feature 6 --------------------------------------------------------------
# Mapping: PRD-003 §6.4 Pod-Native Automations · §7.11 Inbox review · §7.12
# Automation scheduler · ADR-057 AutomationOrchestratorActor. Risks: R8, R10.

@automations @phase-4 @pod-native
Feature: Pod-native automations deliver briefs to the Inbox
  As a contributor
  I want scheduled routines to run inside my pod's scope and land outputs in
  my Inbox for explicit review
  So that no automation can silently publish on my behalf or bypass policy.
  Satisfies PRD-003 §6.4. Risks: R8, R10.

  Background:
    Given Rosa has authenticated and holds a valid NIP-26 delegation with 24-hour expiry
    And the "AutomationOrchestratorActor" is running
    And the Policy Engine rule "automation_budget_total" has a default cap of 100k tokens per pubkey per 24h

  Scenario: Scheduled routine writes output to "/inbox/" with a provenance manifest
    Given Rosa has scheduled automation "weekly-brief" at "/private/automations/weekly-brief.json"
    And the schedule cron is "0 20 * * 0" (Sunday 20:00)
    When the scheduler fires at Sunday 20:00
    Then a synthesised brief is written to "/inbox/weekly-brief-<iso_date>.ttl"
    And a sibling manifest records {source_routine_id, delegation_pubkey, token_spend, tool_calls[]}
    And the event "automation.run.completed" is emitted with budget usage
    And no write occurs outside "/inbox/" or "/private/agent-memory/"

  @risk-R10
  Scenario: Offline contributor — routines run, but no mesh-write intents are emitted
    Given Rosa's device is offline but the pod provider is up
    And Rosa has scheduled automation "nightly-eval" that would emit a ShareIntent on completion
    When the scheduler fires "nightly-eval"
    Then the automation completes and writes to "/inbox/"
    And any ShareIntent the routine attempts to emit is queued under "pending_share_intents" and NOT dispatched
    And on Rosa's next successful login the queued ShareIntents surface for explicit confirmation

  @security
  Scenario: Expired NIP-26 delegation deactivates the routine and notifies the owner
    Given Rosa's NIP-26 delegation expired 10 minutes before the scheduled fire time
    When the scheduler attempts to run "weekly-brief"
    Then the automation fails with reason "delegation_expired"
    And the event "automation.run.failed" is emitted
    And a notification lands in Rosa's "/inbox/" asking her to re-authorise
    And no pod write occurs under "/inbox/"

  Scenario: Contributor accepts an Inbox item and routes it to a WorkArtifact
    Given Rosa's "/inbox/" contains item "inbox-42" produced by automation "weekly-brief"
    When Rosa opens "/studio/inbox" and clicks "Accept" on "inbox-42"
    Then a new WorkArtifact is created with lineage pointing at "inbox-42"
    And "inbox-42" is marked "reviewed" with timestamp and Rosa's WebID
    And the event "automation.output.promoted" is emitted

  Scenario: Contributor escalates an Inbox item to the Broker
    Given Rosa's "/inbox/" contains item "inbox-43" flagged as high-risk
    When Rosa selects "Escalate to broker" on "inbox-43"
    Then a BrokerCase of category "contributor_escalation" is opened with "inbox-43" as payload
    And "inbox-43" moves to status "escalated"
    And the event "inbox.item.reviewed" is emitted with disposition "escalate_to_broker"

  @risk-R8
  Scenario: Budget-cap enforcement is per-contributor-rolling-window, not per-automation
    Given Rosa has 5 scheduled automations, each individually under the per-automation cap
    And the aggregate token spend across the 24h rolling window is 98k of the 100k cap
    When automation "nightly-eval" attempts a run that would spend 5k tokens
    Then the run is throttled with reason "contributor_aggregate_cap"
    And a notice is delivered to Rosa's "/inbox/"
    And no partial run output is written to "/inbox/"

# --- Feature 7 --------------------------------------------------------------
# Mapping: PRD-003 §11 Skill Lifecycle Discipline · design-02 §10. Evidence:
# C10. KPI: §12.6 Redundant skill retirement rate. Risks: R9.

@skill-retirement @phase-4 @anthropic-v2
Feature: Retirement advisor prunes skills the base model has absorbed
  As the Skill Lifecycle context
  I want to flag skills whose baseline (skill-disabled) pass rate has caught
  up with or exceeded the skill-enabled pass rate
  So that the mesh library self-prunes rather than accretes.
  Satisfies design-02 §10. Evidence: C10. KPI: §12.6.

  Background:
    Given "SkillCompatibilityScanner" runs daily on all Team and Mesh skills
    And the baseline comparison runs on the current production model tier

  @kpi-12.6
  Scenario: Compatibility scanner flags a skill once baseline matches the skill
    Given skill "summarise-markdown" v1.0.0 has last benchmark pass_rate 0.78 on tier 2
    And the baseline (skill-disabled) run on tier 2 records pass_rate 0.79
    When the scanner evaluates "summarise-markdown"
    Then the event "skill.model.caughtup" is emitted with baseline_pass_rate 0.79 and skill_pass_rate 0.78
    And a SkillRetirementProposal is delivered to the maintainer's "/inbox/" with evidence

  @risk-R9
  Scenario: Retirement grace period permits maintainer objection
    Given a SkillRetirementProposal was delivered 10 days ago for skill "summarise-markdown"
    When the maintainer posts a rebuttal that keeps the skill active
    Then the skill remains in state "Promoted"
    And the scanner reschedules its next check for 60 days later
    And no pod MOVE occurs

  Scenario: Installed copies keep working after retirement but display a retirement badge
    Given skill "summarise-markdown" v1.0.0 has been retired
    And 3 peers have pinned installs at v1.0.0 in "/private/skills/"
    When each peer opens their Dojo pane
    Then every SkillCard for "summarise-markdown" renders a "retired by author" badge
    And the pinned install continues to execute via "skill_install" invocation
    And the Dojo recommendation ranking demotes "summarise-markdown" below non-retired candidates

  Scenario: Retirement requires either a successor skill or a BaseModelAbsorbed verdict
    Given skill "legacy-tickers" has no successor skill and no BaseModelAbsorbed verdict
    When the maintainer invokes "skill_retire"
    Then the call fails with reason "retirement_requires_successor_or_verdict"
    And the skill remains in its current state

# --- Feature 8 --------------------------------------------------------------
# Mapping: PRD-003 §14 Risks (R1-R10) · ADR-057 §Security. STRIDE classes:
# Spoofing, Tampering, Repudiation, Information disclosure, Denial of service,
# Elevation of privilege.

@security @cross-phase @stride
Feature: Security invariants across the Contributor AI Support Stratum
  As the platform
  I want cross-cutting security invariants to hold regardless of phase, pane,
  or share state
  So that the stratum cannot silently leak private artefacts or bypass
  governance.
  Satisfies PRD-003 §14, ADR-057 §Security.

  @risk-R5
  Scenario: WAC misconfiguration is prevented by the double-gate
    Given Alice edits SKILL.md frontmatter and sets "distribution: public"
    When Alice attempts to publish to "/shared/team-research/skills/"
    Then the pre-flight check "double_gate_mismatch" rejects the publish
    And the error cites the required path "/public/skills/" and the required "public:: true" marker
    And no pod write occurs

  Scenario: Automation delegation cap cannot write outside granted paths
    Given Rosa's automation "intrusion-sim" holds a NIP-26 delegation scoped to "/inbox/"
    When the automation attempts to write to "/public/kg/"
    Then the pod write fails with HTTP 403
    And the event "policy.violation" is emitted with rule "automation_public_write"
    And no partial write remains on the pod

  @stride @risk-R6
  Scenario: Sensei cannot leak private artefact content through suggestions
    Given Rosa's private artefact "/private/kg/confidential-memo.md" contains the string "project-codename-blue"
    And Chen is a peer with no Read access to Rosa's "/private/"
    When Chen's OntologyGuidanceActor computes suggestions for Chen's focus
    Then none of Chen's suggestion payloads or provenance pointers contain "project-codename-blue"
    And no suggestion references the URI "/private/kg/confidential-memo.md"

  Scenario: Skill signature verification uses NIP-98 with an active revocation check
    Given skill "bad-actor-skill" was signed by "npub1evil" who has since broadcast a NIP-09 delete
    When any peer's DojoDiscoveryActor encounters the signed manifest
    Then the signature is treated as revoked
    And the skill is excluded from the SkillIndex
    And the event "skill.signature.revoked" is logged to the audit stream

  Scenario: Inbox items respect owner-only read ACL even if the routine is compromised
    Given Rosa's automation "compromised-routine" has been breached by a malicious payload
    And the payload attempts to open a Read grant on "/inbox/" for "foaf:Agent"
    When the ACL write is submitted
    Then the pod rejects the write because the "/inbox/" ACL forbids widening Read beyond owner
    And the event "policy.violation" is emitted with rule "inbox_acl_widening"
    And the Inbox remains owner-only

  @risk-R2
  Scenario: SkillVersion immutability after benchmark
    Given skill "market-analysis-brief" v1.2.0 has state "Benchmarked"
    When Alice attempts to overwrite SKILL.md for v1.2.0
    Then the write is rejected with reason "version_immutable_after_benchmark"
    And a new version must be published to land the change

# --- Feature 9 --------------------------------------------------------------
# Mapping: PRD-003 §12 KPIs and Observability (all six KPIs). BC15 lineage.

@kpis @observability
Feature: KPI event emission aligns with BC15 lineage
  As the observability layer
  I want the six named KPIs to be computable from the emitted events alone
  So that the BC15 dashboard can slice contributor outcomes without backfill.
  Satisfies PRD-003 §12.

  @kpi-12.1
  Scenario: Contributor activation rate event set emitted on first Studio session
    Given contributor "Rosa" was invited 3 days ago and has not yet activated
    When Rosa opens Studio, attaches a pod, and completes one GuidanceSession with at least one suggestion action
    Then events "contributor.studio.opened", "contributor.studio.pod.attached", "sensei.suggestion.offered", and "sensei.suggestion.accepted" are all emitted within a single 7-day window
    And Rosa's activation flag flips from 0 to 1 for the current 7-day rolling window
    And the aggregate activation rate is recomputed for the invited cohort

  @kpi-12.2
  Scenario: TTFR is the wall-clock delta from first-open to first durable artefact
    Given Rosa first opens Studio at "2026-04-20T09:00:00Z"
    And Rosa installs skill "market-analysis-brief" at "2026-04-20T09:17:44Z"
    When the TTFR aggregator runs
    Then the recorded TTFR for Rosa is 17 minutes 44 seconds
    And the value feeds the §12.2 median aggregation across contributors whose first-open occurred in the reporting window

  @kpi-12.3
  Scenario: Skill reuse rate aggregates installs across distinct peers over 30 days
    Given skill "market-analysis-brief" was published by Alice
    And 5 distinct non-author WebIDs have installed a version in the last 30 days
    And 2 distinct skills were published in the window
    When the reuse-rate aggregator runs
    Then the reuse rate numerator is 5 and denominator is 2
    And the §12.3 value 2.5 is reported

  @kpi-12.4
  Scenario: Share-to-mesh conversion rate emitted on BrokerDecision
    Given 10 mesh ShareIntents were created in the last 30 days
    And 2 were approved by the Broker
    And 6 were rejected and 2 expired
    When the conversion-rate aggregator runs
    Then the reported §12.4 value is 0.20 (2 / 10)

  @kpi-12.5
  Scenario: Ontology guidance hit rate honours mute exclusions
    Given in the trailing 7 days Rosa received 100 offered suggestions
    And 20 were muted and 30 were accepted and 10 were edited toward
    When the hit-rate aggregator runs
    Then the numerator is 40 (accepted + edited)
    And the denominator is 80 (offered - muted)
    And the §12.5 value 0.50 is reported

  @kpi-12.6
  Scenario: Redundant skill retirement rate emitted on SkillRetired events
    Given at the start of the quarter there are 200 active Team-or-Mesh skills
    And 18 skills were marked "Retired" during the quarter
    When the quarterly retirement aggregator runs
    Then the §12.6 value 0.09 is reported
    And the dashboard slice "Contributor" shows this as a positive signal per R9 mitigation

# --- Feature 10 -------------------------------------------------------------
# Mapping: PRD-003 §15 (accessibility criterion) · ADR-046 UI accessibility
# conventions. WCAG 2.2 AA.

@accessibility @wcag22
Feature: Studio meets WCAG 2.2 AA targets
  As a contributor using assistive technology
  I want full keyboard navigation, logical focus order, and screen-reader
  announcements across the four lanes
  So that the stratum is usable without a pointing device.
  Satisfies PRD-003 §15.

  Background:
    Given Rosa is in "/studio/my-workspace" using a keyboard and a screen reader

  Scenario: Full keyboard navigation across all four lanes
    When Rosa presses "Tab" repeatedly from the header focus position
    Then focus cycles in order: graph lane, work lane, partner lane, sensei rail, sidebar, back to header
    And no lane traps focus
    And every interactive element has a visible focus ring with contrast ratio >= 3:1

  Scenario: Screen reader announces pane context transitions
    When Rosa moves focus from the graph lane to the partner lane
    Then the screen reader announces the pane name and its current context summary
    And the announcement is delivered via an aria-live "polite" region

  Scenario: Focus management is preserved on route change
    When Rosa activates the command "Go to Studio > Skills" from the palette
    Then the route changes to "/studio/skills"
    And focus lands on the first result in the skill catalogue
    And the screen reader announces "Skills catalogue, 12 results"

  Scenario: Sensei suggestions are not modal and do not interrupt reading
    Given Rosa's screen reader is reading the work lane
    When a Sensei suggestion appears in the guide rail
    Then the announcement level is "polite" not "assertive"
    And the work lane reading is not interrupted

# --- Feature 11 -------------------------------------------------------------
# Mapping: ADR-057 §Integration Points · PRD-003 §Phase-0 + §13 cross-context.
# Ensures that the stratum's events reach the management mesh correctly.

@cross-phase @governance
Feature: Stratum-to-mesh integration contracts hold
  As the mesh
  I want the stratum's ShareIntents, WorkArtifacts, and Skill events to arrive
  as valid BrokerCase / MigrationCandidate / WorkflowProposal / KPI payloads
  So that the upstream governance contexts can consume without translation
  brittleness.
  Satisfies ADR-057 §Integration Points.

  Scenario: Mesh share of a note raises a MigrationCandidate on ontology implications
    Given Rosa has a WorkArtifact of kind "note" with an accepted canonical_term suggestion "vc:policy/AIRiskControl"
    And the artefact has transitioned Private -> Team
    When Rosa raises a mesh ShareIntent
    And the Broker approves the case
    Then the ShareOrchestrator emits exactly one of {BrokerCase, MigrationPayload, WorkflowProposal}
    And in this case it emits a MigrationPayload referencing "vc:policy/AIRiskControl"
    And the MigrationPayload reaches BC13 Insight Discovery within 60 seconds

  Scenario: Mesh share of a draft workflow raises a WorkflowProposal
    Given the Mesh-bound artefact is of kind "draft-workflow"
    When Broker approval completes
    Then exactly one WorkflowProposal is submitted to BC12
    And the author_id equals the contributor's WebID
    And the ShareIntent's downstream_case_id is populated

  Scenario: Skill promotion emits SkillPromoted and registers a WorkflowPattern
    Given skill "policy-memo-draft" has been approved by the Broker for mesh promotion
    When the ShareOrchestrator completes the MOVE to "/public/skills/"
    Then the event "skill.promoted" is emitted with a signed promotion certificate from the Broker
    And BC12 creates a WorkflowPattern tied to the SkillVersion
    And the Dojo SkillCard renders a "mesh-certified" badge

  Scenario: Invariant — a mesh ShareIntent never produces zero or two downstream cases
    Given any approved mesh ShareIntent "intent-x"
    When its lifecycle completes
    Then exactly one of {BrokerCase, MigrationPayload, WorkflowProposal} references "intent-x"
    And if two references exist the invariant check emits "share.intent.invariant.violation"

# --- Feature 12 -------------------------------------------------------------
# Mapping: design-03 memory-by-default · evidence-annex C6. Guarantees that the
# contributor can see exactly what the agent knows (audit by inspection).

@contributor-studio @phase-1 @ramp-glass-parity
Feature: Memory by default with inspectable write-once-read-many files
  As a contributor
  I want the Studio agent to know what is in my pod memory files and only that
  So that I can audit the agent's context by opening files, not logs.
  Evidence: C6.

  Background:
    Given Rosa's pod contains "/private/agent-memory/episodic/studio-partner/" with synthesised session files

  Scenario: Agent never modifies memory during a conversation
    Given Rosa has an active GuidanceSession with the AI partner lane
    When Rosa exchanges 20 turns of conversation over 15 minutes
    Then no write occurs to "/private/agent-memory/episodic/studio-partner/"
    And the synthesis pipeline runs no earlier than 24 hours after the last synthesis

  Scenario: Inspection endpoint exposes exactly what the agent reads at session start
    Given the AI partner lane initialises a new session
    When Rosa opens the "Memory Inspector" affordance in the partner lane
    Then the inspector renders the full text of every file the partner's system prompt consumed
    And the hash of each file matches the pod-resident content
    And no hidden or backend-only memory source is included

# ---------------------------------------------------------------------------
# Coverage Matrix
# ---------------------------------------------------------------------------
# The matrix below maps PRD-003 / ADR-057 / design-02 / evidence-annex inputs
# to the Features that cover them. Where a cell reads "partial", that Feature
# touches the requirement but does not exhaustively test it; additional
# scenarios are required before Phase gate sign-off.
#
# PRD-003 coverage:
# | PRD Section                                         | Feature(s) covering                                           |
# |-----------------------------------------------------|---------------------------------------------------------------|
# | §6.1 Sovereign Workspace                            | Feature 1 Studio shell; Feature 12 Memory by default          |
# | §6.2 Mesh Dojo                                      | Feature 3 Publishing; Feature 4 Evals; Feature 7 Retirement   |
# | §6.3 Ontology Sensei                                | Feature 2 Sensei; Feature 8 Security (leak test)              |
# | §6.4 Pod-Native Automations                         | Feature 6 Automations                                         |
# | §7.1 Studio shell                                   | Feature 1                                                     |
# | §7.2 Ontology guide rail                            | Feature 2                                                     |
# | §7.3 AI partner lane                                | Feature 1 (graph deep-link); Feature 12                       |
# | §7.4 Graph deep-link                                | Feature 1 (<= 300ms scenario)                                 |
# | §7.5 Command palette extensions                     | Feature 10 (route change via palette)                         |
# | §7.6 Skill discovery                                | Feature 3 (peer discovery WAC); Feature 7 (ranking)           |
# | §7.7 Skill install                                  | Feature 3 (signature verification on install)                 |
# | §7.8 Skill share                                    | Feature 3; Feature 4; Feature 5                               |
# | §7.9 Skill eval                                     | Feature 4                                                     |
# | §7.10 Share-to-mesh funnel                          | Feature 5; Feature 11                                         |
# | §7.11 Inbox review                                  | Feature 6 (accept/escalate); Feature 8 (ACL)                  |
# | §7.12 Automation scheduler                          | Feature 6                                                     |
# | §10 Share-to-Mesh Funnel                            | Feature 5; Feature 11                                         |
# | §11 Skill Lifecycle Discipline                      | Feature 4; Feature 7                                          |
# | §12.1 Contributor Activation Rate                   | Feature 9 (@kpi-12.1); Feature 1 (@kpi-12.1)                  |
# | §12.2 Time-to-First-Result                          | Feature 9 (@kpi-12.2); Feature 1                              |
# | §12.3 Skill Reuse Rate                              | Feature 9 (@kpi-12.3)                                         |
# | §12.4 Share-to-Mesh Conversion Rate                 | Feature 9 (@kpi-12.4); Feature 5                              |
# | §12.5 Ontology Guidance Hit Rate                    | Feature 9 (@kpi-12.5); Feature 2                              |
# | §12.6 Redundant Skill Retirement Rate               | Feature 9 (@kpi-12.6); Feature 7                              |
# | §13 Phase 0 Dependencies                            | Feature 1 Background (STUDIO_ENABLED + QE audit)              |
# | §13 Phase 1 scope                                   | Features 1, 2, 12                                             |
# | §13 Phase 2 scope                                   | Features 3, 4                                                 |
# | §13 Phase 3 scope                                   | Features 5, 11                                                |
# | §13 Phase 4 scope                                   | Features 6, 7                                                 |
# | §14 R1 Broker intake overload                       | Feature 5 (rate limit)                                        |
# | §14 R2 Skill drift                                  | Feature 8 (immutability); Feature 4 (regression)              |
# | §14 R3 Pod-to-Neo4j incoherence                     | Feature 1 (offline degradation) partial                       |
# | §14 R4 Eval gaming                                  | Feature 4 (gaming detection)                                  |
# | §14 R5 WAC misconfiguration                         | Feature 3, Feature 5, Feature 8 (double-gate)                 |
# | §14 R6 Sensei nagware                               | Feature 2 (rate limit, auto-shed)                             |
# | §14 R7 CLI bypass                                   | Feature 1 (CLI activation counts)                             |
# | §14 R8 Budget-cap gaming                            | Feature 6 (aggregate cap)                                     |
# | §14 R9 Retirement abandonment                       | Feature 7 (grace + KPI)                                       |
# | §14 R10 Offline pod failure                         | Feature 1, Feature 6 (offline queue)                          |
# | §15 Acceptance criteria (Phase 1 checklist)         | Features 1, 2, 9, 10, 12                                      |
#
# ADR-057 coverage:
# | ADR-057 Section                                      | Feature(s) covering                                           |
# |------------------------------------------------------|---------------------------------------------------------------|
# | Share-state transition rules                         | Feature 5                                                     |
# | Skill lifecycle state machine                        | Features 3, 4, 7                                              |
# | Actor topology (ShareOrchestrator, DojoDiscovery ...)| Features 3, 5, 6                                              |
# | MCP tool additions (skill_publish / install / ...)   | Features 3, 6                                                 |
# | Pod layout extensions                                | Features 1, 3, 6                                              |
# | REST/WebSocket surface                               | Feature 1 (sidebar badge); Feature 11 (events)                |
# | Integration points (BC11/BC12/BC13/BC15/BC17)        | Features 5, 9, 11                                             |
# | Security invariants                                  | Feature 8                                                     |
#
# Design spec 02 coverage:
# | design-02 Section                                    | Feature(s) covering                                           |
# |------------------------------------------------------|---------------------------------------------------------------|
# | §3 SKILL.md canonical format                         | Feature 3 (frontmatter validation)                            |
# | §4 Pod layout for skills                             | Feature 3                                                     |
# | §5 Publishing flow                                   | Feature 3                                                     |
# | §6 Discovery flow                                    | Feature 3 (WAC-respecting crawl)                              |
# | §7 Installation flow                                 | Feature 3 (install + signature)                               |
# | §8 Eval discipline                                   | Feature 4                                                     |
# | §9 Distribution scopes                               | Features 3, 5                                                 |
# | §10 Retirement                                       | Feature 7                                                     |
# | §11 Broker integration                               | Features 4, 5, 11                                             |
# | §14 Security and policy hooks                        | Features 3, 5, 8                                              |
#
# Evidence annex claim coverage:
# | Claim | Feature(s) covering                                                        |
# |-------|----------------------------------------------------------------------------|
# | C1    | Feature 1 (Studio shell as the "use" half)                                 |
# | C2    | Feature 2, Feature 12                                                      |
# | C3    | Feature 5, Feature 9 (institutional compounding via funnel + KPIs)         |
# | C4    | Feature 1 (harness parity)                                                 |
# | C5    | Features 3, 4, 5 (breakthrough-to-baseline path)                           |
# | C6    | Feature 12                                                                 |
# | C7    | Feature 1, Feature 10                                                      |
# | C8    | Feature 2                                                                  |
# | C9    | Features 3, 4 (eval discipline), Feature 7                                 |
# | C10   | Feature 7                                                                  |
#
# Gaps / untestable requirements surfaced during authoring:
# - PRD-003 §16 Open Questions 1-6 (team-scope Type Index privacy, retirement
#   archive container, offline automation auth, pod-down read caching TTL,
#   Broker ontology expertise routing, cross-team skill visibility) are policy
#   decisions and cannot be asserted as acceptance criteria until ADR-057
#   §D4..§D8 ratify the rules. Scenarios should be added to Features 3/5/6
#   once the decisions land.
# - ADR-057 Open Question 2 (automation delivery guarantee: at-most-once vs
#   at-least-once vs exactly-once) is not asserted anywhere in this file;
#   idempotency-key scenarios must be added to Feature 6 once guarantee is set.
# - design-02 §13 model-tier routing correctness (Tier 2 Haiku for SKILL.md
#   drafting, Tier 3 Sonnet for eval generation) is not externally observable
#   without cost-attribution telemetry; add once ADR-026 emits tier-usage events.
# - Per-contributor storage quotas (ADR-057 Open Question 6) have no policy
#   value defined; quota-breach scenarios should be added once configured.
# - Cross-device workspace identity (DDD Open Question 1) is deferred; no
#   concurrent-session scenarios are asserted here.
# - Dual-tier identity bridge edges (DDD Open Question 2) are not tested;
#   add BRIDGE_TO-emission scenarios to Feature 11 once timing is decided.

# ═════════════════════════════════════════════════════════════════════════════
# Feature 13 — Risk register R11–R14 (promoted from DR5–DR8 during reconciliation)
# ═════════════════════════════════════════════════════════════════════════════

@risk-R11 @sensei @budget
Feature: Sensei sustained background cost has an enforced per-contributor budget cap
  As the Admin operating VisionClaw at scale
  I want per-contributor Sensei budget caps so that nudge frequency cannot
    blow the Tier-2 Haiku inference spend
  So that cost grows with contributor count, not with attention span
  [Satisfies PRD-003 §14 R11; design 03 §12 observability]

  Background:
    Given the contributor "Rosa" is active in /studio
    And the Tier-2 daily budget is set to 40 sensei_nudge calls per contributor

  Scenario: Budget approaching triggers a warning, not a block
    Given Rosa has consumed 35 sensei_nudge calls today
    When the Ontology Guide Rail would emit a 36th nudge
    Then the nudge is emitted at Tier 2
    And the event "sensei.budget.warning" is recorded with remaining = 4

  Scenario: Budget exhaustion degrades Sensei to Tier-1 ontology-distance heuristics
    Given Rosa has consumed exactly 40 sensei_nudge calls today
    When the Ontology Guide Rail is asked to emit a nudge
    Then the call MUST route to Tier 1 Agent Booster, not Tier 2 Haiku
    And the event "sensei.budget.exhausted" is recorded
    And the nudge-card UI shows a subtle "Tier-1 fallback" badge

  Scenario: Budget resets at contributor's configured daily boundary
    Given Rosa's budget is exhausted
    And her `preferences.jsonld` sets her daily reset boundary to 00:00 Europe/London
    When the clock passes 00:00 Europe/London
    Then Rosa's consumed counter returns to 0
    And Tier-2 routing resumes

@risk-R12 @skill-compatibility-scanner @back-pressure
Feature: Compatibility Scanner fan-out is bounded and rate-limited
  As the Admin
  I want the SkillCompatibilityScanner to never starve the mesh benchmark
    runner when the model-routing config (ADR-026) changes
  So that hundreds of skills don't re-benchmark simultaneously
  [Satisfies PRD-003 §14 R12; design 02 §8.4]

  Background:
    Given 250 Personal-scope skills across 40 contributor pods are installed
    And the per-contributor hourly re-benchmark quota is 10 skills

  Scenario: Tier-mapping change queues benchmarks at bounded concurrency
    Given the bounded-concurrency parallel-run limit is 8
    When an Admin changes the ADR-026 tier mapping for a base model
    Then the Scanner enumerates all 250 skills into a priority queue
    And at most 8 benchmarks execute in parallel at any instant
    And per-contributor enqueue rate MUST NOT exceed 10 skills/hour
    And the queue depth is surfaced in the Admin dashboard within 5 s

  Scenario: Tier-mapping change is feature-flagged with a planned rollout schedule
    Given an Admin is about to deploy a tier-mapping change
    When they open the "Schedule rollout" pane
    Then they see three fields: start_at, batch_size, batch_interval_seconds
    And the minimum batch_size is 1 and minimum batch_interval_seconds is 60
    And no benchmark executes before the configured start_at

  Scenario: Queue-depth SLA exposed to Admin
    Given the Scanner queue depth is non-zero
    When the Admin visits /kpi/scanner-queue
    Then they see the queue depth trend chart for the last 24 h
    And they see the ETA to drain at current concurrency

@risk-R13 @inbox @retention
Feature: Inbox retention prevents unbounded growth
  As a contributor
  I want bounded inbox retention so runaway automations cannot fill my pod
  So that new automation output is never silently dropped
  [Satisfies PRD-003 §14 R13; design 03 §5, §6]

  Background:
    Given the default retention is 500 items OR 30 days, whichever first
    And Rosa has 3 automation routines writing to /inbox/

  Scenario: Over-count overflow moves oldest items to the DLQ container
    Given Rosa's /inbox/ namespace contains 499 items
    When a 501st arrival from routine "daily-research-brief" lands
    Then the oldest item is MOVED to /inbox/.dlq/ (not deleted)
    And the event "inbox.quota.approached" is emitted
    And Rosa's Workspace bar shows a visible warning chip within 5 s

  Scenario: Over-age overflow moves expired items to the archive container
    Given an inbox item's created_at is older than 30 days
    When the retention sweeper runs
    Then the item is MOVED to /inbox/.archive/
    And a one-click "Restore" action is available in the Studio inbox view

  Scenario: Contributor can raise per-namespace retention when pod storage allows
    Given Rosa has 50 GiB of pod quota available
    When she sets /inbox/routine-heavy/retention_max_items to 2000 in preferences.jsonld
    Then the sweeper honours the new limit for that namespace only
    And the default 500 remains for other namespaces

@risk-R14 @websocket @fan-out
Feature: WebSocket /api/ws/studio fan-out is capped per WebID
  As the platform operator
  I want per-WebID connection caps and topic-scoped broadcasts so that
    a publish spike cannot saturate the relay
  So that contributors with many tabs open cannot self-DDOS the mesh
  [Satisfies PRD-003 §14 R14; design 01 §15]

  Background:
    Given the per-WebID /api/ws/studio connection cap is 4
    And the chat and nudge channels have independent rate limits

  Scenario: Excess session returns 429 with retry-after
    Given Rosa has 4 concurrent /api/ws/studio sessions open
    When a 5th tab attempts to open a new /api/ws/studio session
    Then the server responds HTTP 429 Too Many Requests
    And the response includes a Retry-After header of at least 5 seconds
    And one of Rosa's existing sessions is NOT terminated to make room

  Scenario: Mesh publish fan-out scopes by workspace_id, not webid
    Given 1000 contributors have active /studio sessions across 2000 workspaces
    When a Mesh skill is promoted and a "skill.mesh.promoted" broadcast fires
    Then the broadcast is scoped to subscribers of the "skills:mesh" topic only
    And no subscriber's chat or inbox channels are touched
    And subscribers NOT on the "skills:mesh" topic receive zero frames

  Scenario: Chat and nudge channels have independent rate limits
    Given Rosa has hit the nudge channel's rate limit
    When her AI partner attempts to stream a chat message to her
    Then the chat message is delivered without delay
    And the nudge channel returns a rate-limit indicator to the Sensei service
