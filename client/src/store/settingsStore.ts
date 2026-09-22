import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'
import { SettingsState } from './settings/settingsTypes'
import { createCoreSlice } from './settings/coreSlice'
import { createPhysicsSlice } from './settings/physicsSlice'
import { createPersistenceSlice } from './settings/persistenceSlice'
import { collectPathsFromSettings, migrateGraphSettingsKey } from './settings/settingsHelpers'
import { createLogger } from '../utils/loggerConfig'
import { debugState } from '../utils/clientDebugState'
import {
  getSectionPaths,
  setNestedValue,
  getAllSettingsPaths,
  getAllAvailableSettingsPaths,
} from './settings/settingsHelpers'

export type { SettingsState }
export type {
  GPUPhysicsParams,
  ConstraintConfig,
  WarmupSettings,
} from './settings/settingsTypes'

const logger = createLogger('SettingsStore')

export const useSettingsStore = create<SettingsState>()(
  persist(
    (...a) => ({
      ...createCoreSlice(...a),
      ...createPhysicsSlice(...a),
      ...createPersistenceSlice(...a),
    }),
    {
      name: 'graph-viz-settings-v2',
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        // Auth state
        authenticated: state.authenticated,
        user: state.user,
        isPowerUser: state.isPowerUser,
        // Persist ALL settings so visual settings (edges, glow, hologram, graphTypeVisuals, etc.)
        // survive page reloads. The server only handles a subset of categories, so localStorage
        // is the primary persistence layer for client-side visual settings.
        partialSettings: state.partialSettings,
      }),
      merge: (persistedState: unknown, currentState: SettingsState): SettingsState => {
        if (!persistedState) return currentState;
        const persisted = persistedState as Record<string, unknown>;

        // Deep-merge persisted partialSettings into the current state so that
        // server-fetched values take priority during initialize(), but any settings
        // that were only stored locally are restored from localStorage.
        // Receive-side migration (ADR-2041 rename; server alias retired by
        // ADR-2115): map a persisted `visualisation.graphs.logseq` object onto
        // `visualisation.graphs.knowledge` before it reaches the store.
        const persistedSettings = migrateGraphSettingsKey(
          (persisted.partialSettings as import('./settings/settingsTypes').DeepPartial<import('../features/settings/config/settings').Settings>) || {},
        );

        return {
          ...currentState,
          authenticated: (persisted.authenticated as boolean) || false,
          user: (persisted.user as SettingsState['user']) || null,
          isPowerUser: (persisted.isPowerUser as boolean) || false,
          partialSettings: persistedSettings,
          settings: persistedSettings,
          loadedPaths: collectPathsFromSettings(persistedSettings),
        };
      },
      onRehydrateStorage: () => (state) => {
        if (state) {
          if (debugState.isEnabled()) {
            logger.info('Settings store rehydrated from storage with persisted visual settings');
          }
        }
      }
    }
  )
)

// Export for testing and direct access
export const settingsStoreUtils = {
  getSectionPaths,
  setNestedValue,
  getAllSettingsPaths,
  getAllAvailableSettingsPaths
};
