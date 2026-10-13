// api/settings/types.ts
// Type definitions matching Rust backend exactly

/**
 * The semantic-force group of the physics settings (`physics.semantic`). Its shape, ranges and
 * defaults are defined once, in the Rust field table (`semantic_config::FIELDS`), which is also
 * emitted as `registry/semanticFields.json`; this is deliberately an open nested record rather
 * than a hand-copied list of 35 fields.
 */
export type SemanticForceSettings = Record<string, Record<string, number | boolean>>;

export interface PhysicsSettings {
  /** Every semantic force with its own `enabled` flag (see SemanticForceSettings). */
  semantic?: SemanticForceSettings;
  // --- Simulation control ---
  enabled: boolean;
  autoBalance: boolean;
  autoBalanceIntervalMs: number;
  autoBalanceConfig: {
    maxIterations: number;
    threshold: number;
  };
  autoPause: {
    enabled: boolean;
    inactivityThresholdMs: number;
  };
  dt: number;
  iterations: number;
  warmupIterations: number;
  coolingRate: number;
  globalSpeed: number;
  damping: number;

  // --- Core forces ---
  springK: number;
  repelK: number;
  restLength: number;
  centerGravityK: number;
  gravity: number;
  maxForce: number;
  maxVelocity: number;

  // --- Repulsion & spacing ---
  maxRepulsionDist: number;
  separationRadius: number;
  gridCellSize: number;
  repulsionSofteningEpsilon: number;

  // --- Bounds ---
  enableBounds: boolean;
  boundsSize: number;
  boundaryDamping: number;

  // --- Layout forces (FA2 / dual-graph) ---
  linLogMode: boolean;
  scalingRatio: number;
  adaptiveSpeed: boolean;
  ssspAlpha: number;
  axisCompressionZ: number;
  enableDualDiscLayout: boolean;
  dagBiasK: number;
  dagLevelDistance: number;

  // --- Per-population spring strength (independent KG/ontology/agent layout control) ---
  // Literal kernel coefficient (1.0 == LinLog identity). Drives the GPU spring_scale buffer.
  springKKnowledge: number;
  springKOntology: number;
  springKAgent: number;
  /** Position-broadcast rate in Hz, 1-60 (server default 8). */
  broadcastFps: number;

  // --- Constraints ---
  constraintRampFrames: number;
  constraintMaxForcePerNode: number;
  clusterStrength: number;

  // --- Misc ---
  temperature: number;
}

export type PriorityWeighting = 'linear' | 'exponential' | 'quadratic';

export interface ConstraintSettings {
  lodEnabled: boolean;
  farThreshold: number;
  mediumThreshold: number;
  nearThreshold: number;
  priorityWeighting: PriorityWeighting;
  progressiveActivation: boolean;
  activationFrames: number;
}

export interface RenderingSettings {
  ambientLightIntensity: number;
  backgroundColor: string;
  directionalLightIntensity: number;
  enableAmbientOcclusion: boolean;
  enableAntialiasing: boolean;
  enableShadows: boolean;
  environmentIntensity: number;
  shadowMapSize?: string;
  shadowBias?: number;
  context?: string;
  /** Phase 6 (ADR-04 D1): hard ceiling on dynamic edge capacity (a capacity
   *  ceiling, not a per-frame draw commitment). Default 65_536. */
  maxEdgesCeiling?: number;
  /** Phase 6 (ADR-04 D5): software-WebGL fallback policy. Default 'auto'. */
  softwareFallback?: 'auto' | 'force-on' | 'force-off';
  /** Phase 6 (ADR-04 D6): frames between full label layout rebuilds. Default 3. */
  labelLayoutEvery?: number;
}

export interface NodeFilterSettings {
  enabled: boolean;
  qualityThreshold: number;
  authorityThreshold: number;
  filterByQuality: boolean;
  filterByAuthority: boolean;
  filterMode: 'or' | 'and';
  includeLinkedPages: boolean;
}

export interface QualityGateSettings {
  gpuAcceleration: boolean;
  ontologyPhysics: boolean;
  layoutMode: 'force-directed' | 'dag-topdown' | 'dag-radial' | 'dag-leftright' | 'type-clustering' | 'spectral' | 'clustered';
  showClusters: boolean;
  showAnomalies: boolean;
  showCommunities: boolean;
  showCentrality: boolean;
  showSSSP: boolean;
  ruvectorEnabled: boolean;
  gnnPhysics: boolean;
  minFpsThreshold: number;
  maxNodeCount: number;
  autoAdjust: boolean;
  ontologyStrength: number;
}

export interface AllSettings {
  physics: PhysicsSettings;
  constraints: ConstraintSettings;
  rendering: RenderingSettings;
  nodeFilter: NodeFilterSettings;
  qualityGates: QualityGateSettings;
  visual?: Record<string, unknown>;
}

export interface SettingsProfile {
  id: number;
  name: string;
  createdAt: string;
  updatedAt: string;
}

export interface SaveProfileRequest {
  name: string;
}

export interface ProfileIdResponse {
  id: number;
}

export interface ErrorResponse {
  error: string;
}
