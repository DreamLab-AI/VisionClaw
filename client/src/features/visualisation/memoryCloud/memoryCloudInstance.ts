/**
 * The app's memory explorer store, wired to the live API. The trajectory
 * module is imported lazily so the HNSW engine stays out of the main bundle
 * until the embedding cloud is switched on.
 *
 * The store's beat clock and query route are relayed to the same user's
 * headset over the graph socket (xrRelay.ts, ADR-2134).
 */

import * as api from './api';
import { createMemoryCloudStore, type MemoryCloudDeps, type TrajectoryModule } from './memoryCloudStore';
import { createXrRelay, type RelayFrame } from './xrRelay';
import { useWebSocketStore } from '@/store/websocketStore';

export const defaultDeps: MemoryCloudDeps = {
  fetchSnapshot: api.fetchSnapshot,
  fetchVectors: api.fetchVectors,
  postQuery: api.postQuery,
  fetchHealth: api.fetchHealth,
  loadTrajectory: () => import('../memoryTrajectory') as unknown as Promise<TrajectoryModule>,
};

export const useMemoryCloudStore = createMemoryCloudStore(defaultDeps);

/** Write one flat relay frame to the open graph socket; drop it while offline. */
export function sendRelayFrame(frame: RelayFrame): boolean {
  const ws = useWebSocketStore.getState();
  if (!ws.isConnected || !ws.socket || ws.socket.readyState !== WebSocket.OPEN) return false;
  try {
    ws.socket.send(JSON.stringify(frame));
    return true;
  } catch {
    return false;
  }
}

// Relay for the app's lifetime; not under test, so no timers leak into vitest.
const isTest = typeof import.meta !== 'undefined' && import.meta.env?.MODE === 'test';
if (!isTest) createXrRelay(useMemoryCloudStore, { send: sendRelayFrame });
