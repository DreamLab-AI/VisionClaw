/**
 * Control Center registry types — the build contract for the settings SSOT.
 *
 * `RegistryField.path` strings are the FROZEN backend contract: routing in
 * api/settings/endpoints.ts prefix-matches these exact strings. Never rename
 * a path here; presentation restructures around them.
 */

import type { LucideIcon } from 'lucide-react';

export type ControlType =
  | 'slider'
  | 'toggle'
  | 'color'
  | 'select'
  | 'text'
  | 'action-button'
  | 'nostr-button'
  | 'readonly';

export interface RegistryField {
  /** unique within its group */
  key: string;
  label: string;
  type: ControlType;
  /** FROZEN backend dot-path — SSOT, never edit the string */
  path?: string;
  /** transient client-local key (analytics grouping); mutually exclusive with path */
  localKey?: string;
  /** seed value for a transient localKey field (initialises the panel's local map) */
  default?: string | number | boolean;
  min?: number;
  max?: number;
  step?: number;
  options?: string[];
  /** 'reset_layout' | 'refresh_graph' | 'toggle-webgpu' | 'toggle-agent-offsets' | 'run_clustering' */
  action?: string;
  description?: string;
  /** divider label within the group */
  subgroup?: string;
  showWhen?: { localKey: string; equals: string };
  isPowerUserOnly?: boolean;
  /** id of a macro that co-drives this path (dims the row while macro active) */
  macro?: string;
}

export interface RegistryGroup {
  /** 'motion' | 'look' | 'labels' | 'quality' | 'atmosphere' | 'xr' | 'ai' | 'system' */
  id: string;
  /** e.g. 'Motion & Forces' */
  label: string;
  icon: LucideIcon;
  description: string;
  /** '1'..'8' — realises the old dead buttonKey badges */
  hotkey?: string;
  /** coarse subtree paths for ensureLoaded() on first open */
  loadPaths: string[];
  fields: RegistryField[];
}

/**
 * Icon-free view of a group — the pure-data shape the group files export and the
 * manifest emitter consumes. `settingsRegistry.ts` re-attaches the LucideIcon to
 * produce a full RegistryGroup. Keeping icons out of this shape lets the emitter
 * (ts-node, no bundler) import group data without pulling in lucide-react/React.
 */
export type GroupData = Omit<RegistryGroup, 'icon'>;

export interface MacroDef {
  id: string;
  label: string;
  icon: LucideIcon;
  /** forward write: t in [0..1] → concrete path writes */
  apply: (t: number) => Array<{ path: string; value: number | boolean }>;
  /** read-back: derive dial position [0..1] from current settings */
  derive: (get: (path: string) => unknown) => number;
}

export interface ManifestEntry {
  key: string;
  path: string | null;
  testid: string;
  control: ControlType;
  group: string;
  subgroup?: string;
  label: string;
  min?: number;
  max?: number;
  step?: number;
  action?: string;
  clientOnly: boolean;
  /** inferred endpoint bucket: physics|rendering|qualityGates|nodeFilter|constraints|visual|null */
  server: string | null;
}
