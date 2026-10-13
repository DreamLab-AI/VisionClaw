import { useEffect, useState } from 'react';
import { analyticsAPI } from '../../../api/analyticsApi';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('useCyclicNodeIds');

/**
 * Ids (as strings, matching `node.id` in the renderer) of nodes on a directed
 * cycle, from the server's `GET /api/analytics/cycles`. Fetches only while
 * `enabled`, and again when the graph's shape changes. Returns `null` until a
 * report arrives or if the request fails, so callers fall back to normal colours.
 */
export function useCyclicNodeIds(
  enabled: boolean,
  nodeCount: number,
  edgeCount: number,
): Set<string> | null {
  const [ids, setIds] = useState<Set<string> | null>(null);

  useEffect(() => {
    if (!enabled) {
      setIds(null);
      return;
    }
    let cancelled = false;
    analyticsAPI
      .getCycles()
      .then((report) => {
        if (!cancelled) setIds(new Set(report.cyclicNodeIds.map(String)));
      })
      .catch((err) => {
        logger.warn('Cycle report unavailable:', err);
        if (!cancelled) setIds(null);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled, nodeCount, edgeCount]);

  return ids;
}
