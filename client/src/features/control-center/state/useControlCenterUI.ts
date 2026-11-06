/**
 * useControlCenterUI — ephemeral UI state for the Control Center shell.
 * design-spec.md §4.3.
 *
 * Deliberately SEPARATE from the settings store: this is view state (which
 * surface is open, dock collapsed, echo-pulse flag) that must NEVER be
 * persisted to the backend, so it can't leak into the frozen 168-path
 * settings contract (§7 overrule 4). `echoPulseEnabled` is the Echo Pulse
 * feature flag — a UI concern, not a settings path — defaulting `true` and
 * forced `false` while the user prefers reduced motion.
 */

import { create } from 'zustand';

/* ---------------------------------------------------------------------- */
/* Docked-panel width (user-resizable via the GlassPanel edge handle).     */
/*                                                                         */
/* Persisted to localStorage ONLY — this is a client-side view preference, */
/* NOT a settings path, so it never touches the frozen 168-path backend    */
/* contract (§7 overrule 4). Clamped so a stale/tampered value can't strand */
/* the panel off-screen or narrower than its rail.                         */
/* ---------------------------------------------------------------------- */
export const PANEL_MIN_WIDTH = 320;
export const PANEL_MAX_WIDTH = 900;
export const PANEL_DEFAULT_WIDTH = 380;
const PANEL_WIDTH_STORAGE_KEY = 'controlCenter.panelWidth';

export function clampPanelWidth(width: number): number {
  if (!Number.isFinite(width)) return PANEL_DEFAULT_WIDTH;
  return Math.min(PANEL_MAX_WIDTH, Math.max(PANEL_MIN_WIDTH, Math.round(width)));
}

function readStoredPanelWidth(): number {
  if (typeof window === 'undefined' || !window.localStorage) return PANEL_DEFAULT_WIDTH;
  try {
    const raw = window.localStorage.getItem(PANEL_WIDTH_STORAGE_KEY);
    if (raw == null) return PANEL_DEFAULT_WIDTH;
    const n = Number.parseInt(raw, 10);
    return Number.isFinite(n) ? clampPanelWidth(n) : PANEL_DEFAULT_WIDTH;
  } catch {
    return PANEL_DEFAULT_WIDTH;
  }
}

function persistPanelWidth(width: number): void {
  if (typeof window === 'undefined' || !window.localStorage) return;
  try {
    window.localStorage.setItem(PANEL_WIDTH_STORAGE_KEY, String(width));
  } catch {
    /* storage unavailable / quota exceeded — width stays session-only */
  }
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  try {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  } catch {
    return false;
  }
}

export interface ControlCenterUIState {
  /** Whether the slide-out SettingsPanel is open. */
  openPanel: boolean;
  /** Active rail entry: a section id ('layers'…'system', ADR-2189) or a bespoke
   *  panel id ('solid' | 'ontology'). Null when nothing is selected. */
  activeGroup: string | null;
  /** Dock collapsed to a single pill (Cmd/Ctrl+. hero mode). */
  dockCollapsed: boolean;
  /** Echo Pulse feature flag (forced false under prefers-reduced-motion). */
  echoPulseEnabled: boolean;
  /** Persisted width (px) of the docked SettingsPanel. User-resizable via the
   *  GlassPanel edge handle; clamped to [PANEL_MIN_WIDTH, PANEL_MAX_WIDTH]. */
  panelWidth: number;
  /** ADR-2185 explore panel (right drawer) open; only used once gv-explore supplies it. */
  explorePanelOpen: boolean;
  /** Generated keyboard-shortcut sheet open ('?'). */
  shortcutSheetOpen: boolean;

  /** Open the panel to a group/panel id (used by hotkeys, rail, reveal). */
  openGroup: (id: string) => void;
  /** Close the slide-out panel (keeps the last activeGroup for re-open). */
  closePanel: () => void;
  /** Toggle a group open/closed by id. */
  togglePanel: (id: string) => void;
  setActiveGroup: (id: string | null) => void;
  toggleDock: () => void;
  setDockCollapsed: (collapsed: boolean) => void;
  setEchoPulseEnabled: (enabled: boolean) => void;
  /** Set + persist the docked panel width (clamped). */
  setPanelWidth: (width: number) => void;
  setExplorePanelOpen: (open: boolean) => void;
  setShortcutSheetOpen: (open: boolean) => void;
}

export const useControlCenterUI = create<ControlCenterUIState>((set) => ({
  openPanel: false,
  activeGroup: null,
  dockCollapsed: false,
  echoPulseEnabled: !prefersReducedMotion(),
  panelWidth: readStoredPanelWidth(),
  explorePanelOpen: false,
  shortcutSheetOpen: false,

  openGroup: (id) => set({ openPanel: true, activeGroup: id }),
  closePanel: () => set({ openPanel: false }),
  togglePanel: (id) =>
    set((s) =>
      s.openPanel && s.activeGroup === id
        ? { openPanel: false }
        : { openPanel: true, activeGroup: id },
    ),
  setActiveGroup: (id) => set({ activeGroup: id }),
  toggleDock: () => set((s) => ({ dockCollapsed: !s.dockCollapsed })),
  setDockCollapsed: (collapsed) => set({ dockCollapsed: collapsed }),
  setEchoPulseEnabled: (enabled) => set({ echoPulseEnabled: enabled }),
  setExplorePanelOpen: (open) => set({ explorePanelOpen: open }),
  setShortcutSheetOpen: (open) => set({ shortcutSheetOpen: open }),
  setPanelWidth: (width) => {
    const w = clampPanelWidth(width);
    persistPanelWidth(w);
    set({ panelWidth: w });
  },
}));

/**
 * Live reduced-motion binding: when the user's motion preference flips, the
 * echo-pulse flag follows it. Registered once at module load; fail-open so a
 * missing/broken matchMedia never throws during import (jsdom, older engines).
 */
if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
  try {
    const mq = window.matchMedia('(prefers-reduced-motion: reduce)');
    const apply = () => useControlCenterUI.getState().setEchoPulseEnabled(!mq.matches);
    if (typeof mq.addEventListener === 'function') mq.addEventListener('change', apply);
    else if (typeof mq.addListener === 'function') mq.addListener(apply);
  } catch {
    /* no-op: reduced-motion binding is best-effort */
  }
}
