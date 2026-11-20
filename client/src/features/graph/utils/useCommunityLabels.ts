/**
 * ADR-2206 — keeps the server's community labels fresh for the hull layer.
 *
 * Fetches when the partition signature the client holds changes (at most once per
 * `MIN_INTERVAL_MS`) and on a slow heartbeat. A response is never cancelled by a
 * re-render: ordering is `acceptLabels`' job (an older generation loses), so a
 * slow first compute still lands.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { acceptLabels, fetchCommunityLabels, type CommunityLabelsResponse } from './communityLabels';

export const MIN_INTERVAL_MS = 5_000;
export const HEARTBEAT_MS = 60_000;

export function useCommunityLabels(enabled: boolean, signature: string): CommunityLabelsResponse | null {
  const [labels, setLabels] = useState<CommunityLabelsResponse | null>(null);
  const lastFetchAt = useRef<number>(-Infinity);
  const mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; }, []);

  const refetch = useCallback(() => {
    lastFetchAt.current = Date.now();
    void fetchCommunityLabels().then((r) => {
      if (mounted.current) setLabels((cur) => acceptLabels(cur, r));
    });
  }, []);

  useEffect(() => {
    if (!enabled || !signature) return;
    const wait = Math.max(0, lastFetchAt.current + MIN_INTERVAL_MS - Date.now());
    const t = setTimeout(refetch, wait);
    return () => clearTimeout(t);
  }, [enabled, signature, refetch]);

  const hasPartition = signature !== '';
  useEffect(() => {
    if (!enabled || !hasPartition) return;
    const i = setInterval(refetch, HEARTBEAT_MS);
    return () => clearInterval(i);
  }, [enabled, hasPartition, refetch]);

  return labels;
}
