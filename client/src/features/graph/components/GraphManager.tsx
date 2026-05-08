import React, { useRef, useEffect, useState, useMemo, useCallback } from 'react'
import { useThree, useFrame, ThreeEvent } from '@react-three/fiber'
// Text, Billboard, Html removed — InstancedLabels handles all label rendering
import * as THREE from 'three'
import { isWebGPURenderer } from '../../../rendering/rendererFactory'
import { graphDataManager, type GraphData, type Node as KGNode } from '../managers/graphDataManager'
import { graphWorkerProxy } from '../managers/graphWorkerProxy'
import { usePlatformStore } from '../../../services/platformManager'
import { createLogger } from '../../../utils/loggerConfig'
import { debugState } from '../../../utils/clientDebugState'
import { useSettingsStore } from '../../../store/settingsStore'
import { useSelectionAndFocusEvents } from '../hooks/useUserInteractionEvents'
import { BinaryNodeData } from '../../../types/binaryProtocol'
import { GemNodes, GemNodesHandle } from './GemNodes'
import { MetadataShapes } from './MetadataShapes'
import { GlassEdges, GlassEdgesHandle } from './GlassEdges'
import { KnowledgeRings } from './KnowledgeRings'
import { ClusterHulls } from './ClusterHulls'
import { useGraphEventHandlers } from '../hooks/useGraphEventHandlers'
import { EdgeSettings } from '../../settings/config/settings'
import { useAnalyticsStore, useCurrentSSSPResult } from '../../analytics/store/analyticsStore'
import { AgentNodesLayer, useAgentNodes } from '../../visualisation/components/AgentNodesLayer'
import { useGraphVisualState, type GraphVisualMode } from '../hooks/useGraphVisualState'
import { useGraphFiltering } from '../hooks/useGraphFiltering'
import { useFpsMonitor } from '../hooks/useFpsMonitor'
import { useCameraAutoFit } from '../hooks/useCameraAutoFit'
import { computeNodeScale } from '../utils/nodeScaling'
import { InstancedLabels } from './InstancedLabels'
import { layoutApi, type LayoutPosition } from '../../../api/layoutApi'
import { SovereignOverlay } from './SovereignOverlay'
import { CollapsedGroupRings } from './CollapsedGroupRings'

const logger = createLogger('GraphManager')

// Re-export GraphVisualMode from the hook for downstream consumers
export type { GraphVisualMode } from '../hooks/useGraphVisualState';

// === PERFORMANCE OPTIMIZATION: Domain colors defined once outside component ===
const DOMAIN_COLORS: Record<string, string> = {
  'AI': '#4FC3F7',   // Light blue
  'BC': '#81C784',   // Green
  'RB': '#FFB74D',   // Orange
  'MV': '#CE93D8',   // Purple
  'TC': '#FFD54F',   // Yellow
  'DT': '#EF5350',   // Red
  'NGM': '#4DB6AC',  // Teal
};
const DEFAULT_DOMAIN_COLOR = '#90A4AE'; // Grey

// Edge type to color mapping (DDD EdgeType enum from ddd-semantic-pipeline.md)
// Colors are chosen to visually distinguish relationship semantics at a glance.
const EDGE_TYPE_COLORS: Record<string, THREE.Color> = {
  'hierarchical':  new THREE.Color('#FFD700'),   // gold — strongest relationship
  'subclass':      new THREE.Color('#FFD700'),   // gold alias
  'structural':    new THREE.Color('#4FC3F7'),   // blue
  'has_part':      new THREE.Color('#4FC3F7'),   // blue alias
  'is_part_of':    new THREE.Color('#4FC3F7'),   // blue alias
  'dependency':    new THREE.Color('#81C784'),   // green
  'requires':      new THREE.Color('#81C784'),   // green alias
  'depends_on':    new THREE.Color('#81C784'),   // green alias
  'enables':       new THREE.Color('#81C784'),   // green alias
  'associative':   new THREE.Color('#CE93D8'),   // purple
  'relates_to':    new THREE.Color('#CE93D8'),   // purple alias
  'bridge':        new THREE.Color('#FF7043'),   // orange — cross-domain
  'bridges_to':    new THREE.Color('#FF7043'),   // orange alias
  'bridges_from':  new THREE.Color('#FF7043'),   // orange alias
  // ADR-048 BRIDGE_TO kinds — distinct hues to mark promotion state
  'bridge_colocated': new THREE.Color('#5DADE2'),// sky blue — co-existence
  'bridge_candidate': new THREE.Color('#F5B041'),// amber — awaiting broker
  'bridge_promoted':  new THREE.Color('#27AE60'),// green — confirmed promotion
  'bridge_revoked':   new THREE.Color('#7F8C8D'),// grey — rolled back
  'bridge_rejected':  new THREE.Color('#E74C3C'),// red — declined
  // Raw Neo4j relationship type fallback
  'SUBCLASS_OF':   new THREE.Color('#FFD700'),   // gold — treat as hierarchical
  'subclass_of':   new THREE.Color('#FFD700'),
  'explicit_link': new THREE.Color('#FFFFFF'),   // white — default wikilink
  'namespace':     new THREE.Color('#78909C'),   // grey — weakest grouping
  'inferred':      new THREE.Color('#B0BEC5'),   // light grey — reasoner output
};
const DEFAULT_EDGE_COLOR = new THREE.Color('#AAAAAA');

/** Resolve edge type string to a pre-allocated THREE.Color. */
function getEdgeTypeColor(edgeType?: string): THREE.Color {
  if (!edgeType) return DEFAULT_EDGE_COLOR;
  return EDGE_TYPE_COLORS[edgeType] ?? EDGE_TYPE_COLORS[edgeType.toLowerCase()] ?? DEFAULT_EDGE_COLOR;
}

/** Resolve edge-type opacity (ADR-048). Bridge edges dim so tiers read as
 *  separate layers; strong relationships (hierarchical, promoted) go solid. */
const EDGE_TYPE_ALPHA: Record<string, number> = {
  'hierarchical':     0.9,
  'SUBCLASS_OF':      0.9,
  'subclass_of':      0.9,
  'bridge_colocated': 0.55,
  'bridge_candidate': 0.35,
  'bridge_promoted':  0.85,
  'bridge_revoked':   0.25,
  'bridge_rejected':  0.3,
  'bridge':           0.5,
  'inferred':         0.4,
};
function getEdgeTypeAlpha(edgeType?: string): number {
  if (!edgeType) return 1.0;
  return EDGE_TYPE_ALPHA[edgeType] ?? EDGE_TYPE_ALPHA[edgeType.toLowerCase()] ?? 1.0;
}

// O(1) domain color lookup
const getDomainColor = (domain?: string): string => {
  return domain && DOMAIN_COLORS[domain] ? DOMAIN_COLORS[domain] : DEFAULT_DOMAIN_COLOR;
};

// === ONTOLOGY MODE: Hierarchy depth color spectrum (cosmic) ===
const ONTOLOGY_DEPTH_COLORS: THREE.Color[] = [
  new THREE.Color('#FF6B6B'), // depth 0: red giant
  new THREE.Color('#FFD93D'), // depth 1: yellow star
  new THREE.Color('#4ECDC4'), // depth 2: cyan nebula
  new THREE.Color('#AA96DA'), // depth 3: purple distant
  new THREE.Color('#95E1D3'), // depth 4+: pale ethereal
];
const ONTOLOGY_PROPERTY_COLOR = new THREE.Color('#F38181');
const ONTOLOGY_INSTANCE_COLOR = new THREE.Color('#B8D4E3');

// === AGENT MODE: Status-based bioluminescence ===
const AGENT_STATUS_COLORS: Record<string, THREE.Color> = {
  'active': new THREE.Color('#2ECC71'),
  'busy': new THREE.Color('#F39C12'),
  'idle': new THREE.Color('#95A5A6'),
  'error': new THREE.Color('#E74C3C'),
  'default': new THREE.Color('#2ECC71'),
};
const AGENT_TYPE_COLORS: Record<string, THREE.Color> = {
  'queen': new THREE.Color('#FFD700'),
  'coordinator': new THREE.Color('#E67E22'),
};

// (Material mode presets removed -- GemNodes handles mode switching internally)

// Metadata overlay helpers (getQualityStars, getRecencyText, etc.) and the old
// NodeLabel component have been removed. InstancedLabels.tsx contains its own
// copies of these helpers to avoid circular imports.

// Enhanced position calculation with better distribution
const getPositionForNode = (node: KGNode, index: number, totalNodes: number): [number, number, number] => {
  if (!node.position || (node.position.x === 0 && node.position.y === 0 && node.position.z === 0)) {
    
    const goldenAngle = Math.PI * (3 - Math.sqrt(5))
    const theta = index * goldenAngle
    const y = 1 - (index / totalNodes) * 2
    const radius = Math.sqrt(1 - y * y)

    const scaleFactor = 15
    const x = Math.cos(theta) * radius * scaleFactor
    const z = Math.sin(theta) * radius * scaleFactor
    const yScaled = y * scaleFactor

    if (node.position) {
      node.position.x = x
      node.position.y = yScaled
      node.position.z = z
    } else {
      node.position = { x, y: yScaled, z }
    }

    return [x, yScaled, z]
  }

  return [node.position.x, node.position.y, node.position.z]
}

// (LOD geometry sets removed -- GemNodes manages its own geometry)

// Reusable Color for getNodeColor to eliminate per-call allocation
const _nodeColor = new THREE.Color();

// Pre-computed type colors as THREE.Color instances (avoid re-parsing hex strings)
const TYPE_THREE_COLORS: Record<string, THREE.Color> = {
  'folder': new THREE.Color('#FFD700'),
  'file': new THREE.Color('#00CED1'),
  'function': new THREE.Color('#FF6B6B'),
  'class': new THREE.Color('#4ECDC4'),
  'variable': new THREE.Color('#95E1D3'),
  'import': new THREE.Color('#F38181'),
  'export': new THREE.Color('#AA96DA'),
  'default': new THREE.Color('#00ffff'),
};

// === MODE-AWARE NODE COLOR ===
// Returns the shared _nodeColor instance -- caller must use values before next call
const getNodeColor = (
  node: KGNode,
  ssspResult?: any,
  graphMode: GraphVisualMode = 'knowledge_graph',
  hierarchyMap?: Map<string, any>,
  connectionCountMap?: Map<string, number>
): THREE.Color => {

  // SSSP visualization overrides all modes
  if (ssspResult) {
    const distance = ssspResult.distances[node.id]

    if (node.id === ssspResult.sourceNodeId) {
      return _nodeColor.set('#00FFFF')
    }

    if (!isFinite(distance)) {
      return _nodeColor.set('#666666')
    }

    const normalizedDistances = ssspResult.normalizedDistances || {}
    const normalizedDistance = normalizedDistances[node.id] || 0

    const red = Math.min(1, normalizedDistance * 1.2)
    const green = Math.min(1, (1 - normalizedDistance) * 1.2)
    const blue = 0.1

    return _nodeColor.setRGB(red, green, blue)
  }

  // --- ONTOLOGY MODE: cosmic hierarchy spectrum ---
  if (graphMode === 'ontology') {
    const nodeType = node.metadata?.type?.toLowerCase() || '';

    // Properties get warm pink
    if (nodeType === 'property' || nodeType === 'datatype_property' || nodeType === 'object_property') {
      return _nodeColor.copy(ONTOLOGY_PROPERTY_COLOR);
    }
    // Instances get white-blue
    if (nodeType === 'instance' || nodeType === 'individual') {
      return _nodeColor.copy(ONTOLOGY_INSTANCE_COLOR);
    }

    // Class nodes: color by hierarchy depth
    const hierarchyNode = hierarchyMap?.get(node.id);
    const depth = hierarchyNode?.depth ?? (node.metadata?.depth ?? 0);
    const depthIndex = Math.min(depth, ONTOLOGY_DEPTH_COLORS.length - 1);
    _nodeColor.copy(ONTOLOGY_DEPTH_COLORS[depthIndex]);

    // Emissive glow proportional to instanceCount
    const instanceCount = parseInt(node.metadata?.instanceCount || '0', 10);
    if (instanceCount > 0) {
      const glowFactor = Math.min(instanceCount / 50, 0.4);
      _nodeColor.offsetHSL(0, glowFactor * 0.2, glowFactor * 0.15);
    }

    return _nodeColor;
  }

  // --- AGENT MODE: status-based bioluminescence ---
  if (graphMode === 'agent') {
    const agentType = node.metadata?.agentType?.toLowerCase() || '';
    const agentStatus = node.metadata?.status?.toLowerCase() || 'active';

    // Queen and coordinator types override status color
    if (AGENT_TYPE_COLORS[agentType]) {
      return _nodeColor.copy(AGENT_TYPE_COLORS[agentType]);
    }

    // Status-based color
    const statusColor = AGENT_STATUS_COLORS[agentStatus] || AGENT_STATUS_COLORS['default'];
    return _nodeColor.copy(statusColor);
  }

  // --- KNOWLEDGE GRAPH MODE (default): enhanced with authority brightness ---
  const nodeType = node.metadata?.type || 'default'
  const precomputed = TYPE_THREE_COLORS[nodeType];
  if (precomputed) {
    _nodeColor.copy(precomputed);
  } else {
    _nodeColor.copy(TYPE_THREE_COLORS['default']);
  }

  // Authority-based brightness boost: higher authority = brighter, more saturated
  const authority = node.metadata?.authority ?? node.metadata?.authorityScore ?? 0;
  if (authority > 0) {
    const brightnessFactor = authority * 0.3;
    _nodeColor.offsetHSL(0, brightnessFactor * 0.2, brightnessFactor);
  }

  // Metallic tinting for crystal aesthetic on highly-connected nodes
  const connections = connectionCountMap?.get(node.id) || 0;
  if (connections > 5) {
    const metallicShift = Math.min(connections / 30, 0.15);
    _nodeColor.offsetHSL(-0.02 * metallicShift, 0.1 * metallicShift, 0.05 * metallicShift);
  }

  return _nodeColor;
}

// Node scaling delegated to shared computeNodeScale (../utils/nodeScaling.ts)
// Both GemNodes and this file use the same function to guarantee edge-node alignment.

interface GraphManagerProps {
  onDragStateChange?: (isDragging: boolean) => void;
}

const GraphManager: React.FC<GraphManagerProps> = ({ onDragStateChange }) => {

  // Narrow selectors: subscribe only to the sub-trees GraphManager actually reads.
  // This prevents full re-renders when unrelated settings (glow, sceneEffects, etc.) change,
  // which previously cascaded through the Three.js scene and caused visible position jumps.
  const logseqSettings = useSettingsStore(s => s.settings?.visualisation?.graphs?.logseq);
  const graphTypeVisuals = useSettingsStore(s => s.settings?.visualisation?.graphTypeVisuals);
  const glowIntensity = useSettingsStore(s => s.settings?.visualisation?.glow?.intensity ?? 0.3);
  const debugSettings = useSettingsStore(s => s.settings?.system?.debug);
  const nodeFilterSettings = useSettingsStore(s => s.settings?.nodeFilter);
  const nodeTypeVisibility = useSettingsStore(
    s => s.settings?.visualisation?.graphs?.logseq?.nodes?.nodeTypeVisibility
  );
  // Stable ref for the full settings object — updated every render but doesn't trigger re-renders.
  // Used only by child components that need the broad settings (GemNodes, event handlers).
  const settingsRef = useRef(useSettingsStore.getState().settings);
  useEffect(() => {
    const unsub = useSettingsStore.subscribe(state => { settingsRef.current = state.settings; });
    return unsub;
  }, []);
  // Convenience alias for reading in render (always current, but selector-gated re-renders)
  const settings = settingsRef.current;
  
  
  
  const ssspResult = useCurrentSSSPResult();
  const normalizeDistances = useAnalyticsStore(state => state.normalizeDistances);
  const [normalizedSSSPResult, setNormalizedSSSPResult] = useState<any>(null);
  const isXRMode = usePlatformStore((state) => state.isXRMode);
  const gemNodesRef = useRef<GemNodesHandle>(null)

  
  // Pre-allocated reusable objects to eliminate GC churn
  const tempPosition = useMemo(() => new THREE.Vector3(), [])
  const tempVec3 = useMemo(() => new THREE.Vector3(), [])
  const tempDirection = useMemo(() => new THREE.Vector3(), [])
  const tempSourceOffset = useMemo(() => new THREE.Vector3(), [])
  const tempTargetOffset = useMemo(() => new THREE.Vector3(), [])

  const [graphData, setGraphData] = useState<GraphData>({ nodes: [], edges: [] })

  // === Decomposed hooks: visual state + filtering ===
  const { perNodeVisualModeMap, hierarchyMap, connectionCountMap, dominantMode: graphMode } = useGraphVisualState(graphData);
  const { visibleNodes, nodeIdToIndexMap, expansionState } = useGraphFiltering(graphData, hierarchyMap, connectionCountMap);

  // Node-type visibility filtering: hide knowledge/ontology/agent nodes based on toggles
  const typeFilteredNodes = useMemo(() => {
    const vis = nodeTypeVisibility;
    if (!vis || (vis.knowledge && vis.ontology && vis.agent)) {
      return visibleNodes; // all visible, no filtering needed
    }
    return visibleNodes.filter(node => {
      const mode = perNodeVisualModeMap.get(String(node.id)) || graphMode;
      if (mode === 'knowledge_graph') return vis.knowledge !== false;
      if (mode === 'ontology') return vis.ontology !== false;
      if (mode === 'agent') return vis.agent !== false;
      return true;
    });
  }, [visibleNodes, perNodeVisualModeMap, graphMode, nodeTypeVisibility]);

  // Agent nodes overlay: polls /api/bots/agents for live agent telemetry
  const { agents: agentLayerNodes, connections: agentLayerConnections } = useAgentNodes();

  // Auto-adjust quality when FPS drops below qualityGates.minFpsThreshold
  useFpsMonitor();

  const nodePositionsRef = useRef<Float32Array | null>(null)

  // Layout mode transition state: LERP from startPositions → targetPositions with mass-aware easing
  const transitionRef = useRef<{
    active: boolean;
    startPositions: Float32Array;
    targetPositions: Float32Array;
    /** Dedicated output buffer — decoupled from SAB so worker writes cannot overwrite LERP results */
    lerpBuffer: Float32Array;
    progress: number;
    duration: number;
    startTime: number;
  } | null>(null);

  // Tracks the currently active layout mode for the HUD indicator
  const [activeLayoutMode, setActiveLayoutMode] = useState<string>('');
  const [layoutTransitioning, setLayoutTransitioning] = useState(false);
  // Ref copy of activeLayoutMode for use inside useFrame (avoids stale closure capture)
  const activeLayoutModeRef = useRef<string>('');

  // Auto-fit camera to bounding box of all nodes on first position data and on explicit request
  const { requestFit: requestCameraFit } = useCameraAutoFit(nodePositionsRef, graphData.nodes.length);

  const [edgePoints, setEdgePoints] = useState<number[]>([])
  const [highlightEdgePoints, setHighlightEdgePoints] = useState<number[]>([]);
  const edgeFlowRef = useRef<GlassEdgesHandle>(null);
  const highlightEdgeFlowRef = useRef<GlassEdgesHandle>(null);
  const prevLabelPositionsLengthRef = useRef<number>(0)
  const labelPositionsRef = useRef<Array<{x: number, y: number, z: number}>>([])
  const edgeUpdatePendingRef = useRef<number[] | null>(null)
  const highlightEdgeUpdatePendingRef = useRef<number[] | null>(null);
  // Pre-allocated buffers to eliminate per-frame array allocation GC pressure
  const edgeBufferRef = useRef<number[]>([]);
  const highlightBufferRef = useRef<number[]>([]);
  // Per-edge RGB color buffer for edge-type-based coloring (3 floats per edge)
  const edgeColorBufferRef = useRef<Float32Array>(new Float32Array(0));
  // Per-edge alpha buffer for edge-type-based opacity (1 float per edge)
  const edgeAlphaBufferRef = useRef<Float32Array>(new Float32Array(0));
  const edgeFrameCounterRef = useRef(0);
  const [nodesAreAtOrigin, setNodesAreAtOrigin] = useState(false)

  const [forceUpdate, setForceUpdate] = useState(0)
  const [labelUpdateTick, setLabelUpdateTick] = useState(0)
  const labelTickRef = useRef(0)

  // Frustum for label culling
  const frustum = useMemo(() => new THREE.Frustum(), [])
  const cameraViewProjectionMatrix = useMemo(() => new THREE.Matrix4(), [])

  const animationStateRef = useRef({
    time: 0,
    selectedNode: null as string | null,
    hoveredNode: null as string | null,
    pulsePhase: 0,
  })

  
  const [dragState, setDragState] = useState<{
    nodeId: string | null;
    instanceId: number | null;
  }>({ nodeId: null, instanceId: null })

  const dragDataRef = useRef({
    isDragging: false,
    pointerDown: false,
    nodeId: null as string | null,
    instanceId: null as number | null,
    startPointerPos: new THREE.Vector2(),
    startTime: 0,
    startNodePos3D: new THREE.Vector3(),
    currentNodePos3D: new THREE.Vector3(),
    lastUpdateTime: 0,
    pendingUpdate: null as BinaryNodeData | null,
  })

  const { camera, size } = useThree()

  
  // These now use the narrow selectors defined at the top of the component.
  // No extra variable needed for logseqSettings — it's already a top-level selector.
  const nodeSettings = logseqSettings?.nodes || settings?.visualisation?.nodes;

  
  useEffect(() => {
    if (ssspResult) {
      const normalized = normalizeDistances(ssspResult);
      setNormalizedSSSPResult({
        ...ssspResult,
        normalizedDistances: normalized
      });
    } else {
      setNormalizedSSSPResult(null);
    }
  }, [ssspResult, normalizeDistances]);

  // Start a layout mode transition: snapshot current positions as start, API positions as target
  const startLayoutTransition = useCallback((targetPositions: LayoutPosition[], durationMs: number) => {
    const positions = nodePositionsRef.current;
    const nodeCount = graphData.nodes.length;
    if (!positions || nodeCount === 0) {
      // Cannot start transition without current positions — cancel immediately
      setLayoutTransitioning(false);
      return;
    }

    const needed = nodeCount * 3;
    const startSnap = new Float32Array(needed);
    startSnap.set(positions.subarray(0, Math.min(needed, positions.length)));

    const targetSnap = new Float32Array(needed);
    // Build a lookup: id → index in targetPositions
    const idxById = new Map<number, number>();
    for (let i = 0; i < targetPositions.length; i++) {
      idxById.set(targetPositions[i].id, i);
    }
    for (let ni = 0; ni < nodeCount; ni++) {
      const node = graphData.nodes[ni];
      const numericId = parseInt(String(node.id), 10);
      const tp = idxById.get(numericId);
      if (tp !== undefined) {
        targetSnap[ni * 3]     = targetPositions[tp].x;
        targetSnap[ni * 3 + 1] = targetPositions[tp].y;
        targetSnap[ni * 3 + 2] = targetPositions[tp].z;
      } else {
        // No target for this node: keep current position as target (no movement)
        targetSnap[ni * 3]     = startSnap[ni * 3];
        targetSnap[ni * 3 + 1] = startSnap[ni * 3 + 1];
        targetSnap[ni * 3 + 2] = startSnap[ni * 3 + 2];
      }
    }

    // Allocate a dedicated output buffer so the SAB worker cannot overwrite LERP results
    const lerpBuf = new Float32Array(needed);
    lerpBuf.set(startSnap); // initialise to start positions so frame-0 is not garbage

    transitionRef.current = {
      active: true,
      startPositions: startSnap,
      targetPositions: targetSnap,
      lerpBuffer: lerpBuf,
      progress: 0,
      duration: durationMs,
      startTime: Date.now(),
    };
    setLayoutTransitioning(true);
  }, [graphData.nodes]);

  // (Color arrays, updateNodeColors, and mesh init removed -- GemNodes handles all node rendering)

  
  // Only forward settings to the worker when physics parameters actually change.
  // Non-physics settings (edge opacity, glow, hologram, etc.) are irrelevant to the worker
  // and sending them would cause unnecessary physics parameter resets that disrupt layout.
  const logseqPhysics = logseqSettings?.physics;
  const visionflowPhysics = useSettingsStore(s => s.settings?.visualisation?.graphs?.visionflow?.physics);
  const physicsFingerprint = useMemo(() => JSON.stringify({
    vf: visionflowPhysics,
    lq: logseqPhysics,
  }), [visionflowPhysics, logseqPhysics]);

  useEffect(() => {
    graphWorkerProxy.updateSettings(settingsRef.current);
  }, [physicsFingerprint]);

  // Subscribe to layoutMode changes and call the layout API when the value changes.
  // Mirrors the pattern used by settingsStore.subscribe for server-action callbacks.
  const layoutMode = useSettingsStore(s =>
    (s.settings as unknown as Record<string, Record<string, unknown>>)?.qualityGates?.layoutMode as string | undefined
  );
  const prevLayoutModeRef = useRef<string | undefined>(undefined);
  const mountedRef = useRef(false);
  useEffect(() => {
    if (!layoutMode || layoutMode === prevLayoutModeRef.current) return;
    prevLayoutModeRef.current = layoutMode;

    const TRANSITION_MS = 800;
    setActiveLayoutMode(layoutMode);
    activeLayoutModeRef.current = layoutMode;
    setLayoutTransitioning(true);

    const fire = () => {
      layoutApi.setMode(layoutMode, TRANSITION_MS).then(response => {
        const { data } = response;
        if (data.success && data.positions && data.positions.length > 0) {
          startLayoutTransition(data.positions, data.transitionMs ?? TRANSITION_MS);
        } else {
          nodePositionsRef.current = null;
          setLayoutTransitioning(false);
        }
      }).catch(err => {
        logger.warn('[GraphManager] layoutApi.setMode failed:', err);
        setLayoutTransitioning(false);
      });
    };

    if (!mountedRef.current) {
      mountedRef.current = true;
      setTimeout(fire, 2000);
    } else {
      fire();
    }
  }, [layoutMode, startLayoutTransition]);


  // Priority -2: run BEFORE child components (GemNodes, InstancedLabels) so
  // nodePositionsRef.current is populated before consumers read it this frame.
  // R3F executes lower priority numbers first.
  // First-frame hazard fix: force an R3F invalidate once positions become
  // non-null. Without this, some sessions settle the canvas before the SAB
  // has any data — subsequent useFrame runs read null positions and skip
  // matrix writes, leaving the user to trigger a window resize (which
  // invalidates the R3F root) just to see nodes appear.
  const positionsFirstSeenRef = useRef(false);
  const { invalidate } = useThree();
  useEffect(() => {
    if (positionsFirstSeenRef.current) return;
    let tries = 0;
    const maxTries = 120; // ~2s at 60Hz poll
    const id = window.setInterval(() => {
      tries++;
      const pos = graphWorkerProxy.getPositionsSync?.();
      if (pos && pos.length > 0) {
        // Scan for a non-zero to confirm the buffer is populated, not just allocated.
        let ok = false;
        for (let i = 0, n = Math.min(pos.length, 900); i < n; i++) {
          if (pos[i] !== 0) { ok = true; break; }
        }
        if (ok) {
          positionsFirstSeenRef.current = true;
          invalidate();
          window.clearInterval(id);
        }
      }
      if (tries >= maxTries) window.clearInterval(id);
    }, 16);
    return () => window.clearInterval(id);
  }, [invalidate]);

  useFrame((state, delta) => {
    animationStateRef.current.time = state.clock.elapsedTime

    // Camera fly-to animation (triggered by search/find commands)
    if (flyToTargetRef.current) {
      flyToProgressRef.current = Math.min(1, flyToProgressRef.current + delta * 2.0);
      const t = flyToProgressRef.current;
      // Smooth ease-out curve
      const eased = 1 - Math.pow(1 - t, 3);
      camera.position.lerp(flyToTargetRef.current, eased * 0.08);
      if (t >= 1) {
        flyToTargetRef.current = null;
      }
    }

    // Periodic label frustum refresh (~4 updates/sec at 60fps)
    labelTickRef.current++;
    if (labelTickRef.current >= 15) {
      labelTickRef.current = 0;
      cameraViewProjectionMatrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
      frustum.setFromProjectionMatrix(cameraViewProjectionMatrix);
      setLabelUpdateTick(prev => prev + 1);
    }

    // Position reading from SharedArrayBuffer (GemNodes reads from nodePositionsRef)
    if (graphData.nodes.length > 0) {
      graphWorkerProxy.requestTick(delta);
      const positions = graphWorkerProxy.getPositionsSync();
      if (!positions) return;

      // Detect pre-allocated but unpopulated SharedArrayBuffer (all zeros).
      // When this happens, set nodePositionsRef to null so GemNodes falls back
      // to node.position from React state (which has generated fallback positions).
      if (!nodePositionsRef.current) {
        // First frame: check if positions are all zero (SAB not yet populated)
        let hasNonZero = false;
        const checkLen = Math.min(graphData.nodes.length * 3, positions.length);
        for (let ci = 0; ci < checkLen; ci++) {
          if (positions[ci] !== 0) { hasNonZero = true; break; }
        }
        if (!hasNonZero && checkLen > 0) {
          // SAB is all zeros — don't set nodePositionsRef yet.
          // GemNodes will use node.position from React state.
          return;
        }
      }
      // === Layout mode transition: mass-aware LERP from start to target positions ===
      if (transitionRef.current?.active) {
        const t = transitionRef.current;
        const elapsed = Date.now() - t.startTime;
        const rawProgress = Math.min(elapsed / t.duration, 1.0);

        // Safety timeout: if transition exceeds 2x duration, force-complete it.
        // Prevents permanently blocking SAB position updates if something goes wrong.
        if (elapsed > t.duration * 2) {
          transitionRef.current.active = false;
          setLayoutTransitioning(false);
          nodePositionsRef.current = positions;
        } else {
          // Ease in-out (smoothstep)
          const progress = rawProgress < 0.5
            ? 2 * rawProgress * rawProgress
            : 1 - Math.pow(-2 * rawProgress + 2, 2) / 2;

          const nodeCount = graphData.nodes.length;
          // Write LERP results into the dedicated lerpBuffer (not the SAB view).
          // This prevents the worker from overwriting interpolated positions between frames.
          const lerp = t.lerpBuffer;
          for (let i = 0; i < nodeCount; i++) {
            const idx = i * 3;
            if (idx + 2 >= lerp.length) break;
            // Mass factor: high-degree nodes move slower (more inertia)
            const connectionCount = connectionCountMap.get(String(graphData.nodes[i]?.id)) || 0;
            const massFactor = 1.0 / (1.0 + Math.sqrt(connectionCount) * 0.3);
            const nodeProgress = Math.min(progress / massFactor, 1.0);

            lerp[idx]     = t.startPositions[idx]     + (t.targetPositions[idx]     - t.startPositions[idx])     * nodeProgress;
            lerp[idx + 1] = t.startPositions[idx + 1] + (t.targetPositions[idx + 1] - t.startPositions[idx + 1]) * nodeProgress;
            lerp[idx + 2] = t.startPositions[idx + 2] + (t.targetPositions[idx + 2] - t.startPositions[idx + 2]) * nodeProgress;
          }
          // Point nodePositionsRef at the lerpBuffer so all consumers (GemNodes, edges) see
          // transition positions this frame instead of the stale/overwritten SAB data.
          nodePositionsRef.current = lerp;

          if (rawProgress >= 1.0) {
            transitionRef.current.active = false;
            setLayoutTransitioning(false);
            const isForceDirected = !activeLayoutModeRef.current || activeLayoutModeRef.current === 'forceDirected';
            if (isForceDirected) {
              // ForceDirected: hand back to SAB so the physics worker drives positions
              nodePositionsRef.current = positions;
            }
            // Non-forceDirected: keep nodePositionsRef pointing at lerp (final layout positions).
            // Physics is paused server-side, so the SAB worker is not updating positions.
          }
        }
      } else {
        // No active transition: SAB/tick positions flow directly to renderer
        nodePositionsRef.current = positions;
      }
      // === End layout mode transition ===

      // Auto-fit camera on first real position data (one-shot, non-continuous)
      requestCameraFit();

      const positionsValid = positions && positions.length > 0 && positions.length >= graphData.nodes.length * 3;
      if (positions && positions.length > 0 && !positionsValid) {
        logger.warn(`Positions array too short: ${positions.length} < ${graphData.nodes.length * 3} (${graphData.nodes.length} nodes). Skipping position-dependent rendering this frame.`);
      }

      if (positionsValid) {
        // Throttle edge computation to every 2nd frame (halves per-frame cost
        // from ~15ms to ~7.5ms on 11K edges). Cached edge buffer stays valid
        // between updates — edges lag node positions by at most 1 frame at 60fps.
        edgeFrameCounterRef.current++;
        const shouldComputeEdges = edgeFrameCounterRef.current % 2 === 0
          || edgeFrameCounterRef.current <= 2; // always compute first 2 frames

        if (shouldComputeEdges) {
        // Edge point computation (GlassEdges needs edgePoints)
        // Reuse pre-allocated buffer -- only grow when needed (never shrinks to avoid churn)
        const edgeCount = graphData.edges.length;
        const edgeBufferNeeded = edgeCount * 6;
        if (edgeBufferRef.current.length < edgeBufferNeeded) {
          edgeBufferRef.current = new Array<number>(edgeBufferNeeded);
        }
        const newEdgePoints = edgeBufferRef.current;
        let edgePointIdx = 0;

        // Per-edge color buffer: 3 floats (RGB) per edge, grows as needed
        const edgeColorNeeded = edgeCount * 3;
        if (edgeColorBufferRef.current.length < edgeColorNeeded) {
          edgeColorBufferRef.current = new Float32Array(edgeColorNeeded);
        }
        const edgeColors = edgeColorBufferRef.current;
        let edgeColorIdx = 0;

        // Per-edge alpha buffer: 1 float per edge
        if (edgeAlphaBufferRef.current.length < edgeCount) {
          edgeAlphaBufferRef.current = new Float32Array(edgeCount);
        }
        const edgeAlphas = edgeAlphaBufferRef.current;
        let edgeAlphaIdx = 0;

        // Cache drag state outside the loop (hot path — avoid ref access per edge)
        const isDragging = dragDataRef.current.isDragging;
        const dragNodeId = isDragging ? dragDataRef.current.nodeId : null;
        const dragPos = dragDataRef.current.currentNodePos3D;

        // Visual surface radius = getNodeScale() * baseScale * GEO_RADIUS
        // baseScale = (nodeSize / 0.5), GEO_RADIUS = 0.5
        // Combined: getNodeScale() * nodeSize  (the 0.5's cancel)
        const nodeSize = nodeSettings?.nodeSize ?? 0.5;

        graphData.edges.forEach(edge => {
          const sourceStr = String(edge.source);
          const targetStr = String(edge.target);
          const sourceNodeIndex = nodeIdToIndexMap.get(sourceStr);
          const targetNodeIndex = nodeIdToIndexMap.get(targetStr);

          if (sourceNodeIndex !== undefined && targetNodeIndex !== undefined) {
            // Skip edges connected to hidden node types
            if (nodeTypeVisibility) {
              const srcVis = perNodeVisualModeMap.get(sourceStr) || graphMode;
              const tgtVis = perNodeVisualModeMap.get(targetStr) || graphMode;
              if ((srcVis === 'knowledge_graph' && !nodeTypeVisibility.knowledge) ||
                  (srcVis === 'ontology' && !nodeTypeVisibility.ontology) ||
                  (srcVis === 'agent' && !nodeTypeVisibility.agent) ||
                  (tgtVis === 'knowledge_graph' && !nodeTypeVisibility.knowledge) ||
                  (tgtVis === 'ontology' && !nodeTypeVisibility.ontology) ||
                  (tgtVis === 'agent' && !nodeTypeVisibility.agent)) return;
            }
            const i3s = sourceNodeIndex * 3;
            const i3t = targetNodeIndex * 3;

            if (i3s + 2 >= positions.length || i3t + 2 >= positions.length) return;

            // Read positions — override with live drag pos BEFORE computing direction
            if (dragNodeId === sourceStr) {
              tempVec3.set(dragPos.x, dragPos.y, dragPos.z);
            } else {
              tempVec3.set(positions[i3s], positions[i3s + 1], positions[i3s + 2]);
            }

            if (dragNodeId === targetStr) {
              tempPosition.set(dragPos.x, dragPos.y, dragPos.z);
            } else {
              tempPosition.set(positions[i3t], positions[i3t + 1], positions[i3t + 2]);
            }

            tempDirection.subVectors(tempPosition, tempVec3);
            const edgeLength = tempDirection.length();

            if (edgeLength > 0.001) {
              tempDirection.normalize();

              const sourceNode = graphData.nodes[sourceNodeIndex];
              const targetNode = graphData.nodes[targetNodeIndex];
              const sourceVisualMode = perNodeVisualModeMap.get(sourceStr) || graphMode;
              const targetVisualMode = perNodeVisualModeMap.get(targetStr) || graphMode;

              // Must match GemNodes: visual surface = getNodeScale * baseScale * 0.5
              // = getNodeScale * (nodeSize/0.5) * 0.5 = getNodeScale * nodeSize
              const sourceRadius = computeNodeScale(sourceNode, connectionCountMap, sourceVisualMode, hierarchyMap, graphTypeVisuals) * nodeSize;
              const targetRadius = computeNodeScale(targetNode, connectionCountMap, targetVisualMode, hierarchyMap, graphTypeVisuals) * nodeSize;

              tempSourceOffset.copy(tempVec3).addScaledVector(tempDirection, sourceRadius);
              tempTargetOffset.copy(tempPosition).addScaledVector(tempDirection, -targetRadius);

              if (tempSourceOffset.distanceTo(tempTargetOffset) > 0.1) {
                newEdgePoints[edgePointIdx++] = tempSourceOffset.x;
                newEdgePoints[edgePointIdx++] = tempSourceOffset.y;
                newEdgePoints[edgePointIdx++] = tempSourceOffset.z;
                newEdgePoints[edgePointIdx++] = tempTargetOffset.x;
                newEdgePoints[edgePointIdx++] = tempTargetOffset.y;
                newEdgePoints[edgePointIdx++] = tempTargetOffset.z;

                // Write per-edge color and alpha from edge type
                const eColor = getEdgeTypeColor(edge.edgeType);
                edgeColors[edgeColorIdx++] = eColor.r;
                edgeColors[edgeColorIdx++] = eColor.g;
                edgeColors[edgeColorIdx++] = eColor.b;
                edgeAlphas[edgeAlphaIdx++] = getEdgeTypeAlpha(edge.edgeType);
              }
            }
          }
        });
        // Compute highlighted edges for the selected node
        if (selectedNodeId) {
          // Reuse pre-allocated highlight buffer -- only grow when needed
          const hlBufferNeeded = edgeCount * 6;
          if (highlightBufferRef.current.length < hlBufferNeeded) {
            highlightBufferRef.current = new Array<number>(hlBufferNeeded);
          }
          const highlightBuf = highlightBufferRef.current;
          let hlIdx = 0;
          graphData.edges.forEach((edge: any) => {
            const sourceStr = String(edge.source);
            const targetStr = String(edge.target);
            if (sourceStr !== selectedNodeId && targetStr !== selectedNodeId) return;

            // Skip highlight edges connected to hidden node types
            if (nodeTypeVisibility) {
              const srcVis = perNodeVisualModeMap.get(sourceStr) || graphMode;
              const tgtVis = perNodeVisualModeMap.get(targetStr) || graphMode;
              if ((srcVis === 'knowledge_graph' && !nodeTypeVisibility.knowledge) ||
                  (srcVis === 'ontology' && !nodeTypeVisibility.ontology) ||
                  (srcVis === 'agent' && !nodeTypeVisibility.agent) ||
                  (tgtVis === 'knowledge_graph' && !nodeTypeVisibility.knowledge) ||
                  (tgtVis === 'ontology' && !nodeTypeVisibility.ontology) ||
                  (tgtVis === 'agent' && !nodeTypeVisibility.agent)) return;
            }

            const sourceIdx = nodeIdToIndexMap.get(sourceStr);
            const targetIdx = nodeIdToIndexMap.get(targetStr);
            if (sourceIdx === undefined || targetIdx === undefined) return;

            const si3 = sourceIdx * 3;
            const ti3 = targetIdx * 3;
            if (si3 + 2 >= positions.length || ti3 + 2 >= positions.length) return;

            // Override with drag pos BEFORE direction computation
            if (dragNodeId === sourceStr) {
              tempVec3.set(dragPos.x, dragPos.y, dragPos.z);
            } else {
              tempVec3.set(positions[si3], positions[si3 + 1], positions[si3 + 2]);
            }
            if (dragNodeId === targetStr) {
              tempPosition.set(dragPos.x, dragPos.y, dragPos.z);
            } else {
              tempPosition.set(positions[ti3], positions[ti3 + 1], positions[ti3 + 2]);
            }
            tempDirection.subVectors(tempPosition, tempVec3);
            const len = tempDirection.length();
            if (len > 0.001) {
              tempDirection.normalize();
              const srcNode = graphData.nodes[sourceIdx];
              const tgtNode = graphData.nodes[targetIdx];
              const srcMode = perNodeVisualModeMap.get(sourceStr) || graphMode;
              const tgtMode = perNodeVisualModeMap.get(targetStr) || graphMode;
              const srcR = computeNodeScale(srcNode, connectionCountMap, srcMode, hierarchyMap, graphTypeVisuals) * nodeSize;
              const tgtR = computeNodeScale(tgtNode, connectionCountMap, tgtMode, hierarchyMap, graphTypeVisuals) * nodeSize;

              tempSourceOffset.copy(tempVec3).addScaledVector(tempDirection, srcR);
              tempTargetOffset.copy(tempPosition).addScaledVector(tempDirection, -tgtR);
              if (tempSourceOffset.distanceTo(tempTargetOffset) > 0.2) {
                highlightBuf[hlIdx++] = tempSourceOffset.x;
                highlightBuf[hlIdx++] = tempSourceOffset.y;
                highlightBuf[hlIdx++] = tempSourceOffset.z;
                highlightBuf[hlIdx++] = tempTargetOffset.x;
                highlightBuf[hlIdx++] = tempTargetOffset.y;
                highlightBuf[hlIdx++] = tempTargetOffset.z;
              }
            }
          });
          if (highlightEdgeFlowRef.current) {
            highlightEdgeFlowRef.current.updatePoints(highlightBuf, hlIdx);
          } else {
            highlightEdgeUpdatePendingRef.current = highlightBuf.slice(0, hlIdx);
          }
        } else if (highlightEdgePoints.length > 0) {
          if (highlightEdgeFlowRef.current) {
            highlightEdgeFlowRef.current.updatePoints([]);
          } else {
            highlightEdgeUpdatePendingRef.current = [];
          }
        }

        // Imperative edge update: push buffer + count directly (no per-frame slice allocation)
        if (edgeFlowRef.current) {
          edgeFlowRef.current.updatePoints(newEdgePoints, edgePointIdx);
          // Push per-edge-type colors (edgeColorIdx / 3 = number of edges with color data)
          const edgeCountWithColor = edgeColorIdx / 3;
          if (edgeCountWithColor > 0) {
            edgeFlowRef.current.updateColors(edgeColors, edgeCountWithColor);
            edgeFlowRef.current.updateAlphas(edgeAlphas, edgeAlphaIdx);
          }
        } else {
          edgeUpdatePendingRef.current = newEdgePoints.slice(0, edgePointIdx);
        }

        // One-time diagnostic for edge/position pipeline
        if (!(window as unknown as Record<string, boolean>).__gmDiagV2) {
          (window as unknown as Record<string, boolean>).__gmDiagV2 = true;
          // Sample first 3 nodes' positions to check if they're real or all-zero
          const samples: Array<{i: number, x: number, y: number, z: number}> = [];
          for (let si = 0; si < Math.min(3, graphData.nodes.length); si++) {
            const si3 = si * 3;
            samples.push({ i: si, x: positions[si3], y: positions[si3+1], z: positions[si3+2] });
          }
          // Check how many positions are non-zero
          let nonZeroCount = 0;
          for (let si = 0; si < graphData.nodes.length; si++) {
            const si3 = si * 3;
            if (Math.abs(positions[si3]) > 0.01 || Math.abs(positions[si3+1]) > 0.01 || Math.abs(positions[si3+2]) > 0.01) {
              nonZeroCount++;
            }
          }
          // Sample a few edges to see source/target lookup
          const edgeSamples: any[] = [];
          for (let ei = 0; ei < Math.min(3, graphData.edges.length); ei++) {
            const edge = graphData.edges[ei];
            const srcIdx = nodeIdToIndexMap.get(String(edge.source));
            const tgtIdx = nodeIdToIndexMap.get(String(edge.target));
            edgeSamples.push({
              src: edge.source, tgt: edge.target,
              srcIdx, tgtIdx,
              srcFound: srcIdx !== undefined, tgtFound: tgtIdx !== undefined,
            });
          }
          logger.debug('[GraphManager] DIAG first frame:', {
            nodeCount: graphData.nodes.length,
            edgeCount: graphData.edges.length,
            positionsLength: positions?.length ?? 0,
            positionsValid,
            edgePointsComputed: edgePointIdx / 6,
            nonZeroPositions: nonZeroCount,
            samplePositions: samples,
            sampleEdges: edgeSamples,
            hasEdgeFlowRef: !!edgeFlowRef.current,
            visibleNodesCount: visibleNodes.length,
          });
        }
        } // end shouldComputeEdges throttle

        // Update label positions ref every frame (fast, no re-render)
        const labelCount = graphData.nodes.length;
        let currentLabelArr = labelPositionsRef.current;
        if (currentLabelArr.length !== labelCount) {
          currentLabelArr = new Array(labelCount);
          for (let i = 0; i < labelCount; i++) {
            currentLabelArr[i] = { x: 0, y: 0, z: 0 };
          }
        }
        for (let i = 0; i < labelCount; i++) {
          const i3 = i * 3;
          currentLabelArr[i].x = positions[i3];
          currentLabelArr[i].y = positions[i3 + 1];
          currentLabelArr[i].z = positions[i3 + 2];
        }
        labelPositionsRef.current = currentLabelArr;

        prevLabelPositionsLengthRef.current = labelCount;
      }
    }

    // Process pending state updates -- only for initial mount before imperative handles are available
    if (edgeUpdatePendingRef.current && !edgeFlowRef.current) {
      const pendingEdges = edgeUpdatePendingRef.current;
      edgeUpdatePendingRef.current = null;
      setEdgePoints(pendingEdges);
    }
    if (highlightEdgeUpdatePendingRef.current !== null && !highlightEdgeFlowRef.current) {
      const pendingHighlight = highlightEdgeUpdatePendingRef.current;
      highlightEdgeUpdatePendingRef.current = null;
      setHighlightEdgePoints(pendingHighlight);
    }
  }, -2)


  useEffect(() => {

    const handleGraphUpdate = (data: GraphData): GraphData | undefined => {

      const debugSettings = settings?.system?.debug;
      if (debugSettings?.enableNodeDebug) {
        logger.debug('Graph data updated', {
          nodeCount: data.nodes.length,
          edgeCount: data.edges.length,
          firstNode: data.nodes.length > 0 ? data.nodes[0] : null,
          hasValidData: data && Array.isArray(data.nodes) && Array.isArray(data.edges)
        });
      }

      if (debugState.isEnabled()) {
        logger.info('Graph data updated', {
          nodeCount: data.nodes.length,
          edgeCount: data.edges.length,
          firstNode: data.nodes.length > 0 ? data.nodes[0] : null
        })
      }


      if (!data || !Array.isArray(data.nodes) || !Array.isArray(data.edges)) {
        return undefined;
      }

      
      const dataWithPositions = {
        ...data,
        nodes: data.nodes.map((node, i) => {
          // Normalize node ID to string (server may return numeric IDs)
          const normalizedNode = typeof node.id !== 'string' ? { ...node, id: String(node.id) } : node;
          if (!normalizedNode.position || (normalizedNode.position.x === 0 && normalizedNode.position.y === 0 && normalizedNode.position.z === 0)) {
            const position = getPositionForNode(normalizedNode, i, data.nodes.length)
            return {
              ...normalizedNode,
              position: { x: position[0], y: position[1], z: position[2] }
            }
          }
          return normalizedNode
        }),
        edges: data.edges.map((edge: any, idx: number) => {
          // Robust source/target extraction: handle multiple API field naming conventions
          let src = edge.source ?? edge.from ?? edge.from_node ?? edge.sourceId ?? edge.source_id ?? edge.start;
          let tgt = edge.target ?? edge.to ?? edge.to_node ?? edge.targetId ?? edge.target_id ?? edge.end;

          // Recover from pre-broken string "undefined" / "null" (graphDataManager may have
          // already String()-coerced an undefined value before this code runs)
          if (src === 'undefined' || src === 'null' || src === '') src = undefined;
          if (tgt === 'undefined' || tgt === 'null' || tgt === '') tgt = undefined;

          // Fallback: extract from edge ID format "source-target" (e.g. "798-861")
          if ((src == null || tgt == null) && edge.id && typeof edge.id === 'string') {
            const parts = edge.id.split('-');
            if (parts.length >= 2) {
              if (src == null) src = parts[0];
              if (tgt == null) tgt = parts.slice(1).join('-');
            }
          }

          // One-time diagnostic (v2: use fresh flag so HMR shows latest logic)
          if (idx === 0 && !(window as unknown as Record<string, boolean>).__edgeRecoveryDiag) {
            (window as unknown as Record<string, boolean>).__edgeRecoveryDiag = true;
            logger.debug('[GraphManager] edge[0] RECOVERY: src=', src, 'tgt=', tgt,
              'raw.source=', edge.source, 'raw.target=', edge.target,
              'id=', edge.id, 'keys=', Object.keys(edge));
          }

          return {
            ...edge,
            source: String(src),
            target: String(tgt),
          };
        }).filter((e: { source: string; target: string }) => e.source !== 'undefined' && e.target !== 'undefined' && e.source !== 'null' && e.target !== 'null')
      }

      // One-time edge pipeline diagnostic
      if (!(window as unknown as Record<string, boolean>).__edgePipelineV2) {
        (window as unknown as Record<string, boolean>).__edgePipelineV2 = true;
        logger.debug('[GraphManager] handleGraphUpdate edge pipeline:',
          'inputEdges=', data.edges.length,
          'outputEdges=', dataWithPositions.edges.length,
          'nodes=', dataWithPositions.nodes.length,
          dataWithPositions.edges.length > 0
            ? { first: { src: dataWithPositions.edges[0].source, tgt: dataWithPositions.edges[0].target, id: dataWithPositions.edges[0].id } }
            : '(no edges survived filter)');
      }

      const allAtOrigin = dataWithPositions.nodes.every(node =>
        !node.position || (node.position.x === 0 && node.position.y === 0 && node.position.z === 0)
      )
      setNodesAreAtOrigin(allAtOrigin)

      setGraphData(dataWithPositions)

      
      // Use dataWithPositions.nodes (which have generated positions) for initial edge computation
      // String() coercion ensures matching even when server returns numeric node IDs
      const posNodeMap = new Map(dataWithPositions.nodes.map(n => [String(n.id), n]))
      const newEdgePoints: number[] = []
      dataWithPositions.edges.forEach((edge) => {
        const sourceNode = posNodeMap.get(String(edge.source))
        const targetNode = posNodeMap.get(String(edge.target))

        if (sourceNode?.position && targetNode?.position) {
          newEdgePoints.push(
            sourceNode.position.x, sourceNode.position.y, sourceNode.position.z,
            targetNode.position.x, targetNode.position.y, targetNode.position.z
          )
        }
      })

      setEdgePoints(newEdgePoints)

      return dataWithPositions
    }

    const unsubscribe = graphDataManager.onGraphDataChange((data) => {
      // Process data locally only — do NOT send back to graphWorkerProxy.setGraphData()
      // as that triggers notifyGraphDataListeners → this callback → infinite loop.
      // The worker already has the data from graphDataManager.fetchInitialData().
      handleGraphUpdate(data)
    })


    graphDataManager.getGraphData().then((data) => {

      const debugSettings = settings?.system?.debug;
      if (debugSettings?.enableNodeDebug) {
        logger.debug('Initial graph data loaded', {
          nodeCount: data.nodes.length,
          edgeCount: data.edges.length
        });
      }
      handleGraphUpdate(data)
    }).then(() => {
    }).catch((error) => {

      const fallbackData = {
        nodes: [
          { id: 'fallback1', label: 'Test Node 1', position: { x: -5, y: 0, z: 0 } },
          { id: 'fallback2', label: 'Test Node 2', position: { x: 5, y: 0, z: 0 } },
          { id: 'fallback3', label: 'Test Node 3', position: { x: 0, y: 5, z: 0 } }
        ],
        edges: [
          { id: 'fallback_edge1', source: 'fallback1', target: 'fallback2' },
          { id: 'fallback_edge2', source: 'fallback2', target: 'fallback3' }
        ]
      };
      handleGraphUpdate(fallbackData);
    })

    return () => {
      unsubscribe()
    }
  }, [])


  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);

  // ADR-059 §3 Phase 3 — emit user_interaction events to agentbox so
  // agents can observe user attention. Transient (not persisted).
  useSelectionAndFocusEvents(selectedNodeId);

  // Camera fly-to animation state
  const flyToTargetRef = useRef<THREE.Vector3 | null>(null);
  const flyToProgressRef = useRef(0);

  // Dispatch node-selected event to NodeDetailPanel when selection changes
  useEffect(() => {
    if (!selectedNodeId) {
      window.dispatchEvent(new CustomEvent('visionflow:node-selected', { detail: null }));
      return;
    }
    const node = graphData.nodes.find(n => String(n.id) === selectedNodeId);
    if (!node) return;

    // Collect neighbor info
    const neighborIds = new Set<string>();
    graphData.edges.forEach(edge => {
      const src = String(edge.source);
      const tgt = String(edge.target);
      if (src === selectedNodeId) neighborIds.add(tgt);
      if (tgt === selectedNodeId) neighborIds.add(src);
    });
    const neighbors = Array.from(neighborIds).map(nid => {
      const n = graphData.nodes.find(nd => String(nd.id) === nid);
      return { id: nid, label: n?.label || nid };
    });

    window.dispatchEvent(new CustomEvent('visionflow:node-selected', {
      detail: {
        nodeId: selectedNodeId,
        label: node.label,
        metadata: node.metadata || {},
        connectionCount: connectionCountMap.get(selectedNodeId) || neighborIds.size,
        neighbors,
      },
    }));
  }, [selectedNodeId, graphData.nodes, graphData.edges, connectionCountMap]);

  // Listen for search events and node-deselect
  useEffect(() => {
    const handleSearch = (event: Event) => {
      const { query, nodeId } = (event as CustomEvent).detail || {};

      let targetNode: KGNode | undefined;

      // Direct node ID navigation (from neighbor click)
      if (nodeId) {
        targetNode = graphData.nodes.find(n => String(n.id) === nodeId);
      }

      // Fuzzy label search
      if (!targetNode && query) {
        const lowerQuery = query.toLowerCase();
        // Exact prefix match first
        targetNode = graphData.nodes.find(n =>
          n.label.toLowerCase().startsWith(lowerQuery)
        );
        // Then substring match
        if (!targetNode) {
          targetNode = graphData.nodes.find(n =>
            n.label.toLowerCase().includes(lowerQuery)
          );
        }
        // Then fuzzy: split query into words, match nodes containing all words
        if (!targetNode && lowerQuery.includes(' ')) {
          const words = lowerQuery.split(/\s+/).filter((w: string) => w.length > 1);
          targetNode = graphData.nodes.find(n => {
            const label = n.label.toLowerCase();
            return words.every((w: string) => label.includes(w));
          });
        }
      }

      if (!targetNode) return;

      // Select the node
      setSelectedNodeId(String(targetNode.id));

      // Get position for camera fly-to
      const idx = nodeIdToIndexMap.get(String(targetNode.id));
      const positions = nodePositionsRef.current;
      let targetPos: THREE.Vector3 | null = null;

      if (idx !== undefined && positions && idx * 3 + 2 < positions.length) {
        targetPos = new THREE.Vector3(
          positions[idx * 3],
          positions[idx * 3 + 1],
          positions[idx * 3 + 2]
        );
      } else if (targetNode.position) {
        targetPos = new THREE.Vector3(
          targetNode.position.x,
          targetNode.position.y,
          targetNode.position.z
        );
      }

      if (targetPos) {
        // Set fly-to target offset from node (camera approaches from current direction)
        const offset = new THREE.Vector3().subVectors(camera.position, targetPos).normalize().multiplyScalar(25);
        flyToTargetRef.current = targetPos.clone().add(offset);
        flyToProgressRef.current = 0;
      }
    };

    const handleDeselect = () => {
      setSelectedNodeId(null);
    };

    window.addEventListener('visionflow:search', handleSearch);
    window.addEventListener('visionflow:node-deselect', handleDeselect);
    return () => {
      window.removeEventListener('visionflow:search', handleSearch);
      window.removeEventListener('visionflow:node-deselect', handleDeselect);
    };
  }, [graphData.nodes, nodeIdToIndexMap, camera]);

  // Proxy ref: useGraphEventHandlers expects RefObject<InstancedMesh>.
  // GemNodes manages its own mesh internally, so we bridge via a getter-backed ref.
  const meshProxyRef = useMemo(() => ({
    get current() { return gemNodesRef.current?.getMesh() ?? null; },
    set current(_v) { /* no-op: GemNodes owns the mesh */ },
  }), []) as React.RefObject<THREE.InstancedMesh>;

  const { handlePointerDown, handlePointerMove, handlePointerUp } = useGraphEventHandlers(
    meshProxyRef,
    dragDataRef,
    setDragState,
    graphData,
    typeFilteredNodes,
    camera,
    size,
    settings,
    setGraphData,
    onDragStateChange,
    setSelectedNodeId
  )

  // Particle geometry removed - was always null (dead code)

  // Label positions are read directly from labelPositionsRef.current (updated every frame in useFrame).
  // No React state needed -- eliminates per-frame re-renders.

  
  // Default edge settings - opacity increased to 0.6 for bloom visibility
  // Bloom threshold is typically 0.15, so edges need opacity > 0.3 to remain visible
  const defaultEdgeSettings: EdgeSettings = {
    arrowSize: 0.5,
    baseWidth: 0.2,
    color: '#FF5722',
    enableArrows: true,
    opacity: 0.6, // Increased from 0.2 to ensure visibility above bloom threshold
    widthRange: [0.1, 0.3],
    quality: 'medium',
    enableFlowEffect: false,
    flowSpeed: 1,
    flowIntensity: 1,
    glowStrength: 1,
    distanceIntensity: 0.5,
    useGradient: false,
    gradientColors: ['#ff0000', '#0000ff'],
  };

  // nodeIdToIndex removed -- use nodeIdToIndexMap (line ~517) which computes the same Map

  // OLD NodeLabels useMemo removed — replaced by <InstancedLabels> component which
  // performs its own frustum culling and layout inside useFrame (zero React re-renders).
  // labelPositionsRef, labelTickRef, and labelUpdateTick are kept as InstancedLabels
  // reads labelPositionsRef as a fallback when SAB positions are unavailable.

  
  useEffect(() => {
    const debugSettings = settings?.system?.debug;
    if (debugSettings?.enableNodeDebug) {
      logger.debug('Component mounted', {
        nodeCount: graphData.nodes.length,
        edgeCount: graphData.edges.length,
        edgePointsLength: edgePoints.length,
        gemNodesRef: !!gemNodesRef.current,
      });
    }

    return () => {
      if (debugSettings?.enableNodeDebug) {
        logger.debug('Component unmounting');
      }
    };
  }, []);

  return (
    <>
      {/* MetadataShapes: shape-based metadata visualization (opt-in via enableMetadataShape) */}
      {(logseqSettings?.nodes?.enableMetadataShape || settings?.visualisation?.nodes?.enableMetadataShape) && (
        <MetadataShapes
          nodes={typeFilteredNodes}
          nodePositions={nodePositionsRef.current}
          settings={settings}
          ssspResult={normalizedSSSPResult}
          graphMode={graphMode}
          hierarchyMap={hierarchyMap}
        />
      )}

      {/* Gem node rendering */}
      <GemNodes
        ref={gemNodesRef}
        nodes={typeFilteredNodes}
        edges={graphData.edges}
        graphMode={graphMode}
        perNodeVisualModeMap={perNodeVisualModeMap}
        nodePositionsRef={nodePositionsRef}
        connectionCountMap={connectionCountMap}
        hierarchyMap={hierarchyMap}
        nodeIdToIndexMap={nodeIdToIndexMap}
        settings={settings}
        ssspResult={normalizedSSSPResult}
        dragDataRef={dragDataRef}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={(event: any) => handlePointerUp(event)}
        onPointerMissed={() => {
          if (dragDataRef.current.pointerDown) {
            handlePointerUp();
          }
          setSelectedNodeId(null);
        }}
        onDoubleClick={(event: ThreeEvent<MouseEvent>) => {
          if (event.instanceId !== undefined && event.instanceId < typeFilteredNodes.length) {
            const node = typeFilteredNodes[event.instanceId];
            if (node) {
              const nodeId = String(node.id);
              const hierarchyNode = hierarchyMap.get(nodeId);

              // Hierarchy parent: double-click toggles collapse/expand.
              // Takes priority over URL navigation so ontology tree works.
              if (hierarchyNode && hierarchyNode.childIds.length > 0) {
                expansionState.toggleExpansion(nodeId);
                return;
              }

              // KG leaf node: open source page if available
              const pageUrl = node.metadata?.page_url || node.metadata?.pageUrl || node.metadata?.url;
              if (pageUrl) {
                window.open(pageUrl, '_blank', 'noopener,noreferrer');
                return;
              }
              const filePath = node.metadata?.file_path || node.metadata?.filePath || node.metadata?.path;
              if (filePath) {
                window.open(`https://narrativegoldmine.com/#/page/${encodeURIComponent(filePath)}`, '_blank', 'noopener,noreferrer');
                return;
              }
              if (node.label) {
                window.open(`https://narrativegoldmine.com/#/page/${encodeURIComponent(node.label)}`, '_blank', 'noopener,noreferrer');
              }
            }
          }
        }}
        selectedNodeId={selectedNodeId}
      />

      {/* Collapsed-group halo rings — pulsing indicator on parent nodes with hidden children */}
      <CollapsedGroupRings
        nodes={typeFilteredNodes}
        hierarchyMap={hierarchyMap}
        expansionState={expansionState}
        nodePositionsRef={nodePositionsRef}
        nodeIdToIndexMap={nodeIdToIndexMap}
      />

      {/* Glass edge rendering */}
      <GlassEdges
        ref={edgeFlowRef}
        points={edgePoints}
        settings={settings?.visualisation?.graphs?.logseq?.edges || settings?.visualisation?.edges || defaultEdgeSettings}
        colorOverride={
          graphMode === 'knowledge_graph'
            ? settings?.visualisation?.graphTypeVisuals?.knowledgeGraph?.edgeColor
            : graphMode === 'ontology'
            ? settings?.visualisation?.graphTypeVisuals?.ontology?.edgeColor
            : undefined
        }
      />

      {/* Highlighted edges for selected node */}
      <GlassEdges
        ref={highlightEdgeFlowRef}
        points={highlightEdgePoints}
        settings={settings?.visualisation?.graphs?.logseq?.edges || settings?.visualisation?.edges || defaultEdgeSettings}
        colorOverride={settings?.visualisation?.interaction?.selectionHighlightColor || '#00FFFF'}
      />

      {/* Knowledge graph rotating rings */}
      <KnowledgeRings
        nodes={graphData.nodes}
        perNodeVisualModeMap={perNodeVisualModeMap}
        nodePositionsRef={nodePositionsRef}
        nodeIdToIndexMap={nodeIdToIndexMap}
        connectionCountMap={connectionCountMap}
        edges={graphData.edges}
        hierarchyMap={hierarchyMap}
        settings={settings}
      />

      {/* Cluster hull visualization */}
      <ClusterHulls
        nodes={graphData.nodes}
        nodePositionsRef={nodePositionsRef}
        nodeIdToIndexMap={nodeIdToIndexMap}
        settings={settings}
      />

      {/* Agent nodes overlay: bioluminescent agent visualization from Management API */}
      {agentLayerNodes.length > 0 && (
        <AgentNodesLayer agents={agentLayerNodes} connections={agentLayerConnections} />
      )}

      {/* Node labels — GPU-instanced text rendering */}
      <InstancedLabels
        nodes={typeFilteredNodes}
        nodeIdToIndexMap={nodeIdToIndexMap}
        nodePositionsRef={nodePositionsRef}
        labelPositionsRef={labelPositionsRef}
        settings={settings}
        graphMode={graphMode}
        perNodeVisualModeMap={perNodeVisualModeMap}
        connectionCountMap={connectionCountMap}
        hierarchyMap={hierarchyMap}
        graphTypeVisuals={graphTypeVisuals}
        ssspResult={normalizedSSSPResult}
        isXRMode={isXRMode}
      />

      {/* Sovereign-mesh overlay: BRIDGE_TO filaments, private-node opacity,
          tombstone markers (ADR-048 / ADR-049). Feature-flag gated inside. */}
      <SovereignOverlay
        nodes={typeFilteredNodes}
        nodePositionsRef={nodePositionsRef}
        nodeIdToIndexMap={nodeIdToIndexMap}
      />
    </>
  )
}

export default GraphManager