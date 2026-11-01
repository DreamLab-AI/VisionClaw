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
// Layout modes
// ============================================================================

/**
 * The layout modes the server stores as `physics.layoutMode` (ADR-2140) and accepts at
 * POST /api/layout/mode: the engine's `LayoutMode` serde names. There is no remapping layer:
 * each name is its own mode (`dagLeftRight` and `typeClustering` are variants of their own).
 */
export const LAYOUT_MODES = [
  'forceDirected',
  'hierarchical',
  'radial',
  'dagLeftRight',
  'typeClustering',
  'spectral',
  'clustered',
] as const;

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
      mode,
      transitionMs,
    }),

  getStatus: (): Promise<AxiosResponse<LayoutStatusResponse>> =>
    axios.get(`${API_BASE}/layout/status`),

  resetLayout: (): Promise<AxiosResponse<LayoutModeResponse>> =>
    axios.post(`${API_BASE}/layout/reset`),
};
