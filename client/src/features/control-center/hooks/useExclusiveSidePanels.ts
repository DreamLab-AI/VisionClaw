/**
 * useExclusiveSidePanels — the settings panel (left) and the right-hand drawer
 * never overlap (ADR-2189 §3/§4).
 *
 * When both fit side by side they stay open together. When they do not — a
 * narrow window (below about 830 px at default widths) or a settings panel
 * resized wide (it goes up to 900 px) — they become mutually exclusive and the
 * one opened last wins:
 *  - opening a drawer tab closes the settings panel;
 *  - opening a settings section, or dragging the panel wider, hides the drawer.
 * Hiding the drawer this way does not deselect the node or forget its tab:
 * `previousDrawerTab` keeps it, so the dock's Explore button or the next node
 * selection brings it back.
 * A window resize that makes an open pair stop fitting closes the settings
 * panel (the drawer usually holds what the user just asked for: a node).
 */
import { useEffect } from 'react';
import { useControlCenterUI, PANEL_DEFAULT_WIDTH, type ControlCenterUIState } from '../state/useControlCenterUI';

/** Side margin and the minimum gap between the two surfaces (px); matches --cc-edge. */
const EDGE_PX = 16;
/** Drawer width cap; matches --cc-drawer-width: min(380px, 100vw - 2 * edge). */
const DRAWER_MAX_PX = 380;

/**
 * Settings panel width. The stored width, not the DOM's: during a drag the
 * store is updated before the panel re-renders, so the DOM still reports the
 * old width when this runs (the rendered panel is exactly panelWidth wide).
 */
function settingsPanelWidth(ui: ControlCenterUIState): number {
  return ui.panelWidth || PANEL_DEFAULT_WIDTH;
}

/** True when the settings panel and the drawer cannot both be shown without overlapping. */
export function sidePanelsCollide(ui: ControlCenterUIState, viewportWidth: number): boolean {
  const drawer = Math.min(DRAWER_MAX_PX, viewportWidth - 2 * EDGE_PX);
  const needed = EDGE_PX + settingsPanelWidth(ui) + EDGE_PX + drawer + EDGE_PX;
  return needed > viewportWidth;
}

export function useExclusiveSidePanels(): void {
  useEffect(() => {
    const viewport = () => (typeof window !== 'undefined' ? window.innerWidth : Infinity);

    const unsubscribe = useControlCenterUI.subscribe((s, prev) => {
      if (!s.openPanel || s.drawerTab === null) return;
      if (!sidePanelsCollide(s, viewport())) return;
      const drawerJustOpened = s.drawerTab !== prev.drawerTab;
      const panelJustOpened = s.openPanel !== prev.openPanel || s.activeGroup !== prev.activeGroup;
      const panelResized = s.panelWidth !== prev.panelWidth;
      if (drawerJustOpened) {
        // Drawer opened last (or both at once): it wins.
        s.closePanel();
      } else if (panelJustOpened || panelResized) {
        // Settings opened last, or dragged wider while in use: hide the drawer and
        // keep its tab to come back to (no deselect).
        useControlCenterUI.setState({ drawerTab: null, previousDrawerTab: s.drawerTab });
      }
      // Any other state change while both are open leaves them alone.
    });

    const onResize = () => {
      const s = useControlCenterUI.getState();
      if (s.openPanel && s.drawerTab !== null && sidePanelsCollide(s, viewport())) s.closePanel();
    };
    window.addEventListener('resize', onResize);
    return () => {
      unsubscribe();
      window.removeEventListener('resize', onResize);
    };
  }, []);
}
