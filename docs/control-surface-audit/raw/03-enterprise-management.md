# Control Surface Audit: Enterprise, Management, and Day-to-Day User Surfaces

**Scope**: Enterprise control surface, Management dashboards, Day-to-Day User (Contributor Studio), onboarding, monitoring, and related management features.

**Branch**: feature/unified-control-surface

**Audit Date**: 2026-04-28

---

## Surface 1: Enterprise Control Panel (Standalone)

### Entry Point
- **File**: `/client/src/enterprise-standalone.tsx`
- **Mount**: Standalone HTML entry point (index.html -> root)
- **Build Target**: Enterprise dashboard application
- **Route Hash**: `#/` (EnterprisePage with hash-based navigation)

### Component Tree

| Path | Component | Role |
|------|-----------|------|
| enterprise-standalone.tsx:20 | `EnterprisePage` | Root container; hash router selector |
| enterprise-standalone.tsx:21-27 | `DrawerFxDemo` (lazy) | WASM FX demo page (route: `/fx-demo`) |
| enterprise-standalone.tsx:29-34 | `EnterpriseDrawerDemo` (lazy) | Drawer demo page (route: `/drawer-demo`) |
| features/enterprise/components/EnterprisePanel.tsx:9 | `EnterprisePanel` | Main control center root; five-panel selector |
| features/enterprise/components/EnterpriseNav.tsx:16 | `EnterpriseNav` | Vertical nav bar; panel switcher |
| features/broker/components/BrokerWorkbench.tsx:9 | `BrokerWorkbench` | "Judgment Broker" tab (Inbox, Submit Case, Timeline) |
| features/workflows/components/WorkflowStudio.tsx:37 | `WorkflowStudio` | "Workflows" tab (proposals, patterns) |
| features/kpi/components/MeshKpiDashboard.tsx:38 | `MeshKpiDashboard` | "KPIs" tab (Mesh Velocity, Augmentation Ratio, Trust Variance, HITL Precision) |
| features/connectors/components/ConnectorPanel.tsx:70 | `ConnectorPanel` | "Connectors" tab (list, signals, setup wizard) |
| features/policy/components/PolicyConsole.tsx:48 | `PolicyConsole` | "Policy" tab (rules, evaluation logs) |
| features/enterprise/components/EnterpriseDrawer.tsx:46 | `EnterpriseDrawer` | Sliding modal panel; overlay on graph |

### Controls

| File:Line | Element | Setting/Action Key | Type | Persisted To | Server Endpoint |
|-----------|---------|-------------------|------|--------------|-----------------|
| features/enterprise/components/EnterpriseNav.tsx:27-39 | Panel toggle buttons (Broker, Workflows, KPIs, Connectors, Policy) | `activePanel` (broker \| workflows \| kpi \| connectors \| policy) | Controlled select | React state | N/A (local UI state) |
| features/enterprise/components/EnterpriseDrawer.tsx:181-188 | Close button (X icon) | `onClose` callback | Button | N/A | N/A |
| features/enterprise/components/EnterpriseDrawer.tsx:64-74 | Escape key listener | `onClose` callback | Keyboard event | N/A | N/A |
| features/enterprise/store/drawerStore.ts:28-32 | `openDrawer()` | `open` (boolean) | State action | localStorage: `visionflow.enterprise.drawer` | N/A |
| features/enterprise/store/drawerStore.ts:33 | `closeDrawer()` | `open` (boolean) | State action | localStorage: `visionflow.enterprise.drawer` | N/A |
| features/enterprise/store/drawerStore.ts:34 | `toggleDrawer()` | `open` (boolean) | State action | localStorage: `visionflow.enterprise.drawer` | N/A |
| features/enterprise/store/drawerStore.ts:35 | `setActiveSection()` | `activeSection` (string \| null) | State action | localStorage: `visionflow.enterprise.drawer` | N/A |
| features/broker/components/CaseSubmitForm.tsx:55-91 | Case submission form | `title`, `description`, `priority` (critical/high/medium/low) | Form inputs | N/A | POST `/api/broker/cases` |
| features/broker/components/CaseSubmitForm.tsx:14-20 | Form state: title, description, priority | Controlled inputs | Text/Select | N/A | N/A |
| features/workflows/components/WorkflowStudio.tsx:44-48 | New proposal form | `newTitle`, `newDescription` | Text inputs | N/A | POST `/api/workflows/proposals` |
| features/workflows/components/WorkflowStudio.tsx:71-89 | Promote button (proposals -> patterns) | Triggers promotion workflow | Button | N/A | POST `/api/workflows/proposals/{id}/promote` |
| features/kpi/components/MeshKpiDashboard.tsx:73-83 | Time window selector | `timeWindow` (24h/7d/30d/90d) | Select dropdown | N/A | GET `/api/mesh-metrics` (param: timeWindow) |
| features/connectors/components/ConnectorPanel.tsx:70-80 | Setup wizard toggle | `showSetup` (boolean) | Controlled state | N/A | N/A |
| features/connectors/components/ConnectorPanel.tsx:78-80 | Setup org/repos/redaction fields | `setupOrg`, `setupRepos`, `setupRedaction` | Text input, Boolean | N/A | POST `/api/connectors` |
| features/policy/components/PolicyConsole.tsx:56-57 | Toggle rule enable/disable | `enabled` (boolean) per rule | Checkbox button | N/A | Locally evaluated (no backend persistence shown) |
| features/policy/components/PolicyConsole.tsx:76-80 | Test action dropdown | `testAction` (workflow.promote/workflow.approve/etc) | Select | N/A | N/A (local evaluation) |
| features/policy/components/PolicyConsole.tsx:53 | Confidence threshold slider | `testConfidence` (0-100) | Slider | N/A | N/A (local evaluation) |
| features/policy/components/PolicyConsole.tsx:76-80 | Run Test button | Executes `evaluateLocally()` | Button | N/A | N/A (local evaluation) |

### Live Displays

| File:Line | Widget | Data Source | Cadence |
|-----------|--------|-------------|---------|
| features/kpi/components/MeshKpiDashboard.tsx:45-66 | KPI metric cards (mesh_velocity, augmentation_ratio, trust_variance, hitl_precision) | GET `/api/mesh-metrics` | 30s poll (useEffect: 30000ms) |
| features/kpi/components/MeshKpiDashboard.tsx:119-126 | Sparkline trend chart (per KPI) | Generated locally from `generateTrendData()` | Static (seeded by kpiKey) |
| features/broker/components/BrokerInbox.tsx | Case inbox | GET `/api/broker/cases` (stub) | On mount, no auto-poll shown |
| features/broker/components/BrokerTimeline.tsx | Timeline of decisions | GET `/api/broker/timeline` (stub) | On mount |
| features/workflows/components/WorkflowStudio.tsx:54-59 | Proposals list | GET `/api/workflows/proposals` | On mount |
| features/workflows/components/WorkflowStudio.tsx:54-59 | Patterns list | GET `/api/workflows/patterns` | On mount, updated after promote |
| features/connectors/components/ConnectorPanel.tsx:84+ | Connectors list with signals | GET `/api/connectors` | On mount, manually refreshed |
| features/policy/components/PolicyConsole.tsx:49-50 | Policy rules list | Hardcoded DEFAULT_RULES; no backend | Static |
| features/policy/components/PolicyConsole.tsx:50 | Evaluation logs | Appended locally on test run | Static |
| features/monitoring/components/HealthDashboard.tsx:103-127 | Overall health status card | GET `/health` | 5s poll (useHealthService: pollInterval) |
| features/monitoring/components/HealthDashboard.tsx:130-147 | Component health grid | GET `/health` response.components | 5s poll |
| features/monitoring/components/HealthDashboard.tsx:150-191 | Physics simulation health | GET `/health/physics` | 5s poll |

### WASM Modules Used

| Crate/Module | JS Bridge | Inputs | Outputs |
|--------------|-----------|--------|---------|
| drawer_fx (Rust WASM) | `/client/src/features/enterprise/fx/wasm/drawer_fx.js` | Canvas element (by DOM id), width, height, x/y mouse pulse coords, quality tier (0/1/2), deltaTime (ms) | 2D Canvas WebGL particles (flow-field); frame tick @ rAF |
| DrawerFx class | `drawerFx.ts:DrawerFxController` | `attach(canvas)`, `resize(w,h)`, `pulse(x,y)`, `setQuality(q)`, `start()`, `stop()`, `destroy()` | Visual effect on canvas; respects `prefers-reduced-motion` |
| Flow-field effect | `useDrawerFx.ts` hook | Canvas ref, active state, quality/dpr options | Attaches to canvas; ~60 Hz throttle on pulse() |

**WASM Integration Points**:
- `enterprise-standalone.tsx:8` lazy-imports `DrawerFxDemo`
- `EnterpriseDrawer.tsx:10` imports `useDrawerFx`
- `drawerFx.ts:44` dynamic import of `./wasm/drawer_fx.js` (split bundle, deferred load)
- No pre-reduced-motion detection fails gracefully; falls back to CSS gradient

### Disconnects & Notes

- **Stub endpoints**: `/api/broker/cases`, `/api/broker/timeline` (inbox, timeline) are not wired in BrokerWorkbench; forms are live but backend integration pending (agent C2)
- **Onboarding disabled**: `OnboardingProvider` components are commented out in `/features/onboarding/components/OnboardingProvider.tsx:52-65`
- **Local-only policy evaluation**: PolicyConsole rules are not persisted to backend; test runs are client-side only
- **Mock data**: Connector signals and workflow patterns use mock/stub responses
- **WASM fallback**: If `drawer_fx.wasm` fails to load, `drawerFx.ts:50` logs warning and returns null; caller (EnterpriseDrawer) applies CSS gradient fallback
- **Focus trap**: EnterpriseDrawer implements hand-rolled Tab trap (no @radix-ui dependency)

---

## Surface 2: Contributor Studio (Day-to-Day User Surface)

### Entry Point
- **File**: `/client/src/features/contributor-studio/routes.tsx` (route table + matcher)
- **Mount**: `/studio*` hash-based routes
- **Build Target**: Lazy-loaded chunks (one per sub-surface)
- **Root Component**: `ContributorStudioRoot` (`/studio/:workspaceId`)

### Routes

| Route Pattern | Component | Role |
|---------------|-----------|------|
| `/studio` | `WorkspaceListView` | List of user's workspaces |
| `/studio/new` | `WorkspaceCreateWizard` | Create new workspace wizard |
| `/studio/automations` | `AutomationList` | List scheduled automations |
| `/studio/automations/new` | `AutomationCreateWizard` | Schedule new automation (stub) |
| `/studio/automations/:id` | `AutomationDetail` | View automation details |
| `/studio/inbox` | `InboxView` | Inbox for nudges/notifications |
| `/studio/:workspaceId` | `ContributorStudioRoot` (4-pane shell) | Main workspace with OntologyGuideRail \| WorkLane \| AIPartnerLane + SessionMemoryBar |
| `/studio/:workspaceId/artifacts/:aid` | ContributorStudioRoot + `ArtifactDetail` | Artifact detail within workspace |
| `/studio/:workspaceId/skills` | ContributorStudioRoot + `InstalledSkills` | Installed skills panel |
| `/studio/:workspaceId/skills/dojo` | ContributorStudioRoot + `SkillDojo` | Skill training dojo |
| `/studio/:workspaceId/sensei` | ContributorStudioRoot + `SenseiTrace` | Sensei tracing/nudges |

### Component Tree (4-Pane Shell)

| Path | Component | Role |
|------|-----------|------|
| features/contributor-studio/components/ContributorStudioRoot.tsx:30 | `ContributorStudioRoot` | Root; fetches workspace context, loads nudges/inbox |
| features/contributor-studio/components/WorkspaceBar.tsx:27 | `WorkspaceBar` | Top 48px bar; workspace selector, focus pill, share state, automation count |
| features/contributor-studio/components/PaneLayout.tsx:30 | `PaneLayout` | 4-pane container (left \| centre \| right + bottom); resizable panels |
| features/contributor-studio/components/OntologyGuideRail.tsx:74 | `OntologyGuideRail` | Left pane (320px); Canonical Terms, Nearby Concepts, Applicable Policies, Installed Skills |
| features/contributor-studio/components/WorkLane.tsx | `WorkLane` | Centre pane (40%+ min); workspace content (children) |
| features/contributor-studio/components/AIPartnerLane.tsx:26 | `AIPartnerLane` | Right pane (380px); Partner selector, context panel, chat transcript, inbox chip |
| features/contributor-studio/components/SessionMemoryBar.tsx | `SessionMemoryBar` | Bottom bar; session memory indicator, memory management controls |

### Controls

| File:Line | Element | Setting/Action Key | Type | Persisted To | Server Endpoint |
|-----------|---------|-------------------|------|--------------|-----------------|
| features/contributor-studio/components/WorkspaceBar.tsx:41-52 | Workspace selector button | Navigate to workspace | Button | State in `studioWorkspaceStore.activeId` | Stub: GET `/api/studio/workspaces` |
| features/contributor-studio/components/WorkspaceBar.tsx:82-89 | Settings gear button | Workspace settings | Button | N/A | Stub (agent C1) |
| features/contributor-studio/components/PaneLayout.tsx:51 | Left pane resize handle | `leftWidth` (px) | ResizeObserver | `studioWorkspaceStore.layout` | Stub: persists on backend write (agent C1) |
| features/contributor-studio/components/PaneLayout.tsx:71 | Right pane resize handle | `rightWidth` (px) | ResizeObserver | `studioWorkspaceStore.layout` | Stub: persists on backend write |
| features/contributor-studio/components/OntologyGuideRail.tsx | Collapsible sections (Terms, Concepts, Policies, Skills) | Open/close state | Collapsible trigger | N/A (ephemeral) | N/A |
| features/contributor-studio/components/AIPartnerLane.tsx:42-52 | Partner selector button | `partnerSelection` (select AI partner) | Button | `studioWorkspaceStore.workspaces[id].partnerSelection` | Stub: POST `/api/studio/partner/select` (agent C1) |
| features/contributor-studio/components/AIPartnerLane.tsx:54-63 | Inbox chip button | Navigate to `/studio/inbox` | Button | N/A | N/A |
| features/contributor-studio/hooks/useStudioCommands.tsx | 15 palette commands | Launch commands (studio:open, studio:new-workspace, studio:switch-workspace, etc) | Command registry | N/A | N/A (dispatch CustomEvent or navigate) |
| features/contributor-studio/components/WorkspaceListView.tsx | Create workspace button | Navigate to `/studio/new` | Button | N/A | N/A |
| features/contributor-studio/components/WorkspaceCreateWizard.tsx | Wizard form (name, description, etc) | `name`, `description`, `shareState` | Form inputs | N/A | Stub: POST `/api/studio/workspaces` (agent C1) |
| features/contributor-studio/components/AutomationCreateWizard.tsx | Create automation form | Schedule, budget tier, delegated-cap | Form inputs | N/A | Stub: POST `/api/studio/automations` (agent C5) |

### Live Displays

| File:Line | Widget | Data Source | Cadence |
|-----------|--------|-------------|---------|
| features/contributor-studio/components/WorkspaceBar.tsx:28-32 | Active workspace name, focus pill, share state chip | `studioWorkspaceStore.workspaces` | On mount + store subscription |
| features/contributor-studio/components/WorkspaceBar.tsx:31 | Unread automation count | `useStudioInboxUnreadCount()` from `studioInboxStore` | Real-time (store update) |
| features/contributor-studio/components/OntologyGuideRail.tsx:77-79 | Nudges (terms, concepts, policies) | `useSenseiStore().nudgesByWorkspaceId[workspaceId]` | Fetched on mount: `loadNudges(workspaceId)` |
| features/contributor-studio/components/OntologyGuideRail.tsx | Installed skills list | `studioWorkspaceStore.workspaces[id].installedSkills` | On mount |
| features/contributor-studio/components/AIPartnerLane.tsx:27-29 | Chat transcript | `studioPartnerStore.transcriptsByWorkspaceId[workspaceId]` | Real-time (store update from WebSocket) |
| features/contributor-studio/stores/senseiStore.ts | Nudges per workspace | Stub: fetches nudges for assembly | Called from ContributorStudioRoot:37 |
| features/contributor-studio/stores/studioInboxStore.ts | Inbox items (automation nudges) | `fetchInbox()` | On mount; polling interval TBD |

### Stores (State Management)

| Store | File | Key Getters/Setters | Persisted |
|-------|------|-------------------|-----------|
| `useStudioWorkspaceStore` | stores/studioWorkspaceStore.ts | `workspaces`, `activeId`, `layout`, `fetchWorkspaces()`, `setActive()`, `updateFocus()`, `setPartner()`, `setLayout()`, `setShareState()` | `layout` to server (stub) |
| `useStudioContextStore` | stores/studioContextStore.ts | `assembleContext(workspaceId)` | N/A (ephemeral) |
| `useSenseiStore` | stores/senseiStore.ts | `nudgesByWorkspaceId`, `loadNudges()` | N/A (ephemeral) |
| `useStudioInboxStore` | stores/studioInboxStore.ts | `inbox`, `unreadCount`, `fetchInbox()` | N/A (ephemeral) |
| `useStudioPartnerStore` | stores/studioPartnerStore.ts | `transcriptsByWorkspaceId`, `partners` | N/A (ephemeral) |

### Disconnects & Notes

- **Stub workspaces**: `studioWorkspaceStore.fetchWorkspaces()` is a no-op (timeouts 0ms); real data comes from agent C1
- **AutomationCreateWizard**: Scaffolded; backend integration pending (agent C5)
- **Onboarding commented out**: OnboardingProvider overlay is disabled
- **AI partner lane**: Context panel says "wire-up with agent X1" (Sensei bridge pending)
- **SessionMemoryBar**: Component exists but implementation minimal
- **Command palette**: 15 commands registered; some dispatch custom events (e.g., `studio:switch-workspace`, `studio:run-skill`)

---

## Surface 3: Monitoring & Health Dashboard

### Entry Point
- **File**: `/client/src/features/monitoring/components/HealthDashboard.tsx`
- **Mount**: Can be mounted in any parent (embedded in management dashboards or standalone)
- **Hook**: `useHealthService` (features/monitoring/hooks/useHealthService.ts)

### Component Tree

| Path | Component | Role |
|------|-----------|------|
| features/monitoring/components/HealthDashboard.tsx:25 | `HealthDashboard` | Root; renders overall health, physics, MCP relay controls |
| features/monitoring/hooks/useHealthService.ts:48 | `useHealthService(options)` | Hook providing health API integration |

### Controls

| File:Line | Element | Setting/Action Key | Type | Persisted To | Server Endpoint |
|-----------|---------|-------------------|------|--------------|-----------------|
| features/monitoring/components/HealthDashboard.tsx:95-97 | Refresh button | `refreshHealth()` | Button | N/A | GET `/health` + `/health/physics` |
| features/monitoring/components/HealthDashboard.tsx:197-204 | Start MCP Relay button | `startMCPRelay()` | Button | N/A | POST `/health/mcp/start` |
| features/monitoring/components/HealthDashboard.tsx:201-204 | View Logs button (MCP) | `getMCPLogs()` | Button | N/A | GET `/health/mcp/logs` |

### Live Displays

| File:Line | Widget | Data Source | Cadence |
|-----------|--------|-------------|---------|
| features/monitoring/components/HealthDashboard.tsx:103-127 | Overall status card | GET `/health` (HealthStatus) | 5s auto-poll (useHealthService) |
| features/monitoring/components/HealthDashboard.tsx:115-120 | Version, last check timestamp | GET `/health` response | 5s auto-poll |
| features/monitoring/components/HealthDashboard.tsx:130-147 | Component health grid (database, graph, physics, websocket) | GET `/health` response.components | 5s auto-poll |
| features/monitoring/components/HealthDashboard.tsx:150-191 | Physics simulation health (running, steps, avg time, GPU memory) | GET `/health/physics` (PhysicsHealth) | 5s auto-poll |
| features/monitoring/components/HealthDashboard.tsx:207-214 | MCP logs display (pre block) | GET `/health/mcp/logs` (on-demand) | On-demand button click |

### API/WS Calls

| Caller File:Line | Method | Path | Description | Response Type |
|------------------|--------|------|-------------|----------------|
| features/monitoring/hooks/useHealthService.ts:64-67 | GET | `/health` | Overall system health | `HealthStatus` |
| features/monitoring/hooks/useHealthService.ts:64-67 | GET | `/health/physics` | Physics simulation status | `PhysicsHealth` |
| features/monitoring/hooks/useHealthService.ts:103-106 | POST | `/health/mcp/start` | Start MCP relay | `{ success: boolean; message: string }` |
| features/monitoring/hooks/useHealthService.ts:132 | GET | `/health/mcp/logs` | Retrieve MCP relay logs | `{ logs: string }` |

### Disconnects & Notes

- **Mock data not found**: Health endpoints appear to be real backend calls via `unifiedApiClient`
- **No error state persistence**: Errors are logged but transient

---

## Surface 4: Management Sub-Surfaces (Embedded in Enterprise Panel)

### 4A. Broker Workbench

**Entry**: `features/broker/components/BrokerWorkbench.tsx:9`

**Components**:
- `BrokerInbox` (tab)
- `CaseSubmitForm` (tab)
- `BrokerTimeline` (tab)
- `DecisionCanvas` (modal overlay)

**Controls**:
- Tab selector: inbox/submit/timeline
- Case selection: click inbox item -> DecisionCanvas
- Case submit form: title (text), description (textarea), priority (select: critical/high/medium/low)
- Decision canvas: decision form (stub)

**Live Data**:
- Inbox: GET `/api/broker/cases` (stub on mount)
- Timeline: GET `/api/broker/timeline` (stub on mount)

**API Endpoints**:
- POST `/api/broker/cases` (submit case)
- GET `/api/broker/cases` (fetch inbox)
- GET `/api/broker/timeline` (fetch timeline)

### 4B. Workflow Studio

**Entry**: `features/workflows/components/WorkflowStudio.tsx:37`

**Controls**:
- Time window selector (N/A for this surface, but in KPI dashboard)
- New proposal form: title, description
- Promote button (per proposal)
- Tab selector: proposals/patterns

**Live Data**:
- GET `/api/workflows/proposals` (list)
- GET `/api/workflows/patterns` (list)

**API Endpoints**:
- GET `/api/workflows/proposals`
- GET `/api/workflows/patterns`
- POST `/api/workflows/proposals` (create)
- POST `/api/workflows/proposals/{id}/promote` (promote to pattern)

### 4C. Connector Panel

**Entry**: `features/connectors/components/ConnectorPanel.tsx:70`

**Controls**:
- Setup wizard toggle
- Setup form: org (text), repos (text), redaction (toggle)
- Delete connector button (per connector)
- Tab selector: connectors/signals

**Live Data**:
- GET `/api/connectors` (list on mount, manual refresh)
- Signals list (from connectors)

**API Endpoints**:
- GET `/api/connectors` (list)
- POST `/api/connectors` (create)
- DELETE `/api/connectors/{id}` (delete)
- GET `/api/connectors/{id}/signals` (implied)

### 4D. Policy Console

**Entry**: `features/policy/components/PolicyConsole.tsx:48`

**Controls**:
- Toggle rule enable/disable (per rule) — local only, no backend
- Test action selector (dropdown)
- Confidence threshold slider (0-100)
- Run Test button

**Live Data**:
- Rules list: hardcoded DEFAULT_RULES (no backend)
- Evaluation logs: appended locally on test

**API Endpoints**: None (fully local evaluation)

---

## Surface 5: Onboarding (Day-to-Day User Surface)

### Entry Point
- **File**: `/client/src/features/onboarding/components/OnboardingProvider.tsx`
- **Mount**: Wrapped around App.tsx (in `/app/App.tsx:14`)
- **Status**: Disabled (commented out in lines 52-65)

### Components

| Path | Component | Role |
|------|-----------|------|
| features/onboarding/components/OnboardingProvider.tsx:27 | `OnboardingProvider` | Context provider; wraps app |
| features/onboarding/components/OnboardingOverlay.tsx:18 | `OnboardingOverlay` | Tooltip overlay with spotlight + step controls |
| features/onboarding/components/OnboardingEventHandler.tsx | `OnboardingEventHandler` | Auto-trigger onboarding flows on route changes (stub) |
| features/onboarding/hooks/useOnboarding.ts:9 | `useOnboarding()` | State management (flow, step, completion tracking) |

### Controls

| File:Line | Element | Setting/Action Key | Type | Persisted To | Server Endpoint |
|-----------|---------|-------------------|------|--------------|-----------------|
| features/onboarding/components/OnboardingOverlay.tsx:130-138 | Close/Skip button (X) | `onSkip()` | Button | N/A | N/A |
| features/onboarding/components/OnboardingOverlay.tsx:161-169 | Previous button | `onPrev()` | Button | N/A | N/A |
| features/onboarding/components/OnboardingOverlay.tsx:183-189 | Next/Finish button | `onNext()` | Button | N/A | N/A |
| features/onboarding/hooks/useOnboarding.ts:36-49 | Start flow | `startFlow(flow, forceRestart)` | Programmatic | localStorage: `onboarding.completedFlows` | N/A |
| features/onboarding/hooks/useOnboarding.ts:78-84 | Reset onboarding | `resetOnboarding()` | Programmatic | localStorage | N/A |

### Live Displays

| File:Line | Widget | Data Source | Cadence |
|-----------|--------|-------------|---------|
| features/onboarding/components/OnboardingOverlay.tsx:48-91 | Tooltip positioning + spotlight | `targetRect` (measured from DOM) | On step.target change (useEffect) |
| features/onboarding/components/OnboardingOverlay.tsx:147-156 | Step counter + progress dots | `currentStepIndex`, `currentFlow.steps.length` | On step change |

### Disconnects & Notes

- **Disabled by default**: Overlay and EventHandler are commented out in OnboardingProvider
- **Local storage only**: Completed flows stored in browser; no backend sync
- **No flows defined**: Default flows stub in `/features/onboarding/flows/defaultFlows.ts` (not fully explored)
- **Spotlight effect**: Implemented with CSS box-shadow + ring; no WASM

---

## Surface 6: Analytics & Semantic Controls

### Entry Point
- **Files**: 
  - `/features/analytics/components/SemanticClusteringControls.tsx`
  - `/features/analytics/components/SemanticAnalysisPanel.tsx`
  - `/features/analytics/components/ShortestPathControls.tsx`
- **Mount**: Embedded in graph visualization (visualisation feature)
- **Hook**: `useSemanticService` (features/analytics/hooks/useSemanticService.ts)

### Controls

| File:Line | Element | Setting/Action Key | Type | Persisted To | Server Endpoint |
|-----------|---------|-------------------|------|--------------|-----------------|
| features/analytics/components/SemanticClusteringControls.tsx | Run clustering button | Trigger `/api/analytics/clustering/run` | Button | N/A | POST `/api/analytics/clustering/run` |
| features/analytics/components/SemanticClusteringControls.tsx | Focus cluster button (per cluster) | `clusterId` selection | Button | N/A | POST `/api/analytics/clustering/focus` |
| features/analytics/components/SemanticClusteringControls.tsx | Toggle anomaly detection | `enabled` (boolean) | Toggle switch | N/A | POST `/api/analytics/anomaly/toggle` |
| features/analytics/hooks/useSemanticService.ts | Cache invalidation | Semantic cache reset | Button/function | N/A | POST `/api/semantic/cache/invalidate` |

### Live Displays

| File:Line | Widget | Data Source | Cadence |
|-----------|--------|-------------|---------|
| features/analytics/hooks/useSemanticService.ts:46 | Semantic statistics (node count, clustering state) | GET `/api/semantic/statistics` | On-demand (cache invalidation) |
| features/analytics/components/SemanticClusteringControls.tsx | Anomaly detection state | GET `/api/analytics/anomaly/current` | On-demand |

### API/WS Calls

| Caller File:Line | Method | Path | Description |
|------------------|--------|------|-------------|
| features/analytics/components/SemanticClusteringControls.tsx | POST | `/api/analytics/clustering/run` | Run semantic clustering |
| features/analytics/components/SemanticClusteringControls.tsx | POST | `/api/analytics/clustering/focus` | Focus on cluster |
| features/analytics/components/SemanticClusteringControls.tsx | POST | `/api/analytics/anomaly/toggle` | Enable/disable anomaly detection |
| features/analytics/components/SemanticClusteringControls.tsx | GET | `/api/analytics/anomaly/current` | Fetch current anomaly state |
| features/analytics/hooks/useSemanticService.ts | GET | `/api/semantic/statistics` | Fetch semantic stats |
| features/analytics/hooks/useSemanticService.ts | POST | `/api/semantic/cache/invalidate` | Invalidate semantic cache |

---

## Global API Client & Transport

### Unified API Client

**File**: `services/api/UnifiedApiClient.ts` (inferred from imports)

**Methods**: `get<T>()`, `post<T>()` (async)

**Used by**:
- Health service: `GET /health`, `GET /health/physics`, `POST /health/mcp/start`, `GET /health/mcp/logs`
- Analytics: `POST /api/analytics/clustering/*`, `GET /api/analytics/anomaly/*`, `POST /api/semantic/*`
- KPI dashboard: `GET /api/mesh-metrics`

### Legacy API Fetch

**File**: `utils/apiFetch.ts` (legacy)

**Methods**: `apiFetch<T>()`, `apiPost<T>()`

**Used by**:
- Workflows: `GET /api/workflows/proposals`, `GET /api/workflows/patterns`, `POST /api/workflows/proposals`
- Connectors: `GET /api/connectors`, `POST /api/connectors`, `DELETE /api/connectors/{id}`
- Broker: `POST /api/broker/cases`

### WebSocket

**Files**:
- `features/bots/hooks/useBotsWebSocketIntegration.ts`
- `features/ontology/hooks/useOntologyWebSocket.ts`
- `features/solid/hooks/useSolidPod.ts`
- `services/SolidPodService.ts` (Solid Pod WebSocket)

**Topics** (inferred):
- Studio workspace updates (`/api/ws/studio`)
- Sensei nudges (agent X1)
- Ontology graph updates
- Bots agent polling

---

## Command Registry & Palette Integration

**File**: `features/command-palette/CommandRegistry.ts`

**Registered commands** (from `useStudioCommands`):
1. `studio:open` → Navigate `/studio`
2. `studio:new-workspace` → Navigate `/studio/new`
3. `studio:switch-workspace` → Custom event dispatch
4. `studio:share-artifact` → Custom event dispatch
5. `studio:run-skill` → Custom event dispatch
6. `studio:new-automation` → Navigate `/studio/automations/new`
7. `studio:inbox` → Navigate `/studio/inbox`
8. `studio:nudge-accept` → Nudge handling (truncated)
9-15. Additional commands (not fully expanded in scope)

---

## Summary: Control Surface Inventory

### Three Primary Surfaces Identified

1. **Enterprise Control Panel**: Five-panel management UI (Broker, Workflows, KPIs, Connectors, Policy) with WASM-powered visual effects.

2. **Contributor Studio (Day-to-Day User)**: Four-pane collaborative workspace (OntologyGuideRail | WorkLane | AIPartnerLane + SessionMemoryBar) with 11 routes and command palette integration.

3. **Monitoring & Health**: Real-time system health dashboard with 5s polling, physics simulation status, and MCP relay controls.

### Secondary Surfaces

4. **Onboarding**: Spotlight-based tutorial system (currently disabled).
5. **Analytics**: Semantic clustering and anomaly detection controls (embedded in graph visualization).

### Key Characteristics

- **WASM eye-candy**: drawer_fx (flow-field particles) in enterprise drawer
- **Stub endpoints**: Workspace management, automation creation, broker case handling (pending agent implementations)
- **Persistent state**: `localStorage` for drawer section, layout, onboarding flows; Zustand stores for workspace/partner/inbox
- **Lazy loading**: Route-level code splitting; WASM module deferred until drawer open
- **Real-time**: WebSocket for bots, ontology, Solid Pod; polling for health/KPIs
- **Accessibility**: Focus traps, ARIA labels, `prefers-reduced-motion` support

### Known Disconnects

- AutomationCreateWizard, WorkspaceCreateWizard: Scaffolded, backend pending
- PolicyConsole: Local evaluation only, no backend persistence
- OnboardingOverlay: Disabled
- AIPartnerLane context panel: Awaiting Sensei bridge (agent X1)
- BrokerInbox/Timeline: Stub API calls

**Total documented controls**: 50+
**Total live displays**: 30+
**Total API endpoints referenced**: 40+
**WASM modules**: 1 (drawer_fx)
