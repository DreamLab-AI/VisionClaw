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
  /** Display labels for `options` (stored value → label). Unlisted options show the stored value. */
  optionLabels?: Record<string, string>;
  /** 'reset_layout' | 'refresh_graph' | 'toggle-webgpu' | 'toggle-agent-offsets' | 'run_clustering' */
  action?: string;
  description?: string;
  /** divider label within the group */
  subgroup?: string;
  showWhen?: { localKey: string; equals: string };
  isPowerUserOnly?: boolean;
  /** id of a macro that co-drives this path (dims the row while macro active) */
  macro?: string;
  /**
   * Source group id (the group file that declares the field). Set by the section
   * assembler; pathless test ids stay `setting-{sourceGroup}.{key}` so they do not
   * change when a field moves section (ADR-2189).
   */
  sourceGroup?: string;
  /** Storage scope for a pathless action whose effect is stored (e.g. reset_layout → shared). */
  scope?: SettingScope;
}

/**
 * Where a setting is stored, derived from its server bucket (ADR-2189 §2):
 * `shared` — the one global physics document: it moves the layout for every viewer and the headset;
 * `server` — a server document stored under one constant key (`visual`, `rendering`,
 *            `quality_gates`, `node_filter`): every viewer loads it, so it is not per account;
 * `browser` — this browser only (localStorage).
 */
export type SettingScope = 'shared' | 'server' | 'browser';

export interface RegistryGroup {
  /** Source groups: 'motion' | 'look' | … ; sections: 'layers' | 'explore' | … (ADR-2189). */
  id: string;
  /** e.g. 'Layout' */
  label: string;
  icon: LucideIcon;
  description: string;
  /** Sections carry '1'..'7' (ADR-2189); source groups carry none. */
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
  /** strongest scope over the paths `apply` writes (derived in macros.ts, never hand-set) */
  scope: SettingScope;
  /** plain-language readout of what the dial currently sets (the dial's aria-valuetext) */
  describe: (get: (path: string) => unknown) => string;
}

export interface ManifestEntry {
  key: string;
  path: string | null;
  testid: string;
  control: ControlType;
  /** section id (ADR-2189): the rail entry / hotkey that opens this control */
  group: string;
  /** the group file that declares the field */
  sourceGroup: string;
  /** subsection label within the section */
  subgroup: string;
  scope: SettingScope;
  label: string;
  min?: number;
  max?: number;
  step?: number;
  action?: string;
  clientOnly: boolean;
  /** inferred endpoint bucket: physics|rendering|qualityGates|nodeFilter|constraints|visual|null */
  server: string | null;
}
