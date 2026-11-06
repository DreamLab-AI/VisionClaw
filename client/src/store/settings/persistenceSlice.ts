import { StateCreator } from 'zustand'
import { createLogger, createErrorMetadata } from '../../utils/loggerConfig'
import { settingsApi } from '../../api/settingsApi'
import { useSettingsSyncStatus } from '../settingsSyncStatus'
import { serverBucketForPath } from '../../api/settings/schemaMappings'

/** Record a direct write's outcome (setByPath / batchUpdate bypass the auto-save queue). */
function recordWrite(paths: string[], ok: boolean): void {
  const server = paths.filter((p) => serverBucketForPath(p) !== null)
  if (server.length === 0) return
  const status = useSettingsSyncStatus.getState()
  if (ok) status.markSaved(server)
  else status.markUnsaved(server, 'failed')
}
import { SettingsState, SettingsPath } from './settingsTypes'
import {
  getAllAvailableSettingsPaths,
  getAllSettingsPaths,
  setNestedValue,
} from './settingsHelpers'

const logger = createLogger('SettingsStore')

export type PersistenceSlice = Pick<
  SettingsState,
  | 'getByPath'
  | 'setByPath'
  | 'batchUpdate'
  | 'flushPendingUpdates'
  | 'resetSettings'
  | 'exportSettings'
  | 'importSettings'
>

export const createPersistenceSlice: StateCreator<SettingsState, [], [], PersistenceSlice> = (set, get) => ({
  getByPath: async <T>(path: SettingsPath): Promise<T> => {
    try {
      const value = await settingsApi.getSettingByPath(path);
      return value as T;
    } catch (error) {
      logger.error(`Failed to get setting by path ${path}:`, createErrorMetadata(error));
      const localValue = get().get<T>(path);
      return localValue as T;
    }
  },

  setByPath: <T>(path: SettingsPath, value: T) => {
    const state = get();

    // Update Zustand state without triggering server sync from set()
    state.set(path, value, true);

    // Single server write from setByPath
    settingsApi.updateSettingByPath(path, value).then(() => recordWrite([path], true)).catch(error => {
      recordWrite([path], false)
      logger.error(`Failed to update setting ${path}:`, createErrorMetadata(error));
    });
  },

  batchUpdate: (updates: Array<{path: SettingsPath, value: unknown}>) => {
    // Accumulate all changes locally without triggering per-key server writes
    set(state => {
      const newPartialSettings = { ...state.partialSettings };
      const newLoadedPaths = new Set(state.loadedPaths);
      for (const { path, value } of updates) {
        setNestedValue(newPartialSettings as Record<string, unknown>, path, value);
        newLoadedPaths.add(path);
      }
      return {
        partialSettings: newPartialSettings,
        settings: newPartialSettings,
        loadedPaths: newLoadedPaths
      };
    });

    // Single batched server write
    const paths = updates.map(u => u.path as string)
    settingsApi.updateSettingsByPaths(updates.map(u => ({ path: u.path, value: u.value }))).then(() => recordWrite(paths, true)).catch(error => {
      recordWrite(paths, false)
      logger.error('Failed to batch update settings:', createErrorMetadata(error));
    });
  },

  flushPendingUpdates: async (): Promise<void> => {
    await settingsApi.flushPendingUpdates();
  },

  resetSettings: async (): Promise<void> => {
    try {
      await settingsApi.resetSettings();

      set({
        partialSettings: {},
        settings: {},
        loadedPaths: new Set()
      });

      await get().initialize();

      logger.info('Settings reset to defaults and essential paths reloaded');
    } catch (error) {
      logger.error('Failed to reset settings:', createErrorMetadata(error));
      throw error;
    }
  },

  exportSettings: async (): Promise<string> => {
    const { partialSettings, loadedPaths } = get();
    const { ESSENTIAL_PATHS } = await import('./settingsHelpers');

    try {
      if (loadedPaths.size === ESSENTIAL_PATHS.length) {
        logger.info('Only essential settings loaded, fetching all settings for export...');

        const allPaths = getAllAvailableSettingsPaths();
        const allSettings = await settingsApi.getSettingsByPaths(allPaths);

        return settingsApi.exportSettings(allSettings);
      } else {
        return settingsApi.exportSettings(partialSettings as unknown as Record<string, unknown>);
      }
    } catch (error) {
      logger.error('Failed to export settings:', createErrorMetadata(error));
      throw error;
    }
  },

  importSettings: async (jsonString: string): Promise<void> => {
    try {
      const importedSettings = settingsApi.importSettings(jsonString);

      const allPaths = getAllSettingsPaths(importedSettings);
      const updates: Array<{path: string, value: unknown}> = [];

      for (const path of allPaths) {
        const value = path.split('.').reduce<unknown>((obj, key) => (obj as Record<string, unknown>)?.[key], importedSettings);
        if (value !== undefined) {
          updates.push({ path, value });
        }
      }

      get().batchUpdate(updates);

      logger.info(`Successfully imported ${updates.length} settings using path-based updates`);
    } catch (error) {
      logger.error('Failed to import settings:', createErrorMetadata(error));
      throw error;
    }
  },
})
