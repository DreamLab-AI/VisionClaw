/**
 * React bindings for the explore lane (ADR-2184, ADR-2185).
 *
 *  - `useExploreTableSource()` keeps `exploreStore`'s table in step with the graph
 *    load (graphDataManager) and the live analytics; mounted by ExplorePanel and
 *    GraphLegend, ref-counted so several mounts share one subscription.
 *  - `useExploreState()` re-renders on any table or clause change.
 *  - `useActiveFilterCount()` counts the active explore-panel filters.
 *  - `useExploreEncoding()` installs the column encoder for
 *    `visualisation.graphView.explore.colorBy` / `sizeBy` and returns the legend.
 */
import { useEffect, useMemo, useSyncExternalStore } from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { graphDataManager, type GraphData } from '../managers/graphDataManager';
import { nodeAnalyticsStore } from '../../analytics/store/nodeAnalyticsStore';
import { nodeEncoding as sharedEncoding, type NodeEncoding } from '../state/nodeEncoding';
import { exploreStore, type ExploreStore } from './exploreStore';
import { createColumnEncoding, type LegendModel } from './columnEncoder';

/** How often the live analytics columns (centrality, community, …) are re-read while mounted. */
export const ANALYTICS_REFRESH_MS = 5000;

// --- table source (ref-counted) ---------------------------------------------------

interface SourceState {
  refs: number;
  unsubscribe: (() => void) | null;
  timer: ReturnType<typeof setInterval> | null;
  nodes: GraphData['nodes'] | null;
  indexMap: Map<string, number>;
  analytics: Float32Array | null;
}

const source: SourceState = {
  refs: 0, unsubscribe: null, timer: null, nodes: null, indexMap: new Map(), analytics: null,
};

function sameFloats(a: Float32Array | null, b: Float32Array | null): boolean {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i] && !(Number.isNaN(a[i]) && Number.isNaN(b[i]))) return false;
  return true;
}

function rebuild(store: ExploreStore): void {
  if (!source.nodes) return;
  const a = nodeAnalyticsStore.getIndexedBuffer(source.indexMap);
  // keep a private copy: the store may reuse its buffer
  source.analytics = a ? a.slice() : null;
  store.setTable({
    nodes: source.nodes,
    indexMap: source.indexMap,
    analytics: a,
    nodeFilter: useSettingsStore.getState().settings?.nodeFilter,
  });
}

/**
 * graphDataManager emits only on a new graph load (setGraphData, hash-deduplicated;
 * positions arrive on a separate channel), so every emission rebuilds the table.
 * The index map is the order of `data.nodes`, which is how GraphManager builds the
 * shared render index (state/nodeIndex.ts).
 */
function onGraphData(data: GraphData, store: ExploreStore): void {
  source.nodes = data.nodes;
  source.indexMap = new Map(data.nodes.map((n, i) => [String(n.id), i]));
  rebuild(store);
}

function acquireSource(store: ExploreStore): () => void {
  if (source.refs++ === 0) {
    source.unsubscribe = graphDataManager.onGraphDataChange((d) => onGraphData(d, store));
    source.timer = setInterval(() => {
      // The store's one-slot cache is keyed by map identity and GemNodes reads it
      // every tick with the shared map, so compare contents, not identity.
      const a = source.nodes ? nodeAnalyticsStore.getIndexedBuffer(source.indexMap) : null;
      if (!sameFloats(a, source.analytics)) rebuild(store);
    }, ANALYTICS_REFRESH_MS);
  }
  return () => {
    if (--source.refs > 0) return;
    source.unsubscribe?.();
    if (source.timer) clearInterval(source.timer);
    source.unsubscribe = null;
    source.timer = null;
  };
}

/** Keep the explore table fed from the graph load while the caller is mounted. */
export function useExploreTableSource(store: ExploreStore = exploreStore): void {
  const includeLinkedPages = useSettingsStore((s) => s.settings?.nodeFilter?.includeLinkedPages ?? false);
  useEffect(() => acquireSource(store), [store]);
  // The population scope decides which rows count; rebuild when it changes.
  useEffect(() => { rebuild(store); }, [includeLinkedPages, store]);
}

// --- state hooks -----------------------------------------------------------------

export function useExploreState(store: ExploreStore = exploreStore) {
  const version = useSyncExternalStore(
    (cb) => store.subscribe(cb),
    () => store.version,
    () => store.version,
  );
  return { version, table: store.table, crossfilter: store.crossfilter, store };
}

/** Number of active explore-panel filters (one per widget with a clause). */
export function useActiveFilterCount(store: ExploreStore = exploreStore): number {
  return useSyncExternalStore(
    (cb) => store.subscribe(cb),
    () => store.activeCount(),
    () => store.activeCount(),
  );
}

// --- encoding --------------------------------------------------------------------

/**
 * Install the column encoder for the colorBy / sizeBy settings and return the
 * legend. Restores the default encoder on unmount or when both are default.
 *
 * `nodeEncoding.setEncoder` takes effect on the renderer's next `encodeAll`.
 */
export function useExploreEncoding(
  store: ExploreStore = exploreStore,
  encoding: NodeEncoding = sharedEncoding,
): LegendModel {
  const colorBy = useSettingsStore((s) => String(s.settings?.visualisation?.graphView?.explore?.colorBy ?? 'scheme'));
  const sizeBy = useSettingsStore((s) => s.settings?.visualisation?.graphView?.explore?.sizeBy ?? 'none');
  const { table } = useExploreState(store);
  const result = useMemo(() => createColumnEncoding(table, colorBy, sizeBy), [table, colorBy, sizeBy]);
  useEffect(() => {
    encoding.setEncoder(result.encoder);
    return () => { encoding.setEncoder(null); };
  }, [result, encoding]);
  return result.legend;
}
