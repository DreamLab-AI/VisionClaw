/**
 * The render-index-aligned analytics buffer layout (ADR-031 D2): stride 5,
 * [clusterId, anomalyScore, communityId, centrality, ssspDistance] per node.
 *
 * The one declaration. It imports nothing, so the node-table worker can load
 * it; `nodeAnalyticsStore` (whose module graph creates the settings store and
 * needs `window`) builds its buffer with it, and every reader imports it here.
 */
export const ANALYTICS_STRIDE = 5;
export const ANALYTICS_CLUSTER_OFFSET = 0;
export const ANALYTICS_ANOMALY_OFFSET = 1;
export const ANALYTICS_COMMUNITY_OFFSET = 2;
export const ANALYTICS_CENTRALITY_OFFSET = 3;
export const ANALYTICS_SSSP_OFFSET = 4;
