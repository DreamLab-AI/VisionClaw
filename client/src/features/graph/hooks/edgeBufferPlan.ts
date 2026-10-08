/**
 * edgeBufferPlan — split of the edge hot loop into a per-topology plan and a
 * per-frame fill.
 *
 * The per-frame edge loop used to resolve, for every edge on every frame, the
 * rendered-set membership, both node indices, both node radii
 * (`computeNodeScale`, twice per edge) and the edge-type colour. None of that
 * depends on node positions: it changes only when the graph, the filters or
 * the scaling settings change. On the live graph (9,473 nodes, 144,674 edges)
 * that redundant work cost about 490 ms a frame and accounted for most of the
 * main-thread long-task time on a cold load.
 *
 * `buildEdgePlan` resolves the invariant part once into typed arrays, with
 * node radii computed once per distinct endpoint. `fillEdgeFrame` and
 * `fillHighlightFrame` then run only the position-dependent maths, with the
 * same operations in the same order as the original loop, so the emitted
 * buffers are bit-identical.
 */
import * as THREE from 'three'
import type { GraphData } from '../managers/graphDataManager'
import type { GraphVisualMode } from './useGraphVisualState'
import { getEdgeTypeColor } from './useGraphNodeColors'
import { computeNodeScale } from '../utils/nodeScaling'

/** Everything the plan depends on. A change to any of these needs a new plan. */
export interface EdgePlanInputs {
  nodes: GraphData['nodes']
  edges: GraphData['edges']
  nodeIdToIndexMap: Map<string, number>
  connectionCountMap: Map<string, number>
  perNodeVisualModeMap: Map<string, GraphVisualMode>
  hierarchyMap: NonNullable<Parameters<typeof computeNodeScale>[3]>
  graphMode: GraphVisualMode
  /** IDs of rendered nodes; an edge is planned only when both ends are in it. */
  visibleNodeIds: Set<string>
  graphTypeVisuals: Parameters<typeof computeNodeScale>[4]
  nodeSize: number
}

/**
 * Position-invariant per-edge data, in original edge order, for edges whose
 * endpoints are both rendered and both present in the index map.
 */
export interface EdgePlan {
  count: number
  srcIdx: Int32Array
  tgtIdx: Int32Array
  /** Surface radius of each endpoint: `computeNodeScale(...) * nodeSize`. */
  srcR: Float64Array
  tgtR: Float64Array
  /** Edge-type colour, 3 floats per edge. */
  colors: Float32Array
  /** `edge.weight ?? 1.0`, one float per edge. */
  weights: Float32Array
}

type ScaleFn = typeof computeNodeScale

/**
 * Resolve the position-invariant part of every renderable edge.
 *
 * Radii are cached per node id string, so `scaleFn` runs at most once per
 * distinct endpoint rather than twice per edge per frame. The cache key is
 * the id string because the original loop looked the visual mode up by that
 * string, and the string determines the index.
 */
export function buildEdgePlan(inputs: EdgePlanInputs, scaleFn: ScaleFn = computeNodeScale): EdgePlan {
  const {
    nodes, edges, nodeIdToIndexMap, connectionCountMap, perNodeVisualModeMap,
    hierarchyMap, graphMode, visibleNodeIds, graphTypeVisuals, nodeSize,
  } = inputs

  const cap = edges.length
  const srcIdx = new Int32Array(cap)
  const tgtIdx = new Int32Array(cap)
  const srcR = new Float64Array(cap)
  const tgtR = new Float64Array(cap)
  const colors = new Float32Array(cap * 3)
  const weights = new Float32Array(cap)
  const radiusById = new Map<string, number>()

  const radiusOf = (id: string, idx: number): number => {
    let r = radiusById.get(id)
    if (r === undefined) {
      const mode = perNodeVisualModeMap.get(id) || graphMode
      r = scaleFn(nodes[idx], connectionCountMap, mode, hierarchyMap, graphTypeVisuals) * nodeSize
      radiusById.set(id, r)
    }
    return r
  }

  let count = 0
  for (let e = 0; e < cap; e++) {
    const edge = edges[e]
    const sourceStr = String(edge.source)
    const targetStr = String(edge.target)
    if (!visibleNodeIds.has(sourceStr) || !visibleNodeIds.has(targetStr)) continue
    const s = nodeIdToIndexMap.get(sourceStr)
    const t = nodeIdToIndexMap.get(targetStr)
    if (s === undefined || t === undefined) continue

    srcIdx[count] = s
    tgtIdx[count] = t
    srcR[count] = radiusOf(sourceStr, s)
    tgtR[count] = radiusOf(targetStr, t)
    const c = getEdgeTypeColor(edge.edgeType)
    colors[count * 3] = c.r
    colors[count * 3 + 1] = c.g
    colors[count * 3 + 2] = c.b
    weights[count] = edge.weight ?? 1.0
    count++
  }

  return { count, srcIdx, tgtIdx, srcR, tgtR, colors, weights }
}

/** Reusable per-frame output buffers. They grow and are never shrunk. */
export interface EdgeFrameBuffers {
  /** Surface-to-surface endpoints, 6 numbers per emitted edge. */
  points: number[]
  colors: Float32Array
  weights: Float32Array
  /** Highlight endpoints for the selected node, 6 numbers per emitted edge. */
  highlight: number[]
}

export function createEdgeFrameBuffers(): EdgeFrameBuffers {
  return { points: [], colors: new Float32Array(0), weights: new Float32Array(0), highlight: [] }
}

const tmpSrc = new THREE.Vector3()
const tmpTgt = new THREE.Vector3()
const tmpDir = new THREE.Vector3()
const tmpSrcOff = new THREE.Vector3()
const tmpTgtOff = new THREE.Vector3()

/**
 * Load both endpoints (substituting the drag position for the dragged node)
 * and offset them to the node surfaces. Returns false for a collapsed edge.
 * The vector operations match the original loop one for one.
 */
function offsetEndpoints(
  plan: EdgePlan, i: number, positions: Float32Array, dragIdx: number, dragPos: THREE.Vector3,
): boolean {
  const s = plan.srcIdx[i]
  const t = plan.tgtIdx[i]
  const i3s = s * 3
  const i3t = t * 3
  if (i3s + 2 >= positions.length || i3t + 2 >= positions.length) return false

  if (dragIdx === s) tmpSrc.set(dragPos.x, dragPos.y, dragPos.z)
  else tmpSrc.set(positions[i3s], positions[i3s + 1], positions[i3s + 2])
  if (dragIdx === t) tmpTgt.set(dragPos.x, dragPos.y, dragPos.z)
  else tmpTgt.set(positions[i3t], positions[i3t + 1], positions[i3t + 2])

  tmpDir.subVectors(tmpTgt, tmpSrc)
  if (tmpDir.length() <= 0.001) return false
  tmpDir.normalize()

  tmpSrcOff.copy(tmpSrc).addScaledVector(tmpDir, plan.srcR[i])
  tmpTgtOff.copy(tmpTgt).addScaledVector(tmpDir, -plan.tgtR[i])
  return true
}

/**
 * Fill the edge point, colour and weight buffers for this frame.
 *
 * @param dragIdx Node index of the dragged node, or -1.
 * @returns The number of edges emitted (points hold 6 numbers per edge).
 */
export function fillEdgeFrame(
  plan: EdgePlan, positions: Float32Array, dragIdx: number, dragPos: THREE.Vector3, out: EdgeFrameBuffers,
): number {
  const n = plan.count
  if (out.points.length < n * 6) out.points = new Array<number>(n * 6)
  if (out.colors.length < n * 3) out.colors = new Float32Array(n * 3)
  if (out.weights.length < n) out.weights = new Float32Array(n)
  const pts = out.points
  const cols = out.colors
  const wts = out.weights
  const pColors = plan.colors
  const pWeights = plan.weights

  let emitted = 0
  for (let i = 0; i < n; i++) {
    if (!offsetEndpoints(plan, i, positions, dragIdx, dragPos)) continue
    if (tmpSrcOff.distanceTo(tmpTgtOff) > 0.1) {
      const p = emitted * 6
      pts[p] = tmpSrcOff.x
      pts[p + 1] = tmpSrcOff.y
      pts[p + 2] = tmpSrcOff.z
      pts[p + 3] = tmpTgtOff.x
      pts[p + 4] = tmpTgtOff.y
      pts[p + 5] = tmpTgtOff.z
      const c = emitted * 3
      cols[c] = pColors[i * 3]
      cols[c + 1] = pColors[i * 3 + 1]
      cols[c + 2] = pColors[i * 3 + 2]
      wts[emitted] = pWeights[i]
      emitted++
    }
  }
  return emitted
}

/**
 * Fill the highlight buffer with the edges touching the selected node.
 *
 * @param selIdx Node index of the selected node, or -1 (emits nothing).
 * @returns The number of highlight edges emitted.
 */
export function fillHighlightFrame(
  plan: EdgePlan, positions: Float32Array, selIdx: number, dragIdx: number, dragPos: THREE.Vector3, out: EdgeFrameBuffers,
): number {
  if (selIdx < 0) return 0
  const n = plan.count
  if (out.highlight.length < n * 6) out.highlight = new Array<number>(n * 6)
  const hl = out.highlight
  let emitted = 0
  for (let i = 0; i < n; i++) {
    if (plan.srcIdx[i] !== selIdx && plan.tgtIdx[i] !== selIdx) continue
    if (!offsetEndpoints(plan, i, positions, dragIdx, dragPos)) continue
    if (tmpSrcOff.distanceTo(tmpTgtOff) > 0.2) {
      const p = emitted * 6
      hl[p] = tmpSrcOff.x
      hl[p + 1] = tmpSrcOff.y
      hl[p + 2] = tmpSrcOff.z
      hl[p + 3] = tmpTgtOff.x
      hl[p + 4] = tmpTgtOff.y
      hl[p + 5] = tmpTgtOff.z
      emitted++
    }
  }
  return emitted
}

/**
 * Bitwise snapshot of the shared position buffer.
 *
 * The graph worker writes positions into one shared Float32Array in place, so
 * identity says nothing about content. Comparing 32-bit patterns (rather than
 * float values) treats NaN in the same slot as unchanged and is exact.
 */
export class PositionSnapshot {
  private bits = new Uint32Array(0)
  private length = -1

  private static view(p: Float32Array): Uint32Array {
    return new Uint32Array(p.buffer, p.byteOffset, p.length)
  }

  /** True when `positions` has the same length and bits as the last record. */
  matches(positions: Float32Array): boolean {
    if (positions.length !== this.length) return false
    const cur = PositionSnapshot.view(positions)
    const prev = this.bits
    for (let i = 0; i < cur.length; i++) {
      if (cur[i] !== prev[i]) return false
    }
    return true
  }

  /** Record the current content of `positions`. */
  record(positions: Float32Array): void {
    if (this.bits.length < positions.length) this.bits = new Uint32Array(positions.length)
    this.bits.set(PositionSnapshot.view(positions))
    this.length = positions.length
  }
}

/** The inputs that decide whether a frame's edge output can differ from the last. */
export interface EdgeFrameState {
  plan: EdgePlan
  positions: Float32Array
  edgeHandle: object | null
  highlightHandle: object | null
  selectedNodeId: string | null
  dragging: boolean
}

/**
 * Skips the edge fill when nothing that feeds it has changed since the last
 * committed frame: same plan, same position bits, same selection, same mounted
 * handles, and no drag this frame or last. Once physics settles this turns
 * the per-frame edge work into one snapshot compare.
 *
 * It never skips while a handle is unmounted, because that path hands the
 * buffers to React state and must keep running until the handle attaches.
 */
export class EdgeFrameGate {
  private snapshot = new PositionSnapshot()
  private plan: EdgePlan | null = null
  private edgeHandle: object | null = null
  private highlightHandle: object | null = null
  private selectedNodeId: string | null = null
  private wasDragging = false

  shouldSkip(f: EdgeFrameState): boolean {
    if (f.dragging || this.wasDragging) return false
    if (f.edgeHandle === null || f.highlightHandle === null) return false
    return (
      f.plan === this.plan &&
      f.edgeHandle === this.edgeHandle &&
      f.highlightHandle === this.highlightHandle &&
      f.selectedNodeId === this.selectedNodeId &&
      this.snapshot.matches(f.positions)
    )
  }

  commit(f: EdgeFrameState): void {
    this.plan = f.plan
    this.edgeHandle = f.edgeHandle
    this.highlightHandle = f.highlightHandle
    this.selectedNodeId = f.selectedNodeId
    this.wasDragging = f.dragging
    this.snapshot.record(f.positions)
  }
}
