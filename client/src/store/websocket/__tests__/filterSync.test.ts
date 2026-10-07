// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * filterSync regression suite.
 *
 * Defect: on every WebSocket connect the client sent NINE identical
 * `filter_update` messages — one from `onopen`, one from each of the seven
 * per-path settings subscriptions (the settings store's `subscribe` fires
 * immediately by default), and one from the whole-store subscription. The
 * server answers every `filter_update` with a full `initialGraphLoad`
 * (9,473 nodes / 144,674 edges, ~30 MB of JSON each), which starved the binary
 * position stream for tens of seconds. Because those sends also armed the
 * shrink-guard's filter-response window, the server's 3,000-node capped
 * connect-time load was accepted as a "filtered" answer and replaced the full
 * REST topology, so the next full position frame carried 6,473 unknown ids.
 */

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

vi.mock('../../../features/graph/managers/graphDataManager', () => ({
  graphDataManager: { setGraphData: vi.fn().mockResolvedValue(undefined) },
}));

type Listener = () => void;
const baseFilter = {
  enabled: false,
  qualityThreshold: 0.74,
  authorityThreshold: 0.5,
  filterByQuality: false,
  filterByAuthority: false,
  filterMode: 'or',
  includeLinkedPages: false,
};
let settingsState: { settings: { nodeFilter: typeof baseFilter; other?: number } };
let storeListeners: Listener[] = [];

vi.mock('../../settingsStore', () => ({
  useSettingsStore: {
    getState: () => ({
      ...settingsState,
      // Mirrors coreSlice.subscribe: immediate defaults to true and fires the
      // callback synchronously once the store is initialised.
      subscribe: (_path: string, cb: Listener, immediate = true) => {
        if (immediate) cb();
        return () => {};
      },
    }),
    subscribe: (listener: Listener) => {
      storeListeners.push(listener);
      return () => {
        storeListeners = storeListeners.filter(l => l !== listener);
      };
    },
  },
}));

import {
  setupFilterSubscription,
  resetFilterState,
  syncFilterToServer,
  expectFilterResponseFor,
  isFilterResponseExpected,
  clearFilterResponseExpectation,
} from '../filterSync';

function notifyStore() {
  storeListeners.forEach(l => l());
}

describe('filterSync', () => {
  let sendFilterUpdate: ReturnType<typeof vi.fn>;
  const get = () => ({ isConnected: true, sendFilterUpdate });

  beforeEach(() => {
    resetFilterState();
    clearFilterResponseExpectation();
    storeListeners = [];
    settingsState = { settings: { nodeFilter: { ...baseFilter } } };
    sendFilterUpdate = vi.fn();
  });

  it('sends exactly one filter_update per connect, not one per subscribed path', () => {
    // onopen: the per-connection server filter must be (re)established once.
    syncFilterToServer(get, { force: true });
    setupFilterSubscription(get);
    // Settings hydration and unrelated settings writes notify the store.
    notifyStore();
    settingsState = { settings: { ...settingsState.settings, other: 1 } };
    notifyStore();

    expect(sendFilterUpdate).toHaveBeenCalledTimes(1);
  });

  it('sends again when a filter field actually changes', () => {
    syncFilterToServer(get, { force: true });
    setupFilterSubscription(get);
    settingsState = {
      settings: { nodeFilter: { ...baseFilter, enabled: true, filterByQuality: true } },
    };
    notifyStore();
    notifyStore();

    expect(sendFilterUpdate).toHaveBeenCalledTimes(2);
    expect(sendFilterUpdate.mock.calls[1][0]).toMatchObject({ enabled: true, filterByQuality: true });
  });

  it('a forced sync resends an unchanged filter (reconnect re-establishes server state)', () => {
    syncFilterToServer(get, { force: true });
    syncFilterToServer(get, { force: true });
    syncFilterToServer(get);

    expect(sendFilterUpdate).toHaveBeenCalledTimes(2);
  });

  it('does not send while disconnected', () => {
    syncFilterToServer(() => ({ isConnected: false, sendFilterUpdate }), { force: true });
    expect(sendFilterUpdate).not.toHaveBeenCalled();
  });

  it('a disabled filter does not arm the shrink-guard acceptance window', () => {
    // The server answers a disabled filter with the FULL graph, so no smaller
    // load may be accepted on its behalf — the capped connect-time load would
    // otherwise clobber the full REST topology.
    expectFilterResponseFor({ ...baseFilter, enabled: false });
    expect(isFilterResponseExpected()).toBe(false);
  });

  it('an enabled filter arms the shrink-guard acceptance window', () => {
    expectFilterResponseFor({ ...baseFilter, enabled: true, filterByQuality: true });
    expect(isFilterResponseExpected()).toBe(true);
  });
});
