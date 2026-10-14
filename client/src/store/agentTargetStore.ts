/**
 * agentTargetStore — tracks each agent's most recent working target node.
 *
 * Fed from the same 0x23 AGENT_ACTION binary path as transientBeamStore.
 * Consumers (BotsVisualization) read `targets` to nudge agent sprites toward
 * the KG node they are currently working on.
 */

import { create } from 'zustand';
import type { AgentActionEvent } from '../services/binaryProtocol/frameTypes';
import { isVirtualAgentId } from '../features/bots/virtualAgents';

interface AgentTargetState {
  /** agent wire id → most recent target KG node wire id. */
  targets: Map<number, number>;
  /** agent wire id → wall-clock ms of its most recent action (presence for node-less agents). */
  seenAt: Map<number, number>;
  /** Ingest a batch of 0x23 events; keeps only the newest per agent. */
  pushActions: (events: AgentActionEvent[]) => void;
  /** Drop a single agent (on despawn). */
  remove: (agentId: number) => void;
  /** Drop the shared-demo agents (reserved ids) when the demo stops. */
  clearVirtual: () => void;
  clear: () => void;
}

export const useAgentTargetStore = create<AgentTargetState>((set) => ({
  targets: new Map(),
  seenAt: new Map(),

  pushActions: (events) => {
    if (events.length === 0) return;
    const now = Date.now();
    set((state) => {
      const next = new Map(state.targets);
      const seen = new Map(state.seenAt);
      for (const e of events) {
        seen.set(e.sourceAgentId, now);
        if (e.targetNodeId > 0) {
          next.set(e.sourceAgentId, e.targetNodeId);
        }
      }
      return { targets: next, seenAt: seen };
    });
  },

  remove: (agentId) => {
    set((state) => {
      if (!state.targets.has(agentId) && !state.seenAt.has(agentId)) return state;
      const next = new Map(state.targets);
      next.delete(agentId);
      const seen = new Map(state.seenAt);
      seen.delete(agentId);
      return { targets: next, seenAt: seen };
    });
  },

  clearVirtual: () => {
    set((state) => {
      const next = new Map([...state.targets].filter(([id]) => !isVirtualAgentId(id)));
      const seen = new Map([...state.seenAt].filter(([id]) => !isVirtualAgentId(id)));
      return { targets: next, seenAt: seen };
    });
  },

  clear: () => set({ targets: new Map(), seenAt: new Map() }),
}));
