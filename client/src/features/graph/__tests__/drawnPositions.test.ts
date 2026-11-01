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
import { agentDrawnPosition } from '../../bots/agentDrawnPosition'
import type { AgentWorkInfo } from '../../bots/agentWorkTargets'

const R = 1

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
/** An agent pass plus the shared buffer it is copied from (the pass itself never sees the shared buffer). */
type Frame = AgentPassInput & { sab: Float32Array }
function pass(over: Partial<AgentPassInput> & { sab: Float32Array; agents: AgentSlot[] }): Frame {
  return {
    getWork: () => null, drift: null, triFrame: separatedFrame(), offsetsEnabled: true, dragIndex: -1,
    nodeIdToIndex: new Map([['12', 2]]), edge: null, nowMs: 0, ...over,
  }
}
/** One frame as GraphManager runs it: copy the shared buffer, then apply the agents. */
function frame(dp: DrawnPositions, input: Frame): Float32Array {
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
    const n = fillEdgeFrame(plan, drawn, bufs)
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

/** Scene edge for the finished-agent tests: a sphere about the origin, well outside the agent (30.4 out). */
const EDGE = { centre: [0, 0, 0] as [number, number, number], radius: 100 }
const distFromCentre = (p: Float32Array, i: number) =>
  Math.hypot(p[i * 3] - EDGE.centre[0], p[i * 3 + 1] - EDGE.centre[1], p[i * 3 + 2] - EDGE.centre[2])

describe('a finished agent reaches the scene edge in about 30 s and stops there (owner decision)', () => {
  for (const fps of [60, 120]) {
    it(`at ${fps} fps: arrives on time, never passes the edge, then is bit-constant, and the edge gate skips`, () => {
      const { sab, map, ids, plan } = scene()
      const agents = resolveAgentSlots([{ id: ids[3] }], map)
      const dp = new DrawnPositions()
      const gate = new EdgeFrameGate()
      const handle = {}
      let prevDist = distFromCentre(sab, 3)
      let last = new Float32Array(0)
      let arrivedAt = -1 // seconds from finish, when the agent is on the edge
      let skippedAfterArrival = 0
      let framesAfterArrival = 0
      const frames = fps * 36
      for (let f = 0; f <= frames; f++) {
        const nowMs = 1000 + (f * 1000) / fps
        const drawn = frame(dp, pass({ sab, agents, edge: EDGE, nowMs, getWork: () => work('done') }))
        const d = distFromCentre(drawn, 3)
        expect(d).toBeGreaterThanOrEqual(prevDist - 1e-4) // outward only
        expect(d).toBeLessThanOrEqual(EDGE.radius + 1e-4) // never past the edge
        if (f === fps * 20) expect(EDGE.radius - d).toBeGreaterThan(1) // not there yet at 20 s
        prevDist = d
        if (arrivedAt < 0 && Math.abs(d - EDGE.radius) < 2e-4) arrivedAt = f / fps
        const same = last.length === drawn.length && last.every((x, k) => x === drawn[k])
        last = Float32Array.from(drawn)
        const state = { plan, positions: drawn, edgeHandle: handle, highlightHandle: handle, selectedNodeId: null, dragging: false }
        const skip = gate.shouldSkip(state)
        if (!skip) gate.commit(state)
        if (arrivedAt >= 0 && f / fps > 30.2) { framesAfterArrival++; if (skip) skippedAfterArrival++; expect(same).toBe(true) }
      }
      expect(arrivedAt).toBeGreaterThan(29.0) // eased, so the last metre takes a while, but not early
      expect(arrivedAt).toBeLessThan(30.2) // 30 s, within a frame or two and the snap
      expect(framesAfterArrival).toBeGreaterThan(fps * 5)
      expect(skippedAfterArrival).toBe(framesAfterArrival) // every frame after arrival is skipped
      expect(sab[10]).toBe(30) // the shared buffer was never written
    })
  }

  it('with two agents finishing 10 s apart, the edge gate never skips while the second is still moving and skips every frame once both have arrived', () => {
    const ids = ['10', '11', '12', '4194307', '4194308']
    const nodes = ids.map(id => ({ id, label: id, position: { x: 0, y: 0, z: 0 }, metadata: {} }))
    const edges = [['10', '11'], ['11', '12'], ['4194307', '12'], ['4194308', '11']].map(([s, t], i) => ({ id: `e${i}`, source: s, target: t }))
    const map = new Map(ids.map((id, i) => [id, i]))
    const plan = buildEdgePlan({
      nodes: nodes as any, edges: edges as any, nodeIdToIndexMap: map,
      connectionCountMap: new Map(), perNodeVisualModeMap: new Map(), hierarchyMap: new Map(),
      graphMode: 'knowledge_graph', visibleNodeIds: new Set(ids), graphTypeVisuals: undefined, nodeSize: 1,
    }, () => R)
    const sab = new Float32Array([0, 0, 0, 10, 0, 0, 20, 0, 0, 5, 30, 0, -5, -30, 0])
    const agents = resolveAgentSlots([{ id: ids[3] }, { id: ids[4] }], map)
    const dp = new DrawnPositions()
    const gate = new EdgeFrameGate()
    const handle = {}
    const fps = 60
    const secondFinishes = 10 // s; the first agent is finished from the start
    let last = new Float32Array(0)
    let movedInWindow = 0
    let skippedWhileMoving = 0
    let framesAfterBoth = 0
    let skippedAfterBoth = 0
    for (let f = 0; f <= fps * 45; f++) {
      const t = f / fps
      const getWork = (id: string): AgentWorkInfo | null =>
        id === ids[3] ? work('done') : work(t < secondFinishes ? 'working' : 'done')
      const drawn = frame(dp, pass({ sab, agents, edge: EDGE, nowMs: 1000 + t * 1000, getWork }))
      const same = last.length === drawn.length && last.every((x, k) => x === drawn[k])
      last = Float32Array.from(drawn)
      const state = { plan, positions: drawn, edgeHandle: handle, highlightHandle: handle, selectedNodeId: null, dragging: false }
      const skip = gate.shouldSkip(state)
      if (!skip) gate.commit(state)
      if (skip) expect(same).toBe(true) // the gate never skips a frame in which something moved
      // the first agent arrived at 30 s, the second is still on its way until 40 s
      if (t > 31 && t < 39) { if (!same) movedInWindow++; if (skip && !same) skippedWhileMoving++ }
      if (t > 40.5) { framesAfterBoth++; if (skip) skippedAfterBoth++ }
    }
    expect(movedInWindow).toBeGreaterThan(fps * 5) // the second agent really was still moving
    expect(skippedWhileMoving).toBe(0)
    expect(framesAfterBoth).toBeGreaterThan(fps * 4)
    expect(skippedAfterBoth).toBe(framesAfterBoth)
  })

  it('an agent that is not done is not sent to the edge, and one with no work relaxes home', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    for (let f = 0; f < 200; f++) frame(dp, pass({ sab, agents, edge: EDGE, getWork: () => null }))
    expect(at(dp.positions, 3).toArray()).toEqual(at(sab, 3).toArray())
  })

  it('with no known edge a finished agent stays at its server position', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    for (let f = 0; f < 50; f++) frame(dp, pass({ sab, agents, edge: null, getWork: () => work('done') }))
    expect(at(dp.positions, 3).toArray()).toEqual(at(sab, 3).toArray())
  })
})

/**
 * Arrival at the edge in the PRODUCTION configuration: the real drift feed (credit halves every 6 s,
 * 1.5 s lag), time injected at 60 fps. The agent works, then finishes and drifts to the edge.
 */
function arrivalFrames(withFeed: boolean, maxFrames: number): number {
  const sab = new Float32Array([0, 0, 0, 5, 30, 0, 40, 25, -10])
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
  const edge = { centre: [0, 0, 0] as [number, number, number], radius: 60 }
  const run = (): Float32Array => {
    now += 1 / 60
    dp.copy(sab)
    dp.applyAgents(pass({ sab, agents, nodeIdToIndex, edge, nowMs: now * 1000, drift: withFeed ? feed : null, getWork: () => work(state, 'T') }))
    return Float32Array.from(dp.positions.subarray(3, 6))
  }
  for (let f = 0; f < 120; f++) run()
  state = 'done'
  let last = run()
  let stableSince = 0
  for (let f = 1; f < maxFrames; f++) {
    const cur = run()
    expect(Math.hypot(cur[0], cur[1], cur[2])).toBeLessThanOrEqual(edge.radius + 1e-3)
    const same = cur[0] === last[0] && cur[1] === last[1] && cur[2] === last[2]
    if (!same) stableSince = 0
    else if (stableSince === 0) stableSince = f
    last = cur
  }
  return stableSince
}

describe('arrival at the edge, production configuration (measured)', () => {
  it('without the drift feed the agent arrives about 30 s (1 800 frames at 60 fps) after it finishes and holds', () => {
    const n = arrivalFrames(false, 4000)
    expect(n).toBeGreaterThan(1700)
    expect(n).toBeLessThan(1850)
  })

  it('with the real drift feed it still never passes the edge and holds once the credit has decayed', () => {
    const n = arrivalFrames(true, 60000)
    expect(n).toBeGreaterThan(0)
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

describe('a layout transition moves the agents with everything else (D3)', () => {
  it('a working agent starts from the drawn base and heads for the drawn target, not the raw shared buffer', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    const drawn = dp.copy(sab)
    // The layout LERP has moved the target node (slot 2) and the agent's own slot in the copy only.
    drawn[6] = 100
    drawn[9] = 50
    dp.applyAgents(pass({ sab, agents, getWork: () => work('working') }))
    expect(dp.positions[9]).toBeCloseTo(50 + (100 - 50) * AGENT_NUDGE_SPEED, 4)
    expect(sab[9]).toBe(5) // the shared buffer stays untouched
  })
})

describe('the drag override is written into the drawn array once (D2)', () => {
  it('during a drag, the gem, label, inferred-edge input and gem edges of the dragged node all equal the hand in the same frame', () => {
    const { sab, plan, map, ids } = scene()
    const dp = new DrawnPositions()
    const hand = new THREE.Vector3(12.5, -3.25, 40)
    const drawn = dp.copy(sab)
    dp.applyAgents(pass({ sab, agents: [], dragIndex: 1 }))
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

  it('the dragged agent is placed at the hand after the agent pass, which leaves it alone', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    const hand = new THREE.Vector3(-4, 8, 2)
    dp.copy(sab)
    dp.applyAgents(pass({ sab, agents, dragIndex: 3, getWork: () => work('working') }))
    dp.applyDrag(3, hand)
    expect(at(dp.positions, 3).toArray()).toEqual(hand.toArray())
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

describe('a finished agent does not teleport when offsets come back on (D5)', () => {
  it('turning offsets off for longer than the run, then on again, restarts the 30 s run instead of jumping to the edge', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    const getWork = () => work('done')
    frame(dp, pass({ sab, agents, edge: EDGE, nowMs: 1000, getWork }))
    for (let t = 2; t < 40; t++) frame(dp, pass({ sab, agents, edge: EDGE, nowMs: t * 1000, offsetsEnabled: false, getWork }))
    expect(at(dp.positions, 3).toArray()).toEqual(at(sab, 3).toArray())
    const before = distFromCentre(sab, 3)
    const drawn = frame(dp, pass({ sab, agents, edge: EDGE, nowMs: 40_000, getWork }))
    expect(distFromCentre(drawn, 3)).toBeCloseTo(before, 3)
  })
})

describe('the agent overlay is drawn where the gem is (D1)', () => {
  const out = new THREE.Vector3()

  it('a finished agent\'s membrane, nameplate, trail, collaboration edges and payment labels sit at its gem, 30 s into the run to the edge', () => {
    const { sab, map, ids } = scene()
    const agents = resolveAgentSlots([{ id: ids[3] }], map)
    const dp = new DrawnPositions()
    let drawn = sab
    for (let t = 0; t <= 35; t++) {
      drawn = frame(dp, pass({ sab, agents, edge: EDGE, nowMs: 1000 + t * 1000, getWork: () => work('done') }))
    }
    const gem = at(drawn, 3)
    expect(gem.distanceTo(at(sab, 3))).toBeGreaterThan(50) // the gem is on the scene edge, not at the server position
    // every overlay consumer asks the one resolver, by the agent's string id
    expect(agentDrawnPosition(ids[3], out, map, drawn)).toBe(true)
    expect(out.toArray()).toEqual(gem.toArray())
    // the server position is never what the overlay gets
    expect(out.distanceTo(at(sab, 3))).toBeGreaterThan(50)
  })

  it('resolves a wire id, and hides an agent with no slot or a non-numeric id', () => {
    const { sab, map } = scene()
    const dp = new DrawnPositions()
    const drawn = dp.copy(sab)
    expect(agentDrawnPosition('4194307', out, map, drawn)).toBe(true)
    expect(out.toArray()).toEqual([5, 30, 0])
    out.set(9, 9, 9)
    expect(agentDrawnPosition('777', out, map, drawn)).toBe(false)
    expect(agentDrawnPosition('not-a-number', out, map, drawn)).toBe(false)
    expect(agentDrawnPosition('4194307', out, map, null)).toBe(false)
    expect(out.toArray()).toEqual([9, 9, 9]) // an unresolved agent leaves the output alone
  })

  it('finds the slot whichever id form the overlay holds: bare (botsData, graph node ids) or with the AGENT flag bit (wire)', () => {
    const { sab, map } = scene()
    const drawn = new DrawnPositions().copy(sab)
    const AGENT_FLAG = 0x80000000
    for (const form of ['4194307', String((AGENT_FLAG | 4194307) >>> 0)]) {
      out.set(0, 0, 0)
      expect(agentDrawnPosition(form, out, map, drawn)).toBe(true)
      expect(out.toArray()).toEqual([5, 30, 0])
    }
  })
})
