/**
 * agentWorkTargets.ts — Client-side tracker mapping agent node IDs to their
 * current work-target graph node, derived from 0x23 AGENT_ACTION frames via
 * the transientBeamStore. Drives the momentum-nudge (agents drift toward their
 * target) and fade-to-edge (idle agents fade and drift outward) behaviours in
 * the GemNodes instanced-mesh render loop.
 */

import { useTransientBeamStore } from '../../store/transientBeamStore';

const IDLE_THRESHOLD_MS = 15_000;

interface WorkEntry {
  targetNodeId: string;
  lastBeamStartMs: number;
}

const targets = new Map<string, WorkEntry>();
let lastProcessedBeamId = -1;
let unsub: (() => void) | null = null;

function ensureSubscribed(): void {
  if (unsub) return;
  unsub = useTransientBeamStore.subscribe(state => {
    for (const beam of state.beams) {
      if (beam.id <= lastProcessedBeamId) continue;
      lastProcessedBeamId = beam.id;
      const agentId = String(beam.sourceAgentId);
      const existing = targets.get(agentId);
      if (!existing || beam.startTime > existing.lastBeamStartMs) {
        targets.set(agentId, {
          targetNodeId: String(beam.targetNodeId),
          lastBeamStartMs: beam.startTime,
        });
      }
    }
  });
}

export type AgentWorkState = 'working' | 'done';

export interface AgentWorkInfo {
  targetNodeId: string;
  state: AgentWorkState;
}

export function getAgentWork(agentNodeId: string): AgentWorkInfo | null {
  ensureSubscribed();
  const entry = targets.get(agentNodeId);
  if (!entry) return null;
  const elapsed = performance.now() - entry.lastBeamStartMs;
  return {
    targetNodeId: entry.targetNodeId,
    state: elapsed < IDLE_THRESHOLD_MS ? 'working' : 'done',
  };
}

export const AGENT_DONE_ACTIVITY = 0.05;
