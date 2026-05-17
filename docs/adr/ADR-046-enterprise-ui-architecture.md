# ADR-046: Enterprise UI Architecture

## Status

Accepted

## Date

2026-04-14

## Context

PRD-002 specifies five enterprise control plane surfaces (Broker Workbench, Workflow Studio, Mesh KPI Dashboard, Connector Management, Policy Console) that consume backend APIs defined in ADR-041 through ADR-045. The client must evolve from a single-view 3D graph visualisation into a multi-view enterprise application without degrading the existing experience.

### Current Client Architecture

- **Single route**: `MainLayout.tsx` renders a full-screen `GraphCanvasWrapper` with an `IntegratedControlPanel` overlay. There is no router.
- **State**: A single `settingsStore.ts` (Zustand + Immer) with subscriber trie for path-based reactive updates. A `websocketStore.ts` for WebSocket connection lifecycle.
- **Design system**: 25+ components in `client/src/features/design-system/components/` built on Radix UI v3 primitives (`@radix-ui/themes` 3.2.1) with Tailwind CSS v4 HSL custom property tokens. Animation presets in `design-system/animations.ts` using Framer Motion. Recently added: `StatusDot`, `Sparkline`, `EmptyState`, `Timeline`, `DataTable`.
- **Feature modules**: `client/src/features/{analytics,bots,command-palette,graph,help,monitoring,onboarding,ontology,physics,settings,solid,visualisation,workspace}/` -- each with `components/`, `hooks/`, optional `store/`, and `index.ts`.
- **WASM**: `client/src/wasm/scene-effects-bridge.ts` provides typed wrappers over scene-effects WASM module with zero-copy Float32Array views. Module-level singleton with state machine init guard.
- **Icons**: Lucide React across all existing panels.
- **3D**: React Three Fiber 9.5.0 + Three.js 0.183.0 with post-processing.
- **Command palette**: `client/src/features/command-palette/` with `CommandRegistry`, `Command` interface, and existing keyboard shortcut infrastructure.
- **No React Router**: navigation is tab-based within control panels. No URL-based routing exists.

### Key Constraints

- Enterprise surfaces are data-dense tabular/form views, architecturally distinct from the 3D graph visualisation
- The 3D canvas is computationally expensive (GPU, WASM particle fields); it must not initialise on enterprise routes
- Backend enterprise endpoints are already live and returning data
- The design system is established and must be extended, not replaced
- Enterprise users need real-time updates for the Broker Inbox and KPI Dashboard
- Code splitting is mandatory to keep initial bundle size under control
- The existing `settingsStore` pattern (Zustand + Immer + subscriber trie) should be the model for new stores

## Decision Drivers

- Preserve the existing graph visualisation experience unchanged
- Enable URL-based deep linking to enterprise views (required for command palette, cross-surface navigation, and browser history)
- Keep enterprise feature bundles lazy-loaded so the graph-only experience does not pay for enterprise code
- Use the existing design system and extend it rather than introducing alternative component libraries
- Each enterprise domain (broker, workflows, KPI, connectors, policy) should be independently developable and testable
- Real-time updates for time-sensitive surfaces must integrate with the existing WebSocket infrastructure
- The navigation model must accommodate future surfaces without structural changes

## Considered Options

### Option 1: React Router with lazy-loaded feature modules and per-domain Zustand stores (chosen)

Add `react-router-dom` v7. Define a root layout with a persistent `SidebarNav` component. The graph visualisation becomes the index route. Enterprise surfaces are peer routes, each lazy-loaded via `React.lazy()`. Each enterprise domain gets its own Zustand store. WebSocket subscriptions are managed per-surface via mount/unmount hooks.

- **Pros**: URL-based deep linking. Code splitting at route boundaries. Independent feature modules. Familiar React Router patterns. Browser history navigation. Each domain store is independently testable.
- **Cons**: Adds a dependency (`react-router-dom`). Requires restructuring `App.tsx` and `MainLayout.tsx` to participate in a router layout hierarchy. Initial migration effort.

### Option 2: Extend the existing tab-based panel system

Add enterprise surfaces as additional tabs within the existing `IntegratedControlPanel`. No router. Navigation via tab selection stored in a Zustand slice.

- **Pros**: No new dependency. Consistent with the current side-panel UX.
- **Cons**: The side panel is 320-400px wide; enterprise surfaces need full-width layouts (tables, multi-column views). No URL deep linking. No code splitting at view boundaries. The `IntegratedControlPanel` would become monolithic. Tabs do not compose well with multi-level navigation (e.g., Broker Inbox -> Decision Canvas -> case detail). Browser back button does not work.

### Option 3: Micro-frontend architecture

Each enterprise surface is a separate micro-frontend (Vite Module Federation or single-spa). Each surface has its own build pipeline and can be deployed independently.

- **Pros**: Maximum isolation. Independent deployment.
- **Cons**: Massive overhead for a single-team project. Shared state (auth, WebSocket, design tokens) requires a complex contract layer. Module federation adds build complexity. Design system consistency is harder to enforce across micro-frontends. The application is developed by one team; the isolation benefits do not justify the coordination costs.

### Option 4: Next.js or Remix migration

Migrate from Vite SPA to a full-stack React framework with file-system routing and SSR.

- **Pros**: Built-in routing. SSR for faster initial loads. Server components for data fetching.
- **Cons**: Enormous migration scope. The existing Vite config, WASM integration, and R3F setup would need to be ported. The backend is Rust/actix-web, not Node.js; SSR adds no value. The client is a thick SPA by design (3D visualisation, WASM, WebSocket). Server components are incompatible with Three.js rendering.

## Decision

**Option 1: React Router v7 with lazy-loaded feature modules and per-domain Zustand stores.**

### Router Structure

```typescript
// client/src/app/router.tsx
import { createBrowserRouter, RouterProvider } from 'react-router-dom';
import { lazy, Suspense } from 'react';
import { RootLayout } from './RootLayout';
import { MainLayout } from './MainLayout';
import { LoadingScreen } from '../components/LoadingScreen';

const BrokerWorkbench = lazy(() => import('../features/broker'));
const BrokerCaseDetail = lazy(() => import('../features/broker/components/DecisionCanvas'));
const WorkflowStudio = lazy(() => import('../features/workflows'));
const WorkflowDetail = lazy(() => import('../features/workflows/components/ProposalDetail'));
const KpiDashboard = lazy(() => import('../features/kpi'));
const KpiDrilldown = lazy(() => import('../features/kpi/components/KpiDrilldown'));
const ConnectorManagement = lazy(() => import('../features/connectors'));
const ConnectorWizard = lazy(() => import('../features/connectors/components/ConnectorWizard'));
const PolicyConsole = lazy(() => import('../features/policy'));
const PolicyRuleEditor = lazy(() => import('../features/policy/components/PolicyRuleEditor'));

const router = createBrowserRouter([
  {
    element: <RootLayout />,          // SidebarNav + Outlet
    children: [
      {
        index: true,
        element: <MainLayout />,       // Existing graph visualisation
      },
      {
        path: 'broker',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <BrokerWorkbench />
          </Suspense>
        ),
      },
      {
        path: 'broker/:caseId',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <BrokerCaseDetail />
          </Suspense>
        ),
      },
      {
        path: 'workflows',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <WorkflowStudio />
          </Suspense>
        ),
      },
      {
        path: 'workflows/:proposalId',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <WorkflowDetail />
          </Suspense>
        ),
      },
      {
        path: 'kpi',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <KpiDashboard />
          </Suspense>
        ),
      },
      {
        path: 'kpi/:metric',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <KpiDrilldown />
          </Suspense>
        ),
      },
      {
        path: 'connectors',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <ConnectorManagement />
          </Suspense>
        ),
      },
      {
        path: 'connectors/new',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <ConnectorWizard />
          </Suspense>
        ),
      },
      {
        path: 'connectors/:id/config',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <ConnectorWizard />
          </Suspense>
        ),
      },
      {
        path: 'policy',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <PolicyConsole />
          </Suspense>
        ),
      },
      {
        path: 'policy/:ruleId',
        element: (
          <Suspense fallback={<LoadingScreen />}>
            <PolicyRuleEditor />
          </Suspense>
        ),
      },
    ],
  },
]);
```

### RootLayout Component

A new `RootLayout` replaces the current top-level render in `App.tsx`:

```typescript
// client/src/app/RootLayout.tsx
import { Outlet } from 'react-router-dom';
import { SidebarNav } from '../features/design-system/components/SidebarNav';

export const RootLayout: React.FC = () => {
  return (
    <div className="flex h-screen w-screen bg-[#000022]">
      <SidebarNav />
      <main className="flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
};
```

The existing `MainLayout` (graph canvas + integrated control panel) becomes the index route child. It is unchanged except for removal of the outermost viewport container (which moves to `RootLayout`).

### SidebarNav Component

Added to `client/src/features/design-system/components/SidebarNav.tsx`:

- 56px wide (collapsed), 200px (expanded on hover or pinned)
- Fixed position, full viewport height
- Navigation items: icon + optional label, active state indicated by `--accent` highlight
- Badge component instances for live counts (open broker cases, pending reviews, connector errors)
- Badge data sourced from lightweight Zustand selectors subscribed to the relevant stores
- Keyboard accessible: `Tab` to focus, `Enter` to navigate, `ArrowUp/Down` to move between items
- Collapse/expand toggle at the bottom, pinned state persisted to `localStorage`
- Uses Framer Motion `variants.slideLeft` for expand/collapse animation

### Feature Module Convention

Each enterprise feature module follows a consistent structure:

```
client/src/features/{domain}/
  components/           # React components, one per file
  hooks/                # Custom hooks for data fetching, subscriptions, commands
  store/                # Zustand store definition
  types/                # TypeScript interfaces and types
  index.ts              # Default export: route-level component (for React.lazy)
```

The `index.ts` default export is the route-level component. This is what `React.lazy()` imports. Internal components are not default-exported; they are imported by the route component.

### Per-Domain Zustand Stores

Each enterprise domain gets its own store created with `zustand/create` + Immer `produce()`:

```
client/src/features/broker/store/brokerStore.ts
client/src/features/workflows/store/workflowStore.ts
client/src/features/kpi/store/kpiStore.ts
client/src/features/connectors/store/connectorStore.ts
client/src/features/policy/store/policyStore.ts
```

Store design principles:
- **No cross-store imports**: stores do not import or subscribe to each other. Cross-surface navigation uses the router, not store coupling.
- **API calls inside store actions**: fetch logic lives in store actions (async thunks within the store), not in components. Components call `store.fetchInbox()`, not `fetch('/api/broker/inbox')`.
- **Optimistic updates**: mutations (claim case, toggle policy) are applied optimistically via `produce()`, with rollback on API error.
- **Selector granularity**: components subscribe to specific slices via Zustand's selector pattern to minimise re-renders.
- **No persistence**: enterprise stores are not persisted to localStorage (unlike `settingsStore` which uses `zustand/persist`). Enterprise data is server-authoritative.

### WebSocket Subscription Management

The existing `websocketStore.ts` manages the WebSocket connection lifecycle. Enterprise surfaces subscribe to additional channels via a hook pattern:

```typescript
// client/src/features/broker/hooks/useBrokerWebSocket.ts
export function useBrokerWebSocket() {
  const subscribe = useWebSocketStore(state => state.subscribe);
  const unsubscribe = useWebSocketStore(state => state.unsubscribe);
  const dispatch = useBrokerStore(state => state.dispatch);

  useEffect(() => {
    const channels = ['broker:inbox'];
    subscribe({ type: 'broker:subscribe', channels });

    const handler = (message: BrokerWsMessage) => {
      switch (message.type) {
        case 'broker:new_case':
          dispatch({ type: 'CASE_ADDED', payload: message.case });
          break;
        case 'broker:case_claimed':
          dispatch({ type: 'CASE_CLAIMED', payload: message.caseId });
          break;
        case 'broker:case_updated':
          dispatch({ type: 'CASE_UPDATED', payload: message.case });
          break;
        case 'broker:priority_changed':
          dispatch({ type: 'PRIORITY_CHANGED', payload: message });
          break;
      }
    };

    const unregister = onWebSocketMessage('broker:*', handler);

    return () => {
      unsubscribe({ type: 'broker:unsubscribe', channels });
      unregister();
    };
  }, [subscribe, unsubscribe, dispatch]);
}
```

This pattern ensures:
- Subscriptions are created on mount and cleaned up on unmount
- No WebSocket traffic for surfaces the user is not viewing
- Multiple subscribers to the same channel share the underlying subscription (reference counted in `websocketStore`)

### Panel Registration System

The `SidebarNav` uses a declarative registration pattern. Each feature module registers its navigation entry:

```typescript
// client/src/features/broker/index.ts
import { registerNavEntry } from '../design-system/components/SidebarNav';
import { Scale } from 'lucide-react';

registerNavEntry({
  id: 'broker',
  label: 'Broker',
  icon: Scale,
  path: '/broker',
  order: 20,
  badge: () => useBrokerStore(state => state.inbox.cases.filter(c => c.status === 'open').length),
  requiredRole: 'broker',   // ADR-040 role gating
});
```

Registration happens at module load time (side effect of importing the module). The `SidebarNav` component reads the registry and renders entries sorted by `order`. Entries with `requiredRole` are hidden if the current user lacks the role.

Built-in entries (Graph, Settings) are registered in `App.tsx` with lower order values to appear first.

### Command Palette Integration

Each feature module registers commands with the existing `CommandRegistry`:

```typescript
// client/src/features/broker/hooks/useBrokerCommands.ts
export function useBrokerCommands() {
  const registry = useCommandRegistry();
  const navigate = useNavigate();

  useEffect(() => {
    const commands: Command[] = [
      {
        id: 'broker:inbox',
        title: 'Go to Broker Inbox',
        category: 'Navigation',
        keywords: ['broker', 'inbox', 'cases', 'escalation'],
        icon: Scale,
        handler: () => navigate('/broker'),
      },
      {
        id: 'broker:timeline',
        title: 'Go to Broker Timeline',
        category: 'Navigation',
        keywords: ['broker', 'timeline', 'history', 'decisions'],
        handler: () => navigate('/broker?tab=timeline'),
      },
      {
        id: 'broker:submit',
        title: 'Submit New Case',
        category: 'Actions',
        keywords: ['broker', 'submit', 'case', 'new', 'create'],
        handler: () => navigate('/broker?action=submit'),
      },
    ];

    commands.forEach(cmd => registry.register(cmd));
    return () => commands.forEach(cmd => registry.unregister(cmd.id));
  }, [registry, navigate]);
}
```

### Design System Extensions

New components are added to the existing design system directory, not to feature modules:

| Component | File | Purpose |
|-----------|------|---------|
| `DataTable` | `design-system/components/DataTable.tsx` | Sortable, filterable, keyboard-navigable table built on HTML `<table>` with Radix primitives for sort controls and filter popovers. Already implemented. |
| `StatusDot` | `design-system/components/StatusDot.tsx` | Colour-coded dot + label with 5 variants (active/warning/error/inactive/processing) and CSS glow effects. Already implemented. |
| `Sparkline` | `design-system/components/Sparkline.tsx` | Canvas2D sparkline with animated draw-on, gradient fill, glow endpoint, DPR-aware rendering. TypeScript baseline; accepts optional WASM bridge for accelerated rendering (ADR-047). Already implemented. |
| `EmptyState` | `design-system/components/EmptyState.tsx` | Centred icon + message + optional CTA button. Three size variants (sm/md/lg). Already implemented. |
| `Timeline` | `design-system/components/Timeline.tsx` | Vertical timeline with status-coloured dots, connector lines, and metadata badges. Already implemented. |
| `StepIndicator` | `design-system/components/StepIndicator.tsx` | Horizontal pipeline progress bar with stage labels and active stage pulse animation. To be added. |
| `FilterBar` | `design-system/components/FilterBar.tsx` | Collapsible container for filter controls, wraps existing `Collapsible`. To be added. |
| `SidebarNav` | `design-system/components/SidebarNav.tsx` | Left sidebar with registration pattern and collapse/expand. To be added. |
| `KeyboardShortcutHint` | `design-system/components/KeyboardShortcutHint.tsx` | Inline `<kbd>` styled badge for displaying keyboard shortcuts. To be added. |

All components are exported from the design system `index.ts` barrel and available to all feature modules.

### Route-Level Code Splitting

Vite's built-in code splitting via dynamic `import()` (triggered by `React.lazy()`) creates separate chunks:

```
dist/
  assets/
    index-[hash].js          # Core: React, Zustand, Router, design system, SidebarNav
    graph-[hash].js           # Graph: Three.js, R3F, WASM scene-effects, MainLayout
    broker-[hash].js          # Broker Workbench
    workflows-[hash].js       # Workflow Studio
    kpi-[hash].js             # KPI Dashboard
    connectors-[hash].js      # Connector Management
    policy-[hash].js          # Policy Console
```

The `graph` chunk is only loaded on the index route. Enterprise chunks are loaded on-demand. The core bundle contains shared infrastructure (React, Zustand, Router, design system components, WebSocket, auth).

Target: core bundle < 120KB gzipped; each enterprise chunk < 40KB gzipped; graph chunk < 300KB gzipped (Three.js is large).

### Type Safety

Backend types generated by `specta` (via `cargo run --bin generate_types`) are extended to include enterprise entity types:

```
client/src/types/generated/
  BrokerCase.ts
  BrokerDecision.ts
  WorkflowProposal.ts
  WorkflowVersion.ts
  WorkflowPattern.ts
  OrganisationalMetricSnapshot.ts
  ConnectorSource.ts
  PolicyResult.ts
  PolicyEvaluated.ts
```

Feature module `types/` directories re-export and extend generated types with client-specific additions (e.g., `InboxFilters`, `SortState`, UI-only fields like `isHighlighted`).

### Migration Path from Current Architecture

The migration is additive:

1. **Add `react-router-dom`** to `package.json`
2. **Create `RootLayout.tsx`** and `router.tsx`
3. **Wrap `App.tsx`** root render with `<RouterProvider router={router} />`
4. **Move `MainLayout.tsx`** to be the index route child (remove its outermost viewport container; that moves to `RootLayout`)
5. **Add `SidebarNav`** to `RootLayout`. Register Graph and Settings as built-in entries.
6. **Create enterprise feature module directories** (empty initially)
7. **Implement feature modules** one at a time (Broker first, per PRD-002 rollout plan)

Steps 1-5 can be done in a single PR. The existing graph experience is completely unchanged after this migration; the sidebar is the only visible addition.

## Consequences

### Positive

- URL-based deep linking enables browser history, bookmarks, and shareable links to specific broker cases or KPI views
- Lazy loading keeps the graph-only experience fast; enterprise code is not loaded until needed
- Per-domain stores are independently testable and do not create coupling between enterprise surfaces
- The design system is extended, not forked; new components benefit all surfaces
- The sidebar registration pattern allows future surfaces to be added by creating a feature module and registering a nav entry
- Command palette integration gives power users fast navigation without touching the mouse
- WebSocket subscription management per-surface prevents unnecessary network traffic

### Negative

- Adding `react-router-dom` introduces a new dependency (~14KB gzipped). Mitigation: well-maintained, widely-used library with no known security issues. The cost is small relative to Three.js (~150KB gzipped).
- `MainLayout` must be refactored to participate in a router layout hierarchy. Mitigation: the refactoring is mechanical (extract viewport container, become a route child) and does not change any rendering logic.
- Five new Zustand stores add memory overhead (five store instances, five subscription sets). Mitigation: stores are lazy (created when the feature module loads) and contain only data fetched on that route. Empty stores consume negligible memory.
- The sidebar reduces the available width for the graph canvas by 56px. Mitigation: the graph canvas is already responsive; 56px is less than the width of the existing `IntegratedControlPanel`.

### Neutral

- Existing feature modules (`analytics`, `bots`, `graph`, `settings`, etc.) are not modified
- The existing `settingsStore`, `websocketStore`, and all existing hooks continue to work unchanged
- The `IntegratedControlPanel` remains the control surface for the graph visualisation route
- The WASM scene-effects bridge is not modified (enterprise WASM modules are separate, per ADR-047)

## Related Decisions

- ADR-040: Enterprise Identity Strategy (role gating for sidebar entries and route guards)
- ADR-041: Judgment Broker Workbench (backend APIs consumed by Broker Workbench surface)
- ADR-042: Workflow Proposal Object Model (backend APIs consumed by Workflow Studio surface)
- ADR-043: KPI Lineage Model (backend APIs consumed by KPI Dashboard surface)
- ADR-044: Connector Governance (backend APIs consumed by Connector Management surface)
- ADR-045: Policy Engine Approach (backend APIs consumed by Policy Console surface)
- ADR-047: WASM Visualization Components (progressive enhancement for sparklines, DAG, timeline)
- ADR-012: WebSocket Store Decomposition (existing WebSocket infrastructure extended with new channels)
- ADR-013: Render Performance (graph canvas performance preserved by not rendering on enterprise routes)
- PRD-002: Enterprise Control Plane UI (product requirements this ADR serves)

## References

- `client/src/app/MainLayout.tsx` (current single-route layout)
- `client/src/app/App.tsx` (current root component)
- `client/src/store/settingsStore.ts` (Zustand + Immer store pattern)
- `client/src/store/websocketStore.ts` (WebSocket connection lifecycle)
- `client/src/features/design-system/components/` (existing component library)
- `client/src/features/design-system/components/index.ts` (barrel export)
- `client/src/features/design-system/animations.ts` (Framer Motion presets)
- `client/src/features/command-palette/types.ts` (Command interface)
- `client/src/features/settings/components/panels/` (existing control panel pattern)
- `client/src/wasm/scene-effects-bridge.ts` (WASM bridge pattern reused by ADR-047)
- React Router v7 documentation: https://reactrouter.com/
