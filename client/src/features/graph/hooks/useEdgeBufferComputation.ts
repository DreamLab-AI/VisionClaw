/**
 * useEdgeBufferComputation — useFrame hot loop for edge buffer computation.
 *
 * Extracted from GraphManager.tsx (Phase B1 modularisation).
 * Reads SAB positions each frame, computes surface-to-surface edge endpoints,
 * fills pre-allocated buffers, and pushes to GlassEdgesHandle imperatively.
 * The position-invariant per-edge work lives in a memoised plan
 * (edgeBufferPlan.ts); the frame runs only position maths, and skips entirely
 * when positions, plan, selection and handles are unchanged.
 */
import { useMemo, useRef } from 'react'
import { useFrame, useThree } from '@react-three/fiber'
import type { GraphData } from '../managers/graphDataManager'
import type { GlassEdgesHandle } from '../components/GlassEdges'
import type { GraphVisualMode } from './useGraphVisualState'
import {
  buildEdgePlan, createEdgeFrameBuffers, fillEdgeFrame, fillHighlightFrame,
  EdgeFrameGate, type EdgeFrameBuffers, type EdgePlan,
} from './edgeBufferPlan'
import { createLogger } from '../../../utils/loggerConfig'
import { highlightMask } from '../state/highlightMask'
import {
  buildEdgeAdjacency, createTubeSelectScratch, emphasisNodes, selectTubeEdges,
  type GlassTubeLod,
} from '../overview/tubeSelect'

/** Frames between glass-tube subset refreshes while the camera or layout moves. */
export const TUBE_REFRESH_FRAMES = 6

const logger = createLogger('useEdgeBufferComputation')

export interface EdgeBufferComputationOptions {
  graphData: GraphData
  nodePositionsRef: React.MutableRefObject<Float32Array | null>
  nodeIdToIndexMap: Map<string, number>
  connectionCountMap: Map<string, number>
  perNodeVisualModeMap: Map<string, GraphVisualMode>
  hierarchyMap: Map<string, any>
  graphMode: GraphVisualMode
  /** IDs of nodes actually rendered (GraphManager's typeFilteredNodes). An edge
   *  is drawn only when BOTH endpoints are in this set, so pruned nodes
   *  (linked_page stubs, low-degree, quality-filtered, or type-toggled-off)
   *  never leave dangling edges. Single source of truth shared with the meshes. */
  visibleNodeIds: Set<string>
  graphTypeVisuals: any
  nodeSize: number
  selectedNodeId: string | null
  edgeFlowRef: React.RefObject<GlassEdgesHandle | null>
  highlightEdgeFlowRef: React.RefObject<GlassEdgesHandle | null>
  highlightEdgePoints: number[]
  setEdgePoints: (pts: number[]) => void
  setHighlightEdgePoints: (pts: number[]) => void
  /**
   * ADR-2181 D3: when set (overview edges enabled and not faulted, pending or
   * live; overview/tubeSelect.ts resolveTubeLod), the glass tubes are fed only
   * the tube subset: emphasis edges plus close-zoom edges, at most `budget`.
   * Null feeds every edge (the safety valve when the overview layer is off or
   * failed).
   */
  tubeLod?: GlassTubeLod | null
}

/**
 * Returns the memoised edge plan: the overview edges (ADR-2181, OverviewLayer)
 * draw from the same renderable set, endpoint indices, colours and weights.
 */
export function useEdgeBufferComputation(opts: EdgeBufferComputationOptions): { plan: EdgePlan } {
  const {
    graphData, nodePositionsRef, nodeIdToIndexMap, connectionCountMap,
    perNodeVisualModeMap, hierarchyMap, graphMode, visibleNodeIds,
    graphTypeVisuals, nodeSize, selectedNodeId,
    edgeFlowRef, highlightEdgeFlowRef, highlightEdgePoints,
    setEdgePoints, setHighlightEdgePoints, tubeLod = null,
  } = opts
  const camera = useThree((s) => s.camera)

  // Position-invariant per-edge data (visibility, indices, node radii,
  // colours, weights). Rebuilt only when topology, filters or scaling inputs
  // change, never per frame: resolving it per edge per frame cost ~490 ms a
  // frame on the 144k-edge graph (computeNodeScale twice per edge).
  const plan = useMemo(
    () => buildEdgePlan({
      nodes: graphData.nodes, edges: graphData.edges, nodeIdToIndexMap, connectionCountMap,
      perNodeVisualModeMap, hierarchyMap, graphMode, visibleNodeIds, graphTypeVisuals, nodeSize,
    }),
    [graphData, nodeIdToIndexMap, connectionCountMap, perNodeVisualModeMap, hierarchyMap,
      graphMode, visibleNodeIds, graphTypeVisuals, nodeSize],
  )

  // Node → edge adjacency for the tube subset; built once per plan, and only
  // while the subset path is in use.
  const subsetOn = tubeLod !== null
  const adjacency = useMemo(
    () => (subsetOn ? buildEdgeAdjacency(plan, graphData.nodes.length) : null),
    [subsetOn, plan, graphData.nodes.length],
  )
  const tubeRef = useRef({
    out: { idx: new Int32Array(0), count: 0 },
    scratch: createTubeSelectScratch(),
    emphasis: new Int32Array(0),
    emphasisCount: 0,
    maskVersion: -1,
    frame: 0,
    version: 1,
    lod: null as GlassTubeLod | null,
    adjacency: null as unknown,
    cam: { x: NaN, y: NaN, z: NaN },
    snapshotLen: -1,
  })

  // Frame buffers — grow only, never shrink.
  const buffersRef = useRef<EdgeFrameBuffers | null>(null)
  if (buffersRef.current === null) buffersRef.current = createEdgeFrameBuffers()
  const gateRef = useRef<EdgeFrameGate | null>(null)
  if (gateRef.current === null) gateRef.current = new EdgeFrameGate()
  const edgeUpdatePendingRef  = useRef<number[] | null>(null)
  const hlUpdatePendingRef    = useRef<number[] | null>(null)
  // Highlight edges last pushed through the imperative handle, so a deselect
  // clears them (the React-state copy is only written before the handle mounts).
  const hlPushedRef           = useRef(0)

  useFrame(() => {
    const positions = nodePositionsRef.current
    if (!positions) return

    // Position-buffer sufficiency is a function of NODE count (3 floats/node),
    // never edge count. The prior `positions.length >= graphData.edges.length`
    // guard froze the whole edge pipeline whenever a graph had more edges than
    // position-buffer floats (e.g. 94k edges vs 84k floats) — edges then never
    // refiltered/cleared, so node-type visibility toggles had no effect on them.
    if (graphData.nodes.length === 0 || positions.length < graphData.nodes.length * 3) return

    // Glass-tube subset (ADR-2181 D3): re-selected when the mask, plan or LOD
    // changes, and every TUBE_REFRESH_FRAMES frames while camera or layout
    // moves. Between refreshes only the subset's endpoints are re-filled.
    const tube = tubeRef.current
    if (tubeLod && adjacency) {
      const maskVersion = highlightMask.version
      const cam = camera.position
      const camMoved = cam.x !== tube.cam.x || cam.y !== tube.cam.y || cam.z !== tube.cam.z
      const structural = maskVersion !== tube.maskVersion || tube.lod !== tubeLod || tube.adjacency !== adjacency
      tube.frame++
      if (structural || (tube.frame >= TUBE_REFRESH_FRAMES && (camMoved || !gateRef.current!.positionsMatch(positions)))) {
        if (maskVersion !== tube.maskVersion || tube.adjacency !== adjacency) {
          const mask = highlightMask.getMask()
          if (tube.emphasis.length < mask.length) tube.emphasis = new Int32Array(mask.length)
          tube.emphasisCount = emphasisNodes(mask, tube.emphasis)
        }
        selectTubeEdges({
          plan, adjacency, positions, mask: highlightMask.getMask(),
          emphasis: tube.emphasis, emphasisCount: tube.emphasisCount,
          camX: cam.x, camY: cam.y, camZ: cam.z, lod: tubeLod,
        }, tube.out, tube.scratch)
        tube.maskVersion = maskVersion
        tube.lod = tubeLod
        tube.adjacency = adjacency
        tube.cam.x = cam.x; tube.cam.y = cam.y; tube.cam.z = cam.z
        tube.frame = 0
        tube.version++
      }
    }

    const frame = {
      plan, positions,
      edgeHandle: edgeFlowRef.current,
      highlightHandle: highlightEdgeFlowRef.current,
      selectedNodeId,
      subsetVersion: tubeLod ? tube.version : 0,
    }
    // Nothing that feeds the edge buffers changed since the last frame (the
    // settled steady state): the meshes already hold this exact output.
    const gate = gateRef.current!
    if (gate.shouldSkip(frame)) return

    const buffers   = buffersRef.current!

    const emitted = tubeLod && adjacency
      ? fillEdgeFrame(plan, positions, buffers, tube.out.idx, tube.out.count)
      : fillEdgeFrame(plan, positions, buffers)
    const edgePointIdx = emitted * 6

    // Highlight edges for selected node
    if (selectedNodeId) {
      const selIdx = nodeIdToIndexMap.get(selectedNodeId) ?? -1
      const hlCount = fillHighlightFrame(plan, positions, selIdx, buffers, adjacency)
      const hlIdx = hlCount * 6
      if (highlightEdgeFlowRef.current) {
        highlightEdgeFlowRef.current.updatePoints(buffers.highlight, hlIdx)
        hlPushedRef.current = hlCount
      } else {
        hlUpdatePendingRef.current = buffers.highlight.slice(0, hlIdx)
      }
    } else if (highlightEdgePoints.length > 0 || hlPushedRef.current > 0) {
      if (highlightEdgeFlowRef.current) {
        highlightEdgeFlowRef.current.updatePoints([])
        hlPushedRef.current = 0
      } else {
        hlUpdatePendingRef.current = []
      }
    }

    // Push edge buffers
    if (edgeFlowRef.current) {
      // Widths BEFORE points: updatePoints composes matrices reading the stored
      // weights, so the radius factor must be current when matrices rebuild.
      edgeFlowRef.current.updateWidths(buffers.weights, emitted)
      edgeFlowRef.current.updatePoints(buffers.points, edgePointIdx)
      if (emitted > 0) {
        edgeFlowRef.current.updateColors(buffers.colors, emitted)
      }
    } else {
      edgeUpdatePendingRef.current = buffers.points.slice(0, edgePointIdx)
    }

    // Flush pending state updates for initial mount before imperative handles are available
    if (edgeUpdatePendingRef.current && !edgeFlowRef.current) {
      const pending = edgeUpdatePendingRef.current
      edgeUpdatePendingRef.current = null
      setEdgePoints(pending)
    }
    if (hlUpdatePendingRef.current !== null && !highlightEdgeFlowRef.current) {
      const pending = hlUpdatePendingRef.current
      hlUpdatePendingRef.current = null
      setHighlightEdgePoints(pending)
    }

    gate.commit(frame)

    // One-time diagnostic
    if (!(window as unknown as Record<string, boolean>).__gmDiagV2) {
      ;(window as unknown as Record<string, boolean>).__gmDiagV2 = true
      let nonZeroCount = 0
      for (let si = 0; si < graphData.nodes.length; si++) {
        const si3 = si * 3
        if (Math.abs(positions[si3]) > 0.01 || Math.abs(positions[si3 + 1]) > 0.01 || Math.abs(positions[si3 + 2]) > 0.01) nonZeroCount++
      }
      logger.debug('[useEdgeBufferComputation] DIAG first frame:', {
        nodeCount: graphData.nodes.length,
        edgeCount: graphData.edges.length,
        positionsLength: positions.length,
        edgePointsComputed: emitted,
        nonZeroPositions: nonZeroCount,
        hasEdgeFlowRef: !!edgeFlowRef.current,
      })
    }
  // After GraphManager's -2 pass, which builds this frame's drawn-positions array (the array
  // `positions` is), and before the priority-0 consumers.
  }, -0.5)

  return { plan }
}
