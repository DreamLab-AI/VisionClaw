import { StateCreator } from 'zustand'
import { createLogger } from '../../utils/loggerConfig'
import { debugState } from '../../utils/clientDebugState'
import { SettingsState, GPUPhysicsParams, WarmupSettings } from './settingsTypes'
import { Settings } from '../../features/settings/config/settings'

const logger = createLogger('SettingsStore')

export type PhysicsSlice = Pick<
  SettingsState,
  | 'updatePhysics'
  | 'updateWarmupSettings'
  | 'notifyPhysicsUpdate'
  | 'updateTweening'
  | 'notifyTweeningUpdate'
>

export const createPhysicsSlice: StateCreator<SettingsState, [], [], PhysicsSlice> = (_set, get) => ({
  updatePhysics: (graphName: string, params: Partial<GPUPhysicsParams>) => {
    // Guard against non-string graph names (prevents [object Object] key corruption)
    if (typeof graphName !== 'string' || !graphName || graphName === '[object Object]') {
      logger.warn('updatePhysics called with invalid graphName, defaulting to "knowledge":', graphName);
      graphName = 'knowledge';
    }
    // Receive-side shim: a graph name read from persisted browser state may still
    // be the pre-ADR-2041 `logseq`; map it so localStorage keeps working. The
    // server rejects `logseq` since ADR-2115, so it is never sent.
    if (graphName === 'logseq') {
      graphName = 'knowledge';
    }
    const state = get();

    const validatedParams = { ...params };

    if (validatedParams.restLength !== undefined) {
      validatedParams.restLength = Math.max(0.1, Math.min(10000.0, validatedParams.restLength));
    }
    if (validatedParams.repulsionCutoff !== undefined) {
      validatedParams.repulsionCutoff = Math.max(1.0, Math.min(50000.0, validatedParams.repulsionCutoff));
    }
    if (validatedParams.repulsionSofteningEpsilon !== undefined) {
      validatedParams.repulsionSofteningEpsilon = Math.max(0.00001, Math.min(1.0, validatedParams.repulsionSofteningEpsilon));
    }
    if (validatedParams.centerGravityK !== undefined) {
      validatedParams.centerGravityK = Math.max(-1000.0, Math.min(1000.0, validatedParams.centerGravityK));
    }
    if (validatedParams.gridCellSize !== undefined) {
      validatedParams.gridCellSize = Math.max(1.0, Math.min(2000.0, validatedParams.gridCellSize));
    }
    if (validatedParams.featureFlags !== undefined) {
      validatedParams.featureFlags = Math.max(0, Math.min(255, Math.floor(validatedParams.featureFlags)));
    }

    const extParams = validatedParams as Record<string, unknown>;
    if (extParams.arrow_size !== undefined) {
      extParams.arrow_size = Math.max(0.01, Math.min(5.0, extParams.arrow_size as number));
    }
    if (extParams.arrowSize !== undefined) {
      extParams.arrowSize = Math.max(0.01, Math.min(5.0, extParams.arrowSize as number));
    }
    if (extParams.base_width !== undefined) {
      extParams.base_width = Math.max(0.01, Math.min(5.0, extParams.base_width as number));
    }
    if (extParams.baseWidth !== undefined) {
      extParams.baseWidth = Math.max(0.01, Math.min(5.0, extParams.baseWidth as number));
    }

    if (validatedParams.springK !== undefined) {
      validatedParams.springK = Math.max(0.001, Math.min(1000.0, validatedParams.springK));
    }
    if (validatedParams.springKKnowledge !== undefined) {
      validatedParams.springKKnowledge = Math.max(0.0, Math.min(10.0, validatedParams.springKKnowledge));
    }
    if (validatedParams.springKOntology !== undefined) {
      validatedParams.springKOntology = Math.max(0.0, Math.min(10.0, validatedParams.springKOntology));
    }
    if (validatedParams.springKAgent !== undefined) {
      validatedParams.springKAgent = Math.max(0.0, Math.min(10.0, validatedParams.springKAgent));
    }
    if (validatedParams.repelK !== undefined) {
      validatedParams.repelK = Math.max(0.001, Math.min(2000.0, validatedParams.repelK));
    }
    if (validatedParams.attractionK !== undefined) {
      validatedParams.attractionK = Math.max(0.0, Math.min(500.0, validatedParams.attractionK));
    }
    if (validatedParams.gravity !== undefined) {
      validatedParams.gravity = Math.max(-1.0, Math.min(1.0, validatedParams.gravity));
    }
    if (validatedParams.warmupIterations !== undefined) {
      validatedParams.warmupIterations = Math.max(0, Math.min(1000, Math.floor(validatedParams.warmupIterations)));
    }
    if (validatedParams.coolingRate !== undefined) {
      validatedParams.coolingRate = Math.max(0.0001, Math.min(1.0, validatedParams.coolingRate));
    }

    state.updateSettings((draft: Settings) => {
      const d = draft as unknown as Record<string, unknown>;
      if (!d.visualisation) d.visualisation = { graphs: {} };
      const vis = d.visualisation as Record<string, unknown>;
      if (!vis.graphs) vis.graphs = {};

      const graphs = vis.graphs as Record<string, unknown>;
      if (!graphs[graphName]) graphs[graphName] = {};
      const graph = graphs[graphName] as Record<string, unknown>;
      if (!graph.physics) graph.physics = {};

      const graphSettings = graphs[graphName] as Record<string, unknown> | undefined;
      if (graphSettings && graphSettings.physics) {
        Object.assign(graphSettings.physics as Record<string, unknown>, validatedParams);

        if (debugState.isEnabled()) {
          logger.info('Physics parameters updated:', {
            graphName,
            updatedParams: validatedParams,
            newPhysicsState: graphSettings.physics
          });
        }
      }
    });

    state.notifyPhysicsUpdate(graphName, validatedParams);
  },

  updateWarmupSettings: (settings: WarmupSettings) => {
    const state = get();
    state.updateSettings((draft: Settings) => {
      const d = draft as unknown as Record<string, unknown>;
      if (!d.performance) {
        d.performance = {};
      }
      Object.assign(d.performance as Record<string, unknown>, settings);
    });
  },

  notifyPhysicsUpdate: (_graphName: string, _params: Partial<GPUPhysicsParams>) => {
    // Persistence is owned solely by the debounced autoSaveManager path
    // (UnifiedSettingsTabContent → autoSaveManager.queueChange → updateSettingsByPaths
    // → settingsApi.updatePhysics). This used to fire an IMMEDIATE
    // settingsApi.updatePhysics() here, which produced a SECOND PUT per slider
    // commit and a second backend UpdateSimulationParams dispatch (double warmup
    // reset / double reheat). Resolved 2026-06-03: local store state is already
    // updated in updatePhysics() above; this hook no longer performs a network
    // persistence side-effect. The debounced path reads store state independently,
    // so the change still reaches the backend exactly once.
  },

  updateTweening: (graphName: string, params: Partial<{ enabled: boolean; lerpBase: number; snapThreshold: number; maxDivergence: number }>) => {
    if (typeof graphName !== 'string' || !graphName || graphName === '[object Object]') {
      logger.warn('updateTweening called with invalid graphName, defaulting to "knowledge":', graphName);
      graphName = 'knowledge';
    }
    // Receive-side shim: a graph name read from persisted browser state may still
    // be the pre-ADR-2041 `logseq`; map it so localStorage keeps working. The
    // server rejects `logseq` since ADR-2115, so it is never sent.
    if (graphName === 'logseq') {
      graphName = 'knowledge';
    }
    const state = get();

    // Validate ranges
    const validatedParams = { ...params };
    if (validatedParams.lerpBase !== undefined) {
      validatedParams.lerpBase = Math.max(0.0001, Math.min(0.5, validatedParams.lerpBase));
    }
    if (validatedParams.snapThreshold !== undefined) {
      validatedParams.snapThreshold = Math.max(0.01, Math.min(1.0, validatedParams.snapThreshold));
    }
    if (validatedParams.maxDivergence !== undefined) {
      validatedParams.maxDivergence = Math.max(1, Math.min(100, validatedParams.maxDivergence));
    }

    state.updateSettings((draft: Settings) => {
      const d = draft as unknown as Record<string, unknown>;
      if (!d.visualisation) d.visualisation = { graphs: {} };
      const vis = d.visualisation as Record<string, unknown>;
      if (!vis.graphs) vis.graphs = {};

      const graphs = vis.graphs as Record<string, unknown>;
      if (!graphs[graphName]) graphs[graphName] = {};
      const graph = graphs[graphName] as Record<string, unknown>;
      if (!graph.tweening) graph.tweening = {};

      Object.assign(graph.tweening as Record<string, unknown>, validatedParams);

      // Also update top-level clientTweening for backward compatibility
      if (!d.clientTweening) d.clientTweening = {};
      Object.assign(d.clientTweening as Record<string, unknown>, validatedParams);
    });

    // Dispatch event so the graph worker picks up the change in real-time
    state.notifyTweeningUpdate(validatedParams);
  },

  notifyTweeningUpdate: (params: Partial<{ enabled: boolean; lerpBase: number; snapThreshold: number; maxDivergence: number }>) => {
    if (typeof window !== 'undefined') {
      const event = new CustomEvent('tweeningSettingsUpdated', {
        detail: params
      });
      window.dispatchEvent(event);
    }
  },
})
