// api/settings/schemaMappings.ts
// Path mapping: client dot-notation paths → API endpoints and visual blob keys
// Also houses the transform from flat API response to nested client structure

import type { AllSettings, PhysicsSettings, ConstraintSettings, RenderingSettings, NodeFilterSettings } from './types';
import {
  DEFAULT_GLOW_SETTINGS,
  DEFAULT_BLOOM_SETTINGS,
  DEFAULT_HOLOGRAM_SETTINGS,
  DEFAULT_GRAPH_TYPE_VISUALS,
  DEFAULT_GEM_MATERIAL,
  DEFAULT_SCENE_EFFECTS,
  DEFAULT_CLUSTER_HULLS,
  DEFAULT_EMBEDDING_CLOUD,
  DEFAULT_ANIMATION_SETTINGS,
  DEFAULT_INTERACTION_SETTINGS,
  DEFAULT_NODES_SETTINGS,
  DEFAULT_EDGES_SETTINGS,
  DEFAULT_LABELS_SETTINGS,
  DEFAULT_QUALITY_GATES,
} from './defaults';

// ============================================================================
// Deep merge helper — stored server values win over local defaults
// ============================================================================

export function deepMergeVisual(
  defaults: Record<string, unknown>,
  stored: Record<string, unknown>
): Record<string, unknown> {
  const result = { ...defaults };
  for (const [key, value] of Object.entries(stored)) {
    if (
      value && typeof value === 'object' && !Array.isArray(value) &&
      result[key] && typeof result[key] === 'object' && !Array.isArray(result[key])
    ) {
      result[key] = deepMergeVisual(
        result[key] as Record<string, unknown>,
        value as Record<string, unknown>
      );
    } else {
      result[key] = value;
    }
  }
  return result;
}

// ============================================================================
// Transform flat API response → nested client structure
// ============================================================================

export function transformApiToClientSettings(
  apiResponse: AllSettings
): Record<string, unknown> {
  const v = (apiResponse.visual || {}) as Record<string, Record<string, unknown>>;
  return {
    visualisation: {
      rendering: apiResponse.rendering || {},
      glow: deepMergeVisual(DEFAULT_GLOW_SETTINGS, v.glow || {}),
      bloom: deepMergeVisual(DEFAULT_BLOOM_SETTINGS, v.bloom || {}),
      hologram: deepMergeVisual(DEFAULT_HOLOGRAM_SETTINGS, v.hologram || {}),
      graphTypeVisuals: deepMergeVisual(DEFAULT_GRAPH_TYPE_VISUALS, v.graphTypeVisuals || {}),
      gemMaterial: deepMergeVisual(DEFAULT_GEM_MATERIAL, v.gemMaterial || {}),
      sceneEffects: deepMergeVisual(DEFAULT_SCENE_EFFECTS, v.sceneEffects || {}),
      clusterHulls: deepMergeVisual(DEFAULT_CLUSTER_HULLS, v.clusterHulls || {}),
      embeddingCloud: deepMergeVisual(DEFAULT_EMBEDDING_CLOUD, v.embeddingCloud || {}),
      animations: deepMergeVisual(DEFAULT_ANIMATION_SETTINGS, v.animations || {}),
      interaction: deepMergeVisual(DEFAULT_INTERACTION_SETTINGS, v.interaction || {}),
      graphs: {
        knowledge: {
          physics: apiResponse.physics || {},
          nodes: deepMergeVisual(DEFAULT_NODES_SETTINGS, v.nodes || {}),
          edges: deepMergeVisual(DEFAULT_EDGES_SETTINGS, v.edges || {}),
          labels: deepMergeVisual(DEFAULT_LABELS_SETTINGS, v.labels || {})
        }
      }
    },
    system: {
      debug: {
        enabled: false,
        enableDataDebug: false,
        enableWebsocketDebug: false,
        logBinaryHeaders: false,
        logFullJson: false
      },
      websocket: {
        reconnectAttempts: 5,
        reconnectDelay: 1000,
        binaryChunkSize: 1024,
        compressionEnabled: true,
        compressionThreshold: 1024,
        updateRate: 60
      },
      persistSettings: true
    },
    xr: {
      enabled: false,
      mode: 'inline' as const,
      enableHandTracking: false,
      enableHaptics: false,
      quality: 'medium' as const
    },
    auth: {
      enabled: false,
      provider: 'nostr' as const,
      required: false
    },
    // ADR-031 D6: merge defaults UNDER the server response so a partial
    // qualityGates payload still ships render-by-default values (and the new
    // showCentrality / showSSSP gates) rather than dropping every default.
    qualityGates: { ...DEFAULT_QUALITY_GATES, ...(apiResponse.qualityGates || {}) },
    nodeFilter: {
      ...{
        enabled: true,
        // Conservative public-demo default: approximately the top quality decile.
        qualityThreshold: 0.9,
        authorityThreshold: 0.5,
        filterByQuality: true,
        filterByAuthority: false,
        filterMode: 'or' as const,
        includeLinkedPages: false,
        minConnections: 0,
        minMaturity: 'off',
      },
      ...(apiResponse.nodeFilter || {}),
    }
  };
}

// ============================================================================
// Path routing helpers
// ============================================================================

/**
 * Returns true when the client path should be routed to /api/settings/visual.
 * Excludes paths already handled by dedicated endpoints (rendering, physics).
 */
export function isVisualSettingsPath(path: string): boolean {
  if (path.startsWith('visualisation.rendering')) return false;
  if (path.startsWith('visualisation.graphs.') && path.includes('.physics')) return false;
  if (path.startsWith('visualisation.')) return true;
  return false;
}

/**
 * Convert a client settings path to its key within the visual blob stored on the server.
 *
 * Mappings:
 *   visualisation.graphs.knowledge.nodes.X  → nodes.X   (pre-ADR-2041 `logseq` mapped on receive)
 *   visualisation.graphs.knowledge.edges.X  → edges.X
 *   visualisation.graphs.knowledge.labels.X → labels.X
 *   visualisation.<category>.X           → <category>.X
 */
export function toVisualKey(path: string): string {
  // Receive-side shim: a path built from persisted browser state may still use
  // the pre-ADR-2041 `logseq` segment; map it onto `knowledge`. The server has
  // rejected `logseq` since ADR-2115, and the mapped path is what gets sent.
  const p = path.startsWith('visualisation.graphs.logseq.')
    ? path.replace('visualisation.graphs.logseq.', 'visualisation.graphs.knowledge.')
    : path;
  for (const bucket of ['nodes', 'edges', 'labels'] as const) {
    const prefix = `visualisation.graphs.knowledge.${bucket}`;
    if (p.startsWith(prefix)) return p.replace(prefix, bucket);
  }
  return p.replace('visualisation.', '');
}

/** Build a nested object from a dot-notation path and a leaf value. */
export function setNestedFromDotPath(
  obj: Record<string, unknown>,
  dotPath: string,
  value: unknown
): void {
  const parts = dotPath.split('.');
  let current: Record<string, unknown> = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    if (!(parts[i] in current) || typeof current[parts[i]] !== 'object') {
      current[parts[i]] = {};
    }
    current = current[parts[i]] as Record<string, unknown>;
  }
  current[parts[parts.length - 1]] = value;
}

/** Read a value from a nested object by dot-notation path. */
export function getNestedValue(obj: Record<string, unknown>, path: string): unknown {
  const parts = path.split('.');
  let current: unknown = obj;
  for (const part of parts) {
    if (current === undefined || current === null || typeof current !== 'object') return undefined;
    current = (current as Record<string, unknown>)[part];
  }
  return current;
}

// Re-export empty skeleton used in error fallback. ADR-031 D6: qualityGates
// ships full defaults even on the error path so analytics never silently
// disappear when a settings fetch fails.
export function emptyAllSettings(): AllSettings {
  return {
    physics: {} as PhysicsSettings,
    constraints: {} as ConstraintSettings,
    rendering: {} as RenderingSettings,
    nodeFilter: {} as NodeFilterSettings,
    qualityGates: { ...DEFAULT_QUALITY_GATES },
  };
}
