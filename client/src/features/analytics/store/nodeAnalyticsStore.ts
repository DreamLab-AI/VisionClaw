import { getActualNodeId } from '../../../types/binaryProtocol';
import type { BinaryNodeData } from '../../../types/binaryProtocol';
import { useSettingsStore } from '../../../store/settingsStore';

/**
 * Main-thread per-node analytics store (ADR-03 D7 "Phase 5", ADR-031 D2/D6).
 *
 * The graph worker no longer owns analytics. cluster_id / anomaly_score /
 * community_id / centrality / sssp_distance ride every V3 position frame
 * (wire offsets 36/40/44/48 plus the existing sssp_distance@28) and are decoded
 * on the main thread in the websocket receive path. This singleton ingests them
 * keyed by masked node id, and exposes a render-index-aligned Float32Array
 * (stride 5: [clusterId, anomalyScore, communityId, centrality, ssspDistance])
 * that GemNodes and ClusterHulls consume — exactly the contract the removed
 * graphWorkerProxy.getAnalyticsBuffer() used to satisfy.
 */

/** Stride of the render-index-aligned analytics buffer. */
export const ANALYTICS_STRIDE = 5;
export const ANALYTICS_CLUSTER_OFFSET = 0;
export const ANALYTICS_ANOMALY_OFFSET = 1;
export const ANALYTICS_COMMUNITY_OFFSET = 2;
export const ANALYTICS_CENTRALITY_OFFSET = 3;
export const ANALYTICS_SSSP_OFFSET = 4;

interface AnalyticsRecord {
  cluster: number;
  anomaly: number;
  community: number;
  centrality: number;
  /** SSSP distance from the active source; Infinity = unreachable / no run. */
  ssspDistance: number;
}

class NodeAnalyticsStore {
  private byMaskedId = new Map<number, AnalyticsRecord>();
  private version = 0;
  private hasAnalytics = false;

  // Render-index-aligned buffer cache (rebuilt on version/map change only).
  private cachedBuffer: Float32Array | null = null;
  private cachedMapRef: ReadonlyMap<string, number> | null = null;
  private cachedVersion = -1;
  private cachedSize = -1;

  /**
   * Ingest a decoded V3 frame. No-op (cheap) when no analytics consumer is
   * active and no analytics have ever been seen, so the common case (no
   * clustering run) costs only a settings read, not a full node scan.
   */
  ingest(nodes: BinaryNodeData[]): void {
    if (!this.hasAnalytics && !NodeAnalyticsStore.anyConsumerEnabled()) return;

    let sawAnalytics = false;
    for (const n of nodes) {
      const cluster = n.clusterId ?? 0;
      const anomaly = n.anomalyScore ?? 0;
      const community = n.communityId ?? 0;
      const centrality = n.centrality ?? 0;
      // sssp_distance@28 rides every V3 record; server feeds Infinity (LE all-FF
      // f32 ⇒ +Inf is not produced, server uses f32::MAX/sentinel) when no run.
      const ssspDistance = Number.isFinite(n.ssspDistance) ? n.ssspDistance : Infinity;
      if (cluster > 0 || community > 0 || anomaly > 0.0001 || centrality > 0.0001 || Number.isFinite(ssspDistance)) {
        sawAnalytics = true;
      }

      const key = getActualNodeId(n.nodeId);
      let t = this.byMaskedId.get(key);
      if (!t) {
        t = { cluster: 0, anomaly: 0, community: 0, centrality: 0, ssspDistance: Infinity };
        this.byMaskedId.set(key, t);
      }
      t.cluster = cluster;
      t.anomaly = anomaly;
      t.community = community;
      t.centrality = centrality;
      t.ssspDistance = ssspDistance;
    }

    if (sawAnalytics) this.hasAnalytics = true;
    this.version++;
  }

  hasData(): boolean {
    return this.hasAnalytics;
  }

  /**
   * Return a Float32Array indexed by render array position (the same index
   * space as nodePositionsRef / nodeIdToIndexMap), stride ANALYTICS_STRIDE (5:
   * [clusterId, anomalyScore, communityId, centrality, ssspDistance]). Returns
   * null when no analytics have been observed so callers transparently fall
   * back to their domain heuristics. Cached by (map identity, version) to keep
   * the per-tick call cheap.
   */
  getIndexedBuffer(nodeIdToIndexMap: ReadonlyMap<string, number>): Float32Array | null {
    if (!this.hasAnalytics || nodeIdToIndexMap.size === 0) return null;

    if (
      this.cachedBuffer &&
      this.cachedMapRef === nodeIdToIndexMap &&
      this.cachedVersion === this.version &&
      this.cachedSize === nodeIdToIndexMap.size
    ) {
      return this.cachedBuffer;
    }

    let maxIndex = 0;
    nodeIdToIndexMap.forEach((idx) => {
      if (idx > maxIndex) maxIndex = idx;
    });

    const buf = new Float32Array((maxIndex + 1) * ANALYTICS_STRIDE);
    nodeIdToIndexMap.forEach((idx, idStr) => {
      const numeric = parseInt(idStr, 10);
      if (Number.isNaN(numeric)) return;
      const t = this.byMaskedId.get(getActualNodeId(numeric));
      if (!t) return;
      const base = idx * ANALYTICS_STRIDE;
      buf[base + ANALYTICS_CLUSTER_OFFSET] = t.cluster;
      buf[base + ANALYTICS_ANOMALY_OFFSET] = t.anomaly;
      buf[base + ANALYTICS_COMMUNITY_OFFSET] = t.community;
      buf[base + ANALYTICS_CENTRALITY_OFFSET] = t.centrality;
      buf[base + ANALYTICS_SSSP_OFFSET] = t.ssspDistance;
    });

    this.cachedBuffer = buf;
    this.cachedMapRef = nodeIdToIndexMap;
    this.cachedVersion = this.version;
    this.cachedSize = nodeIdToIndexMap.size;
    return buf;
  }

  clear(): void {
    this.byMaskedId.clear();
    this.hasAnalytics = false;
    this.version++;
    this.cachedBuffer = null;
    this.cachedMapRef = null;
    this.cachedVersion = -1;
    this.cachedSize = -1;
  }

  /** Dev-only snapshot for diagnostics. */
  stats(): {
    hasAnalytics: boolean;
    nodeCount: number;
    clusteredCount: number;
    distinctClusters: number;
    version: number;
  } {
    let clusteredCount = 0;
    const distinct = new Set<number>();
    this.byMaskedId.forEach((t) => {
      if (t.cluster > 0) {
        clusteredCount++;
        distinct.add(t.cluster);
      }
    });
    return {
      hasAnalytics: this.hasAnalytics,
      nodeCount: this.byMaskedId.size,
      clusteredCount,
      distinctClusters: distinct.size,
      version: this.version,
    };
  }

  private static anyConsumerEnabled(): boolean {
    const s = useSettingsStore.getState().settings as
      | {
          qualityGates?: {
            showAnomalies?: boolean;
            showCentrality?: boolean;
            showSSSP?: boolean;
          };
          visualisation?: {
            clusterHulls?: { enabled?: boolean };
            graphs?: { knowledge?: { nodes?: { colorScheme?: string } } };
            graphView?: {
              labels?: { budget?: number };
              explore?: { colorBy?: string; sizeBy?: string };
            };
          };
        }
      | undefined;
    const qg = s?.qualityGates;
    // ADR-031 D6: an analytic colour scheme consumes the analytics buffer even
    // when every quality gate is off, so ingestion must stay live for it.
    const colorScheme = s?.visualisation?.graphs?.knowledge?.nodes?.colorScheme;
    const analyticColorScheme =
      colorScheme === 'community' ||
      colorScheme === 'cluster' ||
      colorScheme === 'centrality' ||
      colorScheme === 'sssp';
    // ADR-2189 retired Show Clusters / Show Communities (they coloured nothing;
    // showClusters defaulted on, which kept ingestion live for everything below).
    // The analytics buffer's real consumers are named instead: the ADR-2186 label
    // budget ranks labels by centrality, and the ADR-2184 explore encodings can
    // colour or size by an analytics column.
    const gv = s?.visualisation?.graphView;
    const labelBudgetOn = (gv?.labels?.budget ?? 40) > 0;
    const exploreColumn = (v?: string) => !!v && v !== 'scheme' && v !== 'none';
    return Boolean(
      labelBudgetOn ||
        exploreColumn(gv?.explore?.colorBy) ||
        exploreColumn(gv?.explore?.sizeBy) ||
        qg?.showAnomalies ||
        qg?.showCentrality ||
        qg?.showSSSP ||
        s?.visualisation?.clusterHulls?.enabled ||
        analyticColorScheme,
    );
  }
}

export const nodeAnalyticsStore = new NodeAnalyticsStore();

if (import.meta.env.DEV && typeof window !== 'undefined') {
  (window as unknown as { __nodeAnalyticsStore?: NodeAnalyticsStore }).__nodeAnalyticsStore =
    nodeAnalyticsStore;
}
