import { DeepPartial } from '../../features/settings/config/settings'
import { createLogger } from '../../utils/loggerConfig'
import { nostrAuth } from '../../services/nostrAuthService'

const logger = createLogger('SettingsStore')

// Essential paths loaded at startup for fast initialization
export const ESSENTIAL_PATHS = [
  'system.debug.enabled',
  'system.websocket.updateRate',
  'system.websocket.reconnectAttempts',
  'auth.enabled',
  'auth.required',
  'visualisation.rendering.context',
  'xr.enabled',
  'xr.mode',

  'visualisation.graphs.knowledge.physics',

  // Graph-type visual settings - needed for per-type rendering
  'visualisation.graphTypeVisuals',

  // Node filtering settings - needed for visibility filtering
  'nodeFilter.enabled',
  'nodeFilter.qualityThreshold',
  'nodeFilter.authorityThreshold',
  'nodeFilter.filterByQuality',
  'nodeFilter.filterByAuthority',
  'nodeFilter.filterMode',

  // Client-side tweening settings - needed for smooth node movement
  'clientTweening',
  'visualisation.graphs.knowledge.tweening',

  // Rendering settings - needed for scene lighting and quality
  'visualisation.rendering.ambientLightIntensity',
  'visualisation.rendering.directionalLightIntensity',
  'visualisation.rendering.enableAntialiasing',

  // Quality gate settings - needed for cluster/anomaly/layout rendering
  'qualityGates.showClusters',
  'qualityGates.showAnomalies',
  'qualityGates.showCommunities',
  'qualityGates.layoutMode',

  // Animation settings - needed for pulse/wave effects
  'visualisation.animations.enableNodeAnimations',
  'visualisation.animations.pulseEnabled',
  'visualisation.animations.pulseSpeed',
  'visualisation.animations.pulseStrength',
  'visualisation.animations.selectionWaveEnabled',
  'visualisation.animations.waveSpeed'
];

// Helper to wait for authentication to be ready
export async function waitForAuthReady(maxWaitMs: number = 3000): Promise<void> {
  const startTime = Date.now();

  if (!nostrAuth['initialized']) {
    logger.info('Waiting for nostrAuth to initialize...');
    await nostrAuth.initialize();
  }

  return new Promise((resolve) => {
    const checkAuth = () => {
      const elapsed = Date.now() - startTime;

      if (elapsed >= maxWaitMs || !localStorage.getItem('nostr_user')) {
        logger.info('Proceeding with settings initialization', {
          authenticated: nostrAuth.isAuthenticated(),
          elapsed
        });
        resolve();
        return;
      }

      if (nostrAuth.isAuthenticated()) {
        logger.info('Auth ready, proceeding with settings initialization');
        resolve();
        return;
      }

      setTimeout(checkAuth, 100);
    };

    checkAuth();
  });
}

// Deep merge two settings objects. overlay values win over base values.
// Used during initialization to merge server defaults (base) with localStorage (overlay)
// so that user customizations survive page reloads.
export function deepMergeSettings(
  base: Record<string, unknown>,
  overlay: Record<string, unknown>,
): Record<string, unknown> {
  const result = { ...base };
  for (const [key, value] of Object.entries(overlay)) {
    if (value === undefined) continue;
    const baseVal = result[key];
    if (
      value !== null && typeof value === 'object' && !Array.isArray(value) &&
      baseVal !== null && typeof baseVal === 'object' && !Array.isArray(baseVal)
    ) {
      result[key] = deepMergeSettings(
        baseVal as Record<string, unknown>,
        value as Record<string, unknown>,
      );
    } else {
      result[key] = value;
    }
  }
  return result;
}

// Helper function to find changed paths between two objects.
// Uses a collector pattern (mutating `out`) to avoid intermediate array allocations
// from spread operators. Leverages immer structural sharing: unchanged subtrees
// share the same reference so `oldObj === newObj` short-circuits entire branches.
export function findChangedPaths(oldObj: unknown, newObj: unknown, path: string = '', out: string[] = []): string[] {
  if (oldObj === newObj) return out;
  if (oldObj == null || newObj == null) {
    if (path) out.push(path);
    return out;
  }
  if (typeof oldObj !== 'object' || typeof newObj !== 'object') {
    if (oldObj !== newObj && path) out.push(path);
    return out;
  }
  const oldRecord = oldObj as Record<string, unknown>;
  const newRecord = newObj as Record<string, unknown>;
  const allKeys = new Set([...Object.keys(oldRecord), ...Object.keys(newRecord)]);
  for (const key of allKeys) {
    const currentPath = path ? `${path}.${key}` : key;
    const oldValue = oldRecord[key];
    const newValue = newRecord[key];
    if (oldValue === newValue) continue; // Fast skip unchanged subtrees (immer structural sharing)
    if (typeof oldValue === 'object' && typeof newValue === 'object' && oldValue !== null && newValue !== null) {
      findChangedPaths(oldValue, newValue, currentPath, out);
    } else {
      out.push(currentPath);
    }
  }
  return out;
}

// Helper function to set nested value by dot notation path
export function setNestedValue(obj: Record<string, unknown>, path: string, value: unknown): void {
  const keys = path.split('.');
  let current: Record<string, unknown> = obj;

  for (let i = 0; i < keys.length - 1; i++) {
    const key = keys[i];
    if (!(key in current) || typeof current[key] !== 'object' || current[key] === null) {
      current[key] = {};
    } else {
      // Shallow-clone this level to avoid writing to frozen Immer objects
      current[key] = { ...(current[key] as Record<string, unknown>) };
    }
    current = current[key] as Record<string, unknown>;
  }

  current[keys[keys.length - 1]] = value;
}

// Helper function to get paths for a specific section
export function getSectionPaths(section: string): string[] {
  const sectionPathMap: Record<string, string[]> = {
    'physics': [
      'visualisation.graphs.knowledge.physics'
    ],
    'rendering': [
      'visualisation.rendering.ambientLightIntensity',
      'visualisation.rendering.backgroundColor',
      'visualisation.rendering.directionalLightIntensity',
      'visualisation.rendering.enableAmbientOcclusion',
      'visualisation.rendering.enableAntialiasing',
      'visualisation.rendering.enableShadows',
      'visualisation.rendering.environmentIntensity',
      'visualisation.rendering.shadowMapSize',
      'visualisation.rendering.shadowBias',
      'visualisation.rendering.context'
    ],
    'xr': [
      'xr.enabled',
      'xr.mode',
      'xr.enableHandTracking',
      'xr.enableHaptics',
      'xr.quality'
    ],
    'glow': [
      'visualisation.glow.enabled',
      'visualisation.glow.intensity',
      'visualisation.glow.radius',
      'visualisation.glow.threshold'
    ],
    'graphTypeVisuals': [
      'visualisation.graphTypeVisuals.knowledgeGraph',
      'visualisation.graphTypeVisuals.ontology',
      'visualisation.graphTypeVisuals.agent'
    ],
    'nodes': [
      'visualisation.graphs.knowledge.nodes'
    ],
    'edges': [
      'visualisation.graphs.knowledge.edges'
    ],
    'labels': [
      'visualisation.graphs.knowledge.labels'
    ],
    'gemMaterial': [
      'visualisation.gemMaterial'
    ],
    'sceneEffects': [
      'visualisation.sceneEffects'
    ],
    'clusterHulls': [
      'visualisation.clusterHulls'
    ],
    'hologram': [
      'visualisation.hologram'
    ],
    'animations': [
      'visualisation.animations'
    ],
    'interaction': [
      'visualisation.interaction'
    ],
    'analytics': [
      'analytics.enableMetrics',
      'analytics.updateInterval',
      'analytics.showDegreeDistribution',
      'analytics.showClusteringCoefficient',
      'analytics.showCentrality'
    ],
    'qualityGates': [
      'qualityGates'
    ],
    'nodeFilter': [
      'nodeFilter'
    ],
    'constraints': [
      'constraints'
    ],
    'tweening': [
      'clientTweening',
      'visualisation.graphs.knowledge.tweening'
    ]
  };

  return sectionPathMap[section] || [];
}

// Helper function to extract all paths from a settings object
export function getAllSettingsPaths(obj: unknown, prefix: string = ''): string[] {
  const paths: string[] = [];

  if (obj && typeof obj === 'object') {
    for (const [key, value] of Object.entries(obj)) {
      const currentPath = prefix ? `${prefix}.${key}` : key;

      if (value && typeof value === 'object' && !Array.isArray(value)) {
        paths.push(...getAllSettingsPaths(value, currentPath));
      } else {
        paths.push(currentPath);
      }
    }
  }

  return paths;
}

// Helper function to get all available settings paths for comprehensive operations
export function getAllAvailableSettingsPaths(): string[] {
  return [
    ...ESSENTIAL_PATHS,

    'visualisation.rendering.ambientLightIntensity',
    'visualisation.rendering.backgroundColor',
    'visualisation.rendering.directionalLightIntensity',
    'visualisation.rendering.enableAmbientOcclusion',
    'visualisation.rendering.enableAntialiasing',
    'visualisation.rendering.enableShadows',
    'visualisation.rendering.environmentIntensity',
    'visualisation.rendering.shadowMapSize',
    'visualisation.rendering.shadowBias',

    'visualisation.graphs.knowledge.nodes',
    'visualisation.graphs.knowledge.edges',
    'visualisation.graphs.knowledge.labels',
    'visualisation.graphs.knowledge.physics',

    'visualisation.glow.enabled',
    'visualisation.glow.intensity',
    'visualisation.glow.radius',
    'visualisation.glow.threshold',
    'visualisation.hologram.ringCount',
    'visualisation.hologram.ringColor',
    'visualisation.hologram.globalRotationSpeed',

    // Graph-type visual settings
    'visualisation.graphTypeVisuals.knowledgeGraph',
    'visualisation.graphTypeVisuals.ontology',
    'visualisation.graphTypeVisuals.agent',

    'xr.enableHandTracking',
    'xr.enableHaptics',
    'xr.quality',

    // Gem material
    'visualisation.gemMaterial',

    // Scene effects (WASM ambient)
    'visualisation.sceneEffects',

    // Cluster hulls
    'visualisation.clusterHulls',

    // Animations
    'visualisation.animations',

    // Interaction / selection highlighting
    'visualisation.interaction',

    // Analytics
    'analytics',

    // Quality gates
    'qualityGates',

    // Node filter
    'nodeFilter',

    // Constraints / LOD
    'constraints',

    // Client-side tweening
    'clientTweening',
    'visualisation.graphs.knowledge.tweening',
  ];
}

// Reconstruct loadedPaths from a persisted settings object
export function collectPathsFromSettings(obj: unknown, prefix: string = ''): Set<string> {
  const paths = new Set<string>();
  const collect = (o: unknown, pfx: string) => {
    if (o && typeof o === 'object' && !Array.isArray(o)) {
      for (const [key, value] of Object.entries(o as Record<string, unknown>)) {
        const currentPath = pfx ? `${pfx}.${key}` : key;
        if (value && typeof value === 'object' && !Array.isArray(value)) {
          collect(value, currentPath);
        } else {
          paths.add(currentPath);
        }
      }
    }
  };
  collect(obj, prefix);
  return paths;
}

/**
 * Receive-side migration of persisted browser state: settings written before
 * the ADR-2041 rename carry `visualisation.graphs.logseq`. Map that object onto
 * `visualisation.graphs.knowledge` when `knowledge` is absent, and drop the
 * legacy key so the next save emits only `knowledge` — the server rejects
 * `logseq` since ADR-2115 (which superseded ADR-2041). Returns the same
 * reference when there is nothing to migrate. Kept so existing users'
 * localStorage keeps loading.
 */
export function migrateGraphSettingsKey<T>(persisted: T): T {
  const root = persisted as Record<string, unknown> | null | undefined;
  const vis = root?.visualisation as Record<string, unknown> | undefined;
  const graphs = vis?.graphs as Record<string, unknown> | undefined;
  if (!graphs || !('logseq' in graphs)) return persisted;

  const { logseq, ...rest } = graphs as { logseq?: unknown } & Record<string, unknown>;
  const migratedGraphs: Record<string, unknown> = { ...rest };
  // `knowledge` wins when both keys are present: it is the current vocabulary.
  if (migratedGraphs.knowledge === undefined && logseq !== undefined) {
    migratedGraphs.knowledge = logseq;
  }

  return {
    ...(root as Record<string, unknown>),
    visualisation: { ...(vis as Record<string, unknown>), graphs: migratedGraphs },
  } as T;
}
