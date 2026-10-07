import { Settings, SettingsPath, DeepPartial } from '../../features/settings/config/settings'

export type { Settings, SettingsPath, DeepPartial }

// GPU-specific interfaces for type safety
export interface GPUPhysicsParams {
  springK: number;
  springKKnowledge: number;
  springKOntology: number;
  springKAgent: number;
  broadcastFps: number;
  repelK: number;
  attractionK: number;
  gravity: number;
  dt: number;
  maxVelocity: number;
  damping: number;
  temperature: number;
  maxRepulsionDist: number;

  restLength: number;
  repulsionCutoff: number;
  repulsionSofteningEpsilon: number;
  centerGravityK: number;
  gridCellSize: number;
  featureFlags: number;

  warmupIterations: number;
  coolingRate: number;

  enableBounds?: boolean;
  boundsSize?: number;
  boundaryDamping?: number;
  collisionRadius?: number;

  iterations?: number;
  massScale?: number;
  updateThreshold?: number;

  boundaryExtremeMultiplier?: number;
  boundaryExtremeForceMultiplier?: number;
  boundaryVelocityDamping?: number;
  maxForce?: number;
  seed?: number;
  iteration?: number;
}

export interface ConstraintConfig {
  id: string;
  name: string;
  enabled: boolean;
  description?: string;
  icon?: string;
}

export interface WarmupSettings {
  warmupDuration: number;
  convergenceThreshold: number;
  enableAdaptiveCooling: boolean;
  warmupIterations?: number;
  coolingRate?: number;
}

export interface SettingsState {
  partialSettings: DeepPartial<Settings>
  loadedPaths: Set<string>
  loadingSections: Set<string>
  settings: DeepPartial<Settings>

  initialized: boolean
  authenticated: boolean
  user: { isPowerUser: boolean; pubkey: string } | null
  isPowerUser: boolean
  /** Whether settings changes sync to the server. When false, changes are local-only. */
  settingsSyncEnabled: boolean
  setSettingsSyncEnabled: (enabled: boolean) => void

  initialize: () => Promise<void>
  setAuthenticated: (authenticated: boolean) => void
  setUser: (user: { isPowerUser: boolean; pubkey: string } | null) => void
  get: <T>(path: SettingsPath) => T | undefined
  set: <T>(path: SettingsPath, value: T, skipServerSync?: boolean) => void
  subscribe: (path: SettingsPath, callback: () => void, immediate?: boolean) => () => void;
  unsubscribe: (path: SettingsPath, callback: () => void) => void;
  updateSettings: (updater: (draft: Settings) => void) => void;
  notifyViewportUpdate: (path: SettingsPath) => void;

  ensureLoaded: (paths: string[]) => Promise<void>
  loadSection: (section: string) => Promise<void>
  isLoaded: (path: SettingsPath) => boolean

  getByPath: <T>(path: SettingsPath) => Promise<T>;
  setByPath: <T>(path: SettingsPath, value: T) => void;
  batchUpdate: (updates: Array<{path: SettingsPath, value: unknown}>) => void;
  flushPendingUpdates: () => Promise<void>;

  resetSettings: () => Promise<void>;
  exportSettings: () => Promise<string>;
  importSettings: (jsonString: string) => Promise<void>;

  updateComputeMode: (mode: string) => void;
  updateConstraints: (constraints: ConstraintConfig[]) => void;
  updatePhysics: (graphName: string, params: Partial<GPUPhysicsParams>) => void;
  updateWarmupSettings: (settings: WarmupSettings) => void;

  notifyPhysicsUpdate: (graphName: string, params: Partial<GPUPhysicsParams>) => void;

  updateTweening: (graphName: string, params: Partial<{ enabled: boolean; lerpBase: number; snapThreshold: number; maxDivergence: number }>) => void;
  notifyTweeningUpdate: (params: Partial<{ enabled: boolean; lerpBase: number; snapThreshold: number; maxDivergence: number }>) => void;
}
