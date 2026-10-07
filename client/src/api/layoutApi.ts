// frontend/src/api/layoutApi.ts
// REAL API client for layout mode management - NO MOCKS
// Auth handled by global axios interceptor in settingsApi.ts

import axios, { AxiosResponse } from 'axios';

const API_BASE = '/api';

// ============================================================================
// Type Definitions (matching Rust backend)
// ============================================================================

export interface LayoutPosition {
  id: number;
  x: number;
  y: number;
  z: number;
}

export interface LayoutModeResponse {
  success: boolean;
  mode: string;
  positions?: LayoutPosition[];
  transitionMs?: number;
}

export interface LayoutMode {
  id: string;
  label: string;
  description?: string;
}

export interface LayoutModesResponse {
  modes: LayoutMode[];
}

export interface LayoutStatusResponse {
  currentMode: string;
  transitioning: boolean;
  nodeCount: number;
}

// ============================================================================
// Vocabulary bridge
// ============================================================================

/**
 * The persisted `qualityGates.layoutMode` vocabulary, validated by the server's
 * settings route (src/settings/api/settings_routes.rs `valid_modes`).
 */
export const QUALITY_GATE_LAYOUT_MODES = [
  'force-directed',
  'dag-topdown',
  'dag-radial',
  'dag-leftright',
  'type-clustering',
] as const;

/**
 * Quality-gate mode → the layout engine's `LayoutMode` serde name, which is
 * what POST /api/layout/mode deserialises (camelCase; see
 * crates/visionclaw-domain/src/types/layout.rs). The DAG variants share the
 * GPU-resident Sugiyama layer spring (`hierarchical`) except the radial one;
 * `type-clustering` acts through the quality-gate physics overrides
 * (cluster_strength), so the engine runs its force-directed baseline.
 */
const QUALITY_GATE_TO_ENGINE: Record<string, string> = {
  'force-directed': 'forceDirected',
  'dag-topdown': 'hierarchical',
  'dag-leftright': 'hierarchical',
  'dag-radial': 'radial',
  'type-clustering': 'forceDirected',
};

/** Translate a quality-gate layout mode; engine-vocabulary names pass through. */
export function toEngineLayoutMode(mode: string): string {
  return QUALITY_GATE_TO_ENGINE[mode] ?? mode;
}

// ============================================================================
// API Client
// ============================================================================

export const layoutApi = {
  getModes: (): Promise<AxiosResponse<LayoutModesResponse>> =>
    axios.get(`${API_BASE}/layout/modes`),

  setMode: (
    mode: string,
    transitionMs = 500
  ): Promise<AxiosResponse<LayoutModeResponse>> =>
    axios.post<LayoutModeResponse>(`${API_BASE}/layout/mode`, {
      mode: toEngineLayoutMode(mode),
      transitionMs,
    }),

  getStatus: (): Promise<AxiosResponse<LayoutStatusResponse>> =>
    axios.get(`${API_BASE}/layout/status`),

  resetLayout: (): Promise<AxiosResponse<LayoutModeResponse>> =>
    axios.post(`${API_BASE}/layout/reset`),
};
