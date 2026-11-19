// REC-4 / ADR-043 resurrection (PRD-023 WP-8): the stateful four-KPI hook.
//
// Fetches `GET /api/kpi/summary` (which computes the two live KPIs from real
// source events, persists a snapshot, and fires CANARY-VC-REC4-KPI server-side),
// then normalises the response into render-ready tiles via the pure
// `normaliseKpiSummary`. Refreshes on an interval so the panel tracks the
// rolling window without a manual poke.

import { visibleInterval } from '../../../utils/visibleInterval';
import { useCallback, useEffect, useRef, useState } from 'react';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { createLogger } from '../../../utils/loggerConfig';
import { normaliseKpiSummary, type KpiTileView } from './kpiSummary';

const logger = createLogger('kpiSummary');

/** Refresh cadence — the KPI window is 30 days, so a slow poll is ample. */
const REFRESH_MS = 60_000;

export interface KpiSummaryState {
  tiles: KpiTileView[];
  loading: boolean;
  refresh: () => Promise<void>;
}

export function useKpiSummary(): KpiSummaryState {
  const [tiles, setTiles] = useState<KpiTileView[]>([]);
  const [loading, setLoading] = useState(true);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const raw = await unifiedApiClient.getData<unknown>('/kpi/summary');
      if (!mounted.current) return;
      setTiles(normaliseKpiSummary(raw));
    } catch (error) {
      // Fail-soft: an unavailable KPI endpoint leaves the last tiles rather than
      // crashing the control centre.
      logger.debug('kpi summary fetch skipped:', error);
    } finally {
      if (mounted.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    refresh();
    const stop = visibleInterval(refresh, REFRESH_MS);
    return () => {
      mounted.current = false;
      stop();
    };
  }, [refresh]);

  return { tiles, loading, refresh };
}
