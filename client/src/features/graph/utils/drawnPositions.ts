/**
 * The ONE drawn-positions array of the desktop client.
 *
 * Each frame GraphManager copies the worker's shared position buffer once into this array
 * and applies the agent drift and momentum nudge to the copy once. Everything that needs to
 * know where a node is drawn reads it through `nodePositionsRef`: the gems, the edge fill,
 * inferred edges, beams, the edge skip gate, labels, picking, fly-to, hulls and rings. No
 * consumer adds an offset of its own, so for any node one frame yields one position.
 *
 * The shared buffer is never written here. The worker rewrites it on its own 16 ms timer,
 * so a main-thread `+=` would be applied twice on any frame the worker skipped (and would
 * race the worker); writing the copy cannot.
 */
import { getAgentWork, type AgentWorkInfo } from '../../bots/agentWorkTargets'
import { agentDriftFeed } from '../../bots/agentDriftFeed'
import { agentOffsetsEnabled } from '../../../store/agentOffsetPrefs'
import { stepAgentOffset } from './agentNudge'
import type { TriangleFrame } from '../triLayout'

/** One drawn agent: its graph node id (wire id string, flag bit included) and its slot in the buffer. */
export interface AgentSlot {
  id: string
  index: number
}

/** The separated-layout activity drift (`agentDriftFeed`), as the pass needs it. */
export interface DriftSource {
  step(): void
  offset(agentKey: string, frame: TriangleFrame): readonly [number, number, number]
}

/** Everything the agent pass reads. Dependencies are injected so tests can drive it. */
export interface AgentPassInput {
  /** The shared (worker-owned) buffer: server positions, the base of every agent's motion. */
  sab: Float32Array
  agents: readonly AgentSlot[]
  getWork: (agentId: string) => AgentWorkInfo | null
  drift: DriftSource | null
  triFrame: TriangleFrame
  /** The "agent offsets" preference; off draws an agent at its server position plus drift only. */
  offsetsEnabled: boolean
  /** Buffer index of the node being dragged (the gem takes the hand position), or -1. */
  dragIndex: number
  /** Graph node id to buffer index, to find a working agent's target. */
  nodeIdToIndex: Map<string, number>
}

interface AgentState {
  off: Float32Array
  seen: number
}

const _base: [number, number, number] = [0, 0, 0]
const _target: [number, number, number] = [0, 0, 0]

/**
 * The agents that have a slot in the position buffer. An agent not yet in the map has no slot,
 * so it is not drawn at, or offset into, some other node's slot.
 */
export function resolveAgentSlots(
  nodes: ReadonlyArray<{ id: string | number }>,
  nodeIdToIndex: Map<string, number>,
): AgentSlot[] {
  const out: AgentSlot[] = []
  for (const n of nodes) {
    const id = String(n.id)
    const index = nodeIdToIndex.get(id)
    if (index !== undefined) out.push({ id, index })
  }
  return out
}

export class DrawnPositions {
  private drawn = new Float32Array(0)
  /** Per-agent nudge offset, keyed by agent id so a reorder or a reused slot cannot cross-wire them. */
  private agentState = new Map<string, AgentState>()
  private passNo = 0

  /** The drawn positions. Exactly as long as the shared buffer, so every length check stays honest. */
  get positions(): Float32Array {
    return this.drawn
  }

  /** Copy this frame's shared buffer in. Reallocates only when its length changes. */
  copy(sab: Float32Array): Float32Array {
    if (this.drawn.length !== sab.length) this.drawn = new Float32Array(sab.length)
    this.drawn.set(sab)
    return this.drawn
  }

  /**
   * Move each agent to server position + activity drift + nudge, in the copy. Call once a frame,
   * after `copy`. Agents that are gone lose their offset, so the slot is raw again and a new
   * occupant starts from zero.
   */
  applyAgents(input: AgentPassInput): void {
    const { sab, agents } = input
    const drawn = this.drawn
    if (agents.length === 0) {
      this.agentState.clear()
      return
    }
    const pass = ++this.passNo
    input.drift?.step()
    for (let a = 0; a < agents.length; a++) {
      const { id, index } = agents[a]
      const i3 = index * 3
      if (i3 + 2 >= drawn.length || i3 + 2 >= sab.length) continue
      let st = this.agentState.get(id)
      if (st === undefined) {
        st = { off: new Float32Array(3), seen: pass }
        this.agentState.set(id, st)
      }
      st.seen = pass
      if (index === input.dragIndex) continue

      let x = sab[i3], y = sab[i3 + 1], z = sab[i3 + 2]
      // Separated layout (ADR-2135): the agent's home is the triangle centroid plus its
      // activity drift; the nudge works from there.
      if (input.drift) {
        const d = input.drift.offset(id, input.triFrame)
        x += d[0]; y += d[1]; z += d[2]
      }
      const off = st.off
      if (input.offsetsEnabled) {
        const work = input.getWork(id)
        let target: [number, number, number] | null = null
        if (work && work.state === 'working') {
          const t3 = (input.nodeIdToIndex.get(work.targetNodeId) ?? -1) * 3
          if (t3 >= 0 && t3 + 2 < sab.length) {
            _target[0] = sab[t3]; _target[1] = sab[t3 + 1]; _target[2] = sab[t3 + 2]
            target = _target
          }
        }
        _base[0] = x; _base[1] = y; _base[2] = z
        stepAgentOffset(off, 0, _base, target, work?.state ?? null)
      } else {
        off[0] = 0; off[1] = 0; off[2] = 0
      }
      drawn[i3] = x + off[0]
      drawn[i3 + 1] = y + off[1]
      drawn[i3 + 2] = z + off[2]
    }
    // Agents that were not in this pass (filtered, replaced, swarm emptied) free their offset.
    if (this.agentState.size > agents.length) {
      for (const [id, st] of this.agentState) if (st.seen !== pass) this.agentState.delete(id)
    }
  }
}

/** The shared instance GraphManager drives; tests build their own. */
export const drawnPositions = new DrawnPositions()

/** The `applyAgents` input GraphManager builds from the live stores (the one place they are read). */
export function liveAgentPassDeps(): Pick<AgentPassInput, 'getWork' | 'drift' | 'offsetsEnabled'> {
  return { getWork: getAgentWork, drift: agentDriftFeed(), offsetsEnabled: agentOffsetsEnabled() }
}

/** Copy the first `count` drawn positions into the label array (`labelPositionsRef`). */
export function fillLabelPositions(
  drawn: Float32Array,
  count: number,
  out: Array<{ x: number; y: number; z: number }>,
): void {
  for (let i = 0; i < count; i++) {
    const i3 = i * 3
    out[i].x = drawn[i3]
    out[i].y = drawn[i3 + 1]
    out[i].z = drawn[i3 + 2]
  }
}
