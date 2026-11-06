/**
 * Pure-data registry core — the icon-free spine shared by the browser build and
 * the ts-node manifest emitter. Contains NOTHING that imports lucide-react/React
 * so `scripts/emit-settings-manifest.ts` can import it without a bundler.
 *
 * `settingsRegistry.ts` re-attaches LucideIcons to GROUP_DATA to produce the full
 * RegistryGroup[] the UI consumes, and re-exports `testIdFor`.
 */
import type { GroupData, RegistryField, ManifestEntry, SettingScope } from './types';
import { PATH_BEFORE, SECTION_DEFS, PANEL_PLACEMENT, placeField } from './placement';
import type { SectionId } from './placement';
import { motion } from './groups/motion';
import { look } from './groups/look';
import { labels } from './groups/labels';
import { quality } from './groups/quality';
import { atmosphere } from './groups/atmosphere';
import { intelligence } from './groups/intelligence';
import { system } from './groups/system';
import { agents } from './groups/agents';
import { provenance } from './groups/provenance';
import { graphView } from './groups/graphView';

/**
 * The source groups: where each field is DECLARED. They are not shown as menus;
 * `SECTION_DATA` regroups their fields into the seven ADR-2189 sections.
 */
export const GROUP_DATA: GroupData[] = [
  motion,
  look,
  labels,
  quality,
  atmosphere,
  intelligence,
  system,
  agents,
  provenance,
  graphView,
];

/** The bespoke panels; each opens from a launcher inside its section (PANEL_PLACEMENT). */
export const PANELS: ReadonlyArray<{ id: string; testid: string; label: string; section: SectionId; subsection: string }> = [
  { id: 'solid', testid: 'panel-solid', label: 'Solid Pod', section: PANEL_PLACEMENT.solid[0], subsection: PANEL_PLACEMENT.solid[1] },
  { id: 'ontology', testid: 'panel-ontology', label: 'Ontology browser', section: PANEL_PLACEMENT.ontology[0], subsection: PANEL_PLACEMENT.ontology[1] },
];

/**
 * Deterministic, unique test id. Path fields keep dots (`setting-visualisation.glow.intensity`);
 * pathless (transient/action) fields fall back to `setting-{sourceGroup}.{key}`, so a
 * field keeps its test id when it moves section.
 */
export const testIdFor = (f: RegistryField, groupId: string): string =>
  f.path ? `setting-${f.path}` : `setting-${f.sourceGroup ?? groupId}.${f.key}`;

/** Where a setting is stored (ADR-2189 §2), from its server bucket. */
export function scopeFor(path: string | undefined): SettingScope {
  const bucket = serverBucketFor(path);
  if (bucket === 'physics') return 'shared';
  return bucket ? 'server' : 'browser';
}

/** A field's scope: its own override (actions), else derived from its path. */
export function scopeForField(f: RegistryField): SettingScope {
  return f.scope ?? scopeFor(f.path);
}

/** A section with its fields: the icon-free shape the UI and the emitters consume. */
export type SectionData = GroupData & { id: SectionId; hotkey: string };

/**
 * Assemble the seven sections from the source groups. Fields are ordered by the
 * section's subsection order, then by declaration order; each carries
 * `subgroup` = its subsection and `sourceGroup` = its declaring group. A section's
 * loadPaths is the union of its source groups' loadPaths.
 */
function buildSections(): SectionData[] {
  const placed = GROUP_DATA.flatMap((g, gi) =>
    g.fields.map((f, fi) => {
      const { section, subsection } = placeField(g.id, f);
      return { g, f, section, subsection, order: gi * 1000 + fi };
    }),
  );
  const orderOf = new Map(placed.filter((p) => p.f.path).map((p) => [p.f.path!, p.order]));
  for (const p of placed) {
    const before = p.f.path ? PATH_BEFORE[p.f.path] : undefined;
    const anchor = before ? orderOf.get(before) : undefined;
    if (anchor !== undefined) p.order = anchor - 0.5;
  }
  return SECTION_DEFS.map((def) => {
    const mine = placed
      .filter((p) => p.section === def.id)
      .sort(
        (a, b) =>
          def.subsections.indexOf(a.subsection) - def.subsections.indexOf(b.subsection) || a.order - b.order,
      );
    const loadPaths = Array.from(new Set(mine.flatMap((p) => p.g.loadPaths)));
    return {
      id: def.id,
      label: def.label,
      description: def.description,
      hotkey: def.hotkey,
      loadPaths,
      fields: mine.map((p) => ({ ...p.f, subgroup: p.subsection, sourceGroup: p.g.id })),
    };
  });
}

/** The seven ADR-2189 sections, in rail order (hotkeys 1..7). */
export const SECTION_DATA: SectionData[] = buildSections();

/**
 * Infer the server PUT bucket a path routes to, mirroring the prefix routing in
 * api/settings/endpoints.ts::updateSettingsByPaths + schemaMappings.isVisualSettingsPath.
 * Replicated (not imported) to keep this module free of the settings-api graph so
 * the ts-node emitter stays dependency-light. `null` = client/localStorage only.
 */
/** Mirrors schemaMappings.SERVER_RENDERING_KEYS (settingsRoundTrip.test.ts pins the two). */
const SERVER_RENDERING_KEYS: ReadonlySet<string> = new Set([
  'ambientLightIntensity', 'backgroundColor', 'directionalLightIntensity',
  'enableAmbientOcclusion', 'enableAntialiasing', 'enableShadows', 'environmentIntensity',
  'shadowMapSize', 'shadowBias', 'context',
]);

export function serverBucketFor(path: string | undefined): string | null {
  if (!path) return null;
  if (path.startsWith('visualisation.graphs.') && path.includes('.physics.')) return 'physics';
  if (path.startsWith('visualisation.rendering.')) {
    // Only the server RenderingSettings fields go to /rendering (serde drops anything else);
    // agentColors.*, maxEdgesCeiling, softwareFallback and labelLayoutEvery live in /visual.
    const rest = path.slice('visualisation.rendering.'.length);
    // per-device performance budgets stay in this browser (schemaMappings.DEVICE_LOCAL_RENDERING_KEYS)
    if (['maxEdgesCeiling', 'softwareFallback', 'labelLayoutEvery'].includes(rest.split('.')[0])) return null;
    return !rest.includes('.') && SERVER_RENDERING_KEYS.has(rest) ? 'rendering' : 'visual';
  }
  if (path.startsWith('qualityGates.')) return 'qualityGates';
  if (path.startsWith('nodeFilter.')) return 'nodeFilter';
  if (path.startsWith('constraints.')) return 'constraints';
  // isVisualSettingsPath: everything else under visualisation.* (excluding the
  // rendering/physics prefixes already handled above) → the /visual bucket.
  if (path.startsWith('visualisation.rendering')) return null;
  if (path.startsWith('visualisation.graphs.') && path.includes('.physics')) return null;
  if (path.startsWith('visualisation.')) return 'visual';
  return null;
}

export interface SettingsManifest {
  version: string;
  generatedFrom: string;
  count: number;
  /** the seven sections (ADR-2189); `group-{id}` test ids and hotkeys 1..7 */
  groups: Array<{ id: string; label: string; testid: string; hotkey?: string; fieldCount: number }>;
  panels: Array<{ id: string; testid: string; section: string }>;
  settings: ManifestEntry[];
}

/** Flatten GROUP_DATA into the machine-readable manifest consumed by the browser test phase. */
export function buildManifest(): SettingsManifest {
  const settings: ManifestEntry[] = [];
  for (const sec of SECTION_DATA) {
    for (const f of sec.fields) {
      const server = serverBucketFor(f.path);
      const entry: ManifestEntry = {
        key: f.key,
        path: f.path ?? null,
        testid: testIdFor(f, sec.id),
        control: f.type,
        group: sec.id,
        sourceGroup: f.sourceGroup as string,
        subgroup: f.subgroup as string,
        scope: scopeForField(f),
        label: f.label,
        clientOnly: server === null,
        server,
      };
      if (f.min !== undefined) entry.min = f.min;
      if (f.max !== undefined) entry.max = f.max;
      if (f.step !== undefined) entry.step = f.step;
      if (f.action !== undefined) entry.action = f.action;
      settings.push(entry);
    }
  }
  return {
    version: '2.0',
    generatedFrom: 'settingsRegistry.ts',
    count: settings.length,
    groups: SECTION_DATA.map((g) => ({
      id: g.id,
      label: g.label,
      testid: `group-${g.id}`,
      hotkey: g.hotkey,
      fieldCount: g.fields.length,
    })),
    panels: PANELS.map((p) => ({ id: p.id, testid: p.testid, section: p.section })),
    settings,
  };
}

/** One subsection of the shared IA document. */
export interface IaSubsection {
  label: string;
  fields: Array<{ key: string; label: string; path: string | null; control: string; scope: SettingScope }>;
  panels: Array<{ id: string; label: string }>;
}

/**
 * The information architecture both clients render (ADR-2189 §2): section →
 * subsection → fields. Emitted to `registry/ia.json` and `xr-client/data/ia.json`
 * by `scripts/emit-settings-manifest.ts`; a Vitest parity test pins both copies
 * to this function, so desktop and headset cannot drift apart silently.
 */
export interface IaDocument {
  version: string;
  adr: string;
  generatedFrom: string;
  scopes: Record<SettingScope, string>;
  sections: Array<{ id: SectionId; label: string; description: string; hotkey: string; subsections: IaSubsection[] }>;
}

/**
 * Every server bucket is one document under a constant key (settings_routes.rs:
 * "physics", "visual", "quality_gates", "node_filter"; rendering lives in the
 * settings actor), so a saved value reaches every viewer, not one account.
 */
export const SCOPE_TEXT: Record<SettingScope, string> = {
  shared: 'Changes the layout for everyone',
  server: 'Saved on the server for every viewer',
  browser: 'This browser only',
};

const SCOPE_RANK: Record<SettingScope, number> = { browser: 0, server: 1, shared: 2 };

/** The strongest of several scopes (shared > server > browser). */
export function strongestScope(scopes: Iterable<SettingScope>): SettingScope {
  let best: SettingScope = 'browser';
  for (const s of scopes) if (SCOPE_RANK[s] > SCOPE_RANK[best]) best = s;
  return best;
}

export function buildIa(): IaDocument {
  return {
    version: '1.0',
    adr: 'ADR-2189',
    generatedFrom: 'client/src/features/control-center/registry (buildIa)',
    scopes: SCOPE_TEXT,
    sections: SECTION_DATA.map((sec) => {
      const def = SECTION_DEFS.find((d) => d.id === sec.id)!;
      const subsections: IaSubsection[] = def.subsections
        .map((label) => ({
          label,
          fields: sec.fields
            .filter((f) => f.subgroup === label)
            .map((f) => ({ key: f.key, label: f.label, path: f.path ?? null, control: f.type, scope: scopeForField(f) })),
          panels: PANELS.filter((p) => p.section === sec.id && p.subsection === label).map((p) => ({ id: p.id, label: p.label })),
        }))
        .filter((ss) => ss.fields.length > 0 || ss.panels.length > 0);
      return { id: sec.id, label: sec.label, description: sec.description, hotkey: sec.hotkey, subsections };
    }),
  };
}
