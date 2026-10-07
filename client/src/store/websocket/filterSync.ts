/**
 * filterSync.ts — Client filter state synchronization
 *
 * Handles: filter subscription, filter update messages, node filter
 * settings sync over WebSocket.
 */

import { createLogger } from '../../utils/loggerConfig';
import { useSettingsStore } from '../settingsStore';
import { graphDataManager } from '../../features/graph/managers/graphDataManager';
import type { FilterUpdateParams, FilterSnapshot } from './types';

const logger = createLogger('WebSocketStore');

// ── Encapsulated module-level state ────────────────────────────────────
let filterSubscriptionSet = false;
let filterUnsubscribers: (() => void)[] = [];
let lastFilterSnapshot: FilterSnapshot | null = null;

// ── Filter-response expectation window ─────────────────────────────────
//
// After the client sends a filter_update, the server answers with a
// per-client initialGraphLoad carrying the FILTERED (usually smaller) node
// set. handleInitialGraphLoad's shrink-guard — which exists to stop the
// 200-node connect-time payload from clobbering a full REST load — must NOT
// swallow that response, or the quality gates appear to do nothing (the
// long-standing break). The guard consults this window: a smaller load is
// authoritative while a filter response is pending.
const FILTER_RESPONSE_WINDOW_MS = 15_000;
let filterResponseExpectedUntil = 0;

export function expectFilterResponse() {
  filterResponseExpectedUntil = Date.now() + FILTER_RESPONSE_WINDOW_MS;
}

export function isFilterResponseExpected(): boolean {
  return Date.now() < filterResponseExpectedUntil;
}

export function clearFilterResponseExpectation() {
  filterResponseExpectedUntil = 0;
}

// ── State accessors (used by index.ts for _reset) ──

export function resetFilterState() {
  filterUnsubscribers.forEach(unsub => { try { unsub(); } catch (_) { /* ignore */ } });
  filterUnsubscribers = [];
  filterSubscriptionSet = false;
  lastFilterSnapshot = null;
}

export function cleanupFilterSubscriptions() {
  filterUnsubscribers.forEach(unsub => { try { unsub(); } catch (_) { /* ignore */ } });
  filterUnsubscribers = [];
  filterSubscriptionSet = false;
}

export function clearFilterSnapshot() {
  lastFilterSnapshot = null;
}

// ── Filter sync (single, de-duplicated send path) ─────────────────────
//
// Every `filter_update` costs the server a full graph read and costs the
// client a full `initialGraphLoad` (~30 MB of JSON at 9.5k nodes / 145k
// edges). The send path is therefore de-duplicated against the last snapshot
// actually sent: only a real change to a filter field, or a forced sync on
// (re)connect — the server keeps the filter per connection — reaches the wire.

type FilterSyncGet = () => {
  isConnected: boolean;
  sendFilterUpdate: (filter: FilterUpdateParams) => void;
};

function currentFilterSnapshot(): FilterSnapshot | null {
  const nodeFilter = useSettingsStore.getState().settings?.nodeFilter;
  if (!nodeFilter) return null;
  return {
    enabled: nodeFilter.enabled,
    qualityThreshold: nodeFilter.qualityThreshold,
    authorityThreshold: nodeFilter.authorityThreshold,
    filterByQuality: nodeFilter.filterByQuality,
    filterByAuthority: nodeFilter.filterByAuthority,
    filterMode: nodeFilter.filterMode,
    includeLinkedPages: nodeFilter.includeLinkedPages,
  };
}

function sameSnapshot(a: FilterSnapshot | null, b: FilterSnapshot): boolean {
  return (
    !!a &&
    a.enabled === b.enabled &&
    a.qualityThreshold === b.qualityThreshold &&
    a.authorityThreshold === b.authorityThreshold &&
    a.filterByQuality === b.filterByQuality &&
    a.filterByAuthority === b.filterByAuthority &&
    a.filterMode === b.filterMode &&
    a.includeLinkedPages === b.includeLinkedPages
  );
}

/**
 * Send the current node filter to the server if it differs from the last one
 * sent. `force` resends an unchanged filter (used on connect, because a new
 * connection starts with the server's default filter).
 */
export function syncFilterToServer(get: FilterSyncGet, opts: { force?: boolean } = {}) {
  const wsState = get();
  if (!wsState.isConnected) return;
  const snapshot = currentFilterSnapshot();
  if (!snapshot) return;
  if (!opts.force && sameSnapshot(lastFilterSnapshot, snapshot)) return;
  lastFilterSnapshot = snapshot;
  wsState.sendFilterUpdate(snapshot);
}

/**
 * Arm the shrink-guard acceptance window only when the filter can NARROW the
 * graph. A disabled filter is answered with the full graph, so nothing smaller
 * may be accepted on its behalf — otherwise the capped connect-time
 * initialGraphLoad would replace a full REST topology.
 */
export function expectFilterResponseFor(filter: Pick<FilterUpdateParams, 'enabled'>) {
  if (filter.enabled) {
    expectFilterResponse();
  }
}

// ── Filter subscription setup ──────────────────────────────────────────

export function setupFilterSubscription(get: FilterSyncGet) {
  if (filterSubscriptionSet) return;
  filterSubscriptionSet = true;

  // One whole-store subscription, de-duplicated by syncFilterToServer. The
  // former seven per-path subscriptions fired immediately on registration
  // (the settings store's `subscribe` defaults to immediate) and each sent an
  // undeduplicated filter_update — nine full-graph round trips per connect.
  const zustandUnsub = useSettingsStore.subscribe(() => {
    syncFilterToServer(get);
  });
  filterUnsubscribers.push(zustandUnsub);

  logger.info('Filter subscription set up - changes will sync to server');
}

// ── Force refresh ──────────────────────────────────────────────────────

export async function forceRefreshFilter(get: FilterSyncGet) {
  const state = get();
  if (!state.isConnected) {
    logger.warn('Cannot force refresh filter: WebSocket not connected');
    return;
  }

  const nodeFilter = useSettingsStore.getState().settings?.nodeFilter;
  if (nodeFilter) {
    logger.info('[Refresh] Clearing local graph and requesting fresh filtered data', nodeFilter);

    await graphDataManager.setGraphData({ nodes: [], edges: [] });
    logger.info('[Refresh] Local graph cleared, awaiting server response...');

    syncFilterToServer(get, { force: true });
  } else {
    logger.warn('No nodeFilter settings found in store');
  }
}
