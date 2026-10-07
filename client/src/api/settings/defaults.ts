// api/settings/defaults.ts
// Default settings objects — must mirror Rust backend defaults exactly

import type { PhysicsSettings, QualityGateSettings } from './types';

// ============================================================================
// Physics defaults — must mirror Rust SimParams defaults exactly
// ============================================================================

export const DEFAULT_PHYSICS_SETTINGS: Partial<PhysicsSettings> = {
  // --- Simulation control ---
  enabled: true,
  autoBalance: false,
  dt: 0.016,
  iterations: 50,
  warmupIterations: 100,
  coolingRate: 0.001,
  globalSpeed: 0.4,
  damping: 0.9,

  // --- Core forces (canonical compact profile — settles within ~400u bounds) ---
  springK: 12.0,
  repelK: 120.0,
  restLength: 50.0,
  centerGravityK: 0.2,
  gravity: 0.002,
  maxForce: 150.0,
  maxVelocity: 100.0,

  // --- Repulsion & spacing ---
  maxRepulsionDist: 400.0,
  separationRadius: 2.1155233,
  gridCellSize: 50.0,
  repulsionSofteningEpsilon: 0.0001,

  // --- Bounds ---
  // Soft-cube containment sized to the ~400-unit graph envelope. Client fallback
  // only — the backend PhysicsSettings::default() is the single source of truth
  // and is hydrated over these on connect.
  enableBounds: true,
  boundsSize: 400.0,
  boundaryDamping: 0.95,

  // --- Layout forces (FA2 / dual-graph) ---
  linLogMode: true,
  scalingRatio: 10.0,
  adaptiveSpeed: true,
  ssspAlpha: 1.5,
  graphSeparationX: 0.0,
  axisCompressionZ: 1.0,
  enableDualDiscLayout: false,
  dagBiasK: 0.0,
  dagLevelDistance: 60.0,

  // --- Per-population spring strength (independent KG/ontology/agent layout control) ---
  springKKnowledge: 1.0,
  springKOntology: 1.0,
  springKAgent: 1.0,

  // --- Constraints ---
  constraintRampFrames: 60,
  constraintMaxForcePerNode: 50.0,
  clusterStrength: 0.0,

  // --- Misc ---
  temperature: 0.0,
};

// ============================================================================
// Visual effect defaults (used when API doesn't provide them)
// ============================================================================

export const DEFAULT_GLOW_SETTINGS = {
  enabled: true,
  intensity: 0.5,
  radius: 0.3,
  threshold: 0.3,
  diffuseStrength: 0.3,
  atmosphericDensity: 0.2,
  volumetricIntensity: 0.3,
  baseColor: '#ffffff',
  emissionColor: '#00ffff',
  opacity: 1.0,
  pulseSpeed: 1.0,
  flowSpeed: 0.5,
  nodeGlowStrength: 0.6,
  edgeGlowStrength: 0.3,
  environmentGlowStrength: 0.2
};

export const DEFAULT_BLOOM_SETTINGS = {
  enabled: true,
  intensity: 0.4,
  // Threshold sits above the (now neutral-exposure) base lighting so only true
  // emissive highlights bloom. At 0.3 the whole lit scene cleared the threshold
  // and bloomed uniformly, swamping per-node emissive hue toward white.
  threshold: 0.6,
  radius: 0.3,
  strength: 0.4
};

export const DEFAULT_HOLOGRAM_SETTINGS = {
  ringCount: 3,
  ringColor: '#00ffff',
  ringOpacity: 0.5,
  sphereSizes: [100, 150] as [number, number],
  globalRotationSpeed: 0.5,
  ringRotationSpeed: 0.5,
};

export const DEFAULT_GEM_MATERIAL = {
  ior: 2.42,
  transmission: 0.6,
  clearcoat: 1.0,
  clearcoatRoughness: 0.02,
  emissiveIntensity: 0.6,
  iridescence: 0.3,
};

export const DEFAULT_SCENE_EFFECTS = {
  enabled: true,
  particleCount: 128,
  particleOpacity: 0.3,
  particleDrift: 0.5,
  particleColor: '#6680E6',
  wispsEnabled: true,
  wispCount: 32,
  wispOpacity: 0.4,
  wispDriftSpeed: 1.0,
  wispColor: '#668FCC',
  fogEnabled: false,
  fogOpacity: 0.05,
  atmosphereResolution: 128,
};

export const DEFAULT_CLUSTER_HULLS = {
  // ADR-031 D6: the hull layer renders ONLY server-provided clusters. Enabled by
  // default so correct server clusters draw hulls; the JS spatial-grid fallback
  // is a separate opt-in (spatialFallback, default off) and never fabricates hulls.
  enabled: true,
  // At 0.12 the hulls are effectively invisible against the dark scene at the
  // default auto-fit zoom (the clustered core occupies a small screen region);
  // 0.25 makes them legible while staying translucent enough to see the nodes
  // and overlapping clusters through them.
  opacity: 0.25,
  padding: 0.15,
  updateInterval: 30,
  maxHulls: 32,
  slabThickness: 35,
  // ADR-031 D6: opt-in fabricated spatial-grid hulls. Default OFF — when server
  // clusters are absent, show an empty state, never a fabricated grid.
  spatialFallback: false,
  // ADR-031 D6: opt-in Louvain-community hulls. Default OFF — community_id is
  // real but optimised for modularity not spatial locality, so its hulls overlap
  // into a blob. The honest default community signal is per-node colour
  // (nodes.colorScheme: 'community'); enable this only when you want the volumes.
  communityFallback: false,
};

// ADR-031 D6: ship qualityGates defaults so correct server analytics render by
// default. Absent defaults made the whole analytics subsystem invisible.
export const DEFAULT_QUALITY_GATES: QualityGateSettings = {
  gpuAcceleration: true,
  ontologyPhysics: false,
  semanticForces: false,
  layoutMode: 'force-directed',
  showClusters: true,
  showAnomalies: true,
  showCommunities: true,
  showCentrality: true,
  showSSSP: false,
  ruvectorEnabled: false,
  gnnPhysics: false,
  minFpsThreshold: 30,
  // Retained for settings-wire compatibility; graph selection is quality-only.
  maxNodeCount: Number.MAX_SAFE_INTEGER,
  autoAdjust: true,
  ontologyStrength: 0.5,
  dagLevelAttraction: 0.5,
  dagSiblingRepulsion: 0.3,
  typeClusterAttraction: 0.3,
  typeClusterRadius: 100,
};

// W-G phase-1 client-only overlay defaults. All fields DEFAULT-OFF: the governed
// decision/provenance surfaces stay hidden until a user opts in via the Control
// Center (Decisions / Provenance groups). Client-only — never PUT server-side.
export const DEFAULT_DECISIONS_SETTINGS = {
  showDecisionChains: false,
  highlightPrecedents: false,
};

export const DEFAULT_PROVENANCE_SETTINGS = {
  showAttribution: false,
  showGateChips: false,
  enableTimeline: false,
  diffMode: false,
};

export const DEFAULT_EMBEDDING_CLOUD = {
  // Off by default: the RuVector memory cloud fills the scene with a second
  // 50k-point field that obscures the graph itself at rest (operator call,
  // 2026-07-17 — 'to get a clear view'). Opt in via Atmosphere → Embedding Cloud.
  enabled: false,
  pointSize: 7.5,
  opacity: 0.6,
  colorBy: 'namespace' as const,
  rotationSpeed: 0.0005,
  maxPoints: 50000,
  cloudScale: 5.0,
  trajectoryView: 'canopy' as const,
  routeGlow: 1.2,
  playbackSpeed: 1,
  showRejected: true,
  dimOffRoute: 0.75,
  learningEnabled: true,
  learningTargetRecall: 0.9,
  learningRate: 0.2,
  cinematic: false,
};

export const DEFAULT_ANIMATION_SETTINGS = {
  enableMotionBlur: false,
  enableNodeAnimations: true,
  motionBlurStrength: 0.5,
  selectionWaveEnabled: true,
  pulseEnabled: true,
  pulseSpeed: 1.0,
  pulseStrength: 0.5,
  waveSpeed: 1.0,
};

export const DEFAULT_INTERACTION_SETTINGS = {
  selectionHighlightColor: '#ffff00',
  selectionEdgeFlow: false,
  selectionEdgeFlowSpeed: 1.0,
  selectionEdgeWidth: 0.5,
  selectionEdgeOpacity: 0.8,
};

export const DEFAULT_NODES_SETTINGS = {
  baseColor: '#4a6fa5',
  // ADR-031 D6: default to 'community' so the live Louvain partition renders out
  // of the box. Nodes the server left unclustered (community_id 0) fall through
  // to per-type colouring in computeColor, so the default is never all-grey.
  colorScheme: 'community' as 'type' | 'domain' | 'base' | 'community' | 'cluster' | 'centrality' | 'sssp',
  sizeScheme: 'hybrid' as 'degree' | 'fileSize' | 'hybrid',
  perNodeGlow: true,
  metalness: 0.1,
  opacity: 1.0,
  roughness: 0.6,
  nodeSize: 0.4,
  quality: 'high' as const,
  enableInstancing: true,
  enableMetadataShape: false,
  enableMetadataVisualisation: true,
  nodeTypeVisibility: {
    knowledge: true,
    ontology: true,
    agent: true,
  }
};

export const DEFAULT_EDGES_SETTINGS = {
  arrowSize: 0.02,
  baseWidth: 0.1,
  color: '#ff0000',
  enableArrows: false,
  opacity: 0.15,
  colorByType: true,
  widthByWeight: true,
  widthRange: [0.3, 1.5] as [number, number],
  quality: 'high' as const,
  enableFlowEffect: false,
  flowSpeed: 1.0,
  flowIntensity: 0.5,
  glowStrength: 0.3,
  distanceIntensity: 0.5,
  useGradient: false,
  gradientColors: ['#4a9eff', '#ff4a9e'] as [string, string]
};

export const DEFAULT_LABELS_SETTINGS = {
  desktopFontSize: 0.4,
  enableLabels: true,
  labelDistanceThreshold: 1200,
  textColor: '#676565',
  textOutlineColor: '#00ff40',
  textOutlineWidth: 0.0074725277,
  textResolution: 32,
  textPadding: 0.3,
  billboardMode: 'camera' as const,
  showMetadata: true,
  maxLabelWidth: 5.0
};

export const DEFAULT_GRAPH_TYPE_VISUALS = {
  knowledgeGraph: {
    metalness: 0.6,
    roughness: 0.15,
    glowStrength: 2.5,
    innerGlowIntensity: 0.3,
    facetDetail: 2,
    authorityScaleFactor: 0.5,
    connectionInfluence: 0.4,
    globalScaleMultiplier: 2.5,
    showDomainBadge: true,
    showQualityStars: true,
    showRecencyIndicator: true,
    showConnectionDensity: false,
    // File-attention heat: knowledge/ontology nodes glow as agents touch them
    // (0x23), decaying with this half-life (seconds). Consumed by GemNodes.
    attentionHeatEnabled: true,
    attentionHeatHalfLife: 20,
  },
  ontology: {
    ringTintByClass: true,
    glowStrength: 1.8,
    orbitalRingCount: 8,
    orbitalRingSpeed: 0.5,
    hierarchyScaleFactor: 0.15,
    minScale: 0.4,
    instanceCountInfluence: 0.1,
    depthColorGradient: true,
    showHierarchyBreadcrumb: true,
    showInstanceCount: true,
    showConstraintStatus: false,
    nebulaGlowIntensity: 0.7,
  },
  agent: {
    membraneOpacity: 0.7,
    nucleusGlowIntensity: 0.6,
    breathingSpeed: 1.5,
    breathingAmplitude: 0.4,
    workloadInfluence: 0.3,
    tokenRateInfluence: 100,
    tokenRateCap: 0.5,
    showHealthBar: true,
    showTokenRate: true,
    showTaskCount: false,
    bioluminescentIntensity: 0.6,
    // Four configurable health→glow stops; defaults reproduce the historical
    // six-tier ramp exactly (see agentVisualConstants.DEFAULT_HEALTH_COLORS).
    healthColors: {
      excellent: '#00FF00',
      good: '#2ECC71',
      warning: '#F39C12',
      critical: '#E74C3C',
    },
    // Action-beam (0x23) styling consumed by TransientBeamsLayer via GraphManager.
    beamRadius: 0.35,
    beamOpacity: 0.85,
    // Motion-trail ribbons behind each agent (BotsNode → AgentTrail). Client-only.
    showTrails: true,
    trailLength: 24,
    // Nameplate LOD (W3D): declutter dense swarms — full nameplate only close in
    // or hovered/selected/queen, name-only mid-range, hidden far, with hysteresis.
    nameplateLod: true,
    nameplateFullDistance: 40,
  },
};
