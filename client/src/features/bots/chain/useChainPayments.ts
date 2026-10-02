// Polls the sidechain payment view that rides on `/api/bots/data` (S5).
import { useEffect, useState } from 'react';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { ChainPaymentsView, chainViewFromBotsData } from './chainPayments';

/** Matches the server monitor's idle cadence. */
export const CHAIN_POLL_MS = 15_000;

export interface ChainPaymentsState {
  view: ChainPaymentsView | null;
  error: string | null;
}

export function useChainPayments(enabled = true): ChainPaymentsState {
  const [state, setState] = useState<ChainPaymentsState>({ view: null, error: null });

  useEffect(() => {
    if (!enabled) return undefined;
    let cancelled = false;
    const load = async () => {
      try {
        const raw = await unifiedApiClient.getData<unknown>('/bots/data');
        if (!cancelled) setState({ view: chainViewFromBotsData(raw), error: null });
      } catch (err) {
        if (!cancelled) {
          setState((prev) => ({ view: prev.view, error: err instanceof Error ? err.message : String(err) }));
        }
      }
    };
    void load();
    const timer = setInterval(load, CHAIN_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [enabled]);

  return state;
}
