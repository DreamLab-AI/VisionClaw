/**
 * The agent marker pass: the ONE writer of where an agent is drawn (ADR-2142, ADR-2135).
 *
 * Agents are not graph nodes. Once a frame, after GraphManager has copied the drawn-positions array, this
 * pass walks the registry (`agentTargetStore`) and fills `positions`, a map from agent wire id to world
 * position. Everything that draws or starts from an agent reads that map and nothing else: the marker
 * spheres, the beams' source point, the nameplate, the trail, the collaboration edges and the payment labels.
 *
 * An agent's point is
 *   base   the hover point beside its work node (an id-hashed ring), or around the scene centre when it has
 *          no work node on record
 * + drift  the separated-layout activity drift (`agentDriftFeed`, keyed `String(wireId)`)
 * + run    the finished-agent run to the scene edge, `stepAgentOffset` (30 s, then it holds).
 *
 * R3: a working agent is stepped with no target and no state, so only the relax branch acts: its base is
 * already the hover point beside the node, and a working nudge would pull the marker (and the beam source)
 * into the node. The run applies only to the registry's explicit `done` state; idle time never produces it.
 * `stepAgentOffset` stays the single implementation of the run.
 */
import { Vector3 } from 'three'
import { stepAgentOffset, newAgentFinish, type AgentFinish, type SceneEdge, type Vec3 } from '../graph/utils/agentNudge';
import type { TriangleFrame } from '../graph/triLayout';
import { agentDriftFeed } from './agentDriftFeed';
import { agentOffsetsEnabled } from '../../store/agentOffsetPrefs';
import { useAgentTargetStore } from '../../store/agentTargetStore';
import {
  agentHoverOffset, agentWireId, livePresence, VIRTUAL_HOVER_LIFT, VIRTUAL_HOVER_RADIUS,
} from './virtualAgents';

export interface MarkerVec { x: number; y: number; z: number }

/** The separated-layout activity drift (`agentDriftFeed`), as the pass needs it. */
export interface DriftSource {
  step(): void
  offset(agentKey: string, frame: TriangleFrame): readonly [number, number, number]
}

/** The registry maps the pass reads. */
export interface MarkerRegistry {
  targets: ReadonlyMap<number, number>
  seenAt: ReadonlyMap<number, number>
  agents: ReadonlyMap<number, { state: 'working' | 'done' }>
}

/** Everything the pass reads. Dependencies are injected so tests can drive it. */
export interface MarkerPassInput {
  registry: MarkerRegistry
  /** The viewer's own node-position lookup (the drawn array); false when the node is not on screen. */
  resolveNode: (nodeId: number, out: Vector3) => boolean
  drift: DriftSource | null
  triFrame: TriangleFrame
  /** The "agent offsets" preference; off draws a marker at its hover point plus drift only. */
  offsetsEnabled: boolean
  /** The scene's edge (the sphere the camera frames), where finished agents stop; null if unknown. */
  edge: SceneEdge | null
  /** The frame clock in ms (`performance.now()`); the run to the edge runs on it. */
  nowMs: number
  /** Wall-clock ms (`Date.now()`); presence runs on it. */
  wallMs: number
}

interface PassState {
  off: Float32Array
  finish: AgentFinish
  seen: number
}

const _base: [number, number, number] = [0, 0, 0]
const _node = new Vector3()

export class AgentMarkerPass {
  /** agent wire id (flag included) → world position. Written only by `step`. */
  readonly positions = new Map<number, MarkerVec>()
  private state = new Map<number, PassState>()
  private passNo = 0

  step(input: MarkerPassInput): void {
    const { registry } = input
    const pass = ++this.passNo
    const centre: Vec3 = input.edge?.centre ?? [0, 0, 0]
    let any = false
    for (const [id] of registry.seenAt) {
      if (!livePresence(id, registry.seenAt, input.wallMs)) continue
      const ring = agentHoverOffset(id, VIRTUAL_HOVER_RADIUS, VIRTUAL_HOVER_LIFT)
      const target = registry.targets.get(id)
      let bx: number, by: number, bz: number
      if (target === undefined) {
        bx = centre[0]; by = centre[1]; bz = centre[2]
      } else if (input.resolveNode(target, _node)) {
        bx = _node.x; by = _node.y; bz = _node.z
      } else {
        continue // its work node is not on screen: the marker is hidden, not drawn elsewhere
      }
      bx += ring.x; by += ring.y; bz += ring.z
      if (!any) { any = true; input.drift?.step() }
      if (input.drift) {
        const d = input.drift.offset(String(id), input.triFrame)
        bx += d[0]; by += d[1]; bz += d[2]
      }
      let st = this.state.get(id)
      if (st === undefined) {
        st = { off: new Float32Array(3), finish: newAgentFinish(), seen: pass }
        this.state.set(id, st)
      }
      st.seen = pass
      const off = st.off
      if (input.offsetsEnabled) {
        _base[0] = bx; _base[1] = by; _base[2] = bz
        const done = registry.agents.get(id)?.state === 'done'
        stepAgentOffset(off, 0, _base, null, done ? 'done' : null, input.edge, st.finish, input.nowMs)
      } else {
        off[0] = 0; off[1] = 0; off[2] = 0
        // Offsets off: the 30 s run is paused, not elapsed, so it restarts when they come back.
        st.finish.since = -1
      }
      let p = this.positions.get(id)
      if (p === undefined) { p = { x: 0, y: 0, z: 0 }; this.positions.set(id, p) }
      p.x = bx + off[0]; p.y = by + off[1]; p.z = bz + off[2]
    }
    // Retired, expired, hidden: out of the map and out of the state, so a reused id starts from zero.
    if (this.positions.size > 0 || this.state.size > 0) {
      for (const [id, st] of this.state) {
        if (st.seen !== pass) { this.state.delete(id); this.positions.delete(id) }
      }
      for (const id of this.positions.keys()) if (!this.state.has(id)) this.positions.delete(id)
    }
  }

  /** Write the agent's marker position into `out`; false (out untouched) when it has none. */
  position(id: number | string, out: { set(x: number, y: number, z: number): unknown }): boolean {
    const p = this.positions.get(agentWireId(id))
    if (!p) return false
    out.set(p.x, p.y, p.z)
    return true
  }

  clear(): void {
    this.positions.clear()
    this.state.clear()
  }
}

/** The shared instance GraphManager drives; tests build their own. */
export const agentMarkers = new AgentMarkerPass()

/** Where an agent's marker is, by wire id (flagged or masked); the beam source and every overlay read this. */
export function agentMarkerPosition(id: number | string, out: { set(x: number, y: number, z: number): unknown }): boolean {
  return agentMarkers.position(id, out)
}

/** The live stores the pass reads, in the one place they are read. */
export function liveMarkerDeps(): Pick<MarkerPassInput, 'registry' | 'drift' | 'offsetsEnabled'> {
  return { registry: useAgentTargetStore.getState(), drift: agentDriftFeed(), offsetsEnabled: agentOffsetsEnabled() }
}
