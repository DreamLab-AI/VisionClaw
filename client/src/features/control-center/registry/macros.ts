/**
 * Macro layer — the 5 dock dials (ADR-2189 names: Spread, Glow, Motion, Labels, Atmosphere), that write to existing FROZEN paths via a transfer
 * function over t ∈ [0..1], and read them back (`derive`) for their at-rest position.
 * No macro introduces a new path; every path here also appears as a RegistryField
 * (annotated `macro: '<id>'`). See spec §1.1.
 */
import type { LucideIcon } from 'lucide-react';
// @ts-ignore - Atom exists in lucide-react but its type export lags the runtime.
import { Atom } from 'lucide-react';
import { Sun, Wind, Tags, CloudFog } from 'lucide-react';
import type { MacroDef } from './types';
import { scopeFor, strongestScope } from './manifest';
import { LABEL_BUDGET_SUPPORTED } from '../integrationFlags';

const clamp01 = (v: number): number => Math.max(0, Math.min(1, v));
const num = (get: (p: string) => unknown, path: string, fallback: number): number => {
  const v = get(path);
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
};

const P = 'visualisation.graphs.knowledge.physics.';
const S = 'visualisation.sceneEffects.';
const L = 'visualisation.graphs.knowledge.labels.';

const G = 'visualisation.graphView.';

/** ADR-2186 overview-label budget range the Labels dial spans, on the registry slider's step grid. */
const LABEL_BUDGET_MAX = 120;
const LABEL_BUDGET_STEP = 5;

/**
 * The five dock dials (ADR-2189 §3). Ids are stable (`macro-<id>` test ids and the
 * `macro:` field annotations key on them); labels carry the ADR-2189 names.
 * Each dial's `scope` is derived from the paths it writes (see MACROS below).
 */
const MACRO_DEFS: Array<Omit<MacroDef, 'scope'>> = [
  {
    id: 'density',
    // Was "Density": turning it up spreads nodes APART, so the old name read backwards.
    label: 'Spread',
    icon: Atom,
    apply: (t) => [
      { path: `${P}repelK`, value: 40 + t * 360 },        // 40 → 400
      { path: `${P}restLength`, value: 20 + t * 100 },     // 20 → 120
      { path: `${P}centerGravityK`, value: 0.4 - t * 0.3 }, // 0.40 → 0.10
    ],
    derive: (get) => clamp01((num(get, `${P}repelK`, 120) - 40) / 360),
    describe: (get) => `repulsion ${Math.round(num(get, `${P}repelK`, 120))}, spacing ${Math.round(num(get, `${P}restLength`, 50))}`,
  },
  {
    id: 'luminosity',
    // Was "Luminosity". ADR-2182 limits bloom to emphasis, so the dial no longer
    // switches global bloom on or off: it sets ambient light and glow strength. The
    // glow scope (Look › Emphasis glow) decides what glows.
    label: 'Glow',
    icon: Sun,
    apply: (t) => [
      { path: 'visualisation.glow.intensity', value: Math.round(1.5 * t * 20) / 20 },                 // 0 → 1.5, step 0.05
      { path: 'visualisation.rendering.ambientLightIntensity', value: Math.round((0.2 + t * 1.2) * 10) / 10 }, // 0.2 → 1.4, step 0.1
    ],
    derive: (get) => clamp01(num(get, 'visualisation.glow.intensity', 0) / 1.5),
    describe: (get) =>
      `glow ${num(get, 'visualisation.glow.intensity', 0).toFixed(2)}, ambient light ${num(get, 'visualisation.rendering.ambientLightIntensity', 0.2).toFixed(1)}`,
  },
  {
    id: 'motion',
    label: 'Motion',
    icon: Wind,
    apply: (t) => [
      { path: `${P}globalSpeed`, value: 0.05 + t * 1.95 }, // 0.05 → 2.0
      { path: `${P}damping`, value: 0.98 - t * 0.33 },     // 0.98 → 0.65 (inverse)
      { path: `${P}temperature`, value: 0.6 * t },         // 0 → 0.6
    ],
    derive: (get) => clamp01((num(get, `${P}globalSpeed`, 0.4) - 0.05) / 1.95),
    describe: (get) => `speed ${num(get, `${P}globalSpeed`, 0.4).toFixed(2)}, damping ${num(get, `${P}damping`, 0.9).toFixed(2)}`,
  },
  {
    id: 'focus',
    // Was "Focus". ADR-2186 replaces distance culling with a priority-ranked budget:
    // once the renderer reads it (LABEL_BUDGET_SUPPORTED) the dial sets how many of
    // the most important nodes carry a label at overview zoom, 0 → 120 on the
    // registry slider's step of 5. Until then it drives today's label controls:
    // draw distance pulls in (1200 → 300) and the font grows (0.25 → 0.6), never
    // touching nodeFilter.* (writing the filter per drag tick re-ran filtering over
    // the whole corpus and popped labels in and out).
    label: 'Labels',
    icon: Tags,
    apply: (t) =>
      LABEL_BUDGET_SUPPORTED
        ? [{ path: `${G}labels.budget`, value: Math.round((t * LABEL_BUDGET_MAX) / LABEL_BUDGET_STEP) * LABEL_BUDGET_STEP }]
        : [
            { path: `${L}labelDistanceThreshold`, value: 1200 - t * 900 },
            { path: `${L}desktopFontSize`, value: 0.25 + t * 0.35 },
          ],
    derive: (get) =>
      LABEL_BUDGET_SUPPORTED
        ? clamp01(num(get, `${G}labels.budget`, 40) / LABEL_BUDGET_MAX)
        : clamp01((1200 - num(get, `${L}labelDistanceThreshold`, 1200)) / 900),
    describe: (get) =>
      LABEL_BUDGET_SUPPORTED
        ? `${Math.round(num(get, `${G}labels.budget`, 40))} overview labels`
        : `labels within ${Math.round(num(get, `${L}labelDistanceThreshold`, 1200))} units, size ${num(get, `${L}desktopFontSize`, 0.4).toFixed(2)}`,
  },
  {
    id: 'atmosphere',
    label: 'Atmosphere',
    icon: CloudFog,
    apply: (t) => [
      { path: `${S}enabled`, value: t > 0 },
      { path: `${S}particleOpacity`, value: 0.8 * t }, // 0 → 0.8
      { path: `${S}wispOpacity`, value: 0.8 * t },     // 0 → 0.8
      { path: `${S}fogOpacity`, value: 0.12 * t },     // 0 → 0.12
    ],
    derive: (get) => clamp01(num(get, `${S}particleOpacity`, 0) / 0.8),
    describe: (get) => `particles ${Math.round(num(get, `${S}particleOpacity`, 0) * 100)}%, fog ${Math.round((num(get, `${S}fogOpacity`, 0) / 0.12) * 100)}%`,
  },
];

/**
 * Each dial's scope is the strongest scope of the paths it writes, so the dock and
 * the settings rows can never disagree about where a value is stored.
 */
export const MACROS: MacroDef[] = MACRO_DEFS.map((m) => ({
  ...m,
  scope: strongestScope([0, 0.5, 1].flatMap((t) => m.apply(t).map((w) => scopeFor(w.path)))),
}));

export const MACROS_BY_ID: Record<string, MacroDef> = Object.fromEntries(
  MACROS.map((m) => [m.id, m]),
);

/** Every path any macro writes to — used by tests to prove macros never drift off-registry. */
export const MACRO_PATHS: string[] = Array.from(
  new Set(MACROS.flatMap((m) => m.apply(0.5).map((w) => w.path))),
);
