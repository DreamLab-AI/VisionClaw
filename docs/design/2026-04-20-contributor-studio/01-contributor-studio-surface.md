---
title: Contributor Studio — Surface Specification
description: Multi-pane React surface, routing, command palette, ontology guide rail, AI partner lane, session memory bar. Implements PRD-003 capabilities §7.1-7.5. Extends ADR-046 router.
category: design
tags: [contributor-studio, ui, react, radix, surface, 2026-04-20]
updated-date: 2026-04-20
---

# Contributor Studio — Surface Specification

## 1. Purpose

Contributor Studio is the multi-pane React workspace that realises the Contributor AI Support Stratum (PRD-003) and the BC18 Contributor Enablement bounded context (ADR-057). It sits in the enterprise router between `/graph` (substrate) and `/broker` (management mesh), closing the compounding loop by wrapping everyday contributor work with pod context, ontology guidance, a scoped AI partner, and a skill-lifecycle inbox. Every capability in PRD-003 §7.1–7.5 (Studio shell, Ontology guide rail, AI partner lane, Graph deep-link, Command palette extensions) is surfaced here.

This document specifies only the client-side surface: routes, panes, stores, commands, WebSocket wiring, accessibility, performance budgets. Backend actors, MCP tools, Pod layout, and share-state rules are fixed by ADR-057; this spec never invents new backend APIs. Companion docs 02 (Skill Dojo + evals), 03 (Pod context memory + sharing), 04 (acceptance tests) cover the matching sub-surfaces.

## 2. Layout model

### 2.1 Multi-pane structure

```
+---------------------------------------------------------------------------------------------+
| [Workspace v] [Focus: Ontology/Negentropy · graph#n24]  [Share: Private ●] [Auto: 2]  [⚙] |  <- WorkspaceBar (top)
+------------------+------------------------------------------+-----------------------------+
|                  |                                          |                             |
| Canonical Terms  |  [ Editor | Graph | Preview | Diff ]     |  [Partner: Private AI v]    |
|  · Negentropy    |                                          |  ----- Context panel -----  |
|  · Substrate     |  # Negentropy brief                      |  Focus: page "Negentropy"   |
|  · KPI BC15      |  ...markdown editor...                   |  Skills: summarise, align   |
|                  |                                          |  Policies: rate_limit       |
| Nearby Concepts  |                                          |  ---- Chat transcript ----  |
|  · EnergyFlux    |                                          |  > draft a brief on...      |
|  · Coordination  |                                          |  [Inbox: 3 unread]          |
|                  |                                          |                             |
| Policies         |                                          |                             |
|  · rate_limit    |                                          |                             |
|  · sep_of_duty   |                                          |                             |
|                  |                                          |                             |
| Installed Skills |                                          |                             |
|  · summarise  ✓  |                                          |                             |
|  · align v2.1    |                                          |                             |
+------------------+------------------------------------------+-----------------------------+
| Recent: "Ontology/Negentropy" · "KPI-dashboard-v2" · ShareIntent #17 Private→Team (pending) | <- SessionMemoryBar (bottom)
+---------------------------------------------------------------------------------------------+
 OntologyGuideRail (left, 320px)   WorkLane (center, flex)           AIPartnerLane (right, 380px)
```

Four panes plus two bars. Left: Ontology Guide Rail + installed skills (collapsible). Center: Work Lane with tabbed `Editor | Graph | Preview | Diff`. Right: AI Partner Lane (context panel + chat + inbox chip). Top: Workspace Bar. Bottom: Session Memory Bar. Layout state lives in `studioWorkspaceStore` and is persisted to `/private/contributor-profile/studio-layout.json` (per PRD-003 §7.1).

### 2.2 Pane sizing

| Pane | Default | Min | Max | Resize | Collapse | Persist |
|------|---------|-----|-----|--------|----------|---------|
| WorkspaceBar | 48px | 48px | 48px | fixed | never | — |
| OntologyGuideRail | 320px | 240px | 480px | horizontal drag | to 48px (icon strip) | per workspace |
| WorkLane | flex | 560px | — | driven by neighbours | never | — |
| AIPartnerLane | 380px | 320px | 520px | horizontal drag | to 48px (chip column) | per workspace |
| SessionMemoryBar | 96px | 48px | 200px | vertical drag | to 24px (one-line ribbon) | per workspace |

Rules: at viewport widths <1280px the AI Partner Lane collapses by default; at <960px the Ontology Guide Rail also collapses; at <720px both rails become slide-over sheets (Radix `Dialog` with `side="left"`/`"right"`). Drag handles reuse the `SplitPane` primitive introduced in PRD-003 §7.1; min/max enforced by the primitive.

### 2.3 Pane state

| Pane | Empty | Loading | Error | Radix / DS components |
|------|-------|---------|-------|-----------------------|
| WorkspaceBar | "No workspace — create one" button routing to `/studio/new` | skeleton chip row | inline toast + retry | `Select`, `Tooltip`, `DropdownMenu`, `StatusDot` |
| OntologyGuideRail | "No focus yet — select a node or open an artifact" | three shimmer rows per section | per-section error card with "Retry" | `Collapsible`, `ScrollArea`, `HoverCard`, `Badge` |
| WorkLane / Editor | starter prompt + link to three skills | editor skeleton | inline banner with last-saved marker | `Tabs`, `Toolbar`, markdown editor primitive |
| WorkLane / Graph | "No graph focus" + CTA to palette | spinner overlay over canvas | canvas fallback + "Open full Graph" link | `Tabs`, `WasmSceneEffects` (ADR-047) |
| WorkLane / Preview | "Select an artifact" | skeleton card | inline card with "Reopen" | `Card`, `Separator` |
| WorkLane / Diff | "No ShareIntent active" | skeleton diff | inline banner | `Tabs`, diff viewer primitive |
| AIPartnerLane | "No partner selected" picker | streaming skeleton for response | retry row + "Switch partner" | `Select`, `ScrollArea`, `Popover`, chat primitive |
| Inbox chip | "0 unread" greyed | dot only | red `StatusDot` + count | `Badge`, `Popover` |
| SessionMemoryBar | "No recent activity" | three ghost chips | inline row with "Reload" | `ScrollArea` horizontal, `Chip` |

All pane shells implement `aria-busy` during loading and `role="alert"` on error cards.

## 3. Routing (extends ADR-046)

```
/studio                              Workspace list (index)
/studio/new                          Create workspace wizard
/studio/:workspaceId                 Main studio surface
/studio/:workspaceId/artifacts/:aid  Artifact detail modal (deep-link)
/studio/:workspaceId/skills          Installed skills view
/studio/:workspaceId/skills/dojo     Dojo discovery sub-route
/studio/:workspaceId/sensei          Sensei trace + settings
/studio/automations                  Automation routine list
/studio/automations/new              Create automation wizard
/studio/automations/:id              Routine detail
/studio/inbox                        Headless output review
```

Lazy-loaded with `React.lazy()` per ADR-046. Route module file is `client/src/features/contributor-studio/index.ts`; default export is the `ContributorStudioRoot` route component. Sub-chunks: `/studio/:id/skills/dojo`, `/studio/automations`, `/studio/inbox`, and the artifact modal each in their own `lazy()` boundary to keep the Studio shell <60KB gzipped (PRD-003 §8.1).

The artifact modal is a pathless `<Outlet />` overlay: it renders over `/studio/:workspaceId` rather than replacing it, using Radix `Dialog` + a `useLocation` backdrop pattern. Closing the modal restores the prior route via `navigate(-1)`. Sidebar entry is registered between `Graph` and `Broker`.

## 4. Sidebar entry

Format matches the `registerNavEntry()` schema in ADR-046 §SidebarNav.

| Section | Icon | Route | Badge | Required role | Order |
|---------|------|-------|-------|---------------|-------|
| Graph | `Network` | `/` | — | any | 10 |
| **Studio** | **`LayoutPanelLeft`** | **`/studio`** | **Inbox unread (live)** | **`Contributor`** | **15** |
| Broker | `Scale` | `/broker` | Open case count | broker | 20 |
| Workflows | `GitBranch` | `/workflows` | Pending review count | reviewer | 30 |
| KPIs | `BarChart3` | `/kpi` | — | any | 40 |
| Connectors | `Plug` | `/connectors` | Error count | admin | 50 |
| Policy | `Shield` | `/policy` | — | admin | 60 |

Badge source: `useStudioInboxUnreadCount()` selector over `studioInboxStore`. Badge variant is `"default"` for counts and `"destructive"` if any item has `disposition=="pending"` and `severity=="high"`. Entry is gated behind the `STUDIO_ENABLED` feature flag (ADR-057); hidden when the flag is off even for Contributors.

## 5. Command palette extensions

Registered by `client/src/features/contributor-studio/hooks/useStudioCommands.ts` using the `CommandRegistry` pattern from ADR-046. Style matches PRD-002 §5.4 table.

| Command | Action | Visible when |
|---------|--------|--------------|
| Open Contributor Studio | Navigate `/studio` | always |
| New Workspace | Open `/studio/new` | always |
| Switch Workspace… | Quick-switch modal (recent + pinned) | in `/studio` |
| Share Artifact… | Open `ShareIntent` dialog | artifact selected |
| Run Skill… | Open skill picker | in `/studio` |
| Schedule Automation… | Navigate `/studio/automations/new` | in `/studio` |
| Open Inbox | Navigate `/studio/inbox` | always |
| Accept Nudge | Accept active Sensei suggestion | Sensei visible |
| Dismiss Nudge | Dismiss active Sensei suggestion | Sensei visible |
| Promote to Team | ShareIntent Private→Team | artifact in Private |
| Promote to Mesh | ShareIntent Team→Mesh | artifact in Team |
| Open Sensei Trace | Navigate `/studio/:id/sensei` | in `/studio` |
| Pick from Graph | Enter split-view graph pick mode | in `/studio` |
| Send to Studio | Context-menu action on graph node | on `/graph` |

Commands are registered on mount of `ContributorStudioRoot` and unregistered on unmount. Visibility predicates are functions (`visible(ctx) => boolean`) evaluated on every palette open using the current route + `studioWorkspaceStore` state. Keyboard shortcut conventions: `Cmd+K` opens the palette (unchanged); in-lane shortcuts `g s` (go Studio), `n w` (new workspace), `n a` (new automation) added to the existing two-key chord namespace.

## 6. Ontology Guide Rail (left pane)

### 6.1 Sections

Three vertical `Collapsible` sections, each with its own header count chip and collapse caret. Order and contents:

| Section | Content source | Interaction | Events emitted |
|---------|----------------|-------------|----------------|
| Canonical Terms | OwlClass nodes closest to the current `WorkspaceFocus` per `sensei_nudge`'s ontology-distance metric (ADR-057 §MCP §OntologyGuidance) | click term → pin into Work Lane; hover → `HoverCard` with rationale + source node link | `SuggestionAccepted{type:"term"}`, `SuggestionDismissed{type:"term"}` |
| Nearby Concepts | Graph neighbours 1–2 hops from focus, filtered by visibility (per BC17 policy); thumbnails via scene-effects bridge (§18) | click concept → opens neighbour in Work Lane Graph tab; "Browse graph" opens split-view pick mode | `SuggestionAccepted{type:"concept"}`, `NavigateToGraph` |
| Applicable Policies + Precedents | BC17 rules matching the current focus (category, workspace tags) + cited Broker Workbench cases (ADR-041 precedents) | click policy → opens `/policy/:ruleId` in a Radix `Sheet`; click precedent → opens broker case in sheet | `PolicyReferenced`, `PrecedentReferenced` |

Plus a fourth collapsible sub-panel §6.3 for Installed Skills.

### 6.2 Suggestion composition

Backed by MCP tool `sensei_nudge` (ADR-057). Server returns at most three suggestions per section with `{id, label, rationale, confidence, sourceRef}`. Rendering contract:

- Exactly three rows max per section; pad with empty-state row "No suggestions for this focus".
- Each row: label, confidence badge (Low <0.5 / Med 0.5–0.8 / High >0.8), source-type icon.
- `HoverCard` shows rationale (≤2 sentences) + source ref with a link to open it in-place.
- Actions per row: Accept (primary), Dismiss (muted with reason-picker popover), Edit-and-accept (opens editor with suggestion pre-filled where applicable).
- Accept emits `SuggestionAccepted{workspaceId, sectionId, suggestionId, type, confidence}`; Dismiss emits `SuggestionDismissed{...reason}`. Both are POSTed to `/api/studio/sensei/feedback` (ADR-057) and also fed into the `BC15` KPI bus for the `ontology-guidance-hit-rate` metric.
- Rate limit: rail accepts at most one server push per 3s; coalesces bursts locally; max three rows per section is a server contract, enforced by the client.

### 6.3 Installed skills sub-panel

Compact list. Each skill row: name, version chip (`v2.1`), distribution-scope badge (`Personal` / `Team` / `Mesh`), eval-pass badge (pass-rate %, coloured by threshold). Right-click menu: Run, Inspect SKILL.md (opens sheet), View benchmarks, Retire. Left-click invokes the skill in the AI Partner Lane using the `studio_run_skill` MCP tool (§8.3). Sub-panel reads from `studioWorkspaceStore.installedSkills`, hydrated on workspace open via the ADR-057 `GET /api/studio/workspaces/:id/context` response.

## 7. Work Lane (center pane)

### 7.1 Tabs

Four tabs via Radix `Tabs`, persisted per workspace:

| Tab | Purpose | Shown when |
|-----|---------|------------|
| Editor | Markdown draft of current `WorkArtifact` | always (default) |
| Graph | Embedded 3D canvas of focus + neighbours | always |
| Preview | Rendered `WorkArtifact` with lineage + share state | artifact selected |
| Diff | Unified + side-by-side diff between Private and Team/Mesh copies | active `ShareIntent` |

Tab switching never unmounts: hidden tabs keep their DOM so the editor retains scroll + selection and the embedded graph keeps its scene alive (throttled to 15Hz when hidden, §18).

### 7.2 Editor

Markdown editing with ontology-aware autocomplete: `[[wikilinks]]` resolve against pod KG + cached OwlClass index; typing `::` triggers a property completer using ontology vocab. Writes go to `/private/kg/{artifact-slug}.md` via the existing Pod write path (no new API). Autosave debounced 1.5s, on save emits `WorkArtifactEdited` to the WebSocket channel so the Sensei can refresh. Supported block types mirror Logseq conventions already in the graph (pages, journal blocks, OntologyBlock). The editor primitive is shared with the existing knowledge-graph edit surface; only the wrapper + store integration is Studio-specific.

### 7.3 Graph-view embed

A subset of the `/graph` canvas sized to the center pane. Reuses `WasmSceneEffects` (ADR-047) with a reduced tick budget when the Graph tab is not visible. Camera state is workspace-scoped; deep-linking from the Ontology Guide Rail "Nearby Concepts" sets camera target + focus node via `setFocus()` on the canvas imperative handle. Clicking a node in the embed updates `studioWorkspaceStore.focus` and emits a `context_updated` event the server must ack. The embed never mutates graph edges — all writes go through the existing graph services.

### 7.4 Artifact preview

Renders `WorkArtifact` content read-only with a header showing: lineage (parent artifact + originating skill, if any), current share state (coloured chip, same palette as §11), `ShareIntent` affordances (Promote to Team / Promote to Mesh buttons gated on §3 rules from ADR-057). "Open full" routes to `/studio/:workspaceId/artifacts/:aid`. Lineage metadata is sourced from the context payload; no extra fetch.

## 8. AI Partner Lane (right pane)

### 8.1 Partner selection

Top-of-lane Radix `Select` dropdown with four categories:

| Category | Default | Source |
|----------|---------|--------|
| Private AI | Tier 2 Haiku | ADR-026 tier router via `partner_route` call inside `PartnerOrchestrationActor` |
| Team AI | team-scoped binding from `/private/contributor-profile/team-bindings.jsonld` | contributor profile |
| Human Collaborator | WebID search via Type Index (ADR-029) | peer pod discovery |
| Scheduled Automation | handle to an existing `/private/automations/*.json` | automation registry |

Selection persisted per workspace in `studioWorkspaceStore.partnerSelection`. Changing partner clears the in-memory transcript but keeps the `GuidanceSession` record (episodic memory preserved per ADR-030).

### 8.2 Chat surface

An injected context panel at the top of the lane is always visible and shows the assembled `WorkspaceFocus`: current focus label, installed skills in scope, active policies. Clicking any chip pins the underlying artifact into the Work Lane. Messages render below the panel in a virtualised `ScrollArea` (Radix). Tool calls render as inline chips (name, duration, status dot); clicking a chip opens a `Sheet` with the full tool I/O.

Transport: the existing WebSocket bridge on `/api/ws/studio` (ADR-057) streams `partner_message` frames. The lane never posts to the generic chat API — every message is scoped to the current `GuidanceSession` and assembled server-side by `ContextAssemblyActor`. Client responsibility is purely rendering + user intent capture.

### 8.3 Skill invocation UX

Three entrypoints, all converging on the `studio_run_skill` MCP tool:

1. Slash-command in the chat composer: `/skill summarise` fuzzy-matches `studioWorkspaceStore.installedSkills` and shows a completion popover.
2. Command palette "Run Skill…" opens a modal picker with the same completion.
3. Ontology Guide Rail "Run suggested skill" button on any suggestion whose `type="skill"`.

All three open the same parameter form, generated from the skill's SKILL.md frontmatter (fields: name, type, required, default). Submit calls `studio_run_skill`; the response streams into the chat as if it were a partner message. Tool calls are persisted in the `GuidanceSession`; failures surface as inline chat rows with "Retry".

### 8.4 Inbox integration

An Inbox chip at the top-right of the AI Partner Lane shows the unread count from `studioInboxStore`. Click opens `/studio/inbox`. New inbox items also briefly flash the sidebar badge and emit a gentle non-modal toast from the Workspace Bar (auto-dismiss 4s).

## 9. Graph deep-linking

Two flows, both leaving the Studio surface responsible for rebuilding focus server-side via `context_updated`.

**Flow A — Send to Studio.** Right-click a node on `/graph` → context menu → "Send to Studio". The command invokes `studio_context_assemble(workspace_id, current_focus={nodeRef})` and navigates to `/studio/:id`; the selected node plus current ontology context becomes the new workspace focus. When no Studio workspace exists, the command routes to `/studio/new` with `initialFocus=nodeRef` as a query param.

**Flow B — Pick from Graph.** Command palette "Pick from Graph" or Ontology Guide Rail "Browse graph" opens a split-view: `/graph` on the right (`Resizable` panel 50/50), Studio shell on the left. Clicking a node returns focus to Studio and closes the split. Split state is workspace-scoped and not persisted (exit is explicit).

Both flows update `studioWorkspaceStore.focus`, which triggers a single `context_updated` publish and a `sensei_nudge` recompute. Round-trip target: <100ms local update, <300ms server refresh (PRD-003 §7.4).

## 10. Session Memory Bar (bottom pane)

A horizontal `ScrollArea` rail showing three rotating item classes, read-only:

| Item class | Source | Tooltip | Click action |
|------------|--------|---------|--------------|
| Recent episodic highlight | `/private/agent-memory/episodic/studio-partner/` (ADR-030) | 1-line synthesis | pins source memory into Work Lane |
| Recent artifact | `studioInboxStore.recentArtifacts` (computed over last 10 edits) | share state, last edit time | opens artifact preview |
| Open ShareIntent | `studioShareStore` | target scope, status (Pending/Approved/Rejected) | opens Diff tab in Work Lane |

Items are chips (icon + short label + status dot). The bar is read-only; writes go through the standard domain services (no new endpoints). The bar is a single-line ribbon (48px) when collapsed, a 96px strip when expanded. A chip can be pinned; pinned chips move to the left and persist across sessions in `/private/contributor-profile/studio-layout.json`.

## 11. Workspace Bar (top pane)

Layout (left → right): Workspace selector, Focus pill, Share-state chip, Automation status indicator, Settings gear.

| Element | Behaviour |
|---------|-----------|
| Workspace selector | Radix `Select`; last-used first, "Create new" at the bottom; shows workspace name + pod-context chip |
| Focus pill | current project slug + graph snapshot badge (timestamp of last `context_updated`); click opens focus trace sheet |
| Share-state chip | colour-coded per ADR-057 §Share-State Transition: `Private` (slate), `Team` (cyan), `Mesh-candidate` (amber), `Mesh` (emerald), `Retired` (muted); WAC state in the tooltip |
| Automation status indicator | states `idle` / `running` / `errored` / `inbox-unread`; dot + count; click routes to `/studio/automations` |
| Settings gear | opens workspace settings sheet: layout, partner default, Sensei opt-outs, context budgets |

Focus pill and share-state chip share the `StatusDot` primitive with variants extended to the share-state palette. Colour contrast satisfies WCAG 2.2 AA against the `#000022` background.

## 12. First-run experience

Step-by-step Ramp Glass-parity onboarding, executed by `FirstRunCoordinator` within `ContributorStudioRoot`:

1. **Pod provisioning check** — GET `/api/pods/status`; if absent, silently run ADR-052 provisioning flow. No user interaction unless the flow errors.
2. **ContributorProfile wizard** — three-screen wizard: role (defaults from directory), goals (skippable), collaborators (auto-suggests from Type Index). Persists to `/private/contributor-profile/`. Entire wizard is skippable with sensible defaults keyed by directory role.
3. **Default workspace "My First Workspace"** — auto-created with focus set to the user's most-recently-edited KG page, or to the directory home page if no edits.
4. **Three starter skills installed** — `research-brief`, `ontology-align`, `summarise` via `skill_install` MCP calls, pinned to latest published version.
5. **Three Sensei nudges pre-populated** — one term, one concept, one policy; composed from the contributor's profile + starter workspace focus.

Target: user is productive within 90 seconds of first login. Steps 1–5 run in parallel where possible; failures degrade gracefully (a missing starter skill does not block step 3).

## 13. Accessibility (WCAG 2.2 AA)

| Dimension | Requirement |
|-----------|-------------|
| Keyboard nav | All four panes fully operable without a pointer. Tab order: Workspace Bar → Guide Rail sections → Work Lane tabs → editor/graph/preview → AI Partner Lane → Session Memory Bar. Pane collapse/expand toggled with `Alt+[`, `Alt+]`. |
| ARIA roles | Guide Rail sections `role="region" aria-labelledby`; chat transcript `role="log" aria-live="polite"`; Sensei nudge arrival `aria-live="assertive"` with deduplication to avoid spam; inbox chip `aria-describedby` the count. |
| Focus management | Route change focuses the Work Lane heading; modal/sheet close restores focus to the trigger; palette open/close restores focus to prior element. |
| Screen-reader announcements | Sensei nudge arrival, inbox new-item arrival, share-state transitions all announced with short, deduped messages. |
| Reduced motion | Respects `prefers-reduced-motion`; disables lane slide animations; replaces pulse dots with static indicators. |
| Contrast | All text ≥4.5:1, icons + StatusDots ≥3:1 against `#000022` and pane surfaces. |
| Target size | Interactive targets ≥24×24 per WCAG 2.2 2.5.8 (palette rows, nudge row actions). |

## 14. State management (Zustand)

| Store | File | Shape (essentials) |
|-------|------|--------------------|
| `studioWorkspaceStore` | `features/contributor-studio/store/workspaceStore.ts` | `{workspaces, activeId, focus, layout, installedSkills, partnerSelection, fetchContext(), updateFocus()}` |
| `studioContextStore` | `.../contextStore.ts` | `{byWorkspaceId:{ [id]:WorkspaceFocus }, loading, error, invalidate()}` |
| `studioPartnerStore` | `.../partnerStore.ts` | `{transcriptsByWorkspaceId, activePartner, sendMessage(), cancelStream()}` |
| `senseiStore` | `.../senseiStore.ts` | `{nudgesByWorkspaceId:{terms,concepts,policies}, trace, accept(), dismiss()}` |
| `studioInboxStore` | `.../inboxStore.ts` | `{items, unreadCount, recentArtifacts, markRead(), ack()}` |
| `studioShareStore` | `.../shareStore.ts` | `{intents, byArtifactId, createIntent(), cancelIntent()}` |

All stores use Zustand + Immer `produce()` per PRD-002 §4. No cross-store imports; derived UI state comes from cross-store selectors declared in `features/contributor-studio/hooks/selectors.ts` (e.g. `useWorkspaceFocusWithNudges()` composes `studioWorkspaceStore` + `senseiStore`). Optimistic updates on `acceptNudge`, `ack`, `createIntent` with rollback on server failure. No persistence to `localStorage` — server is authoritative; layout persistence is done through the pod write path.

## 15. Real-time channels

WebSocket endpoint `/api/ws/studio` (ADR-057). Client subscribes on mount to channels keyed by the active workspace:

```
studio:workspace:{id}            context_updated, share_intent_updated
studio:sensei:{workspace_id}     sensei_nudge
studio:inbox:{pubkey}            inbox_new
skills:registry                  skill_eval_completed, skill_published, skill_retired
studio:partner:{session_id}      partner_message
```

| Payload | Target store | Effect |
|---------|--------------|--------|
| `context_updated` | `studioContextStore` | replaces focus + triggers Sensei refresh |
| `sensei_nudge` | `senseiStore` | appends/replaces suggestion stack |
| `inbox_new` | `studioInboxStore` | increments `unreadCount`, flashes sidebar badge |
| `share_intent_updated` | `studioShareStore` | updates intent status; may surface a toast |
| `skill_eval_completed` | `studioWorkspaceStore.installedSkills` | refreshes eval-pass badge |
| `partner_message` | `studioPartnerStore` | streams into active transcript |

All subscriptions follow the PRD-002 §12.3 ref-counted pattern. On workspace change, the prior `studio:workspace:{id}` + `studio:sensei:{id}` subscriptions are unsubscribed before the new ones are established to guarantee ordering.

## 16. Performance budgets

| Operation | Budget | Measurement |
|-----------|--------|-------------|
| Studio shell initial paint | <2s | Lighthouse LCP on /studio cold load |
| Context assembly (cache hit) | <500ms | `GET /api/studio/workspaces/:id/context` p95 |
| Context assembly (cold) | <1500ms | same endpoint p95 after workspace invalidate |
| Sensei nudge composition | <1s | `sensei_nudge` Tier 2 RTT; degraded to Tier 3 only on explicit opt-in |
| Skill install optimistic UI | <100ms local, <2s Pod write ack | `studio_run_skill` install-path |
| Real-time message E2E | <250ms | WebSocket frame arrival after server publish |
| Graph embed tick | 60Hz visible / 15Hz hidden | WasmSceneEffects FPS counter |
| Tab switch | <16ms render cost | React Profiler on `WorkLaneTabs` |
| Palette open | <100ms first paint | measured from Cmd+K keydown |

Budgets are enforced by `client/tests/performance/studio-budgets.spec.ts` (Playwright + web-vitals) and regress CI fails on any p95 breach.

## 17. Component tree (summary)

```
ContributorStudioRoot
├── WorkspaceBar
├── PaneLayout
│   ├── OntologyGuideRail
│   │   ├── CanonicalTerms
│   │   ├── NearbyConcepts
│   │   ├── ApplicablePolicies
│   │   └── InstalledSkillsList
│   ├── WorkLane
│   │   ├── WorkLaneTabs (Editor | Graph | Preview | Diff)
│   │   ├── MarkdownEditor
│   │   ├── EmbeddedGraphView
│   │   └── ArtifactPreview
│   └── AIPartnerLane
│       ├── PartnerSelector
│       ├── ContextPanel
│       ├── ChatSurface
│       └── InboxChip
└── SessionMemoryBar
```

Component files under `client/src/features/contributor-studio/`. Shared primitives (`SplitPane`, `StatusDot`, `Chip`, `HoverCard` wrappers) live in `features/design-system/` and are added incrementally per PRD-002 §15. File size limit per project rules: 500 lines per file. Any component exceeding the limit must be split along a natural seam (e.g. `ChatSurface` splits into `ChatTranscript` + `ChatComposer`).

## 18. Integration with scene-effects-bridge (ADR-047)

Two integration points reuse the existing zero-copy Float32Array WASM pattern from the scene-effects bridge rather than introducing any new canvas primitive.

1. **Ontology Guide Rail "Nearby Concepts" thumbnails.** Each suggestion row may carry a 64×64 snapshot of the concept's local subgraph. Thumbnails are rendered by a shared offscreen `WasmSceneEffects` instance, one render pass per visible row, with results blitted into a canvas per row. The zero-copy pattern (`get_*_ptr()`, `get_*_len()`, `Float32Array` view over `WebAssembly.Memory.buffer`) keeps allocations flat.
2. **EmbeddedGraphView.** Reuses the full `WasmSceneEffects` pipeline at a reduced tick rate (15Hz when the Graph tab is hidden, 60Hz when visible). Tick rate is controlled by the existing `setTickRate()` imperative handle. This mirrors the `GraphCanvas` + `GraphViewport` dual-consumer pattern already in the codebase.

No new WASM exports are introduced. If a new export is proposed later, it is added to ADR-047, not here.

## 19. Model routing (ADR-026 tiers)

| Studio operation | Tier | Reason |
|------------------|------|--------|
| Chat message drafting | Tier 2 Haiku | fast, cheap, good enough for contributor-facing drafts |
| Skill run | per-skill `min_model_tier` from SKILL.md frontmatter | author-declared floor |
| Sensei nudge composition | Tier 2 Haiku | latency-sensitive; Tier 3 only on explicit "Deep nudge" opt-in |
| Ontology query (direct) | Tier 1 | no LLM; direct MCP call against the ontology index |
| Share-intent rationale generation | Tier 3 Sonnet | higher stakes, policy + broker-facing justification |
| Context assembly | Tier 1 | server-side compose only; no LLM in the hot path |
| Skill-eval orchestration | per-eval `model_tier` | author-declared; defaults Tier 2 |
| Automation execution (headless) | per-automation `budget.model_tier` | contributor-controlled budget cap |

All routing calls go through `PartnerOrchestrationActor` and `AutomationOrchestratorActor` (ADR-057). The client never selects a model tier directly; it selects a partner or skill, and the server picks the tier from the declared floor + current budget.

## 20. Open questions

1. **Focus-trace retention.** How long do we keep the `WorkspaceFocus` history per workspace in `studioContextStore`? Default proposal: last 50 focus snapshots per workspace, client-side only; contested because episodic memory (ADR-030) may want the full series.
2. **Team-partner bindings UI.** Team AI partners are scoped via `/private/contributor-profile/team-bindings.jsonld`; there is no authoring UI for those bindings in this surface. Does authoring belong in Studio settings or in a separate admin surface?
3. **Multi-workspace concurrency.** Should a contributor be able to open two workspaces in two tabs? Today the WebSocket subscription model assumes a single active workspace per pubkey; a multi-tab user would generate duplicate `context_updated` churn.
4. **Graph embed fidelity.** What is the correct downsample policy when the embed is hidden or backgrounded beyond 15Hz — keep rendering invisibly, pause entirely, or degrade to a static snapshot? Each has different implications for returning to the tab.
5. **Nudge dismissal reason taxonomy.** The dismiss reason popover currently offers `not-relevant / already-knew / wrong-scope / other`; this taxonomy is ad-hoc. The KPI `ontology-guidance-hit-rate` would benefit from a more structured set negotiated with BC15.
6. **Session Memory Bar ranking.** When the bar has more than ~12 items across the three classes, how do we rank? Recency, pinned-first is the MVP, but cross-class weighting is open.
7. **Artifact modal history.** Deep-linking to `/studio/:id/artifacts/:aid` from external tools (Broker case, KPI drilldown) must decide whether to push onto the Studio history stack or replace it. Current bias is push; revisit when the Broker workbench deep-link lands.
