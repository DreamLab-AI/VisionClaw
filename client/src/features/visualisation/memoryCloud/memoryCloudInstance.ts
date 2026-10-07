/**
 * The app's memory explorer store, wired to the live API. The trajectory
 * module is imported lazily so the HNSW engine stays out of the main bundle
 * until the embedding cloud is switched on.
 */

import * as api from './api';
import { createMemoryCloudStore, type MemoryCloudDeps, type TrajectoryModule } from './memoryCloudStore';

export const defaultDeps: MemoryCloudDeps = {
  fetchSnapshot: api.fetchSnapshot,
  fetchVectors: api.fetchVectors,
  postQuery: api.postQuery,
  fetchHealth: api.fetchHealth,
  loadTrajectory: () => import('../memoryTrajectory') as unknown as Promise<TrajectoryModule>,
};

export const useMemoryCloudStore = createMemoryCloudStore(defaultDeps);
