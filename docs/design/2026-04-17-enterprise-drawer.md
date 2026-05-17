# Enterprise Drawer — Design Document

- **Date**: 2026-04-17
- **Author**: System Architecture (VisionFlow)
- **Status**: Proposed
- **Supersedes**: full-viewport `/enterprise` route (`App.tsx:182`, `EnterpriseFullPage.tsx`)
- **ADR reference**: see `§8 Decision Record` below

---

## 0. TL;DR

Replace the full-viewport enterprise route with a **right-anchored slide-out drawer** that extends the existing control centre rightward, covering ~60–70% of the viewport with a frosted-glass backdrop. Graph continues rendering underneath at reduced fidelity. URL contract shifts from `/#/enterprise` → `/?drawer=enterprise&section=broker`. A WASM/WebGPU canvas layer renders ambient eye-candy behind the glass, gated by `prefers-reduced-motion`.

---

## 1. Drawer Mechanics

### 1.1 Geometry & anchoring

```
Viewport (100vw)
+----------------------------+-----------------------------------------------+
|  Graph (interactive)       |          DRAWER (frosted glass)               |
|  30–40% exposed            |          60–70% overlay                       |
|  clicks pan/zoom           |          clicks inside = drawer-local         |
|                            |  ┌── EnterpriseNav (w-48, sticky) ──┐         |
|                            |  │ Broker                            │         |
|                            |  │ Workflows                         │ Panel  |
|                            |  │ KPIs        ← active              │ body   |
|                            |  │ Connectors                        │ scroll |
|                            |  │ Policy                            │        |
|                            |  └───────────────────────────────────┘         |
|   ControlPanel (left,      |                                               |
|   untouched, z=30)         |                                               |
+----------------------------+-----------------------------------------------+
 ^                            ^                                             ^
 0                          30vw (drawer left edge @ desktop)          100vw
```

### 1.2 Breakpoints (Tailwind)

| Breakpoint | Width class | Rationale |
|---|---|---|
| `<768px` (mobile) | `w-full` | Full cover; graph useless at this size |
| `md: 768–1279px` | `w-[75vw]` | Tablet — prefer more drawer, graph still peeks |
| `lg: 1280–1919px` | `w-[65vw] max-w-[1100px]` | Primary target |
| `xl:1920+` | `w-[60vw] max-w-[1400px]` | Ultra-wide — preserve graph real estate |

Anchor: `fixed inset-y-0 right-0 z-40`. Control panel remains at `z-30` (left), drawer above it; Command Palette stays at `z-50`.

### 1.3 Backdrop & glass

```tsx
// Drawer surface
className="
  fixed inset-y-0 right-0 z-40
  bg-background/70 backdrop-blur-xl backdrop-saturate-150
  border-l border-white/10
  shadow-[0_0_80px_-10px_rgba(0,0,0,0.6)]
  ring-1 ring-white/5
"

// Scrim (dims exposed graph slice to draw focus; also captures dismiss clicks)
className="
  fixed inset-0 z-[35]
  bg-gradient-to-l from-transparent via-transparent to-black/30
  pointer-events-auto
"
```

Exact tokens:
- Glass fill: `bg-background/70` (≈ `hsl(var(--background) / 0.70)`)
- Blur: `backdrop-blur-xl` (24px) + `backdrop-saturate-150`
- Edge: `border-l border-white/10` + inner `ring-1 ring-white/5`
- Inner shadow highlight: `before:absolute before:inset-x-0 before:top-0 before:h-px before:bg-gradient-to-r before:from-transparent before:via-white/20 before:to-transparent`

### 1.4 Motion (framer-motion)

Reuse the existing modal easing vocabulary (see `features/design-system` — spring-based, settle under 300ms):

```ts
const drawerSpring = {
  type: 'spring',
  stiffness: 320,
  damping: 36,
  mass: 0.9,
};

const drawerVariants = {
  closed: { x: '100%', opacity: 0.0, transition: drawerSpring },
  open:   { x: '0%',   opacity: 1.0, transition: drawerSpring },
};

const scrimVariants = {
  closed: { opacity: 0, transition: { duration: 0.18, ease: 'easeOut' } },
  open:   { opacity: 1, transition: { duration: 0.22, ease: 'easeOut' } },
};
```

Exit: reverse. `AnimatePresence mode="wait"`. Under `prefers-reduced-motion: reduce` replace spring with a 120ms `tween` (opacity + 8px translate).

### 1.5 Focus, keyboard, ARIA

- `role="dialog" aria-modal="true" aria-labelledby="drawer-title" aria-expanded` on trigger button.
- Focus trap via `@radix-ui/react-focus-scope` (already transitively available through existing Radix usage).
- On open: focus first nav item; restore focus to trigger on close.
- `Escape` closes drawer (unless a nested modal/combobox is open — check `document.activeElement` role).
- Tab cycles inside drawer; arrow keys move nav selection (`EnterpriseNav` updated with `role="tablist"`, items `role="tab"`).
- Announce section changes via a polite live region: `<span className="sr-only" aria-live="polite">{activeSection} active</span>`.

### 1.6 Overlay click behaviour

**Click on exposed graph area dismisses drawer** (opinionated default), because:
1. The graph is the app's primary artefact — users expect "click away = back to work".
2. Matches NodeDetailPanel conventions already in `MainLayout`.
3. Shift-click or drag on the scrim is treated as an intentional graph interaction (pan), not a dismiss — implement by only dismissing on `pointerup` with no drag delta (>5px threshold).

Opt-out: hold `Alt` while clicking scrim = keep drawer open (power-user gesture; documented in help overlay).

---

## 2. Routing & State

### 2.1 URL contract migration

Current (`App.tsx:182`):
```
/#/enterprise           → EnterpriseFullPage (replaces graph)
/#/enterprise/broker    → same
```

New:
```
/#/                           → Graph only
/#/?drawer=enterprise         → Graph + drawer (default section: broker)
/#/?drawer=enterprise&s=kpi   → Graph + drawer open on KPI
```

The existing hash router already parses `route.startsWith('/enterprise')`. To avoid regression:

1. Keep `/enterprise` recognised **but** redirect to new query form on mount (back-compat alias, removed in a later minor).
2. Parse search params from the hash portion (custom helper — standard URL cannot parse `#/path?query` natively; extract manually).

```ts
// hooks/useDrawerRoute.ts
export function useDrawerRoute() {
  const hash = useHashLocation(); // existing
  const [, qs] = hash.split('?');
  const params = new URLSearchParams(qs ?? '');
  return {
    drawer: params.get('drawer'),             // 'enterprise' | null
    section: params.get('s') ?? 'broker',
    open(section = 'broker') {
      window.location.hash = `/?drawer=enterprise&s=${section}`;
    },
    close() {
      window.location.hash = '/';
    },
  };
}
```

### 2.2 Zustand slice

```ts
// src/store/drawerStore.ts
interface DrawerState {
  isOpen: boolean;
  activeSection: 'broker' | 'workflows' | 'kpi' | 'connectors' | 'policy';
  // reduce-motion + perf knobs
  dimGraph: boolean;    // default true while drawer open
  pauseGraph: boolean;  // default false — see §3.3

  open: (section?: DrawerState['activeSection']) => void;
  close: () => void;
  setSection: (s: DrawerState['activeSection']) => void;
}
```

URL is the source of truth; Zustand mirrors it. Subscribe to `hashchange`; on change, sync store. On `open()/close()` actions, push to URL → listener updates store (single direction, no circular writes).

### 2.3 State machine

```
        ┌──────────────┐
        │   CLOSED     │◄───── ESC / scrim click / close()
        │ URL: /#/     │
        └──────┬───────┘
               │ open(section)
               ▼
        ┌──────────────┐
        │   OPENING    │ ←── framer spring in-flight
        │ 0→300ms      │
        └──────┬───────┘
               │ onAnimationComplete
               ▼
        ┌──────────────┐   setSection(s)
        │     OPEN     │ ─────────┐
        │  (interactive)│          │
        └──────┬───────┘ ◄────────┘
               │ close() / route change
               ▼
        ┌──────────────┐
        │   CLOSING    │ ← framer spring exit
        └──────┬───────┘
               ▼
          back to CLOSED
```

### 2.4 Back-button

- Opening the drawer uses `history.pushState` (hash change does so naturally) → browser back closes drawer and returns to `/`.
- Closing uses `history.back()` **only if** the previous entry is the pre-drawer state; otherwise fall back to `replaceState('/')`. Detect via a sentinel flag on `history.state`.

---

## 3. Visual Hierarchy

### 3.1 Triggers

1. **Navbar icon** — new icon button in `IntegratedControlPanel` header row, lucide `LayoutPanelRight`, label "Enterprise (⌘E)". Visible at all breakpoints.
2. **Keyboard shortcut** — `Cmd/Ctrl + E` global listener (registered in `App.tsx` alongside existing CommandPalette hotkey).
3. **Command palette** action — add `enterprise:open` command with sub-commands `enterprise:broker`, `enterprise:kpi`, etc., deep-linking to sections.
4. **Legacy `/enterprise` links** — intercepted and rewritten to drawer URL.

### 3.2 Internal layout

Preserve `EnterpriseNav` vertical rail (already `w-48`), but restyle for dark glass:

```
┌────────────────────────────────────────────────────────────┐
│ ◀ Drag handle (4px)  │  Section title      │    ✕ Close    │ ← header, 44px
├──────────┬─────────────────────────────────────────────────┤
│ EntNav   │                                                 │
│ rail     │       <BrokerWorkbench /> (or active section)   │
│ w-48     │       overflow-auto                             │
│          │                                                 │
│          │                                                 │
└──────────┴─────────────────────────────────────────────────┘
```

- Header has the drag handle (future: resize width; v1 = icon only), section title, and close button.
- `EnterpriseNav` keeps its existing component contract; only visual tokens change to glass-aware variants.
- Section bodies stay untouched (`BrokerWorkbench`, `WorkflowStudio`, `MeshKpiDashboard`, `ConnectorPanel`, `PolicyConsole`) — they already render inside a flex container and work at any width.

### 3.3 Keeping the graph alive

Three modes, user-switchable in `settings.visualisation.drawer`:

| Mode | Behaviour | Cost |
|---|---|---|
| `live` | Full physics + render | 100% |
| `dimmed` *(default)* | Full physics, renderer alpha 0.7, bloom off | ~85% |
| `slowed` | Physics tick rate halved (30→15 Hz) when drawer open | ~55% |
| `paused` | `graphDataManager` pause flag, last frame retained | ~5% (renders one static frame) |

Implement via a single `useDrawerStore` selector consumed by `GraphCanvasWrapper`. When drawer state is `OPEN`, it throttles the physics tick passed to the WASM sim. GPU work stays the same in `dimmed` because we want the frosted glass to refract real motion.

Performance floor: on devices where FPS dips below 45 for >2s after drawer open, auto-demote from `dimmed` → `slowed`. Hook into existing perf monitor.

---

## 4. WASM Rust Eye-Candy Catalogue

Goal: complement existing WebGPU materials (`GemRenderer`, `CrystalOrbMaterial`, `GlassEdgeMaterial`) with ambient motion behind the glass. Canvas sits between the scrim and the drawer: `fixed inset-y-0 right-0 w-[65vw] z-[36]`, `pointer-events-none`.

### 4.1 Available crates (wasm-bindgen)

| Crate | Role | Bundle impact |
|---|---|---|
| `wgpu` (already viable via WebGPU) | Compute + render shaders | ~400KB |
| `glam` | SIMD math | ~25KB |
| `lyon` | Vector tessellation (paths, bezier) | ~150KB |
| `rustfft` | Audio-reactive FFT | ~80KB (only if audio route on) |
| `rapier2d` | 2D physics | ~250KB |
| `resvg` | SVG rasterise | ~600KB (skip — too big) |
| `nannou` / `bevy_ecs` headless | Scene graph | too heavy — skip |

We already ship `scene-effects` WASM 0.1.0 — extend that crate rather than add a sibling. One wasm module, one load cost.

### 4.2 Proposed effects

Ranked `wow × cost⁻¹ × perf⁻¹`. Scores are 1–5.

| # | Effect | Wow | Impl cost | Perf overhead | Score | Recommend |
|---|---|---|---|---|---|---|
| 1 | **Liquid-data edge** — A 6-wide vertical strip at the drawer's left edge runs a wgpu compute shader simulating reaction-diffusion (Gray-Scott) seeded by velocities of the nearest graph nodes. Looks like data oozing from graph into drawer. | 5 | 3 | 2 | **8.3** | ✅ v1 |
| 2 | **Hologram scanline open** — On `OPENING`, a single vertical scan sweeps right→left in 240ms, leaving chromatic aberration + dither noise trail. Pure fragment shader, no state. | 5 | 1 | 1 | **25** | ✅ v1 (shipped with skeleton) |
| 3 | **Morse data streams** — Behind the content, soft vertical columns of on/off bits drift at 0.3 units/s, brightness gated by a 1D perlin noise sampled by `glam`. Each column width 24–36px, monochrome, multiply blended. | 4 | 2 | 2 | **5.0** | ✅ v2 |
| 4 | **Penrose substrate** — A `lyon`-tessellated Penrose P3 tiling rendered once, animated via a subtle parallax + hue-shift. Feels like an architectural blueprint. | 4 | 3 | 1 | **5.3** | ✅ v2 |
| 5 | **Cursor particle trails** — `rapier2d` with 200 soft bodies, gravity follows cursor when drawer open. Decay 1.2s. Applies caustic lighting via pre-baked lookup. | 4 | 4 | 3 | **1.7** | v3 polish |
| 6 | **Procedural caustics** — Full-rect shader, very pretty but competes with glass refraction → visually muddy. | 3 | 3 | 3 | **1.1** | ❌ skip |
| 7 | **Audio-reactive viz** (rustfft) — Only if mic permission granted; otherwise dormant. Cheap because only active on audio route. | 5 | 4 | 2 | **3.1** | ✅ v3 (opt-in) |

**v1 ship**: effects **#1 + #2**. Single wgpu pipeline, ~350 LOC Rust.

### 4.3 Integration surface

```rust
// scene-effects/src/drawer_ambient.rs
#[wasm_bindgen]
pub struct DrawerAmbient { /* wgpu device, pipelines */ }

#[wasm_bindgen]
impl DrawerAmbient {
  #[wasm_bindgen(constructor)]
  pub fn new(canvas: &HtmlCanvasElement) -> Result<DrawerAmbient, JsValue> { /* … */ }

  pub fn set_node_velocities(&mut self, xs: &Float32Array) { /* seed edge fluid */ }
  pub fn trigger_open_sweep(&mut self) {}
  pub fn trigger_close_sweep(&mut self) {}
  pub fn render(&mut self, dt: f32) {}
  pub fn set_quality(&mut self, tier: u8) {} // 0=off, 1=low, 2=high
  pub fn dispose(&mut self) {}
}
```

TS wrapper in `client/src/features/enterprise/wasm/drawerAmbient.ts`. RAF driven; disposes on drawer close.

### 4.4 Quality gating

```ts
const tier =
  matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 :
  navigator.hardwareConcurrency >= 8 ? 2 :
  1;
```

Tier 0 replaces the WASM canvas with a **static CSS gradient**:
```css
background:
  radial-gradient(120% 80% at 0% 50%, rgba(120,180,255,0.08), transparent 60%),
  linear-gradient(90deg, rgba(80,120,200,0.06), transparent 40%);
```

---

## 5. UX Consistency

### 5.1 Typography

Match `features/design-system`:
- Section title: `text-sm font-semibold tracking-tight`
- Nav labels: `text-sm font-medium`
- Body: default `text-sm text-foreground`
- Captions: `text-xs text-muted-foreground uppercase tracking-wider`

No new font faces. Same scale as NodeDetailPanel and IntegratedControlPanel.

### 5.2 Motion vocabulary

Reuse the spring constants from existing modals (NodeDetailPanel uses a similar stiffness 300/damping 30 spring — we bump to 320/36 for the larger mass). Exit eases must mirror enter inverted to prevent the "rubberband" feel.

### 5.3 Dark mode

Base assumption: **dark mode always on** (existing app default). Light-mode tokens exist in design-system but are not shipped to users; we still define them for completeness:

| Token | Dark | Light |
|---|---|---|
| Drawer fill | `bg-background/70` (≈ `#0a0f1ab3`) | `bg-white/75` |
| Border | `border-white/10` | `border-black/10` |
| Scrim gradient | `to-black/30` | `to-black/10` |

### 5.4 Accessibility matrix

| Concern | Mitigation |
|---|---|
| Motion sickness | `prefers-reduced-motion` → WASM tier 0, spring → tween, no scanline |
| Screen readers | `role="dialog"`, labelled by title, focus trap, announce section changes |
| Keyboard only | Trigger focusable, ESC close, arrow nav between sections, Tab cycles |
| High contrast | `prefers-contrast: more` → drop backdrop-blur, raise fill to `bg-background/92`, solid border |
| Colour blind | No colour-only affordances; icons + text labels on nav |

---

## 6. Implementation Plan

### 6.1 Milestones

| # | Milestone | Deliverable | Effort |
|---|---|---|---|
| M1 | Skeleton drawer | Motion + focus trap + URL sync, empty body | 1d |
| M2 | Mount existing `EnterprisePanel` inside | Nav + all five sections working | 0.5d |
| M3 | Routing migration + legacy redirect | `/enterprise` → `/?drawer=…`, back-button safe | 0.5d |
| M4 | Graph performance modes | `dimmed/slowed/paused` wired to store + settings | 0.5d |
| M5 | WASM ambient canvas (v1: effects #1 + #2) | `scene-effects` Rust additions, TS wrapper | 2d |
| M6 | Command palette + hotkey + navbar trigger | Deep-link actions, keyboard | 0.5d |
| M7 | A11y + reduced-motion audit | axe clean, reduced-motion path | 0.5d |
| M8 | Polish (v2 effects #3, #4) | Morse + Penrose layers, gated by tier | 1d |

Total: ~6.5 engineering days for v1 + v2.

### 6.2 Files to create

```
client/src/features/enterprise/
  components/
    EnterpriseDrawer.tsx              ← NEW — shell, motion, a11y
    EnterpriseDrawerHeader.tsx        ← NEW — title + close + (future) resize
    EnterpriseDrawerTrigger.tsx       ← NEW — navbar icon button
  hooks/
    useDrawerRoute.ts                 ← NEW — URL parsing + push
    useFocusRestore.ts                ← NEW — focus trap helper
  wasm/
    drawerAmbient.ts                  ← NEW — TS wrapper over scene-effects
    scene-effects/src/drawer_ambient.rs  ← NEW — Rust ambient canvas

client/src/store/
  drawerStore.ts                      ← NEW — Zustand slice

client/src/features/enterprise/styles/
  drawer.css                          ← NEW — reduced-motion + fallback gradient
```

### 6.3 Files to edit

```
client/src/app/App.tsx                ← drop /enterprise branch → redirect + mount <EnterpriseDrawer />
client/src/app/MainLayout.tsx         ← mount <EnterpriseDrawer /> globally, alongside NodeDetailPanel
client/src/features/visualisation/components/IntegratedControlPanel.tsx
                                      ← add <EnterpriseDrawerTrigger />
client/src/features/enterprise/components/EnterprisePanel.tsx
                                      ← accept activeSection prop (drive from store, remove local state)
client/src/features/enterprise/components/EnterpriseNav.tsx
                                      ← role=tablist + keyboard arrow nav
client/src/features/enterprise/components/EnterpriseFullPage.tsx
                                      ← DELETE after M3 + legacy redirect in place
client/src/enterprise-standalone.tsx  ← KEEP (separate entry); update internal routing note
client/src/features/visualisation/components/CommandPalette.tsx
                                      ← register enterprise:* actions
```

### 6.4 New dependencies

None required for v1/v2. All of `framer-motion`, Radix focus-scope, Zustand, Tailwind are present. WASM Rust additions live in existing `scene-effects` crate — no new Cargo.toml.

Optional v3: `rapier2d-wasm` if effect #5 ships.

### 6.5 Rollback strategy

1. Feature-flag behind `settings.experiments.enterpriseDrawer` (default **on** post-QA, default **off** pre-QA).
2. Flag off → `EnterpriseDrawerTrigger` unmounts; `/enterprise` URL falls back to old `EnterpriseFullPage` (kept in tree until post-release +1 minor).
3. `EnterpriseFullPage.tsx` deletion deferred to cycle +1 after drawer lands.
4. WASM ambient is independently flagged (`settings.experiments.drawerAmbient`) — can be killed without removing drawer itself.
5. Bisect anchor: single commit introduces `EnterpriseDrawer.tsx` + routing redirect; clean revert is `git revert <sha>`.

---

## 7. Non-Functional Requirements

| Attribute | Target | Verification |
|---|---|---|
| TTI impact | < 20ms added to initial render | Lighthouse profile |
| Open animation | First paint ≤ 32ms (2 frames) | Chrome Perf panel |
| FPS floor w/ drawer open | ≥ 45 FPS on M1 / Ryzen 5 laptop iGPU | `StatsPanel` capture |
| WASM bundle delta | ≤ +120KB gzipped for v1 effects | bundle analyser |
| Accessibility | axe-core 0 violations on drawer | CI gate |
| Browser matrix | Chromium 120+, Firefox 121+, Safari 17+ | manual smoke |

---

## 8. Decision Record (ADR-inline)

**Context**: The current `/enterprise` route displaces the graph entirely, breaking the "graph is always the ground truth" mental model. Users report loss of context when toggling between the graph and enterprise views.

**Decision**: Convert `/enterprise` from a route-level full-page to a right-anchored drawer overlay with frosted-glass backdrop, URL contract `?drawer=enterprise&s=<section>`.

**Alternatives considered**:
1. *Keep full page, add back-button shortcut* — does not solve context loss.
2. *Left-anchored drawer next to ControlPanel* — collides with existing `IntegratedControlPanel` and pushes the graph out of frame.
3. *Bottom sheet* — poor fit for tabular enterprise data (KPI tables, policy lists).
4. *Separate window via `window.open`* — loses keyboard + URL integration; not WebXR-friendly.

**Consequences**:
- `+` Graph stays visible; spatial continuity preserved.
- `+` Enterprise sections deep-link-friendly.
- `+` WASM ambient effects add product polish commensurate with WebGPU gem work.
- `−` Smaller working area for enterprise tables on tablets.
- `−` New state machine surface (focus, routing, URL, Zustand) to maintain.
- `−` Additional WASM payload (bounded by feature flag).

**Mitigations**: feature flag, tier-gated WASM, virtualised tables in section bodies (future), clear rollback path (§6.5).

---

*End of document.*
