/**
 * agentTargetStore — the client agent registry (ADR-2142, ADR-2135).
 *
 * Fed from the one 0x23 AGENT_ACTION dispatch (store/websocket/binaryProtocol.ts). Every wire id with live
 * evidence is an agent, whatever its range: the six demo roles, real swarm agents (0x8200_0000..), or an
 * agentbox id nothing else knows. Per agent it keeps its most recent work node (`targets`), when it was last
 * heard (`seenAt`, presence for node-less agents) and its lifecycle (`agents`: state and name).
 *
 * A *state frame* (payload `state`: working | done | retired) is presence plus lifecycle: it never makes a
 * beam. `retired` removes the agent at once; the 30 s presence window (`VIRTUAL_AGENT_TTL_MS`) is only the
 * backstop. An ordinary action frame means the agent is working. The marker pass reads this registry and is
 * the only writer of an agent's drawn position.
 */

import { create } from 'zustand';
import type { AgentActionEvent } from '../services/binaryProtocol/frameTypes';
import { parseActionPayload } from '../services/binaryProtocol/agentActionPayload';
import { isVirtualAgentId, agentWireId, VIRTUAL_AGENT_TTL_MS } from '../features/bots/virtualAgents';

export interface AgentInfo {
  /** `done` only on an explicit done state frame; any ordinary action clears it. */
  state: 'working' | 'done';
  /** Display name from the last state frame that carried one. */
  name?: string;
}

interface AgentTargetState {
  /** agent wire id → most recent target KG node wire id. */
  targets: Map<number, number>;
  /** agent wire id → wall-clock ms of its most recent frame (presence). */
  seenAt: Map<number, number>;
  /** agent wire id → lifecycle state and name. */
  agents: Map<number, AgentInfo>;
  /** Ingest a batch of 0x23 events (action and state frames); `now` is wall-clock ms. */
  pushActions: (events: AgentActionEvent[], now?: number) => void;
  /** Remove an agent at once (a `retired` state frame, or a despawn). */
  retire: (agentId: number) => void;
  /** Drop the shared-demo agents (reserved ids) when the demo stops. */
  clearVirtual: () => void;
  clear: () => void;
}

export const useAgentTargetStore = create<AgentTargetState>((set) => ({
  targets: new Map(),
  seenAt: new Map(),
  agents: new Map(),

  pushActions: (events, now = Date.now()) => {
    if (events.length === 0) return;
    set((state) => {
      const targets = new Map(state.targets);
      const seen = new Map(state.seenAt);
      const agents = new Map(state.agents);
      // Backstop: forget whoever has not been heard from within the presence window.
      for (const [id, t] of seen) {
        if (now - t >= VIRTUAL_AGENT_TTL_MS) { seen.delete(id); targets.delete(id); agents.delete(id); }
      }
      for (const e of events) {
        if (!Number.isFinite(e.sourceAgentId)) continue;
        const id = agentWireId(e.sourceAgentId);
        const p = parseActionPayload(e.payload);
        if (p.state === 'retired') {
          seen.delete(id); targets.delete(id); agents.delete(id);
          continue;
        }
        seen.set(id, now);
        if (e.targetNodeId > 0) targets.set(id, e.targetNodeId);
        const prev = agents.get(id);
        const name = p.name ?? prev?.name;
        agents.set(id, name === undefined ? { state: p.state ?? 'working' } : { state: p.state ?? 'working', name });
      }
      return { targets, seenAt: seen, agents };
    });
  },

  retire: (agentId) => {
    const id = agentWireId(agentId);
    set((state) => {
      if (!state.targets.has(id) && !state.seenAt.has(id) && !state.agents.has(id)) return state;
      const targets = new Map(state.targets); targets.delete(id);
      const seen = new Map(state.seenAt); seen.delete(id);
      const agents = new Map(state.agents); agents.delete(id);
      return { targets, seenAt: seen, agents };
    });
  },

  clearVirtual: () => {
    set((state) => {
      const keep = <V,>(m: Map<number, V>) => new Map([...m].filter(([id]) => !isVirtualAgentId(id)));
      return { targets: keep(state.targets), seenAt: keep(state.seenAt), agents: keep(state.agents) };
    });
  },

  clear: () => set({ targets: new Map(), seenAt: new Map(), agents: new Map() }),
}));
