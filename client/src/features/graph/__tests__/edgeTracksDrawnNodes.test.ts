/**
 * Edges follow each node's DRAWN position (viz/edge-tracking).
 *
 * Owner rule: the client's own chase (worker tween) decides where a node is drawn
 * between server frames, so an edge endpoint must be that same chased position, in the
 * same frame, never the raw server target. These tests drive the real worker tween and
 * the real edge fill and decode the endpoints back.
 */
import { describe, it, expect } from 'vitest'
import * as THREE from 'three'
import { tickTween } from '../workers/lib/tween'
import { DrawnPositions } from '../utils/drawnPositions'
import {
  buildEdgePlan, fillEdgeFrame, fillHighlightFrame, createEdgeFrameBuffers, EdgeFrameGate,
  type EdgePlanInputs,
} from '../hooks/edgeBufferPlan'

/** Radius 1 at every node, so an endpoint sits exactly 1 unit from its node centre. */
const R = 1
function chain(n: number) {
  const nodes = Array.from({ length: n }, (_, i) => ({ id: String(i), label: `n${i}`, position: { x: 0, y: 0, z: 0 }, metadata: {} }))
  const edges = Array.from({ length: n - 1 }, (_, i) => ({ id: `e${i}`, source: String(i), target: String(i + 1) }))
  const inputs: EdgePlanInputs = {
    nodes: nodes as any, edges: edges as any,
    nodeIdToIndexMap: new Map(nodes.map((nd, i) => [nd.id, i])),
    connectionCountMap: new Map(), perNodeVisualModeMap: new Map(), hierarchyMap: new Map(),
    graphMode: 'knowledge_graph', visibleNodeIds: new Set(nodes.map(nd => nd.id)),
    graphTypeVisuals: undefined, nodeSize: 1,
  }
  return { nodes, inputs, plan: buildEdgePlan(inputs, () => R) }
}

/** Expected surface endpoints of an edge between two drawn centres. */
function surface(a: THREE.Vector3, b: THREE.Vector3): number[] {
  const d = new THREE.Vector3().subVectors(b, a).normalize()
  const s = a.clone().addScaledVector(d, R)
  const t = b.clone().addScaledVector(d, -R)
  return [s.x, s.y, s.z, t.x, t.y, t.z]
}
const close = (got: ArrayLike<number>, want: number[]) => {
  for (let k = 0; k < want.length; k++) expect(got[k]).toBeCloseTo(want[k], 3)
}
const v = (p: Float32Array | number[], i: number) => new THREE.Vector3(p[i * 3], p[i * 3 + 1], p[i * 3 + 2])

describe('desktop edges meet the drawn (chased) node positions', () => {
  it('worker tween: mid-ease edges end at the eased positions, not the server targets', () => {
    const { nodes, plan } = chain(3)
    const cur = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const tgt = new Float32Array([0, 0, 0, 20, 30, 0, 40, 0, 0]) // server moved node 1
    tickTween({
      curPos: cur, tgtPos: tgt, vel: new Float32Array(9), nodeCount: 3, pinnedNodeIds: new Set(),
      nodeIdMap: new Map(), nodeIds: nodes.map(n => n.id),
      tweenSettings: { enabled: true, responseMs: 100, snapThreshold: 0.01, maxDivergence: 1000 }, deltaTime: 0.016,
    })
    expect(cur[4]).toBeGreaterThan(0.5)
    expect(cur[4]).toBeLessThan(29)
    const out = createEdgeFrameBuffers()
    expect(fillEdgeFrame(plan, cur, out)).toBe(2)
    close(out.points.slice(0, 6), surface(v(cur, 0), v(cur, 1)))
    close(out.points.slice(6, 12), surface(v(cur, 1), v(cur, 2)))
  })

  it('a position write by the stream (a remote drag) is picked up by the next frame', () => {
    const { plan } = chain(3)
    const pos = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const gate = new EdgeFrameGate()
    const h = {}, hh = {}
    const state = () => ({ plan, positions: pos, edgeHandle: h, highlightHandle: hh, selectedNodeId: null })
    gate.commit(state())
    expect(gate.shouldSkip(state())).toBe(true)
    pos.set([20, 35, 5], 3) // another client drags node 1
    expect(gate.shouldSkip(state())).toBe(false)
    const out = createEdgeFrameBuffers()
    fillEdgeFrame(plan, pos, out)
    close(out.points.slice(0, 6), surface(v(pos, 0), v(pos, 1)))
  })

  it('a local drag draws edges at the hand position, including the highlight', () => {
    const { plan } = chain(3)
    const pos = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const hand = new THREE.Vector3(18, 9, -4)
    const out = createEdgeFrameBuffers()
    // GraphManager writes the hand into the drawn array; the edge fill just reads it.
    const dp = new DrawnPositions()
    const drawn = dp.copy(pos)
    dp.applyDrag(1, hand)
    fillEdgeFrame(plan, drawn, out)
    close(out.points.slice(0, 6), surface(v(pos, 0), hand))
    close(out.points.slice(6, 12), surface(hand, v(pos, 2)))
    expect(fillHighlightFrame(plan, drawn, 1, out)).toBe(2)
    close(out.highlight.slice(0, 6), surface(v(pos, 0), hand))
  })
})
