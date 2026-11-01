/**
 * One drawn-positions array (viz/edge-tracking, owner order: "parallel paths are poison").
 *
 * Every frame the main thread copies the worker's shared buffer once into `DrawnPositions`
 * and applies the agent drift and nudge to the copy once. The gems, the edge fill, labels,
 * picking, fly-to and beams all read that array, so for any node the same frame yields one
 * position. These tests drive the real pass, then read it back through the real consumers.
 */
import { describe, it, expect } from 'vitest'
import * as THREE from 'three'
import {
  DrawnPositions, resolveAgentSlots, fillLabelPositions, type AgentPassInput, type AgentSlot,
} from '../utils/drawnPositions'
import {
  buildEdgePlan, fillEdgeFrame, createEdgeFrameBuffers, EdgeFrameGate, type EdgePlanInputs,
} from '../hooks/edgeBufferPlan'
import { resolveNodeWorldPosition } from '../../visualisation/cameraFocus'
import { separatedFrame } from '../triLayout'
import { AgentDriftFeed } from '../../bots/agentDriftFeed'
import { AGENT_NUDGE_SPEED } from '../utils/agentNudge'
import type { AgentWorkInfo } from '../../bots/agentWorkTargets'

const R = 1
const dragPos = new THREE.Vector3()

/** Nodes 0..2 are knowledge nodes, node 3 an agent; edges 0-1, 1-2, 3-2. */
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

const work = (state: 'working' | 'done', target = '12'): AgentWorkInfo => ({ targetNodeId: target, state })
function pass(over: Partial<AgentPassInput> & { sab: Float32Array; agents: AgentSlot[] }): AgentPassInput {
  return {
    getWork: () => null, drift: null, triFrame: separatedFrame(), offsetsEnabled: true, dragIndex: -1,
    nodeIdToIndex: new Map([['12', 2]]), ...over,
  }
}
/** One frame as GraphManager runs it: copy the shared buffer, then apply the agents. */
function frame(dp: DrawnPositions, input: AgentPassInput): Float32Array {
  dp.copy(input.sab)
  dp.applyAgents(input)
  return dp.positions
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

describe('a drifted agent is at ONE position for every consumer in the same frame', () => {
  it('gem, edge endpoint, label, pick/fly-to and beam source all equal the drawn position', () => {
    const { sab, plan, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    let drawn = sab
    for (let f = 0; f < 90; f++) {
      drawn = frame(dp, pass({ sab, agents, getWork: () => work('working') }))
    }
    // The agent has been nudged toward its target (node 2 at x=20): it is NOT at its server position.
    const gem = at(drawn, 3)
    expect(gem.distanceTo(at(sab, 3))).toBeGreaterThan(5)
    expect(sab[9]).toBe(5) // the shared buffer was never written

    // edge fill: the edge 3-2 ends on the agent's surface, along the line to its drawn centre
    const bufs = createEdgeFrameBuffers()
    const n = fillEdgeFrame(plan, drawn, -1, dragPos, bufs)
    expect(n).toBe(3)
    const dir = new THREE.Vector3().subVectors(at(drawn, 2), gem).normalize()
    const expected = gem.clone().addScaledVector(dir, R)
    const e = 2 // third edge: source = agent
    expect(bufs.points[e * 6]).toBeCloseTo(expected.x, 3)
    expect(bufs.points[e * 6 + 1]).toBeCloseTo(expected.y, 3)
    expect(bufs.points[e * 6 + 2]).toBeCloseTo(expected.z, 3)

    // labels (the array GraphManager hands InstancedLabels as its fallback)
    const labels = ids.map(() => ({ x: 0, y: 0, z: 0 }))
    fillLabelPositions(drawn, ids.length, labels)
    expect(labels[3]).toEqual({ x: gem.x, y: gem.y, z: gem.z })

    // pick / fly-to / beam source / hull and ring input: all read the array by wire id
    const wire = resolveNodeWorldPosition(Number(ids[3]), map, drawn)!
    expect(new THREE.Vector3(wire.x, wire.y, wire.z).distanceTo(gem)).toBe(0)
  })

  it('knowledge nodes are drawn exactly at their server positions', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    const drawn = frame(dp, pass({ sab, agents, getWork: () => work('working') }))
    for (let i = 0; i < 3; i++) expect(at(drawn, i).toArray()).toEqual(at(sab, i).toArray())
  })

  it('a working agent converges on its target at the nudge speed', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    const first = frame(dp, pass({ sab, agents, getWork: () => work('working') }))
    expect(first[9] - sab[9]).toBeCloseTo((20 - 5) * AGENT_NUDGE_SPEED, 4)
  })

  it('offsets off: the agent is drawn at the server position plus its activity drift only', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const drift = { step() {}, offset: () => [1, 2, 3] as const }
    const dp = new DrawnPositions()
    const drawn = frame(dp, pass({ sab, agents, drift, offsetsEnabled: false, getWork: () => work('working') }))
    expect(at(drawn, 3).toArray()).toEqual([6, 32, 3])
  })

  it('steps the drift feed once per frame, and only when an agent is drawn', () => {
    const { sab, map, ids } = scene()
    let steps = 0
    const drift = { step() { steps++ }, offset: () => [0, 0, 0] as const }
    const dp = new DrawnPositions()
    frame(dp, pass({ sab, agents: resolveAgentSlots([{ id: ids[3] }], map), drift }))
    expect(steps).toBe(1)
    frame(dp, pass({ sab, agents: [], drift }))
    expect(steps).toBe(1)
  })

  it('the dragged agent is left at the server position (the gem takes the hand position)', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    const drawn = frame(dp, pass({ sab, agents, dragIndex: 3, getWork: () => work('working') }))
    expect(at(drawn, 3).toArray()).toEqual(at(sab, 3).toArray())
  })
})

describe('a finished agent stops moving (owner decision)', () => {
  it('a done agent that never worked is at its server position and constant, and the edge gate skips', () => {
    const { sab, map, ids, plan } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    const gate = new EdgeFrameGate()
    const handle = {}
    let last = new Float32Array(0)
    let skipped = 0
    for (let f = 0; f < 200; f++) {
      const drawn = frame(dp, pass({ sab, agents, getWork: () => work('done') }))
      if (f > 0) expect(Array.from(drawn)).toEqual(Array.from(last))
      last = Float32Array.from(drawn)
      const state = { plan, positions: drawn, edgeHandle: handle, highlightHandle: handle, selectedNodeId: null, dragging: false }
      if (gate.shouldSkip(state)) skipped++
      else gate.commit(state)
    }
    expect(skipped).toBe(199)
  })

  it('a working agent that finishes glides home and then holds still bit-for-bit; the gate then skips', () => {
    const { sab, map, ids, plan } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    for (let f = 0; f < 120; f++) frame(dp, pass({ sab, agents, getWork: () => work('working') }))
    const away = at(dp.positions, 3).distanceTo(at(sab, 3))
    expect(away).toBeGreaterThan(10)

    const gate = new EdgeFrameGate()
    const handle = {}
    let prevDist = Infinity
    let last = new Float32Array(0)
    let settledAt = -1
    let skippedAfterSettle = 0
    for (let f = 0; f < 3000; f++) {
      const drawn = frame(dp, pass({ sab, agents, getWork: () => work('done') }))
      const dist = at(drawn, 3).distanceTo(at(sab, 3))
      expect(dist).toBeLessThanOrEqual(prevDist) // monotone: never drifts outward
      prevDist = dist
      const same = last.length === drawn.length && last.every((x, k) => x === drawn[k])
      if (same && settledAt < 0) settledAt = f
      if (!same) settledAt = -1
      last = Float32Array.from(drawn)
      const state = { plan, positions: drawn, edgeHandle: handle, highlightHandle: handle, selectedNodeId: null, dragging: false }
      if (gate.shouldSkip(state)) { if (settledAt >= 0) skippedAfterSettle++ } else gate.commit(state)
    }
    expect(settledAt).toBeGreaterThan(0)
    expect(settledAt).toBeLessThan(600) // ~7 s at 60 fps once the offset snaps (AGENT_RELAX_SNAP), not a minute
    expect(at(dp.positions, 3).toArray()).toEqual(at(sab, 3).toArray())
    expect(skippedAfterSettle).toBeGreaterThan(1000)
  })
})

/**
 * The settle tail in the PRODUCTION configuration: the real drift feed (credit halves every 6 s, the
 * shown offset follows through a 1.5 s lag), time injected at 60 fps, and an agent at the origin,
 * where a float32 coordinate has the finest resolution and a geometric tail lasts longest.
 */
function settleFrames(agentAt: number[], withFeed: boolean, maxFrames: number): number {
  const sab = new Float32Array([0, 0, 0, ...agentAt, 40, 25, -10])
  const agents: AgentSlot[] = [{ id: 'A', index: 1 }]
  const nodeIdToIndex = new Map([['T', 2]])
  let now = 100
  const feed = new AgentDriftFeed({
    lookupNode: () => ({ metadata: { type: 'page' } }),
    agentKeys: () => ['A'],
    nowS: () => now,
  })
  if (withFeed) feed.ingestBeams([{ id: 1, sourceAgentId: 'A' as unknown as number, targetNodeId: 'T' as unknown as number }])
  const dp = new DrawnPositions()
  let state: 'working' | 'done' = 'working'
  const run = (): Float32Array => {
    now += 1 / 60
    dp.copy(sab)
    dp.applyAgents(pass({ sab, agents, nodeIdToIndex, drift: withFeed ? feed : null, getWork: () => work(state, 'T') }))
    return Float32Array.from(dp.positions.subarray(3, 6))
  }
  for (let f = 0; f < 120; f++) run()
  state = 'done'
  let last = run()
  let stableSince = 0
  for (let f = 1; f < maxFrames; f++) {
    const cur = run()
    const same = cur[0] === last[0] && cur[1] === last[1] && cur[2] === last[2]
    if (!same) stableSince = 0
    else if (stableSince === 0) stableSince = f
    last = cur
  }
  return stableSince
}

describe('how long a finished agent keeps moving (measured, production configuration)', () => {
  it('the nudge tail ends in about 7 s even at the origin, where float32 is finest', () => {
    for (const at of [[5, 30, 0], [0, 0, 0]]) {
      const n = settleFrames(at, false, 3000)
      expect(n).toBeGreaterThan(0)
      expect(n).toBeLessThan(600)
    }
  })

  it('with the real drift feed the agent holds still after the credit has decayed (about 58 s at 60 fps)', () => {
    for (const at of [[5, 30, 0], [0, 0, 0]]) {
      const n = settleFrames(at, true, 8000)
      expect(n).toBeGreaterThan(0) // it does settle
      expect(n).toBeLessThan(5000) // measured 3468 frames: DriftField credit halves every 6 s
    }
  })
})

describe('agent offsets are keyed by agent and do not outlive it', () => {
  it('an agent that leaves frees its offset: the node that reuses its slot is drawn at its raw position', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    for (let f = 0; f < 60; f++) frame(dp, pass({ sab, agents, getWork: () => work('working') }))
    expect(at(dp.positions, 3).distanceTo(at(sab, 3))).toBeGreaterThan(1)
    // the swarm empties (mesh unmounted): nothing applies, and the slot is raw again at once
    expect(at(frame(dp, pass({ sab, agents: [] })), 3).toArray()).toEqual(at(sab, 3).toArray())
    // a different agent takes the same slot: it starts from zero, not from the dead agent's offset
    const other = [{ id: '4194999', index: 3 }]
    const drawn = frame(dp, pass({ sab, agents: other, getWork: () => work('done') }))
    expect(at(drawn, 3).toArray()).toEqual(at(sab, 3).toArray())
  })

  it('reordering the agent list does not hand one agent the offset of another', () => {
    const sab = new Float32Array([0, 0, 0, 10, 0, 0, 100, 0, 0])
    const a: AgentSlot = { id: 'a', index: 1 }
    const b: AgentSlot = { id: 'b', index: 2 }
    // target T is node 0 at the origin: only agent a is working, so only a moves
    const getWork = (id: string): AgentWorkInfo | null => (id === 'a' ? work('working', 'T') : null)
    const nodeIdToIndex = new Map([['T', 0]])
    const dp = new DrawnPositions()
    for (let f = 0; f < 30; f++) frame(dp, pass({ sab, agents: [a, b], getWork, nodeIdToIndex }))
    expect(at(dp.positions, 1).x).toBeLessThan(10)
    const b0 = at(dp.positions, 2).toArray()
    expect(b0).toEqual([100, 0, 0])
    for (let f = 0; f < 5; f++) frame(dp, pass({ sab, agents: [b, a], getWork, nodeIdToIndex }))
    expect(at(dp.positions, 2).toArray()).toEqual(b0)
  })

  it('an agent with no slot in the position buffer is not drawn at, or offset into, another node', () => {
    const { map } = scene()
    const slots = resolveAgentSlots([{ id: '4194307' }, { id: '4199999' }], map)
    expect(slots).toEqual([{ id: '4194307', index: 3 }])
  })
})
