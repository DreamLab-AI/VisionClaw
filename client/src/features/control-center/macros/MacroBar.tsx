/**
 * MacroBar — the L1 row hosted inside the dock. design-spec.md §1.1, §6.1.
 *
 * Contents: the five macro dials (Density, Luminosity, Motion, Focus,
 * Atmosphere), a physics on/off toggle, a reset-layout action, and the eight
 * group-opening icon buttons so every semantic group is reachable by mouse
 * straight from the dock. Mounting hydrates the subtrees the dials derive from.
 */

import React from 'react';
import { Braces } from 'lucide-react';
import type { MacroDef, RegistryField } from '../registry/types';
import { MACROS } from '../registry/macros';
import { REGISTRY } from '../registry/settingsRegistry';
import { MacroDial } from '../primitives/MacroDial';
import { SettingAction } from '../primitives/SettingAction';
import { SettingToggle } from '../primitives/SettingToggle';
import { useSettingField } from '../hooks/useSettingField';
import { useMacro } from './useMacro';
import { useControlCenterUI } from '../state/useControlCenterUI';
import { useEnsureMacroPathsLoaded } from '../state/useEnsureGroupLoaded';

const PHYSICS_ENABLED_PATH = 'visualisation.graphs.knowledge.physics.enabled';

const PHYSICS_FIELD: RegistryField = {
  key: 'physicsEnabled',
  label: 'Physics',
  type: 'toggle',
  path: PHYSICS_ENABLED_PATH,
};

const RESET_FIELD: RegistryField = {
  key: 'resetLayout',
  label: 'Reset',
  type: 'action-button',
  action: 'reset_layout',
};

/** One dial + its store binding. Kept a component so `useMacro` is a top-level
 *  hook call (never in a loop) for each of the five stable macros. */
const MacroDialItem: React.FC<{ macro: MacroDef }> = ({ macro }) => {
  const { value, onChange, onCommit } = useMacro(macro);
  return (
    <MacroDial
      id={macro.id}
      label={macro.label}
      icon={macro.icon}
      value={value}
      onChange={onChange}
      onCommit={onCommit}
    />
  );
};

const PhysicsToggle: React.FC = () => {
  const [enabled, setEnabled] = useSettingField<boolean>(PHYSICS_ENABLED_PATH);
  return (
    <div className="flex flex-col items-center gap-1">
      <SettingToggle
        field={PHYSICS_FIELD}
        testId="macro-physics"
        checked={!!enabled}
        onChange={(v) => setEnabled(v)}
      />
      <span className="cc-field-label select-none">Physics</span>
    </div>
  );
};

const Divider: React.FC = () => (
  <span aria-hidden="true" className="self-stretch w-px my-1 bg-border/50" />
);

export const MacroBar: React.FC = () => {
  useEnsureMacroPathsLoaded();
  const openGroup = useControlCenterUI((s) => s.openGroup);

  return (
    <div
      data-testid="macro-bar"
      role="group"
      aria-label="Macro controls"
      className="flex items-center gap-3"
    >
      {MACROS.map((macro) => (
        <MacroDialItem key={macro.id} macro={macro} />
      ))}

      <Divider />

      <PhysicsToggle />

      <div className="w-24">
        <SettingAction field={RESET_FIELD} testId="macro-reset" />
      </div>

      <Divider />

      {/* Group launchers — the eight semantic groups, reachable by mouse. */}
      <div className="flex items-center gap-1">
        {REGISTRY.map((group) => {
          const Icon = group.icon;
          return (
            <button
              key={group.id}
              type="button"
              data-testid={`dock-group-${group.id}`}
              aria-label={`Open ${group.label}${group.hotkey ? ` (${group.hotkey})` : ''}`}
              title={`${group.label}${group.hotkey ? ` · ${group.hotkey}` : ''}`}
              onClick={() => openGroup(group.id)}
              className="flex items-center justify-center h-8 w-8 rounded-md text-muted-foreground hover:text-foreground hover:bg-white/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              {Icon && <Icon size={15} aria-hidden="true" />}
            </button>
          );
        })}
      </div>

      <Divider />

      {/* Codescape (ADR-2143): the code-graph section served under /codescape/ on
          this same port via nginx. A plain navigation, not a route: Codescape is a
          separate bundle and process with its own isolation guarantees. */}
      <a
        href="codescape/"
        data-testid="dock-codescape"
        aria-label="Open Codescape (code graph)"
        title="Codescape · code graph"
        className="flex items-center justify-center h-8 w-8 rounded-md text-muted-foreground hover:text-foreground hover:bg-white/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Braces size={15} aria-hidden="true" />
      </a>
    </div>
  );
};

MacroBar.displayName = 'MacroBar';

export default MacroBar;
