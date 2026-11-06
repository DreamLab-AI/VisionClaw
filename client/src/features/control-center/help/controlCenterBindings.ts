/**
 * The control centre's keyboard map, as data (ADR-2189 §2). The hotkey hook binds
 * from it and the shortcut sheet lists it, so the sheet can never advertise a key
 * that does nothing. Section keys are generated from the registry's section order.
 */
import { REGISTRY } from '../registry/settingsRegistry';

export interface Binding {
  /** display form, e.g. "1", "/", "Ctrl/⌘ + ." */
  keys: string;
  description: string;
  group: 'Sections' | 'Control centre';
}

export const SECTION_BINDINGS: Binding[] = REGISTRY.map((g) => ({
  keys: g.hotkey as string,
  description: `Open ${g.label}`,
  group: 'Sections',
}));

export const CONTROL_CENTRE_BINDINGS: Binding[] = [
  { keys: '/', description: 'Explore: open the explore panel and focus its search', group: 'Control centre' },
  { keys: '?', description: 'Show this list of keyboard shortcuts', group: 'Control centre' },
  { keys: 'Esc', description: 'Close the settings panel', group: 'Control centre' },
  { keys: 'Ctrl/⌘ + .', description: 'Hide or show the dock', group: 'Control centre' },
  { keys: 'Ctrl/⌘ + K', description: 'Command palette: search every setting and command', group: 'Control centre' },
];

/** Event names that open the sheet: the hotkey, and the palette/onboarding command. */
export const SHORTCUT_SHEET_EVENTS = ['controlcenter:help', 'show-keyboard-shortcuts'] as const;
