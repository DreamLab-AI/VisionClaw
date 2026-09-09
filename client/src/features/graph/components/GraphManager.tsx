import React, { useRef, useEffect, useState, useMemo, useCallback } from 'react'
import { useThree, useFrame, ThreeEvent } from '@react-three/fiber'
import * as THREE from 'three'
import { graphWorkerProxy } from '../managers/graphWorkerProxy'
import { createLogger } from '../../../utils/loggerConfig'
import { useSettingsStore } from '../../../store/settingsStore'
import { BinaryNodeData, getActualNodeId } from '../../../types/binaryProtocol'
import { GemNodes, GemNodesHandle } from './GemNodes'
import { GlassEdges, GlassEdgesHandle } from './GlassEdges'
import { InferredEdges } from './InferredEdges'
import { KnowledgeRings } from './KnowledgeRings'
import { ClusterHulls } from './ClusterHulls'
import { useGraphEventHandlers } from '../hooks/useGraphEventHandlers'
import { EdgeSettings } from '../../settings/config/settings'
import { useAnalyticsStore, useCurrentSSSPResult } from '../../analytics/store/analyticsStore'
import { TransientBeamsLayer, type BeamPositionResolver } from '../../visualisation/components/TransientBeamsLayer'
import { resolveNodeWorldPosition } from '../../visualisation/cameraFocus'
import { useBotsData } from '../../bots/contexts/BotsDataContext'
import { useGraphVisualState, type GraphVisualMode } from '../hooks/useGraphVisualState'
import { useGraphFiltering } from '../hooks/useGraphFiltering'
import { useFpsMonitor } from '../hooks/useFpsMonitor'
import { useCameraAutoFit, CAMERA_FIT_EVENT } from '../hooks/useCameraAutoFit'
import { InstancedLabels } from './InstancedLabels'
import { layoutApi, type LayoutPosition } from '../../../api/layoutApi'
import { type GraphData } from '../managers/graphDataManager'
import { useGraphDataSubscription } from '../hooks/useGraphDataSubscription'
import { useGraphSelection } from '../hooks/useGraphSelection'
import { useEdgeBufferComputation } from '../hooks/useEdgeBufferComputation'
import { nodePageUrl } from '../utils/pageLinks'
import { setSharedNodePositions, setSharedNodeIdToIndexMap } from '../contexts/NodePositionContext'

const logger = createLogger('GraphManager')

// Re-export GraphVisualMode from the hook for downstream consumers
export type { GraphVisualMode } from '../hooks/useGraphVisualState';

interface GraphManagerProps {
  onDragStateChange?: (isDragging: boolean) => void;
}

const GraphManager: React.FC<GraphManagerProps> = ({ onDragStateChange }) => {

  // Narrow selectors — subscribe only to sub-trees GraphManager actually reads.
  const knowledgeSettings  = useSettingsStore(s => s.settings?.visualisation?.graphs?.knowledge)
  const graphTypeVisuals = useSettingsStore(s => s.settings?.visualisation?.graphTypeVisuals)
  const debugSettings   = useSettingsStore(s => s.settings?.system?.debug)
  const nodeFilterSettings = useSettingsStore(s => s.settings?.nodeFilter)
  const nodeTypeVisibility = useSettingsStore(
    s => s.settings?.visualisation?.graphs?.knowledge?.nodes?.nodeTypeVisibility
  )
  // Stable ref for the full settings object — updated every render but doesn't trigger re-renders.
  const settingsRef = useRef(useSettingsStore.getState().settings)
  useEffect(() => {
    const unsub = useSettingsStore.subscribe(state => { settingsRef.current = state.settings })
    return unsub
  }, [])
  const settings = settingsRef.current

  const ssspResult = useCurrentSSSPResult()
  const normalizeDistances = useAnalyticsStore(state => state.normalizeDistances)
  const [normalizedSSSPResult, setNormalizedSSSPResult] = useState<any>(null)
  // One InstancedMesh per population (gem / orb / capsule). Separate refs so the
  // KnowledgeRings overlay can mirror the KNOWLEDGE mesh's colour buffer and the
  // pointer-handler guard can find any live mesh.
  const knowledgeGemRef = useRef<GemNodesHandle>(null)
  const ontologyGemRef = useRef<GemNodesHandle>(null)
  const agentGemRef = useRef<GemNodesHandle>(null)

  const [graphData, setGraphData] = useState<GraphData>({ nodes: [], edges: [] })

  // === Decomposed hooks: visual state + filtering ===
  const { perNodeVisualModeMap, hierarchyMap, connectionCountMap, dominantMode: graphMode } = useGraphVisualState(graphData)
  const { visibleNodes, nodeIdToIndexMap, expansionState } = useGraphFiltering(graphData, hierarchyMap, connectionCountMap)

  useEffect(() => { setSharedNodeIdToIndexMap(nodeIdToIndexMap) }, [nodeIdToIndexMap])

  // Node-type visibility filtering + per-population partition.
  //
  // We render ONE InstancedMesh per population — gem (knowledge), crystal orb
  // (ontology), capsule (agent) — instead of a single dominant-geometry mesh, so
  // a mixed graph shows every population with its correct primitive. The three
  // visible subsets are concatenated in a FIXED order [knowledge, ontology,
  // agent]; `typeFilteredNodes` is that ordered concat. The global instance index
  // (used by picking + double-click) stays 1:1 with it, while each sibling
  // GemNodes renders a contiguous slice with an `instanceIdBase` offset so a
  // mesh-local instanceId maps back to the correct global node.
  const {
    typeFilteredNodes, knowledgeNodes, ontologyNodes, agentNodes,
    ontologyBase, agentBase,
  } = useMemo(() => {
    const vis = nodeTypeVisibility
    const showK = !vis || vis.knowledge !== false
    const showO = !vis || vis.ontology  !== false
    const showA = !vis || vis.agent     !== false
    const k: typeof visibleNodes = []
    const o: typeof visibleNodes = []
    const a: typeof visibleNodes = []
    for (const node of visibleNodes) {
      const mode = perNodeVisualModeMap.get(String(node.id)) || graphMode
      if (mode === 'ontology')   { if (showO) o.push(node) }
      else if (mode === 'agent') { if (showA) a.push(node) }
      else                       { if (showK) k.push(node) } // knowledge_graph (default)
    }
    return {
      typeFilteredNodes: k.concat(o, a),
      knowledgeNodes: k, ontologyNodes: o, agentNodes: a,
      ontologyBase: k.length, agentBase: k.length + o.length,
    }
  }, [visibleNodes, perNodeVisualModeMap, graphMode, nodeTypeVisibility])

  // The exact set of rendered node IDs (every filter applied). The edge hot-loop
  // gates on this so edges never draw to a pruned endpoint (linked_page stubs,
  // low-degree, quality-filtered) — one source of truth shared with the meshes.
  const renderedNodeIds = useMemo(
    () => new Set(typeFilteredNodes.map(n => String(n.id))),
    [typeFilteredNodes],
  )

  // Cluster hulls are fed the full rendered population. The default hull source
  // is the server's spatial DBSCAN cluster_id (ClusterHulls gives it outright
  // priority): a DBSCAN cluster is spatially compact by construction, so it
  // never spans the KG<->ontology separation gap regardless of population — the
  // ontology-only scoping that the (opt-in) Louvain community fallback needed is
  // unnecessary and would hide hulls on the dominant knowledge disc, where the
  // clusters actually live. Reuse `typeFilteredNodes` so hulls cover exactly the
  // node set the meshes and edges render (one source of truth via renderedNodeIds).

  // BotsDataContext is the single agent state source (its useAgentPolling owns the
  // `/api/bots/agents` poll and reconciles binary SAB positions). GraphManager reads
  // it only for the beam anchor map below — the agent bodies are rendered by the
  // instanced capsules (agentGemRef) plus the BotsNode overlay (BotsVisualization).
  const { botsData } = useBotsData()
  useFpsMonitor()

  const nodePositionsRef = useRef<Float32Array | null>(null)

  // === Transient agent-action beams (0x23) — id-space resolution ===
  //
  // BotsDataContext keys each agent by the MASKED numeric node id as a string
  // (`String(getActualNodeId(nodeId))`) — that is how its binary-position
  // reconciliation attaches SAB positions to agents. We build a lookup from that
  // key to the agent's live world position so source_agent_id (which may arrive
  // with the high AGENT_NODE_FLAG bit set) resolves consistently via
  // getActualNodeId below. Sourcing from the context (rather than a second poll)
  // makes the anchor track live physics instead of a ≤5s telemetry snapshot.
  const botsAgents = botsData?.agents
  const agentPositionByMaskedId = useMemo(() => {
    const map = new Map<string, { x: number; y: number; z: number }>()
    for (const agent of botsAgents ?? []) {
      if (agent.position) map.set(String(agent.id), agent.position)
    }
    return map
  }, [botsAgents])
  const agentPositionMapRef = useRef(agentPositionByMaskedId)
  agentPositionMapRef.current = agentPositionByMaskedId

  // Resolve source_agent_id → agent world position. Mask the AGENT_NODE_FLAG
  // (and any other high flag bits) via getActualNodeId, then look up by the
  // masked string key; fall back to the raw id for safety. Returns false when
  // unresolvable so the beam is skipped silently.
  const resolveAgentPosition = useCallback<BeamPositionResolver>((id, out) => {
    const map = agentPositionMapRef.current
    const masked = getActualNodeId(id)
    const pos = map.get(String(masked)) ?? map.get(String(id))
    if (!pos) return false
    out.set(pos.x, pos.y, pos.z)
    return true
  }, [])

  // Resolve target_node_id → KG node world position from the LIVE position
  // buffer (SAB), via the same nodeIdToIndexMap the edge renderer uses. Tries
  // the raw id first, then the masked id (KG nodes may carry KNOWLEDGE/ontology
  // flag bits). Shared with the transcript click-to-fly path via cameraFocus's
  // resolveNodeWorldPosition so both agree on id-space + bounds. Returns false
  // when unresolvable so the beam is skipped.
  const resolveNodePosition = useCallback<BeamPositionResolver>((id, out) => {
    const pos = resolveNodeWorldPosition(id, nodeIdToIndexMap, nodePositionsRef.current)
    if (!pos) return false
    out.set(pos.x, pos.y, pos.z)
    return true
  }, [nodeIdToIndexMap])

  // Layout mode transition state
  const transitionRef = useRef<{
    active: boolean
    startPositions: Float32Array
    targetPositions: Float32Array
    progress: number
    duration: number
    startTime: number
  } | null>(null)
  const [activeLayoutMode, setActiveLayoutMode] = useState<string>('')
  const [layoutTransitioning, setLayoutTransitioning] = useState(false)

  const { requestFit: requestCameraFit } = useCameraAutoFit(nodePositionsRef, graphData.nodes.length)

  const [edgePoints, setEdgePoints]               = useState<number[]>([])
  const [highlightEdgePoints, setHighlightEdgePoints] = useState<number[]>([])
  const edgeFlowRef          = useRef<GlassEdgesHandle>(null)
  const highlightEdgeFlowRef = useRef<GlassEdgesHandle>(null)
  const prevLabelPositionsLengthRef = useRef<number>(0)
  const labelPositionsRef = useRef<Array<{x: number, y: number, z: number}>>([])
  const labelTickRef  = useRef(0)
  const [labelUpdateTick, setLabelUpdateTick] = useState(0)

  const [nodesAreAtOrigin, setNodesAreAtOrigin] = useState(false)
  const [forceUpdate, setForceUpdate] = useState(0)

  const frustum = useMemo(() => new THREE.Frustum(), [])
  const cameraViewProjectionMatrix = useMemo(() => new THREE.Matrix4(), [])

  const animationStateRef = useRef({
    time: 0,
    selectedNode: null as string | null,
    hoveredNode: null as string | null,
    pulsePhase: 0,
  })

  const [dragState, setDragState] = useState<{ nodeId: string | null; instanceId: number | null }>({
    nodeId: null,
    instanceId: null,
  })
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
  const controls = useThree(s => s.controls)
  const nodeSettings = knowledgeSettings?.nodes || settings?.visualisation?.nodes

  useEffect(() => {
    if (ssspResult) {
      const normalized = normalizeDistances(ssspResult)
      setNormalizedSSSPResult({ ...ssspResult, normalizedDistances: normalized })
    } else {
      setNormalizedSSSPResult(null)
    }
  }, [ssspResult, normalizeDistances])

  // ===  Layout mode transition helpers ===
  const startLayoutTransition = useCallback((targetPositions: LayoutPosition[], durationMs: number) => {
    const positions = nodePositionsRef.current
    const nodeCount = graphData.nodes.length
    if (!positions || nodeCount === 0) return

    const needed = nodeCount * 3
    const startSnap = new Float32Array(needed)
    startSnap.set(positions.subarray(0, Math.min(needed, positions.length)))

    const targetSnap = new Float32Array(needed)
    const idxById = new Map<number, number>()
    for (let i = 0; i < targetPositions.length; i++) idxById.set(targetPositions[i].id, i)
    for (let ni = 0; ni < nodeCount; ni++) {
      const node = graphData.nodes[ni]
      const numericId = parseInt(String(node.id), 10)
      const tp = idxById.get(numericId)
      if (tp !== undefined) {
        targetSnap[ni * 3]     = targetPositions[tp].x
        targetSnap[ni * 3 + 1] = targetPositions[tp].y
        targetSnap[ni * 3 + 2] = targetPositions[tp].z
      } else {
        targetSnap[ni * 3]     = startSnap[ni * 3]
        targetSnap[ni * 3 + 1] = startSnap[ni * 3 + 1]
        targetSnap[ni * 3 + 2] = startSnap[ni * 3 + 2]
      }
    }
    transitionRef.current = { active: true, startPositions: startSnap, targetPositions: targetSnap, progress: 0, duration: durationMs, startTime: Date.now() }
    setLayoutTransitioning(true)
  }, [graphData.nodes])

  // Physics fingerprint (ADR-03 D7: no-op worker forward; kept as dependency marker)
  const knowledgePhysics   = knowledgeSettings?.physics
  const visionclawPhysics = useSettingsStore(s => s.settings?.visualisation?.graphs?.visionclaw?.physics)
  const physicsFingerprint = useMemo(() => JSON.stringify({ vf: visionclawPhysics, lq: knowledgePhysics }), [visionclawPhysics, knowledgePhysics])
  useEffect(() => { void physicsFingerprint }, [physicsFingerprint])

  // layoutMode → layoutApi.setMode
  const layoutMode = useSettingsStore(s =>
    (s.settings as unknown as Record<string, Record<string, unknown>>)?.qualityGates?.layoutMode as string | undefined
  )
  const prevLayoutModeRef = useRef<string | undefined>(undefined)
  useEffect(() => {
    if (!layoutMode || layoutMode === prevLayoutModeRef.current) return
    prevLayoutModeRef.current = layoutMode
    const TRANSITION_MS = 800
    setActiveLayoutMode(layoutMode)
    setLayoutTransitioning(true)
    layoutApi.setMode(layoutMode, TRANSITION_MS).then(response => {
      const { data } = response
      if (data.success && data.positions && data.positions.length > 0) {
        startLayoutTransition(data.positions, data.transitionMs ?? TRANSITION_MS)
      } else {
        setLayoutTransitioning(false)
      }
    }).catch(err => {
      logger.warn('[GraphManager] layoutApi.setMode failed:', err)
      setLayoutTransitioning(false)
    })
  }, [layoutMode, startLayoutTransition])

  // === Graph data subscription ===
  useGraphDataSubscription({
    onGraphData: setGraphData,
    onEdgePoints: setEdgePoints,
    onNodesAtOrigin: setNodesAreAtOrigin,
    settings: settingsRef,
  })

  // === Selection state + camera fly-to + search events ===
  const { selectedNodeId, setSelectedNodeId, flyToTargetRef, flyToLookAtRef, flyToProgressRef } = useGraphSelection({
    graphData,
    nodeIdToIndexMap,
    nodePositionsRef,
    connectionCountMap,
    camera,
  })

  // Fly-to interpolation state: captured start endpoints + which destination we
  // captured them for (object identity distinguishes a NEW fly). Interpolating
  // captured-start → destination by eased progress (rather than a fractional
  // per-frame lerp) guarantees the camera actually reaches the destination.
  const flyStartPosRef = useRef<THREE.Vector3 | null>(null)
  const flyStartTargetRef = useRef<THREE.Vector3 | null>(null)
  const flyCapturedForRef = useRef<THREE.Vector3 | null>(null)
  // ADR-04 D10: persistent capture buffers, allocated once at mount, so the
  // fly-to capture writes into them instead of allocating inside useFrame. The
  // two *Ref values above alias these buffers while a fly is in flight; nothing
  // else writes to them, and the next capture overwrites them at exactly the
  // moment the refs are reassigned, so the alias is never stale.
  const flyStartPosBuf = useRef(new THREE.Vector3())
  const flyStartTargetBuf = useRef(new THREE.Vector3())

  const cancelFlyTo = useCallback(() => {
    flyToTargetRef.current = null
    flyToLookAtRef.current = null
    flyCapturedForRef.current = null
    flyStartPosRef.current = null
    flyStartTargetRef.current = null
  }, [flyToTargetRef, flyToLookAtRef])

  // Cancel an in-flight fly-to on any user camera interaction (OrbitControls
  // 'start') or an explicit Fit request, so the animation never fights the user
  // or a reframe. OrbitControls extends EventDispatcher; guard for the ref shape.
  useEffect(() => {
    const ctl = controls as unknown as {
      addEventListener?: (t: string, f: () => void) => void
      removeEventListener?: (t: string, f: () => void) => void
    } | null
    const onFit = () => cancelFlyTo()
    window.addEventListener(CAMERA_FIT_EVENT, onFit)
    ctl?.addEventListener?.('start', cancelFlyTo)
    return () => {
      window.removeEventListener(CAMERA_FIT_EVENT, onFit)
      ctl?.removeEventListener?.('start', cancelFlyTo)
    }
  }, [controls, cancelFlyTo])

  // === Priority -2 useFrame: SAB reads, layout transition LERP, label pos, camera fly-to ===
  useFrame((state, delta) => {
    animationStateRef.current.time = state.clock.elapsedTime

    // Camera fly-to animation (click-to-focus / search). ~600ms envelope. Both
    // the camera position and the OrbitControls pivot are interpolated from a
    // captured start toward the destination by eased progress, then snapped
    // exactly at completion so the camera truly arrives.
    if (flyToTargetRef.current) {
      const dest = flyToTargetRef.current
      const lookAt = flyToLookAtRef.current
      const ctl = controls as unknown as { target?: THREE.Vector3; update?: () => void } | null

      // Capture start endpoints once per new fly (destination object identity).
      if (flyCapturedForRef.current !== dest) {
        flyCapturedForRef.current = dest
        flyStartPosRef.current = flyStartPosBuf.current.copy(camera.position)
        flyStartTargetRef.current = ctl?.target
          ? flyStartTargetBuf.current.copy(ctl.target)
          : camera.getWorldDirection(flyStartTargetBuf.current).add(camera.position)
      }

      flyToProgressRef.current = Math.min(1, flyToProgressRef.current + delta / 0.6)
      const p = flyToProgressRef.current
      const eased = 1 - Math.pow(1 - p, 3)

      if (flyStartPosRef.current) camera.position.lerpVectors(flyStartPosRef.current, dest, eased)
      if (lookAt && ctl?.target && flyStartTargetRef.current) {
        ctl.target.lerpVectors(flyStartTargetRef.current, lookAt, eased)
        ctl.update?.()
      } else if (lookAt) {
        camera.lookAt(lookAt)
      }

      if (p >= 1) {
        camera.position.copy(dest)
        if (lookAt && ctl?.target) { ctl.target.copy(lookAt); ctl.update?.() }
        else if (lookAt) camera.lookAt(lookAt)
        cancelFlyTo()
      }
    }

    // Periodic label frustum refresh (~4 updates/sec at 60fps)
    labelTickRef.current++
    if (labelTickRef.current >= 15) {
      labelTickRef.current = 0
      cameraViewProjectionMatrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse)
      frustum.setFromProjectionMatrix(cameraViewProjectionMatrix)
      setLabelUpdateTick(prev => prev + 1)
    }

    if (graphData.nodes.length > 0) {
      const positions = graphWorkerProxy.getPositionsSync()
      if (!positions) return

      if (!nodePositionsRef.current) {
        let hasNonZero = false
        const checkLen = Math.min(graphData.nodes.length * 3, positions.length)
        for (let ci = 0; ci < checkLen; ci++) {
          if (positions[ci] !== 0) { hasNonZero = true; break }
        }
        if (!hasNonZero && checkLen > 0) return
      }
      nodePositionsRef.current = positions
      setSharedNodePositions(positions)

      // Layout mode transition: mass-aware LERP
      if (transitionRef.current?.active) {
        const t = transitionRef.current
        const elapsed = Date.now() - t.startTime
        const rawProgress = Math.min(elapsed / t.duration, 1.0)
        const progress = rawProgress < 0.5
          ? 2 * rawProgress * rawProgress
          : 1 - Math.pow(-2 * rawProgress + 2, 2) / 2
        const nodeCount = graphData.nodes.length
        for (let i = 0; i < nodeCount; i++) {
          const idx = i * 3
          if (idx + 2 >= positions.length) break
          const cc = connectionCountMap.get(String(i)) || 0
          const massFactor = 1.0 / (1.0 + Math.sqrt(cc) * 0.3)
          const np = Math.min(progress / massFactor, 1.0)
          positions[idx]     = t.startPositions[idx]     + (t.targetPositions[idx]     - t.startPositions[idx])     * np
          positions[idx + 1] = t.startPositions[idx + 1] + (t.targetPositions[idx + 1] - t.startPositions[idx + 1]) * np
          positions[idx + 2] = t.startPositions[idx + 2] + (t.targetPositions[idx + 2] - t.startPositions[idx + 2]) * np
        }
        if (rawProgress >= 1.0) { transitionRef.current.active = false; setLayoutTransitioning(false) }
      }

      requestCameraFit()

      const positionsValid = positions.length >= graphData.nodes.length * 3

      if (positionsValid) {
        // Update label positions ref every frame (fast, no re-render)
        const labelCount = graphData.nodes.length
        let labelArr = labelPositionsRef.current
        if (labelArr.length !== labelCount) {
          labelArr = new Array(labelCount)
          for (let i = 0; i < labelCount; i++) labelArr[i] = { x: 0, y: 0, z: 0 }
        }
        for (let i = 0; i < labelCount; i++) {
          const i3 = i * 3
          labelArr[i].x = positions[i3]
          labelArr[i].y = positions[i3 + 1]
          labelArr[i].z = positions[i3 + 2]
        }
        labelPositionsRef.current = labelArr
        prevLabelPositionsLengthRef.current = labelCount
      }
    }
  }, -2)

  // === Edge buffer computation (extracted hot loop) — runs in its own useFrame(-2) ===
  useEdgeBufferComputation({
    graphData,
    nodePositionsRef,
    nodeIdToIndexMap,
    connectionCountMap,
    perNodeVisualModeMap,
    hierarchyMap,
    graphMode,
    visibleNodeIds: renderedNodeIds,
    graphTypeVisuals,
    nodeSize: nodeSettings?.nodeSize ?? 0.5,
    selectedNodeId,
    dragDataRef,
    edgeFlowRef,
    highlightEdgeFlowRef,
    highlightEdgePoints,
    setEdgePoints,
    setHighlightEdgePoints,
  })

  // Proxy ref: useGraphEventHandlers expects RefObject<InstancedMesh>. The guard
  // only needs ANY live population mesh, so return the first that exists.
  const meshProxyRef = useMemo(() => ({
    get current() {
      return knowledgeGemRef.current?.getMesh()
        ?? ontologyGemRef.current?.getMesh()
        ?? agentGemRef.current?.getMesh()
        ?? null
    },
    set current(_v: any) { /* GemNodes owns the mesh */ },
  }), []) as React.RefObject<THREE.InstancedMesh>

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
    setSelectedNodeId,
  )

  const defaultEdgeSettings: EdgeSettings = {
    arrowSize: 0.5,
    baseWidth: 0.1,
    color: '#FF5722',
    enableArrows: true,
    opacity: 0.15,
    widthRange: [0.1, 0.3],
    quality: 'medium',
    enableFlowEffect: false,
    flowSpeed: 1,
    flowIntensity: 1,
    glowStrength: 1,
    distanceIntensity: 0.5,
    useGradient: false,
    gradientColors: ['#ff0000', '#0000ff'],
  }

  useEffect(() => {
    if (debugSettings?.enableNodeDebug) {
      logger.debug('Component mounted', {
        nodeCount: graphData.nodes.length,
        edgeCount: graphData.edges.length,
        edgePointsLength: edgePoints.length,
        gemNodesRef: !!knowledgeGemRef.current,
      })
    }
    return () => {
      if (debugSettings?.enableNodeDebug) logger.debug('Component unmounting')
    }
  }, [])

  // Shared pointer-miss + double-click handlers (one identity reused by all
  // three population meshes). event.instanceId is GLOBAL — each GemNodes adds
  // its instanceIdBase before delegating, so it indexes typeFilteredNodes here.
  const handlePointerMissed = useCallback(() => {
    if (dragDataRef.current.pointerDown) handlePointerUp()
    setSelectedNodeId(null)
  }, [handlePointerUp, setSelectedNodeId])

  const handleNodeDoubleClick = useCallback((event: ThreeEvent<MouseEvent>) => {
    if (event.instanceId !== undefined && event.instanceId < typeFilteredNodes.length) {
      // First hit wins: the ray often pierces several instances (and the
      // co-located knowledge + ontology nodes of one concept live in two
      // meshes sharing this handler) — without stopping propagation a single
      // double-click opened the page once per delivered intersection.
      event.stopPropagation()
      const node = typeFilteredNodes[event.instanceId]
      if (node) {
        // Slug-first resolution (metadataId IS the api/pages key) against the
        // path-routed site — the old #/page/<Title> hash route is dead.
        const url = nodePageUrl(node)
        if (url) { window.open(url, '_blank', 'noopener,noreferrer'); return }
        const hierarchyNode = hierarchyMap.get(node.id)
        if (hierarchyNode && hierarchyNode.childIds.length > 0) expansionState.toggleExpansion(node.id)
      }
    }
  }, [typeFilteredNodes, hierarchyMap, expansionState])

  // Right-click a node → dispatch the additive-expansion context menu. The menu
  // itself lives in an HTML overlay (NodeContextMenu, mounted in MainLayout); we
  // only surface which node was hit plus the screen coordinates to anchor it.
  const handleNodeContextMenu = useCallback((event: ThreeEvent<MouseEvent>) => {
    if (event.instanceId === undefined || event.instanceId >= typeFilteredNodes.length) return
    event.stopPropagation()
    // Suppress the browser's native context menu over the canvas.
    event.nativeEvent?.preventDefault?.()
    const node = typeFilteredNodes[event.instanceId]
    if (!node) return
    // Resolve the node's LIVE world position from the SAB buffer (physics owns
    // positions post-load) so additive-merge seeds new nodes from where the
    // anchor actually IS, not its stale cached load-time position.
    const livePos = resolveNodeWorldPosition(
      Number(node.id), nodeIdToIndexMap, nodePositionsRef.current,
    ) ?? node.position ?? null
    window.dispatchEvent(new CustomEvent('visionclaw:node-contextmenu', {
      detail: {
        nodeId: String(node.id),
        label: node.label,
        x: event.nativeEvent?.clientX ?? 0,
        y: event.nativeEvent?.clientY ?? 0,
        position: livePos,
      },
    }))
  }, [typeFilteredNodes, nodeIdToIndexMap])

  // One mesh per population. Empty populations render nothing (no wasted mesh).
  // `base` must match the contiguous ordering of typeFilteredNodes above.
  const populationMeshes: Array<{
    ref: React.RefObject<GemNodesHandle | null>; nodes: typeof knowledgeNodes; mode: GraphVisualMode; base: number
  }> = [
    { ref: knowledgeGemRef, nodes: knowledgeNodes, mode: 'knowledge_graph', base: 0 },
    { ref: ontologyGemRef,  nodes: ontologyNodes,  mode: 'ontology',        base: ontologyBase },
    { ref: agentGemRef,     nodes: agentNodes,     mode: 'agent',           base: agentBase },
  ]

  return (
    <>
      {populationMeshes.map(({ ref, nodes, mode, base }) => (
        nodes.length === 0 ? null : (
          <GemNodes
            key={mode}
            ref={ref}
            nodes={nodes}
            forceMode={mode}
            instanceIdBase={base}
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
            onPointerMissed={handlePointerMissed}
            onDoubleClick={handleNodeDoubleClick}
            onContextMenu={handleNodeContextMenu}
            selectedNodeId={selectedNodeId}
          />
        )
      ))}

      <GlassEdges
        ref={edgeFlowRef}
        points={edgePoints}
        settings={settings?.visualisation?.graphs?.knowledge?.edges || settings?.visualisation?.edges || defaultEdgeSettings}
        colorOverride={
          graphMode === 'knowledge_graph'
            ? settings?.visualisation?.graphTypeVisuals?.knowledgeGraph?.edgeColor
            : graphMode === 'ontology'
            ? settings?.visualisation?.graphTypeVisuals?.ontology?.edgeColor
            : undefined
        }
      />

      <GlassEdges
        ref={highlightEdgeFlowRef}
        points={highlightEdgePoints}
        settings={settings?.visualisation?.graphs?.knowledge?.edges || settings?.visualisation?.edges || defaultEdgeSettings}
        colorOverride={settings?.visualisation?.interaction?.selectionHighlightColor || '#00FFFF'}
      />

      {/* Inferred-graph edges (urn:ngm:graph:ontology:inferred) rendered in a
          distinct dashed-amber style, gated by the InferencePanel toggle.
          Additive overlay — does not touch the GlassEdges instanced pipeline. */}
      <InferredEdges
        nodePositionsRef={nodePositionsRef}
        nodeIdToIndexMap={nodeIdToIndexMap}
      />

      <KnowledgeRings
        nodes={knowledgeNodes}
        perNodeVisualModeMap={perNodeVisualModeMap}
        nodePositionsRef={nodePositionsRef}
        nodeIdToIndexMap={nodeIdToIndexMap}
        connectionCountMap={connectionCountMap}
        edges={graphData.edges}
        hierarchyMap={hierarchyMap}
        settings={settings}
        nodeColorSourceRef={knowledgeGemRef}
      />

      <ClusterHulls
        nodes={typeFilteredNodes}
        nodePositionsRef={nodePositionsRef}
        nodeIdToIndexMap={nodeIdToIndexMap}
        settings={settings}
      />

      {/* Embodied agent-action beams (0x23): agent node → KG node, coloured
          by action type, opacity fades in → holds → out over duration_ms.
          Radius/opacity are control-centre Agents → Behaviour knobs. */}
      <TransientBeamsLayer
        resolveAgentPosition={resolveAgentPosition}
        resolveNodePosition={resolveNodePosition}
        beamRadius={graphTypeVisuals?.agent?.beamRadius ?? 0.35}
        maxOpacity={graphTypeVisuals?.agent?.beamOpacity ?? 0.85}
      />

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
      />
    </>
  )
}

export default GraphManager
