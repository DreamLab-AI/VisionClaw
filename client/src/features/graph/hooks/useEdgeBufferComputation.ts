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
import { useFrame } from '@react-three/fiber'
import type * as THREE from 'three'
import type { GraphData } from '../managers/graphDataManager'
import type { GlassEdgesHandle } from '../components/GlassEdges'
import type { GraphVisualMode } from './useGraphVisualState'
import {
  buildEdgePlan, createEdgeFrameBuffers, fillEdgeFrame, fillHighlightFrame,
  EdgeFrameGate, type EdgeFrameBuffers,
} from './edgeBufferPlan'
import { createLogger } from '../../../utils/loggerConfig'

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
  dragDataRef: React.MutableRefObject<{
    isDragging: boolean
    nodeId: string | null
    currentNodePos3D: THREE.Vector3
  }>
  edgeFlowRef: React.RefObject<GlassEdgesHandle | null>
  highlightEdgeFlowRef: React.RefObject<GlassEdgesHandle | null>
  highlightEdgePoints: number[]
  setEdgePoints: (pts: number[]) => void
  setHighlightEdgePoints: (pts: number[]) => void
}

export function useEdgeBufferComputation(opts: EdgeBufferComputationOptions) {
  const {
    graphData, nodePositionsRef, nodeIdToIndexMap, connectionCountMap,
    perNodeVisualModeMap, hierarchyMap, graphMode, visibleNodeIds,
    graphTypeVisuals, nodeSize, selectedNodeId, dragDataRef,
    edgeFlowRef, highlightEdgeFlowRef, highlightEdgePoints,
    setEdgePoints, setHighlightEdgePoints,
  } = opts

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

    const isDragging = dragDataRef.current.isDragging
    const frame = {
      plan, positions,
      edgeHandle: edgeFlowRef.current,
      highlightHandle: highlightEdgeFlowRef.current,
      selectedNodeId,
      dragging: isDragging,
    }
    // Nothing that feeds the edge buffers changed since the last frame (the
    // settled steady state): the meshes already hold this exact output.
    const gate = gateRef.current!
    if (gate.shouldSkip(frame)) return

    const buffers   = buffersRef.current!
    const dragNodeId = isDragging ? dragDataRef.current.nodeId : null
    const dragIdx   = dragNodeId === null ? -1 : (nodeIdToIndexMap.get(dragNodeId) ?? -1)
    const dragPos   = dragDataRef.current.currentNodePos3D

    const emitted = fillEdgeFrame(plan, positions, dragIdx, dragPos, buffers)
    const edgePointIdx = emitted * 6

    // Highlight edges for selected node
    if (selectedNodeId) {
      const selIdx = nodeIdToIndexMap.get(selectedNodeId) ?? -1
      const hlCount = fillHighlightFrame(plan, positions, selIdx, dragIdx, dragPos, buffers)
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
  }, -2)
}
