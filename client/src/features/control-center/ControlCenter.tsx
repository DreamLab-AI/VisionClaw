/**
 * ControlCenter — the root overlay that composes the whole shell.
 * design-spec.md §2, §6, §7.1.
 *
 * Accepts the SAME props as the legacy IntegratedControlPanel (ControlPanelProps)
 * so the MainLayout swap (WP5) is a one-line change. Responsibilities:
 *  - pointer-events routing: the wrapper is transparent to the pointer; each
 *    surface re-enables pointer events (canvas stays the hero at rest).
 *  - mounts the dock (MacroBar + Ask affordance) and the slide-out SettingsPanel.
 *  - wires the keyboard map (useControlCenterHotkeys) and reveal flow
 *    (useRevealSetting).
 *  - owns the camera-control handoff: a SpacePilot connect/disconnect toggles
 *    the orbit controls. The status/telemetry + the SpacePilot connect UI now
 *    live in the unified StatusSurface (mounted by MainLayout), which reads
 *    SpaceDriver/websocket state directly — no status props are threaded here.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { MessageSquare, MoreHorizontal, Braces, Keyboard } from 'lucide-react';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { SpaceDriver } from '../../services/SpaceDriverService';
import { useSettingsStore } from '../../store/settingsStore';
import type { ControlPanelProps } from '../visualisation/components/ControlPanel/types';
import { CommandInput } from '../visualisation/components/CommandInput';
import { GlassDock } from './primitives/GlassDock';
import { MacroBar } from './macros/MacroBar';
import { SettingsPanel } from './panels/SettingsPanel';
import { useControlCenterUI } from './state/useControlCenterUI';
import { useControlCenterHotkeys } from './hooks/useControlCenterHotkeys';
import { useExclusiveSidePanels } from './hooks/useExclusiveSidePanels';
import { useRevealSetting } from './hooks/useRevealSetting';
import { ShortcutSheet } from './help/ShortcutSheet';
import { GraphLegend } from './integrationSlots';
import { RightDrawer } from './drawer/RightDrawer';
import './styles/control-center.css';

export const ControlCenter: React.FC<ControlPanelProps> = ({
  onOrbitControlsToggle,
}) => {
  const dockCollapsed = useControlCenterUI((s) => s.dockCollapsed);
  const toggleDock = useControlCenterUI((s) => s.toggleDock);

  // Ask affordance: the dock button opens the natural-language CommandInput
  // (the settings assistant). It renders only while open, so nothing extra is
  // mounted at rest.
  const [askOpen, setAskOpen] = useState(false);
  const askRef = useRef<HTMLDivElement>(null);

  const toggleAsk = useCallback(() => setAskOpen((open) => !open), []);

  // Focus the input each time the Ask box opens. CommandInput mounts
  // synchronously with this render, so focus on the next frame once it is live.
  useEffect(() => {
    if (!askOpen) return;
    const raf = requestAnimationFrame(() => {
      askRef.current?.querySelector<HTMLInputElement>('input, textarea')?.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [askOpen]);

  // Realise the ported broadcast contract: `controlcenter:focus-command-input`
  // now opens the command input (previously it focused a phantom, unmounted
  // element) so hotkeys / other surfaces can summon it.
  useEffect(() => {
    const open = () => setAskOpen(true);
    window.addEventListener('controlcenter:focus-command-input', open);
    return () => window.removeEventListener('controlcenter:focus-command-input', open);
  }, []);

  // Esc dismisses the Ask box, scoped to when it is open. The panel owns its own
  // Esc via useControlCenterHotkeys, so this never fights it.
  useEffect(() => {
    if (!askOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setAskOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [askOpen]);

  useControlCenterHotkeys();
  useExclusiveSidePanels();
  useRevealSetting();

  // Camera-control handoff: connecting a SpacePilot hands the camera to the
  // device (orbit controls off); disconnecting hands it back. The SpacePilot
  // display + connect button live in the unified StatusSurface, which reads
  // SpaceDriver directly; this effect owns only the orbit-controls side effect.
  useEffect(() => {
    const handleConnect = () => onOrbitControlsToggle?.(false);
    const handleDisconnect = () => onOrbitControlsToggle?.(true);
    SpaceDriver.addEventListener('connect', handleConnect);
    SpaceDriver.addEventListener('disconnect', handleDisconnect);
    return () => {
      SpaceDriver.removeEventListener('connect', handleConnect);
      SpaceDriver.removeEventListener('disconnect', handleDisconnect);
    };
  }, [onOrbitControlsToggle]);

  // Dev-only test handle required by the browser-automation phase.
  useEffect(() => {
    if (import.meta.env.DEV) {
      (window as unknown as Record<string, unknown>).__settingsStore = useSettingsStore;
      (window as unknown as Record<string, unknown>).__controlCenterUI = useControlCenterUI;
    }
  }, []);

  return (
    <div className="cc-root" data-testid="control-center" style={{ pointerEvents: 'none' }}>
      <div style={{ pointerEvents: 'auto' }}>
        <SettingsPanel />
      </div>

      <div style={{ pointerEvents: 'auto' }}>
        <GlassDock collapsed={dockCollapsed} onToggleCollapsed={toggleDock}>
          <MacroBar />
          <span aria-hidden="true" className="self-stretch w-px my-1 bg-border/50" />
          <button
            type="button"
            data-testid="control-center-ask"
            aria-label="Ask — open the command input"
            aria-expanded={askOpen}
            aria-controls="control-center-command-input"
            onClick={toggleAsk}
            className="cc-glass flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs text-foreground hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <MessageSquare size={14} aria-hidden="true" />
            Ask
          </button>
          <MoreMenu />
        </GlassDock>
      </div>

      <RightDrawer />
      <LegendSlot />
      <ShortcutSheet />

      {/* Ask command input — the ported settings-assistant box. It is
          fixed-position and self-manages its own pointer events, and renders
          only while open (isCollapsed toggles its visibility). */}
      <div ref={askRef} id="control-center-command-input" data-command-input>
        <CommandInput isCollapsed={askOpen} />
      </div>
    </div>
  );
};

ControlCenter.displayName = 'ControlCenter';

/** Overflow menu: destinations that are not settings (Codescape) and the shortcut sheet. */
const MoreMenu: React.FC = () => {
  const openSheet = useControlCenterUI((s) => s.setShortcutSheetOpen);
  const item = 'flex items-center gap-2 rounded px-2 py-1.5 outline-none cursor-pointer data-[highlighted]:bg-white/10';
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          data-testid="dock-more"
          aria-label="More"
          className="flex items-center justify-center h-8 w-8 rounded-md text-muted-foreground hover:text-foreground hover:bg-white/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <MoreHorizontal size={15} aria-hidden="true" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          side="top"
          align="end"
          sideOffset={8}
          className="cc-glass cc-glass--overlay rounded-md p-1 text-xs text-foreground shadow-lg"
          style={{ zIndex: 'var(--cc-z-popover, 45)' as unknown as number }}
        >
          {/* Codescape (ADR-2143): the code-graph section served under /codescape/ on this
              same port. A plain navigation, not a route: a separate bundle and process. */}
          <DropdownMenu.Item asChild className={item}>
            <a href="codescape/" data-testid="dock-codescape">
              <Braces size={13} aria-hidden="true" /> Codescape (code graph)
            </a>
          </DropdownMenu.Item>
          <DropdownMenu.Item className={item} onSelect={() => openSheet(true)} data-testid="dock-shortcuts">
            <Keyboard size={13} aria-hidden="true" /> Keyboard shortcuts <span className="ml-auto pl-3 text-muted-foreground">?</span>
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
};

/**
 * ADR-2184 legend. Always mounted: GraphLegend installs the colour/size encoder,
 * so Encode › Colour by and Size by do nothing without it. It renders nothing
 * while Encode › Show legend is off. It sits above the right-hand corner chips.
 */
const LegendSlot: React.FC = () => {
  // Beside the right-hand drawer while it is open, never under it.
  const drawerOpen = useControlCenterUI((s) => s.drawerTab !== null);
  return (
  <div
    data-testid="legend-slot"
    className="fixed"
    style={{
      right: drawerOpen ? 'calc(var(--cc-edge, 1rem) + var(--cc-drawer-width, min(380px, 100vw - 2rem)) + 0.5rem)' : 'var(--cc-edge, 1rem)',
      bottom: 'var(--cc-dock-clearance, 9rem)',
      maxWidth: 'min(280px, 40vw)',
      maxHeight: '40vh',
      overflowY: 'auto',
      zIndex: 'var(--cc-z-chrome, 40)' as unknown as number,
      pointerEvents: 'auto',
    }}
  >
    <GraphLegend />
  </div>
  );
};

export default ControlCenter;
