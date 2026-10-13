import { StateCreator } from 'zustand'
import { produce } from 'immer'
import { createLogger, createErrorMetadata } from '../../utils/loggerConfig'
import { debugState } from '../../utils/clientDebugState'
import { isViewportSetting } from '../../features/settings/config/viewportSettings'
import { settingsApi } from '../../api/settingsApi'
import { nostrAuth } from '../../services/nostrAuthService'
import { autoSaveManager } from '../autoSaveManager'
import { Settings, SettingsPath } from '../../features/settings/config/settings'
import { SettingsState, ConstraintConfig } from './settingsTypes'
import {
  ESSENTIAL_PATHS,
  waitForAuthReady,
  deepMergeSettings,
  findChangedPaths,
  setNestedValue,
  getSectionPaths,
} from './settingsHelpers'
import {
  getOrCreateTrieNode,
  collectMatchedCallbacks,
  scheduleNotify,
} from './subscriberTrie'

const logger = createLogger('SettingsStore')

/** Read the value at a dot-path within a settings object (undefined if absent). */
function getValueFromPathLocal(obj: unknown, path: string): unknown {
  if (!path) return undefined;
  let current: unknown = obj;
  for (const key of path.split('.')) {
    if (current === null || typeof current !== 'object') return undefined;
    current = (current as Record<string, unknown>)[key];
  }
  return current;
}

export type CoreSlice = Pick<
  SettingsState,
  | 'partialSettings'
  | 'settings'
  | 'loadedPaths'
  | 'loadingSections'
  | 'initialized'
  | 'authenticated'
  | 'user'
  | 'isPowerUser'
  | 'settingsSyncEnabled'
  | 'setSettingsSyncEnabled'
  | 'initialize'
  | 'setAuthenticated'
  | 'setUser'
  | 'get'
  | 'set'
  | 'subscribe'
  | 'unsubscribe'
  | 'updateSettings'
  | 'notifyViewportUpdate'
  | 'ensureLoaded'
  | 'loadSection'
  | 'isLoaded'
  | 'updateComputeMode'
  | 'updateConstraints'
>

export const createCoreSlice: StateCreator<SettingsState, [], [], CoreSlice> = (set, get) => ({
  partialSettings: {},
  settings: {},
  loadedPaths: new Set(),
  loadingSections: new Set(),
  initialized: false,
  authenticated: false,
  user: null,
  isPowerUser: false,
  settingsSyncEnabled: true,

  setSettingsSyncEnabled: (enabled: boolean) => {
    set({ settingsSyncEnabled: enabled });
    autoSaveManager.setSyncEnabled(enabled);
  },

  initialize: async () => {
    try {
      logger.info('[SettingsStore] Starting initialization with essential paths');
      if (debugState.isEnabled()) {
        logger.info('Initializing settings store with essential paths only')
      }

      await waitForAuthReady();

      const isAuthenticated = nostrAuth.isAuthenticated();
      const user = nostrAuth.getCurrentUser();

      logger.info('Settings initialization with auth state', {
        authenticated: isAuthenticated,
        user: user?.pubkey?.slice(0, 8) + '...',
      });

      logger.info('[SettingsStore] Calling settingsApi.getSettingsByPaths');
      const essentialSettings = await settingsApi.getSettingsByPaths(ESSENTIAL_PATHS);
      logger.info('[SettingsStore] Essential settings loaded successfully');

      if (debugState.isEnabled()) {
        logger.info('Essential settings loaded:', { essentialSettings })
      }

      set(state => {
        // Deep merge: server-fetched settings provide defaults and authoritative
        // values as the base. localStorage-persisted customizations overlay on top
        // so user tweaks (graphTypeVisuals, glow, edges, etc.) survive reloads.
        const merged = deepMergeSettings(
          essentialSettings as Record<string, unknown>,
          state.partialSettings as Record<string, unknown>,
        ) as import('./settingsTypes').DeepPartial<Settings>;

        // For physics and tweening paths, server values are authoritative.
        // localStorage may hold stale values that conflict with server-side
        // tuning, so we overlay the server-fetched essential values back on top.
        const essRec = essentialSettings as Record<string, unknown>;
        const mergedRec = merged as Record<string, unknown>;
        const essVis = essRec.visualisation as Record<string, unknown> | undefined;
        const mergedVis = mergedRec.visualisation as Record<string, unknown> | undefined;
        if (essVis && mergedVis) {
          const essGraphs = essVis.graphs as Record<string, Record<string, unknown>> | undefined;
          const mergedGraphs = mergedVis.graphs as Record<string, Record<string, unknown>> | undefined;
          if (essGraphs?.knowledge?.tweening && mergedGraphs?.knowledge) {
            mergedGraphs.knowledge.tweening = essGraphs.knowledge.tweening;
          }
          if (essGraphs?.knowledge?.physics && mergedGraphs?.knowledge) {
            mergedGraphs.knowledge.physics = essGraphs.knowledge.physics;
          }
          // Node visibility and core visual settings are server-authoritative
          // to prevent stale localStorage from hiding graph types or making
          // nodes invisible for new sessions inheriting shared state.
          if (essGraphs?.knowledge?.nodes && mergedGraphs?.knowledge) {
            const essNodes = essGraphs.knowledge.nodes as Record<string, unknown>;
            const mergedNodes = (mergedGraphs.knowledge.nodes || {}) as Record<string, unknown>;
            // Server wins for: visibility toggles, opacity, size
            if (essNodes.nodeTypeVisibility) mergedNodes.nodeTypeVisibility = essNodes.nodeTypeVisibility;
            if (essNodes.opacity !== undefined) mergedNodes.opacity = essNodes.opacity;
            if (essNodes.nodeSize !== undefined) mergedNodes.nodeSize = essNodes.nodeSize;
            mergedGraphs.knowledge.nodes = mergedNodes;
          }
        }
        if (essRec.clientTweening !== undefined && essRec.clientTweening !== null) {
          mergedRec.clientTweening = essRec.clientTweening;
        }

        return {
          partialSettings: merged,
          settings: merged,
          loadedPaths: new Set([...state.loadedPaths, ...ESSENTIAL_PATHS]),
          initialized: true,
          authenticated: isAuthenticated,
          user: user ? { isPowerUser: user.isPowerUser, pubkey: user.pubkey } : null,
          isPowerUser: user?.isPowerUser || false,
        };
      });

      autoSaveManager.setInitialized(true);

      if (debugState.isEnabled()) {
        logger.info('Settings store initialized with essential paths')
      }

    } catch (error) {
      logger.error('[SettingsStore] Failed to initialize:', createErrorMetadata(error));
      logger.error('Failed to initialize settings store:', createErrorMetadata(error))
      set({ initialized: false })
      throw error
    }
  },

  setAuthenticated: (authenticated: boolean) => set({ authenticated }),

  setUser: (user: { isPowerUser: boolean; pubkey: string } | null) => set({
    user,
    isPowerUser: user?.isPowerUser || false
  }),

  notifyViewportUpdate: (_path: SettingsPath) => {
    const node = getOrCreateTrieNode('viewport.update', false);
    if (node && node.subscribers.size) {
      for (const callback of node.subscribers) {
        try {
          callback();
        } catch (error) {
          logger.error(`Error in viewport update subscriber:`, createErrorMetadata(error));
        }
      }
    }
  },

  get: <T>(path: SettingsPath): T | undefined => {
    const { partialSettings, loadedPaths } = get();

    if (!path?.trim()) {
      return partialSettings as unknown as T;
    }

    const isPathLoaded = loadedPaths.has(path) ||
      [...loadedPaths].some(loadedPath =>
        path.startsWith(loadedPath + '.') || loadedPath.startsWith(path + '.')
      );

    if (!isPathLoaded) {
      if (debugState.isEnabled()) {
        logger.warn(`Accessing unloaded path: ${path} - path should be loaded before access`);
      }
      return undefined as unknown as T;
    }

    const pathParts = path.split('.');
    let current: unknown = partialSettings;

    for (const part of pathParts) {
      if (current == null || typeof current !== 'object' || !(part in (current as Record<string, unknown>))) {
        return undefined;
      }
      current = (current as Record<string, unknown>)[part];
    }

    return current as T;
  },

  set: <T>(path: SettingsPath, value: T, skipServerSync: boolean = false) => {
    if (!path?.trim()) {
      throw new Error('Path cannot be empty');
    }

    set(state => {
      const newPartialSettings = { ...state.partialSettings };
      setNestedValue(newPartialSettings as Record<string, unknown>, path, value);
      const newLoadedPaths = new Set(state.loadedPaths);
      newLoadedPaths.add(path);

      return {
        partialSettings: newPartialSettings,
        settings: newPartialSettings,
        loadedPaths: newLoadedPaths
      };
    });

    if (!skipServerSync) {
      // Route through autoSaveManager for 500ms debounce — prevents flooding
      // the backend when sliders fire onValueChange 60+/sec during drag.
      autoSaveManager.queueChange(path, value);
    }

    if (debugState.isEnabled()) {
      logger.info('Setting updated:', { path, value });
    }
  },

  subscribe: (path: SettingsPath, callback: () => void, immediate: boolean = true) => {
    getOrCreateTrieNode(path)!.subscribers.add(callback);

    if (immediate && get().initialized) {
      callback();
    }

    return () => get().unsubscribe(path, callback);
  },

  unsubscribe: (path: SettingsPath, callback: () => void) => {
    getOrCreateTrieNode(path, false)?.subscribers.delete(callback);
  },

  ensureLoaded: async (paths: string[]): Promise<void> => {
    const { loadedPaths } = get();
    const unloadedPaths = paths.filter(path => !loadedPaths.has(path));

    if (unloadedPaths.length === 0) {
      return;
    }

    try {
      const pathSettings = await settingsApi.getSettingsByPaths(unloadedPaths);

      set(state => {
        const newPartialSettings = { ...state.partialSettings };
        const newLoadedPaths = new Set(state.loadedPaths);

        Object.entries(pathSettings).forEach(([path, value]) => {
          // Deep-merge the incoming server subtree OVER the existing client subtree
          // instead of wholesale-replacing it. A wholesale replace (the previous
          // setNestedValue-only path) dropped client-known keys the server's GET
          // response omits — e.g. touching visualisation.rendering.labelLayoutEvery
          // and then any ensureLoaded() would replace the whole `visualisation`
          // subtree with the server's canonical shape, silently losing
          // labelLayoutEvery / maxEdgesCeiling / softwareFallback. deepMergeSettings
          // (base = existing client, overlay = server) makes server values win
          // per-key while unknown local keys survive. See defect-4.
          const existing = getValueFromPathLocal(newPartialSettings, path);
          if (
            value !== null && typeof value === 'object' && !Array.isArray(value) &&
            existing !== null && typeof existing === 'object' && !Array.isArray(existing)
          ) {
            setNestedValue(
              newPartialSettings as Record<string, unknown>,
              path,
              deepMergeSettings(
                existing as Record<string, unknown>,
                value as Record<string, unknown>,
              ),
            );
          } else {
            setNestedValue(newPartialSettings as Record<string, unknown>, path, value);
          }
          newLoadedPaths.add(path);
        });

        return {
          partialSettings: newPartialSettings,
          settings: newPartialSettings,
          loadedPaths: newLoadedPaths
        };
      });

      if (debugState.isEnabled()) {
        logger.info('Paths loaded on demand:', { paths: unloadedPaths });
      }
    } catch (error) {
      logger.error('Failed to load paths:', createErrorMetadata(error));
      throw error;
    }
  },

  loadSection: async (section: string): Promise<void> => {
    const { loadingSections } = get();
    if (loadingSections.has(section)) {
      return;
    }

    const sectionPaths = getSectionPaths(section);
    if (sectionPaths.length === 0) {
      logger.warn(`Unknown section: ${section}`);
      return;
    }

    set(state => ({
      loadingSections: new Set(state.loadingSections).add(section)
    }));

    try {
      await get().ensureLoaded(sectionPaths);

      if (debugState.isEnabled()) {
        logger.info(`Section loaded: ${section}`, { paths: sectionPaths });
      }
    } finally {
      set(state => {
        const newLoadingSections = new Set(state.loadingSections);
        newLoadingSections.delete(section);
        return { loadingSections: newLoadingSections };
      });
    }
  },

  isLoaded: (path: SettingsPath): boolean => {
    const { loadedPaths } = get();
    return loadedPaths.has(path);
  },

  updateSettings: (updater: (draft: Settings) => void): void => {
    const { partialSettings } = get();

    const newSettings = produce(partialSettings, updater);

    const changedPaths = findChangedPaths(partialSettings, newSettings);

    if (changedPaths.length === 0) {
      return;
    }

    set(state => {
      return {
        partialSettings: newSettings,
        settings: newSettings,
        loadedPaths: new Set([...state.loadedPaths, ...changedPaths])
      };
    });

    // Route through autoSaveManager for 500ms debounce — prevents flooding
    // the backend when sliders fire onValueChange 60+/sec during drag.
    const batchChanges = new Map<string, unknown>();
    changedPaths.forEach(path => {
      const pathParts = path.split('.');
      let current: unknown = newSettings;
      for (const part of pathParts) {
        current = (current as Record<string, unknown>)[part];
      }
      batchChanges.set(path, current);
    });
    autoSaveManager.queueChanges(batchChanges);

    if (debugState.isEnabled()) {
      logger.info('Settings updated via updateSettings:', { changedPaths });
    }

    const state = get();

    const viewportUpdated = changedPaths.some(path => isViewportSetting(path));

    if (viewportUpdated) {
      state.notifyViewportUpdate('viewport.update' as SettingsPath);

      if (debugState.isEnabled()) {
        logger.info('Viewport settings updated, triggering immediate update', {
          viewportPaths: changedPaths.filter(path => isViewportSetting(path))
        });
      }
    }

    // Auto-dispatch tweening updates to the graph worker when tweening paths change.
    const tweeningPaths = changedPaths.filter(path =>
      path.startsWith('clientTweening.') || path.includes('.tweening.')
    );
    if (tweeningPaths.length > 0) {
      const tweening = (newSettings as Record<string, unknown>);
      const vis = tweening.visualisation as Record<string, unknown> | undefined;
      const graphs = vis?.graphs as Record<string, Record<string, unknown>> | undefined;
      const perGraphTweening = graphs?.knowledge?.tweening as Record<string, unknown> | undefined;
      const topLevelTweening = tweening.clientTweening as Record<string, unknown> | undefined;
      const tweeningSettings = perGraphTweening || topLevelTweening;
      if (tweeningSettings) {
        state.notifyTweeningUpdate(tweeningSettings as Partial<{ enabled: boolean; responseMs: number; snapThreshold: number; maxDivergence: number }>);
      }
    }

    // Collect matched subscribers via trie walk (O(depth) per changed path) and
    // batch via RAF so 60fps slider drags coalesce into a single notification per frame.
    scheduleNotify(collectMatchedCallbacks(changedPaths));
  },

  updateComputeMode: (mode: string) => {
    const state = get();
    state.updateSettings((draft: Settings) => {
      const d = draft as unknown as Record<string, unknown>;
      if (!d.dashboard) {
        d.dashboard = { computeMode: '' };
      }
      (d.dashboard as Record<string, unknown>).computeMode = mode;
    });
  },

  updateConstraints: (constraints: ConstraintConfig[]) => {
    const state = get();
    state.updateSettings((draft: Settings) => {
      const d = draft as unknown as Record<string, unknown>;
      if (!d.developer) {
        d.developer = {};
      }
      const developer = d.developer as Record<string, unknown>;
      if (!developer.constraints) {
        developer.constraints = {};
      }
      (developer.constraints as Record<string, unknown>).active = constraints;
    });
  },
})
