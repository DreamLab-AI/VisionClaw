/**
 * The analytics buffer layout (stride 5: cluster, anomaly, community, centrality,
 * sssp), as declared by `analytics/store/nodeAnalyticsStore.ts`.
 *
 * Re-declared here so the node-table worker can import the table builder without
 * pulling in nodeAnalyticsStore, whose module graph creates the settings store
 * (window / localStorage) and cannot load in a worker.
 * `__tests__/analyticsLayout.test.ts` pins these to the store's constants.
 */
export const ANALYTICS_STRIDE = 5;
export const ANALYTICS_CLUSTER_OFFSET = 0;
export const ANALYTICS_ANOMALY_OFFSET = 1;
export const ANALYTICS_COMMUNITY_OFFSET = 2;
export const ANALYTICS_CENTRALITY_OFFSET = 3;
