// Type definitions for settings

export type SettingsPath = string | '';

// Node settings
export interface NodeSettings {
  baseColor: string;
  metalness: number;
  opacity: number;
  roughness: number;
  nodeSize: number; 
  quality: 'low' | 'medium' | 'high';
  enableInstancing: boolean;
  enableMetadataShape: boolean;
  enableMetadataVisualisation: boolean;
  enableImportance?: boolean;
  nodeTypeVisibility?: {
    knowledge: boolean;
    ontology: boolean;
    agent: boolean;
  };
}

// Edge settings
export interface EdgeSettings {
  arrowSize: number;
  baseWidth: number;
  color: string;
  enableArrows: boolean;
  opacity: number;
  widthRange: [number, number];
  quality: 'low' | 'medium' | 'high';
  enableFlowEffect: boolean;
  flowSpeed: number;
  flowIntensity: number;
  glowStrength: number;
  distanceIntensity: number;
  useGradient: boolean;
  gradientColors: [string, string];
}

// Physics settings - using camelCase for client.
// These parameters are sent to the SERVER (Rust/CUDA GPU) as the single source
// of truth for layout computation. The client does NOT run force simulation;
// it only performs optimistic interpolation toward server-computed positions.
export interface PhysicsSettings {
  enabled: boolean;
  autoBalance: boolean;

  // --- Server-routed force parameters (PUT /api/physics/parameters) ---
  springK: number;
  repelK: number;
  attractionK: number;
  gravity: number;

  dt: number;
  maxVelocity: number;
  damping: number;
  temperature: number;

  enableBounds: boolean;
  boundsSize: number;
  boundaryDamping: number;
  separationRadius: number;
  collisionRadius?: number;

  restLength: number;
  repulsionCutoff: number;
  repulsionSofteningEpsilon: number;
  centerGravityK: number;
  gridCellSize: number;
  featureFlags: number;

  stressWeight: number;
  stressAlpha: number;
  boundaryLimit: number;
  alignmentStrength: number;
  clusterStrength: number;
  computeMode: number;
  minDistance: number;
  maxRepulsionDist: number;
  boundaryMargin: number;
  boundaryForceStrength: number;

  iterations: number;
  massScale: number;
  updateThreshold: number;

  boundaryExtremeMultiplier: number;
  boundaryExtremeForceMultiplier: number;
  boundaryVelocityDamping: number;
  maxForce: number;
  seed: number;
  iteration: number;

  warmupIterations: number;
  warmupCurve?: string;
  zeroVelocityIterations?: number;
  coolingRate: number;

  clusteringAlgorithm?: string;
  clusterCount?: number;
  clusteringResolution?: number;
  clusteringIterations?: number;

  // SSSP (Single Source Shortest Path) integration
  useSsspDistances?: boolean;
  ssspAlpha?: number;
}

// Client-side interpolation/tweening settings.
// These control how the client smoothly animates toward server-computed positions.
// They are NOT sent to the server — they are purely local visual smoothing.
export interface ClientTweeningSettings {
  /** Whether optimistic tweening is enabled (disable for instant snap) */
  enabled: boolean;
  /** Interpolation base factor: 1 - Math.pow(base, deltaTime). Lower = smoother. Default 0.001 */
  lerpBase: number;
  /** Distance below which positions snap instantly (no tweening). Default 5.0 */
  snapThreshold: number;
  /** Maximum allowed divergence from server before forcing snap. Default 50.0 */
  maxDivergence: number;
}

// Renderer capability report — populated at runtime by rendererFactory.
// Exposed in settings panel so users can see what rendering features are active.
export interface RendererCapabilities {
  /** 'webgpu' | 'webgl' */
  backend: 'webgpu' | 'webgl';
  /** True if TSL (Three Shading Language) node materials are active */
  tslMaterialsActive: boolean;
  /** True if node-based RenderPipeline bloom is active (vs EffectComposer) */
  nodeBasedBloom: boolean;
  /** GPU adapter name (e.g. 'NVIDIA RTX 4090') */
  gpuAdapterName: string;
  /** Maximum texture size */
  maxTextureSize: number;
  /** Device pixel ratio (capped at 2) */
  pixelRatio: number;
}

// Rendering settings
export interface RenderingSettings {
  ambientLightIntensity: number;
  backgroundColor: string;
  directionalLightIntensity: number;
  enableAmbientOcclusion: boolean;
  enableAntialiasing: boolean;
  enableShadows: boolean;
  environmentIntensity: number;
  shadowMapSize: string;
  shadowBias: number;
  context: 'desktop' | 'ar';
  /**
   * Phase 6 (ADR-04 D1): hard ceiling on dynamically-grown edge instance
   * capacity in GlassEdges. Drawn edges beyond this count are skipped with a
   * single structured warning — never silent truncation.
   */
  maxEdgesCeiling?: number;
  /**
   * Phase 6 (ADR-04 D5): behaviour when the WebGL context is detected as
   * software-rendered (SwiftShader / llvmpipe / Microsoft Basic).
   * - 'auto'       -> default: detect and use static-cube fallback
   * - 'force-on'   -> always use the software fallback (no env map)
   * - 'force-off'  -> always render <Environment> regardless of detection
   */
  softwareFallback?: 'auto' | 'force-on' | 'force-off';
  /**
   * Phase 6 (ADR-04 D6 / bug-flag): cadence (in frames) between full
   * InstancedLabels layout rebuilds. Position patching still runs every
   * still frame; only the glyph re-layout pass is throttled. Default 3.
   */
  labelLayoutEvery?: number;
}

// Animation settings
export interface AnimationSettings {
  enableMotionBlur: boolean;
  enableNodeAnimations: boolean;
  motionBlurStrength: number;
  selectionWaveEnabled: boolean;
  pulseEnabled: boolean;
  pulseSpeed: number;
  pulseStrength: number;
  waveSpeed: number;
}

// Label settings
export interface LabelSettings {
  desktopFontSize: number;
  enableLabels: boolean;
  labelDistanceThreshold?: number; // Max camera distance for visible labels (default 500)
  textColor: string;
  textOutlineColor: string;
  textOutlineWidth: number;
  textResolution: number;
  textPadding: number;
  billboardMode: 'camera' | 'vertical';
  showMetadata?: boolean;
  maxLabelWidth?: number;
}


// Glow settings - Unified visual effects with diffuse atmospheric rendering
// This is the server-preferred interface, but client supports both
export interface GlowSettings {
  
  enabled: boolean;
  intensity: number;
  radius: number;
  threshold: number;
  
  
  diffuseStrength: number;
  atmosphericDensity: number;
  volumetricIntensity: number;
  
  
  baseColor: string;
  emissionColor: string;
  opacity: number;
  
  
  pulseSpeed: number;
  flowSpeed: number;
  
  
  nodeGlowStrength: number;
  edgeGlowStrength: number;
  environmentGlowStrength: number;
}

// Hologram settings - Core ring effects (geometry objects removed)
export interface HologramSettings {
  ringCount: number;
  ringColor: string;
  ringOpacity: number;
  sphereSizes: [number, number];
  globalRotationSpeed: number;
  ringRotationSpeed: number;
}

// WebSocket settings
export interface WebSocketSettings {
  reconnectAttempts: number;
  reconnectDelay: number;
  binaryChunkSize: number;
  binaryUpdateRate?: number;
  minUpdateRate?: number;
  maxUpdateRate?: number;
  motionThreshold?: number;
  motionDamping?: number;
  binaryMessageVersion?: number;
  compressionEnabled: boolean;
  compressionThreshold: number;
  heartbeatInterval?: number;
  heartbeatTimeout?: number;
  maxConnections?: number;
  maxMessageSize?: number;
  updateRate: number;
}

// Debug settings
export interface DebugSettings {
  enabled: boolean;
  logLevel?: 'debug' | 'info' | 'warn' | 'error'; 
  logFormat?: 'json' | 'text';
  enableDataDebug: boolean;
  enableWebsocketDebug: boolean;
  logBinaryHeaders: boolean;
  logFullJson: boolean;
  enablePhysicsDebug?: boolean;
  enableNodeDebug?: boolean;
  enableShaderDebug?: boolean;
  enableMatrixDebug?: boolean;
  enablePerformanceDebug?: boolean;
}

// SpacePilot settings
export interface SpacePilotConfig {
  enabled: boolean;
  mode: 'camera' | 'object' | 'navigation';
  sensitivity: {
    translation: number;
    rotation: number;
  };
  smoothing: number;
  deadzone: number;
  buttonFunctions?: Record<number, string>;
}

// XR settings
export interface XRSettings {
  enabled: boolean;
  enableAdaptiveQuality?: boolean;
  clientSideEnableXR?: boolean;
  mode?: 'inline' | 'immersive-vr' | 'immersive-ar';
  roomScale?: number;
  spaceType?: 'local-floor' | 'bounded-floor' | 'unbounded';
  quality?: 'low' | 'medium' | 'high';
  enableHandTracking: boolean;
  handMeshEnabled?: boolean;
  handMeshColor?: string;
  handMeshOpacity?: number;
  handPointSize?: number;
  handRayEnabled?: boolean;
  handRayColor?: string;
  handRayWidth?: number;
  gestureSmoothing?: number;
  enableHaptics: boolean;
  hapticIntensity?: number;
  dragThreshold?: number;
  pinchThreshold?: number;
  rotationThreshold?: number;
  interactionRadius?: number;
  movementSpeed?: number;
  deadZone?: number;
  movementAxes?: {
    horizontal: number;
    vertical: number;
  };
  enableLightEstimation?: boolean;
  enablePlaneDetection?: boolean;
  enableSceneUnderstanding?: boolean;
  planeColor?: string;
  planeOpacity?: number;
  planeDetectionDistance?: number;
  showPlaneOverlay?: boolean;
  snapToFloor?: boolean;
  enablePassthroughPortal?: boolean;
  passthroughOpacity?: number;
  passthroughBrightness?: number;
  passthroughContrast?: number;
  portalSize?: number;
  portalEdgeColor?: string;
  portalEdgeWidth?: number;
  controllerModel?: string;
  renderScale?: number;
  interactionDistance?: number;
  locomotionMethod?: 'teleport' | 'continuous';
  teleportRayColor?: string;
  controllerRayColor?: string;
}

// Head-tracked parallax settings
export interface HeadTrackedParallaxSettings {
  enabled: boolean;
  sensitivity: number;
  cameraMode: 'offset' | 'asymmetricFrustum';
}

// Interaction settings
export interface InteractionSettings {
  headTrackedParallax: HeadTrackedParallaxSettings;
  selectionHighlightColor?: string;
  selectionEdgeFlow?: boolean;
  selectionEdgeFlowSpeed?: number;
  selectionEdgeWidth?: number;
  selectionEdgeOpacity?: number;
}

// Visualisation settings
export interface CameraSettings {
  fov: number;
  near: number;
  far: number;
  position: { x: number; y: number; z: number };
  lookAt?: { x: number; y: number; z: number }; 
}

// Graph-specific settings namespace
export interface GraphSettings {
  nodes: NodeSettings;
  edges: EdgeSettings;
  labels: LabelSettings;
  physics: PhysicsSettings;
  /** Per-graph-type client-side tweening (overrides top-level clientTweening) */
  tweening?: ClientTweeningSettings;
}

// Multi-graph namespace structure
export interface GraphsSettings {
  logseq: GraphSettings;
  [key: string]: GraphSettings;
}

// Graph-type-specific visual settings
export interface KnowledgeGraphVisualSettings {
  edgeColor?: string;  // default '#4FC3F7'
  metalness?: number;
  roughness?: number;
  glowStrength?: number;
  innerGlowIntensity?: number;
  facetDetail?: number;
  authorityScaleFactor?: number;
  connectionInfluence?: number;
  globalScaleMultiplier?: number;
  showDomainBadge?: boolean;
  showQualityStars?: boolean;
  showRecencyIndicator?: boolean;
  showConnectionDensity?: boolean;
}

export interface OntologyVisualSettings {
  edgeColor?: string;  // default '#AA96DA'
  glowStrength?: number;
  orbitalRingCount?: number;
  orbitalRingSpeed?: number;
  hierarchyScaleFactor?: number;
  minScale?: number;
  instanceCountInfluence?: number;
  depthColorGradient?: boolean;
  showHierarchyBreadcrumb?: boolean;
  showInstanceCount?: boolean;
  showConstraintStatus?: boolean;
  nebulaGlowIntensity?: number;
}

export interface AgentVisualSettings {
  membraneOpacity?: number;
  nucleusGlowIntensity?: number;
  breathingSpeed?: number;
  breathingAmplitude?: number;
  workloadInfluence?: number;
  tokenRateInfluence?: number;
  tokenRateCap?: number;
  showHealthBar?: boolean;
  showTokenRate?: boolean;
  showTaskCount?: boolean;
  bioluminescentIntensity?: number;
}

export interface GraphTypeVisualsSettings {
  knowledgeGraph?: KnowledgeGraphVisualSettings;
  ontology?: OntologyVisualSettings;
  agent?: AgentVisualSettings;
}

// Scene effects settings (WASM-powered ambient visuals)
export interface SceneEffectsSettings {
  enabled?: boolean;
  // Particle field
  particleCount?: number;      // 64-512, default 256
  particleOpacity?: number;    // 0-1, default 0.3
  particleDrift?: number;      // 0-2, default 0.5
  particleColor?: string;      // CSS hex/rgb, default '#6680E6' (RGB 0.4, 0.5, 0.9)
  // Atmosphere/fog
  fogEnabled?: boolean;        // default true
  fogOpacity?: number;         // 0-0.15, default 0.03
  atmosphereResolution?: number; // 64-256, default 128
  // Energy wisps
  wispsEnabled?: boolean;      // default true
  wispCount?: number;          // 8-128, default 48
  wispOpacity?: number;        // 0-1, default 0.3
  wispDriftSpeed?: number;     // 0-3, default 1.0
  wispColor?: string;          // CSS hex/rgb, default '#668FCC' (HSL 0.6, 0.7, 0.6)
  // Ambient glow
  ambientGlowEnabled?: boolean; // default true
  ambientGlowOpacity?: number;  // 0-0.1, default 0.02
}

export interface GemMaterialSettings {
  ior: number;                // 1.0-3.0, default 2.42
  transmission: number;       // 0-1, default 0.6
  clearcoat: number;          // 0-1, default 1.0
  clearcoatRoughness: number; // 0-0.5, default 0.02
  emissiveIntensity: number;  // 0-2, default 0.3
  iridescence: number;        // 0-1, default 0.3
}

export interface ClusterHullSettings {
  enabled: boolean;
  opacity: number;      // 0-0.3, default 0.08
  padding: number;      // 0-0.5, default 0.15
  updateInterval: number; // frames between hull recalculation, default 30
}

// Embedding cloud layer settings (PCA-projected RuVector embeddings)
export interface EmbeddingCloudSettings {
  enabled: boolean;
  pointSize: number;       // 0.5-25, default 7.5
  opacity: number;         // 0-1, default 0.6
  colorBy: 'namespace' | 'sourceType';
  rotationSpeed: number;   // rad/frame, default 0.0005
  maxPoints: number;       // cap on rendered points, default 50000
  cloudScale: number;      // overall scale multiplier, default 5.0
}

export interface VisualisationSettings {

  rendering: RenderingSettings;
  animations: AnimationSettings;
  glow: GlowSettings;
  hologram: HologramSettings;
  spacePilot?: SpacePilotConfig;
  camera?: CameraSettings;
  interaction?: InteractionSettings;

  // Scene ambient effects (particles, fog, glow ring)
  sceneEffects?: SceneEffectsSettings;

  // Gem node material properties
  gemMaterial?: GemMaterialSettings;

  // Cluster hull visualization
  clusterHulls?: ClusterHullSettings;

  // Embedding cloud layer (PCA-projected RuVector vector embeddings)
  embeddingCloud?: EmbeddingCloudSettings;

  // Graph-type-specific visual settings
  graphTypeVisuals?: GraphTypeVisualsSettings;

  // Convenience accessors — components read these directly via DeepPartial<VisualisationSettings>
  bloom?: GlowSettings;
  nodes?: NodeSettings;
  edges?: EdgeSettings;
  labels?: LabelSettings;

  graphs: GraphsSettings;
}

// System settings
export interface SystemSettings {
  websocket: WebSocketSettings;
  debug: DebugSettings;
  persistSettings: boolean; 
  customBackendUrl?: string; 
}

// RAGFlow settings
export interface RAGFlowSettings {
  apiKey?: string;
  agentId?: string;
  apiBaseUrl?: string;
  timeout?: number;
  maxRetries?: number;
  chatId?: string;
}

// Perplexity settings
export interface PerplexitySettings {
  apiKey?: string;
  model?: string;
  apiUrl?: string;
  maxTokens?: number;
  temperature?: number;
  topP?: number;
  presencePenalty?: number;
  frequencyPenalty?: number;
  timeout?: number;
  rateLimit?: number;
}

// OpenAI settings
export interface OpenAISettings {
  apiKey?: string;
  baseUrl?: string;
  timeout?: number;
  rateLimit?: number;
}

// Kokoro TTS settings
export interface KokoroSettings {
  apiUrl?: string;
  defaultVoice?: string;
  defaultFormat?: string;
  defaultSpeed?: number;
  timeout?: number;
  stream?: boolean;
  returnTimestamps?: boolean;
  sampleRate?: number;
}

// Auth settings
export interface AuthSettings {
  enabled: boolean;
  provider: 'nostr' | string; 
  required: boolean;
  nostr?: {
    connected: boolean;
    publicKey: string;
    isPowerUser?: boolean;
  };
}

// Whisper speech recognition settings
export interface WhisperSettings {
  apiUrl?: string;
  defaultModel?: string;
  defaultLanguage?: string;
  timeout?: number;
  temperature?: number;
  returnTimestamps?: boolean;
  vadFilter?: boolean;
  wordTimestamps?: boolean;
  initialPrompt?: string;
}

// Dashboard GPU status settings
export interface DashboardSettings {
  autoRefresh: boolean;
  refreshInterval: number;
  computeMode: 'Basic Force-Directed' | 'Dual Graph' | 'Constraint-Enhanced' | 'Visual Analytics';
  iterationCount: number;
  showConvergence: boolean;
  activeConstraints: number;
  clusteringActive: boolean;
}

// Analytics settings with GPU clustering
export interface AnalyticsSettings {
  enableMetrics?: boolean;
  updateInterval: number;
  showDegreeDistribution: boolean;
  showClusteringCoefficient: boolean;
  showCentrality: boolean;
  clustering: {
    algorithm: 'none' | 'kmeans' | 'spectral' | 'louvain';
    clusterCount: number;
    resolution: number;
    iterations: number;
    exportEnabled: boolean;
    importEnabled: boolean;
  };
}

// Performance settings with warmup controls
export interface PerformanceSettings {
  enableAdaptiveQuality: boolean;
  warmupDuration: number;
  convergenceThreshold: number;
  enableAdaptiveCooling: boolean;
}

// Developer GPU debug settings
export interface QualityGatesSettings {
  gpuAcceleration: boolean;
  ontologyPhysics: boolean;
  semanticForces: boolean;
  layoutMode: 'force-directed' | 'dag-topdown' | 'dag-radial' | 'dag-leftright' | 'type-clustering';
  showClusters: boolean;
  showAnomalies: boolean;
  showCommunities: boolean;
  ruvectorEnabled: boolean;
  gnnPhysics: boolean;
  minFpsThreshold: number;
  maxNodeCount: number;
  autoAdjust: boolean;
  // Semantic force parameters (propagated to GPU actors)
  ontologyStrength?: number;       // 0-1, default 0.5 — global ontology constraint weight
  dagLevelAttraction?: number;     // 0-2, default 0.5 — DAG hierarchy level pull
  dagSiblingRepulsion?: number;    // 0-2, default 0.3 — same-level spread force
  typeClusterAttraction?: number;  // 0-2, default 0.3 — same-type grouping force
  typeClusterRadius?: number;      // 10-500, default 100 — cluster zone radius
}

export interface DeveloperSettings {
  gpu: {
    showForceVectors: boolean;
    showConstraints: boolean;
    showBoundaryForces: boolean;
    showConvergenceGraph: boolean;
  };
  constraints: {
    active: Array<{
      id: string;
      name: string;
      enabled: boolean;
      description?: string;
      icon?: string;
    }>;
  };
}

// XR GPU optimization settings
export interface XRGPUSettings {
  enableOptimizedCompute: boolean;
  performance: {
    preset: 'Battery Saver' | 'Balanced' | 'Performance';
  };
  physics: {
    scale: number;
  };
}

// Vircadia integration settings
export interface VircadiaSettings {
  enabled: boolean;
  serverUrl: string;
  autoConnect: boolean;
}

// Node filter settings for graph visualization
export interface NodeFilterSettings {
  enabled: boolean;
  minConnections?: number;
  maxConnections?: number;
  nodeTypes?: string[];
  searchQuery?: string;
  qualityThreshold?: number;
  authorityThreshold?: number;
  filterByQuality?: boolean;
  filterByAuthority?: boolean;
  filterMode?: 'and' | 'or';
}

// Main settings interface - Single source of truth matching server AppFullSettings
export interface Settings {
  visualisation: VisualisationSettings;
  system: SystemSettings;
  xr: XRSettings & { gpu?: XRGPUSettings };
  auth: AuthSettings;
  ragflow?: RAGFlowSettings;
  perplexity?: PerplexitySettings;
  openai?: OpenAISettings;
  kokoro?: KokoroSettings;
  whisper?: WhisperSettings;
  dashboard?: DashboardSettings;
  analytics?: AnalyticsSettings;
  performance?: PerformanceSettings;
  developer?: DeveloperSettings;
  qualityGates?: QualityGatesSettings;
  vircadia?: VircadiaSettings;
  // Node filter settings for graph visualization
  nodeFilter?: NodeFilterSettings;
  // Client-side tweening for server-authoritative positions
  clientTweening?: ClientTweeningSettings;
  // Runtime renderer capabilities (populated by rendererFactory, read-only in UI)
  rendererCapabilities?: RendererCapabilities;
}

// Partial update types for settings mutations
export type DeepPartial<T> = T extends object ? {
  [P in keyof T]?: DeepPartial<T[P]>;
} : T;

export type SettingsUpdate = DeepPartial<Settings>;
