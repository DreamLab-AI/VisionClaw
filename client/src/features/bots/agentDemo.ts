/**
 * The shared agents demo control (ADR-2142).
 *
 * The demo is produced once, on the server; this is only its remote control, shared by the
 * Agents pill and the control-centre button. The state shown is the SERVER's, polled, so a demo
 * started from the headset reads as running here and vice versa. Whatever the server decides
 * (including a refusal) is what is shown.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { unifiedApiClient } from '../../services/api/UnifiedApiClient';
import { useAgentTargetStore } from '../../store/agentTargetStore';

/** Relative to the API client's base, which already prefixes `/api`. */
export const DEMO_ENDPOINT = '/agents/demo';

export interface DemoStatus {
  running: boolean;
  /** The reserved agent ids while running. */
  agentIds: number[];
}

const parse = (raw: unknown): DemoStatus | null => {
  const b = ((raw as { data?: unknown } | null)?.data ?? raw) as Partial<DemoStatus> | null;
  if (!b || typeof b.running !== 'boolean') return null;
  return { running: b.running, agentIds: Array.isArray(b.agentIds) ? b.agentIds : [] };
};

export interface AgentDemo {
  running: boolean;
  /** The last refusal or failure, cleared on the next success. */
  error: string | null;
  busy: boolean;
  toggle: () => Promise<void>;
}

/** Poll the server's demo state and expose a toggle. Mount-scoped polling. */
export function useAgentDemo(pollMs = 4000): AgentDemo {
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const wasRunning = useRef(false);

  const adopt = useCallback((s: DemoStatus) => {
    // The demo just stopped (here or from the other client): its agents leave the screen now
    // rather than fading out over the 30 s presence window.
    if (wasRunning.current && !s.running) useAgentTargetStore.getState().clearVirtual();
    wasRunning.current = s.running;
    setRunning(s.running);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const s = parse(await unifiedApiClient.get<unknown>(DEMO_ENDPOINT, { timeout: 4000 }));
        if (!cancelled && s) adopt(s);
      } catch {
        /* unreachable: keep the last known state */
      }
    };
    void load();
    const id = window.setInterval(load, pollMs);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [pollMs, adopt]);

  const toggle = useCallback(async () => {
    setBusy(true);
    try {
      const res = await unifiedApiClient.post<unknown>(DEMO_ENDPOINT, { running: !wasRunning.current });
      const s = parse(res);
      if (s) adopt(s);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [adopt]);

  return { running, error, busy, toggle };
}
