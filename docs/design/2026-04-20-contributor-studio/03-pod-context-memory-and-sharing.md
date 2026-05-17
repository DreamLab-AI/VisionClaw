---
title: Pod Context Memory and Share-to-Mesh Funnel — Design Spec
description: Contributor profile, episodic memory, automations, inbox, and the Private-Team-Mesh share funnel. Extends ADR-030 and ADR-052. Security-first, WAC-enforced, broker-bridged. Implements PRD-003 §9, §10 and ADR-057.
category: design
tags: [contributor-studio, pod, memory, sharing, wac, security, 2026-04-20]
updated-date: 2026-04-20
---

# Pod Context Memory and Share-to-Mesh Funnel — Design Spec

## 1. Purpose and invariants

This spec defines the contributor-owned memory surfaces that sit underneath
Contributor Studio — the pod-resident containers that carry a contributor's
profile, workspace snapshots, episodic memory, scheduled automations, and
incoming agent briefs — and the transition machinery that promotes
artefacts across the three share states **Private → Team → Mesh**. The
design is adversarial by construction: every container boundary, every
delegation capability, every cache-coherence hop is treated as a potential
breach surface. Three invariants govern every design choice below.

**Invariant 1 — Pod-first.** The contributor's Solid Pod is the
write-master for every content-carrying artefact listed here. The backend
(Neo4j, RuVector, Studio services) is an indexer and cache, never the
source of truth. Loss of the backend is recoverable; loss of a Pod is not.
Every endpoint that mutates a profile, workspace, automation, inbox item
or share-log entry MUST write to the Pod before acknowledging the caller.
A successful pod write with a failed cache update is a cache-consistency
problem; a successful cache update with a failed pod write is an integrity
violation and MUST return a 5xx to the caller.

**Invariant 2 — WAC-enforced.** No share-state transition is enforced by a
path convention alone. Every transition mutates a Web Access Control (WAC)
resource on the Pod, and every read path is validated by the Pod's ACL
evaluator. Per ADR-052 the backend double-gates published writes (artefact
flag AND container path); this spec extends that double-gate to Team shares
(artefact distribution-scope AND named-group-matching container path) and
to delegated-cap writes into `/inbox/` (cap scope AND path-prefix check).
Any single-gate failure mode is treated as a bug.

**Invariant 3 — Broker-bridged.** No artefact enters the mesh without a
broker decision recorded in BC11. Every Team→Mesh transition produces
exactly one of `BrokerCase` (generic), `BrokerCase{category=migration_candidate}`
per ADR-049 (ontology terms), `WorkflowProposal` per ADR-042 (durable
workflows), or `InsightCandidate` per BC13 (emergent patterns). The
`ShareOrchestratorActor` is the sole adapter; contributors never post to
BC11 directly, and no mesh-promotion path bypasses the broker queue.

## 2. Pod layout extensions (on top of ADR-052)

ADR-052 ratified the four-container root (`/private/`, `/public/`,
`/shared/`, `/profile/`) with the double-gated write discipline. This
spec adds contributor-scoped containers, team-scoped containers under
`/shared/`, and the inbox container — without changing any ADR-052
invariants.

### 2.1 Full tree after this spec

```
/                                      (ADR-052: owner-only)
├── .acl                                (unchanged — zero foaf:Agent grants)
├── private/                            (ADR-052: owner-only, acl:default)
│   ├── .acl                            (unchanged)
│   ├── kg/                             (unchanged)
│   ├── config/                         (unchanged)
│   ├── bridges/                        (unchanged — ADR-048)
│   ├── agent-memory/                   (unchanged — ADR-030)
│   │   └── {agentId}/memory/*.jsonld
│   ├── contributor-profile/            NEW: profile.ttl, goals.jsonld,
│   │                                        collaborators.jsonld,
│   │                                        preferences.jsonld,
│   │                                        share-log.jsonld (append-only),
│   │                                        kill-switch.jsonld
│   ├── workspaces/                     NEW: {workspace-id}.jsonld —
│   │                                        ContributorWorkspace snapshots
│   ├── automations/                    NEW: {routine-id}.json —
│   │                                        scheduled routine specs
│   ├── skill-evals/                    NEW: {run-id}.jsonl —
│   │                                        personal eval runs, benchmarks
│   └── skills/                         (ADR-057: installed skill packages)
├── public/                             (ADR-052: foaf:Agent Read,
│   │                                        owner Write/Control, acl:default)
│   ├── .acl                            (unchanged)
│   ├── kg/                             (unchanged)
│   ├── skills/                         (ADR-057: Mesh-published skills)
│   └── workspaces/                     (ADR-057: opt-in templates)
├── shared/                             (ADR-052: placeholder; now populated)
│   ├── .acl                            (unchanged — owner-only root;
│   │                                        per-team ACLs inside)
│   ├── skills/                         NEW: {team}/ — team-scoped skills
│   ├── workspaces/                     NEW: {team}/ — team workspace templates
│   └── memory/                         NEW: {team}/ — team-shared memory
├── inbox/                              NEW: {agent-ns}/{item-id}.jsonld —
│                                            agent-delivered briefs awaiting
│                                            review
└── profile/                            (unchanged — WebID doc, NIP-39 claim)
```

Every container marked NEW is created at the first Studio visit by the
migration job in §10. Existing ADR-052 containers are untouched.

### 2.2 Per-container matrix

| Container | Who writes | Who reads | WAC template | Cache mode |
|-----------|-----------|-----------|--------------|------------|
| `/private/contributor-profile/` | owner | owner + Studio backend (delegated cap) | owner-only + `acl:default` | Solid Notifications |
| `/private/workspaces/` | owner via Studio | owner | owner-only | Solid Notifications |
| `/private/automations/` | owner | owner + `AutomationOrchestratorActor` (delegated cap) | owner-only | Poll 60 s |
| `/private/skill-evals/` | owner via `SkillEvaluationActor` (delegated cap) | owner | owner-only | Lazy (on `/evals/run`) |
| `/private/agent-memory/{agentId}/` | agent (delegated cap, scoped) | owner + agent | owner-only; ADR-030 pattern | Solid Notifications |
| `/shared/skills/{team}/` | team members via WAC named-group | team members | named-group Read + Write | Solid Notifications |
| `/shared/workspaces/{team}/` | team members | team members | named-group Read + Write | Solid Notifications |
| `/shared/memory/{team}/` | team members | team members | named-group Read + Write (fine-grained Append for agents) | Solid Notifications |
| `/inbox/{agent-ns}/` | named agents via NIP-26 delegated cap (Append-only) | owner | owner-only + cap-scoped Append for the agent | Solid Notifications + 30 s poll |
| `/public/skills/` | owner (via ShareOrchestrator on Broker approve) | world | foaf:Agent Read, owner Write/Control | Solid Notifications |
| `/public/workspaces/` | owner (opt-in) | world | foaf:Agent Read, owner Write/Control | Solid Notifications |

**Cap discipline.** Every "delegated cap" entry in the *Who writes* column
is a NIP-26 delegation token scoped to `{tool_scopes, data_scopes, ttl}`
(see §8.3). A cap with no `data_scopes` entry matching the target container
path MUST be rejected by the Pod-facing middleware.

### 2.3 Example WAC snippets

Owner-only template (`/private/contributor-profile/.acl`):

```turtle
@prefix acl: <http://www.w3.org/ns/auth/acl#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

<#owner>
    a acl:Authorization ;
    acl:agent <https://alice.pods.visionclaw.org/profile/card#me> ;
    acl:accessTo <./> ;
    acl:default <./> ;
    acl:mode acl:Read, acl:Write, acl:Control .
```

Named-group team template (`/shared/skills/team-alpha/.acl`):

```turtle
@prefix acl: <http://www.w3.org/ns/auth/acl#> .
@prefix vc: <urn:visionclaw:acl#> .

<#owner>
    a acl:Authorization ;
    acl:agent <https://alice.pods.visionclaw.org/profile/card#me> ;
    acl:accessTo <./> ;
    acl:default <./> ;
    acl:mode acl:Read, acl:Write, acl:Control .

<#team>
    a acl:Authorization ;
    acl:agentGroup <https://alice.pods.visionclaw.org/shared/groups/team-alpha#members> ;
    acl:accessTo <./> ;
    acl:default <./> ;
    acl:mode acl:Read, acl:Write .
```

Inbox delegated-write template (`/inbox/sensei/.acl`):

```turtle
@prefix acl: <http://www.w3.org/ns/auth/acl#> .

<#owner>
    a acl:Authorization ;
    acl:agent <https://alice.pods.visionclaw.org/profile/card#me> ;
    acl:accessTo <./> ;
    acl:default <./> ;
    acl:mode acl:Read, acl:Write, acl:Control .

<#sensei-append>
    a acl:Authorization ;
    acl:agent <https://alice.pods.visionclaw.org/agents/sensei#id> ;
    acl:accessTo <./> ;
    acl:default <./> ;
    acl:mode acl:Append .
```

Agents are restricted to `acl:Append` on their own `/inbox/{agent-ns}/`
sub-container. They cannot Read, Update, or Delete existing inbox items —
only add new ones. This contains a compromised-agent blast radius to
"noise in the inbox", never "silent modification of existing briefs".

## 3. ContributorProfile schema

Four documents sit under `/private/contributor-profile/`. All are
human-readable, Linked-Data-first, and subject to the append-only
share-log entry on every mutation.

### 3.1 profile.ttl

```turtle
@prefix vcard: <http://www.w3.org/2006/vcard/ns#> .
@prefix vc:    <urn:visionclaw:contributor#> .
@prefix foaf:  <http://xmlns.com/foaf/0.1/> .

<#me>
    a vc:Contributor, vcard:Individual ;
    vc:webid       <https://alice.pods.visionclaw.org/profile/card#me> ;
    vcard:fn       "Alice Example" ;
    vc:role        "senior-researcher" ;
    vc:activeProjects (
        <urn:visionclaw:project/contributor-studio>
        <urn:visionclaw:project/ontology-migration>
    ) ;
    vc:timezone    "Europe/London" ;
    vc:workHours   "08:30-18:00" ;
    vc:notificationPrefs [
        vc:channel "studio" ;
        vc:quietHoursStart "22:00" ;
        vc:quietHoursEnd   "07:00"
    ] ;
    vc:podProviderPreference "nss" ;
    vc:profileVersion "2026-04-20" .
```

Fields are schema-stable; additions require an ADR amendment to ADR-057.
Clients MUST tolerate unknown predicates (forward-compat).

### 3.2 goals.jsonld

```json
{
  "@context": {
    "vc":   "urn:visionclaw:contributor#",
    "time": "http://www.w3.org/2006/time#"
  },
  "@id":   "urn:visionclaw:contributor/alice/goals",
  "@type": "vc:ContributorGoals",
  "vc:shortTermGoals": [
    {
      "@type": "vc:Goal",
      "title": "Ship Studio MVP",
      "description": "Deliver §9 of PRD-003 with end-to-end share funnel.",
      "linkedOntologyIRI": "urn:visionclaw:ont/contributor-studio",
      "targetDate": "2026-05-15"
    }
  ],
  "vc:longTermAmbitions": [
    {
      "@type": "vc:Goal",
      "title": "Institutional memory compounding",
      "description": "Every skill I ship becomes the team baseline within 30 days.",
      "linkedOntologyIRI": "urn:visionclaw:ont/capability-compounding",
      "targetDate": "2026-12-31"
    }
  ],
  "vc:learningObjectives": [
    {
      "@type": "vc:LearningObjective",
      "title": "WAC named-group patterns",
      "description": "Build fluency in multi-team ACL authoring.",
      "linkedOntologyIRI": "urn:visionclaw:ont/solid-wac",
      "targetDate": "2026-06-01"
    }
  ]
}
```

Goals feed `OntologyGuidanceActor`: the Sensei weights nudges toward
skills and pages whose IRI is a 2-hop neighbour of any `linkedOntologyIRI`.
Goals are private to the contributor; they are never exposed in KPI
events (§12).

### 3.3 collaborators.jsonld

```json
{
  "@context": {
    "vc":       "urn:visionclaw:contributor#",
    "foaf":     "http://xmlns.com/foaf/0.1/",
    "bridge":   "urn:visionclaw:bridge#"
  },
  "@id":   "urn:visionclaw:contributor/alice/collaborators",
  "@type": "vc:CollaboratorList",
  "vc:entries": [
    {
      "@type": "vc:Collaborator",
      "foaf:name": "Bob",
      "vc:webid":  "https://bob.pods.visionclaw.org/profile/card#me",
      "bridge:BRIDGE_TO": {
        "nostr_pubkey": "npub1bob...",
        "oidc_subject": "bob@corp.example"
      },
      "vc:delegationScopes": [
        {
          "tool_scopes":   ["studio_context_assemble"],
          "data_scopes":   ["pod:/shared/skills/team-alpha/*"],
          "ttl_hours":     168,
          "granted_at":    "2026-04-20T10:00:00Z"
        }
      ],
      "vc:trustTier": "team",
      "vc:lastInteractionAt": "2026-04-19T16:20:00Z"
    }
  ]
}
```

`BRIDGE_TO` follows ADR-048 dual-tier identity — every collaborator
entry MUST carry a Nostr pubkey AND (if enterprise) an OIDC subject, so
that cross-tier identity spoofing is caught at the profile-read layer.

### 3.4 preferences.jsonld

```json
{
  "@context": { "vc": "urn:visionclaw:contributor#" },
  "@id":   "urn:visionclaw:contributor/alice/preferences",
  "@type": "vc:ContributorPreferences",
  "vc:ui": {
    "defaultPartnerTier":   "tier2-haiku",
    "senseiMaxNudgesPerDay": 6,
    "theme":                "dark",
    "paneLayout":           "graph-left"
  },
  "vc:automation": {
    "quietHours": { "tz": "Europe/London", "start": "22:00", "end": "07:00" },
    "maxRoutineRunsPerDay": 200,
    "allowOfflineMeshShares": false
  },
  "vc:sharing": {
    "defaultTargetScope":   "team",
    "requireExplicitRationale": true,
    "priorRejectionCooldownHours": 72
  },
  "vc:pod": {
    "preferredProvider": "nss",
    "fallbackProvider":  "css"
  },
  "vc:killSwitch": {
    "enabled": false,
    "activatedAt": null
  }
}
```

The `killSwitch` is the nuclear option: setting `enabled=true`
immediately revokes every NIP-26 delegated cap issued by this
contributor, drops every in-flight ShareIntent to `cancelled`, and stops
every scheduled automation. The backend ShareOrchestrator MUST honour
the kill-switch within 60 s of the pod write (Solid Notifications drive
the propagation; poll fallback bounds the worst case).

## 4. Episodic memory (extends ADR-030)

ADR-030 established `/private/agent-memory/{agentId}/memory/*.jsonld` as
the contributor-visible memory surface for each agent. This spec names
the Studio-side episodic loop that uses it.

### 4.1 Read path

```
Studio UI                   Backend                               Pod
    │                          │                                    │
    │  GET /api/studio/        │                                    │
    │  workspaces/:id/context  │                                    │
    ├─────────────────────────▶│                                    │
    │                          │  ContextAssemblyService            │
    │                          │    ├── workspace snapshot          │
    │                          │    │   (Pod READ: /private/        │
    │                          │    │    workspaces/:id.jsonld) ◀──────────┤
    │                          │    ├── ontology neighbours         │
    │                          │    │   (backend index)             │
    │                          │    └── episodic memory slice       │
    │                          │        (Pod READ: /private/        │
    │                          │         agent-memory/*)  ◀────────────────┤
    │                          │                                    │
    │  ◀── ContextEnvelope ────┤                                    │
```

Filters applied during assembly:

- **Time window.** Default last 30 days; operator override per workspace.
- **Ontology-neighbour overlap.** Keep only episodic entries whose
  `focus_snapshot_hash` or `artifact_ref` touches an IRI within 2 hops
  of the current workspace focus.
- **Project tag.** Entries whose `project_id` does not match the
  workspace's active project are filtered unless the contributor
  explicitly opts in.

The envelope is cached in `ContextAssemblyActor` for the lifetime of
the `GuidanceSession` (reset on workspace close or a 30-min idle
timeout). On Pod Notification for any read path (workspace, agent
memory), the cache entry is invalidated and recomputed lazily.

### 4.2 Write path

Every event below MUST emit a new episodic entry to
`/private/agent-memory/{originating-agent}/memory/{entry-id}.jsonld`
*before* the outbound effect is acknowledged.

| Event | Emitting actor | Entry `event_type` | Pod path |
|-------|----------------|--------------------|----------|
| `ShareIntentCreated` | `ShareOrchestratorActor` | `share-intent-created` | `/private/agent-memory/share-orch/memory/` |
| `ShareIntentApproved` | `ShareOrchestratorActor` | `share-intent-approved` | same |
| `ShareIntentRejected` | `ShareOrchestratorActor` | `share-intent-rejected` | same |
| `SuggestionAccepted` | `OntologyGuidanceActor` | `suggestion-accepted` | `/private/agent-memory/sensei/memory/` |
| `SuggestionDismissed` | `OntologyGuidanceActor` | `suggestion-dismissed` | same |
| `SkillInstalled` | `SkillRegistrySupervisor` | `skill-installed` | `/private/agent-memory/skill-registry/memory/` |
| `SkillEvalRun` | `SkillEvaluationActor` | `skill-eval-run` | same |
| `AutomationTriggered` | `AutomationOrchestratorActor` | `automation-triggered` | `/private/agent-memory/automation-orch/memory/` |

Entry shape:

```json
{
  "@context": "https://schema.org",
  "@type": "DigitalDocument",
  "identifier": "ep-2026-04-20-00042",
  "name": "ShareIntentApproved: skill:research-brief → team-alpha",
  "additionalProperty": [
    { "@type": "PropertyValue", "name": "event_type",
      "value": "share-intent-approved" },
    { "@type": "PropertyValue", "name": "timestamp",
      "value": "2026-04-20T11:03:27Z" },
    { "@type": "PropertyValue", "name": "focus_snapshot_hash",
      "value": "sha256:7f8a9b…" },
    { "@type": "PropertyValue", "name": "artifact_ref",
      "value": "pod:/shared/skills/team-alpha/research-brief.md" },
    { "@type": "PropertyValue", "name": "outcome",
      "value": "approved" }
  ]
}
```

### 4.3 Privacy split

Episodic memory (pod-only) carries **content**. KPI events (BC15
pipeline) carry only **ids and outcomes**. The two streams are never
cross-joined outside the contributor's workspace.

| Field | Pod (episodic) | KPI (BC15) |
|-------|:--------------:|:----------:|
| `event_type`          | ✓ | ✓ (as outcome code) |
| `timestamp`           | ✓ | ✓ |
| `workspace_id`        | ✓ | ✓ |
| `webid`               | ✓ | hashed (HMAC per §12) |
| `artifact_ref` (pod URI) | ✓ | ✗ (replaced with anonymised id) |
| `focus_snapshot_hash` | ✓ | ✗ |
| `content` (any free text, rationale, summary) | ✓ | ✗ |
| `outcome_code`        | ✓ | ✓ |
| `tier_used` (ADR-026) | ✓ | ✓ |

KPI publishers MUST use the `EpisodicRedactor` adapter which strips
every pod URI, hashes the WebID with the per-workspace HMAC key, and
drops free-text fields. Bypassing this adapter is an Invariant-3 bug.

## 5. Automation routines

Automations live in `/private/automations/{routine-id}.json` and are
executed by `AutomationOrchestratorActor` (peer of `TaskOrchestratorActor`
per ADR-057).

### 5.1 Routine schema

```json
{
  "routine_id":   "uuid",
  "owner_webid":  "https://alice.pods.visionclaw.org/profile/card#me",
  "name":         "Daily research brief",
  "description":  "Assemble a brief of the last 24h of graph activity.",
  "schedule":     "0 8 * * 1-5",
  "trigger": {
    "type":    "time",
    "cron":    "0 8 * * 1-5",
    "tz":      "Europe/London"
  },
  "action": {
    "skill_id":   "urn:skill:research-brief",
    "version":    "1.3.0",
    "parameters": { "graph_view": "active-projects", "depth_hops": 2 }
  },
  "output_target": {
    "kind": "inbox",
    "path": "/inbox/research-brief/"
  },
  "permissions": {
    "tool_scopes":  ["studio_context_assemble", "ontology_discover"],
    "data_scopes":  ["pod:/private/kg/*", "pod:/private/agent-memory/*"]
  },
  "delegated_cap_id":   "urn:nip26:cap:alice:research-brief:2026-04-20",
  "delegated_cap_expiry": "2026-10-18T00:00:00Z",
  "active":     true,
  "quiet_hours": { "tz": "Europe/London", "start": "22:00", "end": "07:00" },
  "max_runs_per_day": 3,
  "consecutive_failure_suspend_threshold": 3,
  "created_at": "2026-04-20T09:00:00Z",
  "expires_at": "2026-10-20T00:00:00Z",
  "schema_version": "1.0"
}
```

Invariants:

- `permissions.data_scopes` MUST be a subset of the scopes on
  `delegated_cap_id`. The orchestrator rejects out-of-scope routines at
  load time.
- `delegated_cap_expiry` ≤ `expires_at`. An expired cap deactivates the
  routine; an expired routine releases its cap.
- `output_target.path` MUST match `output_target.kind` (`inbox` ⇒
  `/inbox/*`; `pod_path` ⇒ `/private/**` or `/shared/**`; `graph_mutation`
  ⇒ backend-only, never a pod write).

### 5.2 AutomationOrchestratorActor

```
┌────────────────────────────────────────────────────────┐
│ AutomationOrchestratorActor (supervised by AppSup)      │
│                                                        │
│   ┌────────────────┐     next_fire_at min-heap          │
│   │ Routine loader │───▶ ┌────┬────┬────┬────┐          │
│   │  (Pod READ +   │     │ r1 │ r2 │ r3 │ … │          │
│   │   Notifications)│    └────┴────┴────┴────┘          │
│   └────────────────┘          │                         │
│           ▲                   ▼ tick                    │
│           │           ┌──────────────────┐              │
│           │           │ Cap verify       │──fail──┐     │
│           │           │ Scope verify     │        │     │
│           │           │ Quiet-hours gate │        │     │
│           │           │ Rate-limit gate  │        │     │
│           │           │ Kill-switch gate │        │     │
│           │           └──────────────────┘        │     │
│           │                   │ pass              │     │
│           │                   ▼                   ▼     │
│           │        TaskOrchestratorActor    /inbox/DLQ  │
│           │               (scoped identity) + deactivate│
│           │                   │                         │
│           │                   ▼                         │
│           │        write output_target                  │
│           │        emit AutomationTriggered → ep-log    │
│           │        emit KPI (BC15, redacted)            │
│           └─ reload on Pod Notification ─────────────────│
└────────────────────────────────────────────────────────┘
```

The loader subscribes to Solid Notifications on
`/private/automations/`; changes replace the affected entries in the
min-heap. On actor restart, the loader rehydrates from Pod read.

### 5.3 Offline automations

A contributor is **offline** when they have had no active
`/api/ws/studio` session for 15 min. The orchestrator continues to
fire routines but applies an offline-mode policy:

- **Permitted writes.** `/inbox/*`, `/private/**`.
- **Forbidden writes.** `/shared/**`, `/public/**`.
- **Forbidden effects.** `share_intent_create(target=mesh)`,
  `share_intent_create(target=team)` when policy rule
  `offline_team_share_block` is enabled (default off for Team, on for
  Mesh).
- **Forbidden tool calls.** Any tool whose scope intersects
  `mesh:*` or `broker:*`.

Enforcement is by Policy Engine rule `offline_mesh_block`:

```toml
[policies.offline_mesh_block]
enabled = true
applies_to = ["share_intent_create"]
forbidden_target_scopes = ["mesh"]
offline_threshold_minutes = 15
action_on_breach = "deny"
```

Violations are denied at the policy check; the orchestrator writes
an error manifest into `/inbox/{routine}/errors/`.

### 5.4 Three example routines

**(a) Daily research brief** — time-triggered, inbox-only:

```json
{
  "routine_id": "rt-001", "name": "Daily research brief",
  "schedule":   "0 8 * * 1-5",
  "trigger":    { "type": "time", "cron": "0 8 * * 1-5", "tz": "Europe/London" },
  "action":     { "skill_id": "urn:skill:research-brief", "version": "1.3.0",
                  "parameters": { "depth_hops": 2 } },
  "output_target": { "kind": "inbox", "path": "/inbox/research-brief/" },
  "permissions": {
    "tool_scopes": ["studio_context_assemble", "ontology_discover"],
    "data_scopes": ["pod:/private/kg/*", "pod:/private/agent-memory/*"]
  }
}
```

**(b) Weekly stale-node sweep** — graph-event-triggered, writes to
pod path:

```json
{
  "routine_id": "rt-002", "name": "Weekly stale-node sweep",
  "schedule":   "0 9 * * MON",
  "trigger":    { "type": "graph_event",
                  "event": "staleness_scan_complete" },
  "action":     { "skill_id": "urn:skill:stale-sweep", "version": "2.0.1",
                  "parameters": { "threshold_days": 90 } },
  "output_target": { "kind": "pod_path",
                     "path": "/private/workspaces/stale-report-weekly.jsonld" },
  "permissions": {
    "tool_scopes": ["ontology_discover", "graph_query"],
    "data_scopes": ["pod:/private/kg/*", "pod:/private/workspaces/*"]
  }
}
```

**(c) On-new-broker-case audit** — ontology-event-triggered, emits a
brief per event:

```json
{
  "routine_id": "rt-003", "name": "Audit new broker cases",
  "trigger":    { "type": "ontology_event",
                  "event": "BrokerCaseOpened",
                  "filter": { "category": "contributor_mesh_share",
                              "subject_contributor": "self" } },
  "action":     { "skill_id": "urn:skill:case-audit-brief", "version": "0.9.2" },
  "output_target": { "kind": "inbox", "path": "/inbox/broker-audit/" },
  "permissions": {
    "tool_scopes": ["broker_case_read"],
    "data_scopes": ["pod:/private/contributor-profile/share-log.jsonld"]
  }
}
```

## 6. The Inbox

`/inbox/` is the one pod container that accepts writes from non-owners
(scoped agents). Every other writable container is owner-only.

### 6.1 Lifecycle

```
   created ───triage──▶ triaged ──┬─ accepted ─────▶ [action fired]
                                  ├─ dismissed ────▶ [retain 14d, then purge]
                                  ├─ escalated ────▶ BrokerCase opened
                                  └─ snoozed ──────▶ [re-triage at wake_at]
```

Timings:

- Default TTL from `created`: 14 days. On expiry without triage the
  item is moved to `/inbox/.archive/{yyyy-mm}/`.
- `snoozed.wake_at` MUST be within 30 days of `created`; longer
  snoozes require explicit re-creation.
- `escalated` is terminal from the inbox's point of view; the BrokerCase
  carries the workflow from there.

### 6.2 Item schema

```json
{
  "@context": "urn:visionclaw:inbox",
  "@type":    "InboxItem",
  "item_id":  "uuid",
  "created_at": "2026-04-20T10:17:11Z",
  "source": {
    "kind": "routine",
    "ref":  "urn:routine:rt-001",
    "agent_webid": "https://alice.pods.visionclaw.org/agents/research-brief#id"
  },
  "topic":    "Daily research brief — 2026-04-20",
  "summary":  "3 new nodes, 12 new edges, 2 staleness warnings.",
  "content_ref": "pod:/private/workspaces/briefs/2026-04-20-research.md",
  "suggested_actions": [
    { "action_id": "act-1", "label": "Open in workspace",
      "target": "studio:workspace/open?ref=..." },
    { "action_id": "act-2", "label": "Share to team",
      "target": "studio:share-intent/open?artifact=..." }
  ],
  "provenance_chain": [
    { "step": "routine-scheduled", "at": "2026-04-20T08:00:00Z",
      "signed_by": "urn:nip26:cap:alice:research-brief:2026-04-20" },
    { "step": "tool-call:studio_context_assemble", "at": "…",
      "tier": "tier2-haiku" }
  ],
  "ttl":      "P14D",
  "priority": "normal",
  "status":   "created"
}
```

Integrity rules:

- `content_ref` MUST resolve under `/private/**` or `/shared/**`.
  Agents MAY NOT embed content directly in the inbox item; this forces
  cap-scoped writes to flow through known pod paths and keeps the
  inbox index small.
- `provenance_chain[0].signed_by` MUST be the `delegated_cap_id` under
  which the item was written. The pod-facing middleware refuses inbox
  Appends without this binding.
- `suggested_actions[].target` MUST be a Studio-registered deep-link
  scheme; `http(s)://` outbound links are rejected at render time to
  prevent phishing.

### 6.3 Triage actions

| Action | Effect on item | Emitted event | Downstream |
|--------|----------------|---------------|------------|
| Accept + invoke | status → `accepted`; fires `suggested_actions[i]` | `InboxItemAccepted` | Studio deep-link handler |
| Dismiss | status → `dismissed` | `InboxItemDismissed` | retention until TTL |
| Escalate to broker | status → `escalated` | `InboxItemEscalated` → BC11 `BrokerCase(category="inbox_escalation")` | BrokerSupervisor |
| Snooze | status → `snoozed` with `wake_at` | `InboxItemSnoozed` | AutomationOrchestrator re-triage fires at `wake_at` |
| Defer to workspace | status → `triaged` + open workspace | `InboxItemDeferred` | `ContextAssemblyActor` |

All five actions write an episodic entry (`/private/agent-memory/inbox/`)
and a KPI outcome (redacted).

### 6.4 Delegated-cap write path

Agents write to `/inbox/{agent-ns}/` under a NIP-26 delegated cap issued
by the owner. The cap document is stored under
`/private/contributor-profile/caps/{cap-id}.jsonld` and referenced from
`collaborators.jsonld`.

```
Agent                          Backend inbox endpoint            Pod
  │                                    │                           │
  │  POST /api/inbox/{ns}/append       │                           │
  │  body: InboxItem (no id)           │                           │
  │  header: NIP-98 signed by          │                           │
  │          cap.delegatee_pubkey      │                           │
  ├───────────────────────────────────▶│                           │
  │                                    │ 1. resolve cap from pod   │
  │                                    │    (cached; invalidated   │
  │                                    │     on kill-switch)       │
  │                                    │ 2. verify                 │
  │                                    │    - cap.ttl not expired  │
  │                                    │    - tool_scopes ⊇ needed │
  │                                    │    - data_scopes ⊇ path   │
  │                                    │    - NIP-98 sig valid     │
  │                                    │    - owner not kill-switched│
  │                                    │ 3. assign item_id         │
  │                                    │ 4. attach provenance.step[0]│
  │                                    │ 5. Pod APPEND (owner-ns)  │
  │                                    ├──────────────────────────▶│
  │  201 Created, item_ref             │                           │
  │ ◀──────────────────────────────────┤                           │
```

Caps renew 48 h before expiry via a Studio notification; the owner
approves with a single NIP-07 signature. Revocation is two-stage:
setting `active=false` on the cap document triggers an immediate
backend cache-flush; the cap is also listed in the contributor
kill-switch registry until expiry for defence-in-depth.

## 7. Share-to-Mesh funnel

Three states, six governed transitions, one broker adapter.

### 7.1 Three states

| State | Storage | WAC | Who sees | Reversible? |
|-------|---------|-----|----------|-------------|
| Private | `/private/{kind}/` | owner-only | contributor | Yes — delete or Team-promote |
| Team | `/shared/{kind}/{team}/` | named-group Read + Write | team members | Yes — `ShareIntentRevoked` demotes to Private |
| Mesh | Neo4j canonical copy + `/public/{kind}/` when applicable | public-read on `/public/` | all contributors; optionally federated | Only via Broker-driven revocation |

The Neo4j canonical copy is an index entry, not a replacement for the
pod source. The pod MOVE into `/public/{kind}/` is what makes a mesh
promotion visible to the outside world (per ADR-052 double-gate). Mesh
promotion without a `/public/` copy is valid for artefact types that
are graph-only (e.g. `ontology_term` migrations, where the `/public/`
copy is implicit in the ontology publication).

### 7.2 Transition matrix

| From | To | Trigger | Policy checks (ADR-045) | Emitted events | WAC / adapter effect |
|------|----|---------|-------------------------|----------------|----------------------|
| Private | Team | `ShareIntent(target_scope=team:{t})` | `pii_scan`, `team_scope_validated`, `delegation_cap_valid`, `rate_limit`, `offline_team_share_block` | `ShareIntentCreated` → `ShareIntentApproved` | Pod MOVE to `/shared/{kind}/{t}/`; apply named-group ACL; update `WorkArtifactIndex` / `SkillIndex` |
| Private | Mesh | **forbidden direct** | — | — | Must route via Team first (default) OR via a fast-path BrokerCase when policy rule `fast_path_mesh_share` is enabled for the contributor (enterprise only; audit-heavy) |
| Team | Mesh | `ShareIntent(target_scope=mesh)` | `broker_review_required`, `pii_rescan`, `mesh_eligibility`, `prior_rejection_cooldown`, `separation_of_duty` | `ShareIntentCreated` → `BrokerCaseOpened` | No WAC change yet; `ShareIntentBrokerAdapter` opens BC11 case (category per §7.6); broker decision drives promotion or rejection |
| Team | Private | `ContributorRevocation` | `authority_check` (contributor owns artefact OR team lead) | `TeamShareRevoked` | Pod MOVE back to `/private/{kind}/`; remove from team index |
| Mesh | Team | `BrokerRevocation(demote)` | `broker_authority` | `MeshRevoked` | Pod MOVE out of `/public/{kind}/`; retain team copy; signed revocation note archived |
| Mesh | Removed | `BrokerDecision(retract)` | `broker_authority` | `MeshRetracted` | Pod DELETE of `/public/{kind}/` copy; signed revocation retained; `/shared/` copy archived to `/private/{kind}/archive/` |

### 7.3 ShareOrchestratorActor

Lifecycle outline:

```
on receive ShareIntentCreated(intent):
    episodic.write("share-intent-created", intent)
    decision = policy.evaluateIntent(intent)  // §7.4

    match decision.outcome:
      Deny(reason):
        emit ShareIntentRejected(intent.id, reason)
        episodic.write("share-intent-rejected", …)
        kpi.emit("share_intent_outcome", code="denied")
        return

      Allow:
        match intent.target_scope:
          team(t):
            pod.move(intent.artifact, dst=/shared/{kind}/{t}/)
            wac.apply_named_group(t, dst)
            index.update(WorkArtifactIndex | SkillIndex)
            emit ShareIntentApproved(intent.id)
            episodic.write("share-intent-approved", …)
            share_log.append(owner, entry)
            kpi.emit("share_intent_outcome", code="team_approved")

          mesh:
            case = broker.open_case(build_broker_case(intent))
            emit BrokerCaseOpened(case.id)
            episodic.write("share-intent-broker-opened", …)
            share_log.append(owner, entry)
            kpi.emit("share_intent_outcome", code="broker_opened")

      Escalate:
        // Team→Mesh path: identical to Allow→mesh above
        case = broker.open_case(build_broker_case(intent,
                                   reason=decision.reasons))
        emit BrokerCaseOpened(case.id)
        episodic.write("share-intent-broker-opened", …)
        kpi.emit("share_intent_outcome", code="escalated")

on receive BrokerDecision(case_id, decision):
    intent = lookup(case_id)
    match decision:
      Promote: promote_to_mesh(intent)
      Demote:  demote_to_team(intent)
      Retract: retract_from_mesh(intent)
      Reject:  apply_cooldown(intent)
    emit appropriate event; episodic.write; share_log.append; kpi.emit
```

The actor writes an append-only entry to
`/private/contributor-profile/share-log.jsonld` on every state change.
The pod write precedes the `emit` (Invariant 1).

### 7.4 Policy Engine hooks (ADR-045)

Per-transition rule set, all implemented as `PolicyRule` structs:

| Transition | Rule ids |
|------------|----------|
| Private → Team | `pii_scan`, `team_scope_validated`, `delegation_cap_valid`, `rate_limit`, `offline_team_share_block` |
| Team → Mesh | `broker_review_required`, `pii_rescan`, `mesh_eligibility`, `prior_rejection_cooldown`, `separation_of_duty`, `rate_limit` |
| Retirement (Mesh → removed) | `archive_required`, `broker_authority` |
| Inbox → BrokerCase escalation | `escalation_threshold`, `separation_of_duty` |

Rule implementations of note:

- `pii_scan` / `pii_rescan` — wraps the `aidefence_has_pii` MCP tool
  over the artefact body and every `suggested_actions.target`.
  Returns `Escalate` (not `Deny`) when PII is detected on team share
  (contributor may redact and retry); returns `Deny` on mesh share.
- `mesh_eligibility` — calls `ontology_discover` MCP tool to verify the
  artefact IRI is a valid mesh publication target (e.g. ontology terms
  must have a resolvable parent class).
- `prior_rejection_cooldown` — queries the share-log for prior
  rejections of the same artefact; enforces
  `preferences.sharing.priorRejectionCooldownHours` (default 72 h).
- `offline_team_share_block` — disabled by default for Team but
  enabled for Mesh (§5.3).

Every evaluation emits a `PolicyEvaluated` provenance event per ADR-045.

### 7.5 Broker intake contract

`ShareOrchestratorActor` constructs a `BrokerCase` from a ShareIntent:

```json
{
  "@type":     "BrokerCase",
  "category":  "contributor_mesh_share",
  "subject_kind":     "skill",
  "subject_ref":      "pod:/shared/skills/team-alpha/research-brief.md",
  "contributor_webid": "https://alice.pods.visionclaw.org/profile/card#me",
  "share_intent_id":   "urn:share-intent:si-2026-04-20-0042",
  "payload": {
    "skill_version":     "1.3.0",
    "benchmark_ref":     "pod:/private/skill-evals/research-brief-v1.3.0.jsonl",
    "team_adoption":     5,
    "rationale":         "Shared baseline for the research pod."
  },
  "provenance": [
    { "step": "share_intent_created", "at": "2026-04-20T10:00:00Z" },
    { "step": "policy_evaluated",     "at": "2026-04-20T10:00:01Z",
      "evaluation_id": "pe-…" },
    { "step": "broker_case_opened",   "at": "2026-04-20T10:00:02Z" }
  ],
  "policy_eval_id": "pe-2026-04-20-0001",
  "created_at":     "2026-04-20T10:00:02Z"
}
```

The broker is free to re-evaluate and attach additional provenance
steps; the share-intent id is the stable link back to the pod artefact.

### 7.6 Adapter table

| `subject_kind` | Routes to | Aggregate created on approve |
|----------------|-----------|------------------------------|
| `skill` | BC11 `BrokerCase(category="contributor_mesh_share", subject_kind="skill")` + optional BC12 `WorkflowPattern` candidate | `SkillDistribution` update, `/public/skills/` published |
| `ontology_term` | BC11 `BrokerCase(category="contributor_mesh_share", subject_kind="ontology_term")` → delegates to ADR-049 `migration_candidate` on broker approve | `MigrationPayload` populated; PR triggered on approve |
| `workflow` | BC11 `BrokerCase(category="contributor_mesh_share", subject_kind="workflow")` → advances BC12 `WorkflowProposal` per ADR-042 | `WorkflowProposal` advanced to `Approved` |
| `work_artifact` | BC11 `BrokerCase(category="contributor_mesh_share", subject_kind="work_artifact")` generic | `WorkArtifactIndex` registered; optional `/public/` copy |
| `graph_view` | BC13 `InsightCandidate` | `InsightCandidate` at `confirmed` threshold |

The adapter is the sole surface through which contributor Studio
content becomes broker-visible; every broker decision flows back
through `ShareOrchestratorActor` to drive the WAC mutation and pod
MOVE.

## 8. Security threat model

### 8.1 STRIDE

| # | Threat | Vector | Control | Residual risk |
|---|--------|--------|---------|---------------|
| 1 | Spoofing | NIP-07 session reuse on shared browser | Signed handshake per ADR-040; session TTL 8 h; re-auth on high-privilege actions (kill-switch, cap issue, mesh-share) | Low |
| 2 | Spoofing | Collaborator WebID impersonation | `BRIDGE_TO` dual-tier check on collaborator load; IdP binding verified per-request | Low |
| 3 | Tampering | Pod MITM on write | NIP-98 signed writes + server-side content-hash check; TLS-pinned pod provider URLs | Low |
| 4 | Tampering | Agent modifies its own inbox items | Agents scoped to `acl:Append` on `/inbox/{ns}/`; Update/Delete are owner-only | Very low |
| 5 | Tampering | Forged episodic entries | Every entry signed by emitting actor key; backend episodic reader validates sig | Low |
| 6 | Repudiation | Contributor denies a share intent | Signed `ShareIntentCreated` + append-only `share-log.jsonld`; log hash-chained per entry | Very low |
| 7 | Repudiation | Agent denies an inbox append | `provenance_chain[0].signed_by` binds the cap id; cap document carries owner sig | Very low |
| 8 | Info disclosure | WAC misconfig leak of `/shared/{team}/` | Double-gate (artefact distribution-scope + path); ACL lint on every profile change; CI test for "any foaf:Agent grant outside /public/" | Medium — misconfig is always possible; mitigation heavy |
| 9 | Info disclosure | Sensei suggestion leaks content across workspaces | `NudgeComposer` runs `aidefence_has_pii` on every suggestion before emit; suggestions scoped to `workspace_id` in cache | Low |
| 10 | Info disclosure | Episodic entry bleeds into KPI stream | `EpisodicRedactor` adapter required; bypass is an Invariant-3 bug; CI test for KPI events containing pod URIs | Low |
| 11 | Info disclosure | Delegated cap widened on re-issue | Renewal UX shows diff; caps are immutable once signed; renewal creates a new cap id | Low |
| 12 | Denial of service | Automation bomb | `max_runs_per_day` default 200; orchestrator rate-limits per-contributor; `consecutive_failure_suspend_threshold` default 3 | Low |
| 13 | Denial of service | Inbox spam from compromised cap | Cap is Append-only on a single `/{ns}/` path; spam is bounded; owner kill-switch revokes in 60 s | Low |
| 14 | Denial of service | ShareIntent bomb | `rate_limit` policy rule (default 10/h per contributor); policy evaluation is < 5 ms per ADR-045 | Low |
| 15 | Elevation | Delegation-cap scope abuse | Cap scoped to `tool_scopes[], data_scopes[], ttl`; server refuses any out-of-scope action; kill-switch in profile | Low |
| 16 | Elevation | Team-group membership spoof | Group membership verified via IdP callback; WAC linter checks group-doc signature | Low |
| 17 | Elevation | Offline automation escapes to mesh | `offline_mesh_block` policy rule + actor-side gate; kill-switch independent | Low |
| 18 | Elevation | Broker-bypass via pod direct-write to `/public/` | ADR-052 double-gate (artefact flag + path); CI test for published artefacts without a matching `BrokerDecision` | Low |

### 8.2 WAC double-gate (extended to Team)

ADR-052 ratified the double-gate for `/public/` (artefact has
`public:: true` AND target path is under `/public/`). This spec extends
the same pattern to Team shares:

- **Gate 1 (artefact).** Artefact manifest carries a
  `distribution_scope` predicate whose value is `team:{t}` for one or
  more `t`; the `allow_list` array names each team.
- **Gate 2 (path).** Target pod path is `/shared/{kind}/{t}/` for a
  `t` named in `allow_list`.

Both MUST hold. A broken client that writes only the manifest (no path
move) or only the path move (no manifest) is caught at the backend
middleware and returns `403 share_scope_mismatch`. The same pattern
applies to Mesh (artefact `distribution_scope=mesh` + target under
`/public/`).

Example skill manifest (`.skill.meta.jsonld` alongside `SKILL.md`):

```json
{
  "@context": "urn:visionclaw:skill",
  "@type":    "SkillManifest",
  "skill_id": "urn:skill:research-brief",
  "version":  "1.3.0",
  "distribution_scope": "team",
  "allow_list":         ["team-alpha", "team-beta"],
  "pii_scan_last": "2026-04-20T10:00:00Z",
  "benchmarks_ref": "pod:/private/skill-evals/research-brief-v1.3.0.jsonl"
}
```

The middleware refuses any `/shared/skills/team-gamma/` write whose
manifest does not include `team-gamma` in `allow_list`.

### 8.3 Delegation caps (NIP-26)

Cap document shape (stored at
`/private/contributor-profile/caps/{cap-id}.jsonld`):

```json
{
  "@context": "urn:visionclaw:nip26",
  "@id": "urn:nip26:cap:alice:sensei:2026-04-20",
  "@type": "DelegationCap",
  "delegator_webid":  "https://alice.pods.visionclaw.org/profile/card#me",
  "delegator_npub":   "npub1alice...",
  "delegatee_pubkey": "npub1sensei...",
  "tool_scopes": ["sensei_nudge", "ontology_discover"],
  "data_scopes": [
    "pod:/inbox/sensei/*",
    "pod:/private/agent-memory/sensei/*"
  ],
  "ttl_hours": 24,
  "granted_at":   "2026-04-20T09:00:00Z",
  "expires_at":   "2026-04-21T09:00:00Z",
  "owner_sig":    "nip26:sig:...",
  "active":       true,
  "revocation_note": null
}
```

Evaluation rules (enforced by middleware before any pod or tool call):

1. `active=true` AND now < `expires_at`.
2. Requested tool ∈ `tool_scopes`.
3. Every path the tool will touch is prefix-matched by at least one
   `data_scopes` glob.
4. NIP-98 request signature is valid for the request body using
   `delegatee_pubkey`.
5. `owner_sig` validates against `delegator_npub`.
6. Owner is not kill-switched (cached 60 s; invalidated on pod
   notification).

**Renewal UX.** At `expires_at - 48 h`, Studio surfaces a renewal card
showing a diff of current vs. proposed cap (which SHOULD be identical
scope; widening requires a full re-issue). One NIP-07 signature creates
a new cap; the old cap auto-expires at its TTL. Caps never renew
silently.

**Kill-switch UX.** Setting
`/private/contributor-profile/preferences.jsonld` `killSwitch.enabled=true`
triggers a pod write that Solid Notifications broadcast to the backend
within 60 s (poll fallback 60 s for NSS without notifications). The
backend flushes the cap cache and refuses every subsequent cap-scoped
request. Existing in-flight requests complete but emit no further
side-effects (idempotency protects against partial writes).

### 8.4 Audit log

`/private/contributor-profile/share-log.jsonld` is append-only and
hash-chained. Entry shape:

```json
{
  "@id":   "urn:share-log:alice:2026-04-20:00042",
  "@type": "ShareLogEntry",
  "prev_hash":   "sha256:…",
  "entry_hash":  "sha256:…",
  "at":          "2026-04-20T10:00:02Z",
  "actor_webid": "https://alice.pods.visionclaw.org/profile/card#me",
  "event":       "share-intent-approved",
  "share_intent_id": "urn:share-intent:si-…",
  "artifact_ref": "pod:/shared/skills/team-alpha/research-brief.md",
  "policy_eval_id": "pe-…",
  "outcome": "approved"
}
```

Hash chain: `entry_hash = sha256(prev_hash || canonical_json(entry))`.
A gap or replayed entry is detected on read; corruption triggers an
admin alert.

Export: the `Auditor` role (PRD insight-migration-loop §4) may read
`share-log.jsonld` through a read-only pod proxy, scoped to the
contributor's explicit consent. Rotation: the log rolls annually to
`/private/contributor-profile/share-log-archive/YYYY.jsonld`; the new
log's `prev_hash` is the final entry of the old log.

## 9. Cache coherence

Extends ADR-052 Solid Notifications model to the containers defined
here.

```
Pod                                 Backend                 Studio client
 │                                     │                          │
 │ owner writes profile.ttl            │                          │
 │────▶ WebSocketChannel2023           │                          │
 │         UPDATE event                │                          │
 │─────────────────────────────────────▶│                         │
 │                                     │ ContextAssemblyActor     │
 │                                     │   invalidate(workspace)  │
 │                                     │ DojoDiscoveryActor       │
 │                                     │   refresh(team-index)    │
 │                                     │ Inbox read model         │
 │                                     │   reindex(owner)         │
 │                                     │ Ship WS event            │
 │                                     │─────────────────────────▶│
 │                                     │                          │ re-render
```

Subscription matrix:

| Container | Actor that subscribes | Cache scope |
|-----------|-----------------------|-------------|
| `/private/contributor-profile/*` | `ContextAssemblyActor`, `AutomationOrchestratorActor` (kill-switch) | workspace context, cap cache |
| `/private/automations/*` | `AutomationOrchestratorActor` | routine min-heap |
| `/private/workspaces/*` | `ContextAssemblyActor` | workspace snapshot cache |
| `/shared/skills/{team}/*` | `DojoDiscoveryActor`, `SkillRegistrySupervisor` | team skill index |
| `/shared/workspaces/{team}/*` | `ContextAssemblyActor` | team template cache |
| `/shared/memory/{team}/*` | `ContextAssemblyActor` | team memory cache |
| `/inbox/*` | Studio Inbox read model | pending-item cache |
| `/public/skills/*` | `DojoDiscoveryActor` | mesh index |

Conflict resolution: the pod is the write-master (Invariant 1).
Concurrent writes from two browser sessions are serialised at the pod
(LDP If-Match ETag); the losing write receives `412 Precondition
Failed` and the client retries with the latest ETag.

Poll fallback: 60 s for providers without notifications (NSS < v8.x).
Stale-indicator surfaces in the Workspace status bar when the
notification channel has been down for > 120 s.

## 10. Migration and backfill

First Studio visit runs the `ContributorProfileBootstrap` job. Feature
flag `CONTRIBUTOR_MIGRATE_ON_LOGIN=true` (default).

Steps (idempotent):

1. Check existence of `/private/contributor-profile/`. If present,
   skip to step 6.
2. Read `/private/agent-memory/` (ADR-030) to seed preferences
   (`defaultPartnerTier` from last-used model).
3. Read Neo4j collaborator edges for the contributor's pubkey; seed
   `collaborators.jsonld` (entries marked `trustTier="pending"` until
   confirmed by the contributor).
4. Read the last 30 days of backend activity to seed `goals.jsonld`
   with three draft short-term goals (contributor confirms or deletes
   on first Studio visit).
5. Write `profile.ttl`, `goals.jsonld`, `collaborators.jsonld`,
   `preferences.jsonld`, empty `share-log.jsonld`, empty
   `kill-switch.jsonld`, `/private/workspaces/`, `/private/automations/`,
   `/private/skill-evals/`, `/inbox/`, `/shared/skills/`,
   `/shared/workspaces/`, `/shared/memory/`.
6. Register Solid Notification subscriptions for all containers listed
   in §9.
7. Emit `ContributorActivated` KPI event to BC15 (redacted).

`/private/agent-memory/` from ADR-030 is **referenced**, not moved.
Existing agent memory continues to live under its current path; the
episodic reader in §4 is the only new consumer.

## 11. Failure modes

| Failure | Symptom | Mitigation |
|---------|---------|------------|
| Pod offline (provider outage) | Studio degrades to read-only; ShareIntent creation returns 503 | Share intents queue in IndexedDB keyed by workspace_id; on reconnect, flush with policy re-check (inflight caps may have expired); stale indicator on status bar |
| Broker unavailable | Mesh intents emit `BrokerCaseQueued`; contributor sees "Queued" status | `ShareOrchestratorActor` retries with exponential backoff (1m, 5m, 15m, 1h, 6h); on repeated failure, escalate to `AdminAlert` |
| Automation failure (skill crash) | Routine exits with error | Error manifest written to `/inbox/{routine}/errors/`; counter increments; routine suspends after `consecutive_failure_suspend_threshold` (default 3); owner notified |
| Cap expired mid-run | Automation aborts in middle of tool call | Partial output + error in `/inbox/{routine}/errors/`; routine marked `active=false`; cap renewal notification surfaces in Studio |
| Notification provider down (NSS older than v8.x) | Cache drifts | Poll fallback 60 s; stale indicator on status bar when channel down > 120 s; operator alert on sustained drift |
| Pod cross-provider inconsistency (pod provider upgraded mid-session) | ETag mismatches on write | Client receives `412`, refetches, retries; on third mismatch surfaces error to user |
| Share-log corruption (hash chain break) | Entry written with wrong `prev_hash` | Read-side detects on next audit; entry flagged; contributor prompted to repair or archive; AdminAlert emitted |
| Kill-switch activated mid-share | In-flight ShareIntents drop to `cancelled` | Orchestrator aborts unresolved intents; emits `ShareIntentCancelled`; pod writes complete for idempotency; backend flushes cap cache |

## 12. Observability

KPI events are the ONLY content that leaves the pod boundary. Every
event is redacted via `EpisodicRedactor` before emission.

Emitted events (content-free):

| Event | Carriers |
|-------|----------|
| `contributor_activated` | `workspace_id`, `webid_hash`, `tier_used`, `ts` |
| `share_intent_created` | `share_intent_id`, `target_scope`, `subject_kind`, `webid_hash`, `ts` |
| `share_intent_outcome` | `share_intent_id`, `outcome_code`, `target_scope`, `policy_eval_id`, `ts` |
| `automation_run_result` | `routine_id_hash`, `outcome_code`, `tier_used`, `duration_ms`, `ts` |
| `sensei_suggestion_outcome` | `workspace_id`, `suggestion_id`, `outcome_code`, `ts` |
| `broker_case_opened_for_share` | `case_id`, `share_intent_id`, `subject_kind`, `ts` |
| `inbox_item_triage` | `item_id_hash`, `action`, `ts` |
| `skill_distribution_update` | `skill_id`, `version`, `scope`, `ts` |

Redaction rules:

- `webid_hash = HMAC-SHA256(key=workspace_key, message=webid)`.
  `workspace_key` rotates daily; hashes are not cross-session
  correlatable without operator access to the key log.
- `routine_id_hash = HMAC(…, routine_id)` so routines are not
  correlatable across days.
- No `artifact_ref`, no `content_ref`, no pod URI ever leaves the pod
  boundary.
- `outcome_code` is an enum; free-text rationales stay pod-side.

CI test: every KPI emission path is unit-tested to contain no
substring starting `pod:` and no URL whose host matches a configured
pod-provider list.

## 13. Open questions

1. **Type Index team-boundary leakage.** ADR-029 Type Index is
   world-readable; naming `/shared/skills/team-alpha/` in the index
   discloses team names. Mitigation: publish opaque team aliases in the
   Type Index and keep the alias-to-name map pod-local. Confirm with
   broker team.
2. **Cap renewal across devices.** A contributor who activates a cap on
   mobile and a renewal prompt arrives on desktop — which device signs?
   Today first-click-wins; is that acceptable, or do we need a
   device-pinning predicate?
3. **Offline-automation trust model.** If the contributor is offline
   for > 48 h, should routines that produce only inbox items still
   fire, or should a quiet-mode ceiling apply? Current default: fire;
   reconsider after beta.
4. **Pod-provider heterogeneity.** NSS, CSS, Pod Spaces, and self-hosted
   LDP servers have subtly different `acl:default` semantics; we assume
   strict inheritance but some providers propagate lazily. Open issue:
   audit matrix across the three providers we support.
5. **Cross-pod collaborator shares.** A skill authored on Alice's pod
   that lists Bob (on a different pod provider) in `allow_list` — do
   we mirror the artefact to Bob's `/shared/` or leave a pointer?
   Current: pointer-only; revisit after first cross-pod team deploys.
6. **Post-retirement data access.** When a Mesh skill is retracted,
   the pod archive retains the artefact, but the Type Index entry is
   gone — does the auditor role still have read access? Current: yes,
   via the archive path; confirm with compliance.

## 14. References

- ADR-029 Type Index Discovery
- ADR-030 Agent Memory in Solid Pods
- ADR-040 Enterprise Identity Strategy (NIP-07 / NIP-26)
- ADR-041 Judgment Broker Workbench
- ADR-042 Workflow Proposal Object Model
- ADR-045 Policy Engine Approach
- ADR-046 Enterprise UI Architecture
- ADR-048 Dual-Tier Identity Model
- ADR-049 Insight-Migration Broker Workflow
- ADR-052 Pod Default WAC + Public Container
- ADR-057 Contributor Enablement Platform
- PRD-003 Contributor AI Support Stratum (§9, §10)
- Companion design docs:
  - `./00-master.md`
  - `./01-contributor-studio-surface.md`
  - `./02-skill-dojo-and-evals.md`
  - `./04-acceptance-tests.feature`
  - `./evidence-annex.md`
