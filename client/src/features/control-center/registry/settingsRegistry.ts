/**
 * Settings registry — the single source of truth the Control Center UI consumes.
 *
 * Assembles the icon-free GROUP_DATA (from manifest.ts) with LucideIcons into the
 * full RegistryGroup[]. Every `path` string is the FROZEN backend contract; the
 * zero-drift test proves this set equals the legacy unifiedSettingsConfig.ts set.
 */
import type { LucideIcon } from 'lucide-react';
// @ts-ignore - Atom exists in lucide-react but its type export lags the runtime.
import { Atom } from 'lucide-react';
import { Layers, Compass, Palette, Sparkles, Network, Settings2 } from 'lucide-react';
import type { RegistryGroup, RegistryField } from './types';
import { GROUP_DATA, SECTION_DATA, testIdFor } from './manifest';

const SECTION_ICONS: Record<string, LucideIcon> = {
  layers: Layers,
  explore: Compass,
  encode: Palette,
  layout: Atom,
  look: Sparkles,
  agents: Network,
  system: Settings2,
};

/**
 * The seven ADR-2189 sections (icons attached), in rail order: these are what
 * the rail, the dock and hotkeys 1..7 open. Each field's `subgroup` is its
 * subsection and `sourceGroup` the group file that declares it.
 */
export const REGISTRY: RegistryGroup[] = SECTION_DATA.map((g) => ({
  ...g,
  icon: SECTION_ICONS[g.id],
}));

/** Flat list of every field across all sections. */
export const ALL_FIELDS: RegistryField[] = REGISTRY.flatMap((g) => g.fields);

/** Every FROZEN backend path (excludes transient localKey/action fields). */
export const ALL_PATHS: string[] = ALL_FIELDS
  .map((f) => f.path)
  .filter((p): p is string => Boolean(p));

/** Look up a section by id. */
export const GROUP_BY_ID: Record<string, RegistryGroup> = Object.fromEntries(
  REGISTRY.map((g) => [g.id, g]),
);

/** The source groups (declaration files), for tests and tooling. */
export const SOURCE_GROUPS = GROUP_DATA;

export { testIdFor };
export { GROUP_DATA, SECTION_DATA, PANELS, serverBucketFor, scopeFor, scopeForField, buildManifest, buildIa, SCOPE_TEXT } from './manifest';
export type { RegistryGroup, RegistryField, MacroDef, ManifestEntry, GroupData, ControlType, SettingScope } from './types';
