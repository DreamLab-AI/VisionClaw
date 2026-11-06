/**
 * Settings changes this browser holds that the server does not have yet.
 *
 * The auto-save path used to drop such changes with a debug log only: a change made
 * before the store initialised, while sync was off (local-only mode), or whose PUT
 * failed. They are recorded here instead, so
 *  - the status chip shows the user how many changes are not saved on the server;
 *  - the next load knows these local values are newer than the server's and keeps them
 *    (coreSlice.initialize), while every other server-stored value wins over a stale copy
 *    in this browser.
 * Persisted to its own localStorage key; a storage failure only loses the notice.
 */
import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';

/** Why a change is not on the server. */
export type UnsavedReason = 'not-ready' | 'sync-off' | 'failed';

interface SettingsSyncStatusState {
  /** path → why it is not on the server */
  unsaved: Record<string, UnsavedReason>;
  markUnsaved: (paths: Iterable<string>, reason: UnsavedReason) => void;
  markSaved: (paths: Iterable<string>) => void;
  clear: () => void;
}

export const UNSYNCED_STORAGE_KEY = 'graph-viz-settings-unsynced';

export const useSettingsSyncStatus = create<SettingsSyncStatusState>()(
  persist(
    (set) => ({
      unsaved: {},
      markUnsaved: (paths, reason) =>
        set((s) => {
          const unsaved = { ...s.unsaved };
          for (const p of paths) unsaved[p] = reason;
          return { unsaved };
        }),
      markSaved: (paths) =>
        set((s) => {
          const unsaved = { ...s.unsaved };
          let changed = false;
          for (const p of paths) {
            if (p in unsaved) {
              delete unsaved[p];
              changed = true;
            }
          }
          return changed ? { unsaved } : s;
        }),
      clear: () => set({ unsaved: {} }),
    }),
    {
      name: UNSYNCED_STORAGE_KEY,
      storage: createJSONStorage(() => localStorage),
      partialize: (s) => ({ unsaved: s.unsaved }),
    },
  ),
);

/** True when `path`, an ancestor of it, or a descendant of it holds an unsaved change. */
export function isUnsyncedPath(path: string): boolean {
  const unsaved = useSettingsSyncStatus.getState().unsaved;
  if (path in unsaved) return true;
  for (const p of Object.keys(unsaved)) {
    if (p.startsWith(`${path}.`) || path.startsWith(`${p}.`)) return true;
  }
  return false;
}

/** One line for the status chip's tooltip, e.g. "2 failed to save, 1 kept in this browser (sync off)". */
export function describeUnsaved(unsaved: Record<string, UnsavedReason>): string {
  const n = { failed: 0, 'sync-off': 0, 'not-ready': 0 } as Record<UnsavedReason, number>;
  for (const r of Object.values(unsaved)) n[r] += 1;
  const parts: string[] = [];
  if (n.failed) parts.push(`${n.failed} failed to save to the server`);
  if (n['sync-off']) parts.push(`${n['sync-off']} kept in this browser only (sync is off)`);
  if (n['not-ready']) parts.push(`${n['not-ready']} waiting for the settings to load`);
  return parts.join('; ');
}
