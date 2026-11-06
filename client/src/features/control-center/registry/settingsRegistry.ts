/**
 * Settings registry — the single source of truth the Control Center UI consumes.
 *
 * Assembles the icon-free GROUP_DATA (from manifest.ts) with LucideIcons into the
 * full RegistryGroup[]. Every `path` string is the FROZEN backend contract; the
 * zero-drift test proves this set equals the legacy unifiedSettingsConfig.ts set.
 */
import type { LucideIcon } from 'lucide-react';
// @ts-ignore - Atom/Glasses exist in lucide-react but their type exports lag the runtime.
import { Atom, Glasses } from 'lucide-react';
import { Palette, Type, SlidersHorizontal, Sparkles, Bot, Settings2, Network, GitBranch, ShieldCheck, ScanEye } from 'lucide-react';
import type { RegistryGroup, RegistryField } from './types';
import { GROUP_DATA, testIdFor } from './manifest';

const GROUP_ICONS: Record<string, LucideIcon> = {
  motion: Atom,
  look: Palette,
  labels: Type,
  quality: SlidersHorizontal,
  atmosphere: Sparkles,
  xr: Glasses,
  ai: Bot,
  system: Settings2,
  agents: Network,
  decisions: GitBranch,
  provenance: ShieldCheck,
  graphView: ScanEye,
};

/** The twelve semantic groups (icons attached), in rail order. */
export const REGISTRY: RegistryGroup[] = GROUP_DATA.map((g) => ({
  ...g,
  icon: GROUP_ICONS[g.id],
}));

/** Flat list of every field across all groups. */
export const ALL_FIELDS: RegistryField[] = REGISTRY.flatMap((g) => g.fields);

/** Every FROZEN backend path (excludes transient localKey/action fields). */
export const ALL_PATHS: string[] = ALL_FIELDS
  .map((f) => f.path)
  .filter((p): p is string => Boolean(p));

/** Look up a group by id. */
export const GROUP_BY_ID: Record<string, RegistryGroup> = Object.fromEntries(
  REGISTRY.map((g) => [g.id, g]),
);

export { testIdFor };
export { GROUP_DATA, PANELS, serverBucketFor, buildManifest } from './manifest';
export type { RegistryGroup, RegistryField, MacroDef, ManifestEntry, GroupData, ControlType } from './types';
