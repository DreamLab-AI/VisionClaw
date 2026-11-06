/**
 * The one client write path for settings (ADR-2189 §6 "one writer per setting").
 *
 * Every surface that changes a setting — a registry row (useSettingField), a dock
 * dial (useMacro) and the Ask box (CommandInput) — calls `applySettingWrites`, so
 * all of them:
 *  - write the settings store in ONE immer batch (one fan-out to the renderer);
 *  - queue each path on autoSaveManager, which routes it to its server bucket
 *    (physics, rendering, visual …) and honours the Sync switch;
 *  - trigger the layout-mode side effect (layoutApi.setMode) exactly as the
 *    registry row always has.
 * Before this, CommandInput PUT `/api/settings/physics` directly: the store did
 * not see the change, the Sync switch was bypassed and the layout side effect
 * never ran (docs/audits/menus-2026-11-06.md finding 8).
 */
import { useSettingsStore } from '../../../store/settingsStore';
import { autoSaveManager } from '../../../store/autoSaveManager';
import { layoutApi } from '../../../api/layoutApi';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('settingWrites');

export interface SettingWrite {
  path: string;
  value: unknown;
}

/** Paths whose change must also tell the server to run the layout transition. */
export const LAYOUT_SETTING_PATHS: ReadonlySet<string> = new Set([
  'visualisation.graphs.knowledge.physics.layoutMode',
  'visualisation.graphs.knowledge.physics.layoutAlgorithm',
]);

/** Prefix for writes into the shared physics document. */
export const PHYSICS_PREFIX = 'visualisation.graphs.knowledge.physics.';

/** `{ repelK: 120 }` → `[{ path: '…physics.repelK', value: 120 }]`. */
export function physicsWrites(body: Record<string, unknown>): SettingWrite[] {
  return Object.entries(body).map(([key, value]) => ({ path: `${PHYSICS_PREFIX}${key}`, value }));
}

export function applySettingWrites(writes: readonly SettingWrite[]): void {
  const valid = writes.filter((w) => w.path);
  if (!valid.length) return;

  useSettingsStore.getState().updateSettings((draft) => {
    const root = draft as unknown as Record<string, unknown>;
    for (const { path, value } of valid) {
      const keys = path.split('.');
      let cur = root;
      for (let i = 0; i < keys.length - 1; i++) {
        const next = cur[keys[i]];
        if (!next || typeof next !== 'object') cur[keys[i]] = {};
        cur = cur[keys[i]] as Record<string, unknown>;
      }
      cur[keys[keys.length - 1]] = value;
    }
  });

  // updateSettings() already queues via autoSaveManager.queueChanges(); the
  // explicit per-path queue is kept (as the registry row always did) so each
  // write lands in the same debounced PUT bucket as a manual edit of that field.
  for (const { path, value } of valid) {
    autoSaveManager.queueChange(path, value);
    if (LAYOUT_SETTING_PATHS.has(path) && typeof value === 'string') {
      layoutApi.setMode(value, 800).catch((err) => {
        logger.warn('[settingWrites] layoutApi.setMode failed:', err);
      });
    }
  }
}
