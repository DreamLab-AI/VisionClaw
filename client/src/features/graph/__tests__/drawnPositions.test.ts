/**
 * One drawn-positions array for graph nodes (viz/edge-tracking, owner order: "parallel paths are poison").
 *
 * Every frame the main thread copies the worker's shared buffer once into `DrawnPositions` and applies the
 * drag hand position to the copy once. The gems, the edge fill, labels, picking and fly-to all read that
 * array, so for any node the same frame yields one position. Agents are not graph nodes (ADR-2142 /
 * ADR-2135) and have no slot here; their finished-agent run to the scene edge is pinned on the marker pass
 * (`features/bots/__tests__/realAgentMarkers.test.ts`, `agentMarkerPass.test.ts`), which moved the
 * arrival-at-edge tests this file used to hold (60 and 120 fps, never past the edge, bit-constant).
 */
import { describe, it, expect } from 'vitest'
import * as THREE from 'three'
import { DrawnPositions, fillLabelPositions } from '../utils/drawnPositions'
import {
  buildEdgePlan, fillEdgeFrame, createEdgeFrameBuffers, EdgeFrameGate, type EdgePlanInputs,
} from '../hooks/edgeBufferPlan'
import { resolveNodeWorldPosition } from '../../visualisation/cameraFocus'

const R = 1

/** Nodes 0..3 are graph nodes; edges 0-1, 1-2, 3-2. */
function scene() {
  const ids = ['10', '11', '12', '4194307']
  const nodes = ids.map(id => ({ id, label: id, position: { x: 0, y: 0, z: 0 }, metadata: {} }))
  const edges = [['10', '11'], ['11', '12'], ['4194307', '12']].map(([s, t], i) => ({ id: `e${i}`, source: s, target: t }))
  const map = new Map(ids.map((id, i) => [id, i]))
  const inputs: EdgePlanInputs = {
    nodes: nodes as any, edges: edges as any, nodeIdToIndexMap: map,
    connectionCountMap: new Map(), perNodeVisualModeMap: new Map(), hierarchyMap: new Map(),
    graphMode: 'knowledge_graph', visibleNodeIds: new Set(ids), graphTypeVisuals: undefined, nodeSize: 1,
  }
  const sab = new Float32Array([
    0, 0, 0,
    10, 0, 0,
    20, 0, 0,
    5, 30, 0,
  ])
  return { ids, nodes, map, plan: buildEdgePlan(inputs, () => R), sab }
}

const at = (p: Float32Array, i: number) => new THREE.Vector3(p[i * 3], p[i * 3 + 1], p[i * 3 + 2])

describe('DrawnPositions: one copy of the shared buffer, never the buffer itself', () => {
  it('copies at exactly the shared length and never aliases it', () => {
    const { sab } = scene()
    const dp = new DrawnPositions()
    const drawn = dp.copy(sab)
    expect(drawn).not.toBe(sab)
    expect(drawn.buffer).not.toBe(sab.buffer)
    expect(drawn.length).toBe(sab.length)
    drawn[0] = 99
    expect(sab[0]).toBe(0)
    // the worker rewrites the shared buffer: the next copy follows it, whatever was written to the copy
    sab[0] = 7
    expect(dp.copy(sab)[0]).toBe(7)
  })

  it('reallocates when the shared buffer changes length, so length checks stay honest', () => {
    const dp = new DrawnPositions()
    dp.copy(new Float32Array(12))
    expect(dp.copy(new Float32Array(30)).length).toBe(30)
  })
})

describe('every consumer reads the same drawn position in the same frame', () => {
  it('gem, edge endpoint, label, pick/fly-to and beam target all equal the copy of the shared buffer', () => {
    const { sab, plan, map, ids } = scene()
    const dp = new DrawnPositions()
    const drawn = dp.copy(sab)
    expect(Array.from(drawn)).toEqual(Array.from(sab))
    const bufs = createEdgeFrameBuffers()
    expect(fillEdgeFrame(plan, drawn, bufs)).toBe(3)
    const labels = ids.map(() => ({ x: 0, y: 0, z: 0 }))
    fillLabelPositions(drawn, ids.length, labels)
    for (let i = 0; i < ids.length; i++) {
      expect(labels[i]).toEqual({ x: sab[i * 3], y: sab[i * 3 + 1], z: sab[i * 3 + 2] })
      const wire = resolveNodeWorldPosition(Number(ids[i]), map, drawn)!
      expect([wire.x, wire.y, wire.z]).toEqual(at(sab, i).toArray())
    }
  })
})

describe('the drag override is written into the drawn array once (D2)', () => {
  it('during a drag, the gem, label, inferred-edge input and gem edges of the dragged node all equal the hand in the same frame', () => {
    const { sab, plan, map, ids } = scene()
    const dp = new DrawnPositions()
    const hand = new THREE.Vector3(12.5, -3.25, 40)
    const drawn = dp.copy(sab)
    dp.applyDrag(1, hand)

    // gem: GemNodes reads the drawn array and nothing else
    expect(at(drawn, 1).toArray()).toEqual(hand.toArray())
    // label
    const labels = ids.map(() => ({ x: 0, y: 0, z: 0 }))
    fillLabelPositions(drawn, ids.length, labels)
    expect(labels[1]).toEqual({ x: hand.x, y: hand.y, z: hand.z })
    // inferred edges, picking, fly-to, beams: all read the array by wire id
    const wire = resolveNodeWorldPosition(Number(ids[1]), map, drawn)!
    expect([wire.x, wire.y, wire.z]).toEqual(hand.toArray())
    // gem edges: edge 0 runs 10 -> 11, so it starts on node 10's surface and ends on the hand's surface
    const bufs = createEdgeFrameBuffers()
    expect(fillEdgeFrame(plan, drawn, bufs)).toBe(3)
    const dir = hand.clone().sub(at(drawn, 0)).normalize()
    const end = hand.clone().addScaledVector(dir, -R)
    expect(bufs.points[3]).toBeCloseTo(end.x, 3)
    expect(bufs.points[4]).toBeCloseTo(end.y, 3)
    expect(bufs.points[5]).toBeCloseTo(end.z, 3)
    expect(sab[3]).toBe(10) // the shared buffer is never written
  })

  it('an index outside the array is ignored', () => {
    const { sab } = scene()
    const dp = new DrawnPositions()
    dp.copy(sab)
    dp.applyDrag(-1, new THREE.Vector3(1, 2, 3))
    dp.applyDrag(99, new THREE.Vector3(1, 2, 3))
    expect(Array.from(dp.positions)).toEqual(Array.from(sab))
  })

  it('the edge gate needs no drag flag: a held hand skips, a moving hand does not', () => {
    const { sab, plan } = scene()
    const dp = new DrawnPositions()
    const gate = new EdgeFrameGate()
    const eh = {}
    const hh = {}
    const run = (x: number) => {
      const drawn = dp.copy(sab)
      dp.applyDrag(1, new THREE.Vector3(x, 0, 0))
      const f = { plan, positions: drawn, edgeHandle: eh, highlightHandle: hh, selectedNodeId: null }
      const skip = gate.shouldSkip(f)
      if (!skip) gate.commit(f)
      return skip
    }
    expect(run(10)).toBe(false)
    expect(run(10)).toBe(true)
    expect(run(11)).toBe(false)
    expect(run(11)).toBe(true)
  })
})
