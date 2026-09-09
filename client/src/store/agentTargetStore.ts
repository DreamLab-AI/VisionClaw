/**
 * agentTargetStore — tracks each agent's most recent working target node.
 *
 * Fed from the same 0x23 AGENT_ACTION binary path as transientBeamStore.
 * Consumers (BotsVisualization) read `targets` to nudge agent sprites toward
 * the KG node they are currently working on.
 */

import { create } from 'zustand';
import type { AgentActionEvent } from '../services/binaryProtocol/frameTypes';

interface AgentTargetState {
  /** agent wire id → most recent target KG node wire id. */
  targets: Map<number, number>;
  /** Ingest a batch of 0x23 events; keeps only the newest per agent. */
  pushActions: (events: AgentActionEvent[]) => void;
  /** Drop a single agent (on despawn). */
  remove: (agentId: number) => void;
  clear: () => void;
}

export const useAgentTargetStore = create<AgentTargetState>((set) => ({
  targets: new Map(),

  pushActions: (events) => {
    if (events.length === 0) return;
    set((state) => {
      const next = new Map(state.targets);
      for (const e of events) {
        if (e.targetNodeId > 0) {
          next.set(e.sourceAgentId, e.targetNodeId);
        }
      }
      return { targets: next };
    });
  },

  remove: (agentId) => {
    set((state) => {
      if (!state.targets.has(agentId)) return state;
      const next = new Map(state.targets);
      next.delete(agentId);
      return { targets: next };
    });
  },

  clear: () => set({ targets: new Map() }),
}));
