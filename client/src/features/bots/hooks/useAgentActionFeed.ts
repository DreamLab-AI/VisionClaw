import { useCallback, useEffect, useRef, useState } from 'react';
import { useWebSocketStore } from '@/store/websocketStore';
import {
  AgentActionType,
  type AgentActionEvent,
} from '@/services/BinaryWebSocketProtocol';
import { parseActionPayload, isAgentStateFrame } from '@/services/binaryProtocol/agentActionPayload';

const DEFAULT_LIMIT = 200;

// Human verb per action type. Exported so every panel rendering the feed reads
// the same wording from one place.
export const AGENT_ACTION_VERBS: Record<AgentActionType, string> = {
  [AgentActionType.Query]: 'queried',
  [AgentActionType.Update]: 'updated',
  [AgentActionType.Create]: 'created',
  [AgentActionType.Delete]: 'deleted',
  [AgentActionType.Link]: 'linked',
  [AgentActionType.Transform]: 'transformed',
};

export function agentActionVerb(actionType: AgentActionType): string {
  return AGENT_ACTION_VERBS[actionType] ?? AgentActionType[actionType]?.toLowerCase() ?? `action#${actionType}`;
}

export interface AgentActionFeedEntry {
  /** Stable React key / dedupe id. */
  id: string;
  /** Event timestamp in ms; falls back to receive time when the wire value is 0. */
  ts: number;
  sourceAgentId: number;
  actionType: AgentActionType;
  /** Enum name, e.g. 'Query'. */
  actionTypeName: string;
  targetNodeId: number;
  /** Animation duration hint; omitted when the producer sends 0. */
  durationMs?: number;
  /** Declared intent text, when the optional payload carries it. */
  intent?: string;
  /** Post-action verification note, when the optional payload carries it. */
  verification?: string;
}

export interface UseAgentActionFeedOptions {
  /** Ring-buffer cap; oldest entries are dropped past this (default 200). */
  limit?: number;
  /** Subscribe to the live stream (default true). */
  enabled?: boolean;
}

export interface UseAgentActionFeedReturn {
  /** Newest first. */
  entries: AgentActionFeedEntry[];
  clear: () => void;
}

function toFeedEntry(event: AgentActionEvent, seq: number): AgentActionFeedEntry {
  const { intent, verification } = parseActionPayload(event.payload);
  return {
    id: `${event.sourceAgentId}-${event.timestamp}-${seq}`,
    ts: event.timestamp > 0 ? event.timestamp : Date.now(),
    sourceAgentId: event.sourceAgentId,
    actionType: event.actionType,
    actionTypeName: AgentActionType[event.actionType] ?? `Action#${event.actionType}`,
    targetNodeId: event.targetNodeId,
    durationMs: event.durationMs > 0 ? event.durationMs : undefined,
    intent,
    verification,
  };
}

/**
 * Live feed of 0x23 AGENT_ACTION events decoded off the binary WebSocket
 * (store/websocket/binaryProtocol.ts → emit('agent-action')). Holds a bounded,
 * newest-first ring buffer so panels can render the real action stream rather
 * than synthesising lines from polled status snapshots.
 */
export function useAgentActionFeed(
  options: UseAgentActionFeedOptions = {},
): UseAgentActionFeedReturn {
  const { limit = DEFAULT_LIMIT, enabled = true } = options;
  const [entries, setEntries] = useState<AgentActionFeedEntry[]>([]);
  const seqRef = useRef(0);
  const wsOn = useWebSocketStore(state => state.on);

  const clear = useCallback(() => {
    setEntries([]);
  }, []);

  useEffect(() => {
    if (!enabled) return;

    const unsubscribe = wsOn('agent-action', (data: unknown) => {
      const actions = data as AgentActionEvent[];
      if (!Array.isArray(actions) || actions.length === 0) return;

      // Wire order is chronological; reverse so the batch's newest lands first.
      // State frames are presence, not activity: the registry takes them, the transcript does not.
      const incoming = actions
        .filter(action => !isAgentStateFrame(action))
        .map(action => toFeedEntry(action, seqRef.current++))
        .reverse();

      if (incoming.length === 0) return;
      setEntries(prev => {
        const merged = incoming.concat(prev);
        return merged.length > limit ? merged.slice(0, limit) : merged;
      });
    });

    return unsubscribe;
  }, [enabled, wsOn, limit]);

  // Trim in place when the cap shrinks between renders.
  useEffect(() => {
    setEntries(prev => (prev.length > limit ? prev.slice(0, limit) : prev));
  }, [limit]);

  return { entries, clear };
}

export default useAgentActionFeed;
