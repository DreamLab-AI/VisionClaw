/**
 * Edge buffer plan — equivalence and cost contract.
 *
 * The per-frame edge loop used to resolve visibility, node indices, node radii
 * (computeNodeScale) and edge colours for every edge on every frame: about
 * 490 ms a frame on the live 9,473-node / 144,674-edge graph, nearly all of it
 * computeNodeScale called twice per edge. The plan moves that node- and
 * topology-invariant work out of the frame. These tests pin that the frame
 * output stays identical to the original algorithm (copied verbatim below as
 * the reference) and that the radius work is per node, not per edge.
 */
import { describe, it, expect, vi } from 'vitest'
import * as THREE from 'three'
import { computeNodeScale } from '../utils/nodeScaling'
import { getEdgeTypeColor } from '../hooks/useGraphNodeColors'
import type { GraphVisualMode } from '../hooks/useGraphVisualState'
import {
  buildEdgePlan,
  fillEdgeFrame,
  fillHighlightFrame,
  createEdgeFrameBuffers,
  PositionSnapshot,
  EdgeFrameGate,
  type EdgePlanInputs,
} from '../hooks/edgeBufferPlan'

// --- deterministic fixture ---------------------------------------------------

function rng(seed: number) {
  let s = seed >>> 0
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0
    return s / 4294967296
  }
}

const EDGE_TYPES = [undefined, 'associative', 'hierarchical', 'SUBCLASS_OF', 'unknown-kind', 'dependency']

function makeFixture(seed: number, nodeCount = 300, edgeCount = 2000) {
  const r = rng(seed)
  const nodes = Array.from({ length: nodeCount }, (_, i) => ({
    id: String(i),
    label: `n${i}`,
    position: { x: 0, y: 0, z: 0 },
    metadata: {
      size: r() < 0.3 ? 1 + r() * 2 : undefined,
      file_size: r() < 0.6 ? String(Math.floor(r() * 9000)) : undefined,
      instanceCount: r() < 0.2 ? String(Math.floor(r() * 30)) : undefined,
      depth: r() < 0.3 ? Math.floor(r() * 5) : undefined,
      authority: r() < 0.3 ? r() : undefined,
      workload: r() < 0.3 ? r() : undefined,
      tokenRate: r() < 0.3 ? r() * 200 : undefined,
    },
  }))
  const edges = Array.from({ length: edgeCount }, (_, i) => {
    // Some edges point at ids that are missing from the index map.
    const source = r() < 0.01 ? 'ghost' : String(Math.floor(r() * nodeCount))
    const target = String(Math.floor(r() * nodeCount))
    return {
      id: `e${i}`,
      source,
      target,
      weight: r() < 0.7 ? r() * 5 : undefined,
      edgeType: EDGE_TYPES[Math.floor(r() * EDGE_TYPES.length)],
    }
  })
  const nodeIdToIndexMap = new Map<string, number>(nodes.map((n, i) => [n.id, i]))
  const connectionCountMap = new Map<string, number>()
  for (const e of edges) {
    connectionCountMap.set(e.source, (connectionCountMap.get(e.source) || 0) + 1)
    connectionCountMap.set(e.target, (connectionCountMap.get(e.target) || 0) + 1)
  }
  const modes: GraphVisualMode[] = ['knowledge_graph', 'ontology', 'agent']
  const perNodeVisualModeMap = new Map<string, GraphVisualMode>()
  for (const n of nodes) if (r() < 0.8) perNodeVisualModeMap.set(n.id, modes[Math.floor(r() * 3)])
  const hierarchyMap = new Map<string, any>()
  for (const n of nodes) if (r() < 0.2) hierarchyMap.set(n.id, { depth: Math.floor(r() * 4) })
  // ~10% of nodes are pruned from the rendered set.
  const visibleNodeIds = new Set<string>(nodes.filter(() => r() > 0.1).map(n => n.id))
  // Positions: a few coincident pairs (zero-length edges) and a short buffer
  // tail so the bounds check is exercised.
  const positions = new Float32Array(nodeCount * 3 - 3)
  for (let i = 0; i < positions.length; i++) positions[i] = (r() - 0.5) * 400
  positions.set(positions.subarray(0, 3), 3) // node 1 sits on node 0
  const inputs: EdgePlanInputs = {
    nodes: nodes as any,
    edges: edges as any,
    nodeIdToIndexMap,
    connectionCountMap,
    perNodeVisualModeMap,
    hierarchyMap,
    graphMode: 'knowledge_graph',
    visibleNodeIds,
    graphTypeVisuals: undefined,
    nodeSize: 0.5,
  }
  return { inputs, positions }
}

// --- reference: the original per-frame algorithm, verbatim -------------------

function referenceFrame(
  inp: EdgePlanInputs,
  positions: Float32Array,
  dragNodeId: string | null,
  dragPos: THREE.Vector3,
  selectedNodeId: string | null,
) {
  const tempVec3 = new THREE.Vector3()
  const tempPosition = new THREE.Vector3()
  const tempDirection = new THREE.Vector3()
  const tempSrcOff = new THREE.Vector3()
  const tempTgtOff = new THREE.Vector3()
  const { nodes, edges, nodeIdToIndexMap, connectionCountMap, perNodeVisualModeMap, hierarchyMap, graphMode, visibleNodeIds, graphTypeVisuals, nodeSize } = inp
  const points: number[] = []
  const colors: number[] = []
  const weights: number[] = []
  edges.forEach((edge: any) => {
    const sourceStr = String(edge.source)
    const targetStr = String(edge.target)
    if (!visibleNodeIds.has(sourceStr) || !visibleNodeIds.has(targetStr)) return
    const sourceNodeIndex = nodeIdToIndexMap.get(sourceStr)
    const targetNodeIndex = nodeIdToIndexMap.get(targetStr)
    if (sourceNodeIndex === undefined || targetNodeIndex === undefined) return
    const i3s = sourceNodeIndex * 3
    const i3t = targetNodeIndex * 3
    if (i3s + 2 >= positions.length || i3t + 2 >= positions.length) return
    if (dragNodeId === sourceStr) tempVec3.set(dragPos.x, dragPos.y, dragPos.z)
    else tempVec3.set(positions[i3s], positions[i3s + 1], positions[i3s + 2])
    if (dragNodeId === targetStr) tempPosition.set(dragPos.x, dragPos.y, dragPos.z)
    else tempPosition.set(positions[i3t], positions[i3t + 1], positions[i3t + 2])
    tempDirection.subVectors(tempPosition, tempVec3)
    const edgeLength = tempDirection.length()
    if (edgeLength <= 0.001) return
    tempDirection.normalize()
    const sourceNode = nodes[sourceNodeIndex]
    const targetNode = nodes[targetNodeIndex]
    const srcMode = perNodeVisualModeMap.get(sourceStr) || graphMode
    const tgtMode = perNodeVisualModeMap.get(targetStr) || graphMode
    const srcR = computeNodeScale(sourceNode, connectionCountMap, srcMode, hierarchyMap, graphTypeVisuals) * nodeSize
    const tgtR = computeNodeScale(targetNode, connectionCountMap, tgtMode, hierarchyMap, graphTypeVisuals) * nodeSize
    tempSrcOff.copy(tempVec3).addScaledVector(tempDirection, srcR)
    tempTgtOff.copy(tempPosition).addScaledVector(tempDirection, -tgtR)
    if (tempSrcOff.distanceTo(tempTgtOff) > 0.1) {
      points.push(tempSrcOff.x, tempSrcOff.y, tempSrcOff.z, tempTgtOff.x, tempTgtOff.y, tempTgtOff.z)
      const eColor = getEdgeTypeColor(edge.edgeType)
      colors.push(Math.fround(eColor.r), Math.fround(eColor.g), Math.fround(eColor.b))
      weights.push(Math.fround(edge.weight ?? 1.0))
    }
  })
  const highlight: number[] = []
  if (selectedNodeId) {
    edges.forEach((edge: any) => {
      const sourceStr = String(edge.source)
      const targetStr = String(edge.target)
      if (sourceStr !== selectedNodeId && targetStr !== selectedNodeId) return
      if (!visibleNodeIds.has(sourceStr) || !visibleNodeIds.has(targetStr)) return
      const sourceIdx = nodeIdToIndexMap.get(sourceStr)
      const targetIdx = nodeIdToIndexMap.get(targetStr)
      if (sourceIdx === undefined || targetIdx === undefined) return
      const si3 = sourceIdx * 3
      const ti3 = targetIdx * 3
      if (si3 + 2 >= positions.length || ti3 + 2 >= positions.length) return
      if (dragNodeId === sourceStr) tempVec3.set(dragPos.x, dragPos.y, dragPos.z)
      else tempVec3.set(positions[si3], positions[si3 + 1], positions[si3 + 2])
      if (dragNodeId === targetStr) tempPosition.set(dragPos.x, dragPos.y, dragPos.z)
      else tempPosition.set(positions[ti3], positions[ti3 + 1], positions[ti3 + 2])
      tempDirection.subVectors(tempPosition, tempVec3)
      const len = tempDirection.length()
      if (len <= 0.001) return
      tempDirection.normalize()
      const srcNode = nodes[sourceIdx]
      const tgtNode = nodes[targetIdx]
      const srcMode = perNodeVisualModeMap.get(sourceStr) || graphMode
      const tgtMode = perNodeVisualModeMap.get(targetStr) || graphMode
      const srcR = computeNodeScale(srcNode, connectionCountMap, srcMode, hierarchyMap, graphTypeVisuals) * nodeSize
      const tgtR = computeNodeScale(tgtNode, connectionCountMap, tgtMode, hierarchyMap, graphTypeVisuals) * nodeSize
      tempSrcOff.copy(tempVec3).addScaledVector(tempDirection, srcR)
      tempTgtOff.copy(tempPosition).addScaledVector(tempDirection, -tgtR)
      if (tempSrcOff.distanceTo(tempTgtOff) > 0.2) {
        highlight.push(tempSrcOff.x, tempSrcOff.y, tempSrcOff.z, tempTgtOff.x, tempTgtOff.y, tempTgtOff.z)
      }
    })
  }
  return { points, colors, weights, highlight }
}

function runPlan(
  inp: EdgePlanInputs,
  positions: Float32Array,
  dragNodeId: string | null,
  dragPos: THREE.Vector3,
  selectedNodeId: string | null,
) {
  const plan = buildEdgePlan(inp)
  const buffers = createEdgeFrameBuffers()
  const dragIdx = dragNodeId === null ? -1 : (inp.nodeIdToIndexMap.get(dragNodeId) ?? -1)
  const n = fillEdgeFrame(plan, positions, dragIdx, dragPos, buffers)
  const selIdx = selectedNodeId === null ? -1 : (inp.nodeIdToIndexMap.get(selectedNodeId) ?? -1)
  const hl = selectedNodeId ? fillHighlightFrame(plan, positions, selIdx, dragIdx, dragPos, buffers) : 0
  return {
    points: buffers.points.slice(0, n * 6),
    colors: Array.from(buffers.colors.subarray(0, n * 3)),
    weights: Array.from(buffers.weights.subarray(0, n)),
    highlight: buffers.highlight.slice(0, hl * 6),
  }
}

// --- tests ---------------------------------------------------------------------

describe('edgeBufferPlan equivalence with the original per-frame loop', () => {
  const dragPos = new THREE.Vector3(12.5, -3.25, 40)

  for (const seed of [1, 7, 42]) {
    it(`matches the reference exactly (seed ${seed}, no drag, no selection)`, () => {
      const { inputs, positions } = makeFixture(seed)
      const ref = referenceFrame(inputs, positions, null, dragPos, null)
      const got = runPlan(inputs, positions, null, dragPos, null)
      expect(ref.points.length).toBeGreaterThan(1000)
      expect(got.points).toEqual(ref.points)
      expect(got.colors).toEqual(ref.colors)
      expect(got.weights).toEqual(ref.weights)
      expect(got.highlight).toEqual([])
    })

    it(`matches the reference exactly while dragging and with a selection (seed ${seed})`, () => {
      const { inputs, positions } = makeFixture(seed)
      const ref = referenceFrame(inputs, positions, '17', dragPos, '17')
      const got = runPlan(inputs, positions, '17', dragPos, '17')
      expect(ref.highlight.length).toBeGreaterThan(0)
      expect(got.points).toEqual(ref.points)
      expect(got.colors).toEqual(ref.colors)
      expect(got.weights).toEqual(ref.weights)
      expect(got.highlight).toEqual(ref.highlight)
    })
  }

  it('honours a non-default graph mode, node size and visuals', () => {
    const { inputs, positions } = makeFixture(99)
    const varied: EdgePlanInputs = {
      ...inputs,
      graphMode: 'ontology',
      nodeSize: 1.75,
      graphTypeVisuals: { ontology: { connectionInfluence: 0.4, sizeInfluence: 0.3 }, knowledgeGraph: { globalScaleMultiplier: 1.5 } } as any,
    }
    const ref = referenceFrame(varied, positions, null, dragPos, '3')
    const got = runPlan(varied, positions, null, dragPos, '3')
    expect(got.points).toEqual(ref.points)
    expect(got.highlight).toEqual(ref.highlight)
  })

  it('emits nothing when the selected or dragged id is not in the index map', () => {
    const { inputs, positions } = makeFixture(5)
    const ref = referenceFrame(inputs, positions, 'ghost', dragPos, 'ghost')
    const got = runPlan(inputs, positions, 'ghost', dragPos, 'ghost')
    expect(got.highlight).toEqual(ref.highlight)
    expect(got.highlight).toEqual([])
    expect(got.points).toEqual(ref.points)
  })
})

describe('edgeBufferPlan cost contract', () => {
  it('computes node scale once per distinct endpoint at plan time, never per frame', () => {
    const { inputs, positions } = makeFixture(3)
    const spy = vi.fn(computeNodeScale)
    const plan = buildEdgePlan(inputs, spy)
    const planCalls = spy.mock.calls.length
    // Bounded by distinct visible endpoints (≤ node count), not edge count.
    expect(planCalls).toBeLessThanOrEqual(inputs.nodes.length)
    expect(planCalls).toBeGreaterThan(0)
    const buffers = createEdgeFrameBuffers()
    fillEdgeFrame(plan, positions, -1, new THREE.Vector3(), buffers)
    fillHighlightFrame(plan, positions, 4, -1, new THREE.Vector3(), buffers)
    expect(spy.mock.calls.length).toBe(planCalls)
  })

  it('reuses frame buffers across frames (grow only)', () => {
    const { inputs, positions } = makeFixture(8)
    const plan = buildEdgePlan(inputs)
    const buffers = createEdgeFrameBuffers()
    fillEdgeFrame(plan, positions, -1, new THREE.Vector3(), buffers)
    const { points, colors, weights } = buffers
    fillEdgeFrame(plan, positions, -1, new THREE.Vector3(), buffers)
    expect(buffers.points).toBe(points)
    expect(buffers.colors).toBe(colors)
    expect(buffers.weights).toBe(weights)
  })
})

describe('PositionSnapshot', () => {
  it('reports a change until the same content has been recorded', () => {
    const snap = new PositionSnapshot()
    const p = new Float32Array([1, 2, 3, 4, 5, 6])
    expect(snap.matches(p)).toBe(false)
    snap.record(p)
    expect(snap.matches(p)).toBe(true)
    // In-place mutation (the worker writes the shared buffer in place).
    p[4] = 5.5
    expect(snap.matches(p)).toBe(false)
    snap.record(p)
    expect(snap.matches(p)).toBe(true)
    // Length change is a change.
    expect(snap.matches(new Float32Array([1, 2, 3]))).toBe(false)
  })

  it('treats NaN in the same slot as unchanged', () => {
    const snap = new PositionSnapshot()
    const p = new Float32Array([NaN, 1, 2])
    snap.record(p)
    expect(snap.matches(p)).toBe(true)
  })
})

describe('EdgeFrameGate', () => {
  const plan = { count: 0 } as any
  const handle = {}
  const hlHandle = {}
  const base = () => ({
    plan, positions: new Float32Array([1, 2, 3]), edgeHandle: handle, highlightHandle: hlHandle,
    selectedNodeId: null as string | null, dragging: false,
  })

  it('skips only after an identical frame has been committed', () => {
    const gate = new EdgeFrameGate()
    const f = base()
    expect(gate.shouldSkip(f)).toBe(false)
    gate.commit(f)
    expect(gate.shouldSkip(f)).toBe(true)
  })

  it('recomputes on an in-place position write', () => {
    const gate = new EdgeFrameGate()
    const f = base()
    gate.commit(f)
    f.positions[1] = 2.5
    expect(gate.shouldSkip(f)).toBe(false)
  })

  it('recomputes on a new plan, selection or handle', () => {
    const gate = new EdgeFrameGate()
    const f = base()
    gate.commit(f)
    expect(gate.shouldSkip({ ...f, plan: { count: 0 } as any })).toBe(false)
    expect(gate.shouldSkip({ ...f, selectedNodeId: '4' })).toBe(false)
    expect(gate.shouldSkip({ ...f, edgeHandle: {} })).toBe(false)
    expect(gate.shouldSkip({ ...f, highlightHandle: {} })).toBe(false)
  })

  it('never skips while either handle is unmounted (pending-state path)', () => {
    const gate = new EdgeFrameGate()
    const f = { ...base(), edgeHandle: null }
    gate.commit(f)
    expect(gate.shouldSkip(f)).toBe(false)
    const g = { ...base(), highlightHandle: null }
    gate.commit(g)
    expect(gate.shouldSkip(g)).toBe(false)
  })

  it('recomputes every frame while dragging and once after the drag ends', () => {
    const gate = new EdgeFrameGate()
    const f = { ...base(), dragging: true }
    gate.commit(f)
    expect(gate.shouldSkip(f)).toBe(false)
    const after = { ...f, dragging: false }
    expect(gate.shouldSkip(after)).toBe(false)
    gate.commit(after)
    expect(gate.shouldSkip(after)).toBe(true)
  })
})
