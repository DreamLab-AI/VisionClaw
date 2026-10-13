import { useEffect, useState } from 'react';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { parsePhysicsState, type PhysicsState } from './physicsState';

const ENDPOINT = '/api/graph/settlement';

/**
 * Poll the server's physics state while the caller is mounted (the flyout, which only
 * mounts when expanded, so the collapsed chip stays poll-free). Returns null until the
 * first answer and again after a failed one, so "unreachable" is never shown as stale data.
 */
export function usePhysicsState(intervalMs = 3000): PhysicsState | null {
  const [state, setState] = useState<PhysicsState | null>(null);
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const res = await unifiedApiClient.get<unknown>(ENDPOINT, { timeout: 4000 });
        if (!cancelled) setState(parsePhysicsState(res.data));
      } catch {
        if (!cancelled) setState(null);
      }
    };
    void load();
    const id = window.setInterval(load, intervalMs);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [intervalMs]);
  return state;
}
