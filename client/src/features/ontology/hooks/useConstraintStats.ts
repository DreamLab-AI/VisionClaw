/**
 * Polls the GPU-resident ontology constraint stats on an interval. Read-only:
 * surfaces what the live CUDA kernel is currently solving (PRD-018). Empty-safe
 * — degrades to zeros when the endpoint is unavailable.
 */

import { visibleInterval } from '../../../utils/visibleInterval';
import { useEffect, useRef, useState } from 'react';
import {
  fetchConstraintStats,
  EMPTY_CONSTRAINT_STATS,
  type ConstraintStats,
} from '../services/ontologyPhysicsService';

export interface UseConstraintStatsResult {
  stats: ConstraintStats;
  loading: boolean;
  /** Force an immediate refresh (e.g. after enable/disable/re-sync). */
  refresh: () => void;
}

/**
 * @param intervalMs polling cadence (default 5s). Pass 0 to fetch once, no poll.
 * @param enabled    gate polling (e.g. only when the panel is visible).
 */
export function useConstraintStats(intervalMs = 5000, enabled = true): UseConstraintStatsResult {
  const [stats, setStats] = useState<ConstraintStats>(EMPTY_CONSTRAINT_STATS);
  const [loading, setLoading] = useState(false);
  const tick = useRef(0);

  const load = async () => {
    setLoading(true);
    const next = await fetchConstraintStats();
    setStats(next);
    setLoading(false);
  };

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const run = async () => {
      if (cancelled) return;
      await load();
    };
    run();
    if (intervalMs > 0) {
      const stop = visibleInterval(run, intervalMs);
      return () => {
        cancelled = true;
        stop();
      };
    }
    return () => { cancelled = true; };
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, [intervalMs, enabled]);

  return { stats, loading, refresh: () => { tick.current++; void load(); } };
}
