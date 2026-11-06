/**
 * RightDrawer — the one right-hand drawer (ADR-2189 §3).
 *
 * Three tabs share one place on screen, so none can cover another:
 *  - Explore (ADR-2185 ExplorePanel): opened from the dock or '/'.
 *  - Memory (Memory explorer): available while the embedding cloud is on, and
 *    shown when the cloud is switched on and the drawer is closed.
 *  - Node (NodeDetailPanel): shown when a node is selected; closing it
 *    deselects the node and returns to the tab shown before it.
 *
 * Keyboard and focus:
 *  - The tab list is a WAI-ARIA tablist: Left/Right/Home/End move between tabs.
 *  - Esc inside the drawer closes the active tab (an open dropdown inside it
 *    keeps Esc for itself). Esc elsewhere is routed by useControlCenterHotkeys.
 *  - Opening from a control moves focus to the active tab; opening from a
 *    canvas click (node selection) leaves focus where it was and announces the
 *    node in a polite live region instead.
 *  - Closing returns focus to the control that opened the drawer.
 *  - Inactive tab panels are `hidden`, and the closed drawer is `hidden` and
 *    inert, so nothing behind them is reachable by Tab.
 *
 * Panels stay mounted across tab switches (the explore panel's search text,
 * the node detail's source card), so switching tabs loses nothing.
 */
import React, { useCallback, useEffect, useId, useRef, useState } from 'react';
import { X } from 'lucide-react';
import { useSettingsStore } from '../../../store/settingsStore';
import { useControlCenterUI, type DrawerTab } from '../state/useControlCenterUI';
import { ExplorePanel } from '../integrationSlots';
import MemoryExplorerPanel from '../../visualisation/memoryCloud/MemoryExplorerPanel';
import { NodeDetailPanel } from '../../graph/components/NodeDetailPanel';

export const DRAWER_TABS: ReadonlyArray<{ id: DrawerTab; label: string }> = [
  { id: 'explore', label: 'Explore' },
  { id: 'memory', label: 'Memory' },
  { id: 'node', label: 'Node' },
];

/** Same rule as the hotkey hook: an open popper, listbox or dialog owns Esc. */
function popoverOwnsEscape(): boolean {
  return (
    typeof document !== 'undefined' &&
    !!document.querySelector('[data-radix-popper-content-wrapper], [role="listbox"], [role="dialog"][data-state="open"]')
  );
}

/** Focus that came from the canvas or nowhere is not worth moving or restoring. */
function isIncidentalFocus(el: Element | null): boolean {
  return !el || el === document.body || el.tagName === 'CANVAS';
}

interface NodeSelected { label?: string; nodeId?: string | number }

/**
 * One panel failing must not take the dock and settings down with it: the
 * drawer lives inside the control centre, so each tab panel gets its own
 * boundary. It resets when its key changes (another node, the tab reopened).
 */
class PanelBoundary extends React.Component<
  { name: string; resetKey: string; children: React.ReactNode },
  { failed: boolean; key: string }
> {
  state = { failed: false, key: this.props.resetKey };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  static getDerivedStateFromProps(props: { resetKey: string }, state: { failed: boolean; key: string }) {
    return props.resetKey !== state.key ? { failed: false, key: props.resetKey } : null;
  }
  componentDidCatch(error: Error) {
    // eslint-disable-next-line no-console
    console.error(`[RightDrawer] ${this.props.name} panel failed:`, error);
  }
  render() {
    if (this.state.failed) {
      return (
        <p role="alert" className="cc-glass rounded-xl p-3 text-xs text-muted-foreground" data-testid="drawer-panel-error">
          The {this.props.name} panel could not be shown. Close it and try again.
        </p>
      );
    }
    return this.props.children;
  }
}

export const RightDrawer: React.FC = () => {
  const tab = useControlCenterUI((s) => s.drawerTab);
  const openDrawer = useControlCenterUI((s) => s.openDrawer);
  const closeDrawer = useControlCenterUI((s) => s.closeDrawer);
  const dismissDrawerTab = useControlCenterUI((s) => s.dismissDrawerTab);
  const cloudOn = useSettingsStore((s) => s.settings?.visualisation?.embeddingCloud?.enabled ?? false);

  const baseId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const tabRefs = useRef<Partial<Record<DrawerTab, HTMLButtonElement | null>>>({});
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const prevTabRef = useRef<DrawerTab | null>(null);
  const [nodeLabel, setNodeLabel] = useState<string | null>(null);
  const [announce, setAnnounce] = useState('');
  // Mount the explore panel on first use, then keep it (it holds search state).
  const [exploreMounted, setExploreMounted] = useState(false);

  // Node selection drives the node tab. A null detail is a deselection.
  useEffect(() => {
    const onSelected = (e: Event) => {
      const detail = (e as CustomEvent<NodeSelected | null>).detail;
      if (detail) {
        const label = detail.label ? String(detail.label) : 'Selected node';
        setNodeLabel(label);
        setAnnounce(`Node details: ${label}`);
        openDrawer('node');
      } else {
        setNodeLabel(null);
        dismissDrawerTab('node');
      }
    };
    const onDeselect = () => {
      setNodeLabel(null);
      dismissDrawerTab('node');
    };
    window.addEventListener('visionclaw:node-selected', onSelected);
    window.addEventListener('visionclaw:node-deselect', onDeselect);
    return () => {
      window.removeEventListener('visionclaw:node-selected', onSelected);
      window.removeEventListener('visionclaw:node-deselect', onDeselect);
    };
  }, [openDrawer, dismissDrawerTab]);

  // The memory tab exists while the cloud is on. Switching the cloud on shows
  // it if the drawer is closed (the explorer used to appear on its own).
  const prevCloudRef = useRef(false);
  useEffect(() => {
    if (cloudOn && !prevCloudRef.current && useControlCenterUI.getState().drawerTab === null) {
      openDrawer('memory');
    }
    if (!cloudOn) dismissDrawerTab('memory');
    prevCloudRef.current = cloudOn;
  }, [cloudOn, openDrawer, dismissDrawerTab]);

  useEffect(() => {
    if (tab === 'explore') setExploreMounted(true);
  }, [tab]);

  // Focus: into the drawer when a control opened it; back out when it closes.
  useEffect(() => {
    const prev = prevTabRef.current;
    prevTabRef.current = tab;
    if (typeof document === 'undefined') return;
    const active = document.activeElement;
    if (prev === null && tab !== null) {
      const fromControl = !isIncidentalFocus(active) && !rootRef.current?.contains(active);
      returnFocusRef.current = fromControl ? (active as HTMLElement) : null;
      if (fromControl) {
        // After paint, so the tab button exists and is visible. The '/' hotkey
        // then moves focus on to the explore search box.
        requestAnimationFrame(() => {
          const insideAlready = rootRef.current?.contains(document.activeElement);
          if (!insideAlready) tabRefs.current[tab]?.focus();
        });
      }
    } else if (prev !== null && tab === null) {
      const target = returnFocusRef.current;
      returnFocusRef.current = null;
      const focusWasInside = !active || active === document.body || rootRef.current?.contains(active);
      if (target && target.isConnected && focusWasInside) target.focus();
    } else if (prev !== null && tab !== null && prev !== tab && rootRef.current?.contains(active)) {
      // Switching tabs from inside the drawer (e.g. closing the node tab back to
      // explore) keeps focus inside, on the new tab.
      requestAnimationFrame(() => tabRefs.current[tab]?.focus());
    }
  }, [tab]);

  const available: Record<DrawerTab, boolean> = {
    explore: true,
    memory: cloudOn,
    node: nodeLabel !== null,
  };
  const tabs = DRAWER_TABS.filter((t) => available[t.id]);

  const onTabKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLButtonElement>) => {
      if (!tab) return;
      const i = tabs.findIndex((t) => t.id === tab);
      let next = -1;
      if (e.key === 'ArrowRight') next = (i + 1) % tabs.length;
      else if (e.key === 'ArrowLeft') next = (i - 1 + tabs.length) % tabs.length;
      else if (e.key === 'Home') next = 0;
      else if (e.key === 'End') next = tabs.length - 1;
      if (next < 0) return;
      e.preventDefault();
      const id = tabs[next].id;
      openDrawer(id);
      tabRefs.current[id]?.focus();
    },
    [tab, tabs, openDrawer],
  );

  const onRootKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (e.key !== 'Escape' || popoverOwnsEscape()) return;
      e.preventDefault();
      e.stopPropagation();
      closeDrawer();
    },
    [closeDrawer],
  );

  const open = tab !== null;
  const tabId = (t: DrawerTab) => `${baseId}-tab-${t}`;
  const panelId = (t: DrawerTab) => `${baseId}-panel-${t}`;

  return (
    <div
      ref={rootRef}
      data-testid="right-drawer"
      data-drawer-tab={tab ?? ''}
      hidden={!open}
      // inert on the closed drawer: hidden already removes it from the a11y
      // tree; inert also guards the mounted-but-hidden panels' focusables.
      inert={!open}
      onKeyDown={onRootKeyDown}
      className="fixed flex flex-col gap-1.5"
      style={{
        right: 'var(--cc-edge, 1rem)',
        top: 'var(--cc-edge, 1rem)',
        bottom: 'var(--cc-dock-clearance, 9rem)',
        width: 'var(--cc-drawer-width, min(380px, calc(100vw - 2 * var(--cc-edge, 1rem))))',
        zIndex: 'var(--cc-z-chrome, 40)' as unknown as number,
        pointerEvents: 'auto',
        display: open ? 'flex' : 'none',
      }}
    >
      {/* Close sits on the left: the top-right corner belongs to the connection
          badge while the backend is unreachable. */}
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          data-testid="right-drawer-close"
          aria-label={tab === 'node' ? 'Close node details' : 'Close side panel'}
          onClick={closeDrawer}
          className="cc-glass flex items-center justify-center h-7 w-7 shrink-0 rounded-full text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X size={14} aria-hidden="true" />
        </button>
        <div role="tablist" aria-label="Side panel" className="cc-glass flex items-center gap-0.5 rounded-full p-0.5">
          {tabs.map((t) => {
            const selected = tab === t.id;
            return (
              <button
                key={t.id}
                ref={(el) => { tabRefs.current[t.id] = el; }}
                type="button"
                role="tab"
                id={tabId(t.id)}
                data-testid={`drawer-tab-${t.id}`}
                aria-selected={selected}
                aria-controls={panelId(t.id)}
                tabIndex={selected ? 0 : -1}
                title={t.id === 'node' && nodeLabel ? nodeLabel : undefined}
                onClick={() => openDrawer(t.id)}
                onKeyDown={onTabKeyDown}
                className={
                  'px-2.5 py-1 rounded-full text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ' +
                  (selected ? 'bg-white/15 text-foreground' : 'text-muted-foreground hover:text-foreground')
                }
              >
                {t.label}
              </button>
            );
          })}
        </div>
      </div>

      <span className="sr-only" aria-live="polite" data-testid="right-drawer-announce">{announce}</span>

      <section
        role="tabpanel"
        id={panelId('explore')}
        aria-labelledby={tabId('explore')}
        hidden={tab !== 'explore'}
        data-testid="drawer-panel-explore"
        className="flex-1 min-h-0 overflow-y-auto"
      >
        <PanelBoundary name="explore" resetKey={String(tab === 'explore')}>
          {exploreMounted && <ExplorePanel style={{ width: '100%' }} />}
        </PanelBoundary>
      </section>

      {cloudOn && (
        <section
          role="tabpanel"
          id={panelId('memory')}
          aria-labelledby={tabId('memory')}
          hidden={tab !== 'memory'}
          data-testid="drawer-panel-memory"
          className="flex-1 min-h-0 overflow-y-auto"
        >
          <PanelBoundary name="memory explorer" resetKey={String(tab === 'memory')}>
            <MemoryExplorerPanel embedded />
          </PanelBoundary>
        </section>
      )}

      <section
        role="tabpanel"
        id={panelId('node')}
        aria-labelledby={nodeLabel ? tabId('node') : undefined}
        aria-label={nodeLabel ? undefined : 'Node details'}
        hidden={tab !== 'node'}
        data-testid="drawer-panel-node"
        className="cc-glass flex-1 min-h-0 overflow-hidden rounded-xl"
      >
        {/* Always mounted: it listens for the selection that opens this tab. */}
        <PanelBoundary name="node detail" resetKey={nodeLabel ?? ''}>
          <NodeDetailPanel embedded />
        </PanelBoundary>
      </section>
    </div>
  );
};

export default RightDrawer;
