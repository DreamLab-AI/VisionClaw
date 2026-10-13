// api/settings/endpoints.ts
// Fetch / PUT wrappers for all settings API endpoints

import axios, { AxiosResponse } from 'axios';
import { nostrAuth } from '../../services/nostrAuthService';
import { computeAuthHeaders } from '../../services/api/authInterceptor';
import { physicsPatchEntry } from './physicsPatch';
import { createLogger } from '../../utils/loggerConfig';
import type {
  PhysicsSettings,
  ConstraintSettings,
  RenderingSettings,
  NodeFilterSettings,
  QualityGateSettings,
  AllSettings,
  SettingsProfile,
  SaveProfileRequest,
  ProfileIdResponse,
} from './types';
import { DEFAULT_PHYSICS_SETTINGS } from './defaults';
import {
  transformApiToClientSettings,
  isVisualSettingsPath,
  toVisualKey,
  setNestedFromDotPath,
  getNestedValue,
  emptyAllSettings,
} from './schemaMappings';

const logger = createLogger('settingsApi');

// Always use relative paths; Vite proxies /api in dev, nginx in prod.
const API_BASE = '';

// ============================================================================
// Global NIP-98 auth interceptor
// ============================================================================

axios.interceptors.request.use(async (config) => {
  if (!nostrAuth.isAuthenticated()) return config;
  if (!config.headers) return config;

  try {
    const fullUrl = new URL(config.url || '', config.baseURL || window.location.origin).href;
    const method = (config.method || 'GET').toUpperCase();
    const body = config.data
      ? (typeof config.data === 'string' ? config.data : JSON.stringify(config.data))
      : undefined;
    Object.assign(config.headers, await computeAuthHeaders(fullUrl, method, body));
  } catch (e) {
    logger.warn('[settingsApi] NIP-98 signing failed:', e);
  }
  return config;
});

// ============================================================================
// Simple in-memory cache for getAll()
// ============================================================================

let _cachedAllSettings: Record<string, unknown> | null = null;
let _cachedAllTimestamp = 0;
const CACHE_TTL_MS = 2000;

// ============================================================================
// Physics
// ============================================================================

export const getPhysics = (): Promise<AxiosResponse<PhysicsSettings>> =>
  axios.get(`${API_BASE}/api/settings/physics`);

export const updatePhysics = async (
  settings: Partial<PhysicsSettings>
): Promise<AxiosResponse<void>> => {
  logger.debug('[SETTINGS-DIAG] updatePhysics called with:', settings);
  logger.debug('[SETTINGS-DIAG] auth: authenticated=', nostrAuth.isAuthenticated());
  try {
    const current = await axios.get(`${API_BASE}/api/settings/physics`);
    logger.debug('[SETTINGS-DIAG] updatePhysics GET current:', current.status, current.data);
    const currentData = current.data?.data ?? current.data ?? {};
    const merged = { ...currentData, ...settings };
    logger.debug('[SETTINGS-DIAG] updatePhysics PUT merged:', merged);
    const result = await axios.put(`${API_BASE}/api/settings/physics`, merged);
    logger.debug('[SETTINGS-DIAG] updatePhysics PUT response:', result.status, result.data);
    return result;
  } catch (err) {
    logger.debug('[SETTINGS-DIAG] updatePhysics FAILED:', err);
    throw err;
  }
};

// ============================================================================
// Constraints
// ============================================================================

export const getConstraints = (): Promise<AxiosResponse<ConstraintSettings>> =>
  axios.get(`${API_BASE}/api/settings/constraints`);

export const updateConstraints = async (
  settings: Partial<ConstraintSettings>
): Promise<AxiosResponse<void>> => {
  const current = await axios.get(`${API_BASE}/api/settings/constraints`);
  const currentData = current.data?.data ?? current.data ?? {};
  const merged = { ...currentData, ...settings };
  return axios.put(`${API_BASE}/api/settings/constraints`, merged);
};

// ============================================================================
// Rendering
// ============================================================================

export const getRendering = (): Promise<AxiosResponse<RenderingSettings>> =>
  axios.get(`${API_BASE}/api/settings/rendering`);

export const updateRendering = async (
  settings: Partial<RenderingSettings>
): Promise<AxiosResponse<void>> => {
  const current = await axios.get(`${API_BASE}/api/settings/rendering`);
  const currentData = current.data?.data ?? current.data ?? {};
  const merged = { ...currentData, ...settings };
  return axios.put(`${API_BASE}/api/settings/rendering`, merged);
};

// ============================================================================
// Node filter
// ============================================================================

export const getNodeFilter = (): Promise<AxiosResponse<NodeFilterSettings>> =>
  axios.get(`${API_BASE}/api/settings/node-filter`);

export const updateNodeFilter = async (
  settings: Partial<NodeFilterSettings>
): Promise<AxiosResponse<void>> => {
  const current = await axios.get(`${API_BASE}/api/settings/node-filter`);
  const currentData = current.data?.data ?? current.data ?? {};
  const merged = { ...currentData, ...settings };
  return axios.put(`${API_BASE}/api/settings/node-filter`, merged);
};

// ============================================================================
// Quality gates
// ============================================================================

export const getQualityGates = (): Promise<AxiosResponse<QualityGateSettings>> =>
  axios.get(`${API_BASE}/api/settings/quality-gates`);

export const updateQualityGates = async (
  settings: Partial<QualityGateSettings>
): Promise<AxiosResponse<void>> => {
  const current = await axios.get(`${API_BASE}/api/settings/quality-gates`);
  const currentData = current.data?.data ?? current.data ?? {};
  const merged = { ...currentData, ...settings };
  const result = await axios.put(`${API_BASE}/api/settings/quality-gates`, merged);

  if ('ontologyPhysics' in settings) {
    toggleOntologyPhysics(!!settings.ontologyPhysics);
  }

  if ('ontologyStrength' in settings) {
    configureOntologyWeights(settings);
  }

  return result;
};

/** Toggle ontology physics forces on the server (fire-and-forget). */
export const toggleOntologyPhysics = async (enabled: boolean): Promise<void> => {
  try {
    const url = enabled
      ? `${API_BASE}/api/ontology-physics/enable`
      : `${API_BASE}/api/ontology-physics/disable`;
    const payload = enabled ? { ontologyId: 'default', strength: 0.8 } : undefined;
    const response = await axios.post(url, payload);
    logger.info(`[settingsApi] ontology physics ${enabled ? 'enabled' : 'disabled'}:`, response.data);
  } catch (err) {
    logger.warn('[settingsApi] ontology physics toggle error:', err);
  }
};

/** Push the ontology constraint weight to the server (fire-and-forget). The semantic-force
 * parameters are not sent from here: they are the physics settings' `semantic` group, written
 * by the ordinary physics PUT. */
export const configureOntologyWeights = async (
  settings: Partial<QualityGateSettings>
): Promise<void> => {
  if (settings.ontologyStrength === undefined) return;
  try {
    await axios.put(`${API_BASE}/api/ontology-physics/weights`, {
      globalStrength: settings.ontologyStrength,
    });
    logger.info('[settingsApi] ontology weight updated:', settings.ontologyStrength);
  } catch (err) {
    logger.warn('[settingsApi] Ontology weights update failed:', err);
  }
};

// ============================================================================
// Visual settings
// ============================================================================

export const getVisualSettings = (): Promise<AxiosResponse<Record<string, unknown>>> =>
  axios.get(`${API_BASE}/api/settings/visual`);

export const updateVisualSettings = async (
  patch: Record<string, unknown>
): Promise<AxiosResponse<Record<string, unknown>>> =>
  axios.put(`${API_BASE}/api/settings/visual`, patch);

// ============================================================================
// All settings (composite)
// ============================================================================

export const getAll = (): Promise<AxiosResponse<AllSettings>> =>
  axios.get(`${API_BASE}/api/settings/all`);

export const getSettingsByPaths = async (
  paths: string[]
): Promise<Record<string, unknown>> => {
  try {
    let allSettings: Record<string, unknown>;
    const now = Date.now();

    if (_cachedAllSettings && (now - _cachedAllTimestamp) < CACHE_TTL_MS) {
      allSettings = _cachedAllSettings;
    } else {
      const response = await axios.get(`${API_BASE}/api/settings/all`);
      allSettings = transformApiToClientSettings(response.data);
      _cachedAllSettings = allSettings;
      _cachedAllTimestamp = now;
    }

    const result: Record<string, unknown> = {};
    for (const path of paths) {
      const value = getNestedValue(allSettings, path);
      if (value !== undefined) {
        result[path] = value;
      }
    }

    return allSettings;
  } catch (error) {
    logger.error('[settingsApi] Failed to get settings:', error);
    return transformApiToClientSettings(emptyAllSettings());
  }
};

export const getSettingByPath = async <T>(path: string): Promise<T> => {
  const allSettings = await getSettingsByPaths([path]);
  const parts = path.split('.');
  let current: unknown = allSettings;
  for (const part of parts) {
    if (current === undefined || current === null || typeof current !== 'object') break;
    current = (current as Record<string, unknown>)[part];
  }
  return current as T;
};

// ============================================================================
// Profiles
// ============================================================================

export const saveProfile = (
  request: SaveProfileRequest
): Promise<AxiosResponse<ProfileIdResponse>> =>
  axios.post(`${API_BASE}/api/settings/profiles`, request);

export const listProfiles = (): Promise<AxiosResponse<SettingsProfile[]>> =>
  axios.get(`${API_BASE}/api/settings/profiles`);

export const loadProfile = (id: number): Promise<AxiosResponse<AllSettings>> =>
  axios.get(`${API_BASE}/api/settings/profiles/${id}`);

export const deleteProfile = (id: number): Promise<AxiosResponse<void>> =>
  axios.delete(`${API_BASE}/api/settings/profiles/${id}`);

// ============================================================================
// Composite path-based update (batches by endpoint)
// ============================================================================

export const updateSettingByPath = async <T>(path: string, value: T): Promise<void> => {
  try {
    if (path.startsWith('visualisation.graphs.') && path.includes('.physics')) {
      await updatePhysics({ [path.split('.').pop()!]: value } as Partial<PhysicsSettings>);
    } else if (path.startsWith('visualisation.rendering')) {
      await updateRendering({ [path.split('.').pop()!]: value } as Partial<RenderingSettings>);
    } else if (path.startsWith('qualityGates')) {
      await updateQualityGates({ [path.split('.').pop()!]: value } as Partial<QualityGateSettings>);
    } else if (path.startsWith('nodeFilter')) {
      await updateNodeFilter({ [path.split('.').pop()!]: value } as Partial<NodeFilterSettings>);
    } else if (path.startsWith('constraints')) {
      await updateConstraints({ [path.split('.').pop()!]: value });
    } else if (isVisualSettingsPath(path)) {
      const visualKey = toVisualKey(path);
      const nested: Record<string, unknown> = {};
      setNestedFromDotPath(nested, visualKey, value);
      await updateVisualSettings(nested);
    } else {
      logger.debug(`[settingsApi] Path "${path}" persisted to localStorage only (no server endpoint)`);
    }
  } catch (error: unknown) {
    const status = (error as { response?: { status?: number } } | undefined)?.response?.status;
    if (status === 401) {
      window.dispatchEvent(new CustomEvent('settings-auth-failed', { detail: { path } }));
      logger.error(`Auth required for "${path}" — settings PUT returned 401`, error);
    } else {
      logger.warn(`Server update failed for "${path}", value persisted locally`, error);
    }
  }
};

/** Merge `patch` into `target`, descending into objects so two sliders of one group in the same
 * debounce window both survive (a shallow assign would keep only the last). */
function mergeInto(target: Record<string, unknown>, patch: Record<string, unknown>): void {
  for (const [k, v] of Object.entries(patch)) {
    const cur = target[k];
    if (v && typeof v === 'object' && !Array.isArray(v) && cur && typeof cur === 'object' && !Array.isArray(cur)) {
      mergeInto(cur as Record<string, unknown>, v as Record<string, unknown>);
    } else {
      target[k] = v;
    }
  }
}

export const updateSettingsByPaths = async (
  updates: Array<{ path: string; value: unknown }>
): Promise<void> => {
  logger.debug(`[SETTINGS-DIAG] updateSettingsByPaths called with ${updates.length} updates:`, updates.map(u => u.path));

  const physicsUpdates: Record<string, unknown> = {};
  const renderingUpdates: Record<string, unknown> = {};
  const qualityGatesUpdates: Record<string, unknown> = {};
  const nodeFilterUpdates: Record<string, unknown> = {};
  const constraintsUpdates: Record<string, unknown> = {};
  const visualUpdates: Record<string, unknown> = {};
  const localOnlyPaths: string[] = [];

  for (const { path, value } of updates) {
    if (path.startsWith('visualisation.graphs.') && path.includes('.physics.')) {
      const entry = physicsPatchEntry(path, value)!;
      mergeInto(physicsUpdates, entry);
      logger.debug(`[SETTINGS-DIAG] routing ${path} → physics ${JSON.stringify(entry)}`);
    } else if (path.startsWith('visualisation.rendering.')) {
      const key = path.split('.').pop()!;
      renderingUpdates[key] = value;
      logger.debug(`[SETTINGS-DIAG] routing ${path} → rendering.${key} = ${value}`);
    } else if (path.startsWith('qualityGates.')) {
      qualityGatesUpdates[path.split('.').pop()!] = value;
    } else if (path.startsWith('nodeFilter.')) {
      nodeFilterUpdates[path.split('.').pop()!] = value;
    } else if (path.startsWith('constraints.')) {
      constraintsUpdates[path.split('.').pop()!] = value;
    } else if (isVisualSettingsPath(path)) {
      setNestedFromDotPath(visualUpdates, toVisualKey(path), value);
      logger.debug(`[SETTINGS-DIAG] routing ${path} → visual.${toVisualKey(path)} = ${value}`);
    } else {
      localOnlyPaths.push(path);
      logger.debug(`[SETTINGS-DIAG] routing ${path} → LOCAL ONLY (no server endpoint)`);
    }
  }

  if (localOnlyPaths.length > 0) {
    logger.debug(`[settingsApi] ${localOnlyPaths.length} paths persisted to localStorage only:`, localOnlyPaths);
  }

  const promises: Promise<unknown>[] = [];
  if (Object.keys(physicsUpdates).length > 0) promises.push(updatePhysics(physicsUpdates as Partial<PhysicsSettings>));
  if (Object.keys(renderingUpdates).length > 0) promises.push(updateRendering(renderingUpdates as Partial<RenderingSettings>));
  if (Object.keys(qualityGatesUpdates).length > 0) promises.push(updateQualityGates(qualityGatesUpdates as Partial<QualityGateSettings>));
  if (Object.keys(nodeFilterUpdates).length > 0) promises.push(updateNodeFilter(nodeFilterUpdates as Partial<NodeFilterSettings>));
  if (Object.keys(constraintsUpdates).length > 0) promises.push(updateConstraints(constraintsUpdates as Partial<ConstraintSettings>));
  if (Object.keys(visualUpdates).length > 0) promises.push(updateVisualSettings(visualUpdates));

  if (promises.length > 0) {
    await Promise.all(promises);
  }
};

// ============================================================================
// Reset / export / import
// ============================================================================

export const resetSettings = async (): Promise<void> => {
  localStorage.removeItem('graph-viz-settings-v2');
  _cachedAllSettings = null;
  _cachedAllTimestamp = 0;
  try {
    const defaultPhysics: Partial<PhysicsSettings> = {
      ...DEFAULT_PHYSICS_SETTINGS,
      autoBalance: false,
      autoBalanceIntervalMs: 5000,
      autoBalanceConfig: { maxIterations: 100, threshold: 0.01 },
      autoPause: { enabled: false, inactivityThresholdMs: 5000 },
    };
    await updatePhysics(defaultPhysics);
  } catch (e) {
    logger.warn('[settingsApi] Failed to reset server settings:', e);
  }
};

export const exportSettings = (settings: Record<string, unknown>): string =>
  JSON.stringify(settings, null, 2);

export const importSettings = (jsonString: string): Record<string, unknown> => {
  const parsed = JSON.parse(jsonString);

  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new Error('Invalid settings format: expected a JSON object');
  }

  const knownSections = [
    'physics', 'constraints', 'rendering', 'nodeFilter', 'qualityGates',
    'visualisation', 'system', 'xr', 'auth',
  ];
  const hasKnownSection = Object.keys(parsed).some(key => knownSections.includes(key));
  if (!hasKnownSection) {
    throw new Error(
      `Invalid settings structure: expected at least one of [${knownSections.join(', ')}]`
    );
  }

  return parsed;
};

export const flushPendingUpdates = async (): Promise<void> => {
  // Updates are synchronous; reserved for future batching.
};
