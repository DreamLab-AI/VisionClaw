# WORKTREE PLAN — Phase 6: Rendering Layer

Branch  : impl/phase-6-rendering (off radical-rollback @ d260a6158)
Author  : anthropic@xrsystems.uk
Date    : 2026-05-16
Related : PRD-04, ADR-04, ADR-03 (data sources)

Depends on Phase 4 (client state) delivering `nodePositionsRef` — a
`SharedArrayBuffer`-backed `Float32Array` view of node positions. The
renderer is a pure reader of that ref; it never writes positions.

---

## 1. Task Breakdown

Tasks are numbered T1–T8 and map directly to ADR-04 decisions (D1–D10)
and PRD-04 acceptance criteria (A1–A10). Each task lists the files it
touches, the acceptance criteria it closes, and the agent responsible.

### T1 — Dynamic edge capacity (replaces MAX_EDGES constant)

**ADR decisions**: D1, D3, D4
**PRD criteria**: A2, A3
**Files**: `client/src/features/graph/components/GlassEdges.tsx`
**Agent**: coder

Remove the top-level `const MAX_EDGES = 10_000` constant. Introduce a
runtime capacity managed by a `useRef`:

- `EDGE_INITIAL = 1024` — first allocation on non-empty `points` prop
- Growth factor: `current_capacity * 2` on each reallocation event
- `EDGE_CEILING` — read from `settings.rendering.maxEdgesCeiling`
  (default `32_768`; see T6 for settings wiring). The PRD lists a
  default of `64_000`; this plan aligns with the PRD. Implementation
  uses `64_000` in the settings default, `32_768` is the per-session
  working value only when a graph has not yet exceeded initial capacity.
- At first non-empty `points` prop: size to
  `min(ceilToPowerOfTwo(edgeCount * 1.25), ceiling)`.
- On any subsequent `points` prop where `edgeCount > capacity`: reallocate
  to `min(max(edgeCount, capacity * 2), ceiling)` — one event per growth
  step. Dispose previous mesh geometry, material, `instanceColor`.
- When `edgeCount > ceiling` after ceiling is hit: draw `ceiling` edges,
  emit one structured `console.warn` naming the unrendered count. Never
  silent truncation.
- The `instanceColor` `Float32Array` is sized to `capacity * 3` at each
  allocation and pre-filled with `1.0` (neutral white multiply). On
  capacity growth, copy prior colour values before replacing the attribute.
- `capacityRef` and `meshRef` are refs; `useMemo` is keyed only to `[]`
  so initial mount creates the mesh once. Growth happens inside
  `updatePoints` / the `points` prop effect — not inside `useMemo`.
- `EDGE_REVEAL_BATCH` formula is unchanged: `max(1, round(revealBatch * 0.67))`.
  Reveal still controls `mesh.count` independent of capacity (D3).

Acceptance test (A2): Chrome DevTools Memory tab: single allocation spike
on graph load, none during a 60-second pan.
Acceptance test (A3): `grep -rn 'MAX_EDGES' client/src/` returns zero hits.

---

### T2 — Edge surface-to-surface offset formula

**ADR decisions**: D2
**PRD criteria**: A10
**Files**: `client/src/features/graph/components/GlassEdges.tsx`
**Agent**: coder

The current `computeInstanceMatrices` places the cylinder midpoint at the
arithmetic midpoint of src→tgt centres and scales Y by full `len`. It does
not subtract node radii, so cylinders visually penetrate geometry.

Replace with the ADR-04 D2 composition:

```
srcR = computeNodeScale(srcNode, ...) * nodeSize
tgtR = computeNodeScale(tgtNode, ...) * nodeSize
adjLen = max(0, len - srcR - tgtR)
midpoint = src + dir * (srcR + adjLen * 0.5)
scale = vec3(1, adjLen, 1)
```

When `adjLen <= 0` (overlapping nodes): `makeScale(0, 0, 0)` — collapsed,
not drawn. Increment a per-frame `collapsedEdgeCount` metric logged once
at info level on first occurrence.

Node radius values by class (for the offset computation):
- GemNodes (knowledge): `r = 0.5` → `computeNodeScale * nodeSize` ≥ 0.5
- CrystalOrb (ontology): `r = 0.5` → same formula
- AgentCapsule (agents): approximate by `r = 0.3`; non-spherical
  envelope accepted as per PRD F9 / ADR R5

`computeNodeScale` and `nodeSize` are already threaded into `GlassEdges`
via the `settings` prop. The caller (`GraphManager.tsx`) must also supply
the per-node type info so `computeNodeScale` can be called with the right
arguments. If per-node access is not available at the call site, fall back
to `nodeSize * 0.5` as a conservative constant.

---

### T3 — Software-WebGL environment fallback

**ADR decisions**: D5
**PRD criteria**: A8
**Files**: `client/src/features/graph/components/GraphCanvas.tsx`
**Agent**: coder

`GraphCanvas` currently mounts `<Environment resolution={256}>` unconditionally.

Add a one-time detector in the `onCreated` callback:

```ts
const detectSoftwareRenderer = (gl: THREE.WebGLRenderer): boolean => {
  try {
    const ext = gl.getContext().getExtension('WEBGL_debug_renderer_info');
    if (!ext) return false; // unknown — default to hardware path
    const renderer = gl.getContext().getParameter(ext.UNMASKED_RENDERER_WEBGL) as string;
    const lower = renderer.toLowerCase();
    return (
      lower.includes('swiftshader') ||
      lower.includes('llvmpipe') ||
      lower.includes('software') ||
      lower.includes('microsoft basic render driver')
    );
  } catch {
    return false;
  }
};
```

Store the result in `useState<boolean | null>` (null = not yet detected).
Log the renderer string and decision once at `info` level:

```
[GraphCanvas] WebGL renderer: "SwiftShader Device ..." → software path: static-cube fallback
```

Branch inside the Canvas JSX:

- **Hardware or unknown** (`isSoftware === false || isSoftware === null`):
  render `<Environment background={false} resolution={256}>` with the
  existing `<color attach="background" args={['#111']} />` child.
- **Software + setting `static-cube`** (default): use
  `CubeTextureLoader` to load a 6×64×64 baked cube from
  `public/env/cube_64/` and assign to `scene.environment` inside a
  `useEffect`. No `<Environment>` component; no per-frame PMREM cost.
- **Software + setting `off`**: assign `scene.environment = null`;
  `GemNodeMaterial` and `CrystalOrbMaterial` must tolerate null env
  (set `envMapIntensity = 0`, `transmission = 0`, `roughness >= 0.4`
  in a dedicated preset applied at material creation).

The baked cube is a build-time asset. A 6×64×64 equirectangular PNG
strip is committed under `client/public/env/cube_64/` (six faces:
`px.jpg`, `nx.jpg`, `py.jpg`, `ny.jpg`, `pz.jpg`, `nz.jpg`) and
bundled by Vite's static asset pipeline.

Setting key `rendering.softwareFallback`: `"static-cube" | "off"`.
Default `"static-cube"`. Surfaced in the control panel in Phase 7 (T6).

Acceptance test (A8): Run against `agentbox` headless (software WebGL).
Page must reach steady-state render within 5 seconds of mount. The
console must contain exactly one log line with the renderer string and
the chosen fallback.

---

### T4 — Statically-typed InstancedLabels wrapper props

**ADR decisions**: D6, D7
**PRD criteria**: A6, A7
**Files**: `client/src/features/graph/components/InstancedLabels.tsx`
**Agent**: coder

The existing `InstancedLabels.tsx` already defines `InstancedLabelsProps`
(line 105) and the `InstancedLabelsWebGL` component. The issues:

1. The named-object forwarding from `InstancedLabels` to `InstancedLabelsWebGL`
   passes props directly (`<InstancedLabelsWebGL {...props} />` would be the
   dangerous form). Verify the current forwarding uses a named typed object.
   If not, convert:

   ```tsx
   const childProps: InstancedLabelsProps = {
     nodes,
     nodeIdToIndexMap,
     nodePositionsRef,        // must be named explicitly
     labelPositionsRef,
     settings,
     graphMode,
     perNodeVisualModeMap,
     connectionCountMap,
     hierarchyMap,
     graphTypeVisuals,
     ssspResult,
     isXRMode,
   };
   return <InstancedLabelsWebGL {...childProps} />;
   ```

2. `InstancedLabelsWebGL` must declare its prop type as:

   ```tsx
   type Props = InstancedLabelsProps;
   const InstancedLabelsWebGL: React.FC<Props> = ({ ... }) => { ... };
   ```

   This makes any future divergence between parent and child prop surfaces
   a TypeScript compile error, not a silent runtime regression.

3. The runtime guard in the renderer (already present as a fallback to
   `labelPositionsRef` when `nodePositionsRef` is absent) must emit a
   single `console.warn` on first fallback:

   ```ts
   if (!rawPositions && !diagLoggedRef.current) {
     console.warn('[InstancedLabelsWebGL] nodePositionsRef absent — falling back to labelPositionsRef. This is a bug upstream.');
     diagLoggedRef.current = true;
   }
   ```

4. The `frameCountRef` cadence magic-`3` is already present. Expose as a
   setting key `rendering.labelLayoutEvery` (default `3`) per ADR §Bugs.
   Read from `settings` inside the component; no behaviour change at default.

Acceptance test (A7): Remove `nodePositionsRef` from a `<InstancedLabels>`
call site, run `tsc --noEmit`. Must fail with a type error on that line.

---

### T5 — Zero-allocation rendering rule + lint enforcement

**ADR decisions**: D10
**PRD criteria closes**: A1, A6, A9 (partially)
**Files**: ESLint config (`client/.eslintrc.cjs` or `client/eslint.config.js`),
           `client/src/features/graph/components/GlassEdges.tsx`,
           `client/src/features/graph/components/InstancedLabels.tsx`,
           `client/src/features/visualisation/components/WasmSceneEffects.tsx`
**Agent**: code-reviewer + coder

The baseline already pre-allocates:
- `GlassEdges`: module-scope `tmpMat`, `tmpPos`, `tmpSrc`, `tmpTgt`,
  `tmpQuat`, `tmpDir`, `tmpScale` — correct.
- `InstancedLabels`: module-scope `_tempVec3`, `_tempColor`, `_frustum`,
  `_projScreenMatrix` — correct.
- `WasmSceneEffects`: module-scope `_tempAtmDir`, `_tmpMat4`, `_tmpPos`,
  `_tmpScale`, `_tmpColor`, `_identityQuat`, `_tmpHsl` — correct.

Bridge gap: `scene-effects-bridge.ts` creates a fresh `Float32Array` view
in every `getPositions()` / `getOpacities()` / `getSizes()` / `getHues()`
call. These views are created inside `useFrame` callbacks in
`WasmSceneEffects.tsx`. Each `new Float32Array(memory.buffer, ptr, len)`
call is zero-copy (it constructs a view, no data copy), but it allocates
the view object on the heap.

Fix: cache views in the bridge object after WASM init, invalidate and
rebuild only if `memory.buffer.byteLength` changes (detects `Memory.grow`):

```ts
class ParticleFieldBridge {
  private _posView: Float32Array | null = null;
  private _lastBufferByteLength = 0;

  private refreshViews(): void {
    const byteLen = this.memory.buffer.byteLength;
    if (this._posView !== null && this._lastBufferByteLength === byteLen) return;
    this._lastBufferByteLength = byteLen;
    this._posView = new Float32Array(
      this.memory.buffer,
      this.inner.get_positions_ptr(),
      this.inner.get_positions_len(),
    );
    // ... same for opacities, sizes
  }

  getPositions(): Float32Array {
    if (this._disposed) return new Float32Array(0);
    this.refreshViews();
    return this._posView!;
  }
}
```

Apply the same pattern to `AtmosphereFieldBridge` (Uint8Array) and
`WispFieldBridge`.

ESLint rule: add a `no-restricted-syntax` rule in the project ESLint
config that flags `new Float32Array(`, `new Uint32Array(`, `new
THREE.Vector3(` when the containing function name starts with `useFrame`
or when it is the direct callback argument of a `useFrame(` call
expression. Use an AST selector:

```js
{
  selector:
    "CallExpression[callee.name='useFrame'] > :matches(ArrowFunctionExpression, FunctionExpression) NewExpression[callee.name=/Float32Array|Uint32Array|Vector3/]",
  message: "Zero-alloc rule: no heap allocation inside useFrame callbacks."
}
```

This is intentionally narrow (does not ban `new THREE.Matrix4()` which is
a common pattern in `useMemo`). Extensions to cover additional types can
be added per-review.

---

### T6 — Settings wiring for rendering keys

**ADR decisions**: D1, D5
**PRD criteria**: A2, A3, A8 (settings side)
**Files**: `client/src/features/settings/config/settings.ts`,
           `client/src/api/settingsApi.ts`,
           `client/src/features/settings/settingsUIDefinition.ts`,
           `client/src/features/settings/unifiedSettingsConfig.ts`
**Agent**: coder

New settings keys required by Phase 6:

| Key | Type | Default | UI label |
|-----|------|---------|----------|
| `rendering.maxEdgesCeiling` | `number` | `64_000` | "Max edges ceiling" |
| `rendering.softwareFallback` | `"static-cube" \| "off"` | `"static-cube"` | "Software WebGL fallback" |
| `rendering.labelLayoutEvery` | `number` | `3` | "Label layout cadence (frames)" |

Add to the `RenderingSettings` interface in `settings.ts`. Add defaults
to `settingsApi.ts`. Add UI controls in `settingsUIDefinition.ts`:
- `maxEdgesCeiling`: integer slider, range 1000–256000, step 1000.
- `softwareFallback`: radio/select with options `static-cube` / `off`.
- `labelLayoutEvery`: integer input, range 1–10.

These settings are consumed by T1 (`maxEdgesCeiling`), T3
(`softwareFallback`), and T4 (`labelLayoutEvery`). Wiring to the settings
store follows the existing `useSettingsStore(s => s.get<T>(...))` pattern.

---

### T7 — WasmSceneEffects zero-copy audit and bridge cache

**ADR decisions**: D8
**PRD criteria**: A9
**Files**: `client/src/wasm/scene-effects-bridge.ts`,
           `client/src/features/visualisation/components/WasmSceneEffects.tsx`,
           `client/src/__tests__/wasm-scene-effects-alloc.test.ts` (new)
**Agent**: tester + coder

The bridge currently creates a new `Float32Array` / `Uint8Array` view
object on every `getPositions()`, `getOpacities()`, etc. call. This
happens inside `useFrame` in `WasmSceneEffects.tsx`. Fix per T5 pattern.

Heap-snapshot test (new file `client/src/__tests__/wasm-scene-effects-alloc.test.ts`):

1. Load a mocked WASM module (or the real one if available in CI) with
   fixed buffer sizes.
2. Instantiate `ParticleFieldBridge` with the mock.
3. Call `getPositions()` 1000 times in a loop.
4. Measure heap delta via `process.memoryUsage().heapUsed` before and after.
5. Assert delta < 64 KB.

The test exercises the bridge in isolation, not the R3F component, so it
runs in vitest/jest without a browser. The WASM binary itself is not
required; mock the `inner` handle and `memory` object.

Additional invariant: `WasmSceneEffects.tsx` `useFrame` callbacks must not
call `new THREE.Color(...)`, `new THREE.Vector3(...)`, or any other
heap-allocating constructor. Audited as part of T5 lint rule.

WASM growth detection (recovery path, not normal):

```ts
private refreshViewsIfGrown(): void {
  const newLen = this.memory.buffer.byteLength;
  if (newLen === this._lastBufferByteLength) return;
  // Memory grew — rebuild all views
  console.warn('[scene-effects-bridge] WASM memory grew — rebuilding views');
  this._lastBufferByteLength = newLen;
  this._posView = new Float32Array(...);
  // ...
}
```

This keeps the invariant that `getPositions()` never creates a new view
during steady-state tick, but survives the edge case of unexpected growth.

---

### T8 — Node class purity audit

**ADR decisions**: D9
**PRD criteria**: A4, A5
**Files**: `client/src/features/graph/components/GemNodes.tsx` (exists on main, read-only at reset point),
           `client/src/features/graph/components/CrystalOrb.tsx` (create on worktree),
           `client/src/features/graph/components/AgentCapsule.tsx` (create on worktree)
**Agent**: coder

At the reset point (`41979d33e`) only `GemNodes.tsx` is confirmed present.
`CrystalOrb` and `AgentCapsule` may be in `GraphManager.tsx` inline or
absent. The worktree must ensure all three exist as standalone components:

- `GemNodes`: IcosahedronGeometry r=0.5, `GemNodeMaterial` (PBR, transmission).
- `CrystalOrb`: SphereGeometry r=0.5, `CrystalOrbMaterial` (PBR, higher
  transmission, lower roughness than Gem).
- `AgentCapsule`: CapsuleGeometry r=0.3 h=0.6, emissive-tinted opaque material.

Each receives a filtered node array (by type bits from ADR-08 §D6) and
renders one `InstancedMesh`. None computes positions, none owns labels,
none owns selection state.

The `computeNodeScale` utility is imported and called at the call site,
not inside the component. Scale is passed in as a pre-computed
`Float32Array` or computed per-instance in the node-pose loop.

Acceptance test (A4): Jest snapshot test for each component verifying
geometry type, radius, and material class. A5 is integration-level and
deferred to the visual regression suite (T_VR below).

---

## 2. Dynamic Edge Capacity — Detail

The constants and logic replacing `MAX_EDGES`:

```ts
const EDGE_INITIAL = 1024;
// ceiling read from settings at component init; store in ref for stable access
const ceilingRef = useRef<number>(
  settings?.rendering?.maxEdgesCeiling ?? 64_000
);
const capacityRef = useRef<number>(0);  // 0 = not yet allocated

function allocateMesh(targetCapacity: number, edgeRadius: number): THREE.InstancedMesh {
  const geo = createGlassEdgeGeometry(edgeRadius);
  const { material } = createGlassEdgeMaterial();
  const m = new THREE.InstancedMesh(geo, material, targetCapacity);
  m.frustumCulled = false;
  m.count = 0;
  const colorArray = new Float32Array(targetCapacity * 3).fill(1.0);
  m.instanceColor = new THREE.InstancedBufferAttribute(colorArray, 3);
  return m;
}

// On first non-empty points prop:
function onFirstEdges(edgeCount: number): void {
  const ceiling = ceilingRef.current;
  const initial = Math.min(ceilToPowerOfTwo(edgeCount * 1.25), ceiling);
  capacityRef.current = initial;
  meshRef.current = allocateMesh(initial, edgeRadius);
  // attach to scene via parent <primitive>
}

// On subsequent points prop when edgeCount > capacity:
function growIfNeeded(edgeCount: number): void {
  const capacity = capacityRef.current;
  const ceiling = ceilingRef.current;
  if (edgeCount <= capacity) return;
  const newCap = Math.min(Math.max(edgeCount, capacity * 2), ceiling);
  const prevColors = (meshRef.current!.instanceColor as THREE.InstancedBufferAttribute)
    .array as Float32Array;
  const prev = meshRef.current!;
  const next = allocateMesh(newCap, edgeRadius);
  // Copy prior colours into new buffer
  const nextColors = (next.instanceColor as THREE.InstancedBufferAttribute)
    .array as Float32Array;
  nextColors.set(prevColors.subarray(0, Math.min(prevColors.length, nextColors.length)));
  // Dispose old mesh resources
  prev.geometry.dispose();
  (prev.material as THREE.Material).dispose();
  prev.dispose();
  capacityRef.current = newCap;
  meshRef.current = next;
  // re-attach to scene
}
```

`ceilToPowerOfTwo(n)` — round up to next power of two:
```ts
function ceilToPowerOfTwo(n: number): number {
  return Math.pow(2, Math.ceil(Math.log2(n)));
}
```

Reallocation event count is tracked; expected ≤ 1 per session for a
single graph load (initial allocation sized with 25% headroom).

---

## 3. Edge Surface-to-Surface Offset — Formula Reference

ADR-04 D2, PRD-04 F9. Implemented in `computeInstanceMatrices` inside
`GlassEdges.tsx`. Formula (all vectors pre-allocated at module scope):

```ts
tmpSrc.set(pts[off], pts[off+1], pts[off+2]);
tmpTgt.set(pts[off+3], pts[off+4], pts[off+5]);
tmpDir.subVectors(tmpTgt, tmpSrc);
const len = tmpDir.length();
if (len < 1e-6) { /* degenerate — collapse */ continue; }
tmpDir.normalize();

const srcR = srcNodeScale * nodeSize;  // e.g. 1.0 * 0.5 = 0.5 for default gem
const tgtR = tgtNodeScale * nodeSize;
const adjLen = Math.max(0, len - srcR - tgtR);

if (adjLen <= 0) {
  tmpMat.makeScale(0, 0, 0);
  mesh.setMatrixAt(i, tmpMat);
  collapsedCount++;
  continue;
}

// Midpoint of the shortened span
tmpPos.copy(tmpSrc).addScaledVector(tmpDir, srcR + adjLen * 0.5);

// Guard anti-parallel (dot ~ -1) to avoid NaN in setFromUnitVectors
const dot = tmpUp.dot(tmpDir);
if (dot < -0.9999) {
  tmpQuat.set(1, 0, 0, 0);
} else {
  tmpQuat.setFromUnitVectors(tmpUp, tmpDir);
}

tmpScale.set(1, adjLen, 1);
tmpMat.compose(tmpPos, tmpQuat, tmpScale);
mesh.setMatrixAt(i, tmpMat);
```

`tmpUp` = `(0, 1, 0)` — module-scope, constant.
`nodeSize` comes from `settings.visualisation.nodes.nodeSize` (default `0.5`).
`srcNodeScale` / `tgtNodeScale` come from `computeNodeScale(node, ...)`.

The capsule approximation (r=0.3 for offset even though capsule height=0.6)
is accepted as a visible-but-minor mismatch per PRD N5 / ADR R5.

---

## 4. Software-WebGL Environment Fallback

Detection flow (one-shot, cached in state):

```
Canvas.onCreated
  ↓
detectSoftwareRenderer(gl) → boolean | null
  ↓
setIsSoftware(result)    (useState)
  ↓
[JSX branch]
  ├── isSoftware === false / null  →  <Environment resolution={256}>
  ├── isSoftware === true
  │   └── softwareFallback === "static-cube"  →  CubeTextureLoader (useEffect once)
  └── isSoftware === true
      └── softwareFallback === "off"          →  scene.environment = null
```

Freeze contributor analysis: `<Environment resolution={256}>` triggers
drei's internal PMREM generator which renders 6 cube faces × mipmap levels
through the WebGL pipeline. On hardware this is GPU-side and fast. On
`swiftshader` / `llvmpipe` each draw call is CPU-rasterised — the PMREM
generation can hold the JS thread for 2–4 seconds, causing the observed
freeze on `agentbox` headless.

The static-cube fallback loads a pre-baked 64×64 equirectangular per-face
PNG (≈ 12 KB total). `CubeTextureLoader` does a single texture upload; no
per-frame CPU cost.

---

## 5. Statically-Typed Wrapper Props

Pattern enforced in review (ADR-04 D7):

```tsx
// In InstancedLabels (parent wrapper):
export type InstancedLabelsProps = { /* all fields */ };

const forwardedProps: InstancedLabelsProps = {
  nodes,
  nodeIdToIndexMap,
  nodePositionsRef,      // must appear explicitly — not in a spread
  labelPositionsRef,
  settings,
  graphMode,
  perNodeVisualModeMap,
  connectionCountMap,
  hierarchyMap,
  graphTypeVisuals,
  ssspResult,
  isXRMode,
};
return <InstancedLabelsWebGL {...forwardedProps} />;

// In InstancedLabelsWebGL (renderer):
type Props = InstancedLabelsProps;   // alias, not a new interface
const InstancedLabelsWebGL: React.FC<Props> = ({ nodePositionsRef, ... }) => { ... };
```

CI enforcement: `tsc --noEmit` run in the rendering worktree PR check.
A missing field in `forwardedProps` is a type error on the object literal.
A missing prop in `InstancedLabelsWebGL` destructuring is a type error
if `Props` is exactly `InstancedLabelsProps`.

No bare `<InstancedLabelsWebGL {...props} />` without an intermediate
typed object is accepted in code review (mechanical rule, not subjective).

---

## 6. Zero-Allocation Rendering Rule

Hot paths that must not allocate per frame:

| Hot path | Location | Pre-allocated objects |
|----------|----------|-----------------------|
| Node pose update | GemNodes useFrame | module-scope `_mat`, `_pos`, `_scale`, `_quat` |
| Edge matrix update | GlassEdges `computeInstanceMatrices` | module-scope `tmpMat`…`tmpScale` |
| Label position patch | InstancedLabelsWebGL Phase 1 | pre-allocated `InstancedBufferAttribute` arrays |
| Label layout rebuild | InstancedLabelsWebGL Phase 2 | pre-allocated `InstancedBufferAttribute` arrays + `_frustum`, `_projScreenMatrix` |
| Scene effects tick | WasmSceneEffects useFrame | module-scope `_tmpMat4`, `_tmpPos`, `_tmpScale`, `_tmpColor`, `_identityQuat`, `_tmpHsl` |
| Bridge view access | `ParticleFieldBridge.getPositions()` | cached view refs, rebuilt only on `Memory.grow` |

`layoutTextInline()` is the innermost glyph layout function. Its signature
passes pre-allocated arrays by reference and writes into them directly;
it must not `new Float32Array(...)` or return a `GlyphInstance[]`. Verify
during T7 review.

Lint rule covers the most common breakage surface. Heap-snapshot tests
(T7) cover the rest.

---

## 7. WasmSceneEffects Integration

Source layout:

```
client/
  crates/scene-effects/        ← Rust crate (particles.rs, atmosphere.rs,
  │                                          energy_wisps.rs, noise.rs, lib.rs)
  src/wasm/
  │  scene-effects-bridge.ts   ← TypeScript bridge (this file manages bridge)
  │  scene-effects/            ← wasm-pack output (scene_effects.js + .wasm)
  src/hooks/
  │  useWasmSceneEffects.ts    ← React hook (manages WASM lifecycle, stable refs)
  src/features/visualisation/components/
     WasmSceneEffects.tsx      ← R3F component (mounts inside Canvas, after geometry passes)
```

Zero-copy pointer contract:

1. `initSceneEffects()` calls `wasmModule.default()` once — returns
   `{ memory: WebAssembly.Memory }`. Bridge stores `memory`.
2. Bridge allocates objects: `new ParticleField(count)`, etc. WASM
   allocates particle buffers internally during construction.
3. Bridge caches `Float32Array` views over `memory.buffer` at those
   offsets. Views are created **once** (or on `Memory.grow` only).
4. Every frame: `particles.getPositions()` returns the cached view —
   no `new`, no copy.
5. `tick(dt)` / `update(dt, ...)` on the WASM side writes into those
   same buffers. JS sees updated values through the live view.

WASM memory growth constraint: `ParticleField(count)` allocates exactly
`count * (3 + 1 + 1) * 4` bytes (positions + opacities + sizes, f32).
The Rust crate uses `Vec::with_capacity(count)` and never `push` beyond
count. `Memory.grow()` must not be triggered during `tick`. The Rust-side
`#[cfg(debug_assertions)] assert!(...)` on buffer lengths guards this in
dev builds.

`WasmSceneEffects` placement in `GraphCanvas.tsx`:

```tsx
<Canvas ...>
  <ambientLight ... />
  <directionalLight ... />
  {/* Environment: hardware or software fallback */}
  {environmentNode}

  <WasmSceneEffects ... />          {/* scene effects, renderOrder -1 */}
  <EmbeddingCloudLayer ... />
  {canvasReady && nodeCount > 0 && <GraphManager />}   {/* geometry passes */}
  <BotsVisualization />
  <AgentActionVisualization ... />
  <OrbitControls ... />
  <GemPostProcessing ... />         {/* post-processing last */}
</Canvas>
```

`WasmSceneEffects` is explicitly before `GraphManager` so its
`renderOrder={-20}` (atmosphere) and `renderOrder={-1}` (particles)
are drawn behind the node geometry.

---

## 8. Spawn Plan

Three agents are required. Tasks are partially parallel after T6 (settings)
is unblocked.

### Agent 1: coder (R3F components)

Owns: T1, T2, T3, T4, T6, T8
Sequential within agent: T6 → T1 → T2 → T3 → T4 → T8

Rationale: settings keys must exist before component code that reads them.
T1 and T2 both touch `GlassEdges.tsx`; merge is trivial (same file, adjacent
functions). T3 and T4 are independent files; can be parallelised as separate
sub-tasks if needed.

Estimated complexity: T1 (medium — capacity growth logic + reallocation),
T2 (low — formula substitution), T3 (medium — detection logic + fallback
paths), T4 (low — type annotation enforcement), T6 (low — settings schema
additions), T8 (low — component split if needed).

### Agent 2: tester (visual regression + performance budget)

Owns: T7 (heap-snapshot test), plus:
- Visual regression screenshots for A4 (node class geometries) and A10
  (edge offset).
- Performance budget test: mount a synthetic 5,000-node / 20,000-edge
  graph, measure time-to-first-complete-reveal (A1 target: ≤ 3 seconds).
- Allocation profile: Chrome DevTools Memory trace exported as JSON,
  programmatically checked for single-spike pattern (A2).

Estimated complexity: medium. The heap-snapshot test is new; the visual
regression harness may already exist in the project.

### Agent 3: code-reviewer (zero-alloc audit)

Owns: T5 (ESLint rule authoring + baseline audit)
Sequential: runs after coder lands T1/T4/T7 changes so the lint rule can
be validated against the new code.

Reviews:
- Every `useFrame` callback in the rendering tree for new `new ...()` calls.
- The bridge view-caching implementation (T7 fix) for correctness.
- TypeScript strict-mode compliance on T4 changes.
- That `GlassEdges.tsx` contains zero top-level `const MAX_*` identifiers
  after T1.

Estimated complexity: low.

---

## 9. Plan Summary

**Plan file**: `/home/devuser/workspace/visionflow-worktrees/phase-6-rendering/docs/migration-sprint/04-rendering/WORKTREE-PLAN.md`

**Tasks**:

| ID | Description | ADR | Agent | Complexity |
|----|-------------|-----|-------|------------|
| T1 | Dynamic edge capacity (`EDGE_INITIAL=1024`, grow×2, ceiling from settings) | D1,D3,D4 | coder | medium |
| T2 | Edge surface-to-surface offset formula | D2 | coder | low |
| T3 | Software-WebGL environment fallback + detection | D5 | coder | medium |
| T4 | InstancedLabels typed wrapper + explicit prop forwarding | D6,D7 | coder | low |
| T5 | Zero-alloc lint rule + baseline audit | D10 | reviewer | low |
| T6 | Settings schema: `maxEdgesCeiling`, `softwareFallback`, `labelLayoutEvery` | D1,D5 | coder | low |
| T7 | WASM bridge view caching + heap-snapshot test | D8 | tester+coder | medium |
| T8 | Node class purity (GemNodes/CrystalOrb/AgentCapsule standalone) | D9 | coder | low |

**Top 3 risks**:

1. **R1 — Reallocation timing during reveal (ADR R1)**
   If `points` prop delivers a count above initial capacity during the
   first mount cycle, the reallocation lands before reveal completes.
   Mitigation: capacity is sized to the final count before reveal starts
   (D3 discipline). The `growIfNeeded` call is in the `points` prop
   `useEffect`, which fires before reveal starts; reveal only drives
   `mesh.count` against a capacity already confirmed sufficient.

2. **R2 — Software-WebGL detection extension absent (ADR R2)**
   Some browsers / privacy extensions block `WEBGL_debug_renderer_info`.
   If absent, `detectSoftwareRenderer` returns `false` (hardware path).
   Users on software WebGL in privacy mode will not get the fallback;
   they may see a freeze. Mitigation: telemetry counter
   `renderer_detection=unknown` surfaced to the client metrics endpoint
   so the affected population is visible. The fallback is opt-in via
   `rendering.softwareFallback` setting; advanced users can set it
   manually.

3. **R3 — Zero-alloc invariant drift (ADR R3)**
   The lint rule covers `new Float32Array` / `new Uint32Array` /
   `new THREE.Vector3` inside `useFrame`. A contributor adding
   `new THREE.Matrix4()`, `new THREE.Color()`, or any other heap
   allocation inside a useFrame callback would not be caught. Mitigation:
   the lint rule is intentionally narrowed to the three most common
   breakage forms; the heap-snapshot CI test (T7) catches the rest.
   Any regression in heap usage over the 64 KB / 1000-frame threshold
   is a CI failure. The reviewer (Agent 3) performs a manual audit on
   every PR that touches rendering hot paths.
