# PRD-002: Enterprise Control Plane UI

**Status**: Proposed
**Author**: Architecture Agent
**Date**: 2026-04-14
**Priority**: P1 -- Required for enterprise pilot readiness
**Depends On**: ADR-040 (Enterprise Identity), ADR-041 (Broker Workbench), ADR-042 (Workflow Proposals), ADR-043 (KPI Lineage), ADR-044 (Connector Governance), ADR-045 (Policy Engine)
**Companion ADRs**: ADR-046 (Enterprise UI Architecture), ADR-047 (WASM Visualization Components)

---

## 1. Problem Statement

VisionClaw has five backend bounded contexts for enterprise governance (BC11 Broker Workbench, Workflow Proposals, KPI Lineage, Connector Governance, Policy Engine) with live REST and WebSocket endpoints. The client has no UI surfaces for any of them. Enterprise users -- Judgment Brokers, Transformation Leaders, Auditors -- cannot use the platform for its intended purpose without these surfaces.

The existing client is a full-screen 3D graph visualisation with a side-panel control system (`DashboardControlPanel`, `PerformanceControlPanel`, `AgentControlPanel`). That architecture works for graph exploration but does not accommodate data-dense enterprise workflows: filtered lists, case detail views, decision forms, KPI dashboards, configuration wizards.

This PRD specifies five enterprise control plane surfaces that consume the existing backend APIs and extend the existing design system (Radix UI v3 + Tailwind CSS v4 + Framer Motion) without replacing or degrading the graph visualisation experience.

---

## 2. Goals

- Ship five enterprise surfaces: Broker Workbench, Workflow Studio, Mesh KPI Dashboard, Connector Management, Policy Console
- Extend, not replace, the existing design system and navigation model
- Maintain the VisionClaw dark-theme aesthetic (cosmic/crystalline/bioluminescent)
- Support real-time updates for time-sensitive surfaces (Broker Inbox, KPI values)
- Desktop-first, information-dense layouts suitable for enterprise power users
- Keyboard-navigable with command palette integration
- Progressive WASM enhancement for high-performance visualisation components

## 3. Non-Goals

- Replacing the 3D graph visualisation or its existing control panels
- Building a mobile-native experience (responsive tablet support is in scope; phone is not)
- Implementing backend API changes (all endpoints referenced here are already live or defined in ADR-041 through ADR-045)
- Building real-time collaboration features (multi-broker concurrent editing)
- Implementing the automated Discovery Engine or Insight Ingestion Loop backend logic

---

## 4. Technology Context

| Layer | Technology | Version | Notes |
|-------|-----------|---------|-------|
| Framework | React | 19 | Concurrent features available |
| Build | Vite | latest | Module federation not needed; single SPA |
| Language | TypeScript | strict mode | Generated types from Rust via `specta` |
| Design System | Radix UI | v3 (`@radix-ui/themes` 3.2.1) | 25+ components in `client/src/features/design-system/components/` |
| Styling | Tailwind CSS | v4.1.7 | HSL custom property tokens |
| State | Zustand + Immer | 5.x / 11.1.3 | Existing pattern: `create()` with `produce()` |
| 3D | React Three Fiber + Three.js | 9.5.0 / 0.183.0 | Embedded mini-graphs in Decision Canvas |
| Animation | Framer Motion | 12.23.26 | Existing presets in `design-system/animations.ts` |
| Icons | Lucide React | 0.562.0 | Consistent with existing panels |
| WASM | wasm-pack + scene-effects bridge | Custom | Zero-copy Float32Array pattern established |

---

## 5. Navigation Architecture

### 5.1 Current State

The client is a single-route application. `MainLayout.tsx` renders a full-screen `GraphCanvasWrapper` with an `IntegratedControlPanel` overlay. There is no router. Navigation between control panels happens via tabs within the side panel.

### 5.2 Target State

Introduce `react-router-dom` v7 with a `RootLayout` that wraps a persistent `SidebarNav` component alongside an `<Outlet />`. The current graph view becomes the default index route. Enterprise surfaces are peer top-level routes, each lazy-loaded via `React.lazy()`. The 3D canvas does not render on enterprise routes (it is computationally expensive and irrelevant to tabular enterprise workflows).

See ADR-046 for the full router structure, `RootLayout` component, and migration path.

```
/                       -> Graph Visualisation (current MainLayout)
/broker                 -> Broker Workbench
/broker/:caseId         -> Decision Canvas (full case detail)
/workflows              -> Workflow Studio
/workflows/:proposalId  -> Proposal Detail
/kpi                    -> Mesh KPI Dashboard
/kpi/:metric            -> KPI Drill-down
/connectors             -> Connector Management
/connectors/new         -> Connector Configuration Wizard (new)
/connectors/:id/config  -> Connector Configuration Wizard (edit)
/policy                 -> Policy Console
/policy/:ruleId         -> Rule Editor
```

### 5.3 Left Sidebar (`SidebarNav`)

A persistent left sidebar replaces the current implicit navigation. Collapsed to 56px (icon-only) by default, expandable to 200px (icon + label) on hover or pin toggle.

| Section | Icon | Route | Badge |
|---------|------|-------|-------|
| Graph | `Network` | `/` | -- |
| Broker | `Scale` | `/broker` | Open case count (live) |
| Workflows | `GitBranch` | `/workflows` | Pending review count |
| KPIs | `BarChart3` | `/kpi` | -- |
| Connectors | `Plug` | `/connectors` | Error count (if any) |
| Policy | `Shield` | `/policy` | -- |
| Settings | `Settings` | (existing panel) | -- |

Design details:
- Fixed position, full viewport height
- Active route indicated by `--accent` HSL highlight on icon
- Badge values driven by lightweight Zustand selectors subscribed to respective stores
- Badges use existing `Badge` component: `variant="destructive"` for errors, `variant="default"` for counts
- Keyboard accessible: `Tab` to focus, `Enter` to navigate, `ArrowUp/Down` between items
- Collapse/expand toggle at bottom (pinned state stored in `localStorage`)
- Registration pattern: each feature module calls `registerNavEntry()` at import time (see ADR-046)

### 5.4 Command Palette Integration

The existing command palette (`Cmd+K`) gains enterprise commands registered by each feature module via the `CommandRegistry` pattern from `client/src/features/command-palette/types.ts`:

| Command | Action |
|---------|--------|
| `Go to Broker Inbox` | Navigate to `/broker` |
| `Go to Workflow Studio` | Navigate to `/workflows` |
| `Go to KPI Dashboard` | Navigate to `/kpi` |
| `Go to Connectors` | Navigate to `/connectors` |
| `Go to Policy Console` | Navigate to `/policy` |
| `Open Case {id}` | Navigate to `/broker/{id}` |
| `Open Proposal {id}` | Navigate to `/workflows/{id}` |
| `Search Cases...` | Open broker search |
| `Search Workflows...` | Open workflow search |

Commands are registered on mount and unregistered on unmount by each feature module's command hook (see ADR-046 for the registration pattern).

---

## 6. Surface 1: Broker Workbench

**Route**: `/broker`
**Backend**: ADR-041 endpoints (`/api/broker/inbox`, `/api/broker/cases/{id}`, etc.)
**Role**: Broker (ADR-040)

### 6.1 Broker Inbox

The primary landing view. A filterable, sortable, keyboard-navigable list of open broker cases.

**Layout**: Full-width `DataTable` (design system component) with the following columns:

| Column | Source Field | Width | Sortable | Filterable |
|--------|-------------|-------|----------|------------|
| Priority | `priority` (0-100) | 64px | Yes (default: desc) | Yes (range slider) |
| Status | `status` | 96px | Yes | Yes (multi-select: open, under_review, delegated) |
| Category | `category` | 120px | Yes | Yes (multi-select: escalation, workflow_review, trust_alert, policy_exception, manual) |
| Title | `title` | flex | No | Yes (text search) |
| Source | `source_event_id` + type icon | 48px | No | Yes (by source type) |
| Age | computed from `created_at` | 96px | Yes | Yes (range: <1h, <24h, <7d, >7d) |
| Assigned | `assigned_broker` display name | 120px | Yes | Yes (assigned/unassigned) |

**Interactions**:
- Click row -> navigate to `/broker/{id}` (Decision Canvas)
- `Enter` on focused row -> same
- `c` key on focused row -> claim case (POST `/api/broker/cases/{id}/claim`)
- `r` key on focused row -> release case (POST `/api/broker/cases/{id}/release`)
- Filter controls in a collapsible `FilterBar` header bar
- Sort by clicking column headers (tri-state: asc, desc, none)
- Pagination: 50 items per page, cursor-based (backend `?limit=50&cursor=...`)

**Real-time updates**: WebSocket subscription to `broker:new_case`, `broker:case_claimed`, `broker:case_updated`, `broker:priority_changed`. New cases appear at the top with a Framer Motion `variants.slideDown` animation (stagger: 10ms per row, max 20). Claimed cases update in-place with a brief highlight flash using `variants.scale` + `transitions.spring.snappy`.

**Priority badges** (`PriorityBadge` component): Colour-coded using HSL tokens:
- Critical (80-100): `--destructive` (red) with `box-shadow: 0 0 8px hsl(var(--destructive) / 0.3)` glow
- High (60-79): `--warning` (amber)
- Medium (30-59): `--accent` (blue)
- Low (0-29): `--muted` (grey)

**Empty state**: `EmptyState` component with text: "No open cases. The mesh is running smoothly." with a CTA button for manual submission (links to Case Submit Form).

### 6.2 Case Submit Form

**Access**: "Submit Case" button in Broker Inbox header, or empty state CTA

A `Dialog` overlay (existing design system) for manual workflow submission that solves the cold-start problem (no automated escalations exist yet).

| Field | Component | Validation |
|-------|-----------|------------|
| Title | `Input` | Required, 5-200 chars |
| Description | `Textarea` with markdown preview toggle | Required, 20+ chars |
| Category | `Select` (manual pre-selected) | Required |
| Priority | `Slider` (0-100, default 50) | Required |
| Related Entities | Multi-select search (queries graph nodes) | Optional |

Submit posts to `POST /api/broker/submit`. On success, toast confirmation and the new case appears in the inbox via WebSocket push. On validation error, inline field errors.

### 6.3 Decision Canvas

**Route**: `/broker/:caseId`

A full-screen case detail view optimised for broker decision-making. Three-column layout:

```
+-------------------+---------------------------+-------------------+
|                   |                           |                   |
|   Evidence Panel  |   Decision Centre         |   Graph Context   |
|   (320px fixed)   |   (flex, min 480px)       |   (360px fixed)   |
|                   |                           |                   |
+-------------------+---------------------------+-------------------+
```

**Evidence Panel (left)**:
- Provenance tree: collapsible tree view showing the bead lifecycle trail (ADR-034)
- Policy evaluations: list of `PolicyEvaluated` results for this case, rendered with `StatusDot` (success=Allow, warning=Warn, error=Deny, info=Escalate)
- Similar past decisions: cards showing 3-5 semantically similar previous broker decisions with outcomes
- Agent activity: `Timeline` component showing relevant agent actions from BC8
- Each evidence section is a `Collapsible` component (existing design system)

**Decision Centre (middle)**:
- Case header: title, category badge, priority badge (`PriorityBadge`), `StatusDot`, age, assigned broker
- Summary: full case description with markdown rendering (`MarkdownRenderer` from design system patterns)
- Suggested decision: hidden behind a "Show AI Suggestion" button to prevent automation bias (ADR-041). Displayed with a confidence percentage and reasoning. Visual treatment: dashed border, muted background, `text-muted-foreground` to distinguish from human-authored content.
- Decision actions (see 6.4 below)
- Decision history: if the case was previously claimed/released/delegated, show that trail using the `Timeline` component

**Graph Context (right)**:
- Embedded React Three Fiber mini-graph showing the subgraph of nodes related to this case
- Uses the existing `GraphCanvasWrapper` component with a constrained viewport (no full-screen controls, disabled post-processing for performance)
- Node selection in the mini-graph populates a tooltip with node metadata
- Click-through: navigates to `/?focus={nodeId}` for full graph view with selected node highlighted
- Fallback: if no graph context (manual submissions), `EmptyState` with "No graph context available"

### 6.4 Decision Actions

A sticky action bar at the bottom of the Decision Centre, using existing `Button` variants:

| Button | Variant | Keyboard | Requires |
|--------|---------|----------|----------|
| Approve | `default` (green accent) | `Ctrl+Enter` | Reasoning `Textarea` (optional) |
| Reject | `destructive` | `Ctrl+Shift+R` | Reasoning `Textarea` (required, min 10 chars) |
| Amend | `outline` | `Ctrl+Shift+A` | Amendments JSON editor + reasoning |
| Delegate | `outline` | `Ctrl+Shift+D` | Delegate-to selector + reason |
| Promote as Workflow | `secondary` | `Ctrl+Shift+W` | Reasoning |
| Mark as Precedent | `secondary` | `Ctrl+Shift+P` | Scope selector + reasoning |

All actions POST to `/api/broker/cases/{id}/decide` with the appropriate action payload. On success, navigate back to `/broker` with a toast confirmation. On 409 (case already decided), error toast and refresh case state.

The reasoning `Textarea` has a markdown preview toggle. Reject actions enforce minimum 10 characters.

### 6.5 Broker Timeline

**Route**: `/broker?tab=timeline` (tab within Broker Workbench, selected via `Tabs` component)

A chronological `Timeline` of the current broker's past decisions. Each entry shows:
- Decision date, case title, action taken (colour-coded dot), reasoning excerpt
- Outcome tracking: if the decision led to a deployed workflow, show deployment status
- Click entry -> navigate to `/broker/{caseId}` for full case detail

Filterable by action type (`Select`), date range, and case category (`Select`). Uses the `DataTable` component with custom cell renderers for the timeline aesthetic.

---

## 7. Surface 2: Workflow Studio

**Route**: `/workflows`
**Backend**: ADR-042 endpoints (`/api/workflows/proposals`, `/api/workflows/proposals/{id}`, etc.)
**Role**: Broker, Contributor (ADR-040)

### 7.1 Proposal List

A card grid (3 columns on desktop, 2 on tablet, CSS Grid `auto-fill minmax(320px, 1fr)`) of workflow proposals. Each card uses the existing `Card` component with hover elevation via Framer Motion `whileHover={{ y: -2 }}`.

Card contents:
- **Title**: `CardTitle`
- **Status badge**: `Badge` colour-coded by lifecycle stage
  - Draft: grey (`variant="secondary"`)
  - Submitted: blue (`variant="default"`)
  - Under Review: amber (`variant="outline"` with warning colour)
  - Approved: green (`variant="default"` with success colour)
  - Deployed: green with checkmark icon
  - Archived: muted with strikethrough text
- **Source indicator**: discovery (robot icon from Lucide) or manual (user icon)
- **Risk score**: `Progress` component (0-1.0), coloured red above 0.7, amber above 0.4, green below
- **Owner**: avatar placeholder + display name
- **Last updated**: relative time (e.g., "3h ago")
- **Version count**: `Badge` with version number

**Filters**: Status multi-select (`Select`), source type, owner, risk score range (`Slider`). Filter state persisted in URL query params (`?status=draft,submitted&source=manual`).

**Actions**:
- Click card -> navigate to `/workflows/{proposalId}`
- "New Proposal" button -> opens creation form

### 7.2 Create Proposal Form

A `Dialog` overlay with the following fields:

| Field | Component | Validation |
|-------|-----------|------------|
| Title | `Input` | Required, 5-200 chars |
| Description | `Textarea` with markdown preview | Required, 20+ chars |
| Source Type | `RadioGroup` (discovery / manual) | Required |
| Initial Step | Step type `Select` + description `Input` | Required (at least 1) |

Posts to `POST /api/workflows/proposals`. On success, navigate to `/workflows/{newId}`.

### 7.3 Proposal Detail

**Route**: `/workflows/:proposalId`

Two-column layout:

```
+---------------------------+-------------------+
|                           |                   |
|   Step Editor             |   Version History |
|   (flex, min 560px)       |   (320px fixed)   |
|                           |                   |
+---------------------------+-------------------+
```

**Step Editor (left)**:
- Visual representation of workflow steps as a vertical DAG
- Each step rendered as a `Card` showing: type icon (Lucide), description, inputs/outputs, timeout/SLA
- Steps connected by directional lines (SVG paths; WASM-accelerated DAG layout as progressive enhancement per ADR-047)
- Conditional steps show branching paths with diverge/converge visual treatment
- **Edit mode** (draft status): drag to reorder, click to edit step details in a slide-over panel, "Add Step" button
- **Read-only mode** (submitted/under_review): steps are not editable but inspectable (click to view detail)

**Version History (right)**:
- `Timeline` component listing `WorkflowVersion` entries with version number, author, date, change summary
- Click version -> highlight diff against previous version in the step editor
- Diff view: added steps (green border, `border-emerald-500`), removed steps (red border with strikethrough, `border-red-500 opacity-50`), modified steps (amber border with field-level diff tooltip, `border-amber-500`)
- Diff computation calls the application-layer diff endpoint (ADR-042)

**Promotion Flow indicator** -- horizontal pipeline at the top of the detail view:

```
Draft -> Submitted -> Under Review -> Approved -> Deployed
  [o]       [o]          [*]           [ ]         [ ]
```

Each stage is a `StepIndicator` component (design system). Completed stages are filled. Current stage pulses via Framer Motion `animate={{ scale: [1, 1.1, 1] }}, transition: { repeat: Infinity, duration: 2 }`. Clicking a completed stage shows a `Tooltip` with actor and timestamp.

**Action bar** (sticky bottom, visible per role/status):
- Draft: "Submit for Review" `Button`
- Under Review (broker only): Approve / Reject / Request Amendments buttons (mirrors broker decision actions)
- Approved (admin only): "Deploy" `Button` with scope `Select` (team / department / org-wide)
- Deployed: "Rollback" `Button` with confirmation `Dialog`

### 7.4 Pattern Library

**Route**: `/workflows?tab=patterns`

A catalogue of deployed `WorkflowPattern` entities. Card grid (same layout as 7.1). Each pattern card shows:
- Title, deployment scope badge, deployed date
- Usage count (adoption metric)
- Active version number
- "View" button -> navigates to the proposal detail for the active version

Sortable by usage count (most-adopted first), deployment date, and scope.

---

## 8. Surface 3: Mesh KPI Dashboard

**Route**: `/kpi`
**Backend**: ADR-043 endpoints (`/api/mesh-metrics`, `/api/mesh-metrics/{kpi}`, etc.)
**Role**: Broker, Auditor, Transformation Leader (ADR-040)

### 8.1 KPI Cards

Four cards in a responsive grid (`grid-cols-1 md:grid-cols-2 xl:grid-cols-4`):

| KPI | Unit | Display | Colour Logic |
|-----|------|---------|-------------|
| Mesh Velocity | hours | Value + sparkline + confidence band | Green < 48h, amber 48-168h, red > 168h |
| Augmentation Ratio | percentage | Value + sparkline + target line | Green > 80%, amber 60-80%, red < 60% |
| Trust Variance | stddev | Value + sparkline + threshold line | Green < 0.15, amber 0.15-0.3, red > 0.3 |
| HITL Precision | percentage | Value + sparkline + target line | Green > 70%, amber 50-70%, red < 50% |

Each card uses the `Card` component and contains:
- KPI name and icon (Lucide: `Zap` for Velocity, `Users` for Augmentation, `Shield` for Trust, `Target` for HITL)
- **Current value** (large, prominent, `text-3xl font-bold`)
- **Confidence indicator**: filled circle (high >= 0.7), half-filled (medium 0.3-0.7), outline (low < 0.3) -- derived from `confidence` field
- **Sparkline**: `Sparkline` component (Canvas2D, design system) with 30-point time series. Uses crystalline colour palette: cyan-to-violet gradient fill (`color="#06b6d4"`, `fillColor="#8b5cf6"`). Animated draw-on with ease-out-cubic (800ms). Glow endpoint dot.
- **Confidence band**: translucent fill area around the sparkline representing confidence range
- **Trend indicator**: Lucide `TrendingUp`/`TrendingDown`/`Minus` icon with percentage change vs previous window, coloured by direction

**Click interaction**: Click a KPI card -> navigate to `/kpi/{metric}` drill-down view. Card gains `cursor-pointer` and Framer Motion `whileHover={{ scale: 1.02 }}` with `transitions.spring.snappy`.

### 8.2 Time Window Selector

A segmented control using the `Tabs` component, positioned at the top of the dashboard:
- 24h | 7d | 30d | 90d
- Changing the window re-fetches all KPI data for the selected range
- Default: 7d
- Persisted in URL query param (`?window=7d`)

### 8.3 Dimension Slicing

A `FilterBar` (collapsible) below the time selector with dimension selectors:
- Team: `Select` multi-select
- Function: `Select` multi-select
- Workflow Type: `Select` multi-select
- Agent Type: `Select` multi-select

Dimension values populated from `/api/mesh-metrics/{kpi}/slice` endpoint. Selecting dimensions re-fetches KPI values filtered to those dimensions. Selections persisted in URL query params.

### 8.4 KPI Drill-down

**Route**: `/kpi/:metric`

Full-page detail for a single KPI:

- **Large sparkline chart**: expanded `Sparkline` (width: full container, height: 240px) with interactive zoom/pan. Mouse hover shows crosshair with exact value + timestamp tooltip.
- **Lineage table**: `DataTable` of source events contributing to the current snapshot value
  - Columns: event type (icon), event ID, timestamp, actor, contribution description
  - Click event row -> navigate to source entity (broker case, workflow proposal, etc.)
- **Historical trend**: Full `Sparkline` with confidence bands, 100+ data points (triggers WASM acceleration per ADR-047 threshold)
- **Export button**: CSV download via `/api/mesh-metrics/export?from=...&to=...&kpis={metric}`

### 8.5 Real-time Updates

The dashboard subscribes to `MetricSnapshotCreated` events via WebSocket. On new snapshot:
1. Value animates from old to new using Framer Motion `useMotionValue` + `useTransform` with spring interpolation
2. Sparkline appends new point with slide-left animation (the draw buffer shifts, new point draws in from the right)
3. Trend arrow recalculates and updates

**Polling fallback**: If WebSocket is unavailable, poll `/api/mesh-metrics` every 60 seconds.

---

## 9. Surface 4: Connector Management

**Route**: `/connectors`
**Backend**: ADR-044 endpoints (`/api/connectors`, `/api/connectors/{id}`, etc.)
**Role**: Admin (ADR-040)

### 9.1 Connector List

A `DataTable` of configured connectors:

| Column | Source | Width |
|--------|--------|-------|
| Name | `display_name` | flex |
| Type | `connector_type` + icon (Lucide `Github` for GitHub) | 96px |
| Status | `status` via `StatusDot` (active=`active`, paused=`warning`, error=`error`, disabled=`inactive`, configuring=`processing`) | 96px |
| Last Sync | `sync_state.last_sync_at` (relative time) | 128px |
| Signals | count of Insights from this connector | 80px |
| Legal Review | `legal_review_mode` via `Switch` toggle | 80px |
| Actions | edit / sync / pause `Button` variants | 120px |

**Interactions**:
- Click row -> navigate to `/connectors/{id}/config`
- "Add Connector" button -> navigate to `/connectors/new` (wizard)
- "Sync Now" button -> POST `/api/connectors/{id}/sync`, shows toast on completion
- Pause/Resume toggle -> PUT `/api/connectors/{id}` with updated status (optimistic update)
- Legal Review toggle -> PUT `/api/connectors/{id}` with updated `legal_review_mode`

**Real-time status**: Poll connector status every 60 seconds. Status transitions animate with `StatusDot` colour change.

### 9.2 Configuration Wizard

**Route**: `/connectors/new` or `/connectors/:id/config`

A multi-step wizard. Step indicators use the `StepIndicator` design system component. Steps navigate via `Tabs` with `Alt+Left` / `Alt+Right` keyboard shortcuts. Each step validates before allowing progression.

**Step 1: Connector Type**
- Card grid of available connector types (GitHub Issues/PRs initially, extensible)
- Each card: type icon, name, description, required permissions list
- Click to select; "Next" button enables

**Step 2: Authentication**
- OAuth2 flow: "Connect with GitHub" button triggers OAuth redirect flow
- Personal access token fallback: `Input` with `type="password"` for token entry
- Validation: POST to `/api/connectors/validate` (calls `ConnectorAdapter::validate_config`)
- Success: green `StatusDot` + "Connected" label. Failure: red `StatusDot` + error message.

**Step 3: Scope**
- Dynamic form based on connector type
- GitHub: owner `Input`, repo multi-`Select` (populated after auth), event type checkboxes (`issues`, `pull_requests`, `reviews`), label filter `Input` (comma-separated), PR state `Select` (`merged`, `closed`, `all`)
- Preview: "This configuration will sync [N repos], [M event types]"

**Step 4: Privacy**
- Redaction policy `RadioGroup` (none / standard / aggressive / custom)
- Description of what each policy redacts (expandable `Collapsible` per policy)
- Custom mode: regex pattern `Input` list with add/remove and live test field
- Legal review mode `Switch` toggle with explanation: "When enabled, all signals require human review before indexing"

**Step 5: Schedule**
- Sync interval `Slider` (5-1440 minutes, default 15)
- Preview: "First sync will process events from the last [interval] minutes"

**Step 6: Review and Create**
- Summary of all configuration choices in a read-only card layout
- "Create Connector" `Button` (POST `/api/connectors`)
- On success: navigate to `/connectors` with toast confirmation
- On error: inline error with link back to the failing step

### 9.3 Signal Feed

**Route**: `/connectors?tab=signals`

A chronological feed of recent discovery signals (Insights) from all connectors:
- Each signal card: connector source icon, title, timestamp, redaction indicator badge (if fields were redacted)
- Signals pending legal review: amber `Badge` with "Pending Review" label
- Click signal -> expand inline to show full Insight detail (fields, provenance, linked graph entities)
- Filter by connector, date range, review status

### 9.4 Redaction Rule Editor

**Route**: `/connectors/:id/config?tab=redaction`

Within the connector configuration view, a tab for managing custom redaction rules:
- `DataTable` of active rules: regex pattern, rule name, enabled `Switch` toggle
- "Add Rule" form: name `Input`, regex pattern `Input`, test input `Textarea` with live preview of redaction (matched text highlighted in red, replaced text shown in green)
- Test bench: paste sample text, see which rules match and what gets redacted
- Validation: regex syntax checked client-side before save
- Changes saved to connector's `redaction_policy` via PUT `/api/connectors/{id}`

---

## 10. Surface 5: Policy Console

**Route**: `/policy`
**Backend**: ADR-045 endpoints (`/api/policy`, `/api/policy/{rule_id}`, etc.)
**Role**: Admin (ADR-040)

### 10.1 Rule List

A `DataTable` of all policy rules:

| Column | Source | Width |
|--------|--------|-------|
| Rule ID | `rule_id` | 200px |
| Description | `description` | flex |
| Enabled | `enabled` via `Switch` toggle | 64px |
| Last Evaluated | from evaluation log | 128px (relative time) |
| Hit Rate | non-Allow outcomes / total evaluations | 96px (`Progress` bar) |

`Switch` toggles fire PUT `/api/policy/{rule_id}` with `{ enabled: true|false }`. Changes applied optimistically with Immer `produce()` and rollback on API error.

### 10.2 Rule Editor

**Route**: `/policy/:ruleId`

A form-based editor for policy rule configuration. Form fields are dynamically generated from the rule's configuration schema. Each rule type has a distinct form layout:

| Rule | Form Fields |
|------|-------------|
| `escalation_threshold` | Threshold `Slider` (0-1.0, step 0.05), applies-to workflow type multi-`Select`, action-on-breach `Select` (deny/escalate) |
| `domain_ownership` | Domain-to-pubkey mapping `DataTable` (add/remove rows), domain name `Input`, pubkey `Input` list |
| `confidence_threshold` | Min confidence `Slider` (0-1.0), action `Select` (deny/escalate) |
| `separation_of_duty` | Applies-to action multi-`Select` from `PolicyAction` enum |
| `deployment_scope` | Max scope without escalation `Select` (team/department/org-wide) |
| `rate_limit` | Max actions per agent `Input` (number), window minutes `Input` (number), action-on-breach `Select` |

Each form uses existing design system components. All fields display current server values on load (GET `/api/policy/{rule_id}`). Changes saved via "Save" `Button` (PUT `/api/policy/{rule_id}`). "Reset to Defaults" `Button` restores factory values with confirmation `Dialog`.

The TOML configuration backing is transparent to the user; they interact with form controls, not raw TOML.

### 10.3 Evaluation Log

**Route**: `/policy?tab=evaluations`

A `DataTable` of recent policy evaluations:

| Column | Source | Width |
|--------|--------|-------|
| Timestamp | `evaluated_at` | 128px (relative time) |
| Rule | `rule_id` | 160px |
| Outcome | `outcome` via `StatusDot` (Allow=`active`, Deny=`error`, Escalate=`info`, Warn=`warning`) | 96px |
| Action | `context_action` | 160px |
| Resource | `context_resource_id` (linked) | flex |
| Reasoning | `reasoning` (truncated to 120 chars) | flex |

Click row -> expand inline (`Collapsible`) to show full reasoning, context metadata, and actor identity. Click resource link -> navigate to the referenced entity (workflow proposal, broker case, etc.).

Filters: rule ID `Select`, outcome multi-`Select`, date range, action type `Select`.

### 10.4 Policy Test Bench

**Route**: `/policy?tab=test`

A form for simulating policy evaluations against hypothetical contexts (dry-run, no provenance event emitted):

| Field | Component |
|-------|-----------|
| Actor | `Select` from known users/agents |
| Action | `Select` from `PolicyAction` enum (ApproveWorkflow, DeployWorkflow, EscalateCase, OverrideDecision, AccessConnector, ModifyPolicy) |
| Resource | `Input` (resource reference UUID) or `Select` from recent resources |
| Confidence | `Slider` (0-1.0, step 0.05) |
| Metadata | JSON `Textarea` for additional context fields |

"Evaluate" `Button` -> POST to test evaluation endpoint. Results panel below shows all rule evaluations with `StatusDot` outcome indicators, reasoning, and which rules would fire.

---

## 11. Design Principles

### 11.1 Dark Theme Continuity

All enterprise surfaces inherit the VisionClaw dark theme. Background: `#000022` (from `MainLayout.tsx`). Text hierarchy:
- `--foreground` for primary text
- `--muted-foreground` for secondary text
- `--accent` for interactive elements
- `--destructive` for error states and critical priority

No light theme planned for initial release. All new components are dark-background-only.

### 11.2 Information Density

Enterprise users need data, not whitespace:
- Table rows: 36px height (compact), 44px (comfortable). User toggle in sidebar settings.
- Card padding: 12px (vs typical 16-24px)
- Font size: 13px body, 11px metadata, 16px headings
- No decorative illustrations or hero sections on data views
- `Collapsible` sections default to expanded on desktop

### 11.3 Cosmic Aesthetic

Subtle visual cues that maintain the VisionClaw identity without distracting from data:
- Card borders: 1px `--border` with subtle gradient shimmer on hover (CSS `border-image` with animated HSL hue rotation, duration 4s, opacity 0.3)
- Priority badges: bioluminescent glow `box-shadow: 0 0 8px hsl(var(--priority-hue) 70% 50% / 0.3)`
- KPI sparklines: crystalline colour palette (cyan-to-violet gradient)
- Loading states: existing `LoadingSkeleton` with gentle pulse animation

### 11.4 Micro-interactions

All state transitions use Framer Motion with presets from `client/src/features/design-system/animations.ts`:

| Trigger | Animation Preset |
|---------|-----------------|
| Panel open/close | `variants.slideLeft` (slide from right) |
| Table row appear | `variants.slideDown` with staggered delay (10ms/row, max 20) |
| Badge status change | `variants.scale` with `transitions.spring.snappy` |
| KPI value change | `useMotionValue` with spring interpolation |
| Page transitions | Cross-fade with `variants.scale` (150ms) |
| Card hover | `whileHover={{ y: -2 }}` with `transitions.spring.smooth` |
| Pipeline stage pulse | `animate={{ scale: [1, 1.1, 1] }}` with `repeat: Infinity, duration: 2` |

### 11.5 Keyboard Navigation

Power users expect keyboard-first interaction:
- `Tab` / `Shift+Tab`: move between focusable elements
- `Arrow Up/Down`: navigate table rows, list items
- `Enter`: activate focused element
- `Escape`: close modal, cancel editing, navigate back
- `Cmd+K`: command palette
- Surface-specific shortcuts documented in a `?` help overlay per surface (using existing `HelpProvider` from `client/src/features/help/`)

### 11.6 Accessibility

- All interactive elements have ARIA labels
- Table rows use `role="row"` with `aria-selected` for focused state
- Status badges have `aria-label` describing the status (not just colour)
- Focus indicators: 2px solid `--ring` on all interactive elements
- Screen reader announcements for real-time updates (`aria-live="polite"` region)
- All `StatusDot` instances include a `label` prop for screen readers

---

## 12. State Management

### 12.1 Store Architecture

Each enterprise surface gets its own Zustand store, following the existing `settingsStore.ts` pattern (`create()` with Immer `produce()`):

```
client/src/features/broker/store/brokerStore.ts
client/src/features/workflows/store/workflowStore.ts
client/src/features/kpi/store/kpiStore.ts
client/src/features/connectors/store/connectorStore.ts
client/src/features/policy/store/policyStore.ts
```

Store design principles (see ADR-046 for full rationale):
- **No cross-store imports**: stores do not subscribe to each other. Cross-surface navigation uses the router.
- **API calls inside store actions**: components call `store.fetchInbox()`, not `fetch('/api/...')`.
- **Optimistic updates**: mutations applied via `produce()` with rollback on API error.
- **Selector granularity**: components subscribe to specific slices via Zustand selectors to minimise re-renders.
- **No persistence**: enterprise stores are not persisted to localStorage. Data is server-authoritative.

### 12.2 Store Shape (Broker -- representative)

```typescript
interface BrokerState {
  inbox: {
    cases: BrokerCase[];
    totalCount: number;
    cursor: string | null;
    filters: InboxFilters;
    sort: { field: string; direction: 'asc' | 'desc' };
    loading: boolean;
    error: string | null;
  };
  activeCase: {
    case: BrokerCaseDetail | null;
    evidence: CaseEvidence | null;
    similarDecisions: BrokerDecision[];
    loading: boolean;
    error: string | null;
  };
  timeline: {
    decisions: BrokerDecision[];
    loading: boolean;
    cursor: string | null;
  };
  fetchInbox: () => Promise<void>;
  fetchCase: (id: string) => Promise<void>;
  claimCase: (id: string) => Promise<void>;
  releaseCase: (id: string) => Promise<void>;
  submitDecision: (id: string, action: DecisionAction) => Promise<void>;
  submitCase: (data: CaseSubmitPayload) => Promise<void>;
  setFilters: (filters: Partial<InboxFilters>) => void;
  setSort: (field: string, direction: 'asc' | 'desc') => void;
}
```

### 12.3 WebSocket Integration

Real-time updates extend the existing WebSocket infrastructure (`client/src/store/websocket/`):
- New subscription channels: `broker:inbox`, `broker:case:{id}`, `kpi:snapshots`, `connector:{id}:sync`
- Subscriptions managed per-surface: subscribe on mount (`useEffect`), unsubscribe on unmount (cleanup)
- Incoming messages dispatch to the appropriate Zustand store via a channel-to-store router
- See ADR-046 for the `useBrokerWebSocket` hook pattern

---

## 13. API Integration

All API calls use the existing fetch-based pattern from `client/src/api/`. New API modules:

```
client/src/api/brokerApi.ts      -> /api/broker/*
client/src/api/workflowApi.ts    -> /api/workflows/*
client/src/api/kpiApi.ts         -> /api/mesh-metrics/*
client/src/api/connectorApi.ts   -> /api/connectors/*
client/src/api/policyApi.ts      -> /api/policy/*
```

### 13.1 Polling Strategy

| Surface | Endpoint | Interval | Method |
|---------|----------|----------|--------|
| Broker Inbox | `/api/broker/inbox` | 15s | WebSocket primary, polling fallback |
| KPI Dashboard | `/api/mesh-metrics` | 30s | WebSocket primary, polling fallback |
| Connector Status | `/api/connectors` | 60s | Polling (no WS channel) |
| Policy Evaluations | `/api/policy/evaluations` | No poll | Manual refresh |

### 13.2 Authentication

All endpoints require Nostr NIP-98 authentication (existing `nostrAuth` service) or OIDC session (ADR-040, when implemented). Unauthorized (401) -> redirect to login. Forbidden (403) -> role-insufficient error message with the required role displayed.

---

## 14. File Structure

New feature modules follow the existing feature directory convention:

```
client/src/features/
  broker/
    components/
      BrokerInbox.tsx
      CaseSubmitForm.tsx
      DecisionCanvas.tsx
      DecisionActions.tsx
      BrokerTimeline.tsx
      EvidencePanel.tsx
      GraphContextViewer.tsx
      PriorityBadge.tsx
      CaseRow.tsx
    hooks/
      useBrokerInbox.ts
      useBrokerCase.ts
      useBrokerWebSocket.ts
      useBrokerCommands.ts
    store/
      brokerStore.ts
    types/
      broker.ts
    index.ts                 # Default export for React.lazy()
  workflows/
    components/
      ProposalList.tsx
      ProposalDetail.tsx
      CreateProposalForm.tsx
      StepEditor.tsx
      VersionDiff.tsx
      PromotionFlow.tsx
      PatternLibrary.tsx
    hooks/
      useWorkflowProposals.ts
      useWorkflowVersions.ts
      useWorkflowCommands.ts
    store/
      workflowStore.ts
    types/
      workflow.ts
    index.ts
  kpi/
    components/
      KpiDashboard.tsx
      KpiCard.tsx
      KpiDrilldown.tsx
      SparklineChart.tsx
      ConfidenceIndicator.tsx
      TimeWindowSelector.tsx
      DimensionSlicers.tsx
    hooks/
      useKpiMetrics.ts
      useKpiWebSocket.ts
      useKpiCommands.ts
    store/
      kpiStore.ts
    types/
      kpi.ts
    index.ts
  connectors/
    components/
      ConnectorList.tsx
      ConnectorWizard.tsx
      WizardStepType.tsx
      WizardStepAuth.tsx
      WizardStepScope.tsx
      WizardStepPrivacy.tsx
      WizardStepSchedule.tsx
      WizardStepReview.tsx
      SignalFeed.tsx
      RedactionRuleEditor.tsx
      SyncStatusIndicator.tsx
    hooks/
      useConnectors.ts
      useConnectorSync.ts
      useConnectorCommands.ts
    store/
      connectorStore.ts
    types/
      connector.ts
    index.ts
  policy/
    components/
      PolicyRuleList.tsx
      PolicyRuleEditor.tsx
      EvaluationLog.tsx
      PolicyTestBench.tsx
    hooks/
      usePolicyRules.ts
      usePolicyEvaluations.ts
      usePolicyCommands.ts
    store/
      policyStore.ts
    types/
      policy.ts
    index.ts
```

### Design System Extensions

Components already added to `client/src/features/design-system/components/` (exported from `index.ts` barrel):

| Component | File | Purpose |
|-----------|------|---------|
| `DataTable` | `DataTable.tsx` | Sortable, filterable, keyboard-navigable table (HTML `<table>`, Radix sort controls) |
| `StatusDot` | `StatusDot.tsx` | Colour-coded dot + label, 5 status variants (active/warning/error/inactive/processing) with CSS glow effects |
| `Sparkline` | `Sparkline.tsx` | Canvas2D sparkline with animated draw-on, gradient fill, glow endpoint, DPR-aware |
| `EmptyState` | `EmptyState.tsx` | Centred icon + title + description + optional CTA action |
| `Timeline` | `Timeline.tsx` | Vertical timeline with status-coloured dots, connector lines, metadata badges |

New components to be added:

| Component | File | Purpose |
|-----------|------|---------|
| `StepIndicator` | `StepIndicator.tsx` | Horizontal pipeline progress bar with stage labels and active stage pulse animation |
| `FilterBar` | `FilterBar.tsx` | Collapsible container for filter controls (wraps `Collapsible`) |
| `SidebarNav` | `SidebarNav.tsx` | Left sidebar with registration pattern (see ADR-046) |
| `KeyboardShortcutHint` | `KeyboardShortcutHint.tsx` | Inline `<kbd>` styled badge for displaying shortcuts |

---

## 15. Performance Requirements

| Metric | Target | Measurement |
|--------|--------|-------------|
| Broker Inbox load (50 cases) | < 2s | Time from route entry to first render with data |
| Decision Canvas load | < 3s | Time from route entry to all evidence panels populated |
| KPI Dashboard load | < 2s | Time from route entry to all 4 cards rendered with sparklines |
| Table sort/filter | < 100ms | Time from user action to re-render |
| WebSocket message to UI update | < 200ms | Time from WS message receipt to DOM update |
| Page transition | < 150ms | Time from route change to new view visible |
| WASM sparkline render (1000 points) | < 16ms | Single frame budget for sparkline re-render |
| Core bundle | < 120KB gzipped | React, Zustand, Router, design system, SidebarNav |
| Enterprise chunk (each) | < 40KB gzipped | Per-surface lazy-loaded chunk |
| Graph chunk | < 300KB gzipped | Three.js, R3F, WASM scene-effects, MainLayout |

Enterprise feature modules are code-split via `React.lazy()` at the route level. The 3D graph bundle (Three.js, R3F) is not loaded on enterprise routes unless the Decision Canvas is accessed (which embeds a mini-graph).

---

## 16. Rollout Plan

### Phase 1: Navigation + Broker Workbench (4 weeks)
- Add `react-router-dom` v7
- Implement `RootLayout`, `SidebarNav`, router structure
- Implement Broker Inbox + Case Submit Form + Decision Canvas + Decision Actions
- Implement Broker Timeline
- WebSocket subscription for inbox updates
- Command palette integration for broker commands
- Design system additions: `StepIndicator`, `FilterBar`, `SidebarNav`, `KeyboardShortcutHint`

### Phase 2: Workflow Studio + KPI Dashboard (3 weeks)
- Proposal List + Create Proposal Form + Proposal Detail with step editor
- Version diff viewer + Promotion Flow indicator
- Pattern Library
- KPI Dashboard with Canvas2D sparklines
- Time window selector + dimension slicing
- KPI drill-down with lineage table

### Phase 3: Connectors + Policy (3 weeks)
- Connector list + configuration wizard (6 steps)
- Signal feed + redaction rule editor
- Policy rule list + editor (6 rule type forms)
- Evaluation log + policy test bench

### Phase 4: WASM Enhancement + Polish (2 weeks)
- Compile `kpi-sparklines`, `workflow-dag`, `broker-heatmap` WASM modules (ADR-047)
- Integrate WASM bridges with capability detection and TypeScript fallback
- Performance optimisation pass (bundle analysis, render profiling)
- Keyboard navigation audit
- Accessibility audit (ARIA, focus management, screen reader testing)

---

## 17. Success Metrics

| Metric | Target | Measurement Method |
|--------|--------|--------------------|
| Broker decision throughput | 20+ decisions/day/broker | Count of `BrokerDecisionMade` events per broker per day |
| Mean time to decision | < 5 minutes | Median elapsed time from case claim to decision |
| Workflow proposals per month | 10+ | Count of `WorkflowProposalSubmitted` events |
| KPI dashboard weekly active users | 80% of enterprise users | Unique sessions on `/kpi` per week |
| Policy configuration changes | Admin self-service (no code deploy) | Count of PUT `/api/policy/*` calls |
| Connector sync success rate | > 99% | Successful syncs / total sync attempts |
| UI error rate | < 0.1% of page loads | Client error tracking |
| Broker Inbox load time | < 2s p95 | Real User Monitoring |
| Page transition time | < 150ms p95 | Real User Monitoring |

---

## 18. Open Questions

1. **Rich text vs markdown for broker reasoning**: The current design uses markdown in a `Textarea`. Should the reasoning field support a WYSIWYG editor for non-technical brokers? Recommendation: defer; markdown with preview is sufficient for pilot. Revisit based on broker feedback.

2. **Offline support**: Should enterprise surfaces work offline with service worker caching? The store architecture does not preclude it, but it is not planned for initial release. Server-authoritative data means offline support adds significant sync complexity.

3. **Notification preferences**: Brokers may want email/Slack notifications for new high-priority cases. This is a backend concern (ADR-041 scope) but the UI would need a preferences panel. Defer to post-pilot.

4. **Multi-language support**: Enterprise deployments in non-English organisations may need i18n. The current codebase has no i18n framework. Defer unless pilot feedback demands it.

5. **Audit export format**: CSV is specified for KPI export. Should the broker timeline and policy evaluation log also support CSV export for compliance teams? Recommendation: yes, add export to all tabular views in Phase 4.

6. **Decision Canvas graph mini-view performance**: Loading R3F for a constrained mini-graph on the Decision Canvas may be heavy. If load time exceeds 1s, consider a static SVG rendering of the subgraph as an alternative.

---

## 19. References

- ADR-040: Enterprise Identity Strategy
- ADR-041: Judgment Broker Workbench Architecture
- ADR-042: Workflow Proposal Object Model
- ADR-043: KPI Lineage Model
- ADR-044: Connector Governance and Privacy Boundaries
- ADR-045: Policy Engine Approach
- ADR-046: Enterprise UI Architecture (companion ADR)
- ADR-047: WASM Visualization Components (companion ADR)
- PRD-001: VisionFlow Data Pipeline Alignment
- `client/src/features/design-system/` (existing component library)
- `client/src/features/design-system/animations.ts` (Framer Motion presets)
- `client/src/wasm/scene-effects-bridge.ts` (existing WASM bridge pattern)
- `client/src/features/command-palette/types.ts` (command registration interface)
- `client/src/store/settingsStore.ts` (Zustand + Immer store pattern)
- `client/src/store/websocketStore.ts` (WebSocket connection lifecycle)
