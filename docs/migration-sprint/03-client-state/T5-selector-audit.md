# T5 — Zustand Selector Narrowing Audit

Status: documented (sweep deferred to Phase 5 — see WORKTREE-PLAN R1)
Date  : 2026-05-16
ADR   : ADR-03 D4

## Scope

ADR-03 D4 requires Zustand selectors to return primitives or stable map
references, never synthesised objects or composite slices. The custom lint
rule (`no-broad-zustand-selector`) blocks regressions going forward. This
document tracks the existing broad selectors that the next pass must split.

## Manifest of broad selectors (from WORKTREE-PLAN section 5)

| File | Line | Selector | Verdict | Replacement plan |
|------|------|----------|---------|------------------|
| `GraphManager.tsx` | 270 | `s.settings?.visualisation?.graphs?.logseq` | BROAD — composite object | Split into per-field primitive selectors used downstream (linkWidth, nodeOpacity, edgeColor, edgeGlowStrength). Most consumers want one field. |
| `GraphManager.tsx` | 271 | `s.settings?.visualisation?.graphTypeVisuals` | BROAD — composite | Split into per-type visibility booleans and per-type colour strings. |
| `GraphManager.tsx` | 272 | `s.settings?.visualisation?.glow?.intensity ?? 0.3` | OK — primitive number | Keep. |
| `GraphManager.tsx` | 273 | `s.settings?.system?.debug` | BROAD — composite | Split into `enablePhysicsDebug`, `enableNodeDebug`, `enablePerformanceDebug`, `enabled`. |
| `GraphManager.tsx` | 274 | `s.settings?.nodeFilter` | BROAD — composite | Split into `enabled`, `qualityThreshold`, `authorityThreshold`, `filterByQuality`, `filterByAuthority`, `filterMode`. |
| `GraphManager.tsx` | 275 | `s.settings?.visualisation?.graphs?.logseq?.nodes?.nodeTypeVisibility` | BROAD — composite | Split per-type boolean. |
| `GraphManager.tsx` | 487 | `s.settings?.visualisation?.graphs?.visionflow?.physics` | BROAD — composite | Split into scalars (springStrength, damping, repulsion, etc.). |
| `GraphCanvas.tsx` | 109 | `s.settings?.visualisation?.sceneEffects` | BROAD — composite | Split per-effect boolean. |
| `GlassEdges.tsx` | 143 | `s.get<GlowSettings>('visualisation.glow')` | BROAD — typed object | Split into `glowEnabled`, `glowIntensity`, `glowColor`. |
| `GlassEdges.tsx` | 148 | `s.get<GemMaterialSettings>('visualisation.gemMaterial')` | BROAD — typed object | Split into individual material scalars. |
| `GemNodes.tsx` | 115 | `s.get<GemMaterialSettings>('visualisation.gemMaterial')` | BROAD — same as above | Same replacement plan. |
| `GemNodes.tsx` | 119 | `s.get<QualityGatesSettings>('qualityGates')` | BROAD — composite | Split into `maxNodeCount`, `showClusters`, `showAnomalies`, `showCommunities`. |
| `useGraphFiltering.ts` | 49 | `s.settings?.nodeFilter` | BROAD — see GraphManager:274 | Split per-primitive. |
| `WasmSceneEffects.tsx` | 642 | `s.get<SceneEffectsSettings>('visualisation.sceneEffects')` | BROAD — composite | Split per-effect. |

## Acceptable selectors (no change needed)

| File | Line | Selector | Note |
|------|------|----------|------|
| `GraphCanvas.tsx` | 105-110 | Individual primitives | Already narrow. |
| `SystemHealthIndicator.tsx` | 267-268 | `s.settingsSyncEnabled`, `s.setSettingsSyncEnabled` | Primitive + stable action ref. |
| `GraphManager.tsx` | 272 | `glowIntensity` | Primitive. |

## Lint rule (`no-broad-zustand-selector`)

Custom ESLint rule, deferred to Phase 5. Rule contract:

- Flags `useXxxStore(s => s.someField)` where the selector body returns
  a non-primitive composite (object expression, member chain ending in a
  non-leaf field).
- Flags `s => ({...})` object-literal returns.
- Flags `s => s` (whole store).
- Allowlist: `// zustand-broad-selector-allowed: <reason>` on the
  preceding line, with reviewer sign-off.
- Enforced in CI via `eslint --max-warnings 0`.

## Why this is deferred

The Phase 4 work touched the graph data delivery pipeline (worker proxy,
single-flight, cache + dedup, identity short-circuit). Selector splits
ripple through the render path and must be paired with downstream
memoisation audits (e.g. `useMemo` whose dependency list assumed a single
object reference). Doing both in one phase risks correctness regressions
with no clear bisection target.

Phase 5 owns:
1. Per-file selector split following this manifest.
2. `no-broad-zustand-selector` ESLint rule implementation.
3. Memoisation audit per touched file.
4. CI gate via `eslint --max-warnings 0`.

## Risks if deferred

- New code may introduce broad selectors without surfacing as a lint
  failure. Mitigation: include a checklist item in the PR template.
- The existing broad selectors do not block ADR-03 D4 compliance (they
  predate it); they are a refactor target, not a regression.
