/**
 * transientBeamStore — ephemeral agent-action beams (0x23 AGENT_ACTION)
 *
 * The single websocket binary path decodes the 0x23 AGENT_ACTION frame in
 * `store/websocket/binaryProtocol.ts::handleAgentAction`. Decoded events are
 * pushed here via `pushBeams` (a non-React entry point, called from the frame
 * handler). The 3D layer (`TransientBeamsLayer`) subscribes to `beams`,
 * animates each beam's opacity over its `durationMs` TTL, and calls `prune
 * expired` once a beam's lifetime elapses.
 *
 * This store is deliberately small and parallel to the existing
 * `emit('agent-action', ...)` event, which now feeds only the live agent
 * transcript (features/bots/hooks/useAgentActionFeed.ts) — the legacy
 * ActionConnections renderer that also consumed it was removed 2026-07-15,
 * leaving TransientBeamsLayer as the sole action renderer. Pushing here never
 * interferes with the newest-wins position frame path.
 */

import { create } from 'zustand';
import type { AgentActionEvent, AgentActionType } from '../services/binaryProtocol/frameTypes';
import { isAgentStateFrame } from '../services/binaryProtocol/agentActionPayload';

/** Default beam lifetime when the frame's durationMs is zero/absent (ms). */
export const DEFAULT_BEAM_DURATION_MS = 1500;
/** Hard cap on concurrent beams. Oldest are evicted first (FIFO). */
export const MAX_TRANSIENT_BEAMS = 256;
/** Minimum lifetime so a near-instant action is still perceptible (ms). */
const MIN_BEAM_DURATION_MS = 400;

/** A single live beam awaiting render + TTL expiry. */
export interface TransientBeam {
  /** Stable per-beam identifier (monotonic counter). */
  id: number;
  /** Source agent id, exactly as received on the wire (may carry AGENT_NODE_FLAG). */
  sourceAgentId: number;
  /** Target KG node id, exactly as received on the wire. */
  targetNodeId: number;
  /** Action type — drives the colour map. */
  actionType: AgentActionType;
  /** Beam lifetime in ms (clamped). Opacity fades in → holds → fades out over this. */
  durationMs: number;
  /** performance.now() at the moment the beam was admitted to the store. */
  startTime: number;
}

interface TransientBeamState {
  beams: TransientBeam[];
  /** Admit a batch of decoded agent-action events as beams. */
  pushBeams: (events: AgentActionEvent[]) => void;
  /** Remove beams whose lifetime has fully elapsed (called from the render loop). */
  pruneExpired: () => void;
  /** Drop every beam (used on disconnect / scene teardown). */
  clear: () => void;
}

let beamIdCounter = 0;

function clampDuration(durationMs: number): number {
  if (!Number.isFinite(durationMs) || durationMs <= 0) {
    return DEFAULT_BEAM_DURATION_MS;
  }
  return Math.max(MIN_BEAM_DURATION_MS, durationMs);
}

export const useTransientBeamStore = create<TransientBeamState>((set, get) => ({
  beams: [],

  pushBeams: (events: AgentActionEvent[]) => {
    if (!events || events.length === 0) return;
    // A state frame (ADR-2142 / ADR-2135) is presence plus state: no beam, hence no heat or drift credit.
    events = events.filter(ev => !isAgentStateFrame(ev));
    if (events.length === 0) return;

    const now = performance.now();
    const incoming: TransientBeam[] = events.map(ev => ({
      id: beamIdCounter++,
      sourceAgentId: ev.sourceAgentId,
      targetNodeId: ev.targetNodeId,
      actionType: ev.actionType,
      durationMs: clampDuration(ev.durationMs),
      startTime: now,
    }));

    set(state => {
      const merged = state.beams.concat(incoming);
      // FIFO eviction once over the cap — oldest beams age out first.
      const trimmed = merged.length > MAX_TRANSIENT_BEAMS
        ? merged.slice(merged.length - MAX_TRANSIENT_BEAMS)
        : merged;
      return { beams: trimmed };
    });
  },

  pruneExpired: () => {
    const now = performance.now();
    const current = get().beams;
    if (current.length === 0) return;

    const alive = current.filter(b => now - b.startTime < b.durationMs);
    // Only commit a new array when something actually expired — avoids
    // gratuitous re-renders of the layer every frame.
    if (alive.length !== current.length) {
      set({ beams: alive });
    }
  },

  clear: () => {
    if (get().beams.length > 0) {
      set({ beams: [] });
    }
  },
}));

/**
 * Non-React push entry point for the websocket binary frame handler.
 * Lives outside the hook so the frame handler (which is not a component)
 * can feed decoded events without a React context.
 */
export function pushTransientBeams(events: AgentActionEvent[]): void {
  useTransientBeamStore.getState().pushBeams(events);
}
