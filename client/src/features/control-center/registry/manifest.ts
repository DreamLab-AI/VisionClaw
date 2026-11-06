/**
 * Pure-data registry core — the icon-free spine shared by the browser build and
 * the ts-node manifest emitter. Contains NOTHING that imports lucide-react/React
 * so `scripts/emit-settings-manifest.ts` can import it without a bundler.
 *
 * `settingsRegistry.ts` re-attaches LucideIcons to GROUP_DATA to produce the full
 * RegistryGroup[] the UI consumes, and re-exports `testIdFor`.
 */
import type { GroupData, RegistryField, ManifestEntry } from './types';
import { motion } from './groups/motion';
import { look } from './groups/look';
import { labels } from './groups/labels';
import { quality } from './groups/quality';
import { atmosphere } from './groups/atmosphere';
import { immersion } from './groups/immersion';
import { intelligence } from './groups/intelligence';
import { system } from './groups/system';
import { agents } from './groups/agents';
import { decisions } from './groups/decisions';
import { provenance } from './groups/provenance';
import { graphView } from './groups/graphView';

/** The twelve semantic groups, in rail order (hotkeys 1..12). */
export const GROUP_DATA: GroupData[] = [
  motion,
  look,
  labels,
  quality,
  atmosphere,
  immersion,
  intelligence,
  system,
  agents,
  decisions,
  provenance,
  graphView,
];

/** The two bespoke panels below the eight groups in the left rail. */
export const PANELS: ReadonlyArray<{ id: string; testid: string; label: string }> = [
  { id: 'solid', testid: 'panel-solid', label: 'Solid Pod' },
  { id: 'ontology', testid: 'panel-ontology', label: 'Ontology' },
];

/**
 * Deterministic, unique test id. Path fields keep dots (`setting-visualisation.glow.intensity`);
 * pathless (transient/action) fields fall back to `setting-{groupId}.{key}`.
 */
export const testIdFor = (f: RegistryField, groupId: string): string =>
  f.path ? `setting-${f.path}` : `setting-${groupId}.${f.key}`;

/**
 * Infer the server PUT bucket a path routes to, mirroring the prefix routing in
 * api/settings/endpoints.ts::updateSettingsByPaths + schemaMappings.isVisualSettingsPath.
 * Replicated (not imported) to keep this module free of the settings-api graph so
 * the ts-node emitter stays dependency-light. `null` = client/localStorage only.
 */
export function serverBucketFor(path: string | undefined): string | null {
  if (!path) return null;
  if (path.startsWith('visualisation.graphs.') && path.includes('.physics.')) return 'physics';
  if (path.startsWith('visualisation.rendering.')) return 'rendering';
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
  groups: Array<{ id: string; label: string; testid: string; hotkey?: string; fieldCount: number }>;
  panels: Array<{ id: string; testid: string }>;
  settings: ManifestEntry[];
}

/** Flatten GROUP_DATA into the machine-readable manifest consumed by the browser test phase. */
export function buildManifest(): SettingsManifest {
  const settings: ManifestEntry[] = [];
  for (const g of GROUP_DATA) {
    for (const f of g.fields) {
      const server = serverBucketFor(f.path);
      const entry: ManifestEntry = {
        key: f.key,
        path: f.path ?? null,
        testid: testIdFor(f, g.id),
        control: f.type,
        group: g.id,
        label: f.label,
        clientOnly: server === null,
        server,
      };
      if (f.subgroup !== undefined) entry.subgroup = f.subgroup;
      if (f.min !== undefined) entry.min = f.min;
      if (f.max !== undefined) entry.max = f.max;
      if (f.step !== undefined) entry.step = f.step;
      if (f.action !== undefined) entry.action = f.action;
      settings.push(entry);
    }
  }
  return {
    version: '1.0',
    generatedFrom: 'settingsRegistry.ts',
    count: settings.length,
    groups: GROUP_DATA.map((g) => ({
      id: g.id,
      label: g.label,
      testid: `group-${g.id}`,
      hotkey: g.hotkey,
      fieldCount: g.fields.length,
    })),
    panels: PANELS.map((p) => ({ id: p.id, testid: p.testid })),
    settings,
  };
}
