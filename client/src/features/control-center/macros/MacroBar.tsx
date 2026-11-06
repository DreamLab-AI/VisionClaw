/**
 * MacroBar — the dock's control row (ADR-2189 §3).
 *
 * Left to right:
 *  1. Layout dials (Spread, Motion). They write the shared physics document, so
 *     each carries the "changes the layout for everyone" marker.
 *  2. View dials (Glow, Labels, Atmosphere). This browser only.
 *  3. Physics switch and Reset layout (asks for confirmation, reports the result).
 *  4. Fit, Legend, Explore. Legend and Explore come from integrationSlots; a
 *     control whose slot is empty is hidden or falls back, never dead.
 *  5. The seven section launchers, hotkeys 1–7.
 *
 * When the dock does not fit the viewport, the five dials fold into one "Dials"
 * menu (useDockCompact), so no control is ever positioned off-screen.
 */

import React, { useCallback, useState } from 'react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { Maximize2, ChevronDown, Map as MapIcon, Filter, RotateCcw, Gauge, Users } from 'lucide-react';
import type { MacroDef, RegistryField } from '../registry/types';
import { MACROS } from '../registry/macros';
import { REGISTRY } from '../registry/settingsRegistry';
import { SCOPE_TEXT, strongestScope } from '../registry/manifest';
import { MacroDial } from '../primitives/MacroDial';
import { SettingToggle } from '../primitives/SettingToggle';
import { useSettingField } from '../hooks/useSettingField';
import { useMacro } from './useMacro';
import { useDockCompact } from './useDockCompact';
import { useControlCenterUI } from '../state/useControlCenterUI';
import { useEnsureMacroPathsLoaded } from '../state/useEnsureGroupLoaded';
import { requestFit, useActiveFilterCount, type FitTarget } from '../integrationSlots';
import { toast } from '../../design-system/components/Toast';

const PHYSICS_ENABLED_PATH = 'visualisation.graphs.knowledge.physics.enabled';
const FIT_TARGET_PATH = 'visualisation.graphView.fitTarget';
const SHOW_LEGEND_PATH = 'visualisation.graphView.explore.showLegend';

const PHYSICS_FIELD: RegistryField = {
  key: 'physicsEnabled',
  label: 'Physics',
  type: 'toggle',
  path: PHYSICS_ENABLED_PATH,
};

const FIT_TARGET_LABELS: Record<FitTarget, string> = {
  knowledge: 'Knowledge body',
  graphs: 'Both graphs',
  scene: 'Whole scene',
};

const dockButton =
  'flex items-center justify-center h-8 min-w-8 px-1.5 gap-1 rounded-md text-muted-foreground hover:text-foreground hover:bg-white/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';

const menuContent = 'cc-glass cc-glass--overlay rounded-md p-1 text-xs text-foreground shadow-lg';
const menuItem =
  'flex items-center gap-2 rounded px-2 py-1.5 outline-none cursor-pointer data-[highlighted]:bg-white/10';

/** One dial + its store binding (a component so `useMacro` is a top-level hook call). */
const MacroDialItem: React.FC<{ macro: MacroDef }> = ({ macro }) => {
  const { value, valueText, onChange, onCommit } = useMacro(macro);
  return (
    <MacroDial
      id={macro.id}
      label={macro.label}
      icon={macro.icon}
      value={value}
      valueText={valueText}
      shared={macro.scope === 'shared'}
      scope={macro.scope}
      scopeText={SCOPE_TEXT[macro.scope]}
      onChange={onChange}
      onCommit={onCommit}
    />
  );
};

const LAYOUT_DIALS = MACROS.filter((m) => m.scope === 'shared');
const VIEW_DIALS = MACROS.filter((m) => m.scope !== 'shared');

const Dials: React.FC = () => (
  <>
    <div role="group" aria-label={`Layout dials. ${SCOPE_TEXT.shared}`} className="flex items-center gap-1" data-testid="dock-layout-dials">
      {LAYOUT_DIALS.map((m) => (
        <MacroDialItem key={m.id} macro={m} />
      ))}
    </div>
    <Divider />
    <div role="group" aria-label={`View dials. ${SCOPE_TEXT[strongestScope(VIEW_DIALS.map((m) => m.scope))]}`} className="flex items-center gap-1" data-testid="dock-view-dials">
      {VIEW_DIALS.map((m) => (
        <MacroDialItem key={m.id} macro={m} />
      ))}
    </div>
  </>
);

/** Compact dock: the five dials behind one button. */
const DialsMenu: React.FC = () => (
  <DropdownMenu.Root>
    <DropdownMenu.Trigger asChild>
      <button type="button" data-testid="dock-dials-menu" aria-label="Dials: Spread, Motion, Glow, Labels, Atmosphere" className={dockButton}>
        <Gauge size={15} aria-hidden="true" />
        <span className="text-[11px]">Dials</span>
        <ChevronDown size={12} aria-hidden="true" />
      </button>
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content
        side="top"
        sideOffset={8}
        className={`${menuContent} flex items-center gap-2 px-3 py-2`}
        style={{ zIndex: 'var(--cc-z-popover, 45)' as unknown as number }}
      >
        <Dials />
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
);

const PhysicsToggle: React.FC = () => {
  const [enabled, setEnabled] = useSettingField<boolean>(PHYSICS_ENABLED_PATH);
  return (
    <div className="flex flex-col items-center gap-1" title={`Physics simulation on or off. ${SCOPE_TEXT.shared}`}>
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

/**
 * Reset layout re-randomises every position on the server, for every viewer.
 * It asks first and reports the result as a toast (audit finding: the old dock
 * button fired immediately and reported nothing).
 */
const ResetLayoutButton: React.FC = () => {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  const run = useCallback(async () => {
    setBusy(true);
    try {
      const { default: axios } = await import('axios');
      await axios.post('/api/settings/physics/reset-layout');
      toast({ title: 'Layout reset', description: 'Positions re-randomised with safe physics defaults, for every viewer.' });
    } catch (err) {
      const message = (err as { response?: { data?: { error?: string } } })?.response?.data?.error;
      toast({ title: 'Layout reset failed', description: message || 'The server did not accept the reset.', variant: 'destructive' });
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  }, []);

  if (confirming) {
    return (
      <div role="group" aria-label="Confirm layout reset" className="flex items-center gap-1" data-testid="macro-reset-confirm">
        <button
          type="button"
          data-testid="macro-reset-confirm-yes"
          disabled={busy}
          onClick={run}
          className="h-8 px-2 rounded-md text-[11px] bg-amber-600 text-white hover:bg-amber-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {busy ? 'Resetting…' : 'Reset for everyone'}
        </button>
        <button type="button" onClick={() => setConfirming(false)} className={`${dockButton} text-[11px]`}>
          Cancel
        </button>
      </div>
    );
  }

  return (
    <button
      type="button"
      data-testid="macro-reset"
      aria-label={`Reset layout. ${SCOPE_TEXT.shared}`}
      title={`Reset layout: re-randomise every position. ${SCOPE_TEXT.shared}`}
      onClick={() => setConfirming(true)}
      className={dockButton}
    >
      <RotateCcw size={15} aria-hidden="true" />
      <Users size={10} aria-hidden="true" className="text-amber-400" />
    </button>
  );
};

/**
 * Fit (ADR-2180): frames the stored fit scope (graphView.fitTarget). The chevron
 * picks the scope, saves it and fits to it. Fit used to be a canvas button that
 * the Status chip covered (audit finding 1).
 */
const FitButton: React.FC = () => {
  const [stored, setTarget] = useSettingField<FitTarget>(FIT_TARGET_PATH);
  const target: FitTarget = stored ?? 'knowledge';

  const fit = (
    <button
      type="button"
      data-testid="dock-fit"
      aria-label={`Fit view: ${FIT_TARGET_LABELS[target]}`}
      title={`Fit view · ${FIT_TARGET_LABELS[target]}`}
      onClick={() => requestFit()}
      className={dockButton}
    >
      <Maximize2 size={15} aria-hidden="true" />
    </button>
  );

  return (
    <div className="flex items-center">
      {fit}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button type="button" data-testid="dock-fit-scope" aria-label="Choose what Fit frames" className={`${dockButton} min-w-0 px-0.5`}>
            <ChevronDown size={12} aria-hidden="true" />
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content side="top" sideOffset={8} className={menuContent} style={{ zIndex: 'var(--cc-z-popover, 45)' as unknown as number }}>
            <DropdownMenu.RadioGroup value={target} onValueChange={(v) => { setTarget(v as FitTarget); requestFit(v as FitTarget); }}>
              {(Object.keys(FIT_TARGET_LABELS) as FitTarget[]).map((t) => (
                <DropdownMenu.RadioItem key={t} value={t} className={menuItem}>
                  <DropdownMenu.ItemIndicator>•</DropdownMenu.ItemIndicator>
                  {FIT_TARGET_LABELS[t]}
                </DropdownMenu.RadioItem>
              ))}
            </DropdownMenu.RadioGroup>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  );
};

/** Legend toggle (ADR-2184): shows or hides the legend ControlCenter mounts. */
const LegendToggle: React.FC = () => {
  const [shown, setShown] = useSettingField<boolean>(SHOW_LEGEND_PATH);
  return (
    <button
      type="button"
      data-testid="dock-legend"
      aria-label="Legend"
      aria-pressed={!!shown}
      title="Show the legend for the colour and size encodings"
      onClick={() => setShown(!shown)}
      className={`${dockButton} ${shown ? 'text-primary' : ''}`}
    >
      <MapIcon size={15} aria-hidden="true" />
    </button>
  );
};

/** Explore (ADR-2185): opens the explore panel; the badge counts active filters. */
const ExploreButton: React.FC = () => {
  const count = useActiveFilterCount();
  const open = useControlCenterUI((s) => s.drawerTab === 'explore');
  const openDrawer = useControlCenterUI((s) => s.openDrawer);
  const closeDrawer = useControlCenterUI((s) => s.closeDrawer);
  const setOpen = (next: boolean) => (next ? openDrawer('explore') : closeDrawer());
  // "Explore panel", not "Explore": section 2 is also called Explore (its settings).
  const label = count > 0 ? `Explore panel: search and filter (${count} active filter${count === 1 ? '' : 's'})` : 'Explore panel: search and filter';
  return (
    <button
      type="button"
      data-testid="dock-explore"
      aria-label={label}
      aria-pressed={open}
      title={label}
      onClick={() => setOpen(!open)}
      className={`${dockButton} relative`}
    >
      <Filter size={15} aria-hidden="true" />
      {count > 0 && (
        <span aria-hidden="true" className="absolute -top-1 -right-1 min-w-4 h-4 px-1 rounded-full bg-primary text-[9px] leading-4 text-primary-foreground">
          {count}
        </span>
      )}
    </button>
  );
};

const Divider: React.FC = () => <span aria-hidden="true" className="self-stretch w-px my-1 bg-border/50" />;

export const MacroBar: React.FC = () => {
  useEnsureMacroPathsLoaded();
  const openGroup = useControlCenterUI((s) => s.openGroup);
  const { ref, compact } = useDockCompact<HTMLDivElement>();

  return (
    <div
      ref={ref}
      data-testid="macro-bar"
      data-compact={compact}
      role="group"
      aria-label="Dock controls"
      className="flex items-center gap-2"
    >
      {compact ? <DialsMenu /> : <Dials />}

      <Divider />

      <PhysicsToggle />
      <ResetLayoutButton />

      <Divider />

      <div role="group" aria-label="View" className="flex items-center gap-0.5">
        <FitButton />
        <LegendToggle />
        <ExploreButton />
      </div>

      <Divider />

      {/* The seven ADR-2189 sections, hotkeys 1–7. */}
      <div role="group" aria-label="Settings sections" className="flex items-center gap-0.5">
        {REGISTRY.map((group) => {
          const Icon = group.icon;
          return (
            <button
              key={group.id}
              type="button"
              data-testid={`dock-group-${group.id}`}
              aria-label={`Open ${group.label} (${group.hotkey})`}
              aria-keyshortcuts={group.hotkey}
              title={`${group.label} · ${group.hotkey}`}
              onClick={() => openGroup(group.id)}
              className={dockButton}
            >
              {Icon && <Icon size={15} aria-hidden="true" />}
            </button>
          );
        })}
      </div>
    </div>
  );
};

MacroBar.displayName = 'MacroBar';

export default MacroBar;
