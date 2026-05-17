# ADR-047: WASM Visualization Components

## Status

Implemented 2026-04-20

## Date

2026-04-14 (proposed) / 2026-04-20 (Sensei + Decision Canvas + Studio work-lane thumbnails implemented)

## Implementation Notes (2026-04-20)

- The `scene-effects` crate (v0.2.0) was extended in-place rather than forking a
  new crate. Two new WASM classes (`OntologyNeighborThumb`, `MiniGraph`) and two
  stateless one-shot functions (`renderOntologyNeighborThumb`, `renderMiniGraph`)
  were added via `client/crates/scene-effects/src/mini_graph.rs`.
- Bridge extended in `client/src/wasm/scene-effects-bridge.ts` with
  `OntologyNeighborThumbBridge` and `MiniGraphBridge`, plus one-shot helpers on
  `SceneEffectsAPI`. The same zero-copy `Float32Array` / `Uint8Array` pattern as
  `ParticleFieldBridge` / `AtmosphereFieldBridge` is reused (pointer + length
  with `byteOffset` into linear memory; bounds-checked view construction).
- React wrappers: `client/src/features/design-system/components/WasmMiniGraph.tsx`
  and `client/src/features/design-system/components/OntologyNeighborThumb.tsx`.
  Both ship a Canvas2D baseline that renders synchronously on mount and
  upgrade to WASM once `initSceneEffects()` resolves. `forceFallback` prop
  disables the WASM path for tests / degraded environments.
- Stride-7 node buffer `[x, y, r, g, b, a, weight]` is the shared interchange
  format, exported as `MINI_GRAPH_NODE_STRIDE` from the bridge.
- Consumers:
  - Sensei nudge thumbnails → `OntologyNeighborThumb`
  - Decision Canvas skill preview cards (broker workbench, ADR-041) → `WasmMiniGraph`
  - `/studio/:workspaceId` embedded work-lane mini-graph (BC18) → `WasmMiniGraph`
- Build: `wasm-pack build --target web --out-dir ../../../client/src/wasm/scene-effects`
  (executed from `client/crates/scene-effects/`). Final wasm binary is ~50 KB,
  comfortably inside the ADR's 50 KB-per-module target.
- 9 Rust unit tests pass (`cargo test --lib` inside the standalone crate).
  Typecheck clean for all files touched by this change.

## Context

PRD-002 specifies five enterprise control plane surfaces that include data-intensive visualisation components: KPI sparklines with real-time updates, workflow DAG layouts, and broker case timelines with heatmap overlays. The existing `Sparkline` component (`client/src/features/design-system/components/Sparkline.tsx`) uses Canvas2D with an animated draw-on effect (ease-out-cubic, 800ms duration) and handles 30-point sparklines well. Enterprise dashboards will encounter larger datasets (100-10,000+ data points for historical trends, drill-downs, and lineage views) and sustained real-time update rates (KPI snapshots arriving every 15 seconds, broker inbox updates every few seconds).

### Current WASM Infrastructure

The platform has an established WASM integration pattern in `client/src/wasm/scene-effects-bridge.ts` that provides:

1. **Module-level singleton** with state machine init guard (`idle` -> `loading` -> `ready` | `failed`)
2. **Typed bridge classes** (`ParticleFieldBridge`, `AtmosphereFieldBridge`, `WispFieldBridge`) wrapping raw WASM handles with TypeScript-friendly APIs
3. **Zero-copy memory views** using `Float32Array` / `Uint8Array` over `WebAssembly.Memory`
4. **Graceful degradation**: callers handle the `initSceneEffects()` promise rejection and fall back to non-WASM rendering
5. **Dispose pattern**: bridge objects expose `dispose()` for explicit WASM resource cleanup with `isDisposed` guard against use-after-free
6. **Bounds checking**: all pointer+length dereferences are validated against `memory.buffer.byteLength`
7. **Retry backoff**: failed init caches the rejected promise for 1 second to prevent thundering-herd retries

The scene-effects WASM module is compiled from Rust via `wasm-pack` and lives in `client/src/wasm/scene-effects/`. It provides `ParticleField`, `AtmosphereField`, and `EnergyWisps` -- real-time simulation workloads where WASM acceleration is justified (thousands of particles updated per frame).

### Canvas2D Performance Profile

Benchmarks of the existing `Sparkline` component's `draw` function:

| Data Points | Canvas2D Time | Frame Budget (16ms) | Assessment |
|-------------|--------------|---------------------|------------|
| 30 | ~0.3ms | 53x headroom | Excellent |
| 100 | ~0.8ms | 20x headroom | Excellent |
| 500 | ~2.5ms | 6x headroom | Good |
| 1,000 | ~4ms | 4x headroom | Acceptable |
| 5,000 | ~15ms | At budget | Risky on low-end |
| 10,000 | ~25ms | Exceeds budget | Guaranteed jank |

The KPI drill-down view (PRD-002 Section 8.4) specifies 100+ data points for historical trends with zoom/pan. The lineage table may reference thousands of source events. Workflow DAG layouts with 20+ steps require graph layout algorithms (topological sort, layer assignment, edge routing) that scale quadratically in naive implementations.

### Build Infrastructure

The Rust toolchain is already configured for WASM compilation. `wasm-pack` is used to build the `scene-effects` module. The Vite build includes WASM file handling. The existing `client/src/wasm/scene-effects/` directory contains the wasm-pack output (`.wasm`, `.js` glue, `.d.ts` types, `package.json`). No additional build infrastructure is needed; new modules follow the same convention.

## Decision Drivers

- The TypeScript Canvas2D baseline must be the primary renderer; WASM is progressive enhancement
- Users on browsers without WASM support (rare but present in some enterprise lockdown environments where CSPs block `WebAssembly.instantiate`) must see functioning visualisations
- WASM module loading adds latency to the first render (50-200ms); this must not block initial page display
- The existing `scene-effects-bridge.ts` pattern is proven and understood by the team
- New WASM modules must be independently compilable and testable (no monolithic WASM binary)
- The WASM acceleration threshold must be data-driven, not arbitrary
- User preference for Rust WASM interfaces with performant, subtle graphics

## Considered Options

### Option 1: Canvas2D first, WASM acceleration via bridge pattern as progressive enhancement (chosen)

Build all enterprise visualisation components with a TypeScript Canvas2D (or SVG) baseline renderer. Define a renderer interface per component. Implement WASM renderers that conform to the same interface. The component detects WASM availability at mount time and upgrades if the module loads successfully and data volume exceeds the acceleration threshold.

- **Pros**: Guaranteed rendering on all browsers. No blocking on WASM load. TypeScript baseline is debuggable with standard browser dev tools. WASM acceleration is opt-in per data volume. Follows the established `scene-effects-bridge.ts` pattern. Each WASM module is independent.
- **Cons**: Dual renderer maintenance (TypeScript + WASM) per accelerated component. The interface boundary adds abstraction overhead.

### Option 2: WASM-only rendering for all enterprise visualisations

Build all sparklines, DAGs, and timelines exclusively in WASM (Rust compiled to WebAssembly). No TypeScript fallback.

- **Pros**: Single codebase (Rust). Maximum performance. No dual-renderer maintenance.
- **Cons**: WASM is required; no fallback for CSP-restricted environments. WASM module loading blocks first paint (50-200ms init violates the 150ms page transition target from PRD-002). Debugging WASM is harder than Canvas2D. Iteration speed is slower (compile step). Blocks enterprise UI shipping until WASM modules are compiled and tested.

### Option 3: No WASM; optimise Canvas2D only

Keep all rendering in TypeScript Canvas2D. Optimise with pre-computed paths, offscreen canvas, and web workers for layout computation.

- **Pros**: Simplest architecture. No WASM build complexity. Single renderer per component.
- **Cons**: Canvas2D optimisations have diminishing returns beyond ~1,000 data points. Web workers add message-passing overhead. The KPI drill-down at 10,000 data points will jank. Workflow DAG layout in JavaScript is significantly slower than Rust for graphs with 50+ nodes. Does not leverage the existing WASM infrastructure.

### Option 4: WebGL for all enterprise visualisations (via Three.js or standalone)

Render enterprise visualisations as WebGL scenes.

- **Pros**: GPU-accelerated rendering. Handles large datasets well.
- **Cons**: R3F and Three.js are loaded only on the graph route (300KB gzipped); pulling them into enterprise routes defeats the code-splitting strategy. WebGL is overkill for 2D sparklines. The setup/teardown overhead of a WebGL context for a 120x40px sparkline is unjustified. Accessibility is worse (canvas-only, no DOM nodes for ARIA).

## Decision

**Option 1: Canvas2D first for all enterprise visualisations. WASM acceleration via the existing bridge pattern as progressive enhancement when data volume exceeds the Canvas2D performance ceiling.**

### Architecture

```
Enterprise Component (React)
    |
    +-- Renderer Interface (abstract)
    |     |
    |     +-- TypeScriptRenderer (Canvas2D / SVG)   <-- ships in Phases 1-3
    |     |
    |     +-- WasmRenderer (bridge to WASM module)  <-- ships in Phase 4
    |
    +-- useRenderer hook:
    |     1. Instantiate TypeScript renderer immediately
    |     2. If data.length >= WASM_THRESHOLD:
    |          a. Attempt async WASM module load
    |          b. If success: swap to WASM renderer
    |          c. If failure: continue with TypeScript renderer
    |     3. On unmount: dispose() both renderers
```

### Renderer Interface Pattern

Each visualisation component defines a renderer interface. The TypeScript and WASM renderers both implement it:

```typescript
// Sparkline renderer interface
export interface SparklineRenderInput {
  values: Float32Array;                // Time-series values (y-axis)
  timestamps: Float64Array;            // Epoch ms (x-axis), for time-aware rendering
  confidenceUpper: Float32Array;       // Upper confidence bound per point
  confidenceLower: Float32Array;       // Lower confidence bound per point
  width: number;                       // Canvas width in CSS pixels
  height: number;                      // Canvas height in CSS pixels
  dpr: number;                         // devicePixelRatio for crisp rendering
  hue: number;                         // HSL hue for primary line (0-360)
}

export interface SparklineRenderer {
  render(canvas: HTMLCanvasElement, input: SparklineRenderInput): void;
  dispose(): void;
}
```

The `Float32Array` and `Float64Array` input types enable zero-copy transfer to WASM linear memory. The TypeScript renderer accepts them identically (they are standard typed arrays).

### TypeScript Baseline Renderers

Ship with PRD-002 Phases 1-3. These are the primary renderers and must be production-quality, not stubs:

**SparklineCanvasRenderer** (extracted from existing `Sparkline` component):
- Renders to a 2D Canvas context with DPR-aware sizing
- Line drawing with `ctx.lineTo()` for the value series
- Gradient fill area for confidence bands (`ctx.globalAlpha` for translucency)
- Gradient stroke using HSL hue from the VisionClaw crystalline palette (cyan-to-violet)
- Animated draw-on with ease-out-cubic (800ms), matching existing `Sparkline` behaviour
- Glow endpoint dot with alpha halo
- Performance: < 2ms for 1,000 points on modern hardware

**DagSvgRenderer**:
- Renders workflow steps as positioned `<div>` elements (for ARIA accessibility) with SVG `<path>` connections
- Layout computed by a TypeScript implementation of simple top-down DAG algorithm (topological sort, layer assignment, fixed vertical spacing, horizontal centering within layers)
- Conditional steps show branching paths with bezier curve diverge/converge lines
- Animated via Framer Motion on the positioned elements
- Performance: < 5ms layout for 50 nodes

**TimelineCanvasRenderer**:
- Renders a 2D heatmap grid to Canvas
- Cell colour derived from decision density (count) and outcome distribution, using the VisionClaw crystalline colour ramp
- Time axis (horizontal), category axis (vertical)
- Performance: < 3ms for a 365 x 10 grid

### WASM Modules (Phase 4 Progressive Enhancement)

Three Rust crates compiled independently via `wasm-pack --target web`:

#### 1. `kpi-sparklines`

**Purpose**: Accelerated sparkline rendering for KPI dashboard and drill-down views.

**Rust crate location**: `crates/kpi-sparklines/`

**Key capabilities beyond the TypeScript baseline**:
- Cubic spline interpolation for smooth curves at any zoom level
- SIMD-friendly inner loops (auto-vectorised by LLVM for `wasm32` target)
- Pre-allocated pixel buffer avoids GC pressure on every re-render
- Confidence band rendering with Gaussian-kernel smoothing

**Acceleration threshold**: `data.length > 500`. Below this, the Canvas2D renderer is indistinguishable in performance and visual output.

```rust
#[wasm_bindgen]
pub struct SparklineEngine {
    width: u32,
    height: u32,
    dpr: f32,
    pixel_buffer: Vec<u8>,       // RGBA output
    interpolated: Vec<f32>,      // Interpolated y-values at pixel x-coords
}

#[wasm_bindgen]
impl SparklineEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32, dpr: f32) -> Self { /* ... */ }

    pub fn compute(
        &mut self,
        values_ptr: *const f32,
        values_len: usize,
        conf_upper_ptr: *const f32,
        conf_lower_ptr: *const f32,
        hue: f32,
    ) { /* ... */ }

    pub fn get_pixels_ptr(&self) -> *const u8 { self.pixel_buffer.as_ptr() }
    pub fn get_pixels_len(&self) -> usize { self.pixel_buffer.len() }
}
```

**Bridge**: `client/src/wasm/kpi-sparklines-bridge.ts`

```typescript
export class SparklineEngineBridge {
  private inner: WasmSparklineEngine;
  private memory: WebAssembly.Memory;
  private _disposed = false;

  constructor(inner: WasmSparklineEngine, memory: WebAssembly.Memory) {
    this.inner = inner;
    this.memory = memory;
  }

  get isDisposed(): boolean { return this._disposed; }

  compute(
    values: Float32Array,
    confidenceUpper: Float32Array,
    confidenceLower: Float32Array,
    hue: number,
  ): void {
    if (this._disposed) return;
    this.inner.compute(
      values.byteOffset, values.length,
      confidenceUpper.byteOffset,
      confidenceLower.byteOffset,
      hue,
    );
  }

  getPixels(): Uint8Array {
    if (this._disposed) return new Uint8Array(0);
    const ptr = this.inner.get_pixels_ptr();
    const len = this.inner.get_pixels_len();
    if (ptr + len > this.memory.buffer.byteLength) {
      throw new Error('WASM pointer out of bounds');
    }
    return new Uint8Array(this.memory.buffer, ptr, len);
  }

  dispose(): void {
    if (this._disposed) return;
    this._disposed = true;
    this.inner.free();
  }
}

let cachedAPI: KpiSparklineAPI | null = null;
let initPromise: Promise<KpiSparklineAPI> | null = null;
let initState: 'idle' | 'loading' | 'ready' | 'failed' = 'idle';

export async function initKpiSparklines(): Promise<KpiSparklineAPI> {
  // Identical singleton + state machine pattern to initSceneEffects()
}
```

#### 2. `workflow-dag`

**Purpose**: Accelerated directed acyclic graph layout for workflow step visualisation.

**Rust crate location**: `crates/workflow-dag/`

**Key capabilities**:
- Layer-constrained force-directed layout (Sugiyama initial placement + spring model refinement)
- Convergence detection (velocity magnitude below threshold)
- Incremental re-layout when steps are added/removed (re-layout affected layers only, not full restart)
- Edge routing with bend minimisation

**Acceleration threshold**: `graph.nodes > 15`. Below this, the TypeScript topological sort + fixed-spacing layout is trivial and sufficient.

```rust
#[wasm_bindgen]
pub struct DagLayout {
    node_count: usize,
    positions: Vec<f32>,         // [x, y, x, y, ...] per node
    edges: Vec<u32>,             // [from_idx, to_idx, ...]
    velocities: Vec<f32>,
    iteration: u32,
}

#[wasm_bindgen]
impl DagLayout {
    #[wasm_bindgen(constructor)]
    pub fn new(node_count: usize) -> Self { /* ... */ }

    pub fn set_edges(&mut self, edges_ptr: *const u32, edges_len: usize) { /* ... */ }
    pub fn step(&mut self, iterations: u32, dt: f32) { /* ... */ }
    pub fn is_converged(&self) -> bool { /* ... */ }

    pub fn get_positions_ptr(&self) -> *const f32 { self.positions.as_ptr() }
    pub fn get_positions_len(&self) -> usize { self.positions.len() }
}
```

**Bridge**: `client/src/wasm/workflow-dag-bridge.ts`

#### 3. `broker-heatmap` (stretch goal)

**Purpose**: Heatmap overlay for broker timeline and case distribution visualisation.

**Rust crate location**: `crates/broker-heatmap/`

**Key capabilities**:
- 2D density computation from time-series event data (timestamp x category)
- RGBA pixel buffer output with configurable HSL colour ramp (VisionClaw crystalline palette)
- Gaussian-smoothed cell transitions (bioluminescent glow between cells)
- Viewport-aware rendering (zoom/pan support via window parameters)

**Acceleration threshold**: `events > 1,000`. Below this, the Canvas2D cell renderer is sufficient.

```rust
#[wasm_bindgen]
pub struct TimelineHeatmap {
    cols: u32,
    rows: u32,
    pixel_buffer: Vec<u8>,
    cell_values: Vec<f32>,
}

#[wasm_bindgen]
impl TimelineHeatmap {
    #[wasm_bindgen(constructor)]
    pub fn new(cols: u32, rows: u32, cell_width: u32, cell_height: u32) -> Self { /* ... */ }

    pub fn set_values(&mut self, values_ptr: *const f32, values_len: usize) { /* ... */ }
    pub fn render(&mut self, hue_start: f32, hue_end: f32) { /* ... */ }

    pub fn get_pixels_ptr(&self) -> *const u8 { self.pixel_buffer.as_ptr() }
    pub fn get_pixels_len(&self) -> usize { self.pixel_buffer.len() }
}
```

**Bridge**: `client/src/wasm/broker-heatmap-bridge.ts`

### Acceleration Thresholds Summary

| Component | WASM Module | Threshold | Rationale |
|-----------|-------------|-----------|-----------|
| Sparkline | `kpi-sparklines` | data.length > 500 | Canvas2D handles 500 in ~2.5ms; WASM provides cubic interpolation headroom |
| DAG Layout | `workflow-dag` | nodes > 15 | Topological sort is O(V+E) and trivial for small graphs; Sugiyama is worth it at 15+ |
| Heatmap | `broker-heatmap` | events > 1,000 | Canvas2D cell rendering is fast for sparse data; density computation is expensive at scale |

Thresholds are configurable via component props and tunable from enterprise pilot performance data.

### Component Integration

The existing `Sparkline` design system component gains an optional threshold prop:

```typescript
interface SparklineProps {
  data: number[];
  width?: number;
  height?: number;
  color?: string;
  fillColor?: string;
  strokeWidth?: number;
  animated?: boolean;
  className?: string;
  wasmThreshold?: number;     // default: 500; set to Infinity to disable WASM
}
```

The component internally uses a `useSparklineRenderer` hook:

```typescript
export function useSparklineRenderer(dataLength: number, threshold: number): SparklineRenderer {
  const [renderer, setRenderer] = useState<SparklineRenderer>(
    () => new SparklineCanvasRenderer()  // immediate, synchronous
  );

  useEffect(() => {
    if (dataLength < threshold) return;

    let cancelled = false;
    initKpiSparklines()
      .then(api => {
        if (!cancelled) {
          setRenderer(prev => {
            prev.dispose();
            return api.createRenderer();
          });
        }
      })
      .catch(() => {
        // WASM unavailable; Canvas2D renderer continues
      });

    return () => { cancelled = true; };
  }, [dataLength >= threshold]);  // Only re-trigger when crossing threshold

  useEffect(() => {
    return () => renderer.dispose();
  }, [renderer]);

  return renderer;
}
```

This ensures:
- The component renders immediately with the Canvas2D renderer (no loading state)
- WASM is attempted only when data volume crosses the threshold
- If WASM loads, subsequent renders use WASM transparently
- If WASM fails, the Canvas2D renderer continues indefinitely
- Both renderers are cleaned up on unmount

### File Structure

```
client/src/wasm/
  scene-effects/                 # Existing (unchanged)
    scene_effects.d.ts
    scene_effects.js
    scene_effects_bg.wasm
    scene_effects_bg.wasm.d.ts
    package.json
  scene-effects-bridge.ts        # Existing (unchanged)

  drawer-fx/                     # Existing — Enterprise drawer ambient effect (shipped Apr 2026)
    src/lib.rs                   #   Flow-field particle effect for drawer canvas layer
    Cargo.toml                   #   Standalone crate; wasm-opt = false (binaryen validation workaround)
    build.sh                     #   wasm-pack build script
    README.md                    #   Crate-level documentation
  useDrawerFx.ts                 # React hook mounting drawer-fx canvas

  kpi-sparklines/                # New WASM build output (Phase 4)
    kpi_sparklines.d.ts
    kpi_sparklines.js
    kpi_sparklines_bg.wasm
    kpi_sparklines_bg.wasm.d.ts
    package.json
  kpi-sparklines-bridge.ts       # New bridge

  workflow-dag/                  # New WASM build output (Phase 4)
    workflow_dag.d.ts
    workflow_dag.js
    workflow_dag_bg.wasm
    workflow_dag_bg.wasm.d.ts
    package.json
  workflow-dag-bridge.ts         # New bridge

  broker-heatmap/                # New WASM build output (Phase 4, stretch)
    broker_heatmap.d.ts
    broker_heatmap.js
    broker_heatmap_bg.wasm
    broker_heatmap_bg.wasm.d.ts
    package.json
  broker-heatmap-bridge.ts       # New bridge

crates/                          # Rust source (project root)
  kpi-sparklines/
    Cargo.toml
    src/lib.rs
  workflow-dag/
    Cargo.toml
    src/lib.rs
  broker-heatmap/
    Cargo.toml
    src/lib.rs
  viz-common/                    # Shared Rust utilities (colour ramps, interpolation)
    Cargo.toml
    src/lib.rs
```

### Note on `drawer-fx`

`drawer-fx` was shipped as a standalone crate in April 2026 rather than extending `scene-effects` as originally specified in the enterprise drawer design document (`docs/design/2026-04-17-enterprise-drawer.md` §7, which proposed `scene-effects/src/drawer_ambient.rs`). The decision to keep it standalone was made to avoid destabilising the production `scene-effects` crate and to keep the ambient effect independently versioned. ADR-046 governs the broader enterprise UI architecture within which `drawer-fx` sits.

### Cargo Configuration

Each WASM crate uses minimal dependencies for small binary size:

```toml
# crates/kpi-sparklines/Cargo.toml
[package]
name = "kpi-sparklines"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wasm-bindgen = "0.2"
viz-common = { path = "../viz-common" }

[profile.release]
opt-level = "s"       # Optimise for size
lto = true
codegen-units = 1
strip = true
```

No `serde`, `web-sys`, or `js-sys` unless specifically needed. Rendering logic is pure computation; I/O is handled by the TypeScript bridge.

### Build Commands

```bash
# Build individual module
cd crates/kpi-sparklines && wasm-pack build --target web --out-dir ../../client/src/wasm/kpi-sparklines/

# Build all WASM modules (npm script)
npm run wasm:build

# Build only sparklines
npm run wasm:build:sparklines
```

WASM modules are pre-built artifacts:
- **Development**: built once, committed to repo. TypeScript fallback is the primary dev renderer.
- **CI**: GitHub Actions runs `wasm:build` before `npm run build`.
- **Production**: Vite includes pre-built `.wasm` files as static assets with correct MIME type.

The WASM build step is explicitly separated from the TypeScript build to avoid requiring `wasm-pack` and the Rust toolchain for frontend developers.

### Performance Targets

| Module | Operation | Target | Measurement |
|--------|-----------|--------|-------------|
| `kpi-sparklines` | Render 1,000 points | < 2ms | `performance.now()` around render call |
| `kpi-sparklines` | Render 10,000 points | < 8ms | Same |
| `workflow-dag` | Layout 50 nodes | < 5ms | Same |
| `workflow-dag` | Layout 200 nodes | < 20ms | Same |
| `broker-heatmap` | Compute 5,000 events | < 10ms | Same |
| All modules | Init (first load) | < 100ms | Module instantiation time |
| All modules | WASM binary size | < 50KB gzipped each | Build output measurement |

### Testing Strategy

1. **Rust unit tests**: Standard `#[cfg(test)]` tests for computation logic (layout algorithms, normalisation, colour ramp). Run with `cargo test` (native, not WASM).

2. **WASM integration tests**: `wasm-pack test --headless --firefox` per crate, verifying WASM entry points produce correct outputs when called from JavaScript.

3. **Component tests**: Vitest + Testing Library tests verify:
   - TypeScript renderer produces correct output at various data sizes
   - Component falls back gracefully when WASM fails to load
   - WASM renderer is activated when data crosses threshold
   - `dispose()` is called on unmount (no memory leaks)

4. **Performance benchmarks**: CI benchmarks measure render time at 100, 1K, and 10K data points for both renderers, fail if WASM exceeds target.

5. **Visual regression**: Playwright screenshot comparison ensures WASM and TypeScript renderers produce visually identical output.

## Consequences

### Positive

- All enterprise visualisations work on all browsers immediately (Canvas2D baseline ships in Phases 1-3)
- WASM acceleration is transparent to component consumers; they pass data, the component chooses the renderer
- The bridge pattern is established and proven (`scene-effects-bridge.ts`); new modules follow the same template
- Each WASM module is independently compilable, testable, and deployable
- Zero-copy data transfer minimises overhead at the TypeScript-WASM boundary
- Threshold-based activation means WASM is only loaded when it provides measurable benefit
- WASM modules do not affect initial page load; they are loaded asynchronously after the component mounts and data exceeds threshold
- Shared `viz-common` crate prevents colour palette and interpolation math duplication

### Negative

- Dual renderer maintenance: TypeScript and WASM per accelerated component. Mitigation: TypeScript renderers are simple (100-200 lines each); visual regression tests catch drift between the two.
- Three additional WASM crates add build pipeline complexity. Mitigation: each crate is small (<500 lines Rust), builds in seconds, cached by cargo. CI builds in parallel.
- WASM binary size adds to total download. Estimated: 30-50KB per module gzipped. Mitigation: lazy-loaded only when threshold is exceeded. Users viewing 30-point sparklines never download the WASM module.
- `broker-heatmap` is a stretch goal and may not ship in Phase 4. Mitigation: explicitly labelled as stretch; the broker timeline works with the Canvas2D fallback.
- The WASM modules may not provide perceptible performance improvement for small datasets (30-point sparklines). The justification is headroom for future complexity (cubic interpolation, Gaussian smoothing, animated transitions), not current-scale performance.

### Neutral

- The existing `scene-effects` WASM module and bridge are unchanged
- The existing `Sparkline` component's public API is backward-compatible; `wasmThreshold` is optional with a default that preserves current behaviour (all existing uses have <500 data points)
- The Vite build configuration does not need changes; it already handles `.wasm` files
- The WASM crate workspace does not affect the main `webxr` server crate compilation
- The 3D rendering pipeline (R3F, Three.js, post-processing) is unaffected

## Related Decisions

- ADR-046: Enterprise UI Architecture (defines the feature modules that consume WASM-accelerated components)
- ADR-043: KPI Lineage Model (defines the data model behind sparkline time series)
- ADR-041: Judgment Broker Workbench (defines the broker timeline and decision canvas that use DAG and timeline visualisations)
- ADR-042: Workflow Proposal Object Model (defines the workflow step model that the DAG visualises)
- ADR-013: Render Performance (established performance budgeting approach for the client)
- PRD-002: Enterprise Control Plane UI (product requirements for visualisation fidelity and performance)

## References

- `client/src/wasm/scene-effects-bridge.ts` (canonical WASM bridge pattern -- the template for all new bridges)
- `client/src/wasm/scene-effects/` (existing WASM build output directory structure)
- `client/src/features/design-system/components/Sparkline.tsx` (Canvas2D baseline implementation)
- `client/src/features/design-system/animations.ts` (Framer Motion presets used alongside WASM renderers)
- wasm-pack documentation: https://rustwasm.github.io/wasm-pack/
- wasm-bindgen documentation: https://rustwasm.github.io/wasm-bindgen/
- Vite WASM integration: https://vite.dev/guide/features#webassembly
- Sugiyama DAG layout: K. Sugiyama, S. Tagawa, M. Toda, IEEE Transactions on SMC, 1981
