/**
 * useControlCenterHotkeys — the keyboard map. design-spec.md §6.2.
 *
 * | Key            | Action                                                    |
 * |----------------|-----------------------------------------------------------|
 * | Cmd/Ctrl+K     | (owned by CommandPalette — NOT bound here, no double-bind)|
 * | 1–7            | open SettingsPanel to that section (ADR-2189, registry order) |
 * | /              | Explore: the explore panel (or Settings › Explore) + search |
 * | Cmd/Ctrl+.     | toggle dock (hero mode)                                   |
 * | Esc            | close the SettingsPanel, else the explore drawer          |
 * | ?              | open the generated shortcut sheet                         |
 * | ←/→ ↑/↓        | (native to the focused dial/slider — not intercepted)     |
 *
 * Digit, '/' and '?' hotkeys are suppressed while a text field / CommandInput is focused.
 * The same map is listed by help/ShortcutSheet from help/controlCenterBindings.
 */

import { useEffect } from 'react';
import { REGISTRY } from '../registry/settingsRegistry';
import { useControlCenterUI } from '../state/useControlCenterUI';
/** Focus the explore panel's search box (ADR-2185). */
function focusExploreSearch(): void {
  const run = () => {
    const el = document.querySelector<HTMLInputElement>('[data-testid="explore-drawer"] input[type="search"]');
    el?.focus();
  };
  if (typeof requestAnimationFrame === 'function') requestAnimationFrame(() => requestAnimationFrame(run));
  else run();
}

function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  return target.isContentEditable;
}

export function useControlCenterHotkeys(): void {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const ui = useControlCenterUI.getState();

      // Cmd/Ctrl+. → dock hero-mode toggle.
      if ((e.metaKey || e.ctrlKey) && e.key === '.') {
        e.preventDefault();
        ui.toggleDock();
        return;
      }

      // Cmd/Ctrl+K is the CommandPalette's — never double-bind. Ignore any other
      // modified chord so app/OS shortcuts pass through untouched.
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      // Esc → close the top-most Control Center surface (the panel). The palette
      // owns its own Esc; the dock has an explicit toggle, so Esc is scoped to
      // the panel and left inert otherwise (canvas/global Esc handlers survive).
      if (e.key === 'Escape') {
        // Focus inside the right-hand drawer: the drawer owns Esc (its own
        // bubble-phase handler closes the active tab). This listener runs in the
        // capture phase, so without this it would close the settings panel, or
        // close the drawer a second time (node tab → previous tab → closed).
        if (e.target instanceof Element && e.target.closest('[data-testid="right-drawer"]')) return;
        if (ui.openPanel) {
          // A Radix Select dropdown (or any popper-positioned overlay) open inside
          // the panel owns Escape first — it should close only itself, not the whole
          // panel. The design-system Select renders position="popper" content, so an
          // open dropdown is present as a [data-radix-popper-content-wrapper] with a
          // role="listbox"/data-state="open" body. If one is mounted, bail and let
          // Radix's own Escape handler dismiss the dropdown. See defect-2.
          if (
            typeof document !== 'undefined' &&
            document.querySelector('[data-radix-popper-content-wrapper], [role="listbox"], [role="dialog"][data-state="open"]')
          ) {
            return;
          }
          e.preventDefault();
          ui.closePanel();
          return;
        }
        // Then the right-hand drawer (same rule: an open popper or dialog owns Escape).
        if (
          ui.drawerTab !== null &&
          !(typeof document !== 'undefined' &&
            document.querySelector('[data-radix-popper-content-wrapper], [role="listbox"], [role="dialog"][data-state="open"]'))
        ) {
          e.preventDefault();
          ui.closeDrawer();
        }
        return;
      }

      // Everything below is suppressed while typing.
      if (isTextEntry(e.target)) return;

      // 1–7 → open the section whose hotkey this is (generated from registry order).
      const section = REGISTRY.find((g) => g.hotkey === e.key);
      if (section) {
        e.preventDefault();
        ui.openGroup(section.id);
        return;
      }

      // / → open the explore panel and focus its search.
      if (e.key === '/') {
        e.preventDefault();
        ui.openDrawer('explore');
        focusExploreSearch();
        return;
      }

      // ? → the shortcut sheet. Opened directly (and also broadcast, for any
      // other help surface that listens).
      if (e.key === '?') {
        e.preventDefault();
        ui.setShortcutSheetOpen(true);
        window.dispatchEvent(new CustomEvent('controlcenter:help'));
      }
    };

    // Capture phase: Radix's document-level Escape handler removes the open
    // popper synchronously before bubble-phase listeners run, so a bubble
    // listener's open-popover probe always misses and Escape collapses both
    // the dropdown and the panel. Capturing first sees the popper while it is
    // still mounted and bails, leaving the dropdown dismissal to Radix.
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, []);
}
