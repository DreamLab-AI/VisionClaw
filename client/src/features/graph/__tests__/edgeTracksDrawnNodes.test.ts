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
import {
  buildEdgePlan, fillEdgeFrame, fillHighlightFrame, createEdgeFrameBuffers, EdgeFrameGate,
  type EdgePlanInputs,
} from '../hooks/edgeBufferPlan'
import { DrawnOffsets } from '../utils/drawnOffsets'
import { resolveDrawnNodePosition } from '../../visualisation/cameraFocus'

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

const dragPos = new THREE.Vector3()

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
    expect(fillEdgeFrame(plan, cur, -1, dragPos, out)).toBe(2)
    close(out.points.slice(0, 6), surface(v(cur, 0), v(cur, 1)))
    close(out.points.slice(6, 12), surface(v(cur, 1), v(cur, 2)))
  })

  it('a position write by the stream (a remote drag) is picked up by the next frame', () => {
    const { plan } = chain(3)
    const pos = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const gate = new EdgeFrameGate()
    const h = {}, hh = {}
    const state = () => ({ plan, positions: pos, edgeHandle: h, highlightHandle: hh, selectedNodeId: null, dragging: false })
    gate.commit(state())
    expect(gate.shouldSkip(state())).toBe(true)
    pos.set([20, 35, 5], 3) // another client drags node 1
    expect(gate.shouldSkip(state())).toBe(false)
    const out = createEdgeFrameBuffers()
    fillEdgeFrame(plan, pos, -1, dragPos, out)
    close(out.points.slice(0, 6), surface(v(pos, 0), v(pos, 1)))
  })

  it('a local drag draws edges at the hand position, including the highlight', () => {
    const { plan } = chain(3)
    const pos = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const hand = new THREE.Vector3(18, 9, -4)
    const out = createEdgeFrameBuffers()
    fillEdgeFrame(plan, pos, 1, hand, out)
    close(out.points.slice(0, 6), surface(v(pos, 0), hand))
    close(out.points.slice(6, 12), surface(hand, v(pos, 2)))
    expect(fillHighlightFrame(plan, pos, 1, 1, hand, out)).toBe(2)
    close(out.highlight.slice(0, 6), surface(v(pos, 0), hand))
  })
})

describe('agent nodes are drawn at raw position + client offset; edges must use the same', () => {
  it('fillEdgeFrame and fillHighlightFrame add the drawn offset to both endpoints', () => {
    const { plan } = chain(3)
    const pos = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const off = new DrawnOffsets()
    off.set(1, 0, 6, 2) // node 1 hovers 6 up, 2 forward of its server position
    const drawn = new Float32Array([0, 0, 0, 20, 6, 2, 40, 0, 0])
    const out = createEdgeFrameBuffers()
    fillEdgeFrame(plan, pos, -1, dragPos, out, off.data)
    close(out.points.slice(0, 6), surface(v(drawn, 0), v(drawn, 1)))
    close(out.points.slice(6, 12), surface(v(drawn, 1), v(drawn, 2)))
    expect(fillHighlightFrame(plan, pos, 1, -1, dragPos, out, off.data)).toBe(2)
    close(out.highlight.slice(0, 6), surface(v(drawn, 0), v(drawn, 1)))
  })

  it('a changed offset defeats the skip gate even when the raw positions are bit-identical', () => {
    const { plan } = chain(3)
    const pos = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const off = new DrawnOffsets()
    const gate = new EdgeFrameGate()
    const h = {}, hh = {}
    const state = () => ({ plan, positions: pos, edgeHandle: h, highlightHandle: hh, selectedNodeId: null, dragging: false, offsetsVersion: off.version })
    gate.commit(state())
    expect(gate.shouldSkip(state())).toBe(true)
    off.set(1, 0, 0.5, 0)
    expect(gate.shouldSkip(state())).toBe(false)
  })

  it('DrawnOffsets bumps its version only on a real change and grows on demand', () => {
    const off = new DrawnOffsets()
    const v0 = off.version
    off.set(4, 1, 2, 3)
    expect(off.data.length).toBeGreaterThanOrEqual(15)
    const v1 = off.version
    expect(v1).toBeGreaterThan(v0)
    off.set(4, 1, 2, 3)
    expect(off.version).toBe(v1)
    off.set(4, 1, 2, 3.5)
    expect(off.version).toBeGreaterThan(v1)
  })
})

describe('draw offsets do not outlive the agent that owned them', () => {
  it('a slot not written this pass returns to its raw position, and the gate sees it', () => {
    const { plan } = chain(3)
    const pos = new Float32Array([0, 0, 0, 20, 0, 0, 40, 0, 0])
    const off = new DrawnOffsets()
    off.beginFrame(); off.set(1, 0, 6, 2); off.endFrame()
    const before = off.version
    off.beginFrame(); off.endFrame() // the agent left slot 1 (filtered out, reindexed, replaced by a KG node)
    expect(off.version).toBeGreaterThan(before)
    expect(Array.from(off.data.slice(3, 6))).toEqual([0, 0, 0])
    const out = createEdgeFrameBuffers()
    fillEdgeFrame(plan, pos, -1, dragPos, out, off.data)
    close(out.points.slice(0, 6), surface(v(pos, 0), v(pos, 1)))
  })

  it('a slot written in consecutive passes keeps its offset and does not bump the version', () => {
    const off = new DrawnOffsets()
    off.beginFrame(); off.set(2, 1, 1, 1); off.endFrame()
    const ver = off.version
    off.beginFrame(); off.set(2, 1, 1, 1); off.endFrame()
    expect(off.version).toBe(ver)
    expect(Array.from(off.data.slice(6, 9))).toEqual([1, 1, 1])
  })
})

describe('desktop beam source meets the drawn agent', () => {
  it('resolveDrawnNodePosition returns SAB position plus the draw offset, masked ids included', () => {
    const map = new Map([['7', 1]])
    const pos = new Float32Array([0, 0, 0, 10, 20, 30])
    const off = new DrawnOffsets()
    off.set(1, 1, 2, 3)
    expect(resolveDrawnNodePosition(7, map, pos, off.data)).toEqual({ x: 11, y: 22, z: 33 })
    expect(resolveDrawnNodePosition(99, map, pos, off.data)).toBeNull()
  })
})
